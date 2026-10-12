mod common;
use common::*;
use openwebui_acp::{error::Error, openwebui::OpenWebUi, session::Session};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{method, path},
};

#[tokio::test]
async fn inaccessible_account_profile_fails_before_any_remote_write() {
    for status in [401, 403] {
        let f = Fixture::start().await;
        Mock::given(path("/api/v1/users/user/status"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&f.server)
            .await;
        assert!(matches!(
            Session::default().run(&f.api(), "test", &CancellationToken::new()).await,
            Err(Error::Http { status: actual, .. }) if actual == status
        ));
        let all = f.server.received_requests().await.unwrap();
        assert!(all.iter().all(|r| r.method == "GET"));
        assert_eq!(
            all.iter()
                .filter(|r| r.url.path() == "/api/v1/users/user/status")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn full_native_loop_preserves_defaults_and_multiturn_tree() {
    let f = Fixture::start().await;
    let api = f.api();
    let cancel = CancellationToken::new();
    let mut s = Session::default();
    assert_eq!(
        s.run(&api, "Remember ORCHARD", &cancel).await.unwrap(),
        "ORCHARD"
    );
    let first = s.chat.as_ref().unwrap().clone();
    let first_assistant = first.chat.history.current_id.as_ref().unwrap();
    let first_user = first.chat.history.messages[first_assistant]
        .parent_id
        .as_ref()
        .expect("the completed assistant must remain linked to its user message");
    assert_eq!(
        first.chat.history.messages[first_user].content,
        "Remember ORCHARD"
    );
    assert_eq!(
        s.run(&api, "What did I ask you to remember?", &cancel)
            .await
            .unwrap(),
        "ORCHARD"
    );
    let second = s.chat.as_ref().unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(second.chat.history.messages.len(), 4);
    let old_assistant = first.chat.history.current_id.as_ref().unwrap();
    let next_user = &second.chat.history.messages[old_assistant].children_ids[0];
    assert_eq!(
        second.chat.history.messages[next_user].parent_id.as_ref(),
        Some(old_assistant)
    );
    assert_eq!(
        second.chat.history.messages[old_assistant].extra["sources"][0]["source"]["id"],
        "server-owned-source"
    );
    let requests = f.completions().await;
    assert_eq!(requests.len(), 2);
    for request in &requests {
        let user = &request["user_message"];
        assert_eq!(user["role"], "user");
        // Open WebUI 0.12's message model accepts an omitted flag, not null.
        assert!(user.get("done").is_none());
        assert!(user["id"].as_str().is_some_and(|id| !id.is_empty()));
        assert_eq!(user["childrenIds"], json!([request["id"]]));
        assert_eq!(request["parent_id"], user["parentId"]);
        assert_eq!(user["models"], json!([MODEL]));
        assert_eq!(request["model"], MODEL);
        assert_eq!(request["stream"], true);
        assert!(!request["session_id"].as_str().unwrap().is_empty());
        assert_eq!(
            request["tool_ids"],
            json!([TOOL, "server:mcp:calendar", "server:mcp:notes"])
        );
        assert_eq!(request["features"]["web_search"], true);
        assert!(request.get("tools").is_none());
        assert!(request.get("skill_ids").is_none());
        assert!(request.get("params").is_none());
        assert_eq!(
            request["background_tasks"],
            json!({"title_generation":false,"tags_generation":false,"follow_up_generation":false})
        );
    }
    assert_eq!(requests[1]["messages"].as_array().unwrap().len(), 3);
    assert_eq!(requests[0]["parent_id"], serde_json::Value::Null);
    assert_eq!(requests[1]["parent_id"], requests[0]["id"]);
    assert_eq!(requests[0]["user_message"]["content"], "Remember ORCHARD");
    assert_eq!(
        requests[1]["user_message"]["content"],
        "What did I ask you to remember?"
    );
    assert_eq!(requests[1]["messages"][0]["content"], "Remember ORCHARD");
    assert!(requests[1]["messages"][1]["output"].is_array());
    assert_eq!(requests[0]["session_id"], requests[1]["session_id"]);
    let all = f.server.received_requests().await.unwrap();
    assert!(all.iter().all(|r| r.url.path() != "/api/v1/auths/"));
    assert_eq!(
        all.iter()
            .filter(|r| r.url.path() == "/api/v1/models/model")
            .count(),
        2
    );
    assert!(
        all.iter()
            .all(|r| r.headers["authorization"] == format!("Bearer {KEY}"))
    );
}

#[tokio::test]
async fn a_completed_answer_with_a_broken_parent_link_is_not_success() {
    let f = Fixture::start().await;
    f.state.lock().unwrap().orphan_assistant = true;
    let error = Session::default()
        .run(&f.api(), "Remember ORCHARD", &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        Error::InvalidResponse("completed message tree")
    ));
    assert_eq!(f.state.lock().unwrap().completions, 1);
}

#[tokio::test]
async fn independent_sessions_have_separate_chats_and_histories() {
    let f = Fixture::start().await;
    let api = f.api();
    let mut a = Session::default();
    let mut b = Session::default();
    let cancel = CancellationToken::new();
    let (a_result, b_result) =
        tokio::join!(a.run(&api, "alpha", &cancel), b.run(&api, "beta", &cancel));
    a_result.unwrap();
    b_result.unwrap();
    assert_ne!(a.chat.as_ref().unwrap().id, b.chat.as_ref().unwrap().id);
    for request in f.completions().await {
        assert_eq!(request["messages"].as_array().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn required_tool_is_checked_before_creating_a_chat() {
    let f = Fixture::start().await;
    let mut model = model_fixture();
    model["meta"]["toolIds"] = json!([]);
    Mock::given(path("/api/v1/models/model"))
        .respond_with(ResponseTemplate::new(200).set_body_json(model))
        .mount(&f.server)
        .await;
    assert!(matches!(
        Session::default()
            .run(&f.api(), "test", &CancellationToken::new())
            .await,
        Err(Error::Model(_))
    ));
    assert!(f.state.lock().unwrap().chats.is_empty());
}

#[tokio::test]
async fn oauth_returns_link_without_starting_auth_or_inference() {
    let f = Fixture::start().await;
    Mock::given(path("/api/v1/tools/"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([{"id":TOOL,"authenticated":false}])),
        )
        .mount(&f.server)
        .await;
    let e = Session::default()
        .run(&f.api(), "test", &CancellationToken::new())
        .await
        .unwrap_err();
    match e {
        Error::Authentication { url } => assert_eq!(
            url,
            format!("{}/oauth/clients/mcp:inventory/authorize", f.server.uri())
        ),
        _ => panic!("expected authentication error"),
    }
    assert!(f.state.lock().unwrap().chats.is_empty());
    assert!(
        !f.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|r| r.url.path().starts_with("/oauth/"))
    );
}

#[tokio::test]
async fn authorization_errors_do_not_retry_or_leak_bodies() {
    for status in [401, 403, 404] {
        let f = Fixture::start().await;
        Mock::given(path("/api/v1/models/model"))
            .respond_with(ResponseTemplate::new(status).set_body_string(KEY))
            .mount(&f.server)
            .await;
        let error = f.api().defaults().await.err().unwrap();
        assert!(matches!(error,Error::Http{status:s,..} if s==status));
        assert!(!error.to_string().contains(KEY));
        assert_eq!(f.server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn timeout_cancels_owner_chat_without_admin_endpoint() {
    let f = Fixture::start().await;
    f.state.lock().unwrap().hang = true;
    let mut c = config(&f.server.uri());
    c.turn_timeout = Duration::from_secs(5);
    let api = OpenWebUi::new(Arc::new(c)).unwrap();
    let mut s = Session::default();
    assert!(matches!(
        s.run(&api, "test", &CancellationToken::new()).await,
        Err(Error::Timeout)
    ));
    assert!(
        f.state.lock().unwrap().stopped,
        "phase {:?}, completions {}",
        s.checkpoint.phase,
        f.state.lock().unwrap().completions
    );
    assert!(
        !f.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|r| r.url.path().starts_with("/api/tasks/stop/"))
    );
}

#[tokio::test]
async fn cancellation_is_bounded_and_failed_stop_quarantines_session() {
    let f = Fixture::start().await;
    f.state.lock().unwrap().hang = true;
    Mock::given(method("POST"))
        .and(path("/api/tasks/chat/chat-1/stop"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&f.server)
        .await;
    let api = f.api();
    let mut s = Session::default();
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let state = f.state.clone();
    let trigger = tokio::spawn(async move {
        while state.lock().unwrap().completions == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        token.cancel();
    });
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), s.run(&api, "test", &cancel))
            .await
            .unwrap(),
        Err(Error::Cancelled)
    ));
    trigger.await.unwrap();
    assert!(matches!(
        s.run(&api, "retry", &CancellationToken::new()).await,
        Err(Error::UncertainSession)
    ));
    assert_eq!(f.state.lock().unwrap().completions, 1);
}

#[tokio::test]
async fn failed_or_missing_assistant_is_not_success() {
    for missing in [false, true] {
        let f = Fixture::start().await;
        {
            let mut state = f.state.lock().unwrap();
            state.failed = !missing;
            state.missing_assistant = missing;
        }
        let e = Session::default()
            .run(&f.api(), "test", &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(e, Error::TaskFailed | Error::InvalidResponse(_)));
        assert!(!e.to_string().contains("SERVER_SECRET"));
    }
}

#[tokio::test]
async fn empty_tasks_with_unfinished_message_eventually_times_out() {
    let f = Fixture::start().await;
    f.state.lock().unwrap().unfinished = true;
    let mut c = config(&f.server.uri());
    c.turn_timeout = Duration::from_millis(150);
    let api = OpenWebUi::new(Arc::new(c)).unwrap();
    assert!(matches!(
        Session::default()
            .run(&api, "test", &CancellationToken::new())
            .await,
        Err(Error::Timeout)
    ));
}

#[tokio::test]
async fn completion_submission_is_never_blindly_replayed() {
    let f = Fixture::start().await;
    Mock::given(path("/api/chat/completions"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&f.server)
        .await;
    assert!(matches!(
        Session::default()
            .run(&f.api(), "test", &CancellationToken::new())
            .await,
        Err(Error::Http { status: 503, .. })
    ));
    assert_eq!(f.completions().await.len(), 1);
}

#[tokio::test]
async fn api_key_public_config_requires_known_memory_default_but_preserves_web_search() {
    let f = Fixture::start().await;
    Mock::given(path("/api/config"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"features":{"auth":true,"websocket_heartbeat_interval":30}})),
        )
        .mount(&f.server)
        .await;
    Mock::given(path("/api/v1/users/user/settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ui":null})))
        .mount(&f.server)
        .await;
    assert!(matches!(f.api().defaults().await, Err(Error::Model(_))));
    let mut c = config(&f.server.uri());
    c.memory_default = Some(false);
    let api = OpenWebUi::new(Arc::new(c)).unwrap();
    let (_, defaults) = api.defaults().await.unwrap();
    assert!(defaults.features.web_search);
    assert!(!defaults.features.memory);
    assert!(f.state.lock().unwrap().chats.is_empty());
}

#[tokio::test]
async fn transient_reads_retry_boundedly_and_malformed_metadata_does_not() {
    let f = Fixture::start().await;
    Mock::given(path("/api/v1/models/model"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&f.server)
        .await;
    assert!(matches!(
        f.api().defaults().await,
        Err(Error::Http { status: 503, .. })
    ));
    assert_eq!(f.server.received_requests().await.unwrap().len(), 3);
    let f = Fixture::start().await;
    Mock::given(path("/api/v1/models/model"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&f.server)
        .await;
    assert!(matches!(
        f.api().defaults().await,
        Err(Error::InvalidResponse(_))
    ));
    assert_eq!(f.server.received_requests().await.unwrap().len(), 1);
}
