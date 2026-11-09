use super::*;

impl ZaloGuiApp {
    /// Start loading the full-resolution texture the image viewer shows. Message bubbles and
    /// the media grid keep a 480 px copy; the `hd_` key keeps this one (and its disk cache) apart.
    pub(super) fn request_hd_texture(&mut self, full_url: &str) -> Option<String> {
        let lower_u = full_url.to_lowercase();
        let is_direct_img = lower_u.contains("zdn.vn")
            || lower_u.ends_with(".png")
            || lower_u.ends_with(".jpg")
            || lower_u.ends_with(".jpeg")
            || lower_u.ends_with(".webp")
            || lower_u.ends_with(".gif")
            || lower_u.ends_with(".bmp")
            || (!lower_u.starts_with("http://") && !lower_u.starts_with("https://"));
        if full_url.is_empty() || !is_direct_img {
            return None;
        }
        let hk = format!("hd_{}", crate::crypto::md5_hex(full_url));
        if !self.sticker_textures.contains_key(&hk) && self.loading_stickers.insert(hk.clone()) {
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::FetchStickerImage {
                    key: hk.clone(),
                    url: full_url.to_string(),
                });
            }
        }
        Some(hk)
    }

    pub(super) fn render_chat_workspace(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        self.render_reaction_people(ctx, pal);
        if let Some(thread) = self
            .threads
            .get(self.selected_thread_index)
            .filter(|t| t.is_group)
        {
            let key = Id::new("posting_requested_thread");
            if ctx.data(|d| d.get_temp::<String>(key)).as_deref() != Some(thread.id.as_str()) {
                ctx.data_mut(|d| {
                    d.insert_temp(key, thread.id.clone());
                    d.insert_temp(Id::new(("posting_ready", &thread.id)), false);
                });
                if let Some(tx) = WORKER_TX.get() {
                    let _ = tx.send(WorkerCmd::FetchGroupPosting {
                        thread_id: thread.id.clone(),
                    });
                }
            }
        }
        // Responsive breakpoints based on live viewport width
        // -------------------------------------------------------------------
        let screen_rect = ctx.screen_rect();
        let screen_w = screen_rect.width();
        let is_compact_w = false;
        let is_narrow_w = false;

        // -------------------------------------------------------------------
        // 1. LEFT NAVIGATION RAIL (Replaces top bar & bottom bar)
        // -------------------------------------------------------------------
        // Navigation moved to a bottom tab bar inside each left sidebar (`render_nav_tabbar`).

        if self.contacts_view_open {
            self.render_contacts_page(ctx, pal);
            self.render_profile_popover(ctx, pal);
            self.render_wipe_confirm(ctx, pal);
            self.render_alias_editor(ctx, pal);
            self.render_leave_group_confirm(ctx, pal);
            self.render_toast_overlay(ctx, pal);
            self.render_forward_dialog(ctx, pal);
            self.render_call_overlay(ctx, pal);
            self.render_image_viewer_modal(ctx, pal);
            self.render_phone_lookup_modal(ctx, pal);
            self.render_settings_modal(ctx, pal);
            self.render_schedule_modal(ctx, pal);
            return;
        }
        if self.feed_modal_open {
            self.render_feed_page(ctx, pal);
            self.render_profile_popover(ctx, pal);
            self.render_wipe_confirm(ctx, pal);
            self.render_alias_editor(ctx, pal);
            self.render_toast_overlay(ctx, pal);
            self.render_forward_dialog(ctx, pal);
            self.render_call_overlay(ctx, pal);
            self.render_image_viewer_modal(ctx, pal);
            self.render_phone_lookup_modal(ctx, pal);
            self.render_settings_modal(ctx, pal);
            return;
        }
        // -------------------------------------------------------------------
        // 2. CONVERSATION LIST SIDEBAR (Responsive width, auto-adapts on resize)
        // -------------------------------------------------------------------
        // ~20 % narrower list card (was 300).
        let sidebar_exact_w = 240.0_f32;
        let sidebar_id = "conversation_sidebar_fixed";
        pal.card_side_panel(
            sidebar_id,
            sidebar_exact_w + 28.0 + 2.0 * ThemePalette::CARD_INSET,
            Margin::ZERO,
        )
        .frame(pal.card(Color32::TRANSPARENT))
        .show(ctx, |ui| {
            let bounds = ui.max_rect();
            let fill = if pal.is_dark {
                pal.bg_elevated
            } else {
                Color32::WHITE
            };
            ui.painter().rect_filled(
                Rect::from_min_size(bounds.min, Vec2::new(bounds.width(), 36.0)),
                Rounding::same(18.0),
                fill,
            );
            ui.painter().rect_filled(
                Rect::from_min_max(bounds.min + Vec2::new(0.0, 44.0), bounds.max),
                Rounding::same(ThemePalette::CARD_RADIUS),
                fill,
            );
            // The whole top row drags the window (macOS title-bar behaviour); registered
            // first so the search box and settings button, added later, keep their clicks.
            {
                let top = ui.max_rect();
                let hdr = Rect::from_min_size(top.min, Vec2::new(top.width(), 4.0 + 28.0 + 4.0));
                let drag = ui.interact(
                    hdr,
                    Id::new("list_header_window_drag"),
                    Sense::click_and_drag(),
                );
                window_drag_from(ctx, &drag);
            }
            // Use the card's real inner width: forcing 300 px made rows overflow past the
            // card's rounded right edge (the translucent band over the list).
            let inner_w = ui.available_width();
            ui.set_min_width(inner_w);
            ui.set_max_width(inner_w);
            // Keep the separator directly below the padded row; inherited item spacing
            // otherwise adds an extra gap below the controls.
            let header_item_spacing = ui.spacing().item_spacing;
            ui.spacing_mut().item_spacing.y = 0.0;
            // Chat list: the search row shares the top row with the traffic lights.
            // 28 pt round buttons, centred in a header ~25 % lower than before (46 → 36).
            const ROW_H: f32 = 28.0;
            egui::Frame::none()
                .inner_margin(Margin {
                    left: if is_narrow_w { 4.0 } else { 14.0 },
                    right: if is_narrow_w { 4.0 } else { 14.0 },
                    top: 4.0,
                    bottom: 4.0,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().interact_size.y = ROW_H;
                    if is_narrow_w {
                        if cfg!(target_os = "macos") {
                            ui.add_space(CHROME_TOP_INSET - 12.0); // traffic lights row
                        }
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
                        let mut trigger_phone_lookup: Option<String> = None;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            if cfg!(target_os = "macos") {
                                // Room for the traffic lights drawn at the card's top-left.
                                ui.add_space(TRAFFIC_LIGHTS_ROW_W);
                            }
                            // Search: a round magnifier while idle; clicking grows it into a
                            // full-width field (eased), and it shrinks back once it is empty and
                            // loses focus.
                            let search_id = Id::new("sidebar_search_input");
                            let has_q = !self.search_query.is_empty();
                            let focused = ui.memory(|m| m.has_focus(search_id));
                            let want_open = self.search_open || has_q || focused;
                            // Green "update" button, only while a newer release exists and
                            // the search field is folded.
                            let update_ready =
                                self.update_info.as_ref().is_some_and(|i| i.has_update);
                            let show_update = update_ready && !want_open;
                            let free_w = (ui.available_width()
                                - ROW_H
                                - ui.spacing().item_spacing.x
                                - if show_update {
                                    ROW_H + ui.spacing().item_spacing.x
                                } else {
                                    0.0
                                })
                            .max(ROW_H);
                            let t = ui.ctx().animate_bool_with_time(
                                Id::new("sidebar_search_expand"),
                                want_open,
                                if reduce_motion() { 0.0 } else { 0.24 },
                            );
                            let ease = 1.0 - (1.0 - t).powi(3);
                            let search_box_w = egui::lerp(ROW_H..=free_w, ease);
                            ui.add_space((free_w - search_box_w).max(0.0));
                            if show_update {
                                let (r, resp) =
                                    ui.allocate_exact_size(Vec2::splat(ROW_H), Sense::click());
                                let hot = resp.hovered();
                                let green = Color32::from_rgb(34, 197, 94);
                                let c = r.center();
                                let rad = ROW_H / 2.0;
                                ui.painter().circle_filled(
                                    c + Vec2::new(0.0, 1.0),
                                    rad + 0.5,
                                    Color32::from_black_alpha(30),
                                );
                                ui.painter().circle_filled(
                                    c,
                                    rad,
                                    if hot {
                                        Color32::from_rgb(22, 163, 74)
                                    } else {
                                        green
                                    },
                                );
                                if self.update_downloading {
                                    // Progress ring while the release downloads.
                                    let frac = self
                                        .update_progress
                                        .filter(|(_, total)| *total > 0)
                                        .map(|(done, total)| {
                                            (done as f32 / total as f32).clamp(0.0, 1.0)
                                        })
                                        .unwrap_or_else(|| {
                                            (ui.input(|i| i.time) as f32 * 0.6) % 1.0
                                        });
                                    let n = 40;
                                    let pts: Vec<Pos2> = (0..=n)
                                        .map(|k| {
                                            let a = -std::f32::consts::FRAC_PI_2
                                                + std::f32::consts::TAU * frac * k as f32
                                                    / n as f32;
                                            c + Vec2::new(a.cos(), a.sin()) * (rad - 4.0)
                                        })
                                        .collect();
                                    ui.painter().add(egui::Shape::line(
                                        pts,
                                        Stroke::new(2.0_f32, Color32::WHITE),
                                    ));
                                    ui.ctx().request_repaint();
                                } else {
                                    draw_lucide_icon(
                                        ui.painter(),
                                        c,
                                        lucide::DOWNLOAD,
                                        15.0,
                                        Color32::WHITE,
                                    );
                                }
                                let tip = self
                                    .update_info
                                    .as_ref()
                                    .map(|i| format!("Cập nhật lên v{}", i.latest_version))
                                    .unwrap_or_default();
                                if resp
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text(tip)
                                    .clicked()
                                    && !self.update_downloading
                                {
                                    if let Some(info) = self.update_info.clone() {
                                        if let (Some(download_url), Some(asset_name)) =
                                            (info.download_url.clone(), info.asset_name.clone())
                                        {
                                            self.update_downloading = true;
                                            self.update_progress = Some((0, info.asset_size));
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ =
                                                    tx.send(WorkerCmd::DownloadAndApplyUpdate {
                                                        download_url,
                                                        asset_name,
                                                        target_version: info.latest_version.clone(),
                                                        expected_sha256: info
                                                            .expected_sha256
                                                            .clone(),
                                                    });
                                            }
                                        } else {
                                            // No build for this platform: show the release notes.
                                            self.update_modal_open = true;
                                        }
                                    }
                                }
                            }
                            let (sbox_rect, sbox_resp) = ui.allocate_exact_size(
                                Vec2::new(search_box_w, ROW_H),
                                Sense::click(),
                            );
                            if search_box_w < ROW_H + 40.0 {
                                // Collapsed: round icon button.
                                let hot = sbox_resp.hovered();
                                paint_round_header_button(ui.painter(), sbox_rect, hot, pal);
                                draw_lucide_icon(
                                    ui.painter(),
                                    sbox_rect.center(),
                                    lucide::SEARCH,
                                    16.0,
                                    if hot {
                                        pal.text_primary
                                    } else {
                                        pal.text_secondary
                                    },
                                );
                                if sbox_resp
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text("Tìm kiếm (⌘F)")
                                    .clicked()
                                    || ui.input(|i| i.modifiers.command && i.key_pressed(Key::F))
                                {
                                    self.search_open = true;
                                    self.search_focus_pending = true;
                                }
                            } else {
                                let rounding = Rounding::same(ROW_H / 2.0);
                                if focused {
                                    // Focus ring: soft outer halo + accent outline.
                                    ui.painter().rect_filled(
                                        sbox_rect.expand(3.0),
                                        Rounding::same(ROW_H / 2.0 + 3.0),
                                        pal.primary.linear_multiply(0.16),
                                    );
                                }
                                ui.painter()
                                    .rect_filled(sbox_rect, rounding, pal.bg_elevated);
                                ui.painter().rect_stroke(
                                    sbox_rect,
                                    rounding,
                                    Stroke::new(
                                        if focused { 1.5_f32 } else { 1.0_f32 },
                                        if focused { pal.primary } else { pal.border },
                                    ),
                                );
                                draw_icon_search(
                                    ui.painter(),
                                    Pos2::new(sbox_rect.left() + 15.0, sbox_rect.center().y),
                                    13.5,
                                    pal.text_primary,
                                );
                                let edit_rect = Rect::from_min_max(
                                    Pos2::new(sbox_rect.left() + 28.0, sbox_rect.top() + 3.0),
                                    Pos2::new(
                                        sbox_rect.right() - if has_q { 26.0 } else { 10.0 },
                                        sbox_rect.bottom() - 3.0,
                                    ),
                                );
                                let composing = self.ime_active;
                                // A detached child: allocating it in the row moved the row's
                                // cursor back inside the search box, so the settings button
                                // overlapped it.
                                let mut edit_ui = ui.new_child(
                                    egui::UiBuilder::new()
                                        .max_rect(edit_rect)
                                        .layout(Layout::left_to_right(Align::Center)),
                                );
                                edit_ui.set_clip_rect(edit_rect.intersect(ui.clip_rect()));
                                let s_resp = show_text_edit_ime(
                                    &mut edit_ui,
                                    egui::TextEdit::singleline(&mut self.search_query)
                                        .id(search_id)
                                        .frame(false)
                                        .hint_text("Tìm kiếm")
                                        .desired_width(edit_rect.width()),
                                    composing,
                                    pal.text_primary,
                                );
                                if self.search_focus_pending {
                                    s_resp.request_focus();
                                    self.search_focus_pending = false;
                                }
                                let is_ime_busy = self.ime_active || self.ime_just_committed;
                                if is_ime_busy && s_resp.lost_focus() {
                                    s_resp.request_focus();
                                }
                                let s_enter = (s_resp.lost_focus() || s_resp.has_focus())
                                    && ui.input(|i| i.key_pressed(Key::Enter))
                                    && !is_ime_busy;
                                let digits_cnt = self
                                    .search_query
                                    .chars()
                                    .filter(|c| c.is_ascii_digit())
                                    .count();
                                if s_enter && digits_cnt >= 8 {
                                    trigger_phone_lookup =
                                        Some(self.search_query.trim().to_string());
                                }
                                if s_resp.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
                                    self.search_query.clear();
                                    self.search_open = false;
                                    s_resp.surrender_focus();
                                } else if s_resp.lost_focus()
                                    && self.search_query.is_empty()
                                    && !is_ime_busy
                                {
                                    // Empty and unfocused: shrink back to the round button.
                                    self.search_open = false;
                                }
                                if has_q {
                                    let clr_rect = Rect::from_center_size(
                                        Pos2::new(sbox_rect.right() - 14.0, sbox_rect.center().y),
                                        Vec2::new(18.0, 18.0),
                                    );
                                    let clr_resp = ui.interact(
                                        clr_rect,
                                        Id::new("sidebar_search_clear_btn"),
                                        Sense::click(),
                                    );
                                    if clr_resp.hovered() {
                                        ui.painter().circle_filled(
                                            clr_rect.center(),
                                            8.5,
                                            pal.bg_hover,
                                        );
                                    }
                                    draw_lucide_icon(
                                        ui.painter(),
                                        clr_rect.center(),
                                        lucide::X,
                                        11.5,
                                        if clr_resp.hovered() {
                                            pal.text_primary
                                        } else {
                                            pal.text_muted
                                        },
                                    );
                                    if clr_resp
                                        .on_hover_cursor(CursorIcon::PointingHand)
                                        .on_hover_text("Xóa tìm kiếm")
                                        .clicked()
                                    {
                                        self.search_query.clear();
                                        self.search_focus_pending = true;
                                    }
                                }
                            }

                            {
                                let (r, resp) =
                                    ui.allocate_exact_size(Vec2::splat(ROW_H), Sense::click());
                                let hot = self.settings_modal_open || resp.hovered();
                                paint_round_header_button(ui.painter(), r, hot, pal);
                                draw_lucide_icon(
                                    ui.painter(),
                                    r.center(),
                                    lucide::SETTINGS,
                                    16.0,
                                    if hot {
                                        pal.text_primary
                                    } else {
                                        pal.text_secondary
                                    },
                                );
                                if resp
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text("Cài đặt")
                                    .clicked()
                                {
                                    self.settings_modal_open = !self.settings_modal_open;
                                }
                            }
                        });

                        if let Some(phone_str) = trigger_phone_lookup {
                            // Inline result in the list (no dialog).
                            self.phone_lookup_inline = Some(phone_str.clone());
                            self.phone_modal_input = phone_str.clone();
                            self.phone_lookup_loading = true;
                            self.phone_lookup_error = None;
                            self.phone_lookup_result = None;
                            if let Some(tx) = WORKER_TX.get() {
                                let _ = tx.send(WorkerCmd::LookupPhone { phone: phone_str });
                            }
                        }
                    }
                });

            ui.add_space(8.0);
            ui.spacing_mut().item_spacing = header_item_spacing;

            // Sort threads: "Cloud của tôi" pinned first, then descending by latest message timestamp
            let query = self.search_query.trim().to_lowercase();
            let query_folded = fold_vn_search(&query);
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
                // Zalo-style: look the number up automatically and show the account inline
                // ("Tìm bạn qua số điện thoại:") instead of a separate dialog.
                let raw_q = self.search_query.trim().to_string();
                if query_digits.len() >= 9
                    && self.phone_lookup_inline.as_deref() != Some(raw_q.as_str())
                {
                    self.phone_lookup_inline = Some(raw_q.clone());
                    self.phone_lookup_loading = true;
                    self.phone_lookup_error = None;
                    self.phone_lookup_result = None;
                    if let Some(tx) = WORKER_TX.get() {
                        let _ = tx.send(WorkerCmd::LookupPhone {
                            phone: raw_q.clone(),
                        });
                    }
                }
                let mut open_prof: Option<crate::chat::PhoneLookupResult> = None;
                let mut add_friend: Option<(String, String)> = None;
                egui::Frame::none()
                    .inner_margin(Margin::symmetric(12.0, 6.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Tìm bạn qua số điện thoại:")
                                .size(13.0)
                                .strong()
                                .color(pal.text_primary),
                        );
                        ui.add_space(4.0);
                        if query_digits.len() < 9 {
                            ui.label(
                                RichText::new("Nhập đủ số điện thoại để tìm")
                                    .size(12.0)
                                    .color(pal.text_muted),
                            );
                        } else if self.phone_lookup_loading {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(
                                    RichText::new("Đang tìm…").size(12.0).color(pal.text_muted),
                                );
                            });
                        } else if let Some(prof) = self.phone_lookup_result.clone() {
                            let (row, resp) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 60.0),
                                Sense::click(),
                            );
                            if resp.hovered() {
                                ui.painter()
                                    .rect_filled(row, Rounding::same(12.0), pal.bg_hover);
                            }
                            let c = row.left_center() + Vec2::new(28.0, 0.0);
                            paint_user_avatar(
                                ui.painter(),
                                c,
                                22.0,
                                &prof.display_name,
                                false,
                                (!prof.avatar.is_empty()).then_some(prof.avatar.as_str()),
                                &self.sticker_textures,
                                &mut self.loading_stickers,
                            );
                            ui.painter().text(
                                Pos2::new(row.left() + 60.0, row.center().y - 10.0),
                                egui::Align2::LEFT_CENTER,
                                truncate_chars(&prof.display_name, 24),
                                FontId::proportional(13.0),
                                pal.text_primary,
                            );
                            let label_g = ui.painter().layout_no_wrap(
                                "Số điện thoại: ".to_string(),
                                FontId::proportional(12.5),
                                pal.text_secondary,
                            );
                            let lw = label_g.size().x;
                            let lp = Pos2::new(row.left() + 60.0, row.center().y + 3.0);
                            ui.painter().galley(lp, label_g, pal.text_secondary);
                            ui.painter().text(
                                Pos2::new(lp.x + lw, lp.y),
                                egui::Align2::LEFT_TOP,
                                &raw_q,
                                FontId::proportional(12.5),
                                pal.primary,
                            );
                            if resp
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .on_hover_text("Mở cuộc trò chuyện")
                                .clicked()
                            {
                                open_prof = Some(prof.clone());
                            }
                            if !matches!(
                                prof.friendship,
                                crate::chat::FriendshipStatus::Friend
                                    | crate::chat::FriendshipStatus::OutgoingPending
                            ) && ui.small_button("Kết bạn").clicked()
                            {
                                add_friend =
                                    Some((prof.user_id.clone(), prof.display_name.clone()));
                            }
                        } else if let Some(err) = &self.phone_lookup_error {
                            ui.label(RichText::new(err).size(12.0).color(pal.text_muted));
                        }
                    });
                if let Some(prof) = open_prof {
                    self.open_chat_with_lookup(prof);
                }
                if let Some((uid, uname)) = add_friend {
                    if let Some(tx) = WORKER_TX.get() {
                        let _ = tx.send(WorkerCmd::AddFriend {
                            user_id: uid,
                            user_name: uname.clone(),
                        });
                    }
                    self.show_toast(format!("Đang gửi lời mời kết bạn tới {uname}…"));
                }
            } else if self.phone_lookup_inline.is_some() {
                self.phone_lookup_inline = None;
            }

            let search_outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut filtered_indices: Vec<usize> = self
                    .threads
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| {
                        if query.is_empty() && self.settings.hidden_threads.contains(&t.id) {
                            return false;
                        }
                        let not_hidden = !t.presentation.hidden || !query.is_empty();
                        not_hidden
                            && (query.is_empty()
                                || matches_vn_query(&t.name, &query, &query_folded)
                                || t.id.contains(&query))
                    })
                    .map(|(i, _)| i)
                    .collect();

                filtered_indices.sort_by(|&a, &b| {
                    let ta = &self.threads[a];
                    let tb = &self.threads[b];
                    let a_pinned = ta.name.contains("Cloud của tôi")
                        || self.settings.pinned_threads.contains(&ta.id);
                    let b_pinned = tb.name.contains("Cloud của tôi")
                        || self.settings.pinned_threads.contains(&tb.id);
                    if a_pinned != b_pinned {
                        return b_pinned.cmp(&a_pinned);
                    }
                    if ta.presentation.pinned != tb.presentation.pinned {
                        return tb.presentation.pinned.cmp(&ta.presentation.pinned);
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

                let msg_hits: Vec<(usize, String, String, String, bool)> =
                    if query.chars().count() >= 2 {
                        let mut hits = Vec::new();
                        for (t_idx, t) in self.threads.iter().enumerate() {
                            for m in t.messages.iter().rev() {
                                if matches_vn_query(&m.text, &query, &query_folded) {
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
                (filtered_indices, msg_hits)
            }));
            let (filtered_indices, msg_hits) = match search_outcome {
                Ok(pair) => pair,
                Err(_) => {
                    self.search_query.clear();
                    self.show_toast("⚠️ Đã đặt lại bộ lọc tìm kiếm");
                    ((0..self.threads.len()).collect(), Vec::new())
                }
            };

            let mut clicked_thread: Option<(usize, String, bool)> = None;
            let mut clicked_msg_hit: Option<(usize, String)> = None;
            let mut global_pick: Option<super::global_search::SearchPick> = None;
            if query.is_empty() {
                self.global_search_tab = 0;
            }
            let mut pending_ctx_action: Option<ThreadCtxAction> = None;
            let now_unix = crate::login::unix_now();
            // Real inner width of the list card (it is wider than the 300 px design value).
            let list_w = ui.available_width();
            let row_target_w = (list_w - 14.0).max(44.0);
            // Stop short of the card's rounded bottom: the clip is rectangular, so rows
            // scrolled into the corner zone were drawn outside the rounded card.
            ui.add_space(16.0);
            let list_clip = Rect::from_min_max(
                bounds.min + Vec2::new(4.0, 60.0),
                bounds.max - Vec2::new(4.0, 16.0),
            );
            let list_h = (ui.available_height() - 16.0).max(60.0);
            egui::ScrollArea::vertical()
                .id_salt("threads_list_scroll")
                .max_width(list_w)
                .max_height(list_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(list_clip));
                    ui.set_min_width(list_w - 6.0);
                    ui.set_max_width(list_w - 6.0);
                    // Zalo-style results (Liên hệ / Tin nhắn / File) while searching.
                    if !query.is_empty() && !is_phone_like_query {
                        global_pick = self.render_global_search(
                            ui,
                            pal,
                            &query,
                            &query_folded,
                            &filtered_indices,
                        );
                        return;
                    }
                    ui.add_space(4.0);
                    for &i in &filtered_indices {
                        let t = &self.threads[i];
                        let is_selected = i == self.selected_thread_index;
                        let is_user_pinned =
                            self.settings.pinned_threads.contains(&t.id) || t.presentation.pinned;
                        let is_pinned_cloud = t.name.contains("Cloud của tôi") || is_user_pinned;
                        let is_muted = self
                            .settings
                            .muted_threads
                            .get(&t.id)
                            .map(|&until| until == -1 || until > now_unix)
                            .unwrap_or(false)
                            || t.presentation.muted
                            || t.unread.is_muted();
                        let is_hidden =
                            self.settings.hidden_threads.contains(&t.id) || t.presentation.hidden;
                        let cur_label = self.settings.thread_labels.get(&t.id).cloned();
                        let row_height = if is_narrow_w { 50.0 } else { 58.0 };
                        let (alloc_rect, row_resp) = ui.allocate_exact_size(
                            Vec2::new(row_target_w, row_height),
                            Sense::click(),
                        );
                        let row_rect = alloc_rect.translate(Vec2::new(4.0, 0.0));
                        // Space is still allocated so the scrollbar is right, but off-screen rows
                        // skip painting, and with it the avatar download/texture for every thread.
                        if !ui.is_rect_visible(row_rect) {
                            continue;
                        }

                        // Hover background fades in/out instead of snapping.
                        let hover_t = ui.ctx().animate_bool_with_time(
                            row_resp.id.with("hover_fade"),
                            row_resp.hovered(),
                            if reduce_motion() { 0.0 } else { 0.14 },
                        );
                        let bg = if is_selected {
                            pal.bg_selected
                        } else {
                            pal.bg_hover.gamma_multiply(hover_t)
                        };

                        ui.painter().rect_filled(row_rect, Rounding::same(11.0), bg);

                        // Left active indicator bar (`w-1 bg-zalo-500 rounded-r-full`)
                        if is_selected {
                            let bar_rect = Rect::from_min_size(
                                Pos2::new(row_rect.left() + 1.0, row_rect.top() + 10.0),
                                Vec2::new(3.5, 38.0),
                            );
                            ui.painter()
                                .rect_filled(bar_rect, Rounding::same(2.0), pal.primary);
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
                        // Cutout status dot (`2.5px` ring matching row background) only when user is actively online
                        let is_user_online = !t.is_group
                            && self
                                .user_online_map
                                .get(&t.id)
                                .map(|(onl, _)| *onl)
                                .unwrap_or(false);
                        if is_user_online {
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
                            ui.painter()
                                .circle_filled(badge_pos, 4.0, pal.accent_online);
                        }
                        // Only show text labels when sidebar is expanded
                        if !is_narrow_w {
                            // Thread Name
                            let text_left = row_rect.left() + 58.0;
                            let name_color = if is_selected && pal.is_dark {
                                Color32::WHITE
                            } else {
                                pal.text_primary
                            };
                            let max_name_chars = if is_compact_w { 13 } else { 16 }; // narrower card
                            let name_x = if t.is_group {
                                draw_icon_contacts(
                                    ui.painter(),
                                    Pos2::new(text_left + 7.0, row_rect.top() + 19.5),
                                    12.5,
                                    pal.primary,
                                );
                                text_left + 18.0
                            } else {
                                text_left
                            };
                            let display_name = truncate_chars(&t.name, max_name_chars);
                            ui.painter().text(
                                Pos2::new(name_x, row_rect.top() + 12.0),
                                egui::Align2::LEFT_TOP,
                                display_name,
                                FontId::proportional(13.0),
                                name_color,
                            );

                            // Pin icon + Timestamp (top-right). Derived from the newest
                            // message instant so every row uses the same Zalo-style label
                            // (the stored text mixed "HH:MM", "HH:MM:SS" and stale days).
                            let time_right = row_rect.right() - 9.0;
                            let newest_ts = t
                                .messages
                                .last()
                                .map(|m| m.ts)
                                .unwrap_or(0)
                                .max(t.presentation.last_message_ts);
                            let row_time = if newest_ts > 0 {
                                crate::clock::sidebar_relative_at(newest_ts, now_unix * 1000)
                            } else {
                                t.time.clone()
                            };
                            if !row_time.is_empty() {
                                let tw = ui.fonts(|f| {
                                    f.layout_no_wrap(
                                        row_time.clone(),
                                        FontId::proportional(11.0),
                                        pal.text_muted,
                                    )
                                    .size()
                                    .x
                                });
                                ui.painter().text(
                                    Pos2::new(time_right, row_rect.top() + 13.5),
                                    egui::Align2::RIGHT_TOP,
                                    &row_time,
                                    FontId::proportional(11.0),
                                    pal.text_muted,
                                );
                                let mut icon_right = time_right - tw - 9.0;
                                if is_pinned_cloud {
                                    draw_icon_pin(
                                        ui.painter(),
                                        Pos2::new(icon_right, row_rect.top() + 19.5),
                                        11.0,
                                        pal.text_muted,
                                    );
                                    icon_right -= 14.0;
                                }
                                if is_muted {
                                    ui.painter().text(
                                        Pos2::new(icon_right + 4.0, row_rect.top() + 13.5),
                                        egui::Align2::RIGHT_TOP,
                                        "🔕",
                                        FontId::proportional(10.5),
                                        pal.text_muted,
                                    );
                                }
                            }
                            if let Some(ref lbl) = cur_label {
                                let tag_col = match lbl.as_str() {
                                    "Khách hàng" => Color32::from_rgb(239, 68, 68),
                                    "Gia đình" => Color32::from_rgb(34, 197, 94),
                                    "Công việc" => Color32::from_rgb(249, 115, 22),
                                    "Bạn bè" => Color32::from_rgb(168, 85, 247),
                                    "Trả lời sau" => Color32::from_rgb(234, 179, 8),
                                    _ => Color32::from_rgb(59, 130, 246),
                                };
                                ui.painter().circle_filled(
                                    Pos2::new(text_left - 8.0, row_rect.top() + 39.5),
                                    3.5,
                                    tag_col,
                                );
                            }

                            // Snippet with `Bạn:` prefix when latest message is outgoing
                            let last_outgoing =
                                t.messages.last().map(|m| m.is_outgoing).unwrap_or(false);
                            let raw_preview = if t.preview.is_empty() {
                                if t.is_group {
                                    "Nhóm trò chuyện".to_string()
                                } else {
                                    "Chưa có tin nhắn".to_string()
                                }
                            } else if last_outgoing && !t.preview.starts_with("Bạn:") {
                                format!("Bạn: {}", t.preview)
                            } else {
                                t.preview.clone()
                            };
                            let unread_n = self.unread_counts.get(&t.id).copied().unwrap_or(0);
                            let max_snip_chars = if is_compact_w {
                                14
                            } else if unread_n > 0 {
                                18
                            } else {
                                22
                            };
                            let snippet = truncate_chars(&raw_preview, max_snip_chars);
                            // Emoji as Apple Color Emoji images, like the chat: egui's own emoji
                            // font lacks newer ones (🥱 drew as a box here).
                            let snip_col = if unread_n > 0 {
                                pal.text_primary
                            } else {
                                pal.text_secondary
                            };
                            let mut snip_job = egui::text::LayoutJob::default();
                            super::emoji_picker::append_text_with_emoji_sized(
                                &mut snip_job,
                                ui.ctx(),
                                &snippet,
                                FontId::proportional(12.0),
                                snip_col,
                                15.0,
                            );
                            let snip_galley = ui.fonts(|f| f.layout_job(snip_job));
                            let snip_pos = Pos2::new(
                                text_left,
                                row_rect.top()
                                    + 33.5
                                    + (15.0 - snip_galley.size().y).min(0.0) * 0.5,
                            );
                            ui.painter().galley(snip_pos, snip_galley.clone(), snip_col);
                            super::emoji_picker::paint_emoji_overlay(
                                ui.painter(),
                                ui.ctx(),
                                &snip_galley,
                                snip_pos,
                            );
                            if unread_n > 0 {
                                let badge_txt = if unread_n > 99 {
                                    "99+".to_string()
                                } else {
                                    unread_n.to_string()
                                };
                                let bw = if unread_n >= 10 { 22.0 } else { 18.0 };
                                let b_rect = Rect::from_min_size(
                                    Pos2::new(row_rect.right() - bw - 8.0, row_rect.top() + 31.5),
                                    Vec2::new(bw, 17.0),
                                );
                                ui.painter()
                                    .rect_filled(b_rect, Rounding::same(8.5), pal.danger);
                                ui.painter().text(
                                    b_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    badge_txt,
                                    FontId::proportional(10.2),
                                    Color32::WHITE,
                                );
                            }
                        }

                        let row_resp = row_resp.on_hover_cursor(CursorIcon::PointingHand);
                        let row_resp = if is_narrow_w {
                            row_resp.on_hover_text(&t.name)
                        } else {
                            row_resp
                        };
                        let tid_cl = t.id.clone();
                        let tname_cl = t.name.clone();
                        let tgroup_cl = t.is_group;
                        let unread_now = self.unread_counts.get(&t.id).copied().unwrap_or(0) > 0;
                        let last_msg_id_cl =
                            t.messages.last().map(|m| m.id.clone()).unwrap_or_default();
                        row_resp.context_menu(|ui| {
                            // One row style for every item (icon + label, full width): the
                            // old egui submenus were indented differently from plain items.
                            // "Phân loại" and "Tắt thông báo" expand inline.
                            ui.set_min_width(250.0);
                            ui.spacing_mut().item_spacing = Vec2::new(0.0, 1.0);
                            let exp_key = Id::new(("thread_menu_expand", &tid_cl));
                            let mut expanded: u8 =
                                ui.ctx().data(|d| d.get_temp(exp_key)).unwrap_or(0);
                            let fg = pal.text_primary;

                            if thread_menu_row(
                                ui,
                                if is_user_pinned {
                                    "\u{e2b6}"
                                } else {
                                    "\u{e259}"
                                },
                                if is_user_pinned {
                                    "Bỏ ghim hội thoại"
                                } else {
                                    "Ghim hội thoại"
                                },
                                fg,
                                None,
                            )
                            .clicked()
                            {
                                pending_ctx_action = Some(ThreadCtxAction::TogglePin(
                                    tid_cl.clone(),
                                    tgroup_cl,
                                    !is_user_pinned,
                                ));
                                ui.close_menu();
                            }

                            if thread_menu_row(ui, "\u{e17f}", "Phân loại", fg, Some(expanded == 1))
                                .clicked()
                            {
                                expanded = if expanded == 1 { 0 } else { 1 };
                            }
                            if expanded == 1 {
                                let categories = [
                                    ("Khách hàng", Color32::from_rgb(239, 68, 68)),
                                    ("Gia đình", Color32::from_rgb(34, 197, 94)),
                                    ("Công việc", Color32::from_rgb(249, 115, 22)),
                                    ("Bạn bè", Color32::from_rgb(168, 85, 247)),
                                    ("Trả lời sau", Color32::from_rgb(234, 179, 8)),
                                    ("Đồng nghiệp", Color32::from_rgb(59, 130, 246)),
                                ];
                                for (cat_name, dot_col) in categories {
                                    let is_cur = cur_label.as_deref() == Some(cat_name);
                                    if thread_menu_sub_row(ui, Some(dot_col), cat_name, is_cur, fg)
                                        .clicked()
                                    {
                                        pending_ctx_action = Some(ThreadCtxAction::SetLabel(
                                            tid_cl.clone(),
                                            if is_cur {
                                                None
                                            } else {
                                                Some(cat_name.to_string())
                                            },
                                        ));
                                        ui.close_menu();
                                    }
                                }
                                if cur_label.is_some()
                                    && thread_menu_sub_row(
                                        ui,
                                        None,
                                        "Bỏ phân loại",
                                        false,
                                        pal.text_secondary,
                                    )
                                    .clicked()
                                {
                                    pending_ctx_action =
                                        Some(ThreadCtxAction::SetLabel(tid_cl.clone(), None));
                                    ui.close_menu();
                                }
                            }

                            if thread_menu_row(
                                ui,
                                if unread_now { "\u{e363}" } else { "\u{e10f}" },
                                if unread_now {
                                    "Đánh dấu đã đọc"
                                } else {
                                    "Đánh dấu chưa đọc"
                                },
                                fg,
                                None,
                            )
                            .clicked()
                            {
                                pending_ctx_action = Some(ThreadCtxAction::ToggleUnread(
                                    tid_cl.clone(),
                                    tgroup_cl,
                                    !unread_now,
                                ));
                                ui.close_menu();
                            }

                            ui.add_space(3.0);
                            ui.separator();
                            ui.add_space(3.0);

                            if is_muted {
                                if thread_menu_row(ui, lucide::BELL, "Mở lại thông báo", fg, None)
                                    .clicked()
                                {
                                    pending_ctx_action = Some(ThreadCtxAction::SetMute(
                                        tid_cl.clone(),
                                        tgroup_cl,
                                        None,
                                        "Đã mở lại thông báo hội thoại".into(),
                                    ));
                                    ui.close_menu();
                                }
                            } else {
                                if thread_menu_row(
                                    ui,
                                    "\u{e05a}",
                                    "Tắt thông báo",
                                    fg,
                                    Some(expanded == 2),
                                )
                                .clicked()
                                {
                                    expanded = if expanded == 2 { 0 } else { 2 };
                                }
                                if expanded == 2 {
                                    let rem_today = 86_400 - (now_unix % 86_400);
                                    let dur_8am = (rem_today + 8 * 3600) % 86_400;
                                    let dur_8am = if dur_8am < 600 {
                                        dur_8am + 86_400
                                    } else {
                                        dur_8am
                                    };
                                    let opts: [(&str, i64); 4] = [
                                        ("Trong 1 giờ", 3600),
                                        ("Trong 4 giờ", 14_400),
                                        ("Cho đến 8:00 AM", dur_8am),
                                        ("Cho đến khi được mở lại", -1),
                                    ];
                                    for (opt_label, dur_s) in opts {
                                        if thread_menu_sub_row(ui, None, opt_label, false, fg)
                                            .clicked()
                                        {
                                            pending_ctx_action = Some(ThreadCtxAction::SetMute(
                                                tid_cl.clone(),
                                                tgroup_cl,
                                                Some(dur_s),
                                                format!("🔕 Đã tắt thông báo ({opt_label})"),
                                            ));
                                            ui.close_menu();
                                        }
                                    }
                                }
                            }

                            if thread_menu_row(
                                ui,
                                if is_hidden { "\u{e0ba}" } else { "\u{e0bb}" },
                                if is_hidden {
                                    "Bỏ ẩn trò chuyện"
                                } else {
                                    "Ẩn trò chuyện"
                                },
                                fg,
                                None,
                            )
                            .clicked()
                            {
                                pending_ctx_action = Some(ThreadCtxAction::ToggleHide(
                                    tid_cl.clone(),
                                    tgroup_cl,
                                    !is_hidden,
                                ));
                                ui.close_menu();
                            }

                            if thread_menu_row(ui, "\u{e304}", "Hẹn giờ gửi / Nhắc hẹn", fg, None)
                                .clicked()
                            {
                                pending_ctx_action = Some(ThreadCtxAction::OpenSchedule(i));
                                ui.close_menu();
                            }

                            ui.add_space(3.0);
                            ui.separator();
                            ui.add_space(3.0);

                            if thread_menu_row(ui, lucide::TRASH, "Xóa hội thoại", pal.danger, None)
                                .clicked()
                            {
                                pending_ctx_action = Some(ThreadCtxAction::DeleteConv(
                                    i,
                                    tid_cl.clone(),
                                    tname_cl.clone(),
                                    tgroup_cl,
                                    last_msg_id_cl.clone(),
                                ));
                                ui.close_menu();
                            }
                            if tgroup_cl
                                && thread_menu_row(
                                    ui,
                                    lucide::LOG_OUT,
                                    "Rời nhóm",
                                    pal.danger,
                                    None,
                                )
                                .clicked()
                            {
                                pending_ctx_action = Some(ThreadCtxAction::AskLeaveGroup(
                                    tid_cl.clone(),
                                    tname_cl.clone(),
                                ));
                                ui.close_menu();
                            }
                            ui.ctx().data_mut(|d| d.insert_temp(exp_key, expanded));
                        });

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
                                RichText::new(format!("TIN NHẮN TÌM THẤY ({})", msg_hits.len()))
                                    .size(11.0)
                                    .color(pal.primary_hover)
                                    .strong(),
                            );
                        });
                        ui.add_space(4.0);
                        for (t_idx, t_name, m_text, m_time, t_group) in &msg_hits {
                            let (alloc_r, r_resp) = ui
                                .allocate_exact_size(Vec2::new(row_target_w, 52.0), Sense::click());
                            let r_rect = alloc_r.translate(Vec2::new(4.0, 0.0));
                            if r_resp.hovered() {
                                ui.painter().rect_filled(
                                    r_rect,
                                    Rounding::same(10.0),
                                    pal.bg_hover,
                                );
                            }
                            let av_c = Pos2::new(r_rect.left() + 24.0, r_rect.center().y);
                            let hit_ava =
                                self.threads.get(*t_idx).and_then(|th| th.avatar.as_deref());
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

                    if !query.is_empty() && filtered_indices.is_empty() && msg_hits.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(32.0);
                            ui.label(
                                RichText::new("Không tìm thấy hội thoại hoặc tin nhắn phù hợp.")
                                    .size(12.5)
                                    .color(pal.text_muted),
                            );
                        });
                    }
                });

            if let Some((idx, q_str)) = clicked_msg_hit {
                self.selected_thread_index = idx;
                self.in_chat_search_open = true;
                self.in_chat_search_focus_pending = true;
                self.in_chat_search_query = q_str;
            }
            let mut pick_jump: Option<(String, String)> = None;
            match global_pick.take() {
                Some(super::global_search::SearchPick::Thread(ti)) => {
                    if let Some(t) = self.threads.get(ti) {
                        clicked_thread = Some((ti, t.id.clone(), t.is_group));
                    }
                }
                Some(super::global_search::SearchPick::Message(ti, mid)) => {
                    if let Some(t) = self.threads.get(ti) {
                        clicked_thread = Some((ti, t.id.clone(), t.is_group));
                        pick_jump = Some((t.id.clone(), mid));
                    }
                }
                None => {}
            }
            if let Some((idx, tid, is_group)) = clicked_thread {
                let resolved_idx = self.threads.iter().position(|t| t.id == tid).unwrap_or(idx);
                if self.selected_thread_index != resolved_idx {
                    self.replying_to = None;
                    self.filter_starred_only = false;
                    self.force_scroll_to_bottom = true;
                    self.chat_render_window = (tid.clone(), CHAT_RENDER_STEP);
                }
                self.selected_thread_index = resolved_idx;
                // Persist the read cursor so a restart does not resurrect read messages.
                if let Some(t) = self.threads.get_mut(resolved_idx) {
                    let before = t.unread.last_seen_id.clone();
                    let msgs = t.messages.clone();
                    t.unread.read_visible(&msgs);
                    if t.unread.last_seen_id != before {
                        self.persist_threads_async();
                    }
                }
                if self.unread_counts.remove(&tid).is_some() {
                    let total_unread: usize = self.unread_counts.values().sum();
                    let title = if total_unread > 0 {
                        format!("({total_unread}) Zalo Lite — Native Rust Client")
                    } else {
                        "Zalo Lite — Native Rust Client".to_string()
                    };
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
                }
                if !self.settings.block_seen {
                    let last_mid = self
                        .threads
                        .get(resolved_idx)
                        .and_then(|t| t.messages.last())
                        .map(|m| m.id.clone())
                        .unwrap_or_default();
                    if !last_mid.is_empty() {
                        if let Some(tx) = WORKER_TX.get() {
                            let _ = tx.send(WorkerCmd::SendSeen {
                                thread_id: tid.clone(),
                                is_group,
                                last_msg_id: last_mid,
                            });
                        }
                    }
                }
                let is_cloud = self
                    .threads
                    .get(resolved_idx)
                    .map(|t| t.name.contains("Cloud của tôi"))
                    .unwrap_or(false);
                let needs_history = self
                    .threads
                    .get(resolved_idx)
                    .is_none_or(|t| t.messages.is_empty());
                if (is_group || is_cloud) && needs_history {
                    if let Some(tx) = WORKER_TX.get() {
                        let _ = tx.send(WorkerCmd::LoadGroupHistory {
                            group_id: tid,
                            is_group,
                            have: 0,
                        });
                    }
                }
            }
            if let Some((tid, reference)) = pick_jump {
                // Open the chat at the picked message (not at the bottom).
                self.force_scroll_to_bottom = false;
                self.filter_starred_only = false;
                self.quote_jump = Some(QuoteJump {
                    thread_id: tid,
                    reference,
                    highlight_until: std::time::Instant::now() + std::time::Duration::from_secs(3),
                    scroll_pending: true,
                });
            }
            if let Some(act) = pending_ctx_action {
                self.apply_thread_ctx_action(ctx, act, now_unix);
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
        // 2b. GROUP MEMBERS PANEL (opened from the chat header avatar)
        if self.members_panel_open {
            match &thread_meta {
                Some((m_id, _, _, true, _)) => {
                    let m_id = m_id.clone();
                    let w = (screen_w * 0.24).clamp(240.0, 300.0);
                    self.render_members_panel(ctx, &m_id, pal, w);
                }
                _ => self.members_panel_open = false,
            }
        }
        // 3. COLLAPSIBLE RIGHT CONVERSATION INFO DRAWER (responsive width)
        // -------------------------------------------------------------------
        let drawer_w = (screen_w * 0.26).clamp(240.0, 310.0);
        // Hide the drawer when it would squeeze the chat below a usable width (bubbles, header
        // and composer overlapped each other in narrow windows); it returns when space allows.
        let list_panel_w = sidebar_exact_w + 28.0 + 2.0 * ThemePalette::CARD_INSET;
        let chat_left_w = screen_w - list_panel_w - (drawer_w + 2.0 * ThemePalette::CARD_INSET);
        // Docked beside the chat when there is room; otherwise it floats over the chat's right
        // side (it used to vanish until the window was widened).
        let drawer_docked = !is_compact_w && chat_left_w >= 460.0;
        if !self.info_drawer_open {
            self.drawer_confirm_delete = false;
        }
        if self.info_drawer_open {
            if let Some(meta) = thread_meta.clone() {
                let mut close = ctx.input(|i| i.key_pressed(Key::Escape));

                let drawer_rect = if drawer_docked {
                    let drawer = egui::SidePanel::right("conversation_info_drawer")
                        .show_separator_line(false)
                        .resizable(false)
                        .exact_width(drawer_w + 2.0 * ThemePalette::CARD_INSET)
                        .frame(pal.card(pal.group_bg()))
                        .show(ctx, |ui| {
                            close |= self.render_conversation_info(ui, pal, &meta);
                        });
                    drawer.response.rect
                } else {
                    let screen = ctx.screen_rect();
                    let inset = ThemePalette::CARD_INSET;
                    let w = drawer_w.min(screen.width() - 2.0 * inset);
                    let h = screen.height() - 2.0 * inset;
                    let slide = appear(ctx, Id::new(("drawer_float", &meta.0)), 0.2);
                    let area = egui::Area::new(Id::new("conversation_info_float"))
                        .order(egui::Order::Middle)
                        .fixed_pos(Pos2::new(
                            screen.right() - w - inset + (1.0 - slide) * 24.0,
                            screen.top() + inset,
                        ))
                        .constrain(false)
                        .show(ctx, |ui| {
                            ui.set_opacity(slide);
                            egui::Frame::none()
                                .fill(pal.group_bg())
                                .rounding(Rounding::same(ThemePalette::CARD_RADIUS))
                                .shadow(egui::epaint::Shadow {
                                    offset: Vec2::new(0.0, 8.0),
                                    blur: 28.0,
                                    spread: 0.0,
                                    color: Color32::from_black_alpha(70),
                                })
                                .show(ui, |ui| {
                                    ui.set_width(w);
                                    ui.set_height(h);
                                    close |= self.render_conversation_info(ui, pal, &meta);
                                });
                        });
                    // Keep it above the floating chat header (same order).
                    ctx.move_to_top(area.response.layer_id);
                    area.response.rect
                };
                close |= ctx.input(|i| i.pointer.any_pressed())
                    && ctx
                        .pointer_interact_pos()
                        .is_some_and(|p| !drawer_rect.contains(p));
                if close {
                    self.info_drawer_open = false;
                    self.settings.info_drawer_open = false;
                    self.settings.save();
                }
            }
        }

        // -------------------------------------------------------------------
        // 4. CENTRAL CHAT AREA (Clean Header, Messages Stream, Icon Toolbar)
        // -------------------------------------------------------------------
        // Telegram-like: the chat area has no panel of its own; wallpaper / glass shows through
        // and the header and composer float above it as pills.
        egui::CentralPanel::default()
            .frame(pal.card(Color32::TRANSPARENT))
            .show(ctx, |ui| {
                if let Some((thread_id, thread_name, thread_avatar, is_group, _msg_count)) =
                    thread_meta
                {
                    let is_cloud_thread = thread_name.contains("Cloud của tôi");
                    if !is_group
                        && !is_cloud_thread
                        && !self.friend_status_map.contains_key(&thread_id)
                        && self.checking_friend_status.insert(thread_id.clone())
                    {
                        if let Some(tx) = WORKER_TX.get() {
                            let _ = tx.send(WorkerCmd::CheckFriendStatus {
                                user_id: thread_id.clone(),
                            });
                        }
                    }
                    let cur_friendship = if is_group || is_cloud_thread {
                        crate::chat::FriendshipStatus::Friend
                    } else {
                        self.friend_status_map
                            .get(&thread_id)
                            .copied()
                            .unwrap_or(crate::chat::FriendshipStatus::Unknown)
                    };
                    // 1. Clean Conversation Header Bar (No raw 19-digit ID, no bulky sync button)
                    // Telegram: the header floats on its own layer so the message list can scroll
                    // beneath the pills instead of being cut by a hard edge under the header.
                    let header_top = ui.cursor().top();
                    let header_left = ui.available_rect_before_wrap().left();
                    let header_w = ui.available_width();
                    let header_clip = ui.clip_rect();
                    let header_area = egui::Area::new(Id::new("chat_header_float"))
                        .order(egui::Order::Middle)
                        .fixed_pos(Pos2::new(header_left, header_top))
                        .constrain(false)
                        .fade_in(false)
                        .show(ctx, |ui| {
                    ui.set_clip_rect(header_clip);
                    ui.set_width(header_w);
                    // Empty header space drags the window; the pills' controls sit on top.
                    let hdr_bg = Rect::from_min_size(ui.max_rect().min, Vec2::new(header_w, 42.0));
                    let drag = ui.interact(hdr_bg, Id::new("chat_header_window_drag"), Sense::click_and_drag());
                    window_drag_from(ctx, &drag);
                    egui::Frame::none()
                        .fill(Color32::TRANSPARENT) // Telegram: two separate pills painted below
                        .rounding(Rounding::same(22.0))
                        .outer_margin(Margin { left: 8.0, right: 8.0, top: 4.0, bottom: 2.0 })
                        .inner_margin(Margin::symmetric(14.0, 6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // Placeholders so the pill backgrounds sit beneath the content.
                                let pill_left_idx = ui.painter().add(egui::Shape::Noop);
                                let pill_right_idx = ui.painter().add(egui::Shape::Noop);
                                // Header Avatar (40x40) with Cutout Online Dot. Click: group →
                                // members panel, person → "Thông tin tài khoản" card.
                                let (av_rect, av_click) =
                                    ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::click());
                                if av_click
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text(if is_group { "Thành viên" } else { "Thông tin tài khoản" })
                                    .clicked()
                                    && !is_cloud_thread
                                {
                                    if is_group {
                                        self.members_panel_open = !self.members_panel_open;
                                    } else {
                                        self.open_account_card(
                                            thread_id.clone(),
                                            thread_name.clone(),
                                            thread_avatar.clone(),
                                        );
                                    }
                                }
                                paint_user_avatar(
                                    ui.painter(),
                                    av_rect.center(),
                                    14.0,
                                    &thread_name,
                                    is_group,
                                    thread_avatar.as_deref(),
                                    &self.sticker_textures,
                                    &mut self.loading_stickers,
                                );
                                let cur_onl = self.user_online_map.get(&thread_id);
                                if !is_group
                                    && cur_onl.map(|(onl, _)| *onl).unwrap_or(false)
                                {
                                    let dot_pos =
                                        Pos2::new(av_rect.right() - 4.0, av_rect.bottom() - 4.0);
                                    ui.painter().circle_filled(dot_pos, 6.2, pal.bg_sidebar);
                                    ui.painter().circle_filled(dot_pos, 4.2, pal.accent_online);
                                }
                                ui.add_space(6.0);
                                // Reserve room for the right-hand action cluster so the name/tag
                                // block can never be overlapped by the call buttons.
                                // Narrow chat: fewer buttons so the name never sits under them.
                                let hdr_compact = is_compact_w || ui.available_width() < 440.0;
                                let actions_base = if hdr_compact { 108.0 } else { 148.0 };
                                let search_target = (ui.available_width() - actions_base - 96.0).clamp(0.0, 150.0);
                                if search_target < 48.0 {
                                    self.in_chat_search_open = false;
                                }
                                let search_t = ctx.animate_bool_with_time(
                                    Id::new(("header_search_width", &thread_id)),
                                    self.in_chat_search_open,
                                    if reduce_motion() { 0.0 } else { 0.24 },
                                );
                                let search_width = search_target * (1.0 - (1.0 - search_t).powi(3));
                                // Leave room for both pill margins as well as the visible gap.
                                let actions_w = actions_base + search_width + if search_width > 0.5 { 8.0 } else { 0.0 };
                                let left_info_w = (ui.available_width() - actions_w - 36.0).max(24.0);
                                let info_resp = ui.allocate_ui_with_layout(
                                    Vec2::new(left_info_w, 28.0),
                                    Layout::top_down(Align::Min),
                                    |ui| {
                                        ui.set_max_width(left_info_w);
                                        ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                                        ui.spacing_mut().item_spacing.y = 0.0;
                                        // Without a status line the name sits centred in the pill.
                                        let has_sub = !is_group
                                            && (is_cloud_thread
                                                || !matches!(
                                                    cur_friendship,
                                                    crate::chat::FriendshipStatus::Friend
                                                        | crate::chat::FriendshipStatus::Unknown
                                                )
                                                || cur_onl.is_some_and(|o| !o.1.is_empty()));
                                        if !has_sub {
                                            ui.add_space(6.0);
                                        }
                                        ui.horizontal(|ui| {
                                            let name_max = if is_compact_w { 12 } else { 22 };
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(truncate_chars(
                                                        &thread_name,
                                                        name_max,
                                                    ))
                                                    .size(13.0)
                                                    .color(pal.text_primary)
                                                    .strong(),
                                                )
                                                .truncate(),
                                            );
                                            // Friends get no header tag: "Bạn bè" is shown in
                                            // the conversation info panel instead.
                                            let is_friend_1v1 = !is_group
                                                && !is_cloud_thread
                                                && cur_friendship == crate::chat::FriendshipStatus::Friend;
                                            if !is_compact_w && left_info_w > 230.0 && search_t < 0.01 && !is_friend_1v1 {
                                                let (tag_lbl, tag_fg, tag_bg) =
                                                    if is_cloud_thread {
                                                        (
                                                            "Đám mây",
                                                            pal.primary_hover,
                                                            pal.primary.linear_multiply(0.14),
                                                        )
                                                    } else if is_group {
                                                        ("Nhóm", pal.text_secondary, pal.bg_elevated)
                                                    } else {
                                                        match cur_friendship {
                                                            crate::chat::FriendshipStatus::Friend => (
                                                                "✓ Bạn bè",
                                                                pal.accent_online,
                                                                pal.accent_online.linear_multiply(0.14),
                                                            ),
                                                            crate::chat::FriendshipStatus::IncomingPending => (
                                                                "Chờ đồng ý kết bạn",
                                                                pal.primary_hover,
                                                                pal.primary.linear_multiply(0.16),
                                                            ),
                                                            crate::chat::FriendshipStatus::OutgoingPending => (
                                                                "Đã gửi lời mời",
                                                                pal.warning,
                                                                pal.warning.linear_multiply(0.15),
                                                            ),
                                                            crate::chat::FriendshipStatus::Stranger => (
                                                                "Người lạ",
                                                                Color32::from_rgb(249, 115, 22),
                                                                Color32::from_rgb(249, 115, 22)
                                                                    .linear_multiply(0.15),
                                                            ),
                                                            crate::chat::FriendshipStatus::Unknown => (
                                                                "Đang kiểm tra…",
                                                                pal.text_muted,
                                                                pal.bg_elevated,
                                                            ),
                                                        }
                                                    };
                                                egui::Frame::none()
                                                    .fill(tag_bg)
                                                    .rounding(Rounding::same(10.0))
                                                    .inner_margin(Margin::symmetric(6.0, 1.5))
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            RichText::new(tag_lbl)
                                                                .size(9.5)
                                                                .color(tag_fg)
                                                                .strong(),
                                                        );
                                                    });
                                            }
                                        });
                                        let (sub_status_lbl, sub_status_col): (String, Color32) =
                                            if is_group {
                                                (String::new(), pal.text_secondary)
                                            } else if is_cloud_thread {
                                                ("Lưu trữ đám mây cá nhân".into(), pal.text_secondary)
                                            } else {
                                                match cur_friendship {
                                                    crate::chat::FriendshipStatus::Friend => {
                                                        match cur_onl {
                                                            Some((true, lbl)) if !lbl.is_empty() => (
                                                                lbl.clone(),
                                                                pal.accent_online,
                                                            ),
                                                            Some((false, lbl)) if !lbl.is_empty() => (
                                                                lbl.clone(),
                                                                pal.text_secondary,
                                                            ),
                                                            _ => (String::new(), pal.text_secondary),
                                                        }
                                                    }
                                                    crate::chat::FriendshipStatus::IncomingPending => (
                                                        "Đã gửi lời mời kết bạn cho bạn".into(),
                                                        pal.primary_hover,
                                                    ),
                                                    crate::chat::FriendshipStatus::OutgoingPending => (
                                                        "Đã gửi lời mời kết bạn · Chờ đồng ý".into(),
                                                        pal.warning,
                                                    ),
                                                    crate::chat::FriendshipStatus::Stranger => (
                                                        "Người lạ · Chưa kết bạn trên Zalo".into(),
                                                        Color32::from_rgb(249, 115, 22),
                                                    ),
                                                    crate::chat::FriendshipStatus::Unknown => {
                                                        match cur_onl {
                                                            Some((true, lbl)) if !lbl.is_empty() => (
                                                                lbl.clone(),
                                                                pal.accent_online,
                                                            ),
                                                            Some((false, lbl)) if !lbl.is_empty() => (
                                                                lbl.clone(),
                                                                pal.text_secondary,
                                                            ),
                                                            _ => (String::new(), pal.text_secondary),
                                                        }
                                                    }
                                                }
                                            };
                                        // No filler line ("Nhóm trò chuyện" / "Trò chuyện cá
                                        // nhân"): only a real status is shown under the name.
                                        if !sub_status_lbl.is_empty() {
                                            ui.label(
                                                RichText::new(sub_status_lbl)
                                                    .size(10.5)
                                                    .color(sub_status_col),
                                            );
                                        }
                                    },
                                );

                                let actions_resp = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    // Info Drawer Toggle Button (hidden on compact widths: auto-collapsed)
                                    if !is_compact_w {
                                        let (drw_rect, drw_resp) = ui.allocate_exact_size(
                                            Vec2::new(28.0, 28.0),
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
                                            15.0,
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
                                            self.settings.info_drawer_open = self.info_drawer_open;
                                            self.settings.save();
                                        }
                                    }

                                    if !hdr_compact {
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
                                        Vec2::new(28.0, 28.0),
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
                                        15.0,
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
                                        let ff_ok = crate::media::video_calls_available();
                                        let (_, cams) = crate::media::list_media_devices(false);
                                        let can_cam = ff_ok && !cams.is_empty();
                                        if !ff_ok {
                                            self.show_toast(
                                                "⚠️ Không tìm thấy ffmpeg trong PATH để bật Camera (cài bằng: winget install Gyan.FFmpeg)",
                                            );
                                        } else if cams.is_empty() {
                                            self.show_toast(
                                                "⚠️ Không phát hiện thiết bị Camera trên máy — đang gọi không hình",
                                            );
                                        }
                                        self.active_call = Some(ActiveCallOverlay {
                                            partner_id: thread_id.clone(),
                                            partner_name: thread_name.clone(),
                                            is_video: true,
                                            is_group,
                                            state_label: "Đang kết nối cuộc gọi video…".into(),
                                            started_at: std::time::Instant::now(),
                                            mic_muted: false,
                                            cam_enabled: can_cam,
                                            is_incoming_ringing: false,
                                            error_code: None,
                                            error_reason: None,
                                            speaker_view: false,
                                            pinned_tile_idx: 0,
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

                                    }
                                    // Voice Call Button
                                    let (aud_rect, aud_resp) = ui.allocate_exact_size(
                                        Vec2::new(28.0, 28.0),
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
                                        15.0,
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
                                            speaker_view: false,
                                            pinned_tile_idx: 0,
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
                                    if search_width > 0.5 {
                                        let (rect, _) = ui.allocate_exact_size(Vec2::new(search_width, 28.0), Sense::hover());
                                        ui.painter().rect_filled(rect, Rounding::same(14.0), pal.bg_input);
                                        let mut input_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(8.0, 4.0))));
                                        input_ui.set_clip_rect(rect.intersect(ui.clip_rect()));
                                        let response = input_ui.add(
                                            egui::TextEdit::singleline(&mut self.in_chat_search_query)
                                                .frame(false)
                                                .font(FontId::proportional(13.0))
                                                .desired_width((search_width - 16.0).max(1.0))
                                                .hint_text("Tìm tin nhắn…")
                                                .id(Id::new(("in_chat_search_input", &thread_id))),
                                        );
                                        if self.in_chat_search_focus_pending && search_width > 48.0 {
                                            response.request_focus();
                                            self.in_chat_search_focus_pending = false;
                                        }
                                    }
                                    // Search Messages in Conversation Button (`Ctrl+F`)
                                    let (srch_rect, srch_resp) = ui.allocate_exact_size(
                                        Vec2::new(28.0, 28.0),
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
                                        15.0,
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
                                        if self.in_chat_search_open {
                                            self.in_chat_search_focus_pending = true;
                                        } else {
                                            self.in_chat_search_query.clear();
                                        }
                                    }
                                });
                                // Telegram header: a pill around avatar + name, another around
                                // the action buttons, with the wallpaper visible between them.
                                let row = av_rect.union(info_resp.response.rect);
                                // Same 34 pt height as the composer blocks.
                                let (top, bottom) = (row.top() - 3.0, row.bottom() + 3.0);
                                let mut left_pill = Rect::from_min_max(
                                    Pos2::new(av_rect.left() - 10.0, top),
                                    Pos2::new(info_resp.response.rect.right() + 18.0, bottom),
                                );
                                let ar = actions_resp.response.rect;
                                let right_pill = Rect::from_min_max(
                                    Pos2::new(ar.left() - 10.0, top),
                                    Pos2::new(ar.right() + 10.0, bottom),
                                );
                                left_pill.max.x = left_pill.max.x.min(right_pill.left() - 12.0);
                                let pill = |r: Rect| {
                                    let rounding = Rounding::same(r.height() / 2.0);
                                    let fill = if pal.is_dark { Color32::from_rgb(38,40,46) } else { Color32::from_rgb(250,250,252) };
                                    let shadow = egui::epaint::Shadow { offset: Vec2::new(0.0,4.0), blur: 16.0, spread: 1.0, color: Color32::from_black_alpha(80) };
                                    egui::Shape::Vec(vec![egui::Shape::Rect(shadow.as_shape(r,rounding)),egui::Shape::Rect(egui::epaint::RectShape::filled(r,rounding,fill))])
                                };
                                ui.painter().set(pill_left_idx, pill(left_pill));
                                if ar.width() > 1.0 {
                                    ui.painter().set(pill_right_idx, pill(right_pill));
                                }
                            });
                        });
                        });
                    let header_h = header_area.response.rect.height();
                    self.header_rect_last = header_area.response.rect;
                    ui.add_space(header_h);
                    let header_bottom = ui.cursor().top();
                    // 1a. Stranger / Pending Friend Request Banner (matching Zalo PC's top relationship bar)
                    if !is_group
                        && !is_cloud_thread
                        && matches!(
                            cur_friendship,
                            crate::chat::FriendshipStatus::Stranger
                                | crate::chat::FriendshipStatus::IncomingPending
                                | crate::chat::FriendshipStatus::OutgoingPending
                        )
                    {
                        // Rounded floating capsule like the header pills (was a square strip).
                        egui::Frame::none()
                            .fill(pal.bg_elevated)
                            .rounding(Rounding::same(22.0))
                            .outer_margin(Margin { left: 8.0, right: 8.0, top: 4.0, bottom: 4.0 })
                            .shadow(egui::epaint::Shadow {
                                offset: Vec2::new(0.0, 3.0),
                                blur: 12.0,
                                spread: 0.0,
                                color: Color32::from_black_alpha(if pal.is_dark { 80 } else { 30 }),
                            })
                            .inner_margin(Margin::symmetric(18.0, 8.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    match cur_friendship {
                                        crate::chat::FriendshipStatus::IncomingPending => {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{}  {} đã gửi lời mời kết bạn cho bạn",
                                                    lucide::USER_PLUS,
                                                    truncate_chars(&thread_name, 26)
                                                ))
                                                .size(12.2)
                                                .color(pal.primary_hover)
                                                .strong(),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.spacing_mut().item_spacing.x = 6.0;
                                                    let rej_btn = egui::Button::new(
                                                        RichText::new("Từ chối")
                                                            .size(11.5)
                                                            .color(pal.text_secondary),
                                                    )
                                                    .fill(pal.bg_input)
                                                    .stroke(Stroke::new(1.0_f32, pal.border))
                                                    .rounding(Rounding::same(10.0))
                                                    .min_size(Vec2::new(68.0, 26.0));
                                                    if ui
                                                        .add(rej_btn)
                                                        .on_hover_cursor(CursorIcon::PointingHand)
                                                        .clicked()
                                                    {
                                                        if let Some(tx) = WORKER_TX.get() {
                                                            let _ = tx.send(
                                                                WorkerCmd::RejectFriend {
                                                                    user_id: thread_id.clone(),
                                                                    user_name: thread_name.clone(),
                                                                },
                                                            );
                                                        }
                                                    }

                                                    let acc_btn = egui::Button::new(
                                                        RichText::new(format!(
                                                            "{} Đồng ý kết bạn",
                                                            lucide::CHECK
                                                        ))
                                                        .size(11.8)
                                                        .color(Color32::WHITE)
                                                        .strong(),
                                                    )
                                                    .fill(pal.accent_online)
                                                    .rounding(Rounding::same(10.0))
                                                    .min_size(Vec2::new(124.0, 26.0));
                                                    if ui
                                                        .add(acc_btn)
                                                        .on_hover_cursor(CursorIcon::PointingHand)
                                                        .clicked()
                                                    {
                                                        if let Some(tx) = WORKER_TX.get() {
                                                            let _ = tx.send(
                                                                WorkerCmd::AcceptFriend {
                                                                    user_id: thread_id.clone(),
                                                                    user_name: thread_name.clone(),
                                                                },
                                                            );
                                                        }
                                                    }
                                                },
                                            );
                                        }
                                        crate::chat::FriendshipStatus::OutgoingPending => {
                                            ui.label(
                                                RichText::new(format!(
                                                    "Đã gửi lời mời kết bạn tới {} · Đang chờ đồng ý",
                                                    truncate_chars(&thread_name, 24)
                                                ))
                                                .size(12.0)
                                                .color(pal.warning),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    let undo_btn = egui::Button::new(
                                                        RichText::new("Hủy lời mời")
                                                            .size(11.5)
                                                            .color(pal.text_secondary),
                                                    )
                                                    .fill(pal.bg_input)
                                                    .stroke(Stroke::new(1.0_f32, pal.border))
                                                    .rounding(Rounding::same(10.0))
                                                    .min_size(Vec2::new(86.0, 26.0));
                                                    if ui
                                                        .add(undo_btn)
                                                        .on_hover_cursor(CursorIcon::PointingHand)
                                                        .clicked()
                                                    {
                                                        if let Some(tx) = WORKER_TX.get() {
                                                            let _ = tx.send(WorkerCmd::UndoFriend {
                                                                user_id: thread_id.clone(),
                                                                user_name: thread_name.clone(),
                                                            });
                                                        }
                                                    }
                                                },
                                            );
                                        }
                                        crate::chat::FriendshipStatus::Stranger => {
                                            ui.label(
                                                RichText::new(format!(
                                                    "Bạn và {} chưa là bạn bè trên Zalo",
                                                    truncate_chars(&thread_name, 26)
                                                ))
                                                .size(12.0)
                                                .color(pal.text_secondary),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    let add_btn = egui::Button::new(
                                                        RichText::new(format!(
                                                            "{} Kết bạn",
                                                            lucide::USER_PLUS
                                                        ))
                                                        .size(11.8)
                                                        .color(Color32::WHITE)
                                                        .strong(),
                                                    )
                                                    .fill(pal.primary)
                                                    .rounding(Rounding::same(10.0))
                                                    .min_size(Vec2::new(92.0, 26.0));
                                                    if ui
                                                        .add(add_btn)
                                                        .on_hover_cursor(CursorIcon::PointingHand)
                                                        .clicked()
                                                    {
                                                        if let Some(tx) = WORKER_TX.get() {
                                                            let _ = tx.send(WorkerCmd::AddFriend {
                                                                user_id: thread_id.clone(),
                                                                user_name: thread_name.clone(),
                                                            });
                                                        }
                                                        self.show_toast(format!(
                                                            "Đã gửi lời mời kết bạn tới {}",
                                                            thread_name
                                                        ));
                                                    }
                                                },
                                            );
                                        }
                                        _ => {}
                                    }
                                });
                            });
                    }
                    // 1b. Pinned messages bar: server pins (`/api/board/pin/list`) plus pins
                    // made locally in this app. One row collapsed, every pin when expanded.
                    if self.pinned_requested_for != thread_id {
                        self.pinned_requested_for = thread_id.clone();
                        self.pinned_expanded = false;
                        self.composer_mentions.clear();
                        self.select_mode = false;
                        self.selected_msgs.clear();
                        if let Some(tx) = WORKER_TX.get() {
                            let _ = tx.send(WorkerCmd::FetchPinnedTopics {
                                thread_id: thread_id.clone(),
                            });
                            if is_group && !self.group_members.contains_key(&thread_id) {
                                let _ = tx.send(WorkerCmd::FetchGroupMembers {
                                    thread_id: thread_id.clone(),
                                });
                            }
                        }
                    }
                    // (sender, text, local message index for unpin)
                    let mut pins: Vec<(String, String, Option<usize>)> = self
                        .pinned_topics
                        .get(&thread_id)
                        .map(|v| {
                            v.iter()
                                .map(|p| (p.sender_name.clone(), p.text.clone(), None))
                                .collect()
                        })
                        .unwrap_or_default();
                    if let Some(t) = self.threads.get(self.selected_thread_index) {
                        for (idx, m) in t.messages.iter().enumerate().filter(|(_, m)| m.is_pinned) {
                            let sender = if m.is_outgoing {
                                "Bạn".to_string()
                            } else if !m.sender.is_empty() && m.sender != "0" {
                                m.sender.clone()
                            } else {
                                thread_name.clone()
                            };
                            let text = if m.sticker_id.is_some() {
                                "[Nhãn dán]".to_string()
                            } else {
                                m.text.clone()
                            };
                            if !pins.iter().any(|(_, t, _)| *t == text) {
                                pins.push((sender, text, Some(idx)));
                            }
                        }
                    }

                    let mut unpin_idx: Option<usize> = None;
                    if !pins.is_empty() {
                        let total = pins.len();
                        let shown = if self.pinned_expanded { total } else { 1 };
                        egui::Frame::none()
                            .fill(pal.bg_elevated)
                            .stroke(Stroke::new(1.0_f32, pal.border))
                            .rounding(Rounding::same(if pal.is_glass { 12.0 } else { 6.0 }))
                            .outer_margin(Margin::symmetric(10.0, 6.0))
                            .inner_margin(Margin::symmetric(12.0, 8.0))
                            .show(ui, |ui| {
                                for (row, (sender, text, local_idx)) in
                                    pins.iter().take(shown).enumerate()
                                {
                                    if row > 0 {
                                        ui.add_space(4.0);
                                        ui.separator();
                                    }
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(lucide::MESSAGE_SQUARE)
                                                .size(18.0)
                                                .color(pal.primary),
                                        );
                                        ui.add_space(4.0);
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new("Tin nhắn")
                                                    .size(12.5)
                                                    .strong()
                                                    .color(pal.text_primary),
                                            );
                                            let who = if sender.is_empty() {
                                                String::new()
                                            } else {
                                                format!("{sender}: ")
                                            };
                                            ui.label(
                                                RichText::new(format!(
                                                    "{who}{}",
                                                    truncate_chars(text, 90)
                                                ))
                                                .size(12.2)
                                                .color(pal.text_secondary),
                                            );
                                        });
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if row == 0 && total > 1 {
                                                let lbl = if self.pinned_expanded {
                                                    "Thu gọn".to_string()
                                                } else {
                                                    format!("+{} ghim", total - 1)
                                                };
                                                if ui
                                                    .small_button(lbl)
                                                    .on_hover_cursor(CursorIcon::PointingHand)
                                                    .clicked()
                                                {
                                                    self.pinned_expanded = !self.pinned_expanded;
                                                }
                                            }
                                            if let Some(li) = local_idx {
                                                if ui
                                                    .small_button(lucide::X)
                                                    .on_hover_text("Bỏ ghim trên máy này")
                                                    .on_hover_cursor(CursorIcon::PointingHand)
                                                    .clicked()
                                                {
                                                    unpin_idx = Some(*li);
                                                }
                                            }
                                        });
                                    });
                                }
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

                    // 1c-0. GitHub Release Update Available Notification Banner
                    if let Some(upd) = self.update_info.clone() {
                        if upd.has_update
                            && !self.update_banner_dismissed
                            && self.settings.skipped_update_version != upd.latest_version
                        {
                            egui::Frame::none()
                                .fill(pal.accent_online.linear_multiply(0.14))
                                .stroke(Stroke::new(1.0_f32, pal.accent_online))
                                .inner_margin(Margin::symmetric(14.0, 6.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let msg_txt = if self.update_downloading {
                                            let (dl, tot) = self.update_progress.unwrap_or((0, 0));
                                            if tot > 0 {
                                                let pct = ((dl as f64 / tot as f64) * 100.0)
                                                    .clamp(0.0, 100.0);
                                                format!(
                                                    "⏳ Đang tải bản cập nhật v{} từ GitHub ({pct:.0}%)…",
                                                    upd.latest_version
                                                )
                                            } else {
                                                format!(
                                                    "⏳ Đang tải bản cập nhật v{} từ GitHub…",
                                                    upd.latest_version
                                                )
                                            }
                                        } else {
                                            format!(
                                                "🚀 Đã có bản cập nhật mới ctdozlite v{} trên GitHub Releases!",
                                                upd.latest_version
                                            )
                                        };
                                        let banner_lbl = ui.label(
                                            RichText::new(msg_txt)
                                                .size(12.0)
                                                .color(pal.accent_online)
                                                .strong(),
                                        );
                                        if !upd.release_notes.trim().is_empty() {
                                            banner_lbl.on_hover_text(upd.release_notes.trim());
                                        }
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .small_button(lucide::X)
                                                    .on_hover_cursor(CursorIcon::PointingHand)
                                                    .on_hover_text("Ẩn thanh thông báo cập nhật")
                                                    .clicked()
                                                {
                                                    self.update_banner_dismissed = true;
                                                }
                                                if ui
                                                    .small_button("Chi tiết")
                                                    .on_hover_cursor(CursorIcon::PointingHand)
                                                    .clicked()
                                                {
                                                    self.update_modal_open = true;
                                                }
                                                if !self.update_downloading {
                                                    if let (Some(dl_url), Some(asset_name)) =
                                                        (upd.download_url.clone(), upd.asset_name.clone())
                                                    {
                                                        if ui
                                                            .add(
                                                                egui::Button::new(
                                                                    RichText::new(format!(
                                                                        "⬇ Cập nhật v{}",
                                                                        upd.latest_version
                                                                    ))
                                                                    .size(11.5)
                                                                    .color(Color32::WHITE)
                                                                    .strong(),
                                                                )
                                                                .fill(pal.accent_online)
                                                                .rounding(Rounding::same(10.0)),
                                                            )
                                                            .on_hover_cursor(
                                                                CursorIcon::PointingHand,
                                                            )
                                                            .clicked()
                                                        {
                                                            self.update_downloading = true;
                                                            self.update_progress =
                                                                Some((0, upd.asset_size));
                                                            self.update_modal_open = true;
                                                            if let Some(tx) = WORKER_TX.get() {
                                                                let _ = tx.send(
                                                                    WorkerCmd::DownloadAndApplyUpdate {
                                                                        download_url: dl_url,
                                                                        asset_name,
                                                                        target_version: upd
                                                                            .latest_version
                                                                            .clone(),
                                                                        expected_sha256: upd
                                                                            .expected_sha256
                                                                            .clone(),
                                                                    },
                                                                );
                                                            }
                                                        }
                                                    }
                                                }
                                            },
                                        );
                                    });
                                });
                        }
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
                                            .small_button(format!("Xem tất cả {}", lucide::X))
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
                    if !self.in_chat_search_open {
                        ctx.data_mut(|d| d.remove::<bool>(Id::new(("in_chat_search_seen", &thread_id))));
                    }
                    if self.in_chat_search_open
                        && (ui.input(|i| i.key_pressed(Key::Escape)) || (self.in_chat_search_query.trim().is_empty() && ctx.input(|i| i.pointer.any_pressed()) && ctx.pointer_interact_pos().is_some_and(|p| !header_area.response.rect.contains(p)))) {
                            self.in_chat_search_open = false;
                            self.in_chat_search_query.clear();
                        }
                    if self.in_chat_search_open {
                        let q_low = self.in_chat_search_query.trim().to_lowercase();
                        let q_folded = fold_vn_search(&q_low);
                        let search_bar_rect = header_area.response.rect;
                        // Zalo-style results under the bar: newest first, keyword highlighted;
                        // a click jumps to the message in the full conversation.
                        if self.in_chat_search_open && !q_low.is_empty() {
                            if self.in_chat_results_query != q_low {
                                self.in_chat_results_query = q_low.clone();
                                self.in_chat_results_hidden = false;
                                self.in_chat_results_limit = 20;
                            }
                            if !self.in_chat_results_hidden {
                                self.render_in_chat_results(
                                    ctx,
                                    pal,
                                    search_bar_rect,
                                    &thread_id,
                                    &thread_name,
                                    thread_avatar.as_deref(),
                                    is_group,
                                    &q_low,
                                    &q_folded,
                                );
                            }
                        }
                    }

                    // 2. Scrollable Messages Area
                    let contextual_stickers = self.resolve_contextual_sticker_suggestions(ctx);
                    let composer_link_preview = self.resolve_composer_link_preview(ctx);
                    // Do NOT subtract `contextual_stickers` height from `picker_reserve`!
                    // The sticker strip is a true floating overlay (`egui::Area`) above the composer bar
                    // (matching Zalo PC's `PopoverIdentitiesDefine.STICKER_SUGGESTION`), so the message scroll
                    // area and composer input box never shift or lose IME focus while typing.
                    let picker_reserve = (if self.sticker_picker_open { 268.0 } else { 0.0 })
                        + (if self.replying_to.is_some() { 48.0 } else { 0.0 })
                        // Attachment strip: one row of 72 pt tiles plus the name pill.
                        + (if self.pending_paste_files.is_empty() && self.pending_clipboard_image.is_none() {
                            0.0
                        } else {
                            ATTACH_TILE + 34.0
                        });
                    let mut pending_action: Option<(usize, BubbleAction)> = None;
                    let mut select_toggles: Vec<String> = Vec::new();
                    let mut load_older_clicked = false;
                    let mut custom_toast: Option<String> = None;
                    // each frame, so a 2 000-message group re-laid out 2 000 galleys per repaint.
                    // "Tải thêm tin nhắn cũ" widens the window before asking the server.
                    if self.chat_render_window.0 != thread_id {
                        self.chat_render_window = (thread_id.clone(), CHAT_RENDER_STEP);
                    }
                    let jump_index = self.quote_jump.as_ref().and_then(|jump| {
                        self.threads
                            .get(self.selected_thread_index)
                            .and_then(|thread| jump.target_index(&thread_id, &thread.messages))
                    });
                    if jump_index.is_none() {
                        self.quote_jump = None;
                    }
                    let jump_scroll_pending = self
                        .quote_jump
                        .as_ref()
                        .map(|jump| jump.scroll_pending)
                        .unwrap_or(false);
                    let mut jump_scrolled = false;
                    if let Some(index) = jump_index {
                        let count = self
                            .threads
                            .get(self.selected_thread_index)
                            .map(|thread| thread.messages.len())
                            .unwrap_or(0);
                        self.chat_render_window.1 = crate::message_navigation::render_limit_for_target(
                            self.chat_render_window.1,
                            count,
                            index,
                        );
                    }
                    let render_limit = self.chat_render_window.1;
                    let mut widen_window = false;
                    // Group sender -> avatar, built once per frame instead of a scan per bubble.
                    // `@Name` tags to colour in this group's messages (Zalo shows them blue).
                    if is_group {
                        let mut names: Vec<String> = vec!["All".into(), self.user_name.clone()];
                        for gm in self.group_members.get(&thread_id).into_iter().flatten() {
                            names.push(gm.name.clone());
                            if let Some(a) = crate::gui::alias_for(&gm.id) {
                                names.push(a);
                            }
                        }
                        if let Some(t) = self.threads.get(self.selected_thread_index) {
                            for m in &t.presentation.members {
                                names.push(m.name.clone());
                            }
                        }
                        set_mention_names(names);
                    } else {
                        set_mention_names(Vec::new());
                    }
                    let avatar_by_name: HashMap<&str, &str> = if is_group {
                        // Friends' 1-1 threads first, then the group's own member list so
                        // non-friends get their avatar too.
                        let mut m: HashMap<&str, &str> = self
                            .threads
                            .iter()
                            .filter(|th| !th.is_group)
                            .filter_map(|th| Some((th.name.as_str(), th.avatar.as_deref()?)))
                            .collect();
                        for gm in self.group_members.get(&thread_id).into_iter().flatten() {
                            if let Some(a) = gm.avatar.as_deref() {
                                m.entry(gm.name.as_str()).or_insert(a);
                            }
                        }
                        m
                    } else {
                        HashMap::new()
                    };
                    let composer_lines = self
                        .input_message
                        .split('\n')
                        .count()
                        .clamp(1, 5);
                    let composer_extra_h = (composer_lines.saturating_sub(1) as f32) * 20.0;
                    // Composer pill height (68 base + 12 outer margins + extra lines).
                    // 34 pt blocks + paddings (Telegram size).
                    let composer_block_h = 34.0 + 4.0 + 12.0 + 6.0 + composer_extra_h;
                    // With nothing staged above the composer, the list runs under it (padding at
                    // the end keeps the newest message visible); trays keep the stacked layout.
                    let list_under_composer = picker_reserve <= 0.0;
                    let msg_area_height = (ui.available_height()
                        - if list_under_composer { 0.0 } else { composer_block_h + picker_reserve })
                        .max(100.0);
                    // Fade the conversation in when switching threads.
                    let thread_fade = appear(ctx, Id::new(("thread_fade", &thread_id)), 0.18);
                    let opacity_before = ui.opacity();
                    ui.set_opacity(opacity_before * thread_fade);
                    // With nothing between header and list (banners, pinned bar, search), the list
                    // starts under the floating header and pads its first message below it.
                    let header_overlap = if (ui.cursor().top() - header_bottom).abs() < 1.0 {
                        (header_bottom - header_top).max(0.0)
                    } else {
                        0.0
                    };
                    let list_avail = ui.available_rect_before_wrap();
                    let list_rect = Rect::from_min_max(
                        Pos2::new(list_avail.left(), list_avail.top() - header_overlap),
                        Pos2::new(list_avail.right(), list_avail.top() + msg_area_height),
                    );
                    let chat_scroll_out = ui
                        .allocate_new_ui(egui::UiBuilder::new().max_rect(list_rect), |ui| {
                    // The list runs under the composer and header: over those, the wheel belongs
                    // to the composer's own text (long drafts scroll) and reactions stay closed.
                    let pointer_over_composer = ctx
                        .pointer_hover_pos()
                        .is_some_and(|p| self.composer_rect_last.contains(p));
                    let mut blockers = vec![self.composer_rect_last, self.header_rect_last];
                    blockers.extend(std::mem::take(&mut self.overlay_rects_last));
                    set_react_blockers(blockers);
                    let now = ctx.input(|i| i.time);
                    if self.force_scroll_to_bottom {
                        // Opening a chat (or the app): hold the newest message in view while
                        // photos and file previews load and grow the rows above it.
                        self.pin_chat_bottom_until = now + 2.5;
                    }
                    let user_scrolled = ctx.input(|i| {
                        i.smooth_scroll_delta.y != 0.0 || i.pointer.primary_down()
                    }) && ctx.pointer_hover_pos().is_some_and(|p| list_rect.contains(p));
                    if user_scrolled || jump_index.is_some() {
                        self.pin_chat_bottom_until = 0.0;
                    }
                    let pin_bottom = now < self.pin_chat_bottom_until;
                    if pin_bottom {
                        ctx.request_repaint();
                    }
                    let chat_scroll = egui::ScrollArea::vertical()
                        .id_salt(Id::new(("chat_scroll", &thread_id)))
                        .enable_scrolling(!pointer_over_composer)
                        .max_height(msg_area_height + header_overlap)
                        .auto_shrink([false, false])
                        .stick_to_bottom(jump_index.is_none());
                    chat_scroll
                        .show(ui, |ui| {
                            ui.add_space(header_overlap);
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
                                                    .size(13.0)
                                                    .color(pal.text_muted),
                                                );
                                            });
                                        } else {
                                            if !self.in_chat_search_open
                                                && !self.filter_starred_only
                                            {
                                                let has_more_in_memory = thread.messages.len() > render_limit;
                                                let reached_start = self.history_reached_start.contains(&thread_id);

                                                if reached_start && !has_more_in_memory {
                                                    ui.vertical_centered(|ui| {
                                                        ui.add_space(8.0);
                                                        ui.label(
                                                            RichText::new("— Đây là tin nhắn đầu tiên của cuộc trò chuyện —")
                                                                .size(11.5)
                                                                .color(pal.text_muted),
                                                        );
                                                        ui.add_space(8.0);
                                                    });
                                                } else {
                                                    ui.vertical_centered(|ui| {
                                                        let hist_btn = egui::Button::new(
                                                            RichText::new(format!(
                                                                "{} Tải thêm tin nhắn cũ",
                                                                lucide::REFRESH
                                                            ))
                                                            .size(11.5)
                                                            .color(pal.text_secondary),
                                                        )
                                                        .fill(pal.bg_elevated)
                                                        .stroke(Stroke::new(1.0_f32, pal.border))
                                                        .rounding(Rounding::same(12.0));
                                                        if ui
                                                            .add(hist_btn)
                                                            .on_hover_cursor(CursorIcon::PointingHand)
                                                            .on_hover_text(
                                                                "Đồng bộ thêm lịch sử tin nhắn cũ từ máy chủ Zalo",
                                                            )
                                                            .clicked()
                                                        {
                                                            if has_more_in_memory {
                                                                widen_window = true;
                                                                let new_count = (render_limit + CHAT_RENDER_STEP).min(thread.messages.len());
                                                                custom_toast = Some(format!("Đã hiển thị thêm tin nhắn cũ ({new_count}/{})", thread.messages.len()));
                                                            } else {
                                                                if let Some(tx) = WORKER_TX.get() {
                                                                    let _ = tx.send(
                                                                        WorkerCmd::LoadGroupHistory {
                                                                            group_id: thread_id.clone(),
                                                                            is_group,
                                                                            have: thread.messages.len(),
                                                                        },
                                                                    );
                                                                }
                                                                load_older_clicked = true;
                                                            }
                                                        }
                                                        ui.add_space(6.0);
                                                    });
                                                }
                                            }
                                            let only_starred = self.filter_starred_only;
                                            // The in-chat search lists matches in its own panel;
                                            // the conversation itself stays complete.
                                            let in_chat_q = String::new();
                                            let in_chat_folded = fold_vn_search(&in_chat_q);
                                            let visible_indices: Vec<usize> = thread
                                                .messages
                                                .iter()
                                                .enumerate()
                                                .filter(|(_, m)| {
                                                    (!only_starred || m.is_starred)
                                                        && (in_chat_q.is_empty()
                                                            || matches_vn_query(
                                                                &m.text,
                                                                &in_chat_q,
                                                                &in_chat_folded,
                                                            )
                                                            || matches_vn_query(
                                                                &m.sender,
                                                                &in_chat_q,
                                                                &in_chat_folded,
                                                            ))
                                                })
                                                .map(|(i, _)| i)
                                                .collect();
                                            // Search and the starred filter still scan everything.
                                            let visible_indices: Vec<usize> =
                                                if only_starred || !in_chat_q.is_empty() {
                                                    visible_indices
                                                } else {
                                                    let skip = visible_indices
                                                        .len()
                                                        .saturating_sub(render_limit);
                                                    visible_indices[skip..].to_vec()
                                                };
                                            if visible_indices.is_empty() && !in_chat_q.is_empty() {
                                                ui.vertical_centered(|ui| {
                                                    ui.add_space(56.0);
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "Không tìm thấy tin nhắn nào khớp từ khóa \"{}\".",
                                                            self.in_chat_search_query.trim()
                                                        ))
                                                        .size(13.5)
                                                        .color(pal.text_muted),
                                                    );
                                                });
                                            }
                                            let last_outgoing_idx = thread
                                                .messages
                                                .iter()
                                                .enumerate()
                                                .rev()
                                                .find(|(_, m)| {
                                                    m.is_outgoing && !is_system_notice_message(m)
                                                })
                                                .map(|(i, _)| i);

                                            // Rows set their own gaps: consecutive messages of one
                                            // sender sit almost touching, like Zalo.
                                            ui.spacing_mut().item_spacing.y = 0.0;
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

                                                let row = ui.scope(|ui| {
                                                    ui.spacing_mut().item_spacing.y = 2.0;
                                                    if is_system_notice_message(msg) {
                                                        render_system_notice_pill(ui, msg, pal);
                                                        ui.add_space(6.0);
                                                        return;
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
                                                        avatar_by_name.get(msg.sender.as_str()).copied()
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
                                                    // A reaction chip hangs 11 pt below its bubble.
                                                    let chip_room = if msg.reactions.is_empty() { 0.0 } else { 11.0 };
                                                    ui.add_space(if next_same_sender { 2.0 } else { 10.0 } + chip_room);
                                                });
                                                if self.select_mode && !is_system_notice_message(msg) {
                                                    let r = row.response.rect;
                                                    let picked = self.selected_msgs.contains(&msg.id);
                                                    if picked {
                                                        ui.painter().rect_filled(
                                                            r.expand2(Vec2::new(4.0, 1.0)),
                                                            Rounding::same(12.0),
                                                            pal.primary.linear_multiply(0.14),
                                                        );
                                                    }
                                                    let c = Pos2::new(r.right() - 12.0, r.top() + 16.0);
                                                    ui.painter().circle_stroke(c, 8.0, Stroke::new(1.5_f32, pal.primary));
                                                    if picked {
                                                        ui.painter().circle_filled(c, 8.0, pal.primary);
                                                        ui.painter().text(
                                                            c,
                                                            egui::Align2::CENTER_CENTER,
                                                            lucide::CHECK,
                                                            FontId::proportional(10.0),
                                                            Color32::WHITE,
                                                        );
                                                    }
                                                    if ui
                                                        .interact(r, Id::new(("select_row", idx)), Sense::click())
                                                        .on_hover_cursor(CursorIcon::PointingHand)
                                                        .clicked()
                                                    {
                                                        select_toggles.push(msg.id.clone());
                                                    }
                                                }
                                                if jump_index == Some(idx) {
                                                    render_quote_target(
                                                        ui,
                                                        row.response.rect,
                                                        jump_scroll_pending,
                                                        pal.primary,
                                                    );
                                                    jump_scrolled |= jump_scroll_pending;
                                                }
                                            }
                                            if list_under_composer {
                                                ui.add_space(composer_block_h);
                                            }
                                            let (bottom_rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 1.0), Sense::hover());
                                            if self.force_scroll_to_bottom || pin_bottom {
                                                // Instant, not animated: rows above may still grow
                                                // (photos, previews) and an animation would stop short.
                                                // Never set the offset past the end up front: egui
                                                // paints at that offset first, which left the chat blank.
                                                ui.scroll_to_rect_animation(
                                                    bottom_rect,
                                                    Some(Align::BOTTOM),
                                                    egui::style::ScrollAnimation::none(),
                                                );
                                                self.force_scroll_to_bottom = false;
                                            }
                                        }
                                    }
                                });
                        })
                        })
                        .inner;

                    ui.set_opacity(opacity_before);
                    let dist_from_bottom = (chat_scroll_out.content_size.y
                        - (chat_scroll_out.state.offset.y + chat_scroll_out.inner_rect.height()))
                        .max(0.0);
                    if dist_from_bottom > 180.0 {
                        let btn_size = 36.0;
                        let btn_pos = Pos2::new(
                            chat_scroll_out.inner_rect.right() - btn_size - 22.0,
                            // Stay above the floating composer (the list runs under it).
                            chat_scroll_out.inner_rect.bottom()
                                - if list_under_composer { composer_block_h } else { 0.0 }
                                - btn_size
                                - 16.0,
                        );
                        egui::Area::new(Id::new(("auto_scroll_btn", &thread_id)))
                            .order(egui::Order::Middle)
                            .fixed_pos(btn_pos)
                            .show(ctx, |ui| {
                                egui::Frame::none()
                                    .fill(pal.bg_elevated)
                                    .stroke(Stroke::new(1.0_f32, pal.border))
                                    .rounding(Rounding::same(btn_size / 2.0))
                                    .shadow(egui::epaint::Shadow {
                                        offset: Vec2::new(0.0, 2.0),
                                        blur: 8.0,
                                        spread: 0.0,
                                        color: Color32::from_black_alpha(if pal.is_dark { 80 } else { 35 }),
                                    })
                                    .show(ui, |ui| {
                                        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(btn_size), Sense::click());
                                        if resp.hovered() {
                                            ui.painter().circle_filled(rect.center(), btn_size / 2.0, pal.bg_hover);
                                        }
                                        draw_lucide_icon(
                                            ui.painter(),
                                            rect.center(),
                                            lucide::CHEVRON_DOWN,
                                            16.0,
                                            if resp.hovered() { pal.primary_hover } else { pal.text_primary },
                                        );
                                        if resp
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .on_hover_text("Cuộn xuống tin nhắn mới nhất")
                                            .clicked()
                                        {
                                            self.force_scroll_to_bottom = true;
                                            ui.ctx().request_repaint();
                                        }
                                    });
                            });
                    }
                    if jump_scrolled {
                        if let Some(jump) = &mut self.quote_jump {
                            jump.scroll_pending = false;
                        }
                    }
                    if widen_window {
                        self.chat_render_window.1 += CHAT_RENDER_STEP;
                    }
                    if let Some(msg) = custom_toast {
                        self.show_toast(msg);
                    } else if load_older_clicked {
                        self.show_toast("Đang kiểm tra tin nhắn cũ từ máy chủ Zalo…");
                    }
                    for id in select_toggles {
                        if !self.selected_msgs.remove(&id) {
                            self.selected_msgs.insert(id);
                        }
                    }
                    if let Some((idx, action)) = pending_action {
                        let target = self
                            .threads
                            .get(self.selected_thread_index)
                            .and_then(|t| t.messages.get(idx))
                            .cloned();
                        if let Some(m) = target {
                            match action {
                                BubbleAction::Reply => self.replying_to = Some(m),
                                BubbleAction::JumpToQuoted(reference) => {
                                    let index = self
                                        .threads
                                        .get(self.selected_thread_index)
                                        .and_then(|thread| {
                                            crate::message_navigation::unique_reference_index(&thread.messages, &reference)
                                        });
                                    if index.is_some() {
                                        self.filter_starred_only = false;
                                        self.in_chat_search_open = false;
                                        self.in_chat_search_query.clear();
                                        self.quote_jump = Some(QuoteJump {
                                            thread_id: thread_id.clone(),
                                            reference,
                                            highlight_until: std::time::Instant::now() + std::time::Duration::from_secs(3),
                                            scroll_pending: true,
                                        });
                                        ctx.request_repaint();
                                    } else {
                                        self.show_toast("Chưa có tin nhắn gốc đã tải với ID duy nhất. Hãy tải thêm lịch sử rồi thử lại.");
                                    }
                                }
                                BubbleAction::Forward => {
                                    self.forward_msg = Some(m);
                                    self.forward_filter.clear();
                                }
                                BubbleAction::OpenSenderProfile => {
                                    // 1:1: the other person is the thread; group: the sender id.
                                    let (uid, name, avatar) = if !is_group {
                                        let t = self.threads.get(self.selected_thread_index);
                                        (
                                            thread_id.clone(),
                                            t.map(|t| t.name.clone()).unwrap_or_default(),
                                            t.and_then(|t| t.avatar.clone()),
                                        )
                                    } else {
                                        let uid = crate::chat::sidebar_sender_id(&m)
                                            .or_else(|| m.sender.bytes().all(|b| b.is_ascii_digit()).then(|| m.sender.clone()))
                                            .or_else(|| {
                                                self.group_members.get(&thread_id).and_then(|ms| {
                                                    ms.iter().find(|x| x.name == m.sender).map(|x| x.id.clone())
                                                })
                                            })
                                            .unwrap_or_default();
                                        let avatar = self.group_members.get(&thread_id).and_then(|ms| {
                                            ms.iter().find(|x| x.id == uid).and_then(|x| x.avatar.clone())
                                        });
                                        (uid, m.sender.clone(), avatar)
                                    };
                                    if uid.is_empty() {
                                        self.show_toast("Chưa xác định được tài khoản người gửi.");
                                    } else {
                                        self.open_account_card(uid, name, avatar);
                                    }
                                }
                                BubbleAction::SelectMany => {
                                    self.select_mode = true;
                                    self.selected_msgs.clear();
                                    self.selected_msgs.insert(m.id.clone());
                                }
                                BubbleAction::ShowDetails => {
                                    self.msg_details = Some(m);
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
                                        speaker_view: false,
                                        pinned_tile_idx: 0,
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
                                BubbleAction::RecallForEveryone => {
                                    if let Some(real) = self
                                        .threads
                                        .get_mut(self.selected_thread_index)
                                        .and_then(|t| t.messages.get_mut(idx))
                                    {
                                        real.text = "[Tin nhắn đã thu hồi]".into();
                                        real.sticker_id = None;
                                        real.link_url = None;
                                        real.link_thumb = None;
                                    }
                                    self.persist_threads_async();
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::RecallMessage {
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
                                    let hd_tex_key = self.request_hd_texture(&full_url);
                                    self.image_viewer = Some(ImageViewerModal {
                                        tex_key,
                                        hd_tex_key,
                                        full_url,
                                        title,
                                        sender,
                                        time,
                                        zoom: 1.0,
                                        pan: Vec2::ZERO,
                                    });
                                }
                                BubbleAction::OpenFile(path) | BubbleAction::RevealFile(path)
                                    if !std::path::Path::new(&path).exists() =>
                                {
                                    // Moved or deleted since it was saved: forget it so the
                                    // card offers "Lưu về máy" again.
                                    self.settings.downloaded_files.retain(|_, p| p != &path);
                                    if let Some(url) = m.link_url.clone() {
                                        crate::gui::set_downloaded_path(&url, None);
                                    }
                                    self.settings.save();
                                    self.show_toast("Tệp đã bị xoá hoặc di chuyển, hãy lưu lại");
                                }
                                BubbleAction::OpenFile(path) => open_local_path(&path, false),
                                BubbleAction::RevealFile(path) => open_local_path(&path, true),
                                BubbleAction::DownloadFile { url, file_name } => {
                                    self.show_toast(format!(
                                        "⏳ Đang tải tập tin \"{file_name}\"…"
                                    ));
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::DownloadFile {
                                            url,
                                            file_name,
                                            download_dir: self.settings.download_dir.clone(),
                                        });
                                    }
                                }
                            }
                        }
                    }

                    // 3. Optional Sticker Picker Drawer
                    if self.sticker_picker_open {
                        let r = ui
                            .scope(|ui| self.render_sticker_picker(ui, &thread_id, is_group, pal))
                            .response
                            .rect;
                        self.overlay_rects_last.push(r);
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
                            // Rounded floating card with a quote bar, matching the composer blocks.
                            let card = egui::Frame::none()
                                .fill(composer_block_fill(pal))
                                .shadow(composer_block_shadow(pal))
                                .rounding(Rounding::same(18.0))
                                .outer_margin(Margin { left: 8.0, right: 8.0, top: 4.0, bottom: 2.0 })
                                .inner_margin(Margin { left: 18.0, right: 10.0, top: 7.0, bottom: 7.0 })
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
                                                let (xr, xresp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                                                if xresp.hovered() {
                                                    ui.painter().circle_filled(xr.center(), 13.0, pal.bg_hover);
                                                }
                                                draw_lucide_icon(ui.painter(), xr.center(), lucide::X, 14.0, pal.text_secondary);
                                                if xresp
                                                    .on_hover_cursor(CursorIcon::PointingHand)
                                                    .on_hover_text("Hủy trả lời (Esc)")
                                                    .clicked()
                                                {
                                                    cancel = true;
                                                }
                                            },
                                        );
                                    });
                                });
                            // The response rect includes the outer margin (8 left, 4 top, 2 bottom).
                            let r = card.response.rect;
                            self.overlay_rects_last.push(r);
                            let bar = Rect::from_min_size(
                                Pos2::new(r.left() + 8.0 + 8.0, r.top() + 4.0 + 9.0),
                                Vec2::new(3.0, (r.height() - 6.0 - 18.0).max(8.0)),
                            );
                            ui.painter().rect_filled(bar, Rounding::same(1.5), pal.primary);
                            if ui.input(|i| i.key_pressed(Key::Escape)) {
                                cancel = true;
                            }
                        }
                        if cancel {
                            self.replying_to = None;
                        }
                    }
                    // 3c. Clipboard Image Preview Banner (`Ctrl+V` / `Shift+Insert` screenshot or image file paste)
                    let paste_now = is_clipboard_paste_triggered(ctx);
                    // Files copied in Finder take priority: the pasteboard also carries their
                    // icon as an image, which must not be sent instead of the file.
                    let pasted_files = if paste_now {
                        read_clipboard_file_paths()
                    } else {
                        Vec::new()
                    };
                    if !pasted_files.is_empty() {
                        ctx.input_mut(|i| {
                            i.events.retain(|e| !matches!(e, egui::Event::Paste(_)));
                        });
                        let mut added = 0;
                        for p in pasted_files {
                            if !self.pending_paste_files.contains(&p) {
                                self.pending_paste_files.push(p);
                                added += 1;
                            }
                        }
                        self.composer_focus_pending = true;
                        let _ = added; // the tray itself shows the new file(s)
                    } else if paste_now {
                        if let Some((path, w, h, col_img)) = read_clipboard_image_to_png() {
                            // Consume any text Paste event on this frame so the composer doesn't paste a file path
                            ctx.input_mut(|i| {
                                i.events.retain(|e| !matches!(e, egui::Event::Paste(_)));
                            });
                            let tex = ctx.load_texture(
                                format!("clip_preview_{}", crate::login::unix_now()),
                                col_img,
                                TextureOptions::LINEAR,
                            );
                            self.pending_clipboard_image = Some((path, w, h, tex));
                            self.composer_focus_pending = true;
                        }
                    }
                    // 3c-0. Multi-select action bar.
                    if self.select_mode {
                        self.render_selection_bar(ui, &thread_id, is_group, pal);
                    }
                    // 3c-1. Staged attachments (pasted or dropped files, clipboard image): one
                    // card each, thumbnail with a corner ✕ and the file name underneath.
                    if !self.pending_paste_files.is_empty() || self.pending_clipboard_image.is_some() {
                        let mut remove: Option<usize> = None;
                        let mut clear_clip = false;
                        let esc = ui.input(|i| i.key_pressed(Key::Escape));
                        let strip = egui::Frame::none()
                            .outer_margin(Margin { left: 12.0, right: 12.0, top: 6.0, bottom: 2.0 })
                            .show(ui, |ui| {
                                // One fixed-height row that scrolls sideways (Zalo), so many
                                // files never push the chat up.
                                egui::ScrollArea::horizontal()
                                    .id_salt("staged_attachments")
                                    .auto_shrink([false, true])
                                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                    .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);
                                    if let Some((ref path, _, _, ref tex)) = self.pending_clipboard_image {
                                        let fname = path
                                            .file_name()
                                            .and_then(|n| n.to_str())
                                            .unwrap_or("clipboard.png")
                                            .to_string();
                                        if attachment_preview_card(ui, pal, Some(tex), &fname, true, "clip") {
                                            clear_clip = true;
                                        }
                                    }
                                    for (i, p) in self.pending_paste_files.iter().enumerate() {
                                        let name = p
                                            .file_name()
                                            .and_then(|n| n.to_str())
                                            .unwrap_or("tệp")
                                            .to_string();
                                        let is_img = is_image_path(p);
                                        let thumb_key = format!(
                                            "pfile_{}",
                                            crate::crypto::md5_hex(&p.display().to_string())
                                        );
                                        // Thumbnails load off the UI thread.
                                        if is_img
                                            && !self.sticker_textures.contains_key(&thumb_key)
                                            && self.loading_stickers.insert(thumb_key.clone())
                                        {
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::FetchStickerImage {
                                                    key: thumb_key.clone(),
                                                    url: p.display().to_string(),
                                                });
                                            }
                                        }
                                        let tex = self.sticker_textures.get(&thumb_key);
                                        if attachment_preview_card(ui, pal, tex, &name, is_img, &format!("pf{i}")) {
                                            remove = Some(i);
                                        }
                                    }
                                });
                                    });
                            });
                        self.overlay_rects_last.push(strip.response.rect);
                        if esc {
                            clear_clip = true;
                            self.pending_paste_files.clear();
                        } else if let Some(i) = remove {
                            self.pending_paste_files.remove(i);
                        }
                        if clear_clip {
                            self.pending_clipboard_image = None;
                        }
                    }

                    // 3d. Pending Scheduled Messages Banner (#38)
                    let thread_sched: Vec<crate::scheduler::ScheduledMessage> = self
                        .scheduled_items
                        .iter()
                        .filter(|s| s.thread_id == thread_id)
                        .cloned()
                        .collect();
                    if let Some(next_sched) = thread_sched.first() {
                        let mut open_sched_mgr = false;
                        let mut cancel_sched_id: Option<i64> = None;
                        egui::Frame::none()
                            .fill(pal.bg_elevated)
                            .inner_margin(Margin::symmetric(14.0, 5.0))
                            .stroke(Stroke::new(1.0_f32, pal.border))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "🕒 Hẹn giờ gửi ({}) · Tiếp theo lúc {}: \"{}\"",
                                            thread_sched.len(),
                                            crate::scheduler::format_schedule_time(
                                                next_sched.deliver_at
                                            ),
                                            truncate_chars(&next_sched.text, 36)
                                        ))
                                        .size(11.6)
                                        .color(pal.primary_hover)
                                        .strong(),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui.small_button("Hủy").clicked() {
                                            cancel_sched_id = Some(next_sched.id);
                                        }
                                        if ui.small_button("Quản lý").clicked() {
                                            open_sched_mgr = true;
                                        }
                                    });
                                });
                            });
                        if let Some(cid) = cancel_sched_id {
                            let _ =
                                crate::scheduler::cancel_scheduled_message("zalo_history.db", cid);
                            self.refresh_scheduled_items();
                            self.show_toast("Đã hủy tin nhắn hẹn giờ");
                        }
                        if open_sched_mgr {
                            self.schedule_modal_open = true;
                        }
                    }

                    // 4. Bottom Message Composer Bar (Compact Icon Actions + Spacious Input + Padded Send Button)
                    // `@` popup state computed inside the composer, drawn after it: (byte offset
                    // of `@`, candidates).
                    let mut mention_popup: Option<(usize, Vec<(String, String)>)> = None;
                    // No container behind the input pill (Telegram): it used to read as a
                    // translucent layer over the chat.
                    // Telegram: the composer floats over the bottom of the message list (the list
                    // runs underneath it) instead of cutting the chat with a straight edge.
                    let composer_slot = {
                        let r = ui.max_rect();
                        // Stacked mode (reply/tray/sticker picker): continue right after them.
                        let top = if list_under_composer {
                            (r.bottom() - composer_block_h).max(r.top())
                        } else {
                            ui.cursor().top()
                        };
                        Rect::from_min_max(Pos2::new(r.left(), top), r.right_bottom())
                    };
                    let composer_frame = ui
                        .allocate_new_ui(egui::UiBuilder::new().max_rect(composer_slot), |ui| {
                        // Three separate blocks (attach, text pill, send), each with its own
                        // solid fill and drop shadow; the row itself stays transparent.
                        let block_fill = composer_block_fill(pal);
                        let block_shadow = composer_block_shadow(pal);
                        egui::Frame::none()
                        // Closer to the window edges, like Telegram.
                        .outer_margin(Margin { left: 2.0, right: 2.0, top: 2.0, bottom: 4.0 })
                        .inner_margin(Margin::symmetric(6.0, 6.0))
                        .show(ui, |ui| {
                            // One centred capsule replacing the composer when the user may not post.
                            let notice_pill = |ui: &mut egui::Ui, icon: &str, text: &str| {
                                ui.vertical_centered(|ui| {
                                    egui::Frame::none()
                                        .fill(block_fill)
                                        .shadow(block_shadow)
                                        .rounding(Rounding::same(17.0))
                                        .inner_margin(Margin::symmetric(16.0, 9.0))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(format!("{icon}  {text}"))
                                                    .size(12.5)
                                                    .color(pal.text_secondary),
                                            );
                                        });
                                });
                            };
                            if is_group && !ctx.data(|d| d.get_temp::<bool>(Id::new(("posting_ready", &thread_id)))).unwrap_or(false) {
                                notice_pill(ui, lucide::REFRESH, "Đang kiểm tra quyền gửi tin của nhóm…");
                                return;
                            }
                            let posting_locked = is_group && self.threads.get(self.selected_thread_index).is_some_and(|t| !t.presentation.posting.allows(&self.user_uin));
                            if posting_locked {
                                self.sticker_picker_open = false;
                                self.attach_menu_open = false;
                                self.emoji_picker_open = false;
                                notice_pill(ui, lucide::LOCK, "Chỉ trưởng/phó cộng đồng được gửi tin nhắn vào cộng đồng.");
                                return;
                            }
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

                            // 4a-0. `@` member picker for groups. The query is the text after the
                            // last `@` that starts a word; Enter/Tab/arrows drive the popup while
                            // it is open, so they are taken out of the event queue here.
                            let mention_query: Option<(usize, String)> =
                                if is_group && !self.ime_active {
                                    let txt = &self.input_message;
                                    txt.rfind('@').and_then(|at| {
                                        let word_start =
                                            at == 0 || txt[..at].ends_with(char::is_whitespace);
                                        let q = &txt[at + 1..];
                                        let completed = self
                                            .composer_mentions
                                            .iter()
                                            .any(|(_, n)| q.starts_with(n.as_str()));
                                        (word_start
                                            && !completed
                                            && !q.contains('\n')
                                            && q.chars().count() <= 30)
                                            .then(|| (at, q.to_string()))
                                    })
                                } else {
                                    None
                                };
                            let mention_candidates: Vec<(String, String)> =
                                match &mention_query {
                                    Some((_, q)) => self.mention_candidates(q),
                                    None => Vec::new(),
                                };
                            let mention_active = !mention_candidates.is_empty();
                            let mut mention_pick: Option<(String, String)> = None;
                            if mention_active {
                                let last = mention_candidates.len() - 1;
                                self.mention_selected = self.mention_selected.min(last);
                                let (up, down, pick) = ui.input(|i| {
                                    (
                                        i.key_pressed(Key::ArrowUp),
                                        i.key_pressed(Key::ArrowDown),
                                        (i.key_pressed(Key::Enter) && !i.modifiers.shift)
                                            || i.key_pressed(Key::Tab),
                                    )
                                });
                                if up {
                                    self.mention_selected =
                                        self.mention_selected.checked_sub(1).unwrap_or(last);
                                }
                                if down {
                                    self.mention_selected =
                                        if self.mention_selected >= last { 0 } else { self.mention_selected + 1 };
                                }
                                if pick {
                                    mention_pick =
                                        mention_candidates.get(self.mention_selected).cloned();
                                }
                                ui.input_mut(|i| {
                                    i.events.retain(|ev| match ev {
                                        egui::Event::Key { key, pressed: true, .. } => !matches!(
                                            key,
                                            Key::ArrowUp | Key::ArrowDown | Key::Tab | Key::Enter
                                        ),
                                        egui::Event::Text(t) => {
                                            !(pick && (t == "\n" || t == "\r" || t == "\t"))
                                        }
                                        _ => true,
                                    });
                                });
                            } else {
                                self.mention_selected = 0;
                            }
                            if let (Some((uid, name)), Some((at, _))) =
                                (mention_pick.take(), mention_query.as_ref())
                            {
                                self.insert_mention(ctx, &thread_id, *at, uid, name);
                            } else if let (true, Some((at, _))) =
                                (mention_active, mention_query.as_ref())
                            {
                                mention_popup = Some((*at, mention_candidates.clone()));
                            }

                            let is_ime_busy = self.ime_active || self.ime_just_committed;
                            let is_cmd_enter_mode = self.settings.send_message_key == "cmd_enter";
                            // Return that committed a Telex word counts as Enter (it used to need
                            // a second press: the first only removed the underline).
                            let ime_enter = std::mem::take(&mut self.ime_enter_commit);
                            let (send_via_enter, insert_newline_via_enter) = ui.input(|i| {
                                let ent = (i.key_pressed(Key::Enter) && !is_ime_busy) || ime_enter;
                                let cmd_or_ctrl = i.modifiers.command || i.modifiers.ctrl;
                                if is_cmd_enter_mode {
                                    (ent && cmd_or_ctrl, ent && !cmd_or_ctrl)
                                } else {
                                    (ent && !i.modifiers.shift, ent && i.modifiers.shift)
                                }
                            });
                            if send_via_enter || insert_newline_via_enter {
                                ui.input_mut(|i| {
                                    i.events.retain(|ev| {
                                        !matches!(
                                            ev,
                                            egui::Event::Key {
                                                key: Key::Enter,
                                                pressed: true,
                                                ..
                                            }
                                        ) && !matches!(
                                            ev,
                                            egui::Event::Text(t)
                                                if t == "\n" || t == "\r" || t == "\r\n"
                                        )
                                    });
                                });
                            }

                            // Telegram layout: attach circle, text pill and send capsule are
                                    // separate blocks (backgrounds painted once their rects are known).
                            egui::Frame::none()
                                .inner_margin(Margin::symmetric(2.0, 2.0))
                                .show(ui, |ui| {
                                    let attach_bg_idx = ui.painter().add(egui::Shape::Noop);
                                    let input_bg_idx = ui.painter().add(egui::Shape::Noop);
                                    let mut attach_rect_for_bg = Rect::NOTHING;
                                    let mut send_rect_for_bg = Rect::NOTHING;
                                    let mut send_clicked = false;
                                    let mut send_selected_sticker: Option<
                                        crate::chat::StickerItem,
                                    > = None;
                                    let composer_id =
                                        Id::new(("chat_composer_input", &thread_id));

                                    let response = ui
                                        .horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 10.0;

                                            // Telegram-style single attach button; photo, file, sticker and
                                            // scheduled send live in its menu.
                                            // Telegram size: 34 pt blocks.
                                            let (att_rect, att_resp) = ui.allocate_exact_size(
                                                Vec2::new(34.0, 34.0),
                                                Sense::click(),
                                            );
                                            attach_rect_for_bg = att_rect;
                                            let att_hot = self.attach_menu_open || att_resp.hovered();
                                            if att_hot {
                                                ui.painter().circle_filled(att_rect.center(), 17.0, pal.bg_hover);
                                            }
                                            draw_icon_attachment(
                                                ui.painter(),
                                                att_rect.center(),
                                                15.0,
                                                if att_hot { pal.primary_hover } else { pal.text_secondary },
                                            );
                                            if att_resp
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .on_hover_text("Đính kèm")
                                                .clicked()
                                            {
                                                self.attach_menu_open = !self.attach_menu_open;
                                            }
                                            if self.attach_menu_open {
                                                let mut pick: Option<usize> = None;
                                                let at = appear(ui.ctx(), Id::new("composer_attach_menu"), 0.16);
                                                let menu = egui::Area::new(Id::new("composer_attach_menu"))
                                                    .order(egui::Order::Foreground)
                                                    .pivot(egui::Align2::LEFT_BOTTOM)
                                                    .fixed_pos(att_rect.left_top() + Vec2::new(-6.0, -10.0 + (1.0 - at) * 10.0))
                                                    .show(ui.ctx(), |ui| {
                                                        ui.set_opacity(at);
                                                        egui::Frame::popup(ui.style())
                                                            .rounding(Rounding::same(16.0))
                                                            .inner_margin(Margin::same(6.0))
                                                            .show(ui, |ui| {
                                                                ui.set_min_width(210.0);
                                                                ui.set_max_width(240.0);
                                                                let items = [
                                                                    (lucide::IMAGE, "Ảnh hoặc Video"),
                                                                    (lucide::FILE, "Tệp tin"),
                                                                    (lucide::STICKER, "Nhãn dán"),
                                                                    (lucide::CLOCK, "Hẹn giờ gửi"),
                                                                ];
                                                                for (i, (icon, label)) in items.into_iter().enumerate() {
                                                                    if menu_item(ui, icon, label, None) {
                                                                        pick = Some(i);
                                                                    }
                                                                }
                                                            });
                                                    });
                                                let outside = ui.input(|i| {
                                                    i.pointer.any_pressed()
                                                        && i.pointer.interact_pos().is_some_and(|q| {
                                                            !menu.response.rect.contains(q) && !att_rect.contains(q)
                                                        })
                                                });
                                                if pick.is_some() || outside || ui.input(|i| i.key_pressed(Key::Escape)) {
                                                    self.attach_menu_open = false;
                                                }
                                                match pick {
                                                    Some(i @ (0 | 1)) => {
                                                        let cap = self.input_message.trim().to_string();
                                                        self.input_message.clear();
                                                        if let Some(tx) = WORKER_TX.get() {
                                                            let _ = tx.send(WorkerCmd::PickAndSendAttachment {
                                                                to_id: thread_id.clone(),
                                                                is_group,
                                                                images_only: i == 0,
                                                                caption: cap,
                                                                preset_path: None,
                                                            });
                                                        }
                                                    }
                                                    Some(2) => {
                                                        self.sticker_picker_open = !self.sticker_picker_open;
                                                        // Open on "Gần đây" when there is history.
                                                        if self.sticker_picker_open
                                                            && !self.settings.recent_stickers.is_empty()
                                                        {
                                                            self.sticker_tab = StickerTab::Recent;
                                                        }
                                                    }
                                                    Some(3) => self.schedule_modal_open = !self.schedule_modal_open,
                                                    _ => {}
                                                }
                                            }

                                            let is_multiline = self.input_message.contains('\n');
                                            let vis_lines = if is_multiline {
                                                self.input_message
                                                    .split('\n')
                                                    .count()
                                                    .clamp(2, 5)
                                            } else {
                                                1
                                            };
                                            let box_h = if is_multiline {
                                                14.0 + (vis_lines as f32 * 20.0)
                                            } else {
                                                34.0
                                            };

                                            // Inner padding of the separate text pill.
                                            ui.add_space(4.0);

                                            let send_btn_w =
                                                if is_narrow_w { 52.0 } else { 76.0 };
                                            let _ = send_btn_w;
                                            // Text pill holds the input plus the emoji button;
                                            // the round send button sits outside it.
                                            let input_w = (ui.available_width()
                                                - 34.0 // send circle
                                                - 28.0 // emoji button
                                                - 3.0 * 10.0)
                                                .max(50.0);
                                            let hint_str = format!(
                                                "Nhập tin nhắn tới {}...",
                                                truncate_chars(&thread_name, 18)
                                            );
                                            let composing = self.ime_active;
                                            let ime_line = pal.text_primary;
                                            // Emoji are laid out as transparent square slots and
                                            // painted in colour afterwards (egui fonts draw them
                                            // as monochrome glyphs or boxes).
                                            let mut emoji_layouter = |ui: &egui::Ui, text: &str, wrap_width: f32| {
                                                let font = egui::TextStyle::Body.resolve(ui.style());
                                                let color = ui
                                                    .visuals()
                                                    .override_text_color
                                                    .unwrap_or_else(|| ui.visuals().widgets.inactive.text_color());
                                                let mut job = egui::text::LayoutJob::default();
                                                super::emoji_picker::append_text_with_emoji(&mut job, ui.ctx(), text, font, color);
                                                job.wrap.max_width = wrap_width;
                                                ui.fonts(|f| f.layout_job(job))
                                            };
                                            let resp = if is_multiline {
                                                ui.allocate_ui_with_layout(
                                                    Vec2::new(input_w, box_h),
                                                    Layout::top_down(Align::Min),
                                                    |ui| {
                                                        ui.set_min_size(Vec2::new(input_w, box_h));
                                                        ui.set_max_size(Vec2::new(input_w, box_h));
                                                        egui::ScrollArea::vertical()
                                                            .id_salt((
                                                                "composer_fixed_scroll",
                                                                &thread_id,
                                                            ))
                                                            .max_width(input_w)
                                                            .max_height(box_h)
                                                            .auto_shrink([false, false])
                                                            .stick_to_bottom(true)
                                                            .show(ui, |ui| {
                                                                let out = show_text_edit_ime_out(
                                                                    ui,
                                                                    egui::TextEdit::multiline(
                                                                        &mut self.input_message,
                                                                    )
                                                                    .layouter(&mut emoji_layouter)
                                                                    .id(composer_id)
                                                                    .desired_rows(vis_lines)
                                                                    .frame(false)
                                                                    .hint_text(RichText::new(hint_str).color(pal.text_muted.gamma_multiply(0.75)))
                                                                    .margin(Margin::symmetric(
                                                                        6.0, 6.0,
                                                                    ))
                                                                    .desired_width(input_w - 10.0),
                                                                    composing,
                                                                    ime_line,
                                                                );
                                                                self.overlay_color_emoji(ui, &out, pal.bg_elevated, pal);
                                                                out.response
                                                            })
                                                            .inner
                                                    },
                                                )
                                                .inner
                                            } else {
                                                ui.allocate_ui_with_layout(
                                                    Vec2::new(input_w, 34.0),
                                                    Layout::left_to_right(Align::Center),
                                                    |ui| {
                                                        let out = show_text_edit_ime_out(
                                                            ui,
                                                            egui::TextEdit::singleline(
                                                                &mut self.input_message,
                                                            )
                                                            .layouter(&mut emoji_layouter)
                                                            .id(composer_id)
                                                            .frame(false)
                                                            .hint_text(RichText::new(hint_str).color(pal.text_muted.gamma_multiply(0.75)))
                                                            .margin(Margin::symmetric(6.0, 7.0))
                                                            .desired_width(input_w),
                                                            composing,
                                                            ime_line,
                                                        );
                                                        self.overlay_color_emoji(ui, &out, pal.bg_elevated, pal);
                                                        out.response
                                                    },
                                                )
                                                .inner
                                            };
                                            if resp.changed()
                                                && !self.input_message.trim().is_empty()
                                                && !self.settings.block_typing
                                            {
                                                let should_emit = self
                                                    .last_typing_sent_at
                                                    .as_ref()
                                                    .map(|(last_tid, t)| {
                                                        last_tid != &thread_id
                                                            || t.elapsed()
                                                                >= std::time::Duration::from_millis(
                                                                    3200,
                                                                )
                                                    })
                                                    .unwrap_or(true);
                                                if should_emit {
                                                    self.last_typing_sent_at = Some((
                                                        thread_id.clone(),
                                                        std::time::Instant::now(),
                                                    ));
                                                    if let Some(tx) = WORKER_TX.get() {
                                                        let _ = tx.send(WorkerCmd::SendTyping {
                                                            thread_id: thread_id.clone(),
                                                            is_group,
                                                        });
                                                    }
                                                }
                                            }

                                            // Emoji button inside the text pill (Telegram).
                                            {
                                                let (er, eresp) = ui.allocate_exact_size(
                                                    Vec2::splat(28.0),
                                                    Sense::click(),
                                                );
                                                let hot = self.emoji_picker_open || eresp.hovered();
                                                if hot {
                                                    ui.painter().circle_filled(er.center(), 14.0, pal.bg_hover);
                                                }
                                                draw_lucide_icon(
                                                    ui.painter(),
                                                    er.center(),
                                                    lucide::SMILE,
                                                    17.0,
                                                    if hot { pal.text_primary } else { pal.text_secondary },
                                                );
                                                self.emoji_btn_rect = er;
                                                if eresp
                                                    .on_hover_text("Biểu tượng cảm xúc")
                                                    .on_hover_cursor(CursorIcon::PointingHand)
                                                    .clicked()
                                                {
                                                    self.emoji_picker_open = !self.emoji_picker_open;
                                                    if self.emoji_picker_open && self.settings.recent_emojis.is_empty() {
                                                        self.emoji_tab = 1;
                                                    }
                                                }
                                            }

                                            // Send Button
                                            let can_send = !self.input_message.trim().is_empty()
                                                || self.pending_clipboard_image.is_some()
                                                || !self.pending_paste_files.is_empty();
                                            let now_t = ui.input(|i| i.time);
                                            let glass_state = self.send_fx_state(&thread_id, now_t);
                                            // Solid round send button with an up arrow (iMessage-
                                            // like); spinner while sending, tick when sent.
                                            let (send_rect, send_resp) = ui.allocate_exact_size(
                                                Vec2::splat(34.0),
                                                if can_send { Sense::click() } else { Sense::hover() },
                                            );
                                            {
                                                let hot = send_resp.hovered() && can_send;
                                                let fill = if can_send {
                                                    if hot { pal.text_primary.gamma_multiply(0.85) } else { pal.text_primary }
                                                } else {
                                                    pal.bg_elevated
                                                };
                                                let fg = if can_send {
                                                    if pal.is_dark { Color32::BLACK } else { Color32::WHITE }
                                                } else {
                                                    pal.text_muted
                                                };
                                                let c = send_rect.center();
                                                let grow = ui.ctx().animate_bool_with_time(
                                                    send_resp.id.with("press"),
                                                    send_resp.is_pointer_button_down_on(),
                                                    0.08,
                                                );
                                                let rad = 17.0 - 1.5 * grow;
                                                ui.painter().add(block_shadow.as_shape(
                                                    Rect::from_center_size(c, Vec2::splat(rad * 2.0)),
                                                    Rounding::same(rad),
                                                ));
                                                ui.painter().circle_filled(c, rad, if can_send { fill } else { block_fill });
                                                match glass_state {
                                                    GlassState::Load => {
                                                        let t = ui.input(|i| i.time) as f32;
                                                        let a0 = t * 6.0;
                                                        let pts: Vec<Pos2> = (0..=18)
                                                            .map(|k| {
                                                                let a = a0 + k as f32 * 0.25;
                                                                c + Vec2::new(a.cos(), a.sin()) * 8.0
                                                            })
                                                            .collect();
                                                        ui.painter().add(egui::Shape::line(pts, Stroke::new(2.0_f32, fg)));
                                                        ui.ctx().request_repaint();
                                                    }
                                                    GlassState::Ok => {
                                                        draw_lucide_icon(ui.painter(), c, "\u{e06c}", 16.0, fg);
                                                    }
                                                    GlassState::Idle => {
                                                        draw_lucide_icon(ui.painter(), c, "\u{e04a}", 17.0, fg);
                                                    }
                                                }
                                            }
                                            let send_resp = send_resp.on_hover_text("Gửi (chuột phải: hẹn giờ gửi)");
                                            send_rect_for_bg = send_resp.rect;
                                            if send_resp.secondary_clicked() {
                                                self.schedule_modal_open = true;
                                            }
                                            send_clicked = send_resp
                                                .on_hover_cursor(if can_send {
                                                    CursorIcon::PointingHand
                                                } else {
                                                    CursorIcon::Default
                                                })
                                                .clicked()
                                                && glass_state != GlassState::Load;
                                            resp
                                        })
                                        .inner;
                                    {
                                        // Separate block backgrounds (Telegram): circle behind the
                                        // attach button, rounded pill behind the text field.
                                        let row = ui.min_rect();
                                        if attach_rect_for_bg.is_positive() {
                                            ui.painter().set(
                                                attach_bg_idx,
                                                egui::Shape::Vec(vec![
                                                    egui::Shape::Rect(block_shadow.as_shape(
                                                        Rect::from_center_size(attach_rect_for_bg.center(), Vec2::splat(34.0)),
                                                        Rounding::same(17.0),
                                                    )),
                                                    egui::Shape::circle_filled(attach_rect_for_bg.center(), 17.0, block_fill),
                                                ]),
                                            );
                                        }
                                        let right = if send_rect_for_bg.is_positive() {
                                            send_rect_for_bg.left() - 6.0
                                        } else {
                                            row.right()
                                        };
                                        // Same 44 pt height as the attach circle and send
                                        // capsule; only a multi-line draft makes it taller.
                                        let pill = Rect::from_min_max(
                                            Pos2::new(attach_rect_for_bg.right() + 6.0, row.top()),
                                            Pos2::new(right, row.bottom()),
                                        );
                                        if pill.is_positive() {
                                            let r = Rounding::same((pill.height() / 2.0).min(17.0));
                                            ui.painter().set(
                                                input_bg_idx,
                                                egui::Shape::Vec(vec![
                                                    egui::Shape::Rect(block_shadow.as_shape(pill, r)),
                                                    egui::Shape::Rect(egui::epaint::RectShape::filled(pill, r, block_fill)),
                                                ]),
                                            );
                                        }
                                    }

                                    if is_ime_busy && response.lost_focus() {
                                        response.request_focus();
                                    }
                                    if insert_newline_via_enter
                                        && (response.has_focus() || response.lost_focus())
                                    {
                                        if let Some(mut st) =
                                            egui::TextEdit::load_state(ui.ctx(), composer_id)
                                        {
                                            let char_idx = st
                                                .cursor
                                                .char_range()
                                                .map(|r| r.primary.index)
                                                .unwrap_or_else(|| {
                                                    self.input_message.chars().count()
                                                });
                                            let byte_idx = self
                                                .input_message
                                                .char_indices()
                                                .nth(char_idx)
                                                .map(|(i, _)| i)
                                                .unwrap_or(self.input_message.len());
                                            self.input_message.insert(byte_idx, '\n');
                                            let next_c = egui::text::CCursor::new(char_idx + 1);
                                            st.cursor.set_char_range(Some(
                                                egui::text::CCursorRange::one(next_c),
                                            ));
                                            st.store(ui.ctx(), composer_id);
                                        } else {
                                            self.input_message.push('\n');
                                        }
                                        response.request_focus();
                                    }
                                    if response.changed() {
                                        ensure_cjk_font(
                                            ui.ctx(),
                                            std::iter::once(self.input_message.as_str()),
                                        );
                                    }
                                    if self.composer_focus_pending {
                                        response.request_focus();
                                        self.composer_focus_pending = false;
                                    }
                                    response.context_menu(|ui| {
                                        if ui
                                            .button(format!(
                                                "{} Dán hình ảnh từ Clipboard (Ctrl+V)",
                                                lucide::IMAGE
                                            ))
                                            .clicked()
                                        {
                                            ui.close_menu();
                                            if let Some((path, w, h, col_img)) =
                                                read_clipboard_image_to_png()
                                            {
                                                let tex = ctx.load_texture(
                                                    format!(
                                                        "clip_preview_{}",
                                                        crate::login::unix_now()
                                                    ),
                                                    col_img,
                                                    TextureOptions::LINEAR,
                                                );
                                                self.pending_clipboard_image =
                                                    Some((path, w, h, tex));
                                                self.composer_focus_pending = true;
                                            } else {
                                                self.show_toast(
                                                    "Không tìm thấy hình ảnh nào trong Clipboard",
                                                );
                                            }
                                        }
                                    });

                                    if !contextual_stickers.is_empty() && response.has_focus() {
                                        let len = contextual_stickers.len();
                                        if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
                                            self.composer_sticker_selected = Some(0);
                                        } else if let Some(sel) = self.composer_sticker_selected {
                                            if ui.input(|i| i.key_pressed(Key::ArrowRight)) {
                                                self.composer_sticker_selected =
                                                    Some((sel + 1) % len);
                                            } else if ui.input(|i| i.key_pressed(Key::ArrowLeft)) {
                                                self.composer_sticker_selected =
                                                    Some((sel + len - 1) % len);
                                            } else if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                                                self.composer_sticker_selected = None;
                                            }
                                        }
                                    } else {
                                        self.composer_sticker_selected = None;
                                    }

                                    let enter_pressed = (response.lost_focus()
                                        || response.has_focus())
                                        && send_via_enter
                                        && (!is_ime_busy || ime_enter);
                                    if enter_pressed {
                                        if let Some(sel) = self.composer_sticker_selected {
                                            if let Some(st) = contextual_stickers.get(sel) {
                                                send_selected_sticker = Some(st.clone());
                                            }
                                        }
                                    }
                                    let can_send = !self.input_message.trim().is_empty()
                                        || self.pending_clipboard_image.is_some()
                                        || !self.pending_paste_files.is_empty();

                                    if let Some(chosen) = send_selected_sticker {
                                        self.input_message.clear();
                                        self.dismissed_sticker_suggest_for.clear();
                                        self.composer_sticker_selected = None;
                                        response.request_focus();
                                        if let Some(tx) = WORKER_TX.get() {
                                            let _ = tx.send(WorkerCmd::SendSticker {
                                                to_id: thread_id.clone(),
                                                sticker_id: chosen.sticker_id,
                                                cate_id: chosen.cate_id,
                                                sticker_type: chosen.sticker_type,
                                                is_group,
                                            });
                                        }
                                    } else if (send_clicked || enter_pressed)
                                        && !self.pending_paste_files.is_empty()
                                    {
                                        self.start_send_fx(ctx, &thread_id);
                                        // Text (if any) goes first as its own message, then each
                                        // pasted file — like Zalo's staged attachments.
                                        let text = self.input_message.trim().to_string();
                                        self.input_message.clear();
                                        self.dismissed_sticker_suggest_for.clear();
                                        self.composer_sticker_selected = None;
                                        response.request_focus();
                                        let files = std::mem::take(&mut self.pending_paste_files);
                                        let tags = std::mem::take(&mut self.composer_mentions);
                                        if let Some(tx) = WORKER_TX.get() {
                                            if !text.is_empty() {
                                                let _ = tx.send(if is_group && !tags.is_empty() {
                                                    WorkerCmd::SendGroupMention {
                                                        to_id: thread_id.clone(),
                                                        text,
                                                        tags,
                                                    }
                                                } else {
                                                    WorkerCmd::SendMessage {
                                                        to_id: thread_id.clone(),
                                                        text,
                                                        is_group,
                                                    }
                                                });
                                            }
                                            for path in files {
                                                let _ = tx.send(WorkerCmd::PickAndSendAttachment {
                                                    to_id: thread_id.clone(),
                                                    is_group,
                                                    images_only: is_image_path(&path),
                                                    caption: String::new(),
                                                    preset_path: Some(path),
                                                });
                                            }
                                        }
                                    } else if (send_clicked || enter_pressed)
                                        && self.pending_clipboard_image.is_some()
                                    {
                                        self.start_send_fx(ctx, &thread_id);
                                        if let Some((clip_path, _, _, _)) =
                                            self.pending_clipboard_image.take()
                                        {
                                            let cap = self.input_message.trim().to_string();
                                            self.input_message.clear();
                                            self.dismissed_sticker_suggest_for.clear();
                                            self.composer_sticker_selected = None;
                                            response.request_focus();
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::PickAndSendAttachment {
                                                    to_id: thread_id.clone(),
                                                    is_group,
                                                    images_only: true,
                                                    caption: cap,
                                                    preset_path: Some(clip_path),
                                                });
                                            }
                                        }
                                    } else if (send_clicked || enter_pressed) && can_send {
                                        self.start_send_fx(ctx, &thread_id);
                                        let text = self.input_message.trim().to_string();
                                        self.input_message.clear();
                                        self.dismissed_sticker_suggest_for.clear();
                                        self.composer_sticker_selected = None;
                                        response.request_focus();
                                        let quoted = self.replying_to.take();
                                        if let Some(tx) = WORKER_TX.get() {
                                            if let Some(quoted) = quoted {
                                                let _ = tx.send(WorkerCmd::SendQuote {
                                                    to_id: thread_id.clone(),
                                                    text,
                                                    quoted,
                                                    is_group,
                                                });
                                            } else if is_group && !self.composer_mentions.is_empty() {
                                                let _ = tx.send(WorkerCmd::SendGroupMention {
                                                    to_id: thread_id.clone(),
                                                    text,
                                                    tags: std::mem::take(&mut self.composer_mentions),
                                                });
                                            } else {
                                                let _ = tx.send(WorkerCmd::SendMessage {
                                                    to_id: thread_id.clone(),
                                                    text,
                                                    is_group,
                                                });
                                            }
                                        }
                                        self.composer_mentions.clear();
                                    } else if enter_pressed {
                                        response.request_focus();
                                    }
                                });
                        })
                        })
                        .inner;

                    self.composer_rect_last = composer_frame.response.rect;
                    if self.emoji_picker_open {
                        let anchor = self.emoji_btn_rect;
                        if let Some(e) = self.render_emoji_picker(ctx, pal, anchor) {
                            self.insert_into_composer(ctx, &thread_id, &e);
                        }
                    }
                    // 4a-1. `@` member popup above the composer.
                    if let Some((at, candidates)) = mention_popup {
                        if let Some((uid, name)) = self.render_mention_popup(
                            ctx,
                            composer_frame.response.rect,
                            &candidates,
                            pal,
                        ) {
                            self.insert_mention(ctx, &thread_id, at, uid, name);
                        }
                    }
                    // 4b. Floating Contextual Sticker Suggestion Popover (`egui::Area`, rendered AFTER composer bar)
                    if !contextual_stickers.is_empty() && !self.sticker_picker_open {
                        self.render_contextual_sticker_popover(
                            ctx,
                            composer_frame.response.rect,
                            &contextual_stickers,
                            &thread_id,
                            is_group,
                            pal,
                        );
                    }
                    if let Some(ref link_meta) = composer_link_preview {
                        if contextual_stickers.is_empty() && !self.sticker_picker_open {
                            self.render_composer_link_preview_popover(
                                ctx,
                                composer_frame.response.rect,
                                link_meta,
                                pal,
                            );
                        }
                    }
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(120.0);
                        ui.label(
                            RichText::new("Chọn một hội thoại bên trái để bắt đầu trò chuyện")
                                .size(14.0)
                                .color(pal.text_muted),
                        );
                    });
                }
            });

        self.render_profile_popover(ctx, pal);
        self.render_account_card(ctx, pal);
        self.render_message_details(ctx, pal);
        self.render_wipe_confirm(ctx, pal);
        self.render_alias_editor(ctx, pal);
        self.render_leave_group_confirm(ctx, pal);
        self.render_toast_overlay(ctx, pal);
        self.render_forward_dialog(ctx, pal);
        self.render_call_overlay(ctx, pal);
        self.render_image_viewer_modal(ctx, pal);
        self.render_phone_lookup_modal(ctx, pal);
        self.render_schedule_modal(ctx, pal);
        self.render_settings_modal(ctx, pal);
    }
}

