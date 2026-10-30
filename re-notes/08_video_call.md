# Video call — reverse notes (Zalo desktop)

Date: 2026-10-30. Method: static analysis only (Python + `pefile` + `capstone`, regex over JS).
No REA MCP in the session, so there are no REA Evidence IDs. No runtime capture of a real video call.
`ZaloCall.exe` has no PDB; function names below come from log strings only.

## Versions (read first)
| Artifact | Version | Note |
|---|---|---|
| `Zalo-26.10.11\plugins\capture\ZaloCall.exe` | 26.10.11 | MD5 `A2B920C00115BA11A7FCDCB642147E9C`, 19,587,624 B |
| `Zalo-26.10.20\plugins\capture\ZaloCall.exe` | 26.10.20 | MD5 `AAA59942E5D8B6BCDE09A8C9E29F26CD`, 19,588,136 B (PE32 x86) |
| `asar_extracted` (JS renderer) | 26.10.11 | `compact-app-pc.b7bec208c4c8f73e09f8.js` |
| this repo (`CALL_NATIVE_HASH`, UA) | 26.10.11 | |

Native findings below are from **26.10.20**; JS findings are from **26.10.11**.
Not checked: whether the video path differs between the two `ZaloCall.exe` builds.

## Observed

### Signaling (JS renderer 26.10.11)
- `12006` → `sendOnOffCamMic(calleeId, callId, isOnCam, isOnMic, data)` → `/api/voicecall/conf`,
  body `{calleeId, callId, isOnCam, isOnMic, data}`, AES-encoded `params`. `call.rs:700` matches.
- `11310` is `unholdresp` (signal `414`). The older report that said `12006 → requestaudio2video (11310)` was wrong (corrected in `zalo_call_videocall_re_report.md`).
- `/api/voicecall/requestaudio2video` is registered as `W(12056, ...)` in the endpoint table.
  The signal dispatcher (`switch(t){case 401:...}`) has **no** `case 12056`.
- Real signal → endpoint map (id from the `W(...)` table):
  401→requestcall 11300, 416→request 11301, 407→ringring 11302, 402→answer 11303,
  408→answerack 11304, 405→cancel 11305, 409→endcall 11306, 411→holdreq 11307,
  412→holdresp 11308, 413→unholdreq 11309, 414→unholdresp 11310, 406→logendcall 11311.
  `call.rs` comments already match this.

### `_parseExtendData` (ZaloCall.exe 26.10.20, `callcontroller.cpp`, string xrefs at `0x8d89d3..0x8d9370`)
Keys read: `callType`, `video` → `codec`, `newZrtc`, `packetMode` (default 2),
`gccMode`, `gccAudio`, `gccEarlyCall`, `gccSVLR`, `srtpMode`, `srtcp`, `sP2P`, `maxFT`,
`platform`, `fecTP`, `supportHevcDecode`, `tpType`, `spTcp`, `serverAddr`,
`select2side`, `extraInfo`, `serverResult`. Log: `_parseExtendData srtpMode = %d`.
- `numServers` string exists in the binary but was not seen inside this function.
- Codec entry keys: `name`, `payload`, `frmPtime`, `dynamicFptime`;
  log format `[codecName = %s, payload = %d, frameTime = %d, dynamicFPT = %d]`.
- Without `supportHevcDecode`: `partner using old version, not support hevc decode`.
  With it: `partner support hevc decode = %d`.
- All of these keys already appear in `extend_obj` (`call.rs:1754`).

### Audio → video upgrade in the engine (`peercallzrtpimpl.cpp`, `switchToVideoCall`)
Same call, no new call id. Log-string order:
`Begin switchToVideoCall(): startCamera=` → `Call to switchToVideoCall() while not in call !!!`
→ `You already in a Video Call !!!` → `Invalid zrtc config to switch to video call !!!`
→ `_audioDevice reinit ...` → `Failed to inti video coding & capture, ret=`.
Other strings: `switchToVideoCallAnswer`, `startCamera`, `acceptStartCamera`, `SwitchToVideo`,
`updateLocalCameraState`, `updatePartnerCameraState`, `onCallVideoState`, JSON field `video.cameraState`.

