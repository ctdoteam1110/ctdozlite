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
/// 1 tin trong hộp "Đã lưu" toàn cục (WaveA2). `due_at` epoch giây, 0 = không hẹn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SavedItem {
    pub msg_id: String,
    pub thread_id: String,
    pub note: String,
    pub due_at: i64,
    pub created_at: i64,
    pub text: String,
    pub sender_name: String,
    pub ts: i64,
    pub thread_name: String,
    pub is_group: bool,
}

/// 1 bản cũ của tin đã sửa (WaveA3). `edited_at`: epoch ms lúc sửa.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MessageEditRecord {
    pub msg_id: String,
    pub thread_id: String,
    pub old_text: String,
    pub edited_at: i64,
}
/// Nháp composer theo hội thoại (WaveA1). `reply_ref`: id tin đang trả lời, "" = không.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DraftEntry {
    pub thread_id: String,
    pub text: String,
    pub reply_ref: String,
    pub updated_at: i64,
}

/// 1 lần nhắc về tin nhắn (WaveA1). `due_at`/`created_at`: epoch giây.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReminderItem {
    pub id: i64,
    pub thread_id: String,
    pub msg_id: String,
    pub due_at: i64,
    pub text_preview: String,
    pub created_at: i64,
}
/// Loại `has:` trong search nâng cao (WaveA2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchHas { Link, File, Image, Voice }

/// Kết quả parse query search: operators + từ tự do (WaveA2, thuần, có test).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub from: Option<String>,
    pub in_thread: Option<String>,
    pub has: Vec<SearchHas>,
    pub before_ms: Option<i64>,
    pub after_ms: Option<i64>,
    pub is_starred: bool,
    pub is_pinned: bool,
    pub phrases: Vec<String>,
    pub hashtags: Vec<String>,
    pub words: Vec<String>,
}

/// Ngày `YYYY-MM-DD` → epoch ms biên ngày (local). `end` = cuối ngày, else đầu ngày.
fn search_day_bound_ms(day: &str, end: bool) -> Option<i64> {
    use chrono::TimeZone;
    let mut it = day.split('-');
    let (y, mo, d): (i32, u32, u32) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    if it.next().is_some() { return None; }
    let date = chrono::NaiveDate::from_ymd_opt(y, mo, d)?;
    let dt = if end { date.and_hms_opt(23, 59, 59)? } else { date.and_hms_opt(0, 0, 0)? };
    let local = chrono::Local.from_local_datetime(&dt).single()?;
    Some(local.timestamp_millis())
}

/// Parser operator search.
pub fn parse_search_query(raw: &str) -> SearchQuery {
    let mut q = SearchQuery::default();
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    for ch in raw.chars() {
        if ch == '"' { in_q = !in_q; cur.push(ch); }
        else if ch.is_whitespace() && !in_q {
            if !cur.is_empty() { tokens.push(std::mem::take(&mut cur)); }
        } else { cur.push(ch); }
    }
    if !cur.is_empty() { tokens.push(cur); }
    for tok in tokens {
        if tok.len() >= 2 && tok.starts_with('"') && tok.ends_with('"') {
            let p = tok[1..tok.len()-1].trim();
            if !p.is_empty() { q.phrases.push(p.to_string()); }
            continue;
        }
        if let Some(v) = tok.strip_prefix("from:") {
            let v = v.trim(); if !v.is_empty() { q.from = Some(v.to_string()); }
            continue;
        }
        if let Some(v) = tok.strip_prefix("in:") {
            let v = v.trim(); if !v.is_empty() { q.in_thread = Some(v.to_string()); }
            continue;
        }
        if let Some(v) = tok.strip_prefix("has:") {
            match v.trim().to_lowercase().as_str() {
                "link" => q.has.push(SearchHas::Link),
                "file" => q.has.push(SearchHas::File),
                "image" | "img" | "photo" => q.has.push(SearchHas::Image),
                "voice" | "audio" => q.has.push(SearchHas::Voice),
                _ => q.words.push(tok.clone()),
            }
            continue;
        }
        if let Some(v) = tok.strip_prefix("before:") {
            if let Some(ms) = search_day_bound_ms(v.trim(), true) { q.before_ms = Some(ms); }
            else { q.words.push(tok.clone()); }
            continue;
        }
        if let Some(v) = tok.strip_prefix("after:") {
            if let Some(ms) = search_day_bound_ms(v.trim(), false) { q.after_ms = Some(ms); }
            else { q.words.push(tok.clone()); }
            continue;
        }
        if let Some(v) = tok.strip_prefix("is:") {
            match v.trim().to_lowercase().as_str() {
                "starred" | "star" | "saved" => q.is_starred = true,
                "pinned" | "pin" | "ghim" => q.is_pinned = true,
                _ => q.words.push(tok.clone()),
            }
            continue;
        }
        if let Some(tag) = tok.strip_prefix('#') {
            let t = tag.trim_matches(|c: char| ".,;:!?()[]".contains(c));
            if !t.is_empty() && t.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
                q.hashtags.push(t.to_string());
            } else { q.words.push(tok.clone()); }
            continue;
        }
        q.words.push(tok);
    }
    q
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StoredThread {
    pub thread_id: String,
    pub name: String,
    pub is_group: bool,
    pub last_ts: i64,
    #[serde(default)]
    pub vanish_duration_sec: i64,
    #[serde(default)]
    pub vanish_started_at_sec: i64,
    #[serde(default)]
    pub last_read_msg_id: String,
}
/// Non-empty raw_json marker for vanish-expiry tombstones. The hydrate path
/// treats `vanished: true` as authoritative: it never restores link/sticker
/// metadata or original text from this row (gui.rs:1908).
pub const VANISH_TOMBSTONE_JSON: &str = r#"{"vanished":true}"#;


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
        let _ = std::fs::create_dir_all("accounts");
        format!("accounts/{uin}_history.db")
    }
}

impl MessageStore {
    /// Messages per thread loaded at startup / on refresh. The list only needs the newest one
    /// for ordering and preview; opening a thread reads deeper (`LoadGroupHistory`).
    pub const PRELOAD_MSGS: usize = 20;

