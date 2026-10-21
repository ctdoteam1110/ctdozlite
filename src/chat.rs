//! Chat data: friend/conversation list + last messages, all over REST.
//!
//! Notes: `re-notes/02_chat_data.md`.

use anyhow::Result;
use serde_json::{json, Value};

use crate::session::{obj, Session};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub sender: String,
    pub text: String,
    pub time: String,
    pub is_outgoing: bool,
    pub ts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticker_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticker_cate_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticker_type: Option<u32>,
}

impl ChatMessage {
    /// Compute the static PNG URL for a Zalo sticker (`compact-app-pc.b7bec208c4c8f73e09f8.js:7739583` & `18589292`).
    pub fn sticker_image_url(&self) -> Option<String> {
        let sid = self.sticker_id?;
        if sid == 0 {
            return None;
        }
        if self.sticker_cate_id == Some(0) && (1..=30).contains(&sid) {
            Some(format!(
                "https://stc-chat.zdn.vn/images/stickers/default/thumb/{sid}.png"
            ))
        } else {
            Some(format!(
                "https://api.zaloapp.com/api/emoticon/sticker/webpc?eid={sid}&size=130"
            ))
        }
    }

    pub fn sticker_cache_key(&self) -> Option<String> {
        let sid = self.sticker_id?;
        if sid == 0 {
            return None;
        }
        let cid = self.sticker_cate_id.unwrap_or(0);
        Some(format!("stk_{cid}_{sid}"))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Thread {
    pub id: String,
    pub name: String,
    pub preview: String,
    pub time: String,
    pub avatar: Option<String>,
    pub is_group: bool,
    pub messages: Vec<ChatMessage>,
}

/// Load locally cached conversations and message history.
pub fn load_threads_cache() -> Vec<Thread> {
    if let Ok(text) = std::fs::read_to_string("threads_cache.json") {
        if let Ok(threads) = serde_json::from_str::<Vec<Thread>>(&text) {
            return threads;
        }
    }
    Vec::new()
}

/// Persist conversations and message history locally so they survive app restarts.
pub fn save_threads_cache(threads: &[Thread]) {
    if let Ok(text) = serde_json::to_string_pretty(threads) {
        let _ = std::fs::write("threads_cache.json", text);
    }
}

/// `/api/social/friend/getfriends` — the friend list.
///
/// Uses `s.domains.friend` (defaulting to `tt-friend-wpa.chat.zalo.me`), falling back
/// to `s.domains.profile` only if `domains.friend` is empty.
pub async fn get_friends(s: &Session) -> Result<Vec<Thread>> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let body = obj(&[
        ("incInvalid", json!(1)),
        ("page", json!(1)),
        ("count", json!(20000)),
        ("avatar_size", json!(160)),
        ("actiontime", json!(0)),
        ("imei", json!(s.imei)),
    ]);
    let params = match s.encoded_params(&body) {
        Ok(p) => format!("params={p}"),
        Err(e) => {
            crate::logger::log_error("chat::get_friends::encoded_params", &e.to_string());
            eprintln!("! get_friends: no session key for encoded_params ({e}), falling back to plain_params");
            Session::plain_params(&body)
        }
    };
    let hosts = [&s.domains.profile, &s.domains.friend];
    let mut resp_opt = None;
    let mut last_err = None;
    for host in hosts {
        if host.is_empty() {
            continue;
        }
        let url = format!(
            "{}/api/social/friend/getfriends?{}{}",
            host,
            s.common_params(),
            if params.is_empty() { String::new() } else { format!("&{params}") }
        );
        match s.get(&url).await {
            Ok(r) => {
                resp_opt = Some(s.maybe_decrypt(r, s.session_key_bytes()).await);
                break;
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
    }
    let resp = match resp_opt {
        Some(r) => r,
        None => return Err(last_err.unwrap_or_else(|| anyhow::anyhow!("get_friends failed"))),
    };
    let data_root = resp.get("data").unwrap_or(&resp);
    let mut threads: Vec<Thread> = match data_root {
        Value::Array(items) => items.iter().filter_map(parse_thread).collect(),
        Value::Object(o) if o.contains_key("friends") => o["friends"]
            .as_array()
            .map(|a| a.iter().filter_map(parse_thread).collect())
            .unwrap_or_default(),
        Value::Object(o) if o.get("data").and_then(Value::as_array).is_some() => o["data"]
            .as_array()
            .map(|a| a.iter().filter_map(parse_thread).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    // If Cloud của tôi (send2me) is available, place it at the top
    if let Some(li) = &s.login {
        if !li.send2me_id.is_empty() && !threads.iter().any(|t| t.id == li.send2me_id) {
            threads.insert(
                0,
                Thread {
                    id: li.send2me_id.clone(),
                    name: "Cloud của tôi (Lưu trữ)".into(),
                    preview: "".into(),
                    time: crate::ui::clock(),
                    avatar: None,
                    is_group: false,
                    messages: Vec::new(),
                },
            );
        }
    }

    // Merge user groups into threads list
    if let Ok(mut groups) = get_groups(s).await {
        for g in groups.drain(..) {
            if !threads.iter().any(|t| t.id == g.id) {
                threads.push(g);
            }
        }
    }

    // Hydrate with locally persisted history from SQLite (`zalo_history.db`)
    crate::store::load_history_into_threads("zalo_history.db", &mut threads, 200);

    Ok(threads)
}

fn parse_thread(v: &Value) -> Option<Thread> {
    let id = pick_str(v, &["userId", "uid", "id", "friendId", "globalId"])?;
    let name = pick_str(v, &["displayName", "zaloName", "display_name", "name"])
        .or_else(|| {
            v.get("profile_displayName")
                .or_else(|| v.get("profile").and_then(|p| p.get("displayName")))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| id.clone());
    let avatar = pick_str(v, &["avatar", "avt", "profile_avatar"]);
    let is_group = is_group_flag(v);
    let time = v.get("lastActionTime")
        .and_then(|val| match val {
            Value::Number(n) => n.as_i64(),
            Value::String(s) => s.parse::<i64>().ok(),
            _ => None,
        })
        .map(|ts| {
            let s = ts / 1000;
            format!("{:02}:{:02}", (s / 3600) % 24, (s / 60) % 60)
        })
        .unwrap_or_default();

    Some(Thread {
        id,
        name,
        preview: String::new(),
        time,
        avatar,
        is_group,
        messages: Vec::new(),
    })
}

fn pick_str(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        match v.get(*k) {
            Some(Value::String(s)) if !s.is_empty() => return Some(s.clone()),
            Some(Value::Number(n)) => return Some(n.to_string()),
            _ => {}
        }
    }
    None
}

/// Send a 1-1 text message: `POST {domains.chat}/api/message/sms?{common}`
pub async fn send_message(s: &Session, to_id: &str, text: &str) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let body = obj(&[
        ("toid", json!(to_id)),
        ("message", json!(text)),
        ("clientId", json!(now)),
        ("imei", json!(s.imei)),
    ]);
    let params = s.encoded_params(&body)?;

    let host = if s.domains.chat.contains("-msg-chat.")
        || s.domains.chat.contains("tt-chat2-wpa")
        || s.domains.chat.is_empty()
    {
        "https://tt-chat3-wpa.api.zaloapp.com"
    } else {
        &s.domains.chat
    };

    let url = format!(
        "{host}/api/message/sms?{}",
        s.common_params()
    );
    let form = format!("params={params}");
    let resp = s.post_form(&url, form).await?;
    let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if let Some(code) = resp.get("error_code").and_then(Value::as_i64) {
        if code != 0 {
            let msg = resp
                .get("error_message")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            anyhow::bail!("server error {code}: {msg}");
        }
    }
    Ok(resp)
}

/// Send a group text message: `POST {domains.group}/api/group/sendmsg?{common}`
pub async fn send_group_message(s: &Session, grid: &str, text: &str) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let clean_grid = grid
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G');
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let body = obj(&[
        ("grid", json!(clean_grid)),
        ("message", json!(text)),
        ("clientId", json!(now)),
        ("imei", json!(s.imei)),
        ("visibility", json!(0)),
    ]);
    let params = s.encoded_params(&body)?;
    let url = format!("{}/api/group/sendmsg?{}", s.domains.group, s.common_params());
    let form = format!("params={params}");
    let resp = s.post_form(&url, form).await?;
    let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if let Some(code) = resp.get("error_code").and_then(Value::as_i64) {
        if code != 0 {
            let msg = resp
                .get("error_message")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            anyhow::bail!("server error {code}: {msg}");
        }
    }
    Ok(resp)
}

/// Send a sticker (`sendSticker` in `compact-app-pc.b7bec208c4c8f73e09f8.js:14325410`):
/// - 1-1 (`cmd 11154`): `POST {domains.chat}/api/message/sticker?{common}`
/// - Group (`cmd 11165`): `POST {domains.group}/api/group/sticker?{common}`
pub async fn send_sticker(
    s: &Session,
    to_id: &str,
    sticker_id: u64,
    cate_id: u64,
    sticker_type: u32,
    is_group: bool,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let url = if is_group {
        let clean_grid = to_id
            .trim_start_matches("group_")
            .trim_start_matches('g')
            .trim_start_matches('G');
        let body = obj(&[
            ("grid", json!(clean_grid)),
            ("stickerId", json!(sticker_id)),
            ("cateId", json!(cate_id)),
            ("type", json!(sticker_type)),
            ("clientId", json!(now)),
            ("imei", json!(s.imei)),
            ("zsource", json!(101)),
        ]);
        let params = s.encoded_params(&body)?;
        let url = format!("{}/api/group/sticker?{}", s.domains.group, s.common_params());
        let form = format!("params={params}");
        let resp = s.post_form(&url, form).await?;
        let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
        if let Some(code) = resp.get("error_code").and_then(Value::as_i64) {
            if code != 0 {
                let msg = resp
                    .get("error_message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                anyhow::bail!("server error {code}: {msg}");
            }
        }
        return Ok(resp);
    } else {
        let host = if s.domains.chat.contains("-msg-chat.")
            || s.domains.chat.contains("tt-chat2-wpa")
            || s.domains.chat.is_empty()
        {
            "https://tt-chat3-wpa.api.zaloapp.com"
        } else {
            &s.domains.chat
        };
        format!("{host}/api/message/sticker?{}", s.common_params())
    };

    let body = obj(&[
        ("toid", json!(to_id)),
        ("stickerId", json!(sticker_id)),
        ("cateId", json!(cate_id)),
        ("type", json!(sticker_type)),
        ("clientId", json!(now)),
        ("imei", json!(s.imei)),
        ("zsource", json!(101)),
    ]);
    let params = s.encoded_params(&body)?;
    let form = format!("params={params}");
    let resp = s.post_form(&url, form).await?;
    let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if let Some(code) = resp.get("error_code").and_then(Value::as_i64) {
        if code != 0 {
            let msg = resp
                .get("error_message")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            anyhow::bail!("server error {code}: {msg}");
        }
    }
    Ok(resp)
}

/// `GET /api/preloadconvers/get-last-msgs` (cmd 10053).
///
/// [O] request: GET `{convers_api}/api/preloadconvers/get-last-msgs?{common}&params={encodeAES(json)}`
/// where payload is `{ threadIdLocalMsgId: map_object, imei }`.
pub async fn get_last_msgs(s: &Session, thread_ids: &[String]) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let mut map = serde_json::Map::new();
    // [O] `getLastMessagesForPreview(e)` in `compact-app-pc.js:14385933` passes `{}` on preload
    // to fetch all recent conversation previews without overflowing URL length.
    if thread_ids.len() <= 25 {
        for id in thread_ids {
            map.insert(id.clone(), json!(0));
        }
    }
    let map_str = serde_json::to_string(&map).unwrap_or_else(|_| "{}".into());
    let body = obj(&[
        ("threadIdLocalMsgId", json!(map_str)),
        ("imei", json!(s.imei)),
    ]);
    let params = match s.encoded_params(&body) {
        Ok(p) => format!("params={p}"),
        Err(e) => {
            crate::logger::log_error("chat::get_last_msgs::encoded_params", &e.to_string());
            eprintln!("! get_last_msgs: no session key for encoded_params ({e}), falling back to plain_params");
            Session::plain_params(&body)
        }
    };

    let url = format!(
        "{}/api/preloadconvers/get-last-msgs?{}&{params}",
        s.domains.convers_api,
        s.common_params()
    );
    let resp = s.get(&url).await?;
    let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    Ok(resp)
}

/// `GET /api/pinconvers/list` (cmd 12120) — pinned threads.
pub async fn get_pinned(s: &Session) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let url = format!(
        "{}/api/pinconvers/list?{}",
        s.domains.conversation,
        s.common_params()
    );
    s.get(&url).await
}

/// `GET /api/group/history` (cmd 11240) — group message history.
pub async fn get_group_history(s: &Session, group_id: &str, count: u32) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let grid = group_id
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G');
    let body = obj(&[
        ("grid", json!(grid)),
        ("count", json!(count)),
        ("imei", json!(s.imei)),
    ]);
    let params = match s.encoded_params(&body) {
        Ok(p) => format!("params={p}"),
        Err(e) => {
            crate::logger::log_error("chat::get_group_history::encoded_params", &e.to_string());
            Session::plain_params(&body)
        }
    };
    let url = format!(
        "{}/api/group/history?{}&{params}",
        s.domains.group,
        s.common_params()
    );
    match s.get(&url).await {
        Ok(resp) => Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await),
        Err(_) => get_group_history_raw(s, grid, count).await,
    }
}

