#!/usr/bin/env bash
# Cross-check, from Linux, that the Windows-only code (`#[cfg(windows)]`) compiles.
#
# Uses the x86_64-pc-windows-gnu target with MinGW-w64 so C dependencies (opus, sqlite, ring)
# can be compiled without the MSVC toolchain. Official Windows builds still use MSVC in CI;
# this script only catches compile errors early.
#
# Usage:
#   scripts/check-windows.sh            # cargo check (fast)
#   scripts/check-windows.sh --build    # cargo build --release (produces target/x86_64-pc-windows-gnu/release/zalo-rust.exe)
#   Extra arguments after the mode flag are passed to cargo.
set -euo pipefail

TARGET="x86_64-pc-windows-gnu"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MODE="check"
if [ "${1:-}" = "--build" ]; then
    MODE="build"
    shift
fi

if ! rustup target list --installed | grep -qx "$TARGET"; then
    echo "==> Installing Rust target $TARGET"
    rustup target add "$TARGET"
fi

if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    echo "==> Installing MinGW-w64 cross compiler"
    if command -v apt-get >/dev/null 2>&1; then
        SUDO=""
        if [ "$(id -u)" -ne 0 ]; then SUDO="sudo"; fi
        $SUDO apt-get install -y --no-install-recommends mingw-w64
    else
        echo "Please install MinGW-w64 (x86_64-w64-mingw32-gcc) with your package manager." >&2
        exit 1
    fi
fi

export CMAKE_POLICY_VERSION_MINIMUM="${CMAKE_POLICY_VERSION_MINIMUM:-3.5}"
export CC_x86_64_pc_windows_gnu="${CC_x86_64_pc_windows_gnu:-x86_64-w64-mingw32-gcc}"
export CXX_x86_64_pc_windows_gnu="${CXX_x86_64_pc_windows_gnu:-x86_64-w64-mingw32-g++}"
export AR_x86_64_pc_windows_gnu="${AR_x86_64_pc_windows_gnu:-x86_64-w64-mingw32-ar}"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER:-x86_64-w64-mingw32-gcc}"

if [ "$MODE" = "build" ]; then
    echo "==> cargo build --release --target $TARGET"
    cargo build --locked --release --target "$TARGET" --bin zalo-rust "$@"
    echo "==> Output: target/$TARGET/release/zalo-rust.exe"
else
    echo "==> cargo check --target $TARGET"
    cargo check --locked --target "$TARGET" --bin zalo-rust "$@"
fi
