//! Complete native desktop GUI powered by `eframe` and `egui`.
//!
//! Provides a Zalo-styled desktop interface with:
//! - QR login & password login
//! - 2-column chat workspace with contact list & speech bubbles
//! - Message sending & auto-refresh
//! - Collapsible Dev Inspector & Protocol Scanner (active only in Dev mode)

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use eframe::egui::{self, Align, Color32, Layout, RichText, Rounding, Stroke, Vec2};

use crate::chat::{ChatMessage, Thread};
use crate::logger::{self, FindingLevel, LogEntry, ScanFinding};
use crate::qr::QrLogin;
use crate::session::Session;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StickerTab {
    Popular,
    Onion,
}

#[derive(Debug, Clone, Copy)]
struct PresetSticker {
    sticker_id: u64,
    cate_id: u64,
    sticker_type: u32,
    label: &'static str,
}

const POPULAR_STICKERS: &[PresetSticker] = &[
    PresetSticker { sticker_id: 18009, cate_id: 10130, sticker_type: 7, label: "Xin chào" },
    PresetSticker { sticker_id: 21458, cate_id: 10302, sticker_type: 7, label: "Vẫy tay" },
    PresetSticker { sticker_id: 21459, cate_id: 10302, sticker_type: 7, label: "Vui vẻ" },
    PresetSticker { sticker_id: 21460, cate_id: 10302, sticker_type: 7, label: "Thả tim" },
    PresetSticker { sticker_id: 21461, cate_id: 10302, sticker_type: 7, label: "Cười lớn" },
    PresetSticker { sticker_id: 21462, cate_id: 10302, sticker_type: 7, label: "Hóng" },
    PresetSticker { sticker_id: 21463, cate_id: 10302, sticker_type: 7, label: "Ngạc nhiên" },
    PresetSticker { sticker_id: 21464, cate_id: 10302, sticker_type: 7, label: "Cảm ơn" },
    PresetSticker { sticker_id: 16713, cate_id: 10047, sticker_type: 7, label: "Hello" },
    PresetSticker { sticker_id: 18869, cate_id: 10177, sticker_type: 7, label: "OK" },
    PresetSticker { sticker_id: 19773, cate_id: 10233, sticker_type: 7, label: "Tuyệt" },
    PresetSticker { sticker_id: 17751, cate_id: 10117, sticker_type: 7, label: "Yêu thương" },
    PresetSticker { sticker_id: 18910, cate_id: 10183, sticker_type: 7, label: "Haha" },
    PresetSticker { sticker_id: 20695, cate_id: 10275, sticker_type: 7, label: "Chơi game" },
    PresetSticker { sticker_id: 19949, cate_id: 10242, sticker_type: 7, label: "Mèo con" },
    PresetSticker { sticker_id: 394, cate_id: 14, sticker_type: 7, label: "Chào bạn" },
];

const ONION_STICKERS: &[PresetSticker] = &[
    PresetSticker { sticker_id: 1, cate_id: 0, sticker_type: 7, label: "Củ hành #1" },
    PresetSticker { sticker_id: 2, cate_id: 0, sticker_type: 7, label: "Củ hành #2" },
    PresetSticker { sticker_id: 3, cate_id: 0, sticker_type: 7, label: "Củ hành #3" },
    PresetSticker { sticker_id: 4, cate_id: 0, sticker_type: 7, label: "Củ hành #4" },
    PresetSticker { sticker_id: 5, cate_id: 0, sticker_type: 7, label: "Củ hành #5" },
    PresetSticker { sticker_id: 6, cate_id: 0, sticker_type: 7, label: "Củ hành #6" },
    PresetSticker { sticker_id: 7, cate_id: 0, sticker_type: 7, label: "Củ hành #7" },
    PresetSticker { sticker_id: 8, cate_id: 0, sticker_type: 7, label: "Củ hành #8" },
    PresetSticker { sticker_id: 9, cate_id: 0, sticker_type: 7, label: "Củ hành #9" },
    PresetSticker { sticker_id: 10, cate_id: 0, sticker_type: 7, label: "Củ hành #10" },
    PresetSticker { sticker_id: 11, cate_id: 0, sticker_type: 7, label: "Củ hành #11" },
    PresetSticker { sticker_id: 12, cate_id: 0, sticker_type: 7, label: "Củ hành #12" },
    PresetSticker { sticker_id: 13, cate_id: 0, sticker_type: 7, label: "Củ hành #13" },
    PresetSticker { sticker_id: 14, cate_id: 0, sticker_type: 7, label: "Củ hành #14" },
    PresetSticker { sticker_id: 15, cate_id: 0, sticker_type: 7, label: "Củ hành #15" },
    PresetSticker { sticker_id: 16, cate_id: 0, sticker_type: 7, label: "Củ hành #16" },
];

pub enum GuiEvent {
    QrReady {
        qr_login: QrLogin,
        qr_image: egui::ColorImage,
    },
    QrScanned {
        name: String,
    },
    LoginSuccess {
        name: String,
        uin: String,
    },
    LoginFailed(String),
    ThreadsUpdated(Vec<Thread>),
    MessageSent {
        thread_id: String,
        text: String,
        msg_id: String,
    },
    StickerSent {
        thread_id: String,
        msg_id: String,
        sticker_id: u64,
        cate_id: u64,
        sticker_type: u32,
    },
    StickerImageReady {
        key: String,
        image: egui::ColorImage,
    },
    SendFailed {
        thread_id: String,
        text: String,
        error: String,
    },
    SessionExpired(String),
    StatusUpdate(String),
    LoggedOut,
}

pub struct ZaloGuiApp {
    // Session state
    is_logged_in: bool,
    user_name: String,
    user_uin: String,

    // Login screen state
    login_tab: LoginTab,
    phone_input: String,
    password_input: String,
    login_in_progress: bool,
    login_status_msg: String,

    // QR state
    qr_texture: Option<egui::TextureHandle>,
    qr_status: String,

    // Chat screen state
    threads: Vec<Thread>,
    selected_thread_index: usize,
    search_query: String,
    input_message: String,
    status_msg: String,

    // Sticker picker & image cache state
    sticker_picker_open: bool,
    sticker_tab: StickerTab,
    sticker_textures: HashMap<String, egui::TextureHandle>,
    loading_stickers: HashSet<String>,

    // Dev Inspector state
    dev_mode: bool,
    dev_panel_open: bool,
    dev_tab: DevTab,
    selected_log: Option<LogEntry>,
    scanner_findings: Vec<ScanFinding>,

    // Background channels
    event_rx: Receiver<GuiEvent>,
    #[allow(dead_code)]
    event_tx: Sender<GuiEvent>,

    // Polling timer
    last_poll: std::time::Instant,
}

#[derive(Debug, PartialEq, Eq)]
enum LoginTab {
    QrCode,
    Password,
}

#[derive(Debug, PartialEq, Eq)]
enum DevTab {
    Logs,
    Scanner,
    CryptoState,
}

