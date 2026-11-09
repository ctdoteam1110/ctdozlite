//! Single-writer admission and per-session tracking for embedded-metadata phone batches.

use super::{PhoneSyncIds, PhoneSyncRequestBatch, Session, SyncV2Keys};
use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, Semaphore};

static FILE_ADMISSION: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(1)));
static METADATA_READY: Notify = Notify::const_new();
static STATE: LazyLock<Mutex<ImportState>> = LazyLock::new(|| Mutex::new(ImportState::default()));

#[derive(Default)]
struct ImportState {
    sync_id: Option<String>,
    owner: String,
    identities: Arc<PhoneSyncIds>,
    request_index: usize,
    total_requests: usize,
    total_scopes: usize,
    imported_before: usize,
    unavailable_before: usize,
    unavailable_partitions: HashSet<String>,
    metadata_ready: bool,
    confirmed: bool,
    completed: HashSet<u64>,
    received: BTreeMap<u64, BatchObservation>,
    last_mobile_status: Option<u8>,
    expected: Option<u64>,
    imported: usize,
    failed: Option<String>,
    last_activity: Option<Instant>,
}

#[derive(serde::Serialize)]
struct BatchObservation {
    batch_type: u8,
    is_last: bool,
    error_code: i64,
    scope_count: Option<usize>,
}

impl ImportState {
    fn diagnostic_shape(&self) -> serde_json::Value {
        let mut completed: Vec<_> = self.completed.iter().copied().collect();
        completed.sort_unstable();
        serde_json::json!({
            "received": self.received,
            "completed_indices": completed,
            "expected_batches": self.expected,
            "metadata_ready": self.metadata_ready,
            "last_mobile_status": self.last_mobile_status,
            "unavailable_threads": self.unavailable_before + self.unavailable_partitions.len(),
        })
    }

    fn complete_missing_partition(
        &mut self,
        batch: &crate::chat::MobileSyncV2Batch,
    ) -> Result<V2BatchProgress> {
        // PartitionNotFound is a per-range absence in the native task manager, not a codec error.
        anyhow::ensure!(
            batch.error_code == 502 && batch.batch_type == 1,
            "Lỗi phạm vi đồng bộ không hợp lệ"
        );
        let scopes = batch
            .scopes
            .as_ref()
            .filter(|scopes| !scopes.is_empty())
            .ok_or_else(|| anyhow!("Điện thoại báo mã 502 nhưng không chỉ rõ hội thoại"))?;
        anyhow::ensure!(
            scopes.len() <= super::PHONE_SCOPES_PER_REQUEST
                && scopes
                    .iter()
                    .all(|scope| self.identities.partition_threads.contains_key(scope)),
            "Điện thoại báo mã 502 cho phạm vi ngoài yêu cầu"
        );
        self.unavailable_partitions.extend(scopes.iter().cloned());
        Ok(self.complete_batch(batch.index, batch.is_last, 0))
    }

    fn complete_batch(&mut self, index: u64, is_last: bool, added: usize) -> V2BatchProgress {
        if self.completed.insert(index) {
            self.imported += added;
        }
        if is_last {
            self.expected = Some(index + 1);
        }
        self.last_activity = Some(Instant::now());
        V2BatchProgress {
            imported: self.imported_before + self.imported,
            request_number: self.request_index + 1,
            request_total: self.total_requests.max(1),
            skipped_threads: self.identities.skipped_threads,
            unavailable_threads: self.unavailable_before + self.unavailable_partitions.len(),
            complete: self.expected.is_some_and(|total| {
                self.completed.len() as u64 == total
                    && (0..total).all(|i| self.completed.contains(&i))
            }),
        }
    }
}

/// Cumulative, idempotent result of an imported phone batch.
#[derive(Clone, Debug)]
pub struct V2BatchProgress {
    /// New rows inserted across the current session.
    pub imported: usize,
    /// Known threads omitted because no global identity could be resolved.
    pub skipped_threads: usize,
    /// Distinct requested scopes that the phone explicitly reported as absent.
    pub unavailable_threads: usize,
    /// One-based request number within the overall operation.
    pub request_number: usize,
    /// Total requests in the operation.
    pub request_total: usize,
    /// Every received file batch through this request's last index has completed.
    pub complete: bool,
}

