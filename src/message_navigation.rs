//! Exact quoted-message lookup within the currently loaded conversation.
use crate::chat::ChatMessage;
use serde_json::Value;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

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

#[derive(Clone, Debug, PartialEq, Eq)]
/// A verified exact identity and a grapheme-aligned source-text span.
pub struct MentionSpan {
    /// Protocol UID, never inferred from display text.
    pub user_id: String,
    /// Half-open UTF-8 byte boundaries into the original message.
    pub byte_range: Range<usize>,
}

/// JS substring offsets are UTF-16 code units, not bytes or Unicode scalars.
pub fn utf16_byte_range(text: &str, position: usize, length: usize) -> Option<Range<usize>> {
    if length == 0 {
        return None;
    }
    let end = position.checked_add(length)?;
    let boundary = |wanted| {
        let mut units = 0;
        for (byte, ch) in text.char_indices() {
            if units == wanted {
                return Some(byte);
            }
            units += ch.len_utf16();
            if units > wanted {
                return None;
            }
        }
        (units == wanted).then_some(text.len())
    };
    let range = boundary(position)?..boundary(end)?;
    let is_grapheme_boundary =
        |byte| byte == text.len() || text.grapheme_indices(true).any(|(index, _)| index == byte);
    (is_grapheme_boundary(range.start) && is_grapheme_boundary(range.end)).then_some(range)
}

/// Original shared-worker-original.js CP143496: mentions[].pos/len are passed to
/// message.substring(pos, pos + len). Archive meta.mentionsList uses the same spans;
/// opaque archive UIDs require the exact-key sidecar from the identity decoder.
pub fn message_mentions(message: &ChatMessage) -> Vec<MentionSpan> {
    let wire = message.wire_ref.fields.get("mentions");
    let archive = message
        .history_source
        .as_ref()
        .and_then(|source| source.pointer("/meta/mentionsList"));
    let Some(mentions) = wire.or(archive).and_then(Value::as_array) else {
        return vec![];
    };
    let mut result = Vec::new();
    for mention in mentions {
        let Some(mut user_id) = mention.get("uid").and_then(exact_id) else {
            continue;
        };
        if user_id.trim().is_empty() || matches!(user_id.as_str(), "0" | "-1") {
            continue;
        }
        if wire.is_none() {
            if !user_id.bytes().all(|byte| byte.is_ascii_digit()) {
                let Some(resolved) = message.resolved_mention_uids.get(&user_id) else {
                    continue;
                };
                user_id = resolved.clone();
            }
            if user_id.is_empty()
                || !user_id.bytes().all(|byte| byte.is_ascii_digit())
                || !user_id.bytes().any(|byte| byte != b'0')
            {
                continue;
            }
        }
        let Some(position) = mention
            .get("pos")
            .and_then(Value::as_u64)
            .and_then(|number| usize::try_from(number).ok())
        else {
            continue;
        };
        let Some(length) = mention
            .get("len")
            .and_then(Value::as_u64)
            .and_then(|number| usize::try_from(number).ok())
        else {
            continue;
        };
        let Some(byte_range) = utf16_byte_range(&message.text, position, length) else {
            continue;
        };
        result.push(MentionSpan {
            user_id,
            byte_range,
        });
    }
    result.sort_by_key(|span| (span.byte_range.start, span.byte_range.end));
    result.dedup();
    // Conflicting/overlapping claims do not establish a clickable identity.
    result
        .iter()
        .enumerate()
        .filter(|(index, span)| {
            !result.iter().enumerate().any(|(other, candidate)| {
                other != *index
                    && span.byte_range.start < candidate.byte_range.end
                    && candidate.byte_range.start < span.byte_range.end
            })
        })
        .map(|(_, span)| span.clone())
        .collect()
}

#[cfg(test)]
mod mention_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inline_mention_utf16_ranges_reject_partial_graphemes_and_overflow() {
        let text = "😀👩‍💻e\u{301} @An";
        let start = text.find('@').unwrap();
        let units = text[..start].encode_utf16().count();
        assert_eq!(utf16_byte_range(text, units, 3), Some(start..text.len()));
        for (position, length) in [(1, 1), (2, 2), (7, 1), (0, 0), (usize::MAX, 2)] {
            assert_eq!(utf16_byte_range(text, position, length), None);
        }
    }

    #[test]
    fn inline_mention_keeps_duplicate_names_bound_to_exact_uids() {
        let mut message = ChatMessage {
            text: "😀 @An @An".into(),
            ..Default::default()
        };
        message.wire_ref.fields.insert(
            "mentions".into(),
            json!([
                {"uid":"11","pos":3,"len":3}, {"uid":"22","pos":7,"len":3}
            ]),
        );
        let spans = message_mentions(&message);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].user_id, "11");
        assert_eq!(spans[1].user_id, "22");
        assert_eq!(&message.text[spans[1].byte_range.clone()], "@An");
    }

    #[test]
    fn inline_mention_rejects_conflicting_claims_and_unresolved_archive_ids() {
        let mut message = ChatMessage {
            text: "@An".into(),
            ..Default::default()
        };
        message.wire_ref.fields.insert(
            "mentions".into(),
            json!([
                {"uid":"11","pos":0,"len":3}, {"uid":"22","pos":0,"len":3}
            ]),
        );
        assert!(message_mentions(&message).is_empty());
        message.wire_ref.fields.clear();
        message.history_source =
            Some(json!({"meta":{"mentionsList":[{"uid":"opaque-key","pos":0,"len":3}]}}));
        assert!(message_mentions(&message).is_empty());
        message
            .resolved_mention_uids
            .insert("opaque-key".into(), "123".into());
        assert_eq!(message_mentions(&message)[0].user_id, "123");
        message
            .resolved_mention_uids
            .insert("opaque-key".into(), "000".into());
        assert!(message_mentions(&message).is_empty());
    }
}
