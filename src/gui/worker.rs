use super::*;

/// Thread (id, name, is_group) list the poll timer maps incoming messages onto; the GUI
/// republishes it whenever its threads change.
pub(super) static KNOWN_THREADS: std::sync::Mutex<Vec<(String, String, bool)>> =
    std::sync::Mutex::new(Vec::new());

/// Keep the vanish sweep off disk when no conversation has expiring messages.
pub(super) static VANISH_THREADS: std::sync::Mutex<Vec<String>> =
    std::sync::Mutex::new(Vec::new());

/// Clear the polling and expiry lists before a different account can use them.
fn clear_account_thread_lists() {
    if let Ok(mut known) = KNOWN_THREADS.lock() {
        known.clear();
    }
    if let Ok(mut vanish) = VANISH_THREADS.lock() {
        vanish.clear();
    }
}

/// Process phone sync controls from WebSocket and long polling. Ordering metadata
/// first and deduplicating across both sources protects the decryption session.
pub(super) async fn handle_mobile_sync_controls(
    mut events: Vec<crate::chat::MobileSyncControlEvent>,
    s: Session,
    gui_tx: GuiTx,
    ctx: egui::Context,
    trigger_sync_on_login: bool,
) {
    let Some(login) = s.login.as_ref() else {
        return;
    };
    let uin = &login.uin;
    events.sort_by_key(|ev| match ev {
        crate::chat::MobileSyncControlEvent::SyncV2Batch { batch_type: 2, .. } => 0u8,
        crate::chat::MobileSyncControlEvent::SyncV2Batch { batch_type: 1, .. } => 2u8,
        _ => 1u8,
    });

    for sync_ev in events {
        if !gui_tx.is_current() {
            return;
        }
        match sync_ev {
            crate::chat::MobileSyncControlEvent::TransferAfterLogin { temp_key } => {
                crate::history_sync::set_latest_mobile_temp_key(&temp_key);
                if trigger_sync_on_login {
                    let known = crate::chat::load_threads_cache_for_account(uin);
                    queue_automatic_phone_sync(&gui_tx, uin, known);
                }
            }
            crate::chat::MobileSyncControlEvent::WaitingConfirmOnPhone { sync_id } => {
                if !sync_id.is_empty()
                    && crate::history_sync::active_v2_sync_id().as_deref()
                        != Some(sync_id.as_str())
                {
                    crate::logger::log_sync(&format!(
                        "bỏ qua WaitingConfirmOnPhone session cũ: sync={sync_id}"
                    ));
                    continue;
                }
                let note = "📲 Đang chờ bạn bấm 'Đồng bộ' trên màn hình Zalo điện thoại…".to_string();
                crate::history_sync::set_last_mobile_sync_status(Some(note.clone()));
                let _ = gui_tx.send(GuiEvent::StatusUpdate(note));
                ctx.request_repaint();
            }
            crate::chat::MobileSyncControlEvent::MobileBusyRestoring => {
                let note = if trigger_sync_on_login {
                    "⚠️ Điện thoại đang bận sao lưu/khôi phục phiên trước — hãy mở Zalo trên điện thoại xác nhận hoặc thử lại sau ít phút".to_string()
                } else {
                    "⏳ Điện thoại đang nén và chuẩn bị dữ liệu tin nhắn…".to_string()
                };
                crate::history_sync::set_last_mobile_sync_status(Some(note.clone()));
                let _ = gui_tx.send(GuiEvent::StatusUpdate(note));
                ctx.request_repaint();
            }
            crate::chat::MobileSyncControlEvent::UserConfirmed => {
                let note = "📲 Điện thoại đã xác nhận đồng bộ — đang nén và đóng gói tin nhắn…".to_string();
                crate::history_sync::set_last_mobile_sync_status(Some(note.clone()));
                let _ = gui_tx.send(GuiEvent::StatusUpdate(note));
                ctx.request_repaint();
            }
            crate::chat::MobileSyncControlEvent::UserRejected => {
                let note = "⚠️ Bạn đã từ chối yêu cầu đồng bộ tin nhắn trên điện thoại".to_string();
                crate::history_sync::set_last_mobile_sync_status(Some(note.clone()));
                let _ = gui_tx.send(GuiEvent::StatusUpdate(note));
                ctx.request_repaint();
            }
            crate::chat::MobileSyncControlEvent::BackupReady {
                url,
                file_name,
                encrypted_key,
                ..
            } => {
                let status_dl = format!("📥 Đang tải và giải mã gói đồng bộ tin nhắn ({file_name})…");
                crate::history_sync::set_last_mobile_sync_status(Some(status_dl.clone()));
                let _ = gui_tx.send(GuiEvent::StatusUpdate(status_dl));
                ctx.request_repaint();

                let result = crate::history_sync::import_mobile_backup_container(&s, &url, &encrypted_key).await;
                if !gui_tx.is_current() {
                    return;
                }
                match result {
                    Ok(count) => {
                        let note = format!("🎉 Đã giải mã & đồng bộ thành công {count} tin nhắn từ điện thoại vào ứng dụng!");
                        crate::history_sync::set_last_mobile_sync_status(Some(note.clone()));
                        let _ = gui_tx.send(GuiEvent::StatusUpdate(note));
                        let updated = crate::chat::load_threads_cache_for_account(uin);
                        let _ = gui_tx.send(GuiEvent::ThreadsUpdated(updated));
                    }
                    Err(e) => {
                        let err_note = format!("⚠️ Không thể giải mã gói đồng bộ ({file_name}): {e}");
                        crate::history_sync::set_last_mobile_sync_status(Some(err_note.clone()));
                        let _ = gui_tx.send(GuiEvent::StatusUpdate(err_note));
                    }
                }
                ctx.request_repaint();
            }
            crate::chat::MobileSyncControlEvent::SyncV2Batch {
                url,
                sync_id,
                batch_type,
                is_last,
            } => {
                if crate::history_sync::active_v2_sync_id().as_deref() != Some(sync_id.as_str()) {
                    crate::logger::log_sync(&format!(
                        "bỏ qua + dọn 591 batch cũ: sync={} type={}",
                        sync_id, batch_type
                    ));
                    crate::socket::send_sync_ack_delete(
                        &sync_id,
                        crate::socket::SYNC_DISPOSE_OUTDATED,
                    );
                    continue;
                }

                if !crate::history_sync::mark_sync_v2_batch_seen(&sync_id, batch_type, &url) {
                    crate::logger::log_sync(&format!(
                        "bỏ qua batch trùng: sync={} type={}",
                        sync_id, batch_type
                    ));
                    continue;
                }

                if batch_type == 1 && !crate::history_sync::has_sync_v2_session(&sync_id) {
                    crate::logger::log_sync(&format!(
                        "batch msg tới sớm: sync={}, chờ metadata tối đa 8s…",
                        sync_id
                    ));
                    for _ in 0..32 {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                        if !gui_tx.is_current() {
                            return;
                        }
                        if crate::history_sync::has_sync_v2_session(&sync_id) {
                            break;
                        }
                    }
                }

                let _guard = crate::history_sync::sync_v2_batch_lock().lock().await;
                if !gui_tx.is_current() {
                    return;
                }
                if crate::history_sync::active_v2_sync_id().as_deref() != Some(sync_id.as_str()) {
                    continue;
                }
                if batch_type == 2 {
                    let note = "📥 Đang giải mã thông tin phiên đồng bộ từ iPhone…".to_string();
                    crate::history_sync::set_last_mobile_sync_status(Some(note.clone()));
                    let _ = gui_tx.send(GuiEvent::StatusUpdate(note));
                    ctx.request_repaint();
                    crate::logger::log_sync(&format!("batch meta về: sync={}", sync_id));
                    let result = crate::history_sync::import_sync_v2_meta(&s, &url).await;
                    if !gui_tx.is_current() {
                        return;
                    }
                    match result {
                        Ok(meta) => {
                            crate::history_sync::record_sync_v2_session(&sync_id, &meta.session_id);
                            crate::logger::log_sync(&format!(
                                "meta OK: sync={} session={}",
                                sync_id, meta.session_id
                            ));
                            let ok_note = "📲 Đã giải mã thông tin phiên từ iPhone — đang chờ gói tin nhắn…".to_string();
                            crate::history_sync::set_last_mobile_sync_status(Some(ok_note.clone()));
                            let _ = gui_tx.send(GuiEvent::StatusUpdate(ok_note));
                        }
                        Err(e) => {
                            let err = format!("⚠️ Lỗi giải mã phiên Sync v2 ({sync_id}): {e}");
                            crate::logger::log_sync(&format!("meta FAIL: sync={} err={}", sync_id, e));
                            crate::history_sync::set_last_mobile_sync_status(Some(err.clone()));
                            let _ = gui_tx.send(GuiEvent::StatusUpdate(err));
                        }
                    }
                    ctx.request_repaint();
                } else if batch_type == 1 {
                    if !crate::history_sync::has_sync_v2_session(&sync_id) {
                        let err = format!(
                            "⚠️ Gói Sync v2 ({sync_id}) tới trước metadata — bỏ qua, chờ batch meta"
                        );
                        crate::logger::log_sync(&format!(
                            "batch msg FAIL (no meta): sync={}",
                            sync_id
                        ));
                        crate::history_sync::set_last_mobile_sync_status(Some(err.clone()));
                        let _ = gui_tx.send(GuiEvent::StatusUpdate(err));
                        ctx.request_repaint();
                        continue;
                    }
                    let note = "📥 Đang tải và giải mã gói tin nhắn từ iPhone…".to_string();
                    crate::history_sync::set_last_mobile_sync_status(Some(note.clone()));
                    let _ = gui_tx.send(GuiEvent::StatusUpdate(note));
                    ctx.request_repaint();

                    crate::logger::log_sync(&format!("batch msg về: sync={} last={}", sync_id, is_last));
                    let result = crate::history_sync::import_sync_v2_messages_inner(
                        &s,
                        &url,
                        Some(&sync_id),
                        is_last,
                    )
                    .await;
                    if !gui_tx.is_current() {
                        return;
                    }
                    match result {
                        Ok(count) => {
                            let total = crate::history_sync::add_sync_v2_imported_count(count);
                            crate::logger::log_sync(&format!(
                                "msg OK: sync={} count={} total={} last={}",
                                sync_id, count, total, is_last
                            ));
                            let fin = if is_last {
                                format!(
                                    "🎉 Đã giải mã & đồng bộ thành công {total} tin nhắn từ điện thoại vào ứng dụng!"
                                )
                            } else {
                                format!(
                                    "📥 Đã giải mã {total} tin nhắn (+{count} tin) — đang chờ gói tin tiếp theo từ iPhone…"
                                )
                            };
                            crate::history_sync::set_last_mobile_sync_status(Some(fin.clone()));
                            let _ = gui_tx.send(GuiEvent::StatusUpdate(fin));
                            let updated = crate::chat::load_threads_cache_for_account(uin);
                            let _ = gui_tx.send(GuiEvent::ThreadsUpdated(updated));
                        }
                        Err(e) => {
                            crate::logger::log_sync(&format!("msg FAIL: sync={} err={}", sync_id, e));
                            let err = format!("⚠️ Lỗi giải mã tin nhắn Sync v2 ({sync_id}): {e}");
                            crate::history_sync::set_last_mobile_sync_status(Some(err.clone()));
                            let _ = gui_tx.send(GuiEvent::StatusUpdate(err));
                        }
                    }
                    ctx.request_repaint();
                } else {
                    crate::logger::log_sync(&format!(
                        "batch unknown type: sync={} type={} last={}",
                        sync_id, batch_type, is_last
                    ));
                }
            }
            crate::chat::MobileSyncControlEvent::SyncV2EmptyDone { sync_id } => {
                if !sync_id.is_empty()
                    && crate::history_sync::active_v2_sync_id().as_deref() != Some(sync_id.as_str())
                {
                    continue;
                }
                crate::logger::log_sync(&format!(
                    "sync v2 empty done (isLast=1, no url): sync={sync_id}"
                ));
                let fin = "🎉 Đã đồng bộ thành công (không có tin nhắn mới cần tải thêm từ điện thoại)!".to_string();
                crate::history_sync::set_last_mobile_sync_status(Some(fin.clone()));
                let _ = gui_tx.send(GuiEvent::StatusUpdate(fin));
                ctx.request_repaint();
            }
            crate::chat::MobileSyncControlEvent::SyncV2MobileIdle { sync_id } => {
                if !sync_id.is_empty()
                    && crate::history_sync::active_v2_sync_id().as_deref() != Some(sync_id.as_str())
                {
                    continue;
                }
                let woke = if !sync_id.is_empty() {
                    crate::socket::send_mobile_wakeup(&sync_id)
                } else {
                    false
                };
                crate::logger::log_sync(&format!(
                    "phone idle (status=2): sync={sync_id} wakeup_592={woke}"
                ));
                let note = "📱 Zalo trên iPhone đang tạm nghỉ (nền) — hãy giữ màn hình Zalo trên iPhone mở để tiếp tục gửi tin nhắn…".to_string();
                crate::history_sync::set_last_mobile_sync_status(Some(note.clone()));
                let _ = gui_tx.send(GuiEvent::StatusUpdate(note));
                ctx.request_repaint();
            }
            crate::chat::MobileSyncControlEvent::SyncV2Error {
                sync_id,
                err,
                message,
                is_last,
            } => {
                if !sync_id.is_empty()
                    && crate::history_sync::active_v2_sync_id().as_deref() != Some(sync_id.as_str())
                {
                    crate::logger::log_sync(&format!(
                        "bỏ qua SyncV2Error session cũ: sync={sync_id} err={err} last={is_last}"
                    ));
                    continue;
                }
                crate::logger::log_sync(&format!(
                    "sync v2 error: sync={sync_id} err={err} last={is_last} msg={message}"
                ));
                let total = crate::history_sync::sync_v2_imported_count();
                let note = if !is_last {
                    // Cảnh báo cục bộ 1 phân vùng (vd OA `errorCode: 502`, `isLast: 0`):
                    // không đặt chữ "Lỗi" để tiến trình đồng bộ tiếp tục chờ các batch thật phía sau.
                    format!(
                        "⏳ Điện thoại bỏ qua 1 phân vùng (mã {err}) — đang tiếp tục nhận các gói tin còn lại…"
                    )
                } else if total > 0 {
                    format!(
                        "🎉 Đã giải mã & đồng bộ thành công {total} tin nhắn từ điện thoại vào ứng dụng!"
                    )
                } else {
                    format!("⚠️ Lỗi đồng bộ từ điện thoại (mã {err}): {message}")
                };
                crate::history_sync::set_last_mobile_sync_status(Some(note.clone()));
                let _ = gui_tx.send(GuiEvent::StatusUpdate(note));
                ctx.request_repaint();
            }
        }
    }
}

// Async tasks can return early or be cancelled during account transitions.
struct RunningPermit(&'static std::sync::atomic::AtomicBool);

impl Drop for RunningPermit {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

struct VoiceRecording {
    path: std::path::PathBuf,
    dir: Option<std::path::PathBuf>,
}

impl Drop for VoiceRecording {
    /// Remove temporary recording files even when an account transition cancels upload.
    fn drop(&mut self) {
        // Removal is best effort and also runs when an account switch cancels the upload.
        let _ = std::fs::remove_file(&self.path);
        if let Some(dir) = &self.dir {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

/// Queue a sync without prompting the phone, preserving the snapshot's original account scope.
fn queue_automatic_phone_sync(scope: &GuiTx, uin: &str, threads: Vec<Thread>) {
    if let Some(tx) = WORKER_TX.get() {
        // The original scope travels with the snapshot in case a switch is queued ahead of it.
        let _ = tx.send_for_account(
            WorkerCmd::SyncPhoneMessages { threads, send_phone_prompt: false },
            uin,
            scope,
        );
    }
}

/// Route phone sync controls through the account task queue so a transition can
/// cancel downloads and imports tied to the supplied session.
fn queue_mobile_sync_controls(
    events: Vec<crate::chat::MobileSyncControlEvent>,
    session: &Session,
    scope: &GuiTx,
    trigger_sync_on_login: bool,
) {
    if events.is_empty() {
        return;
    }
    let Some(login) = session.login.as_ref() else {
        return;
    };
    if let Some(tx) = WORKER_TX.get() {
        // Dispatch through the account task set so transitions can cancel downloads and imports.
        let _ = tx.send_for_account(
            WorkerCmd::HandleMobileSyncControls {
                events,
                session: Box::new(session.clone()),
                trigger_sync_on_login,
            },
            &login.uin,
            scope,
        );
    }
}

// Track the blocking closure itself: cancelling an async waiter cannot stop its disk write.
fn spawn_history_write<T: Send + 'static>(
    writes: &mut tokio::task::JoinSet<()>,
    write: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> tokio::sync::oneshot::Receiver<anyhow::Result<T>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    writes.spawn_blocking(move || {
        // The waiter may be cancelled during a wipe; the write remains tracked until exit.
        let _ = tx.send(write());
    });
    rx
}

#[derive(Clone)]
struct ScopedHistoryWriter {
    sender: tokio::sync::mpsc::UnboundedSender<Box<dyn FnOnce() + Send>>,
}

impl ScopedHistoryWriter {
    fn new(writes: &mut tokio::task::JoinSet<()>) -> Self {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<Box<dyn FnOnce() + Send>>();
        // The account task owns the sender. Aborting that task closes the queue, while
        // the wipe still waits for the blocking jobs already submitted to the receiver.
        writes.spawn(async move {
            while let Some(write) = receiver.recv().await {
                if let Err(error) = tokio::task::spawn_blocking(write).await {
                    crate::logger::log_error("history::write", &error.to_string());
                }
            }
        });
        Self { sender }
    }

    async fn write<T: Send + 'static>(
        &self,
        write: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
    ) -> anyhow::Result<T> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.sender.send(Box::new(move || {
            // A cancelled account task may no longer need the result, but wipe waits for the write.
            let _ = tx.send(write());
        })).map_err(|_| anyhow::anyhow!("Không thể ghi lịch sử."))?;
        rx.await?
    }
}

/// Cancel history tasks and drain their blocking writes before local rows can be deleted.
async fn stop_history_tasks(
    tasks: &mut tokio::task::JoinSet<()>,
    writes: &mut tokio::task::JoinSet<()>,
) {
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    // Already-running blocking jobs are not abortable. Finish them before deleting rows.
    while writes.join_next().await.is_some() {}
}

async fn restore_failed_switch(
    session: &std::sync::Arc<tokio::sync::Mutex<Session>>,
    previous: Session,
    events: &GuiTx,
    error: &str,
) {
    *session.lock().await = previous;
    // The UI must rebind its command sender even though its displayed UIN did not change.
    let _ = events.send(GuiEvent::AccountSwitchFailed(format!(
        "⚠️ Lỗi chuyển tài khoản: {error}"
    )));
}

async fn prepare_add_account(uin: &str, pending_notes: Vec<(String, String)>, events: &GuiTx) -> bool {
    let uin = uin.to_owned();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        crate::account_manager::save_current_account_snapshot(&uin)?;
        if !pending_notes.is_empty() {
            let mut store = crate::store::MessageStore::open_account(&uin)?;
            for (thread_id, draft) in pending_notes {
                store.set_group_note(&thread_id, &draft, chrono::Local::now().timestamp())?;
            }
        }
        // The saved copy must be restorable before the active session is removed.
        std::fs::remove_file("session.json")?;
        Ok(())
    })
    .await
    .unwrap_or_else(|error| Err(error.into()));
    if let Err(error) = result {
        // A disconnected UI can no longer display the failure.
        let _ = events.send(GuiEvent::AddAccountFailed(format!(
            "⚠️ Không lưu được phiên hiện tại: {error}"
        )));
        return false;
    }
    true
}

pub enum WorkerCmd {
    /// Discover native audio/video devices outside the UI thread.
    RefreshMediaDevices,
    /// Discover saved account profiles outside the rendering thread.
    RefreshSavedAccounts {
        request_id: u64,
    },
    /// Delete an inactive saved account and refresh the profile snapshot.
    DeleteSavedAccount {
        request_id: u64,
        target_uin: String,
    },
    /// Read OS authorization without prompting.
    ReadNotificationAuthorization {
        request_id: u64,
    },
    /// Request OS authorization following an explicit Settings click.
    RequestNotificationAuthorization {
        request_id: u64,
    },
    RequestQr,
    LoginPassword {
        phone: String,
        pass: String,
    },
    RefreshThreads,
    RefreshThreadsManual,
    PollMessages {
        known_threads: Vec<(String, String, bool)>,
    },
    LoadGroupHistory {
        group_id: String,
        is_group: bool,
        /// Messages the GUI already holds. SQLite is read `have + 300` deep, so each
        /// "Tải thêm" reaches further back instead of re-reading the same newest 500.
        have: usize,
    },
    /// Page group / Cloud history back from the server into SQLite: (id, name, is_group).
    SyncHistory {
        threads: Vec<(String, String, bool)>,
    },
    SendMessage {
        to_id: String,
        text: String,
        is_group: bool,
    },
    SendQuote {
        to_id: String,
        text: String,
        quoted: ChatMessage,
        is_group: bool,
    },
    Forward {
        target_id: String,
        target_is_group: bool,
        msg: ChatMessage,
        target_name: String,
    },
    React {
        to_id: String,
        is_group: bool,
        msg: ChatMessage,
        reaction: String,
    },
    DeleteForMe {
        to_id: String,
        is_group: bool,
        msg: ChatMessage,
    },
    RecallMessage {
        to_id: String,
        is_group: bool,
        msg: ChatMessage,
    },
    /// Sửa tin (WaveA3): recall tin cũ rồi gửi tin mới; worker báo
    /// `GuiEvent::MessageEdited` để GUI gắn cờ + lưu bản cũ.
    EditMessageCommit {
        to_id: String,
        is_group: bool,
        old_msg: ChatMessage,
        new_text: String,
    },
    SendSticker {
        to_id: String,
        sticker_id: u64,
        cate_id: u64,
        sticker_type: u32,
        is_group: bool,
    },
    /// Open a native picker and queue the chosen files in the composer.
    PickAttachments {
        mode: super::attachments::PickMode,
    },
    /// Decode previews for already-known paths (clipboard file list, drag-and-drop).
    PrepareAttachments {
        paths: Vec<std::path::PathBuf>,
    },
    /// Upload and send queued files one after another. Still photos are moved together to
    /// the first photo's position (see `attachments::album_send_order`) so they form one
    /// album; everything else keeps its order. `caption` goes on the first photo, or, when
    /// the queue has no photo, is sent as a text message after the files.
    SendAttachments {
        to_id: String,
        is_group: bool,
        paths: Vec<std::path::PathBuf>,
        caption: String,
        original_hd: bool,
    },
    TriggerScreenshot,
    SendVoice {
        to_id: String,
        is_group: bool,
        voice_path: std::path::PathBuf,
        voice_dir: Option<std::path::PathBuf>,
        mime: String,
        duration_secs: u64,
    },
    SendTyping {
        thread_id: String,
        is_group: bool,
    },
    FetchStickerImage {
        key: String,
        url: String,
    },
    FetchStickerStore,
    FetchStickerPack {
        cate_id: u64,
        name: String,
    },
    SearchStickers {
        keyword: String,
    },
    SuggestComposerStickers {
        keyword: String,
    },
    ParseLinkPreview {
        url: String,
    },
    TranslateMessage {
        thread_id: String,
        msg_id: String,
        text: String,
        target_lang: String,
        /// Nguồn do user chọn ("auto" = heuristic), đi kèm để worker báo đúng.
        source_lang: String,
        /// Generation do GUI cấp, tăng mỗi lần bấm Dịch trên cùng tin.
        generation: u64,
    },
    LookupPhone {
        phone: String,
    },
    StartCall {
        partner_id: String,
        partner_name: String,
        partner_avatar: Option<String>,
        is_video: bool,
        is_group: bool,
    },
    AnswerCall {
        accept: bool,
        cam_start_off: bool,
    },
    SetCallMediaState {
        is_on_mic: bool,
        is_on_cam: bool,
    },
    InviteGroupCallMembers {
        user_ids: Vec<String>,
    },
    EndCall,
    AddFriend {
        user_id: String,
        user_name: String,
    },
    CheckFriendStatus {
        user_id: String,
    },
    AcceptFriend {
        user_id: String,
        user_name: String,
    },
    RejectFriend {
        user_id: String,
        user_name: String,
    },
    UndoFriend {
        user_id: String,
        user_name: String,
    },
    PersistThreads {
        uin: String,
        threads: Vec<Thread>,
    },
    /// Import a selected local backup into the originating account's history.
    ImportHistory {
        uin: String,
        path: std::path::PathBuf,
    },
    /// Cancel a schedule in the account that created the UI action.
    CancelScheduledMessage {
        uin: String,
        id: i64,
    },
    /// Make an unclaimed schedule due through the scheduler's single delivery path.
    SendScheduledMessageNow {
        uin: String,
        id: i64,
    },
    /// Load the originating account's schedule on a blocking worker.
    LoadScheduledMessages {
        uin: String,
        notify_sent: bool,
    },
    CheckForUpdates {
        silent: bool,
    },
    DownloadAndApplyUpdate {
        download_url: String,
        asset_name: String,
        target_version: String,
        expected_sha256: Option<String>,
    },
    Logout,
    /// Preserve this account's saved session before opening login for another account.
    /// Save the current account and pending group drafts before opening another login.
    BeginAddAccount {
        pending_notes: Vec<(String, String)>,
    },
    /// Clear the account history database and thread cache without
    /// touching credentials. Separate from [`WorkerCmd::Logout`] by design (#21).
    WipeLocalData,
    PinThread {
        thread_id: String,
        is_group: bool,
        pin: bool,
    },
    /// Reload pinned board items for one conversation (server is the source of truth).
    RefreshBoardPins {
        thread_id: String,
        is_group: bool,
    },
    /// Pin/unpin a single chat message through the group-board API (#119 follow-up).
    PinMessage {
        thread_id: String,
        is_group: bool,
        msg: crate::chat::ChatMessage,
        pin: bool,
        /// Board topic id, required to unpin a previously pinned message.
        topic_id: String,
    },
    MarkThreadUnread {
        thread_id: String,
        is_group: bool,
        unread: bool,
    },
    MuteThread {
        thread_id: String,
        is_group: bool,
        duration_sec: i64,
        mute: bool,
    },
    HideThread {
        thread_id: String,
        is_group: bool,
        hide: bool,
    },
    /// Bật/tắt tin nhắn tự xoá local-only (`duration_sec <= 0` = tắt).
    SetThreadVanish {
        thread_id: String,
        duration_sec: i64,
    },
    DeleteThread {
        thread_id: String,
        is_group: bool,
        last_cli_msg_id: String,
    },
    /// Rời nhóm Zalo (endpoint `/api/group/leave`, cmd 11189)
    LeaveGroup {
        group_id: String,
        silent: bool,
    },
    FetchSocialProfile {
        target_uid: String,
    },
    FetchGroupMembers {
        group_id: String,
    },
    FetchMySocialAvatars {
        page: u32,
        count: u32,
    },
    FetchTimelineFeed {
        friend_uids: Vec<String>,
        shared_media_by_uid: std::collections::HashMap<String, Vec<crate::feed::FeedMedia>>,
    },
    SyncPhoneMessages {
        threads: Vec<Thread>,
        send_phone_prompt: bool,
    },
    /// Process phone sync controls under the originating account's lifetime.
    HandleMobileSyncControls {
        events: Vec<crate::chat::MobileSyncControlEvent>,
        session: Box<Session>,
        trigger_sync_on_login: bool,
    },
    SetProxy {
        proxy_url: String,
    },
    DownloadFile {
        url: String,
        file_name: String,
        download_dir: String,
    },
    /// Tải bytes tài liệu (giới hạn 10MB) + trích text xem trước trong app.
    FetchDocPreview {
        msg_id: String,
        url: String,
        file_name: String,
    },
    FetchVideoThumbnail {
        key: String,
        url: String,
    },
    SendSeen {
        thread_id: String,
        is_group: bool,
        last_msg_id: String,
    },
    SwitchAccount {
        target_uin: String,
    },
}

