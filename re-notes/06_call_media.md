# Call Media Roadmap (Phase A)

## Status
- Signaling & ZRTP relay registration: working end-to-end (`401` → UDP `InitZRTP` `0x000B`/`0x000C` + `Echo` `0x0005` → `416` → `ring_ring status=0` → `answer status=0` → `408 answerack`), verified live 2026-10-28 (call `464564007`).
- ZRTP UDP keepalive: implemented and verified against live Zalo relay servers (`parse_zrtp_init_response` extracts `res=0`, `token`, and `"public_ip|public_port"`).
- Audio I/O: Phase 1 done — `src/audio.rs` provides `AudioHandle` (mic->speaker pass-through on a dedicated thread, Send-safe). Wired into call connect (AnswerCall accept + incoming "answer" status 0) and released on EndCall/timeout.
- Network media (`src/media.rs`, Opus/RTP over the call UDP socket): implemented and wired into `CallBridge`.

## Phase 2 plan
1. Encode captured mic PCM (16 kHz mono) with Opus (`opus` crate, 20 ms frames).
2. Packetize into RTP (payload type 111, standard Opus) and send to `rtp_ip` (from 401/416 response) over the existing UDP socket (or a new one bound to the relay port).
3. Receive RTP from relay, depacketize, Opus-decode, push to speaker via a jitter buffer.
4. Handle call teardown: send RTP BYE / ZRTP end packet, stop audio thread.
5. Add device selection + mute/unmute wiring to `SetMediaState` (12006 conf).

## Build notes
- `opus` crate builds `libopus` via CMake. On this machine CMake is too new; set
  `CMAKE_POLICY_VERSION_MINIMUM=3.5` before `cargo build`.
- `cpal` works on Windows (WASAPI) with default features.

## Files
- `src/audio.rs` — AudioHandle, device enumeration, pass-through.
- `src/call.rs` — PureRustCallState.audio field; audio started on connect.
- `src/bin/probe_audio.rs`, `src/bin/probe_call.rs` — dev diagnostics.