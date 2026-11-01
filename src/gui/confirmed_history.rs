//! Exact confirmed-send projections for the shared account-history retention sink.
use crate::{
    chat::{ChatMessage, Thread},
    composer_mentions::OutgoingMention,
    message_model::{DeliveryState, MessageKind},
};

/// Actual payload submitted by a legacy producer, not an inferred message type.
pub(super) enum Payload<'a> {
    Text {
        raw: &'a str,
        mentions: &'a [OutgoingMention],
        quoted: Option<&'a ChatMessage>,
    },
    Sticker {
        sticker_id: u64,
        category_id: u64,
        sticker_type: u32,
    },
    /// Forward requests submit new text/sticker content, never the source message's identity.
    Forward(&'a ChatMessage),
    /// Transport-produced media metadata must retain the exact submitted request identity.
    Attachment(&'a ChatMessage),
}

/// Construct one partial thread projection before admitting a server confirmation to the GUI.
/// The caller retains this projection until the shared history sink durably acknowledges it.
///
/// # Errors
/// Rejects missing identities, invalid mention spans, or inconsistent transport-produced media IDs.
pub(super) fn project(
    thread: &str,
    is_group: bool,
    client_id: u64,
    server_id: &str,
    payload: Payload<'_>,
) -> anyhow::Result<Thread> {
    anyhow::ensure!(
        !thread.trim().is_empty() && client_id != 0,
        "Missing confirmed-send identity"
    );
    anyhow::ensure!(
        !server_id
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control()),
        "Invalid server confirmation identity"
    );
    let mut message = match payload {
        Payload::Text {
            raw,
            mentions,
            quoted,
        } => {
            crate::composer_mentions::validate_mentions(raw, mentions)?;
            anyhow::ensure!(
                is_group || mentions.is_empty(),
                "Unverified direct mention payload"
            );
            let mut message = super::worker::composed_sent_message(
                raw,
                mentions,
                quoted,
                client_id,
                server_id.into(),
            );
            message.kind = MessageKind::Text;
            message
        }
        Payload::Sticker {
            sticker_id,
            category_id,
            sticker_type,
        } => {
            anyhow::ensure!(sticker_id != 0, "Missing submitted sticker identity");
            let mut message = super::worker::composed_sent_message(
                "[Nhãn dán]",
                &[],
                None,
                client_id,
                server_id.into(),
            );
            message.kind = MessageKind::Sticker;
            message.sticker_id = Some(sticker_id);
            message.sticker_cate_id = Some(category_id);
            message.sticker_type = Some(sticker_type);
            message
        }
        Payload::Forward(source) => {
            // This mirrors forward_message_with_client_id's actual text/sticker dispatch.
            // Do not clone server references, reactions, receipts, mentions or reply metadata.
            if let Some(sticker_id) = source.sticker_id.filter(|id| *id > 0) {
                return project(
                    thread,
                    is_group,
                    client_id,
                    server_id,
                    Payload::Sticker {
                        sticker_id,
                        category_id: source.sticker_cate_id.unwrap_or(0),
                        sticker_type: source.sticker_type.unwrap_or(7),
                    },
                );
            }
            return project(
                thread,
                is_group,
                client_id,
                server_id,
                Payload::Text {
                    raw: &source.text,
                    mentions: &[],
                    quoted: None,
                },
            );
        }
        Payload::Attachment(message) => {
            let expected_id = if server_id.is_empty() {
                format!("out_{client_id}")
            } else {
                server_id.into()
            };
            anyhow::ensure!(
                message.is_outgoing
                    && message.client_message_id == client_id.to_string()
                    && message.id == expected_id,
                "Transport media confirmation identity mismatch"
            );
            message.clone()
        }
    };
    message.delivery = message.delivery.merge(DeliveryState::ServerAccepted);
    Ok(Thread {
        id: thread.into(),
        is_group,
        preview: message.text.clone(),
        time: message.time.clone(),
        messages: vec![message],
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gui::history, session::Session};
    use std::sync::Arc;

    #[test]
    fn confirmed_outgoing_text_quote_preserves_raw_uid_and_exact_request_identity() {
        let quote = ChatMessage {
            id: "original-server".into(),
            text: "Raw :+1:".into(),
            sender: "22".into(),
            ..Default::default()
        };
        let mentions = [OutgoingMention {
            uid: "22".into(),
            pos: 3,
            len: 3,
            kind: 0,
        }];
        let projection = project(
            "group-1",
            true,
            123,
            "new-server",
            Payload::Text {
                raw: "😀 @An ",
                mentions: &mentions,
                quoted: Some(&quote),
            },
        )
        .unwrap();
        let row = &projection.messages[0];
        assert_eq!(
            (row.id.as_str(), row.client_message_id.as_str()),
            ("new-server", "123")
        );
        assert_eq!(row.text, "😀 @An ");
        assert_eq!(row.reply_to_id.as_deref(), Some("original-server"));
        assert_eq!(row.reply_to_text.as_deref(), Some("Raw :+1:"));
        assert_eq!(
            crate::message_navigation::message_mentions(row)[0].user_id,
            "22"
        );
        assert_eq!(row.delivery, DeliveryState::ServerAccepted);
        let no_server = project(
            "contact-1",
            false,
            124,
            "",
            Payload::Text {
                raw: "same text",
                mentions: &[],
                quoted: None,
            },
        )
        .unwrap();
        assert_eq!(no_server.messages[0].id, "out_124");
        assert!(project(
            "contact-1",
            false,
            124,
            "",
            Payload::Text {
                raw: "😀 @An ",
                mentions: &mentions,
                quoted: None
            }
        )
        .is_err());
    }

    #[test]
    fn confirmed_outgoing_forward_never_inherits_source_identity_or_reply() {
        let mut source = ChatMessage {
            id: "old-server".into(),
            client_message_id: "111".into(),
            text: "Forward raw".into(),
            reply_to_id: Some("old-quote".into()),
            delivery: DeliveryState::Read,
            wire_ref: crate::message_model::WireMessageRef::from_raw(
                &serde_json::json!({"msgId":"old-server","mentions":[{"uid":"old-uid","pos":0,"len":3}]}),
            ),
            ..Default::default()
        };
        let row = project(
            "target",
            true,
            200,
            "forward-server",
            Payload::Forward(&source),
        )
        .unwrap()
        .messages
        .remove(0);
        assert_eq!(row.id, "forward-server");
        assert_eq!(row.client_message_id, "200");
        assert_eq!(row.text, source.text);
        assert!(row.reply_to_id.is_none());
        assert!(crate::message_navigation::message_mentions(&row).is_empty());
        assert_eq!(row.delivery, DeliveryState::ServerAccepted);
        source.sticker_id = Some(77);
        source.sticker_cate_id = Some(88);
        source.sticker_type = Some(7);
        let row = project(
            "target",
            true,
            201,
            "sticker-server",
            Payload::Forward(&source),
        )
        .unwrap()
        .messages
        .remove(0);
        assert_eq!(row.kind, MessageKind::Sticker);
        assert_eq!(row.sticker_id, Some(77));
        assert_eq!(row.sticker_cate_id, Some(88));
        assert!(row.reply_to_id.is_none());
        assert_eq!(row.id, "sticker-server");
        assert_eq!(row.client_message_id, "201");
    }

    #[test]
    fn confirmed_outgoing_media_requires_transport_identity_and_keeps_actual_metadata() {
        let media = ChatMessage {
            id: "media-server".into(),
            client_message_id: "300".into(),
            is_outgoing: true,
            text: "[Hình ảnh] raw.png".into(),
            link_url: Some("fixture://raw.png".into()),
            link_thumb: Some("fixture://thumb.png".into()),
            kind: MessageKind::Photo,
            ..Default::default()
        };
        assert!(project(
            "target",
            false,
            301,
            "media-server",
            Payload::Attachment(&media)
        )
        .is_err());
        let row = project(
            "target",
            false,
            300,
            "media-server",
            Payload::Attachment(&media),
        )
        .unwrap()
        .messages
        .remove(0);
        assert_eq!(row.link_url.as_deref(), Some("fixture://raw.png"));
        assert_eq!(row.link_thumb.as_deref(), Some("fixture://thumb.png"));
        assert_eq!(row.delivery, DeliveryState::ServerAccepted);
    }

    #[tokio::test]
    async fn confirmed_outgoing_actual_history_sink_reopens_and_deduplicates_exact_ids() {
        let root = std::env::temp_dir().join(format!(
            "confirmed-outgoing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut captured = Session::new(false, false, false).unwrap();
        captured.login = Some(crate::session::LoginInfo {
            uin: "fixture-A".into(),
            ..Default::default()
        });
        captured.zpw_sek = Some("fixture-key".into());
        let shared = Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let row = project(
            "target",
            true,
            400,
            "accepted-server",
            Payload::Sticker {
                sticker_id: 77,
                category_id: 88,
                sticker_type: 7,
            },
        )
        .unwrap();
        let existing = Thread {
            id: "target".into(),
            name: "Existing thread name".into(),
            is_group: true,
            messages: vec![ChatMessage {
                id: "older-server".into(),
                text: "Old row".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let first = project(
            "target",
            true,
            401,
            "same-text-A",
            Payload::Text {
                raw: "Identical",
                mentions: &[],
                quoted: None,
            },
        )
        .unwrap();
        let second = project(
            "target",
            true,
            402,
            "same-text-B",
            Payload::Text {
                raw: "Identical",
                mentions: &[],
                quoted: None,
            },
        )
        .unwrap();
        for projection in [existing, row.clone(), row, first, second] {
            assert!(matches!(
                history::execute(
                    root.clone(),
                    shared.clone(),
                    captured.clone(),
                    "fixture-A".into(),
                    history::Request::Persist(vec![projection])
                )
                .await
                .unwrap(),
                history::Reply::Written(_)
            ));
        }
        let history::Reply::Threads(threads) = history::execute(
            root.clone(),
            shared.clone(),
            captured.clone(),
            "fixture-A".into(),
            history::Request::Load(Vec::new()),
        )
        .await
        .unwrap() else {
            panic!("Expected durable reload")
        };
        let target = threads.iter().find(|t| t.id == "target").unwrap();
        assert_eq!(target.name, "Existing thread name");
        assert_eq!(target.messages.len(), 4);
        assert_eq!(
            target
                .messages
                .iter()
                .filter(|row| row.text == "Identical")
                .count(),
            2
        );
        let accepted = target
            .messages
            .iter()
            .find(|row| row.id == "accepted-server")
            .unwrap();
        assert_eq!(accepted.client_message_id, "400");
        assert_eq!(accepted.sticker_id, Some(77));
        assert_eq!(accepted.delivery, DeliveryState::ServerAccepted);
        let mut other = captured;
        other.login.as_mut().unwrap().uin = "fixture-B".into();
        let other_shared = Arc::new(tokio::sync::Mutex::new(other.clone()));
        let history::Reply::Threads(other_rows) = history::execute(
            root.clone(),
            other_shared,
            other,
            "fixture-B".into(),
            history::Request::Load(Vec::new()),
        )
        .await
        .unwrap() else {
            panic!("Expected isolated reload")
        };
        assert!(other_rows.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
