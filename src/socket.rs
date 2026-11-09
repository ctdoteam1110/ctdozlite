//! Control websocket (`zpw_ws`) — the real-time transport.
//!
//! [O] Decoded from Zalo PC 26.10.11 (`pc-dist_compact-app-pc.*.js`):
//!
//! * URL — `Q[i] + "?" + commonParams`, then the client appends `&t=<ms>`
//!   (`getSocketUrl`, `_connectSocket`). `Q` is the login-config `zpw_ws` list.
//! * Frame — 4-byte header + UTF-8 JSON: `u8 version`, `u16 LE cmd`, `u8 subCmd`
//!   (`_send`, `_getHeader`, `_processEventBlob`).
//! * The server speaks first: `cmd 1` (`AUTHEN`) with `{key, encrypt, data}`.
//!   `key` is base64 and becomes the AES-GCM key (`_cipherKey`).
//! * `encrypt`: 0 plain, 1 base64+zlib, 2 AES-GCM+zlib, 3 AES-GCM
//!   (`_decrypt`, `_decompress`). AES-GCM blob = `iv(16) | aad(16) | ciphertext||tag`.
//! * After auth the payload lists the queues to pull (`data.ctrl_qCmds`); each is pulled with
//!   `{cmd, subCmd, data:{first, reqId:"req_N", lastId, preIds}}` (`doGetOffline`).
//! * Ping — `{version:1, cmd:2, subCmd:1, data:{eventId}}`; `_onPong` matches `eventId`.
//!
//! HTTP long-polling only ever carries durable actions; live VoIP invites are socket-only,
//! which is why a polling-only callee sees `act:"answer" status:5` and never `act:"request"`.

use aes::{Aes128, Aes256};
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::AesGcm;
use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use futures_util::{SinkExt, StreamExt};
use generic_array::typenum::U16;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{COOKIE, ORIGIN, USER_AGENT};
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

use crate::session::Session;

// -- command codes (`A={AUTHEN:1,PING:2,...,CTRL_*}` in the bundle) -------------

pub const CMD_AUTHEN: u16 = 1;
pub const CMD_PING: u16 = 2;
pub const CMD_PING_ACTIVE: u16 = 4;
pub const SUB_KEEP_CONNECTION: u8 = 1;
pub const SUB_KEEP_ACTIVE: u8 = 2;

pub const CMD_CTRL_PUSH_CTRL: u16 = 601;
pub const CMD_CTRL_ACTION: u16 = 602;
pub const CMD_CTRL_PULL_CTRL: u16 = 603;
pub const CMD_CTRL_PULL_VOIP: u16 = 604;

pub const CMD_PUSH_MSG_1_1: u16 = 501;
pub const CMD_PUSH_MSG_GROUP: u16 = 521;
pub const CMD_PULL_MSG_1_1: u16 = 510;
pub const CMD_PULL_MSG_GROUP: u16 = 511;
pub const CMD_PULL_MSG_513: u16 = 513;
pub const CMD_PULL_MSG_514: u16 = 514;
pub const CMD_PULL_MSG_515: u16 = 515;
pub const CMD_PULL_MSG_517: u16 = 517;
pub const CMD_PULL_MSG_518: u16 = 518;
pub const CMD_HAS_MSG: u16 = 533;
pub const CMD_PUSH_MISS_MSG: u16 = 534;
pub const CMD_GET_MSG: u16 = 570;
pub const CMD_SYNC_REQUEST: u16 = 590;
pub const CMD_SYNC_ACK_DELETE: u16 = 591;
pub const CMD_SYNC_MOBILE_WAKE_UP: u16 = 592;

static WS_OUTBOUND_TX: std::sync::Mutex<Option<mpsc::UnboundedSender<Vec<u8>>>> =
    std::sync::Mutex::new(None);

struct PendingSyncRequest {
    cmd: u16,
    frame_bytes: usize,
    connection: mpsc::UnboundedSender<Vec<u8>>,
    reply: oneshot::Sender<std::result::Result<Value, String>>,
}

static SYNC_REQUESTS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, PendingSyncRequest>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
static NEXT_SYNC_REQUEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct SyncRequestCleanup(String);

impl Drop for SyncRequestCleanup {
    fn drop(&mut self) {
        // Cancellation and timeouts must remove senders even when no server reply arrives.
        if let Ok(mut pending) = SYNC_REQUESTS.lock() {
            pending.remove(&self.0);
        }
    }
}