struct QueuedWorkerCommand {
    command: WorkerCmd,
    origin_uin: Option<String>,
    origin_events: Option<GuiTx>,
}

impl QueuedWorkerCommand {
    fn origin_is_current(&self) -> bool {
        self.origin_events.as_ref().is_none_or(GuiTx::is_current)
    }
    fn origin_matches(&self, uin: &str) -> bool {
        self.origin_uin
            .as_deref()
            .is_none_or(|origin| origin == uin)
    }

    fn reject(self, events: &GuiTx) {
        if let Some(notice) = self.command.stale_notice() {
            // Shutdown can close the receiver while stale work is being discarded.
            let _ = events.send(notice);
        }
        if let WorkerCmd::SendVoice { voice_path, voice_dir, .. } = self.command {
            drop(VoiceRecording { path: voice_path, dir: voice_dir });
        }
    }

    /// Enforce the enqueue-time account at dispatch. Authentication can replace the
    /// identity, and cleanup can proceed after the worker has cleared it.
    fn accepts_account(&self, current_uin: &str) -> bool {
        // Authentication replaces an identity; its enqueue-time epoch is checked first.
        if matches!(
            self.command,
            WorkerCmd::RequestQr | WorkerCmd::LoginPassword { .. }
        ) {
            return true;
        }
        self.origin_matches(current_uin)
            || (current_uin.is_empty()
                && matches!(self.command, WorkerCmd::Logout | WorkerCmd::BeginAddAccount { .. }))
    }
}

pub(super) struct WorkerTx {
    sender: tokio::sync::mpsc::UnboundedSender<QueuedWorkerCommand>,
    // Updated only when the UI displays a new identity, independently of worker transitions.
    visible_account: std::sync::Mutex<(String, GuiTx)>,
    root_events: GuiTx,
}

impl WorkerTx {
    fn new(
        sender: tokio::sync::mpsc::UnboundedSender<QueuedWorkerCommand>,
        uin: String,
        events: &GuiTx,
    ) -> Self {
        Self {
            sender,
            visible_account: std::sync::Mutex::new((uin, events.scoped())),
            root_events: events.clone(),
        }
    }

    /// Bind future GUI commands to the account and generation now shown on screen.
    pub(super) fn set_visible_account(&self, uin: &str) {
        if let Ok(mut visible) = self.visible_account.lock() {
            *visible = (uin.to_string(), self.root_events.scoped());
        }
    }

    /// Refresh cancelled work scopes without changing the authenticated identity.
    pub(super) fn rebind_visible_scope(&self) {
        if let Ok(mut visible) = self.visible_account.lock() {
            visible.1 = self.root_events.scoped();
        }
    }

