use super::*;

// ---------------------------------------------------------------------------
// Semantic Design System Tokens (UI/UX Pro Max — Minimalism & Swiss Style)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(super) struct ThemePalette {
    pub(super) is_dark: bool,
    pub(super) bg_app: Color32,
    pub(super) bg_sidebar: Color32,
    pub(super) bg_chat_canvas: Color32,
    pub(super) bg_elevated: Color32,
    pub(super) bg_hover: Color32,
    pub(super) bg_selected: Color32,
    pub(super) bg_input: Color32,
    pub(super) border: Color32,
    pub(super) text_primary: Color32,
    pub(super) text_secondary: Color32,
    pub(super) text_muted: Color32,
    pub(super) primary: Color32,
    pub(super) primary_hover: Color32,
    pub(super) accent_online: Color32,
    pub(super) danger: Color32,
    pub(super) warning: Color32,
    pub(super) bubble_out_bg: Color32,
    pub(super) bubble_out_text: Color32,
    pub(super) bubble_out_meta: Color32,
    pub(super) bubble_in_bg: Color32,
    pub(super) bubble_in_border: Color32,
    pub(super) bubble_in_text: Color32,
    pub(super) bubble_in_meta: Color32,
    /// Left navigation sidebar surface.
    pub(super) bg_nav: Color32,
    /// Conversation list surface.
    pub(super) bg_list: Color32,
    /// Active navigation item fill.
    pub(super) nav_active: Color32,
    /// Chat canvas gradient (top-left → bottom-right).
    pub(super) canvas_top: Color32,
    pub(super) canvas_bottom: Color32,
    /// Unread badge fill.
    pub(super) badge: Color32,
    /// Date chip / reaction chip surface.
    pub(super) chip_bg: Color32,
    /// Composer input surface.
    pub(super) composer_bg: Color32,
    /// Soft drop shadow tint.
    pub(super) shadow: Color32,
}

