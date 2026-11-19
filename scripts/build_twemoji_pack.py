#!/usr/bin/env python3
"""Pack Twemoji 72x72 PNGs into src/assets/emoji/twemoji72.pack.

Usage: python3 scripts/build_twemoji_pack.py <twemoji>/assets/72x72 src/assets/emoji/twemoji72.pack

Source: https://github.com/jdecked/twemoji (graphics under CC-BY 4.0).

Layout (little endian), read by `src/gui/color_emoji.rs`:
    b"TWEMOJI1"
    u32 entry count
    per entry: u8 key length, key (UTF-8, U+FE0F removed), u32 offset, u32 length
    PNG data; offsets are relative to the start of this data block
"""

import pathlib
import struct
import sys


def main() -> None:
    src, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
    entries = {}
    for png in sorted(src.glob("*.png")):
        chars = [chr(int(cp, 16)) for cp in png.stem.split("-")]
        key = "".join(c for c in chars if c != "️")
        # Twemoji ships some sequences both with and without U+FE0F; keep the first.
        entries.setdefault(key, png.read_bytes())

    header = bytearray(b"TWEMOJI1")
    header += struct.pack("<I", len(entries))
    data = bytearray()
    for key, png in entries.items():
        raw = key.encode("utf-8")
        header += struct.pack("<B", len(raw)) + raw
        header += struct.pack("<II", len(data), len(png))
        data += png
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(bytes(header + data))
    print(f"{len(entries)} emoji, {len(header) + len(data)} bytes -> {out}")


if __name__ == "__main__":
    main()
