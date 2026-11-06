//! Telegram-style emoji panel for the composer. egui can only draw monochrome emoji glyphs,
//! so on macOS each emoji is rendered once by AppKit (Apple Color Emoji) into a small RGBA
//! image and cached as a texture; elsewhere the glyph is drawn as text.

use super::*;

/// (tab icon, title, emoji list). The first tab ("Gần đây") is filled from settings.
pub(super) const EMOJI_CATEGORIES: &[(&str, &str, &[&str])] = &[
    (
        "😀",
        "Mặt cười",
        &[
            "😀", "😃", "😄", "😁", "😆", "😅", "😂", "🤣", "🥲", "😊", "😇", "🙂", "🙃", "😉",
            "😌", "😍", "🥰", "😘", "😗", "😙", "😚", "😋", "😛", "😝", "😜", "🤪", "🤨", "🧐",
            "🤓", "😎", "🥸", "🤩", "🥳", "😏", "😒", "😞", "😔", "😟", "😕", "🙁", "😣", "😖",
            "😫", "😩", "🥺", "😢", "😭", "😤", "😠", "😡", "🤬", "🤯", "😳", "🥵", "🥶", "😱",
            "😨", "😰", "😥", "😓", "🤗", "🤔", "🤭", "🤫", "🤥", "😶", "😐", "😑", "😬", "🙄",
            "😯", "😦", "😧", "😮", "😲", "🥱", "😴", "🤤", "😪", "😵", "🤐", "🥴", "🤢", "🤮",
            "🤧", "😷", "🤒", "🤕",
        ],
    ),
    (
        "👍",
        "Cử chỉ",
        &[
            "👍", "👎", "👌", "🤌", "🤏", "✌️", "🤞", "🤟", "🤘", "🤙", "👈", "👉", "👆", "👇",
            "☝️", "✋", "🤚", "🖐️", "🖖", "👋", "🤝", "🙏", "💪", "👏", "🙌", "👐", "🤲", "✍️",
            "💅", "🫶",
        ],
    ),
    (
        "❤️",
        "Trái tim",
        &[
            "❤️", "🧡", "💛", "💚", "💙", "💜", "🖤", "🤍", "🤎", "💔", "❣️", "💕", "💞", "💓",
            "💗", "💖", "💘", "💝", "💋", "🌹",
        ],
    ),
    (
        "🐶",
        "Động vật",
        &[
            "🐶", "🐱", "🐭", "🐹", "🐰", "🦊", "🐻", "🐼", "🐨", "🐯", "🦁", "🐮", "🐷", "🐸",
            "🐵", "🙈", "🐔", "🐧", "🐦", "🐤", "🦆", "🦉", "🐴", "🦄", "🐝", "🦋", "🐌", "🐞",
            "🐢", "🐍", "🐙", "🐠", "🐬", "🐳",
        ],
    ),
    (
        "🍔",
        "Đồ ăn",
        &[
            "🍏", "🍎", "🍐", "🍊", "🍋", "🍌", "🍉", "🍇", "🍓", "🍒", "🍑", "🥭", "🍍", "🥥",
            "🥝", "🍅", "🥑", "🥦", "🌽", "🥕", "🍞", "🧀", "🍗", "🍖", "🍔", "🍟", "🍕", "🌭",
            "🥪", "🌮", "🍜", "🍲", "🍣", "🍤", "🍙", "🍚", "🍰", "🎂", "🍦", "🍩", "🍪", "☕",
            "🍵", "🧋", "🍺", "🍻", "🥂", "🍷",
        ],
    ),
    (
        "⚽",
        "Hoạt động",
        &[
            "⚽", "🏀", "🏈", "⚾", "🎾", "🏐", "🎱", "🏓", "🏸", "🥊", "🎮", "🎯", "🎲", "🎸",
            "🎤", "🎧", "🎬", "🎨", "🎉", "🎊", "🎁", "🏆", "🥇",
        ],
    ),
    (
        "💡",
        "Đồ vật",
        &[
            "💻", "📱", "⌚", "📷", "💡", "📚", "✏️", "📌", "📎", "✂️", "🔑", "🔒", "💰", "💳",
            "📦", "✉️", "📅", "⏰", "🚗", "✈️", "🏠", "🌙", "⭐", "☀️", "🌈", "🔥", "💯", "✅",
            "❌", "❓", "❗",
        ],
    ),
];

