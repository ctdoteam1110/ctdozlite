# 05 — Local Message Store (SQLite) & Security Hardening Report

Date: 2026-10-21
Scope: Task 1 (Security hardening), Task 2 (Local SQLite message store + incremental polling), Task 3 (Multi-message thread preview parsing), Task 4 (`group/history` endpoint for groups).

---

## 1. Files Created and Changed

| File | Status | Purpose |
|---|---|---|
| `.gitignore` | Created | Ignores build artifacts, session secrets, backups, SQLite databases, dumps, and QR images. |
| `SESSION_SECURITY.md` | Created | Documents live credential risks in `session.json` and security handling rules. |
| `Cargo.toml` | Modified | Added `rusqlite = { version = "0.32", features = ["bundled"] }`. |
| `src/session.rs` | Modified | Added `redact_secret` helper, URL query redaction in verbose logs, and unit tests. |
| `src/bin/test_fetch.rs` | Modified | Masked `zpw_enk` and `zpw_sek` output using `redact_secret`. |
| `src/qr.rs` | Modified | Redacted captured `zpw_sek` cookie and query URLs in debug logging. |
| `src/bin/compare.rs` | Modified | Redacted `enk` in decoded request fields and error reporting. |
| `src/store.rs` | Created | SQLite storage engine (`MessageStore`), schemas, indices, upsert methods, and unit tests. |
| `src/lib.rs` | Modified | Exported `pub mod store;`. |
| `src/main.rs` | Modified | Registered `mod store;` and added thread persistence upon initial preload. |
| `src/chat.rs` | Modified | Added `ts` to `ChatMessage`, rewrote `merge_previews`, implemented `get_group_history` and `merge_group_history`, added unit tests. |
| `src/tui.rs` | Modified | Added SQLite store integration on polling/refresh, added key `h` for on-demand group history, and updated footer shortcuts. |
| `src/gui.rs` | Modified | Populated timestamp `ts` on outgoing `ChatMessage` instances. |
| `re-notes/05_message_store.md` | Created | Full documentation and verification report. |

---

## 2. `.gitignore` Contents

```gitignore
target/
session.json
*.bak*
dump/
qr.png
*.db
src.bak.*
.env
```

---

## 3. Credential Printing Sites Found & Redactions Applied

Every occurrence printing credentials across `src/` was identified and sanitized:

1. **`src/bin/test_fetch.rs` (lines 23–24)**
   - *Previous:* Printed `li.zpw_enk` and `li.zpw_sek` directly to stdout.
   - *Fix:* Replaced with `zalo_rust::session::redact_secret(&li.zpw_enk)` and `zalo_rust::session::redact_secret(&li.zpw_sek)`.

2. **`src/qr.rs` (line 380)**
   - *Previous:* `eprintln!("  Captured zpw_sek cookie: {k}");` in verbose mode.
   - *Fix:* `eprintln!("  Captured zpw_sek cookie: {}", crate::session::redact_secret(k));`.

3. **`src/qr.rs` (line 377)**
   - *Previous:* Printed raw request URL containing query tokens.
   - *Fix:* Sanitized URL through `crate::session::redact_url(url)`.

4. **`src/session.rs` (line 343)**
   - *Previous:* `eprintln!("→ {} {url}", m);` printed raw URL with `params` / `signkey` / `imei` in verbose mode.
   - *Fix:* Redacted via `if self.dump_raw { url.to_string() } else { redact_url(url) }`.

5. **`src/bin/compare.rs` (line 98)**
   - *Previous:* Printed decoded request fields containing `enk` in cleartext.
   - *Fix:* Redacted `enk` field with `zalo_rust::session::redact_secret(v)`.

6. **`src/bin/compare.rs` (line 148)**
   - *Previous:* `println!("decrypt failed with enk={enk}");`.
   - *Fix:* `println!("decrypt failed with enk={}", zalo_rust::session::redact_secret(&enk));`.

---

## 4. SQLite Schema as Shipped

Shipped in `src/store.rs` with `bundled` rusqlite (no external DLL or C compiler dependency required on target):

