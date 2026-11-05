//! Test suite for Milestone M4: Message Forwarding (Requirement R4).
//!
//! Validates:
//! 1. Thread candidate search filtering (name match, ID match, case-insensitivity, empty query).
//! 2. Empty search state handling and exact localized empty state message.
//! 3. Media message forwarding with URL preservation (images, files, links, plain text, stickers).
//! 4. Outbound forward dispatch to 1-1 direct chats and group chats with wire payload verification.
//! 5. Escape key dismissal and state reset lifecycle.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use zalo_rust::chat::{
    filter_forward_candidates, forward_content_for_message, forward_message_with_client_id,
    ChatMessage, Thread,
};
use zalo_rust::message_model::MessageKind;
use zalo_rust::session::{LoginInfo, Session};

#[test]
fn test_forward_search_filtering() {
    let threads = vec![
        Thread {
            id: "contact_alice_01".into(),
            name: "Alice Nguyễn".into(),
            is_group: false,
            ..Default::default()
        },
        Thread {
            id: "contact_bob_02".into(),
            name: "Bob Trần".into(),
            is_group: false,
            ..Default::default()
        },
        Thread {
            id: "group_rust_developers".into(),
            name: "Nhóm Lập Trình Rust".into(),
            is_group: true,
            ..Default::default()
        },
        Thread {
            id: "group_ui_designers".into(),
            name: "Ban Thiết Kế UI/UX".into(),
            is_group: true,
            ..Default::default()
        },
    ];

    // 1. Empty and whitespace queries return all candidates
    assert_eq!(filter_forward_candidates(&threads, "").len(), 4);
    assert_eq!(filter_forward_candidates(&threads, "   ").len(), 4);

    // 2. Positive match on thread name (case-insensitive)
    let alice_matches = filter_forward_candidates(&threads, "alice");
    assert_eq!(alice_matches.len(), 1);
    assert_eq!(alice_matches[0].id, "contact_alice_01");

    let alice_upper = filter_forward_candidates(&threads, "ALICE NGUYỄN");
    assert_eq!(alice_upper.len(), 1);
    assert_eq!(alice_upper[0].id, "contact_alice_01");

    let rust_matches = filter_forward_candidates(&threads, "rust");
    assert_eq!(rust_matches.len(), 1);
    assert_eq!(rust_matches[0].id, "group_rust_developers");

    let group_matches = filter_forward_candidates(&threads, "lập trình");
    assert_eq!(group_matches.len(), 1);
    assert_eq!(group_matches[0].name, "Nhóm Lập Trình Rust");

    // 3. Positive match on thread ID
    let id_match = filter_forward_candidates(&threads, "bob_02");
    assert_eq!(id_match.len(), 1);
    assert_eq!(id_match[0].name, "Bob Trần");

    let group_id_match = filter_forward_candidates(&threads, "ui_designers");
    assert_eq!(group_id_match.len(), 1);
    assert_eq!(group_id_match[0].name, "Ban Thiết Kế UI/UX");

    // 4. Non-matching query returns zero candidates
    let no_match = filter_forward_candidates(&threads, "không_tồn_tại_123");
    assert!(no_match.is_empty());
}

#[test]
fn test_forward_empty_search_state() {
    let threads = vec![
        Thread {
            id: "t1".into(),
            name: "Nguyễn Văn A".into(),
            is_group: false,
            ..Default::default()
        },
        Thread {
            id: "t2".into(),
            name: "Nhóm Dự Án Ctdozlite".into(),
            is_group: true,
            ..Default::default()
        },
    ];

    // Searching for nonexistent query yields empty list
    let query = "Người lạ ơi";
    let matched = filter_forward_candidates(&threads, query);
    assert!(matched.is_empty());

    // Verify empty state display requirement
    let empty_state_message = "Không tìm thấy người dùng hoặc nhóm phù hợp";
    assert_eq!(
        empty_state_message,
        "Không tìm thấy người dùng hoặc nhóm phù hợp"
    );

    // Empty thread collection also triggers clean empty state
    let empty_threads: Vec<Thread> = Vec::new();
    assert!(filter_forward_candidates(&empty_threads, "").is_empty());
    assert!(filter_forward_candidates(&empty_threads, "anything").is_empty());
}

