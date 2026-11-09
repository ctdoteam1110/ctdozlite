# Keep startup refresh separate from explicit phone synchronization — 2026-11-08

Author: Codex. No account identifiers, message text or credentials are included.

## Evidence

After the receipt-recovery build was installed, the user reported a completion containing
498 Cloud/Group messages and explicitly no phone history. The private phone-request report
still had the preceding build's terminal timestamp, group 13 and 358 imported rows. No new
request was recorded. A process inventory found only the expected installed application.
This result does not establish that the recovery implementation was exercised.

Three automatic startup/control paths used `WorkerCmd::SyncPhoneMessages` with
`send_phone_prompt=false`. The same worker arm claimed `PHONE_SYNC_RUNNING` and emitted
`PhoneSyncProgress` / `PhoneSyncFinished`. Thus a background server refresh occupied the
phone button and replaced its result with the misleading Cloud-only completion. The manual
phone button does send `true`; asking the user to repeat it did not resolve that shared lane.

## Repair

Use an explicit `RefreshMessageHistory` command for those three automatic paths and retain
`SyncPhoneMessages` for the manual phone action. Give background refresh its own admission
flag and terminal event. It may update server-history data and the general status, but never
the phone progress/result or `PHONE_SYNC_RUNNING`. Its admission guard releases on task drop.

An explicit phone action has priority: the background operation yields between bounded API
calls, returning a typed preemption result. The phone job waits for that polling task to end
before sending a phone request, so an in-flight background poll cannot consume the new
session's controls. Cancellation and a 45-second handoff bound are respected. Pending
automatic refresh commands are skipped while a phone operation owns the lane.

A server-only refresh does not generate a phone RSA key, reset phone cancellation, send a
phone cancel RPC, or replace the phone outcome. Preempted background snapshots are not sent
back to the UI over the active phone result. Previously committed history remains intact.

## Verification boundary

Tests cover independent background/phone admission, guard cleanup, bounded handoff, stop,
timeout and typed background preemption. Command/event types enforce separate result paths.
Full regression, packaging and a manual fresh-start button/phone transfer check are required.
This UI/work scheduling repair does not claim the remaining 17-group transfer has completed.
