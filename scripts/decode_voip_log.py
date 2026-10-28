#!/usr/bin/env python3
"""Decoder cho voip.log của ZaloCall.exe (native VoIP engine của Zalo PC).

File log không mã hóa thật — chỉ là bảng thế cố định 128 byte nhúng trong binary:
log-init function VA 0x7f8380, bảng 32 dword LE tại VA 0x7f86d1 (xem re-notes/07_voip_log.md).
Mỗi dòng: `<ts>\t<hex>`, cipher byte = T[plain & 0x7F]. Dòng hex không có ts là dòng
tiếp diễn của sự kiện nhiều dòng.

Dùng:
  python scripts/decode_voip_log.py              # giải mã toàn bộ file mặc định
  python scripts/decode_voip_log.py <file>       # giải mã file khác
  python scripts/decode_voip_log.py --tail       # theo dõi trực tiếp (chờ log mới)
  python scripts/decode_voip_log.py --tail -n 40 # in 40 dòng cuối rồi theo dõi

Stdlib duy nhất, Python 3.8+.
"""

import argparse
import os
import sys
import time

# 32 dword LE lấy từ ZaloCall.exe (bảng dựng trên stack, VA 0x7f86d1..0x7f87c4).
# Byte theo thứ tự LE = T[0x00..0x7F]; mã hóa: cipher = T[plain & 0x7F].
_DWORDS = [
    0x29354F63, 0x162F1D42, 0x101C0F65, 0x55121750, 0x2D222543, 0x381B6D13,
    0x7E0B6E15, 0x7B66184E, 0x5D522608, 0x73370964, 0x51726B14, 0x190A2153,
    0x6C0E1A0D, 0x5E3B0778, 0x2028006F, 0x4D7D1130, 0x5B543E31, 0x48751E04,
    0x057A3A27, 0x79583D23, 0x68572C06, 0x560C6003, 0x2B770161, 0x49703C34,
    0x4167334C, 0x765C7C1F, 0x5A474639, 0x407F0274, 0x5F592A24, 0x4B2E3F6A,
    0x36713269, 0x4A454462,
]

_T = bytearray(128)
_i = 0
for _d in _DWORDS:
    _T[_i] = _d & 0xFF
    _T[_i + 1] = (_d >> 8) & 0xFF
    _T[_i + 2] = (_d >> 16) & 0xFF
    _T[_i + 3] = (_d >> 24) & 0xFF
    _i += 4

if len(set(_T)) != 128:
    raise SystemExit("bảng thế không phải bijection — binary sai?")

# nghịch đảo: cipher byte -> plain (byte lạ giữ nguyên để debug dễ)
_INV = list(range(256))
for _p in range(128):
    _INV[_T[_p]] = _p


def decode_hex(hexstr):
    try:
        raw = bytes.fromhex(hexstr)
    except ValueError:
        return None
    return "".join(chr(_INV[b]) if _INV[b] != b or b < 128 else f"<{b:02X}>" for b in raw)


def decode_line(line):
    """Trả về (ts, plain) hoặc (None, None) nếu không khớp cấu trúc log."""
    line = line.rstrip("\r\n")
    if not line:
        return None, None
    if "\t" in line:
        ts, _, hexpart = line.partition("\t")
        hexpart = hexpart.strip()
        if hexpart and all(c in "0123456789abcdefABCDEF" for c in hexpart) and len(hexpart) % 2 == 0:
            return ts, decode_hex(hexpart)
        return ts, None  # có ts nhưng phần sau không phải hex
    # dòng tiếp diễn: hex thuần không có ts
    s = line.strip()
    if s and all(c in "0123456789abcdefABCDEF" for c in s) and len(s) % 2 == 0:
        return None, decode_hex(s)
    return None, None


def default_log_path():
    appdata = os.environ.get("APPDATA", "")
    return os.path.join(appdata, "ZaloData", "cal", "voip.log")


def read_lines(handle):
    return handle.read().splitlines()


def main():
    ap = argparse.ArgumentParser(description="Giải mã voip.log của ZaloCall.exe")
    ap.add_argument("file", nargs="?", default=None, help="đường dẫn voip.log (mặc định %%APPDATA%%\\ZaloData\\cal\\voip.log)")
    ap.add_argument("--tail", action="store_true", help="theo dõi file, giải mã log mới ngay khi xuất hiện")
    ap.add_argument("-n", "--lines", type=int, default=0, help="với --tail: in N dòng cuối trước khi theo dõi")
    ap.add_argument("--interval", type=float, default=1.0, help="chu kỳ poll khi --tail (giây)")
    args = ap.parse_args()

    path = args.file or default_log_path()
    if not os.path.exists(path):
        sys.exit(f"không tìm thấy {path}")

    if not args.tail:
        with open(path, "r", encoding="utf-8", errors="replace") as f:
            for raw in f:
                ts, plain = decode_line(raw)
                if plain is None:
                    print(f"RAW |{raw.rstrip()}|")
                elif ts:
                    print(f"{ts} |{plain}|")
                else:
                    print(f"    |{plain}|")
        return

    # --tail: in đuôi nếu -n, rồi poll phần tăng thêm
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        if args.lines:
            deque_tail = f.readlines()[-args.lines:]
            for raw in deque_tail:
                ts, plain = decode_line(raw)
                if plain is None:
                    print(f"RAW |{raw.rstrip()}|")
                elif ts:
                    print(f"{ts} |{plain}|")
                else:
                    print(f"    |{plain}|")
        pos = f.tell()
        print(f"--- tail {path} ---", flush=True)
        while True:
            try:
                size = os.path.getsize(path)
                if size < pos:  # file bị xoay/ghi lại
                    f.seek(0)
                    pos = 0
                f.seek(pos)
                new = f.read()
                pos = f.tell()
                if new:
                    for raw in new.splitlines():
                        ts, plain = decode_line(raw)
                        if plain is None:
                            print(f"RAW |{raw.rstrip()}|")
                        elif ts:
                            print(f"{ts} |{plain}|")
                        else:
                            print(f"    |{plain}|")
                    sys.stdout.flush()
            except FileNotFoundError:
                pos = 0
                try:
                    f = open(path, "r", encoding="utf-8", errors="replace")
                except FileNotFoundError:
                    pass
            time.sleep(args.interval)


if __name__ == "__main__":
    main()
