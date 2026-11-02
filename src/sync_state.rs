//! Persistent sync cursors and catch-up helpers (v0.1.2).
//!
//! Evidence (`re-notes/10_bang_chung_dong_bo.md`, real account): the server keeps every action
//! since login in the per-session offline queues (`510_1`, `511_1`, `603_1`, …) and pulling does
//! not remove anything, so a client that always asks from `lastId: 1` downloads the whole queue
//! again on every start. Zalo PC keeps `lastActionId` per queue in its DB (`init_lastid_lp`,
//! `init_lastid_ctrl_sock`); QTzalo now keeps the same cursors per account in
//! `<data>/sync_state_<uin>.json`:
//!
//! * `ws_queues` — socket queues by name (`"510_1"`), updated by `socket.rs` from every
//!   `queueStatus` it sees;
//! * `poll` — the HTTP long-poll cursor (`getChatFromPolling`).
//!
//! The file holds cursors only (no keys, no message text) and is written with
//! [`crate::paths::write_private`] (atomic, owner-only).
//!
//! Also here, as pure functions: queue-gap signals ([`gap_signals`]) and the `msgId` → time
//! estimate used to order conversations like the phone does ([`MsgIdClock`]).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chat::{PollState, QueueCursor};

const VERSION: u32 = 1;
/// `queueStatus[q].ids` / `preIds` kept per queue (Zalo PC keeps at most 10).
const MAX_PRE_IDS: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SyncState {
    pub version: u32,
    pub uin: String,
    pub poll: PollState,
    #[serde(default)]
    pub ws_queues: BTreeMap<String, QueueCursor>,
    #[serde(default)]
    pub updated_at_ms: i64,
}

impl SyncState {
    pub fn new(uin: &str) -> Self {
        Self {
            version: VERSION,
            uin: uin.to_string(),
            poll: PollState::default(),
            ws_queues: BTreeMap::new(),
            updated_at_ms: 0,
        }
    }

    /// Saved state for `uin`, or a fresh one when the file is missing, unreadable, from another
    /// account or from a newer format. Never fails: a bad file only costs a full re-pull.
    pub fn load(uin: &str) -> Self {
        match crate::paths::sync_state_file(uin) {
            Some(path) => Self::load_from(&path, uin),
            None => Self::new(uin),
        }
    }

    pub fn load_from(path: &Path, uin: &str) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::new(uin);
        };
        match serde_json::from_str::<SyncState>(&text) {
            Ok(mut st) if st.uin == uin && st.version <= VERSION => {
                // Queues configured for this run (e.g. `QTZALO_LP_QUEUES`) that the file lacks.
                for (qid, cur) in PollState::default().queues {
                    st.poll.queues.entry(qid).or_insert(cur);
                }
                st
            }
            Ok(_) => Self::new(uin),
            Err(e) => {
                crate::logger::log_error("sync_state::load", &e.to_string());
                Self::new(uin)
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        match crate::paths::sync_state_file(&self.uin) {
            Some(path) => self.save_to(&path),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "uin không hợp lệ",
            )),
        }
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        crate::paths::write_private(path, json)
    }
}

/// `uin` as a file-name component: ASCII digits only, 1–32 of them; anything else is refused.
pub fn sanitize_uin(uin: &str) -> Option<&str> {
    let u = uin.trim();
    (!u.is_empty() && u.len() <= 32 && u.bytes().all(|b| b.is_ascii_digit())).then_some(u)
}