/// Highlight the exact resolved row and optionally center it in the message viewport.
fn render_quote_target(ui: &egui::Ui, rect: egui::Rect, scroll_pending: bool, color: Color32) {
    ui.painter().rect_stroke(
        rect.expand(2.0),
        Rounding::same(8.0),
        Stroke::new(2.0_f32, color),
    );
    if scroll_pending {
        ui.scroll_to_rect(rect, Some(Align::Center));
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(100));
}

#[cfg(test)]
mod quote_navigation_tests {
    use super::*;

    #[test]
    fn quote_navigation_invalidates_when_thread_changes_or_highlight_expires() {
        let messages = [ChatMessage {
            id: "original".into(),
            ..Default::default()
        }];
        let mut jump = QuoteJump {
            thread_id: "thread-1".into(),
            reference: "original".into(),
            highlight_until: std::time::Instant::now() + std::time::Duration::from_secs(3),
            scroll_pending: true,
        };
        assert_eq!(jump.target_index("thread-1", &messages), Some(0));
        assert_eq!(jump.target_index("thread-2", &messages), None);
        jump.highlight_until = std::time::Instant::now();
        assert_eq!(jump.target_index("thread-1", &messages), None);
    }

    #[test]
    fn quote_navigation_keeps_animated_jump_visible_after_highlight_expires() {
        for (count, target) in [(1000, 12), (1000, 900), (150, 12)] {
            let ctx = egui::Context::default();
            let color = Color32::from_rgb(1, 2, 3);
            let mut before = 0.0;
            let mut after = 0.0;
            let mut highlighted = false;
            let mut target_visible = false;
            for frame in 0..10 {
                let jumping = frame > 0;
                let highlighting = jumping && frame < 7;
                let limit = if jumping {
                    crate::message_navigation::render_limit_for_target(150, count, target)
                } else {
                    150
                };
                if count == 1000 && target == 12 {
                    assert!(jumping || count - limit > target);
                }
                let output = ctx.run(
                    egui::RawInput {
                        time: Some(frame as f64 * 0.5),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(400.0, 300.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let scroll = egui::ScrollArea::vertical()
                                .id_salt("quote_jump_test")
                                .max_height(200.0)
                                .stick_to_bottom(!highlighting)
                                .show(ui, |ui| {
                                    for index in count - limit..count {
                                        let (rect, _) = ui.allocate_exact_size(
                                            Vec2::new(200.0, 30.0),
                                            Sense::hover(),
                                        );
                                        if jumping && index == target {
                                            if highlighting {
                                                render_quote_target(ui, rect, frame == 1, color);
                                                highlighted = true;
                                            }
                                            if frame == 9 {
                                                target_visible =
                                                    ui.clip_rect().contains(rect.center());
                                            }
                                        }
                                    }
                                });
                            if jumping {
                                after = scroll.state.offset.y;
                            } else {
                                before = scroll.state.offset.y;
                            }
                        });
                    },
                );
                if frame == 4 {
                    assert!(output.shapes.iter().any(|shape| {
                    matches!(&shape.shape, egui::Shape::Rect(rect) if rect.stroke.color == color)
                }), "Exact target must have a visible highlight shape");
                }
            }
            assert!(highlighted);
            assert!(
                before > 1000.0,
                "Initial viewport must start at the newest messages"
            );
            assert!(
                after < before && target_visible,
                "Jump must center the older target, not remain at the bottom: {after}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// `@` mentions in the group composer
// ---------------------------------------------------------------------------
impl ZaloGuiApp {
    /// Members matching `query` for the open group: known members first, then people who have
    /// written in the thread (newest first). Excludes yourself; at most 60 rows.
    pub(super) fn mention_candidates(&self, query: &str) -> Vec<(String, String)> {
        let Some(t) = self.threads.get(self.selected_thread_index) else {
            return Vec::new();
        };
        let q_lower = query.to_lowercase();
        let q_folded = fold_vn_search(query);
        // Full list from `FetchGroupMembers` when loaded, else the sidebar's avatar sample.
        let full: Vec<(String, String)> = self
            .group_members
            .get(&t.id)
            .map(|v| v.iter().map(|m| (m.id.clone(), m.name.clone())).collect())
            .unwrap_or_default();
        let members = full.into_iter().chain(
            t.presentation
                .members
                .iter()
                .map(|m| (m.id.clone(), m.name.clone())),
        );
        let senders = t
            .messages
            .iter()
            .rev()
            .filter(|m| !m.is_outgoing)
            .filter_map(|m| crate::chat::sidebar_sender_id(m).map(|id| (id, m.sender.clone())));
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        // Zalo lists "Báo cho cả nhóm · @All" first.
        if matches_vn_query("All", &q_lower, &q_folded)
            || matches_vn_query("Báo cho cả nhóm", &q_lower, &q_folded)
        {
            out.push((crate::chat::MENTION_ALL_UID.to_string(), "All".to_string()));
        }
        for (id, name) in members.chain(senders) {
            let name = name.trim().to_string();
            if id.is_empty()
                || id == self.user_uin
                || name.is_empty()
                || name == "0"
                || !seen.insert(id.clone())
            {
                continue;
            }
            // Show (and insert) the alias you set for that person, like Zalo; the real name
            // still matches the query.
            let alias = crate::gui::alias_for(&id);
            let shown = alias.clone().unwrap_or_else(|| name.clone());
            if matches_vn_query(&shown, &q_lower, &q_folded)
                || (alias.is_some() && matches_vn_query(&name, &q_lower, &q_folded))
            {
                out.push((id, shown));
                if out.len() >= 60 {
                    break;
                }
            }
        }
        out
    }

    /// Replace the `@query` that starts at byte `at` with `@Name `, remember the tag and put
    /// the caret at the end of the composer text.
    pub(super) fn insert_mention(
        &mut self,
        ctx: &egui::Context,
        thread_id: &str,
        at: usize,
        uid: String,
        name: String,
    ) {
        if at > self.input_message.len() || !self.input_message.is_char_boundary(at) {
            return;
        }
        self.input_message.truncate(at);
        self.input_message.push('@');
        self.input_message.push_str(&name);
        self.input_message.push(' ');
        self.composer_mentions.push((uid, name));
        self.mention_selected = 0;
        self.composer_focus_pending = true;
        let id = Id::new(("chat_composer_input", thread_id));
        if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
            let end = self.input_message.chars().count();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(end),
                )));
            state.store(ctx, id);
        }
    }

    /// Floating list above the composer; returns the member clicked this frame.
    pub(super) fn render_mention_popup(
        &mut self,
        ctx: &egui::Context,
        composer_rect: Rect,
        candidates: &[(String, String)],
        pal: &ThemePalette,
    ) -> Option<(String, String)> {
        let mut picked = None;
        let at = appear(ctx, Id::new("composer_mention_popup"), 0.14);
        egui::Area::new(Id::new("composer_mention_popup"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(composer_rect.left_top() + Vec2::new(10.0, -6.0 + (1.0 - at) * 8.0))
            .show(ctx, |ui| {
                ui.set_opacity(at);
                egui::Frame::none()
                    .fill(pal.bg_elevated)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .rounding(Rounding::same(if pal.is_glass { 14.0 } else { 8.0 }))
                    .shadow(ctx.style().visuals.popup_shadow)
                    .inner_margin(Margin::same(6.0))
                    .show(ui, |ui| {
                        ui.set_width(300.0);
                        // Zalo-style hint row.
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
                            ui.painter().circle_filled(
                                r.center(),
                                13.0,
                                pal.warning.linear_multiply(0.85),
                            );
                            draw_lucide_icon(
                                ui.painter(),
                                r.center(),
                                lucide::INFO,
                                14.0,
                                Color32::WHITE,
                            );
                            ui.label(
                                RichText::new("Nhấn ↑ ↓ để chọn, Enter để dùng")
                                    .size(12.0)
                                    .color(pal.text_secondary),
                            );
                        });
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(2.0);
                        egui::ScrollArea::vertical()
                            .max_height(272.0)
                            .show(ui, |ui| {
                                for (i, (uid, name)) in candidates.iter().enumerate() {
                                    let selected = i == self.mention_selected;
                                    let (rect, resp) = ui.allocate_exact_size(
                                        Vec2::new(ui.available_width(), 40.0),
                                        Sense::click(),
                                    );
                                    if selected || resp.hovered() {
                                        ui.painter().rect_filled(
                                            rect,
                                            Rounding::same(8.0),
                                            if selected {
                                                pal.bg_selected
                                            } else {
                                                pal.bg_hover
                                            },
                                        );
                                    }
                                    let c = rect.left_center() + Vec2::new(20.0, 0.0);
                                    let text_x = rect.left() + 44.0;
                                    let mut real_name: Option<String> = None;
                                    if uid == crate::chat::MENTION_ALL_UID {
                                        ui.painter().circle_filled(c, 15.0, pal.primary);
                                        ui.painter().text(
                                            c,
                                            egui::Align2::CENTER_CENTER,
                                            "@",
                                            FontId::proportional(16.0),
                                            Color32::WHITE,
                                        );
                                        let g = ui.painter().text(
                                            Pos2::new(text_x, rect.center().y),
                                            egui::Align2::LEFT_CENTER,
                                            "Báo cho cả nhóm",
                                            FontId::proportional(13.0),
                                            pal.text_primary,
                                        );
                                        ui.painter().text(
                                            Pos2::new(g.right() + 6.0, rect.center().y),
                                            egui::Align2::LEFT_CENTER,
                                            "· @All",
                                            FontId::proportional(12.5),
                                            pal.primary_hover,
                                        );
                                    } else {
                                        let member = self
                                            .group_members
                                            .values()
                                            .flatten()
                                            .find(|m| &m.id == uid);
                                        let ava = member.and_then(|m| m.avatar.clone());
                                        real_name =
                                            member.map(|m| m.name.clone()).filter(|n| n != name);
                                        paint_user_avatar(
                                            ui.painter(),
                                            c,
                                            15.0,
                                            name,
                                            false,
                                            ava.as_deref(),
                                            &self.sticker_textures,
                                            &mut self.loading_stickers,
                                        );
                                        ui.painter().text(
                                            Pos2::new(text_x, rect.center().y),
                                            egui::Align2::LEFT_CENTER,
                                            truncate_chars(name, 30),
                                            FontId::proportional(13.0),
                                            pal.text_primary,
                                        );
                                    }
                                    let resp = match real_name {
                                        Some(real) => resp.on_hover_text(real),
                                        None => resp,
                                    };
                                    if selected {
                                        resp.scroll_to_me(None);
                                    }
                                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                        picked = Some((uid.clone(), name.clone()));
                                    }
                                }
                            });
                    });
            });
        picked
    }
}

