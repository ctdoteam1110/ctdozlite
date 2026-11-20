# Photo placeholder size (`content.params.width` / `height`)

## Symptom

Opening a chat with many photos made the message list jump. Each photo drew a fixed
280×150 placeholder, then took its real size (up to 360×340) when the thumbnail
loaded, pushing every row below it.

## Data

A photo message's `content.params` is a JSON string carrying the sender's pixel size,
next to the album fields read in `38_photo_album_group_layout.md`:

```json
{"width":1280,"height":960,"group_layout_id":"…","is_group_layout":1,…}
```

Our own `photo_original/send` sends the same `width` / `height` fields.

## Change

- `ChatMessage::media_size` keeps the declared size. It comes from
  `chat::photo_size_from_content` for incoming photos, or from the uploaded image for
  our own. It is saved in the `raw_json` metadata of the SQLite row, like
  `media_group`, and in the thread cache.
- `photo_display_size` (`src/gui/messages.rs`) computes the on-screen size. The
  placeholder, the loaded photo and the bubble width pre-measure all use it, so the
  row keeps one height from the first frame.
- Once the thumbnail is loaded, the declared size is used only if its aspect ratio
  is within 3% of the thumbnail's. If it is not, the thumbnail's own size wins, so a
  wrong or rotated declaration never stretches the photo.
- Photos without a declared size (older stored rows) keep the old placeholder.

No request, signing or frame behaviour changes; this only reads fields already
received.

## Verification

- Unit tests:
  - `photo_size_reads_declared_params_dimensions` (`src/chat.rs`)
  - `declared_photo_size_keeps_placeholder_and_loaded_photo_the_same_size`
    (`src/gui/messages.rs`)
  - `photo_declared_size_survives_store_and_hydrate` (`src/store.rs`)
- Manual check on a real account: pending.
