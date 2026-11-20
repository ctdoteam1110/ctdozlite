//! Group poll card: question, options with counts and voters, and voting.
//!
//! The card is drawn under the newest message of a poll in the conversation, as Zalo Web
//! does (`re-notes/44_group_poll.md`). Network calls go through `WorkerCmd::FetchPoll` and
//! `WorkerCmd::VotePoll`.

use super::*;
use crate::poll::{PollDetail, VoteBlock};

/// Per-poll UI state, keyed by poll id in `ZaloGuiApp::polls`.
#[derive(Debug, Default)]
pub(crate) struct PollCardState {
    pub(crate) detail: Option<PollDetail>,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    /// Selection being edited; `None` shows the user's current votes.
    pub(crate) draft: Option<Vec<i64>>,
    pub(crate) voting: bool,
    /// Newest poll event timestamp the loaded detail reflects. A newer vote or create event
    /// in the conversation triggers a refetch, which keeps counts live.
    pub(crate) seen_event_ts: i64,
}

impl PollCardState {
    /// The selection the card shows: the draft, else the user's votes.
    pub(crate) fn shown_selection(&self, my_uid: &str) -> Vec<i64> {
        match (&self.draft, &self.detail) {
            (Some(d), _) => d.clone(),
            (None, Some(detail)) => detail.my_choices(my_uid),
            (None, None) => Vec::new(),
        }
    }

    /// Whether the card needs a (re)fetch for a conversation whose newest event of this
    /// poll is at `latest_event_ts`.
    pub(crate) fn needs_fetch(&self, latest_event_ts: i64) -> bool {
        if self.loading || self.voting {
            return false;
        }
        if self.detail.is_none() && self.error.is_none() {
            return true;
        }
        // After an error, only a newer poll event retries on its own; the card offers a
        // manual retry otherwise.
        latest_event_ts > self.seen_event_ts
    }
}

/// What the user did on a card this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PollCardAction {
    Fetch,
    Toggle(i64),
    Submit(Vec<i64>),
    CancelDraft,
}

impl ZaloGuiApp {
    /// Apply one frame's poll card fetches and clicks: update state, then send the worker
    /// commands. `fetches` carries the newest event timestamp each fetch will reflect.
    pub(super) fn apply_poll_ui(
        &mut self,
        fetches: Vec<(String, i64)>,
        actions: Vec<(String, PollCardAction)>,
    ) {
        let send = |cmd: WorkerCmd| {
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(cmd);
            }
        };
        for (poll_id, ts) in fetches {
            let st = self.polls.entry(poll_id.clone()).or_default();
            st.loading = true;
            st.seen_event_ts = st.seen_event_ts.max(ts);
            send(WorkerCmd::FetchPoll { poll_id });
        }
        for (poll_id, action) in actions {
            let st = self.polls.entry(poll_id.clone()).or_default();
            match action {
                PollCardAction::Fetch => {
                    st.loading = true;
                    st.error = None;
                    send(WorkerCmd::FetchPoll { poll_id });
                }
                PollCardAction::Toggle(id) => {
                    let Some(detail) = st.detail.as_ref() else {
                        continue;
                    };
                    let multi = detail.allow_multi_choices;
                    let current = detail.my_choices(&self.user_uin);
                    let draft = st.draft.get_or_insert(current);
                    crate::poll::toggle_choice(draft, id, multi);
                }
                PollCardAction::Submit(option_ids) => {
                    st.voting = true;
                    send(WorkerCmd::VotePoll {
                        poll_id,
                        option_ids,
                    });
                }
                PollCardAction::CancelDraft => st.draft = None,
            }
        }
    }
}

const CARD_MAX_W: f32 = 400.0;
const ROW_H: f32 = 34.0;
const VOTER_PREVIEW: usize = 3;