impl ZaloGuiApp {
    /// Render icon-only navigation tabs inside the visible list card.
    pub(super) fn render_nav_tabbar(
        &mut self,
        ui: &mut egui::Ui,
        _ctx: &egui::Context,
        pal: &ThemePalette,
    ) {
        let active_tab = if self.settings_modal_open {
            2
        } else if self.contacts_view_open {
            1
        } else {
            0
        };
        let unread: usize = self.unread_counts.values().sum();
        let has_update = self.update_info.as_ref().is_some_and(|u| u.has_update);
        let tabs = [
            (
                "Tin nhắn",
                draw_icon_chat_nav as fn(&egui::Painter, Pos2, f32, Color32),
            ),
            ("Danh bạ", draw_icon_contacts),
            ("Cài đặt", draw_icon_settings),
        ];
        let mut clicked_tab = None;

        egui::TopBottomPanel::bottom(ui.id().with("nav_tabbar"))
            .show_separator_line(false)
            .frame(egui::Frame::none().inner_margin(Margin {
                left: 6.0,
                right: 6.0,
                top: 6.0,
                bottom: 8.0,
            }))
            .show_inside(ui, |ui| {
                let bounds = ui.max_rect();
                ui.painter().hline(
                    bounds.x_range(),
                    bounds.top() - 6.0,
                    Stroke::new(1.0_f32, pal.border.linear_multiply(0.6)),
                );
                ui.spacing_mut().item_spacing.x = 0.0;
                let tab_width = ui.available_width() / tabs.len() as f32;
                ui.horizontal(|ui| {
                    for (index, (label, draw_icon)) in tabs.iter().enumerate() {
                        let selected = index == active_tab;
                        let (rect, response) =
                            ui.allocate_exact_size(Vec2::new(tab_width, 42.0), Sense::click());
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::SelectableLabel,
                                true,
                                selected,
                                *label,
                            )
                        });
                        let color = if selected {
                            pal.primary
                        } else if response.hovered() || response.has_focus() {
                            pal.text_primary
                        } else {
                            pal.text_secondary
                        };
                        if response.hovered() || response.has_focus() {
                            ui.painter().rect_filled(
                                rect.shrink2(Vec2::new(3.0, 0.0)),
                                Rounding::same(9.0),
                                pal.bg_hover,
                            );
                        }
                        let icon_center = Pos2::new(rect.center().x, rect.center().y);
                        draw_icon(ui.painter(), icon_center, 21.0, color);
                        if index == 0 && unread > 0 {
                            let text = if unread > 99 {
                                "99+".to_owned()
                            } else {
                                unread.to_string()
                            };
                            let width = 15.0 + 5.0 * text.len().saturating_sub(1) as f32;
                            let badge = Rect::from_center_size(
                                icon_center + Vec2::new(11.0, -8.0),
                                Vec2::new(width, 16.0),
                            );
                            ui.painter().rect_filled(
                                badge,
                                Rounding::same(8.0),
                                Color32::from_rgb(239, 56, 56),
                            );
                            ui.painter().rect_stroke(
                                badge,
                                Rounding::same(8.0),
                                Stroke::new(1.2_f32, pal.bg_sidebar),
                            );
                            ui.painter().text(
                                badge.center(),
                                egui::Align2::CENTER_CENTER,
                                text,
                                FontId::proportional(10.0),
                                Color32::WHITE,
                            );
                        }
                        if index == 2 && has_update {
                            ui.painter().circle_filled(
                                icon_center + Vec2::new(10.0, -9.0),
                                3.0,
                                pal.accent_online,
                            );
                        }
                        let tooltip = match index {
                            1 => {
                                let friends = self
                                    .threads
                                    .iter()
                                    .filter(|t| !t.is_group && !t.name.contains("Cloud của tôi"))
                                    .count();
                                let groups = self.threads.iter().filter(|t| t.is_group).count();
                                format!("Danh bạ ({friends} bạn bè · {groups} nhóm)")
                            }
                            2 => match self.update_info.as_ref().filter(|u| u.has_update) {
                                Some(update) => format!(
                                    "Cài đặt · Có bản cập nhật mới v{}",
                                    update.latest_version
                                ),
                                None => format!("Cài đặt (v{})", env!("CARGO_PKG_VERSION")),
                            },
                            _ => (*label).to_owned(),
                        };
                        if response
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .on_hover_text(tooltip)
                            .clicked()
                        {
                            clicked_tab = Some(index);
                        }
                    }
                });
            });

        match clicked_tab {
            Some(0) => {
                self.settings_modal_open = false;
                self.feed_modal_open = false;
                self.contacts_view_open = false;
            }
            Some(1) => {
                self.settings_modal_open = false;
                self.feed_modal_open = false;
                self.contacts_view_open = true;
            }
            Some(2) => {
                self.settings_modal_open = !self.settings_modal_open;
                if self.settings_modal_open && has_update {
                    self.settings_tab = SettingsTab::Update;
                }
            }
            _ => {}
        }
    }
}

