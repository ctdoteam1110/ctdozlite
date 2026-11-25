//! Paged history sync for groups, "Cloud của tôi", and mobile message sync (`Đồng bộ tin nhắn từ điện thoại`, #19, #23, #68).
//!
//! - Group & Cloud history walks `/api/cm/getoldv2` backwards 50 messages at a time from the oldest
//!   message in the captured account's store, so a second run resumes where the first stopped.
//! - Phone message sync (`sync_phone_messages`, #68) starts a Sync V2 transfer,
//!   drains `lp_chat` multi-batch queues (`getChatFromPolling`), batch-queries `/api/preloadconvers/get-last-msgs`
//!   across all 1-1 and group threads in 25-thread chunks, and persists to the captured account's store.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::Result;

use crate::chat::{self, Thread};
use crate::session::Session;
use crate::store::{self, MessageStore};

const PAGE: u32 = 50;
/// Pages per thread per run (1 000 messages); run again to go further back.
const MAX_PAGES: usize = 20;
const PAUSE: Duration = Duration::from_millis(400);

/// Set by the GUI's "Dừng" button; checked between pages.
pub static CANCEL: AtomicBool = AtomicBool::new(false);
/// True while a Cloud/Group sync runs, so a second click doesn't start a parallel one.
pub static RUNNING: AtomicBool = AtomicBool::new(false);

/// Set by the GUI's "Dừng đồng bộ điện thoại" button (#68).
pub static PHONE_SYNC_CANCEL: AtomicBool = AtomicBool::new(false);
/// True while a phone message sync (`sync_phone_messages`) is active (#68).
pub static PHONE_SYNC_RUNNING: AtomicBool = AtomicBool::new(false);

/// Commit a fetched page to the captured account on the blocking pool and count new messages.
///
/// # Errors
/// Returns an error if the task fails or SQLite cannot open or persist the snapshot.
pub(crate) fn persist_thread_snapshot(uin: &str, snapshot: Thread) -> Result<usize> {
    store::persist_threads(
        &mut MessageStore::open_account(uin)?,
        std::slice::from_ref(&snapshot),
    )
}

/// Fetch older pages for `t` until the server has nothing older, `MAX_PAGES` is hit, or the user
/// cancels. Each page is committed immediately so a crash or cancel loses nothing. SQLite work
/// runs on the blocking pool; no connection is held across an await.
/// Returns how many messages were new to the database.
pub async fn sync_thread(s: &Session, t: &mut Thread) -> Result<usize> {
    sync_thread_with_writer(s, t, |uin, snapshot| async move {
        tokio::task::spawn_blocking(move || persist_thread_snapshot(&uin, snapshot)).await?
    })
    .await
}

/// Fetch history using a writer controlled by the caller's account lifecycle.
///
/// # Errors
/// Returns an error if the account is unavailable, fetching fails, or the writer cannot persist a page.
pub(crate) async fn sync_thread_with_writer<F>(
    s: &Session,
    t: &mut Thread,
    persist_page: impl Fn(String, Thread) -> F + Send,
) -> Result<usize>
where
    F: std::future::Future<Output = Result<usize>> + Send,
{
    let id = t.id.clone();
    let uin = s
        .login
        .as_ref()
        .map(|login| login.uin.clone())
        .filter(|uin| !uin.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Chưa đăng nhập"))?;

    let load_uin = uin.clone();
    let mut loaded_vec = vec![t.clone()];
    let (loaded_thread, oldest_db) = tokio::task::spawn_blocking(move || -> Result<_> {
        let store = MessageStore::open_account(&load_uin)?;
        store.hydrate_threads(&mut loaded_vec, 1000)?;
        let oldest = store.oldest_msg_id(&id);
        let loaded = loaded_vec.into_iter().next().unwrap_or_default();
        Ok((loaded, oldest))
    })
    .await??;
    *t = loaded_thread;

    let mut added = 0;
    let mut is_old = false;
    let mut cursor = oldest_db.unwrap_or_default();

    if t.messages.is_empty() {
        let resp = match chat::get_cloud_messages(s, &t.id, "0", false, PAGE).await {
            Ok(r) => r,
            Err(e) if chat::is_cloud_stop_retry(&e) => return Ok(0),
            Err(e) => return Err(e),
        };
        let before = t.messages.len();
        chat::merge_group_history(t, &resp);
        if t.messages.len() > before {
            added += persist_page(uin.clone(), t.clone()).await?;
        }
        let (has_more, next_is_old, resp_cursor) = chat::parse_cloud_page_meta(&resp);
        if !has_more {
            return Ok(added);
        }
        is_old = next_is_old;
        cursor = require_next_cursor(resp_cursor.or_else(|| next_cursor(t, "")), "")?;
        tokio::time::sleep(PAUSE).await;
    } else if cursor.is_empty() {
        cursor = next_cursor(t, "").unwrap_or_else(|| "0".to_string());
    }

    for _ in 0..MAX_PAGES {
        if CANCEL.load(Ordering::Relaxed) {
            break;
        }
        let resp = match chat::get_cloud_messages(s, &t.id, &cursor, is_old, PAGE).await {
            Ok(r) => r,
            Err(e) if chat::is_cloud_stop_retry(&e) => break,
            Err(e) => return Err(e),
        };
        let (has_more, next_is_old, resp_cursor) = chat::parse_cloud_page_meta(&resp);
        let before = t.messages.len();
        chat::merge_group_history(t, &resp);
        if t.messages.len() > before {
            added += persist_page(uin.clone(), t.clone()).await?;
        }
        if t.messages.len() == before && !is_old && next_is_old && has_more {
            // Server signaled cold-storage pagination (`isOld = 1`) for this cursor; retry page once via getoldv2.
            is_old = true;
            tokio::time::sleep(PAUSE).await;
            continue;
        }
        is_old = next_is_old;
        if !has_more {
            break;
        }
        cursor = require_next_cursor(resp_cursor.or_else(|| next_cursor(t, &cursor)), &cursor)?;
        tokio::time::sleep(PAUSE).await;
    }
    Ok(added)
}

fn sync_error_summary(error: &anyhow::Error) -> String {
    if let Some(error) = error.downcast_ref::<crate::session::SessionError>() {
        format!("Zalo trả mã lỗi {}", error.code)
    } else if let Some(error) = error.downcast_ref::<reqwest::Error>() {
        if error.is_timeout() {
            "hết thời gian chờ kết nối".into()
        } else {
            "lỗi kết nối đến Zalo".into()
        }
    } else {
        "lỗi đồng bộ; xem log --dev".into()
    }
}

/// The oldest server message id after a merge, or `None` when the page didn't move the cursor
/// back (the server returned only messages we already had); continuing would loop forever.
/// Local echoes (`out_*`) are not valid cursors, hence the digits-only filter.
fn next_cursor(t: &Thread, current: &str) -> Option<String> {
    let oldest = t
        .messages
        .iter()
        .map(|m| m.id.as_str())
        .find(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))?;
    (oldest != current).then(|| oldest.to_string())
}

fn require_next_cursor(candidate: Option<String>, current: &str) -> Result<String> {
    candidate
        .filter(|cursor| !cursor.is_empty() && cursor != current)
        .ok_or_else(|| {
            anyhow::anyhow!("server indicated more history but the cursor did not advance")
        })
}

static LATEST_MOBILE_TEMP_KEY: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Record a fresh `temp_key` received from a `transfer_after_login` control event.
pub fn set_latest_mobile_temp_key(temp_key: &str) {
    if let Ok(mut g) = LATEST_MOBILE_TEMP_KEY.lock() {
        *g = temp_key.trim().to_string();
    }
}

/// Return the most recent `temp_key` issued by the user's phone on login (if any).
pub fn latest_mobile_temp_key() -> String {
    LATEST_MOBILE_TEMP_KEY
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}
static LAST_MOBILE_SYNC_STATUS: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Record the latest status returned by the user's phone (`WaitingConfirmOnPhone`, `MobileBusyRestoring`, etc.).
pub fn set_last_mobile_sync_status(status: Option<String>) {
    if let Ok(mut g) = LAST_MOBILE_SYNC_STATUS.lock() {
        *g = status;
    }
}

static LAST_PHONE_SYNC_ERROR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn set_last_phone_sync_error(error: Option<String>) {
    if let Ok(mut g) = LAST_PHONE_SYNC_ERROR.lock() {
        *g = error;
    }
}

/// Why the phone (Sync V2) step of the last `sync_phone_messages` run could not start,
/// e.g. no Zalo PC WASM on this machine. `None` when it started or was not attempted.
pub fn last_phone_sync_error() -> Option<String> {
    LAST_PHONE_SYNC_ERROR.lock().ok().and_then(|g| g.clone())
}

/// Read the latest status returned by the user's phone during `sync_phone_messages`.
pub fn last_mobile_sync_status() -> Option<String> {
    LAST_MOBILE_SYNC_STATUS.lock().ok().and_then(|g| g.clone())
}


static CURRENT_SYNC_KEYS: std::sync::Mutex<Option<(rsa::RsaPrivateKey, String)>> =
    std::sync::Mutex::new(None);

/// Retrieve the active RSA 2048 keys used for mobile sync decryption without consuming them.
pub fn get_sync_keys() -> Option<(rsa::RsaPrivateKey, String)> {
    CURRENT_SYNC_KEYS.lock().ok().and_then(|g| g.clone())
}

/// Retrieve the active RSA 2048 public key.
pub fn get_sync_public_key() -> Option<String> {
    CURRENT_SYNC_KEYS
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|(_, pub_k)| pub_k.clone()))
}
/// Clear the active sync keys once sync is finalized.
pub fn clear_sync_keys() {
    if let Ok(mut g) = CURRENT_SYNC_KEYS.lock() {
        *g = None;
    }
}

/// Load (hoặc gen 1 lần + lưu `accounts/<uin>/sync_rsa_identity.json`) RSA-2048 key cố định của
/// tài khoản cho V1 `pullMobileMsg` (`cmd 12000`). Đúng Zalo PC: phone match
/// `pc_name`+`public_key` khi gửi `user_confirm` về — key mới mỗi lần khiến
/// phone thấy thiết bị lạ, bấm vào thông báo không mở được.
pub fn generate_sync_public_key_for(uin: &str) -> String {
    use base64::Engine;
    use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey};
    use rsa::{RsaPrivateKey, RsaPublicKey};

    let path = crate::account_manager::sync_rsa_path_for(uin);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(parent);
    }
    // 1. Thử đọc key đã lưu.
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(pem) = v.get("private_pem").and_then(|x| x.as_str()) {
                if let Ok(priv_key) = RsaPrivateKey::from_pkcs8_pem(pem) {
                    let pub_key = RsaPublicKey::from(&priv_key);
                    if let Ok(der) = pub_key.to_public_key_der() {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(der.as_bytes());
                        if let Ok(mut g) = CURRENT_SYNC_KEYS.lock() {
                            *g = Some((priv_key, b64.clone()));
                        }
                        crate::logger::log_sync("rsa: dùng key đã lưu");
                        return b64;
                    }
                }
            }
        }
    }
    // 2. Gen mới + lưu với quyền riêng tư.
    let mut rng = rsa::rand_core::OsRng;
    if let Ok(priv_key) = RsaPrivateKey::new(&mut rng, 2048) {
        let pub_key = RsaPublicKey::from(&priv_key);
        if let Ok(der) = pub_key.to_public_key_der() {
            let b64 = base64::engine::general_purpose::STANDARD.encode(der.as_bytes());
            if let Ok(pem) = priv_key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF) {
                let doc = serde_json::json!({ "private_pem": pem.as_str() });
                let text = serde_json::to_string_pretty(&doc).unwrap_or_default();
                let _ = crate::data_dir::write_private_atomic(&path, text.as_bytes());
            }
            if let Ok(mut g) = CURRENT_SYNC_KEYS.lock() {
                *g = Some((priv_key, b64.clone()));
            }
            crate::logger::log_sync("rsa: đã gen key mới + lưu");
            return b64;
        }
    }
    String::new()
}

/// Wrapper giữ tương thích chữ ký cũ, tự đọc theo `active_uin()`.
pub fn generate_sync_public_key() -> String {
    let uin = crate::account_manager::active_uin();
    generate_sync_public_key_for(&uin)
}
/// Synchronize 1-1 and group messages from the user's phone and Zalo cloud (`Đồng bộ tin nhắn từ điện thoại`, #68).
///
/// Workflow:
/// 1. Sends mobile pull signal (tries Sync V2 `cmd 590` first for iOS, and automatically falls back
///    to Sync V1 `/api/message/pull_mobile_msg` `cmd 12000` for Android if V2 receives no response).
/// 2. Drains `lp_chat` (`get_chat_from_polling`) across multiple batches to collect queued 1-1 & group messages.
/// 3. Queries `/api/preloadconvers/get-last-msgs` (`get_last_msgs`) in 25-thread batches across all
///    1-1 and group conversations and merges messages into `threads`.
/// 4. Fetches recent & older pages via `/api/cm/getrecentv2` and `/api/cm/getoldv2` for Cloud & groups.
/// 5. Persists all merged messages to per-account SQLite (`accounts/<uin>_history.db`) and cache.
///
/// Returns `(added_messages, synced_threads_count)`.
pub async fn sync_phone_messages<F>(
    s: &Session,
    poll_state: &tokio::sync::Mutex<chat::PollState>,
    threads: &mut Vec<Thread>,
    on_progress: F,
) -> Result<(usize, usize)>
where
    F: FnMut(f32, String),
{
    sync_phone_messages_opts(s, poll_state, threads, true, on_progress).await
}

/// Detailed result for the GUI so partial fetch failures are not reported as success.
#[derive(Debug, Default)]
pub(crate) struct PhoneSyncReport {
    pub(crate) added: usize,
    pub(crate) active_threads: usize,
    pub(crate) warnings: Vec<String>,
}

/// Synchronize messages with explicit control over whether to send a wake-up/pull prompt
/// (`send_phone_prompt`) to the user's phone or only drain server/cloud queues.
pub async fn sync_phone_messages_opts<F>(
    s: &Session,
    poll_state: &tokio::sync::Mutex<chat::PollState>,
    threads: &mut Vec<Thread>,
    send_phone_prompt: bool,
    on_progress: F,
) -> Result<(usize, usize)>
where
    F: FnMut(f32, String),
{
    let report =
        sync_phone_messages_with_report(s, poll_state, threads, send_phone_prompt, on_progress)
            .await?;
    Ok((report.added, report.active_threads))
}

