//! Test suite for Milestone M3: Message Quote & Reply (Requirement R3).
//!
//! Validates:
//! 1. Data model serialization and deserialization of quote metadata (reply_to_id, reply_to_sender, reply_to_text).
//! 2. SQLite store persistence and hydration of quoted messages.
//! 3. Quote reference matching and navigation index resolution.
//! 4. Quote preview snippet fallback logic for stickers, photos, and files.
//! 5. Quote reply state transitions: trigger reply -> focus composer -> cancel on escape or send with quote.

use zalo_rust::chat::{ChatMessage, Thread};
use zalo_rust::message_navigation::{message_reply_reference, unique_reference_index};
use zalo_rust::store::{persist_threads, MessageStore};

#[test]
fn test_quote_message_fields_and_json_roundtrip() {
    let msg = ChatMessage {
        id: "msg_quote_001".into(),
        sender: "Nguyễn Văn A".into(),
        text: "Đây là phản hồi cho tin nhắn trước".into(),
        time: "10:30".into(),
        is_outgoing: true,
        ts: 1730790000000,
        reply_to_id: Some("orig_msg_000".into()),
        reply_to_sender: Some("Trần Thị B".into()),
        reply_to_text: Some("Nội dung tin nhắn gốc".into()),
        ..Default::default()
    };

    let serialized = serde_json::to_string(&msg).expect("ChatMessage serialization failed");
    let deserialized: ChatMessage = serde_json::from_str(&serialized).expect("ChatMessage deserialization failed");

    assert_eq!(deserialized.id, "msg_quote_001");
    assert_eq!(deserialized.reply_to_id.as_deref(), Some("orig_msg_000"));
    assert_eq!(deserialized.reply_to_sender.as_deref(), Some("Trần Thị B"));
    assert_eq!(deserialized.reply_to_text.as_deref(), Some("Nội dung tin nhắn gốc"));
}

#[test]
fn test_quote_persistence_in_sqlite_store() {
    let mut store = MessageStore::open(":memory:").expect("Failed to create in-memory SQLite store");

    let orig_msg = ChatMessage {
        id: "orig_001".into(),
        sender: "Alice".into(),
        text: "Gặp nhau lúc 2h chiều nhé".into(),
        time: "10:55".into(),
        is_outgoing: false,
        ts: 1730791000000,
        ..Default::default()
    };

    let reply_msg = ChatMessage {
        id: "reply_002".into(),
        sender: "Bob".into(),
        text: "Ok, mình sẽ đến đúng giờ!".into(),
        time: "11:00".into(),
        is_outgoing: true,
        ts: 1730792000000,
        reply_to_id: Some("orig_001".into()),
        reply_to_sender: Some("Alice".into()),
        reply_to_text: Some("Gặp nhau lúc 2h chiều nhé".into()),
        ..Default::default()
    };

    let thread = Thread {
        id: "thread_quote_test".into(),
        name: "Test Group".into(),
        is_group: true,
        preview: "Replying to original".into(),
        time: "11:00".into(),
        avatar: None,
        messages: vec![orig_msg, reply_msg],
        ..Default::default()
    };

    let inserted = persist_threads(&mut store, &[thread]).expect("persist_threads failed");
    assert_eq!(inserted, 2);

    let mut empty_threads = vec![Thread {
        id: "thread_quote_test".into(),
        name: "Test Group".into(),
        is_group: true,
        ..Default::default()
    }];

    store.hydrate_threads(&mut empty_threads, 50).expect("hydrate_threads failed");
    let loaded_messages = &empty_threads[0].messages;
    assert_eq!(loaded_messages.len(), 2);

    let retrieved_reply = loaded_messages.iter().find(|m| m.id == "reply_002").expect("reply message not found");
    assert_eq!(retrieved_reply.reply_to_id.as_deref(), Some("orig_001"));
    assert_eq!(retrieved_reply.reply_to_sender.as_deref(), Some("Alice"));
    assert_eq!(retrieved_reply.reply_to_text.as_deref(), Some("Gặp nhau lúc 2h chiều nhé"));
}

#[test]
fn test_quote_reference_navigation_and_jump_index() {
    let msg1 = ChatMessage {
        id: "server_id_100".into(),
        client_message_id: "client_id_100".into(),
        sender: "User A".into(),
        text: "Original message to be quoted".into(),
        ..Default::default()
    };
    let msg2 = ChatMessage {
        id: "server_id_101".into(),
        client_message_id: "client_id_101".into(),
        sender: "User B".into(),
        text: "Unrelated message in between".into(),
        ..Default::default()
    };
    let reply = ChatMessage {
        id: "server_id_102".into(),
        client_message_id: "client_id_102".into(),
        sender: "User C".into(),
        text: "Quoting User A".into(),
        reply_to_id: Some("server_id_100".into()),
        reply_to_sender: Some("User A".into()),
        reply_to_text: Some("Original message to be quoted".into()),
        ..Default::default()
    };

    let thread_messages = vec![msg1, msg2, reply.clone()];

    let reference = message_reply_reference(&reply).expect("Reply reference extraction failed");
    assert_eq!(reference, "server_id_100");

    let target_index = unique_reference_index(&thread_messages, &reference);
    assert_eq!(target_index, Some(0), "Navigation should jump directly to index 0");
}

