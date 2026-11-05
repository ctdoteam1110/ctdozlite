//! Adversarial test harness for Milestone M3 (Requirement R3: Message Quote & Reply).
//!
//! Focus areas:
//! 1. Bubble double-click vs single-click event propagation:
//!    - Quoted block click-to-scroll header.
//!    - Links in messages.
//!    - Attachment/media buttons.
//!    - Context menu trigger (right-click / Secondary button).
//! 2. Composer state transitions and focus management across consecutive interactions:
//!    - Consecutive replies across different messages.
//!    - Consecutive replies on the same message.
//!    - Dismissal via Escape and X button.
//!    - Outbound quote consumption on send.
//!    - Immediate reply re-activation after send.

use eframe::egui::{self, Color32, Margin, Pos2, Rect, Rounding, Sense, Vec2};
use zalo_rust::chat::ChatMessage;
use zalo_rust::message_navigation::{message_reply_reference, unique_reference_index};

#[derive(Debug, Clone, PartialEq, Eq)]
enum MockBubbleAction {
    Reply,
    JumpToQuoted(String),
    OpenLink(String),
    DownloadAttachment(String),
    OpenContextMenu,
}

/// Headless simulation of bubble rendering and event arbitration exactly matching
/// the logic in `src/gui/messages.rs`.
fn simulate_bubble_interaction(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    has_quote_block: bool,
    has_link: bool,
    has_attachment: bool,
) -> Option<MockBubbleAction> {
    let mut quote_action = None;
    let mut card_action = None;

    // 1. Render nested elements inside the bubble frame
    let fr = egui::Frame::none()
        .fill(Color32::from_rgb(240, 240, 240))
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::same(8.0))
        .show(ui, |ui| {
            // A. Quoted Block (Click to Scroll)
            if has_quote_block {
                let quote_frame = egui::Frame::none()
                    .fill(Color32::from_rgb(220, 230, 245))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(Margin::same(6.0))
                    .show(ui, |ui| {
                        ui.label("Quoted Message Header");
                    });
                let quote_resp = ui.interact(
                    quote_frame.response.rect,
                    quote_frame.response.id.with("quote_jump"),
                    Sense::click(),
                );
                if quote_resp.clicked() {
                    let ref_id = msg.reply_to_id.clone().unwrap_or_else(|| "orig_default".into());
                    quote_action = Some(MockBubbleAction::JumpToQuoted(ref_id));
                }
            }

            // B. Attachment Button
            if has_attachment {
                let attach_resp = ui.button("📥 Tải tập tin đính kèm");
                if attach_resp.clicked() {
                    card_action = Some(MockBubbleAction::DownloadAttachment(
                        msg.link_url.clone().unwrap_or_else(|| "file_url".into()),
                    ));
                }
            }

            // C. Link in message
            if has_link {
                let link_resp = ui.link("https://example.com/test-link");
                if link_resp.clicked() {
                    card_action = Some(MockBubbleAction::OpenLink("https://example.com/test-link".into()));
                }
            }

            // Normal message text
            ui.label(&msg.text);
        });

    // 2. Primary Double-Click Detection (as implemented in src/gui/messages.rs lines 2397-2404 & 2532-2539)
    let is_double_clicked = ui.input(|i| {
        i.pointer.button_double_clicked(egui::PointerButton::Primary)
            && i.pointer
                .interact_pos()
                .or(i.pointer.latest_pos())
                .map(|pos| fr.response.rect.contains(pos))
                .unwrap_or(false)
    });

    if is_double_clicked {
        return Some(MockBubbleAction::Reply);
    }

    // 3. Fallback to inner actions or Context Menu
    quote_action.or(card_action).or_else(|| {
        // Context menu check (Secondary button / Right-click)
        let secondary_or_popup = ui.input(|i| {
            i.pointer.button_down(egui::PointerButton::Secondary)
                || i.pointer.button_clicked(egui::PointerButton::Secondary)
                || i.pointer.button_released(egui::PointerButton::Secondary)
        }) || ui.memory(|m| m.any_popup_open());

        if secondary_or_popup {
            let resp = ui.interact(
                fr.response.rect,
                egui::Id::new(("ctx_menu", &msg.id)),
                Sense::click(),
            );
            if resp.secondary_clicked() || ui.input(|i| i.pointer.button_clicked(egui::PointerButton::Secondary)) {
                return Some(MockBubbleAction::OpenContextMenu);
            }
        }
        None
    })
}

