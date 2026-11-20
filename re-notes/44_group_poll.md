# Group polls: detail and vote

## Goal

Show a group poll as a card (question, options, counts, voters) and let the user vote or
change their vote. Before this change a poll was only a one-line notice
(`re-notes/43_group_poll_vote_notice.md`).

## Source

The poll API is not in the old `web-bundle/` snapshot (that only has the IndexedDB `poll`
table schema, `web-bundle/default-embed-render.js` @ 591000..592200). It is in the current
Zalo Web lazy chunk, downloaded 2026-11-20 from `zalo-chat-static.zadn.vn/v1/lazy/`
(the chunk list comes from `render.081738ba2350940a6b28.js`, linked from `chat.zalo.me`):

| File | Byte offset | What |
| :--- | ---: | :--- |
| `1.c72c0957b455ab70b73c.js` | 2521356 | command table: `M=s.a.GROUP_POLL`, `12080 /api/poll/create`, `12081 /api/poll/detail`, `12083 /api/poll/option/add`, `12084 /api/poll/vote`, `12085 /api/poll/share`, `12089 /api/poll/end` |
| same | 13657113 | domain enum: `GROUP_POLL:"group_poll"` (the `zpw_service_map_v3` key) |
| same | 13661565 | `getGroupPollDomain(){return this.getDomainByType(o.GROUP_POLL)}` |
| same | 11658837 | `static getPollDetail(e,t)` |
| same | 11659340 | `static vote(e,t,n)` |
| same | 11659823 | `static lockPoll(e)` (`/api/poll/end`, not used here) |
| same | 11284318 | `_preParsePollData`: response handling |
| same | 2431580 | vote modal: `u.default.vote(t,n,e)`, then `Object.assign(data,{options:e.options})` |
| same | 2431730 | vote error `166` → `STR_BLOCKED_POLL` |
| `lang-vi.81401760339cc690532c.js` | — | Vietnamese labels (`STR_GROUP_POLL_*`, `STR_POLL_*`, `STR_VOTE`) |

The chunk is not committed (23 MB of third-party code); the hashes in the file names pin the
version.

## Requests

Both are `GET` on the `group_poll` host, with the usual common query (`zpw_ver`, `zpw_type`)
and `params=<encodeAES(JSON)>`:

```js
static getPollDetail(e,t){let n={poll_id:t,imei:…getZaloClientID()},
  a=p.b.getGroupPollDomain()+"/api/poll/detail?"+this._getCommonParams()+"&params="+encodeURIComponent(o.default.encodeAES(JSON.stringify(n)));
  return this._get(a,null,12081)}
static vote(e,t,n){let a={poll_id:t,option_ids:n,imei:…getZaloClientID()},
  s=p.b.getGroupPollDomain()+"/api/poll/vote?"+…;return this._get(s,null,12084)}
```

- `poll_id` is the number from the event's `content.params.pollId`.
- `option_ids` is an array of option ids (`getCheckedIdsList()`), not a JSON string. The group
  id argument `e` is not sent.
- Adding a new option goes through `/api/poll/option/add` (`new_options` as a JSON string,
  `voted_option_ids`); not implemented here.

## Response

`_preParsePollData` checks `error_code` of the envelope, decrypts `data` with the session
key, reads ids in `creator` and `voters` as strings (`preParse(a,["creator","voters"])`),
then rejects when the inner `error_code` is non-zero and returns the inner `data`. The fields
match the IndexedDB `poll` table:

```json
{
  "poll_id": 1001, "creator": "user-1", "question": "…", "group_id": "…",
  "options": [{ "option_id": 1, "content": "…", "votes": 2, "voted": false, "voters": ["user-2", "user-3"] }],
  "joined": true, "closed": false, "allow_multi_choices": false, "allow_add_new_option": true,
  "is_anonymous": false, "is_hide_vote_preview": false, "poll_type": 0,
  "created_time": 0, "updated_time": 0, "expired_time": 0, "num_vote": 2
}
```

- `expired_time` is in ms; `0` means no deadline.
- `num_vote` is the number of distinct voters; when absent the web client counts the union of
  `voters`.
- The vote reply has the same shape; the client only takes `options` from it.
- Error `166`: poll closed or locked (`STR_POLL_ERR` when `closed`, otherwise
  `STR_BLOCKED_POLL`).

## Card behaviour (Zalo Web)

- The card sits under the latest poll message of that poll in the conversation
  (`isShowPollCard`, `getLastMessagePoll`, `web-startup.a00a02c532982ab4915a.js` @ 1115591).
- `is_anonymous`: voters are hidden.
- `is_hide_vote_preview` and not `joined`: results are hidden until the user votes, and a vote
  cannot be changed (`STR_GROUP_POLL_CONFIRM_PREVIEW`).
- `closed` or an expired `expired_time`: no voting (`STR_GROUP_POLL_REASON_CLOSED`).

## Host

`group_poll` is now read from `zpw_service_map_v3` like the other service keys. If the session
has no such entry, the request falls back to the `group` host.

## Verification

- Unit tests: `src/poll.rs` (wire ref, params, detail/vote parsing, error `166`, vote
  rules), `src/gui/poll_card.rs` (refetch rules, draft selection, voter line) and
  `src/session.rs` (`group_poll_candidates_*`). Fixtures use made-up ids.
- Manual, debug build, 2026-11-20, a real group with an active poll: the card loads with
  the options and counts, and voting and changing the vote update the counts.
- No `session::failover::group_poll` line was logged, so the first host candidate answered
  both calls. The run did not record whether that was the service-map entry or the
  `group` fallback.
