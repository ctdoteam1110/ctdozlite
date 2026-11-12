//! Thống kê nhóm cho Group Workspace: tính từ DB (SQLite) + RAM (thread đang mở).
//!
//! Rẻ: 1 query COUNT + GROUP BY cho DB, còn lại duyệt RAM. Không đoán —
//! thiếu dữ liệu thì số 0 / chuỗi rỗng.

use std::collections::HashMap;

use crate::chat::Thread;
use crate::message_model::MessageKind;
use crate::store::MessageStore;

/// Thống kê 1 thread nhóm.
#[derive(Debug, Clone, Default)]
pub struct GroupStats {
    /// Tổng số tin (DB + RAM, khử trùng theo id).
    pub total: usize,
    /// Số tin 7 ngày gần nhất.
    pub last_7d: usize,
    /// Phân phối 7 ngày (cũ → mới, 7 bucket).
    pub daily: [usize; 7],
    /// Top 5 người gửi (tên, số tin).
    pub top_senders: Vec<(String, usize)>,
    /// Đếm media: ảnh, file, link, voice.
    pub photos: usize,
    pub files: usize,
    pub links: usize,
    pub voices: usize,
    /// Giờ sôi động nhất 0–23h (giờ địa phương). `None` khi chưa có tin nào.
    pub peak_hour: Option<u8>,
    /// Số tin trong giờ sôi động nhất.
    pub peak_hour_count: usize,
}

fn classify(kind: MessageKind, link_src: Option<&str>, text: &str) -> &'static str {
    match kind {
        MessageKind::Photo => "photo",
        MessageKind::Video => "photo",
        MessageKind::File => "file",
        MessageKind::Voice => "voice",
        MessageKind::Link | MessageKind::WebContent => "link",
        _ => {
            let src = link_src.unwrap_or("");
            let t = text.trim();
            if src == "Hình ảnh Zalo"
                || t == "[Hình ảnh]"
                || t.starts_with("[Hình ảnh] ")
            {
                "photo"
            } else if src == "Tập tin Zalo" || t.starts_with("[Tập tin]") {
                "file"
            } else if src == "Thoại Zalo"
                || t == "[Tin nhắn thoại]"
                || t.starts_with("[Tin nhắn thoại]")
            {
                "voice"
            } else if t.starts_with("http://") || t.starts_with("https://") {
                "link"
            } else {
                "text"
            }
        }
    }
}

fn hour_of(ts_ms: i64) -> Option<u8> {
    crate::clock::local_datetime(ts_ms).map(|d| {
        use chrono::Timelike;
        d.hour() as u8
    })
}

/// Bucket 1 tin vào 1 trong 7 ngày địa phương gần nhất (0 = 6 ngày trước, 6 = hôm nay).
/// So sánh theo ngày lịch địa phương, không theo 24h rolling — DST không làm mất ngày.
/// Tin tương lai (> hôm nay) và tin quá 6 ngày trước → `None`.
fn day_bucket(ts_ms: i64, today_day: i64) -> Option<usize> {
    let day = crate::clock::local_day(ts_ms)?;
    if day > today_day || today_day - day > 6 {
        return None;
    }
    Some((6 - (today_day - day)) as usize)
}