impl ZaloGuiApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        session: Session,
        dev_mode: bool,
        initial_threads: Vec<Thread>,
    ) -> Self {
        setup_custom_fonts(&_cc.egui_ctx);
        let (tx, rx) = channel();

        let is_logged_in = session.login.is_some();
        let (user_name, user_uin) = session
            .login
            .as_ref()
            .map(|l| (l.display_name.clone(), l.uin.clone()))
            .unwrap_or_default();

        let mut threads = if initial_threads.is_empty() {
            crate::chat::load_threads_cache()
        } else {
            let mut cached = crate::chat::load_threads_cache();
            for t in initial_threads {
                if let Some(c) = cached.iter_mut().find(|c| c.id == t.id) {
                    c.name = t.name;
                    if !t.preview.is_empty() {
                        c.preview = t.preview;
                        c.time = t.time;
                    }
                    for m in t.messages {
                        if !c.messages.iter().any(|cm| !cm.id.is_empty() && cm.id == m.id) {
                            c.messages.push(m);
                        }
                    }
                    c.messages.sort_by_key(|m| m.ts);
                } else {
                    cached.push(t);
                }
            }
            cached
        };
        crate::store::load_history_into_threads("zalo_history.db", &mut threads, 200);

        // Select the conversation with the most recent message by default (e.g., "Đtc")
        let selected_thread_index = threads
            .iter()
            .enumerate()
            .max_by_key(|(_, t)| t.messages.last().map(|m| m.ts).unwrap_or(0))
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        let app = Self {
            is_logged_in,
            user_name,
            user_uin,
            login_tab: LoginTab::QrCode,
            phone_input: String::new(),
            password_input: String::new(),
            login_in_progress: false,
            login_status_msg: String::new(),
            qr_texture: None,
            qr_status: "Đang tạo mã QR đăng nhập…".into(),
            threads,
            selected_thread_index,
            search_query: String::new(),
            input_message: String::new(),
            status_msg: "Sẵn sàng".into(),
            sticker_picker_open: false,
            sticker_tab: StickerTab::Popular,
            sticker_textures: HashMap::new(),
            loading_stickers: HashSet::new(),
            dev_mode,
            dev_panel_open: false,
            dev_tab: DevTab::Logs,
            selected_log: None,
            scanner_findings: Vec::new(),
            event_rx: rx,
            event_tx: tx.clone(),
            last_poll: std::time::Instant::now(),
        };

        // Shared background session runner
        spawn_session_worker(session, tx, _cc.egui_ctx.clone());

        // Trigger initial background action without blocking window creation
        if let Some(wtx) = WORKER_TX.get() {
            if is_logged_in {
                let _ = wtx.send(WorkerCmd::RefreshThreads);
            } else {
                let _ = wtx.send(WorkerCmd::RequestQr);
            }
        }

        app
    }

    fn check_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                GuiEvent::QrReady { qr_image, .. } => {
                    self.qr_texture = Some(ctx.load_texture(
                        "qr_code_tex",
                        qr_image,
                        egui::TextureOptions::NEAREST,
                    ));
                    self.qr_status = "Quét mã bằng ứng dụng Zalo trên điện thoại…".into();
                    self.login_in_progress = false;
                }
                GuiEvent::QrScanned { name } => {
                    self.qr_status = format!("✓ Đã quét: {name}. Vui lòng bấm 'Đăng nhập' trên điện thoại…");
                }
                GuiEvent::LoginSuccess { name, uin } => {
                    self.is_logged_in = true;
                    self.user_name = name;
                    self.user_uin = uin;
                    self.login_in_progress = false;
                    self.status_msg = "Đăng nhập thành công · Đang lắng nghe tin nhắn thời gian thực".into();
                }
                GuiEvent::LoginFailed(err) => {
                    self.login_in_progress = false;
                    self.login_status_msg = format!("Lỗi đăng nhập: {err}");
                }
                GuiEvent::ThreadsUpdated(updates) => {
                    if self.is_logged_in && !updates.is_empty() {
                        let mut has_changes = false;
                        for nt in updates {
                            if let Some(existing) = self.threads.iter_mut().find(|t| t.id == nt.id) {
                                let is_generic_name = nt.name.is_empty()
                                    || nt.name.starts_with("Người dùng ")
                                    || nt.name.starts_with("Nhóm ");
                                if !nt.name.is_empty() && (!is_generic_name || existing.name.is_empty()) && existing.name != nt.name {
                                    existing.name = nt.name;
                                    has_changes = true;
                                }
                                if !nt.preview.is_empty() && existing.preview != nt.preview {
                                    existing.preview = nt.preview;
                                    existing.time = nt.time;
                                    has_changes = true;
                                }
                                for msg in nt.messages {
                                    if let Some(existing_msg) = existing.messages.iter_mut().find(|m| {
                                        (!m.id.is_empty() && !msg.id.is_empty() && m.id == msg.id)
                                            || (m.text == msg.text
                                                && m.is_outgoing == msg.is_outgoing
                                                && (m.time == msg.time
                                                    || (m.ts != 0 && msg.ts != 0 && (m.ts - msg.ts).abs() < 5000)))
                                    }) {
                                        if existing_msg.id.starts_with("out_") && !msg.id.is_empty() && !msg.id.starts_with("out_") {
                                            existing_msg.id = msg.id;
                                            has_changes = true;
                                        }
                                        if msg.ts > 0 && existing_msg.ts != msg.ts {
                                            existing_msg.ts = msg.ts;
                                            existing_msg.time = msg.time;
                                            has_changes = true;
                                        }
                                        if existing_msg.sticker_id.is_none() && msg.sticker_id.is_some() {
                                            existing_msg.sticker_id = msg.sticker_id;
                                            existing_msg.sticker_cate_id = msg.sticker_cate_id;
                                            existing_msg.sticker_type = msg.sticker_type;
                                            has_changes = true;
                                        }
                                    } else {
                                        existing.messages.push(msg);
                                        has_changes = true;
                                    }
                                }
                                existing.messages.sort_by_key(|m| m.ts);
                                if let Some(newest) = existing.messages.last() {
                                    if !newest.text.is_empty() && existing.preview != newest.text {
                                        existing.preview = newest.text.replace('\n', " ");
                                        existing.time = newest.time.clone();
                                    }
                                }
                            } else if !nt.id.is_empty() && nt.id != "0" {
                                self.threads.push(nt);
                                has_changes = true;
                            }
                        }
                        if self.selected_thread_index >= self.threads.len() {
                            self.selected_thread_index = self.threads.len().saturating_sub(1);
                        }
                        if has_changes {
                            if let Ok(mut store) = crate::store::MessageStore::open("zalo_history.db") {
                                let _ = crate::store::persist_threads(&mut store, &self.threads);
                            }
                            crate::chat::save_threads_cache(&self.threads);
                        }
                    }
                    self.status_msg = format!("🟢 Trực tuyến · Cập nhật lúc {}", crate::ui::clock());
                }
                GuiEvent::SessionExpired(reason) => {
                    self.is_logged_in = false;
                    self.qr_texture = None;
                    self.qr_status = format!("⚠ {reason} — Vui lòng quét mã QR mới để gửi/nhận tin nhắn trực tiếp.");
                    self.status_msg = format!("⚠ {reason}. Lịch sử tin nhắn vẫn được giữ nguyên.");
                }
                GuiEvent::LoggedOut => {
                    self.is_logged_in = false;
                    self.user_name.clear();
                    self.user_uin.clear();
                    self.threads.clear();
                    self.selected_thread_index = 0;
                    self.qr_texture = None;
                    self.qr_status = "Đang xin mã QR mới…".into();
                    self.status_msg = "Đã đăng xuất. Vui lòng quét mã QR mới.".into();
                }
                GuiEvent::MessageSent { thread_id, text, msg_id } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.preview = text.clone();
                        let now = crate::ui::clock();
                        t.time = now.clone();
                        let final_id = if msg_id.is_empty() {
                            format!("out_{}", crate::login::unix_now())
                        } else {
                            msg_id
                        };
                        if !t.messages.iter().any(|m| !m.id.is_empty() && m.id == final_id) {
                            t.messages.push(ChatMessage {
                                id: final_id,
                                sender: "Tôi".into(),
                                text,
                                time: now,
                                is_outgoing: true,
                                ts: (crate::login::unix_now() * 1000) as i64,
                                sticker_id: None,
                                sticker_cate_id: None,
                                sticker_type: None,
                            });
                            t.messages.sort_by_key(|m| m.ts);
                        }
                        if let Ok(mut store) = crate::store::MessageStore::open("zalo_history.db") {
                            let _ = crate::store::persist_threads(&mut store, &self.threads);
                        }
                        crate::chat::save_threads_cache(&self.threads);
                    }
                    self.status_msg = format!("✓ Đã gửi tin nhắn lúc {}", crate::ui::clock());
                }
                GuiEvent::StickerSent {
                    thread_id,
                    msg_id,
                    sticker_id,
                    cate_id,
                    sticker_type,
                } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.preview = "[Nhãn dán]".into();
                        let now = crate::ui::clock();
                        t.time = now.clone();
                        let final_id = if msg_id.is_empty() {
                            format!("out_{}", crate::login::unix_now())
                        } else {
                            msg_id
                        };
                        if !t.messages.iter().any(|m| !m.id.is_empty() && m.id == final_id) {
                            t.messages.push(ChatMessage {
                                id: final_id,
                                sender: "Tôi".into(),
                                text: "[Nhãn dán]".into(),
                                time: now,
                                is_outgoing: true,
                                ts: (crate::login::unix_now() * 1000) as i64,
                                sticker_id: Some(sticker_id),
                                sticker_cate_id: Some(cate_id),
                                sticker_type: Some(sticker_type),
                            });
                            t.messages.sort_by_key(|m| m.ts);
                        }
                        if let Ok(mut store) = crate::store::MessageStore::open("zalo_history.db") {
                            let _ = crate::store::persist_threads(&mut store, &self.threads);
                        }
                        crate::chat::save_threads_cache(&self.threads);
                    }
                    self.status_msg = format!("✓ Đã gửi nhãn dán lúc {}", crate::ui::clock());
                }
                GuiEvent::StickerImageReady { key, image } => {
                    let tex = ctx.load_texture(&key, image, egui::TextureOptions::LINEAR);
                    self.sticker_textures.insert(key, tex);
                }
                GuiEvent::SendFailed { thread_id: _, text, error } => {
                    // Restore text to input box if empty so user doesn't lose what they typed
                    if self.input_message.is_empty() && !text.is_empty() {
                        self.input_message = text;
                    }
                    self.status_msg = format!("❌ Không gửi được: {error}");
                }
                GuiEvent::StatusUpdate(msg) => {
                    self.status_msg = msg;
                }
            }
        }
    }
}

