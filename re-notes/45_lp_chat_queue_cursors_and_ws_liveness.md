# 45 — `lp_chat` multi-queue cursor pagination & WebSocket sleep-wake liveness

## Problem

After closing/sleeping a MacBook overnight (or switching Wi-Fi subnets), messages received while offline failed to appear upon wake, even though they remained intact on Zalo mobile.

## Live Evidence (2026-11-26)

1. **`lp_chat` (`p4-msg-chat.zaloapp.com`) queue cursor starvation:**
   - A live probe against `getChatFromPolling` returned `queueStatus` keys `1`, `2`, `3`, `7`, `8`, `15`, and `16`, where queue `16` carries paginated `groupMsgs` (100 messages/page, `more: 1`) and queue `15` carries 1-1 message cursors.
   - Previously, `PollState::update_from_response` (`src/chat.rs`) ignored every queue not in `["1", "2", "3"]`.
   - **Observed with `["1", "2", "3"]` only:** every subsequent poll (`A2`, `A3`, …) returned `error_code = 0, more = 1` with the exact same 100 group messages from page 1 (~24h old) forever, never advancing to pages 2–6 (491 missed messages across overnight/morning).
   - **Observed when tracking all numeric `queueStatus` keys (`1`, `2`, `3`, `7`, `8`, `15`, `16`):**
     - `B1..B5`: `error_code = 0, more = 1`, each advancing cleanly through 100-message windows.
     - `B6`: `error_code = 0, more = 0`, draining the remaining 91 messages up to current wall-clock time.
     - `B7`: cleanly held open as an idle long-poll with zero server rejection.

2. **Zombie WebSocket (`zpw_ws`) across sleep / Wi-Fi change:**
   - `lsof -nP -p <pid> -i` showed the active `zpw_ws` socket still `ESTABLISHED` bound to the previous Wi-Fi subnet (`192.168.1.38 -> 49.213.95.30:443`) while the host's active IP had moved to `192.168.2.10`.
   - `src/socket.rs` sent `CMD_PING` every `15s` without checking whether any server frame/pong arrived and without send/connect timeouts, leaving half-open sockets wedged indefinitely.

## Fix

- `src/chat.rs`: Track all numeric `queueStatus` queues in `PollState::update_from_response`, unwrap stringified `data` if present, merge `ids` across batches, and raise `poll_new_messages` drain cap from 3 to 20 pages (with cursor-progress stop guard).
- `src/socket.rs`: Add `CONNECT_TIMEOUT_SECS` (15s), `SEND_TIMEOUT_SECS` (10s), `STALE_TIMEOUT_SECS` (45s without inbound frame/pong), and reset reconnect backoff to `2s` after a successful auth.
- `src/gui/worker.rs` & `src/history_sync.rs`: Raise multi-page `lp_chat` drain cap to 20 (with cursor-progress stop guard), detect wall-clock sleep/wake jumps (`>= 60s`) to trigger `WorkerCmd::RefreshThreads`, and trigger throttled `RefreshThreads` catch-up on WebSocket reconnect `CMD_AUTHEN`.