impl V2BatchProgress {
    /// Whether every request, rather than only the current one, has imported all its files.
    pub fn transfer_complete(&self) -> bool {
        self.complete && self.request_number >= self.request_total.max(1)
    }

    /// Describe completed data without reporting an intermediate request as overall success.
    pub fn completion_message(&self) -> String {
        if !self.transfer_complete() {
            return format!(
                "Đã nhập {} tin từ điện thoại; đã xử lý đợt {}/{}",
                self.imported,
                self.request_number,
                self.request_total.max(1)
            );
        }
        let mut note = format!(
            "Đã giải mã & đồng bộ thành công {} tin nhắn từ điện thoại",
            self.imported
        );
        if self.skipped_threads > 0 {
            note.push_str(&format!(
                "; {} hội thoại chưa lấy được định danh đồng bộ",
                self.skipped_threads
            ));
        }
        if self.unavailable_threads > 0 {
            note.push_str(&format!(
                "; {} hội thoại có phạm vi lịch sử không tìm thấy trên điện thoại (mã 502)",
                self.unavailable_threads
            ));
        }
        note
    }
}

/// A metadata-only snapshot for the parent sync task.
pub struct V2Snapshot {
    /// A v2 session is active.
    pub active: bool,
    /// Number of distinct, verified phone scopes requested.
    pub scope_count: usize,
    /// Total verified scopes across all requests in the operation.
    pub total_scope_count: usize,
    /// The current phone has confirmed or sent a batch.
    pub confirmed: bool,
    /// Current cumulative import result.
    pub progress: V2BatchProgress,
    /// A sanitized terminal failure.
    pub failed: Option<String>,
    /// Last activity within this session.
    pub last_activity: Option<Instant>,
    /// Bounded batch indices/flags only; excludes identities, URLs and content.
    pub batch_diagnostics: serde_json::Value,
    /// A matching phone status or file proves delivery independently of an RPC acknowledgement.
    pub phone_observed: bool,
}

/// Read current progress without retaining a lock across asynchronous work.
pub fn v2_snapshot() -> V2Snapshot {
    let state = STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let complete = state.expected.is_some_and(|total| {
        state.completed.len() as u64 == total && (0..total).all(|i| state.completed.contains(&i))
    });
    V2Snapshot {
        active: state.sync_id.is_some(),
        scope_count: state.identities.partition_threads.len(),
        total_scope_count: state.total_scopes,
        confirmed: state.confirmed,
        progress: V2BatchProgress {
            imported: state.imported_before + state.imported,
            request_number: if state.sync_id.is_some() {
                state.request_index + 1
            } else {
                0
            },
            request_total: state.total_requests,
            skipped_threads: state.identities.skipped_threads,
            unavailable_threads: state.unavailable_before + state.unavailable_partitions.len(),
            complete,
        },
        failed: state.failed.clone(),
        last_activity: state.last_activity,
        batch_diagnostics: state.diagnostic_shape(),
        phone_observed: phone_delivery_observed(
            &state,
            state.sync_id.as_deref().unwrap_or_default(),
        ),
    }
}

fn phone_delivery_observed(state: &ImportState, id: &str) -> bool {
    matches_session(state, id)
        && (state.confirmed || !state.received.is_empty() || state.failed.is_some())
}

async fn wait_for_observation<F, C>(timeout: Duration, mut observed: F, mut cancelled: C) -> bool
where
    F: FnMut() -> bool,
    C: FnMut() -> bool,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if cancelled() {
            return false;
        }
        if observed() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep_until(
            deadline.min(tokio::time::Instant::now() + Duration::from_millis(100)),
        )
        .await;
    }
}

/// Observe the existing request after uncertain delivery; never enqueue a replacement.
pub(crate) async fn wait_for_v2_receipt(id: &str, timeout: Duration) -> bool {
    wait_for_observation(
        timeout,
        || {
            STATE
                .lock()
                .ok()
                .is_some_and(|state| phone_delivery_observed(&state, id))
        },
        || super::PHONE_SYNC_CANCEL.load(std::sync::atomic::Ordering::Relaxed),
    )
    .await
}

/// Give the normal socket supervisor time to reconnect before the next unsent group.
pub(super) async fn wait_for_connection(timeout: Duration) -> bool {
    wait_for_observation(timeout, crate::socket::has_authenticated_connection, || {
        super::PHONE_SYNC_CANCEL.load(std::sync::atomic::Ordering::Relaxed)
    })
    .await
}

