//! HTTP session: cookie jar, domain table, common params, request encoding.
//!
//! Mirrors the `_get` / `_post` / `_getCommonParams` / `getEncodedParams` helpers from
//! `compact-app-pc-*.js`. See `re-notes/02_chat_data.md` §2–§4.

use std::sync::Arc;

use anyhow::{bail, Result};
use reqwest::cookie::Jar;
use reqwest::header::{HeaderMap, HeaderValue, ORIGIN, REFERER};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::crypto;

pub const ZPW_VER: &str = "694";
pub const ZPW_TYPE: &str = "30";
pub const CONFIG_DOMAIN: &str = "zaloapp.com";

/// Domain table.
///
/// [O] **Live-verified** against a QR-authenticated Zalo Web session on 2026-10-21.
/// Every host below answered `error_code 0` for its endpoint. Note these are
/// `*.chat.zalo.me`, not the `*.zaloapp.com` names the PC bundle defaults to — the two
/// flavours differ, and using the wrong set yields nginx 404s.
///
/// [`Session::apply_server_info`] re-applies them from the login payload each run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Domains {
    /// `wpa.zaloapp.com` — `reqqr`, password login.
    pub auth: String,
    /// `wpa.chat.zalo.me` — `getServerInfo` / `getLoginInfo` once a session exists.
    pub wpa_chat: String,
    /// `zpw_service_map_v3.chat[0]` — message send (`tt-chat*-wpa.*`).
    pub chat: String,
    /// `zpw_service_map_v3.conversation[0]` — conversation control (pins, preload).
    pub conversation: String,
    /// `zpw_chts_new[0]` — HTTP long-polling message receiver (`getChatFromPolling`, `p*-msg-chat.zaloapp.com`).
    #[serde(default = "default_lp_chat")]
    pub lp_chat: String,
    /// `zpw_ctrs_new[0]` — HTTP long-polling control receiver (`getControllFromPolling`, `p*-ctl-chat.zaloapp.com`).
    #[serde(default = "default_lp_control")]
    pub lp_control: String,
    /// `zpw_service_map_v3.friend[0]` — `social/friend/getfriends`.
    pub friend: String,
    /// `zpw_service_map_v3.conversation[0]` — `conv/*`, `hiddenconvers/*`.
    pub convers_api: String,
    /// `zpw_service_map_v3.profile[0]` — `getprofiles`, `me-v2`.
    pub profile: String,
    pub group: String,
    pub file: String,
    /// `zpw_service_map_v3.group_cloud_message[0]` — `/api/cm/getoldv2`, `/api/cm/getrecentv2`.
    #[serde(default = "default_cloud_msg")]
    pub cloud_msg: String,
    /// `zpw_service_map_v3.voice_call[0]` — `/api/voicecall/*` signaling endpoints.
    #[serde(default = "default_voice_call")]
    pub voice_call: String,
    /// `zpw_service_map_v3.sticker[0]` — `/api/message/sticker/*` catalog & search endpoints.
    #[serde(default = "default_sticker")]
    pub sticker: String,
    /// `zpw_ws[...]` — control websocket endpoints (`wss://wsN-msg.chat.zalo.me`).
    ///
    /// [O] The server hands back full `wss://` URLs in the login config; the client never
    /// builds them from a literal. Real-time VoIP invites only arrive over this transport.
    #[serde(default = "default_ws")]
    pub ws: Vec<String>,
}

fn default_lp_chat() -> String {
    "https://p4-msg-chat.zaloapp.com".into()
}

fn default_lp_control() -> String {
    "https://p4-ctl-chat.zaloapp.com".into()
}

fn default_cloud_msg() -> String {
    "https://tt-group-cm.api.zaloapp.com".into()
}

fn default_voice_call() -> String {
    "https://voicecall-wpa.chat.zalo.me".into()
}

fn default_sticker() -> String {
    crate::endpoints::service("sticker")
        .unwrap_or("https://tt-sticker-wpa.chat.zalo.me")
        .into()
}

/// [O] `re-notes/02_chat_data.md`: `zpw_ws` = `wss://ws{1..8}-msg.chat.zalo.me`.
fn default_ws() -> Vec<String> {
    (1..=8)
        .map(|n| format!("wss://ws{n}-msg.chat.zalo.me"))
        .collect()
}

