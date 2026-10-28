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
            bg_app: Color32::from_rgb(13, 19, 31),         // #0D131F Left Rail
            bg_sidebar: Color32::from_rgb(17, 24, 39),     // #111827 Sidebar & Header
            bg_chat_canvas: Color32::from_rgb(11, 15, 23), // #0B0F17 Deep Chat Canvas
            bg_elevated: Color32::from_rgb(30, 41, 59),    // #1E293B Slate 800
            bg_hover: Color32::from_rgb(30, 41, 59),       // #1E293B
            bg_selected: Color32::from_rgb(30, 41, 59),    // #1E293B Active Conversation Row
            bg_input: Color32::from_rgb(15, 23, 42),       // #0F172A
            border: Color32::from_rgb(30, 41, 59),         // #1E293B Subtle divider
            text_primary: Color32::from_rgb(248, 250, 252), // #F8FAFC (15.8:1 contrast)
            text_secondary: Color32::from_rgb(148, 163, 184), // #94A3B8 (6.4:1 contrast)
            text_muted: Color32::from_rgb(100, 116, 139),  // #64748B
            primary: Color32::from_rgb(37, 99, 235),       // #2563EB Primary Blue
            primary_hover: Color32::from_rgb(59, 130, 246), // #3B82F6
            accent_online: Color32::from_rgb(16, 185, 129), // #10B981 Emerald
            danger: Color32::from_rgb(239, 68, 68),        // #EF4444
            warning: Color32::from_rgb(245, 158, 11),      // #F59E0B
            bubble_out_bg: Color32::from_rgb(0, 88, 218),  // Rich Zalo Royal Blue
            bubble_out_text: Color32::from_rgb(248, 250, 252),
            bubble_out_meta: Color32::from_rgb(191, 219, 254),
            bubble_in_bg: Color32::from_rgb(26, 36, 54),   // Elevated Slate Card
            bubble_in_border: Color32::from_rgb(48, 63, 90),
            bubble_in_text: Color32::from_rgb(241, 245, 249),
            bubble_in_meta: Color32::from_rgb(148, 163, 184),
        }
    }

    fn light() -> Self {
        Self {
            is_dark: false,
            bg_app: Color32::from_rgb(255, 255, 255),      // #FFFFFF
            bg_sidebar: Color32::from_rgb(255, 255, 255),  // #FFFFFF
            bg_chat_canvas: Color32::from_rgb(235, 240, 247), // Authentic Zalo PC cool blue-gray canvas
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
            bubble_out_bg: Color32::from_rgb(229, 239, 255), // Zalo Desktop Light Outgoing (#E5EFFF)
            bubble_out_text: Color32::from_rgb(15, 23, 42),  // Crisp dark text on light blue (14:1)
            bubble_out_meta: Color32::from_rgb(71, 92, 122),
            bubble_in_bg: Color32::WHITE,
            bubble_in_border: Color32::from_rgb(216, 224, 236),
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

/// Extract up to 2 uppercase initials from a display name (e.g. `"Đỗ Thành"` -> `"ĐT"`, `"Hoang"` -> `"H"`).
fn avatar_initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().filter(|w| !w.is_empty()).collect();
    if words.len() >= 2 {
        let c1 = words[0].chars().next().unwrap_or('Z').to_uppercase();
        let c2 = words[words.len() - 1]
            .chars()
            .next()
            .unwrap_or('U')
            .to_uppercase();
        format!("{c1}{c2}")
    } else {
        name.chars()
            .next()
            .unwrap_or('Z')
            .to_uppercase()
            .to_string()
    }
}

/// Paint a circular user or group avatar: uses the real circular-masked Zalo profile image when
/// `avatar_url` is available (fetching it asynchronously via `FetchStickerImage`), and falls back
/// to a colored circle with initials while loading or when no URL exists.
fn paint_user_avatar(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    name: &str,
    is_group: bool,
    avatar_url: Option<&str>,
    textures: &HashMap<String, TextureHandle>,
    loading: &mut HashSet<String>,
) {
    if let Some(url) = avatar_url
        .map(str::trim)
        .filter(|u| !u.is_empty() && (u.starts_with("http://") || u.starts_with("https://")))
    {
        let key = format!("ava_{}", crate::crypto::md5_hex(url));
        if let Some(tex) = textures.get(&key) {
            let rect = Rect::from_center_size(center, Vec2::splat(radius * 2.0));
            painter.image(
                tex.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            painter.circle_stroke(
                center,
                radius,
                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 34)),
            );
            return;
        }
        if loading.insert(key.clone()) {
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::FetchStickerImage {
                    key,
                    url: url.to_string(),
                });
            }
        }
    }

    let av_col = avatar_color_for(name, is_group);
    painter.circle_filled(center, radius, av_col);
    let initial = if radius >= 18.0 {
        avatar_initials(name)
    } else {
        name.chars()
            .next()
            .unwrap_or('Z')
            .to_uppercase()
            .to_string()
    };
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        initial,
        FontId::proportional((radius * 0.72).max(11.0)),
        Color32::WHITE,
    );
}

pub mod lucide {
    pub const SEARCH: &str = "\u{e151}";
    pub const REFRESH: &str = "\u{e145}";
    pub const SMILE: &str = "\u{e164}";
    pub const SEND: &str = "\u{e4f2}";
    pub const SUN: &str = "\u{e178}";
    pub const MOON: &str = "\u{e11e}";
    pub const PHONE: &str = "\u{e133}";
    pub const PHONE_CALL: &str = "\u{e134}";
    pub const PHONE_OFF: &str = "\u{e138}";
    pub const VIDEO: &str = "\u{e1a5}";
    pub const VIDEO_OFF: &str = "\u{e1a6}";
    pub const CAMERA: &str = "\u{e062}";
    pub const IMAGE: &str = "\u{e0f6}";
    pub const PAPERCLIP: &str = "\u{e12d}";
    pub const CHECK: &str = "\u{e06c}";
    pub const CHECK_CHECK: &str = "\u{e38e}";
    pub const MESSAGE_SQUARE: &str = "\u{e117}";
    pub const USERS: &str = "\u{e1a4}";
    pub const USER_PLUS: &str = "\u{e1a2}";
    pub const CLOUD: &str = "\u{e088}";
    pub const SETTINGS: &str = "\u{e154}";
    pub const PANEL_RIGHT: &str = "\u{e431}";
    pub const PIN: &str = "\u{e259}";
    pub const COPY: &str = "\u{e09e}";
    pub const STAR: &str = "\u{e176}";
    pub const GLOBE: &str = "\u{e0e8}";
    pub const LINK: &str = "\u{e102}";
    pub const EXTERNAL_LINK: &str = "\u{e0b9}";
    pub const X: &str = "\u{e1b2}";
    pub const PLUS: &str = "\u{e13d}";
    pub const MINUS: &str = "\u{e11c}";
    pub const STORE: &str = "\u{e3e4}";
    pub const FLAME: &str = "\u{e0d2}";
    pub const SPARKLES: &str = "\u{e412}";
    pub const BAN: &str = "\u{e051}";
    pub const MIC: &str = "\u{e118}";
    pub const MIC_OFF: &str = "\u{e119}";
    pub const LOG_OUT: &str = "\u{e10e}";
    pub const FILE_TEXT: &str = "\u{e0cc}";
    pub const FILM: &str = "\u{e0d0}";
    pub const SHIELD_CHECK: &str = "\u{e15c}";
}

fn draw_lucide_icon(painter: &egui::Painter, center: Pos2, icon: &str, size: f32, color: Color32) {
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        icon,
        FontId::proportional(size),
        color,
    );
}

fn draw_icon_search(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::SEARCH, size, color);
}

fn draw_icon_refresh(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::REFRESH, size, color);
}

fn draw_icon_sticker(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::SMILE, size, color);
}

fn draw_icon_send(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::SEND, size, color);
}

fn draw_icon_phone(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::PHONE, size, color);
}

fn draw_icon_video(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::VIDEO, size, color);
}

fn draw_icon_image(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::IMAGE, size, color);
}

fn draw_icon_attachment(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::PAPERCLIP, size, color);
}

fn draw_icon_check_double(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::CHECK_CHECK, size, color);
}

fn draw_icon_chat_nav(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::MESSAGE_SQUARE, size, color);
}

fn draw_icon_contacts(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::USERS, size, color);
}

fn draw_icon_cloud(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::CLOUD, size, color);
}

fn draw_icon_settings(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::SETTINGS, size, color);
}

fn draw_icon_drawer(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::PANEL_RIGHT, size, color);
}

fn draw_icon_pin(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::PIN, size, color);
}

fn draw_icon_copy(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::COPY, size, color);
}

fn draw_icon_user_plus(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::USER_PLUS, size, color);
}
// ---------------------------------------------------------------------------
// Sticker Presets & State Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StickerTab {
    Popular,
    Onion,
    Pack(u64),
    Search,
    Store,
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

/// Built-in featured Zalo sticker packs (`(cate_id, title)`).
const FEATURED_PACK_TABS: &[(u64, &str)] = &[
    (61900, "Zapy Dô Tri"),
    (12815, "Bé Hai"),
    (12748, "Cò Lõ Đi Làm"),
    (12705, "Mèo Meon"),
    (12718, "Gấu Cay"),
    (12045, "Zameo"),
    (11150, "Bé Dúi"),
    (10186, "Đậu Đỏ"),
    (10177, "Heo Luci"),
    (10117, "Chó Toto"),
];

fn builtin_pack_stickers(cate_id: u64) -> Option<Vec<crate::chat::StickerItem>> {
    let (name, ids): (&str, Vec<u64>) = match cate_id {
        61900 => ("Zapy Dô Tri", (98930..=98941).collect()),
        12815 => ("Bé Hai", (101001..=101030).collect()),
        12748 => ("Cò Lõ Đi Làm", (99978..=99997).collect()),
        12705 => ("Mèo Meon", (99278..=99293).collect()),
        12718 => ("Gấu Cay", (99486..=99501).collect()),
        12045 => (
            "Zameo Rốn Lồi",
            (47614..=47624).chain(47626..=47633).collect(),
        ),
        11150 => ("Bé Dúi", (34107..=34110).chain(34827..=34837).collect()),
        10186 => ("Đậu Đỏ", (18974..=18995).collect()),
        10177 => ("Heo Luci", (18847..=18871).collect()),
        10117 => ("Chó Toto", (17745..=17764).collect()),
        _ => return None,
    };
    Some(
        ids.into_iter()
            .enumerate()
            .map(|(i, sid)| crate::chat::StickerItem {
                sticker_id: sid,
                cate_id,
                sticker_type: 7,
                label: format!("{name} #{}", i + 1),
            })
            .collect(),
    )
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
    PresetSticker { sticker_id: 98930, cate_id: 61900, sticker_type: 7, label: "Zapy Xin chào" },
    PresetSticker { sticker_id: 101001, cate_id: 12815, sticker_type: 7, label: "Bé Hai #1" },
    PresetSticker { sticker_id: 99978, cate_id: 12748, sticker_type: 7, label: "Cò Lõ #1" },
    PresetSticker { sticker_id: 99278, cate_id: 12705, sticker_type: 7, label: "Mèo Meon #1" },
    PresetSticker { sticker_id: 99486, cate_id: 12718, sticker_type: 7, label: "Gấu Cay #1" },
    PresetSticker { sticker_id: 47614, cate_id: 12045, sticker_type: 7, label: "Zameo #1" },
    PresetSticker { sticker_id: 34107, cate_id: 11150, sticker_type: 7, label: "Bé Dúi #1" },
    PresetSticker { sticker_id: 18974, cate_id: 10186, sticker_type: 7, label: "Đậu Đỏ #1" },
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
    PresetSticker { sticker_id: 17, cate_id: 0, sticker_type: 7, label: "Củ hành #17" },
    PresetSticker { sticker_id: 18, cate_id: 0, sticker_type: 7, label: "Củ hành #18" },
    PresetSticker { sticker_id: 19, cate_id: 0, sticker_type: 7, label: "Củ hành #19" },
    PresetSticker { sticker_id: 20, cate_id: 0, sticker_type: 7, label: "Củ hành #20" },
    PresetSticker { sticker_id: 21, cate_id: 0, sticker_type: 7, label: "Củ hành #21" },
    PresetSticker { sticker_id: 22, cate_id: 0, sticker_type: 7, label: "Củ hành #22" },
    PresetSticker { sticker_id: 23, cate_id: 0, sticker_type: 7, label: "Củ hành #23" },
    PresetSticker { sticker_id: 24, cate_id: 0, sticker_type: 7, label: "Củ hành #24" },
    PresetSticker { sticker_id: 25, cate_id: 0, sticker_type: 7, label: "Củ hành #25" },
    PresetSticker { sticker_id: 26, cate_id: 0, sticker_type: 7, label: "Củ hành #26" },
    PresetSticker { sticker_id: 27, cate_id: 0, sticker_type: 7, label: "Củ hành #27" },
    PresetSticker { sticker_id: 28, cate_id: 0, sticker_type: 7, label: "Củ hành #28" },
    PresetSticker { sticker_id: 29, cate_id: 0, sticker_type: 7, label: "Củ hành #29" },
    PresetSticker { sticker_id: 30, cate_id: 0, sticker_type: 7, label: "Củ hành #30" },
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
        avatar: Option<String>,
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
    AttachmentSent {
        thread_id: String,
        msg: ChatMessage,
    },
    StickerImageReady {
        key: String,
        image: egui::ColorImage,
    },
    StickerStoreLoaded(Vec<crate::chat::StickerPackInfo>),
    StickerPackItemsLoaded {
        cate_id: u64,
        name: String,
        items: Vec<crate::chat::StickerItem>,
    },
    StickerSearchResults {
        keyword: String,
        items: Vec<crate::chat::StickerItem>,
    },
    PhoneLookupSuccess(crate::chat::PhoneLookupResult),
    PhoneLookupFailed(String),
    QuoteSent {
        thread_id: String,
        text: String,
        msg_id: String,
        quoted: ChatMessage,
    },
    Reacted {
        thread_id: String,
        msg_id: String,
        ts: i64,
        reaction: String,
    },
    MessageDeleted {
        thread_id: String,
        msg_id: String,
        ts: i64,
    },
    SendFailed {
        thread_id: String,
        text: String,
        error: String,
    },
    SessionExpired(String),
    CallStateChanged(String),
    IncomingCall {
        call_id: i64,
        caller_id: String,
        caller_name: String,
        is_video: bool,
        is_group: bool,
    },
    CallRejected {
        code: i64,
        reason: String,
    },
    StatusUpdate(String),
    LoggedOut,
}

#[derive(Debug, Clone)]
struct ActiveCallOverlay {
    partner_id: String,
    partner_name: String,
    is_video: bool,
    is_group: bool,
    state_label: String,
    started_at: std::time::Instant,
    mic_muted: bool,
    cam_enabled: bool,
    is_incoming_ringing: bool,
    error_code: Option<i64>,
    error_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct ImageViewerModal {
    tex_key: String,
    hd_tex_key: Option<String>,
    full_url: String,
    title: String,
    sender: String,
    time: String,
    zoom: f32,
}

pub struct ZaloGuiApp {
    // Theme state
    dark_mode: bool,
    theme_dirty: bool,

    // Session state
    is_logged_in: bool,
    user_name: String,
    user_uin: String,
    user_avatar: Option<String>,

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

    // Reply / forward state
    replying_to: Option<ChatMessage>,
    forward_msg: Option<ChatMessage>,
    forward_filter: String,
    filter_starred_only: bool,
    active_call: Option<ActiveCallOverlay>,
    image_viewer: Option<ImageViewerModal>,

    info_drawer_open: bool,
    profile_menu_open: bool,
    toast_notice: Option<(String, std::time::Instant)>,
    phone_modal_open: bool,
    phone_modal_input: String,
    phone_lookup_loading: bool,
    phone_lookup_result: Option<crate::chat::PhoneLookupResult>,
    phone_lookup_error: Option<String>,
    in_chat_search_open: bool,
    in_chat_search_query: String,
    // Sticker picker & image cache state
    sticker_picker_open: bool,
    sticker_tab: StickerTab,
    sticker_textures: HashMap<String, TextureHandle>,
    loading_stickers: HashSet<String>,
    emoji_textures: HashMap<String, TextureHandle>,
    sticker_store_packs: Vec<crate::chat::StickerPackInfo>,
    sticker_store_loading: bool,
    sticker_store_filter: String,
    sticker_pack_items: HashMap<u64, Vec<crate::chat::StickerItem>>,
    loading_sticker_packs: HashSet<u64>,
    pinned_sticker_packs: Vec<(u64, String)>,
    sticker_search_query: String,
    sticker_search_results: Vec<crate::chat::StickerItem>,
    sticker_search_loading: bool,

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
        let (user_name, user_uin, user_avatar) = session
            .login
            .as_ref()
            .map(|l| {
                (
                    l.display_name.clone(),
                    l.uin.clone(),
                    if l.avatar.is_empty() {
                        None
                    } else {
                        Some(l.avatar.clone())
                    },
                )
            })
            .unwrap_or_default();

