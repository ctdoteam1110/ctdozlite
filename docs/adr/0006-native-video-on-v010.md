# ADR-0006: Reconcile native video with upstream v0.1.0

- Status: accepted for PR #129 integration
- Date: 2026-11-10
- Deciders: user instruction to resolve PR #129 against current main

## Context

The native-video branch was tested on v0.0.9. Upstream v0.1.0 added native
Wasmtime Sync v2, persisted read points and simplified reaction-hover UI.
Replaying the branch conflicts with those changes.

## Decision

Retain upstream's native Sync v2 and UI behavior alongside the bundled video
codec. The v0.0.9 unsupported-Sync-v2 fallback in ADR-0004 is superseded; account
isolation and other failure policies remain. Sync imports use account-owned
stores and caches. Legacy-history migration preserves upstream read points.

Keep Wasmtime 50.0.1 and its existing lockfile selections when adding the pinned
native-video dependencies. Its declared minimum is Rust 1.97, so the combined
manifest documents 1.97 rather than the old video-only 1.89 baseline. Edition
2021 remains unchanged. This compiler constraint comes from upstream Sync v2.

## Consequences

- The PR can integrate without dropping newer upstream features.
- Earlier v0.0.9 test results remain historical; the rebased tree needs fresh
  compilation and regression checks.
- Video hardware, peer interoperability and cross-platform release gates from
  ADR-0005 remain pending. This repair does not claim release readiness.

## Verification

- Rust 1.99: locked compilation of all targets/tests succeeded, and the full
  suite reported 344 passed, zero failed and one ignored live-network test.
- The v0.1.0 binary's native codec test decoded ten frames with PATH restricted
  to Windows system tools. No FFmpeg installation was used.
- Upstream's framing and missing-module tests pass. Its key generation and
  agreement tests self-skip because the Zalo WASM module is absent here; real
  phone-sync interoperability is not verified by this repair.
