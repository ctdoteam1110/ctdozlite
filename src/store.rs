//! Local message and thread store backed by SQLite (rusqlite bundled).
//!
//! Persists every message seen via polling or group history so chat history accumulates locally.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::chat::{ChatMessage, Thread};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredMessage {
    pub msg_id: String,
    pub thread_id: String,
    pub sender_id: String,
    pub sender_name: String,
    pub text: String,
    pub msg_type: String,
    pub ts: i64,
    pub is_outgoing: bool,
    pub raw_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredThread {
    pub thread_id: String,
    pub name: String,
    pub is_group: bool,
    pub last_ts: i64,
}

pub struct MessageStore {
    pub conn: Connection,
}

fn metadata_field<T: serde::de::DeserializeOwned + Default>(
    metadata: &serde_json::Value,
    key: &str,
) -> T {
    metadata
        .get(key)
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default()
}

/// Display names and avatars do not prove that persisted thread IDs are aliases.
fn deduplicate_exact_threads(threads: &mut Vec<Thread>) {
    let mut indices = std::collections::HashMap::new();
    let mut merged: Vec<Thread> = Vec::with_capacity(threads.len());
    for thread in std::mem::take(threads) {
        if thread.id.is_empty() || thread.id == "0" {
            continue;
        }
        let key = (thread.is_group, thread.id.clone());
        if let Some(&index) = indices.get(&key) {
            crate::chat::merge_thread_into(&mut merged[index], thread, false);
        } else {
            indices.insert(key, merged.len());
            merged.push(thread);
        }
    }
    *threads = merged;
}

impl MessageStore {
    /// Messages per thread loaded at startup / on refresh. The list only needs the newest one
    /// for ordering and preview; opening a thread reads deeper (`LoadGroupHistory`).
    pub const PRELOAD_MSGS: usize = 20;