/// Byte-exact replica of the shipped bundle's `getHistoryMessage(e, t)`.
///
/// [O] `compact-app-pc:14364564`:
/// `n = {grid: e, count: t}`
/// `getGroupDomain() + "/api/group/history?" + _getCommonParams() + "&params=" + encodeURIComponent(encodeAES(JSON.stringify(n)))`
/// Signing tag: **11240**.
///
/// Note: the bundle sends **only** `grid` and `count` — no `imei`, no `signkey` on this
/// path. Kept as a separate raw probe so the guessed `get_group_history` above can be
/// compared against it without breaking existing callers.
pub async fn get_group_history_raw(s: &Session, grid: &str, count: u32) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    // [O] getGroupIdFromConversationId: strip the group-id prefix if present.
    let grid = grid
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G');
    let body = obj(&[("grid", json!(grid)), ("count", json!(count))]);
    let params = match s.encoded_params(&body) {
        Ok(p) => format!("params={p}"),
        Err(e) => {
            crate::logger::log_error("chat::get_group_history_raw::encoded_params", &e.to_string());
            Session::plain_params(&body)
        }
    };
    let url = format!(
        "{}/api/group/history?{}&{params}",
        s.domains.group,
        s.common_params()
    );
    let resp = s.get(&url).await?;
    let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    Ok(resp)
}

