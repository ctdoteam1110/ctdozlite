# 01 — Zalo PC 26.10.11 Login Protocol (RE notes)

Target: `Zalo-26.10.11` Electron app, extracted to `asar_extracted/`.
Primary bundle: `pc-dist/lazy/default-login-main-startup-shared-worker-znotification.95fa59a429e23a433366.js` (16.2 MB minified).
Secrets module: `pc-dist/lazy/vendors-login-…81ce8c4b….js` → webpack module `"5NUA"` (`ZPC_SECRET_VALUES`).
Crypto bridge `$znode.crypto`: `main-dist/preload-render.js`.

Evidence tags: **[O]** observed in bundle, **[I]** inference, **[U]** unknown.

## 1. Hardcoded secrets (module 5NUA) [O]

```text
paramCipherKey      "3FC4F0D2AB50057BCE0D90D9187A22B1"   # 32 ASCII chars → raw AES-256 key
passwordCipherKey   "9fc5vEW3"                            # input string for enk derivation, not a raw key
timeOnAppXorKey     "IUU3RM3GABH3NS0ABD3TT04N9ABCAA1U"
zsafeStorageKey     "afe75eea0d3c4bd9a65439a54eabc988"
syncV2TestPrivateKeys.pcik "8CogpzApFRI1AeeaGqS/j3lFtKzLZcUjQxlH843yE30="  # base64 32B (Ed25519?) [I]
syncV2TestPrivateKeys.pcek "IEQpDf07mUSdOtfj2+fsme5jkcxIj5oc2DqMDI9q+Wk="
legacyDataUtilsAesKey       "eEH6Qo6+URg6QCZoYnQ+pQ=="
genServicesDecodeAesKey     "LCEW+XgyWHheUbHs4bSm4Q=="
genServicesDecryptAesKey    "b419830e37258e92d57adcfb0c65920d"
callV2TransportAesKey       "yjAF9oqMWl6XfXYJn9mA7w=="
legacyNativeMigrationAesKey "S2h1ZUJlb0RvbTE5MDg4NQ=="
MAP_API_KEY         "AIzaSyBkeSHnerWPkqVM1-b5kk1xvtol7qAOi-A"  # Google Maps, irrelevant
ZLOG_MODULE_KEY     key "vOTH6sd9pNWjRriqCc7rDxs01lwHzfr3" iv "7eb5dbe084b7eedeef04e2622d46ba00"
```

## 2. Crypto primitives [O]

`$znode.crypto` wraps Node crypto with defaults:

- `encrypt(text, algo, key, iv = Buffer.alloc(16,0), opts)` → **IV default = 16 zero bytes**, PKCS7 (node default).
- Key handling: `opts.key="utf-8"` + string key → `Buffer.from(key,"utf-8")`.
- So `encodeAES(key, text, outHex, upper)` = **AES-256-CBC, IV=0…, PKCS7**, output hex/base64.
- `decrypt(text, "aes-256-cbc", key, iv?, opts{input:"base64"})` — same zero-IV default.
- `createHash("sha256").update(s).digest("hex")`.
- RSA `publicEncrypt`/`privateDecrypt` also exposed (E2EE paths; not needed for password login) [O].

### 2.1 ParamCipher (class y) — per-request param encryption "enc_ver: v2" [O]

**[O] IMPLEMENTED** in `src/param_cipher.rs`. Exact formulas, all confirmed from
`compact-app-pc-*.js` (const `R = zsecureSecrets().getParamCipherKey()`):

