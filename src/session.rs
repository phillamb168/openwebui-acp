use std::{
    collections::{BTreeMap, HashSet},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    openwebui::OpenWebUi,
    projection::{Defaults, Features},
};

#[derive(Clone, Deserialize, Serialize)]
pub struct Chat {
    pub id: String,
    pub chat: ChatData,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ChatData {
    pub title: String,
    pub models: Vec<String>,
    pub history: History,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub current_id: Option<String>,
    pub messages: BTreeMap<String, Message>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub children_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<bool>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Serialize)]
pub struct CompletionMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Deliberately no flatten/extra fields, tools, params, skill_ids, or provider ID.
#[derive(Serialize)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<CompletionMessage>,
    pub stream: bool,
    pub chat_id: String,
    pub id: String,
    pub session_id: String,
    // The server uses this envelope when rebuilding the saved assistant node.
    // Saving the tree first does not replace these completion-time references.
    pub parent_id: Option<String>,
    pub user_message: Message,
    pub features: Features,
    pub tool_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub filter_ids: Vec<String>,
    pub background_tasks: BackgroundTasks,
}

#[derive(Serialize)]
pub struct BackgroundTasks {
    pub title_generation: bool,
    pub tags_generation: bool,
    pub follow_up_generation: bool,
}

#[derive(Deserialize)]
struct Started {
    status: bool,
    task_ids: Vec<String>,
    chat_id: String,
}

#[derive(Deserialize)]
struct Tasks {
    task_ids: Vec<String>,
}

/// Only the small checkpoint is persisted. Chat bodies stay in Open WebUI.
pub struct Session {
    pub chat: Option<Chat>,
    pub checkpoint: crate::state::Checkpoint,
    store: Option<(std::sync::Arc<crate::state::Store>, String)>,
    remote_pending: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            chat: None,
            checkpoint: crate::state::Checkpoint {
                remote_session_id: Uuid::new_v4().to_string(),
                ..Default::default()
            },
            store: None,
            remote_pending: false,
        }
    }
}

