#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/native/macos/build"
mkdir -p "$OUT"
xcrun swiftc -O "$ROOT/native/macos/MediaPreview.swift" \
    -framework ImageIO -framework UniformTypeIdentifiers -o "$OUT/MediaPreview"
if [[ -n "${1:-}" ]]; then
    mkdir -p "$1/Contents/Resources"
    cp "$OUT/MediaPreview" "$1/Contents/Resources/MediaPreview"
fi
printf '%s\n' "$OUT/MediaPreview"
