//! Verification of encrypted Zalo Desktop V2 backup headers.
//!
//! The cipher parameters match the installed Zalo Desktop 26.10.20 backup code.

use aes::Aes256;
use anyhow::{bail, Result};
use cipher::{
    block_padding::{NoPadding, Pkcs7},
    BlockDecryptMut, KeyIvInit,
};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::{Read, Write};

/// Verify a V2 backup's first encrypted TAR header using an account-specific UIN.
///
/// This is only a preliminary format check: CBC does not authenticate the archive.
/// The caller must validate the remaining stream and every archive entry.
///
/// # Errors
/// Returns an error for a malformed UIN, truncated input, or an invalid header.
pub fn verify_v2_header(ciphertext: &[u8], uin: &str) -> Result<()> {
    if uin.len() != 32 || !uin.is_ascii() {
        bail!("Giá trị tài khoản không hợp lệ");
    }
    if ciphertext.len() < 512 {
        bail!("Tệp sao lưu quá ngắn hoặc bị hỏng");
    }
    let key = Sha256::digest(uin.as_bytes());
    let mut iv = [0u8; 16];
    iv[..3].copy_from_slice(b"zie");
    iv[3..].copy_from_slice(&uin.as_bytes()[..13]);
    let mut header = [0u8; 512];
    header.copy_from_slice(&ciphertext[..512]);
    cbc::Decryptor::<Aes256>::new_from_slices(&key, &iv)
        .map_err(|_| anyhow::anyhow!("Không thể khởi tạo giải mã"))?
        .decrypt_padded_mut::<NoPadding>(&mut header)
        .map_err(|_| anyhow::anyhow!("Không thể đọc bản sao lưu"))?;

    if &header[257..262] != b"ustar" {
        bail!("Giá trị tài khoản không đúng hoặc bản sao lưu không được hỗ trợ");
    }
    let checksum = std::str::from_utf8(&header[148..156])?.trim_matches(|c| c == '\0' || c == ' ');
    let expected = u64::from_str_radix(checksum, 8)
        .map_err(|_| anyhow::anyhow!("Phần đầu bản sao lưu bị hỏng"))?;
    let actual: u64 = header
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            if (148..156).contains(&i) {
                u64::from(b' ')
            } else {
                u64::from(b)
            }
        })
        .sum();
    if expected != actual {
        bail!("Phần đầu bản sao lưu bị hỏng");
    }
    Ok(())
}

/// Decrypt a V2 backup in bounded chunks to a caller-owned staging destination.
///
/// Output may be incomplete if an error occurs. The caller must discard it on failure
/// and validate the entire TAR before importing any contents. CBC is not authenticated.
///
/// # Errors
/// Returns an error for an invalid header, truncated input, bad padding, or I/O failure.
pub fn decrypt_v2<R: Read, W: Write>(mut input: R, mut output: W, uin: &str) -> Result<()> {
    let mut first = [0u8; 512];
    input
        .read_exact(&mut first)
        .map_err(|_| anyhow::anyhow!("Tệp sao lưu quá ngắn hoặc bị hỏng"))?;
    verify_v2_header(&first, uin)?;

    let key = Sha256::digest(uin.as_bytes());
    let mut iv = [0u8; 16];
    iv[..3].copy_from_slice(b"zie");
    iv[3..].copy_from_slice(&uin.as_bytes()[..13]);
    let mut decipher = cbc::Decryptor::<Aes256>::new_from_slices(&key, &iv)
        .map_err(|_| anyhow::anyhow!("Không thể khởi tạo giải mã"))?;
    let mut source = std::io::Cursor::new(first).chain(input);
    let mut buffer = vec![0u8; 64 * 1024 + 32];
    let mut pending = 0;
    loop {
        let n = source.read(&mut buffer[pending..])?;
        pending += n;
        if n == 0 {
            if pending != 16 {
                bail!("Tệp sao lưu bị cắt ngắn hoặc bị hỏng");
            }
            let last = decipher
                .decrypt_padded_mut::<Pkcs7>(&mut buffer[..pending])
                .map_err(|_| anyhow::anyhow!("Tệp sao lưu bị hỏng"))?;
            output.write_all(last)?;
            return Ok(());
        }
        let bytes = (pending.saturating_sub(16) / 16) * 16;
        if bytes > 0 {
            for block in buffer[..bytes].chunks_exact_mut(16) {
                decipher.decrypt_block_mut(cipher::Block::<Aes256>::from_mut_slice(block));
            }
            output.write_all(&buffer[..bytes])?;
            buffer.copy_within(bytes..pending, 0);
            pending -= bytes;
        }
    }
}