/// Render one emoji with Apple Color Emoji into a `px`×`px` premultiplied RGBA image.
#[cfg(target_os = "macos")]
pub(super) fn render_emoji_image(emoji: &str, px: usize) -> Option<egui::ColorImage> {
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{msg_send, msg_send_id};
    use objc2_foundation::{NSPoint, NSSize, NSString};

    objc2::rc::autoreleasepool(|_| unsafe {
        // SAFETY: standard AppKit bitmap drawing into a private NSBitmapImageRep context on the
        // main thread; all objects are retained for the duration of the drawing.
        let rep_class = AnyClass::get("NSBitmapImageRep")?;
        let ctx_class = AnyClass::get("NSGraphicsContext")?;
        let font_class = AnyClass::get("NSFont")?;
        let dict_class = AnyClass::get("NSDictionary")?;
        let astr_class = AnyClass::get("NSAttributedString")?;
        let color_space = NSString::from_str("NSDeviceRGBColorSpace");
        let planes: *mut *mut u8 = std::ptr::null_mut();
        let alloc: Allocated<AnyObject> = msg_send_id![rep_class, alloc];
        let rep: Option<Retained<AnyObject>> = msg_send_id![
            alloc,
            initWithBitmapDataPlanes: planes,
            pixelsWide: px as isize,
            pixelsHigh: px as isize,
            bitsPerSample: 8isize,
            samplesPerPixel: 4isize,
            hasAlpha: true,
            isPlanar: false,
            colorSpaceName: &*color_space,
            bytesPerRow: (px * 4) as isize,
            bitsPerPixel: 32isize
        ];
        let rep = rep?;
        let gctx: *mut AnyObject = msg_send![ctx_class, graphicsContextWithBitmapImageRep: &*rep];
        if gctx.is_null() {
            return None;
        }
        let font_name = NSString::from_str("Apple Color Emoji");
        let size = px as f64 * 0.78;
        let mut font: *mut AnyObject = msg_send![font_class, fontWithName: &*font_name, size: size];
        if font.is_null() {
            font = msg_send![font_class, systemFontOfSize: size];
        }
        let key = NSString::from_str("NSFont");
        let attrs: *mut AnyObject =
            msg_send![dict_class, dictionaryWithObject: font, forKey: &*key];
        let text = NSString::from_str(emoji);
        let alloc: Allocated<AnyObject> = msg_send_id![astr_class, alloc];
        let astr: Option<Retained<AnyObject>> =
            msg_send_id![alloc, initWithString: &*text, attributes: attrs];
        let astr = astr?;
        let _: () = msg_send![ctx_class, saveGraphicsState];
        let _: () = msg_send![ctx_class, setCurrentContext: gctx];
        let sz: NSSize = msg_send![&*astr, size];
        let x = (px as f64 - sz.width) / 2.0;
        let y = (px as f64 - sz.height) / 2.0;
        let _: () = msg_send![&*astr, drawAtPoint: NSPoint::new(x, y)];
        let _: () = msg_send![gctx, flushGraphics];
        let _: () = msg_send![ctx_class, restoreGraphicsState];
        let data: *const u8 = msg_send![&*rep, bitmapData];
        if data.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(data, px * px * 4);
        Some(egui::ColorImage::from_rgba_premultiplied([px, px], bytes))
    })
}

#[cfg(not(target_os = "macos"))]
pub(super) fn render_emoji_image(_emoji: &str, _px: usize) -> Option<egui::ColorImage> {
    None
}

/// Colour emoji textures shared by the picker, the composer and message bubbles.
#[allow(clippy::type_complexity)] // (textures, keys being rendered)
static EMOJI_CACHE: std::sync::Mutex<Option<(HashMap<String, TextureHandle>, HashSet<String>)>> =
    std::sync::Mutex::new(None);
/// (pass number, renders done this pass): at most a few new AppKit renders per frame.
static EMOJI_BUDGET: std::sync::Mutex<(u64, usize)> = std::sync::Mutex::new((0, 0));