impl ThemePalette {
    pub(super) fn dark() -> Self {
        // Official Atom One Dark (`one-dark-ui` + `one-dark-syntax`) color system
        Self {
            is_dark: true,
            bg_app: Color32::from_rgb(24, 26, 31),          // #181A1F Activity Bar & Title Bar
            bg_sidebar: Color32::from_rgb(33, 37, 43),      // #21252B Tree-View / Sidebar / Header
            bg_chat_canvas: Color32::from_rgb(40, 44, 52),  // #282C34 Atom One Dark Editor Canvas
            bg_elevated: Color32::from_rgb(44, 49, 58),     // #2C313A Elevated Card / Modal / Popover
            bg_hover: Color32::from_rgb(50, 56, 66),        // #323842 List Hover Surface
            bg_selected: Color32::from_rgb(58, 63, 75),     // #3A3F4B Active Selection Row
            bg_input: Color32::from_rgb(29, 31, 35),        // #1D1F23 Input / Search Well
            border: Color32::from_rgb(52, 57, 66),          // #343942 Subtle Panel Divider
            text_primary: Color32::from_rgb(215, 218, 224), // #D7DAE0 Bright Foreground
            text_secondary: Color32::from_rgb(171, 178, 191), // #ABB2BF Atom @mono-1
            text_muted: Color32::from_rgb(118, 126, 142),   // #767E8E Atom @mono-3 (readable muted)
            primary: Color32::from_rgb(82, 139, 255),       // #528BFF Atom UI Accent Blue
            primary_hover: Color32::from_rgb(97, 175, 239), // #61AFEF Atom @hue-2 Cyan-Blue
            accent_online: Color32::from_rgb(152, 195, 121), // #98C379 Atom @hue-4 Green
            danger: Color32::from_rgb(224, 108, 117),       // #E06C75 Atom @hue-5 Coral Red
            warning: Color32::from_rgb(229, 192, 123),      // #E5C07B Atom @hue-62 Warm Gold
            bubble_out_bg: Color32::from_rgb(46, 110, 230), // solid blue outgoing bubble
            bubble_out_text: Color32::from_rgb(255, 255, 255),
            bubble_out_meta: Color32::from_rgba_unmultiplied(255, 255, 255, 190),
            bubble_in_bg: Color32::from_rgb(50, 56, 66),    // #323842 Atom Elevated Incoming Bubble
            bubble_in_border: Color32::from_rgb(62, 68, 81), // #3E4451 Atom Gutter Border
            bubble_in_text: Color32::from_rgb(215, 218, 224), // #D7DAE0
            bubble_in_meta: Color32::from_rgb(142, 150, 165), // #8E96A5
            bg_nav: Color32::from_rgb(28, 31, 37),
            bg_list: Color32::from_rgb(33, 37, 43),
            nav_active: Color32::from_rgb(45, 62, 92),
            canvas_top: Color32::from_rgb(40, 44, 52),
            canvas_bottom: Color32::from_rgb(31, 38, 50),
            badge: Color32::from_rgb(64, 132, 255),
            chip_bg: Color32::from_rgb(50, 56, 66),
            composer_bg: Color32::from_rgb(44, 49, 58),
            shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 60),
        }
    }

    pub(super) fn light() -> Self {
        // Airy light theme: frosted blue-gray rails, soft blue selection, solid blue outgoing bubbles.
        Self {
            is_dark: false,
            bg_app: Color32::from_rgb(243, 247, 252),       // title bar blends with the nav rail
            bg_sidebar: Color32::from_rgb(248, 251, 255),   // headers / drawers
            bg_chat_canvas: Color32::from_rgb(244, 249, 254),
            bg_elevated: Color32::from_rgb(255, 255, 255),
            bg_hover: Color32::from_rgb(232, 240, 250),
            bg_selected: Color32::from_rgb(207, 227, 253),  // selected conversation row
            bg_input: Color32::from_rgb(255, 255, 255),
            border: Color32::from_rgb(224, 232, 242),
            text_primary: Color32::from_rgb(24, 33, 48),
            text_secondary: Color32::from_rgb(84, 96, 114),
            text_muted: Color32::from_rgb(128, 140, 158),
            primary: Color32::from_rgb(10, 110, 245),        // #0A6EF5
            primary_hover: Color32::from_rgb(8, 94, 214),
            accent_online: Color32::from_rgb(22, 163, 74),
            danger: Color32::from_rgb(220, 38, 38),
            warning: Color32::from_rgb(217, 119, 6),
            bubble_out_bg: Color32::from_rgb(10, 110, 245),
            bubble_out_text: Color32::WHITE,
            bubble_out_meta: Color32::from_rgba_unmultiplied(255, 255, 255, 200),
            bubble_in_bg: Color32::from_rgb(242, 244, 247),
            bubble_in_border: Color32::from_rgb(232, 236, 242),
            bubble_in_text: Color32::from_rgb(24, 33, 48),
            bubble_in_meta: Color32::from_rgb(128, 140, 158),
            bg_nav: Color32::from_rgb(243, 247, 252),
            bg_list: Color32::from_rgb(249, 251, 254),
            nav_active: Color32::from_rgb(214, 230, 251),
            canvas_top: Color32::from_rgb(252, 253, 255),
            canvas_bottom: Color32::from_rgb(225, 240, 250),
            badge: Color32::from_rgb(10, 110, 245),
            chip_bg: Color32::from_rgb(255, 255, 255),
            composer_bg: Color32::from_rgb(255, 255, 255),
            shadow: Color32::from_rgba_unmultiplied(30, 60, 110, 22),
        }
    }

    pub(super) fn apply_to_ctx(&self, ctx: &egui::Context) {
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
        // Thin floating scroll bars (appear on hover) keep the airy look of the reference design.
        style.spacing.scroll = egui::style::ScrollStyle::floating();
        style.spacing.scroll.bar_width = 6.0;
        style.spacing.scroll.floating_allocated_width = 0.0;
        style.visuals = visuals;
        ctx.set_style(style);
    }
}

// ---------------------------------------------------------------------------
// Zero-allocation Vector Icon Painter Primitives (Replaces Emoji Icons)
// ---------------------------------------------------------------------------

