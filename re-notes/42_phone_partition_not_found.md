# Scoped phone history absence — 2026-11-07

Author: Codex. No account identifiers, signed URLs, keys or message text are included.

## Live evidence

The diagnostic build received direct v2 acknowledgement for request 1/17 (30 of 504
verified scopes). It then received message-batch failures at indices 2 and 12 with code
502, type 1, and a non-final flag. The last mobile status was Confirmed=4. This time the
new inbound error branch surfaced the failure immediately, instead of ignoring those
indices and waiting 90 seconds. No new rows were imported in this trial. The WebSocket
remained authenticated, no legacy request was sent, disposals were acknowledged, and
temporary transfer files were removed. The user had confirmed Zalo remained open in the
preceding full trial. The retained diagnostic deliberately contains no scope identities.

## Owning-source evidence

Installed official PC 26.11.10 public static sources:

- `default-login-main-startup-shared-worker-znotification.4bb5e1e2c2e9bab4f7b3.js`,
  module `o7eC` at offset 14305134: wire error `PartitionNotFound=502`.
- `/private/tmp/ctdozlite-sync-worker-reference.js`, module `HEuX` at offset 6112620:
  maps that result to `PARTITION_NOT_FOUND=1120`; offset 6122481 describes it as
  "Partition not found on mobile". Offset 6116286 places only that result in the
  separate non-retry absence category.
- The native task manager at offset 15713626 counts these outcomes separately in
  `rangePartitionNotFoundCount`, without treating them as exhausted transport/range
  failures. Its final report uses transport plus conversation failures for
  `has_failed_range` (`main-startup`, offset 2573323), keeping absent partitions explicit.
- Native session `handleResponse` (`shared-worker`, offsets 286000–287600) accounts
  for every received batch index, including failed scopes, and keeps the last marker
  as the expected count. Omitting error indices makes completion impossible.

These sources explain the error category and missing-index behavior. They do not prove
why a particular conversation is absent from this phone; do not assert deletion or loss.

## Repair and boundary

Retain the bounded scope list from each control privately. Only a message batch with
code 502, a nonempty well-formed scope list, and every scope matching the active request
may be counted as unavailable. Account for its index/final marker with zero inserted
rows; continue receiving the other files and requests. Count each absent partition once
and carry that count across request groups. Disclose it in the final UI and private report.

Metadata failures, unknown scopes, missing/malformed scope lists and other error codes
remain failures. Never invent an empty successful download or retry another protocol.
Existing history is preserved; absence on the phone never deletes a local conversation.

Tests cover continuation around a failed index, duplicate delivery, cumulative omission
counts, final disclosure, rejection of mixed/unknown/metadata/unscoped failures, bounded
wire scope parsing, and diagnostics excluding identity strings. A full real-device trial
is still required; offline checks alone do not establish a successful full transfer.
