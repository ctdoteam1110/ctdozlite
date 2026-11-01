//! Account-owned durable drafts and outbox, without transport or automatic retry.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
fn schema_version() -> u32 {
    1
}
fn read_data(path: &Path, account: &str) -> Result<ComposerData> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_FILE_BYTES,
        "Composer file limit exceeded"
    );
    let data: ComposerData = serde_json::from_slice(&bytes)?;
    ensure!(data.version == 1, "Unsupported composer schema");
    ensure!(data.account == account, "Composer owner mismatch");
    ensure!(
        data.drafts.len() <= 512 && data.outbox.len() <= 128,
        "Composer entry limit exceeded"
    );
    for draft in data
        .drafts
        .values()
        .chain(data.outbox.values().map(|e| &e.draft))
    {
        validate_draft(draft)?;
    }
    Ok(data)
}
fn validate_draft(draft: &Draft) -> Result<()> {
    ensure!(
        draft.text.len() <= 128 * 1024 && draft.attachments.len() <= 32,
        "Composer draft limit exceeded"
    );
    Ok(())
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
/// Exact draft payload owned by one conversation.
pub struct Draft {
    pub text: String,
    pub reply: Option<crate::chat::ChatMessage>,
    #[serde(default)]
    pub attachments: Vec<PathBuf>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
/// A staged send with persisted client identities and independent attachment acknowledgements.
pub struct OutboxEntry {
    #[serde(default)]
    pub attachment_client_ids: Vec<String>,
    pub id: String,
    pub thread_id: String,
    pub draft: Draft,
    pub state: crate::message_model::DeliveryState,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct ComposerData {
    #[serde(default = "schema_version")]
    version: u32,
    account: String,
    drafts: BTreeMap<String, Draft>,
    outbox: BTreeMap<String, OutboxEntry>,
    next_id: u64,
}
/// Blocking account-owned composer store; callers serialize access in the worker.
pub struct ComposerStore {
    path: PathBuf,
    data: ComposerData,
}
fn terminal_delivery(state: crate::message_model::DeliveryState) -> bool {
    matches!(
        state,
        crate::message_model::DeliveryState::ServerAccepted
            | crate::message_model::DeliveryState::Delivered
            | crate::message_model::DeliveryState::Read
    )
}
fn composer_mutations() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}
impl ComposerStore {
    /// Open a validated account file without sending or recovering anything.
    ///
    /// # Errors
    /// Returns an error for invalid owner/schema, malformed data, or filesystem failure.
    pub fn open(root: &Path, account: &str) -> Result<Self> {
        ensure!(!account.is_empty(), "Account identity required");
        let dir = root
            .join("accounts")
            .join(format!("{:x}", Sha256::digest(account.as_bytes())));
        let path = dir.join("composer.json");
        // Validate existing owner/schema before mutating directory permissions.
        let data = if path.exists() {
            read_data(&path, account)?
        } else {
            ComposerData {
                version: 1,
                account: account.into(),
                ..Default::default()
            }
        };
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self { path, data })
    }
    fn snapshot(&self) -> Result<ComposerData> {
        if !self.path.exists() {
            return Ok(self.data.clone());
        }
        let data = read_data(&self.path, &self.data.account)?;
        ensure!(data.account == self.data.account, "Composer owner mismatch");
        Ok(data)
    }
    fn serial_transaction(&self) -> Result<rusqlite::Connection> {
        // The process mutex handles threads; a SQLite RESERVED lock also serializes
        // independent app processes before either reads and replaces composer.json.
        if self.path.exists() {
            read_data(&self.path, &self.data.account)?;
        }
        let lock = self.path.with_extension("lock.sqlite3");
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(&lock) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                ensure!(
                    std::fs::symlink_metadata(&lock)?.file_type().is_file(),
                    "Invalid composer lock file"
                );
            }
            Err(error) => return Err(error.into()),
        }
        let conn = rusqlite::Connection::open(&lock)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("BEGIN IMMEDIATE")?;
        Ok(conn)
    }
    fn reload(&mut self) -> Result<()> {
        let mut data = self.snapshot()?;
        data.outbox.retain(|_, entry| {
            !(terminal_delivery(entry.state) && entry.draft.attachments.is_empty())
        });
        self.data = data;
        Ok(())
    }
    /// Read the latest persisted draft.
    ///
    /// # Errors
    /// Returns an error when the current file fails validation.
    pub fn draft(&self, thread: &str) -> Result<Draft> {
        Ok(self
            .snapshot()?
            .drafts
            .get(thread)
            .cloned()
            .unwrap_or_default())
    }
    /// Atomically replace one conversation draft.
    ///
    /// # Errors
    /// Returns an error for invalid identity, limits, or failed persistence.
    pub fn set_draft(&mut self, thread: &str, draft: Draft) -> Result<()> {
        ensure!(!thread.is_empty(), "Conversation identity required");
        validate_draft(&draft)?;
        let _guard = composer_mutations()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _transaction = self.serial_transaction()?;
        self.reload()?;
        ensure!(
            self.data.drafts.contains_key(thread) || self.data.drafts.len() < 512,
            "Draft limit reached"
        );
        let before = self.data.drafts.insert(thread.into(), draft);
        if let Err(error) = self.save() {
            if let Some(previous) = before {
                self.data.drafts.insert(thread.into(), previous);
            } else {
                self.data.drafts.remove(thread);
            }
            return Err(error);
        }
        Ok(())
    }
    /// Persist a send before assigning it to network work; never performs I/O on the network.
    ///
    /// # Errors
    /// Returns an error for limits, exhausted IDs, or failed persistence.
    pub fn stage(&mut self, thread: &str, draft: Draft) -> Result<String> {
        validate_draft(&draft)?;
        let _guard = composer_mutations()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _transaction = self.serial_transaction()?;
        self.reload()?;
        ensure!(!thread.is_empty(), "Conversation identity required");
        ensure!(self.data.outbox.len() < 128, "Outbox pending limit reached");
        ensure!(
            draft.attachments.len() <= 32,
            "Attachment batch limit reached"
        );
        let now = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis(),
        )?;
        let persisted_max = self
            .data
            .outbox
            .keys()
            .filter_map(|id| id.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        let previous = self.data.next_id.max(persisted_max);
        let next = previous
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Client ID space exhausted"))?
            .max(now);
        let before = self.data.clone();
        let attachment_client_ids = (0..draft.attachments.len())
            .map(|index| {
                next.checked_add(index as u64 + 1)
                    .map(|id| id.to_string())
                    .ok_or_else(|| anyhow::anyhow!("Client ID space exhausted"))
            })
            .collect::<Result<Vec<_>>>()?;
        self.data.next_id = next
            .checked_add(draft.attachments.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("Client ID space exhausted"))?;
        let id = next.to_string();
        self.data.outbox.insert(
            id.clone(),
            OutboxEntry {
                attachment_client_ids,
                id: id.clone(),
                thread_id: thread.into(),
                draft,
                state: crate::message_model::DeliveryState::Pending,
            },
        );
        self.data.drafts.remove(thread);
        if let Err(error) = self.save() {
            self.data = before;
            return Err(error);
        }
        Ok(id)
    }
    /// Persist monotonic text delivery; retain unacknowledged attachments.
    ///
    /// # Errors
    /// Returns an error when persistence fails.
    pub fn complete(&mut self, id: &str, state: crate::message_model::DeliveryState) -> Result<()> {
        let _guard = composer_mutations()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _transaction = self.serial_transaction()?;
        self.reload()?;
        let before = self.data.clone();
        let terminal = if let Some(entry) = self.data.outbox.get_mut(id) {
            entry.state = entry.state.merge(state);
            terminal_delivery(entry.state) && entry.draft.attachments.is_empty()
        } else {
            false
        };
        if terminal {
            self.data.outbox.remove(id);
        }
        if let Err(error) = self.save() {
            self.data = before;
            return Err(error);
        }
        Ok(())
    }
    /// Mark interrupted sends uncertain without automatically resending.
    ///
    /// # Errors
    /// Returns an error when persistence fails.
    pub fn recover_interrupted(&mut self) -> Result<usize> {
        let _guard = composer_mutations()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _transaction = self.serial_transaction()?;
        self.reload()?;
        let before = self.data.clone();
        let mut count = 0;
        for entry in self.data.outbox.values_mut() {
            if entry.state == crate::message_model::DeliveryState::Pending {
                entry.state = crate::message_model::DeliveryState::DeliveryUnknown;
                count += 1;
            }
        }
        if count > 0 {
            if let Err(error) = self.save() {
                self.data = before;
                return Err(error);
            }
        }
        Ok(count)
    }
    /// Acknowledge one unambiguous attachment path.
    ///
    /// # Errors
    /// Returns an error for ambiguous paths, missing entries, or failed persistence.
    pub fn complete_attachment(&mut self, id: &str, path: &Path) -> Result<()> {
        let _guard = composer_mutations()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _transaction = self.serial_transaction()?;
        self.reload()?;
        let before = self.data.clone();
        let entry = self
            .data
            .outbox
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("Outbox entry not found"))?;
        ensure!(
            entry
                .draft
                .attachments
                .iter()
                .filter(|item| *item == path)
                .count()
                <= 1,
            "Ambiguous attachment path; use client identity"
        );
        if let Some(index) = entry.draft.attachments.iter().position(|item| item == path) {
            entry.draft.attachments.remove(index);
            if index < entry.attachment_client_ids.len() {
                entry.attachment_client_ids.remove(index);
            }
        }
        if terminal_delivery(entry.state) && entry.draft.attachments.is_empty() {
            self.data.outbox.remove(id);
        }
        if let Err(error) = self.save() {
            self.data = before;
            return Err(error);
        }
        Ok(())
    }
    /// Acknowledge one attachment by its persisted request client ID, including duplicate paths.
    ///
    /// # Errors
    /// Returns an error for a missing entry/identity or failed persistence.
    pub fn complete_attachment_id(&mut self, id: &str, client_id: &str) -> Result<()> {
        let _guard = composer_mutations()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _transaction = self.serial_transaction()?;
        self.reload()?;
        let before = self.data.clone();
        let entry = self
            .data
            .outbox
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("Outbox entry not found"))?;
        let index = entry
            .attachment_client_ids
            .iter()
            .position(|item| item == client_id)
            .ok_or_else(|| anyhow::anyhow!("Attachment identity not found"))?;
        ensure!(
            index < entry.draft.attachments.len(),
            "Invalid attachment identity mapping"
        );
        entry.attachment_client_ids.remove(index);
        entry.draft.attachments.remove(index);
        if terminal_delivery(entry.state) && entry.draft.attachments.is_empty() {
            self.data.outbox.remove(id);
        }
        if let Err(error) = self.save() {
            self.data = before;
            return Err(error);
        }
        Ok(())
    }
    /// Read unresolved failures for one exact conversation.
    ///
    /// # Errors
    /// Returns an error if the current file fails validation.
    pub fn failed_for(&self, thread: &str) -> Result<Vec<OutboxEntry>> {
        Ok(self
            .snapshot()?
            .outbox
            .values()
            .filter(|e| {
                e.thread_id == thread
                    && matches!(
                        e.state,
                        crate::message_model::DeliveryState::Failed
                            | crate::message_model::DeliveryState::DeliveryUnknown
                    )
            })
            .cloned()
            .collect())
    }
    /// Read pending or uncertain work for explicit user recovery, not automatic resend.
    ///
    /// # Errors
    /// Returns an error if the current file fails validation.
    pub fn pending(&self) -> Result<Vec<OutboxEntry>> {
        Ok(self
            .snapshot()?
            .outbox
            .values()
            .filter(|e| {
                !e.draft.attachments.is_empty()
                    || matches!(
                        e.state,
                        crate::message_model::DeliveryState::Pending
                            | crate::message_model::DeliveryState::DeliveryUnknown
                            | crate::message_model::DeliveryState::Failed
                    )
            })
            .cloned()
            .collect())
    }
    /// Read one staged send by its exact persisted ID.
    ///
    /// # Errors
    /// Returns an error if the current file fails validation.
    pub fn entry(&self, id: &str) -> Result<Option<OutboxEntry>> {
        Ok(self.snapshot()?.outbox.get(id).cloned())
    }
    fn save(&self) -> Result<()> {
        let bytes = serde_json::to_vec(&self.data)?;
        ensure!(
            bytes.len() as u64 <= MAX_FILE_BYTES,
            "Composer file limit exceeded"
        );
        crate::data_dir::write_private_atomic(&self.path, &bytes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composer_failure_keeps_exact_owner_and_thread() {
        let p = std::env::temp_dir().join(format!("composer-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        let mut s = ComposerStore::open(&p, "A").unwrap();
        let id = s
            .stage(
                "chat-A",
                Draft {
                    text: "original".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        s.set_draft(
            "chat-B",
            Draft {
                text: "new".into(),
                ..Default::default()
            },
        )
        .unwrap();
        s.complete(&id, crate::message_model::DeliveryState::Failed)
            .unwrap();
        assert_eq!(s.draft("chat-B").unwrap().text, "new");
        assert_eq!(s.failed_for("chat-A").unwrap()[0].draft.text, "original");
        drop(s);
        let s = ComposerStore::open(&p, "A").unwrap();
        assert_eq!(s.failed_for("chat-A").unwrap().len(), 1);
        assert!(ComposerStore::open(&p, "B")
            .unwrap()
            .pending()
            .unwrap()
            .is_empty());
        let _ = std::fs::remove_dir_all(p);
    }
}

#[cfg(test)]
mod durable_id_tests {
    use super::*;
    #[test]
    fn staged_client_ids_are_decimal_monotonic_and_persisted() {
        let p = std::env::temp_dir().join(format!("composer-id-test-{}", std::process::id()));
        let mut store = ComposerStore::open(&p, "fixture-account").unwrap();
        let first = store
            .stage(
                "A",
                Draft {
                    text: "first".into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .parse::<u64>()
            .unwrap();
        store.data.next_id = first + 10000;
        store.save().unwrap();
        let second = store
            .stage(
                "B",
                Draft {
                    text: "second".into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert!(second > first + 10000);
        drop(store);
        let mut store = ComposerStore::open(&p, "fixture-account").unwrap();
        let third = store
            .stage(
                "A",
                Draft {
                    text: "third".into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert!(third > second);
        assert_eq!(store.pending().unwrap().len(), 3);
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn stage_failure_preserves_draft_and_does_not_enqueue() {
        let p = std::env::temp_dir().join(format!("composer-rollback-test-{}", std::process::id()));
        let mut store = ComposerStore::open(&p, "fixture-account").unwrap();
        store
            .set_draft(
                "A",
                Draft {
                    text: "keep".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        store.path = p.join("missing-parent/composer.json");
        assert!(store
            .stage(
                "A",
                Draft {
                    text: "keep".into(),
                    ..Default::default()
                }
            )
            .is_err());
        assert_eq!(store.draft("A").unwrap().text, "keep");
        assert!(store.pending().unwrap().is_empty());
        std::fs::remove_dir_all(p).unwrap();
    }
}

#[cfg(test)]
mod concurrent_store_tests {
    use super::*;
    #[test]
    fn alternating_store_instances_preserve_drafts_and_attachment_ack() {
        let root =
            std::env::temp_dir().join(format!("composer-multiple-test-{}", std::process::id()));
        let mut ui = ComposerStore::open(&root, "fixture").unwrap();
        let mut worker = ComposerStore::open(&root, "fixture").unwrap();
        ui.set_draft(
            "A",
            Draft {
                text: "draft A".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let path = PathBuf::from("fixture-photo.png");
        let id = ui
            .stage(
                "A",
                Draft {
                    text: "caption".into(),
                    attachments: vec![path.clone()],
                    ..Default::default()
                },
            )
            .unwrap();
        worker
            .set_draft(
                "B",
                Draft {
                    text: "draft B".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        ui.set_draft(
            "C",
            Draft {
                text: "draft C".into(),
                ..Default::default()
            },
        )
        .unwrap();
        worker.complete_attachment(&id, &path).unwrap();
        assert!(ui.entry(&id).unwrap().unwrap().draft.attachments.is_empty());
        ui.set_draft(
            "B",
            Draft {
                text: "draft B edited".into(),
                ..Default::default()
            },
        )
        .unwrap();
        worker
            .complete(&id, crate::message_model::DeliveryState::ServerAccepted)
            .unwrap();
        assert_eq!(ui.draft("B").unwrap().text, "draft B edited");
        assert_eq!(worker.draft("C").unwrap().text, "draft C");
        assert!(ui.entry(&id).unwrap().is_none());
        let second = worker
            .stage(
                "B",
                Draft {
                    text: "next".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(second.parse::<u64>().unwrap() > id.parse::<u64>().unwrap());
        assert_eq!(ui.pending().unwrap().len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod terminal_outbox_tests {
    use super::*;
    #[test]
    fn terminal_completions_keep_the_composer_file_bounded() {
        let root =
            std::env::temp_dir().join(format!("composer-bounded-test-{}", std::process::id()));
        let mut store = ComposerStore::open(&root, "fixture").unwrap();
        let mut last = 0u64;
        for index in 0..128 {
            let id = store
                .stage(
                    "thread",
                    Draft {
                        text: format!("message {index}"),
                        ..Default::default()
                    },
                )
                .unwrap();
            assert!(id.parse::<u64>().unwrap() > last);
            last = id.parse().unwrap();
            store
                .complete(&id, crate::message_model::DeliveryState::ServerAccepted)
                .unwrap();
            assert!(store.pending().unwrap().is_empty());
            assert!(store.entry(&id).unwrap().is_none());
        }
        assert!(std::fs::metadata(&store.path).unwrap().len() < 1024);
        assert_eq!(store.data.next_id, last);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod auth_recovery_tests {
    use super::*;
    #[test]
    fn interrupted_send_recovers_as_unknown_without_resending() {
        let p = std::env::temp_dir().join(format!("composer-recovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        let mut store = ComposerStore::open(&p, "A").unwrap();
        let id = store
            .stage(
                "chat",
                Draft {
                    text: "keep".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        drop(store);
        let mut reopened = ComposerStore::open(&p, "A").unwrap();
        assert_eq!(reopened.recover_interrupted().unwrap(), 1);
        assert_eq!(
            reopened.entry(&id).unwrap().unwrap().state,
            crate::message_model::DeliveryState::DeliveryUnknown
        );
        assert_eq!(reopened.entry(&id).unwrap().unwrap().draft.text, "keep");
        assert_eq!(reopened.recover_interrupted().unwrap(), 0);
        let _ = std::fs::remove_dir_all(p);
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    fn root(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("zl-composer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }
    #[test]
    fn cross_process_writer_fixture() {
        let Ok(root) = std::env::var("ZL_COMPOSER_TEST_ROOT") else {
            return;
        };
        let mut store = ComposerStore::open(Path::new(&root), "process-fixture").unwrap();
        for n in 0..8 {
            store
                .stage(
                    &format!("thread-{}-{n}", std::process::id()),
                    Draft {
                        text: "fixture".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
    }
    #[test]
    fn independent_processes_allocate_unique_ids_without_lost_entries() {
        let p = root("processes");
        let store = ComposerStore::open(&p, "process-fixture").unwrap();
        let exe = std::env::current_exe().unwrap();
        let mut children = Vec::new();
        for _ in 0..4 {
            children.push(
                std::process::Command::new(&exe)
                    .args([
                        "--exact",
                        "composer_store::validation_tests::cross_process_writer_fixture",
                    ])
                    .env("ZL_COMPOSER_TEST_ROOT", &p)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
        }
        for child in children {
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let rows = store.pending().unwrap();
        assert_eq!(rows.len(), 32);
        let ids: std::collections::BTreeSet<_> = rows.iter().map(|e| &e.id).collect();
        assert_eq!(ids.len(), 32);
        assert!(rows.iter().all(|e| e.id.parse::<u64>().is_ok()));
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn newer_schema_and_wrong_owner_do_not_mutate_existing_files() {
        for (label, field, value) in [
            ("newer", "version", serde_json::json!(99)),
            ("owner", "account", serde_json::json!("B")),
        ] {
            let p = root(label);
            let mut store = ComposerStore::open(&p, "A").unwrap();
            store
                .set_draft(
                    "t",
                    Draft {
                        text: "keep".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
            let mut data: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&store.path).unwrap()).unwrap();
            data[field] = value;
            let bytes = serde_json::to_vec(&data).unwrap();
            std::fs::write(&store.path, &bytes).unwrap();
            assert!(ComposerStore::open(&p, "A").is_err());
            assert_eq!(std::fs::read(&store.path).unwrap(), bytes);
            assert!(store.draft("t").is_err());
            assert!(store.set_draft("t", Draft::default()).is_err());
            assert_eq!(std::fs::read(&store.path).unwrap(), bytes);
            std::fs::remove_dir_all(p).unwrap();
        }
    }
    #[test]
    fn corrupt_current_file_never_returns_stale_in_memory_draft() {
        let p = root("corrupt");
        let mut store = ComposerStore::open(&p, "A").unwrap();
        store
            .set_draft(
                "t",
                Draft {
                    text: "old".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        std::fs::write(&store.path, b"{").unwrap();
        assert!(store.draft("t").is_err());
        assert!(store.pending().is_err());
        assert!(store.failed_for("t").is_err());
        assert!(store.entry("id").is_err());
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn text_ack_does_not_discard_unacknowledged_attachments_after_reopen() {
        let p = root("partial-ack");
        let mut store = ComposerStore::open(&p, "A").unwrap();
        let photo = PathBuf::from("photo.png");
        let id = store
            .stage(
                "t",
                Draft {
                    text: "caption".into(),
                    attachments: vec![photo.clone()],
                    ..Default::default()
                },
            )
            .unwrap();
        let attachment_id = store.entry(&id).unwrap().unwrap().attachment_client_ids[0].clone();
        store
            .complete(&id, crate::message_model::DeliveryState::ServerAccepted)
            .unwrap();
        drop(store);
        let mut reopened = ComposerStore::open(&p, "A").unwrap();
        assert_eq!(
            reopened.entry(&id).unwrap().unwrap().attachment_client_ids,
            vec![attachment_id]
        );
        reopened.set_draft("another", Draft::default()).unwrap();
        assert!(reopened.entry(&id).unwrap().is_some());
        reopened.complete_attachment(&id, &photo).unwrap();
        assert!(reopened.entry(&id).unwrap().is_none());
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn duplicate_paths_ack_only_by_exact_attachment_client_identity() {
        let p = root("duplicate-path");
        let mut store = ComposerStore::open(&p, "A").unwrap();
        let photo = PathBuf::from("photo.png");
        let id = store
            .stage(
                "t",
                Draft {
                    attachments: vec![photo.clone(), photo.clone()],
                    ..Default::default()
                },
            )
            .unwrap();
        let clients = store.entry(&id).unwrap().unwrap().attachment_client_ids;
        assert_ne!(clients[0], clients[1]);
        assert!(store.complete_attachment(&id, &photo).is_err());
        store.complete_attachment_id(&id, &clients[0]).unwrap();
        let entry = store.entry(&id).unwrap().unwrap();
        assert_eq!(entry.draft.attachments, vec![photo]);
        assert_eq!(entry.attachment_client_ids, vec![clients[1].clone()]);
        assert!(store.complete_attachment_id(&id, &clients[0]).is_err());
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn oversized_draft_and_empty_thread_preserve_original_file() {
        let p = root("limit");
        let mut store = ComposerStore::open(&p, "A").unwrap();
        store
            .set_draft(
                "t",
                Draft {
                    text: "keep".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let bytes = std::fs::read(&store.path).unwrap();
        assert!(store.set_draft("", Draft::default()).is_err());
        assert!(store
            .stage(
                "t",
                Draft {
                    text: "x".repeat(128 * 1024 + 1),
                    ..Default::default()
                }
            )
            .is_err());
        assert_eq!(std::fs::read(&store.path).unwrap(), bytes);
        std::fs::remove_dir_all(p).unwrap();
    }
}
