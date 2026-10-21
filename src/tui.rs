//! Responsive TUI interface powered by Ratatui & Crossterm.
//!
//! Designed for clean rendering across all Windows PowerShell / CMD fonts:
//! - ASCII-safe borders and single-width tags (no emoji displacement bugs).
//! - Non-blocking event loop (30ms polling) with async background message dispatch.
//! - Clear focus state with auto-focus when typing.
//! - Real-time Dev Log Inspector modal in Dev mode (`--dev`).

use std::io;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
    },
    Frame, Terminal,
};

use crate::chat::{ChatMessage, Thread};
use crate::logger::{self, FindingLevel};
use crate::session::Session;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Focus {
    Threads,
    Input,
}

enum BgEvent {
    ThreadsRefreshed(Vec<Thread>),
    MessageSent { thread_id: String, text: String },
}

pub struct TuiApp<'a> {
    pub session: &'a mut Session,
    pub threads: Vec<Thread>,
    pub list_state: ListState,
    pub selected_thread: usize,
    pub input_text: String,
    pub focus: Focus,
    pub status: String,
    pub show_log_modal: bool,
    pub log_scroll: usize,
    pub should_quit: bool,
    pub store: Option<crate::store::MessageStore>,
    pub poll_state: crate::chat::PollState,
    bg_rx: Receiver<BgEvent>,
    bg_tx: Sender<BgEvent>,
}

impl<'a> TuiApp<'a> {
    pub fn new(session: &'a mut Session, mut threads: Vec<Thread>) -> Self {
        let store = crate::store::MessageStore::open("zalo_history.db").ok();
        if let Some(ref st) = store {
            let _ = st.hydrate_threads(&mut threads, 200);
        }

        let mut list_state = ListState::default();
        if !threads.is_empty() {
            list_state.select(Some(0));
        }

        let (tx, rx) = channel();

        Self {
            session,
            threads,
            list_state,
            selected_thread: 0,
            input_text: String::new(),
            focus: Focus::Threads,
            status: "San sang".into(),
            show_log_modal: false,
            log_scroll: 0,
            should_quit: false,
            store,
            poll_state: crate::chat::PollState::default(),
            bg_rx: rx,
            bg_tx: tx,
        }
    }

    pub fn persist_threads(&mut self, threads: &[Thread]) {
        if let Some(ref mut st) = self.store {
            let _ = crate::store::persist_threads(st, threads);
        } else if let Ok(mut st) = crate::store::MessageStore::open("zalo_history.db") {
            let _ = crate::store::persist_threads(&mut st, threads);
            self.store = Some(st);
        }
    }

    pub fn next_thread(&mut self) {
        if self.threads.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i + 1 < self.threads.len() {
                    i + 1
                } else {
                    0
                }
            }
            None => 0,
        };
        self.selected_thread = i;
        self.list_state.select(Some(i));
    }

    pub fn previous_thread(&mut self) {
        if self.threads.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i > 0 {
                    i - 1
                } else {
                    self.threads.len() - 1
                }
            }
            None => 0,
        };
        self.selected_thread = i;
        self.list_state.select(Some(i));
    }

    pub fn check_background_events(&mut self) {
        while let Ok(event) = self.bg_rx.try_recv() {
            match event {
                BgEvent::ThreadsRefreshed(list) => {
                    if !list.is_empty() {
                        self.threads = list;
                        if self.selected_thread >= self.threads.len() {
                            self.selected_thread = self.threads.len().saturating_sub(1);
                        }
                        self.list_state.select(Some(self.selected_thread));
                    }
                    self.status = format!("Cap nhat {}", crate::ui::clock());
                }
                BgEvent::MessageSent { thread_id, text } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.preview = text.clone();
                    }
                    self.status = "Da gui tin nhan".into();
                }
            }
        }
    }
}

