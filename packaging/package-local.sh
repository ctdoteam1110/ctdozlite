#!/usr/bin/env bash
# Build and package ctdozlite locally on macOS (.dmg) or Linux (.deb / .tar.gz)
# without mutating tracked packaging templates (which keep the 0.0.0 sentinel).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

VER="$(cargo metadata --locked --no-deps --format-version 1 \
  | python3 -c 'import json,sys; d=json.load(sys.stdin); print(next(p["version"] for p in d["packages"] if p["name"]=="ctdozlite"))')"

echo "=== Building ctdozlite v${VER} ==="
mkdir -p dist

OS_NAME="$(uname -s)"
if [[ "$OS_NAME" == "Darwin" ]]; then
  ARCH="$(uname -m)"
  if [[ "$ARCH" == "arm64" ]]; then
    TARGET="aarch64-apple-darwin"
    TOOLCHAIN="$ROOT_DIR/packaging/macos/toolchain-arm64.cmake"
    SWIFT_TARGETS="arm64-apple-macosx11.0"
    DMG_OUT="dist/ctdozlite-${VER}-macos-arm64.dmg"
  else
    TARGET="x86_64-apple-darwin"
    TOOLCHAIN="$ROOT_DIR/packaging/macos/toolchain-x86_64.cmake"
    SWIFT_TARGETS="x86_64-apple-macosx11.0"
    DMG_OUT="dist/ctdozlite-${VER}-macos-x86_64.dmg"
  fi

  MACOSX_DEPLOYMENT_TARGET="11.0" \
  LIBOPUS_STATIC="1" LIBOPUS_NO_PKG="1" OPUS_STATIC="1" OPUS_NO_PKG="1" \
  CMAKE_TOOLCHAIN_FILE="$TOOLCHAIN" \
    cargo build --release --locked --target "$TARGET" --bin ctdozlite

  STAGE_DIR="$(mktemp -d)"
  trap 'rm -rf "$STAGE_DIR"' EXIT

  mkdir -p "$STAGE_DIR/ctdozlite.app/Contents/MacOS" "$STAGE_DIR/ctdozlite.app/Contents/Resources"
  cp "target/${TARGET}/release/ctdozlite" "$STAGE_DIR/ctdozlite.app/Contents/MacOS/ctdozlite"
  chmod +x "$STAGE_DIR/ctdozlite.app/Contents/MacOS/ctdozlite"

  env -u HISTORY_CODEC_ASSETS SWIFT_TARGETS="$SWIFT_TARGETS" \
    bash native/macos/build-native-helpers.sh "$STAGE_DIR/ctdozlite.app"

  cp packaging/macos/Info.plist "$STAGE_DIR/ctdozlite.app/Contents/Info.plist"
  plutil -replace CFBundleShortVersionString -string "$VER" "$STAGE_DIR/ctdozlite.app/Contents/Info.plist"
  plutil -replace CFBundleVersion -string "$VER" "$STAGE_DIR/ctdozlite.app/Contents/Info.plist"
  printf 'APPL????' > "$STAGE_DIR/ctdozlite.app/Contents/PkgInfo"

  ICONSET_DIR="$STAGE_DIR/AppIcon.iconset"
  mkdir -p "$ICONSET_DIR"
  for sz in 16 32 128 256 512; do
    sips -s format png -z "$sz" "$sz" src/assets/app_icon.png --out "$ICONSET_DIR/icon_${sz}x${sz}.png" >/dev/null
    sz2=$((sz * 2))
    sips -s format png -z "$sz2" "$sz2" src/assets/app_icon.png --out "$ICONSET_DIR/icon_${sz}x${sz}@2x.png" >/dev/null
  done
  iconutil -c icns "$ICONSET_DIR" -o "$STAGE_DIR/ctdozlite.app/Contents/Resources/AppIcon.icns" || true
  rm -rf "$ICONSET_DIR"

  xattr -cr "$STAGE_DIR/ctdozlite.app" || true
  codesign --force --sign - "$STAGE_DIR/ctdozlite.app/Contents/Helpers/DailyNotifier.app"
  codesign --force --deep --sign - "$STAGE_DIR/ctdozlite.app"
  ln -s /Applications "$STAGE_DIR/Applications"

  hdiutil create -volname "ctdozlite" -srcfolder "$STAGE_DIR" -ov -fs HFS+ -format UDZO "$DMG_OUT"
  echo "=== Built $DMG_OUT ==="
else
  cargo build --release --locked --bin ctdozlite
  strip target/release/ctdozlite || true
  STAGE="ctdozlite-${VER}-linux-x86_64"
  STAGE_DIR="$(mktemp -d)"
  trap 'rm -rf "$STAGE_DIR"' EXIT
  mkdir -p "$STAGE_DIR/$STAGE"
  cp target/release/ctdozlite "$STAGE_DIR/$STAGE/ctdozlite"
  cp packaging/linux/install.sh "$STAGE_DIR/$STAGE/install.sh"
  cp packaging/linux/uninstall.sh "$STAGE_DIR/$STAGE/uninstall.sh"
  cp packaging/linux/ctdozlite.desktop "$STAGE_DIR/$STAGE/ctdozlite.desktop"
  cp src/assets/app_icon.png "$STAGE_DIR/$STAGE/ctdozlite.png"
  cp README.md LICENSE-MIT LICENSE-APACHE "$STAGE_DIR/$STAGE/"
  chmod +x "$STAGE_DIR/$STAGE/ctdozlite" "$STAGE_DIR/$STAGE/install.sh" "$STAGE_DIR/$STAGE/uninstall.sh"
  tar -C "$STAGE_DIR" -czf "dist/${STAGE}.tar.gz" "$STAGE"
  echo "=== Built dist/${STAGE}.tar.gz ==="
fi