        let mut threads = if initial_threads.is_empty() {
            crate::chat::load_threads_cache()
        } else {
            let mut cached = crate::chat::load_threads_cache();
            for t in initial_threads {
                if let Some(c) = cached
                    .iter_mut()
                    .find(|c| crate::chat::threads_represent_same_contact(c, &t))
                {
                    crate::chat::merge_thread_into(c, t, true);
                } else {
                    cached.push(t);
                }
            }
            cached
        };
        if let Some(li) = session.login.as_ref() {
            if !li.send2me_id.is_empty() {
                for t in threads.iter_mut() {
                    if !t.is_group && t.name.contains("Cloud của tôi") {
                        t.id = li.send2me_id.clone();
                    }
                }
            }
        }
        crate::chat::deduplicate_threads(&mut threads, None);
        crate::store::load_history_into_threads("zalo_history.db", &mut threads, 200);
        crate::chat::deduplicate_threads(&mut threads, None);
        crate::chat::save_threads_cache(&threads);

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
            user_avatar,
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
            replying_to: None,
            forward_msg: None,
            forward_filter: String::new(),
            filter_starred_only: false,
            active_call: None,
            image_viewer: None,
            status_msg: "Trực tuyến · Sẵn sàng".into(),
            info_drawer_open: true,
            profile_menu_open: false,
            toast_notice: None,
            phone_modal_open: false,
            phone_modal_input: String::new(),
            phone_lookup_loading: false,
            phone_lookup_result: None,
            phone_lookup_error: None,
            in_chat_search_open: false,
            in_chat_search_query: String::new(),
            sticker_picker_open: false,
            sticker_tab: StickerTab::Popular,
            sticker_textures: HashMap::new(),
            loading_stickers: HashSet::new(),
            emoji_textures: load_default_emoji_textures(&cc.egui_ctx),
            sticker_store_packs: Vec::new(),
            sticker_store_loading: false,
            sticker_store_filter: String::new(),
            sticker_pack_items: FEATURED_PACK_TABS
                .iter()
                .filter_map(|&(cid, _)| builtin_pack_stickers(cid).map(|items| (cid, items)))
                .collect(),
            loading_sticker_packs: HashSet::new(),
            pinned_sticker_packs: FEATURED_PACK_TABS
                .iter()
                .map(|&(cid, title)| (cid, title.to_string()))
                .collect(),
            sticker_search_query: String::new(),
            sticker_search_results: Vec::new(),
            sticker_search_loading: false,
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
                GuiEvent::LoginSuccess { name, uin, avatar } => {
                    self.is_logged_in = true;
                    self.user_name = name;
                    self.user_uin = uin;
                    if avatar.is_some() {
                        self.user_avatar = avatar;
                    }
                    self.login_in_progress = false;
                    self.status_msg = "Trực tuyến · Đồng bộ thời gian thực".into();
                }
                GuiEvent::LoginFailed(err) => {
                    self.login_in_progress = false;
                    self.login_status_msg = format!("Lỗi đăng nhập: {err}");
                }
                GuiEvent::ThreadsUpdated(updates) => {
                    if self.is_logged_in && !updates.is_empty() {
                        let prev_selected_name = self
                            .threads
                            .get(self.selected_thread_index)
                            .map(|t| (t.name.clone(), t.is_group));
                        let live_ids: HashSet<String> = updates
                            .iter()
                            .filter(|u| !u.id.is_empty() && u.id != "0")
                            .map(|u| u.id.clone())
                            .collect();

                        for nt in updates {
                            if let Some(existing) = self.threads.iter_mut().find(|t| t.id == nt.id)
                            {
                                crate::chat::merge_thread_into(existing, nt, true);
                            } else if let Some(existing) = self.threads.iter_mut().find(|t| {
                                !live_ids.contains(&t.id)
                                    && crate::chat::threads_represent_same_contact(t, &nt)
                            }) {
                                crate::chat::merge_thread_into(existing, nt, true);
                            } else if !nt.id.is_empty() && nt.id != "0" {
                                self.threads.push(nt);
                            }
                        }

                        crate::chat::deduplicate_threads(&mut self.threads, Some(&live_ids));

                        if let Some((sel_name, sel_grp)) = prev_selected_name {
                            if let Some(idx) = self
                                .threads
                                .iter()
                                .position(|t| t.is_group == sel_grp && t.name == sel_name)
                            {
                                self.selected_thread_index = idx;
                            }
                        }
                        if self.selected_thread_index >= self.threads.len() {
                            self.selected_thread_index = self.threads.len().saturating_sub(1);
                        }
                        self.persist_threads_async();
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
                                ..Default::default()
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
                                ..Default::default()
                            });
                            t.messages.sort_by_key(|m| m.ts);
                        }
                        self.persist_threads_async();
                    }
                    self.status_msg = format!("Đã gửi nhãn dán lúc {}", crate::ui::clock());
                }
                GuiEvent::AttachmentSent { thread_id, msg } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.preview = msg.text.clone();
                        t.time = msg.time.clone();
                        if !t.messages.iter().any(|m| !m.id.is_empty() && m.id == msg.id) {
                            t.messages.push(msg);
                            t.messages.sort_by_key(|m| m.ts);
                        }
                        self.persist_threads_async();
                    }
                    self.status_msg = format!("Đã gửi đính kèm lúc {}", crate::ui::clock());
                }
                GuiEvent::QuoteSent {
                    thread_id,
                    text,
                    msg_id,
                    quoted,
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
                                reply_to_id: Some(quoted.id.clone()),
                                reply_to_sender: Some(quoted.sender.clone()),
                                reply_to_text: Some(quoted.text.clone()),
                                ..Default::default()
                            });
                            t.messages.sort_by_key(|m| m.ts);
                        }
                        self.persist_threads_async();
                    }
                    self.status_msg = format!("Đã gửi trả lời lúc {}", crate::ui::clock());
                }
                GuiEvent::Reacted {
                    thread_id,
                    msg_id,
                    ts,
                    reaction,
                } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        if let Some(m) = t.messages.iter_mut().find(|m| {
                            (!m.id.is_empty() && m.id == msg_id) || (m.id.is_empty() && m.ts == ts)
                        }) {
                            m.reactions = vec![reaction];
                        }
                        self.persist_threads_async();
                    }
                    self.status_msg = "Đã thả cảm xúc".into();
                }
                GuiEvent::MessageDeleted {
                    thread_id,
                    msg_id,
                    ts,
                } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.messages.retain(|m| {
                            !((!msg_id.is_empty() && m.id == msg_id)
                                || (msg_id.is_empty() && m.ts == ts))
                        });
                        if let Some(last) = t.messages.last() {
                            t.preview = last.text.clone();
                        }
                        self.persist_threads_async();
                    }
                    self.status_msg = "Đã xóa tin nhắn ở phía bạn".into();
                }
                GuiEvent::StickerImageReady { key, image } => {
                    let tex = ctx.load_texture(&key, image, TextureOptions::LINEAR);
                    self.sticker_textures.insert(key, tex);
                }
                GuiEvent::StickerStoreLoaded(packs) => {
                    self.sticker_store_loading = false;
                    let count = packs.len();
                    self.sticker_store_packs = packs;
                    self.status_msg = format!("Đã tải Kho Nhãn Dán Zalo ({count} bộ nhãn)");
                }
                GuiEvent::StickerPackItemsLoaded {
                    cate_id,
                    name,
                    items,
                } => {
                    self.loading_sticker_packs.remove(&cate_id);
                    let count = items.len();
                    if !self.pinned_sticker_packs.iter().any(|(id, _)| *id == cate_id) {
                        self.pinned_sticker_packs.insert(0, (cate_id, name.clone()));
                    }
                    self.sticker_pack_items.insert(cate_id, items);
                    self.status_msg = format!("Đã mở bộ nhãn \"{name}\" ({count} nhãn dán)");
                }
                GuiEvent::StickerSearchResults { keyword, items } => {
                    self.sticker_search_loading = false;
                    let count = items.len();
                    self.sticker_search_results = items;
                    self.status_msg =
                        format!("Tìm thấy {count} nhãn dán cho từ khóa \"{keyword}\"");
                }
                GuiEvent::PhoneLookupSuccess(res) => {
                    self.phone_lookup_loading = false;
                    self.phone_lookup_error = None;
                    self.phone_modal_open = true;
                    self.show_toast(format!(
                        "Đã tìm thấy tài khoản: {}",
                        res.display_name
                    ));
                    self.phone_lookup_result = Some(res);
                }
                GuiEvent::PhoneLookupFailed(err) => {
                    self.phone_lookup_loading = false;
                    self.phone_lookup_result = None;
                    self.phone_modal_open = true;
                    self.phone_lookup_error = Some(err);
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
                GuiEvent::CallStateChanged(state) => {
                    if state == "free" {
                        // Keep modal open if there is a rejection reason so the user can read it
                        let has_error = self
                            .active_call
                            .as_ref()
                            .and_then(|c| c.error_reason.as_ref())
                            .is_some();
                        if !has_error {
                            self.active_call = None;
                        }
                    } else if let Some(ref mut call) = self.active_call {
                        if state == "incall" || state == "connected" {
                            call.is_incoming_ringing = false;
                        }
                        if call.error_reason.is_none() {
                            call.state_label = match state.as_str() {
                                "calling" | "ringing" => "Đang đổ chuông…".into(),
                                "incoming_ringing" => "Cuộc gọi đến đang đổ chuông…".into(),
                                "connecting" => "Đang kết nối luồng âm thanh/video…".into(),
                                "incall" | "connected" => "Đang trong cuộc gọi (Pure-Rust VoIP)".into(),
                                _ => format!("Đang kết nối ({state})…"),
                            };
                        }
                    }
                }
                GuiEvent::IncomingCall {
                    call_id: _,
                    caller_id,
                    caller_name,
                    is_video,
                    is_group,
                } => {
                    let resolved_name = self
                        .threads
                        .iter()
                        .find(|t| t.id == caller_id)
                        .map(|t| t.name.clone())
                        .unwrap_or(caller_name);
                    self.active_call = Some(ActiveCallOverlay {
                        partner_id: caller_id,
                        partner_name: resolved_name,
                        is_video,
                        is_group,
                        state_label: "Cuộc gọi đến đang đổ chuông…".into(),
                        started_at: std::time::Instant::now(),
                        mic_muted: false,
                        cam_enabled: is_video,
                        is_incoming_ringing: true,
                        error_code: None,
                        error_reason: None,
                    });
                }
                GuiEvent::CallRejected { code, reason } => {
                    self.status_msg = format!("⚠️ Từ chối cuộc gọi (mã {code}): {reason}");
                    if let Some(ref mut call) = self.active_call {
                        call.state_label = "Không thể thực hiện cuộc gọi".into();
                        call.error_code = Some(code);
                        call.error_reason = Some(reason);
                    }
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
    PersistThreads(Vec<Thread>),
    Logout,
}

static WORKER_TX: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<WorkerCmd>> =
    std::sync::OnceLock::new();

fn extract_msg_id(resp: &serde_json::Value) -> String {
    resp.get("data")
        .and_then(|d| d.get("msgId"))
        .and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

fn resp_error_code(resp: &serde_json::Value) -> Option<i64> {
    resp.get("error_code")
        .and_then(serde_json::Value::as_i64)
        .filter(|c| *c != 0)
}

fn is_session_expired_error(err_str: &str) -> bool {
    err_str.contains("server error 600")
        || err_str.contains("zpw_sek")
        || err_str.contains("server error -102")
        || err_str.contains("chưa đăng nhập")
}

fn spawn_session_worker(session: Session, gui_tx: Sender<GuiEvent>, egui_ctx: egui::Context) {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<WorkerCmd>();
    let _ = WORKER_TX.set(cmd_tx);

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
                    let _ = crate::login::get_server_info(&s).await;
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
                        loop {
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
                                            if ev.is_voip_ish() {
                                                let data_node = ev
                                                    .payload
                                                    .get("data")
                                                    .unwrap_or(&ev.payload);
                                                for voip_ev in
                                                    crate::call::extract_voip_controls(data_node)
                                                {
                                                    cb.handle_control_event(voip_ev);
                                                }
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
                            let _ = std::fs::remove_file("zalo_history.db");
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
                                                            }
                                                        }
                                                        let _ = s.sync_profile_me().await;
                                                        let _ = s.save_to_file("session.json");

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
                                let mut s = session_clone.lock().await.clone();
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
                                    if let Some((uid, name)) = s.sync_profile_me().await {
                                        {
                                            let mut sl = session_clone.lock().await;
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
                                    let is_send2me = s
                                        .login
                                        .as_ref()
                                        .map(|l| !l.send2me_id.is_empty() && l.send2me_id == group_id)
                                        .unwrap_or(false);
                                    let mut t = Thread {
                                        id: group_id.clone(),
                                        name: String::new(),
                                        preview: String::new(),
                                        time: String::new(),
                                        avatar: None,
                                        is_group: !is_send2me,
                                        messages: Vec::new(),
                                    };
                                    if !is_send2me {
                                        if let Ok(resp) =
                                            crate::chat::get_group_history(&s, &group_id, 50).await
                                        {
                                            crate::chat::merge_group_history(&mut t, &resp);
                                        }
                                    }
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
                                                crate::store::MessageStore::open("zalo_history.db")
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
                                } else if !url.starts_with("http://") && !url.starts_with("https://") {
                                    std::fs::read(&url).ok()
                                } else {
                                    let client = { session_clone.lock().await.clone().http };
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
                                        let _ = gui_tx_clone.send(GuiEvent::StickerSearchResults {
                                            keyword,
                                            items,
                                        });
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
                                    }
                                }
                                ctx_clone.request_repaint();
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
                        WorkerCmd::AddFriend { user_id, user_name } => {
                            let session_clone = session.clone();
                            let gui_tx_clone = gui_tx.clone();
                            let ctx_clone = egui_ctx.clone();
                            tokio::spawn(async move {
                                let s = session_clone.lock().await.clone();
                                // Check if there is an incoming request we can accept directly
                                if crate::chat::accept_friend_request(&s, &user_id).await.is_ok() {
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                        "Đã đồng ý kết bạn với {user_name}! Bạn có thể gọi lại ngay."
                                    )));
                                    ctx_clone.request_repaint();
                                    return;
                                }
                                match crate::chat::send_friend_request(
                                    &s,
                                    &user_id,
                                    "Xin chào, kết bạn với mình để gọi điện nhé!",
                                )
                                .await
                                {
                                    Ok(_) => {
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "Đã gửi lời mời kết bạn tới {user_name} ({user_id}). Đợi người nhận đồng ý để gọi."
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

        // Only show a minimal top bar on the Login screen; when logged in, the 64px Left
        // Navigation Rail replaces both the top bar and the bottom status footer.
        if !self.is_logged_in {
            egui::TopBottomPanel::top("top_bar_login")
                .frame(
                    egui::Frame::none()
                        .fill(pal.bg_sidebar)
                        .inner_margin(Margin::symmetric(16.0, 9.0))
                        .stroke(Stroke::new(1.0_f32, pal.border)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
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
                            if !self.threads.is_empty() {
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
        }
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

    fn show_toast(&mut self, msg: impl Into<String>) {
        let text = msg.into();
        self.status_msg = text.clone();
        self.toast_notice = Some((text, std::time::Instant::now()));
    }
    fn render_chat_workspace(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        // Responsive breakpoints based on live viewport width
        // -------------------------------------------------------------------
        let screen_rect = ctx.screen_rect();
        let screen_w = screen_rect.width();
        let is_compact_w = screen_w < 780.0;
        let is_narrow_w = screen_w < 620.0;
        let left_rail_w = if is_narrow_w { 52.0 } else { 62.0 };

        // -------------------------------------------------------------------
        // 1. LEFT NAVIGATION RAIL (Replaces top bar & bottom bar)
        // -------------------------------------------------------------------
        egui::SidePanel::left("left_nav_rail")
            .resizable(false)
            .exact_width(left_rail_w)
            .frame(
                egui::Frame::none()
                    .fill(pal.bg_app)
                    .inner_margin(Margin::symmetric(if is_narrow_w { 4.0 } else { 8.0 }, 14.0))
                    .stroke(Stroke::new(1.0_f32, pal.border)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    // User Avatar Button with Cutout Online Status Dot
                    let (av_rect, av_resp) =
                        ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::click());
                    let user_disp = if self.user_name.is_empty() {
                        "Đỗ Thành"
                    } else {
                        &self.user_name
                    };
                    paint_user_avatar(
                        ui.painter(),
                        av_rect.center(),
                        20.0,
                        user_disp,
                        false,
                        self.user_avatar.as_deref(),
                        &self.sticker_textures,
                        &mut self.loading_stickers,
                    );
                    if av_resp.hovered() || self.profile_menu_open {
                        ui.painter().circle_stroke(
                            av_rect.center(),
                            21.0,
                            Stroke::new(2.0_f32, pal.primary_hover),
                        );
                    }
                    // Cutout status dot
                    let dot_c = Pos2::new(av_rect.right() - 4.0, av_rect.bottom() - 4.0);
                    ui.painter().circle_filled(dot_c, 6.5, pal.bg_app);
                    ui.painter().circle_filled(dot_c, 4.5, pal.accent_online);

                    if av_resp
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text(format!(
                            "{} (Đang trực tuyến — Bấm để mở menu tài khoản)",
                            if self.user_name.is_empty() {
                                "Tài khoản Zalo"
                            } else {
                                &self.user_name
                            }
                        ))
                        .clicked()
                    {
                        self.profile_menu_open = !self.profile_menu_open;
                    }

                    ui.add_space(18.0);

                    // Navigation Icon 1: Tin nhắn (All / Active Chat)
                    let is_msg_nav = self.thread_filter == ThreadFilter::All;
                    let (nav1_rect, nav1_resp) =
                        ui.allocate_exact_size(Vec2::new(42.0, 42.0), Sense::click());
                    if is_msg_nav {
                        ui.painter().rect_filled(
                            nav1_rect,
                            Rounding::same(11.0),
                            pal.primary.linear_multiply(0.18),
                        );
                        let pill = Rect::from_min_size(
                            Pos2::new(nav1_rect.left() - 6.0, nav1_rect.center().y - 10.0),
                            Vec2::new(3.5, 20.0),
                        );
                        ui.painter()
                            .rect_filled(pill, Rounding::same(2.0), pal.primary);
                    } else if nav1_resp.hovered() {
                        ui.painter()
                            .rect_filled(nav1_rect, Rounding::same(11.0), pal.bg_hover);
                    }
                    draw_icon_chat_nav(
                        ui.painter(),
                        nav1_rect.center(),
                        19.0,
                        if is_msg_nav {
                            pal.primary_hover
                        } else {
                            pal.text_secondary
                        },
                    );
                    if nav1_resp
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text("Tin nhắn")
                        .clicked()
                    {
                        self.thread_filter = ThreadFilter::All;
                    }

                    ui.add_space(6.0);

                    // Navigation Icon 2: Danh bạ / Cá nhân
                    let is_contacts_nav = self.thread_filter == ThreadFilter::Direct;
                    let (nav2_rect, nav2_resp) =
                        ui.allocate_exact_size(Vec2::new(42.0, 42.0), Sense::click());
                    if is_contacts_nav {
                        ui.painter().rect_filled(
                            nav2_rect,
                            Rounding::same(11.0),
                            pal.primary.linear_multiply(0.18),
                        );
                        let pill = Rect::from_min_size(
                            Pos2::new(nav2_rect.left() - 6.0, nav2_rect.center().y - 10.0),
                            Vec2::new(3.5, 20.0),
                        );
                        ui.painter()
                            .rect_filled(pill, Rounding::same(2.0), pal.primary);
                    } else if nav2_resp.hovered() {
                        ui.painter()
                            .rect_filled(nav2_rect, Rounding::same(11.0), pal.bg_hover);
                    }
                    draw_icon_contacts(
                        ui.painter(),
                        nav2_rect.center(),
                        19.0,
                        if is_contacts_nav {
                            pal.primary_hover
                        } else {
                            pal.text_secondary
                        },
                    );
                    if nav2_resp
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text(format!("Danh bạ ({} liên hệ)", self.threads.len()))
                        .clicked()
                    {
                        self.thread_filter = if self.thread_filter == ThreadFilter::Direct {
                            ThreadFilter::All
                        } else {
                            ThreadFilter::Direct
                        };
                    }

                    ui.add_space(6.0);

                    // Navigation Icon 3: Cloud của tôi
                    let is_cloud_selected = self
                        .threads
                        .get(self.selected_thread_index)
                        .map(|t| t.name.contains("Cloud của tôi"))
                        .unwrap_or(false);
                    let (nav3_rect, nav3_resp) =
                        ui.allocate_exact_size(Vec2::new(42.0, 42.0), Sense::click());
                    if is_cloud_selected {
                        ui.painter().rect_filled(
                            nav3_rect,
                            Rounding::same(11.0),
                            pal.primary.linear_multiply(0.18),
                        );
                    } else if nav3_resp.hovered() {
                        ui.painter()
                            .rect_filled(nav3_rect, Rounding::same(11.0), pal.bg_hover);
                    }
                    draw_icon_cloud(
                        ui.painter(),
                        nav3_rect.center(),
                        19.0,
                        if is_cloud_selected {
                            pal.primary_hover
                        } else {
                            pal.text_secondary
                        },
                    );
                    if nav3_resp
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text("Cloud của tôi")
                        .clicked()
                    {
                        if let Some((cidx, cthread)) = self
                            .threads
                            .iter()
                            .enumerate()
                            .find(|(_, t)| t.name.contains("Cloud của tôi"))
                        {
                            let cid = cthread.id.clone();
                            self.selected_thread_index = cidx;
                            if let Some(tx) = WORKER_TX.get() {
                                let _ = tx.send(WorkerCmd::LoadGroupHistory { group_id: cid });
                            }
                        }
                    }

                    // Push bottom actions to the bottom of the Left Rail
                    let rem_h = (ui.available_height() - 100.0).max(12.0);
                    ui.add_space(rem_h);

                    // Bottom Action 1: Background Sync Indicator Icon
                    let (sync_rect, sync_resp) =
                        ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::click());
                    if sync_resp.hovered() {
                        ui.painter()
                            .rect_filled(sync_rect, Rounding::same(10.0), pal.bg_hover);
                    }
                    draw_icon_refresh(
                        ui.painter(),
                        sync_rect.center(),
                        17.0,
                        if sync_resp.hovered() {
                            pal.accent_online
                        } else {
                            pal.text_secondary
                        },
                    );
                    ui.painter().circle_filled(
                        Pos2::new(sync_rect.right() - 9.0, sync_rect.top() + 9.0),
                        3.0,
                        pal.accent_online,
                    );
                    if sync_resp
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text(format!(
                            "Trạng thái: {} · {} hội thoại (Bấm để đồng bộ ngay)",
                            self.status_msg,
                            self.threads.len()
                        ))
                        .clicked()
                    {
                        if let Some(tx) = WORKER_TX.get() {
                            let _ = tx.send(WorkerCmd::RefreshThreads);
                        }
                        self.show_toast(format!(
                            "Đang đồng bộ ngầm {} hội thoại…",
                            self.threads.len()
                        ));
                    }

                    ui.add_space(6.0);

                    // Bottom Action 2: Theme / Settings Icon
                    let (set_rect, set_resp) =
                        ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::click());
                    if set_resp.hovered() {
                        ui.painter()
                            .rect_filled(set_rect, Rounding::same(10.0), pal.bg_hover);
                    }
                    draw_icon_settings(
                        ui.painter(),
                        set_rect.center(),
                        17.5,
                        if set_resp.hovered() {
                            pal.text_primary
                        } else {
                            pal.text_secondary
                        },
                    );
                    if set_resp
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text("Cài đặt & Chuyển đổi giao diện Sáng / Tối")
                        .clicked()
                    {
                        self.dark_mode = !self.dark_mode;
                        self.theme_dirty = true;
                        self.show_toast(if self.dark_mode {
                            "Đã chuyển sang giao diện Tối (Dark Mode)"
                        } else {
                            "Đã chuyển sang giao diện Sáng (Light Mode)"
                        });
                    }
                });
            });

        // -------------------------------------------------------------------
        // 2. CONVERSATION LIST SIDEBAR (Responsive width, auto-adapts on resize)
        // -------------------------------------------------------------------
        let sidebar_default_w = if is_narrow_w {
            76.0
        } else if is_compact_w {
            240.0
        } else {
            300.0
        };
        let (sidebar_id, sidebar_min_w, sidebar_max_w) = if is_narrow_w {
            ("conversation_sidebar_narrow", 68.0, 68.0)
        } else {
            (
                "conversation_sidebar_wide",
                190.0,
                (screen_w * 0.45).clamp(240.0, 420.0),
            )
        };

        egui::SidePanel::left(sidebar_id)
            .resizable(!is_narrow_w)
            .default_width(sidebar_default_w)
            .width_range(sidebar_min_w..=sidebar_max_w)
            .frame(
                egui::Frame::none()
                    .fill(pal.bg_sidebar)
                    .stroke(Stroke::new(1.0_f32, pal.border)),
            )
            .show(ctx, |ui| {
                egui::Frame::none()
                    .inner_margin(Margin {
                        left: if is_narrow_w { 4.0 } else { 14.0 },
                        right: if is_narrow_w { 4.0 } else { 14.0 },
                        top: 12.0,
                        bottom: 0.0,
                    })
                    .show(ui, |ui| {
                        if is_narrow_w {
                            // Compact mode: icon-only search / lookup button
                            ui.vertical_centered(|ui| {
                                let (srch_r, srch_resp) =
                                    ui.allocate_exact_size(Vec2::new(42.0, 32.0), Sense::click());
                                ui.painter()
                                    .rect_filled(srch_r, Rounding::same(8.0), pal.bg_input);
                                ui.painter().rect_stroke(
                                    srch_r,
                                    Rounding::same(8.0),
                                    Stroke::new(1.0_f32, pal.border),
                                );
                                draw_icon_search(
                                    ui.painter(),
                                    srch_r.center(),
                                    16.0,
                                    pal.primary_hover,
                                );
                                if srch_resp
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text("Tìm kiếm hội thoại & Tra cứu SĐT")
                                    .clicked()
                                {
                                    self.phone_modal_open = true;
                                }
                            });
                            ui.add_space(6.0);
                        } else {
                        // Search Box + Quick Phone Lookup (`+👤`) Button
                        let avail_row_w = ui.available_width();
                        let mut trigger_phone_lookup: Option<String> = None;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let search_box_w = (avail_row_w - 40.0).max(120.0);
                            egui::Frame::none()
                                .fill(pal.bg_input)
                                .rounding(Rounding::same(8.0))
                                .stroke(Stroke::new(1.0_f32, pal.border))
                                .inner_margin(Margin::symmetric(9.0, 6.0))
                                .show(ui, |ui| {
                                    ui.set_width((search_box_w - 20.0).max(90.0));
                                    ui.horizontal(|ui| {
                                        let (icon_rect, _) = ui.allocate_exact_size(
                                            Vec2::new(15.0, 15.0),
                                            Sense::hover(),
                                        );
                                        draw_icon_search(
                                            ui.painter(),
                                            icon_rect.center(),
                                            13.5,
                                            pal.text_muted,
                                        );
                                        let s_resp = ui.add(
                                            egui::TextEdit::singleline(&mut self.search_query)
                                                .frame(false)
                                                .hint_text("Tìm hội thoại, tin nhắn, SĐT...")
                                                .desired_width((search_box_w - 58.0).max(60.0)),
                                        );
                                        let s_enter = (s_resp.lost_focus() || s_resp.has_focus())
                                            && ui.input(|i| i.key_pressed(Key::Enter));
                                        let digits_cnt = self
                                            .search_query
                                            .chars()
                                            .filter(|c| c.is_ascii_digit())
                                            .count();
                                        if s_enter && digits_cnt >= 8 {
                                            trigger_phone_lookup =
                                                Some(self.search_query.trim().to_string());
                                        }
                                        if !self.search_query.is_empty()
                                            && ui
                                                .small_button("×")
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .on_hover_text("Xóa tìm kiếm")
                                                .clicked()
                                        {
                                            self.search_query.clear();
                                        }
                                    });
                                });

                            // Add Friend / Lookup by Phone Button (`+👤`)
                            let (add_rect, add_resp) =
                                ui.allocate_exact_size(Vec2::new(32.0, 32.0), Sense::click());
                            ui.painter().rect_filled(
                                add_rect,
                                Rounding::same(8.0),
                                if self.phone_modal_open {
                                    pal.primary.linear_multiply(0.22)
                                } else if add_resp.hovered() {
                                    pal.bg_hover
                                } else {
                                    pal.bg_input
                                },
                            );
                            ui.painter().rect_stroke(
                                add_rect,
                                Rounding::same(8.0),
                                Stroke::new(
                                    1.0_f32,
                                    if self.phone_modal_open || add_resp.hovered() {
                                        pal.primary
                                    } else {
                                        pal.border
                                    },
                                ),
                            );
                            draw_icon_user_plus(
                                ui.painter(),
                                add_rect.center(),
                                16.5,
                                if self.phone_modal_open || add_resp.hovered() {
                                    pal.primary_hover
                                } else {
                                    pal.text_secondary
                                },
                            );
                            if add_resp
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .on_hover_text("Tìm kiếm số điện thoại Zalo & Kết bạn")
                                .clicked()
                            {
                                self.phone_modal_open = true;
                                let digits_cnt = self
                                    .search_query
                                    .chars()
                                    .filter(|c| c.is_ascii_digit())
                                    .count();
                                if digits_cnt >= 8 && self.phone_modal_input.is_empty() {
                                    self.phone_modal_input = self.search_query.trim().to_string();
                                }
                            }
                        });

                        if let Some(phone_str) = trigger_phone_lookup {
                            self.phone_modal_open = true;
                            self.phone_modal_input = phone_str.clone();
                            self.phone_lookup_loading = true;
                            self.phone_lookup_error = None;
                            self.phone_lookup_result = None;
                            if let Some(tx) = WORKER_TX.get() {
                                let _ = tx.send(WorkerCmd::LookupPhone { phone: phone_str });
                            }
                        }
                        ui.add_space(10.0);

                        // Clean Underline Segmented Tabs (`Tất cả`, `Cá nhân`, `Nhóm`) + `Đã ghim (1)`
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 16.0;
                            for (filter, label) in [
                                (ThreadFilter::All, "Tất cả"),
                                (ThreadFilter::Direct, "Cá nhân"),
                                (ThreadFilter::Groups, "Nhóm"),
                            ] {
                                let active = self.thread_filter == filter;
                                let text_w = ui.fonts(|f| {
                                    f.layout_no_wrap(
                                        label.to_string(),
                                        FontId::proportional(12.5),
                                        pal.text_primary,
                                    )
                                    .size()
                                    .x
                                });
                                let (tab_rect, tab_resp) = ui.allocate_exact_size(
                                    Vec2::new(text_w + 4.0, 26.0),
                                    Sense::click(),
                                );
                                let fg = if active {
                                    pal.primary_hover
                                } else if tab_resp.hovered() {
                                    pal.text_primary
                                } else {
                                    pal.text_secondary
                                };
                                ui.painter().text(
                                    Pos2::new(tab_rect.center().x, tab_rect.top() + 8.0),
                                    egui::Align2::CENTER_CENTER,
                                    label,
                                    FontId::proportional(12.5),
                                    fg,
                                );
                                if active {
                                    let line_y = tab_rect.bottom() - 1.0;
                                    ui.painter().line_segment(
                                        [
                                            Pos2::new(tab_rect.left(), line_y),
                                            Pos2::new(tab_rect.right(), line_y),
                                        ],
                                        Stroke::new(2.2_f32, pal.primary),
                                    );
                                }
                                if tab_resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                    self.thread_filter = filter;
                                }
                            }

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let pinned_cnt = self
                                    .threads
                                    .iter()
                                    .filter(|t| t.name.contains("Cloud của tôi"))
                                    .count();
                                if pinned_cnt > 0 {
                                    ui.label(
                                        RichText::new(format!("Đã ghim ({pinned_cnt})"))
                                            .size(11.0)
                                            .color(pal.text_muted),
                                    );
                                }
                            });
                        });
                        }
                    });

                ui.add(egui::Separator::default().spacing(1.0));

                // Sort threads: "Cloud của tôi" pinned first, then descending by latest message timestamp
                let query = self.search_query.trim().to_lowercase();
                let query_digits: String = self
                    .search_query
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect();
                let is_phone_like_query = query_digits.len() >= 8
                    && self
                        .search_query
                        .trim()
                        .chars()
                        .all(|c| c.is_ascii_digit() || matches!(c, '+' | ' ' | '-' | '.'));

                if is_phone_like_query {
                    let raw_q = self.search_query.trim().to_string();
                    egui::Frame::none()
                        .fill(pal.primary.linear_multiply(0.14))
                        .stroke(Stroke::new(1.0_f32, pal.primary))
                        .rounding(Rounding::same(9.0))
                        .inner_margin(Margin::symmetric(10.0, 7.0))
                        .outer_margin(Margin::symmetric(8.0, 4.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (ir, _) =
                                    ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                                draw_icon_user_plus(ui.painter(), ir.center(), 16.0, pal.primary_hover);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(format!("Tìm Zalo qua SĐT: {raw_q}"))
                                            .size(12.2)
                                            .color(pal.text_primary)
                                            .strong(),
                                    );
                                    ui.label(
                                        RichText::new("Nhấn nút Tìm hoặc phím Enter để tra cứu")
                                            .size(10.8)
                                            .color(pal.text_secondary),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let btn = egui::Button::new(
                                        RichText::new("Tìm SĐT")
                                            .size(11.5)
                                            .color(Color32::WHITE)
                                            .strong(),
                                    )
                                    .fill(pal.primary)
                                    .rounding(Rounding::same(6.0));
                                    if ui
                                        .add(btn)
                                        .on_hover_cursor(CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        self.phone_modal_open = true;
                                        self.phone_modal_input = raw_q.clone();
                                        self.phone_lookup_loading = true;
                                        self.phone_lookup_error = None;
                                        self.phone_lookup_result = None;
                                        if let Some(tx) = WORKER_TX.get() {
                                            let _ =
                                                tx.send(WorkerCmd::LookupPhone { phone: raw_q });
                                        }
                                    }
                                });
                            });
                        });
                }

                let mut filtered_indices: Vec<usize> = self
                    .threads
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| {
                        let pass_filter = match self.thread_filter {
                            ThreadFilter::All => true,
                            ThreadFilter::Direct => !t.is_group,
                            ThreadFilter::Groups => t.is_group,
                        };
                        pass_filter
                            && (query.is_empty()
                                || t.name.to_lowercase().contains(&query)
                                || t.id.contains(&query))
                    })
                    .map(|(i, _)| i)
                    .collect();

                filtered_indices.sort_by(|&a, &b| {
                    let ta = &self.threads[a];
                    let tb = &self.threads[b];
                    let a_cloud = ta.name.contains("Cloud của tôi");
                    let b_cloud = tb.name.contains("Cloud của tôi");
                    if a_cloud != b_cloud {
                        return b_cloud.cmp(&a_cloud);
                    }
                    let ts_a = ta
                        .messages
                        .last()
                        .map(|m| normalize_ts_ms(m.ts))
                        .unwrap_or(0);
                    let ts_b = tb
                        .messages
                        .last()
                        .map(|m| normalize_ts_ms(m.ts))
                        .unwrap_or(0);
                    ts_b.cmp(&ts_a)
                });

                // Also search message contents across all conversations when `query` is non-empty!
                let msg_hits: Vec<(usize, String, String, String, bool)> = if query.len() >= 2 {
                    let mut hits = Vec::new();
                    for (t_idx, t) in self.threads.iter().enumerate() {
                        for m in t.messages.iter().rev() {
                            if m.text.to_lowercase().contains(&query) {
                                hits.push((
                                    t_idx,
                                    t.name.clone(),
                                    m.text.clone(),
                                    m.time.clone(),
                                    t.is_group,
                                ));
                                if hits.len() >= 25 {
                                    break;
                                }
                            }
                        }
                        if hits.len() >= 25 {
                            break;
                        }
                    }
                    hits
                } else {
                    Vec::new()
                };

                let mut clicked_thread: Option<(usize, String, bool)> = None;
                let mut clicked_msg_hit: Option<(usize, String)> = None;

                egui::ScrollArea::vertical()
                    .id_salt("threads_list_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(4.0);
                        for &i in &filtered_indices {
                            let t = &self.threads[i];
                            let is_selected = i == self.selected_thread_index;
                            let is_pinned_cloud = t.name.contains("Cloud của tôi");

                            let row_height = if is_narrow_w { 50.0 } else { 58.0 };
                            let row_width = (ui.available_width() - 8.0).max(44.0);
                            let (row_rect, row_resp) = ui.allocate_exact_size(
                                Vec2::new(row_width, row_height),
                                Sense::click(),
                            );
                            let row_rect = row_rect.translate(Vec2::new(4.0, 0.0));

                            let bg = if is_selected {
                                pal.bg_selected
                            } else if row_resp.hovered() {
                                pal.bg_hover.linear_multiply(0.65)
                            } else {
                                Color32::TRANSPARENT
                            };

                            ui.painter()
                                .rect_filled(row_rect, Rounding::same(11.0), bg);

                            // Left active indicator bar (`w-1 bg-zalo-500 rounded-r-full`)
                            if is_selected {
                                let bar_rect = Rect::from_min_size(
                                    Pos2::new(row_rect.left() + 1.0, row_rect.top() + 10.0),
                                    Vec2::new(3.5, 38.0),
                                );
                                ui.painter().rect_filled(
                                    bar_rect,
                                    Rounding::same(2.0),
                                    pal.primary,
                                );
                            }

                            // Avatar circle with real user/group image & Cutout Status Dot
                            let av_radius = if is_narrow_w { 18.0 } else { 20.5 };
                            let avatar_center = if is_narrow_w {
                                row_rect.center()
                            } else {
                                Pos2::new(row_rect.left() + 29.0, row_rect.center().y)
                            };
                            paint_user_avatar(
                                ui.painter(),
                                avatar_center,
                                av_radius,
                                &t.name,
                                t.is_group,
                                t.avatar.as_deref(),
                                &self.sticker_textures,
                                &mut self.loading_stickers,
                            );
                            // Cutout status dot (`2.5px` ring matching row background)
                            let cutout_col = if is_selected {
                                pal.bg_selected
                            } else {
                                pal.bg_sidebar
                            };
                            let badge_pos = Pos2::new(
                                avatar_center.x + av_radius * 0.71,
                                avatar_center.y + av_radius * 0.71,
                            );
                            ui.painter().circle_filled(badge_pos, 6.0, cutout_col);
                            ui.painter().circle_filled(
                                badge_pos,
                                4.0,
                                if t.is_group {
                                    pal.primary
                                } else {
                                    pal.accent_online
                                },
                            );

                            // Only show text labels when sidebar is expanded
                            if !is_narrow_w {
                                // Thread Name
                                let text_left = row_rect.left() + 58.0;
                                let name_color = if is_selected && pal.is_dark {
                                    Color32::WHITE
                                } else {
                                    pal.text_primary
                                };
                                let max_name_chars = if is_compact_w { 15 } else { 21 };
                                let display_name = truncate_chars(&t.name, max_name_chars);
                                ui.painter().text(
                                    Pos2::new(text_left, row_rect.top() + 12.0),
                                    egui::Align2::LEFT_TOP,
                                    display_name,
                                    FontId::proportional(13.6),
                                    name_color,
                                );

                                // Pin icon + Timestamp (top-right)
                                let time_right = row_rect.right() - 9.0;
                                if !t.time.is_empty() {
                                    let tw = ui.fonts(|f| {
                                        f.layout_no_wrap(
                                            t.time.clone(),
                                            FontId::proportional(11.0),
                                            pal.text_muted,
                                        )
                                        .size()
                                        .x
                                    });
                                    ui.painter().text(
                                        Pos2::new(time_right, row_rect.top() + 13.5),
                                        egui::Align2::RIGHT_TOP,
                                        &t.time,
                                        FontId::proportional(11.0),
                                        pal.text_muted,
                                    );
                                    if is_pinned_cloud {
                                        draw_icon_pin(
                                            ui.painter(),
                                            Pos2::new(
                                                time_right - tw - 9.0,
                                                row_rect.top() + 19.5,
                                            ),
                                            11.0,
                                            pal.text_muted,
                                        );
                                    }
                                }

                                // Snippet with `Bạn:` prefix when latest message is outgoing
                                let last_outgoing = t
                                    .messages
                                    .last()
                                    .map(|m| m.is_outgoing)
                                    .unwrap_or(false);
                                let raw_preview = if t.preview.is_empty() {
                                    "Chưa có tin nhắn".to_string()
                                } else if last_outgoing && !t.preview.starts_with("Bạn:") {
                                    format!("Bạn: {}", t.preview)
                                } else {
                                    t.preview.clone()
                                };
                                let max_snip_chars = if is_compact_w { 18 } else { 28 };
                                let snippet = truncate_chars(&raw_preview, max_snip_chars);
                                ui.painter().text(
                                    Pos2::new(text_left, row_rect.top() + 33.5),
                                    egui::Align2::LEFT_TOP,
                                    snippet,
                                    FontId::proportional(12.0),
                                    pal.text_secondary,
                                );
                            }

                            let row_resp = row_resp.on_hover_cursor(CursorIcon::PointingHand);
                            let row_resp = if is_narrow_w {
                                row_resp.on_hover_text(&t.name)
                            } else {
                                row_resp
                            };
                            if row_resp.clicked() {
                                clicked_thread = Some((i, t.id.clone(), t.is_group));
                            }
                        }

                        // Global Message Content Matches (`Tin nhắn tìm thấy`)
                        if !msg_hits.is_empty() {
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.add_space(12.0);
                                ui.label(
                                    RichText::new(format!(
                                        "TIN NHẮN TÌM THẤY ({})",
                                        msg_hits.len()
                                    ))
                                    .size(11.0)
                                    .color(pal.primary_hover)
                                    .strong(),
                                );
                            });
                            ui.add_space(4.0);
                            for (t_idx, t_name, m_text, m_time, t_group) in &msg_hits {
                                let row_width = ui.available_width() - 12.0;
                                let (r_rect, r_resp) = ui.allocate_exact_size(
                                    Vec2::new(row_width, 52.0),
                                    Sense::click(),
                                );
                                let r_rect = r_rect.translate(Vec2::new(6.0, 0.0));
                                if r_resp.hovered() {
                                    ui.painter().rect_filled(
                                        r_rect,
                                        Rounding::same(10.0),
                                        pal.bg_hover.linear_multiply(0.75),
                                    );
                                }
                                let av_c = Pos2::new(r_rect.left() + 24.0, r_rect.center().y);
                                let hit_ava = self
                                    .threads
                                    .get(*t_idx)
                                    .and_then(|th| th.avatar.as_deref());
                                paint_user_avatar(
                                    ui.painter(),
                                    av_c,
                                    16.0,
                                    t_name,
                                    *t_group,
                                    hit_ava,
                                    &self.sticker_textures,
                                    &mut self.loading_stickers,
                                );
                                let tl = r_rect.left() + 50.0;
                                ui.painter().text(
                                    Pos2::new(tl, r_rect.top() + 9.0),
                                    egui::Align2::LEFT_TOP,
                                    truncate_chars(t_name, 20),
                                    FontId::proportional(12.8),
                                    pal.text_primary,
                                );
                                if !m_time.is_empty() {
                                    ui.painter().text(
                                        Pos2::new(r_rect.right() - 10.0, r_rect.top() + 10.0),
                                        egui::Align2::RIGHT_TOP,
                                        m_time,
                                        FontId::proportional(10.5),
                                        pal.text_muted,
                                    );
                                }
                                ui.painter().text(
                                    Pos2::new(tl, r_rect.top() + 28.0),
                                    egui::Align2::LEFT_TOP,
                                    truncate_chars(m_text, 30),
                                    FontId::proportional(11.8),
                                    pal.primary_hover,
                                );
                                if r_resp
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text(m_text)
                                    .clicked()
                                {
                                    clicked_msg_hit = Some((*t_idx, query.clone()));
                                }
                            }
                        }
                    });

                if let Some((idx, q_str)) = clicked_msg_hit {
                    self.selected_thread_index = idx;
                    self.in_chat_search_open = true;
                    self.in_chat_search_query = q_str;
                }
                if let Some((idx, tid, is_group)) = clicked_thread {
                    if self.selected_thread_index != idx {
                        self.replying_to = None;
                        self.filter_starred_only = false;
                    }
                    self.selected_thread_index = idx;
                    let is_cloud = self
                        .threads
                        .get(idx)
                        .map(|t| t.name.contains("Cloud của tôi"))
                        .unwrap_or(false);
                    if is_group || is_cloud {
                        if let Some(tx) = WORKER_TX.get() {
                            let _ = tx.send(WorkerCmd::LoadGroupHistory { group_id: tid });
                        }
                    }
                }
            });

        // Extract lightweight metadata for Header, Right Info Drawer, and Central Chat Stream
        let thread_meta = self.threads.get(self.selected_thread_index).map(|t| {
            (
                t.id.clone(),
                t.name.clone(),
                t.avatar.clone(),
                t.is_group,
                t.messages.len(),
            )
        });

        // -------------------------------------------------------------------
        // 3. COLLAPSIBLE RIGHT CONVERSATION INFO DRAWER (responsive width)
        // -------------------------------------------------------------------
        let show_info_drawer = self.info_drawer_open && !is_compact_w;
        let drawer_w = (screen_w * 0.26).clamp(240.0, 310.0);

        if show_info_drawer {
            if let Some((ref d_id, ref d_name, ref d_ava, d_is_group, d_msg_count)) = thread_meta {
                let shared_photos: Vec<(String, String, String, String)> = self
                    .threads
                    .get(self.selected_thread_index)
                    .map(|t| {
                        t.messages
                            .iter()
                            .rev()
                            .filter_map(|m| {
                                let thumb = m
                                    .link_thumb
                                    .as_deref()
                                    .or_else(|| {
                                        if m.link_src.as_deref() == Some("Hình ảnh Zalo") {
                                            m.link_url.as_deref()
                                        } else {
                                            None
                                        }
                                    })
                                    .filter(|u| !u.is_empty())?;
                                let full = m.link_url.as_deref().unwrap_or(thumb).to_string();
                                let title = m
                                    .link_title
                                    .clone()
                                    .unwrap_or_else(|| "Hình ảnh đã chia sẻ".into());
                                Some((
                                    format!("thumb_{}", crate::crypto::md5_hex(thumb)),
                                    thumb.to_string(),
                                    full,
                                    title,
                                ))
                            })
                            .take(9)
                            .collect()
                    })
                    .unwrap_or_default();

                egui::SidePanel::right("conversation_info_drawer")
                    .resizable(true)
                    .default_width(drawer_w)
                    .width_range(220.0..=340.0)
                    .frame(
                        egui::Frame::none()
                            .fill(pal.bg_sidebar)
                            .stroke(Stroke::new(1.0_f32, pal.border)),
                    )
                    .show(ctx, |ui| {
                        // Profile Card Header
                        egui::Frame::none()
                            .inner_margin(Margin::symmetric(16.0, 18.0))
                            .show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    let (av_rect, _) = ui.allocate_exact_size(
                                        Vec2::new(64.0, 64.0),
                                        Sense::hover(),
                                    );
                                    paint_user_avatar(
                                        ui.painter(),
                                        av_rect.center(),
                                        32.0,
                                        d_name,
                                        d_is_group,
                                        d_ava.as_deref(),
                                        &self.sticker_textures,
                                        &mut self.loading_stickers,
                                    );
                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new(d_name)
                                            .size(14.5)
                                            .color(pal.text_primary)
                                            .strong(),
                                    );
                                    ui.add_space(2.0);
                                    ui.label(
                                        RichText::new(if d_is_group {
                                            "Nhóm đang hoạt động"
                                        } else {
                                            "Đang trực tuyến"
                                        })
                                        .size(12.0)
                                        .color(pal.accent_online),
                                    );

                                    ui.add_space(14.0);
                                    // Quick Action Buttons (Sync & Pin/Star Filter)
                                    ui.horizontal(|ui| {
                                        ui.add_space(34.0);
                                        let sync_btn = egui::Button::new(
                                            RichText::new("Đồng bộ")
                                                .size(11.5)
                                                .color(pal.text_secondary),
                                        )
                                        .fill(pal.bg_elevated)
                                        .stroke(Stroke::new(1.0_f32, pal.border))
                                        .rounding(Rounding::same(8.0))
                                        .min_size(Vec2::new(92.0, 30.0));
                                        if ui
                                            .add(sync_btn)
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::RefreshThreads);
                                                if d_is_group || d_name.contains("Cloud của tôi") {
                                                    let _ = tx.send(WorkerCmd::LoadGroupHistory {
                                                        group_id: d_id.clone(),
                                                    });
                                                }
                                            }
                                            self.show_toast("Đang đồng bộ lịch sử hội thoại…");
                                        }

                                        let star_active = self.filter_starred_only;
                                        let star_btn = egui::Button::new(
                                            RichText::new(if star_active {
                                                "Hiện tất cả"
                                            } else {
                                                "Tin đánh dấu"
                                            })
                                            .size(11.5)
                                            .color(if star_active {
                                                Color32::WHITE
                                            } else {
                                                pal.text_secondary
                                            }),
                                        )
                                        .fill(if star_active {
                                            pal.primary
                                        } else {
                                            pal.bg_elevated
                                        })
                                        .stroke(Stroke::new(1.0_f32, pal.border))
                                        .rounding(Rounding::same(8.0))
                                        .min_size(Vec2::new(92.0, 30.0));
                                        if ui
                                            .add(star_btn)
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            self.filter_starred_only = !self.filter_starred_only;
                                        }
                                    });
                                });
                            });

                        ui.add(egui::Separator::default().spacing(1.0));

                        // Shared Media Grid (`Ảnh / Video đã chia sẻ`)
                        egui::Frame::none()
                            .inner_margin(Margin::symmetric(16.0, 14.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Ảnh / Video đã chia sẻ")
                                            .size(12.5)
                                            .color(pal.text_primary)
                                            .strong(),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(
                                            RichText::new(format!("({})", shared_photos.len()))
                                                .size(11.5)
                                                .color(pal.primary_hover),
                                        );
                                    });
                                });
                                ui.add_space(8.0);

                                if shared_photos.is_empty() {
                                    ui.label(
                                        RichText::new("Chưa có ảnh hoặc video chia sẻ.")
                                            .size(11.5)
                                            .color(pal.text_muted),
                                    );
                                } else {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                                        for (tkey, thumb_u, full_u, ptitle) in &shared_photos {
                                            if !self.sticker_textures.contains_key(tkey)
                                                && self.loading_stickers.insert(tkey.clone())
                                            {
                                                if let Some(tx) = WORKER_TX.get() {
                                                    let _ = tx.send(WorkerCmd::FetchStickerImage {
                                                        key: tkey.clone(),
                                                        url: thumb_u.clone(),
                                                    });
                                                }
                                            }
                                            let (cell_r, cell_resp) = ui.allocate_exact_size(
                                                Vec2::splat(74.0),
                                                Sense::click(),
                                            );
                                            ui.painter().rect_filled(
                                                cell_r,
                                                Rounding::same(8.0),
                                                pal.bg_elevated,
                                            );
                                            if let Some(tex) = self.sticker_textures.get(tkey) {
                                                ui.painter().image(
                                                    tex.id(),
                                                    cell_r,
                                                    Rect::from_min_max(
                                                        Pos2::ZERO,
                                                        Pos2::new(1.0, 1.0),
                                                    ),
                                                    Color32::WHITE,
                                                );
                                            } else {
                                                draw_icon_image(
                                                    ui.painter(),
                                                    cell_r.center(),
                                                    20.0,
                                                    pal.text_muted,
                                                );
                                            }
                                            ui.painter().rect_stroke(
                                                cell_r,
                                                Rounding::same(8.0),
                                                Stroke::new(
                                                    1.0_f32,
                                                    if cell_resp.hovered() {
                                                        pal.primary
                                                    } else {
                                                        pal.border
                                                    },
                                                ),
                                            );
                                            if cell_resp
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .on_hover_text("Xem ảnh phóng to")
                                                .clicked()
                                            {
                                                self.image_viewer = Some(ImageViewerModal {
                                                    tex_key: tkey.clone(),
                                                    hd_tex_key: None,
                                                    full_url: full_u.clone(),
                                                    title: ptitle.clone(),
                                                    sender: d_name.clone(),
                                                    time: String::new(),
                                                    zoom: 1.0,
                                                });
                                            }
                                        }
                                    });
                                }
                            });

                        // Push technical metadata to the bottom of the drawer
                        let rem = (ui.available_height() - 88.0).max(8.0);
                        ui.add_space(rem);
                        ui.add(egui::Separator::default().spacing(1.0));

                        egui::Frame::none()
                            .fill(pal.bg_app.linear_multiply(0.55))
                            .inner_margin(Margin::symmetric(16.0, 12.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Tổng số tin nhắn:")
                                            .size(11.5)
                                            .color(pal.text_muted),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(
                                            RichText::new(format!("{d_msg_count} tin nhắn"))
                                                .size(11.5)
                                                .color(pal.text_secondary)
                                                .strong(),
                                        );
                                    });
                                });
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Mã định danh (ID):")
                                            .size(11.5)
                                            .color(pal.text_muted),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let short_id = if d_id.len() > 10 {
                                            format!(
                                                "{}...{}",
                                                &d_id[..4],
                                                &d_id[d_id.len().saturating_sub(4)..]
                                            )
                                        } else {
                                            d_id.clone()
                                        };
                                        let (id_rect, id_resp) = ui.allocate_exact_size(
                                            Vec2::new(98.0, 22.0),
                                            Sense::click(),
                                        );
                                        ui.painter().rect_filled(
                                            id_rect,
                                            Rounding::same(5.0),
                                            if id_resp.hovered() {
                                                pal.bg_hover
                                            } else {
                                                pal.bg_elevated
                                            },
                                        );
                                        ui.painter().text(
                                            Pos2::new(
                                                id_rect.left() + 7.0,
                                                id_rect.center().y,
                                            ),
                                            egui::Align2::LEFT_CENTER,
                                            short_id,
                                            FontId::monospace(10.5),
                                            pal.text_secondary,
                                        );
                                        draw_icon_copy(
                                            ui.painter(),
                                            Pos2::new(
                                                id_rect.right() - 10.0,
                                                id_rect.center().y,
                                            ),
                                            11.5,
                                            pal.text_secondary,
                                        );
                                        if id_resp
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .on_hover_text(format!("Sao chép ID: {d_id}"))
                                            .clicked()
                                        {
                                            ui.ctx().copy_text(d_id.clone());
                                            self.show_toast(
                                                "Đã sao chép ID hội thoại vào bộ nhớ tạm",
                                            );
                                        }
                                    });
                                });
                            });
                    });
            }
        }

        // -------------------------------------------------------------------
        // 4. CENTRAL CHAT AREA (Clean Header, Messages Stream, Icon Toolbar)
        // -------------------------------------------------------------------
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(pal.bg_chat_canvas))
            .show(ctx, |ui| {
                if let Some((thread_id, thread_name, thread_avatar, is_group, _msg_count)) =
                    thread_meta
                {
                    // 1. Clean Conversation Header Bar (No raw 19-digit ID, no bulky sync button)
                    egui::Frame::none()
                        .fill(pal.bg_sidebar)
                        .inner_margin(Margin::symmetric(18.0, 11.0))
                        .stroke(Stroke::new(1.0_f32, pal.border))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // Header Avatar (40x40) with Cutout Online Dot
                                let (av_rect, _) =
                                    ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::hover());
                                paint_user_avatar(
                                    ui.painter(),
                                    av_rect.center(),
                                    20.0,
                                    &thread_name,
                                    is_group,
                                    thread_avatar.as_deref(),
                                    &self.sticker_textures,
                                    &mut self.loading_stickers,
                                );
                                let dot_pos =
                                    Pos2::new(av_rect.right() - 4.0, av_rect.bottom() - 4.0);
                                ui.painter().circle_filled(dot_pos, 6.2, pal.bg_sidebar);
                                ui.painter().circle_filled(dot_pos, 4.2, pal.accent_online);

                                ui.add_space(6.0);
                                // Reserve room for the right-hand action cluster so the name/tag
                                // block can never be overlapped by the call buttons.
                                let actions_w = if is_compact_w { 82.0 } else { 124.0 };
                                let left_info_w =
                                    (ui.available_width() - actions_w).max(60.0);
                                ui.allocate_ui_with_layout(
                                    Vec2::new(left_info_w, 40.0),
                                    Layout::top_down(Align::Min),
                                    |ui| {
                                        ui.spacing_mut().item_spacing.y = 2.0;
                                        ui.horizontal(|ui| {
                                            let name_max = if is_compact_w { 12 } else { 22 };
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(truncate_chars(
                                                        &thread_name,
                                                        name_max,
                                                    ))
                                                    .size(15.0)
                                                    .color(pal.text_primary)
                                                    .strong(),
                                                )
                                                .truncate(),
                                            );
                                            if !is_compact_w && left_info_w > 190.0 {
                                                let tag_lbl =
                                                    if thread_name.contains("Cloud của tôi") {
                                                        "Đám mây"
                                                    } else if is_group {
                                                        "Nhóm"
                                                    } else {
                                                        "Bạn bè"
                                                    };
                                                egui::Frame::none()
                                                    .fill(pal.bg_elevated)
                                                    .rounding(Rounding::same(4.0))
                                                    .inner_margin(Margin::symmetric(6.0, 1.5))
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            RichText::new(tag_lbl)
                                                                .size(10.2)
                                                                .color(pal.text_secondary),
                                                        );
                                                    });
                                            }
                                        });
                                        ui.label(
                                            RichText::new(if is_group {
                                                "Đang hoạt động"
                                            } else {
                                                "Vừa mới truy cập"
                                            })
                                            .size(11.8)
                                            .color(pal.accent_online),
                                        );
                                    },
                                );

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    // Info Drawer Toggle Button (hidden on compact widths: auto-collapsed)
                                    if !is_compact_w {
                                        let (drw_rect, drw_resp) = ui.allocate_exact_size(
                                            Vec2::new(34.0, 34.0),
                                            Sense::click(),
                                        );
                                        ui.painter().rect_filled(
                                            drw_rect,
                                            Rounding::same(8.0),
                                            if self.info_drawer_open {
                                                pal.primary.linear_multiply(0.18)
                                            } else if drw_resp.hovered() {
                                                pal.bg_hover
                                            } else {
                                                Color32::TRANSPARENT
                                            },
                                        );
                                        draw_icon_drawer(
                                            ui.painter(),
                                            drw_rect.center(),
                                            17.0,
                                            if self.info_drawer_open {
                                                pal.primary_hover
                                            } else if drw_resp.hovered() {
                                                pal.text_primary
                                            } else {
                                                pal.text_secondary
                                            },
                                        );
                                        if drw_resp
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .on_hover_text("Thông tin hội thoại")
                                            .clicked()
                                        {
                                            self.info_drawer_open = !self.info_drawer_open;
                                        }
                                    }

                                    // Subtle vertical divider
                                    let (div_r, _) = ui.allocate_exact_size(
                                        Vec2::new(8.0, 20.0),
                                        Sense::hover(),
                                    );
                                    ui.painter().line_segment(
                                        [div_r.center_top(), div_r.center_bottom()],
                                        Stroke::new(1.0_f32, pal.border),
                                    );

                                    // Video Call Button
                                    let (vid_rect, vid_resp) = ui.allocate_exact_size(
                                        Vec2::new(34.0, 34.0),
                                        Sense::click(),
                                    );
                                    if vid_resp.hovered() {
                                        ui.painter().rect_filled(
                                            vid_rect,
                                            Rounding::same(8.0),
                                            pal.bg_hover,
                                        );
                                    }
                                    draw_icon_video(
                                        ui.painter(),
                                        vid_rect.center(),
                                        17.5,
                                        if vid_resp.hovered() {
                                            pal.text_primary
                                        } else {
                                            pal.text_secondary
                                        },
                                    );
                                    if vid_resp
                                        .on_hover_cursor(CursorIcon::PointingHand)
                                        .on_hover_text("Gọi Video")
                                        .clicked()
                                    {
                                        self.active_call = Some(ActiveCallOverlay {
                                            partner_id: thread_id.clone(),
                                            partner_name: thread_name.clone(),
                                            is_video: true,
                                            is_group,
                                            state_label: "Đang kết nối cuộc gọi video…".into(),
                                            started_at: std::time::Instant::now(),
                                            mic_muted: false,
                                            cam_enabled: true,
                                            is_incoming_ringing: false,
                                            error_code: None,
                                            error_reason: None,
                                        });
                                        if let Some(tx) = WORKER_TX.get() {
                                            let _ = tx.send(WorkerCmd::StartCall {
                                                partner_id: thread_id.clone(),
                                                partner_name: thread_name.clone(),
                                                partner_avatar: None,
                                                is_video: true,
                                                is_group,
                                            });
                                        }
                                    }

                                    // Voice Call Button
                                    let (aud_rect, aud_resp) = ui.allocate_exact_size(
                                        Vec2::new(34.0, 34.0),
                                        Sense::click(),
                                    );
                                    if aud_resp.hovered() {
                                        ui.painter().rect_filled(
                                            aud_rect,
                                            Rounding::same(8.0),
                                            pal.bg_hover,
                                        );
                                    }
                                    draw_icon_phone(
                                        ui.painter(),
                                        aud_rect.center(),
                                        17.5,
                                        if aud_resp.hovered() {
                                            pal.text_primary
                                        } else {
                                            pal.text_secondary
                                        },
                                    );
                                    if aud_resp
                                        .on_hover_cursor(CursorIcon::PointingHand)
                                        .on_hover_text("Gọi thoại")
                                        .clicked()
                                    {
                                        self.active_call = Some(ActiveCallOverlay {
                                            partner_id: thread_id.clone(),
                                            partner_name: thread_name.clone(),
                                            is_video: false,
                                            is_group,
                                            state_label: "Đang kết nối cuộc gọi thoại…".into(),
                                            started_at: std::time::Instant::now(),
                                            mic_muted: false,
                                            cam_enabled: false,
                                            is_incoming_ringing: false,
                                            error_code: None,
                                            error_reason: None,
                                        });
                                        if let Some(tx) = WORKER_TX.get() {
                                            let _ = tx.send(WorkerCmd::StartCall {
                                                partner_id: thread_id.clone(),
                                                partner_name: thread_name.clone(),
                                                partner_avatar: None,
                                                is_video: false,
                                                is_group,
                                            });
                                        }
                                    }
                                    // Search Messages in Conversation Button (`Ctrl+F`)
                                    let (srch_rect, srch_resp) = ui.allocate_exact_size(
                                        Vec2::new(34.0, 34.0),
                                        Sense::click(),
                                    );
                                    ui.painter().rect_filled(
                                        srch_rect,
                                        Rounding::same(8.0),
                                        if self.in_chat_search_open {
                                            pal.primary.linear_multiply(0.18)
                                        } else if srch_resp.hovered() {
                                            pal.bg_hover
                                        } else {
                                            Color32::TRANSPARENT
                                        },
                                    );
                                    draw_icon_search(
                                        ui.painter(),
                                        srch_rect.center(),
                                        16.5,
                                        if self.in_chat_search_open {
                                            pal.primary_hover
                                        } else if srch_resp.hovered() {
                                            pal.text_primary
                                        } else {
                                            pal.text_secondary
                                        },
                                    );
                                    if srch_resp
                                        .on_hover_cursor(CursorIcon::PointingHand)
                                        .on_hover_text("Tìm kiếm tin nhắn trong hội thoại này (Ctrl+F)")
                                        .clicked()
                                        || ui.input(|i| {
                                            (i.modifiers.ctrl || i.modifiers.command)
                                                && i.key_pressed(Key::F)
                                        })
                                    {
                                        self.in_chat_search_open = !self.in_chat_search_open;
                                        if !self.in_chat_search_open {
                                            self.in_chat_search_query.clear();
                                        }
                                    }
                                });
                            });
                        });
                    // 1b. Pinned Message Banner (if any message is pinned in this thread)
                    let pinned_msg_info = self
                        .threads
                        .get(self.selected_thread_index)
                        .and_then(|t| {
                            t.messages
                                .iter()
                                .enumerate()
                                .find(|(_, m)| m.is_pinned)
                                .map(|(idx, m)| {
                                    (
                                        idx,
                                        if m.is_outgoing {
                                            "Tôi".to_string()
                                        } else if !m.sender.is_empty() && m.sender != "0" {
                                            m.sender.clone()
                                        } else {
                                            thread_name.clone()
                                        },
                                        if m.sticker_id.is_some() {
                                            "[Nhãn dán]".to_string()
                                        } else {
                                            truncate_chars(&m.text, 80).into_owned()
                                        },
                                    )
                                })
                        });

                    let mut unpin_idx: Option<usize> = None;
                    if let Some((pidx, psender, ptext)) = pinned_msg_info {
                        egui::Frame::none()
                            .fill(pal.bg_elevated)
                            .stroke(Stroke::new(1.0_f32, pal.primary.linear_multiply(0.4)))
                            .inner_margin(Margin::symmetric(14.0, 7.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("{} Tin ghim:", lucide::PIN))
                                            .size(12.5)
                                            .color(pal.primary)
                                            .strong(),
                                    );
                                    ui.label(
                                        RichText::new(format!("{psender}: {ptext}"))
                                            .size(12.5)
                                            .color(pal.text_primary),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .small_button("Bỏ ghim ✕")
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            unpin_idx = Some(pidx);
                                        }
                                    });
                                });
                            });
                    }
                    if let Some(uidx) = unpin_idx {
                        if let Some(real) = self
                            .threads
                            .get_mut(self.selected_thread_index)
                            .and_then(|t| t.messages.get_mut(uidx))
                        {
                            real.is_pinned = false;
                        }
                        self.persist_threads_async();
                        self.status_msg = "Đã bỏ ghim tin nhắn".into();
                    }

                    // 1c. Starred filter active banner
                    if self.filter_starred_only {
                        egui::Frame::none()
                            .fill(Color32::from_rgb(234, 179, 8).linear_multiply(0.12))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(234, 179, 8)))
                            .inner_margin(Margin::symmetric(14.0, 6.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("{} Đang lọc tin nhắn đã đánh dấu sao", lucide::STAR))
                                            .size(12.0)
                                            .color(Color32::from_rgb(234, 179, 8))
                                            .strong(),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .small_button("Xem tất cả ✕")
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            self.filter_starred_only = false;
                                        }
                                    });
                                });
                            });
                    }

                    // 1d. In-Conversation Message Search Bar (`self.in_chat_search_open`)
                    if self.in_chat_search_open {
                        let q_low = self.in_chat_search_query.trim().to_lowercase();
                        let match_cnt = if q_low.is_empty() {
                            0
                        } else {
                            self.threads
                                .get(self.selected_thread_index)
                                .map(|t| {
                                    t.messages
                                        .iter()
                                        .filter(|m| m.text.to_lowercase().contains(&q_low))
                                        .count()
                                })
                                .unwrap_or(0)
                        };
                        egui::Frame::none()
                            .fill(pal.bg_sidebar)
                            .stroke(Stroke::new(1.0_f32, pal.border))
                            .inner_margin(Margin::symmetric(16.0, 7.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let (ir, _) = ui
                                        .allocate_exact_size(Vec2::new(16.0, 16.0), Sense::hover());
                                    draw_icon_search(ui.painter(), ir.center(), 14.0, pal.primary_hover);
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.in_chat_search_query)
                                            .hint_text(
                                                "Nhập từ khóa cần tìm trong đoạn hội thoại này...",
                                            )
                                            .desired_width(280.0),
                                    );
                                    if !q_low.is_empty() {
                                        ui.label(
                                            RichText::new(format!("Tìm thấy {match_cnt} tin nhắn"))
                                                .size(12.0)
                                                .color(if match_cnt > 0 {
                                                    pal.accent_online
                                                } else {
                                                    pal.danger
                                                })
                                                .strong(),
                                        );
                                    }
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .small_button("Đóng ✕")
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            self.in_chat_search_open = false;
                                            self.in_chat_search_query.clear();
                                        }
                                        if !self.in_chat_search_query.is_empty()
                                            && ui
                                                .small_button("Xóa từ khóa")
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .clicked()
                                        {
                                            self.in_chat_search_query.clear();
                                        }
                                    });
                                });
                            });
                    }

                    // 2. Scrollable Messages Area
                    let picker_reserve = (if self.sticker_picker_open { 268.0 } else { 0.0 })
                        + (if self.replying_to.is_some() { 48.0 } else { 0.0 })
                        + (if self.in_chat_search_open { 42.0 } else { 0.0 });
                    let mut pending_action: Option<(usize, BubbleAction)> = None;
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
                                            let only_starred = self.filter_starred_only;
                                            let in_chat_q = if self.in_chat_search_open {
                                                self.in_chat_search_query.trim().to_lowercase()
                                            } else {
                                                String::new()
                                            };
                                            let visible_indices: Vec<usize> = thread
                                                .messages
                                                .iter()
                                                .enumerate()
                                                .filter(|(_, m)| {
                                                    (!only_starred || m.is_starred)
                                                        && (in_chat_q.is_empty()
                                                            || m.text
                                                                .to_lowercase()
                                                                .contains(&in_chat_q)
                                                            || m.sender
                                                                .to_lowercase()
                                                                .contains(&in_chat_q))
                                                })
                                                .map(|(i, _)| i)
                                                .collect();
                                            let last_outgoing_idx = thread
                                                .messages
                                                .iter()
                                                .enumerate()
                                                .rev()
                                                .find(|(_, m)| {
                                                    m.is_outgoing && !is_system_notice_message(m)
                                                })
                                                .map(|(i, _)| i);

                                            for (pos, &idx) in visible_indices.iter().enumerate() {
                                                let msg = &thread.messages[idx];
                                                let prev_msg = pos
                                                    .checked_sub(1)
                                                    .and_then(|p| visible_indices.get(p))
                                                    .map(|&pi| &thread.messages[pi]);
                                                let next_msg = visible_indices
                                                    .get(pos + 1)
                                                    .map(|&ni| &thread.messages[ni]);

                                                if should_show_time_divider(prev_msg, msg) {
                                                    if pos > 0 {
                                                        ui.add_space(6.0);
                                                    }
                                                    render_date_divider(ui, msg, pal);
                                                    ui.add_space(6.0);
                                                }

                                                if is_system_notice_message(msg) {
                                                    render_system_notice_pill(ui, msg, pal);
                                                    ui.add_space(6.0);
                                                    continue;
                                                }

                                                let prev_same_sender = prev_msg
                                                    .map(|pm| same_message_cluster(pm, msg))
                                                    .unwrap_or(false);
                                                let next_same_sender = next_msg
                                                    .map(|nm| same_message_cluster(msg, nm))
                                                    .unwrap_or(false);
                                                let is_latest_outgoing =
                                                    last_outgoing_idx == Some(idx);

                                                let sender_ava_opt = if !is_group {
                                                    thread_avatar.as_deref()
                                                } else {
                                                    self.threads
                                                        .iter()
                                                        .find(|th| !th.is_group && th.name == msg.sender)
                                                        .and_then(|th| th.avatar.as_deref())
                                                };
                                                if let Some(action) = render_chat_bubble(
                                                    ui,
                                                    msg,
                                                    prev_same_sender,
                                                    next_same_sender,
                                                    is_latest_outgoing,
                                                    is_group,
                                                    &thread_name,
                                                    sender_ava_opt,
                                                    pal,
                                                    &self.sticker_textures,
                                                    &mut self.loading_stickers,
                                                    &self.emoji_textures,
                                                    idx,
                                                ) {
                                                    pending_action = Some((idx, action));
                                                }
                                                ui.add_space(if next_same_sender {
                                                    2.5
                                                } else {
                                                    9.0
                                                });
                                            }
                                        }
                                    }
                                });
                        });

                    if let Some((idx, action)) = pending_action {
                        let target = self
                            .threads
                            .get(self.selected_thread_index)
                            .and_then(|t| t.messages.get(idx))
                            .cloned();
                        if let Some(m) = target {
                            match action {
                                BubbleAction::Reply => self.replying_to = Some(m),
                                BubbleAction::Forward => {
                                    self.forward_msg = Some(m);
                                    self.forward_filter.clear();
                                }
                                BubbleAction::React(r) => {
                                    if let Some(real) = self
                                        .threads
                                        .get_mut(self.selected_thread_index)
                                        .and_then(|t| t.messages.get_mut(idx))
                                    {
                                        if real.reactions.contains(&r.to_string()) {
                                            real.reactions.clear();
                                            self.status_msg = "Đã bỏ cảm xúc".into();
                                        } else {
                                            real.reactions = vec![r.to_string()];
                                            self.status_msg = format!("Đã thả cảm xúc {r}");
                                        }
                                    }
                                    self.persist_threads_async();
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::React {
                                            to_id: thread_id.clone(),
                                            is_group,
                                            msg: m,
                                            reaction: r.to_string(),
                                        });
                                    }
                                }
                                BubbleAction::TogglePin | BubbleAction::ToggleStar => {
                                    if let Some(real) = self
                                        .threads
                                        .get_mut(self.selected_thread_index)
                                        .and_then(|t| t.messages.get_mut(idx))
                                    {
                                        if action == BubbleAction::TogglePin {
                                            real.is_pinned = !real.is_pinned;
                                            self.status_msg = if real.is_pinned {
                                                "Đã ghim tin nhắn lên đầu hội thoại".into()
                                            } else {
                                                "Đã bỏ ghim tin nhắn".into()
                                            };
                                        } else {
                                            real.is_starred = !real.is_starred;
                                            self.status_msg = if real.is_starred {
                                                "Đã đánh dấu sao tin nhắn".into()
                                            } else {
                                                "Đã bỏ đánh dấu sao".into()
                                            };
                                        }
                                    }
                                    self.persist_threads_async();
                                }
                                BubbleAction::CallAgain { is_video } => {
                                    self.active_call = Some(ActiveCallOverlay {
                                        partner_id: thread_id.clone(),
                                        partner_name: thread_name.clone(),
                                        is_video,
                                        is_group,
                                        state_label: if is_video {
                                            "Đang kết nối cuộc gọi video…".into()
                                        } else {
                                            "Đang kết nối cuộc gọi thoại…".into()
                                        },
                                        started_at: std::time::Instant::now(),
                                        mic_muted: false,
                                        cam_enabled: is_video,
                                        is_incoming_ringing: false,
                                        error_code: None,
                                        error_reason: None,
                                    });
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::StartCall {
                                            partner_id: thread_id.clone(),
                                            partner_name: thread_name.clone(),
                                            partner_avatar: None,
                                            is_video,
                                            is_group,
                                        });
                                    }
                                }
                                BubbleAction::DeleteForMe => {
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::DeleteForMe {
                                            to_id: thread_id.clone(),
                                            is_group,
                                            msg: m,
                                        });
                                    }
                                }
                                BubbleAction::OpenImagePopup {
                                    tex_key,
                                    full_url,
                                    title,
                                    sender,
                                    time,
                                } => {
                                    let hd_tex_key = if !full_url.is_empty() {
                                        let hk = format!(
                                            "thumb_{}",
                                            crate::crypto::md5_hex(&full_url)
                                        );
                                        if hk != tex_key
                                            && !self.sticker_textures.contains_key(&hk)
                                            && self.loading_stickers.insert(hk.clone())
                                        {
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::FetchStickerImage {
                                                    key: hk.clone(),
                                                    url: full_url.clone(),
                                                });
                                            }
                                        }
                                        Some(hk)
                                    } else {
                                        None
                                    };
                                    self.image_viewer = Some(ImageViewerModal {
                                        tex_key,
                                        hd_tex_key,
                                        full_url,
                                        title,
                                        sender,
                                        time,
                                        zoom: 1.0,
                                    });
                                }
                            }
                        }
                    }

                    // 3. Optional Sticker Picker Drawer
                    if self.sticker_picker_open {
                        self.render_sticker_picker(ui, &thread_id, is_group, pal);
                    }

                    // 3b. Reply banner (quoted message preview above the composer)
                    if self.replying_to.is_some() {
                        let mut cancel = false;
                        if let Some(q) = &self.replying_to {
                            let who = if q.is_outgoing {
                                "chính bạn".to_string()
                            } else if !q.sender.is_empty() && q.sender != "0" {
                                q.sender.clone()
                            } else {
                                thread_name.clone()
                            };
                            egui::Frame::none()
                                .fill(pal.bg_elevated)
                                .inner_margin(Margin::symmetric(14.0, 6.0))
                                .stroke(Stroke::new(1.0_f32, pal.border))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new(format!("Đang trả lời {who}"))
                                                    .size(12.0)
                                                    .color(pal.primary)
                                                    .strong(),
                                            );
                                            let preview = if q.sticker_id.is_some() {
                                                "[Nhãn dán]".to_string()
                                            } else {
                                                truncate_chars(&q.text, 100).into_owned()
                                            };
                                            ui.label(
                                                RichText::new(preview)
                                                    .size(12.0)
                                                    .color(pal.text_muted),
                                            );
                                        });
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .small_button("✕")
                                                    .on_hover_cursor(CursorIcon::PointingHand)
                                                    .on_hover_text("Hủy trả lời")
                                                    .clicked()
                                                {
                                                    cancel = true;
                                                }
                                            },
                                        );
                                    });
                                });
                        }
                        if cancel {
                            self.replying_to = None;
                        }
                    }

                    // 4. Bottom Message Composer Bar (Compact Icon Actions + Spacious Input + Padded Send Button)
                    egui::Frame::none()
                        .fill(pal.bg_sidebar)
                        .inner_margin(Margin::symmetric(14.0, 10.0))
                        .stroke(Stroke::new(1.0_f32, pal.border))
                        .show(ui, |ui| {
                            // Handle drag-and-drop files onto the chat window
                            let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|i| {
                                i.raw
                                    .dropped_files
                                    .iter()
                                    .filter_map(|df| df.path.clone())
                                    .collect()
                            });
                            for dpath in dropped {
                                if let Some(tx) = WORKER_TX.get() {
                                    let _ = tx.send(WorkerCmd::PickAndSendAttachment {
                                        to_id: thread_id.clone(),
                                        is_group,
                                        images_only: false,
                                        caption: String::new(),
                                        preset_path: Some(dpath),
                                    });
                                }
                            }

                            egui::Frame::none()
                                .fill(pal.bg_input)
                                .rounding(Rounding::same(11.0))
                                .stroke(Stroke::new(1.0_f32, pal.border))
                                .inner_margin(Margin::symmetric(8.0, 5.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;

                                        // 1) Compact Sticker Icon Button
                                        let (stk_rect, stk_resp) = ui.allocate_exact_size(
                                            Vec2::new(34.0, 34.0),
                                            Sense::click(),
                                        );
                                        if self.sticker_picker_open || stk_resp.hovered() {
                                            ui.painter().rect_filled(
                                                stk_rect,
                                                Rounding::same(8.0),
                                                pal.bg_hover,
                                            );
                                        }
                                        draw_icon_sticker(
                                            ui.painter(),
                                            stk_rect.center(),
                                            18.0,
                                            if self.sticker_picker_open || stk_resp.hovered() {
                                                Color32::from_rgb(251, 191, 36)
                                            } else {
                                                pal.text_secondary
                                            },
                                        );
                                        if stk_resp
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .on_hover_text("Nhãn dán (Sticker)")
                                            .clicked()
                                        {
                                            self.sticker_picker_open = !self.sticker_picker_open;
                                        }

                                        // 2) Compact Photo Upload Icon Button
                                        let (img_rect, img_resp) = ui.allocate_exact_size(
                                            Vec2::new(34.0, 34.0),
                                            Sense::click(),
                                        );
                                        if img_resp.hovered() {
                                            ui.painter().rect_filled(
                                                img_rect,
                                                Rounding::same(8.0),
                                                pal.bg_hover,
                                            );
                                        }
                                        draw_icon_image(
                                            ui.painter(),
                                            img_rect.center(),
                                            18.0,
                                            if img_resp.hovered() {
                                                pal.accent_online
                                            } else {
                                                pal.text_secondary
                                            },
                                        );
                                        if img_resp
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .on_hover_text("Gửi hình ảnh")
                                            .clicked()
                                        {
                                            let cap = self.input_message.trim().to_string();
                                            self.input_message.clear();
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::PickAndSendAttachment {
                                                    to_id: thread_id.clone(),
                                                    is_group,
                                                    images_only: true,
                                                    caption: cap,
                                                    preset_path: None,
                                                });
                                            }
                                        }

                                        // 3) Compact File Upload Icon Button
                                        let (att_rect, att_resp) = ui.allocate_exact_size(
                                            Vec2::new(34.0, 34.0),
                                            Sense::click(),
                                        );
                                        if att_resp.hovered() {
                                            ui.painter().rect_filled(
                                                att_rect,
                                                Rounding::same(8.0),
                                                pal.bg_hover,
                                            );
                                        }
                                        draw_icon_attachment(
                                            ui.painter(),
                                            att_rect.center(),
                                            17.0,
                                            if att_resp.hovered() {
                                                pal.primary_hover
                                            } else {
                                                pal.text_secondary
                                            },
                                        );
                                        if att_resp
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .on_hover_text("Đính kèm File / Video")
                                            .clicked()
                                        {
                                            let cap = self.input_message.trim().to_string();
                                            self.input_message.clear();
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::PickAndSendAttachment {
                                                    to_id: thread_id.clone(),
                                                    is_group,
                                                    images_only: false,
                                                    caption: cap,
                                                    preset_path: None,
                                                });
                                            }
                                        }

                                        // Vertical hairline separator
                                        let (sep_r, _) = ui.allocate_exact_size(
                                            Vec2::new(8.0, 20.0),
                                            Sense::hover(),
                                        );
                                        ui.painter().line_segment(
                                            [sep_r.center_top(), sep_r.center_bottom()],
                                            Stroke::new(1.0_f32, pal.border),
                                        );
                                        // Responsive Borderless Input Field (shrinks gracefully down to 50px)
                                        let send_btn_w = if is_narrow_w { 52.0 } else { 76.0 };
                                        let input_w =
                                            (ui.available_width() - send_btn_w - 12.0).max(50.0);
                                        let text_edit =
                                            egui::TextEdit::singleline(&mut self.input_message)
                                                .frame(false)
                                                .hint_text(format!(
                                                    "Nhập tin nhắn tới {}...",
                                                    truncate_chars(&thread_name, 16)
                                                ))
                                                .margin(Margin::symmetric(6.0, 7.0))
                                                .desired_width(input_w);

                                        let response = ui.add(text_edit);
                                        let enter_pressed =
                                            (response.lost_focus() || response.has_focus())
                                                && ui.input(|i| i.key_pressed(Key::Enter));

                                        // Responsive Padded Send Button (shows icon-only on very tight widths)
                                        let can_send = !self.input_message.trim().is_empty();
                                        let (send_rect, send_resp) = ui.allocate_exact_size(
                                            Vec2::new(send_btn_w, 34.0),
                                            Sense::click(),
                                        );
                                        let send_bg = if !can_send {
                                            pal.primary.linear_multiply(0.42)
                                        } else if send_resp.hovered() {
                                            pal.primary_hover
                                        } else {
                                            pal.primary
                                        };
                                        ui.painter().rect_filled(
                                            send_rect,
                                            Rounding::same(8.0),
                                            send_bg,
                                        );
                                        if !is_narrow_w {
                                            ui.painter().text(
                                                Pos2::new(
                                                    send_rect.left() + 15.0,
                                                    send_rect.center().y,
                                                ),
                                                egui::Align2::LEFT_CENTER,
                                                "Gửi",
                                                FontId::proportional(13.0),
                                                Color32::WHITE,
                                            );
                                            draw_icon_send(
                                                ui.painter(),
                                                Pos2::new(
                                                    send_rect.right() - 17.0,
                                                    send_rect.center().y,
                                                ),
                                                14.0,
                                                Color32::WHITE,
                                            );
                                        } else {
                                            draw_icon_send(
                                                ui.painter(),
                                                send_rect.center(),
                                                16.0,
                                                Color32::WHITE,
                                            );
                                        }
                                        draw_icon_send(
                                            ui.painter(),
                                            Pos2::new(
                                                send_rect.right() - 18.0,
                                                send_rect.center().y,
                                            ),
                                            14.0,
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
                                            let quoted = self.replying_to.take();
                                            if let Some(tx) = WORKER_TX.get() {
                                                if let Some(quoted) = quoted {
                                                    let _ = tx.send(WorkerCmd::SendQuote {
                                                        to_id: thread_id,
                                                        text,
                                                        quoted,
                                                        is_group,
                                                    });
                                                } else {
                                                    let _ = tx.send(WorkerCmd::SendMessage {
                                                        to_id: thread_id,
                                                        text,
                                                        is_group,
                                                    });
                                                }
                                            }
                                        } else if enter_pressed {
                                            response.request_focus();
                                        }
                                    });
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

        self.render_profile_popover(ctx, pal);
        self.render_toast_overlay(ctx, pal);
        self.render_forward_dialog(ctx, pal);
        self.render_call_overlay(ctx, pal);
        self.render_image_viewer_modal(ctx, pal);
        self.render_phone_lookup_modal(ctx, pal);
    }

    fn render_profile_popover(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        if !self.profile_menu_open {
            return;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.profile_menu_open = false;
            return;
        }

        let mut close_menu = false;
        let mut do_logout = false;
        let area_resp = egui::Area::new(Id::new("left_rail_profile_popover"))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::new(68.0, 14.0))
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(pal.bg_elevated)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .rounding(Rounding::same(12.0))
                    .inner_margin(Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.set_width(220.0);
                        let display_user = if self.user_name.is_empty() {
                            "Người dùng Zalo"
                        } else {
                            &self.user_name
                        };
                        ui.label(
                            RichText::new(display_user)
                                .size(14.0)
                                .color(pal.text_primary)
                                .strong(),
                        );
                        ui.horizontal(|ui| {
                            let (dr, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                            ui.painter()
                                .circle_filled(dr.center(), 3.8, pal.accent_online);
                            ui.label(
                                RichText::new("Đang hoạt động")
                                    .size(11.5)
                                    .color(pal.text_secondary),
                            );
                        });
                        if !self.user_uin.is_empty() {
                            ui.label(
                                RichText::new(format!("UIN: {}", self.user_uin))
                                    .size(10.5)
                                    .color(pal.text_muted),
                            );
                        }

                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(4.0);

                        if ui
                            .add_sized(
                                [200.0, 28.0],
                                egui::Button::new(
                                    RichText::new("✓  Đã đồng bộ đám mây")
                                        .size(12.2)
                                        .color(pal.text_primary),
                                )
                                .fill(Color32::TRANSPARENT),
                            )
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            if let Some(tx) = WORKER_TX.get() {
                                let _ = tx.send(WorkerCmd::RefreshThreads);
                            }
                            self.show_toast("Đang đồng bộ 20 hội thoại ngầm…");
                            close_menu = true;
                        }

                        let theme_lbl = if self.dark_mode {
                            format!("{}  Chuyển giao diện Sáng", lucide::SUN)
                        } else {
                            format!("{}  Chuyển giao diện Tối", lucide::MOON)
                        };
                        if ui
                            .add_sized(
                                [200.0, 28.0],
                                egui::Button::new(
                                    RichText::new(theme_lbl).size(12.2).color(pal.text_primary),
                                )
                                .fill(Color32::TRANSPARENT),
                            )
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.dark_mode = !self.dark_mode;
                            self.theme_dirty = true;
                            close_menu = true;
                        }

                        if self.dev_mode {
                            let dev_lbl = if self.dev_panel_open {
                                "Đóng Dev Inspector"
                            } else {
                                "Mở Dev Inspector"
                            };
                            if ui
                                .add_sized(
                                    [200.0, 28.0],
                                    egui::Button::new(
                                        RichText::new(dev_lbl).size(12.2).color(pal.warning),
                                    )
                                    .fill(Color32::TRANSPARENT),
                                )
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                self.dev_panel_open = !self.dev_panel_open;
                                if self.dev_panel_open {
                                    self.scanner_findings = logger::scan_logs();
                                }
                                close_menu = true;
                            }
                        }

                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);

                        if ui
                            .add_sized(
                                [200.0, 30.0],
                                egui::Button::new(
                                    RichText::new("Đăng xuất tài khoản")
                                        .size(12.5)
                                        .color(pal.danger)
                                        .strong(),
                                )
                                .fill(pal.danger.linear_multiply(0.12))
                                .rounding(Rounding::same(7.0)),
                            )
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            do_logout = true;
                            close_menu = true;
                        }
                    });
            });

        if ctx.input(|i| i.pointer.any_click())
            && !area_resp.response.hovered()
            && ctx
                .input(|i| i.pointer.interact_pos())
                .map(|p| p.x > 64.0 || p.y > 60.0)
                .unwrap_or(false)
        {
            close_menu = true;
        }

        if do_logout {
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
        if close_menu {
            self.profile_menu_open = false;
        }
    }

    fn render_toast_overlay(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        let Some((ref text, ts)) = self.toast_notice else {
            return;
        };
        if ts.elapsed() > Duration::from_millis(2500) {
            self.toast_notice = None;
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(200));
        let screen = ctx.screen_rect();
        egui::Area::new(Id::new("floating_toast_notice"))
            .order(egui::Order::Tooltip)
            .fixed_pos(Pos2::new((screen.right() - 320.0).max(80.0), screen.bottom() - 54.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(pal.bg_elevated)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .rounding(Rounding::same(10.0))
                    .inner_margin(Margin::symmetric(14.0, 9.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (dr, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                            ui.painter()
                                .circle_filled(dr.center(), 4.0, pal.accent_online);
                            ui.label(
                                RichText::new(text)
                                    .size(12.0)
                                    .color(pal.text_primary)
                                    .strong(),
                            );
                        });
                    });
            });
    }

    fn render_phone_lookup_modal(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        if !self.phone_modal_open {
            return;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.phone_modal_open = false;
            return;
        }

        let mut open = true;
        let mut trigger_lookup: Option<String> = None;
        let mut open_chat_user: Option<crate::chat::PhoneLookupResult> = None;
        let mut add_friend_user: Option<(String, String)> = None;

        egui::Window::new("Tìm kiếm Số điện thoại & Kết bạn Zalo")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width((ctx.screen_rect().width() * 0.78).clamp(280.0, 390.0))
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(pal.bg_elevated)
                    .rounding(Rounding::same(14.0))
                    .stroke(Stroke::new(1.2_f32, pal.primary))
                    .inner_margin(Margin::symmetric(20.0, 16.0)),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Nhập số điện thoại để tìm tài khoản Zalo (VD: 0912345678 hoặc +84...):")
                        .size(12.2)
                        .color(pal.text_secondary),
                );
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    let te = egui::TextEdit::singleline(&mut self.phone_modal_input)
                        .hint_text("Số điện thoại Zalo...")
                        .margin(Margin::symmetric(10.0, 7.0))
                        .desired_width(250.0);
                    let resp = ui.add(te);
                    let enter = (resp.lost_focus() || resp.has_focus())
                        && ui.input(|i| i.key_pressed(Key::Enter));

                    let search_btn = egui::Button::new(
                        RichText::new("Tìm kiếm")
                            .size(12.8)
                            .color(Color32::WHITE)
                            .strong(),
                    )
                    .fill(pal.primary)
                    .rounding(Rounding::same(8.0))
                    .min_size(Vec2::new(88.0, 32.0));

                    if (ui
                        .add(search_btn)
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .clicked()
                        || enter)
                        && !self.phone_modal_input.trim().is_empty()
                    {
                        trigger_lookup = Some(self.phone_modal_input.trim().to_string());
                    }
                });

                ui.add_space(12.0);

                if self.phone_lookup_loading {
                    ui.vertical_centered(|ui| {
                        ui.add_space(16.0);
                        ui.spinner();
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Đang tra cứu hồ sơ Zalo theo số điện thoại…")
                                .size(12.5)
                                .color(pal.text_secondary),
                        );
                        ui.add_space(12.0);
                    });
                } else if let Some(ref err) = self.phone_lookup_error {
                    egui::Frame::none()
                        .fill(pal.danger.linear_multiply(0.14))
                        .stroke(Stroke::new(1.0_f32, pal.danger))
                        .rounding(Rounding::same(10.0))
                        .inner_margin(Margin::symmetric(12.0, 10.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("Không tìm thấy: {err}"))
                                    .size(12.5)
                                    .color(pal.text_primary)
                                    .strong(),
                            );
                        });
                } else if let Some(ref prof) = self.phone_lookup_result {
                    egui::Frame::none()
                        .fill(pal.bg_input)
                        .stroke(Stroke::new(1.0_f32, pal.border))
                        .rounding(Rounding::same(12.0))
                        .inner_margin(Margin::symmetric(14.0, 12.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (av_rect, _) =
                                    ui.allocate_exact_size(Vec2::splat(54.0), Sense::hover());
                                paint_user_avatar(
                                    ui.painter(),
                                    av_rect.center(),
                                    27.0,
                                    &prof.display_name,
                                    false,
                                    Some(prof.avatar.as_str()),
                                    &self.sticker_textures,
                                    &mut self.loading_stickers,
                                );

                                ui.add_space(6.0);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(&prof.display_name)
                                            .size(15.0)
                                            .color(pal.text_primary)
                                            .strong(),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "SĐT: {} · UID: {}",
                                            prof.phone, prof.user_id
                                        ))
                                        .size(11.5)
                                        .color(pal.text_secondary),
                                    );
                                    if !prof.status.is_empty() {
                                        ui.label(
                                            RichText::new(&prof.status)
                                                .size(11.5)
                                                .color(pal.text_muted),
                                        );
                                    }
                                });
                            });

                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                let chat_btn = egui::Button::new(
                                    RichText::new("Nhắn tin ngay")
                                        .size(12.5)
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .fill(pal.primary)
                                .rounding(Rounding::same(8.0))
                                .min_size(Vec2::new(155.0, 32.0));
                                if ui
                                    .add(chat_btn)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    open_chat_user = Some(prof.clone());
                                }

                                let friend_btn = egui::Button::new(
                                    RichText::new("Kết bạn Zalo")
                                        .size(12.5)
                                        .color(pal.accent_online)
                                        .strong(),
                                )
                                .fill(pal.accent_online.linear_multiply(0.15))
                                .stroke(Stroke::new(1.0_f32, pal.accent_online))
                                .rounding(Rounding::same(8.0))
                                .min_size(Vec2::new(155.0, 32.0));
                                if ui
                                    .add(friend_btn)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    add_friend_user =
                                        Some((prof.user_id.clone(), prof.display_name.clone()));
                                }
                            });
                        });
                }
            });

        if let Some(phone_q) = trigger_lookup {
            self.phone_lookup_loading = true;
            self.phone_lookup_error = None;
            self.phone_lookup_result = None;
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::LookupPhone { phone: phone_q });
            }
        }

        if let Some(prof) = open_chat_user {
            if let Some(idx) = self.threads.iter().position(|t| t.id == prof.user_id) {
                self.selected_thread_index = idx;
            } else {
                self.threads.insert(
                    0,
                    Thread {
                        id: prof.user_id.clone(),
                        name: prof.display_name.clone(),
                        preview: format!("SĐT: {}", prof.phone),
                        time: crate::ui::clock(),
                        avatar: if prof.avatar.is_empty() {
                            None
                        } else {
                            Some(prof.avatar)
                        },
                        is_group: false,
                        messages: Vec::new(),
                    },
                );
                self.selected_thread_index = 0;
                self.persist_threads_async();
            }
            self.search_query.clear();
            self.phone_modal_open = false;
            self.show_toast(format!("Đã mở cuộc trò chuyện với {}", prof.display_name));
        }

        if let Some((uid, uname)) = add_friend_user {
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::AddFriend {
                    user_id: uid,
                    user_name: uname.clone(),
                });
            }
            self.show_toast(format!("Đang gửi lời mời kết bạn tới {uname}…"));
        }

        if !open {
            self.phone_modal_open = false;
        }
    }

    fn render_call_overlay(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        let Some(mut call) = self.active_call.clone() else {
            return;
        };
        // Keep timer updating every second while call modal is visible
        ctx.request_repaint_after(Duration::from_secs(1));

        let elapsed = call.started_at.elapsed().as_secs();
        let mins = elapsed / 60;
        let secs = elapsed % 60;
        let timer_str = format!("{mins:02}:{secs:02}");

        let title = if call.is_video {
            format!("{} Cuộc gọi Video Zalo (In-App + ZRTP)", lucide::VIDEO)
        } else {
            format!("{} Cuộc gọi Thoại Zalo (In-App + ZRTP)", lucide::PHONE)
        };

        let mut hangup_clicked = false;

        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width((ctx.screen_rect().width() * 0.76).clamp(280.0, 380.0))
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(pal.bg_elevated)
                    .rounding(Rounding::same(16.0))
                    .stroke(Stroke::new(1.5_f32, pal.primary))
                    .inner_margin(Margin::symmetric(24.0, 20.0)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(6.0);
                    // Partner Avatar Circle
                    let (av_rect, _) =
                        ui.allocate_exact_size(Vec2::new(72.0, 72.0), Sense::hover());
                    let call_ava = self
                        .threads
                        .iter()
                        .find(|t| t.id == call.partner_id || t.name == call.partner_name)
                        .and_then(|t| t.avatar.as_deref());
                    paint_user_avatar(
                        ui.painter(),
                        av_rect.center(),
                        36.0,
                        &call.partner_name,
                        call.is_group,
                        call_ava,
                        &self.sticker_textures,
                        &mut self.loading_stickers,
                    );
                    ui.painter().circle_stroke(
                        av_rect.center(),
                        36.0,
                        Stroke::new(2.0_f32, pal.primary),
                    );

                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(&call.partner_name)
                            .size(17.5)
                            .color(pal.text_primary)
                            .strong(),
                    );
                    if !call.partner_id.is_empty() {
                        ui.label(
                            RichText::new(format!("UID: {}", call.partner_id))
                                .size(11.5)
                                .color(pal.text_muted),
                        );
                    }

                    ui.add_space(6.0);
                    if let Some(ref err_msg) = call.error_reason {
                        ui.label(
                            RichText::new(&call.state_label)
                                .size(13.5)
                                .color(pal.danger)
                                .strong(),
                        );
                        ui.add_space(6.0);
                        egui::Frame::none()
                            .fill(pal.danger.linear_multiply(0.14))
                            .stroke(Stroke::new(1.0_f32, pal.danger))
                            .rounding(Rounding::same(8.0))
                            .inner_margin(Margin::symmetric(12.0, 8.0))
                            .show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new(format!("[!] {err_msg}"))
                                            .size(12.5)
                                            .color(pal.text_primary)
                                            .strong(),
                                    );
                                    if call.error_code == Some(7) {
                                        ui.add_space(3.0);
                                        ui.label(
                                            RichText::new(
                                                "Gợi ý: Tài khoản hiện tại chưa kết bạn (isFr=0) với người này nên máy chủ Zalo chặn cuộc gọi trước khi đổ chuông.",
                                            )
                                            .size(11.0)
                                            .color(pal.text_secondary),
                                        );
                                        if !call.partner_id.is_empty() && !call.is_group {
                                            ui.add_space(6.0);
                                            let add_fr_btn = egui::Button::new(
                                                RichText::new("+ Gửi lời mời kết bạn")
                                                    .size(12.0)
                                                    .color(Color32::WHITE)
                                                    .strong(),
                                            )
                                            .fill(pal.primary)
                                            .rounding(Rounding::same(6.0))
                                            .min_size(Vec2::new(150.0, 28.0));
                                            if ui
                                                .add(add_fr_btn)
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .clicked()
                                            {
                                                if let Some(tx) = WORKER_TX.get() {
                                                    let _ = tx.send(WorkerCmd::AddFriend {
                                                        user_id: call.partner_id.clone(),
                                                        user_name: call.partner_name.clone(),
                                                    });
                                                }
                                                call.state_label =
                                                    "Đã gửi lời mời kết bạn! Hãy chờ đối phương đồng ý.".into();
                                            }
                                        }
                                    } else if call.error_code == Some(1) {
                                        ui.add_space(3.0);
                                        ui.label(
                                            RichText::new(
                                                "Gợi ý: Hội thoại này là tài khoản hệ thống / Cloud của tôi nên không hỗ trợ cuộc gọi thoại/video.",
                                            )
                                            .size(11.0)
                                            .color(pal.text_secondary),
                                        );
                                    }
                                });
                            });
                    } else {
                        ui.label(
                            RichText::new(format!("{} · {timer_str}", call.state_label))
                                .size(13.5)
                                .color(pal.accent_online)
                                .strong(),
                        );
                    }

                    ui.add_space(14.0);
                    if call.is_incoming_ringing && call.error_reason.is_none() {
                        ui.horizontal(|ui| {
                            let avail = ui.available_width();
                            ui.add_space((avail - 210.0).max(0.0) * 0.5);

                            let accept_btn = egui::Button::new(
                                RichText::new(format!("{} Nghe máy", lucide::PHONE_CALL))
                                    .size(13.0)
                                    .color(Color32::WHITE)
                                    .strong(),
                            )
                            .fill(pal.accent_online)
                            .rounding(Rounding::same(8.0))
                            .min_size(Vec2::new(98.0, 36.0));
                            if ui
                                .add(accept_btn)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                call.is_incoming_ringing = false;
                                call.state_label = "Đang kết nối cuộc gọi…".into();
                                if let Some(tx) = WORKER_TX.get() {
                                    let _ = tx.send(WorkerCmd::AnswerCall { accept: true });
                                }
                            }

                            let decline_btn = egui::Button::new(
                                RichText::new(format!("{} Từ chối", lucide::PHONE_OFF))
                                    .size(13.0)
                                    .color(Color32::WHITE)
                                    .strong(),
                            )
                            .fill(pal.danger)
                            .rounding(Rounding::same(8.0))
                            .min_size(Vec2::new(98.0, 36.0));
                            if ui
                                .add(decline_btn)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                if let Some(tx) = WORKER_TX.get() {
                                    let _ = tx.send(WorkerCmd::AnswerCall { accept: false });
                                }
                                self.active_call = None;
                                return;
                            }
                        });
                    } else {
                        ui.horizontal(|ui| {
                            let avail = ui.available_width();
                            ui.add_space((avail - 280.0).max(0.0) * 0.5);

                            // Mic Toggle
                            let mic_label = if call.mic_muted {
                                format!("{} Bật Mic", lucide::MIC_OFF)
                            } else {
                                format!("{} Tắt Mic", lucide::MIC)
                            };
                            let mic_btn = egui::Button::new(
                                RichText::new(mic_label)
                                    .size(12.5)
                                    .color(pal.text_primary)
                                    .strong(),
                            )
                            .fill(if call.mic_muted {
                                pal.warning.linear_multiply(0.25)
                            } else {
                                pal.bg_input
                            })
                            .rounding(Rounding::same(8.0))
                            .min_size(Vec2::new(84.0, 34.0));
                            if ui
                                .add(mic_btn)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                call.mic_muted = !call.mic_muted;
                                if let Some(tx) = WORKER_TX.get() {
                                    let _ = tx.send(WorkerCmd::SetCallMediaState {
                                        is_on_mic: !call.mic_muted,
                                        is_on_cam: call.cam_enabled,
                                    });
                                }
                            }

                            // Camera Toggle
                            let cam_label = if call.cam_enabled {
                                format!("{} Tắt Cam", lucide::VIDEO_OFF)
                            } else {
                                format!("{} Bật Cam", lucide::CAMERA)
                            };
                            let cam_btn = egui::Button::new(
                                RichText::new(cam_label)
                                    .size(12.5)
                                    .color(pal.text_primary)
                                    .strong(),
                            )
                            .fill(if call.cam_enabled {
                                pal.primary.linear_multiply(0.25)
                            } else {
                                pal.bg_input
                            })
                            .rounding(Rounding::same(8.0))
                            .min_size(Vec2::new(84.0, 34.0));
                            if ui
                                .add(cam_btn)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                call.cam_enabled = !call.cam_enabled;
                                call.is_video = call.cam_enabled;
                                if let Some(tx) = WORKER_TX.get() {
                                    let _ = tx.send(WorkerCmd::SetCallMediaState {
                                        is_on_mic: !call.mic_muted,
                                        is_on_cam: call.cam_enabled,
                                    });
                                }
                            }

                            // End Call / Close Button
                            let end_text = if call.error_reason.is_some() {
                                format!("{} Đóng", lucide::X)
                            } else {
                                format!("{} Kết thúc", lucide::PHONE_OFF)
                            };
                            let end_btn = egui::Button::new(
                                RichText::new(end_text)
                                    .size(12.5)
                                    .color(Color32::WHITE)
                                    .strong(),
                            )
                            .fill(pal.danger)
                            .rounding(Rounding::same(8.0))
                            .min_size(Vec2::new(92.0, 34.0));
                            if ui
                                .add(end_btn)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                hangup_clicked = true;
                            }
                        });
                    }
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("100% Pure-Rust VoIP Signaling (Không dùng tiến trình ZaloCall.exe)")
                            .size(10.5)
                            .color(pal.text_muted),
                    );
                });
            });

        if hangup_clicked {
            self.active_call = None;
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::EndCall);
            }
        } else if self.active_call.is_some() {
            self.active_call = Some(call);
        }
    }

    fn render_forward_dialog(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        if self.forward_msg.is_none() {
            return;
        }
        let preview = self
            .forward_msg
            .as_ref()
            .map(|m| {
                if m.sticker_id.is_some() {
                    "[Nhãn dán]".to_string()
                } else {
                    truncate_chars(&m.text, 80).into_owned()
                }
            })
            .unwrap_or_default();
        let query = self.forward_filter.to_lowercase();
        let candidates: Vec<(String, String, bool)> = self
            .threads
            .iter()
            .filter(|t| query.is_empty() || t.name.to_lowercase().contains(&query))
            .map(|t| (t.id.clone(), t.name.clone(), t.is_group))
            .collect();
        let mut open = true;
        let mut chosen: Option<(String, bool)> = None;
        egui::Window::new("Chuyển tiếp tin nhắn")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(360.0, 420.0))
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!("Nội dung: {preview}"))
                        .size(12.5)
                        .color(pal.text_secondary),
                );
                ui.add_space(6.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.forward_filter)
                        .hint_text("Tìm hội thoại…")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for (id, name, is_group) in &candidates {
                            let label = if *is_group {
                                format!("[Nhóm] {name}")
                            } else {
                                name.clone()
                            };
                            let btn = egui::Button::new(RichText::new(label).size(13.5))
                                .fill(pal.bg_input);
                            if ui
                                .add_sized([ui.available_width(), 30.0], btn)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                chosen = Some((id.clone(), *is_group));
                            }
                        }
                    });
            });
        if let Some((target_id, target_is_group)) = chosen {
            if let (Some(msg), Some(tx)) = (self.forward_msg.take(), WORKER_TX.get()) {
                let _ = tx.send(WorkerCmd::Forward {
                    target_id,
                    target_is_group,
                    msg,
                });
                self.status_msg = "Đang chuyển tiếp…".into();
            }
        } else if !open {
            self.forward_msg = None;
        }
    }

    fn render_image_viewer_modal(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        let Some(mut viewer) = self.image_viewer.clone() else {
            return;
        };

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.image_viewer = None;
            return;
        }

        // Smooth mouse-wheel zoom while lightbox is open
        let scroll_delta_y = ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll_delta_y.abs() > 0.5 && ctx.input(|i| i.modifiers.ctrl || i.modifiers.command) {
            viewer.zoom = (viewer.zoom + scroll_delta_y * 0.003).clamp(0.4, 4.0);
        }

        let screen_rect = ctx.screen_rect();
        let mut should_close = false;

        // Dimmed full-screen backdrop
        egui::Area::new(Id::new("image_viewer_backdrop"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen_rect.min)
            .show(ctx, |ui| {
                let (bg_rect, bg_resp) =
                    ui.allocate_exact_size(screen_rect.size(), Sense::click());
                ui.painter().rect_filled(
                    bg_rect,
                    Rounding::ZERO,
                    Color32::from_rgba_unmultiplied(6, 10, 20, 218),
                );
                if bg_resp.clicked() {
                    should_close = true;
                }
            });

        let active_tex = viewer
            .hd_tex_key
            .as_ref()
            .and_then(|hk| self.sticker_textures.get(hk))
            .or_else(|| self.sticker_textures.get(&viewer.tex_key))
            .cloned();

        let modal_w = (screen_rect.width() * 0.92).clamp(260.0, 1080.0);
        let modal_h = (screen_rect.height() * 0.9).clamp(240.0, 820.0);

        egui::Window::new("image_viewer_popup")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(modal_w, modal_h))
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(15, 23, 42))
                    .stroke(Stroke::new(1.2_f32, Color32::from_rgb(51, 65, 85)))
                    .rounding(Rounding::same(14.0))
                    .inner_margin(Margin::same(0.0)),
            )
            .show(ctx, |ui| {
                ui.set_min_size(Vec2::new(modal_w, modal_h));

                // 1. Top Toolbar Header
                egui::Frame::none()
                    .fill(Color32::from_rgb(17, 24, 39))
                    .inner_margin(Margin::symmetric(16.0, 10.0))
                    .rounding(Rounding {
                        nw: 14.0,
                        ne: 14.0,
                        sw: 0.0,
                        se: 0.0,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (ir, _) =
                                ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                            draw_icon_image(
                                ui.painter(),
                                ir.center(),
                                18.0,
                                Color32::from_rgb(96, 165, 250),
                            );
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.label(
                                    RichText::new(truncate_chars(&viewer.title, 56))
                                        .size(13.8)
                                        .color(Color32::from_rgb(248, 250, 252))
                                        .strong(),
                                );
                                let dim_info = if let Some(ref tex) = active_tex {
                                    let [tw, th] = tex.size();
                                    format!(" · {tw}×{th} px")
                                } else {
                                    " · Đang tải ảnh gốc…".to_string()
                                };
                                let meta_line = if viewer.time.is_empty() {
                                    format!("{}{dim_info}", viewer.sender)
                                } else {
                                    format!("{} • {}{dim_info}", viewer.sender, viewer.time)
                                };
                                ui.label(
                                    RichText::new(meta_line)
                                        .size(11.5)
                                        .color(Color32::from_rgb(148, 163, 184)),
                                );
                            });

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                // Close button
                                let close_btn = egui::Button::new(
                                    RichText::new("✕ Đóng")
                                        .size(12.5)
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .fill(Color32::from_rgb(220, 38, 38))
                                .rounding(Rounding::same(7.0))
                                .min_size(Vec2::new(68.0, 28.0));
                                if ui
                                    .add(close_btn)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text("Đóng cửa sổ xem ảnh (Esc)")
                                    .clicked()
                                {
                                    should_close = true;
                                }

                                // Optional: Open in browser if URL is available
                                if !viewer.full_url.is_empty() {
                                    let ext_btn = egui::Button::new(
                                        RichText::new(format!("{} Mở ngoài ↗", lucide::GLOBE))
                                            .size(12.0)
                                            .color(Color32::from_rgb(226, 232, 240)),
                                    )
                                    .fill(Color32::from_rgb(30, 41, 59))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(51, 65, 85)))
                                    .rounding(Rounding::same(7.0))
                                    .min_size(Vec2::new(88.0, 28.0));
                                    if ui
                                        .add(ext_btn)
                                        .on_hover_cursor(CursorIcon::PointingHand)
                                        .on_hover_text(&viewer.full_url)
                                        .clicked()
                                    {
                                        open_external_url(&viewer.full_url);
                                    }

                                    let copy_btn = egui::Button::new(
                                        RichText::new(format!("{} Copy link", lucide::COPY))
                                            .size(12.0)
                                            .color(Color32::from_rgb(226, 232, 240)),
                                    )
                                    .fill(Color32::from_rgb(30, 41, 59))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(51, 65, 85)))
                                    .rounding(Rounding::same(7.0))
                                    .min_size(Vec2::new(82.0, 28.0));
                                    if ui
                                        .add(copy_btn)
                                        .on_hover_cursor(CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        ui.ctx().copy_text(viewer.full_url.clone());
                                        self.status_msg =
                                            "Đã sao chép đường dẫn hình ảnh vào bộ nhớ tạm".into();
                                    }
                                }

                                // Zoom controls (+ / 100% / -)
                                let zin_btn = egui::Button::new(
                                    RichText::new("＋")
                                        .size(13.5)
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .fill(Color32::from_rgb(30, 41, 59))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(51, 65, 85)))
                                .rounding(Rounding::same(7.0))
                                .min_size(Vec2::new(30.0, 28.0));
                                if ui
                                    .add(zin_btn)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text("Phóng to (+25%)")
                                    .clicked()
                                {
                                    viewer.zoom = (viewer.zoom + 0.25).min(4.0);
                                }

                                let zoom_pct = format!("{:.0}%", viewer.zoom * 100.0);
                                let zreset_btn = egui::Button::new(
                                    RichText::new(zoom_pct)
                                        .size(12.0)
                                        .color(Color32::from_rgb(147, 197, 253))
                                        .strong(),
                                )
                                .fill(Color32::from_rgb(30, 41, 59))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(51, 65, 85)))
                                .rounding(Rounding::same(7.0))
                                .min_size(Vec2::new(54.0, 28.0));
                                if ui
                                    .add(zreset_btn)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text("Đặt lại kích thước vừa khung (100%)")
                                    .clicked()
                                {
                                    viewer.zoom = 1.0;
                                }

                                let zout_btn = egui::Button::new(
                                    RichText::new("－")
                                        .size(13.5)
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .fill(Color32::from_rgb(30, 41, 59))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(51, 65, 85)))
                                .rounding(Rounding::same(7.0))
                                .min_size(Vec2::new(30.0, 28.0));
                                if ui
                                    .add(zout_btn)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text("Thu nhỏ (-25%)")
                                    .clicked()
                                {
                                    viewer.zoom = (viewer.zoom - 0.25).max(0.4);
                                }
                            });
                        });
                    });

                // Thin separator line
                let (sep_rect, _) =
                    ui.allocate_exact_size(Vec2::new(modal_w, 1.0), Sense::hover());
                ui.painter().line_segment(
                    [sep_rect.left_center(), sep_rect.right_center()],
                    Stroke::new(1.0_f32, Color32::from_rgb(38, 50, 72)),
                );

                // 2. Central Image Canvas
                let body_h = (modal_h - 58.0).max(180.0);
                let avail_canvas = Vec2::new(modal_w - 28.0, body_h - 24.0);

                egui::ScrollArea::both()
                    .id_salt("image_viewer_scroll_canvas")
                    .max_width(modal_w)
                    .max_height(body_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_size(Vec2::new(modal_w, body_h));
                        if let Some(tex) = active_tex {
                            let [tw, th] = tex.size();
                            let tw_f = (tw.max(1)) as f32;
                            let th_f = (th.max(1)) as f32;
                            let base_scale = (avail_canvas.x / tw_f)
                                .min(avail_canvas.y / th_f)
                                .min(1.6);
                            let disp_w = (tw_f * base_scale * viewer.zoom).max(64.0);
                            let disp_h = (th_f * base_scale * viewer.zoom).max(64.0);
                            let pad_x = ((modal_w - disp_w) * 0.5).max(12.0);
                            let pad_y = ((body_h - disp_h) * 0.5).max(12.0);

                            ui.add_space(pad_y);
                            ui.horizontal(|ui| {
                                ui.add_space(pad_x);
                                let img_widget = egui::Image::new(&tex)
                                    .fit_to_exact_size(Vec2::new(disp_w, disp_h))
                                    .rounding(Rounding::same(8.0));
                                let img_resp = ui.add(img_widget);
                                ui.painter().rect_stroke(
                                    img_resp.rect,
                                    Rounding::same(8.0),
                                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 36)),
                                );
                            });
                            ui.add_space(pad_y);
                        } else {
                            ui.vertical_centered(|ui| {
                                ui.add_space(body_h * 0.42);
                                ui.spinner();
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new("Đang tải hình ảnh độ phân giải cao…")
                                        .size(13.5)
                                        .color(pal.text_secondary),
                                );
                            });
                        }
                    });
            });

        if should_close {
            self.image_viewer = None;
        } else {
            self.image_viewer = Some(viewer);
        }
    }

    fn render_sticker_picker(
        &mut self,
        ui: &mut egui::Ui,
        thread_id: &str,
        is_group: bool,
        pal: &ThemePalette,
    ) {
        // Trigger background store catalog load once when drawer opens
        if self.sticker_store_packs.is_empty() && !self.sticker_store_loading {
            self.sticker_store_loading = true;
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::FetchStickerStore);
            }
        }

        egui::Frame::none()
            .fill(pal.bg_sidebar)
            .stroke(Stroke::new(1.0_f32, pal.border))
            .inner_margin(Margin::symmetric(14.0, 8.0))
            .show(ui, |ui| {
                ui.set_height(248.0);

                // 1. Top Pack & Store Navigation Bar
                ui.horizontal(|ui| {
                    let store_count_lbl = if !self.sticker_store_packs.is_empty() {
                        format!("{} Kho Nhãn Dán ({})", lucide::STORE, self.sticker_store_packs.len())
                    } else {
                        format!("{} Kho Nhãn Dán", lucide::STORE)
                    };

                    for (tab, title) in [
                        (StickerTab::Popular, format!("{} Phổ biến", lucide::FLAME)),
                        (StickerTab::Onion, "Củ hành (30)".to_string()),
                        (StickerTab::Search, format!("{} Tìm nhãn", lucide::SEARCH)),
                        (StickerTab::Store, store_count_lbl),
                    ] {
                        let active = self.sticker_tab == tab;
                        let btn = egui::Button::new(
                            RichText::new(title)
                                .size(12.2)
                                .color(if active {
                                    Color32::WHITE
                                } else {
                                    pal.text_primary
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
                            .small_button("✕ Đóng")
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.sticker_picker_open = false;
                        }
                    });
                });

                ui.add_space(3.0);

                // 1b. Pinned / Featured Pack Strip (10+ packs + any pack opened from the Store)
                egui::ScrollArea::horizontal()
                    .id_salt("sticker_pack_tabs_scroll")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            let pinned_snapshot = self.pinned_sticker_packs.clone();
                            for (cid, pname) in pinned_snapshot {
                                let tab = StickerTab::Pack(cid);
                                let active = self.sticker_tab == tab;
                                let btn = egui::Button::new(
                                    RichText::new(&pname)
                                        .size(11.8)
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
                                    pal.bg_elevated
                                })
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
                                    if !self.sticker_pack_items.contains_key(&cid)
                                        && self.loading_sticker_packs.insert(cid)
                                    {
                                        if let Some(tx) = WORKER_TX.get() {
                                            let _ = tx.send(WorkerCmd::FetchStickerPack {
                                                cate_id: cid,
                                                name: pname,
                                            });
                                        }
                                    }
                                }
                            }
                        });
                    });

                ui.add_space(4.0);

                // 2. Tab Body
                match self.sticker_tab {
                    StickerTab::Store => {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Lọc bộ nhãn:")
                                    .size(12.0)
                                    .color(pal.text_secondary),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut self.sticker_store_filter)
                                    .hint_text("Tìm trong 390+ bộ nhãn Zalo (VD: Zapy, Mèo, Gấu, Tết, Cò Lõ…)")
                                    .desired_width(340.0),
                            );
                            if !self.sticker_store_filter.is_empty()
                                && ui.small_button("Xóa").clicked()
                            {
                                self.sticker_store_filter.clear();
                            }
                            if ui
                                .small_button("🔄 Tải lại kho")
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                self.sticker_store_loading = true;
                                if let Some(tx) = WORKER_TX.get() {
                                    let _ = tx.send(WorkerCmd::FetchStickerStore);
                                }
                            }
                        });
                        ui.add_space(4.0);

                        if self.sticker_store_loading && self.sticker_store_packs.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(36.0);
                                ui.spinner();
                                ui.label("Đang tải danh mục Kho Nhãn Dán Zalo…");
                            });
                            return;
                        }

                        let q = self.sticker_store_filter.trim().to_lowercase();
                        let packs_snapshot: Vec<crate::chat::StickerPackInfo> = self
                            .sticker_store_packs
                            .iter()
                            .filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q))
                            .take(120)
                            .cloned()
                            .collect();

                        let mut open_pack: Option<(u64, String)> = None;
                        egui::ScrollArea::vertical()
                            .id_salt("sticker_store_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                                    for pack in &packs_snapshot {
                                        let icon_key = format!("pack_ic_{}", pack.id);
                                        if !pack.icon_url.is_empty()
                                            && !self.sticker_textures.contains_key(&icon_key)
                                            && self.loading_stickers.insert(icon_key.clone())
                                        {
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::FetchStickerImage {
                                                    key: icon_key.clone(),
                                                    url: pack.icon_url.clone(),
                                                });
                                            }
                                        }

                                        let (card_rect, card_resp) = ui.allocate_exact_size(
                                            Vec2::new(195.0, 56.0),
                                            Sense::click(),
                                        );
                                        let card_bg = if card_resp.hovered() {
                                            pal.bg_hover
                                        } else {
                                            pal.bg_elevated
                                        };
                                        let card_border = if card_resp.hovered() {
                                            pal.primary
                                        } else {
                                            pal.border
                                        };
                                        ui.painter().rect_filled(
                                            card_rect,
                                            Rounding::same(8.0),
                                            card_bg,
                                        );
                                        ui.painter().rect_stroke(
                                            card_rect,
                                            Rounding::same(8.0),
                                            Stroke::new(1.0_f32, card_border),
                                        );

                                        let ic_rect = Rect::from_min_size(
                                            Pos2::new(card_rect.left() + 6.0, card_rect.top() + 6.0),
                                            Vec2::splat(44.0),
                                        );
                                        if let Some(tex) = self.sticker_textures.get(&icon_key) {
                                            ui.painter().image(
                                                tex.id(),
                                                ic_rect,
                                                Rect::from_min_max(
                                                    Pos2::ZERO,
                                                    Pos2::new(1.0, 1.0),
                                                ),
                                                Color32::WHITE,
                                            );
                                        } else {
                                            ui.painter().rect_filled(
                                                ic_rect,
                                                Rounding::same(6.0),
                                                pal.primary.linear_multiply(0.15),
                                            );
                                            draw_icon_sticker(
                                                ui.painter(),
                                                ic_rect.center(),
                                                18.0,
                                                pal.primary,
                                            );
                                        }

                                        ui.painter().text(
                                            Pos2::new(
                                                card_rect.left() + 56.0,
                                                card_rect.top() + 19.0,
                                            ),
                                            egui::Align2::LEFT_CENTER,
                                            truncate_chars(&pack.name, 18),
                                            FontId::proportional(12.5),
                                            pal.text_primary,
                                        );
                                        ui.painter().text(
                                            Pos2::new(
                                                card_rect.left() + 56.0,
                                                card_rect.top() + 38.0,
                                            ),
                                            egui::Align2::LEFT_CENTER,
                                            format!("{} nhãn · Bấm để mở", pack.total_image),
                                            FontId::proportional(11.0),
                                            pal.primary,
                                        );

                                        if card_resp
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .on_hover_text(format!(
                                                "Mở bộ nhãn \"{}\" (ID: {})",
                                                pack.name, pack.id
                                            ))
                                            .clicked()
                                        {
                                            open_pack = Some((pack.id, pack.name.clone()));
                                        }
                                    }
                                });
                            });

                        if let Some((cid, pname)) = open_pack {
                            if !self.pinned_sticker_packs.iter().any(|(id, _)| *id == cid) {
                                self.pinned_sticker_packs.insert(0, (cid, pname.clone()));
                            }
                            self.sticker_tab = StickerTab::Pack(cid);
                            if !self.sticker_pack_items.contains_key(&cid)
                                && self.loading_sticker_packs.insert(cid)
                            {
                                if let Some(tx) = WORKER_TX.get() {
                                    let _ = tx.send(WorkerCmd::FetchStickerPack {
                                        cate_id: cid,
                                        name: pname,
                                    });
                                }
                            }
                        }
                    }
                    StickerTab::Search => {
                        let mut trigger_kw: Option<String> = None;
                        ui.horizontal(|ui| {
                            let te = egui::TextEdit::singleline(&mut self.sticker_search_query)
                                .hint_text("Nhập từ khóa cảm xúc (xin chào, cảm ơn, yêu, haha, ok, buồn…)")
                                .desired_width(260.0);
                            let resp = ui.add(te);
                            let enter = (resp.lost_focus() || resp.has_focus())
                                && ui.input(|i| i.key_pressed(Key::Enter));
                            if (ui
                                .button(format!("{} Tìm", lucide::SEARCH))
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                                || enter)
                                && !self.sticker_search_query.trim().is_empty()
                            {
                                trigger_kw = Some(self.sticker_search_query.trim().to_string());
                            }

                            for chip in [
                                "xin chào",
                                "cảm ơn",
                                "haha",
                                "ok",
                                "yêu",
                                "buồn",
                                "ngủ ngon",
                                "chúc mừng",
                            ] {
                                if ui
                                    .small_button(chip)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    self.sticker_search_query = chip.to_string();
                                    trigger_kw = Some(chip.to_string());
                                }
                            }
                        });
                        if let Some(kw) = trigger_kw {
                            self.sticker_search_loading = true;
                            if let Some(tx) = WORKER_TX.get() {
                                let _ = tx.send(WorkerCmd::SearchStickers { keyword: kw });
                            }
                        }
                        ui.add_space(4.0);
                        let items = self.sticker_search_results.clone();
                        if self.sticker_search_loading {
                            ui.vertical_centered(|ui| {
                                ui.add_space(28.0);
                                ui.spinner();
                                ui.label("Đang tìm nhãn dán phù hợp…");
                            });
                        } else if items.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(28.0);
                                ui.label(
                                    RichText::new(
                                        "Chọn một từ khóa gợi ý bên trên hoặc nhập từ khóa để tìm nhãn dán Zalo.",
                                    )
                                    .size(12.5)
                                    .color(pal.text_muted),
                                );
                            });
                        } else {
                            self.render_sticker_items_grid(ui, &items, thread_id, is_group, pal);
                        }
                    }
                    StickerTab::Popular | StickerTab::Onion => {
                        let presets = if self.sticker_tab == StickerTab::Popular {
                            POPULAR_STICKERS
                        } else {
                            ONION_STICKERS
                        };
                        let items: Vec<crate::chat::StickerItem> = presets
                            .iter()
                            .map(|p| crate::chat::StickerItem {
                                sticker_id: p.sticker_id,
                                cate_id: p.cate_id,
                                sticker_type: p.sticker_type,
                                label: p.label.to_string(),
                            })
                            .collect();
                        self.render_sticker_items_grid(ui, &items, thread_id, is_group, pal);
                    }
                    StickerTab::Pack(cid) => {
                        if let Some(items) = self.sticker_pack_items.get(&cid).cloned() {
                            self.render_sticker_items_grid(ui, &items, thread_id, is_group, pal);
                        } else {
                            ui.vertical_centered(|ui| {
                                ui.add_space(36.0);
                                ui.spinner();
                                ui.label("Đang tải bộ nhãn dán từ máy chủ Zalo…");
                            });
                        }
                    }
                }
            });
    }

    fn render_sticker_items_grid(
        &mut self,
        ui: &mut egui::Ui,
        items: &[crate::chat::StickerItem],
        thread_id: &str,
        is_group: bool,
        pal: &ThemePalette,
    ) {
        egui::ScrollArea::vertical()
            .id_salt(("sticker_items_grid", self.sticker_tab_salt()))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for item in items {
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
                            let img_rect = cell_rect.shrink(4.0);
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
                                truncate_chars(&item.label, 10),
                                FontId::proportional(10.5),
                                pal.text_secondary,
                            );
                        }

                        if cell_resp
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .on_hover_text(format!(
                                "Gửi nhãn dán: {} (ID: {})",
                                item.label, item.sticker_id
                            ))
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
    }

    fn sticker_tab_salt(&self) -> u64 {
        match self.sticker_tab {
            StickerTab::Popular => 1,
            StickerTab::Onion => 2,
            StickerTab::Search => 3,
            StickerTab::Store => 4,
            StickerTab::Pack(cid) => 1000 + cid,
        }
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


#[derive(Debug, Clone)]
struct CallCardView {
    title: String,
    status_badge: String,
    reason: Option<String>,
    is_video: bool,
    is_missed_or_failed: bool,
}

/// Detect whether `msg` is a voice/video call log entry and parse its structured metadata.
fn extract_call_card_view(msg: &ChatMessage) -> Option<CallCardView> {
    let raw = msg.text.trim();
    let is_call_id = msg.id.starts_with("call_");
    let is_call_text = raw == "Cuộc gọi"
        || raw == "Cuộc gọi thoại"
        || raw == "Cuộc gọi video"
        || raw.starts_with("📞")
        || raw.starts_with("📹")
        || raw.starts_with("Cuộc gọi đi")
        || raw.starts_with("Cuộc gọi đến")
        || raw.starts_with("Cuộc gọi video");
    if !is_call_id && !is_call_text {
        return None;
    }

    let lower = raw.to_lowercase();
    let is_video = lower.contains("video") || raw.starts_with("📹");
    let is_incoming = lower.contains("gọi đến") || (!lower.contains("gọi đi") && !msg.is_outgoing);

    // Parse optional duration `(Ns)`
    let secs_opt: Option<u64> = raw.find('(').and_then(|open| {
        let rest = &raw[open + 1..];
        let close = rest.find("s)")?;
        rest[..close].trim().parse::<u64>().ok()
    });

    // Parse optional failure/rejection reason after `—` or `-`
    let raw_reason = raw
        .split_once(" — ")
        .or_else(|| raw.split_once(" - "))
        .map(|(_, r)| r.trim().to_string())
        .filter(|r| !r.is_empty());

    let is_missed_or_failed = match secs_opt {
        Some(0) => true,
        Some(_) => raw_reason.is_some(),
        None => raw_reason.is_some() || (!msg.is_outgoing && raw == "Cuộc gọi"),
    };

    let title = if is_video {
        if is_incoming {
            if is_missed_or_failed {
                "Cuộc gọi video nhỡ".to_string()
            } else {
                "Cuộc gọi video đến".to_string()
            }
        } else {
            "Cuộc gọi video đi".to_string()
        }
    } else if is_incoming {
        if is_missed_or_failed {
            "Cuộc gọi thoại nhỡ".to_string()
        } else {
            "Cuộc gọi thoại đến".to_string()
        }
    } else {
        "Cuộc gọi thoại đi".to_string()
    };

    let arrow = if is_incoming { "↙" } else { "↗" };
    let status_badge = match (is_missed_or_failed, secs_opt) {
        // The title already names the missed/failed state; show the reason instead of repeating it.
        (true, _) => {
            let reason_txt = raw_reason
                .as_deref()
                .filter(|r| !r.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    if is_incoming {
                        "Bạn đã bỏ lỡ cuộc gọi này".to_string()
                    } else {
                        "Người nhận không nghe máy".to_string()
                    }
                });
            format!("{arrow} {reason_txt}")
        }
        (false, Some(s)) if s >= 60 => {
            format!("{arrow} Thời lượng: {} phút {:02} giây", s / 60, s % 60)
        }
        (false, Some(s)) => format!("{arrow} Thời lượng: {s} giây"),
        (false, None) => format!("{arrow} Đã kết thúc"),
    };

    // When the call is missed/failed the badge already carries the reason, so only surface a
    // separate reason line for calls that otherwise look successful.
    let reason = if is_missed_or_failed {
        None
    } else {
        raw_reason.map(|r| {
            if r.contains("gián đoạn") || r.contains("từ chối") {
                "Người nhận từ chối hoặc kết nối gián đoạn".to_string()
            } else if r.contains("đang bận") {
                "Người nhận đang bận hoặc từ chối".to_string()
            } else {
                r
            }
        })
    };

    Some(CallCardView {
        title,
        status_badge,
        reason,
        is_video,
        is_missed_or_failed,
    })
}

