# Progress audit — zalo-rust
Date: 2026-10-21

## Verdict
The project has successfully reverse-engineered and implemented the core cryptographic foundations—ParamCipher v2, AES-256-CBC zero-IV cipher, and `zsecure` request signing are byte-accurate and verified against live captures. However, the client is entirely non-functional as an end-user application: `cargo check --all-targets` fails to compile due to type mismatches in `src/bin/test_qr_tag.rs`, `zpw_sek` session extraction is completely missing, and no session or device persistence exists. Chat functionality is a non-working scaffolding that cannot pass the server's authentication gate, while TUI and GUI architectures remain unimplemented prototypes.

| Subsystem | Status | % | Evidence |
|---|---|---|---|
| Crypto Primitives | Complete & Verified | 95% | `src/crypto.rs:13-87`, 13 passing unit tests |
| Param Cipher v2 | Complete & Verified | 90% | `src/param_cipher.rs:60-146`, `param_cipher.rs:212-221` |
| Request Signing (`zsecure`) | Working (tested endpoints) | 80% | `src/crypto.rs:75-87`, `src/login.rs:74,179`, `src/qr.rs:120` |
| QR Login Flow | Partially Implemented (stalls at confirm) | 50% | `src/qr.rs:142-202, 243-305` |
| Session Persistence | Non-existent | 0% | `src/session.rs:97, 360-369`, `src/login.rs:243-276` |
| Chat & Messaging | Non-functional Stub | 15% | `src/chat.rs:23-50, 84-101`, missing `zpw_sek` & send APIs |
| TUI Frontend | Primitive ANSI Prototype (no ratatui) | 10% | `src/ui.rs:61-105`, `Cargo.toml:6-18` |
| GUI Frontend | Not Started | 0% | `Cargo.toml`, zero files |

---

## Build status
- `cargo check --lib`: **PASS** (clean, 0 warnings, 0 errors).
- `cargo check --bin zalo-rust`: **PASS** with 1 warning (`src/main.rs:57:20` unused assignment to `resp`).
- `cargo check --all-targets`: **FAIL** with 1 distinct error and 1 warning:
  - `src/bin/test_qr_tag.rs:68:47`: `error[E0308]: mismatched types` — expected `&[u8]`, found `&String` in `s.maybe_decrypt(resp, &cipher.encrypt_key).await`. Cause: `maybe_decrypt` expects byte slice `&[u8]`, but `&cipher.encrypt_key` provides `&String` (which coerces to `&str`, not `&[u8]`). Needs `.as_bytes()`.
  - `src/main.rs:57:20`: `warning: value assigned to resp is never read`.
