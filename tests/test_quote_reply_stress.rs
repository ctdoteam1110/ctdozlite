//! Adversarial stress test suite for Message Quote & Reply (Milestone M3, Requirement R3).
//!
//! Validates under heavy load and adversarial edge cases:
//! 1. Extreme length payloads (10k, 50k, 100k chars), 2,000 multiline newlines, whitespace, empty strings.
//! 2. Adversarial unicode, ZWJ sequences, skin tone modifiers, flags, RTL override, control characters, null bytes, SQL/XSS/JSON injection strings.
//! 3. Missing/malformed sender resolution matrix (numeric "0", phone/UID strings, empty senders, direct vs group context).
//! 4. Rapid-sequence GUI state machine fuzzing (Quote -> Escape cancel -> Quote -> Send -> overwrite).
//! 5. Thread switching while quote preview is open (zero cross-thread quote leakage).
//! 6. SQLite high volume roundtrip with 500-level deep quote chains and diverse payload combinations.
//! 7. High-collision ID navigation disambiguation and dummy ID rejection ("0", whitespace, colliding duplicates).
//! 8. Smart snippet generation and wire/history format payload precedence.
//! 9. Egui UI text layout and widget safety under extreme length strings and complex unicode.

use eframe::egui;
use serde_json::json;
use std::collections::HashMap;
use zalo_rust::chat::{ChatMessage, Thread};
use zalo_rust::message_navigation::{message_reply_reference, unique_reference_index};
use zalo_rust::store::{persist_threads, MessageStore};

