# Photo album (group layout) on `photo_original/send`

Zalo shows several photos as one album when each `photo_original/send` request
carries the same group-layout id. The field set comes from zca-js (MIT), which
reverse-engineers Zalo Web: `src/apis/sendMessage.ts`, `handleAttachment`:
https://github.com/RFS-ADRENO/zca-js/blob/main/src/apis/sendMessage.ts

For a send with more than one photo, every photo's params gain:

| Field | Value |
| :--- | :--- |
| `groupLayoutId` | Shared id, string; zca-js uses `Date.now()` (Unix ms) |
| `isGroupLayout` | `1` |
| `idInGroup` | Zero-based position inside the album |
| `totalItemInGroup` | Number of photos in the album |
| `extMsgProp` | JSON string `{"groupMediaMsg":{"groupLayoutId":"<id>"}}` |

The upload stage (`photo_original/upload`) is unchanged; each photo is still
uploaded and sent with its own request, in order. zca-js removes GIFs from the
batch before building the album; videos and other files go through
`asyncfile` and never carry these fields.

Implementation: `chat::MediaGroupLayout`, `chat::send_attachment_in_layout`,
`gui::attachments::plan_album_slots` (albums form only from two or more
PNG/JPG/JPEG/WEBP/BMP files). The caption is sent as `desc` on the first photo
only. The single-shot `message/photo` fallback does not add the fields.

## Incoming albums

Received (and echoed) album photos carry the membership inside the photo
`content.params` JSON string as snake_case keys: `group_layout_id`,
`is_group_layout` (1), `id_in_group` and, when present, `total_item_in_group`.
This matches the open-source deplao-builder client
(`src/ui/lib/chat/messages/messageGrouping.ts`, `getGroupLayoutId`):
https://github.com/babyvibe/deplao-builder

`chat::MediaGroupLayout::from_content` reads these keys (camelCase accepted
too); the value is stored as `ChatMessage::media_group` and persisted in the
`raw_json` metadata column. The GUI groups consecutive photos from the same
side and sender sharing one id into a single grid bubble
(`gui::messages::album_run_lengths`).

## Verification

- 2026-11-09: several photos sent from ctdozlite rendered as one album on the
  official Zalo mobile client (manual check by the contributor).
- Not yet confirmed against a Zalo PC capture. Photos stored before this change
  lack `media_group` in `raw_json` and keep rendering one by one.
