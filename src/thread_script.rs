//! Per-thread mini script workspace: user-written rhai scripts chạy theo trigger
//! (tay / tin mới / lịch / từ khóa), đọc-ghi dữ liệu riêng, gửi tin qua outbox.
//!
//! Sandbox: timeout 5 s (`on_progress`), giới hạn ~100k operations, KHÔNG cho
//! file/network/thread — chỉ API đã đăng ký. Gửi tin KHÔNG trực tiếp: script gọi
//! `send_queued(thread_id, text)`, worker drain outbox qua `WorkerCmd::SendMessage`.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use rhai::{Array, Dynamic, Engine, Map};

/// 1 script gắn với 1 hội thoại. Persist ở SQLite bảng `thread_scripts`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ThreadScript {
    pub id: i64,
    pub thread_id: String,
    pub name: String,
    pub code: String,
    pub trigger: String,
    pub trigger_arg: String,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 1 dòng log chạy script. Persist ở SQLite bảng `thread_script_logs`.
#[derive(Debug, Clone, Default)]
pub struct ScriptLog {
    pub thread_id: String,
    pub name: String,
    pub level: String,
    pub msg: String,
    pub ts: i64,
}

/// Sự kiện kích hoạt 1 lần chạy.
#[derive(Debug, Clone, Default)]
pub struct ScriptEvent {
    /// `manual` | `message` | `interval` | `test`.
    pub kind: String,
    pub msg_text: String,
    pub msg_sender: String,
}

impl ScriptEvent {
    pub fn manual() -> Self {
        Self { kind: "manual".into(), ..Default::default() }
    }
    pub fn test() -> Self {
        Self { kind: "test".into(), ..Default::default() }
    }
    pub fn interval() -> Self {
        Self { kind: "interval".into(), ..Default::default() }
    }
    pub fn message(text: &str, sender: &str) -> Self {
        Self { kind: "message".into(), msg_text: text.to_string(), msg_sender: sender.to_string() }
    }
}

/// Trigger đã parse từ cặp `(trigger, trigger_arg)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerKind {
    Manual,
    OnMessage,
    /// Lặp mỗi N phút.
    OnInterval(u64),
    /// Tin mới chứa từ khóa (case-insensitive).
    OnKeyword(String),
}

/// Parse trigger script. Chấp nhận cả 2 kiểu lưu:
/// `trigger` = `manual|on_message|on_interval_N|on_keyword:kw` hoặc
/// `trigger` = `on_interval|on_keyword` + `trigger_arg` = `N|kw`.
pub fn parse_trigger(trigger: &str, trigger_arg: &str) -> TriggerKind {
    let t = trigger.trim();
    if let Some(rest) = t.strip_prefix("on_interval") {
        let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
        let n: u64 = digits
            .parse()
            .ok()
            .filter(|n| *n > 0)
            .or_else(|| trigger_arg.trim().parse().ok().filter(|n: &u64| *n > 0))
            .unwrap_or(60);
        return TriggerKind::OnInterval(n.max(1));
    }
    if let Some(rest) = t.strip_prefix("on_keyword") {
        let mut kw = rest.trim_start_matches([':', '=', '_', ' ']).trim().to_string();
        if kw.is_empty() {
            kw = trigger_arg.trim().to_string();
        }
        if kw.is_empty() {
            return TriggerKind::Manual;
        }
        return TriggerKind::OnKeyword(kw);
    }
    match t {
        "on_message" => TriggerKind::OnMessage,
        _ => TriggerKind::Manual,
    }
}

/// Script có nên chạy với event tin nhắn tới không (manual/interval → false).
pub fn trigger_fires_on_message(kind: &TriggerKind, msg_text: &str) -> bool {
    match kind {
        TriggerKind::OnMessage => true,
        TriggerKind::OnKeyword(kw) => {
            let hay = msg_text.to_lowercase();
            !kw.is_empty() && hay.contains(&kw.to_lowercase())
        }
        _ => false,
    }
}

/// Script interval đã tới hạn chưa. `last_run_sec <= 0` = chưa chạy lần nào → tới hạn.
pub fn interval_due(last_run_sec: i64, every_min: u64, now_sec: i64) -> bool {
    if every_min == 0 {
        return false;
    }
    if last_run_sec <= 0 {
        return true;
    }
    now_sec.saturating_sub(last_run_sec) >= (every_min as i64).saturating_mul(60)
}

