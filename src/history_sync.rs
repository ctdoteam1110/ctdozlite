//! Paged history sync for groups, "Cloud của tôi", and mobile message sync (`Đồng bộ tin nhắn từ điện thoại`, #19, #23, #68).
//!
//! - Group & Cloud history walks `/api/cm/getoldv2` backwards 50 messages at a time from the oldest
//!   message already in `zalo_history.db`, so a second run resumes where the first stopped.
//! - Phone message sync (`sync_phone_messages`, #68) triggers Zalo's mobile pull endpoints
//!   (`/api/message/pull_mobile_msg` cmd `12000` & `/api/transfer-sync-v2/request-sync` cmd `12888`),
//!   drains `lp_chat` multi-batch queues (`getChatFromPolling`), batch-queries `/api/preloadconvers/get-last-msgs`
//!   across all 1-1 and group threads in 25-thread chunks, and syncs recent cloud/group history into `zalo_history.db`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::chat::{self, Thread};
use crate::session::Session;
use crate::store::{self, MessageStore};

const DB_PATH: &str = "zalo_history.db";
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

/// Fetch older pages for `t` until the server has nothing older, `MAX_PAGES` is hit, or the user
/// cancels. Each page is committed immediately so a crash or cancel loses nothing. SQLite work
/// runs on the blocking pool; no connection is held across an await.
/// Returns how many messages were new to the database.
pub async fn sync_thread(s: &Session, t: &mut Thread) -> Result<usize> {
    let id = t.id.clone();
    let mut cursor =
        tokio::task::spawn_blocking(move || MessageStore::open(DB_PATH).ok()?.oldest_msg_id(&id))
            .await?
            .unwrap_or_default();
    let mut added = 0;
    for _ in 0..MAX_PAGES {
        if CANCEL.load(Ordering::Relaxed) {
            break;
        }
        let resp = chat::get_cloud_messages(s, &t.id, &cursor, true, PAGE).await?;
        let before = t.messages.len();
        chat::merge_group_history(t, &resp);
        if t.messages.len() == before {
            break; // nothing older on the server
        }
        let snapshot = t.clone();
        added += tokio::task::spawn_blocking(move || {
            store::persist_threads(
                &mut MessageStore::open(DB_PATH)?,
                std::slice::from_ref(&snapshot),
            )
        })
        .await??;
        match next_cursor(t, &cursor) {
            Some(c) => cursor = c,
            None => break,
        }
        tokio::time::sleep(PAUSE).await;
    }
    Ok(added)
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

/// Read the latest status returned by the user's phone during `sync_phone_messages`.
pub fn last_mobile_sync_status() -> Option<String> {
    LAST_MOBILE_SYNC_STATUS.lock().ok().and_then(|g| g.clone())
}

/// Generate a 33-byte (`0x05` curve type prefix + 32-byte key) Base64 public key
/// matching `zprotoEd25519GenerateKeyPair` in `libzproto_wasm_bg.wasm` for Sync v2 (`cmd 590`).
pub fn generate_sync_v2_ed25519_public_key(domain_tag: u8) -> String {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let seed = crate::param_cipher::random_hex(32);
    let mut hasher = Sha256::new();
    hasher.update([domain_tag]);
    hasher.update(seed.as_bytes());
    let digest = hasher.finalize();
    let mut raw = [0u8; 33];
    raw[0] = 0x05; // Signal / ZaloTrustedSynced25519 DJB_TYPE = 0x05
    raw[1..].copy_from_slice(&digest);
    raw[1] &= 0xF8;
    raw[32] = (raw[32] & 0x7F) | 0x40;
    base64::engine::general_purpose::STANDARD.encode(raw)
}

/// Ephemeral RSA-2048 key for one phone-sync run: PKCS#8 DER of the private key (zeroed when
/// dropped or cleared) and the SPKI Base64 public key sent to the server. The private key never
/// leaves this mutex: `decrypt_with_sync_key` uses it in place.
type SyncKeys = (zeroize::Zeroizing<Vec<u8>>, String);
static CURRENT_SYNC_KEYS: std::sync::Mutex<Option<SyncKeys>> = std::sync::Mutex::new(None);

/// Unwrap `ciphertext` with the active sync private key without copying the key out of its slot.
/// Returns the plaintext (zeroed on drop) and the matching public key.
fn decrypt_with_sync_key(ciphertext: &[u8]) -> Result<(zeroize::Zeroizing<Vec<u8>>, String)> {
    let guard = CURRENT_SYNC_KEYS
        .lock()
        .map_err(|_| anyhow::anyhow!("khoá RSA phiên đồng bộ không khả dụng"))?;
    let (pkcs8, pub_key) = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Không tìm thấy khoá RSA phiên đồng bộ hiện tại"))?;
    Ok((rsa_pkcs1_v15_decrypt(pkcs8, ciphertext)?, pub_key.clone()))
}

/// Retrieve the active RSA 2048 public key.
pub fn get_sync_public_key() -> Option<String> {
    CURRENT_SYNC_KEYS
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|(_, pub_k)| pub_k.clone()))
}
/// Clear the active sync keys once sync is finalized; `Zeroizing` wipes the private key bytes.
pub fn clear_sync_keys() {
    if let Ok(mut g) = CURRENT_SYNC_KEYS.lock() {
        *g = None;
    }
}

