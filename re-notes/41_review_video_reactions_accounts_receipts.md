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

## Final verification

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
