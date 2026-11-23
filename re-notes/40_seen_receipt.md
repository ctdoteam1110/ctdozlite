# Seen receipts (`seenv2`)

## Symptom

A conversation read in the desktop client still showed as unread on the phone.

## Cause

Two problems, both on the client side:

1. **Receipts were sent from one place only.** The GUI marks a thread read locally
   in three places, but only one of them sent `WorkerCmd::SendSeen`:
   - clicking the thread in the sidebar: sent a receipt;
   - a new message arriving in the open, focused thread: local read point only,
     and only when `collect_fresh_incoming_unread` accepted the message, which it
     often does not (it skips ids it cannot order against the read point);
   - the window regaining focus on the open thread (`mark_active_thread_read_if_focused`):
     local read point only.

   Reading new messages in a conversation that is already open is the common case,
   and it never reached the server.
2. **The receipt named the wrong message ids.** `send_seen_v2` put
   `ChatMessage::id` into both `gmi` and `cmi`. That field holds either the server
   `msgId` or the client `cliMsgId`, depending on the source
   (`35_exact_message_identity.md`); this is the same issue as
   `39_reaction_target_ids.md`. The receipt also named `messages.last()`, which is
   often our own outgoing message.

## Change

- `gui::send_seen_receipt` is called from all three places. For live messages it
  runs after every `ThreadsUpdated` batch that touches the open thread while the
  window is focused, independent of the unread filter. It names the newest
  incoming, non-system message. It still does nothing when "Ẩn trạng thái đã xem"
  (`block_seen`) is on. It remembers the last `gmi` sent per thread, so repaints do
  not resend the same receipt.
- `chat::seen_target_ids` takes `gmi` from the wire server id and `cmi` from the
  wire client id, using the same resolution as the reaction fix. When the client id
  is unknown, it falls back to the server id for both fields, which is what every
  receipt sent before this change did. It returns `None` for a `local_` row.
- `send_seen_v2` now returns an error when the server answers a non-zero
  `error_code`, and the worker logs it (`gui::worker::send_seen`, path only,
  no query string). Before, the result was dropped.

Review follow-up:

- A receipt is sent only while the conversation is on screen. Contacts and Feed replace
  the chat view but keep the selected thread, so a focused window alone is not enough.
- The worker reports a failed `seenv2` request back to the GUI (`SeenReceiptFailed`),
  which forgets that receipt; the next read of the thread sends it again.
- The record of sent receipts belongs to one account and is cleared when another account
  becomes active, so a group shared by two accounts gets a receipt from each.

Second review (after the rebase):

- The receipt target skips only protocol system messages (`MessageKind::System` or a
  local `sys_` id). The text-based `is_system_notice_message` matched ordinary text
  such as "Tôi đã đổi tên nhóm rồi", so the newest message could go unreported.
- A message that arrives in the selected chat while Contacts or Feed hides it is not
  counted unread. When the chat is shown again, `mark_active_thread_read_if_focused`
  now reports it and advances the read point, even with no unread entry
  (`report_unreported_arrival`). `SeenReceipts` keeps later frames from resending.

No other part of the request changed: the endpoints, `msgInfos` shape, `grid` /
`senderId` and `imei` are the same.

## Verification

- Unit tests:
  - `seen_receipts_dedupe_per_account_and_retry_after_failure` (`src/gui.rs`);
  - `seen_target_ids_prefer_wire_server_and_client_ids` (`src/chat.rs`);
  - `seen_receipt_targets_newest_incoming_message`,
    `seen_receipt_skips_only_protocol_system_messages` and
    `message_that_arrived_while_chat_was_hidden_is_reported_once` (`src/gui.rs`).
- Manual, on a real account:
  1. Keep a 1-1 conversation and a group open in the desktop client.
  2. Send a message to each from another account.
  3. Check that the phone clears the unread badge without opening the conversation
     there.
  4. In dev mode, check the log for `gui::worker::send_seen` entries.

  Result (2026-11-19, with a temporary log of each receipt and its response):
  - first build: only the sidebar-click path sent receipts; a new message in the
    open thread sent none;
  - after moving the live path to the end of `ThreadsUpdated`: 3 group receipts
    and 1 one-to-one receipt, every one answered
    `{"error_code":0,"error_message":"Successful.","data":{"status":0}}`;
  - the phone then cleared the unread badge for those conversations.