/// Check archive entries before extraction; this function does not write files.
///
/// # Errors
/// Returns an error for invalid or unsafe entries, duplicate destinations, resource
/// limits, or incomplete file bodies. Successful validation is not authentication.
pub fn validate_tar<R: Read>(input: R) -> Result<()> {
    const MAX_ENTRIES: usize = 100_000;
    const MAX_TOTAL_BYTES: u64 = 20 * 1024 * 1024 * 1024;
    const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
    let mut archive = tar::Archive::new(input);
    let mut destinations = HashSet::new();
    let mut regular_files = HashSet::new();
    let mut ancestor_paths = HashSet::new();
    let mut total = 0u64;
    let mut count = 0usize;
    // Raw entries prevent the TAR parser from buffering unbounded PAX or GNU metadata.
    for item in archive.entries()?.raw(true) {
        let mut entry = item?;
        count += 1;
        if count > MAX_ENTRIES {
            bail!("Bản sao lưu có quá nhiều tệp");
        }
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            bail!("Bản sao lưu chứa mục không được hỗ trợ");
        }
        let bytes = entry.path_bytes();
        let name = std::str::from_utf8(&bytes)
            .map_err(|_| anyhow::anyhow!("Đường dẫn trong bản sao lưu không hợp lệ"))?;
        let name = if kind.is_dir() {
            name.trim_end_matches('/')
        } else {
            name
        };
        if name.is_empty() || name.starts_with('/') || name.contains(['\\', ':']) {
            bail!("Đường dẫn trong bản sao lưu không an toàn");
        }
        let mut parts = Vec::new();
        for part in name.split('/') {
            let upper = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || (upper.len() == 4
                    && (upper.starts_with("COM") || upper.starts_with("LPT"))
                    && matches!(upper.as_bytes()[3], b'1'..=b'9'));
            if part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with(['.', ' '])
                || part.contains(['<', '>', '"', '|', '?', '*'])
                || part.chars().any(char::is_control)
                || reserved
            {
                bail!("Đường dẫn trong bản sao lưu không an toàn");
            }
            parts.push(part.to_lowercase());
        }
        let dest = parts.join("/");
        for i in 1..parts.len() {
            let ancestor = parts[..i].join("/");
            if regular_files.contains(&ancestor) {
                bail!("Đường dẫn trong bản sao lưu bị xung đột");
            }
            ancestor_paths.insert(ancestor);
        }
        if !destinations.insert(dest.clone()) {
            bail!("Bản sao lưu có đường dẫn trùng lặp");
        }
        if kind.is_file() {
            if ancestor_paths.contains(&dest) {
                bail!("Đường dẫn trong bản sao lưu bị xung đột");
            }
            regular_files.insert(dest);
        }
        let size = entry.size();
        total = total
            .checked_add(size)
            .ok_or_else(|| anyhow::anyhow!("Bản sao lưu quá lớn"))?;
        if size > MAX_FILE_BYTES || total > MAX_TOTAL_BYTES {
            bail!("Bản sao lưu quá lớn");
        }
        if std::io::copy(&mut entry, &mut std::io::sink())? != size {
            bail!("Tệp sao lưu bị cắt ngắn");
        }
    }
    if count == 0 {
        bail!("Bản sao lưu không có dữ liệu");
    }
    // The TAR iterator accepts bare EOF; require the second end marker and
    // consume the rest so a truncated or appended archive cannot pass silently.
    let mut input = archive.into_inner();
    let mut end = [0u8; 512];
    input
        .read_exact(&mut end)
        .map_err(|_| anyhow::anyhow!("Tệp sao lưu bị cắt ngắn"))?;
    if end.iter().any(|b| *b != 0) {
        bail!("Phần cuối bản sao lưu không hợp lệ");
    }
    let mut trailing = 0u64;
    let mut buffer = [0u8; 8192];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        if buffer[..n].iter().any(|b| *b != 0) {
            bail!("Phần cuối bản sao lưu không hợp lệ");
        }
        trailing += n as u64;
    }
    if trailing % 512 != 0 {
        bail!("Phần cuối bản sao lưu không hợp lệ");
    }
    Ok(())
}

