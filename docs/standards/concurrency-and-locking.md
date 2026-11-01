# Concurrency & Locking Standards

Runtime model: one UI thread (`egui` / `ratatui`), one Tokio runtime in the worker, blocking work on `spawn_blocking`, SQLite owned by the worker.

---

## 1. Thread Ownership Matrix

| Resource | Owner | Access rule |
| :--- | :--- | :--- |
| UI state (`egui` context, widget state) | UI thread | Never mutated off-thread. |
| Tokio runtime, HTTP client, WebSocket | Worker | UI only sends `WorkerCmd`. |
| SQLite `MessageStore` | Worker | One connection, no cross-thread sharing. |
| `session.json` / `Session` | Worker | UI receives snapshots, not handles. |
| Media/sticker caches | Worker (write), UI (read snapshot) | No shared `&mut`. |

**INV-3**: the UI thread never blocks on network, disk, crypto or SQLite.

---

## 2. Channel Rules

| Channel | Type | Direction | Rule |
| :--- | :--- | :--- | :--- |
| Commands | `tokio::sync::mpsc::UnboundedSender<WorkerCmd>` | UI → Worker | One variant per user intent; no generic escape hatch. |
| Events | `std::sync::mpsc` (UI polled via `try_recv`) | Worker → UI | Terminal event required for every fallible command. |
| WebSocket | Tokio task | Worker internal | Read loop never blocks on decoding. |

- Sending must never block the UI; use `send`/`try_send` appropriately and handle a closed channel.
- A producer that can flood must rate-limit at the source (polling cadence, coalescing) rather than buffering unboundedly.
- Dropping the receiver must not panic the worker; log and shut down cleanly.

---

## 3. Blocking Work

- `spawn_blocking` for: SQLite, image decode, ZIP/sticker unpack, file hashing, audio device setup.
- Never call `block_on` inside async context.
- Never hold a `MutexGuard` across `.await`.
- File and DB handles stay inside the blocking closure.

---

## 4. Shared State

| Tier | Mechanism | When |
| :--- | :--- | :--- |
| **Tier 1 — message passing** | `mpsc` channels | Default. Prefer over shared state. |
| **Tier 2 — single-writer** | worker-owned struct | Mutable state only the worker touches. |
| **Tier 3 — shared read** | `Arc<T>` immutable snapshot | UI needs data the worker produced. |
| **Tier 4 — lock** | `std::sync::Mutex` / `RwLock` (short critical sections) | Rare, and only when tiers 1–3 do not fit. |

If a lock is unavoidable:

- Document why in a Tier 2 comment.
- Keep the critical section free of I/O, logging and allocation-heavy work.
- Never acquire a second lock while holding one unless an ordering rule is documented.

---

## 5. Lock Ordering & Deadlock Prevention

When multiple resources must be locked:

1. Define a global order once (e.g. `session → store → cache`).
2. Acquire in that order everywhere; never invert.
3. Prefer try-lock with timeout over indefinite blocking on user-visible paths.
4. If ordering cannot be guaranteed, restructure to message passing instead of adding a lock.

---

## 6. Failure Policy

| Operation | Policy | Rationale |
| :--- | :--- | :--- |
| UI refresh | **Fail-open** | Stale view beats a frozen window. |
| Message send | **Fail-closed** | Never show "sent" when the server rejected it. |
| History write | **Fail-open** | Local cache loss must not drop a message. |
| Session/auth | **Fail-closed** | Never continue with an expired credential. |
| Call signalling | **Fail-closed** | Hang up cleanly instead of a half-open call. |

---

## 7. Timeouts & Cancellation

- Every network await has a timeout; no unbounded `await`.
- Long operations check a cancellation token / channel-closed state between units of work.
- On app shutdown: signal cancellation, drain the command channel, join with a bounded wait, then exit.
- Background tasks that outlive their owner are a leak — tie them to the worker lifetime.

---

## 8. Race Checklist

- [ ] No shared mutable state without a documented owner.
- [ ] No `.await` while holding a lock.
- [ ] No `block_on` in async code; no blocking call on the UI thread.
- [ ] Every channel send handles a closed receiver.
- [ ] Every spawned task has an owner and a termination path.
- [ ] Polling loops have a fixed cadence and cannot overlap themselves.