/// Switch to the legacy protocol so stale v2 controls cannot affect it.
pub fn disable_v2_session() {
    let mut state = STATE.lock().unwrap_or_else(|p| p.into_inner());
    *state = ImportState::default();
    METADATA_READY.notify_waiters();
}

/// Read the current remote ID only for lifecycle disposal; never expose it in diagnostics.
pub(super) fn current_sync_id() -> Option<String> {
    STATE.lock().ok().and_then(|state| state.sync_id.clone())
}

/// Dispose every completed/uncertain remote session together, as the native manager does.
///
/// # Errors
/// Returns local cleanup failures; the result counts missing remote acknowledgements.
pub(super) async fn finish_v2_transfer(ids: Vec<String>, reason: u8) -> Result<usize> {
    disable_v2_session();
    let ids: std::collections::BTreeSet<_> = ids.into_iter().collect();
    if !ids.is_empty() {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !crate::socket::has_authenticated_connection()
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    let failures = futures_util::stream::iter(ids).map(|id|async move {
        let result = crate::socket::request_sync_frame(
            crate::socket::CMD_SYNC_ACK_DELETE,
            serde_json::json!({"data":{"syncId":id,"toDevice":0,"reason":reason}}),
            Duration::from_secs(3),
        ).await;
        usize::from(!matches!(result, Ok(ref response) if response.get("error_code").and_then(serde_json::Value::as_i64) == Some(0)))
    }).buffer_unordered(4).fold(0,|total,failed|async move {total+failed}).await;
    if failures > 0 {
        crate::logger::log_error(
            "phone_sync_end",
            "Some transfer disposals were not acknowledged",
        );
    }
    cancel_v2_session().await?;
    Ok(failures)
}

/// Cancel local batch admission and remove per-transfer key files after in-flight work settles.
///
/// # Errors
/// Returns task or filesystem failures while cleaning the stopped transfer.
pub async fn cancel_v2_session() -> Result<()> {
    disable_v2_session();
    let admission = FILE_ADMISSION.clone().acquire_owned().await?;
    tokio::task::spawn_blocking(move || {
        let _admission = admission;
        for file in [
            "sync_v2_keys.json",
            "sync_v2_session.bin",
            "sync_v2_message.bin",
        ] {
            if let Err(error) = std::fs::remove_file(file) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err(error.into());
                }
            }
        }
        Ok(())
    })
    .await
    .context("Không dừng được bộ nhập tin nhắn")?
}

/// Prepare genuine keys and reset files under exclusive single-writer admission.
///
/// # Errors
/// Returns key generation, filesystem or task errors without exporting private material.
pub(crate) async fn prepare_sync_v2_keys(
    sync_id: String,
    owner: String,
    batch: PhoneSyncRequestBatch,
) -> Result<SyncV2Keys> {
    let admission = FILE_ADMISSION.clone().acquire_owned().await?;
    tokio::task::spawn_blocking(move || {
        let _admission = admission;
        {
            let mut state = STATE
                .lock()
                .map_err(|_| anyhow!("Không mở được phiên đồng bộ"))?;
            *state = ImportState {
                sync_id: Some(sync_id),
                owner,
                identities: Arc::new(batch.identities),
                request_index: batch.index,
                total_requests: batch.total_requests,
                total_scopes: batch.total_scopes,
                imported_before: batch.imported_before,
                unavailable_before: batch.unavailable_before,
                last_activity: Some(Instant::now()),
                ..Default::default()
            };
        }
        METADATA_READY.notify_waiters();
        for name in [
            "sync_v2_keys.json",
            "sync_v2_session.bin",
            "sync_v2_message.bin",
        ] {
            if batch.index > 0 && name == "sync_v2_keys.json" {
                continue;
            }
            if let Err(error) = std::fs::remove_file(name) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err(error.into());
                }
            }
        }
        if batch.index == 0 {
            super::generate_sync_v2_keys()
        } else {
            super::generate_sync_v2_keys_mode(true)
        }
    })
    .await
    .context("Không chạy được bộ chuẩn bị đồng bộ")?
}

fn matches_session(state: &ImportState, id: &str) -> bool {
    state.sync_id.as_deref() == Some(id)
}