/// `GET /api/group/getlg/v4` (cmd 12027) — list of all groups user belongs to.
pub async fn get_groups(s: &Session) -> Result<Vec<Thread>> {
    if s.login.is_none() {
        return Ok(Vec::new());
    }
    let url = format!("{}/api/group/getlg/v4?{}", s.domains.group, s.common_params());
    let resp = s.get(&url).await?;
    let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;

    let mut groups = Vec::new();
    if let Some(data) = resp.get("data") {
        if let Some(map) = data.get("gridVerMap").and_then(Value::as_object) {
            for (grid, _ver) in map {
                groups.push(Thread {
                    id: grid.clone(),
                    name: format!("Nhóm {}", grid),
                    preview: String::new(),
                    time: crate::ui::clock(),
                    avatar: None,
                    is_group: true,
                    messages: Vec::new(),
                });
            }
        }
    }
    Ok(groups)
}

/// `POST /api/social/friend/getminiprofiles` (cmd 12134) — get basic profiles (name, avatar) for user IDs.
pub async fn get_mini_profiles(s: &Session, user_ids: &[String]) -> Result<serde_json::Map<String, Value>> {
    if s.login.is_none() || user_ids.is_empty() {
        return Ok(serde_json::Map::new());
    }
    let body = obj(&[
        ("friend_ids", json!(user_ids)),
        ("imei", json!(s.imei)),
    ]);
    let params = s.encoded_params(&body)?;
    let url = format!("{}/api/social/friend/getminiprofiles?{}", s.domains.profile, s.common_params());
    let form = format!("params={params}");
    let resp = s.post_form(&url, form).await?;
    let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;

    if let Some(data) = resp.get("data").and_then(Value::as_object) {
        Ok(data.clone())
    } else {
        Ok(serde_json::Map::new())
    }
}