fn render_call_card_widget(
    ui: &mut egui::Ui,
    card: &CallCardView,
    pal: &ThemePalette,
    card_width: f32,
) -> Option<BubbleAction> {
    let mut action = None;
    let accent = if card.is_missed_or_failed {
        Color32::from_rgb(239, 68, 68)
    } else {
        Color32::from_rgb(34, 197, 94)
    };

    ui.set_width(card_width);
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::hover());
            ui.painter().circle_filled(
                icon_rect.center(),
                20.0,
                accent.linear_multiply(if pal.is_dark { 0.18 } else { 0.14 }),
            );
            ui.painter().circle_stroke(
                icon_rect.center(),
                20.0,
                Stroke::new(1.2_f32, accent.linear_multiply(0.55)),
            );
            if card.is_video {
                draw_icon_video(ui.painter(), icon_rect.center(), 18.0, accent);
            } else {
                draw_icon_phone(ui.painter(), icon_rect.center(), 18.0, accent);
            }

            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(&card.title)
                            .size(13.8)
                            .color(pal.text_primary)
                            .strong(),
                    )
                    .wrap(),
                );
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(&card.status_badge)
                            .size(11.8)
                            .color(accent)
                            .strong(),
                    )
                    .wrap(),
                );
                if let Some(reason) = &card.reason {
                    ui.add_space(3.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(reason)
                                .size(11.2)
                                .color(pal.text_secondary),
                        )
                        .wrap(),
                    );
                }
            });
        });

        ui.add_space(8.0);
        let (sep_rect, _) =
            ui.allocate_exact_size(Vec2::new(card_width, 1.0), Sense::hover());
        ui.painter().line_segment(
            [sep_rect.left_center(), sep_rect.right_center()],
            Stroke::new(1.0_f32, pal.border),
        );
        ui.add_space(6.0);

        let btn_label = if card.is_video {
            format!("{}  Gọi video lại", lucide::VIDEO)
        } else {
            format!("{}  Gọi lại", lucide::PHONE)
        };
        let call_btn = egui::Button::new(
            RichText::new(btn_label)
                .size(12.5)
                .color(Color32::WHITE)
                .strong(),
        )
        .fill(pal.primary)
        .stroke(Stroke::new(1.0_f32, pal.primary_hover))
        .rounding(Rounding::same(7.0))
        .min_size(Vec2::new(card_width, 30.0));

        if ui
            .add(call_btn)
            .on_hover_cursor(CursorIcon::PointingHand)
            .clicked()
        {
            action = Some(BubbleAction::CallAgain {
                is_video: card.is_video,
            });
        }
    });
    action
}

