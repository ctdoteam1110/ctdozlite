# Preserve a queued phone request across transient acknowledgement loss — 2026-11-08

Author: Codex. No account identities, signed locations, keys or message text are included.

## Captured result

The close-code build completed 12 of 17 groups, received exactly each completed group's
declared file count, and committed 358 new phone messages. The next request (group 13,
3407 encoded bytes) lost its acknowledgement when the WebSocket ended without a received
close frame. Cleanup subsequently reconnected and all disposals were acknowledged. This
does not identify the underlying network/server cause. The first two request sizes had
also matched in the independent read-only size preflight; do not assert a size-limit cause.

Previously the controller immediately ended the whole operation on that uncertain result,
disabled the active sync ID and deleted its keys. Matching phone controls arriving through
the normal reconnected socket or polling path could then no longer finish the request.

## Recovery boundary

Only an already queued request with the typed delivery-uncertain transport error may wait
up to 45 seconds for matching phone evidence. Retain that request's sync ID, keys and import
state. A status confirming/rejecting that exact session or a received batch is evidence
that the request reached the phone; key preparation or a different session is not evidence.
Keep actual RPC acknowledgement and phone-observed delivery as separate receipt kinds and
diagnostic fields. Recovered delivery is not an invented server acknowledgement.

Do not send a replacement frame. If no matching evidence arrives, return the original
uncertain error and finish cleanup without legacy fallback. Definitive server rejection,
malformed acknowledgement and unsent availability failures do not enter this recovery path.
Cancellation takes precedence. Before an unsent subsequent group, allow the existing socket
supervisor up to 45 seconds to authenticate again; this does not retry a queued request.

Final success still requires every file index of every group to finish import. Scoped 502
outcomes stay explicit. Preserve committed rows on interruption and remove transfer keys
only when the operation actually ends. Diagnostics count recovered deliveries separately.

## Verification

Offline tests distinguish real acknowledgement from later phone evidence, preserve the
original uncertain failure when no evidence arrives, avoid waiting on definitive/unsent
results, reject stale/prepared-only evidence, and cover delayed observation, timeout and
cancellation without network I/O. A real multi-group transfer remains required. This
recovery improves handling of the observed interruption; it does not claim to fix the
unidentified underlying disconnect or prove every uncertain request reached the phone.