### Media engine (strings only)
- `zrtc_core` = WebRTC fork. Video codecs: H.264 (OpenH264, X264, OpenH264SVC), H.265/HEVC (FFmpeg, x265, `hevc_mediacodec`), VP8.
- `RtpPacketizerH264` with FU-A and STAP-A.
- RTCP/RTP: NACK, PLI (`Incoming PLI from SSRC`), FIR, REMB, transport feedback, RTX, ULPFEC/RED, `abs-send-time`.
- Keyframe: `keyFrameIntervalMs`, `requestKeyFrameRateMs`, `RequestKeyFrame`, stats `vidKeyFrameEncode/Decode`.
- Log lines `Use ZRTP packetization mode` (xref `0x8c83c6`), `Create VideoRtpRtcp: _callType=` (`0x8c4187`).

### This repo (source)
- `media.rs` (state before the live capture, kept for history): audio Opus PT 112 (16 kHz mono, 20 ms). Video = fixed SPS/PPS/IDR/P NALs, PT 97, ~15 fps,
  one NAL per RTP packet, no FU-A. Incoming PT != 112 was dropped. RTCP = simple RR every ~2 s. See "Live capture" and "Implemented" for the current state.
- `extendData`: `video.codec = [{name:"h264", payload:97}]`, `supportHevcDecode:1`, `srtpMode:0`, `srtcp:0`, `packetMode:2`.

## Inferred (not proven)
- Because `supportHevcDecode:1` is advertised but no HEVC decoder exists, the peer may send HEVC that we cannot decode.
- Missing PLI/FIR/NACK handling means a lost keyframe is never recovered.
- Fixed NALs larger than the MTU would be sent as one oversized datagram (no FU-A).
- The current blocker in `07_voip_log.md` (`_doInitZRTP size = 1` vs official `4`) is most likely in invite/`extendData`, not in video. Not compared field by field.
- `switchToVideoCall` + `acceptStartCamera` imply a two-sided handshake; the signaling carrier is probably `12006 conf` with `isOnCam=1`, but this is not confirmed.

## Unknown
- Real payload type for **VP8 / HEVC** (H.264 is `97`, plus `98` for fragments, from the live capture). The codec JSON is exchanged at runtime.
- Whether RTP header extensions are negotiated on the relay path. Every RTP packet (audio and video) carries one; see "Second capture" for the shape (probably a transport-wide sequence number, unproven).
- Exact meaning of `packetMode=2`, and whether `srtpMode=0` means plain RTP (the captured RTP/RTCP were readable, which supports it).
- Whether the relay/peer accepts 1200-byte payloads **from us** (the peer itself sends up to 1212; ours is untested).
- Contents of `data` in the `12006` body.
- Who calls `requestaudio2video` (`12056`), and whether it replaces `conf` for the upgrade.
- Whether 26.10.20 changes any of the above vs 26.10.11.

## Live capture (2026-10-30, `voip.log`, one outgoing video call, peer enabled camera for ~1-2 s)
**Observed** (339 log lines; peer = Zalo desktop/mobile, relay `171.244.128.51:4200`, 28 s call):
- Video payload type: peer advertises `video.codec=[{h264, payload:97}]` in `extendData`; actual RTP carries **PT 97 and PT 98**
  (PT 98 is not advertised anywhere). Video shares the audio SSRC (`0x1bba1962`) but has its own seq space; clock 90 kHz,
  ~30 fps (ts step 2970..3150). Packets carry an RTP header extension (`x=1`, 8 bytes; content not yet logged).
- Wire format is **not RFC 6184**. `PT 97` payload = one whole access unit as Annex-B (`00 00 00 01 21 ...`), marker=1.
  `PT 98` payload = `1c <flags> <Annex-B chunk>`: flags `80` first, `00` middle, `40` last, low 5 bits 0; the first chunk
  carried `00 00 00 01 27 42 ...` (SPS), 3 fragments (853/853/851 payload bytes) for the first frame, marker on the last.
