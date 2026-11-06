# 36 — @All mentions and friend aliases

Source: Zalo PC bundle extracted from `app.asar`
(`default-login-main-startup-shared-worker-znotification.*.js`, `main-startup.*.js`), 2026-11-05/06.

## Friend alias (tên gợi nhớ)
- Host: `getAliasDomain()` → `alias` in `SERVICE_MAP_V3` (`tt-alias-wpa.chat.zalo.me`).
- Commands: 12200 `/api/alias/list`, 12201 `/api/alias/update`, 12202 `/api/alias/remove`,
  12205 `/api/alias/list/v2`.
- `getAlias(page, count)` → GET `params = encodeAES({page, count, imei})`.
- `updateAlias(friendId, alias)` → GET `params = encodeAES({friendId, alias, imei})`.
- `removeAlias(friendId)` → GET `params = encodeAES({friendId})`.
- Response used by `_updateDataAlias`: `{items: [{userId, alias}], updateTime, hasMore}`;
  default page size `request_items_per_page || 100`.
- Implemented in `chat::get_aliases` / `chat::update_alias`; parser covered by
  `parse_alias_page_reads_items_and_has_more`.

## @All mention
- Bundle: `getEffectiveMentionAlias`: `if(-1==e.uid||"-1"===e.uid)return"@All"`; composer
  builds `{pos, len, uid, type}` per mention; outgoing items map `MENTION_ALL_ID` ↔
  `MENTION_TYPE_ALL`, members ↔ `MENTION_TYPE_USER`.
- The numeric values of `MENTION_TYPE_*` were not found as literals in the minified bundle.
  `type: 1` for uid `-1` (members `type: 0`) follows the open-source zca-js client.
- Not yet verified against a live capture: send one "@All" message in a test group and confirm
  every member gets the mention notification.
