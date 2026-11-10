# Review corrections: camera permission, reactions, accounts and receipts

Date: 2026-11-10 (Asia/Bangkok).

## Corrections

- Camera-off call acceptance supplies the initial camera permission separately
  from video transport capability. `MediaHandle` passes that permission into the
  shared `VideoSession` before either worker is started. Incoming video and the
  camera-off transport behavior remain available. Late legs inherit the shared
  permission rather than reopening capture.
- A loaded reaction snapshot is baselined before a newly received live batch is
  recorded. Events queued before message loading retain the existing snapshot
  reconciliation behavior. Exact applied-event identities are saved alongside
  the snapshot so a restarted inbox can distinguish replay from a new event.
- SQLite saves and restores `reaction_versions` in the existing extensible
  `raw_json` message metadata, alongside `reaction_summaries`. Metadata upserts
  union stored identities even when a later history refresh omits its ledger.
  Hydration also restores identities into already-loaded rows. This introduces
  no schema or wire-format change.
- Settings renders cached account profiles. Discovery and deletion go through
  `WorkerCmd`, with filesystem work in `spawn_blocking`. Requests are coalesced
  across repaints; completions are matched by request ID and account generation.
  Login, logout and switching invalidate the snapshot. Deletion is serialized
  with account switching, and failures terminate the loading state.
- Incoming messages no longer imply that earlier outgoing messages were read.
  Both history merging and preview merging preserve Pending, Failed and
  Delivered states until authoritative metadata or a receipt advances them.
  The existing `SeenEventsReceived` receipt path remains in place.

## Regression evidence

Before correction, focused tests reproduced five reactions remaining at five
after a new event and Pending outgoing messages becoming Read after an incoming
message in both merge paths. Those tests pass after correction.

A second test reproduced an applied reaction becoming count two after SQLite
hydration and replay. It now verifies the actual in-memory SQLite save/update/
hydrate path, including a metadata-bearing duplicate write without versions,
then accepts a genuinely new event. JSON restart coverage remains separate.

Account-cache tests cover repeated repaints, stale responses after invalidation,
and terminal failure followed by an explicit retry. The shared-camera permission
test covers initial camera-off state, toggling, and a joining video leg.

Verification logs are local artifacts under `target/review-four-fixes-*` and
`target/review-fix-*`. These automated checks do not replace the outstanding
hardware and real-peer acceptance checks in [note 40](40_native_video_integration.md).
No commit, push or publication is authorized by these corrections.

## Historical v0.0.9 verification, before the upstream v0.1.0 rebase

These results apply to the older v0.0.9 lockfile, before upstream added
Wasmtime 50.0.1. They do not describe the compiler requirement or test count of
the current rebased branch; see the follow-up section below and ADR-0006.

- Rust 1.89 locked full suite: **338 passed, 0 failed, 1 ignored**.
- Locked Windows x64 release build completed. Changed source files pass rustfmt
  checks and `git diff --check` passes.
- The packaged executable, with PATH restricted to Windows system tools,
  encoded and decoded all 10 codec-test frames. Native camera discovery returned
  valid JSON with one device; version and embedded-license commands succeeded.
  Discovery did not open a camera stream.
- Strict Clippy remains unsuccessful because of existing repository-wide lint
  findings. Existing dependency-audit and hardware/peer blockers from note 40
  remain unresolved; no clean release-readiness claim is made.
- Local test binary: `dist/test-build/ctdozlite-native-video-win64.exe`.
  SHA-256: `6fc03b63e4f840a63d65aef61344df511429d7c03974e61fa112f371e55ace0f`.

## Follow-up review on upstream v0.1.0 (2026-11-10)

The current manifest requires Rust **1.97**, matching the locked Wasmtime
50.0.1 manifest's `rust-version`. Initial follow-up verification used **Rust 1.99**:
`cargo +stable test --locked` reports **373 passed, zero failed, one ignored**.
The earlier Rust 1.89 results above remain historical results for v0.0.9 only.

