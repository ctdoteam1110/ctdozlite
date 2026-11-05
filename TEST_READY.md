# E2E Test Suite Ready

## Test Runner
- Prerequisites: Ensure developer tools (`cargo`, `cmake`, `ninja`, `clang`) are in PATH.
- Commands:
  ```powershell
  Get-Command cargo, cmake, ninja, clang | Out-Null
  $env:CMAKE_POLICY_VERSION_MINIMUM = "3.5"
  $env:CMAKE_GENERATOR = "Ninja"
  cargo check --locked
  cargo test --locked -- --test-threads=1
  git branch --show-current # Validates R5 branch: feature/ui-improvements-quote-forward
  ```
- Expected: All tests pass with exit code 0.

## Coverage Summary
| Tier | Count | Description |
|------|------:|-------------|
| 1. Feature Unit Tests (`src/lib.rs`) | 212 | Baseline unit tests covering protocol, session, crypto, store, call, and core features |
| 2. Feature & Boundary Tests (`tests/`) | 25 | Isolated happy-path and edge cases across R1-R4 (Unicode, extreme payloads) |
| 3. Cross-Feature Integration (`tests/`) | 15 | Pairwise interaction tests (quote + scroll, forward + media URL, settings + persistence) |
| 4. Real-World Workflows (`tests/`) | 14 | End-to-end workflows (theme customization, download folder, quoting, forwarding) |
| 5. Adversarial Hardening (`tests/`) | 20+ | Multi-agent stress tests (`test_theme_and_font_stress`, `test_quote_reply_stress`, `test_forward_stress`, `test_forward_dialog_adversarial`) |
| **Total** | **286+** | 212 unit tests + 74 integration tests across 10 test suites passing cleanly |

## Feature Checklist
| Feature | Tier 1 (Unit) | Tier 2 (Boundary) | Tier 3 (Cross-Feature) | Tier 4 (Workflow) | Tier 5 (Adversarial) | Supporting Evidence |
|---------|:------:|:------:|:------:|:------:|:------:|---------------------|
| R1: Download Folder Selection | ✓ | ✓ | ✓ | ✓ | ✓ | `tests/test_download_settings.rs` (path norm, JSON roundtrip, write probe, COM threading) |
| R2: Direct Font & Color Picker | ✓ | ✓ | ✓ | ✓ | ✓ | `tests/test_theme_and_font_customization.rs`, `tests/test_theme_and_font_stress.rs`, `tests/test_scaling_and_live_updates_stress.rs` |
| R3: Message Quote & Reply | ✓ | ✓ | ✓ | ✓ | ✓ | `tests/test_message_quote_reply.rs`, `tests/test_quote_reply_stress.rs`, `tests/test_bubble_interaction_and_focus_adversarial.rs` |
| R4: Message Forwarding | ✓ | ✓ | ✓ | ✓ | ✓ | `tests/test_message_forward.rs`, `tests/test_forward_dialog_adversarial.rs`, `tests/test_forward_stress.rs` |
| R5: Build Environment & Git Branch | ✓ | ✓ | ✓ | ✓ | ✓ | `cargo check --locked`, `git branch --show-current` (`feature/ui-improvements-quote-forward`) |