/// Generate a genuine 2048-bit RSA SubjectPublicKeyInfo (`SPKI`) DER Base64 key
/// (`294 bytes` -> `392 chars` Base64 starting with `MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA...`),
/// matching `sync-message-key-generator` (`main-startup.js:598226`) for `pullMobileMsg` (`cmd 12000`).
/// Keys come from AWS-LC (constant-time RSA, see ADR-0005); the wire format is unchanged.
pub fn generate_sync_public_key() -> String {
    match generate_rsa_sync_keypair() {
        Some((pkcs8, b64)) => {
            if let Ok(mut g) = CURRENT_SYNC_KEYS.lock() {
                *g = Some((pkcs8, b64.clone()));
            }
            b64
        }
        None => String::new(),
    }
}

/// New RSA-2048 key pair as (PKCS#8 DER private key, SPKI DER Base64 public key). aws-lc-rs zeroes
/// its own DER buffer on drop; our copy is held in `Zeroizing` from the start.
fn generate_rsa_sync_keypair() -> Option<(zeroize::Zeroizing<Vec<u8>>, String)> {
    use aws_lc_rs::encoding::AsDer;
    use aws_lc_rs::rsa::{KeySize, PrivateDecryptingKey};
    use base64::Engine;
    let key = PrivateDecryptingKey::generate(KeySize::Rsa2048).ok()?;
    let spki = key.public_key().as_der().ok()?;
    let pkcs8 = key.as_der().ok()?;
    Some((
        zeroize::Zeroizing::new(pkcs8.as_ref().to_vec()),
        base64::engine::general_purpose::STANDARD.encode(spki.as_ref()),
    ))
}

/// RSAES-PKCS1-v1_5 decryption of the backup key the phone encrypted to our sync public key.
/// Same padding as before (`rsa::Pkcs1v15Encrypt`); AWS-LC performs it in constant time with
/// blinding, which closes RUSTSEC-2023-0071 (Marvin) for this path.
fn rsa_pkcs1_v15_decrypt(pkcs8: &[u8], ciphertext: &[u8]) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    use aws_lc_rs::rsa::{Pkcs1PrivateDecryptingKey, PrivateDecryptingKey};
    let key = PrivateDecryptingKey::from_pkcs8(pkcs8)
        .map_err(|_| anyhow::anyhow!("khoá RSA phiên đồng bộ không hợp lệ"))?;
    let key = Pkcs1PrivateDecryptingKey::new(key)
        .map_err(|_| anyhow::anyhow!("khoá RSA phiên đồng bộ không hợp lệ"))?;
    let mut out = zeroize::Zeroizing::new(vec![0u8; key.min_output_size()]);
    let n = key
        .decrypt(ciphertext, &mut out)
        .map_err(|_| anyhow::anyhow!("không giải mã được khoá bản sao lưu"))?
        .len();
    out.truncate(n);
    Ok(out)
}
/// Synchronize 1-1 and group messages from the user's phone and Zalo cloud (`Đồng bộ tin nhắn từ điện thoại`, #68).
///
/// Workflow:
/// 1. Sends mobile pull signal (`request_phone_sync` `cmd 12000` + `request_phone_sync_v2` `cmd 12888`)
///    so the phone Zalo app is notified to push recent 1-1 & group messages.
/// 2. Drains `lp_chat` (`get_chat_from_polling`) across multiple batches to collect queued 1-1 & group messages.
/// 3. Queries `/api/preloadconvers/get-last-msgs` (`get_last_msgs`) in 25-thread batches across all
///    1-1 and group conversations and merges messages into `threads`.
/// 4. Fetches recent & older pages via `/api/cm/getrecentv2` and `/api/cm/getoldv2` for Cloud & groups.
/// 5. Persists all merged messages to `zalo_history.db` and `threads_cache.json`.
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