pub(super) fn avatar_color_for(name: &str, is_group: bool) -> Color32 {
    const PALETTE: &[Color32] = &[
        Color32::from_rgb(24, 132, 236),  // blue
        Color32::from_rgb(240, 129, 78),  // orange
        Color32::from_rgb(123, 97, 255),  // violet
        Color32::from_rgb(28, 176, 125),  // green
        Color32::from_rgb(232, 121, 168), // pink
        Color32::from_rgb(20, 158, 196),  // cyan
        Color32::from_rgb(234, 88, 92),   // coral
        Color32::from_rgb(92, 120, 220),  // indigo
    ];
    let mut hash: usize = if is_group { 3 } else { 0 };
    for b in name.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(b as usize);
    }
    PALETTE[hash % PALETTE.len()]
}

pub(super) fn truncate_chars<'a>(s: &'a str, max_chars: usize) -> std::borrow::Cow<'a, str> {
    if let Some((byte_idx, _)) = s.char_indices().nth(max_chars) {
        std::borrow::Cow::Owned(format!("{}…", &s[..byte_idx]))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// Extract up to 2 uppercase initials from a display name (e.g. `"Nguyễn An"` -> `"NA"`, `"Hoang"` -> `"H"`).
///
/// Group names drop a leading "Nhóm" and use their first two words
/// (`"Nhóm Thiết kế"` -> `"TK"`, `"Đội bóng công ty"` -> `"ĐB"`).
pub(super) fn avatar_initials_for(name: &str, is_group: bool) -> String {
    if is_group {
        let mut words: Vec<&str> = name.split_whitespace().collect();
        if words.len() >= 2 && words[0].to_lowercase() == "nhóm" {
            words.remove(0);
        }
        let initials: String = words
            .iter()
            .take(2)
            .filter_map(|w| w.chars().next())
            .flat_map(|c| c.to_uppercase())
            .collect();
        if !initials.is_empty() {
            return initials;
        }
    }
    avatar_initials(name)
}

pub(super) fn avatar_initials(name: &str) -> String {
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

/// Fold Vietnamese text to lowercase ASCII for diacritic-insensitive search
/// (e.g. `"Nguyễn Văn An"` -> `"nguyen van an"`).
pub(super) fn fold_vn_search(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for raw in s.chars() {
        for c in raw.to_lowercase() {
            let mapped = match c {
                'à' | 'á' | 'ạ' | 'ả' | 'ã' | 'â' | 'ầ' | 'ấ' | 'ậ' | 'ẩ' | 'ẫ' | 'ă' | 'ằ'
                | 'ắ' | 'ặ' | 'ẳ' | 'ẵ' => 'a',
                'è' | 'é' | 'ẹ' | 'ẻ' | 'ẽ' | 'ê' | 'ề' | 'ế' | 'ệ' | 'ể' | 'ễ' => 'e',
                'ì' | 'í' | 'ị' | 'ỉ' | 'ĩ' => 'i',
                'ò' | 'ó' | 'ọ' | 'ỏ' | 'õ' | 'ô' | 'ồ' | 'ố' | 'ộ' | 'ổ' | 'ỗ' | 'ơ' | 'ờ'
                | 'ớ' | 'ợ' | 'ở' | 'ỡ' => 'o',
                'ù' | 'ú' | 'ụ' | 'ủ' | 'ũ' | 'ư' | 'ừ' | 'ứ' | 'ự' | 'ử' | 'ữ' => 'u',
                'ỳ' | 'ý' | 'ỵ' | 'ỷ' | 'ỹ' => 'y',
                'đ' => 'd',
                other => other,
            };
            out.push(mapped);
        }
    }
    out
}

/// Check whether `haystack` matches `query_lower` (or `query_folded` without Vietnamese accents).
pub(super) fn matches_vn_query(haystack: &str, query_lower: &str, query_folded: &str) -> bool {
    if query_lower.is_empty() {
        return true;
    }
    if haystack.to_lowercase().contains(query_lower) {
        return true;
    }
    !query_folded.is_empty() && fold_vn_search(haystack).contains(query_folded)
}

/// Paint a circular user or group avatar: uses the real circular-masked Zalo profile image when
/// `avatar_url` is available (fetching it asynchronously via `FetchStickerImage`), and falls back
/// to a colored circle with initials while loading or when no URL exists.
pub(super) fn paint_user_avatar(
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
    let initial = if radius >= 14.0 {
        avatar_initials_for(name, is_group)
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
        FontId::proportional((radius * 0.72).max(10.5)),
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
    pub const LOCK: &str = "\u{e10b}";
    pub const BELL: &str = "\u{e059}";
    pub const PAINTBRUSH: &str = "\u{e2e7}";
    pub const SLIDERS: &str = "\u{e29a}";
    pub const FOLDER_OPEN: &str = "\u{e247}";
    pub const CHEVRON_RIGHT: &str = "\u{e06f}";
    pub const CHEVRON_DOWN: &str = "\u{e06d}";
    pub const USER: &str = "\u{e19f}";
    pub const INBOX: &str = "\u{e0f7}";
    pub const ARCHIVE: &str = "\u{e041}";
    pub const MESSAGES_SQUARE: &str = "\u{e40d}";
    pub const MESSAGE_CIRCLE_MORE: &str = "\u{e565}";
    pub const USER_SEARCH: &str = "\u{e578}";
    pub const EYE_OFF: &str = "\u{e0bb}";
    pub const HARD_DRIVE: &str = "\u{e0ed}";
    pub const TAG: &str = "\u{e17f}";
    pub const EDIT: &str = "\u{e172}";
    pub const PANEL_LEFT_CLOSE: &str = "\u{e21c}";
    pub const PANEL_LEFT_OPEN: &str = "\u{e21d}";
    pub const CHEVRONS_LEFT: &str = "\u{e072}";
    pub const CHEVRONS_RIGHT: &str = "\u{e073}";
    pub const FILE: &str = "\u{e0c0}";
    pub const STICKER: &str = "\u{e302}";
    pub const ARROW_UP: &str = "\u{e04a}";
    pub const ELLIPSIS: &str = "\u{e0b6}";
    pub const HEART: &str = "\u{e0f2}";
}

pub(super) fn draw_lucide_icon(painter: &egui::Painter, center: Pos2, icon: &str, size: f32, color: Color32) {
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        icon,
        FontId::proportional(size),
        color,
    );
}

pub(super) fn draw_icon_search(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::SEARCH, size, color);
}

pub(super) fn draw_icon_sticker(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::SMILE, size, color);
}

