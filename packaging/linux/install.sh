#!/usr/bin/env bash
# Install QTzalo from the portable .tar.gz.
#   ./install.sh          -> /usr/local (uses sudo when available), otherwise ~/.local
#   ./install.sh --user   -> always ~/.local (no root needed)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODE="${1:-}"

if [ "$MODE" != "--user" ] && [ "$(id -u)" -eq 0 ]; then
    PREFIX="/usr/local"
elif [ "$MODE" != "--user" ] && command -v sudo >/dev/null 2>&1; then
    echo "Cài đặt QTzalo vào /usr/local (yêu cầu quyền sudo). Dùng --user để chỉ cài cho tài khoản hiện tại."
    exec sudo bash "$0" "$@"
else
    PREFIX="$HOME/.local"
fi

BIN_DIR="$PREFIX/bin"
APP_DIR="$PREFIX/share/applications"
ICON_DIR="$PREFIX/share/icons/hicolor/256x256/apps"

mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR"
install -m 755 "$SCRIPT_DIR/qtzalo" "$BIN_DIR/qtzalo"
install -m 644 "$SCRIPT_DIR/qtzalo.desktop" "$APP_DIR/qtzalo.desktop"
install -m 644 "$SCRIPT_DIR/qtzalo.png" "$ICON_DIR/qtzalo.png"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APP_DIR" >/dev/null 2>&1 || true
fi

echo "Đã cài đặt QTzalo tại $BIN_DIR/qtzalo"
echo "Chạy bằng lệnh: qtzalo (hoặc mở QTzalo từ menu ứng dụng)."
echo "Dữ liệu người dùng: \${XDG_DATA_HOME:-~/.local/share}/qtzalo"
