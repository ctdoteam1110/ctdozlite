//! Offline sample workspace for `--demo`.
//!
//! Used for screenshots, design reviews and UI testing without a Zalo account. Nothing here
//! touches the network or the disk: the GUI constructor skips the session worker entirely when
//! demo mode is on, so every `WORKER_TX` send becomes a no-op.

use std::collections::HashMap;

use crate::chat::{ChatMessage, FriendshipStatus, Thread};

pub(super) struct DemoWorkspace {
    pub(super) user_name: String,
    pub(super) threads: Vec<Thread>,
    pub(super) unread_counts: HashMap<String, usize>,
    pub(super) typing: HashMap<String, String>,
    pub(super) member_counts: HashMap<String, usize>,
    pub(super) friend_status: HashMap<String, FriendshipStatus>,
}

/// Demo attachment URLs (never requested; used as `downloads` keys by `QTZALO_DEMO_DOWNLOADS`).
pub(super) const DEMO_FILE_URL: &str = "https://f21-zpc.zdn.vn/demo/QTzalo-landing.fig";
pub(super) const DEMO_VIDEO_URL: &str = "https://video.zdn.vn/demo/demo-onboarding.mp4";
pub(super) const DEMO_PHOTO_URL: &str = "https://photo-stal-1.zdn.vn/demo/bang-mau.jpg";

fn msg(id: &str, sender: &str, text: &str, ts_ms: i64, outgoing: bool) -> ChatMessage {
    ChatMessage {
        id: id.to_string(),
        sender: sender.to_string(),
        text: text.to_string(),
        time: crate::timefmt::hm_from_ms(ts_ms),
        is_outgoing: outgoing,
        ts: ts_ms,
        ..Default::default()
    }
}

fn thread(id: &str, name: &str, is_group: bool, preview: &str, messages: Vec<ChatMessage>) -> Thread {
    let time = messages
        .last()
        .map(|m| crate::timefmt::hm_from_ms(m.ts))
        .unwrap_or_default();
    Thread {
        id: id.to_string(),
        name: name.to_string(),
        preview: preview.to_string(),
        time,
        avatar: None,
        is_group,
        messages,
    }
}

