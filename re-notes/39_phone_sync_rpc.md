# Phone sync request acceptance — 2026-11-07

Latest correction: the scope builder interpretation below came from the separate PC-to-phone
backup path. Phone SyncFull needs `oneone/<globalId>` / `group/<globalId>` and reverse identity
translation. See [note 40](40_phone_sync_identity_namespace.md) for the current evidence and
repair. The earlier nonempty bare-ID scopes are not a valid final implementation.

Author: Codex. No live identifiers, keys, session values or raw captures are included.

## Reproduction

The signed-in community app restored cloud/group history, but a manual phone-sync attempt
produced no notification on the user's phone. The UI eventually reported that phone history
had not been loaded. Both request results had been discarded; the v2 function manufactured
`error_code: 0` even when its outgoing WebSocket channel was unavailable.

## Owning-source evidence

Installed official Zalo PC version: 26.11.10. Static inspection of its public app.asar only.

- `pc-dist/compact-app-pc.cfeba43a9a8e4e40dda3.js`, offset 15128423:
  `requestSyncMessage` constructs params with string `reqId` and JSON-stringified `data`,
  passes them through the standard params encoder, then GETs
  `/api/transfer-sync-v2/request-sync` on the file service (command 12888, timeout 10000 ms).
- The same bundle, offset 17594583, also exposes WebSocket request command 590, and
  command 592 for mobile wake-up. The official client awaits an actual response.
- Offset 15127985: legacy `pullMobileMsg` GETs `/api/message/pull_mobile_msg` with
  the existing RSA public-key fields. No POST fallback was present in this method.
- `pc-dist/lazy/default-login-main-startup-shared-worker-znotification.4bb5e1e2c2e9bab4f7b3.js`,
  offset 7898263: SyncFull=0, Mobile=0, WaitingConfirm=3, Confirmed=4, BypassConfirmed=5,
  UserReject=6 and UserCancel=7. Existing payload/enumeration values are preserved.

## Runtime transport evidence

A manual, isolated probe used the standard Session client, the existing public v2 keys and
only emitted numeric/status classifications. The HTTP v2 route returned HTTP 404 on the
current service; no cookie, signed URL or response body was recorded. The legacy route was
accepted while no phone notification arrived. Therefore HTTP acceptance of the legacy route
must not be labelled as v2 acceptance.

The active PC path is `requestAsyncV2` in the compact bundle at offset 17592182:
it adds a request ID to the nested data, sends the frame and waits for the matching result.
Its `_send` at offset 1365940 serializes that nested data as the JSON body after the four-byte
header. The server result is correlated by `data.reqId`, and nonzero error codes are rejected.

## Repair

Preserve the existing sync payload and genuine zproto keys. Request v2 over authenticated
WebSocket command 590, correlate `data.reqId`, and await the actual acknowledgement rather
than treating a queued frame as success. Bound pending requests and remove them on timeout,
caller cancellation and connection loss. Older connections cannot clear newer registrations.

Use the legacy RSA GET as an availability fallback only; do not hide a definitive rejection
or malformed server acknowledgement by immediately starting another protocol. Keep secrets
and signed URLs out of errors. Do not wait for phone confirmation when both requests failed,
or label cloud-only data as a completed phone sync. A metadata-only local status snapshot
records which route was accepted and whether the WebSocket was authenticated.

## Verification

Fixture-only tests cover rejected/malformed acknowledgements, secret-free errors, the exact
WebSocket body shape, request/result correlation, disconnects, timeouts and cancellation.
A local helper check generated 33-byte public / 32-byte private key pairs in 156 ms using the
installed PC wasm, in a fresh private temporary directory; no generated key was logged.
Live notification and backup-import verification remain in progress.

## Phone crash and receive-pipeline follow-up

After the acknowledged WebSocket repair, the user received a phone request and confirmed it.
They reported that Zalo on the phone exited after pressing its immediate-backup button. The
custom client was stopped, and no further live phone request was sent during the offline repair.
The notification proves delivery, not correct payload processing or successful data import.

Static owning-source comparison found an invalid message query: `partition: ""`. The installed
PC `shared-worker.cfeba43a9a8e4e40dda3.js`, offset 255040 (`wa=1e5` and `WorkerScopeBuilder`),
uses concrete conversation `userId` partitions for message scopes, range 0 to MAX_SAFE_INTEGER,
limit 100000. Only a conversation-list query uses an empty partition. Offset 285062 builds the
v2 request from those scopes; `transfer_batch_size=2000` is separately configured in startup.
The repair deduplicates valid numeric peer/group partitions and rejects an empty scope list.
This is an observed malformed request; attribution of the phone crash remains unconfirmed.

The same worker at offset 283138 decrypts MetadataEmbedded (batchType=2) into both the session
record and the first plaintext chunk. Message (batchType=1) uses the retained session record.
The previous helper discarded the embedded plaintext and deleted its session after a message.
It now persists both outputs privately and leaves lifecycle cleanup to the Rust coordinator.
Private keys and session-record bytes are never included in helper stdout.

Offsets 305670 and 378862 show that each downloaded file is a stream of u32-LE-length-prefixed
records, each independently decrypted, decompressed and deserialized. The repaired coordinator
validates every frame boundary before importing, processes every record, serializes helper-file
access, and retains keys until every batch index through `isLast` is imported. Duplicate controls
are idempotent; stale session IDs and phone errors cannot become a success. Record imports use
an atomic SQLite transaction, preserve existing authoritative rows and never erase history.

The PC release path at offset 281251 calls disposal with syncId, toDevice and reason. `SCpg` in
`default-login-main-startup-shared-worker-znotification.4bb5e1e2c2e9bab4f7b3.js`, offset 7898064,
defines UserCancel=0, SyncSuccess=1 and Error=2; compact offset 17594816 uses acknowledged
command 591 with nested `{data: payload}`. The repair sends this lifecycle request on completion,
cancel or failure, rejects late controls and clears the local per-transfer files.

The official installed PC app's existing-data sync displayed success during comparison. This
could have been a no-op and is not evidence of a full phone backup transfer. The custom repaired
pipeline still requires a real-device import test. A UI option restricts the phone request to the
currently open conversation for that first test; no automatic phone request is started by install.

Offline coverage includes scope identity/range, control metadata, framed-file bounds, out-of-order
and duplicate completion, atomic/idempotent store writes, secret-free errors and helper output
contracts. `tests/phone_sync_helper.py` runs a mocked zproto module in a temporary directory;
it does not use the installed phone account, private sessions, real databases or network.

A further schema check at shared-worker offset 13909 shows `Message.globalId` (field 6) is
`readInt64String`, i.e. a protobuf varint presented as a decimal string by the JS library. The
previous Rust parser expected wire type 2 and silently lost that stable message ID. It now reads
the varint without a floating-point conversion, tests an ID above 2^53, and rejects malformed
nested fields/lengths instead of silently discarding messages. This prevents client-ID fallbacks
from duplicating a message already stored under its authoritative global ID.


## Latest real-device result: still failing

The limited current-conversation test also reproduced the phone exit after confirmation.
The desktop received confirmation but no batch, then timed out and removed its transfer files.
The user reports iPhone 11, iOS 27 and Zalo 26.11.01. No further phone request was made.
A separate isolated real-wasm check verified that both generated key pairs match independently
derived X25519 public keys (33-byte prefixed public, 32-byte private), are distinct and have
entropy. This does not prove phone compatibility. The actual crash cause remains unresolved;
a phone crash report is needed before another live trial. Offline tests are not device acceptance.
