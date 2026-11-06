//! Show known reactor identities and disclose incomplete participant lists.
use super::*;
impl ZaloGuiApp {
    pub(super) fn render_reaction_people(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        let id = Id::new("reaction_people_message");
        let Some(message) = ctx.data(|d| d.get_temp::<ChatMessage>(id)) else {
            return;
        };
        if let Some(thread) = self
            .threads
            .get(self.selected_thread_index)
            .filter(|t| t.is_group)
        {
            if !self.group_members.contains_key(&thread.id) {
                self.group_members.insert(thread.id.clone(), Vec::new());
                if let Some(tx) = WORKER_TX.get() {
                    let _ = tx.send(WorkerCmd::FetchGroupMembers {
                        thread_id: thread.id.clone(),
                    });
                }
            }
        }
        let mut people = HashMap::new();
        people.insert(
            self.user_uin.clone(),
            ("Bạn".to_owned(), self.user_avatar.clone()),
        );
        for thread in &self.threads {
            if !thread.is_group {
                people.insert(
                    thread.id.clone(),
                    (thread.name.clone(), thread.avatar.clone()),
                );
            }
            for m in &thread.presentation.members {
                people.insert(m.id.clone(), (m.name.clone(), m.avatar.clone()));
            }
        }
        for m in self.group_members.values().flatten() {
            people.insert(m.id.clone(), (m.name.clone(), m.avatar.clone()));
        }
        let filter_id = id.with("filter");
        let mut filter = ctx
            .data(|d| d.get_temp::<String>(filter_id))
            .unwrap_or_default();
        let mut open = !ctx.input(|i| i.key_pressed(Key::Escape));

        // (uid, emoji, count) rows, ordered by emoji group then name.
        let mut rows: Vec<(String, String, u64)> = Vec::new();
        let mut incomplete = false;
        for r in &message.reaction_summaries {
            let mut ids = r.reactor_ids.clone();
            ids.extend(r.reactor_counts.keys().cloned());
            ids.sort();
            ids.dedup();
            if r.count_is_lower_bound
                || r.count > r.reactor_counts.values().sum::<u64>().max(ids.len() as u64)
            {
                incomplete = true;
            }
            for uid in ids {
                let n = r.reactor_counts.get(&uid).copied().unwrap_or(1);
                rows.push((uid, r.emoji.clone(), n));
            }
        }
        let total_people = {
            let mut u: Vec<&String> = rows.iter().map(|r| &r.0).collect();
            u.sort();
            u.dedup();
            u.len()
        };
        let total_count: u64 = message.reaction_summaries.iter().map(|r| r.count).sum();
        let display = |uid: &str| -> (String, Option<String>) {
            let (name, avatar) = people
                .get(uid)
                .cloned()
                .unwrap_or_else(|| (format!("Thành viên ({uid})"), None));
            (
                alias_for(uid).filter(|n| !n.is_empty()).unwrap_or(name),
                avatar,
            )
        };

        let screen = ctx.screen_rect();
        let fade = appear(ctx, id.with("fade"), 0.18);
        let backdrop = egui::Area::new(id.with("backdrop"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (r, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter().rect_filled(
                    r,
                    Rounding::same(if cfg!(target_os = "macos") {
                        crate::gui::WINDOW_RADIUS
                    } else {
                        0.0
                    }),
                    Color32::from_black_alpha((100.0 * fade) as u8),
                );
                resp.clicked()
            });
        if backdrop.inner {
            open = false;
        }
        let card_w = 400.0_f32.min(screen.width() - 32.0);
        let list_h = (screen.height() * 0.5).clamp(160.0, 380.0);
        let card_fill = if pal.is_dark {
            Color32::from_rgb(38, 40, 46)
        } else {
            Color32::from_rgb(250, 250, 252)
        };
        egui::Area::new(id.with("card"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::new(0.0, (1.0 - fade) * 16.0))
            .show(ctx, |ui| {
                ui.set_opacity(fade);
                egui::Frame::none()
                    .fill(card_fill)
                    .rounding(Rounding::same(20.0))
                    .shadow(ctx.style().visuals.window_shadow)
                    .inner_margin(Margin { left: 16.0, right: 16.0, top: 14.0, bottom: 12.0 })
                    .show(ui, |ui| {
                        ui.set_width(card_w - 32.0);
                        // Header: title, people count, round close button.
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.label(RichText::new("Biểu cảm").size(16.0).strong().color(pal.text_primary));
                                ui.label(
                                    RichText::new(format!("{total_people} người · {total_count} lượt"))
                                        .size(11.5)
                                        .color(pal.text_muted),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let (xr, xresp) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
                                ui.painter().circle_filled(
                                    xr.center(),
                                    15.0,
                                    if xresp.hovered() { pal.bg_hover } else { Color32::TRANSPARENT },
                                );
                                draw_lucide_icon(ui.painter(), xr.center(), lucide::X, 15.0, pal.text_secondary);
                                if xresp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                    open = false;
                                }
                            });
                        });
                        ui.add_space(10.0);
                        // Filter chips.
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                            if filter_chip(ui, pal, None, &format!("Tất cả {total_count}"), filter.is_empty()).clicked() {
                                filter.clear();
                            }
                            for r in &message.reaction_summaries {
                                if filter_chip(ui, pal, Some(&r.emoji), &r.count.to_string(), filter == r.emoji).clicked() {
                                    filter = r.emoji.clone();
                                }
                            }
                        });
                        ui.add_space(8.0);
                        egui::ScrollArea::vertical()
                            .max_height(list_h)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                let mut shown = 0;
                                for (uid, emoji, n) in &rows {
                                    if !filter.is_empty() && &filter != emoji {
                                        continue;
                                    }
                                    shown += 1;
                                    let (name, avatar) = display(uid);
                                    let (row, resp) = ui.allocate_exact_size(
                                        Vec2::new(ui.available_width(), 52.0),
                                        Sense::hover(),
                                    );
                                    if resp.hovered() {
                                        ui.painter().rect_filled(row, Rounding::same(12.0), pal.bg_hover);
                                    }
                                    paint_user_avatar(
                                        ui.painter(),
                                        row.left_center() + Vec2::new(26.0, 0.0),
                                        18.0,
                                        &name,
                                        false,
                                        avatar.as_deref(),
                                        &self.sticker_textures,
                                        &mut self.loading_stickers,
                                    );
                                    let me = uid == &self.user_uin;
                                    ui.painter().text(
                                        row.left_center() + Vec2::new(54.0, if me { 0.0 } else { -8.0 }),
                                        egui::Align2::LEFT_CENTER,
                                        truncate_chars(&name, 30),
                                        FontId::proportional(13.5),
                                        pal.text_primary,
                                    );
                                    if !me {
                                        ui.painter().text(
                                            row.left_center() + Vec2::new(54.0, 9.0),
                                            egui::Align2::LEFT_CENTER,
                                            if *n > 1 { format!("Đã thả {n} lần") } else { "Đã thả biểu cảm".to_string() },
                                            FontId::proportional(11.0),
                                            pal.text_muted,
                                        );
                                    }
                                    paint_reaction(ui, row.right_center() - Vec2::new(22.0, 0.0), emoji, 22.0);
                                }
                                if shown == 0 {
                                    ui.add_space(24.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new("Chưa có thông tin người thả biểu cảm trong dữ liệu đã tải.")
                                                .size(12.5)
                                                .color(pal.text_muted),
                                        );
                                    });
                                    ui.add_space(24.0);
                                }
                            });
                        if incomplete {
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(format!(
                                    "{}  Máy chủ chưa gửi đủ danh sách người thả biểu cảm.",
                                    lucide::INFO
                                ))
                                .size(11.0)
                                .color(pal.text_muted),
                            );
                        }
                    });
            });
        ctx.data_mut(|d| {
            d.insert_temp(filter_id, filter);
            if !open {
                d.remove::<ChatMessage>(id);
            }
        });
    }
}