- Largest datagram received: **874 B** = 1 relay + 12 RTP + 8 extension + 853 payload. Biggest P frame seen: 786 B, single packet.
  So the peer's fragment size is **853 B payload**. Only one fragmented frame was captured (fragment rules inferred from 3 packets).
- **PLI is real**: 56 PLI received in 28 s, all via the **RTCP socket** (none on the RTP socket), outer byte `0x0f`, compound
  `SR + SDES + PSFB/PLI`, one every ~506 ms (487..520 ms), always `media=` our video SSRC. No FIR seen.
  14 NACK seen, all for our **audio** SSRC (not handled yet).
- Relay-side extras: `UNK<` datagrams (`01 01 00 00 ... 62 19 ba 1b ...` 47 B and `02 01 ...` 25 B), about 1/s on both sockets with a counter; not decoded.
- Peer `extendData` for this call: `supportHevcDecode:4`, `packetMode:2`, `srtpMode:0`, `srtcp:0`, `negoVidQual:1`, `maxBitratePC:800`, `minBitratePC:150`.

**Inferred**
- The PLI storm continued at 2 Hz for the whole call although we answered each with an IDR, so the peer most likely could not
  decode our video (RFC 6184 framing, and/or the synthetic 18-byte IDR is not a valid picture). Not proven.
- `outer=0x05` seems to be audio RTCP and `0x0f` video RTCP (from a few samples).
- `type=1` of `onoff_camera` = camera on, `type=0` = off (video stopped right after `type=0`).