    /// Mở SQLite mà KHÔNG chạy migration/khởi tạo schema (đọc đường nhanh).
    /// Dùng cho lookup nền (vd. watch keyword khi match tin mới): DB cũ thiếu
    /// bảng → các hàm đọc trả rỗng thay vì chờ `ALTER TABLE`.
    pub fn open_no_migrate(path: &str) -> Result<Self> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(path)?
        };
        Ok(Self { conn })
    }

     /// Open SQLite database at `path` and initialize table schemas and indices if absent.
     pub fn open(path: &str) -> Result<Self> {
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
                last_ts INTEGER,
                vanish_duration_sec INTEGER NOT NULL DEFAULT 0,
                vanish_started_at_sec INTEGER NOT NULL DEFAULT 0,
                last_read_msg_id TEXT NOT NULL DEFAULT '',
                contact_name INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS thread_vanish (
                thread_id TEXT PRIMARY KEY,
                duration_sec INTEGER NOT NULL,
                started_at_sec INTEGER NOT NULL
            );",
        )?;
        // Migration: vanish columns / side table on DBs created before vanish slice.
        Self::ensure_vanish_schema(&conn)?;
        // Group workspace tables (notes + keyword watch): safe re-run on old DBs.
        Self::ensure_group_workspace_schema(&conn)?;
        // Saved items (WaveA2) → draft/reminder (WaveA1) → edit history (WaveA3).
        Self::ensure_saved_schema(&conn)?;
        // Nháp theo hội thoại + nhắc tin (WaveA1): bảng drafts, message_reminders.
        Self::ensure_draft_reminder_schema(&conn)?;
        // Lịch sử bản cũ của tin đã sửa (WaveA3): bảng message_edits.
        Self::ensure_edit_history_schema(&conn)?;
        // Per-thread mini script workspace: bảng thread_scripts + thread_data + logs.
        Self::ensure_script_schema(&conn)?;
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

    /// Thêm cột vanish / bảng `thread_vanish` cho DB cũ (pre-vanish slice).
    /// Chạy an toàn nhiều lần: dò `PRAGMA table_info` rồi mới `ALTER TABLE`.
    fn ensure_vanish_schema(conn: &Connection) -> Result<()> {
    let mut cols: Vec<String> = Vec::new();
    if let Ok(mut stmt) = conn.prepare("PRAGMA table_info(threads)") {
        if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(1)) {
            for c in rows.flatten() {
                cols.push(c);
            }
        }
    }
    if !cols.iter().any(|c| c == "vanish_duration_sec") {
        let _ = conn.execute(
            "ALTER TABLE threads ADD COLUMN vanish_duration_sec INTEGER NOT NULL DEFAULT 0",
            [],
        );
    }
    if !cols.iter().any(|c| c == "vanish_started_at_sec") {
        let _ = conn.execute(
            "ALTER TABLE threads ADD COLUMN vanish_started_at_sec INTEGER NOT NULL DEFAULT 0",
            [],
        );
    }
    if !cols.iter().any(|c| c == "last_read_msg_id") {
        let _ = conn.execute(
            "ALTER TABLE threads ADD COLUMN last_read_msg_id TEXT NOT NULL DEFAULT ''",
            [],
        );
    }
    if !cols.iter().any(|c| c == "contact_name") {
        let _ = conn.execute(
            "ALTER TABLE threads ADD COLUMN contact_name INTEGER NOT NULL DEFAULT 0",
            [],
        );
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS thread_vanish (
            thread_id TEXT PRIMARY KEY,
            duration_sec INTEGER NOT NULL,
            started_at_sec INTEGER NOT NULL
        );",
    )?;
    Ok(())
}

    /// Tạo bảng workspace nhóm nếu DB cũ chưa có (group notes + keyword watch).
    /// Chạy an toàn nhiều lần: chỉ `CREATE TABLE IF NOT EXISTS`.
    fn ensure_group_workspace_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS group_notes (
                thread_id TEXT PRIMARY KEY,
                note TEXT NOT NULL DEFAULT '',
                updated_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS group_watch (
                thread_id TEXT NOT NULL,
                keyword TEXT NOT NULL,
                PRIMARY KEY (thread_id, keyword)
            );
            CREATE INDEX IF NOT EXISTS idx_group_watch_thread ON group_watch(thread_id);",
        )?;
        Ok(())
    }

    /// Tạo bảng tin đã lưu toàn cục (#saved). `msg_id` PK; `due_at` epoch giây, 0 = không hẹn.
    /// Chạy an toàn nhiều lần. Bảng `saved_items` sở hữu bởi WaveA2.
    fn ensure_saved_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS saved_items (
                msg_id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL DEFAULT '',
                note TEXT NOT NULL DEFAULT '',
                due_at INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_saved_thread ON saved_items(thread_id);
            CREATE INDEX IF NOT EXISTS idx_saved_due ON saved_items(due_at);",
         )?;
         Ok(())
     }
    /// Tạo bảng nháp + nhắc tin (WaveA1). `drafts`: 1 row/thread; `message_reminders`:
    /// 1 row/lần nhắc, `due_at` epoch giây. Chạy an toàn nhiều lần.
    fn ensure_draft_reminder_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS drafts (
                thread_id TEXT PRIMARY KEY,
                text TEXT NOT NULL DEFAULT '',
                reply_ref TEXT NOT NULL DEFAULT '',
                updated_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS message_reminders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                thread_id TEXT NOT NULL DEFAULT '',
                msg_id TEXT NOT NULL DEFAULT '',
                due_at INTEGER NOT NULL DEFAULT 0,
                text_preview TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_reminders_due ON message_reminders(due_at);",
        )?;
        Ok(())
    }


    /// Tạo bảng lịch sử bản cũ của tin đã sửa (WaveA3). Mỗi lần sửa append 1 row
    /// `(msg_id, thread_id, old_text, edited_at)`. Chạy an toàn nhiều lần.
    fn ensure_edit_history_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS message_edits (
                msg_id TEXT NOT NULL,
                thread_id TEXT NOT NULL DEFAULT '',
                old_text TEXT NOT NULL DEFAULT '',
                edited_at INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (msg_id, edited_at)
            );
            CREATE INDEX IF NOT EXISTS idx_message_edits_msg ON message_edits(msg_id);",
        )?;
        Ok(())
    }
    /// Tạo bảng script per-thread + KV riêng + log chạy. Chạy an toàn nhiều lần.
    fn ensure_script_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS thread_scripts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                thread_id TEXT NOT NULL DEFAULT '',
                name TEXT NOT NULL DEFAULT '',
                code TEXT NOT NULL DEFAULT '',
                trigger TEXT NOT NULL DEFAULT 'manual',
                trigger_arg TEXT NOT NULL DEFAULT '',
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_thread_scripts_thread ON thread_scripts(thread_id);
            CREATE TABLE IF NOT EXISTS thread_data (
                thread_id TEXT NOT NULL DEFAULT '',
                key TEXT NOT NULL DEFAULT '',
                value_json TEXT NOT NULL DEFAULT '',
                updated_at INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (thread_id, key)
            );
            CREATE INDEX IF NOT EXISTS idx_thread_data_thread ON thread_data(thread_id);
            CREATE TABLE IF NOT EXISTS thread_script_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                thread_id TEXT NOT NULL DEFAULT '',
                name TEXT NOT NULL DEFAULT '',
                level TEXT NOT NULL DEFAULT 'info',
                msg TEXT NOT NULL DEFAULT '',
                ts INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_thread_script_logs_thread ON thread_script_logs(thread_id, id);",
        )?;
        Ok(())
    }


    /// Lưu 1 bản cũ trước khi thay text mới. `edited_at`: epoch ms.
    pub fn save_message_edit(
        &mut self,
        msg_id: &str,
        thread_id: &str,
        old_text: &str,
        edited_at: i64,
    ) -> Result<()> {
        if msg_id.is_empty() {
            return Ok(());
        }
        let _ = Self::ensure_edit_history_schema(&self.conn);
        self.conn.execute(
            "INSERT OR IGNORE INTO message_edits (msg_id, thread_id, old_text, edited_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![msg_id, thread_id, old_text, edited_at],
        )?;
        Ok(())
    }

    /// Liệt kê các bản cũ của 1 tin, cũ → mới. DB thiếu bảng → vec rỗng.
    pub fn list_message_edits(&self, msg_id: &str) -> Vec<MessageEditRecord> {
        if msg_id.is_empty() {
            return Vec::new();
        }
        let mut stmt = match self.conn.prepare(
            "SELECT msg_id, thread_id, old_text, edited_at FROM message_edits
             WHERE msg_id = ?1 ORDER BY edited_at ASC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(params![msg_id], |r| {
            Ok(MessageEditRecord {
                msg_id: r.get(0).unwrap_or_default(),
                thread_id: r.get(1).unwrap_or_default(),
                old_text: r.get(2).unwrap_or_default(),
                edited_at: r.get(3).unwrap_or(0),
            })
        })
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }
    /// Lưu 1 script per-thread. `trigger`: manual|on_message|on_interval_N|on_keyword:kw.
    /// Trả id row mới.
    pub fn save_thread_script(
        &mut self,
        thread_id: &str,
        name: &str,
        code: &str,
        trigger: &str,
        trigger_arg: &str,
        enabled: bool,
    ) -> Result<i64> {
        if thread_id.is_empty() || name.trim().is_empty() { return Ok(0); }
        let _ = Self::ensure_script_schema(&self.conn);
        let now = crate::login::unix_now();
        self.conn.execute(
            "INSERT INTO thread_scripts (thread_id, name, code, trigger, trigger_arg, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![thread_id, name.trim(), code, trigger, trigger_arg, i64::from(enabled), now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Cập nhật 1 script đã có (thuộc đúng thread). Không match → Ok(false).
    pub fn update_thread_script(
        &mut self,
        id: i64,
        thread_id: &str,
        name: &str,
        code: &str,
        trigger: &str,
        trigger_arg: &str,
        enabled: bool,
    ) -> Result<bool> {
        let _ = Self::ensure_script_schema(&self.conn);
        let now = crate::login::unix_now();
        let rows = self.conn.execute(
            "UPDATE thread_scripts SET name = ?2, code = ?3, trigger = ?4, trigger_arg = ?5,
                enabled = ?6, updated_at = ?7 WHERE id = ?1 AND thread_id = ?8",
            params![id, name.trim(), code, trigger, trigger_arg, i64::from(enabled), now, thread_id],
        )?;
        Ok(rows > 0)
    }

    /// Bật/tắt 1 script.
    pub fn set_thread_script_enabled(&mut self, id: i64, enabled: bool) -> Result<()> {
        let _ = Self::ensure_script_schema(&self.conn);
        let now = crate::login::unix_now();
        self.conn.execute(
            "UPDATE thread_scripts SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, i64::from(enabled), now],
        )?;
        Ok(())
    }

    /// Xoá 1 script (đúng thread).
    pub fn delete_thread_script(&mut self, id: i64, thread_id: &str) -> Result<()> {
        let _ = Self::ensure_script_schema(&self.conn);
        self.conn.execute(
            "DELETE FROM thread_scripts WHERE id = ?1 AND thread_id = ?2",
            params![id, thread_id],
        )?;
        Ok(())
    }

    /// Liệt kê script của 1 thread (mới → cũ theo id DESC).
    pub fn list_thread_scripts(&self, thread_id: &str) -> Vec<crate::thread_script::ThreadScript> {
        if thread_id.is_empty() { return Vec::new(); }
        let mut stmt = match self.conn.prepare(
            "SELECT id, thread_id, name, code, trigger, trigger_arg, enabled, created_at, updated_at
             FROM thread_scripts WHERE thread_id = ?1 ORDER BY id DESC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(params![thread_id], |r| {
            let enabled: i64 = r.get(6).unwrap_or(1);
            Ok(crate::thread_script::ThreadScript {
                id: r.get(0).unwrap_or(0),
                thread_id: r.get(1).unwrap_or_default(),
                name: r.get(2).unwrap_or_default(),
                code: r.get(3).unwrap_or_default(),
                trigger: r.get(4).unwrap_or_default(),
                trigger_arg: r.get(5).unwrap_or_default(),
                enabled: enabled != 0,
                created_at: r.get(7).unwrap_or(0),
                updated_at: r.get(8).unwrap_or(0),
            })
        })
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Liệt kê script đang bật của các thread cho trước (worker tick: chỉ thread có script mới chạy).
    pub fn list_enabled_scripts_for(&self, thread_ids: &[String]) -> Vec<crate::thread_script::ThreadScript> {
        if thread_ids.is_empty() { return Vec::new(); }
        let placeholders = thread_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, thread_id, name, code, trigger, trigger_arg, enabled, created_at, updated_at
             FROM thread_scripts WHERE enabled != 0 AND thread_id IN ({placeholders}) ORDER BY id ASC"
        );
        let mut stmt = match self.conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let params_vec: Vec<&dyn rusqlite::ToSql> =
            thread_ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        stmt.query_map(params_vec.as_slice(), |r| {
            let enabled: i64 = r.get(6).unwrap_or(1);
            Ok(crate::thread_script::ThreadScript {
                id: r.get(0).unwrap_or(0),
                thread_id: r.get(1).unwrap_or_default(),
                name: r.get(2).unwrap_or_default(),
                code: r.get(3).unwrap_or_default(),
                trigger: r.get(4).unwrap_or_default(),
                trigger_arg: r.get(5).unwrap_or_default(),
                enabled: enabled != 0,
                created_at: r.get(7).unwrap_or(0),
                updated_at: r.get(8).unwrap_or(0),
            })
        })
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }
    /// Liệt kê mọi script đang bật trong DB (worker tick interval: DB per-acc nên không lẫn acc).
    pub fn all_enabled_scripts(&self) -> Vec<crate::thread_script::ThreadScript> {
        let mut stmt = match self.conn.prepare(
            "SELECT id, thread_id, name, code, trigger, trigger_arg, enabled, created_at, updated_at
             FROM thread_scripts WHERE enabled != 0 ORDER BY thread_id ASC, id ASC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |r| {
            let enabled: i64 = r.get(6).unwrap_or(1);
            Ok(crate::thread_script::ThreadScript {
                id: r.get(0).unwrap_or(0),
                thread_id: r.get(1).unwrap_or_default(),
                name: r.get(2).unwrap_or_default(),
                code: r.get(3).unwrap_or_default(),
                trigger: r.get(4).unwrap_or_default(),
                trigger_arg: r.get(5).unwrap_or_default(),
                enabled: enabled != 0,
                created_at: r.get(7).unwrap_or(0),
                updated_at: r.get(8).unwrap_or(0),
            })
        })
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Thread có script enabled nào khớp event tin nhắn không (gate rẻ cho on_message).
    pub fn thread_has_message_scripts(&self, thread_id: &str) -> bool {
        if thread_id.is_empty() { return false; }
        let mut stmt = match self.conn.prepare(
            "SELECT trigger, trigger_arg FROM thread_scripts
             WHERE thread_id = ?1 AND enabled != 0",
        ) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let rows: Vec<(String, String)> = stmt
            .query_map(params![thread_id], |r| {
                Ok((r.get(0).unwrap_or_default(), r.get(1).unwrap_or_default()))
            })
            .ok()
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default();
        rows.iter().any(|(t, a)| {
            matches!(
                crate::thread_script::parse_trigger(t, a),
                crate::thread_script::TriggerKind::OnMessage
                    | crate::thread_script::TriggerKind::OnKeyword(_)
            )
        })
    }


    /// Đọc 1 KV riêng của thread. Chưa có → `None`.
    pub fn thread_kv_get(&self, thread_id: &str, key: &str) -> Option<String> {
        if thread_id.is_empty() || key.is_empty() { return None; }
        self.conn
            .query_row(
                "SELECT value_json FROM thread_data WHERE thread_id = ?1 AND key = ?2",
                params![thread_id, key],
                |r| r.get::<_, String>(0),
            )
            .ok()
    }

    /// Ghi 1 KV riêng của thread (upsert).
    pub fn thread_kv_set(&mut self, thread_id: &str, key: &str, value_json: &str) -> Result<()> {
        if thread_id.is_empty() || key.is_empty() { return Ok(()); }
        let _ = Self::ensure_script_schema(&self.conn);
        let now = crate::login::unix_now();
        self.conn.execute(
            "INSERT INTO thread_data (thread_id, key, value_json, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(thread_id, key) DO UPDATE SET value_json = excluded.value_json,
                 updated_at = excluded.updated_at",
            params![thread_id, key, value_json, now],
        )?;
        Ok(())
    }

    /// Liệt kê key KV của 1 thread (A–Z).
    pub fn thread_kv_list(&self, thread_id: &str) -> Vec<String> {
        if thread_id.is_empty() { return Vec::new(); }
        let mut stmt = match self.conn.prepare(
            "SELECT key FROM thread_data WHERE thread_id = ?1 ORDER BY key ASC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(params![thread_id], |r| r.get::<_, String>(0))
            .ok()
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// Ghi 1 dòng log chạy script; giữ tối đa 200 dòng/thread (xoá cũ nhất).
    pub fn push_script_log(&mut self, thread_id: &str, name: &str, level: &str, msg: &str) -> Result<()> {
        if thread_id.is_empty() { return Ok(()); }
        let _ = Self::ensure_script_schema(&self.conn);
        let now = crate::login::unix_now();
        let short: String = msg.chars().take(2000).collect();
        self.conn.execute(
            "INSERT INTO thread_script_logs (thread_id, name, level, msg, ts)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![thread_id, name, level, short, now],
        )?;
        self.conn.execute(
            "DELETE FROM thread_script_logs WHERE id NOT IN (
                 SELECT id FROM thread_script_logs WHERE thread_id = ?1 ORDER BY id DESC LIMIT 200
             ) AND thread_id = ?1",
            params![thread_id],
        )?;
        Ok(())
    }

    /// Đọc log chạy script của 1 thread (mới nhất trước, tối đa `limit`).
    pub fn list_script_logs(&self, thread_id: &str, limit: usize) -> Vec<crate::thread_script::ScriptLog> {
        if thread_id.is_empty() { return Vec::new(); }
        let lim = limit.clamp(1, 200) as i64;
        let mut stmt = match self.conn.prepare(
            "SELECT thread_id, name, level, msg, ts FROM thread_script_logs
             WHERE thread_id = ?1 ORDER BY id DESC LIMIT ?2",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(params![thread_id, lim], |r| {
            Ok(crate::thread_script::ScriptLog {
                thread_id: r.get(0).unwrap_or_default(),
                name: r.get(1).unwrap_or_default(),
                level: r.get(2).unwrap_or_default(),
                msg: r.get(3).unwrap_or_default(),
                ts: r.get(4).unwrap_or(0),
            })
        })
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Xoá log của 1 thread (nút Xoá trong UI).
    pub fn clear_script_logs(&mut self, thread_id: &str) -> Result<()> {
        if thread_id.is_empty() { return Ok(()); }
        let _ = Self::ensure_script_schema(&self.conn);
        self.conn.execute(
            "DELETE FROM thread_script_logs WHERE thread_id = ?1",
            params![thread_id],
        )?;
        Ok(())
    }

    /// Đọc ghi chú của 1 nhóm. Chưa có → `""`.
    pub fn get_group_note(&self, thread_id: &str) -> String {
        if thread_id.is_empty() {
            return String::new();
        }
        self.conn
            .query_row(
                "SELECT note FROM group_notes WHERE thread_id = ?1",
                params![thread_id],
                |r| r.get::<_, String>(0),
            )
            .unwrap_or_default()
    }

    /// Lưu ghi chú của 1 nhóm (upsert, kèm mốc cập nhật giây).
    pub fn set_group_note(&mut self, thread_id: &str, note: &str, updated_at: i64) -> Result<()> {
        if thread_id.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO group_notes (thread_id, note, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(thread_id) DO UPDATE SET note = excluded.note,
                 updated_at = excluded.updated_at",
            params![thread_id, note, updated_at],
        )?;
        Ok(())
    }

    // ── Saved items toàn cục (WaveA2) ─────────────────────────────
    /// Lưu tin (star = saved, không note). Trả `true` nếu thêm mới.
    pub fn save_message_item(&mut self, msg_id: &str, thread_id: &str, created_at: i64) -> Result<bool> {
        if msg_id.is_empty() { return Ok(false); }
        let rows = self.conn.execute(
            "INSERT OR IGNORE INTO saved_items (msg_id, thread_id, note, due_at, created_at)
             VALUES (?1, ?2, '', 0, ?3)",
            params![msg_id, thread_id, created_at],
        )?;
        Ok(rows > 0)
    }

    /// Bỏ lưu tin. Trả `true` nếu đã xoá.
    pub fn unsave_message_item(&mut self, msg_id: &str) -> Result<bool> {
        if msg_id.is_empty() { return Ok(false); }
        let rows = self.conn.execute("DELETE FROM saved_items WHERE msg_id = ?1", params![msg_id])?;
        Ok(rows > 0)
    }

    /// Tin có đang lưu không?
    pub fn is_message_saved(&self, msg_id: &str) -> bool {
        if msg_id.is_empty() { return false; }
        self.conn.query_row(
            "SELECT 1 FROM saved_items WHERE msg_id = ?1",
            params![msg_id],
            |_| Ok(()),
        ).is_ok()
    }

    /// Ghi chú / hạn cho tin đã lưu. Tin chưa lưu → upsert luôn.
    pub fn set_saved_note_due(&mut self, msg_id: &str, thread_id: &str, note: &str, due_at: i64) -> Result<()> {
        if msg_id.is_empty() { return Ok(()); }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        self.conn.execute(
            "INSERT INTO saved_items (msg_id, thread_id, note, due_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(msg_id) DO UPDATE SET note = excluded.note, due_at = excluded.due_at,
                 thread_id = CASE WHEN excluded.thread_id != '' THEN excluded.thread_id ELSE saved_items.thread_id END",
            params![msg_id, thread_id, note, due_at, now],
        )?;
        Ok(())
    }

    /// Liệt kê tin đã lưu (mới lưu trước), kèm text/tên thread qua JOIN messages/threads.
    pub fn list_saved_items(&self) -> Vec<SavedItem> {
        let mut stmt = match self.conn.prepare(
            "SELECT s.msg_id, s.thread_id, s.note, s.due_at, s.created_at,
                    COALESCE(m.text, ''), COALESCE(m.sender_name, ''), COALESCE(m.ts, 0),
                    COALESCE(t.name, ''), COALESCE(t.is_group, 0)
             FROM saved_items s
             LEFT JOIN messages m ON m.msg_id = s.msg_id
             LEFT JOIN threads t ON t.thread_id = s.thread_id
             ORDER BY s.created_at DESC, s.msg_id DESC",
        ) { Ok(s) => s, Err(_) => return Vec::new() };
        stmt.query_map([], |r| {
            let is_group: i64 = r.get(9).unwrap_or(0);
            Ok(SavedItem {
                msg_id: r.get(0)?, thread_id: r.get(1)?, note: r.get(2)?,
                due_at: r.get(3)?, created_at: r.get(4)?,
                text: r.get(5)?, sender_name: r.get(6)?, ts: r.get(7)?,
                thread_name: r.get(8)?, is_group: is_group != 0,
            })
        }).ok().map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    /// Tin đã lưu tới hạn (`due_at > 0 && due_at <= now`).
    pub fn due_saved_items(&self, now_sec: i64) -> Vec<SavedItem> {
        self.list_saved_items().into_iter().filter(|s| s.due_at > 0 && s.due_at <= now_sec).collect()
    }
    // ── Drafts + message reminders (WaveA1) ─────────────────────────
    /// Lấy nháp composer của 1 thread. DB thiếu bảng → `None` (không crash tick nền).
    pub fn get_draft(&self, thread_id: &str) -> Option<DraftEntry> {
        if thread_id.is_empty() {
            return None;
        }
        let mut stmt = self
            .conn
            .prepare("SELECT thread_id, text, reply_ref, updated_at FROM drafts WHERE thread_id = ?1")
            .ok()?;
        stmt.query_row(params![thread_id], |r| {
            Ok(DraftEntry {
                thread_id: r.get(0).unwrap_or_default(),
                text: r.get(1).unwrap_or_default(),
                reply_ref: r.get(2).unwrap_or_default(),
                updated_at: r.get(3).unwrap_or(0),
            })
        })
        .ok()
    }

    /// Lưu/đè nháp composer của 1 thread (text rỗng + không reply → xoá row).
    pub fn set_draft(&mut self, thread_id: &str, text: &str, reply_ref: &str, updated_at: i64) -> Result<()> {
        if thread_id.is_empty() {
            return Ok(());
        }
        let _ = Self::ensure_draft_reminder_schema(&self.conn);
        if text.trim().is_empty() && reply_ref.trim().is_empty() {
            let _ = self
                .conn
                .execute("DELETE FROM drafts WHERE thread_id = ?1", params![thread_id]);
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO drafts (thread_id, text, reply_ref, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(thread_id) DO UPDATE SET text = excluded.text,
                 reply_ref = excluded.reply_ref, updated_at = excluded.updated_at",
            params![thread_id, text, reply_ref, updated_at],
        )?;
        Ok(())
    }

    /// Xoá nháp của 1 thread (gửi xong).
    pub fn delete_draft(&mut self, thread_id: &str) -> Result<()> {
        if thread_id.is_empty() {
            return Ok(());
        }
        let _ = Self::ensure_draft_reminder_schema(&self.conn);
        let _ = self
            .conn
            .execute("DELETE FROM drafts WHERE thread_id = ?1", params![thread_id]);
        Ok(())
    }

    /// Liệt kê mọi thread đang có nháp (để vẽ badge "nháp" ở sidebar).
    pub fn list_draft_threads(&self) -> Vec<String> {
        let mut stmt = match self.conn.prepare(
            "SELECT thread_id FROM drafts WHERE TRIM(text) != '' OR TRIM(reply_ref) != ''",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |r| r.get::<_, String>(0))
            .ok()
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// Tạo 1 lần nhắc về tin nhắn. `due_at` phải > `now_sec`.
    pub fn add_reminder(
        &mut self,
        thread_id: &str,
        msg_id: &str,
        due_at: i64,
        text_preview: &str,
        now_sec: i64,
    ) -> Result<ReminderItem> {
        if thread_id.is_empty() || msg_id.is_empty() {
            anyhow::bail!("Thiếu thread_id/msg_id");
        }
        if due_at <= now_sec {
            anyhow::bail!("Thời điểm nhắc phải nằm trong tương lai");
        }
        let _ = Self::ensure_draft_reminder_schema(&self.conn);
        let preview: String = text_preview.chars().take(140).collect();
        self.conn.execute(
            "INSERT INTO message_reminders (thread_id, msg_id, due_at, text_preview, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![thread_id, msg_id, due_at, preview, now_sec],
        )?;
        Ok(ReminderItem {
            id: self.conn.last_insert_rowid(),
            thread_id: thread_id.to_string(),
            msg_id: msg_id.to_string(),
            due_at,
            text_preview: preview,
            created_at: now_sec,
        })
    }

    /// Liệt kê mọi reminder, tới hạn trước (QL list).
    pub fn list_reminders(&self) -> Vec<ReminderItem> {
        let mut stmt = match self.conn.prepare(
            "SELECT id, thread_id, msg_id, due_at, text_preview, created_at
             FROM message_reminders ORDER BY due_at ASC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |r| {
            Ok(ReminderItem {
                id: r.get(0).unwrap_or(0),
                thread_id: r.get(1).unwrap_or_default(),
                msg_id: r.get(2).unwrap_or_default(),
                due_at: r.get(3).unwrap_or(0),
                text_preview: r.get(4).unwrap_or_default(),
                created_at: r.get(5).unwrap_or(0),
            })
        })
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Claim + xoá atomically các reminder tới hạn (`due_at <= now`) để worker bắn toast đúng 1 lần.
    pub fn claim_due_reminders(&mut self, now_sec: i64) -> Vec<ReminderItem> {
        let mut stmt = match self.conn.prepare(
            "SELECT id, thread_id, msg_id, due_at, text_preview, created_at
             FROM message_reminders WHERE due_at > 0 AND due_at <= ?1 ORDER BY due_at ASC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let due: Vec<ReminderItem> = stmt
            .query_map(params![now_sec], |r| {
                Ok(ReminderItem {
                    id: r.get(0).unwrap_or(0),
                    thread_id: r.get(1).unwrap_or_default(),
                    msg_id: r.get(2).unwrap_or_default(),
                    due_at: r.get(3).unwrap_or(0),
                    text_preview: r.get(4).unwrap_or_default(),
                    created_at: r.get(5).unwrap_or(0),
                })
            })
            .ok()
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default();
        if due.is_empty() {
            return due;
        }
        let ids: Vec<i64> = due.iter().map(|r| r.id).collect();
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("DELETE FROM message_reminders WHERE id IN ({placeholders})");
        let _ = self.conn.execute(
            &sql,
            rusqlite::params_from_iter(ids.iter()),
        );
        due
    }

    /// Xoá 1 reminder theo id. Trả `true` nếu có row bị xoá.
    pub fn delete_reminder(&mut self, id: i64) -> Result<bool> {
        let _ = Self::ensure_draft_reminder_schema(&self.conn);
        let rows = self
            .conn
            .execute("DELETE FROM message_reminders WHERE id = ?1", params![id])?;
        Ok(rows > 0)
    }

    // ── Search operators (WaveA2) ─────────────────────────────────
    /// Query SQLite messages với operators: from:, in:, has:link|file|image|voice,
    /// before:/after: (YYYY-MM-DD), is:starred|pinned, "cụm chính xác", #hashtag.
    pub fn search_messages_advanced(&self, query: &crate::store::SearchQuery, limit: usize) -> Result<Vec<StoredMessage>> {
        let mut sql = String::from(
            "SELECT msg_id, thread_id, sender_id, sender_name, text, msg_type, ts, is_outgoing, raw_json FROM messages WHERE 1 = 1",
        );
        // Mỗi điều kiện thêm 1 placeholder `?` để rusqlite bind đúng thứ tự.
        enum P { S(String), I(i64) }
        let mut args: Vec<P> = Vec::new();
        let like = |s: &str| format!("%{}%", s.replace('%', "\\%").replace('_', "\\_"));
        if let Some(f) = &query.from {
            sql.push_str(" AND (LOWER(sender_name) LIKE LOWER(?) ESCAPE '\\')");
            args.push(P::S(like(f)));
        }
        if let Some(t) = &query.in_thread {
            sql.push_str(" AND thread_id = ?");
            args.push(P::S(t.clone()));
        }
        for h in &query.has {
            match h {
                crate::store::SearchHas::Link => sql.push_str(" AND (LOWER(text) LIKE 'http://%' ESCAPE '\\' OR LOWER(text) LIKE 'https://%' ESCAPE '\\' OR INSTR(LOWER(COALESCE(raw_json,'')), 'link_url') > 0)"),
                crate::store::SearchHas::File => sql.push_str(" AND (LOWER(text) LIKE '[tập tin%' ESCAPE '\\' OR LOWER(text) LIKE '[tep tin%' ESCAPE '\\' OR LOWER(COALESCE(msg_type,'')) LIKE '%file%')"),
                crate::store::SearchHas::Image => sql.push_str(" AND (LOWER(text) LIKE '[hình ảnh%' ESCAPE '\\' OR LOWER(text) LIKE '[hinh anh%' ESCAPE '\\' OR LOWER(COALESCE(msg_type,'')) LIKE '%photo%')"),
                crate::store::SearchHas::Voice => sql.push_str(" AND (LOWER(text) LIKE '[tin nhắn thoại%' ESCAPE '\\' OR LOWER(text) LIKE '%thoai%' ESCAPE '\\' OR LOWER(COALESCE(msg_type,'')) LIKE '%voice%')"),
            }
        }
        if let Some(b) = query.before_ms {
            sql.push_str(" AND ts < ?");
            args.push(P::I(b));
        }
        if let Some(a) = query.after_ms {
            sql.push_str(" AND ts > ?");
            args.push(P::I(a));
        }
        if query.is_starred {
            sql.push_str(" AND INSTR(COALESCE(raw_json,''), '\"is_starred\"') > 0");
        }
        if query.is_pinned {
            sql.push_str(" AND INSTR(COALESCE(raw_json,''), '\"is_pinned\"') > 0");
        }
        for ph in &query.phrases {
            sql.push_str(" AND LOWER(text) LIKE LOWER(?) ESCAPE '\\'");
            args.push(P::S(like(ph)));
        }
        for tag in &query.hashtags {
            sql.push_str(" AND LOWER(text) LIKE LOWER(?) ESCAPE '\\'");
            args.push(P::S(like(&format!("#{tag}"))));
        }
        for w in &query.words {
            sql.push_str(" AND LOWER(text) LIKE LOWER(?) ESCAPE '\\'");
            args.push(P::S(like(w)));
        }
        sql.push_str(" ORDER BY ts DESC LIMIT ?");
        args.push(P::I(limit as i64));
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(args.iter().map(|a| match a {
            P::S(s) => s as &dyn rusqlite::ToSql,
            P::I(i) => i as &dyn rusqlite::ToSql,
        })))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let is_outgoing_num: i64 = row.get(7)?;
            out.push(StoredMessage {
                msg_id: row.get(0)?, thread_id: row.get(1)?,
                sender_id: row.get(2).unwrap_or_default(),
                sender_name: row.get(3).unwrap_or_default(),
                text: row.get(4).unwrap_or_default(),
                msg_type: row.get(5).unwrap_or_default(),
                ts: row.get(6)?, is_outgoing: is_outgoing_num != 0,
                raw_json: row.get(8).unwrap_or_default(),
            });
        }
        Ok(out)
    }

    /// Thêm từ khóa theo dõi cho 1 nhóm (chuẩn hoá trim + lowercase, bỏ trống).
    pub fn add_group_watch(&mut self, thread_id: &str, keyword: &str) -> Result<bool> {
        let kw = keyword.trim().to_lowercase();
        if thread_id.is_empty() || kw.is_empty() {
            return Ok(false);
        }
        let rows = self.conn.execute(
            "INSERT OR IGNORE INTO group_watch (thread_id, keyword) VALUES (?1, ?2)",
            params![thread_id, kw],
        )?;
        Ok(rows > 0)
    }

    /// Xoá 1 từ khóa theo dõi của nhóm (chuẩn hoá trim/lowercase như lúc thêm).
    pub fn remove_group_watch(&mut self, thread_id: &str, keyword: &str) -> Result<()> {
        if thread_id.is_empty() {
            return Ok(());
        }
        let kw = keyword.trim();
        if kw.is_empty() {
            return Ok(());
        }
        let norm = kw.to_lowercase();
        // Xoá khớp chính xác trước; sau đó xoá nốt biến thể chỉ khác chữ hoa/thường
        // hoặc khoảng trắng (DB cũ lưu trước khi chuẩn hoá).
        let _ = self.conn.execute(
            "DELETE FROM group_watch WHERE thread_id = ?1 AND (keyword = ?2 OR LOWER(TRIM(keyword)) = ?3)",
            params![thread_id, kw, norm],
        )?;
        Ok(())
    }

    /// Liệt kê từ khóa theo dõi của 1 nhóm (sắp xếp A–Z).
    pub fn list_group_watch(&self, thread_id: &str) -> Vec<String> {
        if thread_id.is_empty() {
            return Vec::new();
        }
        let mut stmt = match self
            .conn
            .prepare("SELECT keyword FROM group_watch WHERE thread_id = ?1 ORDER BY keyword ASC")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(params![thread_id], |r| r.get::<_, String>(0))
            .ok()
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// Insert a message. Returns `true` if a new row was inserted, `false` if already existed.
    /// Expiry tombstones (`[Tin nhắn đã biến mất]`, empty raw_json) MUST overwrite the
    /// original row — otherwise the sweep re-persists expiry every 2s tick and a
    /// restart restores the original content from the stale row (gui.rs:1908).
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
        let is_tombstone = msg.raw_json.is_empty()
            && msg.text.trim() == crate::chat::VANISH_EXPIRED_TEXT;
        if rows == 0 && (!msg.raw_json.is_empty() || is_tombstone) {
            let raw = if is_tombstone {
                // Non-empty tombstone marker so the row is never skipped or
                // re-hydrated as original content; hydrate ignores `vanished`.
                VANISH_TOMBSTONE_JSON
            } else {
                msg.raw_json.as_str()
            };
            let _ = self.conn.execute(
                "UPDATE messages SET raw_json = ?1, msg_type = ?2, text = ?4 WHERE msg_id = ?3",
                params![raw, msg.msg_type, msg.msg_id, msg.text],
            );
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
        // Group Workspace: thread mất thì ghi chú + từ khóa theo dõi cũng mất.
        let _ = self.conn.execute(
            "DELETE FROM group_notes WHERE thread_id = ?1",
            params![thread_id],
        );
        let _ = self.conn.execute(
            "DELETE FROM group_watch WHERE thread_id = ?1",
            params![thread_id],
        );
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

    /// Insert or update thread metadata. History-sync pages carry no vanish
    /// setting (`0/0`, like the existing read-point CASE below): preserve the
    /// stored vanish instead of clearing it, so the sweep keeps expiring
    /// messages. An explicit disable still clears via `upsert_thread_vanish`
    /// when the thread carries vanish columns from a user action (#376).
    /// Callers that only carry messages (history_sync, import) MUST pass
    /// `vanish_duration_sec = 0` + `vanish_from_user = false`.
    pub fn upsert_thread(&mut self, t: &StoredThread) -> Result<()> {
        self.upsert_thread_inner(t, false, false)
    }

    /// `upsert_thread` with an explicit-disable flag. Pass `true` only when
    /// the caller knows the user's vanish intent (RAM `Thread.vanish` snapshot
    /// from `persist_threads`, or the SetThreadVanish worker lane).
    pub fn upsert_thread_with_vanish_intent(
        &mut self,
        t: &StoredThread,
        vanish_from_user: bool,
    ) -> Result<()> {
        self.upsert_thread_inner(t, vanish_from_user, false)
    }

    fn upsert_thread_inner(
        &mut self,
        t: &StoredThread,
        vanish_from_user: bool,
        contact_name: bool,
    ) -> Result<()> {
        // History-only upsert: keep stored vanish columns (CASE mirrors the
        // read-point CASE). User-intent upsert: overwrite (incl. explicit 0/0 disable).
        let (vanish_dur, vanish_start) = if vanish_from_user {
            (t.vanish_duration_sec, t.vanish_started_at_sec)
        } else {
            // Sentinel: negative keeps the CASE on the stored value.
            (-1, -1)
        };
        self.conn.execute(
            "INSERT INTO threads (thread_id, name, is_group, last_ts, vanish_duration_sec, vanish_started_at_sec, last_read_msg_id, contact_name)
             VALUES (?1, ?2, ?3, ?4, CASE WHEN ?5 < 0 THEN 0 ELSE ?5 END, CASE WHEN ?6 < 0 THEN 0 ELSE ?6 END, ?7, ?8)
             ON CONFLICT(thread_id) DO UPDATE SET
                 name = CASE
                     WHEN excluded.contact_name != 0
                          AND excluded.name IS NOT NULL AND TRIM(excluded.name) != ''
                     THEN excluded.name
                     WHEN threads.contact_name != 0
                          AND threads.name IS NOT NULL AND TRIM(threads.name) != ''
                     THEN threads.name
                     WHEN excluded.name IS NOT NULL AND TRIM(excluded.name) != ''
                          AND TRIM(excluded.name) != excluded.thread_id
                          AND TRIM(excluded.name) != ('Người dùng ' || excluded.thread_id)
                          AND TRIM(excluded.name) != ('Nhóm ' || excluded.thread_id)
                          AND TRIM(excluded.name) != ('Nhóm ' || LTRIM(excluded.thread_id, 'gG_'))
                          AND NOT (excluded.name LIKE 'Người dùng %' AND TRIM(SUBSTR(excluded.name, length('Người dùng ') + 1)) GLOB '[0-9]*')
                          AND NOT (excluded.name LIKE 'Nhóm %' AND TRIM(SUBSTR(excluded.name, length('Nhóm ') + 1)) GLOB '[0-9]*')
                          AND (
                              threads.name IS NULL OR TRIM(threads.name) = ''
                              OR TRIM(threads.name) = threads.thread_id
                              OR TRIM(threads.name) = ('Người dùng ' || threads.thread_id)
                              OR TRIM(threads.name) = ('Nhóm ' || threads.thread_id)
                              OR TRIM(threads.name) = ('Nhóm ' || LTRIM(threads.thread_id, 'gG_'))
                              OR (threads.name LIKE 'Người dùng %' AND TRIM(SUBSTR(threads.name, length('Người dùng ') + 1)) GLOB '[0-9]*')
                              OR (threads.name LIKE 'Nhóm %' AND TRIM(SUBSTR(threads.name, length('Nhóm ') + 1)) GLOB '[0-9]*')
                          )
                     THEN excluded.name
                     ELSE COALESCE(NULLIF(threads.name, ''), excluded.name)
                 END,
                 contact_name = CASE
                     WHEN excluded.contact_name != 0
                          AND excluded.name IS NOT NULL AND TRIM(excluded.name) != ''
                     THEN 1
                     ELSE threads.contact_name
                 END,
                 is_group = excluded.is_group,
                 last_ts = MAX(COALESCE(last_ts, 0), excluded.last_ts),
                 vanish_duration_sec = CASE
                     WHEN ?5 < 0 THEN threads.vanish_duration_sec
                     ELSE excluded.vanish_duration_sec
                 END,
                 vanish_started_at_sec = CASE
                     WHEN ?6 < 0 THEN threads.vanish_started_at_sec
                     ELSE excluded.vanish_started_at_sec
                 END,
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
                vanish_dur,
                vanish_start,
                t.last_read_msg_id,
                if contact_name { 1i64 } else { 0i64 },
            ],
        )?;
        if vanish_from_user {
            self.upsert_thread_vanish(&t.thread_id, t.vanish_duration_sec, t.vanish_started_at_sec)?;
        }
        Ok(())
    }

    /// Lưu read-point (id tin cuối đã đọc) của 1 thread (#119). Upsert an toàn
    /// kể cả khi thread chưa có row.
    pub fn set_thread_read(&mut self, thread_id: &str, last_msg_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO threads (thread_id, last_read_msg_id)
             VALUES (?1, ?2)
             ON CONFLICT(thread_id) DO UPDATE SET last_read_msg_id = excluded.last_read_msg_id",
            params![thread_id, last_msg_id],
        )?;
        Ok(())
    }

    /// Lưu cấu hình vanish 1 thread vào bảng phụ `thread_vanish` (persist + load restart).
    pub fn upsert_thread_vanish(
        &mut self,
        thread_id: &str,
        duration_sec: i64,
        started_at_sec: i64,
    ) -> Result<()> {
        if thread_id.is_empty() {
            return Ok(());
        }
        if duration_sec > 0 {
            self.conn.execute(
                "INSERT INTO thread_vanish (thread_id, duration_sec, started_at_sec)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(thread_id) DO UPDATE SET duration_sec = excluded.duration_sec,
                     started_at_sec = excluded.started_at_sec",
                params![thread_id, duration_sec, started_at_sec],
            )?;
        } else {
            let _ = self.conn.execute(
                "DELETE FROM thread_vanish WHERE thread_id = ?1",
                params![thread_id],
            );
        }
        Ok(())
    }

    /// Đọc cấu hình vanish 1 thread. `None` = tắt.
    pub fn load_thread_vanish(&self, thread_id: &str) -> Option<(i64, i64)> {
        self.conn
            .query_row(
                "SELECT duration_sec, started_at_sec FROM thread_vanish WHERE thread_id = ?1",
                params![thread_id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .ok()
            .filter(|(d, _)| *d > 0)
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

    /// Đếm toàn bộ tin đã lưu của 1 thread (không cap) — dùng cho thống kê nhóm.
    /// Bảng `messages` thiếu (DB cũ) → `Ok(0)` thay vì lỗi.
    pub fn count_messages(&self, thread_id: &str) -> Result<usize> {
        if thread_id.is_empty() {
            return Ok(0);
        }
        let n: i64 = match self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE thread_id = ?1",
            params![thread_id],
            |r| r.get(0),
        ) {
            Ok(n) => n,
            Err(_) => return Ok(0),
        };
        Ok(n.max(0) as usize)
    }

    /// Đọc toàn bộ lịch sử đã lưu của 1 thread (cũ → mới, không cap).
    /// Thread rất dài có thể nặng RAM — caller (stats) chỉ gọi khi cần số toàn DB.
    pub fn messages_for_all(&self, thread_id: &str) -> Result<Vec<StoredMessage>> {
        if thread_id.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = match self.conn.prepare(
            "SELECT msg_id, thread_id, sender_id, sender_name, text, msg_type, ts, is_outgoing, raw_json
             FROM messages
             WHERE thread_id = ?1
             ORDER BY ts ASC",
        ) {
            Ok(s) => s,
            Err(_) => return Ok(Vec::new()),
        };
        let rows = stmt.query_map(params![thread_id], |row| {
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
        Ok(self
            .all_threads_with_contact_flag()?
            .into_iter()
            .map(|(st, _)| st)
            .collect())
    }

    fn all_threads_with_contact_flag(&self) -> Result<Vec<(StoredThread, bool)>> {
        // Cột vanish chỉ có sau migration 2; SELECT liệt kê tường minh + fallback PRAGMA
        // để DB cũ chưa migrate vẫn đọc được (migration chạy ở `open`, đây là lưới an toàn).
        let cols: Vec<String> = self
            .conn
            .prepare("PRAGMA table_info(threads)")
            .ok()
            .and_then(|mut s| {
                s.query_map([], |r| r.get::<_, String>(1))
                    .ok()
                    .map(|rows| rows.flatten().collect::<Vec<_>>())
            })
            .unwrap_or_default();
        let has_vcols = cols.iter().any(|c| c == "vanish_duration_sec")
            && cols.iter().any(|c| c == "vanish_started_at_sec");
        let has_read_col = cols.iter().any(|c| c == "last_read_msg_id");
        let has_contact_col = cols.iter().any(|c| c == "contact_name");
        let v_select = if has_vcols {
            "vanish_duration_sec, vanish_started_at_sec"
        } else {
            "0, 0"
        };
        let r_select = if has_read_col {
            "last_read_msg_id"
        } else {
            "''"
        };
        let c_select = if has_contact_col { "contact_name" } else { "0" };
        let sql = format!(
            "SELECT thread_id, name, is_group, last_ts, {v_select}, {r_select}, {c_select}
             FROM threads
             ORDER BY last_ts DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            let is_group_num: i64 = row.get(2).unwrap_or(0);
            let contact_num: i64 = row.get(7).unwrap_or(0);
            Ok((
                StoredThread {
                    thread_id: row.get(0)?,
                    name: row.get(1).unwrap_or_default(),
                    is_group: is_group_num != 0,
                    last_ts: row.get(3).unwrap_or(0),
                    vanish_duration_sec: row.get(4).unwrap_or(0),
                    vanish_started_at_sec: row.get(5).unwrap_or(0),
                    last_read_msg_id: row.get(6).unwrap_or_default(),
                },
                contact_num != 0,
            ))
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

        let stored_threads = self.all_threads_with_contact_flag().unwrap_or_default();
        for (st, st_contact_name) in stored_threads {
            if st.thread_id.is_empty() || st.thread_id == "0" {
                continue;
            }
            if let Some(existing) = threads.iter_mut().find(|t| t.id == st.thread_id) {
                if !st.name.trim().is_empty() {
                    if st_contact_name {
                        crate::chat::adopt_thread_name(existing, st.name.clone(), true);
                    } else if !existing.presentation.contact_name
                        && !crate::chat::is_placeholder_thread_name(&st.name, &st.thread_id)
                        && (existing.name.trim().is_empty()
                            || crate::chat::is_placeholder_thread_name(&existing.name, &existing.id)
                            || crate::chat::is_generic_thread_name(&existing.name))
                    {
                        existing.name = st.name.clone();
                    }
                }
                existing.is_group = existing.is_group || st.is_group;
                apply_stored_vanish(existing, &st, self);
                // Khôi phục read-point đã lưu (#119, #565): chỉ tiến, không lùi.
                // SQLite có thể giữ 100 trong khi RAM đã 200 (đọc tiếp sau hydrate):
                // gán thẳng sẽ đếm lại tin 150 thành unread.
                merge_stored_read_point(&mut existing.unread, &st.last_read_msg_id);
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
                let is_placeholder =
                    crate::chat::is_placeholder_thread_name(&st.name, &st.thread_id);
                threads.push(Thread {
                    presentation: crate::chat::SidebarPresentation {
                        contact_name: st_contact_name,
                        ..Default::default()
                    },
                    unread,
                    id: st.thread_id.clone(),
                    name: if st.name.trim().is_empty() || (!st_contact_name && is_placeholder) {
                        if st.is_group {
                            format!("Nhóm {}", st.thread_id)
                        } else {
                            format!("Người dùng {}", st.thread_id)
                        }
                    } else {
                        st.name.clone()
                    },
                    preview: String::new(),
                    time: String::new(),
                    avatar: None,
                    is_group: st.is_group,
                    messages: Vec::new(),
                    vanish: vanish_from_stored(&st, self),
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
                let mut is_pinned = false;
                let mut is_starred = false;
                let mut is_edited = false;
                let mut link_url = None;
                let mut link_title = None;
                let mut link_desc = None;
                let mut link_thumb = None;
                let mut link_src = None;
                let mut media_group: Option<crate::chat::MediaGroupLayout> = None;
                if !sm.raw_json.is_empty() {
                    // Expiry tombstone (`{"vanished":true}`): authoritative —
                    // keep the tombstone text, restore NO metadata, and never
                    // let stored links resurrect expired content (#1908).
                    let is_tombstone = sm.raw_json.trim() == VANISH_TOMBSTONE_JSON
                        || serde_json::from_str::<serde_json::Value>(&sm.raw_json)
                            .ok()
                            .and_then(|v| v.get("vanished").and_then(|b| b.as_bool()))
                            .unwrap_or(false);
                    if is_tombstone {
                        let cm = ChatMessage {
                            id: sm.msg_id,
                            sender: if !sm.sender_name.is_empty() {
                                sm.sender_name
                            } else {
                                sm.sender_id
                            },
                            text: sm.text,
                            time: if sm.ts > 0 {
                                crate::clock::message_time(sm.ts)
                            } else {
                                crate::ui::clock()
                            },
                            is_outgoing: sm.is_outgoing,
                            ts: sm.ts,
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
                            existing.text = cm.text.clone();
                            existing.translation = None;
                            existing.link_url = None;
                            existing.link_title = None;
                            existing.link_desc = None;
                            existing.link_thumb = None;
                            existing.link_src = None;
                            existing.sticker_id = None;
                            existing.sticker_cate_id = None;
                            existing.sticker_type = None;
                        } else {
                            t.messages.push(cm);
                            loaded_count += 1;
                        }
                        continue;
                    }
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
                        is_edited = val
                            .get("is_edited")
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
                        media_group = val
                            .get("media_group")
                            .and_then(|v| serde_json::from_value(v.clone()).ok());
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
                    delivery: if sm.is_outgoing {
                        crate::message_model::DeliveryState::ServerAccepted
                    } else {
                        crate::message_model::DeliveryState::Unknown
                    },
                    sticker_id,
                    sticker_cate_id,
                    sticker_type,
                    reply_to_id,
                    reply_to_sender,
                    reply_to_text,
                    reactions,
                    reaction_summaries,
                    is_pinned,
                    is_starred,
                    edited: is_edited,
                    link_url,
                    link_title,
                    link_desc,
                    link_thumb,
                    link_src,
                    media_group,

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
                    existing.is_pinned = existing.is_pinned || cm.is_pinned;
                    existing.is_starred = existing.is_starred || cm.is_starred;
                    existing.edited = existing.edited || cm.edited;
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
        if m.is_pinned {
            meta.insert("is_pinned".into(), serde_json::json!(true));
        }
        if m.is_starred {
            meta.insert("is_starred".into(), serde_json::json!(true));
        }
        if m.edited {
            meta.insert("is_edited".into(), serde_json::json!(true));
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
        if let Some(group) = &m.media_group {
            meta.insert("media_group".into(), serde_json::json!(group));
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


/// Dựng `VanishSetting` từ hàng `threads` (ưu tiên), fallback bảng phụ `thread_vanish`.
/// `None` = tắt.
fn vanish_from_stored(st: &StoredThread, store: &MessageStore) -> Option<crate::chat::VanishSetting> {
    if st.vanish_duration_sec > 0 {
        return Some(crate::chat::VanishSetting {
            duration_sec: st.vanish_duration_sec,
            started_at_sec: st.vanish_started_at_sec,
        });
    }
    store
        .load_thread_vanish(&st.thread_id)
        .map(|(duration_sec, started_at_sec)| crate::chat::VanishSetting {
            duration_sec,
            started_at_sec,
        })
}

/// Áp cấu hình vanish đã lưu vào thread có sẵn (không đè vanish đang bật trong RAM).
fn apply_stored_vanish(thread: &mut Thread, st: &StoredThread, store: &MessageStore) {
    if thread.vanish.as_ref().is_some_and(|v| v.enabled()) {
        return;
    }
    thread.vanish = vanish_from_stored(st, store);
}

/// Merge a stored read-point into RAM without moving the cursor backward
/// (#565): keep the max of stored vs existing, comparing numerically; either
/// side absent or non-comparable leaves the other side intact.
pub fn merge_stored_read_point(unread: &mut crate::unread::UnreadState, stored: &str) {
    if stored.is_empty() {
        return;
    }
    match unread.last_seen_id.clone() {
        None => {
            unread.last_seen_id = Some(stored.to_string());
        }
        Some(cur) if cur == stored => {}
        Some(cur) => {
            match crate::unread::compare_ids(stored, &cur) {
                Some(std::cmp::Ordering::Greater) => {
                    unread.last_seen_id = Some(stored.to_string());
                }
                None => {
                    // Không so được (non-numeric): giữ read-point đã có trong RAM,
                    // vốn mới hơn vì được advance sau hydrate.
                }
                _ => {}
            }
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
            vanish_duration_sec: t.vanish.as_ref().map(|v| v.duration_sec).unwrap_or(0),
            vanish_started_at_sec: t.vanish.as_ref().map(|v| v.started_at_sec).unwrap_or(0),
            last_read_msg_id: t.unread.last_seen_id.clone().unwrap_or_default(),
        };
        // RAM snapshot carries the user's vanish intent (incl. explicit disable),
        // unlike history-sync pages which must preserve stored vanish (#376).
        store.upsert_thread_inner(&st, true, t.presentation.contact_name)?;
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
            if let Some(decrypted_bytes) = try_decrypt_zalo_pc_database(&bytes) {
                let tmp_dir = std::env::temp_dir();
                let tmp_name = format!(
                    "ctdozlite_import_{}_{}.db",
                    std::process::id(),
                    report.sources_read.len()
                );
                let tmp_path = tmp_dir.join(tmp_name);
                if std::fs::write(&tmp_path, &decrypted_bytes).is_ok() {
                    if let Ok(src_conn) = Connection::open(&tmp_path) {
                        let _ = import_official_zalo_pc_db(
                            &src_conn,
                            &mut dst,
                            &file_path,
                            &mut report,
                        );
                    }
                    let _ = std::fs::remove_file(&tmp_path);
                }
            }
        }
    }

    if src.is_dir() {
        let _ = scan_and_import_zalo_data_dir(src, &mut dst, &mut report);
    }

    Ok(report)
}

/// Scan a directory for Zalo PC's `Core/Message` database folder and `Index.db`, decrypting and importing them.
fn scan_and_import_zalo_data_dir(
    root: &std::path::Path,
    dst: &mut MessageStore,
    report: &mut ImportReport,
) -> Result<()> {
    // 1. Recover the 32-char XOR key from Local Storage LevelDB if present
    let mut cipher_key = recover_zalo_localstorage_key(root);
    if cipher_key.is_empty() {
        cipher_key = "de039414865b856ea650d3305b1f387f".to_string();
    }

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
            let name = if !dname.trim().is_empty() {
                dname
            } else {
                zname
            };
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
            let name = if !dname.trim().is_empty() {
                dname
            } else {
                zname
            };
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
        vanish_duration_sec: 0,
        vanish_started_at_sec: 0,
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

/// Recover the 32-character XOR key from LevelDB in `Partitions/zalo/Local Storage/leveldb`.
pub fn recover_zalo_localstorage_key(base_dir: &std::path::Path) -> String {
    let cand_dirs = vec![base_dir
        .join("Partitions")
        .join("zalo")
        .join("Local Storage")
        .join("leveldb")];

    let mut key = vec![None; 32];
    let u_pat = "z_ecrp_"
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect::<Vec<u8>>();

    for ldb_dir in cand_dirs {
        let Ok(entries) = std::fs::read_dir(ldb_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or_default();
            if ext != "ldb" && ext != "log" {
                continue;
            }
            let Ok(b) = std::fs::read(&p) else {
                continue;
            };
            let mut pos = 0;
            while let Some(rel) = b[pos..]
                .windows(u_pat.len())
                .position(|w| w == u_pat.as_slice())
            {
                let idx = pos + rel + u_pat.len();
                pos = idx;
                let mut chars = Vec::new();
                for i in (0..64).step_by(2) {
                    if idx + i + 1 < b.len() {
                        chars.push((b[idx + i] as u16 | ((b[idx + i + 1] as u16) << 8)) as u32);
                    }
                }
                if chars.len() >= 32 {
                    let prefix = b"{\"version\":";
                    let cand: Vec<u32> = chars
                        .iter()
                        .take(prefix.len())
                        .zip(prefix.iter())
                        .map(|(c, p)| c ^ (*p as u32))
                        .collect();
                    if cand.iter().all(|x| (*x as u8 as char).is_ascii_hexdigit()) {
                        for (i, v) in cand.iter().enumerate() {
                            key[i] = Some(*v as u8 as char);
                        }
                        let suffix = b",\"list_recent\":[{\"";
                        if chars.len() >= 14 + suffix.len() {
                            let cand2: Vec<u32> = chars[14..14 + suffix.len()]
                                .iter()
                                .zip(suffix.iter())
                                .map(|(c, p)| c ^ (*p as u32))
                                .collect();
                            if cand2.iter().all(|x| (*x as u8 as char).is_ascii_hexdigit()) {
                                for (i, v) in cand2.iter().enumerate() {
                                    key[14 + i] = Some(*v as u8 as char);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut full_key = String::new();
    for k in key {
        match k {
            Some(c) => full_key.push(c),
            None => return "de039414865b856ea650d3305b1f387f".to_string(),
        }
    }
    full_key
}

/// Decrypt single official Zalo PC database bytes using PBKDF2-HMAC-SHA512 + AES-256-CBC (SQLCipher v4 profile).
pub fn try_decrypt_zalo_pc_database(bytes: &[u8]) -> Option<Vec<u8>> {
    let key = "de039414865b856ea650d3305b1f387f";
    decrypt_sqlcipher_bytes(bytes, key.as_bytes())
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

fn import_official_zalo_pc_db(
    src_conn: &Connection,
    dst: &mut MessageStore,
    file_path: &std::path::Path,
    report: &mut ImportReport,
) -> Result<()> {
    let stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("import_pc");
    let cipher_key = "de039414865b856ea650d3305b1f387f";
    import_message_table(
        src_conn,
        dst,
        stem,
        stem,
        stem.starts_with('g'),
        cipher_key,
        report,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_watch_normalizes_case_and_spaces_on_add_and_remove() {
        let mut store = MessageStore::open(":memory:").unwrap();
        assert!(store.add_group_watch("g1", "  Deadline  ").unwrap());
        // trùng sau chuẩn hoá → không thêm mới.
        assert!(!store.add_group_watch("g1", "deadline").unwrap());
        assert_eq!(store.list_group_watch("g1"), vec!["deadline".to_string()]);
        // xoá với padding + khác hoa/thường → gỡ được watch đã cài.
        store.remove_group_watch("g1", "  DEADLINE  ").unwrap();
        assert!(store.list_group_watch("g1").is_empty());
    }
    #[test]
    fn saved_items_crud_and_note_due() {
        let mut store = MessageStore::open(":memory:").unwrap();
        assert!(store.save_message_item("m1", "t1", 100).unwrap());
        assert!(!store.save_message_item("m1", "t1", 100).unwrap());
        assert!(store.is_message_saved("m1"));
        store.set_saved_note_due("m1", "t1", "xem lại", 200).unwrap();
        let items = store.list_saved_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].note, "xem lại");
        assert_eq!(items[0].due_at, 200);
        assert_eq!(store.due_saved_items(150).len(), 0);
        assert_eq!(store.due_saved_items(200).len(), 1);
        assert!(store.unsave_message_item("m1").unwrap());
        assert!(!store.is_message_saved("m1"));
        assert!(store.list_saved_items().is_empty());
    }

    #[test]
    fn parse_search_query_operators() {
        let q = parse_search_query("from:Long has:image \"cụm chính xác\" #hashtag hello is:starred before:2026-01-01 after:2025-01-01");
        assert_eq!(q.from.as_deref(), Some("Long"));
        assert_eq!(q.has, vec![SearchHas::Image]);
        assert_eq!(q.phrases, vec!["cụm chính xác".to_string()]);
        assert_eq!(q.hashtags, vec!["hashtag".to_string()]);
        assert_eq!(q.words, vec!["hello".to_string()]);
        assert!(q.is_starred);
        assert!(q.before_ms.is_some() && q.after_ms.is_some());
    }

    #[test]
    fn search_advanced_from_has_image() {
        let mut store = MessageStore::open(":memory:").unwrap();
        for (id, sender, text, mt) in [
            ("a1", "Long", "[Hình ảnh] beach", "chat.photo"),
            ("a2", "Nam", "hello world", "chat.text"),
        ] {
            store.upsert_message(&StoredMessage {
                msg_id: id.into(), thread_id: "t1".into(),
                sender_id: sender.into(), sender_name: sender.into(),
                text: text.into(), msg_type: mt.into(), ts: 1700000000000,
                is_outgoing: false, raw_json: String::new(),
            }).unwrap();
        }
        let q = parse_search_query("from:Long has:image");
        let hits = store.search_messages_advanced(&q, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].msg_id, "a1");
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
                vanish_duration_sec: 0,
                vanish_started_at_sec: 0,
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
            vanish_duration_sec: 0,
            vanish_started_at_sec: 0,
            last_read_msg_id: String::new(),
        };
        store.upsert_thread(&st(300)).unwrap();
        store.upsert_thread(&st(100)).unwrap();
        assert_eq!(store.all_threads().unwrap()[0].last_ts, 300);
    }

    #[test]
    fn thread_read_point_roundtrips_and_survives_reupsert() {
        let mut store = MessageStore::open(":memory:").unwrap();
        store.set_thread_read("t1", "msg-100").unwrap();
        let got = store.all_threads().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].last_read_msg_id, "msg-100");
        // Upsert thread không mang read-point thì giữ cũ, không xóa.
        store
            .upsert_thread(&StoredThread {
                thread_id: "t1".into(),
                name: "A".into(),
                is_group: false,
                last_ts: 1,
                vanish_duration_sec: 0,
                vanish_started_at_sec: 0,
                last_read_msg_id: String::new(),
            })
            .unwrap();
        assert_eq!(store.all_threads().unwrap()[0].last_read_msg_id, "msg-100");
        // Đọc tiếp thì cập nhật.
        store.set_thread_read("t1", "msg-200").unwrap();
        assert_eq!(store.all_threads().unwrap()[0].last_read_msg_id, "msg-200");
    }
    #[test]
    fn history_upsert_preserves_vanish_and_explicit_disable_clears() {
        // #376: history-sync page (vanish 0/0, no intent) must not clear vanish.
        let mut store = MessageStore::open(":memory:").unwrap();
        store
            .upsert_thread_with_vanish_intent(
                &StoredThread {
                    thread_id: "g1".into(),
                    name: "Nhóm".into(),
                    is_group: true,
                    last_ts: 1,
                    vanish_duration_sec: 3600,
                    vanish_started_at_sec: 1000,
                    last_read_msg_id: String::new(),
                },
                true,
            )
            .unwrap();
        store
            .upsert_thread(&StoredThread {
                thread_id: "g1".into(),
                name: "Nhóm".into(),
                is_group: true,
                last_ts: 5,
                vanish_duration_sec: 0,
                vanish_started_at_sec: 0,
                last_read_msg_id: String::new(),
            })
            .unwrap();
        let got = store.all_threads().unwrap();
        assert_eq!(got[0].vanish_duration_sec, 3600, "history upsert cleared vanish");
        assert_eq!(store.load_thread_vanish("g1"), Some((3600, 1000)));
        // Explicit user disable DOES clear (intent path from persist_threads).
        store
            .upsert_thread_with_vanish_intent(
                &StoredThread {
                    thread_id: "g1".into(),
                    name: "Nhóm".into(),
                    is_group: true,
                    last_ts: 5,
                    vanish_duration_sec: 0,
                    vanish_started_at_sec: 0,
                    last_read_msg_id: String::new(),
                },
                true,
            )
            .unwrap();
        let got = store.all_threads().unwrap();
        assert_eq!(got[0].vanish_duration_sec, 0);
        assert_eq!(store.load_thread_vanish("g1"), None);
    }

    #[test]
    fn hydrate_never_moves_read_point_backward() {
        // #565: SQLite 100 + RAM 200 → keep 200 (no recount of 150 as unread).
        let mut store = MessageStore::open(":memory:").unwrap();
        store.set_thread_read("t1", "100").unwrap();
        let mut threads = vec![Thread {
            id: "t1".into(),
            name: "A".into(),
            unread: crate::unread::UnreadState {
                last_seen_id: Some("200".into()),
                ..Default::default()
            },
            ..Default::default()
        }];
        store.hydrate_threads(&mut threads, 10).unwrap();
        assert_eq!(
            threads[0].unread.last_seen_id.as_deref(),
            Some("200"),
            "hydrate moved read-point backward"
        );
        // Forward case still advances: RAM none + stored 100 → 100.
        let mut fresh: Vec<Thread> = vec![Thread {
            id: "t1".into(),
            name: "A".into(),
            ..Default::default()
        }];
        store.hydrate_threads(&mut fresh, 10).unwrap();
        assert_eq!(
            fresh[0].unread.last_seen_id.as_deref(),
            Some("100")
        );
        // Direct merge helper: newer stored wins, older stored ignored.
        let mut u = crate::unread::UnreadState {
            last_seen_id: Some("200".into()),
            ..Default::default()
        };
        crate::store::merge_stored_read_point(&mut u, "100");
        assert_eq!(u.last_seen_id.as_deref(), Some("200"));
        crate::store::merge_stored_read_point(&mut u, "300");
        assert_eq!(u.last_seen_id.as_deref(), Some("300"));
    }

    #[test]
    fn expiry_tombstone_overwrites_row_and_hydrate_keeps_it() {
        // #1908: expiring a message must overwrite the SQLite row (non-empty
        // marker) and hydrate must not restore original content/metadata.
        let mut store = MessageStore::open(":memory:").unwrap();
        let mut meta = serde_json::Map::new();
        meta.insert("link_url".into(), serde_json::json!("https://x.test/a"));
        store
            .upsert_message(&StoredMessage {
                msg_id: "m1".into(),
                thread_id: "t1".into(),
                sender_id: "u9".into(),
                sender_name: "Peer".into(),
                text: "secret link".into(),
                msg_type: "chat.text".into(),
                ts: 1000,
                is_outgoing: false,
                raw_json: serde_json::Value::Object(meta).to_string(),
            })
            .unwrap();
        // Expire → text + cleared metadata → empty raw_json tombstone form.
        let mut expired = StoredMessage {
            msg_id: "m1".into(),
            thread_id: "t1".into(),
            sender_id: "u9".into(),
            sender_name: "Peer".into(),
            text: crate::chat::VANISH_EXPIRED_TEXT.into(),
            msg_type: "chat.text".into(),
            ts: 1000,
            is_outgoing: false,
            raw_json: String::new(),
        };
        assert!(!store.upsert_message(&expired).unwrap(), "tombstone must UPDATE, not insert");
        let rows = store.messages_for("t1", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, crate::chat::VANISH_EXPIRED_TEXT);
        assert_eq!(rows[0].raw_json, crate::store::VANISH_TOMBSTONE_JSON);
        // Hydrate restores tombstone text with NO link metadata.
        store
            .upsert_thread(&StoredThread {
                thread_id: "t1".into(),
                name: "A".into(),
                is_group: false,
                last_ts: 1000,
                vanish_duration_sec: 3600,
                vanish_started_at_sec: 1,
                last_read_msg_id: String::new(),
            })
            .unwrap();
        let mut threads: Vec<Thread> = Vec::new();
        store.hydrate_threads(&mut threads, 10).unwrap();
        let t = threads.iter().find(|t| t.id == "t1").expect("thread hydrated");
        let m = t.messages.iter().find(|m| m.id == "m1").expect("msg hydrated");
        assert_eq!(m.text, crate::chat::VANISH_EXPIRED_TEXT);
        assert!(m.link_url.is_none(), "hydrate restored expired link");
        // Sweep path skips tombstones (no re-persist loop): simulate the check.
        expired.raw_json = rows[0].raw_json.clone();
        assert!(
            expired.text.trim() == crate::chat::VANISH_EXPIRED_TEXT
                || expired.raw_json.trim() == crate::store::VANISH_TOMBSTONE_JSON
        );
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
            vanish: None,
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
                vanish_duration_sec: 0,
                vanish_started_at_sec: 0,
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
                    vanish_duration_sec: 0,
                    vanish_started_at_sec: 0,
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
    fn group_note_and_watch_roundtrip() {
        let mut store = MessageStore::open(":memory:").unwrap();
        // Chưa có ghi chú → rỗng.
        assert_eq!(store.get_group_note("g1"), "");
        store.set_group_note("g1", "kế hoạch sprint", 111).unwrap();
        assert_eq!(store.get_group_note("g1"), "kế hoạch sprint");
        store.set_group_note("g1", "đã cập nhật", 222).unwrap();
        assert_eq!(store.get_group_note("g1"), "đã cập nhật");
        // Nhóm khác độc lập.
        store.set_group_note("g2", "riêng", 1).unwrap();
        assert_eq!(store.get_group_note("g2"), "riêng");
        assert_eq!(store.get_group_note("g1"), "đã cập nhật");
        // Watch: thêm / trùng / liệt kê / xoá.
        assert!(store.list_group_watch("g1").is_empty());
        assert!(store.add_group_watch("g1", "  báo cáo  ").unwrap());
        assert!(!store.add_group_watch("g1", "báo cáo").unwrap());
        assert!(!store.add_group_watch("g1", "   ").unwrap());
        assert!(store.add_group_watch("g1", "deadline").unwrap());
        assert_eq!(store.list_group_watch("g1"), vec!["báo cáo", "deadline"]);
        store.remove_group_watch("g1", "báo cáo").unwrap();
        assert_eq!(store.list_group_watch("g1"), vec!["deadline"]);
    }

    #[test]
    fn jxl_to_jpg_url_and_decode_zalo_cipher_payload_work() {
        let jxl = "https://photo-stal-17.zdn.vn/no/jxl/ba5262149ded4eb317fc/2aOboR74hO3nF0sTxZSJAzpYZhLbEB780ODEOKsS.jxl?jxlstatus=1";
        assert_eq!(
            jxl_to_jpg_url(jxl),
            "https://photo-stal-17.zdn.vn/no/jpg/ba5262149ded4eb317fc/2aOboR74hO3nF0sTxZSJAzpYZhLbEB780ODEOKsS.jpg"
        );
        let key = "de039414865b856ea650d3305b1f387f";
        let plain = "\"Này bố kiểm tra mail xem ạ\"";
        let key_chars: Vec<char> = key.chars().collect();
        let enc: String = plain
            .chars()
            .enumerate()
            .map(|(i, c)| {
                char::from_u32((c as u32) ^ (key_chars[i % key_chars.len()] as u32)).unwrap()
            })
            .collect();
        assert_eq!(
            decode_zalo_cipher_payload(&enc, key),
            "Này bố kiểm tra mail xem ạ"
        );
    }
    #[test]
    fn draft_roundtrip_and_empty_clears() {
        let mut store = MessageStore::open(":memory:").unwrap();
        assert!(store.get_draft("t1").is_none());
        store.set_draft("t1", "đang gõ dở", "", 1000).unwrap();
        let d = store.get_draft("t1").unwrap();
        assert_eq!(d.text, "đang gõ dở");
        assert_eq!(d.reply_ref, "");
        store.set_draft("t1", "", "m9", 1001).unwrap();
        let d2 = store.get_draft("t1").unwrap();
        assert_eq!(d2.text, "");
        assert_eq!(d2.reply_ref, "m9");
        assert_eq!(store.list_draft_threads(), vec!["t1".to_string()]);
        // Text rỗng + không reply → xoá row.
        store.set_draft("t1", "   ", "", 1002).unwrap();
        assert!(store.get_draft("t1").is_none());
        assert!(store.list_draft_threads().is_empty());
        store.set_draft("t2", "nháp 2", "", 1003).unwrap();
        store.delete_draft("t2").unwrap();
        assert!(store.get_draft("t2").is_none());
    }

    #[test]
    fn reminder_claim_once_and_list_ordered() {
        let mut store = MessageStore::open(":memory:").unwrap();
        // Quá khứ → từ chối.
        assert!(store.add_reminder("t1", "m1", 900, "tin 1", 1000).is_err());
        store.add_reminder("t1", "m1", 1100, "tin 1", 1000).unwrap();
        store.add_reminder("t1", "m2", 1050, "tin 2", 1000).unwrap();
        let listed = store.list_reminders();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].msg_id, "m2");
        // Chưa tới giờ → claim rỗng.
        assert!(store.claim_due_reminders(1049).is_empty());
        // Tới giờ → claim đúng 1 tin, đúng 1 lần (xoá sau claim).
        let due = store.claim_due_reminders(1050);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].msg_id, "m2");
        assert!(store.claim_due_reminders(1060).is_empty());
        assert_eq!(store.list_reminders().len(), 1);
        let id = store.list_reminders()[0].id;
        assert!(store.delete_reminder(id).unwrap());
        assert!(!store.delete_reminder(id).unwrap());
        assert!(store.list_reminders().is_empty());
    }

    #[test]
    fn issue_126_contact_aliases_and_group_prefix_names_survive_offline_restart_and_placeholders() {
        let mut store = MessageStore::open(":memory:").unwrap();

        // 1) Genuine numeric contact alias "123" and "Nhóm học" persisted via persist_threads
        let initial_threads = vec![
            Thread {
                presentation: crate::chat::SidebarPresentation {
                    contact_name: true,
                    ..Default::default()
                },
                id: "1000000000001".into(),
                name: "123".into(),
                is_group: false,
                ..Default::default()
            },
            Thread {
                presentation: crate::chat::SidebarPresentation {
                    contact_name: true,
                    ..Default::default()
                },
                id: "1000000000002".into(),
                name: "Nhóm học".into(),
                is_group: false,
                ..Default::default()
            },
        ];
        persist_threads(&mut store, &initial_threads).unwrap();

        // 2) Generated placeholders or non-contact profile names must NOT overwrite persisted contact aliases
        store
            .upsert_thread(&StoredThread {
                thread_id: "1000000000001".into(),
                name: "Người dùng 1000000000001".into(),
                is_group: false,
                last_ts: 200,
                ..Default::default()
            })
            .unwrap();
        store
            .upsert_thread(&StoredThread {
                thread_id: "1000000000002".into(),
                name: "Nhóm 1000000000002".into(),
                is_group: false,
                last_ts: 200,
                ..Default::default()
            })
            .unwrap();

        // 3) Offline restart: hydrate_threads into an empty Vec or over placeholder threads
        let mut restarted: Vec<Thread> = Vec::new();
        store.hydrate_threads(&mut restarted, 20).unwrap();
        let t1 = restarted.iter().find(|t| t.id == "1000000000001").unwrap();
        let t2 = restarted.iter().find(|t| t.id == "1000000000002").unwrap();
        assert_eq!(t1.name, "123");
        assert!(t1.presentation.contact_name);
        assert_eq!(t2.name, "Nhóm học");
        assert!(t2.presentation.contact_name);

        // 4) Also verify hydrate_threads replaces generated placeholders on existing in-memory threads
        let mut placeholder_ram = vec![
            Thread {
                id: "1000000000001".into(),
                name: "Người dùng 1000000000001".into(),
                is_group: false,
                ..Default::default()
            },
            Thread {
                id: "1000000000002".into(),
                name: "Nhóm 1000000000002".into(),
                is_group: false,
                ..Default::default()
            },
        ];
        store.hydrate_threads(&mut placeholder_ram, 20).unwrap();
        assert_eq!(placeholder_ram[0].name, "123");
        assert!(placeholder_ram[0].presentation.contact_name);
        assert_eq!(placeholder_ram[1].name, "Nhóm học");
        assert!(placeholder_ram[1].presentation.contact_name);
    }

    #[test]
    fn query_names_from_index_db_prefers_display_name_over_zalo_name() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE friends_info (userId TEXT, displayName TEXT, zaloName TEXT);
             CREATE TABLE friend (userId TEXT, displayName TEXT, zaloName TEXT);
             INSERT INTO friends_info VALUES ('u1', 'Biệt danh 1', 'Tên Gốc 1');
             INSERT INTO friend VALUES ('u2', '123', 'Tên Gốc 2');
             INSERT INTO friend VALUES ('u3', '   ', 'Tên Gốc 3');",
        )
        .unwrap();
        let mut map = std::collections::HashMap::new();
        query_names_from_index_db(&conn, &mut map).unwrap();
        assert_eq!(map.get("u1").map(|x| x.0.as_str()), Some("Biệt danh 1"));
        assert_eq!(map.get("u2").map(|x| x.0.as_str()), Some("123"));
        assert_eq!(map.get("u3").map(|x| x.0.as_str()), Some("Tên Gốc 3"));
    }
}
