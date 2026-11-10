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
50.0.1 manifest's `rust-version`. Current verification uses **Rust 1.99**:
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