pub(super) fn draw_icon_phone(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::PHONE, size, color);
}

pub(super) fn draw_icon_video(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::VIDEO, size, color);
}

pub(super) fn draw_icon_image(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::IMAGE, size, color);
}

pub(super) fn draw_icon_drawer(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::PANEL_RIGHT, size, color);
}

pub(super) fn draw_icon_copy(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::COPY, size, color);
}

pub(super) fn draw_icon_user_plus(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    draw_lucide_icon(painter, center, lucide::USER_PLUS, size, color);
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

pub(super) const APP_ICON_PNG: &[u8] = include_bytes!("../assets/app_icon.png");

/// Decode the embedded 256x256 RGBA application icon for the OS window/taskbar (`ViewportBuilder::with_icon`).
pub fn load_app_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(APP_ICON_PNG).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

pub(super) fn load_app_logo_texture(ctx: &egui::Context) -> Option<TextureHandle> {
    let img = image::load_from_memory(APP_ICON_PNG).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    let color_img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    Some(ctx.load_texture("zalo_app_logo", color_img, TextureOptions::LINEAR))
}

pub(super) fn load_default_emoji_textures(ctx: &egui::Context) -> HashMap<String, TextureHandle> {
    let raw: [(&str, &[u8]); 6] = [
        ("like", include_bytes!("../assets/emojis/like.png")),
        ("heart", include_bytes!("../assets/emojis/heart.png")),
        ("smile", include_bytes!("../assets/emojis/smile.png")),
        ("surprised", include_bytes!("../assets/emojis/surprised.png")),
        ("cry", include_bytes!("../assets/emojis/cry.png")),
        ("rage", include_bytes!("../assets/emojis/rage.png")),
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


/// Render an iOS/Zalo PC style rounded Toggle Switch in pure egui.
pub(super) fn render_toggle_switch(ui: &mut egui::Ui, value: &mut bool, pal: &ThemePalette) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(38.0, 20.0), Sense::click());
    if resp.clicked() {
        *value = !*value;
        resp.mark_changed();
    }
    resp = resp.on_hover_cursor(CursorIcon::PointingHand);

    let bg = if *value {
        pal.primary
    } else if pal.is_dark {
        Color32::from_rgb(58, 63, 75)
    } else {
        Color32::from_rgb(203, 213, 225)
    };
    ui.painter().rect_filled(rect, Rounding::same(10.0), bg);

    let knob_radius = 7.5_f32;
    let knob_x = if *value {
        rect.right() - 10.0
    } else {
        rect.left() + 10.0
    };
    ui.painter().circle_filled(
        Pos2::new(knob_x, rect.center().y),
        knob_radius,
        Color32::WHITE,
    );
    resp
}


/// One font file chosen for the egui font stack (`index` selects a face inside a `.ttc`).
#[derive(Debug, Clone)]
struct FontPick {
    path: std::path::PathBuf,
    index: u32,
}

/// Number of faces in a TrueType collection (`ttcf` header), or 1 for a plain font.
fn ttc_face_count(bytes: &[u8]) -> u32 {
    if bytes.len() >= 12 && &bytes[0..4] == b"ttcf" {
        u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]).max(1)
    } else {
        1
    }
}

