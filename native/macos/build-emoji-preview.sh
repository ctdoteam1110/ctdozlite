#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/native/macos/build"
mkdir -p "$OUT"
# Match each packaged Rust binary instead of shipping a host-only helper.
IFS=',' read -r -a TARGETS <<< "${SWIFT_TARGETS:-$(uname -m)-apple-macosx11.0}"
BINARIES=()
for target in "${TARGETS[@]}"; do
    binary="$OUT/EmojiPreview-$target"
    xcrun swiftc -O -target "$target" "$ROOT/native/macos/EmojiPreview.swift" \
        -framework CoreText -framework CoreGraphics -framework ImageIO -framework Foundation \
        -o "$binary"
    BINARIES+=("$binary")
done
if [[ "${#BINARIES[@]}" -gt 1 ]]; then
    lipo -create "${BINARIES[@]}" -output "$OUT/EmojiPreview"
else
    cp "${BINARIES[0]}" "$OUT/EmojiPreview"
fi
if [[ -n "${1:-}" ]]; then
    mkdir -p "$1/Contents/Resources"
    cp "$OUT/EmojiPreview" "$1/Contents/Resources/EmojiPreview"
fi
printf '%s\n' "$OUT/EmojiPreview"
