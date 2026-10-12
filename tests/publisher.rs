use base64::{Engine, engine::general_purpose::STANDARD};
use nostr::{Event, JsonUtil, Keys};
use openwebui_acp::{
    buzz::{Destination, Publisher},
    config::{BuzzConfig, Secret},
    error::{Error, PublishFailure},
    state::{DeliveryStatus, Scope, Store},
};
use tokio_util::sync::CancellationToken;
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{method, path},
};

fn config(url: &str) -> (BuzzConfig, String) {
    let requester = Keys::generate().public_key().to_hex();
    let config = BuzzConfig {
        relay: url.parse().unwrap(),
        private_key: Secret::new(Keys::generate().secret_key().to_secret_hex()),
        auth_tag: None,
        allowed_authors: vec![requester.clone()],
        allowed_channels: vec!["12345678-1234-4234-8234-123456789abc".parse().unwrap()],
    };
    (config, requester)
}
fn destination(author: &str) -> Destination {
    Destination {
        channel: "12345678-1234-4234-8234-123456789abc".parse().unwrap(),
        reply_to: "a".repeat(64),
        event_id: "b".repeat(64),
        author: Some(author.into()),
    }
}

#[tokio::test]
async fn sdk_reply_preserves_text_and_only_explicit_recipients_and_signs_http_auth() {
    let server = MockServer::start().await;
    let (config, author) = config(&format!("{}/", server.uri()));
    let publisher = Publisher::new(config).unwrap();
    let d = destination(&author);
    let expected_url = format!("{}/events", server.uri());
    Mock::given(method("POST"))
        .and(path("/events"))
        .respond_with(move |r: &Request| {
            let event: Event = serde_json::from_slice(&r.body).unwrap();
            event.verify().unwrap();
            let auth = r
                .headers
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap()
                .strip_prefix("Nostr ")
                .unwrap();
            let auth = Event::from_json(STANDARD.decode(auth).unwrap()).unwrap();
            auth.verify().unwrap();
            assert_eq!(auth.kind.as_u16(), 27235);
            assert_eq!(auth.pubkey, event.pubkey);
            use sha2::{Digest, Sha256};
            let digest = format!("{:x}", Sha256::digest(&r.body));
            assert!(
                auth.tags
                    .iter()
                    .any(|t| t.as_slice() == ["payload", digest.as_str()])
            );
            assert!(
                auth.tags
                    .iter()
                    .any(|t| t.as_slice() == ["u", expected_url.as_str()])
            );
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"event_id":event.id.to_hex(),"accepted":true}))
        })
        .mount(&server)
        .await;
    let text = "Draft: @Someone Else and nostr:npub1untrusted are literal. `$(echo nope)`\n```sh\n$HOME\n```";
    let event = publisher.sign_reply(&d, text).unwrap();
    assert_eq!(event.content, text);
    let p = event
        .tags
        .iter()
        .filter(|t| t.as_slice().first().is_some_and(|s| s == "p"))
        .collect::<Vec<_>>();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].as_slice(), ["p", author.as_str()]);
    assert!(
        !event
            .tags
            .iter()
            .any(|t| t.as_slice().first().is_some_and(|s| s == "broadcast"))
    );
    publisher
        .publish(&d, &event, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    let mut denied = d.clone();
    denied.author = Some(Keys::generate().public_key().to_hex());
    assert!(matches!(
        publisher.sign_reply(&denied, text),
        Err(Error::BuzzAuthor)
    ));
    denied = d.clone();
    denied.channel = uuid::Uuid::new_v4();
    assert!(matches!(
        publisher
            .publish(&denied, &event, &CancellationToken::new())
            .await,
        Err(Error::BuzzAuthor)
    ));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn durable_outbox_retries_identical_event_after_lost_acknowledgement() {
    let server = MockServer::start().await;
    let (config, author) = config(&format!("{}/", server.uri()));
    let publisher = Publisher::new(config.clone()).unwrap();
    let d = destination(&author);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state");
    let scope = Scope {
        endpoint: "https://webui.example/".into(),
        model: "example".into(),
        relay: Some(config.relay.to_string()),
        agent: Some(publisher.public_key()),
    };
    let store = Store::open(Some(path.clone()), scope.clone()).unwrap();
    store
        .reserve(d.clone(), "thread".into(), "operation".into())
        .unwrap();
    let event = publisher.sign_reply(&d, "PRIVATE_PENDING_REPLY").unwrap();
    store
        .set_delivery(
            &d.key(),
            DeliveryStatus::Pending {
                event: event.clone(),
            },
        )
        .unwrap();
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("lost/invalid acknowledgement SECRET_PAYLOAD"),
        )
        .mount(&server)
        .await;
    let error = publisher
        .publish(&d, &event, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(!error.to_string().contains("SECRET_PAYLOAD"));
    assert!(matches!(
        error,
        Error::Publish(PublishFailure::Acknowledgement)
    ));
    drop(store);
    drop(publisher);
    let store = Store::open(Some(path.clone()), scope).unwrap();
    let publisher = Publisher::new(config).unwrap();
    let record = store.event(&d.key()).unwrap().unwrap();
    let DeliveryStatus::Pending { event: recovered } = record.status else {
        panic!("lost pending event")
    };
    assert_eq!(recovered.id, event.id);
    Mock::given(method("POST")).respond_with(move |r:&Request| {
        let e:Event=serde_json::from_slice(&r.body).unwrap();ResponseTemplate::new(200).set_body_json(serde_json::json!({"event_id":e.id.to_hex(),"accepted":true,"message":"duplicate:"}))
    }).with_priority(1).mount(&server).await;
    publisher
        .publish(&d, &recovered, &CancellationToken::new())
        .await
        .unwrap();
    store
        .set_delivery(
            &d.key(),
            DeliveryStatus::Delivered {
                event_id: recovered.id.to_hex(),
            },
        )
        .unwrap();
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body, requests[1].body);
    assert_ne!(
        requests[0].headers["authorization"],
        requests[1].headers["authorization"]
    );
    assert!(
        !std::fs::read_to_string(path.join("state.json"))
            .unwrap()
            .contains("PRIVATE_PENDING_REPLY")
    );
}

#[tokio::test]
async fn transient_retries_are_bounded_and_cancelled_publication_sends_nothing() {
    let server = MockServer::start().await;
    let (config, author) = config(&format!("{}/", server.uri()));
    let p = Publisher::new(config).unwrap();
    let d = destination(&author);
    let e = p.sign_reply(&d, "reply").unwrap();
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        p.publish(&d, &e, &cancel).await,
        Err(Error::Cancelled)
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
    assert!(p.publish(&d, &e, &CancellationToken::new()).await.is_err());
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|r| r.body == requests[0].body));
}