/// Recursively list font files below `roots` (depth-limited, capped).
#[cfg_attr(any(target_os = "windows", target_os = "macos"), allow(dead_code))]
fn scan_font_dirs(roots: &[std::path::PathBuf], max_depth: usize) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack: Vec<(std::path::PathBuf, usize)> = roots
        .iter()
        .filter(|r| r.is_dir())
        .map(|r| (r.clone(), 0))
        .collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < max_depth {
                    stack.push((path, depth + 1));
                }
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc") {
                    out.push(path);
                }
                if out.len() > 20_000 {
                    return out;
                }
            }
        }
    }
    out
}

/// Pick the first file (by priority order of `wanted` file names, case-insensitive) present in `files`.
#[cfg_attr(any(target_os = "windows", target_os = "macos"), allow(dead_code))]
fn pick_by_name(files: &[std::path::PathBuf], wanted: &[&str]) -> Option<std::path::PathBuf> {
    for want in wanted {
        if let Some(found) = files.iter().find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.eq_ignore_ascii_case(want))
                .unwrap_or(false)
        }) {
            return Some(found.clone());
        }
    }
    None
}

/// Locate a Vietnamese-capable Latin UI font, a CJK fallback and a monochrome symbol font.
fn discover_system_fonts() -> (Option<FontPick>, Option<FontPick>, Vec<FontPick>) {
    let pick = |p: std::path::PathBuf| FontPick { path: p, index: 0 };

    #[cfg(target_os = "windows")]
    {
        let windir = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"));
        let mut roots = vec![windir.join("Fonts")];
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            roots.push(std::path::PathBuf::from(local).join(r"Microsoft\Windows\Fonts"));
        }
        let find = |names: &[&str]| -> Option<std::path::PathBuf> {
            for root in &roots {
                for n in names {
                    let p = root.join(n);
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
            None
        };
        let latin = find(&["segoeui.ttf", "arial.ttf", "tahoma.ttf"]).map(pick);
        let cjk = find(&[
            "msyh.ttc",
            "msyhl.ttc",
            "msjh.ttc",
            "simsun.ttc",
            "YuGothM.ttc",
            "meiryo.ttc",
            "malgun.ttf",
        ])
        .map(pick);
        // Segoe UI Emoji (outline glyphs, rendered monochrome) then Segoe UI Symbol, so
        // characters like 👀 / 🙏 / ✓ never fall back to empty boxes.
        let symbols: Vec<FontPick> = ["seguiemj.ttf", "seguisym.ttf"]
            .iter()
            .filter_map(|n| find(&[n]))
            .map(pick)
            .collect();
        (latin, cjk, symbols)
    }

    #[cfg(target_os = "macos")]
    {
        let first = |names: &[&str]| -> Option<std::path::PathBuf> {
            names
                .iter()
                .map(std::path::PathBuf::from)
                .find(|p| p.is_file())
        };
        let latin = first(&[
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/Library/Fonts/Arial.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/Supplemental/Tahoma.ttf",
        ])
        .map(pick);
        let cjk = first(&[
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/Library/Fonts/Arial Unicode.ttf",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ])
        .map(pick);
        let symbols: Vec<FontPick> = first(&[
            "/System/Library/Fonts/Apple Symbols.ttf",
            "/System/Library/Fonts/Supplemental/Apple Symbols.ttf",
        ])
        .map(pick)
        .into_iter()
        .collect();
        (latin, cjk, symbols)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let mut roots = vec![
            std::path::PathBuf::from("/usr/share/fonts"),
            std::path::PathBuf::from("/usr/local/share/fonts"),
        ];
        if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
            roots.push(home.join(".local/share/fonts"));
            roots.push(home.join(".fonts"));
        }
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from) {
            roots.push(xdg.join("fonts"));
        }
        let files = scan_font_dirs(&roots, 5);
        let latin = pick_by_name(
            &files,
            &[
                "NotoSans-Regular.ttf",
                "NotoSans[wdth,wght].ttf",
                "NotoSans[wght].ttf",
                "NotoSans-Regular.otf",
                "DejaVuSans.ttf",
                "Ubuntu-R.ttf",
                "Ubuntu[wdth,wght].ttf",
                "LiberationSans-Regular.ttf",
                "OpenSans-Regular.ttf",
                "OpenSans[wdth,wght].ttf",
                "Roboto-Regular.ttf",
                "FreeSans.ttf",
            ],
        )
        .map(pick);
        let cjk = pick_by_name(
            &files,
            &[
                "NotoSansCJK-Regular.ttc",
                "NotoSansCJK-VF.ttc",
                "NotoSansCJK-VF.otf.ttc",
                "NotoSansCJKsc-Regular.otf",
                "NotoSansSC-Regular.otf",
                "NotoSansSC[wght].ttf",
                "NotoSansSC-Regular.ttf",
                "SourceHanSans-Regular.ttc",
                "SourceHanSansSC-Regular.otf",
                "SourceHanSansCN-Regular.otf",
                "wqy-microhei.ttc",
                "wqy-zenhei.ttc",
                "DroidSansFallbackFull.ttf",
                "DroidSansFallback.ttf",
                "NotoSerifCJK-Regular.ttc",
            ],
        )
        .map(pick);
        let symbols: Vec<FontPick> = [
            "NotoSansSymbols2-Regular.ttf",
            "NotoSansSymbols-Regular.ttf",
            "Symbola.ttf",
        ]
        .iter()
        .filter_map(|n| pick_by_name(&files, &[n]))
        .map(pick)
        .collect();
        (latin, cjk, symbols)
    }
}

/// Load `pick` into `fonts` under `name`; for CJK collections prefer the Simplified Chinese face.
fn insert_font(
    fonts: &mut egui::FontDefinitions,
    name: &str,
    pick: &FontPick,
    prefer_sc_face: bool,
) -> bool {
    let Ok(bytes) = std::fs::read(&pick.path) else {
        return false;
    };
    if bytes.len() < 12 {
        return false;
    }
    let faces = ttc_face_count(&bytes);
    let file_name = pick
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    // Noto Sans CJK / Source Han Sans collections are ordered JP, KR, SC, TC(, HK).
    let index = if prefer_sc_face
        && faces > 2
        && (file_name.starts_with("notosanscjk")
            || file_name.starts_with("sourcehansans")
            || file_name.starts_with("notoserifcjk"))
    {
        2
    } else {
        pick.index.min(faces.saturating_sub(1))
    };
    let mut data = egui::FontData::from_owned(bytes);
    data.index = index;
    fonts.font_data.insert(name.to_owned(), data);
    true
}

pub(super) fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 1. Embedded Lucide icon font (Private Use Area glyphs for every UI icon).
    const LUCIDE_BYTES: &[u8] = include_bytes!("../assets/fonts/lucide.ttf");
    fonts.font_data.insert(
        "lucide_font".to_owned(),
        egui::FontData::from_static(LUCIDE_BYTES),
    );

    // 2. System fonts: Vietnamese-capable Latin UI font, CJK fallback, symbols.
    let (latin, cjk, extras) = discover_system_fonts();
    let has_latin = latin
        .as_ref()
        .map(|p| insert_font(&mut fonts, "ui_latin", p, false))
        .unwrap_or(false);
    let has_cjk = cjk
        .as_ref()
        .map(|p| insert_font(&mut fonts, "ui_cjk", p, true))
        .unwrap_or(false);
    let mut extra_names: Vec<String> = Vec::new();
    for (i, p) in extras.iter().enumerate() {
        let name = format!("ui_symbols_{i}");
        if insert_font(&mut fonts, &name, p, false) {
            extra_names.push(name);
        }
    }

    // Proportional: Latin → Lucide → CJK → symbols → egui defaults (Ubuntu-Light, emoji).
    // Lucide sits before CJK so CJK fonts that ship Private-Use glyphs never shadow UI icons.
    let prop = fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default();
    let defaults: Vec<String> = std::mem::take(prop);
    if has_latin {
        prop.push("ui_latin".to_owned());
    }
    prop.push("lucide_font".to_owned());
    if has_cjk {
        prop.push("ui_cjk".to_owned());
    }
    prop.extend(extra_names.iter().cloned());
    prop.extend(defaults);

    let mono = fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default();
    if has_latin {
        mono.push("ui_latin".to_owned());
    }
    mono.push("lucide_font".to_owned());
    if has_cjk {
        mono.push("ui_cjk".to_owned());
    }

    ctx.set_fonts(fonts);
}