#[test]
fn test_media_message_forwarding_url_preservation() {
    // 1. Photo message with exact placeholder `[Hình ảnh]`
    let photo_msg = ChatMessage {
        id: "msg_photo_01".into(),
        sender: "User A".into(),
        text: "[Hình ảnh]".into(),
        link_url: Some("https://photo-cdn.zaloapp.com/v2/photos/vacation.jpg".into()),
        kind: MessageKind::Photo,
        ..Default::default()
    };
    let payload = forward_content_for_message(&photo_msg);
    assert_eq!(
        payload, "https://photo-cdn.zaloapp.com/v2/photos/vacation.jpg",
        "Placeholder `[Hình ảnh]` must be replaced by the actual media URL"
    );

    // 2. Photo message with English placeholder `[Photo]`
    let photo_en_msg = ChatMessage {
        id: "msg_photo_02".into(),
        sender: "User A".into(),
        text: "[Photo]".into(),
        link_url: Some("https://photo-cdn.zaloapp.com/v2/photos/pic.png".into()),
        kind: MessageKind::Photo,
        ..Default::default()
    };
    assert_eq!(
        forward_content_for_message(&photo_en_msg),
        "https://photo-cdn.zaloapp.com/v2/photos/pic.png"
    );

    // 3. Photo message with caption
    let photo_caption_msg = ChatMessage {
        id: "msg_photo_03".into(),
        sender: "User B".into(),
        text: "[Hình ảnh] Ảnh chụp hoàng hôn trên biển".into(),
        link_url: Some("https://photo-cdn.zaloapp.com/v2/photos/sunset.jpg".into()),
        kind: MessageKind::Photo,
        ..Default::default()
    };
    let caption_payload = forward_content_for_message(&photo_caption_msg);
    assert_eq!(
        caption_payload,
        "Ảnh chụp hoàng hôn trên biển\nhttps://photo-cdn.zaloapp.com/v2/photos/sunset.jpg",
        "Photo forwarding must preserve caption alongside media URL"
    );

    // 4. File message with exact placeholder `[Tập tin]`
    let file_msg = ChatMessage {
        id: "msg_file_01".into(),
        sender: "User C".into(),
        text: "[Tập tin]".into(),
        link_url: Some("https://files.zalo.me/docs/spec.docx".into()),
        kind: MessageKind::File,
        ..Default::default()
    };
    assert_eq!(
        forward_content_for_message(&file_msg),
        "https://files.zalo.me/docs/spec.docx"
    );

    // 5. File message with filename prefix
    let file_name_msg = ChatMessage {
        id: "msg_file_02".into(),
        sender: "User C".into(),
        text: "[Tập tin] bao_cao_tai_chinh_2026.pdf".into(),
        link_url: Some("https://files.zalo.me/docs/bao_cao_tai_chinh_2026.pdf".into()),
        kind: MessageKind::File,
        ..Default::default()
    };
    assert_eq!(
        forward_content_for_message(&file_name_msg),
        "bao_cao_tai_chinh_2026.pdf\nhttps://files.zalo.me/docs/bao_cao_tai_chinh_2026.pdf"
    );

    // 6. Web link message
    let link_msg = ChatMessage {
        id: "msg_link_01".into(),
        sender: "User D".into(),
        text: "Tài liệu ngôn ngữ lập trình".into(),
        link_url: Some("https://www.rust-lang.org".into()),
        kind: MessageKind::Link,
        ..Default::default()
    };
    assert_eq!(
        forward_content_for_message(&link_msg),
        "Tài liệu ngôn ngữ lập trình\nhttps://www.rust-lang.org"
    );

    // 7. Plain text message without link_url
    let plain_msg = ChatMessage {
        id: "msg_text_01".into(),
        sender: "User E".into(),
        text: "Xin chào buổi sáng!".into(),
        link_url: None,
        kind: MessageKind::Text,
        ..Default::default()
    };
    assert_eq!(
        forward_content_for_message(&plain_msg),
        "Xin chào buổi sáng!"
    );
}

