mod common;
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo,
    schema::{ProtocolVersion, v1::*},
};
use common::*;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::method};

fn agent(url: &str, state: &std::path::Path) -> AcpAgentConfig {
    AcpAgentConfig::new(env!("CARGO_BIN_EXE_openwebui-acp"))
        .args([
            "--url",
            url,
            "--model",
            MODEL,
            "--state-dir",
            state.to_str().unwrap(),
            "--allow-insecure-http",
            "true",
            "--timeout-secs",
            "5",
        ])
        .env("OPENWEBUI_API_KEY", KEY)
}

#[tokio::test]
async fn official_sdk_load_across_processes_restores_visible_history_and_same_chat() {
    let f = Fixture::start().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state");
    let saved = Arc::new(Mutex::new(None));
    let capture = saved.clone();
    Client
        .builder()
        .connect_with(
            AcpAgent::new(agent(&f.server.uri(), &path)),
            |cx: ConnectionTo<Agent>| async move {
                let init = cx
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                assert!(init.agent_capabilities.load_session);
                let id = cx
                    .send_request(NewSessionRequest::new("/client-only"))
                    .block_task()
                    .await?
                    .session_id;
                cx.send_request(PromptRequest::new(
                    id.clone(),
                    vec!["Remember ORCHARD".into()],
                ))
                .block_task()
                .await?;
                *capture.lock().unwrap() = Some(id);
                Ok(())
            },
        )
        .await
        .unwrap();
    let id = saved.lock().unwrap().clone().unwrap();
    let updates = Arc::new(Mutex::new(Vec::new()));
    let capture = updates.clone();
    // SDK disconnect waits for its child; the next process must acquire the same lock.
    Client
        .builder()
        .on_receive_notification(
            async move |n: SessionNotification, _| {
                capture.lock().unwrap().push(n.update);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(
            AcpAgent::new(agent(&f.server.uri(), &path)),
            |cx: ConnectionTo<Agent>| async move {
                cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                cx.send_request(LoadSessionRequest::new(id.clone(), "/client-only"))
                    .block_task()
                    .await?;
                assert_eq!(
                    cx.send_request(PromptRequest::new(id, vec!["What codename?".into()]))
                        .block_task()
                        .await?
                        .stop_reason,
                    StopReason::EndTurn
                );
                Ok(())
            },
        )
        .await
        .unwrap();
    assert_eq!(f.state.lock().unwrap().chats.len(), 1);
    assert_eq!(f.state.lock().unwrap().completions, 2);
    assert!(
        updates
            .lock()
            .unwrap()
            .iter()
            .any(|u| matches!(u, SessionUpdate::UserMessageChunk(_)))
    );
    assert!(
        f.completions()
            .await
            .iter()
            .all(|r| r.get("tools").is_none())
    );
}

fn prompt(id: &SessionId, author: &str, channel: &str, root: char, event: char) -> PromptRequest {
    let root = root.to_string().repeat(64);
    let event = event.to_string().repeat(64);
    PromptRequest::new(id.clone(),vec![format!("<context>\nScope: thread\nChannel: {channel}\nThread root: {root}\nIMPORTANT: use `--reply-to {root}` on `buzz messages send`.\n</context>").into(),format!("<buzz-event type=\"@mention\">\nEvent ID: {event}\nChannel: {channel}\nFrom: Requester (hex: {author})\nContent: Recall thread context\n</buzz-event>").into()])
}

#[tokio::test]
async fn buzz_threads_survive_process_restart_and_denied_requests_never_reach_openwebui() {
    let f = Fixture::start().await;
    let relay = MockServer::start().await;
    let keys = nostr::Keys::generate();
    let human = nostr::Keys::generate().public_key().to_hex();
    let delegate = nostr::Keys::generate().public_key().to_hex();
    let channel = "12345678-1234-4234-8234-123456789abc";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state");
    Mock::given(method("POST"))
        .and(wiremock::matchers::path("/events"))
        .respond_with(|r: &Request| {
            let event: nostr::Event = serde_json::from_slice(&r.body).unwrap();
            event.verify().unwrap();
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"event_id":event.id.to_hex(),"accepted":true}))
        })
        .mount(&relay)
        .await;
    for cycle in 0..2 {
        let config = agent(&f.server.uri(), &path)
            .args([
                "--buzz-auto-publish",
                "true",
                "--buzz-relay",
                &relay.uri(),
                "--buzz-allow-author",
                &human,
                "--buzz-allow-author",
                &delegate,
                "--buzz-allow-channel",
                channel,
            ])
            .env("BUZZ_PRIVATE_KEY", keys.secret_key().to_secret_hex());
        let author = if cycle == 0 {
            human.clone()
        } else {
            delegate.clone()
        };
        let base = &f;
        tokio::time::timeout(
            Duration::from_secs(15),
            Client.builder().connect_with(
                AcpAgent::new(config),
                |cx: ConnectionTo<Agent>| async move {
                    cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                        .block_task()
                        .await?;
                    let id = cx
                        .send_request(NewSessionRequest::new("/client-only"))
                        .block_task()
                        .await?
                        .session_id;
                    let before = base.server.received_requests().await.unwrap().len();
                    assert!(
                        cx.send_request(prompt(&id, &"e".repeat(64), channel, 'a', 'f'))
                            .block_task()
                            .await
                            .is_err()
                    );
                    assert!(
                        cx.send_request(prompt(
                            &id,
                            &author,
                            "99999999-9999-4999-8999-999999999999",
                            'a',
                            'e'
                        ))
                        .block_task()
                        .await
                        .is_err()
                    );
                    assert_eq!(base.server.received_requests().await.unwrap().len(), before);
                    for (root, event) in if cycle == 0 {
                        vec![('a', 'a'), ('b', 'b'), ('a', 'c')]
                    } else {
                        vec![('a', 'd'), ('b', 'e')]
                    } {
                        assert_eq!(
                            cx.send_request(prompt(&id, &author, channel, root, event))
                                .block_task()
                                .await?
                                .stop_reason,
                            StopReason::EndTurn
                        );
                        let before = base.server.received_requests().await.unwrap().len();
                        assert_eq!(
                            cx.send_request(prompt(&id, &author, channel, root, event))
                                .block_task()
                                .await?
                                .stop_reason,
                            StopReason::EndTurn
                        );
                        assert_eq!(base.server.received_requests().await.unwrap().len(), before);
                    }
                    Ok(())
                },
            ),
        )
        .await
        .unwrap()
        .unwrap();
    }
    let c = f.completions().await;
    assert_eq!(c.len(), 5);
    assert_eq!(c[0]["chat_id"], c[2]["chat_id"]);
    assert_eq!(c[0]["chat_id"], c[3]["chat_id"]);
    assert_eq!(c[1]["chat_id"], c[4]["chat_id"]);
    assert_ne!(c[0]["chat_id"], c[1]["chat_id"]);
    assert_eq!(relay.received_requests().await.unwrap().len(), 5);
}
