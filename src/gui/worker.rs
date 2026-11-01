use super::*;

pub enum WorkerCmd {
    RequestQr,
    LoginPassword {
        phone: String,
        pass: String,
    },
    RefreshThreads,
    PollMessages {
        known_threads: Vec<(String, String, bool)>,
    },
    LoadGroupHistory {
        group_id: String,
        is_group: bool,
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
    SendSticker {
        to_id: String,
        sticker_id: u64,
        cate_id: u64,
        sticker_type: u32,
        is_group: bool,
    },
    PickAndSendAttachment {
        to_id: String,
        is_group: bool,
        images_only: bool,
        caption: String,
        preset_path: Option<std::path::PathBuf>,
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
    LookupPhone {
        phone: String,
    },
    /// Save an attachment (file / video / original photo) into `dest_dir` ("Thư mục lưu file").
    /// `url` and `suggested_name` come from a chat message and are untrusted; all policy lives in
    /// [`crate::download`]. Progress is reported as `GuiEvent::Download*` keyed by `url`.
    DownloadAttachment {
        url: String,
        suggested_name: String,
        thread_id: String,
        dest_dir: String,
    },
    /// Fetch a chat video into `paths::video_cache_dir()` for in-app playback (reusing a cached
    /// copy) → `GuiEvent::Video{Progress,Ready,Failed}` keyed by `url`. `url` is untrusted; the
    /// URL policy is [`crate::download::download_to_dir`]'s.
    PrepareVideo {
        url: String,
    },
    /// "Đóng" on the missing-messages banner → `MessageStore::dismiss_missing_ranges`.
    DismissMissingRanges,
    /// Measure `paths::cache_dir()` → `GuiEvent::CacheSizeComputed`.
    ComputeCacheSize,
    /// Delete the contents of `paths::cache_dir()` (`stickers_cache/` only) →
    /// `GuiEvent::ImageCacheCleared`.
    ClearImageCache,
    StartCall {
        partner_id: String,
        partner_name: String,
        partner_avatar: Option<String>,
        is_video: bool,
        is_group: bool,
    },
    AnswerCall {
        accept: bool,
    },
    SetCallMediaState {
        is_on_mic: bool,
        is_on_cam: bool,
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
    PersistThreads(Vec<Thread>),
    Logout,
}

pub(super) static WORKER_TX: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<WorkerCmd>> =
    std::sync::OnceLock::new();

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

/// `true` only for an explicit Zalo auth error (`server error 600` / `-102`, see
/// [`crate::session::AUTH_ERROR_CODES`]). Network failures and other HTTP errors never count as
/// an expired session (upstream issue #21).
pub(super) fn is_session_expired_error(err_str: &str) -> bool {
    crate::session::AUTH_ERROR_CODES
        .iter()
        .any(|code| err_str.contains(&format!("server error {code}:")))
}

/// Largest image accepted from the network or disk for stickers / thumbnails / avatars.
pub(super) const MAX_IMAGE_BYTES: usize = 15 * 1024 * 1024;

/// Zalo's own static/CDN hosts (`*.zalo.me`, `*.zaloapp.com`, `*.zdn.vn`, `*.zadn.vn`).
fn is_zalo_cdn_host(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    crate::session::is_zalo_host(&h)
        || h == "zdn.vn"
        || h.ends_with(".zdn.vn")
        || h == "zadn.vn"
        || h.ends_with(".zadn.vn")
}

/// May this URL be fetched automatically to render an image?
///
/// `https://` always; plain `http://` only for Zalo's own CDN hosts (some legacy stickers are
/// still served that way). Anything else (`file:`, `ftp:`, other `http://` hosts) is refused.
pub(super) fn image_url_allowed(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url.trim()) else {
        return false;
    };
    match u.scheme() {
        "https" => u.host_str().is_some(),
        "http" => u.host_str().map(is_zalo_cdn_host).unwrap_or(false),
        _ => false,
    }
}

/// Resolve the path of a locally sent image (`local://<absolute path>` or an absolute path).
///
/// UNC / network paths (`\\server\share\x.png`, `//server/x`) are refused: on Windows merely
/// opening one makes the OS connect to that host and offer the user's NTLM credentials.
pub(super) fn local_image_path(url: &str) -> Option<std::path::PathBuf> {
    let raw = url.strip_prefix("local://").unwrap_or(url).trim();
    if raw.is_empty() || raw.starts_with("\\\\") || raw.starts_with("//") {
        return None;
    }
    let p = std::path::Path::new(raw);
    if !p.is_absolute() {
        return None;
    }
    let meta = std::fs::metadata(p).ok()?;
    if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES as u64 {
        return None;
    }
    Some(p.to_path_buf())
}

/// Download an image (stickers, thumbnails, avatars) with the scheme/host policy of
/// [`image_url_allowed`] and a [`MAX_IMAGE_BYTES`] cap.
pub(super) async fn fetch_image_bytes(client: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    if !image_url_allowed(url) {
        return None;
    }
    let mut resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    if resp.content_length().is_some_and(|n| n > MAX_IMAGE_BYTES as u64) {
        return None;
    }
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        if buf.len() + chunk.len() > MAX_IMAGE_BYTES {
            return None;
        }
        buf.extend_from_slice(&chunk);
    }
    Some(buf)
}

/// Status line shown when Zalo cannot be reached; the saved session is kept.
pub(super) fn unreachable_status(why: &str) -> String {
    let why = crate::session::truncate(why, 140);
    format!("Không kết nối được máy chủ Zalo — vẫn giữ phiên đăng nhập. ({why})")
}

/// The server rejected the session: forget it **in memory** and ask for a new QR login.
///
/// `session.json` stays on disk and is only overwritten after a new login succeeds, so a
/// misclassified error can never destroy working credentials (upstream issue #21).
pub(super) async fn on_session_expired(
    session: &std::sync::Arc<tokio::sync::Mutex<Session>>,
    gui_tx: &Sender<GuiEvent>,
    ctx: &egui::Context,
    reason: &str,
) {
    {
        let mut sl = session.lock().await;
        sl.logout();
    }
    let _ = gui_tx.send(GuiEvent::SessionExpired(reason.to_string()));
    ctx.request_repaint();
    if let Some(tx) = WORKER_TX.get() {
        let _ = tx.send(WorkerCmd::RequestQr);
    }
}

/// Immediately decode any disk-cached sticker thumbnails (`stickers_cache/stk_{cid}_{sid}.png`)
/// and push `GuiEvent::StickerImageReady` *before* sending sticker list metadata, returning any
/// missing `(key, url, cache_path)` entries for concurrent network prefetching.
pub(super) fn preload_disk_sticker_thumbnails(
    items: &[crate::chat::StickerItem],
    gui_tx: &Sender<GuiEvent>,
    limit: usize,
) -> Vec<(String, String, std::path::PathBuf)> {
    let cache_dir_buf = crate::paths::cache_dir();
    let cache_dir = cache_dir_buf.as_path();

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
    gui_tx: &Sender<GuiEvent>,
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
            if let Some(vec) = fetch_image_bytes(&client, &url).await {
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
    });
    futures_util::future::join_all(futs).await;
}

