use crate::{
    config::BuzzConfig,
    error::{Error, PublishFailure, Result},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use nostr::{
    Event, EventBuilder, EventId, JsonUtil, Keys, Tag,
    hashes::{Hash, sha256},
    nips::nip98::{HttpData, HttpMethod},
};
use reqwest::header;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const TRANSPORT_NOTE: &str = "[Harness transport context]\nYour final answer will be delivered to the current Buzz conversation by the harness. Do not attempt to execute the local buzz CLI or claim that you sent a message yourself. You have the selected Open WebUI model's server-side tools. The ACP client's local working directory and MCP servers are not available to you.\n[/Harness transport context]";

pub const THREAD_NOTE: &str = "[Harness conversation scope]\nThis Open WebUI chat belongs to the current Buzz thread. A Buzz 'Session scope: channel' line describes the ACP transport, not this chat's history. Use the supplied thread context; if referenced notes are missing, ask the human to provide them.\n[/Harness conversation scope]";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ThreadKey {
    channel: Uuid,
    root: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub channel: Uuid,
    pub reply_to: String,
    pub event_id: String,
    pub author: Option<String>,
}

impl Destination {
    /// Buzz supplies each section as a distinct ACP text content block. Never scan
    /// arbitrary event content for routing fields or fall back to a previous turn.
    pub fn parse(blocks: &[String]) -> Result<Self> {
        let contexts: Vec<_> = blocks
            .iter()
            .filter_map(|s| section(s, "context", "Context"))
            .collect();
        if contexts.len() != 1 {
            return Err(Error::BuzzContext);
        }
        let context = contexts[0];
        let channels: Vec<_> = context
            .lines()
            .filter_map(|s| s.strip_prefix("Channel: "))
            .collect();
        if channels.len() != 1 {
            return Err(Error::BuzzContext);
        }
        let display = channels[0];
        let channel = display
            .rsplit_once("(#")
            .and_then(|(_, s)| s.strip_suffix(')'))
            .unwrap_or(display);
        let channel = Uuid::parse_str(channel).map_err(|_| Error::BuzzContext)?;
        let scopes: Vec<_> = context
            .lines()
            .filter_map(|s| s.strip_prefix("Scope: "))
            .collect();
        if !matches!(scopes.as_slice(), ["channel"] | ["thread"]) {
            return Err(Error::BuzzContext);
        }
        let mut anchors = Vec::new();
        for line in context
            .lines()
            .filter(|line| line.starts_with("IMPORTANT: "))
        {
            for (_, tail) in line
                .match_indices("`--reply-to ")
                .map(|(n, _)| (n, &line[n + 12..]))
            {
                let id = tail.split('`').next().ok_or(Error::BuzzContext)?;
                if !event_id(id) {
                    return Err(Error::BuzzContext);
                }
                anchors.push(id.to_owned());
            }
        }
        if anchors.len() != 1 {
            return Err(Error::BuzzContext);
        }
        let events: Vec<_> = blocks.iter().filter(|s| is_current_event(s)).collect();
        if events.len() != 1 {
            return Err(Error::BuzzContext);
        }
        let block = events[0];
        // Buzz may merge several triggers into one text block after batching or
        // cancellation. Their Content is free-form, so a textual split cannot
        // safely establish every author. v1 accepts one ordinary event only.
        if !(block.starts_with("<buzz-event ")
            || block.starts_with("[Event]\n")
            || block.starts_with("[Event "))
        {
            return Err(Error::BuzzContext);
        }
        // Check only the initial trusted event header, before arbitrary Content.
        let header = block
            .lines()
            .skip(1)
            .take_while(|s| !s.starts_with("Content: "))
            .collect::<Vec<_>>();
        let event_ids: Vec<_> = header
            .iter()
            .filter_map(|s| s.strip_prefix("Event ID: "))
            .collect();
        if event_ids.len() != 1 || !event_id(event_ids[0]) {
            return Err(Error::BuzzContext);
        }
        let roots: Vec<_> = context
            .lines()
            .filter_map(|s| s.strip_prefix("Thread root: "))
            .collect();
        // Human channel triggers become new roots; thread replies use the
        // canonical root supplied by Buzz. Older envelopes may omit Thread root.
        if (scopes[0] == "channel" && !anchors[0].eq_ignore_ascii_case(event_ids[0]))
            || roots.len() > 1
            || roots
                .first()
                .is_some_and(|root| !event_id(root) || !root.eq_ignore_ascii_case(&anchors[0]))
        {
            return Err(Error::BuzzContext);
        }
        let event_channels: Vec<_> = header
            .iter()
            .filter_map(|s| s.strip_prefix("Channel: "))
            .collect();
        if event_channels.len() != 1 || event_channels[0] != display {
            return Err(Error::BuzzContext);
        }
        let authors: Vec<_> = header
            .iter()
            .filter_map(|s| s.strip_prefix("From: "))
            .collect();
        let author = match authors.as_slice() {
            [from] => from
                .strip_suffix(')')
                .and_then(|s| s.rsplit_once("hex: "))
                .map(|(_, key)| key)
                .filter(|key| event_id(key))
                .map(str::to_ascii_lowercase),
            _ => None,
        };
        Ok(Self {
            channel,
            reply_to: anchors.remove(0).to_ascii_lowercase(),
            event_id: event_ids[0].to_ascii_lowercase(),
            author,
        })
    }

    /// Match only Buzz's initial transport header, never profile labels, quoted
    /// history, event Content, or workflow-attributed ownership.
    pub fn require_author(&self, public_key: &str) -> Result<()> {
        match &self.author {
            Some(author) if author.eq_ignore_ascii_case(public_key) => Ok(()),
            _ => Err(Error::BuzzAuthor),
        }
    }

    pub fn thread_key(&self) -> ThreadKey {
        ThreadKey {
            channel: self.channel,
            root: self.reply_to.clone(),
        }
    }

    pub fn key(&self) -> String {
        // Stable event identity survives profile/standing-context formatting changes.
        format!("{}\n{}", self.channel, self.event_id)
    }
}

/// Buzz v1 sends standing instructions only on the ACP session's first turn.
/// Copy these explicit sections into a new thread chat, never prior events or
/// conversation history. Match whole transport blocks, not embedded headers.
pub fn standing_context(blocks: &[String]) -> Vec<String> {
    blocks
        .iter()
        .filter(|block| {
            [
                ("base", "Base"),
                ("agent-instructions", "Agent Instructions"),
                ("team-instructions", "Team Instructions"),
                ("core-memory", "Agent Memory — core"),
                ("huddle-instructions", "Huddle Instructions"),
                ("channel-canvas", "Channel Canvas"),
            ]
            .iter()
            .any(|(tag, legacy)| section(block, tag, legacy).is_some())
        })
        .cloned()
        .collect()
}

fn section<'a>(s: &'a str, tag: &str, legacy: &str) -> Option<&'a str> {
    s.strip_prefix(&format!("<{tag}>\n"))
        .and_then(|body| body.strip_suffix(&format!("\n</{tag}>")))
        .or_else(|| s.strip_prefix(&format!("[{legacy}]\n")))
}