    /// Open SQLite database at `path` and initialize table schemas and indices if absent.
    pub fn open(path: &str) -> Result<Self> {
        let _scope_guard = crate::runtime_scope::mutation()?;
        let conn = if path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(path)?
        };

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                msg_id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                sender_id TEXT,
                sender_name TEXT,
                text TEXT,
                msg_type TEXT,
                ts INTEGER,
                is_outgoing INTEGER,
                raw_json TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_thread_ts ON messages(thread_id, ts);
            CREATE TABLE IF NOT EXISTS threads (
                thread_id TEXT PRIMARY KEY,
                name TEXT,
                is_group INTEGER,
                last_ts INTEGER
            );",
        )?;
        // Migration 1: one-time cleanup of placeholder rows. This DELETE used to run (a full
        // table scan) on every hydrate; new rows are filtered in `persist_threads` instead.
        if conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? < 1 {
            conn.execute_batch(
                "BEGIN;
                 DELETE FROM messages WHERE text = 'sendBubbleMessage';
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }

        Ok(Self { conn })
    }

    /// Insert a message. Returns `true` if a new row was inserted, `false` if already existed.
    pub fn upsert_message(&mut self, msg: &StoredMessage) -> Result<bool> {
        let _scope_guard = crate::runtime_scope::mutation()?;
        let rows = self.conn.execute(
            "INSERT OR IGNORE INTO messages (msg_id, thread_id, sender_id, sender_name, text, msg_type, ts, is_outgoing, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                msg.msg_id,
                msg.thread_id,
                msg.sender_id,
                msg.sender_name,
                msg.text,
                msg.msg_type,
                msg.ts,
                if msg.is_outgoing { 1 } else { 0 },
                msg.raw_json,
            ],
        )?;
        if rows == 0 && !msg.raw_json.is_empty() {
            let _ = self.conn.execute(
                "UPDATE messages SET raw_json = ?1, msg_type = ?2, text = ?4 WHERE msg_id = ?3",
                params![msg.raw_json, msg.msg_type, msg.msg_id, msg.text],
            );
        }
        Ok(rows > 0)
    }

    /// Delete one exact conversation/message identity from the local store.
    /// The legacy timestamp argument remains for call-site compatibility, not matching.
    ///
    /// # Errors
    /// Returns an error for an empty identity or failed SQLite deletion.
    pub fn delete_message(&mut self, thread_id: &str, msg_id: &str, _ts: i64) -> Result<()> {
        let _scope_guard = crate::runtime_scope::mutation()?;
        anyhow::ensure!(
            !thread_id.is_empty() && !msg_id.is_empty(),
            "Cần định danh cuộc trò chuyện và tin nhắn để xóa lịch sử"
        );
        self.conn.execute(
            "DELETE FROM messages WHERE thread_id = ?1 AND msg_id = ?2",
            params![thread_id, msg_id],
        )?;
        Ok(())
    }

    /// Delete an entire conversation and all its messages from local SQLite store (#24).
    pub fn delete_thread(&mut self, thread_id: &str) -> Result<()> {
        let _scope_guard = crate::runtime_scope::mutation()?;
        if thread_id.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "DELETE FROM messages WHERE thread_id = ?1",
            params![thread_id],
        )?;
        self.conn.execute(
            "DELETE FROM threads WHERE thread_id = ?1",
            params![thread_id],
        )?;
        Ok(())
    }

    /// Search messages across all threads by keyword (case-insensitive substring match) (#35).
    pub fn search_messages(&self, query: &str, limit: usize) -> Result<Vec<StoredMessage>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let pattern = format!("%{}%", q.replace('%', "\\%").replace('_', "\\_"));
        let mut stmt = self.conn.prepare(
            "SELECT msg_id, thread_id, sender_id, sender_name, text, msg_type, ts, is_outgoing, raw_json
             FROM messages
             WHERE LOWER(text) LIKE LOWER(?1) ESCAPE '\\'
                OR LOWER(sender_name) LIKE LOWER(?1) ESCAPE '\\'
             ORDER BY ts DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit as i64], |row| {
            let is_outgoing_num: i64 = row.get(7)?;
            Ok(StoredMessage {
                msg_id: row.get(0)?,
                thread_id: row.get(1)?,
                sender_id: row.get(2).unwrap_or_default(),
                sender_name: row.get(3).unwrap_or_default(),
                text: row.get(4).unwrap_or_default(),
                msg_type: row.get(5).unwrap_or_default(),
                ts: row.get(6)?,
                is_outgoing: is_outgoing_num != 0,
                raw_json: row.get(8).unwrap_or_default(),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Insert or update thread metadata.
    pub fn upsert_thread(&mut self, t: &StoredThread) -> Result<()> {
        let _scope_guard = crate::runtime_scope::mutation()?;
        self.conn.execute(
            "INSERT INTO threads (thread_id, name, is_group, last_ts)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(thread_id) DO UPDATE SET
                 name = excluded.name,
                 is_group = excluded.is_group,
                 last_ts = MAX(COALESCE(last_ts, 0), excluded.last_ts)",
            params![
                t.thread_id,
                t.name,
                if t.is_group { 1 } else { 0 },
                t.last_ts,
            ],
        )?;
        Ok(())
    }

    /// Id of the oldest stored server message in a thread, where a history sync resumes from.
    /// Local ids (`out_*`, `{thread}_{ts}`) are skipped: the server rejects them as cursors.
    pub fn oldest_msg_id(&self, thread_id: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT msg_id FROM messages
                 WHERE thread_id = ?1 AND msg_id <> '' AND msg_id NOT GLOB '*[^0-9]*'
                 ORDER BY ts ASC LIMIT 1",
                params![thread_id],
                |row| row.get(0),
            )
            .ok()
    }

    /// Retrieve the newest `limit` messages for a thread, ordered by timestamp ascending.
    pub fn messages_for(&self, thread_id: &str, limit: usize) -> Result<Vec<StoredMessage>> {
        let mut stmt = self.conn.prepare(
            // Newest `limit` rows, returned oldest-first. `ORDER BY ts ASC LIMIT` handed back the
            // *oldest* rows, so a thread longer than the limit reopened on stale messages.
            "SELECT msg_id, thread_id, sender_id, sender_name, text, msg_type, ts, is_outgoing, raw_json
             FROM (
                 SELECT msg_id, thread_id, sender_id, sender_name, text, msg_type, ts, is_outgoing, raw_json
                 FROM messages
                 WHERE thread_id = ?1
                 ORDER BY ts DESC
                 LIMIT ?2
             ) ORDER BY ts ASC",
        )?;

        let rows = stmt.query_map(params![thread_id, limit as i64], |row| {
            let is_outgoing_num: i64 = row.get(7)?;
            Ok(StoredMessage {
                msg_id: row.get(0)?,
                thread_id: row.get(1)?,
                sender_id: row.get(2).unwrap_or_default(),
                sender_name: row.get(3).unwrap_or_default(),
                text: row.get(4).unwrap_or_default(),
                msg_type: row.get(5).unwrap_or_default(),
                ts: row.get(6)?,
                is_outgoing: is_outgoing_num != 0,
                raw_json: row.get(8).unwrap_or_default(),
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    /// Retrieve all persisted thread metadata ordered by `last_ts` descending.
    pub fn all_threads(&self) -> Result<Vec<StoredThread>> {
        let mut stmt = self.conn.prepare(
            "SELECT thread_id, name, is_group, last_ts
             FROM threads
             ORDER BY last_ts DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let is_group_num: i64 = row.get(2).unwrap_or(0);
            Ok(StoredThread {
                thread_id: row.get(0)?,
                name: row.get(1).unwrap_or_default(),
                is_group: is_group_num != 0,
                last_ts: row.get(3).unwrap_or(0),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Hydrate `threads` with persisted threads and message history from SQLite.
    /// Returns the total number of messages loaded into `threads`.
    pub fn hydrate_threads(&self, threads: &mut Vec<Thread>, limit_per_thread: usize) -> Result<usize> {
        let _scope_guard = crate::runtime_scope::mutation()?;
        deduplicate_exact_threads(threads);

        let stored_threads = self.all_threads()?;
        for st in stored_threads {
            if st.thread_id.is_empty() || st.thread_id == "0" {
                continue;
            }
            if let Some(existing) = threads.iter_mut().find(|t| t.id == st.thread_id) {
                if crate::chat::is_generic_thread_name(&existing.name) && !st.name.is_empty() {
                    existing.name = st.name;
                }
                existing.is_group = existing.is_group || st.is_group;
            } else {
                threads.push(Thread {
                    presentation: Default::default(),
                    unread: Default::default(),
                    id: st.thread_id.clone(),
                    name: if st.name.is_empty() {
                        format!("Người dùng {}", st.thread_id)
                    } else {
                        st.name
                    },
                    preview: String::new(),
                    time: String::new(),
                    avatar: None,
                    is_group: st.is_group,
                    messages: Vec::new(),
                });
            }
        }

        // Primary-key lookups for the messages actually in memory. This used to load every
        // (msg_id, thread_id) row in the database into a HashMap on each call. One transaction
        // around them: in autocommit every SELECT takes and drops the file lock, ~50 µs on
        // Windows, which made 62k lookups cost 3 s.
        let tx = self.conn.unchecked_transaction()?;
        let mut owner_of = self
            .conn
            .prepare("SELECT thread_id FROM messages WHERE msg_id = ?1")?;

        let mut loaded_count = 0;
        for t in threads.iter_mut() {
            let before = t.messages.len();
            let tid = t.id.clone();
            t.messages.retain(|m| {
                owner_of
                    .query_row(params![m.id], |r| r.get::<_, String>(0))
                    .map_or(true, |owner| owner == tid)
            });
            if t.messages.len() != before && t.messages.is_empty() {
                t.preview.clear();
                t.time.clear();
                let _ = self.conn.execute(
                    "UPDATE threads SET last_ts = 0 WHERE thread_id = ?1",
                    params![t.id],
                );
            }
            let saved_msgs = self.messages_for(&t.id, limit_per_thread)?;
            let mut saved_messages = Vec::with_capacity(saved_msgs.len());
            for sm in saved_msgs {
                let time = if sm.ts > 0 {
                    crate::clock::message_time(sm.ts)
                } else {
                    crate::ui::clock()
                };
                let mut sticker_id = None;
                let mut sticker_cate_id = None;
                let mut sticker_type = None;
                let mut reply_to_id = None;
                let mut reply_to_sender = None;
                let mut reply_to_text = None;
                let mut reactions = Vec::new();
                let mut kind = Default::default();
                let mut wire_ref = Default::default();
                let mut unread_policy = Default::default();
                let mut delivery = Default::default();
                let mut history_source = Default::default();
                let mut reaction_summaries = Default::default();
                let mut reaction_snapshot = Default::default();
                let mut reaction_authoritative_snapshot = Default::default();
                let mut reaction_snapshot_provenance = Default::default();
                let mut reaction_versions = Default::default();
                let mut client_message_id = Default::default();
                let mut resolved_mention_uids = Default::default();
                let mut is_pinned = false;
                let mut is_starred = false;
                let mut link_url = None;
                let mut link_title = None;
                let mut link_desc = None;
                let mut link_thumb = None;
                let mut link_src = None;
                if !sm.raw_json.is_empty() {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&sm.raw_json) {
                        kind = metadata_field(&val, "kind");
                        wire_ref = metadata_field(&val, "wire_ref");
                        unread_policy = metadata_field(&val, "unread_policy");
                        delivery = metadata_field(&val, "delivery");
                        reaction_summaries = metadata_field(&val, "reaction_summaries");
                        reaction_snapshot = metadata_field(&val, "reaction_snapshot");
                        reaction_authoritative_snapshot =
                            metadata_field(&val, "reaction_authoritative_snapshot");
                        reaction_snapshot_provenance =
                            metadata_field(&val, "reaction_snapshot_provenance");
                        reaction_versions = metadata_field(&val, "reaction_versions");
                        client_message_id = metadata_field(&val, "client_message_id");
                        resolved_mention_uids = metadata_field(&val, "resolved_mention_uids");
                        history_source = val.get("history_source").cloned();
                        sticker_id = val
                            .get("stickerId")
                            .or_else(|| val.get("id"))
                            .and_then(|v| v.as_u64());
                        sticker_cate_id = val
                            .get("cateId")
                            .or_else(|| val.get("catId"))
                            .and_then(|v| v.as_u64());
                        sticker_type = val.get("type").and_then(|v| v.as_u64()).map(|n| n as u32);
                        reply_to_id = val
                            .get("reply_to_id")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                        reply_to_sender = val
                            .get("reply_to_sender")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                        reply_to_text = val
                            .get("reply_to_text")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                        if let Some(arr) = val.get("reactions").and_then(|v| v.as_array()) {
                            reactions = arr
                                .iter()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect();
                        }
                        is_pinned = val
                            .get("is_pinned")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        is_starred = val
                            .get("is_starred")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        link_url = val
                            .get("link_url")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                        link_title = val
                            .get("link_title")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                        link_desc = val
                            .get("link_desc")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                        link_thumb = val
                            .get("link_thumb")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                        link_src = val
                            .get("link_src")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                    }
                }
                let cm = ChatMessage {
                    id: sm.msg_id,
                    sender: if !sm.sender_name.is_empty() {
                        sm.sender_name
                    } else {
                        sm.sender_id
                    },
                    text: sm.text,
                    time,
                    is_outgoing: sm.is_outgoing,
                    ts: sm.ts,
                    sticker_id,
                    sticker_cate_id,
                    sticker_type,
                    reply_to_id,
                    reply_to_sender,
                    reply_to_text,
                    reactions,
                    kind,
                    wire_ref,
                    unread_policy,
                    delivery,
                    history_source,
                    reaction_summaries,
                    reaction_snapshot,
                    reaction_authoritative_snapshot,
                    reaction_snapshot_provenance,
                    reaction_versions,
                    client_message_id,
                    resolved_mention_uids,
                    reaction_snapshot_fresh: false,
                    is_pinned,
                    is_starred,
                    link_url,
                    link_title,
                    link_desc,
                    link_thumb,
                    link_src,

                    ..Default::default()
                };
                saved_messages.push(cm);
            }
            let before = t
                .messages
                .iter()
                .filter(|message| message.text != "sendBubbleMessage")
                .count();
            crate::chat::merge_cached_messages(t, saved_messages);
            loaded_count += t.messages.len().saturating_sub(before);
            if !t.messages.is_empty() {
                t.messages.sort_by_key(|m| m.ts);
                if let Some(newest) = t.messages.last() {
                    if !newest.text.is_empty() {
                        t.preview = newest.text.replace('\n', " ");
                        t.time = newest.time.clone();
                    }
                }
            }
        }
        drop(owner_of);
        tx.commit()?;
        deduplicate_exact_threads(threads);
        Ok(loaded_count)
    }
}

impl From<&ChatMessage> for StoredMessage {
    fn from(m: &ChatMessage) -> Self {
        let mut meta = serde_json::Map::new();
        if m.kind != Default::default() {
            meta.insert("kind".into(), serde_json::json!(m.kind));
        }
        if m.wire_ref != Default::default() {
            meta.insert("wire_ref".into(), serde_json::json!(m.wire_ref));
        }
        if m.unread_policy != Default::default() {
            meta.insert("unread_policy".into(), serde_json::json!(m.unread_policy));
        }
        if m.delivery != Default::default() {
            meta.insert("delivery".into(), serde_json::json!(m.delivery));
        }
        if let Some(source) = &m.history_source {
            meta.insert("history_source".into(), source.clone());
        }
        if !m.reaction_summaries.is_empty() {
            meta.insert(
                "reaction_summaries".into(),
                serde_json::json!(m.reaction_summaries),
            );
        }
        if m.reaction_snapshot {
            meta.insert(
                "reaction_snapshot".into(),
                serde_json::json!(m.reaction_snapshot),
            );
        }
        if m.reaction_authoritative_snapshot {
            meta.insert(
                "reaction_authoritative_snapshot".into(),
                serde_json::json!(m.reaction_authoritative_snapshot),
            );
        }
        if m.reaction_snapshot_provenance != Default::default() {
            meta.insert(
                "reaction_snapshot_provenance".into(),
                serde_json::json!(m.reaction_snapshot_provenance),
            );
        }
        if !m.reaction_versions.is_empty() {
            meta.insert(
                "reaction_versions".into(),
                serde_json::json!(m.reaction_versions),
            );
        }
        if !m.client_message_id.is_empty() {
            meta.insert(
                "client_message_id".into(),
                serde_json::json!(m.client_message_id),
            );
        }
        if !m.resolved_mention_uids.is_empty() {
            meta.insert(
                "resolved_mention_uids".into(),
                serde_json::json!(m.resolved_mention_uids),
            );
        }

        let msg_type = if let Some(sid) = m.sticker_id {
            let cid = m.sticker_cate_id.unwrap_or(0);
            let st = m.sticker_type.unwrap_or(7);
            meta.insert("stickerId".into(), serde_json::json!(sid));
            meta.insert("cateId".into(), serde_json::json!(cid));
            meta.insert("type".into(), serde_json::json!(st));
            "chat.sticker".to_string()
        } else {
            "chat.text".to_string()
        };
        if let Some(ref rid) = m.reply_to_id {
            meta.insert("reply_to_id".into(), serde_json::json!(rid));
        }
        if let Some(ref rsender) = m.reply_to_sender {
            meta.insert("reply_to_sender".into(), serde_json::json!(rsender));
        }
        if let Some(ref rtext) = m.reply_to_text {
            meta.insert("reply_to_text".into(), serde_json::json!(rtext));
        }
        if !m.reactions.is_empty() {
            meta.insert("reactions".into(), serde_json::json!(m.reactions));
        }
        if m.is_pinned {
            meta.insert("is_pinned".into(), serde_json::json!(true));
        }
        if m.is_starred {
            meta.insert("is_starred".into(), serde_json::json!(true));
        }
        if let Some(lurl) = &m.link_url {
            meta.insert("link_url".into(), serde_json::json!(lurl));
        }
        if let Some(ltitle) = &m.link_title {
            meta.insert("link_title".into(), serde_json::json!(ltitle));
        }
        if let Some(ldesc) = &m.link_desc {
            meta.insert("link_desc".into(), serde_json::json!(ldesc));
        }
        if let Some(lthumb) = &m.link_thumb {
            meta.insert("link_thumb".into(), serde_json::json!(lthumb));
        }
        if let Some(lsrc) = &m.link_src {
            meta.insert("link_src".into(), serde_json::json!(lsrc));
        }
        let raw_json = if meta.is_empty() {
            String::new()
        } else {
            serde_json::Value::Object(meta).to_string()
        };
        Self {
            msg_id: if !m.id.is_empty() {
                m.id.clone()
            } else {
                format!("msg_{}", m.ts)
            },
            thread_id: String::new(),
            sender_id: if m.is_outgoing {
                "0".into()
            } else {
                m.sender.clone()
            },
            sender_name: m.sender.clone(),
            text: m.text.clone(),
            msg_type,
            ts: m.ts,
            is_outgoing: m.is_outgoing,
            raw_json,
        }
    }
}

/// Persist threads and their messages to SQLite store, returning the count of newly inserted messages.
pub fn persist_threads(store: &mut MessageStore, threads: &[Thread]) -> Result<usize> {
    let _scope_guard = crate::runtime_scope::mutation()?;
    // One transaction for the whole snapshot: in autocommit every row was its own fsync.
    store.conn.execute_batch("BEGIN")?;
    let res = persist_threads_inner(store, threads);
    store
        .conn
        .execute_batch(if res.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
    res
}

fn persist_threads_inner(store: &mut MessageStore, threads: &[Thread]) -> Result<usize> {
    let mut new_count = 0;
    for t in threads {
        let last_ts = t.messages.iter().map(|m| m.ts).max().unwrap_or(0);
        let st = StoredThread {
            thread_id: t.id.clone(),
            name: t.name.clone(),
            is_group: t.is_group,
            last_ts,
        };
        store.upsert_thread(&st)?;

        for m in t.messages.iter().filter(|m| m.text != "sendBubbleMessage") {
            let mut sm = StoredMessage::from(m);
            sm.thread_id = t.id.clone();
            if sm.msg_id.is_empty() {
                sm.msg_id = format!("{}_{}", t.id, sm.ts);
            }
            if store.upsert_message(&sm)? {
                new_count += 1;
            }
        }
    }
    Ok(new_count)
}

/// Convenience helper: open `zalo_history.db` (or `path`) and hydrate `threads` with saved history.
pub fn load_history_into_threads(path: &str, threads: &mut Vec<Thread>, limit_per_thread: usize) {
    let Ok(_scope_guard) = crate::runtime_scope::mutation() else { return; };
    if let Ok(store) = MessageStore::open(path) {
        let _ = store.hydrate_threads(threads, limit_per_thread);
    }
}

/// Summary returned after migrating/importing history from an external file or directory (#9).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    pub threads_imported: usize,
    pub messages_imported: usize,
    pub sources_read: Vec<String>,
}

/// Import / migrate message history from an existing SQLite database (`.db`/`.sqlite`),
/// a JSON backup (`threads_cache.json` or export JSON), or a directory containing them (#9).
pub fn import_history_from_path(
    src: &std::path::Path,
    dst_db_path: &str,
) -> Result<ImportReport> {
    import_history_impl(src, dst_db_path, false).map(|(report, _)| report)
}

/// Import history and return only source-owned thread/message records for UI merging.
///
/// # Errors
/// Returns source parsing, persistence or source hydration errors without a destination snapshot.
pub fn import_history_delta_from_path(src: &std::path::Path, dst_db_path: &str) -> Result<(ImportReport, Vec<Thread>)> {
    import_history_impl(src, dst_db_path, true)
}

fn import_history_impl(src: &std::path::Path, dst_db_path: &str, capture_delta: bool) -> Result<(ImportReport, Vec<Thread>)> {
    let _scope_guard = crate::runtime_scope::mutation()?;
    if !src.exists() {
        anyhow::bail!("Đường dẫn nguồn không tồn tại: {}", src.display());
    }
    let mut dst = MessageStore::open(dst_db_path)?;
    let mut report = ImportReport::default();
    let mut imported_threads = Vec::new();

    let mut files_to_import = Vec::new();
    if src.is_dir() {
        for candidate in [
            "zalo_history.db",
            "ctdozlite_history.db",
            "threads_cache.json",
            "export.json",
        ] {
            let p = src.join(candidate);
            if p.is_file() {
                files_to_import.push(p);
            }
        }
        if let Ok(entries) = std::fs::read_dir(src) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() && !files_to_import.contains(&p) {
                    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                        if matches!(ext.to_ascii_lowercase().as_str(), "db" | "sqlite" | "sqlite3" | "json") {
                            files_to_import.push(p);
                        }
                    }
                }
            }
        }
    } else {
        files_to_import.push(src.to_path_buf());
    }

    if files_to_import.is_empty() {
        anyhow::bail!(
            "Không tìm thấy tệp dữ liệu lịch sử (.db, .sqlite, .json) trong {}",
            src.display()
        );
    }

    for file_path in files_to_import {
        if let Ok(canon_src) = std::fs::canonicalize(&file_path) {
            if let Ok(canon_dst) = std::fs::canonicalize(dst_db_path) {
                if canon_src == canon_dst {
                    continue;
                }
            }
        }
        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let bytes = std::fs::read(&file_path)?;
        if bytes.starts_with(b"SQLite format 3") || matches!(ext.as_str(), "db" | "sqlite" | "sqlite3") {
            let src_store = MessageStore::open(file_path.to_string_lossy().as_ref())?;
            let src_threads = src_store.all_threads().unwrap_or_default();
            dst.conn.execute_batch("BEGIN")?;
            let step = (|| -> Result<(usize, usize)> {
                let mut t_cnt = 0;
                let mut m_cnt = 0;
                for st in &src_threads {
                    dst.upsert_thread(st)?;
                    t_cnt += 1;
                }
                let mut stmt = src_store.conn.prepare(
                    "SELECT msg_id, thread_id, sender_id, sender_name, text, msg_type, ts, is_outgoing, raw_json
                     FROM messages ORDER BY ts ASC",
                )?;
                let rows = stmt.query_map([], |row| {
                    let is_out: i64 = row.get(7).unwrap_or(0);
                    Ok(StoredMessage {
                        msg_id: row.get(0)?,
                        thread_id: row.get(1)?,
                        sender_id: row.get(2).unwrap_or_default(),
                        sender_name: row.get(3).unwrap_or_default(),
                        text: row.get(4).unwrap_or_default(),
                        msg_type: row.get(5).unwrap_or_default(),
                        ts: row.get(6).unwrap_or(0),
                        is_outgoing: is_out != 0,
                        raw_json: row.get(8).unwrap_or_default(),
                    })
                })?;
                for r in rows {
                    let sm = r?;
                    if dst.upsert_message(&sm)? {
                        m_cnt += 1;
                    }
                }
                Ok((t_cnt, m_cnt))
            })();
            match step {
                Ok((tc, mc)) => {
                    if capture_delta {
                        let mut source_threads = Vec::new();
                        src_store.hydrate_threads(&mut source_threads, i64::MAX as usize)?;
                        imported_threads.extend(source_threads);
                    }
                    dst.conn.execute_batch("COMMIT")?;
                    report.threads_imported += tc;
                    report.messages_imported += mc;
                    report
                        .sources_read
                        .push(file_path.file_name().unwrap_or_default().to_string_lossy().into());
                }
                Err(e) => {
                    let _ = dst.conn.execute_batch("ROLLBACK");
                    return Err(e);
                }
            }
        } else if let Ok(text) = std::str::from_utf8(&bytes) {
            if let Ok(threads) = serde_json::from_str::<Vec<Thread>>(text) {
                let added = persist_threads(&mut dst, &threads)?;
                if capture_delta { imported_threads.extend(threads.iter().cloned()); }
                report.threads_imported += threads.len();
                report.messages_imported += added;
                report
                    .sources_read
                    .push(file_path.file_name().unwrap_or_default().to_string_lossy().into());
            } else if let Ok(val) = serde_json::from_str::<serde_json::Value>(text) {
                if let Some(arr) = val.get("threads").and_then(|v| v.as_array()) {
                    let threads: Vec<Thread> = arr
                        .iter()
                        .filter_map(|item| serde_json::from_value(item.clone()).ok())
                        .collect();
                    if !threads.is_empty() {
                        let added = persist_threads(&mut dst, &threads)?;
                        if capture_delta { imported_threads.extend(threads.iter().cloned()); }
                        report.threads_imported += threads.len();
                        report.messages_imported += added;
                        report
                            .sources_read
                            .push(file_path.file_name().unwrap_or_default().to_string_lossy().into());
                    }
                }
            }
        }
    }

    Ok((report, imported_threads))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_time_when_hydrating_seconds_and_milliseconds() {
        let mut store = MessageStore::open(":memory:").unwrap();
        store
            .upsert_thread(&StoredThread {
                thread_id: "thread-1".into(),
                name: "Example".into(),
                is_group: false,
                last_ts: 1793467800000,
            })
            .unwrap();
        for (id, ts) in [("msg-0001", 1793467800), ("msg-0002", 1793467800000)] {
            store
                .upsert_message(&StoredMessage {
                    msg_id: id.into(),
                    thread_id: "thread-1".into(),
                    sender_id: "user-1".into(),
                    sender_name: "Example".into(),
                    text: "hello".into(),
                    msg_type: "chat.text".into(),
                    ts,
                    is_outgoing: false,
                    raw_json: String::new(),
                })
                .unwrap();
        }
        let mut threads = Vec::new();
        assert_eq!(store.hydrate_threads(&mut threads, 10).unwrap(), 2);
        let expected = crate::clock::message_time(1793467800000);
        assert_eq!(threads[0].time, expected);
        for message in &threads[0].messages {
            assert_eq!(message.time, expected);
        }
        assert_eq!(
            store.messages_for("thread-1", 10).unwrap()[0].ts,
            1793467800
        );
    }

    #[test]
    fn test_insert_twice_returns_false() {
        let mut store = MessageStore::open(":memory:").unwrap();
        let msg = StoredMessage {
            msg_id: "m_1001".into(),
            thread_id: "thread_1".into(),
            sender_id: "u_1".into(),
            sender_name: "Alice".into(),
            text: "Xin chao".into(),
            msg_type: "chat.text".into(),
            ts: 1729500000000,
            is_outgoing: false,
            raw_json: "".into(),
        };

        assert!(store.upsert_message(&msg).unwrap());
        assert!(!store.upsert_message(&msg).unwrap());
    }

    #[test]
    fn test_messages_for_returns_ordered_rows() {
        let mut store = MessageStore::open(":memory:").unwrap();
        let thread_id = "thread_order_test";

        let m1 = StoredMessage {
            msg_id: "m1".into(),
            thread_id: thread_id.into(),
            sender_id: "u1".into(),
            sender_name: "User".into(),
            text: "Msg 200".into(),
            msg_type: "chat.text".into(),
            ts: 200,
            is_outgoing: false,
            raw_json: "".into(),
        };
        let m2 = StoredMessage {
            msg_id: "m2".into(),
            thread_id: thread_id.into(),
            sender_id: "u1".into(),
            sender_name: "User".into(),
            text: "Msg 100".into(),
            msg_type: "chat.text".into(),
            ts: 100,
            is_outgoing: false,
            raw_json: "".into(),
        };
        let m3 = StoredMessage {
            msg_id: "m3".into(),
            thread_id: thread_id.into(),
            sender_id: "u1".into(),
            sender_name: "User".into(),
            text: "Msg 300".into(),
            msg_type: "chat.text".into(),
            ts: 300,
            is_outgoing: false,
            raw_json: "".into(),
        };

        store.upsert_message(&m1).unwrap();
        store.upsert_message(&m2).unwrap();
        store.upsert_message(&m3).unwrap();

        let msgs = store.messages_for(thread_id, 10).unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].ts, 100);
        assert_eq!(msgs[0].msg_id, "m2");
        assert_eq!(msgs[1].ts, 200);
        assert_eq!(msgs[1].msg_id, "m1");
        assert_eq!(msgs[2].ts, 300);
        assert_eq!(msgs[2].msg_id, "m3");

        // A limit keeps the *newest* rows, still returned oldest-first.
        let newest = store.messages_for(thread_id, 2).unwrap();
        assert_eq!(newest.iter().map(|m| m.ts).collect::<Vec<_>>(), vec![200, 300]);
    }

    #[test]
    fn oldest_msg_id_skips_local_ids_and_last_ts_never_goes_back() {
        let mut store = MessageStore::open(":memory:").unwrap();
        for (id, ts) in [("out_1", 50), ("g1_60", 60), ("1002", 100), ("1003", 300)] {
            store
                .upsert_message(&StoredMessage {
                    msg_id: id.into(),
                    thread_id: "g1".into(),
                    sender_id: "u1".into(),
                    sender_name: "User".into(),
                    text: id.into(),
                    msg_type: "chat.text".into(),
                    ts,
                    is_outgoing: false,
                    raw_json: "".into(),
                })
                .unwrap();
        }
        assert_eq!(store.oldest_msg_id("g1").as_deref(), Some("1002"));
        assert_eq!(store.oldest_msg_id("missing"), None);

        // Persisting a page of older history must not move the thread down the list.
        let st = |last_ts| StoredThread {
            thread_id: "g1".into(),
            name: "Nhóm".into(),
            is_group: true,
            last_ts,
        };
        store.upsert_thread(&st(300)).unwrap();
        store.upsert_thread(&st(100)).unwrap();
        assert_eq!(store.all_threads().unwrap()[0].last_ts, 300);
    }

    #[test]
    fn test_persist_threads_and_upsert_thread() {
        let mut store = MessageStore::open(":memory:").unwrap();
        let thread = Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: "t_test".into(),
            name: "Test Thread".into(),
            preview: "Preview".into(),
            time: "10:00".into(),
            avatar: None,
            is_group: false,
            messages: vec![
                ChatMessage {
                    id: "pt_m1".into(),
                    sender: "Alice".into(),
                    text: "Hello".into(),
                    time: "10:00".into(),
                    is_outgoing: false,
                    ts: 1000,
                    ..Default::default()
                },
                ChatMessage {
                    id: "pt_m2".into(),
                    sender: "Tôi".into(),
                    text: "Hi Alice".into(),
                    time: "10:01".into(),
                    is_outgoing: true,
                    ts: 2000,
                    ..Default::default()
                },
            ],
        };

        let count1 = persist_threads(&mut store, &[thread.clone()]).unwrap();
        assert_eq!(count1, 2);

        // Second persist returns 0 newly inserted
        let count2 = persist_threads(&mut store, &[thread]).unwrap();
        assert_eq!(count2, 0);

        let msgs = store.messages_for("t_test", 10).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].text, "Hello");
        assert_eq!(msgs[1].text, "Hi Alice");

        // Hydrate into an empty thread list
        let mut hydrated: Vec<Thread> = Vec::new();
        let loaded = store.hydrate_threads(&mut hydrated, 50).unwrap();
        assert_eq!(loaded, 2);
        assert_eq!(hydrated.len(), 1);
        assert_eq!(hydrated[0].id, "t_test");
        assert_eq!(hydrated[0].name, "Test Thread");
        assert_eq!(hydrated[0].messages.len(), 2);
        assert_eq!(hydrated[0].preview, "Hi Alice");
    }

    #[test]
    fn delete_thread_and_search_messages_work() {
        let mut store = MessageStore::open(":memory:").unwrap();
        store
            .upsert_thread(&StoredThread {
                thread_id: "t1".into(),
                name: "Nhóm Kỹ Thuật".into(),
                is_group: true,
                last_ts: 500,
            })
            .unwrap();
        store
            .upsert_message(&StoredMessage {
                msg_id: "m_search_1".into(),
                thread_id: "t1".into(),
                sender_id: "u1".into(),
                sender_name: "Minh".into(),
                text: "Triển khai bản cập nhật ctdozlite".into(),
                msg_type: "chat.text".into(),
                ts: 500,
                is_outgoing: false,
                raw_json: String::new(),
            })
            .unwrap();

        let hits = store.search_messages("ctdozlite", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].msg_id, "m_search_1");

        store.delete_thread("t1").unwrap();
        assert!(store.messages_for("t1", 10).unwrap().is_empty());
        assert!(store.all_threads().unwrap().is_empty());
    }

    #[test]
    fn import_history_from_path_migrates_sqlite_and_json_directory() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ctdozlite-import-test-{nonce}"));
        std::fs::create_dir_all(&dir).unwrap();
        let src_db = dir.join("zalo_history.db");
        {
            let mut src_store = MessageStore::open(src_db.to_str().unwrap()).unwrap();
            src_store
                .upsert_thread(&StoredThread {
                    thread_id: "g_mig".into(),
                    name: "Migrated Group".into(),
                    is_group: true,
                    last_ts: 999,
                })
                .unwrap();
            src_store
                .upsert_message(&StoredMessage {
                    msg_id: "mig_1".into(),
                    thread_id: "g_mig".into(),
                    sender_id: "u1".into(),
                    sender_name: "Alice".into(),
                    text: "Tin nhắn cũ đã migrate".into(),
                    msg_type: "chat.text".into(),
                    ts: 999,
                    is_outgoing: false,
                    raw_json: String::new(),
                })
                .unwrap();
        }
        let dst_db = dir.join("dest.db");
        let rep = import_history_from_path(&src_db, dst_db.to_str().unwrap()).unwrap();
        assert_eq!(rep.threads_imported, 1);
        assert_eq!(rep.messages_imported, 1);

        let dst_store = MessageStore::open(dst_db.to_str().unwrap()).unwrap();
        let msgs = dst_store.messages_for("g_mig", 10).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "Tin nhắn cũ đã migrate");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
