mod common;
use clap::Parser;
use futures::StreamExt;
use openwebui_acp::{
    config::{Args, Config},
    error::Error,
    secure_file,
    session::Session,
    state::{Phase, Scope, Store},
    transport,
};
use std::{collections::HashMap, io::Write, sync::Arc};
use tokio_util::sync::CancellationToken;
use wiremock::{Mock, ResponseTemplate, matchers::path};

fn scope() -> Scope {
    Scope {
        endpoint: "https://webui.example/".into(),
        model: "assistant".into(),
        relay: None,
        agent: None,
    }
}
fn args(extra: &[&str]) -> Args {
    Args::try_parse_from(std::iter::once("adapter").chain(extra.iter().copied())).unwrap()
}
fn env() -> HashMap<String, String> {
    HashMap::from([
        ("OPENWEBUI_URL".into(), "https://webui.example".into()),
        ("OPENWEBUI_MODEL".into(), "environment-model".into()),
        ("OPENWEBUI_API_KEY".into(), common::KEY.into()),
    ])
}

#[test]
fn configuration_is_explicit_https_by_default_and_redacted() {
    let e = env();
    let f = HashMap::from([("OPENWEBUI_MODEL".into(), "file-model".into())]);
    assert_eq!(
        Config::resolve(args(&["--model", "cli-model"]), &e, &f)
            .unwrap()
            .model,
        "cli-model"
    );
    let c = Config::resolve(args(&[]), &e, &f).unwrap();
    assert_eq!(c.model, "environment-model");
    assert!(c.buzz.is_none());
    assert!(c.state_dir.is_none());
    assert!(!format!("{c:?}").contains(common::KEY));
    for url in [
        "http://127.0.0.1:1234",
        "https://user:secret@webui.example",
        "https://webui.example/?token=secret",
        "file:///etc/passwd",
    ] {
        assert!(Config::resolve(args(&["--url", url]), &e, &f).is_err());
    }
    assert!(
        Config::resolve(
            args(&["--url", "http://127.0.0.1", "--allow-insecure-http", "true"]),
            &e,
            &f
        )
        .is_ok()
    );
    for timeout in ["0", "3601", "18446744073709551615"] {
        assert!(Config::resolve(args(&["--timeout-secs", timeout]), &e, &f).is_err());
    }
    assert!(Config::resolve(args(&["--state-dir", "relative"]), &e, &f).is_err());
    assert!(Config::resolve(args(&["--buzz-auto-publish", "true"]), &e, &f).is_err());
}

#[test]
fn key_file_precedence_is_environment_before_explicit_env_file() {
    let mut e = env();
    e.remove("OPENWEBUI_API_KEY");
    e.insert(
        "OPENWEBUI_API_KEY_FILE".into(),
        "/private/environment-key".into(),
    );
    let f = HashMap::from([("OPENWEBUI_API_KEY".into(), "lower-priority".into())]);
    let c = Config::resolve(args(&[]), &e, &f).unwrap();
    // A pure configuration preview cannot accidentally transmit its unresolved file path.
    assert!(openwebui_acp::openwebui::OpenWebUi::new(Arc::new(c)).is_err());
    let c = Config::resolve(args(&["--api-key-file", "/private/cli-key"]), &e, &f).unwrap();
    assert!(openwebui_acp::openwebui::OpenWebUi::new(Arc::new(c)).is_err());
}

#[cfg(unix)]
#[test]
fn private_file_reader_rejects_fifos_without_blocking() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(secure_file::read_secret(&fifo).is_err());
    });
    assert!(rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap());
}

#[test]
fn secret_files_require_private_regular_owned_files() {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    writeln!(f, "synthetic-key").unwrap();
    assert_eq!(
        secure_file::read_secret(f.path()).unwrap().trim(),
        "synthetic-key"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let link = f.path().with_extension("link");
        symlink(f.path(), &link).unwrap();
        assert!(secure_file::read_secret(&link).is_err());
        std::fs::remove_file(link).unwrap();
        f.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o644))
            .unwrap();
        assert!(secure_file::read_secret(f.path()).is_err());
    }
}

