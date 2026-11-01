use super::*;

/// Thread (id, name, is_group) list the poll timer maps incoming messages onto; the GUI
/// republishes it whenever its threads change.
type KnownThreadsSnapshot = (u64, Vec<(String, String, bool)>);
pub(super) static KNOWN_THREADS: std::sync::Mutex<KnownThreadsSnapshot> =
    std::sync::Mutex::new((0, Vec::new()));

pub enum WorkerCmd {
    /// Persist the actual viewport read, then send only independently verified live receipts.
    AcknowledgeVisibleRead { account: String, thread_id: String, is_group: bool, visible: Vec<ChatMessage>, receipts: Vec<ChatMessage>, ticket: crate::read_receipts::VisibilityTicket },
    /// Open a native photo viewer only following an explicit viewer-button click.
    OpenNativePhoto { key: crate::media_resources::MediaKey, url: String },
    /// Explicitly select and adopt a validated original reaction-cache export.
    ImportOriginalReactions {
        account: String,
        path: Option<std::path::PathBuf>,
    },
    /// Render one exact grapheme through the bounded native emoji helper off the UI thread.
    RenderInlineEmoji { cluster: String, sprite_id: Option<u16> },
    /// Import and hydrate local history on the blocking pool.
    ImportHistory {
        account: String,
        revisions: std::collections::HashMap<String, u64>,
        path: std::path::PathBuf,
    },
    /// Fetch only photo cards with a bounded original-source fallback.
    FetchPhotoPreview {
        key: crate::media_resources::MediaKey,
        thumbnail: String,
        source: Option<String>,
    },
    /// Deliver an already admitted background incoming-message alert.
    DeliverIncomingNotification {
        account: String,
        thread: String,
        message: String,
        sound: bool,
    },
    /// Replay durable reactions into a loaded snapshot on the blocking worker.
    ReplayReactions {
        account: String,
        threads: Vec<Thread>,
    },
    /// Drain account writes and native media teardown before the GUI closes.
    Shutdown,
    /// Read OS authorization without prompting.
    ReadNotificationAuthorization { request_id: u64 },
    /// Request OS authorization following an explicit Settings click.
    RequestNotificationAuthorization { request_id: u64 },
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
    /// Explicit bounded group history request over the existing shared socket.
    LoadGroupHistoryStream,
    /// Phone transfer controls preserve user intent order through the shared worker.
    PhoneHistory(super::phone_history::PhoneAction),
    /// Page group / Cloud history back from the server into SQLite: (id, name, is_group).
    SyncHistory {
        threads: Vec<(String, String, bool)>,
    },
    SendMessage {
        to_id: String,
        text: String,
        is_group: bool,
    },
    /// Explicit member mentions are group-only; optional quote retains its original message object.
    SendWithMentions {
        to_id: String,
        text: String,
        mentions: Vec<crate::composer_mentions::OutgoingMention>,
        quoted: Option<ChatMessage>,
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
    /// Persist only the account that supplied this UI snapshot.
    PersistThreads { account: String, threads: Vec<Thread> },
    /// Load one bounded local-history page without requesting network history.
    LoadHistoryPage { account: String, thread_id: String, revision: u64, before: Option<crate::store_v2::HistoryCursor> },
    /// Resolve an offscreen exact quote against durable account history.
    NavigateHistory { account: String, thread_id: String, reference: String, request_id: u64, revision: u64 },
    /// Search durable history outside the loaded viewport.
    SearchHistory { account: String, thread_id: String, query: String, revision: u64 },
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
    /// Clear only the current account-owned history without touching legacy data or credentials. Separate from [`WorkerCmd::Logout`] by design (#21).
    WipeLocalData { account: String },
    PinThread {
        thread_id: String,
        is_group: bool,
        pin: bool,
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
    DeleteThread {
        thread_id: String,
        is_group: bool,
        last_cli_msg_id: String,
    },
    FetchSocialProfile {
        target_uid: String,
        request_id: u64,
    },
    FetchMySocialAvatars {
        page: u32,
        count: u32,
    },
    FetchTimelineFeed {
        friend_uids: Vec<String>,
        shared_media_by_uid: std::collections::HashMap<String, Vec<crate::feed::FeedMedia>>,
    },
}

/// Dispatch only receipts belonging to the still-current authenticated session.
/// The runtime integration must additionally scope this event by session generation.
async fn dispatch_delivery_receipts(
    session: &std::sync::Arc<tokio::sync::Mutex<Session>>,
    captured: &Session,
    sender: &GuiTx,
    cmd: u16,
    sub_cmd: u8,
    payload: &serde_json::Value,
) {
    let account = captured
        .login
        .as_ref()
        .map(|login| login.uin.as_str())
        .unwrap_or_default();
    let events = crate::delivery_receipts::parse_delivery_events(cmd, sub_cmd, payload, account);
    if events.is_empty() {
        return;
    }
    // Keep the session check and channel emission in one short, I/O-free section.
    let current = session.lock().await;
    if current.login.as_ref().map(|login| login.uin.as_str()) != Some(account)
        || current.zpw_sek != captured.zpw_sek
    {
        return;
    }
    let result = sender.send(GuiEvent::DeliveryEvents {
        account: account.into(),
        events,
    });
    drop(current);
    if result.is_err() {
        crate::logger::log_error("gui::delivery_receipts", "UI event receiver closed");
    }
}

fn dispatch_group_history_request(
    sync: &mut super::group_history::GroupHistorySync,
    commands: Option<&tokio::sync::mpsc::UnboundedSender<crate::socket::WsCommand>>,
    request: Option<super::group_history::GroupHistoryRequest>,
    gui_tx: &GuiTx,
) {
    if let Some(request) = request {
        let sent = commands.is_some_and(|commands| {
            commands
                .send(crate::socket::WsCommand::RequestGroupHistory {
                    last_id: request.last_id,
                })
                .is_ok()
        });
        if !sent {
            sync.fail();
            let _ = gui_tx.send(GuiEvent::GroupHistoryStatus(
                "Lịch sử nhóm: chưa có kết nối; bấm tải thêm để thử lại".into(),
            ));
            return;
        }
    }
    let _ = gui_tx.send(GuiEvent::GroupHistoryStatus(sync.status()));
}

async fn process_original_reaction_import(
    root: std::path::PathBuf,
    session: std::sync::Arc<tokio::sync::Mutex<Session>>,
    captured: Session,
    sender: GuiTx,
    account: String,
    selected: Option<std::path::PathBuf>,
    pick_file: bool,
) {
    use crate::original_reactions::ImportError;
    let result = if captured.login.as_ref().map(|l| l.uin.as_str()) != Some(account.as_str()) {
        Err(ImportError::StaleAccount)
    } else {
        // No session or journal locks are held while an explicit native picker is open.
        let selected = if pick_file {
            crate::runtime_scope::spawn_blocking(|| pick_native_file_dialog(false))
                .await
                .map_err(|_| ImportError::Worker)
                .and_then(|result| result.map_err(|_| ImportError::StaleAccount))
        } else {
            Ok(selected)
        };
        match selected {
            Ok(None) => Ok(None),
            Err(error) => Err(error),
            Ok(Some(selected)) => {
                let _lane = REACTION_JOURNAL_LANE.lock().await;
                let import_account = account.clone();
                match crate::runtime_scope::spawn_blocking(move || {
                    let current = session.blocking_lock();
                    let _guard = match crate::runtime_scope::mutation() {
                        Ok(guard) => guard,
                        Err(_) => return Err(ImportError::StaleAccount),
                    };
                    if current.login.as_ref().map(|l| l.uin.as_str())
                        != Some(import_account.as_str())
                        || current.zpw_sek != captured.zpw_sek
                    {
                        return Err(ImportError::StaleAccount);
                    }
                    crate::original_reactions::import_selected_cache(
                        &root,
                        &selected,
                        &import_account,
                    )
                    .map(Some)
                })
                .await
                {
                    Ok(Ok(result)) => result,
                    Ok(Err(_)) => Err(ImportError::StaleAccount),
                    Err(_) => Err(ImportError::Worker),
                }
            }
        }
    };
    // The UI may have closed or rejected the initiating scope while the import ran.
    let _ = sender.send(GuiEvent::OriginalReactionImportFinished { account, result });
}

// A single lane serializes append/replay across accounts, bounding concurrent SQLite writers.
static REACTION_JOURNAL_LANE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn process_reaction_journal(
    session: std::sync::Arc<tokio::sync::Mutex<Session>>,
    captured: Session,
    sender: GuiTx,
    events: Vec<crate::reactions::ReactionEvent>,
    threads: Vec<Thread>,
) {
    process_reaction_journal_at(
        std::path::PathBuf::from("."),
        session,
        captured,
        sender,
        events,
        threads,
    )
    .await;
}

async fn process_reaction_journal_at(
    root: std::path::PathBuf,
    session: std::sync::Arc<tokio::sync::Mutex<Session>>,
    captured: Session,
    sender: GuiTx,
    events: Vec<crate::reactions::ReactionEvent>,
    mut threads: Vec<Thread>,
) {
    if events.is_empty() && threads.is_empty() { return; }
    let account = captured
        .login
        .as_ref()
        .map(|l| l.uin.clone())
        .unwrap_or_default();
    if account.is_empty() {
        return;
    }
    let _lane = REACTION_JOURNAL_LANE.lock().await;
    let result =
        crate::runtime_scope::spawn_blocking(move || -> anyhow::Result<Option<(String, Vec<Thread>)>> {
            // Keep the account stable through all journal side effects. The runtime epoch fence
            // additionally rejects same-account reauthentication and queued stale UI results.
            let current = session.blocking_lock();
            let _guard = crate::runtime_scope::mutation()?;
            if current.login.as_ref().map(|l| &l.uin) != Some(&account)
                || current.zpw_sek != captured.zpw_sek
            {
                return Ok(None);
            }
            let mut journal = crate::reaction_journal::ReactionJournal::open(&root, &account)?;
            journal.append(&events)?;
            if !threads.is_empty() {
                if let Some(cache) = crate::original_reactions::load_imported_cache(&root, &account)? {
                    cache.apply_to_threads(&mut threads);
                }
                journal.apply_loaded(&mut threads)?;
            }
            Ok(Some((account, threads)))
        })
        .await;
    match result {
        Ok(Ok(Ok(Some((account, threads))))) => {
            let event = if threads.is_empty() {
                GuiEvent::ReactionEvents { account }
            } else {
                GuiEvent::ReactionThreadsUpdated { account, threads }
            };
            let _ = sender.send(event);
        }
        Ok(Ok(Ok(None))) => {}
        Ok(Err(_)) => {}
        _ => {
            let _ = sender.send(GuiEvent::StatusUpdate(
                "Chưa lưu hoặc khôi phục được sự kiện thả cảm xúc; vui lòng thử đồng bộ lại".into(),
            ));
        }
    }
}

/// Enqueue-time epoch capture prevents old UI commands from adopting a new login.
pub(super) struct WorkerSender {
    sender: tokio::sync::mpsc::UnboundedSender<(u64, WorkerCmd)>,
    ui_thread: std::thread::ThreadId,
    ui_generation: std::sync::atomic::AtomicU64,
}
impl WorkerSender {
    pub(super) fn acknowledge_ui_generation(&self, generation: u64) {
        self.ui_generation.store(generation, std::sync::atomic::Ordering::SeqCst);
    }
    pub(super) fn ui_generation(&self) -> u64 {
        self.ui_generation.load(std::sync::atomic::Ordering::SeqCst)
    }
    #[allow(clippy::result_large_err)]
    fn send_at(&self, generation: u64, command: WorkerCmd) -> Result<(), tokio::sync::mpsc::error::SendError<(u64, WorkerCmd)>> {
        self.sender.send((generation, command))
    }
    #[allow(clippy::result_large_err)]
    fn send_lifecycle_from(&self, generation: u64, command: WorkerCmd) -> Result<(), tokio::sync::mpsc::error::SendError<(u64, WorkerCmd)>> {
        match crate::runtime_scope::registry().invalidate_if_current(generation) {
            Ok(next) => self.send_at(next, command),
            Err(_) => Err(tokio::sync::mpsc::error::SendError((generation, command))),
        }
    }
    #[allow(clippy::result_large_err)]
    pub(super) fn send(&self, command: WorkerCmd) -> Result<(), tokio::sync::mpsc::error::SendError<(u64, WorkerCmd)>> {
        let lifecycle = matches!(command, WorkerCmd::Shutdown | WorkerCmd::Logout | WorkerCmd::WipeLocalData { .. } | WorkerCmd::RequestQr | WorkerCmd::LoginPassword { .. });
        let generation = if lifecycle {
            // A late expired-session response must not invalidate a newer login.
            if let Some(scope) = crate::runtime_scope::execution_scope() {
                match crate::runtime_scope::registry().invalidate_if_current(scope.generation) {
                    Ok(generation) => generation,
                    Err(_) => return Err(tokio::sync::mpsc::error::SendError((scope.generation, command))),
                }
            } else {
                crate::runtime_scope::registry().invalidate_now();
                crate::runtime_scope::registry().generation()
            }
        } else if let Some(scope) = crate::runtime_scope::execution_scope() {
            scope.generation
        } else if std::thread::current().id() == self.ui_thread {
            // The visible UI may not have consumed LoginSuccess yet. Never adopt the newer
            // worker account for commands or snapshots built from the old screen's state.
            self.ui_generation()
        } else { crate::runtime_scope::registry().generation() };
        self.send_at(generation, command)
    }
}
pub(super) static WORKER_TX: std::sync::OnceLock<WorkerSender> =
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

/// Immediately decode any disk-cached sticker thumbnails (`stickers_cache/stk_{cid}_{sid}.png`)
/// and push `GuiEvent::StickerImageReady` *before* sending sticker list metadata, returning any
/// missing `(key, url, cache_path)` entries for concurrent network prefetching.
pub(super) fn preload_disk_sticker_thumbnails(
    items: &[crate::chat::StickerItem],
    gui_tx: &GuiTx,
    limit: usize,
) -> Vec<(String, String, std::path::PathBuf)> {
    let cache_dir = std::path::Path::new("stickers_cache");
    if let Ok(_scope_guard) = crate::runtime_scope::mutation() {
        let _ = std::fs::create_dir_all(cache_dir);
    }

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
            } else if let Ok(_scope_guard) = crate::runtime_scope::mutation() {
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
                            if let Ok(_scope_guard) = crate::runtime_scope::mutation() {
                                let _ = std::fs::write(&cache_path, &vec);
                            }
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

pub(super) fn spawn_session_worker(session: Session, gui_tx: GuiTx, egui_ctx: egui::Context) {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, WorkerCmd)>();
    let _ = crate::runtime_scope::transition(session.login.as_ref().map(|login| login.uin.as_str()).unwrap_or(""));
    let _ = WORKER_TX.set(WorkerSender {
        sender: cmd_tx, ui_thread: std::thread::current().id(),
        ui_generation: std::sync::atomic::AtomicU64::new(crate::runtime_scope::registry().generation()),
    });

    let (phone_tx, mut phone_rx) = tokio::sync::mpsc::unbounded_channel();
    let (group_tx, mut group_rx) = tokio::sync::mpsc::unbounded_channel();
    let session = std::sync::Arc::new(tokio::sync::Mutex::new(session));
    let poll_state =
        std::sync::Arc::new(tokio::sync::Mutex::new(crate::chat::PollState::default()));
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
                    let scope = crate::runtime_scope::registry().current();
                    crate::runtime_scope::run(scope, async { let _ = crate::login::get_server_info(&s).await; }).await;
                }
                {
                    let startup_scope = crate::runtime_scope::registry().current();
                    crate::runtime_scope::run(startup_scope, async {
                        let captured = session.lock().await.clone();
                        let account = captured.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
                        if !account.is_empty() {
                            match account_history(session.clone(), captured, account.clone(), history::Request::Load(Vec::new())).await {
                                Ok(history::Reply::Threads(threads)) => { let _ = gui_tx.send(GuiEvent::AccountThreadsUpdated { account, threads, live: false }); }
                                _ => { let _ = gui_tx.send(GuiEvent::StatusUpdate("Chưa khôi phục được lịch sử tài khoản".into())); }
                            }
                        }
                    }).await;
                }
                // Message poll timer. It used to live in the GUI's update(), which forced a full
                // repaint every 1.2 s just to fire it. The PollMessages handler skips while a
                // long-poll is in flight and when logged out.
                tokio::spawn(async {
                    let mut tick = tokio::time::interval(Duration::from_millis(1200));
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        let (generation, known_threads) =
                            KNOWN_THREADS.lock().map(|k| k.clone()).unwrap_or_default();
                        if let Some(tx) = WORKER_TX.get() {
                            let _ = tx.send_at(generation, WorkerCmd::PollMessages { known_threads });
                        }
                    }
                });