/// Open an `http://` or `https://` URL in the user's default system browser.
fn open_external_url(url: &str) {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", trimmed])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::process::Command::new("xdg-open").arg(trimmed).spawn();
    }
}
/// Open a native OS file-picker dialog on a worker thread and return the selected path.
fn pick_native_file_dialog(images_only: bool) -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let filter = if images_only {
            "Image Files (*.png;*.jpg;*.jpeg;*.webp;*.gif;*.bmp)|*.png;*.jpg;*.jpeg;*.webp;*.gif;*.bmp|All Files (*.*)|*.*"
        } else {
            "All Files (*.*)|*.*|Video & Media (*.mp4;*.mov;*.mkv;*.mp3;*.wav)|*.mp4;*.mov;*.mkv;*.mp3;*.wav|Documents (*.pdf;*.zip;*.docx;*.xlsx;*.txt)|*.pdf;*.zip;*.docx;*.xlsx;*.txt"
        };
        let title = if images_only {
            "Chọn hình ảnh để gửi qua Zalo"
        } else {
            "Chọn video hoặc tập tin đính kèm để gửi qua Zalo"
        };
        let script = format!(
            "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
             Add-Type -AssemblyName System.Windows.Forms; \
             $f = New-Object System.Windows.Forms.OpenFileDialog; \
             $f.Title = '{title}'; \
             $f.Filter = '{filter}'; \
             $top = New-Object System.Windows.Forms.Form; \
             $top.TopMost = $true; \
             if ($f.ShowDialog($top) -eq [System.Windows.Forms.DialogResult]::OK) {{ Write-Output $f.FileName }}"
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-STA", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        let path_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if path_str.is_empty() {
            None
        } else {
            Some(std::path::PathBuf::from(path_str))
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = images_only;
        None
    }
}