// Background commands sent from GUI to session worker
pub enum WorkerCmd {
    RequestQr,
    LoginPassword { phone: String, pass: String },
    RefreshThreads,
    PollMessages { known_threads: Vec<(String, String, bool)> },
    LoadGroupHistory { group_id: String },
    SendMessage { to_id: String, text: String, is_group: bool },
    SendSticker {
        to_id: String,
        sticker_id: u64,
        cate_id: u64,
        sticker_type: u32,
        is_group: bool,
    },
    FetchStickerImage { key: String, url: String },
    Logout,
}

static WORKER_TX: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<WorkerCmd>> =
    std::sync::OnceLock::new();

fn is_session_expired_error(err_str: &str) -> bool {
    err_str.contains("server error 600")
        || err_str.contains("zpw_sek")
        || err_str.contains("server error -102")
        || err_str.contains("server error -69")
        || err_str.contains("chưa đăng nhập")
}

fn spawn_session_worker(session: Session, gui_tx: Sender<GuiEvent>, egui_ctx: egui::Context) {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<WorkerCmd>();
    let _ = WORKER_TX.set(cmd_tx);

    let session = std::sync::Arc::new(tokio::sync::Mutex::new(session));
    let poll_state = std::sync::Arc::new(tokio::sync::Mutex::new(crate::chat::PollState::default()));

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async move {
            // Initial server info check
            {
                let s = session.lock().await.clone();
                let _ = crate::login::get_server_info(&s).await;
            }

            let mut current_qr_task: Option<tokio::task::JoinHandle<()>> = None;

            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    WorkerCmd::Logout => {
                        if let Some(task) = current_qr_task.take() {
                            task.abort();
                        }
                        let _ = std::fs::remove_file("session.json");
                        let _ = std::fs::remove_file("threads_cache.json");
                        {
                            let mut s = session.lock().await;
                            s.logout();
                        }
                        {
                            let mut ps = poll_state.lock().await;
                            *ps = crate::chat::PollState::default();
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

                        let handle = tokio::spawn(async move {
                            // Reset session credentials before requesting fresh QR
                            {
                                let mut s = session_clone.lock().await;
                                s.logout();
                            }
                            {
                                let mut ps = poll_clone.lock().await;
                                *ps = crate::chat::PollState::default();
                            }
                            let _ = gui_tx_clone.send(GuiEvent::StatusUpdate("Đang xin mã QR mới…".into()));
                            ctx_clone.request_repaint();

                            let s_snap = { session_clone.lock().await.clone() };
                            let q_res = crate::qr::request_qr(&s_snap, "vi").await;

                            match q_res {
                                Ok(q) => {
                                    if let Some(payload) = q.payload() {
                                        if let Ok(code) = qrcode::QrCode::new(payload.as_bytes()) {
                                            let w = code.width();
                                            let quiet_zone = 4;
                                            let total_w = w + quiet_zone * 2;
                                            let mut pixels = Vec::with_capacity(total_w * total_w * 4);
                                            for y in 0..total_w {
                                                for x in 0..total_w {
                                                    let in_qr = x >= quiet_zone && x < quiet_zone + w && y >= quiet_zone && y < quiet_zone + w;
                                                    let dark = in_qr && (code[(x - quiet_zone, y - quiet_zone)] == qrcode::Color::Dark);
                                                    let c = if dark { [0, 0, 0, 255] } else { [255, 255, 255, 255] };
                                                    pixels.extend_from_slice(&c);
                                                }
                                            }
                                            let img = egui::ColorImage::from_rgba_unmultiplied([total_w, total_w], &pixels);
                                            let _ = gui_tx_clone.send(GuiEvent::QrReady {
                                                qr_login: q.clone(),
                                                qr_image: img,
                                            });
                                            ctx_clone.request_repaint();

                                            // Wait for mobile scan
                                            let scan_res = crate::qr::wait_for_scan(&s_snap, &q).await;

                                            if let Ok(scanned) = scan_res {
                                                let name = scanned.get("display_name").and_then(|v| v.as_str()).unwrap_or("Zalo User");
                                                let _ = gui_tx_clone.send(GuiEvent::QrScanned { name: name.to_string() });
                                                ctx_clone.request_repaint();

                                                // Wait for mobile confirmation
                                                let confirm_res = crate::qr::confirm(&s_snap, &q).await;

                                                if let Ok(resp) = confirm_res {
                                                    let mut s = session_clone.lock().await;
                                                    if let Some(sek) = crate::qr::extract_zpw_sek(&resp) {
                                                        s.set_zpw_sek(sek);
                                                    }
                                                    if let Some(data) = resp.get("data") {
                                                        crate::login::apply_login_config(&mut s, data);
                                                    }
                                                    let _ = s.save_to_file("session.json");

                                                    if let Ok(info) = crate::login::get_login_info(&s).await {
                                                        if let Some(cfg) = info.get("data") {
                                                            crate::login::apply_login_config(&mut s, cfg);
                                                            s.apply_server_info(cfg);
                                                            let _ = s.save_to_file("session.json");
                                                        }
                                                    }

                                                    let uin = s.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
                                                    let dname = s.login.as_ref().map(|l| l.display_name.clone()).unwrap_or_default();
                                                    let _ = gui_tx_clone.send(GuiEvent::LoginSuccess { name: dname, uin });
                                                    ctx_clone.request_repaint();

                                                    // Fetch initial friends, merge local SQLite history & full queued messages
                                                    let mut list = crate::chat::get_friends(&s).await.unwrap_or_default();
                                                    crate::store::load_history_into_threads("zalo_history.db", &mut list, 200);
                                                    {
                                                        let mut ps = poll_clone.lock().await;
                                                        let _ = crate::chat::poll_new_messages(&s, &mut ps, &mut list).await;
                                                    }
                                                    let _ = gui_tx_clone.send(GuiEvent::ThreadsUpdated(list));
                                                    ctx_clone.request_repaint();
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = gui_tx_clone.send(GuiEvent::LoginFailed(e.to_string()));
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
                        tokio::spawn(async move {
                            let s = session_clone.lock().await.clone();
                            if s.login.is_some() {
                                // Auto-validate session on server to prevent getting stuck in expired session
                                if !s.validate_session().await {
                                    let _ = std::fs::remove_file("session.json");
                                    {
                                        let mut sl = session_clone.lock().await;
                                        sl.logout();
                                    }
                                    let _ = gui_tx_clone.send(GuiEvent::SessionExpired(
                                        "Phiên đăng nhập (zpw_sek) đã hết hạn trên máy chủ Zalo".into(),
                                    ));
                                    ctx_clone.request_repaint();
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::RequestQr);
                                    }
                                    return;
                                }
                                let mut list = crate::chat::get_friends(&s).await.unwrap_or_default();
                                crate::store::load_history_into_threads("zalo_history.db", &mut list, 200);
                                {
                                    let mut ps = poll_clone.lock().await;
                                    let _ = crate::chat::poll_new_messages(&s, &mut ps, &mut list).await;
                                }
                                let _ = gui_tx_clone.send(GuiEvent::ThreadsUpdated(list));
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
                        tokio::spawn(async move {
                            let s = session_clone.lock().await.clone();
                            if s.login.is_some() {
                                // Seed `temp_threads` with known thread metadata so `Cloud của tôi` routing works
                                // and we don't spam `getminiprofiles` for already-known contacts.
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
                                for _ in 0..3 {
                                    let poll_res = {
                                        let mut ps = poll_clone.lock().await;
                                        crate::chat::get_chat_from_polling(&s, &mut ps).await
                                    };
                                    match poll_res {
                                        Ok(lp_resp) => {
                                            crate::chat::merge_previews(&mut temp_threads, &lp_resp);
                                            let data_node = lp_resp.get("data").unwrap_or(&lp_resp);
                                            let more = data_node.get("more").and_then(serde_json::Value::as_i64).unwrap_or(0);
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
                                            poll_err = Some(e.to_string());
                                            break;
                                        }
                                    }
                                }

                                if let Some(err_s) = poll_err {
                                    if is_session_expired_error(&err_s) {
                                        let _ = std::fs::remove_file("session.json");
                                        {
                                            let mut sl = session_clone.lock().await;
                                            sl.logout();
                                        }
                                        let _ = gui_tx_clone.send(GuiEvent::SessionExpired(
                                            "Phiên đăng nhập (zpw_sek) đã hết hạn".into(),
                                        ));
                                        ctx_clone.request_repaint();
                                        if let Some(tx) = WORKER_TX.get() {
                                            let _ = tx.send(WorkerCmd::RequestQr);
                                        }
                                    }
                                } else {
                                    // Only keep threads that actually received new messages or were newly discovered
                                    temp_threads.retain(|t| !t.messages.is_empty());
                                    if !temp_threads.is_empty() {
                                        crate::chat::resolve_thread_names(&s, &mut temp_threads).await;
                                        let _ = gui_tx_clone.send(GuiEvent::ThreadsUpdated(temp_threads));
                                        ctx_clone.request_repaint();
                                    }
                                }
                            }
                            IS_POLLING.store(false, std::sync::atomic::Ordering::SeqCst);
                        });
                    }
                    WorkerCmd::LoadGroupHistory { group_id } => {
                        let session_clone = session.clone();
                        let gui_tx_clone = gui_tx.clone();
                        let ctx_clone = egui_ctx.clone();
                        tokio::spawn(async move {
                            let s = session_clone.lock().await.clone();
                            if s.login.is_some() {
                                let mut t = Thread {
                                    id: group_id.clone(),
                                    name: String::new(),
                                    preview: String::new(),
                                    time: String::new(),
                                    avatar: None,
                                    is_group: true,
                                    messages: Vec::new(),
                                };
                                if let Ok(resp) = crate::chat::get_group_history(&s, &group_id, 50).await {
                                    crate::chat::merge_group_history(&mut t, &resp);
                                }
                                if let Ok(cm_resp) = crate::chat::get_cloud_messages(&s, &group_id, "10000000000000000", false, 50).await {
                                    crate::chat::merge_group_history(&mut t, &cm_resp);
                                }
                                if !t.messages.is_empty() {
                                    let _ = gui_tx_clone.send(GuiEvent::ThreadsUpdated(vec![t]));
                                    ctx_clone.request_repaint();
                                }
                            }
                        });
                    }
                    WorkerCmd::SendMessage { to_id, text, is_group } => {
                        let session_clone = session.clone();
                        let gui_tx_clone = gui_tx.clone();
                        let ctx_clone = egui_ctx.clone();
                        tokio::spawn(async move {
                            let s = session_clone.lock().await.clone();
                            let res = if is_group {
                                crate::chat::send_group_message(&s, &to_id, &text).await
                            } else {
                                crate::chat::send_message(&s, &to_id, &text).await
                            };
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
                                        let _ = std::fs::remove_file("session.json");
                                        {
                                            let mut sl = session_clone.lock().await;
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
                                        let _ = std::fs::remove_file("session.json");
                                        {
                                            let mut sl = session_clone.lock().await;
                                            sl.logout();
                                        }
                                        let _ = gui_tx_clone.send(GuiEvent::SessionExpired(
                                            "Không thể gửi nhãn dán vì phiên zpw_sek đã hết hạn".into(),
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
                    WorkerCmd::FetchStickerImage { key, url } => {
                        let session_clone = session.clone();
                        let gui_tx_clone = gui_tx.clone();
                        let ctx_clone = egui_ctx.clone();
                        tokio::spawn(async move {
                            let cache_dir = std::path::Path::new("stickers_cache");
                            let _ = std::fs::create_dir_all(cache_dir);
                            let cache_path = cache_dir.join(format!("{key}.png"));

                            let bytes_opt = if let Ok(disk_bytes) = std::fs::read(&cache_path) {
                                Some(disk_bytes)
                            } else {
                                let client = { session_clone.lock().await.http.clone() };
                                if let Ok(resp) = client.get(&url).send().await {
                                    if let Ok(b) = resp.bytes().await {
                                        let vec = b.to_vec();
                                        let _ = std::fs::write(&cache_path, &vec);
                                        Some(vec)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            };

                            if let Some(bytes) = bytes_opt {
                                if let Ok(dyn_img) = image::load_from_memory(&bytes) {
                                    let rgba = dyn_img.to_rgba8();
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
                                }
                            }
                        });
                    }
                    WorkerCmd::LoginPassword { phone, pass } => {
                        let session_clone = session.clone();
                        let poll_clone = poll_state.clone();
                        let gui_tx_clone = gui_tx.clone();
                        let ctx_clone = egui_ctx.clone();
                        tokio::spawn(async move {
                            let _ = gui_tx_clone.send(GuiEvent::StatusUpdate("Đang xác thực tài khoản…".into()));
                            ctx_clone.request_repaint();
                            let mut s = session_clone.lock().await;
                            match crate::login::login_by_password(&mut s, &phone, &pass, "VN").await {
                                Ok(_) => {
                                    if let Ok(info) = crate::login::get_login_info(&s).await {
                                        if let Some(cfg) = info.get("data") {
                                            crate::login::apply_login_config(&mut s, cfg);
                                            s.apply_server_info(cfg);
                                            let _ = s.save_to_file("session.json");

                                            let uin = s.login.as_ref().map(|l| l.uin.clone()).unwrap_or_default();
                                            let name = s.login.as_ref().map(|l| l.display_name.clone()).unwrap_or_default();
                                            let _ = gui_tx_clone.send(GuiEvent::LoginSuccess { name, uin });
                                            ctx_clone.request_repaint();

                                            if let Ok(mut list) = crate::chat::get_friends(&s).await {
                                                {
                                                    let mut ps = poll_clone.lock().await;
                                                    *ps = crate::chat::PollState::default();
                                                    let _ = crate::chat::poll_new_messages(&s, &mut ps, &mut list).await;
                                                }
                                                let _ = gui_tx_clone.send(GuiEvent::ThreadsUpdated(list));
                                                ctx_clone.request_repaint();
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = gui_tx_clone.send(GuiEvent::LoginFailed(e.to_string()));
                                    ctx_clone.request_repaint();
                                }
                            }
                        });
                    }
                }
            }
        });
    });
}

impl eframe::App for ZaloGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.check_events(ctx);

        // Continuous non-overlapping HTTP long-polling (`getChatFromPolling` on `lp_chat`, guarded by `IS_POLLING`)
        if self.is_logged_in && self.last_poll.elapsed() >= Duration::from_millis(800) {
            self.last_poll = std::time::Instant::now();
            if let Some(tx) = WORKER_TX.get() {
                let known: Vec<(String, String, bool)> = self
                    .threads
                    .iter()
                    .map(|t| (t.id.clone(), t.name.clone(), t.is_group))
                    .collect();
                let _ = tx.send(WorkerCmd::PollMessages { known_threads: known });
            }
        }

        ctx.request_repaint_after(Duration::from_millis(200));

        // Top App Bar
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("Zalo").color(Color32::from_rgb(0, 104, 255)).strong());
                ui.label(RichText::new("· Rust Client").color(Color32::GRAY));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.dev_mode {
                        let dev_badge = RichText::new(" ⚙ DEV MODE ")
                            .color(Color32::BLACK)
                            .background_color(Color32::from_rgb(255, 193, 7))
                            .strong();
                        if ui.button(dev_badge).clicked() {
                            self.dev_panel_open = !self.dev_panel_open;
                            if self.dev_panel_open {
                                self.scanner_findings = logger::scan_logs();
                            }
                        }
                    } else {
                        ui.label(RichText::new(" 🔒 PRODUCTION ").color(Color32::WHITE).background_color(Color32::from_rgb(40, 167, 69)));
                    }

                    if self.is_logged_in {
                        if ui.button(RichText::new("🚪 Đăng xuất / Quét lại QR").small().color(Color32::from_rgb(255, 90, 90))).clicked() {
                            self.is_logged_in = false;
                            self.user_name.clear();
                            self.user_uin.clear();
                            self.threads.clear();
                            self.selected_thread_index = 0;
                            self.qr_texture = None;
                            self.login_status_msg.clear();
                            self.qr_status = "Đang xin mã QR mới…".into();
                            self.status_msg = "Đã đăng xuất. Đang tạo mã QR mới…".into();
                            let _ = std::fs::remove_file("session.json");
                            let _ = std::fs::remove_file("threads_cache.json");
                            if let Some(tx) = WORKER_TX.get() {
                                let _ = tx.send(WorkerCmd::Logout);
                                let _ = tx.send(WorkerCmd::RequestQr);
                            }
                        }
                        ui.label(format!("👤 {} ({})", self.user_name, self.user_uin));
                    } else if !self.threads.is_empty() {
                        if ui.button(RichText::new("💬 Xem lịch sử đã lưu").small().color(Color32::from_rgb(100, 180, 255))).clicked() {
                            self.is_logged_in = true;
                        }
                    }
                });
            });
        });

        // Bottom Status Bar
        egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&self.status_msg).small().color(Color32::GRAY));
                if self.is_logged_in {
                    ui.label(RichText::new(format!("· {} hội thoại", self.threads.len())).small().color(Color32::DARK_GRAY));
                }

                if self.dev_mode {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if self.dev_panel_open { "▼ Đóng Dev Inspector" } else { "▲ Mở Dev Inspector" };
                        if ui.button(RichText::new(label).small()).clicked() {
                            self.dev_panel_open = !self.dev_panel_open;
                            if self.dev_panel_open {
                                self.scanner_findings = logger::scan_logs();
                            }
                        }
                    });
                }
            });
        });

        // Dev Inspector Panel (Collapsible bottom panel, ONLY active in Dev mode)
        if self.dev_mode && self.dev_panel_open {
            egui::TopBottomPanel::bottom("dev_inspector")
                .resizable(true)
                .default_height(260.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong("🔍 Dev Inspector & Protocol Scanner");
                        ui.separator();
                        ui.selectable_value(&mut self.dev_tab, DevTab::Logs, "Lưu lượng mạng (Logs)");
                        ui.selectable_value(&mut self.dev_tab, DevTab::Scanner, "Quét lỗi hệ thống (Scanner)");
                        ui.selectable_value(&mut self.dev_tab, DevTab::CryptoState, "Trạng thái Crypto & Keys");

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Làm mới quét").clicked() {
                                self.scanner_findings = logger::scan_logs();
                            }
                            if ui.button("Xóa log").clicked() {
                                logger::clear_entries();
                                self.selected_log = None;
                                self.scanner_findings.clear();
                            }
                        });
                    });
                    ui.separator();

                    match self.dev_tab {
                        DevTab::Logs => self.render_dev_logs(ui),
                        DevTab::Scanner => self.render_dev_scanner(ui),
                        DevTab::CryptoState => self.render_dev_crypto(ui),
                    }
                });
        }

        // Central Panel: Either Login Screen or Main Chat Screen
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.is_logged_in {
                self.render_login_screen(ui);
            } else {
                self.render_chat_screen(ui);
            }
        });
    }
}