/// Synchronize messages and retain non-fatal endpoint failures for the completion event.
///
/// # Errors
/// Returns an error when there is no active login or required local persistence fails.
pub(crate) async fn sync_phone_messages_with_report<F>(
    s: &Session,
    poll_state: &tokio::sync::Mutex<chat::PollState>,
    threads: &mut Vec<Thread>,
    send_phone_prompt: bool,
    mut on_progress: F,
) -> Result<PhoneSyncReport>
where
    F: FnMut(f32, String),
{
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let uin = s
        .login
        .as_ref()
        .map(|l| l.uin.as_str())
        .unwrap_or_default()
        .to_string();
    crate::account_manager::validate_uin(&uin)?;
    let pub_key = generate_sync_public_key_for(&uin);
    let before_total: usize = threads.iter().map(|t| t.messages.len()).sum();
    let mut warnings = Vec::new();

    if send_phone_prompt {
        set_last_mobile_sync_status(None);
        set_last_phone_sync_error(None);
        reset_sync_v2_imported_count();

        // Phase 1 — Dual Sync (iOS V2 -> Android V1 fallback):
        // iOS dùng Sync V2 (`cmd 590` `transfer_sync2`) và phản hồi rất nhanh (~0.5–1.5s).
        // Android APK không hỗ trợ `transfer_sync2`, chỉ hỗ trợ Sync V1 (`cmd 12000`
        // `pull_mobile_msg` -> `transfer_msg`/`syncmsgmb` -> `.db.crypt`).
        // Vì vậy: gửi `cmd 590` trước KHÔNG kèm V1 (tránh 2 phiên đè nhau trên iOS), thăm dò
        // 5.5s; nếu im lặng hoàn toàn thì dọn phiên V2 (`cmd 591` OutDated) rồi bắn V1!
        on_progress(
            0.10,
            "📲 Đang gửi yêu cầu đồng bộ tới Zalo trên điện thoại (hãy mở Zalo trên điện thoại và bấm 'Đồng bộ ngay')…".to_string(),
        );
        let partitions = chat::resolve_thread_partitions(s, threads).await;
        crate::logger::log_sync(&format!("Tìm được {} partitions cho sync", partitions.len()));

        let mut v2_sync_id = match chat::wake_phone_sync_v2(s).await {
            Ok(sync_id) => match chat::send_sync_v2_request_with_queries(s, &sync_id, &partitions) {
                Ok(true) => Some(sync_id),
                Ok(false) => {
                    set_active_v2_sync_id(None);
                    crate::logger::log_sync("V2 cmd 590 thất bại (WebSocket chưa kết nối)");
                    None
                }
                Err(e) => {
                    set_active_v2_sync_id(None);
                    crate::logger::log_sync(&format!("V2 tạo gói cmd 590 FAIL: {e}"));
                    None
                }
            },
            Err(e) => {
                set_active_v2_sync_id(None);
                crate::logger::log_sync(&format!("V2 mở session FAIL: {e}"));
                None
            }
        };

        let mut v2_got_response = false;
        let probe_start = std::time::Instant::now();
        if v2_sync_id.is_some() {
            for _ in 0..11 {
                if PHONE_SYNC_CANCEL.load(Ordering::Relaxed) {
                    if let Some(sid) = v2_sync_id.as_deref() {
                        crate::socket::send_sync_ack_delete(
                            sid,
                            crate::socket::SYNC_DISPOSE_USER_CANCEL,
                        );
                        remove_sync_v2_session(sid);
                    }
                    set_active_v2_sync_id(None);
                    return Ok(PhoneSyncReport::default());
                }
                if last_mobile_sync_status().is_some() {
                    v2_got_response = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }

        let mut v1_active = false;
        if !v2_got_response {
            if let Some(sid) = v2_sync_id.take() {
                crate::socket::send_sync_ack_delete(&sid, crate::socket::SYNC_DISPOSE_OUTDATED);
                remove_sync_v2_session(&sid);
            }
            set_active_v2_sync_id(None);
            set_last_phone_sync_error(None);
            set_last_mobile_sync_status(None);
            crate::logger::log_sync(
                "V2 không phản hồi sau 5.5s (Android / V1) -> chuyển sang pull_mobile_msg (cmd 12000)",
            );
            if !pub_key.is_empty() {
                match chat::request_phone_sync(s, &pub_key).await {
                    Ok(resp) => {
                        let ec = resp
                            .get("error_code")
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or(0);
                        crate::logger::log_sync(&format!(
                            "V1 pull_mobile_msg response: error_code={ec}"
                        ));
                        if ec == 0 {
                            v1_active = true;
                            on_progress(
                                0.15,
                                "📲 Đã gửi yêu cầu đồng bộ tới Zalo trên điện thoại — hãy mở Zalo trên điện thoại và bấm 'Đồng bộ'…".to_string(),
                            );
                        } else {
                            let em = resp
                                .get("error_message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("máy chủ từ chối yêu cầu đồng bộ V1");
                            let err = format!("⚠️ Yêu cầu đồng bộ V1 thất bại (code {ec}): {em}");
                            set_last_phone_sync_error(Some(err.clone()));
                            on_progress(0.15, err);
                        }
                    }
                    Err(e) => {
                        crate::logger::log_sync(&format!("V1 pull_mobile_msg FAIL: {e}"));
                        let err = format!("⚠️ Không gửi được yêu cầu đồng bộ tới điện thoại: {e}");
                        set_last_phone_sync_error(Some(err.clone()));
                        on_progress(0.15, err);
                    }
                }
            } else {
                let err = "⚠️ Không tạo được khoá RSA đồng bộ cho điện thoại".to_string();
                set_last_phone_sync_error(Some(err.clone()));
                on_progress(0.15, err);
            }
        }

        // Chờ user xác nhận trên phone (tối đa 90s kể cả lúc probe). Không gửi lại gì cả.
        let confirm_window = if v2_sync_id.is_some() {
            Duration::from_secs(90).saturating_sub(probe_start.elapsed())
        } else if v1_active {
            Duration::from_secs(90)
        } else {
            Duration::ZERO
        };
        let deadline = std::time::Instant::now() + confirm_window;
        let mut confirmed = false;
        let mut rejected = false;
        let mut had_error = false;
        while std::time::Instant::now() < deadline {
            if PHONE_SYNC_CANCEL.load(Ordering::Relaxed) {
                if let Some(sid) = v2_sync_id.as_deref() {
                    crate::socket::send_sync_ack_delete(
                        sid,
                        crate::socket::SYNC_DISPOSE_USER_CANCEL,
                    );
                    remove_sync_v2_session(sid);
                }
                if v1_active {
                    let _ = chat::cancel_phone_sync(s, &pub_key).await;
                }
                set_active_v2_sync_id(None);
                return Ok(PhoneSyncReport::default());
            }
            if let Some(st) = last_mobile_sync_status().as_deref() {
                let is_error = st.contains("Lỗi") || st.contains("Không thể") || st.contains("thất bại");
                if is_error {
                    had_error = true;
                    set_last_phone_sync_error(Some(st.to_string()));
                    on_progress(1.0, st.to_string());
                    break;
                } else if st.contains("từ chối") {
                    rejected = true;
                    set_last_phone_sync_error(Some(st.to_string()));
                    on_progress(1.0, st.to_string());
                    break;
                } else if st.contains("đã xác nhận")
                    || st.contains("đang nén")
                    || st.contains("giải mã")
                    || st.contains("Đang tải và giải mã")
                    || st.contains("bỏ qua 1 phân vùng")
                    || st.contains("Đã nhận gói đồng bộ")
                    || st.contains("thành công")
                {
                    confirmed = true;
                    on_progress(0.40, st.to_string());
                    break;
                } else {
                    on_progress(0.20, st.to_string());
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        if rejected {
            if let Some(sid) = v2_sync_id.as_deref() {
                crate::socket::send_sync_ack_delete(sid, crate::socket::SYNC_DISPOSE_USER_CANCEL);
                remove_sync_v2_session(sid);
            }
            if v1_active {
                let _ = chat::cancel_phone_sync(s, &pub_key).await;
            }
            set_active_v2_sync_id(None);
            return Ok(PhoneSyncReport::default());
        }

        if confirmed {
            // Đã xác nhận → chỉ chờ batch / BackupReady, không gửi lại request.
            on_progress(0.50, "📲 Đã xác nhận — chờ điện thoại nén & gửi gói tin…".to_string());
            let finish_deadline = std::time::Instant::now() + Duration::from_secs(180);
            let mut ok = false;
            while std::time::Instant::now() < finish_deadline {
                if PHONE_SYNC_CANCEL.load(Ordering::Relaxed) {
                    break;
                }
                let v2_done = v2_sync_id.is_some()
                    && is_sync_v2_last_seen()
                    && sync_v2_inflight_count() == 0;
                if v2_done {
                    let done_msg = last_mobile_sync_status()
                        .filter(|s| s.contains("thành công"))
                        .unwrap_or_else(|| {
                            format!(
                                "🎉 Đã giải mã & đồng bộ thành công {} tin nhắn từ điện thoại vào ứng dụng!",
                                sync_v2_imported_count()
                            )
                        });
                    ok = true;
                    on_progress(0.95, done_msg);
                    tokio::time::sleep(Duration::from_millis(1200)).await;
                    let _ = sync_v2_batch_lock().lock().await;
                    break;
                }
                if let Some(st) = last_mobile_sync_status().as_deref() {
                    if st.contains("thành công") {
                        ok = true;
                        on_progress(0.95, st.to_string());
                        if v2_sync_id.is_some() {
                            // Đợi nhịp ngắn để các batch upload song song (nếu tới lệch thứ tự
                            // sau gói `isLast=1`) kịp tải & ghi vào SQLite trước khi gửi `cmd 591`.
                            tokio::time::sleep(Duration::from_millis(1200)).await;
                            let _ = sync_v2_batch_lock().lock().await;
                        }
                        break;
                    } else if st.contains("Lỗi") || st.contains("Không thể") || st.contains("thất bại") {
                        set_last_phone_sync_error(Some(st.to_string()));
                        on_progress(1.0, st.to_string());
                        break;
                    } else {
                        on_progress(0.70, st.to_string());
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            let cancelled = PHONE_SYNC_CANCEL.load(Ordering::Relaxed);
            if !ok && !cancelled && last_phone_sync_error().is_none() {
                set_last_phone_sync_error(Some(
                    "⚠️ Hết thời gian chờ gói tin nhắn từ điện thoại (hãy giữ Zalo trên điện thoại mở)".to_string(),
                ));
            }
            // Xong (hoặc hết chờ) → 591 dọn session V2 hoặc clean/cancel V1.
            if let Some(sid) = v2_sync_id.as_deref() {
                let reason = if ok {
                    crate::socket::SYNC_DISPOSE_SUCCESS
                } else if cancelled {
                    crate::socket::SYNC_DISPOSE_USER_CANCEL
                } else {
                    crate::socket::SYNC_DISPOSE_ERROR
                };
                crate::socket::send_sync_ack_delete(sid, reason);
                remove_sync_v2_session(sid);
            }
            set_active_v2_sync_id(None);
            if cancelled && v1_active {
                let _ = chat::cancel_phone_sync(s, &pub_key).await;
            } else if let Some(pub_k) = get_sync_public_key() {
                let _ = crate::chat::clean_sync(s, &pub_k).await;
            }
            clear_sync_keys();
        } else if v2_sync_id.is_some() || v1_active {
            if !had_error && last_phone_sync_error().is_none() {
                set_last_phone_sync_error(Some(
                    "⚠️ Hết thời gian chờ xác nhận đồng bộ trên điện thoại".to_string(),
                ));
            }
            if let Some(sid) = v2_sync_id.as_deref() {
                crate::socket::send_sync_ack_delete(sid, crate::socket::SYNC_DISPOSE_ERROR);
                remove_sync_v2_session(sid);
            }
            if v1_active {
                let _ = chat::cancel_phone_sync(s, &pub_key).await;
            }
            set_active_v2_sync_id(None);
        }
    }
    // Phase 1b: Synchronize pinned conversations from server (/api/pinconvers/list, #87)
    on_progress(
        0.18,
        "📌 Đang đồng bộ danh sách hội thoại đã ghim từ máy chủ Zalo…".to_string(),
    );
    if let Ok(pinned_val) = chat::get_pinned(s).await {
        let pinned_ids = chat::parse_pinned_conversations(&pinned_val);
        chat::apply_pinned_threads(threads, &pinned_ids);
    }

    if PHONE_SYNC_CANCEL.load(Ordering::Relaxed) {
        let _ = chat::cancel_phone_sync(s, &pub_key).await;
        return Ok(PhoneSyncReport::default());
    }

    // Phase 2: Drain `lp_chat` multi-batch action queues (`getChatFromPolling`)
    on_progress(
        0.22,
        "📥 Đang nhận hàng đợi tin nhắn 1-1 & nhóm từ điện thoại…".to_string(),
    );
    for batch_idx in 0..5 {
        if PHONE_SYNC_CANCEL.load(Ordering::Relaxed) {
            let _ = chat::cancel_phone_sync(s, &pub_key).await;
            return Ok(PhoneSyncReport {
                warnings,
                ..PhoneSyncReport::default()
            });
        }
        let lp_res = {
            let mut ps = poll_state.lock().await;
            chat::get_chat_from_polling(s, &mut ps).await
        };
        let lp_resp = match lp_res {
            Ok(response) => response,
            Err(error) => {
                warnings.push(format!("Hàng đợi tin nhắn: {}", sync_error_summary(&error)));
                break;
            }
        };
        chat::merge_previews(threads, &lp_resp);
        if let Some(data_node) = lp_resp.get("data") {
            crate::unread::merge_read_points(threads, data_node);
        }
        let data_node = lp_resp.get("data").unwrap_or(&lp_resp);
        let more = data_node
            .get("more")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let has_reset = data_node
            .get("resetLastActionId")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|r| !r.is_empty() && r != "0");
        on_progress(
            0.22 + (batch_idx as f32 + 1.0) * 0.04,
            format!("📥 Đã nhận lô hàng đợi #{} từ điện thoại…", batch_idx + 1),
        );
        if more == 0 && !has_reset {
            break;
        }
    }

    // Phase 3: Batch-query `/api/preloadconvers/get-last-msgs` across all 1-1 and group conversations
    on_progress(
        0.45,
        "🔄 Đang đồng bộ tin nhắn các cuộc trò chuyện 1-1 & nhóm…".to_string(),
    );
    match chat::get_last_msgs(s, &[]).await {
        Ok(response) => chat::merge_previews(threads, &response),
        Err(error) => warnings.push(format!(
            "Tải danh sách hội thoại: {}",
            sync_error_summary(&error)
        )),
    }
    let known_ids: Vec<String> = threads
        .iter()
        .filter(|t| !t.id.is_empty())
        .map(|t| {
            if t.is_group && !t.id.starts_with('g') && !t.id.starts_with("group_") {
                format!("g{}", t.id)
            } else {
                t.id.clone()
            }
        })
        .collect();
    let chunks: Vec<&[String]> = known_ids.chunks(25).collect();
    let total_chunks = chunks.len().max(1);
    for (idx, chunk) in chunks.into_iter().enumerate() {
        if PHONE_SYNC_CANCEL.load(Ordering::Relaxed) {
            let _ = chat::cancel_phone_sync(s, &pub_key).await;
            break;
        }
        match chat::get_last_msgs(s, chunk).await {
            Ok(response) => chat::merge_previews(threads, &response),
            Err(error) => warnings.push(format!(
                "Tải tin nhắn xem trước lô {}: {}",
                idx + 1,
                sync_error_summary(&error)
            )),
        }
        let frac = 0.45 + 0.25 * ((idx + 1) as f32 / total_chunks as f32);
        on_progress(
            frac,
            format!(
                "🔄 Đang đồng bộ hội thoại 1-1 & nhóm ({}/{total_chunks} nhóm lô)…",
                idx + 1
            ),
        );
        tokio::time::sleep(Duration::from_millis(120)).await;
    }

    // Phase 4: Sync Cloud & group conversations via `/api/cm/getrecentv2` + `/api/cm/getoldv2`
    let send2me_id = s
        .login
        .as_ref()
        .map(|l| l.send2me_id.clone())
        .unwrap_or_default();
    let cm_targets: Vec<usize> = threads
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            !t.id.is_empty()
                && (t.is_group
                    || t.name.contains("Cloud của tôi")
                    || (!send2me_id.is_empty() && t.id == send2me_id))
        })
        .map(|(i, _)| i)
        .collect();
    let total_cm = cm_targets.len().max(1);
    for (pos, tidx) in cm_targets.into_iter().enumerate() {
        if PHONE_SYNC_CANCEL.load(Ordering::Relaxed) {
            let _ = chat::cancel_phone_sync(s, &pub_key).await;
            break;
        }
        let (tid, tname) = {
            let t = &threads[tidx];
            (t.id.clone(), t.name.clone())
        };
        let frac = 0.70 + 0.25 * ((pos + 1) as f32 / total_cm as f32);
        on_progress(
            frac,
            format!("☁️ Đang đồng bộ lịch sử: {tname} ({}/{total_cm})…", pos + 1),
        );
        match chat::get_cloud_messages(s, &tid, "0", false, PAGE).await {
            Ok(response) => {
                chat::merge_group_history(&mut threads[tidx], &response);
                let (has_more, is_old, last_msg_id) = chat::parse_cloud_page_meta(&response);
                if has_more {
                    if let Some(oldest) = last_msg_id.or_else(|| next_cursor(&threads[tidx], "")) {
                        match chat::get_cloud_messages(s, &tid, &oldest, is_old, PAGE).await {
                            Ok(old_response) => {
                                chat::merge_group_history(&mut threads[tidx], &old_response);
                            }
                            Err(error) if chat::is_cloud_stop_retry(&error) => {}
                            Err(error) => warnings.push(format!(
                                "Tải trang lịch sử Cloud/Nhóm: {}",
                                sync_error_summary(&error)
                            )),
                        }
                    } else {
                        warnings.push(
                            "Cloud/Nhóm: máy chủ báo còn lịch sử nhưng không có mốc phân trang"
                                .into(),
                        );
                    }
                }
            }
            Err(error) if chat::is_cloud_stop_retry(&error) => {}
            Err(error) => warnings.push(format!(
                "Tải lịch sử Cloud/Nhóm: {}",
                sync_error_summary(&error)
            )),
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    // Phase 5: Resolve contact/group display names, deduplicate, and persist to per-account SQLite & cache
    let uin_str = s.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
    let db_path = store::db_path_for_account(&uin_str);
    on_progress(
        0.96,
        format!("💾 Đang lưu tin nhắn đã đồng bộ vào cơ sở dữ liệu {db_path}…"),
    );
    chat::resolve_thread_names(s, threads).await;
    chat::deduplicate_threads(threads, None);

    let mut snapshot = threads.clone();
    let sync_uin = uin.to_string();
    let (db_added, hydrated_threads) = tokio::task::spawn_blocking(move || -> Result<(usize, Vec<Thread>)> {
        let mut st = MessageStore::open_account(&sync_uin)?;
        let n = store::persist_threads(&mut st, &snapshot)?;
        // Hydrate ngược từ SQLite để nạp toàn bộ tin nhắn 1-1 & nhóm vừa giải mã
        // Hydrate messages imported from both sync paths before writing the account cache.
        st.hydrate_threads(&mut snapshot, 100)?;
        chat::save_threads_cache_for_account(&sync_uin, &snapshot);
        Ok((n, snapshot))
    })
    .await??;
    *threads = hydrated_threads;
    chat::resolve_thread_names(s, threads).await;
    chat::deduplicate_threads(threads, None);
    chat::save_threads_cache_for_account(&uin, threads);
    let after_total: usize = threads.iter().map(|t| t.messages.len()).sum();
    let mobile_added = sync_v2_imported_count();
    let added = (db_added + mobile_added).max(after_total.saturating_sub(before_total));
    let active_threads = threads.iter().filter(|t| !t.messages.is_empty()).count();
    let final_msg = if !warnings.is_empty() {
        format!(
            "⚠️ Đồng bộ chưa đầy đủ: +{added} tin nhắn ({active_threads} hội thoại), {} lỗi tải dữ liệu (vd. {})",
            warnings.len(),
            warnings[0]
        )
    } else if let Some(phone_note) = last_mobile_sync_status() {
        format!(
            "{phone_note} · Đã đồng bộ Cloud/Nhóm: +{added} tin nhắn ({active_threads} hội thoại)"
        )
    } else {
        format!("✅ Đã gửi yêu cầu tới điện thoại & đồng bộ Cloud/Nhóm: +{added} tin nhắn ({active_threads} hội thoại). Hãy mở Zalo trên điện thoại để xác nhận nếu muốn kéo tin nhắn 1-1 cũ.")
    };
    on_progress(1.0, final_msg);
    Ok(PhoneSyncReport {
        added,
        active_threads,
        warnings,
    })
}

// The existing znoise response maps numeric backup user IDs to web IDs. A decimal web
// UIN alone is not evidence of the Android SenderId namespace (re-notes/39_phone_sync_safety.md).
/// Resolve the account's numeric Android sender ID from direct-chat backup database names.
///
/// # Errors
/// Returns an error if there is no matching positive ID or the backup maps multiple IDs to the account.
fn android_self_sender_id(
    uin: &str,
    id_map: &std::collections::HashMap<String, String>,
) -> Result<i64> {
    let mut sender_id = None;
    for (name, web_id) in id_map {
        if uin.is_empty() || web_id != uin || name.starts_with("group_") {
            continue;
        }
        let Some(id) = name
            .strip_suffix(".db")
            .and_then(|stem| stem.parse::<i64>().ok())
            .filter(|id| *id > 0)
        else {
            continue;
        };
        if sender_id.is_some_and(|previous| previous != id) {
            crate::logger::log_error(
                "history_sync::android_identity",
                "Backup maps multiple Android sender IDs to the captured account",
            );
            anyhow::bail!(
                "Không thể nhập bản sao Android: ánh xạ định danh chủ tài khoản không rõ ràng."
            );
        }
        sender_id = Some(id);
    }
    sender_id.ok_or_else(|| {
        crate::logger::log_error("history_sync::android_identity", "Backup has no trusted Android sender identity for the captured account");
        anyhow::anyhow!("Không thể nhập bản sao Android: chưa xác định được định danh người gửi của tài khoản từ ánh xạ bản sao. Vui lòng dùng đồng bộ Cloud/Nhóm.")
    })
}

/// Download, decrypt and ingest a mobile backup for the captured account.
///
/// # Errors
/// Rejects backups without an unambiguous numeric-to-web mapping for the account owner.
pub async fn import_mobile_backup_container(
    s: &Session,
    url: &str,
    encrypted_key: &str,
) -> Result<usize> {
    use crate::store::StoredMessage;
    use aes::cipher::{BlockDecryptMut, KeyIvInit};
    use base64::Engine;
    use rsa::Pkcs1v15Encrypt;
    use rusqlite::Connection;
    use serde_json::json;
    use std::collections::{HashMap, HashSet};
    use std::io::Cursor;

    let uin = s
        .login
        .as_ref()
        .map(|login| login.uin.clone())
        .filter(|uin| !uin.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Chưa đăng nhập"))?;
    crate::account_manager::validate_uin(&uin)?;
    if url.is_empty() {
        anyhow::bail!("empty backup url");
    }

    let (priv_key, pub_key_b64) = get_sync_keys()
        .ok_or_else(|| anyhow::anyhow!("Không tìm thấy khoá RSA phiên đồng bộ hiện tại"))?;

    let raw_key = if !encrypted_key.is_empty() {
        let enc_bytes = base64::engine::general_purpose::STANDARD.decode(encrypted_key)?;
        priv_key.decrypt(Pkcs1v15Encrypt, &enc_bytes)?
    } else {
        anyhow::bail!("encrypted_key is empty");
    };

    let sek = s.zpw_sek.clone().unwrap_or_default();
    let cookie_hdr = format!("zpw_sek={sek}; zpw_uin={uin}");

    let resp = s
        .http
        .get(url)
        .header("Cookie", cookie_hdr)
        .header("User-Agent", "ZaloPC-win32-26.10.11")
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("failed to download backup: HTTP {}", resp.status());
    }

    let crypt_data = resp.bytes().await?;
    if crypt_data.len() < 48 {
        anyhow::bail!("backup file too small: {} bytes", crypt_data.len());
    }

    let hex_key: String = raw_key.iter().map(|b| format!("{:02X}", b)).collect();
    let key_32 = &hex_key.as_bytes()[..32];
    type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

    let chunk_size = 65536;
    let iv_zero = [0u8; 16];
    let mut decrypted = Vec::with_capacity(crypt_data.len());

    for chunk in crypt_data.chunks(chunk_size) {
        let rem = chunk.len() % 16;
        let aligned_len = chunk.len() - rem;
        let mut buf = chunk[..aligned_len].to_vec();

        let dec = Aes256CbcDec::new(key_32.into(), &iv_zero.into());
        dec.decrypt_padded_mut::<cipher::block_padding::NoPadding>(&mut buf)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        decrypted.extend_from_slice(&buf);
        if rem > 0 {
            decrypted.extend_from_slice(&chunk[aligned_len..]);
        }
    }

    let magic = decrypted.get(..7).unwrap_or_default();
    if magic != b"ZDB4.0\0" {
        anyhow::bail!("invalid backup container magic: {:?}", magic);
    }

    let mut offset = 14;
    if offset + 4 > decrypted.len() {
        anyhow::bail!("truncated container header");
    }
    let n_files = u32::from_be_bytes(decrypted[offset..offset + 4].try_into()?) as usize;
    offset += 4;

    let mut toc = Vec::new();
    let mut fids = Vec::new();
    let mut gids = Vec::new();

    for _ in 0..n_files {
        if offset + 4 > decrypted.len() {
            break;
        }
        let name_len = u32::from_be_bytes(decrypted[offset..offset + 4].try_into()?) as usize;
        offset += 4;
        if offset + name_len > decrypted.len() {
            break;
        }
        let raw_name = String::from_utf8_lossy(&decrypted[offset..offset + name_len]).to_string();
        offset += name_len;
        if offset + 4 > decrypted.len() {
            break;
        }
        let size = u32::from_be_bytes(decrypted[offset..offset + 4].try_into()?) as usize;
        offset += 4;

        let safe_name = std::path::Path::new(&raw_name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if safe_name.is_empty() {
            toc.push((String::new(), size));
            continue;
        }
        let stem = safe_name.trim_end_matches(".db");
        if stem.starts_with("group_") {
            if let Ok(gid) = stem[6..].parse::<i64>() {
                gids.push(gid);
            }
        } else if let Ok(fid) = stem.parse::<i64>() {
            fids.push(fid);
        }

        toc.push((safe_name, size));
    }
    let toc_end = offset;

    if toc_end >= decrypted.len() {
        anyhow::bail!("corrupted container: toc extends beyond payload");
    }
    let xz_slice = &decrypted[toc_end..];
    let end_pos = xz_slice
        .windows(2)
        .rposition(|w| w == b"YZ")
        .ok_or_else(|| anyhow::anyhow!("XZ footer not found"))?
        + 2;

    let mut cursor = Cursor::new(&xz_slice[..end_pos]);
    let mut payload = Vec::new();
    lzma_rs::xz_decompress(&mut cursor, &mut payload)?;

    // Discover senders present in group databases as well as direct-chat filenames.
    // The upstream sync path resolves these IDs through the same znoise endpoint.
    let scan_payload = payload.clone();
    let scan_toc = toc.clone();
    let mut all_fids = fids.clone();
    let discovered = tokio::task::spawn_blocking(move || -> Result<HashSet<i64>> {
        let temp_dir = std::env::temp_dir().join(format!("ctdozlite_sync_scan_{}", crate::param_cipher::random_hex(8)));
        std::fs::create_dir_all(&temp_dir)?;
        let result = (|| -> Result<HashSet<i64>> {
            let mut ids = HashSet::new();
            let mut offset = 0usize;
            for (index, (_, size)) in scan_toc.iter().enumerate() {
                let Some(end) = offset.checked_add(*size).filter(|end| *end <= scan_payload.len()) else { break; };
                let path = temp_dir.join(format!("{index}.db"));
                std::fs::write(&path, &scan_payload[offset..end])?;
                offset = end;
                if let Ok(conn) = Connection::open(&path) {
                    if let Ok(mut stmt) = conn.prepare("SELECT DISTINCT SenderId FROM ChatContent WHERE SenderId > 0") {
                        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, i64>(0)) {
                            ids.extend(rows.flatten().filter(|id| *id > 0));
                        }
                    }
                }
            }
            Ok(ids)
        })();
        let _ = std::fs::remove_dir_all(&temp_dir);
        result
    }).await??;
    let mut seen: HashSet<i64> = all_fids.iter().copied().collect();
    all_fids.extend(discovered.into_iter().filter(|id| seen.insert(*id)));
    let mut id_map: HashMap<String, String> = HashMap::new();
    let mut fid_to_uid: HashMap<i64, String> = HashMap::new();
    let fid_chunks: Vec<_> = all_fids.chunks(200).collect();
    let gid_chunks: Vec<_> = gids.chunks(200).collect();
    for index in 0..fid_chunks.len().max(gid_chunks.len()) {
        let f_slice = fid_chunks.get(index).copied().unwrap_or(&[]);
        let g_slice = gid_chunks.get(index).copied().unwrap_or(&[]);
        let enc_params = s.encoded_params(&json!({"fids": f_slice, "gids": g_slice}))?;
        let znoise_url = format!("{}/api/znoise?{}", crate::endpoints::ZNOISE, s.common_params());
        let resp = s.post_form(&znoise_url, format!("params={enc_params}")).await?;
        let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
        if let Some(data) = dec.get("data") {
            if let Some(arr) = data.get("fids").and_then(|v| v.as_array()) {
                for (fid, value) in f_slice.iter().zip(arr) {
                    if let Some(uid) = value.as_str().map(str::trim).filter(|uid| !uid.is_empty()) {
                        fid_to_uid.insert(*fid, uid.to_string());
                        id_map.insert(format!("{fid}.db"), uid.to_string());
                    }
                }
            }
            if let Some(arr) = data.get("gids").and_then(|v| v.as_array()) {
                for (gid, value) in g_slice.iter().zip(arr) {
                    if let Some(uid) = value.as_str().map(str::trim).filter(|uid| !uid.is_empty()) {
                        id_map.insert(format!("group_{gid}.db"), uid.to_string());
                    }
                }
            }
        }
    }

    let my_uin = android_self_sender_id(&uin, &id_map)?;
    let my_uid = uin.clone();
    let my_name = s
        .login
        .as_ref()
        .map(|l| l.display_name.clone())
        .unwrap_or_else(|| "Tôi".into());
    let total_imported = tokio::task::spawn_blocking(move || -> Result<usize> {
        let mut store = crate::store::MessageStore::open_account(&uin)?;
        let temp_dir = std::env::temp_dir().join(format!("ctdozlite_sync_{}", crate::param_cipher::random_hex(8)));
        let _ = std::fs::create_dir_all(&temp_dir);

        let mut curr_offset = 0;
        let mut total_imported = 0;

        for (name, size) in toc {
            if curr_offset + size > payload.len() {
                break;
            }
            let db_bytes = &payload[curr_offset..curr_offset + size];
            curr_offset += size;

            let Some(thread_id) = id_map.get(&name) else {
                continue;
            };

            let temp_db_path = temp_dir.join(&name);
            if std::fs::write(&temp_db_path, db_bytes).is_err() {
                continue;
            }

            let Ok(conn) = Connection::open(&temp_db_path) else {
                continue;
            };

            let Ok(mut stmt) = conn.prepare(
                "SELECT SenderId, CliMsgId, GlbMsgId, MsgStatus, MsgType, TimeStamp, MsgContent, BinNet
                 FROM ChatContent ORDER BY TimeStamp ASC",
            ) else {
                continue;
            };

            let Ok(rows) = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<Vec<u8>>>(7)?,
                ))
            }) else {
                continue;
            };

            for r in rows.flatten() {
                let (sender_id_num, cli_msg_id, glb_msg_id, _status, msg_type_num, ts, content_opt, bin_net_opt) = r;
                let is_outgoing = sender_id_num == my_uin;
                let sender_id = if is_outgoing {
                    my_uid.clone()
                } else if let Some(uid) = fid_to_uid.get(&sender_id_num) {
                    uid.clone()
                } else if name.starts_with("group_") {
                    sender_id_num.to_string()
                } else {
                    thread_id.clone()
                };
                let sender_name = if is_outgoing { my_name.clone() } else { String::new() };

                let msg_id = if glb_msg_id > 0 {
                    glb_msg_id.to_string()
                } else {
                    cli_msg_id.to_string()
                };

                let mut text = content_opt.unwrap_or_default().trim().to_string();
                let mut msg_type = "webchat".to_string();

                if text.is_empty() {
                    if let Some(bn) = &bin_net_opt {
                        let s_latin = String::from_utf8_lossy(bn);
                        if let Some(pos) = s_latin.find("maps.app.goo.gl") {
                            msg_type = "chat.link".to_string();
                            let sub = &s_latin[pos..];
                            let end_url = sub.find(['"', '}', '\\', ' ']).unwrap_or(sub.len());
                            let url = format!("https://{}", &sub[..end_url]);
                            text = format!("Tôi đang trên đường tới. Bạn có thể xem tiến độ đi đường và thời gian đến của tôi trên Maps: {url}");
                        } else if msg_type_num == 10 {
                            msg_type = "chat.sticker".to_string();
                            text = "[Nhãn dán]".to_string();
                        } else if msg_type_num == 3 {
                            msg_type = "chat.photo".to_string();
                            text = "[Hình ảnh]".to_string();
                        } else if msg_type_num == 12 {
                            msg_type = "chat.link".to_string();
                            text = "[Liên kết]".to_string();
                        }
                    }
                }

                if text.is_empty() {
                    if msg_type_num == 10 {
                        text = "[Nhãn dán]".to_string();
                    } else if msg_type_num == 3 {
                        text = "[Hình ảnh]".to_string();
                    } else if msg_type_num == 12 {
                        text = "[Liên kết]".to_string();
                    }
                }

                let sm = StoredMessage {
                    msg_id,
                    thread_id: thread_id.clone(),
                    sender_id,
                    sender_name,
                    text,
                    msg_type,
                    ts,
                    is_outgoing,
                    raw_json: String::new(),
                };

                if store.upsert_message(&sm).unwrap_or(false) {
                    total_imported += 1;
                }
            }
        }

        let _ = std::fs::remove_dir_all(&temp_dir);

        let mut cached_threads = crate::chat::load_threads_cache_for_account(&uin);
        let _ = store.hydrate_threads(&mut cached_threads, 50);
        crate::chat::save_threads_cache_for_account(&uin, &cached_threads);
        Ok(total_imported)
    }).await??;

    // Notify Zalo server and phone that snapshot is downloaded, dismissing mobile spinner
    let _ = crate::chat::clean_sync(s, &pub_key_b64).await;
    clear_sync_keys();

    Ok(total_imported)
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct SyncV2Keys {
    pub ik_pub: String,
    pub ik_priv: String,
    pub ek_pub: String,
    pub ek_priv: String,
}

#[derive(Debug, Clone)]
struct SyncV2KeysRaw {
    ik_pub: Vec<u8>,
    ik_priv: Vec<u8>,
    ek_pub: Vec<u8>,
    ek_priv: Vec<u8>,
}

/// Khóa Sync V2 ed25519 của phiên hiện tại (raw bytes, giữ trong bộ nhớ).
static CURRENT_V2_KEYS: std::sync::Mutex<Option<SyncV2KeysRaw>> = std::sync::Mutex::new(None);

pub fn generate_sync_v2_keys_for(uin: &str) -> Result<SyncV2Keys> {
    // wasmtime host (không cần Node.js). scripts/sync_v2_helper.js chỉ còn
    // là tài liệu tham khảo cho luồng JS gốc (deprecated path).
    // QUAN TRỌNG (đúng Zalo PC `genSyncKeyPair`): ik là IDENTITY key cố định
    // của tài khoản (lưu `accounts/<uin>/sync_identity.json`, gen 1 lần duy nhất),
    // chỉ ek là ephemeral (gen mới mỗi lần sync). Gen ik mới mỗi lần khiến điện
    // thoại thấy key lạ → văng app khi bấm vào thông báo đồng bộ.
    let host = crate::zproto_host::ZprotoHost::load()?;
    let (ik_pub, ik_priv) = load_or_create_identity_key(&host, uin)?;
    let (ek_pub, ek_priv) = host.generate_keypair()?;
    use base64::Engine;
    let keys = SyncV2Keys {
        ik_pub: base64::engine::general_purpose::STANDARD.encode(&ik_pub),
        ik_priv: hex_encode(&ik_priv),
        ek_pub: base64::engine::general_purpose::STANDARD.encode(&ek_pub),
        ek_priv: hex_encode(&ek_priv),
    };
    if let Ok(mut g) = CURRENT_V2_KEYS.lock() {
        *g = Some(SyncV2KeysRaw { ik_pub, ik_priv, ek_pub: ek_pub.clone(), ek_priv });
    }
    Ok(keys)
}

pub fn generate_sync_v2_keys() -> Result<SyncV2Keys> {
    let uin = crate::account_manager::active_uin();
    generate_sync_v2_keys_for(&uin)
}

fn hex_encode(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 15) as usize] as char);
    }
    s
}

/// Đọc identity key từ `accounts/<uin>/sync_identity.json`, gen mới + lưu nếu chưa có.
fn load_or_create_identity_key(
    host: &crate::zproto_host::ZprotoHost,
    uin: &str,
) -> Result<(Vec<u8>, Vec<u8>)> {
    use base64::Engine;
    let path = crate::account_manager::sync_identity_path_for(uin);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            let dec = |k: &str| {
                v.get(k)
                    .and_then(|x| x.as_str())
                    .and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok())
            };
            if let (Some(pubk), Some(privk)) = (dec("public_key"), dec("private_key")) {
                if pubk.len() == 33 && privk.len() == 32 {
                    crate::logger::log_sync("ik: dùng identity key đã lưu");
                    return Ok((pubk, privk));
                }
            }
        }
    }
    let (pubk, privk) = host.generate_keypair()?;
    let doc = serde_json::json!({
        "public_key": base64::engine::general_purpose::STANDARD.encode(&pubk),
        "private_key": base64::engine::general_purpose::STANDARD.encode(&privk),
    });
    let text = serde_json::to_string_pretty(&doc).unwrap_or_default();
    let _ = crate::data_dir::write_private_atomic(&path, text.as_bytes());
    crate::logger::log_sync("ik: đã gen identity key mới + lưu");
    Ok((pubk, privk))
}
/// Kết quả giải mã metadata Sync V2: session id để đối chiếu với event batch.
#[derive(Debug, Default)]
pub struct SyncV2Meta {
    pub session_id: String,
    pub sender_identity: String,
}