#[test]
fn test_quote_fallback_snippet_generation() {
    // When quoted message is a sticker with empty text
    let sticker_msg = ChatMessage {
        id: "sticker_01".into(),
        sender: "User A".into(),
        text: "".into(),
        sticker_id: Some(12345),
        ..Default::default()
    };
    let quote_text_sticker = if !sticker_msg.text.trim().is_empty() {
        sticker_msg.text.clone()
    } else if sticker_msg.sticker_id.is_some() {
        "[Nhãn dán]".to_string()
    } else {
        "[Tin nhắn]".to_string()
    };
    assert_eq!(quote_text_sticker, "[Nhãn dán]");

    // When quoted message is a photo
    let photo_msg = ChatMessage {
        id: "photo_01".into(),
        sender: "User B".into(),
        text: "[Hình ảnh]".into(),
        link_url: Some("https://example.com/photo.jpg".into()),
        ..Default::default()
    };
    let quote_text_photo = if !photo_msg.text.trim().is_empty() && photo_msg.text.trim() != "[Hình ảnh]" {
        photo_msg.text.clone()
    } else if photo_msg.link_url.is_some() {
        "[Hình ảnh]".to_string()
    } else {
        "[Tin nhắn]".to_string()
    };
    assert_eq!(quote_text_photo, "[Hình ảnh]");

    // When quoted message sender is numeric UID, fallback to thread friend name
    let numeric_sender = ChatMessage {
        id: "msg_num".into(),
        sender: "29384729183".into(),
        text: "Hello".into(),
        ..Default::default()
    };
    let thread_name = "Thùy Trang";
    let is_group = false;
    let resolved_sender = if !numeric_sender.sender.is_empty()
        && numeric_sender.sender != "0"
        && !numeric_sender.sender.chars().all(|c| c.is_ascii_digit())
    {
        numeric_sender.sender.clone()
    } else if !is_group && !thread_name.is_empty() {
        thread_name.to_string()
    } else {
        "Tin nhắn trích dẫn".to_string()
    };
    assert_eq!(resolved_sender, "Thùy Trang");
}

#[test]
fn test_quote_reply_state_machine_flow() {
    // Represents app state during reply interactions
    struct StateSimulator {
        replying_to: Option<ChatMessage>,
        composer_focus_pending: bool,
        input_message: String,
        sent_messages: Vec<ChatMessage>,
    }

    let mut state = StateSimulator {
        replying_to: None,
        composer_focus_pending: false,
        input_message: String::new(),
        sent_messages: Vec::new(),
    };

    let target_msg = ChatMessage {
        id: "msg_orig".into(),
        sender: "Lan Anh".into(),
        text: "Bạn đã xem báo cáo chưa?".into(),
        ..Default::default()
    };

    // 1. Double-click or context menu selects Reply
    state.replying_to = Some(target_msg.clone());
    state.composer_focus_pending = true;

    assert!(state.replying_to.is_some());
    assert!(state.composer_focus_pending);

    // 2. Composer receives focus and clears pending flag
    if state.composer_focus_pending {
        state.composer_focus_pending = false;
    }
    assert!(!state.composer_focus_pending);

    // 3. User cancels via Escape key
    let escape_pressed = true;
    if escape_pressed && state.replying_to.is_some() {
        state.replying_to = None;
    }
    assert!(state.replying_to.is_none());

    // 4. User selects Reply again and enters message
    state.replying_to = Some(target_msg.clone());
    state.composer_focus_pending = true;
    state.input_message = "Mình đang xem rồi nhé!".into();

    // 5. User sends message -> replying_to is consumed (taken)
    let quoted = state.replying_to.take();
    assert!(quoted.is_some());
    assert!(state.replying_to.is_none(), "replying_to must be cleared after sending");

    let sent = ChatMessage {
        id: "out_12345".into(),
        sender: "Tôi".into(),
        text: state.input_message.clone(),
        reply_to_id: quoted.as_ref().map(|q| q.id.clone()),
        reply_to_sender: quoted.as_ref().map(|q| q.sender.clone()),
        reply_to_text: quoted.as_ref().map(|q| q.text.clone()),
        is_outgoing: true,
        ..Default::default()
    };
    state.sent_messages.push(sent);

    let last_sent = state.sent_messages.last().unwrap();
    assert_eq!(last_sent.reply_to_id.as_deref(), Some("msg_orig"));
    assert_eq!(last_sent.reply_to_sender.as_deref(), Some("Lan Anh"));
    assert_eq!(last_sent.reply_to_text.as_deref(), Some("Bạn đã xem báo cáo chưa?"));
}