/// Offline catch-up progress shared by the socket loop and `RefreshThreads`: ids of messages
/// seen while a catch-up runs, deduplicated across socket and long-poll.
#[derive(Default)]
struct CatchUp {
    active: bool,
    ids: HashSet<String>,
}

type SharedCatchUp = std::sync::Arc<std::sync::Mutex<CatchUp>>;

fn catchup_key(thread_id: &str, m: &ChatMessage) -> String {
    if m.id.is_empty() {
        format!("{thread_id}@{}", m.ts)
    } else {
        format!("{thread_id}:{}", m.id)
    }
}

/// Count the messages of `threads` into a running catch-up and report progress.
fn catchup_note(c: &SharedCatchUp, threads: &[Thread], gui_tx: &Sender<GuiEvent>, ctx: &egui::Context) {
    let Ok(mut cu) = c.lock() else {
        return;
    };
    if !cu.active {
        return;
    }
    let before = cu.ids.len();
    for t in threads {
        for m in &t.messages {
            cu.ids.insert(catchup_key(&t.id, m));
        }
    }
    if cu.ids.len() != before {
        let _ = gui_tx.send(GuiEvent::SyncProgress { fetched: cu.ids.len(), done: false });
        ctx.request_repaint();
    }
}

/// Push the open missing-message ranges to the GUI when a new gap was recorded.
fn send_new_gaps(gui_tx: &Sender<GuiEvent>, ctx: &egui::Context) {
    if !crate::sync_state::take_new_gaps() {
        return;
    }
    if let Ok(store) = crate::store::MessageStore::open(crate::paths::history_db()) {
        let ranges = store.open_missing_ranges().unwrap_or_default();
        let _ = gui_tx.send(GuiEvent::MissingRanges(ranges));
        ctx.request_repaint();
    }
}

/// Recompute conversation order (`get-last-msgs` activity + msgId clock) for the GUI.
fn send_conversation_activity(gui_tx: &Sender<GuiEvent>, ctx: &egui::Context) {
    if let Ok(store) = crate::store::MessageStore::open(crate::paths::history_db()) {
        let map = crate::sync_state::load_conversation_activity(&store);
        let _ = gui_tx.send(GuiEvent::ConversationActivity(map));
        ctx.request_repaint();
    }
}

/// Consecutive `RefreshThreads` attempts that found the server unreachable.
static REFRESH_RETRIES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Ask for another `RefreshThreads` later (5 s, 15 s, 30 s, then every 60 s) so a start-up
/// without network still catches up once the network is back.
fn schedule_refresh_retry() {
    let n = REFRESH_RETRIES.fetch_add(1, std::sync::atomic::Ordering::SeqCst) as usize;
    let delay = [5u64, 15, 30, 60][n.min(3)];
    crate::trace_shape::line("sync", format!("refresh: retry in {delay}s (attempt {})", n + 1));
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(delay)).await;
        if let Some(tx) = WORKER_TX.get() {
            let _ = tx.send(WorkerCmd::RefreshThreads);
        }
    });
}