- `cargo test --lib`: **PASS** (13/13 unit tests passed).
- `cargo test --bin zalo-rust`: **PASS** (19/19 unit tests passed).
- `cargo build --release`:
  - Release artifact `target\release\zalo-rust.exe` exists from prior compilation: **4,190,720 bytes** (~4.00 MB).
  - Re-compilation currently fails with `Access is denied (os error 5)` because a running process (`zalo-rust.exe`, PID 26908) has an active handle on the binary file.
  - Sibling binaries exist in `target\release\`: `compare.exe` (291,840 bytes), `test_qr_tag.exe` (3,932,160 bytes), `probe.exe` (3,585,536 bytes).

---

## What actually works
- **AES-256-CBC & AES-128-CBC encryption/decryption** (`src/crypto.rs:15-53`): Correctly enforces 16-zero-byte IV, PKCS7 padding, and base64 encoding/decoding. Confirmed with test `aes_matches_crypto_js_shape` (`src/crypto.rs:111-118`).
- **MD5 hashing and key sorting** (`src/crypto.rs:63-87`): `sign_key` accurately implements `md5("zsecure" + tag + sorted_values)` as proven in test `signkey_sorts_keys` (`src/crypto.rs:126-131`).
- **ParamCipher v2 core derivation** (`src/param_cipher.rs:72-92`): `zcid` is generated as hex uppercase AES-256 (`src/param_cipher.rs:74`), `zcid_ext` generates random hex (`src/param_cipher.rs:75`), and `encrypt_key` interleaves `n_even[0..8] + a_even[0..12] + a_odd.rev()[0..12]` (`src/param_cipher.rs:82-84`). Proven byte-accurate against real captured server traffic in `decrypts_real_login_response` (`src/param_cipher.rs:212-221`).
- **ParamCipher payload encryption and body construction** (`src/param_cipher.rs:116-136`): Constructs `{client_version, enc_ver, params, type, zcid, zcid_ext}` and signs with `signkey`.
- **Pre-login server info query** (`src/login.rs:65-92`): `get_server_info` signs query parameters with tag `"getserverinfo"`, fetches dynamic domain routing from `wpa.zaloapp.com`, and parses `zpw_chts_new`, `zpw_ctrs_new`, and `zpw_service_map_v3` (`src/session.rs:185-216`).
- **QR ticket acquisition** (`src/qr.rs:142-165`): `request_qr` sends GET `/api/login/reqqr` with `self_g = 1` and `qr_form = 0` signed with tag `"reqqr"`. Correctly parses `token_id`, `qr_cnt`, `chk_wait_scan`, and `chk_wait_cfirm` (`src/qr.rs:53-78`).
- **QR terminal rendering** (`src/qr.rs:171-184`): Uses `qrcode` crate with `Dense1x2` unicode characters to produce compact, scannable terminal QR blocks.
- **QR scan polling loop** (`src/qr.rs:202-240`): Handles HTTP 408 long-poll responses without treating them as transport errors, looping until scan confirmation.

---

## What is broken or wrong
- **[CRITICAL] Missing `zpw_sek` capture and transmission** (`src/login.rs:243-276`, `src/session.rs:60-66`, `src/chat.rs:23-40`):
  `re-notes/02_chat_data.md:180-183` establishes that all authenticated endpoints reject requests with `error_code 600: zpw_sek not found or invalid` if `zpw_sek` is absent. `apply_login_config` only extracts `dkey` and `zpw_enk`. `LoginInfo` does not have a `zpw_sek` field, `Session` does not hold it, and no outgoing request attaches it in headers, cookies, or queries. All post-login data queries fail.
- **[CRITICAL] Compilation failure on target `test_qr_tag`** (`src/bin/test_qr_tag.rs:68:47`):
  `let decrypted = s.maybe_decrypt(resp, &cipher.encrypt_key).await;` passes `&String` to a parameter expecting `&[u8]`. This breaks `cargo check --all-targets` and `cargo test`.
- **[CRITICAL] Total absence of session and credential persistence** (`src/session.rs:97, 360-369`, `src/login.rs:243-276`):
  Every startup calls `random_imei()` (`src/session.rs:360`), generating a new 24-character hex identifier. Cookies are kept only in a non-persisted in-memory reqwest cookie jar. No tokens, keys, UINs, or device IDs are written to disk. The application cannot resume a session and forces re-authentication on every execution.
- **[HIGH] QR confirmation handoff drops session data** (`src/qr.rs:288-302`):
  `confirm(s, &q)` calls `q.chk_wait_cfirm` (`/polling/qr/pc-status`) on `qr-id.zaloapp.com`, but the returned JSON response is completely discarded (`let resp = confirm(s, &q).await?;` followed immediately by `get_login_info(s)`). If `pc-status` returns session cookies or tickets in its response body, they are lost. If the cookies were set only on `qr-id.zaloapp.com`, subsequent requests to `chat.zalo.me` will fail.
- **[HIGH] `src/lib.rs` fails to expose `qr` module** (`src/lib.rs:1-7`):
  `src/lib.rs` exports `chat, endpoints, crypto, login, param_cipher, session, ui`, but omits `pub mod qr;`. The QR subsystem is only accessible if a binary includes `mod qr;` directly.
- **[HIGH] `chat::get_last_msgs` transmits unencrypted parameters** (`src/chat.rs:99`):
  `get_last_msgs` calls `Session::plain_params(&body)`, whereas `re-notes/02_chat_data.md:43` explicitly notes that `params` must be `encodeAES(JSON.stringify(t))` under the session key. Unencrypted parameters on conversation endpoints cause server rejection (`18060 Invalid encryption protocol`).
- **[MEDIUM] Incomplete and duplicated endpoint mappings** (`src/endpoints.rs:10-23`, `src/session.rs:44-58`):
  `session.rs:Domains::default()` hardcodes literal domain strings instead of querying `endpoints::SERVICE_MAP_V3` or using `endpoints::service()`. The function `endpoints::service()` (`src/endpoints.rs:38-43`) is entirely dead code.
- **[MEDIUM] Blocking terminal I/O inside asynchronous runtime** (`src/ui.rs:93`, `src/main.rs:154-167`):
  `ui::draw` uses blocking `io::stdin().lock().read_line(&mut line)`. Because it is called synchronously inside `#[tokio::main]`, the entire worker thread freezes waiting for user input, halting background poll intervals.
- **[MEDIUM] Code duplication in test/diagnostic binaries** (`src/bin/compare.rs:15-77`, `src/bin/test_qr_tag.rs:30-36`):
  `compare.rs` re-implements MD5, AES-256 decryption, and URL decoding from scratch without importing `zalo_rust::crypto`. `test_qr_tag.rs` manually reimplements `ParamCipher::build_body`.
- **[LOW] Unused fields and assignments** (`src/session.rs:75`, `src/main.rs:57:20`):
  `Session.session_key` is defined as `Option<String>` but never written or read. `main.rs:57` assigns `let mut resp = Value::Null;` which is immediately overwritten at line 91 without being read.

---

## What is missing
To reach a minimum viable client that logs in via QR, displays chats, and sends a text message, the following must be implemented in dependency order:

1. **Fix compilation of `test_qr_tag.rs`**: Change line 68 to `cipher.encrypt_key.as_bytes()`.
2. **Expose `pub mod qr;` in `src/lib.rs`**: Ensure library consumers have access to the QR login module.
3. **Session persistence layer (`src/storage.rs` or `src/session.rs`)**:
   - Persist device identification (`imei`, `first_launch_time`, `computer_name`) to `~/.config/zalo-rust/session.json` or local directory.
   - Persist session tokens (`zpw_sek`, `dkey`/UIN, `zpw_enk`, cookies) across launches.
