//! Pure local read-state transitions; this module never sends network receipts.

/// Advance local visibility state independently of wire-receipt eligibility.
/// The caller supplies viewport candidates after its own foreground and dwell gating.
pub fn read_local_visible(
    state: &mut crate::unread::UnreadState,
    messages: &[crate::chat::ChatMessage],
) -> bool {
    if messages.is_empty() {
        return false;
    }
    let before = state.clone();
    state.read_visible(messages);
    // A manual dot has no numeric cursor and can apply to opaque archive identities.
    state.marked_unread = false;
    *state != before
}
