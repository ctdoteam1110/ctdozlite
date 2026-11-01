use super::*;

impl ZaloGuiApp {
    pub(super) fn render_chat_workspace(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        // Responsive breakpoints based on live viewport width
        // -------------------------------------------------------------------
        let screen_rect = ctx.screen_rect();
        let screen_w = screen_rect.width();
        // The right info drawer only fits on wide windows.
        let drawer_allowed = screen_w >= 1000.0;

        // -------------------------------------------------------------------
        // 1. LEFT NAVIGATION SIDEBAR + 2. CONVERSATION LIST (both collapsible)
        // -------------------------------------------------------------------
        // Ctrl/Cmd+K focuses conversation search (expanding the list if needed).
        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::K)) {
            if self.settings.list_collapsed {
                self.settings.list_collapsed = false;
                self.settings.save();
            }
            self.search_focus_pending = true;
        }
        // Below ~760 px both columns must collapse (hard). Between 760 and 1100 px the nav
        // auto-collapses to its icon rail unless the user expanded it manually this session; the
        // automatic state is never written back to the persisted preference.
        let nav_forced = screen_w < 760.0;
        let nav_auto_collapse = screen_w < 1100.0;
        let list_forced = screen_w < 760.0;
        let nav_expanded = !nav_forced
            && match self.nav_user_override {
                Some(expanded) => expanded,
                None => !self.settings.nav_collapsed && !nav_auto_collapse,
            };
        let list_expanded = !self.settings.list_collapsed && !list_forced;
        self.render_nav_sidebar(ctx, pal, nav_expanded, nav_forced);
        let list_max_w = (screen_w * 0.42).clamp(260.0, 440.0);
        self.render_conversation_list(ctx, pal, list_expanded, list_forced, list_max_w);

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
        let show_info_drawer = self.info_drawer_open && drawer_allowed;
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
                                let is_photo = m.link_src.as_deref() == Some("Hình ảnh Zalo")
                                    || m.text.trim() == "[Hình ảnh]"
                                    || m.text.trim().starts_with("[Hình ảnh] ");
                                if !is_photo {
                                    return None;
                                }
                                let thumb = m
                                    .link_thumb
                                    .as_deref()
                                    .or(m.link_url.as_deref())
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
                            .fill(pal.bg_list)
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
                                    let d_is_cloud = d_name.contains("Cloud của tôi");
                                    let d_fr_status = if d_is_group || d_is_cloud {
                                        crate::chat::FriendshipStatus::Friend
                                    } else {
                                        self.friend_status_map
                                            .get(d_id)
                                            .copied()
                                            .unwrap_or(crate::chat::FriendshipStatus::Unknown)
                                    };
                                    let (sub_txt, sub_col) = if d_is_group {
                                        ("Nhóm đang hoạt động", pal.accent_online)
                                    } else if d_is_cloud {
                                        ("Lưu trữ đám mây cá nhân", pal.primary_hover)
                                    } else {
                                        match d_fr_status {
                                            crate::chat::FriendshipStatus::Friend => {
                                                ("✓ Đã kết bạn · Trực tuyến", pal.accent_online)
                                            }
                                            crate::chat::FriendshipStatus::IncomingPending => (
                                                "📩 Đã gửi lời mời kết bạn cho bạn",
                                                pal.primary_hover,
                                            ),
                                            crate::chat::FriendshipStatus::OutgoingPending => {
                                                ("⏳ Đã gửi lời mời kết bạn", pal.warning)
                                            }
                                            crate::chat::FriendshipStatus::Stranger => (
                                                "Người lạ · Chưa kết bạn",
                                                Color32::from_rgb(249, 115, 22),
                                            ),
                                            crate::chat::FriendshipStatus::Unknown => {
                                                ("Đang kiểm tra kết bạn…", pal.text_muted)
                                            }
                                        }
                                    };
                                    ui.label(RichText::new(sub_txt).size(12.0).color(sub_col));

                                    ui.add_space(14.0);
                                    // Quick Action Buttons (Sync & Pin/Star Filter)
                                    ui.horizontal(|ui| {
                                        ui.add_space(34.0);
                                        let sync_btn = egui::Button::new(
                                            RichText::new("Làm mới")
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
                                                        is_group: d_is_group,
                                                    });
                                                }
                                            }
                                            self.show_toast("Đang làm mới hội thoại…");
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
                                                    pan: Vec2::ZERO,
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
                            .fill(pal.bg_app)
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
                // Soft diagonal gradient behind the whole chat column (header, stream, composer).
                paint_diagonal_gradient(ui.painter(), ui.max_rect(), pal.canvas_top, pal.canvas_bottom);
                // Breakpoints follow the real chat-column width (columns may be collapsed or not).
                let chat_w = ui.available_width();
                let is_compact_w = chat_w < 560.0;
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
                    let header_frame = egui::Frame::none()
                        .fill(if pal.is_dark {
                            Color32::from_rgba_unmultiplied(33, 37, 43, 200)
                        } else {
                            Color32::from_rgba_unmultiplied(255, 255, 255, 120)
                        })
                        .inner_margin(Margin::symmetric(18.0, 12.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // Header avatar (44×44)
                                let (av_rect, _) =
                                    ui.allocate_exact_size(Vec2::new(44.0, 44.0), Sense::hover());
                                if is_cloud_thread {
                                    ui.painter().circle_filled(av_rect.center(), 22.0, pal.bg_elevated);
                                    ui.painter().circle_stroke(av_rect.center(), 22.0, Stroke::new(1.0_f32, pal.border));
                                    draw_lucide_icon(ui.painter(), av_rect.center(), lucide::CLOUD, 19.0, pal.primary);
                                } else {
                                    paint_user_avatar(
                                        ui.painter(),
                                        av_rect.center(),
                                        22.0,
                                        &thread_name,
                                        is_group,
                                        thread_avatar.as_deref(),
                                        &self.sticker_textures,
                                        &mut self.loading_stickers,
                                    );
                                }

                                ui.add_space(8.0);
                                // Reserve room for the right-hand action cluster so the name block
                                // can never be overlapped by the call buttons.
                                let actions_w = if is_compact_w { 120.0 } else { 176.0 };
                                let left_info_w = (ui.available_width() - actions_w).max(60.0);
                                ui.allocate_ui_with_layout(
                                    Vec2::new(left_info_w, 44.0),
                                    Layout::top_down(Align::Min),
                                    |ui| {
                                        ui.spacing_mut().item_spacing.y = 1.0;
                                        ui.add_space(2.0);
                                        ui.horizontal(|ui| {
                                            // Only relationship states that need attention get a pill.
                                            let tag = if is_group || is_cloud_thread {
                                                None
                                            } else {
                                                match cur_friendship {
                                                    crate::chat::FriendshipStatus::IncomingPending => Some((
                                                        "Chờ đồng ý kết bạn",
                                                        pal.primary,
                                                    )),
                                                    crate::chat::FriendshipStatus::OutgoingPending => Some((
                                                        "Đã gửi lời mời",
                                                        pal.warning,
                                                    )),
                                                    crate::chat::FriendshipStatus::Stranger => Some((
                                                        "Người lạ",
                                                        Color32::from_rgb(234, 108, 20),
                                                    )),
                                                    _ => None,
                                                }
                                            };
                                            let tag_w = if tag.is_some() && left_info_w > 200.0 { 120.0 } else { 0.0 };
                                            let g = crate::gui::sidebar::single_line_galley(
                                                ui,
                                                &thread_name,
                                                16.5,
                                                pal.text_primary,
                                                (left_info_w - tag_w).max(40.0),
                                            );
                                            let (r, _) = ui.allocate_exact_size(Vec2::new(g.size().x, 22.0), Sense::hover());
                                            ui.painter().galley(Pos2::new(r.left(), r.center().y - g.size().y / 2.0), g, pal.text_primary);
                                            if let (Some((lbl, col)), true) = (tag, tag_w > 0.0) {
                                                egui::Frame::none()
                                                    .fill(col.linear_multiply(0.14))
                                                    .rounding(Rounding::same(8.0))
                                                    .inner_margin(Margin::symmetric(7.0, 1.5))
                                                    .show(ui, |ui| {
                                                        ui.label(RichText::new(lbl).size(10.8).color(col));
                                                    });
                                            }
                                        });
                                        let sub = if is_group {
                                            // Demo counts first, then the real `totalMember` from `getmg-v2`.
                                            let count = self
                                                .group_member_counts
                                                .get(&thread_id)
                                                .copied()
                                                .or_else(|| crate::chat::group_member_count(&thread_id).map(|n| n as usize));
                                            match count {
                                                Some(n) => format!("{n} thành viên"),
                                                None => "Nhóm".to_string(),
                                            }
                                        } else if is_cloud_thread {
                                            "Lưu tin nhắn, ghi chú và tệp của bạn".to_string()
                                        } else {
                                            match cur_friendship {
                                                crate::chat::FriendshipStatus::Friend => "Bạn bè".to_string(),
                                                crate::chat::FriendshipStatus::IncomingPending => "Đã gửi lời mời kết bạn cho bạn".to_string(),
                                                crate::chat::FriendshipStatus::OutgoingPending => "Đang chờ đồng ý kết bạn".to_string(),
                                                crate::chat::FriendshipStatus::Stranger => "Chưa kết bạn trên Zalo".to_string(),
                                                crate::chat::FriendshipStatus::Unknown => "Tin nhắn cá nhân".to_string(),
                                            }
                                        };
                                        let g = crate::gui::sidebar::single_line_galley(ui, &sub, 12.5, pal.text_muted, left_info_w);
                                        let (r, _) = ui.allocate_exact_size(Vec2::new(g.size().x, 18.0), Sense::hover());
                                        ui.painter().galley(Pos2::new(r.left(), r.center().y - g.size().y / 2.0), g, pal.text_muted);
                                    },
                                );

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    // Info Drawer Toggle Button (hidden on narrow windows: auto-collapsed)
                                    if drawer_allowed {
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

                                    ui.add_space(4.0);

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
                                        if self.in_chat_search_open {
                                            self.in_chat_search_focus_pending = true;
                                        } else {
                                            self.in_chat_search_query.clear();
                                        }
                                    }
                                });
                            });
                        });
                    ui.painter().hline(
                        header_frame.response.rect.x_range(),
                        header_frame.response.rect.bottom(),
                        Stroke::new(1.0_f32, pal.border),
                    );
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
                        egui::Frame::none()
                            .fill(pal.bg_elevated)
                            .stroke(Stroke::new(1.0_f32, pal.border))
                            .inner_margin(Margin::symmetric(18.0, 7.0))
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
                                                    .rounding(Rounding::same(6.0))
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
                                                    .rounding(Rounding::same(6.0))
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
                                                    .rounding(Rounding::same(6.0))
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
                                                    .rounding(Rounding::same(6.0))
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
                                            .small_button(format!("Bỏ ghim {}", lucide::X))
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
                    if self.in_chat_search_open {
                        if ui.input(|i| i.key_pressed(Key::Escape)) {
                            self.in_chat_search_open = false;
                            self.in_chat_search_query.clear();
                        }
                    }
                    if self.in_chat_search_open {
                        let q_low = self.in_chat_search_query.trim().to_lowercase();
                        let q_folded = fold_vn_search(&q_low);
                        let match_cnt = if q_low.is_empty() {
                            0
                        } else {
                            self.threads
                                .get(self.selected_thread_index)
                                .map(|t| {
                                    t.messages
                                        .iter()
                                        .filter(|m| {
                                            matches_vn_query(&m.text, &q_low, &q_folded)
                                                || matches_vn_query(&m.sender, &q_low, &q_folded)
                                        })
                                        .count()
                                })
                                .unwrap_or(0)
                        };
                        egui::Frame::none()
                            .fill(pal.bg_sidebar)
                            .stroke(Stroke::new(1.0_f32, pal.border))
                            .inner_margin(Margin::symmetric(18.0, 8.0))
                            .show(ui, |ui| {
                                let bar_avail_w = ui.available_width();
                                ui.set_width(bar_avail_w);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    let right_reserve = if q_low.is_empty() {
                                        86.0
                                    } else {
                                        174.0
                                    };
                                    let pill_w = (bar_avail_w - right_reserve).max(140.0);
                                    egui::Frame::none()
                                        .fill(pal.bg_input)
                                        .rounding(Rounding::same(8.0))
                                        .stroke(Stroke::new(
                                            1.0_f32,
                                            pal.primary.linear_multiply(0.65),
                                        ))
                                        .inner_margin(Margin::symmetric(10.0, 5.0))
                                        .show(ui, |ui| {
                                            let inner_w = (pill_w - 20.0).max(110.0);
                                            ui.set_width(inner_w);
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 6.0;
                                                let (ir, _) = ui.allocate_exact_size(
                                                    Vec2::new(15.0, 18.0),
                                                    Sense::hover(),
                                                );
                                                draw_icon_search(
                                                    ui.painter(),
                                                    ir.center(),
                                                    13.5,
                                                    pal.primary_hover,
                                                );
                                                let has_txt =
                                                    !self.in_chat_search_query.is_empty();
                                                let te_w = if has_txt {
                                                    (inner_w - 44.0).max(60.0)
                                                } else {
                                                    (inner_w - 22.0).max(80.0)
                                                };
                                                let te_resp = ui.add(
                                                    egui::TextEdit::singleline(
                                                        &mut self.in_chat_search_query,
                                                    )
                                                    .id(Id::new((
                                                        "in_chat_search_input",
                                                        &thread_id,
                                                    )))
                                                    .frame(false)
                                                    .hint_text(
                                                        "Tìm tin nhắn trong hội thoại này...",
                                                    )
                                                    .desired_width(te_w),
                                                );
                                                if self.in_chat_search_focus_pending {
                                                    te_resp.request_focus();
                                                    self.in_chat_search_focus_pending = false;
                                                }
                                                if has_txt {
                                                    let (clr_r, clr_resp) = ui
                                                        .allocate_exact_size(
                                                            Vec2::splat(16.0),
                                                            Sense::click(),
                                                        );
                                                    if clr_resp.hovered() {
                                                        ui.painter().circle_filled(
                                                            clr_r.center(),
                                                            8.0,
                                                            pal.bg_hover,
                                                        );
                                                    }
                                                    draw_lucide_icon(
                                                        ui.painter(),
                                                        clr_r.center(),
                                                        lucide::X,
                                                        11.5,
                                                        pal.text_muted,
                                                    );
                                                    if clr_resp
                                                        .on_hover_cursor(CursorIcon::PointingHand)
                                                        .on_hover_text("Xóa từ khóa")
                                                        .clicked()
                                                    {
                                                        self.in_chat_search_query.clear();
                                                    }
                                                }
                                            });
                                        });

                                    if !q_low.is_empty() {
                                        let badge_col = if match_cnt > 0 {
                                            pal.accent_online
                                        } else {
                                            pal.danger
                                        };
                                        egui::Frame::none()
                                            .fill(badge_col.linear_multiply(0.14))
                                            .rounding(Rounding::same(6.0))
                                            .inner_margin(Margin::symmetric(8.0, 4.0))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(if match_cnt > 0 {
                                                        format!("{match_cnt} kết quả")
                                                    } else {
                                                        "0 kết quả".to_string()
                                                    })
                                                    .size(11.5)
                                                    .color(badge_col)
                                                    .strong(),
                                                );
                                            });
                                    }

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let close_btn = egui::Button::new(
                                            RichText::new(format!("{} Đóng", lucide::X))
                                                .size(11.8)
                                                .color(pal.text_secondary)
                                                .strong(),
                                        )
                                        .fill(pal.bg_elevated)
                                        .stroke(Stroke::new(1.0_f32, pal.border))
                                        .rounding(Rounding::same(6.0))
                                        .min_size(Vec2::new(66.0, 28.0));
                                        if ui
                                            .add(close_btn)
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .on_hover_text("Đóng thanh tìm kiếm (Esc)")
                                            .clicked()
                                        {
                                            self.in_chat_search_open = false;
                                            self.in_chat_search_query.clear();
                                        }
                                    });
                                });
                            });
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
                        + (if self.pending_clipboard_image.is_some() {
                            64.0
                        } else {
                            0.0
                        });
                    let mut pending_action: Option<(usize, BubbleAction)> = None;
                    let mut load_older_clicked = false;
                    let typing_name = self.typing_indicators.get(&thread_id).cloned();
                    let typing_reserve = if typing_name.is_some() { 24.0 } else { 0.0 };
                    let msg_area_height =
                        (ui.available_height() - 70.0 - picker_reserve - typing_reserve).max(100.0);

                    egui::ScrollArea::vertical()
                        .id_salt(Id::new(("chat_scroll", &thread_id)))
                        .max_height(msg_area_height)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            egui::Frame::none()
                                .inner_margin(Margin {
                                    left: 20.0,
                                    right: 22.0,
                                    top: 14.0,
                                    bottom: 10.0,
                                })
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
                                            if !self.in_chat_search_open
                                                && !self.filter_starred_only
                                            {
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
                                                        if let Some(tx) = WORKER_TX.get() {
                                                            let _ = tx.send(
                                                                WorkerCmd::LoadGroupHistory {
                                                                    group_id: thread_id.clone(),
                                                                    is_group,
                                                                },
                                                            );
                                                        }
                                                        load_older_clicked = true;
                                                    }
                                                    ui.add_space(6.0);
                                                });
                                            }
                                            let only_starred = self.filter_starred_only;
                                            let in_chat_q = if self.in_chat_search_open {
                                                self.in_chat_search_query.trim().to_lowercase()
                                            } else {
                                                String::new()
                                            };
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
                                                    render_date_divider(ui, prev_msg, msg, pal);
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
                    if load_older_clicked {
                        self.show_toast("Đang tải thêm tin nhắn cũ từ máy chủ Zalo…");
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
                                    let lower_u = full_url.to_lowercase();
                                    let is_direct_img = lower_u.contains("zdn.vn")
                                        || lower_u.ends_with(".png")
                                        || lower_u.ends_with(".jpg")
                                        || lower_u.ends_with(".jpeg")
                                        || lower_u.ends_with(".webp")
                                        || lower_u.ends_with(".gif")
                                        || lower_u.ends_with(".bmp")
                                        || (!lower_u.starts_with("http://")
                                            && !lower_u.starts_with("https://"));
                                    let hd_tex_key = if !full_url.is_empty() && is_direct_img {
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
                                        pan: Vec2::ZERO,
                                    });
                                }
                            }
                        }
                    }

                    // 2b. Typing indicator ("••• Anh Minh đang nhập")
                    if let Some(name) = typing_name.as_deref() {
                        let (row, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), typing_reserve),
                            Sense::hover(),
                        );
                        let t = ui.input(|i| i.time) as f32;
                        let base = Pos2::new(row.left() + 66.0, row.center().y);
                        for k in 0..3 {
                            let phase = (t * 4.0 - k as f32 * 0.7).sin() * 0.5 + 0.5;
                            ui.painter().circle_filled(
                                Pos2::new(base.x + k as f32 * 7.0, base.y),
                                2.6,
                                pal.text_muted.linear_multiply(0.35 + 0.65 * phase),
                            );
                        }
                        ui.painter().text(
                            Pos2::new(base.x + 24.0, base.y),
                            egui::Align2::LEFT_CENTER,
                            format!("{name} đang nhập"),
                            FontId::proportional(12.5),
                            pal.text_muted,
                        );
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(80));
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
                                                    .small_button(lucide::X)
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
                    // 3c. Clipboard Image Preview Banner (`Ctrl+V` / `Shift+Insert` screenshot or image file paste)
                    if is_clipboard_paste_triggered(ctx) {
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
                            self.show_toast(format!(
                                "Đã dán ảnh ({w}×{h} px) từ Clipboard — nhấn Enter để gửi"
                            ));
                        }
                    }
                    if self.pending_clipboard_image.is_some() {
                        let mut clear_clip = false;
                        if ui.input(|i| i.key_pressed(Key::Escape)) {
                            clear_clip = true;
                        }
                        if let Some((ref path, w, h, ref tex)) = self.pending_clipboard_image {
                            egui::Frame::none()
                                .fill(pal.bg_elevated)
                                .inner_margin(Margin::symmetric(14.0, 6.0))
                                .stroke(Stroke::new(
                                    1.0_f32,
                                    pal.primary.linear_multiply(0.55),
                                ))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            egui::Image::new(tex)
                                                .fit_to_exact_size(Vec2::new(48.0, 48.0))
                                                .rounding(Rounding::same(6.0)),
                                        );
                                        ui.add_space(6.0);
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} Ảnh chụp từ bộ nhớ tạm (Ctrl+V)",
                                                    lucide::IMAGE
                                                ))
                                                .size(12.2)
                                                .color(pal.primary)
                                                .strong(),
                                            );
                                            let fname = path
                                                .file_name()
                                                .and_then(|n| n.to_str())
                                                .unwrap_or("clipboard.png");
                                            ui.label(
                                                RichText::new(format!(
                                                    "{fname} · {w}×{h} px — Nhập chú thích bên dưới rồi nhấn Enter để gửi"
                                                ))
                                                .size(11.3)
                                                .color(pal.text_secondary),
                                            );
                                        });
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .small_button(format!("{} Hủy", lucide::X))
                                                    .on_hover_cursor(CursorIcon::PointingHand)
                                                    .on_hover_text("Hủy ảnh dán (Esc)")
                                                    .clicked()
                                                {
                                                    clear_clip = true;
                                                }
                                            },
                                        );
                                    });
                                });
                        }
                        if clear_clip {
                            self.pending_clipboard_image = None;
                        }
                    }

                    // 4. Bottom Message Composer Bar (+ menu · sticker · emoji · pill input · round send)
                    let mut plus_rect = Rect::NOTHING;
                    let mut emoji_rect = Rect::NOTHING;
                    let composer_frame = egui::Frame::none()
                        .fill(if pal.is_dark {
                            Color32::from_rgba_unmultiplied(33, 37, 43, 190)
                        } else {
                            Color32::from_rgba_unmultiplied(255, 255, 255, 96)
                        })
                        .inner_margin(Margin::symmetric(16.0, 11.0))
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
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;

                                        // (+) attachments menu: image / file / clipboard image
                                        let (pr, presp) =
                                            ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
                                        plus_rect = pr;
                                        let plus_hot = presp.hovered() || self.attach_menu_open;
                                        ui.painter().circle_filled(
                                            pr.center(),
                                            16.0,
                                            if plus_hot { pal.bg_hover } else { pal.bg_elevated },
                                        );
                                        ui.painter().circle_stroke(
                                            pr.center(),
                                            16.0,
                                            Stroke::new(1.0_f32, pal.border),
                                        );
                                        draw_lucide_icon(
                                            ui.painter(),
                                            pr.center(),
                                            lucide::PLUS,
                                            17.0,
                                            if plus_hot { pal.primary } else { pal.text_secondary },
                                        );
                                        if presp
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .on_hover_text("Đính kèm: hình ảnh, tệp, ảnh từ Clipboard")
                                            .clicked()
                                        {
                                            self.attach_menu_open = !self.attach_menu_open;
                                            self.emoji_popup_open = false;
                                        }

                                        // Sticker picker
                                        if icon_button(
                                            ui,
                                            lucide::STICKER,
                                            32.0,
                                            18.0,
                                            self.sticker_picker_open,
                                            pal,
                                            "Nhãn dán (Sticker)",
                                        )
                                        .clicked()
                                        {
                                            self.sticker_picker_open = !self.sticker_picker_open;
                                        }

                                        // Emoji quick picker
                                        let er = icon_button(
                                            ui,
                                            lucide::SMILE,
                                            32.0,
                                            18.0,
                                            self.emoji_popup_open,
                                            pal,
                                            "Biểu tượng cảm xúc",
                                        );
                                        emoji_rect = er.rect;
                                        if er.clicked() {
                                            self.emoji_popup_open = !self.emoji_popup_open;
                                            self.attach_menu_open = false;
                                        }
                                        ui.add_space(2.0);

                                        // Rounded pill input
                                        let send_btn_w = 38.0_f32;
                                        let pill_w =
                                            (ui.available_width() - send_btn_w - 8.0).max(60.0);
                                        let hint = format!(
                                            "Nhắn tin tới {}",
                                            truncate_chars(&thread_name, 24)
                                        );
                                        let response = egui::Frame::none()
                                            .fill(pal.composer_bg)
                                            .rounding(Rounding::same(20.0))
                                            .stroke(Stroke::new(1.0_f32, pal.border))
                                            .inner_margin(Margin::symmetric(12.0, 3.0))
                                            .show(ui, |ui| {
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut self.input_message)
                                                        .id(Id::new(("chat_composer_input", &thread_id)))
                                                        .frame(false)
                                                        .hint_text(RichText::new(hint).color(pal.text_muted))
                                                        .margin(Margin::symmetric(4.0, 6.0))
                                                        .desired_width((pill_w - 40.0).max(40.0)),
                                                )
                                            })
                                            .inner;
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
                                                    self.show_toast(format!(
                                                        "Đã dán ảnh ({w}×{h} px) từ Clipboard — nhấn Enter để gửi"
                                                    ));
                                                } else {
                                                    self.show_toast(
                                                        "Không tìm thấy hình ảnh nào trong Clipboard",
                                                    );
                                                }
                                            }
                                        });

                                        // Keyboard navigation for contextual sticker popover (Zalo PC `changeSelectedIndex`):
                                        // - ArrowUp selects the first suggested sticker
                                        // - ArrowLeft / ArrowRight cycles through suggested stickers
                                        // - ArrowDown / Escape deselects or hides the popover
                                        // - Enter sends the selected sticker if one is highlighted, otherwise sends text!
                                        let mut send_selected_sticker: Option<
                                            crate::chat::StickerItem,
                                        > = None;
                                        if !contextual_stickers.is_empty() && response.has_focus() {
                                            let len = contextual_stickers.len();
                                            if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
                                                self.composer_sticker_selected = Some(0);
                                            } else if let Some(sel) = self.composer_sticker_selected
                                            {
                                                if ui.input(|i| i.key_pressed(Key::ArrowRight)) {
                                                    self.composer_sticker_selected =
                                                        Some((sel + 1) % len);
                                                } else if ui
                                                    .input(|i| i.key_pressed(Key::ArrowLeft))
                                                {
                                                    self.composer_sticker_selected =
                                                        Some((sel + len - 1) % len);
                                                } else if ui
                                                    .input(|i| i.key_pressed(Key::ArrowDown))
                                                {
                                                    self.composer_sticker_selected = None;
                                                }
                                            }
                                        } else {
                                            self.composer_sticker_selected = None;
                                        }

                                        let enter_pressed =
                                            (response.lost_focus() || response.has_focus())
                                                && ui.input(|i| i.key_pressed(Key::Enter));
                                        if enter_pressed {
                                            if let Some(sel) = self.composer_sticker_selected {
                                                if let Some(st) = contextual_stickers.get(sel) {
                                                    send_selected_sticker = Some(st.clone());
                                                }
                                            }
                                        }
                                        // Round send button (arrow-up)
                                        let can_send = !self.input_message.trim().is_empty()
                                            || self.pending_clipboard_image.is_some();
                                        let (send_rect, send_resp) = ui.allocate_exact_size(
                                            Vec2::splat(send_btn_w),
                                            Sense::click(),
                                        );
                                        let send_bg = if !can_send {
                                            pal.primary.linear_multiply(0.45)
                                        } else if send_resp.hovered() {
                                            pal.primary_hover
                                        } else {
                                            pal.primary
                                        };
                                        ui.painter().circle_filled(send_rect.center(), 18.0, send_bg);
                                        draw_lucide_icon(
                                            ui.painter(),
                                            send_rect.center(),
                                            lucide::ARROW_UP,
                                            18.0,
                                            Color32::WHITE,
                                        );
                                        let send_resp = send_resp.on_hover_text("Gửi (Enter)");
                                        let send_clicked = send_resp
                                            .on_hover_cursor(if can_send {
                                                CursorIcon::PointingHand
                                            } else {
                                                CursorIcon::Default
                                            })
                                            .clicked();

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
                                            && self.pending_clipboard_image.is_some()
                                        {
                                            if let Some((clip_path, _, _, _)) =
                                                self.pending_clipboard_image.take()
                                            {
                                                let cap =
                                                    self.input_message.trim().to_string();
                                                self.input_message.clear();
                                                self.dismissed_sticker_suggest_for.clear();
                                                self.composer_sticker_selected = None;
                                                response.request_focus();
                                                if let Some(tx) = WORKER_TX.get() {
                                                    let _ = tx.send(
                                                        WorkerCmd::PickAndSendAttachment {
                                                            to_id: thread_id.clone(),
                                                            is_group,
                                                            images_only: true,
                                                            caption: cap,
                                                            preset_path: Some(clip_path),
                                                        },
                                                    );
                                                }
                                            }
                                        } else if (send_clicked || enter_pressed) && can_send {
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
                                                } else {
                                                    let _ = tx.send(WorkerCmd::SendMessage {
                                                        to_id: thread_id.clone(),
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

                    ui.painter().hline(
                        composer_frame.response.rect.x_range(),
                        composer_frame.response.rect.top(),
                        Stroke::new(1.0_f32, pal.border),
                    );
                    self.render_composer_popups(ctx, pal, plus_rect, emoji_rect, &thread_id, is_group);
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
        self.render_settings_modal(ctx, pal);
    }

}