#[test]
fn test_adversarial_single_click_on_quote_header_does_not_trigger_reply() {
    let ctx = egui::Context::default();
    let msg = ChatMessage {
        id: "msg_with_quote".into(),
        text: "Tin nhắn có trích dẫn".into(),
        reply_to_id: Some("target_orig_001".into()),
        reply_to_sender: Some("Lan".into()),
        reply_to_text: Some("Nội dung gốc cần nhảy tới".into()),
        ..Default::default()
    };

    let mut position = Pos2::new(20.0, 20.0);
    let mut captured_action = None;

    for frame in 0..4 {
        let mut events = vec![egui::Event::PointerMoved(position)];
        if frame >= 2 {
            events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: frame == 2,
                modifiers: egui::Modifiers::NONE,
            });
        }

        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    captured_action = simulate_bubble_interaction(ui, &msg, true, false, false);
                });
            },
        );

        if frame == 0 {
            position = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "Quoted Message Header" => {
                        Some(text.pos + Vec2::new(4.0, 4.0))
                    }
                    _ => None,
                })
                .expect("Quoted header text must be rendered");
        }
    }

    assert_eq!(
        captured_action,
        Some(MockBubbleAction::JumpToQuoted("target_orig_001".into())),
        "Single click on quote block header must produce JumpToQuoted and never be hijacked into Reply"
    );
}

#[test]
fn test_adversarial_single_click_on_link_does_not_trigger_reply() {
    let ctx = egui::Context::default();
    let msg = ChatMessage {
        id: "msg_with_link".into(),
        text: "Xem tài liệu tại link".into(),
        link_url: Some("https://example.com/test-link".into()),
        ..Default::default()
    };

    let mut position = Pos2::new(20.0, 20.0);
    let mut captured_action = None;

    for frame in 0..4 {
        let mut events = vec![egui::Event::PointerMoved(position)];
        if frame >= 2 {
            events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: frame == 2,
                modifiers: egui::Modifiers::NONE,
            });
        }

        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    captured_action = simulate_bubble_interaction(ui, &msg, false, true, false);
                });
            },
        );

        if frame == 0 {
            position = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text.contains("https://example.com/test-link") => {
                        Some(text.pos + Vec2::new(4.0, 4.0))
                    }
                    _ => None,
                })
                .expect("Link text must be rendered");
        }
    }

    assert_eq!(
        captured_action,
        Some(MockBubbleAction::OpenLink("https://example.com/test-link".into())),
        "Single click on link must open link and never trigger Reply"
    );
}

#[test]
fn test_adversarial_single_click_on_attachment_does_not_trigger_reply() {
    let ctx = egui::Context::default();
    let msg = ChatMessage {
        id: "msg_with_file".into(),
        text: "[Tập tin] bao_cao.pdf".into(),
        link_url: Some("https://example.com/files/bao_cao.pdf".into()),
        ..Default::default()
    };

    let mut position = Pos2::new(20.0, 20.0);
    let mut captured_action = None;

    for frame in 0..4 {
        let mut events = vec![egui::Event::PointerMoved(position)];
        if frame >= 2 {
            events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: frame == 2,
                modifiers: egui::Modifiers::NONE,
            });
        }

        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    captured_action = simulate_bubble_interaction(ui, &msg, false, false, true);
                });
            },
        );

        if frame == 0 {
            position = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text.contains("Tải tập tin đính kèm") => {
                        Some(text.pos + Vec2::new(4.0, 4.0))
                    }
                    _ => None,
                })
                .expect("Attachment text must be rendered");
        }
    }

    assert_eq!(
        captured_action,
        Some(MockBubbleAction::DownloadAttachment("https://example.com/files/bao_cao.pdf".into())),
        "Single click on attachment button must initiate download and never trigger Reply"
    );
}