/// Check whether an asynchronous result still belongs to the active attempt.
pub fn is_active_v2_session(id: &str) -> bool {
    STATE
        .lock()
        .ok()
        .is_some_and(|state| matches_session(&state, id))
}

/// Ignore controls from another device or an earlier sync attempt.
pub fn accept_mobile_sync_event(event: &crate::chat::MobileSyncControlEvent) -> bool {
    use crate::chat::MobileSyncControlEvent::*;
    let state = STATE.lock().unwrap_or_else(|p| p.into_inner());
    match event {
        TransferAfterLogin { .. } => true,
        WaitingConfirmOnPhone { sync_id } | V2Status { sync_id, .. } => {
            matches_session(&state, sync_id)
        }
        SyncV2Batch(batch) => matches_session(&state, &batch.sync_id),
        _ => state.sync_id.is_none(),
    }
}

/// Apply only the active phone's status, including automatic confirmation without a popup.
pub fn handle_v2_status(sync_id: &str, status: u8) -> Option<String> {
    let mut state = STATE.lock().ok()?;
    if !matches_session(&state, sync_id) {
        return None;
    }
    state.last_activity = Some(Instant::now());
    state.last_mobile_status = Some(status);
    let message = match status {
        3 => "Đang chờ bạn xác nhận đồng bộ trên điện thoại",
        4 => {
            state.confirmed = true;
            "Điện thoại đã xác nhận đồng bộ"
        }
        5 => {
            state.confirmed = true;
            "Điện thoại đã xác nhận đồng bộ tự động"
        }
        6 | 7 => "Điện thoại đã từ chối hoặc hủy đồng bộ",
        8 => "Điện thoại đang bận đồng bộ hoặc khôi phục dữ liệu",
        9 => "Điện thoại không đủ dung lượng để đồng bộ",
        10 => "Zalo trên điện thoại chưa đăng nhập",
        _ => return None,
    }
    .to_string();
    if (6..=10).contains(&status) {
        state.failed = Some(message.clone());
    }
    Some(message)
}

fn record_failure(id: &str, message: String) {
    if let Ok(mut state) = STATE.lock() {
        if matches_session(&state, id) {
            state.failed = Some(message);
        }
    }
    METADATA_READY.notify_waiters();
}

async fn wait_for_metadata(id: &str) -> Result<bool> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    loop {
        let notified = METADATA_READY.notified();
        {
            let state = STATE
                .lock()
                .map_err(|_| anyhow!("Không đọc được phiên đồng bộ"))?;
            if !matches_session(&state, id) {
                return Ok(false);
            }
            if let Some(error) = &state.failed {
                bail!("{error}");
            }
            if state.metadata_ready {
                return Ok(true);
            }
        }
        tokio::time::timeout_at(deadline, notified)
            .await
            .map_err(|_| anyhow!("Chưa nhận được thông tin giải mã từ điện thoại"))?;
    }
}

struct PrivateStage(std::path::PathBuf);
impl Drop for PrivateStage {
    fn drop(&mut self) {
        // Best-effort removal must not replace the primary decode/import error.
        let _ = std::fs::remove_dir_all(&self.0);
        let _ = std::fs::remove_file("sync_v2_message.bin");
    }
}

/// Download, decrypt and import a batch once, retaining keys until all declared batches finish.
///
/// # Errors
/// Rejects stale/failed sessions, invalid batch metadata, oversized input, download and decode errors.
pub async fn import_sync_v2_batch(
    s: &Session,
    batch: crate::chat::MobileSyncV2Batch,
) -> Result<Option<V2BatchProgress>> {
    let id = batch.sync_id.clone();
    let result = import_batch(s, batch).await;
    if let Err(error) = &result {
        record_failure(&id, error.to_string());
    }
    result
}

