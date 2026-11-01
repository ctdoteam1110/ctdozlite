//! Structural trace for the message-sync investigation (`ZCALL_WS_TRACE=1`).
//!
//! The offline catch-up work (v0.1.2) has to see what the server returns — queue cursors,
//! `more`, `queueStatus`, `evict`, E2EE markers, control events — without copying message text
//! into a log file. [`shape`] renders a JSON value as its structure: key names, types and counts.
//! Values are printed only for a fixed list of cursor / status / type keys ([`VALUE_KEYS`]);
//! every other string is reduced to its length, so `content`, names and captions never reach
//! the trace. Object keys that look like user or group ids are folded into one `<id>×N` entry.

use serde_json::{Map, Value};

/// Keys whose scalar values are cursors, flags or type tags — never user text.
const VALUE_KEYS: &[&str] = &[
    "error_code",
    "error_message",
    "more",
    "hasMore",
    "lastActionId",
    "lastActionIdOther",
    "resetLastActionId",
    "lastId",
    "evict",
    "isLastQueue",
    "queueId",
    "cmd",
    "subCmd",
    "act",
    "act_type",
    "actType",
    "action",
    "eventType",
    "msgType",
    "e2eeStatus",
    "encrypt",
    "status",
    "st",
    "at",
    "ver",
    "version",
    "type",
    "src",
    "isGroup",
    "count",
    "total",
    "db_format",
    "format",
    "is_full_transfer",
    "user_action",
    "pc",
    "error",
    "isFiltered",
    "isFilteredByPhase",
    "isFilteredByTimeJoin",
    "isOld",
    "isRootDel",
];

/// Keys holding Unix timestamps; an array of objects shows them as `min..max`.
const TIME_KEYS: &[&str] = &["ts", "sendDttm", "serverTime", "createdTime", "tsJoinGroup"];

/// Keys whose arrays hold action ids worth seeing (`queueStatus[q].ids`, `preIds`).
const ID_LIST_KEYS: &[&str] = &["ids", "preIds"];

const MAX_DEPTH: usize = 8;
const MAX_LEN: usize = 6000;
const MAX_DISTINCT: usize = 6;
const MAX_VALUE_CHARS: usize = 40;
const MAX_LISTED_IDS: usize = 12;

/// `true` when `ZCALL_WS_TRACE=1`.
pub fn enabled() -> bool {
    std::env::var("ZCALL_WS_TRACE").ok().as_deref() == Some("1")
}

/// Print one trace line to stderr with a local timestamp, when tracing is on.
pub fn line(tag: &str, msg: impl std::fmt::Display) {
    if enabled() {
        eprintln!("{} [{tag}] {msg}", chrono::Local::now().format("%H:%M:%S%.3f"));
    }
}

/// Structure of `v` with message text removed (see the module docs).
pub fn shape(v: &Value) -> String {
    let mut out = String::new();
    render(v, 0, &mut out);
    if out.chars().count() > MAX_LEN {
        out = out.chars().take(MAX_LEN).collect();
        out.push('…');
    }
    out
}

fn render(v: &Value, depth: usize, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(_) => out.push_str("bool"),
        Value::Number(_) => out.push_str("num"),
        Value::String(s) => render_str(s, depth, out),
        Value::Array(a) => render_array(a, depth, out),
        Value::Object(o) => render_object(o, depth, out),
    }
}

/// Strings are reduced to their length, except embedded JSON (`data` of control events,
/// attachment `content`), which is rendered as structure too.
fn render_str(s: &str, depth: usize, out: &mut String) {
    let t = s.trim();
    if (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']')) {
        if let Ok(inner) = serde_json::from_str::<Value>(t) {
            out.push_str("json:");
            render(&inner, depth, out);
            return;
        }
    }
    out.push_str(&format!("str({})", s.chars().count()));
}