/// Key KV lưu mốc chạy gần nhất của 1 script interval.
pub fn last_run_key(script_id: i64) -> String {
    format!("__script_last_run_{script_id}")
}

/// Tin script muốn gửi (worker drain qua `WorkerCmd::SendMessage`, KHÔNG gửi trực tiếp).
#[derive(Debug, Clone)]
pub struct OutboxMsg {
    pub thread_id: String,
    pub text: String,
}

/// Kết quả 1 lần chạy: output (giá trị biểu thức cuối) + outbox + toast.
/// Log đã persist vào `thread_script_logs` trước khi trả về.
#[derive(Debug, Clone, Default)]
pub struct ScriptRunOutput {
    pub output: String,
    pub outbox: Vec<OutboxMsg>,
    pub toasts: Vec<String>,
}

const MAX_OPERATIONS: u64 = 100_000;
const TIMEOUT_SECS: u64 = 5;
const MAX_CODE_BYTES: usize = 50_000;
const MAX_OUTBOX_PER_RUN: usize = 5;
const MAX_TOAST_PER_RUN: usize = 5;

fn build_engine(
    db_path: String,
    threads: Arc<Vec<crate::chat::Thread>>,
    outbox: Arc<Mutex<Vec<OutboxMsg>>>,
    toasts: Arc<Mutex<Vec<String>>>,
    logs: Arc<Mutex<Vec<(String, String)>>>,
) -> Engine {
    let mut engine = Engine::new();
    // Không cho đọc file / module ngoài / eval động.
    engine.disable_symbol("import");
    engine.disable_symbol("eval");
    engine.set_max_operations(MAX_OPERATIONS);
    let start = Instant::now();
    engine.on_progress(move |_ops| {
        if start.elapsed() > std::time::Duration::from_secs(TIMEOUT_SECS) {
            Some("script timeout (5s)".into())
        } else {
            None
        }
    });
    {
        let logs = logs.clone();
        engine.on_print(move |s: &str| {
            if let Ok(mut l) = logs.lock() {
                l.push(("info".to_string(), s.to_string()));
            }
        });
    }
    // ── get_messages(thread_id, limit) -> array(map) ──
    {
        let db = db_path.clone();
        let threads = threads.clone();
        engine.register_fn(
            "get_messages",
            move |tid: &str, limit: i64| -> Array {
                let lim = limit.clamp(1, 200) as usize;
                let mut rows: Vec<(i64, String, String, String, bool)> = Vec::new();
                if let Ok(store) = crate::store::MessageStore::open(&db) {
                    if let Ok(all) = store.messages_for_all(tid) {
                        for m in all {
                            rows.push((m.ts, m.msg_id, m.sender_name, m.text, m.is_outgoing));
                        }
                    }
                }
                if let Some(t) = threads.iter().find(|t| t.id == tid) {
                    for m in &t.messages {
                        if m.id.is_empty() || !rows.iter().any(|(_, id, _, _, _)| *id == m.id) {
                            rows.push((m.ts, m.id.clone(), m.sender.clone(), m.text.clone(), m.is_outgoing));
                        }
                    }
                }
                rows.sort_by_key(|(ts, _, _, _, _)| *ts);
                rows.into_iter()
                    .rev()
                    .take(lim)
                    .rev()
                    .map(|(ts, id, sender, text, outgoing)| {
                        let mut map = Map::new();
                        map.insert("id".into(), Dynamic::from(id));
                        map.insert("sender".into(), Dynamic::from(sender));
                        map.insert("text".into(), Dynamic::from(text));
                        map.insert("ts".into(), Dynamic::from(ts));
                        map.insert("outgoing".into(), Dynamic::from(outgoing));
                        Dynamic::from(map)
                    })
                    .collect()
            },
        );
    }
    // ── get_note / set_note ──
    {
        let db = db_path.clone();
        engine.register_fn("get_note", move |tid: &str| -> String {
            crate::store::MessageStore::open(&db)
                .map(|s| s.get_group_note(tid))
                .unwrap_or_default()
        });
    }
    {
        let db = db_path.clone();
        engine.register_fn("set_note", move |tid: &str, note: &str| -> bool {
            let now = crate::login::unix_now();
            crate::store::MessageStore::open(&db)
                .and_then(|mut s| {
                    s.set_group_note(tid, note, now)?;
                    Ok(())
                })
                .map(|_| true)
                .unwrap_or(false)
        });
    }
    // ── kv_get / kv_set / kv_list ──
    {
        let db = db_path.clone();
        engine.register_fn("kv_get", move |tid: &str, key: &str| -> String {
            crate::store::MessageStore::open(&db)
                .ok()
                .and_then(|s| s.thread_kv_get(tid, key))
                .unwrap_or_default()
        });
    }
    {
        let db = db_path.clone();
        engine.register_fn("kv_set", move |tid: &str, key: &str, val: &str| -> bool {
            crate::store::MessageStore::open(&db)
                .and_then(|mut s| {
                    s.thread_kv_set(tid, key, val)?;
                    Ok(())
                })
                .map(|_| true)
                .unwrap_or(false)
        });
    }
    // ── now_day(): ngày local YYYY-MM-DD (chống spam chào theo ngày) ──
    engine.register_fn("now_day", || -> String {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    });

    // ── kv_list ──
    {
        let db = db_path.clone();
        engine.register_fn("kv_list", move |tid: &str| -> Array {
            crate::store::MessageStore::open(&db)
                .map(|s| {
                    s.thread_kv_list(tid)
                        .into_iter()
                        .map(Dynamic::from)
                        .collect()
                })
                .unwrap_or_default()
        });
    }
    // ── thread_stats(thread_id) -> map (tái dùng group_stats) ──
    {
        let db = db_path.clone();
        let threads = threads.clone();
        engine.register_fn("thread_stats", move |tid: &str| -> Map {
            let mut map = Map::new();
            let store = crate::store::MessageStore::open(&db).ok();
            let thread = threads.iter().find(|t| t.id == tid);
            let st = crate::group_stats::compute_group_stats(store.as_ref(), thread);
            map.insert("total".into(), Dynamic::from(st.total as i64));
            map.insert("last_7d".into(), Dynamic::from(st.last_7d as i64));
            map.insert("photos".into(), Dynamic::from(st.photos as i64));
            map.insert("files".into(), Dynamic::from(st.files as i64));
            map.insert("links".into(), Dynamic::from(st.links as i64));
            map.insert("voices".into(), Dynamic::from(st.voices as i64));
            map.insert(
                "peak_hour".into(),
                Dynamic::from(st.peak_hour.map(|h| h as i64).unwrap_or(-1)),
            );
            map.insert("peak_hour_count".into(), Dynamic::from(st.peak_hour_count as i64));
            map.insert(
                "top_senders".into(),
                Dynamic::from(
                    st.top_senders
                        .iter()
                        .map(|(n, c)| format!("{n} ({c})"))
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            );
            map
        });
    }
    // ── send_queued(thread_id, text): đẩy outbox, worker gửi ──
    {
        engine.register_fn(
            "send_queued",
            move |tid: &str, text: &str| -> bool {
                let t = text.trim();
                if t.is_empty() {
                    return false;
                }
                let short: String = t.chars().take(2000).collect();
                if let Ok(mut o) = outbox.lock() {
                    if o.len() < MAX_OUTBOX_PER_RUN {
                        o.push(OutboxMsg { thread_id: tid.to_string(), text: short });
                        return true;
                    }
                }
                false
            },
        );
    }
    // ── toast(msg) ──
    {
        engine.register_fn("toast", move |msg: &str| {
            if msg.trim().is_empty() {
                return;
            }
            if let Ok(mut t) = toasts.lock() {
                if t.len() < MAX_TOAST_PER_RUN {
                    t.push(msg.to_string());
                }
            }
        });
    }
    // ── log(msg): lưu thread_script_logs ──
    {
        let logs = logs.clone();
        engine.register_fn("log", move |msg: &str| {
            if let Ok(mut l) = logs.lock() {
                l.push(("info".to_string(), msg.to_string()));
            }
        });
    }
    engine
}

/// Chạy 1 script với sandbox (timeout 5 s, ~100k ops). Log persist vào DB.
/// Trả `output` = giá trị biểu thức cuối (rỗng nếu không có).
pub fn run_script(
    db_path: &str,
    threads: &[crate::chat::Thread],
    thread_id: &str,
    script: &ThreadScript,
    event: &ScriptEvent,
) -> anyhow::Result<ScriptRunOutput> {
    if script.code.trim().is_empty() {
        anyhow::bail!("script rỗng");
    }
    if script.code.len() > MAX_CODE_BYTES {
        anyhow::bail!("script quá dài (> 50KB)");
    }
    let _ = crate::store::MessageStore::open(db_path);
    let outbox: Arc<Mutex<Vec<OutboxMsg>>> = Arc::new(Mutex::new(Vec::new()));
    let toasts: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let logs: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let engine = build_engine(
        db_path.to_string(),
        Arc::new(threads.to_vec()),
        outbox.clone(),
        toasts.clone(),
        logs.clone(),
    );
    let mut scope = rhai::Scope::new();
    scope.push("thread_id", thread_id.to_string());
    scope.push("event_kind", event.kind.clone());
    scope.push("event_text", event.msg_text.clone());
    scope.push("event_sender", event.msg_sender.clone());
    let ast = engine
        .compile(&script.code)
        .map_err(|e| anyhow::anyhow!("lỗi biên dịch: {e}"))?;
    let ret: Dynamic = engine
        .eval_ast_with_scope(&mut scope, &ast)
        .map_err(|e| anyhow::anyhow!("lỗi chạy: {e}"))?;
    let output = if ret.is_unit() { String::new() } else { ret.to_string() };
    let out = ScriptRunOutput {
        output,
        outbox: outbox.lock().map(|o| o.clone()).unwrap_or_default(),
        toasts: toasts.lock().map(|t| t.clone()).unwrap_or_default(),
    };
    // Persist log (kể cả khi output rỗng): log API + 1 dòng output.
    if let Ok(store) = crate::store::MessageStore::open(db_path) {
        let mut store = store;
        if let Ok(l) = logs.lock() {
            for (level, msg) in l.iter() {
                let _ = store.push_script_log(thread_id, &script.name, level, msg);
            }
        }
        if !out.output.is_empty() {
            let _ = store.push_script_log(thread_id, &script.name, "info", &format!("→ {}", out.output));
        }
    }
    // Ghi mốc chạy cho script interval (worker dùng để tính tới hạn).
    if matches!(parse_trigger(&script.trigger, &script.trigger_arg), TriggerKind::OnInterval(_)) {
        if let Ok(mut store) = crate::store::MessageStore::open(db_path) {
            let now = crate::login::unix_now().to_string();
            let _ = store.thread_kv_set(thread_id, &last_run_key(script.id), &now);
        }
    }
    Ok(out)
}

/// Persist 1 dòng log lỗi khi script fail (gọi từ worker/UI trước khi hiện lỗi).
pub fn persist_error_log(db_path: &str, thread_id: &str, name: &str, err: &str) {
    if let Ok(mut store) = crate::store::MessageStore::open(db_path) {
        let _ = store.push_script_log(thread_id, name, "error", err);
    }
}

// ── Template kèm sẵn ─────────────────────────────────────────────

/// Tổng hợp ngày: đếm tin + top người gửi + từ khóa hot → ghi note.
pub const TEMPLATE_DIGEST: &str = r#"// tong-hop-ngay: tổng hợp hội thoại → ghi note (chạy tay hoặc theo lịch)
let tid = thread_id;
let msgs = get_messages(tid, 200);
let total = msgs.len();
let st = thread_stats(tid);
let stop = ["va", "la", "cua", "cho", "voi", "thi", "ma", "de", "do", "nay", "ban", "minh", "the", "and", "for"];
let counts = #{};
for m in msgs {
    let words = m["text"].split(" ");
    for w in words {
        let k = w.to_lower();
        k.trim();
        if k.len() > 3 && !stop.contains(k) {
            if counts.contains(k) { counts[k] += 1; } else { counts[k] = 1; }
        }
    }
}
let hot = "";
let best = 0;
for k in counts.keys() {
    if counts[k] > best { best = counts[k]; hot = k; }
}
let top = st["top_senders"];
let note = `📊 Tổng hợp (${total} tin, 7 ngày: ${st["last_7d"]})\n- Top gửi: ${top}\n- Từ khóa hot: ${hot} (${best})\n- Ảnh: ${st["photos"]}, file: ${st["files"]}, link: ${st["links"]}, voice: ${st["voices"]}`;
set_note(tid, note);
log(`tong-hop-ngay: ${total} tin, hot=${hot}`);
note
"#;

/// Quét tin chứa 'deadline/hạn' → toast nhắc.
pub const TEMPLATE_DEADLINE: &str = r#"// nhac-deadline: quét tin nhắc hạn → toast (chạy theo lịch on_interval_60)
let tid = thread_id;
let msgs = get_messages(tid, 100);
let hits = 0;
for m in msgs {
    let t = m["text"].to_lower();
    if t.contains("deadline") || t.contains("hạn") || t.contains("han chot") || t.contains("hạn chót") {
        hits += 1;
    }
}
if hits > 0 {
    toast(`⏰ Có ${hits} tin nhắc deadline/hạn trong hội thoại này`);
}
log(`nhac-deadline: ${hits} tin`);
hits
"#;

/// Chào thành viên mới: tin đầu của người lạ → gửi chào 1 lần/người/ngày (kv).
pub const TEMPLATE_WELCOME: &str = r#"// chao-thanh-vien-moi: chào người lạ 1 lần/ngày (trigger on_message)
if event_kind == "message" && event_sender != "" {
    let day = `d${now_day()}`;
    let key = `welcomed_${event_sender}_${day}`;
    if kv_get(thread_id, key) == "" {
        send_queued(thread_id, `Chào ${event_sender} 👋 Chào mừng bạn đến với nhóm!`);
        kv_set(thread_id, key, "1");
        log(`chao-thanh-vien-moi: đã chào ${event_sender}`);
    }
}
"ok"
"#;

/// Danh sách template kèm sẵn: (id, tên hiển thị, trigger mặc định, mô tả).
pub fn builtin_templates() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    vec![
        ("tong-hop-ngay", "📊 Tổng hợp ngày", "manual", "Đếm tin + top người gửi + từ khóa hot → ghi note"),
        ("nhac-deadline", "⏰ Nhắc deadline", "on_interval_60", "Quét tin chứa 'deadline/hạn' mỗi 60 phút → toast"),
        ("chao-thanh-vien-moi", "👋 Chào thành viên mới", "on_message", "Tin đầu của người lạ → gửi chào 1 lần/người/ngày"),
    ]
}