- Account-transition hangup carries the departing session snapshot and scoped
  GUI sender before logout or account-generation advance. The signal payloads
  and endpoints are unchanged. An isolated command-queue regression verifies
  that clearing the active session cannot clear the queued hangup credentials;
  GUI generation tests cover rejection of departing-account events.
- GUI commands carry the identity visible when enqueued, rather than inheriting
  the account active when dispatched. Cross-account commands are rejected;
  remote mutations and user call commands retain their accepted session
  snapshot. Login callbacks are generation-checked, and internal sync follow-ups
  retain the account that produced them. Queue and delayed-task regressions
  cover the account-switch window without contacting Zalo.
- Restarting QR/password authentication invalidates the previous account's
  completions, including after runtime session expiration. Explicit logout
  retains the departing owner so saved credentials are removed. The call engine
  never saves its private profile-resolution snapshot to `session.json`.
- Session snapshots use a nonblocking lock attempt in the dispatcher. Locked
  login work defers ordinary commands in FIFO order; authorization, updates and
  account transitions bypass that backlog. Tests hold the login mutex while
  checking app-wide dispatch and cancellation priority.
- Completed dials recheck their account generation, cancellation generation
  and empty active-call slot under the call-state lock. Rejected dials release
  media and relay keepalives before sending the existing `405` / `12437` cancel
  payloads with the original session. Tests cover departure during dialing,
  cancellation without an account change, occupied call state and payloads.
- Late-loaded reaction snapshots reconcile queued identities against their
  persisted ledger before adding new events. The synthetic five-replay plus
  one-new-event regression preserves a snapshot of ten as eleven. Snapshots
  without identities retain the previous conservative counting policy.
- Schedule reads and send-now/cancel updates run on blocking workers and return
  scoped result events. Cancellation never uses the automatic-delivery toast.
- TUI quit requests during a confirmed reset are remembered and drain that
  reset before exiting; aborted sticker fetches release their loading flag.

The earlier release-binary results above remain historical. Physical devices,
peer calls and cross-platform release verification remain pending.

## Lifecycle and per-member call fallback review (2026-11-10)

Local-history wipes invalidate an independent work generation, cancel and drain
account tasks, and wait for tracked blocking persistence/import/cache writes.
Queued pre-wipe events and persistence commands cannot repopulate the cleared
view. Calls retain their account generation and survive a history wipe. Failed
switches restore the previous session and send a terminal event that rebinds the
visible command scope; failed wipes also rebind that scope. Regressions exercise
the queued-event boundary, blocking-write barrier, credential rollback and call
frame lifetime without devices or network requests.

Source evidence in `src/call.rs`: when native group creation (`12433`) fails,
the existing per-member fallback dials each peer with `401` / `416`, retains
`is_group` for the conversation, and sets `group_host_call` to an empty string.
That state must use the existing direct-leg protocol rather than native-group
builders. A common predicate now requires an actual group host for native-group
cancellation/hangup, media state, relay setup, group SRTP selection and group
control handling. Adding members through the native-group endpoint is rejected
clearly for a per-member fallback call.

The audio/video cancellation regression failed before the fix: it produced
`12437` with an empty `hostCall` rather than `405` with the primary peer ID and
direct `callType` (audio `1`, video `3`). The same predicate selects the existing
EndCall paths (`405` while ringing, `409` / `406` once connected). Native groups
retain `12437` / `12438` and their original fields. Cancellation request formats,
signing, encryption algorithms and endpoints are unchanged. This is source-level and
synthetic regression evidence; live peer fallback interoperability is pending.

### H.264-only capability advertisement

The native decoder uses OpenH264 and cannot decode HEVC. Unlike the previous
FFmpeg-availability-dependent value, `extendData.supportHevcDecode` is now
always `0` in caller invites (`401` / `416`) and callee answers (`402`). This is
an intentional outbound capability-value change: advertising HEVC would allow
a peer to choose a codec that the application cannot decode. Both builders
continue to advertise the existing H.264 video codec with payload `97`.
Caller and callee regression assertions cover the capability field; the caller
assertion also checks the H.264 codec entry. This checks the local advertisement,
not actual peer negotiation.

