//! Telegram ↔ Zalo bridge (`--tg-bridge`).
//!
//! Chat with Zalo counterparts entirely from Telegram; Telegram keeps files
//! forever, local disk is only a transfer buffer with a fixed quota.
//!
//! - Zalo → Telegram: hook into the agent poll loop; new messages (+attachments)
//!   download to `data_dir/bridge/<tid>/` then forward via Bot API.
//! - Telegram → Zalo: long-poll updates, filter echo (`via_bridge`), send via
//!   existing Zalo APIs (chunked upload with resume offsets).
//! - `bridge_files` SQLite table maps msg ↔ path ↔ tg_file_id ↔ zalo_url.
//!   Quota LRU evicts files (keeps rows); a lone file bigger than free space
//!   is refused up front.
//!
//! Auth: `TELEGRAM_BOT_TOKEN` env (or `zlite --auth` Keychain in later step);
//! only `TELEGRAM_OWNER_ID` may control the bridge.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// `"<tg_chat_id>:<zalo_thread_id>,..."` (tg side may be `chat_id/topic_id`).
pub type TopicMap = HashMap<String, String>;

pub fn parse_topic_map(s: &str) -> TopicMap {
    let mut m = TopicMap::new();
    for pair in s.split(',') {
        if let Some((a, b)) = pair.split_once(':') {
            let (a, b) = (a.trim().to_string(), b.trim().to_string());
            if !a.is_empty() && !b.is_empty() {
                m.insert(a, b);
            }
        }
    }
    m
}

/// Default 10 GiB buffer quota.
pub const DEFAULT_QUOTA_BYTES: u64 = 10 * 1024 * 1024 * 1024;