#[test]
fn persistent_state_is_exclusive_scoped_bounded_and_rejects_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(Some(dir.path().join("state")), scope()).unwrap();
    assert!(Store::open(Some(dir.path().join("state")), scope()).is_err());
    store.bind_account("user-a").unwrap();
    store.bind_account("user-a").unwrap();
    assert!(matches!(
        store.bind_account("user-b"),
        Err(Error::StateScope)
    ));
    store.create_session("saved", "/local-cwd".into()).unwrap();
    drop(store);
    let store = Store::open(Some(dir.path().join("state")), scope()).unwrap();
    assert_eq!(
        store.session("saved").unwrap(),
        std::path::PathBuf::from("/local-cwd")
    );
    drop(store);
    for changed in 0..4 {
        let mut other = scope();
        match changed {
            0 => other.model = "other-model".into(),
            1 => other.endpoint = "https://other.example/".into(),
            2 => other.relay = Some("https://relay.example/".into()),
            _ => other.agent = Some("different-agent".into()),
        }
        assert!(matches!(
            Store::open(Some(dir.path().join("state")), other),
            Err(Error::StateScope)
        ));
    }
    std::fs::write(
        dir.path().join("state/state.json"),
        b"corrupt PRIVATE_PAYLOAD",
    )
    .unwrap();
    match Store::open(Some(dir.path().join("state")), scope()) {
        Ok(_) => panic!("corrupt state accepted"),
        Err(e) => assert!(!e.to_string().contains("PRIVATE_PAYLOAD")),
    }
}

