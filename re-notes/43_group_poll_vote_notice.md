# Poll vote events (`msgType: "group.poll"`)

## Symptom

In a group with an active poll, each vote showed as a member bubble reading
"[Hình ảnh]" with no image.

## Data

A vote arrives as a normal message (seen with `cmd` 511, 521 and 122):

```json
{
  "msgType": "group.poll", "dName": "<member>", "uidFrom": "…", "idTo": "<group>",
  "paramsExt": { "countUnread": 0, … },
  "content": {
    "action": "vote", "href": "", "thumb": "", "title": "", "description": "",
    "params": "{\"msg\":{\"vi\":\"%1$s tham gia cuộc bình chọn: %2$s\",\"en\":\"%1$s voted in the poll: %2$s\",…},\"question\":\"…\",\"pollId\":…,\"dName\":\"<member>\",\"isAnonymous\":false,…}"
  }
}
```

`content` has no text. `parse_chat_message`'s last fallback labelled any content with an
`href` or `thumb` key as "[Hình ảnh]", even when both were empty strings.

## Change

- `group.poll` builds its text from the `params.msg.vi` template: `%1$s` is the
  member's name, `%2$s` is the poll question. It is parsed as `MessageKind::System`,
  so it renders as a centred notice and does not count as unread, matching
  `countUnread: 0`.
- Without a template it falls back to "<name> đã cập nhật cuộc bình chọn: <question>".
- The last fallback labels a message "[Hình ảnh]" only when `href` or `thumb` is
  non-empty.
- When a parsed system notice is merged into an existing row
  (`merge_message_metadata`), its text replaces the stored one, so a row re-fetched
  from the server drops an old "[Hình ảnh]" label.
- `load_threads_cache` repairs cached rows saved by older builds: when
  `wire_ref.original_type` is `group.poll`, it rebuilds the notice from the stored raw
  `content`. Older rows that exist only in SQLite keep their text, because SQLite does
  not store the raw content.

No request, signing or frame behaviour changes.

## Verification

- Unit tests (`src/chat.rs`):
  - `poll_vote_is_a_system_notice_not_a_photo`
  - `poll_notice_falls_back_without_a_template`
  - `cached_poll_votes_labelled_as_photos_are_repaired`
  - `empty_href_and_thumb_do_not_make_a_photo_label`
  - `merging_a_parsed_poll_notice_replaces_the_stale_photo_label`
- The payload shape is from cached vote events in a real group (2026-11-20).
- Manual, debug build on the same account: every create and vote event in that group
  shows as a centred notice. A first run left five re-fetched rows reading
  "[Hình ảnh]", which led to the merge change above.