/// Format a URL for clean UI display by stripping tracking query strings (`?_r=1&utm_...`)
/// and truncating to `max_chars`.
fn clean_display_url(url: &str, max_chars: usize) -> String {
    let trimmed = url.trim();
    let no_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let base = no_scheme.split('?').next().unwrap_or(no_scheme).trim_end_matches('/');
    truncate_chars(base, max_chars).into_owned()
}

/// Extract `(clean_caption, url, domain)` from a message when `msg.link_url` is set or when
/// `msg.text` contains an embedded `http://` / `https://` URL.
fn extract_message_link_view(msg: &ChatMessage) -> Option<(String, String, String)> {
    if let Some(url) = msg.link_url.as_deref().filter(|u| !u.is_empty()) {
        let mut caption = msg.link_desc.clone().unwrap_or_else(|| msg.text.clone());
        if let Some(pos) = caption.find("http://").or_else(|| caption.find("https://")) {
            let after = &caption[pos..];
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            let found = &after[..end];
            if found == url || found.starts_with(url.split('?').next().unwrap_or(url)) {
                caption = format!("{}{}", &caption[..pos], &after[end..]).trim().to_string();
            }
        }
        let domain = msg
            .link_src
            .clone()
            .or_else(|| {
                url.split("://")
                    .nth(1)
                    .and_then(|r| r.split('/').next())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "Liên kết".into());
        return Some((caption, url.to_string(), domain));
    }
    let text = msg.text.trim();
    let pos = text.find("https://").or_else(|| text.find("http://"))?;
    let after = &text[pos..];
    let end = after.find(char::is_whitespace).unwrap_or(after.len());
    let url = after[..end].trim_end_matches(|c: char| matches!(c, ')' | ']' | ',' | ';'));
    if url.len() < 10 {
        return None;
    }
    let caption = format!("{}{}", &text[..pos], &after[end..]).trim().to_string();
    let domain = url
        .split("://")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .unwrap_or("Liên kết")
        .to_string();
    Some((caption, url.to_string(), domain))
}

/// Convert days since 1970-01-01 into `(year, month, day)` (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = (y + if m <= 2 { 1 } else { 0 }) as i32;
    (year, m, d)
}