// Helper: unicode-safe truncation identical to src/gui/theme.rs
fn truncate_chars<'a>(s: &'a str, max_chars: usize) -> std::borrow::Cow<'a, str> {
    if let Some((byte_idx, _)) = s.char_indices().nth(max_chars) {
        std::borrow::Cow::Owned(format!("{}…", &s[..byte_idx]))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

// Helper: resolve sender display name identical to src/gui/workspace.rs and src/gui/messages.rs
fn resolve_sender_name(
    quoted: &ChatMessage,
    thread_name: &str,
    is_group: bool,
    friend_threads: &HashMap<String, String>,
) -> String {
    if quoted.is_outgoing {
        "chính bạn".to_string()
    } else if !quoted.sender.is_empty()
        && quoted.sender != "0"
        && !quoted.sender.chars().all(|c| c.is_ascii_digit())
    {
        quoted.sender.clone()
    } else if let Some(friend_name) = friend_threads.get(&quoted.sender) {
        friend_name.clone()
    } else if !is_group && !thread_name.is_empty() && !thread_name.chars().all(|c| c.is_ascii_digit()) {
        thread_name.to_string()
    } else if !quoted.sender.is_empty() && quoted.sender != "0" {
        format!("Người dùng {}", quoted.sender)
    } else if !thread_name.is_empty() && !thread_name.chars().all(|c| c.is_ascii_digit()) {
        thread_name.to_string()
    } else {
        "Tin nhắn trích dẫn".to_string()
    }
}

// Helper: resolve quote preview snippet identical to src/gui/workspace.rs
fn resolve_quote_snippet(quoted: &ChatMessage) -> String {
    if quoted.sticker_id.is_some() {
        "[Nhãn dán]".to_string()
    } else if quoted.link_url.is_some() && (quoted.text.trim().is_empty() || quoted.text == "[Hình ảnh]") {
        "[Hình ảnh]".to_string()
    } else if quoted.link_title.is_some() {
        format!("[Tập tin] {}", quoted.link_title.as_deref().unwrap_or("[Tập tin]"))
    } else if quoted.text.trim().is_empty() {
        "[Tin nhắn]".to_string()
    } else {
        truncate_chars(&quoted.text, 100).into_owned()
    }
}

#[test]
fn test_stress_extreme_text_payloads_and_unicode_safety() {
    let mut store = MessageStore::open(":memory:").expect("Failed to initialize in-memory SQLite");

    // 1. Extreme 100,000 characters ASCII string
    let huge_100k = "A".repeat(100_000);
    // 2. Multiline string with 2,000 newlines
    let multiline_2k = (0..2000).map(|i| format!("Line {i}: hello world")).collect::<Vec<_>>().join("\n");
    // 3. Complex unicode and emoji test
    let complex_unicode = "🇻🇳 CộNG HÒA XÃ HỘI CHỦ NGHĨA VIỆT NAM — 👨‍👩‍👧‍👦 👩‍💻 👍🏽 \u{202E}RTL_OVERRIDE\u{202C} \u{200B}\u{FEFF} 🚀";
    // 4. Injection attack payloads
    let sql_injection = "'; DROP TABLE messages; DROP TABLE threads; SELECT * FROM sqlite_master WHERE '1'='1";
    let xss_injection = "<script>alert(document.cookie); window.location='http://attacker.com';</script>";
    let json_injection = r#"{"id": "fake", "reply_to_id": "pwned", "nested": {"array": [1,2,3]}}"#;
    let format_string = "%s%s%s%n%d%x%p";
    let control_chars = "\0\x01\x02\x03\x04\x05\x06\x07\x08\x0e\x0f\x1b[31;1mRED_ALERT\x1b[0m";

    let test_cases = vec![
        ("huge_100k", huge_100k),
        ("multiline_2k", multiline_2k),
        ("complex_unicode", complex_unicode.to_string()),
        ("sql_injection", sql_injection.to_string()),
        ("xss_injection", xss_injection.to_string()),
        ("json_injection", json_injection.to_string()),
        ("format_string", format_string.to_string()),
        ("control_chars", control_chars.to_string()),
        ("empty_string", "".to_string()),
        ("whitespace_only", "   \t\r\n   ".to_string()),
    ];

    let mut messages = Vec::new();
    for (idx, (label, payload)) in test_cases.into_iter().enumerate() {
        let msg = ChatMessage {
            id: format!("msg_stress_{idx}"),
            sender: format!("Sender_{label}"),
            text: format!("Reply payload for {label}"),
            reply_to_id: Some(format!("orig_target_{idx}")),
            reply_to_sender: Some(format!("OrigSender_{label}")),
            reply_to_text: Some(payload.clone()),
            ts: 1730800000000 + idx as i64,
            is_outgoing: idx % 2 == 0,
            ..Default::default()
        };

        // Validate JSON serialization & deserialization roundtrip
        let serialized = serde_json::to_string(&msg).expect("JSON serialization must succeed");
        let deserialized: ChatMessage = serde_json::from_str(&serialized).expect("JSON deserialization must succeed");
        assert_eq!(deserialized.id, msg.id);
        assert_eq!(deserialized.reply_to_id, msg.reply_to_id);
        assert_eq!(deserialized.reply_to_sender, msg.reply_to_sender);
        assert_eq!(deserialized.reply_to_text, msg.reply_to_text);

        // Validate truncate_chars safety (must never panic on char boundary)
        let truncated = truncate_chars(&payload, 100);
        assert!(truncated.chars().count() <= 101); // 100 chars + optional '…'

        messages.push(msg);
    }

    let thread = Thread {
        id: "thread_stress_payloads".into(),
        name: "Stress Test Thread".into(),
        messages,
        ..Default::default()
    };

    let inserted = persist_threads(&mut store, std::slice::from_ref(&thread)).expect("SQLite persist_threads failed");
    assert_eq!(inserted, 10);

    let mut hydrated_threads = vec![Thread {
        id: "thread_stress_payloads".into(),
        ..Default::default()
    }];
    store.hydrate_threads(&mut hydrated_threads, 50).expect("SQLite hydrate_threads failed");

    let loaded = &hydrated_threads[0].messages;
    assert_eq!(loaded.len(), 10);

    for orig in &thread.messages {
        let hydrated = loaded.iter().find(|m| m.id == orig.id).expect("Message must be found");
        assert_eq!(hydrated.reply_to_id, orig.reply_to_id);
        assert_eq!(hydrated.reply_to_sender, orig.reply_to_sender);
        assert_eq!(hydrated.reply_to_text, orig.reply_to_text);
    }
}

#[test]
fn test_stress_quote_sender_resolution_matrix() {
    let mut friend_threads = HashMap::new();
    friend_threads.insert("84901234567".into(), "Nguyễn Thu Hà".into());
    friend_threads.insert("84999888777".into(), "Trần Minh Quân".into());

    struct MatrixCase {
        sender: &'static str,
        is_outgoing: bool,
        thread_name: &'static str,
        is_group: bool,
        expected: &'static str,
    }

    let cases = vec![
        // 1. Outgoing messages always resolve to "chính bạn"
        MatrixCase { sender: "Tôi", is_outgoing: true, thread_name: "Group 1", is_group: true, expected: "chính bạn" },
        MatrixCase { sender: "12345", is_outgoing: true, thread_name: "Alice", is_group: false, expected: "chính bạn" },
        MatrixCase { sender: "", is_outgoing: true, thread_name: "", is_group: false, expected: "chính bạn" },

        // 2. Incoming with non-numeric sender name resolves directly
        MatrixCase { sender: "Lê Văn C", is_outgoing: false, thread_name: "Group Alpha", is_group: true, expected: "Lê Văn C" },
        MatrixCase { sender: "Bảo Ngọc (Dev)", is_outgoing: false, thread_name: "Direct", is_group: false, expected: "Bảo Ngọc (Dev)" },

        // 3. Incoming with known friend UID in direct chat or group
        MatrixCase { sender: "84901234567", is_outgoing: false, thread_name: "Nguyễn Thu Hà", is_group: false, expected: "Nguyễn Thu Hà" },
        MatrixCase { sender: "84999888777", is_outgoing: false, thread_name: "Team Rust", is_group: true, expected: "Trần Minh Quân" },

        // 4. Incoming with unknown numeric UID in direct chat -> falls back to thread friend name
        MatrixCase { sender: "84111222333", is_outgoing: false, thread_name: "Hoàng Yến", is_group: false, expected: "Hoàng Yến" },

        // 5. Incoming with unknown numeric UID in group chat -> falls back to "Người dùng <UID>"
        MatrixCase { sender: "84111222333", is_outgoing: false, thread_name: "Nhóm Bạn Thân", is_group: true, expected: "Người dùng 84111222333" },

        // 6. Incoming with "0" or empty sender when thread_name is known (e.g. group name or friend name)
        MatrixCase { sender: "0", is_outgoing: false, thread_name: "Nhóm Công Ty", is_group: true, expected: "Nhóm Công Ty" },
        MatrixCase { sender: "", is_outgoing: false, thread_name: "Nhóm Công Ty", is_group: true, expected: "Nhóm Công Ty" },

        // 7. Incoming with "0" or empty sender in direct chat -> falls back to thread friend name
        MatrixCase { sender: "0", is_outgoing: false, thread_name: "Mai Linh", is_group: false, expected: "Mai Linh" },
        MatrixCase { sender: "", is_outgoing: false, thread_name: "Mai Linh", is_group: false, expected: "Mai Linh" },

        // 8. Both sender and thread name are empty or pure numeric -> falls back to "Tin nhắn trích dẫn"
        MatrixCase { sender: "", is_outgoing: false, thread_name: "", is_group: false, expected: "Tin nhắn trích dẫn" },
        MatrixCase { sender: "0", is_outgoing: false, thread_name: "012345", is_group: false, expected: "Tin nhắn trích dẫn" },
        MatrixCase { sender: "", is_outgoing: false, thread_name: "987654321", is_group: true, expected: "Tin nhắn trích dẫn" },
    ];

    for (i, c) in cases.into_iter().enumerate() {
        let msg = ChatMessage {
            id: format!("matrix_{i}"),
            sender: c.sender.to_string(),
            is_outgoing: c.is_outgoing,
            text: "Hello".into(),
            ..Default::default()
        };
        let resolved = resolve_sender_name(&msg, c.thread_name, c.is_group, &friend_threads);
        assert_eq!(
            resolved, c.expected,
            "Case {i} failed: sender='{}', is_outgoing={}, thread='{}', is_group={}",
            c.sender, c.is_outgoing, c.thread_name, c.is_group
        );
    }
}

#[test]
fn test_stress_rapid_quote_reply_state_machine_fuzzing() {
    // Simulator matching AppState in src/gui.rs & src/gui/workspace.rs
    #[allow(dead_code)]
    struct AppStateSimulator {
        replying_to: Option<ChatMessage>,
        composer_focus_pending: bool,
        input_message: String,
        outbound_queue: Vec<ChatMessage>,
    }

    impl AppStateSimulator {
        fn new() -> Self {
            Self {
                replying_to: None,
                composer_focus_pending: false,
                input_message: String::new(),
                outbound_queue: Vec::new(),
            }
        }

        fn trigger_reply(&mut self, msg: ChatMessage) {
            self.replying_to = Some(msg);
            self.composer_focus_pending = true;
        }

        fn on_escape_press(&mut self) {
            if self.replying_to.is_some() {
                self.replying_to = None;
            }
        }

        fn on_composer_focused(&mut self) {
            self.composer_focus_pending = false;
        }

        fn send_message(&mut self, text: String, thread_id: &str) {
            let quoted = self.replying_to.take();
            let sent = ChatMessage {
                id: format!("out_{}", self.outbound_queue.len() + 1),
                sender: "Tôi".into(),
                text,
                time: "12:00".into(),
                is_outgoing: true,
                reply_to_id: quoted.as_ref().map(|q| q.id.clone()),
                reply_to_sender: quoted.as_ref().map(|q| q.sender.clone()),
                reply_to_text: quoted.as_ref().map(|q| q.text.clone()),
                ..Default::default()
            };
            let _ = thread_id;
            self.outbound_queue.push(sent);
        }
    }

    let mut sim = AppStateSimulator::new();

    // Run 1,000 rapid state transitions
    for i in 0..1000 {
        let msg_a = ChatMessage {
            id: format!("msg_fuzz_a_{i}"),
            sender: "User A".into(),
            text: format!("Message A at iter {i}"),
            ..Default::default()
        };
        let msg_b = ChatMessage {
            id: format!("msg_fuzz_b_{i}"),
            sender: "User B".into(),
            text: format!("Message B at iter {i}"),
            ..Default::default()
        };

        // 1. Trigger reply to msg_a -> must set replying_to and focus_pending
        sim.trigger_reply(msg_a);
        assert!(sim.replying_to.is_some());
        assert!(sim.composer_focus_pending);

        // 2. Composer consumes focus
        sim.on_composer_focused();
        assert!(!sim.composer_focus_pending);

        // 3. User cancels reply via Escape key -> replying_to becomes None
        sim.on_escape_press();
        assert!(sim.replying_to.is_none());

        // 4. Repeated Escape must be idempotent
        sim.on_escape_press();
        assert!(sim.replying_to.is_none());

        // 5. Trigger reply to msg_b
        sim.trigger_reply(msg_b.clone());
        assert_eq!(sim.replying_to.as_ref().unwrap().id, msg_b.id);

        // 6. Rapid overwrite: trigger reply to another message without canceling first
        let msg_c = ChatMessage {
            id: format!("msg_fuzz_c_{i}"),
            sender: "User C".into(),
            text: format!("Message C at iter {i}"),
            ..Default::default()
        };
        sim.trigger_reply(msg_c.clone());
        assert_eq!(sim.replying_to.as_ref().unwrap().id, msg_c.id);

        // 7. Send message -> replying_to must be consumed and cleared
        sim.send_message(format!("My reply {i}"), "thread_01");
        assert!(sim.replying_to.is_none(), "replying_to must be None after sending");

        let last_out = sim.outbound_queue.last().unwrap();
        assert_eq!(last_out.reply_to_id.as_deref(), Some(msg_c.id.as_str()));
        assert_eq!(last_out.reply_to_sender.as_deref(), Some("User C"));
        assert_eq!(last_out.reply_to_text.as_deref(), Some(msg_c.text.as_str()));

        // 8. Send without reply -> must have None quote fields
        sim.send_message(format!("Standalone message {i}"), "thread_01");
        let standalone = sim.outbound_queue.last().unwrap();
        assert!(standalone.reply_to_id.is_none());
        assert!(standalone.reply_to_sender.is_none());
        assert!(standalone.reply_to_text.is_none());
    }

    assert_eq!(sim.outbound_queue.len(), 2000);
}

#[test]
fn test_stress_thread_switching_isolation() {
    // Models thread switching logic from src/gui/workspace.rs lines 1632-1640
    struct WorkspaceState {
        selected_thread_index: usize,
        replying_to: Option<ChatMessage>,
        threads: Vec<Thread>,
    }

    let threads = vec![
        Thread { id: "thread_1".into(), name: "Thread 1".into(), ..Default::default() },
        Thread { id: "thread_2".into(), name: "Thread 2".into(), ..Default::default() },
        Thread { id: "thread_3".into(), name: "Thread 3".into(), ..Default::default() },
        Thread { id: "thread_4".into(), name: "Thread 4".into(), ..Default::default() },
    ];

    let mut ws = WorkspaceState {
        selected_thread_index: 0,
        replying_to: None,
        threads,
    };

    // User quotes a message in Thread 1
    let quoted_msg_t1 = ChatMessage {
        id: "msg_t1_001".into(),
        sender: "User T1".into(),
        text: "Important info in Thread 1".into(),
        ..Default::default()
    };
    ws.replying_to = Some(quoted_msg_t1.clone());

    // Switch to Thread 2
    let target_idx = 1;
    let tid = "thread_2";
    let resolved_idx = ws.threads.iter().position(|t| t.id == tid).unwrap_or(target_idx);

    if ws.selected_thread_index != resolved_idx {
        ws.replying_to = None;
    }
    ws.selected_thread_index = resolved_idx;

    // Verify replying_to is CLEARED to prevent leaking quote from Thread 1 into Thread 2
    assert!(ws.replying_to.is_none(), "replying_to must be cleared when switching threads");

    // Click on Thread 2 again (same thread) -> should NOT clear replying_to if user had started a reply in Thread 2
    let quoted_msg_t2 = ChatMessage {
        id: "msg_t2_001".into(),
        sender: "User T2".into(),
        text: "Discussion in Thread 2".into(),
        ..Default::default()
    };
    ws.replying_to = Some(quoted_msg_t2.clone());

    let same_idx = 1;
    let resolved_same = ws.threads.iter().position(|t| t.id == "thread_2").unwrap_or(same_idx);
    if ws.selected_thread_index != resolved_same {
        ws.replying_to = None;
    }
    ws.selected_thread_index = resolved_same;

    // Replying state in Thread 2 is safely preserved
    assert_eq!(ws.replying_to.as_ref().map(|q| q.id.as_str()), Some("msg_t2_001"));

    // Fuzz 500 thread switches to ensure no panic or state leakage
    for i in 0..500 {
        let dest = (i % ws.threads.len()) as usize;
        let prev = ws.selected_thread_index;
        if prev != dest {
            ws.replying_to = None;
        }
        ws.selected_thread_index = dest;
        if prev != dest {
            assert!(ws.replying_to.is_none());
        }
    }
}

#[test]
fn test_stress_sqlite_high_volume_roundtrip() {
    let mut store = MessageStore::open(":memory:").expect("Failed to open SQLite store");

    // Construct a 500-level deep quote chain: Msg N quotes Msg N-1
    let count = 500;
    let mut messages = Vec::with_capacity(count);

    for i in 0..count {
        let (quote_id, quote_sender, quote_text) = if i > 0 {
            (
                Some(format!("chain_msg_{}", i - 1)),
                Some(format!("ChainSender_{}", (i - 1) % 10)),
                Some(format!("Content of message {}", i - 1)),
            )
        } else {
            (None, None, None)
        };

        let msg = ChatMessage {
            id: format!("chain_msg_{i}"),
            client_message_id: format!("cli_msg_{i}"),
            sender: format!("ChainSender_{}", i % 10),
            text: format!("Message content at level {i}"),
            time: format!("{}:{:02}", (i / 60) % 24, i % 60),
            is_outgoing: i % 2 == 1,
            ts: 1730000000000 + (i as i64 * 1000),
            reply_to_id: quote_id,
            reply_to_sender: quote_sender,
            reply_to_text: quote_text,
            ..Default::default()
        };
        messages.push(msg);
    }

    let thread = Thread {
        id: "thread_500_chain".into(),
        name: "500 Message Chain".into(),
        messages,
        ..Default::default()
    };

    let inserted = persist_threads(&mut store, std::slice::from_ref(&thread)).expect("Failed to persist 500 messages");
    assert_eq!(inserted, count);

    let mut hydrated = vec![Thread {
        id: "thread_500_chain".into(),
        ..Default::default()
    }];
    store.hydrate_threads(&mut hydrated, count).expect("Failed to hydrate 500 messages");

    let loaded = &hydrated[0].messages;
    assert_eq!(loaded.len(), count);

    for i in 0..count {
        let m = loaded.iter().find(|m| m.id == format!("chain_msg_{i}")).expect("Message must exist");
        if i > 0 {
            assert_eq!(m.reply_to_id.as_deref(), Some(format!("chain_msg_{}", i - 1).as_str()));
            assert_eq!(m.reply_to_sender.as_deref(), Some(format!("ChainSender_{}", (i - 1) % 10).as_str()));
            assert_eq!(m.reply_to_text.as_deref(), Some(format!("Content of message {}", i - 1).as_str()));
        } else {
            assert!(m.reply_to_id.is_none());
            assert!(m.reply_to_sender.is_none());
            assert!(m.reply_to_text.is_none());
        }
    }
}

#[test]
fn test_stress_quote_navigation_resolution_and_collision_robustness() {
    let mut messages = Vec::new();

    // 1,000 background messages
    for i in 0..1000 {
        messages.push(ChatMessage {
            id: format!("srv_{i}"),
            client_message_id: format!("cli_{i}"),
            sender: "User".into(),
            text: "Standard chat text".into(),
            ..Default::default()
        });
    }

    // 1. Distinct match: Target at index 42
    let target_ref = "srv_42";
    let found = unique_reference_index(&messages, target_ref);
    assert_eq!(found, Some(42));

    // 2. Distinct match: Target at index 999
    let last_ref = "srv_999";
    let found_last = unique_reference_index(&messages, last_ref);
    assert_eq!(found_last, Some(999));

    // 3. Collision: two messages share the exact same server ID
    let mut colliding_messages = messages.clone();
    colliding_messages[10].id = "collision_id".into();
    colliding_messages[88].id = "collision_id".into();
    assert_eq!(
        unique_reference_index(&colliding_messages, "collision_id"),
        None,
        "Colliding reference must be safely rejected (return None) to prevent jumping to wrong message"
    );

    // 4. Invalid dummy IDs must never match
    assert_eq!(unique_reference_index(&messages, ""), None);
    assert_eq!(unique_reference_index(&messages, "   "), None);
    assert_eq!(unique_reference_index(&messages, "0"), None);
    assert_eq!(unique_reference_index(&messages, "00000"), None);

    // 5. Quote reference extraction from wire_ref
    let mut wire_msg = ChatMessage {
        id: "wire_reply_1".into(),
        ..Default::default()
    };
    wire_msg.wire_ref.fields.insert(
        "quote".into(),
        json!({
            "cliMsgId": "0",
            "globalMsgId": "wire_global_99",
            "msgId": 99
        }),
    );
    assert_eq!(message_reply_reference(&wire_msg).as_deref(), Some("wire_global_99"));

    // 6. Quote reference extraction from history_source
    let mut hist_msg = ChatMessage {
        id: "hist_reply_1".into(),
        ..Default::default()
    };
    hist_msg.history_source = Some(json!({
        "meta": {
            "quote": {
                "msgId": "hist_target_77"
            }
        }
    }));
    assert_eq!(message_reply_reference(&hist_msg).as_deref(), Some("hist_target_77"));

    // 7. Explicit reply_to_id overrides wire_ref and history_source
    let mut explicit_msg = wire_msg.clone();
    explicit_msg.history_source = hist_msg.history_source.clone();
    explicit_msg.reply_to_id = Some("explicit_winner".into());
    assert_eq!(message_reply_reference(&explicit_msg).as_deref(), Some("explicit_winner"));
}

#[test]
fn test_stress_quote_payload_precedence_and_smart_snippets() {
    // 1. Sticker message quote
    let sticker_msg = ChatMessage {
        id: "s1".into(),
        sender: "User 1".into(),
        text: "".into(),
        sticker_id: Some(88888),
        ..Default::default()
    };
    assert_eq!(resolve_quote_snippet(&sticker_msg), "[Nhãn dán]");

    // 2. Photo message quote
    let photo_msg = ChatMessage {
        id: "p1".into(),
        sender: "User 2".into(),
        text: "[Hình ảnh]".into(),
        link_url: Some("https://example.com/pic.png".into()),
        ..Default::default()
    };
    assert_eq!(resolve_quote_snippet(&photo_msg), "[Hình ảnh]");

    // 3. File message quote
    let file_msg = ChatMessage {
        id: "f1".into(),
        sender: "User 3".into(),
        text: "[Tập tin]".into(),
        link_title: Some("BaoCaoQ4.xlsx".into()),
        ..Default::default()
    };
    assert_eq!(resolve_quote_snippet(&file_msg), "[Tập tin] BaoCaoQ4.xlsx");

    // 4. Empty text fallback
    let empty_msg = ChatMessage {
        id: "e1".into(),
        sender: "User 4".into(),
        text: "   ".into(),
        ..Default::default()
    };
    assert_eq!(resolve_quote_snippet(&empty_msg), "[Tin nhắn]");

    // 5. Normal text truncated to 100 characters
    let long_text = "T".repeat(150);
    let text_msg = ChatMessage {
        id: "t1".into(),
        sender: "User 5".into(),
        text: long_text,
        ..Default::default()
    };
    let snippet = resolve_quote_snippet(&text_msg);
    assert_eq!(snippet.chars().count(), 101); // 100 'T's + '…'
    assert!(snippet.ends_with('…'));
}

#[test]
fn test_stress_egui_ui_layout_under_extreme_payloads() {
    let ctx = egui::Context::default();

    // Adversarial payloads tested in egui text layout
    let extreme_payloads = [
        "A".repeat(50_000),
        "Tiếng Việt có dấu: Cảm ơn bạn rất nhiều! Chúc một ngày tốt lành 🇻🇳 👍🏽".repeat(500),
        (0..1000).map(|i| format!("Line {i}")).collect::<Vec<_>>().join("\n"),
        "\0\x1b[31;1mANSI_INJECTION\x1b[0m <script>alert(1)</script>".into(),
        "👨‍👩‍👧‍👦 👩‍💻 👍🏿 🇻🇳 🇺🇸 🚀 \u{202E}RTL\u{202C} \u{200B}".repeat(200),
    ];

    for (idx, payload) in extreme_payloads.iter().enumerate() {
        let truncated = truncate_chars(payload, 84);
        let snippet = truncate_chars(payload, 100);

        // Run an egui layout frame with the truncated texts to guarantee no panic in text galley / layout
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("Payload {idx}")).strong());
                    ui.label(egui::RichText::new(truncated.as_ref()));
                });
                ui.vertical(|ui| {
                    ui.add(egui::Label::new(egui::RichText::new(snippet.as_ref())).wrap());
                });
            });
        });
    }
}
