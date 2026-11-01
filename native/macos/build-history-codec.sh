#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/native/macos/build"
ASSETS="${1:-$OUT/HistoryCodecAssets}"
ZSTD="${ZSTD_STATIC_LIBRARY:-/opt/homebrew/lib/libzstd.a}"
[[ -f "$ZSTD" ]] || { printf 'Missing static zstd library: set ZSTD_STATIC_LIBRARY\n' >&2; exit 1; }
if [[ -z "${1:-}" ]]; then
    missing=false
    for name in web-module-hRO2.js web-module-Ho1Y.js web-module-1y5R.js web-module-Vox7.js libzproto.wasm; do
        [[ -f "$ASSETS/$name" ]] || missing=true
    done
    if [[ "$missing" == true ]]; then
        python3 "$ROOT/native/macos/prepare-history-assets.py" --output "$ASSETS"
    fi
fi
ASSETS="$(cd "$ASSETS" && pwd)"
WASM="$ASSETS/libzproto.wasm"
[[ -f "$WASM" ]] || WASM="$ASSETS/libzproto_wasm_bg.ecc0555ba9d0f8867a4699ce943ec745.wasm"
# Verify pinned codec bytes before compiling or packaging them.
python3 - "$ROOT/native/macos" "$ASSETS" "$WASM" <<'PY'
import importlib.util
from pathlib import Path
import sys
spec = importlib.util.spec_from_file_location("assets", Path(sys.argv[1]) / "prepare-history-assets.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
for name, expected in module.MODULE_HASHES.items():
    module.verify((Path(sys.argv[2]) / f"web-module-{name}.js").read_bytes(), expected, name)
module.verify(Path(sys.argv[3]).read_bytes(), module.WASM_HASH, "WASM")
PY
mkdir -p "$OUT/HistoryCodecAssets"
if [[ "$(cd "$ASSETS" && pwd)" != "$OUT/HistoryCodecAssets" ]]; then
    for module in hRO2 Ho1Y 1y5R Vox7; do
        cp "$ASSETS/web-module-$module.js" "$OUT/HistoryCodecAssets/"
    done
    cp "$WASM" "$OUT/HistoryCodecAssets/libzproto.wasm"
    [[ ! -f "$ASSETS/MANIFEST.json" ]] || cp "$ASSETS/MANIFEST.json" "$OUT/HistoryCodecAssets/"
elif [[ "$WASM" != "$OUT/HistoryCodecAssets/libzproto.wasm" ]]; then
    cp "$WASM" "$OUT/HistoryCodecAssets/libzproto.wasm"
fi
xcrun swiftc -O "$ROOT/native/macos/HistoryCodec.swift" "$ZSTD" -lz \
    -framework JavaScriptCore -framework Security -o "$OUT/HistoryCodec"
# Optional packaging destination must be a prepared application bundle.
if [[ -n "${2:-}" ]]; then
    APP="$2"
    mkdir -p "$APP/Contents/Helpers" "$APP/Contents/Resources/HistoryCodecAssets"
    cp "$OUT/HistoryCodec" "$APP/Contents/Helpers/HistoryCodec"
    cp "$OUT/HistoryCodecAssets/"* "$APP/Contents/Resources/HistoryCodecAssets/"
fi
printf '%s\n' "$OUT/HistoryCodec"