// ---------------------------------------------------------------------------
// Layout helpers shared by the redesigned workspace
// ---------------------------------------------------------------------------

/// Conversation-list timestamp: `HH:MM` today, `Hôm qua`, weekday `T2…CN` within a week, else `dd/mm`.
pub(super) fn format_list_time(ts_ms: i64, fallback: &str) -> String {
    if ts_ms <= 0 {
        return fallback.to_string();
    }
    let now = crate::timefmt::now_ms();
    let days = crate::timefmt::local_day_index(now) - crate::timefmt::local_day_index(ts_ms);
    match days {
        i64::MIN..=0 => crate::timefmt::hm_from_ms(ts_ms),
        1 => "Hôm qua".to_string(),
        2..=6 => weekday_short(crate::timefmt::local_weekday(ts_ms)).to_string(),
        _ => {
            let (y, m, d) = crate::timefmt::local_ymd(ts_ms);
            let (ny, _, _) = crate::timefmt::local_ymd(now);
            if y == ny {
                format!("{d:02}/{m:02}")
            } else {
                format!("{d:02}/{m:02}/{:02}", y.rem_euclid(100))
            }
        }
    }
}

/// `T2`…`T7`, `CN` for Monday = 1 … Sunday = 7.
pub(super) fn weekday_short(wd: u32) -> &'static str {
    match wd {
        1 => "T2",
        2 => "T3",
        3 => "T4",
        4 => "T5",
        5 => "T6",
        6 => "T7",
        _ => "CN",
    }
}

