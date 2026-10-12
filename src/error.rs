/// Never embeds HTTP bodies, transport errors, credentials, or subprocess output.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Local state/configuration: {0}")]
    State(&'static str),
    #[error(
        "Stored state belongs to a different endpoint, account, model, relay, or agent; use a separate state directory"
    )]
    StateScope,
    #[error("Saved chat changed outside this adapter; inspect Open WebUI before continuing")]
    ChatConflict,
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Open WebUI rejected {operation} (HTTP {status}); check the API-key account's access")]
    Http {
        operation: &'static str,
        status: u16,
    },
    #[error("Open WebUI could not complete {0}; the network request failed or timed out")]
    Network(&'static str),
    #[error("Open WebUI returned an invalid response for {0}")]
    InvalidResponse(&'static str),
    #[error("Workspace configuration is incompatible: {0}")]
    Model(String),
    #[error(
        "Tool authentication is required. Sign into Open WebUI as the API-key owner, open {url}, then send the prompt again"
    )]
    Authentication { url: String },
    #[error("Open WebUI reported an unsuccessful assistant turn; inspect the saved chat")]
    TaskFailed,
    #[error("Open WebUI requires an interactive approval; inspect the saved chat before retrying")]
    ApprovalRequired,
    #[error("The Open WebUI turn exceeded its deadline")]
    Timeout,
    #[error("The turn was cancelled")]
    Cancelled,
    #[error(
        "Session is unavailable because a previous remote request has an uncertain outcome; start a new session after inspecting Open WebUI"
    )]
    UncertainSession,
    #[error("Unknown ACP session")]
    UnknownSession,
    #[error("This ACP session already has an active turn")]
    Busy,
    #[error("Only nonempty text prompts are supported in v1")]
    UnsupportedContent,
    #[error("Buzz delivery context is missing, unsupported, or ambiguous")]
    BuzzContext,
    #[error("Buzz request rejected: requester or channel is not explicitly allowed")]
    BuzzAuthor,
    #[error(
        "Buzz publication failed ({0}); the signed reply remains pending for recovery when this thread is next requested"
    )]
    Publish(PublishFailure),
    #[error(
        "This Buzz event was already handled; refusing to rerun the model or create another reply"
    )]
    Duplicate,
    #[error(
        "A configured input, session, history, or delivery limit was reached; inspect state and use a new instance if its retained state is full"
    )]
    Capacity,
}

/// Fixed categories only: never expose remote bodies or OS error text.
#[derive(Debug, thiserror::Error)]
pub enum PublishFailure {
    #[error("could not construct or sign the reply")]
    Signing,
    #[error("relay rejected the reply (HTTP {0}); inspect access and pending delivery")]
    Http(u16),
    #[error("relay request failed; signed reply remains pending")]
    Network,
    #[error("relay acknowledgement is invalid; signed reply remains pending")]
    Acknowledgement,
    #[error("relay publication exceeded its deadline; signed reply remains pending")]
    Timeout,
}

pub type Result<T> = std::result::Result<T, Error>;
