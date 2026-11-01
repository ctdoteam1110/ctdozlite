# Database & Migration Standards (SQLite)

Persistence is local SQLite via `rusqlite` (bundled), owned by `src/store.rs`. There is no server database.

---

## 1. Schema Conventions

| Rule | Detail |
| :--- | :--- |
| Naming | `snake_case` for tables, columns, indexes. |
| Table names | Plural (`messages`, `threads`). |
| Primary keys | Natural stable IDs: `msg_id TEXT PRIMARY KEY`, `thread_id TEXT PRIMARY KEY`. No autoincrement surrogate unless justified. |
| Timestamps | `INTEGER` Unix epoch milliseconds in a `ts` / `last_ts` column; name them consistently. |
| Booleans | `INTEGER` 0/1, column prefixed with `is_` (`is_outgoing`, `is_group`). |
| Nullability | Explicit. Prefer `NOT NULL` for identity/lookup columns. |
| Raw payloads | Keep `raw_json TEXT` for forward-compatible replay; never parse it on the hot path. |
| Foreign keys | `PRAGMA foreign_keys = ON` per connection when relations are added. |

Current tables: `messages (msg_id, thread_id, sender_id, sender_name, text, msg_type, ts, is_outgoing, raw_json)`, `threads (thread_id, name, is_group, last_ts)`. Extend with new tables, do not overload existing columns.

---

## 2. Connection Setup

Every opened connection MUST apply:

```sql
PRAGMA journal_mode = WAL;      -- concurrent reads while the worker writes
PRAGMA synchronous = NORMAL;    -- durability/throughput balance for chat history
PRAGMA busy_timeout = 5000;     -- bounded wait instead of instant SQLITE_BUSY
PRAGMA foreign_keys = ON;
```

Rules:

- `MessageStore` is **not** shared across threads. The worker owns the connection; UI never touches SQLite directly (INV-3).
- Open `:memory:` for tests and fixtures.
- Do not hold a `Connection` across `.await`; run DB work inside `spawn_blocking` or a dedicated task.

---

## 3. Indexing Strategy

| Index | Purpose |
| :--- | :--- |
| `PRIMARY KEY (msg_id)` | Idempotent upsert. |
| `idx_thread_ts (thread_id, ts)` | Thread history pagination (high-selectivity `thread_id` first). |
| Partial index | For sparse states, e.g. `WHERE is_outgoing = 1`, when a query proves it. |

- Add an index only with a query that needs it (YAGNI); record the query in the migration comment.
- `EXPLAIN QUERY PLAN` before merging any index or query that touches history pagination.

---

## 4. Query Rules

- **No `SELECT *` on hot paths.** Project only required columns.
- **Always parameterize** (`params![...]`); never interpolate values into SQL.
- **Batch writes** inside one transaction (`conn.transaction()`), especially history sync.
- **Upserts are idempotent**: `INSERT OR IGNORE` then targeted `UPDATE`, keyed by `msg_id`.
- **Bounded reads**: always `LIMIT`; never load an unbounded thread into memory.
- Escape/quote identifiers defensively when building any dynamic SQL.

---

## 5. Transactions

- Wrap interdependent mutations in one transaction; history insert + thread `last_ts` update is one unit.
- Keep transactions short — no network, crypto or `await` inside.
- On failure, return the error; do not partially commit and log "best effort".

---

## 6. Migration Policy

Schema evolves through **versioned, forward-only migrations** tracked by `PRAGMA user_version`.

```
schema version N = PRAGMA user_version
migrate path: N -> N+1 -> ... -> target
```

Rules:

| Rule | Detail |
| :--- | :--- |
| Single source | Migration list lives in `store.rs` (or `store/migrations.rs`); no ad-hoc `CREATE TABLE` at call sites. |
| Idempotent bootstrap | Fresh DB runs every migration from 0; existing DB runs only missing steps. |
| Forward-only | No down-migrations; ship a compensating migration instead. |
| Atomic step | Each step runs in a transaction and bumps `user_version` in the same transaction. |
| Startup safety | Migration failure aborts startup with a clear error; never run on a corrupted/unknown newer version. |
| Backup | Before a destructive migration, copy the DB file to `<name>.bak-<user_version>` when feasible. |

### Expand & Contract (SQLite adaptation)

SQLite has limited `ALTER TABLE`; use the rebuild pattern:

1. **Expand**: create the new table/column (nullable or defaulted); code writes both old and new shapes.
2. **Backfill**: copy/transform rows in a migration transaction.
3. **Contract**: switch reads to the new shape; drop the old table/column in a later release.

Never rename or drop a column in the same release that stops writing to it.

### Unsupported-version Guard

If `user_version > CURRENT_SCHEMA_VERSION`, refuse to open the store and tell the user to upgrade the app — do not silently write into a newer schema.

---

## 7. Data Retention & Privacy

- Local history is user data: deletion must be real (`DELETE`), not soft-deleted unless the UI exposes restore.
- `session.json` is never stored in SQLite.
- No raw credentials, cookies or tokens in any column.
- Tests use `:memory:` or temp files; never the user's real database.

---

## 8. Review Checklist

- [ ] New column is `snake_case`, typed, and nullable only when justified.
- [ ] Migration bumps `user_version` atomically and handles fresh + existing DBs.
- [ ] Hot-path query has explicit projections + `LIMIT` + an index if needed.
- [ ] Writes are parameterized and idempotent.
- [ ] No blocking DB call on the UI thread.
