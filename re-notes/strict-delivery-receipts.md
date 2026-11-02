# Strict delivery receipt restoration (R1)

## Evidence provenance

This restoration reads the archived implementation at Git object `20a7412`,
`src/delivery_receipts.rs`, `tests/delivery_receipts_regressions.rs`, and
`src/gui/worker.rs:1148,1826`. No live capture or private account data was used.
The archive records the owning original bundle as
`pc-dist/lazy/default-login-main-startup-shared-worker-znotification.d9141cdf23123a5c2330.js`,
SHA256 `0a5a08a2a1d074c827d2b1826f8434fc04e243a1daa0a03a376353e54cdd2466`.
These are archived provenance metadata, not an independently rehashed capture in
this slice. Archive Unicode code-point spans: command enum 7423900..7424300;
group prefix 1505622; getDeliveredsFromData 12307896..12308645; status application
11750050..11751080; processSeenFromServer 7667818..7669200; _processMessage
12321078 ignores subcommand.

## Restored interpretation

The inbound handler accepts 501/502/521/522/534/570 only. `delivereds`,
`groupSeens`, and `seens` are separate source-derived shapes. Group thread IDs
use the existing `g` prefix. Recipient fallback uses the source's first
`deliveredUids` entry only when an explicit thread is absent. Only nonzero exact
string or positive integer identities and strict numeric/string 0/1 flags count.
Own-only recipient reports are not remote delivery evidence. Error envelopes
are rejected, including the final depth-limited body (a hardened archive gap).
Versions and timestamps are not receipt clocks. Explicit null versus absent raw
version survives serialization. Send ACKs never become remote delivery.

Socket dispatch occurs before `is_chat_ish`, including receipt-only 502/522.
Polling uses the archive's 502 inbound interpretation without changing requests,
transport, signing, cadence, scheduler, or the local API. No raw seen transport
was imported. Tests are synthetic, not redacted capture claims.

## Mutation and integration boundary

Only unique exact outgoing server IDs match. A unique client ID may match an
unreconciled local row, never override a conflicting real server ID. Delivery
progress only advances; text, IDs, timestamp, reactions and unread do not change.
Deleted or unloaded rows are not resurrected. UI queues existing worker FIFO
persistence only after a real loaded-row update. This slice adds no durable
receipt inbox for unloaded messages and no schema migration.

`dispatch_delivery_receipts` checks captured account and session token before
channel emission. Parent runtime integration must generation-scope this handler,
`GuiEvent::DeliveryEvents`, and persistence commands, as for the other event
variants. Account/token checks alone do not prove same-account ABA fencing.
