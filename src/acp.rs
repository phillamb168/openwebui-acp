use crate::{
    buzz::{Destination, Publisher, THREAD_NOTE, TRANSPORT_NOTE, standing_context},
    config::Config,
    error::{Error, Result},
    openwebui::OpenWebUi,
    session::Session,
    state::{DeliveryStatus, EventRecord, MAX_CONVERSATIONS, Scope, Store},
};
use agent_client_protocol::{
    Agent, Client, ConnectionTo, Responder,
    schema::{ProtocolVersion, v1::*},
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio_util::sync::CancellationToken;

struct Slot {
    _cwd: PathBuf,
    _mcp_server_count: usize,
    standing: Mutex<HashMap<uuid::Uuid, Vec<String>>>,
    active: Mutex<Option<CancellationToken>>,
}
type Conversation = Arc<tokio::sync::Mutex<Session>>;
pub struct Harness {
    api: OpenWebUi,
    buzz: Option<Publisher>,
    publish: bool,
    threads: bool,
    store: Arc<Store>,
    initialized: AtomicBool,
    sessions: Mutex<HashMap<String, Arc<Slot>>>,
    conversations: Mutex<HashMap<String, Conversation>>,
    permits: Arc<tokio::sync::Semaphore>,
}
struct ActiveTurn {
    slot: Arc<Slot>,
    cancel: CancellationToken,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
impl Drop for ActiveTurn {
    fn drop(&mut self) {
        self.slot.active.lock().expect("active lock").take();
    }
}

impl Harness {
    pub fn new(config: Config) -> Result<Arc<Self>> {
        let config = Arc::new(config);
        let buzz = config.buzz.clone().map(Publisher::new).transpose()?;
        let store = Store::open(
            config.state_dir.clone(),
            Scope {
                endpoint: config.url.to_string(),
                model: config.model.clone(),
                relay: config.buzz.as_ref().map(|b| b.relay.to_string()),
                agent: buzz.as_ref().map(Publisher::public_key),
            },
        )?;
        Ok(Arc::new(Self {
            api: OpenWebUi::new(config.clone())?,
            buzz,
            publish: config.buzz_auto_publish,
            threads: config.buzz_thread_chats,
            store,
            initialized: AtomicBool::new(false),
            sessions: Mutex::new(HashMap::new()),
            conversations: Mutex::new(HashMap::new()),
            permits: Arc::new(tokio::sync::Semaphore::new(8)),
        }))
    }
    fn insert_slot(&self, id: String, cwd: PathBuf, mcp: Vec<McpServer>) -> Result<()> {
        if !cwd.is_absolute() || cwd.as_os_str().len() > 4096 || mcp.len() > 32 {
            return Err(Error::UnsupportedContent);
        }
        let mut sessions = self.sessions.lock().expect("sessions lock");
        if sessions.contains_key(&id) {
            return Err(Error::Busy);
        }
        if sessions.len() >= MAX_CONVERSATIONS {
            return Err(Error::Capacity);
        }
        sessions.insert(
            id,
            Arc::new(Slot {
                _cwd: cwd,
                _mcp_server_count: mcp.len(),
                standing: Mutex::new(HashMap::new()),
                active: Mutex::new(None),
            }),
        );
        Ok(())
    }
    fn new_session(&self, request: NewSessionRequest) -> Result<NewSessionResponse> {
        if !self.initialized.load(Ordering::Acquire) {
            return Err(Error::Config(
                "ACP initialize must precede session/new".into(),
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.insert_slot(id.clone(), request.cwd.clone(), request.mcp_servers)?;
        if let Err(e) = self.store.create_session(&id, request.cwd) {
            self.sessions.lock().expect("sessions lock").remove(&id);
            return Err(e);
        }
        Ok(NewSessionResponse::new(id))
    }
    fn conversation(&self, key: String) -> Result<Conversation> {
        let mut states = self.conversations.lock().expect("conversations lock");
        if let Some(state) = states.get(&key) {
            return Ok(state.clone());
        }
        if states.len() >= MAX_CONVERSATIONS {
            return Err(Error::Capacity);
        }
        let state = Arc::new(tokio::sync::Mutex::new(Session::attach(
            self.store.clone(),
            key.clone(),
        )?));
        states.insert(key, state.clone());
        Ok(state)
    }
    fn begin(&self, id: &SessionId) -> Result<ActiveTurn> {
        let slot = self
            .sessions
            .lock()
            .expect("sessions lock")
            .get(id.0.as_ref())
            .cloned()
            .ok_or(Error::UnknownSession)?;
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let cancel = CancellationToken::new();
        {
            let mut active = slot.active.lock().expect("active lock");
            if active.is_some() {
                return Err(Error::Busy);
            }
            *active = Some(cancel.clone());
        }
        Ok(ActiveTurn {
            slot,
            cancel,
            _permit: permit,
        })
    }
    fn cancel(&self, id: &SessionId) {
        if let Some(slot) = self
            .sessions
            .lock()
            .expect("sessions lock")
            .get(id.0.as_ref())
            && let Some(cancel) = slot.active.lock().expect("active lock").as_ref()
        {
            cancel.cancel();
        }
    }
    fn conversation_key(
        &self,
        id: &SessionId,
        destination: Option<&Destination>,
    ) -> Result<String> {
        if self.threads {
            let d = destination.ok_or(Error::BuzzContext)?;
            let thread = format!("buzz/{}/{}", d.channel, d.reply_to);
            Ok(if self.store.persistent() {
                thread
            } else {
                format!("acp/{}/{thread}", id.0)
            })
        } else {
            Ok(format!("acp/{}", id.0))
        }
    }
    async fn bind_account(&self) -> Result<()> {
        self.store.bind_account(&self.api.account_id().await?)
    }
    async fn deliver(
        &self,
        record: EventRecord,
        state: &mut Session,
        cancel: &CancellationToken,
    ) -> Result<String> {
        let publisher = self.buzz.as_ref().ok_or(Error::BuzzContext)?;
        publisher.authorize(&record.destination)?;
        let event = match record.status {
            DeliveryStatus::Delivered { .. } => return Err(Error::Duplicate),
            DeliveryStatus::Pending { event } => event,
            DeliveryStatus::Processing => {
                if state.checkpoint.operation_id.as_deref() != Some(&record.operation_id) {
                    return Err(Error::UncertainSession);
                }
                let text = state
                    .recover(&self.api, cancel)
                    .await?
                    .ok_or(Error::UncertainSession)?;
                let event = publisher.sign_reply(&record.destination, &text)?;
                self.store.set_delivery(
                    &record.destination.key(),
                    DeliveryStatus::Pending {
                        event: event.clone(),
                    },
                )?;
                event
            }
        };
        publisher
            .publish(&record.destination, &event, cancel)
            .await?;
        self.store.set_delivery(
            &record.destination.key(),
            DeliveryStatus::Delivered {
                event_id: event.id.to_hex(),
            },
        )?;
        Ok(event.content)
    }
    async fn process(
        &self,
        request: &PromptRequest,
        active: &ActiveTurn,
        cx: &ConnectionTo<Client>,
    ) -> Result<()> {
        let mut blocks = text_blocks(&request.prompt)?;
        let destination = if let Some(buzz) = &self.buzz {
            let destination = Destination::parse(&blocks)?;
            buzz.authorize(&destination)?;
            Some(destination)
        } else {
            None
        };
        // Acknowledge delivered duplicates without output/network. Buzz retries errors.
        // Never accept a duplicate with changed routing headers.
        let existing = if self.publish {
            let d = destination.as_ref().ok_or(Error::BuzzContext)?;
            let existing = self.store.event(&d.key())?;
            if let Some(r) = &existing {
                if r.destination != *d {
                    return Err(Error::BuzzContext);
                }
                if matches!(r.status, DeliveryStatus::Delivered { .. }) {
                    return Ok(());
                }
            }
            existing
        } else {
            None
        };
        tokio::select! { biased; _=active.cancel.cancelled()=>return Err(Error::Cancelled), r=self.bind_account()=>r? };
        let key = self.conversation_key(&request.session_id, destination.as_ref())?;
        let state = self.conversation(key.clone())?;
        let mut state = state.try_lock().map_err(|_| Error::Busy)?;
        if let Some(record) = existing {
            if record.conversation != key {
                return Err(Error::StateScope);
            }
            let answer = self.deliver(record, &mut state, &active.cancel).await?;
            return notify(cx, &request.session_id, answer);
        }
        // Finish a prior saved reply before accepting a later command in this thread.
        if self.publish
            && let Some(record) = self.store.pending_for(&key)?
        {
            self.deliver(record, &mut state, &active.cancel).await?;
        }
        if self.threads {
            let supplied = standing_context(&blocks);
            let mut cache = active.slot.standing.lock().expect("standing context lock");
            let cached = cache
                .entry(destination.as_ref().ok_or(Error::BuzzContext)?.channel)
                .or_default();
            if !supplied.is_empty() {
                *cached = supplied;
            } else if state.fresh() {
                blocks.splice(0..0, cached.clone());
            }
            blocks.insert(0, THREAD_NOTE.into());
        }
        if self.publish {
            blocks.insert(0, TRANSPORT_NOTE.into());
        }
        let prompt = blocks.join("\n");
        if prompt.len() > crate::transport::MAX_PROMPT_BYTES {
            return Err(Error::Capacity);
        }
        let operation = uuid::Uuid::new_v4().to_string();
        if self.publish {
            self.store.reserve(
                destination.clone().ok_or(Error::BuzzContext)?,
                key,
                operation.clone(),
            )?;
        }
        let result = state
            .run_operation(&self.api, &prompt, &operation, &active.cancel)
            .await;
        if active.cancel.is_cancelled() || matches!(result, Err(Error::Cancelled)) {
            return Err(Error::Cancelled);
        }
        let text = match &result {
            Ok(text) => text.clone(),
            Err(e) => e.to_string(),
        };
        // Persist delivery BEFORE notifying ACP: losing its transport cannot lose the outbox.
        if self.publish {
            let d = destination.as_ref().ok_or(Error::BuzzContext)?;
            let event = self
                .buzz
                .as_ref()
                .ok_or(Error::BuzzContext)?
                .sign_reply(d, &text)?;
            self.store
                .set_delivery(&d.key(), DeliveryStatus::Pending { event })?;
        }
        notify(cx, &request.session_id, text)?;
        if self.publish {
            let d = destination.as_ref().ok_or(Error::BuzzContext)?;
            self.deliver(
                self.store.event(&d.key())?.ok_or(Error::Duplicate)?,
                &mut state,
                &active.cancel,
            )
            .await?;
        }
        result.map(|_| ())
    }
    async fn load(
        &self,
        request: LoadSessionRequest,
        cx: &ConnectionTo<Client>,
        cancellation: agent_client_protocol::RequestCancellation,
    ) -> Result<LoadSessionResponse> {
        if !self.initialized.load(Ordering::Acquire)
            || !self.store.persistent()
            || self.buzz.is_some()
        {
            return Err(Error::UnknownSession);
        }
        let id = request.session_id.0.to_string();
        let cwd = self.store.session(&id)?;
        if cwd != request.cwd {
            return Err(Error::State("Session CWD differs from its saved metadata"));
        }
        self.insert_slot(id.clone(), cwd, request.mcp_servers)?;
        let active = self.begin(&request.session_id)?;
        let work = async {
            tokio::select! { biased; _=active.cancel.cancelled()=>return Err(Error::Cancelled), r=self.bind_account()=>r? };
            let state = self.conversation(format!("acp/{id}"))?;
            let mut state = state.try_lock().map_err(|_| Error::Busy)?;
            state.recover(&self.api, &active.cancel).await?;
            for (role, text) in state.visible_history()? {
                let chunk = ContentChunk::new(text.into());
                let update = if role == "user" {
                    SessionUpdate::UserMessageChunk(chunk)
                } else {
                    SessionUpdate::AgentMessageChunk(chunk)
                };
                cx.send_notification(SessionNotification::new(request.session_id.clone(), update))
                    .map_err(|_| Error::Cancelled)?;
            }
            Ok(LoadSessionResponse::new())
        };
        tokio::pin!(work);
        let result = tokio::select! { biased; _=cancellation.cancelled()=> { active.cancel.cancel(); work.await },r=&mut work=>r };
        if result.is_err() {
            self.sessions.lock().expect("sessions lock").remove(&id);
        }
        result
    }
    async fn prompt(
        self: Arc<Self>,
        request: PromptRequest,
        responder: Responder<PromptResponse>,
        cx: ConnectionTo<Client>,
        active: ActiveTurn,
    ) -> agent_client_protocol::Result<()> {
        let cancellation = responder.cancellation();
        let process = self.process(&request, &active, &cx);
        tokio::pin!(process);
        let result = tokio::select! { biased; _=cancellation.cancelled()=> { active.cancel.cancel(); process.await },r=&mut process=>r };
        match result {
            Ok(()) if !active.cancel.is_cancelled() => {
                responder.respond(PromptResponse::new(StopReason::EndTurn))
            }
            Ok(()) | Err(Error::Cancelled) => {
                responder.respond(PromptResponse::new(StopReason::Cancelled))
            }
            Err(e) => {
                tracing::warn!(error=%e,"ACP turn failed");
                responder.respond_with_error(protocol_error(e))
            }
        }
    }
    pub async fn serve(self: Arc<Self>) -> agent_client_protocol::Result<()> {
        let init = self.clone();
        let new = self.clone();
        let prompt = self.clone();
        let cancel = self.clone();
        let load = self.clone();
        Agent
            .builder()
            .name("openwebui-acp")
            .on_receive_request(
                async move |request: InitializeRequest, responder, _cx| {
                    // Negotiate the supported stable version; incompatible peers may disconnect.
                    let _ = request;
                    init.initialized.store(true, Ordering::Release);
                    responder.respond(
                        InitializeResponse::new(ProtocolVersion::V1)
                            .agent_capabilities(
                                AgentCapabilities::new()
                                    .load_session(init.store.persistent() && init.buzz.is_none()),
                            )
                            .agent_info(Implementation::new(
                                "openwebui-acp",
                                env!("CARGO_PKG_VERSION"),
                            )),
                    )
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |request: NewSessionRequest, responder, _cx| match new
                    .new_session(request)
                {
                    Ok(r) => responder.respond(r),
                    Err(e) => responder.respond_with_error(protocol_error(e)),
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |request: LoadSessionRequest, responder, cx| {
                    let agent = load.clone();
                    let connection = cx.clone();
                    cx.spawn(async move {
                        match agent
                            .load(request, &connection, responder.cancellation())
                            .await
                        {
                            Ok(r) => responder.respond(r),
                            Err(e) => responder.respond_with_error(protocol_error(e)),
                        }
                    })
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |request: PromptRequest, responder, cx| {
                    let active = match prompt.begin(&request.session_id) {
                        Ok(a) => a,
                        Err(e) => return responder.respond_with_error(protocol_error(e)),
                    };
                    let agent = prompt.clone();
                    let connection = cx.clone();
                    cx.spawn(
                        async move { agent.prompt(request, responder, connection, active).await },
                    )
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_notification(
                async move |request: CancelNotification, _cx| {
                    cancel.cancel(&request.session_id);
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .connect_to(agent_client_protocol::Lines::new(
                Box::pin(crate::transport::outgoing(blocking::Unblock::new(
                    std::io::stdout(),
                ))),
                Box::pin(crate::transport::incoming(blocking::Unblock::new(
                    std::io::stdin(),
                ))),
            ))
            .await
    }
    pub async fn shutdown(&self) {
        for slot in self.sessions.lock().expect("sessions lock").values() {
            if let Some(c) = slot.active.lock().expect("active lock").as_ref() {
                c.cancel();
            }
        }
        let states = self
            .conversations
            .lock()
            .expect("conversations lock")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut stops = tokio::task::JoinSet::new();
        for state in states {
            let api = self.api.clone();
            stops.spawn(async move {
                state.lock().await.stop(&api).await;
            });
        }
        let _ = tokio::time::timeout(
            self.api.config.stop_timeout + std::time::Duration::from_secs(1),
            async { while stops.join_next().await.is_some() {} },
        )
        .await;
    }
}
fn notify(cx: &ConnectionTo<Client>, session: &SessionId, text: String) -> Result<()> {
    cx.send_notification(SessionNotification::new(
        session.clone(),
        SessionUpdate::AgentMessageChunk(ContentChunk::new(text.into())),
    ))
    .map_err(|_| Error::Cancelled)
}
pub fn text_blocks(content: &[ContentBlock]) -> Result<Vec<String>> {
    if content.len() > 128 {
        return Err(Error::Capacity);
    }
    let mut bytes = 0usize;
    let mut blocks = Vec::new();
    for block in content {
        let ContentBlock::Text(text) = block else {
            return Err(Error::UnsupportedContent);
        };
        bytes = bytes.checked_add(text.text.len()).ok_or(Error::Capacity)?;
        if bytes > crate::transport::MAX_PROMPT_BYTES {
            return Err(Error::Capacity);
        }
        blocks.push(text.text.clone());
    }
    if blocks.iter().all(|b| b.trim().is_empty()) {
        return Err(Error::UnsupportedContent);
    }
    Ok(blocks)
}
fn protocol_error(error: Error) -> agent_client_protocol::Error {
    let mut response = match error {
        Error::UnknownSession
        | Error::Busy
        | Error::UnsupportedContent
        | Error::BuzzContext
        | Error::BuzzAuthor => agent_client_protocol::Error::invalid_params(),
        _ => agent_client_protocol::Error::internal_error(),
    };
    response.message = error.to_string();
    response
}
