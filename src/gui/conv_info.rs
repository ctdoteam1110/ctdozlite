//! "Thông tin hội thoại" drawer, laid out like Zalo PC: profile, quick actions, then collapsible
//! sections for shared photos, files and links, and the conversation's privacy actions.

use super::workspace::ThreadCtxAction;
use super::*;

/// Photos shown before "Xem tất cả" (Zalo shows two rows of four).
const PHOTO_PREVIEW: usize = 8;
/// Files / links shown before "Xem tất cả".
const LIST_PREVIEW: usize = 3;
/// Upper bound once expanded, so huge threads stay cheap to lay out.
const LIST_MAX: usize = 60;

struct SharedFile {
    name: String,
    size: String,
    date: String,
    url: String,
    is_video: bool,
}

struct SharedLink {
    title: String,
    domain: String,
    date: String,
    url: String,
}

/// `dd/mm` for this year, `dd/mm/yyyy` otherwise; "Hôm nay" / "Hôm qua" for recent days.
fn short_date(ts: i64) -> String {
    let Some(d) = crate::clock::local_datetime(ts) else {
        return String::new();
    };
    let now = chrono::Local::now();
    let days = (now.date_naive() - d.date_naive()).num_days();
    match days {
        0 => "Hôm nay".into(),
        1 => "Hôm qua".into(),
        _ if d.format("%Y").to_string() == now.format("%Y").to_string() => {
            d.format("%d/%m").to_string()
        }
        _ => d.format("%d/%m/%Y").to_string(),
    }
}

/// Split `"report.pdf (1.5 MB)"` into the size part when the title carries one.
pub(super) fn size_from_title(title: Option<&str>) -> String {
    let t = title.unwrap_or("").trim();
    if let (Some(open), true) = (t.rfind('('), t.ends_with(')')) {
        let inner = &t[open + 1..t.len() - 1];
        if inner.ends_with("B") && inner.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return inner.to_string();
        }
    }
    String::new()
}

/// Colored file-type badge (PDF red, Word blue, Excel green, …) like Zalo's file list.
pub(super) fn paint_file_badge(painter: &egui::Painter, rect: Rect, name: &str, is_video: bool) {
    let ext = name
        .rsplit('.')
        .next()
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let (label, color) = if is_video {
        ("▶", Color32::from_rgb(124, 58, 237))
    } else {
        match ext.as_str() {
            "pdf" => ("PDF", Color32::from_rgb(239, 83, 80)),
            "doc" | "docx" => ("W", Color32::from_rgb(59, 130, 246)),
            "xls" | "xlsx" | "csv" => ("X", Color32::from_rgb(34, 160, 90)),
            "ppt" | "pptx" => ("P", Color32::from_rgb(234, 120, 40)),
            "zip" | "rar" | "7z" => ("ZIP", Color32::from_rgb(140, 100, 200)),
            "mp3" | "m4a" | "wav" => ("♪", Color32::from_rgb(236, 72, 153)),
            _ => ("FILE", Color32::from_rgb(120, 130, 145)),
        }
    };
    painter.rect_filled(rect, Rounding::same(8.0), color);
    let size = if label.chars().count() >= 3 {
        10.5
    } else {
        15.0
    };
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(size),
        Color32::WHITE,
    );
}