    /// Queue a command with the account and work scope visible to the GUI at enqueue time.
    /// App-wide commands carry no account binding.
    ///
    /// # Errors
    /// Returns an error if the visible-account lock is poisoned or the worker queue is closed.
    pub(super) fn send(
        &self,
        command: WorkerCmd,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<()>> {
        let (origin_uin, origin_events) = if command.is_app_wide() {
            (None, None)
        } else {
            let visible = self
                .visible_account
                .lock()
                .map_err(|_| tokio::sync::mpsc::error::SendError(()))?;
            (Some(visible.0.clone()), Some(visible.1.clone()))
        };
        self.sender
            .send(QueuedWorkerCommand {
                command,
                origin_uin,
                origin_events,
            })
            .map_err(|_| tokio::sync::mpsc::error::SendError(()))
    }

    fn send_for_account(
        &self,
        command: WorkerCmd,
        uin: &str,
        events: &GuiTx,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<()>> {
        self.sender
            .send(QueuedWorkerCommand {
                command,
                origin_uin: Some(uin.to_string()),
                origin_events: Some(events.clone()),
            })
            .map_err(|_| tokio::sync::mpsc::error::SendError(()))
    }

    /// Queue follow-up work with its original scope, silently skipping an obsolete scope.
    ///
    /// # Errors
    /// Returns an error if the visible-account lock is poisoned or the worker queue is closed.
    fn send_if_current(
        &self,
        command: WorkerCmd,
        events: &GuiTx,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<()>> {
        let origin = self
            .visible_account
            .lock()
            .map(|visible| visible.0.clone())
            .map_err(|_| tokio::sync::mpsc::error::SendError(()))?;
        if !events.is_current() {
            return Ok(());
        }
        self.send_for_account(command, &origin, events)
    }
}

impl WorkerCmd {
    fn stale_notice(&self) -> Option<GuiEvent> {
        // Background reads must not overwrite the status of the transition that cancelled them.
        let user_action = matches!(
            self,
            Self::RefreshThreadsManual
                | Self::LoginPassword { .. }
                | Self::SendMessage { .. }
                | Self::SendQuote { .. }
                | Self::Forward { .. }
                | Self::React { .. }
                | Self::DeleteForMe { .. }
                | Self::RecallMessage { .. }
                | Self::SendSticker { .. }
                | Self::SendAttachments { .. }
                | Self::SendVoice { .. }
                | Self::TriggerScreenshot
                | Self::StartCall { .. }
                | Self::AnswerCall { .. }
                | Self::SetCallMediaState { .. }
                | Self::InviteGroupCallMembers { .. }
                | Self::EndCall
                | Self::AddFriend { .. }
                | Self::AcceptFriend { .. }
                | Self::RejectFriend { .. }
                | Self::UndoFriend { .. }
                | Self::DeleteSavedAccount { .. }
                | Self::ImportHistory { .. }
                | Self::CancelScheduledMessage { .. }
                | Self::SendScheduledMessageNow { .. }
                | Self::Logout
                | Self::BeginAddAccount { .. }
                | Self::WipeLocalData
                | Self::SwitchAccount { .. }
                | Self::PinThread { .. }
                | Self::PinMessage { .. }
                | Self::MarkThreadUnread { .. }
                | Self::MuteThread { .. }
                | Self::HideThread { .. }
                | Self::DeleteThread { .. }
                | Self::SyncHistory { .. }
                | Self::SyncPhoneMessages {
                    send_phone_prompt: true,
                    ..
                }
                | Self::SetProxy { .. }
                | Self::DownloadFile { .. }
        );
        user_action.then(|| {
            GuiEvent::StatusUpdate(
                "Dữ liệu tài khoản đã thay đổi; vui lòng thực hiện lại thao tác.".into(),
            )
        })
    }

    fn is_transition(&self) -> bool {
        matches!(
            self,
            Self::Logout
                | Self::BeginAddAccount { .. }
                | Self::SwitchAccount { .. }
                | Self::RequestQr
                | Self::LoginPassword { .. }
        )
    }
    fn is_app_wide(&self) -> bool {
        matches!(
            self,
            Self::ReadNotificationAuthorization { .. }
                | Self::RequestNotificationAuthorization { .. }
                | Self::CheckForUpdates { .. }
                | Self::DownloadAndApplyUpdate { .. }
                | Self::RefreshMediaDevices
        )
    }

    fn needs_session_snapshot(&self) -> bool {
        // Only authentication and guarded session-maintenance flows update the live session.
        !matches!(
            self,
            Self::RequestQr
                | Self::LoginPassword { .. }
                | Self::SetProxy { .. }
                | Self::RefreshThreads
                | Self::RefreshThreadsManual
                | Self::PollMessages { .. }
        )
    }
}

pub(super) static WORKER_TX: std::sync::OnceLock<WorkerTx> = std::sync::OnceLock::new();

/// Allow incoming transitions and app-wide commands to bypass deferred account work.
/// Keep ordinary commands in FIFO order and retry the oldest when the queue is quiet.
async fn next_worker_command(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<QueuedWorkerCommand>,
    deferred: &mut std::collections::VecDeque<QueuedWorkerCommand>,
) -> Option<QueuedWorkerCommand> {
    loop {
        tokio::select! {
            biased;
            next = receiver.recv() => {
                let next = next?;
                if !deferred.is_empty() && !next.command.is_app_wide() && !next.command.is_transition() {
                    deferred.push_back(next);
                } else {
                    return Some(next);
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(10)), if !deferred.is_empty() => {
                return deferred.pop_front();
            }
        }
    }
}

/// Return `Some(None)` for app-wide work. Account commands take a session snapshot
/// when the lock is free and defer dispatch on `None`.
fn try_command_snapshot(
    command: &WorkerCmd,
    live: &std::sync::Arc<tokio::sync::Mutex<Session>>,
) -> Option<Option<Session>> {
    if command.is_app_wide() {
        Some(None)
    } else {
        live.try_lock().ok().map(|session| Some(session.clone()))
    }
}

/// Keep ordinary actions on their dispatch-time session across account switches.
/// Authentication and session maintenance share the live session.
fn session_for_command(
    command: &WorkerCmd,
    snapshot: Option<&Session>,
    live: &std::sync::Arc<tokio::sync::Mutex<Session>>,
) -> std::sync::Arc<tokio::sync::Mutex<Session>> {
    if command.needs_session_snapshot() {
        if let Some(snapshot) = snapshot {
            return std::sync::Arc::new(tokio::sync::Mutex::new(snapshot.clone()));
        }
    }
    live.clone()
}

/// Start a new event generation for authentication, ending the previous account's
/// call and invalidating the thread list used by polling.
fn authentication_events(
    command: &WorkerCmd,
    events: GuiTx,
    call_bridge: &crate::call::CallBridge,
) -> GuiTx {
    if matches!(
        command,
        WorkerCmd::RequestQr | WorkerCmd::LoginPassword { .. }
    ) {
        let next = events.advance_account();
        call_bridge.end_call();
        if let Ok(mut known) = KNOWN_THREADS.lock() {
            known.clear();
        }
        let _ = next.send(GuiEvent::AuthenticationRestarted);
        next
    } else {
        events
    }
}

fn departing_account_uin(session: &Session, origin_uin: &str) -> String {
    session
        .login
        .as_ref()
        .map(|login| login.uin.clone())
        .unwrap_or_else(|| origin_uin.to_string())
}

pub(super) fn extract_msg_id(resp: &serde_json::Value) -> String {
    resp.get("data")
        .and_then(|d| d.get("msgId"))
        .and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

pub(super) fn resp_error_code(resp: &serde_json::Value) -> Option<i64> {
    resp.get("error_code")
        .and_then(serde_json::Value::as_i64)
        .filter(|c| *c != 0)
}

/// Immediately decode any disk-cached sticker thumbnails (`<media cache>/stk_{cid}_{sid}.png`)
/// and push `GuiEvent::StickerImageReady` *before* sending sticker list metadata, returning any
/// missing `(key, url, cache_path)` entries for concurrent network prefetching.
pub(super) fn preload_disk_sticker_thumbnails(
    items: &[crate::chat::StickerItem],
    gui_tx: &GuiTx,
    limit: usize,
) -> Vec<(String, String, std::path::PathBuf)> {
    let cache_dir = crate::account_manager::ensure_media_cache_dir();

    let mut missing = Vec::new();
    for item in items.iter().take(limit) {
        let key = format!("stk_{}_{}", item.cate_id, item.sticker_id);
        let cache_path = cache_dir.join(format!("{key}.png"));
        if let Ok(disk_bytes) = std::fs::read(&cache_path) {
            if let Ok(dyn_img) = image::load_from_memory(&disk_bytes) {
                let rgba = dyn_img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.as_flat_samples();
                let color_img = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
                let _ = gui_tx.send(GuiEvent::StickerImageReady {
                    key,
                    image: color_img,
                });
                continue;
            } else {
                let _ = std::fs::remove_file(&cache_path);
            }
        }
        let url = if item.cate_id == 0 && (1..=30).contains(&item.sticker_id) {
            format!(
                "https://stc-chat.zdn.vn/images/stickers/default/thumb/{}.png",
                item.sticker_id
            )
        } else {
            format!(
                "https://api.zaloapp.com/api/emoticon/sticker/webpc?eid={}&size=130",
                item.sticker_id
            )
        };
        missing.push((key, url, cache_path));
    }
    missing
}

/// Concurrently download, cache, and decode missing sticker thumbnails so the popover and search
/// grid render smoothly without waiting for frame-by-frame lazy requests.
async fn prefetch_missing_sticker_thumbnails(
    http: &reqwest::Client,
    missing: Vec<(String, String, std::path::PathBuf)>,
    gui_tx: &GuiTx,
    egui_ctx: &egui::Context,
) {
    if missing.is_empty() {
        return;
    }
    let futs = missing.into_iter().map(|(key, url, cache_path)| {
        let client = http.clone();
        let tx = gui_tx.clone();
        let ctx = egui_ctx.clone();
        async move {
            if let Ok(resp) = client.get(&url).send().await {
                if resp.status().is_success() {
                    if let Ok(b) = resp.bytes().await {
                        let vec = b.to_vec();
                        if let Ok(dyn_img) = image::load_from_memory(&vec) {
                            let _ = std::fs::write(&cache_path, &vec);
                            let rgba = dyn_img.to_rgba8();
                            let size = [rgba.width() as usize, rgba.height() as usize];
                            let pixels = rgba.as_flat_samples();
                            let color_img =
                                egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
                            let _ = tx.send(GuiEvent::StickerImageReady {
                                key,
                                image: color_img,
                            });
                            ctx.request_repaint();
                        }
                    }
                }
            }
        }
    });
    futures_util::future::join_all(futs).await;
}

/// Register the GUI command sender and start the background runtime for session work.
///
/// # Panics
/// Panics if the worker thread cannot start. Runtime initialization failure panics
/// on the worker thread.
pub(super) fn spawn_session_worker(session: Session, gui_tx: GuiTx, egui_ctx: egui::Context) {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<QueuedWorkerCommand>();
    let visible_uin = session
        .login
        .as_ref()
        .map(|login| login.uin.clone())
        .unwrap_or_default();
    let _ = WORKER_TX.set(WorkerTx::new(cmd_tx, visible_uin, &gui_tx));
    if let Some(tx) = WORKER_TX.get() {
        // Cache-only UI reads can start immediately while driver discovery runs here.
        let _ = tx.send(WorkerCmd::RefreshMediaDevices);
    }

    let session = std::sync::Arc::new(tokio::sync::Mutex::new(session));
    let mut poll_state =
        std::sync::Arc::new(tokio::sync::Mutex::new(crate::chat::PollState::default()));

    std::thread::Builder::new()
        .name("zalo-worker".into())
        .spawn(move || {
            // Lightweight 2-thread async runtime to keep memory & CPU footprint minimal
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async move {
                {
                    let s = session.lock().await.clone();
                    if let Some(login) = &s.login {
                        let uin = login.uin.clone();
                        if let Err(error) = tokio::task::spawn_blocking(move || {
                            crate::store::MessageStore::open_account(&uin).map(|_| ())
                        }).await.unwrap_or_else(|error| Err(error.into())) {
                            crate::logger::log_error("history::migration", &error.to_string());
                        }
                        let _ = gui_tx.send(GuiEvent::ScheduledListUpdated);
                    }
                    let _ = crate::login::get_server_info(&s).await;
                }
                // Message poll timer. It used to live in the GUI's update(), which forced a full
                // repaint every 1.2 s just to fire it. The PollMessages handler skips while a
                // long-poll is in flight and when logged out.
                tokio::spawn(async {
                    let mut tick = tokio::time::interval(Duration::from_millis(1200));
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        let known_threads =
                            KNOWN_THREADS.lock().map(|k| k.clone()).unwrap_or_default();
                        if let Some(tx) = WORKER_TX.get() {
                            let _ = tx.send(WorkerCmd::PollMessages { known_threads });
                        }
                    }
                });

                // Scheduled messages worker (#38) + optional HTTP REST API server (#35)
                if let Ok(port_str) = std::env::var("CTDOZLITE_API_PORT") {
                    if let Ok(port) = port_str.parse::<u16>() {
                        if port > 0 {
                            let s_api = session.clone();
                            tokio::spawn(async move {
                                let _ = crate::api_mcp::run_local_http_server(
                                    s_api,
                                    port,
                                    "zalo_history.db".into(),
                                )
                                .await;
                            });
                        }
                    }
                }
                {
                    let s_sched = session.clone();
                    let gui_tx_sched = gui_tx.clone();
                    let ctx_sched = egui_ctx.clone();
                    tokio::spawn(async move {
                        // Keep account ownership across switches; this sequential loop finishes
                        // every active delivery before visiting another account.
                        let mut delivery_workers = std::collections::HashMap::new();
                        let mut tick = tokio::time::interval(Duration::from_secs(2));
                        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                        loop {
                            tick.tick().await;
                            let gui_tx_sched = gui_tx_sched.scoped();
                            let s = s_sched.lock().await.clone();
                            let Some(login) = s.login.as_ref() else { continue; };
                            let db_path = crate::store::db_path_for_account(&login.uin);
                            let worker = match delivery_workers.entry(db_path.clone()) {
                                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                                std::collections::hash_map::Entry::Vacant(entry) => {
                                    let startup_path = db_path.clone();
                                    match tokio::task::spawn_blocking(move || {
                                        crate::scheduler::DeliveryWorker::start(&startup_path)
                                    }).await.unwrap_or_else(|error| Err(error.into())) {
                                        Ok(worker) => {
                                            let _ = gui_tx_sched.send(GuiEvent::ScheduledListUpdated);
                                            entry.insert(worker)
                                        }
                                        Err(error) => {
                                            crate::logger::log_error("scheduler::startup", &error.to_string());
                                            continue;
                                        }
                                    }
                                }
                            };
                            let now_ts = crate::login::unix_now() as i64;
                            let due = worker.claim_due(now_ts)
                            .unwrap_or_default();
                            if due.is_empty() {
                                continue;
                            }
                            for item in due {
                                let client_id = crate::chat::next_client_message_id();
                                let send_res = if item.is_group {
                                    crate::chat::send_group_message_with_client_id(&s, &item.thread_id, &item.text, client_id)
                                        .await
                                } else {
                                    crate::chat::send_message_with_client_id(&s, &item.thread_id, &item.text, client_id).await
                                };
                                match send_res {
                                    Ok(resp) => {
                                        if let Some(code) = resp_error_code(&resp) {
                                            let _ = crate::scheduler::mark_scheduled_failed(
                                                &db_path,
                                                item.id,
                                                &format!("Mã lỗi Zalo {code}"),
                                            );
                                        } else {
                                            let msg_id = extract_msg_id(&resp);
                                            let _ = crate::scheduler::mark_scheduled_sent(
                                                &db_path,
                                                item.id,
                                            );
                                            let _ = gui_tx_sched.send(GuiEvent::MessageSent {
                                                client_message_id: client_id.to_string(),
                                                thread_id: item.thread_id.clone(),
                                                text: item.text.clone(),
                                                msg_id,
                                            });
                                        }
                                    }
                                    Err(e) => {
                                        let _ = crate::scheduler::mark_scheduled_failed(
                                            &db_path,
                                            item.id,
                                            &e.to_string(),
                                        );
                                    }
                                }
                            }
                            let _ = gui_tx_sched.send(GuiEvent::ScheduledListUpdated);
                            ctx_sched.request_repaint();
                        }
                    });
                }
                {
                    // Vanish sweep (tin nhắn tự xoá local-only): nhịp tick 2s riêng.
                    // RẺ: early-return ngay khi `VANISH_THREADS` rỗng (cache memory, không
                    // query DB mỗi tick). Chỉ thread có vanish mới đọc SQLite + gửi event.
                    let gui_tx_vanish = gui_tx.clone();
                    let ctx_vanish = egui_ctx.clone();
                    tokio::spawn(async move {
                        let mut tick = tokio::time::interval(Duration::from_secs(2));
                        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                        loop {
                            tick.tick().await;
                            let watch: Vec<String> = VANISH_THREADS
                                .lock()
                                .map(|v| v.clone())
                                .unwrap_or_default();
                            if watch.is_empty() {
                                continue;
                            }
                            let now_ms = crate::login::unix_now().saturating_mul(1000);
                            let swept: Vec<(String, Vec<String>)> =
                                tokio::task::spawn_blocking(move || {
                                    let mut out = Vec::new();
                                    let Ok(store) =
                                        crate::store::MessageStore::open("zalo_history.db")
                                    else {
                                        return out;
                                    };
                                    for tid in watch {
                                        let Ok(msgs) = store.messages_for(&tid, 500) else {
                                            continue;
                                        };
                                        if msgs.is_empty() {
                                            continue;
                                        }
                                        let Some((dur, start)) =
                                            store.load_thread_vanish(&tid)
                                        else {
                                            continue;
                                        };
                                        let setting = crate::chat::VanishSetting {
                                            duration_sec: dur,
                                            started_at_sec: start,
                                        };
                                        let dur_ms = dur.saturating_mul(1000);
                                        let start_ms = start.saturating_mul(1000);
                                        let mut expired_ids = Vec::new();
                                        for sm in msgs {
                                            // Tombstone đã persist (text + marker): bỏ qua,
                                            // không gửi event lại mỗi 2s (#1908).
                                            if sm.text.trim()
                                                == crate::chat::VANISH_EXPIRED_TEXT
                                                || sm.raw_json.trim()
                                                    == crate::store::VANISH_TOMBSTONE_JSON
                                            {
                                                continue;
                                            }
                                            let ts =
                                                crate::clock::normalize_ts_ms(sm.ts);
                                            if ts < start_ms {
                                                continue;
                                            }
                                            if now_ms.saturating_sub(ts) >= dur_ms
                                                && setting.is_expired(ts, now_ms)
                                            {
                                                expired_ids.push(sm.msg_id.clone());
                                            }
                                        }
                                        if !expired_ids.is_empty() {
                                            out.push((tid, expired_ids));
                                        }
                                    }
                                    out
                                })
                                .await
                                .unwrap_or_default();
                            if swept.is_empty() {
                                continue;
                            }
                            for (tid, ids) in swept {
                                let _ = gui_tx_vanish.send(GuiEvent::VanishExpired {
                                    thread_id: tid,
                                    expired_ids: ids,
                                });
                            }
                            ctx_vanish.request_repaint();
                        }
                    });
                }
                {
                    // Reminder tick (WaveA1): ké nhịp 2s của vanish sweep — claim
                    // message_reminders tới hạn qua mọi DB acc rồi bắn ReminderDue 1 lần.
                    // Không early-return theo VANISH_THREADS (reminder độc lập vanish).
                    let gui_tx_rem = gui_tx.clone();
                    let ctx_rem = egui_ctx.clone();
                    tokio::spawn(async move {
                        let mut tick = tokio::time::interval(Duration::from_secs(2));
                        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                        loop {
                            tick.tick().await;
                            let now_sec = crate::login::unix_now() as i64;
                            let due: Vec<crate::store::ReminderItem> =
                                tokio::task::spawn_blocking(move || {
                                    let mut out = Vec::new();
                                    let mut seen: std::collections::HashSet<i64> =
                                        std::collections::HashSet::new();
                                    // DB mặc định + mọi DB acc (accounts/*_history.db).
                                    let mut dbs = vec!["zalo_history.db".to_string()];
                                    if let Ok(entries) = std::fs::read_dir("accounts") {
                                        for e in entries.flatten() {
                                            let p = e.path();
                                            if p.extension().and_then(|x| x.to_str())
                                                == Some("db")
                                            {
                                                dbs.push(p.to_string_lossy().into_owned());
                                            }
                                        }
                                    }
                                    for db in dbs {
                                        if let Ok(mut store) =
                                            crate::store::MessageStore::open(&db)
                                        {
                                            for r in store.claim_due_reminders(now_sec) {
                                                if seen.insert(r.id) {
                                                    out.push(r);
                                                }
                                            }
                                        }
                                    }
                                    out.sort_by_key(|r| r.due_at);
                                    out
                                })
                                .await
                                .unwrap_or_default();
                            if due.is_empty() {
                                continue;
                            }
                            let _ = gui_tx_rem.send(GuiEvent::ReminderDue(due));
                            ctx_rem.request_repaint();
                        }
                    });
                }
                {
                    // Script per-thread trigger on_interval: nhịp 60s, chỉ DB acc đang dùng.
                    // RẺ: mở DB 1 lần/tick, chỉ chạy script tới hạn; outbox drain qua SendMessage.
                    let gui_tx_script = gui_tx.clone();
                    let ctx_script = egui_ctx.clone();
                    let session_script = session.clone();
                    tokio::spawn(async move {
                        // Lấy uin async trong task (blocking_lock panic trong runtime).
                        let login_uin = session_script
                            .lock()
                            .await
                            .login
                            .as_ref()
                            .map(|l| l.uin.clone())
                            .unwrap_or_default();
                        let mut tick = tokio::time::interval(Duration::from_secs(60));
                        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                        loop {
                            tick.tick().await;
                            let now_sec = crate::login::unix_now();
                            let due: Vec<(
                                String,
                                String,
                                Vec<crate::chat::Thread>,
                                Vec<crate::thread_script::ThreadScript>,
                            )> = tokio::task::spawn_blocking({
                                let login_uin = login_uin.clone();
                                move || {
                                    let mut out = Vec::new();
                                    // DB của acc đang dùng (per-acc path).
                                    let mut dbs = vec![crate::store::db_path_for_account(&login_uin)];
                                    // Fallback DB mặc định khi chưa login.
                                    if dbs[0].is_empty() || dbs[0] == "zalo_history.db" {
                                        dbs = vec!["zalo_history.db".to_string()];
                                    }
                                    for db in dbs {
                                        let store = match crate::store::MessageStore::open(&db) {
                                            Ok(s) => s,
                                            Err(_) => continue,
                                        };
                                        let enabled = store.all_enabled_scripts();
                                        if enabled.is_empty() {
                                            continue;
                                        }
                                        // RAM snapshot threads từ cache disk (không block UI).
                                        let threads = crate::chat::load_threads_cache();
                                        // Gom theo thread: chỉ script interval + tới hạn.
                                        let mut by_thread: std::collections::HashMap<
                                            String,
                                            Vec<crate::thread_script::ThreadScript>,
                                        > = std::collections::HashMap::new();
                                        for s in enabled {
                                            if let crate::thread_script::TriggerKind::OnInterval(
                                                every,
                                            ) = crate::thread_script::parse_trigger(
                                                &s.trigger,
                                                &s.trigger_arg,
                                            ) {
                                                let last: i64 = store
                                                    .thread_kv_get(
                                                        &s.thread_id,
                                                        &crate::thread_script::last_run_key(s.id),
                                                    )
                                                    .and_then(|v| v.parse().ok())
                                                    .unwrap_or(0);
                                                if crate::thread_script::interval_due(
                                                    last, every, now_sec,
                                                ) {
                                                    by_thread
                                                        .entry(s.thread_id.clone())
                                                        .or_default()
                                                        .push(s);
                                                }
                                            }
                                        }
                                        for (tid, scripts) in by_thread {
                                            out.push((db.clone(), tid, threads.clone(), scripts));
                                        }
                                    }
                                    out
                                }
                            })
                            .await
                            .unwrap_or_default();
                            for (db, tid, snapshot, scripts) in due {
                                let ev = crate::thread_script::ScriptEvent::interval();
                                for s in &scripts {
                                    match crate::thread_script::run_script(
                                        &db, &snapshot, &tid, s, &ev,
                                    ) {
                                        Ok(res) => {
                                            for o in res.outbox {
                                                if let Some(wtx) = WORKER_TX.get() {
                                                    let is_group = snapshot
                                                        .iter()
                                                        .find(|t| t.id == o.thread_id)
                                                        .map(|t| t.is_group)
                                                        .unwrap_or(false);
                                                    // Drain outbox qua SendMessage có sẵn.
                                                    let _ = wtx.send(WorkerCmd::SendMessage {
                                                        to_id: o.thread_id.clone(),
                                                        text: o.text.clone(),
                                                        is_group,
                                                    });
                                                }
                                            }
                                            for t in res.toasts {
                                                let _ = gui_tx_script.send(
                                                    GuiEvent::ScriptToast {
                                                        thread_id: tid.clone(),
                                                        message: t,
                                                    },
                                                );
                                            }
                                            let _ = gui_tx_script.send(
                                                GuiEvent::ScriptLogsUpdated { thread_id: tid.clone() },
                                            );
                                            ctx_script.request_repaint();
                                        }
                                        Err(e) => {
                                            crate::thread_script::persist_error_log(
                                                &db,
                                                &tid,
                                                &s.name,
                                                &e.to_string(),
                                            );
                                            let _ = gui_tx_script.send(
                                                GuiEvent::ScriptLogsUpdated { thread_id: tid.clone() },
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    });
                }
                {
                    let s_kw = session.clone();
                    let gui_tx_kw = gui_tx.clone();
                    let ctx_kw = egui_ctx.clone();
                    tokio::spawn(async move {
                        let s = s_kw.lock().await.clone();
                        if s.login.is_some() {
                            if let Some(tx) = WORKER_TX.get() {
                                let _ = tx.send_for_account(WorkerCmd::RefreshThreads, s.login.as_ref().map(|login| login.uin.as_str()).unwrap_or_default(), &gui_tx_kw.scoped());
                            }
                            if let Ok(kws) = crate::chat::fetch_sticker_suggest_keywords(&s).await {
                                if !kws.is_empty() {
                                    let _ = gui_tx_kw
                                        .send(GuiEvent::StickerSuggestKeywordsLoaded(kws));
                                    ctx_kw.request_repaint();
                                }
                            }
                        }
                    });
                }

                let call_bridge = std::sync::Arc::new(crate::call::CallBridge::spawn(
                    session.clone(),
                    gui_tx.clone(),
                    egui_ctx.clone(),
                ));

                // 1) Real-time `zpw_ws` WebSocket loop for instant chat messages + VoIP events
                {
                    let s_ws = session.clone();
                    let cb = call_bridge.clone();
                    let gui_tx_ws = gui_tx.clone();
                    let ctx_ws = egui_ctx.clone();
                    tokio::spawn(async move {
                        let mut ws_rx: Option<
                            tokio::sync::mpsc::UnboundedReceiver<crate::socket::WsEvent>,
                        > = None;
                        let mut ws_sek = String::new();
                        let mut scope = gui_tx_ws.scoped();
                        loop {
                            if !scope.is_current() {
                                scope = gui_tx_ws.scoped();
                                ws_rx = None;
                                ws_sek.clear();
                            }
                            let gui_tx_ws = &scope;
                            let s_snap = s_ws.lock().await.clone();
                            if s_snap.login.is_some() {
                                let cur_sek = s_snap.zpw_sek.clone().unwrap_or_default();
                                if !cur_sek.is_empty() && (ws_rx.is_none() || ws_sek != cur_sek) {
                                    ws_sek = cur_sek;
                                    ws_rx = Some(crate::socket::spawn(s_snap.clone()));
                                }
                                if let Some(rx) = ws_rx.as_mut() {
                                    match tokio::time::timeout(
                                        Duration::from_millis(1500),
                                        rx.recv(),
                                    )
                                    .await
                                    {
                                        Ok(Some(ev)) => {
                                            if !gui_tx_ws.is_current() {
                                                continue;
                                            }
                                            crate::reactions::trace_ws_payload(
                                                ev.cmd,
                                                ev.sub_cmd,
                                                &ev.payload,
                                            );
                                            let data_node = ev
                                                .payload
                                                .get("data")
                                                .unwrap_or(&ev.payload);
                                            for voip_ev in
                                                crate::call::extract_voip_controls(data_node)
                                            {
                                                cb.for_account(s_snap.clone(), gui_tx_ws.clone()).handle_control_event(voip_ev);
                                            }
                                            crate::chat::dispatch_file_done_controls(data_node);
                                            let mute_evs = crate::chat::extract_mute_controls(
                                                data_node,
                                                crate::login::unix_now() as i64,
                                            );
                                            if !mute_evs.is_empty() {
                                                let _ = gui_tx_ws
                                                    .send(GuiEvent::MuteEventsReceived(mute_evs));
                                                ctx_ws.request_repaint();
                                            }
                                            let (typing_evs, seen_evs) =
                                                crate::chat::extract_typing_and_seen_controls(data_node);
                                            if !typing_evs.is_empty() {
                                                let _ = gui_tx_ws.send(GuiEvent::TypingEventsReceived(typing_evs));
                                                ctx_ws.request_repaint();
                                            }
                                            if !seen_evs.is_empty() {
                                                let _ = gui_tx_ws.send(GuiEvent::SeenEventsReceived(seen_evs));
                                                ctx_ws.request_repaint();
                                            }
                                            let react_evs =
                                                crate::reactions::parse_reaction_events(data_node);
                                            if !react_evs.is_empty() {
                                                let _ = gui_tx_ws
                                                    .send(GuiEvent::ReactionEventsReceived(react_evs));
                                                ctx_ws.request_repaint();
                                            }

                                            queue_mobile_sync_controls(
                                                crate::chat::extract_mobile_sync_controls(data_node),
                                                &s_snap,
                                                gui_tx_ws,
                                                true,
                                            );
                                            if ev.is_chat_ish() {
                                                let mut ws_threads = Vec::new();
                                                if let Some(li) = s_snap.login.as_ref() {
                                                    if !li.send2me_id.is_empty() {
                                                        ws_threads.push(Thread {
                                                            presentation: Default::default(),
                                                            unread: Default::default(),
                                                            id: li.send2me_id.clone(),
                                                            name: "Cloud của tôi (Lưu trữ)"
                                                                .into(),
                                                            preview: String::new(),
                                                            time: String::new(),
                                                            avatar: None,
                                                            is_group: false,
                                                            messages: Vec::new(),
                                                            vanish: None,
                                                        });
                                                    }
                                                }
                                                if let Ok(known) = KNOWN_THREADS.lock() {
                                                    for (id, name, is_group) in known.iter() {
                                                        if !ws_threads.iter().any(|t| &t.id == id) {
                                                            ws_threads.push(Thread {
                                                                presentation: Default::default(),
                                                                unread: Default::default(),
                                                                id: id.clone(),
                                                                name: name.clone(),
                                                                preview: String::new(),
                                                                time: String::new(),
                                                                avatar: None,
                                                                is_group: *is_group,
                                                                messages: Vec::new(),
                                                                vanish: None,
                                                            });
                                                        }
                                                    }
                                                }
                                                crate::chat::merge_previews(
                                                    &mut ws_threads,
                                                    &ev.payload,
                                                );
                                                ws_threads.retain(|t| !t.messages.is_empty());
                                                if !ws_threads.is_empty() {
                                                    // Deliver incoming messages to UI immediately (0ms network wait)
                                                    let _ = gui_tx_ws.send(
                                                        GuiEvent::ThreadsUpdated(ws_threads.clone()),
                                                    );
                                                    ctx_ws.request_repaint();
                                                    let needs_resolve = ws_threads.iter().any(|t| {
                                                        !t.presentation.contact_name
                                                            && (t.name.is_empty()
                                                                || crate::chat::is_placeholder_thread_name(&t.name, &t.id)
                                                                || crate::chat::is_generic_thread_name(&t.name))
                                                    });
                                                    if needs_resolve {
                                                        let s_bg = s_snap.clone();
                                                        let gui_tx_bg = gui_tx_ws.clone();
                                                        let ctx_bg = ctx_ws.clone();
                                                        tokio::spawn(async move {
                                                            let mut resolved = ws_threads;
                                                            crate::chat::resolve_thread_names(
                                                                &s_bg,
                                                                &mut resolved,
                                                            )
                                                            .await;
                                                            let _ = gui_tx_bg.send(
                                                                GuiEvent::ThreadsUpdated(resolved),
                                                            );
                                                            ctx_bg.request_repaint();
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                        Ok(None) => {
                                            ws_rx = None;
                                            tokio::time::sleep(Duration::from_secs(2)).await;
                                        }
                                        Err(_) => {}
                                    }
                                } else {
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                            } else {
                                ws_rx = None;
                                ws_sek.clear();
                                tokio::time::sleep(Duration::from_secs(3)).await;
                            }
                        }
                    });
                }

                // 2) Background HTTP long-poll fallback for VoIP & mute control events (`lp_control`)
                {
                    let s_ctrl = session.clone();
                    let cb = call_bridge.clone();
                    let gui_tx_ctrl = gui_tx.clone();
                    let ctx_ctrl = egui_ctx.clone();
                    tokio::spawn(async move {
                        let mut scope = gui_tx_ctrl.scoped();
                        let mut state = crate::call::CtrlPollState::default();
                        loop {
                            if !scope.is_current() {
                                scope = gui_tx_ctrl.scoped();
                                state = crate::call::CtrlPollState::default();
                            }
                            let s_snap = s_ctrl.lock().await.clone();
                            if s_snap.login.is_some() {
                                let ctrl_res = crate::call::poll_control_events(&s_snap, &mut state).await;
                                if let Ok((events, mute_evs, typing_evs, seen_evs)) = ctrl_res {
                                    if !scope.is_current() { continue; }
                                    for ev in events {
                                        cb.for_account(s_snap.clone(), scope.clone()).handle_control_event(ev);
                                    }
                                    if !mute_evs.is_empty() {
                                        let _ = scope
                                            .send(GuiEvent::MuteEventsReceived(mute_evs));
                                        ctx_ctrl.request_repaint();
                                    }
                                    if !typing_evs.is_empty() {
                                        let _ = scope
                                            .send(GuiEvent::TypingEventsReceived(typing_evs));
                                        ctx_ctrl.request_repaint();
                                    }
                                    if !seen_evs.is_empty() {
                                        let _ = scope
                                            .send(GuiEvent::SeenEventsReceived(seen_evs));
                                        ctx_ctrl.request_repaint();
                                    }
                                } else {
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            } else {
                                tokio::time::sleep(Duration::from_secs(3)).await;
                            }
                        }
                    });
                }

                // 3) Periodic GitHub Releases API update checker (polls every 30 minutes)
                {
                    let gui_tx_upd = gui_tx.clone();
                    let ctx_upd = egui_ctx.clone();
                    tokio::spawn(async move {
                        let mut ticker = tokio::time::interval(Duration::from_secs(30 * 60));
                        // First tick fires immediately; skip it because ZaloGuiApp::new already
                        // dispatches an initial CheckForUpdates on startup.
                        ticker.tick().await;
                        loop {
                            ticker.tick().await;
                            let res = fetch_github_latest_release().await;
                            let _ = gui_tx_upd.send(GuiEvent::UpdateCheckFinished {
                                result: res,
                                silent: true,
                            });
                            ctx_upd.request_repaint();
                        }
                    });
                }

                // Inactive accounts keep a passive socket so they still notify (multi-account).
                {
                    let gui_tx_bg = gui_tx.clone();
                    let ctx_bg = egui_ctx.clone();
                    let sink: crate::background_accounts::ActivitySink =
                        std::sync::Arc::new(move |activity| {
                            // A closed UI channel means the app is shutting down.
                            let _ = gui_tx_bg.send(GuiEvent::BackgroundAccountActivity(activity));
                            ctx_bg.request_repaint();
                        });
                    tokio::spawn(crate::background_accounts::supervise(session.clone(), sink));
                }

                let mut current_qr_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut current_password_task: Option<tokio::task::AbortHandle> = None;
                let mut account_tasks = tokio::task::JoinSet::new();
                let mut history_writes = tokio::task::JoinSet::new();
                let mut global_tasks = tokio::task::JoinSet::new();
                let mut deferred = std::collections::VecDeque::new();

                while let Some(queued) = next_worker_command(&mut cmd_rx, &mut deferred).await {
                    while account_tasks.try_join_next().is_some() {}
                    while history_writes.try_join_next().is_some() {}
                    while global_tasks.try_join_next().is_some() {}
                    if !queued.origin_is_current() {
                        queued.reject(&gui_tx);
                        continue;
                    }
                    // Login tasks can hold the session across network awaits. Cancel them
                    // before attempting to inspect that session for a transition command.
                    if queued.command.is_transition()
                    {
                        if let Some(task) = current_qr_task.take() { task.abort(); }
                        account_tasks.abort_all();
                    }
                    // App-wide authorization, updates and discovery do not wait for login.
                    let Some(snapshot) = try_command_snapshot(&queued.command, &session) else {
                        // Keep FIFO for ordinary actions while allowing app-wide commands and
                        // account transitions to bypass login's long critical section.
                        deferred.push_front(queued);
                        continue;
                    };
                    let current_uin = snapshot.as_ref().and_then(|snapshot| snapshot.login.as_ref())
                        .map(|login| login.uin.clone()).unwrap_or_default();
                    if !queued.accepts_account(&current_uin) {
                        queued.reject(&gui_tx);
                        continue;
                    }
                    let command_origin = queued.origin_uin.clone().unwrap_or_default();
                    let cmd = queued.command;
                    let command_session = session_for_command(&cmd, snapshot.as_ref(), &session);
                    let global = matches!(&cmd,
                        WorkerCmd::ReadNotificationAuthorization { .. }
                        | WorkerCmd::RequestNotificationAuthorization { .. }
                        | WorkerCmd::CheckForUpdates { .. }
                        | WorkerCmd::DownloadAndApplyUpdate { .. }
                        | WorkerCmd::RefreshMediaDevices
                    );
                    let tasks = if global { &mut global_tasks } else { &mut account_tasks };
                    let gui_tx = if global { gui_tx.clone() } else { gui_tx.scoped() };
                    let call_bridge = snapshot.map(|snapshot| call_bridge.for_account(snapshot, gui_tx.clone()))
                        .unwrap_or_else(|| call_bridge.as_ref().clone());
                    let gui_tx = authentication_events(&cmd, gui_tx, &call_bridge);
                    match cmd {
                        WorkerCmd::RefreshSavedAccounts { request_id } => {
                            let uin = current_uin.clone();
                            let tx = gui_tx.clone();
                            tasks.spawn(async move {
                                let result = tokio::task::spawn_blocking(move ||
                                    crate::account_manager::list_saved_accounts(&uin)
                                ).await.map_err(|_| "Không thể tải danh sách tài khoản.".to_string());
                                // Closing the UI receiver is normal during shutdown or account switching.
                                let _ = tx.send(GuiEvent::SavedAccountsLoaded { request_id, result });
                            });
                        }
                        WorkerCmd::DeleteSavedAccount { request_id, target_uin } => {
                            let uin = current_uin.clone();
                            // Serialize deletion with account switches so the target cannot become
                            // active between validation and removal. The UI remains asynchronous.
                            let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                                crate::account_manager::validate_uin(&target_uin)?;
                                anyhow::ensure!(target_uin != uin, "Không thể xoá tài khoản đang sử dụng.");
                                crate::account_manager::delete_saved_account(&target_uin)?;
                                Ok(crate::account_manager::list_saved_accounts(&uin))
                            }).await.map_err(|_| "Không thể xoá tài khoản đã lưu.".to_string())
                                .and_then(|result| result.map_err(|error| error.to_string()));
                            // The account-scoped sender rejects completions from obsolete accounts.
                            let _ = gui_tx.send(GuiEvent::SavedAccountsLoaded { request_id, result });
                        }
                        WorkerCmd::RefreshMediaDevices => {
                            let tx = gui_tx.clone();
                            tasks.spawn(async move {
                                let result = tokio::task::spawn_blocking(crate::media::refresh_media_devices)
                                    .await.unwrap_or_else(|error| Err(error.into()));
                                if let Err(error) = &result {
                                    crate::logger::log_error("media::device_discovery", &error.to_string());
                                }
                                // Closing the receiver is normal during application shutdown.
                                let _ = tx.send(GuiEvent::MediaDevicesRefreshed {
                                    error: result.err().map(|error| error.to_string()),
                                });
                            });
                        }
                        WorkerCmd::ReadNotificationAuthorization { request_id }
                        | WorkerCmd::RequestNotificationAuthorization { request_id } => {
                            let request = matches!(
                                cmd, WorkerCmd::RequestNotificationAuthorization { .. }
                            );
                            let tx = gui_tx.clone();
                            tasks.spawn(async move {

                                let result = if request {
                                    crate::native_notifications::request_if_user_action(true).await
                                } else {
                                    crate::native_notifications::status().await
                                };
                                // Closing the UI receiver is normal during shutdown.
                                let _ = tx.send(GuiEvent::NotificationAuthorization {
                                    request_id, result,
                                });
                            });
                        }
                        WorkerCmd::PersistThreads { uin, threads: threads_snapshot } => {
                            if uin.is_empty() {
                                continue;
                            }
                            let _completion = spawn_history_write(&mut history_writes, move || {
                                if let Ok(mut store) =
                                    crate::store::MessageStore::open_account(&uin)
                                {
                                    let _ =
                                        crate::store::persist_threads(&mut store, &threads_snapshot);
                                }
                                crate::chat::save_threads_cache_for_account(&uin, &threads_snapshot);
                                Ok(())
                            });
                        }
                        WorkerCmd::ImportHistory { uin, path } => {
                            let completion = spawn_history_write(&mut history_writes, move || -> anyhow::Result<_> {
                                    crate::store::MessageStore::open_account(&uin)?;
                                    let summary = crate::store::import_history_from_path(
                                        &path, &crate::store::db_path_for_account(&uin),
                                    )?;
                                    let mut threads = crate::chat::load_threads_cache_for_account(&uin);
                                    crate::store::MessageStore::open_account(&uin)?.hydrate_threads(
                                        &mut threads, crate::store::MessageStore::PRELOAD_MSGS,
                                    )?;
                                    crate::chat::save_threads_cache_for_account(&uin, &threads);
                                    Ok((summary, threads))
                            });
                            tasks.spawn(async move {
                                let result = completion.await.unwrap_or_else(|error| Err(error.into()));
                                match result {
                                    Ok((summary, threads)) => {
                                        let _ = gui_tx.send(GuiEvent::ThreadsUpdated(threads));
                                        let _ = gui_tx.send(GuiEvent::HistoryImportFinished(Ok(format!(
                                            "Đã nhập {} tin nhắn từ {} hội thoại.",
                                            summary.messages_imported, summary.threads_imported,
                                        ))));
                                    }
                                    Err(error) => {
                                        let _ = gui_tx.send(GuiEvent::HistoryImportFinished(Err(error.to_string())));
                                    }
                                }
                            });
                        }
                        WorkerCmd::LoadScheduledMessages { uin, notify_sent } => {
                            tasks.spawn(async move {
                                let result = tokio::task::spawn_blocking(move || {
                                    crate::store::MessageStore::open_account(&uin)?;
                                    crate::scheduler::list_pending_scheduled(
                                        &crate::store::db_path_for_account(&uin), None,
                                    )
                                }).await.unwrap_or_else(|error| Err(error.into()));
                                let _ = gui_tx.send(GuiEvent::ScheduledItemsLoaded {
                                    result: result.map_err(|error| error.to_string()),
                                    notify_sent,
                                });
                            });
                        }
                        WorkerCmd::SendScheduledMessageNow { uin, id } => {
                            tasks.spawn(async move {
                                let result = tokio::task::spawn_blocking(move || {
                                    crate::account_manager::validate_uin(&uin)?;
                                    crate::scheduler::send_scheduled_now(
                                        &crate::store::db_path_for_account(&uin), id,
                                        crate::login::unix_now() as i64,
                                    )
                                }).await.unwrap_or_else(|error| Err(error.into()));
                                let _ = gui_tx.send(GuiEvent::ScheduledListUpdated);
                                let _ = gui_tx.send(GuiEvent::ScheduleSendNowFinished(
                                    result.map_err(|error| error.to_string()),
                                ));
                            });
                        }
                        WorkerCmd::CancelScheduledMessage { uin, id } => {
                            tasks.spawn(async move {
                                let result = tokio::task::spawn_blocking(move || {
                                    crate::account_manager::validate_uin(&uin)?;
                                    crate::scheduler::cancel_scheduled_message(
                                        &crate::store::db_path_for_account(&uin), id,
                                    )
                                }).await.unwrap_or_else(|error| Err(error.into()));
                                let _ = gui_tx.send(GuiEvent::ScheduleCancelFinished(
                                    result.map_err(|error| error.to_string()),
                                ));
                            });
                        }
                        WorkerCmd::Logout | WorkerCmd::BeginAddAccount { .. } => {
                            account_tasks.abort_all();
                            if let Some(task) = current_qr_task.take() {
                                task.abort();
                            }
                            let previous_account = if let WorkerCmd::BeginAddAccount { pending_notes } = cmd {
                                // A cancelled session refresh must finish before we remove its active session file.
                                while account_tasks.join_next().await.is_some() {}
                                let uin = {
                                    let active = session.lock().await;
                                    departing_account_uin(&active, &command_origin)
                                };
                                if !prepare_add_account(&uin, pending_notes, &gui_tx).await {
                                    continue;
                                }
                                Some(uin)
                            } else {
                                None
                            };
                            let (cur_uin, gui_tx) = {
                                let mut s = session.lock().await;
                                call_bridge.end_call_for_account(s.clone(), gui_tx.scoped());
                                let uin = departing_account_uin(&s, &command_origin);
                                let tx = gui_tx.advance_account();
                                s.logout();
                                (uin, tx)
                            };
                            crate::history_sync::PHONE_SYNC_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
                            crate::history_sync::set_active_v2_sync_id(None);
                            poll_state = Default::default();
                            crate::account_manager::set_active_uin("");
                            clear_account_thread_lists();
                            let result = if previous_account.is_none() {
                                tokio::task::spawn_blocking(move || crate::account_manager::logout_account(&cur_uin))
                                    .await.unwrap_or_else(|error| Err(error.into()))
                            } else {
                                Ok(())
                            };
                            crate::background_accounts::request_reconcile();
                            let _ = gui_tx.send(GuiEvent::LoggedOut { previous_account });
                            if let Err(error) = result {
                                crate::logger::log_error("account::logout", &error.to_string());
                                let _ = gui_tx.send(GuiEvent::StatusUpdate("Không thể xoá hết phiên đã lưu; vui lòng thử đăng xuất lại.".into()));
                            }
                            egui_ctx.request_repaint();
                        }
                        WorkerCmd::SwitchAccount { target_uin } => {
                            account_tasks.abort_all();
                            if let Some(task) = current_qr_task.take() {
                                task.abort();
                            }
                            let (previous, gui_tx) = {
                                let mut s = session.lock().await;
                                call_bridge.end_call_for_account(s.clone(), gui_tx.scoped());
                                let previous = s.clone();
                                let tx = gui_tx.advance_account();
                                // Background loops see no active account during the disk transition.
                                s.logout();
                                (previous, tx)
                            };
                            crate::history_sync::PHONE_SYNC_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
                            crate::history_sync::set_active_v2_sync_id(None);
                            poll_state = Default::default();
                            clear_account_thread_lists();
                            let rollback = previous.clone();
                            let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                                crate::account_manager::validate_uin(&target_uin)?;
                                let cur_uin = previous.login.as_ref().map(|l| l.uin.as_str()).unwrap_or_default();
                                let mut next = previous.clone();
                                next.load_from_file(&format!("accounts/{target_uin}.json"))?;
                                let mut threads = crate::chat::load_threads_cache_for_account(&target_uin);
                                crate::store::MessageStore::open_account(&target_uin)?.hydrate_threads(&mut threads, 50)?;
                                crate::account_manager::switch_to_account(cur_uin, &target_uin)?;
                                Ok((next, threads))
                            }).await.unwrap_or_else(|error| Err(error.into()));
                            match result {
                                Ok((next, threads)) => {
                                    let uin = next.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
                                    let name = next.login.as_ref().map(|l| l.display_name.clone()).unwrap_or_default();
                                    let ava = next.login.as_ref().map(|l| l.avatar.clone()).filter(|a| !a.is_empty());
                                    crate::account_manager::set_active_uin(&uin);
                                    *session.lock().await = next;
                                    crate::background_accounts::request_reconcile();
                                    let _ = gui_tx.send(GuiEvent::AccountSwitched {
                                        uin,
                                        name,
                                        avatar: ava,
                                        threads,
                                    });
                                }
                                Err(error) => {
                                    restore_failed_switch(&session, rollback, &gui_tx, &error.to_string()).await;
                                }
                            }
                            egui_ctx.request_repaint();
                        }
                        WorkerCmd::WipeLocalData => {
                            let gui_tx = gui_tx.advance_work();
                            crate::history_sync::CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
                            crate::history_sync::PHONE_SYNC_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
                            stop_history_tasks(&mut account_tasks, &mut history_writes).await;
                            poll_state = Default::default();
                            if let Ok(mut known) = KNOWN_THREADS.lock() { known.clear(); }
                            let uin = current_uin.clone();
                            let result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                                crate::store::MessageStore::open_account(&uin)?.wipe_history()?;
                                crate::chat::save_threads_cache_for_account(&uin, &[]);
                                Ok(())
                            }).await.unwrap_or_else(|error| Err(error.into()));
                            if let Err(error) = result {
                                let _ = gui_tx.send(GuiEvent::LocalDataWipeFailed(format!("Không thể xoá lịch sử: {error}")));
                                continue;
                            }
                            let _ = gui_tx.send(GuiEvent::LocalDataWiped);
                            egui_ctx.request_repaint();
                        }
                        WorkerCmd::RefreshBoardPins {
                            thread_id,
                            is_group,
                        } => {
                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            tasks.spawn(async move {
                                let s = session_clone.lock().await.clone();
                                let result =
                                    crate::chat::list_board_topics(&s, &thread_id, is_group).await;
                                let (topics, error) = match result {
                                    Ok(list) => (list, None),
                                    Err(e) => (Vec::new(), Some(e.to_string())),
                                };
                                let _ = gui_tx_clone.send(GuiEvent::BoardPinsUpdated {
                                    thread_id,
                                    topics,
                                    error,
                                });
                            });
                        }
                        WorkerCmd::PinMessage {
                            thread_id,
                            is_group,
                            msg,
                            pin,
                            topic_id,
                        } => {
                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            tasks.spawn(async move {
                                let s = session_clone.lock().await.clone();
                                let result = if pin {
                                    crate::chat::pin_message(&s, &thread_id, is_group, &msg).await
                                } else {
                                    crate::chat::unpin_message(&s, &thread_id, is_group, &topic_id)
                                        .await
                                };
                                match result {
                                    Ok(v) => {
                                        // The server answers with the board topic; keep its id so a
                                        // later unpin targets the right item. `data` may be a JSON
                                        // object or a JSON string, depending on the endpoint.
                                        let data = match v.get("data") {
                                            Some(serde_json::Value::String(txt)) => {
                                                serde_json::from_str::<serde_json::Value>(txt)
                                                    .unwrap_or(serde_json::Value::Null)
                                            }
                                            Some(other) => other.clone(),
                                            None => serde_json::Value::Null,
                                        };
                                        let pick = |val: &serde_json::Value| {
                                            val.as_str()
                                                .map(str::to_string)
                                                .or_else(|| val.as_i64().map(|n| n.to_string()))
                                        };
                                        let topic = pick(&data)
                                            .or_else(|| data.pointer("/topic/id").and_then(pick))
                                            .or_else(|| data.pointer("/id").and_then(pick))
                                            .or_else(|| data.pointer("/topicId").and_then(pick))
                                            .unwrap_or_default();
                                        let _ = gui_tx_clone.send(GuiEvent::MessagePinResult {
                                            thread_id,
                                            msg_id: msg.id.clone(),
                                            pin,
                                            topic_id: topic,
                                            error: None,
                                        });
                                    }
                                    Err(e) => {
                                        let _ = gui_tx_clone.send(GuiEvent::MessagePinResult {
                                            thread_id,
                                            msg_id: msg.id.clone(),
                                            pin,
                                            topic_id: String::new(),
                                            error: Some(e.to_string()),
                                        });
                                    }
                                }
                            });
                        }
                        WorkerCmd::PinThread {
                            thread_id,
                            is_group,
                            pin,
                        } => {

                            let session_clone = command_session.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let _ = crate::chat::update_pinned_conversation(
                                    &s, &thread_id, is_group, pin,
                                )
                                .await;
                            });
                        }
                        WorkerCmd::MarkThreadUnread {
                            thread_id,
                            is_group,
                            unread,
                        } => {

                            let session_clone = command_session.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let _ =
                                    crate::chat::set_unread_mark(&s, &thread_id, is_group, unread)
                                        .await;
                            });
                        }
                        WorkerCmd::MuteThread {
                            thread_id,
                            is_group,
                            duration_sec,
                            mute,
                        } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let _ = crate::chat::set_mute_conversation(
                                    &s,
                                    &thread_id,
                                    is_group,
                                    duration_sec,
                                    mute,
                                )
                                .await;
                                if let Ok(mute_resp) = crate::chat::get_mute_conversations(&s).await
                                {
                                    if let Some(mute_map) = crate::unread::extract_mute_map(
                                        &mute_resp,
                                        crate::login::unix_now() as i64,
                                    ) {
                                        let _ = gui_tx_clone
                                            .send(GuiEvent::MuteSnapshotSynced(mute_map));
                                        ctx_clone.request_repaint();
                                    }
                                }
                            });
                        }
                        WorkerCmd::HideThread {
                            thread_id,
                            is_group,
                            hide,
                        } => {

                            let session_clone = command_session.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let _ = crate::chat::set_hidden_conversation(
                                    &s, &thread_id, is_group, hide,
                                )
                                .await;
                            });
                        }
                        WorkerCmd::SetThreadVanish {
                            thread_id,
                            duration_sec,
                        } => {
                            // Vanish là local-only: không gọi server, chỉ persist SQLite + báo GUI
                            // để apply ngay vào thread đang hiển thị. Ghi vào BOTH DB:
                            // account DB (theo uin, nguồn thật cho sweep/hydrate) VÀ
                            // active `zalo_history.db` (worker sweep/hydrate đọc path này;
                            // chỉ ghi history DB thì sweep skip vì active DB không có
                            // setting → tin đủ hạn vẫn hiển thị, #1156). Lỗi persist
                            // chỉ log, GUI vẫn giữ trạng thái RAM.
                            let db_path = crate::store::db_path_for_account(&current_uin);
                            let now_sec = crate::login::unix_now();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::task::spawn_blocking(move || {
                                // 1) Account DB — nguồn thật.
                                if let Ok(mut store) =
                                    crate::store::MessageStore::open(&db_path)
                                {
                                    if duration_sec > 0 {
                                        let _ = store.upsert_thread_vanish(
                                            &thread_id,
                                            duration_sec,
                                            now_sec,
                                        );
                                    } else {
                                        let _ = store
                                            .upsert_thread_vanish(&thread_id, 0, 0);
                                    }
                                }
                                // 2) Active DB — worker sweep + hydrate đang đọc path này.
                                // Trùng khi chưa login multi-account thì open cùng file, rẻ.
                                if db_path != "zalo_history.db" {
                                    if let Ok(mut active) =
                                        crate::store::MessageStore::open("zalo_history.db")
                                    {
                                        if duration_sec > 0 {
                                            let _ = active.upsert_thread_vanish(
                                                &thread_id,
                                                duration_sec,
                                                now_sec,
                                            );
                                        } else {
                                            let _ = active
                                                .upsert_thread_vanish(&thread_id, 0, 0);
                                        }
                                    }
                                }
                                let _ = gui_tx_clone.send(GuiEvent::VanishSettingChanged {
                                    thread_id,
                                    duration_sec,
                                    started_at_sec: if duration_sec > 0 {
                                        now_sec
                                    } else {
                                        0
                                    },
                                });
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::DeleteThread {
                            thread_id,
                            is_group,
                            last_cli_msg_id,
                        } => {

                            let uin = current_uin.clone();
                            let deleted_id = thread_id.clone();
                            let completion = spawn_history_write(&mut history_writes, move || -> anyhow::Result<()> {
                                    crate::store::MessageStore::open_account(&uin)?.delete_thread(&deleted_id)?;
                                    let mut threads = crate::chat::load_threads_cache_for_account(&uin);
                                    threads.retain(|thread| thread.id != deleted_id);
                                    crate::chat::save_threads_cache_for_account(&uin, &threads);
                                    Ok(())
                            });
                            let session_clone = command_session.clone();
                            tasks.spawn(async move {
                                let s = session_clone.lock().await.clone();
                                let local_result = completion.await.unwrap_or_else(|error| Err(error.into()));
                                if let Err(error) = local_result {
                                    crate::logger::log_error("history::delete_thread", &error.to_string());
                                }
                                let _ = crate::chat::delete_conversation(
                                    &s,
                                    &thread_id,
                                    is_group,
                                    &last_cli_msg_id,
                                )
                                .await;
                            });
                        }
                        WorkerCmd::LeaveGroup { group_id, silent } => {
                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let egui_ctx_clone = egui_ctx.clone();
                            let gid = group_id.clone();
                            tasks.spawn(async move {
                                let s = session_clone.lock().await.clone();
                                match crate::chat::leave_group(&s, &gid, silent).await {
                                    Ok(_) => {
                                        let _ = gui_tx_clone.send(GuiEvent::GroupLeft {
                                            group_id: gid.clone(),
                                            success: true,
                                            error: None,
                                        });
                                        egui_ctx_clone.request_repaint();
                                    }
                                    Err(e) => {
                                        let _ = gui_tx_clone.send(GuiEvent::GroupLeft {
                                            group_id: gid.clone(),
                                            success: false,
                                            error: Some(e.to_string()),
                                        });
                                        egui_ctx_clone.request_repaint();
                                    }
                                }
                            });
                        }
                        WorkerCmd::RequestQr => {
                            if let Some(task) = current_qr_task.take() {
                                task.abort();
                            }

                            let session_clone = command_session.clone();
                            let poll_clone = poll_state.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();

                            let handle = tokio::spawn(async move {
                                {
                                    let mut s = session_clone.lock().await; if !gui_tx_clone.is_current() { return; }
                                    s.logout();
                                }
                                {
                                    let mut ps = poll_clone.lock().await;
                                    *ps = crate::chat::PollState::default();
                                }
                                let _ = gui_tx_clone
                                    .send(GuiEvent::StatusUpdate("Đang xin mã QR mới…".into()));
                                ctx_clone.request_repaint();

                                let s_snap = { session_clone.lock().await.clone() };
                                let q_res = crate::qr::request_qr(&s_snap, "vi").await;

                                match q_res {
                                    Ok(q) => {
                                        if let Some(payload) = q.payload() {
                                            if let Ok(code) = qrcode::QrCode::new(payload.as_bytes())
                                            {
                                                let w = code.width();
                                                let quiet_zone = 4;
                                                let total_w = w + quiet_zone * 2;
                                                let mut pixels =
                                                    Vec::with_capacity(total_w * total_w * 4);
                                                for y in 0..total_w {
                                                    for x in 0..total_w {
                                                        let in_qr = x >= quiet_zone
                                                            && x < quiet_zone + w
                                                            && y >= quiet_zone
                                                            && y < quiet_zone + w;
                                                        let dark = in_qr
                                                            && (code[(
                                                                x - quiet_zone,
                                                                y - quiet_zone,
                                                            )] == qrcode::Color::Dark);
                                                        let c = if dark {
                                                            [15, 23, 42, 255]
                                                        } else {
                                                            [255, 255, 255, 255]
                                                        };
                                                        pixels.extend_from_slice(&c);
                                                    }
                                                }
                                                let img = egui::ColorImage::from_rgba_unmultiplied(
                                                    [total_w, total_w],
                                                    &pixels,
                                                );
                                                let _ = gui_tx_clone.send(GuiEvent::QrReady {
                                                    qr_login: q.clone(),
                                                    qr_image: img,
                                                });
                                                ctx_clone.request_repaint();

                                                let scan_res =
                                                    crate::qr::wait_for_scan(&s_snap, &q).await;

                                                if let Ok(scanned) = scan_res {
                                                    let scanned_name = scanned
                                                        .get("display_name")
                                                        .or_else(|| scanned.get("displayName"))
                                                        .and_then(|v| v.as_str())
                                                        .map(str::trim)
                                                        .unwrap_or_default()
                                                        .to_string();
                                                    let scanned_avatar = scanned
                                                        .get("avatar")
                                                        .and_then(|v| v.as_str())
                                                        .map(str::trim)
                                                        .unwrap_or_default()
                                                        .to_string();
                                                    let name = if scanned_name.is_empty() {
                                                        "Zalo User"
                                                    } else {
                                                        scanned_name.as_str()
                                                    };
                                                    let _ =
                                                        gui_tx_clone.send(GuiEvent::QrScanned {
                                                            name: name.to_string(),
                                                        });
                                                    ctx_clone.request_repaint();

                                                    let confirm_res =
                                                        crate::qr::confirm(&s_snap, &q).await;

                                                    if let Ok(resp) = confirm_res {
                                                        let mut s = session_clone.lock().await; if !gui_tx_clone.is_current() { return; }
                                                        if let Some(sek) =
                                                            crate::qr::extract_zpw_sek(&resp)
                                                        {
                                                            s.set_zpw_sek(sek);
                                                        }
                                                        if let Some(data) = resp.get("data") {
                                                            crate::login::apply_login_config(
                                                                &mut s, data,
                                                            );
                                                        }
                                                        let _ = s.save_to_file("session.json");

                                                        if let Ok(info) =
                                                            crate::login::get_login_info(&s).await
                                                        {
                                                            if let Some(cfg) = info.get("data") {
                                                                crate::login::apply_login_config(
                                                                    &mut s, cfg,
                                                                );
                                                                s.apply_server_info(cfg);
                                                            }
                                                        }
                                                        let _ = s.sync_profile_me().await;
                                                        // The profile call can fail or omit the
                                                        // name; the scan result already named the
                                                        // account, so keep it rather than leaving
                                                        // the account labelled by its id.
                                                        if let Some(ref mut li) = s.login {
                                                            if li.display_name.trim().is_empty() {
                                                                li.display_name = scanned_name.clone();
                                                            }
                                                            if li.avatar.trim().is_empty() {
                                                                li.avatar = scanned_avatar.clone();
                                                            }
                                                        }
                                                        let _ = s.save_to_file("session.json");

                                                        let uin = s
                                                            .login
                                                            .as_ref()
                                                            .map(|l| l.uin.clone())
                                                            .unwrap_or_default();
                                                        // Best effort: list the account in the
                                                        // account menu before its first switch.
                                                        let _ = crate::account_manager::save_current_account_snapshot(&uin);
                                                        crate::background_accounts::request_reconcile();
                                                        let dname = s
                                                            .login
                                                            .as_ref()
                                                            .map(|l| l.display_name.clone())
                                                            .unwrap_or_default();
                                                        let u_ava = s
                                                            .login
                                                            .as_ref()
                                                            .map(|l| l.avatar.clone())
                                                            .filter(|a| !a.is_empty());
                                                        let _ = gui_tx_clone.send(
                                                            GuiEvent::LoginSuccess {
                                                                name: dname,
                                                                uin: uin.clone(),
                                                                avatar: u_ava,
                                                            },
                                                        );
                                                        ctx_clone.request_repaint();

                                                        let mut list: Vec<Thread> = Vec::new();
                                                        if let Ok(friends) =
                                                            crate::chat::get_friends(&s).await
                                                        {
                                                            for ft in friends {
                                                                if let Some(ex) = list
                                                                    .iter_mut()
                                                                    .find(|t| t.id == ft.id)
                                                                {
                                                                    crate::chat::adopt_thread_name(
                                                                        ex,
                                                                        ft.name,
                                                                        ft.presentation.contact_name,
                                                                    );
                                                                    if crate::chat::should_replace_avatar(
                                                                        ex.avatar.as_deref(),
                                                                        ft.avatar.as_deref(),
                                                                    ) {
                                                                        ex.avatar = ft.avatar;
                                                                    }
                                                                    ex.is_group =
                                                                        ex.is_group || ft.is_group;
                                                                } else {
                                                                    list.push(ft);
                                                                }
                                                            }
                                                        }
                                                        if let Ok(groups) =
                                                            crate::chat::get_groups(&s).await
                                                        {
                                                            for gt in groups {
                                                                if let Some(ex) = list
                                                                    .iter_mut()
                                                                    .find(|t| t.id == gt.id)
                                                                {
                                                                    if !gt.name.is_empty() {
                                                                        ex.name = gt.name;
                                                                    }
                                                                    if crate::chat::should_replace_avatar(
                                                                        ex.avatar.as_deref(),
                                                                        gt.avatar.as_deref(),
                                                                    ) {
                                                                        ex.avatar = gt.avatar;
                                                                    }
                                                                    ex.presentation.posting_restricted = gt.presentation.posting_restricted;
                                                                    ex.is_group = true;
                                                                } else {
                                                                    list.push(gt);
                                                                }
                                                            }
                                                        }
                                                        let db_path = crate::store::db_path_for_account(&uin);
                                                        crate::store::load_history_into_threads(
                                                            &db_path,
                                                            &mut list,
                                                            crate::store::MessageStore::PRELOAD_MSGS,
                                                        );
                                                        {
                                                            let mut ps = poll_clone.lock().await;
                                                            let _ = crate::chat::poll_new_messages(
                                                                &s, &mut ps, &mut list,
                                                            )
                                                            .await;
                                                        }
                                                        if let Ok(mute_resp) =
                                                            crate::chat::get_mute_conversations(&s)
                                                                .await
                                                        {
                                                            crate::unread::merge_mute_metadata(
                                                                &mut list, &mute_resp,
                                                            );
                                                            if let Some(mute_map) =
                                                                crate::unread::extract_mute_map(
                                                                    &mute_resp,
                                                                    crate::login::unix_now() as i64,
                                                                )
                                                            {
                                                                let _ = gui_tx_clone.send(
                                                                    GuiEvent::MuteSnapshotSynced(
                                                                        mute_map,
                                                                    ),
                                                                );
                                                            }
                                                        }
                                                        if let Ok(pinned_resp) = crate::chat::get_pinned(&s).await {
                                                            let pids = crate::chat::parse_pinned_conversations(&pinned_resp);
                                                            crate::chat::apply_pinned_threads(&mut list, &pids);
                                                            let _ = gui_tx_clone.send(GuiEvent::PinnedSnapshotSynced(pids));
                                                        }
                                                        let sync_snapshot = list.clone();
                                                        let _ = gui_tx_clone
                                                            .send(GuiEvent::ThreadsUpdated(list));
                                                        ctx_clone.request_repaint();

                                                        queue_automatic_phone_sync(&gui_tx_clone, &uin, sync_snapshot);

                                                        let s_kw = s.clone();
                                                        let tx_kw = gui_tx_clone.clone();
                                                        let ctx_kw = ctx_clone.clone();
                                                        tokio::spawn(async move {
                                                            if let Ok(kws) = crate::chat::fetch_sticker_suggest_keywords(&s_kw).await {
                                                                if !kws.is_empty() {
                                                                    let _ = tx_kw.send(GuiEvent::StickerSuggestKeywordsLoaded(kws));
                                                                    ctx_kw.request_repaint();
                                                                }
                                                            }
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let _ = gui_tx_clone
                                            .send(GuiEvent::LoginFailed(e.to_string()));
                                        ctx_clone.request_repaint();
                                    }
                                }
                            });
                            current_qr_task = Some(handle);
                        }
                        ref cmd @ (WorkerCmd::RefreshThreads | WorkerCmd::RefreshThreadsManual) => {
                            let manual = matches!(cmd, WorkerCmd::RefreshThreadsManual);

                            let session_clone = command_session.clone();
                            let poll_clone = poll_state.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let mut s = session_clone.lock().await.clone();
                                if s.login.is_some() {
                                    match s.session_status().await {
                                        crate::session::SessionStatus::Valid => {}
                                        crate::session::SessionStatus::Expired => {
                                            // Keep `session.json`: the old credentials stay
                                            // readable until a new login replaces the file (#21).
                                            {
                                                let mut sl = session_clone.lock().await; if !gui_tx_clone.is_current() { return; }
                                                sl.mark_expired();
                                                let _ = sl.save_to_file("session.json");
                                                sl.logout();
                                            }
                                            let _ = gui_tx_clone.send(GuiEvent::SessionExpired(
                                                "Phiên đăng nhập (zpw_sek) đã hết hạn trên máy chủ Zalo"
                                                    .into(),
                                            ));
                                            ctx_clone.request_repaint();
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send_if_current(WorkerCmd::RequestQr, &gui_tx_clone);
                                            }
                                            return;
                                        }
                                        crate::session::SessionStatus::Unreachable => {
                                            // Offline: keep the session, say so, and let the
                                            // next poll retry.
                                            crate::logger::log_error(
                                                "gui::refresh_threads",
                                                "không kết nối được máy chủ Zalo",
                                            );
                                            let _ = gui_tx_clone.send(GuiEvent::SessionUnreachable(
                                                "Không kết nối được máy chủ Zalo".into(),
                                            ));
                                            ctx_clone.request_repaint();
                                            return;
                                        }
                                    }
                                    if let Some((uid, name)) = s.sync_profile_me().await {
                                        // `s` is a snapshot: copy every synced profile field
                                        // back, or the avatar and phone never reach the
                                        // saved session and the account stays unlabelled.
                                        let synced = s.login.clone().unwrap_or_default();
                                        {
                                            let mut sl = session_clone.lock().await; if !gui_tx_clone.is_current() { return; }
                                            if let Some(ref mut li) = sl.login {
                                                li.uin = uid.clone();
                                                if !name.is_empty() {
                                                    li.display_name = name.clone();
                                                }
                                                if !synced.avatar.is_empty() {
                                                    li.avatar = synced.avatar.clone();
                                                }
                                                if !synced.phone.is_empty() {
                                                    li.phone = synced.phone.clone();
                                                }
                                            }
                                            let _ = sl.save_to_file("session.json");
                                        }
                                        // An empty name must not blank the one already shown.
                                        let name = if name.is_empty() {
                                            synced.display_name.clone()
                                        } else {
                                            name
                                        };
                                        let u_ava = s
                                            .login
                                            .as_ref()
                                            .map(|l| l.avatar.clone())
                                            .filter(|a| !a.is_empty());
                                        let _ = gui_tx_clone.send(GuiEvent::LoginSuccess {
                                            name,
                                            uin: uid,
                                            avatar: u_ava,
                                        });
                                    }
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                        "⏳ Đang làm mới danh bạ & nhóm từ máy chủ Zalo (1/3)…".into(),
                                    ));
                                    ctx_clone.request_repaint();
                                    let mut list = crate::chat::load_threads_cache_for_account(s.login.as_ref().map(|l| l.uin.as_str()).unwrap_or_default());
                                    let mut errors = Vec::new();
                                    match crate::chat::get_friends(&s).await {
                                        Ok(friends) => {
                                            for ft in friends {
                                                if let Some(ex) = list.iter_mut().find(|t| t.id == ft.id) {
                                                    crate::chat::adopt_thread_name(
                                                        ex,
                                                        ft.name,
                                                        ft.presentation.contact_name,
                                                    );
                                                    if crate::chat::should_replace_avatar(
                                                        ex.avatar.as_deref(),
                                                        ft.avatar.as_deref(),
                                                    ) {
                                                        ex.avatar = ft.avatar;
                                                    }
                                                    ex.is_group = ex.is_group || ft.is_group;
                                                } else {
                                                    list.push(ft);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            crate::logger::log_error("gui::refresh_friends", &e.to_string());
                                            errors.push(format!("Danh bạ: {e}"));
                                        }
                                    }
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                        "⏳ Đang tải danh sách nhóm Zalo (2/3)…".into(),
                                    ));
                                    ctx_clone.request_repaint();
                                    match crate::chat::get_groups(&s).await {
                                        Ok(groups) => {
                                            for gt in groups {
                                                if let Some(ex) = list.iter_mut().find(|t| t.id == gt.id) {
                                                    if !gt.name.is_empty() {
                                                        ex.name = gt.name;
                                                    }
                                                    if crate::chat::should_replace_avatar(
                                                        ex.avatar.as_deref(),
                                                        gt.avatar.as_deref(),
                                                    ) {
                                                        ex.avatar = gt.avatar;
                                                    }
                                                    ex.presentation.posting_restricted = gt.presentation.posting_restricted;
                                                    ex.is_group = true;
                                                } else {
                                                    list.push(gt);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            crate::logger::log_error("gui::refresh_groups", &e.to_string());
                                            errors.push(format!("Nhóm: {e}"));
                                        }
                                    }
                                    let uin = s.login.as_ref().map(|l| l.uin.as_str()).unwrap_or_default();
                                    let db_path = crate::store::db_path_for_account(uin);
                                    crate::store::load_history_into_threads(
                                        &db_path,
                                        &mut list,
                                        crate::store::MessageStore::PRELOAD_MSGS,
                                    );
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                        format!("⏳ Đang làm mới tin nhắn cho {} hội thoại (3/3)…", list.len()),
                                    ));
                                    ctx_clone.request_repaint();
                                    let mut fresh_ps = crate::chat::PollState::default();
                                    if let Err(e) =
                                        crate::chat::poll_new_messages(&s, &mut fresh_ps, &mut list)
                                            .await
                                    {
                                        crate::logger::log_error("gui::refresh_poll", &e.to_string());
                                        errors.push(format!("Tin nhắn: {e}"));
                                    }
                                    {
                                        let mut ps = poll_clone.lock().await;
                                        *ps = fresh_ps;
                                    }
                                    if let Ok(mute_resp) =
                                        crate::chat::get_mute_conversations(&s).await
                                    {
                                        crate::unread::merge_mute_metadata(&mut list, &mute_resp);
                                        if let Some(mute_map) = crate::unread::extract_mute_map(
                                            &mute_resp,
                                            crate::login::unix_now() as i64,
                                        ) {
                                            let _ = gui_tx_clone
                                                .send(GuiEvent::MuteSnapshotSynced(mute_map));
                                        }
                                    }
                                    if let Ok(pinned_resp) = crate::chat::get_pinned(&s).await {
                                        let pids = crate::chat::parse_pinned_conversations(&pinned_resp);
                                        crate::chat::apply_pinned_threads(&mut list, &pids);
                                        let _ = gui_tx_clone.send(GuiEvent::PinnedSnapshotSynced(pids));
                                    }
                                    let total_msgs: usize =
                                        list.iter().map(|t| t.messages.len()).sum();
                                    let n_threads = list.len();
                                    let _ = gui_tx_clone.send(GuiEvent::ThreadsUpdated(list));
                                    let _ = gui_tx_clone.send(GuiEvent::RefreshFinished {
                                        threads: n_threads,
                                        messages: total_msgs,
                                        errors,
                                        manual,
                                    });
                                    ctx_clone.request_repaint();
                                }
                            });
                        }
                        WorkerCmd::PollMessages { known_threads } => {
                            static IS_POLLING: std::sync::atomic::AtomicBool =
                                std::sync::atomic::AtomicBool::new(false);
                            if IS_POLLING.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                continue;
                            }
                            let poll_permit = RunningPermit(&IS_POLLING);


                            let session_clone = command_session.clone();
                            let poll_clone = poll_state.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let _poll_permit = poll_permit;
                                let s = session_clone.lock().await.clone();
                                if s.login.is_some() {
                                    let mut temp_threads: Vec<Thread> = known_threads
                                        .into_iter()
                                        .map(|(id, name, is_group)| Thread {
                                            presentation: Default::default(),
                                            unread: Default::default(),
                                            id,
                                            name,
                                            preview: String::new(),
                                            time: String::new(),
                                            avatar: None,
                                            is_group,
                                            messages: Vec::new(),
                                            vanish: None,
                                        })
                                        .collect();

                                    let mut poll_err: Option<anyhow::Error> = None;
                                    for _ in 0..3 {
                                        let poll_res = {
                                            let mut ps = poll_clone.lock().await;
                                            crate::chat::get_chat_from_polling(&s, &mut ps).await
                                        };
                                        match poll_res {
                                            Ok(lp_resp) => {
                                                crate::chat::merge_previews(
                                                    &mut temp_threads,
                                                    &lp_resp,
                                                );
                                                let data_node =
                                                    lp_resp.get("data").unwrap_or(&lp_resp);
                                                queue_mobile_sync_controls(
                                                    crate::chat::extract_mobile_sync_controls(data_node),
                                                    &s,
                                                    &gui_tx_clone,
                                                    false,
                                                );
                                                let more = data_node
                                                    .get("more")
                                                    .and_then(serde_json::Value::as_i64)
                                                    .unwrap_or(0);
                                                let has_reset = data_node
                                                    .get("resetLastActionId")
                                                    .and_then(serde_json::Value::as_str)
                                                    .map(|r| r != "0" && !r.is_empty())
                                                    .unwrap_or(false);
                                                if more == 0 && !has_reset {
                                                    break;
                                                }
                                            }
                                            Err(e) => {
                                                poll_err = Some(e);
                                                break;
                                            }
                                        }
                                    }

                                    match poll_err {
                                        Some(e) => {
                                            if crate::session::SessionStatus::from_error(&e)
                                                == crate::session::SessionStatus::Expired
                                            {
                                                {
                                                    let mut sl = session_clone.lock().await; if !gui_tx_clone.is_current() { return; }
                                                    sl.mark_expired();
                                                    let _ = sl.save_to_file("session.json");
                                                    sl.logout();
                                                }
                                                let _ = gui_tx_clone.send(GuiEvent::SessionExpired(
                                                    "Phiên đăng nhập (zpw_sek) đã hết hạn".into(),
                                                ));
                                                ctx_clone.request_repaint();
                                                if let Some(tx) = WORKER_TX.get() {
                                                    let _ = tx.send_if_current(WorkerCmd::RequestQr, &gui_tx_clone);
                                                }
                                            } else {
                                                // The session is untouched: report and retry on
                                                // the next poll instead of logging the user out.
                                                crate::logger::log_error(
                                                    "gui::poll_messages",
                                                    &e.to_string(),
                                                );
                                                let _ = gui_tx_clone.send(
                                                    GuiEvent::SessionUnreachable(e.to_string()),
                                                );
                                                ctx_clone.request_repaint();
                                            }
                                        }
                                        None => {
                                            temp_threads.retain(|t| !t.messages.is_empty());
                                            if !temp_threads.is_empty() {
                                                // Skip UI churn when nothing actually changed (#135:
                                                // 1.2s poll used to relayout the whole message list
                                                // every tick even with zero new messages).
                                                let fingerprint = |ts: &[Thread]| -> u64 {
                                                    use std::hash::{Hash, Hasher};
                                                    let mut h = std::collections::hash_map::DefaultHasher::new();
                                                    for th in ts {
                                                        th.id.hash(&mut h);
                                                        th.preview.hash(&mut h);
                                                        th.messages.len().hash(&mut h);
                                                        th.messages
                                                            .last()
                                                            .map(|m| m.id.clone())
                                                            .unwrap_or_default()
                                                            .hash(&mut h);
                                                        th.unread.last_seen_id.hash(&mut h);
                                                        th.unread.unread_ids.len().hash(&mut h);
                                                        th.unread.marked_unread.hash(&mut h);
                                                    }
                                                    h.finish()
                                                };
                                                static LAST_SENT: std::sync::Mutex<u64> =
                                                    std::sync::Mutex::new(0);
                                                let fp = fingerprint(&temp_threads);
                                                let is_fresh = match LAST_SENT.lock() {
                                                    Ok(mut g) => {
                                                        if *g == fp {
                                                            false
                                                        } else {
                                                            *g = fp;
                                                            true
                                                        }
                                                    }
                                                    Err(_) => true,
                                                };
                                                if is_fresh {
                                                    // Deliver polled messages to UI immediately without blocking on profile HTTP
                                                    let _ = gui_tx_clone
                                                        .send(GuiEvent::ThreadsUpdated(temp_threads.clone()));
                                                    ctx_clone.request_repaint();
                                                }
                                                let needs_resolve = temp_threads.iter().any(|t| {
                                                    !t.presentation.contact_name
                                                        && (t.name.is_empty()
                                                            || crate::chat::is_placeholder_thread_name(&t.name, &t.id)
                                                            || crate::chat::is_generic_thread_name(&t.name))
                                                });
                                                if needs_resolve {
                                                    crate::chat::resolve_thread_names(
                                                        &s,
                                                        &mut temp_threads,
                                                    )
                                                    .await;
                                                    let _ = gui_tx_clone
                                                        .send(GuiEvent::ThreadsUpdated(temp_threads.clone()));
                                                    ctx_clone.request_repaint();
                                                }
                                                let rules = crate::api_mcp::load_autoreply_rules();
                                                if rules.iter().any(|r| r.enabled) {
                                                    for t in &temp_threads {
                                                        for m in &t.messages {
                                                            if !m.is_outgoing {
                                                                if let Some(matched) = crate::api_mcp::match_autoreply_rule(&rules, &m.text, t.is_group) {
                                                                    let reply_text = matched.reply_text;
                                                                    let client_id = crate::chat::next_client_message_id();
                                                                    let res = if t.is_group {
                                                                        crate::chat::send_group_message_with_client_id(&s, &t.id, &reply_text, client_id).await
                                                                    } else {
                                                                        crate::chat::send_message_with_client_id(&s, &t.id, &reply_text, client_id).await
                                                                    };
                                                                    if let Ok(resp) = res {
                                                                        if resp_error_code(&resp).is_none() {
                                                                            let msg_id = extract_msg_id(&resp);
                                                                            let _ = gui_tx_clone.send(GuiEvent::MessageSent {
                                                                                client_message_id: client_id.to_string(),
                                                                                thread_id: t.id.clone(),
                                                                                text: reply_text,
                                                                                msg_id,
                                                                            });
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            });
                        }
                        WorkerCmd::SyncHistory { threads } => {
                            use std::sync::atomic::Ordering;
                            if crate::history_sync::RUNNING.swap(true, Ordering::SeqCst) {
                                continue;
                            }
                            crate::history_sync::CANCEL.store(false, Ordering::Relaxed);
                            let running_permit = RunningPermit(&crate::history_sync::RUNNING);
                            let writer = ScopedHistoryWriter::new(&mut history_writes);

                            let s = command_session.lock().await.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {
                                let _running_permit = running_permit;
                                let uin = s
                                    .login
                                    .as_ref()
                                    .map(|l| l.uin.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                let total = threads.len();
                                let mut added = 0;
                                let mut errors = Vec::new();
                                let mut synced_threads = Vec::with_capacity(total);
                                for (i, (id, name, is_group)) in threads.into_iter().enumerate() {
                                    if crate::history_sync::CANCEL.load(Ordering::Relaxed) {
                                        break;
                                    }
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                        "Đang đồng bộ {}/{total} · {name}",
                                        i + 1
                                    )));
                                    ctx_clone.request_repaint();
                                    let mut t = Thread {
                                        id,
                                        name,
                                        preview: String::new(),
                                        time: String::new(),
                                        avatar: None,
                                        is_group,
                                        messages: Vec::new(),
                                        ..Default::default()
                                    };
                                    let persist_page = |uin: String, snapshot: Thread| {
                                        let writer = writer.clone();
                                        async move {
                                            writer.write(move || crate::history_sync::persist_thread_snapshot(&uin, snapshot)).await
                                        }
                                    };
                                    match crate::history_sync::sync_thread_with_writer(&s, &mut t, persist_page).await {
                                        Ok(n) => {
                                            added += n;
                                            if !t.messages.is_empty() {
                                                synced_threads.push(t);
                                            }
                                        }
                                        Err(e) => errors.push(format!("{}: {e}", t.name)),
                                    }
                                }
                                let db_path = crate::store::db_path_for_account(&uin);
                                match writer.write(move || {
                                    let mut updated_threads = crate::chat::load_threads_cache_for_account(&uin);
                                    crate::store::load_history_into_threads(
                                        &db_path,
                                        &mut updated_threads,
                                        crate::store::MessageStore::PRELOAD_MSGS,
                                    );
                                    for synced in synced_threads {
                                        if let Some(existing) =
                                            updated_threads.iter_mut().find(|x| x.id == synced.id)
                                        {
                                            crate::chat::merge_thread_into(existing, synced, false);
                                        } else {
                                            updated_threads.push(synced);
                                        }
                                    }
                                    crate::chat::save_threads_cache_for_account(&uin, &updated_threads);
                                    Ok(updated_threads)
                                })
                                .await
                                {
                                    Ok(updated_threads) => {
                                        let _ = gui_tx_clone.send(GuiEvent::ThreadsUpdated(updated_threads));
                                    }
                                    Err(error) => errors.push(error.to_string()),
                                }
                                let _ = gui_tx_clone.send(GuiEvent::SyncFinished {
                                    added,
                                    errors,
                                    cancelled: crate::history_sync::CANCEL.load(Ordering::Relaxed),
                                });
                                ctx_clone.request_repaint();

                            });
                        }
                        WorkerCmd::LoadGroupHistory {
                            group_id,
                            is_group,
                            have,
                        } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                if s.login.is_some() {
                                    let is_send2me = s
                                        .login
                                        .as_ref()
                                        .map(|l| !l.send2me_id.is_empty() && l.send2me_id == group_id)
                                        .unwrap_or(false);
                                    let t = Thread {
                                        presentation: Default::default(),
                                        unread: Default::default(),
                                        id: group_id.clone(),
                                        name: String::new(),
                                        preview: String::new(),
                                        time: String::new(),
                                        avatar: None,
                                        is_group: is_group && !is_send2me,
                                        messages: Vec::new(),
                                        vanish: None,
                                    };
                                    let mut t_list = vec![t];
                                    let uin = s.login.as_ref().map(|l| l.uin.as_str()).unwrap_or_default();
                                    let db_path = crate::store::db_path_for_account(uin);
                                    crate::store::load_history_into_threads(
                                        &db_path,
                                        &mut t_list,
                                        (have + 300).max(500),
                                    );
                                    let Some(mut t) =
                                        t_list.into_iter().find(|x| x.id == group_id)
                                    else {
                                        return;
                                    };
                                    if !is_group && !is_send2me {
                                        let mut one_list = vec![t.clone()];
                                        if let Ok(resp) = crate::chat::get_last_msgs(
                                            &s,
                                            std::slice::from_ref(&group_id),
                                        )
                                        .await
                                        {
                                            crate::chat::merge_previews(&mut one_list, &resp);
                                        }
                                        if let Ok(resp_all) =
                                            crate::chat::get_last_msgs(&s, &[]).await
                                        {
                                            crate::chat::merge_previews(&mut one_list, &resp_all);
                                        }
                                        if let Some(updated) =
                                            one_list.into_iter().find(|x| x.id == group_id)
                                        {
                                            t = updated;
                                        }
                                    }
                                    if is_group || is_send2me {
                                        let (mut has_more, mut is_old, mut server_cursor) =
                                            (false, false, None);
                                        if let Ok(cm_resp) = crate::chat::get_cloud_messages(
                                            &s,
                                            &group_id,
                                            "9223372036854775808",
                                            false,
                                            50,
                                        )
                                        .await
                                        {
                                            crate::chat::merge_group_history(&mut t, &cm_resp);
                                            (has_more, is_old, server_cursor) =
                                                crate::chat::parse_cloud_page_meta(&cm_resp);
                                        }
                                        if has_more {
                                            // Backfill up to 5 pages (250 messages) of older cloud history
                                            let mut cursor_id = t
                                                .messages
                                                .first()
                                                .map(|m| m.id.clone())
                                                .filter(|id| {
                                                    !id.is_empty()
                                                        && id.chars().all(|c| c.is_ascii_digit())
                                                })
                                                .or(server_cursor)
                                                .unwrap_or_else(|| "9223372036854775808".to_string());
                                            for page in 1..=5 {
                                                let prev_len = t.messages.len();
                                                let Ok(cm_old) = crate::chat::get_cloud_messages(
                                                    &s,
                                                    &group_id,
                                                    &cursor_id,
                                                    is_old,
                                                    50,
                                                )
                                                .await
                                                else {
                                                    break;
                                                };
                                                crate::chat::merge_group_history(&mut t, &cm_old);
                                                let new_len = t.messages.len();
                                                let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                                    format!(
                                                        "⏳ Đang đồng bộ lịch sử cũ (trang {page}/5 — {new_len} tin nhắn)…"
                                                    ),
                                                ));
                                                ctx_clone.request_repaint();
                                                let (more_after, next_is_old, resp_cursor) =
                                                    crate::chat::parse_cloud_page_meta(&cm_old);
                                                if new_len == prev_len
                                                    && !is_old
                                                    && next_is_old
                                                    && more_after
                                                {
                                                    is_old = true;
                                                    continue;
                                                }
                                                is_old = next_is_old;
                                                let next_cursor = resp_cursor.or_else(|| {
                                                    t.messages
                                                        .first()
                                                        .map(|m| m.id.clone())
                                                        .filter(|id| {
                                                            !id.is_empty()
                                                                && id
                                                                    .chars()
                                                                    .all(|c| c.is_ascii_digit())
                                                        })
                                                });
                                                let Some(nc) = next_cursor else {
                                                    break;
                                                };
                                                if !more_after
                                                    || nc == cursor_id
                                                    || new_len == prev_len
                                                {
                                                    break;
                                                }
                                                cursor_id = nc;
                                            }
                                        }
                                    }
                                    if is_group
                                        && !is_send2me
                                        && (crate::chat::is_generic_thread_name(&t.name)
                                            || t.avatar.as_deref().unwrap_or("").is_empty())
                                    {
                                        crate::chat::resolve_thread_names(
                                            &s,
                                            std::slice::from_mut(&mut t),
                                        )
                                        .await;
                                    }
                                    let cnt = t.messages.len();
                                    if !t.messages.is_empty() || !t.name.is_empty() {
                                        let uin = s.login.as_ref().map(|l| l.uin.as_str()).unwrap_or_default();
                                        let db_path = crate::store::db_path_for_account(uin);
                                        if let Ok(mut store) =
                                            crate::store::MessageStore::open(&db_path)
                                        {
                                            let _ = crate::store::persist_threads(
                                                &mut store,
                                                std::slice::from_ref(&t),
                                            );
                                        }
                                        let _ = gui_tx_clone.send(GuiEvent::ThreadsUpdated(vec![t]));
                                    }
                                    if cnt <= have {
                                        let _ = gui_tx_clone.send(GuiEvent::HistoryReachedStart(group_id.clone()));
                                    } else {
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "✅ Đã tải thêm {} tin nhắn cũ ({cnt} tin trong hội thoại)",
                                            cnt.saturating_sub(have)
                                        )));
                                    }
                                    ctx_clone.request_repaint();
                                }
                            });
                        }
                        WorkerCmd::SendMessage {
                            to_id,
                            text,
                            is_group,
                        } => {

                            let session_clone = command_session.clone();
                            let session_state = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let client_id = crate::chat::next_client_message_id();
                                if let Some(found_url) = crate::chat::extract_first_url(&text) {
                                    let meta = crate::chat::parse_link_metadata(&s, &found_url)
                                        .await
                                        .unwrap_or_else(|_| crate::chat::LinkMetadata {
                                            url: found_url.clone(),
                                            title: found_url.clone(),
                                            desc: String::new(),
                                            thumb: String::new(),
                                            src: "Liên kết".into(),
                                        });
                                    let _ =
                                        gui_tx_clone.send(GuiEvent::LinkPreviewReady(meta.clone()));
                                    match crate::chat::send_link_message_with_client_id(
                                        &s, &to_id, &text, &meta, is_group, client_id)
                                    .await
                                    {
                                        Ok(resp) => {
                                            let msg_id = extract_msg_id(&resp);
                                            let now = crate::ui::clock();
                                            let final_id = if msg_id.is_empty() {
                                                format!("out_{client_id}")
                                            } else {
                                                msg_id
                                            };
                                            let msg = ChatMessage {
                                                id: final_id,
                                                client_message_id: client_id.to_string(),
                                                sender: "Tôi".into(),
                                                text,
                                                time: now,
                                                is_outgoing: true,
                                                ts: (crate::login::unix_now() * 1000) as i64,
                                                delivery: crate::message_model::DeliveryState::ServerAccepted,
                                                link_url: Some(meta.url),
                                                link_title: if meta.title.is_empty() {
                                                    None
                                                } else {
                                                    Some(meta.title)
                                                },
                                                link_desc: if meta.desc.is_empty() {
                                                    None
                                                } else {
                                                    Some(meta.desc)
                                                },
                                                link_thumb: if meta.thumb.is_empty() {
                                                    None
                                                } else {
                                                    Some(meta.thumb)
                                                },
                                                link_src: if meta.src.is_empty() {
                                                    None
                                                } else {
                                                    Some(meta.src)
                                                },
                                                ..Default::default()
                                            };
                                            let _ = gui_tx_clone.send(GuiEvent::AttachmentSent {
                                                thread_id: to_id,
                                                msg,
                                            });
                                            ctx_clone.request_repaint();
                                            return;
                                        }
                                        Err(e) => {
                                            let err_s = e.to_string();
                                            crate::logger::log_error("gui::send_link", &err_s);
                                            let _ = gui_tx_clone.send(GuiEvent::SendFailed {
                                                thread_id: to_id,
                                                text,
                                                error: err_s,
                                            });
                                            ctx_clone.request_repaint();
                                            return;
                                        }
                                    }
                                }

                                let res = if is_group {
                                    crate::chat::send_group_message_with_client_id(&s, &to_id, &text, client_id).await
                                } else {
                                    crate::chat::send_message_with_client_id(&s, &to_id, &text, client_id).await
                                };
                                match res {
                                    Ok(resp) => {
                                        let msg_id = extract_msg_id(&resp);
                                        let _ = gui_tx_clone.send(GuiEvent::MessageSent {
                                            client_message_id: client_id.to_string(),
                                            thread_id: to_id,
                                            text,
                                            msg_id,
                                        });
                                        ctx_clone.request_repaint();
                                    }
                                    Err(e) => {
                                        let err_s = e.to_string();
                                        crate::logger::log_error("gui::send_message", &err_s);
                                        let _ = gui_tx_clone.send(GuiEvent::SendFailed {
                                            thread_id: to_id,
                                            text,
                                            error: err_s.clone(),
                                        });
                                        if crate::session::SessionStatus::from_error(&e)
                                            == crate::session::SessionStatus::Expired
                                        {
                                            {
                                                let mut sl = session_state.lock().await; if !gui_tx_clone.is_current() { return; }
                                                sl.mark_expired();
                                                let _ = sl.save_to_file("session.json");
                                                sl.logout();
                                            }
                                            let _ = gui_tx_clone.send(GuiEvent::SessionExpired(
                                                "Không thể gửi vì phiên zpw_sek đã hết hạn".into(),
                                            ));
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send_if_current(WorkerCmd::RequestQr, &gui_tx_clone);
                                            }
                                        }
                                        ctx_clone.request_repaint();
                                    }
                                }
                            });
                        }
                        WorkerCmd::SendQuote {
                            to_id,
                            text,
                            quoted,
                            is_group,
                        } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let client_id = crate::chat::next_client_message_id();
                                match crate::chat::send_quote_message_with_client_id(
                                    &s, &to_id, &text, &quoted, is_group, client_id)
                                .await
                                {
                                    Ok(resp) => {
                                        let _ = gui_tx_clone.send(GuiEvent::QuoteSent {
                                            client_message_id: client_id.to_string(),
                                            thread_id: to_id,
                                            text,
                                            msg_id: extract_msg_id(&resp),
                                            quoted,
                                        });
                                    }
                                    Err(e) => {
                                        let err_s = e.to_string();
                                        crate::logger::log_error("gui::send_quote", &err_s);
                                        let _ = gui_tx_clone.send(GuiEvent::SendFailed {
                                            thread_id: to_id,
                                            text,
                                            error: err_s,
                                        });
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::Forward {
                            target_id,
                            target_is_group,
                            msg,
                            target_name,
                        } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let client_id = crate::chat::next_client_message_id();
                                let res = crate::chat::forward_message_with_client_id(
                                    &s,
                                    &target_id,
                                    target_is_group,
                                    &msg, client_id)
                                .await;
                                match res {
                                    Ok(resp) if resp_error_code(&resp).is_none() => {
                                        let msg_id = extract_msg_id(&resp);
                                        if let Some(sid) = msg.sticker_id.filter(|v| *v > 0) {
                                            let _ = gui_tx_clone.send(GuiEvent::StickerSent {
                                                client_message_id: client_id.to_string(),
                                                thread_id: target_id.clone(),
                                                msg_id,
                                                sticker_id: sid,
                                                cate_id: msg.sticker_cate_id.unwrap_or(0),
                                                sticker_type: msg.sticker_type.unwrap_or(7),
                                            });
                                        } else if msg.link_url.is_some() {
                                            let clean_text = if let Some(stripped) = msg.text.strip_prefix("[Đã thu hồi] ") {
                                                stripped
                                            } else {
                                                &msg.text
                                            };
                                            let out_msg = ChatMessage {
                                                id: if msg_id.is_empty() { format!("out_{client_id}") } else { msg_id },
                                                client_message_id: client_id.to_string(),
                                                sender: "Tôi".into(),
                                                text: clean_text.to_string(),
                                                time: crate::ui::clock(),
                                                is_outgoing: true,
                                                ts: (crate::login::unix_now() * 1000) as i64,
                                                delivery: crate::message_model::DeliveryState::ServerAccepted,
                                                link_url: msg.link_url.clone(),
                                                link_thumb: msg.link_thumb.clone(),
                                                link_src: msg.link_src.clone(),
                                                ..Default::default()
                                            };
                                            let _ = gui_tx_clone.send(GuiEvent::AttachmentSent {
                                                thread_id: target_id.clone(),
                                                msg: out_msg,
                                            });
                                        } else {
                                            let clean_text = if let Some(stripped) = msg.text.strip_prefix("[Đã thu hồi] ") {
                                                stripped
                                            } else {
                                                &msg.text
                                            };
                                            let _ = gui_tx_clone.send(GuiEvent::MessageSent {
                                                client_message_id: client_id.to_string(),
                                                thread_id: target_id.clone(),
                                                text: clean_text.to_string(),
                                                msg_id,
                                            });
                                        }
                                        let _ = gui_tx_clone.send(GuiEvent::ForwardFinished {
                                            target_name: target_name.clone(),
                                            error: None,
                                        });
                                    }
                                    Ok(resp) => {
                                        let err_s = format!(
                                            "server error {}",
                                            resp_error_code(&resp).unwrap_or(0)
                                        );
                                        crate::logger::log_error("gui::forward", &err_s);
                                        let _ = gui_tx_clone.send(GuiEvent::ForwardFinished {
                                            target_name,
                                            error: Some(err_s),
                                        });
                                    }
                                    Err(e) => {
                                        let err_s = e.to_string();
                                        crate::logger::log_error("gui::forward", &err_s);
                                        let _ = gui_tx_clone.send(GuiEvent::ForwardFinished {
                                            target_name,
                                            error: Some(err_s),
                                        });
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::React {
                            to_id,
                            is_group,
                            msg,
                            reaction,
                        } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let res = crate::chat::send_reaction(
                                    &s, &to_id, is_group, &msg, &reaction,
                                )
                                .await;
                                match res {
                                    Ok(resp) if resp_error_code(&resp).is_none() => {
                                        let _ = gui_tx_clone.send(GuiEvent::Reacted {
                                            thread_id: to_id,
                                            msg_id: msg.id.clone(),
                                            ts: msg.ts,
                                            reaction,
                                        });
                                    }
                                    Ok(resp) => {
                                        let err_s = format!(
                                            "server error {}",
                                            resp_error_code(&resp).unwrap_or(0)
                                        );
                                        crate::logger::log_error("gui::react", &err_s);
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Thả cảm xúc thất bại: {err_s}"
                                        )));
                                    }
                                    Err(e) => {
                                        crate::logger::log_error("gui::react", &e.to_string());
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Thả cảm xúc thất bại: {e}"
                                        )));
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::DeleteForMe {
                            to_id,
                            is_group,
                            msg,
                        } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let local_only = msg.id.is_empty()
                                    || msg.id.starts_with("out_")
                                    || msg.id.starts_with("local_");
                                let res: anyhow::Result<serde_json::Value> = if local_only {
                                    Ok(serde_json::json!({}))
                                } else {
                                    crate::chat::delete_message_for_me(&s, &to_id, is_group, &msg)
                                        .await
                                };
                                match res {
                                    Ok(resp) if resp_error_code(&resp).is_none() => {
                                        let (tid, mid, ts) =
                                            (to_id.clone(), msg.id.clone(), msg.ts);
                                        let uin = s.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
                                        let _ = tokio::task::spawn_blocking(move || {
                                            if let Ok(mut store) =
                                                crate::store::MessageStore::open_account(&uin)
                                            {
                                                let _ = store.delete_message(&tid, &mid, ts);
                                            }
                                        })
                                        .await;
                                        let _ = gui_tx_clone.send(GuiEvent::MessageDeleted {
                                            thread_id: to_id,
                                            msg_id: msg.id.clone(),
                                            ts: msg.ts,
                                        });
                                    }
                                    Ok(resp) => {
                                        let err_s = format!(
                                            "server error {}",
                                            resp_error_code(&resp).unwrap_or(0)
                                        );
                                        crate::logger::log_error("gui::delete", &err_s);
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Xóa tin nhắn thất bại: {err_s}"
                                        )));
                                    }
                                    Err(e) => {
                                        crate::logger::log_error("gui::delete", &e.to_string());
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Xóa tin nhắn thất bại: {e}"
                                        )));
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::RecallMessage {
                            to_id,
                            is_group,
                            msg,
                        } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let local_only = msg.id.is_empty()
                                    || msg.id.starts_with("out_")
                                    || msg.id.starts_with("local_");
                                let res: anyhow::Result<serde_json::Value> = if local_only {
                                    Ok(serde_json::json!({}))
                                } else {
                                    crate::chat::recall_message(&s, &to_id, is_group, &msg).await
                                };
                                match res {
                                    Ok(resp) if resp_error_code(&resp).is_none() => {
                                        let _ = gui_tx_clone.send(GuiEvent::MessageRecalled {
                                            thread_id: to_id,
                                            msg_id: msg.id.clone(),
                                            ts: msg.ts,
                                        });
                                    }
                                    Ok(resp) => {
                                        let err_s = format!(
                                            "server error {}",
                                            resp_error_code(&resp).unwrap_or(0)
                                        );
                                        crate::logger::log_error("gui::recall", &err_s);
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Đã thu hồi cục bộ (Máy chủ: {err_s})"
                                        )));
                                    }
                                    Err(e) => {
                                        crate::logger::log_error("gui::recall", &e.to_string());
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Không thể thu hồi trên máy chủ: {e}"
                                        )));
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::EditMessageCommit {
                            to_id,
                            is_group,
                            old_msg,
                            new_text,
                        } => {
                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {
                                let s = session_clone.lock().await.clone();
                                let old_id = old_msg.id.clone();
                                let old_text = old_msg.text.clone();
                                // 1. Recall tin cũ (local-only thì bỏ qua gọi mạng).
                                let local_only = old_id.is_empty()
                                    || old_id.starts_with("out_")
                                    || old_id.starts_with("local_");
                                if !local_only {
                                    let _ =
                                        crate::chat::recall_message(&s, &to_id, is_group, &old_msg)
                                            .await;
                                }
                                // 2. Gửi tin mới với text đã sửa.
                                let client_id = crate::chat::next_client_message_id();
                                let res = if is_group {
                                    crate::chat::send_group_message_with_client_id(
                                        &s, &to_id, &new_text, client_id,
                                    )
                                    .await
                                } else {
                                    crate::chat::send_message_with_client_id(
                                        &s, &to_id, &new_text, client_id,
                                    )
                                    .await
                                };
                                match res {
                                    Ok(resp) if resp_error_code(&resp).is_none() => {
                                        let msg_id = extract_msg_id(&resp);
                                        let final_id = if msg_id.is_empty() {
                                            format!("out_{client_id}")
                                        } else {
                                            msg_id
                                        };
                                        let _ = gui_tx_clone.send(GuiEvent::MessageEdited {
                                            thread_id: to_id,
                                            old_msg_id: old_id,
                                            old_text,
                                            new_text,
                                            new_msg_id: final_id,
                                            new_ts: (crate::login::unix_now() * 1000) as i64,
                                        });
                                    }
                                    Ok(resp) => {
                                        let err_s = format!(
                                            "server error {}",
                                            resp_error_code(&resp).unwrap_or(0)
                                        );
                                        crate::logger::log_error("gui::edit", &err_s);
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Sửa tin nhắn thất bại: {err_s}"
                                        )));
                                    }
                                    Err(e) => {
                                        crate::logger::log_error("gui::edit", &e.to_string());
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Sửa tin nhắn thất bại: {e}"
                                        )));
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::SendSticker {
                            to_id,
                            sticker_id,
                            cate_id,
                            sticker_type,
                            is_group,
                        } => {

                            let session_clone = command_session.clone();
                            let session_state = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            if cate_id == AMI_FAT_CAT_CATE_ID {
                                if let Some((_lbl, png_bytes, gif_bytes)) =
                                    get_ami_fat_cat_gif(sticker_id)
                                {
                                    tasks.spawn(async move {
                                        let idx = sticker_id.saturating_sub(999000);
                                        let gif_path =
                                            crate::account_manager::ensure_media_cache_dir()
                                                .join(format!("ami_fat_cat_{idx:02}.gif"));
                                        if !gif_path.exists() {
                                            let _ = std::fs::write(&gif_path, gif_bytes);
                                        }
                                        let key = format!("stk_{cate_id}_{sticker_id}");
                                        if let Ok(dyn_img) = image::load_from_memory(png_bytes) {
                                            let rgba = dyn_img.to_rgba8();
                                            let size =
                                                [rgba.width() as usize, rgba.height() as usize];
                                            let pixels = rgba.as_flat_samples();
                                            let color_img =
                                                egui::ColorImage::from_rgba_unmultiplied(
                                                    size,
                                                    pixels.as_slice(),
                                                );
                                            let _ = gui_tx_clone
                                                .send(GuiEvent::StickerImageReady {
                                                    key,
                                                    image: color_img,
                                                });
                                        }
                                        let s = session_clone.lock().await.clone();
                                        let client_id = crate::chat::next_client_message_id();
                                        let ami_tag = format!("#ami_fat_cat_{idx:02}");
                                        match crate::chat::send_attachment(
                                            &s, &to_id, is_group, &gif_path, &ami_tag,
                                        )
                                        .await
                                        {
                                            Ok((_resp, cm)) => {
                                                let _ = gui_tx_clone.send(GuiEvent::StickerSent {
                                                    client_message_id: cm.client_message_id,
                                                    thread_id: to_id,
                                                    msg_id: cm.id,
                                                    sticker_id,
                                                    cate_id,
                                                    sticker_type,
                                                });
                                            }
                                            Err(e) => {
                                                let err_s = e.to_string();
                                                crate::logger::log_error(
                                                    "gui::send_ami_sticker",
                                                    &err_s,
                                                );
                                                let _ = gui_tx_clone.send(GuiEvent::StickerSent {
                                                    client_message_id: client_id.to_string(),
                                                    thread_id: to_id,
                                                    msg_id: format!("out_{client_id}"),
                                                    sticker_id,
                                                    cate_id,
                                                    sticker_type,
                                                });
                                            }
                                        }
                                        ctx_clone.request_repaint();
                                    });
                                    continue;
                                }
                            }
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let client_id = crate::chat::next_client_message_id();
                                let res = crate::chat::send_sticker_with_client_id(
                                    &s,
                                    &to_id,
                                    sticker_id,
                                    cate_id,
                                    sticker_type,
                                    is_group, client_id)
                                .await;
                                match res {
                                    Ok(resp) => {
                                        let msg_id = resp
                                            .get("data")
                                            .and_then(|d| d.get("msgId"))
                                            .and_then(|v| match v {
                                                serde_json::Value::String(s) => Some(s.clone()),
                                                serde_json::Value::Number(n) => Some(n.to_string()),
                                                _ => None,
                                            })
                                            .unwrap_or_default();
                                        let _ = gui_tx_clone.send(GuiEvent::StickerSent {
                                            client_message_id: client_id.to_string(),
                                            thread_id: to_id,
                                            msg_id,
                                            sticker_id,
                                            cate_id,
                                            sticker_type,
                                        });
                                        ctx_clone.request_repaint();
                                    }
                                    Err(e) => {
                                        let err_s = e.to_string();
                                        crate::logger::log_error("gui::send_sticker", &err_s);
                                        let _ = gui_tx_clone.send(GuiEvent::SendFailed {
                                            thread_id: to_id,
                                            text: String::new(),
                                            error: err_s.clone(),
                                        });
                                        if crate::session::SessionStatus::from_error(&e)
                                            == crate::session::SessionStatus::Expired
                                        {
                                            {
                                                let mut sl = session_state.lock().await; if !gui_tx_clone.is_current() { return; }
                                                sl.mark_expired();
                                                let _ = sl.save_to_file("session.json");
                                                sl.logout();
                                            }
                                            let _ = gui_tx_clone.send(GuiEvent::SessionExpired(
                                                "Không thể gửi nhãn dán vì phiên zpw_sek đã hết hạn"
                                                    .into(),
                                            ));
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send_if_current(WorkerCmd::RequestQr, &gui_tx_clone);
                                            }
                                        }
                                        ctx_clone.request_repaint();
                                    }
                                }
                            });
                        }
                        WorkerCmd::FetchStickerImage { key, url } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            if key.starts_with("stk_99901_") {
                                if let Some(sid) = key
                                    .strip_prefix("stk_99901_")
                                    .and_then(|s| s.parse::<u64>().ok())
                                {
                                    if let Some((_lbl, png_bytes, _gif)) = get_ami_fat_cat_gif(sid)
                                    {
                                        if let Ok(dyn_img) = image::load_from_memory(png_bytes) {
                                            let rgba = dyn_img.to_rgba8();
                                            let size =
                                                [rgba.width() as usize, rgba.height() as usize];
                                            let pixels = rgba.as_flat_samples();
                                            let color_img =
                                                egui::ColorImage::from_rgba_unmultiplied(
                                                    size,
                                                    pixels.as_slice(),
                                                );
                                            let _ = gui_tx.send(GuiEvent::StickerImageReady {
                                                key,
                                                image: color_img,
                                            });
                                            egui_ctx.request_repaint();
                                        }
                                        continue;
                                    }
                                }
                            }
                            tasks.spawn(async move {
                                let cache_dir =
                                    crate::account_manager::ensure_media_cache_dir();
                                let cache_path = cache_dir.join(format!("{key}.png"));

                                // Bytes come from the disk cache, a local file or the network. They
                                // used to be decoded once to validate and again to use; now once, on
                                // the blocking pool so a large photo cannot stall the 2-thread runtime.
                                let (bytes_opt, from_network) =
                                    if let Ok(disk_bytes) = std::fs::read(&cache_path) {
                                        (Some(disk_bytes), false)
                                    } else if !url.starts_with("http://")
                                        && !url.starts_with("https://")
                                    {
                                        let file_path = url.strip_prefix("local://").unwrap_or(&url);
                                        (std::fs::read(file_path).ok(), false)
                                    } else {
                                        let client = { session_clone.lock().await.clone().http };
                                        let fetch_url = crate::store::jxl_to_jpg_url(&url);
                                        let body = match client
                                            .get(&fetch_url)
                                            .header("Accept", "image/avif,image/webp,image/apng,image/*,*/*;q=0.8")
                                            .send()
                                            .await
                                        {
                                            Ok(resp) => {
                                                if resp.status().is_success() {
                                                    resp.bytes().await.ok()
                                                } else {
                                                    None
                                                }
                                            }
                                            Err(_) => None,
                                        };
                                        (body.map(|b| b.to_vec()), true)
                                    };
                                let Some(bytes) = bytes_opt else {
                                    let _ = gui_tx_clone.send(GuiEvent::StickerImageFailed { key });
                                    ctx_clone.request_repaint();
                                    return;
                                };

                                let _ = tokio::task::spawn_blocking(move || {
                                    let is_svg = bytes.len() >= 5
                                        && (bytes.starts_with(b"<svg")
                                            || bytes.starts_with(b"<?xml")
                                            || bytes.windows(5).any(|w| w == b"<svg "));
                                    if is_svg {
                                        // SVG không decode được bằng image crate: fail vĩnh viễn,
                                        // không xoá cache, bubble hiển thị link fallback (#5508).
                                        let _ = gui_tx_clone.send(GuiEvent::StickerImageFailed { key });
                                        ctx_clone.request_repaint();
                                        return;
                                    }
                                    let decoded = decode_image_with_fallback(&bytes, &url);
                                    let Some((mut dyn_img, transcoded_png)) = decoded else {
                                        if !from_network {
                                            // Corrupt cache entry: drop it so the next request refetches.
                                            let _ = std::fs::remove_file(&cache_path);
                                        }
                                        let _ = gui_tx_clone.send(GuiEvent::StickerImageFailed { key });
                                        ctx_clone.request_repaint();
                                        return;
                                    };
                                    if from_network {
                                        let to_cache = transcoded_png.as_deref().unwrap_or(&bytes);
                                        let _ = std::fs::write(&cache_path, to_cache);
                                    }
                                    // Bubbles draw images up to 360 pt wide; resize large photos to 720px max
                                    // so inline thumbnails stay crisp on Retina/HiDPI displays (#72).
                                    if !key.starts_with("ava_")
                                        && !key.starts_with("hd_")
                                        && (dyn_img.width() > 720 || dyn_img.height() > 720)
                                    {
                                        dyn_img = dyn_img.thumbnail(720, 720);
                                    }
                                    {
                                        let mut rgba = dyn_img.to_rgba8();
                                        // Apply smooth anti-aliased circular alpha mask for avatar textures (`ava_*`)
                                        if key.starts_with("ava_") {
                                            let side = rgba.width().min(rgba.height()).clamp(32, 160);
                                            let x0 = (rgba.width() - side) / 2;
                                            let y0 = (rgba.height() - side) / 2;
                                            let cropped = image::imageops::crop_imm(
                                                &rgba, x0, y0, side, side,
                                            )
                                            .to_image();
                                            rgba = if side > 128 {
                                                image::imageops::resize(
                                                    &cropped,
                                                    128,
                                                    128,
                                                    image::imageops::FilterType::Triangle,
                                                )
                                            } else {
                                                cropped
                                            };
                                            let w = rgba.width() as f32;
                                            let h = rgba.height() as f32;
                                            let cx = (w - 1.0) * 0.5;
                                            let cy = (h - 1.0) * 0.5;
                                            let r = w.min(h) * 0.5;
                                            for (x, y, px) in rgba.enumerate_pixels_mut() {
                                                let dx = x as f32 - cx;
                                                let dy = y as f32 - cy;
                                                let dist = (dx * dx + dy * dy).sqrt();
                                                if dist > r {
                                                    px[3] = 0;
                                                } else if dist > r - 1.4 {
                                                    let cov = ((r - dist) / 1.4).clamp(0.0, 1.0);
                                                    px[3] = (px[3] as f32 * cov) as u8;
                                                }
                                            }
                                        }
                                        let size =
                                            [rgba.width() as usize, rgba.height() as usize];
                                        let pixels = rgba.as_flat_samples();
                                        let color_img = egui::ColorImage::from_rgba_unmultiplied(
                                            size,
                                            pixels.as_slice(),
                                        );
                                        let _ = gui_tx_clone.send(GuiEvent::StickerImageReady {
                                            key,
                                            image: color_img,
                                        });
                                        ctx_clone.request_repaint();
                                    }
                                })
                                .await;
                            });
                        }
                        WorkerCmd::FetchStickerStore => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                match crate::chat::fetch_sticker_store_packs(&s).await {
                                    Ok(packs) => {
                                        let _ = gui_tx_clone
                                            .send(GuiEvent::StickerStoreLoaded(packs));
                                    }
                                    Err(e) => {
                                        crate::logger::log_error(
                                            "gui::fetch_sticker_store",
                                            &e.to_string(),
                                        );
                                        let _ = gui_tx_clone
                                            .send(GuiEvent::StickerStoreLoaded(Vec::new()));
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Không tải được kho nhãn dán: {e}"
                                        )));
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::FetchStickerPack { cate_id, name } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                match crate::chat::fetch_sticker_pack_items(&s, cate_id, &name)
                                    .await
                                {
                                    Ok(items) => {
                                        let _ = gui_tx_clone.send(
                                            GuiEvent::StickerPackItemsLoaded {
                                                cate_id,
                                                name,
                                                items,
                                            },
                                        );
                                    }
                                    Err(e) => {
                                        crate::logger::log_error(
                                            "gui::fetch_sticker_pack",
                                            &e.to_string(),
                                        );
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Không tải được bộ nhãn \"{name}\": {e}"
                                        )));
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::SearchStickers { keyword } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                match crate::chat::search_stickers_by_keyword(&s, &keyword).await {
                                    Ok(items) => {
                                        let missing = preload_disk_sticker_thumbnails(
                                            &items,
                                            &gui_tx_clone,
                                            24,
                                        );
                                        let _ = gui_tx_clone.send(GuiEvent::StickerSearchResults {
                                            keyword,
                                            items,
                                        });
                                        ctx_clone.request_repaint();
                                        prefetch_missing_sticker_thumbnails(
                                            &s.http,
                                            missing,
                                            &gui_tx_clone,
                                            &ctx_clone,
                                        )
                                        .await;
                                    }
                                    Err(e) => {
                                        crate::logger::log_error(
                                            "gui::search_stickers",
                                            &e.to_string(),
                                        );
                                        let _ = gui_tx_clone.send(GuiEvent::StickerSearchResults {
                                            keyword,
                                            items: Vec::new(),
                                        });
                                        ctx_clone.request_repaint();
                                    }
                                }
                            });
                        }
                        WorkerCmd::SuggestComposerStickers { keyword } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let items = crate::chat::search_stickers_by_keyword(&s, &keyword)
                                    .await
                                    .unwrap_or_default();
                                let missing =
                                    preload_disk_sticker_thumbnails(&items, &gui_tx_clone, 16);
                                let _ =
                                    gui_tx_clone.send(GuiEvent::ComposerStickerSuggestions {
                                        keyword,
                                        items,
                                    });
                                ctx_clone.request_repaint();
                                prefetch_missing_sticker_thumbnails(
                                    &s.http,
                                    missing,
                                    &gui_tx_clone,
                                    &ctx_clone,
                                )
                                .await;
                            });
                        }
                        WorkerCmd::ParseLinkPreview { url } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                if let Ok(meta) = crate::chat::parse_link_metadata(&s, &url).await {
                                    let _ = gui_tx_clone.send(GuiEvent::LinkPreviewReady(meta));
                                    ctx_clone.request_repaint();
                                }
                            });
                        }
                        WorkerCmd::TranslateMessage {
                            thread_id,
                            msg_id,
                            text,
                            target_lang,
                            source_lang,
                            generation,
                        } => {
                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {
                                let s = session_clone.lock().await.clone();
                                let target_norm = target_lang.trim();
                                let target_norm = if target_norm.is_empty() { "vi" } else { target_norm };
                                let manual_src = {
                                    let m = source_lang.trim();
                                    if m.is_empty() || m.eq_ignore_ascii_case("auto") {
                                        None
                                    } else {
                                        Some(m.to_ascii_lowercase())
                                    }
                                };
                                // Tách 2 nhánh để tránh Box::leak lifetime: manual dùng
                                // String sở hữu riêng, auto dùng &'static từ heuristic.
                                let send_result = if let Some(m) = manual_src {
                                    if m.eq_ignore_ascii_case(target_norm) {
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                            "Tin nhắn đã đúng ngôn ngữ đích, không cần dịch".into(),
                                        ));
                                        ctx_clone.request_repaint();
                                        return;
                                    }
                                    crate::chat::translate_via_mymemory(&s.http, &text, &m, target_norm).await
                                } else {
                                    let Some(source) = crate::chat::translate_source_for_target(&text, target_norm)
                                    else {
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                            "Tin nhắn đã đúng ngôn ngữ đích (hoặc quá ngắn), không cần dịch".into(),
                                        ));
                                        ctx_clone.request_repaint();
                                        return;
                                    };
                                    if source.eq_ignore_ascii_case(target_norm) {
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                            "Tin nhắn đã đúng ngôn ngữ đích, không cần dịch".into(),
                                        ));
                                        ctx_clone.request_repaint();
                                        return;
                                    }
                                    crate::chat::translate_via_mymemory(&s.http, &text, source, target_norm).await
                                };
                                match send_result {
                                    Ok(translated) => {
                                        let _ = gui_tx_clone.send(GuiEvent::MessageTranslated {
                                            thread_id,
                                            msg_id,
                                            text: translated,
                                            generation,
                                        });
                                    }
                                    Err(e) => {
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "⚠️ Lỗi dịch: {e}"
                                        )));
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::PickAttachments { mode } => {
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::task::spawn_blocking(move || {
                                let Some(paths) = super::attachments::pick_attachments(mode) else {
                                    return;
                                };
                                let batch = super::attachments::prepare_batch(paths);
                                let _ = gui_tx_clone.send(GuiEvent::AttachmentsPrepared(batch));
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::PrepareAttachments { paths } => {
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::task::spawn_blocking(move || {
                                let batch = super::attachments::prepare_batch(paths);
                                let _ = gui_tx_clone.send(GuiEvent::AttachmentsPrepared(batch));
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::SendAttachments {
                            to_id,
                            is_group,
                            paths,
                            caption,
                            original_hd,
                        } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {
                                let paths = super::attachments::album_send_order(paths);
                                let total = paths.len();
                                // Two or more still photos go out as one album; GIFs, videos
                                // and files are sent standalone (same split as Zalo Web).
                                let album_slots = super::attachments::plan_album_slots(&paths);
                                // Only photo sends carry a description; for a queue without
                                // photos the caption goes out as its own text message.
                                let caption_idx = paths
                                    .iter()
                                    .position(|p| super::attachments::is_image_path(p));
                                let album_id = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis())
                                    .unwrap_or(0)
                                    .to_string();
                                // Sequential on purpose: concurrent uploads reorder the
                                // resulting messages in the conversation.
                                for (idx, path) in paths.into_iter().enumerate() {
                                    let fname = path
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("tập tin")
                                        .to_string();
                                    let progress = if total > 1 {
                                        format!(" ({}/{total})", idx + 1)
                                    } else {
                                        String::new()
                                    };
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                        "⏳ Đang tải lên và gửi '{fname}'{progress}…"
                                    )));
                                    ctx_clone.request_repaint();

                                    let item_caption =
                                        if Some(idx) == caption_idx { caption.as_str() } else { "" };
                                    let s = session_clone.lock().await.clone();
                                    let layout = album_slots[idx].map(|(slot, of)| {
                                        crate::chat::MediaGroupLayout::new(album_id.clone(), slot, of)
                                    });
                                    match crate::chat::send_attachment_in_layout(
                                        &s,
                                        &to_id,
                                        is_group,
                                        &path,
                                        item_caption,
                                        original_hd,
                                        layout.as_ref(),
                                    )
                                    .await
                                    {
                                        Ok((_resp, cm)) => {
                                            let _ = gui_tx_clone.send(GuiEvent::AttachmentSent {
                                                thread_id: to_id.clone(),
                                                msg: cm,
                                            });
                                        }
                                        Err(e) => {
                                            let err_s = e.to_string();
                                            crate::logger::log_error("gui::send_attachment", &err_s);
                                            let _ = gui_tx_clone.send(GuiEvent::SendFailed {
                                                thread_id: to_id.clone(),
                                                text: item_caption.to_string(),
                                                error: format!("Lỗi gửi '{fname}': {err_s}"),
                                            });
                                        }
                                    }
                                    ctx_clone.request_repaint();
                                }
                                if caption_idx.is_none() && !caption.trim().is_empty() {
                                    let s = session_clone.lock().await.clone();
                                    let client_id = crate::chat::next_client_message_id();
                                    let res = if is_group {
                                        crate::chat::send_group_message_with_client_id(
                                            &s, &to_id, &caption, client_id,
                                        )
                                        .await
                                    } else {
                                        crate::chat::send_message_with_client_id(
                                            &s, &to_id, &caption, client_id,
                                        )
                                        .await
                                    };
                                    match res {
                                        Ok(resp) => {
                                            let _ = gui_tx_clone.send(GuiEvent::MessageSent {
                                                client_message_id: client_id.to_string(),
                                                thread_id: to_id,
                                                text: caption,
                                                msg_id: extract_msg_id(&resp),
                                            });
                                        }
                                        Err(e) => {
                                            let err_s = e.to_string();
                                            crate::logger::log_error("gui::send_caption", &err_s);
                                            let _ = gui_tx_clone.send(GuiEvent::SendFailed {
                                                thread_id: to_id,
                                                text: caption,
                                                error: err_s,
                                            });
                                        }
                                    }
                                    ctx_clone.request_repaint();
                                }
                            });
                        }
                        WorkerCmd::TriggerScreenshot => {
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::task::spawn_blocking(move || {
                                capture_screen_interactive(gui_tx_clone, ctx_clone);
                            });
                        }
                        WorkerCmd::SendVoice {
                            to_id,
                            is_group,
                            voice_path,
                            voice_dir,
                            mime,
                            duration_secs,
                        } => {
                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            let recording = VoiceRecording { path: voice_path, dir: voice_dir };
                            tasks.spawn(async move {
                                let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                    "⏳ Đang tải lên và gửi tin nhắn thoại…".into(),
                                ));
                                ctx_clone.request_repaint();
                                let s = session_clone.lock().await.clone();
                                match crate::chat::send_voice_message(
                                    &s,
                                    &to_id,
                                    is_group,
                                    &recording.path,
                                    &mime,
                                    duration_secs,
                                )
                                .await
                                {
                                    Ok((_resp, cm)) => {
                                        let _ = gui_tx_clone.send(GuiEvent::AttachmentSent {
                                            thread_id: to_id,
                                            msg: cm,
                                        });
                                    }
                                    Err(e) => {
                                        let err_s = e.to_string();
                                        crate::logger::log_error("gui::send_voice", &err_s);
                                        let _ = gui_tx_clone.send(GuiEvent::SendFailed {
                                            thread_id: to_id,
                                            text: String::new(),
                                            error: format!("Lỗi gửi thoại: {err_s}"),
                                        });
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::StartCall {
                            partner_id,
                            partner_name,
                            partner_avatar,
                            is_video,
                            is_group,
                        } => {
                            let kind_label = if is_video { "Gọi video" } else { "Gọi thoại" };
                            call_bridge.make_call(
                                &partner_id,
                                &partner_name,
                                partner_avatar.as_deref(),
                                is_video,
                                is_group,
                            );
                            let status_msg = format!(
                                "📞 Đang khởi tạo {kind_label} trực tiếp (Pure-Rust) tới {partner_name} ({partner_id})…"
                            );
                            let _ = gui_tx.send(GuiEvent::StatusUpdate(status_msg));
                            egui_ctx.request_repaint();
                        }
                        WorkerCmd::AnswerCall {
                            accept,
                            cam_start_off,
                        } => {
                            call_bridge.answer_call(accept, cam_start_off);
                            egui_ctx.request_repaint();
                        }
                        WorkerCmd::SetCallMediaState {
                            is_on_mic,
                            is_on_cam,
                        } => {
                            call_bridge.set_media_state(is_on_mic, is_on_cam);
                            egui_ctx.request_repaint();
                        }
                        WorkerCmd::InviteGroupCallMembers { user_ids } => {
                            call_bridge.invite_group_members(user_ids);
                            egui_ctx.request_repaint();
                        }
                        WorkerCmd::EndCall => {
                            call_bridge.end_call();
                            let _ = gui_tx.send(GuiEvent::CallStateChanged("free".into()));
                            let _ = gui_tx.send(GuiEvent::StatusUpdate(
                                "Đã kết thúc cuộc gọi".into(),
                            ));
                            egui_ctx.request_repaint();
                        }
                        WorkerCmd::CheckFriendStatus { user_id } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let status_res =
                                    crate::chat::check_friendship_status(&s, &user_id).await;
                                let online_opt =
                                    crate::chat::get_user_online_status(&s, &user_id).await.ok();
                                if let Ok(status) = status_res {
                                    let _ = gui_tx_clone.send(GuiEvent::FriendStatusChecked {
                                        user_id,
                                        status,
                                        online: online_opt,
                                    });
                                    ctx_clone.request_repaint();
                                }
                            });
                        }
                        WorkerCmd::AcceptFriend { user_id, user_name } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                match crate::chat::accept_friend_request(&s, &user_id).await {
                                    Ok(_) => {
                                        let _ = gui_tx_clone.send(GuiEvent::FriendStatusChecked {
                                            user_id,
                                            status: crate::chat::FriendshipStatus::Friend,
                                            online: None,
                                        });
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Đã đồng ý kết bạn với {user_name}!"
                                        )));
                                    }
                                    Err(e) => {
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Không thể đồng ý kết bạn với {user_name}: {e}"
                                        )));
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::RejectFriend { user_id, user_name } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                if crate::chat::reject_friend_request(&s, &user_id)
                                    .await
                                    .is_ok()
                                {
                                    let _ = gui_tx_clone.send(GuiEvent::FriendStatusChecked {
                                        user_id,
                                        status: crate::chat::FriendshipStatus::Stranger,
                                        online: None,
                                    });
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                        "Đã từ chối lời mời kết bạn của {user_name}."
                                    )));
                                    ctx_clone.request_repaint();
                                }
                            });
                        }
                        WorkerCmd::UndoFriend { user_id, user_name } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                if crate::chat::undo_friend_request(&s, &user_id).await.is_ok() {
                                    let _ = gui_tx_clone.send(GuiEvent::FriendStatusChecked {
                                        user_id,
                                        status: crate::chat::FriendshipStatus::Stranger,
                                        online: None,
                                    });
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                        "Đã hủy lời mời kết bạn tới {user_name}."
                                    )));
                                    ctx_clone.request_repaint();
                                }
                            });
                        }
                        WorkerCmd::AddFriend { user_id, user_name } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                // Check if there is an incoming request we can accept directly
                                if crate::chat::accept_friend_request(&s, &user_id).await.is_ok() {
                                    let _ = gui_tx_clone.send(GuiEvent::FriendStatusChecked {
                                        user_id,
                                        status: crate::chat::FriendshipStatus::Friend,
                                        online: None,
                                    });
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                        "Đã đồng ý kết bạn với {user_name}! Bạn có thể gọi lại ngay."
                                    )));
                                    ctx_clone.request_repaint();
                                    return;
                                }
                                match crate::chat::send_friend_request(
                                    &s,
                                    &user_id,
                                    "Xin chào, kết bạn với mình nhé!",
                                )
                                .await
                                {
                                    Ok(_) => {
                                        let _ = gui_tx_clone.send(GuiEvent::FriendStatusChecked {
                                            user_id: user_id.clone(),
                                            status: crate::chat::FriendshipStatus::OutgoingPending,
                                            online: None,
                                        });
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Đã gửi lời mời kết bạn tới {user_name} ({user_id})."
                                        )));
                                    }
                                    Err(e) => {
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Không thể gửi lời mời kết bạn tới {user_name}: {e}"
                                        )));
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::LookupPhone { phone } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                match crate::chat::find_user_by_phone(&s, &phone).await {
                                    Ok(profile) => {
                                        let _ = gui_tx_clone
                                            .send(GuiEvent::PhoneLookupSuccess(profile));
                                    }
                                    Err(e) => {
                                        crate::logger::log_error(
                                            "gui::lookup_phone",
                                            &e.to_string(),
                                        );
                                        let _ = gui_tx_clone
                                            .send(GuiEvent::PhoneLookupFailed(e.to_string()));
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::LoginPassword { phone, pass } => {

                            let session_clone = command_session.clone();
                            let poll_clone = poll_state.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            if let Some(task) = current_password_task.take() {
                                task.abort();
                            }
                            current_password_task = Some(tasks.spawn(async move {
                                let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                    "Đang xác thực tài khoản…".into(),
                                ));
                                ctx_clone.request_repaint();
                                let mut s = session_clone.lock().await; if !gui_tx_clone.is_current() { return; }
                                s.logout();
                                match crate::login::login_by_password(&mut s, &phone, &pass, "VN")
                                    .await
                                {
                                    Ok(_) => {
                                        if let Ok(info) = crate::login::get_login_info(&s).await {
                                            if let Some(cfg) = info.get("data") {
                                                crate::login::apply_login_config(&mut s, cfg);
                                                s.apply_server_info(cfg);
                                                // Labels the account when no profile name is known.
                                                if let Some(ref mut li) = s.login {
                                                    li.phone = crate::session::display_phone(&phone);
                                                }
                                                let _ = s.save_to_file("session.json");

                                                let uin = s
                                                    .login
                                                    .as_ref()
                                                    .map(|l| l.uin.clone())
                                                    .unwrap_or_default();
                                                let _ = crate::account_manager::save_current_account_snapshot(&uin);
                                                let name = s
                                                    .login
                                                    .as_ref()
                                                    .map(|l| l.display_name.clone())
                                                    .unwrap_or_default();
                                                let u_ava = s
                                                    .login
                                                    .as_ref()
                                                    .map(|l| l.avatar.clone())
                                                    .filter(|a| !a.is_empty());
                                                let _ = gui_tx_clone.send(GuiEvent::LoginSuccess {
                                                    name,
                                                    uin: uin.clone(),
                                                    avatar: u_ava,
                                                });
                                                ctx_clone.request_repaint();

                                                let mut list: Vec<Thread> = Vec::new();
                                                if let Ok(friends) =
                                                    crate::chat::get_friends(&s).await
                                                {
                                                    for ft in friends {
                                                        if let Some(ex) =
                                                            list.iter_mut().find(|t| t.id == ft.id)
                                                        {
                                                            crate::chat::adopt_thread_name(
                                                                ex,
                                                                ft.name,
                                                                ft.presentation.contact_name,
                                                            );
                                                            if crate::chat::should_replace_avatar(
                                                                ex.avatar.as_deref(),
                                                                ft.avatar.as_deref(),
                                                            ) {
                                                                ex.avatar = ft.avatar;
                                                            }
                                                            ex.is_group =
                                                                ex.is_group || ft.is_group;
                                                        } else {
                                                            list.push(ft);
                                                        }
                                                    }
                                                }
                                                if let Ok(groups) =
                                                    crate::chat::get_groups(&s).await
                                                {
                                                    for gt in groups {
                                                        if let Some(ex) =
                                                            list.iter_mut().find(|t| t.id == gt.id)
                                                        {
                                                            if !gt.name.is_empty() {
                                                                ex.name = gt.name;
                                                            }
                                                            if crate::chat::should_replace_avatar(
                                                                ex.avatar.as_deref(),
                                                                gt.avatar.as_deref(),
                                                            ) {
                                                                ex.avatar = gt.avatar;
                                                            }
                                                            ex.presentation.posting_restricted = gt.presentation.posting_restricted;
                                                            ex.is_group = true;
                                                        } else {
                                                            list.push(gt);
                                                        }
                                                    }
                                                }
                                                let db_path = crate::store::db_path_for_account(&uin);
                                                crate::store::load_history_into_threads(
                                                    &db_path,
                                                    &mut list,
                                                    crate::store::MessageStore::PRELOAD_MSGS,
                                                );
                                                {
                                                    let mut ps = poll_clone.lock().await;
                                                    *ps = crate::chat::PollState::default();
                                                    let _ = crate::chat::poll_new_messages(
                                                        &s, &mut ps, &mut list,
                                                    )
                                                    .await;
                                                }
                                                if let Ok(mute_resp) =
                                                    crate::chat::get_mute_conversations(&s).await
                                                {
                                                    crate::unread::merge_mute_metadata(
                                                        &mut list, &mute_resp,
                                                    );
                                                    if let Some(mute_map) =
                                                        crate::unread::extract_mute_map(
                                                            &mute_resp,
                                                            crate::login::unix_now() as i64,
                                                        )
                                                    {
                                                        let _ = gui_tx_clone.send(
                                                            GuiEvent::MuteSnapshotSynced(mute_map),
                                                        );
                                                    }
                                                }
                                                let sync_snapshot = list.clone();
                                                let _ = gui_tx_clone
                                                    .send(GuiEvent::ThreadsUpdated(list));
                                                ctx_clone.request_repaint();

                                                queue_automatic_phone_sync(&gui_tx_clone, &uin, sync_snapshot);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let _ = gui_tx_clone
                                            .send(GuiEvent::LoginFailed(e.to_string()));
                                        ctx_clone.request_repaint();
                                    }
                                }
                            }));
                        }
                        WorkerCmd::CheckForUpdates { silent } => {
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let res = fetch_github_latest_release().await;
                                let _ = gui_tx_clone.send(GuiEvent::UpdateCheckFinished {
                                    result: res,
                                    silent,
                                });
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::DownloadAndApplyUpdate {
                            download_url,
                            asset_name,
                            target_version,
                            expected_sha256,
                        } => {
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let _ = gui_tx_clone.send(GuiEvent::UpdateDownloadStatus {
                                    downloading: true,
                                    message: format!(
                                        "Đang tải bản cập nhật v{target_version}…"
                                    ),
                                });
                                ctx_clone.request_repaint();
                                match apply_github_release_update(
                                    &download_url,
                                    &asset_name,
                                    &target_version,
                                    expected_sha256.as_deref(),
                                    &gui_tx_clone,
                                    &ctx_clone,
                                )
                                .await
                                {
                                    Ok(restarting) => {
                                        if restarting {
                                            let _ = gui_tx_clone.send(
                                                GuiEvent::UpdateAppliedAndRestarting(target_version),
                                            );
                                        } else {
                                            let _ = gui_tx_clone.send(
                                                GuiEvent::UpdateDownloadStatus {
                                                    downloading: false,
                                                    message: format!(
                                                        "Đã tải và mở bộ cài đặt {asset_name} (v{target_version})"
                                                    ),
                                                },
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        let _ = gui_tx_clone.send(GuiEvent::UpdateDownloadStatus {
                                            downloading: false,
                                            message: format!("Lỗi cập nhật: {e}"),
                                        });
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::FetchSocialProfile { target_uid } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let res = s
                                    .get_friend_social_profile(&target_uid)
                                    .await
                                    .map_err(|e| e.to_string());
                                let _ = gui_tx_clone.send(GuiEvent::SocialProfileLoaded(res));
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::FetchGroupMembers { group_id } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                if let Ok((info_map, _)) = crate::chat::fetch_group_profiles(&s, &[group_id.clone()]).await {
                                    if let Some(info) = info_map.get(&group_id) {
                                        let mut member_uids: Vec<String> = Vec::new();
                                        if let Some(arr) = info.get("memVerList").and_then(serde_json::Value::as_array) {
                                            for item in arr {
                                                if let Some(s_val) = item.as_str() {
                                                    let uid = s_val.split('_').next().unwrap_or("").trim();
                                                    if !uid.is_empty() && !member_uids.contains(&uid.to_string()) {
                                                        member_uids.push(uid.to_string());
                                                    }
                                                }
                                            }
                                        }
                                        let creator_id = crate::chat::pick_str(info, &["creatorId"]).unwrap_or_default();
                                        if !creator_id.is_empty() && !member_uids.contains(&creator_id) {
                                            member_uids.insert(0, creator_id.clone());
                                        }
                                        let mut admin_ids: Vec<String> = Vec::new();
                                        if let Some(admins) = info.get("adminIds").and_then(serde_json::Value::as_array) {
                                            for a in admins {
                                                if let Some(aid) = a.as_str() {
                                                    let aid_s = aid.to_string();
                                                    if !member_uids.contains(&aid_s) {
                                                        member_uids.push(aid_s.clone());
                                                    }
                                                    if !admin_ids.contains(&aid_s) {
                                                        admin_ids.push(aid_s);
                                                    }
                                                }
                                            }
                                        }
                                        let mut profiles = serde_json::Map::new();
                                        for chunk in member_uids.chunks(50) {
                                            if let Ok(mp) = crate::chat::get_mini_profiles(&s, chunk).await {
                                                for (k, v) in mp {
                                                    profiles.insert(k, v);
                                                }
                                            }
                                        }
                                        let mut members = Vec::new();
                                        for uid in &member_uids {
                                            let mut m_name = "Thành viên".to_string();
                                            let mut m_ava = None;
                                            if let Some(p) = profiles.get(uid) {
                                                if let Some(n) = crate::chat::pick_str(p, &["displayName", "zaloName", "name"]) {
                                                    if !n.is_empty() {
                                                        m_name = n;
                                                    }
                                                }
                                                if let Some(a) = crate::chat::pick_str(p, &["avatar", "avt"]) {
                                                    if !a.is_empty() && !a.ends_with("/default") {
                                                        m_ava = Some(a);
                                                    }
                                                }
                                            }
                                            let role = if uid == &creator_id {
                                                "creator".to_string()
                                            } else if admin_ids.contains(uid) {
                                                "admin".to_string()
                                            } else {
                                                "member".to_string()
                                            };
                                            members.push(crate::chat::SidebarMember {
                                                id: uid.clone(),
                                                name: m_name,
                                                avatar: m_ava,
                                                role,
                                            });
                                        }
                                        let _ = gui_tx_clone.send(GuiEvent::GroupMembersLoaded {
                                            group_id,
                                            members,
                                        });
                                        ctx_clone.request_repaint();
                                        return;
                                    }
                                }
                                let _ = gui_tx_clone.send(GuiEvent::GroupMembersLoaded {
                                    group_id,
                                    members: Vec::new(),
                                });
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::FetchMySocialAvatars { page, count } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let res = s
                                    .get_my_social_avatar_list(page, count)
                                    .await
                                    .map_err(|e| e.to_string());
                                let _ = gui_tx_clone.send(GuiEvent::SocialAvatarsLoaded(res));
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::FetchTimelineFeed {
                            friend_uids,
                            shared_media_by_uid,
                        } => {

                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {

                                let s = session_clone.lock().await.clone();
                                let res = s
                                    .fetch_timeline_feed(&friend_uids, &shared_media_by_uid)
                                    .await
                                    .map_err(|e| e.to_string());
                                let _ = gui_tx_clone.send(GuiEvent::SocialTimelineLoaded(res));
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::HandleMobileSyncControls {
                            events,
                            session,
                            trigger_sync_on_login,
                        } => {
                            tasks.spawn(handle_mobile_sync_controls(
                                events,
                                *session,
                                gui_tx.clone(),
                                egui_ctx.clone(),
                                trigger_sync_on_login,
                            ));
                        }
                        WorkerCmd::SyncPhoneMessages { mut threads, send_phone_prompt } => {
                            use std::sync::atomic::Ordering;
                            if crate::history_sync::PHONE_SYNC_RUNNING.swap(true, Ordering::SeqCst)
                            {
                                let _ = gui_tx.send(GuiEvent::StatusUpdate(
                                    "⚠️ Tiến trình đồng bộ tin nhắn đang chạy ngầm, vui lòng chờ trong giây lát…".into(),
                                ));
                                egui_ctx.request_repaint();
                                continue;
                            }
                            crate::history_sync::PHONE_SYNC_CANCEL.store(false, Ordering::Relaxed);

                            let running_permit = RunningPermit(&crate::history_sync::PHONE_SYNC_RUNNING);
                            let session_clone = command_session.clone();
                            let poll_clone = poll_state.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {
                                let _running_permit = running_permit;

                                let s = session_clone.lock().await.clone();
                                let gui_tx_prog = gui_tx_clone.clone();
                                let ctx_prog = ctx_clone.clone();
                                let res = crate::history_sync::sync_phone_messages_opts(
                                    &s,
                                    &poll_clone,
                                    &mut threads,
                                    send_phone_prompt,
                                    move |progress, status| {
                                        let _ = gui_tx_prog.send(GuiEvent::PhoneSyncProgress {
                                            progress,
                                            status,
                                        });
                                        ctx_prog.request_repaint();
                                    },
                                )
                                .await;
                                let cancelled = crate::history_sync::PHONE_SYNC_CANCEL
                                    .load(Ordering::Relaxed);
                                // Read while PHONE_SYNC_RUNNING is still set: a new run clears
                                // this global as soon as it starts.
                                let phone_error = crate::history_sync::last_phone_sync_error();
                                match res {
                                    Ok((added, active_threads)) => {
                                        let _ = gui_tx_clone.send(GuiEvent::PhoneSyncFinished {
                                            threads,
                                            added,
                                            active_threads,
                                            cancelled,
                                            error: None,
                                            phone_error,
                                        });
                                    }
                                    Err(e) => {
                                        let _ = gui_tx_clone.send(GuiEvent::PhoneSyncFinished {
                                            threads,
                                            added: 0,
                                            active_threads: 0,
                                            cancelled,
                                            error: Some(e.to_string()),
                                            phone_error,
                                        });
                                    }
                                }
                                ctx_clone.request_repaint();

                            });
                        }
                        WorkerCmd::SetProxy { proxy_url } => {
                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {
                                let res = {
                                    let mut s = session_clone.lock().await; if !gui_tx_clone.is_current() { return; }
                                    let r = s
                                        .set_proxy(Some(&proxy_url))
                                        .map_err(|e| e.to_string());
                                    if r.is_ok() {
                                        let _ = s.save_to_file("session.json");
                                    }
                                    r
                                };
                                let _ = gui_tx_clone.send(GuiEvent::ProxyUpdated(res));
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::DownloadFile {
                            url,
                            file_name,
                            download_dir,
                        } => {
                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {
                                let (client, uin) = {
                                    let s = session_clone.lock().await;
                                    (
                                        s.http.clone(),
                                        s.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default(),
                                    )
                                };
                                let res = download_attachment_with_original_name(
                                    &client,
                                    &url,
                                    &file_name,
                                    &download_dir,
                                    &uin,
                                )
                                .await;
                                let _ = gui_tx_clone.send(GuiEvent::FileDownloaded(res));
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::FetchDocPreview {
                            msg_id,
                            url,
                            file_name,
                        } => {
                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {
                                let client = { session_clone.lock().await.clone().http };
                                let res = download_preview_bytes(&client, &url).await;
                                match res {
                                    Ok(bytes) => {
                                        let ext = file_name
                                            .rsplit('.')
                                            .next()
                                            .unwrap_or_default()
                                            .to_ascii_lowercase();
                                        // Extract trên blocking thread (zip/pdf CPU-bound).
                                        let extracted = tokio::task::spawn_blocking(move || {
                                            crate::doc_preview::extract_text(&bytes, &ext)
                                        })
                                        .await
                                        .map_err(|e| format!("Lỗi tác vụ trích text: {e}"))
                                        .and_then(|r| r);
                                        match extracted {
                                            Ok(text) => {
                                                let _ = gui_tx_clone.send(
                                                    GuiEvent::DocPreviewReady { msg_id, text },
                                                );
                                            }
                                            Err(err) => {
                                                let _ = gui_tx_clone.send(
                                                    GuiEvent::DocPreviewFailed { msg_id, err },
                                                );
                                            }
                                        }
                                    }
                                    Err(err) => {
                                        let _ = gui_tx_clone
                                            .send(GuiEvent::DocPreviewFailed { msg_id, err });
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::FetchVideoThumbnail { key, url } => {
                            let session_clone = command_session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tasks.spawn(async move {
                                // No ffmpeg -> no thumbnail, no crash: the bubble
                                // keeps its fallback card below.
                                if !crate::media::is_ffmpeg_available() {
                                    let _ = gui_tx_clone
                                        .send(GuiEvent::StickerImageFailed { key });
                                    ctx_clone.request_repaint();
                                    return;
                                }
                                let bytes = {
                                    let client = { session_clone.lock().await.clone().http };
                                    match client.get(&url).send().await {
                                        Ok(resp) if resp.status().is_success() => {
                                            resp.bytes().await.ok().map(|b| b.to_vec())
                                        }
                                        _ => None,
                                    }
                                };
                                let Some(bytes) = bytes else {
                                    let _ = gui_tx_clone
                                        .send(GuiEvent::StickerImageFailed { key });
                                    ctx_clone.request_repaint();
                                    return;
                                };
                                let frame = tokio::task::spawn_blocking(move || {
                                    extract_first_video_frame(&bytes)
                                })
                                .await
                                .ok()
                                .flatten();
                                let Some(rgba) = frame else {
                                    let _ = gui_tx_clone
                                        .send(GuiEvent::StickerImageFailed { key });
                                    ctx_clone.request_repaint();
                                    return;
                                };
                                let size = [rgba.width() as usize, rgba.height() as usize];
                                let pixels = rgba.as_flat_samples();
                                let color_img = egui::ColorImage::from_rgba_unmultiplied(
                                    size,
                                    pixels.as_slice(),
                                );
                                let _ = gui_tx_clone.send(GuiEvent::StickerImageReady {
                                    key,
                                    image: color_img,
                                });
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::SendTyping {
                            thread_id,
                            is_group,
                        } => {
                            let session_clone = command_session.clone();
                            tasks.spawn(async move {
                                let s = session_clone.lock().await.clone();
                                let _ = crate::chat::send_typing_indicator(&s, &thread_id, is_group).await;
                            });
                        }
                        WorkerCmd::SendSeen {
                            thread_id,
                            is_group,
                            last_msg_id,
                        } => {
                            let session_clone = command_session.clone();
                            tasks.spawn(async move {
                                let s = session_clone.lock().await.clone();
                                let _ = crate::chat::send_seen_v2(&s, &thread_id, is_group, &last_msg_id).await;
                            });
                        }
                    }
                }
            });
        })
        .expect("Failed to spawn zalo-worker thread");
}

const GITHUB_REPO: &str = "ctdoteam1110/ctdozlite";

pub(crate) async fn fetch_github_latest_release() -> Result<UpdateCheckResult, String> {
    let current_ver = env!("CARGO_PKG_VERSION").to_string();
    let client = reqwest::Client::builder()
        .user_agent(format!("ctdozlite/{current_ver}"))
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| format!("Khởi tạo HTTP client thất bại: {e}"))?;

    let latest_url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let mut rel_json: Option<serde_json::Value> = None;

    if let Ok(resp) = client
        .get(&latest_url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                rel_json = Some(v);
            }
        }
    }

    if rel_json.is_none() {
        let list_url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases?per_page=5");
        if let Ok(resp) = client
            .get(&list_url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(arr) = resp.json::<serde_json::Value>().await {
                    if let Some(first) = arr
                        .as_array()
                        .and_then(|a| {
                            a.iter()
                                .find(|r| {
                                    !r.get("draft").and_then(|v| v.as_bool()).unwrap_or(false)
                                })
                                .or_else(|| a.first())
                        })
                        .cloned()
                    {
                        rel_json = Some(first);
                    }
                }
            }
        }
    }

    // Rate-limit resilient fallback: fetch static `latest.json` directly from GitHub Release CDN
    if rel_json.is_none() {
        let cdn_manifest_url =
            format!("https://github.com/{GITHUB_REPO}/releases/latest/download/latest.json");
        if let Ok(resp) = client.get(&cdn_manifest_url).send().await {
            if resp.status().is_success() {
                if let Ok(manifest) = resp.json::<serde_json::Value>().await {
                    rel_json = Some(manifest);
                }
            }
        }
    }

    let Some(rel) = rel_json else {
        return Err("Không truy cập được máy chủ GitHub Releases API".into());
    };

    let mut meta = crate::updater::parse_github_release_value(
        &rel,
        &current_ver,
        GITHUB_REPO,
        std::env::consts::OS,
        std::env::consts::ARCH,
    );

    if meta.expected_sha256.is_none() {
        if let (Some(ref s_url), Some(ref b_name)) = (&meta.sha256sums_url, &meta.asset_name) {
            if let Ok(resp) = client.get(s_url).send().await {
                if let Ok(txt) = resp.text().await {
                    meta.expected_sha256 = crate::updater::parse_sha256sums_for_asset(&txt, b_name);
                }
            }
        }
    }

    Ok(UpdateCheckResult {
        current_version: meta.current_version,
        latest_version: meta.latest_version,
        has_update: meta.has_update,
        release_title: meta.release_title,
        release_notes: meta.release_notes,
        html_url: meta.html_url,
        download_url: meta.download_url,
        asset_name: meta.asset_name,
        asset_size: meta.asset_size,
        expected_sha256: meta.expected_sha256,
        published_at: meta.published_at,
    })
}

/// Stream-download the release asset from GitHub Releases with real-time progress reporting,
/// verify binary integrity (PE/ELF magic + SHA-256 checksum when provided) via [`crate::updater`],
/// and atomically replace the running executable in-place with automatic rollback on failure.
async fn apply_github_release_update(
    download_url: &str,
    asset_name: &str,
    target_version: &str,
    expected_sha256: Option<&str>,
    gui_tx: &GuiTx,
    ctx: &egui::Context,
) -> Result<bool, String> {
    let client = reqwest::Client::builder()
        .user_agent(format!("ctdozlite/{target_version}"))
        .timeout(Duration::from_secs(240))
        .build()
        .map_err(|e| format!("Khởi tạo HTTP client thất bại: {e}"))?;

    let req = crate::updater::DownloadRequest {
        url: download_url,
        asset_name,
        expected_sha256,
        expected_size: None,
        limits: crate::updater::DownloadLimits::default(),
    };

    let bytes = crate::updater::download_release(&client, &req, |downloaded, total| {
        let _ = gui_tx.send(GuiEvent::UpdateDownloadProgress { downloaded, total });
        ctx.request_repaint();
    })
    .await?;

    let name_low = asset_name.to_lowercase();
    let is_portable_bin =
        (cfg!(target_os = "windows") && name_low.ends_with(".exe") && !name_low.contains("setup"))
            || (cfg!(target_os = "linux") && name_low.ends_with(".appimage"));

    if is_portable_bin {
        if let Ok(exe_path) = std::env::current_exe() {
            #[cfg(target_os = "windows")]
            {
                // On Windows a running .exe cannot be overwritten directly, so rename current exe
                // to `.exe.old` and atomically place the new portable binary at `exe_path`.
                let new_path = exe_path.with_extension("exe.new");
                let old_path = exe_path.with_extension("exe.old");
                if std::fs::write(&new_path, &bytes).is_ok() {
                    let _ = std::fs::remove_file(&old_path);
                    if std::fs::rename(&exe_path, &old_path).is_ok() {
                        if std::fs::rename(&new_path, &exe_path).is_ok() {
                            let args: Vec<String> = std::env::args().skip(1).collect();
                            if std::process::Command::new(&exe_path)
                                .args(&args)
                                .spawn()
                                .is_ok()
                            {
                                return Ok(true);
                            }
                            let _ = std::fs::rename(&old_path, &exe_path);
                        } else {
                            let _ = std::fs::rename(&old_path, &exe_path);
                        }
                    }
                    let _ = std::fs::remove_file(&new_path);
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let args: Vec<String> = std::env::args().skip(1).collect();
                if crate::updater::install_release(
                    &bytes,
                    &crate::updater::InstallTarget::Portable(exe_path),
                    |installed_path| {
                        std::process::Command::new(installed_path)
                            .args(&args)
                            .spawn()
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    },
                )
                .is_ok()
                {
                    return Ok(true);
                }
            }
        }
    }

    // Fallback / Installer mode: save to Downloads (or current directory) via transactional installer
    let dl_dir = std::env::var("USERPROFILE")
        .ok()
        .map(|p| std::path::PathBuf::from(p).join("Downloads"))
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join("Downloads"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dl_dir);
    let dest_path = dl_dir.join(asset_name);

    crate::updater::install_release(
        &bytes,
        &crate::updater::InstallTarget::Installer(dest_path),
        |installer_path| {
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new(installer_path)
                    .spawn()
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
            #[cfg(target_os = "macos")]
            {
                std::process::Command::new("open")
                    .arg(installer_path)
                    .spawn()
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            {
                std::process::Command::new("xdg-open")
                    .arg(installer_path)
                    .spawn()
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
        },
    )?;
    Ok(false)
}

/// Download a Zalo file attachment (`share.file` / `chat.file` / `/renewMedia`) and save it
/// inside `download_dir/<account_uin>` using the sender's original filename instead of
/// `renewMedia.doc` (#74).
async fn download_attachment_with_original_name(
    client: &reqwest::Client,
    url: &str,
    file_name: &str,
    download_dir: &str,
    account_uin: &str,
) -> Result<(String, String), String> {
    let clean_name = crate::chat::resolve_attachment_file_name(Some(file_name), file_name, url);
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Không thể tải tập tin: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Máy chủ trả về mã HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Lỗi đọc dữ liệu tập tin: {e}"))?;

    let target_dir = if !download_dir.trim().is_empty() {
        std::path::PathBuf::from(download_dir.trim())
    } else {
        std::env::var("USERPROFILE")
            .ok()
            .map(|p| {
                std::path::PathBuf::from(p)
                    .join("Downloads")
                    .join("Zalo Received Files")
            })
            .or_else(|| {
                std::env::var("HOME").ok().map(|h| {
                    std::path::PathBuf::from(h)
                        .join("Downloads")
                        .join("Zalo Received Files")
                })
            })
            .unwrap_or_else(|| std::path::PathBuf::from("Downloads"))
    };
    // Each account saves into its own subfolder so received files never mix.
    let target_dir = crate::account_manager::account_download_dir(&target_dir, account_uin);

    tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&target_dir)
            .map_err(|e| format!("Không thể tạo thư mục lưu file: {e}"))?;

        let (stem, ext) = match clean_name.rfind('.') {
            Some(dot_idx) if dot_idx > 0 && dot_idx + 1 < clean_name.len() => {
                (&clean_name[..dot_idx], Some(&clean_name[dot_idx + 1..]))
            }
            _ => (clean_name.as_str(), None),
        };

        let mut dest_path = target_dir.join(&clean_name);
        let mut final_name = clean_name.clone();
        let mut suffix_idx = 1_u32;
        while dest_path.exists() {
            final_name = match ext {
                Some(e) => format!("{stem} ({suffix_idx}).{e}"),
                None => format!("{stem} ({suffix_idx})"),
            };
            dest_path = target_dir.join(&final_name);
            suffix_idx += 1;
        }

        std::fs::write(&dest_path, &bytes).map_err(|e| format!("Không thể ghi tập tin: {e}"))?;
        Ok((final_name, dest_path.display().to_string()))
    })
    .await
    .map_err(|e| format!("Lỗi tác vụ lưu file: {e}"))?
}
/// Download attachment bytes for in-app preview, bounded at 10 MiB (no disk write).
async fn download_preview_bytes(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<u8>, String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Không thể tải tập tin: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Máy chủ trả về mã HTTP {}", resp.status()));
    }
    if resp
        .content_length()
        .is_some_and(|len| len > crate::doc_preview::MAX_PREVIEW_BYTES as u64)
    {
        return Err("File quá lớn để xem trước (>10MB), hãy tải về.".to_string());
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Lỗi đọc dữ liệu tập tin: {e}"))?;
    if bytes.len() > crate::doc_preview::MAX_PREVIEW_BYTES {
        return Err("File quá lớn để xem trước (>10MB), hãy tải về.".to_string());
    }
    Ok(bytes.to_vec())
}

/// Extract the first video frame via the system `ffmpeg` into a downscaled RGBA thumbnail.
/// Called on a blocking thread; returns `None` on any failure (no ffmpeg, bad bytes,
/// decode error) so the caller can keep the fallback card.
/// Decode an image in memory using `image::load_from_memory` (PNG/JPEG/GIF/WebP/BMP/ICO/TIFF/QOI/PNM),
/// then extension-hinted `load_from_memory_with_format` (e.g. TGA), falling back to `ffmpeg` pipe
/// transcoding (e.g. AVIF/HEIC/JXL) when available.
fn decode_image_with_fallback(
    bytes: &[u8],
    hint_url_or_path: &str,
) -> Option<(image::DynamicImage, Option<Vec<u8>>)> {
    if let Ok(img) = image::load_from_memory(bytes) {
        return Some((img, None));
    }
    let clean_hint = hint_url_or_path
        .split(['?', '#'])
        .next()
        .unwrap_or(hint_url_or_path);
    if let Some(ext) = std::path::Path::new(clean_hint)
        .extension()
        .and_then(|e| e.to_str())
    {
        if let Some(fmt) = image::ImageFormat::from_extension(ext) {
            if let Ok(img) = image::load_from_memory_with_format(bytes, fmt) {
                return Some((img, None));
            }
        }
    }
    if !crate::media::is_ffmpeg_available() {
        return None;
    }
    let png_bytes = transcode_pipe_to_png_with_ffmpeg(bytes, None)?;
    let img = image::load_from_memory(&png_bytes).ok()?;
    Some((img, Some(png_bytes)))
}

/// Transcode stdin media bytes to PNG via `ffmpeg` without flashing a console window on Windows.
fn transcode_pipe_to_png_with_ffmpeg(bytes: &[u8], vf: Option<&str>) -> Option<Vec<u8>> {
    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args(["-hide_banner", "-loglevel", "error", "-i", "pipe:0", "-vframes", "1"]);
    if let Some(filter) = vf {
        cmd.args(["-vf", filter]);
    }
    cmd.args(["-f", "image2pipe", "-vcodec", "png", "pipe:1"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().ok()?;
    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(bytes);
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() || out.stdout.is_empty() {
        return None;
    }
    Some(out.stdout)
}

fn extract_first_video_frame(bytes: &[u8]) -> Option<image::RgbaImage> {
    // Lấy kích thước frame đã scale trực tiếp từ header PNG của ffmpeg
    // (image2pipe + png): không còn lệch 480x360 vs 320x240 (#4226).
    let png_bytes = transcode_pipe_to_png_with_ffmpeg(bytes, Some("scale=480:-1"))?;
    image::load_from_memory(&png_bytes).ok().map(|d| d.to_rgba8())
}


pub(crate) fn capture_screen_interactive(gui_tx: crate::gui::GuiTx, ctx: egui::Context) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "Start-Process ms-screenclip:"])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .status();
        let _ = gui_tx.send(crate::gui::GuiEvent::StatusUpdate(
            "Đã mở công cụ chụp ảnh màn hình — sau khi chụp hãy nhấn Ctrl+V để dán".into(),
        ));
        ctx.request_repaint();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let now = crate::login::unix_now();
        let temp_path = std::env::temp_dir().join(format!("ctdozlite_screenshot_{now}.png"));

        #[cfg(target_os = "macos")]
        let status = std::process::Command::new("/usr/sbin/screencapture")
            .args(["-i", "-x", temp_path.to_str().unwrap_or("")])
            .status();

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let status = {
            let temp_str = temp_path.to_str().unwrap_or("");
            std::process::Command::new("flameshot")
                .args(["gui", "-r", "-p", temp_str])
                .status()
                .or_else(|_| {
                    std::process::Command::new("gnome-screenshot")
                        .args(["-a", "-f", temp_str])
                        .status()
                })
                .or_else(|_| {
                    std::process::Command::new("spectacle")
                        .args(["-b", "-r", "-o", temp_str])
                        .status()
                })
                .or_else(|_| {
                    std::process::Command::new("scrot")
                        .args(["-s", temp_str])
                        .status()
                })
        };

        if let Ok(st) = status {
            if st.success() && temp_path.exists() {
                if let Ok(meta) = std::fs::metadata(&temp_path) {
                    if meta.len() > 0 {
                        if let Ok(bytes) = std::fs::read(&temp_path) {
                            if let Ok(dyn_img) = image::load_from_memory(&bytes) {
                                if let Some((final_path, w, h, col_img)) =
                                    crate::gui::messages::finalize_clipboard_rgba_image(
                                        dyn_img.to_rgba8(),
                                    )
                                {
                                    let _ = std::fs::remove_file(&temp_path);
                                    let _ = gui_tx.send(crate::gui::GuiEvent::ScreenshotCaptured {
                                        path: final_path,
                                        width: w,
                                        height: h,
                                        image: col_img,
                                    });
                                    ctx.request_repaint();
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
        let _ = std::fs::remove_file(&temp_path);
        let _ = gui_tx.send(crate::gui::GuiEvent::StatusUpdate(
            "Đã huỷ chụp màn hình".into(),
        ));
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod account_task_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Check that dropped background work preserves the transition status while a
    /// dropped user action receives feedback.
    ///
    /// # Panics
    /// Panics if a background command gets a notice or a user action loses its notice.
    #[test]
    fn stale_background_work_is_quiet_but_user_mutations_get_a_notice() {
        let automatic = [
            WorkerCmd::PollMessages {
                known_threads: Vec::new(),
            },
            WorkerCmd::RefreshThreads,
            WorkerCmd::PersistThreads {
                uin: "account-a".into(),
                threads: Vec::new(),
            },
            WorkerCmd::LoadScheduledMessages {
                uin: "account-a".into(),
                notify_sent: false,
            },
        ];
        for command in automatic {
            assert!(command.stale_notice().is_none());
        }
        assert!(WorkerCmd::SendMessage {
            to_id: "peer-1".into(),
            text: "Test".into(),
            is_group: false
        }
        .stale_notice()
        .is_some());
        assert!(WorkerCmd::RefreshThreadsManual.stale_notice().is_some());
    }

    #[test]
    fn rejected_queued_voice_removes_recording_and_notifies_the_gui() {
        let recording_dir = std::env::temp_dir().join(format!(
            "ctdozlite-voice-{}",
            crate::param_cipher::random_hex(8)
        ));
        std::fs::create_dir(&recording_dir).unwrap();
        let recording_path = recording_dir.join("voice.mp3");
        std::fs::write(&recording_path, b"sample recording").unwrap();
        let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
        let (events, completions) = std::sync::mpsc::channel();
        let root = GuiTx::new(events, egui::Context::default());
        let tx = WorkerTx::new(sender, "account-a".into(), &root);
        tx.send(WorkerCmd::SendVoice {
            to_id: "peer-1".into(),
            is_group: false,
            voice_path: recording_path.clone(),
            voice_dir: Some(recording_dir.clone()),
            mime: "audio/mpeg".into(),
            duration_secs: 1,
        })
        .unwrap();
        root.advance_account();
        let queued = commands.try_recv().unwrap();
        assert!(!queued.origin_is_current());
        queued.reject(&root);
        assert!(!recording_path.exists());
        assert!(!recording_dir.exists());
        assert!(matches!(
            completions.try_recv().unwrap().for_current_account(),
            Some(GuiEvent::StatusUpdate(_))
        ));
    }

    #[tokio::test]
    async fn invalid_add_account_preparation_reports_failure_without_advancing_scope() {
        let (events, completions) = std::sync::mpsc::channel();
        let root = GuiTx::new(events, egui::Context::default());
        let current = root.scoped();
        assert!(!prepare_add_account("../invalid-account", Vec::new(), &current).await);
        assert!(current.is_current());
        assert!(matches!(
            completions.try_recv().unwrap().for_current_account(),
            Some(GuiEvent::AddAccountFailed(_))
        ));
        assert!(completions.try_recv().is_err());
    }

    #[tokio::test]
    async fn failed_switch_restores_credentials_and_emits_a_scope_rebind() {
        let mut previous = Session::new(false, false, false).unwrap();
        previous.login = Some(crate::session::LoginInfo {
            uin: "account-a".into(),
            ..Default::default()
        });
        previous.zpw_sek = Some("fake-original-credential".into());
        let live = std::sync::Arc::new(tokio::sync::Mutex::new(previous.clone()));
        let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
        let (events, completions) = std::sync::mpsc::channel();
        let root = GuiTx::new(events, egui::Context::default());
        let tx = WorkerTx::new(sender, "account-a".into(), &root);
        tx.send(WorkerCmd::RefreshThreads).unwrap();
        let current = root.advance_account();
        live.lock().await.logout();
        restore_failed_switch(&live, previous, &current, "Test missing profile").await;
        let restored = live.lock().await.clone();
        assert_eq!(
            restored.zpw_sek.as_deref(),
            Some("fake-original-credential")
        );
        assert!(matches!(
            completions.try_recv().unwrap().for_current_account(),
            Some(GuiEvent::AccountSwitchFailed(_))
        ));
        // This is the same rebind performed when the GUI consumes the terminal event.
        assert_eq!(restored.login.unwrap().uin, "account-a");
        tx.rebind_visible_scope();
        assert!(!commands.try_recv().unwrap().origin_is_current());
        tx.send(WorkerCmd::RefreshThreads).unwrap();
        assert!(commands.try_recv().unwrap().origin_is_current());
    }

    /// Block a write after its async waiter is cancelled to check that the wipe still
    /// waits for the blocking closure to finish.
    ///
    /// # Panics
    /// Panics if the write cannot start or the drain completes before the write exits.
    #[tokio::test]
    async fn wipe_waits_for_non_abortable_history_writes() {
        let mut tasks = tokio::task::JoinSet::new();
        let mut writes = tokio::task::JoinSet::new();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let finished = std::sync::Arc::new(AtomicBool::new(false));
        let did_finish = finished.clone();
        let result = spawn_history_write(&mut writes, move || -> anyhow::Result<()> {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            did_finish.store(true, Ordering::SeqCst);
            Ok(())
        });
        tasks.spawn(async move {
            let _ = result.await;
        });
        started_rx.await.unwrap();
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        let drain = stop_history_tasks(&mut tasks, &mut writes);
        tokio::pin!(drain);
        assert!(std::future::Future::poll(
            drain.as_mut(),
            &mut std::task::Context::from_waker(std::task::Waker::noop()),
        )
        .is_pending());
        release_tx.send(()).unwrap();
        drain.await;
        assert!(finished.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn wipe_waits_for_a_sync_write_submitted_by_a_running_account_task() {
        let mut tasks = tokio::task::JoinSet::new();
        let mut writes = tokio::task::JoinSet::new();
        let writer = ScopedHistoryWriter::new(&mut writes);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (finished_tx, mut finished_rx) = tokio::sync::oneshot::channel();
        tasks.spawn(async move {
            writer.write(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                finished_tx.send(()).unwrap();
                Ok(())
            }).await.unwrap();
        });
        started_rx.await.unwrap();
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        let drain = stop_history_tasks(&mut tasks, &mut writes);
        tokio::pin!(drain);
        assert!(std::future::Future::poll(
            drain.as_mut(),
            &mut std::task::Context::from_waker(std::task::Waker::noop()),
        )
        .is_pending());
        release_tx.send(()).unwrap();
        drain.await;
        assert_eq!(finished_rx.try_recv(), Ok(()));
    }

    #[test]
    fn work_reset_keeps_queued_old_snapshots_rejected_until_ui_rebinds() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let (events, _) = std::sync::mpsc::channel();
        let root = GuiTx::new(events, egui::Context::default());
        let tx = WorkerTx::new(sender, "account-a".into(), &root);
        tx.send(WorkerCmd::PersistThreads {
            uin: "account-a".into(),
            threads: Vec::new(),
        })
        .unwrap();
        root.advance_work();
        assert!(!receiver.try_recv().unwrap().origin_is_current());
        tx.send(WorkerCmd::RefreshThreads).unwrap();
        assert!(!receiver.try_recv().unwrap().origin_is_current());
        tx.set_visible_account("account-a");
        tx.send(WorkerCmd::RefreshThreads).unwrap();
        assert!(receiver.try_recv().unwrap().origin_is_current());
    }

    #[tokio::test]
    async fn replacement_login_invalidates_expired_account_completions() {
        let session = Session::new(false, false, false).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let root = GuiTx::new(tx, egui::Context::default());
        let departing = root.scoped();
        let bridge = crate::call::CallBridge::spawn(
            std::sync::Arc::new(tokio::sync::Mutex::new(session.clone())),
            root.clone(),
            egui::Context::default(),
        )
        .for_account(session, departing.clone());
        let current = authentication_events(&WorkerCmd::RequestQr, departing.clone(), &bridge);
        assert!(!departing.is_current());
        assert!(current.is_current());
        assert!(matches!(
            rx.try_recv().unwrap().for_current_account(),
            Some(GuiEvent::AuthenticationRestarted)
        ));
    }

    #[test]
    fn explicit_logout_keeps_the_owner_after_runtime_session_expiration() {
        let session = Session::new(false, false, false).unwrap();
        let logout = QueuedWorkerCommand {
            command: WorkerCmd::Logout,
            origin_uin: Some("account-a".into()),
            origin_events: None,
        };
        assert!(logout.accepts_account(""));
        assert!(!logout.accepts_account("account-b"));
        assert_eq!(departing_account_uin(&session, "account-a"), "account-a");
    }

    #[tokio::test]
    async fn locked_login_session_does_not_block_app_wide_commands() {
        let live = std::sync::Arc::new(tokio::sync::Mutex::new(
            Session::new(false, false, false).unwrap(),
        ));
        let held = live.lock().await;
        assert!(try_command_snapshot(&WorkerCmd::RefreshThreads, &live).is_none());
        assert!(matches!(
            try_command_snapshot(&WorkerCmd::CheckForUpdates { silent: true }, &live),
            Some(None)
        ));
        drop(held);
        assert!(matches!(
            try_command_snapshot(&WorkerCmd::RefreshThreads, &live),
            Some(Some(_))
        ));
    }

    #[tokio::test]
    async fn priority_commands_bypass_deferred_actions_without_reordering_them() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut deferred = std::collections::VecDeque::from([QueuedWorkerCommand {
            command: WorkerCmd::RefreshThreads,
            origin_uin: None,
            origin_events: None,
        }]);
        sender
            .send(QueuedWorkerCommand {
                command: WorkerCmd::RefreshThreadsManual,
                origin_uin: None,
                origin_events: None,
            })
            .unwrap();
        sender
            .send(QueuedWorkerCommand {
                command: WorkerCmd::CheckForUpdates { silent: true },
                origin_uin: None,
                origin_events: None,
            })
            .unwrap();
        let priority = tokio::time::timeout(
            Duration::from_millis(500),
            next_worker_command(&mut receiver, &mut deferred),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(matches!(
            priority.command,
            WorkerCmd::CheckForUpdates { .. }
        ));
        sender
            .send(QueuedWorkerCommand {
                command: WorkerCmd::Logout,
                origin_uin: None,
                origin_events: None,
            })
            .unwrap();
        assert!(matches!(
            next_worker_command(&mut receiver, &mut deferred)
                .await
                .unwrap()
                .command,
            WorkerCmd::Logout
        ));
        assert!(matches!(
            next_worker_command(&mut receiver, &mut deferred)
                .await
                .unwrap()
                .command,
            WorkerCmd::RefreshThreads
        ));
        assert!(matches!(
            next_worker_command(&mut receiver, &mut deferred)
                .await
                .unwrap()
                .command,
            WorkerCmd::RefreshThreadsManual
        ));
    }

    #[test]
    fn queued_actions_keep_the_visible_account_until_dispatch() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let (events, _events_rx) = std::sync::mpsc::channel();
        let root = GuiTx::new(events, egui::Context::default());
        let tx = WorkerTx::new(sender, "account-a".into(), &root);
        tx.send(WorkerCmd::SendMessage {
            to_id: "thread-a".into(),
            text: "Test".into(),
            is_group: false,
        })
        .unwrap();
        tx.set_visible_account("account-b");
        let old = receiver.try_recv().unwrap();
        assert_eq!(old.origin_uin.as_deref(), Some("account-a"));
        assert!(!old.accepts_account("account-b"));
        tx.send(WorkerCmd::SendMessage {
            to_id: "thread-b".into(),
            text: "Test".into(),
            is_group: false,
        })
        .unwrap();
        assert!(receiver.try_recv().unwrap().accepts_account("account-b"));
        tx.send(WorkerCmd::CheckForUpdates { silent: true })
            .unwrap();
        assert!(receiver.try_recv().unwrap().accepts_account("account-c"));
        tx.send_for_account(WorkerCmd::RefreshThreads, "account-a", &root.scoped())
            .unwrap();
        assert!(!receiver.try_recv().unwrap().accepts_account("account-b"));
        root.advance_account();
        // Until AccountSwitched is rendered, even a matching UIN retains the old UI epoch.
        tx.send(WorkerCmd::RefreshThreads).unwrap();
        let old_view = receiver.try_recv().unwrap();
        assert!(old_view.accepts_account("account-b"));
        assert!(!old_view.origin_is_current());
        tx.set_visible_account("account-b");
        tx.send(WorkerCmd::RefreshThreads).unwrap();
        assert!(receiver.try_recv().unwrap().origin_is_current());
    }

    /// Check that dispatched chat mutations retain the old account's credentials
    /// after a switch and that the next QR login uses the live session.
    ///
    /// # Panics
    /// Panics if the fixture session or task fails, or a command uses the wrong session.
    #[tokio::test]
    async fn dispatched_actions_keep_their_session_after_an_account_switch() {
        let mut snapshot = Session::new(false, false, false).unwrap();
        snapshot.zpw_sek = Some("fake-account-a".into());
        let live = std::sync::Arc::new(tokio::sync::Mutex::new(snapshot.clone()));
        let commands = [
            WorkerCmd::SendMessage {
                to_id: "thread-a".into(),
                text: "Test".into(),
                is_group: false,
            },
            WorkerCmd::SendSticker {
                to_id: "thread-a".into(),
                sticker_id: 1,
                cate_id: 1,
                sticker_type: 1,
                is_group: false,
            },
            WorkerCmd::PinMessage {
                thread_id: "thread-a".into(),
                is_group: false,
                msg: Default::default(),
                pin: true,
                topic_id: String::new(),
            },
        ];
        let captured: Vec<_> = commands
            .iter()
            .map(|command| session_for_command(command, Some(&snapshot), &live))
            .collect();
        live.lock().await.zpw_sek = Some("fake-account-b".into());
        for session in captured {
            let credentials = tokio::spawn(async move { session.lock().await.zpw_sek.clone() })
                .await
                .unwrap();
            assert_eq!(credentials.as_deref(), Some("fake-account-a"));
        }
        assert!(std::sync::Arc::ptr_eq(
            &live,
            &session_for_command(&WorkerCmd::RequestQr, Some(&snapshot), &live)
        ));
    }

    #[test]
    fn departing_tasks_cannot_queue_a_new_login_after_switching_accounts() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let (events, _rx) = std::sync::mpsc::channel();
        let root = GuiTx::new(events, egui::Context::default());
        let tx = WorkerTx::new(sender, "account-b".into(), &root);
        let old = root.scoped();
        root.advance_account();
        tx.send_if_current(WorkerCmd::RequestQr, &old).unwrap();
        assert!(receiver.try_recv().is_err());
    }

    /// Exercise cancellation both before and after the task starts so the running
    /// guard is released even if the task never reaches its first poll.
    ///
    /// # Panics
    /// Panics if the task cannot signal startup or cancellation leaves the guard set.
    #[tokio::test]
    async fn running_permit_resets_after_task_cancellation() {
        static RUNNING: AtomicBool = AtomicBool::new(false);
        for wait_for_start in [false, true] {
            RUNNING.store(true, Ordering::SeqCst);
            let permit = RunningPermit(&RUNNING);
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let mut tasks = tokio::task::JoinSet::new();
            tasks.spawn(async move {
                let _permit = permit;
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            });
            if wait_for_start {
                started_rx.await.unwrap();
            }
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            assert!(!RUNNING.load(Ordering::SeqCst));
        }
    }
}