fn as_id(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn id_num(s: &str) -> u128 {
    s.parse().unwrap_or(0)
}

/// `"510_1"`-style socket queue names.
fn is_socket_queue_name(k: &str) -> bool {
    match k.split_once('_') {
        Some((cmd, sub)) => {
            !cmd.is_empty()
                && !sub.is_empty()
                && cmd.len() <= 4
                && sub.len() <= 2
                && cmd.bytes().all(|b| b.is_ascii_digit())
                && sub.bytes().all(|b| b.is_ascii_digit())
        }
        None => false,
    }
}

/// The `data` object of a decrypted frame or long-poll reply (`{data: {...}}` or bare).
fn data_node(payload: &Value) -> &Value {
    let d = payload.get("data").unwrap_or(payload);
    match d.get("data") {
        Some(inner) if inner.is_object() => inner,
        _ => d,
    }
}

/// Fold every socket-queue `queueStatus` entry of a frame into `cursors`. A cursor only moves
/// forward; `pre_ids` keeps the newest [`MAX_PRE_IDS`] ids at or below it. Returns `true` when
/// something changed.
pub fn merge_ws_queue_status(cursors: &mut BTreeMap<String, QueueCursor>, payload: &Value) -> bool {
    let Some(qs) = data_node(payload).get("queueStatus").and_then(Value::as_object) else {
        return false;
    };
    let mut changed = false;
    for (name, q) in qs {
        if !is_socket_queue_name(name) {
            continue;
        }
        let Some(last) = q.get("lastId").and_then(as_id).filter(|s| id_num(s) > 0) else {
            continue;
        };
        let entry = cursors.entry(name.clone()).or_insert_with(|| QueueCursor {
            last_id: "1".into(),
            ids: Vec::new(),
        });
        if id_num(&last) < id_num(&entry.last_id) {
            continue;
        }
        let limit = id_num(&last);
        let mut ids: Vec<String> = q
            .get("ids")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(as_id).filter(|s| id_num(s) <= limit).collect())
            .unwrap_or_default();
        ids.sort_by_key(|s| id_num(s));
        ids.dedup();
        if ids.len() > MAX_PRE_IDS {
            ids.drain(..ids.len() - MAX_PRE_IDS);
        }
        if entry.last_id != last || (!ids.is_empty() && entry.ids != ids) {
            entry.last_id = last;
            if !ids.is_empty() {
                entry.ids = ids;
            }
            changed = true;
        }
    }
    changed
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !matches!(s.trim(), "" | "0" | "false"),
        _ => false,
    }
}

/// Signs that the server dropped part of a queue: `queueStatus[q].evict` set, or a non-zero
/// `resetLastActionId` (cursor too old). Each entry names the cause (`"evict:510_1"`,
/// `"reset"`). Empty when the reply is complete.
pub fn gap_signals(payload: &Value) -> Vec<String> {
    let d = data_node(payload);
    let mut out = Vec::new();
    if let Some(qs) = d.get("queueStatus").and_then(Value::as_object) {
        for (name, q) in qs {
            if q.get("evict").map(truthy).unwrap_or(false) {
                out.push(format!("evict:{name}"));
            }
        }
    }
    if let Some(r) = d.get("resetLastActionId").and_then(as_id) {
        if id_num(&r) > 0 {
            out.push("reset".into());
        }
    }
    out
}

/// Queues whose loss means lost messages (not reactions or controls): socket `51x_*` / `71x_*`
/// and long-poll `1, 6, 15, 16` (`evict_broadcast_queues.lp_msg` plus the `_1` shards seen on
/// the real account).
pub fn is_message_queue(name: &str) -> bool {
    matches!(name, "1" | "6" | "15" | "16")
        || (is_socket_queue_name(name) && (name.starts_with("51") || name.starts_with("71")))
}

/// [`gap_signals`] restricted to message queues.
pub fn message_gap_signals(payload: &Value) -> Vec<String> {
    gap_signals(payload)
        .into_iter()
        .filter(|s| s == "reset" || s.strip_prefix("evict:").map(is_message_queue).unwrap_or(false))
        .collect()
}

static GAPS_DIRTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// When `payload` says the server dropped messages, record a missing range in the history DB:
/// from the newest message we have (we are complete up to it) to now. Returns `true` when a
/// range was recorded; the GUI learns about it through [`take_new_gaps`].
pub fn record_gaps_from(payload: &Value) -> bool {
    let signals = message_gap_signals(payload);
    if signals.is_empty() {
        return false;
    }
    let Ok(store) = crate::store::MessageStore::open(crate::paths::history_db()) else {
        return false;
    };
    let from = store.newest_message_ts().ok().flatten().unwrap_or(0);
    let to = crate::timefmt::now_ms();
    crate::trace_shape::line("sync", format!("gap {signals:?} from={from} to={to}"));
    match store.add_missing_range("*", from, to, &signals.join(",")) {
        Ok(_) => {
            GAPS_DIRTY.store(true, std::sync::atomic::Ordering::SeqCst);
            true
        }
        Err(e) => {
            crate::logger::log_error("sync_state::record_gaps_from", &e.to_string());
            false
        }
    }
}

