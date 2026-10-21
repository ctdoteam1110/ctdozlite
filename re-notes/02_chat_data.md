# 02 — Chat data flow (conversation list + message display)

Target: Zalo PC 26.10.11, Electron asar. Scope: get conversation list + read messages
without reimplementing sync-v2.

Evidence tags: [O] = read from shipped bundle, [I] = inference, [U] = unknown.

---

## 1. Where conversations come from — two planes

[O] The 232-entry command registry (REST) contains **no** "list all conversations" or
"get message history" command. Those live in sync-v2 (dedicated workers, binary framing).

[O] But the registry **does** contain a REST bootstrap path the PC client uses at startup
to paint the chat list before sync-v2 catches up:

| cmd | path | domain | purpose |
|---|---|---|---|
| 10053 | `/api/preloadconvers/get-last-msgs` | conversation | last message per thread |
| 12009 | `/api/preloadconvers/get-last-seen` | chat | last-seen per thread |
| 12120 | `/api/pinconvers/list` | conversation | pinned threads |
| 10048 | `/api/hiddenconvers/get-all` | conversation | hidden threads |
| 12076 | `/api/archivedchat/list` | conversation | archived threads |
| 11138 | `/api/social/friend/verify` | social | friend check |
| — | `/api/social/friend/getfriends` | social | friend list |
| — | `/api/friend/profile/multiget` | friend | display names / avatars |
| — | `/api/group/history` | group | group message history |

[I] `getfriends` + `get-last-msgs` is enough for a readable chat list.
`/api/group/history` is the only REST message-history endpoint (groups only).

---

## 2. Exact request shape — `getLastMessagesForPreview`

[O] Verbatim from `compact-app-pc.b7bec208c4c8f73e09f8.js`:

```
t = { threadIdLocalMsgId: JSON.stringify(e), imei: getZaloClientID() }
n = CONVERSATION_DOMAIN + "/api/preloadconvers/get-last-msgs?"
  + _getCommonParams()
  + "&params=" + encodeURIComponent( encodeAES( JSON.stringify(t) ) )
GET n   // commandId 10053
```

[O] `_getCommonParams()` → `_constructUrlParams(se)` where

```
se = { zpw_ver: apiVersion /*693*/, zpw_type: apiType /*30*/ }
```

That is the **entire** common-param object. Every other param is per-call.

[O] `e` (the argument) is the array of `{threadId, localMsgId}` pairs the local DB already
knows. [U] Schema of the array elements beyond `threadId`/`localMsgId` is not confirmed —
send `[]` or `{threadId, localMsgId:0}` and inspect.

---

## 3. Legacy param encryption (`encodeAES`) — confirmed

[O]

```
static encodeAES(e, t = 0)                 // e = plaintext JSON string
  key = Base64.parse( getSecretKey() )      // raw bytes
  AES-256-CBC, iv = 16 zero bytes, PKCS7
  output = base64(ciphertext)
```

[O] `setDecryptKey(e){ le = e }` and `getSecretKey()` returns `le`, therefore:

**the legacy AES key is the `zpw_enk` field of the `getLoginInfo` response.**

[O] Confirmed at call site:
`n({config:e.data.data, UIN:e.data.data.dkey, decryptKey:e.data.data.zpw_enk, ...})`
then `l.default.setUIN(t.UIN); l.default.setDecryptKey(t.decryptKey)`.

[O] `zpw_enk` arrives **base64-encoded**; base64-decode it → 32 raw bytes → AES-256 key.
[I] It is per-session, delivered post-login. There is no hardcoded param key.

### Response decryption

[O] `decryptResp(key, resp)` is applied only when the request was param-encrypted
(`enk` was sent). Plain `params=` requests get plain JSON back. So an MVP can run
**fully unencrypted** and never needs `zpw_enk` for reads.

[O] `decodeAES(e,t,n,a)` → `$znode.crypto.decrypt(t, "aes-256-cbc", e, n, {key:"utf-8", input:"base64", iv:...})`

---

## 4. Signing

[O]

```
getSignKey(tag, params):
  keys = Object.keys(params).sort()
  s = "zsecure" + tag
  for k in keys: s += params[k]
  return md5(s)
```

[O] `tag` is the commandId (rendered as a string) in the v2 path. Params object here is the
**post-encryption** object `{type, client_version, signkey, params|encrypted_*, enk}`.

[I] For an unencrypted MVP, `sig` is not required — plain legacy calls in the bundle
(e.g. `resetPIN`) send only `_getCommonParams()` with no signature.

