//! Complete native desktop GUI powered by `eframe` and `egui`.
//!
//! Designed with the UI/UX Pro Max Minimalism & Swiss Style system:
//! - Zero-idle-CPU reactive architecture (event-driven repaint, zero per-frame Thread cloning)
//! - Custom vector icons rendered via `egui::Painter` (zero texture/SVG bloat)
//! - High-contrast Light & Dark semantic color tokens (`#0068FF` / `#2563EB` primary, `#059669` online accent)
//! - Filterable conversation sidebar (`Tất cả`, `Cá nhân`, `Nhóm`), avatar badges, and grouped chat bubbles
//! - Built-in Zalo Sticker rendering & picker (`[Nhãn dán]`)

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use eframe::egui::{
    self, Align, Color32, CursorIcon, FontId, Id, Key, Layout, Margin, Pos2, Rect, RichText,
    Rounding, Sense, Stroke, TextureHandle, TextureOptions, Vec2,
};

use crate::chat::{ChatMessage, Thread};
use crate::logger::{self, FindingLevel, LogEntry, ScanFinding};
use crate::qr::QrLogin;
use crate::session::Session;

// ---------------------------------------------------------------------------
// Semantic Design System Tokens (UI/UX Pro Max — Minimalism & Swiss Style)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct ThemePalette {
    is_dark: bool,
    bg_app: Color32,
    bg_sidebar: Color32,
    bg_chat_canvas: Color32,
    bg_elevated: Color32,
    bg_hover: Color32,
    bg_selected: Color32,
    bg_input: Color32,
    border: Color32,
    text_primary: Color32,
    text_secondary: Color32,
    text_muted: Color32,
    primary: Color32,
    primary_hover: Color32,
    accent_online: Color32,
    danger: Color32,
    warning: Color32,
    bubble_out_bg: Color32,
    bubble_out_text: Color32,
    bubble_out_meta: Color32,
    bubble_in_bg: Color32,
    bubble_in_border: Color32,
    bubble_in_text: Color32,
    bubble_in_meta: Color32,
}

impl ThemePalette {
    fn dark() -> Self {
        Self {
            is_dark: true,
            bg_app: Color32::from_rgb(15, 23, 42),         // #0F172A Slate 900
            bg_sidebar: Color32::from_rgb(17, 24, 39),     // #111827 Gray 900
            bg_chat_canvas: Color32::from_rgb(11, 17, 32), // #0B1120 Deep Canvas
            bg_elevated: Color32::from_rgb(30, 41, 59),    // #1E293B Slate 800
            bg_hover: Color32::from_rgb(30, 41, 59),       // #1E293B
            bg_selected: Color32::from_rgb(23, 49, 92),    // Deep Zalo Blue selection
            bg_input: Color32::from_rgb(15, 23, 42),       // #0F172A
            border: Color32::from_rgb(38, 50, 72),         // Subtle divider
            text_primary: Color32::from_rgb(248, 250, 252), // #F8FAFC (15.8:1 contrast)
            text_secondary: Color32::from_rgb(148, 163, 184), // #94A3B8 (6.4:1 contrast)
            text_muted: Color32::from_rgb(100, 116, 139),  // #64748B
            primary: Color32::from_rgb(37, 99, 235),       // #2563EB Primary Blue
            primary_hover: Color32::from_rgb(59, 130, 246), // #3B82F6
            accent_online: Color32::from_rgb(16, 185, 129), // #10B981 Emerald
            danger: Color32::from_rgb(239, 68, 68),        // #EF4444
            warning: Color32::from_rgb(245, 158, 11),      // #F59E0B
            bubble_out_bg: Color32::from_rgb(0, 90, 224),  // Zalo Royal Blue
            bubble_out_text: Color32::WHITE,
            bubble_out_meta: Color32::from_rgb(191, 219, 254),
            bubble_in_bg: Color32::from_rgb(30, 41, 59),   // #1E293B Card
            bubble_in_border: Color32::from_rgb(45, 58, 82),
            bubble_in_text: Color32::from_rgb(241, 245, 249),
            bubble_in_meta: Color32::from_rgb(148, 163, 184),
        }
    }

    fn light() -> Self {
        Self {
            is_dark: false,
            bg_app: Color32::from_rgb(255, 255, 255),      // #FFFFFF
            bg_sidebar: Color32::from_rgb(255, 255, 255),  // #FFFFFF
            bg_chat_canvas: Color32::from_rgb(238, 242, 248), // Soft Zalo Blue-Gray canvas
            bg_elevated: Color32::from_rgb(255, 255, 255), // #FFFFFF
            bg_hover: Color32::from_rgb(241, 245, 249),    // #F1F5F9
            bg_selected: Color32::from_rgb(229, 241, 255), // #E5F1FF Zalo active row
            bg_input: Color32::from_rgb(248, 250, 252),    // #F8FAFC
            border: Color32::from_rgb(226, 232, 240),      // #E2E8F0
            text_primary: Color32::from_rgb(15, 23, 42),   // #0F172A (16.1:1 contrast)
            text_secondary: Color32::from_rgb(71, 85, 105), // #475569 (7.2:1 contrast)
            text_muted: Color32::from_rgb(100, 116, 139),  // #64748B (4.6:1 contrast)
            primary: Color32::from_rgb(0, 104, 255),       // #0068FF Zalo Blue
            primary_hover: Color32::from_rgb(37, 99, 235), // #2563EB
            accent_online: Color32::from_rgb(5, 150, 105), // #059669 Emerald
            danger: Color32::from_rgb(220, 38, 38),        // #DC2626
            warning: Color32::from_rgb(217, 119, 6),       // #D97706
            bubble_out_bg: Color32::from_rgb(229, 241, 255), // Zalo Desktop Light Outgoing (#E5F1FF)
            bubble_out_text: Color32::from_rgb(15, 23, 42),  // Crisp dark text on light blue (14:1)
            bubble_out_meta: Color32::from_rgb(71, 85, 105),
            bubble_in_bg: Color32::WHITE,
            bubble_in_border: Color32::from_rgb(219, 228, 240),
            bubble_in_text: Color32::from_rgb(15, 23, 42),
            bubble_in_meta: Color32::from_rgb(100, 116, 139),
        }
    }

    fn apply_to_ctx(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        let mut visuals = if self.is_dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        visuals.panel_fill = self.bg_app;
        visuals.window_fill = self.bg_elevated;
        visuals.extreme_bg_color = self.bg_input;
        visuals.faint_bg_color = self.bg_hover;
        visuals.override_text_color = Some(self.text_primary);
        visuals.selection.bg_fill = self.primary.linear_multiply(if self.is_dark { 0.45 } else { 0.22 });
        visuals.selection.stroke = Stroke::new(1.5_f32, self.primary);

        let r = Rounding::same(8.0);
        visuals.widgets.noninteractive.bg_fill = self.bg_app;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, self.border);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, self.text_primary);
        visuals.widgets.noninteractive.rounding = r;

        visuals.widgets.inactive.bg_fill = self.bg_elevated;
        visuals.widgets.inactive.weak_bg_fill = self.bg_elevated;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, self.border);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, self.text_primary);
        visuals.widgets.inactive.rounding = r;

        visuals.widgets.hovered.bg_fill = self.bg_hover;
        visuals.widgets.hovered.weak_bg_fill = self.bg_hover;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.2_f32, self.primary);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.2_f32, self.text_primary);
        visuals.widgets.hovered.rounding = r;

        visuals.widgets.active.bg_fill = self.primary;
        visuals.widgets.active.weak_bg_fill = self.primary;
        visuals.widgets.active.bg_stroke = Stroke::new(1.5_f32, self.primary);
        visuals.widgets.active.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
        visuals.widgets.active.rounding = r;

        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(10.0, 6.0);
        style.spacing.scroll = egui::style::ScrollStyle::solid();
        style.spacing.scroll.bar_width = 6.0;
        style.visuals = visuals;
        ctx.set_style(style);
    }
}

