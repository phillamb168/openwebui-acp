//! Opt-in live client. Without --execute this only prints a reviewable preview.
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo,
    schema::{ProtocolVersion, v1::*},
};
use clap::Parser;
use openwebui_acp::{
    config::{Args, Config},
    openwebui::OpenWebUi,
};
use std::{path::PathBuf, sync::Arc};

#[derive(Parser)]
struct Smoke {
    #[arg(long)]
    url: String,
    #[arg(long)]
    model: String,
    #[arg(long)]
    env_file: PathBuf,
    #[arg(long, default_value = "target/debug/openwebui-acp")]
    binary: PathBuf,
    #[arg(long = "require-tool")]
    required_tools: Vec<String>,
    /// Fetch model/default/auth metadata only; creates no chat and invokes no model.
    #[arg(long)]
    metadata_only: bool,
    /// One text file per turn. Files are read for preview; no default test prompts.
    #[arg(long = "prompt-file")]
    prompt_files: Vec<PathBuf>,
    /// Actually contact the endpoint. Obtain specific approval first for production.
    #[arg(long)]
    execute: bool,
    #[arg(long)]
    allow_insecure_http: bool,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let s = Smoke::parse();
    match run(s).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(s: Smoke) -> Result<(), String> {
    if s.metadata_only && !s.prompt_files.is_empty() {
        return Err("Choose metadata-only or prompt files, not both".into());
    }
    if !s.metadata_only && s.prompt_files.is_empty() {
        return Err("Supply --metadata-only or at least one --prompt-file".into());
    }
    let prompts = s
        .prompt_files
        .iter()
        .map(|p| std::fs::read_to_string(p).map_err(|_| "Cannot read prompt file".to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    if prompts.iter().any(|p| p.trim().is_empty()) {
        return Err("Prompt files must contain text".into());
    }
    // Parse/validate even in preview, without ever printing credentials or loading
    // the env file. A dummy key is sufficient for offline URL/config validation.
    let preview_args = Args {
        url: Some(s.url.clone()),
        model: Some(s.model.clone()),
        env_file: None,
        required_tools: s.required_tools.clone(),
        timeout_secs: None,
        buzz_auto_publish: Some(false),
        buzz_thread_chats: Some(false),
        memory_default: None,
        allow_insecure_http: Some(s.allow_insecure_http),
        ..Default::default()
    };
    let preview = Config::resolve(
        preview_args,
        &std::collections::HashMap::from([(
            "OPENWEBUI_API_KEY".into(),
            "preview-placeholder".into(),
        )]),
        &Default::default(),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "Endpoint: {}\nWorkspace model: {}\nRequired tools: {:?}",
        preview.url, preview.model, preview.required_tools
    );
    println!(
        "Metadata GETs: /api/v1/models/model?id=<model>, /api/models, /api/v1/tools/, /api/config, /api/v1/users/user/status, /api/v1/users/permissions, /api/v1/users/user/settings; /api/v1/terminals/ only if the model specifies a terminal."
    );
    if !s.metadata_only {
        println!(
            "Then: create one retained chat; run each prompt through /api/chat/completions; poll /api/tasks/chat/<chat>; read/update the saved chat. On failure/cancellation, attempt /api/tasks/chat/<chat>/stop. Buzz publishing is disabled."
        );
        for (i, p) in prompts.iter().enumerate() {
            println!("\nTurn {}:\n{}", i + 1, p);
        }
    }
    if !s.execute {
        println!(
            "\nPREVIEW ONLY: no environment file loaded, process spawned, or network request made. Production execution requires approval of these specific actions before adding --execute."
        );
        return Ok(());
    }
    if s.metadata_only {
        let config = Config::load(Args {
            url: Some(s.url),
            model: Some(s.model),
            env_file: Some(s.env_file),
            required_tools: s.required_tools,
            timeout_secs: None,
            buzz_auto_publish: Some(false),
            buzz_thread_chats: Some(false),
            memory_default: None,
            allow_insecure_http: Some(s.allow_insecure_http),
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;
        let api = OpenWebUi::new(Arc::new(config)).map_err(|e| e.to_string())?;
        let (model, defaults) = api.defaults().await.map_err(|e| e.to_string())?;
        println!(
            "Validated Workspace model: {}\nDefault tools: {:?}\nFeatures: {:?}\nTerminal: {:?}\nDefault filters: {:?}",
            model.id,
            defaults.tool_ids,
            defaults.features,
            defaults.terminal_id,
            defaults.filter_ids
        );
        return Ok(());
    }
    let mut config = AcpAgentConfig::new(s.binary).args([
        "--url".into(),
        s.url,
        "--model".into(),
        s.model,
        "--env-file".into(),
        s.env_file.to_string_lossy().into_owned(),
        "--buzz-auto-publish".into(),
        "false".into(),
        "--buzz-thread-chats".into(),
        "false".into(),
    ]);
    if s.allow_insecure_http {
        config = config.args(["--allow-insecure-http", "true"]);
    }
    for tool in s.required_tools {
        config = config.args(["--require-tool".into(), tool]);
    }
    let task = Client
        .builder()
        .on_receive_notification(
            async move |n: SessionNotification, _cx| {
                if let SessionUpdate::AgentMessageChunk(chunk) = n.update
                    && let ContentBlock::Text(t) = chunk.content
                {
                    println!("{}", t.text);
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(
            AcpAgent::new(config),
            |cx: ConnectionTo<Agent>| async move {
                let init = cx
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                if init.protocol_version != ProtocolVersion::V1 {
                    return Err(agent_client_protocol::Error::invalid_params());
                }
                let session = cx
                    .send_request(NewSessionRequest::new(
                        std::env::current_dir().unwrap_or_else(|_| "/".into()),
                    ))
                    .block_task()
                    .await?
                    .session_id;
                for prompt in prompts {
                    let r = cx
                        .send_request(PromptRequest::new(
                            session.clone(),
                            vec![ContentBlock::Text(TextContent::new(prompt))],
                        ))
                        .block_task()
                        .await?;
                    if r.stop_reason != StopReason::EndTurn {
                        return Err(agent_client_protocol::Error::internal_error());
                    }
                }
                Ok(())
            },
        );
    tokio::select! {
        result=task=>result.map_err(|_|"ACP smoke failed; inspect redacted harness diagnostics".into()),
        _=tokio::signal::ctrl_c()=>Err("Smoke interrupted; inspect the retained Open WebUI chat".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn preview_does_not_open_env_file_spawn_agent_or_contact_endpoint() {
        run(Smoke {
            url: "http://127.0.0.1:1".into(),
            model: "preview-model".into(),
            env_file: "/does/not/exist".into(),
            binary: "/must/not/spawn".into(),
            required_tools: vec![],
            metadata_only: true,
            prompt_files: vec![],
            execute: false,
            allow_insecure_http: true,
        })
        .await
        .unwrap();
    }
}