/// Convert a verified Desktop backup message to the existing local store shape.
///
/// # Errors
/// Returns an error for missing identity or timestamps, or for message types whose
/// Desktop semantics have not yet been established. The caller must match the
/// conversation against the backup's conversation database before importing it.
pub fn parse_desktop_record(
    record: &serde_json::Value,
    account_id: &str,
) -> Result<crate::store::StoredMessage> {
    use serde_json::{json, Value};
    if account_id.is_empty() {
        bail!("Thiếu mã tài khoản hiện tại");
    }
    let mut normalized = record.clone();
    let object = normalized
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Tin nhắn không hợp lệ"))?;
    let read_id = |key: &str| -> Option<String> {
        match record.get(key)? {
            Value::String(v) if !v.is_empty() => Some(v.clone()),
            Value::Number(v) => Some(v.to_string()),
            _ => None,
        }
    };
    let id = read_id("msgId").ok_or_else(|| anyhow::anyhow!("Tin nhắn thiếu mã"))?;
    let thread_id = read_id("toUid").ok_or_else(|| anyhow::anyhow!("Tin nhắn thiếu hội thoại"))?;
    let sender_id = read_id("fromUid").unwrap_or_default();
    let timestamp = record
        .get("serverTime")
        .and_then(|v| match v {
            Value::Number(n) => n.as_i64(),
            Value::String(s) => s.parse().ok(),
            _ => None,
        })
        .filter(|v| *v > 0)
        .unwrap_or(0);
    let kind = match (
        record
            .get("msgType")
            .and_then(Value::as_i64)
            .filter(|_| !sender_id.is_empty() && timestamp > 0),
        record.get("message"),
    ) {
        (Some(1), Some(Value::String(_))) => "chat.text",
        (Some(2 | 7), Some(Value::Object(_))) => "chat.photo",
        (Some(4), Some(Value::Object(_))) => "chat.sticker",
        (Some(18), Some(Value::Object(_))) => "chat.video",
        (Some(19), Some(Value::Object(_))) => "chat.file",
        _ => {
            return Ok(crate::store::StoredMessage {
                msg_id: id,
                thread_id,
                sender_id: sender_id.clone(),
                sender_name: record
                    .get("dName")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                text: "[Tin nhắn chưa được hỗ trợ]".into(),
                msg_type: "backup.unknown".into(),
                ts: timestamp,
                is_outgoing: sender_id == "0" || sender_id == account_id,
                raw_json: json!({"backup_record": record}).to_string(),
            });
        }
    };
    object.remove("content");
    object.insert("msgType".into(), json!(kind));
    object.insert("ts".into(), json!(timestamp));
    if let Some(Value::Object(message)) = object.get_mut("message") {
        if !message.contains_key("href") {
            if let Some(url) = message.get("oriUrl").cloned() {
                message.insert("href".into(), url);
            }
        }
        if !message.contains_key("thumb") {
            if let Some(thumb) = message.get("thumbUrl").cloned() {
                message.insert("thumb".into(), thumb);
            }
        }
    }
    let mut parsed = crate::chat::parse_chat_message(&normalized)
        .ok_or_else(|| anyhow::anyhow!("Không thể đọc tin nhắn"))?;
    if kind == "chat.video" {
        parsed.link_src = Some("Video Zalo".to_owned());
        if !parsed.text.starts_with("[Video]") {
            parsed.text = "[Video]".to_owned();
        }
    }
    let mut stored = crate::store::StoredMessage::from(&parsed);
    stored.msg_id = id;
    stored.thread_id = thread_id;
    stored.is_outgoing = sender_id == "0" || sender_id == account_id;
    stored.sender_id = sender_id;
    stored.msg_type = kind.to_owned();
    Ok(stored)
}

/// Atomically merge validated messages into the local store without replacing existing rows.
///
/// The caller must first verify the backup account and select the correct profile.
///
/// # Errors
/// Rolls back on malformed or unsupported records, missing conversations, identity
/// conflicts, or database errors.
pub fn merge_desktop_records<I>(
    store: &mut crate::store::MessageStore,
    threads: &[crate::store::StoredThread],
    records: I,
    account_id: &str,
) -> Result<usize>
where
    I: IntoIterator<Item = serde_json::Value>,
{
    use rusqlite::{params, OptionalExtension};
    let ids: HashSet<&str> = threads.iter().map(|t| t.thread_id.as_str()).collect();
    if ids.len() != threads.len() || ids.contains("") {
        bail!("Danh sách hội thoại không hợp lệ");
    }
    let tx = store.conn.transaction()?;
    for thread in threads {
        tx.execute(
            "INSERT OR IGNORE INTO threads (thread_id, name, is_group, last_ts) VALUES (?1, ?2, ?3, ?4)",
            params![thread.thread_id, thread.name, thread.is_group as i64, thread.last_ts],
        )?;
    }
    let mut added = 0;
    for record in records {
        let message = parse_desktop_record(&record, account_id)?;
        if !ids.contains(message.thread_id.as_str()) {
            bail!("Tin nhắn không khớp danh sách hội thoại");
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT thread_id FROM messages WHERE msg_id = ?1",
                [&message.msg_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(owner) = existing {
            if owner != message.thread_id {
                bail!("Mã tin nhắn bị xung đột giữa các hội thoại");
            }
            continue;
        }
        added += tx.execute(
            "INSERT INTO messages (msg_id, thread_id, sender_id, sender_name, text, msg_type, ts, is_outgoing, raw_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![message.msg_id, message.thread_id, message.sender_id, message.sender_name,
                message.text, message.msg_type, message.ts, message.is_outgoing as i64, message.raw_json],
        )?;
    }
    tx.commit()?;
    Ok(added)
}

/// Counts returned after a completed backup import.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackupImportSummary {
    /// Number of new messages inserted.
    pub messages_added: usize,
    /// Number of new local attachments stored.
    pub media_added: usize,
    /// Number of unsupported records retained as placeholders.
    pub unsupported_records: usize,
    /// Number of attachment records without an available matching file.
    pub missing_media: usize,
}

/// Import a validated Desktop TAR into a local message store.
///
/// # Errors
/// Rolls back for unsafe archives, a different account, malformed records, or
/// conflicting message identities. The caller owns staging and cleanup.
pub fn import_tar_records<R: Read + std::io::Seek>(
    input: R,
    store: &mut crate::store::MessageStore,
    account_id: &str,
) -> Result<usize> {
    Ok(import_tar_records_inner(input, store, account_id, None, None)?.messages_added)
}

