//! Exact quoted-message lookup within the currently loaded conversation.
use crate::chat::ChatMessage;
use serde_json::Value;

fn exact_id(value: &Value) -> Option<String> {
    match value {
        Value::String(id) if !id.is_empty() => Some(id.clone()),
        Value::Number(id) if id.is_u64() || id.is_i64() => Some(id.to_string()),
        _ => None,
    }
}

/// Resolve a reference only when exactly one loaded message matches any ID namespace.
/// Text, timestamps, numeric coercion and namespace preference never disambiguate IDs.
pub fn unique_reference_index(messages: &[ChatMessage], reference: &str) -> Option<usize> {
    if !valid_reference(reference) {
        return None;
    }
    let mut matches = messages.iter().enumerate().filter(|(_, message)| {
        message.id == reference
            || message.client_message_id == reference
            || [&message.wire_ref.server_id, &message.wire_ref.client_id]
                .into_iter()
                .flatten()
                .any(|id| exact_id(id).as_deref() == Some(reference))
    });
    let (index, _) = matches.next()?;
    matches.next().is_none().then_some(index)
}

fn valid_reference(reference: &str) -> bool {
    !reference.trim().is_empty() && !reference.bytes().all(|byte| byte == b'0')
}

fn reply_reference(quote: &Value) -> Option<String> {
    ["cliMsgId", "globalMsgId", "msgId"]
        .into_iter()
        .filter_map(|key| quote.get(key).and_then(exact_id))
        .find(|id| valid_reference(id))
}

/// Read an explicit reply reference, preserving its original ID representation.
pub fn message_reply_reference(message: &ChatMessage) -> Option<String> {
    message
        .reply_to_id
        .as_ref()
        .filter(|id| valid_reference(id))
        .cloned()
        .or_else(|| {
            message
                .wire_ref
                .fields
                .get("quote")
                .and_then(reply_reference)
        })
        .or_else(|| {
            message
                .history_source
                .as_ref()
                .and_then(|raw| raw.pointer("/meta/quote"))
                .and_then(reply_reference)
        })
}

/// Widen the newest-message window enough to lay out the target and preceding context.
pub(crate) fn render_limit_for_target(current: usize, count: usize, target: usize) -> usize {
    current.max(count.saturating_sub(target.saturating_sub(3)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(id: &str, client: &str) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            client_message_id: client.into(),
            ..Default::default()
        }
    }

    #[test]
    fn quote_navigation_matches_each_exact_id_namespace() {
        let mut wire = message("stored", "local");
        wire.wire_ref.server_id = Some(json!(123));
        wire.wire_ref.client_id = Some(json!("00123"));
        for reference in ["stored", "local", "123", "00123"] {
            assert_eq!(unique_reference_index(&[wire.clone()], reference), Some(0));
        }
        assert_eq!(unique_reference_index(&[wire], "0123"), None);
    }

    #[test]
    fn quote_navigation_rejects_collisions_and_missing_ids() {
        let messages = [
            message("server-1", "client-1"),
            message("client-1", "client-2"),
        ];
        assert_eq!(unique_reference_index(&messages, "client-1"), None);
        assert_eq!(unique_reference_index(&messages, "not-loaded"), None);
        assert_eq!(unique_reference_index(&messages, ""), None);
        assert_eq!(unique_reference_index(&[message("0", "0")], "0"), None);
    }

    #[test]
    fn quote_navigation_rejects_cross_namespace_wire_collisions() {
        let first = message("server-1", "client-1");
        let mut second = message("server-2", "client-2");
        second.wire_ref.server_id = Some(json!("client-1"));
        assert_eq!(unique_reference_index(&[first, second], "client-1"), None);
    }

    #[test]
    fn quote_navigation_ignores_identical_text_and_timestamps() {
        let messages = [message("one", ""), message("two", "")];
        assert_eq!(unique_reference_index(&messages, "two"), Some(1));
    }

    #[test]
    fn quote_navigation_preserves_wire_and_archive_references() {
        let mut msg = message("reply", "");
        msg.wire_ref.fields.insert(
            "quote".into(),
            json!({"cliMsgId":"0", "globalMsgId":"00123"}),
        );
        assert_eq!(message_reply_reference(&msg).as_deref(), Some("00123"));
        msg.wire_ref.fields.clear();
        msg.history_source = Some(json!({"meta":{"quote":{"msgId":123}}}));
        assert_eq!(message_reply_reference(&msg).as_deref(), Some("123"));
        msg.reply_to_id = Some("explicit".into());
        assert_eq!(message_reply_reference(&msg).as_deref(), Some("explicit"));
    }

    #[test]
    fn quote_navigation_widens_window_for_target_outside_rendered_range() {
        let limit = render_limit_for_target(150, 1000, 12);
        assert_eq!(limit, 991);
        assert!(1000 - limit <= 12);
        assert_eq!(render_limit_for_target(150, 1000, 999), 150);
    }
}
