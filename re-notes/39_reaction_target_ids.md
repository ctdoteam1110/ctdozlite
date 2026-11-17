# Reaction target message IDs

## Symptom

A reaction sent from the desktop client was accepted (`error_code` 0) and shown
locally, but did not appear on the phone or for the other participant.

## Cause

`send_reaction` (`src/chat.rs`) filled the `rMsg` entry with:

- `gMsgID` = `ChatMessage::id`. That field is a single canonical id: depending on
  the message kind and source, `parse_chat_message` stores the server `msgId` or the
  `cliMsgId` there (see `35_exact_message_identity.md`), so it is not reliably the
  server id.
- `cMsgID` = `ChatMessage::ts`, which is a timestamp, not the sender's `cliMsgId`.

The server stores the reaction against these values, so other devices could not
match it to a message.

## Change

`reaction_target_ids` now takes:

- `gMsgID` from `server_message_id` (wire `msgId`/`globalMsgId`),
- `cMsgID` from `client_message_id` (wire `cliMsgId`).

Fallbacks when no wire identity is stored (legacy SQLite rows), in order:

- `gMsgID`: `ChatMessage::id` (empty for local `out_`/`local_` rows), as before.
- `cMsgID`: `ChatMessage::id` when numeric, then `ts`, then the current time.
  Before this change `ts` came first. `id` is tried first as a best effort: it
  may be the `cliMsgId` or the server `msgId` (see above), so it is not a
  guaranteed client id, but it is an id; `ts` is a time and only matches when the
  sender happened to use it as `cliMsgId`. Rows with wire identity never reach
  this fallback.

Request shape, host, cipher, signing and cookies are
unchanged; only the values of the two existing fields differ. This matches the
`rMsg: [{ gMsgID: msgId, cMsgID: cliMsgId, msgType }]` contract of
`sendReactionMessage` in the web client.

## Verification

- Unit test `chat::tests::reaction_targets_server_and_client_ids_not_timestamp`.
- Manual: react to a message on desktop, check that it appears on the phone.