impl ZaloGuiApp {
    /// Select the "Cloud của tôi" thread (leaving contacts/feed) and load its history.
    pub(super) fn open_cloud_thread(&mut self) {
        self.feed_modal_open = false;
        self.contacts_view_open = false;
        if let Some((cidx, cthread)) = self
            .threads
            .iter()
            .enumerate()
            .find(|(_, t)| t.name.contains("Cloud của tôi"))
        {
            let cid = cthread.id.clone();
            self.selected_thread_index = cidx;
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::LoadGroupHistory {
                    group_id: cid,
                    is_group: false,
                    have: 0,
                });
            }
        }
    }
}

impl ZaloGuiApp {
    /// Glass send-button state for `thread_id`; "ok" lasts 2 s (1 s with reduced motion, as in
    /// the original), and a send that never reports back is released after 20 s.
    pub(super) fn send_fx_state(&mut self, thread_id: &str, now: f64) -> GlassState {
        let ok_hold = if reduce_motion() { 1.0 } else { 2.0 };
        match &self.send_fx {
            Some((tid, st, since)) if tid == thread_id => {
                let age = now - since;
                match st {
                    GlassState::Ok if age > ok_hold => {
                        self.send_fx = None;
                        GlassState::Idle
                    }
                    GlassState::Load if age > 20.0 => {
                        self.send_fx = None;
                        GlassState::Idle
                    }
                    s => *s,
                }
            }
            _ => GlassState::Idle,
        }
    }