/// Colour texture for `emoji`, rendering it (bounded per frame) on first use. `None` while
/// pending or when the platform cannot render it; callers request a repaint for pending ones.
pub(super) fn emoji_texture_shared(ctx: &egui::Context, emoji: &str) -> Option<TextureHandle> {
    let mut guard = EMOJI_CACHE.lock().ok()?;
    let (cache, failed) = guard.get_or_insert_with(|| (HashMap::new(), HashSet::new()));
    if let Some(t) = cache.get(emoji) {
        return Some(t.clone());
    }
    if failed.contains(emoji) {
        return None;
    }
    {
        let pass = ctx.cumulative_pass_nr();
        let mut b = EMOJI_BUDGET.lock().ok()?;
        if b.0 != pass {
            *b = (pass, 0);
        }
        if b.1 >= 16 {
            ctx.request_repaint();
            return None;
        }
        b.1 += 1;
    }
    match render_emoji_image(emoji, 64) {
        Some(img) => {
            let tex = ctx.load_texture(format!("emoji_{emoji}"), img, TextureOptions::LINEAR);
            cache.insert(emoji.to_string(), tex.clone());
            Some(tex)
        }
        None => {
            failed.insert(emoji.to_string());
            None
        }
    }
}

/// Append `text` to `job`; emoji clusters become transparent, square slots (widened with
/// letter spacing) that [`paint_emoji_overlay`] later fills with the colour image.
pub(super) fn append_text_with_emoji(
    job: &mut egui::text::LayoutJob,
    ctx: &egui::Context,
    text: &str,
    font: FontId,
    color: Color32,
) {
    let fmt = |c: Color32| egui::TextFormat {
        font_id: font.clone(),
        color: c,
        ..Default::default()
    };
    let clusters = emoji_clusters(text);
    if clusters.is_empty() || !cfg!(target_os = "macos") {
        if !text.is_empty() {
            job.append(text, 0.0, fmt(color));
        }
        return;
    }
    let byte_of: Vec<usize> = text
        .char_indices()
        .map(|(b, _)| b)
        .chain(std::iter::once(text.len()))
        .collect();
    let mut last = 0usize;
    for (a, b, cluster) in clusters {
        let (ba, bb) = (byte_of[a], byte_of[b]);
        if ba > last {
            job.append(&text[last..ba], 0.0, fmt(color));
        }
        // Make the slot one em-ish square whatever glyph (or box) the font has.
        let target = 26.0;
        let n = cluster.chars().count().max(1) as f32;
        let emoji_font = FontId::proportional(26.0);
        let natural: f32 =
            ctx.fonts(|f| cluster.chars().map(|c| f.glyph_width(&emoji_font, c)).sum());
        let mut f = fmt(Color32::TRANSPARENT);
        f.font_id = emoji_font;
        f.extra_letter_spacing = ((target - natural) / n).max(0.0);
        job.append(&text[ba..bb], 0.0, f);
        last = bb;
    }
    if last < text.len() {
        job.append(&text[last..], 0.0, fmt(color));
    }
}