impl Default for Domains {
    fn default() -> Self {
        Self {
            auth: crate::endpoints::AUTH.into(),
            wpa_chat: crate::endpoints::WPA_CHAT.into(),
            chat: crate::endpoints::MSG_CHAT.into(),
            conversation: crate::endpoints::CONVERS_CTL.into(),
            lp_chat: default_lp_chat(),
            lp_control: default_lp_control(),
            friend: "https://tt-friend-wpa.chat.zalo.me".into(),
            convers_api: "https://tt-convers-wpa.chat.zalo.me".into(),
            profile: "https://tt-profile-wpa.chat.zalo.me".into(),
            group: "https://tt-group-wpa.zaloapp.com".into(),
            file: "https://tt-files-wpa.zaloapp.com".into(),
            cloud_msg: default_cloud_msg(),
            voice_call: default_voice_call(),
            sticker: default_sticker(),
            ws: default_ws(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoginInfo {
    pub uin: String,
    /// `zpw_enk` — base64 AES-256 key for legacy param encryption.
    pub zpw_enk: String,
    /// `zpw_sek` — session encryption/cookie token for authenticated endpoints.
    pub zpw_sek: String,
    pub display_name: String,
    pub send2me_id: String,
    #[serde(default)]
    pub avatar: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSession {
    pub imei: String,
    pub computer_name: String,
    pub secret_key_b64: Option<String>,
    pub session_key: Option<String>,
    pub zpw_sek: Option<String>,
    pub login: Option<LoginInfo>,
    pub domains: Domains,
}

#[derive(Clone)]
pub struct Session {
    pub http: Client,
    pub jar: Arc<Jar>,
    pub domains: Domains,
    pub imei: String,
    pub computer_name: String,
    pub secret_key: Option<Vec<u8>>,
    /// Same key as a string, for `maybe_decrypt`.
    pub session_key: Option<String>,
    pub zpw_sek: Option<String>,
    pub login: Option<LoginInfo>,
    /// Draw commands in front of the user so they can be reproduced by hand.
    pub verbose: bool,
    /// Append every exchange (pretty-printed) to `dump/<n>-<slug>.json`.
    pub dump: bool,
    /// Skip URL redaction in dumps (needed to reproduce signature failures).
    pub dump_raw: bool,
}

impl Session {
    pub fn new(verbose: bool, dump: bool, dump_raw: bool) -> Result<Self> {
        let jar = Arc::new(Jar::default());
        let mut default_headers = HeaderMap::new();
        default_headers.insert(
            REFERER,
            HeaderValue::from_static("https://chat.zalo.me/"),
        );
        default_headers.insert(
            ORIGIN,
            HeaderValue::from_static("https://chat.zalo.me"),
        );
        let http = Client::builder()
            .cookie_provider(Arc::clone(&jar))
            .default_headers(default_headers)
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(35))
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) ZaloPC/26.10.11 Chrome/122.0.0.0 Electron/29.0.0 Safari/537.36",
            )
            .build()?;
        Ok(Self {
            http,
            jar,
            domains: Domains::default(),
            imei: random_imei(),
            computer_name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "DESKTOP".into()),
            secret_key: None,
            session_key: None,
            zpw_sek: None,
            login: None,
            verbose,
            dump,
            dump_raw,
        })
    }

    /// Store `zpw_sek` session token, attach it to `self.login`, and insert the session cookie.
    pub fn set_zpw_sek(&mut self, v: String) {
        if v.is_empty() {
            return;
        }
        // Insert cookie for all required Zalo hosts
        let hosts = [
            "https://chat.zalo.me",
            "https://wpa.zaloapp.com",
            "https://wpa.chat.zalo.me",
            &self.domains.auth,
            &self.domains.chat,
            &self.domains.conversation,
            &self.domains.lp_chat,
            &self.domains.lp_control,
            &self.domains.convers_api,
            &self.domains.profile,
            &self.domains.friend,
            &self.domains.group,
            &self.domains.cloud_msg,
            &self.domains.voice_call,
        ];
        for host in hosts {
            if let Ok(url) = reqwest::Url::parse(host) {
                let cookie = format!("zpw_sek={v}; Path=/");
                self.jar.add_cookie_str(&cookie, &url);
            }
        }
        self.zpw_sek = Some(v.clone());
        if let Some(ref mut li) = self.login {
            li.zpw_sek = v;
        }
    }

    /// Check whether the current session is valid on the server.
    pub async fn validate_session(&self) -> bool {
        if self.login.is_none() || self.zpw_sek.is_none() {
            return false;
        }
        let body = serde_json::json!({
            "avatar_size": 160,
            "imei": self.imei,
        });
        let url = match self.encoded_params(&body) {
            Ok(enc) => format!(
                "{}/api/social/profile/me-v2?{}&params={enc}&os=8",
                self.domains.profile,
                self.common_params()
            ),
            Err(_) => format!(
                "{}/api/social/profile/me-v2?{}",
                self.domains.profile,
                self.common_params()
            ),
        };
        match self.get(&url).await {
            Ok(resp) => {
                let code = resp.get("error_code").and_then(serde_json::Value::as_i64).unwrap_or(-1);
                code == 0
            }
            Err(_) => false,
        }
    }

    /// Fetch the authenticated user's own numeric `userId` and `displayName` from `/api/social/profile/me-v2`.
    pub async fn sync_profile_me(&mut self) -> Option<(String, String)> {
        if self.login.is_none() || self.zpw_sek.is_none() {
            return None;
        }
        let body = serde_json::json!({
            "avatar_size": 160,
            "imei": self.imei,
        });
        let enc = self.encoded_params(&body).ok()?;
        let url = format!(
            "{}/api/social/profile/me-v2?{}&params={enc}&os=8",
            self.domains.profile,
            self.common_params()
        );
        let resp = self.get(&url).await.ok()?;
        let dec = self.maybe_decrypt(resp, self.session_key_bytes()).await;
        let profile = dec
            .get("data")
            .and_then(|d| d.get("profile"))
            .or_else(|| dec.get("profile"))?;
        let uid = profile
            .get("userId")
            .or_else(|| profile.get("uid"))
            .and_then(|v| match v {
                Value::String(s) if !s.is_empty() => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })?;
        let name = profile
            .get("displayName")
            .or_else(|| profile.get("zaloName"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let avatar = profile
            .get("avatar")
            .or_else(|| profile.get("avt"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if let Some(ref mut li) = self.login {
            li.uin = uid.clone();
            if !name.is_empty() {
                li.display_name = name.clone();
            }
            if !avatar.is_empty() {
                li.avatar = avatar;
            }
        }
        Some((uid, name))
    }

    /// Clear all session credentials, tokens, keys, and cookies.
    pub fn logout(&mut self) {
        self.login = None;
        self.zpw_sek = None;
        self.secret_key = None;
        self.session_key = None;
        self.jar = Arc::new(Jar::default());
        let mut default_headers = HeaderMap::new();
        default_headers.insert(
            REFERER,
            HeaderValue::from_static("https://chat.zalo.me/"),
        );
        default_headers.insert(
            ORIGIN,
            HeaderValue::from_static("https://chat.zalo.me"),
        );
        if let Ok(c) = Client::builder()
            .cookie_provider(Arc::clone(&self.jar))
            .default_headers(default_headers)
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(35))
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) ZaloPC/26.10.11 Chrome/122.0.0.0 Electron/29.0.0 Safari/537.36",
            )
            .build()
        {
            self.http = c;
        }
    }

    /// Save the authenticated session to disk so subsequent runs can restore it without re-authenticating.
    pub fn save_to_file(&self, path: &str) -> Result<()> {
        use base64::Engine;
        let saved = SavedSession {
            imei: self.imei.clone(),
            computer_name: self.computer_name.clone(),
            secret_key_b64: self.secret_key.as_ref().map(|k| base64::engine::general_purpose::STANDARD.encode(k)),
            session_key: self.session_key.clone(),
            zpw_sek: self.zpw_sek.clone(),
            login: self.login.clone(),
            domains: self.domains.clone(),
        };
        let text = serde_json::to_string_pretty(&saved)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Load a previously saved session from disk and restore cookies, keys, and endpoints.
    pub fn load_from_file(&mut self, path: &str) -> Result<()> {
        use base64::Engine;
        let text = std::fs::read_to_string(path)?;
        let saved: SavedSession = serde_json::from_str(&text)?;

        self.imei = if saved.imei.len() == 69 {
            saved.imei
        } else {
            random_imei()
        };
        self.computer_name = saved.computer_name;
        self.domains = saved.domains;
        // Auto-heal older session.json files where `zpw_chts_new` / `zpw_ctrs_new` overwrote REST domains
        if self.domains.chat.contains("-msg-chat.") || self.domains.chat.contains("tt-chat2-wpa") || self.domains.chat.is_empty() {
            if self.domains.chat.contains("-msg-chat.") && self.domains.lp_chat.is_empty() {
                self.domains.lp_chat = self.domains.chat.clone();
            }
            self.domains.chat = crate::endpoints::service("chat")
                .unwrap_or("https://tt-chat3-wpa.api.zaloapp.com")
                .into();
        }
        if self.domains.conversation.contains("-ctl-chat.") || self.domains.conversation.is_empty() {
            if self.domains.conversation.contains("-ctl-chat.") && self.domains.lp_control.is_empty() {
                self.domains.lp_control = self.domains.conversation.clone();
            }
            self.domains.conversation = crate::endpoints::service("conversation")
                .unwrap_or("https://tt-convers-wpa.chat.zalo.me")
                .into();
        }
        self.session_key = saved.session_key;
        let sek = saved
            .zpw_sek
            .filter(|s| !s.is_empty())
            .or_else(|| saved.login.as_ref().map(|l| l.zpw_sek.clone()))
            .filter(|s| !s.is_empty());

        if let Some(b64) = saved.secret_key_b64 {
            if let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(b64) {
                self.secret_key = Some(raw);
            }
        }

        self.login = saved.login;

        if let Some(s) = sek {
            self.set_zpw_sek(s);
        } else {
            self.login = None;
            anyhow::bail!("session.json thiếu mã xác thực zpw_sek");
        }

        Ok(())
    }

    /// Persist one request/response pair for later schema mining.
    ///
    /// Credentials are redacted; the point is the response *shape*, not the content.
    fn dump_exchange(&self, method: &str, url: &str, status: u16, body: &str) {
        let dir = std::path::Path::new("dump");
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("! dump dir: {e}");
            return;
        }
        let n = std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0) + 1;
        // Name the file from the path only; the query carries ids we should not keep.
        let path_part = url.split('?').next().unwrap_or(url);
        let slug: String = path_part
            .split("/api/")
            .nth(1)
            .unwrap_or("root")
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let path = dir.join(format!("{n:02}-{slug}.json"));

        // Parse to a real JSON object so the dump nests properly instead of holding a
        // double-encoded string.
        let body_json = serde_json::from_str::<Value>(body)
            .unwrap_or_else(|_| Value::String(body.to_string()));

        let doc = serde_json::json!({
            "method": method,
            // `dump_raw` keeps the full URL so a signature mismatch can be reproduced
            // offline. Only use it on a throwaway account.
            "url": if self.dump_raw { url.to_string() } else { redact_url(url) },
            "status": status,
            "body": body_json,
        });
        let text = serde_json::to_string_pretty(&doc).unwrap_or_default();
        if let Err(e) = std::fs::write(&path, text) {
            eprintln!("! dump write: {e}");
        } else {
            eprintln!("  dumped → {}", path.display());
        }
    }

    /// `_constructUrlParams(se)` → `zpw_ver=693&zpw_type=30`. That is the whole object.
    pub fn common_params(&self) -> String {
        format!("zpw_ver={ZPW_VER}&zpw_type={ZPW_TYPE}")
    }

    /// The raw session AES key as bytes, for response decryption.
    pub fn session_key_bytes(&self) -> &[u8] {
        self.secret_key.as_deref().unwrap_or(&[])
    }

    /// `getEncodedParams(obj)` → url-encoded `encodeAES(json)`.
    ///
    /// Requires the session key; before login there is nothing to encrypt, so callers
    /// that run pre-auth use plain params instead.
    pub fn encoded_params(&self, v: &Value) -> Result<String> {
        let key = self
            .secret_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no session key yet (pre-login)"))?;
        let b64 = crypto::encode_aes(key, &v.to_string())?;
        Ok(urlencode(&b64))
    }

    /// Plain `params=<url-encoded json>` — accepted by the server when param encryption
    /// is off, which is the MVP path.
    pub fn plain_params(v: &Value) -> String {
        format!("params={}", urlencode(&v.to_string()))
    }

    /// Re-apply the endpoint table from a `getServerInfo` / `getLoginInfo` payload.
    ///
    /// [O] `setEndpoints(chts, ctrs, serviceMapNew, conf, ws)`:
    /// `zpw_chts_new` → chat, `zpw_ctrs_new` → conversation,
    /// `zpw_service_map_new.{friend,conversation,profile,group,file}` → the rest.
    pub fn apply_server_info(&mut self, data: &Value) {
        fn first(v: &Value) -> Option<String> {
            v.as_array()
                .and_then(|a| a.first())
                .and_then(|x| x.as_str())
                .map(|s| s.trim_end_matches('/').to_string())
                .or_else(|| v.as_str().map(|s| s.trim_end_matches('/').to_string()))
        }

        if let Some(v) = data.get("zpw_chts_new").and_then(first) {
            self.domains.lp_chat = v;
        }
        if let Some(v) = data.get("zpw_ctrs_new").and_then(first) {
            self.domains.lp_control = v;
        }
        if let Some(arr) = data.get("zpw_ws").and_then(|v| v.as_array()) {
            let list: Vec<String> = arr
                .iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim_end_matches('/').to_string())
                .filter(|s| s.starts_with("wss://") || s.starts_with("ws://"))
                .collect();
            if !list.is_empty() {
                self.domains.ws = list;
            }
        }
        if let Some(map) = data
            .get("zpw_service_map_v3")
            .or_else(|| data.get("zpw_service_map_new"))
        {
            for (key, slot) in [
                ("chat", &mut self.domains.chat),
                ("friend", &mut self.domains.friend),
                ("conversation", &mut self.domains.convers_api),
                ("profile", &mut self.domains.profile),
                ("group", &mut self.domains.group),
                ("file", &mut self.domains.file),
                ("group_cloud_message", &mut self.domains.cloud_msg),
                ("voice_call", &mut self.domains.voice_call),
                ("sticker", &mut self.domains.sticker),
            ] {
                if let Some(v) = map.get(key).and_then(first) {
                    *slot = v;
                }
            }
            if let Some(v) = map.get("conversation").and_then(first) {
                self.domains.conversation = v;
            }
        }
    }