/// Import a validated archive and return message and media counts.
///
/// # Errors
/// Returns an error if validation or the database transaction fails.
pub fn import_tar_records_report<R: Read + std::io::Seek>(
    input: R,
    store: &mut crate::store::MessageStore,
    account_id: &str,
) -> Result<BackupImportSummary> {
    import_tar_records_inner(input, store, account_id, None, None)
}

fn import_tar_records_inner<R: Read + std::io::Seek>(
    mut input: R,
    store: &mut crate::store::MessageStore,
    account_id: &str,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    progress: Option<&dyn Fn(&str)>,
) -> Result<BackupImportSummary> {
    use rusqlite::{params, OptionalExtension};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, SeekFrom};
    if account_id.is_empty()
        || !account_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("Mã tài khoản không hợp lệ");
    }
    if let Some(notify) = progress {
        notify("Đang kiểm tra bản sao lưu…");
    }
    validate_tar(&mut input)?;
    if let Some(notify) = progress {
        notify("Đang đọc danh sách hội thoại…");
    }
    input.seek(SeekFrom::Start(0))?;
    let mut threads = HashMap::<String, crate::store::StoredThread>::new();
    let mut message_files = 0usize;
    let mut assets: HashMap<String, Vec<(String, String, u64)>> = HashMap::new();
    {
        let mut archive = tar::Archive::new(&mut input);
        for item in archive.entries()?.raw(true) {
            let entry = item?;
            let name = entry.path_bytes();
            let path = std::str::from_utf8(&name)?;
            let root = path.split('/').next().unwrap_or("");
            if root != account_id {
                bail!("Bản sao lưu không thuộc tài khoản hiện tại");
            }
            if !entry.header().entry_type().is_file() {
                continue;
            }
            if path.to_ascii_lowercase().ends_with("_zmessage.zdb") {
                message_files += 1;
                continue;
            }
            if !path.to_ascii_lowercase().ends_with(".zdb") {
                let filename = path.rsplit('/').next().unwrap_or("");
                let stem = filename
                    .rsplit_once('.')
                    .map(|(stem, _)| stem)
                    .unwrap_or(filename);
                let parts: Vec<_> = stem.splitn(4, '_').collect();
                if parts.len() >= 3
                    && parts[..2].iter().all(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
                    && numeric_thread_id(parts[2]).is_some()
                    && entry.size() > 0
                    && entry.size() <= 512 * 1024 * 1024
                {
                    let prefix = format!("{}_{}_{}", parts[0], parts[1], numeric_thread_id(parts[2]).unwrap());
                    let list = assets.entry(prefix).or_default();
                    if list.len() >= 128 {
                        bail!("Quá nhiều tệp phương tiện trùng mã");
                    }
                    list.push((path.to_owned(), filename.to_owned(), entry.size()));
                }
                continue;
            }
            let mut reader = BufReader::new(entry);
            loop {
                let mut line = Vec::new();
                let n = reader
                    .by_ref()
                    .take(4 * 1024 * 1024 + 1)
                    .read_until(b'\n', &mut line)?;
                if n == 0 {
                    break;
                }
                if n > 4 * 1024 * 1024 {
                    bail!("Bản ghi sao lưu quá lớn");
                }
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let value: Value = serde_json::from_slice(&line)
                    .map_err(|_| anyhow::anyhow!("Dữ liệu hội thoại không hợp lệ"))?;
                let id = match value.get("userId") {
                    Some(Value::String(v)) if !v.is_empty() => v.clone(),
                    Some(Value::Number(v)) => v.to_string(),
                    _ => continue,
                };
                let is_group = value
                    .get("isGroup")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    || value.get("isGroup").and_then(Value::as_i64) == Some(1);
                let t = crate::store::StoredThread {
                    thread_id: id.clone(),
                    name: String::new(),
                    is_group,
                    last_ts: 0,
                };
                if threads.insert(id, t).is_some() {
                    bail!("Hội thoại bị trùng trong bản sao lưu");
                }
            }
        }
    }
    if threads.is_empty() || message_files != 1 {
        bail!("Không tìm thấy dữ liệu hội thoại hoặc tin nhắn được hỗ trợ");
    }
    input.seek(SeekFrom::Start(0))?;
    if let Some(notify) = progress {
        notify("Đang nhập tin nhắn…");
    }
    let tx = store.conn.transaction()?;
    let owner: Option<String> = tx.query_row("SELECT account_id FROM profile_owner WHERE id=1", [], |r| r.get(0)).optional()?;
    match owner {
        Some(owner) if owner != account_id => bail!("Hồ sơ dữ liệu thuộc tài khoản khác"),
        Some(_) => {},
        None => {
            let occupied: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM messages LIMIT 1)", [], |r| r.get(0))?;
            if occupied { bail!("Lịch sử hiện có chưa xác minh chủ sở hữu; hãy dùng hồ sơ mới"); }
            tx.execute("INSERT INTO profile_owner(id,account_id) VALUES(1,?1)", [account_id])?;
        }
    }
    for thread in threads.values() {
        tx.execute("INSERT OR IGNORE INTO threads (thread_id, name, is_group, last_ts) VALUES (?1, ?2, ?3, ?4)", params![thread.thread_id, thread.name, thread.is_group as i64, thread.last_ts])?;
    }
    let mut report = BackupImportSummary::default();
    let mut wanted: HashMap<String, Vec<String>> = HashMap::new();
    let mut assigned_media_ids = HashSet::new();
    {
        let mut archive = tar::Archive::new(&mut input);
        for item in archive.entries()?.raw(true) {
            let entry = item?;
            let name = entry.path_bytes();
            if !std::str::from_utf8(&name)?
                .to_ascii_lowercase()
                .ends_with("_zmessage.zdb")
            {
                continue;
            }
            let mut reader = BufReader::new(entry);
            loop {
                let mut line = Vec::new();
                let n = reader
                    .by_ref()
                    .take(4 * 1024 * 1024 + 1)
                    .read_until(b'\n', &mut line)?;
                if n == 0 {
                    break;
                }
                if n > 4 * 1024 * 1024 {
                    bail!("Bản ghi sao lưu quá lớn");
                }
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let record: Value = serde_json::from_slice(&line)
                    .map_err(|_| anyhow::anyhow!("Dữ liệu tin nhắn không hợp lệ"))?;
                let mut message = parse_desktop_record(&record, account_id)?;
                // Preserve an existing cross-thread row and assign a stable local
                // identity to the archive record, retaining its original wire ID.
                let original_id = message.msg_id.clone();
                let mut hasher = Sha256::new();
                hasher.update((message.thread_id.len() as u64).to_be_bytes());
                hasher.update(message.thread_id.as_bytes());
                hasher.update(original_id.as_bytes());
                let local_id = format!("backup:{:x}", hasher.finalize());
                let local_exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM messages WHERE msg_id=?1)", [&local_id], |r| r.get(0))?;
                let original_owner: Option<String> = tx.query_row("SELECT thread_id FROM messages WHERE msg_id=?1", [&original_id], |r| r.get(0)).optional()?;
                if local_exists || original_owner.as_deref().is_some_and(|owner| owner != message.thread_id) {
                    message.msg_id = local_id;
                    let mut metadata = serde_json::from_str::<Value>(&message.raw_json).unwrap_or_else(|_| serde_json::json!({}));
                    if let Some(object) = metadata.as_object_mut() {
                        object.insert("backup_original_msg_id".into(), Value::String(original_id));
                        let mut wire = serde_json::Map::new();
                        for key in ["msgId", "cliMsgId", "fromUid", "toUid", "msgType", "serverTime"] {
                            if let Some(value) = record.get(key) { wire.insert(key.to_owned(), value.clone()); }
                        }
                        object.insert("backup_wire".into(), Value::Object(wire));
                    }
                    message.raw_json = metadata.to_string();
                }
                if !threads.contains_key(&message.thread_id) {
                    bail!("Tin nhắn không khớp hội thoại");
                }
                if message.msg_type == "backup.unknown" {
                    report.unsupported_records += 1;
                }
                let expects_media = matches!(
                    record.get("msgType").and_then(Value::as_i64),
                    Some(2 | 7 | 18 | 19)
                );
                if expects_media {
                    let already: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM backup_media WHERE msg_id=?1)",
                        [&message.msg_id],
                        |r| r.get(0),
                    )?;
                    let selected = desktop_media_prefix(&record, account_id)
                        .and_then(|prefix| assets.get(&prefix))
                        .and_then(|items| {
                            items
                                .iter()
                                .filter(|(_, name, _)| {
                                    let cache_group = name.splitn(4, '_').nth(2).is_some_and(|part| part.starts_with('g') || part.starts_with('G'));
                                    let thread_is_group = threads.get(&message.thread_id).is_some_and(|thread| thread.is_group);
                                    (cache_group == thread_is_group) && desktop_media_matches(&record, account_id, name)
                                })
                                .max_by_key(|(path, _, size)| (*size, path.as_str()))
                        });
                    if !already && !assigned_media_ids.contains(&message.msg_id) {
                        if let Some((path, _, _)) = selected {
                            let ids = wanted.entry(path.clone()).or_default();
                            if assigned_media_ids.insert(message.msg_id.clone()) {
                                ids.push(message.msg_id.clone());
                            }
                        } else {
                            report.missing_media += 1;
                        }
                    }
                }
                let owner: Option<String> = tx
                    .query_row(
                        "SELECT thread_id FROM messages WHERE msg_id = ?1",
                        [&message.msg_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                if let Some(owner) = owner {
                    if owner != message.thread_id {
                        bail!("Mã tin nhắn bị xung đột giữa các hội thoại");
                    }
                } else {
                    report.messages_added += tx.execute("INSERT INTO messages (msg_id,thread_id,sender_id,sender_name,text,msg_type,ts,is_outgoing,raw_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![message.msg_id,message.thread_id,message.sender_id,message.sender_name,message.text,message.msg_type,message.ts,message.is_outgoing as i64,message.raw_json])?;
                    tx.execute(
                        "UPDATE threads SET last_ts=MAX(COALESCE(last_ts,0),?1) WHERE thread_id=?2",
                        params![message.ts, message.thread_id],
                    )?;
                }
                if message.msg_type == "backup.unknown" {
                    tx.execute("INSERT OR IGNORE INTO backup_unknown_records (msg_id,payload) VALUES (?1,?2)", params![message.msg_id, record.to_string()])?;
                }
            }
        }
    }
    if let Some(notify) = progress {
        notify("Đang khôi phục tệp và hình ảnh…");
    }
    input.seek(SeekFrom::Start(0))?;
    {
        let mut archive = tar::Archive::new(&mut input);
        for item in archive.entries()?.raw(true) {
            let mut entry = item?;
            let name = entry.path_bytes().into_owned();
            let path = std::str::from_utf8(&name)?;
            let Some(ids) = wanted.get(path) else {
                continue;
            };
            if ids.is_empty() {
                continue;
            }
            let size = i32::try_from(entry.size())
                .map_err(|_| anyhow::anyhow!("Tệp phương tiện quá lớn"))?;
            let extension = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            let first = &ids[0];
            tx.execute(
                "INSERT INTO backup_media (msg_id, data, extension) VALUES (?1, ?2, ?3)",
                params![first, rusqlite::blob::ZeroBlob(size), extension],
            )?;
            let rowid: i64 = tx.query_row(
                "SELECT rowid FROM backup_media WHERE msg_id=?1",
                [first],
                |r| r.get(0),
            )?;
            {
                let mut blob = tx.blob_open(
                    rusqlite::DatabaseName::Main,
                    "backup_media",
                    "data",
                    rowid,
                    false,
                )?;
                if std::io::copy(&mut entry, &mut blob)? != size as u64 {
                    bail!("Tệp phương tiện bị cắt ngắn");
                }
            }
            report.media_added += ids.len();
            for id in &ids[1..] {
                tx.execute("INSERT INTO backup_media (msg_id, data, extension) SELECT ?1, data, extension FROM backup_media WHERE msg_id=?2", params![id, first])?;
            }
        }
    }
    if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) {
        bail!("Đã dừng nhập bản sao lưu");
    }
    tx.commit()?;
    Ok(report)
}