/// Fill the transparent emoji slots of `galley` (drawn at `pos`) with colour images.
pub(super) fn paint_emoji_overlay(
    painter: &egui::Painter,
    ctx: &egui::Context,
    galley: &egui::Galley,
    pos: Pos2,
) {
    let text = galley.text();
    if !text.chars().any(is_emoji_start) {
        return;
    }
    for (a, b, cluster) in emoji_clusters(text) {
        let r0 = galley.pos_from_ccursor(egui::text::CCursor::new(a));
        let r1 = galley.pos_from_ccursor(egui::text::CCursor::new(b));
        if (r0.min.y - r1.min.y).abs() > 0.5 {
            continue;
        }
        let slot = Rect::from_min_max(
            Pos2::new(pos.x + r0.min.x, pos.y + r0.min.y),
            Pos2::new(pos.x + r1.min.x, pos.y + r0.max.y),
        );
        let side = 24.0_f32.min(slot.width());
        if let Some(tex) = emoji_texture_shared(ctx, &cluster) {
            painter.image(
                tex.id(),
                Rect::from_center_size(slot.center(), Vec2::splat(side)),
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            ctx.request_repaint();
        }
    }
}

/// Whether `c` starts an emoji (pictographs, symbols & dingbats, misc technical).
fn is_emoji_start(c: char) -> bool {
    matches!(c as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2300..=0x23FF | 0x2B00..=0x2BFF | 0x3297 | 0x3299)
}

/// Emoji clusters in `text` as (start char index, end char index, cluster string): a base
/// emoji plus variation selectors, skin tones and ZWJ-joined parts.
pub(super) fn emoji_clusters(text: &str) -> Vec<(usize, usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !is_emoji_start(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        loop {
            match chars.get(i).map(|c| *c as u32) {
                Some(0xFE0F) | Some(0x1F3FB..=0x1F3FF) | Some(0x20E3) => i += 1,
                Some(0x200D) if chars.get(i + 1).is_some() => i += 2,
                _ => break,
            }
        }
        out.push((start, i, chars[start..i].iter().collect()));
    }
    out
}

impl ZaloGuiApp {
    /// Fill the composer's transparent emoji slots with colour images.
    pub(super) fn overlay_color_emoji(
        &mut self,
        ui: &egui::Ui,
        out: &egui::text_edit::TextEditOutput,
        _bg: Color32,
        _pal: &ThemePalette,
    ) {
        let painter = ui.painter().with_clip_rect(out.text_clip_rect);
        paint_emoji_overlay(&painter, ui.ctx(), &out.galley, out.galley_pos);
    }

    /// Texture for `emoji` from the shared cache (`budget` kept for call-site compatibility).
    pub(super) fn emoji_texture(
        &mut self,
        ctx: &egui::Context,
        emoji: &str,
        budget: &mut usize,
    ) -> Option<TextureHandle> {
        let _ = budget;
        emoji_texture_shared(ctx, emoji)
    }

    /// Draw one emoji (colour texture when available, glyph otherwise) centred in `rect`.
    fn paint_emoji(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        emoji: &str,
        budget: &mut usize,
        pal: &ThemePalette,
    ) {
        if let Some(tex) = self.emoji_texture(ui.ctx(), emoji, budget) {
            ui.painter().image(
                tex.id(),
                Rect::from_center_size(rect.center(), Vec2::splat(24.0)),
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                emoji,
                FontId::proportional(rect.height() * 0.62),
                pal.text_primary,
            );
            ui.ctx().request_repaint();
        }
    }

    /// Floating panel above the composer's emoji button. Returns the emoji clicked.
    pub(super) fn render_emoji_picker(
        &mut self,
        ctx: &egui::Context,
        pal: &ThemePalette,
        anchor: Rect,
    ) -> Option<String> {
        let mut picked: Option<String> = None;
        let mut budget = 12usize;
        let fade = appear(ctx, Id::new("composer_emoji_picker"), 0.16);
        let recent: Vec<String> = self.settings.recent_emojis.clone();
        let screen = ctx.screen_rect();
        let area = egui::Area::new(Id::new("composer_emoji_picker"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .fixed_pos(Pos2::new(
                anchor.right() + 8.0,
                anchor.top() - 10.0 + (1.0 - fade) * 8.0,
            ))
            .constrain_to(screen)
            .show(ctx, |ui| {
                ui.set_opacity(fade);
                egui::Frame::none()
                    .fill(pal.bg_elevated)
                    .rounding(Rounding::same(20.0))
                    .shadow(egui::epaint::Shadow {
                        offset: Vec2::new(0.0, 6.0),
                        blur: 22.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(if pal.is_dark { 120 } else { 50 }),
                    })
                    .inner_margin(Margin::symmetric(10.0, 10.0))
                    .show(ui, |ui| {
                        let width = 8.0 * 38.0;
                        ui.set_width(width);
                        // Category tabs
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let tabs: Vec<(usize, &str)> = std::iter::once((0usize, "🕘"))
                                .chain(
                                    EMOJI_CATEGORIES
                                        .iter()
                                        .enumerate()
                                        .map(|(i, c)| (i + 1, c.0)),
                                )
                                .collect();
                            for (i, icon) in tabs {
                                let (r, resp) =
                                    ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
                                if self.emoji_tab == i || resp.hovered() {
                                    ui.painter().circle_filled(r.center(), 16.0, pal.bg_hover);
                                }
                                if i == 0 {
                                    draw_lucide_icon(
                                        ui.painter(),
                                        r.center(),
                                        lucide::CLOCK,
                                        15.0,
                                        pal.text_secondary,
                                    );
                                } else {
                                    self.paint_emoji(ui, r.shrink(4.0), icon, &mut budget, pal);
                                }
                                let title = if i == 0 {
                                    "Gần đây"
                                } else {
                                    EMOJI_CATEGORIES[i - 1].1
                                };
                                if resp
                                    .on_hover_text(title)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    self.emoji_tab = i;
                                }
                            }
                        });
                        ui.add_space(6.0);
                        let (title, list): (&str, Vec<String>) = if self.emoji_tab == 0 {
                            ("GẦN ĐÂY", recent.clone())
                        } else {
                            let c = &EMOJI_CATEGORIES[self.emoji_tab - 1];
                            (c.1, c.2.iter().map(|s| s.to_string()).collect())
                        };
                        ui.label(
                            RichText::new(title.to_uppercase())
                                .size(11.0)
                                .color(pal.text_muted),
                        );
                        ui.add_space(2.0);
                        egui::ScrollArea::vertical()
                            .id_salt(("emoji_grid", self.emoji_tab))
                            .max_height((screen.height() - 160.0).clamp(80.0, 180.0))
                            .min_scrolled_height((screen.height() - 160.0).clamp(80.0, 180.0))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if list.is_empty() {
                                    ui.label(
                                        RichText::new("Chưa có emoji dùng gần đây")
                                            .size(12.0)
                                            .color(pal.text_muted),
                                    );
                                    return;
                                }
                                for row in list.chunks(8) {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 0.0;
                                        for e in row {
                                            let (r, resp) = ui.allocate_exact_size(
                                                Vec2::splat(38.0),
                                                Sense::click(),
                                            );
                                            if resp.hovered() {
                                                ui.painter().rect_filled(
                                                    r.shrink(1.0),
                                                    Rounding::same(10.0),
                                                    pal.bg_hover,
                                                );
                                            }
                                            self.paint_emoji(
                                                ui,
                                                r.shrink(3.0),
                                                e,
                                                &mut budget,
                                                pal,
                                            );
                                            if resp
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .clicked()
                                            {
                                                picked = Some(e.clone());
                                            }
                                        }
                                    });
                                }
                            });
                    });
            });
        // Click outside (not on the toggle button) closes the panel.
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed())
            && ctx.pointer_interact_pos().is_some_and(|p| {
                !area.response.rect.contains(p) && !anchor.expand(4.0).contains(p)
            });
        if clicked_outside || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.emoji_picker_open = false;
        }
        if let Some(e) = &picked {
            let list = &mut self.settings.recent_emojis;
            list.retain(|x| x != e);
            list.insert(0, e.clone());
            list.truncate(32);
            self.settings.save();
        }
        picked
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::render_emoji_image;

    #[test]
    fn renders_colour_emoji_with_appkit() {
        let img = render_emoji_image("😀", 64).expect("AppKit rendered an image");
        assert_eq!(img.size, [64, 64]);
        let opaque = img.pixels.iter().filter(|p| p.a() > 200).count();
        assert!(
            opaque > 300,
            "expected a drawn glyph, got {opaque} opaque pixels"
        );
        // Apple Color Emoji is coloured: some pixels are clearly not grey.
        let coloured = img
            .pixels
            .iter()
            .filter(|p| p.a() > 200 && (p.r() as i32 - p.b() as i32).abs() > 60)
            .count();
        assert!(coloured > 50, "expected colour pixels, got {coloured}");
    }
}

#[cfg(test)]
mod cluster_tests {
    use super::emoji_clusters;

    #[test]
    fn groups_variation_selectors_and_zwj() {
        let t = "hi 😀 👍🏽 ❤️ 👨\u{200D}💻!";
        let c = emoji_clusters(t);
        let strs: Vec<&str> = c.iter().map(|(_, _, s)| s.as_str()).collect();
        assert_eq!(strs, vec!["😀", "👍🏽", "❤️", "👨\u{200D}💻"]);
        assert!(emoji_clusters("xin chào").is_empty());
    }
}
