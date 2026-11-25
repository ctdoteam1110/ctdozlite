# Native video dependency probe

This standalone crate reproduces the approved native-video feasibility gate without modifying the application manifest, lockfile, or production code. Its library is intentionally empty: this is a dependency-build probe, not an implementation or a passing codec test suite.

Pinned direct dependencies:

- Nokhwa 0.10.11: defaults disabled; `input-native`, `decoding`.
- OpenH264 0.9.9: defaults disabled; `source`.

Reproduce from the repository root, after installing Rust 1.88.0:

```powershell
cargo +1.88.0 fetch --locked --manifest-path tools/native-video-feasibility/Cargo.toml
cargo +1.88.0 build --locked --offline --manifest-path tools/native-video-feasibility/Cargo.toml
```

**Expected result: failure.** The lockfile contains `wide 1.7.1` and `safe_arch 1.2.0`, which require Rust 1.89. OpenH264's minimum allowed `wide 1.1.1` also requires Rust 1.89. Raising the compiler or substituting dependencies requires a design revision; do not treat compilation on the default installed toolchain as proof of Rust 1.88 support.

No camera is opened and no live session, database, personal video, FFmpeg executable, or runtime download is needed to reproduce this failure. The fetch command requires network access for uncached build dependencies; the locked offline build does not.

See [the feasibility report](../../re-notes/38_native_video_feasibility.md) for baseline build results, cancellation concerns, and remaining hardware/fixture requirements. This records the original Rust 1.88 failure. The later local Rust 1.89 implementation and remaining release blockers are documented in [note 40](../../re-notes/40_native_video_integration.md).