impl ZaloGuiApp {
    fn render_login_screen(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            ui.heading(RichText::new("Đăng nhập Zalo").color(Color32::from_rgb(0, 104, 255)).size(24.0).strong());
            ui.label(RichText::new("Chọn phương thức đăng nhập để bắt đầu trò chuyện").color(Color32::GRAY));
            ui.add_space(20.0);

            // Tab bar
            ui.horizontal(|ui| {
                ui.add_space(ui.available_width() / 2.0 - 120.0);
                ui.selectable_value(&mut self.login_tab, LoginTab::QrCode, "📱 Quét mã QR");
                ui.selectable_value(&mut self.login_tab, LoginTab::Password, "🔑 Mật khẩu");
            });
            ui.add_space(15.0);

            match self.login_tab {
                LoginTab::QrCode => {
                    egui::Frame::group(ui.style())
                        .rounding(Rounding::same(12.0))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(80, 85, 95)))
                        .inner_margin(20.0)
                        .show(ui, |ui| {
                            ui.set_width(340.0);
                            ui.vertical_centered(|ui| {
                                if let Some(tex) = &self.qr_texture {
                                    egui::Frame::none()
                                        .fill(Color32::WHITE)
                                        .rounding(Rounding::same(8.0))
                                        .inner_margin(12.0)
                                        .show(ui, |ui| {
                                            ui.image((tex.id(), Vec2::new(280.0, 280.0)));
                                        });
                                } else {
                                    let (rect, _) = ui.allocate_exact_size(Vec2::new(280.0, 280.0), egui::Sense::hover());
                                    ui.painter().rect_filled(rect, 8.0, Color32::from_rgb(245, 247, 250));
                                    ui.painter().text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "Chưa có mã QR",
                                        egui::FontId::proportional(16.0),
                                        Color32::GRAY,
                                    );
                                }
                                ui.add_space(14.0);
                                let dark = ui.visuals().dark_mode;
                                let text_color = if dark { Color32::from_rgb(220, 225, 235) } else { Color32::from_rgb(40, 40, 40) };
                                ui.label(RichText::new(&self.qr_status).color(text_color).size(14.0));
                                ui.add_space(12.0);

                                if ui.button(RichText::new("🔄 Tạo / Làm mới mã QR").size(14.0).strong()).clicked() {
                                    self.login_in_progress = true;
                                    self.qr_status = "Đang xin mã QR…".into();
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::RequestQr);
                                    }
                                }
                            });
                        });
                }
                LoginTab::Password => {
                    egui::Frame::group(ui.style())
                        .rounding(Rounding::same(8.0))
                        .inner_margin(20.0)
                        .show(ui, |ui| {
                            ui.set_width(320.0);
                            ui.vertical(|ui| {
                                ui.label("Số điện thoại (VD: 0836553142):");
                                ui.text_edit_singleline(&mut self.phone_input);
                                ui.add_space(8.0);

                                ui.label("Mật khẩu:");
                                ui.add(egui::TextEdit::singleline(&mut self.password_input).password(true));
                                ui.add_space(15.0);

                                if !self.login_status_msg.is_empty() {
                                    ui.label(RichText::new(&self.login_status_msg).color(Color32::RED));
                                    ui.add_space(8.0);
                                }

                                let btn = egui::Button::new(RichText::new("Đăng nhập").color(Color32::WHITE).strong())
                                    .fill(Color32::from_rgb(0, 104, 255))
                                    .min_size(Vec2::new(ui.available_width(), 34.0));

                                if ui.add_enabled(!self.login_in_progress, btn).clicked() {
                                    self.login_in_progress = true;
                                    self.login_status_msg = "Đang kết nối…".into();
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::LoginPassword {
                                            phone: self.phone_input.clone(),
                                            pass: self.password_input.clone(),
                                        });
                                    }
                                }
                            });
                        });
                }
            }
        });
    }

    fn render_chat_screen(&mut self, ui: &mut egui::Ui) {
        let full_height = ui.available_height();

        // 2-Column Layout
        ui.horizontal(|ui| {
            // Left Column: Contact / Conversation List (300px)
            ui.vertical(|ui| {
                ui.set_width(290.0);
                ui.set_height(full_height);

                // Search box & Refresh header
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .hint_text("🔍 Tìm kiếm bạn bè…")
                            .desired_width(230.0),
                    );
                    if ui.button("🔄").on_hover_text("Làm mới danh sách").clicked() {
                        if let Some(tx) = WORKER_TX.get() {
                            let _ = tx.send(WorkerCmd::RefreshThreads);
                        }
                    }
                });
                ui.add_space(6.0);

                // Thread list
                egui::ScrollArea::vertical()
                    .id_salt("threads_list_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let query = self.search_query.to_lowercase();
                        for (i, t) in self.threads.iter().enumerate() {
                            if !query.is_empty() && !t.name.to_lowercase().contains(&query) {
                                continue;
                            }

                            let is_selected = i == self.selected_thread_index;
                            let dark = ui.visuals().dark_mode;
                            let bg_color = if is_selected {
                                if dark {
                                    Color32::from_rgb(32, 54, 88)
                                } else {
                                    Color32::from_rgb(225, 238, 255)
                                }
                            } else {
                                Color32::TRANSPARENT
                            };

                            let frame = egui::Frame::none()
                                .fill(bg_color)
                                .rounding(Rounding::same(6.0))
                                .inner_margin(8.0);

                            let resp = frame.show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Avatar circle
                                    let (circle_rect, _) = ui.allocate_exact_size(Vec2::new(36.0, 36.0), egui::Sense::hover());
                                    let circle_color = if t.is_group {
                                        Color32::from_rgb(108, 117, 125)
                                    } else {
                                        Color32::from_rgb(0, 104, 255)
                                    };
                                    ui.painter().circle_filled(circle_rect.center(), 18.0, circle_color);
                                    let initial = t.name.chars().next().unwrap_or('?').to_uppercase().to_string();
                                    ui.painter().text(
                                        circle_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        initial,
                                        egui::FontId::proportional(16.0),
                                        Color32::WHITE,
                                    );

                                    // Name & snippet
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            let name_color = if is_selected {
                                                if dark { Color32::WHITE } else { Color32::from_rgb(0, 104, 255) }
                                            } else {
                                                if dark { Color32::from_rgb(225, 230, 240) } else { Color32::from_rgb(30, 30, 30) }
                                            };
                                            ui.strong(RichText::new(&t.name).color(name_color));
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.label(RichText::new(&t.time).small().color(Color32::GRAY));
                                            });
                                        });
                                        let snippet = if t.preview.is_empty() {
                                            "Chưa có tin nhắn".to_string()
                                        } else {
                                            t.preview.chars().take(24).collect::<String>()
                                        };
                                        let snippet_color = if dark {
                                            Color32::from_rgb(145, 155, 170)
                                        } else {
                                            Color32::GRAY
                                        };
                                        ui.label(RichText::new(snippet).small().color(snippet_color));
                                    });
                                });
                            });

                            if resp.response.interact(egui::Sense::click()).clicked() {
                                self.selected_thread_index = i;
                                if t.is_group {
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::LoadGroupHistory {
                                            group_id: t.id.clone(),
                                        });
                                    }
                                }
                            }
                        }
                    });
            });

            ui.separator();

            // Right Column: Active Conversation & Messages Area
            ui.vertical(|ui| {
                ui.set_height(full_height);

                let active_thread = self.threads.get(self.selected_thread_index).cloned();

                if let Some(thread) = active_thread {
                    // Chat header
                    ui.horizontal(|ui| {
                        ui.heading(RichText::new(&thread.name).strong());
                        ui.label(
                            RichText::new(format!("· {} tin nhắn", thread.messages.len()))
                                .small()
                                .color(Color32::from_rgb(40, 167, 69)),
                        );
                        if thread.is_group {
                            if ui.small_button("📜 Tải lịch sử nhóm").clicked() {
                                if let Some(tx) = WORKER_TX.get() {
                                    let _ = tx.send(WorkerCmd::LoadGroupHistory {
                                        group_id: thread.id.clone(),
                                    });
                                }
                            }
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(format!("ID: {}", thread.id)).small().color(Color32::GRAY));
                        });
                    });
                    ui.separator();

                    // Scrollable messages viewport (auto_shrink false keeps the input bar docked at bottom)
                    let picker_reserve = if self.sticker_picker_open { 205.0 } else { 0.0 };
                    let msg_area_height = (ui.available_height() - 52.0 - picker_reserve).max(100.0);
                    egui::ScrollArea::vertical()
                        .id_salt("chat_messages_scroll")
                        .max_height(msg_area_height)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.add_space(10.0);
                            if thread.messages.is_empty() {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(40.0);
                                    ui.label(RichText::new("Chưa có tin nhắn trong cuộc trò chuyện này.").color(Color32::GRAY));
                                });
                            } else {
                                for msg in &thread.messages {
                                    render_chat_bubble(
                                        ui,
                                        msg,
                                        &self.sticker_textures,
                                        &mut self.loading_stickers,
                                    );
                                    ui.add_space(6.0);
                                }
                            }
                        });

                    if self.sticker_picker_open {
                        ui.separator();
                        self.render_sticker_picker(ui, &thread.id, thread.is_group);
                    }

                    ui.separator();

                    // Bottom chat input area
                    ui.horizontal(|ui| {
                        let sticker_btn_color = if self.sticker_picker_open {
                            Color32::from_rgb(0, 104, 255)
                        } else {
                            Color32::from_rgb(70, 78, 92)
                        };
                        let sticker_btn = egui::Button::new(
                            RichText::new("😊 Nhãn dán").color(Color32::WHITE).strong(),
                        )
                        .fill(sticker_btn_color)
                        .min_size(Vec2::new(92.0, 30.0));
                        if ui.add(sticker_btn).on_hover_text("Mở bảng chọn Nhãn dán Zalo").clicked() {
                            self.sticker_picker_open = !self.sticker_picker_open;
                        }

                        let text_edit = egui::TextEdit::singleline(&mut self.input_message)
                            .hint_text("Nhập tin nhắn…")
                            .desired_width((ui.available_width() - 90.0).max(80.0));

                        let response = ui.add(text_edit);
                        let enter_pressed = (response.lost_focus() || response.has_focus())
                            && ui.input(|i| i.key_pressed(egui::Key::Enter));

                        let send_btn = egui::Button::new(RichText::new("Gửi").color(Color32::WHITE).strong())
                            .fill(Color32::from_rgb(0, 104, 255))
                            .min_size(Vec2::new(75.0, 30.0));

                        let send_clicked = ui.add(send_btn).clicked();

                        if (send_clicked || enter_pressed) && !self.input_message.trim().is_empty() {
                            let text = self.input_message.trim().to_string();
                            self.input_message.clear();
                            response.request_focus();
                            if let Some(tx) = WORKER_TX.get() {
                                let _ = tx.send(WorkerCmd::SendMessage {
                                    to_id: thread.id.clone(),
                                    text,
                                    is_group: thread.is_group,
                                });
                            }
                        } else if enter_pressed {
                            response.request_focus();
                        }
                    });
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(100.0);
                        ui.label(RichText::new("Chọn một hội thoại bên trái để xem tin nhắn.").color(Color32::GRAY));
                    });
                }
            });
        });
    }

    fn render_dev_logs(&mut self, ui: &mut egui::Ui) {
        let entries = logger::get_entries();
        ui.horizontal(|ui| {
            // Left list of entries (350px)
            ui.vertical(|ui| {
                ui.set_width(360.0);
                egui::ScrollArea::vertical()
                    .id_salt("dev_logs_list_scroll")
                    .show(ui, |ui| {
                        for e in entries.iter().rev() {
                            let _status_color = if e.is_error { Color32::RED } else { Color32::from_rgb(40, 167, 69) };
                            let st_str = e.status.map(|s| s.to_string()).unwrap_or_else(|| "ERR".into());

                            let row = ui.selectable_label(
                                self.selected_log.as_ref().map(|s| s.id == e.id).unwrap_or(false),
                                format!("[{}] {} {} ({}ms)", e.timestamp, e.method, st_str, e.latency_ms),
                            );
                            if row.clicked() {
                                self.selected_log = Some(e.clone());
                            }
                        }
                    });
            });

            ui.separator();

            // Right detail inspector
            ui.vertical(|ui| {
                if let Some(e) = &self.selected_log {
                    ui.strong(format!("{} {}", e.method, e.url));
                    ui.label(format!("Thời gian: {} · Độ trễ: {}ms · HTTP: {:?}", e.timestamp, e.latency_ms, e.status));
                    if let Some(err) = &e.error_msg {
                        ui.colored_label(Color32::RED, format!("Lỗi: {err}"));
                    }
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .id_salt("dev_logs_detail_scroll")
                        .show(ui, |ui| {
                            if let Some(dec) = &e.decrypted_preview {
                                ui.label("Dữ liệu giải mã (Decrypted Payload):");
                                ui.code(dec);
                            } else {
                                ui.label("Payload:");
                                ui.code(&e.payload_preview);
                            }
                        });
                } else {
                    ui.label("Chọn một request bên trái để kiểm tra chi tiết.");
                }
            });
        });
    }

    fn render_dev_scanner(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("dev_scanner_scroll")
            .show(ui, |ui| {
                ui.heading("Báo cáo quét chẩn đoán tự động:");
                ui.add_space(6.0);

                if self.scanner_findings.is_empty() {
                    ui.label("Bấm 'Làm mới quét' để chạy kiểm tra.");
                }

                for f in &self.scanner_findings {
                    let (badge_text, color) = match f.level {
                        FindingLevel::Info => ("INFO", Color32::from_rgb(40, 167, 69)),
                        FindingLevel::Warning => ("CẢNH BÁO", Color32::from_rgb(255, 193, 7)),
                        FindingLevel::Critical => ("NGHIÊM TRỌNG", Color32::RED),
                    };

                    egui::Frame::group(ui.style())
                        .stroke(Stroke::new(1.0_f32, color))
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.colored_label(color, format!("[{badge_text}]"));
                                ui.strong(&f.title);
                            });
                            ui.label(&f.detail);
                            ui.label(RichText::new(format!("Khắc phục: {}", f.suggestion)).color(Color32::from_rgb(0, 104, 255)));
                        });
                    ui.add_space(6.0);
                }
            });
    }

    fn render_dev_crypto(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("dev_crypto_scroll")
            .show(ui, |ui| {
                ui.heading("Thông số mật mã & Khung giao thức:");
                ui.add_space(8.0);

                egui::Grid::new("crypto_grid").striped(true).show(ui, |ui| {
                    ui.label("API Version (zpw_ver):");
                    ui.code(crate::session::ZPW_VER);
                    ui.end_row();

                    ui.label("API Type (zpw_type):");
                    ui.code(crate::session::ZPW_TYPE);
                    ui.end_row();

                    ui.label("ParamCipher Master Key:");
                    ui.code(format!("{:02X?}", crate::param_cipher::PARAM_CIPHER_KEY));
                    ui.end_row();

                    ui.label("Zero IV:");
                    ui.code("00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 (16 zero bytes)");
                    ui.end_row();

                    ui.label("Thuật toán mã hoá:");
                    ui.label("AES-128-CBC / AES-256-CBC with PKCS7 padding");
                    ui.end_row();

                    ui.label("Chữ ký SignKey:");
                    ui.code("md5('zsecure' + tag + sorted_values)");
                    ui.end_row();
                });
            });
    }

    fn render_sticker_picker(&mut self, ui: &mut egui::Ui, thread_id: &str, is_group: bool) {
        egui::Frame::none()
            .fill(if ui.visuals().dark_mode {
                Color32::from_rgb(30, 34, 42)
            } else {
                Color32::from_rgb(245, 247, 250)
            })
            .rounding(Rounding::same(8.0))
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_height(180.0);
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.sticker_tab,
                        StickerTab::Popular,
                        "🔥 Nhãn dán phổ biến",
                    );
                    ui.selectable_value(
                        &mut self.sticker_tab,
                        StickerTab::Onion,
                        "🧅 Bộ Củ hành",
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("✖ Đóng").clicked() {
                            self.sticker_picker_open = false;
                        }
                    });
                });
                ui.separator();

                let list = match self.sticker_tab {
                    StickerTab::Popular => POPULAR_STICKERS,
                    StickerTab::Onion => ONION_STICKERS,
                };

                egui::ScrollArea::vertical()
                    .id_salt("sticker_picker_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for item in list {
                                let key = format!("stk_{}_{}", item.cate_id, item.sticker_id);
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

                                if !self.sticker_textures.contains_key(&key)
                                    && self.loading_stickers.insert(key.clone())
                                {
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::FetchStickerImage {
                                            key: key.clone(),
                                            url,
                                        });
                                    }
                                }

                                let clicked = if let Some(tex) = self.sticker_textures.get(&key) {
                                    let img_btn = egui::ImageButton::new((tex.id(), Vec2::new(62.0, 62.0)));
                                    ui.add(img_btn)
                                        .on_hover_text(format!("Gửi nhãn dán: {}", item.label))
                                        .clicked()
                                } else {
                                    ui.add_sized([68.0, 68.0], egui::Button::new(item.label))
                                        .on_hover_text(format!("Gửi nhãn dán: {}", item.label))
                                        .clicked()
                                };

                                if clicked {
                                    self.sticker_picker_open = false;
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::SendSticker {
                                            to_id: thread_id.to_string(),
                                            sticker_id: item.sticker_id,
                                            cate_id: item.cate_id,
                                            sticker_type: item.sticker_type,
                                            is_group,
                                        });
                                    }
                                }
                            }
                        });
                    });
            });
    }
}

