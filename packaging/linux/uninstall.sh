#!/usr/bin/env bash
# Remove QTzalo binaries and menu entries.
# User data (${XDG_DATA_HOME:-~/.local/share}/qtzalo) is kept unless --purge is given.
set -euo pipefail

PURGE="${1:-}"
SYSTEM_FILES=(
    /usr/local/bin/qtzalo
    /usr/local/share/applications/qtzalo.desktop
    /usr/local/share/icons/hicolor/256x256/apps/qtzalo.png
)

if [ -f /usr/local/bin/qtzalo ]; then
    if [ "$(id -u)" -eq 0 ]; then
        rm -f "${SYSTEM_FILES[@]}"
    elif command -v sudo >/dev/null 2>&1; then
        sudo rm -f "${SYSTEM_FILES[@]}"
    else
        echo "Cần quyền root để gỡ các file trong /usr/local." >&2
    fi
fi

rm -f "$HOME/.local/bin/qtzalo" \
      "$HOME/.local/share/applications/qtzalo.desktop" \
      "$HOME/.local/share/icons/hicolor/256x256/apps/qtzalo.png"

if [ "$PURGE" = "--purge" ]; then
    DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/qtzalo"
    rm -rf -- "$DATA_DIR"
    echo "Đã xoá dữ liệu người dùng tại $DATA_DIR"
fi

echo "Đã gỡ cài đặt QTzalo."
