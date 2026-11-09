# Connection loss between phone request groups — 2026-11-08

Author: Codex. No account identities, signed locations or raw message data are included.

## Current live result

The scoped-absence build completed request group 1 and attempted group 2/17. Its terminal
report retained 30 scopes in the next request, 504 total scopes, zero identity-resolution
skips, zero new phone rows and 30 distinct scopes with a received 502 range outcome.
The second request lost its acknowledgement when the WebSocket disconnected. By final
cleanup, the transport had authenticated again, disposals were acknowledged, and local
transfer files were removed. No legacy fallback was started.

The user subsequently reported a Cloud/Group-only result (142 new messages / 320
conversations), explicitly saying phone history had not loaded. This is not full phone
success and does not establish the initiator of the disconnect. The first group also
decoded successful message data that was already present locally. Therefore a 502 range
outcome must not be described as proof that the entire conversation/history is absent.

## Static cross-check

The official shared worker's `Ao.nextIndex` at offset 276784 starts at zero and increases
per user-sync operation. Its session manager at offset 292355 reuses identity, generates
a fresh ephemeral key and includes the one-time key only for its first request. The
current construction matches those properties. This comparison does not prove every
request field or live transport behavior is correct.

## Diagnostic follow-up

Keep the numeric WebSocket close code in the connection registration, and include it
and the actual outgoing request byte count when failing a queued request. Do not expose
the server's raw close reason, request body, URL, key or conversation identities. Preserve
the delivery-uncertain error type and the prohibition on automatic legacy retry.

Retain counts for each completed request group before replacing its state: scope count,
cumulative imported rows, scoped absences, received/completed file counts and expected
count. The final report then distinguishes a completed preceding group from an unconfirmed
next request. No new wire request behavior is introduced by this diagnostic follow-up.

Full multi-request phone import remains unverified. The prior installed build passed 324
Rust tests; validation of this follow-up is recorded separately in the local release review.