/// Decrypt the Sync V2 metadata and retain its session record for subsequent message packages.
///
/// # Errors
/// Returns an error if sync keys are missing, the host or download fails, or metadata cannot be decoded.
pub async fn import_sync_v2_meta(s: &Session, url: &str) -> Result<SyncV2Meta> {
    // Validate local decryption prerequisites before downloading a peer package.
    let keys = current_v2_keys_raw()?;
    let host = crate::zproto_host::ZprotoHost::load()?;
    let uin = s.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
    let sek = s.zpw_sek.clone().unwrap_or_default();
    let cookie_hdr = format!("zpw_sek={sek}; zpw_uin={uin}");

    let resp = s
        .http
        .get(url)
        .header("Cookie", cookie_hdr)
        .header("User-Agent", "ZaloPC-win32-26.10.11")
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("failed to download v2 meta: HTTP {}", resp.status());
    }

    let bytes = resp.bytes().await?;
    // wasmtime host (không cần Node.js). Keys phải tồn tại trong bộ nhớ tiến
    // trình (generate_sync_v2_keys đã chạy trước khi request sync).
    let mut session_record: Option<Vec<u8>> = None;
    let mut last: Option<crate::zproto_host::DecryptedMeta> = None;
    for chunk in crate::zproto_host::split_frames(&bytes)? {
        let m = host.decrypt_metadata(&chunk, &keys.ik_pub, &keys.ik_priv, &keys.ek_priv, session_record.as_deref())?;
        session_record = Some(m.session_record.clone());
        last = Some(m);
    }
    let m = last.ok_or_else(|| anyhow::anyhow!("metadata rỗng"))?;
    store_v2_session(&m.session_record);
    Ok(SyncV2Meta {
        session_id: m.session_id,
        sender_identity: m.sender_identity,
    })
}


