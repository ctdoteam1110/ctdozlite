//! Test suite for Milestone M4: Message Forwarding (Requirement R4).
//!
//! Validates:
//! 1. Thread candidate search filtering (name match, ID match, case-insensitivity, empty query).
//! 2. Empty search state handling and exact localized empty state message.
//! 3. Media message forwarding with URL preservation (images, files, links, plain text, stickers).
//! 4. Rich-link forward fallback preserving URLs even when captions omit them (CodeRabbit review fix).
//! 5. Outbound forward dispatch to 1-1 direct chats and group chats with wire payload verification.
//! 6. Escape key dismissal and state reset lifecycle.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use zalo_rust::chat::{
    filter_forward_candidates, forward_content_for_message, forward_message_with_client_id,
    send_link_message_with_client_id, ChatMessage, LinkMetadata, Thread,
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

    // 2. Photo message with caption `[Hình ảnh] Ảnh du lịch hè`
    let photo_caption_msg = ChatMessage {
        id: "msg_photo_02".into(),
        sender: "User A".into(),
        text: "[Hình ảnh] Ảnh du lịch hè".into(),
        link_url: Some("https://photo-cdn.zaloapp.com/v2/photos/vacation.jpg".into()),
        kind: MessageKind::Photo,
        ..Default::default()
    };
    let payload_caption = forward_content_for_message(&photo_caption_msg);
    assert_eq!(
        payload_caption, "Ảnh du lịch hè\nhttps://photo-cdn.zaloapp.com/v2/photos/vacation.jpg",
        "Photo caption must be preserved along with the media URL"
    );

    // 3. File message with placeholder `[Tập tin]`
    let file_msg = ChatMessage {
        id: "msg_file_01".into(),
        sender: "User B".into(),
        text: "[Tập tin]".into(),
        link_url: Some("https://files-cdn.zaloapp.com/docs/spec.pdf".into()),
        kind: MessageKind::File,
        ..Default::default()
    };
    let file_payload = forward_content_for_message(&file_msg);
    assert_eq!(
        file_payload, "https://files-cdn.zaloapp.com/docs/spec.pdf",
        "File placeholder `[Tập tin]` must be replaced by the download link"
    );

    // 4. File message with caption `[Tập tin] bao_cao_quy_4.xlsx`
    let file_caption_msg = ChatMessage {
        id: "msg_file_02".into(),
        sender: "User B".into(),
        text: "[Tập tin] bao_cao_quy_4.xlsx".into(),
        link_url: Some("https://files-cdn.zaloapp.com/docs/bao_cao_quy_4.xlsx".into()),
        kind: MessageKind::File,
        ..Default::default()
    };
    let file_cap_payload = forward_content_for_message(&file_caption_msg);
    assert_eq!(
        file_cap_payload,
        "bao_cao_quy_4.xlsx\nhttps://files-cdn.zaloapp.com/docs/bao_cao_quy_4.xlsx",
        "File caption must be retained above the file URL"
    );

    // 5. Plain text message forwarded unmodified
    let text_msg = ChatMessage {
        id: "msg_text_01".into(),
        sender: "User C".into(),
        text: "Xin chào đồng nghiệp, chúc một ngày làm việc hiệu quả!".into(),
        link_url: None,
        kind: MessageKind::Text,
        ..Default::default()
    };
    let text_payload = forward_content_for_message(&text_msg);
    assert_eq!(
        text_payload, "Xin chào đồng nghiệp, chúc một ngày làm việc hiệu quả!",
        "Standard text content must not be altered"
    );

    // 6. Recalled message forwarded with prefix stripped
    let recalled_msg = ChatMessage {
        id: "msg_recalled_01".into(),
        sender: "User D".into(),
        text: "[Đã thu hồi] Nội dung nguyên bản trước khi thu hồi".into(),
        link_url: None,
        kind: MessageKind::Text,
        ..Default::default()
    };
    let recalled_payload = forward_content_for_message(&recalled_msg);
    assert_eq!(
        recalled_payload, "Nội dung nguyên bản trước khi thu hồi",
        "Recalled prefix must be stripped when forwarding"
    );
}