async fn import_batch(
    s: &Session,
    batch: crate::chat::MobileSyncV2Batch,
) -> Result<Option<V2BatchProgress>> {
    let owner = s
        .login
        .as_ref()
        .ok_or_else(|| anyhow!("Chưa đăng nhập"))?
        .uin
        .clone();
    {
        let mut state = STATE
            .lock()
            .map_err(|_| anyhow!("Không đọc được phiên đồng bộ"))?;
        if !matches_session(&state, &batch.sync_id) || state.owner != owner {
            return Ok(None);
        }
        anyhow::ensure!(batch.index < 8192, "Phiên đồng bộ có quá nhiều gói dữ liệu");
        state.received.insert(
            batch.index,
            BatchObservation {
                batch_type: batch.batch_type,
                is_last: batch.is_last,
                error_code: batch.error_code,
                scope_count: batch.scopes.as_ref().map(Vec::len),
            },
        );
        if batch.error_code == 502 && batch.batch_type == 1 {
            return state.complete_missing_partition(&batch).map(Some);
        }
    }
    anyhow::ensure!(
        batch.error_code == 0,
        "Điện thoại không cung cấp được dữ liệu đồng bộ (mã {})",
        batch.error_code
    );
    if batch.batch_type == 1 && !wait_for_metadata(&batch.sync_id).await? {
        return Ok(None);
    }
    let admission = FILE_ADMISSION.clone().acquire_owned().await?;
    let name = s
        .login
        .as_ref()
        .map(|v| v.display_name.clone())
        .unwrap_or_default();
    let identities = {
        let mut state = STATE
            .lock()
            .map_err(|_| anyhow!("Không đọc được phiên đồng bộ"))?;
        if !matches_session(&state, &batch.sync_id) || state.owner != owner {
            return Ok(None);
        }
        if state.completed.contains(&batch.index) {
            return Ok(Some(state.complete_batch(batch.index, batch.is_last, 0)));
        }
        if let Some(error) = &state.failed {
            bail!("{error}");
        }
        state.confirmed = true;
        state.last_activity = Some(Instant::now());
        state.identities.clone()
    };
    anyhow::ensure!(
        batch.error_code == 0,
        "Điện thoại không cung cấp được dữ liệu đồng bộ (mã {})",
        batch.error_code
    );
    anyhow::ensure!(
        matches!(batch.batch_type, 1 | 2),
        "Kiểu gói đồng bộ chưa được hỗ trợ"
    );
    let url = reqwest::Url::parse(&batch.url)
        .map_err(|_| anyhow!("Đường dẫn gói đồng bộ không hợp lệ"))?;
    anyhow::ensure!(
        url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(crate::endpoints::is_zalo_download_host),
        "Máy chủ gói đồng bộ không hợp lệ"
    );
    let mut request = s.http.get(url.clone()).timeout(Duration::from_secs(35));
    // CDN download links are already signed; attach session cookies only to the Zalo API domain.
    if url
        .host_str()
        .is_some_and(crate::endpoints::is_zalo_api_host)
    {
        if let Some(cookie) = &s.zpw_sek {
            request = request.header("Cookie", format!("zpw_sek={cookie}"));
        }
    }
    let mut response = request
        .send()
        .await
        .map_err(|_| anyhow!("Không tải được gói đồng bộ điện thoại"))?;
    anyhow::ensure!(
        response.url().scheme() == "https"
            && response
                .url()
                .host_str()
                .is_some_and(crate::endpoints::is_zalo_download_host),
        "Máy chủ chuyển hướng gói đồng bộ không hợp lệ"
    );
    anyhow::ensure!(
        response.status().is_success(),
        "Máy chủ gói đồng bộ trả HTTP {}",
        response.status().as_u16()
    );
    const MAX_BYTES: usize = 64 * 1024 * 1024;
    anyhow::ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= MAX_BYTES as u64),
        "Gói đồng bộ quá lớn"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("Kết nối tải gói đồng bộ bị gián đoạn"))?
    {
        anyhow::ensure!(
            chunk.len() <= MAX_BYTES.saturating_sub(bytes.len()),
            "Gói đồng bộ quá lớn"
        );
        bytes.extend_from_slice(&chunk);
    }
    let sync_id = batch.sync_id.clone();
    let batch_type = batch.batch_type;
    let decoded = tokio::task::spawn_blocking(move || -> Result<_> {
        // Admission moves into blocking work so cancellation cannot expose shared helper files.
        let stage =
            std::env::temp_dir().join(format!("ctdo-v2-{}", crate::param_cipher::random_hex(12)));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&stage)?;
        let stage = PrivateStage(stage);
        let input = stage.0.join("batch.enc");
        let records = encrypted_records(&bytes)?;
        let mut partitions = Vec::new();
        let mut plaintext_bytes = 0;
        for record in records {
            if !is_active_v2_session(&sync_id) {
                return Ok(None);
            }
            std::fs::write(&input, record)?;
            let command = if batch_type == 2 {
                "decrypt-meta"
            } else {
                "decrypt-message"
            };
            let output = super::run_sync_v2_helper(&[command.as_ref(), input.as_os_str()])?;
            anyhow::ensure!(
                output.status.success(),
                "Không giải mã được gói đồng bộ điện thoại"
            );
            let (part, size) = super::decode_v2_plaintext(128 * 1024 * 1024 - plaintext_bytes)?;
            plaintext_bytes += size;
            partitions.extend(part);
            if let Ok(mut state) = STATE.lock() {
                if matches_session(&state, &sync_id) {
                    state.last_activity = Some(Instant::now());
                }
            }
        }
        // Retain only protocol shape for diagnosis; never message text or identities.
        let mut shape = phone_batch_shape(&partitions);
        shape["batch_index"] = serde_json::json!(batch.index);
        shape["batch_type"] = serde_json::json!(batch.batch_type);
        shape["is_last"] = serde_json::json!(batch.is_last);
        if crate::data_dir::write_private_atomic(
            std::path::Path::new("phone_sync_types.json"),
            shape.to_string().as_bytes(),
        )
        .is_err()
        {
            crate::logger::log_error("phone_sync_shape", "Could not persist protocol shape");
        }
        Ok(Some((admission, partitions)))
    })
    .await
    .context("Không chạy được bộ giải mã tin nhắn")??;
    let Some((admission, partitions)) = decoded else {
        return Ok(None);
    };
    if !is_active_v2_session(&batch.sync_id) {
        return Ok(None);
    }
    let senders = tokio::time::timeout(
        Duration::from_secs(30),
        super::ids::resolve_phone_senders(s, &identities, &partitions),
    )
    .await
    .map_err(|_| anyhow!("Máy chủ ánh xạ người gửi phản hồi quá lâu"))??;
    if !is_active_v2_session(&batch.sync_id) {
        return Ok(None);
    }
    tokio::task::spawn_blocking(move || -> Result<_> {
        let _admission = admission;
        if !is_active_v2_session(&batch.sync_id) {
            return Ok(None);
        }
        let added = super::import_v2_partitions(&owner, &name, partitions, &identities, &senders)?;
        let progress = {
            let mut state = STATE
                .lock()
                .map_err(|_| anyhow!("Không cập nhật được phiên đồng bộ"))?;
            if !matches_session(&state, &batch.sync_id) {
                return Ok(None);
            }
            if batch.batch_type == 2 {
                state.metadata_ready = true;
            }
            state.complete_batch(batch.index, batch.is_last, added)
        };
        METADATA_READY.notify_waiters();
        // Identity survives intermediate requests; the transfer controller disposes all sessions.
        Ok(Some(progress))
    })
    .await
    .context("Không chạy được bộ nhập tin nhắn")?
}

