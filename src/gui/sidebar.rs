//! Left navigation sidebar ("Hộp thư / Lưu trữ / Danh bạ") and the conversation list.
//!
//! Both columns collapse: the navigation shrinks to an icon rail, the conversation list to an
//! avatar column. State is persisted in `AppSettings` and both auto-collapse on narrow windows.

use super::*;

/// Expanded / collapsed widths of the navigation sidebar.
pub(super) const NAV_W_EXPANDED: f32 = 236.0;
pub(super) const NAV_W_RAIL: f32 = 64.0;
/// Collapsed width of the conversation list (avatar column).
pub(super) const LIST_W_COLLAPSED: f32 = 78.0;

/// One entry of the left navigation.
struct NavItem {
    section: Option<NavSection>,
    icon: &'static str,
    label: &'static str,
    badge: usize,
    /// `Some(text)` → item has no backing feature yet; clicking shows this toast.
    soon: Option<&'static str>,
    action: NavAction,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NavAction {
    Section,
    OpenCloud,
    ZCloud,
}

/// Lay out a single line of text, truncating with `…` to `max_w`.
pub(super) fn single_line_galley(
    ui: &egui::Ui,
    text: &str,
    size: f32,
    color: Color32,
    max_w: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.replace('\n', " "),
        egui::TextFormat::simple(FontId::proportional(size), color),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: max_w.max(8.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.fonts(|f| f.layout_job(job))
}

fn human_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    let (val, unit) = if b >= 1024.0 * 1024.0 * 1024.0 {
        (b / (1024.0 * 1024.0 * 1024.0), "GB")
    } else if b >= 1024.0 * 1024.0 {
        (b / (1024.0 * 1024.0), "MB")
    } else {
        (b / 1024.0, "KB")
    };
    format!("{val:.1} {unit}").replace('.', ",")
}

/// Total size of the files below `dir` (bounded walk, best effort).
fn dir_size(dir: &std::path::Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    let mut visited = 0usize;
    while let Some((d, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            visited += 1;
            if visited > 200_000 {
                return total;
            }
            let Ok(meta) = e.metadata() else {
                continue;
            };
            if meta.is_dir() {
                if depth < 6 {
                    stack.push((e.path(), depth + 1));
                }
            } else {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

impl ZaloGuiApp {
    /// Is `t` the "Cloud của tôi" self-chat?
    fn is_cloud_thread(t: &Thread) -> bool {
        t.name.contains("Cloud của tôi")
    }

    fn friendship_of(&self, t: &Thread) -> crate::chat::FriendshipStatus {
        self.friend_status_map
            .get(&t.id)
            .copied()
            .unwrap_or(crate::chat::FriendshipStatus::Unknown)
    }

    /// Last activity of a conversation (ms): its newest loaded message, or the time estimated
    /// from `get-last-msgs` (`conv_activity`), whichever is later. 0 when there is none.
    pub(super) fn thread_activity_ms(&self, t: &Thread) -> i64 {
        let last_msg = t.messages.last().map(|m| normalize_ts_ms(m.ts)).unwrap_or(0);
        last_msg.max(self.conv_activity.get(&t.id).copied().unwrap_or(0))
    }

    /// Would the phone list this conversation? Yes when it has messages here, server-side
    /// activity, unread messages, or it is "Cloud của tôi".
    fn thread_has_activity(&self, t: &Thread) -> bool {
        !t.messages.is_empty()
            || self.conv_activity.contains_key(&t.id)
            || self.unread_counts.get(&t.id).copied().unwrap_or(0) > 0
            || Self::is_cloud_thread(t)
    }

    /// Thin strip under the list header while the offline catch-up runs, then "Đã đồng bộ N
    /// tin" for a few seconds.
    fn render_sync_strip(&mut self, ui: &mut egui::Ui, pal: &ThemePalette) {
        let Some((fetched, done, at)) = self.sync_progress else {
            return;
        };
        const SHOW_DONE: Duration = Duration::from_secs(4);
        if done && (fetched == 0 || at.elapsed() > SHOW_DONE) {
            if at.elapsed() > SHOW_DONE {
                self.sync_progress = None;
            }
            return;
        }
        let text = if done {
            format!("Đã đồng bộ {fetched} tin nhắn")
        } else if fetched == 0 {
            "Đang đồng bộ tin nhắn…".to_string()
        } else {
            format!("Đang đồng bộ tin nhắn… {fetched} tin")
        };
        let resp = egui::Frame::none()
            .fill(pal.primary.linear_multiply(0.08))
            .rounding(Rounding::same(8.0))
            .inner_margin(Margin::symmetric(10.0, 6.0))
            .outer_margin(Margin {
                left: 12.0,
                right: 12.0,
                top: 0.0,
                bottom: 6.0,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let (icon, color) = if done {
                        (lucide::CHECK, pal.accent_online)
                    } else {
                        (lucide::REFRESH, pal.primary)
                    };
                    let (ir, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                    draw_lucide_icon(ui.painter(), ir.center(), icon, 13.0, color);
                    ui.label(RichText::new(text).size(12.0).color(pal.text_secondary));
                });
            });
        if !done {
            // Indeterminate progress line along the bottom edge of the strip.
            let r = resp.response.rect;
            let line = Rect::from_min_max(Pos2::new(r.left() + 8.0, r.bottom() - 2.0), Pos2::new(r.right() - 8.0, r.bottom()));
            let t = (ui.ctx().input(|i| i.time) * 0.8).fract() as f32;
            let seg_w = line.width() * 0.3;
            let x0 = line.left() + (line.width() + seg_w) * t - seg_w;
            let seg = Rect::from_min_max(
                Pos2::new(x0.max(line.left()), line.top()),
                Pos2::new((x0 + seg_w).min(line.right()), line.bottom()),
            );
            if seg.width() > 0.0 {
                ui.painter().rect_filled(seg, Rounding::same(1.0), pal.primary);
            }
            ui.ctx().request_repaint_after(Duration::from_millis(40));
        } else {
            ui.ctx().request_repaint_after(SHOW_DONE);
        }
    }

    /// Banner when the server dropped part of an offline queue (`missing_ranges`), like Zalo's
    /// "Tin nhắn chưa được đồng bộ". QTzalo cannot fetch those messages, so it points to the
    /// phone; "Đóng" dismisses the current ranges.
    fn render_missing_banner(&mut self, ui: &mut egui::Ui, pal: &ThemePalette) {
        if self.missing_ranges.is_empty() {
            return;
        }
        let from = self.missing_ranges.iter().map(|r| r.from_ts).min().unwrap_or(0);
        let to = self.missing_ranges.iter().map(|r| r.to_ts).max().unwrap_or(0);
        let span = if from > 0 {
            format!(
                "Có thể thiếu tin từ {} đến {}.",
                crate::timefmt::dm_hm_from_ms(from),
                crate::timefmt::dm_hm_from_ms(to)
            )
        } else {
            format!("Có thể thiếu tin trước {}.", crate::timefmt::dm_hm_from_ms(to))
        };
        let mut dismiss = false;
        egui::Frame::none()
            .fill(pal.warning.linear_multiply(0.12))
            .stroke(Stroke::new(1.0_f32, pal.warning.linear_multiply(0.55)))
            .rounding(Rounding::same(10.0))
            .inner_margin(Margin::symmetric(10.0, 8.0))
            .outer_margin(Margin {
                left: 12.0,
                right: 12.0,
                top: 0.0,
                bottom: 6.0,
            })
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let (ir, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                    draw_lucide_icon(ui.painter(), ir.center(), lucide::INBOX, 14.0, pal.warning);
                    let text_w = (ui.available_width() - 26.0).max(60.0);
                    ui.allocate_ui_with_layout(Vec2::new(text_w, 0.0), Layout::top_down(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(
                            RichText::new("Tin nhắn chưa được đồng bộ đủ")
                                .size(12.5)
                                .strong()
                                .color(pal.text_primary),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("{span} Xem đầy đủ trên điện thoại."))
                                    .size(11.5)
                                    .color(pal.text_secondary),
                            )
                            .wrap(),
                        );
                    });
                    if icon_button(ui, lucide::X, 22.0, 12.0, false, pal, "Đóng").clicked() {
                        dismiss = true;
                    }
                });
            });
        if dismiss {
            self.missing_ranges.clear();
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::DismissMissingRanges);
            }
        }
    }

    /// Does thread `t` belong to the active navigation section (before search filtering)?
    pub(super) fn thread_in_section(&self, t: &Thread) -> bool {
        use crate::chat::FriendshipStatus as F;
        let base = match self.thread_filter {
            ThreadFilter::All => true,
            ThreadFilter::Direct => !t.is_group,
            ThreadFilter::Groups => t.is_group,
        };
        let in_section = match self.nav_section {
            NavSection::Messages => true,
            NavSection::Other | NavSection::Hidden => false,
            NavSection::Unread => self.unread_counts.get(&t.id).copied().unwrap_or(0) > 0,
            NavSection::Strangers => !t.is_group && matches!(self.friendship_of(t), F::Stranger),
            NavSection::Friends => {
                !t.is_group
                    && !Self::is_cloud_thread(t)
                    && !matches!(self.friendship_of(t), F::Stranger | F::IncomingPending)
            }
            NavSection::Groups => t.is_group,
            NavSection::FriendRequests => {
                !t.is_group && matches!(self.friendship_of(t), F::IncomingPending)
            }
        };
        base && in_section
    }

    /// Refresh the "local data" size shown under zCloud (background thread, every 2 minutes).
    fn maybe_refresh_storage_usage(&mut self) {
        if self.demo_mode {
            return;
        }
        let due = self
            .storage_scan_at
            .map(|t| t.elapsed() >= std::time::Duration::from_secs(120))
            .unwrap_or(true);
        if !due {
            return;
        }
        self.storage_scan_at = Some(std::time::Instant::now());
        let slot = self.local_storage_bytes.clone();
        let _ = std::thread::Builder::new()
            .name("qtzalo-storage-scan".into())
            .spawn(move || {
                let bytes = dir_size(crate::paths::data_dir());
                slot.store(bytes, std::sync::atomic::Ordering::Relaxed);
            });
    }

    fn nav_items(&self) -> [Vec<NavItem>; 3] {
        use crate::chat::FriendshipStatus as F;
        let unread_threads = self.unread_counts.values().filter(|n| **n > 0).count();
        let stranger_cnt = self
            .threads
            .iter()
            .filter(|t| !t.is_group && matches!(self.friendship_of(t), F::Stranger))
            .count();
        let request_cnt = self
            .threads
            .iter()
            .filter(|t| !t.is_group && matches!(self.friendship_of(t), F::IncomingPending))
            .count();
        let item = |section: NavSection, icon, label, badge| NavItem {
            section: Some(section),
            icon,
            label,
            badge,
            soon: None,
            action: NavAction::Section,
        };
        let inbox = vec![
            item(NavSection::Messages, lucide::INBOX, "Tin nhắn", 0),
            NavItem {
                soon: Some("Hộp thư \"Khác\" (phân loại ưu tiên) sẽ có trong bản cập nhật sau"),
                ..item(NavSection::Other, lucide::ARCHIVE, "Khác", 0)
            },
            item(NavSection::Unread, lucide::MESSAGES_SQUARE, "Chưa đọc", unread_threads),
            item(NavSection::Strangers, lucide::USER_SEARCH, "Người lạ", 0),
            NavItem {
                soon: Some("Trò chuyện ẩn (khoá bằng mã PIN) sẽ có trong bản cập nhật sau"),
                ..item(NavSection::Hidden, lucide::EYE_OFF, "Trò chuyện ẩn", 0)
            },
        ];
        let storage = vec![
            NavItem {
                section: None,
                icon: lucide::CLOUD,
                label: "Cloud của tôi",
                badge: 0,
                soon: None,
                action: NavAction::OpenCloud,
            },
            NavItem {
                section: None,
                icon: lucide::HARD_DRIVE,
                label: "zCloud",
                badge: 0,
                soon: Some("zCloud chưa được hỗ trợ trong QTzalo — dung lượng bên dưới là dữ liệu lưu trên máy"),
                action: NavAction::ZCloud,
            },
        ];
        let contacts = vec![
            item(NavSection::Friends, lucide::USER, "Bạn bè", 0),
            item(NavSection::Groups, lucide::USERS, "Nhóm và cộng đồng", 0),
            item(NavSection::FriendRequests, lucide::USER_PLUS, "Lời mời kết bạn", request_cnt),
        ];
        // Strangers badge is informational only; keep the list calm like the reference.
        let _ = stranger_cnt;
        [inbox, storage, contacts]
    }

    fn activate_nav_item(&mut self, item_section: Option<NavSection>, action: NavAction, soon: Option<&'static str>) {
        match action {
            NavAction::Section => {
                if let Some(sec) = item_section {
                    self.nav_section = sec;
                    if sec != NavSection::Messages {
                        self.thread_filter = ThreadFilter::All;
                    }
                }
                if let Some(msg) = soon {
                    self.show_toast(msg);
                }
            }
            NavAction::OpenCloud => {
                self.nav_section = NavSection::Messages;
                if let Some((cidx, cid)) = self
                    .threads
                    .iter()
                    .enumerate()
                    .find(|(_, t)| Self::is_cloud_thread(t))
                    .map(|(i, t)| (i, t.id.clone()))
                {
                    self.selected_thread_index = cidx;
                    if let Some(tx) = WORKER_TX.get() {
                        let _ = tx.send(WorkerCmd::LoadGroupHistory {
                            group_id: cid,
                            is_group: false,
                        });
                    }
                } else {
                    self.show_toast("Chưa tìm thấy hội thoại \"Cloud của tôi\"");
                }
            }
            NavAction::ZCloud => {
                if let Some(msg) = soon {
                    self.show_toast(msg);
                }
            }
        }
    }

    fn is_nav_item_active(&self, item: &NavItem) -> bool {
        match item.action {
            NavAction::Section => item.section == Some(self.nav_section),
            NavAction::OpenCloud => self
                .threads
                .get(self.selected_thread_index)
                .map(Self::is_cloud_thread)
                .unwrap_or(false),
            NavAction::ZCloud => false,
        }
    }

    /// Render the left navigation sidebar (expanded list or icon rail).
    pub(super) fn render_nav_sidebar(&mut self, ctx: &egui::Context, pal: &ThemePalette, expanded: bool, forced_collapsed: bool) {
        self.maybe_refresh_storage_usage();
        let frame = egui::Frame::none()
            .fill(pal.bg_nav)
            .stroke(Stroke::new(1.0_f32, pal.border));
        let collapsed_panel = egui::SidePanel::left("nav_sidebar_rail")
            .resizable(false)
            .exact_width(NAV_W_RAIL)
            .frame(frame);
        let expanded_panel = egui::SidePanel::left("nav_sidebar_full")
            .resizable(false)
            .exact_width(NAV_W_EXPANDED)
            .frame(frame);
        egui::SidePanel::show_animated_between(ctx, expanded, collapsed_panel, expanded_panel, |ui, how| {
            let full = how > 0.55;
            self.render_nav_contents(ui, pal, full, forced_collapsed);
        });
    }

    fn render_nav_contents(&mut self, ui: &mut egui::Ui, pal: &ThemePalette, full: bool, forced_collapsed: bool) {
        // Footer (profile + settings) pinned to the bottom.
        egui::TopBottomPanel::bottom(if full { "nav_footer_full" } else { "nav_footer_rail" })
            .frame(
                egui::Frame::none()
                    .fill(pal.bg_nav)
                    .inner_margin(Margin {
                        left: if full { 12.0 } else { 6.0 },
                        right: if full { 10.0 } else { 6.0 },
                        top: 10.0,
                        bottom: 12.0,
                    }),
            )
            .show_separator_line(true)
            .show_inside(ui, |ui| self.render_nav_footer(ui, pal, full));

        egui::ScrollArea::vertical()
            .id_salt(if full { "nav_scroll_full" } else { "nav_scroll_rail" })
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.add_space(12.0);
                let groups = self.nav_items();
                let titles = ["Hộp thư", "Lưu trữ", "Danh bạ"];
                let mut clicked: Option<(Option<NavSection>, NavAction, Option<&'static str>)> = None;

                for (gi, (title, items)) in titles.iter().zip(groups.iter()).enumerate() {
                    if full {
                        ui.horizontal(|ui| {
                            ui.add_space(14.0);
                            ui.label(RichText::new(*title).size(12.0).color(pal.text_muted));
                            if gi == 0 && !forced_collapsed {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.add_space(8.0);
                                    if icon_button(ui, lucide::PANEL_LEFT_CLOSE, 26.0, 15.0, false, pal, "Thu gọn thanh điều hướng").clicked() {
                                        self.settings.nav_collapsed = true;
                                        self.nav_user_override = Some(false);
                                        self.settings.save();
                                    }
                                });
                            }
                        });
                        ui.add_space(2.0);
                    } else if gi == 0 {
                        ui.vertical_centered(|ui| {
                            let tip = if forced_collapsed {
                                "Cửa sổ hẹp: thanh điều hướng tự thu gọn"
                            } else {
                                "Mở rộng thanh điều hướng"
                            };
                            if icon_button(ui, lucide::PANEL_LEFT_OPEN, 36.0, 17.0, false, pal, tip).clicked() && !forced_collapsed {
                                self.settings.nav_collapsed = false;
                                self.settings.save();
                                self.nav_user_override = Some(true);
                            }
                        });
                        ui.add_space(6.0);
                    } else {
                        ui.add_space(4.0);
                        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 9.0), Sense::hover());
                        ui.painter().line_segment(
                            [Pos2::new(r.left() + 16.0, r.center().y), Pos2::new(r.right() - 16.0, r.center().y)],
                            Stroke::new(1.0_f32, pal.border),
                        );
                        ui.add_space(4.0);
                    }

                    for item in items {
                        let active = self.is_nav_item_active(item);
                        let resp = if full {
                            self.paint_nav_row(ui, pal, item, active)
                        } else {
                            self.paint_nav_icon(ui, pal, item, active)
                        };
                        if resp.clicked() {
                            clicked = Some((item.section, item.action, item.soon));
                        }
                        if item.action == NavAction::ZCloud && full {
                            self.paint_storage_bar(ui, pal);
                        }
                    }
                    if full {
                        ui.add_space(10.0);
                    }
                }
                if let Some((sec, act, soon)) = clicked {
                    self.activate_nav_item(sec, act, soon);
                }
            });
    }

    fn paint_nav_row(&mut self, ui: &mut egui::Ui, pal: &ThemePalette, item: &NavItem, active: bool) -> egui::Response {
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 36.0), Sense::click());
        let row = Rect::from_min_max(
            Pos2::new(rect.left() + 8.0, rect.top() + 1.0),
            Pos2::new(rect.right() - 8.0, rect.bottom() - 1.0),
        );
        let bg = if active {
            pal.nav_active
        } else if resp.hovered() {
            pal.bg_hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(row, Rounding::same(10.0), bg);
        let fg = if active { pal.primary } else { pal.text_primary };
        let icon_fg = if active { pal.primary } else { pal.text_secondary };
        draw_lucide_icon(ui.painter(), Pos2::new(row.left() + 18.0, row.center().y), item.icon, 16.5, icon_fg);
        let mut label_max = row.width() - 46.0;
        if item.badge > 0 {
            let c = Pos2::new(row.right() - 18.0, row.center().y);
            let b = paint_count_badge(ui.painter(), c, item.badge, pal.badge);
            label_max -= b.width() + 6.0;
        }
        let g = single_line_galley(ui, item.label, 14.0, fg, label_max);
        ui.painter().galley(
            Pos2::new(row.left() + 38.0, row.center().y - g.size().y / 2.0),
            g,
            fg,
        );
        let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
        match item.soon {
            Some(msg) => resp.on_hover_text(msg),
            None => resp,
        }
    }

    fn paint_nav_icon(&mut self, ui: &mut egui::Ui, pal: &ThemePalette, item: &NavItem, active: bool) -> egui::Response {
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 42.0), Sense::click());
        let cell = Rect::from_center_size(rect.center(), Vec2::splat(40.0));
        let bg = if active {
            pal.nav_active
        } else if resp.hovered() {
            pal.bg_hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(cell, Rounding::same(11.0), bg);
        draw_lucide_icon(
            ui.painter(),
            cell.center(),
            item.icon,
            18.0,
            if active { pal.primary } else { pal.text_secondary },
        );
        if item.badge > 0 {
            let c = Pos2::new(cell.right() - 6.0, cell.top() + 7.0);
            ui.painter().circle_filled(c, 7.5, pal.bg_nav);
            ui.painter().circle_filled(c, 6.0, pal.badge);
            if item.badge < 10 {
                ui.painter().text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    item.badge.to_string(),
                    FontId::proportional(9.0),
                    Color32::WHITE,
                );
            }
        }
        let tip = match (item.badge, item.soon) {
            (_, Some(s)) => format!("{} — {s}", item.label),
            (0, None) => item.label.to_string(),
            (n, None) => format!("{} ({n})", item.label),
        };
        resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(tip)
    }

    fn paint_storage_bar(&mut self, ui: &mut egui::Ui, pal: &ThemePalette) {
        let (used, total, label) = if self.demo_mode {
            let used = 1_288_490_189_u64; // 1.2 GB (sample)
            let total = 5_368_709_120_u64; // 5 GB (sample)
            (used, total, format!("Đã dùng {} trên {}", human_bytes(used), human_bytes(total)))
        } else {
            let used = self
                .local_storage_bytes
                .load(std::sync::atomic::Ordering::Relaxed);
            let total = 1_073_741_824_u64; // scale: 1 GB
            (used, total, format!("Dữ liệu trên máy: {}", human_bytes(used)))
        };
        let frac = if total == 0 { 0.0 } else { (used as f64 / total as f64).clamp(0.0, 1.0) as f32 };
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 30.0), Sense::hover());
        let bar = Rect::from_min_size(Pos2::new(rect.left() + 46.0, rect.top() + 3.0), Vec2::new((w - 66.0).max(40.0), 4.0));
        ui.painter().rect_filled(bar, Rounding::same(2.0), pal.border);
        let mut fill = bar;
        fill.set_width((bar.width() * frac).max(if used > 0 { 3.0 } else { 0.0 }));
        ui.painter().rect_filled(fill, Rounding::same(2.0), pal.primary);
        ui.painter().text(
            Pos2::new(bar.left(), bar.bottom() + 6.0),
            egui::Align2::LEFT_TOP,
            label,
            FontId::proportional(11.0),
            pal.text_muted,
        );
    }

    fn render_nav_footer(&mut self, ui: &mut egui::Ui, pal: &ThemePalette, full: bool) {
        let display = if self.user_name.is_empty() {
            "Người dùng Zalo".to_string()
        } else {
            self.user_name.clone()
        };
        if full {
            ui.horizontal(|ui| {
                let (av_rect, av_resp) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::click());
                paint_user_avatar(
                    ui.painter(),
                    av_rect.center(),
                    19.0,
                    &display,
                    false,
                    self.user_avatar.as_deref(),
                    &self.sticker_textures,
                    &mut self.loading_stickers,
                );
                if self.settings.show_online_status {
                    let c = Pos2::new(av_rect.right() - 4.0, av_rect.bottom() - 4.0);
                    ui.painter().circle_filled(c, 6.0, pal.bg_nav);
                    ui.painter().circle_filled(c, 4.2, pal.accent_online);
                }
                ui.add_space(4.0);
                let text_w = (ui.available_width() - 46.0).max(40.0);
                let (txt_rect, txt_resp) = ui.allocate_exact_size(Vec2::new(text_w, 38.0), Sense::click());
                let g_name = single_line_galley(ui, &display, 14.0, pal.text_primary, text_w);
                ui.painter().galley(Pos2::new(txt_rect.left(), txt_rect.top() + 2.0), g_name, pal.text_primary);
                let status = if self.demo_mode {
                    "Chế độ demo".to_string()
                } else if self.settings.show_online_status {
                    "Đang hoạt động".to_string()
                } else {
                    "Ẩn trạng thái".to_string()
                };
                let g_status = single_line_galley(ui, &status, 12.0, pal.text_muted, text_w);
                ui.painter().galley(Pos2::new(txt_rect.left(), txt_rect.top() + 21.0), g_status, pal.text_muted);
                if av_resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
                    || txt_resp
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text("Tài khoản & tuỳ chọn")
                        .clicked()
                {
                    self.profile_menu_open = !self.profile_menu_open;
                }
                if icon_button(ui, lucide::SETTINGS, 30.0, 16.0, self.settings_modal_open, pal, "Cài đặt").clicked() {
                    self.settings_modal_open = !self.settings_modal_open;
                }
            });
        } else {
            ui.vertical_centered(|ui| {
                if icon_button(ui, lucide::SETTINGS, 36.0, 17.0, self.settings_modal_open, pal, "Cài đặt").clicked() {
                    self.settings_modal_open = !self.settings_modal_open;
                }
                ui.add_space(6.0);
                let (av_rect, av_resp) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::click());
                paint_user_avatar(
                    ui.painter(),
                    av_rect.center(),
                    18.0,
                    &display,
                    false,
                    self.user_avatar.as_deref(),
                    &self.sticker_textures,
                    &mut self.loading_stickers,
                );
                if self.settings.show_online_status {
                    let c = Pos2::new(av_rect.right() - 5.0, av_rect.bottom() - 5.0);
                    ui.painter().circle_filled(c, 5.5, pal.bg_nav);
                    ui.painter().circle_filled(c, 3.8, pal.accent_online);
                }
                if av_resp
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text(format!("{display} — tài khoản & tuỳ chọn"))
                    .clicked()
                {
                    self.profile_menu_open = !self.profile_menu_open;
                }
            });
        }
    }

    // -----------------------------------------------------------------------
    // Conversation list
    // -----------------------------------------------------------------------

    /// Render the conversation list column (full list or avatar column).
    pub(super) fn render_conversation_list(
        &mut self,
        ctx: &egui::Context,
        pal: &ThemePalette,
        expanded: bool,
        forced_collapsed: bool,
        max_w: f32,
    ) {
        let frame = egui::Frame::none()
            .fill(pal.bg_list)
            .stroke(Stroke::new(1.0_f32, pal.border));
        let collapsed_panel = egui::SidePanel::left("conv_list_collapsed")
            .resizable(false)
            .exact_width(LIST_W_COLLAPSED)
            .frame(frame);
        let expanded_panel = egui::SidePanel::left("conv_list_expanded")
            .resizable(true)
            .default_width(self.settings.list_width.clamp(250.0, max_w.max(260.0)))
            .width_range(250.0..=max_w.max(260.0))
            .frame(frame);
        let shown = egui::SidePanel::show_animated_between(ctx, expanded, collapsed_panel, expanded_panel, |ui, how| {
            let full = how > 0.6;
            self.render_conversation_list_contents(ctx, ui, pal, full, forced_collapsed);
            ui.max_rect().width()
        });
        if expanded {
            if let Some(inner) = shown {
                let w = inner.response.rect.width();
                if (w - self.settings.list_width).abs() > 1.0 && w >= 250.0 && !ctx.input(|i| i.pointer.any_down()) {
                    self.settings.list_width = w;
                    self.settings.save();
                }
            }
        }
    }

    fn render_list_header(&mut self, ui: &mut egui::Ui, pal: &ThemePalette, forced_collapsed: bool) {
        let title = if self.nav_section == NavSection::Messages {
            match self.thread_filter {
                ThreadFilter::All => "Tin nhắn",
                ThreadFilter::Direct => "Tin nhắn · Cá nhân",
                ThreadFilter::Groups => "Tin nhắn · Nhóm",
            }
        } else {
            self.nav_section.title()
        };
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            let g = single_line_galley(ui, title, 19.0, pal.text_primary, (ui.available_width() - 100.0).max(60.0));
            let (r, _) = ui.allocate_exact_size(Vec2::new(g.size().x, 30.0), Sense::hover());
            ui.painter().galley(Pos2::new(r.left(), r.center().y - g.size().y / 2.0), g, pal.text_primary);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if !forced_collapsed
                    && icon_button(ui, lucide::CHEVRONS_LEFT, 28.0, 15.0, false, pal, "Thu gọn danh sách hội thoại").clicked()
                {
                    self.settings.list_collapsed = true;
                    self.settings.save();
                }
                if icon_button(ui, lucide::EDIT, 28.0, 15.5, self.phone_modal_open, pal, "Tìm bạn qua số điện thoại & kết bạn").clicked() {
                    self.phone_modal_open = true;
                }
                let filter_active = self.thread_filter != ThreadFilter::All || self.list_filter_menu_open;
                if icon_button(ui, lucide::TAG, 28.0, 15.5, filter_active, pal, "Phân loại hội thoại").clicked() {
                    self.list_filter_menu_open = !self.list_filter_menu_open;
                }
            });
        });
    }

    fn render_search_box(&mut self, ui: &mut egui::Ui, pal: &ThemePalette) -> Option<String> {
        let mut trigger_phone_lookup = None;
        let has_q = !self.search_query.is_empty();
        egui::Frame::none()
            .fill(pal.bg_input)
            .rounding(Rounding::same(10.0))
            .stroke(Stroke::new(
                1.0_f32,
                if has_q { pal.primary.linear_multiply(0.6) } else { pal.border },
            ))
            .inner_margin(Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(16.0, 20.0), Sense::hover());
                    draw_icon_search(ui.painter(), icon_rect.center(), 14.0, if has_q { pal.primary } else { pal.text_muted });
                    // TextEdit adds its own 4px side margins on top of `desired_width`.
                    let hint_w = if has_q { 24.0 } else { 42.0 };
                    let edit_w = (ui.available_width() - hint_w - 14.0).max(40.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .id(Id::new("sidebar_search_input"))
                            .frame(false)
                            .hint_text(RichText::new("Tìm kiếm").color(pal.text_muted))
                            .desired_width(edit_w),
                    );
                    if self.search_focus_pending {
                        resp.request_focus();
                        self.search_focus_pending = false;
                    }
                    let enter = (resp.lost_focus() || resp.has_focus()) && ui.input(|i| i.key_pressed(Key::Enter));
                    let digits = self.search_query.chars().filter(|c| c.is_ascii_digit()).count();
                    if enter && digits >= 8 {
                        trigger_phone_lookup = Some(self.search_query.trim().to_string());
                    }
                    if has_q {
                        if icon_button(ui, lucide::X, 18.0, 11.0, false, pal, "Xoá tìm kiếm").clicked() {
                            self.search_query.clear();
                        }
                    } else {
                        // Shortcut hint chip (⌘K on macOS, Ctrl K elsewhere).
                        let (chip, _) = ui.allocate_exact_size(Vec2::new(36.0, 20.0), Sense::hover());
                        ui.painter().rect_stroke(chip, Rounding::same(5.0), Stroke::new(1.0_f32, pal.border));
                        let label = if cfg!(target_os = "macos") { "⌘K" } else { "Ctrl K" };
                        ui.painter().text(
                            chip.center(),
                            egui::Align2::CENTER_CENTER,
                            label,
                            FontId::proportional(if cfg!(target_os = "macos") { 11.0 } else { 9.5 }),
                            pal.text_muted,
                        );
                    }
                });
            });
        trigger_phone_lookup
    }

    fn render_filter_chips(&mut self, ui: &mut egui::Ui, pal: &ThemePalette) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
            for (filter, label) in [
                (ThreadFilter::All, "Tất cả"),
                (ThreadFilter::Direct, "Cá nhân"),
                (ThreadFilter::Groups, "Nhóm"),
            ] {
                let active = self.thread_filter == filter;
                let btn = egui::Button::new(
                    RichText::new(label)
                        .size(12.5)
                        .color(if active { Color32::WHITE } else { pal.text_secondary }),
                )
                .fill(if active { pal.primary } else { pal.bg_elevated })
                .stroke(Stroke::new(1.0_f32, if active { pal.primary } else { pal.border }))
                .rounding(Rounding::same(12.0));
                if ui.add(btn).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    self.thread_filter = filter;
                    self.nav_section = NavSection::Messages;
                }
            }
        });
    }

    fn render_conversation_list_contents(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        pal: &ThemePalette,
        full: bool,
        forced_collapsed: bool,
    ) {
        let query = self.search_query.trim().to_lowercase();
        let query_folded = fold_vn_search(&query);
        let query_digits: String = self.search_query.chars().filter(|c| c.is_ascii_digit()).collect();
        let is_phone_like_query = query_digits.len() >= 8
            && self
                .search_query
                .trim()
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '+' | ' ' | '-' | '.'));

        // --- Header -------------------------------------------------------
        let mut trigger_phone_lookup: Option<String> = None;
        if full {
            egui::Frame::none()
                .inner_margin(Margin {
                    left: 14.0,
                    right: 12.0,
                    top: 14.0,
                    bottom: 8.0,
                })
                .show(ui, |ui| {
                    self.render_list_header(ui, pal, forced_collapsed);
                    ui.add_space(8.0);
                    trigger_phone_lookup = self.render_search_box(ui, pal);
                    if self.list_filter_menu_open || self.thread_filter != ThreadFilter::All {
                        ui.add_space(8.0);
                        self.render_filter_chips(ui, pal);
                    }
                });
        } else {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                let tip = if forced_collapsed {
                    "Cửa sổ hẹp: danh sách tự thu gọn"
                } else {
                    "Mở rộng danh sách hội thoại"
                };
                if icon_button(ui, lucide::CHEVRONS_RIGHT, 36.0, 16.0, false, pal, tip).clicked() && !forced_collapsed {
                    self.settings.list_collapsed = false;
                    self.settings.save();
                }
                ui.add_space(2.0);
                if icon_button(ui, lucide::SEARCH, 36.0, 16.0, false, pal, "Tìm kiếm (Ctrl+K)").clicked() {
                    if forced_collapsed {
                        self.phone_modal_open = true;
                    } else {
                        self.settings.list_collapsed = false;
                        self.settings.save();
                        self.search_focus_pending = true;
                    }
                }
            });
            ui.add_space(6.0);
        }

        if full {
            self.render_sync_strip(ui, pal);
            self.render_missing_banner(ui, pal);
        }

        if let Some(phone_str) = trigger_phone_lookup {
            self.start_phone_lookup(phone_str);
        }

        if full && is_phone_like_query {
            let raw_q = self.search_query.trim().to_string();
            let mut lookup = false;
            egui::Frame::none()
                .fill(pal.primary.linear_multiply(0.10))
                .stroke(Stroke::new(1.0_f32, pal.primary.linear_multiply(0.6)))
                .rounding(Rounding::same(10.0))
                .inner_margin(Margin::symmetric(10.0, 7.0))
                .outer_margin(Margin::symmetric(12.0, 4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (ir, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                        draw_icon_user_plus(ui.painter(), ir.center(), 16.0, pal.primary);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(format!("Tìm Zalo qua SĐT: {raw_q}")).size(12.5).color(pal.text_primary).strong());
                            ui.label(RichText::new("Nhấn Enter hoặc nút Tìm để tra cứu").size(11.0).color(pal.text_secondary));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let btn = egui::Button::new(RichText::new("Tìm").size(12.0).color(Color32::WHITE).strong())
                                .fill(pal.primary)
                                .rounding(Rounding::same(8.0));
                            if ui.add(btn).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                lookup = true;
                            }
                        });
                    });
                });
            if lookup {
                self.start_phone_lookup(raw_q);
            }
        }

        // --- Filtering & ordering ----------------------------------------
        let selected_id = self.threads.get(self.selected_thread_index).map(|t| t.id.clone());
        let mut filtered: Vec<usize> = self
            .threads
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                // "Tin nhắn" lists conversations with activity, like the phone; a search looks
                // through every contact and group.
                let listed = self.thread_in_section(t)
                    && (self.nav_section != NavSection::Messages
                        || !query.is_empty()
                        || self.thread_has_activity(t)
                        || selected_id.as_deref() == Some(t.id.as_str()));
                listed
                    && (query.is_empty() || matches_vn_query(&t.name, &query, &query_folded) || t.id.contains(&query))
            })
            .map(|(i, _)| i)
            .collect();
        filtered.sort_by(|&a, &b| {
            let ta = &self.threads[a];
            let tb = &self.threads[b];
            let (ac, bc) = (Self::is_cloud_thread(ta), Self::is_cloud_thread(tb));
            self.thread_activity_ms(tb)
                .cmp(&self.thread_activity_ms(ta))
                .then(bc.cmp(&ac))
        });

        let msg_hits: Vec<(usize, String, String, String, bool)> = if full && query.chars().count() >= 2 {
            let mut hits = Vec::new();
            'outer: for (t_idx, t) in self.threads.iter().enumerate() {
                for m in t.messages.iter().rev() {
                    if matches_vn_query(&m.text, &query, &query_folded) {
                        hits.push((t_idx, t.name.clone(), m.text.clone(), m.time.clone(), t.is_group));
                        if hits.len() >= 25 {
                            break 'outer;
                        }
                    }
                }
            }
            hits
        } else {
            Vec::new()
        };

        let mut clicked_thread: Option<(usize, String, bool)> = None;
        let mut clicked_msg_hit: Option<(usize, String)> = None;

        egui::ScrollArea::vertical()
            .id_salt(if full { "threads_list_scroll" } else { "threads_list_scroll_collapsed" })
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(2.0);
                if matches!(self.nav_section, NavSection::Other | NavSection::Hidden) {
                    if full {
                        Self::paint_empty_state(
                            ui,
                            pal,
                            if self.nav_section == NavSection::Other { lucide::ARCHIVE } else { lucide::EYE_OFF },
                            "Sắp có",
                            if self.nav_section == NavSection::Other {
                                "Hộp thư \"Khác\" sẽ gom các hội thoại ít ưu tiên. Tính năng đang được phát triển."
                            } else {
                                "Trò chuyện ẩn (khoá bằng mã PIN) đang được phát triển."
                            },
                        );
                    }
                    return;
                }
                for &i in &filtered {
                    let t = &self.threads[i];
                    let selected = i == self.selected_thread_index;
                    let unread = self.unread_counts.get(&t.id).copied().unwrap_or(0);
                    let resp = if full {
                        self.paint_thread_row(ui, pal, i, selected, unread)
                    } else {
                        self.paint_thread_avatar_cell(ui, pal, i, selected, unread)
                    };
                    if resp.clicked() {
                        let t = &self.threads[i];
                        clicked_thread = Some((i, t.id.clone(), t.is_group));
                    }
                }

                if !msg_hits.is_empty() {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.label(RichText::new(format!("TIN NHẮN TÌM THẤY ({})", msg_hits.len())).size(11.0).color(pal.primary).strong());
                    });
                    ui.add_space(4.0);
                    for (t_idx, t_name, m_text, m_time, t_group) in &msg_hits {
                        let w = ui.available_width();
                        let (r_rect, r_resp) = ui.allocate_exact_size(Vec2::new(w, 54.0), Sense::click());
                        let row = r_rect.shrink2(Vec2::new(8.0, 1.0));
                        if r_resp.hovered() {
                            ui.painter().rect_filled(row, Rounding::same(12.0), pal.bg_hover);
                        }
                        let av_c = Pos2::new(row.left() + 26.0, row.center().y);
                        let ava = self.threads.get(*t_idx).and_then(|th| th.avatar.clone());
                        paint_user_avatar(ui.painter(), av_c, 17.0, t_name, *t_group, ava.as_deref(), &self.sticker_textures, &mut self.loading_stickers);
                        let tl = row.left() + 52.0;
                        let tw = (row.right() - tl - 56.0).max(40.0);
                        let g1 = single_line_galley(ui, t_name, 13.0, pal.text_primary, tw);
                        ui.painter().galley(Pos2::new(tl, row.top() + 9.0), g1, pal.text_primary);
                        if !m_time.is_empty() {
                            ui.painter().text(Pos2::new(row.right() - 10.0, row.top() + 10.0), egui::Align2::RIGHT_TOP, m_time, FontId::proportional(11.0), pal.text_muted);
                        }
                        let g2 = single_line_galley(ui, m_text, 12.0, pal.primary, row.right() - tl - 10.0);
                        ui.painter().galley(Pos2::new(tl, row.top() + 29.0), g2, pal.primary);
                        if r_resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(m_text).clicked() {
                            clicked_msg_hit = Some((*t_idx, query.clone()));
                        }
                    }
                }

                if full && filtered.is_empty() && msg_hits.is_empty() {
                    if !query.is_empty() {
                        Self::paint_empty_state(ui, pal, lucide::SEARCH, "Không tìm thấy", "Không có hội thoại hoặc tin nhắn phù hợp.");
                    } else {
                        let (icon, title, body) = match self.nav_section {
                            NavSection::Unread => (lucide::CHECK_CHECK, "Đã đọc hết", "Bạn không còn tin nhắn chưa đọc."),
                            NavSection::Strangers => (lucide::USER_SEARCH, "Không có người lạ", "Tin nhắn từ người chưa kết bạn sẽ hiện ở đây."),
                            NavSection::FriendRequests => (lucide::USER_PLUS, "Chưa có lời mời", "Lời mời kết bạn mới sẽ hiện ở đây."),
                            NavSection::Groups => (lucide::USERS, "Chưa có nhóm", "Các nhóm bạn tham gia sẽ hiện ở đây."),
                            NavSection::Friends => (lucide::USER, "Chưa có bạn bè", "Danh bạ bạn bè sẽ hiện ở đây sau khi làm mới."),
                            _ => (lucide::MESSAGES_SQUARE, "Chưa có hội thoại", "Bấm \"Làm mới hội thoại\" trong menu tài khoản để tải lại."),
                        };
                        Self::paint_empty_state(ui, pal, icon, title, body);
                    }
                }
                ui.add_space(8.0);
            });

        if let Some((idx, q)) = clicked_msg_hit {
            self.selected_thread_index = idx;
            self.in_chat_search_open = true;
            self.in_chat_search_focus_pending = true;
            self.in_chat_search_query = q;
        }
        if let Some((idx, tid, is_group)) = clicked_thread {
            self.select_thread(ctx, idx, tid, is_group);
        }
    }

    /// Select a conversation: clears unread, updates the window title, loads history.
    pub(super) fn select_thread(&mut self, ctx: &egui::Context, idx: usize, tid: String, is_group: bool) {
        if self.selected_thread_index != idx {
            self.replying_to = None;
            self.filter_starred_only = false;
        }
        self.selected_thread_index = idx;
        if self.unread_counts.remove(&tid).is_some() {
            let total_unread: usize = self.unread_counts.values().sum();
            let title = if total_unread > 0 {
                format!("({total_unread}) QTzalo")
            } else {
                "QTzalo".to_string()
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
        let is_cloud = self.threads.get(idx).map(Self::is_cloud_thread).unwrap_or(false);
        if is_group || is_cloud {
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::LoadGroupHistory { group_id: tid, is_group });
            }
        }
    }

    pub(super) fn start_phone_lookup(&mut self, phone: String) {
        self.phone_modal_open = true;
        self.phone_modal_input = phone.clone();
        self.phone_lookup_loading = true;
        self.phone_lookup_error = None;
        self.phone_lookup_result = None;
        if let Some(tx) = WORKER_TX.get() {
            let _ = tx.send(WorkerCmd::LookupPhone { phone });
        } else {
            self.phone_lookup_loading = false;
            self.phone_lookup_error = Some("Không có kết nối (chế độ demo hoặc chưa đăng nhập)".into());
        }
    }

    fn paint_empty_state(ui: &mut egui::Ui, pal: &ThemePalette, icon: &str, title: &str, body: &str) {
        ui.add_space(36.0);
        ui.vertical_centered(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 28.0, pal.nav_active);
            draw_lucide_icon(ui.painter(), r.center(), icon, 24.0, pal.primary);
            ui.add_space(10.0);
            ui.label(RichText::new(title).size(14.5).color(pal.text_primary).strong());
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(body).size(12.5).color(pal.text_muted)).wrap());
        });
    }

    fn thread_preview_text(&self, t: &Thread) -> String {
        let last_outgoing = t.messages.last().map(|m| m.is_outgoing).unwrap_or(false);
        let base = if t.preview.is_empty() {
            t.messages.last().map(|m| m.text.clone()).unwrap_or_else(|| "Chưa có tin nhắn".into())
        } else {
            t.preview.clone()
        };
        if last_outgoing && !base.starts_with("Bạn:") && !Self::is_cloud_thread(t) {
            format!("Bạn: {base}")
        } else {
            base
        }
    }

    fn paint_thread_row(&mut self, ui: &mut egui::Ui, pal: &ThemePalette, i: usize, selected: bool, unread: usize) -> egui::Response {
        let (name, is_group, avatar, ts, fallback_time, is_cloud) = {
            let t = &self.threads[i];
            (
                t.name.clone(),
                t.is_group,
                t.avatar.clone(),
                t.messages.last().map(|m| normalize_ts_ms(m.ts)).unwrap_or(0),
                t.time.clone(),
                Self::is_cloud_thread(t),
            )
        };
        let preview = self.thread_preview_text(&self.threads[i]);
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 68.0), Sense::click());
        let row = Rect::from_min_max(Pos2::new(rect.left() + 8.0, rect.top() + 2.0), Pos2::new(rect.right() - 8.0, rect.bottom() - 2.0));
        let bg = if selected {
            pal.bg_selected
        } else if resp.hovered() {
            pal.bg_hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(row, Rounding::same(14.0), bg);

        let av_c = Pos2::new(row.left() + 32.0, row.center().y);
        if is_cloud {
            ui.painter().circle_filled(av_c, 24.0, if pal.is_dark { pal.bg_elevated } else { Color32::WHITE });
            ui.painter().circle_stroke(av_c, 24.0, Stroke::new(1.0_f32, pal.border));
            draw_lucide_icon(ui.painter(), av_c, lucide::CLOUD, 19.0, pal.primary);
        } else {
            paint_user_avatar(ui.painter(), av_c, 24.0, &name, is_group, avatar.as_deref(), &self.sticker_textures, &mut self.loading_stickers);
        }

        let text_left = row.left() + 66.0;
        let time_txt = format_list_time(ts, &fallback_time);
        let time_w = if time_txt.is_empty() {
            0.0
        } else {
            let g = single_line_galley(ui, &time_txt, 12.0, pal.text_muted, 80.0);
            let tw = g.size().x;
            ui.painter().galley(Pos2::new(row.right() - 12.0 - tw, row.top() + 15.0), g, pal.text_muted);
            tw
        };
        let name_max = (row.right() - text_left - time_w - 20.0).max(30.0);
        let name_col = if selected && pal.is_dark { Color32::WHITE } else { pal.text_primary };
        let g_name = single_line_galley(ui, &name, 14.5, name_col, name_max);
        ui.painter().galley(Pos2::new(text_left, row.top() + 13.0), g_name, name_col);

        let mut prev_max = (row.right() - text_left - 14.0).max(30.0);
        if unread > 0 {
            let b = paint_count_badge(ui.painter(), Pos2::new(row.right() - 22.0, row.top() + 44.0), unread, pal.badge);
            prev_max -= b.width() + 12.0;
        }
        let prev_col = if unread > 0 { pal.text_primary } else { pal.text_secondary };
        let g_prev = single_line_galley(ui, &preview, 13.0, prev_col, prev_max);
        ui.painter().galley(Pos2::new(text_left, row.top() + 35.0), g_prev, prev_col);

        resp.on_hover_cursor(CursorIcon::PointingHand)
    }

    /// Composer popovers: the (+) attachment menu and the quick emoji grid.
    pub(super) fn render_composer_popups(
        &mut self,
        ctx: &egui::Context,
        pal: &ThemePalette,
        plus_rect: Rect,
        emoji_rect: Rect,
        thread_id: &str,
        is_group: bool,
    ) {
        let pointer_in = |r: Rect| {
            ctx.input(|i| i.pointer.interact_pos().map(|p| r.contains(p)).unwrap_or(false))
        };
        let popup_frame = egui::Frame::none()
            .fill(pal.bg_elevated)
            .stroke(Stroke::new(1.0_f32, pal.border))
            .rounding(Rounding::same(12.0))
            .inner_margin(Margin::same(6.0))
            .shadow(egui::epaint::Shadow {
                offset: Vec2::new(0.0, 4.0),
                blur: 16.0,
                spread: 0.0,
                color: pal.shadow,
            });

        if self.attach_menu_open && plus_rect.is_positive() {
            let mut action: Option<u8> = None;
            let area = egui::Area::new(Id::new("composer_attach_menu"))
                .order(egui::Order::Foreground)
                .pivot(egui::Align2::LEFT_BOTTOM)
                .fixed_pos(Pos2::new(plus_rect.left(), plus_rect.top() - 8.0))
                .show(ctx, |ui| {
                    popup_frame.show(ui, |ui| {
                        ui.set_width(230.0);
                        for (code, icon, label) in [
                            (0u8, lucide::IMAGE, "Gửi hình ảnh"),
                            (1u8, lucide::PAPERCLIP, "Đính kèm tệp / video"),
                            (2u8, lucide::COPY, "Dán ảnh từ Clipboard"),
                        ] {
                            let (r, resp) = ui.allocate_exact_size(Vec2::new(230.0, 34.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(r, Rounding::same(8.0), pal.bg_hover);
                            }
                            draw_lucide_icon(ui.painter(), Pos2::new(r.left() + 18.0, r.center().y), icon, 16.0, pal.primary);
                            ui.painter().text(
                                Pos2::new(r.left() + 38.0, r.center().y),
                                egui::Align2::LEFT_CENTER,
                                label,
                                FontId::proportional(13.5),
                                pal.text_primary,
                            );
                            if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                action = Some(code);
                            }
                        }
                    });
                });
            if area.response.clicked_elsewhere() && !pointer_in(plus_rect) {
                self.attach_menu_open = false;
            }
            if let Some(code) = action {
                self.attach_menu_open = false;
                match code {
                    0 | 1 => {
                        let cap = self.input_message.trim().to_string();
                        self.input_message.clear();
                        if let Some(tx) = WORKER_TX.get() {
                            let _ = tx.send(WorkerCmd::PickAndSendAttachment {
                                to_id: thread_id.to_string(),
                                is_group,
                                images_only: code == 0,
                                caption: cap,
                                preset_path: None,
                            });
                        } else {
                            self.show_toast("Chưa kết nối — không thể gửi tệp (chế độ demo)");
                        }
                    }
                    _ => {
                        if let Some((path, w, h, col_img)) = read_clipboard_image_to_png() {
                            let tex = ctx.load_texture(
                                format!("clip_preview_{}", crate::timefmt::now_ms()),
                                col_img,
                                TextureOptions::LINEAR,
                            );
                            self.pending_clipboard_image = Some((path, w, h, tex));
                            self.composer_focus_pending = true;
                            self.show_toast(format!("Đã dán ảnh ({w}×{h} px) — nhấn Enter để gửi"));
                        } else {
                            self.show_toast("Không tìm thấy hình ảnh nào trong Clipboard");
                        }
                    }
                }
            }
        }

        if self.emoji_popup_open && emoji_rect.is_positive() {
            const QUICK_EMOJIS: [&str; 32] = [
                "😀", "😂", "🥰", "😍", "😘", "😊", "😉", "😎", "🤔", "😅", "😭", "😡", "😴", "🤗",
                "😮", "🥲", "👍", "👏", "🙏", "💪", "🤝", "👋", "👀", "✌️", "❤️", "🔥", "🎉", "✨",
                "🌹", "☕", "🎂", "💯",
            ];
            let mut picked: Option<&str> = None;
            let area = egui::Area::new(Id::new("composer_emoji_popup"))
                .order(egui::Order::Foreground)
                .pivot(egui::Align2::LEFT_BOTTOM)
                .fixed_pos(Pos2::new(emoji_rect.left() - 40.0, emoji_rect.top() - 8.0))
                .show(ctx, |ui| {
                    popup_frame.show(ui, |ui| {
                        ui.label(RichText::new("Biểu tượng cảm xúc").size(12.0).color(pal.text_muted));
                        ui.add_space(4.0);
                        egui::Grid::new("emoji_grid").spacing(Vec2::new(2.0, 2.0)).show(ui, |ui| {
                            for (i, e) in QUICK_EMOJIS.iter().enumerate() {
                                let (r, resp) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
                                if resp.hovered() {
                                    ui.painter().rect_filled(r, Rounding::same(8.0), pal.bg_hover);
                                }
                                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, *e, FontId::proportional(19.0), pal.text_primary);
                                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                    picked = Some(e);
                                }
                                if i % 8 == 7 {
                                    ui.end_row();
                                }
                            }
                        });
                    });
                });
            if area.response.clicked_elsewhere() && !pointer_in(emoji_rect) {
                self.emoji_popup_open = false;
            }
            if let Some(e) = picked {
                self.input_message.push_str(e);
                self.composer_focus_pending = true;
            }
        }
    }

    fn paint_thread_avatar_cell(&mut self, ui: &mut egui::Ui, pal: &ThemePalette, i: usize, selected: bool, unread: usize) -> egui::Response {
        let (name, is_group, avatar, is_cloud) = {
            let t = &self.threads[i];
            (t.name.clone(), t.is_group, t.avatar.clone(), Self::is_cloud_thread(t))
        };
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 58.0), Sense::click());
        let cell = Rect::from_center_size(rect.center(), Vec2::splat(54.0));
        let bg = if selected {
            pal.bg_selected
        } else if resp.hovered() {
            pal.bg_hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(cell, Rounding::same(14.0), bg);
        let c = cell.center();
        if is_cloud {
            ui.painter().circle_filled(c, 21.0, if pal.is_dark { pal.bg_elevated } else { Color32::WHITE });
            ui.painter().circle_stroke(c, 21.0, Stroke::new(1.0_f32, pal.border));
            draw_lucide_icon(ui.painter(), c, lucide::CLOUD, 17.0, pal.primary);
        } else {
            paint_user_avatar(ui.painter(), c, 21.0, &name, is_group, avatar.as_deref(), &self.sticker_textures, &mut self.loading_stickers);
        }
        if unread > 0 {
            let bc = Pos2::new(c.x + 16.0, c.y - 15.0);
            ui.painter().circle_filled(bc, 9.0, pal.bg_list);
            ui.painter().circle_filled(bc, 7.5, pal.badge);
            ui.painter().text(
                bc,
                egui::Align2::CENTER_CENTER,
                if unread > 9 { "9+".to_string() } else { unread.to_string() },
                FontId::proportional(9.5),
                Color32::WHITE,
            );
        }
        let tip = if unread > 0 { format!("{name} · {unread} tin chưa đọc") } else { name };
        resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(tip)
    }
}