pub fn current_v2_keys() -> Result<SyncV2Keys> {
    use base64::Engine;
    let raw = current_v2_keys_raw()?;
    Ok(SyncV2Keys {
        ik_pub: base64::engine::general_purpose::STANDARD.encode(&raw.ik_pub),
        ik_priv: hex_encode(&raw.ik_priv),
        ek_pub: base64::engine::general_purpose::STANDARD.encode(&raw.ek_pub),
        ek_priv: hex_encode(&raw.ek_priv),
    })
}

fn current_v2_keys_raw() -> Result<SyncV2KeysRaw> {
    CURRENT_V2_KEYS
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .ok_or_else(|| anyhow::anyhow!("chưa có khóa Sync V2 — gọi generate_sync_v2_keys trước"))
}

/// Session record của metadata gần nhất (raw bytes, giữ trong bộ nhớ).
static CURRENT_V2_SESSION: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);

fn store_v2_session(session_record: &[u8]) {
    if let Ok(mut g) = CURRENT_V2_SESSION.lock() {
        *g = Some(session_record.to_vec());
    }
}

fn current_v2_session() -> Result<Vec<u8>> {
    CURRENT_V2_SESSION
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .ok_or_else(|| anyhow::anyhow!("chưa giải mã metadata — gọi import_sync_v2_meta trước"))
}
/// Đăng ký session Sync V2 đã giải mã xong metadata, key theo sync_id của event.
/// Batch message chỉ được giải mã khi sync_id của nó đã có metadata — tránh
/// dùng nhầm session của phiên cũ (#116).
static SYNC_V2_SESSIONS: std::sync::Mutex<Vec<(String, String)>> =
    std::sync::Mutex::new(Vec::new());

static ACTIVE_V2_SYNC_ID: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static SYNC_V2_IMPORTED_COUNT: AtomicUsize = AtomicUsize::new(0);
static SYNC_V2_LAST_SEEN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static SYNC_V2_INFLIGHT: AtomicUsize = AtomicUsize::new(0);

pub struct SyncV2InflightGuard;

impl SyncV2InflightGuard {
    pub fn enter() -> Self {
        SYNC_V2_INFLIGHT.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for SyncV2InflightGuard {
    fn drop(&mut self) {
        let _ = SYNC_V2_INFLIGHT.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
            Some(v.saturating_sub(1))
        });
    }
}

pub fn mark_sync_v2_last_seen() {
    SYNC_V2_LAST_SEEN.store(true, Ordering::SeqCst);
}

pub fn is_sync_v2_last_seen() -> bool {
    SYNC_V2_LAST_SEEN.load(Ordering::SeqCst)
}

pub fn sync_v2_inflight_count() -> usize {
    SYNC_V2_INFLIGHT.load(Ordering::SeqCst)
}

pub fn reset_sync_v2_imported_count() {
    SYNC_V2_IMPORTED_COUNT.store(0, Ordering::Relaxed);
    SYNC_V2_LAST_SEEN.store(false, Ordering::SeqCst);
    SYNC_V2_INFLIGHT.store(0, Ordering::SeqCst);
}

pub fn add_sync_v2_imported_count(n: usize) -> usize {
    SYNC_V2_IMPORTED_COUNT.fetch_add(n, Ordering::Relaxed) + n
}

pub fn sync_v2_imported_count() -> usize {
    SYNC_V2_IMPORTED_COUNT.load(Ordering::Relaxed)
}

pub fn set_active_v2_sync_id(sync_id: Option<String>) {
    if let Ok(mut guard) = ACTIVE_V2_SYNC_ID.lock() {
        *guard = sync_id;
    }
}

pub fn active_v2_sync_id() -> Option<String> {
    ACTIVE_V2_SYNC_ID.lock().ok().and_then(|g| g.clone())
}

static SYNC_V2_BATCH_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

static PROCESSED_SYNC_V2_URLS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

static PROCESSED_BACKUP_URLS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

static NOISED_TO_THREAD_ID: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, (String, bool)>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Xóa toàn bộ state/key đồng bộ điện thoại trong bộ nhớ khi chuyển/đăng xuất tài khoản.
pub fn reset_for_account_switch() {
    set_latest_mobile_temp_key("");
    set_last_mobile_sync_status(None);
    set_last_phone_sync_error(None);
    clear_sync_keys();
    if let Ok(mut g) = CURRENT_V2_KEYS.lock() {
        *g = None;
    }
    if let Ok(mut g) = CURRENT_V2_SESSION.lock() {
        *g = None;
    }
    if let Ok(mut m) = SYNC_V2_SESSIONS.lock() {
        m.clear();
    }
    set_active_v2_sync_id(None);
    reset_sync_v2_imported_count();
    if let Ok(mut m) = NOISED_TO_THREAD_ID.lock() {
        m.clear();
    }
    if let Ok(mut s) = PROCESSED_SYNC_V2_URLS.lock() {
        s.clear();
    }
    if let Ok(mut s) = PROCESSED_BACKUP_URLS.lock() {
        s.clear();
    }
}