fn phone_batch_shape(partitions: &[super::ParsedProtoPartition]) -> serde_json::Value {
    let mut types = std::collections::BTreeMap::<i32, usize>::new();
    let mut count = 0;
    let mut empty_sender = 0;
    let mut zero_sender = 0;
    for message in partitions.iter().flat_map(|p| p.messages.iter()) {
        count += 1;
        *types.entry(message.msg_type).or_default() += 1;
        empty_sender += usize::from(message.sender_id.is_empty());
        zero_sender += usize::from(message.sender_id == "0");
    }
    serde_json::json!({"source":"phone_v2","partitions":partitions.len(),"messages":count,
        "sender_empty":empty_sender,"sender_zero":zero_sender,
        "types":types.into_iter().map(|(kind,rows)|serde_json::json!({"type":kind,"rows":rows})).collect::<Vec<_>>()})
}

fn encrypted_records(bytes: &[u8]) -> Result<Vec<&[u8]>> {
    let mut rest = bytes;
    let mut records = Vec::new();
    while !rest.is_empty() {
        anyhow::ensure!(rest.len() >= 4, "Gói đồng bộ thiếu độ dài bản ghi");
        let length = u32::from_le_bytes(rest[..4].try_into()?) as usize;
        rest = &rest[4..];
        anyhow::ensure!(
            length > 0 && length <= rest.len(),
            "Độ dài bản ghi đồng bộ không hợp lệ"
        );
        anyhow::ensure!(records.len() < 8192, "Gói đồng bộ có quá nhiều bản ghi");
        records.push(&rest[..length]);
        rest = &rest[length..];
    }
    // The native loader accepts an empty successful download as zero records.
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_or_stale_sessions_cannot_confirm_uncertain_delivery() {
        let mut state = ImportState {
            sync_id: Some("active-fixture".into()),
            ..Default::default()
        };
        assert!(!phone_delivery_observed(&state, "active-fixture"));
        state.confirmed = true;
        assert!(!phone_delivery_observed(&state, "stale-fixture"));
        assert!(phone_delivery_observed(&state, "active-fixture"));
        state.confirmed = false;
        state.received.insert(
            0,
            BatchObservation {
                batch_type: 2,
                is_last: false,
                error_code: 0,
                scope_count: Some(1),
            },
        );
        assert!(phone_delivery_observed(&state, "active-fixture"));
    }

    #[tokio::test]
    async fn receipt_wait_handles_late_evidence_timeout_and_cancellation_without_network_io() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let observed = Arc::new(AtomicBool::new(false));
        let later = observed.clone();
        let producer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            later.store(true, Ordering::SeqCst);
        });
        assert!(
            wait_for_observation(
                Duration::from_secs(1),
                || observed.load(Ordering::SeqCst),
                || false
            )
            .await
        );
        producer.await.unwrap();
        assert!(!wait_for_observation(Duration::ZERO, || false, || false).await);
        assert!(!wait_for_observation(Duration::from_secs(1), || true, || true).await);
    }

    fn absent_batch() -> crate::chat::MobileSyncV2Batch {
        crate::chat::MobileSyncV2Batch {
            url: String::new(),
            sync_id: "fixture".into(),
            batch_type: 1,
            index: 1,
            is_last: false,
            error_code: 502,
            scopes: Some(vec!["group/opaque-fixture".into()]),
        }
    }

    #[test]
    fn missing_phone_partition_completes_its_index_without_hiding_other_files() {
        let mut identities = PhoneSyncIds::default();
        identities
            .partition_threads
            .insert("group/opaque-fixture".into(), "g1".into());
        let mut state = ImportState {
            identities: Arc::new(identities),
            unavailable_before: 4,
            ..Default::default()
        };
        state.complete_batch(0, false, 3);
        let batch = absent_batch();
        let progress = state.complete_missing_partition(&batch).unwrap();
        assert_eq!(progress.imported, 3);
        assert_eq!(progress.unavailable_threads, 5);
        assert!(!progress.complete);
        assert_eq!(
            state
                .complete_missing_partition(&batch)
                .unwrap()
                .unavailable_threads,
            5
        );
        let progress = state.complete_batch(2, true, 7);
        assert!(progress.complete);
        assert_eq!(progress.imported, 10);
        assert!(progress
            .completion_message()
            .contains("5 hội thoại có phạm vi lịch sử không tìm thấy trên điện thoại (mã 502)"));
        assert!(!state
            .diagnostic_shape()
            .to_string()
            .contains("opaque-fixture"));
    }

    #[test]
    fn missing_partition_exception_rejects_unknown_metadata_and_unscoped_errors() {
        let mut identities = PhoneSyncIds::default();
        identities
            .partition_threads
            .insert("group/opaque-fixture".into(), "g1".into());
        let mut state = ImportState {
            identities: Arc::new(identities),
            ..Default::default()
        };
        for case in 0..6 {
            let mut batch = absent_batch();
            match case {
                0 => batch.batch_type = 2,
                1 => batch.error_code = 701,
                2 => batch.scopes = None,
                3 => batch.scopes = Some(vec![]),
                4 => batch.scopes = Some(vec!["oneone/unrequested".into()]),
                _ => batch
                    .scopes
                    .as_mut()
                    .unwrap()
                    .push("oneone/unrequested".into()),
            }
            assert!(state.complete_missing_partition(&batch).is_err());
            assert!(state.completed.is_empty());
            assert!(state.unavailable_partitions.is_empty());
        }
    }

    #[test]
    fn batch_diagnostics_explain_missing_indices_without_account_data() {
        let mut state = ImportState {
            sync_id: Some("private-session-id".into()),
            owner: "private-owner-id".into(),
            last_mobile_status: Some(2),
            ..ImportState::default()
        };
        for index in [0, 2] {
            state.received.insert(
                index,
                BatchObservation {
                    batch_type: if index == 0 { 2 } else { 1 },
                    is_last: index == 2,
                    error_code: 0,
                    scope_count: Some(1),
                },
            );
            state.complete_batch(index, index == 2, 1);
        }
        let shape = state.diagnostic_shape();
        assert_eq!(shape["completed_indices"], serde_json::json!([0, 2]));
        assert_eq!(shape["expected_batches"], 3);
        assert_eq!(shape["last_mobile_status"], 2);
        assert_eq!(shape["received"]["2"]["is_last"], true);
        assert!(!shape.to_string().contains("private-"));
        assert!(!state.complete_batch(2, true, 1).complete);
    }

    #[test]
    fn diagnostic_shape_contains_counts_without_message_text_or_identifiers() {
        let parts = vec![super::super::ParsedProtoPartition {
            id: "private-partition".into(),
            messages: vec![
                super::super::ParsedProtoMsg {
                    sender_id: "private-sender".into(),
                    content: "private-content".into(),
                    msg_type: 3,
                    ..Default::default()
                },
                super::super::ParsedProtoMsg {
                    sender_id: "0".into(),
                    msg_type: 20,
                    ..Default::default()
                },
            ],
        }];
        let shape = phone_batch_shape(&parts);
        assert_eq!(shape["messages"], 2);
        assert_eq!(shape["sender_zero"], 1);
        let text = shape.to_string();
        assert!(
            !text.contains("private-partition")
                && !text.contains("private-sender")
                && !text.contains("private-content")
        );
    }

    #[test]
    fn intermediate_request_cannot_report_overall_success_and_counts_are_cumulative() {
        let mut first = ImportState {
            total_requests: 2,
            ..Default::default()
        };
        let progress = first.complete_batch(0, true, 12);
        assert!(progress.complete);
        assert!(!progress.transfer_complete());
        assert!(!progress.completion_message().contains("thành công"));
        let mut second = ImportState {
            request_index: 1,
            total_requests: 2,
            imported_before: progress.imported,
            ..Default::default()
        };
        let progress = second.complete_batch(0, true, 8);
        assert_eq!(progress.imported, 20);
        assert!(progress.transfer_complete());
        assert!(progress.completion_message().contains("thành công 20 tin"));
    }

    #[test]
    fn completed_import_discloses_threads_without_verified_global_ids() {
        let progress = V2BatchProgress {
            imported: 7,
            request_number: 1,
            request_total: 1,
            complete: true,
            skipped_threads: 2,
            unavailable_threads: 0,
        };
        let note = progress.completion_message();
        assert!(note.contains("7 tin nhắn từ điện thoại"));
        assert!(note.contains("2 hội thoại chưa lấy được định danh"));
    }

    #[test]
    fn transfer_file_splits_all_records_and_rejects_truncated_frames() {
        assert!(encrypted_records(&[]).unwrap().is_empty());
        let records = encrypted_records(&[2, 0, 0, 0, 10, 11, 1, 0, 0, 0, 12]).unwrap();
        assert_eq!(records, vec![&[10, 11][..], &[12][..]]);
        for invalid in [&[1, 0][..], &[0, 0, 0, 0][..], &[2, 0, 0, 0, 12][..]] {
            assert!(encrypted_records(invalid).is_err());
        }
    }

    #[test]
    fn duplicate_final_marker_can_finish_without_counting_rows_twice() {
        let mut state = ImportState::default();
        assert!(!state.complete_batch(0, false, 5).complete);
        let result = state.complete_batch(0, true, 5);
        assert!(result.complete);
        assert_eq!(result.imported, 5); // (INV-4)
    }

    #[test]
    fn session_identity_rejects_old_and_unidentified_controls() {
        let state = ImportState {
            sync_id: Some("current".into()),
            ..Default::default()
        };
        assert!(matches_session(&state, "current"));
        assert!(!matches_session(&state, "previous"));
        assert!(!matches_session(&state, ""));
    }
    #[test]
    fn completion_requires_every_index_and_duplicate_batches_do_not_increase_count() {
        let mut state = ImportState::default();
        assert!(!state.complete_batch(2, true, 4).complete);
        assert!(!state.complete_batch(0, false, 3).complete);
        assert!(!state.complete_batch(0, false, 3).complete);
        let end = state.complete_batch(1, false, 2);
        assert!(end.complete);
        assert_eq!(end.imported, 9);
    }
}
