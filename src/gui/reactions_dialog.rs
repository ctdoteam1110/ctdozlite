//! "Who reacted" dialog opened by clicking a message's reaction pill.
//!
//! The list is built only from the reaction ledger (`ReactionSummary::reactor_ids`), so every name
//! shown is a sender Zalo really reported. Counts that no known reactor explains (the part of a
//! history snapshot that has no per-user event) are listed as "Chưa rõ người thả" instead of
//! being attributed to anybody.

use super::*;

/// One person (or the unattributed remainder) in the dialog list.
struct ReactorRow {
    name: String,
    avatar: Option<String>,
    unknown: bool,
    /// (emoji, count) pairs, most used first.
    icons: Vec<(String, u64)>,
}

impl ZaloGuiApp {
    /// Resolve a reactor id to a display name and avatar from data already loaded in the app.
    fn reactor_identity(&self, thread_idx: usize, id: &str) -> (String, Option<String>) {
        if id == "0" || (!self.user_uin.is_empty() && id == self.user_uin) {
            let name = if self.user_name.is_empty() {
                "Bạn".to_string()
            } else {
                self.user_name.clone()
            };
            return (name, self.user_avatar.clone());
        }
        if let Some(thread) = self.threads.get(thread_idx) {
            let member = self
                .group_members_map
                .get(&thread.id)
                .into_iter()
                .flatten()
                .chain(thread.presentation.members.iter())
                .find(|m| m.id == id && !m.name.is_empty());
            if let Some(m) = member {
                return (m.name.clone(), m.avatar.clone());
            }
        }
        if let Some(t) = self.threads.iter().find(|t| !t.is_group && t.id == id) {
            return (t.name.clone(), t.avatar.clone());
        }
        ("Người dùng Zalo".to_string(), None)
    }

    pub(super) fn render_reactions_dialog(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        let Some((thread_id, msg_id)) = self.reactions_dialog.clone() else {
            return;
        };
        let Some(thread_idx) = self.threads.iter().position(|t| t.id == thread_id) else {
            self.reactions_dialog = None;
            return;
        };
        let Some(msg) = self.threads[thread_idx]
            .messages
            .iter()
            .find(|m| m.id == msg_id)
            .cloned()
        else {
            self.reactions_dialog = None;
            return;
        };

        let mut summaries: Vec<crate::reactions::ReactionSummary> = msg
            .reaction_summaries
            .iter()
            .filter(|s| s.count > 0)
            .cloned()
            .collect();
        summaries.sort_by(|a, b| b.count.cmp(&a.count));
        let total: u64 = summaries.iter().map(|s| s.count).sum();
        let tab = self
            .reactions_tab
            .filter(|kind| summaries.iter().any(|s| s.r_type == Some(*kind)));

        // Build rows first: names come from `self`, which the window closure borrows mutably.
        let mut rows: Vec<ReactorRow> = Vec::new();
        let mut index_by_id: HashMap<String, usize> = HashMap::new();
        let mut unknown_icons: Vec<(String, u64)> = Vec::new();
        for s in summaries
            .iter()
            .filter(|s| tab.is_none() || s.r_type == tab)
        {
            let mut known = 0u64;
            for id in &s.reactor_ids {
                let n = s.reactor_counts.get(id).copied().unwrap_or(1);
                known += n;
                let idx = *index_by_id.entry(id.clone()).or_insert_with(|| {
                    let (name, avatar) = self.reactor_identity(thread_idx, id);
                    rows.push(ReactorRow {
                        name,
                        avatar,
                        unknown: false,
                        icons: Vec::new(),
                    });
                    rows.len() - 1
                });
                rows[idx].icons.push((s.emoji.clone(), n));
            }
            if s.count > known {
                unknown_icons.push((s.emoji.clone(), s.count - known));
            }
        }
        if !unknown_icons.is_empty() {
            rows.push(ReactorRow {
                name: "Chưa rõ người thả".to_string(),
                avatar: None,
                unknown: true,
                icons: unknown_icons,
            });
        }

        let screen = ctx.screen_rect();
        let dlg_w = (screen.width() - 24.0).clamp(240.0, 340.0);
        let dlg_h = (screen.height() - 32.0).clamp(240.0, 380.0);
        let mut open = true;
        let mut next_tab = tab;
        egui::Window::new("Cảm xúc")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(dlg_w, dlg_h))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(format!("Tất cả {total}")).size(12.5).strong(),
                            )
                            .selected(tab.is_none()),
                        )
                        .clicked()
                    {
                        next_tab = None;
                    }
                    for s in &summaries {
                        let Some(kind) = s.r_type else { continue };
                        let label = RichText::new(s.count.to_string()).size(12.5).strong();
                        let tex_key = emoji_to_tex_key(&s.emoji);
                        let button = match (!tex_key.is_empty())
                            .then(|| self.emoji_textures.get(tex_key))
                            .flatten()
                        {
                            Some(tex) => egui::Button::image_and_text(
                                egui::Image::new(tex).fit_to_exact_size(Vec2::splat(16.0)),
                                label,
                            ),
                            None => egui::Button::new(
                                RichText::new(format!("{} {}", s.emoji, s.count)).size(12.5),
                            ),
                        };
                        if ui.add(button.selected(tab == Some(kind))).clicked() {
                            next_tab = Some(kind);
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if rows.is_empty() {
                            ui.label(
                                RichText::new("Chưa có dữ liệu cảm xúc.")
                                    .color(pal.text_muted)
                                    .size(12.5),
                            );
                        }
                        for row in &rows {
                            ui.horizontal(|ui| {
                                let (av_rect, _) =
                                    ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
                                if row.unknown {
                                    ui.painter().circle_filled(
                                        av_rect.center(),
                                        17.0,
                                        pal.bg_input,
                                    );
                                    ui.painter().text(
                                        av_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "?",
                                        FontId::proportional(15.0),
                                        pal.text_muted,
                                    );
                                } else {
                                    paint_user_avatar(
                                        ui.painter(),
                                        av_rect.center(),
                                        17.0,
                                        &row.name,
                                        false,
                                        row.avatar.as_deref(),
                                        &self.sticker_textures,
                                        &mut self.loading_stickers,
                                    );
                                }
                                ui.add_space(6.0);
                                ui.label(RichText::new(&row.name).size(13.5).color(
                                    if row.unknown {
                                        pal.text_muted
                                    } else {
                                        pal.text_primary
                                    },
                                ));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    for (emoji, n) in &row.icons {
                                        if *n > 1 {
                                            ui.label(
                                                RichText::new(n.to_string())
                                                    .size(12.0)
                                                    .color(pal.text_secondary),
                                            );
                                        }
                                        let tex_key = emoji_to_tex_key(emoji);
                                        match (!tex_key.is_empty())
                                            .then(|| self.emoji_textures.get(tex_key))
                                            .flatten()
                                        {
                                            Some(tex) => {
                                                ui.add(
                                                    egui::Image::new(tex)
                                                        .fit_to_exact_size(Vec2::splat(20.0)),
                                                );
                                            }
                                            None => {
                                                ui.label(RichText::new(emoji).size(16.0));
                                            }
                                        }
                                    }
                                });
                            });
                            ui.add_space(4.0);
                        }
                    });
            });
        self.reactions_tab = next_tab;
        if !open {
            self.reactions_dialog = None;
        }
    }
}