/// Kiểm tra + đánh dấu URL gói backup V1 đã tải/giải mã chưa (chống trùng giữa WS và `lp_control`).
pub fn mark_backup_url_seen(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return false;
    }
    if let Ok(mut set) = PROCESSED_BACKUP_URLS.lock() {
        if set.len() > 256 {
            set.clear();
        }
        return set.insert(trimmed.to_string());
    }
    true
}

pub fn sync_v2_batch_lock() -> &'static tokio::sync::Mutex<()> {
    &SYNC_V2_BATCH_LOCK
}

/// Ghi nhận ánh xạ `noised_id` <-> `real_thread_id` trả về từ `/api/znoise`.
pub fn record_sync_v2_partition_map(entries: &[(String, String, bool)]) {
    if let Ok(mut map) = NOISED_TO_THREAD_ID.lock() {
        for (noised_id, real_id, is_group) in entries {
            let nid = noised_id.trim();
            let rid = real_id.trim();
            if nid.is_empty() || rid.is_empty() {
                continue;
            }
            let prefix = if *is_group { "group/" } else { "oneone/" };
            map.insert(nid.to_string(), (rid.to_string(), *is_group));
            map.insert(format!("{prefix}{nid}"), (rid.to_string(), *is_group));
        }
    }
}

/// Ánh xạ `part.id` (`oneone/<noised_id>`, `group/<noised_id>`, hoặc `<noised_id>`)
/// về `(real_thread_id, is_group_hint)`.
pub fn map_sync_v2_partition_id(raw_part_id: &str) -> (String, Option<bool>) {
    let trimmed = raw_part_id.trim();
    let is_group_hint = if trimmed.starts_with("group/") {
        Some(true)
    } else if trimmed.starts_with("oneone/") {
        Some(false)
    } else {
        None
    };
    let bare = trimmed
        .strip_prefix("group/")
        .or_else(|| trimmed.strip_prefix("oneone/"))
        .unwrap_or(trimmed);
    if let Ok(map) = NOISED_TO_THREAD_ID.lock() {
        if let Some((real_id, is_grp)) = map.get(trimmed).or_else(|| map.get(bare)) {
            return (real_id.clone(), Some(*is_grp));
        }
    }
    (bare.to_string(), is_group_hint)
}

/// Kiểm tra + đánh dấu `(sync_id, batch_type, url)` đã xử lý chưa (tránh xử lý
/// trùng giữa WebSocket và `lp_ctl`, vốn sẽ làm hỏng ratchet giải mã).
pub fn mark_sync_v2_batch_seen(sync_id: &str, batch_type: u8, url: &str) -> bool {
    let key = format!("{sync_id}:{batch_type}:{url}");
    if let Ok(mut set) = PROCESSED_SYNC_V2_URLS.lock() {
        if set.len() > 2048 {
            set.clear();
        }
        return set.insert(key);
    }
    true
}

pub fn record_sync_v2_session(sync_id: &str, session_id: &str) {
    if let Ok(mut m) = SYNC_V2_SESSIONS.lock() {
        m.retain(|(k, _)| k != sync_id);
        m.push((sync_id.to_string(), session_id.to_string()));
    }
}

pub fn has_sync_v2_session(sync_id: &str) -> bool {
    SYNC_V2_SESSIONS
        .lock()
        .map(|m| m.iter().any(|(k, _)| k == sync_id))
        .unwrap_or(false)
}

pub fn remove_sync_v2_session(sync_id: &str) {
    if let Ok(mut m) = SYNC_V2_SESSIONS.lock() {
        m.retain(|(k, _)| k != sync_id);
    }
}

#[derive(Debug, Default, Clone)]
pub struct ParsedProtoAttach {
    pub attach_type: Option<String>,
    pub ext_info: Option<String>,
    pub action: Option<String>,
    pub params: Option<String>,
    pub title: Option<String>,
    pub href: Option<String>,
    pub thumb: Option<String>,
    pub description: Option<String>,
    pub cat_id: Option<i32>,
    pub id: Option<i32>,
    pub child_number: Option<i32>,
    pub remains: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct ParsedProtoQuote {
    pub owner_id: String,
    pub from_d: String,
    pub cli_msg_id: u64,
    pub global_msg_id: String,
    pub msg: String,
}

#[derive(Debug, Default)]
pub struct ParsedProtoMsg {
    pub sender_id: String,
    pub client_id: u64,
    pub msg_status: i32,
    pub msg_type: i32,
    pub timestamp: u64,
    pub global_id: String,
    pub content: String,
    pub attaches: Vec<ParsedProtoAttach>,
    pub quote: Option<ParsedProtoQuote>,
}

#[derive(Debug, Default)]
pub struct ParsedProtoPartition {
    pub id: String,
    pub messages: Vec<ParsedProtoMsg>,
}

/// Read a protobuf varint and return the next byte offset when a terminating byte is found.
fn parse_vli(buf: &[u8], mut offset: usize) -> Option<(u64, usize)> {
    let mut result: u64 = 0;
    let mut shift = 0;
    while offset < buf.len() {
        let b = buf[offset];
        offset += 1;
        result |= ((b & 0x7F) as u64) << shift;
        if (b & 0x80) == 0 {
            return Some((result, offset));
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    None
}

fn parse_proto_attach(buf: &[u8]) -> Option<ParsedProtoAttach> {
    let mut att = ParsedProtoAttach::default();
    let mut offset = 0;
    while offset < buf.len() {
        let (tag, next) = parse_vli(buf, offset)?;
        offset = next;
        let field_num = tag >> 3;
        let wire_type = tag & 0x07;
        match wire_type {
            0 => {
                let (val, next) = parse_vli(buf, offset)?;
                offset = next;
                match field_num {
                    9 => att.cat_id = Some(val as i32),
                    10 => att.id = Some(val as i32),
                    11 => att.child_number = Some(val as i32),
                    _ => {}
                }
            }
            2 => {
                let (len, next) = parse_vli(buf, offset)?;
                offset = next;
                let len = len as usize;
                if offset + len > buf.len() { return None; }
                let s = String::from_utf8_lossy(&buf[offset..offset + len]).into_owned();
                offset += len;
                match field_num {
                    1 => att.attach_type = Some(s),
                    2 => att.ext_info = Some(s),
                    3 => att.action = Some(s),
                    4 => att.params = Some(s),
                    5 => att.title = Some(s),
                    6 => att.href = Some(s),
                    7 => att.thumb = Some(s),
                    8 => att.description = Some(s),
                    12 => att.remains = Some(s),
                    _ => {}
                }
            }
            1 => offset += 8,
            5 => offset += 4,
            _ => return None,
        }
    }
    Some(att)
}

fn parse_proto_quote(buf: &[u8]) -> Option<ParsedProtoQuote> {
    let mut q = ParsedProtoQuote::default();
    let mut offset = 0;
    while offset < buf.len() {
        let (tag, next) = parse_vli(buf, offset)?;
        offset = next;
        let field_num = tag >> 3;
        let wire_type = tag & 0x07;
        match wire_type {
            0 => {
                let (val, next) = parse_vli(buf, offset)?;
                offset = next;
                if field_num == 5 {
                    q.cli_msg_id = val;
                }
            }
            2 => {
                let (len, next) = parse_vli(buf, offset)?;
                offset = next;
                let len = len as usize;
                if offset + len > buf.len() {
                    return None;
                }
                let s = String::from_utf8_lossy(&buf[offset..offset + len]).into_owned();
                offset += len;
                match field_num {
                    2 => q.owner_id = s,
                    4 => q.from_d = s,
                    6 => q.global_msg_id = s,
                    7 => q.msg = s,
                    _ => {}
                }
            }
            1 => offset += 8,
            5 => offset += 4,
            _ => return None,
        }
    }
    Some(q)
}

fn parse_proto_metadata(buf: &[u8]) -> Option<(Vec<ParsedProtoAttach>, Option<ParsedProtoQuote>)> {
    let mut attaches = Vec::new();
    let mut quote = None;
    let mut offset = 0;
    while offset < buf.len() {
        let (tag, next) = parse_vli(buf, offset)?;
        offset = next;
        let field_num = tag >> 3;
        let wire_type = tag & 0x07;
        match wire_type {
            0 => {
                let (_, next) = parse_vli(buf, offset)?;
                offset = next;
            }
            2 => {
                let (len, next) = parse_vli(buf, offset)?;
                offset = next;
                let len = len as usize;
                if offset + len > buf.len() { return None; }
                if field_num == 1 {
                    if let Some(att) = parse_proto_attach(&buf[offset..offset + len]) {
                        attaches.push(att);
                    }
                } else if field_num == 4 {
                    if let Some(q) = parse_proto_quote(&buf[offset..offset + len]) {
                        quote = Some(q);
                    }
                }
                offset += len;
            }
            1 => offset += 8,
            5 => offset += 4,
            _ => return None,
        }
    }
    Some((attaches, quote))
}

/// Decode the Sync V2 fields needed for storage, including any readable attachment and quote data.
fn parse_message(buf: &[u8]) -> Option<ParsedProtoMsg> {
    let mut msg = ParsedProtoMsg::default();
    let mut offset = 0;
    while offset < buf.len() {
        let (tag, next) = parse_vli(buf, offset)?;
        offset = next;
        let field_num = tag >> 3;
        let wire_type = tag & 0x07;
        match wire_type {
            0 => {
                let (val, next) = parse_vli(buf, offset)?;
                offset = next;
                match field_num {
                    2 => msg.client_id = val,
                    3 => msg.msg_status = val as i32,
                    4 => msg.msg_type = val as i32,
                    5 => msg.timestamp = val,
                    _ => {}
                }
            }
            2 => {
                let (len, next) = parse_vli(buf, offset)?;
                offset = next;
                let len = len as usize;
                if offset + len > buf.len() {
                    return None;
                }
                let bytes = &buf[offset..offset + len];
                offset += len;
                match field_num {
                    1 => msg.sender_id = String::from_utf8_lossy(bytes).into_owned(),
                    6 => msg.global_id = String::from_utf8_lossy(bytes).into_owned(),
                    7 => msg.content = String::from_utf8_lossy(bytes).into_owned(),
                    8 => {
                        if let Some((atts, q)) = parse_proto_metadata(bytes) {
                            msg.attaches.extend(atts);
                            if q.is_some() {
                                msg.quote = q;
                            }
                        }
                    }
                    _ => {}
                }
            }
            1 => offset += 8,
            5 => offset += 4,
            _ => return None,
        }
    }
    Some(msg)
}

/// Decode a conversation partition while retaining independently readable message records.
fn parse_partition(buf: &[u8]) -> Option<ParsedProtoPartition> {
    let mut part = ParsedProtoPartition::default();
    let mut offset = 0;
    while offset < buf.len() {
        let (tag, next) = parse_vli(buf, offset)?;
        offset = next;
        let field_num = tag >> 3;
        let wire_type = tag & 0x07;
        match (field_num, wire_type) {
            (1, 2) => {
                let (len, next) = parse_vli(buf, offset)?;
                offset = next;
                let len = len as usize;
                if offset + len > buf.len() {
                    return None;
                }
                part.id = String::from_utf8_lossy(&buf[offset..offset + len]).into_owned();
                offset += len;
            }
            (2, 2) => {
                let (len, next) = parse_vli(buf, offset)?;
                offset = next;
                let len = len as usize;
                if offset + len > buf.len() {
                    return None;
                }
                if let Some(m) = parse_message(&buf[offset..offset + len]) {
                    part.messages.push(m);
                }
                offset += len;
            }
            (_, 0) => {
                let (_, next) = parse_vli(buf, offset)?;
                offset = next;
            }
            (_, 2) => {
                let (len, next) = parse_vli(buf, offset)?;
                offset = next + len as usize;
            }
            _ => return None,
        }
    }
    Some(part)
}

/// Giải nén payload Sync V2 tự nhận dạng: zstd (28 b5 2f fd) / gzip (1f 8b) /
/// zlib (78 xx) / raw. Không bao giờ fail — trả nguyên bytes khi không nhận ra.
pub fn decompress_sync_payload(buf: &[u8]) -> Vec<u8> {
    use std::io::Read;
    if buf.len() >= 4 && buf[0] == 0x28 && buf[1] == 0xb5 && buf[2] == 0x2f && buf[3] == 0xfd {
        if let Ok(d) = zstd::decode_all(std::io::Cursor::new(buf)) {
            return d;
        }
        return buf.to_vec();
    }
    if buf.len() >= 2 && buf[0] == 0x1f && buf[1] == 0x8b {
        let mut out = Vec::new();
        if flate2::read::GzDecoder::new(buf).read_to_end(&mut out).is_ok() {
            return out;
        }
        return buf.to_vec();
    }
    if buf.len() >= 2 && buf[0] == 0x78 {
        let mut out = Vec::new();
        if flate2::read::ZlibDecoder::new(buf).read_to_end(&mut out).is_ok() {
            return out;
        }
    }
    buf.to_vec()
}

/// Extract conversation partitions from an uncompressed Sync V2 protobuf payload.
///
/// An unreadable partition can be skipped when its length is available.
///
/// # Panics
/// A length field that overflows a platform byte offset can panic with overflow checks enabled.
pub fn parse_sync_chunk(buf: &[u8]) -> Option<Vec<ParsedProtoPartition>> {
    let mut parts = Vec::new();
    let mut offset = 0;
    while offset < buf.len() {
        let (tag, next) = parse_vli(buf, offset)?;
        offset = next;
        let field_num = tag >> 3;
        let wire_type = tag & 0x07;
        match (field_num, wire_type) {
            (2, 2) => {
                let (len, next) = parse_vli(buf, offset)?;
                offset = next;
                let len = len as usize;
                if offset + len > buf.len() {
                    return None;
                }
                if let Some(p) = parse_partition(&buf[offset..offset + len]) {
                    parts.push(p);
                }
                offset += len;
            }
            (_, 0) => {
                let (_, next) = parse_vli(buf, offset)?;
                offset = next;
            }
            (_, 2) => {
                let (len, next) = parse_vli(buf, offset)?;
                offset = next + len as usize;
            }
            _ => return None,
        }
    }
    Some(parts)
}

/// Parse bare Sync V2 call duration strings like `"0:18"`, `"0:2"`, `"1:14"`, or `"1:02:03"`.
pub(crate) fn parse_colon_duration_secs(raw: &str) -> Option<u64> {
    let s = raw.trim();
    if s.is_empty() || !s.contains(':') {
        return None;
    }
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 && parts.len() != 3 {
        return None;
    }
    if !parts
        .iter()
        .all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    if parts.len() == 2 {
        let mins: u64 = parts[0].parse().ok()?;
        let secs: u64 = parts[1].parse().ok()?;
        if secs >= 60 {
            return None;
        }
        Some(mins * 60 + secs)
    } else {
        let hrs: u64 = parts[0].parse().ok()?;
        let mins: u64 = parts[1].parse().ok()?;
        let secs: u64 = parts[2].parse().ok()?;
        if mins >= 60 || secs >= 60 {
            return None;
        }
        Some(hrs * 3600 + mins * 60 + secs)
    }
}

pub(crate) fn is_raw_call_duration_text(raw: &str) -> bool {
    parse_colon_duration_secs(raw).is_some()
}

fn parse_json_obj(raw: Option<&str>) -> Option<serde_json::Map<String, serde_json::Value>> {
    let s = raw?.trim();
    if s.is_empty() {
        return None;
    }
    match serde_json::from_str::<serde_json::Value>(s).ok()? {
        serde_json::Value::Object(map) => Some(map),
        _ => None,
    }
}

fn format_highlight_msg(obj: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let msg_obj = obj.get("msg")?.as_object()?;
    let base = msg_obj
        .get("vi")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| msg_obj.get("en").and_then(|v| v.as_str()).filter(|s| !s.is_empty()))?;
    let mut out = base.to_string();
    if let Some(arr) = obj
        .get("highLightsV2")
        .or_else(|| obj.get("highLights"))
        .and_then(|v| v.as_array())
    {
        for (idx, item) in arr.iter().enumerate() {
            if let Some(dpn) = item
                .as_object()
                .and_then(|m| m.get("dpn"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                out = out.replace(&format!("%{}$s", idx + 1), dpn);
            }
        }
    }
    Some(out)
}

/// Format canonical call bubble text (`📞 Cuộc gọi thoại ...` / `📹 Cuộc gọi video ...`)
/// from Sync V2 `Attach` metadata (`action`, `params`, `description`, and `m.content` like `"0:18"` or `""`).
pub(crate) fn format_sync_v2_call_text(
    content: &str,
    action: Option<&str>,
    params_raw: Option<&str>,
    description: Option<&str>,
    is_outgoing: bool,
) -> (String, bool, Option<u64>) {
    let params = parse_json_obj(params_raw);
    let param_u64 = |keys: &[&str]| -> Option<u64> {
        let p = params.as_ref()?;
        for k in keys {
            if let Some(v) = p.get(*k) {
                if let Some(n) = v.as_u64() {
                    return Some(n);
                }
                if let Some(i) = v.as_i64() {
                    if i >= 0 {
                        return Some(i as u64);
                    }
                }
                if let Some(s) = v.as_str().and_then(|s| s.trim().parse::<u64>().ok()) {
                    return Some(s);
                }
            }
        }
        None
    };

    let is_video = param_u64(&["calltype", "callType"]) == Some(1)
        || description.is_some_and(|d| d.to_lowercase().contains("video"))
        || content.to_lowercase().contains("video");

    let is_caller = params
        .as_ref()
        .and_then(|p| p.get("isCaller"))
        .and_then(|v| match v {
            serde_json::Value::Bool(b) => Some(*b),
            serde_json::Value::Number(n) => n.as_i64().map(|i| i != 0),
            serde_json::Value::String(s) => match s.trim() {
                "1" | "true" => Some(true),
                "0" | "false" => Some(false),
                _ => None,
            },
            _ => None,
        })
        .unwrap_or_else(|| match action {
            Some("recommened.calling") | Some("recommended.calling") => true,
            Some("recommened.receivecall") | Some("recommended.receivecall") => false,
            _ => is_outgoing,
        });

    let colon_secs = parse_colon_duration_secs(content);
    let dur_secs = param_u64(&["duration", "dur"]).or(colon_secs);
    let action_missed = action.is_some_and(|a| a.contains("misscall"));
    let reason_code = param_u64(&["reason"]);

    let is_missed = action_missed
        || matches!(dur_secs, Some(0))
        || (dur_secs.is_none() && content.trim().is_empty());

    let icon = if is_video { "📹" } else { "📞" };
    let kind_lbl = if is_video {
        "Cuộc gọi video"
    } else {
        "Cuộc gọi thoại"
    };
    let dir_lbl = if is_caller { "đi" } else { "đến" };

    let text = if is_missed {
        let reason_str = match reason_code {
            Some(1) => "Người nhận đang bận",
            Some(2) => {
                if is_caller {
                    "Người nhận không nghe máy"
                } else {
                    "Bạn bị nhỡ"
                }
            }
            Some(3) => {
                if is_caller {
                    "Người nhận từ chối"
                } else {
                    "Bạn đã từ chối"
                }
            }
            Some(4 | 5) => {
                if is_caller {
                    "Bạn đã hủy"
                } else {
                    "Bạn bị nhỡ"
                }
            }
            _ => {
                if is_caller {
                    "Bạn đã hủy"
                } else {
                    "Bạn bị nhỡ"
                }
            }
        };
        format!("{icon} {kind_lbl} {dir_lbl} (0s) — {reason_str}")
    } else if let Some(secs) = dur_secs {
        format!("{icon} {kind_lbl} {dir_lbl} ({secs}s)")
    } else {
        format!("{icon} {kind_lbl} {dir_lbl}")
    };

    (text, is_video, dur_secs)
}

fn is_sync_v2_call_attach(att: &ParsedProtoAttach, content: &str) -> bool {
    if let Some(act) = att.action.as_deref().map(str::trim) {
        if matches!(
            act,
            "recommened.calltime"
                | "recommened.misscall"
                | "recommened.calling"
                | "recommened.receivecall"
                | "recommended.calltime"
                | "recommended.misscall"
                | "recommended.calling"
                | "recommended.receivecall"
        ) {
            return true;
        }
    }
    if att.title.as_deref().map(str::trim) == Some("sendBubbleMessage") {
        return true;
    }
    if att
        .description
        .as_deref()
        .is_some_and(|d| d.trim().starts_with("Cuộc gọi"))
        && (content.trim().is_empty() || parse_colon_duration_secs(content).is_some())
    {
        return true;
    }
    false
}

/// Convert a decoded Sync V2 protobuf `ParsedProtoMsg` into a `StoredMessage` using Zalo's
/// verified archive `xi` message-type table (`0=Text`, `2=Doodle`, `3|4|31=Photo`, `6=Voice`,
/// `10=Sticker`, `12=Contact/Call/Link`, `15=OA`, `18=Location`, `19|25=Video`, `20=ChatInfo`,
/// `21=ECard`, `22=File`, `23=Gif`, `24|53=Webcontent`, `26=Poll`, `33=Delete`, `36=Undo`).
pub(crate) fn proto_attach_to_json(att: &ParsedProtoAttach) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    if let Some(ref t) = att.attach_type {
        if !t.is_empty() {
            obj.insert("type".into(), serde_json::json!(t));
        }
    }
    if let Some(ref a) = att.action {
        if !a.is_empty() {
            obj.insert("action".into(), serde_json::json!(a));
        }
    }
    if let Some(ref t) = att.title {
        if !t.is_empty() {
            obj.insert("title".into(), serde_json::json!(t));
        }
    }
    if let Some(ref d) = att.description {
        if !d.is_empty() {
            obj.insert("description".into(), serde_json::json!(d));
        }
    }
    if let Some(ref th) = att.thumb {
        if !th.is_empty() {
            obj.insert("thumb".into(), serde_json::json!(th));
        }
    }
    if let Some(ref hr) = att.href {
        if !hr.is_empty() {
            obj.insert("href".into(), serde_json::json!(hr));
        }
    }
    if let Some(ref p) = att.params {
        if !p.is_empty() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(p) {
                obj.insert("params".into(), v);
            } else {
                obj.insert("params".into(), serde_json::json!(p));
            }
        }
    }
    if let Some(ref ext) = att.ext_info {
        if !ext.is_empty() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(ext) {
                if v.get("zinstant_msg").is_some() {
                    if let Some(zm) = v.get("zinstant_msg") {
                        obj.insert("zinstant_msg".into(), zm.clone());
                    }
                } else {
                    obj.insert("extInfo".into(), v);
                }
            }
        }
    }
    if let Some(ref rem) = att.remains {
        if !rem.is_empty() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(rem) {
                if v.get("zinstant_msg").is_some() && !obj.contains_key("zinstant_msg") {
                    if let Some(zm) = v.get("zinstant_msg") {
                        obj.insert("zinstant_msg".into(), zm.clone());
                    }
                }
                obj.insert("remains".into(), v);
            } else {
                obj.insert("remains".into(), serde_json::json!(rem));
            }
        }
    }
    serde_json::Value::Object(obj)
}