/// `true` once after a gap was recorded since the last call.
pub fn take_new_gaps() -> bool {
    GAPS_DIRTY.swap(false, std::sync::atomic::Ordering::SeqCst)
}

/// Estimated last-activity time (ms) of every conversation seen in `get-last-msgs`, from its
/// `lastMsgId` and a [`MsgIdClock`] fitted on stored messages. Without enough stored messages
/// to fit the clock, conversations keep their relative order with tiny placeholder times
/// (below any real timestamp). Estimates are not capped at `now`, so their order (= id order)
/// is never flattened.
pub fn load_conversation_activity(
    store: &crate::store::MessageStore,
) -> std::collections::HashMap<String, i64> {
    let rows = store.thread_activity().unwrap_or_default();
    let mut out = std::collections::HashMap::with_capacity(rows.len());
    if rows.is_empty() {
        return out;
    }
    let pairs = store.msg_id_ts_pairs(2000).unwrap_or_default();
    match MsgIdClock::fit(&pairs) {
        Some(clock) => {
            for (tid, last) in rows {
                if let Ok(id) = last.parse::<u64>() {
                    out.insert(tid, clock.estimate_ms(id).max(1));
                }
            }
        }
        None => {
            let mut ids: Vec<(u64, String)> = rows
                .into_iter()
                .filter_map(|(tid, last)| last.parse::<u64>().ok().map(|n| (n, tid)))
                .collect();
            ids.sort();
            for (rank, (_, tid)) in ids.into_iter().enumerate() {
                out.insert(tid, rank as i64 + 1);
            }
        }
    }
    out
}

/// Message-id clock: Zalo `msgId`s grow with server time (measured on a real account: ~41 ids
/// per ms from 03/2026 to 11/2026, one inversion in 75 messages, 22 ms apart). Built from
/// `(msgId, ts_ms)` pairs of stored messages, it turns a conversation's `lastMsgId` (from
/// `get-last-msgs`) into an approximate time of its last activity.
///
/// The id rate varies over a day, so one straight line over months is hours off near the
/// ends; the clock interpolates between neighbouring known messages instead and only
/// extrapolates (with the overall rate) outside them. It is monotonic, so ordering by the
/// estimate is the same as ordering by id.
#[derive(Debug, Clone, PartialEq)]
pub struct MsgIdClock {
    /// Anchors sorted by id, with strictly increasing id and non-decreasing ts.
    anchors: Vec<(f64, f64)>,
    /// Overall rate (ids per millisecond) between the first and last anchor.
    ids_per_ms: f64,
}

impl MsgIdClock {
    /// Needs at least two usable pairs spanning ≥ 1 minute with ids growing over time;
    /// `None` otherwise (callers then fall back to ordering by id).
    pub fn fit(pairs: &[(u64, i64)]) -> Option<Self> {
        let mut pts: Vec<(f64, f64)> = pairs
            .iter()
            .filter(|(id, ts)| *id > 0 && *ts > 0)
            .map(|(id, ts)| (*id as f64, *ts as f64))
            .collect();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Keep a monotonic chain: drop duplicates and points whose time goes backwards.
        let mut anchors: Vec<(f64, f64)> = Vec::with_capacity(pts.len());
        for p in pts {
            match anchors.last() {
                Some(&(id, ts)) if p.0 <= id || p.1 < ts => {}
                _ => anchors.push(p),
            }
        }
        let (&(id_a, ts_a), &(id_b, ts_b)) = (anchors.first()?, anchors.last()?);
        if anchors.len() < 2 || ts_b - ts_a < 60_000.0 {
            return None;
        }
        let ids_per_ms = (id_b - id_a) / (ts_b - ts_a);
        (ids_per_ms.is_finite() && ids_per_ms > 0.0).then_some(Self { anchors, ids_per_ms })
    }