fn voter_line(uids: &[String], names: &HashMap<String, String>, my_uid: &str) -> String {
    let label = |uid: &String| -> String {
        if !my_uid.is_empty() && uid == my_uid {
            "Bạn".to_string()
        } else {
            names
                .get(uid)
                .cloned()
                .unwrap_or_else(|| "Thành viên".into())
        }
    };
    let shown: Vec<String> = uids.iter().take(VOTER_PREVIEW).map(label).collect();
    let rest = uids.len().saturating_sub(VOTER_PREVIEW);
    if rest > 0 {
        format!("{} và {rest} người khác", shown.join(", "))
    } else {
        shown.join(", ")
    }
}

fn format_deadline(ms: i64) -> String {
    crate::clock::local_datetime(ms)
        .map(|t| t.format("%H:%M %d/%m/%Y").to_string())
        .unwrap_or_default()
}

/// Draw one poll card. `question_hint` comes from the event and is shown until the detail
/// arrives.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_poll_card(
    ui: &mut egui::Ui,
    question_hint: &str,
    state: Option<&PollCardState>,
    my_uid: &str,
    names: &HashMap<String, String>,
    pal: &ThemePalette,
    now_ms: i64,
) -> Option<PollCardAction> {
    let mut action = None;
    let avail_w = ui.available_width();
    let card_w = (avail_w - 24.0).clamp(160.0, CARD_MAX_W);
    ui.horizontal(|ui| {
        ui.add_space(((avail_w - card_w) * 0.5).max(0.0));
        egui::Frame::none()
            .fill(pal.bg_elevated)
            .stroke(Stroke::new(1.0_f32, pal.border))
            .rounding(Rounding::same(12.0))
            .inner_margin(Margin::same(12.0))
            .show(ui, |ui| {
                let inner_w = card_w - 24.0;
                ui.set_width(inner_w);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("BÌNH CHỌN")
                            .size(10.5)
                            .strong()
                            .color(pal.text_muted),
                    );
                    let detail = state.and_then(|s| s.detail.as_ref());
                    let question = detail
                        .map(|d| d.question.as_str())
                        .filter(|q| !q.trim().is_empty())
                        .unwrap_or(question_hint);
                    ui.add(
                        egui::Label::new(
                            RichText::new(question.trim())
                                .size(14.5)
                                .strong()
                                .color(pal.text_primary),
                        )
                        .wrap(),
                    );
                    match (state, detail) {
                        (_, Some(d)) => {
                            let st = state.expect("detail implies state");
                            if let Some(a) =
                                render_poll_body(ui, d, st, my_uid, names, pal, now_ms, inner_w)
                            {
                                action = Some(a);
                            }
                        }
                        (Some(st), None) if st.loading => {
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(
                                    RichText::new("Đang tải bình chọn…")
                                        .size(12.5)
                                        .color(pal.text_secondary),
                                );
                            });
                        }
                        (Some(st), None) if st.error.is_some() => {
                            ui.add_space(6.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!(
                                        "Có lỗi xảy ra khi lấy thông tin bình chọn ({})",
                                        st.error.as_deref().unwrap_or_default()
                                    ))
                                    .size(12.5)
                                    .color(pal.danger_text),
                                )
                                .wrap(),
                            );
                            if ui
                                .button("Thử lại")
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                            {
                                action = Some(PollCardAction::Fetch);
                            }
                        }
                        _ => {
                            ui.add_space(6.0);
                            ui.spinner();
                        }
                    }
                });
            });
    });
    action
}