```
zcid     = AES256CBC(key=paramCipherKey, IV=0, PKCS7,
                     "{type},{imei},{firstLaunchTime}").hex().toUpperCase()
zcid_ext = randomString()            # rand(6..12) hex chars from Math.random

createEncryptKey():
  n = md5(zcid_ext).hexdigest().toUpperCase()
  (n_even, _)      = processStr(n)     # even = chars at index 0,2,4…
  (a_even, a_odd)  = processStr(zcid)
  key = n_even[0..8] + a_even[0..12] + reverse(a_odd)[0..12]     # 32 chars

getParams() → { zcid, zcid_ext, enc_ver: "v2" }

encryptParam(tag):
  m = { ...getParams(), params: base64(AES256CBC(key, JSON.stringify(payload))) }
  m.type           = apiType            # 30
  m.client_version = apiVersion         # 693
  m.signkey        = md5("zsecure" + tag + concat(sorted values of m))
  → sent as URL query params (flattened), NOT as `params=`
```

[O] The `enk` (= derived `key`) travels in the query so the server can encrypt the
response; `decryptResp` then AES-decrypts `data` with it.

Gating: `enable_encrypt_param` fetched from server config (getServerInfo, 5 s timeout; persisted via electron-store). **Live test shows the gate is effectively always on** — plain requests get 18060. [O]

### 2.2 PasswordCipher (class N) — loginByPasswv2 password field [O]

```text
salt = randomString(8)
enk_src = sha256("{passwordCipherKey}-{phone}-{imei}-{salt}").hexdigest().upper()
(even, odd) = split chars by parity
enk = even(enk_src)[0..16] + reverse(odd(enk_src))[0..16]        # 32 ASCII chars = AES-256 key
encryptPassword(pwd) = base64(AES256CBC(key=enk, IV=0, salt + reverse(pwd)))
```

No RSA in the password path. [O]

## 3. Domain resolution [O]

```text
CONFIG_DOMAIN default: "zaloapp.com"        # let Ye="zaloapp.com"; override via launch param zdmapi
authDomain default:    https://wpa.{CONFIG_DOMAIN}   # override via install-config param zauthdm (must start with https)
domain                 chat.{CD}     cookieDomain .{CD}
apiChatDomain          https://chat-wpa.{CD}
apiConversationDomain  https://convers-wpa.api.{CD}
apiFriendDomain        https://friend-wpa.{CD}
apiProfileDomain       https://profile-wpa.{CD}
apiGroupDomain         https://group-wpa.{CD}
apiFileDomain          https://files-wpa.{CD}
apiStickerDomain       https://sticker-wpa.{CD}
apiMediaStoreDomain    https://media-wpa.{CD}
apiVoiceCallDomain     https://voicecall-wpa.{CD}
apiLabelDomain         https://label-wpa.api.{CD}
apiAliasDomain         https://alias-wpa.api.{CD}
apiBoardDomain         https://board-wpa.{CD}
apiGroupBoard          https://groupboard-wpa.api.{CD}
apiLanFriend           https://friendlan-wpa.api.{CD}
apiCloudDomain         https://group-cm.api.{CD}
sp_domain              https://spcontact-wpa.api.{CD}/
apiLogin               https://chat.{CD}/login
apiLoginOnAuth         https://id.{CD}/account/login
apiLogout              https://id.{CD}/account/logout
```

Cookie domain `.{CD}`; requests `withCredentials:true` → **cookie/session-based auth**. [O]
Real runtime authDomain value from server config not yet captured. [U] (`vlogin.zaloapp.com` seen in old docs — unconfirmed here.)

## 4. Common params & constants [O]

```text
zpw_ver  = 693          # apiVersion default; overridable via stored item "zpw_ver"
zpw_type = 30           # apiType (31 for embed bundle)
secretKey = "zpw_enk"   # legacy md5 sign key
common params object: { zpw_ver, zpw_type } (+ ref=origin on cross-domain)
```

## 5. Login endpoints [O]

**[O] VERIFIED LIVE 2026-10-20:** `loginByPasswv2` with plain `params=` returns
`error_code 18060 "Invalid encryption protocol"`. The unencrypted branch is **rejected**
by this account/server tier. ParamCipher v2 (§2.1) is mandatory — there is no plain fallback.

Note this is a *protocol* rejection: the password is never evaluated, so 18060 says nothing
about credential correctness. Do not treat it as a wrong-password signal.

