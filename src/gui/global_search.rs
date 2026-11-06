//! Zalo-style search results for the chat-list search box: tabs (Tất cả / Liên hệ / Tin nhắn /
//! File), sections with highlighted matches, and "Xem tất cả …" to open a full tab.

use super::workspace::match_ranges;
use super::*;

/// What the user picked in the results.
pub(super) enum SearchPick {
    /// Open a conversation (thread index).
    Thread(usize),
    /// Open a conversation and jump to a message (thread index, message id).
    Message(usize, String),
}

struct MsgHit {
    thread: usize,
    msg_id: String,
    conv: String,
    sender: String,
    text: String,
    ts: i64,
    is_group: bool,
    avatar: Option<String>,
}

struct FileHit {
    thread: usize,
    msg_id: String,
    name: String,
    size: String,
    sender: String,
    ts: i64,
}

/// Layout `text` on one or two rows with every hit of `q` in `hi`.
#[allow(clippy::too_many_arguments)]
fn highlight_galley(
    ui: &egui::Ui,
    text: &str,
    q: &str,
    size: f32,
    base: Color32,
    hi: Color32,
    max_w: f32,
    rows: usize,
) -> std::sync::Arc<egui::Galley> {
    let ranges = match_ranges(text, q);
    // Start a little before the first hit so it is visible in long texts.
    let start = ranges
        .first()
        .map(|(a, _)| {
            let pre: Vec<(usize, char)> = text[..*a].char_indices().collect();
            pre.len().checked_sub(28).map(|k| pre[k].0).unwrap_or(0)
        })
        .unwrap_or(0);
    let fmt = |c: Color32| egui::TextFormat {
        font_id: FontId::proportional(size),
        color: c,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    if start > 0 {
        job.append("… ", 0.0, fmt(base));
    }
    let mut last = start;
    for (a, b) in ranges.iter().filter(|(a, _)| *a >= start) {
        job.append(&text[last..*a], 0.0, fmt(base));
        job.append(&text[*a..*b], 0.0, fmt(hi));
        last = *b;
    }
    job.append(&text[last..], 0.0, fmt(base));
    job.wrap = egui::text::TextWrapping {
        max_width: max_w,
        max_rows: rows,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    ui.fonts(|f| f.layout_job(job))
}

fn full_date(ts: i64) -> String {
    crate::clock::local_datetime(ts)
        .map(|d| d.format("%d/%m/%Y").to_string())
        .unwrap_or_default()
}

impl ZaloGuiApp {
    /// Render the results in place of the chat list. Returns the user's pick, if any.
    pub(super) fn render_global_search(
        &mut self,
        ui: &mut egui::Ui,
        pal: &ThemePalette,
        query: &str,
        folded: &str,
        filtered_threads: &[usize],
    ) -> Option<SearchPick> {
        let mut pick = None;
        let hi = if pal.is_dark {
            Color32::from_rgb(97, 175, 239)
        } else {
            Color32::from_rgb(0, 104, 255)
        };
        let now_ms = crate::login::unix_now() * 1000;
        let q = query.trim().to_lowercase();

        // ── Collect ──────────────────────────────────────────────────────────
        let mut msgs: Vec<MsgHit> = Vec::new();
        let mut files: Vec<FileHit> = Vec::new();
        if q.chars().count() >= 2 {
            for (ti, t) in self.threads.iter().enumerate() {
                for m in t.messages.iter().rev() {
                    let sender = if m.is_outgoing {
                        "Bạn".to_string()
                    } else if t.is_group {
                        crate::chat::sidebar_sender_id(m)
                            .and_then(|uid| crate::gui::alias_for(&uid))
                            .unwrap_or_else(|| m.sender.clone())
                    } else {
                        t.name.clone()
                    };
                    if is_zalo_file_or_video_message(m) {
                        let url = m.link_url.clone().unwrap_or_default();
                        let name = crate::chat::resolve_attachment_file_name(
                            m.link_title.as_deref(),
                            m.text.trim(),
                            &url,
                        );
                        if matches_vn_query(&name, &q, folded) && files.len() < 300 {
                            files.push(FileHit {
                                thread: ti,
                                msg_id: m.id.clone(),
                                name,
                                size: super::conv_info::size_from_title(m.link_title.as_deref()),
                                sender: sender.clone(),
                                ts: m.ts,
                            });
                        }
                        continue;
                    }
                    if is_sticker_message(m).is_some() || m.text.trim().is_empty() {
                        continue;
                    }
                    if matches_vn_query(&m.text, &q, folded) && msgs.len() < 300 {
                        msgs.push(MsgHit {
                            thread: ti,
                            msg_id: m.id.clone(),
                            conv: t.name.clone(),
                            sender,
                            text: m.text.replace('\n', " "),
                            ts: m.ts,
                            is_group: t.is_group,
                            avatar: t.avatar.clone(),
                        });
                    }
                }
            }
            msgs.sort_by_key(|m| std::cmp::Reverse(m.ts));
            files.sort_by_key(|f| std::cmp::Reverse(f.ts));
        }
        let contacts: Vec<usize> = filtered_threads.to_vec();

        // The shared sidebar card owns the background and rounded corners.

        // ── Tabs ─────────────────────────────────────────────────────────────
        let tab = self.global_search_tab;
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.spacing_mut().item_spacing.x = 14.0;
            for (i, label) in ["Tất cả", "Liên hệ", "Tin nhắn", "File"].iter().enumerate()
            {
                let active = tab == i as u8;
                let galley = ui.fonts(|f| {
                    f.layout_no_wrap(
                        label.to_string(),
                        FontId::proportional(12.5),
                        pal.text_primary,
                    )
                });
                let (r, resp) =
                    ui.allocate_exact_size(Vec2::new(galley.size().x, 24.0), Sense::click());
                let col = if active {
                    hi
                } else if resp.hovered() {
                    pal.text_primary
                } else {
                    pal.text_secondary
                };
                ui.painter().text(
                    r.center_top() + Vec2::new(0.0, 4.0),
                    egui::Align2::CENTER_TOP,
                    *label,
                    FontId::proportional(12.5),
                    col,
                );
                if active {
                    ui.painter().line_segment(
                        [
                            Pos2::new(r.left(), r.bottom() - 1.0),
                            Pos2::new(r.right(), r.bottom() - 1.0),
                        ],
                        Stroke::new(2.0_f32, hi),
                    );
                }
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    self.global_search_tab = i as u8;
                }
            }
        });
        ui.add(egui::Separator::default().spacing(2.0));
        let w = ui.available_width() - 8.0;

        let section = |ui: &mut egui::Ui, title: String| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(
                    RichText::new(title)
                        .size(12.5)
                        .strong()
                        .color(pal.text_primary),
                );
            });
            ui.add_space(2.0);
        };
        let see_all = |ui: &mut egui::Ui, label: &str| -> bool {
            ui.add_space(4.0);
            // Centred under the rows (it sat shifted to one side).
            let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 32.0), Sense::click());
            let btn = Rect::from_center_size(r.center(), Vec2::new(w - 16.0, 30.0));
            ui.painter().rect_filled(
                btn,
                Rounding::same(15.0),
                if resp.hovered() {
                    pal.bg_hover.gamma_multiply(1.4)
                } else {
                    pal.bg_hover
                },
            );
            ui.painter().text(
                btn.center(),
                egui::Align2::CENTER_CENTER,
                label,
                FontId::proportional(12.5),
                pal.text_primary,
            );
            ui.add_space(4.0);
            resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
        };

        // ── Liên hệ ──────────────────────────────────────────────────────────
        if tab == 0 || tab == 1 {
            let limit = if tab == 0 { 6 } else { 200 };
            section(ui, format!("Liên hệ ({})", contacts.len()));
            if contacts.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("Không có liên hệ phù hợp")
                            .size(12.0)
                            .color(pal.text_muted),
                    );
                });
            }
            for &ti in contacts.iter().take(limit) {
                let Some(t) = self.threads.get(ti) else {
                    continue;
                };
                let (name, is_group, ava) = (t.name.clone(), t.is_group, t.avatar.clone());
                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 48.0), Sense::click());
                if resp.hovered() {
                    ui.painter()
                        .rect_filled(r, Rounding::same(12.0), pal.bg_hover);
                }
                paint_user_avatar(
                    ui.painter(),
                    Pos2::new(r.left() + 26.0, r.center().y),
                    17.0,
                    &name,
                    is_group,
                    ava.as_deref(),
                    &self.sticker_textures,
                    &mut self.loading_stickers,
                );
                let g = highlight_galley(
                    ui,
                    &name,
                    &q,
                    13.5,
                    pal.text_primary,
                    hi,
                    r.right() - r.left() - 60.0,
                    1,
                );
                ui.painter().galley(
                    Pos2::new(r.left() + 52.0, r.center().y - g.size().y / 2.0),
                    g,
                    pal.text_primary,
                );
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    pick = Some(SearchPick::Thread(ti));
                }
            }
            if tab == 0 && contacts.len() > limit && see_all(ui, "Xem tất cả liên hệ") {
                self.global_search_tab = 1;
            }
        }

        // ── Tin nhắn ─────────────────────────────────────────────────────────
        if (tab == 0 || tab == 2) && q.chars().count() >= 2 {
            let limit = if tab == 0 { 3 } else { 150 };
            let count = if msgs.len() >= 300 {
                "300+".to_string()
            } else {
                msgs.len().to_string()
            };
            section(ui, format!("Tin nhắn ({count})"));
            if msgs.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("Không có tin nhắn đã tải nào khớp")
                            .size(12.0)
                            .color(pal.text_muted),
                    );
                });
            }
            for h in msgs.iter().take(limit) {
                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 62.0), Sense::click());
                if resp.hovered() {
                    ui.painter()
                        .rect_filled(r, Rounding::same(12.0), pal.bg_hover);
                }
                paint_user_avatar(
                    ui.painter(),
                    Pos2::new(r.left() + 26.0, r.center().y),
                    17.0,
                    &h.conv,
                    h.is_group,
                    h.avatar.as_deref(),
                    &self.sticker_textures,
                    &mut self.loading_stickers,
                );
                let tx = r.left() + 52.0;
                let clip = ui.painter().with_clip_rect(Rect::from_min_max(
                    Pos2::new(tx, r.top()),
                    Pos2::new(r.right() - 60.0, r.bottom()),
                ));
                clip.text(
                    Pos2::new(tx, r.top() + 14.0),
                    egui::Align2::LEFT_CENTER,
                    truncate_chars(&h.conv, 30).into_owned(),
                    FontId::proportional(12.0),
                    pal.text_muted,
                );
                ui.painter().text(
                    Pos2::new(r.right() - 8.0, r.top() + 14.0),
                    egui::Align2::RIGHT_CENTER,
                    crate::clock::sidebar_relative_at(h.ts, now_ms),
                    FontId::proportional(11.0),
                    pal.text_muted,
                );
                let line = format!("{}: {}", h.sender, h.text);
                let g = highlight_galley(
                    ui,
                    &line,
                    &q,
                    13.0,
                    pal.text_primary,
                    hi,
                    r.right() - 8.0 - tx,
                    2,
                );
                ui.painter()
                    .galley(Pos2::new(tx, r.top() + 24.0), g, pal.text_primary);
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    pick = Some(SearchPick::Message(h.thread, h.msg_id.clone()));
                }
            }
            if tab == 0 && msgs.len() > limit && see_all(ui, "Xem tất cả tin nhắn") {
                self.global_search_tab = 2;
            }
        }

        // ── File ─────────────────────────────────────────────────────────────
        if (tab == 0 || tab == 3) && q.chars().count() >= 2 {
            let limit = if tab == 0 { 3 } else { 150 };
            section(ui, format!("File ({})", files.len()));
            if files.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("Không có file đã tải nào khớp")
                            .size(12.0)
                            .color(pal.text_muted),
                    );
                });
            }
            for f in files.iter().take(limit) {
                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 54.0), Sense::click());
                if resp.hovered() {
                    ui.painter()
                        .rect_filled(r, Rounding::same(12.0), pal.bg_hover);
                }
                let badge = Rect::from_min_size(
                    Pos2::new(r.left() + 8.0, r.center().y - 18.0),
                    Vec2::splat(36.0),
                );
                super::conv_info::paint_file_badge(ui.painter(), badge, &f.name, false);
                let tx = badge.right() + 10.0;
                let g = highlight_galley(
                    ui,
                    &f.name,
                    &q,
                    13.0,
                    pal.text_primary,
                    hi,
                    r.right() - 8.0 - tx,
                    1,
                );
                ui.painter()
                    .galley(Pos2::new(tx, r.top() + 8.0), g, pal.text_primary);
                let meta = if f.size.is_empty() {
                    f.sender.clone()
                } else {
                    format!("{} - {}", f.size, f.sender)
                };
                let clip = ui.painter().with_clip_rect(Rect::from_min_max(
                    Pos2::new(tx, r.top()),
                    Pos2::new(r.right() - 76.0, r.bottom()),
                ));
                clip.text(
                    Pos2::new(tx, r.top() + 38.0),
                    egui::Align2::LEFT_CENTER,
                    meta,
                    FontId::proportional(11.5),
                    pal.text_muted,
                );
                ui.painter().text(
                    Pos2::new(r.right() - 8.0, r.top() + 38.0),
                    egui::Align2::RIGHT_CENTER,
                    full_date(f.ts),
                    FontId::proportional(11.0),
                    pal.text_muted,
                );
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    pick = Some(SearchPick::Message(f.thread, f.msg_id.clone()));
                }
            }
            if tab == 0 && files.len() > limit && see_all(ui, "Xem tất cả file") {
                self.global_search_tab = 3;
            }
        }
        if q.chars().count() < 2 && tab != 1 {
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(
                    RichText::new("Gõ ít nhất 2 ký tự để tìm tin nhắn và file")
                        .size(11.5)
                        .color(pal.text_muted),
                );
            });
        }
        ui.add_space(8.0);
        pick
    }
}