pub async fn run_tui(mut app: TuiApp<'_>) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut last_poll = std::time::Instant::now();
    let poll_interval = Duration::from_secs(4);

    while !app.should_quit {
        app.check_background_events();
        terminal.draw(|f| ui_draw(f, &mut app))?;

        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if app.show_log_modal {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('l') => {
                                app.show_log_modal = false;
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.log_scroll = app.log_scroll.saturating_sub(1);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                app.log_scroll = app.log_scroll.saturating_add(1);
                            }
                            _ => {}
                        }
                    } else {
                        match (key.modifiers, key.code) {
                            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                                app.should_quit = true;
                            }
                            (_, KeyCode::Char('q')) if app.focus != Focus::Input => {
                                app.should_quit = true;
                            }
                            (_, KeyCode::Char('x')) if app.focus != Focus::Input => {
                                let _ = std::fs::remove_file("session.json");
                                let _ = std::fs::remove_file("threads_cache.json");
                                app.session.logout();
                                app.threads.clear();
                                app.status = "Da dang xuat va xoa session.json, threads_cache.json. Bam 'q' de thoat.".into();
                            }
                            (_, KeyCode::Tab) => {
                                app.focus = match app.focus {
                                    Focus::Threads => Focus::Input,
                                    Focus::Input => Focus::Threads,
                                };
                            }
                            (_, KeyCode::Esc) => {
                                app.focus = Focus::Threads;
                            }
                            (_, KeyCode::Char('l')) if app.focus != Focus::Input => {
                                if logger::is_dev_mode() {
                                    app.show_log_modal = true;
                                    app.log_scroll = 0;
                                }
                            }
                            (_, KeyCode::Char('r')) if app.focus != Focus::Input => {
                                app.status = "Dang lam moi…".into();
                                match crate::chat::get_friends(app.session).await {
                                    Ok(mut list) => {
                                        let ids: Vec<String> = list.iter().map(|t| t.id.clone()).collect();
                                        match crate::chat::get_last_msgs(app.session, &ids).await {
                                            Ok(resp) => {
                                                crate::chat::merge_previews(&mut list, &resp);
                                                app.persist_threads(&list);
                                            }
                                            Err(e) => {
                                                crate::logger::log_error("tui::refresh::get_last_msgs", &e.to_string());
                                            }
                                        }
                                        let _ = app.bg_tx.send(BgEvent::ThreadsRefreshed(list));
                                    }
                                    Err(e) => {
                                        crate::logger::log_error("tui::refresh::get_friends", &e.to_string());
                                        app.status = format!("Lỗi tải bạn bè: {e}");
                                    }
                                }
                            }
                            (_, KeyCode::Char('h')) if app.focus != Focus::Input => {
                                if let Some(t) = app.threads.get(app.selected_thread) {
                                    if t.is_group {
                                        let gid = t.id.clone();
                                        app.status = "Dang tai lich su nhom…".into();
                                        match crate::chat::get_group_history(app.session, &gid, 50).await {
                                            Ok(resp) => {
                                                let msg_count = if let Some(target) = app.threads.get_mut(app.selected_thread) {
                                                    crate::chat::merge_group_history(target, &resp);
                                                    target.messages.len()
                                                } else {
                                                    0
                                                };
                                                if let Some(target) = app.threads.get(app.selected_thread) {
                                                    let updated = vec![target.clone()];
                                                    app.persist_threads(&updated);
                                                }
                                                app.status = format!("Da tai lich su nhom ({msg_count} tin)");
                                            }
                                            Err(e) => {
                                                crate::logger::log_error("tui::get_group_history", &e.to_string());
                                                app.status = format!("Loi tai lich su nhom: {e}");
                                            }
                                        }
                                    } else {
                                        app.status = "Chi ap dung cho nhom chat".into();
                                    }
                                }
                            }
                            // Navigation when focusing threads
                            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.focus != Focus::Input => {
                                app.previous_thread();
                            }
                            (_, KeyCode::Down) | (_, KeyCode::Char('j')) if app.focus != Focus::Input => {
                                app.next_thread();
                            }
                            (_, KeyCode::Enter) if app.focus != Focus::Input => {
                                app.focus = Focus::Input;
                            }
                            // Typing when focusing input
                            (_, KeyCode::Enter) if app.focus == Focus::Input => {
                                if !app.input_text.trim().is_empty() && !app.threads.is_empty() {
                                    let to_id = app.threads[app.selected_thread].id.clone();
                                    let text = app.input_text.trim().to_string();
                                    app.input_text.clear();

                                    let now = crate::ui::clock();
                                    app.threads[app.selected_thread].messages.push(ChatMessage {
                                        id: format!("local_{}", crate::login::unix_now()),
                                        sender: "Toi".into(),
                                        text: text.clone(),
                                        time: now.clone(),
                                        is_outgoing: true,
                                        ts: (crate::login::unix_now() * 1000) as i64,
                                        sticker_id: None,
                                        sticker_cate_id: None,
                                        sticker_type: None,
                                    });
                                    app.threads[app.selected_thread].preview = text.clone();
                                    app.threads[app.selected_thread].time = now;
                                    let snapshot = vec![app.threads[app.selected_thread].clone()];
                                    app.persist_threads(&snapshot);
                                    crate::chat::save_threads_cache(&app.threads);

                                    let is_group = app.threads[app.selected_thread].is_group;
                                    let res = if is_group {
                                        crate::chat::send_group_message(app.session, &to_id, &text).await
                                    } else {
                                        crate::chat::send_message(app.session, &to_id, &text).await
                                    };
                                    if let Err(e) = res {
                                        crate::logger::log_error("tui::send_message", &e.to_string());
                                    }
                                    let _ = app.bg_tx.send(BgEvent::MessageSent { thread_id: to_id, text });
                                }
                            }
                            (_, KeyCode::Backspace) if app.focus == Focus::Input => {
                                app.input_text.pop();
                            }
                            // Auto-focus input when user types any character even if in Threads focus
                            (_, KeyCode::Char(c)) => {
                                if app.focus != Focus::Input {
                                    app.focus = Focus::Input;
                                }
                                app.input_text.push(c);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Automatic non-blocking polling check (`getChatFromPolling` + `get-last-msgs`, no `getfriends` 429)
        if last_poll.elapsed() >= poll_interval {
            last_poll = std::time::Instant::now();
            if app.session.login.is_some() {
                if let Err(e) = crate::chat::poll_new_messages(
                    app.session,
                    &mut app.poll_state,
                    &mut app.threads,
                )
                .await
                {
                    crate::logger::log_error("tui::poll_new_messages", &e.to_string());
                } else {
                    app.status = format!("Cap nhat {}", crate::ui::clock());
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn ui_draw(f: &mut Frame, app: &mut TuiApp) {
    let size = f.area();

    // Layout: Header (3), Main Body (flex), Footer (3)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(size);

    // 1. Header Bar
    let user_name = app
        .session
        .login
        .as_ref()
        .map(|l| {
            if l.display_name.is_empty() {
                format!("UIN: {}", l.uin)
            } else {
                format!("{} (UIN: {})", l.display_name, l.uin)
            }
        })
        .unwrap_or_else(|| "Chua dang nhap".into());

    let mode_span = if logger::is_dev_mode() {
        Span::styled(" [DEV MODE] ", Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" [PRODUCTION] ", Style::default().fg(Color::White).bg(Color::Green))
    };

    let header_line = Line::from(vec![
        Span::styled(" Zalo Rust Client ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("| Tai khoan: "),
        Span::styled(user_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw(" | Trang thai: "),
        Span::styled(&app.status, Style::default().fg(Color::Yellow)),
        Span::raw(" "),
        mode_span,
    ]);

    let header = Paragraph::new(header_line)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .alignment(Alignment::Left);
    f.render_widget(header, chunks[0]);

    // 2. Main Body: Split 32% Contacts | 68% Chat Area
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .split(chunks[1]);

    // 2a. Contacts / Threads List (clean ASCII layout)
    let thread_items: Vec<ListItem> = app
        .threads
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let is_sel = i == app.selected_thread;
            let tag = if t.name.contains("Cloud") {
                "[Cloud] "
            } else if t.is_group {
                "[Group] "
            } else {
                "[User]  "
            };
            let cursor = if is_sel { "> " } else { "  " };

            let name_style = if is_sel {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let preview_text = if t.preview.is_empty() {
                "- chua co tin nhan -".to_string()
            } else {
                let p = t.preview.chars().take(22).collect::<String>();
                format!("{p}...")
            };

            let line1 = Line::from(vec![
                Span::raw(cursor),
                Span::styled(tag, Style::default().fg(Color::DarkGray)),
                Span::styled(&t.name, name_style),
                Span::raw("  "),
                Span::styled(&t.time, Style::default().fg(Color::DarkGray)),
            ]);
            let line2 = Line::from(vec![
                Span::raw("    "),
                Span::styled(preview_text, Style::default().fg(Color::Gray)),
            ]);

            ListItem::new(vec![line1, line2])
        })
        .collect();

    let threads_title = if app.focus == Focus::Threads {
        format!(" Danh sach hoi thoai ({}) [DANG CHON] ", app.threads.len())
    } else {
        format!(" Danh sach hoi thoai ({}) [Bam Tab de chuyen] ", app.threads.len())
    };

    let threads_border_style = if app.focus == Focus::Threads {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let threads_list = List::new(thread_items)
        .block(
            Block::default()
                .title(threads_title)
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(threads_border_style),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(25, 40, 65))
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(threads_list, body_chunks[0], &mut app.list_state);

    // 2b. Right Column: Contact Header + Message Bubbles + Input Area
    let chat_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(3),
        ])
        .split(body_chunks[1]);

    let active_thread = app.threads.get(app.selected_thread);

    // Header of current active conversation
    let contact_name = active_thread.map(|t| t.name.as_str()).unwrap_or("Chua chon hoi thoai");
    let contact_id = active_thread.map(|t| t.id.as_str()).unwrap_or("");
    let chat_header = Paragraph::new(Line::from(vec![
        Span::styled(format!(" {contact_name} "), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("| ID: "),
        Span::styled(contact_id, Style::default().fg(Color::DarkGray)),
        Span::raw(" | Ket noi truc tiep tu server Zalo"),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(chat_header, chat_chunks[0]);

    // Messages Area
    let mut message_lines: Vec<Line> = Vec::new();
    if let Some(t) = active_thread {
        if t.messages.is_empty() {
            message_lines.push(Line::raw(""));
            message_lines.push(Line::from(vec![Span::styled(
                format!("  [Thong tin] Hoi thoai '{}' da san sang.", t.name),
                Style::default().fg(Color::DarkGray),
            )]));
            message_lines.push(Line::from(vec![Span::styled(
                "  Nhap tin nhan vao o ben duoi roi bam [Enter] de gui tin len Zalo.",
                Style::default().fg(Color::Cyan),
            )]));
        } else {
            for m in &t.messages {
                let time_tag = format!(" [{}] ", m.time);
                if m.is_outgoing {
                    message_lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled("[Toi]: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::styled(&m.text, Style::default().fg(Color::White)),
                        Span::styled(time_tag, Style::default().fg(Color::DarkGray)),
                    ]));
                } else {
                    message_lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(format!("[{}]: ", t.name), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                        Span::styled(&m.text, Style::default().fg(Color::White)),
                        Span::styled(time_tag, Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
        }
    }

    let chat_body = Paragraph::new(message_lines)
        .block(
            Block::default()
                .title(" Noi dung tin nhan ")
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(chat_body, chat_chunks[1]);

    // Input Box
    let input_title = if app.focus == Focus::Input {
        " Soan tin nhan: [DANG GO] (Enter: Gui | Esc: Chon danh sach) "
    } else {
        " Soan tin nhan: [Go ky tu bat ky hoac bam Tab de nhap] "
    };

    let input_border_style = if app.focus == Focus::Input {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let input_widget = Paragraph::new(format!(" {}", app.input_text))
        .block(
            Block::default()
                .title(input_title)
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(input_border_style),
        );
    f.render_widget(input_widget, chat_chunks[2]);

    // 3. Footer Shortcuts Bar
    let dev_hint = if logger::is_dev_mode() {
        " | [l] Quet log (Dev)"
    } else {
        ""
    };

    let footer_text = format!(
        " [^/v] Chon | [Tab] Focus | [Enter] Gui | [r] Lam moi | [x] Dang xuat | [h] Nhom{dev_hint} | [q] Thoat "
    );
    let footer = Paragraph::new(footer_text)
        .block(Block::default().borders(Borders::ALL).border_type(BorderType::Plain))
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(footer, chunks[2]);

    // 4. Dev Log Modal (if triggered in Dev mode)
    if app.show_log_modal && logger::is_dev_mode() {
        render_log_modal(f, size, app.log_scroll);
    }
}

fn render_log_modal(f: &mut Frame, area: Rect, scroll: usize) {
    let modal_area = centered_rect(88, 80, area);
    f.render_widget(Clear, modal_area);

    let findings = logger::scan_logs();
    let entries = logger::get_entries();

    let mut content = Vec::new();

    // Findings section
    content.push(Line::from(vec![
        Span::styled("=== KET QUA QUET LOG & CHAN DOAN GIAO THUC ===", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    ]));
    for finding in &findings {
        let (icon, color) = match finding.level {
            FindingLevel::Info => ("[INFO] ", Color::Green),
            FindingLevel::Warning => ("[WARN] ", Color::Yellow),
            FindingLevel::Critical => ("[CRIT] ", Color::Red),
        };
        content.push(Line::from(vec![
            Span::styled(icon, Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Span::styled(&finding.title, Style::default().fg(color).add_modifier(Modifier::BOLD)),
        ]));
        content.push(Line::from(vec![
            Span::raw("  Chi tiet: "),
            Span::styled(&finding.detail, Style::default().fg(Color::White)),
        ]));
        content.push(Line::from(vec![
            Span::raw("  Goi y: "),
            Span::styled(&finding.suggestion, Style::default().fg(Color::Cyan)),
        ]));
        content.push(Line::raw(""));
    }

    // Traffic stream section
    content.push(Line::from(vec![
        Span::styled(format!("=== LUU LUONG MANG THOI GIAN THUC ({} requests) ===", entries.len()), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ]));
    for e in entries.iter().rev() {
        let status_color = if e.is_error { Color::Red } else { Color::Green };
        let status_str = e.status.map(|s| s.to_string()).unwrap_or_else(|| "ERR".into());

        content.push(Line::from(vec![
            Span::styled(format!("[{}] ", e.timestamp), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<4} ", e.method), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:<3} ", status_str), Style::default().fg(status_color)),
            Span::styled(format!("({}ms) ", e.latency_ms), Style::default().fg(Color::DarkGray)),
            Span::styled(&e.url, Style::default().fg(Color::White)),
        ]));
        if let Some(dec) = &e.decrypted_preview {
            let preview = dec.chars().take(80).collect::<String>();
            content.push(Line::from(vec![
                Span::raw("  + Decrypted: "),
                Span::styled(format!("{preview}..."), Style::default().fg(Color::Gray)),
            ]));
        }
        if let Some(err) = &e.error_msg {
            content.push(Line::from(vec![
                Span::styled(format!("  + Loi: {err}"), Style::default().fg(Color::Red)),
            ]));
        }
    }

    let block = Block::default()
        .title(" Dev Inspector & Protocol Scanner (Esc/q de dong, Up/Down de cuon) ")
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(Color::Yellow));

    let p = Paragraph::new(content)
        .block(block)
        .scroll((scroll as u16, 0))
        .wrap(Wrap { trim: true });

    f.render_widget(p, modal_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
