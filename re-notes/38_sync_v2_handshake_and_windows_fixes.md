# Sync V2 mobile handshake and Windows subprocess isolation

## Evidence and upstream inspection

Inspection of Zalo PC Electron bundle `app.asar` (version 26.11.11) and repository source:

- `src/gui/worker.rs:4938` invokes `std::process::Command::new("ffmpeg")` to extract
  the first video frame for chat thread thumbnails without `CREATE_NO_WINDOW`. On
  Windows, `CreateProcessW` without `0x0800_0000` allocates a standard console host,
  causing a visible `cmd.exe` or `conhost.exe` flicker for each thumbnail job.
- `src/gui/messages.rs:552, 622` and `src/gui/worker.rs:4975` invoke `powershell`
  for screen capture and native dialogs without window suppression flags.
- `src/history_sync.rs:323-343` collects all thread IDs via `resolve_thread_partitions`
  and passes them to `build_sync_v2_payload`. When tens or hundreds of partition
  queries are serialized, the resulting payload is passed to `request_phone_sync_v2_http`
  at `src/chat.rs:8867` via HTTP GET query string `params={params}`. Upstream reverse
  proxies reject request-lines exceeding standard 4KB to 8KB limits with HTTP 414.
- `src/logger.rs:154-158` defaults `log_sync` output to `/tmp/sync_debug.log` when
  `HOME` is unset. On Windows, `/tmp` does not exist, causing logging failures to
  discard network diagnostics silently.
- Zalo PC `shared-worker.js` initiates the sync session through WebSocket CMD 590
  (`sue.requestSyncMessage`) using an initial request payload where `req.type` is
  `"conversation"` and `req.queries` contains only the `"conversations"` partition.
  Individual `oneone/<nid>` and `group/<nid>` message queries are dispatched only after
  receiving `USER_CONFIRM` from the mobile client.

## Protocol contract and constraints

1. Session initiation payload:
   - `syncType`: 0 (`SyncFull`).
   - `toDevice`: 0 (`Mobile`).
   - `req.type`: `"conversation"`.
   - `req.queries`: Single query targeting partition `"conversations"` with limit 2000.
   - Public keys: Real Curve25519 `ik` and `ek` generated through the WASM runtime.
2. Partition query payload (post-confirmation):
   - Dispatched only after the mobile client acknowledges the session.
   - `req.type`: `"message"`.
   - `req.queries`: Partition paths resolved via `/api/znoise`.
3. Transport fallback:
   - HTTP 12888 (`/api/transfer-sync-v2/request-sync`) sends payloads via form-urlencoded
     POST when payload size exceeds URI line limits.
4. Windows subprocess constraints:
   - All invocations of `ffmpeg` and `powershell` on Windows attach `CREATE_NO_WINDOW`
     (`0x0800_0000`) via `std::os::windows::process::CommandExt`.
   - Windows diagnostic logs resolve against local application data rather than `/tmp`.
