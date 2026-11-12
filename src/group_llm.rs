//! Nền LLM cho Group Workspace: gom ngữ cảnh 1 nhóm để sau gắn provider thật.
//!
//! Không gọi mạng ở đây. `StubProvider` trả lỗi thân thiện "chưa cấu hình LLM"
//! để UI báo "sắp có"; provider thật (OpenAI/Anthropic/local) sau này chỉ cần
//! impl `LlmProvider` là dùng được ngay.

use crate::chat::Thread;
use crate::store::MessageStore;

/// 1 tin nhắn gọn trong ngữ cảnh LLM.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupRecentMessage {
    pub sender: String,
    pub text: String,
    pub time: String,
}

/// 1 tin khớp từ khóa theo dõi.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackedHit {
    pub keyword: String,
    pub sender: String,
    pub text: String,
    pub time: String,
}

/// Ngữ cảnh đầy đủ của 1 nhóm cho LLM.
#[derive(Debug, Clone, Default)]
pub struct GroupContext {
    pub thread_id: String,
    pub thread_name: String,
    pub note: String,
    pub stats_summary: String,
    pub recent_messages: Vec<GroupRecentMessage>,
    pub tracked_hits: Vec<TrackedHit>,
}

/// Provider LLM trừu tượng. Sau này gắn API thật chỉ cần impl trait này.
pub trait LlmProvider {
    fn summarize(&self, ctx: &GroupContext) -> anyhow::Result<String>;
    fn ask(&self, ctx: &GroupContext, question: &str) -> anyhow::Result<String>;
}

/// Provider giữ chỗ: chưa có cấu hình LLM nên mọi gọi đều báo "sắp có".
pub struct StubProvider;

impl LlmProvider for StubProvider {
    fn summarize(&self, _ctx: &GroupContext) -> anyhow::Result<String> {
        anyhow::bail!("chưa cấu hình LLM — tính năng tóm tắt nhóm sắp có")
    }

    fn ask(&self, _ctx: &GroupContext, _question: &str) -> anyhow::Result<String> {
        anyhow::bail!("chưa cấu hình LLM — tính năng hỏi đáp nhóm sắp có")
    }
}