/// A queued request lost its acknowledgement; delivery must not be assumed to have failed.
#[derive(Debug)]
pub(crate) struct SyncRequestUnconfirmed(pub(crate) String);

impl std::fmt::Display for SyncRequestUnconfirmed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for SyncRequestUnconfirmed {}

/// Send a sync API frame and await its correlated server acknowledgement.
///
/// # Errors
/// Rejects unsupported commands, unavailable connections, send failures, disconnects and timeouts.
/// A nonzero server response is returned intact for the protocol caller to classify.
pub async fn request_sync_frame(cmd: u16, data: Value, timeout: Duration) -> Result<Value> {
    if !matches!(
        cmd,
        CMD_SYNC_REQUEST | CMD_SYNC_ACK_DELETE | CMD_SYNC_MOBILE_WAKE_UP
    ) || !data.is_object()
    {
        bail!("Yêu cầu đồng bộ WebSocket không hợp lệ");
    }
    let connection = WS_OUTBOUND_TX
        .lock()
        .map_err(|_| anyhow!("Không đọc được trạng thái kết nối đồng bộ"))?
        .as_ref()
        .filter(|tx| !tx.is_closed())
        .cloned()
        .ok_or_else(|| anyhow!("Chưa kết nối máy chủ thời gian thực; hãy chờ rồi thử lại"))?;
    let request_id = format!(
        "req_sync_{}_{}",
        now_ms(),
        NEXT_SYNC_REQUEST.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let (reply, response) = oneshot::channel();
    let mut data = data;
    if let Some(object) = data.as_object_mut() {
        object.insert("reqId".into(), Value::String(request_id.clone()));
    }
    let frame = encode_frame(cmd, 0, 1, Some(&data));
    SYNC_REQUESTS
        .lock()
        .map_err(|_| anyhow!("Không đăng ký được yêu cầu đồng bộ"))?
        .insert(
            request_id.clone(),
            PendingSyncRequest {
                cmd,
                frame_bytes: frame.len(),
                connection: connection.clone(),
                reply,
            },
        );
    let _cleanup = SyncRequestCleanup(request_id.clone());
    connection
        .send(frame)
        .map_err(|_| anyhow!("Kết nối đồng bộ đã đóng trước khi gửi"))?;
    match tokio::time::timeout(timeout, response).await {
        Ok(Ok(Ok(value))) => Ok(value),
        Ok(Ok(Err(message))) => Err(SyncRequestUnconfirmed(message).into()),
        Ok(Err(_)) => Err(SyncRequestUnconfirmed(
            "Kết nối đồng bộ đã đóng trước khi nhận xác nhận".into(),
        )
        .into()),
        Err(_) => Err(SyncRequestUnconfirmed(
            "Chưa nhận được xác nhận đồng bộ từ máy chủ thời gian thực".into(),
        )
        .into()),
    }
}

fn complete_sync_request(cmd: u16, payload: &Value) -> bool {
    let parsed;
    let payload = if let Some(text) = payload.get("data").and_then(Value::as_str) {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return false;
        };
        parsed = value;
        &parsed
    } else {
        payload
    };
    let Some(request_id) = payload.pointer("/data/reqId").and_then(Value::as_str) else {
        return false;
    };
    let Ok(mut pending) = SYNC_REQUESTS.lock() else {
        return false;
    };
    if !pending
        .get(request_id)
        .is_some_and(|request| request.cmd == cmd)
    {
        return false;
    }
    if let Some(request) = pending.remove(request_id) {
        // A cancelled receiver is harmless; its request has already been removed.
        let _ = request.reply.send(Ok(payload.clone()));
    }
    true
}

// Publish only authenticated connections, and never let an older connection clear its successor.
struct OutboundRegistration(mpsc::UnboundedSender<Vec<u8>>, Option<u16>);

impl OutboundRegistration {
    fn activate(&self) {
        if let Ok(mut current) = WS_OUTBOUND_TX.lock() {
            *current = Some(self.0.clone());
        }
    }
}

impl Drop for OutboundRegistration {
    fn drop(&mut self) {
        if let Ok(mut current) = WS_OUTBOUND_TX.lock() {
            if current.as_ref().is_some_and(|tx| tx.same_channel(&self.0)) {
                *current = None;
            }
        }
        if let Ok(mut pending) = SYNC_REQUESTS.lock() {
            let ids: Vec<_> = pending
                .iter()
                .filter(|(_, request)| request.connection.same_channel(&self.0))
                .map(|(id, _)| id.clone())
                .collect();
            for id in ids {
                if let Some(request) = pending.remove(&id) {
                    // Disconnect notification is best-effort when the caller was cancelled.
                    // A close code and byte count are useful without exposing the wire body/reason.
                    let code = self.1.map_or_else(
                        || "không có mã đóng".into(),
                        |code| format!("mã đóng {code}"),
                    );
                    let _ = request.reply.send(Err(format!(
                        "WebSocket đã ngắt trong lúc đồng bộ ({code}; yêu cầu {} byte)",
                        request.frame_bytes
                    )));
                }
            }
        }
    }
}