/// Rounded filter chip for the reactions dialog: optional emoji plus a label; filled when active.
fn filter_chip(
    ui: &mut egui::Ui,
    pal: &ThemePalette,
    emoji: Option<&str>,
    label: &str,
    selected: bool,
) -> egui::Response {
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(
            label.to_string(),
            FontId::proportional(12.0),
            pal.text_primary,
        )
    });
    let w = galley.size().x + 24.0 + if emoji.is_some() { 22.0 } else { 0.0 };
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 30.0), Sense::click());
    let (fill, fg) = if selected {
        (
            pal.text_primary,
            if pal.is_dark {
                Color32::BLACK
            } else {
                Color32::WHITE
            },
        )
    } else if resp.hovered() {
        (pal.bg_hover, pal.text_primary)
    } else if pal.is_dark {
        (Color32::from_rgb(52, 54, 62), pal.text_secondary)
    } else {
        (Color32::from_rgb(236, 236, 241), pal.text_secondary)
    };
    ui.painter().rect_filled(r, Rounding::same(15.0), fill);
    let mut x = r.left() + 12.0;
    if let Some(e) = emoji {
        paint_reaction(ui, Pos2::new(x + 9.0, r.center().y), e, 17.0);
        x += 22.0;
    }
    ui.painter().text(
        Pos2::new(x, r.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.0),
        fg,
    );
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Paint an emoji through the same colour texture cache used by the composer.
fn paint_reaction(ui: &egui::Ui, center: Pos2, emoji: &str, size: f32) {
    if let Some(tex) = super::emoji_picker::emoji_texture_shared(ui.ctx(), emoji) {
        ui.painter().image(
            tex.id(),
            Rect::from_center_size(center, Vec2::splat(size)),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        ui.ctx().request_repaint();
    }
}
