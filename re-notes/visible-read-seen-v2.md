# Visible local reads and seen-v2 transport

## Primary source and deliberate protocol exception

Read directly on 2026-11-02 from the existing local research bundle
`dist/sync-fix-evidence/protocol-research/default-login-main-startup-shared-worker-znotification.d9141cdf23123a5c2330.js`.
SHA256: `0a5a08a2a1d074c827d2b1826f8434fc04e243a1daa0a03a376353e54cdd2466`.
Exact primary slices are retained in `visible-read-seen-v2-primary.txt`.
Unicode code-point spans: direct/group endpoint table CP2623220/2633006;
seen-v2 body mapping CP1396480; direct send CP12157091; group send CP12158652;
`_post` CP12131755; common params CP12127557; separate ParamCipher/signkey
implementation CP12128774. Archive comparison: Git object `20a7412`,
`src/read_receipts.rs` and `src/gui/workspace.rs`.

The primary seen-v2 methods call session `encodeAES(JSON)`, form-encode exactly
`params`, attach common query parameters, use credentials and `_post` with
`retry:0`; `_post` defaults to 10000 ms. They do not call `encryptParam` and do
not supply a signing tag. Inventing `seenv2` as a ParamCipher signing tag would
change the protocol without evidence. This is an explicit exception to the
repository's generalized ParamCipher/signkey guidance, not a new assertion that
all chat endpoints use this contract. The implementation centralizes the exact
legacy contract inside Session with shared authenticated request construction,
no body/response logging, bounded response reading, explicit ten-second timeout
and no transport retries or host failover for this write.

The source maps direct command12118 to `/api/message/seenv2`, group12124 to
`/api/group/seenv2`; `msgInfos` is a JSON string with `senderId` or `grid`, and
`data` rows preserve `cmi,gmi,si,di,mt,st,at,cmd,ts`. A source `msgId` must agree
with the wire server identity. Direct sender must match the exact selected
thread; group destination must match the selected group after its `g` prefix.
Display IDs, archive IDs and timestamps never manufacture wire receipt fields.

## Visibility and persistence

Only main-thread actual clipped incoming rows at the bottom of the unfiltered
chat are candidates. Independent native and logical foreground, nonminimized,
unobscured UI and the same account/thread/visible identity must remain sampled
at intervals no longer than250ms for a continuous500ms. Any lost gate invalidates
queued transport tickets. Each ticket also expires after250ms without a fresh monotonic sample, independently of UI frames. Known-unsent reservations return to the live ledger; a polled transport outcome never retries automatically. Pure local reads remain independent of wire eligibility. The account storage lane carries the visibility ticket and rechecks it after queueing, inside its epoch-guarded blocking closure, before changing durable unread state. A stale local result never advances the GUI cursor. Session also checks the captured runtime epoch immediately before receipt HTTP dispatch.
An ephemeral ledger is populated only by live socket/poll events, never startup,
history hydration or manual clear. A request claims at most50 exact live IDs;
unknown transport delivery is not automatically replayed within the current account epoch. Submitted identities are never evicted within an account epoch. After4096 outstanding/attempted identities, new wire work fails closed until the epoch resets; durable local reads continue. A one-time user-visible status announces this cap. This bounded-session transport cap is explicit rather than silently permitting uncertain replay. Exclusion is not persisted across account epochs; durable cross-epoch receipt exclusion is outside this change.

Tests use synthetic wire mappings, disposable AccountStore databases and a
loopback HTTP fixture with a fake session key/cookie. No live mark-seen request,
real account, physical UI/focus interaction or provider acceptance is claimed.