/// Normalize a millisecond or second timestamp to milliseconds (`> 0`).
fn normalize_ts_ms(ts: i64) -> i64 {
    if ts <= 0 {
        0
    } else if ts < 10_000_000_000 {
        ts * 1000
    } else {
        ts
    }
}

/// Return `true` when a date/time separator pill should be inserted before `curr`.
fn should_show_time_divider(prev: Option<&ChatMessage>, curr: &ChatMessage) -> bool {
    let Some(prev) = prev else {
        return true;
    };
    let prev_ms = normalize_ts_ms(prev.ts);
    let curr_ms = normalize_ts_ms(curr.ts);
    if prev_ms > 0 && curr_ms > 0 {
        let tz_offset_sec = 7 * 3600; // UTC+7 (Vietnam)
        let prev_day = (prev_ms / 1000 + tz_offset_sec) / 86_400;
        let curr_day = (curr_ms / 1000 + tz_offset_sec) / 86_400;
        if prev_day != curr_day {
            return true;
        }
        return (curr_ms - prev_ms).abs() >= 30 * 60 * 1000;
    }
    false
}

/// Format the label for a centered date/time separator pill.
fn format_time_divider_label(msg: &ChatMessage) -> String {
    let ts_ms = normalize_ts_ms(msg.ts);
    if ts_ms <= 0 {
        return if msg.time.is_empty() {
            "Cuộc trò chuyện".to_string()
        } else {
            format!("Hôm nay • {}", msg.time)
        };
    }
    let tz_offset_sec = 7 * 3600;
    let local_sec = ts_ms / 1000 + tz_offset_sec;
    let msg_day = local_sec.div_euclid(86_400);
    let sec_of_day = local_sec.rem_euclid(86_400);
    let hh = sec_of_day / 3600;
    let mm = (sec_of_day % 3600) / 60;

    let now_sec = (crate::login::unix_now() as i64) + tz_offset_sec;
    let today_day = now_sec.div_euclid(86_400);
    let (y, m, d) = civil_from_days(msg_day);
    let (ty, _, _) = civil_from_days(today_day);

    if msg_day == today_day {
        format!("Hôm nay • {hh:02}:{mm:02}")
    } else if msg_day == today_day - 1 {
        format!("Hôm qua • {hh:02}:{mm:02}")
    } else if y == ty {
        format!("{d:02}/{m:02} • {hh:02}:{mm:02}")
    } else {
        format!("{d:02}/{m:02}/{y} • {hh:02}:{mm:02}")
    }
}