**Unknown**
- What the 8-byte RTP header extension carries; meaning of `supportHevcDecode:4`; whether a 1200-byte packet would be accepted
  (we now use 853); behaviour of `UNK<` control datagrams; whether PT 98 is also used for non-first fragments in larger frames than 3 packets.
  **Superseded by the second capture below** (853 is not the peer's limit; PT 98 is not "fragments of any frame").

## Second capture (2026-10-30 17:53:55-17:54:09 UTC, `voip.log`, relay `171.244.128.56:4200`, build with `video_format=annexb-au`)
User report (verbatim meaning): the call showed a poor-network indicator and the PC never displayed the peer's camera, while the phone showed its own picture.

**Observed**
- The peer **did send video**: 66 packets by the `STAT` at up=10s (64 x PT 97 at 12.8 pps, about 78 kbps, plus 2 x PT 98), `lost=0`. The first video packet is at 17:54:00.6 (5 s after media start).
- Fragmentation (peer -> us), same `1c <flags>` header as before, but:
  - first frame (SPS+PPS+SEI+IDR, `annexb[7,8,6,5]`): **PT 98**, 2 fragments, 1083 + 1083 payload bytes, flags `80` / `40`;
  - a **P frame** (`annexb[1]`): **PT 97**, 3 fragments of 1062 bytes each, flags `80` / `00` / `40`, marker on the last only;
  - largest unfragmented payload: **1212** bytes (frame of 1 packet, `wire=1233`).
  So the first capture's 853 B limit was specific to that call's path (first call: 853/853/851), and fragments are cut **evenly** (`ceil(len/~1200)` parts). PT 98 has only ever been seen on the first keyframe's fragments (2 captures); "PT 98 = keyframe" is an inference.
- RTP header extension, one-byte form: `be de 00 01 | 51 XX XX 00` (ID 5, 2 data bytes, 1 pad byte). The 2-byte value counts up (audio `0001, 0002, 0003, ...`; video `0109, 010d, 010f, 0112, ...`).
- PLI: 25 in 12 s (17:53:55.8 .. 17:54:07.9), all via `rtcp-sock`, about every 500 ms, `media=0x19f5f926` = our video SSRC. PLI started 0.8 s after media start, before we had sent anything but the synthetic frames. NACK: 10, all for our **audio** SSRC `0x19f5f925`.
- Our RR was all zeros (`RR(... head=81c90007 19f5f925 1bba1962 0000...)`): we reported no reception, `fraction lost=0`, `highest seq=0`.
- The earlier call in the same log (17:42-17:43, **peer camera off**, `onoff_camera type=0`): no peer video at all, yet PLI began ~27 s into the call (22 PLI, ~500 ms apart) and this time with `media=0x1bba1962` (the peer's own SSRC). Not explained.

**Inferred (unproven)**
- The header extension is a transport-wide sequence number (TWCC; `gccMode:1` is advertised). We send no extension and no feedback, which may be what the peer reports as poor network.
- The PLI stream at 2 Hz means the peer cannot decode our video. Our "IDR" is 18 bytes of invented data, so no encoder-side change can make PLI stop before a real encoder exists.
- Why the PC showed no picture: the decoder is a headless `ffmpeg` pipe (`-f h264 -i pipe:0`); with default probing it needs ~3.8 s of stream before the first frame (measured on a synthetic 15 fps stream, 3.82 s vs 1.01 s with `-probesize 32 -analyzeduration 0`), and the peer's video lasted only a few seconds. The old code logged nothing about the decoder, so **whether any frame was decoded is unknown**.

**Unknown**: whether `ffmpeg` produced any frame in that call; meaning of the 2-byte extension value; why PLI started late in the camera-off call.

## Changes after the second capture (built, `cargo test` 90 passed, `cargo check --all-targets` clean; **not yet tried on a live call**)
- `media.rs`: `RTP_MAX_PAYLOAD` 853 -> **1200**; fragments are split evenly; fragments use PT 98 only when the access unit contains an IDR slice (`au_has_idr`), PT 97 otherwise.
- `media.rs`: the `ffmpeg` decoder gets `-probesize 32 -analyzeduration 0` and now logs to `voip.log` with tag `VDEC` (started/spawn failure, first and every 100th access unit fed, first and every 100th frame decoded, up to 20 stderr chunks from `ffmpeg`, stop with frame count).
- `media.rs`/`voip_log.rs`: RTCP is now real. Audio: RR with a report block about the peer's audio SSRC (cumulative lost, highest seq, LSR/DLSR; jitter left 0). Video: SR + empty SDES from our video SSRC (52 + 12 bytes, same shape as the peer's), carrying a report block for the peer's video. `describe_rtcp` prints `blk[ssrc,fl,lost,hi,jit,lsr,dlsr]` for SR/RR, so the next log shows how the **peer** sees us.
- Still not done: the header extension and TWCC feedback (only a hypothesis), NACK handling, real camera + encoder.

## Third capture (2026-10-30 18:28:55-18:29:30 UTC, relay `171.244.128.60:4200`, build with VDEC/RR/SR, peer camera on, phone said "PC connection unstable")
**Observed**
- `VDEC`: ffmpeg started, 1 AU fed, `non-existing PPS 0 referenced` on every packet, **`decoder stop, 0 frames decoded`**. The peer sent **only P frames** (`annexb[1]`, `zfrag/S/annexb[1]`) in this call: no SPS (7), PPS (8) or IDR (5) AU and no PT 98 ever reached us (first packet seen seq 31709 was already a P frame; `lost=7` over the call). We sent **no PLI** back, so the peer had no reason to send a keyframe.
- Peer's reports about **our** streams (new `blk[...]` output): audio SSRC `0x19f5f925` last `lost=1296 hi=1661` (about 78% lost), video SSRC `0x19f5f926` last `lost=370 hi=474` (about 78%). The loss comes in bursts (fraction `fl=250..253` for a few seconds, then `fl=0`) and hits audio and video together. `hi` keeps rising at the expected rate (~47 pps audio), so the peer sees packets arriving but most are missing. The short audio-only call (18:35, 6 s) shows the same: 30 packets fine, then 100/101 lost, etc.
- In the peer's report blocks `lsr=0x0 dlsr=0` throughout, i.e. the peer never echoed a Sender Report of ours (our video SR is sent every 2 s on the RTCP socket with relay type `0x0D`). Meaning unknown.
- PLI from the peer continued every ~500 ms for the whole call (about 60 packets), same as before.
- `UNK<` relay datagrams (`01 01 00 00 ...`) 40 of them, 1/s, always 47 B (29 B twice at the start). Not decoded.

**Inferred**: the black picture on the PC is explained by "no keyframe reached ffmpeg" (observed), not by ffmpeg timing. Why the peer sent no keyframe is unknown (it may have sent one before our media socket was reading; we never asked).
**Unknown**: whether the ~78% loss of our packets is a local send failure (old code dropped every `try_send_to` result with `let _ =`), relay/NAT behaviour, or a rejected packet format. This is the cause of the phone's "unstable connection" message only as far as loss and message coincide.

## Changes after the third capture (`cargo test` 93 passed, `cargo check --all-targets` clean; **not yet tried on a live call**)
- `media.rs`: ffmpeg is not fed until the first access unit with SPS/IDR (`au_is_sync_point`); earlier P frames are dropped and counted in `VDEC`.
- `media.rs`: we now send PLI (compound RR + PSFB/PLI, relay type `0x0D`/`0x0F`) to the peer's SSRC at most every 500 ms while the decoder has no SPS/IDR yet or a sequence gap was seen in the peer's video (`build_pli`, `RxStats::update` now returns "gap").
- `media.rs`: every UDP send goes through `send_dgram`, which counts ok / `WouldBlock` / short / error; new log tag **`TXST`** every 5 s: `sent_ok`, `would_block`, `short`, `err`, `last_err`, `max_loop_gap_ms` (longest stall of the media loop), `pli_sent`, `video_synced`. Behaviour of the send itself is unchanged: this only measures, to see whether the loss is local.


Verified only by `cargo test` (86 passed) and `cargo check --all-targets`. The first block below was built before the live capture; **the Zalo wire format change at the end has not been tried on a live call yet**.
- `media.rs`: outgoing H.264 now goes through `packetize_h264_nal` (single NAL if <= 1200 bytes, FU-A otherwise);
  marker bit is set on the last packet of the access unit only (before, it was set on the last NAL of each group).
  **Superseded by the live capture** (see "Zalo wire format" below); `packetize_h264_nal` is now `#[cfg(test)]` only.
- `media.rs`: incoming RTCP (PT 200..=207) is no longer fed to the video depacketizer; PLI (PSFB fmt 1) and
  FIR (PSFB fmt 4) force an immediate SPS+PPS+IDR (`rtcp_requests_keyframe`).
- RTCP socket path: the keepalive task in `call.rs` is the only reader of `udp_rtcp_sock`. Anything on it that is not a
  ZRTP init reply is now forwarded via `media::push_rtcp_inbox` (bounded 64, global, single-call assumption) and drained by
  `run_media_loop` (`wire_requests_keyframe` strips the relay header first). The RTP socket path is kept too.
  Whether the relay really sends PLI/FIR on the RTCP socket: **confirmed** by the live capture above (56/56 via the RTCP socket).
- `call.rs`: `supportHevcDecode` changed `1 -> 0` in both invite and answer `extendData`, because there is no HEVC decoder.
  Effect on the server/peer is **unknown**; revert if invites start failing.
- `voip_log.rs` (new): wire log for capturing a real call, plain text in `./voip.log` (override `ZALO_VOIP_LOG=<path>`, `off` disables; rotates at 2 MiB; git-ignored via `*.log`).
  It is NOT the encoded `%APPDATA%\ZaloData\cal\voip.log` and `decode_voip_log.py` does not read it. Tags: `CALL` (start/stop, our PT/MTU constants),
  `SIG` (remote `codec` + `extendData` + `rtpIP` from every control event), `RTP>`/`RTP<` (first 32 per direction+PT, then 1 in 200: PT, marker, seq, ts, ssrc, CC/X, payload size, NAL/FU-A kind, 16 B head),
  `RTCP>`/`RTCP<` (every packet: `via=rtp-sock|rtcp-sock`, block names PLI/FIR/NACK/REMB/TWCC, `kf_req`), `UNK<` (datagrams the relay unwrapper rejects),
  `STAT` every 5 s per stream (pps, kbps, min/max wire size, seq gaps = loss, marker count, NAL histogram) plus `mtu_hint` = largest datagram seen.
  How to read the answers after a call: payload type = `pt=` on `RTP<` lines with FU-A/nal5 + `SIG ... codec=`; MTU = `wire[max=]` / `wire_rx_max` minus the relay header (1 or 5 bytes; `RTP<` shows `outer=`) minus 12;
  PLI/FIR = `grep kf_req=1` and its `via=`. Only built and unit-tested; the capture above was made with this log.
- Tests added in `media.rs`: FU-A round trip through the existing depacketizer, single-NAL case, PLI/FIR detection (NACK ignored), marker bit, PLI behind relay header, bounded RTCP inbox.
- **Zalo wire format (after the live capture, untested live):** `media.rs` now sends one Annex-B access unit per frame:
  `PT 97` if it fits `RTP_MAX_PAYLOAD` (**853 here; changed to 1200 after the second capture, see above**), otherwise `PT 98` fragments `1c <80|00|40> <chunk>` (`packetize_zalo_au`); marker on the
  last packet. Receive (`depacketize_h264`) dispatches by content: Annex-B access unit, Zalo fragment (lost first fragment drops the
  access unit), else the old RFC 6184 path. PT 98 is now also routed to the video decoder. `voip_log.rs` describes `annexb[..]` /
  `zfrag/S|M|E` and logs the first 16 bytes of the RTP header extension (`ext=`). Tests: 5 new in `media.rs` (fragment round trip, captured
  shapes, lost first fragment, RFC 6184 fallback, small AU), 1 extended in `voip_log.rs`.
  Whether the peer now decodes our video (PLI stops) is the thing to check with the next capture.

## Fourth capture (2026-10-30 19:03:26-19:03:39 UTC, relay `171.244.25.111:4200`, build with TXST/PLI)
**Observed**
- `TXST`: `sent_ok` 329 -> 656, `would_block=0 short=0 err=0`, `max_loop_gap_ms` 8-9, `pli_sent=1`, `video_synced=true`. Local UDP sends did not fail and the media loop did not stall.
- Peer report about our streams: audio `lost=2 hi=503`, video `lost=0 hi=181` (about 0% loss; the third capture had ~78%). Cause of the difference is **unknown** (different relay, or the changes; not proven).
- Our PLI at 19:03:27.886 was followed at once by a keyframe from the peer: PT 98 seq 23404-23405, `zfrag/S/annexb[7,8,6,5]`, 961+961 bytes (AU 1918 B), then P frames. So PLI works.
- The peer sent only 22 video packets (seq 23404-23425) in ~0.55 s (19:03:27.885-19:03:28.436), then stopped (`onoff_camera type=0`, peer camera off). Its own PLI to us continued (26 packets until 19:03:39).
- `VDEC`: `first SPS/IDR access unit after 0 undecodable ones`, `fed 1 access units`, then `decoder stop, 0 frames decoded`.

**Measured offline (synthetic libx264 baseline 320x240 15 fps stream, AUs pushed through a pipe 30 ms apart)**
- Old args (`-fflags nobuffer -flags low_delay -probesize 32 -analyzeduration 0`): first frame only at AU #33 (~1.0 s); with 22 AUs: **0 frames**. That matches the live result (peer sent ~22 AUs).
- No variant helped (`-threads 1`, `-fpsprobesize 0`, `-flush_packets 1`, `-fps_mode passthrough`, `-max_delay 0`, `-avioflags direct`, `-probesize 2048/4096`, AUD after AU) except: **drop `-fflags nobuffer` and write an AUD (`00 00 00 01 09 f0`) before every access unit** -> first frame at AU #2 (~0.07 s), 7/8, 11/12, 21/22 frames.

**Inferred**: the 0-frame result of the live call was ffmpeg's pipe latency, not packet loss or a bad keyframe.

## Changes after the fourth capture (`cargo check --all-targets` clean; **not yet tried on a live call**)
- `media.rs` `run_h264_ffmpeg_decoder`: removed `-fflags nobuffer`; every access unit is preceded by `H264_AUD`.


## Fifth capture (2026-10-30 19:15:43-19:15:58 UTC, relay `171.244.128.58:4200`) + Android `libzrtc.so` (26.10.02) root-cause fixes
**Observed in `voip.log` (lines 2248-2500)**
- `VDEC`: `ffmpeg decoder started` at `19:15:42.883`, then at `19:15:50.810` (`seq=1021`): `VDEC waiting for SPS/IDR, dropped 1 P access units`, and at call end: `Output file does not contain any stream` / `VDEC decoder stop, 0 frames decoded`.
- Yet `WireLog` `STAT` at `19:15:53.284` recorded **all** peer packets with **`lost=0`**:
  - Audio `<pt112`: `pkts=477 seq=21430..21906 lost=0`
  - Video `<pt98`: `pkts=2 seq=989..990 lost=0 zfrag/S/annexb[7,8,6,5] + zfrag/E` (keyframe arrived at `19:15:49.347`–`19:15:49.375`!)
  - Video `<pt97`: `pkts=59 seq=991..1049 lost=0`
- Meanwhile our own `rx_audio` RR reported `lost=341 hi=21837` (71% missing!) and `rx_video` SR reported `lost=53 hi=1087`, and the peer's SR reported bogus SSRCs `0x81ce0002` and `0xee8f6b61..0xee8f6b6d`.

**Root causes (proven from `media.rs`, `call.rs`, and ARM64 disassembly of `libzrtc.so` in `Zalo_26.10.02_APKPure.xapk`)**
1. **Incoming RTP on `rtcp_sock` was logged to `voip.log` and then dropped (`media.rs:934-937`, `call.rs:1399,1460,1507`)**:
   - On single-port relays (`rtp == rtcp == 171.244.128.58:4200`), `call.rs` sent `pkt_init` with `sub_cmd=2` (instead of `sub_cmd | 4 = 6` per `ZRTPPacket::initZRTPPacketRequestInitCall @ 0x4ad248`) and `pkt_p` (`cmd=2` ping) from `rtcp_sock` right after `rtp_sock`, causing the relay to bind our client endpoint to `rtcp_sock`'s local port.
   - `rtcp_sock` received 71% of incoming audio and almost all incoming video (including `seq=989..990` `PT 98` keyframe and `seq=991..1020`), pushed them to `RTCP_INBOX`, and `media.rs:936` called `wl.rtp("<", ...)` **without** updating `rx_audio`/`rx_video` or feeding Opus / `depacketize_h264`.
2. **Outgoing RTCP used `outer=0x0D` (Video RTP) instead of `0x05` / `0x0F` (`media.rs:814,857,907,1029`)**:
   - In `libzrtc.so` (`ZRTPPacket::initZRTPPacketAudio @ 0x4ad63c`, `initZRTPPacketVideo @ 0x4ad704`, `_buildPacketInternal @ 0x4adca0` bitmask `0xa0a8`), relay type bytes depend on `isRtcp`, **never** on `isCaller`: client always sends `0x03` = Audio RTP, `0x05` = Audio RTCP, `0x0D` = Video RTP, `0x0F` = Video RTCP.
   - Sending RTCP SR and RR+PLI with `0x0D` caused the relay to forward our RTCP packets to the peer as Video RTP (`0x0E`); the peer's RTP parser read bytes `[8..12]` of our SR (`0xee8f6b65` NTP timestamp) and RR+PLI (`0x81ce0002` PLI header) as RTP SSRCs, and never received our PLIs or SRs as RTCP (`lsr=0x0` in third/fifth captures).
3. **Unfragmented H.264 keyframes (`au.len() <= max_payload`) were sent with `PT 97` instead of `PT 98` (`media.rs:271-274`)**:
   - In `libzrtc.so`, `RTPSender::SendOutgoingData @ 0x727948` (`0x727bbc..0x727c40`) and `RTPReceiverVideo::ParseRtpPacket @ 0x7185c0` (`0x71871c..0x7187f0`) map H.264 `kVideoFrameKey (3) <-> PT 98 (0x62)` and `kVideoFrameDelta (4) <-> PT 97 (0x61)` unconditionally (and H.265 Key/Delta `<-> PT 121/122`, SVC Key/Delta `<-> PT 126/125`). Any H.264 packet received with `PT 97` has `rtp_header->frameType` overwritten to `4` (`DeltaFrame`), even if it contains SPS+PPS+IDR.
4. **Other `libzrtc.so` confirmations**:
   - RTP Header Extension ID `5` = `kRtpTransportSequenceNumberHeaderExtensionDefaultId @ 0x360c88` (TWCC).
   - `supportHevcDecode: 4` (or `1`) in `Peer::_handlePartnerHevcDecCapacityChange @ 0x509bf4` triggers `Peer::_changeVidCodec(5 /* H265 */)` when hardware HEVC encoding is supported.
   - `UNK<` `01 01 ...` (47 B) and `02 01 ...` (25 B) are `ZRTPPacket` control request/response (`cmd=0x0020` `RequestForward`, `subCmd=0x0a` `VideoControl` / `0x14` `IncallNegotiateInfo`, 21-byte header `[01, 01, seq:u32, subSeq:u32, callId:u32, token:u32, cmd:u16, subCmd:u8]` + `[len:u32, payload]`).

## Changes after the fifth capture (`cargo test` 186 passed)
- `media.rs`: merged `drain_rtcp_inbox()` and `udp_rtp_sock` receive paths so RTP packets arriving on either socket update `rx_audio`/`rx_video`, decode Opus, and feed `depacketize_h264`; increased `RTCP_INBOX_MAX` `64 -> 512` and per-tick RTP drain `8 -> 64`.
- `media.rs`: fixed outgoing relay type bytes (`0x03` Audio RTP, `0x05` Audio RTCP, `0x0D` Video RTP, `0x0F` Video RTCP/PLI).
- `media.rs`: `packetize_zalo_au` now uses `PT 98` (`VIDEO_PT_FRAG`) for all IDR access units (including single-packet keyframes) and `PT 97` for non-IDR access units.
- `call.rs`: `rtcp_sock` InitZRTP uses `sub_cmd | 4` (`6`), and keepalive `cmd=2` pings are not duplicated from `rtcp_sock` when `rtcp_addr == rtp_addr`.
## Still open
1. Next call (after the fourth-capture build): read `VDEC` (`decoded frame #1` appears?) and confirm the picture shows on the PC. Keep the peer's camera on for 30 s. Also check whether ~0% loss repeats.
2. If the poor-network indicator remains: add the RTP header extension (ID 5, 2-byte counter) and TWCC feedback (RTCP PT 205 fmt 15); logging already shows `TWCC` if the peer sends it.
3. Decode the `UNK<` relay datagrams (`01 01` / `02 01`).
4. Handle NACK (10-14 per call, all for the audio SSRC) and send PLI when incoming video frames are lost.
5. Real camera capture + encoder instead of the synthetic NALs (the stream is still fixed SPS/PPS/IDR/P content; the 18-byte IDR is probably why the peer cannot decode it and keeps sending PLI).
6. Audio -> video upgrade on an existing call via `12006` (not implemented; carrier unconfirmed).
7. Re-check `CALL_NATIVE_HASH` and the UA against 26.10.20 if the server validates versions.