#[test]
fn test_adversarial_right_click_context_menu_does_not_trigger_reply() {
    let ctx = egui::Context::default();
    let msg = ChatMessage {
        id: "msg_ctx_test".into(),
        text: "Right click me for context menu".into(),
        ..Default::default()
    };

    let mut position = Pos2::new(20.0, 20.0);
    let mut captured_action = None;

    for frame in 0..4 {
        let mut events = vec![egui::Event::PointerMoved(position)];
        if frame >= 2 {
            events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Secondary,
                pressed: frame == 2,
                modifiers: egui::Modifiers::NONE,
            });
        }

        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    captured_action = simulate_bubble_interaction(ui, &msg, false, false, false);
                });
            },
        );

        if frame == 0 {
            position = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text.contains("Right click me") => {
                        Some(text.pos + Vec2::new(4.0, 4.0))
                    }
                    _ => None,
                })
                .expect("Bubble text must be rendered");
        }
    }

    assert_eq!(
        captured_action,
        Some(MockBubbleAction::OpenContextMenu),
        "Right click must trigger context menu and never trigger Reply"
    );
}

#[test]
fn test_adversarial_double_click_triggers_reply() {
    let ctx = egui::Context::default();
    let msg = ChatMessage {
        id: "msg_double_click".into(),
        text: "Double click to reply".into(),
        ..Default::default()
    };

    let mut position = Pos2::new(20.0, 20.0);
    let mut captured_action = None;

    // Frame 0: layout pass to find text position
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            events: vec![egui::Event::PointerMoved(position)],
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                captured_action = simulate_bubble_interaction(ui, &msg, false, false, false);
            });
        },
    );

    position = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Double click to reply" => {
                Some(text.pos + Vec2::new(4.0, 4.0))
            }
            _ => None,
        })
        .expect("Bubble text must be rendered");

    // Frame 1: Rapid consecutive clicks (double click)
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            events: vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                captured_action = simulate_bubble_interaction(ui, &msg, false, false, false);
            });
        },
    );

    assert_eq!(
        captured_action,
        Some(MockBubbleAction::Reply),
        "Double primary click on bubble must trigger Reply"
    );
}

// ---------------------------------------------------------------------------
// Composer State Machine & Consecutive Focus Request Harness
// ---------------------------------------------------------------------------

struct ComposerStateHarness {
    replying_to: Option<ChatMessage>,
    composer_focus_pending: bool,
    focus_request_counter: usize,
    input_message: String,
    outbound_messages: Vec<ChatMessage>,
}

impl ComposerStateHarness {
    fn new() -> Self {
        Self {
            replying_to: None,
            composer_focus_pending: false,
            focus_request_counter: 0,
            input_message: String::new(),
            outbound_messages: Vec::new(),
        }
    }

    /// Handles `BubbleAction::Reply` as in `src/gui/workspace.rs:3587-3591`.
    fn on_bubble_reply(&mut self, msg: ChatMessage) {
        self.replying_to = Some(msg);
        self.composer_focus_pending = true;
    }

    /// Simulates a frame rendering pass of the composer as in `src/gui/workspace.rs:4420-4423`.
    fn render_frame(&mut self) {
        if self.composer_focus_pending {
            self.focus_request_counter += 1;
            self.composer_focus_pending = false;
        }
    }