fn is_current_event(s: &str) -> bool {
    [
        "<buzz-event ",
        "<buzz-events ",
        "<new-message-arrived-while-you-were-working>\n",
        "<new-request-supersedes-previous>\n",
        "[Event]",
        "[Event ",
        "[Events]",
        "[New message arrived while you were working]",
        "[New request supersedes previous]",
    ]
    .iter()
    .any(|prefix| s.starts_with(prefix))
}

fn event_id(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Narrow outbound adapter: only kind-9 replies. No shell, tool proxy or arbitrary API.
#[derive(Clone)]
pub struct Publisher {
    config: Arc<BuzzConfig>,
    keys: Arc<Keys>,
    auth_tag: Option<Tag>,
    http: reqwest::Client,
}

impl Publisher {
    pub fn new(config: BuzzConfig) -> Result<Self> {
        let keys = Keys::parse(config.private_key.expose())
            .map_err(|_| Error::Config("Invalid Buzz private key".into()))?;
        if config.allowed_authors.contains(&keys.public_key().to_hex()) {
            return Err(Error::Config(
                "Buzz requester allowlist must exclude the agent itself".into(),
            ));
        }
        let auth_tag = if let Some(raw) = &config.auth_tag {
            buzz_sdk::nip_oa::verify_auth_tag(raw.expose(), &keys.public_key())
                .map_err(|_| Error::Config("Invalid Buzz owner attestation".into()))?;
            let parts: Vec<String> = serde_json::from_str(raw.expose())
                .map_err(|_| Error::Config("Invalid Buzz owner attestation".into()))?;
            Some(
                Tag::parse(parts)
                    .map_err(|_| Error::Config("Invalid Buzz owner attestation".into()))?,
            )
        } else {
            None
        };
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| Error::Config("Cannot initialize Buzz HTTP transport".into()))?;
        Ok(Self {
            config: Arc::new(config),
            keys: Arc::new(keys),
            auth_tag,
            http,
        })
    }
    pub fn public_key(&self) -> String {
        self.keys.public_key().to_hex()
    }
    pub fn authorize(&self, destination: &Destination) -> Result<()> {
        if destination
            .author
            .as_ref()
            .is_some_and(|a| self.config.allowed_authors.contains(a))
            && self.config.allowed_channels.contains(&destination.channel)
        {
            Ok(())
        } else {
            Err(Error::BuzzAuthor)
        }
    }
    pub fn sign_reply(&self, destination: &Destination, text: &str) -> Result<Event> {
        self.authorize(destination)?;
        if text.trim().is_empty() || text.len() > 64 * 1024 {
            return Err(Error::Capacity);
        }
        let root = EventId::from_hex(&destination.reply_to).map_err(|_| Error::BuzzContext)?;
        let thread = buzz_sdk::ThreadRef {
            root_event_id: root,
            parent_event_id: root,
        };
        let author = destination.author.as_deref().ok_or(Error::BuzzAuthor)?;
        let mut builder = buzz_sdk::build_message(
            destination.channel,
            text,
            Some(&thread),
            &[author],
            false,
            &[],
            &[],
        )
        .map_err(|_| Error::Publish(PublishFailure::Signing))?;
        if let Some(tag) = &self.auth_tag {
            builder = builder.tags([tag.clone()]);
        }
        builder
            .sign_with_keys(&self.keys)
            .map_err(|_| Error::Publish(PublishFailure::Signing))
    }
    pub async fn publish(
        &self,
        destination: &Destination,
        event: &Event,
        cancel: &CancellationToken,
    ) -> Result<()> {
        self.authorize(destination)?;
        event
            .verify()
            .map_err(|_| Error::State("Pending reply signature is invalid"))?;
        let expected = self.sign_reply(destination, &event.content)?;
        if event.kind != expected.kind
            || event.pubkey != expected.pubkey
            || event.tags != expected.tags
        {
            return Err(Error::State(
                "Pending reply does not match this destination/identity",
            ));
        }
        let send = async {
            for attempt in 0..3 {
                match self.send(event).await {
                    Ok(()) => return Ok(()),
                    Err(Error::Publish(
                        PublishFailure::Network | PublishFailure::Http(429 | 502 | 503 | 504),
                    )) if attempt < 2 => {
                        tokio::time::sleep(Duration::from_millis(200 * (1 << attempt))).await;
                    }
                    Err(e) => return Err(e),
                }
            }
            unreachable!()
        };
        tokio::select! {
            biased;
            _=cancel.cancelled()=>Err(Error::Cancelled),
            r=tokio::time::timeout(Duration::from_secs(30),send)=>r.unwrap_or(Err(Error::Publish(PublishFailure::Timeout))),
        }
    }
    async fn send(&self, event: &Event) -> Result<()> {
        let url = self
            .config
            .relay
            .join("events")
            .map_err(|_| Error::BuzzContext)?;
        let body = event.as_json();
        // Fresh transport auth nonce; the stored message event is never re-signed.
        let data = HttpData::new(
            url.as_str().parse().map_err(|_| Error::BuzzContext)?,
            HttpMethod::POST,
        )
        .payload(sha256::Hash::hash(body.as_bytes()));
        let nonce = Tag::parse(["nonce", &Uuid::new_v4().to_string()])
            .map_err(|_| Error::Publish(PublishFailure::Signing))?;
        let auth = EventBuilder::http_auth(data)
            .tags([nonce])
            .sign_with_keys(&self.keys)
            .map_err(|_| Error::Publish(PublishFailure::Signing))?;
        if let Some(raw) = &self.config.auth_tag {
            buzz_sdk::nip_oa::verify_auth_tag_for_auth_event(
                raw.expose(),
                &self.keys.public_key(),
                auth.created_at.as_secs(),
            )
            .map_err(|_| {
                Error::Config("Buzz owner attestation is expired or not yet valid".into())
            })?;
        }
        let mut header_value =
            header::HeaderValue::from_str(&format!("Nostr {}", STANDARD.encode(auth.as_json())))
                .map_err(|_| Error::Publish(PublishFailure::Signing))?;
        header_value.set_sensitive(true);
        let mut request = self
            .http
            .post(url)
            .header(header::AUTHORIZATION, header_value)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body);
        if let Some(raw) = &self.config.auth_tag {
            let mut tag = header::HeaderValue::from_str(raw.expose())
                .map_err(|_| Error::Config("Invalid Buzz owner attestation header".into()))?;
            tag.set_sensitive(true);
            request = request.header("x-auth-tag", tag);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| Error::Publish(PublishFailure::Network))?;
        if !response.status().is_success() {
            return Err(Error::Publish(PublishFailure::Http(
                response.status().as_u16(),
            )));
        }
        if response.content_length().is_some_and(|n| n > 64 * 1024) {
            return Err(Error::Publish(PublishFailure::Acknowledgement));
        }
        let mut body = Vec::new();
        while let Some(bytes) = response
            .chunk()
            .await
            .map_err(|_| Error::Publish(PublishFailure::Network))?
        {
            if body.len() + bytes.len() > 64 * 1024 {
                return Err(Error::Publish(PublishFailure::Acknowledgement));
            }
            body.extend_from_slice(&bytes);
        }
        #[derive(serde::Deserialize)]
        struct Ack {
            event_id: String,
            accepted: bool,
        }
        let ack: Ack = serde_json::from_slice(&body)
            .map_err(|_| Error::Publish(PublishFailure::Acknowledgement))?;
        if !ack.accepted || ack.event_id != event.id.to_hex() {
            return Err(Error::Publish(PublishFailure::Acknowledgement));
        }
        Ok(())
    }
}