fn render_object(o: &Map<String, Value>, depth: usize, out: &mut String) {
    if o.is_empty() {
        out.push_str("{}");
        return;
    }
    if depth >= MAX_DEPTH {
        out.push_str("{…}");
        return;
    }
    let mut parts: Vec<String> = Vec::new();
    let mut id_keys = 0usize;
    let mut first_id_value: Option<&Value> = None;
    for (k, v) in o {
        if id_like(k) {
            id_keys += 1;
            first_id_value.get_or_insert(v);
            continue;
        }
        parts.push(render_entry(k, v, depth));
    }
    if let Some(v) = first_id_value {
        let mut s = format!("<id>×{id_keys}:");
        render(v, depth + 1, &mut s);
        parts.push(s);
    }
    out.push('{');
    out.push_str(&parts.join(", "));
    out.push('}');
}

fn render_entry(k: &str, v: &Value, depth: usize) -> String {
    if VALUE_KEYS.contains(&k) || TIME_KEYS.contains(&k) {
        if let Some(text) = scalar_text(v) {
            return format!("{k}={text}");
        }
    }
    if ID_LIST_KEYS.contains(&k) {
        if let Some(list) = id_list(v) {
            return format!("{k}={list}");
        }
    }
    let mut s = format!("{k}:");
    render(v, depth + 1, &mut s);
    s
}

