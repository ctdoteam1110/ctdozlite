#!/bin/bash
# Replace a stopped app using a private sibling transaction; retain the backup on rollback failure.
set -euo pipefail
PID="$1"
DMG="$2"
APP="$3"
VERSION="$4"
DOWNLOAD_DIR="$(dirname "$DMG")"
MOUNT=""
TRANSACTION=""
REPLACED=0
COMMITTED=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ -n "$MOUNT" ]]; then
        hdiutil detach "$MOUNT" -quiet >/dev/null 2>&1 || true
        rmdir "$MOUNT" 2>/dev/null || true
    fi
    if [[ -n "$TRANSACTION" ]]; then
        if [[ "$COMMITTED" == 0 && -d "$TRANSACTION/previous.app" ]]; then
            if [[ "$REPLACED" == 1 && -d "$APP" ]]; then
                mv "$APP" "$TRANSACTION/failed.app" || exit 1
            fi
            mv "$TRANSACTION/previous.app" "$APP" || exit 1
        fi
        rm -rf "$TRANSACTION"
    fi
    if [[ "$COMMITTED" == 1 ]]; then
        rm -f "$DMG"
        rmdir "$DOWNLOAD_DIR" 2>/dev/null || true
    elif [[ "$status" != 0 && -d "$APP" ]]; then
        open "$APP" || true
    fi
    exit "$status"
}
trap cleanup EXIT
[[ "$PID" =~ ^[1-9][0-9]*$ ]] || exit 1
[[ "$APP" == /*.app && -d "$APP" && ! -L "$APP" ]] || exit 1
for ((attempt=0; attempt<120; attempt++)); do
    kill -0 "$PID" 2>/dev/null || break
    sleep 0.25
done
if kill -0 "$PID" 2>/dev/null; then
    echo 'Application did not exit; update cancelled' >&2
    exit 1
fi
MOUNT="$(mktemp -d)"
hdiutil attach -nobrowse -readonly -mountpoint "$MOUNT" "$DMG" >/dev/null
SOURCE="$MOUNT/ctdozlite.app"
[[ -d "$SOURCE" && ! -L "$SOURCE" ]] || exit 1
codesign --verify --deep --strict "$SOURCE"
actual_version="$(plutil -extract CFBundleShortVersionString raw -o - "$SOURCE/Contents/Info.plist")"
[[ "$actual_version" == "$VERSION" ]] || exit 1
[[ "$(lipo -archs "$SOURCE/Contents/MacOS/ctdozlite")" == arm64 ]] || exit 1
TRANSACTION="$(mktemp -d "$(dirname "$APP")/.ctdozlite-update.XXXXXX")"
ditto "$SOURCE" "$TRANSACTION/new.app"
codesign --verify --deep --strict "$TRANSACTION/new.app"
hdiutil detach "$MOUNT" -quiet
rmdir "$MOUNT" 2>/dev/null || true
MOUNT=""
mv "$APP" "$TRANSACTION/previous.app"
mv "$TRANSACTION/new.app" "$APP"
REPLACED=1
open "$APP"
COMMITTED=1