Manual verification remains pending: on a test account with FFmpeg absent,
place and accept video calls with an actual Zalo peer, record the sanitized
invite/answer capability and negotiated codec, and confirm received H.264
SPS/PPS/IDR decoding on direct, relay and group paths. Do not record session
keys, cookies or personal identifiers. No peer capture or completed manual
negotiation check is available in this environment, so this remains a release
verification gate rather than a claimed interoperability result.

### Per-stream recovery and current-schema opens

Primary video PLI targets and receiver-report identities now use the remembered
primary video SSRC, falling back to the last peer SSRC only before discovery.
Interleaved extra streams do not update the primary sequence-loss tracker;
their existing per-SSRC ingress and decoder queues request their own recovery.
Synthetic regressions cover a last-packet sender different from the primary,
deduplicated extra-stream recovery, initial SSRC fallback, and an interleaved
sequence that must not create a false primary loss. RTCP packet layouts and the
500 ms PLI interval are unchanged.

Current SQLite schema version 2 opens now read `user_version` without taking
the writer lock. Older schemas still migrate under `BEGIN IMMEDIATE` with a
second version check, preserving concurrent-first-open safety. The isolated
WAL regression failed before the fix while another connection held a write
transaction; migration/import concurrency coverage is retained.

Device discovery releases its enumeration lock before publishing completion.
The deterministic regression reproduces a refresh requested at that publication
boundary and verifies it can acquire enumeration, avoiding a permanently pending
refresh flag. It uses private test locks and makes no driver calls.

Authentication restarts invalidate old work and rebind commands to the empty
authenticated identity without discarding locally viewable history. Explicit
logout/add-account events still clear that view. Login success compares the
cached history's UIN with the authenticated UIN, rather than relying on the GUI's
cached-history display mode; a different owner clears old threads, reactions,
draft/reply state and avatar before publishing the new identity. Failure-scope
rebinds preserve the authenticated identity instead of taking it from a cached
history view. Stale background polling, cache and read-refresh commands are
quiet, while stale user mutations retain an actionable cancellation notice.
Explicit logout/add-account completion queues the next QR request only after
rebinding the anonymous scope; a request queued before logout would be stale.
Account switches, new-owner logins and explicit logout discard pending reply/
forward messages and recipient filters, so an old account's quote cannot be
sent into a new account's conversation. History wipes discard those history
references too, while retaining an unsent plain-text draft for the same account.

TUI logout keeps its in-memory account identity until credential/cache cleanup
succeeds. A disk error leaves the identity available for another confirmation;
the terminal reset event distinguishes logout from history-only wipe. Closing
SQLite is part of the blocking job, and a missing account gets an explicit
status rather than a silent return. An isolated profile regression makes a
saved-session path a directory to force deletion failure, verifies credentials
and retry identity survive, removes that obstruction, and verifies retry cleanup.

### Final local verification for these follow-ups

- Windows x64, Rust 1.99: `cargo +stable test --locked` passed 385 tests with
  zero failures and one ignored live-network test. The two upstream key tests
  still self-skip without the Zalo WASM module; they do not verify phone sync.
- The final plain CodeRabbit review exited successfully with no new findings.
  Its retained historical reply/forward finding was checked against the current
  owner-transition handlers and is fixed; review history was not cleared.
- The rebuilt executable's bundled codec smoke check encoded and decoded ten
  320×240 frames with PATH restricted to Windows system tools. Embedded Nokhwa
  version and attribution checks pass. This is not a camera or latency result.
- Focused Rust formatting and patch whitespace checks pass. Strict Clippy still
  stops at the unchanged build-script `manual_is_multiple_of` diagnostic on
  Rust 1.99; an additional diagnostic run found no errors on changed lines.
  Repository-wide formatting and dependency-audit findings remain separate
  existing gates. No code suppression or unrelated formatting sweep was added.
- Physical cameras, peer negotiation/calls, repeated-call resources, latency,
  Linux, both macOS architectures and distribution checks remain pending.
