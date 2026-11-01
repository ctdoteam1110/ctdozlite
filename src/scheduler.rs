//! Scheduled message delivery (`Hẹn giờ gửi tin nhắn` & `Nhắc hẹn`, #38, #24).
//!
//! Persists scheduled messages in SQLite (`zalo_history.db`, table `scheduled_messages`)
//! so pending deliveries survive application restarts. The background worker polls
//! [`claim_due_scheduled_messages`] and dispatches due items through `chat::send_message`.

use anyhow::Result;
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

pub const DEFAULT_DB_PATH: &str = "zalo_history.db";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulePreset {
    In15Minutes,
    In1Hour,
    Today18h,
    Tomorrow08h,
}

impl SchedulePreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::In15Minutes => "Sau 15 phút",
            Self::In1Hour => "Sau 1 giờ",
            Self::Today18h => "Chiều tối nay (18:00)",
            Self::Tomorrow08h => "Sáng mai (08:00)",
        }
    }

    pub fn resolve_unix_ts(self, now_ts: i64) -> i64 {
        compute_preset_timestamp(now_ts, self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledMessage {
    pub id: i64,
    pub thread_id: String,
    pub thread_name: String,
    pub is_group: bool,
    pub text: String,
    /// Target delivery time in Unix epoch seconds.
    pub deliver_at: i64,
    /// Creation time in Unix epoch seconds.
    pub created_at: i64,
    /// `'pending'`, `'sending'`, `'sent'`, `'cancelled'`, `'failed'`.
    pub status: String,
    pub error_msg: String,
}

fn open_scheduler_db(db_path: &str) -> Result<Connection> {
    let conn = if db_path == ":memory:" {
        Connection::open_in_memory()?
    } else {
        Connection::open(db_path)?
    };
    init_scheduler_schema(&conn)?;
    Ok(conn)
}

pub fn init_scheduler_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS scheduled_messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            thread_id TEXT NOT NULL,
            thread_name TEXT NOT NULL DEFAULT '',
            is_group INTEGER NOT NULL DEFAULT 0,
            text TEXT NOT NULL,
            deliver_at INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            error_msg TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_sched_status_deliver
            ON scheduled_messages(status, deliver_at);",
    )?;
    Ok(())
}

/// Compute a target Unix timestamp (seconds) from a quick preset relative to `now_ts`.
pub fn compute_preset_timestamp(now_ts: i64, preset: SchedulePreset) -> i64 {
    match preset {
        SchedulePreset::In15Minutes => now_ts + 15 * 60,
        SchedulePreset::In1Hour => now_ts + 60 * 60,
        SchedulePreset::Today18h => {
            if let Some(dt) = Local.timestamp_opt(now_ts, 0).single() {
                let date = dt.date_naive();
                if let Some(target_naive) = date.and_hms_opt(18, 0, 0) {
                    if let Some(target_dt) = Local.from_local_datetime(&target_naive).single() {
                        let ts = target_dt.timestamp();
                        if ts > now_ts + 60 {
                            return ts;
                        }
                    }
                }
                // If past 18:00 today, schedule for 21:00 today or 18:00 tomorrow
                if dt.hour() < 21 {
                    if let Some(t21) = date.and_hms_opt(21, 0, 0) {
                        if let Some(dt21) = Local.from_local_datetime(&t21).single() {
                            if dt21.timestamp() > now_ts + 60 {
                                return dt21.timestamp();
                            }
                        }
                    }
                }
                if let Some(next_day) = date.succ_opt() {
                    if let Some(t18) = next_day.and_hms_opt(18, 0, 0) {
                        if let Some(dt18) = Local.from_local_datetime(&t18).single() {
                            return dt18.timestamp();
                        }
                    }
                }
            }
            now_ts + 3 * 3600
        }
        SchedulePreset::Tomorrow08h => {
            if let Some(dt) = Local.timestamp_opt(now_ts, 0).single() {
                let date = dt.date_naive();
                if let Some(next_day) = date.succ_opt() {
                    if let Some(t08) = next_day.and_hms_opt(8, 0, 0) {
                        if let Some(dt08) = Local.from_local_datetime(&t08).single() {
                            return dt08.timestamp();
                        }
                    }
                }
            }
            now_ts + 12 * 3600
        }
    }
}

