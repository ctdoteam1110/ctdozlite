#!/usr/bin/env bash
set -euo pipefail

if [ "$(id -u)" -ne 0 ] && [ -f "/usr/local/bin/zalo-rust" ] && command -v sudo >/dev/null 2>&1; then
    exec sudo bash "$0" "$@"
fi

rm -f /usr/local/bin/zalo-rust /usr/local/bin/ctdozlite \
      /usr/local/share/applications/ctdozlite.desktop \
      /usr/local/share/icons/hicolor/256x256/apps/ctdozlite.png \
      "$HOME/.local/bin/zalo-rust" "$HOME/.local/bin/ctdozlite" \
      "$HOME/.local/share/applications/ctdozlite.desktop" \
      "$HOME/.local/share/icons/hicolor/256x256/apps/ctdozlite.png"

echo "Đã gỡ cài đặt ctdozlite."