    /// Handles dismiss via Escape or X button as in `src/gui/workspace.rs:3767-3770`.
    fn on_dismiss_reply(&mut self) {
        self.replying_to = None;
    }

    /// Handles outbound message send as in `src/gui.rs:1256-1290` & `src/gui/workspace.rs:4494-4509`.
    fn on_send(&mut self) {
        let quoted = self.replying_to.take();
        let sent = ChatMessage {
            id: format!("out_{}", self.outbound_messages.len() + 1),
            sender: "Bạn".into(),
            text: self.input_message.drain(..).collect(),
            is_outgoing: true,
            reply_to_id: quoted.as_ref().map(|q| q.id.clone()),
            reply_to_sender: quoted.as_ref().map(|q| q.sender.clone()),
            reply_to_text: quoted.as_ref().map(|q| q.text.clone()),
            ..Default::default()
        };
        self.outbound_messages.push(sent);
    }
}

#[test]
fn test_consecutive_replies_across_different_messages_properly_requests_focus() {
    let mut harness = ComposerStateHarness::new();

    let msg1 = ChatMessage {
        id: "msg_1".into(),
        sender: "Alice".into(),
        text: "First message".into(),
        ..Default::default()
    };
    let msg2 = ChatMessage {
        id: "msg_2".into(),
        sender: "Bob".into(),
        text: "Second message".into(),
        ..Default::default()
    };
    let msg3 = ChatMessage {
        id: "msg_3".into(),
        sender: "Charlie".into(),
        text: "Third message".into(),
        ..Default::default()
    };

    // 1. Reply to Msg 1
    harness.on_bubble_reply(msg1.clone());
    assert!(harness.composer_focus_pending, "Focus must be pending after reply 1");
    assert_eq!(harness.replying_to.as_ref().map(|m| m.id.as_str()), Some("msg_1"));

    // Render frame 1: Focus consumed
    harness.render_frame();
    assert!(!harness.composer_focus_pending, "Focus flag must be cleared");
    assert_eq!(harness.focus_request_counter, 1);

    // 2. Consecutively reply to Msg 2 without closing preview banner
    harness.on_bubble_reply(msg2.clone());
    assert!(harness.composer_focus_pending, "Focus must be pending again after reply 2");
    assert_eq!(harness.replying_to.as_ref().map(|m| m.id.as_str()), Some("msg_2"), "replying_to must update to Msg 2");

    // Render frame 2: Focus consumed
    harness.render_frame();
    assert!(!harness.composer_focus_pending);
    assert_eq!(harness.focus_request_counter, 2);

    // 3. Consecutively reply to Msg 3
    harness.on_bubble_reply(msg3.clone());
    assert!(harness.composer_focus_pending, "Focus must be pending again after reply 3");
    assert_eq!(harness.replying_to.as_ref().map(|m| m.id.as_str()), Some("msg_3"));

    // Render frame 3: Focus consumed
    harness.render_frame();
    assert!(!harness.composer_focus_pending);
    assert_eq!(harness.focus_request_counter, 3);
}

#[test]
fn test_consecutive_replies_on_same_message_properly_re_requests_focus() {
    let mut harness = ComposerStateHarness::new();

    let msg = ChatMessage {
        id: "msg_repeat".into(),
        sender: "Diana".into(),
        text: "Double click me twice".into(),
        ..Default::default()
    };

    // First double click
    harness.on_bubble_reply(msg.clone());
    harness.render_frame();
    assert_eq!(harness.focus_request_counter, 1);

    // Second double click on the exact same message
    harness.on_bubble_reply(msg.clone());
    assert!(harness.composer_focus_pending, "Focus must be requested even if replying_to is the same message");

    harness.render_frame();
    assert_eq!(harness.focus_request_counter, 2, "Focus must have been requested a second time");
    assert_eq!(harness.replying_to.as_ref().map(|m| m.id.as_str()), Some("msg_repeat"));
}