fn render_array(a: &[Value], depth: usize, out: &mut String) {
    if a.is_empty() {
        out.push_str("[]");
        return;
    }
    if depth >= MAX_DEPTH {
        out.push_str(&format!("[{}×…]", a.len()));
        return;
    }
    if a.iter().all(Value::is_object) {
        out.push_str(&format!("[{}× ", a.len()));
        render_merged(a, depth, out);
        out.push(']');
        return;
    }
    let mut kinds: Vec<&str> = Vec::new();
    for v in a {
        let kind = match v {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Number(_) => "num",
            Value::String(_) => "str",
            Value::Array(_) => "arr",
            Value::Object(_) => "obj",
        };
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    out.push_str(&format!("[{}× {}]", a.len(), kinds.join("|")));
}

/// One merged view of an array of objects: the union of keys (with how many elements carry
/// each key when not all do), a histogram for value keys, `min..max` for time keys, and the
/// structure of the first value otherwise.
fn render_merged(a: &[Value], depth: usize, out: &mut String) {
    let mut keys: Vec<&str> = Vec::new();
    for v in a {
        if let Some(o) = v.as_object() {
            for k in o.keys() {
                if !keys.contains(&k.as_str()) {
                    keys.push(k.as_str());
                }
            }
        }
    }
    let n = a.len();
    let mut parts: Vec<String> = Vec::new();
    for k in keys {
        let present: Vec<&Value> = a.iter().filter_map(|v| v.get(k)).collect();
        let label = if present.len() == n {
            k.to_string()
        } else {
            format!("{k}[{}/{n}]", present.len())
        };
        if TIME_KEYS.contains(&k) {
            if let Some(range) = time_range(&present) {
                parts.push(format!("{label}={range}"));
                continue;
            }
        }
        if VALUE_KEYS.contains(&k) && present.iter().all(|v| scalar_text(v).is_some()) {
            parts.push(format!("{label}={}", histogram(&present)));
            continue;
        }
        let first = present.iter().find(|v| !v.is_null()).or(present.first());
        let mut s = format!("{label}:");
        match first {
            Some(v) => render(v, depth + 1, &mut s),
            None => s.push_str("null"),
        }
        parts.push(s);
    }
    out.push('{');
    out.push_str(&parts.join(", "));
    out.push('}');
}

fn histogram(values: &[&Value]) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for v in values {
        let text = scalar_text(v).unwrap_or_default();
        match counts.iter_mut().find(|(t, _)| *t == text) {
            Some(entry) => entry.1 += 1,
            None => counts.push((text, 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut parts: Vec<String> = counts
        .iter()
        .take(MAX_DISTINCT)
        .map(|(t, c)| format!("{t}×{c}"))
        .collect();
    if counts.len() > MAX_DISTINCT {
        parts.push(format!("+{} khác", counts.len() - MAX_DISTINCT));
    }
    parts.join("|")
}

fn time_range(values: &[&Value]) -> Option<String> {
    let nums: Vec<i64> = values.iter().filter_map(|v| as_i64(v)).collect();
    let min = nums.iter().min()?;
    let max = nums.iter().max()?;
    Some(if min == max {
        min.to_string()
    } else {
        format!("{min}..{max}")
    })
}

fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Printable form of a scalar; long strings are reduced to their length.
fn scalar_text(v: &Value) -> Option<String> {
    match v {
        Value::Null => Some("null".into()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) if s.chars().count() <= MAX_VALUE_CHARS => Some(format!("{s:?}")),
        Value::String(s) => Some(format!("str({})", s.chars().count())),
        _ => None,
    }
}

fn id_list(v: &Value) -> Option<String> {
    let a = v.as_array()?;
    let items: Vec<String> = a
        .iter()
        .map(|x| match x {
            Value::String(s) if s.len() <= 24 => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let mut shown: Vec<String> = items.iter().take(MAX_LISTED_IDS).cloned().collect();
    if items.len() > MAX_LISTED_IDS {
        shown.push(format!("+{}", items.len() - MAX_LISTED_IDS));
    }
    Some(format!("[{}]", shown.join(",")))
}

/// User / group ids used as object keys (`"1234567890123"`, `"g4567890123"`).
fn id_like(k: &str) -> bool {
    let digits = k.strip_prefix('g').unwrap_or(k);
    digits.len() >= 6 && digits.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn message_text_never_reaches_the_trace() {
        let v = json!({ "data": { "msgs": [
            { "content": "mật khẩu ngân hàng 123456", "dName": "Nguyễn Văn A", "msgType": "webchat",
              "e2eeStatus": 0, "ts": "1730000000000" },
            { "content": { "title": "ảnh riêng tư", "href": "https://f.example/x.jpg" },
              "dName": "B", "msgType": "chat.photo", "ts": "1730000005000" }
        ] } });
        let s = shape(&v);
        for secret in ["mật khẩu", "123456", "Nguyễn", "riêng tư", "f.example"] {
            assert!(!s.contains(secret), "leaked {secret:?} in {s}");
        }
        assert!(s.contains("[2× "), "{s}");
        assert!(s.contains(r#"msgType="chat.photo"×1|"webchat"×1"#), "{s}");
        assert!(s.contains("e2eeStatus[1/2]=0×1"), "{s}");
        assert!(s.contains("ts=1730000000000..1730000005000"), "{s}");
        assert!(s.contains("content:str(25)"), "{s}");
    }

    #[test]
    fn queue_cursors_are_shown() {
        let v = json!({ "data": { "more": 1, "lastActionId": "8812",
            "queueStatus": { "510_0": { "lastId": "8812", "ids": ["8801", "8812"], "evict": false } } } });
        let s = shape(&v);
        assert!(s.contains("more=1"), "{s}");
        assert!(s.contains(r#"lastActionId="8812""#), "{s}");
        assert!(s.contains("510_0:{"), "{s}");
        for part in [r#"lastId="8812""#, "ids=[8801,8812]", "evict=false"] {
            assert!(s.contains(part), "missing {part} in {s}");
        }
    }

    #[test]
    fn id_keys_are_folded() {
        let v = json!({ "1234567890": { "lastMsgId": "9" }, "g9876543210": { "lastMsgId": "8" }, "ok": true });
        let s = shape(&v);
        assert!(!s.contains("1234567890") && !s.contains("9876543210"), "{s}");
        assert!(s.contains("<id>×2:{lastMsgId:str(1)}"), "{s}");
        assert!(s.contains("ok:bool"), "{s}");
    }

    #[test]
    fn embedded_json_is_rendered_as_structure() {
        let v = json!({ "controls": [ { "content": { "act_type": "voip", "act": "request",
            "data": "{\"callId\":1,\"note\":\"bí mật\"}" } } ] });
        let s = shape(&v);
        assert!(s.contains(r#"act_type="voip""#), "{s}");
        assert!(s.contains("data:json:{callId:num, note:str(6)}"), "{s}");
        assert!(!s.contains("bí mật"), "{s}");
    }

    #[test]
    fn long_values_and_output_are_capped() {
        let v = json!({ "error_message": "x".repeat(200) });
        assert_eq!(shape(&v), "{error_message=str(200)}");

        let big: Vec<Value> = (0..2000).map(|i| json!({ format!("k{i}"): i })).collect();
        let s = shape(&Value::Array(big));
        assert!(s.ends_with('…'));
        assert!(s.chars().count() <= MAX_LEN + 1);
    }
}