4. **QR Login session extraction**:
   - Inspect and capture the full response envelope of `chk_wait_cfirm` (`/polling/qr/pc-status`).
   - Extract `zpw_sek` from response cookies (`Set-Cookie`) or JSON data fields.
   - Attach `zpw_sek` to the session cookie jar for `.zaloapp.com` and `*.chat.zalo.me`.
5. **Session-resume startup path in `main.rs`**:
   - Check if a saved session exists on disk.
   - Call `get_login_info` with saved credentials to validate and refresh domains. Skip QR prompt if valid.
6. **Encrypted parameter support for conversation endpoints**:
   - Update `chat::get_last_msgs` (`src/chat.rs:84-101`) to encrypt payload using `s.encoded_params(&body)`.
   - Update `chat::get_friends` to ensure correct profile/friend service domain resolution.
7. **Message Sending Endpoint (`chat::send_text_message`)**:
   - Implement REST send endpoint (e.g. `/api/message/sms` or message plane equivalent on `zpw_chts_new`).
   - Define message payload struct (`msgType`, `content`, `toId`, `clientId`, `msgId`).
8. **Decoupled TUI using `ratatui` + `crossterm`**:
   - Remove blocking `stdin().read_line()` from `src/ui.rs`.
   - Implement an async event loop (`crossterm::event::EventStream`) allowing simultaneous background network polling and responsive UI interaction.
   - Add chat conversation view and message input text box.

---

## Duplication and dead code
- **`src/endpoints.rs` vs `src/session.rs`**:
  `src/endpoints.rs:10-23` defines `SERVICE_MAP_V3` array of tuples. `src/session.rs:44-58` ignores it and re-specifies URLs as literal strings in `Domains::default()`. `endpoints::service` (`src/endpoints.rs:38-43`) is never referenced.
- **`src/bin/compare.rs` vs `src/crypto.rs`**:
  `compare.rs:15-23` duplicates `crypto::md5_hex` (`src/crypto.rs:63-72`).
  `compare.rs:57-65` duplicates `crypto::decode_aes` (`src/crypto.rs:36-53`).
- **`src/main.rs` module declarations vs `src/lib.rs`**:
  `src/main.rs:9-16` declares `mod chat; mod endpoints; mod param_cipher; mod crypto; mod login; mod qr; mod session; mod ui;`, compiling every source file directly into the binary rather than importing from `zalo_rust`.
- **Dead functions and unused fields**:
  - `src/chat.rs:104-111`: `get_pinned` is never called.
  - `src/session.rs:75`: `pub session_key: Option<String>` in `Session` is never initialized or used.
  - `src/qr.rs:187-195`: `save_png` is dead code unless terminal QR rendering fails.

---

## Architecture assessment
The current repository structure does NOT satisfy the target architecture of "one shared core library (`zalo_rust`) + TUI frontend (`ratatui`) + GUI frontend (`egui`/`eframe`)":

1. **Tight UI coupling inside the core library**:
   - `src/lib.rs` exports `pub mod ui;`, binding console drawing logic directly into the core crate.
   - `src/qr.rs` embeds ANSI string generation via `render_terminal` and `Dense1x2`. The library should only yield QR data/byte representations.
2. **Missing Workspace structure**:
   - The project is a single crate (`zalo-rust`). Adding `ratatui` and `egui` to `Cargo.toml` would bloat the core library with massive windowing, OpenGL, and TUI dependencies.
   - It should be structured as a Cargo workspace:
     - `crates/zalo_core` (HTTP, ParamCipher, crypto, endpoints, models)
     - `crates/zalo_tui` (ratatui + crossterm binary)
     - `crates/zalo_gui` (egui + eframe binary)
3. **No high-level Client facade**:
   - `src/main.rs` directly manipulates `Session`, calls low-level `login::*` and `qr::*` routines, and runs custom polling logic. The core library needs a unified `ZaloClient` struct providing high-level methods (`login_qr()`, `get_threads()`, `send_message()`).

---

## Next 5 concrete steps
1. **Fix compilation error in `src/bin/test_qr_tag.rs`**:
   Change line 68 from `&cipher.encrypt_key` to `cipher.encrypt_key.as_bytes()`.
2. **Expose `qr` in library root `src/lib.rs`**:
   Add `pub mod qr;` to `src/lib.rs:7`.
3. **Implement session token extraction in `src/qr.rs` and `src/login.rs`**:
   Parse response cookies from `chk_wait_cfirm` to extract `zpw_sek`, store `zpw_sek` in `LoginInfo`, and ensure subsequent requests send it.
4. **Implement disk-based session persistence in `src/session.rs`**:
   Save and load `imei`, `dkey`, `zpw_enk`, and `zpw_sek` to a local JSON file so the client does not require scanning a QR code on every launch.
5. **Encrypt payload in `src/chat.rs:get_last_msgs`**:
   Replace `Session::plain_params` at `src/chat.rs:99` with `s.encoded_params(&body)` to comply with ParamCipher protocol requirements on conversation endpoints.