/// Seekable, in-memory decryption of an encrypted Desktop V2 TAR.
pub struct EncryptedTarReader<'a, R> {
    source: R,
    key: [u8; 32],
    iv: [u8; 16],
    pos: u64,
    len: u64,
    cancel: &'a std::sync::atomic::AtomicBool,
}

impl<R> Drop for EncryptedTarReader<'_, R> {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.key.zeroize();
        self.iv.zeroize();
    }
}

impl<'a, R: Read + std::io::Seek> EncryptedTarReader<'a, R> {
    /// Open and verify a Desktop V2 stream.
    pub fn new(mut source: R, uin: &str, cancel: &'a std::sync::atomic::AtomicBool) -> Result<Self> {
        use std::io::SeekFrom;
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            bail!("Đã dừng nhập bản sao lưu");
        }
        let size = source.seek(SeekFrom::End(0))?;
        if size < 528 || size % 16 != 0 {
            bail!("Tệp sao lưu bị cắt ngắn");
        }
        source.seek(SeekFrom::Start(0))?;
        let mut header = [0u8; 512];
        source.read_exact(&mut header)?;
        verify_v2_header(&header, uin)?;
        let mut key = [0u8; 32];
        key.copy_from_slice(&Sha256::digest(uin.as_bytes()));
        let mut iv = [0u8; 16];
        iv[..3].copy_from_slice(b"zie");
        iv[3..].copy_from_slice(&uin.as_bytes()[..13]);
        source.seek(SeekFrom::Start(size - 32))?;
        let mut prev = [0u8; 16];
        let mut last = [0u8; 16];
        source.read_exact(&mut prev)?;
        source.read_exact(&mut last)?;
        let tail_len = cbc::Decryptor::<Aes256>::new_from_slices(&key, &prev)
            .map_err(|_| anyhow::anyhow!("Không thể khởi tạo giải mã"))?
            .decrypt_padded_mut::<Pkcs7>(&mut last)
            .map_err(|_| anyhow::anyhow!("Tệp sao lưu bị hỏng"))?
            .len();
        Ok(Self {
            source,
            key,
            iv,
            pos: 0,
            len: size - 16 + tail_len as u64,
            cancel,
        })
    }
}

