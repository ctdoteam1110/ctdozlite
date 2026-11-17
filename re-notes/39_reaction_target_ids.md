# Reaction target message IDs

## Symptom

A reaction sent from the desktop client was accepted (`error_code` 0) and shown
locally, but did not appear on the phone or for the other participant.

## Cause

`send_reaction` (`src/chat.rs`) filled the `rMsg` entry with:

- `gMsgID` = `ChatMessage::id`. Per `35_exact_message_identity.md`,
  `parse_chat_message` selects `cliMsgId` before `globalMsgId` for that field, so
  for most rows this is the client id, not the server message id.
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
  Before this change `ts` came first. `id` now wins because `parse_chat_message`
  stores `cliMsgId` there when present (note 35), so for legacy rows it is the
  best available client id; `ts` is a time, not an id, and only matches when the
  sender happened to use it as `cliMsgId`.

Request shape, host, cipher, signing and cookies are
unchanged; only the values of the two existing fields differ. This matches the
`rMsg: [{ gMsgID: msgId, cMsgID: cliMsgId, msgType }]` contract of
`sendReactionMessage` in the web client.

## Verification

- Unit test `chat::tests::reaction_targets_server_and_client_ids_not_timestamp`.
- Manual: react to a message on desktop, check that it appears on the phone.