**[O] VERIFIED LIVE, round 2:** with ParamCipher v2 the server answered `error_code 0`
with an encrypted `data` blob. Decrypting that blob with our derived `enk` yields:

```json
{"error_code":18022,"error_message":"Invalid Signature","data":null}
```

Two conclusions, both important:

1. **The `enk` derivation is byte-exact.** If `createEncryptKey` were wrong, AES decryption
   with PKCS7 would fail outright. It decrypts cleanly → `zcid`, `zcid_ext`, `processStr`
   and the key assembly are all correct.
2. **The rejection is now at the signature layer, not the crypto layer** — progress moved
   from 18060 (protocol) to 18022 (signature).

[O] The real problem was **field-set drift** in the signed payload. Bundle:
`salt` and the encrypted `password` are set *before* `encryptParam`; `ref` is assigned
*after* `encryptParam` and therefore excluded from `signkey`. Our first attempt put
`client_time` in the payload (not in the bundle) and included `ref` in the signature.

```text
/api/login/reqqr                    GET   QR request
/api/login/getCaptcha               POST  captcha
/api/login/loginByPassw             POST  v1 password login (fallback)
/api/login/loginByPasswv2           POST  v2 password login
/api/login/loginByResetPwd          POST
/api/login/getLoginInfo             POST/GET auto-login with cookie
/api/login/logOut, /api/v2/login/logOut
/api/login/getServerInfo            server config (gates encryption)
/api/login/setting                  user settings (cmd 12113)
/api/login/actlistv2                activity list
/api/login/chkUpdate, popup, getSupportCountry, hasNewMsg,
/api/login/getActiveCodeByZalo, getFbAuthInfo, fbAuth, linkFacebook,
/api/login/loginByFacebook(WithSecure), fblogout, migrateSS, ssoSyncMp3, completeRemoveData
/api/enterprise/login/emailPassword
```

cmd IDs seen: reqqr=11133, getLoginInfo=11134, logOut=11135, loginByPasswv2=11176, getCaptcha=11325, getServerInfo=11323, setting/me=12113. [O]

### 5.1 signInPasswordV2 request [O]

**[O] Verbatim flow from `compact-app-pc-*.js`:**

```js
p = authDomain + "/api/login/loginByPasswv2"
g = { phone_num, iso_country_code, password, cptch_token, cptch_answer,
      make_answer, language, logged_uids, computer_name, imei }
if (enable_encrypt_param) { n = new N({phone, imei}); g.salt = n.salt; g.password = n.encryptPassword(t) }
l && (g.quest_cert = l);  c && (g.cptch_cert = c)
h && (g.last_uid = h);    m && (g.viewerkey = m)
{params: f, enk: _} = encryptParam(g, "loginbypasswv2")
le && (f.ref = window.location.origin)        // ← AFTER signing
_post(p, _constructUrlParams(f), null, 11176)
decryptResp(_, E, ...)
```

Ordering rule that bit us: **anything added after `encryptParam` is not in `signkey`.**
`ref` is such a field. Any field added *before* it is.

cmd id 11176. v1 fallback: plain POST `/api/login/loginByPassw` + signkey("loginbypassw"). [O]

### 5.2 QR login [O] — resolved, live-verified 2026-10-20

GET `{authDomain}/api/login/reqqr` params `{language, client_time(unix), imei, computer_name, logged_uids, self_g, qr_form:0}`
plus `type=30`, `client_version=693`, `signkey=md5("zsecure"+"reqqr"+values sorted by key)`. The plain
(unencrypted) form is accepted by the server.

`self_g` picks the flavour of code:

| self_g | server answers | QR content |
|---|---|---|
| 0 | `base64_qr` — a rendered PNG (22 KB) | server-side image |
| 1 | `qr_cnt` — a URL | `http://zaloapp.com/qr/l?tk=zaloqr:v2_…` |