impl<R: Read + std::io::Seek> Read for EncryptedTarReader<'_, R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        use std::io::{Error, ErrorKind, SeekFrom};
        if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(Error::new(ErrorKind::Other, "cancelled"));
        }
        if self.pos >= self.len || out.is_empty() {
            return Ok(0);
        }
        let count = out.len().min(64 * 1024).min((self.len - self.pos) as usize);
        let start = self.pos / 16 * 16;
        let offset = (self.pos - start) as usize;
        let bytes = (offset + count + 15) / 16 * 16;
        let mut previous = self.iv;
        if start > 0 {
            self.source.seek(SeekFrom::Start(start - 16))?;
            self.source.read_exact(&mut previous)?;
        }
        self.source.seek(SeekFrom::Start(start))?;
        let mut buffer = vec![0u8; bytes];
        self.source.read_exact(&mut buffer)?;
        cbc::Decryptor::<Aes256>::new_from_slices(&self.key, &previous)
            .map_err(|_| Error::new(ErrorKind::InvalidData, "cipher"))?
            .decrypt_padded_mut::<NoPadding>(&mut buffer)
            .map_err(|_| Error::new(ErrorKind::InvalidData, "cipher"))?;
        out[..count].copy_from_slice(&buffer[offset..offset + count]);
        use zeroize::Zeroize;
        buffer.zeroize();
        self.pos += count as u64;
        Ok(count)
    }
}