impl ZaloGuiApp {
    /// Render the drawer contents. Returns `true` when the user closed it (X).
    pub(super) fn render_conversation_info(
        &mut self,
        ui: &mut egui::Ui,
        pal: &ThemePalette,
        meta: &(String, String, Option<String>, bool, usize),
    ) -> bool {
        let (d_id, d_name, d_ava, d_is_group, d_msg_count) = meta;
        let (d_id, d_name, d_is_group, d_msg_count) =
            (d_id.clone(), d_name.clone(), *d_is_group, *d_msg_count);
        let d_ava = d_ava.clone();
        let mut close = false;
        let now_unix = crate::login::unix_now();
        let d_is_cloud = d_name.contains("Cloud của tôi");

        // Collect shared media/files/links from the loaded messages (newest first).
        let mut photos: Vec<(String, String, String, String)> = Vec::new();
        let mut files: Vec<SharedFile> = Vec::new();
        let mut links: Vec<SharedLink> = Vec::new();
        let mut last_msg_id = String::new();
        let mut is_pinned = self.settings.pinned_threads.contains(&d_id);
        let mut is_muted = self.settings.muted_threads.contains_key(&d_id);
        let mut is_hidden = self.settings.hidden_threads.contains(&d_id);
        if let Some(t) = self
            .threads
            .get(self.selected_thread_index)
            .filter(|t| t.id == d_id)
        {
            is_pinned |= t.presentation.pinned;
            is_muted |= t.presentation.muted;
            is_hidden |= t.presentation.hidden;
            last_msg_id = t.messages.last().map(|m| m.id.clone()).unwrap_or_default();
            for m in t.messages.iter().rev() {
                if is_sticker_message(m).is_some() {
                    continue;
                }
                if is_zalo_photo_message(m) {
                    if photos.len() < LIST_MAX {
                        if let Some(thumb) = photo_primary_url(m) {
                            let full = m.link_url.as_deref().unwrap_or(thumb).to_string();
                            photos.push((
                                format!("thumb_{}", crate::crypto::md5_hex(thumb)),
                                thumb.to_string(),
                                full,
                                m.link_title
                                    .clone()
                                    .unwrap_or_else(|| "Hình ảnh đã chia sẻ".into()),
                            ));
                        }
                    }
                } else if is_zalo_file_or_video_message(m) {
                    if files.len() < LIST_MAX {
                        let url = m.link_url.clone().unwrap_or_default();
                        let is_video = m.link_src.as_deref() == Some("Video Zalo")
                            || m.text.trim().starts_with("[Video]");
                        let name = crate::chat::resolve_attachment_file_name(
                            m.link_title.as_deref(),
                            m.text.trim(),
                            &url,
                        );
                        files.push(SharedFile {
                            name,
                            size: size_from_title(m.link_title.as_deref()),
                            date: short_date(m.ts),
                            url,
                            is_video,
                        });
                    }
                } else if let Some((_, url, domain)) = extract_message_link_view(m) {
                    if links.len() < LIST_MAX {
                        let title = m
                            .link_title
                            .as_deref()
                            .map(str::trim)
                            .filter(|t| !t.is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| url.clone());
                        links.push(SharedLink {
                            title,
                            domain,
                            date: short_date(m.ts),
                            url,
                        });
                    }
                }
            }
        }

        // Pinned: the group board's topics plus messages pinned on this Mac.
        let mut pins: Vec<(String, String, String)> = self
            .pinned_topics
            .get(&d_id)
            .map(|v| {
                v.iter()
                    .map(|p| (p.sender_name.clone(), p.text.clone(), p.msg_id.clone()))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(t) = self
            .threads
            .get(self.selected_thread_index)
            .filter(|t| t.id == d_id)
        {
            for m in t.messages.iter().rev().filter(|m| m.is_pinned) {
                if pins.iter().any(|(_, _, id)| !id.is_empty() && *id == m.id) {
                    continue;
                }
                let sender = if m.is_outgoing {
                    "Bạn".to_string()
                } else if !m.sender.is_empty() && m.sender != "0" {
                    m.sender.clone()
                } else {
                    d_name.clone()
                };
                let text = if m.sticker_id.is_some() {
                    "[Nhãn dán]".to_string()
                } else {
                    m.text.clone()
                };
                pins.push((sender, text, m.id.clone()));
            }
        }
        let mut jump_to: Option<String> = None;

        let mut action: Option<ThreadCtxAction> = None;
        let mut open_alias = false;
        let mut open_profile = false;
        let mut open_members = false;
        let mut download: Option<(String, String)> = None;

        // Title bar
        egui::Frame::none()
            .inner_margin(Margin::symmetric(14.0, 10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(lucide::X)
                                        .size(15.0)
                                        .color(pal.text_secondary),
                                )
                                .frame(false),
                            )
                            .on_hover_text("Đóng")
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            close = true;
                        }
                        ui.with_layout(Layout::top_down(Align::Center), |ui| {
                            ui.label(
                                RichText::new("Thông tin hội thoại")
                                    .size(14.5)
                                    .strong()
                                    .color(pal.text_primary),
                            );
                        });
                    });
                });
            });

        egui::ScrollArea::vertical()
            .id_salt(("conv_info_scroll", &d_id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let inner_w = ui.available_width();
                egui::Frame::none()
                    .inner_margin(Margin { left: 12.0, right: 12.0, top: 2.0, bottom: 14.0 })
                    .show(ui, |ui| {
                        ui.set_width(inner_w - 24.0);
                        let card_w = ui.available_width();
                        // ── Profile ─────────────────────────────────────────────
                        ui.vertical_centered(|ui| {
                            let (av_rect, _) = ui.allocate_exact_size(Vec2::splat(68.0), Sense::hover());
                            paint_user_avatar(
                                ui.painter(),
                                av_rect.center(),
                                34.0,
                                &d_name,
                                d_is_group,
                                d_ava.as_deref(),
                                &self.sticker_textures,
                                &mut self.loading_stickers,
                            );
                            ui.add_space(8.0);
                            let can_alias = !d_is_group && !d_is_cloud && d_id != self.user_uin;
                            let name_w = ui.fonts(|f| {
                                f.layout_no_wrap(d_name.clone(), FontId::proportional(15.0), pal.text_primary)
                                    .size()
                                    .x
                            });
                            let row_w = (name_w + if can_alias { 26.0 } else { 0.0 } + 2.0)
                                .min(ui.available_width());
                            ui.allocate_ui_with_layout(
                                Vec2::new(row_w, 22.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    // Truncated with "…": a long group name widened the whole
                                    // panel past its card, clipping its right side.
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&d_name).size(15.0).strong().color(pal.text_primary),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(&d_name);
                                    if can_alias
                                        && ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(lucide::PENCIL).size(13.0).color(pal.text_secondary),
                                                )
                                                .frame(false),
                                            )
                                            .on_hover_text("Đặt tên gợi nhớ")
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                    {
                                        open_alias = true;
                                    }
                                },
                            );
                            let sub = if d_is_group {
                                match self.group_members.get(&d_id) {
                                    Some(m) if !m.is_empty() => format!("{} thành viên", m.len()),
                                    _ => "Nhóm".into(),
                                }
                            } else if d_is_cloud {
                                "Lưu trữ đám mây cá nhân".into()
                            } else {
                                match self.user_online_map.get(&d_id) {
                                    Some((_, lbl)) if !lbl.is_empty() => lbl.clone(),
                                    _ => String::new(),
                                }
                            };
                            if !sub.is_empty() {
                                ui.label(RichText::new(sub).size(12.0).color(pal.text_muted));
                            }
                            // Friendship lives here now, not in the chat header.
                            if !d_is_group && !d_is_cloud && d_id != self.user_uin
                                && self.friend_status_map.get(&d_id).copied()
                                    == Some(crate::chat::FriendshipStatus::Friend)
                            {
                                ui.add_space(4.0);
                                egui::Frame::none()
                                    .fill(pal.accent_online.linear_multiply(0.14))
                                    .rounding(Rounding::same(10.0))
                                    .inner_margin(Margin::symmetric(9.0, 3.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new("✓ Bạn bè · Đã kết bạn")
                                                .size(11.5)
                                                .color(pal.accent_online)
                                                .strong(),
                                        );
                                    });
                            }
                        });
                        ui.add_space(12.0);

                        // ── Quick actions (round icon + caption, like Zalo) ────────
                        let third: Option<(&str, &str)> = if d_is_group {
                            Some((lucide::USERS, "Thành viên"))
                        } else if !d_is_cloud {
                            Some((lucide::USER, "Trang cá nhân"))
                        } else {
                            None
                        };
                        let mut buttons: Vec<(&str, &str, bool, u8)> = vec![
                            (
                                if is_muted { "\u{e05a}" } else { lucide::BELL },
                                if is_muted { "Bật thông báo" } else { "Tắt thông báo" },
                                is_muted,
                                0,
                            ),
                            (
                                if is_pinned { "\u{e2b6}" } else { lucide::PIN },
                                if is_pinned { "Bỏ ghim hội thoại" } else { "Ghim hội thoại" },
                                is_pinned,
                                1,
                            ),
                        ];
                        if let Some((ic, lbl)) = third {
                            buttons.push((ic, lbl, false, 2));
                        }
                        let col_w = (card_w / buttons.len() as f32).floor();
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for (icon, label, active, which) in buttons {
                                let (cell, resp) =
                                    ui.allocate_exact_size(Vec2::new(col_w, 72.0), Sense::click());
                                let c = Pos2::new(cell.center().x, cell.top() + 20.0);
                                let hot = resp.hovered();
                                let fill = if active {
                                    pal.primary.linear_multiply(0.22)
                                } else if hot {
                                    pal.bg_hover
                                } else {
                                    pal.group_card()
                                };
                                ui.painter().circle_filled(c, 18.0, fill);
                                draw_lucide_icon(ui.painter(), c, icon, 16.0, if active { pal.primary } else { pal.text_primary });
                                let galley = ui.fonts(|f| {
                                    f.layout(
                                        label.to_string(),
                                        FontId::proportional(11.5),
                                        pal.text_secondary,
                                        col_w - 6.0,
                                    )
                                });
                                let gpos = Pos2::new(cell.center().x - galley.size().x / 2.0, c.y + 24.0);
                                ui.painter().galley(gpos, galley, pal.text_secondary);
                                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                    match which {
                                        0 => {
                                            action = Some(if is_muted {
                                                ThreadCtxAction::SetMute(
                                                    d_id.clone(),
                                                    d_is_group,
                                                    None,
                                                    "🔔 Đã bật lại thông báo".into(),
                                                )
                                            } else {
                                                ThreadCtxAction::SetMute(
                                                    d_id.clone(),
                                                    d_is_group,
                                                    Some(-1),
                                                    "🔕 Đã tắt thông báo".into(),
                                                )
                                            })
                                        }
                                        1 => {
                                            action = Some(ThreadCtxAction::TogglePin(
                                                d_id.clone(),
                                                d_is_group,
                                                !is_pinned,
                                            ))
                                        }
                                        _ => {
                                            if d_is_group {
                                                open_members = true;
                                            } else {
                                                open_profile = true;
                                            }
                                        }
                                    }
                                }
                            }
                        });
                        ui.add_space(10.0);

                        // ── Tin nhắn đã ghim ────────────────────────────────────
                        self.info_section(ui, pal, "pins", "Tin nhắn đã ghim", pins.len(), |_app, ui, show_all| {
                            if pins.is_empty() {
                                ui.label(RichText::new("Chưa có tin nhắn được ghim").size(12.0).color(pal.text_muted));
                                return;
                            }
                            let shown = if show_all { pins.len() } else { pins.len().min(LIST_PREVIEW) };
                            for (sender, text, msg_id) in &pins[..shown] {
                                let w = ui.available_width();
                                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 46.0), Sense::click());
                                if resp.hovered() {
                                    ui.painter().rect_filled(r.expand2(Vec2::new(6.0, 0.0)), Rounding::same(8.0), pal.bg_hover);
                                }
                                let badge = Rect::from_min_size(Pos2::new(r.left(), r.center().y - 17.0), Vec2::splat(34.0));
                                ui.painter().rect_filled(badge, Rounding::same(8.0), pal.primary.linear_multiply(0.16));
                                draw_lucide_icon(ui.painter(), badge.center(), lucide::PIN, 15.0, pal.primary);
                                let tx = badge.right() + 10.0;
                                let clip = ui.painter().with_clip_rect(Rect::from_min_max(Pos2::new(tx, r.top()), r.right_bottom()));
                                clip.text(
                                    Pos2::new(tx, r.top() + 14.0),
                                    egui::Align2::LEFT_CENTER,
                                    truncate_chars(sender, 30).into_owned(),
                                    FontId::proportional(12.0),
                                    pal.text_muted,
                                );
                                clip.text(
                                    Pos2::new(tx, r.top() + 32.0),
                                    egui::Align2::LEFT_CENTER,
                                    truncate_chars(&text.replace('\n', " "), 40).into_owned(),
                                    FontId::proportional(13.0),
                                    pal.text_primary,
                                );
                                if resp.on_hover_text("Xem tin nhắn").on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                    jump_to = Some(msg_id.clone());
                                }
                            }
                        }, pins.len() > LIST_PREVIEW);

                        // ── Ảnh/Video ───────────────────────────────────────────
                        self.info_section(ui, pal, "photos", "Ảnh/Video", photos.len(), |app, ui, show_all| {
                            if photos.is_empty() {
                                ui.label(RichText::new("Chưa có ảnh/video được chia sẻ").size(12.0).color(pal.text_muted));
                                return;
                            }
                            let gap = 6.0;
                            let cell = ((ui.available_width() - gap * 3.0) / 4.0).floor().max(40.0);
                            let shown = if show_all { photos.len() } else { photos.len().min(PHOTO_PREVIEW) };
                            for row in photos[..shown].chunks(4) {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = gap;
                                    for (tkey, thumb_u, full_u, ptitle) in row {
                                        if !app.sticker_textures.contains_key(tkey)
                                            && app.loading_stickers.insert(tkey.clone())
                                        {
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::FetchStickerImage {
                                                    key: tkey.clone(),
                                                    url: thumb_u.clone(),
                                                });
                                            }
                                        }
                                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(cell), Sense::click());
                                        ui.painter().rect_filled(r, Rounding::same(8.0), pal.bg_hover);
                                        if let Some(tex) = app.sticker_textures.get(tkey) {
                                            // Center-crop to a square.
                                            let sz = tex.size_vec2();
                                            let uv = if sz.x > sz.y {
                                                let w = sz.y / sz.x;
                                                Rect::from_min_max(Pos2::new((1.0 - w) / 2.0, 0.0), Pos2::new((1.0 + w) / 2.0, 1.0))
                                            } else {
                                                let h = sz.x / sz.y.max(1.0);
                                                Rect::from_min_max(Pos2::new(0.0, (1.0 - h) / 2.0), Pos2::new(1.0, (1.0 + h) / 2.0))
                                            };
                                            egui::Image::new(tex)
                                                .uv(uv)
                                                .rounding(Rounding::same(8.0))
                                                .paint_at(ui, r);
                                        } else {
                                            draw_icon_image(ui.painter(), r.center(), 18.0, pal.text_muted);
                                        }
                                        if resp.hovered() {
                                            ui.painter().rect_stroke(r, Rounding::same(8.0), Stroke::new(1.5_f32, pal.primary));
                                        }
                                        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                            app.image_viewer = Some(ImageViewerModal {
                                                tex_key: tkey.clone(),
                                                hd_tex_key: app.request_hd_texture(full_u),
                                                full_url: full_u.clone(),
                                                title: ptitle.clone(),
                                                sender: String::new(),
                                                time: String::new(),
                                                zoom: 1.0,
                                                pan: Vec2::ZERO,
                                            });
                                        }
                                    }
                                });
                            }
                        }, photos.len() > PHOTO_PREVIEW);

                        // ── File ────────────────────────────────────────────────
                        self.info_section(ui, pal, "files", "File", files.len(), |_app, ui, show_all| {
                            if files.is_empty() {
                                ui.label(RichText::new("Chưa có file được chia sẻ").size(12.0).color(pal.text_muted));
                                return;
                            }
                            let shown = if show_all { files.len() } else { files.len().min(LIST_PREVIEW) };
                            for f in &files[..shown] {
                                let w = ui.available_width();
                                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 50.0), Sense::click());
                                if resp.hovered() {
                                    ui.painter().rect_filled(r.expand2(Vec2::new(6.0, 0.0)), Rounding::same(8.0), pal.bg_hover);
                                }
                                let badge = Rect::from_min_size(Pos2::new(r.left(), r.center().y - 19.0), Vec2::splat(38.0));
                                paint_file_badge(ui.painter(), badge, &f.name, f.is_video);
                                let tx = badge.right() + 10.0;
                                let name = truncate_chars(&f.name, 34).into_owned();
                                ui.painter().with_clip_rect(Rect::from_min_max(Pos2::new(tx, r.top()), r.right_bottom())).text(
                                    Pos2::new(tx, r.top() + 15.0),
                                    egui::Align2::LEFT_CENTER,
                                    name,
                                    FontId::proportional(13.0),
                                    pal.text_primary,
                                );
                                ui.painter().text(
                                    Pos2::new(tx, r.top() + 35.0),
                                    egui::Align2::LEFT_CENTER,
                                    if f.size.is_empty() {
                                        if crate::gui::downloaded_path(&f.url).is_some() {
                                            "Đã có trên máy"
                                        } else if f.is_video {
                                            "Video"
                                        } else {
                                            "Tệp tin"
                                        }
                                    } else {
                                        f.size.as_str()
                                    },
                                    FontId::proportional(11.5),
                                    pal.text_muted,
                                );
                                ui.painter().text(
                                    Pos2::new(r.right(), r.top() + 35.0),
                                    egui::Align2::RIGHT_CENTER,
                                    &f.date,
                                    FontId::proportional(11.5),
                                    pal.text_muted,
                                );
                                if resp
                                    .on_hover_text(&f.name)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                    && !f.url.is_empty()
                                {
                                    if let Some(path) = crate::gui::downloaded_path(&f.url)
                                        .filter(|p| std::path::Path::new(p).exists())
                                    {
                                        super::workspace::open_local_path(&path, false);
                                    } else if f.is_video && !f.url.to_ascii_lowercase().contains("/renewmedia") {
                                        open_external_url(&f.url);
                                    } else {
                                        download = Some((f.url.clone(), f.name.clone()));
                                    }
                                }
                            }
                        }, files.len() > LIST_PREVIEW);

                        // ── Link ────────────────────────────────────────────────
                        self.info_section(ui, pal, "links", "Link", links.len(), |_app, ui, show_all| {
                            if links.is_empty() {
                                ui.label(RichText::new("Chưa có link được chia sẻ").size(12.0).color(pal.text_muted));
                                return;
                            }
                            let link_col = if pal.is_dark { Color32::from_rgb(97, 175, 239) } else { pal.primary };
                            let shown = if show_all { links.len() } else { links.len().min(LIST_PREVIEW) };
                            for l in &links[..shown] {
                                let w = ui.available_width();
                                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 50.0), Sense::click());
                                if resp.hovered() {
                                    ui.painter().rect_filled(r.expand2(Vec2::new(6.0, 0.0)), Rounding::same(8.0), pal.bg_hover);
                                }
                                let badge = Rect::from_min_size(Pos2::new(r.left(), r.center().y - 19.0), Vec2::splat(38.0));
                                ui.painter().rect_stroke(badge, Rounding::same(8.0), Stroke::new(1.0_f32, pal.border));
                                draw_lucide_icon(ui.painter(), badge.center(), lucide::LINK, 16.0, pal.text_secondary);
                                let tx = badge.right() + 10.0;
                                let clip = ui.painter().with_clip_rect(Rect::from_min_max(Pos2::new(tx, r.top()), r.right_bottom()));
                                clip.text(
                                    Pos2::new(tx, r.top() + 15.0),
                                    egui::Align2::LEFT_CENTER,
                                    truncate_chars(&l.title, 34).into_owned(),
                                    FontId::proportional(13.0),
                                    pal.text_primary,
                                );
                                clip.text(
                                    Pos2::new(tx, r.top() + 35.0),
                                    egui::Align2::LEFT_CENTER,
                                    &l.domain,
                                    FontId::proportional(11.5),
                                    link_col,
                                );
                                ui.painter().text(
                                    Pos2::new(r.right(), r.top() + 35.0),
                                    egui::Align2::RIGHT_CENTER,
                                    &l.date,
                                    FontId::proportional(11.5),
                                    pal.text_muted,
                                );
                                if resp.on_hover_text(&l.url).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                    open_external_url(&l.url);
                                }
                            }
                        }, links.len() > LIST_PREVIEW);

                        // ── Tuỳ chọn (existing tools: history sync, starred filter) ─
                        let starred = self.filter_starred_only;
                        self.info_section(ui, pal, "tools", "Tuỳ chọn", 0, |app, ui, _| {
                            if info_row(ui, pal, lucide::REFRESH, "Đồng bộ lịch sử tin nhắn", None, pal.text_primary) {
                                if let Some(tx) = WORKER_TX.get() {
                                    let _ = tx.send(WorkerCmd::RefreshThreads);
                                    let _ = tx.send(WorkerCmd::LoadGroupHistory {
                                        group_id: d_id.clone(),
                                        is_group: d_is_group,
                                        have: d_msg_count,
                                    });
                                }
                                app.show_toast("Đang đồng bộ lịch sử hội thoại…");
                            }
                            let mut on = starred;
                            if info_row(ui, pal, lucide::STAR, "Chỉ hiện tin đã đánh dấu", Some(&mut on), pal.text_primary) || on != starred {
                                app.filter_starred_only = !starred;
                            }
                        }, false);

                        // ── Thiết lập bảo mật ───────────────────────────────────
                        self.info_section(ui, pal, "privacy", "Thiết lập bảo mật", 0, |app, ui, _| {
                            let mut hidden = is_hidden;
                            if info_row(ui, pal, "\u{e0bb}", "Ẩn trò chuyện", Some(&mut hidden), pal.text_primary) || hidden != is_hidden {
                                action = Some(ThreadCtxAction::ToggleHide(d_id.clone(), d_is_group, !is_hidden));
                            }
                            if d_is_group {
                                let leaving = app.leaving_groups.contains(&d_id);
                                let label = if leaving { "Đang rời nhóm…" } else { "Rời nhóm" };
                                if info_row(ui, pal, lucide::LOG_OUT, label, None, pal.danger) && !leaving {
                                    action = Some(ThreadCtxAction::AskLeaveGroup(d_id.clone(), d_name.clone()));
                                }
                            }
                            if !app.drawer_confirm_delete {
                                if info_row(ui, pal, lucide::TRASH, "Xoá lịch sử trò chuyện", None, pal.danger) {
                                    app.drawer_confirm_delete = true;
                                }
                            } else {
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("Xoá toàn bộ cuộc trò chuyện này khỏi danh sách và máy này? Không thể hoàn tác.")
                                        .size(12.0)
                                        .color(pal.text_secondary),
                                );
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(RichText::new("Xoá").size(12.5).color(Color32::WHITE).strong())
                                                .fill(pal.danger)
                                                .rounding(Rounding::same(8.0))
                                                .min_size(Vec2::new(72.0, 28.0)),
                                        )
                                        .clicked()
                                    {
                                        app.drawer_confirm_delete = false;
                                        action = Some(ThreadCtxAction::DeleteConv(
                                            app.selected_thread_index,
                                            d_id.clone(),
                                            d_name.clone(),
                                            d_is_group,
                                            last_msg_id.clone(),
                                        ));
                                    }
                                    if ui
                                        .add(
                                            egui::Button::new(RichText::new("Huỷ").size(12.5).color(pal.text_secondary))
                                                .fill(pal.bg_hover)
                                                .rounding(Rounding::same(8.0))
                                                .min_size(Vec2::new(72.0, 28.0)),
                                        )
                                        .clicked()
                                    {
                                        app.drawer_confirm_delete = false;
                                    }
                                });
                            }
                        }, false);

                        // ── ID + count ──────────────────────────────────────────
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add_space(4.0);
                            ui.label(RichText::new(format!("{d_msg_count} tin nhắn đã tải")).size(11.5).color(pal.text_muted));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let short_id = {
                                    let n = d_id.chars().count();
                                    if n > 10 {
                                        format!(
                                            "{}…{}",
                                            d_id.chars().take(4).collect::<String>(),
                                            d_id.chars().skip(n - 4).collect::<String>()
                                        )
                                    } else {
                                        d_id.clone()
                                    }
                                };
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(format!("ID {short_id}  {}", lucide::COPY))
                                                .size(11.0)
                                                .color(pal.text_secondary),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_text(format!("Sao chép ID: {d_id}"))
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    ui.ctx().copy_text(d_id.clone());
                                    self.show_toast("Đã sao chép ID hội thoại vào bộ nhớ tạm");
                                }
                            });
                        });
                    });
            });

        if open_alias {
            let real = self.real_thread_name(&d_id, &d_name);
            self.open_alias_editor(d_id.clone(), real, d_ava.clone());
        }
        if open_members {
            self.members_panel_open = true;
        }
        if open_profile {
            self.feed_modal_open = true;
            self.feed_modal_tab = FeedModalTab::Profile;
            self.feed_target_uid = d_id.clone();
            self.feed_loading_profile = true;
            self.feed_profile_error = None;
            self.trigger_feed_timeline_load();
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::FetchSocialProfile {
                    target_uid: d_id.clone(),
                });
            }
        }
        if let Some((url, file_name)) = download {
            self.show_toast(format!("⏳ Đang tải tập tin \"{file_name}\"…"));
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::DownloadFile {
                    url,
                    file_name,
                    download_dir: self.settings.download_dir.clone(),
                });
            }
        }
        if let Some(reference) = jump_to {
            let found = !reference.is_empty()
                && self
                    .threads
                    .get(self.selected_thread_index)
                    .and_then(|t| {
                        crate::message_navigation::unique_reference_index(&t.messages, &reference)
                    })
                    .is_some();
            if found {
                self.filter_starred_only = false;
                self.in_chat_search_open = false;
                self.in_chat_search_query.clear();
                self.quote_jump = Some(QuoteJump {
                    thread_id: d_id.clone(),
                    reference,
                    highlight_until: std::time::Instant::now() + std::time::Duration::from_secs(3),
                    scroll_pending: true,
                });
                ui.ctx().request_repaint();
            } else {
                self.show_toast(
                    "Tin nhắn ghim chưa có trong lịch sử đã tải. Hãy tải thêm lịch sử rồi thử lại.",
                );
            }
        }
        if let Some(act) = action {
            self.apply_thread_ctx_action(ui.ctx(), act, now_unix);
        }
        close
    }

    /// One collapsible white card on the drawer's grey background: title + count + chevron,
    /// body, and an optional "Xem tất cả" toggle.
    #[allow(clippy::too_many_arguments)]
    fn info_section(
        &mut self,
        ui: &mut egui::Ui,
        pal: &ThemePalette,
        key: &str,
        title: &str,
        count: usize,
        body: impl FnOnce(&mut Self, &mut egui::Ui, bool),
        has_more: bool,
    ) {
        let collapsed = self.drawer_collapsed.contains(key);
        let show_all = self.drawer_show_all.contains(key);
        egui::Frame::none()
            .fill(pal.group_card())
            .rounding(Rounding::same(14.0))
            .inner_margin(Margin::symmetric(12.0, 10.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let (hr, hresp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
                ui.painter().text(
                    Pos2::new(hr.left(), hr.center().y),
                    egui::Align2::LEFT_CENTER,
                    title,
                    FontId::proportional(14.0),
                    pal.text_primary,
                );
                let chev_open =
                    ui.ctx()
                        .animate_bool_with_time(ui.id().with(("sec", key)), !collapsed, 0.16);
                let c = Pos2::new(hr.right() - 8.0, hr.center().y);
                let s = 4.5;
                // Chevron rotates from right (collapsed) to down (open).
                let ang = chev_open * std::f32::consts::FRAC_PI_2;
                let rot = |p: Vec2| {
                    Vec2::new(
                        p.x * ang.cos() - p.y * ang.sin(),
                        p.x * ang.sin() + p.y * ang.cos(),
                    )
                };
                let pts = [
                    c + rot(Vec2::new(-s * 0.5, -s)),
                    c + rot(Vec2::new(s * 0.5, 0.0)),
                    c + rot(Vec2::new(-s * 0.5, s)),
                ];
                ui.painter().add(egui::Shape::line(
                    pts.to_vec(),
                    Stroke::new(1.6_f32, pal.text_secondary),
                ));
                if count > 0 {
                    ui.painter().text(
                        Pos2::new(hr.right() - 22.0, hr.center().y),
                        egui::Align2::RIGHT_CENTER,
                        count.to_string(),
                        FontId::proportional(12.0),
                        pal.text_muted,
                    );
                }
                if hresp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    if collapsed {
                        self.drawer_collapsed.remove(key);
                    } else {
                        self.drawer_collapsed.insert(key.to_string());
                    }
                }
                if !collapsed {
                    ui.add_space(8.0);
                    body(self, ui, show_all);
                    if has_more {
                        ui.add_space(8.0);
                        if ui
                            .add_sized(
                                [ui.available_width(), 30.0],
                                egui::Button::new(
                                    RichText::new(if show_all {
                                        "Thu gọn"
                                    } else {
                                        "Xem tất cả"
                                    })
                                    .size(12.5)
                                    .color(pal.text_primary),
                                )
                                .fill(pal.bg_hover)
                                .rounding(Rounding::same(8.0)),
                            )
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            if show_all {
                                self.drawer_show_all.remove(key);
                            } else {
                                self.drawer_show_all.insert(key.to_string());
                            }
                        }
                    }
                }
            });
        ui.add_space(10.0);
    }
}