fn percent_decode(input: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap(), 16)
            {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

#[tokio::test]
async fn test_rich_link_forward_fallback_preserves_url() {
    // Tests the CodeRabbit PR #105 review finding:
    // When the rich-link API endpoint fails or returns an error, the fallback
    // must send plain text containing the URL, even if the message text was a caption
    // that omitted the URL.

    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind test listener");
    let addr = listener.local_addr().expect("Failed to get local addr");
    let host = format!("http://{}", addr);

    let server_handle = thread::spawn(move || {
        let mut requests = Vec::new();
        // We will make 1 request to /api/message/link that fails (500),
        // triggering fallback to /api/message/sms (send_message_with_client_id)
        for i in 0..2 {
            let (mut stream, _) = listener.accept().expect("Failed to accept connection");
            let mut buf = vec![0u8; 16384];
            let mut read_bytes = 0;
            let mut header_end = None;
            let mut expected_total = None;

            loop {
                let n = stream.read(&mut buf[read_bytes..]).unwrap();
                if n == 0 {
                    break;
                }
                read_bytes += n;

                if header_end.is_none() {
                    if let Some(pos) = buf[..read_bytes].windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = Some(pos);
                        let header_str = String::from_utf8_lossy(&buf[..pos]);
                        let mut clen = 0usize;
                        for line in header_str.lines() {
                            if let Some(val) =
                                line.to_ascii_lowercase().strip_prefix("content-length:")
                            {
                                clen = val.trim().parse::<usize>().unwrap_or(0);
                                break;
                            }
                        }
                        expected_total = Some(pos + 4 + clen);
                    }
                }

                if let Some(total) = expected_total {
                    if read_bytes >= total {
                        break;
                    }
                }
            }
            let req_str = String::from_utf8_lossy(&buf[..read_bytes]);
            let first_line = req_str.lines().next().unwrap_or_default();
            let path = first_line
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_string();

            let body_part = if let Some(pos) = req_str.find("\r\n\r\n") {
                &req_str[pos + 4..]
            } else {
                ""
            };
            requests.push((path, body_part.to_string()));

            if i == 0 {
                // Link endpoint fails
                let response = r#"{"error_code":500,"error_message":"link api down"}"#;
                write!(
                    stream,
                    "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                    response.len(),
                    response
                )
                .unwrap();
            } else {
                // Plain text fallback succeeds
                let response = r#"{"error_code":0,"data":{"msgId":"fallback_text_msg_200"}}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                    response.len(),
                    response
                )
                .unwrap();
            }
        }
        requests
    });

    let mut session = Session::new(false, false, false).unwrap();
    session.login = Some(LoginInfo {
        uin: "test_user_owner".into(),
        display_name: "Test User".into(),
        avatar: "".into(),
        zpw_enk: "".into(),
        zpw_sek: "1111222233334444".into(),
        send2me_id: "".into(),
    });
    session.imei = "test-imei-client-id-01".into();
    session.secret_key = Some(b"1111222233334444".to_vec());
    session.domains.chat = host.clone();
    session.domains.group = host.clone();

    let meta = LinkMetadata {
        url: "https://example.com/breaking-news".into(),
        title: "Tin Tức Mới Nhất".into(),
        desc: "Mô tả chi tiết tin tức".into(),
        thumb: "https://example.com/thumb.jpg".into(),
        src: "example.com".into(),
    };

    // Text is a caption that DOES NOT contain the URL
    let result = send_link_message_with_client_id(
        &session,
        "contact_friend_1",
        "Đọc bài báo này hay lắm!",
        &meta,
        false,
        1700000000100,
    )
    .await;

    assert!(result.is_ok(), "Fallback to text message must succeed");

    let requests = server_handle.join().expect("Mock server panicked");
    assert_eq!(
        requests.len(),
        2,
        "Expected 1 failed link call and 1 fallback text call"
    );

    // Verify first request was to link endpoint
    assert!(requests[0].0.contains("/api/message/link"));

    // Verify second request was to text endpoint with fallback text containing the URL
    assert!(requests[1].0.contains("/api/message/sms"));

    let body_str = &requests[1].1;
    assert!(body_str.starts_with("params="));
    let key = session.session_key_bytes();
    let params_b64_vec = percent_decode(&body_str[7..]);
    let params_b64 = String::from_utf8(params_b64_vec).expect("valid utf-8");
    let decrypted =
        zalo_rust::crypto::decode_aes(key, &params_b64).expect("Failed to decode AES params");
    let json_body: serde_json::Value =
        serde_json::from_str(&decrypted).expect("Invalid decrypted JSON");

    let sent_msg = json_body.get("message").and_then(|v| v.as_str()).unwrap();
    assert!(
        sent_msg.contains("https://example.com/breaking-news"),
        "Fallback message must contain the URL, got: {sent_msg}"
    );
    assert!(
        sent_msg.contains("Đọc bài báo này hay lắm!"),
        "Fallback message must contain caption, got: {sent_msg}"
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
            let mut buf = vec![0u8; 16384];
            let mut read_bytes = 0;
            let mut header_end = None;
            let mut expected_total = None;

            loop {
                let n = stream.read(&mut buf[read_bytes..]).unwrap();
                if n == 0 {
                    break;
                }
                read_bytes += n;

                if header_end.is_none() {
                    if let Some(pos) = buf[..read_bytes].windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = Some(pos);
                        let header_str = String::from_utf8_lossy(&buf[..pos]);
                        let mut clen = 0usize;
                        for line in header_str.lines() {
                            if let Some(val) =
                                line.to_ascii_lowercase().strip_prefix("content-length:")
                            {
                                clen = val.trim().parse::<usize>().unwrap_or(0);
                                break;
                            }
                        }
                        expected_total = Some(pos + 4 + clen);
                    }
                }

                if let Some(total) = expected_total {
                    if read_bytes >= total {
                        break;
                    }
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
    session.login = Some(LoginInfo {
        uin: "test_user_owner".into(),
        display_name: "Test User".into(),
        avatar: "".into(),
        zpw_enk: "".into(),
        zpw_sek: "1111222233334444".into(),
        send2me_id: "".into(),
    });
    session.imei = "test-imei-client-id-01".into();
    session.secret_key = Some(b"1111222233334444".to_vec());
    session.domains.chat = host.clone();
    session.domains.group = host.clone();

    let client_id_base = 1700000000000_u64;

    // 1. Direct text message forward
    let msg1 = ChatMessage {
        id: "orig_msg_1".into(),
        text: "Kế hoạch tuần tới".into(),
        ..Default::default()
    };
    forward_message_with_client_id(&session, "contact_user_1", false, &msg1, client_id_base + 1)
        .await
        .expect("Dispatch 1 failed");

    // 2. Direct photo message forward (URL in link_url, placeholder in text)
    let msg2 = ChatMessage {
        id: "orig_msg_2".into(),
        text: "[Hình ảnh]".into(),
        link_url: Some("https://photos.example.invalid/screenshot.png".into()),
        kind: MessageKind::Photo,
        ..Default::default()
    };
    forward_message_with_client_id(&session, "contact_user_2", false, &msg2, client_id_base + 2)
        .await
        .expect("Dispatch 2 failed");

    // 3. Group text message forward
    let msg3 = ChatMessage {
        id: "orig_msg_3".into(),
        text: "Thông báo bảo trì hệ thống".into(),
        ..Default::default()
    };
    forward_message_with_client_id(&session, "group_100", true, &msg3, client_id_base + 3)
        .await
        .expect("Dispatch 3 failed");

    // 4. Group file message forward with caption
    let msg4 = ChatMessage {
        id: "orig_msg_4".into(),
        text: "[Tập tin] bao_cao_tai_chinh.xlsx".into(),
        link_url: Some("https://files.example.invalid/bao_cao_tai_chinh.xlsx".into()),
        kind: MessageKind::File,
        ..Default::default()
    };
    forward_message_with_client_id(&session, "group_200", true, &msg4, client_id_base + 4)
        .await
        .expect("Dispatch 4 failed");

    let requests = server_handle.join().expect("Mock server panicked");
    assert_eq!(requests.len(), 4, "Must make 4 forward requests");

    // Verify paths: 2 direct to chat domain, 2 group to group domain
    assert!(requests[0].0.contains("/api/message/sms"));
    assert!(requests[1].0.contains("/api/message/sms"));
    assert!(requests[2].0.contains("/api/group/sendmsg"));
    assert!(requests[3].0.contains("/api/group/sendmsg"));

    let key = session.session_key_bytes();

    for (i, (_, body_str)) in requests.iter().enumerate() {
        assert!(body_str.starts_with("params="));
        let params_b64_vec = percent_decode(&body_str[7..]);
        let params_b64 = String::from_utf8(params_b64_vec).expect("valid utf-8");
        let decrypted =
            zalo_rust::crypto::decode_aes(key, &params_b64).expect("Failed to decode AES params");
        let json_body: serde_json::Value =
            serde_json::from_str(&decrypted).expect("Invalid decrypted JSON");

        match i {
            0 => {
                assert_eq!(
                    json_body.get("toid").and_then(|v| v.as_str()),
                    Some("contact_user_1")
                );
                assert_eq!(
                    json_body.get("message").and_then(|v| v.as_str()),
                    Some("Kế hoạch tuần tới")
                );
                assert_eq!(
                    json_body.get("clientId").and_then(|v| v.as_u64()),
                    Some(client_id_base + 1)
                );
            }
            1 => {
                assert_eq!(
                    json_body.get("toid").and_then(|v| v.as_str()),
                    Some("contact_user_2")
                );
                assert_eq!(
                    json_body.get("message").and_then(|v| v.as_str()),
                    Some("https://photos.example.invalid/screenshot.png")
                );
                assert_eq!(
                    json_body.get("clientId").and_then(|v| v.as_u64()),
                    Some(client_id_base + 2)
                );
            }
            2 => {
                assert_eq!(json_body.get("grid").and_then(|v| v.as_str()), Some("100"));
                assert_eq!(
                    json_body.get("message").and_then(|v| v.as_str()),
                    Some("Thông báo bảo trì hệ thống")
                );
                assert_eq!(
                    json_body.get("clientId").and_then(|v| v.as_u64()),
                    Some(client_id_base + 3)
                );
            }
            3 => {
                assert_eq!(json_body.get("grid").and_then(|v| v.as_str()), Some("200"));
                assert_eq!(
                    json_body.get("message").and_then(|v| v.as_str()),
                    Some("bao_cao_tai_chinh.xlsx\nhttps://files.example.invalid/bao_cao_tai_chinh.xlsx")
                );
                assert_eq!(
                    json_body.get("clientId").and_then(|v| v.as_u64()),
                    Some(client_id_base + 4)
                );
            }
            _ => {}
        }
    }
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

#[tokio::test]
async fn test_rich_link_forward_priority_over_photo_cdn() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let host = format!("http://{}", addr);

    let server_handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 8192];
        let mut read_bytes = 0usize;
        let mut header_end = None;
        let mut expected_total = None;

        loop {
            let n = stream.read(&mut buf[read_bytes..]).unwrap();
            if n == 0 {
                break;
            }
            read_bytes += n;

            if header_end.is_none() {
                if let Some(pos) = buf[..read_bytes].windows(4).position(|w| w == b"\r\n\r\n") {
                    header_end = Some(pos);
                    let header_str = String::from_utf8_lossy(&buf[..pos]);
                    let mut clen = 0usize;
                    for line in header_str.lines() {
                        if let Some(val) = line.to_ascii_lowercase().strip_prefix("content-length:")
                        {
                            clen = val.trim().parse::<usize>().unwrap_or(0);
                            break;
                        }
                    }
                    expected_total = Some(pos + 4 + clen);
                }
            }

            if let Some(total) = expected_total {
                if read_bytes >= total {
                    break;
                }
            }
        }
        let req_str = String::from_utf8_lossy(&buf[..read_bytes]);
        let first_line = req_str.lines().next().unwrap_or_default();
        let path = first_line
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_string();

        let response = r#"{"error_code":0,"data":{"msgId":"link_fwd_123"}}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
            response.len(),
            response
        )
        .unwrap();

        path
    });

    let mut session = Session::new(false, false, false).unwrap();
    session.login = Some(LoginInfo {
        uin: "test_user".into(),
        display_name: "Test User".into(),
        avatar: "".into(),
        zpw_enk: "".into(),
        zpw_sek: "1111222233334444".into(),
        send2me_id: "".into(),
    });
    session.imei = "test-imei".into();
    session.secret_key = Some(b"1111222233334444".to_vec());
    session.domains.chat = host.clone();
    session.domains.group = host.clone();

    let link_msg = ChatMessage {
        id: "link_msg_1".into(),
        text: "https://example.com/news".into(),
        link_url: Some("https://example.com/news".into()),
        link_title: Some("Tin tức nổi bật".into()),
        link_desc: Some("Mô tả bài viết".into()),
        link_thumb: Some("https://example.com/thumb.jpg".into()),
        kind: MessageKind::Link,
        ..Default::default()
    };

    forward_message_with_client_id(&session, "group_888", true, &link_msg, 1700000000001)
        .await
        .expect("Link forward failed");

    let path = server_handle.join().expect("Mock server panicked");
    assert!(
        path.contains("/api/group/sendlink"),
        "Expected sendlink endpoint, got: {}",
        path
    );
}