impl<R: Read + std::io::Seek> std::io::Seek for EncryptedTarReader<'_, R> {
    fn seek(&mut self, from: std::io::SeekFrom) -> std::io::Result<u64> {
        use std::io::{Error, ErrorKind, SeekFrom};
        let next = match from {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::End(n) => self.len as i128 + n as i128,
            SeekFrom::Current(n) => self.pos as i128 + n as i128,
        };
        if next < 0 || next > u64::MAX as i128 {
            return Err(Error::new(ErrorKind::InvalidInput, "seek"));
        }
        self.pos = next as u64;
        Ok(self.pos)
    }
}

/// Import a V2 backup without creating a plaintext staging file.
///
/// # Errors
/// Returns an error for cancellation, invalid ciphertext, account mismatch,
/// invalid archive data, or a transactional database failure.
pub fn import_encrypted_backup<R: Read + std::io::Seek>(
    source: R,
    uin: &str,
    account_id: &str,
    store: &mut crate::store::MessageStore,
    _staging_dir: &std::path::Path,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<usize> {
    let reader = EncryptedTarReader::new(source, uin, cancel)?;
    Ok(import_tar_records_inner(reader, store, account_id, Some(cancel), None)?.messages_added)
}

/// Import an encrypted archive and return message and media counts.
///
/// # Errors
/// Returns an error for cancellation, invalid data, or a database failure.
pub fn import_encrypted_backup_with_report<R: Read + std::io::Seek>(
    source: R,
    uin: &str,
    account_id: &str,
    store: &mut crate::store::MessageStore,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<BackupImportSummary> {
    let reader = EncryptedTarReader::new(source, uin, cancel)?;
    import_tar_records_inner(reader, store, account_id, Some(cancel), None)
}

/// Import an encrypted backup while reporting non-sensitive phase descriptions.
///
/// # Errors
/// Returns an error for cancellation, invalid data, or a database failure.
pub fn import_encrypted_backup_with_progress<R: Read + std::io::Seek>(
    source: R,
    uin: &str,
    account_id: &str,
    store: &mut crate::store::MessageStore,
    cancel: &std::sync::atomic::AtomicBool,
    progress: &dyn Fn(&str),
) -> Result<BackupImportSummary> {
    let reader = EncryptedTarReader::new(source, uin, cancel)?;
    import_tar_records_inner(reader, store, account_id, Some(cancel), Some(progress))
}

fn numeric_thread_id(value: &str) -> Option<&str> {
    let value = value.strip_prefix('g').or_else(|| value.strip_prefix('G')).unwrap_or(value);
    (!value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())).then_some(value)
}
fn desktop_media_prefix(record: &serde_json::Value, account_id: &str) -> Option<String> {
    let get = |key: &str| match record.get(key)? {
        serde_json::Value::String(v) => Some(v.clone()),
        serde_json::Value::Number(v) => Some(v.to_string()),
        _ => None,
    };
    let client = get("cliMsgId")?;
    let mut sender = get("fromUid")?;
    let mut thread = get("toUid")?;
    if sender == "0" {
        sender = account_id.to_owned();
    }
    if thread == "0" {
        thread = account_id.to_owned();
    }
    if [&client, &sender].iter().any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit())) { return None; }
    Some(format!("{client}_{sender}_{}", numeric_thread_id(&thread)?))
}