/// 1 bản ghi nhẹ đã gộp DB+RAM: (identity, tên hiển thị, text, ts_ms, media label).
/// identity = sender uid khi có (DB `sender_id` / RAM `sidebar_sender_id`),
/// fallback tên hiển thị — 2 người trùng tên không gộp nhầm.
type LiteMsg = (String, String, String, i64, &'static str);

fn text_hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// Gộp DB (toàn bộ lịch sử đã lưu) + RAM (tin chưa persist) theo id.
/// DB trước, RAM đè (RAM mới hơn / đủ kind). Tin id rỗng vẫn giữ riêng lẻ
/// bằng key `(ts, hash text)` thay vì ghi đè nhau.
fn merge_lite(
    store: Option<&MessageStore>,
    thread: Option<&Thread>,
) -> Vec<LiteMsg> {
    let mut by_id: HashMap<String, LiteMsg> = HashMap::new();
    if let (Some(s), Some(t)) = (store, thread) {
        if let Ok(rows) = s.messages_for_all(&t.id) {
            for r in rows {
                let kind = MessageKind::from_wire_name(&r.msg_type);
                let link_src: Option<String> =
                    serde_json::from_str::<serde_json::Value>(&r.raw_json)
                        .ok()
                        .and_then(|v| {
                            v.get("link_src")
                                .and_then(|x| x.as_str())
                                .map(|x| x.to_string())
                        });
                // raw_json giữ link_src đã persist (ảnh/file/voice) để classify đúng.
                let label = classify(kind, link_src.as_deref(), &r.text);
                let ident = if r.sender_id.trim().is_empty() {
                    r.sender_name.clone()
                } else {
                    r.sender_id.clone()
                };
                if r.msg_id.is_empty() {
                    by_id.insert(
                        format!("db_noid_{}_{}", r.ts, text_hash(&r.text)),
                        (ident, r.sender_name.clone(), r.text.clone(), r.ts, label),
                    );
                } else {
                    by_id
                        .entry(r.msg_id.clone())
                        .or_insert((ident, r.sender_name.clone(), r.text.clone(), r.ts, ""));
                    if let Some(e) = by_id.get_mut(&r.msg_id) {
                        e.4 = label;
                    }
                }
            }
        }
    }
    if let Some(t) = thread {
        for m in &t.messages {
            let label = classify(m.kind, m.link_src.as_deref(), &m.text);
            let ident = crate::chat::sidebar_sender_id(m).unwrap_or_else(|| m.sender.clone());
            if m.id.is_empty() {
                by_id.insert(
                    format!("ram_noid_{}_{}_{}", t.id, m.ts, text_hash(&m.text)),
                    (ident, m.sender.clone(), m.text.clone(), m.ts, label),
                );
            } else {
                by_id.insert(
                    m.id.clone(),
                    (ident, m.sender.clone(), m.text.clone(), m.ts, label),
                );
            }
        }
    }
    by_id.into_values().collect()
}

/// Tính thống kê từ DB (toàn bộ lịch sử đã lưu) + RAM (tin chưa persist).
/// DB thiếu (mở lỗi) → chỉ dùng RAM; RAM thiếu → chỉ dùng DB.
pub fn compute_group_stats(store: Option<&MessageStore>, thread: Option<&Thread>) -> GroupStats {
    let now_ms = crate::clock::normalize_ts_ms(chrono::Local::now().timestamp_millis());
    let today_day = crate::clock::local_day(now_ms).unwrap_or(0);
    let week_ago = now_ms - 7 * 86_400_000;

    let mut st = GroupStats::default();
    let msgs = merge_lite(store, thread);
    st.total = msgs.len();
    // Hiển thị theo identity (uid), tên chỉ để trình bày — trùng tên không gộp.
    let mut sender_counts: HashMap<String, (String, usize)> = HashMap::new();
    let mut hour_counts = [0usize; 24];

    for (ident, sender, _text, ts, label) in &msgs {
        let ts_ms = crate::clock::normalize_ts_ms(*ts);
        match *label {
            "photo" => st.photos += 1,
            "file" => st.files += 1,
            "link" => st.links += 1,
            "voice" => st.voices += 1,
            _ => {}
        }
        let key = if ident.trim().is_empty() {
            format!("name:{}", sender)
        } else {
            format!("id:{ident}")
        };
        let disp = if sender.trim().is_empty() {
            "Ẩn danh".to_string()
        } else {
            sender.clone()
        };
        sender_counts
            .entry(key)
            .and_modify(|e| e.1 += 1)
            .or_insert((disp, 1));
        // last_7d chặn trên now_ms: tin tương lai không lọt vào rolling count
        // (daily bucket đã loại tin tương lai riêng).
        if ts_ms >= week_ago && ts_ms <= now_ms {
            st.last_7d += 1;
        }
        if let Some(b) = day_bucket(ts_ms, today_day) {
            st.daily[b] += 1;
        }
        if let Some(h) = hour_of(ts_ms) {
            hour_counts[h as usize] += 1;
        }
    }

    let mut senders: Vec<(String, usize)> = sender_counts.into_values().collect();
    senders.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    senders.truncate(5);
    st.top_senders = senders;

    let (peak_h, peak_c) = hour_counts
        .iter()
        .enumerate()
        .max_by_key(|(_, c)| **c)
        .map(|(h, c)| (h as u8, *c))
        .unwrap_or((0, 0));
    if peak_c > 0 {
        st.peak_hour = Some(peak_h);
        st.peak_hour_count = peak_c;
    }
    st
}

/// Tóm tắt 1 dòng cho LLM + UI ("120 tin · 7 ngày: 34 · top: An (40) · ảnh 5 …").
pub fn summarize_thread(thread: Option<&Thread>, store: &MessageStore) -> String {
    let st = compute_group_stats(Some(store), thread);
    let top = st
        .top_senders
        .iter()
        .map(|(n, c)| format!("{n} ({c})"))
        .collect::<Vec<_>>()
        .join(", ");
    let peak = match st.peak_hour {
        Some(h) => format!("{h:02}h ({} tin)", st.peak_hour_count),
        None => "—".to_string(),
    };
    format!(
        "{} tin · 7 ngày: {} · top: {} · ảnh {} · file {} · link {} · voice {} · giờ sôi động: {}",
        st.total,
        st.last_7d,
        if top.is_empty() { "—".into() } else { top },
        st.photos,
        st.files,
        st.links,
        st.voices,
        peak,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatMessage;
    use crate::store::{StoredMessage, StoredThread};

    fn thread_with_messages() -> Thread {
        let now = chrono::Local::now().timestamp_millis();
        Thread {
            id: "g1".to_string(),
            name: "G".to_string(),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: true,
            messages: vec![
                ChatMessage {
                    id: "m1".into(),
                    sender: "An".into(),
                    text: "hello".into(),
                    time: "10:00".into(),
                    ts: now,
                    kind: MessageKind::Text,
                    ..Default::default()
                },
                ChatMessage {
                    id: "m2".into(),
                    sender: "An".into(),
                    text: "[Hình ảnh]".into(),
                    time: "10:01".into(),
                    ts: now,
                    kind: MessageKind::Photo,
                    link_src: Some("Hình ảnh Zalo".into()),
                    ..Default::default()
                },
                ChatMessage {
                    id: "m3".into(),
                    sender: "Bình".into(),
                    text: "https://example.com/x".into(),
                    time: "10:02".into(),
                    ts: now,
                    kind: MessageKind::Text,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn stats_from_ram_only_are_real() {
        let t = thread_with_messages();
        let st = compute_group_stats(None, Some(&t));
        assert_eq!(st.total, 3);
        assert_eq!(st.photos, 1);
        assert_eq!(st.links, 1);
        assert_eq!(st.top_senders[0], ("An".to_string(), 2));
        assert!(st.peak_hour.is_some());
        assert_eq!(st.last_7d, 3);
        assert_eq!(st.daily.iter().sum::<usize>(), 3);
    }

    #[test]
    fn stats_dedupe_db_and_ram() {
        let mut store = MessageStore::open(":memory:").unwrap();
        store
            .upsert_thread(&StoredThread {
                thread_id: "g1".into(),
                name: "G".into(),
                is_group: true,
                last_ts: 1,
                vanish_duration_sec: 0,
                vanish_started_at_sec: 0,
                last_read_msg_id: String::new(),
            })
            .unwrap();
        store
            .upsert_message(&StoredMessage {
                msg_id: "m1".into(),
                thread_id: "g1".into(),
                sender_id: "An".into(),
                sender_name: "An".into(),
                text: "hello".into(),
                msg_type: "chat.text".into(),
                ts: chrono::Local::now().timestamp_millis(),
                is_outgoing: false,
                raw_json: String::new(),
            })
            .unwrap();
        let t = thread_with_messages();
        let st = compute_group_stats(Some(&store), Some(&t));
        // m1 trùng id DB+RAM → đếm 1 lần.
        assert_eq!(st.total, 3);
    }

    #[test]
    fn stats_full_db_beyond_cap_idless_distinct_and_future_excluded() {
        use chrono::{Duration, Local};
        let mut store = MessageStore::open(":memory:").unwrap();
        store
            .upsert_thread(&StoredThread {
                thread_id: "g9".into(),
                name: "G9".into(),
                is_group: true,
                last_ts: 1,
                vanish_duration_sec: 0,
                vanish_started_at_sec: 0,
                last_read_msg_id: String::new(),
            })
            .unwrap();
        let now = Local::now();
        // >100k tin trong DB: vẫn đếm toàn bộ (không cap 100k).
        for i in 0..100_005 {
            let ts = (now - Duration::days(10)).timestamp_millis();
            store
                .upsert_message(&StoredMessage {
                    msg_id: format!("db{i}"),
                    thread_id: "g9".into(),
                    sender_id: "u_old".into(),
                    sender_name: "Cũ".into(),
                    text: "old".into(),
                    msg_type: "chat.text".into(),
                    ts,
                    is_outgoing: false,
                    raw_json: String::new(),
                })
                .unwrap();
        }
        // 2 tin id rỗng cùng ts nhưng khác text → giữ riêng, đếm 2.
        let ts_now = now.timestamp_millis();
        let mut t = Thread {
            id: "g9".to_string(),
            name: "G9".to_string(),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: true,
            messages: vec![
                ChatMessage {
                    id: String::new(),
                    sender: "An".into(),
                    text: "alpha".into(),
                    time: "t".into(),
                    ts: ts_now,
                    kind: MessageKind::Text,
                    ..Default::default()
                },
                ChatMessage {
                    id: String::new(),
                    sender: "An".into(),
                    text: "beta".into(),
                    time: "t".into(),
                    ts: ts_now,
                    kind: MessageKind::Text,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        // 2 người trùng tên "An" nhưng khác uid → rank theo identity, không gộp.
        t.messages[0].wire_ref =
            crate::message_model::WireMessageRef::from_raw(&serde_json::json!({"sender": "111"}));
        t.messages[1].wire_ref =
            crate::message_model::WireMessageRef::from_raw(&serde_json::json!({"sender": "222"}));
        // Tin tương lai (+2 ngày): total có, last_7d/daily không.
        let mut future = ChatMessage {
            id: "future1".into(),
            sender: "An".into(),
            text: "future".into(),
            time: "t".into(),
            ts: (now + Duration::days(2)).timestamp_millis(),
            kind: MessageKind::Text,
            ..Default::default()
        };
        future.wire_ref =
            crate::message_model::WireMessageRef::from_raw(&serde_json::json!({"sender": "111"}));
        t.messages.push(future);
        let st = compute_group_stats(Some(&store), Some(&t));
        assert_eq!(st.total, 100_005 + 3);
        // 100_005 tin cũ (10 ngày trước) + future ngoài rolling → last_7d = 2 (alpha+beta).
        assert_eq!(st.last_7d, 2);
        // daily chỉ đếm hôm nay (bucket local): alpha+beta, không gồm future.
        let today = crate::clock::local_day(crate::clock::normalize_ts_ms(ts_now)).unwrap();
        let b = day_bucket(crate::clock::normalize_ts_ms(ts_now), today).unwrap();
        assert_eq!(st.daily[b], 2);
        assert_eq!(st.daily.iter().sum::<usize>(), 2);
        // top_senders tách theo identity: 2 An riêng + Cũ.
        assert_eq!(st.top_senders.len(), 3);
    }

    #[test]
    fn stats_local_day_bucket_not_rolling_24h() {
        use chrono::{Local, TimeZone};
        // 23:59 hôm qua và 00:01 hôm nay (giờ địa phương): khác ngày lịch
        // nhưng cách nhau 2 phút — rolling 24h sẽ gộp, bucket local phải tách.
        let today = Local::now().date_naive();
        let late = Local
            .from_local_datetime(&(today - chrono::Days::new(1)).and_hms_opt(23, 59, 0).unwrap())
            .unwrap()
            .timestamp_millis();
        let early = Local
            .from_local_datetime(&today.and_hms_opt(0, 1, 0).unwrap())
            .unwrap()
            .timestamp_millis();
        let now_ms = crate::clock::normalize_ts_ms(Local::now().timestamp_millis());
        let today_day = crate::clock::local_day(now_ms).unwrap();
        let b_late = day_bucket(crate::clock::normalize_ts_ms(late), today_day).unwrap();
        let b_early = day_bucket(crate::clock::normalize_ts_ms(early), today_day).unwrap();
        assert_eq!(b_early, 6);
        assert_eq!(b_late, 5);
    }
}
