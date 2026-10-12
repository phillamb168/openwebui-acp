use crate::{
    error::{Error, Result},
    secure_file,
};
use clap::Parser;
use reqwest::Url;
use std::{collections::HashMap, fmt, path::PathBuf, time::Duration};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, Default, Parser)]
#[command(
    name = "openwebui-acp",
    version,
    about = "Open WebUI Workspace models over ACP v1 stdio"
)]
pub struct Args {
    #[arg(long)]
    pub url: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    /// Explicit, owner-only configuration file; never loaded automatically.
    #[arg(long)]
    pub env_file: Option<PathBuf>,
    /// Read the Open WebUI key from an owner-only file, never command-line text.
    #[arg(long)]
    pub api_key_file: Option<PathBuf>,
    #[arg(long = "require-tool")]
    pub required_tools: Vec<String>,
    #[arg(long)]
    pub timeout_secs: Option<u64>,
    /// Explicitly permit unencrypted HTTP/WS for a local or private test network.
    #[arg(long, action = clap::ArgAction::Set)]
    pub allow_insecure_http: Option<bool>,
    /// Enable private, exclusively locked state for restart continuity.
    #[arg(long)]
    pub state_dir: Option<PathBuf>,
    #[arg(long, action = clap::ArgAction::Set)]
    pub buzz_auto_publish: Option<bool>,
    /// Default true when Buzz publication is enabled; independent of Buzz's global policy.
    #[arg(long, action = clap::ArgAction::Set)]
    pub buzz_thread_chats: Option<bool>,
    #[arg(long)]
    pub buzz_relay: Option<String>,
    #[arg(long)]
    pub buzz_key_file: Option<PathBuf>,
    /// Exact requester signing keys; repeat for teammates and delegating agents.
    #[arg(long = "buzz-allow-author")]
    pub buzz_allowed_authors: Vec<String>,
    /// Exact channel UUIDs; repeat for each authorized channel.
    #[arg(long = "buzz-allow-channel")]
    pub buzz_allowed_channels: Vec<String>,
    /// Browser-verified fallback only when account and global memory defaults are absent.
    #[arg(long, action = clap::ArgAction::Set)]
    pub memory_default: Option<bool>,
}

#[derive(Clone)]
pub struct Secret(Zeroizing<String>);
impl Secret {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

#[derive(Clone, Debug)]
pub struct BuzzConfig {
    pub relay: Url,
    pub private_key: Secret,
    pub auth_tag: Option<Secret>,
    pub allowed_authors: Vec<String>,
    pub allowed_channels: Vec<Uuid>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub url: Url,
    pub api_key: Secret,
    pub model: String,
    pub required_tools: Vec<String>,
    pub turn_timeout: Duration,
    pub request_timeout: Duration,
    pub poll_interval: Duration,
    pub stop_timeout: Duration,
    pub allow_insecure_http: bool,
    pub state_dir: Option<PathBuf>,
    pub buzz_auto_publish: bool,
    pub buzz_thread_chats: bool,
    pub buzz: Option<BuzzConfig>,
    pub log_level: String,
    pub memory_default: Option<bool>,
}

impl Config {
    pub fn load(args: Args) -> Result<Self> {
        let file = if let Some(path) = &args.env_file {
            let text = Zeroizing::new(secure_file::read_secret(path)?);
            dotenvy::from_read_iter(text.as_bytes())
                .collect::<std::result::Result<HashMap<_, _>, _>>()
                .map_err(|_| Error::Config("Invalid environment file".into()))?
        } else {
            HashMap::new()
        };
        let mut config = Self::resolve(args, &std::env::vars().collect(), &file)?;
        // Explicit credential paths are resolved by load, while resolve is pure for tests/preview.
        if let Some(path) = config.api_key.expose().strip_prefix("file:") {
            config.api_key = Secret::new(
                secure_file::read_secret(std::path::Path::new(path))?
                    .trim()
                    .to_owned(),
            );
        }
        if let Some(buzz) = &mut config.buzz
            && let Some(path) = buzz.private_key.expose().strip_prefix("file:")
        {
            buzz.private_key = Secret::new(
                secure_file::read_secret(std::path::Path::new(path))?
                    .trim()
                    .to_owned(),
            );
        }
        if config.api_key.expose().is_empty() {
            return Err(Error::Config("Open WebUI key is empty".into()));
        }
        Ok(config)
    }