/// Match a Desktop cache filename to the message identity and one of its media URLs.
pub fn desktop_media_matches(record: &serde_json::Value, account_id: &str, filename: &str) -> bool {
    use serde_json::Value;
    let identity = |key: &str| match record.get(key) {
        Some(Value::String(v)) => Some(v.clone()),
        Some(Value::Number(v)) => Some(v.to_string()),
        _ => None,
    };
    let (Some(client), Some(mut sender), Some(mut thread)) =
        (identity("cliMsgId"), identity("fromUid"), identity("toUid"))
    else {
        return false;
    };
    if sender == "0" {
        sender = account_id.to_owned();
    }
    if thread == "0" {
        thread = account_id.to_owned();
    }
    if [&client, &sender].iter().any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit())) { return false; }
    let Some(thread) = numeric_thread_id(&thread) else { return false; };
    let variants = [thread.to_owned(), format!("g{thread}"), format!("G{thread}")];
    if matches!(record.get("msgType").and_then(Value::as_i64), Some(18 | 19)) {
        for variant in &variants {
            let exact = format!("{client}_{sender}_{variant}.");
            if let Some(ext) = filename.strip_prefix(&exact) {
                if !ext.is_empty() && ext.len() <= 12 && ext.bytes().all(|b| b.is_ascii_alphanumeric()) { return true; }
            }
        }
    }
    let Some(message) = record.get("message").and_then(Value::as_object) else {
        return false;
    };
    for field in ["oriUrl", "hdUrl", "normalUrl", "thumbUrl", "href"] {
        let Some(url) = message.get(field).and_then(Value::as_str) else {
            continue;
        };
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            continue;
        }
        for variant in &variants {
        let prefix = format!("{client}_{sender}_{variant}_{}.", crate::crypto::md5_hex(url));
        if let Some(ext) = filename.strip_prefix(&prefix) {
            if matches!(
                ext.to_ascii_lowercase().as_str(),
                "jxl"
                    | "jpg"
                    | "jpeg"
                    | "png"
                    | "webp"
                    | "gif"
                    | "mp4"
                    | "mov"
                    | "mkv"
                    | "aac"
                    | "mp3"
            ) {
                return true;
            }
        }
        }
    }
    false
}

/// Decode a bounded backup image, including JPEG XL, to RGBA pixels.
///
/// # Errors
/// Rejects malformed, unsupported, or oversized images.
pub fn decode_backup_image(bytes: &[u8]) -> Result<image::RgbaImage> {
    const MAX_BYTES: usize = 64 * 1024 * 1024;
    const MAX_PIXELS: u64 = 16_000_000;
    if bytes.len() > MAX_BYTES {
        bail!("Hình ảnh quá lớn");
    }
    let jxl = bytes.starts_with(&[0xff, 0x0a])
        || bytes.starts_with(&[0, 0, 0, 12, b'J', b'X', b'L', b' ', 13, 10, 135, 10]);
    if jxl {
        let tracker = jxl_oxide::AllocTracker::with_limit(128 * 1024 * 1024);
        let decoder = jxl_oxide::JxlImage::builder()
            .alloc_tracker(tracker)
            .read(std::io::Cursor::new(bytes))
            .map_err(|_| anyhow::anyhow!("Không thể đọc hình ảnh JXL"))?;
        let (width, height) = (decoder.width(), decoder.height());
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
            bail!("Kích thước hình ảnh không được hỗ trợ");
        }
        let render = decoder
            .render_frame(0)
            .map_err(|_| anyhow::anyhow!("Không thể giải mã hình ảnh JXL"))?;
        let mut stream = render.stream();
        let (width, height, channels) =
            (stream.width(), stream.height(), stream.channels() as usize);
        if !(1..=4).contains(&channels) || u64::from(width) * u64::from(height) > MAX_PIXELS {
            bail!("Số kênh hoặc kích thước hình ảnh không được hỗ trợ");
        }
        let mut pixels = vec![0u8; width as usize * height as usize * channels];
        if stream.write_to_buffer(&mut pixels) != pixels.len() {
            bail!("Hình ảnh không hoàn chỉnh");
        }
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for pixel in pixels.chunks_exact(channels) {
            match channels {
                1 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]),
                2 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]),
                3 => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
                4 => rgba.extend_from_slice(pixel),
                _ => unreachable!(),
            }
        }
        return image::RgbaImage::from_raw(width, height, rgba)
            .ok_or_else(|| anyhow::anyhow!("Hình ảnh không hợp lệ"));
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(10000);
    limits.max_image_height = Some(10000);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let img = reader
        .decode()
        .map_err(|_| anyhow::anyhow!("Không thể giải mã hình ảnh"))?;
    if u64::from(img.width()) * u64::from(img.height()) > MAX_PIXELS {
        bail!("Kích thước hình ảnh không được hỗ trợ");
    }
    Ok(img.to_rgba8())
}

/// Stream a recovered attachment to a caller-owned destination.
///
/// # Errors
/// Returns an error if the attachment does not exist or reading or writing fails.
pub fn copy_backup_media<W: Write>(
    store: &crate::store::MessageStore,
    msg_id: &str,
    mut out: W,
) -> Result<u64> {
    let rowid: i64 = store.conn.query_row(
        "SELECT rowid FROM backup_media WHERE msg_id=?1",
        [msg_id],
        |r| r.get(0),
    )?;
    let mut blob = store.conn.blob_open(
        rusqlite::DatabaseName::Main,
        "backup_media",
        "data",
        rowid,
        true,
    )?;
    Ok(std::io::copy(&mut blob, &mut out)?)
}
