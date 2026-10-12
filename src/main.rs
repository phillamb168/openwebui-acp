use clap::Parser;
use openwebui_acp::{
    acp::Harness,
    config::{Args, Config},
};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let config = match Config::load(Args::parse()) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    // Protocol/HTTP trace logs can contain prompts or tokens. Enable only our
    // explicitly redacted application events, regardless of ambient RUST_LOG.
    let level = config.log_level.rsplit('=').next().unwrap_or("info");
    let level = match level {
        "off" | "error" | "warn" | "info" | "debug" | "trace" => level,
        _ => "info",
    };
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(format!("off,openwebui_acp={level}"))
        .init();
    let harness = match Harness::new(config) {
        Ok(harness) => harness,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    tracing::info!("ACP v1 harness ready; Open WebUI chats are retained");
    let result = tokio::select! {
        r = harness.clone().serve() => r,
        _ = shutdown_signal() => Ok(()),
    };
    harness.shutdown().await;
    if result.is_err() {
        // SDK errors may contain raw input; do not format them.
        eprintln!("ACP transport closed with an error");
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