/// Whether an authenticated WebSocket can currently accept outgoing frames.
pub fn has_authenticated_connection() -> bool {
    WS_OUTBOUND_TX
        .lock()
        .ok()
        .is_some_and(|current| current.as_ref().is_some_and(|tx| !tx.is_closed()))
}

/// Send a raw encoded frame over the active `zpw_ws` WebSocket if connected.
pub fn send_ws_frame(cmd: u16, sub_cmd: u8, version: u8, data: Option<&Value>) -> bool {
    let frame = encode_frame(cmd, sub_cmd, version, data);
    if let Ok(guard) = WS_OUTBOUND_TX.lock() {
        if let Some(ref tx) = *guard {
            return tx.send(frame).is_ok();
        }
    }
    false
}

/// Emit `SYNC_MESSAGE.REQUEST_MOBILE_WAKE_UP` (`cmd 592`, `subCmd 0`) over the live WebSocket
/// to wake up the user's phone Zalo client for cross-device message sync.
pub fn send_mobile_wakeup(sync_id: &str) -> bool {
    let req_id = format!("req_592_{}", now_ms());
    let body = json!({
        "reqId": req_id,
        "syncId": sync_id,
        "toDevice": 0
    });
    send_ws_frame(CMD_SYNC_MOBILE_WAKE_UP, 0, 1, Some(&body))
}

/// Emit `SYNC_MESSAGE.REQUEST` (`cmd 590`, `subCmd 0`) over the live WebSocket.
pub fn send_sync_message_ws_request(payload: &Value) -> bool {
    let req_id = format!("req_590_{}", now_ms());
    let body = json!({
        "reqId": req_id,
        "data": payload
    });
    send_ws_frame(CMD_SYNC_REQUEST, 0, 1, Some(&body))
}
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) ZaloPC/26.10.11 Chrome/122.0.0.0 Electron/29.0.0 Safari/537.36";

const PING_SECS: u64 = 15;

// -- wire level ----------------------------------------------------------------

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn trace_on() -> bool {
    std::env::var("ZCALL_WS_TRACE").ok().as_deref() == Some("1")
}