#[tokio::test]
async fn restart_recovers_completed_submission_without_reexecuting_and_preserves_context() {
    let f = common::Fixture::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(Some(dir.path().join("state")), scope()).unwrap();
    store.bind_account("fixture-account").unwrap();
    let mut session = Session::attach(store.clone(), "acp/synthetic".into()).unwrap();
    session
        .run(&f.api(), "PRIVATE_PROMPT_MARKER", &CancellationToken::new())
        .await
        .unwrap();
    // Simulate a crash after server completion but before durable local acknowledgement.
    let mut checkpoint = session.checkpoint.clone();
    checkpoint.phase = Phase::Submitted;
    checkpoint.last_completed = None;
    store.save_checkpoint("acp/synthetic", checkpoint).unwrap();
    drop(session);
    drop(store);
    let bytes = std::fs::read_to_string(dir.path().join("state/state.json")).unwrap();
    assert!(!bytes.contains("PRIVATE_PROMPT_MARKER"));
    assert!(!bytes.contains(common::KEY));
    let store = Store::open(Some(dir.path().join("state")), scope()).unwrap();
    let mut session = Session::attach(store, "acp/synthetic".into()).unwrap();
    assert_eq!(
        session
            .recover(&f.api(), &CancellationToken::new())
            .await
            .unwrap()
            .as_deref(),
        Some("ORCHARD")
    );
    assert_eq!(f.state.lock().unwrap().completions, 1);
    session
        .run(&f.api(), "continue", &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(f.state.lock().unwrap().completions, 2);
    assert_eq!(f.state.lock().unwrap().chats.len(), 1);
    assert_eq!(
        session.chat.as_ref().unwrap().chat.history.messages.len(),
        4
    );
}

#[tokio::test]
async fn ambiguous_writes_and_external_chat_changes_fail_closed() {
    let f = common::Fixture::start().await;
    for phase in [Phase::Saving, Phase::Saved, Phase::Interrupted] {
        let mut s = Session::default();
        s.checkpoint.phase = phase;
        assert!(matches!(
            s.recover(&f.api(), &CancellationToken::new()).await,
            Err(Error::UncertainSession)
        ));
        assert!(matches!(
            s.run(&f.api(), "do not retry", &CancellationToken::new())
                .await,
            Err(Error::UncertainSession)
        ));
    }
    assert!(f.server.received_requests().await.unwrap().is_empty());
    let mut s = Session::default();
    s.run(&f.api(), "first", &CancellationToken::new())
        .await
        .unwrap();
    let id = s.chat.as_ref().unwrap().id.clone();
    f.state.lock().unwrap().chats.get_mut(&id).unwrap()["chat"]["history"]["currentId"] =
        serde_json::json!("external-edit");
    assert!(matches!(
        s.run(&f.api(), "second", &CancellationToken::new()).await,
        Err(Error::ChatConflict)
    ));
    assert_eq!(f.state.lock().unwrap().completions, 1);
}

#[tokio::test]
async fn credentials_never_follow_redirects_or_cross_origins() {
    let f = common::Fixture::start().await;
    let outside = wiremock::MockServer::start().await;
    let api = f.api();
    assert!(
        api.get::<serde_json::Value>(outside.uri().parse().unwrap(), "test")
            .await
            .is_err()
    );
    Mock::given(path("/redirect"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", outside.uri()))
        .mount(&f.server)
        .await;
    assert!(
        api.get::<serde_json::Value>(api.endpoint(&["redirect"]), "test")
            .await
            .is_err()
    );
    assert!(outside.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn framing_and_prompt_limits_apply_before_deserialization_and_network() {
    let input = b"one\ntwo\n".to_vec();
    let lines = transport::incoming(futures::io::Cursor::new(input));
    futures::pin_mut!(lines);
    assert_eq!(lines.next().await.unwrap().unwrap(), "one");
    assert_eq!(lines.next().await.unwrap().unwrap(), "two");
    assert!(lines.next().await.is_none());
    for data in [
        vec![b'x'; transport::MAX_FRAME_BYTES + 1],
        vec![255, b'\n'],
        b"unterminated".to_vec(),
    ] {
        let lines = transport::incoming(futures::io::Cursor::new(data));
        futures::pin_mut!(lines);
        assert!(lines.next().await.unwrap().is_err());
        assert!(lines.next().await.is_none());
    }
    let too_large = agent_client_protocol::schema::v1::ContentBlock::Text(
        agent_client_protocol::schema::v1::TextContent::new(
            "x".repeat(transport::MAX_PROMPT_BYTES + 1),
        ),
    );
    assert!(matches!(
        openwebui_acp::acp::text_blocks(&[too_large]),
        Err(Error::Capacity)
    ));
}

#[tokio::test]
async fn saved_account_identity_is_checked_after_key_rotation() {
    let f = common::Fixture::start().await;
    let store = Store::open(None, scope()).unwrap();
    store
        .bind_account(&f.api().account_id().await.unwrap())
        .unwrap();
    let mut config = common::config(&f.server.uri());
    config.api_key = openwebui_acp::config::Secret::new("rotated-key".into());
    let api = openwebui_acp::openwebui::OpenWebUi::new(Arc::new(config)).unwrap();
    store
        .bind_account(&api.account_id().await.unwrap())
        .unwrap();
    Mock::given(path("/api/v1/users/user/status"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"different-account"})),
        )
        .mount(&f.server)
        .await;
    assert!(matches!(
        store.bind_account(&api.account_id().await.unwrap()),
        Err(Error::StateScope)
    ));
}

#[tokio::test]
async fn cancelled_recovery_does_not_start_http_or_publish_a_late_result() {
    let f = common::Fixture::start().await;
    let mut s = Session::default();
    s.run(&f.api(), "first", &CancellationToken::new())
        .await
        .unwrap();
    let before = f.server.received_requests().await.unwrap().len();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        s.recover(&f.api(), &cancel).await,
        Err(Error::Cancelled)
    ));
    assert_eq!(before, f.server.received_requests().await.unwrap().len());
}
