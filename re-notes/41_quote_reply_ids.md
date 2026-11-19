# Quote replies (`/api/message/quote`, `/api/group/quote`)

## Symptom

A reply sent from the desktop client arrived on the phone as a plain message, with no
link to the quoted message. Seen in a group, replying to another member.

## Cause

`send_quote_message_with_client_id` filled the quote fields from the wrong data:

- `qmsgOwner` came from `ChatMessage::sender`. For group rows stored from live pushes
  that field is the display name ("Vũ Thái Học"), not a uid, so groups always sent
  `qmsgOwner = "0"`.
- `qmsgId` came from `ChatMessage::id`, which can hold the `cliMsgId`
  (`35_exact_message_identity.md`).
- `qmsgCliId` was set to the quoted message's timestamp, not its `cliMsgId`.

The server accepted the request (`error_code 0`) in every case, so nothing showed the
problem. When the quote request did fail, the client fell back to a plain message
without logging anything.

## Change

- `chat::quote_target` takes the owner from the wire sender (`sidebar_sender_id`),
  then a numeric `sender`, then the peer id for 1-1, and our uid for our own messages.
  `qmsgId` and `qmsgCliId` come from the wire server and client ids. `qmsgCliId` falls
  back to `qmsgId` when the client id is unknown.
- A quote with no server id is sent as a plain message, as before.
- When the quote request fails, the error and the server's `error_message` are logged
  (`chat::send_quote`, dev mode) before the plain-message fallback.

The endpoints, the other body fields and `qmsgTs` are unchanged.

## Verification

- Unit test `quote_target_uses_wire_sender_and_message_ids` (`src/chat.rs`).
- Manual, on a real account, with a temporary log of each quote request (2026-11-19):
  - 1-1 ("Truyền File", own message): `qmsgCliId` is now the real `cliMsgId`, not the
    timestamp; the server answered `error_code 0`; the phone showed the quote.
  - Group, replying to another member: `qmsgOwner` changed from `"0"` to the member's
    uid, and `qmsgCliId` from the timestamp to the `cliMsgId`; the server answered
    `error_code 0`; the phone showed the quote, which it did not before the change.