```sql
CREATE TABLE IF NOT EXISTS messages (
    msg_id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL,
    sender_id TEXT,
    sender_name TEXT,
    text TEXT,
    msg_type TEXT,
    ts INTEGER,
    is_outgoing INTEGER,
    raw_json TEXT
);

CREATE INDEX IF NOT EXISTS idx_thread_ts ON messages(thread_id, ts);

CREATE TABLE IF NOT EXISTS threads (
    thread_id TEXT PRIMARY KEY,
    name TEXT,
    is_group INTEGER,
    last_ts INTEGER
);
```

---

## 5. Task Status & Evidence

### Task 1 — Security Hardening: DONE
- Created `.gitignore` preventing leaks of `session.json`, `dump/`, `*.db`, etc.
- Added `redact_secret` helper showing at most 8 chars + `…<redacted>` with unit tests.
- Created `SESSION_SECURITY.md`.
- Redacted all 6 identified credential-printing sites across `src/`.

### Task 2 — Local Message Store (SQLite) + Polling Persistence: DONE
- Added `rusqlite = { version = "0.32", features = ["bundled"] }` to `Cargo.toml`.
- Implemented `MessageStore::open`, `upsert_message` (`INSERT OR IGNORE`), `upsert_thread` (`ON CONFLICT DO UPDATE`), `messages_for` (ordered by `ts ASC`), and `persist_threads`.
- Exported `store` in `src/lib.rs` and `src/main.rs`.
- Persisted threads and messages to `zalo_history.db` on initial startup in `main.rs`, and on every manual refresh and background poll in `tui.rs`.
- *Evidence:* 3 unit tests in `src/store.rs` (`test_insert_twice_returns_false`, `test_messages_for_returns_ordered_rows`, `test_persist_threads_and_upsert_thread`).

### Task 3 — Multi-Message per Thread Parsing: DONE
- Rewrote `merge_previews` to handle single objects (`lastMsg`, `message`) and arrays (`messages`, `msgs`), as well as flat message nodes.
- Sorted `Thread.messages` by timestamp ascending (`ts ASC`), updated `Thread.preview` from the newest message, and cleanly skipped entries that parse to nothing.
- *Evidence:* 4 unit tests in `src/chat.rs` covering single `message` object, `lastMsg` object, 3-item `messages` array with out-of-order timestamps, and empty entries.

### Task 4 — Group History Endpoint & TUI Exposure: DONE
- Added `get_group_history(s: &Session, group_id: &str, count: u32) -> Result<Value>` querying `{domains.group}/api/group/history` with param encryption and unauthenticated guard.
- Added `merge_group_history(thread: &mut Thread, resp: &Value)` tolerating `data`, `data.msgs`, `data.groupMsgs` arrays, deduping by `msg_id`, and sorting by `ts ASC`.
- Exposed on key `h` in TUI for selected group threads, updating thread messages and writing to `zalo_history.db`.
- *Evidence:* `test_merge_group_history_fixture` and unauthenticated check in `src/chat.rs`.

---

## 6. Unverified Claims `[U]`

- **Group History Body Fields:**
  In `get_group_history`:
  ```rust
  let body = obj(&[
      ("groupid", json!(group_id)),    // [U] inferred from group API family, verify live
      ("count", json!(count)),         // [U] inferred from group API family, verify live
      ("globalMsgId", json!(0)),      // [U] inferred from group API family, verify live
      ("imei", json!(s.imei)),
  ]);
  ```
  The exact naming (`groupid` vs `grid`, `globalMsgId`) could not be verified against live server traffic without an active group conversation containing historical messages on the authenticated test account.

---

## 7. Verification & Build Summary

- **`cargo check --all-targets`:** Passed with 0 errors, 0 warnings.
- **`cargo test --lib`:** 34 passed, 0 failed (baseline was 25; +9 new tests).
- **`cargo test --bin zalo-rust`:** 34 passed, 0 failed (baseline was 25; +9 new tests).
- **`cargo build --release`:** Finished in release mode.
- **Binary Size (`target\release\zalo-rust.exe`):** 10,796,032 bytes (~10.3 MB).