    /// Mark a composer send for `thread_id` as in flight.
    pub(super) fn start_send_fx(&mut self, ctx: &egui::Context, thread_id: &str) {
        self.send_fx = Some((
            thread_id.to_string(),
            GlassState::Load,
            ctx.input(|i| i.time),
        ));
    }

    /// Server answered: success → ✓, failure → back to idle (the error toast says why).
    pub(super) fn finish_send_fx(&mut self, ctx: &egui::Context, thread_id: &str, ok: bool) {
        if let Some((tid, GlassState::Load, _)) = &self.send_fx {
            if tid == thread_id {
                self.send_fx = if ok {
                    Some((thread_id.to_string(), GlassState::Ok, ctx.input(|i| i.time)))
                } else {
                    None
                };
            }
        }
    }
}

// ---------------------------------------------------------------------------
// "Chọn nhiều tin nhắn" and "Xem chi tiết"
// ---------------------------------------------------------------------------
impl ZaloGuiApp {
    /// Bar shown while multi-selecting: count, Copy, Chia sẻ (single), Xóa phía tôi, Hủy.
    pub(super) fn render_selection_bar(
        &mut self,
        ui: &mut egui::Ui,
        thread_id: &str,
        is_group: bool,
        pal: &ThemePalette,
    ) {
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.select_mode = false;
            self.selected_msgs.clear();
            return;
        }
        let chosen: Vec<ChatMessage> = self
            .threads
            .get(self.selected_thread_index)
            .map(|t| {
                t.messages
                    .iter()
                    .filter(|m| self.selected_msgs.contains(&m.id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let n = chosen.len();
        let (mut copy, mut share, mut delete, mut cancel) = (false, false, false, false);
        egui::Frame::none()
            .fill(pal.bg_elevated)
            .rounding(Rounding::same(18.0))
            .outer_margin(Margin::symmetric(10.0, 4.0))
            .inner_margin(Margin::symmetric(14.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Đã chọn {n} tin nhắn"))
                            .size(13.0)
                            .strong()
                            .color(pal.text_primary),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        cancel = ui.button("Hủy").clicked();
                        delete = ui
                            .add_enabled(
                                n > 0,
                                egui::Button::new(
                                    RichText::new(format!("{} Xóa phía tôi", lucide::TRASH))
                                        .color(Color32::from_rgb(220, 60, 60)),
                                ),
                            )
                            .clicked();
                        share = ui
                            .add_enabled(
                                n == 1,
                                egui::Button::new(format!("{} Chia sẻ", lucide::FORWARD)),
                            )
                            .on_disabled_hover_text("Chia sẻ từng tin nhắn một")
                            .clicked();
                        copy = ui
                            .add_enabled(n > 0, egui::Button::new(format!("{} Copy", lucide::COPY)))
                            .clicked();
                    });
                });
            });
        if copy {
            let text = chosen
                .iter()
                .map(|m| m.text.trim())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            ui.ctx().copy_text(text);
            self.show_toast(format!("Đã copy {n} tin nhắn"));
        }
        if share {
            if let Some(m) = chosen.first() {
                self.forward_msg = Some(m.clone());
                self.forward_filter.clear();
            }
        }
        if delete {
            if let Some(tx) = WORKER_TX.get() {
                for m in &chosen {
                    let _ = tx.send(WorkerCmd::DeleteForMe {
                        to_id: thread_id.to_string(),
                        is_group,
                        msg: m.clone(),
                    });
                }
            }
            self.show_toast(format!("Đã xóa {n} tin nhắn ở phía bạn"));
            cancel = true;
        }
        if cancel {
            self.select_mode = false;
            self.selected_msgs.clear();
        }
    }

    /// "Chi tiết tin nhắn" dialog: sender, time, ids, kind and status.
    pub(super) fn render_message_details(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        let Some(m) = self.msg_details.clone() else {
            return;
        };
        let sent = chrono_like_datetime(m.ts);
        let screen = ctx.screen_rect();
        // Rounded card sized to the window (an `egui::Window` grew with long text and spilled
        // past the app edges at the fixed 118% zoom).
        let fade = appear(ctx, Id::new("msg_details"), 0.18);
        let backdrop = egui::Area::new(Id::new("msg_details_backdrop"))
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
        let card_w = 380.0_f32.min(screen.width() - 32.0);
        let body_max_h = (screen.height() - 140.0).max(120.0);
        egui::Area::new(Id::new("msg_details_card"))
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
                        let inner_w = card_w - 36.0;
                        ui.set_width(inner_w);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Chi tiết tin nhắn")
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
                                {
                                    close = true;
                                }
                            });
                        });
                        ui.add_space(10.0);
                        egui::ScrollArea::vertical()
                            .id_salt("msg_details_scroll")
                            .max_height(body_max_h)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.set_max_width(inner_w);
                                let who = if m.is_outgoing {
                                    "Bạn".to_string()
                                } else {
                                    m.sender.clone()
                                };
                                let mut rows: Vec<(&str, String)> = vec![
                                    ("Người gửi", who),
                                    ("Thời gian", sent),
                                    ("Loại", format!("{:?}", m.kind)),
                                    ("Mã tin nhắn", m.id.clone()),
                                ];
                                if !m.client_message_id.is_empty() {
                                    rows.push(("Mã phía máy", m.client_message_id.clone()));
                                }
                                if m.is_outgoing {
                                    rows.push(("Trạng thái", "Đã gửi".into()));
                                }
                                if m.is_pinned {
                                    rows.push(("Ghim", "Đã ghim trên máy này".into()));
                                }
                                let key_w = 96.0;
                                for (k, v) in rows {
                                    ui.horizontal_top(|ui| {
                                        ui.add_sized(
                                            [key_w, 18.0],
                                            egui::Label::new(
                                                RichText::new(k).color(pal.text_muted),
                                            ),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(v).color(pal.text_primary),
                                            )
                                            .wrap(),
                                        );
                                    });
                                    ui.add_space(6.0);
                                }
                                if !m.text.trim().is_empty() {
                                    ui.separator();
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(truncate_chars(m.text.trim(), 400))
                                                .color(pal.text_secondary),
                                        )
                                        .wrap(),
                                    );
                                }
                            });
                    });
            });
        if close || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.msg_details = None;
        }
    }
}

