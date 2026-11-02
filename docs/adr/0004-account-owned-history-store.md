# ADR-0004: Separate account-owned incremental history storage

- **Status:** accepted
- **Date:** 2026-11-02
- **Deciders:** Project maintainers, implementation authorized for history restoration

## Context

ADR-0001 describes the legacy history-sync database and import flow. That database
uses schema version 1 and a message-only primary key. Scheduler, API, and sync
cursor tables already share that file. Rebuilding those tables to change message
identity would couple unrelated features and risk destructive migration.

The archived implementation at `20a7412` contains an account-scoped store and
fixture-backed forward-only upgrades. Its storage foundation can be restored
without adopting its GUI, transport, or implicit identity heuristics. The existing
schema-1 store does not persist unread state unless another source supplies it.
An import cannot recover state that was never stored.

## Decision

We add `AccountStore` in `src/store_v2.rs` as a distinct schema namespace:
`<root>/accounts/<sha256(account_id)>/history-v2.sqlite3`, currently version 2.
The legacy `MessageStore`, its schema/version, newest-message query, scheduler,
API tables, and sync cursors remain in their original file. This explicitly
extends ADR-0001 and the database standard's single-message-key convention only
for the new account-owned file; it does not silently upgrade the legacy file.

Within one account, message identity is `(thread_id, msg_id)`. This refines INV-4's
idempotence requirement without conflating equal IDs from different conversations.
Text, timestamps, avatars, and display names never establish identity. Aliases
require explicit protocol identity confirmation and do not rewrite stored rows.

The worker owns the blocking SQLite connection (INV-3). New files/directories use
0600/0700 on Unix. Existing files receive a read-only owner/version preflight
before permissions, pragmas, or migration can mutate them. Unknown newer versions
and owner mismatch fail closed. Versioned upgrades run one atomic transaction per
step, preserving the version on failure. WAL, NORMAL synchronous mode, a 5000 ms
busy timeout, and foreign keys apply to initialized connections.

Legacy import is explicit copy-on-write into the destination transaction. SQLite
sources open read-only under a read snapshot; JSON sources are read without writes.
A source-set manifest makes repeated imports idempotent. The source stays available
as the rollback copy, so no destructive source migration or backup rewrite occurs.
Malformed imports roll back destination rows and the manifest. Optional legacy
unread tables and JSON unread state are imported when present; missing state is
not invented.

Incremental writes retain normalized metadata, opaque raw payloads, reaction replay
ledgers, delivery progress, mentions, and unread cursors. Exact local deletions
create account/thread/message tombstones which stale snapshots cannot resurrect.
Local visible-page reads alter local state only and never emit a wire receipt.

History uses an exclusive `(ts, msg_id)` keyset and a compound index. A page returns
at most 200 bodies and one `has_more` flag. Cursor predicates avoid a nullable OR
which otherwise prevents SQLite from seeking past the history prefix. FTS5 search
quotes user text as a literal phrase, removes diacritics, folds Vietnamese D-stroke,
and caps results at 200. Quote navigation resolves exactly one server/client
reference across namespaces; collisions remain unresolved and its context is
bounded to 200 bodies.

## Consequences

- Positive: account isolation, non-destructive import, durable deltas, bounded
  history/search, and independent schema upgrades have executable fixture coverage.
- Negative: the legacy and account-owned stores coexist; the new module alone does
  not switch GUI storage or prove full-history availability in the running app.
- Follow-ups: H4 must adopt the store at the worker/GUI boundary, select the account
  identity explicitly, and validate actual user-visible archive navigation. Legacy
  unread durability remains a known limitation until that adoption replaces it.
- Compatibility: the narrowly related legacy delete method now requires both exact
  thread and message IDs, reports SQLite errors, and does not delete other messages
  sharing a timestamp. Its timestamp parameter remains source-compatible but has
  no matching semantics.

## Alternatives Considered

| Option | Why rejected |
| :--- | :--- |
| Rebuild the shared legacy file in place | Couples scheduler/API data and risks destructive migration. |
| Match legacy threads or messages by name/text/time | Ambiguous evidence can merge unrelated identities. |
| Copy the archived GUI and network stack | Exceeds foundation scope and changes protocol behavior. |
| Treat the module as GUI history adoption | Does not prove the running application's storage path. |

## Verification

`tests/schema_migrations.rs` exercises version, owner, failed-upgrade rollback,
permissions/no-mutation rejection, and reopen behavior. `tests/store_v2_regressions.rs`
exercises composite IDs, COW imports, tombstones, metadata, keyset pages, literal
search, exact navigation, and 10,000-row incremental writes. The module's query-plan
fixture compares nullable-OR and direct keyset predicates on the same 10,000 rows;
timing is diagnostic, while the index/range plan is the deterministic acceptance.
