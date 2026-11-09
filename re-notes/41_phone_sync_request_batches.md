# Native-sized phone sync requests — 2026-11-07

Author: Codex. No account identifiers, keys or raw phone logs are included.

## Evidence

After the identity-namespace repair in note 40, a real one-conversation transfer succeeded:
the phone remained open, account history grew by 25 rows, temporary key/session files were
cleared, and the user confirmed success in the Mac sync page.

The subsequent full-scope attempt lost its WebSocket acknowledgement and fell back to legacy.
The user reported a cloud-only completion: 100 messages / 320 conversations, with phone history
not loaded. That is not a successful full phone transfer. The exact close code was not captured;
do not assert a specific server frame-size limit from this result.

Installed official PC 26.11.10, public static source:

- `pc-dist/shared-worker.cfeba43a9a8e4e40dda3.js`, offset 302089: `buildPrepareQuery` groups
  queries by priority and starts another request when the current group's query count reaches
  30 (`getQueryCount() < 30`). The previous community implementation put the whole scope set
  into one request. A read-only preflight resolved 504 scopes, so the native boundary requires
  17 requests, with at most 30 scopes each.
- Offset 292355: the session manager creates each request with the shared sync-proto identity,
  a newly generated ephemeral key, and the one-time key only for its first session.
- Offset 284199 builds the request with the increasing user-sync index (`ussidx`).
- Offset 281251 disposes a remote session; the manager's `release` disposes all sessions when
  the overall database operation closes, rather than declaring each request a full completion.
- Offsets 305886 and 306774: the loader iterates while downloaded records remain and only
  tallies `wk_dl_empty` for a zero-byte successful download. Accept that as zero records;
  a present zero-length record or a truncated frame remains an error.

## Repair

Split verified scopes into groups of at most 30 and process them sequentially under the existing
single-writer helper admission. This preserves the native wire boundary without requiring
multiple private ephemeral key files concurrently. Keep the same genuine identity key across
the operation, rotate the ephemeral key for each request, increment `ussidx`, and send the
one-time key only on the first request. Missing/corrupt retained identity is an error, never a
reason to silently generate a different identity halfway through a transfer.

Retain remote session IDs until the whole operation ends, then dispose them with bounded
concurrency and remove all local transfer key/session files. Counters are cumulative. A completed
intermediate request reports progress; only the last fully imported request reports overall
success. Cancellation keeps committed rows. Acknowledgement timeouts and disconnects after
queueing are typed as delivery-uncertain and must not automatically start legacy or a new phone
request. Once v2 has started, later errors also cannot fall back to another protocol.

Diagnostics contain only counts, stage/result flags and sanitized failures. The request status
file now records current/total request groups, total scopes, imported rows, overall completion,
cancellation and unacknowledged disposals. It is written privately and atomically. The optional
protocol-shape report contains type/sender-shape counts only, not message text or identities.

## Verification boundary

Offline tests cover 0/30/31/504 scope boundaries without losing or duplicating scopes, cumulative
counts, no intermediate success, post-queue timeout/disconnect classification, no unsafe legacy
fallback, cancelled-import wording, retained identity/fresh ephemeral keys, and missing/corrupt
identity rejection. A separate private real-wasm check confirmed identity reuse, three distinct
ephemeral keys, private/public key consistency and mode 0600. No account or phone request was
used by those crypto checks. A real full transfer remains required before release.

## First full batch-build trial

The first 30-scope request was directly acknowledged, two new rows were committed, and
the WebSocket remained authenticated. No further activity arrived within the existing
90-second inactivity bound, so the operation stopped at request 1/17 without legacy
fallback and removed the transfer keys. One disposal acknowledgement was missing. The
user confirmed Zalo remained open on the phone. This is partial import, not success.

The retained diagnostics from that build did not distinguish a missing file index from
a missing final marker. The next diagnostic build records bounded received/completed
batch indices, type/final/error flags, expected count, metadata readiness and last mobile
status in the existing private request report. The decoded shape report also records its
batch index/type/final flag. Neither includes identities, signed locations, keys or text.
This diagnostic addition does not change request, decryption or completion behaviour.

Further static comparison found an inbound branch omitted by our parser:
`pc-dist/main-startup` (`/private/tmp/ctdozlite-main-startup-reference.js`), offsets
2533634–2538450, defines `transfer_error`, routes it to `handleSyncError`, and forwards
`data.errorCode` to the matching session. Preserve that code as a failed batch control
even if no download URL/type/index is present. Reject malformed/zero-coded error controls
as a generic failure, never a successful empty batch. Tests cover explicit/string codes,
the legacy `err` field, and missing/zero codes. This is an inbound error-reporting fix;
it does not send additional requests or establish that this event caused the last timeout.
