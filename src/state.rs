//! Bounded, private write-ahead state. No transcripts or credentials are archived.
use crate::{
    buzz::Destination,
    error::{Error, Result},
    secure_file::{check_file, private_options, read_private},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Write,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub const MAX_CONVERSATIONS: usize = 256;
pub const MAX_EVENTS: usize = 10_000;
const MAX_STATE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub endpoint: String,
    pub model: String,
    pub relay: Option<String>,
    pub agent: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Idle,
    Saving,
    Saved,
    Submitted,
    Completed,
    Interrupted,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub operation_id: Option<String>,
    pub history_hash: Option<String>,
    pub chat_id: Option<String>,
    pub remote_session_id: String,
    pub last_completed: Option<String>,
    pub user_id: Option<String>,
    pub assistant_id: Option<String>,
    pub parent_id: Option<String>,
    pub phase: Phase,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeliveryStatus {
    Processing,
    Pending { event: nostr::Event },
    Delivered { event_id: String },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventRecord {
    pub destination: Destination,
    pub conversation: String,
    pub operation_id: String,
    pub status: DeliveryStatus,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    version: u32,
    scope: Scope,
    account: Option<String>,
    sessions: BTreeMap<String, PathBuf>,
    conversations: BTreeMap<String, Checkpoint>,
    events: BTreeMap<String, EventRecord>,
}

struct StateLock(File);

impl Drop for StateLock {
    fn drop(&mut self) {
        // Closing only our descriptor can leave flock held by a concurrently
        // spawned child until exec closes its inherited duplicate.
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

pub struct Store {
    directory: Option<PathBuf>,
    _lock: Option<StateLock>,
    data: Mutex<Data>,
    poisoned: AtomicBool,
}

impl Store {
    pub fn open(directory: Option<PathBuf>, scope: Scope) -> Result<Arc<Self>> {
        let mut data = Data {
            version: 1,
            scope: scope.clone(),
            account: None,
            sessions: BTreeMap::new(),
            conversations: BTreeMap::new(),
            events: BTreeMap::new(),
        };
        let mut lock = None;
        if let Some(path) = &directory {
            if !path.is_absolute() {
                return Err(Error::State("State directory must be absolute"));
            }
            if !path.exists() {
                let mut builder = fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(path).map_err(|_| {
                    Error::State("Cannot create state directory; create its parent first")
                })?;
            }
            let meta = fs::symlink_metadata(path)
                .map_err(|_| Error::State("Cannot inspect state directory"))?;
            if !meta.is_dir() || meta.is_symlink() {
                return Err(Error::State("State directory must be a real directory"));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if meta.mode() & 0o077 != 0 || meta.uid() != rustix::process::geteuid().as_raw() {
                    return Err(Error::State(
                        "State directory must be owned by this user with mode 0700",
                    ));
                }
            }
            let file = private_options()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path.join("lock"))
                .map_err(|_| Error::State("Cannot open state lock"))?;
            check_file(&file, 1024)?;
            fs2::FileExt::try_lock_exclusive(&file).map_err(|_| {
                Error::State("State directory is already in use; use one process per directory")
            })?;
            lock = Some(StateLock(file));
            let state_path = path.join("state.json");
            match fs::symlink_metadata(&state_path) {
                Ok(_) => {
                    data=serde_json::from_slice(&read_private(&state_path, MAX_STATE_BYTES)?).map_err(|_| Error::State("State is corrupt or uses an unsupported schema; do not delete uncertain work"))?;
                    if data.version != 1 || data.scope != scope {
                        return Err(Error::StateScope);
                    }
                    if data.sessions.len() > MAX_CONVERSATIONS
                        || data.conversations.len() > MAX_CONVERSATIONS
                        || data.events.len() > MAX_EVENTS
                    {
                        return Err(Error::Capacity);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(Error::State("Cannot inspect state file")),
            }
        }
        let store = Arc::new(Self {
            directory,
            _lock: lock,
            data: Mutex::new(data),
            poisoned: AtomicBool::new(false),
        });
        store.change(|_| Ok(()))?;
        Ok(store)
    }
    pub fn persistent(&self) -> bool {
        self.directory.is_some()
    }
    pub fn bind_account(&self, id: &str) -> Result<()> {
        if id.is_empty() || id.len() > 512 {
            return Err(Error::InvalidResponse("account identity"));
        }
        let current = self
            .data
            .lock()
            .map_err(|_| Error::State("State lock failed"))?
            .account
            .clone();
        if let Some(current) = current {
            return if current == id {
                Ok(())
            } else {
                Err(Error::StateScope)
            };
        }
        self.change(|d| {
            if d.account.as_ref().is_some_and(|current| current != id) {
                return Err(Error::StateScope);
            }
            d.account = Some(id.to_owned());
            Ok(())
        })
    }
    pub fn create_session(&self, id: &str, cwd: PathBuf) -> Result<()> {
        self.change(|d| {
            if d.sessions.len() >= MAX_CONVERSATIONS {
                return Err(Error::Capacity);
            }
            d.sessions.insert(id.to_owned(), cwd);
            Ok(())
        })
    }
    pub fn session(&self, id: &str) -> Result<PathBuf> {
        self.data
            .lock()
            .map_err(|_| Error::State("State lock failed"))?
            .sessions
            .get(id)
            .cloned()
            .ok_or(Error::UnknownSession)
    }
    pub fn checkpoint(&self, key: &str) -> Result<Option<Checkpoint>> {
        Ok(self
            .data
            .lock()
            .map_err(|_| Error::State("State lock failed"))?
            .conversations
            .get(key)
            .cloned())
    }
    pub fn save_checkpoint(&self, key: &str, checkpoint: Checkpoint) -> Result<()> {
        self.change(|d| {
            if !d.conversations.contains_key(key) && d.conversations.len() >= MAX_CONVERSATIONS {
                return Err(Error::Capacity);
            }
            d.conversations.insert(key.to_owned(), checkpoint);
            Ok(())
        })
    }
    pub fn event(&self, key: &str) -> Result<Option<EventRecord>> {
        Ok(self
            .data
            .lock()
            .map_err(|_| Error::State("State lock failed"))?
            .events
            .get(key)
            .cloned())
    }
    pub fn reserve(
        &self,
        destination: Destination,
        conversation: String,
        operation_id: String,
    ) -> Result<()> {
        self.change(|d| {
            if d.events.contains_key(&destination.key()) {
                return Err(Error::Duplicate);
            }
            if d.events.len() >= MAX_EVENTS {
                return Err(Error::Capacity);
            }
            d.events.insert(
                destination.key(),
                EventRecord {
                    destination,
                    conversation,
                    operation_id,
                    status: DeliveryStatus::Processing,
                },
            );
            Ok(())
        })
    }
    pub fn pending_for(&self, conversation: &str) -> Result<Option<EventRecord>> {
        Ok(self
            .data
            .lock()
            .map_err(|_| Error::State("State lock failed"))?
            .events
            .values()
            .find(|e| {
                e.conversation == conversation
                    && !matches!(e.status, DeliveryStatus::Delivered { .. })
            })
            .cloned())
    }
    pub fn set_delivery(&self, key: &str, status: DeliveryStatus) -> Result<()> {
        self.change(|d| {
            d.events.get_mut(key).ok_or(Error::Duplicate)?.status = status;
            Ok(())
        })
    }
    fn change(&self, f: impl FnOnce(&mut Data) -> Result<()>) -> Result<()> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::State(
                "A state write failed; restart and reconcile before proceeding",
            ));
        }
        let mut current = self
            .data
            .lock()
            .map_err(|_| Error::State("State lock failed"))?;
        let mut next = current.clone();
        f(&mut next)?;
        let bytes = serde_json::to_vec(&next).map_err(|_| Error::State("Cannot encode state"))?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err(Error::Capacity);
        }
        if let Some(dir) = &self.directory {
            let temp = dir.join(format!("state.next.{}", uuid::Uuid::new_v4()));
            let write = (|| -> std::io::Result<()> {
                let mut file = private_options().write(true).create_new(true).open(&temp)?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                fs::rename(&temp, dir.join("state.json"))?;
                File::open(dir)?.sync_all()?;
                Ok(())
            })();
            if write.is_err() {
                self.poisoned.store(true, Ordering::Release);
                let _ = fs::remove_file(&temp);
                return Err(Error::State(
                    "Durable state write failed; remote work was not retried",
                ));
            }
        }
        *current = next;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn last_store_owner_releases_lock_with_an_inherited_descriptor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state");
        let scope = Scope {
            endpoint: "https://webui.example/".into(),
            model: "assistant".into(),
            relay: None,
            agent: None,
        };
        let store = Store::open(Some(path.clone()), scope.clone()).unwrap();
        // A concurrent subprocess launch can inherit this open file description
        // until exec closes it, even when the descriptor is close-on-exec.
        let inherited = store._lock.as_ref().unwrap().0.try_clone().unwrap();
        let other_owner = store.clone();
        drop(store);
        assert!(Store::open(Some(path.clone()), scope.clone()).is_err());
        drop(other_owner);

        let reopened = Store::open(Some(path.clone()), scope.clone()).unwrap();
        assert!(inherited.metadata().is_ok());
        assert!(Store::open(Some(path), scope).is_err());
        drop(reopened);
    }
}
