#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BUNDLE="$ROOT/native/macos/build/DailyNotifier.app"
mkdir -p "$BUNDLE/Contents/MacOS"
xcrun swiftc -O "$ROOT/native/macos/DailyNotifier.swift" \
    -framework AppKit -framework UserNotifications -o "$BUNDLE/Contents/MacOS/DailyNotifier"
cat > "$BUNDLE/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>DailyNotifier</string>
<key>CFBundleIdentifier</key><string>com.ctdoteam1110.ctdozlite.notifications</string>
<key>CFBundleName</key><string>Zalo Notifications</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>1.0</string>
<key>LSUIElement</key><true/>
<key>NSPrincipalClass</key><string>NSApplication</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
</dict></plist>
PLIST
# Release packaging must re-sign the nested bundle with the parent distribution identity.
codesign --force --sign - "$BUNDLE"
if [[ -n "${1:-}" ]]; then
    mkdir -p "$1/Contents/Helpers"
    ditto "$BUNDLE" "$1/Contents/Helpers/DailyNotifier.app"
fi
printf '%s\n' "$BUNDLE"
