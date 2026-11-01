# Glossary

Ubiquitous language for `ctdozlite`. Use these terms verbatim in issues, ADRs, tests and code identifiers; do not drift to synonyms.

> Purpose: keep protocol vocabulary, UI vocabulary and storage vocabulary aligned. Update via `/domain-modeling` when a term is resolved or renamed.

## Session & Authentication

| Term | Definition | Avoid |
| :--- | :--- | :--- |
| **Session** | Authenticated Zalo identity persisted in `session.json`: cookies, `zpw_sek`, `zpw_enk`, IMEI, user info. | "login state", "auth blob" |
| **`zpw_sek`** | Server-issued session cookie proving an authenticated session. Attached to every `*.zaloapp.com` / `*.chat.zalo.me` request. | "session key" |
| **`zpw_enk`** | Base64-encoded encryption key used to derive the AES key for `params`. | "encryption key" (ambiguous) |
| **QR login** | Login by scanning a QR code, then long-polling `pc-status` until the phone confirms. | "scan login" |
| **Password login** | Login with phone number + password via `PasswordCipher`. | "manual login" |

## Protocol

| Term | Definition | Avoid |
| :--- | :--- | :--- |
| **ParamCipher v2** | Request parameter encoding scheme (`src/param_cipher.rs`) required by the server. | "param encryption" |
| **`signkey`** | MD5-based signature over selected request parameters, bound to a timestamp/tag. | "checksum", "hash" |
| **Service map** | Server-provided `zpw_service_map_v3` mapping logical services (`chat`, `group`, `file`, …) to hosts; mirrored in `src/endpoints.rs`. | "host list" |
| **`error_code`** | Numeric status inside Zalo JSON responses. `0` means success. | "status" |
| **Push / Pull** | WebSocket message classes: `501/521` push, `510/511` pull, `601–604` control. | "socket message" |

## Chat Domain

| Term | Definition | Avoid |
| :--- | :--- | :--- |
| **Thread** | A conversation: 1:1 user thread or group thread. Identified by `thread_id`. | "chat", "conversation" (when the ID matters) |
| **Message** | One chat entry with `msg_id`, `thread_id`, sender, timestamp, type and raw JSON. | "text" |
| **Local message ID** | Client-generated ID used to reconcile optimistic UI rows with server echoes. | "temp id" |
| **Recall** | Server-side removal of a message for all participants. | "delete" (that is local-only) |
| **Delete for me** | Local-only removal mirrored into SQLite. | "recall" |

## Runtime

| Term | Definition | Avoid |
| :--- | :--- | :--- |
| **Worker** | Tokio task owning all network, disk and crypto work; communicates with the UI via `WorkerCmd` / `WorkerEvent`. | "backend", "service" |
| **UI thread** | The egui/ratatui event loop; must never block on I/O. | "frontend" |
| **Store** | Local SQLite database managed by `MessageStore`. | "cache" (cache is non-authoritative) |
| **Call** | VoIP session (audio/video) handled by `src/call.rs` and `src/audio.rs`. | "phone" |

## Invariants

Domain invariants are tagged `INV-N` here and referenced verbatim from code and tests.

- **INV-1** — No request leaves the process without `ParamCipher v2` encoding and a valid `signkey`.
- **INV-2** — `session.json` is never logged, committed or transmitted outside Zalo endpoints.
- **INV-3** — The UI thread never performs blocking I/O; all such work runs in the worker.
- **INV-4** — A message row is idempotent by `msg_id`; re-delivery never duplicates history.