fn trace(msg: impl std::fmt::Display) {
    if trace_on() {
        eprintln!("[ws] {msg}");
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

/// Encode one frame: `u8 version | u16 LE cmd | u8 subCmd | JSON body`.
pub fn encode_frame(cmd: u16, sub_cmd: u8, version: u8, data: Option<&Value>) -> Vec<u8> {
    let body = data.map(|d| d.to_string()).unwrap_or_default();
    let mut out = Vec::with_capacity(4 + body.len());
    out.push(version);
    out.extend_from_slice(&cmd.to_le_bytes());
    out.push(sub_cmd);
    out.extend_from_slice(body.as_bytes());
    out
}

/// Decode one frame into `(version, cmd, subCmd, JSON)`.
pub fn decode_frame(buf: &[u8]) -> Option<(u8, u16, u8, Value)> {
    if buf.len() < 4 {
        return None;
    }
    let version = buf[0];
    let cmd = u16::from_le_bytes([buf[1], buf[2]]);
    let sub_cmd = buf[3];
    let body = &buf[4..];
    if body.is_empty() {
        return Some((version, cmd, sub_cmd, Value::Null));
    }
    let value = serde_json::from_slice(body).ok()?;
    Some((version, cmd, sub_cmd, value))
}

/// `decodeURIComponent` for the AES branch, tolerating raw `+`/`/`/`=` base64.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Inflate a decrypted payload.
///
/// [O] The server sends **gzip** (`1f 8b 08 …`) for `encrypt` 2/3; the JS side calls a generic
/// `inflate` helper that sniffs the container. Older captures decoded zlib, so try gzip first
/// (by magic), then zlib, then raw deflate.
fn inflate(buf: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;
    if trace_on() {
        let head: String = buf.iter().take(12).map(|b| format!("{b:02x}")).collect();
        trace(format!("inflate input {} bytes head={head}", buf.len()));
    }
    #[allow(clippy::type_complexity)] // two named decoder constructors
    let attempts: [(&str, fn(&[u8]) -> Box<dyn Read + '_>); 2] = [
        ("gzip", |b| Box::new(flate2::read::GzDecoder::new(b))),
        ("zlib", |b| Box::new(flate2::read::ZlibDecoder::new(b))),
    ];
    let order: [usize; 2] = if buf.starts_with(&[0x1f, 0x8b]) {
        [0, 1]
    } else {
        [1, 0]
    };
    let mut last_err: Option<anyhow::Error> = None;
    for idx in order {
        let (name, make) = attempts[idx];
        let mut out = Vec::new();
        match make(buf).read_to_end(&mut out) {
            Ok(_) if !out.is_empty() => {
                trace(format!(
                    "inflate ok via {name}: {} -> {} bytes",
                    buf.len(),
                    out.len()
                ));
                return Ok(out);
            }
            Ok(_) => last_err = Some(anyhow!("{name}: empty output")),
            Err(e) => last_err = Some(anyhow!("{name}: {e}")),
        }
    }
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(buf).read_to_end(&mut out)?;
    if out.is_empty() {
        return Err(last_err.unwrap_or_else(|| anyhow!("empty inflate output")));
    }
    Ok(out)
}

/// AES-GCM with a 16-byte nonce: `iv(16) | aad(16) | ciphertext||tag(16)`.
fn gcm_decrypt(key: &[u8], blob: &[u8]) -> Result<Vec<u8>> {
    if blob.len() < 48 {
        bail!("aes-gcm blob too short ({})", blob.len());
    }
    let nonce = aes_gcm::Nonce::<U16>::from_slice(&blob[..16]);
    let payload = Payload {
        msg: &blob[32..],
        aad: &blob[16..32],
    };
    let plain = match key.len() {
        32 => AesGcm::<Aes256, U16>::new_from_slice(key)
            .map_err(|e| anyhow!("aes key: {e}"))?
            .decrypt(nonce, payload)
            .map_err(|e| anyhow!("aes-gcm: {e}"))?,
        16 => AesGcm::<Aes128, U16>::new_from_slice(key)
            .map_err(|e| anyhow!("aes key: {e}"))?
            .decrypt(nonce, payload)
            .map_err(|e| anyhow!("aes-gcm: {e}"))?,
        n => bail!("unsupported AES key length {n}"),
    };
    Ok(plain)
}

/// Apply the envelope's `encrypt` scheme to its `data` field.
pub fn decrypt_payload(cipher_key: Option<&[u8]>, encrypt: u8, data: &str) -> Result<Vec<u8>> {
    match encrypt {
        0 => Ok(data.as_bytes().to_vec()),
        1 => inflate(&B64.decode(data)?),
        2 => {
            let key = cipher_key.ok_or_else(|| anyhow!("no cipher key"))?;
            let raw = B64.decode(percent_decode(data))?;
            inflate(&gcm_decrypt(key, &raw)?)
        }
        3 => {
            let key = cipher_key.ok_or_else(|| anyhow!("no cipher key"))?;
            let raw = B64.decode(percent_decode(data))?;
            gcm_decrypt(key, &raw)
        }
        other => bail!("unsupported encrypt type {other}"),
    }
}

// -- client --------------------------------------------------------------------

/// One decrypted socket event.
#[derive(Debug, Clone)]
pub struct WsEvent {
    pub cmd: u16,
    pub sub_cmd: u8,
    pub version: u8,
    /// Decrypted body when possible, otherwise the raw envelope.
    pub payload: Value,
}

impl WsEvent {
    /// `true` for anything that may carry a VoIP invite (`601` push, `604` pull page).
    pub fn is_voip_ish(&self) -> bool {
        matches!(
            self.cmd,
            CMD_CTRL_PUSH_CTRL | CMD_CTRL_ACTION | CMD_CTRL_PULL_CTRL | CMD_CTRL_PULL_VOIP
        )
    }

    /// `true` for real-time or offline chat message frames (`501`, `521`, `510`, `511`, `515`, `570`, …).
    pub fn is_chat_ish(&self) -> bool {
        matches!(
            self.cmd,
            CMD_PUSH_MSG_1_1
                | CMD_PUSH_MSG_GROUP
                | CMD_PULL_MSG_1_1
                | CMD_PULL_MSG_GROUP
                | CMD_PULL_MSG_513
                | CMD_PULL_MSG_514
                | CMD_PULL_MSG_515
                | CMD_PULL_MSG_517
                | CMD_PULL_MSG_518
                | CMD_PUSH_MISS_MSG
                | CMD_GET_MSG
        )
    }
}

fn build_url(sess: &Session, endpoint_idx: usize) -> String {
    let endpoints = &sess.domains.ws;
    let base = std::env::var("ZCALL_WS")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            if endpoints.is_empty() {
                None
            } else {
                Some(endpoints[endpoint_idx % endpoints.len()].clone())
            }
        })
        .unwrap_or_else(|| "wss://ws1-msg.chat.zalo.me".to_string());
    // The server's `zlpws` front end answers `501 Not Implemented` to a request line with an
    // empty path (`GET ?zpw_ver=...`), so always send an explicit `/`.
    let (host, query) = match base.split_once('?') {
        Some((h, q)) => (h, Some(q)),
        None => (base.as_str(), None),
    };
    // Authority-only URLs (`wss://host`) need an explicit `/` before the query string.
    let after_scheme = host.split_once("://").map(|(_, rest)| rest).unwrap_or(host);
    let has_path = after_scheme.contains('/');
    let mut url = format!("{host}{}", if has_path { "" } else { "/" });
    let extra = format!("{}&t={}", sess.common_params(), now_ms());
    url.push('?');
    if let Some(q) = query {
        url.push_str(q);
        url.push('&');
    }
    url.push_str(&extra);
    url
}

/// Queues the server told us to pull.
///
/// The auth payload lists the exact shard-local queue names (`603_1`, `610_1`, …); pulling a
/// queue the worker does not own (e.g. `604_0`) closes the socket with `3002 NOT_FOUND_WORKER`,
/// so only server-listed queues are used unless `ZCALL_WS_FORCE_VOIP=1` asks for `604_0`.
fn queues_from_auth(auth: &Value) -> Vec<(u16, u8)> {
    let mut out: Vec<(u16, u8)> = Vec::new();
    for key in ["ctrl_qCmds", "qCmds"] {
        if let Some(arr) = auth
            .pointer(&format!("/data/{key}"))
            .and_then(|v| v.as_array())
        {
            for q in arr {
                let cmd = q.get("cmd").and_then(|v| v.as_u64());
                let sub = q.get("subCmd").and_then(|v| v.as_u64()).unwrap_or(0);
                if let Some(cmd) = cmd {
                    let pair = (cmd as u16, sub as u8);
                    if !out.contains(&pair) {
                        out.push(pair);
                    }
                }
            }
        }
    }
    if std::env::var("ZCALL_WS_FORCE_VOIP").ok().as_deref() == Some("1")
        && !out.iter().any(|(c, _)| *c == CMD_CTRL_PULL_VOIP)
    {
        out.push((CMD_CTRL_PULL_VOIP, 0));
    }
    out
}

fn pull_frame(cmd: u16, sub_cmd: u8, req_id: &str) -> Vec<u8> {
    let data = json!({
        "first": true,
        "reqId": req_id,
        "lastId": 1,
        "preIds": [],
    });
    encode_frame(cmd, sub_cmd, 1, Some(&data))
}

fn pull_next_frame(cmd: u16, sub_cmd: u8, req_id: &str, last_id: Value, pre_ids: Value) -> Vec<u8> {
    let data = json!({
        "first": false,
        "reqId": req_id,
        "lastId": last_id,
        "preIds": pre_ids,
    });
    encode_frame(cmd, sub_cmd, 1, Some(&data))
}

/// Connect, authenticate, pull the control/voip queues, then stream events.
///
/// Reconnects forever with capped backoff. Dropping the returned receiver stops the task.
pub fn spawn(sess: Session) -> mpsc::UnboundedReceiver<WsEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut backoff = 2u64;
        let mut endpoint_idx = 0usize;
        loop {
            match run_once(&sess, &tx, endpoint_idx).await {
                Ok(reason) => trace(format!("closed: {reason}")),
                Err(e) => trace(format!("error: {e:#}")),
            }
            if tx.is_closed() {
                break;
            }
            // `3002 NOT_FOUND_WORKER` means this shard does not host the session; rotate.
            endpoint_idx = endpoint_idx.wrapping_add(1);
            tokio::time::sleep(Duration::from_secs(backoff)).await;
            backoff = (backoff * 2).min(30);
        }
    });
    rx
}

