# ADR-0005: Native call video and cancellable camera isolation

- Status: accepted for local implementation; release verification pending
- Date: 2026-11-08
- Deciders: user instruction to implement FFmpeg-free video

## Context

The v0.0.9 media pipeline launches FFmpeg for camera discovery, capture, preview
and incoming decoding. The initial Rust 1.88 feasibility probe failed because
OpenH264's required `wide` dependency requires Rust 1.89. Nokhwa's synchronous
driver reads do not expose portable cancellation. See
[the initial feasibility record](../../re-notes/38_native_video_feasibility.md).

## Decision

Keep edition 2021 and require Rust 1.89. Pin Nokhwa 0.10.11 with native input and
decoding, and OpenH264 0.9.9 with source linkage. Codec work runs in owned native
threads. Camera discovery and capture run in a private mode of the same
executable, using operating-system camera APIs. No external helper installation,
FFmpeg executable/library, runtime codec download, or dependency server is needed
for video. Killing and reaping the camera child interrupts a blocked driver read;
its pipe reader is joined before its owner exits.

Each call shares one camera permission flag, camera and encoder across its legs.
A joining participant inherits camera-off permission. Each outgoing leg has four
ordered access units; overflow waits for a newly encoded SPS/PPS/IDR. Raw capture
and rendered previews use replaceable latest-frame slots. Incoming streams have
separate assemblers, eight-unit decoder queues, parameter caches and native
decoders. Compressed units are bounded to 1 MiB and SPS dimensions are validated
before allocating native decoder frames. GUI delivery checks account and call
generations and coalesces at most seven stream keys.

Advertise H.264 capability only. Keep the existing relay, SRTP, camera-off
placeholder and Zalo/RFC 6184 framing. Protocol evidence and verification limits
are recorded in [re-notes/40](../../re-notes/40_native_video_integration.md).

## Consequences

- Source-built OpenH264 notices ship embedded in the executable, available in
  Settings and `--video-licenses`. Cisco binary-module royalty coverage is not
  assumed for this build. Distribution policy still needs release review.
- The child process adds pipe copies and process startup cost, but isolates
  blocking camera drivers without leaking threads or using unsafe cancellation.
- Windows compilation and automated synthetic tests can run locally. Real peer
  fixtures, camera hardware, latency percentiles, repeated calls, Linux and both
  macOS architectures remain release blockers until verified on those systems.
- Chat attachment playback and screen sharing remain outside this change.

## Alternatives Considered

| Option | Why rejected |
| :--- | :--- |
| Keep FFmpeg on PATH | Conflicts with the requested offline native video runtime. |
| Stop flag around synchronous Nokhwa reads | Cannot interrupt an already blocked native read. |
| Download Cisco codec binaries at runtime | Conflicts with offline operation and the selected source-linked codec. |
| Claim Rust 1.88 compatibility | Contradicted by the locked codec dependency graph. |