**[O] VERIFIED LIVE:** this assumption was wrong for `getServerInfo`. First run returned
`error_code 18022 "Invalid Signature"`. After adding
`signkey = md5("zsecure" + "getserverinfo" + sorted values)` the call returned
`error_code 0`. So **signing is mandatory on at least the pre-auth endpoints** even when
params are unencrypted.

[O] Bundle body for `getServerInfo`:

```
o = { imei, type: apiType, client_version: apiVersion, computer_name, sp_mtn: -1, bkt: -1 }
o.signkey = getSignKey("getserverinfo", o)
GET {auth}/api/login/getServerInfo?imei=…&type=30&client_version=693&…&signkey=…
```

Note the params go **inline in the query string**, not inside `params=`.

---

## 5. Realtime — WPA websocket

[O] Login response carries `config.zpw_ws` (socket endpoint list), `zpw_chts_new`,
`zpw_ctrs_new`, `zpw_conf`, `zpw_service_map_new`, `zpw_service_map_v3`, `zpw_ctrs_new`.
`setEndpoints(chts, ctrs, serviceMapNew, conf, ws)` consumes them.

[O] `new WebSocket(config.url + "&t=" + Date.now())`, `binaryType = "arraybuffer"`.
[I] Frames are JSON `{name, data}` (one handler class dispatches on `name`).
[U] Exact frame envelope + auth handshake after connect is **not** decoded — the socket
implementation sits in a separate multi-MB worker chunk not yet traced.

**MVP decision: skip websocket. Poll `get-last-msgs` every N seconds.** One endpoint,
already understood, no framing work. Upgrade to WPA later if latency matters.

---

## 6. Domain resolution

**[O] LIVE-VERIFIED** — `getServerInfo` returns the full endpoint table. Defaults cannot be
derived from `zaloapp.com` by string surgery; they must be read from the payload.

Top-level keys observed: `zpw_chts_new`, `zpw_ctrs_new`, `zpw_service_map_v3`, `zpw_conf`,
`zpw_ws`, `setttings`, `qos_setting`.

```
zpw_chts_new[0]              = https://p4-msg-chat.zaloapp.com     → chat (send/history)
zpw_ctrs_new[0]              = https://p4-ctl-chat.zaloapp.com     → conversation control
zpw_service_map_v3.friend[]         = https://tt-friend-wpa.zaloapp.com
zpw_service_map_v3.conversation[]   = https://tt-convers-wpa.api.zaloapp.com
zpw_service_map_v3.profile[]        = https://tt-profile-wpa.zaloapp.com
zpw_service_map_v3.group[]          = https://tt-group-wpa.zaloapp.com
zpw_service_map_v3.file[]           = https://tt-files-wpa.zaloapp.com
zpw_ws[]                     = wss://ws{1..8}-msg.chat.zalo.me   (6 endpoints)
```

[O] Notably **not** `zpw_service_map_new` — that key does not exist in this response, only
`zpw_service_map_v3`. Reading the wrong key silently leaves every domain at its default.

[O] `setEndpoints(chts, ctrs, mapNew, conf, ws)` mapping confirmed in bundle:
`chs→chat`, `ctrs→conversation`, `mapNew.{friend,profile,group,file,label,alias,board,sticker}`.
`n.conversation` overrides `apiConversationDomain` only when present.

[O] The host index (`p4`) is server-assigned per session and varies between requests.

---

## 6.1 Authentication gate — live-verified

[O] Calling `/api/preloadconvers/get-last-msgs` without a session returns:

```
HTTP 200  {"error_code":600,"error_message":"zpw_sek not found or invalid","data":null}
```

**`zpw_sek` is the session cookie/param that gates all chat data.** It is issued at login.
The unauthenticated surface is therefore only: `getServerInfo`, `reqqr`, `getCaptcha`,
`loginByPasswv2` — everything else needs `zpw_sek`.

[I] MVP consequence: no chat data can be fetched before a real login. The chat layer cannot
be validated without credentials.

---

## 7. Minimum viable Rust path

1. `GET /api/login/getServerInfo` → `enable_encrypt_param` flag, config (unauthenticated)
2. QR or password login → cookies (`Set-Cookie`) + `getLoginInfo` JSON
3. store `zpw_enk` (unused in MVP), `UIN = dkey`, and the domain block
4. `GET {social}/api/social/friend/getfriends?{common}` → threads
5. `GET {conv}/api/preloadconvers/get-last-msgs?{common}&params={...}` → last messages
6. render list; poll every 3–5 s
7. (optional) `encodeAES` with `zpw_enk` once the plain path is proven

Open [U]:
- `getfriends` exact param/response schema (not yet extracted)
- whether `get-last-msgs` accepts an empty thread list or needs an explicit array
- whether a plain (unencrypted) call is rejected by server policy for this account tier