async fn run_once(
    sess: &Session,
    tx: &mpsc::UnboundedSender<WsEvent>,
    endpoint_idx: usize,
) -> Result<String> {
    let url = build_url(sess, endpoint_idx);
    trace(format!("connect {url}"));

    let mut req = url.clone().into_client_request()?;
    {
        let h = req.headers_mut();
        if let Some(sek) = sess.zpw_sek.as_deref().filter(|s| !s.is_empty()) {
            h.insert(
                COOKIE,
                HeaderValue::from_str(&format!("zpw_sek={sek}; _zlang=vi"))?,
            );
            h.insert("zpw_sek", HeaderValue::from_str(sek)?);
        }
        h.insert(ORIGIN, HeaderValue::from_static("https://chat.zalo.me"));
        h.insert(USER_AGENT, HeaderValue::from_static(UA));
    }

    let (mut ws, resp) = match connect_async(req).await {
        Ok(pair) => pair,
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
            let mut detail = format!("handshake status {}", resp.status());
            for (k, v) in resp.headers() {
                detail.push_str(&format!("\n  {k}: {}", v.to_str().unwrap_or("<bin>")));
            }
            bail!("{detail}");
        }
        Err(e) => return Err(e.into()),
    };
    trace(format!("handshake {}", resp.status()));

    let mut cipher_key: Option<Vec<u8>> = None;
    let mut authed = false;
    let mut req_id = 0u64;
    let mut ping = tokio::time::interval(Duration::from_secs(PING_SECS));
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ping.tick().await; // consume the immediate first tick

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let mut outbound = OutboundRegistration(out_tx, None);
    loop {
        tokio::select! {
            _ = ping.tick() => {
                if authed {
                    let body = json!({ "eventId": now_ms() });
                    let f = encode_frame(CMD_PING, SUB_KEEP_CONNECTION, 1, Some(&body));
                    ws.send(Message::Binary(f)).await?;
                    trace("-> ping");
                }
            }
            Some(frame) = out_rx.recv() => {
                if authed {
                    ws.send(Message::Binary(frame)).await?;
                    trace("-> outbound ws frame");
                }
            }
            incoming = ws.next() => {
                let Some(incoming) = incoming else {
                    return Ok("eof".into());
                };
                let incoming = incoming?;
                let buf = match incoming {
                    Message::Binary(b) => b,
                    Message::Text(t) => {
                        trace(format!("<- text {}", truncate(t.as_str(), 200)));
                        continue;
                    }
                    Message::Ping(p) => {
                        ws.send(Message::Pong(p)).await?;
                        continue;
                    }
                    Message::Close(c) => {
                        outbound.1 = c.as_ref().map(|frame| u16::from(frame.code));
                        return Ok(format!("close {c:?}"));
                    }
                    _ => continue,
                };

                let Some((version, cmd, sub_cmd, envelope)) = decode_frame(&buf) else {
                    trace(format!("<- undecodable {} bytes", buf.len()));
                    continue;
                };

                let encrypt = envelope.get("encrypt").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
                let err = envelope.get("error_code").and_then(|v| v.as_i64());
                trace(format!(
                    "<- v{version} cmd={cmd} sub={sub_cmd} enc={encrypt} err={err:?} {}",
                    truncate(&envelope.to_string(), 300)
                ));

                if cmd == CMD_AUTHEN {
                    if let Some(key) = envelope.get("key").and_then(|v| v.as_str()) {
                        if !key.is_empty() {
                            cipher_key = Some(B64.decode(key)?);
                            trace(format!(
                                "cipher key {} bytes",
                                cipher_key.as_ref().map(|k| k.len()).unwrap_or(0)
                            ));
                        }
                    }
                    let raw = envelope.get("data").and_then(|v| v.as_str()).unwrap_or_default();
                    let plain = decrypt_payload(cipher_key.as_deref(), encrypt, raw)?;
                    let text = String::from_utf8_lossy(&plain).into_owned();
                    trace(format!("auth body {}", truncate(&text, 1200)));
                    let auth: Value = serde_json::from_str(&text)?;
                    if auth.get("error_code").and_then(|v| v.as_i64()) != Some(0) {
                        return Err(anyhow!("auth rejected: {text}"));
                    }
                    authed = true;
                    outbound.activate();
                    let _ = tx.send(WsEvent { cmd, sub_cmd, version, payload: auth.clone() });

                    for (qcmd, qsub) in queues_from_auth(&auth) {
                        let id = format!("req_{req_id}");
                        req_id += 1;
                        ws.send(Message::Binary(pull_frame(qcmd, qsub, &id))).await?;
                        trace(format!("-> pull {qcmd}_{qsub} {id}"));
                    }
                    continue;
                }

                let payload = if encrypt == 0 {
                    envelope.clone()
                } else {
                    match envelope.get("data").and_then(|v| v.as_str()) {
                        Some(d) => decrypt_payload(cipher_key.as_deref(), encrypt, d)
                            .ok()
                            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                            .unwrap_or_else(|| envelope.clone()),
                        None => envelope.clone(),
                    }
                };
                if matches!(cmd, CMD_SYNC_REQUEST | CMD_SYNC_ACK_DELETE | CMD_SYNC_MOBILE_WAKE_UP) {
                    complete_sync_request(cmd, &payload);
                }
                if cmd == CMD_HAS_MSG {
                    if let (Some(qid), Some(ids)) = (
                        payload.pointer("/data/queueId").cloned(),
                        payload.pointer("/data/ids").cloned(),
                    ) {
                        let id = format!("req_{req_id}");
                        req_id += 1;
                        let get_req = json!({
                            "queueId": qid,
                            "ids": ids,
                            "lastId": "1",
                            "reqId": id,
                        });
                        let _ = ws
                            .send(Message::Binary(
                                encode_frame(CMD_GET_MSG, 0, 1, Some(&get_req)),
                            ))
                            .await;
                        trace(format!("-> get_msg 570_0 {id}"));
                    }
                }
                if payload.pointer("/data/more").and_then(|v| v.as_i64()) == Some(1) {
                    let qkey = format!("{cmd}_{sub_cmd}");
                    let last_id = payload
                        .pointer("/data/lastActionId")
                        .or_else(|| payload.pointer(&format!("/data/queueStatus/{qkey}/lastId")))
                        .cloned()
                        .unwrap_or(json!(1));
                    let pre_ids = payload
                        .pointer(&format!("/data/queueStatus/{qkey}/ids"))
                        .cloned()
                        .unwrap_or_else(|| json!([]));
                    let id = format!("req_{req_id}");
                    req_id += 1;
                    let _ = ws
                        .send(Message::Binary(
                            pull_next_frame(cmd, sub_cmd, &id, last_id, pre_ids),
                        ))
                        .await;
                    trace(format!("-> pull next {qkey} {id}"));
                }
                let _ = tx.send(WsEvent { cmd, sub_cmd, version, payload });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip() {
        let body = json!({ "a": 1, "b": "x" });
        let f = encode_frame(604, 0, 1, Some(&body));
        assert_eq!(f[0], 1);
        assert_eq!(u16::from_le_bytes([f[1], f[2]]), 604);
        assert_eq!(f[3], 0);
        let (v, cmd, sub, val) = decode_frame(&f).expect("decode");
        assert_eq!((v, cmd, sub), (1, 604, 0));
        assert_eq!(val, body);
    }

    #[test]
    fn frame_without_body() {
        let f = encode_frame(2, 1, 1, None);
        assert_eq!(f.len(), 4);
        let (_, cmd, sub, val) = decode_frame(&f).expect("decode");
        assert_eq!((cmd, sub), (2, 1));
        assert!(val.is_null());
    }

    #[test]
    fn queues_follow_auth_lists() {
        let auth = json!({ "error_code": 0, "data": {
            "ctrl_qCmds": [
                { "cmd": 610, "subCmd": 1 }, { "cmd": 610, "subCmd": 1 }, { "cmd": 603, "subCmd": 1 }
            ],
            "qCmds": [{ "cmd": 511, "subCmd": 1 }]
        } });
        assert_eq!(queues_from_auth(&auth), vec![(610, 1), (603, 1), (511, 1)]);

        // An empty list stays empty: injecting an unowned queue kills the socket.
        let auth = json!({ "error_code": 0, "data": {} });
        assert!(queues_from_auth(&auth).is_empty());
    }

    #[test]
    fn percent_decode_passthrough() {
        assert_eq!(percent_decode("abc+/="), "abc+/=");
        assert_eq!(percent_decode("a%2Bb"), "a+b");
    }

    #[test]
    fn gcm_rejects_short_blob() {
        let err = gcm_decrypt(&[0u8; 32], &[0u8; 16]).unwrap_err().to_string();
        assert!(err.contains("too short"), "{err}");
    }
}

#[cfg(test)]
mod outbound_registration_tests {
    use super::*;
    static TRANSPORT_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn only_authenticated_live_connection_accepts_outbound_frames() {
        let _serial = TRANSPORT_TEST_LOCK.lock().await;
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let first = OutboundRegistration(sender, None);
        assert!(!send_mobile_wakeup("test-sync"));
        first.activate();
        assert!(send_mobile_wakeup("test-sync"));
        assert!(receiver.try_recv().is_ok());
        let (new_sender, mut new_receiver) = mpsc::unbounded_channel();
        let next = OutboundRegistration(new_sender, None);
        next.activate();
        drop(first);
        assert!(send_mobile_wakeup("test-sync"));
        assert!(new_receiver.try_recv().is_ok());
        drop(next);
        assert!(!send_mobile_wakeup("test-sync"));
    }

    #[tokio::test]
    async fn request_waits_for_matching_server_result_and_preserves_rejection() {
        let _serial = TRANSPORT_TEST_LOCK.lock().await;
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let connection = OutboundRegistration(sender, None);
        connection.activate();
        let payload = json!({"syncId":"test-sync","syncType":0});
        let request = tokio::spawn(request_sync_frame(
            CMD_SYNC_REQUEST,
            json!({"data":payload.clone()}),
            Duration::from_secs(1),
        ));
        let frame = receiver.recv().await.unwrap();
        let (_, cmd, sub_cmd, body) = decode_frame(&frame).unwrap();
        assert_eq!((cmd, sub_cmd), (590, 0));
        assert_eq!(body["data"], payload);
        let id = body["reqId"].as_str().unwrap();
        let reply = json!({"error_code":211,"data":{"reqId":id}});
        assert!(!complete_sync_request(CMD_SYNC_MOBILE_WAKE_UP, &reply));
        assert!(!complete_sync_request(
            CMD_SYNC_REQUEST,
            &json!({"error_code":0,"data":{"reqId":"wrong-request"}})
        ));
        assert!(complete_sync_request(CMD_SYNC_REQUEST, &reply));
        assert_eq!(request.await.unwrap().unwrap()["error_code"], 211);
        assert!(SYNC_REQUESTS.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn timeout_and_disconnect_remove_pending_requests() {
        let _serial = TRANSPORT_TEST_LOCK.lock().await;
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let mut connection = OutboundRegistration(sender, None);
        connection.activate();
        let request = tokio::spawn(request_sync_frame(
            CMD_SYNC_REQUEST,
            json!({"data":{}}),
            Duration::from_millis(10),
        ));
        receiver.recv().await.unwrap();
        let timeout_error = request.await.unwrap().unwrap_err();
        assert!(timeout_error.is::<SyncRequestUnconfirmed>());
        assert!(SYNC_REQUESTS.lock().unwrap().is_empty());
        let request = tokio::spawn(request_sync_frame(
            CMD_SYNC_REQUEST,
            json!({"data":{"private_fixture":"not-for-diagnostics"}}),
            Duration::from_secs(10),
        ));
        let sent_bytes = receiver.recv().await.unwrap().len();
        connection.1 = Some(3002);
        drop(connection);
        let disconnect_error = tokio::time::timeout(Duration::from_secs(1), request)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(disconnect_error.is::<SyncRequestUnconfirmed>());
        assert!(disconnect_error.to_string().contains("mã đóng 3002"));
        assert!(disconnect_error
            .to_string()
            .contains(&format!("{sent_bytes} byte")));
        assert!(!disconnect_error.to_string().contains("not-for-diagnostics"));
        assert!(SYNC_REQUESTS.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn caller_cancellation_cleans_up_without_a_server_reply() {
        let _serial = TRANSPORT_TEST_LOCK.lock().await;
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let connection = OutboundRegistration(sender, None);
        connection.activate();
        let request = tokio::spawn(request_sync_frame(
            CMD_SYNC_REQUEST,
            json!({"data":{}}),
            Duration::from_secs(10),
        ));
        receiver.recv().await.unwrap();
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        assert!(SYNC_REQUESTS.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unencrypted_json_envelope_can_resolve_the_request() {
        let _serial = TRANSPORT_TEST_LOCK.lock().await;
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let connection = OutboundRegistration(sender, None);
        connection.activate();
        let request = tokio::spawn(request_sync_frame(
            CMD_SYNC_REQUEST,
            json!({"data":{}}),
            Duration::from_secs(1),
        ));
        let frame = receiver.recv().await.unwrap();
        let (_, _, _, body) = decode_frame(&frame).unwrap();
        let reply = json!({"error_code":0,"data":{"reqId":body["reqId"]}});
        assert!(complete_sync_request(
            CMD_SYNC_REQUEST,
            &json!({"encrypt":0,"data":reply.to_string()})
        ));
        assert_eq!(request.await.unwrap().unwrap()["error_code"], 0);
    }
}