/// A tappable row (icon + label) with an optional trailing switch. Returns `true` on click.
fn info_row(
    ui: &mut egui::Ui,
    pal: &ThemePalette,
    icon: &str,
    label: &str,
    toggle: Option<&mut bool>,
    color: Color32,
) -> bool {
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 36.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(
            r.expand2(Vec2::new(6.0, 0.0)),
            Rounding::same(8.0),
            pal.bg_hover,
        );
    }
    draw_lucide_icon(
        ui.painter(),
        Pos2::new(r.left() + 10.0, r.center().y),
        icon,
        15.0,
        color,
    );
    ui.painter().text(
        Pos2::new(r.left() + 28.0, r.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        color,
    );
    let mut clicked = resp.on_hover_cursor(CursorIcon::PointingHand).clicked();
    if let Some(on) = toggle {
        let sw = Rect::from_center_size(
            Pos2::new(r.right() - 20.0, r.center().y),
            Vec2::new(36.0, 20.0),
        );
        let t = ui
            .ctx()
            .animate_bool_with_time(ui.id().with(("info_row", label)), *on, 0.16);
        let track = if *on { pal.primary } else { pal.bg_hover };
        ui.painter().rect_filled(sw, Rounding::same(10.0), track);
        let knob_x = egui::lerp((sw.left() + 10.0)..=(sw.right() - 10.0), t);
        ui.painter()
            .circle_filled(Pos2::new(knob_x, sw.center().y), 8.0, Color32::WHITE);
        if clicked {
            *on = !*on;
            clicked = false;
        }
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::size_from_title;

    #[test]
    fn size_from_title_reads_trailing_size_only() {
        assert_eq!(
            size_from_title(Some("Bai_thu_hoach.pdf (161.9 KB)")),
            "161.9 KB"
        );
        assert_eq!(size_from_title(Some("report (final).docx")), "");
        assert_eq!(size_from_title(None), "");
    }
}