fn extract_sync_v2_oa_card(
    msg_type: i32,
    msg_type_str: &str,
    raw_content: &str,
    attaches: &[ParsedProtoAttach],
) -> Option<crate::chat::OaRichCard> {
    if matches!(
        msg_type,
        2 | 3 | 4 | 6 | 10 | 18 | 19 | 20 | 22 | 23 | 25 | 26 | 31 | 33 | 36
    ) {
        return None;
    }
    let trimmed = raw_content.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(card) = crate::chat::extract_oa_rich_card(msg_type_str, Some(&v)) {
                return Some(card);
            }
        }
    }
    if attaches.is_empty() {
        return None;
    }
    if msg_type == 15 || attaches.len() > 1 {
        let arr = serde_json::Value::Array(attaches.iter().map(proto_attach_to_json).collect());
        if let Some(card) = crate::chat::extract_oa_rich_card(msg_type_str, Some(&arr)) {
            return Some(card);
        }
    }
    let single = proto_attach_to_json(&attaches[0]);
    crate::chat::extract_oa_rich_card(msg_type_str, Some(&single))
}

fn decode_sync_v2_proto_message(
    m: ParsedProtoMsg,
    thread_id: &str,
    uin: &str,
    my_name: &str,
    peer_thread_name: &str,
) -> crate::store::StoredMessage {
    let (mapped_sender, _) = map_sync_v2_partition_id(&m.sender_id);
    let is_outgoing = (!uin.is_empty()) && (mapped_sender == uin || m.sender_id == uin);
    let sender_name = if is_outgoing {
        my_name.to_string()
    } else {
        peer_thread_name.to_string()
    };
    let real_sender_id = if is_outgoing {
        uin.to_string()
    } else if !mapped_sender.trim().is_empty() {
        mapped_sender
    } else {
        thread_id.to_string()
    };
    let msg_id = if !m.global_id.is_empty() && m.global_id != "0" {
        m.global_id
    } else {
        m.client_id.to_string()
    };

    let mut msg_type_str = match m.msg_type {
        0 | 1 => "chat.text",
        2 => "chat.doodle",
        3 | 4 | 31 => "chat.photo",
        6 => "chat.voice",
        10 => "chat.sticker",
        12 => "chat.recommended",
        15 => "chat.list.action",
        18 => "chat.location.new",
        19 => "chat.video.msg",
        20 => "chat.system",
        21 | 24 => "chat.webcontent",
        22 => "share.file",
        23 => "chat.gif",
        25 => "chat.video.live.msg",
        26 => "group.poll",
        33 => "chat.delete",
        36 => "chat.undo",
        53 => "chat.webcontent.v2",
        _ => "chat.text",
    };

    let mut text = m.content;
    let attaches = m.attaches;
    let raw_content_snapshot = text.clone();
    let mut raw_meta = serde_json::Map::new();
    let att_opt = attaches.first().cloned();
    let mut is_call_bubble = false;

    if let Some(ref att) = att_opt {
        if is_sync_v2_call_attach(att, &text) {
            is_call_bubble = true;
            let (call_txt, is_video, dur_opt) = format_sync_v2_call_text(
                &text,
                att.action.as_deref(),
                att.params.as_deref(),
                att.description.as_deref(),
                is_outgoing,
            );
            text = call_txt;
            msg_type_str = "chat.ecall";
            raw_meta.insert("call_bubble".into(), serde_json::json!(true));
            raw_meta.insert("is_video".into(), serde_json::json!(is_video));
            if let Some(dur) = dur_opt {
                raw_meta.insert("duration".into(), serde_json::json!(dur));
            }
        } else {
            let params_obj = parse_json_obj(att.params.as_deref());
            if let Some(href) = att.href.as_deref().filter(|h| !h.is_empty()) {
                raw_meta.insert("link_url".into(), serde_json::json!(href));
            }
            if let Some(title) = att
                .title
                .as_deref()
                .filter(|t| !t.is_empty() && *t != "sendBubbleMessage")
            {
                raw_meta.insert("link_title".into(), serde_json::json!(title));
            }
            if let Some(thumb) = att.thumb.as_deref().filter(|t| !t.is_empty()) {
                raw_meta.insert("link_thumb".into(), serde_json::json!(thumb));
            }
            if let Some(desc) = att.description.as_deref().filter(|d| !d.is_empty()) {
                raw_meta.insert("link_desc".into(), serde_json::json!(desc));
            }
            if let Some(sid) = att.id {
                raw_meta.insert("stickerId".into(), serde_json::json!(sid));
                if let Some(cid) = att.cat_id {
                    raw_meta.insert("cateId".into(), serde_json::json!(cid));
                }
                let st_type = att
                    .attach_type
                    .as_deref()
                    .and_then(|t| t.trim().parse::<u32>().ok())
                    .unwrap_or(7);
                raw_meta.insert("type".into(), serde_json::json!(st_type));
            }

            if att.action.as_deref() == Some("rtf") && text.trim().is_empty() {
                if let Some(t) = att.title.as_deref().filter(|s| !s.is_empty()) {
                    text = t.to_string();
                }
            }

            match m.msg_type {
                2 | 3 | 4 | 23 | 31 => {
                    raw_meta.insert("link_src".into(), serde_json::json!("Hình ảnh Zalo"));
                    if let Some(hd) = params_obj
                        .as_ref()
                        .and_then(|p| p.get("hd"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        if !raw_meta.contains_key("link_thumb") {
                            if let Some(orig) = raw_meta.get("link_url").cloned() {
                                raw_meta.insert("link_thumb".into(), orig);
                            }
                        }
                        raw_meta.insert("link_url".into(), serde_json::json!(hd));
                    }
                    if text.trim().is_empty()
                        || (text.len() > 20 && !text.contains(' ') && !text.starts_with('['))
                    {
                        text = "[Hình ảnh]".into();
                    }
                }
                6 => {
                    raw_meta.insert("link_src".into(), serde_json::json!("Thoại Zalo"));
                    if !raw_meta.contains_key("link_url") {
                        if let Some(m4a) = params_obj
                            .as_ref()
                            .and_then(|p| p.get("m4a").or_else(|| p.get("voice")))
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            raw_meta.insert("link_url".into(), serde_json::json!(m4a));
                        }
                    }
                    if let Some(dur) = params_obj.as_ref().and_then(|p| p.get("duration")) {
                        raw_meta.insert("duration".into(), dur.clone());
                    }
                    text = "[Tin nhắn thoại]".into();
                }
                10 => {
                    text = match (att.cat_id, att.id) {
                        (Some(cid), Some(sid)) => format!("[^{cid},{sid}^]"),
                        _ => "[Nhãn dán]".into(),
                    };
                }
                12 => {
                    let act = att.action.as_deref().unwrap_or("");
                    if act == "recommened.user" || act == "recommended.user" {
                        msg_type_str = "chat.contact";
                        let name = att
                            .title
                            .as_deref()
                            .filter(|s| !s.is_empty())
                            .or_else(|| att.description.as_deref().filter(|s| !s.is_empty()))
                            .unwrap_or(text.trim());
                        text = if name.is_empty() {
                            "[Danh thiếp]".into()
                        } else {
                            format!("[Danh thiếp] {name}")
                        };
                    } else if text.trim().is_empty() {
                        if let Some(u) = raw_meta
                            .get("link_url")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            text = u.to_string();
                        } else if let Some(t) = raw_meta
                            .get("link_title")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            text = t.to_string();
                        }
                    }
                }
                18 => {
                    let loc_desc = att
                        .title
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .or_else(|| att.description.as_deref().filter(|s| !s.is_empty()))
                        .unwrap_or(text.trim());
                    if !text.trim().starts_with("[Vị trí]") {
                        text = if loc_desc.is_empty() {
                            "[Vị trí]".into()
                        } else {
                            format!("[Vị trí] {loc_desc}")
                        };
                    }
                }
                19 | 25 => {
                    raw_meta.insert("link_src".into(), serde_json::json!("Video Zalo"));
                    raw_meta.insert("link_title".into(), serde_json::json!("Video"));
                    text = "[Video]".into();
                }
                20 => {
                    if text.trim().is_empty() {
                        text = params_obj
                            .as_ref()
                            .and_then(format_highlight_msg)
                            .or_else(|| {
                                att.title
                                    .as_deref()
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_string)
                            })
                            .or_else(|| {
                                att.description
                                    .as_deref()
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_string)
                            })
                            .unwrap_or_default();
                    }
                    // System notifications should not carry link_title as a clickable card.
                    raw_meta.remove("link_title");
                }
                21 | 24 | 53 => {
                    if text.trim().is_empty() {
                        let preview = params_obj
                            .as_ref()
                            .and_then(|p| {
                                p.get("previewTxt")
                                    .and_then(|v| v.as_str())
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_string)
                                    .or_else(|| {
                                        p.get("customMsg")
                                            .and_then(|v| v.as_object())
                                            .and_then(format_highlight_msg)
                                    })
                            })
                            .or_else(|| {
                                att.title
                                    .as_deref()
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_string)
                            })
                            .or_else(|| {
                                att.description
                                    .as_deref()
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_string)
                            });
                        if let Some(p) = preview {
                            text = p;
                        }
                    }
                }
                22 => {
                    raw_meta.insert("link_src".into(), serde_json::json!("Tập tin Zalo"));
                    let title = raw_meta
                        .get("link_title")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or(text.trim());
                    text = if title.is_empty() {
                        "[Tập tin]".into()
                    } else if title.starts_with("[Tập tin]") {
                        title.to_string()
                    } else {
                        format!("[Tập tin] {title}")
                    };
                }
                26 => {
                    let q = params_obj
                        .as_ref()
                        .and_then(|p| p.get("question"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .or_else(|| att.title.as_deref().filter(|s| !s.is_empty()))
                        .unwrap_or(text.trim());
                    text = if q.is_empty() {
                        "[Bình chọn]".into()
                    } else if q.starts_with("[Bình chọn]") || q.starts_with("[Khảo sát]") {
                        q.to_string()
                    } else {
                        format!("[Bình chọn] {q}")
                    };
                }
                36 => {
                    text = "[Tin nhắn đã thu hồi]".into();
                }
                _ => {}
            }
        }
    } else {
        match m.msg_type {
            3 | 4 | 31 | 2 | 23 if text.trim().is_empty() => text = "[Hình ảnh]".into(),
            6 => text = "[Tin nhắn thoại]".into(),
            10 if text.trim().is_empty() => text = "[Nhãn dán]".into(),
            19 | 25 => text = "[Video]".into(),
            22 if text.trim().is_empty() => text = "[Tập tin]".into(),
            36 => text = "[Tin nhắn đã thu hồi]".into(),
            _ => {}
        }
    }

    // Fallback URL-based video detection for non-call messages
    if msg_type_str != "chat.ecall"
        && raw_meta
            .get("link_url")
            .and_then(|v| v.as_str())
            .is_some_and(|u| {
                let l = u.to_ascii_lowercase();
                l.contains("video-") || l.contains(".dlmd.me") || l.ends_with(".mp4")
            })
    {
        msg_type_str = "chat.video.msg";
        raw_meta.insert("link_src".into(), serde_json::json!("Video Zalo"));
        raw_meta.insert("link_title".into(), serde_json::json!("Video"));
        text = "[Video]".into();
    }

    if !is_call_bubble && msg_type_str != "chat.ecall" {
        if let Some(card) = extract_sync_v2_oa_card(
            m.msg_type,
            msg_type_str,
            &raw_content_snapshot,
            &attaches,
        ) {
            if let Ok(val) = serde_json::to_value(&card) {
                raw_meta.insert("oa_card".into(), val);
            }
            let t_trim = text.trim();
            if t_trim.is_empty() || t_trim.starts_with('[') || t_trim.starts_with('{') {
                if let Some(fallback_title) = card
                    .title
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        card.items
                            .first()
                            .map(|i| i.title.as_str())
                            .filter(|s| !s.is_empty())
                    })
                    .or_else(|| card.description.as_deref().filter(|s| !s.is_empty()))
                {
                    text = fallback_title.to_string();
                }
            }
        }
    }

    if let Some(q) = m.quote {
        let q_id = if !q.global_msg_id.is_empty() && q.global_msg_id != "0" {
            Some(q.global_msg_id)
        } else if q.cli_msg_id > 0 {
            Some(q.cli_msg_id.to_string())
        } else {
            None
        };
        if let Some(id) = q_id {
            raw_meta.insert("reply_to_id".into(), serde_json::json!(id));
        }
        if !q.from_d.is_empty() {
            raw_meta.insert("reply_to_sender".into(), serde_json::json!(q.from_d));
        }
        if !q.msg.is_empty() {
            raw_meta.insert("reply_to_text".into(), serde_json::json!(q.msg));
        }
    }

    let raw_json = if raw_meta.is_empty() {
        String::new()
    } else {
        serde_json::Value::Object(raw_meta).to_string()
    };

    crate::store::StoredMessage {
        msg_id,
        thread_id: thread_id.to_string(),
        sender_id: real_sender_id,
        sender_name,
        text,
        msg_type: msg_type_str.into(),
        ts: m.timestamp as i64,
        is_outgoing,
        raw_json,
    }
}

