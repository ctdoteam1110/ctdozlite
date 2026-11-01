#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/native/macos/build"
mkdir -p "$OUT"
xcrun swiftc -O "$ROOT/native/macos/DailyMediaViewer.swift" \
    -framework AppKit -framework AVKit -framework AVFoundation \
    -framework ImageIO -framework UniformTypeIdentifiers -o "$OUT/DailyMediaViewer"
if [[ -n "${1:-}" ]]; then
    mkdir -p "$1/Contents/Resources"
    cp "$OUT/DailyMediaViewer" "$1/Contents/Resources/DailyMediaViewer"
fi
printf '%s\n' "$OUT/DailyMediaViewer"