/// Local "dd/mm/yyyy HH:MM" for a message timestamp.
fn chrono_like_datetime(ts: i64) -> String {
    crate::clock::local_datetime(ts)
        .map(|d| d.format("%d/%m/%Y %H:%M").to_string())
        .unwrap_or_else(|| "—".into())
}

/// Show a text field so macOS Telex's word-in-progress (IME preedit) looks native: no
/// selection box (egui keeps the preedit selected so the next update replaces it), and a thin
/// underline beneath the composed characters like AppKit's marked text.
pub(super) fn show_text_edit_ime(
    ui: &mut egui::Ui,
    edit: egui::TextEdit<'_>,
    composing: bool,
    line_col: Color32,
) -> egui::Response {
    show_text_edit_ime_out(ui, edit, composing, line_col).response
}

/// [`show_text_edit_ime`] returning the full output (galley + position) for overlays.
pub(super) fn show_text_edit_ime_out(
    ui: &mut egui::Ui,
    edit: egui::TextEdit<'_>,
    composing: bool,
    line_col: Color32,
) -> egui::text_edit::TextEditOutput {
    let saved = ui.visuals().selection.bg_fill;
    if composing {
        ui.visuals_mut().selection.bg_fill = Color32::TRANSPARENT;
    }
    let out = edit.show(ui);
    ui.visuals_mut().selection.bg_fill = saved;
    if composing && out.response.has_focus() {
        if let Some(range) = out.cursor_range {
            let [a, b] = range.sorted_cursors();
            let painter = ui.painter().with_clip_rect(out.text_clip_rect);
            for i in a.ccursor.index..b.ccursor.index {
                let r0 = out.galley.pos_from_ccursor(egui::text::CCursor::new(i));
                let r1 = out.galley.pos_from_ccursor(egui::text::CCursor::new(i + 1));
                if (r0.min.y - r1.min.y).abs() < 0.5 {
                    let y = out.galley_pos.y + r0.max.y - 0.5;
                    painter.hline(
                        (out.galley_pos.x + r0.min.x)..=(out.galley_pos.x + r1.min.x),
                        y,
                        Stroke::new(1.0_f32, line_col),
                    );
                }
            }
        }
    }
    out
}