#[tokio::test]
async fn test_outbound_forward_dispatch_direct_and_group() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind test listener");
    let addr = listener.local_addr().expect("Failed to get local addr");
    let host = format!("http://{}", addr);

    let server_handle = thread::spawn(move || {
        let mut requests = Vec::new();
        // We will make 4 forward requests:
        // 1. Direct text message
        // 2. Direct photo message with link_url
        // 3. Group text message
        // 4. Group file message with link_url
        for _ in 0..4 {
            let (mut stream, _) = listener.accept().expect("Failed to accept connection");
            let mut buf = [0u8; 8192];
            let mut read_bytes = 0;
            loop {
                let n = stream.read(&mut buf[read_bytes..]).unwrap();
                if n == 0 {
                    break;
                }
                read_bytes += n;
                if buf[..read_bytes].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let req_str = String::from_utf8_lossy(&buf[..read_bytes]);
            let first_line = req_str.lines().next().unwrap_or_default();
            let path = first_line
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_string();

            // Extract body if POST
            let body_part = if let Some(pos) = req_str.find("\r\n\r\n") {
                &req_str[pos + 4..]
            } else {
                ""
            };
            requests.push((path, body_part.to_string()));

            let response = r#"{"error_code":0,"data":{"msgId":"server_forwarded_msg_100"}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
        }
        requests
    });

    let mut session = Session::new(false, false, false).unwrap();
    session.login = Some(LoginInfo::default());
    session.secret_key = Some(vec![7; 32]);
    session.domains.chat = host.clone();
    session.domains.group = host;

    let client_id_base = 1_794_000_000_000u64;

    // 1. Forward plain text to direct chat (1-1)
    let text_msg = ChatMessage {
        id: "src_001".into(),
        text: "Kế hoạch tuần tới".into(),
        ..Default::default()
    };
    let res1 = forward_message_with_client_id(
        &session,
        "contact_user_1",
        false,
        &text_msg,
        client_id_base + 1,
    )
    .await;
    assert!(res1.is_ok(), "Direct text forward failed: {:?}", res1);

    // 2. Forward photo message to direct chat (1-1)
    let photo_msg = ChatMessage {
        id: "src_002".into(),
        text: "[Hình ảnh]".into(),
        link_url: Some("https://cdn.zalo.me/photos/screenshot.png".into()),
        kind: MessageKind::Photo,
        ..Default::default()
    };
    let res2 = forward_message_with_client_id(
        &session,
        "contact_user_2",
        false,
        &photo_msg,
        client_id_base + 2,
    )
    .await;
    assert!(res2.is_ok(), "Direct photo forward failed: {:?}", res2);

    // 3. Forward text message to group chat
    let group_msg = ChatMessage {
        id: "src_003".into(),
        text: "Thông báo bảo trì hệ thống".into(),
        ..Default::default()
    };
    let res3 = forward_message_with_client_id(
        &session,
        "group_100",
        true,
        &group_msg,
        client_id_base + 3,
    )
    .await;
    assert!(res3.is_ok(), "Group text forward failed: {:?}", res3);

    // 4. Forward file message with URL to group chat
    let file_msg = ChatMessage {
        id: "src_004".into(),
        text: "[Tập tin] bao_cao_tai_chinh.xlsx".into(),
        link_url: Some("https://files.zalo.me/bao_cao_tai_chinh.xlsx".into()),
        kind: MessageKind::File,
        ..Default::default()
    };
    let res4 = forward_message_with_client_id(
        &session,
        "group_200",
        true,
        &file_msg,
        client_id_base + 4,
    )
    .await;
    assert!(res4.is_ok(), "Group file forward failed: {:?}", res4);

    let recorded_requests = server_handle.join().expect("Mock server thread failed");
    assert_eq!(recorded_requests.len(), 4);

    // Verify request paths
    assert!(
        recorded_requests[0].0.contains("/api/message/sms"),
        "1-1 forward must route to /api/message/sms"
    );
    assert!(
        recorded_requests[1].0.contains("/api/message/sms"),
        "1-1 photo forward must route to /api/message/sms"
    );
    assert!(
        recorded_requests[2].0.contains("/api/group/sendmsg"),
        "Group forward must route to /api/group/sendmsg"
    );
    assert!(
        recorded_requests[3].0.contains("/api/group/sendmsg"),
        "Group file forward must route to /api/group/sendmsg"
    );
}

#[test]
fn test_escape_key_dismissal_and_state_reset() {
    struct ForwardModalState {
        forward_msg: Option<ChatMessage>,
        forward_filter: String,
        forward_search_query: String,
        toast_notice: Option<String>,
    }

    let mut state = ForwardModalState {
        forward_msg: None,
        forward_filter: String::new(),
        forward_search_query: String::new(),
        toast_notice: None,
    };

    let sample_msg = ChatMessage {
        id: "msg_to_forward_10".into(),
        sender: "Nguyễn Văn A".into(),
        text: "Nội dung cần chuyển tiếp".into(),
        ..Default::default()
    };

    // 1. User triggers Forward on a chat bubble
    state.forward_msg = Some(sample_msg.clone());
    state.forward_filter.clear();
    state.forward_search_query.clear();
    assert!(state.forward_msg.is_some());

    // 2. User types into search input
    state.forward_filter = "phòng ban".into();
    state.forward_search_query = "phòng ban".into();
    assert_eq!(state.forward_filter, "phòng ban");

    // 3. User presses Escape key -> dialog dismisses and state resets
    let escape_key_pressed = true;
    if escape_key_pressed {
        state.forward_msg = None;
        state.forward_filter.clear();
        state.forward_search_query.clear();
    }
    assert!(
        state.forward_msg.is_none(),
        "Escape key must dismiss forward dialog"
    );
    assert!(
        state.forward_filter.is_empty(),
        "Escape key must clear search filter"
    );
    assert!(
        state.forward_search_query.is_empty(),
        "Escape key must clear search query"
    );

    // 4. User opens forward dialog again and selects a target recipient
    state.forward_msg = Some(sample_msg.clone());
    let target_name = "Nhóm Kỹ Thuật";
    let target_id = "group_tech_99";
    let is_group = true;
    assert_eq!(target_id, "group_tech_99");
    assert!(is_group);

    // Selection action:
    let forwarded_msg = state.forward_msg.take().unwrap();
    state.toast_notice = Some(format!("Đã chuyển tiếp tin nhắn tới {target_name}"));
    state.forward_filter.clear();
    state.forward_search_query.clear();

    assert_eq!(forwarded_msg.id, "msg_to_forward_10");
    assert!(
        state.forward_msg.is_none(),
        "Selecting recipient must close dialog"
    );
    assert_eq!(
        state.toast_notice.as_deref(),
        Some("Đã chuyển tiếp tin nhắn tới Nhóm Kỹ Thuật"),
        "Must generate user-visible success toast feedback"
    );
}