pub fn bridge_dir(data_dir: &std::path::Path, thread_id: &str) -> PathBuf {
    // Sanitize thread id for a directory name.
    let safe: String = thread_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    data_dir.join("bridge").join(safe)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BridgeFile {
    pub msg_id: String,
    pub thread_id: String,
    pub path: String,
    pub tg_file_id: String,
    pub zalo_url: String,
    pub bytes: u64,
    pub offset: u64,
    pub done: bool,
    pub last_access: i64,
}

/// Ensure `bytes_needed` fit in quota: LRU-evict finished files (oldest
/// `last_access` first) until usage drops to 70%, skipping in-progress ones.
/// Returns Err when even empty quota cannot fit (single file too big).
pub fn ensure_quota(
    store: &mut crate::store::MessageStore,
    quota_bytes: u64,
    bytes_needed: u64,
) -> Result<()> {
    if bytes_needed > quota_bytes {
        anyhow::bail!("file quá lớn cho quota ({bytes_needed} > {quota_bytes} byte)");
    }
    let mut files = store.bridge_files_all()?;
    let mut used: u64 = files.iter().map(|f| f.bytes).sum();
    if used + bytes_needed <= quota_bytes {
        return Ok(());
    }
    files.sort_by_key(|f| f.last_access);
    let target = quota_bytes * 70 / 100;
    for f in files {
        if used + bytes_needed <= target {
            break;
        }
        if !f.done {
            continue; // never evict an in-progress upload
        }
        let _ = std::fs::remove_file(&f.path);
        store.bridge_file_delete(&f.msg_id)?;
        used = used.saturating_sub(f.bytes);
    }
    if used + bytes_needed > quota_bytes {
        anyhow::bail!("quota đầy ngay cả sau khi xả LRU");
    }
    Ok(())
}

/// Minimal Telegram Bot API client (long-poll + send). No extra deps.
#[derive(Debug, Clone)]
pub struct TgClient {
    pub base: String,
    pub token: String,
    client: reqwest::Client,
}

impl TgClient {
    pub fn new(token: &str) -> Result<Self> {
        if token.trim().is_empty() {
            anyhow::bail!("thiếu TELEGRAM_BOT_TOKEN");
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        Ok(Self {
            base: "https://api.telegram.org".into(),
            token: token.trim().to_string(),
            client,
        })
    }

    fn url(&self, method: &str) -> String {
        format!("{}/bot{}/{method}", self.base, self.token)
    }

    pub async fn get_updates(&self, offset: i64, timeout_secs: u64) -> Result<Vec<TgUpdate>> {
        let resp = self
            .client
            .get(self.url("getUpdates"))
            .query(&[("offset", offset.to_string()), ("timeout", timeout_secs.to_string())])
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("telegram getUpdates HTTP {}", resp.status());
        }
        let v: serde_json::Value = resp.json().await?;
        if v.get("ok").and_then(|o| o.as_bool()) != Some(true) {
            anyhow::bail!("telegram getUpdates ok=false");
        }
        let out = v
            .get("result")
            .and_then(|r| r.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|u| serde_json::from_value::<TgUpdate>(u.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();
        Ok(out)
    }

    /// Create a forum topic, returns topic id. Requires supergroup with forum enabled.
    pub async fn create_topic(&self, chat: &str, name: &str) -> Result<i64> {
        let resp = self
            .client
            .post(self.url("createForumTopic"))
            .json(&serde_json::json!({"chat_id": chat, "name": name}))
            .send()
            .await?;
        let v: serde_json::Value = resp.json().await?;
        v.pointer("/result/message_thread_id")
            .and_then(|m| m.as_i64())
            .ok_or_else(|| anyhow::anyhow!("createForumTopic failed: {}", v.to_string().chars().take(200).collect::<String>()))
    }

    /// True if chat is a forum supergroup (topics available).
    pub async fn is_forum(&self, chat: &str) -> bool {
        let resp = match self.client.get(self.url("getChat")).query(&[("chat_id", chat)]).send().await {
            Ok(r) => r,
            Err(_) => return false,
        };
        let v: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
        v.pointer("/result/is_forum").and_then(|b| b.as_bool()).unwrap_or(false)
    }

    pub async fn send_text(&self, chat: &str, thread: Option<i64>, text: &str, reply_to: Option<i64>) -> Result<i64> {
        let mut body = serde_json::json!({"chat_id": chat, "text": text});
        if let Some(t) = thread {
            body["message_thread_id"] = t.into();
        }
        if let Some(r) = reply_to {
            body["reply_parameters"] = serde_json::json!({"message_id": r});
        }
        let resp = self.client.post(self.url("sendMessage")).json(&body).send().await?;
        Self::msg_id(&resp.json::<serde_json::Value>().await?)
    }

    /// Guess MIME from extension (offline-safe, no extra deps).
    pub fn guess_mime(path: &std::path::Path) -> String {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "mp4" => "video/mp4",
            "pdf" => "application/pdf",
            "zip" => "application/zip",
            _ => "application/octet-stream",
        }
        .to_string()
    }

    /// Hand-built multipart/form-data (avoids reqwest `multipart` feature).
    fn multipart_body(
        boundary: &str,
        fields: &[(&str, &str)],
        file_field: &str,
        fname: &str,
        mime: &str,
        bytes: &[u8],
    ) -> Vec<u8> {
        let mut b = Vec::with_capacity(bytes.len() + 512);
        for (k, v) in fields {
            b.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes());
        }
        b.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{file_field}\"; filename=\"{fname}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes(),
        );
        b.extend_from_slice(bytes);
        b.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        b
    }

    pub async fn send_file(
        &self,
        chat: &str,
        thread: Option<i64>,
        path: &std::path::Path,
        caption: &str,
        is_photo: bool,
    ) -> Result<(i64, String)> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("đọc file bridge {}", path.display()))?;
        let fname = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file.bin")
            .to_string();
        let mime = Self::guess_mime(path);
        let boundary = format!("zlite{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        let mut fields = vec![("chat_id", chat), ("caption", caption)];
        let thread_s;
        if let Some(t) = thread {
            thread_s = t.to_string();
            fields.push(("message_thread_id", &thread_s));
        }
        let body = Self::multipart_body(
            &boundary,
            &fields,
            if is_photo { "photo" } else { "document" },
            &fname,
            &mime,
            &bytes,
        );
        let method = if is_photo { "sendPhoto" } else { "sendDocument" };
        let resp = self
            .client
            .post(self.url(method))
            .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
            .body(body)
            .send()
            .await?;
        let v: serde_json::Value = resp.json().await?;
        let mid = Self::msg_id(&v)?;
        let fid = v
            .pointer("/result/photo/2/file_id")
            .or_else(|| v.pointer("/result/photo/0/file_id"))
            .or_else(|| v.pointer("/result/document/file_id"))
            .and_then(|f| f.as_str())
            .unwrap_or("")
            .to_string();
        Ok((mid, fid))
    }

    fn msg_id(v: &serde_json::Value) -> Result<i64> {
        v.get("ok")
            .and_then(|o| o.as_bool())
            .filter(|&o| o)
            .ok_or_else(|| anyhow::anyhow!("telegram ok=false: {}", v.to_string().chars().take(200).collect::<String>()))?;
        v.get("result")
            .and_then(|r| r.get("message_id"))
            .and_then(|m| m.as_i64())
            .ok_or_else(|| anyhow::anyhow!("telegram: missing message_id"))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TgUpdate {
    #[serde(default)]
    pub update_id: i64,
    pub message: Option<TgMessage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TgReplyTo {
    #[serde(default)]
    pub message_id: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TgMessage {
    #[serde(default)]
    pub message_id: i64,
    #[serde(default)]
    pub reply_to_message: Option<TgReplyTo>,
    #[serde(default)]
    pub chat: TgChat,
    #[serde(default)]
    pub from: Option<TgUser>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub caption: String,
    pub photo: Option<Vec<TgPhoto>>,
    pub document: Option<TgDoc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TgChat {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub message_thread_id: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TgUser {
    #[serde(default)]
    pub id: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TgPhoto {
    #[serde(default)]
    pub file_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TgDoc {
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub file_name: String,
}

/// Split `"<chat>[/<topic>]..."` key into chat id + optional topic.
pub fn split_tg_target(key: &str) -> (String, Option<i64>) {
    if let Some((c, t)) = key.split_once('/') {
        (c.to_string(), t.parse::<i64>().ok())
    } else {
        (key.to_string(), None)
    }
}

/// Bridge runtime: poll Zalo (reuse agent primitives) + Telegram long-poll.
/// Runs until Ctrl-C. `--dry-run` logs without sending either way.
pub async fn run_bridge_loop(
    session: crate::session::Session,
    db_path: String,
    data_dir: PathBuf,
    quota_bytes: u64,
    topic_map: TopicMap,
    owner_id: i64,
    dry_run: bool,
) -> Result<()> {
    run_bridge_loop_inner(session, db_path, data_dir, quota_bytes, topic_map, owner_id, dry_run, false).await
}

/// Full loop with explicit media switch (PR1: text-only default).
#[allow(clippy::too_many_arguments)]
pub async fn run_bridge_loop_full(
    session: crate::session::Session,
    db_path: String,
    data_dir: PathBuf,
    quota_bytes: u64,
    topic_map: TopicMap,
    owner_id: i64,
    dry_run: bool,
    media: bool,
) -> Result<()> {
    run_bridge_loop_inner(session, db_path, data_dir, quota_bytes, topic_map, owner_id, dry_run, media).await
}

#[allow(clippy::too_many_arguments)]
async fn run_bridge_loop_inner(
    session: crate::session::Session,
    db_path: String,
    data_dir: PathBuf,
    quota_bytes: u64,
    topic_map: TopicMap,
    owner_id: i64,
    dry_run: bool,
    media: bool,
) -> Result<()> {
    let token = std::env::var("TELEGRAM_BOT_TOKEN")
        .unwrap_or_default();
    if token.trim().is_empty() && !dry_run {
        anyhow::bail!("thiếu TELEGRAM_BOT_TOKEN (hoặc chạy --dry-run để test không gửi)");
    }
    // Reverse map: zalo tid -> tg target.
    let mut rev: HashMap<String, String> = HashMap::new();
    for (tg, zt) in &topic_map {
        rev.insert(zt.clone(), tg.clone());
    }
    let tg = TgClient { base: "https://api.telegram.org".into(), token, client: reqwest::Client::builder().timeout(std::time::Duration::from_secs(60)).build()? };
    let mut threads = crate::chat::load_threads_cache();
    crate::store::load_history_into_threads(&db_path, &mut threads, 50);
    let mut state = crate::chat::PollState::default();
    let mut seen: HashSet<String> = HashSet::new();
    for th in &threads {
        for m in &th.messages {
            seen.insert(m.id.clone());
        }
    }
    let mut tg_offset: i64 = 0;
    // Persist offset in thread KV ("__bridge" pseudo-thread).
    if let Ok(st) = crate::store::MessageStore::open(&db_path) {
        if let Some(v) = st.thread_kv_get("__bridge", "tg_offset") {
            tg_offset = v.parse::<i64>().unwrap_or(0);
        }
    }
    println!("[bridge] start: {} pairs, quota {}MB{}",
        topic_map.len(), quota_bytes / 1024 / 1024, if dry_run { " [dry-run]" } else { "" });
    let mut topic_map = topic_map;
    {
        let guesses: Vec<(String, String)> = topic_map
            .iter()
            .filter(|(k, _)| !k.contains('/'))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (chat, ztid) in guesses {
            let tname = threads.iter().find(|t| t.id == ztid).map(|t| t.name.clone()).unwrap_or_else(|| ztid.clone());
            if dry_run {
                println!("[bridge dry] would ensure topic in {chat} for '{tname}'");
                continue;
            }
            if !tg.is_forum(&chat).await {
                eprintln!("[bridge] chat {chat} is not a forum supergroup — tạo topic thất bại; hãy bật Topics trong group settings hoặc map tới topic có sẵn chat/topic");
                continue;
            }
            let short: String = tname.chars().take(100).collect();
            match tg.create_topic(&chat, &short).await {
                Ok(tid) => {
                    println!("[bridge] created topic {tid} in {chat} for '{tname}'");
                    topic_map.remove(&chat);
                    topic_map.insert(format!("{chat}/{tid}"), ztid);
                }
                Err(e) => eprintln!("[bridge] create topic FAIL {chat}: {e}"),
            }
        }
    }
    loop {
        // --- Zalo → TG ---
        let mut st = state.clone();
        let mut th = threads.clone();
        if crate::chat::poll_new_messages(&session, &mut st, &mut th).await.is_ok() {
            state = st;
            let before: HashSet<String> = threads
                .iter()
                .flat_map(|t| t.messages.iter().map(|m| m.id.clone()))
                .collect();
            threads = th;
            for t in threads.clone() {
                let Some(target) = rev.get(&t.id) else { continue };
                for m in t.messages.iter().filter(|m| {
                    !before.contains(&m.id) && !m.id.starts_with("local_") && !m.id.starts_with("tgzalo-")
                }) {
                    seen.insert(m.id.clone());
                    if dry_run {
                        println!("[bridge dry] Z->TG {} :: {}: {}", t.name, m.sender, m.text.chars().take(60).collect::<String>());
                        continue;
                    }
                    if let Ok(mut store) = crate::store::MessageStore::open(&db_path) {
                        match forward_zalo_to_tg(&tg, &mut store, &data_dir, quota_bytes, target, &t, m, media).await {
                            Ok(mid) => println!("[bridge] Z->TG {} mid={mid}", m.id),
                            Err(e) => eprintln!("[bridge] Z->TG {} FAIL: {e}", m.id),
                        }
                    }
                }
            }
            crate::chat::save_threads_cache(&threads);
        }
        // --- TG → Zalo (long-poll 25s) ---
        match tg.get_updates(tg_offset, 25).await {
            Ok(updates) => {
                for u in updates {
                    tg_offset = tg_offset.max(u.update_id + 1);
                    let Some(msg) = u.message else { continue };
                    // owner gate
                    if owner_id != 0
                        && msg.from.as_ref().map(|f| f.id).unwrap_or(0) != owner_id
                    {
                        continue;
                    }
                    // echo guard: our own forwarded messages start with sender line? We tag via caption; skip text starting with our marker is fragile — instead track sent ids in KV.
                    let key = format!("{}:{}", msg.chat.id, msg.message_id);
                    let already = crate::store::MessageStore::open(&db_path)
                        .ok()
                        .and_then(|st| st.thread_kv_get("__bridge", &format!("tgseen-{key}")))
                        .is_some();
                    if already {
                        continue;
                    }
                    // find paired zalo thread: match by chat[/topic] key then fallback chat-only
                    let full = match msg.chat.message_thread_id {
                        Some(tp) => format!("{}/{}", msg.chat.id, tp),
                        None => msg.chat.id.to_string(),
                    };
                    let ztid = topic_map.get(&full).or_else(|| topic_map.get(&msg.chat.id.to_string()));
                    let Some(ztid) = ztid else { continue };
                    let body = if !msg.text.trim().is_empty() { msg.text.clone() } else if !msg.caption.trim().is_empty() { msg.caption.clone() } else { String::new() };
                    // attachments: download tg file → buffer → Zalo send_attachment
                    let mut sent_ok = false;
                    if media && (msg.photo.is_some() || msg.document.is_some()) && !dry_run {
                        // need file_path: getFile
                        let fid = msg.photo.as_ref().and_then(|p| p.last()).map(|p| p.file_id.clone())
                            .or_else(|| msg.document.as_ref().map(|d| d.file_id.clone()))
                            .unwrap_or_default();
                        let fname = msg.document.as_ref().map(|d| if d.file_name.is_empty() { "file.bin".to_string() } else { d.file_name.clone() }).unwrap_or_else(|| "photo.jpg".to_string());
                        if !fid.is_empty() {
                            if let Ok(resp) = tg.client.get(tg.url("getFile")).query(&[("file_id", fid.as_str())]).send().await {
                                if let Ok(v) = resp.json::<serde_json::Value>().await {
                                    if let Some(fp) = v.pointer("/result/file_path").and_then(|p| p.as_str()) {
                                        let durl = format!("https://api.telegram.org/file/bot{}/{}", tg.token, fp);
                                        if let Ok(by) = tg.client.get(&durl).send().await {
                                            if let Ok(bytes) = by.bytes().await {
                                                // quota check via temp store handle
                                                if let Ok(mut store) = crate::store::MessageStore::open(&db_path) {
                                                    let dir = bridge_dir(&data_dir, ztid);
                                                    let _ = std::fs::create_dir_all(&dir);
                                                    let dest = dir.join(format!("tg_{}_{fname}", msg.message_id));
                                                    // rough quota: file size
                                                    match ensure_quota(&mut store, quota_bytes, bytes.len() as u64) {
                                                        Ok(()) => {
                                                            if std::fs::write(&dest, &bytes).is_ok() {
                                                                let th = threads.iter().find(|t| &t.id == ztid);
                                                                let is_group = th.map(|t| t.is_group).unwrap_or(false);
                                                                let cap = body.clone();
                                                                match crate::chat::send_attachment(&session, ztid, is_group, &dest, &cap).await {
                                                                    Ok(_) => {
                                                                        println!("[bridge] TG->Z file {} ok", msg.message_id);
                                                                        sent_ok = true;
                                                                    }
                                                                    Err(e) => eprintln!("[bridge] TG->Z file FAIL: {e}"),
                                                                }
                                                            }
                                                        }
                                                        Err(e) => eprintln!("[bridge] quota refuse: {e}"),
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if !sent_ok && !body.trim().is_empty() && !dry_run {
                        let th = threads.iter().find(|t| &t.id == ztid);
                        let is_group = th.map(|t| t.is_group).unwrap_or(false);
                        let res = if is_group {
                            crate::chat::send_group_message(&session, ztid, &body).await
                        } else {
                            crate::chat::send_message(&session, ztid, &body).await
                        };
                        match res {
                            Ok(_) => println!("[bridge] TG->Z text {} ok", msg.message_id),
                            Err(e) => eprintln!("[bridge] TG->Z text FAIL: {e}"),
                        }
                    }
                    if dry_run {
                        println!("[bridge dry] TG->Z chat={} topic={:?} :: {}", msg.chat.id, msg.chat.message_thread_id, body.chars().take(80).collect::<String>());
                    }
                    if let Ok(mut store) = crate::store::MessageStore::open(&db_path) {
                        let _ = store.thread_kv_set("__bridge", &format!("tgseen-{key}"), "1");
                        let _ = store.thread_kv_set("__bridge", "tg_offset", &tg_offset.to_string());
                    }
                }
            }
            Err(e) => eprintln!("[bridge] tg poll: {e}"),
        }
        if dry_run {
            eprintln!("[bridge dry] tick ok (tg poll attempted, no sends)");
        }
    }
}

/// Forward one Zalo message to Telegram. Downloads attachments into the
/// buffer dir (quota-checked), uploads, records mapping. Returns TG msg id.
#[allow(clippy::too_many_arguments)]
pub async fn forward_zalo_to_tg(
    tg: &TgClient,
    store: &mut crate::store::MessageStore,
    data_dir: &std::path::Path,
    quota_bytes: u64,
    tg_target: &str,
    thread: &crate::chat::Thread,
    msg: &crate::chat::ChatMessage,
    media: bool,
) -> Result<i64> {
    let (chat, topic) = split_tg_target(tg_target);
    let caption = format!("{} · {}", msg.sender, msg.time);
    // Attachment? PR1 is text-only: describe it, skip download.
    let url = msg.link_url.clone().filter(|u| !u.is_empty());
    if url.is_none() || !media {
        let mut text = if msg.text.trim().is_empty() { "(tin nhắn rỗng)".to_string() } else { msg.text.clone() };
        if url.is_some() && !media {
            text.push_str("\n[đính kèm: xem trên Zalo — media ở PR2]");
        }
        // Reply-chain: if this Zalo msg quotes another, map to its TG msg.
        // reply_to_* fields carry original sender/text; resolve via recent mappings is
        // best-effort here (full chain in PR3). Pass None for now.
        let mid = tg.send_text(&chat, topic, &format!("{caption}\n{text}"), None).await?;
        let _ = store.bridge_sent_put(&format!("{chat}:{mid}"), &msg.id);
        let f = BridgeFile {
            msg_id: format!("tgzalo-{}", msg.id),
            thread_id: thread.id.clone(),
            path: String::new(),
            tg_file_id: String::new(),
            zalo_url: String::new(),
            bytes: 0,
            offset: 0,
            done: true,
            last_access: crate::login::unix_now(),
        };
        store.bridge_file_put(&f)?;
        let _ = mid;
        return Ok(mid);
    }
    let url = url.unwrap();
    // Download Zalo CDN → buffer (quota-checked).
    let dir = bridge_dir(data_dir, &thread.id);
    std::fs::create_dir_all(&dir)?;
    let fname = url
        .split(['?', '#'])
        .next()
        .unwrap_or("file.bin")
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("file.bin")
        .to_string();
    let dest = dir.join(format!("{}_{fname}", msg.id.replace('/', "_")));
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(120)).build()?;
    let bytes = client.get(&url).send().await?.bytes().await?.to_vec();
    ensure_quota(store, quota_bytes, bytes.len() as u64)?;
    std::fs::write(&dest, &bytes)?;
    let is_photo = matches!(
        msg.kind,
        crate::message_model::MessageKind::Photo
    ) || dest
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp"))
        .unwrap_or(false);
    let label = if msg.text.trim().is_empty() { fname.clone() } else { msg.text.chars().take(120).collect() };
    let (mid, fid) = tg.send_file(&chat, topic, &dest, &format!("{caption}\n{label}"), is_photo).await?;
    store.bridge_file_put(&BridgeFile {
        msg_id: format!("tgzalo-{}", msg.id),
        thread_id: thread.id.clone(),
        path: dest.to_string_lossy().to_string(),
        tg_file_id: fid,
        zalo_url: url,
        bytes: bytes.len() as u64,
        offset: bytes.len() as u64,
        done: true,
        last_access: crate::login::unix_now(),
    })?;
    Ok(mid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_map_parses_pairs() {
        let m = parse_topic_map("  1/2 : tid1 , 3:tid2,bad, :x ");
        assert_eq!(m.get("1/2").map(String::as_str), Some("tid1"));
        assert_eq!(m.get("3").map(String::as_str), Some("tid2"));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn quota_lru_evicts_oldest_first() {
        let dir = std::env::temp_dir().join(format!("zlite-bridge-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dbp = dir.join("t.db");
        let mut st = crate::store::MessageStore::open(dbp.to_str().unwrap()).unwrap();
        let mk = |id: &str, bytes: u64, last: i64| crate::tg_bridge::BridgeFile {
            msg_id: id.into(),
            thread_id: "t1".into(),
            path: dir.join(id).to_string_lossy().to_string(),
            tg_file_id: String::new(),
            zalo_url: String::new(),
            bytes,
            offset: bytes,
            done: true,
            last_access: last,
        };
        std::fs::write(dir.join("a"), vec![0u8; 500]).unwrap();
        std::fs::write(dir.join("b"), vec![0u8; 300]).unwrap();
        st.bridge_file_put(&mk("a", 500, 1)).unwrap();
        st.bridge_file_put(&mk("b", 300, 2)).unwrap();
        // quota 1000, need 300 more: usage 800+300=1100 > 1000 → evict oldest (a,500)
        // → used 300, 300+300=600 ≤ target 700 → stop. Keeps newest (b).
        super::ensure_quota(&mut st, 1000, 300).unwrap();
        let rest = st.bridge_files_all().unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].msg_id, "b");
        // single file bigger than quota → refuse
        assert!(super::ensure_quota(&mut st, 100, 500).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_target_chat_and_topic() {
        assert_eq!(split_tg_target("-100x"), ("-100x".into(), None));
        assert_eq!(split_tg_target("-100x/5"), ("-100x".to_string(), Some(5)));
    }

    #[test]
    fn bridge_dir_sanitizes_thread() {
        let p = bridge_dir(std::path::Path::new("/d"), "g12 3/4:b");
        assert_eq!(p, PathBuf::from("/d/bridge/g12_3_4_b"));
    }
}