    /// Estimated server time (ms) of a message id.
    pub fn estimate_ms(&self, msg_id: u64) -> i64 {
        let id = msg_id as f64;
        let first = self.anchors[0];
        let last = self.anchors[self.anchors.len() - 1];
        let ts = if id <= first.0 {
            first.1 - (first.0 - id) / self.ids_per_ms
        } else if id >= last.0 {
            last.1 + (id - last.0) / self.ids_per_ms
        } else {
            let i = self.anchors.partition_point(|a| a.0 <= id);
            let (a, b) = (self.anchors[i - 1], self.anchors[i]);
            a.1 + (id - a.0) * (b.1 - a.1) / (b.0 - a.0)
        };
        ts.round() as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("qtzalo_sync_state_{}_{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("sync_state_123.json")
    }

    #[test]
    fn save_then_load_roundtrips() {
        let path = tmp("roundtrip");
        let mut st = SyncState::new("123");
        st.poll.last_action_id = "14489176548854".into();
        st.ws_queues.insert(
            "510_1".into(),
            QueueCursor { last_id: "8440948595701".into(), ids: vec!["8440948595700".into()] },
        );
        st.updated_at_ms = 42;
        st.save_to(&path).unwrap();
        assert_eq!(SyncState::load_from(&path, "123"), st);
    }

    #[test]
    fn bad_or_foreign_files_give_a_fresh_state() {
        let path = tmp("bad");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(SyncState::load_from(&path, "123"), SyncState::new("123"));

        let mut other = SyncState::new("999");
        other.poll.last_action_id = "77".into();
        other.save_to(&path).unwrap();
        assert_eq!(SyncState::load_from(&path, "123"), SyncState::new("123"));

        let missing = path.with_file_name("does_not_exist.json");
        assert_eq!(SyncState::load_from(&missing, "123"), SyncState::new("123"));
    }

    #[test]
    fn uin_must_be_digits() {
        assert_eq!(sanitize_uin(" 12345 "), Some("12345"));
        for bad in ["", "../1", "12a", "1/2", "１２３", &"9".repeat(33)] {
            assert_eq!(sanitize_uin(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn queue_status_moves_cursors_forward_only() {
        let mut c = BTreeMap::new();
        let frame = json!({ "data": { "more": 0, "queueStatus": {
            "510_1": { "lastId": "8440948595701", "ids": ["8440948595690", "8440948595701", "8440948599999"], "evict": 0 },
            "1": { "lastId": "14489134894670" }
        } } });
        assert!(merge_ws_queue_status(&mut c, &frame));
        assert_eq!(c.len(), 1, "long-poll queue ids are not socket queues");
        assert_eq!(c["510_1"].last_id, "8440948595701");
        assert_eq!(c["510_1"].ids, vec!["8440948595690", "8440948595701"]);

        let older = json!({ "data": { "queueStatus": { "510_1": { "lastId": "100", "ids": [] } } } });
        assert!(!merge_ws_queue_status(&mut c, &older));
        assert_eq!(c["510_1"].last_id, "8440948595701");

        assert!(!merge_ws_queue_status(&mut c, &frame), "same status is not a change");
    }

    #[test]
    fn pre_ids_are_capped() {
        let mut c = BTreeMap::new();
        let ids: Vec<String> = (1..=25).map(|i| i.to_string()).collect();
        let frame = json!({ "data": { "queueStatus": { "511_1": { "lastId": "25", "ids": ids } } } });
        merge_ws_queue_status(&mut c, &frame);
        assert_eq!(c["511_1"].ids.len(), MAX_PRE_IDS);
        assert_eq!(c["511_1"].ids.first().map(String::as_str), Some("16"));
    }

    #[test]
    fn gap_signals_detect_evict_and_reset() {
        let ok = json!({ "data": { "queueStatus": { "510_1": { "evict": 0 }, "1": { "evict": false } }, "resetLastActionId": "0" } });
        assert!(gap_signals(&ok).is_empty());
        let bad = json!({ "data": { "queueStatus": { "510_1": { "evict": 1 }, "7": { "evict": true } }, "resetLastActionId": "9000" } });
        let mut s = gap_signals(&bad);
        s.sort();
        assert_eq!(s, vec!["evict:510_1", "evict:7", "reset"]);
    }

    #[test]
    fn only_message_queues_raise_gaps() {
        assert!(is_message_queue("510_1") && is_message_queue("511_0") && is_message_queue("713_0"));
        assert!(is_message_queue("1") && is_message_queue("16"));
        assert!(!is_message_queue("603_1") && !is_message_queue("610_1") && !is_message_queue("4"));
        let ctrl = json!({ "data": { "queueStatus": { "603_1": { "evict": 1 }, "19": { "evict": 1 } } } });
        assert!(message_gap_signals(&ctrl).is_empty());
        let msg = json!({ "data": { "queueStatus": { "510_1": { "evict": 1 } } } });
        assert_eq!(message_gap_signals(&msg), vec!["evict:510_1"]);
    }

    #[test]
    fn msg_id_clock_interpolates_on_a_line() {
        // Anchors shaped like the measured data (~41 ids/ms), plus jitter of a few ms.
        let t0 = 1_774_875_484_698i64;
        let id0 = 7_674_131_260_947u64;
        let pairs: Vec<(u64, i64)> = (0..20)
            .map(|k| {
                let t = t0 + k * 86_400_000 * 10; // every 10 days
                let id = id0 + (k as u64) * 86_400_000 * 10 * 41;
                (id, t + (k % 3) * 7)
            })
            .collect();
        let clock = MsgIdClock::fit(&pairs).expect("fit");
        let mid_t = t0 + 95 * 86_400_000;
        let mid_id = id0 + 95 * 86_400_000 * 41;
        let est = clock.estimate_ms(mid_id);
        assert!((est - mid_t).abs() < 1_000, "estimate off by {} ms", est - mid_t);
        assert!(clock.estimate_ms(8_000_000_000_000) < clock.estimate_ms(8_400_000_000_000));
    }

    #[test]
    fn msg_id_clock_is_exact_at_anchors_and_follows_local_rate() {
        // Rate changes: 10 ids/ms for the first hour, 50 ids/ms for the second.
        let pairs = [(1_000_000u64, 0i64), (37_000_000, 3_600_000), (217_000_000, 7_200_000)];
        let clock = MsgIdClock::fit(&[(pairs[0].0, 1), (pairs[1].0, 3_600_001), (pairs[2].0, 7_200_001)]).unwrap();
        assert_eq!(clock.estimate_ms(37_000_000), 3_600_001);
        // Halfway through the fast hour by id is halfway in time, not what one line would say.
        assert_eq!(clock.estimate_ms(127_000_000), 5_400_001);
        // Outside the anchors: overall rate, still monotonic.
        assert!(clock.estimate_ms(300_000_000) > clock.estimate_ms(217_000_000));
        assert!(clock.estimate_ms(500_000) < 1);
        // Out-of-order and duplicate pairs are ignored.
        let messy = MsgIdClock::fit(&[(10, 1_000), (20, 500), (20, 900), (30, 200_000)]).unwrap();
        assert_eq!(messy.estimate_ms(10), 1_000);
        assert_eq!(messy.estimate_ms(30), 200_000);
    }

    #[test]
    fn msg_id_clock_needs_spread() {
        assert!(MsgIdClock::fit(&[]).is_none());
        assert!(MsgIdClock::fit(&[(10, 1000)]).is_none());
        assert!(MsgIdClock::fit(&[(10, 1000), (20, 2000)]).is_none(), "< 1 minute apart");
        assert!(MsgIdClock::fit(&[(20, 1000), (10, 900_000)]).is_none(), "negative slope");
    }
}