/// Conversation actions shared by the chat-list context menu and the info drawer.
#[derive(Clone)]
pub(super) enum ThreadCtxAction {
    TogglePin(String, bool, bool),
    SetLabel(String, Option<String>),
    ToggleUnread(String, bool, bool),
    SetMute(String, bool, Option<i64>, String),
    ToggleHide(String, bool, bool),
    OpenSchedule(usize),
    DeleteConv(usize, String, String, bool, String),
    /// Open the "Rời nhóm" confirmation for (group id, group name).
    AskLeaveGroup(String, String),
}

impl ZaloGuiApp {
    /// Drops a conversation from the list, the local store and the pin list, and tombstones it
    /// so the worker's stale copy cannot bring the old messages back (delete / leave group).
    pub(super) fn remove_thread_locally(&mut self, tid: &str) {
        if let Ok(mut store) = crate::store::MessageStore::open("zalo_history.db") {
            let _ = store.delete_thread(tid);
        }
        if let Some(pos) = self.threads.iter().position(|t| t.id == tid) {
            let selected_id = self
                .threads
                .get(self.selected_thread_index)
                .map(|t| t.id.clone());
            self.threads.remove(pos);
            self.selected_thread_index = selected_id
                .and_then(|id| self.threads.iter().position(|t| t.id == id))
                .unwrap_or(0)
                .min(self.threads.len().saturating_sub(1));
        }
        self.unread_counts.remove(tid);
        self.settings.pinned_threads.retain(|x| x != tid);
        self.settings
            .deleted_threads
            .insert(tid.to_string(), crate::login::unix_now() * 1000);
        self.settings.save();
        self.persist_threads_async();
    }

