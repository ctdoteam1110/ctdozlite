//! Group members panel and the Zalo-style "Thông tin tài khoản" (account info) card.

use super::*;

impl ZaloGuiApp {
    /// Open the account card for `uid` and request its profile from the worker.
    pub(super) fn open_account_card(&mut self, uid: String, name: String, avatar: Option<String>) {
        if uid.is_empty() {
            return;
        }
        if let Some(tx) = WORKER_TX.get() {
            let _ = tx.send(WorkerCmd::FetchAccountInfo { uid: uid.clone() });
        }
        self.account_card = Some(AccountCard {
            uid,
            name,
            avatar,
            loading: true,
            profile: None,
            error: None,
        });
    }

    /// Right-hand "Thành viên" panel for the selected group (must run before `CentralPanel`).
    pub(super) fn render_members_panel(
        &mut self,
        ctx: &egui::Context,
        thread_id: &str,
        pal: &ThemePalette,
        width: f32,
    ) {
        let members = self.group_members.get(thread_id).cloned();
        let mut close = false;
        let mut open: Option<(String, String, Option<String>)> = None;
        egui::SidePanel::right("group_members_panel")
            .show_separator_line(false)
            .resizable(false)
            .exact_width(width)
            .frame(
                egui::Frame::none()
                    .fill(pal.bg_sidebar)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .rounding(Rounding::same(if pal.is_glass { 16.0 } else { 0.0 }))
                    .outer_margin(Margin::same(if pal.is_glass { 6.0 } else { 0.0 }))
                    .inner_margin(Margin::symmetric(12.0, 12.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(RichText::new("‹").size(20.0)).frame(false))
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .clicked()
                    {
                        close = true;
                    }
                    ui.label(
                        RichText::new("Thành viên")
                            .size(16.0)
                            .strong()
                            .color(pal.text_primary),
                    );
                });
                ui.add_space(8.0);
                let Some(members) = members else {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Đang tải danh sách…").color(pal.text_muted));
                    });
                    return;
                };
                ui.label(
                    RichText::new(format!("Danh sách thành viên ({})", members.len()))
                        .size(13.0)
                        .strong()
                        .color(pal.text_secondary),
                );
                ui.add_space(6.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for m in &members {
                        let (rect, resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 52.0),
                            Sense::click(),
                        );
                        if resp.hovered() {
                            ui.painter()
                                .rect_filled(rect, Rounding::same(10.0), pal.bg_hover);
                        }
                        let c = rect.left_center() + Vec2::new(24.0, 0.0);
                        paint_user_avatar(
                            ui.painter(),
                            c,
                            18.0,
                            &m.name,
                            false,
                            m.avatar.as_deref(),
                            &self.sticker_textures,
                            &mut self.loading_stickers,
                        );
                        let is_me = m.id == self.user_uin;
                        let name = if is_me {
                            "Bạn".to_string()
                        } else {
                            m.name.clone()
                        };
                        let role = match m.role {
                            2 => Some("Trưởng nhóm"),
                            1 => Some("Phó nhóm"),
                            _ => None,
                        };
                        let text_x = rect.left() + 52.0;
                        let name_y = if role.is_some() {
                            rect.center().y - 9.0
                        } else {
                            rect.center().y
                        };
                        ui.painter().text(
                            Pos2::new(text_x, name_y),
                            egui::Align2::LEFT_CENTER,
                            truncate_chars(&name, 28),
                            FontId::proportional(13.0),
                            pal.text_primary,
                        );
                        if let Some(r) = role {
                            ui.painter().text(
                                Pos2::new(text_x, rect.center().y + 10.0),
                                egui::Align2::LEFT_CENTER,
                                r,
                                FontId::proportional(12.0),
                                pal.text_muted,
                            );
                        }
                        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                            open = Some((m.id.clone(), m.name.clone(), m.avatar.clone()));
                        }
                    }
                });
            });
        if close {
            self.members_panel_open = false;
        }
        if let Some((uid, name, ava)) = open {
            self.open_account_card(uid, name, ava);
        }
    }

    /// Centered "Thông tin tài khoản" card: cover, avatar, name, call/message, personal info.
    pub(super) fn render_account_card(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        let Some(card) = self.account_card.as_ref() else {
            return;
        };
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.account_card = None;
            return;
        }
        let uid = card.uid.clone();
        let prof = card.profile.clone();
        let loading = card.loading;
        let error = card.error.clone();
        let name = prof
            .as_ref()
            .map(|p| p.display_name.clone())
            .filter(|n| !n.is_empty() && *n != uid)
            .unwrap_or_else(|| card.name.clone());
        let avatar = prof
            .as_ref()
            .map(|p| p.avatar_url.clone())
            .filter(|a| !a.is_empty())
            .or_else(|| card.avatar.clone());
        let cover = prof
            .as_ref()
            .map(|p| p.cover_url.clone())
            .unwrap_or_default();
        let is_me = uid == self.user_uin;

        let screen = ctx.screen_rect();
        // Dim backdrop; a click outside the card closes it.
        let fade = appear(ctx, Id::new("account_card"), 0.18);
        let backdrop = egui::Area::new(Id::new("account_card_backdrop"))
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
                    Color32::from_black_alpha((110.0 * fade) as u8),
                );
                resp.clicked()
            });

        let mut close = backdrop.inner;
        let mut message = false;
        let mut call = false;
        let mut edit_alias = false;
        let mut open_photo: Option<String> = None;
        let mut open_group: Option<String> = None;

        // Data shown below the header, gathered up front.
        let alias = if is_me {
            None
        } else {
            crate::gui::alias_for(&uid)
        };
        let real_name = prof
            .as_ref()
            .map(|p| p.zalo_name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| name.clone());
        let common_groups: Vec<(String, String, Option<String>)> = if is_me {
            Vec::new()
        } else {
            self.threads
                .iter()
                .filter(|t| t.is_group)
                .filter(|t| {
                    t.presentation.members.iter().any(|m| m.id == uid)
                        || self
                            .group_members
                            .get(&t.id)
                            .is_some_and(|ms| ms.iter().any(|m| m.id == uid))
                })
                .map(|t| (t.id.clone(), t.name.clone(), t.avatar.clone()))
                .collect()
        };
        let shared_photos: Vec<String> = self
            .threads
            .iter()
            .find(|t| t.id == uid && !t.is_group)
            .map(|t| {
                t.messages
                    .iter()
                    .rev()
                    .filter(|m| super::messages::is_zalo_photo_message(m))
                    .filter_map(|m| super::messages::photo_primary_url(m).map(str::to_string))
                    .take(8)
                    .collect()
            })
            .unwrap_or_default();

        let card_w = 400.0_f32.min(screen.width() - 32.0);
        let card_max_h = (screen.height() - 48.0).max(320.0);
        let card_fill = if pal.is_dark {
            Color32::from_rgb(38, 40, 46)
        } else {
            Color32::from_rgb(250, 250, 252)
        };
        let section_fill = if pal.is_dark {
            Color32::from_rgb(48, 50, 58)
        } else {
            Color32::WHITE
        };
        egui::Area::new(Id::new("account_card"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::new(0.0, (1.0 - fade) * 16.0))
            .show(ctx, |ui| {
                ui.set_opacity(fade);
                egui::Frame::none()
                    .fill(card_fill)
                    .rounding(Rounding::same(20.0))
                    .shadow(ctx.style().visuals.window_shadow)
                    .show(ui, |ui| {
                        ui.set_width(card_w);
                        // Title row with a round close button.
                        egui::Frame::none()
                            .inner_margin(Margin { left: 18.0, right: 12.0, top: 12.0, bottom: 10.0 })
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Thông tin tài khoản")
                                            .size(15.0)
                                            .strong()
                                            .color(pal.text_primary),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let (xr, xresp) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
                                        ui.painter().circle_filled(
                                            xr.center(),
                                            15.0,
                                            if xresp.hovered() { pal.bg_hover } else { Color32::TRANSPARENT },
                                        );
                                        draw_lucide_icon(ui.painter(), xr.center(), lucide::X, 15.0, pal.text_secondary);
                                        if xresp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                            close = true;
                                        }
                                    });
                                });
                            });
                        egui::ScrollArea::vertical()
                            .max_height(card_max_h - 56.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                        ui.set_width(card_w);
                        // Cover with rounded corners, inset from the card edge.
                        let (band, _) = ui.allocate_exact_size(Vec2::new(card_w, 150.0), Sense::hover());
                        let cover_rect = band.shrink2(Vec2::new(12.0, 0.0));
                        let cover_round = Rounding::same(16.0);
                        if let Some(tex) = self.url_texture(&cover) {
                            let sz = tex.size_vec2();
                            let target = cover_rect.width() / cover_rect.height();
                            let src = sz.x / sz.y.max(1.0);
                            let uv = if src > target {
                                let w = target / src;
                                Rect::from_min_max(Pos2::new((1.0 - w) / 2.0, 0.0), Pos2::new((1.0 + w) / 2.0, 1.0))
                            } else {
                                let h = src / target;
                                Rect::from_min_max(Pos2::new(0.0, (1.0 - h) / 2.0), Pos2::new(1.0, (1.0 + h) / 2.0))
                            };
                            egui::Image::new(&tex).uv(uv).rounding(cover_round).paint_at(ui, cover_rect);
                        } else {
                            ui.painter().rect_filled(cover_rect, cover_round, pal.primary.linear_multiply(0.22));
                        }
                        // Avatar overlapping the cover's bottom edge, name beside it.
                        let av_c = Pos2::new(cover_rect.left() + 18.0 + 42.0, cover_rect.bottom());
                        ui.painter().circle_filled(av_c, 46.0, card_fill);
                        paint_user_avatar(
                            ui.painter(),
                            av_c,
                            42.0,
                            &name,
                            false,
                            avatar.as_deref(),
                            &self.sticker_textures,
                            &mut self.loading_stickers,
                        );
                        ui.horizontal(|ui| {
                            ui.add_space(12.0 + 18.0 + 84.0 + 12.0);
                            ui.vertical(|ui| {
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(alias.as_deref().unwrap_or(&name))
                                                .size(18.0)
                                                .strong()
                                                .color(pal.text_primary),
                                        )
                                        .truncate(),
                                    );
                                    if !is_me
                                        && ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(lucide::PENCIL).size(14.0).color(pal.text_secondary),
                                                )
                                                .frame(false),
                                            )
                                            .on_hover_text("Đặt tên gợi nhớ")
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                    {
                                        edit_alias = true;
                                    }
                                });
                                let mut sub: Vec<String> = Vec::new();
                                if alias.is_some() {
                                    sub.push(format!("Tên Zalo: {real_name}"));
                                }
                                if prof.as_ref().is_some_and(|p| p.is_friend) {
                                    sub.push("Bạn bè".into());
                                }
                                if let Some((_, lbl)) = self.user_online_map.get(&uid).filter(|(_, l)| !l.is_empty()) {
                                    sub.push(lbl.clone());
                                }
                                if !sub.is_empty() {
                                    ui.label(RichText::new(sub.join(" · ")).size(11.5).color(pal.text_muted));
                                }
                            });
                        });
                        ui.add_space(12.0);
                        // Actions
                        if !is_me {
                            ui.horizontal(|ui| {
                                ui.add_space(16.0);
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let bw = (card_w - 32.0 - 10.0) / 2.0;
                                let secondary = if pal.is_dark { Color32::from_rgb(56, 58, 66) } else { Color32::from_rgb(234, 234, 239) };
                                if ui
                                    .add_sized(
                                        [bw, 38.0],
                                        egui::Button::new(
                                            RichText::new(format!("{}  Gọi điện", lucide::PHONE)).size(13.5).color(pal.text_primary),
                                        )
                                        .fill(secondary)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(19.0)),
                                    )
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    call = true;
                                }
                                if ui
                                    .add_sized(
                                        [bw, 38.0],
                                        egui::Button::new(
                                            RichText::new(format!("{}  Nhắn tin", lucide::MESSAGE_SQUARE))
                                                .size(13.5)
                                                .strong()
                                                .color(if pal.is_dark { Color32::BLACK } else { Color32::WHITE }),
                                        )
                                        .fill(pal.text_primary)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(19.0)),
                                    )
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    message = true;
                                }
                            });
                            ui.add_space(12.0);
                        }
                        // Rounded section card helper.
                        let section = |ui: &mut egui::Ui, title: &str, body: &mut dyn FnMut(&mut egui::Ui)| {
                            egui::Frame::none()
                                .fill(section_fill)
                                .rounding(Rounding::same(16.0))
                                .outer_margin(Margin { left: 12.0, right: 12.0, top: 0.0, bottom: 10.0 })
                                .inner_margin(Margin::symmetric(16.0, 12.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new(title).size(13.5).strong().color(pal.text_primary));
                                    ui.add_space(8.0);
                                    body(ui);
                                });
                        };
                        section(ui, "Thông tin cá nhân", &mut |ui| {
                            if loading {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(RichText::new("Đang tải…").color(pal.text_muted));
                                });
                            } else if let Some(p) = &prof {
                                let gender = match p.gender {
                                    0 => "Nam",
                                    1 => "Nữ",
                                    _ => "Chưa cập nhật",
                                };
                                let mut rows: Vec<(&str, String)> = vec![("Giới tính", gender.into())];
                                rows.push((
                                    "Ngày sinh",
                                    if p.sdob.is_empty() { "Chưa chia sẻ".into() } else { format_vn_dob(&p.sdob) },
                                ));
                                rows.push((
                                    "Điện thoại",
                                    if p.phone.is_empty() { "Chưa chia sẻ".into() } else { p.phone.clone() },
                                ));
                                if !p.username.is_empty() && p.username != p.uid {
                                    rows.push(("Tên người dùng", format!("@{}", p.username)));
                                }
                                if !p.status.is_empty() {
                                    rows.push(("Giới thiệu", p.status.clone()));
                                }
                                egui::Grid::new("account_card_info")
                                    .num_columns(2)
                                    .spacing([20.0, 10.0])
                                    .show(ui, |ui| {
                                        for (k, v) in rows {
                                            ui.label(RichText::new(k).size(13.0).color(pal.text_muted));
                                            ui.add(egui::Label::new(RichText::new(v).size(13.0).color(pal.text_primary)).wrap());
                                            ui.end_row();
                                        }
                                    });
                            } else if let Some(e) = &error {
                                ui.label(
                                    RichText::new(format!("Không tải được thông tin: {e}"))
                                        .size(12.5)
                                        .color(pal.danger),
                                );
                            }
                        });
                        if !shared_photos.is_empty() {
                            section(ui, "Ảnh đã chia sẻ", &mut |ui| {
                                let gap = 6.0;
                                let side = ((ui.available_width() - gap * 3.0) / 4.0).floor();
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = Vec2::splat(gap);
                                    for url in &shared_photos {
                                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
                                        let key = format!("thumb_{}", crate::crypto::md5_hex(url));
                                        if let Some(tex) = self.sticker_textures.get(&key) {
                                            let s = tex.size_vec2();
                                            let ar = s.x / s.y.max(1.0);
                                            let uv = if ar > 1.0 {
                                                let k = 1.0 / ar;
                                                Rect::from_min_max(Pos2::new((1.0 - k) / 2.0, 0.0), Pos2::new((1.0 + k) / 2.0, 1.0))
                                            } else {
                                                Rect::from_min_max(Pos2::new(0.0, (1.0 - ar) / 2.0), Pos2::new(1.0, (1.0 + ar) / 2.0))
                                            };
                                            egui::Image::new(tex).uv(uv).rounding(Rounding::same(10.0)).paint_at(ui, r);
                                        } else {
                                            ui.painter().rect_filled(r, Rounding::same(10.0), pal.bg_hover);
                                            if self.loading_stickers.insert(key.clone()) {
                                                if let Some(tx) = WORKER_TX.get() {
                                                    let _ = tx.send(WorkerCmd::FetchStickerImage { key, url: url.clone() });
                                                }
                                            }
                                        }
                                        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                            open_photo = Some(url.clone());
                                        }
                                    }
                                });
                            });
                        }
                        if !is_me {
                            let title = format!("Nhóm chung ({})", common_groups.len());
                            section(ui, &title, &mut |ui| {
                                if common_groups.is_empty() {
                                    ui.label(
                                        RichText::new("Chưa thấy nhóm chung trong các nhóm đã tải thành viên.")
                                            .size(12.0)
                                            .color(pal.text_muted),
                                    );
                                }
                                for (gid, gname, gav) in common_groups.iter().take(8) {
                                    let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
                                    if resp.hovered() {
                                        ui.painter().rect_filled(row, Rounding::same(10.0), pal.bg_hover);
                                    }
                                    paint_user_avatar(
                                        ui.painter(),
                                        row.left_center() + Vec2::new(18.0, 0.0),
                                        14.0,
                                        gname,
                                        true,
                                        gav.as_deref(),
                                        &self.sticker_textures,
                                        &mut self.loading_stickers,
                                    );
                                    ui.painter().text(
                                        row.left_center() + Vec2::new(40.0, 0.0),
                                        egui::Align2::LEFT_CENTER,
                                        truncate_chars(gname, 36),
                                        FontId::proportional(13.0),
                                        pal.text_primary,
                                    );
                                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                        open_group = Some(gid.clone());
                                    }
                                }
                            });
                        }
                        ui.add_space(4.0);
                            });
                    });
            });

        if message {
            if let Some(idx) = self.threads.iter().position(|t| t.id == uid && !t.is_group) {
                self.selected_thread_index = idx;
                self.composer_focus_pending = true;
                close = true;
            } else {
                self.show_toast("Chưa có cuộc trò chuyện với người này trong danh sách");
            }
        }
        if call {
            self.active_call = Some(ActiveCallOverlay {
                partner_id: uid.clone(),
                partner_name: name.clone(),
                is_video: false,
                is_group: false,
                state_label: "Đang kết nối cuộc gọi thoại…".into(),
                started_at: std::time::Instant::now(),
                mic_muted: false,
                cam_enabled: false,
                is_incoming_ringing: false,
                error_code: None,
                error_reason: None,
                speaker_view: false,
                pinned_tile_idx: 0,
            });
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::StartCall {
                    partner_id: uid.clone(),
                    partner_name: name.clone(),
                    partner_avatar: avatar.clone(),
                    is_video: false,
                    is_group: false,
                });
            }
            close = true;
        }
        if let Some(url) = open_photo {
            let key = format!("thumb_{}", crate::crypto::md5_hex(&url));
            let hd_tex_key = self.request_hd_texture(&url);
            self.image_viewer = Some(crate::gui::ImageViewerModal {
                tex_key: key,
                hd_tex_key,
                full_url: url,
                title: "Ảnh".into(),
                sender: name.clone(),
                time: String::new(),
                zoom: 1.0,
                pan: Vec2::ZERO,
            });
            close = true;
        }
        if let Some(gid) = open_group {
            if let Some(idx) = self.threads.iter().position(|t| t.id == gid) {
                self.selected_thread_index = idx;
                close = true;
            }
        }
        if edit_alias {
            let real = self.real_thread_name(&uid, &name);
            self.open_alias_editor(uid.clone(), real, avatar.clone());
            close = true;
        }
        if close {
            self.account_card = None;
        }
    }

    pub(super) fn open_alias_editor(
        &mut self,
        uid: String,
        real_name: String,
        avatar: Option<String>,
    ) {
        let draft = crate::gui::alias_for(&uid).unwrap_or_default();
        self.alias_editor = Some(crate::gui::AliasEditor {
            uid,
            real_name,
            avatar,
            draft,
            saving: false,
        });
    }

    /// Zalo's "Đặt tên gợi nhớ" dialog: avatar, hint, one text field, Huỷ / Xác nhận.
    pub(super) fn render_alias_editor(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        let Some(ed) = self.alias_editor.as_ref() else {
            return;
        };
        if ctx.input(|i| i.key_pressed(Key::Escape)) && !ed.saving {
            self.alias_editor = None;
            return;
        }
        let real_name = ed.real_name.clone();
        let avatar = ed.avatar.clone();
        let saving = ed.saving;
        let had_alias = crate::gui::alias_for(&ed.uid).is_some();
        let screen = ctx.screen_rect();
        let fade = appear(ctx, Id::new("alias_editor"), 0.18);
        let backdrop = egui::Area::new(Id::new("alias_editor_backdrop"))
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
                    Color32::from_black_alpha((110.0 * fade) as u8),
                );
                resp.clicked()
            });
        let mut close = backdrop.inner && !saving;
        let mut submit = false;
        let card_w = 360.0_f32.min(screen.width() - 32.0);
        let mut draft = self
            .alias_editor
            .as_ref()
            .map(|e| e.draft.clone())
            .unwrap_or_default();
        egui::Area::new(Id::new("alias_editor_card"))
            .order(egui::Order::Tooltip)
            .anchor(
                egui::Align2::CENTER_CENTER,
                Vec2::new(0.0, (1.0 - fade) * 16.0),
            )
            .show(ctx, |ui| {
                ui.set_opacity(fade);
                egui::Frame::none()
                    .fill(pal.bg_elevated)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .rounding(Rounding::same(18.0))
                    .shadow(ctx.style().visuals.window_shadow)
                    .inner_margin(Margin::symmetric(18.0, 14.0))
                    .show(ui, |ui| {
                        ui.set_width(card_w - 36.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Đặt tên gợi nhớ")
                                    .size(14.0)
                                    .strong()
                                    .color(pal.text_primary),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(
                                        egui::Button::new(RichText::new(lucide::X).size(16.0))
                                            .frame(false),
                                    )
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                    && !saving
                                {
                                    close = true;
                                }
                            });
                        });
                        ui.add_space(12.0);
                        ui.vertical_centered(|ui| {
                            let (av_rect, _) =
                                ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                            paint_user_avatar(
                                ui.painter(),
                                av_rect.center(),
                                32.0,
                                &real_name,
                                false,
                                avatar.as_deref(),
                                &self.sticker_textures,
                                &mut self.loading_stickers,
                            );
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(format!(
                                    "Hãy đặt cho {} một cái tên dễ nhớ.",
                                    truncate_chars(&real_name, 28)
                                ))
                                .size(13.0)
                                .color(pal.text_primary),
                            );
                            ui.label(
                                RichText::new("Lưu ý: Tên gợi nhớ sẽ chỉ hiển thị riêng với bạn.")
                                    .size(12.0)
                                    .color(pal.text_muted),
                            );
                        });
                        ui.add_space(12.0);
                        let edit = ui.add_sized(
                            [ui.available_width(), 34.0],
                            egui::TextEdit::singleline(&mut draft)
                                .hint_text(real_name.as_str())
                                .char_limit(50)
                                .margin(Margin::symmetric(10.0, 8.0))
                                .interactive(!saving),
                        );
                        if fade < 1.0 && !edit.has_focus() {
                            edit.request_focus();
                        }
                        if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            submit = true;
                        }
                        ui.add_space(14.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let can_submit = !saving && (!draft.trim().is_empty() || had_alias);
                            let label = if saving {
                                "Đang lưu…"
                            } else {
                                "Xác nhận"
                            };
                            if ui
                                .add_enabled(
                                    can_submit,
                                    egui::Button::new(
                                        RichText::new(label)
                                            .size(13.0)
                                            .color(Color32::WHITE)
                                            .strong(),
                                    )
                                    .fill(pal.primary)
                                    .rounding(Rounding::same(10.0))
                                    .min_size(Vec2::new(96.0, 34.0)),
                                )
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                submit = true;
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Huỷ").size(13.0).color(pal.text_secondary),
                                    )
                                    .fill(pal.bg_input)
                                    .rounding(Rounding::same(10.0))
                                    .min_size(Vec2::new(80.0, 34.0)),
                                )
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                                && !saving
                            {
                                close = true;
                            }
                        });
                    });
            });
        if let Some(ed) = self.alias_editor.as_mut() {
            ed.draft = draft;
        }
        if close {
            self.alias_editor = None;
            return;
        }
        if submit && !saving {
            if let Some(ed) = self.alias_editor.as_mut() {
                let alias = ed.draft.trim().to_string();
                if alias.is_empty() && !had_alias {
                    return;
                }
                ed.saving = true;
                if let Some(tx) = WORKER_TX.get() {
                    let _ = tx.send(WorkerCmd::SetAlias {
                        friend_id: ed.uid.clone(),
                        alias,
                    });
                }
            }
        }
    }

    /// Texture for an arbitrary image URL via the shared avatar/sticker cache (`ava_<md5>` key),
    /// requesting a download on first use.
    pub(super) fn url_texture(&mut self, url: &str) -> Option<TextureHandle> {
        let url = url.trim();
        if url.is_empty() || !(url.starts_with("http://") || url.starts_with("https://")) {
            return None;
        }
        let base = url.split('?').next().unwrap_or(url);
        let key = format!("ava_{}", crate::crypto::md5_hex(base));
        if let Some(t) = self.sticker_textures.get(&key) {
            return Some(t.clone());
        }
        if self.loading_stickers.insert(key.clone()) {
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::FetchStickerImage {
                    key,
                    url: url.to_string(),
                });
            }
        }
        None
    }
}

/// `sdob` arrives as `dd/mm/yyyy` (or `yyyy-mm-dd`); show it the way Zalo does: "22 tháng 11, 2008".
fn format_vn_dob(sdob: &str) -> String {
    let parts: Vec<&str> = sdob.split(['/', '-']).collect();
    let (d, m, y) = match parts.as_slice() {
        [y, m, d] if y.len() == 4 => (*d, *m, *y),
        [d, m, y] => (*d, *m, *y),
        _ => return sdob.to_string(),
    };
    match (d.parse::<u32>(), m.parse::<u32>()) {
        (Ok(d), Ok(m)) if y.is_empty() || y == "0" => format!("{d} tháng {m}"),
        (Ok(d), Ok(m)) => format!("{d} tháng {m}, {y}"),
        _ => sdob.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::format_vn_dob;

    #[test]
    fn dob_formats_like_zalo() {
        assert_eq!(format_vn_dob("22/11/2008"), "22 tháng 11, 2008");
        assert_eq!(format_vn_dob("2008-11-22"), "22 tháng 11, 2008");
        assert_eq!(format_vn_dob("bad"), "bad");
    }
}