impl Session {
    pub fn attach(store: std::sync::Arc<crate::state::Store>, key: String) -> Result<Self> {
        let checkpoint = store
            .checkpoint(&key)?
            .unwrap_or_else(|| Self::default().checkpoint);
        if Uuid::parse_str(&checkpoint.remote_session_id).is_err() {
            return Err(Error::State("Invalid remote session identity"));
        }
        let s = Self {
            chat: None,
            checkpoint,
            store: Some((store, key)),
            remote_pending: false,
        };
        s.save()?;
        Ok(s)
    }
    fn save(&self) -> Result<()> {
        if let Some((store, key)) = &self.store {
            store.save_checkpoint(key, self.checkpoint.clone())?;
        }
        Ok(())
    }
    pub fn fresh(&self) -> bool {
        self.checkpoint.chat_id.is_none()
    }
    pub async fn run(
        &mut self,
        api: &OpenWebUi,
        prompt: &str,
        cancel: &CancellationToken,
    ) -> Result<String> {
        self.run_operation(api, prompt, &Uuid::new_v4().to_string(), cancel)
            .await
    }
    pub async fn run_operation(
        &mut self,
        api: &OpenWebUi,
        prompt: &str,
        operation: &str,
        cancel: &CancellationToken,
    ) -> Result<String> {
        use crate::state::Phase;
        if !matches!(self.checkpoint.phase, Phase::Idle | Phase::Completed) {
            return Err(Error::UncertainSession);
        }
        if prompt.len() > crate::transport::MAX_PROMPT_BYTES {
            return Err(Error::Capacity);
        }
        let result = tokio::select! {
            biased;
            _=cancel.cancelled()=>Err(Error::Cancelled),
            r=tokio::time::timeout(api.config.turn_timeout,self.run_inner(api,prompt,operation))=>r.unwrap_or(Err(Error::Timeout)),
        };
        if result.is_err() && self.remote_pending {
            self.stop(api).await;
        }
        result
    }
    async fn refresh(&mut self, api: &OpenWebUi) -> Result<()> {
        if let Some(id) = &self.checkpoint.chat_id {
            let chat: Chat = api
                .get(api.endpoint(&["api", "v1", "chats", id]), "chat refresh")
                .await?;
            if chat.id != *id || chat.chat.models != [api.config.model.clone()] {
                return Err(Error::ChatConflict);
            }
            if chat.chat.history.messages.len() > 4096 {
                return Err(Error::Capacity);
            }
            let expected = match self.checkpoint.phase {
                crate::state::Phase::Idle | crate::state::Phase::Completed => {
                    &self.checkpoint.last_completed
                }
                _ => &self.checkpoint.assistant_id,
            };
            if chat.chat.history.current_id != *expected {
                return Err(Error::ChatConflict);
            }
            if matches!(
                self.checkpoint.phase,
                crate::state::Phase::Idle | crate::state::Phase::Completed
            ) && self
                .checkpoint
                .history_hash
                .as_ref()
                .is_some_and(|expected| {
                    history_hash(&chat.chat.history).as_ref().ok() != Some(expected)
                })
            {
                return Err(Error::ChatConflict);
            }
            self.chat = Some(chat);
        }
        Ok(())
    }
    async fn run_inner(
        &mut self,
        api: &OpenWebUi,
        prompt: &str,
        operation: &str,
    ) -> Result<String> {
        use crate::state::Phase;
        let (model, defaults) = api.defaults().await?;
        self.refresh(api).await?;
        let mut data = self
            .chat
            .as_ref()
            .map(|c| c.chat.clone())
            .unwrap_or_else(|| ChatData {
                title: "ACP conversation".into(),
                models: vec![model.id.clone()],
                history: History::default(),
                extra: BTreeMap::new(),
            });
        if data.history.messages.len() > 4094 {
            return Err(Error::Capacity);
        }
        let (user_id, assistant_id) = data.append(
            prompt,
            &model.id,
            &model.name,
            self.checkpoint.last_completed.as_deref(),
        )?;
        let messages = data.history.completion_messages(&user_id)?;
        let user_message = data.history.messages[&user_id].clone();
        // Check limits before any saved-chat mutation.
        if serde_json::to_vec(&data)
            .map_err(|_| Error::Capacity)?
            .len()
            > 16 * 1024 * 1024
        {
            return Err(Error::Capacity);
        }
        self.checkpoint.operation_id = Some(operation.into());
        self.checkpoint.user_id = Some(user_id);
        self.checkpoint.assistant_id = Some(assistant_id.clone());
        self.checkpoint.parent_id = self.checkpoint.last_completed.clone();
        self.checkpoint.phase = Phase::Saving;
        self.save()?;
        let saved: Chat = if let Some(id) = &self.checkpoint.chat_id {
            api.post(
                api.endpoint(&["api", "v1", "chats", id]),
                &json!({"chat":data}),
                "chat update",
            )
            .await?
        } else {
            api.post(
                api.endpoint(&["api", "v1", "chats", "new"]),
                &json!({"chat":data}),
                "chat creation",
            )
            .await?
        };
        if saved.id.is_empty()
            || saved.id.len() > 512
            || self
                .checkpoint
                .chat_id
                .as_ref()
                .is_some_and(|old| old != &saved.id)
            || !saved.chat.history.messages.contains_key(&assistant_id)
            || saved.chat.history.current_id.as_deref() != Some(&assistant_id)
        {
            return Err(Error::InvalidResponse("saved chat tree"));
        }
        let chat_id = saved.id.clone();
        self.checkpoint.chat_id = Some(chat_id.clone());
        self.chat = Some(saved);
        self.checkpoint.phase = Phase::Saved;
        self.save()?;
        let request = completion_request(
            &model.id,
            &chat_id,
            &assistant_id,
            &self.checkpoint.remote_session_id,
            user_message,
            messages,
            defaults,
        );
        self.checkpoint.phase = Phase::Submitted;
        self.save()?; // durable BEFORE submission, including a lost HTTP acknowledgement
        self.remote_pending = true;
        let started: Started = api
            .post(
                api.endpoint(&["api", "chat", "completions"]),
                &request,
                "completion submission",
            )
            .await?;
        if !started.status
            || started.chat_id != chat_id
            || started.task_ids.iter().any(String::is_empty)
        {
            return Err(Error::InvalidResponse(
                "asynchronous completion acknowledgement",
            ));
        }
        self.wait_result(api).await
    }
    async fn wait_result(&mut self, api: &OpenWebUi) -> Result<String> {
        let chat_id = self
            .checkpoint
            .chat_id
            .clone()
            .ok_or(Error::UncertainSession)?;
        loop {
            let tasks: Tasks = api
                .get(
                    api.endpoint(&["api", "tasks", "chat", &chat_id]),
                    "chat tasks",
                )
                .await?;
            if tasks.task_ids.is_empty() {
                self.refresh(api).await?;
                if let Some(answer) = self.completed_answer()? {
                    self.checkpoint.last_completed = self.checkpoint.assistant_id.clone();
                    self.checkpoint.phase = crate::state::Phase::Completed;
                    self.checkpoint.history_hash = Some(history_hash(
                        &self
                            .chat
                            .as_ref()
                            .ok_or(Error::UncertainSession)?
                            .chat
                            .history,
                    )?);
                    self.remote_pending = false;
                    self.save()?;
                    return Ok(answer);
                }
            }
            tokio::time::sleep(api.config.poll_interval).await;
        }
    }
    fn completed_answer(&self) -> Result<Option<String>> {
        let chat = self.chat.as_ref().ok_or(Error::UncertainSession)?;
        let assistant = self
            .checkpoint
            .assistant_id
            .as_ref()
            .ok_or(Error::UncertainSession)?;
        let user_id = self
            .checkpoint
            .user_id
            .as_ref()
            .ok_or(Error::UncertainSession)?;
        let message = chat
            .chat
            .history
            .messages
            .get(assistant)
            .ok_or(Error::InvalidResponse("completed assistant message"))?;
        if message.id != *assistant || message.role != "assistant" {
            return Err(Error::InvalidResponse("completed assistant identity"));
        }
        if message
            .extra
            .get("error")
            .is_some_and(|v| !v.is_null() && v != &Value::Bool(false))
        {
            return Err(Error::TaskFailed);
        }
        if message
            .extra
            .get("output")
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().any(|v| v["status"] == "requires_approval"))
        {
            return Err(Error::ApprovalRequired);
        }
        if message.done != Some(true) {
            return Ok(None);
        }
        let user = chat.chat.history.messages.get(user_id);
        if message.parent_id.as_ref() != Some(user_id)
            || !user.is_some_and(|u| {
                u.id == *user_id
                    && u.role == "user"
                    && u.parent_id == self.checkpoint.parent_id
                    && u.children_ids.contains(assistant)
            })
        {
            return Err(Error::InvalidResponse("completed message tree"));
        }
        if message.content.trim().is_empty() {
            return Err(Error::InvalidResponse("nonempty final assistant text"));
        }
        if message.content.len() > 1024 * 1024 {
            return Err(Error::Capacity);
        }
        Ok(Some(message.content.clone()))
    }
    /// Read-only reconciliation of a previously submitted turn. Never POST completions.
    pub async fn recover(
        &mut self,
        api: &OpenWebUi,
        cancel: &CancellationToken,
    ) -> Result<Option<String>> {
        use crate::state::Phase;
        let submitted = self.checkpoint.phase == Phase::Submitted;
        self.remote_pending = submitted;
        let recovery = async {
            match self.checkpoint.phase {
                Phase::Idle => {
                    self.refresh(api).await?;
                    Ok(None)
                }
                Phase::Completed => {
                    self.refresh(api).await?;
                    self.completed_answer()
                }
                Phase::Submitted => self.wait_result(api).await.map(Some),
                Phase::Saving | Phase::Saved | Phase::Interrupted => Err(Error::UncertainSession),
            }
        };
        let result = tokio::select! {
            biased;
            _=cancel.cancelled()=>Err(Error::Cancelled),
            r=tokio::time::timeout(api.config.turn_timeout,recovery)=>r.unwrap_or(Err(Error::UncertainSession)),
        };
        if matches!(result, Err(Error::Cancelled)) && submitted {
            self.stop(api).await;
        }
        result
    }
    pub fn visible_history(&self) -> Result<Vec<(String, String)>> {
        let Some(chat) = &self.chat else {
            return Ok(Vec::new());
        };
        let Some(id) = &self.checkpoint.last_completed else {
            return Ok(Vec::new());
        };
        // Validate ancestry, then expose only visible text, never structured output/reasoning.
        chat.chat.history.completion_messages(id)?;
        let mut next = Some(id.as_str());
        let mut messages = Vec::new();
        while let Some(id) = next {
            let m = &chat.chat.history.messages[id];
            messages.push((m.role.clone(), m.content.clone()));
            next = m.parent_id.as_deref();
        }
        messages.reverse();
        Ok(messages)
    }
    pub async fn stop(&mut self, api: &OpenWebUi) {
        if !self.remote_pending {
            return;
        }
        self.checkpoint.phase = crate::state::Phase::Interrupted;
        if self.save().is_err() {
            tracing::warn!("Cannot record cancellation state; recovery will be required");
        }
        if let Some(id) = &self.checkpoint.chat_id {
            let stopped = tokio::time::timeout(api.config.stop_timeout, async {
                let stopped: Value = api
                    .post(
                        api.endpoint(&["api", "tasks", "chat", id, "stop"]),
                        &json!({}),
                        "chat cancellation",
                    )
                    .await?;
                if stopped.get("status").and_then(Value::as_bool) != Some(true) {
                    return Err(Error::UncertainSession);
                }
                let tasks: Tasks = api
                    .get(
                        api.endpoint(&["api", "tasks", "chat", id]),
                        "cancelled chat tasks",
                    )
                    .await?;
                if tasks.task_ids.is_empty() {
                    Ok(())
                } else {
                    Err(Error::UncertainSession)
                }
            })
            .await;
            if matches!(stopped, Ok(Ok(()))) {
                self.remote_pending = false;
            } else {
                tracing::warn!("Remote cancellation could not be confirmed; session quarantined");
            }
        }
    }
}
impl ChatData {
    fn append(
        &mut self,
        prompt: &str,
        model: &str,
        name: &str,
        parent: Option<&str>,
    ) -> Result<(String, String)> {
        if let Some(id) = parent {
            let p = self
                .history
                .messages
                .get(id)
                .ok_or(Error::InvalidResponse("conversation parent"))?;
            if p.role != "assistant" || p.done != Some(true) {
                return Err(Error::InvalidResponse("conversation parent state"));
            }
        }
        let user_id = Uuid::new_v4().to_string();
        let assistant_id = Uuid::new_v4().to_string();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if let Some(id) = parent {
            self.history
                .messages
                .get_mut(id)
                .expect("validated parent")
                .children_ids
                .push(user_id.clone());
        }
        self.history.messages.insert(
            user_id.clone(),
            Message {
                id: user_id.clone(),
                role: "user".into(),
                content: prompt.into(),
                parent_id: parent.map(str::to_owned),
                children_ids: vec![assistant_id.clone()],
                done: None,
                extra: BTreeMap::from([
                    ("timestamp".into(), json!(timestamp)),
                    ("models".into(), json!([model])),
                ]),
            },
        );
        self.history.messages.insert(
            assistant_id.clone(),
            Message {
                id: assistant_id.clone(),
                role: "assistant".into(),
                content: String::new(),
                parent_id: Some(user_id.clone()),
                children_ids: Vec::new(),
                done: Some(false),
                extra: BTreeMap::from([
                    ("timestamp".into(), json!(timestamp)),
                    ("model".into(), json!(model)),
                    ("modelName".into(), json!(name)),
                    ("modelIdx".into(), json!(0)),
                ]),
            },
        );
        self.history.current_id = Some(assistant_id.clone());
        // Avoid writing a stale legacy flat history alongside the authoritative tree.
        self.extra.remove("messages");
        Ok((user_id, assistant_id))
    }
}

