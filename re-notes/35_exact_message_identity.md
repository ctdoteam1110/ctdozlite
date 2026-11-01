# Exact message identity reconciliation

## Evidence and contract

Source inspection at upstream commit `20e742c` (2026-11-02):

- `src/chat.rs:1170-1252` sends text and group text with an existing numeric
  `clientId`, generated once inside each function.
- `src/chat.rs:1323-1649` shares an attachment `client_id` across upload/send
  paths, but omits that value from the returned `ChatMessage`.
- `src/chat.rs:1912-2110` and `2760-2903` use the same existing `clientId`
  field for links, stickers, replies and forwarding via text/sticker sends.
- `src/chat.rs:789-900`, `merge_previews`, and `merge_group_history` previously
  matched text/clock values as a fallback. Distinct server IDs could disappear.
- `parse_chat_message` previously discarded client/wire identity and selects
  `cliMsgId` before `globalMsgId`. That canonical ID priority is retained because
  SQLite stores the chosen ID but not the original wire/client references.
  A schema-aware canonical-ID migration is intentionally not included.
- `src/gui.rs:945-1050` inserts local rows after worker success events. A server
  push can therefore precede that insertion; reconciliation must work in both
  arrival orders. Display names are not sender identities.

These are repository observations, not new live protocol captures. The change
adds no request fields and changes no host, cipher, signing or cookie path.
Public send functions retain their signatures. Internal send variants accept
one caller-owned client ID and preserve it across the already-existing fallback
paths. A process-local monotonic millisecond-shaped allocator prevents two sends
in one clock tick from sharing that value. GUI success events and attachment
rows retain that exact request ID; local call-log rows have no client ID.

## Conservative matching rules

- Nonempty server IDs match exactly; positive integral numeric and string wire
  IDs have the same textual representation. Numeric zero, negative/fractional
  IDs, empty strings and the string zero do not establish message identity.
  Sender zero remains valid as the existing outgoing sender sentinel.
- The persisted canonical ID remains an exact compatibility alias. When both
  rows have explicit wire server IDs, differing server IDs reject that alias.
  Legacy SQLite rows without wire metadata retain their old canonical-ID match.
  Recall target IDs remain primary rather than becoming recall-event IDs.
- Nonempty local IDs match exactly. Client IDs bridge an outgoing `out_*` row
  and a server row, or identify replayed client-only packets before a server ID
  is available. Two distinct pending/server IDs never merge by client ID.
- Empty IDs, conflicting client/wire references, ambiguous indexed rows,
  differing direction, and differing known wire senders do not establish a
  match. Text and timestamps never establish a match.
- Successful merges retain existing enrichment and merge wire/reaction/delivery
  metadata. Index membership is refreshed after enrichment and ID promotion.
  Local success acknowledgements preserve the timestamp of an existing server
  row rather than moving that message to the acknowledgement receipt time.
- Index lookup/update is expected O(n + m), followed by the existing timestamp
  sort. Preview thread lookup retains its existing linear thread search.

## Verification

`tests/message_identity_integration.rs` uses synthetic messages and JSON
responses to cover distinct equal-text/time IDs, exact echoes, missing and
conflicting IDs, ambiguity, direction/sender collisions, metadata enrichment,
replays, preview/history parsing, an actual in-memory legacy SQLite persist/
hydrate roundtrip, explicit server-ID collisions behind a legacy client alias,
invalid numeric references and a 5,000 + 5,000 message merge.

`chat::send_identity_tests` runs synthetic loopback HTTP fixtures, decrypts the
existing request body with a fixture key, and verifies the supplied `clientId`
through text/group/link/sticker/reply/forward requests and link/reply fallback.
It also checks 10,000 allocations for strictly increasing IDs.

`gui::tests::sent_message_reconciles_when_server_push_arrives_first` executes
the merge helper used by all send-success handlers with server-first identity
and repeated acknowledgements. The delayed-local-ack test preserves server
message A at 1000 before B at 2000 when A's acknowledgement arrives at 3000.
These are runtime model regressions, not rendered desktop or live-account
acceptance tests.