impl DemoWorkspace {
    pub(super) fn build() -> Self {
        let now = crate::timefmt::now_ms();
        let min = 60_000_i64;
        let day = 86_400_000_i64;

        // 1. Group chat with a reply, a reaction, a file card and a typing indicator.
        let mut g1 = vec![
            msg(
                "d1",
                "Lan Phương",
                "Mọi người xem bản dựng QTzalo mới chưa? Giao diện native nhìn đã thật 👀",
                now - 52 * min,
                false,
            ),
            msg("d2", "Bạn", "Xem rồi, mượt hơn hẳn bản Electron", now - 49 * min, true),
            msg(
                "d3",
                "Anh Minh",
                "Đồng bộ tin nhắn từ điện thoại chạy ổn không?",
                now - 41 * min,
                false,
            ),
            msg(
                "d4",
                "Bạn",
                "Ổn, mình làm mới xong hồi sáng. Xác nhận trên điện thoại là chạy.",
                now - 38 * min,
                true,
            ),
            msg("d6", "Anh Minh", "[Hình ảnh]", now - 30 * min, false),
            msg("d7", "Bạn", "[Video] demo-onboarding.mp4", now - 21 * min, true),
            msg("d5", "Lan Phương", "[Tập tin] QTzalo-landing.fig", now - 12 * min, false),
        ];
        g1[1].reactions = vec!["❤️".into(), "❤️".into()];
        g1[2].reply_to_text = Some("Xem rồi, mượt hơn hẳn bản Electron".into());
        g1[2].reply_to_sender = Some("Bạn".into());
        // Attachments use the same fields as real messages (see `chat.rs`). The URLs are never
        // fetched: demo mode has no worker, so thumbnails stay placeholders.
        g1[4].link_src = Some("Hình ảnh Zalo".into());
        g1[4].link_title = Some("Ảnh chụp màn hình bảng màu".into());
        g1[4].link_url = Some(DEMO_PHOTO_URL.into());
        g1[4].link_thumb = Some(DEMO_PHOTO_URL.into());
        g1[5].link_src = Some("Video Zalo".into());
        g1[5].link_title = Some("demo-onboarding.mp4".into());
        g1[5].link_desc = Some("Video · 0:42 · 8,6 MB".into());
        g1[5].link_url = Some(DEMO_VIDEO_URL.into());
        g1[6].link_src = Some("Tập tin Zalo".into());
        g1[6].link_title = Some("QTzalo-landing.fig".into());
        g1[6].link_desc = Some("24,1 MB".into());
        g1[6].link_url = Some(DEMO_FILE_URL.into());

        let threads = vec![
            thread("demo_tk", "Nhóm Thiết kế", true, "Lan Phương: QTzalo-landing.fig", g1),
            thread(
                "demo_me",
                "Mẹ",
                false,
                "Cuối tuần về ăn cơm nhé con",
                vec![msg("m1", "Mẹ", "Cuối tuần về ăn cơm nhé con", now - 3 * 60 * min, false)],
            ),
            thread(
                "demo_am",
                "Anh Minh",
                false,
                "Ok để mình xem lại rồi báo",
                vec![msg("a1", "Anh Minh", "Ok để mình xem lại rồi báo", now - day - 2 * 60 * min, false)],
            ),
            thread(
                "demo_cloud",
                "Cloud của tôi",
                false,
                "Ghi chú họp 27/10",
                vec![msg("c1", "Bạn", "Ghi chú họp 27/10", now - day - 5 * 60 * min, true)],
            ),
            thread(
                "demo_db",
                "Đội bóng công ty",
                true,
                "Tuấn: Chủ nhật 7h sân cũ nhé",
                vec![msg("b1", "Tuấn", "Chủ nhật 7h sân cũ nhé", now - 3 * day, false)],
            ),
            thread(
                "demo_lp",
                "Lan Phương",
                false,
                "Cảm ơn nha 🙏",
                vec![msg("l1", "Lan Phương", "Cảm ơn nha 🙏", now - 4 * day, false)],
            ),
            thread(
                "demo_wl",
                "Wang Lei 王磊",
                false,
                "Được chứ! Hẹn bạn 9 giờ sáng Chủ nhật — 星期天见。",
                vec![
                    msg("w1", "Wang Lei 王磊", "你好！周末一起喝咖啡吗？", now - 5 * day, false),
                    msg(
                        "w2",
                        "Bạn",
                        "Được chứ! Hẹn bạn 9 giờ sáng Chủ nhật nhé — 星期天上午九点见。",
                        now - 5 * day + 4 * min,
                        true,
                    ),
                ],
            ),
            thread(
                "demo_bn",
                "Bố Nam",
                false,
                "Ảnh",
                vec![msg("n1", "Bố Nam", "Ảnh chụp chuyến đi Đà Lạt", now - 6 * day, false)],
            ),
            thread(
                "demo_nh",
                "Ngọc Hân",
                false,
                "Chào bạn, mình lấy số từ nhóm Thiết kế",
                vec![msg("h1", "Ngọc Hân", "Chào bạn, mình lấy số từ nhóm Thiết kế", now - 9 * day, false)],
            ),
            thread(
                "demo_qb",
                "Trần Quốc Bảo",
                false,
                "Đã gửi lời mời kết bạn",
                vec![msg("q1", "Trần Quốc Bảo", "Xin chào, kết bạn với mình nhé!", now - 2 * day, false)],
            ),
        ];

        let mut unread_counts = HashMap::new();
        unread_counts.insert("demo_me".to_string(), 1);
        unread_counts.insert("demo_db".to_string(), 3);
        unread_counts.insert("demo_qb".to_string(), 1);

        let mut typing = HashMap::new();
        typing.insert("demo_tk".to_string(), "Anh Minh".to_string());

        let mut member_counts = HashMap::new();
        member_counts.insert("demo_tk".to_string(), 5);
        member_counts.insert("demo_db".to_string(), 18);

        let mut friend_status = HashMap::new();
        for id in ["demo_me", "demo_am", "demo_lp", "demo_wl", "demo_bn"] {
            friend_status.insert(id.to_string(), FriendshipStatus::Friend);
        }
        friend_status.insert("demo_nh".to_string(), FriendshipStatus::Stranger);
        friend_status.insert("demo_qb".to_string(), FriendshipStatus::IncomingPending);

        Self {
            user_name: "Minh Anh".into(),
            threads,
            unread_counts,
            typing,
            member_counts,
            friend_status,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_workspace_is_populated_and_offline() {
        let d = DemoWorkspace::build();
        assert!(d.threads.len() >= 8);
        assert!(d.threads.iter().any(|t| t.is_group));
        assert!(d.threads.iter().any(|t| t.name.contains("Cloud của tôi")));
        let g = &d.threads[0];
        assert!(g.messages.iter().any(|m| m.reply_to_text.is_some()));
        assert!(g.messages.iter().any(|m| !m.reactions.is_empty()));
        assert!(g.messages.iter().any(|m| m.link_src.as_deref() == Some("Tập tin Zalo")));
        assert!(g.messages.iter().any(|m| m.link_src.as_deref() == Some("Video Zalo")));
        assert!(g.messages.iter().any(|m| m.link_src.as_deref() == Some("Hình ảnh Zalo")));
        // Every attachment card has a download target.
        for m in g.messages.iter().filter(|m| m.link_src.is_some()) {
            assert!(super::super::messages::attachment_download_target(m).is_some());
        }
        assert_eq!(d.typing.get("demo_tk").map(String::as_str), Some("Anh Minh"));
        // No remote avatars: rendering demo threads never asks the worker for images.
        assert!(d.threads.iter().all(|t| t.avatar.is_none()));
    }
}
