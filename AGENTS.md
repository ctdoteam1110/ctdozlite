# AGENTS.md

Operating guide for AI agents and humans working in this repository.

## Project

`ctdozlite` (`zalo-rust`) is a native Zalo desktop client written in Rust: `egui` GUI, `ratatui` TUI, a Tokio worker for network I/O, and a local SQLite message store. No Node.js, no Electron, no Chromium.

- Repository: <https://github.com/ctdoteam1110/ctdozlite>
- Rust 1.75+ / edition 2021
- Product docs are Vietnamese (`docs/`, `README.md`); engineering standards are English (`docs/standards/`)

## Commands

| Task | Command |
| :--- | :--- |
| Build (debug) | `cargo build --locked` |
| Build (release binary) | `cargo build --release --locked --bin zalo-rust` |
| Test | `cargo test --locked` |
| Format | `cargo fmt --all` |
| Lint | `cargo clippy --all-targets -- -D warnings` |
| Run GUI | `cargo run -- --gui` |
| Run TUI | `cargo run -- --tui --qr` |

`CMAKE_POLICY_VERSION_MINIMUM=3.5` is required when building against CMake 4.x (see `docs/KHAC_PHUC_SU_CO.md`).

## Architecture Map

| Path | Responsibility |
| :--- | :--- |
| `src/main.rs` | CLI entry; selects GUI or TUI mode |
| `src/lib.rs` | Library surface shared by binaries and tests |
| `src/gui.rs`, `src/gui/` | egui app shell, worker bridge, messages, modals, theme, stickers, workspace |
| `src/gui/worker.rs` | Tokio runtime; `WorkerCmd` / `WorkerEvent` boundary between UI and network |
| `src/tui.rs`, `src/ui.rs` | ratatui terminal client |
| `src/chat.rs` | Message and thread operations against the Zalo chat API |
| `src/session.rs` | `session.json`, cookies, service map, request signing and params |
| `src/login.rs`, `src/qr.rs` | QR and password login flows |
| `src/socket.rs` | WebSocket transport, frame codec, push/pull command IDs |
| `src/param_cipher.rs` | `ParamCipher v2` request encoding |
| `src/crypto.rs` | AES-CBC/GCM helpers, MD5 `signkey` |
| `src/media.rs` | Attachments, images, stickers, voice payloads |
| `src/call.rs`, `src/audio.rs` | VoIP signalling and audio capture/playback |
| `src/store.rs` | SQLite persistence (`messages`, `threads`) |
| `src/endpoints.rs` | Single source of truth for service hosts |
| `src/logger.rs`, `src/voip_log.rs` | Diagnostic logging and call tracing |

## Non-Negotiables

- **Never commit secrets.** `session.json`, `*.db`, cookies, `zpw_sek`, `zpw_enk`, IMEI and capture dumps stay out of Git (`.gitignore`, `SESSION_SECURITY.md`).
- **No blocking work on the UI thread.** Network, disk and crypto go through `WorkerCmd`; the UI only renders and sends commands.
- **All hosts live in `src/endpoints.rs`.** Never hardcode a Zalo host elsewhere.
- **Every outbound request goes through the standard path:** params → `ParamCipher v2` → `signkey` → session cookie attachment.
- **Comments in English, user-facing strings in Vietnamese.** See the comment taxonomy below.
- **Do not silently change protocol behaviour.** Any change to params, signing, frame IDs or cipher flow requires evidence (capture, note in `re-notes/`) and a test or documented manual verification.

## Engineering Standards

MUST read the corresponding standard file under `docs/standards/` before modifying related code or tests:

- **Issue tracking & tickets** → `docs/standards/issue-tracker.md`
- **Domain glossary & ADRs** → `docs/standards/domain-docs.md`
- **Comments & docstrings** → `docs/standards/code-comment-taxonomy.md`
- **API calls, IPC, and error handling** → `docs/standards/api-design-and-error-handling.md`
- **SQLite schema and migrations** → `docs/standards/database-and-migrations.md`
- **Threads, channels, locks, and races** → `docs/standards/concurrency-and-locking.md`
- **Tests, fixtures, and benchmarks** → `docs/standards/testing-and-fixtures.md`
- **Secrets, crypto, and session handling** → `docs/standards/security-and-cryptography.md`
- **Branches, commits, and PRs** → `docs/standards/git-flow-and-pr-matrix.md`