                // Scheduled messages worker (#38) + optional HTTP REST API server (#35)
                if let Ok(port_str) = std::env::var("CTDOZLITE_API_PORT") {
                    if let Ok(port) = port_str.parse::<u16>() {
                        if port > 0 {
                            let s_api = session.clone();
                            tokio::spawn(async move {
                                let _ = crate::api_mcp::run_scoped_local_http_server(
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
                        let mut tick = tokio::time::interval(Duration::from_secs(2));
                        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                        loop {
                            tick.tick().await;
                            let scope = crate::runtime_scope::registry().current();
                            if scope.account.is_empty() { continue; }
                            crate::runtime_scope::run(scope, async {
                            let now_ts = crate::login::unix_now() as i64;
                            let Ok(batch) = crate::scheduler::claim_due_scheduled_batch("zalo_history.db", now_ts) else { return; };
                            let due = batch.messages.clone();
                            if due.is_empty() { return; }
                            let s = s_sched.lock().await.clone();
                            if s.login.is_none() {
                                for item in due {
                                    let _ = crate::scheduler::mark_scheduled_failed(
                                        "zalo_history.db",
                                        item.id,
                                        "Chưa đăng nhập phiên Zalo",
                                    );
                                }
                                let _ = gui_tx_sched.send(GuiEvent::ScheduledListUpdated);
                                ctx_sched.request_repaint();
                                return;
                            }
                            for item in due {
                                batch.begin_send(item.id);
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
                                                "zalo_history.db",
                                                item.id,
                                                &format!("Mã lỗi Zalo {code}"),
                                            );
                                        } else {
                                            let msg_id = extract_msg_id(&resp);
                                            let _ = crate::scheduler::mark_scheduled_sent(
                                                "zalo_history.db",
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
                                            "zalo_history.db",
                                            item.id,
                                            &e.to_string(),
                                        );
                                    }
                                }
                            }
                            let _ = gui_tx_sched.send(GuiEvent::ScheduledListUpdated);
                            ctx_sched.request_repaint();
                            }).await;
                        }
                    });
                }
                {
                    let s_kw = session.clone();
                    let gui_tx_kw = gui_tx.clone();
                    let ctx_kw = egui_ctx.clone();
                    crate::runtime_scope::spawn(async move {
                        let s = s_kw.lock().await.clone();
                        if s.login.is_some() {
                            if let Some(tx) = WORKER_TX.get() {
                                let _ = tx.send(WorkerCmd::RefreshThreads);
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
                        let mut ws_generation = 0;
                        let mut ws_account = String::new();
                        let mut ws_commands = None;
                        let mut phone = super::phone_history::PhoneCoordinator::default();
                        let mut group = super::group_history::GroupHistorySync::default();
                        loop {
                            let scope = crate::runtime_scope::registry().current();
                            if ws_generation != scope.generation {
                                phone.reset();
                                group = super::group_history::GroupHistorySync::default();
                                ws_commands = None;
                                ws_account.clear();
                                ws_rx = None;
                                ws_sek.clear();
                                ws_generation = scope.generation;
                            }
                            if scope.account.is_empty() { tokio::time::sleep(Duration::from_millis(100)).await; continue; }
                            crate::runtime_scope::run(scope, async {
                            let s_snap = s_ws.lock().await.clone();
                            if s_snap.login.is_some() {
                                let cur_sek = s_snap.zpw_sek.clone().unwrap_or_default();
                                let account = s_snap
                                    .login
                                    .as_ref()
                                    .map(|login| login.uin.clone())
                                    .unwrap_or_default();
                                if !cur_sek.is_empty() && (ws_rx.is_none() || ws_sek != cur_sek || ws_account != account) {
                                    phone.reset_for_session(&gui_tx_ws, &ctx_ws, &ws_account);
                                    group = super::group_history::GroupHistorySync::default();
                                    ws_sek = cur_sek;
                                    ws_account = account;
                                    let (events, commands) = crate::socket::spawn_with_history(s_snap.clone());
                                    ws_rx = Some(events);
                                    ws_commands = Some(commands);
                                }
                                while let Ok((ticket, action)) = phone_rx.try_recv() {
                                    if !crate::runtime_scope::registry().accepts(&ticket) || ticket.account != ws_account { continue; }
                                    phone.control(ticket, s_ws.clone(), action, &s_snap, ws_commands.as_ref(), &gui_tx_ws, &ctx_ws);
                                }
                                let mut requested = false;
                                while let Ok(ticket) = group_rx.try_recv() {
                                    if crate::runtime_scope::registry().accepts(&ticket) && ticket.account == ws_account { requested = true; }
                                }
                                if let Some(status) = group.check_timeout(std::time::Instant::now()) {
                                    let _ = gui_tx_ws.send(GuiEvent::GroupHistoryStatus(status));
                                }
                                if requested {
                                    let request = group.request_batch(std::time::Instant::now());
                                    dispatch_group_history_request(&mut group, ws_commands.as_ref(), request, &gui_tx_ws);
                                }
                                if let Some(rx) = ws_rx.as_mut() {
                                    match tokio::time::timeout(
                                        Duration::from_millis(1500),
                                        rx.recv(),
                                    )
                                    .await
                                    {
                                        Ok(Some(ev)) => {
                                            let current = s_ws.lock().await;
                                            if current.login.as_ref().map(|login| login.uin.as_str()) != Some(ws_account.as_str())
                                                || current.zpw_sek.as_deref() != Some(ws_sek.as_str())
                                            {
                                                return;
                                            }
                                            drop(current);
                                            phone.forward(&ev);
                                            let group_page = ev.cmd == crate::socket::CMD_PULL_MSG_GROUP && ev.sub_cmd == 1;
                                            if group_page {
                                                match super::group_history::GroupHistoryPage::parse(&ev.payload) {
                                                    Ok(page) => {
                                                        let account = s_snap
                                                            .login
                                                            .as_ref()
                                                            .map(|login| login.uin.as_str())
                                                            .unwrap_or_default();
                                                        let mut threads = Vec::new();
                                                        crate::chat::merge_previews(&mut threads, &page.normalized_payload(account));
                                                        if !threads.is_empty() {
                                                            let _ = gui_tx_ws.send(GuiEvent::GroupHistoryThreads {
                                                                account: account.into(),
                                                                threads,
                                                            });
                                                        }
                                                        let update = group.accept_page(&page, std::time::Instant::now());
                                                        let _ = gui_tx_ws.send(GuiEvent::GroupHistoryStatus(update.status));
                                                        dispatch_group_history_request(
                                                            &mut group,
                                                            ws_commands.as_ref(),
                                                            update.next_request,
                                                            &gui_tx_ws,
                                                        );
                                                    }
                                                    Err(_) => {
                                                        group.fail();
                                                        let _ = gui_tx_ws.send(GuiEvent::GroupHistoryStatus(
                                                            "Lịch sử nhóm trả phản hồi không hợp lệ".into(),
                                                        ));
                                                    }
                                                }
                                            }
                                            let data_node = ev
                                                .payload
                                                .get("data")
                                                .unwrap_or(&ev.payload);
                                            for voip_ev in
                                                crate::call::extract_voip_controls(data_node)
                                            {
                                                cb.handle_control_event(voip_ev);
                                            }
                                            let account = s_snap.login.as_ref().map(|l| l.uin.as_str()).unwrap_or_default();
                                            let reactions = crate::reaction_journal::socket_events(ev.cmd, &ev.payload, account);
                                            if !reactions.is_empty() {
                                                process_reaction_journal(s_ws.clone(), s_snap.clone(), gui_tx_ws.clone(), reactions, Vec::new()).await;
                                            }
                                            dispatch_delivery_receipts(&s_ws, &s_snap, &gui_tx_ws, ev.cmd, ev.sub_cmd, &ev.payload).await;
                                            if ev.is_chat_ish() && !group_page {
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
                                                        });
                                                    }
                                                }
                                                crate::chat::merge_previews(
                                                    &mut ws_threads,
                                                    &ev.payload,
                                                );
                                                ws_threads.retain(|t| !t.messages.is_empty());
                                                if !ws_threads.is_empty() {
                                                    crate::chat::resolve_thread_names(
                                                        &s_snap,
                                                        &mut ws_threads,
                                                    )
                                                    .await;
                                                    publish_account_threads(s_ws.clone(), s_snap.clone(), gui_tx_ws.clone(), ws_threads, true).await;
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
                                phone.reset_for_session(&gui_tx_ws, &ctx_ws, &ws_account);
                                ws_commands = None;
                                ws_account.clear();
                                ws_rx = None;
                                ws_sek.clear();
                                tokio::time::sleep(Duration::from_secs(3)).await;
                            }
                            }).await;
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
                            let scope = crate::runtime_scope::registry().current();
                            if scope.account.is_empty() { tokio::time::sleep(Duration::from_millis(100)).await; continue; }
                            crate::runtime_scope::run(scope, async {
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
                            }).await;
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

                let mut current_qr_task: Option<tokio::task::JoinHandle<()>> = None;

                while let Some((generation, cmd)) = cmd_rx.recv().await {
                    let lifecycle = matches!(cmd, WorkerCmd::Shutdown | WorkerCmd::Logout | WorkerCmd::WipeLocalData { .. } | WorkerCmd::RequestQr | WorkerCmd::LoginPassword { .. });
                    let application = matches!(cmd, WorkerCmd::ReadNotificationAuthorization { .. } | WorkerCmd::RequestNotificationAuthorization { .. } | WorkerCmd::CheckForUpdates { .. } | WorkerCmd::DownloadAndApplyUpdate { .. });
                    let command_scope = crate::runtime_scope::registry().current();
                    if !application && generation != command_scope.generation { continue; }
                    if !lifecycle && !application && command_scope.account.is_empty() { continue; }
                    match cmd {
                        command @ WorkerCmd::AcknowledgeVisibleRead { .. } => {
                            let captured = session.lock().await.clone();
                            let session = session.clone();
                            let sender = gui_tx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let event = process_visible_read_at(history::root(), session, captured, command).await;
                                let _ = sender.send(event);
                            });
                        }
                        WorkerCmd::ImportOriginalReactions { account, path } => {
                            let captured = session.lock().await.clone();
                            let session = session.clone();
                            let sender = gui_tx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let pick_file = path.is_none();
                                process_original_reaction_import(std::path::PathBuf::from("."), session, captured, sender, account, path, pick_file).await;
                            });
                        }
                        WorkerCmd::ReplayReactions { account, threads } => {
                            let captured = session.lock().await.clone();
                            if captured.login.as_ref().map(|l| l.uin.as_str()) == Some(account.as_str()) {
                                let session = session.clone();
                                let sender = gui_tx.clone();
                                crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                    process_reaction_journal(session, captured, sender, Vec::new(), threads).await;
                                });
                            }
                        }
                        WorkerCmd::ImportHistory { account, revisions, path } => {
                            let tx = gui_tx.clone();
                            let ctx = egui_ctx.clone();
                            let captured = session.lock().await.clone();
                            let session = session.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let result = match account_history(session, captured, account.clone(), history::Request::Import(path)).await {
                                    Ok(history::Reply::Imported(report, threads)) => Ok((report, threads)),
                                    _ => Err("Chưa nhập được lịch sử theo tài khoản".into()),
                                };
                                let _ = tx.send(GuiEvent::HistoryImported { account, revisions, result });
                                ctx.request_repaint();
                            });
                        }
                        WorkerCmd::OpenNativePhoto { key, url } => {
                            let tx = gui_tx.clone();
                            static VIEWERS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> = std::sync::OnceLock::new();
                            let permit = VIEWERS.get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(1))).clone().try_acquire_owned();
                            let Ok(permit) = permit else {
                                let _ = tx.send(GuiEvent::NativePhotoViewerFinished { account: key.account, result: Err(crate::media_resources::MediaError::Cancelled) });
                                continue;
                            };
                            let snapshot = session.lock().await.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let _permit = permit;
                                let account = snapshot.login.as_ref().map(|info| info.uin.as_str()).unwrap_or_default();
                                let result = super::native_photo_viewer::open(&snapshot.http, account, key.clone(), &url).await;
                                let _ = tx.send(GuiEvent::NativePhotoViewerFinished { account: key.account, result });
                            });
                        }
                        WorkerCmd::FetchPhotoPreview { key, thumbnail, source } => {
                            let tx = gui_tx.clone();
                            let ctx = egui_ctx.clone();
                            let snapshot = session.lock().await.clone();
                            static JOBS: std::sync::OnceLock<crate::media_resources::MediaJobQueue> = std::sync::OnceLock::new();
                            let admission = JOBS.get_or_init(Default::default).try_admit();
                            let Some(admission) = admission else {
                                let texture = format!("photo_{}", key.cache_id());
                                let _ = tx.send(GuiEvent::PhotoPreviewReady { account: key.account, key: texture, result: Err(crate::media_resources::MediaError::Cancelled) });
                                continue;
                            };
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let permit = admission.start().await;
                                let result = if snapshot.login.as_ref().map(|info| info.uin.as_str()) != Some(key.account.as_str()) { Err(crate::media_resources::MediaError::AccountMismatch) }
                                else if let Some(_permit) = permit {
                                    let generation = crate::media_resources::RequestGeneration::default();
                                    super::media_preview::load_photo(&snapshot.http, &key, &thumbnail, source.as_deref(), &generation, generation.capture()).await.map(|image| {
                                        egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw())
                                    })
                                } else { Err(crate::media_resources::MediaError::Cancelled) };
                                let texture = format!("photo_{}", key.cache_id());
                                let _ = tx.send(GuiEvent::PhotoPreviewReady { account: key.account, key: texture, result });
                                ctx.request_repaint();
                            });
                        }
                        WorkerCmd::DeliverIncomingNotification { account, thread, message, sound } => {
                            let tx = gui_tx.clone();
                            let ctx = egui_ctx.clone();
                            if session.lock().await.login.as_ref().map(|info| info.uin.as_str()) == Some(account.as_str()) {
                                crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                    let result = crate::native_notifications::deliver(&account, &thread, &message, sound).await;
                                    let _ = tx.send(GuiEvent::IncomingNotificationFinished { result });
                                    ctx.request_repaint();
                                });
                            } else {
                                let _ = tx.send(GuiEvent::IncomingNotificationFinished { result: Err(crate::native_notifications::NotificationError::HelperUnavailable) });
                            }
                        }
                        WorkerCmd::Shutdown => {
                            let barrier = tokio::task::spawn_blocking(move || crate::runtime_scope::transition_if_current(generation, "", || {}));
                            let result = async {
                                let shutdown_scope = barrier.await.map_err(|_| "Dừng ghi dữ liệu bị gián đoạn".to_owned())?
                                    .map_err(|_| "Phiên làm việc đã thay đổi".to_owned())?;
                                crate::runtime_scope::drain().await;
                                call_bridge.shutdown().await.map_err(|_| "Chưa dừng xong âm thanh cuộc gọi".to_owned())?;
                                if !crate::runtime_scope::registry().accepts(&shutdown_scope) { return Err("Đã có thao tác mới; yêu cầu đóng cũ đã được huỷ".to_owned()); }
                                Ok(())
                            };
                            match tokio::time::timeout(Duration::from_secs(10), result).await {
                                Ok(Ok(())) => {
                                    let _ = gui_tx.send(GuiEvent::ShutdownFinished(Ok(())));
                                    break;
                                }
                                Ok(Err(error)) => { let _ = gui_tx.send(GuiEvent::ShutdownFinished(Err(error))); }
                                Err(_) => { let _ = gui_tx.send(GuiEvent::ShutdownFinished(Err("Đóng ứng dụng quá thời gian; hãy thử lại".into()))); }
                            }
                        }
                        WorkerCmd::ReadNotificationAuthorization { request_id }
                        | WorkerCmd::RequestNotificationAuthorization { request_id } => {
                            let request = matches!(
                                cmd, WorkerCmd::RequestNotificationAuthorization { .. }
                            );
                            let tx = gui_tx.clone();
                            tokio::spawn(async move {
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
                        WorkerCmd::PersistThreads { account, threads } => {
                            let captured = session.lock().await.clone();
                            let session = session.clone();
                            let sender = gui_tx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let result = account_history(session, captured, account, history::Request::Persist(threads)).await;
                                if result.is_err() { let _ = sender.send(GuiEvent::StatusUpdate("Chưa lưu được lịch sử theo tài khoản".into())); }
                            });
                        }
                        WorkerCmd::LoadHistoryPage { account, thread_id, revision, before } => {
                            let captured = session.lock().await.clone();
                            let session = session.clone();
                            let sender = gui_tx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let result = account_history(session, captured, account.clone(), history::Request::Page { thread: thread_id.clone(), before }).await;
                                let result = match result {
                                    Ok(history::Reply::Page { messages, has_more }) => Ok((messages, has_more)),
                                    _ => Err("Chưa tải được trang lịch sử".into()),
                                };
                                let _ = sender.send(GuiEvent::HistoryPageLoaded { account, thread_id, revision, result });
                            });
                        }
                        WorkerCmd::NavigateHistory { account, thread_id, reference, request_id, revision } => {
                            let captured = session.lock().await.clone();
                            let session = session.clone();
                            let sender = gui_tx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let result = account_history(session, captured, account.clone(), history::Request::Navigate { thread: thread_id.clone(), reference: reference.clone() }).await;
                                let result = match result {
                                    Ok(history::Reply::Navigation(page)) => Ok(page),
                                    _ => Err("Chưa tra được tin nhắn gốc".into()),
                                };
                                let _ = sender.send(GuiEvent::HistoryNavigationLoaded { account, thread_id, reference, request_id, revision, result });
                            });
                        }
                        WorkerCmd::SearchHistory { account, thread_id, query, revision } => {
                            let captured = session.lock().await.clone();
                            let session = session.clone();
                            let sender = gui_tx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let result = account_history(session, captured, account.clone(), history::Request::Search { thread: thread_id.clone(), query: query.clone() }).await;
                                let result = match result {
                                    Ok(history::Reply::Page { messages, .. }) => Ok(messages),
                                    _ => Err("Chưa tìm được lịch sử".into()),
                                };
                                let _ = sender.send(GuiEvent::HistorySearchLoaded { account, thread_id, query, revision, result });
                            });
                        }
                        WorkerCmd::Logout => {
                            crate::history_sync::CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
                            if let Some(task) = current_qr_task.take() { task.abort(); }
                            let Ok(Ok(logout_scope)) = tokio::task::spawn_blocking(move || {
                                crate::runtime_scope::transition_if_current(generation, "", || {
                                    let _ = std::fs::remove_file("session.json");
                                })
                            }).await else { continue; };
                            session.lock().await.logout();
                            *poll_state.lock().await = crate::chat::PollState::default();
                            let _ = gui_tx.send(GuiEvent::Scoped {
                                scope: logout_scope.clone(), event: Box::new(GuiEvent::LoggedOut),
                            });
                            egui_ctx.request_repaint();
                            // QR begins only after logout cleanup; it carries a fresh validated intent.
                            if let Some(tx) = WORKER_TX.get() { let _ = tx.send_lifecycle_from(logout_scope.generation, WorkerCmd::RequestQr); }
                        }
                        WorkerCmd::WipeLocalData { account } => {
                            let captured = session.lock().await.clone();
                            if captured.login.as_ref().map(|login| login.uin.as_str()) != Some(account.as_str()) { continue; }
                            let transition_account = account.clone();
                            let Ok(Ok(wipe_scope)) = tokio::task::spawn_blocking(move || {
                                crate::runtime_scope::transition_if_current(generation, &transition_account, || {})
                            }).await else { continue; };
                            let session = session.clone();
                            let sender = gui_tx.clone();
                            crate::runtime_scope::spawn_scoped(wipe_scope, async move {
                                match account_history(session, captured, account, history::Request::Clear).await {
                                    Ok(_) => { let _ = sender.send(GuiEvent::LocalDataWiped); }
                                    Err(error) => { let _ = sender.send(GuiEvent::HistoryClearFailed(error)); }
                                }
                            });
                            egui_ctx.request_repaint();
                        }
                        WorkerCmd::PinThread {
                            thread_id,
                            is_group,
                            pin,
                        } => {
                            let session_clone = session.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            let session_clone = session.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            let session_clone = session.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let s = session_clone.lock().await.clone();
                                let _ = crate::chat::set_mute_conversation(
                                    &s,
                                    &thread_id,
                                    is_group,
                                    duration_sec,
                                    mute,
                                )
                                .await;
                            });
                        }
                        WorkerCmd::HideThread {
                            thread_id,
                            is_group,
                            hide,
                        } => {
                            let session_clone = session.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let s = session_clone.lock().await.clone();
                                let _ = crate::chat::set_hidden_conversation(
                                    &s, &thread_id, is_group, hide,
                                )
                                .await;
                            });
                        }
                        WorkerCmd::DeleteThread {
                            thread_id,
                            is_group,
                            last_cli_msg_id,
                        } => {
                            let session_clone = session.clone();
                            let sender = gui_tx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let s = session_clone.lock().await.clone();
                                let account = s.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
                                let result = match crate::chat::delete_conversation(&s, &thread_id, is_group, &last_cli_msg_id).await {
                                    Ok(response) if resp_error_code(&response).is_none() => {
                                        account_history(session_clone, s, account.clone(), history::Request::DeleteThread(thread_id.clone())).await
                                            .map(|_| ())
                                    }
                                    _ => Err("Chưa xóa được cuộc trò chuyện trên máy chủ".into()),
                                };
                                let _ = sender.send(GuiEvent::HistoryThreadDeleted { account, thread_id, result });
                            });
                        }
                        WorkerCmd::RequestQr => {
                            let Ok(Ok(attempt_scope)) = tokio::task::spawn_blocking(move || crate::runtime_scope::transition_if_current(generation, "", || {})).await else { continue; };
                            let _ = gui_tx.send(GuiEvent::Scoped {
                                scope: attempt_scope.clone(), event: Box::new(GuiEvent::LoggedOut),
                            });
                            if let Some(task) = current_qr_task.take() {
                                task.abort();
                            }
                            let session_clone = session.clone();
                            let poll_clone = poll_state.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();

                            let handle = crate::runtime_scope::spawn_scoped(attempt_scope, async move {
                                {
                                    let mut s = session_clone.lock().await;
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
                                                        let mut s = session_clone.lock().await.clone();
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
                                                        {
                                                            let mut shared = session_clone.lock().await;
                                                            let account = s.login.as_ref().map(|login| login.uin.as_str()).unwrap_or("");
                                                            if crate::runtime_scope::transition(account).is_err() { return; }
                                                            *shared = s.clone();
                                                            let _ = s.save_to_file("session.json");
                                                        }

                                                        let snapshot = s.clone();
                                                        drop(s);
                                                        let s = snapshot;
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
                                                        let _ = gui_tx_clone.send(
                                                            GuiEvent::LoginSuccess {
                                                                name: dname,
                                                                uin,
                                                                avatar: u_ava,
                                                            },
                                                        );
                                                        ctx_clone.request_repaint();

                                                        let mut list = crate::chat::get_friends_network(&s)
                                                            .await
                                                            .unwrap_or_default();
                                                        list = load_account_threads(session_clone.clone(), s.clone(), list).await;
                                                        {
                                                            let mut ps = poll_clone.lock().await;
                                                            let _ = crate::chat::poll_new_messages_network(
                                                                &s, &mut ps, &mut list,
                                                            )
                                                            .await;
                                                        }
                                                        publish_account_threads(session_clone.clone(), s.clone(), gui_tx_clone.clone(), list, false).await;
                                                        ctx_clone.request_repaint();

                                                        let s_kw = s.clone();
                                                        let tx_kw = gui_tx_clone.clone();
                                                        let ctx_kw = ctx_clone.clone();
                                                        crate::runtime_scope::spawn(async move {
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
                            let session_clone = session.clone();
                            let poll_clone = poll_state.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let mut s = session_clone.lock().await.clone();
                                if s.login.is_some() {
                                    match s.session_status().await {
                                        crate::session::SessionStatus::Valid => {}
                                        crate::session::SessionStatus::Expired => {
                                            // Keep `session.json`: the old credentials stay
                                            // readable until a new login replaces the file (#21).
                                            {
                                                let mut sl = session_clone.lock().await;
                                                let Ok(_scope_guard) = crate::runtime_scope::mutation() else { return; };
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
                                                let _ = tx.send(WorkerCmd::RequestQr);
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
                                        {
                                            let mut sl = session_clone.lock().await;
                                            if crate::runtime_scope::transition(&uid).is_err() { return; }
                                            if let Some(ref mut li) = sl.login {
                                                li.uin = uid.clone();
                                                if !name.is_empty() {
                                                    li.display_name = name.clone();
                                                }
                                            }
                                            let _ = sl.save_to_file("session.json");
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
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                        "⏳ Đang làm mới danh bạ & nhóm từ máy chủ Zalo (1/3)…".into(),
                                    ));
                                    ctx_clone.request_repaint();
                                    let mut list: Vec<Thread> = Vec::new();
                                    let mut errors = Vec::new();
                                    match crate::chat::get_friends_network(&s).await {
                                        Ok(friends) => {
                                            for ft in friends {
                                                if let Some(ex) = list.iter_mut().find(|t| t.id == ft.id) {
                                                    if !ft.name.is_empty() {
                                                        ex.name = ft.name;
                                                    }
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
                                    list = load_account_threads(session_clone.clone(), s.clone(), list).await;
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                        format!("⏳ Đang làm mới tin nhắn cho {} hội thoại (3/3)…", list.len()),
                                    ));
                                    ctx_clone.request_repaint();
                                    let mut fresh_ps = crate::chat::PollState::default();
                                    if let Err(e) =
                                        crate::chat::poll_new_messages_network(&s, &mut fresh_ps, &mut list)
                                            .await
                                    {
                                        crate::logger::log_error("gui::refresh_poll", &e.to_string());
                                        errors.push(format!("Tin nhắn: {e}"));
                                    }
                                    {
                                        let mut ps = poll_clone.lock().await;
                                        *ps = fresh_ps;
                                    }
                                    let total_msgs: usize =
                                        list.iter().map(|t| t.messages.len()).sum();
                                    let n_threads = list.len();
                                    publish_account_threads(session_clone.clone(), s.clone(), gui_tx_clone.clone(), list, false).await;
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

                            let session_clone = session.clone();
                            let poll_clone = poll_state.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            let reset_poll = crate::runtime_scope::ResetFlag(&IS_POLLING);
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let _reset_poll = reset_poll;
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
                                                dispatch_delivery_receipts(&session_clone, &s, &gui_tx_clone, crate::socket::CMD_PULL_DELIVERIES, 0, &lp_resp).await;
                                                let account = s.login.as_ref().map(|l| l.uin.as_str()).unwrap_or_default();
                                                let reactions = crate::reaction_journal::account_events(&lp_resp, account);
                                                if !reactions.is_empty() {
                                                    process_reaction_journal(session_clone.clone(), s.clone(), gui_tx_clone.clone(), reactions, Vec::new()).await;
                                                }
                                                crate::chat::merge_previews(
                                                    &mut temp_threads,
                                                    &lp_resp,
                                                );
                                                let data_node =
                                                    lp_resp.get("data").unwrap_or(&lp_resp);
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
                                                    let mut sl = session_clone.lock().await;
                                                let Ok(_scope_guard) = crate::runtime_scope::mutation() else { return; };
                                                    sl.mark_expired();
                                                    let _ = sl.save_to_file("session.json");
                                                    sl.logout();
                                                }
                                                let _ = gui_tx_clone.send(GuiEvent::SessionExpired(
                                                    "Phiên đăng nhập (zpw_sek) đã hết hạn".into(),
                                                ));
                                                ctx_clone.request_repaint();
                                                if let Some(tx) = WORKER_TX.get() {
                                                    let _ = tx.send(WorkerCmd::RequestQr);
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
                                                crate::chat::resolve_thread_names(
                                                    &s,
                                                    &mut temp_threads,
                                                )
                                                .await;
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
                                                publish_account_threads(session_clone.clone(), s.clone(), gui_tx_clone.clone(), temp_threads, true).await;
                                                ctx_clone.request_repaint();
                                            }
                                        }
                                    }
                                }
                            });
                        }
                        WorkerCmd::PhoneHistory(action) => { let _ = phone_tx.send((command_scope.clone(), action)); }
                        WorkerCmd::LoadGroupHistoryStream => { let _ = group_tx.send(command_scope.clone()); }
                        WorkerCmd::SyncHistory { threads } => {
                            use std::sync::atomic::Ordering;
                            if crate::history_sync::RUNNING.swap(true, Ordering::SeqCst) {
                                continue;
                            }
                            crate::history_sync::CANCEL.store(false, Ordering::Relaxed);
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            let reset_sync = crate::runtime_scope::ResetFlag(&crate::history_sync::RUNNING);
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let _reset_sync = reset_sync;
                                let s = session_clone.lock().await.clone();
                                let total = threads.len();
                                let mut added = 0;
                                let mut errors = Vec::new();
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
                                    let account = s.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
                                    let cursor = match account_history(session_clone.clone(), s.clone(), account.clone(), history::Request::OldestCursor(t.id.clone())).await {
                                        Ok(history::Reply::Cursor(cursor)) => cursor,
                                        _ => { errors.push(format!("{}: chưa đọc được con trỏ lịch sử", t.name)); continue; }
                                    };
                                    let storage_session = session_clone.clone();
                                    let storage_capture = s.clone();
                                    match crate::history_sync::sync_thread_network(&s, &mut t, cursor, move |snapshot| {
                                        let session = storage_session.clone();
                                        let captured = storage_capture.clone();
                                        let account = account.clone();
                                        async move {
                                            match account_history(session, captured, account, history::Request::Persist(vec![snapshot])).await {
                                                Ok(history::Reply::Written(inserted)) => Ok(inserted),
                                                _ => anyhow::bail!("Account history write failed"),
                                            }
                                        }
                                    }).await {
                                        Ok(n) => added += n,
                                        Err(e) => errors.push(format!("{}: {e}", t.name)),
                                    }
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                                    };
                                    let mut t = t;
                                    let _ = have; // Local archive paging uses an exact cursor, not a growing LIMIT.
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
                                        }
                                        // Backfill up to 5 pages (250 messages) of older cloud history
                                        let mut cursor_id = t
                                            .messages
                                            .first()
                                            .map(|m| m.id.as_str())
                                            .filter(|id| {
                                                !id.is_empty()
                                                    && id.chars().all(|c| c.is_ascii_digit())
                                            })
                                            .unwrap_or("9223372036854775808")
                                            .to_string();
                                        for page in 1..=5 {
                                            let prev_len = t.messages.len();
                                            let Ok(cm_old) = crate::chat::get_cloud_messages(
                                                &s,
                                                &group_id,
                                                &cursor_id,
                                                true,
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
                                            let data_node = cm_old
                                                .pointer("/data/data")
                                                .or_else(|| cm_old.get("data"))
                                                .unwrap_or(&cm_old);
                                            let has_more = data_node
                                                .get("hasMore")
                                                .and_then(|v| v.as_i64())
                                                .unwrap_or(0);
                                            let next_cursor = data_node
                                                .get("lastMsgId")
                                                .and_then(|v| {
                                                    v.as_str().map(str::to_string).or_else(|| {
                                                        v.as_u64().map(|n| n.to_string())
                                                    })
                                                })
                                                .filter(|s| {
                                                    !s.is_empty()
                                                        && s != "0"
                                                        && s.chars().all(|c| c.is_ascii_digit())
                                                })
                                                .or_else(|| {
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
                                            if has_more == 0 || nc == cursor_id || new_len == prev_len {
                                                break;
                                            }
                                            cursor_id = nc;
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
                                        publish_account_threads(session_clone.clone(), s.clone(), gui_tx_clone.clone(), vec![t], false).await;
                                    }
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                        "✅ Đã đồng bộ {cnt} tin nhắn lịch sử"
                                    )));
                                    ctx_clone.request_repaint();
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
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                                                let mut sl = session_clone.lock().await;
                                                let Ok(_scope_guard) = crate::runtime_scope::mutation() else { return; };
                                                sl.mark_expired();
                                                let _ = sl.save_to_file("session.json");
                                                sl.logout();
                                            }
                                            let _ = gui_tx_clone.send(GuiEvent::SessionExpired(
                                                "Không thể gửi vì phiên zpw_sek đã hết hạn".into(),
                                            ));
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::RequestQr);
                                            }
                                        }
                                        ctx_clone.request_repaint();
                                    }
                                }
                            });
                        }
                        WorkerCmd::SendWithMentions { to_id, text, mentions, quoted } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let s = session_clone.lock().await.clone();
                                let client_id = crate::chat::next_client_message_id();
                                // Keep raw text/offsets intact instead of rewriting a URL into link-card params.
                                let result = if let Some(quote) = &quoted {
                                    crate::chat::send_quote_message_with_mentions_and_client_id(
                                        &s, &to_id, &text, quote, true, client_id, &mentions).await
                                } else {
                                    crate::chat::send_group_message_with_mentions_and_client_id(
                                        &s, &to_id, &text, client_id, &mentions).await
                                };
                                let account = s.login.as_ref().map(|login| login.uin.clone()).unwrap_or_default();
                                let event = match result {
                                    Ok(response) => {
                                        let msg_id = extract_msg_id(&response);
                                        let msg = composed_sent_message(&text, &mentions, quoted.as_ref(), client_id, msg_id);
                                        GuiEvent::ComposerMessageSent { account, thread_id: to_id, msg }
                                    }
                                    Err(error) => GuiEvent::ComposerSendFailed { account, thread_id: to_id, text, mentions, quoted, error: error.to_string() },
                                };
                                // GuiTx wakes the UI; a closed receiver means the app has exited.
                                let _ = gui_tx_clone.send(event);
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
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                        } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                                                thread_id: target_id,
                                                msg_id,
                                                sticker_id: sid,
                                                cate_id: msg.sticker_cate_id.unwrap_or(0),
                                                sticker_type: msg.sticker_type.unwrap_or(7),
                                            });
                                        } else {
                                            let _ = gui_tx_clone.send(GuiEvent::MessageSent {
                                                client_message_id: client_id.to_string(),
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
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                                        let account = s.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
                                        if account_history(session_clone.clone(), s.clone(), account, history::Request::Delete { thread: to_id.clone(), message: msg.id.clone() }).await.is_err() {
                                            let _ = gui_tx_clone.send(GuiEvent::StatusUpdate("Chưa lưu được thao tác xóa tin nhắn".into()));
                                            return;
                                        }
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
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                                                let mut sl = session_clone.lock().await;
                                                let Ok(_scope_guard) = crate::runtime_scope::mutation() else { return; };
                                                sl.mark_expired();
                                                let _ = sl.save_to_file("session.json");
                                                sl.logout();
                                            }
                                            let _ = gui_tx_clone.send(GuiEvent::SessionExpired(
                                                "Không thể gửi nhãn dán vì phiên zpw_sek đã hết hạn"
                                                    .into(),
                                            ));
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::RequestQr);
                                            }
                                        }
                                        ctx_clone.request_repaint();
                                    }
                                }
                            });
                        }
                        WorkerCmd::RenderInlineEmoji { cluster, sprite_id } => {
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), render_inline_emoji(cluster, sprite_id, gui_tx.clone()));
                        }
                        WorkerCmd::FetchStickerImage { key, url } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let cache_dir = std::path::Path::new("stickers_cache");
                                if let Ok(_scope_guard) = crate::runtime_scope::mutation() {
                                    let _ = std::fs::create_dir_all(cache_dir);
                                }
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
                                        (std::fs::read(&url).ok(), false)
                                    } else {
                                        let client = { session_clone.lock().await.clone().http };
                                        let body = match client.get(&url).send().await {
                                            Ok(resp) if resp.status().is_success() => {
                                                resp.bytes().await.ok()
                                            }
                                            _ => None,
                                        };
                                        (body.map(|b| b.to_vec()), true)
                                    };
                                let Some(bytes) = bytes_opt else {
                                    return;
                                };

                                let _ = crate::runtime_scope::spawn_blocking(move || {
                                    let Ok(mut dyn_img) = image::load_from_memory(&bytes) else {
                                        if !from_network {
                                            // Corrupt cache entry: drop it so the next request refetches.
                                            if let Ok(_scope_guard) = crate::runtime_scope::mutation() {
                                                let _ = std::fs::remove_file(&cache_path);
                                            }
                                        }
                                        return;
                                    };
                                    if from_network {
                                        if let Ok(_scope_guard) = crate::runtime_scope::mutation() {
                                            let _ = std::fs::write(&cache_path, &bytes);
                                        }
                                    }
                                    // Bubbles draw images at most 240 pt wide; full-size photos kept as
                                    // textures cost ~8 MB each (1080p) and are never freed.
                                    // The viewer's `hd_` textures stay full size.
                                    if !key.starts_with("ava_")
                                        && !key.starts_with("hd_")
                                        && (dyn_img.width() > 480 || dyn_img.height() > 480)
                                    {
                                        dyn_img = dyn_img.thumbnail(480, 480);
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let s = session_clone.lock().await.clone();
                                if let Ok(meta) = crate::chat::parse_link_metadata(&s, &url).await {
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
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let picked_path = if let Some(p) = preset_path {
                                    Some(p)
                                } else {
                                    crate::runtime_scope::spawn_blocking(move || {
                                        pick_native_file_dialog(images_only)
                                    })
                                    .await
                                    .ok()
                                    .and_then(Result::ok)
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
                            crate::runtime_scope::run(command_scope.clone(), async {
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
                            }).await;
                        }
                        WorkerCmd::AnswerCall { accept } => {
                            crate::runtime_scope::run(command_scope.clone(), async {
                            call_bridge.answer_call(accept);
                            egui_ctx.request_repaint();
                            }).await;
                        }
                        WorkerCmd::SetCallMediaState {
                            is_on_mic,
                            is_on_cam,
                        } => {
                            crate::runtime_scope::run(command_scope.clone(), async {
                            call_bridge.set_media_state(is_on_mic, is_on_cam);
                            egui_ctx.request_repaint();
                            }).await;
                        }
                        WorkerCmd::EndCall => {
                            crate::runtime_scope::run(command_scope.clone(), async {
                            call_bridge.end_call();
                            let _ = gui_tx.send(GuiEvent::CallStateChanged("free".into()));
                            let _ = gui_tx.send(GuiEvent::StatusUpdate(
                                "Đã kết thúc cuộc gọi".into(),
                            ));
                            egui_ctx.request_repaint();
                            }).await;
                        }
                        WorkerCmd::CheckFriendStatus { user_id } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            let Ok(Ok(attempt_scope)) = tokio::task::spawn_blocking(move || crate::runtime_scope::transition_if_current(generation, "", || {})).await else { continue; };
                            let _ = gui_tx.send(GuiEvent::Scoped {
                                scope: attempt_scope.clone(), event: Box::new(GuiEvent::AuthenticationStarted),
                            });
                            let session_clone = session.clone();
                            let poll_clone = poll_state.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(attempt_scope, async move {
                                let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                    "Đang xác thực tài khoản…".into(),
                                ));
                                ctx_clone.request_repaint();
                                let mut s = session_clone.lock().await.clone();
                                match crate::login::login_by_password(&mut s, &phone, &pass, "VN")
                                    .await
                                {
                                    Ok(_) => {
                                        if let Ok(info) = crate::login::get_login_info(&s).await {
                                            if let Some(cfg) = info.get("data") {
                                                crate::login::apply_login_config(&mut s, cfg);
                                                s.apply_server_info(cfg);
                                                {
                                                    let mut shared = session_clone.lock().await;
                                                    let account = s.login.as_ref().map(|login| login.uin.as_str()).unwrap_or("");
                                                    if crate::runtime_scope::transition(account).is_err() { return; }
                                                    *shared = s.clone();
                                                    let _ = s.save_to_file("session.json");
                                                }

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
                                                    crate::chat::get_friends_network(&s).await
                                                {
                                                    {
                                                        let mut ps = poll_clone.lock().await;
                                                        *ps = crate::chat::PollState::default();
                                                        let _ = crate::chat::poll_new_messages_network(
                                                            &s, &mut ps, &mut list,
                                                        )
                                                        .await;
                                                    }
                                                    publish_account_threads(session_clone.clone(), s.clone(), gui_tx_clone.clone(), list, false).await;
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
                        WorkerCmd::CheckForUpdates { silent } => {
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
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
                            tokio::spawn(async move {
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
                        WorkerCmd::FetchSocialProfile { target_uid, request_id } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let s = session_clone.lock().await.clone();
                                let res = s
                                    .get_friend_social_profile(&target_uid)
                                    .await
                                    .map_err(|e| e.to_string());
                                let _ = gui_tx_clone.send(GuiEvent::SocialProfileLoaded { target_uid, request_id, result: res });
                                ctx_clone.request_repaint();
                            });
                        }
                        WorkerCmd::FetchMySocialAvatars { page, count } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
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
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            crate::runtime_scope::spawn_scoped(command_scope.clone(), async move {
                                let s = session_clone.lock().await.clone();
                                let res = s
                                    .fetch_timeline_feed(&friend_uids, &shared_media_by_uid)
                                    .await
                                    .map_err(|e| e.to_string());
                                let _ = gui_tx_clone.send(GuiEvent::SocialTimelineLoaded(res));
                                ctx_clone.request_repaint();
                            });
                        }
                    }
                }
            });
            rt.shutdown_timeout(Duration::from_secs(2));
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
        let list_url =
            format!("https://api.github.com/repos/{GITHUB_REPO}/releases?per_page=5");
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
                    meta.expected_sha256 =
                        crate::updater::parse_sha256sums_for_asset(&txt, b_name);
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
    let is_portable_bin = (cfg!(target_os = "windows")
        && name_low.ends_with(".exe")
        && !name_low.contains("setup"))
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
                            if std::process::Command::new(&exe_path).args(&args).spawn().is_ok() {
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


#[cfg(test)]
mod reaction_consumer_tests {
    use super::*;

    fn session_fixture(account: &str) -> Session {
        let mut session = Session::new(false, false, false).unwrap();
        session.login = Some(crate::session::LoginInfo {
            uin: account.into(),
            ..Default::default()
        });
        session.zpw_sek = Some("fixture-session".into());
        session
    }

    fn fixture_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "zl-reaction-consumer-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn event_fixture() -> Vec<crate::reactions::ReactionEvent> {
        crate::reaction_journal::account_events(
            &serde_json::json!({"error_code":0,"data":{"reacts":[{
                "idTo":"account-a", "uidFrom":"peer-fixture", "msgId":"event-fixture", "ts":1700000000000_i64,
                "content":{"rType":5,"rMsg":[{"gMsgID":"message-fixture"}]}
            }]}}),
            "account-a",
        )
    }

    #[tokio::test]
    async fn reaction_consumer_empty_replay_never_loops_notifications() {
        let root = fixture_root("empty");
        let captured = session_fixture("account-a");
        let session = std::sync::Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let (tx, rx) = channel();
        let sender = GuiTx { scope: None, tx, ctx: egui::Context::default() };
        process_reaction_journal_at(root.clone(), session, captured, sender, Vec::new(), Vec::new()).await;
        assert!(!root.exists());
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn reaction_consumer_appends_before_notification_and_replays_after_target_load() {
        let root = fixture_root("flow");
        let captured = session_fixture("account-a");
        let session = std::sync::Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let (tx, rx) = channel();
        let sender = GuiTx {
            scope: None,
            tx,
            ctx: egui::Context::default(),
        };
        process_reaction_journal_at(
            root.clone(),
            session.clone(),
            captured.clone(),
            sender.clone(),
            event_fixture(),
            Vec::new(),
        )
        .await;
        assert!(
            matches!(rx.try_recv().unwrap(), GuiEvent::ReactionEvents { account } if account == "account-a")
        );
        let journal = crate::reaction_journal::ReactionJournal::open(&root, "account-a").unwrap();
        assert_eq!(journal.pending_count().unwrap(), 1);
        drop(journal);
        let loaded = vec![Thread {
            id: "peer-fixture".into(),
            messages: vec![ChatMessage {
                id: "message-fixture".into(),
                ..Default::default()
            }],
            ..Default::default()
        }];
        for _ in 0..2 {
            process_reaction_journal_at(
                root.clone(),
                session.clone(),
                captured.clone(),
                sender.clone(),
                Vec::new(),
                loaded.clone(),
            )
            .await;
            let GuiEvent::ReactionThreadsUpdated { account, threads } = rx.try_recv().unwrap()
            else {
                panic!("projection missing");
            };
            assert_eq!(account, "account-a");
            assert_eq!(threads[0].messages[0].reaction_summaries[0].count, 1);
        }
        let journal = crate::reaction_journal::ReactionJournal::open(&root, "account-a").unwrap();
        assert_eq!(journal.pending_count().unwrap(), 0);
        drop(journal);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn reaction_consumer_rejects_account_switch_before_any_journal_mutation() {
        let root = fixture_root("switch");
        let captured = session_fixture("account-a");
        let session = std::sync::Arc::new(tokio::sync::Mutex::new(session_fixture("account-b")));
        let (tx, rx) = channel();
        let sender = GuiTx {
            scope: None,
            tx,
            ctx: egui::Context::default(),
        };
        process_reaction_journal_at(
            root.clone(),
            session,
            captured,
            sender,
            event_fixture(),
            Vec::new(),
        )
        .await;
        assert!(!root.exists());
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn reaction_consumer_reports_disk_failure_without_false_durable_notification() {
        let root = fixture_root("failure");
        std::fs::write(&root, "fixture file prevents creating directory").unwrap();
        let captured = session_fixture("account-a");
        let session = std::sync::Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let (tx, rx) = channel();
        let sender = GuiTx {
            scope: None,
            tx,
            ctx: egui::Context::default(),
        };
        process_reaction_journal_at(
            root.clone(),
            session,
            captured,
            sender,
            event_fixture(),
            Vec::new(),
        )
        .await;
        assert!(matches!(rx.try_recv().unwrap(), GuiEvent::StatusUpdate(_)));
        assert!(rx.try_recv().is_err());
        std::fs::remove_file(root).unwrap();
    }
}


#[cfg(test)]
async fn import_history_async(path: std::path::PathBuf, database: String) -> Result<(crate::store::ImportReport, Vec<Thread>), String> {
    crate::runtime_scope::spawn_blocking(move || crate::store::import_history_delta_from_path(&path, &database).map_err(|_| "History import failed".to_owned()))
        .await.map_err(|_| "Import task failed".to_owned())?
        .map_err(|_| "Import session expired".to_owned())?
}

#[cfg(test)]
mod media_import_regressions {
    use super::*;
    #[tokio::test]
    async fn import_delta_never_resurrects_unrelated_deleted_snapshot_records() {
        for format in ["json", "sqlite"] {
            let root = std::env::temp_dir().join(format!("zl-import-delta-{}-{format}",std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            let destination = root.join("destination.db").to_string_lossy().into_owned();
            let json = root.join("input.json");
            let thread = |id: &str| Thread { id: id.into(), name: id.into(), messages: vec![ChatMessage { id: format!("message-{id}"), text: "fixture".into(), ..Default::default() }], ..Default::default() };
            let a = thread("thread-A");
            std::fs::write(&json, serde_json::to_vec(&vec![a.clone()]).unwrap()).unwrap();
            let (_, mut current) = import_history_async(json.clone(), destination.clone()).await.unwrap();
            std::fs::write(&json, serde_json::to_vec(&vec![thread("thread-B")]).unwrap()).unwrap();
            let source = if format == "sqlite" {
                let source = root.join("source.db");
                crate::store::import_history_from_path(&json, source.to_str().unwrap()).unwrap();
                source
            } else { json };
            let (_, delta) = import_history_async(source, destination.clone()).await.unwrap();
            // The destination still contains A; only the user's current UI state deleted it.
            assert_eq!(crate::store::MessageStore::open(&destination).unwrap().all_threads().unwrap().len(),2);
            current.clear();
            super::super::apply_import_delta(&mut current, delta);
            assert_eq!(current.len(),1);
            assert_eq!(current[0].id,"thread-B");
            assert_eq!(current[0].messages.len(),1);
            assert_eq!(current[0].messages[0].id,"message-thread-B");
            let mut same_thread_input = a.clone();
            same_thread_input.messages[0].id = "message-B-in-A".into();
            let same_json = root.join("same-thread.json");
            std::fs::write(&same_json,serde_json::to_vec(&vec![same_thread_input]).unwrap()).unwrap();
            let source = if format == "sqlite" {
                let source = root.join("same-thread.db");
                crate::store::import_history_from_path(&same_json,source.to_str().unwrap()).unwrap(); source
            } else { same_json };
            let (_, delta) = import_history_async(source,destination).await.unwrap();
            let mut current = vec![a];
            current[0].messages.clear();
            super::super::apply_import_delta(&mut current,delta);
            assert_eq!(current.len(),1);
            assert_eq!(current[0].messages.len(),1);
            assert_eq!(current[0].messages[0].id,"message-B-in-A");
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[tokio::test]
    async fn history_import_hydrates_off_runtime_thread_and_returns_terminal_errors() {
        let root = std::env::temp_dir().join(format!(
            "zl-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("fixture.json");
        let database = root.join("history.db").to_string_lossy().into_owned();
        std::fs::write(&input, r#"[{"id":"thread-1","name":"Fixture","preview":"fixture","time":"","avatar":null,"is_group":false,"messages":[{"id":"message-1","sender":"Fixture","text":"fixture","time":"","is_outgoing":false,"ts":123}]}]"#).unwrap();
        let (report, threads) = import_history_async(input, database.clone())
            .await
            .unwrap();
        assert_eq!(report.messages_imported, 1);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].messages[0].text, "fixture");
        assert!(
            import_history_async(root.join("missing.json"), database)
                .await
                .is_err()
        );
        assert!(!include_str!("modals.rs").contains("crate::store::import_history_from_path"));
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// Bound concurrent native helper processes independently of the 32-entry UI pending budget.
async fn render_inline_emoji(cluster: String, sprite_id: Option<u16>, gui_tx: GuiTx) {
    static SLOTS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
        std::sync::OnceLock::new();
    let slots = SLOTS
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(2)))
        .clone();
    let key = cluster.clone();
    let image = match slots.acquire_owned().await {
        Ok(permit) => crate::runtime_scope::spawn_blocking(move || {
            let _permit = permit;
            match sprite_id {
                Some(sprite) => super::emoji_text::render_original(sprite),
                None => super::emoji_text::render_native(&cluster),
            }
        })
        .await
        .ok()
        .and_then(Result::ok)
        .flatten(),
        Err(_) => None,
    };
    // GuiTx also schedules the UI repaint; failure means the app has closed.
    let _ = gui_tx.send(GuiEvent::InlineEmojiReady {
        cluster: key,
        sprite_id,
        image,
    });
}

#[cfg(all(test, target_os = "macos"))]
mod inline_emoji_native_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires native/macos/build-emoji-preview.sh"]
    async fn inline_emoji_worker_native_color_reaches_actual_inline_mesh() {
        let ctx = egui::Context::default();
        setup_custom_fonts(&ctx);
        let (tx, rx) = std::sync::mpsc::channel();
        let mut images = Vec::new();
        for cluster in ["👍🏽", "👩‍💻", "🇻🇳", "1️⃣"] {
            render_inline_emoji(
                cluster.into(),
                None,
                GuiTx {
            scope: None,
                    tx: tx.clone(),
                    ctx: ctx.clone(),
                },
            )
            .await;
            let GuiEvent::InlineEmojiReady {
                cluster: key,
                image,
                ..
            } = rx.recv_timeout(Duration::from_secs(3)).unwrap()
            else {
                panic!("Expected terminal native render result")
            };
            assert_eq!(key, cluster);
            let image = image.expect("Existing native helper must render a complete grapheme");
            assert_eq!(image.size, [64, 64]);
            assert!(image
                .pixels
                .iter()
                .any(|pixel| pixel.a() > 0 && pixel.r().abs_diff(pixel.g()) > 20));
            images.push(image);
        }
        // The bridge must preserve distinct sequences rather than draw their base scalar.
        let base_thumb = tokio::task::spawn_blocking(|| super::super::emoji_text::render_native("👍")).await.unwrap().unwrap();
        let base_woman = tokio::task::spawn_blocking(|| super::super::emoji_text::render_native("👩")).await.unwrap().unwrap();
        assert_ne!(images[0].pixels, base_thumb.pixels);
        assert_ne!(images[1].pixels, base_woman.pixels);
        super::super::emoji_text::seed_worker_results(
            &ctx,
            ["👍🏽", "👩‍💻", "🇻🇳", "1️⃣"].into_iter().zip(images),
        );
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                super::super::emoji_text::show(ui, "👍🏽 👩‍💻 🇻🇳 1️⃣", 16.0, Color32::WHITE, 300.0);
            });
        });
        let meshes = ctx.tessellate(output.shapes, output.pixels_per_point);
        let color_meshes = meshes
            .iter()
            .filter(|mesh| {
                matches!(&mesh.primitive, egui::epaint::Primitive::Mesh(mesh)
                if mesh.texture_id != egui::TextureId::Managed(0))
            })
            .count();
        assert!(
            color_meshes >= 4,
            "Native worker images must become visible color meshes"
        );
    }
}

#[cfg(test)]
mod original_reaction_import_consumer_tests {
    use super::*;
    use crate::original_reactions::ImportError;

    fn session_fixture(account: &str) -> Session {
        let mut session = Session::new(false, false, false).unwrap();
        session.login = Some(crate::session::LoginInfo {
            uin: account.into(),
            ..Default::default()
        });
        session.zpw_sek = Some("fixture-session".into());
        session
    }
    fn fixture_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "zl-original-import-consumer-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        root
    }
    fn write_source(root: &std::path::Path) -> std::path::PathBuf {
        let path = root.join("explicit-selection.json");
        std::fs::write(&path,serde_json::to_vec(&serde_json::json!({"schema":1,"account_id":"account-a","source":"zalo-macos-local-cache","captured_at_ms":1700000000000_i64,
            "snapshots":[{"idTo":"peer-fixture","rClientMsgId":"client-fixture","rMsgId":"message-fixture","reactions":{"reactor-fixture":{"5":2}}}]})).unwrap()).unwrap();
        path
    }

    #[tokio::test]
    async fn explicit_import_consumer_adopts_file_and_late_replay_uses_only_managed_artifact() {
        let root = fixture_root("flow");
        let managed = root.join("managed");
        let path = write_source(&root);
        let captured = session_fixture("account-a");
        let session = std::sync::Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let (tx, rx) = channel();
        let sender = GuiTx {
            scope: None,
            tx,
            ctx: egui::Context::default(),
        };
        process_original_reaction_import(
            managed.clone(),
            session.clone(),
            captured.clone(),
            sender.clone(),
            "account-a".into(),
            Some(path.clone()),
            false,
        )
        .await;
        let GuiEvent::OriginalReactionImportFinished {
            account,
            result: Ok(Some(cache)),
        } = rx.try_recv().unwrap()
        else {
            panic!("successful import result missing");
        };
        assert_eq!(account, "account-a");
        assert_eq!(cache.provenance().captured_at_ms, 1700000000000);
        std::fs::remove_file(path).unwrap();
        let loaded = vec![Thread {
            id: "peer-fixture".into(),
            messages: vec![ChatMessage {
                id: "message-fixture".into(),
                client_message_id: "client-fixture".into(),
                ..Default::default()
            }],
            ..Default::default()
        }];
        process_reaction_journal_at(managed, session, captured, sender, Vec::new(), loaded).await;
        let GuiEvent::ReactionThreadsUpdated { threads, .. } = rx.try_recv().unwrap() else {
            panic!("managed replay result missing");
        };
        assert_eq!(threads[0].messages[0].reaction_summaries[0].count, 2);
        assert!(threads[0].messages[0].reaction_authoritative_snapshot);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn explicit_import_consumer_cancel_invalid_and_stale_have_structured_terminal_results() {
        let root = fixture_root("terminal");
        let managed = root.join("managed");
        let path = write_source(&root);
        let captured = session_fixture("account-a");
        let session = std::sync::Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let (tx, rx) = channel();
        let sender = GuiTx {
            scope: None,
            tx,
            ctx: egui::Context::default(),
        };
        process_original_reaction_import(
            managed.clone(),
            session.clone(),
            captured.clone(),
            sender.clone(),
            "account-a".into(),
            None,
            false,
        )
        .await;
        assert!(matches!(
            rx.try_recv().unwrap(),
            GuiEvent::OriginalReactionImportFinished {
                result: Ok(None),
                ..
            }
        ));
        std::fs::write(&path, "not an envelope: private-value").unwrap();
        process_original_reaction_import(
            managed.clone(),
            session.clone(),
            captured.clone(),
            sender.clone(),
            "account-a".into(),
            Some(path.clone()),
            false,
        )
        .await;
        assert!(matches!(
            rx.try_recv().unwrap(),
            GuiEvent::OriginalReactionImportFinished {
                result: Err(ImportError::InvalidCache),
                ..
            }
        ));
        *session.lock().await = session_fixture("account-b");
        process_original_reaction_import(
            managed.clone(),
            session,
            captured,
            sender,
            "account-a".into(),
            Some(path),
            false,
        )
        .await;
        assert!(matches!(
            rx.try_recv().unwrap(),
            GuiEvent::OriginalReactionImportFinished {
                result: Err(ImportError::StaleAccount),
                ..
            }
        ));
        assert!(!managed.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn explicit_import_consumer_storage_failure_never_reports_imported_cache() {
        let root = fixture_root("disk");
        let managed = root.join("managed");
        let path = write_source(&root);
        std::fs::write(&managed, "fixture prevents directory creation").unwrap();
        let captured = session_fixture("account-a");
        let session = std::sync::Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let (tx, rx) = channel();
        let sender = GuiTx {
            scope: None,
            tx,
            ctx: egui::Context::default(),
        };
        process_original_reaction_import(
            managed,
            session,
            captured,
            sender,
            "account-a".into(),
            Some(path),
            false,
        )
        .await;
        assert!(matches!(
            rx.try_recv().unwrap(),
            GuiEvent::OriginalReactionImportFinished {
                result: Err(ImportError::Storage),
                ..
            }
        ));
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// Construct the same confirmed local row the GUI inserts, retaining the exact selected identities.
fn composed_sent_message(text: &str, mentions: &[crate::composer_mentions::OutgoingMention], quoted: Option<&ChatMessage>, client_id: u64, server_id: String) -> ChatMessage {
    ChatMessage {
        id: if server_id.is_empty() { format!("out_{client_id}") } else { server_id },
        client_message_id: client_id.to_string(), text: text.into(), sender: "Tôi".into(),
        time: crate::ui::clock(), is_outgoing: true, ts: (crate::login::unix_now() * 1000) as i64,
        wire_ref: crate::message_model::WireMessageRef::from_raw(&serde_json::json!({"mentions": mentions})),
        reply_to_id: quoted.map(|quote| quote.id.clone()),
        reply_to_text: quoted.map(|quote| quote.text.clone()),
        reply_to_sender: quoted.map(|quote| quote.sender.clone()),
        ..Default::default()
    }
}

#[cfg(test)]
mod composer_tests {
    use super::*;
    #[test]
    fn composer_confirmed_local_row_keeps_request_identity_mentions_and_raw_reply() {
        let mentions = [crate::composer_mentions::OutgoingMention { uid: "22".into(), pos: 3, len: 3, kind: 0 }];
        let quote = ChatMessage { id: "original-1".into(), text: "Reply :+1:".into(), ..Default::default() };
        let row = composed_sent_message("😀 @An", &mentions, Some(&quote), 1234567, "server-1".into());
        assert_eq!(row.client_message_id, "1234567");
        assert_eq!(row.id, "server-1");
        assert_eq!(row.text, "😀 @An");
        assert_eq!(row.reply_to_text.as_deref(), Some("Reply :+1:"));
        assert_eq!(crate::message_navigation::message_mentions(&row)[0].user_id, "22");
    }
}

#[cfg(test)]
mod delivery_consumer_tests {
    use super::*;

    fn session(account: &str, token: &str) -> Session {
        let mut session = Session::new(false, false, false).unwrap();
        session.login = Some(crate::session::LoginInfo {
            uin: account.into(),
            ..Default::default()
        });
        session.zpw_sek = Some(token.into());
        session
    }

    #[tokio::test]
    async fn delivery_consumer_socket_and_poll_emit_receipt_only_packets() {
        let captured = session("account-fixture", "token-fixture");
        let current = std::sync::Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let (tx, rx) = std::sync::mpsc::channel();
        let sender = GuiTx {
            scope: None,
            tx,
            ctx: egui::Context::default(),
        };
        let packet = serde_json::json!({"error_code":0,"data":{"delivereds":[{"threadId":"peer-fixture","msgId":"message-fixture"}]}});
        for cmd in [
            crate::socket::CMD_GROUP_DELIVERIES,
            crate::socket::CMD_PULL_DELIVERIES,
        ] {
            dispatch_delivery_receipts(&current, &captured, &sender, cmd, 0, &packet).await;
            match rx.try_recv().unwrap() {
                GuiEvent::DeliveryEvents { account, events } => {
                    assert_eq!(account, "account-fixture");
                    assert_eq!(events.len(), 1);
                    assert_eq!(events[0].message_id.as_deref(), Some("message-fixture"));
                }
                _ => panic!("expected receipt-only event"),
            }
        }
    }

    #[tokio::test]
    async fn delivery_consumer_rejects_stale_account_token_and_malformed_packet() {
        let captured = session("account-fixture", "token-fixture");
        let current = std::sync::Arc::new(tokio::sync::Mutex::new(session(
            "other-fixture",
            "token-fixture",
        )));
        let (tx, rx) = std::sync::mpsc::channel();
        let sender = GuiTx {
            scope: None,
            tx,
            ctx: egui::Context::default(),
        };
        let packet = serde_json::json!({"delivereds":[{"threadId":"peer-fixture","msgId":"message-fixture"}]});
        dispatch_delivery_receipts(&current, &captured, &sender, 502, 0, &packet).await;
        *current.lock().await = session("account-fixture", "other-token-fixture");
        dispatch_delivery_receipts(&current, &captured, &sender, 502, 0, &packet).await;
        *current.lock().await = captured.clone();
        dispatch_delivery_receipts(
            &current,
            &captured,
            &sender,
            502,
            0,
            &serde_json::json!({"delivereds":[{"msgId":1.5}]}),
        )
        .await;
        assert!(rx.try_recv().is_err());
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;

    #[tokio::test]
    async fn stale_worker_lifecycle_intents_do_not_invalidate_or_enqueue() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let stale = crate::runtime_scope::transition("account-a").unwrap();
        let current = crate::runtime_scope::transition("account-b").unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let bridge = WorkerSender {
            sender, ui_thread: std::thread::current().id(),
            ui_generation: std::sync::atomic::AtomicU64::new(current.generation),
        };
        crate::runtime_scope::run_stale_for_test(stale, async {
            assert!(bridge.send(WorkerCmd::RequestQr).is_err());
            assert!(bridge.send(WorkerCmd::Logout).is_err());
        }).await;
        assert_eq!(crate::runtime_scope::registry().current(), current);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn ui_commands_keep_visible_epoch_until_transition_event_is_consumed() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let bridge = WorkerSender {
            sender, ui_thread: std::thread::current().id(),
            ui_generation: std::sync::atomic::AtomicU64::new(7),
        };
        bridge.send(WorkerCmd::RefreshThreads).unwrap();
        assert_eq!(receiver.try_recv().unwrap().0, 7);
        bridge.acknowledge_ui_generation(9);
        bridge.send(WorkerCmd::RefreshThreads).unwrap();
        assert_eq!(receiver.try_recv().unwrap().0, 9);
        bridge.send_at(7, WorkerCmd::PollMessages { known_threads: Vec::new() }).unwrap();
        assert_eq!(receiver.try_recv().unwrap().0, 7);
    }
}

#[cfg(test)]
mod integrated_epoch_tests {
    use super::*;
    fn session_fixture() -> Session {
        let mut session = Session::new(false, false, false).unwrap();
        session.login = Some(crate::session::LoginInfo { uin: "account-a".into(), ..Default::default() });
        session.zpw_sek = Some("fixture-session".into());
        session
    }
    #[tokio::test]
    async fn runtime_new_receipt_consumer_rejects_same_account_aba_with_identical_credentials() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let old = crate::runtime_scope::transition("account-a").unwrap();
        crate::runtime_scope::transition("account-b").unwrap();
        let current = crate::runtime_scope::transition("account-a").unwrap();
        let captured = session_fixture();
        let session = std::sync::Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let (tx, rx) = channel();
        let sender = GuiTx::for_test(tx);
        let payload = serde_json::json!({"delivereds":[{"threadId":"peer-fixture","msgId":"message-fixture"}]});
        crate::runtime_scope::run_stale_for_test(old, async {
            dispatch_delivery_receipts(&session, &captured, &sender, 502, 0, &payload).await;
        }).await;
        assert!(rx.try_recv().is_err());
        crate::runtime_scope::run(current.clone(), async {
            dispatch_delivery_receipts(&session, &captured, &sender, 502, 0, &payload).await;
        }).await.unwrap();
        let GuiEvent::Scoped { scope, event } = rx.try_recv().unwrap() else { panic!("receipt lost its epoch"); };
        assert_eq!(scope, current);
        assert!(matches!(*event, GuiEvent::DeliveryEvents { .. }));
    }
    #[tokio::test]
    async fn runtime_new_journal_and_import_consumers_never_write_after_same_account_aba() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let old = crate::runtime_scope::transition("account-a").unwrap();
        crate::runtime_scope::transition("account-b").unwrap();
        crate::runtime_scope::transition("account-a").unwrap();
        let root = std::env::temp_dir().join(format!("zl-integrated-epoch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("explicit.json");
        let bytes = serde_json::to_vec(&serde_json::json!({"schema":1,"account_id":"account-a","source":"zalo-macos-local-cache","captured_at_ms":1700000000000_i64,"snapshots":[]})).unwrap();
        std::fs::write(&source, &bytes).unwrap();
        let captured = session_fixture();
        let session = std::sync::Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let (tx, rx) = channel();
        let sender = GuiTx::for_test(tx);
        let events = crate::reaction_journal::account_events(&serde_json::json!({"reacts":[{
            "idTo":"account-a","uidFrom":"peer-fixture","msgId":"event-fixture","ts":1700000000000_i64,
            "content":{"rType":5,"rMsg":[{"gMsgID":"message-fixture"}]}
        }]}), "account-a");
        assert!(!events.is_empty());
        crate::runtime_scope::run_stale_for_test(old, async {
            process_reaction_journal_at(root.join("journal"), session.clone(), captured.clone(), sender.clone(), events, Vec::new()).await;
            process_original_reaction_import(root.join("managed"), session, captured, sender, "account-a".into(), Some(source.clone()), false).await;
        }).await;
        assert!(!root.join("journal").exists());
        assert!(!root.join("managed").exists());
        assert_eq!(std::fs::read(source).unwrap(), bytes);
        assert!(rx.try_recv().is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}


async fn account_history(session: std::sync::Arc<tokio::sync::Mutex<Session>>, captured: Session, account: String, request: history::Request) -> Result<history::Reply, String> {
    history::execute(history::root()?, session, captured, account, request).await
}

async fn load_account_threads(session: std::sync::Arc<tokio::sync::Mutex<Session>>, captured: Session, live: Vec<Thread>) -> Vec<Thread> {
    let account = captured.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
    match account_history(session, captured, account, history::Request::Load(live.clone())).await {
        Ok(history::Reply::Threads(threads)) => threads,
        _ => live,
    }
}

async fn publish_account_threads(session: std::sync::Arc<tokio::sync::Mutex<Session>>, captured: Session, sender: GuiTx, threads: Vec<Thread>, live: bool) {
    match history::root() {
        Ok(root) => publish_account_threads_at(root, session, captured, sender, threads, live).await,
        Err(message) => { let _ = sender.send(GuiEvent::StatusUpdate(message)); }
    }
}

pub(super) async fn publish_account_threads_at(root: std::path::PathBuf, session: std::sync::Arc<tokio::sync::Mutex<Session>>, captured: Session, sender: GuiTx, threads: Vec<Thread>, live: bool) {
    let account = captured.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
    match history::execute(root, session, captured, account.clone(), history::Request::Reconcile(threads)).await {
        Ok(history::Reply::Threads(threads)) => { let _ = sender.send(GuiEvent::AccountThreadsUpdated { account, threads, live }); }
        _ => { let _ = sender.send(GuiEvent::StatusUpdate("Chưa lưu được bản cập nhật lịch sử".into())); }
    }
}

// Shared by the real worker arm and disposable persistence/queue fixtures.
async fn process_visible_read_at(root: Result<std::path::PathBuf, String>, session: std::sync::Arc<tokio::sync::Mutex<Session>>, captured: Session, command: WorkerCmd) -> GuiEvent {
    let WorkerCmd::AcknowledgeVisibleRead { account, thread_id, is_group, visible, receipts, ticket } = command else { unreachable!("visible-read command required"); };
    let local = if !ticket.is_current() { Ok(false) } else {
        match root { Ok(root) => history::execute(root, session.clone(), captured.clone(), account.clone(), history::Request::VisibleRead { thread: thread_id.clone(), messages: visible.clone(), ticket: ticket.clone() }).await.map(|reply| matches!(reply, history::Reply::VisibleRead(true))), Err(error) => Err(error) }
    };
    let mut unsent = receipts.clone();
    let mut seen = Ok(false);
    if matches!(local, Ok(true)) && !receipts.is_empty() && ticket.is_current() {
        let verified = crate::read_receipts::verified_visible_receipts(&thread_id, is_group, &receipts);
        let refs = verified.iter().map(|message| message.wire_ref.clone()).collect::<Vec<_>>();
        if let Ok(batch) = crate::read_receipts::SeenBatch::from_wire(&thread_id, is_group, &refs, &captured.imei) {
            let current = session.lock().await;
            let same_account = current.login.as_ref().map(|login| login.uin.as_str()) == Some(account.as_str()) && current.zpw_sek == captured.zpw_sek;
            drop(current);
            let scope_current = crate::runtime_scope::execution_scope().is_some_and(|scope| crate::runtime_scope::registry().accepts(&scope));
            if same_account && scope_current {
                let (submitted, result) = crate::read_receipts::submit_visible(&ticket, crate::read_receipts::send_seen(&captured, &batch)).await;
                if submitted { unsent.clear(); }
                seen = result.map_err(|_| "Seen delivery outcome unknown".to_owned());
            }
        }
    }
    GuiEvent::VisibleReadFinished { account, thread_id, visible, unsent, local, seen }
}

#[cfg(test)]
mod visible_read_worker_tests {
    use super::*;
    #[tokio::test]
    async fn visible_read_worker_persists_local_without_automatic_wire_and_rejects_expired_queue() {
        let root = std::env::temp_dir().join(format!("zl-visible-worker-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mut captured = Session::new(false, false, false).unwrap();
        captured.login = Some(crate::session::LoginInfo { uin: "fixture-account".into(), ..Default::default() });
        let session = std::sync::Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let mut store = crate::store_v2::AccountStore::open(&root, "fixture-account").unwrap();
        let row = ChatMessage { id: "10".into(), kind: crate::message_model::MessageKind::Text, ..Default::default() };
        let thread = Thread { id: "fixture-thread".into(), messages: vec![row.clone()], unread: crate::unread::UnreadState { marked_unread: true, ..Default::default() }, ..Default::default() };
        store.persist_threads(&[thread]).unwrap();
        drop(store);
        let mut gate = crate::read_receipts::ReadVisibilityGate::default();
        for now in [0,100,200,300,400,500] { gate.observe("fixture", true, false, true, now); }
        let event = process_visible_read_at(Ok(root.clone()), session.clone(), captured.clone(), WorkerCmd::AcknowledgeVisibleRead { account: "fixture-account".into(), thread_id: "fixture-thread".into(), is_group: false, visible: vec![row.clone()], receipts: Vec::new(), ticket: gate.ticket().unwrap() }).await;
        assert!(matches!(event, GuiEvent::VisibleReadFinished { local: Ok(true), seen: Ok(false), .. }));
        let reopened = crate::store_v2::AccountStore::open(&root, "fixture-account").unwrap();
        assert_eq!(reopened.unread("fixture-thread").unwrap().last_seen_id.as_deref(), Some("10"));
        assert!(!reopened.unread("fixture-thread").unwrap().marked_unread);
        drop(reopened);
        let ticket = gate.ticket().unwrap();
        gate.reset();
        let event = process_visible_read_at(Ok(root.clone()), session, captured, WorkerCmd::AcknowledgeVisibleRead { account: "fixture-account".into(), thread_id: "fixture-thread".into(), is_group: false, visible: vec![ChatMessage { id: "20".into(), ..row }], receipts: Vec::new(), ticket }).await;
        assert!(matches!(event, GuiEvent::VisibleReadFinished { local: Ok(false), seen: Ok(false), .. }));
        let reopened = crate::store_v2::AccountStore::open(&root, "fixture-account").unwrap();
        assert_eq!(reopened.unread("fixture-thread").unwrap().last_seen_id.as_deref(), Some("10"));
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
}
