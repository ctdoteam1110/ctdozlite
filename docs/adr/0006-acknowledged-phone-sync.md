# ADR-0006: Acknowledged, session-scoped phone sync

- **Status:** accepted for experimental 0.0.9 distribution (2026-11-09)
- **Date:** 2026-11-07
- **Deciders:** maintainer (authorized release with the documented phone-sync limitation)

## Context

The v2 request previously reported success when a frame was merely queued, including when no
socket was available. The live legacy route could return success without a phone notification.
After correlating the v2 acknowledgement, the phone received a prompt but the user reported Zalo
exiting after confirming backup. Static comparison then identified an empty message partition.
This malformed scope is confirmed; causation of the phone crash is not established.

The receive pipeline also discarded plaintext embedded in metadata, ignored multiple encrypted
records in a file, and deleted session material before subsequent batches could use it.
Owning-source offsets and the limits of live evidence are in [re-notes/39](../../re-notes/39_phone_sync_rpc.md).

## Decision

- Await authenticated WebSocket command 590 by its request ID. A definitive rejection must not
  silently start another protocol. Legacy RSA GET remains an availability fallback.
- Resolve server-issued global identities and use `oneone/<globalId>` / `group/<globalId>` phone
  partitions; translate returned partitions and senders back to desktop IDs before storage.
  Never substitute a desktop ID when mapping is unavailable; disclose omitted scopes. See
  [re-notes/40](../../re-notes/40_phone_sync_identity_namespace.md) for the corrected namespace.
  Use the existing genuine zproto key generator. The cryptographic
  primitives, key encoding and wasm source remain unchanged. Private halves remain in the helper's
  private file; only public halves leave the helper through stdout.
- Track the transfer by syncId and owner. Ignore stale controls. Serialize helper-file operations
  through an owned semaphore permit that remains alive inside blocking work, even if its caller exits.
- Parse all length-prefixed records, retain both embedded plaintext and the session record, and keep
  key material until every declared batch is imported. Insert message rows transactionally and
  idempotently by the original 64-bit global message ID (INV-4).
- End the remote transfer through acknowledged command 591 with the observed lifecycle reason,
  then remove local transfer files. A missing disposal acknowledgement is logged generically;
  it does not undo already committed rows.
- UI success requires a complete import. Expose an optional current-conversation scope to support
  limited testing and user control.
- Match the native 30-scope request boundary. Keep one identity key, generate a new ephemeral key
  for each request, increment the native sequence index and dispose remote sessions after the
  whole operation. Intermediate requests cannot report overall success. Delivery uncertainty
  after queueing cannot trigger legacy fallback. Evidence: [re-notes/41](../../re-notes/41_phone_sync_request_batches.md).
- Account for explicit message-partition absence (wire 502) as a scoped zero-row result
  only when every reported partition belongs to the active request. Continue the other
  files, retain index/final-marker completion, and disclose cumulative absent scopes.
  Metadata, malformed/unscoped and other failures still stop the transfer. Evidence:
  [re-notes/42](../../re-notes/42_phone_partition_not_found.md).
- Preserve a queued request across acknowledgement loss for a bounded observation window.
  Continue only on evidence from its exact phone session, distinct from RPC acknowledgement;
  never retransmit that request. Wait for normal reconnection before a subsequent unsent
  group. Rejections and unobserved delivery remain errors. Evidence and limits:
  [re-notes/44](../../re-notes/44_phone_receipt_after_disconnect.md).
- Keep automatic server refresh in a separate command/result lane. It yields and finishes
  its polling before an explicit phone request starts; it cannot own or overwrite phone
  progress/completion. Evidence: [re-notes/45](../../re-notes/45_background_refresh_phone_ui.md).

## Consequences and verification

No dependencies or persistent database schema change. Previously persisted messages remain intact.
The helper continues to require local Node.js and the user's installed Zalo PC wasm; it is not
bundled from an external source. No account data, private keys or signed download links are fixtures.

Offline tests cover acknowledgement correlation, disconnect/cancel cleanup, query identity,
framing and protobuf bounds, large integer IDs, embedded plaintext, retained session material,
batch completion, duplicate delivery and transactional rollback. These tests cannot establish
that a real phone safely finishes backup. A complete multi-group transfer remains unverified.
The maintainer chose to publish 0.0.9 with this explicit limitation; passing CI must not be
presented as proof that full phone history synchronization works.