    /// CLI > environment > explicit file. This function performs no I/O.
    pub fn resolve(
        args: Args,
        env: &HashMap<String, String>,
        file: &HashMap<String, String>,
    ) -> Result<Self> {
        let value = |name: &str| env.get(name).or_else(|| file.get(name)).cloned();
        let boolean = |name: &str| -> Result<bool> {
            match value(name).as_deref() {
                None | Some("false") | Some("0") => Ok(false),
                Some("true") | Some("1") => Ok(true),
                _ => Err(Error::Config(format!("{name} must be true or false"))),
            }
        };
        let insecure = args
            .allow_insecure_http
            .map(Ok)
            .unwrap_or_else(|| boolean("OPENWEBUI_ACP_ALLOW_INSECURE_HTTP"))?;
        let url = base_url(
            &args
                .url
                .or_else(|| value("OPENWEBUI_URL"))
                .ok_or_else(|| Error::Config("Set --url or OPENWEBUI_URL".into()))?,
            insecure,
            false,
        )?;
        let model = args
            .model
            .or_else(|| value("OPENWEBUI_MODEL"))
            .filter(|s| !s.trim().is_empty() && s.len() <= 512)
            .ok_or_else(|| {
                Error::Config("Set --model or OPENWEBUI_MODEL (maximum 512 bytes)".into())
            })?;
        let key = secret_source(args.api_key_file, "OPENWEBUI_API_KEY", env, file)?;
        let timeout = args
            .timeout_secs
            .map(Ok)
            .unwrap_or_else(|| {
                value("OPENWEBUI_ACP_TIMEOUT_SECS")
                    .map(|s| s.parse::<u64>())
                    .unwrap_or(Ok(600))
            })
            .map_err(|_| Error::Config("Timeout must be an integer".into()))?;
        if !(1..=3600).contains(&timeout) {
            return Err(Error::Config(
                "Timeout must be between 1 and 3600 seconds".into(),
            ));
        }
        if value("OPENWEBUI_ACP_KEEP_OPENWEBUI_CHATS").is_some()
            && !boolean("OPENWEBUI_ACP_KEEP_OPENWEBUI_CHATS")?
        {
            return Err(Error::Config(
                "Saved Open WebUI chats are retained; automatic deletion is unsupported".into(),
            ));
        }
        let publish = args
            .buzz_auto_publish
            .map(Ok)
            .unwrap_or_else(|| boolean("OPENWEBUI_ACP_BUZZ_AUTO_PUBLISH"))?;
        let threads = args.buzz_thread_chats.map(Ok).unwrap_or_else(|| {
            if value("OPENWEBUI_ACP_BUZZ_THREAD_CHATS").is_some() {
                boolean("OPENWEBUI_ACP_BUZZ_THREAD_CHATS")
            } else {
                Ok(publish)
            }
        })?;
        let list = |cli: Vec<String>, name: &str| -> Vec<String> {
            if cli.is_empty() {
                value(name)
                    .unwrap_or_default()
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            } else {
                cli
            }
        };
        let authors = list(
            args.buzz_allowed_authors,
            "OPENWEBUI_ACP_BUZZ_ALLOWED_AUTHORS",
        );
        let channels = list(
            args.buzz_allowed_channels,
            "OPENWEBUI_ACP_BUZZ_ALLOWED_CHANNELS",
        );
        let buzz = if publish || threads {
            if authors.is_empty()
                || channels.is_empty()
                || authors.len() > 256
                || channels.len() > 256
            {
                return Err(Error::Config(
                    "Buzz requires explicit requester and channel allowlists (1–256 entries each)"
                        .into(),
                ));
            }
            let authors = authors
                .into_iter()
                .map(|key| {
                    if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
                        Err(Error::Config(
                            "Buzz requester keys must be 64 hexadecimal characters".into(),
                        ))
                    } else {
                        Ok(key.to_ascii_lowercase())
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            let channels = channels
                .into_iter()
                .map(|c| {
                    Uuid::parse_str(&c)
                        .map_err(|_| Error::Config("Buzz channels must be UUIDs".into()))
                })
                .collect::<Result<Vec<_>>>()?;
            let relay = base_url(
                &args
                    .buzz_relay
                    .or_else(|| value("BUZZ_RELAY_URL"))
                    .ok_or_else(|| Error::Config("Set --buzz-relay or BUZZ_RELAY_URL".into()))?,
                insecure,
                true,
            )?;
            Some(BuzzConfig {
                relay,
                private_key: secret_source(args.buzz_key_file, "BUZZ_PRIVATE_KEY", env, file)?,
                auth_tag: value("BUZZ_AUTH_TAG").map(Secret::new),
                allowed_authors: authors,
                allowed_channels: channels,
            })
        } else {
            if !authors.is_empty() || !channels.is_empty() {
                return Err(Error::Config(
                    "Buzz allowlists require publication or thread mode".into(),
                ));
            }
            None
        };
        let state_dir = args
            .state_dir
            .or_else(|| value("OPENWEBUI_ACP_STATE_DIR").map(PathBuf::from));
        if state_dir.as_ref().is_some_and(|p| !p.is_absolute()) {
            return Err(Error::Config("--state-dir must be an absolute path".into()));
        }
        Ok(Self {
            url,
            api_key: key,
            model,
            required_tools: list(args.required_tools, "OPENWEBUI_ACP_REQUIRED_TOOLS"),
            turn_timeout: Duration::from_secs(timeout),
            request_timeout: Duration::from_secs(30),
            poll_interval: Duration::from_secs(1),
            stop_timeout: Duration::from_secs(5),
            allow_insecure_http: insecure,
            state_dir,
            buzz_auto_publish: publish,
            buzz_thread_chats: threads,
            buzz,
            log_level: value("RUST_LOG").unwrap_or_else(|| "info".into()),
            memory_default: args.memory_default.or(
                if value("OPENWEBUI_ACP_MEMORY_DEFAULT").is_some() {
                    Some(boolean("OPENWEBUI_ACP_MEMORY_DEFAULT")?)
                } else {
                    None
                },
            ),
        })
    }
}

fn secret_source(
    path: Option<PathBuf>,
    name: &str,
    env: &HashMap<String, String>,
    file: &HashMap<String, String>,
) -> Result<Secret> {
    if let Some(path) = path {
        return Ok(Secret::new(format!("file:{}", path.to_string_lossy())));
    }
    for source in [env, file] {
        if let Some(key) = source.get(name).filter(|v| !v.trim().is_empty()) {
            if key.starts_with("file:") {
                return Err(Error::Config(
                    "Use an explicit key-file option for file credentials".into(),
                ));
            }
            return Ok(Secret::new(key.clone()));
        }
        if let Some(path) = source.get(&format!("{name}_FILE")) {
            if path.is_empty() {
                return Err(Error::Config("Credential file path is empty".into()));
            }
            return Ok(Secret::new(format!("file:{path}")));
        }
    }
    Err(Error::Config(format!("Set {name} or its _FILE setting")))
}

pub fn base_url(raw: &str, insecure: bool, relay: bool) -> Result<Url> {
    let mut url = Url::parse(raw).map_err(|_| Error::Config("Invalid base URL".into()))?;
    if relay {
        match url.scheme() {
            "ws" => {
                let _ = url.set_scheme("http");
            }
            "wss" => {
                let _ = url.set_scheme("https");
            }
            _ => {}
        }
    }
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Config(
            "Use an HTTP(S) base URL without credentials, query, or fragment".into(),
        ));
    }
    if url.scheme() == "http" && !insecure {
        return Err(Error::Config("HTTPS is required; use --allow-insecure-http true only for a trusted local/private test network".into()));
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}
