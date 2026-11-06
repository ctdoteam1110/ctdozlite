# 38 — Reusing Zalo PC's downloaded media (read-only)

Observed on macOS, 2026-11-06, by listing directory entries only (no message database was opened).

## Where Zalo PC keeps things
- Root: `~/Library/Application Support/ZaloData/`.
- `Database/_production/<uid>/…` (`MsgInfo.db`, `Core/Index.db`, `Core/Message/…`): the first 16 bytes
  are not `SQLite format 3\0`, so these are encrypted. Only `Storage.db` is plain SQLite, and it does
  not hold messages. ctdozlite does not read any of them.
- `media/<own uid>/ZaloDownloads/` holds plain files (about 3.4 GB on the test machine):
  - `picture/<thread>[_group]/z<global msg id>_<hash>.<jpg|jxl>`. The id matches `messages.msg_id`
    in `zalo_history.db` (417 rows on the test machine, most of them empty phone-sync rows).
  - `resource/<g?thread>/<picture|video|file|Cache>/<cli msg id>_<sender uid>_<g?thread>[_<hash>.<ext>]`.
    The first number is the sender's client message id, which is a millisecond clock value and
    usually lands within about 2 s of the server `ts`. `file/` entries have no extension
    (`file(1)` shows PDF and similar types); `video/` entries are MP4.
  - `file/<md5>` with a `.rescache` JSON index (`{data: {<md5>: [0, size]}}`). The key is not the md5
    of the file URL; it is not used yet.
  - Photos are mostly JPEG XL (`.jxl`). The `image` crate cannot decode them; macOS ImageIO
    (`NSBitmapImageRep imageRepWithData:`) can.

## Implementation
- `zalo_pc_media::ZaloPcMedia::scan` builds an index from directory entries off the UI thread after
  login. `attachment_for` pairs a message by exact global id, exact client id, or the only entry from
  the same sender within ±2.5 s. A burst (album) is left unpaired, and each file is given to at most
  one message.
- `gui::fill_open_thread_from_zalo_pc` fills empty messages of the open conversation (no text, no
  URL) with the local photo, video or file. Files are registered as already downloaded, so the card
  offers open and reveal.
- `worker` image decoding falls back to `decode_to_png_with_imageio` for JPEG XL.
- Nothing is ever written under `ZaloData`.

## Not verified
- Time-window pairing on real conversations has not been checked against Zalo PC side by side.
  Exact-id pairs are reliable.
- Windows and Linux paths are not handled (`default_root` returns `None` there).