/// Synchronize messages with explicit control over whether to send a wake-up/pull prompt
/// (`send_phone_prompt`) to the user's phone or only drain server/cloud queues.
pub async fn sync_phone_messages_opts<F>(
    s: &Session,
    poll_state: &tokio::sync::Mutex<chat::PollState>,
    threads: &mut Vec<Thread>,
    send_phone_prompt: bool,
    mut on_progress: F,
) -> Result<(usize, usize)>
where
    F: FnMut(f32, String),
{
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let pub_key = generate_sync_public_key();
    let before_total: usize = threads.iter().map(|t| t.messages.len()).sum();

    if send_phone_prompt {
        set_last_mobile_sync_status(None);

        // Phase 1: Send mobile wake-up / pull request to phone (`cmd 12000` + WebSocket `cmd 592` & `cmd 590`)
        on_progress(
            0.10,
            "📲 Đang gửi yêu cầu đồng bộ tới Zalo trên điện thoại (hãy mở Zalo trên điện thoại và bấm 'Đồng bộ ngay')…".to_string(),
        );
        let _ = chat::request_phone_sync(s, &pub_key).await;
        let _ = chat::request_phone_sync_v2(s, &pub_key).await;

        // Wait for user to confirm on phone (up to 90 seconds)
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        let mut confirmed = false;
        while std::time::Instant::now() < deadline {
            if PHONE_SYNC_CANCEL.load(Ordering::Relaxed) {
                let _ = chat::cancel_phone_sync(s, &pub_key).await;
                return Ok((0, 0));
            }
            if let Some(ref st) = last_mobile_sync_status() {
                if st.contains("đã xác nhận")
                    || st.contains("đang nén")
                    || st.contains("giải mã")
                    || st.contains("Đã nhận gói đồng bộ")
                    || st.contains("thành công")
                {
                    confirmed = true;
                    on_progress(0.40, st.clone());
                    break;
                } else if st.contains("từ chối") {
                    on_progress(1.0, st.clone());
                    return Ok((0, 0));
                } else {
                    on_progress(0.20, st.clone());
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        if confirmed {
            let finish_deadline = std::time::Instant::now() + Duration::from_secs(45);
            while std::time::Instant::now() < finish_deadline {
                if let Some(ref st) = last_mobile_sync_status() {
                    if st.contains("thành công") || st.contains("Không thể") || st.contains("thất bại") {
                        on_progress(1.0, st.clone());
                        break;
                    } else {
                        on_progress(0.70, st.clone());
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
    // Phase 2: Drain `lp_chat` multi-batch action queues (`getChatFromPolling`)
    on_progress(
        0.22,
        "📥 Đang nhận hàng đợi tin nhắn 1-1 & nhóm từ điện thoại…".to_string(),
    );
    for batch_idx in 0..5 {
        if PHONE_SYNC_CANCEL.load(Ordering::Relaxed) {
            let _ = chat::cancel_phone_sync(s, &pub_key).await;
            return Ok((0, 0));
        }
        let lp_res = {
            let mut ps = poll_state.lock().await;
            chat::get_chat_from_polling(s, &mut ps).await
        };
        let Ok(lp_resp) = lp_res else {
            break;
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
    if let Ok(preview_resp) = chat::get_last_msgs(s, &[]).await {
        chat::merge_previews(threads, &preview_resp);
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
        if let Ok(preview_resp) = chat::get_last_msgs(s, chunk).await {
            chat::merge_previews(threads, &preview_resp);
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
        if let Ok(cm_resp) = chat::get_cloud_messages(s, &tid, "0", false, PAGE).await {
            chat::merge_group_history(&mut threads[tidx], &cm_resp);
        }
        if let Some(oldest) = next_cursor(&threads[tidx], "") {
            if let Ok(old_resp) = chat::get_cloud_messages(s, &tid, &oldest, true, PAGE).await {
                chat::merge_group_history(&mut threads[tidx], &old_resp);
            }
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    // Phase 5: Resolve contact/group display names, deduplicate, and persist to SQLite (`zalo_history.db`)
    on_progress(
        0.96,
        "💾 Đang lưu tin nhắn đã đồng bộ vào cơ sở dữ liệu zalo_history.db…".to_string(),
    );
    chat::resolve_thread_names(s, threads).await;
    chat::deduplicate_threads(threads, None);

    let snapshot = threads.clone();
    let uin_str = s.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
    let db_path = store::db_path_for_account(&uin_str);
    let db_path_clone = db_path.clone();
    let db_added = tokio::task::spawn_blocking(move || -> Result<usize> {
        let mut st = MessageStore::open(&db_path_clone)?;
        let n = store::persist_threads(&mut st, &snapshot)?;
        chat::save_threads_cache(&snapshot);
        Ok(n)
    })
    .await??;
    let after_total: usize = threads.iter().map(|t| t.messages.len()).sum();
    let added = db_added.max(after_total.saturating_sub(before_total));
    let active_threads = threads.iter().filter(|t| !t.messages.is_empty()).count();
    let final_msg = if let Some(phone_note) = last_mobile_sync_status() {
        format!(
            "{phone_note} · Đã đồng bộ Cloud/Nhóm: +{added} tin nhắn ({active_threads} hội thoại)"
        )
    } else {
        format!("✅ Đã gửi yêu cầu tới điện thoại & đồng bộ Cloud/Nhóm: +{added} tin nhắn ({active_threads} hội thoại). Hãy mở Zalo trên điện thoại để xác nhận nếu muốn kéo tin nhắn 1-1 cũ.")
    };
    on_progress(1.0, final_msg);
    Ok((added, active_threads))
}

/// Download, decrypt, decompress and ingest a mobile backup container (.db.crypt)
/// into `zalo_history.db` and hydrate `threads_cache.json`.
pub async fn import_mobile_backup_container(
    s: &Session,
    url: &str,
    encrypted_key: &str,
) -> Result<usize> {
    use crate::store::StoredMessage;
    use aes::cipher::{BlockDecryptMut, KeyIvInit};
    use base64::Engine;
    use rusqlite::Connection;
    use serde_json::json;
    use std::collections::HashMap;
    use std::io::Cursor;

    if url.is_empty() {
        anyhow::bail!("empty backup url");
    }

    if encrypted_key.is_empty() {
        anyhow::bail!("encrypted_key is empty");
    }
    let enc_bytes = base64::engine::general_purpose::STANDARD.decode(encrypted_key)?;
    let (raw_key, pub_key_b64) = decrypt_with_sync_key(&enc_bytes)?;

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
        let name = String::from_utf8_lossy(&decrypted[offset..offset + name_len]).to_string();
        offset += name_len;
        if offset + 4 > decrypted.len() {
            break;
        }
        let size = u32::from_be_bytes(decrypted[offset..offset + 4].try_into()?) as usize;
        offset += 4;

        let stem = name.trim_end_matches(".db");
        if let Some(rest) = stem.strip_prefix("group_") {
            if let Ok(gid) = rest.parse::<i64>() {
                gids.push(gid);
            }
        } else if let Ok(fid) = stem.parse::<i64>() {
            fids.push(fid);
        }

        toc.push((name, size));
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

    let noise_body = json!({
        "fids": fids,
        "gids": gids
    });
    let enc_params = s.encoded_params(&noise_body)?;
    let znoise_url = format!("https://zwid.api.zalo.me/api/znoise?{}", s.common_params());
    let resp = s
        .post_form(&znoise_url, format!("params={enc_params}"))
        .await?;
    let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;

    let mut id_map: HashMap<String, String> = HashMap::new();
    if let Some(data) = dec.get("data") {
        if let Some(arr) = data.get("fids").and_then(|v| v.as_array()) {
            for (fid, thread_id_val) in fids.iter().zip(arr.iter()) {
                if let Some(tid) = thread_id_val.as_str() {
                    id_map.insert(format!("{fid}.db"), tid.to_string());
                }
            }
        }
        if let Some(arr) = data.get("gids").and_then(|v| v.as_array()) {
            for (gid, thread_id_val) in gids.iter().zip(arr.iter()) {
                if let Some(tid) = thread_id_val.as_str() {
                    id_map.insert(format!("group_{gid}.db"), tid.to_string());
                }
            }
        }
    }

    let mut store = crate::store::MessageStore::open("zalo_history.db")?;
    let temp_dir = std::env::temp_dir().join(format!(
        "ctdozlite_sync_{}",
        crate::param_cipher::random_hex(8)
    ));
    let _ = std::fs::create_dir_all(&temp_dir);

    let my_uid = s.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
    let my_name = s
        .login
        .as_ref()
        .map(|l| l.display_name.clone())
        .unwrap_or_else(|| "Tôi".into());

    // Pass 1: write every per-thread SQLite file out of the container.
    let mut curr_offset = 0;
    let mut extracted: Vec<(std::path::PathBuf, String)> = Vec::new();
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
        extracted.push((temp_db_path, thread_id.clone()));
    }

    // The phone stores the owner's numeric mobile id as `SenderId`, which is not the web
    // `uin`. The owner is the one sender present in (nearly) every conversation.
    let my_sender_id = infer_backup_owner_sender_id(extracted.iter().map(|(p, _)| p.as_path()));

    let mut total_imported = 0;
    // Per phone `MsgType`: (rows, empty content, duration-like content). Numbers only — used
    // to map call/other types from real backups without recording message content.
    let mut type_stats: std::collections::BTreeMap<i64, (usize, usize, usize)> =
        std::collections::BTreeMap::new();

    // Pass 2: import messages.
    for (temp_db_path, thread_id) in &extracted {
        let thread_id = thread_id.clone();
        let Ok(conn) = Connection::open(temp_db_path) else {
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
            let (
                sender_id_num,
                cli_msg_id,
                glb_msg_id,
                status_num,
                msg_type_num,
                ts,
                content_opt,
                bin_net_opt,
            ) = r;
            {
                let e = type_stats
                    .entry(msg_type_num)
                    .or_insert((0usize, 0usize, 0usize));
                e.0 += 1;
                let c = content_opt.as_deref().unwrap_or("").trim();
                if c.is_empty() {
                    e.1 += 1;
                } else if looks_like_duration(c) {
                    e.2 += 1;
                }
            }
            let is_outgoing = my_sender_id == Some(sender_id_num);
            let sender_id = if is_outgoing {
                my_uid.clone()
            } else {
                thread_id.clone()
            };
            let sender_name = if is_outgoing {
                my_name.clone()
            } else {
                String::new()
            };

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
                // Keep the phone's own type/status so later builds can render calls and other
                // kinds this importer does not decode yet (nested: no clash with sticker keys).
                raw_json: serde_json::json!({
                    "phone_backup": { "type": msg_type_num, "status": status_num }
                })
                .to_string(),
            };

            if store.upsert_message(&sm).unwrap_or(false) {
                total_imported += 1;
            } else if my_sender_id.is_some() {
                // Repair rows stored by earlier imports that misattributed the owner's messages.
                let _ = store.set_message_direction(
                    &sm.msg_id,
                    sm.is_outgoing,
                    &sm.sender_id,
                    &sm.sender_name,
                );
            }
        }
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
    // Local diagnostics next to zalo_history.db: counts per phone message type only.
    let stats = serde_json::json!({
        "owner_detected": my_sender_id.is_some(),
        "types": type_stats
            .iter()
            .map(|(t, (n, empty, dur))| serde_json::json!({"type": t, "rows": n, "empty": empty, "duration_like": dur}))
            .collect::<Vec<_>>(),
    });
    let _ = std::fs::write("phone_sync_types.json", stats.to_string());

    let mut cached_threads = crate::chat::load_threads_cache();
    let _ = store.hydrate_threads(&mut cached_threads, 50);
    crate::chat::save_threads_cache(&cached_threads);

    // Notify Zalo server and phone that snapshot is downloaded, dismissing mobile spinner
    let _ = crate::chat::clean_sync(s, &pub_key_b64).await;
    clear_sync_keys();

    Ok(total_imported)
}

/// `m:ss` / `mm:ss` / `h:mm:ss` — how the phone backup stores call durations as content.
fn looks_like_duration(text: &str) -> bool {
    let parts: Vec<&str> = text.split(':').collect();
    (2..=3).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Pick the backup owner's mobile `SenderId`: the sender found in the most per-thread
/// databases. Requires presence in at least two threads and a strict lead over the runner-up,
/// otherwise returns `None` (messages are then imported as incoming rather than guessed).
fn infer_backup_owner_sender_id<'a>(dbs: impl Iterator<Item = &'a std::path::Path>) -> Option<i64> {
    let mut threads_per_sender: std::collections::HashMap<i64, usize> =
        std::collections::HashMap::new();
    // One-to-one conversations (exactly two senders) identify the owner unambiguously: the
    // owner is in every one of them, each partner in only one.
    let mut pairs_per_sender: std::collections::HashMap<i64, usize> =
        std::collections::HashMap::new();
    for path in dbs {
        let Ok(conn) =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        else {
            continue;
        };
        let Ok(mut stmt) = conn.prepare("SELECT DISTINCT SenderId FROM ChatContent") else {
            continue;
        };
        let Ok(rows) = stmt.query_map([], |row| row.get::<_, i64>(0)) else {
            continue;
        };
        // No `> 0` filter: the phone may store the owner's own rows with SenderId 0, and
        // dropping it made a frequent group member win (their messages became "mine" and the
        // owner's became incoming).
        let senders: std::collections::HashSet<i64> = rows.flatten().collect();
        if senders.len() == 2 {
            for sid in &senders {
                *pairs_per_sender.entry(*sid).or_default() += 1;
            }
        }
        for sid in senders {
            *threads_per_sender.entry(sid).or_default() += 1;
        }
    }
    let mut pairs: Vec<(i64, usize)> = pairs_per_sender.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    match pairs.as_slice() {
        [(sid, n), (_, second), ..] if *n >= 2 && n > second => return Some(*sid),
        [(sid, n)] if *n >= 2 => return Some(*sid),
        _ => {}
    }
    let mut ranked: Vec<(i64, usize)> = threads_per_sender.into_iter().collect();
    ranked.sort_by_key(|r| std::cmp::Reverse(r.1));
    match ranked.as_slice() {
        [(sid, n), (_, second), ..] if *n >= 2 && n > second => Some(*sid),
        [(sid, n)] if *n >= 2 => Some(*sid),
        _ => None,
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct SyncV2Keys {
    pub ik_pub: String,
    pub ik_priv: String,
    pub ek_pub: String,
    pub ek_priv: String,
}

/// Locate a Node.js binary. Apps launched from Finder get a minimal `PATH`
/// (`/usr/bin:/bin:...`), so the usual install locations are probed explicitly.
fn find_node() -> Option<std::path::PathBuf> {
    let from_path = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("node"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let fixed = [
        "/opt/homebrew/bin/node",
        "/usr/local/bin/node",
        "/usr/bin/node",
    ]
    .map(std::path::PathBuf::from);
    from_path.into_iter().chain(fixed).find(|p| p.is_file())
}

/// Locate `sync_v2_helper.js`: next to the binary, in the macOS bundle's `Resources`,
/// in the working directory, or in the source tree for `cargo run` builds.
fn find_sync_v2_helper() -> Option<std::path::PathBuf> {
    const NAME: &str = "sync_v2_helper.js";
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()))
    {
        candidates.push(dir.join(NAME));
        candidates.push(dir.join("../Resources").join(NAME));
    }
    candidates.push(std::path::PathBuf::from(NAME));
    candidates.push(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(NAME));
    candidates.into_iter().find(|p| p.is_file())
}

/// Run `node sync_v2_helper.js <args>` from the data directory (the process cwd), where the
/// helper keeps `sync_v2_keys.json` / `sync_v2_session.bin` between calls.
fn run_sync_v2_helper(args: &[&std::ffi::OsStr]) -> Result<std::process::Output> {
    let node = find_node().context("không tìm thấy Node.js (cần để đồng bộ v2 với điện thoại)")?;
    let script = find_sync_v2_helper().context("không tìm thấy sync_v2_helper.js")?;
    Ok(std::process::Command::new(node)
        .arg(script)
        .args(args)
        .output()?)
}

pub fn generate_sync_v2_keys() -> Result<SyncV2Keys> {
    let out = run_sync_v2_helper(&["gen-keys".as_ref()])?;
    if !out.status.success() {
        anyhow::bail!(
            "node sync_v2_helper.js gen-keys failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let keys: SyncV2Keys = serde_json::from_slice(&out.stdout)?;
    Ok(keys)
}

pub async fn import_sync_v2_meta(s: &Session, url: &str) -> Result<()> {
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
    let temp_meta =
        std::env::temp_dir().join(format!("meta_{}.bin", crate::param_cipher::random_hex(6)));
    std::fs::write(&temp_meta, &bytes)?;

    let out = run_sync_v2_helper(&["decrypt-meta".as_ref(), temp_meta.as_os_str()])?;

    let _ = std::fs::remove_file(&temp_meta);

    if !out.status.success() {
        anyhow::bail!(
            "decrypt-meta failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
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
}

#[derive(Debug, Default)]
pub struct ParsedProtoPartition {
    pub id: String,
    pub messages: Vec<ParsedProtoMsg>,
}

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

pub async fn import_sync_v2_messages(s: &Session, url: &str) -> Result<usize> {
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
    let temp_msg =
        std::env::temp_dir().join(format!("msg_{}.bin", crate::param_cipher::random_hex(6)));
    std::fs::write(&temp_msg, &bytes)?;

    let out = run_sync_v2_helper(&["decrypt-message".as_ref(), temp_msg.as_os_str()])?;

    let _ = std::fs::remove_file(&temp_msg);

    if !out.status.success() {
        anyhow::bail!(
            "decrypt-message failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let decrypted_bytes = std::fs::read("sync_v2_message.bin")?;
    let decompressed = match zstd::decode_all(std::io::Cursor::new(&decrypted_bytes)) {
        Ok(d) => d,
        Err(_) => decrypted_bytes,
    };

    let partitions = parse_sync_chunk(&decompressed).unwrap_or_default();
    let db_path = crate::store::db_path_for_account(&uin);
    let mut store = crate::store::MessageStore::open(&db_path)?;
    let my_name = s
        .login
        .as_ref()
        .map(|l| l.display_name.clone())
        .unwrap_or_else(|| "Tôi".into());

    let mut imported = 0;
    for part in partitions {
        let thread_id = part.id.clone();
        for m in part.messages {
            let is_outgoing = m.sender_id == uin;
            let sender_name = if is_outgoing {
                my_name.clone()
            } else {
                String::new()
            };
            let msg_id = if !m.global_id.is_empty() && m.global_id != "0" {
                m.global_id
            } else {
                m.client_id.to_string()
            };

            let sm = crate::store::StoredMessage {
                msg_id,
                thread_id: thread_id.clone(),
                sender_id: if is_outgoing {
                    uin.clone()
                } else {
                    thread_id.clone()
                },
                sender_name,
                text: m.content,
                msg_type: "webchat".into(),
                ts: m.timestamp as i64,
                is_outgoing,
                raw_json: String::new(),
            };

            if store.upsert_message(&sm).unwrap_or(false) {
                imported += 1;
            }
        }
    }

    let mut cached_threads = crate::chat::load_threads_cache();
    let _ = store.hydrate_threads(&mut cached_threads, 50);
    crate::chat::save_threads_cache(&cached_threads);

    if let Some(pub_k) = get_sync_public_key() {
        let _ = crate::chat::clean_sync(s, &pub_k).await;
    }

    let _ = std::fs::remove_file("sync_v2_message.bin");
    let _ = std::fs::remove_file("sync_v2_session.bin");
    let _ = std::fs::remove_file("sync_v2_keys.json");

    Ok(imported)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatMessage;

    fn backup_thread_db(dir: &std::path::Path, name: &str, senders: &[i64]) -> std::path::PathBuf {
        let path = dir.join(name);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("CREATE TABLE ChatContent (SenderId INTEGER)", [])
            .unwrap();
        for s in senders {
            conn.execute("INSERT INTO ChatContent VALUES (?1)", [s])
                .unwrap();
        }
        path
    }

    #[test]
    fn backup_owner_is_sender_present_in_most_threads() {
        let dir =
            std::env::temp_dir().join(format!("owner_test_{}", crate::param_cipher::random_hex(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let dbs = [
            backup_thread_db(&dir, "a.db", &[777, 101, 777]),
            backup_thread_db(&dir, "b.db", &[202, 777]),
            backup_thread_db(&dir, "c.db", &[303]),
        ];
        assert_eq!(
            infer_backup_owner_sender_id(dbs.iter().map(|p| p.as_path())),
            Some(777)
        );
        // Owner rows stored with SenderId 0 while one member shows up in several groups.
        let zero = [
            backup_thread_db(&dir, "z1.db", &[0, 11]),
            backup_thread_db(&dir, "z2.db", &[0, 12]),
            backup_thread_db(&dir, "z3.db", &[0, 13]),
            backup_thread_db(&dir, "g1.db", &[0, 99, 21, 22]),
            backup_thread_db(&dir, "g2.db", &[0, 99, 23, 24]),
            backup_thread_db(&dir, "g3.db", &[99, 25, 26]),
        ];
        assert_eq!(
            infer_backup_owner_sender_id(zero.iter().map(|p| p.as_path())),
            Some(0)
        );
        assert!(
            looks_like_duration("0:43")
                && looks_like_duration("31:55")
                && looks_like_duration("1:02:03")
        );
        assert!(
            !looks_like_duration("25 triệu")
                && !looks_like_duration("a:b")
                && !looks_like_duration("12")
        );

        // A tie or a single thread is not enough evidence.
        let tie = [
            backup_thread_db(&dir, "d.db", &[1, 2]),
            backup_thread_db(&dir, "e.db", &[1, 2]),
        ];
        assert_eq!(
            infer_backup_owner_sender_id(tie.iter().map(|p| p.as_path())),
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn cursor_stops_when_page_brought_nothing_older() {
        assert_eq!(next_cursor(&thread_with(&["5", "9"]), "5"), None);
        assert_eq!(next_cursor(&thread_with(&[]), "5"), None);
        assert_eq!(next_cursor(&thread_with(&["", "out_2"]), "5"), None);
    }

    /// Known-answer test for the RSA swap (ADR-0005): a PKCS#1 v1.5 ciphertext produced by
    /// OpenSSL/LibreSSL (`pkeyutl -encrypt -pkeyopt rsa_padding_mode:pkcs1`) must decrypt to the
    /// exact plaintext. The key below is a throwaway generated for this test only.
    #[test]
    fn rsa_pkcs1_v15_decrypt_matches_openssl_known_answer() {
        use base64::Engine;
        const TEST_ONLY_PKCS8: &str = "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC33TjF3BlOqjhMJvUqYn1+lJAifzo6qhOTnwNlYyUzBtTSkb9WLToqL9IqsCsMcQKqX6fayLXDnpcJcRaGrumBAlxwxU2HA/hS7280W25z56cvl403fQ26LbcZhjKR/3oXQIqX6sWUcuUKpVhWVXAKPdTm0IruxY/OEC6jX2omLwRvg03yFPGjENbGKVwo1HT7nK0WxwkxjjSO/0QhAzdHJBvvy8u7s2H4yL0gd4HglMgnAIz/bZJtQtqnhJJcy2StnIjovdEMSXI2UvHoRgtwTIIBer2v9tkygJKgiwwXgTex1nUPNaxNKM72xn/LXSl/Lom3gXEjmFhzPCnM/l4LAgMBAAECggEBAJ7T7igVcpMevhUijiR4AhpncX8rUny80L5z3XYmyEqk+xGPL67c0OMCflW76uQLd5b/nLuFKGb8x1Izq5ba4AE+reYIumkIh+n0xaZ/2REsqqKVEzs8vq/2r7XP1D2EaM5PRann95e+DJc+r+mcUnW2ZgShX3LVnN0P44njcYnbGu8iVSkG/P4v4kCQeQcHVQGhzTPSEfOuC/YWyyfJvbLCPlmpRNHmjG42h4+6+aTd4/RfF1nFUY/DEv6D216RHuNYo11tuY3p2MMMboXPdGC1qboZqA+fSDxL1ZROx1Gi6hO+2k9PyRoBFAFNEudHtDAwV7p9uaCnhJ9G5Y9e2xECgYEA2f7x6/Ht943IP6trmD9CKNcnpYzGAIvD6BIfUuk03ADPQgcGOyrlHqEUudZqLmXoVsC8wbGgg99LkjWN+YkJD81eIYBsnE5bwRgjQ3PzRWn5CmFhxRi24YqFbyZUtffW2iDY+xsFoAIPbmBOaSQO5YgZtvvHZ3fin7J61SfHhakCgYEA1+r+UJSbRZNyKbLo85kwIp4hK8ePUTwZY5JWQw/yG/coQWTguKNieOo7YeHgwHHhbDYtrJP5Sxt7O+c3syEb6K6hnKKDmA3dhwYUN5p5lsPpiyuUSbHkO05fx3pkOgXFmQyjWqStnHCaa2gzSnpPTayaTm+bZYqDgkmIPMzObpMCgYB+Ck3N4b95FlzlkzNOf6S9AElICrCguC65C2FZCsUpPr6WqoELzK1UnEXjfPk/gmRnZGC+jPkFxiFnk9dWo8i9fjwimqIxbYzXhCE/atcJF1CKk+t3jeaPWQv1S/BGldl/nOZXqTQDY62QX9Z2LAcA50QW7FRKmqJJO2SRwcI2oQKBgQDB8j+GOjZPBOajNiWxyoRGVCwPmy3NFmLn4ebCTmx3Wkc6llaVA43MdvnoLnEn7CNEhFX6kuB26rSDVmnd6XucQmDAxenWh33Zb0hxubxGBKyfagHFQJl1/CxkjjfBNrYLiCKjpRCdWTgcCnkoCT160/O3SOEZEJaYTVym4ID9KQKBgBOjKF6buXYCr9D9UejL8t0KUtsZKWMFrVKaqDWO/nVUB66rqzlapeIMM219fxssStobZpf7sCrFOF3DVXUvYS0uNOi9LHAd8PXVMhPIOgWkWGKZrjtXHLNkf55vJaPv5YEoCa2wacoe66VdmhlkBXtZ4Ujldofz/tcbrot88bRb";
        const CIPHERTEXT: &str = "Ph/sL5YFrDAnYTb0xk+9Ti3BOKreP9/KP31AcJpYiStGQRfcS1MYxfG4audb48SDc7e2hs0kHVxgKyabduooxEZXljJNTvlTdkW5r7CTwPPFlkOnJi7gx7QTwjzLjgd8uGIQk5N6FLS2s4vlsaGRbAqb1G0IkOgHnwhRCUdIadUzJ+xMWqT2bLDQxC5g9muXVdAszhObMcmvbk2B4eC1lSze0+0eR4LOtXcw8ePYzxsX9DMpXBFz7upnN+C9pcbEq6EVkU7z+qGVIApTeJR48ZmU85Brbw9fUgjkJV/85Yms8aYN3pCVAUq8Nu7zTPk0yUJJ4K3MUX490fTT9oS5EQ==";
        let b64 = base64::engine::general_purpose::STANDARD;
        let key = b64.decode(TEST_ONLY_PKCS8).unwrap();
        let ct = b64.decode(CIPHERTEXT).unwrap();
        let pt = rsa_pkcs1_v15_decrypt(&key, &ct).expect("decrypts");
        assert_eq!(pt.as_slice(), b"ctdozlite-kat:32-byte-aes-key!!!");
        // Tampered ciphertext must be rejected, not yield garbage.
        let mut bad = ct.clone();
        bad[0] ^= 0x01;
        assert!(rsa_pkcs1_v15_decrypt(&key, &bad).map(|p| *p != *pt).unwrap_or(true));
    }

    #[test]
    fn sync_keypair_round_trips_through_public_key() {
        use aws_lc_rs::rsa::{Pkcs1PublicEncryptingKey, PublicEncryptingKey};
        use base64::Engine;
        let (pkcs8, spki_b64) = generate_rsa_sync_keypair().unwrap();
        let spki = base64::engine::general_purpose::STANDARD.decode(spki_b64).unwrap();
        let pub_key = Pkcs1PublicEncryptingKey::new(PublicEncryptingKey::from_der(&spki).unwrap()).unwrap();
        let msg = b"0123456789abcdef0123456789abcdef";
        let mut ct = vec![0u8; pub_key.ciphertext_size()];
        let ct = pub_key.encrypt(msg, &mut ct).unwrap().to_vec();
        assert_eq!(rsa_pkcs1_v15_decrypt(&pkcs8, &ct).unwrap().as_slice(), msg);
    }

    /// Tests that touch the process-wide sync key slot run one at a time.
    static SYNC_KEY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn stored_sync_key_decrypts_in_place_and_clear_removes_it() {
        let _serial = SYNC_KEY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use aws_lc_rs::rsa::{Pkcs1PublicEncryptingKey, PublicEncryptingKey};
        use base64::Engine;
        let spki_b64 = generate_sync_public_key();
        let spki = base64::engine::general_purpose::STANDARD.decode(&spki_b64).unwrap();
        let pub_key = Pkcs1PublicEncryptingKey::new(PublicEncryptingKey::from_der(&spki).unwrap()).unwrap();
        let mut buf = vec![0u8; pub_key.ciphertext_size()];
        let ct = pub_key.encrypt(b"backup-key", &mut buf).unwrap().to_vec();
        let (pt, pk) = decrypt_with_sync_key(&ct).unwrap();
        assert_eq!(pt.as_slice(), b"backup-key");
        assert_eq!(pk, spki_b64);
        clear_sync_keys();
        assert!(decrypt_with_sync_key(&ct).is_err());
        assert!(get_sync_public_key().is_none());
    }

    #[test]
    fn generate_sync_public_key_produces_valid_rsa2048_spki_der_base64() {
        let _serial = SYNC_KEY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use base64::Engine;
        let k1 = generate_sync_public_key();
        let k2 = generate_sync_public_key();
        assert_ne!(k1, k2);
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
    fn generate_sync_v2_ed25519_public_key_produces_33_byte_djb_curve_key() {
        use base64::Engine;
        let ik = generate_sync_v2_ed25519_public_key(0xA1);
        let ek = generate_sync_v2_ed25519_public_key(0xB2);
        assert_ne!(ik, ek);
        assert!(
            ik.starts_with("B"),
            "DJB 0x05 prefix must base64-encode to 'B': {ik}"
        );
        let raw = base64::engine::general_purpose::STANDARD
            .decode(&ik)
            .expect("valid base64");
        assert_eq!(raw.len(), 33);
        assert_eq!(raw[0], 0x05);
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
}
