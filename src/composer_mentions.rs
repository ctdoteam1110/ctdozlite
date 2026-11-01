//! Explicit group-member selection and UTF-16 mention metadata for in-memory composer drafts.
use crate::chat::{ChatMessage, SidebarMember};
use crate::message_navigation::utf16_byte_range;
use std::collections::HashMap;
use std::ops::Range;

/// One explicitly selected user mention in the outbound PC protocol representation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OutgoingMention {
    /// Exact selected member UID, never inferred from the displayed name.
    pub uid: String,
    /// Start offset in UTF-16 code units of the unchanged message string.
    pub pos: usize,
    /// Length in UTF-16 code units, including the leading at-sign.
    pub len: usize,
    /// PC MENTION_TYPE_USER is zero; all-member mentions are not synthesized.
    #[serde(rename = "type")]
    pub kind: u8,
}

/// Reject malformed identity/ranges before constructing signed outbound parameters.
///
/// # Errors
/// Returns an error for unsupported mention types, invalid UIDs, overlapping or partial-grapheme spans.
pub fn validate_mentions(text: &str, mentions: &[OutgoingMention]) -> anyhow::Result<()> {
    let mut end = 0;
    for mention in mentions {
        anyhow::ensure!(
            !mention.uid.trim().is_empty() && !matches!(mention.uid.as_str(), "0" | "-1"),
            "UID nhắc tên không hợp lệ"
        );
        anyhow::ensure!(mention.kind == 0, "Kiểu nhắc tên chưa được xác minh");
        let range = utf16_byte_range(text, mention.pos, mention.len)
            .ok_or_else(|| anyhow::anyhow!("Vị trí nhắc tên không hợp lệ"))?;
        anyhow::ensure!(
            range.start >= end && text[range.clone()].starts_with('@'),
            "Vùng nhắc tên không hợp lệ"
        );
        end = range.end;
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct SelectedMention {
    uid: String,
    label: String,
    bytes: Range<usize>,
}

#[derive(Default)]
struct Draft {
    text: String,
    mentions: Vec<SelectedMention>,
    reply: Option<ChatMessage>,
}

/// In-memory draft metadata stays bound to its thread while the existing GUI owns text/reply fields.
#[derive(Default)]
pub(crate) struct ComposerMentions {
    thread_id: String,
    snapshot: String,
    mentions: Vec<SelectedMention>,
    drafts: HashMap<String, Draft>,
}

impl ComposerMentions {
    /// Save the outgoing thread draft and restore only the incoming thread's exact draft.
    pub(crate) fn switch_thread(
        &mut self,
        thread: &str,
        text: &mut String,
        reply: &mut Option<ChatMessage>,
    ) {
        if self.thread_id == thread {
            self.reconcile(text);
            return;
        }
        if !self.thread_id.is_empty() {
            self.reconcile(text);
            self.drafts.insert(
                self.thread_id.clone(),
                Draft {
                    text: std::mem::take(text),
                    mentions: std::mem::take(&mut self.mentions),
                    reply: reply.take(),
                },
            );
            let draft = self.drafts.remove(thread).unwrap_or_default();
            *text = draft.text;
            *reply = draft.reply;
            self.mentions = draft.mentions;
        }
        self.thread_id = thread.into();
        self.snapshot = text.clone();
    }

    /// Shift untouched spans after a contiguous edit; edits intersecting a mention remove its identity.
    pub(crate) fn reconcile(&mut self, text: &str) {
        if text == self.snapshot {
            return;
        }
        let mut prefix = 0;
        for (old, new) in self.snapshot.chars().zip(text.chars()) {
            if old != new {
                break;
            }
            prefix += old.len_utf8();
        }
        let mut suffix = 0;
        for (old, new) in self.snapshot[prefix..]
            .chars()
            .rev()
            .zip(text[prefix..].chars().rev())
        {
            if old != new {
                break;
            }
            suffix += old.len_utf8();
        }
        let old_end = self.snapshot.len() - suffix;
        let new_end = text.len() - suffix;
        // Repeated visible labels can make a text-only edit position ambiguous.
        // Retain an identity only when every compatible contiguous edit leaves it untouched.
        let shared_suffix: usize = self
            .snapshot
            .chars()
            .rev()
            .zip(text.chars().rev())
            .take_while(|(old, new)| old == new)
            .map(|(ch, _)| ch.len_utf8())
            .sum();
        let certain_prefix = prefix.min(self.snapshot.len().min(text.len()) - shared_suffix);
        self.mentions.retain_mut(|mention| {
            if mention.bytes.end <= certain_prefix {
                return true;
            }
            if mention.bytes.start >= old_end {
                mention.bytes = (new_end + mention.bytes.start - old_end)
                    ..(new_end + mention.bytes.end - old_end);
                return text.get(mention.bytes.clone()) == Some(mention.label.as_str());
            }
            false
        });
        self.snapshot = text.into();
    }

    /// Replace the current selection or at-query with a member selected by exact UID.
    /// Returns the new cursor position in Unicode scalar units for egui TextEdit.
    pub(crate) fn select_member(
        &mut self,
        text: &mut String,
        member: &SidebarMember,
        selection: Range<usize>,
    ) -> Option<usize> {
        self.reconcile(text);
        let name = member.name.trim();
        if member.id.trim().is_empty()
            || matches!(member.id.as_str(), "0" | "-1")
            || name.is_empty()
            || name.chars().any(char::is_control)
        {
            return None;
        }
        if selection.start > selection.end || selection.end > text.chars().count() {
            return None;
        }
        let byte_at = |index| {
            text.char_indices()
                .nth(index)
                .map(|(byte, _)| byte)
                .unwrap_or(text.len())
        };
        let mut start = byte_at(selection.start);
        let end = byte_at(selection.end);
        if start > end {
            return None;
        }
        if start == end {
            let token = text[..start]
                .rfind(char::is_whitespace)
                .map(|at| at + text[at..].chars().next().unwrap().len_utf8())
                .unwrap_or(0);
            if text[token..start].starts_with('@') {
                start = token;
            }
        }
        let gap = start > 0 && !text[..start].ends_with(char::is_whitespace);
        let label = format!("@{name}");
        let replacement = format!("{}{label} ", if gap { " " } else { "" });
        // Replacing identical visible text still replaces the selected identity.
        self.mentions.retain(|mention| {
            mention.bytes.end <= start
                || mention.bytes.start >= end
                    && !(mention.bytes.start < start && start < mention.bytes.end)
        });
        text.replace_range(start..end, &replacement);
        // The picker owns this exact edit range, unlike an arbitrary TextEdit diff.
        for mention in &mut self.mentions {
            if mention.bytes.start >= end {
                mention.bytes = (start + replacement.len() + mention.bytes.start - end)
                    ..(start + replacement.len() + mention.bytes.end - end);
            }
        }
        self.snapshot = text.clone();
        let first = start + usize::from(gap);
        self.mentions.push(SelectedMention {
            uid: member.id.clone(),
            label: label.clone(),
            bytes: first..first + label.len(),
        });
        self.mentions.sort_by_key(|mention| mention.bytes.start);
        Some(text[..start + replacement.len()].chars().count())
    }

    /// Emit only surviving explicit selections; raw text never creates mention identities.
    pub(crate) fn outgoing(&mut self, text: &str) -> Vec<OutgoingMention> {
        self.reconcile(text);
        let mentions: Vec<_> = self
            .mentions
            .iter()
            .filter_map(|mention| {
                if text.get(mention.bytes.clone()) != Some(mention.label.as_str()) {
                    return None;
                }
                let pos = text[..mention.bytes.start].encode_utf16().count();
                let len = mention.label.encode_utf16().count();
                utf16_byte_range(text, pos, len)?;
                Some(OutgoingMention {
                    uid: mention.uid.clone(),
                    pos,
                    len,
                    kind: 0,
                })
            })
            .collect();
        if validate_mentions(text, &mentions).is_ok() {
            mentions
        } else {
            Vec::new()
        }
    }

    /// Restore a failed send only into an empty draft for the same thread, without overwriting edits.
    pub(crate) fn restore_failed(
        &mut self,
        thread: &str,
        text: String,
        mentions: &[OutgoingMention],
        quoted: Option<ChatMessage>,
        current_text: &mut String,
        current_reply: &mut Option<ChatMessage>,
    ) -> bool {
        if validate_mentions(&text, mentions).is_err() {
            return false;
        }
        let selected = mentions
            .iter()
            .filter_map(|mention| {
                let bytes = utf16_byte_range(&text, mention.pos, mention.len)?;
                Some(SelectedMention {
                    uid: mention.uid.clone(),
                    label: text[bytes.clone()].into(),
                    bytes,
                })
            })
            .collect();
        if thread == self.thread_id {
            if !current_text.is_empty() || current_reply.is_some() {
                return false;
            }
            *current_text = text;
            *current_reply = quoted;
            self.snapshot = current_text.clone();
            self.mentions = selected;
        } else {
            if self
                .drafts
                .get(thread)
                .is_some_and(|draft| !draft.text.is_empty() || draft.reply.is_some())
            {
                return false;
            }
            self.drafts.insert(
                thread.into(),
                Draft {
                    text,
                    mentions: selected,
                    reply: quoted,
                },
            );
        }
        true
    }

    /// Clear identities after the GUI consumes the draft text for a send.
    pub(crate) fn clear(&mut self) {
        self.mentions.clear();
        self.snapshot.clear();
    }
}

#[cfg(test)]
mod composer_tests {
    use super::*;

    fn member(id: &str) -> SidebarMember {
        SidebarMember {
            id: id.into(),
            name: "An".into(),
            avatar: None,
        }
    }

    #[test]
    fn composer_edits_shift_utf16_and_remove_only_the_touched_identity() {
        let mut model = ComposerMentions::default();
        let mut text = "  😀 @".to_string();
        let mut reply = None;
        model.switch_thread("group-1", &mut text, &mut reply);
        let end = text.chars().count();
        model
            .select_member(&mut text, &member("11"), end..end)
            .unwrap();
        let end = text.chars().count();
        model
            .select_member(&mut text, &member("22"), end..end)
            .unwrap();
        let original = text.clone();
        let first = model.outgoing(&text);
        assert_eq!(
            first
                .iter()
                .map(|mention| mention.uid.as_str())
                .collect::<Vec<_>>(),
            ["11", "22"]
        );
        text.insert_str(0, "👩‍💻 ");
        let shifted = model.outgoing(&text);
        assert_eq!(shifted[0].pos, first[0].pos + 6);
        assert_eq!(shifted[1].pos, first[1].pos + 6);
        let at = text.find("@An").unwrap();
        text.replace_range(at + 1..at + 2, "X");
        let remaining = model.outgoing(&text);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].uid, "22");
        assert!(original.starts_with("  😀 @An @An"));
    }

    #[test]
    fn composer_thread_switch_preserves_raw_draft_reply_and_identity_without_leaking() {
        let mut model = ComposerMentions::default();
        let mut text = "😀 ".to_string();
        let mut reply = Some(ChatMessage {
            id: "reply-1".into(),
            text: "Raw :+1:".into(),
            ..Default::default()
        });
        model.switch_thread("group-1", &mut text, &mut reply);
        model.select_member(&mut text, &member("22"), 2..2).unwrap();
        let expected = text.clone();
        model.switch_thread("group-2", &mut text, &mut reply);
        assert!(text.is_empty());
        assert!(reply.is_none());
        text = "@An".into();
        assert!(model.outgoing(&text).is_empty());
        model.switch_thread("group-1", &mut text, &mut reply);
        assert_eq!(text, expected);
        assert_eq!(reply.as_ref().unwrap().id, "reply-1");
        assert_eq!(reply.as_ref().unwrap().text, "Raw :+1:");
        assert_eq!(model.outgoing(&text)[0].uid, "22");
    }

    #[test]
    fn composer_failed_send_restores_only_its_empty_thread_and_preserves_reply() {
        let mut model = ComposerMentions::default();
        let mut text = String::new();
        let mut reply = None;
        model.switch_thread("group-1", &mut text, &mut reply);
        model.switch_thread("group-2", &mut text, &mut reply);
        let mentions = [OutgoingMention {
            uid: "22".into(),
            pos: 0,
            len: 3,
            kind: 0,
        }];
        assert!(model.restore_failed(
            "group-1",
            "@An".into(),
            &mentions,
            Some(ChatMessage {
                id: "quoted".into(),
                ..Default::default()
            }),
            &mut text,
            &mut reply
        ));
        assert!(text.is_empty());
        assert!(reply.is_none());
        model.switch_thread("group-1", &mut text, &mut reply);
        assert_eq!(text, "@An");
        assert_eq!(reply.as_ref().unwrap().id, "quoted");
        assert_eq!(model.outgoing(&text)[0].uid, "22");
        assert!(!model.restore_failed(
            "group-1",
            "@An".into(),
            &mentions,
            None,
            &mut text,
            &mut reply
        ));
    }

    #[test]
    fn composer_identical_label_reselection_uses_new_exact_uid() {
        let mut model = ComposerMentions::default();
        let mut text = String::new();
        model.select_member(&mut text, &member("11"), 0..0).unwrap();
        model.select_member(&mut text, &member("22"), 0..3).unwrap();
        assert_eq!(model.outgoing(&text)[0].uid, "22");
        assert_eq!(model.outgoing(&text).len(), 1);
    }
    #[test]
    fn composer_ambiguous_identical_label_deletion_never_retargets_another_uid() {
        let mut model = ComposerMentions::default();
        let mut text = String::new();
        model.select_member(&mut text, &member("11"), 0..0).unwrap();
        let end = text.chars().count();
        model
            .select_member(&mut text, &member("22"), end..end)
            .unwrap();
        assert_eq!(text, "@An @An ");
        // Either deletion produces the same raw string. No UID can be inferred from that string.
        text.drain(..4);
        assert!(model.outgoing(&text).is_empty());
    }
}
