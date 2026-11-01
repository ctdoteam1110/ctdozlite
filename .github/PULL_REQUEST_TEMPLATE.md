<!--
Tier guide (see docs/standards/git-flow-and-pr-matrix.md):
- Tier 1 (docs/typo/deps): keep What + Why, delete the rest.
- Tier 2 (feature/bugfix/refactor): fill every section, tick the checks you ran.
- Tier 3 (protocol/schema/crypto/breaking): add ADR link, evidence and rollback detail.
Use concise bullets, not paragraphs. Never paste session.json, cookies, zpw_sek/zpw_enk, IMEI or raw captures.
-->

## What
-

## Why
-

## How
-

## Verification
- [ ] `cargo test --locked`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --all --check`
- [ ] Manual: <GUI/TUI, OS, flow, observed error_code>

## Risk & Rollback
- Risk: two-way door / one-way door
- Blast radius:
- Rollback:

## Context
- Resolves: None
- Relates to:
- ADR / RFC: None