pub async fn import_sync_v2_messages(s: &Session, url: &str, is_last: bool) -> Result<usize> {
    import_sync_v2_messages_inner(s, url, None, is_last).await
}

/// Import one decrypted Sync V2 message package into the captured account and count newly stored messages.
///
/// # Errors
/// Returns an error if the metadata session is missing or the package cannot be fetched, decoded, or opened for import.
pub(crate) async fn import_sync_v2_messages_inner(
    s: &Session,
    url: &str,
    sync_id: Option<&str>,
    is_last: bool,
) -> Result<usize> {
    // A message package cannot be imported before its metadata session exists.
    let session_record = current_v2_session()?;
    let host = crate::zproto_host::ZprotoHost::load()?;
    let uin = s.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
    let sek = s.zpw_sek.clone().unwrap_or_default();
    let cookie_hdr = format!("zpw_sek={sek}; zpw_uin={uin}");

    let resp = s
        .http
        .get(url)
        .header("Cookie", cookie_hdr)
        .header("User-Agent", "ZaloPC-win32-26.10.11")
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("failed to download v2 messages: HTTP {}", resp.status());
    }

    let bytes = resp.bytes().await?;
    // wasmtime host (không cần Node.js). Multi-chunk: decrypt + unpack từng
    // chunk, nối protobuf bytes lại (đúng luồng HistoryCodec mẫu).
    let frames = crate::zproto_host::split_frames(&bytes)?;
    if frames.is_empty() {
        anyhow::bail!("gói sync rỗng (không có frame nào)");
    }
    let mut parts: Vec<u8> = Vec::new();
    for chunk in frames {
        let res = host.decrypt_message(&chunk, &session_record)?;
        if res.plaintext.is_empty() {
            anyhow::bail!("giải mã sync trả về rỗng (payload lỗi)");
        }
        parts.extend_from_slice(&decompress_sync_payload(&res.plaintext));
    }
    let decrypted_bytes = parts;
    // Nếu batch trả raw còn nén thì Rust tự nhận dạng thêm
    // (zstd/zlib/gzip — Zalo dùng cả 3 theo HistoryCodec mẫu).
    let decompressed = decompress_sync_payload(&decrypted_bytes);

    let partitions = parse_sync_chunk(&decompressed).ok_or_else(|| {
        anyhow::anyhow!(
            "payload sync hỏng ({} bytes sau giải mã, parse thất bại)",
            decompressed.len()
        )
    })?;
    let mut store = crate::store::MessageStore::open_account(&uin)?;
    let my_name = s.login.as_ref().map(|l| l.display_name.clone()).unwrap_or_else(|| "Tôi".into());
    let mut cached_threads = crate::chat::load_threads_cache_for(&uin);

    let mut imported = 0;
    for part in partitions {
        let part_msg_count = part.messages.len();
        let mut part_oa_cards = 0usize;
        let (mut thread_id, is_group_hint) = map_sync_v2_partition_id(&part.id);
        let thread_known = cached_threads.iter().any(|t| t.id == thread_id);
        if !thread_known && is_group_hint != Some(true) {
            if let Some(peer_uid) = part.messages.iter().find_map(|m| {
                let sid = m.sender_id.trim();
                (!sid.is_empty() && sid != uin && sid.bytes().all(|b| b.is_ascii_digit()))
                    .then(|| sid.to_string())
            }) {
                thread_id = peer_uid;
            }
        }
        if !thread_id.is_empty() && !cached_threads.iter().any(|t| t.id == thread_id) {
            let short: String = thread_id.chars().take(8).collect();
            cached_threads.push(Thread {
                id: thread_id.clone(),
                name: format!("Hội thoại {short}"),
                is_group: is_group_hint.unwrap_or(false),
                ..Default::default()
            });
        }
        let peer_thread_name = cached_threads
            .iter()
            .find(|t| t.id == thread_id && !t.is_group)
            .map(|t| t.name.clone())
            .unwrap_or_default();
        for m in part.messages {
            let mt_num = m.msg_type;
            let att_len = m.attaches.len();
            let content_len = m.content.len();
            let att0_diag = m
                .attaches
                .first()
                .map(|a| {
                    format!(
                        "t={:?} act={:?} p={} e={} r={}",
                        a.attach_type,
                        a.action,
                        a.params.as_ref().map_or(0, String::len),
                        a.ext_info.as_ref().map_or(0, String::len),
                        a.remains.as_ref().map_or(0, String::len)
                    )
                })
                .unwrap_or_default();
            let sm = decode_sync_v2_proto_message(
                m,
                &thread_id,
                &uin,
                &my_name,
                &peer_thread_name,
            );
            if sm.raw_json.contains("oa_card") {
                part_oa_cards += 1;
            } else if sm.raw_json.is_empty() && (mt_num != 0 || att_len > 0) {
                crate::logger::log_sync(&format!(
                    "v2_empty_raw tid={} mid={} mt={} clen={} atts={} {}",
                    thread_id, sm.msg_id, mt_num, content_len, att_len, att0_diag
                ));
            }

            if store.upsert_message(&sm).unwrap_or(false) {
                imported += 1;
            }
        }
        crate::logger::log_sync(&format!(
            "v2_part part={} -> tid={} msgs={} oa_cards={}",
            part.id, thread_id, part_msg_count, part_oa_cards
        ));
    }

    let _ = store.hydrate_threads(&mut cached_threads, 50);
    crate::chat::save_threads_cache_for(&uin, &cached_threads);

    // Không xoá `SYNC_V2_SESSIONS` ngay tại `is_last == true` vì các gói upload
    // song song có thể tới trễ hơn gói `isLast=1` vài trăm ms; việc dọn session
    // được thực hiện tập trung ở cuối Phase 1 sau khi gửi `cmd 591`.
    let _ = (sync_id, is_last);

    // Keys/session giữ trong bộ nhớ tiến trình (CURRENT_V2_KEYS/_SESSION),
    // không còn file temp sync_v2_*.bin/json của helper JS.

    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatMessage;

    #[test]
    fn android_sender_identity_uses_each_accounts_backup_mapping() {
        let mappings = std::collections::HashMap::from([
            ("101.db".into(), "web-account-a".into()),
            ("202.db".into(), "web-account-b".into()),
            ("group_303.db".into(), "web-account-b".into()),
        ]);
        assert_eq!(
            android_self_sender_id("web-account-a", &mappings).unwrap(),
            101
        );
        assert_eq!(
            android_self_sender_id("web-account-b", &mappings).unwrap(),
            202
        );
    }

    #[test]
    fn android_sender_identity_rejects_missing_mapping_even_for_decimal_web_uin() {
        assert!(android_self_sender_id("1234567890123456789", &Default::default()).is_err());
    }

    #[test]
    fn android_sender_identity_rejects_ambiguous_backup_mapping() {
        let mappings = std::collections::HashMap::from([
            ("101.db".into(), "web-account-a".into()),
            ("202.db".into(), "web-account-a".into()),
        ]);
        assert!(android_self_sender_id("web-account-a", &mappings).is_err());
    }

    fn thread_with(ids: &[&str]) -> Thread {
        Thread {
            id: "g1".into(),
            name: "Nhóm".into(),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: true,
            messages: ids
                .iter()
                .map(|id| ChatMessage {
                    id: (*id).into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn history_sync_requires_an_advancing_cursor_when_more_pages_exist() {
        assert_eq!(require_next_cursor(Some("20".into()), "10").unwrap(), "20");
        assert!(require_next_cursor(None, "10").is_err());
        assert!(require_next_cursor(Some("10".into()), "10").is_err());
    }

    #[test]
    fn sync_error_summary_does_not_expose_session_details() {
        let error: anyhow::Error = crate::session::SessionError::new(600, "private message")
            .at("https://example.test/?params=private-token")
            .into();

        let summary = sync_error_summary(&error);
        assert_eq!(summary, "Zalo trả mã lỗi 600");
        assert!(!summary.contains("private"));
    }

    #[test]
    fn cursor_moves_to_the_oldest_server_message() {
        assert_eq!(
            next_cursor(&thread_with(&["5", "9"]), "9").as_deref(),
            Some("5")
        );
        assert_eq!(
            next_cursor(&thread_with(&["out_1", "7", "9"]), "9").as_deref(),
            Some("7")
        );
    }

    #[tokio::test]
    async fn wake_phone_sync_v2_rejects_logged_out_sessions_before_key_generation() {
        let session = Session::new(false, false, false).unwrap();
        let error = chat::wake_phone_sync_v2(&session)
            .await
            .expect_err("logged-out Sync v2 must not generate keys or queue a wakeup");
        assert!(error.to_string().contains("chưa đăng nhập"));
    }

    #[tokio::test]
    async fn sync_v2_import_checks_local_prerequisites_before_building_http_requests() {
        let session = Session::new(false, false, false).unwrap();
        let meta = import_sync_v2_meta(&session, "invalid-url")
            .await
            .unwrap_err();
        assert!(meta.to_string().contains("khóa Sync V2"), "{meta}");
        let messages = import_sync_v2_messages(&session, "invalid-url", false)
            .await
            .unwrap_err();
        assert!(messages.to_string().contains("metadata"), "{messages}");
    }

    #[test]
    fn cursor_stops_when_page_brought_nothing_older() {
        assert_eq!(next_cursor(&thread_with(&["5", "9"]), "5"), None);
        assert_eq!(next_cursor(&thread_with(&[]), "5"), None);
        assert_eq!(next_cursor(&thread_with(&["", "out_2"]), "5"), None);
    }

    #[test]
    fn generate_sync_public_key_produces_valid_rsa2048_spki_der_base64() {
        use base64::Engine;
        // Key cố định của máy (lưu file): 2 lần gọi phải TRÙNG nhau.
        let _ = std::fs::remove_file("sync_rsa_identity.json");
        let k1 = generate_sync_public_key();
        let k2 = generate_sync_public_key();
        assert_eq!(k1, k2);
        let _ = std::fs::remove_file("sync_rsa_identity.json");
        assert!(
            k1.starts_with("MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA"),
            "must have valid ASN.1 RSA-2048 SPKI DER prefix: {k1}"
        );
        let raw = base64::engine::general_purpose::STANDARD
            .decode(&k1)
            .expect("valid base64");
        assert_eq!(raw.len(), 294);
        assert_eq!(&raw[raw.len() - 5..], &[0x02, 0x03, 0x01, 0x00, 0x01]);
    }


    #[test]
    fn extract_mobile_sync_controls_parses_v1_and_v2_events() {
        let payload = serde_json::json!({
            "controls": [
                {
                    "content": {
                        "act_type": "transfer_sync2",
                        "act": "transfer_status",
                        "data": "{\"syncId\":\"abc123\",\"fromDevice\":0,\"status\":3}"
                    }
                },
                {
                    "content": {
                        "act_type": "syncmsgmb",
                        "act": "user_confirm",
                        "data": "{\"user_action\":2,\"pc_name\":\"ADMIN-PC\"}"
                    }
                },
                {
                    "content": {
                        "act_type": "transfer_sync2",
                        "act": "upload_batch",
                        "data": "{\"syncId\":\"abc123\",\"err\":0,\"msgUrl\":\"https://f.zalo.me/batch1\"}"
                    }
                }
            ]
        });
        let evs = crate::chat::extract_mobile_sync_controls(&payload);
        assert_eq!(
            evs,
            vec![
                crate::chat::MobileSyncControlEvent::WaitingConfirmOnPhone {
                    sync_id: "abc123".into()
                },
                crate::chat::MobileSyncControlEvent::MobileBusyRestoring,
                crate::chat::MobileSyncControlEvent::SyncV2Batch {
                    url: "https://f.zalo.me/batch1".into(),
                    sync_id: "abc123".into(),
                    batch_type: 0,
                    is_last: false,
                },
            ]
        );
    }

    #[test]
    fn decompress_sync_payload_detects_zlib_gzip_and_raw() {
        use std::io::Write;
        let raw = b"hello sync";
        // zlib
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(raw).unwrap();
        let zlib_bytes = enc.finish().unwrap();
        assert_eq!(decompress_sync_payload(&zlib_bytes), raw);
        // gzip
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(raw).unwrap();
        let gzip_bytes = enc.finish().unwrap();
        assert_eq!(decompress_sync_payload(&gzip_bytes), raw);
        // raw passthrough
        assert_eq!(decompress_sync_payload(raw), raw);
        // zstd (encode bằng crate zstd đã có)
        let zstd_bytes = zstd::encode_all(std::io::Cursor::new(raw), 1).unwrap();
        assert_eq!(decompress_sync_payload(&zstd_bytes), raw);
    }

    #[test]
    fn sync_v2_session_registry_gates_batches() {
        assert!(!has_sync_v2_session("nope-sync"));
        record_sync_v2_session("sync-1", "sess-aaa");
        assert!(has_sync_v2_session("sync-1"));
        assert!(!has_sync_v2_session("sync-2"));
        // Ghi đè cùng sync_id không trùng lặp.
        record_sync_v2_session("sync-1", "sess-bbb");
        assert!(has_sync_v2_session("sync-1"));
    }

    #[test]
    fn partition_id_mapping_resolves_noised_oneone_and_group_ids() {
        record_sync_v2_partition_map(&[
            ("noised_user_1".into(), "3247153953".into(), false),
            ("noised_grp_9".into(), "9988776655".into(), true),
        ]);
        assert_eq!(
            map_sync_v2_partition_id("oneone/noised_user_1"),
            ("3247153953".into(), Some(false))
        );
        assert_eq!(
            map_sync_v2_partition_id("group/noised_grp_9"),
            ("9988776655".into(), Some(true))
        );
        assert_eq!(
            map_sync_v2_partition_id("noised_grp_9"),
            ("9988776655".into(), Some(true))
        );
        assert_eq!(
            map_sync_v2_partition_id("oneone/unmapped_id"),
            ("unmapped_id".into(), Some(false))
        );
    }

    #[test]
    fn sync_v2_batch_dedup_marks_each_url_once() {
        assert!(mark_sync_v2_batch_seen("s_dedup_1", 2, "https://f.zalo.me/meta"));
        assert!(!mark_sync_v2_batch_seen("s_dedup_1", 2, "https://f.zalo.me/meta"));
        assert!(mark_sync_v2_batch_seen("s_dedup_1", 1, "https://f.zalo.me/msg1"));
        assert!(!mark_sync_v2_batch_seen("s_dedup_1", 1, "https://f.zalo.me/msg1"));
    }

    #[test]
    fn backup_v1_url_dedup_and_reset_work() {
        assert!(!mark_backup_url_seen("   "));
        assert!(mark_backup_url_seen("https://f.zalo.me/v1_backup.db.crypt"));
        assert!(!mark_backup_url_seen("https://f.zalo.me/v1_backup.db.crypt"));
        set_latest_mobile_temp_key("tmp_k");
        set_last_mobile_sync_status(Some("ok".into()));
        reset_for_account_switch();
        assert_eq!(latest_mobile_temp_key(), "");
        assert_eq!(last_mobile_sync_status(), None);
        assert!(mark_backup_url_seen("https://f.zalo.me/v1_backup.db.crypt"));
    }

    #[test]
    fn sync_v2_call_bubbles_and_archive_msg_types_decode_accurately() {
        // 1. Incoming answered voice call with bare "0:18" duration and sendBubbleMessage
        let call_in = decode_sync_v2_proto_message(
            ParsedProtoMsg {
                sender_id: "peer_1".into(),
                global_id: "c1".into(),
                msg_type: 12,
                timestamp: 1_730_450_000_000,
                content: "0:18".into(),
                attaches: vec![ParsedProtoAttach {
                    action: Some("recommened.calltime".into()),
                    title: Some("sendBubbleMessage".into()),
                    description: Some("Cuộc gọi".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            "peer_1",
            "me_99",
            "Tôi",
            "Đỗ Thành",
        );
        assert_eq!(call_in.msg_type, "chat.ecall");
        assert_eq!(call_in.text, "📞 Cuộc gọi thoại đến (18s)");
        assert!(!call_in.raw_json.contains("sendBubbleMessage"));

        // 2. Outgoing cancelled voice call (empty content, reason=4)
        let call_cancel = decode_sync_v2_proto_message(
            ParsedProtoMsg {
                sender_id: "me_99".into(),
                global_id: "c2".into(),
                msg_type: 12,
                timestamp: 1_730_450_010_000,
                content: String::new(),
                attaches: vec![ParsedProtoAttach {
                    action: Some("recommened.misscall".into()),
                    params: Some(r#"{"calltype":0,"duration":0,"reason":4,"isCaller":1}"#.into()),
                    title: Some("sendBubbleMessage".into()),
                    description: Some("Cuộc gọi".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            "peer_1",
            "me_99",
            "Tôi",
            "Đỗ Thành",
        );
        assert_eq!(call_cancel.msg_type, "chat.ecall");
        assert_eq!(call_cancel.text, "📞 Cuộc gọi thoại đi (0s) — Bạn đã hủy");

        // 3. Incoming missed voice call (empty content)
        let call_miss = decode_sync_v2_proto_message(
            ParsedProtoMsg {
                sender_id: "peer_1".into(),
                global_id: "c3".into(),
                msg_type: 12,
                timestamp: 1_730_450_020_000,
                content: String::new(),
                attaches: vec![ParsedProtoAttach {
                    action: Some("recommened.misscall".into()),
                    title: Some("sendBubbleMessage".into()),
                    description: Some("Cuộc gọi".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            "peer_1",
            "me_99",
            "Tôi",
            "Đỗ Thành",
        );
        assert_eq!(call_miss.msg_type, "chat.ecall");
        assert_eq!(call_miss.text, "📞 Cuộc gọi thoại đến (0s) — Bạn bị nhỡ");

        // 4. Archive Photo (msg_type = 3) must NOT be misdecoded as Voice
        let photo = decode_sync_v2_proto_message(
            ParsedProtoMsg {
                sender_id: "peer_1".into(),
                global_id: "p1".into(),
                msg_type: 3,
                timestamp: 1_730_450_030_000,
                content: String::new(),
                attaches: vec![ParsedProtoAttach {
                    href: Some("https://photo-stal-27.zdn.vn/no/test.jpg".into()),
                    thumb: Some("https://photo-stal-27.zdn.vn/no/test_t.jpg".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            "peer_1",
            "me_99",
            "Tôi",
            "Đỗ Thành",
        );
        assert_eq!(photo.msg_type, "chat.photo");
        assert_eq!(photo.text, "[Hình ảnh]");
        assert!(photo.raw_json.contains("Hình ảnh Zalo"));

        // 5. Archive ChatInfo (msg_type = 20) must NOT be misdecoded as Undo
        let sys_pin = decode_sync_v2_proto_message(
            ParsedProtoMsg {
                sender_id: "me_99".into(),
                global_id: "s1".into(),
                msg_type: 20,
                timestamp: 1_730_450_040_000,
                content: String::new(),
                attaches: vec![ParsedProtoAttach {
                    title: Some("Đtc đã ghim tin nhắn hi.".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            "peer_1",
            "me_99",
            "Tôi",
            "Đỗ Thành",
        );
        assert_eq!(sys_pin.msg_type, "chat.system");
        assert_eq!(sys_pin.text, "Đtc đã ghim tin nhắn hi.");
    }

    #[test]
    fn sync_v2_oa_list_action_and_webcontent_preserve_oa_card() {
        let oa_list = decode_sync_v2_proto_message(
            ParsedProtoMsg {
                sender_id: "6091808854554923872".into(),
                global_id: "oa1".into(),
                msg_type: 15,
                timestamp: 1_794_571_926_938,
                content: String::new(),
                attaches: vec![
                    ParsedProtoAttach {
                        attach_type: Some("l.a.header.full".into()),
                        title: Some("⚡Siêu Ưu Đãi: Mua 1 Tặng 1".into()),
                        description: Some("Ưu đãi độc quyền zStyle".into()),
                        thumb: Some("https://cdn.zalo.me/zstyle_banner.jpg".into()),
                        href: Some("https://zalo.me/zstyle/promo".into()),
                        ..Default::default()
                    },
                    ParsedProtoAttach {
                        attach_type: Some("l.a.child.full".into()),
                        title: Some("Gói chủ đề mới".into()),
                        description: Some("Áp dụng hôm nay".into()),
                        thumb: Some("https://cdn.zalo.me/zstyle_item1.jpg".into()),
                        href: Some("https://zalo.me/zstyle/theme".into()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            "6091808854554923872",
            "me_99",
            "Tôi",
            "zStyle – Phong cách Zalo",
        );
        assert_eq!(oa_list.msg_type, "chat.list.action");
        assert!(oa_list.raw_json.contains("oa_card"));
        assert!(oa_list.raw_json.contains("Gói chủ đề mới"));
        assert!(oa_list.raw_json.contains("https://cdn.zalo.me/zstyle_banner.jpg"));

        let webcontent = decode_sync_v2_proto_message(
            ParsedProtoMsg {
                sender_id: "5128600805588494022".into(),
                global_id: "wc1".into(),
                msg_type: 53,
                timestamp: 1_795_306_210_316,
                content: "Chúc một ngày tốt lành, thời tiết TP. Hà Nội hôm nay 22/11: Nhiều mây.".into(),
                attaches: vec![ParsedProtoAttach {
                    thumb: Some("https://cdn.zalo.me/weather_banner.jpg".into()),
                    title: Some("TP. Hà Nội 19° - 25°".into()),
                    params: Some(r#"{"zinstant_msg":{"elements":[{"type":"article","title":"Tin ngày 22/11","thumb":"https://cdn.zalo.me/news1.jpg","href":"https://zalo.me/news1"}],"actions":[{"label":"LỊCH ÂM","href":"https://zalo.me/licham"}]}}"#.into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            "5128600805588494022",
            "me_99",
            "Tôi",
            "Thời Tiết",
        );
        assert_eq!(webcontent.msg_type, "chat.webcontent.v2");
        assert!(webcontent.raw_json.contains("oa_card"));
        assert!(webcontent.raw_json.contains("https://cdn.zalo.me/weather_banner.jpg"));
        assert!(webcontent.raw_json.contains("Tin ngày 22/11"));
        assert!(webcontent.raw_json.contains("LỊCH ÂM"));
    }

    #[test]
    fn sync_v2_out_of_order_last_batch_completes_when_inflight_drains() {
        reset_sync_v2_imported_count();
        assert!(!is_sync_v2_last_seen());
        assert_eq!(sync_v2_inflight_count(), 0);

        let batch_a = SyncV2InflightGuard::enter();
        let batch_b = SyncV2InflightGuard::enter();
        assert_eq!(sync_v2_inflight_count(), 2);

        mark_sync_v2_last_seen();
        drop(batch_a);
        assert!(is_sync_v2_last_seen());
        assert_eq!(sync_v2_inflight_count(), 1);

        drop(batch_b);
        assert!(is_sync_v2_last_seen());
        assert_eq!(sync_v2_inflight_count(), 0);

        reset_sync_v2_imported_count();
        assert!(!is_sync_v2_last_seen());
        assert_eq!(sync_v2_inflight_count(), 0);
    }
}
