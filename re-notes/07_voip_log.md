# 07 — ZaloCall.exe voip.log: decoder + native state machine (2026-10-28)

Evidence file: `%APPDATA%\ZaloData\cal\voip.log` (official native VoIP engine log).
Full decode: `%TEMP%\opencode\voip_decoded_final.txt` (TEMP is volatile — re-decode if gone).
Decoder: `scripts/decode_voip_log.py [--tail]` in this repo; node variant in
`%TEMP%\opencode\decode_voip.js`. Spans 2025-11-15 → 2026-10-27, 1873 lines.

## 1. The "cipher" [O]

Not a cipher — a **fixed 128-byte substitution table** baked into `ZaloCall.exe`
(x86, unpacked, `plugins\capture\ZaloCall.exe`):

- Log-init function at VA `0x7f8380` (file `0x3f7780`); only xref of the `\voip.log`
  string (VA `0x1235260`) is at VA `0x7f83b0`.
- Table built on the stack: 32 LE dwords at VA `0x7f86d1`–`0x7f87c4`, also copied to
  global `0x15e55e0`. Path arg (`...\ZaloData\cal`) cached in global `0x15b9024`.
- Encoding is `cipher = T[plain & 0x7F]`, hex-printed; log line = `<ts>\t<hex>`,
  ts format `%Y-%m-%d %X` (local). Multi-line events produce continuation hex lines
  **without** a timestamp.
- T is a bijection on 0..127 (verified). Dwords (bytes in order = T[0x00..0x7F]):
  `0x29354f63 0x162f1d42 0x101c0f65 0x55121750 0x2d222543 0x381b6d13 0x7e0b6e15 0x7b66184e`
  `0x5d522608 0x73370964 0x51726b14 0x190a2153 0x6c0e1a0d 0x5e3b0778 0x2028006f 0x4d7d1130`
  `0x5b543e31 0x48751e04 0x057a3a27 0x79583d23 0x68572c06 0x560c6003 0x2b770161 0x49703c34`
  `0x4167334c 0x765c7c1f 0x5a474639 0x407f0274 0x5f592a24 0x4b2e3f6a 0x36713269 0x4a454462`
- 2 MB rotation via `.old.` rename. Log has **no request bodies** — state events only [O].

## 2. Native caller sequence (official, observed 3×) [O]

```
cmd makecall → request make call 1-1 → [SIG]requestCall (plain/ui/devc/state/ui show)
→ cmd makeCall done → [SIG]receive 401 resp
→ makeCall isVideo = 0 → _initZrtcConfig 1..4 (+ congestionThread Start)
→ _initAudioDevice begin/end → startNoAudio → _handleStartAudio startAudio ret = 0
→ setCallState state = 1, curState = 6 → onMakeCall → _scheduleHandler success
→ _doInitZRTP size = 0 → _sendEchoRequestToServers startSeq = 0, endSeq = 120..160, timeout = 2000
→ _handleSelectServersTimeout size = 0 → _doInitZRTP size = 4
→ _handleInitZRTPSuccessful index = 0, size = 4 → onInitZrtpWithServer
→ [SIG]receive 416 resp (ui:stateChange 7) → [SIG]receive ring (ui:stateChange 8)
→ setCallState state = 3, curState = 1 → onCallState: 3
→ [SIG]receive answer → updateCallerInfo → state = 4, curState = 3 → onCallState: 4 (ui:stateChange 9)
→ state = 4, curState = 4 → state = 5, curState = 4 → "Call to confirmed. _handleStartCall"
→ onCallUpdateP2PStatus → onCallState: 5 → ui:stateChange 1 → onCallAudioState, 10  (media up)
```

Echo probe `endSeq` varied 120/140/160 across runs — server-count heuristic, not fixed [I].

## 3. Native callee sequences [O]

Ringing: `recv incoming call req 1-1` → `[SIG]recvIncomCall` → device list logs →
`ui:stateChange, 10` → `ZCallMainWindowVideo_v2::showCall` → `recvAliasName` →
`backgroundSize 452-695` → `ZCalleeWindow::showCall`.

Accept: `btnAcceptClick` → `sendActionLog: 1150311` → `sendAction: 5` →
`UiAnswerCallEvent, 1` → `ui:stateChange, 13` → `incomingCall isVideo = 0` →
`_initZrtcConfig 1..4` → `setCallState state = 2, curState = 6` → `_scheduleHandler success` →
`_doInitZRTP size = N` → …

Reject: `btnRejectClick` → `sendActionLog: 1150303` → `sendAction: 8` → `UiAnswerCallEvent, 0`.
Cancel while dialing (caller UI): `vidBtnEndcallClick` → `sendActionLog: 1150201` →
`UiEndCallEvent, 6`; end while ringing → `UiEndCallEvent, 8`.

**Our probe invite accepted twice on 2026-10-24 (00:32:37, 00:36:32)** — each time:
`_doInitZRTP size = 1` → teardown `endCall stopDevices = 0` → `disconnect 3,4,6,7` →
`onCallState: 6` within ~1 s. No echo probe, no `InitZRTPSuccessful` [O].

Healthy caller config always reaches `size = 4` (relay servers). Our invite yields
`size = 1` — matching our single private type-0 `p2p` entry vs official 7 entries incl.
a public type-1 IP. [I] `size` counts usable entries from the invite's `extendData`;
this is the current call-death point.

## 4. Log-noise decoder ring (do NOT misread) [O]

- `setCallState end` — exit-log of the state-set helper; appears after **every**
  `setCallState state = N`, even mid-successful-call. NOT an end-of-call marker.
- `ZErr:<pid> calle recv answer` — fires on the callee ~0–1 s into every incoming call,
  answered or not. Benign.
- `ZErr:<pid> Undefined recv cmd, 408` — native caller has **no answerack handler**; logged
  right after `Call to confirmed`. Call proceeds. Ours must not choke on 408 either.
- `ZErr:<pid> not dialing` — a 416/request arrived after the call record died (our probe's
  416 hit the desktop at 00:37:54 right after it cancelled its own call → proves our 416
  reaches the official engine).
- `ZErr:<pid> invalid state` — `recvAnsAck` while still ringing (caller-side stray ack).
- `ZErr:<pid> no any call` — endcall/recvEndCall with no live record.
- `no default vid inf` / `No default camera` — device enumeration noise.
- `showNoti <uid>` — missed-call notification ("Cuộc gọi thoại nhỡ") for uid.
- Real teardown marker = the 4-line run `disconnect 3 / 4 / 6 / 7` (+ `onCallState: 6`).

## 5. UI state codes seen [O]

`ui:stateChange`: 1 in-call, 2 hidden/reset, 6 outgoing, 7 dial-upgrade, 8 ringing,
9 answer-received, 10 incoming, 13 accepting. `onCallState`: 3 ringing, 4 answer, 5 confirmed,
6 torn-down. `setCallState` main states: 1 dialing, 2 callee-accepting, 3 ringing, 4 answer,
5 confirmed; `disconnect 3/4/6/7` teardown sub-states.

## 6. Unknowns [U]

- Healthy native callee `_doInitZRTP size` value (no successful callee session in this log).
- Official `402` request body (log carries no bodies; hook capture never saw one).
- Exact semantics of `size` (relay-server count vs p2p-entry count) — inference in §3.