#[allow(clippy::too_many_arguments)]
fn render_poll_body(
    ui: &mut egui::Ui,
    d: &PollDetail,
    st: &PollCardState,
    my_uid: &str,
    names: &HashMap<String, String>,
    pal: &ThemePalette,
    now_ms: i64,
    inner_w: f32,
) -> Option<PollCardAction> {
    let mut action = None;
    let mut flags: Vec<String> = Vec::new();
    if d.allow_multi_choices {
        flags.push("Chọn nhiều phương án".into());
    }
    if d.is_anonymous {
        flags.push("Bình chọn ẩn danh".into());
    }
    if d.expired_time > 0 {
        flags.push(format!("Kết thúc lúc {}", format_deadline(d.expired_time)));
    }
    if !flags.is_empty() {
        ui.label(
            RichText::new(flags.join(" · "))
                .size(11.5)
                .color(pal.text_muted),
        );
    }
    ui.add_space(8.0);

    let block = d.vote_block(my_uid, now_ms);
    let hidden = d.results_hidden(my_uid);
    let selection = st.shown_selection(my_uid);
    let max_votes = d.options.iter().map(|o| o.votes).max().unwrap_or(0).max(1);
    let can_click = block.is_none() && !st.voting;

    for opt in &d.options {
        let selected = selection.contains(&opt.id);
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(inner_w, ROW_H),
            if can_click {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        let painter = ui.painter_at(rect);
        let rounding = Rounding::same(8.0);
        painter.rect_filled(rect, rounding, pal.bg_input_track);
        if !hidden && opt.votes > 0 {
            let frac = opt.votes as f32 / max_votes as f32;
            let mut fill = rect;
            fill.set_width((rect.width() * frac).max(8.0));
            painter.rect_filled(fill, rounding, pal.primary.gamma_multiply(0.22));
        }
        if selected || (resp.hovered() && can_click) {
            painter.rect_stroke(rect, rounding, Stroke::new(1.0_f32, pal.primary));
        }
        // Selector: a circle for single choice, a rounded square for multi choice.
        let mark_c = Pos2::new(rect.left() + 16.0, rect.center().y);
        let mark_r = 7.0;
        let mark_rect = Rect::from_center_size(mark_c, Vec2::splat(mark_r * 2.0));
        let mark_round = if d.allow_multi_choices {
            Rounding::same(3.0)
        } else {
            Rounding::same(mark_r)
        };
        if selected {
            painter.rect_filled(mark_rect, mark_round, pal.primary);
            draw_lucide_icon(&painter, mark_c, lucide::CHECK, 10.0, pal.primary_fg);
        } else {
            painter.rect_stroke(mark_rect, mark_round, Stroke::new(1.4_f32, pal.text_muted));
        }
        let count_text = if hidden {
            String::new()
        } else {
            opt.votes.to_string()
        };
        let count_w = if count_text.is_empty() { 0.0 } else { 34.0 };
        let text_left = rect.left() + 32.0;
        let text_w = (rect.right() - 10.0 - count_w - text_left).max(20.0);
        let galley = ui.fonts(|f| {
            f.layout(
                opt.content.clone(),
                FontId::proportional(13.0),
                pal.text_primary,
                text_w,
            )
        });
        let text_pos = Pos2::new(
            text_left,
            rect.center().y - galley.size().y.min(ROW_H) * 0.5,
        );
        painter.galley(text_pos, galley, pal.text_primary);
        if !count_text.is_empty() {
            painter.text(
                Pos2::new(rect.right() - 10.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                count_text,
                FontId::proportional(12.5),
                pal.text_secondary,
            );
        }
        let resp = if can_click {
            resp.on_hover_cursor(CursorIcon::PointingHand)
        } else {
            resp
        };
        if resp.clicked() {
            action = Some(PollCardAction::Toggle(opt.id));
        }
        if !hidden && !d.is_anonymous && !opt.voters.is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(voter_line(&opt.voters, names, my_uid))
                        .size(11.0)
                        .color(pal.text_muted),
                )
                .wrap(),
            );
        }
        ui.add_space(5.0);
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let voters = d.voter_count();
        let summary = if hidden {
            "Bình chọn để xem kết quả".to_string()
        } else if voters == 0 {
            "Chưa có ai bình chọn".to_string()
        } else {
            let total = d.vote_total();
            if d.allow_multi_choices && total != voters {
                format!("{voters} người bình chọn, {total} lượt bình chọn")
            } else {
                format!("{voters} người bình chọn")
            }
        };
        ui.label(RichText::new(summary).size(11.5).color(pal.text_secondary));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if st.voting {
                ui.spinner();
                return;
            }
            if let Some(b) = block {
                let icon = if b == VoteBlock::Closed {
                    lucide::LOCK
                } else {
                    lucide::CLOCK
                };
                ui.label(
                    RichText::new(format!("{icon} {}", b.label()))
                        .size(11.5)
                        .color(pal.text_muted),
                );
                return;
            }
            let current = d.my_choices(my_uid);
            let changed = crate::poll::selection_changed(&selection, &current);
            let label = if current.is_empty() {
                "Bình chọn"
            } else {
                "Đổi lựa chọn"
            };
            let btn =
                egui::Button::new(RichText::new(label).size(12.5).strong().color(if changed {
                    pal.primary_fg
                } else {
                    pal.text_muted
                }))
                .fill(if changed {
                    pal.primary
                } else {
                    pal.bg_input_track
                })
                .rounding(Rounding::same(8.0));
            if ui
                .add_enabled(changed, btn)
                .on_hover_cursor(CursorIcon::PointingHand)
                .clicked()
            {
                action = Some(PollCardAction::Submit(selection.clone()));
            }
            if st.draft.is_some() && changed && ui.small_button("Hủy").clicked() {
                action = Some(PollCardAction::CancelDraft);
            }
        });
    });
    if d.is_hide_vote_preview && block.is_none() {
        ui.label(
            RichText::new("Bạn không thể thay đổi lựa chọn sau khi bình chọn")
                .size(11.0)
                .color(pal.text_muted),
        );
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poll::PollOption;

    fn detail() -> PollDetail {
        PollDetail {
            poll_id: "1001".into(),
            question: "Ăn trưa ở đâu?".into(),
            options: vec![
                PollOption {
                    id: 11,
                    votes: 1,
                    voters: vec!["user-2".into()],
                    ..Default::default()
                },
                PollOption {
                    id: 12,
                    votes: 1,
                    voters: vec!["user-me".into()],
                    ..Default::default()
                },
            ],
            joined: true,
            ..Default::default()
        }
    }

    #[test]
    fn needs_fetch_when_no_detail_and_no_error() {
        let st = PollCardState::default();
        assert!(st.needs_fetch(0));
        let st = PollCardState {
            error: Some("x".into()),
            ..Default::default()
        };
        assert!(!st.needs_fetch(0));
    }

    #[test]
    fn needs_fetch_when_a_newer_poll_event_arrives() {
        let st = PollCardState {
            detail: Some(detail()),
            seen_event_ts: 100,
            ..Default::default()
        };
        assert!(!st.needs_fetch(100));
        assert!(st.needs_fetch(101));
    }

    #[test]
    fn needs_fetch_is_false_while_loading_or_voting() {
        let st = PollCardState {
            loading: true,
            ..Default::default()
        };
        assert!(!st.needs_fetch(1));
        let st = PollCardState {
            detail: Some(detail()),
            voting: true,
            ..Default::default()
        };
        assert!(!st.needs_fetch(1));
    }

    #[test]
    fn shown_selection_prefers_draft_over_current_votes() {
        let mut st = PollCardState {
            detail: Some(detail()),
            ..Default::default()
        };
        assert_eq!(st.shown_selection("user-me"), vec![12]);
        st.draft = Some(vec![11]);
        assert_eq!(st.shown_selection("user-me"), vec![11]);
    }

    #[test]
    fn voter_line_names_me_and_summarises_the_rest() {
        let names: HashMap<String, String> = [("user-2".to_string(), "Lan".to_string())]
            .into_iter()
            .collect();
        let uids: Vec<String> = ["user-me", "user-2", "user-3", "user-4", "user-5"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            voter_line(&uids, &names, "user-me"),
            "Bạn, Lan, Thành viên và 2 người khác"
        );
    }
}
