# Phone SyncFull identity namespace — 2026-11-07

Author: Codex. No live identifiers, credentials or raw device logs are included.

## Correction to note 39

The previous scope interpretation used `WorkerScopeBuilder` from the separate PC-to-phone
backup pipeline. It enumerates desktop conversation IDs. Phone-to-PC SyncFull uses a different
query builder. A nonempty bare desktop ID is still not a valid phone partition.

Owning source: installed official Zalo PC 26.11.10, public static `app.asar` code only:

- `pc-dist/shared-worker.cfeba43a9a8e4e40dda3.js`, offset 322317: `QueryFactory.createMessageQuery`
  takes `globalNoisedId` and calls `getPartitionFromConversation`. Prepared queries skip ranges
  with no global mapping. Offset 323233 delegates formatting to module `aSMs`.
- `compact-app-pc.cfeba43a9a8e4e40dda3.js`, offset 13378686: `aSMs` formats
  `oneone/<globalId>` or `group/<globalId>`, not a bare desktop ID or `g<desktopId>`.
- Shared worker offset 345307 resolves `safePcNoised2gNoised` before requesting messages;
  offset 349841 does the equivalent for prepared ranges.
- `main-startup` offset 830621 maps desktop IDs through friend/group `globalId` metadata.
- Compact offset 15035607: profile-service GET `/api/gid/encrypt` (cmd 12053), encrypted params
  `{noiseUids: ids}`. Offset 15035831: GET `/api/gid/decrypt` (cmd 12054), params
  `{globalUids: JSON.stringify(ids)}` for reverse sender translation.

Global IDs are opaque strings and can contain base64 punctuation. Do not parse them as numeric
desktop IDs, strip a leading `g`, or split away a slash inside the opaque part.

## Real-device evidence

The user saved a filtered Console buffer locally outside Git: 36,960 records covering
17:32:57–17:33:58 +0700. The controlled request was acknowledged at 17:33:02.

- 17:33:03.769792: phone looked up the UI label meaning “Syncing messages”.
- 17:33:03.786129: Zalo entered its exit handler.
- 17:33:04.166033: CoreAnalytics reported an exit-barrier timeout, after exit had already begun.
  That timeout does not establish the cause of termination.
- 17:33:04.205915: launchd termination was reported as `(0, 0, 0)`.
- 17:33:04.244112: SpringBoard reported `RBSProcessExitContext| voluntary`.
- locationd recorded `isUserKill: 0` for the same process.

This narrows the symptom to termination during phone sync. The log does not contain the mobile
function or condition that initiated exit, and does not prove that this namespace error is the
sole cause. The earlier JetsamEvent concerned mDNSResponder rather than Zalo. Localization
catalog lookups for generic TLS/DNS errors are not evidence those errors occurred.

Manual read-only probes using the existing standard Session client confirmed that user and
group `globalId` fields differ from local IDs and are nonnumeric. Both identity API routes
returned error_code 0 and round-tripped the account UID. Only codes/booleans were recorded;
the probes did not send a phone request. The checked success envelope contained an additional
`data`-only wrapper; any nested envelope codes must also be validated while unwrapping it.

## Repair

Resolve global user IDs and group metadata before key generation or cmd 590. Use exact prefixed
phone partitions and retain their original desktop thread IDs for returned data. Skip unresolved
scopes, never substitute a raw desktop ID, and disclose omitted scopes in completion text.
Missing account identity or an entirely unresolved set stops the request without another phone
protocol attempt. Cancellation during identity lookup must not create a new phone prompt.

Translate returned partition and sender identities before SQLite insertion. Unknown senders use
the observed reverse API with bounded waits; do not guess message direction. Reject unrequested
partitions or unresolved senders before writing. Decrypt all framed records under single-writer
admission, bound total decoded bytes across the batch, and insert the mapped batch atomically.

Fixture-only coverage includes opaque IDs, correct prefixes, deduplication, missing mappings,
mapping parameter types, nested/rejected envelopes, large local IDs, routing, sender direction,
and skipped-scope reporting. Real phone import remains a release prerequisite.