/// Parse user-entered schedule time (`+15m`, `+2h`, `HH:MM`, `DD/MM/YYYY HH:MM`, `YYYY-MM-DD HH:MM`).
pub fn parse_custom_schedule_input(input: &str, now_ts: i64) -> Result<i64, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("Vui lòng nhập thời gian hẹn giờ (ví dụ: 08:00, +30m, 2026-12-31 09:30)".into());
    }
    if let Some(rest) = s.strip_prefix('+') {
        let rest = rest.trim();
        if let Some(mins) = rest.strip_suffix('m').or_else(|| rest.strip_suffix("ph")) {
            let n: i64 = mins
                .trim()
                .parse()
                .map_err(|_| "Số phút không hợp lệ (ví dụ: +15m)".to_string())?;
            if n <= 0 {
                return Err("Thời gian hẹn giờ phải lớn hơn thời điểm hiện tại".into());
            }
            return Ok(now_ts + n * 60);
        }
        if let Some(hours) = rest.strip_suffix('h').or_else(|| rest.strip_suffix('g')) {
            let n: i64 = hours
                .trim()
                .parse()
                .map_err(|_| "Số giờ không hợp lệ (ví dụ: +2h)".to_string())?;
            if n <= 0 {
                return Err("Thời gian hẹn giờ phải lớn hơn thời điểm hiện tại".into());
            }
            return Ok(now_ts + n * 3600);
        }
    }

    let now_local = Local
        .timestamp_opt(now_ts, 0)
        .single()
        .ok_or_else(|| "Không xác định được múi giờ hệ thống".to_string())?;

    // Try full datetime formats first
    for fmt in [
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%d/%m/%Y %H:%M",
        "%d/%m/%Y %H:%M:%S",
        "%d-%m-%Y %H:%M",
    ] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            if let Some(ldt) = Local.from_local_datetime(&ndt).single() {
                let ts = ldt.timestamp();
                if ts <= now_ts {
                    return Err("Thời điểm hẹn giờ phải nằm trong tương lai".into());
                }
                return Ok(ts);
            }
        }
    }

    // Try `DD/MM HH:MM` (current year)
    if let Some((dpart, tpart)) = s.split_once(' ') {
        if let Some((dd, mm)) = dpart.split_once('/') {
            if dd.split('/').count() == 1 && mm.split('/').count() == 1 {
                if let (Ok(d), Ok(m)) = (dd.trim().parse::<u32>(), mm.trim().parse::<u32>()) {
                    if let Ok(nt) = NaiveTime::parse_from_str(tpart.trim(), "%H:%M") {
                        if let Some(nd) = NaiveDate::from_ymd_opt(now_local.year(), m, d) {
                            let ndt = nd.and_time(nt);
                            if let Some(ldt) = Local.from_local_datetime(&ndt).single() {
                                let ts = ldt.timestamp();
                                if ts > now_ts {
                                    return Ok(ts);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Try `HH:MM` (today if in future, else tomorrow)
    for tfmt in ["%H:%M", "%H:%M:%S"] {
        if let Ok(nt) = NaiveTime::parse_from_str(s, tfmt) {
            let today = now_local.date_naive();
            let ndt = today.and_time(nt);
            if let Some(ldt) = Local.from_local_datetime(&ndt).single() {
                if ldt.timestamp() > now_ts {
                    return Ok(ldt.timestamp());
                }
            }
            if let Some(tomorrow) = today.succ_opt() {
                let ndt_tom = tomorrow.and_time(nt);
                if let Some(ldt_tom) = Local.from_local_datetime(&ndt_tom).single() {
                    return Ok(ldt_tom.timestamp());
                }
            }
        }
    }

    Err("Định dạng thời gian không hợp lệ. Hỗ trợ: HH:MM (vd 08:00), DD/MM/YYYY HH:MM, YYYY-MM-DD HH:MM, hoặc +30m / +2h".into())
}

/// Format a scheduled delivery timestamp in local civil time (`HH:MM · DD/MM/YYYY`).
pub fn format_schedule_time(deliver_at: i64) -> String {
    if let Some(dt) = Local.timestamp_opt(deliver_at, 0).single() {
        dt.format("%H:%M · %d/%m/%Y").to_string()
    } else {
        format!("{deliver_at}")
    }
}

/// Insert a new scheduled message into SQLite.
pub fn schedule_message(
    db_path: &str,
    thread_id: &str,
    thread_name: &str,
    is_group: bool,
    text: &str,
    deliver_at: i64,
    now_ts: i64,
) -> Result<ScheduledMessage> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Nội dung tin nhắn hẹn giờ không được để trống");
    }
    if thread_id.trim().is_empty() {
        anyhow::bail!("Thiếu định danh hội thoại (thread_id)");
    }
    if deliver_at <= now_ts {
        anyhow::bail!("Thời điểm phát tin phải lớn hơn thời gian hiện tại");
    }
    let conn = open_scheduler_db(db_path)?;
    conn.execute(
        "INSERT INTO scheduled_messages (thread_id, thread_name, is_group, text, deliver_at, created_at, status, error_msg)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', '')",
        params![
            thread_id,
            thread_name,
            if is_group { 1 } else { 0 },
            trimmed,
            deliver_at,
            now_ts,
        ],
    )?;
    let id = conn.last_insert_rowid();
    Ok(ScheduledMessage {
        id,
        thread_id: thread_id.to_string(),
        thread_name: thread_name.to_string(),
        is_group,
        text: trimmed.to_string(),
        deliver_at,
        created_at: now_ts,
        status: "pending".into(),
        error_msg: String::new(),
    })
}

/// List all pending (or failed) scheduled messages, optionally filtered by `thread_id`.
pub fn list_pending_scheduled(
    db_path: &str,
    thread_id: Option<&str>,
) -> Result<Vec<ScheduledMessage>> {
    let conn = open_scheduler_db(db_path)?;
    let mut out = Vec::new();
    if let Some(tid) = thread_id {
        let mut stmt = conn.prepare(
            "SELECT id, thread_id, thread_name, is_group, text, deliver_at, created_at, status, error_msg
             FROM scheduled_messages
             WHERE thread_id = ?1 AND status IN ('pending', 'sending', 'failed')
             ORDER BY deliver_at ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![tid], map_scheduled_row)?;
        for r in rows {
            out.push(r?);
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, thread_id, thread_name, is_group, text, deliver_at, created_at, status, error_msg
             FROM scheduled_messages
             WHERE status IN ('pending', 'sending', 'failed')
             ORDER BY deliver_at ASC, id ASC",
        )?;
        let rows = stmt.query_map([], map_scheduled_row)?;
        for r in rows {
            out.push(r?);
        }
    }
    Ok(out)
}

fn map_scheduled_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduledMessage> {
    let is_group_i: i64 = row.get(3).unwrap_or(0);
    Ok(ScheduledMessage {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        thread_name: row.get(2).unwrap_or_default(),
        is_group: is_group_i != 0,
        text: row.get(4)?,
        deliver_at: row.get(5)?,
        created_at: row.get(6).unwrap_or(0),
        status: row.get(7).unwrap_or_else(|_| "pending".into()),
        error_msg: row.get(8).unwrap_or_default(),
    })
}

/// Update text and/or delivery timestamp of a pending scheduled message (#38).
pub fn update_scheduled_message(
    db_path: &str,
    id: i64,
    new_text: &str,
    new_deliver_at: i64,
) -> Result<bool> {
    let trimmed = new_text.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Nội dung tin nhắn hẹn giờ không được để trống");
    }
    let conn = open_scheduler_db(db_path)?;
    let updated = conn.execute(
        "UPDATE scheduled_messages
         SET text = ?1, deliver_at = ?2, status = 'pending', error_msg = ''
         WHERE id = ?3 AND status IN ('pending', 'failed')",
        params![trimmed, new_deliver_at, id],
    )?;
    Ok(updated > 0)
}

/// Cancel and remove a scheduled message before it is sent (#38).
pub fn cancel_scheduled_message(db_path: &str, id: i64) -> Result<bool> {
    let conn = open_scheduler_db(db_path)?;
    let changed = conn.execute(
        "DELETE FROM scheduled_messages WHERE id = ?1 AND status != 'sent'",
        params![id],
    )?;
    Ok(changed > 0)
}

/// Atomically claim all scheduled messages whose `deliver_at <= now_ts` and `status = 'pending'`.
pub fn claim_due_scheduled_messages(db_path: &str, now_ts: i64) -> Result<Vec<ScheduledMessage>> {
    let conn = open_scheduler_db(db_path)?;
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let res = (|| -> Result<Vec<ScheduledMessage>> {
        let mut stmt = conn.prepare(
            "SELECT id, thread_id, thread_name, is_group, text, deliver_at, created_at, status, error_msg
             FROM scheduled_messages
             WHERE status = 'pending' AND deliver_at <= ?1
             ORDER BY deliver_at ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![now_ts], map_scheduled_row)?;
        let mut due = Vec::new();
        for r in rows {
            due.push(r?);
        }
        for item in &due {
            conn.execute(
                "UPDATE scheduled_messages SET status = 'sending' WHERE id = ?1",
                params![item.id],
            )?;
        }
        Ok(due)
    })();
    match res {
        Ok(due) => {
            conn.execute_batch("COMMIT")?;
            Ok(due)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Mark a claimed scheduled message as sent (removes it from active queue).
pub fn mark_scheduled_sent(db_path: &str, id: i64) -> Result<()> {
    let conn = open_scheduler_db(db_path)?;
    conn.execute(
        "DELETE FROM scheduled_messages WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Record failure on a scheduled message so the user can see the error and retry or edit.
pub fn mark_scheduled_failed(db_path: &str, id: i64, err: &str) -> Result<()> {
    let conn = open_scheduler_db(db_path)?;
    conn.execute(
        "UPDATE scheduled_messages SET status = 'failed', error_msg = ?1 WHERE id = ?2",
        params![err, id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_and_custom_input_parsing_produce_future_timestamps() {
        let now = 1_762_000_000_i64;
        assert_eq!(
            compute_preset_timestamp(now, SchedulePreset::In15Minutes),
            now + 900
        );
        assert_eq!(
            compute_preset_timestamp(now, SchedulePreset::In1Hour),
            now + 3600
        );
        assert!(compute_preset_timestamp(now, SchedulePreset::Today18h) > now);
        assert!(compute_preset_timestamp(now, SchedulePreset::Tomorrow08h) > now);

        assert_eq!(parse_custom_schedule_input("+30m", now).unwrap(), now + 1800);
        assert_eq!(parse_custom_schedule_input("+2h", now).unwrap(), now + 7200);
        assert!(parse_custom_schedule_input("08:30", now).unwrap() > now);
        assert!(parse_custom_schedule_input("-5m", now).is_err());
    }

    #[test]
    fn schedule_lifecycle_create_edit_claim_and_cancel() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ctdozlite-sched-{nonce}.db"));
        let db = path.to_str().unwrap();

        let now = 1_762_000_000_i64;
        let m1 = schedule_message(
            db,
            "u_100",
            "Nguyễn Văn A",
            false,
            "Báo cáo tiến độ sáng mai",
            now + 600,
            now,
        )
        .unwrap();
        let m2 = schedule_message(
            db,
            "g_200",
            "Nhóm Dự Án",
            true,
            "Họp lúc 9h",
            now + 1200,
            now,
        )
        .unwrap();

        let pending = list_pending_scheduled(db, None).unwrap();
        assert_eq!(pending.len(), 2);

        // Edit m1 text and time
        assert!(update_scheduled_message(db, m1.id, "Báo cáo đã cập nhật", now + 300).unwrap());
        let thread_pending = list_pending_scheduled(db, Some("u_100")).unwrap();
        assert_eq!(thread_pending.len(), 1);
        assert_eq!(thread_pending[0].text, "Báo cáo đã cập nhật");
        assert_eq!(thread_pending[0].deliver_at, now + 300);

        // Cancel m2
        assert!(cancel_scheduled_message(db, m2.id).unwrap());
        assert_eq!(list_pending_scheduled(db, None).unwrap().len(), 1);

        // Not due yet at now + 100
        assert!(claim_due_scheduled_messages(db, now + 100).unwrap().is_empty());

        // Due at now + 300
        let due = claim_due_scheduled_messages(db, now + 300).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, m1.id);

        mark_scheduled_sent(db, m1.id).unwrap();
        assert!(list_pending_scheduled(db, None).unwrap().is_empty());

        let _ = std::fs::remove_file(&path);
    }
}