/// Automatically resolve real display names and avatars for threads that only have placeholder names.
pub async fn resolve_thread_names(s: &Session, threads: &mut [Thread]) {
    let unknown_ids: Vec<String> = threads
        .iter()
        .filter(|t| !t.is_group && (t.name.starts_with("Người dùng ") || t.name.is_empty()))
        .map(|t| t.id.clone())
        .collect();

    if !unknown_ids.is_empty() {
        for chunk in unknown_ids.chunks(50) {
            if let Ok(profiles) = get_mini_profiles(s, chunk).await {
                for t in threads.iter_mut() {
                    if let Some(p) = profiles.get(&t.id) {
                        if let Some(name) = pick_str(p, &["zaloName", "displayName", "name", "display_name"]) {
                            if !name.is_empty() {
                                t.name = name;
                            }
                        }
                        if let Some(ava) = pick_str(p, &["avatar", "avt", "profile_avatar"]) {
                            if !ava.is_empty() {
                                t.avatar = Some(ava);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Per-queue cursor for `getChatFromPolling` (`MultiQueueId` in module `1iR4`, `sync-v2-sub-worker.js:1620000`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct QueueCursor {
    #[serde(rename = "lastId")]
    pub last_id: String,
    pub ids: Vec<String>,
}

/// State machine for Zalo's native HTTP long-polling message receiver (`getChatFromPolling`, cmd `99100`).
///
/// [O] Observed in `sync-v2-sub-worker.b7bec208c4c8f73e09f8.js` (offsets `1620000`, `3498103`, `14557981`):
/// - `R = new y({ type: "MSG", qosId: 91021, queueIds: [1, 2, 3], ... })`
/// - `getLastActionIds()` builds:
///   `"{lastActionId},{lastActionIdOther}," + encodeURIComponent(btoa(JSON.stringify(queues)) + ",1")`
/// - `getChatFromPolling(params)`:
///   `GET {zpw_chts_new[0]}/?zpw_ver=694&zpw_type=30&params={params}&ts={unix_sec}`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PollState {
    pub last_action_id: String,
    pub last_action_id_other: String,
    pub queues: std::collections::BTreeMap<String, QueueCursor>,
}

impl Default for PollState {
    fn default() -> Self {
        let mut queues = std::collections::BTreeMap::new();
        for qid in ["1", "2", "3"] {
            queues.insert(
                qid.to_string(),
                QueueCursor {
                    last_id: "1".into(),
                    ids: Vec::new(),
                },
            );
        }
        Self {
            last_action_id: "1".into(),
            last_action_id_other: "1".into(),
            queues,
        }
    }
}

fn max_action_id(a: &str, b: &str) -> String {
    let na = a.parse::<u128>().unwrap_or(0);
    let nb = b.parse::<u128>().unwrap_or(0);
    if nb >= na && nb > 0 {
        b.to_string()
    } else if na > 0 {
        a.to_string()
    } else {
        "1".into()
    }
}

impl PollState {
    pub fn encode_params(&self) -> String {
        use base64::Engine;
        let q_json = serde_json::to_string(&self.queues).unwrap_or_else(|_| "{}".into());
        let b64 = base64::engine::general_purpose::STANDARD.encode(q_json.as_bytes());
        let tail = crate::session::urlencode(&format!("{b64},1"));
        format!(
            "{},{},{}",
            self.last_action_id, self.last_action_id_other, tail
        )
    }

    pub fn update_from_response(&mut self, resp: &Value) {
        let mut data_node = resp.get("data").unwrap_or(resp);
        if let Some(inner) = data_node.get("data").filter(|v| v.is_object()) {
            data_node = inner;
        }
        if let Some(reset_id) = pick_str(data_node, &["resetLastActionId"]) {
            if reset_id != "0" && reset_id.parse::<u128>().unwrap_or(0) > 0 {
                self.last_action_id = reset_id.clone();
                self.last_action_id_other = "1".into();
                for q in self.queues.values_mut() {
                    q.last_id = reset_id.clone();
                    q.ids.clear();
                }
                return;
            }
        }
        if let Some(lai) = pick_str(data_node, &["lastActionId"]) {
            if lai != "0" {
                self.last_action_id = max_action_id(&self.last_action_id, &lai);
            }
        }
        if let Some(laio) = pick_str(data_node, &["lastActionIdOther"]) {
            if laio != "0" {
                self.last_action_id_other = max_action_id(&self.last_action_id_other, &laio);
            }
        }
        if let Some(qs) = data_node.get("queueStatus").and_then(Value::as_object) {
            for (qid, qval) in qs {
                // Only update configured HTTP long-polling MSG queues ("1", "2", "3").
                // Adding socket-only queues ("15", "16", "7", "8") causes lp_chat to reject subsequent requests.
                let Some(entry) = self.queues.get_mut(qid) else {
                    continue;
                };
                if let Some(lid) = pick_str(qval, &["lastId"]) {
                    if lid != "0" {
                        entry.last_id = max_action_id(&entry.last_id, &lid);
                    }
                }
                if let Some(ids_arr) = qval.get("ids").and_then(Value::as_array) {
                    let max_lid = entry.last_id.parse::<u128>().unwrap_or(u128::MAX);
                    let mut new_ids: Vec<String> = ids_arr
                        .iter()
                        .filter_map(|v| match v {
                            Value::String(s) if !s.is_empty() => Some(s.clone()),
                            Value::Number(n) => Some(n.to_string()),
                            _ => None,
                        })
                        .filter(|s| s.parse::<u128>().map(|n| n <= max_lid).unwrap_or(true))
                        .collect();
                    if !new_ids.is_empty() {
                        // [O] `QueueControl.getLastActionIdsArray` (`sync-v2-sub-worker.js:1606950`):
                        // sort descending, take at most 10 <= lastId, then reverse so `ids` is ascending.
                        new_ids.sort_by_key(|s| std::cmp::Reverse(s.parse::<u128>().unwrap_or(0)));
                        new_ids.dedup();
                        new_ids.truncate(10);
                        new_ids.reverse();
                        entry.ids = new_ids;
                    }
                }
            }
        }
    }
}

/// Fetch cloud message history (`getCM` in `sync-v2-sub-worker.b7bec208c4c8f73e09f8.js:14595433`, cmd `12506` / `12507`).
///
/// [O] Observed in module `1EWQ` (`sync-v2-sub-worker.js:1545933`):
/// Cloud message history (`/api/cm/getrecentv2` and `/api/cm/getoldv2` on `group_cloud_message`)
/// is enabled for Group conversations (`g<groupId>`) and My Cloud (`sendToMeId`).
pub async fn get_cloud_messages(
    s: &Session,
    conv_id: &str,
    global_msg_id: &str,
    is_old: bool,
    count: u32,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let clean_id = conv_id
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G');
    let is_send2me = s
        .login
        .as_ref()
        .map(|l| !l.send2me_id.is_empty() && l.send2me_id == clean_id)
        .unwrap_or(false);
    let mut map = serde_json::Map::new();
    map.insert("groupId".into(), json!(clean_id));
    map.insert(
        "globalMsgId".into(),
        json!(if global_msg_id.is_empty() {
            "10000000000000000"
        } else {
            global_msg_id
        }),
    );
    map.insert("count".into(), json!(count));
    map.insert("msgIds".into(), json!([]));
    map.insert("imei".into(), json!(s.imei));
    if is_send2me {
        map.insert("isOA".into(), json!(1));
    }
    map.insert("src".into(), json!(if is_old { 3 } else { 1 }));

    let body = Value::Object(map);
    let params = s.encoded_params(&body)?;
    let ep = if is_old {
        "/api/cm/getoldv2"
    } else {
        "/api/cm/getrecentv2"
    };
    let host = if s.domains.cloud_msg.is_empty() {
        "https://tt-group-cm.api.zaloapp.com"
    } else {
        &s.domains.cloud_msg
    };
    let url = format!(
        "{}{ep}?{}&params={params}&nretry=0",
        host.trim_end_matches('/'),
        s.common_params()
    );
    let resp = s.get(&url).await?;
    Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await)
}

/// Call Zalo's native HTTP long-polling message endpoint (`getChatFromPolling`, cmd `99100`).
///
/// Uses a 12-second timeout matching `lp_chat` long-polling windows (`sync-v2-sub-worker.js:3498030`).
pub async fn get_chat_from_polling(s: &Session, state: &mut PollState) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let host = if s.domains.lp_chat.is_empty() {
        "https://p4-msg-chat.zaloapp.com"
    } else {
        &s.domains.lp_chat
    };
    let ts = crate::login::unix_now();
    let url = format!(
        "{}/?{}&params={}&ts={ts}",
        host.trim_end_matches('/'),
        s.common_params(),
        state.encode_params()
    );
    let fut = s.get(&url);
    match tokio::time::timeout(std::time::Duration::from_millis(12000), fut).await {
        Ok(Ok(resp)) => {
            let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
            if let Some(code) = dec.get("error_code").and_then(Value::as_i64) {
                if code != 0 {
                    let msg = dec
                        .get("error_message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    anyhow::bail!("server error {code}: {msg}");
                }
            }
            state.update_from_response(&dec);
            Ok(dec)
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Ok(json!({"error_code": 0, "data": {}})),
    }
}

/// Poll both `getChatFromPolling` (`lp_chat`) and `get-last-msgs` (`convers_api`),
/// merge all incoming/outgoing/group messages into `threads`, resolve unknown names,
/// and persist to `zalo_history.db` and `threads_cache.json`.
pub async fn poll_new_messages(
    s: &Session,
    state: &mut PollState,
    threads: &mut Vec<Thread>,
) -> Result<()> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    // 1. Poll live/offline message queues via `getChatFromPolling` (follow `more == 1` / `resetLastActionId` up to 3 batches)
    for _ in 0..3 {
        let Ok(lp_resp) = get_chat_from_polling(s, state).await else {
            break;
        };
        merge_previews(threads, &lp_resp);
        let data_node = lp_resp.get("data").unwrap_or(&lp_resp);
        let more = data_node.get("more").and_then(Value::as_i64).unwrap_or(0);
        let has_reset = pick_str(data_node, &["resetLastActionId"])
            .map(|r| r != "0" && !r.is_empty())
            .unwrap_or(false);
        if more == 0 && !has_reset {
            break;
        }
    }
    // 2. Poll conversation previews via `get-last-msgs`
    if let Ok(preview_resp) = get_last_msgs(s, &[]).await {
        merge_previews(threads, &preview_resp);
    }
    // 3. Resolve display names for any newly discovered threads
    resolve_thread_names(s, threads).await;
    // 4. Persist to SQLite and JSON cache
    if let Ok(mut store) = crate::store::MessageStore::open("zalo_history.db") {
        let _ = crate::store::persist_threads(&mut store, threads);
    }
    save_threads_cache(threads);
    Ok(())
}

fn extract_content_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.starts_with('{') && trimmed.ends_with('}') {
                if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                    if let Some(inner) = extract_content_text(&parsed) {
                        if !inner.is_empty() {
                            return Some(inner);
                        }
                    }
                    // It is a JSON object with no text/caption fields (like sticker {catId, id} or photo {href}).
                    // Return None so parse_chat_message can assign a clean descriptive label like [Nhãn dán] or [Hình ảnh]
                    // instead of displaying the raw JSON string in chat.
                    return None;
                }
            }
            Some(s.clone())
        }
        Value::Object(o) => {
            for k in ["title", "description", "caption", "text", "msg"] {
                if let Some(Value::String(s)) = o.get(k) {
                    if !s.trim().is_empty() {
                        return Some(s.clone());
                    }
                }
            }
            if let Some(Value::String(s)) = o.get("fileName").or_else(|| o.get("name")) {
                if !s.trim().is_empty() {
                    return Some(format!("[Tập tin] {s}"));
                }
            }
            None
        }
        _ => None,
    }
}

fn normalize_msg_type(it: &Value) -> String {
    let raw = it.get("msgType")
        .or_else(|| it.get("messageType"))
        .or_else(|| it.get("type"));
    match raw {
        Some(Value::String(s)) => s.to_ascii_lowercase(),
        Some(Value::Number(n)) => {
            match n.as_i64() {
                Some(1) => "chat.text".into(),
                Some(2) => "chat.photo".into(),
                Some(3) => "chat.voice".into(),
                Some(4) => "chat.video".into(),
                Some(5) => "chat.sticker".into(),
                Some(19) => "chat.file".into(),
                Some(24) => "chat.location".into(),
                Some(31) => "chat.undo".into(),
                _ => n.to_string(),
            }
        }
        _ => String::new(),
    }
}

fn extract_sticker_info(v: &Value) -> Option<(u64, u64, u32)> {
    match v {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.starts_with('{') && trimmed.ends_with('}') {
                if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                    return extract_sticker_info(&parsed);
                }
            }
            None
        }
        Value::Object(o) => {
            let parse_u64 = |val: &Value| -> Option<u64> {
                match val {
                    Value::Number(n) => n.as_u64(),
                    Value::String(s) => s.parse::<u64>().ok(),
                    _ => None,
                }
            };
            let cat_id = o
                .get("catId")
                .or_else(|| o.get("cateId"))
                .and_then(parse_u64);
            let stk_id = o
                .get("stickerId")
                .or_else(|| o.get("id"))
                .and_then(parse_u64);
            let stk_type = o
                .get("type")
                .and_then(parse_u64)
                .map(|t| t as u32)
                .unwrap_or(7);
            if let Some(sid) = stk_id {
                if cat_id.is_some() || o.contains_key("stickerId") || o.contains_key("catId") || o.contains_key("cateId") {
                    return Some((sid, cat_id.unwrap_or(0), stk_type));
                }
            }
            None
        }
        _ => None,
    }
}

fn parse_chat_message(it: &Value) -> Option<ChatMessage> {
    let msg_type = normalize_msg_type(it);

    let raw_content_val = it
        .get("content")
        .or_else(|| it.get("message").and_then(|m| m.get("content")).or_else(|| it.get("message")))
        .or_else(|| it.get("msg"));

    let mut sticker_meta = raw_content_val.and_then(extract_sticker_info);
    if sticker_meta.is_none() && (msg_type == "chat.sticker" || msg_type == "sticker" || msg_type == "5") {
        sticker_meta = extract_sticker_info(it);
    }

    let mut content = raw_content_val
        .and_then(extract_content_text)
        .unwrap_or_default();

    if sticker_meta.is_some() && (content.is_empty() || content.starts_with('{')) {
        content = "[Nhãn dán]".to_string();
    }

    if content.is_empty() {
        content = match msg_type.as_str() {
            "chat.sticker" | "sticker" | "5" => "[Nhãn dán]".to_string(),
            "chat.photo" | "photo" | "2" => "[Hình ảnh]".to_string(),
            "chat.voice" | "voice" | "3" => "[Tin nhắn thoại]".to_string(),
            "chat.video" | "video" | "4" => "[Video]".to_string(),
            "chat.file" | "file" | "19" => "[Tập tin]".to_string(),
            "chat.location" | "location" | "24" => "[Vị trí]".to_string(),
            "chat.contact" | "contact" => "[Danh thiếp]".to_string(),
            "chat.undo" | "undo" | "31" => "[Tin nhắn đã thu hồi]".to_string(),
            "chat.recommended" | "share.file" => "[Chia sẻ]".to_string(),
            _ => {
                let obj = it.get("content").or_else(|| it.get("message"));
                if let Some(o) = obj {
                    if o.get("catId").is_some() || o.get("stickerId").is_some() || o.get("id").is_some() {
                        "[Nhãn dán]".to_string()
                    } else if o.get("href").is_some() || o.get("thumb").is_some() {
                        "[Hình ảnh]".to_string()
                    } else if o.is_object() {
                        "[Tin nhắn]".to_string()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            }
        };
    }

    let msg_id = it
        .get("msgId")
        .or_else(|| it.get("cliMsgId"))
        .or_else(|| it.get("globalMsgId"))
        .or_else(|| it.get("lastMsgId"))
        .or_else(|| it.get("id"))
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .unwrap_or_default();

    if content.is_empty() && msg_id.is_empty() {
        return None;
    }

    let uid_from = it
        .get("uidFrom")
        .or_else(|| it.get("fromUid"))
        .or_else(|| it.get("fromId"))
        .or_else(|| it.get("senderId"))
        .or_else(|| it.get("sender"))
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .unwrap_or_default();

    let display_sender = it
        .get("dName")
        .or_else(|| it.get("displayName"))
        .or_else(|| it.get("fromName"))
        .or_else(|| it.get("senderName"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uid_from.clone());

    let is_outgoing = uid_from == "0"
        || uid_from.is_empty()
        || it.get("isOutgoing").and_then(Value::as_bool).unwrap_or(false)
        || it.get("is_outgoing").and_then(Value::as_i64).map(|n| n == 1).unwrap_or(false);

    let ts = it
        .get("ts")
        .or_else(|| it.get("sendTime"))
        .or_else(|| it.get("serverTime"))
        .or_else(|| it.get("time"))
        .and_then(|v| match v {
            Value::Number(n) => n.as_i64(),
            Value::String(s) => s.parse::<i64>().ok(),
            _ => None,
        })
        .unwrap_or(0);

    let time = if ts > 0 {
        let s = ts / 1000;
        format!("{:02}:{:02}", (s / 3600) % 24, (s / 60) % 60)
    } else {
        crate::ui::clock()
    };

    Some(ChatMessage {
        id: msg_id,
        sender: display_sender,
        text: content,
        time,
        is_outgoing,
        ts,
        sticker_id: sticker_meta.map(|(sid, _, _)| sid),
        sticker_cate_id: sticker_meta.map(|(_, cid, _)| cid),
        sticker_type: sticker_meta.map(|(_, _, st)| st),
    })
}

fn is_group_flag(v: &Value) -> bool {
    v.get("isGroup")
        .map(|g| g.as_bool() == Some(true) || g.as_i64() == Some(1))
        .unwrap_or(false)
}

fn extract_message_thread_id_ext(
    entry: &Value,
    m_val: &Value,
    in_group_bucket: bool,
    cloud_thread_id: Option<&str>,
) -> Option<(String, bool)> {
    let is_group = in_group_bucket || is_group_flag(m_val) || is_group_flag(entry);

    // 1. Explicit `grid` field or `groupMsgs` bucket (`e.msgKey = GROUPID_PREFIX + e.idTo` in `sync-v2-sub-worker.js:3500340`)
    for obj in [m_val, entry] {
        if let Some(grid) = pick_str(obj, &["grid", "groupId"]) {
            if !grid.is_empty() && grid != "0" {
                let clean = grid.trim_start_matches("group_").to_string();
                return Some((clean, true));
            }
        }
    }
    if is_group {
        if let Some(gid) = pick_str(m_val, &["idTo", "toid", "toId", "threadId", "conversId"])
            .or_else(|| pick_str(entry, &["idTo", "toid", "toId", "threadId", "conversId"]))
        {
            if !gid.is_empty() && gid != "0" {
                let clean = gid.trim_start_matches("group_").to_string();
                return Some((clean, true));
            }
        }
    }

    // 2. 1-on-1 or self ("Cloud của tôi") message routing
    // [O] `sync-v2-sub-worker.js:3500260`:
    //   `"0" == e.idTo && "0" == e.uidFrom && (e.idTo = "" + this.userId)`
    // [O] `sync-v2-sub-worker.js:829500` (`_filterDuplicatedMessages`):
    //   `e.isGroup ? e.msgKey : "0" == e.idTo ? e.uidFrom : e.idTo`
    let uid_from = pick_str(m_val, &["uidFrom", "fromUid", "fromId", "senderId"])
        .or_else(|| pick_str(entry, &["uidFrom", "fromUid", "fromId", "senderId"]))
        .unwrap_or_default();
    let id_to = pick_str(m_val, &["idTo", "toid", "toId"])
        .or_else(|| pick_str(entry, &["idTo", "toid", "toId"]))
        .unwrap_or_default();

    if (id_to.is_empty() || id_to == "0") && (uid_from.is_empty() || uid_from == "0") {
        if let Some(cid) = cloud_thread_id {
            if !cid.is_empty() {
                return Some((cid.to_string(), false));
            }
        }
    }

    if !uid_from.is_empty() && uid_from != "0" && (id_to.is_empty() || id_to == "0" || Some(id_to.as_str()) == cloud_thread_id) {
        // Incoming 1-1 message: peer sent it to us -> thread ID is `uid_from`
        Some((uid_from, false))
    } else if !id_to.is_empty() && id_to != "0" {
        // Outgoing 1-1 message or conversation wrapper with explicit `idTo` -> thread ID is `id_to`
        Some((id_to, false))
    } else if !uid_from.is_empty() && uid_from != "0" {
        Some((uid_from, false))
    } else {
        pick_str(m_val, &["threadId", "conversId", "userId", "friendId", "id"])
            .or_else(|| pick_str(entry, &["threadId", "conversId", "userId", "friendId", "id"]))
            .filter(|s| s != "0" && !s.is_empty())
            .map(|s| (s, is_group))
    }
}

/// Fold a `get-last-msgs` or `getChatFromPolling` response into thread previews and conversation messages.
pub fn merge_previews(threads: &mut Vec<Thread>, resp: &Value) {
    let mut entries: Vec<(&Value, bool)> = Vec::new();
    let mut data_node = resp.get("data").unwrap_or(resp);
    if let Some(inner_data) = data_node.get("data").filter(|v| v.is_object() || v.is_array()) {
        data_node = inner_data;
    }

    let cloud_tid: Option<String> = threads
        .iter()
        .find(|t| t.name.contains("Cloud"))
        .map(|t| t.id.clone());

    if let Some(arr) = data_node.as_array() {
        for item in arr {
            entries.push((item, is_group_flag(item)));
        }
    } else if let Some(obj) = data_node.as_object() {
        let mut found_list = false;
        for (key, is_grp) in [
            ("msgs", false),
            ("groupMsgs", true),
            ("pageMsgs", false),
            ("conversations", false),
            ("threads", false),
            ("allmsgs", false),
        ] {
            if let Some(arr) = obj.get(key).and_then(Value::as_array) {
                for item in arr {
                    entries.push((item, is_grp || is_group_flag(item)));
                }
                found_list = true;
            }
        }
        if !found_list {
            for (_, val) in obj {
                if val.is_object() {
                    entries.push((val, is_group_flag(val)));
                }
            }
        }
    }

    for (entry, is_grp_bucket) in entries {
        let mut raw_msgs: Vec<(&Value, bool)> = Vec::new();
        if let Some(arr) = entry.get("messages").and_then(Value::as_array) {
            for m in arr {
                raw_msgs.push((m, is_grp_bucket || is_group_flag(m)));
            }
        }
        if let Some(arr) = entry.get("msgs").and_then(Value::as_array) {
            for m in arr {
                raw_msgs.push((m, is_grp_bucket || is_group_flag(m)));
            }
        }
        if let Some(arr) = entry.get("groupMsgs").and_then(Value::as_array) {
            for m in arr {
                raw_msgs.push((m, true));
            }
        }
        if let Some(obj) = entry.get("lastMsg").filter(|v| v.is_object()) {
            raw_msgs.push((obj, is_grp_bucket || is_group_flag(obj)));
        }
        if let Some(obj) = entry.get("message").filter(|v| v.is_object()) {
            raw_msgs.push((obj, is_grp_bucket || is_group_flag(obj)));
        }
        if raw_msgs.is_empty()
            && (entry.get("content").is_some() || entry.get("msg").is_some() || entry.get("msgId").is_some())
        {
            raw_msgs.push((entry, is_grp_bucket));
        }

        for (m_val, is_grp) in raw_msgs {
            let Some(parsed) = parse_chat_message(m_val) else {
                continue;
            };

            let Some((tid, thread_is_group)) =
                extract_message_thread_id_ext(entry, m_val, is_grp, cloud_tid.as_deref())
            else {
                continue;
            };

            if let Some(t) = threads.iter_mut().find(|t| t.id == tid) {
                if thread_is_group {
                    t.is_group = true;
                }
                if let Some(existing) = t.messages.iter_mut().find(|existing| {
                    (!existing.id.is_empty() && !parsed.id.is_empty() && existing.id == parsed.id)
                        || (existing.text == parsed.text
                            && existing.is_outgoing == parsed.is_outgoing
                            && (existing.time == parsed.time
                                || (existing.ts != 0 && parsed.ts != 0 && (existing.ts - parsed.ts).abs() < 5000)))
                }) {
                    if existing.id.starts_with("out_") && !parsed.id.is_empty() && !parsed.id.starts_with("out_") {
                        existing.id = parsed.id;
                    }
                    if parsed.ts > 0 && existing.ts != parsed.ts {
                        existing.ts = parsed.ts;
                        existing.time = parsed.time;
                    }
                    if existing.sticker_id.is_none() && parsed.sticker_id.is_some() {
                        existing.sticker_id = parsed.sticker_id;
                        existing.sticker_cate_id = parsed.sticker_cate_id;
                        existing.sticker_type = parsed.sticker_type;
                    }
                } else {
                    t.messages.push(parsed);
                }
            } else {
                // New thread detected from incoming message!
                let default_name = if thread_is_group {
                    format!("Nhóm {}", tid)
                } else {
                    format!("Người dùng {}", tid)
                };
                threads.push(Thread {
                    id: tid,
                    name: default_name,
                    preview: parsed.text.clone(),
                    time: parsed.time.clone(),
                    avatar: None,
                    is_group: thread_is_group,
                    messages: vec![parsed],
                });
            }
        }
    }

    for t in threads.iter_mut() {
        if !t.messages.is_empty() {
            t.messages.sort_by_key(|m| m.ts);
            if let Some(newest) = t.messages.last() {
                if !newest.text.is_empty() {
                    t.preview = newest.text.replace('\n', " ");
                    t.time = newest.time.clone();
                }
            }
        }
    }
}

/// Merge group history response into a thread's messages, sorted by timestamp and deduplicated by msg_id.
pub fn merge_group_history(thread: &mut Thread, resp: &Value) {
    let mut raw_msgs: Vec<&Value> = Vec::new();
    let mut data_node = resp.get("data").unwrap_or(resp);
    if let Some(inner_data) = data_node.get("data").filter(|v| v.is_object() || v.is_array()) {
        data_node = inner_data;
    }

    if let Some(arr) = data_node.as_array() {
        raw_msgs.extend(arr);
    } else if let Some(obj) = data_node.as_object() {
        for key in ["msgs", "groupMsgs", "messages"] {
            if let Some(arr) = obj.get(key).and_then(Value::as_array) {
                raw_msgs.extend(arr);
            }
        }
    }

    for it in raw_msgs {
        if let Some(m) = parse_chat_message(it) {
            if !thread.messages.iter().any(|existing| {
                (!existing.id.is_empty() && !m.id.is_empty() && existing.id == m.id)
                    || (existing.text == m.text && existing.ts != 0 && existing.ts == m.ts)
            }) {
                thread.messages.push(m);
            }
        }
    }

    thread.messages.sort_by_key(|m| m.ts);

    if let Some(newest) = thread.messages.last() {
        if !newest.text.is_empty() {
            thread.preview = newest.text.replace('\n', " ");
            thread.time = newest.time.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unauthenticated_calls_fail_cleanly() {
        let s = Session::new(false, false, false).unwrap();
        let err1 = get_friends(&s).await.unwrap_err();
        assert_eq!(err1.to_string(), "chưa đăng nhập");

        let err2 = get_last_msgs(&s, &["test_tid".into()]).await.unwrap_err();
        assert_eq!(err2.to_string(), "chưa đăng nhập");

        let err3 = send_message(&s, "test_to", "hello").await.unwrap_err();
        assert_eq!(err3.to_string(), "chưa đăng nhập");

        let err4 = get_pinned(&s).await.unwrap_err();
        assert_eq!(err4.to_string(), "chưa đăng nhập");

        let err5 = get_group_history(&s, "test_gid", 20).await.unwrap_err();
        assert_eq!(err5.to_string(), "chưa đăng nhập");
    }

    fn sample_thread(id: &str) -> Thread {
        Thread {
            id: id.into(),
            name: format!("Name {id}"),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: false,
            messages: Vec::new(),
        }
    }

    #[test]
    fn test_merge_previews_single_message_object() {
        let mut threads = vec![sample_thread("u1")];
        let resp = json!({
            "error_code": 0,
            "data": [
                {
                    "idTo": "u1",
                    "message": {
                        "msgId": "m_single",
                        "content": "Single message text",
                        "ts": 1729501000000_i64,
                        "uidFrom": "u1"
                    }
                }
            ]
        });

        merge_previews(&mut threads, &resp);
        assert_eq!(threads[0].messages.len(), 1);
        assert_eq!(threads[0].messages[0].id, "m_single");
        assert_eq!(threads[0].messages[0].text, "Single message text");
        assert_eq!(threads[0].preview, "Single message text");
    }

    #[test]
    fn test_merge_previews_last_msg_object() {
        let mut threads = vec![sample_thread("u2")];
        let resp = json!({
            "error_code": 0,
            "data": [
                {
                    "idTo": "u2",
                    "lastMsg": {
                        "msgId": "m_last",
                        "content": "LastMsg text",
                        "ts": 1729502000000_i64,
                        "uidFrom": "u2"
                    }
                }
            ]
        });

        merge_previews(&mut threads, &resp);
        assert_eq!(threads[0].messages.len(), 1);
        assert_eq!(threads[0].messages[0].id, "m_last");
        assert_eq!(threads[0].preview, "LastMsg text");
    }

    #[test]
    fn test_merge_previews_messages_array_three_items() {
        let mut threads = vec![sample_thread("u3")];
        // Provide items out of order to verify timestamp ascending sort
        let resp = json!({
            "error_code": 0,
            "data": [
                {
                    "idTo": "u3",
                    "messages": [
                        {
                            "msgId": "m_item1",
                            "content": "First message",
                            "ts": 1000_i64,
                            "uidFrom": "u3"
                        },
                        {
                            "msgId": "m_item3",
                            "content": "Third message",
                            "ts": 3000_i64,
                            "uidFrom": "u3"
                        },
                        {
                            "msgId": "m_item2",
                            "content": "Second message",
                            "ts": 2000_i64,
                            "uidFrom": "0"
                        }
                    ]
                }
            ]
        });

        merge_previews(&mut threads, &resp);
        assert_eq!(threads[0].messages.len(), 3);
        assert_eq!(threads[0].messages[0].id, "m_item1");
        assert_eq!(threads[0].messages[0].text, "First message");
        assert_eq!(threads[0].messages[1].id, "m_item2");
        assert_eq!(threads[0].messages[1].text, "Second message");
        assert!(threads[0].messages[1].is_outgoing);
        assert_eq!(threads[0].messages[2].id, "m_item3");
        assert_eq!(threads[0].messages[2].text, "Third message");
        // Preview must be from the newest message (m_item3)
        assert_eq!(threads[0].preview, "Third message");
    }

    #[test]
    fn test_merge_previews_entry_with_no_message() {
        let mut threads = vec![sample_thread("u4")];
        threads[0].preview = "Existing preview".into();
        let resp = json!({
            "error_code": 0,
            "data": [
                {
                    "idTo": "u4"
                },
                {
                    "idTo": "u4",
                    "messages": []
                }
            ]
        });

        merge_previews(&mut threads, &resp);
        assert!(threads[0].messages.is_empty());
        assert_eq!(threads[0].preview, "Existing preview");
    }

    #[test]
    fn test_merge_group_history_fixture() {
        let mut thread = sample_thread("grp_123");
        thread.is_group = true;

        let resp = json!({
            "error_code": 0,
            "data": {
                "groupMsgs": [
                    {
                        "msgId": "gm_2",
                        "content": "Second group message",
                        "ts": 2000_i64,
                        "uidFrom": "member_2"
                    },
                    {
                        "msgId": "gm_1",
                        "content": "First group message",
                        "ts": 1000_i64,
                        "uidFrom": "member_1"
                    },
                    {
                        "msgId": "gm_2",
                        "content": "Duplicate second message",
                        "ts": 2000_i64,
                        "uidFrom": "member_2"
                    }
                ]
            }
        });

        merge_group_history(&mut thread, &resp);
        assert_eq!(thread.messages.len(), 2);
        assert_eq!(thread.messages[0].id, "gm_1");
        assert_eq!(thread.messages[1].id, "gm_2");
        assert_eq!(thread.preview, "Second group message");
    }

    #[test]
    fn test_merge_previews_incoming_1_1_and_group_and_cloud() {
        let mut cloud = sample_thread("7223873394142226858");
        cloud.name = "Cloud của tôi (Lưu trữ)".into();
        let friend = sample_thread("5226566053441588578");
        let mut group = sample_thread("888999000");
        group.is_group = true;
        let mut threads = vec![cloud, friend, group];

        let resp = json!({
            "error_code": 0,
            "data": {
                "msgs": [
                    {
                        "msgId": "in_1",
                        "content": "Tin nhan den tu ban be",
                        "idTo": "0",
                        "uidFrom": "5226566053441588578",
                        "ts": "1729505000000"
                    },
                    {
                        "msgId": "cloud_1",
                        "content": "Tin nhan tu gui vao Cloud",
                        "idTo": "0",
                        "uidFrom": "0",
                        "ts": "1729506000000"
                    }
                ],
                "groupMsgs": [
                    {
                        "msgId": "grp_msg_1",
                        "content": "Tin nhan trong nhom",
                        "idTo": "888999000",
                        "uidFrom": "111222333",
                        "dName": "Nguyen Van B",
                        "ts": "1729507000000"
                    }
                ]
            }
        });

        merge_previews(&mut threads, &resp);
        // Cloud message routed to `7223873394142226858`
        assert_eq!(threads[0].messages.len(), 1);
        assert_eq!(threads[0].messages[0].text, "Tin nhan tu gui vao Cloud");
        assert!(threads[0].messages[0].is_outgoing);

        // Incoming 1-1 routed to `5226566053441588578`
        assert_eq!(threads[1].messages.len(), 1);
        assert_eq!(threads[1].messages[0].text, "Tin nhan den tu ban be");
        assert!(!threads[1].messages[0].is_outgoing);

        // Group message routed to `888999000` (not `111222333`)
        assert_eq!(threads[2].messages.len(), 1);
        assert_eq!(threads[2].messages[0].text, "Tin nhan trong nhom");
        assert_eq!(threads[2].messages[0].sender, "Nguyen Van B");
        assert!(!threads[2].messages[0].is_outgoing);
    }

    #[test]
    fn test_poll_state_encode_and_update() {
        let mut ps = PollState::default();
        let encoded = ps.encode_params();
        assert!(encoded.starts_with("1,1,"));
        assert!(encoded.ends_with("%2C1"));

        let resp = json!({
            "data": {
                "lastActionId": "8399086511016",
                "lastActionIdOther": "8399036593265",
                "queueStatus": {
                    "1": {
                        "lastId": "8399086511016",
                        "ids": ["8399086511016"]
                    },
                    "15": {
                        "lastId": "8399036593265",
                        "ids": ["8399036593265"]
                    }
                }
            }
        });
        ps.update_from_response(&resp);
        assert_eq!(ps.last_action_id, "8399086511016");
        assert_eq!(ps.last_action_id_other, "8399036593265");
        assert_eq!(ps.queues.get("1").unwrap().last_id, "8399086511016");
        assert_eq!(ps.queues.get("1").unwrap().ids, vec!["8399086511016".to_string()]);
        // Socket-only queue "15" must NOT be added to HTTP polling queues
        assert!(!ps.queues.contains_key("15"));
    }

    #[test]
    fn test_parse_chat_message_sticker_json_string() {
        let msg_val = json!({
            "msgId": "stk_1",
            "msgType": "chat.sticker",
            "content": "{\"catId\":1002,\"id\":2005,\"type\":1}",
            "uidFrom": "user_a",
            "ts": 1729508000000_i64
        });
        let parsed = parse_chat_message(&msg_val).expect("must parse sticker");
        assert_eq!(parsed.text, "[Nhãn dán]");
        assert_eq!(parsed.id, "stk_1");
        assert_eq!(parsed.sticker_id, Some(2005));
        assert_eq!(parsed.sticker_cate_id, Some(1002));
        assert_eq!(parsed.sticker_type, Some(1));
        assert_eq!(
            parsed.sticker_image_url().as_deref(),
            Some("https://api.zaloapp.com/api/emoticon/sticker/webpc?eid=2005&size=130")
        );
    }

    #[test]
    fn test_parse_chat_message_integer_msg_types() {
        // Integer msgType = 5 (sticker) with empty content
        let stk = json!({
            "msgId": "stk_int",
            "msgType": 5,
            "content": "",
            "uidFrom": "user_b",
            "ts": 1000_i64
        });
        assert_eq!(parse_chat_message(&stk).unwrap().text, "[Nhãn dán]");

        // Integer msgType = 2 (photo)
        let photo = json!({
            "msgId": "photo_int",
            "msgType": 2,
            "content": "",
            "uidFrom": "user_b",
            "ts": 2000_i64
        });
        assert_eq!(parse_chat_message(&photo).unwrap().text, "[Hình ảnh]");

        // Integer msgType = 3 (voice)
        let voice = json!({
            "msgId": "voice_int",
            "msgType": 3,
            "content": "",
            "uidFrom": "user_b",
            "ts": 3000_i64
        });
        assert_eq!(parse_chat_message(&voice).unwrap().text, "[Tin nhắn thoại]");

        // Integer msgType = 19 (file)
        let file = json!({
            "msgId": "file_int",
            "msgType": 19,
            "content": "",
            "uidFrom": "user_b",
            "ts": 4000_i64
        });
        assert_eq!(parse_chat_message(&file).unwrap().text, "[Tập tin]");

        // Integer msgType = 31 (undo)
        let undo = json!({
            "msgId": "undo_int",
            "msgType": 31,
            "content": "",
            "uidFrom": "user_b",
            "ts": 5000_i64
        });
        assert_eq!(parse_chat_message(&undo).unwrap().text, "[Tin nhắn đã thu hồi]");
    }

    #[test]
    fn test_parse_chat_message_with_plain_message_string() {
        let msg = json!({
            "msgId": "msg_str",
            "message": "Xin chao buoi sang",
            "uidFrom": "user_c",
            "dName": "Charlie",
            "ts": 1000_i64
        });
        let parsed = parse_chat_message(&msg).unwrap();
        assert_eq!(parsed.text, "Xin chao buoi sang");
        assert_eq!(parsed.sender, "Charlie");
    }

    #[test]
    fn test_parse_thread_boolean_is_group_and_string_time() {
        let val = json!({
            "userId": "grp_test",
            "displayName": "Nhom Chem Gio",
            "isGroup": true,
            "lastActionTime": "1729505000000"
        });
        let thread = parse_thread(&val).unwrap();
        assert!(thread.is_group);
        assert_eq!(thread.name, "Nhom Chem Gio");
        assert!(!thread.time.is_empty());
    }
}
