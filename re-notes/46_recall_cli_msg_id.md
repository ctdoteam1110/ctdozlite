# Recall uses the message's client id — 2026-11-09

Author: Claude Code. Static inspection of the installed official Zalo PC 26.11.10 `app.asar` only;
no capture, account identifiers or keys recorded.

## Evidence

`MultiRecall.recall` (app.asar offset ~54559302) builds each recall item as
`{cliMsgId: <new id>, cliMsgIdUndo: e.cliMsgId, globalMsgId: normalize(e.msgId)}`:
`cliMsgIdUndo` is the recalled message's own client id.

## Change

`chat::recall_message` sent `cliMsgIdUndo = msg.ts` (server timestamp), which usually differs
from the client id. It now sends `msg.client_message_id` and falls back to `ts` only for cached
messages that never stored a client id (the previous behaviour). Endpoints, other params,
`ParamCipher` and signing are unchanged.

## Verification

Unit test `recall_tests::recall_targets_the_client_message_id_like_zalo_pc`. A live recall was
not performed by the assistant (it would act on a real conversation); the user verifies it
from the app.