// ---------------------------------------------------------------------------
// Zero-allocation Vector Icon Painter Primitives (Replaces Emoji Icons)
// ---------------------------------------------------------------------------

fn avatar_color_for(name: &str, is_group: bool) -> Color32 {
    const PALETTE: &[Color32] = &[
        Color32::from_rgb(37, 99, 235),  // Blue
        Color32::from_rgb(13, 148, 136), // Teal
        Color32::from_rgb(124, 58, 237), // Violet
        Color32::from_rgb(219, 39, 119), // Pink
        Color32::from_rgb(234, 88, 12),  // Orange
        Color32::from_rgb(5, 150, 105),  // Emerald
        Color32::from_rgb(79, 70, 229),  // Indigo
        Color32::from_rgb(2, 132, 199),  // Sky
    ];
    let mut hash: usize = if is_group { 3 } else { 0 };
    for b in name.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(b as usize);
    }
    PALETTE[hash % PALETTE.len()]
}

fn truncate_chars<'a>(s: &'a str, max_chars: usize) -> std::borrow::Cow<'a, str> {
    if let Some((byte_idx, _)) = s.char_indices().nth(max_chars) {
        std::borrow::Cow::Owned(format!("{}…", &s[..byte_idx]))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

fn draw_icon_search(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    let r = size * 0.34;
    let c = Pos2::new(center.x - size * 0.08, center.y - size * 0.08);
    painter.circle_stroke(c, r, Stroke::new(1.6_f32, color));
    let p1 = Pos2::new(c.x + r * 0.72, c.y + r * 0.72);
    let p2 = Pos2::new(center.x + size * 0.38, center.y + size * 0.38);
    painter.line_segment([p1, p2], Stroke::new(1.8_f32, color));
}

fn draw_icon_refresh(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    let r = size * 0.36;
    painter.circle_stroke(center, r, Stroke::new(1.6_f32, color));
    let tip = Pos2::new(center.x + r * 0.85, center.y - r * 0.35);
    painter.line_segment(
        [tip, Pos2::new(tip.x - 4.0, tip.y - 1.5)],
        Stroke::new(1.6_f32, color),
    );
    painter.line_segment(
        [tip, Pos2::new(tip.x - 1.0, tip.y + 4.0)],
        Stroke::new(1.6_f32, color),
    );
}

fn draw_icon_sticker(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    let r = size * 0.42;
    painter.circle_stroke(center, r, Stroke::new(1.6_f32, color));
    let eye_y = center.y - r * 0.22;
    let eye_dx = r * 0.34;
    painter.circle_filled(Pos2::new(center.x - eye_dx, eye_y), 1.5, color);
    painter.circle_filled(Pos2::new(center.x + eye_dx, eye_y), 1.5, color);
    // Smile curve approximated with 3 segments
    let s1 = Pos2::new(center.x - r * 0.42, center.y + r * 0.14);
    let s2 = Pos2::new(center.x - r * 0.16, center.y + r * 0.36);
    let s3 = Pos2::new(center.x + r * 0.16, center.y + r * 0.36);
    let s4 = Pos2::new(center.x + r * 0.42, center.y + r * 0.14);
    painter.line_segment([s1, s2], Stroke::new(1.5_f32, color));
    painter.line_segment([s2, s3], Stroke::new(1.5_f32, color));
    painter.line_segment([s3, s4], Stroke::new(1.5_f32, color));
}

fn draw_icon_send(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    let r = size * 0.42;
    let p_right = Pos2::new(center.x + r, center.y);
    let p_top = Pos2::new(center.x - r * 0.85, center.y - r * 0.78);
    let p_mid = Pos2::new(center.x - r * 0.35, center.y);
    let p_bot = Pos2::new(center.x - r * 0.85, center.y + r * 0.78);
    let stroke = Stroke::new(1.6_f32, color);
    painter.line_segment([p_top, p_right], stroke);
    painter.line_segment([p_right, p_bot], stroke);
    painter.line_segment([p_bot, p_mid], stroke);
    painter.line_segment([p_mid, p_top], stroke);
    painter.line_segment([p_mid, p_right], stroke);
}

fn draw_icon_theme(painter: &egui::Painter, center: Pos2, size: f32, is_dark: bool, color: Color32) {
    let r = size * 0.32;
    if is_dark {
        // Sun icon (switch to light)
        painter.circle_stroke(center, r * 0.75, Stroke::new(1.6_f32, color));
        for i in 0..8 {
            let angle = (i as f32) * std::f32::consts::TAU / 8.0;
            let (sin, cos) = angle.sin_cos();
            let p1 = Pos2::new(center.x + cos * r * 1.05, center.y + sin * r * 1.05);
            let p2 = Pos2::new(center.x + cos * r * 1.45, center.y + sin * r * 1.45);
            painter.line_segment([p1, p2], Stroke::new(1.4_f32, color));
        }
    } else {
        // Crescent indicator (switch to dark)
        painter.circle_filled(center, r, color);
        painter.circle_filled(
            Pos2::new(center.x + r * 0.42, center.y - r * 0.32),
            r * 0.82,
            Color32::WHITE,
        );
    }
}

// ---------------------------------------------------------------------------
// Sticker Presets & State Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StickerTab {
    Popular,
    Onion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThreadFilter {
    All,
    Direct,
    Groups,
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
    // Theme state
    dark_mode: bool,
    theme_dirty: bool,

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
    qr_texture: Option<TextureHandle>,
    qr_status: String,

    // Chat screen state
    threads: Vec<Thread>,
    selected_thread_index: usize,
    thread_filter: ThreadFilter,
    search_query: String,
    input_message: String,
    status_msg: String,

    // Sticker picker & image cache state
    sticker_picker_open: bool,
    sticker_tab: StickerTab,
    sticker_textures: HashMap<String, TextureHandle>,
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

    // Background poll trigger
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
        cc: &eframe::CreationContext<'_>,
        session: Session,
        dev_mode: bool,
        initial_threads: Vec<Thread>,
    ) -> Self {
        setup_custom_fonts(&cc.egui_ctx);
        let dark_mode = true;
        ThemePalette::dark().apply_to_ctx(&cc.egui_ctx);

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

        let selected_thread_index = threads
            .iter()
            .enumerate()
            .max_by_key(|(_, t)| t.messages.last().map(|m| m.ts).unwrap_or(0))
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        let app = Self {
            dark_mode,
            theme_dirty: false,
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
            thread_filter: ThreadFilter::All,
            search_query: String::new(),
            input_message: String::new(),
            status_msg: "Trực tuyến · Sẵn sàng".into(),
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

        spawn_session_worker(session, tx, cc.egui_ctx.clone());

        if let Some(wtx) = WORKER_TX.get() {
            if is_logged_in {
                let _ = wtx.send(WorkerCmd::RefreshThreads);
            } else {
                let _ = wtx.send(WorkerCmd::RequestQr);
            }
        }

        app
    }

    fn palette(&self) -> ThemePalette {
        if self.dark_mode {
            ThemePalette::dark()
        } else {
            ThemePalette::light()
        }
    }

    fn persist_threads_async(&self) {
        let snapshot = self.threads.clone();
        if let Some(wtx) = WORKER_TX.get() {
            let _ = wtx.send(WorkerCmd::PersistThreads(snapshot));
        }
    }

    fn check_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                GuiEvent::QrReady { qr_image, .. } => {
                    self.qr_texture = Some(ctx.load_texture(
                        "qr_code_tex",
                        qr_image,
                        TextureOptions::NEAREST,
                    ));
                    self.qr_status = "Mở ứng dụng Zalo trên điện thoại và quét mã QR".into();
                    self.login_in_progress = false;
                }
                GuiEvent::QrScanned { name } => {
                    self.qr_status =
                        format!("Đã quét bởi {name} — Vui lòng bấm 'Đăng nhập' trên điện thoại…");
                }
                GuiEvent::LoginSuccess { name, uin } => {
                    self.is_logged_in = true;
                    self.user_name = name;
                    self.user_uin = uin;
                    self.login_in_progress = false;
                    self.status_msg = "Trực tuyến · Đồng bộ thời gian thực".into();
                }
                GuiEvent::LoginFailed(err) => {
                    self.login_in_progress = false;
                    self.login_status_msg = format!("Lỗi đăng nhập: {err}");
                }
                GuiEvent::ThreadsUpdated(updates) => {
                    if self.is_logged_in && !updates.is_empty() {
                        let mut has_changes = false;
                        for nt in updates {
                            if let Some(existing) = self.threads.iter_mut().find(|t| t.id == nt.id)
                            {
                                let is_generic_name = nt.name.is_empty()
                                    || nt.name.starts_with("Người dùng ")
                                    || nt.name.starts_with("Nhóm ");
                                if !nt.name.is_empty()
                                    && (!is_generic_name || existing.name.is_empty())
                                    && existing.name != nt.name
                                {
                                    existing.name = nt.name;
                                    has_changes = true;
                                }
                                if !nt.preview.is_empty() && existing.preview != nt.preview {
                                    existing.preview = nt.preview;
                                    existing.time = nt.time;
                                    has_changes = true;
                                }
                                for msg in nt.messages {
                                    if let Some(existing_msg) =
                                        existing.messages.iter_mut().find(|m| {
                                            (!m.id.is_empty()
                                                && !msg.id.is_empty()
                                                && m.id == msg.id)
                                                || (m.text == msg.text
                                                    && m.is_outgoing == msg.is_outgoing
                                                    && (m.time == msg.time
                                                        || (m.ts != 0
                                                            && msg.ts != 0
                                                            && (m.ts - msg.ts).abs() < 5000)))
                                        })
                                    {
                                        if existing_msg.id.starts_with("out_")
                                            && !msg.id.is_empty()
                                            && !msg.id.starts_with("out_")
                                        {
                                            existing_msg.id = msg.id;
                                            has_changes = true;
                                        }
                                        if msg.ts > 0 && existing_msg.ts != msg.ts {
                                            existing_msg.ts = msg.ts;
                                            existing_msg.time = msg.time;
                                            has_changes = true;
                                        }
                                        if existing_msg.sticker_id.is_none()
                                            && msg.sticker_id.is_some()
                                        {
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
                            self.persist_threads_async();
                        }
                    }
                    self.status_msg = format!("Trực tuyến · Cập nhật {}", crate::ui::clock());
                }
                GuiEvent::SessionExpired(reason) => {
                    self.is_logged_in = false;
                    self.qr_texture = None;
                    self.qr_status = format!(
                        "{reason} — Quét mã QR bên dưới để khôi phục phiên trực tuyến."
                    );
                    self.status_msg = format!("{reason} (Lịch sử tin nhắn vẫn được bảo toàn)");
                }
                GuiEvent::LoggedOut => {
                    self.is_logged_in = false;
                    self.user_name.clear();
                    self.user_uin.clear();
                    self.threads.clear();
                    self.selected_thread_index = 0;
                    self.qr_texture = None;
                    self.qr_status = "Đang xin mã QR mới…".into();
                    self.status_msg = "Đã đăng xuất".into();
                }
                GuiEvent::MessageSent {
                    thread_id,
                    text,
                    msg_id,
                } => {
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
                        self.persist_threads_async();
                    }
                    self.status_msg = format!("Đã gửi tin nhắn lúc {}", crate::ui::clock());
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
                        self.persist_threads_async();
                    }
                    self.status_msg = format!("Đã gửi nhãn dán lúc {}", crate::ui::clock());
                }
                GuiEvent::StickerImageReady { key, image } => {
                    let tex = ctx.load_texture(&key, image, TextureOptions::LINEAR);
                    self.sticker_textures.insert(key, tex);
                }
                GuiEvent::SendFailed {
                    thread_id: _,
                    text,
                    error,
                } => {
                    if self.input_message.is_empty() && !text.is_empty() {
                        self.input_message = text;
                    }
                    self.status_msg = format!("Không gửi được: {error}");
                }
                GuiEvent::StatusUpdate(msg) => {
                    self.status_msg = msg;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Lightweight Background Session Worker (2-thread Tokio pool, off-UI disk I/O)
// ---------------------------------------------------------------------------

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
    },
    SendMessage {
        to_id: String,
        text: String,
        is_group: bool,
    },
    SendSticker {
        to_id: String,
        sticker_id: u64,
        cate_id: u64,
        sticker_type: u32,
        is_group: bool,
    },
    FetchStickerImage {
        key: String,
        url: String,
    },
    PersistThreads(Vec<Thread>),
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
    let poll_state =
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
                    let _ = crate::login::get_server_info(&s).await;
                }

                let mut current_qr_task: Option<tokio::task::JoinHandle<()>> = None;

                while let Some(cmd) = cmd_rx.recv().await {
                    match cmd {
                        WorkerCmd::PersistThreads(threads_snapshot) => {
                            tokio::task::spawn_blocking(move || {
                                if let Ok(mut store) =
                                    crate::store::MessageStore::open("zalo_history.db")
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
                                                        let _ = s.save_to_file("session.json");

                                                        if let Ok(info) =
                                                            crate::login::get_login_info(&s).await
                                                        {
                                                            if let Some(cfg) = info.get("data") {
                                                                crate::login::apply_login_config(
                                                                    &mut s, cfg,
                                                                );
                                                                s.apply_server_info(cfg);
                                                                let _ =
                                                                    s.save_to_file("session.json");
                                                            }
                                                        }

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
                                                        let _ = gui_tx_clone.send(
                                                            GuiEvent::LoginSuccess {
                                                                name: dname,
                                                                uin,
                                                            },
                                                        );
                                                        ctx_clone.request_repaint();

                                                        let mut list = crate::chat::get_friends(&s)
                                                            .await
                                                            .unwrap_or_default();
                                                        crate::store::load_history_into_threads(
                                                            "zalo_history.db",
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
                                                        ctx_clone.request_repaint();
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
                            tokio::spawn(async move {
                                let s = session_clone.lock().await.clone();
                                if s.login.is_some() {
                                    if !s.validate_session().await {
                                        let _ = std::fs::remove_file("session.json");
                                        {
                                            let mut sl = session_clone.lock().await;
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
                                    let mut list =
                                        crate::chat::get_friends(&s).await.unwrap_or_default();
                                    crate::store::load_history_into_threads(
                                        "zalo_history.db",
                                        &mut list,
                                        200,
                                    );
                                    {
                                        let mut ps = poll_clone.lock().await;
                                        let _ =
                                            crate::chat::poll_new_messages(&s, &mut ps, &mut list)
                                                .await;
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
                                        temp_threads.retain(|t| !t.messages.is_empty());
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
                                    if let Ok(resp) =
                                        crate::chat::get_group_history(&s, &group_id, 50).await
                                    {
                                        crate::chat::merge_group_history(&mut t, &resp);
                                    }
                                    if let Ok(cm_resp) = crate::chat::get_cloud_messages(
                                        &s,
                                        &group_id,
                                        "10000000000000000",
                                        false,
                                        50,
                                    )
                                    .await
                                    {
                                        crate::chat::merge_group_history(&mut t, &cm_resp);
                                    }
                                    if !t.messages.is_empty() {
                                        let _ = gui_tx_clone.send(GuiEvent::ThreadsUpdated(vec![t]));
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
                                                let _ = s.save_to_file("session.json");

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
                                                let _ = gui_tx_clone
                                                    .send(GuiEvent::LoginSuccess { name, uin });
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

// ---------------------------------------------------------------------------
// Main eframe::App Implementation
// ---------------------------------------------------------------------------

impl eframe::App for ZaloGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.theme_dirty {
            self.palette().apply_to_ctx(ctx);
            self.theme_dirty = false;
        }

        self.check_events(ctx);

        // Non-overlapping long-poll trigger; wakes only every 1.2s to keep idle CPU ~0%
        if self.is_logged_in && self.last_poll.elapsed() >= Duration::from_millis(1200) {
            self.last_poll = std::time::Instant::now();
            if let Some(tx) = WORKER_TX.get() {
                let known: Vec<(String, String, bool)> = self
                    .threads
                    .iter()
                    .map(|t| (t.id.clone(), t.name.clone(), t.is_group))
                    .collect();
                let _ = tx.send(WorkerCmd::PollMessages {
                    known_threads: known,
                });
            }
        }

        ctx.request_repaint_after(Duration::from_millis(1200));

        let pal = self.palette();

        // Top Navigation Bar
        egui::TopBottomPanel::top("top_bar")
            .frame(
                egui::Frame::none()
                    .fill(pal.bg_sidebar)
                    .inner_margin(Margin::symmetric(16.0, 9.0))
                    .stroke(Stroke::new(1.0_f32, pal.border)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Brand Logo Pill
                    let (logo_rect, _) =
                        ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
                    ui.painter()
                        .rect_filled(logo_rect, Rounding::same(7.0), pal.primary);
                    ui.painter().text(
                        logo_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Z",
                        FontId::proportional(16.5),
                        Color32::WHITE,
                    );

                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Zalo")
                            .color(pal.text_primary)
                            .size(17.5)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("Native Desktop")
                            .color(pal.text_muted)
                            .size(12.5),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Theme toggle button (Vector Sun/Moon icon)
                        let (theme_rect, theme_resp) =
                            ui.allocate_exact_size(Vec2::new(30.0, 28.0), Sense::click());
                        let theme_bg = if theme_resp.hovered() {
                            pal.bg_hover
                        } else {
                            Color32::TRANSPARENT
                        };
                        ui.painter()
                            .rect_filled(theme_rect, Rounding::same(6.0), theme_bg);
                        draw_icon_theme(
                            ui.painter(),
                            theme_rect.center(),
                            16.0,
                            self.dark_mode,
                            pal.text_secondary,
                        );
                        if theme_resp
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .on_hover_text(if self.dark_mode {
                                "Chuyển sang giao diện Sáng (Light Mode)"
                            } else {
                                "Chuyển sang giao diện Tối (Dark Mode)"
                            })
                            .clicked()
                        {
                            self.dark_mode = !self.dark_mode;
                            self.theme_dirty = true;
                        }

                        if self.dev_mode {
                            let dev_btn = egui::Button::new(
                                RichText::new("DEV INSPECTOR")
                                    .size(11.0)
                                    .color(Color32::BLACK)
                                    .strong(),
                            )
                            .fill(pal.warning)
                            .rounding(Rounding::same(6.0));
                            if ui
                                .add(dev_btn)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                self.dev_panel_open = !self.dev_panel_open;
                                if self.dev_panel_open {
                                    self.scanner_findings = logger::scan_logs();
                                }
                            }
                        }

                        if self.is_logged_in {
                            let logout_btn = egui::Button::new(
                                RichText::new("Đăng xuất")
                                    .size(12.5)
                                    .color(pal.danger)
                                    .strong(),
                            )
                            .fill(pal.danger.linear_multiply(0.12))
                            .stroke(Stroke::new(1.0_f32, pal.danger.linear_multiply(0.35)))
                            .rounding(Rounding::same(6.0));

                            if ui
                                .add(logout_btn)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .on_hover_text("Đăng xuất hoặc quét lại mã QR")
                                .clicked()
                            {
                                self.is_logged_in = false;
                                self.user_name.clear();
                                self.user_uin.clear();
                                self.threads.clear();
                                self.selected_thread_index = 0;
                                self.qr_texture = None;
                                self.login_status_msg.clear();
                                self.qr_status = "Đang xin mã QR mới…".into();
                                self.status_msg = "Đã đăng xuất".into();
                                let _ = std::fs::remove_file("session.json");
                                let _ = std::fs::remove_file("threads_cache.json");
                                if let Some(tx) = WORKER_TX.get() {
                                    let _ = tx.send(WorkerCmd::Logout);
                                    let _ = tx.send(WorkerCmd::RequestQr);
                                }
                            }

                            ui.add_space(6.0);
                            // User profile chip with online dot
                            egui::Frame::none()
                                .fill(pal.bg_elevated)
                                .rounding(Rounding::same(16.0))
                                .stroke(Stroke::new(1.0_f32, pal.border))
                                .inner_margin(Margin::symmetric(10.0, 4.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let (dot_rect, _) = ui.allocate_exact_size(
                                            Vec2::new(8.0, 8.0),
                                            Sense::hover(),
                                        );
                                        ui.painter().circle_filled(
                                            dot_rect.center(),
                                            4.0,
                                            pal.accent_online,
                                        );
                                        let display = if self.user_name.is_empty() {
                                            "Zalo User"
                                        } else {
                                            &self.user_name
                                        };
                                        ui.label(
                                            RichText::new(display)
                                                .size(12.5)
                                                .color(pal.text_primary)
                                                .strong(),
                                        );
                                    });
                                });
                        } else if !self.threads.is_empty() {
                            let hist_btn = egui::Button::new(
                                RichText::new("Xem lịch sử đã lưu")
                                    .size(12.5)
                                    .color(pal.primary)
                                    .strong(),
                            )
                            .fill(pal.primary.linear_multiply(0.12))
                            .stroke(Stroke::new(1.0_f32, pal.primary))
                            .rounding(Rounding::same(6.0));
                            if ui
                                .add(hist_btn)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                self.is_logged_in = true;
                            }
                        }
                    });
                });
            });

        // Bottom Status Bar
        egui::TopBottomPanel::bottom("bottom_bar")
            .frame(
                egui::Frame::none()
                    .fill(pal.bg_sidebar)
                    .inner_margin(Margin::symmetric(16.0, 5.0))
                    .stroke(Stroke::new(1.0_f32, pal.border)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (dot_rect, _) =
                        ui.allocate_exact_size(Vec2::new(7.0, 7.0), Sense::hover());
                    let dot_color = if self.is_logged_in {
                        pal.accent_online
                    } else {
                        pal.warning
                    };
                    ui.painter().circle_filled(dot_rect.center(), 3.5, dot_color);
                    ui.label(
                        RichText::new(&self.status_msg)
                            .size(11.5)
                            .color(pal.text_secondary),
                    );
                    if self.is_logged_in {
                        ui.label(
                            RichText::new(format!("· {} hội thoại", self.threads.len()))
                                .size(11.5)
                                .color(pal.text_muted),
                        );
                    }

                    if self.dev_mode {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let label = if self.dev_panel_open {
                                "Đóng Dev Inspector"
                            } else {
                                "Mở Dev Inspector"
                            };
                            if ui
                                .small_button(label)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                self.dev_panel_open = !self.dev_panel_open;
                                if self.dev_panel_open {
                                    self.scanner_findings = logger::scan_logs();
                                }
                            }
                        });
                    }
                });
            });

        // Dev Inspector Panel
        if self.dev_mode && self.dev_panel_open {
            egui::TopBottomPanel::bottom("dev_inspector")
                .resizable(true)
                .default_height(250.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong("Dev Inspector & Protocol Scanner");
                        ui.separator();
                        ui.selectable_value(
                            &mut self.dev_tab,
                            DevTab::Logs,
                            "Lưu lượng mạng (Logs)",
                        );
                        ui.selectable_value(
                            &mut self.dev_tab,
                            DevTab::Scanner,
                            "Quét lỗi hệ thống (Scanner)",
                        );
                        ui.selectable_value(
                            &mut self.dev_tab,
                            DevTab::CryptoState,
                            "Trạng thái Crypto & Keys",
                        );

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

        // Main Workspace
        if !self.is_logged_in {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(pal.bg_chat_canvas))
                .show(ctx, |ui| {
                    self.render_login_screen(ui, &pal);
                });
        } else {
            self.render_chat_workspace(ctx, &pal);
        }
    }
}

// ---------------------------------------------------------------------------
// Screen Renderers
// ---------------------------------------------------------------------------

impl ZaloGuiApp {
    fn render_login_screen(&mut self, ui: &mut egui::Ui, pal: &ThemePalette) {
        ui.vertical_centered(|ui| {
            let avail_h = ui.available_height();
            ui.add_space((avail_h * 0.08).max(20.0));

            ui.label(
                RichText::new("Đăng nhập Zalo")
                    .color(pal.primary)
                    .size(26.0)
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Khởi chạy nhẹ, bảo mật và đồng bộ tin nhắn thời gian thực")
                    .color(pal.text_secondary)
                    .size(13.5),
            );
            ui.add_space(18.0);

            egui::Frame::none()
                .fill(pal.bg_elevated)
                .rounding(Rounding::same(16.0))
                .stroke(Stroke::new(1.0_f32, pal.border))
                .inner_margin(Margin::symmetric(28.0, 24.0))
                .show(ui, |ui| {
                    ui.set_width(360.0);
                    ui.vertical_centered(|ui| {
                        // Segmented Tab Switcher
                        egui::Frame::none()
                            .fill(pal.bg_input)
                            .rounding(Rounding::same(10.0))
                            .inner_margin(Margin::same(4.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let tab_w = (ui.available_width() - 8.0) / 2.0;
                                    for (tab, title) in [
                                        (LoginTab::QrCode, "Quét mã QR"),
                                        (LoginTab::Password, "Số điện thoại"),
                                    ] {
                                        let active = self.login_tab == tab;
                                        let btn = egui::Button::new(
                                            RichText::new(title)
                                                .size(13.0)
                                                .color(if active {
                                                    Color32::WHITE
                                                } else {
                                                    pal.text_secondary
                                                })
                                                .strong(),
                                        )
                                        .fill(if active {
                                            pal.primary
                                        } else {
                                            Color32::TRANSPARENT
                                        })
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(7.0))
                                        .min_size(Vec2::new(tab_w, 32.0));

                                        if ui
                                            .add(btn)
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            self.login_tab = tab;
                                        }
                                    }
                                });
                            });

                        ui.add_space(18.0);

                        match self.login_tab {
                            LoginTab::QrCode => {
                                if let Some(tex) = &self.qr_texture {
                                    egui::Frame::none()
                                        .fill(Color32::WHITE)
                                        .rounding(Rounding::same(12.0))
                                        .stroke(Stroke::new(1.0_f32, pal.border))
                                        .inner_margin(Margin::same(14.0))
                                        .show(ui, |ui| {
                                            ui.image((tex.id(), Vec2::new(240.0, 240.0)));
                                        });
                                } else {
                                    let (rect, _) = ui.allocate_exact_size(
                                        Vec2::new(268.0, 268.0),
                                        Sense::hover(),
                                    );
                                    ui.painter().rect_filled(
                                        rect,
                                        Rounding::same(12.0),
                                        pal.bg_input,
                                    );
                                    ui.painter().rect_stroke(
                                        rect,
                                        Rounding::same(12.0),
                                        Stroke::new(1.0_f32, pal.border),
                                    );
                                    ui.painter().text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "Đang tải mã QR…",
                                        FontId::proportional(14.5),
                                        pal.text_secondary,
                                    );
                                }

                                ui.add_space(14.0);
                                ui.label(
                                    RichText::new(&self.qr_status)
                                        .color(pal.text_primary)
                                        .size(13.5),
                                );
                                ui.add_space(12.0);

                                let refresh_btn = egui::Button::new(
                                    RichText::new("Tạo / Làm mới mã QR")
                                        .size(13.5)
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .fill(pal.primary)
                                .rounding(Rounding::same(8.0))
                                .min_size(Vec2::new(200.0, 36.0));

                                if ui
                                    .add(refresh_btn)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    self.login_in_progress = true;
                                    self.qr_status = "Đang xin mã QR…".into();
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::RequestQr);
                                    }
                                }
                            }
                            LoginTab::Password => {
                                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                    ui.label(
                                        RichText::new("Số điện thoại")
                                            .size(13.0)
                                            .color(pal.text_secondary)
                                            .strong(),
                                    );
                                    ui.add_space(4.0);
                                    ui.add_sized(
                                        [ui.available_width(), 34.0],
                                        egui::TextEdit::singleline(&mut self.phone_input)
                                            .hint_text("Ví dụ: 0912345678"),
                                    );
                                    ui.add_space(12.0);

                                    ui.label(
                                        RichText::new("Mật khẩu")
                                            .size(13.0)
                                            .color(pal.text_secondary)
                                            .strong(),
                                    );
                                    ui.add_space(4.0);
                                    ui.add_sized(
                                        [ui.available_width(), 34.0],
                                        egui::TextEdit::singleline(&mut self.password_input)
                                            .password(true)
                                            .hint_text("Nhập mật khẩu Zalo"),
                                    );
                                    ui.add_space(16.0);

                                    if !self.login_status_msg.is_empty() {
                                        ui.label(
                                            RichText::new(&self.login_status_msg)
                                                .color(pal.danger)
                                                .size(12.5),
                                        );
                                        ui.add_space(8.0);
                                    }

                                    let btn = egui::Button::new(
                                        RichText::new("Đăng nhập")
                                            .color(Color32::WHITE)
                                            .size(14.0)
                                            .strong(),
                                    )
                                    .fill(pal.primary)
                                    .rounding(Rounding::same(8.0))
                                    .min_size(Vec2::new(ui.available_width(), 38.0));

                                    if ui
                                        .add_enabled(!self.login_in_progress, btn)
                                        .on_hover_cursor(CursorIcon::PointingHand)
                                        .clicked()
                                    {
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
                            }
                        }
                    });
                });
        });
    }

    fn render_chat_workspace(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        // Left Resizable Sidebar: Conversation List
        egui::SidePanel::left("conversation_sidebar")
            .resizable(true)
            .default_width(310.0)
            .width_range(250.0..=420.0)
            .frame(
                egui::Frame::none()
                    .fill(pal.bg_sidebar)
                    .stroke(Stroke::new(1.0_f32, pal.border)),
            )
            .show(ctx, |ui| {
                // Search & Refresh Header
                egui::Frame::none()
                    .inner_margin(Margin::symmetric(12.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Search input pill with vector search icon
                            let avail_w = (ui.available_width() - 38.0).max(120.0);
                            egui::Frame::none()
                                .fill(pal.bg_input)
                                .rounding(Rounding::same(8.0))
                                .stroke(Stroke::new(1.0_f32, pal.border))
                                .inner_margin(Margin::symmetric(8.0, 5.0))
                                .show(ui, |ui| {
                                    ui.set_width(avail_w - 18.0);
                                    ui.horizontal(|ui| {
                                        let (icon_rect, _) = ui.allocate_exact_size(
                                            Vec2::new(16.0, 16.0),
                                            Sense::hover(),
                                        );
                                        draw_icon_search(
                                            ui.painter(),
                                            icon_rect.center(),
                                            14.0,
                                            pal.text_muted,
                                        );
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.search_query)
                                                .frame(false)
                                                .hint_text("Tìm kiếm hội thoại…")
                                                .desired_width(avail_w - 56.0),
                                        );
                                        if !self.search_query.is_empty() {
                                            if ui
                                                .small_button("×")
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .on_hover_text("Xóa tìm kiếm")
                                                .clicked()
                                            {
                                                self.search_query.clear();
                                            }
                                        }
                                    });
                                });

                            // Refresh button with vector icon
                            let (ref_rect, ref_resp) =
                                ui.allocate_exact_size(Vec2::new(30.0, 30.0), Sense::click());
                            let ref_bg = if ref_resp.hovered() {
                                pal.bg_hover
                            } else {
                                pal.bg_input
                            };
                            ui.painter()
                                .rect_filled(ref_rect, Rounding::same(8.0), ref_bg);
                            ui.painter().rect_stroke(
                                ref_rect,
                                Rounding::same(8.0),
                                Stroke::new(1.0_f32, pal.border),
                            );
                            draw_icon_refresh(
                                ui.painter(),
                                ref_rect.center(),
                                15.0,
                                pal.text_secondary,
                            );
                            if ref_resp
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .on_hover_text("Đồng bộ lại danh sách hội thoại")
                                .clicked()
                            {
                                if let Some(tx) = WORKER_TX.get() {
                                    let _ = tx.send(WorkerCmd::RefreshThreads);
                                }
                            }
                        });

                        ui.add_space(8.0);

                        // Category Filter Pills (`Tất cả`, `Cá nhân`, `Nhóm`)
                        ui.horizontal(|ui| {
                            for (filter, label) in [
                                (ThreadFilter::All, "Tất cả"),
                                (ThreadFilter::Direct, "Cá nhân"),
                                (ThreadFilter::Groups, "Nhóm"),
                            ] {
                                let active = self.thread_filter == filter;
                                let pill = egui::Button::new(
                                    RichText::new(label)
                                        .size(12.0)
                                        .color(if active {
                                            Color32::WHITE
                                        } else {
                                            pal.text_secondary
                                        })
                                        .strong(),
                                )
                                .fill(if active {
                                    pal.primary
                                } else {
                                    pal.bg_input
                                })
                                .stroke(Stroke::new(
                                    1.0_f32,
                                    if active { pal.primary } else { pal.border },
                                ))
                                .rounding(Rounding::same(14.0))
                                .min_size(Vec2::new(64.0, 24.0));

                                if ui
                                    .add(pill)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    self.thread_filter = filter;
                                }
                            }
                        });
                    });

                ui.add(egui::Separator::default().spacing(1.0));

                // Virtualized / Filtered Thread List
                let query = self.search_query.trim().to_lowercase();
                let filtered_indices: Vec<usize> = self
                    .threads
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| {
                        let pass_filter = match self.thread_filter {
                            ThreadFilter::All => true,
                            ThreadFilter::Direct => !t.is_group,
                            ThreadFilter::Groups => t.is_group,
                        };
                        pass_filter && (query.is_empty() || t.name.to_lowercase().contains(&query))
                    })
                    .map(|(i, _)| i)
                    .collect();

                let mut clicked_thread: Option<(usize, String, bool)> = None;

                egui::ScrollArea::vertical()
                    .id_salt("threads_list_scroll")
                    .auto_shrink([false, false])
                    .show_rows(ui, 62.0, filtered_indices.len(), |ui, row_range| {
                        ui.add_space(4.0);
                        for row_idx in row_range {
                            let i = filtered_indices[row_idx];
                            let t = &self.threads[i];
                            let is_selected = i == self.selected_thread_index;

                            let row_width = ui.available_width() - 12.0;
                            let (row_rect, row_resp) = ui.allocate_exact_size(
                                Vec2::new(row_width, 58.0),
                                Sense::click(),
                            );
                            let row_rect = row_rect.translate(Vec2::new(6.0, 0.0));

                            let bg = if is_selected {
                                pal.bg_selected
                            } else if row_resp.hovered() {
                                pal.bg_hover
                            } else {
                                Color32::TRANSPARENT
                            };

                            ui.painter()
                                .rect_filled(row_rect, Rounding::same(10.0), bg);

                            // Left active indicator bar
                            if is_selected {
                                let bar_rect = Rect::from_min_size(
                                    Pos2::new(row_rect.left() + 2.0, row_rect.top() + 12.0),
                                    Vec2::new(3.5, 34.0),
                                );
                                ui.painter().rect_filled(
                                    bar_rect,
                                    Rounding::same(2.0),
                                    pal.primary,
                                );
                            }

                            // Avatar circle
                            let avatar_center =
                                Pos2::new(row_rect.left() + 28.0, row_rect.center().y);
                            let avatar_col = avatar_color_for(&t.name, t.is_group);
                            ui.painter().circle_filled(avatar_center, 19.0, avatar_col);

                            let initial = t
                                .name
                                .chars()
                                .next()
                                .unwrap_or('Z')
                                .to_uppercase()
                                .to_string();
                            ui.painter().text(
                                avatar_center,
                                egui::Align2::CENTER_CENTER,
                                initial,
                                FontId::proportional(15.5),
                                Color32::WHITE,
                            );

                            // Online / Group badge on avatar corner
                            let badge_pos =
                                Pos2::new(avatar_center.x + 13.5, avatar_center.y + 13.5);
                            ui.painter().circle_filled(badge_pos, 5.5, pal.bg_sidebar);
                            ui.painter().circle_filled(
                                badge_pos,
                                4.0,
                                if t.is_group {
                                    pal.primary
                                } else {
                                    pal.accent_online
                                },
                            );

                            // Thread Name
                            let text_left = row_rect.left() + 56.0;
                            let name_color = if is_selected {
                                if pal.is_dark {
                                    Color32::WHITE
                                } else {
                                    pal.primary
                                }
                            } else {
                                pal.text_primary
                            };
                            let display_name = truncate_chars(&t.name, 22);
                            ui.painter().text(
                                Pos2::new(text_left, row_rect.top() + 13.0),
                                egui::Align2::LEFT_TOP,
                                display_name,
                                FontId::proportional(14.0),
                                name_color,
                            );

                            // Timestamp (top-right)
                            if !t.time.is_empty() {
                                ui.painter().text(
                                    Pos2::new(row_rect.right() - 10.0, row_rect.top() + 14.0),
                                    egui::Align2::RIGHT_TOP,
                                    &t.time,
                                    FontId::proportional(11.0),
                                    pal.text_muted,
                                );
                            }

                            // Snippet (bottom)
                            let snippet = if t.preview.is_empty() {
                                std::borrow::Cow::Borrowed("Chưa có tin nhắn")
                            } else {
                                truncate_chars(&t.preview, 28)
                            };
                            ui.painter().text(
                                Pos2::new(text_left, row_rect.top() + 34.0),
                                egui::Align2::LEFT_TOP,
                                snippet,
                                FontId::proportional(12.5),
                                pal.text_secondary,
                            );

                            if row_resp
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                clicked_thread = Some((i, t.id.clone(), t.is_group));
                            }
                        }
                    });

                if let Some((idx, tid, is_group)) = clicked_thread {
                    self.selected_thread_index = idx;
                    if is_group {
                        if let Some(tx) = WORKER_TX.get() {
                            let _ = tx.send(WorkerCmd::LoadGroupHistory { group_id: tid });
                        }
                    }
                }
            });

        // Right Central Panel: Active Conversation Viewport
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(pal.bg_chat_canvas))
            .show(ctx, |ui| {
                // Extract lightweight metadata without cloning the entire Vec<ChatMessage>
                let thread_meta = self.threads.get(self.selected_thread_index).map(|t| {
                    (
                        t.id.clone(),
                        t.name.clone(),
                        t.is_group,
                        t.messages.len(),
                    )
                });

                if let Some((thread_id, thread_name, is_group, msg_count)) = thread_meta {
                    // 1. Conversation Header Bar
                    egui::Frame::none()
                        .fill(pal.bg_sidebar)
                        .inner_margin(Margin::symmetric(18.0, 10.0))
                        .stroke(Stroke::new(1.0_f32, pal.border))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // Header Avatar
                                let (av_rect, _) =
                                    ui.allocate_exact_size(Vec2::new(38.0, 38.0), Sense::hover());
                                let av_col = avatar_color_for(&thread_name, is_group);
                                ui.painter().circle_filled(av_rect.center(), 19.0, av_col);
                                let initial = thread_name
                                    .chars()
                                    .next()
                                    .unwrap_or('Z')
                                    .to_uppercase()
                                    .to_string();
                                ui.painter().text(
                                    av_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    initial,
                                    FontId::proportional(16.0),
                                    Color32::WHITE,
                                );

                                ui.add_space(4.0);
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    ui.label(
                                        RichText::new(&thread_name)
                                            .size(15.5)
                                            .color(pal.text_primary)
                                            .strong(),
                                    );
                                    ui.horizontal(|ui| {
                                        let kind_str = if is_group {
                                            "Nhóm trò chuyện"
                                        } else {
                                            "Trực tuyến"
                                        };
                                        ui.label(
                                            RichText::new(format!(
                                                "{kind_str} · {msg_count} tin nhắn"
                                            ))
                                            .size(12.0)
                                            .color(pal.accent_online),
                                        );
                                    });
                                });

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(format!("ID: {thread_id}"))
                                            .size(11.0)
                                            .color(pal.text_muted),
                                    );
                                    if is_group {
                                        let sync_btn = egui::Button::new(
                                            RichText::new("Đồng bộ lịch sử nhóm")
                                                .size(12.0)
                                                .color(pal.primary)
                                                .strong(),
                                        )
                                        .fill(pal.primary.linear_multiply(0.12))
                                        .stroke(Stroke::new(1.0_f32, pal.primary))
                                        .rounding(Rounding::same(6.0));
                                        if ui
                                            .add(sync_btn)
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::LoadGroupHistory {
                                                    group_id: thread_id.clone(),
                                                });
                                            }
                                        }
                                    }
                                });
                            });
                        });

                    // 2. Scrollable Messages Area (zero-clone borrow of `self.threads[selected]`)
                    let picker_reserve = if self.sticker_picker_open { 200.0 } else { 0.0 };
                    let msg_area_height =
                        (ui.available_height() - 62.0 - picker_reserve).max(100.0);

                    egui::ScrollArea::vertical()
                        .id_salt(Id::new(("chat_scroll", &thread_id)))
                        .max_height(msg_area_height)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            egui::Frame::none()
                                .inner_margin(Margin::symmetric(18.0, 12.0))
                                .show(ui, |ui| {
                                    if let Some(thread) =
                                        self.threads.get(self.selected_thread_index)
                                    {
                                        if thread.messages.is_empty() {
                                            ui.vertical_centered(|ui| {
                                                ui.add_space(60.0);
                                                ui.label(
                                                    RichText::new(
                                                        "Chưa có tin nhắn trong cuộc trò chuyện này.",
                                                    )
                                                    .size(14.0)
                                                    .color(pal.text_muted),
                                                );
                                            });
                                        } else {
                                            for (idx, msg) in thread.messages.iter().enumerate() {
                                                let prev_same_sender = idx > 0
                                                    && thread.messages[idx - 1].is_outgoing
                                                        == msg.is_outgoing
                                                    && thread.messages[idx - 1].sender
                                                        == msg.sender;
                                                render_chat_bubble(
                                                    ui,
                                                    msg,
                                                    prev_same_sender,
                                                    &thread_name,
                                                    pal,
                                                    &self.sticker_textures,
                                                    &mut self.loading_stickers,
                                                );
                                                ui.add_space(if prev_same_sender {
                                                    3.0
                                                } else {
                                                    8.0
                                                });
                                            }
                                        }
                                    }
                                });
                        });

                    // 3. Optional Sticker Picker Drawer
                    if self.sticker_picker_open {
                        self.render_sticker_picker(ui, &thread_id, is_group, pal);
                    }

                    // 4. Bottom Message Composer Bar
                    egui::Frame::none()
                        .fill(pal.bg_sidebar)
                        .inner_margin(Margin::symmetric(14.0, 10.0))
                        .stroke(Stroke::new(1.0_f32, pal.border))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // Sticker Picker Button (Vector icon + label)
                                let (stk_rect, stk_resp) = ui.allocate_exact_size(
                                    Vec2::new(102.0, 36.0),
                                    Sense::click(),
                                );
                                let stk_bg = if self.sticker_picker_open {
                                    pal.primary
                                } else if stk_resp.hovered() {
                                    pal.bg_hover
                                } else {
                                    pal.bg_input
                                };
                                let stk_fg = if self.sticker_picker_open {
                                    Color32::WHITE
                                } else {
                                    pal.text_primary
                                };
                                ui.painter().rect_filled(
                                    stk_rect,
                                    Rounding::same(8.0),
                                    stk_bg,
                                );
                                ui.painter().rect_stroke(
                                    stk_rect,
                                    Rounding::same(8.0),
                                    Stroke::new(
                                        1.0_f32,
                                        if self.sticker_picker_open {
                                            pal.primary
                                        } else {
                                            pal.border
                                        },
                                    ),
                                );
                                draw_icon_sticker(
                                    ui.painter(),
                                    Pos2::new(stk_rect.left() + 18.0, stk_rect.center().y),
                                    16.0,
                                    stk_fg,
                                );
                                ui.painter().text(
                                    Pos2::new(stk_rect.left() + 34.0, stk_rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    "Nhãn dán",
                                    FontId::proportional(13.0),
                                    stk_fg,
                                );
                                if stk_resp
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text("Mở bảng chọn Nhãn dán Zalo")
                                    .clicked()
                                {
                                    self.sticker_picker_open = !self.sticker_picker_open;
                                }

                                // Rounded Input Field
                                let input_w = (ui.available_width() - 94.0).max(80.0);
                                let text_edit = egui::TextEdit::singleline(&mut self.input_message)
                                    .hint_text(format!("Nhập tin nhắn tới {thread_name}…"))
                                    .margin(Margin::symmetric(12.0, 8.0))
                                    .desired_width(input_w);

                                let response = ui.add(text_edit);
                                let enter_pressed = (response.lost_focus() || response.has_focus())
                                    && ui.input(|i| i.key_pressed(Key::Enter));

                                // Send Button (Vector Paper-Plane + Label)
                                let can_send = !self.input_message.trim().is_empty();
                                let (send_rect, send_resp) =
                                    ui.allocate_exact_size(Vec2::new(82.0, 36.0), Sense::click());
                                let send_bg = if !can_send {
                                    pal.primary.linear_multiply(0.45)
                                } else if send_resp.hovered() {
                                    pal.primary_hover
                                } else {
                                    pal.primary
                                };
                                ui.painter()
                                    .rect_filled(send_rect, Rounding::same(8.0), send_bg);
                                draw_icon_send(
                                    ui.painter(),
                                    Pos2::new(send_rect.left() + 20.0, send_rect.center().y),
                                    15.0,
                                    Color32::WHITE,
                                );
                                ui.painter().text(
                                    Pos2::new(send_rect.left() + 36.0, send_rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    "Gửi",
                                    FontId::proportional(13.5),
                                    Color32::WHITE,
                                );

                                let send_clicked = send_resp
                                    .on_hover_cursor(if can_send {
                                        CursorIcon::PointingHand
                                    } else {
                                        CursorIcon::Default
                                    })
                                    .clicked();

                                if (send_clicked || enter_pressed) && can_send {
                                    let text = self.input_message.trim().to_string();
                                    self.input_message.clear();
                                    response.request_focus();
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::SendMessage {
                                            to_id: thread_id,
                                            text,
                                            is_group,
                                        });
                                    }
                                } else if enter_pressed {
                                    response.request_focus();
                                }
                            });
                        });
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(120.0);
                        ui.label(
                            RichText::new("Chọn một hội thoại bên trái để bắt đầu trò chuyện")
                                .size(15.0)
                                .color(pal.text_muted),
                        );
                    });
                }
            });
    }

    fn render_sticker_picker(
        &mut self,
        ui: &mut egui::Ui,
        thread_id: &str,
        is_group: bool,
        pal: &ThemePalette,
    ) {
        egui::Frame::none()
            .fill(pal.bg_sidebar)
            .stroke(Stroke::new(1.0_f32, pal.border))
            .inner_margin(Margin::symmetric(14.0, 8.0))
            .show(ui, |ui| {
                ui.set_height(180.0);
                ui.horizontal(|ui| {
                    for (tab, title) in [
                        (StickerTab::Popular, "Nhãn dán phổ biến"),
                        (StickerTab::Onion, "Bộ Củ hành"),
                    ] {
                        let active = self.sticker_tab == tab;
                        let btn = egui::Button::new(
                            RichText::new(title)
                                .size(12.5)
                                .color(if active {
                                    Color32::WHITE
                                } else {
                                    pal.text_secondary
                                })
                                .strong(),
                        )
                        .fill(if active { pal.primary } else { pal.bg_input })
                        .stroke(Stroke::new(
                            1.0_f32,
                            if active { pal.primary } else { pal.border },
                        ))
                        .rounding(Rounding::same(6.0));
                        if ui
                            .add(btn)
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.sticker_tab = tab;
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .small_button("Đóng")
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.sticker_picker_open = false;
                        }
                    });
                });
                ui.add_space(4.0);

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
                                let url = if item.cate_id == 0 && (1..=30).contains(&item.sticker_id)
                                {
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

                                let (cell_rect, cell_resp) =
                                    ui.allocate_exact_size(Vec2::new(68.0, 68.0), Sense::click());
                                if cell_resp.hovered() {
                                    ui.painter().rect_filled(
                                        cell_rect,
                                        Rounding::same(8.0),
                                        pal.bg_hover,
                                    );
                                    ui.painter().rect_stroke(
                                        cell_rect,
                                        Rounding::same(8.0),
                                        Stroke::new(1.0_f32, pal.primary),
                                    );
                                }

                                if let Some(tex) = self.sticker_textures.get(&key) {
                                    let img_rect = cell_rect.shrink(5.0);
                                    ui.painter().image(
                                        tex.id(),
                                        img_rect,
                                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                        Color32::WHITE,
                                    );
                                } else {
                                    ui.painter().text(
                                        cell_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        item.label,
                                        FontId::proportional(11.0),
                                        pal.text_secondary,
                                    );
                                }

                                if cell_resp
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text(format!("Gửi nhãn dán: {}", item.label))
                                    .clicked()
                                {
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

    fn render_dev_logs(&mut self, ui: &mut egui::Ui) {
        let entries = logger::get_entries();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(360.0);
                egui::ScrollArea::vertical()
                    .id_salt("dev_logs_list_scroll")
                    .show(ui, |ui| {
                        for e in entries.iter().rev() {
                            let st_str =
                                e.status.map(|s| s.to_string()).unwrap_or_else(|| "ERR".into());
                            let row = ui.selectable_label(
                                self.selected_log
                                    .as_ref()
                                    .map(|s| s.id == e.id)
                                    .unwrap_or(false),
                                format!(
                                    "[{}] {} {} ({}ms)",
                                    e.timestamp, e.method, st_str, e.latency_ms
                                ),
                            );
                            if row.clicked() {
                                self.selected_log = Some(e.clone());
                            }
                        }
                    });
            });

            ui.separator();

            ui.vertical(|ui| {
                if let Some(e) = &self.selected_log {
                    ui.strong(format!("{} {}", e.method, e.url));
                    ui.label(format!(
                        "Thời gian: {} · Độ trễ: {}ms · HTTP: {:?}",
                        e.timestamp, e.latency_ms, e.status
                    ));
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
                            ui.label(
                                RichText::new(format!("Khắc phục: {}", f.suggestion))
                                    .color(Color32::from_rgb(0, 104, 255)),
                            );
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
}

// ---------------------------------------------------------------------------
// Chat Bubble & Sticker Widget Rendering
// ---------------------------------------------------------------------------

fn is_sticker_message(msg: &ChatMessage) -> Option<(u64, u64)> {
    if let Some(sid) = msg.sticker_id {
        if sid > 0 {
            return Some((sid, msg.sticker_cate_id.unwrap_or(0)));
        }
    }
    if msg.text.trim() == "[Nhãn dán]" {
        return Some((18009, 10130));
    }
    None
}

fn render_sticker_widget(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
) -> bool {
    let Some((sid, cid)) = is_sticker_message(msg) else {
        return false;
    };

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
        ui.image((tex.id(), Vec2::new(116.0, 116.0)));
        true
    } else {
        false
    }
}

fn render_chat_bubble(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    prev_same_sender: bool,
    thread_name: &str,
    pal: &ThemePalette,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
) {
    let max_bubble_width = (ui.available_width() * 0.66).min(500.0).max(150.0);
    let has_sticker = is_sticker_message(msg).is_some();

    if msg.is_outgoing {
        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
            ui.add_space(4.0);
            let fill = if has_sticker {
                Color32::TRANSPARENT
            } else {
                pal.bubble_out_bg
            };
            let stroke = if has_sticker {
                Stroke::NONE
            } else if !pal.is_dark {
                Stroke::new(1.0_f32, pal.primary.linear_multiply(0.25))
            } else {
                Stroke::NONE
            };
            let rounding = Rounding {
                nw: 14.0,
                ne: if prev_same_sender { 14.0 } else { 4.0 },
                sw: 14.0,
                se: 14.0,
            };

            egui::Frame::none()
                .fill(fill)
                .stroke(stroke)
                .rounding(rounding)
                .inner_margin(if has_sticker {
                    Margin::same(4.0)
                } else {
                    Margin::symmetric(14.0, 9.0)
                })
                .show(ui, |ui| {
                    ui.set_max_width(max_bubble_width);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        if !render_sticker_widget(ui, msg, sticker_textures, loading_stickers) {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&msg.text)
                                        .color(pal.bubble_out_text)
                                        .size(14.5),
                                )
                                .wrap(),
                            );
                        }
                        if !msg.time.is_empty() {
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(&msg.time)
                                    .color(if has_sticker {
                                        pal.text_muted
                                    } else {
                                        pal.bubble_out_meta
                                    })
                                    .size(10.5),
                            );
                        }
                    });
                });
        });
    } else {
        ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
            // Sender Avatar (only on first message of a consecutive group)
            if !prev_same_sender {
                let display_sender = if !msg.sender.is_empty() && msg.sender != "0" {
                    &msg.sender
                } else {
                    thread_name
                };
                let (av_rect, _) =
                    ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
                let av_col = avatar_color_for(display_sender, false);
                ui.painter().circle_filled(av_rect.center(), 14.0, av_col);
                let initial = display_sender
                    .chars()
                    .next()
                    .unwrap_or('U')
                    .to_uppercase()
                    .to_string();
                ui.painter().text(
                    av_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    initial,
                    FontId::proportional(12.5),
                    Color32::WHITE,
                );
            } else {
                ui.add_space(28.0);
            }

            let fill = if has_sticker {
                Color32::TRANSPARENT
            } else {
                pal.bubble_in_bg
            };
            let stroke = if has_sticker {
                Stroke::NONE
            } else {
                Stroke::new(1.0_f32, pal.bubble_in_border)
            };
            let rounding = Rounding {
                nw: if prev_same_sender { 14.0 } else { 4.0 },
                ne: 14.0,
                sw: 14.0,
                se: 14.0,
            };

            egui::Frame::none()
                .fill(fill)
                .stroke(stroke)
                .rounding(rounding)
                .inner_margin(if has_sticker {
                    Margin::same(4.0)
                } else {
                    Margin::symmetric(14.0, 9.0)
                })
                .show(ui, |ui| {
                    ui.set_max_width(max_bubble_width);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        if !prev_same_sender && !msg.sender.is_empty() && msg.sender != "0" {
                            ui.label(
                                RichText::new(&msg.sender)
                                    .color(pal.primary)
                                    .size(12.0)
                                    .strong(),
                            );
                            ui.add_space(2.0);
                        }
                        if !render_sticker_widget(ui, msg, sticker_textures, loading_stickers) {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&msg.text)
                                        .color(pal.bubble_in_text)
                                        .size(14.5),
                                )
                                .wrap(),
                            );
                        }
                        if !msg.time.is_empty() {
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(&msg.time)
                                    .color(pal.bubble_in_meta)
                                    .size(10.5),
                            );
                        }
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
