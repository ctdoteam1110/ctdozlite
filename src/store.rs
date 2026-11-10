//! Local message and thread store backed by SQLite (rusqlite bundled).
//!
//! Persists every message seen via polling or group history so chat history accumulates locally.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
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
    #[serde(default)]
    pub last_read_msg_id: String,
}

pub struct MessageStore {
    pub conn: Connection,
}
/// Returns the database path scoped to the active account UIN.
/// If `uin` is provided: `accounts/<uin>_history.db`.
/// If `uin` is empty: falls back to `zalo_history.db`.
pub fn db_path_for_account(uin: &str) -> String {
    if uin.is_empty() {
        "zalo_history.db".to_string()
    } else {
        format!("accounts/{uin}_history.db")
    }
}

impl MessageStore {
    /// Open canonical account history, importing already-owned legacy history once.
    ///
    /// # Errors
    /// Returns an error for an invalid UIN or a failed database operation.
    pub fn open_account(uin: &str) -> Result<Self> {
        Self::open_account_in(std::path::Path::new("."), uin)
    }

    /// Open account history below an explicit application root without changing the process directory.
    ///
    /// # Errors
    /// Returns an error for an invalid UIN or a failed migration. Legacy history is never
    /// imported without an existing matching ownership record.
    pub fn open_account_in(root: &std::path::Path, uin: &str) -> Result<Self> {
        crate::account_manager::validate_uin(uin)?;
        std::fs::create_dir_all(root.join("accounts"))?;
        let path = root.join(db_path_for_account(uin));
        let mut store = Self::open(
            path.to_str()
                .ok_or_else(|| anyhow::anyhow!("Đường dẫn dữ liệu không hợp lệ"))?,
        )
        .context("Không thể mở kho dữ liệu tài khoản")?;
        let imported: bool = store
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM account_store_metadata WHERE key = 'legacy_imported')",
                [],
                |row| row.get(0),
            )
            .context("Không thể đọc trạng thái nhập lịch sử")?;
        if imported {
            return Ok(store);
        }
        let legacy_path = root.join("zalo_history.db");
        if !legacy_path.is_file() {
            return Ok(store);
        }
        // The current login does not prove who wrote a database left behind by logout.
        // Unowned history stays untouched for explicit import through Settings.
        let probe =
            Connection::open_with_flags(&legacy_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let has_metadata: bool = probe.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'account_store_metadata')",
            [], |row| row.get(0),
        )?;
        if !has_metadata {
            return Ok(store);
        }
        let owner: Option<String> = probe
            .query_row(
                "SELECT value FROM account_store_metadata WHERE key = 'legacy_owner'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if owner.as_deref() != Some(uin) {
            return Ok(store);
        }
        drop(probe);
        let legacy = Self::open(
            legacy_path
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Đường dẫn dữ liệu không hợp lệ"))?,
        )
        .context("Không thể mở kho dữ liệu cũ")?;
        let has_scheduled_messages: bool = legacy.conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'scheduled_messages')", [], |row| row.get(0))?;
        if has_scheduled_messages {
            crate::scheduler::init_scheduler_schema(&store.conn)?;
        }
        store
            .conn
            .execute(
                "ATTACH DATABASE ?1 AS legacy_history",
                [legacy_path.to_string_lossy().as_ref()],
            )
            .context("Không thể gắn kho dữ liệu cũ")?;
        let tx = store
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .context("Không thể khóa giao dịch nhập lịch sử cũ")?;
        let imported: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM account_store_metadata WHERE key = 'legacy_imported')",
            [],
            |row| row.get(0),
        )?;
        if !imported {
            // Read through SQLite so committed WAL rows are included. Canonical rows
            // win on ID collisions; migration must not overwrite newer account data.
            tx.execute_batch("INSERT OR IGNORE INTO messages(msg_id, thread_id, sender_id, sender_name, text, msg_type, ts, is_outgoing, raw_json)
                SELECT msg_id, thread_id, sender_id, sender_name, text, msg_type, ts, is_outgoing, raw_json FROM legacy_history.messages;
                INSERT OR IGNORE INTO threads(thread_id, name, is_group, last_ts, last_read_msg_id)
                SELECT thread_id, name, is_group, last_ts, last_read_msg_id FROM legacy_history.threads;
                INSERT INTO account_store_metadata(key, value) VALUES ('legacy_imported', '1');")?;
            if has_scheduled_messages {
                // Scheduler IDs are local autoincrement values, so both databases can
                // contain unrelated ID 1. Allocate fresh IDs instead of dropping either row.
                tx.execute_batch("INSERT INTO scheduled_messages(thread_id, thread_name, is_group, text, deliver_at, created_at, status, error_msg)
                    SELECT thread_id, thread_name, is_group, text, deliver_at, created_at, status, error_msg FROM legacy_history.scheduled_messages;")?;
            }
        }
        tx.commit()
            .context("Không thể hoàn tất giao dịch nhập lịch sử cũ")?;
        store
            .conn
            .execute_batch("DETACH DATABASE legacy_history")
            .context("Không thể đóng kho dữ liệu cũ")?;
        Ok(store)
    }

    /// Messages per thread loaded at startup / on refresh. The list only needs the newest one
    /// for ordering and preview; opening a thread reads deeper (`LoadGroupHistory`).
    pub const PRELOAD_MSGS: usize = 20;

    /// Open SQLite database at `path` and initialize table schemas and indices if absent.
    pub fn open(path: &str) -> Result<Self> {
        let mut conn = if path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(path)?
        };

        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match conn.pragma_update(None, "journal_mode", "WAL") {
                Ok(()) => break,
                // Concurrent first opens can race while switching a new file to WAL;
                // SQLite does not always invoke the busy handler for that transition.
                Err(rusqlite::Error::SqliteFailure(error, _))
                    if error.code == rusqlite::ErrorCode::DatabaseBusy
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => return Err(error.into()),
            }
        }
        conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")?;
        // Serialize schema inspection and migration before taking a read snapshot.
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let version: i64 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > 2 {
            anyhow::bail!("Dữ liệu được tạo bởi phiên bản mới hơn; vui lòng nâng cấp ứng dụng");
        }
        tx.execute_batch(
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
                last_ts INTEGER,
                last_read_msg_id TEXT NOT NULL DEFAULT ''
            );",
        )?;

        // Ensure `last_read_msg_id` column exists for databases created before this migration (#119).
        let cols = tx
            .prepare("PRAGMA table_info(threads)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(std::result::Result::ok)
            .collect::<Vec<_>>();
        if !cols.iter().any(|c| c == "last_read_msg_id") {
            tx.execute(
                "ALTER TABLE threads ADD COLUMN last_read_msg_id TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }

        // Migration 1: one-time cleanup of placeholder rows. This DELETE used to run (a full
        // table scan) on every hydrate; new rows are filtered in `persist_threads` instead.
        if version < 1 {
            tx.execute_batch(
                "DELETE FROM messages WHERE text = 'sendBubbleMessage';
                 PRAGMA user_version = 1;",
            )?;
        }
        if version < 2 {
            tx.execute_batch("CREATE TABLE IF NOT EXISTS account_store_metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                PRAGMA user_version = 2;")?;
        }
        tx.commit()?;

        Ok(Self { conn })
    }

    /// Insert a message. Returns `true` if a new row was inserted, `false` if already existed.
    pub fn upsert_message(&mut self, msg: &StoredMessage) -> Result<bool> {
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
            let previous: Option<String> = self.conn.query_row(
                "SELECT raw_json FROM messages WHERE msg_id = ?1",
                [&msg.msg_id],
                |row| row.get(0),
            )?;
            let raw_json = preserve_reaction_replay_metadata(
                previous.as_deref().unwrap_or_default(),
                &msg.raw_json,
            );
            self.conn.execute(
                "UPDATE messages SET raw_json = ?1, msg_type = ?2, text = ?4 WHERE msg_id = ?3",
                params![raw_json, msg.msg_type, msg.msg_id, msg.text],
            )?;
        }
        Ok(rows > 0)
    }

    /// Delete a message from local SQLite store when user selects "Xóa chỉ ở phía tôi".
    pub fn delete_message(&mut self, thread_id: &str, msg_id: &str, ts: i64) -> Result<()> {
        if !msg_id.is_empty() {
            let _ = self
                .conn
                .execute("DELETE FROM messages WHERE msg_id = ?1", params![msg_id]);
        }
        if ts > 0 && !thread_id.is_empty() {
            let _ = self.conn.execute(
                "DELETE FROM messages WHERE thread_id = ?1 AND ts = ?2",
                params![thread_id, ts],
            );
        }
        Ok(())
    }

    /// Delete an entire conversation and all its messages from local SQLite store (#24).
    pub fn delete_thread(&mut self, thread_id: &str) -> Result<()> {
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
        self.conn.execute(
            "INSERT INTO threads (thread_id, name, is_group, last_ts, last_read_msg_id)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(thread_id) DO UPDATE SET
                 name = CASE
                     WHEN excluded.name IS NOT NULL AND excluded.name != ''
                          AND excluded.name NOT LIKE 'Người dùng %'
                          AND excluded.name NOT LIKE 'Nhóm %'
                          AND (threads.name IS NULL OR threads.name = '' OR threads.name LIKE 'Người dùng %' OR threads.name LIKE 'Nhóm %')
                     THEN excluded.name
                     ELSE COALESCE(NULLIF(threads.name, ''), excluded.name)
                 END,
                 is_group = excluded.is_group,
                 last_ts = MAX(COALESCE(last_ts, 0), excluded.last_ts),
                 last_read_msg_id = CASE
                     WHEN excluded.last_read_msg_id IS NOT NULL AND excluded.last_read_msg_id != ''
                     THEN excluded.last_read_msg_id
                     ELSE threads.last_read_msg_id
                 END",
            params![
                t.thread_id,
                t.name,
                if t.is_group { 1 } else { 0 },
                t.last_ts,
                t.last_read_msg_id,
            ],
        )?;
        Ok(())
    }

    /// Persist the read-point (last read message ID) of a thread (#119).
    /// Upserts safely even if the thread does not have a row yet.
    pub fn set_thread_read(&mut self, thread_id: &str, last_msg_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO threads (thread_id, last_read_msg_id)
             VALUES (?1, ?2)
             ON CONFLICT(thread_id) DO UPDATE SET last_read_msg_id = excluded.last_read_msg_id",
            params![thread_id, last_msg_id],
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
        let has_read_col = self
            .conn
            .prepare("PRAGMA table_info(threads)")
            .ok()
            .and_then(|mut s| {
                s.query_map([], |r| r.get::<_, String>(1))
                    .ok()
                    .map(|rows| rows.flatten().any(|c| c == "last_read_msg_id"))
            })
            .unwrap_or(false);

        let sql = if has_read_col {
            "SELECT thread_id, name, is_group, last_ts, last_read_msg_id
             FROM threads
             ORDER BY last_ts DESC"
        } else {
            "SELECT thread_id, name, is_group, last_ts, ''
             FROM threads
             ORDER BY last_ts DESC"
        };

        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| {
            let is_group_num: i64 = row.get(2).unwrap_or(0);
            Ok(StoredThread {
                thread_id: row.get(0)?,
                name: row.get(1).unwrap_or_default(),
                is_group: is_group_num != 0,
                last_ts: row.get(3).unwrap_or(0),
                last_read_msg_id: row.get(4).unwrap_or_default(),
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
    pub fn hydrate_threads(
        &self,
        threads: &mut Vec<Thread>,
        limit_per_thread: usize,
    ) -> Result<usize> {
        crate::chat::deduplicate_threads(threads, None);

        let stored_threads = self.all_threads().unwrap_or_default();
        for st in stored_threads {
            if st.thread_id.is_empty() || st.thread_id == "0" {
                continue;
            }
            if let Some(existing) = threads.iter_mut().find(|t| t.id == st.thread_id) {
                if !st.name.is_empty() && !crate::chat::is_generic_thread_name(&st.name) {
                    existing.name = st.name;
                }
                existing.is_group = existing.is_group || st.is_group;
                // Restore persisted read-point (#119) so read messages are not recounted as unread.
                if !st.last_read_msg_id.is_empty() {
                    existing.unread.last_seen_id = Some(st.last_read_msg_id.clone());
                }
            } else if !st.is_group
                && st.name.contains("Cloud của tôi")
                && threads
                    .iter()
                    .any(|t| !t.is_group && t.name.contains("Cloud của tôi"))
            {
                // Migrate stale session-scoped `send2me_id` ("Cloud của tôi") in SQLite to the active session's `id`.
                // Normal contacts/groups must NEVER be merged by display name alone (#63).
                if let Some(live_id) = threads
                    .iter()
                    .find(|t| !t.is_group && t.name.contains("Cloud của tôi"))
                    .map(|t| t.id.clone())
                {
                    let _ = self.conn.execute(
                        "UPDATE messages SET thread_id = ?1 WHERE thread_id = ?2",
                        params![live_id, st.thread_id],
                    );
                    let _ = self.conn.execute(
                        "DELETE FROM threads WHERE thread_id = ?1",
                        params![st.thread_id],
                    );
                }
            } else {
                let mut unread = crate::unread::UnreadState::default();
                if !st.last_read_msg_id.is_empty() {
                    unread.last_seen_id = Some(st.last_read_msg_id.clone());
                }
                threads.push(Thread {
                    presentation: Default::default(),
                    unread,
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
                let mut reaction_summaries = Vec::new();
                let mut reaction_versions = std::collections::BTreeMap::new();
                let mut is_pinned = false;
                let mut is_starred = false;
                let mut link_url = None;
                let mut link_title = None;
                let mut link_desc = None;
                let mut link_thumb = None;
                let mut link_src = None;
                if !sm.raw_json.is_empty() {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&sm.raw_json) {
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
                        if let Some(sums) = val.get("reaction_summaries") {
                            if let Ok(parsed_sums) = serde_json::from_value::<
                                Vec<crate::reactions::ReactionSummary>,
                            >(sums.clone())
                            {
                                reaction_summaries = parsed_sums;
                            }
                        }
                        if let Some(versions) = val.get("reaction_versions") {
                            reaction_versions =
                                serde_json::from_value(versions.clone()).unwrap_or_default();
                        }
                        if reaction_summaries.is_empty() && !reactions.is_empty() {
                            reaction_summaries = reactions
                                .iter()
                                .map(|e| crate::reactions::ReactionSummary {
                                    emoji: e.clone(),
                                    count: 1,
                                    ..Default::default()
                                })
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
                    reaction_summaries,
                    reaction_versions,
                    is_pinned,
                    is_starred,
                    link_url,
                    link_title,
                    link_desc,
                    link_thumb,
                    link_src,

                    ..Default::default()
                };
                if let Some(existing) = t.messages.iter_mut().find(|existing| {
                    (!existing.id.is_empty() && existing.id == cm.id)
                        || (existing.text == cm.text
                            && existing.is_outgoing == cm.is_outgoing
                            && (existing.ts == cm.ts
                                || (existing.ts != 0
                                    && cm.ts != 0
                                    && (existing.ts - cm.ts).abs() < 5000)))
                }) {
                    if existing.sticker_id.is_none() && cm.sticker_id.is_some() {
                        existing.sticker_id = cm.sticker_id;
                        existing.sticker_cate_id = cm.sticker_cate_id;
                        existing.sticker_type = cm.sticker_type;
                    }
                    if existing.reply_to_text.is_none() && cm.reply_to_text.is_some() {
                        existing.reply_to_id = cm.reply_to_id;
                        existing.reply_to_sender = cm.reply_to_sender;
                        existing.reply_to_text = cm.reply_to_text;
                    }
                    if existing.link_url.is_none() && cm.link_url.is_some() {
                        existing.link_url = cm.link_url;
                        existing.link_title = cm.link_title;
                        existing.link_desc = cm.link_desc;
                        existing.link_thumb = cm.link_thumb;
                        existing.link_src = cm.link_src;
                        if !cm.text.is_empty() {
                            existing.text = cm.text;
                        }
                    }
                    if existing.reactions.is_empty() && !cm.reactions.is_empty() {
                        existing.reactions = cm.reactions;
                    }
                    if existing.reaction_summaries.is_empty() && !cm.reaction_summaries.is_empty() {
                        crate::reactions::trace_count_change(
                            "store:load_from_db",
                            &existing.id,
                            0,
                            crate::reactions::total_count(&cm.reaction_summaries),
                        );
                        existing.reaction_summaries = cm.reaction_summaries;
                    }
                    for (sender, version) in cm.reaction_versions {
                        let current = existing
                            .reaction_versions
                            .entry(sender)
                            .or_insert_with(|| version.clone());
                        for event in version.events.iter().chain(&version.covered_events) {
                            current.cover(event);
                        }
                    }
                    existing.is_pinned = existing.is_pinned || cm.is_pinned;
                    existing.is_starred = existing.is_starred || cm.is_starred;
                } else {
                    t.messages.push(cm);
                    loaded_count += 1;
                }
            }
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
        crate::chat::deduplicate_threads(threads, None);
        Ok(loaded_count)
    }
}

// Counts may be refreshed from history without event IDs. Retain replay identities across
// metadata upserts; an older or incomplete writer cannot erase the local deduplication ledger.
fn preserve_reaction_replay_metadata(previous: &str, incoming: &str) -> String {
    type Versions = std::collections::BTreeMap<String, crate::reactions::ReactionVersion>;
    let old: Versions = serde_json::from_str::<serde_json::Value>(previous)
        .ok()
        .and_then(|value| value.get("reaction_versions").cloned())
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    if old.is_empty() {
        return incoming.to_string();
    }
    let Ok(serde_json::Value::Object(mut metadata)) = serde_json::from_str(incoming) else {
        return incoming.to_string();
    };
    let mut versions: Versions = metadata
        .get("reaction_versions")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    for (sender, old_version) in old {
        let current = versions
            .entry(sender)
            .or_insert_with(|| old_version.clone());
        for event in old_version.events.iter().chain(&old_version.covered_events) {
            current.cover(event);
        }
    }
    metadata.insert("reaction_versions".into(), serde_json::json!(versions));
    serde_json::Value::Object(metadata).to_string()
}

impl From<&ChatMessage> for StoredMessage {
    fn from(m: &ChatMessage) -> Self {
        let mut meta = serde_json::Map::new();
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
        if !m.reaction_summaries.is_empty() {
            meta.insert(
                "reaction_summaries".into(),
                serde_json::json!(m.reaction_summaries),
            );
        }
        if !m.reaction_versions.is_empty() {
            meta.insert(
                "reaction_versions".into(),
                serde_json::json!(m.reaction_versions),
            );
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
            last_read_msg_id: t.unread.last_seen_id.clone().unwrap_or_default(),
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
pub fn import_history_from_path(src: &std::path::Path, dst_db_path: &str) -> Result<ImportReport> {
    if !src.exists() {
        anyhow::bail!("Đường dẫn nguồn không tồn tại: {}", src.display());
    }
    let mut dst = MessageStore::open(dst_db_path)?;
    let mut report = ImportReport::default();

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
                        if matches!(
                            ext.to_ascii_lowercase().as_str(),
                            "db" | "sqlite" | "sqlite3" | "json"
                        ) {
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

    // Classify every selected source before committing any plaintext export.
    for file_path in &files_to_import {
        let bytes = std::fs::read(file_path)?;
        if !bytes.starts_with(b"SQLite format 3")
            && std::str::from_utf8(&bytes).is_err()
            && bytes.len() >= 4096
        {
            anyhow::bail!("Chưa hỗ trợ nhập cơ sở dữ liệu Zalo PC được mã hóa khi chưa xác minh được khóa của tài khoản");
        }
    }
    // Reject unsupported encrypted directory layouts before plaintext writes, too.
    if src.is_dir() {
        scan_and_import_zalo_data_dir(src, &mut dst, &mut report)?;
    }

    for file_path in files_to_import {
        if let Ok(canon_src) = std::fs::canonicalize(&file_path) {
            if let Ok(canon_dst) = std::fs::canonicalize(dst_db_path) {
                if canon_src == canon_dst {
                    continue;
                }
            }
        }
        let bytes = std::fs::read(&file_path)?;
        if bytes.starts_with(b"SQLite format 3") {
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
                    dst.conn.execute_batch("COMMIT")?;
                    report.threads_imported += tc;
                    report.messages_imported += mc;
                    report.sources_read.push(
                        file_path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into(),
                    );
                }
                Err(e) => {
                    let _ = dst.conn.execute_batch("ROLLBACK");
                    return Err(e);
                }
            }
        } else if let Ok(text) = std::str::from_utf8(&bytes) {
            if let Ok(threads) = serde_json::from_str::<Vec<Thread>>(text) {
                let added = persist_threads(&mut dst, &threads)?;
                report.threads_imported += threads.len();
                report.messages_imported += added;
                report.sources_read.push(
                    file_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into(),
                );
            } else if let Ok(val) = serde_json::from_str::<serde_json::Value>(text) {
                if let Some(arr) = val.get("threads").and_then(|v| v.as_array()) {
                    let threads: Vec<Thread> = arr
                        .iter()
                        .filter_map(|item| serde_json::from_value(item.clone()).ok())
                        .collect();
                    if !threads.is_empty() {
                        let added = persist_threads(&mut dst, &threads)?;
                        report.threads_imported += threads.len();
                        report.messages_imported += added;
                        report.sources_read.push(
                            file_path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into(),
                        );
                    }
                }
            }
        } else if bytes.len() >= 4096 {
            anyhow::bail!("Chưa hỗ trợ nhập cơ sở dữ liệu Zalo PC được mã hóa khi chưa xác minh được khóa của tài khoản");
        }
    }

    Ok(report)
}

/// Scan a directory for Zalo PC's `Core/Message` database folder and `Index.db`, decrypting and importing them.
fn scan_and_import_zalo_data_dir(
    root: &std::path::Path,
    dst: &mut MessageStore,
    report: &mut ImportReport,
) -> Result<()> {
    // 2. Discover user folders under Database/_production
    let mut message_dirs = Vec::new();
    let mut index_files = Vec::new();

    let search_roots = vec![root.to_path_buf()];

    for s_root in search_roots {
        let prod = s_root.join("Database").join("_production");
        if let Ok(entries) = std::fs::read_dir(&prod) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let msg_dir = p.join("Core").join("Message");
                    if msg_dir.is_dir() {
                        message_dirs.push(msg_dir);
                    }
                    let idx_file = p.join("Core").join("Index.db");
                    if idx_file.is_file() {
                        index_files.push(idx_file);
                    }
                }
            }
        }
    }

    if message_dirs.is_empty() && index_files.is_empty() {
        return Ok(());
    }
    let cipher_key = recover_zalo_localstorage_key(root)?;

    // 3. Build contact/group name dictionary from Index.db
    let mut id_to_name: std::collections::HashMap<String, (String, bool)> =
        std::collections::HashMap::new();
    for idx_path in index_files {
        if let Ok(bytes) = std::fs::read(&idx_path) {
            if let Some(dec) = decrypt_sqlcipher_bytes(&bytes, cipher_key.as_bytes()) {
                let tmp_p = std::env::temp_dir().join(format!("idx_{}.db", std::process::id()));
                if std::fs::write(&tmp_p, &dec).is_ok() {
                    if let Ok(conn) = Connection::open(&tmp_p) {
                        let _ = query_names_from_index_db(&conn, &mut id_to_name);
                    }
                    let _ = std::fs::remove_file(&tmp_p);
                }
            }
        }
    }

    // 4. Import all message databases in Core/Message
    for msg_dir in message_dirs {
        if let Ok(entries) = std::fs::read_dir(msg_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("db") {
                    let stem = p
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default()
                        .to_string();
                    if let Ok(bytes) = std::fs::read(&p) {
                        if let Some(dec) = decrypt_sqlcipher_bytes(&bytes, cipher_key.as_bytes()) {
                            let tmp_p = std::env::temp_dir().join(format!(
                                "msg_{}_{}.db",
                                std::process::id(),
                                stem
                            ));
                            if std::fs::write(&tmp_p, &dec).is_ok() {
                                if let Ok(conn) = Connection::open(&tmp_p) {
                                    let thread_id = stem.clone();
                                    let (t_name, is_group) =
                                        id_to_name.get(&thread_id).cloned().unwrap_or_else(|| {
                                            (
                                                format!("Người dùng {thread_id}"),
                                                thread_id.starts_with('g'),
                                            )
                                        });
                                    let _ = import_message_table(
                                        &conn,
                                        dst,
                                        &thread_id,
                                        &t_name,
                                        is_group,
                                        &cipher_key,
                                        report,
                                    );
                                }
                                let _ = std::fs::remove_file(&tmp_p);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Attempt to query contact & group display names from a decrypted `Index.db`.
fn query_names_from_index_db(
    conn: &Connection,
    out: &mut std::collections::HashMap<String, (String, bool)>,
) -> Result<()> {
    if let Ok(mut stmt) = conn.prepare("SELECT userId, displayName, zaloName FROM friends_info") {
        let rows = stmt.query_map([], |r| {
            let uid: String = r.get(0)?;
            let dname: String = r.get(1).unwrap_or_default();
            let zname: String = r.get(2).unwrap_or_default();
            let name = if !zname.is_empty() { zname } else { dname };
            Ok((uid, name))
        })?;
        for r in rows.flatten() {
            if !r.0.is_empty() && !r.1.is_empty() {
                out.insert(r.0, (r.1, false));
            }
        }
    }
    if let Ok(mut stmt) = conn.prepare("SELECT userId, displayName, zaloName FROM friend") {
        let rows = stmt.query_map([], |r| {
            let uid: String = r.get(0)?;
            let dname: String = r.get(1).unwrap_or_default();
            let zname: String = r.get(2).unwrap_or_default();
            let name = if !zname.is_empty() { zname } else { dname };
            Ok((uid, name))
        })?;
        for r in rows.flatten() {
            if !r.0.is_empty() && !r.1.is_empty() {
                out.insert(r.0, (r.1, false));
            }
        }
    }
    if let Ok(mut stmt) = conn.prepare("SELECT userId, displayName FROM [group]") {
        let rows = stmt.query_map([], |r| {
            let uid: String = r.get(0)?;
            let dname: String = r.get(1).unwrap_or_default();
            Ok((uid, dname))
        })?;
        for r in rows.flatten() {
            if !r.0.is_empty() && !r.1.is_empty() {
                let mut gid = r.0.clone();
                out.insert(gid.clone(), (r.1.clone(), true));
                if !gid.starts_with('g') {
                    gid.insert(0, 'g');
                    out.insert(gid, (r.1, true));
                }
            }
        }
    }
    Ok(())
}

/// Import a decrypted official Zalo PC `message` table into `dst`.
fn import_message_table(
    src_conn: &Connection,
    dst: &mut MessageStore,
    thread_id: &str,
    thread_name: &str,
    is_group: bool,
    cipher_key: &str,
    report: &mut ImportReport,
) -> Result<()> {
    let mut stmt = match src_conn.prepare(
        "SELECT msgId, cliMsgId, fromUid, toUid, dName, msgType, sendDttm, message, quote
         FROM message ORDER BY CAST(sendDttm AS INTEGER) ASC",
    ) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    let mut msgs = Vec::new();
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0).unwrap_or_default(),
            row.get::<_, String>(1).unwrap_or_default(),
            row.get::<_, String>(2).unwrap_or_default(),
            row.get::<_, String>(3).unwrap_or_default(),
            row.get::<_, String>(4).unwrap_or_default(),
            row.get::<_, i64>(5).unwrap_or(1),
            row.get::<_, String>(6).unwrap_or_default(),
            row.get::<_, String>(7).unwrap_or_default(),
            row.get::<_, Option<String>>(8).unwrap_or_default(),
        ))
    })?;

    for r in rows.flatten() {
        let (msg_id, cli_msg_id, from_uid, _to_uid, dname, mtype, send_dttm, raw_msg, quote_str) =
            r;
        let ts: i64 = send_dttm.parse().unwrap_or(0);
        let is_outgoing = from_uid == "0" || from_uid.is_empty();
        let (text, mut meta) = decode_zalo_cipher_payload_with_meta(&raw_msg, cipher_key, mtype);
        if let Some(q) = quote_str {
            if !q.is_empty() {
                meta.insert("quote".into(), serde_json::Value::String(q));
            }
        }
        let msg_type_str = match mtype {
            1 => "chat.text",
            2 => "chat.photo",
            4 => "chat.sticker",
            6 => "chat.link",
            18 => "chat.video",
            19 => "chat.file",
            _ => "chat.text",
        };
        msgs.push(StoredMessage {
            msg_id: if !msg_id.is_empty() {
                msg_id
            } else {
                cli_msg_id
            },
            thread_id: thread_id.to_string(),
            sender_id: from_uid,
            sender_name: if !dname.is_empty() {
                dname
            } else {
                if is_outgoing {
                    "Tôi".into()
                } else {
                    thread_name.into()
                }
            },
            text,
            msg_type: msg_type_str.to_string(),
            ts,
            is_outgoing,
            raw_json: if meta.is_empty() {
                String::new()
            } else {
                serde_json::Value::Object(meta).to_string()
            },
        });
    }

    if msgs.is_empty() {
        return Ok(());
    }

    let last_ts = msgs.iter().map(|m| m.ts).max().unwrap_or(0);
    dst.upsert_thread(&StoredThread {
        thread_id: thread_id.to_string(),
        name: thread_name.to_string(),
        is_group,
        last_ts,
        last_read_msg_id: String::new(),
    })?;

    dst.conn.execute_batch("BEGIN")?;
    let mut added = 0;
    for m in &msgs {
        if dst.upsert_message(m)? {
            added += 1;
        }
    }
    dst.conn.execute_batch("COMMIT")?;

    report.threads_imported += 1;
    report.messages_imported += added;
    report
        .sources_read
        .push(format!("ZaloPC: {thread_name} ({thread_id})"));
    Ok(())
}

/// Recover an account's XOR key only when all key material can be verified.
///
/// # Errors
/// Returns an unsupported error: the observed prefix and suffix leave three key
/// positions unknown, so the existing evidence cannot establish a complete key.
pub fn recover_zalo_localstorage_key(_base_dir: &std::path::Path) -> Result<String> {
    anyhow::bail!(
        "Chưa hỗ trợ khôi phục đầy đủ khóa dữ liệu Zalo PC; không thể nhập dữ liệu mã hóa an toàn"
    )
}

/// Decrypt an official database with an explicitly supplied account passphrase.
/// Returns `None` if the data cannot be decrypted using the supplied key.
pub fn try_decrypt_zalo_pc_database(bytes: &[u8], passphrase: &[u8]) -> Option<Vec<u8>> {
    if passphrase.is_empty() {
        return None;
    }
    decrypt_sqlcipher_bytes(bytes, passphrase)
}

/// Decrypt SQLCipher v4 database pages with 80-byte reserve, 4096 page size, SHA-512 256k iterations.
pub fn decrypt_sqlcipher_bytes(bytes: &[u8], passphrase: &[u8]) -> Option<Vec<u8>> {
    use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit};

    type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

    if bytes.len() < 4096 {
        return None;
    }
    if bytes.starts_with(b"SQLite format 3\x00") {
        return Some(bytes.to_vec());
    }

    let salt = &bytes[0..16];
    // PBKDF2-HMAC-SHA512 256000 iterations -> 32-byte key
    let mut derived_key = [0u8; 32];
    pbkdf2_hmac_sha512(passphrase, salt, 256000, &mut derived_key);

    let page_size = 4096;
    let reserve = 80;
    let num_pages = bytes.len() / page_size;
    if num_pages == 0 {
        return None;
    }

    let mut out = Vec::with_capacity(bytes.len());
    for p in 0..num_pages {
        let page = &bytes[p * page_size..(p + 1) * page_size];
        let iv = &page[page_size - reserve..page_size - reserve + 16];
        let mut dec_buf;
        if p == 0 {
            dec_buf = page[16..page_size - reserve].to_vec();
            let decryptor = Aes256CbcDec::new((&derived_key).into(), iv.into());
            if decryptor
                .decrypt_padded_mut::<NoPadding>(&mut dec_buf)
                .is_err()
            {
                return None;
            }
            // Check SQLite magic page size (offset 0..2 in decrypted buffer)
            if dec_buf.len() < 16 || dec_buf[0] != 0x10 || dec_buf[1] != 0x00 {
                return None;
            }
            out.extend_from_slice(b"SQLite format 3\x00");
            out.extend_from_slice(&dec_buf);
            out.extend(std::iter::repeat(0u8).take(reserve));
        } else {
            dec_buf = page[0..page_size - reserve].to_vec();
            let decryptor = Aes256CbcDec::new((&derived_key).into(), iv.into());
            if decryptor
                .decrypt_padded_mut::<NoPadding>(&mut dec_buf)
                .is_err()
            {
                return None;
            }
            out.extend_from_slice(&dec_buf);
            out.extend(std::iter::repeat(0u8).take(reserve));
        }
    }
    Some(out)
}

fn pbkdf2_hmac_sha512(password: &[u8], salt: &[u8], iterations: u32, output: &mut [u8]) {
    use sha2::{Digest, Sha512};
    let mut k = [0u8; 128];
    if password.len() > 128 {
        let h = Sha512::digest(password);
        k[..64].copy_from_slice(&h);
    } else {
        k[..password.len()].copy_from_slice(password);
    }
    let mut i_pad = [0x36u8; 128];
    let mut o_pad = [0x5cu8; 128];
    for i in 0..128 {
        i_pad[i] ^= k[i];
        o_pad[i] ^= k[i];
    }
    let mut base_inner = Sha512::new();
    base_inner.update(&i_pad);
    let mut base_outer = Sha512::new();
    base_outer.update(&o_pad);

    let hmac = |data: &[u8]| -> [u8; 64] {
        let mut inner = base_inner.clone();
        inner.update(data);
        let inner_hash = inner.finalize();

        let mut outer = base_outer.clone();
        outer.update(&inner_hash);
        let mut res = [0u8; 64];
        res.copy_from_slice(&outer.finalize());
        res
    };

    let mut salt_and_i = Vec::with_capacity(salt.len() + 4);
    salt_and_i.extend_from_slice(salt);
    salt_and_i.extend_from_slice(&1u32.to_be_bytes());

    let mut u = hmac(&salt_and_i);
    let mut t = u;

    for _ in 1..iterations {
        u = hmac(&u);
        for j in 0..64 {
            t[j] ^= u[j];
        }
    }
    let len = output.len().min(64);
    output[..len].copy_from_slice(&t[..len]);
}

/// Decrypt message payload string using the 32-character XOR key.
pub fn decode_zalo_cipher_payload(raw: &str, key: &str) -> String {
    if raw.is_empty() || key.is_empty() {
        return raw.to_string();
    }
    let s = if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(st) = val.as_str() {
            st.to_string()
        } else {
            raw.to_string()
        }
    } else {
        raw.to_string()
    };

    let key_chars: Vec<char> = key.chars().collect();
    let k_len = key_chars.len();
    let xored: String = s
        .chars()
        .enumerate()
        .map(|(i, c)| {
            let k = key_chars[i % k_len] as u32;
            let code = c as u32 ^ k;
            char::from_u32(code).unwrap_or(c)
        })
        .collect();

    let (text, _meta) = extract_zalo_message_text_and_meta(&xored, 1);
    text
}

fn decode_zalo_cipher_payload_with_meta(
    raw: &str,
    key: &str,
    mtype: i64,
) -> (String, serde_json::Map<String, serde_json::Value>) {
    if raw.is_empty() || key.is_empty() {
        return (raw.to_string(), serde_json::Map::new());
    }
    let s = if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(st) = val.as_str() {
            st.to_string()
        } else {
            raw.to_string()
        }
    } else {
        raw.to_string()
    };
    let key_chars: Vec<char> = key.chars().collect();
    let k_len = key_chars.len();
    let xored: String = s
        .chars()
        .enumerate()
        .map(|(i, c)| {
            let k = key_chars[i % k_len] as u32;
            let code = c as u32 ^ k;
            char::from_u32(code).unwrap_or(c)
        })
        .collect();
    extract_zalo_message_text_and_meta(&xored, mtype)
}

/// Convert Zalo `.jxl` URLs (`/jxl/` ... `.jxl?jxlstatus=1`) to standard JPEG URLs (`/jpg/` ... `.jpg`)
/// matching `QWlH.toJPGUrl` in `compact-app-pc.js:8396800`.
pub fn jxl_to_jpg_url(url: &str) -> String {
    if !url.contains("/jxl/") || !url.contains(".jxl") {
        return url.to_string();
    }
    let without_flag = url
        .replace("?jxlstatus=1&", "?")
        .replace("&jxlstatus=1", "")
        .replace("?jxlstatus=1", "");
    if without_flag.contains("e2esession=") {
        return without_flag;
    }
    without_flag
        .replace("/jxl/", "/jpg/")
        .replace(".jxl", ".jpg")
}

fn extract_zalo_message_text_and_meta(
    xored: &str,
    mtype: i64,
) -> (String, serde_json::Map<String, serde_json::Value>) {
    let mut meta = serde_json::Map::new();
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(xored) {
        if let Some(inner) = val.as_str() {
            return (inner.to_string(), meta);
        }
        if let Some(arr) = val.as_array() {
            for item in arr {
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_default();
                let desc = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_default();
                let href = item
                    .get("href")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_default();
                let thumb = item
                    .get("thumb")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_default();
                if !title.is_empty() || !desc.is_empty() {
                    if !href.is_empty() {
                        meta.insert("link_url".into(), serde_json::json!(href));
                    }
                    if !title.is_empty() {
                        meta.insert("link_title".into(), serde_json::json!(title));
                    }
                    if !desc.is_empty() {
                        meta.insert("link_desc".into(), serde_json::json!(desc));
                    }
                    if !thumb.is_empty() {
                        meta.insert("link_thumb".into(), serde_json::json!(thumb));
                    }
                    let txt = if !title.is_empty() { title } else { desc };
                    return (txt.to_string(), meta);
                }
            }
        }
        if mtype == 4 || val.get("catId").is_some() {
            if let Some(sid) = val
                .get("id")
                .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
            {
                let cid = val
                    .get("catId")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
                    .unwrap_or(0);
                let st = val
                    .get("type")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
                    .unwrap_or(7);
                meta.insert("stickerId".into(), serde_json::json!(sid));
                meta.insert("cateId".into(), serde_json::json!(cid));
                meta.insert("type".into(), serde_json::json!(st));
                return ("[Nhãn dán]".to_string(), meta);
            }
        }
        // Photo (msgType == 2)
        if mtype == 2 || val.get("oriUrl").is_some() || val.get("thumbUrl").is_some() {
            let ori = jxl_to_jpg_url(
                val.get("oriUrl")
                    .or_else(|| val.get("normalUrl"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
            );
            let thumb = jxl_to_jpg_url(
                val.get("thumbUrl")
                    .or_else(|| val.get("oriUrl"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
            );
            let title = val
                .get("title")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("[Hình ảnh]");
            if !ori.is_empty() {
                meta.insert("link_url".into(), serde_json::json!(ori));
            }
            if !thumb.is_empty() {
                meta.insert("link_thumb".into(), serde_json::json!(thumb));
            }
            meta.insert("link_title".into(), serde_json::json!(title));
            meta.insert("link_src".into(), serde_json::json!("Hình ảnh Zalo"));
            return (title.to_string(), meta);
        }
        // Call bubble or link/file (msgType == 6, 18, 19)
        if let Some(action) = val.get("action").and_then(|v| v.as_str()) {
            if action == "recommened.calltime" {
                return ("📞 Cuộc gọi thoại/video".to_string(), meta);
            } else if action == "recommened.misscall" {
                return ("📞 Cuộc gọi nhỡ".to_string(), meta);
            }
        }
        let href = val.get("href").and_then(|v| v.as_str()).unwrap_or_default();
        let title = val
            .get("title")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty() && *s != "sendBubbleMessage")
            .unwrap_or_default();
        let desc = val
            .get("description")
            .or_else(|| val.get("desc"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty() && *s != "Cuộc gọi")
            .unwrap_or_default();
        let thumb = val
            .get("thumb")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if !href.is_empty() {
            meta.insert("link_url".into(), serde_json::json!(href));
        }
        if !title.is_empty() {
            meta.insert("link_title".into(), serde_json::json!(title));
        }
        if !desc.is_empty() {
            meta.insert("link_desc".into(), serde_json::json!(desc));
        }
        if !thumb.is_empty() {
            meta.insert("link_thumb".into(), serde_json::json!(thumb));
        }
        if !title.is_empty() {
            return (title.to_string(), meta);
        }
        if !desc.is_empty() {
            return (desc.to_string(), meta);
        }
        if !href.is_empty() {
            return (href.to_string(), meta);
        }
        if val.get("params").is_some() && val.get("childnumber").is_some() {
            return ("📢 Thông báo hệ thống Zalo".to_string(), meta);
        }
    }
    (xored.to_string(), meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_recovery_does_not_invent_a_key_when_local_storage_is_absent() {
        let absent = std::env::temp_dir().join(format!("ctdozlite-no-key-{}", std::process::id()));
        assert!(recover_zalo_localstorage_key(&absent).is_err());
    }

    #[test]
    fn unsupported_encrypted_directory_is_rejected_before_plaintext_rows_are_written() {
        let root = std::env::temp_dir().join(format!(
            "ctdozlite-mixed-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source = root.join("source");
        std::fs::create_dir_all(source.join("Database/_production/user-1/Core/Message")).unwrap();
        std::fs::write(
            source.join("export.json"),
            serde_json::to_vec(&vec![Thread {
                id: "thread-1".into(),
                messages: vec![ChatMessage {
                    id: "message-1".into(),
                    text: "Test".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }])
            .unwrap(),
        )
        .unwrap();
        let destination = root.join("destination.db");
        let result = import_history_from_path(&source, destination.to_str().unwrap());
        let store = MessageStore::open(destination.to_str().unwrap()).unwrap();
        let count = store.messages_for("thread-1", 10).unwrap().len();
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
        assert!(result.is_err());
        assert_eq!(
            count, 0,
            "an unsupported source must not leave a partially committed import"
        );
    }

    #[test]
    fn unowned_legacy_history_is_not_claimed_by_a_new_account() {
        let root = std::env::temp_dir().join(format!(
            "ctdozlite-unowned-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let legacy = MessageStore::open(root.join("zalo_history.db").to_str().unwrap()).unwrap();
        legacy.conn.execute("INSERT INTO messages(msg_id,thread_id,text) VALUES ('message-a','thread-a','Private')", []).unwrap();
        drop(legacy);
        let account = MessageStore::open_account_in(&root, "new-account-b").unwrap();
        let count: usize = account
            .conn
            .query_row("SELECT count(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        drop(account);
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn loose_encrypted_file_is_rejected_before_other_exports_are_committed() {
        let root = std::env::temp_dir().join(format!(
            "ctdozlite-loose-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source = root.join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("export.json"),
            serde_json::to_vec(&vec![Thread {
                id: "thread-1".into(),
                messages: vec![ChatMessage {
                    id: "message-1".into(),
                    text: "Test".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }])
            .unwrap(),
        )
        .unwrap();
        std::fs::write(source.join("encrypted.db"), vec![0xff; 4096]).unwrap();
        let destination = root.join("destination.db");
        let result = import_history_from_path(&source, destination.to_str().unwrap());
        let store = MessageStore::open(destination.to_str().unwrap()).unwrap();
        let count = store.messages_for("thread-1", 10).unwrap().len();
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
        assert!(result.is_err());
        assert_eq!(count, 0);
    }

    #[test]
    fn concurrent_first_account_opens_import_legacy_history_once() {
        let root = std::env::temp_dir().join(format!(
            "ctdozlite-concurrent-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let legacy = MessageStore::open(root.join("zalo_history.db").to_str().unwrap()).unwrap();
        legacy
            .conn
            .execute(
                "INSERT INTO account_store_metadata(key,value) VALUES ('legacy_owner','account-1')",
                [],
            )
            .unwrap();
        legacy.conn.execute_batch("WITH RECURSIVE ids(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM ids WHERE n<2000)
            INSERT INTO messages(msg_id,thread_id,sender_id,sender_name,text,msg_type,ts,is_outgoing,raw_json)
            SELECT 'message-'||n,'thread-1','user-1','Test','Test','chat.text',n,0,'' FROM ids;").unwrap();
        drop(legacy);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let root = root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    MessageStore::open_account_in(&root, "account-1").map(|store| {
                        store
                            .conn
                            .query_row("SELECT count(*) FROM messages", [], |row| {
                                row.get::<_, usize>(0)
                            })
                            .unwrap()
                    })
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        std::fs::remove_dir_all(root).unwrap();
        for result in results {
            assert_eq!(result.unwrap(), 2000);
        }
    }

    #[test]
    fn account_open_merges_legacy_wal_once_for_its_owner() {
        let root = std::env::temp_dir().join(format!(
            "ctdozlite-owner-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("accounts")).unwrap();
        let mut legacy =
            MessageStore::open(root.join("zalo_history.db").to_str().unwrap()).unwrap();
        legacy
            .conn
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        legacy
            .conn
            .execute(
                "INSERT INTO account_store_metadata(key,value) VALUES ('legacy_owner','user-a')",
                [],
            )
            .unwrap();
        let message = |id: &str| StoredMessage {
            msg_id: id.into(),
            thread_id: "thread-1".into(),
            sender_id: "user-1".into(),
            sender_name: "Example".into(),
            text: id.into(),
            msg_type: "chat.text".into(),
            ts: 1,
            is_outgoing: false,
            raw_json: String::new(),
        };
        legacy.upsert_message(&message("msg-legacy")).unwrap();
        legacy
            .upsert_thread(&StoredThread {
                thread_id: "thread-1".into(),
                name: "Example".into(),
                is_group: false,
                last_ts: 1,
                last_read_msg_id: "msg-legacy".into(),
            })
            .unwrap();
        crate::scheduler::init_scheduler_schema(&legacy.conn).unwrap();
        legacy.conn.execute("INSERT INTO scheduled_messages(thread_id, text, deliver_at, created_at) VALUES ('thread-1', 'legacy schedule', 2, 1)", []).unwrap();
        {
            let mut existing =
                MessageStore::open(root.join("accounts/user-a_history.db").to_str().unwrap())
                    .unwrap();
            existing.upsert_message(&message("msg-canonical")).unwrap();
            crate::scheduler::init_scheduler_schema(&existing.conn).unwrap();
            existing.conn.execute("INSERT INTO scheduled_messages(thread_id, text, deliver_at, created_at) VALUES ('thread-1', 'canonical schedule', 2, 1)", []).unwrap();
        }
        {
            let account = MessageStore::open_account_in(&root, "user-a").unwrap();
            assert_eq!(account.messages_for("thread-1", 20).unwrap().len(), 2);
            assert_eq!(
                account.all_threads().unwrap()[0].last_read_msg_id,
                "msg-legacy"
            );
            let count: i64 = account
                .conn
                .query_row("SELECT COUNT(*) FROM scheduled_messages", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 2);
        }
        {
            let account = MessageStore::open_account_in(&root, "user-a").unwrap();
            assert_eq!(account.messages_for("thread-1", 20).unwrap().len(), 2); // (INV-4)
            let count: i64 = account
                .conn
                .query_row("SELECT COUNT(*) FROM scheduled_messages", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 2);
        }
        let other = MessageStore::open_account_in(&root, "user-b").unwrap();
        assert!(other.messages_for("thread-1", 20).unwrap().is_empty());
        drop(other);
        drop(legacy);
        std::fs::write(
            root.join("zalo_history.db"),
            b"unusable retired legacy file",
        )
        .unwrap();
        let migrated = MessageStore::open_account_in(&root, "user-a").unwrap();
        assert_eq!(migrated.messages_for("thread-1", 20).unwrap().len(), 2);
        drop(migrated);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_time_when_hydrating_seconds_and_milliseconds() {
        let mut store = MessageStore::open(":memory:").unwrap();
        store
            .upsert_thread(&StoredThread {
                thread_id: "thread-1".into(),
                name: "Example".into(),
                is_group: false,
                last_ts: 1793467800000,
                last_read_msg_id: String::new(),
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
        assert_eq!(
            newest.iter().map(|m| m.ts).collect::<Vec<_>>(),
            vec![200, 300]
        );
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
            last_read_msg_id: String::new(),
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
    fn thread_read_point_roundtrips_and_survives_reupsert() {
        let mut store = MessageStore::open(":memory:").unwrap();
        store.set_thread_read("t1", "msg-100").unwrap();
        let got = store.all_threads().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].last_read_msg_id, "msg-100");

        // Re-upserting without read-point preserves existing read-point (#119).
        store
            .upsert_thread(&StoredThread {
                thread_id: "t1".into(),
                name: "A".into(),
                is_group: false,
                last_ts: 1,
                last_read_msg_id: String::new(),
            })
            .unwrap();
        assert_eq!(store.all_threads().unwrap()[0].last_read_msg_id, "msg-100");

        // Explicit new read-point updates.
        store.set_thread_read("t1", "msg-200").unwrap();
        assert_eq!(store.all_threads().unwrap()[0].last_read_msg_id, "msg-200");
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
                last_read_msg_id: String::new(),
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
                    last_read_msg_id: String::new(),
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

    #[test]
    fn jxl_to_jpg_url_and_decode_zalo_cipher_payload_work() {
        let jxl = "https://example.invalid/no/jxl/fixture.jxl?jxlstatus=1";
        assert_eq!(
            jxl_to_jpg_url(jxl),
            "https://example.invalid/no/jpg/fixture.jpg"
        );
        let key = "0123456789abcdef0123456789abcdef";
        let plain = "\"Synthetic fixture\"";
        let key_chars: Vec<char> = key.chars().collect();
        let enc: String = plain
            .chars()
            .enumerate()
            .map(|(i, c)| {
                char::from_u32((c as u32) ^ (key_chars[i % key_chars.len()] as u32)).unwrap()
            })
            .collect();
        assert_eq!(decode_zalo_cipher_payload(&enc, key), "Synthetic fixture");
    }
}