fn render_sticker_widget(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    sticker_textures: &HashMap<String, egui::TextureHandle>,
    loading_stickers: &mut HashSet<String>,
) -> bool {
    let (sid, cid) = if let Some(sid) = msg.sticker_id {
        (sid, msg.sticker_cate_id.unwrap_or(0))
    } else if msg.text.trim() == "[Nhãn dán]" {
        // Fallback for legacy `[Nhãn dán]` messages saved before sticker_id column existed
        (18009, 10130)
    } else {
        return false;
    };

    if sid == 0 {
        return false;
    }

    let key = format!("stk_{cid}_{sid}");
    let url = if cid == 0 && (1..=30).contains(&sid) {
        format!("https://stc-chat.zdn.vn/images/stickers/default/thumb/{sid}.png")
    } else {
        format!("https://api.zaloapp.com/api/emoticon/sticker/webpc?eid={sid}&size=130")
    };

    if !sticker_textures.contains_key(&key) && loading_stickers.insert(key.clone()) {
        if let Some(tx) = WORKER_TX.get() {
            let _ = tx.send(WorkerCmd::FetchStickerImage {
                key: key.clone(),
                url,
            });
        }
    }

    if let Some(tex) = sticker_textures.get(&key) {
        egui::Frame::none()
            .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 235))
            .rounding(Rounding::same(10.0))
            .inner_margin(6.0)
            .show(ui, |ui| {
                ui.image((tex.id(), Vec2::new(112.0, 112.0)));
            });
        true
    } else {
        false
    }
}

