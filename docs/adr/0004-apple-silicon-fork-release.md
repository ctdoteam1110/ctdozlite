# ADR 0004: Apple Silicon fork distribution and update channel

- **Status:** accepted for 0.0.9 distribution (2026-11-09)
- **Date:** 2026-11-06

## Context

The user requests free public distribution of the complete current macOS build, including its existing feature changes. The upstream repository is ctdoteam1110/ctdozlite; the user's public fork is lebad280324/ctdozlite. No fork release exists yet, so its public latest-release API returns 404. Installed builds report 0.0.8. Source changes are isolated from both dirty development checkouts.

## Decision

Distribute version 0.0.9 for aarch64-apple-darwin only. Preserve upstream attribution and MIT OR Apache-2.0 licenses. Build and ad-hoc sign artifacts in GitHub Actions from the release commit, with the SHA256SUMS.txt and latest.json contract of ADR 0002. Ad-hoc signing does not constitute Apple Developer ID/notarization. The fork update feed must remain independent from upstream. CI on the release branch precedes tagging that exact clean commit; this is the fork-specific release lane instead of merging unrelated source changes into upstream main.

The macOS installer uses private download and sibling staging directories, waits for process exit, verifies bundle version, arm64 architecture and codesign validity, then retains its old bundle until launch succeeds. Failure preserves/restores the old bundle; rollback failure preserves recovery files. It does not claim post-launch health verification.

## Evidence and verification

- Public repository, API status and existing source checked 2026-11-06.
- Protocol features retain their existing evidence under re-notes/36* through 38*; no protocol encoding is changed by the release work.
- Rust regression suite, disposable macOS transaction tests and relocated native bundle contracts must pass.
- cargo audit and explicit human release audit are required before publication by project standards. Outstanding advisories are recorded in the local release review report; no blanket audit exception is approved by this ADR.
- The maintainer authorized public distribution of the current build with phone-history synchronization explicitly documented as experimental and incomplete. Local checks passed 341 Rust tests (2 ignored), Clippy, Rust 1.95, native bundle/update contracts and the helper fixtures; the current RustSec scan found zero vulnerabilities. Tagged artifacts must still pass GitHub Actions before publication.

## Consequences

- Fork users keep the customized interface when updating.
- Intel/other-platform installers are intentionally absent.
- Apple may prompt for trust because no Developer ID signing identity is available.
- Live Zalo account workflows remain manual verification; private user screenshots are not release assets.

## Rollback

Remove a failed release from the latest channel and publish a higher patch version with corrected source. Installed apps do not downgrade automatically. Restore bundles from the retained transaction directory if rollback itself failed.
