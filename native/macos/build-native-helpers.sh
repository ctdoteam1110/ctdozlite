#!/bin/bash
# Build host-architecture development helpers; never changes the installed app.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP="${1:-}"
for helper in emoji-preview media-preview daily-media-viewer daily-notifier; do
    bash "$ROOT/native/macos/build-$helper.sh" ${APP:+"$APP"}
done
# History uses pinned external codec assets and requires an explicit opt-in.
if [[ "${WITH_HISTORY_CODEC:-0}" == 1 ]]; then
    ASSETS="${HISTORY_CODEC_ASSETS:-$ROOT/native/macos/build/HistoryCodecAssets}"
    if [[ ! -f "$ASSETS/libzproto.wasm" ]]; then
        python3 "$ROOT/native/macos/prepare-history-assets.py" --output "$ASSETS"
    fi
    bash "$ROOT/native/macos/build-history-codec.sh" "$ASSETS" ${APP:+"$APP"}
fi
printf 'Host helpers built for %s\n' "$(uname -m)"
