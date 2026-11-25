# Native call-video integration

Date: 2026-11-08. Baseline: exact v0.0.9 release commit
`d1f954119aa94eebc309dd525923c3b127de22ad`.

## Scope and evidence

This implements the subsequent request to continue with FFmpeg-free video.
The initial failed gate in [note 38](38_native_video_feasibility.md) remains a
historical record; its statements about unchanged production files describe
that earlier phase. The local implementation uses Rust 1.89 and the
[same-executable camera adapter](../docs/adr/0005-native-call-video.md).

Transport hosts, signing, SRTP, relay wrappers, payload types 97/98 and the
primary-plus-five stream limit are retained. [Note 08](08_video_call.md)
documents the observed `1c 80`, `1c 00`, `1c 40` Annex-B fragments and RFC 6184
receive support. The replacement assembler checks sequence wrap, timestamps,
marker/completeness and the 1 MiB bound before queueing complete units.

Both invite and answer now advertise `supportHevcDecode: 0`: the bundled codec
is H.264 only. Note 08 already identifies false HEVC capability advertisement
as a possible cause of undecodable peer video (and documents the prior
`1 -> 0` change). The codec list remains H.264; this is an explicit capability
correction, not a new packet format. Automated tests check the capability field.

Synthetic tests exercise native encode/decode, separate SPS/PPS, IDR generation,
SPS bounds, malformed and lost fragments, wraparound, outgoing/incoming overflow,
multiple outgoing legs, account/call generations and controlled blocked-helper
shutdown. A module integration test passes newly encoded native H.264 through
the existing Zalo packetizer, replacement ingress and decoder, drops a fragment,
then recovers using newly generated SPS/PPS/IDR.

## Native build tools

Windows: MSVC C/C++ build tools and CMake (including Opus); NASM is optional for
OpenH264 assembly optimization. Linux: C/C++, CMake, pkg-config, Clang/libclang
for V4L2 bindings, plus the existing GUI/audio development packages. macOS:
Xcode command-line tools and CMake. Keep `CMAKE_POLICY_VERSION_MINIMUM=3.5`
when using CMake 4.x. No build tools are required on end-user machines.

## Verification limits

No sanitized peer-video fixture is available in this repository. Synthetic
codec/framing tests do not prove real Zalo interoperability. Physical permission
denial, switching/disconnection, 20 repeated calls, direct/relay/group calls,
p95 preview/display latency and the two-second hardware shutdown target remain
unverified. Linux and both macOS architectures must be built and exercised on
their actual release systems. The existing macOS plist has camera/microphone
purpose strings, but actual permission attribution to the helper is unverified.

Use `--video-self-test` to exercise the embedded codec without credentials,
network or camera. `--native-camera-list` exercises the private discovery
dispatch without opening a capture stream. These are local checks, not evidence
that the outstanding hardware and peer compatibility gate has passed.

The 260 kbps encoder configuration is a target: with frame skipping disabled,
complex scenes may exceed it. No measured bandwidth or latency claim is made.
All new notices are embedded and available through Settings or
`--video-licenses`; source-built OpenH264 is not claimed to receive Cisco's
binary-module royalty coverage.

## Local results (Windows x86_64 MSVC)

- Rust 1.89.0: `cargo test --locked` passed 330 tests, with one existing live
  signalling test ignored. After final supervisor/lint edits, all 37 focused
  `video::` tests passed again. The Zalo native packetizer/loss/recovery module
  integration test also passed.
- `cargo build --release --locked --bin ctdozlite` completed successfully using
  the same Rust 1.89 toolchain. The resolved graph has 520 packages; no package
  declares an MSRV above 1.89. This does not replace other-platform builds.
- Release executable `--video-self-test` with PATH limited to
  `C:\Windows\System32`: 10 encoded and 10 decoded 320x240 frames, 646 compressed
  bytes, 24 ms total in this synthetic run. This is not call latency evidence.
- The same restricted-PATH executable returned valid JSON with one device from
  `--native-camera-list`, and `--version` exited successfully. No capture stream
  was opened. This checks the real executable's helper dispatch.
- MSVC `dumpbin /dependents` lists Windows Media Foundation and system/CRT
  libraries, with no FFmpeg, OpenH264 DLL or external Opus DLL dependency.
- Formatting checks pass for all native-video source files; `git diff --check`
  passes. Repository-wide `cargo fmt --all -- --check` still flags existing
  formatting in unchanged files, including `build.rs` and
  `tests/message_identity_integration.rs`.
- Strict Clippy was run with Rust 1.89 and is not clean: the repository contains
  existing lint failures in call/chat/GUI/store/transport code. No lint error was
  reported in `src/video/`; new device-refresh/stream-type lint findings were
  corrected. This is not a passing global lint result.
- `cargo audit --json` reports `RUSTSEC-2023-0071` in `rsa 0.9.10` and
  `RUSTSEC-2026-0384` in `tokio 1.54.0`, plus maintenance/unsound/yanked warnings.
  Both versions already exist in the v0.0.9 lockfile; the video implementation
  does not update them. Dependency auditing is therefore not a clean release
  gate.

Evidence logs are local build artifacts under `target/native-*.log`,
`target/native-dependency-audit.json`, `target/native-executable-smoke.json` and
`target/native-executable-dependents.txt`. No commit, push or publication was made.

## Recovery after a lost access unit (2026-11-10)

The receive path resets its decoder queue after sequence loss or an incomplete
unit's timestamp change, then retries the triggering packet once with a clean
assembler. That packet may be the first fragment of the recovery IDR. Malformed
continuation fragments still fail validation. The wire formats remain those
documented in re-notes/08. The synthetic regression
`first_recovery_unit_survives_sequence_loss_and_timestamp_change` checks both a
sequence gap and an in-order timestamp boundary, using the observed Zalo fragment
headers and ensuring that no bytes from the damaged unit reach the next unit.

Slice-header inspection now unescapes at most 64 RBSP bytes; compressed slice
data stays with the native decoder. A terminal `00 00 03` is accepted for CABAC
padding, as described by H.264 (02/2016), clause 7.4.2.10
([ITU recommendation](https://www.itu.int/rec/T-REC-H.264-201602-S/en)). The
`slice_header_unescape_ignores_coded_data_and_accepts_terminal_prevention` test
checks terminal prevention, rejects invalid prevention sequences, and confirms
that slice data beyond the bounded header is not parsed as header syntax.

PLI targets are selected independently for the primary and extra SSRCs. An
unsynchronized extra stream does not request an IDR from a healthy primary.
`extra_stream_recovery_does_not_request_healthy_primary_keyframes` checks this
selection, inactive streams and deduplication. RTP/RTCP packet layouts and the
existing 500 ms PLI interval are unchanged.