Both carry the same `token_id` (`zaloqr:v2_…`); the PNG encodes the same URL. Server config today is
`setttings.authen.trusted_device = {self_g: 0, qr_format_trust_device: 0, logo_qr: 0}` — the
device-linking ("trust device") flavour is **off**, so the payload path is the live one. [O]

Reply fields (snake_case):

```text
qr_timeout     180      seconds the code stays valid
qr_retry       3        how many times the client may re-poll
token_id       "zaloqr:v2_…"
qr_cnt         "http://zaloapp.com/qr/l?tk=…"    (self_g=1)
base64_qr      "<PNG base64>"                    (self_g=0)
chk_wait_scan  "https://qr-id.zaloapp.com/polling/qr/waiting?src=pc&code=…"
chk_wait_cfirm "https://qr-id.zaloapp.com/polling/qr/pc-status?src=pc&code=…"
```

The poll mechanism **is** in the bundle — it is not `qrstatus`/`checkqrcode`, it is the URL the server
hands back:

1. `chk_wait_scan` is a long poll. The client appends `&lang=vi` and loops with `qr_timeout` /
   `qr_retry`. While nobody has scanned it answers **HTTP 408** — that is "still waiting", not an
   error. [O]
2. On a successful scan it returns the preview payload (display name, avatar, `link_data`). [I]
3. `chk_wait_cfirm` (`/polling/qr/pc-status`) turns the confirmed scan into a session. Before the scan
   it answers `18012 Invalid status`. [O]

Bundle path: `login-startup.c06a42b8d2b42a0bedf3.js` → `getQRCode` (core,
`default-login-main-startup-*.js`) → `genFirstQR` → `verifyDataWaitingScan` → `genUrlWaitingConfirm`. [O]

`link_data` = `{enc_link_data, enc_link_hmac}` → `createCompanionLinkingProof` →
`encLinkingMetaData`/`encAccountSign`/`encDeviceSign`/`encPK`, appended to the confirm URL. That is the
trust-device proof, and it is only produced when `qr_format_trust_device` is on — the live server has
it off, so the confirm URL is used as-is. [I]

Implementation: `src/qr.rs` (ticket → terminal art → wait → confirm), driven by `zalo-rust --qr`. [O]

## 6. Session artifacts [O/U]

- Login success sets cookies on `.{CONFIG_DOMAIN}` (session cookie names not yet observed — need runtime capture). [U]
- Auto-login: call `getLoginInfo` with existing cookie. [O]
- imei + computer_name are client-generated identifiers persisted locally; Rust impl can generate & persist its own. [I]

## 7. Rust implementation checklist (pre-login)

1. AES-256-CBC helper: IV=zeros, PKCS7, key from UTF-8 string. (`aes` + `cbc` crates or `openssl`)
2. md5/sha256 helpers; even/odd char split util (`processStr`).
3. ParamCipher v2 + signkey (md5 "zsecure"+tag+sorted-values).
4. PasswordCipher for loginByPasswv2.
5. HTTP client with cookie jar on `.{CONFIG_DOMAIN}`, common params `{zpw_ver:693, zpw_type:30}`.
6. Start unencrypted (`enable_encrypt_param=false`), handle 19000 → retry plain; upgrade path later.
7. Login flow: getServerInfo → (captcha?) → loginByPasswv2 → cookies → getLoginInfo/setting/me.

## 8. Open questions

- [O] QR poll endpoint — `chk_wait_scan` (long poll, 408 = waiting) + `chk_wait_cfirm`. See §5.2.
- [U] What `chk_wait_cfirm` returns after a real scan (cookie names, session blob) — needs one live scan.
- [U] Real cookie names (`sessionid`? `token`? `is_zpw_login*`?) — capture at runtime or dig response handling.
- [U] Whether server rejects our hardcoded zpw_ver=693 (version pinning).
- [U] captcha flow internals (getCaptcha fields) — only needed on failed attempts.
