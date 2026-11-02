# Embedded API account history

The GUI listener (`run_scoped_local_http_server`) pins an account, epoch and history root when a connection is accepted. The root is the legacy database path's parent, or `.` for a bare filename, matching the GUI startup data directory. The accepted account must equal `Session.login.uin`. A mismatched or unauthenticated owner receives HTTP 503; an invalidated epoch cancels its connection. A later account never becomes the owner of an older request.

Only this listener installs the task-local history context. The standalone CLI/TUI listener and direct public handler calls retain their explicit legacy database/cache behavior. Do not use direct legacy handlers as a GUI history entry point.

## Existing endpoints

| Consumer | Embedded history source |
| --- | --- |
| REST threads; MCP `zalo_list_threads` | AccountStore metadata and newest 5 bodies per thread |
| REST messages; MCP `zalo_get_messages` | AccountStore keyset pages, newest rows returned oldest-first; existing maximum 500, at most 3 queries of 200 |
| REST search; MCP `zalo_search_messages` | AccountStore literal-phrase, Vietnamese-diacritic-folded message-text search, maximum 200; result retains existing StoredMessage JSON shape |
| MCP resource `zalo://threads` | AccountStore metadata and newest 20 bodies per thread |
| REST export | AccountStore metadata and at most 500 newest bodies per thread, preserving export object shape |
| REST/MCP status | AccountStore thread count |

The V2 search engine is not the legacy `LIKE` substring/sender-name search. No unsupported V2 sender-name search or SQL syntax endpoint is invented. A missing V2 store initializes an empty account-owned store; it never imports or falls back to legacy messages or global thread caches. Store access runs in inherited scoped blocking tasks, with a mutation guard around open/query to serialize initialization against transition/wipe.

There is no existing API endpoint to delete message history or mark messages read in this version. This change does not add one. GUI deletion/read state stays owned by the H4 history adapter.

Scheduler and auto-reply schemas, API payloads and standalone behavior are unchanged. Their legacy shared persistence remains an explicit separate ownership gap; account-owned history export does not imply scheduler/auto-reply isolation.

## Verification

The actual loopback HTTP server test seeds different account-owned histories using identical thread/message identities, plus a poisoned legacy row. It sends identical REST routes and MCP request ID 17 through A/B/A transitions, checks 500-row paging, both search and resource consumers, a fresh empty account, UID mismatch rejection, an already-accepted idle connection cancelled across A/B/A, and a separate standalone listener reading only the legacy row. The fixtures use no Zalo credentials, real remote requests or socket login.
