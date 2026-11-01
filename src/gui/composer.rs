use super::*;

/// Render an explicit member picker; duplicate display names remain distinct selectable UIDs.
pub(super) fn member_picker(
    ui: &mut egui::Ui,
    members: &[crate::chat::SidebarMember],
    text: &mut String,
    mentions: &mut crate::composer_mentions::ComposerMentions,
    selection: std::ops::Range<usize>,
) -> Option<usize> {
    let mut cursor = None;
    ui.menu_button("@", |ui| {
        if members.is_empty() {
            ui.label("Chưa có danh sách thành viên đã tải.");
        }
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                for member in members.iter().filter(|member| !member.id.trim().is_empty()) {
                    if ui
                        .button(format!("{} · {}", member.name, member.id))
                        .clicked()
                    {
                        cursor = mentions.select_member(text, member, selection.clone());
                        ui.close_menu();
                    }
                }
            });
    })
    .response
    .on_hover_text("Chọn thành viên để nhắc tên");
    cursor
}

/// Construct exactly the command used by the composer send action without rewriting raw mention text.
///
/// # Errors
/// Returns an error for unsupported direct mentions or invalid selected spans.
pub(super) fn send_command(
    thread_id: String,
    text: String,
    is_group: bool,
    quoted: Option<ChatMessage>,
    mentions: Vec<crate::composer_mentions::OutgoingMention>,
) -> anyhow::Result<WorkerCmd> {
    crate::composer_mentions::validate_mentions(&text, &mentions)?;
    anyhow::ensure!(
        is_group || mentions.is_empty(),
        "Nhắc tên chỉ được xác minh cho nhóm"
    );
    Ok(if !mentions.is_empty() {
        WorkerCmd::SendWithMentions {
            to_id: thread_id,
            text,
            mentions,
            quoted,
        }
    } else if let Some(quoted) = quoted {
        WorkerCmd::SendQuote {
            to_id: thread_id,
            text,
            quoted,
            is_group,
        }
    } else {
        WorkerCmd::SendMessage {
            to_id: thread_id,
            text,
            is_group,
        }
    })
}

#[cfg(test)]
mod composer_tests {
    use super::*;

    #[test]
    fn composer_member_pointer_selection_emits_raw_text_quote_and_exact_uid_command() {
        let ctx = egui::Context::default();
        let members: Vec<_> = ["11", "22"]
            .into_iter()
            .map(|id| crate::chat::SidebarMember {
                id: id.into(),
                name: "An".into(),
                avatar: None,
            })
            .collect();
        let mut model = crate::composer_mentions::ComposerMentions::default();
        let mut text = "😀 @".to_string();
        let mut menu = Pos2::ZERO;
        let mut member = Pos2::ZERO;
        let mut chosen = false;
        for frame in 0..7 {
            let target = if frame <= 3 { menu } else { member };
            let mut events = vec![egui::Event::PointerMoved(target)];
            if matches!(frame, 1 | 2 | 5 | 6) {
                events.push(egui::Event::PointerButton {
                    pos: target,
                    button: egui::PointerButton::Primary,
                    pressed: frame == 1 || frame == 5,
                    modifiers: Default::default(),
                });
            }
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 350.0))),
                    events,
                    time: Some(frame as f64 * 0.02),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let end = text.chars().count();
                        chosen |=
                            member_picker(ui, &members, &mut text, &mut model, end..end).is_some();
                    });
                },
            );
            for shape in &output.shapes {
                if let egui::Shape::Text(shape) = &shape.shape {
                    let center = shape.pos + shape.galley.size() * 0.5;
                    if shape.galley.job.text == "@" {
                        menu = center;
                    }
                    if shape.galley.job.text == "An · 22" {
                        member = center;
                    }
                }
            }
        }
        assert!(
            chosen,
            "Actual member picker must receive the pointer selection"
        );
        let mentions = model.outgoing(&text);
        let quoted = ChatMessage {
            id: "original-1".into(),
            text: "Raw :+1:".into(),
            ..Default::default()
        };
        let command =
            send_command("group-1".into(), text.clone(), true, Some(quoted), mentions).unwrap();
        let WorkerCmd::SendWithMentions {
            to_id,
            text: sent,
            mentions,
            quoted,
        } = command
        else {
            panic!("Mention selections must reach the dedicated worker request")
        };
        assert_eq!(to_id, "group-1");
        assert_eq!(sent, "😀 @An ");
        assert_eq!(sent, text);
        assert_eq!(mentions[0].uid, "22");
        assert_eq!(mentions[0].pos, 3);
        assert_eq!(quoted.unwrap().text, "Raw :+1:");
    }
}