/// Lấy code template theo id (`tong-hop-ngay` | `nhac-deadline` | `chao-thanh-vien-moi`).
pub fn builtin_template_code(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "tong-hop-ngay" => Some((TEMPLATE_DIGEST, "manual")),
        "nhac-deadline" => Some((TEMPLATE_DEADLINE, "on_interval_60")),
        "chao-thanh-vien-moi" => Some((TEMPLATE_WELCOME, "on_message")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db(name: &str) -> String {
        let p = std::env::temp_dir().join(format!("ctdoz_script_test_{name}_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p.to_string_lossy().to_string()
    }

    fn mk_script(code: &str) -> ThreadScript {
        ThreadScript {
            id: 1,
            thread_id: "t1".into(),
            name: "test".into(),
            code: code.into(),
            trigger: "manual".into(),
            ..Default::default()
        }
    }

    fn seed_messages(db: &str) {
        let mut store = crate::store::MessageStore::open(db).unwrap();
        let now_ms = chrono::Local::now().timestamp_millis();
        for (i, (sender, text)) in
            [("An", "hello deadline tuần này"), ("Bình", "ok nhé"), ("An", "chốt hạn cuối tháng")].iter().enumerate()
        {
            store
                .upsert_message(&crate::store::StoredMessage {
                    msg_id: format!("m{i}"),
                    thread_id: "t1".into(),
                    sender_id: format!("u{i}"),
                    sender_name: sender.to_string(),
                    text: text.to_string(),
                    msg_type: "text".into(),
                    ts: now_ms - (i as i64) * 1000,
                    is_outgoing: false,
                    raw_json: "{}".into(),
                })
                .unwrap();
        }
    }

    #[test]
    fn sandbox_blocks_infinite_loop() {
        let db = test_db("loop");
        let s = mk_script("while true {}");
        let err = run_script(&db, &[], "t1", &s, &ScriptEvent::test()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("timeout") || msg.contains("operation") || msg.contains("limit"),
            "unexpected: {msg}"
        );
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn sandbox_blocks_import_and_eval() {
        let db = test_db("block");
        // `nonExistentFunction` thay vì import: import bị disable → lỗi biên dịch.
        let s = mk_script(r#"import "x" as y;"#);
        assert!(run_script(&db, &[], "t1", &s, &ScriptEvent::test()).is_err());
        let s2 = mk_script(r#"eval("40+2");"#);
        assert!(run_script(&db, &[], "t1", &s2, &ScriptEvent::test()).is_err());
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn kv_roundtrip_through_script() {
        let db = test_db("kv");
        let s = mk_script(r#"kv_set(thread_id, "k1", "v1"); kv_get(thread_id, "k1")"#);
        let out = run_script(&db, &[], "t1", &s, &ScriptEvent::test()).unwrap();
        assert_eq!(out.output, "v1");
        let store = crate::store::MessageStore::open(&db).unwrap();
        assert_eq!(store.thread_kv_get("t1", "k1").as_deref(), Some("v1"));
        assert_eq!(store.thread_kv_list("t1"), vec!["k1".to_string()]);
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn digest_template_on_sample_db() {
        let db = test_db("digest");
        seed_messages(&db);
        let s = ThreadScript {
            code: TEMPLATE_DIGEST.into(),
            ..mk_script("")
        };
        let out = run_script(&db, &[], "t1", &s, &ScriptEvent::test()).unwrap();
        assert!(out.output.contains("Tổng hợp"), "output: {}", out.output);
        let store = crate::store::MessageStore::open(&db).unwrap();
        assert!(store.get_group_note("t1").contains("Tổng hợp"));
        assert!(!store.list_script_logs("t1", 10).is_empty());
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn deadline_template_counts_hits() {
        let db = test_db("deadline");
        seed_messages(&db);
        let s = ThreadScript {
            code: TEMPLATE_DEADLINE.into(),
            ..mk_script("")
        };
        let out = run_script(&db, &[], "t1", &s, &ScriptEvent::test()).unwrap();
        assert_eq!(out.output, "2");
        assert_eq!(out.toasts.len(), 1);
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn welcome_template_only_once_per_day() {
        let db = test_db("welcome");
        let s = ThreadScript { code: TEMPLATE_WELCOME.into(), ..mk_script("") };

        let ev = ScriptEvent::message("hello", "Lạ");
        let out1 = run_script(&db, &[], "t1", &s, &ev).unwrap();
        assert_eq!(out1.outbox.len(), 1, "lần đầu phải chào");
        let out2 = run_script(&db, &[], "t1", &s, &ev).unwrap();
        assert!(out2.outbox.is_empty(), "lần 2 trong ngày không chào lại");
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn trigger_match_keyword_and_interval() {
        assert_eq!(parse_trigger("on_keyword:deadline", ""), TriggerKind::OnKeyword("deadline".into()));
        assert_eq!(parse_trigger("on_keyword", "han"), TriggerKind::OnKeyword("han".into()));
        assert_eq!(parse_trigger("on_interval_30", ""), TriggerKind::OnInterval(30));
        assert_eq!(parse_trigger("on_interval", "15"), TriggerKind::OnInterval(15));
        assert_eq!(parse_trigger("manual", ""), TriggerKind::Manual);
        assert!(trigger_fires_on_message(&TriggerKind::OnMessage, "bất kỳ"));
        assert!(trigger_fires_on_message(&TriggerKind::OnKeyword("dead".into()), "có DEADLINE nhé"));
        assert!(!trigger_fires_on_message(&TriggerKind::OnKeyword("dead".into()), "không có gì"));
        assert!(!trigger_fires_on_message(&TriggerKind::Manual, "deadline"));
        assert!(interval_due(0, 60, 1_000_000));
        assert!(!interval_due(1_000_000, 60, 1_000_000 + 100));
        assert!(interval_due(1_000_000, 60, 1_000_000 + 3600));
    }

    #[test]
    fn outbox_capped_and_no_direct_send() {
        let db = test_db("outbox");
        let s = mk_script(
            r#"for i in 0..20 { send_queued(thread_id, `hi ${i}`); } send_queued(thread_id, "x")"#,
        );
        let out = run_script(&db, &[], "t1", &s, &ScriptEvent::test()).unwrap();
        assert_eq!(out.outbox.len(), MAX_OUTBOX_PER_RUN);
        let _ = std::fs::remove_file(&db);
    }
}

