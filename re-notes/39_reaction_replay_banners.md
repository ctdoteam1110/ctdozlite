# Reaction replay on every connect, and the banner watermark

## Problem

Every WebSocket connect pulls each control queue with `lastId: 1`
(`socket::pull_frame`). The server answers with every reaction it still holds,
so the GUI showed the same OS banners again on every launch.

The replay is intentional for counting: `ReactionInbox` computes counts as
`base + distinct live events` because history snapshots are stale. For that
reason the fix gates banners and keeps the replay.

## Capture

Captured on 2026-11-15 with `ZALO_REACTION_TRACE=1`. The app was opened around
22:00. Uids are redacted and text has been removed.

The `610_1` pull answer:

```json
{"data":{"clearUnreads":[...],"controls":[],"lastActionId":"8492421889729","more":0,
 "queueStatus":{"610_1":{"evict":0,
   "ids":["8491736742795","8492232095093","8492232107943","8492232109138","8492421889729"],
   "lastId":"8492421889729"}},
 "reacts":[ ... 5 items ... ]}}
```

| `actionId` = `msgId` | `cliMsgId` | `ts` (ms) | `uidFrom` | `rType` |
| :--- | :--- | :--- | :--- | :--- |
| 8491736742795 | 1794742223145 | 1794742224142 (18:30) | `0` (self) | 3 👍 |
| 8492232095093 | 1794750559862 | 1794750560038 (20:49) | peer | 5 ❤️ |
| 8492232107943 | 1794750560180 | 1794750560261 | peer | 5 ❤️ |
| 8492232109138 | 1794750560093 | 1794750560188 | peer | 5 ❤️ |
| 8492421889729 | 1794753880355 | 1794753880382 (21:44) | peer | 5 ❤️ |

Findings from the table:

*   `msgId` equals the queue `actionId`, and it increases in server order.
*   Neither `ts` nor `cliMsgId` follows that order (rows 3 and 4).
*   Four banners per launch: the self reaction (`uidFrom: "0"`) is skipped.

## Saved cursor (tried and rejected)

Sending the stored `queueStatus["610_1"].lastId` back (`first: true`,
`lastId: "8492421889729"`) was accepted. The server replied with
`reacts: []`, `ids: []` and `evict: 0`.

This was rejected as the fix for these reasons:

*   It removes the replay that the counts rely on.
*   Passive sockets would advance the cursor past events they drop.
*   It has no recovery if the server rejects or evicts the cursor.

## Fix

`reactions::{banner_seq, BannerMark, load_banner_mark, save_banner_mark}`
and the `GuiEvent::ReactionEventsReceived` handler.

*   A banner is shown, and the reaction-unread count goes up, only for events
    that are not done: `msgId > mark`, or listed as pending.
*   A peer reaction whose target message is not loaded yet (replays usually
    arrive before history) gets no banner decision. It stays pending, so a
    later replay can still banner it. At most 256 pending ids are kept.
*   State is persisted in `accounts/<uin>/reaction_banner_mark`: line 1 is the
    mark, line 2 the pending ids.
*   An event without a numeric id is never suppressed.

Open question: whether live pushes (`cmd 601`) carry the same `msgId`.
