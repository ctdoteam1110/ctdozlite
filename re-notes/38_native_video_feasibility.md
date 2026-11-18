# Native video feasibility gate

Date: 2026-11-08 (Asia/Bangkok).
Status: **FAILED — production integration has not started.**

Historical snapshot of the initial Rust 1.88 gate. The subsequent user-directed
local Rust 1.89 implementation and remaining release blockers are recorded in
[note 40](40_native_video_integration.md); the results below are retained as
evidence of the original failure.

## Release and isolation

- Upstream annotated tag: `v0.0.9`, object `401ddea75ee04dc74866f69994064452cde42f01`.
- Peeled release commit: `d1f954119aa94eebc309dd525923c3b127de22ad`.
- Isolated branch: `codex/native-video-v009`.
- The original checkout, its feature commits and uncommitted changes were preserved.
- The root manifest, root lockfile, protocol, GUI and production media pipeline were not modified.

## Findings and commands

| Requirement | Result | Evidence |
|---|---|---|
| Fetch exact release and isolate work | PASS | Tag peeled to the commit above; managed worktree created on the requested branch. |
| Rust 1.88 baseline release build | FAIL | `cargo +1.88.0 build --locked` reached `audiopus_sys v0.2.2` and failed because the `cmake` executable was not found. This is a pre-existing build prerequisite, not a video regression. No baseline success is claimed. |
| Pinned video dependencies on Rust 1.88 | FAIL | The isolated locked build rejects `wide 1.7.1` and `safe_arch 1.2.0`, both requiring Rust 1.89. Even `wide 1.1.1`, OpenH264's minimum allowed version, declares Rust 1.89. |
| Cancellable camera reads and release within two seconds | UNVERIFIED | Source inspection shows synchronous reads and no demonstrated independent cancellation handle. Hardware cancellation tests have not run. |
| Sanitized real Zalo stream decoding | BLOCKED | No committed `.h264`, `.264`, `.bin`, or `.rtp` peer-video fixtures were found under `tests/` and `re-notes/`. Existing protocol notes are not decode fixtures. |
| Linux and macOS builds and device tests | NOT RUN | This execution environment is Windows; only the Windows MSVC Rust target is installed. Cross-compilation alone would not prove permission/device behavior. |
| Keyframe, color, latency and repeated-call checks | NOT RUN | Dependency/compiler gate failed before codec compilation or camera access. |

### Compiler incompatibility

The published `openh264 0.9.9` manifest declares Rust 1.85 but depends on `wide = "1.1.1"` (a caret requirement). The lower-bound `wide 1.1.1` manifest declares Rust 1.89. Thus the wrapper's declared minimum does not establish compatibility of its complete dependency graph with Rust 1.88.

Dependency resolution was explicitly requested with Rust-version fallback:

```powershell
cargo +1.88.0 fetch --manifest-path tools/native-video-feasibility/Cargo.toml --config 'resolver.incompatible-rust-versions="fallback"'
cargo +1.88.0 build --locked --offline --manifest-path tools/native-video-feasibility/Cargo.toml
cargo +1.88.0 info wide@1.1.1
```

The fallback resolver still selected incompatible versions and printed their requirements. The final build command is expected to fail with the recorded lockfile. No dependencies were downgraded, patched, replaced, or integrated into the application.

Published sources:

- https://docs.rs/crate/openh264/0.9.9/source/Cargo.toml (`dependencies.wide`).
- https://docs.rs/crate/wide/1.1.1/source/Cargo.toml (`rust-version = "1.89"`).
- https://docs.rs/crate/nokhwa/0.10.11/source/Cargo.toml.orig (native capture and decoding features).

### Camera cancellation concern

Inspected downloaded sources pinned by the probe lockfile:

- `nokhwa-bindings-windows 0.4.6`: `raw_bytes(&mut self)` loops over synchronous Media Foundation `ReadSample` calls; `stop_stream(&mut self)` only changes local open-state fields. `Drop` performs a reader flush, but this does not demonstrate the ability to cancel a read while that same object is mutably borrowed on its capture thread.
- `nokhwa-bindings-linux 0.1.4`: frame acquisition calls `stream_handle.next()`; stopping drops the handle through `&mut self`. This requires a platform timeout/cancellation proof before adopting the proposed worker lifecycle.

These are source observations, not proof that every device hangs. An atomic stop flag alone does not prove the two-second shutdown requirement. The adapter must interrupt an in-flight read, or that backend cannot pass the gate. Do not use unsafe concurrent mutable access or leaked threads as a workaround.

## Proposed design revision — not applied

1. Change the proposed compiler baseline to Rust **1.89**, retaining application edition 2021 and the pinned top-level libraries. Re-run the full locked graph on that compiler; 1.89 is a candidate, not a verified application minimum.
2. Make CMake available to the existing Windows build before running the baseline. Audit native tool requirements for the camera/codec builds and each release target. NASM, C/C++ compilers and CMake are build-time tools, not runtime requirements.
3. Demonstrate camera cancellation on each native backend before integration. If Nokhwa's public API cannot meet the bound, propose a narrowly scoped cancellable adapter/upstream patch for review; do not silently change the architecture.
4. Supply authorized, sanitized Zalo video fixtures with known expected dimensions/frame output, and platform/device evidence for direct, relay and group calls. Keep personal video, credentials and raw captures outside Git.
5. Only after the gate passes, implement the approved ordered queues, fresh-IDR recovery, SPS allocation bounds, worker commands, generation-tagged frame delivery and packaging removal of FFmpeg.

The feasibility failure does not authorize raising the compiler baseline, changing libraries, or removing the working FFmpeg implementation. The approved plan explicitly requires stopping before full integration when a mandatory check fails.

## Distribution decision retained

The candidate uses source-linked OpenH264, with no runtime codec download. Include all applicable notices and review the source-build distribution terms before a release. Cisco's binary-module royalty arrangement must not be represented as covering this source build:
https://www.openh264.org/faq.html

## Remaining acceptance criteria

The original requirements remain pending: native camera permissions/selection, capture conversion, fresh keyframes, real-peer decode, malformed/lost/oversized packet recovery, group streams, stale-generation rejection, bounded delivery, offline packaging, latency measurements, <=2 s shutdown, and 20 repeated calls without resource growth on all three platforms. No commit, push or publication was performed.