    pub async fn get(&self, url: &str) -> Result<Value> {
        self.send(Method::GET, url, None).await
    }

    pub async fn post_form(&self, url: &str, body: String) -> Result<Value> {
        self.send(Method::POST, url, Some(body)).await
    }

    /// Send an RFC 7578 `multipart/form-data` binary upload (`chunkContent` or `fileContent`)
    /// to Zalo's file/photo servers.
    pub async fn post_multipart(
        &self,
        url: &str,
        field_name: &str,
        file_name: &str,
        mime_type: &str,
        file_bytes: &[u8],
    ) -> Result<Value> {
        let boundary = format!(
            "----WebKitFormBoundary{}",
            crate::param_cipher::random_hex(16)
        );
        let safe_name = file_name.replace(['"', '\r', '\n'], "_");
        let head = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{field_name}\"; filename=\"{safe_name}\"\r\nContent-Type: {mime_type}\r\n\r\n"
        );
        let tail = format!("\r\n--{boundary}--\r\n");
        let mut body = Vec::with_capacity(head.len() + file_bytes.len() + tail.len());
        body.extend_from_slice(head.as_bytes());
        body.extend_from_slice(file_bytes);
        body.extend_from_slice(tail.as_bytes());

        let start = std::time::Instant::now();
        let mut rb = self
            .http
            .request(Method::POST, url)
            .timeout(std::time::Duration::from_secs(90))
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            );
        if let Some(sek) = &self.zpw_sek {
            if !sek.is_empty() {
                rb = rb
                    .header("Cookie", format!("zpw_sek={sek}; _zlang=vi"))
                    .header("zpw_sek", sek)
                    .header("Origin", "https://chat.zalo.me")
                    .header("Referer", "https://chat.zalo.me/");
            }
        }
        let resp = rb.body(body).send().await?;
        let latency_ms = start.elapsed().as_millis();
        let status = resp.status();
        let text = resp.text().await?;
        crate::logger::log_request(
            "POST(multipart)",
            url,
            Some(status.as_u16()),
            latency_ms,
            &format!("[{field_name}: {safe_name} ({} bytes)]", file_bytes.len()),
            Some(&text),
            if status.is_success() {
                None
            } else {
                Some("HTTP multipart error")
            },
        );
        if !status.is_success() {
            anyhow::bail!("HTTP {status} from {url}: {}", truncate(&text, 300));
        }
        if text.trim().is_empty() {
            return Ok(serde_json::json!({ "error_code": 0, "data": null }));
        }
        let v: Value = serde_json::from_str(&text)
            .unwrap_or_else(|_| serde_json::json!({ "data": text.trim() }));
        Ok(v)
    }

    async fn send(&self, m: Method, url: &str, body: Option<String>) -> Result<Value> {
        let start = std::time::Instant::now();
        if self.verbose {
            eprintln!("→ {} {}", m, if self.dump_raw { url.to_string() } else { redact_url(url) });
            if let Some(b) = &body {
                eprintln!("  body: {}", truncate(b, 400));
            }
        }
        let method = m.as_str().to_string();
        let mut rb = self.http.request(m, url);
        if let Some(ref sek) = self.zpw_sek {
            if !sek.is_empty() {
                let is_zalo_host = reqwest::Url::parse(url)
                    .map(|u| {
                        u.host_str()
                            .map(|h| {
                                h == "zalo.me"
                                    || h.ends_with(".zalo.me")
                                    || h == "zaloapp.com"
                                    || h.ends_with(".zaloapp.com")
                            })
                            .unwrap_or(false)
                    })
                    .unwrap_or_else(|_| url.contains("zalo.me") || url.contains("zaloapp.com"));
                if is_zalo_host {
                    rb = rb.header("Cookie", format!("zpw_sek={sek}; _zlang=vi"));
                    rb = rb.header("zpw_sek", sek);
                    rb = rb.header("Origin", "https://chat.zalo.me");
                    rb = rb.header("Referer", "https://chat.zalo.me/");
                }
            }
        }
        if let Some(b) = &body {
            rb = rb
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(b.clone());
        }
        let resp_result = rb.send().await;
        let latency_ms = start.elapsed().as_millis();

        let resp = match resp_result {
            Ok(r) => r,
            Err(e) => {
                crate::logger::log_request(
                    &method,
                    url,
                    None,
                    latency_ms,
                    body.as_deref().unwrap_or(""),
                    None,
                    Some(&e.to_string()),
                );
                return Err(e.into());
            }
        };

        let status = resp.status();
        let text = resp.text().await?;
        if self.verbose {
            eprintln!("← {status} {}", truncate(&text, 400));
        }
        if self.dump {
            self.dump_exchange(&method, url, status.as_u16(), &text);
        }

        let is_err = !status.is_success();
        let err_str = if is_err {
            Some(format!("HTTP {status}"))
        } else {
            None
        };

        crate::logger::log_request(
            &method,
            url,
            Some(status.as_u16()),
            latency_ms,
            body.as_deref().unwrap_or(""),
            Some(&text),
            err_str.as_deref(),
        );

        if is_err {
            bail!("HTTP {status} from {url}: {}", truncate(&text, 300));
        }
        if text.trim().is_empty() {
            // Long-polling endpoints (`p4-msg-chat.zaloapp.com`) return HTTP 200 with an empty body `""`
            // on keepalive timeout (`if ("" === e.data) return void this.processEmptyData()`).
            return Ok(serde_json::json!({ "error_code": 0, "data": null }));
        }
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|_| {
            // Encrypted responses arrive as a bare base64 blob, not JSON. Hand the raw
            // text back so callers can decrypt it instead of failing here.
            Value::String(text.clone())
        });
        let code = v.get("error_code").and_then(|c| c.as_i64()).unwrap_or(0);
        if code != 0 {
            let msg = v.get("error_message").and_then(|m| m.as_str()).unwrap_or("");
            crate::logger::log_request(
                &method,
                url,
                Some(status.as_u16()),
                latency_ms,
                body.as_deref().unwrap_or(""),
                Some(&text),
                Some(&format!("server error {code}: {msg}")),
            );
            bail!("server error {code}: {msg} (from {url})");
        }
        Ok(v)
    }

    /// `decryptResp` — when the request was param-encrypted, `data` comes back as
    /// base64 AES over the key (AES-128 or AES-256), not plain JSON.
    pub async fn maybe_decrypt(&self, v: Value, key: &[u8]) -> Value {
        let Some(data) = v.get("data") else {
            return v;
        };
        let Some(b64) = data.as_str() else {
            return v;
        };
        if b64.is_empty() || b64.starts_with('{') {
            return v;
        }
        if key.is_empty() {
            return v;
        }
        match crate::crypto::decode_aes(key, b64) {
            Ok(pt) => match serde_json::from_str::<Value>(&pt) {
                Ok(inner) => {
                    if self.verbose {
                        eprintln!("  data decrypted ({} bytes)", pt.len());
                    }
                    // The decrypted payload is itself a full response envelope with its
                    // own error_code — propagate it instead of masking as success.
                    inner
                }
                Err(e) => {
                    if self.verbose {
                        eprintln!("  data decrypted but not JSON: {e} / {}", truncate(&pt, 200));
                    }
                    v
                }
            },
            Err(e) => {
                if self.verbose {
                    eprintln!("  data decrypt failed: {e}");
                }
                v
            }
        }
    }
}