    /// Zalo-style "Rời nhóm" dialog: warning text, a "leave silently" switch and the
    /// destructive button. Sends `WorkerCmd::LeaveGroup`; the row goes once the server agrees.
    pub(super) fn render_leave_group_confirm(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        let Some((tid, name, silent)) = self.leave_group_confirm.clone() else {
            return;
        };
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.leave_group_confirm = None;
            return;
        }
        let screen = ctx.screen_rect();
        let fade = appear(ctx, Id::new("leave_group_confirm"), 0.18);
        let backdrop = egui::Area::new(Id::new("leave_group_backdrop"))
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
        let mut confirm = false;
        let mut silent_now = silent;
        let card_w = 380.0_f32.min(screen.width() - 32.0);
        egui::Area::new(Id::new("leave_group_card"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::new(0.0, (1.0 - fade) * 16.0))
            .show(ctx, |ui| {
                ui.set_opacity(fade);
                egui::Frame::none()
                    .fill(pal.bg_elevated)
                    .rounding(Rounding::same(18.0))
                    .shadow(ctx.style().visuals.window_shadow)
                    .inner_margin(Margin::symmetric(18.0, 16.0))
                    .show(ui, |ui| {
                        ui.set_width(card_w - 36.0);
                        ui.label(RichText::new("Rời nhóm").size(15.0).strong().color(pal.text_primary));
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!(
                                "Bạn có chắc muốn rời nhóm \"{}\"? Bạn sẽ không nhận được tin nhắn mới từ nhóm này nữa.",
                                truncate_chars(&name, 40)
                            ))
                            .size(13.0)
                            .color(pal.text_secondary),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Nếu bạn là trưởng nhóm, hãy chuyển quyền trưởng nhóm cho người khác trước.")
                                .size(11.5)
                                .color(pal.text_muted),
                        );
                        ui.add_space(12.0);
                        let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(row, Rounding::same(10.0), pal.bg_hover);
                        }
                        ui.painter().text(
                            row.left_top() + Vec2::new(8.0, 4.0),
                            egui::Align2::LEFT_TOP,
                            "Rời nhóm trong im lặng",
                            FontId::proportional(13.0),
                            pal.text_primary,
                        );
                        ui.painter().text(
                            row.left_top() + Vec2::new(8.0, 22.0),
                            egui::Align2::LEFT_TOP,
                            "Chỉ trưởng và phó nhóm biết bạn đã rời",
                            FontId::proportional(11.0),
                            pal.text_muted,
                        );
                        // Small switch on the right.
                        let sw = Rect::from_center_size(row.right_center() - Vec2::new(26.0, 0.0), Vec2::new(36.0, 20.0));
                        let t = ctx.animate_bool_with_time(Id::new("leave_silent_sw"), silent_now, 0.15);
                        ui.painter().rect_filled(sw, Rounding::same(10.0), pal.border.lerp_to_gamma(pal.text_primary, t));
                        let knob_x = egui::lerp(sw.left() + 10.0..=sw.right() - 10.0, t);
                        ui.painter().circle_filled(Pos2::new(knob_x, sw.center().y), 8.0, Color32::WHITE);
                        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                            silent_now = !silent_now;
                        }
                        ui.add_space(14.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("Rời nhóm").size(13.0).color(Color32::WHITE).strong())
                                        .fill(pal.danger)
                                        .rounding(Rounding::same(10.0))
                                        .min_size(Vec2::new(96.0, 32.0)),
                                )
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                confirm = true;
                            }
                            ui.add_space(6.0);
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("Huỷ").size(13.0).color(pal.text_secondary))
                                        .fill(pal.bg_hover)
                                        .rounding(Rounding::same(10.0))
                                        .min_size(Vec2::new(80.0, 32.0)),
                                )
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                close = true;
                            }
                        });
                    });
            });
        if confirm {
            self.leave_group_confirm = None;
            if let Some(tx) = WORKER_TX.get() {
                self.leaving_groups.insert(tid.clone());
                let _ = tx.send(WorkerCmd::LeaveGroup {
                    thread_id: tid,
                    name,
                    silent: silent_now,
                });
                self.show_toast("Đang rời nhóm…");
            }
        } else if close {
            self.leave_group_confirm = None;
        } else if let Some(c) = self.leave_group_confirm.as_mut() {
            c.2 = silent_now;
        }
    }

    pub(super) fn apply_thread_ctx_action(
        &mut self,
        ctx: &egui::Context,
        act: ThreadCtxAction,
        now_unix: i64,
    ) {
        match act {
            ThreadCtxAction::TogglePin(tid, is_group, pin) => {
                if pin {
                    if !self.settings.pinned_threads.contains(&tid) {
                        self.settings.pinned_threads.push(tid.clone());
                    }
                    self.show_toast("📌 Đã ghim hội thoại");
                } else {
                    self.settings.pinned_threads.retain(|x| x != &tid);
                    self.show_toast("Đã bỏ ghim hội thoại");
                }
                if let Some(th) = self.threads.iter_mut().find(|t| t.id == tid) {
                    th.presentation.pinned = pin;
                }
                self.settings.save();
                self.persist_threads_async();
                if let Some(tx) = WORKER_TX.get() {
                    let _ = tx.send(WorkerCmd::PinThread {
                        thread_id: tid,
                        is_group,
                        pin,
                    });
                }
            }
            ThreadCtxAction::SetLabel(tid, lbl_opt) => {
                if let Some(lbl) = lbl_opt {
                    self.settings.thread_labels.insert(tid, lbl.clone());
                    self.show_toast(format!("🏷️ Đã phân loại: {lbl}"));
                } else {
                    self.settings.thread_labels.remove(&tid);
                    self.show_toast("Đã bỏ thẻ phân loại");
                }
                self.settings.save();
            }
            ThreadCtxAction::ToggleUnread(tid, is_group, unread) => {
                if unread {
                    self.unread_counts.insert(tid.clone(), 1);
                    self.show_toast("Đã đánh dấu chưa đọc");
                } else {
                    self.unread_counts.remove(&tid);
                    self.show_toast("Đã đánh dấu đã đọc");
                }
                let total_unread: usize = self.unread_counts.values().sum();
                let title = if total_unread > 0 {
                    format!("({total_unread}) Zalo Lite — Native Rust Client")
                } else {
                    "Zalo Lite — Native Rust Client".to_string()
                };
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
                if let Some(tx) = WORKER_TX.get() {
                    let _ = tx.send(WorkerCmd::MarkThreadUnread {
                        thread_id: tid,
                        is_group,
                        unread,
                    });
                }
            }
            ThreadCtxAction::SetMute(tid, is_group, dur_opt, toast_txt) => {
                let (dur_sec, mute) = match dur_opt {
                    Some(-1) => {
                        self.settings.muted_threads.insert(tid.clone(), -1);
                        (-1, true)
                    }
                    Some(d) => {
                        self.settings
                            .muted_threads
                            .insert(tid.clone(), now_unix + d);
                        (d, true)
                    }
                    None => {
                        self.settings.muted_threads.remove(&tid);
                        (-1, false)
                    }
                };
                if let Some(th) = self.threads.iter_mut().find(|t| t.id == tid) {
                    th.presentation.muted = mute;
                    th.unread.muted = mute;
                    th.unread.muted_until = if mute && dur_sec > 0 {
                        Some(now_unix + dur_sec)
                    } else {
                        None
                    };
                    th.unread.settings_fresh = true;
                    th.unread.fresh = true;
                }
                self.settings.save();
                self.persist_threads_async();
                self.show_toast(toast_txt);
                if let Some(tx) = WORKER_TX.get() {
                    let _ = tx.send(WorkerCmd::MuteThread {
                        thread_id: tid,
                        is_group,
                        duration_sec: dur_sec,
                        mute,
                    });
                }
            }
            ThreadCtxAction::ToggleHide(tid, is_group, hide) => {
                if hide {
                    self.settings.hidden_threads.insert(tid.clone());
                    self.show_toast("🙈 Đã ẩn trò chuyện (gõ tên vào ô Tìm kiếm để mở lại)");
                } else {
                    self.settings.hidden_threads.remove(&tid);
                    self.show_toast("Đã bỏ ẩn trò chuyện");
                }
                if let Some(th) = self.threads.iter_mut().find(|t| t.id == tid) {
                    th.presentation.hidden = hide;
                }
                self.settings.save();
                self.persist_threads_async();
                if let Some(tx) = WORKER_TX.get() {
                    let _ = tx.send(WorkerCmd::HideThread {
                        thread_id: tid,
                        is_group,
                        hide,
                    });
                }
            }
            ThreadCtxAction::OpenSchedule(idx) => {
                self.selected_thread_index = idx;
                self.schedule_modal_open = true;
            }
            ThreadCtxAction::AskLeaveGroup(tid, tname) => {
                self.leave_group_confirm = Some((tid, tname, false));
            }
            ThreadCtxAction::DeleteConv(_idx, tid, tname, is_group, last_msg_id) => {
                self.remove_thread_locally(&tid);
                self.show_toast(format!("🗑️ Đã xóa hội thoại \"{tname}\""));
                if let Some(tx) = WORKER_TX.get() {
                    let _ = tx.send(WorkerCmd::DeleteThread {
                        thread_id: tid,
                        is_group,
                        last_cli_msg_id: last_msg_id,
                    });
                }
            }
        }
    }
}

/// Open a saved file, or (`reveal`) select it in Finder / Explorer. The launcher process is
/// spawned and not waited on, so the UI thread never blocks.
pub(super) fn open_local_path(path: &str, reveal: bool) {
    #[cfg(target_os = "macos")]
    let cmd = if reveal {
        std::process::Command::new("open")
            .args(["-R", path])
            .spawn()
    } else {
        std::process::Command::new("open").arg(path).spawn()
    };
    #[cfg(target_os = "windows")]
    let cmd = if reveal {
        std::process::Command::new("explorer")
            .arg(format!("/select,{path}"))
            .spawn()
    } else {
        std::process::Command::new("explorer").arg(path).spawn()
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = {
        let target = if reveal {
            std::path::Path::new(path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string())
        } else {
            path.to_string()
        };
        std::process::Command::new("xdg-open").arg(target).spawn()
    };
    if let Err(e) = cmd {
        crate::logger::log_error("gui::open_local_path", &e.to_string());
    }
}

/// Title-bar behaviour for a drag area: drag moves the window, double-click zooms.
pub(super) fn window_drag_from(ctx: &egui::Context, resp: &egui::Response) {
    if resp.double_clicked() {
        let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
    } else if resp.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

/// Char ranges (byte offsets) where `q` (already lower-case) occurs in `text`, ignoring case.
pub(super) fn match_ranges(text: &str, q: &str) -> Vec<(usize, usize)> {
    let q_chars: Vec<char> = q.chars().collect();
    if q_chars.is_empty() {
        return Vec::new();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|(_, c)| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + q_chars.len() <= lower.len() {
        if lower[i..i + q_chars.len()] == q_chars[..] {
            let start = chars[i].0;
            let end = chars
                .get(i + q_chars.len())
                .map(|(b, _)| *b)
                .unwrap_or(text.len());
            out.push((start, end));
            i += q_chars.len();
        } else {
            i += 1;
        }
    }
    out
}

impl ZaloGuiApp {
    /// Results dropdown for the in-conversation search.
    #[allow(clippy::too_many_arguments)]
    fn render_in_chat_results(
        &mut self,
        ctx: &egui::Context,
        pal: &ThemePalette,
        bar: Rect,
        thread_id: &str,
        thread_name: &str,
        thread_avatar: Option<&str>,
        is_group: bool,
        q_low: &str,
        q_folded: &str,
    ) {
        let now_ms = crate::login::unix_now() * 1000;
        let Some(thread) = self.threads.get(self.selected_thread_index) else {
            return;
        };
        // (message id, sender label, avatar, ts, text)
        let matches: Vec<(String, String, Option<String>, i64, String)> = thread
            .messages
            .iter()
            .rev()
            .filter(|m| {
                !m.text.trim().is_empty()
                    && (matches_vn_query(&m.text, q_low, q_folded)
                        || matches_vn_query(&m.sender, q_low, q_folded))
            })
            .map(|m| {
                let sender = if m.is_outgoing {
                    if self.user_name.is_empty() {
                        "Bạn".to_string()
                    } else {
                        self.user_name.clone()
                    }
                } else if is_group && !m.sender.is_empty() && m.sender != "0" {
                    crate::chat::sidebar_sender_id(m)
                        .and_then(|uid| crate::gui::alias_for(&uid))
                        .unwrap_or_else(|| m.sender.clone())
                } else {
                    thread_name.to_string()
                };
                let avatar = if m.is_outgoing {
                    self.user_avatar.clone()
                } else if is_group {
                    self.group_members
                        .get(thread_id)
                        .and_then(|ms| ms.iter().find(|gm| gm.name == m.sender))
                        .and_then(|gm| gm.avatar.clone())
                } else {
                    thread_avatar.map(str::to_string)
                };
                (
                    m.id.clone(),
                    sender,
                    avatar,
                    m.ts,
                    m.text.replace('\n', " "),
                )
            })
            .collect();
        let total = matches.len();
        let shown = total.min(self.in_chat_results_limit);
        let link_col = if pal.is_dark {
            Color32::from_rgb(97, 175, 239)
        } else {
            Color32::from_rgb(0, 104, 255)
        };
        let fade = appear(ctx, Id::new(("in_chat_results", q_low)), 0.16);
        let mut jump: Option<String> = None;
        let mut more = false;
        let width = bar.width() - 16.0;
        let results_area = egui::Area::new(Id::new("in_chat_search_results"))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::new(bar.left() + 8.0, bar.bottom() + 2.0 - (1.0 - fade) * 6.0))
            .show(ctx, |ui| {
                ui.set_opacity(fade);
                egui::Frame::none()
                    .fill(pal.bg_elevated)
                    .rounding(Rounding::same(18.0))
                    .shadow(egui::epaint::Shadow {
                        offset: Vec2::new(0.0, 6.0),
                        blur: 20.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(if pal.is_dark { 110 } else { 45 }),
                    })
                    .inner_margin(Margin::symmetric(10.0, 10.0))
                    .show(ui, |ui| {
                        ui.set_width(width - 20.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Tin nhắn").size(13.0).strong().color(pal.text_primary));
                            ui.label(
                                RichText::new(format!("{total} kết quả"))
                                    .size(11.5)
                                    .color(pal.text_muted),
                            );
                        });
                        ui.add_space(4.0);
                        if total == 0 {
                            ui.label(
                                RichText::new("Không có tin nhắn đã tải nào khớp. Tải thêm tin nhắn cũ để tìm xa hơn.")
                                    .size(12.0)
                                    .color(pal.text_muted),
                            );
                            return;
                        }
                        egui::ScrollArea::vertical()
                            .id_salt("in_chat_results_scroll")
                            // ~70 % taller than before (360), but never past the composer.
                            .max_height(612.0_f32.min((ctx.screen_rect().bottom() - bar.bottom() - 140.0).max(120.0)))
                            .min_scrolled_height(350.0_f32.min((ctx.screen_rect().bottom() - bar.bottom() - 140.0).max(120.0)))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for (id, sender, avatar, ts, text) in matches.iter().take(shown) {
                                    let w = ui.available_width();
                                    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 58.0), Sense::click());
                                    if resp.hovered() {
                                        ui.painter().rect_filled(r, Rounding::same(12.0), pal.bg_hover);
                                    }
                                    let c = Pos2::new(r.left() + 24.0, r.center().y);
                                    paint_user_avatar(
                                        ui.painter(),
                                        c,
                                        17.0,
                                        sender,
                                        false,
                                        avatar.as_deref(),
                                        &self.sticker_textures,
                                        &mut self.loading_stickers,
                                    );
                                    let tx = r.left() + 50.0;
                                    let clip = ui.painter().with_clip_rect(Rect::from_min_max(Pos2::new(tx, r.top()), Pos2::new(r.right() - 8.0, r.bottom())));
                                    clip.text(
                                        Pos2::new(tx, r.top() + 16.0),
                                        egui::Align2::LEFT_CENTER,
                                        truncate_chars(sender, 28).into_owned(),
                                        FontId::proportional(12.0),
                                        pal.text_muted,
                                    );
                                    ui.painter().text(
                                        Pos2::new(r.right() - 10.0, r.top() + 16.0),
                                        egui::Align2::RIGHT_CENTER,
                                        crate::clock::sidebar_relative_at(*ts, now_ms),
                                        FontId::proportional(11.5),
                                        pal.text_muted,
                                    );
                                    // Snippet around the first hit, keyword in accent colour.
                                    let ranges = match_ranges(text, q_low);
                                    let start_byte = ranges
                                        .first()
                                        .map(|(a, _)| {
                                            let pre: Vec<(usize, char)> = text[..*a].char_indices().collect();
                                            pre.len().checked_sub(24).map(|k| pre[k].0).unwrap_or(0)
                                        })
                                        .unwrap_or(0);
                                    let mut job = egui::text::LayoutJob::default();
                                    let fmt = |c: Color32| egui::TextFormat {
                                        font_id: FontId::proportional(13.0),
                                        color: c,
                                        ..Default::default()
                                    };
                                    if start_byte > 0 {
                                        job.append("… ", 0.0, fmt(pal.text_primary));
                                    }
                                    let mut last = start_byte;
                                    for (a, b) in ranges.iter().filter(|(a, _)| *a >= start_byte) {
                                        job.append(&text[last..*a], 0.0, fmt(pal.text_primary));
                                        job.append(&text[*a..*b], 0.0, fmt(link_col));
                                        last = *b;
                                    }
                                    job.append(&text[last..], 0.0, fmt(pal.text_primary));
                                    job.wrap = egui::text::TextWrapping {
                                        max_width: r.right() - 10.0 - tx,
                                        max_rows: 1,
                                        break_anywhere: true,
                                        overflow_character: Some('…'),
                                    };
                                    let galley = ui.fonts(|f| f.layout_job(job));
                                    ui.painter().galley(Pos2::new(tx, r.top() + 28.0), galley, pal.text_primary);
                                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                        jump = Some(id.clone());
                                    }
                                }
                                if total > shown {
                                    ui.add_space(4.0);
                                    if ui
                                        .add_sized(
                                            [ui.available_width(), 30.0],
                                            egui::Button::new(RichText::new("Xem thêm").size(12.5).color(pal.text_primary))
                                                .fill(pal.bg_hover)
                                                .rounding(Rounding::same(15.0)),
                                        )
                                        .clicked()
                                    {
                                        more = true;
                                    }
                                }
                            });
                    });
            });
        if ctx.input(|i| i.key_pressed(Key::Escape))
            || (ctx.input(|i| i.pointer.any_pressed())
                && ctx
                    .pointer_interact_pos()
                    .is_some_and(|p| !results_area.response.rect.contains(p) && !bar.contains(p)))
        {
            self.in_chat_search_open = false;
            self.in_chat_search_query.clear();
        }
        if more {
            self.in_chat_results_limit += 20;
        }
        if let Some(reference) = jump {
            self.in_chat_results_hidden = true;
            self.filter_starred_only = false;
            self.quote_jump = Some(QuoteJump {
                thread_id: thread_id.to_string(),
                reference,
                highlight_until: std::time::Instant::now() + std::time::Duration::from_secs(3),
                scroll_pending: true,
            });
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod in_chat_search_tests {
    use super::match_ranges;

    #[test]
    fn finds_case_insensitive_hits_on_char_boundaries() {
        let t = "Đã tốn một ngày cho cái Landing rồi, landing page";
        let r = match_ranges(t, "land");
        assert_eq!(r.len(), 2);
        assert_eq!(&t[r[0].0..r[0].1], "Land");
        assert_eq!(&t[r[1].0..r[1].1], "land");
        assert!(match_ranges(t, "").is_empty());
    }
}

impl ZaloGuiApp {
    /// Insert `text` at the composer's caret (end when unknown) and keep the caret after it.
    pub(super) fn insert_into_composer(
        &mut self,
        ctx: &egui::Context,
        thread_id: &str,
        text: &str,
    ) {
        let id = Id::new(("chat_composer_input", thread_id));
        let state = egui::TextEdit::load_state(ctx, id);
        let char_idx = state
            .as_ref()
            .and_then(|st| st.cursor.char_range())
            .map(|r| r.primary.index)
            .unwrap_or_else(|| self.input_message.chars().count());
        let byte_idx = self
            .input_message
            .char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or(self.input_message.len());
        self.input_message.insert_str(byte_idx, text);
        let next = char_idx + text.chars().count();
        let mut st = state.unwrap_or_default();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(next),
        )));
        st.store(ctx, id);
        self.composer_focus_pending = true;
    }
}

/// White round button with a hairline border and a soft shadow (chat-list header controls).
pub(super) fn paint_round_header_button(
    painter: &egui::Painter,
    r: Rect,
    hot: bool,
    pal: &ThemePalette,
) {
    let c = r.center();
    let rad = r.height() / 2.0;
    painter.circle_filled(
        c + Vec2::new(0.0, 1.0),
        rad + 0.5,
        Color32::from_black_alpha(if pal.is_dark { 60 } else { 18 }),
    );
    painter.circle_filled(c, rad, pal.bg_elevated);
    if hot {
        painter.circle_filled(c, rad, pal.bg_hover);
    }
    painter.circle_stroke(c, rad, Stroke::new(1.0_f32, pal.border));
}

/// One chat-list context-menu row: icon, label, optional expand chevron (`Some(open)`).
fn thread_menu_row(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    color: Color32,
    expand: Option<bool>,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(240.0), 32.0),
        Sense::click(),
    );
    if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            Rounding::same(8.0),
            ui.visuals().widgets.hovered.weak_bg_fill,
        );
    }
    draw_lucide_icon(
        ui.painter(),
        rect.left_center() + Vec2::new(16.0, 0.0),
        icon,
        15.0,
        color,
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(38.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.5),
        color,
    );
    if let Some(open) = expand {
        draw_lucide_icon(
            ui.painter(),
            rect.right_center() - Vec2::new(16.0, 0.0),
            if open { "\u{e06d}" } else { "\u{e06f}" },
            13.0,
            ui.visuals().weak_text_color(),
        );
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Indented option under an expanded row (optional colour dot, check when current).
fn thread_menu_sub_row(
    ui: &mut egui::Ui,
    dot: Option<Color32>,
    label: &str,
    current: bool,
    color: Color32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(240.0), 28.0),
        Sense::click(),
    );
    if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            Rounding::same(8.0),
            ui.visuals().widgets.hovered.weak_bg_fill,
        );
    }
    let x = rect.left() + 38.0;
    let mut text_x = x;
    if let Some(c) = dot {
        ui.painter()
            .circle_filled(Pos2::new(x + 4.0, rect.center().y), 4.0, c);
        text_x = x + 16.0;
    }
    ui.painter().text(
        Pos2::new(text_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        color,
    );
    if current {
        draw_lucide_icon(
            ui.painter(),
            rect.right_center() - Vec2::new(16.0, 0.0),
            "\u{e06c}",
            13.0,
            color,
        );
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Solid fill shared by the chat header pills and the three composer blocks.
pub(super) fn composer_block_fill(pal: &ThemePalette) -> Color32 {
    if pal.is_dark {
        Color32::from_rgb(38, 40, 46)
    } else {
        Color32::from_rgb(250, 250, 252)
    }
}

/// Soft drop shadow that lifts each composer block off the wallpaper.
pub(super) fn composer_block_shadow(pal: &ThemePalette) -> egui::epaint::Shadow {
    egui::epaint::Shadow {
        offset: Vec2::new(0.0, 3.0),
        blur: 14.0,
        spread: 0.0,
        color: Color32::from_black_alpha(if pal.is_dark { 110 } else { 46 }),
    }
}

/// Side of a staged attachment tile (fixed, whatever the picture's size).
const ATTACH_TILE: f32 = 72.0;

/// Staged attachment preview (Zalo-style): a fixed 72 pt rounded tile (cover-cropped picture,
/// or a file icon with its extension) with a ✕ in the corner, and the file name in a small
/// rounded pill below, as wide as the tile. Returns true when ✕ was clicked.
fn attachment_preview_card(
    ui: &mut egui::Ui,
    pal: &ThemePalette,
    tex: Option<&TextureHandle>,
    name: &str,
    is_image: bool,
    id_salt: &str,
) -> bool {
    let side = ATTACH_TILE;
    let rounding = Rounding::same(12.0);
    let mut removed = false;
    ui.allocate_ui_with_layout(
        Vec2::new(side, side + 26.0),
        Layout::top_down(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let (r, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
            ui.painter()
                .add(composer_block_shadow(pal).as_shape(r, rounding));
            match tex.filter(|_| is_image) {
                Some(t) => {
                    // Cover-crop the square so the picture fills it without stretching.
                    let s = t.size_vec2();
                    let ar = s.x / s.y.max(1.0);
                    let uv = if ar > 1.0 {
                        let k = 1.0 / ar;
                        Rect::from_min_max(
                            Pos2::new((1.0 - k) / 2.0, 0.0),
                            Pos2::new((1.0 + k) / 2.0, 1.0),
                        )
                    } else {
                        let k = ar;
                        Rect::from_min_max(
                            Pos2::new(0.0, (1.0 - k) / 2.0),
                            Pos2::new(1.0, (1.0 + k) / 2.0),
                        )
                    };
                    egui::Image::new(t)
                        .uv(uv)
                        .rounding(rounding)
                        .paint_at(ui, r);
                }
                None => {
                    ui.painter()
                        .rect_filled(r, rounding, composer_block_fill(pal));
                    let icon = if is_image {
                        lucide::IMAGE
                    } else {
                        lucide::FILE_TEXT
                    };
                    draw_lucide_icon(
                        ui.painter(),
                        r.center() - Vec2::new(0.0, 7.0),
                        icon,
                        22.0,
                        pal.text_secondary,
                    );
                    if let Some(ext) = std::path::Path::new(name)
                        .extension()
                        .and_then(|e| e.to_str())
                    {
                        ui.painter().text(
                            r.center() + Vec2::new(0.0, 17.0),
                            egui::Align2::CENTER_CENTER,
                            ext.to_uppercase(),
                            FontId::proportional(10.0),
                            pal.text_muted,
                        );
                    }
                }
            }
            let c = Pos2::new(r.right() - 10.0, r.top() + 10.0);
            let x_resp = ui
                .interact(
                    Rect::from_center_size(c, Vec2::splat(20.0)),
                    ui.id().with(("att_x", id_salt)),
                    Sense::click(),
                )
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text("Bỏ");
            ui.painter().circle_filled(
                c,
                8.0,
                Color32::from_black_alpha(if x_resp.hovered() { 210 } else { 150 }),
            );
            draw_lucide_icon(ui.painter(), c, lucide::X, 9.0, Color32::WHITE);
            removed = x_resp.clicked();
            // Name pill, styled like a message bubble.
            let pill_w = side;
            let mut job = egui::text::LayoutJob::simple_singleline(
                name.to_string(),
                FontId::proportional(10.5),
                pal.bubble_in_text,
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(pill_w - 12.0);
            let galley = ui.fonts(|f| f.layout_job(job));
            let (pr, presp) = ui.allocate_exact_size(Vec2::new(pill_w, 20.0), Sense::hover());
            ui.painter()
                .rect_filled(pr, Rounding::same(10.0), pal.bubble_in_bg);
            ui.painter().galley(
                pr.center() - galley.size() / 2.0,
                galley,
                pal.bubble_in_text,
            );
            presp.on_hover_text(name);
        },
    );
    removed
}