#[test]
fn test_reply_lifecycle_dismiss_escape_and_send_consumption() {
    let mut harness = ComposerStateHarness::new();

    let target = ChatMessage {
        id: "msg_target".into(),
        sender: "Eve".into(),
        text: "Important announcement".into(),
        ..Default::default()
    };

    // Step 1: Reply triggered
    harness.on_bubble_reply(target.clone());
    harness.render_frame();
    assert_eq!(harness.focus_request_counter, 1);
    assert!(harness.replying_to.is_some());

    // Step 2: Dismiss via Escape key
    harness.on_dismiss_reply();
    assert!(harness.replying_to.is_none(), "Escape key must clear replying_to");

    // Frame after dismissal: No focus request triggered
    harness.render_frame();
    assert_eq!(harness.focus_request_counter, 1, "Dismissal should not request focus");

    // Step 3: Normal send without quote
    harness.input_message = "Normal message".into();
    harness.on_send();
    let sent1 = harness.outbound_messages.last().unwrap();
    assert!(sent1.reply_to_id.is_none(), "Normal send must not have reply_to_id");

    // Step 4: Re-trigger Reply and Send with quote
    harness.on_bubble_reply(target.clone());
    harness.render_frame();
    assert_eq!(harness.focus_request_counter, 2);

    harness.input_message = "I have noted this!".into();
    harness.on_send();

    assert!(harness.replying_to.is_none(), "replying_to must be consumed (None) after sending");
    let sent2 = harness.outbound_messages.last().unwrap();
    assert_eq!(sent2.reply_to_id.as_deref(), Some("msg_target"));
    assert_eq!(sent2.reply_to_sender.as_deref(), Some("Eve"));
    assert_eq!(sent2.reply_to_text.as_deref(), Some("Important announcement"));

    // Step 5: Immediately reply to another message right after send
    let next_target = ChatMessage {
        id: "msg_next".into(),
        sender: "Frank".into(),
        text: "Next query".into(),
        ..Default::default()
    };
    harness.on_bubble_reply(next_target);
    assert!(harness.composer_focus_pending);
    harness.render_frame();
    assert_eq!(harness.focus_request_counter, 3);
    assert_eq!(harness.replying_to.as_ref().map(|m| m.id.as_str()), Some("msg_next"));
}

#[test]
fn test_quote_reference_resolution_adversarial_matrix() {
    let thread_messages = vec![
        ChatMessage {
            id: "server_001".into(),
            client_message_id: "cli_001".into(),
            text: "First".into(),
            ..Default::default()
        },
        ChatMessage {
            id: "server_002".into(),
            client_message_id: "cli_002".into(),
            text: "Second".into(),
            ..Default::default()
        },
        ChatMessage {
            id: "server_003".into(),
            client_message_id: "cli_003".into(),
            text: "Third".into(),
            ..Default::default()
        },
    ];

    // Reference by server id
    let reply_server = ChatMessage {
        reply_to_id: Some("server_002".into()),
        ..Default::default()
    };
    let ref_s = message_reply_reference(&reply_server).expect("Should extract server reference");
    assert_eq!(ref_s, "server_002");
    assert_eq!(unique_reference_index(&thread_messages, &ref_s), Some(1));

    // Reference by client message id
    let reply_client = ChatMessage {
        reply_to_id: Some("cli_003".into()),
        ..Default::default()
    };
    let ref_c = message_reply_reference(&reply_client).expect("Should extract client reference");
    assert_eq!(ref_c, "cli_003");
    assert_eq!(unique_reference_index(&thread_messages, &ref_c), Some(2));

    // Reference to non-existent message returns None index safely without panic
    let reply_missing = ChatMessage {
        reply_to_id: Some("non_existent_999".into()),
        ..Default::default()
    };
    let ref_m = message_reply_reference(&reply_missing).expect("Should extract reference");
    assert_eq!(unique_reference_index(&thread_messages, &ref_m), None);
}
