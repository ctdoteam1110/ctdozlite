# API, IPC & Error Handling Standards

This project is a client, not a server. The "API" surface is three boundaries:

| Boundary | Direction | Module |
| :--- | :--- | :--- |
| **Zalo HTTP API** | outbound | `session.rs`, `chat.rs`, `login.rs`, `qr.rs`, `media.rs` |
| **Zalo WebSocket** | bidirectional | `socket.rs` |
| **UI ↔ Worker IPC** | in-process | `gui/worker.rs` (`WorkerCmd` / `WorkerEvent`) |

Plus local filesystem persistence (`store.rs`, `session.json`).

---

## 1. Outbound HTTP Conventions

- **Hosts come from `src/endpoints.rs` only.** No literal `*.zalo.me` / `*.zaloapp.com` URLs elsewhere.
- **Every request goes through one path**: build params → `ParamCipher v2` (`param_cipher.rs`) → `signkey` (`crypto.rs`) → attach session cookies → send.
- **No ad-hoc `reqwest::Client` per call.** Reuse the shared client so cookies and TLS config stay consistent.
- **Timeouts are explicit** on every request; no unbounded awaits.
- **Response parsing is strict**: decode into `serde` structs, not ad-hoc `Value` walks on hot paths.

### Response Envelope

Zalo responses use `{ error_code, error_message, data }`. Parse once, then branch:

| `error_code` | Meaning | Handling |
| :--- | :--- | :--- |
| `0` | Success | Return typed `data`. |
| `18012` | QR scanned, not yet confirmed | Keep long-polling; no user error. |
| `18022` | Invalid signature | Bug: wrong `signkey` parameter set. Do not retry; log and fix. |
| `18060` | Invalid encryption protocol | Bug: request bypassed `ParamCipher v2`. Do not retry. |
| `600` / `-102` | `zpw_sek` missing or expired | Fail closed: clear session, restart QR login. |
| `114` | Invalid parameter type | Bug: malformed nested JSON. Do not retry. |
| `429` | Rate limited | Back off exponentially; surface a retry hint. |

Canonical table lives in `docs/KHAC_PHUC_SU_CO.md`; keep both in sync when a new code is observed.

---

## 2. Error Taxonomy

Use `anyhow::Result` at module boundaries. Add typed errors only where callers must branch on the cause.

| Category | Examples | Retry? | Surface |
| :--- | :--- | :--- | :--- |
| `Network` | DNS, TLS, timeout, socket reset | Yes, with backoff | "Mất kết nối, đang thử lại…" |
| `Auth` | expired `zpw_sek`, invalid credentials | No | Prompt re-login |
| `Protocol` | bad `signkey`, cipher mismatch, decode failure | No (bug) | Log + generic error |
| `RateLimit` | `429` | Yes, honoring delay | "Thao tác quá nhanh, thử lại sau" |
| `Io` | SQLite, file, media read/write | Conditional | Context-specific |
| `Unsupported` | missing `ffmpeg`, missing audio device | No | Actionable hint |

Rules:

- **No `unwrap()` / `expect()` on network, parse, file or user-input paths.** Allowed in tests and in true compile-time invariants with a message.
- **`?` over manual matching**; add `.context("...")` when the error crosses a module boundary.
- **Never swallow errors silently.** `let _ = ...` is only acceptable for best-effort cleanup, and requires a Tier 2 comment.
- **Never include secrets in error text** — no cookies, `zpw_sek`, `zpw_enk`, tokens or raw `session.json` fragments.
- **User-facing messages are Vietnamese; log messages are English.**

---

## 3. Retry & Backoff

- Retry only `Network` and `RateLimit` categories.
- Exponential backoff with jitter: `250ms → 500ms → 1s → 2s`, capped at 8s, max 4 attempts.
- Honor `Retry-After` / `429` delay when present.
- Never retry:
  - Auth failures (must re-authenticate instead).
  - Protocol bugs (`18022`, `18060`, `114`).
  - Non-idempotent operations without a stable idempotency key.
- Background polling (`POLL_SECS`) is not a retry loop; failures must be logged and the next tick scheduled.

---

## 4. Idempotency

- Client-generated message IDs are created **once** per user action and reused across retries; never regenerate on retry.
- Local persistence is idempotent by `msg_id` (`INSERT OR IGNORE`), so re-delivery cannot duplicate history (INV-4).
- Optimistic UI rows reconcile by local message ID; a server echo updates the row, it does not insert a second one.

---

## 5. WebSocket Conventions

- Command IDs are constants in `socket.rs`; never inline magic numbers.
- A push frame is acknowledged/parsed without blocking the read loop; heavy decoding moves to `spawn_blocking`.
- Reconnect uses jittered backoff and re-authenticates before resubscribing.
- Unknown frame types are logged and skipped, never fatal.

---

## 6. UI ↔ Worker IPC

- The UI thread sends `WorkerCmd` and renders `WorkerEvent`; it never performs I/O (INV-3).
- One command variant = one user intent. Do not add a generic `Do(String)` escape hatch.
- Every command that can fail emits a terminal event (success or error); the UI must not hang on a dropped command.
- Long-running tasks report progress events rather than blocking the channel.
- Channels are unbounded today for UI responsiveness; any producer that can flood must rate-limit at the source.

---

## 7. Local Persistence Boundary

- `store.rs` returns `anyhow::Result`; callers decide whether the failure is fatal.
- A failed history write must never block message delivery or UI rendering.
- Schema and migration rules: `docs/standards/database-and-migrations.md`.

---

## 8. Review Checklist

- [ ] No hardcoded host outside `endpoints.rs`.
- [ ] New request uses the standard cipher/sign path.
- [ ] Error branch added for any new `error_code`.
- [ ] Retries limited to idempotent / transient failures.
- [ ] No secret in logs, errors or issue reports.
- [ ] New `WorkerCmd` has a matching completion event.