fn render_chat_bubble(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    sticker_textures: &HashMap<String, egui::TextureHandle>,
    loading_stickers: &mut HashSet<String>,
) {
    let max_bubble_width = (ui.available_width() * 0.68).min(480.0).max(140.0);
    if msg.is_outgoing {
        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
            ui.add_space(10.0);
            egui::Frame::none()
                .fill(Color32::from_rgb(0, 104, 255))
                .rounding(Rounding { nw: 14.0, ne: 4.0, sw: 14.0, se: 14.0 })
                .inner_margin(egui::Margin::symmetric(14.0, 9.0))
                .show(ui, |ui| {
                    ui.set_max_width(max_bubble_width);
                    // Use top_down(Align::Min) without nested right_to_left so the bubble shrinks tightly to text!
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        if !render_sticker_widget(ui, msg, sticker_textures, loading_stickers) {
                            ui.add(egui::Label::new(RichText::new(&msg.text).color(Color32::WHITE).size(14.5)).wrap());
                        }
                        ui.label(RichText::new(&msg.time).color(Color32::from_rgb(190, 220, 255)).small());
                    });
                });
        });
    } else {
        ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
            ui.add_space(8.0);
            let dark = ui.visuals().dark_mode;
            let bg = if dark {
                Color32::from_rgb(45, 52, 62)
            } else {
                Color32::from_rgb(240, 242, 245)
            };
            let text_color = if dark {
                Color32::WHITE
            } else {
                Color32::BLACK
            };
            egui::Frame::none()
                .fill(bg)
                .rounding(Rounding { nw: 4.0, ne: 14.0, sw: 14.0, se: 14.0 })
                .inner_margin(egui::Margin::symmetric(14.0, 9.0))
                .show(ui, |ui| {
                    ui.set_max_width(max_bubble_width);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        if !msg.sender.is_empty() && msg.sender != "0" {
                            ui.label(
                                RichText::new(&msg.sender)
                                    .color(Color32::from_rgb(100, 180, 255))
                                    .small()
                                    .strong(),
                            );
                        }
                        if !render_sticker_widget(ui, msg, sticker_textures, loading_stickers) {
                            ui.add(egui::Label::new(RichText::new(&msg.text).color(text_color).size(14.5)).wrap());
                        }
                        ui.label(RichText::new(&msg.time).color(Color32::GRAY).small());
                    });
                });
        });
    }
}

fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let font_candidates = [
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
        "C:\\Windows\\Fonts\\tahoma.ttf",
    ];

    for path in font_candidates {
        if let Ok(font_data) = std::fs::read(path) {
            fonts.font_data.insert(
                "vietnamese_font".to_owned(),
                egui::FontData::from_owned(font_data),
            );

            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "vietnamese_font".to_owned());

            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("vietnamese_font".to_owned());
            break;
        }
    }

    ctx.set_fonts(fonts);
}