/// Dựng `GroupContext` từ DB mẫu + RAM: tên nhóm, ghi chú, tóm tắt thống kê,
/// 30 tin mới nhất, và tin khớp từ khóa (tối đa 10 tin/keyword, gộp lại ≤ 30).
pub fn build_group_context(
    store: &MessageStore,
    threads: &[Thread],
    thread_id: &str,
) -> GroupContext {
    if thread_id.is_empty() {
        return GroupContext::default();
    }
    let thread = threads.iter().find(|t| t.id == thread_id);
    let thread_name = thread.map(|t| t.name.clone()).unwrap_or_default();
    let note = store.get_group_note(thread_id);
    let keywords = store.list_group_watch(thread_id);

    // Gộp DB + RAM theo id trước khi chọn recent/hits: tin chỉ có trong DB
    // (ngoài cửa sổ RAM) vẫn vào ngữ cảnh, không trùng lặp.
    // Sắp xếp theo ts chuẩn hoá ms (lẫn lộn giây/ms không đảo thứ tự).
    let mut with_ts: Vec<(i64, String, String, String, String)> = Vec::new();
    if let Ok(rows) = store.messages_for_all(thread_id) {
        for r in rows {
            let time = crate::clock::message_time(crate::clock::normalize_ts_ms(r.ts));
            with_ts.push((
                crate::clock::normalize_ts_ms(r.ts),
                r.msg_id,
                r.sender_name,
                r.text,
                time,
            ));
        }
    }
    if let Some(t) = thread {
        for m in &t.messages {
            let ts = crate::clock::normalize_ts_ms(m.ts);
            if !m.id.is_empty() {
                if let Some(slot) = with_ts.iter_mut().find(|(_, id, _, _, _)| *id == m.id) {
                    slot.0 = ts;
                    slot.2 = m.sender.clone();
                    slot.3 = m.text.clone();
                    slot.4 = m.time.clone();
                    continue;
                }
            }
            with_ts.push((ts, m.id.clone(), m.sender.clone(), m.text.clone(), m.time.clone()));
        }
    }
    with_ts.sort_by_key(|(ts, _, _, _, _)| *ts);

    let mut recent: Vec<GroupRecentMessage> = Vec::new();
    let mut tracked: Vec<TrackedHit> = Vec::new();
    {
        let n = with_ts.len();
        let start = n.saturating_sub(30);
        for (_, _, sender, text, time) in &with_ts[start..] {
            recent.push(GroupRecentMessage {
                sender: if sender.is_empty() {
                    "Ẩn danh".to_string()
                } else {
                    sender.clone()
                },
                text: text.clone(),
                time: time.clone(),
            });
        }
        // Tin khớp: duyệt từ mới nhất về cũ, mỗi keyword tối đa 10 tin.
        let mut per_kw: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for (_, _, sender, text, time) in with_ts.iter().rev() {
            if tracked.len() >= 30 {
                break;
            }
            let hay = text.to_lowercase();
            for kw in &keywords {
                if tracked.len() >= 30 {
                    break;
                }
                let needle = kw.to_lowercase();
                if needle.is_empty() || !hay.contains(&needle) {
                    continue;
                }
                let used = per_kw.get(kw).copied().unwrap_or(0);
                if used >= 10 {
                    continue;
                }
                per_kw.insert(kw.clone(), used + 1);
                tracked.push(TrackedHit {
                    keyword: kw.clone(),
                    sender: if sender.is_empty() {
                        "Ẩn danh".to_string()
                    } else {
                        sender.clone()
                    },
                    text: text.clone(),
                    time: time.clone(),
                });
            }
        }
        tracked.reverse();
    }

    let stats_summary = crate::group_stats::summarize_thread(thread, store);
    GroupContext {
        thread_id: thread_id.to_string(),
        thread_name,
        note,
        stats_summary,
        recent_messages: recent,
        tracked_hits: tracked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatMessage;
    use crate::store::{StoredMessage, StoredThread};

    fn sample_thread() -> Thread {
        Thread {
            id: "g1".to_string(),
            name: "Nhóm Kế Toán".to_string(),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: true,
            messages: vec![
                ChatMessage {
                    id: "m1".into(),
                    sender: "An".into(),
                    text: "chốt báo cáo quý".into(),
                    time: "09:00".into(),
                    ts: 1000,
                    ..Default::default()
                },
                ChatMessage {
                    id: "m2".into(),
                    sender: "Bình".into(),
                    text: "ok, gửi file nhé".into(),
                    time: "09:05".into(),
                    ts: 2000,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn context_carries_note_stats_recent_and_hits() {
        let mut store = MessageStore::open(":memory:").unwrap();
        store
            .upsert_thread(&StoredThread {
                thread_id: "g1".into(),
                name: "Nhóm Kế Toán".into(),
                is_group: true,
                last_ts: 2000,
                vanish_duration_sec: 0,
                vanish_started_at_sec: 0,
                last_read_msg_id: String::new(),
            })
            .unwrap();
        for (id, sender, text, ts) in [
            ("m1", "An", "chốt báo cáo quý", 1000),
            ("m2", "Bình", "ok, gửi file nhé", 2000),
        ] {
            store
                .upsert_message(&StoredMessage {
                    msg_id: id.into(),
                    thread_id: "g1".into(),
                    sender_id: sender.into(),
                    sender_name: sender.into(),
                    text: text.into(),
                    msg_type: "chat.text".into(),
                    ts,
                    is_outgoing: false,
                    raw_json: String::new(),
                })
                .unwrap();
        }
        store.set_group_note("g1", "ghi chú nhóm", 123).unwrap();
        store.add_group_watch("g1", "báo cáo").unwrap();

        let threads = vec![sample_thread()];
        let ctx = build_group_context(&store, &threads, "g1");
        assert_eq!(ctx.thread_id, "g1");
        assert_eq!(ctx.thread_name, "Nhóm Kế Toán");
        assert_eq!(ctx.note, "ghi chú nhóm");
        assert!(!ctx.stats_summary.is_empty());
        assert_eq!(ctx.recent_messages.len(), 2);
        assert_eq!(ctx.recent_messages[0].sender, "An");
        assert_eq!(ctx.tracked_hits.len(), 1);
        assert_eq!(ctx.tracked_hits[0].keyword, "báo cáo");
    }

    #[test]
    fn stub_provider_reports_not_configured() {
        let ctx = GroupContext::default();
        let p = StubProvider;
        assert!(p.summarize(&ctx).unwrap_err().to_string().contains("chưa cấu hình"));
        assert!(p.ask(&ctx, "gì hot?").unwrap_err().to_string().contains("chưa cấu hình"));
    }
}
