# Media placeholder sizes (`content.params` width / height)

## Symptom

Opening a chat with many photos or videos made the message list jump. A row that
waits for a thumbnail drew a small placeholder, then grew when the thumbnail loaded,
pushing every row below it. A temporary log of row heights on a real account
(2026-11-20) found 83 rows that changed height while loading:

| Kind | Rows | Height before → after |
| :--- | :--- | :--- |
| Video | 48 | 99 → 264 |
| File with an image preview | 20 | 144 → 406 |
| Link with a preview thumbnail | 12 | ~155 → ~347 |
| Sticker | 3 | 38 → 136 |

Single photos jumped too: a fixed 280×150 placeholder became up to 360×340.

## Data

A media message's `content.params` is a JSON string. For a photo it carries the
sender's pixel size next to the album fields read in `38_photo_album_group_layout.md`:

```json
{"width":1280,"height":960,"group_layout_id":"…","is_group_layout":1,…}
```

For a video, Zalo's web client reads `video_width` / `video_height` (the `MSG_VIDEO`
case in `web-bundle/default-embed-render.js`):

```json
{"duration":5000,"video_width":720,"video_height":1280,…}
```

Our own `photo_original/send` sends `width` / `height`. Files and link previews carry
no image size.

## Change

- `ChatMessage::media_size` keeps the declared size: `chat::photo_size_from_content`
  for photos, `chat::video_size_from_content` for videos, the uploaded image for our
  own photos. It is saved in the `raw_json` metadata of the SQLite row, like
  `media_group`, and in the thread cache.
- Photos: `photo_display_size` (`src/gui/messages.rs`) sizes the placeholder, the
  loaded photo and the bubble width pre-measure from that size.
- Videos: with a declared size, a loading thumbnail draws an empty box of the final
  size (`render_video_thumb_block` with no texture), and the bubble width uses it too.
  Videos without a declared size keep the plain card.
- In both cases, once the thumbnail is loaded the declared size is kept only if its
  aspect ratio is within 3% of the thumbnail's (`media_source_size`). Otherwise the
  thumbnail's own size wins, so a wrong or rotated declaration never stretches it.
- Image files and link previews: the preview is a fixed box (4:3 for files, up to
  260×132 for links), with the thumbnail center-cropped into it (`cover_uv_for`). The
  box is reserved while the thumbnail loads. If the fetch fails, the row falls back as
  before.
- Stickers: a loading sticker holds its 116×116 box instead of the "[Nhãn dán]" text;
  a failed one still falls back to the text.

Review follow-up (PR #192):

- `compute_video_thumb_size` caps each width floor (140 / 160) at `max_w`, so a chat
  pane narrower than the floor no longer inverts the `clamp` range and panics. Image
  file previews now call it too.
- `apply_vanish_expiry` also clears `media_size` and `media_group`. Otherwise an
  expired photo serialized non-empty `raw_json`, the store did not write the vanish
  tombstone, and a later upsert could restore the content.
- Hydrating from SQLite copies `media_size` into a message already in RAM that has
  none, so the stored size is neither ignored nor dropped on the next save.
- `StoredMessage::from` writes empty `raw_json` for any expired message, so the store
  keeps it as a tombstone even when leftover metadata such as reactions remains.
- `upsert_message` writes the `{"vanished":true}` marker when it inserts a tombstone as
  a new row too, not only when it updates an existing one. Otherwise an expired
  message imported first could be overwritten by a later sync of the original.

No request, signing or frame behaviour changes; this only reads fields already
received.

## Verification

- Unit tests:
  - `photo_size_reads_declared_params_dimensions` (`src/chat.rs`)
  - `declared_photo_size_keeps_placeholder_and_loaded_photo_the_same_size`,
    `cover_uv_for_crops_to_the_target_aspect` and
    `loading_sticker_and_video_hold_their_final_size` (`src/gui/messages.rs`)
  - `photo_declared_size_survives_store_and_hydrate` (`src/store.rs`; also covers
    hydrating over a message in RAM and the expired-photo tombstone)
  - `narrow_bubbles_do_not_invert_media_size_clamps` and
    `loading_image_file_preview_renders_in_a_narrow_bubble` (`src/gui/messages.rs`)
  - `expired_message_with_reactions_stays_a_tombstone` and
    `expired_message_inserted_first_stays_a_tombstone` (`src/store.rs`)
- Manual checks on a real account (2026-11-20), each with a temporary log:
  - Of about 200 photo messages loaded at startup, all but two carried `width` and
    `height`. Of 135 video messages, all but one carried `video_width` and
    `video_height`.
  - With the change, the per-row height log recorded no row changing height while
    the same chats loaded and scrolled (it recorded 83 before).
