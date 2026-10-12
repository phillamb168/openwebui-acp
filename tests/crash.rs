#![cfg(unix)]
mod common;
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo,
    schema::{ProtocolVersion, v1::*},
};
use std::{
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
    time::Duration,
};
use wiremock::{
    Mock, Request, ResponseTemplate,
    matchers::{method, path},
};

#[tokio::test]
async fn sigkill_at_remote_write_boundaries_never_replays_model_submission() {
    for during_save in [true, false] {
        let f = common::Fixture::start().await;
        f.state.lock().unwrap().hang = true;
        if during_save {
            let capture = f.state.clone();
            Mock::given(method("POST"))
                .and(path("/api/v1/chats/new"))
                .respond_with(move |r: &Request| {
                    let body: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
                    let chat = serde_json::json!({"id":"chat-1", "chat":body["chat"]});
                    capture
                        .lock()
                        .unwrap()
                        .chats
                        .insert("chat-1".into(), chat.clone());
                    ResponseTemplate::new(200)
                        .set_body_json(chat)
                        .set_delay(Duration::from_secs(10))
                })
                .mount(&f.server)
                .await;
        }
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state");
        let pid_path = dir.path().join("child.pid");
        let wrapper = dir.path().join("spawn-test-agent");
        // Record only the PID of this specific child, then replace the shell with the agent.
        std::fs::write(&wrapper, b"#!/bin/sh\nprintf '%s\\n' \"$$\" > \"$FIXTURE_PID_FILE\"\nexec \"$FIXTURE_AGENT_BINARY\" \"$@\"\n").unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let args = vec![
            "--url".into(),
            f.server.uri(),
            "--model".into(),
            common::MODEL.into(),
            "--state-dir".into(),
            state_path.to_str().unwrap().into(),
            "--allow-insecure-http".into(),
            "true".into(),
        ];
        let launch = AcpAgentConfig::new(&wrapper)
            .args(args.clone())
            .env("OPENWEBUI_API_KEY", common::KEY)
            .env("FIXTURE_PID_FILE", pid_path.to_str().unwrap())
            .env("FIXTURE_AGENT_BINARY", env!("CARGO_BIN_EXE_openwebui-acp"));
        let saved_id = Arc::new(Mutex::new(None));
        let save = saved_id.clone();
        let captured = f.state.clone();
        let run = Client.builder().connect_with(
            AcpAgent::new(launch),
            |cx: ConnectionTo<Agent>| async move {
                cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                let id = cx
                    .send_request(NewSessionRequest::new("/fixture-only"))
                    .block_task()
                    .await?
                    .session_id;
                *save.lock().unwrap() = Some(id.clone());
                let request = cx
                    .send_request(PromptRequest::new(id, vec!["Remember ORCHARD".into()]))
                    .block_task();
                let crash = async {
                    loop {
                        let reached = {
                            let s = captured.lock().unwrap();
                            if during_save {
                                !s.chats.is_empty()
                            } else {
                                s.completions == 1
                            }
                        };
                        if reached {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    let pid: i32 = std::fs::read_to_string(&pid_path)
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    assert!(pid > 1);
                    rustix::process::kill_process(
                        rustix::process::Pid::from_raw(pid).unwrap(),
                        rustix::process::Signal::KILL,
                    )
                    .unwrap();
                };
                let (response, ()) = tokio::join!(request, crash);
                assert!(response.is_err());
                Ok(())
            },
        );
        // Transport shutdown after SIGKILL may itself return an error; it must be bounded.
        let _ = tokio::time::timeout(Duration::from_secs(15), run)
            .await
            .unwrap();
        assert!(
            !f.state.lock().unwrap().stopped,
            "SIGKILL must bypass graceful cancellation"
        );
        let id = saved_id.lock().unwrap().clone().unwrap();
        {
            let mut remote = f.state.lock().unwrap();
            remote.hang = false;
            for chat in remote.chats.values_mut() {
                for m in chat["chat"]["history"]["messages"]
                    .as_object_mut()
                    .unwrap()
                    .values_mut()
                {
                    if m["role"] == "assistant" {
                        m["done"] = true.into();
                    }
                }
            }
        }
        let launch = AcpAgentConfig::new(env!("CARGO_BIN_EXE_openwebui-acp"))
            .args(args)
            .env("OPENWEBUI_API_KEY", common::KEY);
        tokio::time::timeout(
            Duration::from_secs(10),
            Client.builder().connect_with(
                AcpAgent::new(launch),
                |cx: ConnectionTo<Agent>| async move {
                    cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                        .block_task()
                        .await?;
                    let loaded = cx
                        .send_request(LoadSessionRequest::new(id, "/fixture-only"))
                        .block_task()
                        .await;
                    assert_eq!(loaded.is_err(), during_save);
                    Ok(())
                },
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            f.state.lock().unwrap().completions,
            if during_save { 0 } else { 1 }
        );
    }
}