impl History {
    pub fn completion_messages(&self, last: &str) -> Result<Vec<CompletionMessage>> {
        let mut next = Some(last);
        let mut seen = HashSet::new();
        let mut messages = Vec::new();
        while let Some(id) = next {
            if !seen.insert(id) {
                return Err(Error::InvalidResponse("cyclic message tree"));
            }
            let m = self
                .messages
                .get(id)
                .ok_or(Error::InvalidResponse("message tree ancestor"))?;
            if m.id != id || !matches!(m.role.as_str(), "user" | "assistant") {
                return Err(Error::InvalidResponse("message tree role or ID"));
            }
            let output = m
                .extra
                .get("output")
                .filter(|v| m.role == "assistant" && v.as_array().is_some_and(|a| !a.is_empty()))
                .cloned();
            messages.push(CompletionMessage {
                role: m.role.clone(),
                content: output.is_none().then(|| m.content.clone()),
                model: output
                    .as_ref()
                    .and_then(|_| m.extra.get("model"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                output,
            });
            next = m.parent_id.as_deref();
        }
        messages.reverse();
        Ok(messages)
    }
}

fn completion_request(
    model: &str,
    chat: &str,
    assistant: &str,
    session: &str,
    user_message: Message,
    messages: Vec<CompletionMessage>,
    d: Defaults,
) -> CompletionRequest {
    CompletionRequest {
        model: model.into(),
        messages,
        stream: true,
        chat_id: chat.into(),
        id: assistant.into(),
        session_id: session.into(),
        parent_id: user_message.parent_id.clone(),
        user_message,
        features: d.features,
        tool_ids: d.tool_ids,
        terminal_id: d.terminal_id,
        filter_ids: d.filter_ids,
        background_tasks: BackgroundTasks {
            title_generation: false,
            tags_generation: false,
            follow_up_generation: false,
        },
    }
}

fn history_hash(history: &History) -> Result<String> {
    use sha2::{Digest, Sha256};
    // Keep the rc.1 checkpoint representation (including done:null) stable even
    // though current Open WebUI rejects null on the write API. Wire formatting
    // must not make an unchanged saved conversation fail its integrity check.
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct FingerprintMessage<'a> {
        id: &'a str,
        role: &'a str,
        content: &'a str,
        parent_id: &'a Option<String>,
        children_ids: &'a [String],
        done: Option<bool>,
        #[serde(flatten)]
        extra: &'a BTreeMap<String, Value>,
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct FingerprintHistory<'a> {
        current_id: &'a Option<String>,
        messages: BTreeMap<&'a str, FingerprintMessage<'a>>,
    }
    let stable = FingerprintHistory {
        current_id: &history.current_id,
        messages: history
            .messages
            .iter()
            .map(|(id, m)| {
                (
                    id.as_str(),
                    FingerprintMessage {
                        id: &m.id,
                        role: &m.role,
                        content: &m.content,
                        parent_id: &m.parent_id,
                        children_ids: &m.children_ids,
                        done: m.done,
                        extra: &m.extra,
                    },
                )
            })
            .collect(),
    };
    // Include tool output as well as visible content; omit unrelated chat UI metadata.
    let bytes =
        serde_json::to_vec(&stable).map_err(|_| Error::InvalidResponse("history fingerprint"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod fingerprint_tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn prior_checkpoint_hash_survives_wire_format_change() {
        // Exact rc.1 serialization; preserve field order as well as null values.
        let old = r#"{"currentId":"u","messages":{"u":{"id":"u","role":"user","content":"hello","parentId":null,"childrenIds":[],"done":null,"timestamp":1}}}"#;
        let mut history: History = serde_json::from_str(old).unwrap();
        assert_eq!(
            history_hash(&history).unwrap(),
            format!("{:x}", Sha256::digest(old.as_bytes()))
        );
        let wire = serde_json::to_value(&history).unwrap();
        assert!(wire["messages"]["u"].get("done").is_none());
        history.messages.get_mut("u").unwrap().content = "edited".into();
        assert_ne!(
            history_hash(&history).unwrap(),
            format!("{:x}", Sha256::digest(old.as_bytes()))
        );
    }
}
