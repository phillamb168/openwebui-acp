mod common;
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo,
    schema::{ProtocolVersion, v1::*},
};
use common::*;
use serde_json::Value;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

fn agent(url: &str) -> AcpAgent {
    AcpAgent::new(
        AcpAgentConfig::new(env!("CARGO_BIN_EXE_openwebui-acp"))
            .args([
                "--url",
                url,
                "--model",
                MODEL,
                "--require-tool",
                TOOL,
                "--buzz-auto-publish",
                "false",
            ])
            .env("OPENWEBUI_API_KEY", KEY)
            .env("OPENWEBUI_ACP_ALLOW_INSECURE_HTTP", "true")
            .env("OPENWEBUI_ACP_KEEP_OPENWEBUI_CHATS", "true")
            .env("OPENWEBUI_ACP_TIMEOUT_SECS", "5")
            .env("RUST_LOG", "trace"),
    )
}

#[tokio::test]
async fn official_sdk_stdio_transcript_multiturn_and_stdout_purity() {
    let f = Fixture::start().await;
    let lines = Arc::new(Mutex::new(Vec::new()));
    let record = lines.clone();
    let agent = agent(&f.server.uri()).with_debug(move |line, direction| {
        record
            .lock()
            .unwrap()
            .push((format!("{direction:?}"), line.to_owned()));
    });
    let updates = Arc::new(Mutex::new(Vec::new()));
    let received = updates.clone();
    tokio::time::timeout(Duration::from_secs(10),Client.builder()
        .on_receive_notification(async move |n:SessionNotification,_cx|{received.lock().unwrap().push(n);Ok(())},agent_client_protocol::on_receive_notification!())
        .connect_with(agent,|cx:ConnectionTo<Agent>|async move{
            let init=cx.send_request(InitializeRequest::new(ProtocolVersion::V1)).block_task().await?;
            assert_eq!(init.protocol_version,ProtocolVersion::V1);assert!(!init.agent_capabilities.load_session);
            let new:NewSessionRequest=serde_json::from_value(serde_json::json!({"cwd":"/client-only","mcpServers":[{"name":"not-started","command":"must-not-execute","args":[],"env":[]}]})).unwrap();
            let session=cx.send_request(new).block_task().await?.session_id;
            for text in ["Remember ORCHARD","What codename?"]{
                let r=cx.send_request(PromptRequest::new(session.clone(),vec![ContentBlock::Text(TextContent::new(text))])).block_task().await?;
                assert_eq!(r.stop_reason,StopReason::EndTurn);
            }
            let other=cx.send_request(NewSessionRequest::new("/different")).block_task().await?.session_id;assert_ne!(session,other);
            let bad=cx.send_request(PromptRequest::new(session,vec![])).block_task().await;assert!(bad.is_err());
            Ok(())
        })).await.unwrap().unwrap();
    assert_eq!(updates.lock().unwrap().len(), 2);
    let lines = lines.lock().unwrap();
    assert!(!lines.is_empty());
    assert!(lines.iter().any(|(direction, _)| direction == "Stdout"));
    assert!(lines.iter().any(|(direction, _)| direction == "Stderr"));
    for (direction, line) in lines.iter() {
        assert!(!line.contains(KEY));
        if direction.contains("Stdout") {
            let v: Value = serde_json::from_str(line).expect("stdout must be JSON-RPC");
            assert_eq!(v["jsonrpc"], "2.0");
        }
    }
    assert_eq!(f.state.lock().unwrap().completions, 2);
}

#[tokio::test]
async fn session_cancel_is_processed_while_prompt_is_running() {
    let f = Fixture::start().await;
    f.state.lock().unwrap().hang = true;
    let state = f.state.clone();
    let updates = Arc::new(Mutex::new(Vec::new()));
    let received = updates.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        Client
            .builder()
            .on_receive_notification(
                async move |n: SessionNotification, _cx| {
                    received.lock().unwrap().push(n);
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .connect_with(
                agent(&f.server.uri()),
                |cx: ConnectionTo<Agent>| async move {
                    cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                        .block_task()
                        .await?;
                    let session = cx
                        .send_request(NewSessionRequest::new("/"))
                        .block_task()
                        .await?
                        .session_id;
                    let request = cx
                        .send_request(PromptRequest::new(
                            session.clone(),
                            vec![ContentBlock::Text(TextContent::new("wait"))],
                        ))
                        .block_task();
                    let cancel = async {
                        while state.lock().unwrap().completions == 0 {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                        let busy = cx
                            .send_request(PromptRequest::new(
                                session.clone(),
                                vec![ContentBlock::Text(TextContent::new("overlap"))],
                            ))
                            .block_task()
                            .await;
                        assert!(
                            busy.is_err(),
                            "concurrent prompts in one session must be rejected"
                        );
                        cx.send_notification(CancelNotification::new(session))
                    };
                    let (response, sent) = tokio::join!(request, cancel);
                    sent?;
                    assert_eq!(response?.stop_reason, StopReason::Cancelled);
                    Ok(())
                },
            ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(updates.lock().unwrap().is_empty());
    assert!(f.state.lock().unwrap().stopped);
}