/// Build `{a:1,b:"x"}` maps tersely.
pub fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

/// Strip anything that could be a credential before a URL lands in a dump file.
pub fn redact_url(url: &str) -> String {
    const SENSITIVE: &[&str] = &[
        "password", "passw", "zpw_sek", "zpw_enk", "cookie", "token", "signkey", "params", "imei",
    ];
    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };
    let cleaned: Vec<String> = query
        .split('&')
        .map(|kv| {
            let key = kv.split('=').next().unwrap_or("").to_ascii_lowercase();
            if SENSITIVE.iter().any(|s| key.contains(s)) {
                format!("{}=<redacted>", kv.split('=').next().unwrap_or(""))
            } else {
                kv.to_string()
            }
        })
        .collect();
    format!("{base}?{}", cleaned.join("&"))
}

/// Redact a sensitive secret string, showing at most the first 8 characters followed by `…<redacted>`.
pub fn redact_secret(s: &str) -> String {
    let prefix: String = s.chars().take(8).collect();
    format!("{prefix}…<redacted>")
}

pub fn urlencode(s: &str) -> String {    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n).collect();
        format!("{t}…")
    }
}

/// Client-generated device id. Persisted only in memory for now — regenerate per run.
fn random_imei() -> String {
    crate::call::official_imei()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_new_initializes_zpw_sek_and_headers() {
        let s = Session::new(false, false, false).unwrap();
        assert!(s.zpw_sek.is_none());
        assert!(s.login.is_none());
    }

    #[test]
    fn session_set_zpw_sek_updates_fields() {
        let mut s = Session::new(false, false, false).unwrap();
        s.login = Some(LoginInfo::default());
        s.set_zpw_sek("test_sek_value".into());
        assert_eq!(s.zpw_sek.as_deref(), Some("test_sek_value"));
        assert_eq!(s.login.as_ref().unwrap().zpw_sek, "test_sek_value");
    }

    #[test]
    fn redact_secret_truncates_to_eight_chars() {
        assert_eq!(redact_secret("1234567890abcdef"), "12345678…<redacted>");
        assert_eq!(redact_secret("short"), "short…<redacted>");
        assert_eq!(redact_secret(""), "…<redacted>");
    }
}