/// Render a centered Zalo-style date/time divider pill with soft side hairlines.
fn render_date_divider(ui: &mut egui::Ui, msg: &ChatMessage, pal: &ThemePalette) {
    let label = format_time_divider_label(msg);
    let avail_w = ui.available_width();
    ui.horizontal(|ui| {
        let text_w = ui.fonts(|f| {
            f.layout_no_wrap(label.clone(), FontId::proportional(11.5), pal.text_muted)
                .size()
                .x
        });
        let pill_w = text_w + 24.0;
        let line_w = ((avail_w - pill_w - 24.0) * 0.5).max(0.0).min(90.0);
        let left_space = ((avail_w - (pill_w + line_w * 2.0 + 16.0)) * 0.5).max(0.0);
        if left_space > 0.0 {
            ui.add_space(left_space);
        }
        if line_w > 12.0 {
            let (lrect, _) = ui.allocate_exact_size(Vec2::new(line_w, 22.0), Sense::hover());
            ui.painter().line_segment(
                [lrect.left_center(), lrect.right_center()],
                Stroke::new(1.0_f32, pal.border.linear_multiply(0.65)),
            );
        }
        let pill_bg = if pal.is_dark {
            Color32::from_rgba_unmultiplied(30, 41, 59, 210)
        } else {
            Color32::from_rgba_unmultiplied(219, 228, 240, 215)
        };
        egui::Frame::none()
            .fill(pill_bg)
            .stroke(Stroke::new(1.0_f32, pal.border.linear_multiply(0.75)))
            .rounding(Rounding::same(11.0))
            .inner_margin(Margin::symmetric(11.0, 3.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(label)
                        .size(11.2)
                        .color(pal.text_secondary)
                        .strong(),
                );
            });
        if line_w > 12.0 {
            let (rrect, _) = ui.allocate_exact_size(Vec2::new(line_w, 22.0), Sense::hover());
            ui.painter().line_segment(
                [rrect.left_center(), rrect.right_center()],
                Stroke::new(1.0_f32, pal.border.linear_multiply(0.65)),
            );
        }
    });
}

/// Detect whether `msg` is a system/event notification rather than a user speech bubble.
fn is_system_notice_message(msg: &ChatMessage) -> bool {
    if msg.id.starts_with("sys_") {
        return true;
    }
    let t = msg.text.trim();
    t.starts_with("[Hệ thống]")
        || t.starts_with("[Thông báo]")
        || (t.starts_with("Bạn và ") && t.contains("đã trở thành bạn bè"))
        || t.contains("đã thêm bạn vào nhóm")
        || t.contains("đã tham gia nhóm")
        || t.contains("đã rời khỏi nhóm")
        || t.contains("đã đổi tên nhóm")
}

/// Render a centered system event pill (e.g., friend connected, group event).
fn render_system_notice_pill(ui: &mut egui::Ui, msg: &ChatMessage, pal: &ThemePalette) {
    let avail_w = ui.available_width();
    let text = msg.text.trim();
    ui.horizontal(|ui| {
        let text_w = ui.fonts(|f| {
            f.layout_no_wrap(text.to_string(), FontId::proportional(12.0), pal.text_secondary)
                .size()
                .x
        });
        let pill_w = (text_w + 28.0).min(avail_w - 40.0);
        let left_pad = ((avail_w - pill_w) * 0.5).max(0.0);
        if left_pad > 0.0 {
            ui.add_space(left_pad);
        }
        egui::Frame::none()
            .fill(pal.bg_elevated.linear_multiply(0.85))
            .stroke(Stroke::new(1.0_f32, pal.border))
            .rounding(Rounding::same(12.0))
            .inner_margin(Margin::symmetric(12.0, 4.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("{} {text}", lucide::SPARKLES))
                        .size(12.0)
                        .color(pal.text_secondary),
                );
            });
    });
}

/// Return `true` when two adjacent messages belong to the same visual bubble group.
fn same_message_cluster(a: &ChatMessage, b: &ChatMessage) -> bool {
    if is_system_notice_message(a) || is_system_notice_message(b) {
        return false;
    }
    if a.is_outgoing != b.is_outgoing || a.sender != b.sender {
        return false;
    }
    if should_show_time_divider(Some(a), b) {
        return false;
    }
    let a_ms = normalize_ts_ms(a.ts);
    let b_ms = normalize_ts_ms(b.ts);
    if a_ms > 0 && b_ms > 0 && (b_ms - a_ms).abs() > 10 * 60 * 1000 {
        return false;
    }
    true
}

fn render_message_body(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    thread_name: &str,
    pal: &ThemePalette,
    outgoing: bool,
    max_text_width: f32,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
) -> Option<BubbleAction> {
    let mut body_action = None;
    let (text_col, meta_col) = if outgoing {
        (pal.bubble_out_text, pal.bubble_out_meta)
    } else {
        (pal.bubble_in_text, pal.bubble_in_meta)
    };

    let trimmed_text = msg.text.trim();
    if trimmed_text == "[Tin nhắn đã thu hồi]" {
        ui.label(
            RichText::new(format!("{} Tin nhắn đã được thu hồi", lucide::BAN))
                .color(meta_col)
                .italics()
                .size(13.8),
        );
        return None;
    }

    let sender_label = if msg.is_outgoing {
        "Bạn".to_string()
    } else if !msg.sender.is_empty() && msg.sender != "0" {
        msg.sender.clone()
    } else {
        thread_name.to_string()
    };

    // Dedicated inline Photo Card when the message is an uploaded/received Zalo photo
    let is_zalo_photo = msg.link_src.as_deref() == Some("Hình ảnh Zalo")
        || trimmed_text == "[Hình ảnh]"
        || trimmed_text.starts_with("[Hình ảnh] ");
    if is_zalo_photo {
        if let Some(thumb_url) = msg
            .link_thumb
            .as_deref()
            .or(msg.link_url.as_deref())
            .filter(|u| !u.is_empty())
        {
            let key = format!("thumb_{}", crate::crypto::md5_hex(thumb_url));
            if !sticker_textures.contains_key(&key) && loading_stickers.insert(key.clone()) {
                if let Some(tx) = WORKER_TX.get() {
                    let _ = tx.send(WorkerCmd::FetchStickerImage {
                        key: key.clone(),
                        url: thumb_url.to_string(),
                    });
                }
            }
            let open_target = msg.link_url.as_deref().unwrap_or(thumb_url).to_string();
            let title_label = msg
                .link_title
                .as_deref()
                .filter(|t| !t.is_empty())
                .unwrap_or(trimmed_text)
                .to_string();
            if let Some(tex) = sticker_textures.get(&key) {
                let [tw, th] = tex.size();
                let tw_f = (tw.max(1)) as f32;
                let th_f = (th.max(1)) as f32;
                let max_w = max_text_width.clamp(160.0, 270.0);
                let max_h = 260.0_f32;
                let scale = (max_w / tw_f).min(max_h / th_f).min(1.0);
                let disp_w = (tw_f * scale).clamp(135.0, max_w);
                let disp_h = (th_f * scale).clamp(95.0, max_h);
                let img = egui::Image::new(tex)
                    .fit_to_exact_size(Vec2::new(disp_w, disp_h))
                    .rounding(Rounding::same(12.0))
                    .sense(Sense::click());
                let resp = ui
                    .add(img)
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("Bấm để phóng to hình ảnh");
                ui.painter().rect_stroke(
                    resp.rect,
                    Rounding::same(12.0),
                    Stroke::new(
                        if resp.hovered() { 1.6_f32 } else { 1.0_f32 },
                        if resp.hovered() {
                            pal.primary
                        } else {
                            pal.border
                        },
                    ),
                );
                if resp.hovered() {
                    ui.painter().rect_filled(
                        resp.rect,
                        Rounding::same(12.0),
                        Color32::from_rgba_unmultiplied(6, 10, 20, 88),
                    );
                    let pill_r = Rect::from_center_size(resp.rect.center(), Vec2::new(108.0, 26.0));
                    ui.painter().rect_filled(
                        pill_r,
                        Rounding::same(13.0),
                        Color32::from_rgba_unmultiplied(15, 23, 42, 225),
                    );
                    ui.painter().text(
                        pill_r.center(),
                        egui::Align2::CENTER_CENTER,
                        "Phóng to ảnh",
                        FontId::proportional(11.5),
                        Color32::WHITE,
                    );
                }
                if resp.clicked() {
                    body_action = Some(BubbleAction::OpenImagePopup {
                        tex_key: key,
                        full_url: open_target,
                        title: title_label,
                        sender: sender_label,
                        time: msg.time.clone(),
                    });
                }
            } else {
                let ph_frame = egui::Frame::none()
                    .fill(text_col.linear_multiply(0.08))
                    .rounding(Rounding::same(10.0))
                    .inner_margin(Margin::symmetric(12.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (ir, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                            draw_icon_image(ui.painter(), ir.center(), 18.0, text_col);
                            ui.label(
                                RichText::new(&title_label)
                                    .size(13.5)
                                    .color(text_col)
                                    .strong(),
                            );
                        });
                    });
                if ui
                    .interact(
                        ph_frame.response.rect,
                        Id::new(("photo_ph_click", &msg.id, msg.ts)),
                        Sense::click(),
                    )
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("Bấm để xem hình ảnh")
                    .clicked()
                {
                    body_action = Some(BubbleAction::OpenImagePopup {
                        tex_key: key,
                        full_url: open_target,
                        title: title_label,
                        sender: sender_label,
                        time: msg.time.clone(),
                    });
                }
            }
            if let Some(cap) = msg.link_desc.as_deref().filter(|c| !c.trim().is_empty()) {
                ui.add_space(5.0);
                ui.add(egui::Label::new(RichText::new(cap).color(text_col).size(14.5)).wrap());
            } else if !trimmed_text.starts_with("[Hình ảnh]") && !trimmed_text.is_empty() {
                ui.add_space(5.0);
                ui.add(
                    egui::Label::new(RichText::new(trimmed_text).color(text_col).size(14.5)).wrap(),
                );
            }
            return body_action;
        }
    }
    // Dedicated File / Video Attachment Card
    let is_zalo_file_or_video = matches!(
        msg.link_src.as_deref(),
        Some("Tập tin Zalo") | Some("Video Zalo")
    ) || trimmed_text.starts_with("[Tập tin]")
        || trimmed_text.starts_with("[Video]")
        || trimmed_text.starts_with("[Tin nhắn thoại]");
    if is_zalo_file_or_video {
        let is_vid = msg.link_src.as_deref() == Some("Video Zalo")
            || trimmed_text.starts_with("[Video]");
        let card_w = (max_text_width - 4.0).clamp(210.0, 300.0);
        let card_bg = if outgoing {
            if pal.is_dark {
                Color32::from_rgba_unmultiplied(0, 0, 0, 48)
            } else {
                Color32::from_rgba_unmultiplied(255, 255, 255, 175)
            }
        } else if pal.is_dark {
            pal.bg_app.linear_multiply(0.85)
        } else {
            Color32::from_rgb(241, 245, 249)
        };
        let card_border = if outgoing && pal.is_dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 42)
        } else {
            pal.border
        };
        let title_str = msg
            .link_title
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(trimmed_text);
        let sub_str = msg
            .link_src
            .as_deref()
            .unwrap_or(if is_vid { "Video đính kèm" } else { "Tập tin đính kèm" });

        egui::Frame::none()
            .fill(card_bg)
            .stroke(Stroke::new(1.0_f32, card_border))
            .rounding(Rounding::same(10.0))
            .inner_margin(Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                ui.set_width(card_w);
                ui.horizontal(|ui| {
                    let (badge_rect, _) =
                        ui.allocate_exact_size(Vec2::new(36.0, 36.0), Sense::hover());
                    let badge_col = if outgoing && pal.is_dark {
                        Color32::from_rgba_unmultiplied(255, 255, 255, 38)
                    } else {
                        pal.primary.linear_multiply(0.15)
                    };
                    ui.painter()
                        .rect_filled(badge_rect, Rounding::same(8.0), badge_col);
                    let icon_col = if outgoing && pal.is_dark {
                        Color32::WHITE
                    } else {
                        pal.primary
                    };
                    if is_vid {
                        draw_icon_video(ui.painter(), badge_rect.center(), 17.0, icon_col);
                    } else {
                        draw_icon_attachment(ui.painter(), badge_rect.center(), 17.0, icon_col);
                    }
                    ui.vertical(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(title_str).size(13.2).color(text_col).strong(),
                            )
                            .wrap(),
                        );
                        ui.label(RichText::new(sub_str).size(11.2).color(meta_col));
                    });
                    if let Some(url) = msg.link_url.as_deref().filter(|u| !u.is_empty()) {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let btn = egui::Button::new(
                                RichText::new("Mở ↗")
                                    .size(11.5)
                                    .color(icon_col)
                                    .strong(),
                            )
                            .fill(badge_col)
                            .rounding(Rounding::same(6.0))
                            .min_size(Vec2::new(52.0, 24.0));
                            if ui
                                .add(btn)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .on_hover_text(url)
                                .clicked()
                            {
                                open_external_url(url);
                            }
                        });
                    }
                });
            });
        if let Some(cap) = msg.link_desc.as_deref().filter(|c| !c.trim().is_empty()) {
            ui.add_space(5.0);
            ui.add(egui::Label::new(RichText::new(cap).color(text_col).size(14.5)).wrap());
        }
        return None;
    }

    let Some((caption, url, domain)) = extract_message_link_view(msg) else {
        ui.add(
            egui::Label::new(RichText::new(&msg.text).color(text_col).size(14.5)).wrap(),
        );
        return None;
    };

    if !caption.is_empty() && caption != url {
        ui.add(
            egui::Label::new(RichText::new(&caption).color(text_col).size(14.5)).wrap(),
        );
        ui.add_space(6.0);
    }

    let card_w = (max_text_width - 4.0).clamp(220.0, 320.0);
    let card_bg = if outgoing {
        if pal.is_dark {
            Color32::from_rgba_unmultiplied(0, 0, 0, 45)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, 185)
        }
    } else if pal.is_dark {
        pal.bg_elevated
    } else {
        Color32::from_rgb(244, 247, 252)
    };
    let card_border = if outgoing && pal.is_dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 42)
    } else {
        pal.border
    };
    let accent_col = if outgoing && pal.is_dark {
        Color32::from_rgb(147, 197, 253)
    } else {
        pal.primary
    };

    let card_frame = egui::Frame::none()
        .fill(card_bg)
        .stroke(Stroke::new(1.0_f32, card_border))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::symmetric(10.0, 8.0))
        .show(ui, |ui| {
            ui.set_width(card_w);
            ui.vertical(|ui| {
                // Optional link thumbnail preview (loaded asynchronously via `FetchStickerImage`)
                if let Some(thumb_url) = msg.link_thumb.as_deref().filter(|u| !u.is_empty()) {
                    let key = format!("thumb_{}", crate::crypto::md5_hex(thumb_url));
                    if !sticker_textures.contains_key(&key)
                        && (thumb_url.starts_with("http://") || thumb_url.starts_with("https://"))
                        && loading_stickers.insert(key.clone())
                    {
                        if let Some(tx) = WORKER_TX.get() {
                            let _ = tx.send(WorkerCmd::FetchStickerImage {
                                key: key.clone(),
                                url: thumb_url.to_string(),
                            });
                        }
                    }
                    if let Some(tex) = sticker_textures.get(&key) {
                        let [tw, th] = tex.size();
                        let aspect = if tw > 0 && th > 0 {
                            (th as f32 / tw as f32).clamp(0.45, 1.1)
                        } else {
                            0.56
                        };
                        let img_h = (card_w * aspect).min(175.0);
                        let img = egui::Image::new(tex)
                            .fit_to_exact_size(Vec2::new(card_w, img_h))
                            .rounding(Rounding::same(7.0))
                            .sense(Sense::click());
                        if ui
                            .add(img)
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .on_hover_text("Bấm để xem ảnh phóng to")
                            .clicked()
                        {
                            body_action = Some(BubbleAction::OpenImagePopup {
                                tex_key: key,
                                full_url: thumb_url.to_string(),
                                title: msg
                                    .link_title
                                    .clone()
                                    .unwrap_or_else(|| domain.clone()),
                                sender: sender_label.clone(),
                                time: msg.time.clone(),
                            });
                        }
                        ui.add_space(6.0);
                    }
                }

                // Source / Media Title badge with left accent bar
                let header_badge = msg
                    .link_title
                    .as_deref()
                    .filter(|t| !t.is_empty())
                    .unwrap_or(&domain);
                ui.horizontal(|ui| {
                    let (bar_rect, _) =
                        ui.allocate_exact_size(Vec2::new(3.0, 15.0), Sense::hover());
                    ui.painter()
                        .rect_filled(bar_rect, Rounding::same(1.5), accent_col);
                    ui.label(
                        RichText::new(format!("{} {header_badge}", lucide::GLOBE))
                            .size(12.2)
                            .color(accent_col)
                            .strong(),
                    );
                });

                ui.add_space(3.0);
                let short_url = clean_display_url(&url, 38);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{} {short_url}", lucide::LINK))
                            .size(11.5)
                            .color(meta_col),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let open_btn = egui::Button::new(
                            RichText::new("Mở ↗")
                                .size(11.5)
                                .color(if outgoing && pal.is_dark {
                                    Color32::WHITE
                                } else {
                                    pal.primary
                                })
                                .strong(),
                        )
                        .fill(if outgoing && pal.is_dark {
                            Color32::from_rgba_unmultiplied(255, 255, 255, 42)
                        } else {
                            pal.primary.linear_multiply(0.14)
                        })
                        .rounding(Rounding::same(6.0))
                        .min_size(Vec2::new(56.0, 24.0));
                        if ui
                            .add(open_btn)
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .on_hover_text(&url)
                            .clicked()
                        {
                            open_external_url(&url);
                        }
                    });
                });
            });
        });
    let _ = card_frame;
    body_action
}