/// `Thứ Hai`…`Chủ nhật` for Monday = 1 … Sunday = 7.
pub(super) fn weekday_long(wd: u32) -> &'static str {
    match wd {
        1 => "Thứ Hai",
        2 => "Thứ Ba",
        3 => "Thứ Tư",
        4 => "Thứ Năm",
        5 => "Thứ Sáu",
        6 => "Thứ Bảy",
        _ => "Chủ nhật",
    }
}

/// Paint a diagonal two-stop gradient across `rect` (top-left `top` → bottom-right `bottom`).
pub(super) fn paint_diagonal_gradient(
    painter: &egui::Painter,
    rect: Rect,
    top: Color32,
    bottom: Color32,
) {
    use egui::epaint::{Mesh, Vertex};
    let mid = lerp_color(top, bottom, 0.5);
    let mut mesh = Mesh::default();
    let uv = egui::epaint::WHITE_UV;
    for (pos, color) in [
        (rect.left_top(), top),
        (rect.right_top(), mid),
        (rect.right_bottom(), bottom),
        (rect.left_bottom(), mid),
    ] {
        mesh.vertices.push(Vertex { pos, uv, color });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// Linear interpolation between two colours (sRGB space; good enough for UI tints).
pub(super) fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 { (x as f32 + (y as f32 - x as f32) * t).round() as u8 };
    Color32::from_rgba_unmultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

/// Pill badge with a count (e.g. unread). Returns the painted rect.
pub(super) fn paint_count_badge(
    painter: &egui::Painter,
    center: Pos2,
    count: usize,
    fill: Color32,
) -> Rect {
    let txt = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    let w = match txt.chars().count() {
        1 => 20.0,
        2 => 24.0,
        _ => 30.0,
    };
    let rect = Rect::from_center_size(center, Vec2::new(w, 20.0));
    painter.rect_filled(rect, Rounding::same(10.0), fill);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        txt,
        FontId::proportional(11.5),
        Color32::WHITE,
    );
    rect
}

/// Draw a clickable square icon button; returns the response.
pub(super) fn icon_button(
    ui: &mut egui::Ui,
    icon: &str,
    size: f32,
    icon_size: f32,
    active: bool,
    pal: &ThemePalette,
    tooltip: &str,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let bg = if active {
        pal.nav_active
    } else if resp.hovered() {
        pal.bg_hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, Rounding::same(size * 0.28), bg);
    draw_lucide_icon(
        ui.painter(),
        rect.center(),
        icon,
        icon_size,
        if active {
            pal.primary
        } else if resp.hovered() {
            pal.text_primary
        } else {
            pal.text_secondary
        },
    );
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if tooltip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tooltip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttc_header_face_count() {
        let mut ttc = b"ttcf\x00\x02\x00\x00".to_vec();
        ttc.extend_from_slice(&5u32.to_be_bytes());
        assert_eq!(ttc_face_count(&ttc), 5);
        assert_eq!(ttc_face_count(b"\x00\x01\x00\x00plainfontdata"), 1);
    }

    #[test]
    fn list_time_uses_weekday_and_dates() {
        let now = crate::timefmt::now_ms();
        let today = format_list_time(now, "");
        assert_eq!(today.len(), 5, "today renders HH:MM, got {today}");
        assert_eq!(format_list_time(now - 86_400_000, ""), "Hôm qua");
        let three_days = format_list_time(now - 3 * 86_400_000, "");
        assert!(["T2", "T3", "T4", "T5", "T6", "T7", "CN"].contains(&three_days.as_str()));
        assert!(format_list_time(now - 40 * 86_400_000, "").contains('/'));
        assert_eq!(format_list_time(0, "09:30"), "09:30");
    }

    #[test]
    fn pick_by_name_respects_priority_order() {
        let files = vec![
            std::path::PathBuf::from("/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf"),
            std::path::PathBuf::from("/usr/share/fonts/google-noto-vf/NotoSans[wght].ttf"),
        ];
        let got = pick_by_name(
            &files,
            &["NotoSans-Regular.ttf", "NotoSans[wght].ttf", "DejaVuSans.ttf"],
        );
        assert_eq!(got.unwrap().file_name().unwrap(), "NotoSans[wght].ttf");
    }
}
