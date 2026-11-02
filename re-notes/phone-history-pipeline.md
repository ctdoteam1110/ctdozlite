# P1-P3: shared-socket group and phone history restoration

## Evidence and provenance

The protocol source for this slice is archived Git object `20a7412`:

- `src/gui/group_history.rs`: the archive attributes command-511 global group
  pagination to zca-js `listen.ts` at `2a7e360257dfe98e2eb48dc46bbf389834be7e0b`.
- `src/gui/phone_history.rs`: transfer-sync-v2 round state, completion coverage,
  ACK correlation, identity lookup, archive object adapters and regression tests.
- `src/socket.rs:254..476,596..664,914..1053`: shared command ownership,
  590/591/592 envelopes, FIFO/authentication and synthetic socket tests.
- `src/gui/worker.rs`: shared socket routing and control lifetime.
- `src/chat.rs:5133`: `historySource` preservation at the archive adapter boundary.

These are repository-source observations, not a newly captured authenticated
session or independently rehashed vendor bundle. No private captures or account
dumps were opened. Synthetic URLs, IDs, keys and bodies in tests are fixtures.
The existing `native/macos/HistoryCodec.swift`, build scripts, codec assets and
native protocol were not modified. Its disposable existing self-check suite was
run separately from the mock-helper pipeline tests.

## Restored contracts

Group pages are a global stream, not a guessed per-contact endpoint. The explicit
Settings action owns at most twenty 511/subcommand-1 pages and resumes using the
last message in wire order, never a timestamp sort. `req_id`, nullable first cursor
and queue coexistence follow the archive. Existing upstream `LoadGroupHistory`
and `SyncHistory` / `history_sync` (#45), the local API and scheduler remain intact.

The explicit phone Start/Cancel/Wakeup actions flow through the worker to one
shared socket and one owned native codec subprocess. Commands 590 and 591 wrap
`data`, while 592 uses the archived top-level wakeup body. Session indices, public
ephemeral keys, request IDs, round IDs, batch kinds, indexes and final coverage
follow the archive. An unrelated ACK does not complete a round. Disconnect or
rejection is terminal, not success. Repeated wakeups are throttled. Cancellation
aborts the owned task, queues disposal for active rounds and kills its codec child.
A disconnected round is not silently replayed with stale keys after reconnect.

Codec initialization uses app-owned Helpers/HistoryCodec and Resources assets,
then the existing development build location. The existing explicit absolute
ZL_HISTORY_CODEC_ASSETS override is retained. Missing assets produce a terminal
Vietnamese status. No private-key implementation was added to Rust.

## HTTP and resource boundaries

Authenticated identity/group metadata requests retain Session encoded_params,
service domains, cookies and standard signing. A `msgUrl` from an authenticated
transfer control is instead a capability blob URL: GET does not attach session
cookies, token, API params or a guessed ParamCipher signature. Its archived CDN
suffixes are centralized in endpoints.rs. HTTPS, default TLS port, no userinfo,
exact suffix boundary and at most three validated redirects are required. Every
redirect is checked again. Non-success final status is rejected. Blob URLs and
payload contents are not logged.

Bounds: 64 MiB per downloaded blob; 128 MiB compressed and decoded per round;
128 MiB helper line/input with a pre-allocation read limit; 200,000 records per
round/run; 256 MiB decoded message source per run; 4,096 batches per round;
10,000 messages per group page; 20-page group batch; 25-second group response,
45-second whole codec exchange, 60-second HTTP request, 180-second transfer wait.
The helper input/output timeout covers both writing and reading. Numeric-only
bounded diagnostics write on the blocking pool, never on UI.

## Merge and acceptance boundaries

Historical deltas use cache enrichment, not the live-message notification path.
Loaded text, reactions, delivery progress and unread state are not rewound by
archive replay. Source wire identities and attachments remain available without
laundering archive global IDs into live receipt server IDs. Unknown group identity
is retained as an explicit unresolved result instead of a guessed conversation.
The archived cached-sidebar database reader was not ported because its account
store API differs; its pure exact-identity regressions remain test-only.

The pipeline is rebased onto runtime integration `7d1a9dc`. Worker command
channels retain their original SessionScope ticket, including across A/B/A
transitions. PhoneCoordinator starts its child with spawn_scoped(ticket), never
recaptures a future login, and resets on epoch changes. Socket descendants retain
the inherited scope. GuiTx wraps phone/group output in Scoped; both registry and
visible-epoch dequeue gates reject stale batches before reducers or persistence.
UI epoch transitions also invalidate the phone run generation and pending state.

Native helper admission acquires session lock, then mutation guard, validates the
captured account/token/ticket and spawns synchronously under that guard. Both locks
are released before helper IPC awaits. Helper path resolution, ID generation and
diagnostic writes use runtime_scope spawn_blocking; mutation guards reject work
that starts stale. Drop cleanup still queues best-effort round disposal and kills
the owned child even after epoch cancellation. No stale guard prevents cleanup.

Synthetic tests use the actual GuiTx channel and GUI dequeue gates to reject an
A1 batch after A/B/A3 even when account, token and phone generation match; a stale
queued Start cannot create a task. A sentinel executable proves stale/mismatched
helper admission never starts a process. A fresh scoped helper starts, then epoch
cancellation terminates its PID and emits disposal cleanup.

Legacy FIFO persistence remains pending the separate H4 storage integration;
these results do not claim account-store adoption or durable cross-account
persistence acceptance. Real phone approval, production CDN download and live
authenticated history remain untested here.
