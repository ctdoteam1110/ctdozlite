//! Durable, account-isolated reaction inbox and history coverage checkpoints.
//! Resolved rows remain as a replay journal: a crash before message-cache persistence
//! must not lose the event. Message ledgers make replay into loaded pages idempotent.
use crate::{chat::Thread, reactions::ReactionEvent};
use anyhow::{ensure, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Maximum events decoded at once during replay.
pub const JOURNAL_PAGE_SIZE: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
/// One durable event awaiting a unique loaded target.
pub struct PendingReaction {
    /// Stable deduplication key.
    pub key: String,
    /// Lossless normalized wire event.
    pub event: ReactionEvent,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
/// Explicit coverage reported by a history provider, never inferred from an empty page.
pub struct CoverageCheckpoint {
    /// History provider or thread scope.
    pub scope: String,
    /// Number of pages explicitly scanned.
    pub pages: u64,
    /// Whether the provider verified full coverage.
    pub complete: bool,
    /// Reason coverage remains partial.
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// Durable inbox counters and coverage, including unresolved events.
pub struct JournalStatus {
    /// Unresolved event count.
    pub pending: u64,
    /// Total retained replay events.
    pub total: u64,
    /// Whether inbox or coverage remains incomplete.
    pub partial: bool,
    /// Explicit provider coverage checkpoints.
    pub checkpoints: Vec<CoverageCheckpoint>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// Replay counters for one loaded snapshot.
pub struct ApplySummary {
    /// Number of retained rows examined.
    pub scanned: u64,
    /// Events that changed a loaded ledger.
    pub applied: u64,
    /// Newly matched durable rows.
    pub resolved: u64,
    /// Rows with multiple candidate targets.
    pub ambiguous: u64,
    /// Unresolved event count.
    pub pending: u64,
}

/// Account-owned SQLite journal; callers serialize append and replay on a blocking worker.
pub struct ReactionJournal {
    conn: Connection,
    path: PathBuf,
    owner: String,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn known(id: &str) -> bool {
    !id.is_empty() && id != "0"
}
fn private(path: &Path, directory: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(if directory { 0o700 } else { 0o600 }),
        )?;
    }
    Ok(())
}
/// Wire event IDs carry their version. Scope includes target and reactor because
/// one wire envelope may contain several targets. No-ID events use the exact payload.
pub fn event_key(event: &ReactionEvent) -> Result<String> {
    let target = if known(&event.message_id) {
        ("global", &event.message_id)
    } else {
        ("client", &event.client_message_id)
    };
    let identity = if known(&event.event_id) {
        serde_json::to_vec(&(
            "server",
            &event.thread_id,
            target,
            &event.sender_id,
            &event.event_id,
        ))?
    } else if known(&event.client_event_id) {
        serde_json::to_vec(&(
            "client",
            &event.thread_id,
            target,
            &event.sender_id,
            &event.client_event_id,
        ))?
    } else {
        serde_json::to_vec(event)?
    };
    Ok(hash(&identity))
}
impl ReactionJournal {
    /// Open or initialize an account-owned journal.
    ///
    /// # Errors
    /// Returns filesystem, SQLite, owner mismatch, or unsupported schema errors.
    pub fn open(root: &Path, account: &str) -> Result<Self> {
        ensure!(
            known(account) && !account.trim().is_empty(),
            "Account identity required"
        );
        let owner = hash(account.as_bytes());
        let directory = root.join("accounts").join(&owner);
        std::fs::create_dir_all(&directory)?;
        private(&root.join("accounts"), true)?;
        private(&directory, true)?;
        let path = directory.join("reactions-v2.sqlite");
        if !path.exists() {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        let mut conn = Connection::open(&path)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(
            version <= 1,
            "Reaction journal schema is newer than this application"
        );
        let has_owner: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='owner')",
            [],
            |r| r.get(0),
        )?;
        if has_owner {
            let owners: Vec<String> = conn
                .prepare("SELECT owner_hash FROM owner")?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            ensure!(
                owners == [owner.clone()],
                "Reaction journal account mismatch"
            );
        } else {
            ensure!(version == 0, "Reaction journal owner missing");
        }
        // Ownership and newer-version guards precede every schema/pragma mutation.
        private(&path, false)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
            PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;",
        )?;
        if version == 0 {
            let tx = conn.transaction()?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS owner(owner_hash TEXT PRIMARY KEY NOT NULL);
                CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT,event_key TEXT UNIQUE NOT NULL,payload TEXT NOT NULL,resolved INTEGER NOT NULL DEFAULT 0);
                CREATE INDEX IF NOT EXISTS pending_events ON events(resolved,seq);
                CREATE TABLE IF NOT EXISTS checkpoints(scope TEXT PRIMARY KEY NOT NULL,pages INTEGER NOT NULL,complete INTEGER NOT NULL,reason TEXT);")?;
            tx.execute("INSERT OR IGNORE INTO owner VALUES(?1)", [&owner])?;
            tx.pragma_update(None, "user_version", 1)?;
            tx.commit()?;
        }
        Ok(Self { conn, path, owner })
    }
    fn validate_owner(&self) -> Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(version == 1, "Unsupported reaction journal schema");
        let owners: Vec<String> = self
            .conn
            .prepare("SELECT owner_hash FROM owner")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        ensure!(
            owners == [self.owner.clone()],
            "Reaction journal account mismatch"
        );
        Ok(())
    }
    /// Return the account-isolated SQLite location.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Atomic batch append. Existing rows are never dropped to enforce a memory limit.
    ///
    /// # Errors
    /// Returns invalid input, account mismatch, serialization, or SQLite errors.
    pub fn append(&mut self, events: &[ReactionEvent]) -> Result<usize> {
        self.validate_owner()?;
        let tx = self.conn.transaction()?;
        let mut inserted = 0;
        for event in events {
            ensure!(
                known(&event.thread_id)
                    && !event.sender_id.is_empty()
                    && (known(&event.message_id) || known(&event.client_message_id))
                    && event.r_type >= -1,
                "Invalid reaction journal event"
            );
            inserted += tx.execute(
                "INSERT OR IGNORE INTO events(event_key,payload) VALUES(?1,?2)",
                params![event_key(event)?, serde_json::to_string(event)?],
            )?;
        }
        tx.commit()?;
        Ok(inserted)
    }
    /// Bounded in-memory page. The on-disk queue has no 4096-event eviction policy.
    ///
    /// # Errors
    /// Returns invalid input, account mismatch, serialization, or SQLite errors.
    pub fn pending(&self, limit: usize) -> Result<Vec<PendingReaction>> {
        let mut statement = self.conn.prepare(
            "SELECT event_key,payload FROM events WHERE resolved=0 ORDER BY seq LIMIT ?1",
        )?;
        let rows = statement.query_map([limit.min(4096) as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (key, payload) = row?;
            result.push(PendingReaction {
                key,
                event: serde_json::from_str(&payload)?,
            });
        }
        Ok(result)
    }
    /// Remove from the pending inbox, retaining the replay payload for crash recovery.
    ///
    /// # Errors
    /// Returns invalid input, account mismatch, serialization, or SQLite errors.
    pub fn resolve(&mut self, key: &str) -> Result<bool> {
        self.validate_owner()?;
        Ok(self.conn.execute(
            "UPDATE events SET resolved=1 WHERE event_key=?1 AND resolved=0",
            [key],
        )? > 0)
    }
    /// Scan bounded SQLite pages, including previously resolved rows for stale-cache recovery.
    /// A missing/ambiguous target stays pending. Existing message ledgers own chronology.
    ///
    /// # Errors
    /// Returns invalid input, account mismatch, serialization, or SQLite errors.
    pub fn apply_loaded(&mut self, threads: &mut [Thread]) -> Result<ApplySummary> {
        self.validate_owner()?;
        let mut summary = ApplySummary::default();
        let mut after = 0_i64;
        loop {
            let rows = {
                let mut statement = self.conn.prepare("SELECT seq,event_key,payload,resolved FROM events WHERE seq>?1 ORDER BY seq LIMIT ?2")?;
                let rows = statement.query_map(params![after, JOURNAL_PAGE_SIZE as i64], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, bool>(3)?,
                    ))
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            if rows.is_empty() {
                break;
            }
            for (seq, key, payload, was_resolved) in rows {
                after = seq;
                summary.scanned += 1;
                let event: ReactionEvent = serde_json::from_str(&payload)?;
                let scopes: Vec<_> = threads
                    .iter()
                    .enumerate()
                    .filter(|(_, thread)| {
                        thread.id == event.thread_id
                            || (thread.is_group
                                && thread.id.strip_prefix('g').unwrap_or(&thread.id)
                                    == event
                                        .thread_id
                                        .strip_prefix('g')
                                        .unwrap_or(&event.thread_id))
                    })
                    .map(|(index, _)| index)
                    .collect();
                let [thread_index] = scopes.as_slice() else {
                    if scopes.len() > 1 {
                        summary.ambiguous += 1;
                    }
                    continue;
                };
                let thread = &mut threads[*thread_index];
                let exact: Vec<_> = thread
                    .messages
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| known(&event.message_id) && m.id == event.message_id)
                    .map(|(i, _)| i)
                    .collect();
                let matches = if exact.is_empty() {
                    thread
                        .messages
                        .iter()
                        .enumerate()
                        .filter(|(_, m)| {
                            known(&event.client_message_id)
                                && (m.client_message_id == event.client_message_id
                                    || m.history_source
                                        .as_ref()
                                        .and_then(|s| s.get("clientId"))
                                        .is_some_and(|id| {
                                            id.as_str()
                                                .map(str::to_owned)
                                                .or_else(|| id.as_u64().map(|id| id.to_string()))
                                                .as_deref()
                                                == Some(event.client_message_id.as_str())
                                        }))
                        })
                        .map(|(i, _)| i)
                        .collect()
                } else {
                    exact
                };
                let [message_index] = matches.as_slice() else {
                    if matches.len() > 1 {
                        summary.ambiguous += 1;
                    }
                    continue;
                };
                let message = &mut thread.messages[*message_index];
                let watermark = &message.reaction_snapshot_provenance;
                let covered_by_snapshot = message.reaction_authoritative_snapshot
                    && !watermark.source.is_empty()
                    && watermark.captured_at_ms > 0
                    && event.ts > 0
                    && crate::clock::normalize_ts_ms(event.ts) <= watermark.captured_at_ms;
                if !covered_by_snapshot && message.apply_reaction(&event) {
                    summary.applied += 1;
                }
                if !was_resolved && self.resolve(&key)? {
                    summary.resolved += 1;
                }
            }
        }
        summary.pending = self.pending_count()?;
        Ok(summary)
    }
    /// Count unresolved events.
    ///
    /// # Errors
    /// Returns SQLite read errors.
    pub fn pending_count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM events WHERE resolved=0", [], |r| {
                r.get(0)
            })?)
    }
    /// Only an explicitly completed full scan clears partial coverage for this scope.
    ///
    /// # Errors
    /// Returns invalid input, account mismatch, serialization, or SQLite errors.
    pub fn record_checkpoint(
        &mut self,
        scope: &str,
        pages: u64,
        complete: bool,
        reason: Option<&str>,
    ) -> Result<()> {
        self.validate_owner()?;
        ensure!(
            !scope.is_empty() && pages <= i64::MAX as u64,
            "Invalid reaction checkpoint"
        );
        let complete = complete && pages <= 20 && reason.is_none();
        let reason = if pages > 20 {
            Some("page_limit")
        } else if !complete && reason.is_none() {
            Some("incomplete")
        } else {
            reason
        };
        self.conn.execute("INSERT INTO checkpoints(scope,pages,complete,reason) VALUES(?1,?2,?3,?4) ON CONFLICT(scope) DO UPDATE SET pages=excluded.pages,complete=excluded.complete,reason=excluded.reason",params![scope,pages as i64,complete,reason])?;
        Ok(())
    }
    /// Read counters and explicit coverage checkpoints.
    ///
    /// # Errors
    /// Returns SQLite read errors.
    pub fn status(&self) -> Result<JournalStatus> {
        let mut statement = self
            .conn
            .prepare("SELECT scope,pages,complete,reason FROM checkpoints ORDER BY scope")?;
        let checkpoints = statement
            .query_map([], |r| {
                Ok(CoverageCheckpoint {
                    scope: r.get(0)?,
                    pages: r.get(1)?,
                    complete: r.get(2)?,
                    reason: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let pending = self.pending_count()?;
        Ok(JournalStatus {
            pending,
            total: self
                .conn
                .query_row("SELECT count(*) FROM events", [], |r| r.get(0))?,
            partial: pending > 0
                || checkpoints.is_empty()
                || checkpoints.iter().any(|c| !c.complete),
            checkpoints,
        })
    }
}

/// Parse a verified reaction channel and normalize only the account's documented self aliases.
/// An incoming direct event addressed to the account belongs to its reactor's conversation.
pub fn account_events(payload: &serde_json::Value, account: &str) -> Vec<ReactionEvent> {
    if !known(account) {
        return Vec::new();
    }
    let mut payload = payload;
    for depth in 0..=4 {
        if let Some(code) = payload.get("error_code") {
            let code = code.as_i64().or_else(|| code.as_str()?.parse().ok());
            if code != Some(0) {
                return Vec::new();
            }
        }
        let Some(data) = payload.get("data") else {
            break;
        };
        if depth == 4 {
            return Vec::new();
        }
        payload = data;
    }
    let mut events = crate::reactions::parse_reaction_events(payload);
    for event in &mut events {
        if event.thread_id == account {
            event.thread_id = event.sender_id.clone();
        }
        if event.sender_id == account {
            event.sender_id = "0".into();
        }
    }
    events
}

/// Merge only reaction state into unique existing targets; never resurrect absent messages.
/// Text, sender, timestamps, unread state and non-reaction metadata remain the UI's current state.
pub fn merge_loaded_reactions(current: &mut [Thread], updates: &[Thread]) -> bool {
    let mut changed = false;
    for update in updates {
        let candidates: Vec<_> = current
            .iter()
            .enumerate()
            .filter(|(_, t)| t.id == update.id && t.is_group == update.is_group)
            .map(|(i, _)| i)
            .collect();
        let [thread_index] = candidates.as_slice() else {
            continue;
        };
        let thread = &mut current[*thread_index];
        for incoming in &update.messages {
            let exact: Vec<_> = thread
                .messages
                .iter()
                .enumerate()
                .filter(|(_, m)| known(&incoming.id) && m.id == incoming.id)
                .map(|(i, _)| i)
                .collect();
            let matches = if exact.is_empty() {
                thread
                    .messages
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| {
                        known(&incoming.client_message_id)
                            && m.client_message_id == incoming.client_message_id
                    })
                    .map(|(i, _)| i)
                    .collect()
            } else {
                exact
            };
            let [message_index] = matches.as_slice() else {
                continue;
            };
            let target = &mut thread.messages[*message_index];
            // Reuse #55's chronology reducer with a projection of current non-reaction fields.
            let mut reaction_only = target.clone();
            reaction_only.reaction_summaries = incoming.reaction_summaries.clone();
            reaction_only.reactions = incoming.reactions.clone();
            reaction_only.reaction_snapshot = incoming.reaction_snapshot;
            reaction_only.reaction_authoritative_snapshot =
                incoming.reaction_authoritative_snapshot;
            reaction_only.reaction_snapshot_fresh = incoming.reaction_snapshot_fresh;
            reaction_only.reaction_versions = incoming.reaction_versions.clone();
            reaction_only.reaction_snapshot_provenance =
                incoming.reaction_snapshot_provenance.clone();
            changed |= target.merge_reaction_metadata(&reaction_only);
        }
    }
    changed
}

/// Consume the archive's reaction history queues (610/611) or explicitly tagged reaction data.
/// Other socket channels cannot turn a similarly shaped control payload into a reaction.
pub fn socket_events(
    command: u16,
    payload: &serde_json::Value,
    account: &str,
) -> Vec<ReactionEvent> {
    let historical = matches!(command, 610 | 611);
    let data = payload.get("data").unwrap_or(payload);
    let explicitly_tagged = ["reacts", "reactGroups", "reactions", "messages"]
        .iter()
        .any(|key| data.get(key).is_some())
        || data.get("msgType").and_then(serde_json::Value::as_str) == Some("chat.reaction")
        || data.as_array().is_some_and(|items| {
            items.iter().all(|item| {
                item.get("msgType").and_then(serde_json::Value::as_str) == Some("chat.reaction")
            })
        });
    if !historical && !explicitly_tagged {
        return Vec::new();
    }
    let mut events = account_events(payload, account);
    for event in &mut events {
        event.historical = historical;
    }
    events
}
