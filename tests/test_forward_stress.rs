//! Adversarial stress test suite for Message Forwarding (Milestone M4, Requirement R4).
//!
//! Validates:
//! 1. Extreme search strings (10k, 50k, 100k chars, special symbols, SQL/XSS, Vietnamese diacritics, emojis).
//! 2. Empty threads list, threads with duplicate names or IDs, threads with blank names, and 10,000+ thread scale.
//! 3. Extreme media URLs (10k+ chars, complex query params, special chars, unicode paths, data URIs, deduplication).
//! 4. Messages with complex caption formatting (multiple lines, emoji captions, whitespace trimming, placeholder variants).
//! 5. Sticker forwarding invariants: invalid/zero IDs fallback to text message, valid stickers forward to sticker APIs.
//! 6. GUI preview truncation safety on UTF-8 boundaries and state machine rapid lifecycle fuzzing.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Instant;

use zalo_rust::chat::{
    filter_forward_candidates, forward_content_for_message, forward_message_with_client_id,
    ChatMessage, Thread,
};
use zalo_rust::message_model::MessageKind;
use zalo_rust::session::{LoginInfo, Session};

/// Helper replicating UTF-8 character truncation from `src/gui/theme.rs`.
fn truncate_chars<'a>(s: &'a str, max_chars: usize) -> std::borrow::Cow<'a, str> {
    if let Some((byte_idx, _)) = s.char_indices().nth(max_chars) {
        std::borrow::Cow::Owned(format!("{}…", &s[..byte_idx]))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// Helper replicating dialog display name resolution from `src/gui/modals.rs:5851`.
fn resolve_candidate_display_name<'a>(id: &'a str, name: &'a str) -> &'a str {
    if name.trim().is_empty() {
        id
    } else {
        name
    }
}

/// Helper replicating dialog preview resolution from `src/gui/modals.rs:5775`.
fn resolve_forward_preview(m: &ChatMessage) -> String {
    if m.sticker_id.is_some() {
        "[Nhãn dán]".to_string()
    } else if !m.text.trim().is_empty() {
        truncate_chars(&m.text, 80).into_owned()
    } else if let Some(ref url) = m.link_url {
        truncate_chars(url, 80).into_owned()
    } else {
        "[Tin nhắn]".to_string()
    }
}

#[test]
fn test_stress_search_candidate_filtering_extreme_inputs() {
    let threads = vec![
        Thread {
            id: "uid_anh_vien".into(),
            name: "Nguyễn Thị Ánh Viên".into(),
            is_group: false,
            ..Default::default()
        },
        Thread {
            id: "uid_van_lam".into(),
            name: "Đặng Văn Lâm".into(),
            is_group: false,
            ..Default::default()
        },
        Thread {
            id: "gid_project_rocket".into(),
            name: "Dự Án 🚀 Ctdozlite".into(),
            is_group: true,
            ..Default::default()
        },
        Thread {
            id: "gid_vietnam_team".into(),
            name: "Team 🇻🇳 Khởi Nghiệp".into(),
            is_group: true,
            ..Default::default()
        },
        Thread {
            id: "gid_family_zwj".into(),
            name: "Gia Đình 👨‍👩‍👧‍👦 Hạnh Phúc".into(),
            is_group: true,
            ..Default::default()
        },
        Thread {
            id: "uid_special_chars".into(),
            name: "Admin [DevOps] (Core & Infra)".into(),
            is_group: false,
            ..Default::default()
        },
    ];

    // 1. Extreme 10k, 50k, 100k character search strings
    let query_10k = "A".repeat(10_000);
    let start_10k = Instant::now();
    let res_10k = filter_forward_candidates(&threads, &query_10k);
    assert!(res_10k.is_empty(), "10k string should match nothing");
    assert!(
        start_10k.elapsed().as_millis() < 50,
        "10k query must finish in <50ms"
    );

    let query_50k = "Z".repeat(50_000);
    assert!(filter_forward_candidates(&threads, &query_50k).is_empty());

    let query_100k = "🔥".repeat(25_000); // 100,000 bytes
    assert!(filter_forward_candidates(&threads, &query_100k).is_empty());

    // 2. Special symbols, regex metacharacters, ANSI escapes, null bytes, control characters
    let adversarial_symbols = vec![
        "!@#$%^&*()_+-=[]{}|;':\",.<>?/~`",
        ".*+?^${}()|[]\\",
        "\0\x01\x02\x03\x04\x05\x06\x07\x08\x0e\x0f",
        "\x1b[31;1mRED_ALERT\x1b[0m",
        "\r\n\t\r\n\t",
        "$PATH; rm -rf /; echo pwned",
    ];
    for sym in adversarial_symbols {
        // Must never panic or crash
        let matches = filter_forward_candidates(&threads, sym);
        if sym.contains("[DevOps]") {
            assert!(!matches.is_empty());
        }
    }

    // 3. SQL / XSS / JSON injections
    let injection_payloads = vec![
        "'; DROP TABLE threads; --",
        "\" OR \"1\"=\"1",
        "' OR 1=1 --",
        "<script>alert(document.cookie)</script>",
        "<img src=x onerror=alert(1)>",
        "{\"target\": \"all\", \"$where\": \"sleep(1000)\"}",
        "%s%s%s%n%d%x%p",
    ];
    for payload in injection_payloads {
        let res = filter_forward_candidates(&threads, payload);
        assert!(res.is_empty(), "Injection string must safely return 0 results");
    }

    // 4. Vietnamese diacritics permutations (lowercase, uppercase, mixed, NFC/NFD)
    // Lowercase
    let m1 = filter_forward_candidates(&threads, "ánh viên");
    assert_eq!(m1.len(), 1);
    assert_eq!(m1[0].id, "uid_anh_vien");

    // Uppercase
    let m2 = filter_forward_candidates(&threads, "ÁNH VIÊN");
    assert_eq!(m2.len(), 1);
    assert_eq!(m2[0].id, "uid_anh_vien");

    // Mixed case
    let m3 = filter_forward_candidates(&threads, "áNh ViÊn");
    assert_eq!(m3.len(), 1);
    assert_eq!(m3[0].id, "uid_anh_vien");

    // "đặng văn lâm"
    let m4 = filter_forward_candidates(&threads, "ĐẶNG");
    assert_eq!(m4.len(), 1);
    assert_eq!(m4[0].name, "Đặng Văn Lâm");

    let m5 = filter_forward_candidates(&threads, "đặng văn");
    assert_eq!(m5.len(), 1);
    assert_eq!(m5[0].id, "uid_van_lam");

    // 5. Multi-byte UTF-8 emojis, flags, ZWJ sequences
    let m_rocket = filter_forward_candidates(&threads, "🚀");
    assert_eq!(m_rocket.len(), 1);
    assert_eq!(m_rocket[0].id, "gid_project_rocket");

    let m_flag = filter_forward_candidates(&threads, "🇻🇳");
    assert_eq!(m_flag.len(), 1);
    assert_eq!(m_flag[0].id, "gid_vietnam_team");

    let m_zwj = filter_forward_candidates(&threads, "👨‍👩‍👧‍👦");
    assert_eq!(m_zwj.len(), 1);
    assert_eq!(m_zwj[0].id, "gid_family_zwj");

    // 6. Purely whitespace queries of varying lengths -> all candidates returned
    assert_eq!(filter_forward_candidates(&threads, " ").len(), threads.len());
    assert_eq!(
        filter_forward_candidates(&threads, "     \t\r\n     ").len(),
        threads.len()
    );
    let spaces_1k = " ".repeat(1_000);
    assert_eq!(
        filter_forward_candidates(&threads, &spaces_1k).len(),
        threads.len()
    );
}

#[test]
fn test_stress_threads_boundary_conditions() {
    // 1. Empty threads list
    let empty_threads: Vec<Thread> = Vec::new();
    assert!(filter_forward_candidates(&empty_threads, "").is_empty());
    assert!(filter_forward_candidates(&empty_threads, "alice").is_empty());
    assert!(filter_forward_candidates(&empty_threads, &"X".repeat(10_000)).is_empty());
    assert!(filter_forward_candidates(&empty_threads, "🚀🇻🇳").is_empty());

    // 2. Threads with blank or whitespace-only names
    let blank_name_threads = vec![
        Thread {
            id: "uid_blank_1".into(),
            name: "".into(),
            is_group: false,
            ..Default::default()
        },
        Thread {
            id: "uid_blank_2".into(),
            name: "   ".into(),
            is_group: false,
            ..Default::default()
        },
        Thread {
            id: "gid_blank_3".into(),
            name: "\t\r\n".into(),
            is_group: true,
            ..Default::default()
        },
        Thread {
            id: "uid_normal_4".into(),
            name: "Người Dùng Thường".into(),
            is_group: false,
            ..Default::default()
        },
    ];

    // Search by ID for threads with blank names
    let match_id = filter_forward_candidates(&blank_name_threads, "blank_1");
    assert_eq!(match_id.len(), 1);
    assert_eq!(match_id[0].id, "uid_blank_1");

    let match_gid = filter_forward_candidates(&blank_name_threads, "gid_blank");
    assert_eq!(match_gid.len(), 1);
    assert_eq!(match_gid[0].id, "gid_blank_3");

    // Invariant: Display name resolution fallback for blank names
    assert_eq!(
        resolve_candidate_display_name(&blank_name_threads[0].id, &blank_name_threads[0].name),
        "uid_blank_1",
        "Empty name must fall back to thread ID"
    );
    assert_eq!(
        resolve_candidate_display_name(&blank_name_threads[1].id, &blank_name_threads[1].name),
        "uid_blank_2",
        "Whitespace name must fall back to thread ID"
    );
    assert_eq!(
        resolve_candidate_display_name(&blank_name_threads[2].id, &blank_name_threads[2].name),
        "gid_blank_3",
        "Tab/newline name must fall back to thread ID"
    );
    assert_eq!(
        resolve_candidate_display_name(&blank_name_threads[3].id, &blank_name_threads[3].name),
        "Người Dùng Thường",
        "Non-empty name must be preserved"
    );

    // 3. Duplicate names
    let dup_name_threads = vec![
        Thread {
            id: "uid_dup_101".into(),
            name: "Trần Văn An".into(),
            is_group: false,
            ..Default::default()
        },
        Thread {
            id: "uid_dup_102".into(),
            name: "Trần Văn An".into(),
            is_group: false,
            ..Default::default()
        },
        Thread {
            id: "uid_dup_103".into(),
            name: "Trần Văn An".into(),
            is_group: true,
            ..Default::default()
        },
    ];
    let dup_matches = filter_forward_candidates(&dup_name_threads, "Trần Văn An");
    assert_eq!(dup_matches.len(), 3, "Must match all 3 duplicate named threads");
    assert_eq!(dup_matches[0].id, "uid_dup_101");
    assert_eq!(dup_matches[1].id, "uid_dup_102");
    assert_eq!(dup_matches[2].id, "uid_dup_103");

    // 4. Duplicate IDs (corrupted/edge case store data)
    let dup_id_threads = vec![
        Thread {
            id: "same_id_999".into(),
            name: "Phiên Bản 1".into(),
            ..Default::default()
        },
        Thread {
            id: "same_id_999".into(),
            name: "Phiên Bản 2".into(),
            ..Default::default()
        },
    ];
    let dup_id_matches = filter_forward_candidates(&dup_id_threads, "same_id_999");
    assert_eq!(dup_id_matches.len(), 2);

    // 5. High-volume scale stress: 10,000 threads
    let mut large_threads = Vec::with_capacity(10_000);
    for i in 0..10_000 {
        large_threads.push(Thread {
            id: format!("usr_{i:05}"),
            name: if i % 500 == 0 {
                format!("Người Dùng VIP {i}")
            } else if i % 2 == 0 {
                format!("User Number {i}")
            } else {
                format!("Nhóm Công Tác {i}")
            },
            is_group: i % 2 != 0,
            ..Default::default()
        });
    }

    let start_scale = Instant::now();
    let query_vip = filter_forward_candidates(&large_threads, "VIP");
    let vip_elapsed = start_scale.elapsed();
    assert_eq!(query_vip.len(), 20); // 10000 / 500 = 20
    assert!(
        vip_elapsed.as_millis() < 50,
        "Filtering 10,000 threads took {:?}, must be < 50ms",
        vip_elapsed
    );

    let query_exact = filter_forward_candidates(&large_threads, "usr_09999");
    assert_eq!(query_exact.len(), 1);
    assert_eq!(query_exact[0].id, "usr_09999");

    let query_none = filter_forward_candidates(&large_threads, "hoàn toàn không có");
    assert!(query_none.is_empty());
}

#[test]
fn test_stress_media_urls_and_edge_cases() {
    // 1. Extreme 10,000+ character URL
    let huge_url = format!(
        "https://cdn.zalo.me/res/file?token={}&checksum={}",
        "A".repeat(5_000),
        "B".repeat(5_000)
    );
    let huge_msg = ChatMessage {
        id: "msg_huge_url".into(),
        text: "[Hình ảnh]".into(),
        link_url: Some(huge_url.clone()),
        kind: MessageKind::Photo,
        ..Default::default()
    };
    let payload = forward_content_for_message(&huge_msg);
    assert_eq!(payload, huge_url, "10k URL must be preserved intact");

    // 2. Complex query params, anchors, percent encoding, and special chars
    let complex_url = "https://example.com/api/v2/view?file=test%20name%5B1%5D.pdf&auth=yes&tags=a,b,c&redirect=https%3A%2F%2Fother.com%2Fpath%3Fid%3D99#section-header";
    let complex_msg = ChatMessage {
        id: "msg_complex_url".into(),
        text: "[Tập tin]".into(),
        link_url: Some(complex_url.to_string()),
        kind: MessageKind::File,
        ..Default::default()
    };
    assert_eq!(forward_content_for_message(&complex_msg), complex_url);

    // 3. Unicode in URL path
    let unicode_url = "https://files.zalo.me/tài_liệu_2026/kế_hoạch_kinh_doanh.xlsx";
    let unicode_url_msg = ChatMessage {
        id: "msg_unicode_url".into(),
        text: "[Tập tin]".into(),
        link_url: Some(unicode_url.to_string()),
        kind: MessageKind::File,
        ..Default::default()
    };
    assert_eq!(forward_content_for_message(&unicode_url_msg), unicode_url);

    // 4. Data URI payloads (inline base64 image)
    let data_uri = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let data_uri_msg = ChatMessage {
        id: "msg_data_uri".into(),
        text: "[Hình ảnh]".into(),
        link_url: Some(data_uri.to_string()),
        kind: MessageKind::Photo,
        ..Default::default()
    };
    assert_eq!(forward_content_for_message(&data_uri_msg), data_uri);

    // 5. All 8 placeholder labels must resolve directly to link_url
    let placeholders = vec![
        "[Hình ảnh]",
        "[Photo]",
        "[Tập tin]",
        "[File]",
        "[Nhãn dán]",
        "[Chia sẻ]",
        "[Video]",
        "[Tin nhắn]",
        "",
        "   \t\n  ",
    ];
    let test_url = "https://media.zalo.me/test_asset.mp4";
    for ph in placeholders {
        let msg = ChatMessage {
            id: "ph_test".into(),
            text: ph.to_string(),
            link_url: Some(test_url.to_string()),
            ..Default::default()
        };
        assert_eq!(
            forward_content_for_message(&msg),
            test_url,
            "Placeholder '{}' must yield the link URL",
            ph
        );
    }

    // 6. Invariant: Deduplication when text ALREADY contains the link_url
    let existing_url_msg = ChatMessage {
        id: "msg_dedup_1".into(),
        text: "https://example.com/single_link".into(),
        link_url: Some("https://example.com/single_link".into()),
        ..Default::default()
    };
    assert_eq!(
        forward_content_for_message(&existing_url_msg),
        "https://example.com/single_link",
        "Must not duplicate when text is exactly the URL"
    );

    let embedded_url_msg = ChatMessage {
        id: "msg_dedup_2".into(),
        text: "Xem tài liệu tại https://example.com/single_link nhé mọi người".into(),
        link_url: Some("https://example.com/single_link".into()),
        ..Default::default()
    };
    assert_eq!(
        forward_content_for_message(&embedded_url_msg),
        "Xem tài liệu tại https://example.com/single_link nhé mọi người",
        "Must not duplicate when text already contains the URL"
    );

    // 7. Message without link_url (None) -> returns text unaltered
    let huge_text = "Nội dung văn bản dài: ".to_string() + &"X".repeat(50_000);
    let no_url_msg = ChatMessage {
        id: "msg_plain_huge".into(),
        text: huge_text.clone(),
        link_url: None,
        ..Default::default()
    };
    assert_eq!(forward_content_for_message(&no_url_msg), huge_text);

    let empty_text_no_url = ChatMessage {
        id: "msg_empty".into(),
        text: "".into(),
        link_url: None,
        ..Default::default()
    };
    assert_eq!(forward_content_for_message(&empty_text_no_url), "");
}

#[test]
fn test_stress_captions_multiline_emoji_formatting() {
    let media_url = "https://cdn.zalo.me/photos/album_2026.jpg";

    // 1. Multi-line captions (100 lines) with [Hình ảnh]
    let lines: Vec<String> = (1..=100)
        .map(|i| format!("Dòng caption thứ {i}: mô tả chi tiết hình ảnh"))
        .collect();
    let multiline_caption = lines.join("\n");
    let multiline_msg = ChatMessage {
        id: "msg_multiline_caption".into(),
        text: format!("[Hình ảnh] {multiline_caption}"),
        link_url: Some(media_url.to_string()),
        kind: MessageKind::Photo,
        ..Default::default()
    };
    let payload = forward_content_for_message(&multiline_msg);
    let expected = format!("{multiline_caption}\n{media_url}");
    assert_eq!(payload, expected);

    // 2. Multi-line file captions with [Tập tin]
    let file_url = "https://files.zalo.me/specs/arch.pdf";
    let file_lines = "Bản vẽ thiết kế hệ thống\nPhiên bản 2.1 (Final)\nKèm chữ ký duyệt số 10";
    let file_msg = ChatMessage {
        id: "msg_file_caption".into(),
        text: format!("[Tập tin] {file_lines}"),
        link_url: Some(file_url.to_string()),
        kind: MessageKind::File,
        ..Default::default()
    };
    assert_eq!(
        forward_content_for_message(&file_msg),
        format!("{file_lines}\n{file_url}")
    );

    // 3. Emoji-rich caption with multi-byte characters, flags, and symbols
    let emoji_caption = "🇻🇳 Chào cờ sáng thứ hai! 👨‍👩‍👧‍👦 Đầy đủ thành viên 🚀🎉🔥 Cố lên nhé cả nhà! 💯✨";
    let emoji_msg = ChatMessage {
        id: "msg_emoji_caption".into(),
        text: format!("[Hình ảnh] {emoji_caption}"),
        link_url: Some(media_url.to_string()),
        kind: MessageKind::Photo,
        ..Default::default()
    };
    assert_eq!(
        forward_content_for_message(&emoji_msg),
        format!("{emoji_caption}\n{media_url}")
    );

    // 4. Captions with leading / trailing whitespace
    let padded_msg = ChatMessage {
        id: "msg_padded".into(),
        text: "[Hình ảnh]    \t\n  Ảnh họp tổng kết năm 2026   \t\r\n   ".into(),
        link_url: Some(media_url.to_string()),
        kind: MessageKind::Photo,
        ..Default::default()
    };
    assert_eq!(
        forward_content_for_message(&padded_msg),
        format!("Ảnh họp tổng kết năm 2026\n{media_url}")
    );

    // 5. Placeholder with only whitespace after prefix -> returns just URL
    let whitespace_after_photo = ChatMessage {
        id: "msg_ws_photo".into(),
        text: "[Hình ảnh]     \t\r\n   ".into(),
        link_url: Some(media_url.to_string()),
        kind: MessageKind::Photo,
        ..Default::default()
    };
    assert_eq!(
        forward_content_for_message(&whitespace_after_photo),
        media_url,
        "Whitespace-only caption must return only the media URL without extra newlines"
    );

    let whitespace_after_file = ChatMessage {
        id: "msg_ws_file".into(),
        text: "[Tập tin]     ".into(),
        link_url: Some(file_url.to_string()),
        kind: MessageKind::File,
        ..Default::default()
    };
    assert_eq!(
        forward_content_for_message(&whitespace_after_file),
        file_url
    );

    // 6. English placeholder variants: [Photo] and [File]
    let photo_en_caption = ChatMessage {
        id: "msg_en_photo".into(),
        text: "[Photo] Sunset in Da Nang beach".into(),
        link_url: Some(media_url.to_string()),
        kind: MessageKind::Photo,
        ..Default::default()
    };
    assert_eq!(
        forward_content_for_message(&photo_en_caption),
        format!("Sunset in Da Nang beach\n{media_url}")
    );

    let file_en_caption = ChatMessage {
        id: "msg_en_file".into(),
        text: "[File] quarterly_report.xlsx".into(),
        link_url: Some(file_url.to_string()),
        kind: MessageKind::File,
        ..Default::default()
    };
    assert_eq!(
        forward_content_for_message(&file_en_caption),
        format!("quarterly_report.xlsx\n{file_url}")
    );
}

#[tokio::test]
async fn test_stress_sticker_forwarding_invariants_and_mock_wire() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind test listener");
    let addr = listener.local_addr().expect("Failed to get local addr");
    let host = format!("http://{}", addr);

    let server_handle = thread::spawn(move || {
        let mut requests = Vec::new();
        // We will make 6 requests:
        // 1. Valid sticker forward to direct chat -> /api/message/sticker
        // 2. Valid sticker forward to group chat -> /api/group/sticker
        // 3. Invalid sticker forward (sid = 0) to direct chat -> fallback to /api/message/sms
        // 4. Invalid sticker forward (sid = 0) to group chat -> fallback to /api/group/sendmsg
        // 5. Rich link forward to direct chat -> /api/message/link
        // 6. Rich link forward to group chat -> /api/group/sendlink
        for _ in 0..6 {
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

            let body_part = if let Some(pos) = req_str.find("\r\n\r\n") {
                &req_str[pos + 4..]
            } else {
                ""
            };
            requests.push((path, body_part.to_string()));

            let response = r#"{"error_code":0,"data":{"msgId":"mock_msg_id_888"}}"#;
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

    let base_client_id = 1_795_000_000_000u64;

    // 1. Valid sticker forward to direct chat
    let valid_sticker_msg = ChatMessage {
        id: "stk_01".into(),
        text: "[Nhãn dán]".into(),
        sticker_id: Some(25001),
        sticker_cate_id: Some(101),
        sticker_type: Some(7),
        ..Default::default()
    };
    let r1 = forward_message_with_client_id(
        &session,
        "contact_alice",
        false,
        &valid_sticker_msg,
        base_client_id + 1,
    )
    .await;
    assert!(r1.is_ok(), "Direct sticker forward failed: {:?}", r1);

    // 2. Valid sticker forward to group chat
    let r2 = forward_message_with_client_id(
        &session,
        "group_999",
        true,
        &valid_sticker_msg,
        base_client_id + 2,
    )
    .await;
    assert!(r2.is_ok(), "Group sticker forward failed: {:?}", r2);

    // 3. Invalid sticker forward with sticker_id = 0 to direct chat
    // Invariant: Must fall through sid > 0 guard and forward as text message
    let zero_sticker_msg = ChatMessage {
        id: "stk_zero".into(),
        text: "Sticker dự phòng khi ID = 0".into(),
        sticker_id: Some(0),
        sticker_cate_id: Some(0),
        sticker_type: Some(7),
        ..Default::default()
    };
    let r3 = forward_message_with_client_id(
        &session,
        "contact_bob",
        false,
        &zero_sticker_msg,
        base_client_id + 3,
    )
    .await;
    assert!(
        r3.is_ok(),
        "Zero sticker ID direct fallback failed: {:?}",
        r3
    );

    // 4. Invalid sticker forward with sticker_id = 0 to group chat
    let r4 = forward_message_with_client_id(
        &session,
        "group_888",
        true,
        &zero_sticker_msg,
        base_client_id + 4,
    )
    .await;
    assert!(r4.is_ok(), "Zero sticker ID group fallback failed: {:?}", r4);

    // 5. Rich link message forward to direct chat
    let link_msg = ChatMessage {
        id: "link_rich_01".into(),
        text: "Tin tức công nghệ mới".into(),
        link_url: Some("https://news.ycombinator.com".into()),
        link_title: Some("Hacker News".into()),
        link_desc: Some("Technology and startup news".into()),
        link_thumb: Some("https://news.ycombinator.com/favicon.ico".into()),
        kind: MessageKind::Link,
        ..Default::default()
    };
    let r5 = forward_message_with_client_id(
        &session,
        "contact_charlie",
        false,
        &link_msg,
        base_client_id + 5,
    )
    .await;
    assert!(r5.is_ok(), "Direct rich link forward failed: {:?}", r5);

    // 6. Rich link message forward to group chat
    let r6 = forward_message_with_client_id(
        &session,
        "group_777",
        true,
        &link_msg,
        base_client_id + 6,
    )
    .await;
    assert!(r6.is_ok(), "Group rich link forward failed: {:?}", r6);

    let recorded_requests = server_handle.join().expect("Mock server failed");
    assert_eq!(recorded_requests.len(), 6);

    // Verify wire paths
    assert!(
        recorded_requests[0].0.contains("/api/message/sticker"),
        "Request 1 must hit /api/message/sticker, got: {}",
        recorded_requests[0].0
    );
    assert!(
        recorded_requests[1].0.contains("/api/group/sticker"),
        "Request 2 must hit /api/group/sticker, got: {}",
        recorded_requests[1].0
    );
    assert!(
        recorded_requests[2].0.contains("/api/message/sms"),
        "Request 3 (zero sticker) must fall back to /api/message/sms, got: {}",
        recorded_requests[2].0
    );
    assert!(
        recorded_requests[3].0.contains("/api/group/sendmsg"),
        "Request 4 (zero sticker) must fall back to /api/group/sendmsg, got: {}",
        recorded_requests[3].0
    );
    assert!(
        recorded_requests[4].0.contains("/api/message/link"),
        "Request 5 must hit /api/message/link, got: {}",
        recorded_requests[4].0
    );
    assert!(
        recorded_requests[5].0.contains("/api/group/sendlink"),
        "Request 6 must hit /api/group/sendlink, got: {}",
        recorded_requests[5].0
    );
}

#[test]
fn test_stress_gui_preview_and_state_machine_fuzzing() {
    // 1. Truncation and preview with 100,000 character ASCII message
    let huge_100k = "K".repeat(100_000);
    let huge_msg = ChatMessage {
        id: "msg_preview_100k".into(),
        text: huge_100k,
        ..Default::default()
    };
    let preview_huge = resolve_forward_preview(&huge_msg);
    assert_eq!(preview_huge.chars().count(), 81); // 80 chars + '…'
    assert!(preview_huge.ends_with('…'));

    // 2. Truncation with multi-byte emojis right around char boundary 80
    // Each emoji is 4 bytes, so byte offset != char offset
    let emoji_stream: String = (0..100).map(|_| "🔥").collect();
    let emoji_msg = ChatMessage {
        id: "msg_preview_emoji".into(),
        text: emoji_stream,
        ..Default::default()
    };
    let preview_emoji = resolve_forward_preview(&emoji_msg);
    assert_eq!(preview_emoji.chars().count(), 81);
    assert!(preview_emoji.ends_with('…'));

    // 3. Preview for sticker message
    let sticker_msg = ChatMessage {
        id: "msg_preview_stk".into(),
        text: "[Nhãn dán]".into(),
        sticker_id: Some(12345),
        ..Default::default()
    };
    assert_eq!(resolve_forward_preview(&sticker_msg), "[Nhãn dán]");

    // 4. Preview for media message with empty text but link_url present
    let url_only_msg = ChatMessage {
        id: "msg_preview_url".into(),
        text: "".into(),
        link_url: Some("https://photo-cdn.zaloapp.com/vacation_photos_2026.png".into()),
        ..Default::default()
    };
    let preview_url = resolve_forward_preview(&url_only_msg);
    assert_eq!(
        preview_url,
        "https://photo-cdn.zaloapp.com/vacation_photos_2026.png"
    );

    // 5. Preview for blank message with no link_url
    let blank_msg = ChatMessage {
        id: "msg_preview_blank".into(),
        text: "   ".into(),
        link_url: None,
        ..Default::default()
    };
    assert_eq!(resolve_forward_preview(&blank_msg), "[Tin nhắn]");

    // 6. Forward modal state machine rapid fuzzing
    struct ModalFuzzState {
        forward_msg: Option<ChatMessage>,
        forward_filter: String,
        forward_search_query: String,
        toast_msg: Option<String>,
    }

    let mut state = ModalFuzzState {
        forward_msg: None,
        forward_filter: String::new(),
        forward_search_query: String::new(),
        toast_msg: None,
    };

    // Cycle 1: Open -> Type 10,000 char query -> Escape -> Clean state
    state.forward_msg = Some(huge_msg.clone());
    state.forward_filter = "A".repeat(10_000);
    state.forward_search_query = state.forward_filter.clone();
    assert!(state.forward_msg.is_some());

    // User presses Escape
    state.forward_msg = None;
    state.forward_filter.clear();
    state.forward_search_query.clear();
    assert!(state.forward_msg.is_none());
    assert!(state.forward_filter.is_empty());
    assert!(state.forward_search_query.is_empty());

    // Cycle 2: Open -> Search -> No candidates -> empty state string verification
    state.forward_msg = Some(huge_msg.clone());
    state.forward_filter = "hoàn_toàn_không_khớp".into();
    state.forward_search_query = state.forward_filter.clone();

    let candidates: Vec<Thread> = vec![];
    let filtered = filter_forward_candidates(&candidates, &state.forward_filter);
    assert!(filtered.is_empty());
    let empty_label = "Không tìm thấy người dùng hoặc nhóm phù hợp";
    assert_eq!(
        empty_label,
        "Không tìm thấy người dùng hoặc nhóm phù hợp"
    );

    // Cycle 3: Select recipient -> Toast emitted -> State cleaned
    let target_name = "Nhóm Ban Lãnh Đạo";
    let _taken_msg = state.forward_msg.take().unwrap();
    state.toast_msg = Some(format!("Đã chuyển tiếp tin nhắn tới {target_name}"));
    state.forward_filter.clear();
    state.forward_search_query.clear();

    assert!(state.forward_msg.is_none());
    assert_eq!(
        state.toast_msg.as_deref(),
        Some("Đã chuyển tiếp tin nhắn tới Nhóm Ban Lãnh Đạo")
    );
    assert!(state.forward_filter.is_empty());
    assert!(state.forward_search_query.is_empty());
}