fn render_chat_bubble(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    prev_same_sender: bool,
    next_same_sender: bool,
    is_latest_outgoing: bool,
    is_group: bool,
    thread_name: &str,
    sender_avatar: Option<&str>,
    pal: &ThemePalette,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
    emoji_textures: &HashMap<String, TextureHandle>,
    idx: usize,
) -> Option<BubbleAction> {
    let avail_w = ui.available_width();
    let max_bubble_width = (avail_w * 0.72).clamp(140.0, 520.0);
    let max_text_width = (max_bubble_width - 24.0).max(90.0);
    let has_sticker = is_sticker_message(msg).is_some();
    let call_card_opt = extract_call_card_view(msg);
    let is_call_card = call_card_opt.is_some();
    let is_photo_card = msg.link_src.as_deref() == Some("Hình ảnh Zalo")
        || msg.text.trim() == "[Hình ảnh]"
        || msg.text.trim().starts_with("[Hình ảnh] ");
    let is_file_card = matches!(
        msg.link_src.as_deref(),
        Some("Tập tin Zalo") | Some("Video Zalo")
    ) || msg.text.trim().starts_with("[Tập tin]")
        || msg.text.trim().starts_with("[Video]")
        || msg.text.trim().starts_with("[Tin nhắn thoại]");
    let has_link_card = extract_message_link_view(msg).is_some();
    let has_interactive_card = is_call_card || has_link_card || is_photo_card || is_file_card;

    let show_timestamp = !msg.time.is_empty()
        && (!next_same_sender
            || is_latest_outgoing
            || has_interactive_card
            || !msg.reactions.is_empty()
            || msg.is_pinned
            || msg.is_starred);

    if msg.is_outgoing {
        let has_photo_caption = is_photo_card
            && (msg
                .link_desc
                .as_deref()
                .map(|c| !c.trim().is_empty())
                .unwrap_or(false)
                || (!msg.text.trim().starts_with("[Hình ảnh]") && !msg.text.trim().is_empty()));
        let is_frameless_photo = is_photo_card && !has_photo_caption && msg.reply_to_text.is_none();

        // Pre-measure exact content width so outgoing bubbles and portrait photos hug their content
        // tightly (`w-fit`) without any excess blue background rectangle.
        let content_width = if has_sticker {
            116.0_f32
        } else if is_call_card {
            max_text_width.min(300.0)
        } else if is_photo_card {
            let thumb_u = msg
                .link_thumb
                .as_deref()
                .or(msg.link_url.as_deref())
                .unwrap_or("");
            let tkey = format!("thumb_{}", crate::crypto::md5_hex(thumb_u));
            if let Some(tex) = sticker_textures.get(&tkey) {
                let [tw, th] = tex.size();
                let tw_f = (tw.max(1)) as f32;
                let th_f = (th.max(1)) as f32;
                let max_w = max_text_width.clamp(160.0, 270.0);
                let max_h = 260.0_f32;
                let scale = (max_w / tw_f).min(max_h / th_f).min(1.0);
                (tw_f * scale).clamp(135.0, max_w).ceil()
            } else {
                220.0_f32
            }
        } else if is_file_card {
            max_text_width.min(300.0)
        } else if has_link_card {
            max_text_width.min(330.0)
        } else {
            let display_str = if msg.text.trim() == "[Tin nhắn đã thu hồi]" {
                "Tin nhắn đã được thu hồi"
            } else {
                msg.text.as_str()
            };
            let text_w = ui.fonts(|f| {
                f.layout(
                    display_str.to_string(),
                    FontId::proportional(14.2),
                    pal.bubble_out_text,
                    max_text_width,
                )
                .size()
                .x
            });
            let quote_w = match msg.reply_to_text.as_ref() {
                Some(qt) => {
                    ui.fonts(|f| {
                        f.layout(
                            truncate_chars(qt, 80).into_owned(),
                            FontId::proportional(12.0),
                            pal.bubble_out_meta,
                            max_text_width,
                        )
                        .size()
                        .x
                    }) + 26.0
                }
                None => 0.0_f32,
            };
            let marks_w = if !msg.reactions.is_empty() || msg.is_pinned || msg.is_starred {
                78.0_f32
            } else {
                0.0_f32
            };
            text_w
                .max(quote_w)
                .max(marks_w)
                .max(44.0)
                .min(max_text_width)
                .ceil()
                + 2.0
        };

        let h_margin = if has_sticker || is_frameless_photo {
            2.0_f32
        } else {
            13.0_f32
        };
        let v_margin = if has_sticker || is_frameless_photo {
            2.0_f32
        } else {
            8.5_f32
        };
        let total_bubble_w = content_width + h_margin * 2.0;

        let out_resp = ui.vertical(|ui| {
            let row_resp = ui.horizontal(|ui| {
                let left_pad = (ui.available_width() - total_bubble_w - 6.0).max(0.0);
                if left_pad > 0.0 {
                    ui.add_space(left_pad);
                }

                let fill = if has_sticker || is_frameless_photo {
                    Color32::TRANSPARENT
                } else if is_call_card {
                    pal.bg_elevated
                } else {
                    pal.bubble_out_bg
                };
                let stroke = if has_sticker || is_frameless_photo {
                    Stroke::NONE
                } else if let Some(card) = &call_card_opt {
                    let border_col = if card.is_missed_or_failed {
                        Color32::from_rgb(239, 68, 68).linear_multiply(0.45)
                    } else {
                        Color32::from_rgb(34, 197, 94).linear_multiply(0.45)
                    };
                    Stroke::new(1.2_f32, border_col)
                } else if !pal.is_dark {
                    Stroke::new(1.0_f32, Color32::from_rgb(194, 218, 255))
                } else {
                    Stroke::new(1.0_f32, Color32::from_rgb(30, 112, 238))
                };
                let rounding = Rounding {
                    nw: 15.0,
                    ne: if prev_same_sender { 5.0 } else { 15.0 },
                    sw: 15.0,
                    se: if next_same_sender { 5.0 } else { 4.0 },
                };

                let mut card_action = None;
                let fr = egui::Frame::none()
                    .fill(fill)
                    .stroke(stroke)
                    .rounding(rounding)
                    .inner_margin(Margin::symmetric(h_margin, v_margin))
                    .show(ui, |ui| {
                        ui.set_width(content_width);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            render_quote_block(ui, msg, pal, true);
                            if let Some(card) = &call_card_opt {
                                card_action =
                                    render_call_card_widget(ui, card, pal, content_width);
                            } else if !render_sticker_widget(
                                ui,
                                msg,
                                sticker_textures,
                                loading_stickers,
                            ) {
                                card_action = render_message_body(
                                    ui,
                                    msg,
                                    thread_name,
                                    pal,
                                    true,
                                    content_width,
                                    sticker_textures,
                                    loading_stickers,
                                );
                            }
                            render_reactions(ui, msg, pal, emoji_textures);
                        });
                    });
                card_action.or_else(|| {
                    bubble_context_menu(
                        ui,
                        &fr.response,
                        msg,
                        idx,
                        pal,
                        emoji_textures,
                        has_interactive_card,
                    )
                })
            });

            // Crisp external meta row below outgoing bubble with vector double-check icon (`✓✓`)
            if show_timestamp {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(8.0);
                    let (chk_rect, _) =
                        ui.allocate_exact_size(Vec2::new(14.0, 12.0), Sense::hover());
                    draw_icon_check_double(
                        ui.painter(),
                        chk_rect.center(),
                        12.5,
                        if is_latest_outgoing {
                            pal.accent_online
                        } else {
                            Color32::from_rgb(96, 165, 250)
                        },
                    );
                    let status_txt = if is_latest_outgoing {
                        format!("{} · Đã nhận", msg.time)
                    } else {
                        msg.time.clone()
                    };
                    ui.label(
                        RichText::new(status_txt)
                            .size(10.8)
                            .color(pal.text_muted),
                    );
                });
            }

            row_resp.inner
        });
        out_resp.inner
    } else {
        ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
            let display_sender = if !msg.sender.is_empty()
                && msg.sender != "0"
                && !msg.sender.chars().all(|c| c.is_ascii_digit())
            {
                &msg.sender
            } else {
                thread_name
            };
            let sender_accent = avatar_color_for(
                if !msg.sender.is_empty() && !msg.sender.chars().all(|c| c.is_ascii_digit()) {
                    &msg.sender
                } else {
                    thread_name
                },
                false,
            );

            // Sender Avatar (only on first message of a consecutive group)
            if !prev_same_sender {
                let (av_rect, _) =
                    ui.allocate_exact_size(Vec2::new(30.0, 30.0), Sense::hover());
                paint_user_avatar(
                    ui.painter(),
                    av_rect.center(),
                    15.0,
                    display_sender,
                    false,
                    sender_avatar,
                    sticker_textures,
                    loading_stickers,
                );
            } else {
                ui.add_space(30.0);
            }

            let has_in_photo_caption = is_photo_card
                && (msg
                    .link_desc
                    .as_deref()
                    .map(|c| !c.trim().is_empty())
                    .unwrap_or(false)
                    || (!msg.text.trim().starts_with("[Hình ảnh]")
                        && !msg.text.trim().is_empty()));
            let is_frameless_in_photo =
                is_photo_card && !has_in_photo_caption && msg.reply_to_text.is_none() && !is_group;

            let fill = if has_sticker || is_frameless_in_photo {
                Color32::TRANSPARENT
            } else if is_call_card {
                pal.bg_elevated
            } else {
                pal.bubble_in_bg
            };
            let stroke = if has_sticker || is_frameless_in_photo {
                Stroke::NONE
            } else if let Some(card) = &call_card_opt {
                let border_col = if card.is_missed_or_failed {
                    Color32::from_rgb(239, 68, 68).linear_multiply(0.45)
                } else {
                    Color32::from_rgb(34, 197, 94).linear_multiply(0.45)
                };
                Stroke::new(1.2_f32, border_col)
            } else {
                Stroke::new(1.0_f32, pal.bubble_in_border)
            };
            let rounding = Rounding {
                nw: if prev_same_sender { 5.0 } else { 4.0 },
                ne: 15.0,
                sw: if next_same_sender { 5.0 } else { 15.0 },
                se: 15.0,
            };

            let mut card_action = None;
            let fr = egui::Frame::none()
                .fill(fill)
                .stroke(stroke)
                .rounding(rounding)
                .inner_margin(if has_sticker || is_frameless_in_photo {
                    Margin::same(2.0)
                } else {
                    Margin::symmetric(13.0, 8.5)
                })
                .show(ui, |ui| {
                    ui.set_max_width(max_bubble_width);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        // Only show a sender label when it is a real name (never a raw numeric UID)
                        let sender_is_name = !msg.sender.is_empty()
                            && msg.sender != "0"
                            && !msg.sender.chars().all(|c| c.is_ascii_digit());
                        if !prev_same_sender && is_group && sender_is_name {
                            ui.label(
                                RichText::new(truncate_chars(&msg.sender, 22))
                                    .color(sender_accent)
                                    .size(12.0)
                                    .strong(),
                            );
                            ui.add_space(2.0);
                        }
                        render_quote_block(ui, msg, pal, false);
                        if let Some(card) = &call_card_opt {
                            card_action = render_call_card_widget(
                                ui,
                                card,
                                pal,
                                max_text_width.min(300.0),
                            );
                        } else if !render_sticker_widget(ui, msg, sticker_textures, loading_stickers)
                        {
                            card_action = render_message_body(
                                ui,
                                msg,
                                thread_name,
                                pal,
                                false,
                                max_text_width,
                                sticker_textures,
                                loading_stickers,
                            );
                        }
                        render_reactions(ui, msg, pal, emoji_textures);
                        if show_timestamp {
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(&msg.time)
                                    .color(pal.bubble_in_meta)
                                    .size(10.2),
                            );
                        }
                    });
                });
            card_action.or_else(|| {
                bubble_context_menu(
                    ui,
                    &fr.response,
                    msg,
                    idx,
                    pal,
                    emoji_textures,
                    has_interactive_card,
                )
            })
        })
        .inner
    }
}

#[derive(Clone, PartialEq)]
enum BubbleAction {
    Reply,
    Forward,
    React(&'static str),
    TogglePin,
    ToggleStar,
    DeleteForMe,
    CallAgain { is_video: bool },
    OpenImagePopup {
        tex_key: String,
        full_url: String,
        title: String,
        sender: String,
        time: String,
    },
}

pub const REACTION_LIST: [(&str, &str, &str); 6] = [
    ("👍", "like", "Thích"),
    ("❤️", "heart", "Yêu thích"),
    ("😆", "smile", "Haha"),
    ("😮", "surprised", "Wow"),
    ("😢", "cry", "Buồn"),
    ("😠", "rage", "Phẫn nộ"),
];

pub fn emoji_to_tex_key(emoji: &str) -> &'static str {
    match emoji {
        "👍" | "/-strong" => "like",
        "❤️" | "❤" | "/-heart" => "heart",
        "😆" | ":>" => "smile",
        "😮" | ":o" => "surprised",
        "😢" | ":-(( " | ":-((" => "cry",
        "😠" | ":-h" => "rage",
        _ => "like",
    }
}

fn load_default_emoji_textures(ctx: &egui::Context) -> HashMap<String, TextureHandle> {
    let raw: [(&str, &[u8]); 6] = [
        ("like", include_bytes!("assets/emojis/like.png")),
        ("heart", include_bytes!("assets/emojis/heart.png")),
        ("smile", include_bytes!("assets/emojis/smile.png")),
        ("surprised", include_bytes!("assets/emojis/surprised.png")),
        ("cry", include_bytes!("assets/emojis/cry.png")),
        ("rage", include_bytes!("assets/emojis/rage.png")),
    ];
    let mut map = HashMap::new();
    for (name, bytes) in raw {
        if let Ok(img) = image::load_from_memory(bytes) {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let color_img = egui::ColorImage::from_rgba_unmultiplied(
                [w as usize, h as usize],
                rgba.as_raw(),
            );
            let tex = ctx.load_texture(
                format!("zalo_emoji_{name}"),
                color_img,
                TextureOptions::LINEAR,
            );
            map.insert(name.to_string(), tex);
        }
    }
    map
}

/// Right-click menu on a chat bubble: reactions, reply, forward, copy, delete-for-me.
fn bubble_context_menu(
    ui: &mut egui::Ui,
    bubble: &egui::Response,
    msg: &ChatMessage,
    idx: usize,
    _pal: &ThemePalette,
    emoji_textures: &HashMap<String, TextureHandle>,
    _has_interactive_card: bool,
) -> Option<BubbleAction> {
    let secondary_or_popup = ui.input(|i| {
        i.pointer.button_down(egui::PointerButton::Secondary)
            || i.pointer.button_clicked(egui::PointerButton::Secondary)
            || i.pointer.button_released(egui::PointerButton::Secondary)
    }) || ui.memory(|m| m.any_popup_open());
    if !secondary_or_popup {
        return None;
    }
    let resp = ui.interact(
        bubble.rect,
        Id::new(("bubble_ctx", idx, msg.ts, &msg.id)),
        Sense::click(),
    );
    let mut action = None;

    resp.context_menu(|ui| {
        ui.set_min_width(210.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);
            for &(code, key, tooltip) in &REACTION_LIST {
                if let Some(tex) = emoji_textures.get(key) {
                    let img = egui::Image::new(tex).fit_to_exact_size(Vec2::splat(24.0));
                    let btn = egui::ImageButton::new(img)
                        .rounding(Rounding::same(6.0));
                    if ui.add(btn).on_hover_text(tooltip).clicked() {
                        action = Some(BubbleAction::React(code));
                        ui.close_menu();
                    }
                } else if ui.button(RichText::new(code).size(18.0)).clicked() {
                    action = Some(BubbleAction::React(code));
                    ui.close_menu();
                }
            }
        });
        ui.separator();
        if let Some(thumb_url) = msg
            .link_thumb
            .as_deref()
            .or_else(|| {
                if msg.link_src.as_deref() == Some("Hình ảnh Zalo") {
                    msg.link_url.as_deref()
                } else {
                    None
                }
            })
            .filter(|u| !u.is_empty())
        {
            if ui.button(format!("{} Xem ảnh phóng to", lucide::SEARCH)).clicked() {
                action = Some(BubbleAction::OpenImagePopup {
                    tex_key: format!("thumb_{}", crate::crypto::md5_hex(thumb_url)),
                    full_url: msg
                        .link_url
                        .clone()
                        .unwrap_or_else(|| thumb_url.to_string()),
                    title: msg
                        .link_title
                        .clone()
                        .unwrap_or_else(|| "Hình ảnh".to_string()),
                    sender: if msg.is_outgoing {
                        "Bạn".to_string()
                    } else {
                        msg.sender.clone()
                    },
                    time: msg.time.clone(),
                });
                ui.close_menu();
            }
        }
        if ui.button("Trả lời").clicked() {
            action = Some(BubbleAction::Reply);
            ui.close_menu();
        }
        if ui.button("Chuyển tiếp").clicked() {
            action = Some(BubbleAction::Forward);
            ui.close_menu();
        }
        if ui.button("Copy tin nhắn").clicked() {
            ui.ctx().copy_text(msg.text.clone());
            ui.close_menu();
        }
        if ui
            .button(if msg.is_pinned { "Bỏ ghim tin nhắn" } else { "Ghim tin nhắn" })
            .clicked()
        {
            action = Some(BubbleAction::TogglePin);
            ui.close_menu();
        }
        if ui
            .button(if msg.is_starred {
                "Bỏ đánh dấu tin nhắn"
            } else {
                "Đánh dấu tin nhắn"
            })
            .clicked()
        {
            action = Some(BubbleAction::ToggleStar);
            ui.close_menu();
        }
        ui.separator();
        if ui
            .button(RichText::new("Xóa chỉ ở phía tôi").color(Color32::from_rgb(220, 60, 60)))
            .clicked()
        {
            action = Some(BubbleAction::DeleteForMe);
            ui.close_menu();
        }
    });
    action
}

/// Quoted-message block shown at the top of a reply bubble.
fn render_quote_block(ui: &mut egui::Ui, msg: &ChatMessage, pal: &ThemePalette, outgoing: bool) {
    let Some(qt) = msg.reply_to_text.as_ref() else {
        return;
    };
    let (text_col, meta_col) = if outgoing {
        (pal.bubble_out_text, pal.bubble_out_meta)
    } else {
        (pal.bubble_in_text, pal.bubble_in_meta)
    };
    let bar_col = if outgoing && pal.is_dark {
        Color32::from_rgb(147, 197, 253)
    } else {
        pal.primary
    };
    let bg_col = if outgoing && pal.is_dark {
        Color32::from_rgba_unmultiplied(0, 0, 0, 42)
    } else {
        text_col.linear_multiply(0.07)
    };
    egui::Frame::none()
        .fill(bg_col)
        .rounding(Rounding::same(7.0))
        .inner_margin(Margin::symmetric(8.0, 5.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (bar_rect, _) = ui.allocate_exact_size(Vec2::new(3.0, 28.0), Sense::hover());
                ui.painter()
                    .rect_filled(bar_rect, Rounding::same(1.5), bar_col);
                ui.vertical(|ui| {
                    let who = msg
                        .reply_to_sender
                        .as_deref()
                        .filter(|sn| !sn.is_empty() && !sn.chars().all(|c| c.is_ascii_digit()))
                        .unwrap_or("Tin nhắn trích dẫn");
                    ui.label(RichText::new(who).color(bar_col).size(11.5).strong());
                    ui.label(
                        RichText::new(truncate_chars(qt, 84))
                            .color(meta_col)
                            .size(12.0),
                    );
                });
            });
        });
    ui.add_space(5.0);
}

fn render_reactions(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    pal: &ThemePalette,
    emoji_textures: &HashMap<String, TextureHandle>,
) {
    let has_pin = msg.is_pinned;
    let has_star = msg.is_starred;
    let has_react = !msg.reactions.is_empty();

    if !has_pin && !has_star && !has_react {
        return;
    }

    ui.add_space(3.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);

        if has_pin {
            egui::Frame::none()
                .fill(pal.primary.linear_multiply(0.18))
                .stroke(Stroke::new(1.0_f32, pal.primary))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::symmetric(5.0, 2.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("{} Đã ghim", lucide::PIN))
                            .size(11.0)
                            .color(pal.primary)
                            .strong(),
                    );
                });
        }

        if has_star {
            egui::Frame::none()
                .fill(Color32::from_rgb(234, 179, 8).linear_multiply(0.2))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(234, 179, 8)))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::symmetric(5.0, 2.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("{} Đã lưu", lucide::STAR))
                            .size(11.0)
                            .color(Color32::from_rgb(234, 179, 8))
                            .strong(),
                    );
                });
        }

        if has_react {
            egui::Frame::none()
                .fill(pal.bg_elevated)
                .stroke(Stroke::new(1.0_f32, pal.border))
                .rounding(Rounding::same(12.0))
                .inner_margin(Margin::symmetric(6.0, 2.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);
                        for r in &msg.reactions {
                            let key = emoji_to_tex_key(r);
                            if let Some(tex) = emoji_textures.get(key) {
                                ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::splat(18.0)));
                            } else {
                                ui.label(RichText::new(r).size(13.0));
                            }
                        }
                    });
                });
        }
    });
}

fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 1. Embed official Lucide Icon Font (vector glyphs for all UI icons)
    const LUCIDE_BYTES: &[u8] = include_bytes!("assets/fonts/lucide.ttf");
    fonts.font_data.insert(
        "lucide_font".to_owned(),
        egui::FontData::from_static(LUCIDE_BYTES),
    );

    // 2. Load system Vietnamese sans-serif font
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
            break;
        }
    }

    // Fallback order: Proportional text prefers Vietnamese sans, then Lucide for icon glyphs
    let prop = fonts.families.entry(egui::FontFamily::Proportional).or_default();
    if fonts.font_data.contains_key("vietnamese_font") {
        prop.insert(0, "vietnamese_font".to_owned());
        prop.push("lucide_font".to_owned());
    } else {
        prop.insert(0, "lucide_font".to_owned());
    }

    let mono = fonts.families.entry(egui::FontFamily::Monospace).or_default();
    if fonts.font_data.contains_key("vietnamese_font") {
        mono.push("vietnamese_font".to_owned());
    }
    mono.push("lucide_font".to_owned());

    ctx.set_fonts(fonts);
}