pub(super) fn spawn_session_worker(session: Session, gui_tx: Sender<GuiEvent>, egui_ctx: egui::Context) {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<WorkerCmd>();
    let _ = WORKER_TX.set(cmd_tx);

    // Saved cursors of this account (A1): the long-poll state and the socket queue cursors.
    let saved = crate::sync_state::SyncState::load(
        &session.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default(),
    );
    let ws_cursors: crate::socket::WsCursors =
        std::sync::Arc::new(std::sync::Mutex::new(saved.ws_queues.clone()));
    let catchup: SharedCatchUp = std::sync::Arc::new(std::sync::Mutex::new(CatchUp {
        active: session.login.is_some(),
        ids: HashSet::new(),
    }));

    let session = std::sync::Arc::new(tokio::sync::Mutex::new(session));
    let poll_state = std::sync::Arc::new(tokio::sync::Mutex::new(saved.poll));
    let ctrl_poll_state =
        std::sync::Arc::new(tokio::sync::Mutex::new(crate::call::CtrlPollState::default()));

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
                    let _ = crate::login::get_server_info(&s).await;
                }
                {
                    let s_kw = session.clone();
                    let gui_tx_kw = gui_tx.clone();
                    let ctx_kw = egui_ctx.clone();
                    tokio::spawn(async move {
                        let s = s_kw.lock().await.clone();
                        if s.login.is_some() {
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

                // 0) Persist the sync cursors (A1): every 2 s, only when they changed.
                {
                    let s_sv = session.clone();
                    let ps_sv = poll_state.clone();
                    let wc_sv = ws_cursors.clone();
                    tokio::spawn(async move {
                        let mut last_saved: Option<crate::sync_state::SyncState> = None;
                        loop {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            let uin = {
                                let s = s_sv.lock().await;
                                s.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default()
                            };
                            if crate::sync_state::sanitize_uin(&uin).is_none() {
                                continue;
                            }
                            // A long-poll holds this lock for up to 12 s; try again next tick.
                            let Ok(poll) = ps_sv.try_lock().map(|p| p.clone()) else {
                                continue;
                            };
                            let mut st = crate::sync_state::SyncState::new(&uin);
                            st.poll = poll;
                            st.ws_queues = wc_sv.lock().map(|c| c.clone()).unwrap_or_default();
                            if last_saved.as_ref() == Some(&st) {
                                continue;
                            }
                            let mut to_save = st.clone();
                            to_save.updated_at_ms = crate::timefmt::now_ms();
                            match to_save.save() {
                                Ok(()) => last_saved = Some(st),
                                Err(e) => crate::logger::log_error("worker::sync_state", &e.to_string()),
                            }
                        }
                    });
                }

                // 1) Real-time `zpw_ws` WebSocket loop for instant chat messages + VoIP events
                {
                    let s_ws = session.clone();
                    let cb = call_bridge.clone();
                    let gui_tx_ws = gui_tx.clone();
                    let ctx_ws = egui_ctx.clone();
                    let wc_ws = ws_cursors.clone();
                    let cu_ws = catchup.clone();
                    tokio::spawn(async move {
                        let mut ws_rx: Option<
                            tokio::sync::mpsc::UnboundedReceiver<crate::socket::WsEvent>,
                        > = None;
                        let mut ws_sek = String::new();
                        loop {
                            let s_snap = s_ws.lock().await.clone();
                            if s_snap.login.is_some() {
                                let cur_sek = s_snap.zpw_sek.clone().unwrap_or_default();
                                if !cur_sek.is_empty() && (ws_rx.is_none() || ws_sek != cur_sek) {
                                    ws_sek = cur_sek;
                                    ws_rx = Some(crate::socket::spawn_with_cursors(
                                        s_snap.clone(),
                                        wc_ws.clone(),
                                    ));
                                }
                                if let Some(rx) = ws_rx.as_mut() {
                                    match tokio::time::timeout(
                                        Duration::from_millis(1500),
                                        rx.recv(),
                                    )
                                    .await
                                    {
                                        Ok(Some(ev)) => {
                                            let data_node = ev
                                                .payload
                                                .get("data")
                                                .unwrap_or(&ev.payload);
                                            for voip_ev in
                                                crate::call::extract_voip_controls(data_node)
                                            {
                                                cb.handle_control_event(voip_ev);
                                            }
                                            if ev.is_chat_ish()
                                                && crate::sync_state::record_gaps_from(&ev.payload)
                                            {
                                                send_new_gaps(&gui_tx_ws, &ctx_ws);
                                            }
                                            if ev.is_chat_ish() {
                                                let mut ws_threads = Vec::new();
                                                if let Some(li) = s_snap.login.as_ref() {
                                                    if !li.send2me_id.is_empty() {
                                                        ws_threads.push(Thread {
                                                            id: li.send2me_id.clone(),
                                                            name: "Cloud của tôi (Lưu trữ)"
                                                                .into(),
                                                            preview: String::new(),
                                                            time: String::new(),
                                                            avatar: None,
                                                            is_group: false,
                                                            messages: Vec::new(),
                                                        });
                                                    }
                                                }
                                                crate::chat::merge_previews(
                                                    &mut ws_threads,
                                                    &ev.payload,
                                                );
                                                ws_threads.retain(|t| !t.messages.is_empty());
                                                catchup_note(&cu_ws, &ws_threads, &gui_tx_ws, &ctx_ws);
                                                if !ws_threads.is_empty() {
                                                    crate::chat::resolve_thread_names(
                                                        &s_snap,
                                                        &mut ws_threads,
                                                    )
                                                    .await;
                                                    let _ = gui_tx_ws.send(
                                                        GuiEvent::ThreadsUpdated(ws_threads),
                                                    );
                                                    ctx_ws.request_repaint();
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

                // 2) Background HTTP long-poll fallback for VoIP control events (`lp_control`)
                {
                    let s_ctrl = session.clone();
                    let cps = ctrl_poll_state.clone();
                    let cb = call_bridge.clone();
                    tokio::spawn(async move {
                        loop {
                            let s_snap = s_ctrl.lock().await.clone();
                            if s_snap.login.is_some() {
                                let voip_res = {
                                    let mut st = cps.lock().await;
                                    crate::call::poll_voip_controls(&s_snap, &mut st).await
                                };
                                if let Ok(events) = voip_res {
                                    for ev in events {
                                        cb.handle_control_event(ev);
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

                let mut current_qr_task: Option<tokio::task::JoinHandle<()>> = None;

                while let Some(cmd) = cmd_rx.recv().await {
                    match cmd {
                        WorkerCmd::PersistThreads(threads_snapshot) => {
                            tokio::task::spawn_blocking(move || {
                                if let Ok(mut store) =
                                    crate::store::MessageStore::open(crate::paths::history_db())
                                {
                                    let _ =
                                        crate::store::persist_threads(&mut store, &threads_snapshot);
                                }
                                crate::chat::save_threads_cache(&threads_snapshot);
                            });
                        }
                        WorkerCmd::Logout => {
                            if let Some(task) = current_qr_task.take() {
                                task.abort();
                            }
                            // Logging out removes credentials only. The local message history
                            // (`zalo_history.db`) is kept: deleting it is a separate, explicit action.
                            let _ = std::fs::remove_file(crate::paths::session_file());
                            let _ = std::fs::remove_file(crate::paths::threads_cache_file());
                            {
                                let mut s = session.lock().await;
                                // Cursors belong to the session that ends here.
                                if let Some(path) = s
                                    .login
                                    .as_ref()
                                    .and_then(|l| crate::paths::sync_state_file(&l.uin))
                                {
                                    let _ = std::fs::remove_file(path);
                                }
                                s.logout();
                            }
                            {
                                let mut ps = poll_state.lock().await;
                                *ps = crate::chat::PollState::default();
                            }
                            if let Ok(mut c) = ws_cursors.lock() {
                                c.clear();
                            }
                            let _ = gui_tx.send(GuiEvent::LoggedOut);
                            egui_ctx.request_repaint();
                        }
                        WorkerCmd::RequestQr => {
                            if let Some(task) = current_qr_task.take() {
                                task.abort();
                            }
                            let session_clone = session.clone();
                            let poll_clone = poll_state.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            let wc_qr = ws_cursors.clone();

                            let handle = tokio::spawn(async move {
                                {
                                    let mut s = session_clone.lock().await;
                                    s.logout();
                                }
                                {
                                    let mut ps = poll_clone.lock().await;
                                    *ps = crate::chat::PollState::default();
                                }
                                // Another account may log in: never reuse its cursors.
                                if let Ok(mut c) = wc_qr.lock() {
                                    c.clear();
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
                                                    let name = scanned
                                                        .get("display_name")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("Zalo User");
                                                    let _ =
                                                        gui_tx_clone.send(GuiEvent::QrScanned {
                                                            name: name.to_string(),
                                                        });
                                                    ctx_clone.request_repaint();

                                                    let confirm_res =
                                                        crate::qr::confirm(&s_snap, &q).await;

                                                    if let Ok(resp) = confirm_res {
                                                        let mut s = session_clone.lock().await;
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
                                                        let _ = s.save();

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
                                                        let _ = s.save();

                                                        let uin = s
                                                            .login
                                                            .as_ref()
                                                            .map(|l| l.uin.clone())
                                                            .unwrap_or_default();
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
                                                        // Cursors saved earlier for this account, if any.
                                                        let saved = crate::sync_state::SyncState::load(&uin);
                                                        *poll_clone.lock().await = saved.poll;
                                                        if let Ok(mut c) = wc_qr.lock() {
                                                            *c = saved.ws_queues;
                                                        }
                                                        let _ = gui_tx_clone.send(
                                                            GuiEvent::LoginSuccess {
                                                                name: dname,
                                                                uin,
                                                                avatar: u_ava,
                                                            },
                                                        );
                                                        ctx_clone.request_repaint();

                                                        let mut list = match crate::chat::get_friends(&s).await {
                                                            Ok(l) => l,
                                                            Err(e) => {
                                                                let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!("{e:#}")));
                                                                Vec::new()
                                                            }
                                                        };
                                                        crate::store::load_history_into_threads(
                                                            crate::paths::history_db(),
                                                            &mut list,
                                                            200,
                                                        );
                                                        {
                                                            let mut ps = poll_clone.lock().await;
                                                            let _ = crate::chat::poll_new_messages(
                                                                &s, &mut ps, &mut list,
                                                            )
                                                            .await;
                                                        }
                                                        let _ = gui_tx_clone
                                                            .send(GuiEvent::ThreadsUpdated(list));
                                                        send_conversation_activity(&gui_tx_clone, &ctx_clone);
                                                        send_new_gaps(&gui_tx_clone, &ctx_clone);
                                                        ctx_clone.request_repaint();

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
                        WorkerCmd::RefreshThreads => {
                            let session_clone = session.clone();
                            let poll_clone = poll_state.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            let cu_rf = catchup.clone();
                            tokio::spawn(async move {
                                let mut s = session_clone.lock().await.clone();
                                if s.login.is_some() {
                                    let status = s.check_session().await;
                                    crate::trace_shape::line(
                                        "sync",
                                        format!("refresh: check_session -> {status:?}"),
                                    );
                                    match status {
                                        crate::session::SessionStatus::Valid => {
                                            REFRESH_RETRIES
                                                .store(0, std::sync::atomic::Ordering::SeqCst);
                                        }
                                        crate::session::SessionStatus::Unreachable(why) => {
                                            // Keep the saved session; say we are offline and
                                            // try again so the catch-up still happens.
                                            let _ = gui_tx_clone
                                                .send(GuiEvent::StatusUpdate(unreachable_status(&why)));
                                            ctx_clone.request_repaint();
                                            schedule_refresh_retry();
                                            return;
                                        }
                                        crate::session::SessionStatus::Expired => {
                                            on_session_expired(
                                                &session_clone,
                                                &gui_tx_clone,
                                                &ctx_clone,
                                                "Máy chủ Zalo báo phiên đăng nhập đã hết hạn — vui lòng quét mã QR để đăng nhập lại",
                                            )
                                            .await;
                                            return;
                                        }
                                    }
                                    if let Some((uid, name)) = s.sync_profile_me().await {
                                        {
                                            let mut sl = session_clone.lock().await;
                                            if let Some(ref mut li) = sl.login {
                                                li.uin = uid.clone();
                                                if !name.is_empty() {
                                                    li.display_name = name.clone();
                                                }
                                            }
                                            let _ = sl.save();
                                        }
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
                                    // Surface failures instead of silently showing an empty list.
                                    let mut errors: Vec<String> = Vec::new();
                                    let mut list = match crate::chat::get_friends(&s).await {
                                        Ok(l) => l,
                                        Err(e) => {
                                            errors.push(format!("{e:#}"));
                                            Vec::new()
                                        }
                                    };
                                    crate::store::load_history_into_threads(
                                        crate::paths::history_db(),
                                        &mut list,
                                        200,
                                    );
                                    crate::trace_shape::line(
                                        "sync",
                                        format!(
                                            "refresh: threads={} groups={} errors={}",
                                            list.len(),
                                            list.iter().filter(|t| t.is_group).count(),
                                            errors.len()
                                        ),
                                    );
                                    if let Ok(mut cu) = cu_rf.lock() {
                                        if !cu.active {
                                            cu.active = true;
                                            cu.ids.clear();
                                        }
                                        let _ = gui_tx_clone.send(GuiEvent::SyncProgress {
                                            fetched: cu.ids.len(),
                                            done: false,
                                        });
                                    }
                                    let before: HashSet<String> = list
                                        .iter()
                                        .flat_map(|t| t.messages.iter().map(move |m| catchup_key(&t.id, m)))
                                        .collect();
                                    {
                                        let mut ps = poll_clone.lock().await;
                                        if let Err(e) =
                                            crate::chat::poll_new_messages(&s, &mut ps, &mut list)
                                                .await
                                        {
                                            errors.push(format!("Không tải được tin nhắn mới: {e:#}"));
                                        }
                                    }
                                    crate::trace_shape::line("sync", "refresh: poll_new_messages done");
                                    if let Ok(mut cu) = cu_rf.lock() {
                                        for t in &list {
                                            for m in &t.messages {
                                                let k = catchup_key(&t.id, m);
                                                if !before.contains(&k) {
                                                    cu.ids.insert(k);
                                                }
                                            }
                                        }
                                        cu.active = false;
                                        let _ = gui_tx_clone.send(GuiEvent::SyncProgress {
                                            fetched: cu.ids.len(),
                                            done: true,
                                        });
                                    }
                                    send_conversation_activity(&gui_tx_clone, &ctx_clone);
                                    send_new_gaps(&gui_tx_clone, &ctx_clone);
                                    let thread_count = list.len();
                                    let _ = gui_tx_clone.send(GuiEvent::ThreadsUpdated(list));
                                    let status = if errors.is_empty() {
                                        format!(
                                            "Đã làm mới {thread_count} hội thoại lúc {}",
                                            crate::ui::clock()
                                        )
                                    } else {
                                        crate::session::truncate(&errors.join(" · "), 220)
                                    };
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(status));
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

                            let session_clone = session.clone();
                            let poll_clone = poll_state.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            let cu_pm = catchup.clone();
                            tokio::spawn(async move {
                                let s = session_clone.lock().await.clone();
                                if s.login.is_some() {
                                    let mut temp_threads: Vec<Thread> = known_threads
                                        .into_iter()
                                        .map(|(id, name, is_group)| Thread {
                                            id,
                                            name,
                                            preview: String::new(),
                                            time: String::new(),
                                            avatar: None,
                                            is_group,
                                            messages: Vec::new(),
                                        })
                                        .collect();

                                    let mut poll_err: Option<String> = None;
                                    for _ in 0..crate::chat::LP_CATCHUP_MAX_BATCHES {
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
                                                if !crate::chat::lp_reply_wants_more(&lp_resp) {
                                                    break;
                                                }
                                            }
                                            Err(e) => {
                                                poll_err = Some(e.to_string());
                                                break;
                                            }
                                        }
                                    }

                                    if let Some(err_s) = poll_err {
                                        if is_session_expired_error(&err_s) {
                                            on_session_expired(
                                                &session_clone,
                                                &gui_tx_clone,
                                                &ctx_clone,
                                                "Phiên đăng nhập (zpw_sek) đã hết hạn",
                                            )
                                            .await;
                                        }
                                    } else {
                                        send_new_gaps(&gui_tx_clone, &ctx_clone);
                                        temp_threads.retain(|t| !t.messages.is_empty());
                                        catchup_note(&cu_pm, &temp_threads, &gui_tx_clone, &ctx_clone);
                                        if !temp_threads.is_empty() {
                                            crate::chat::resolve_thread_names(
                                                &s,
                                                &mut temp_threads,
                                            )
                                            .await;
                                            let _ = gui_tx_clone
                                                .send(GuiEvent::ThreadsUpdated(temp_threads));
                                            ctx_clone.request_repaint();
                                        }
                                    }
                                }
                                IS_POLLING.store(false, std::sync::atomic::Ordering::SeqCst);
                            });
                        }
                        WorkerCmd::LoadGroupHistory { group_id, is_group } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let s = session_clone.lock().await.clone();
                                if s.login.is_some() {
                                    let is_send2me = s
                                        .login
                                        .as_ref()
                                        .map(|l| !l.send2me_id.is_empty() && l.send2me_id == group_id)
                                        .unwrap_or(false);
                                    let t = Thread {
                                        id: group_id.clone(),
                                        name: String::new(),
                                        preview: String::new(),
                                        time: String::new(),
                                        avatar: None,
                                        is_group: is_group && !is_send2me,
                                        messages: Vec::new(),
                                    };
                                    let mut t_list = vec![t];
                                    crate::store::load_history_into_threads(
                                        crate::paths::history_db(),
                                        &mut t_list,
                                        500,
                                    );
                                    let Some(mut t) =
                                        t_list.into_iter().find(|x| x.id == group_id)
                                    else {
                                        return;
                                    };
                                    if is_group && !is_send2me {
                                        if let Ok(resp) =
                                            crate::chat::get_group_history(&s, &group_id, 50).await
                                        {
                                            crate::chat::merge_group_history(&mut t, &resp);
                                        }
                                    }
                                    if is_group || is_send2me {
                                        if let Ok(cm_resp) = crate::chat::get_cloud_messages(
                                            &s,
                                            &group_id,
                                            "0",
                                            false,
                                            50,
                                        )
                                        .await
                                        {
                                            crate::chat::merge_group_history(&mut t, &cm_resp);
                                        }
                                        if let Ok(cm_old) = crate::chat::get_cloud_messages(
                                            &s,
                                            &group_id,
                                            "",
                                            true,
                                            50,
                                        )
                                        .await
                                        {
                                            crate::chat::merge_group_history(&mut t, &cm_old);
                                        }
                                    }
                                    if !t.messages.is_empty() {
                                        let cnt = t.messages.len();
                                        let _ = gui_tx_clone.send(GuiEvent::ThreadsUpdated(vec![t]));
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Đã đồng bộ {cnt} tin nhắn lịch sử"
                                        )));
                                        ctx_clone.request_repaint();
                                    }
                                }
                            });
                        }
                        WorkerCmd::SendMessage {
                            to_id,
                            text,
                            is_group,
                        } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let s = session_clone.lock().await.clone();
                                if let Some(found_url) = crate::chat::extract_first_url(&text) {
                                    // The user is sending this link: a direct OpenGraph fetch
                                    // from this machine is acceptable here.
                                    let meta = crate::chat::parse_link_metadata(&s, &found_url, true)
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
                                    match crate::chat::send_link_message(
                                        &s, &to_id, &text, &meta, is_group,
                                    )
                                    .await
                                    {
                                        Ok(resp) => {
                                            let msg_id = extract_msg_id(&resp);
                                            let now = crate::ui::clock();
                                            let final_id = if msg_id.is_empty() {
                                                format!("out_{}", crate::login::unix_now())
                                            } else {
                                                msg_id
                                            };
                                            let msg = ChatMessage {
                                                id: final_id,
                                                sender: "Tôi".into(),
                                                text,
                                                time: now,
                                                is_outgoing: true,
                                                ts: (crate::login::unix_now() * 1000) as i64,
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
                                    crate::chat::send_group_message(&s, &to_id, &text).await
                                } else {
                                    crate::chat::send_message(&s, &to_id, &text).await
                                };
                                match res {
                                    Ok(resp) => {
                                        let msg_id = extract_msg_id(&resp);
                                        let _ = gui_tx_clone.send(GuiEvent::MessageSent {
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
                                        if is_session_expired_error(&err_s) {
                                            on_session_expired(
                                                &session_clone,
                                                &gui_tx_clone,
                                                &ctx_clone,
                                                "Không thể gửi vì phiên zpw_sek đã hết hạn",
                                            )
                                            .await;
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let s = session_clone.lock().await.clone();
                                match crate::chat::send_quote_message(
                                    &s, &to_id, &text, &quoted, is_group,
                                )
                                .await
                                {
                                    Ok(resp) => {
                                        let _ = gui_tx_clone.send(GuiEvent::QuoteSent {
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
                        } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let s = session_clone.lock().await.clone();
                                let res = crate::chat::forward_message(
                                    &s,
                                    &target_id,
                                    target_is_group,
                                    &msg,
                                )
                                .await;
                                match res {
                                    Ok(resp) if resp_error_code(&resp).is_none() => {
                                        let msg_id = extract_msg_id(&resp);
                                        if let Some(sid) = msg.sticker_id.filter(|v| *v > 0) {
                                            let _ = gui_tx_clone.send(GuiEvent::StickerSent {
                                                thread_id: target_id,
                                                msg_id,
                                                sticker_id: sid,
                                                cate_id: msg.sticker_cate_id.unwrap_or(0),
                                                sticker_type: msg.sticker_type.unwrap_or(7),
                                            });
                                        } else {
                                            let _ = gui_tx_clone.send(GuiEvent::MessageSent {
                                                thread_id: target_id,
                                                text: msg.text.clone(),
                                                msg_id,
                                            });
                                        }
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                            "Đã chuyển tiếp tin nhắn".into(),
                                        ));
                                    }
                                    Ok(resp) => {
                                        let err_s = format!(
                                            "server error {}",
                                            resp_error_code(&resp).unwrap_or(0)
                                        );
                                        crate::logger::log_error("gui::forward", &err_s);
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Chuyển tiếp thất bại: {err_s}"
                                        )));
                                    }
                                    Err(e) => {
                                        crate::logger::log_error("gui::forward", &e.to_string());
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Chuyển tiếp thất bại: {e}"
                                        )));
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
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
                                        let _ = tokio::task::spawn_blocking(move || {
                                            if let Ok(mut store) =
                                                crate::store::MessageStore::open(crate::paths::history_db())
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
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
                        WorkerCmd::SendSticker {
                            to_id,
                            sticker_id,
                            cate_id,
                            sticker_type,
                            is_group,
                        } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let s = session_clone.lock().await.clone();
                                let res = crate::chat::send_sticker(
                                    &s,
                                    &to_id,
                                    sticker_id,
                                    cate_id,
                                    sticker_type,
                                    is_group,
                                )
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
                                        if is_session_expired_error(&err_s) {
                                            on_session_expired(
                                                &session_clone,
                                                &gui_tx_clone,
                                                &ctx_clone,
                                                "Không thể gửi nhãn dán vì phiên zpw_sek đã hết hạn",
                                            )
                                            .await;
                                        }
                                        ctx_clone.request_repaint();
                                    }
                                }
                            });
                        }
                        WorkerCmd::FetchStickerImage { key, url } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let cache_path =
                                    crate::paths::cache_dir().join(format!("{key}.png"));

                                let bytes_opt = if let Ok(disk_bytes) = std::fs::read(&cache_path) {
                                    if image::load_from_memory(&disk_bytes).is_ok() {
                                        Some(disk_bytes)
                                    } else {
                                        let _ = std::fs::remove_file(&cache_path);
                                        None
                                    }
                                } else if !url.starts_with("http://") && !url.starts_with("https://") {
                                    // Locally sent photos only — never UNC / network paths.
                                    local_image_path(&url).and_then(|p| std::fs::read(p).ok())
                                } else {
                                    let client = { session_clone.lock().await.clone().http };
                                    match fetch_image_bytes(&client, &url).await {
                                        Some(vec) if image::load_from_memory(&vec).is_ok() => {
                                            let _ = std::fs::write(&cache_path, &vec);
                                            Some(vec)
                                        }
                                        _ => None,
                                    }
                                };

                                if let Some(bytes) = bytes_opt {
                                    if let Ok(dyn_img) = image::load_from_memory(&bytes) {
                                        let mut rgba = dyn_img.to_rgba8();
                                        // Apply smooth anti-aliased circular alpha mask for avatar textures (`ava_*`)
                                        if key.starts_with("ava_") {
                                            let side = rgba.width().min(rgba.height()).clamp(32, 160);
                                            let x0 = rgba.width().saturating_sub(side) / 2;
                                            let y0 = rgba.height().saturating_sub(side) / 2;
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
                                }
                            });
                        }
                        WorkerCmd::FetchStickerStore => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let s = session_clone.lock().await.clone();
                                // Automatic previews (received messages, composer) only use
                                // Zalo's server-side parser: no request from this machine to an
                                // arbitrary URL (privacy: IP leak / LAN probing).
                                if let Ok(meta) = crate::chat::parse_link_metadata(&s, &url, false).await {
                                    let _ = gui_tx_clone.send(GuiEvent::LinkPreviewReady(meta));
                                    ctx_clone.request_repaint();
                                }
                            });
                        }
                        WorkerCmd::PickAndSendAttachment {
                            to_id,
                            is_group,
                            images_only,
                            caption,
                            preset_path,
                        } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let picked_path = if let Some(p) = preset_path {
                                    Some(p)
                                } else {
                                    tokio::task::spawn_blocking(move || {
                                        pick_native_file_dialog(images_only)
                                    })
                                    .await
                                    .ok()
                                    .flatten()
                                };

                                let Some(path) = picked_path else {
                                    return;
                                };
                                let fname = path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("tập tin")
                                    .to_string();
                                let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                    "⏳ Đang tải lên và gửi '{fname}'…"
                                )));
                                ctx_clone.request_repaint();

                                let s = session_clone.lock().await.clone();
                                match crate::chat::send_attachment(
                                    &s, &to_id, is_group, &path, &caption,
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
                                        crate::logger::log_error("gui::send_attachment", &err_s);
                                        let _ = gui_tx_clone.send(GuiEvent::SendFailed {
                                            thread_id: to_id,
                                            text: caption,
                                            error: format!("Lỗi gửi '{fname}': {err_s}"),
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
                        WorkerCmd::AnswerCall { accept } => {
                            call_bridge.answer_call(accept);
                            egui_ctx.request_repaint();
                        }
                        WorkerCmd::SetCallMediaState {
                            is_on_mic,
                            is_on_cam,
                        } => {
                            call_bridge.set_media_state(is_on_mic, is_on_cam);
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let s = session_clone.lock().await.clone();
                                if let Ok(status) =
                                    crate::chat::check_friendship_status(&s, &user_id).await
                                {
                                    let _ = gui_tx_clone
                                        .send(GuiEvent::FriendStatusChecked { user_id, status });
                                    ctx_clone.request_repaint();
                                }
                            });
                        }
                        WorkerCmd::AcceptFriend { user_id, user_name } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let s = session_clone.lock().await.clone();
                                match crate::chat::accept_friend_request(&s, &user_id).await {
                                    Ok(_) => {
                                        let _ = gui_tx_clone.send(GuiEvent::FriendStatusChecked {
                                            user_id,
                                            status: crate::chat::FriendshipStatus::Friend,
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let s = session_clone.lock().await.clone();
                                if crate::chat::reject_friend_request(&s, &user_id)
                                    .await
                                    .is_ok()
                                {
                                    let _ = gui_tx_clone.send(GuiEvent::FriendStatusChecked {
                                        user_id,
                                        status: crate::chat::FriendshipStatus::Stranger,
                                    });
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                        "Đã từ chối lời mời kết bạn của {user_name}."
                                    )));
                                    ctx_clone.request_repaint();
                                }
                            });
                        }
                        WorkerCmd::UndoFriend { user_id, user_name } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let s = session_clone.lock().await.clone();
                                if crate::chat::undo_friend_request(&s, &user_id).await.is_ok() {
                                    let _ = gui_tx_clone.send(GuiEvent::FriendStatusChecked {
                                        user_id,
                                        status: crate::chat::FriendshipStatus::Stranger,
                                    });
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                        "Đã hủy lời mời kết bạn tới {user_name}."
                                    )));
                                    ctx_clone.request_repaint();
                                }
                            });
                        }
                        WorkerCmd::AddFriend { user_id, user_name } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let s = session_clone.lock().await.clone();
                                // Check if there is an incoming request we can accept directly
                                if crate::chat::accept_friend_request(&s, &user_id).await.is_ok() {
                                    let _ = gui_tx_clone.send(GuiEvent::FriendStatusChecked {
                                        user_id,
                                        status: crate::chat::FriendshipStatus::Friend,
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
                        WorkerCmd::DownloadAttachment {
                            url,
                            suggested_name,
                            thread_id: _,
                            dest_dir,
                        } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let result = match crate::download::validate_target_dir(&dest_dir) {
                                    Err(e) => Err(e),
                                    Ok(dir) => {
                                        let client = { session_clone.lock().await.clone().http };
                                        let tx = gui_tx_clone.clone();
                                        let ctx = ctx_clone.clone();
                                        let key = url.clone();
                                        crate::download::download_to_dir(
                                            &client,
                                            &url,
                                            &suggested_name,
                                            &dir,
                                            move |p| {
                                                let _ = tx.send(GuiEvent::DownloadProgress {
                                                    url: key.clone(),
                                                    received: p.received,
                                                    total: p.total,
                                                });
                                                ctx.request_repaint();
                                            },
                                        )
                                        .await
                                    }
                                };
                                match result {
                                    Ok(path) => {
                                        let _ = gui_tx_clone
                                            .send(GuiEvent::DownloadFinished { url, path });
                                    }
                                    Err(error) => {
                                        crate::logger::log_error("gui::download", &error);
                                        let _ = gui_tx_clone
                                            .send(GuiEvent::DownloadFailed { url, error });
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::PrepareVideo { url } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let dir = crate::paths::video_cache_dir();
                                let name = format!("{}.mp4", crate::crypto::md5_hex(url.trim()));
                                let cached = dir.join(&name);
                                let result = if cached.is_file() {
                                    Ok(cached)
                                } else {
                                    let client = { session_clone.lock().await.clone().http };
                                    let tx = gui_tx_clone.clone();
                                    let ctx = ctx_clone.clone();
                                    let key = url.clone();
                                    crate::download::download_to_dir(
                                        &client,
                                        &url,
                                        &name,
                                        &dir,
                                        move |p| {
                                            let _ = tx.send(GuiEvent::VideoProgress {
                                                url: key.clone(),
                                                received: p.received,
                                                total: p.total,
                                            });
                                            ctx.request_repaint();
                                        },
                                    )
                                    .await
                                };
                                match result {
                                    Ok(path) => {
                                        let _ = gui_tx_clone.send(GuiEvent::VideoReady { url, path });
                                    }
                                    Err(error) => {
                                        crate::logger::log_error("gui::video", &error);
                                        let _ = gui_tx_clone.send(GuiEvent::VideoFailed { url, error });
                                    }
                                }
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::DismissMissingRanges => {
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::task::spawn_blocking(move || {
                                if let Ok(store) =
                                    crate::store::MessageStore::open(crate::paths::history_db())
                                {
                                    let _ = store.dismiss_missing_ranges();
                                }
                                let _ = gui_tx_clone.send(GuiEvent::MissingRanges(Vec::new()));
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::ComputeCacheSize => {
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::task::spawn_blocking(move || {
                                let bytes = crate::download::dir_size(&crate::paths::cache_dir());
                                let _ = gui_tx_clone.send(GuiEvent::CacheSizeComputed(bytes));
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::ClearImageCache => {
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::task::spawn_blocking(move || {
                                // Only `stickers_cache/` — never session.json, the history DB or
                                // app_settings.json, which live one level up.
                                let (freed, failed) =
                                    crate::download::clear_dir_contents(&crate::paths::cache_dir());
                                let _ = gui_tx_clone
                                    .send(GuiEvent::ImageCacheCleared { freed, failed });
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::LookupPhone { phone } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
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
                            let session_clone = session.clone();
                            let poll_clone = poll_state.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                    "Đang xác thực tài khoản…".into(),
                                ));
                                ctx_clone.request_repaint();
                                let mut s = session_clone.lock().await;
                                match crate::login::login_by_password(&mut s, &phone, &pass, "VN")
                                    .await
                                {
                                    Ok(_) => {
                                        if let Ok(info) = crate::login::get_login_info(&s).await {
                                            if let Some(cfg) = info.get("data") {
                                                crate::login::apply_login_config(&mut s, cfg);
                                                s.apply_server_info(cfg);
                                                let _ = s.save();

                                                let uin = s
                                                    .login
                                                    .as_ref()
                                                    .map(|l| l.uin.clone())
                                                    .unwrap_or_default();
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
                                                    uin,
                                                    avatar: u_ava,
                                                });
                                                ctx_clone.request_repaint();

                                                if let Ok(mut list) =
                                                    crate::chat::get_friends(&s).await
                                                {
                                                    {
                                                        let mut ps = poll_clone.lock().await;
                                                        *ps = crate::chat::PollState::default();
                                                        let _ = crate::chat::poll_new_messages(
                                                            &s, &mut ps, &mut list,
                                                        )
                                                        .await;
                                                    }
                                                    let _ = gui_tx_clone
                                                        .send(GuiEvent::ThreadsUpdated(list));
                                                    ctx_clone.request_repaint();
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
                        }
                    }
                }
            });
        })
        .expect("Failed to spawn zalo-worker thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_auth_codes_count_as_expired() {
        assert!(is_session_expired_error("server error 600: zpw_sek not found (from https://x)"));
        assert!(is_session_expired_error("server error -102: chưa đăng nhập"));
        // Network / HTTP problems never log the user out (issue #21).
        assert!(!is_session_expired_error("HTTP 404 Not Found from https://tt-friend-wpa.zaloapp.com"));
        assert!(!is_session_expired_error("error sending request: dns error"));
        assert!(!is_session_expired_error("server error 6001: something else"));
        assert!(!is_session_expired_error("zpw_sek appears in some other message"));
    }

    #[test]
    fn image_urls_follow_scheme_policy() {
        assert!(image_url_allowed("https://s120-ava-talk.zadn.vn/a.jpg"));
        assert!(image_url_allowed("https://example.com/og.png"));
        assert!(image_url_allowed("http://stc-chat.zdn.vn/images/stickers/default/thumb/1.png"));
        assert!(!image_url_allowed("http://example.com/tracker.png"));
        assert!(!image_url_allowed("file:///etc/passwd"));
        assert!(!image_url_allowed("ftp://zdn.vn/x.png"));
        assert!(!image_url_allowed("not a url"));
    }

    #[test]
    fn local_images_refuse_network_paths() {
        assert!(local_image_path(r"\\attacker.example\share\x.png").is_none());
        assert!(local_image_path("local://\\\\attacker\\share\\x.png").is_none());
        assert!(local_image_path("//attacker/share/x.png").is_none());
        assert!(local_image_path("relative/x.png").is_none());
        assert!(local_image_path("").is_none());

        let dir = std::env::temp_dir().join(format!("qtzalo-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x.png");
        std::fs::write(&f, b"\x89PNG").unwrap();
        let as_local = format!("local://{}", f.display());
        assert_eq!(local_image_path(&as_local).as_deref(), Some(f.as_path()));
        assert_eq!(local_image_path(&f.display().to_string()).as_deref(), Some(f.as_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

