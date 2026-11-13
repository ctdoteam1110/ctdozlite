//! Responsive TUI interface powered by Ratatui & Crossterm.
//!
//! Design goals:
//! - Non-blocking: every network call runs in a `tokio::spawn` worker and reports
//!   back over an mpsc channel, so the 30 ms render loop never stalls.
//! - ASCII-safe chrome (single-width borders and tags) for Windows consoles while
//!   keeping original UTF-8 message content.
//! - GUI parity: search/filter/unread, scrollback, quote, reactions, delete,
//!   attachments, sticker store, group history and the dev log inspector.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use serde_json::Value;

use crate::chat::{self, ChatMessage, StickerItem, StickerPackInfo, Thread};
use crate::logger::{self, FindingLevel};
use crate::session::Session;

const POLL_SECS: u64 = 4;

/// Quick-reaction palette for keys `1`..`6`, same order as the GUI picker.
const QUICK_REACTIONS: [&str; 6] = [
    "\u{1F44D}",
    "\u{2764}\u{FE0F}",
    "\u{1F606}",
    "\u{1F62E}",
    "\u{1F622}",
    "\u{1F620}",
];

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Focus {
    Threads,
    Messages,
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterTab {
    All,
    Direct,
    Group,
}

impl FilterTab {
    fn next(self) -> Self {
        match self {
            Self::All => Self::Direct,
            Self::Direct => Self::Group,
            Self::Group => Self::All,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "Tất cả",
            Self::Direct => "Cá nhân",
            Self::Group => "Nhóm",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overlay {
    None,
    Help,
    Log,
    Sticker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptKind {
    Attachment,
}

struct PromptState {
    kind: PromptKind,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmKind {
    Logout,
    DeleteMessage,
    /// Drop local message history (`zalo_history.db` + `threads_cache.json`) while
    /// keeping the session (#21).
    WipeLocalData,
}

struct ConfirmAction {
    question: String,
    detail: String,
    kind: ConfirmKind,
    thread_id: String,
    msg_id: String,
}

/// Sidebar width mode (lazygit-style flexible panels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SidebarMode {
    #[default]
    Normal,
    Wide,
    Hidden,
}

impl SidebarMode {
    fn cycle(self) -> Self {
        match self {
            Self::Normal => Self::Wide,
            Self::Wide => Self::Hidden,
            Self::Hidden => Self::Normal,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Normal => "32%",
            Self::Wide => "50%",
            Self::Hidden => "ẩn",
        }
    }
}

#[derive(Default)]
struct StickerState {
    packs: Vec<StickerPackInfo>,
    items: Vec<StickerItem>,
    pack_idx: usize,
    item_idx: usize,
    stage: u8,
    loading: bool,
    loaded_name: String,
}

enum BgEvent {
    PollDone {
        threads: Vec<Thread>,
        state: chat::PollState,
    },
    PollFailed(String),
    ThreadsRefreshed(Vec<Thread>),
    HistoryLoaded {
        thread_id: String,
        resp: Value,
    },
    MessageSent {
        thread_id: String,
        text: String,
    },
    AttachmentSent {
        thread_id: String,
        msg: ChatMessage,
    },
    StickerSent {
        thread_id: String,
    },
    ReactionDone {
        thread_id: String,
        msg_id: String,
        icon: String,
    },
    DeleteDone {
        thread_id: String,
        msg_id: String,
    },
    StickerPacks(Vec<StickerPackInfo>),
    StickerItems(Vec<StickerItem>),
    ConsoleLine {
        thread_id: String,
        text: String,
    },
    UpdateAvailable(crate::gui::UpdateCheckResult),
    SendFailed(String),
    ActionFailed(String),
}

/// Braille spinner frames (omp-style busy indicator).
pub const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// ASCII splash logo shown at startup (~1.2s).
/// Truecolor palette (matches GUI `theme.rs` dark mode).
pub mod theme {
    use ratatui::style::Color;
    pub const BG: Color = Color::Rgb(29, 32, 37);
    pub const PANEL: Color = Color::Rgb(34, 38, 43);
    pub const BORDER: Color = Color::Rgb(55, 61, 69);
    pub const ACCENT: Color = Color::Rgb(51, 132, 255);
    pub const ACCENT_DIM: Color = Color::Rgb(31, 52, 77);
    pub const TEXT: Color = Color::Rgb(230, 233, 238);
    pub const MUTED: Color = Color::Rgb(139, 150, 167);
    pub const OK: Color = Color::Rgb(55, 179, 97);
    /// Boot sweep stops (deep blue → accent → cyan → white).
    pub const SWEEP: [Color; 6] = [
        Color::Rgb(31, 52, 77),
        Color::Rgb(51, 132, 255),
        Color::Rgb(102, 166, 255),
        Color::Rgb(34, 211, 238),
        Color::Rgb(230, 233, 238),
        Color::Rgb(230, 233, 238),
    ];
    pub const WARN: Color = Color::Rgb(229, 192, 123);
    pub const ERR: Color = Color::Rgb(233, 73, 73);
    /// Sender colors (hash-picked, omp-style identity tint).
    pub const SENDER: [Color; 8] = [
        Color::Rgb(102, 166, 255),
        Color::Rgb(55, 179, 97),
        Color::Rgb(234, 179, 8),
        Color::Rgb(236, 72, 153),
        Color::Rgb(168, 85, 247),
        Color::Rgb(34, 211, 238),
        Color::Rgb(251, 146, 60),
        Color::Rgb(163, 230, 53),
    ];
    pub fn sender_color(name: &str) -> Color {
        let mut h: u32 = 0;
        for b in name.bytes() {
            h = h.wrapping_mul(31).wrapping_add(b as u32);
        }
        SENDER[(h as usize) % SENDER.len()]
    }
}

pub const SPLASH_LOGO: [&str; 5] = [
    "   ___ ___ ___  ___  ____ _    ___ _____ ___ ",
    r"  / __|_  |   \| / _ \__  |  |_ _|_   _| __|",
    r" | (__ / /| |) | (_) |/ /| |__ | |  | | | _| ",
    r"  \___/___|___/ \___//_/ |____|___| |_| |___|",
    "        native zalo client - rust, no electron",
];

pub struct TuiApp<'a> {
    pub session: &'a mut Session,
    pub threads: Vec<Thread>,
    pub list_state: ListState,
    pub selected_thread: usize,
    pub input_text: String,
    pub focus: Focus,
    pub status: String,
    pub should_quit: bool,
    pub store: Option<crate::store::MessageStore>,
    pub poll_state: chat::PollState,
    filtered: Vec<usize>,
    filter: FilterTab,
    search: String,
    search_mode: bool,
    unread: HashMap<String, usize>,
    selected_msg: Option<usize>,
    msg_scroll: usize,
    follow_tail: bool,
    msg_ranges: Vec<(usize, usize)>,
    chat_view_h: usize,
    pinned_ids: Vec<String>,
    find_query: String,
    find_hits: Vec<usize>,
    find_pos: usize,
    find_mode: bool,
    slash_sel: usize,
    threads_width: u16,
    reply_target: Option<ChatMessage>,
    overlay: Overlay,
    log_scroll: usize,
    sticker: StickerState,
    prompt: Option<PromptState>,
    confirm: Option<ConfirmAction>,
    poll_inflight: bool,
    busy: bool,
    boot_at: std::time::Instant,
    boot_done: bool,
    llm_label: Option<String>,
    llm_override: Option<(String, String)>,
    cached_models: Option<(std::time::Instant, Vec<(String, String, String)>)>,
    sidebar: SidebarMode,
    bg_rx: Receiver<BgEvent>,
    bg_tx: Sender<BgEvent>,
}

impl<'a> TuiApp<'a> {
    pub fn new(session: &'a mut Session, mut threads: Vec<Thread>) -> Self {
        let store = crate::store::MessageStore::open("zalo_history.db").ok();
        if let Some(ref st) = store {
            let _ = st.hydrate_threads(&mut threads, 200);
        }

        let (tx, rx) = channel();

        let mut app = Self {
            session,
            threads,
            list_state: ListState::default(),
            selected_thread: 0,
            input_text: String::new(),
            focus: Focus::Threads,
            status: "Sẵn sàng".into(),
            should_quit: false,
            store,
            poll_state: chat::PollState::default(),
            filtered: Vec::new(),
            filter: FilterTab::All,
            search: String::new(),
            search_mode: false,
            unread: HashMap::new(),
            selected_msg: None,
            msg_scroll: 0,
            follow_tail: true,
            msg_ranges: Vec::new(),
            chat_view_h: 10,
            pinned_ids: TuiApp::load_pinned_ids(),
            find_query: String::new(),
            find_hits: Vec::new(),
            find_pos: 0,
            find_mode: false,
            slash_sel: 0,
            threads_width: 0,
            reply_target: None,
            overlay: Overlay::None,
            log_scroll: 0,
            sticker: StickerState::default(),
            prompt: None,
            confirm: None,
            poll_inflight: false,
            busy: false,
            boot_at: std::time::Instant::now(),
            boot_done: false,
            llm_label: None,
            llm_override: None,
            cached_models: None,
            sidebar: SidebarMode::default(),
            bg_rx: rx,
            bg_tx: tx,
        };
        app.rebuild_filter();
        app.ensure_msg_selection();
        app
    }

    fn session_clone(&self) -> Session {
        (*self.session).clone()
    }

    fn current_thread(&self) -> Option<&Thread> {
        self.threads.get(self.selected_thread)
    }

    fn rebuild_filter(&mut self) {
        let preferred = self.current_thread().map(|t| t.id.clone());
        self.rebuild_filter_prefer(preferred);
    }

    fn rebuild_filter_prefer(&mut self, preferred: Option<String>) {
        let query = self.search.to_lowercase();
        let mut filtered = Vec::new();
        for (i, t) in self.threads.iter().enumerate() {
            if self.filter == FilterTab::Direct && t.is_group {
                continue;
            }
            if self.filter == FilterTab::Group && !t.is_group {
                continue;
            }
            if !query.is_empty() {
                let hit = t.name.to_lowercase().contains(&query)
                    || t.id.contains(&query)
                    || t.preview.to_lowercase().contains(&query)
                    || t.messages
                        .iter()
                        .rev()
                        .take(20)
                        .any(|m| m.text.to_lowercase().contains(&query));
                if !hit {
                    continue;
                }
            }
            filtered.push(i);
        }
        self.filtered = filtered;

        if self.filtered.is_empty() {
            self.list_state.select(None);
            self.selected_thread = self.threads.len();
            self.selected_msg = None;
            self.msg_ranges.clear();
            return;
        }

        let pos = preferred
            .as_ref()
            .and_then(|id| {
                self.filtered
                    .iter()
                    .position(|&i| self.threads[i].id == *id)
            })
            .unwrap_or(0)
            .min(self.filtered.len() - 1);
        let idx = self.filtered[pos];
        self.list_state.select(Some(pos));

        let resolved = self.threads.get(idx).map(|t| t.id.clone());
        if resolved != preferred {
            self.selected_msg = self.threads[idx].messages.len().checked_sub(1);
            self.follow_tail = true;
            self.msg_scroll = 0;
        }
        self.selected_thread = idx;
    }

    /// Select a thread by id (CLI `--thread`). Returns false if unknown.
    pub fn select_thread_by_id(&mut self, thread_id: &str) -> bool {
        if let Some(idx) = self.threads.iter().position(|t| t.id == thread_id) {
            self.select_index(idx);
            self.focus = Focus::Messages;
            self.ensure_msg_selection();
            return true;
        }
        // Fallback: substring match on name (first hit).
        let q = thread_id.to_lowercase();
        if let Some(idx) = self
            .threads
            .iter()
            .position(|t| t.name.to_lowercase().contains(&q) || t.id.contains(thread_id))
        {
            self.select_index(idx);
            self.focus = Focus::Messages;
            self.ensure_msg_selection();
            return true;
        }
        false
    }

    fn select_index(&mut self, thread_idx: usize) {
        let pos = self.filtered.iter().position(|&i| i == thread_idx);
        self.list_state.select(pos);
        self.selected_thread = thread_idx;
        if let Some(t) = self.threads.get(thread_idx) {
            self.unread.remove(&t.id);
            self.selected_msg = t.messages.len().checked_sub(1);
        } else {
            self.selected_msg = None;
        }
        self.msg_scroll = 0;
        self.follow_tail = true;
        self.reply_target = None;
        self.msg_ranges.clear();
    }

    fn move_thread(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            return;
        }
        let cur = self
            .list_state
            .selected()
            .unwrap_or(0)
            .min(self.filtered.len() - 1);
        let n = self.filtered.len() as isize;
        let next = if delta.abs() > 1 {
            (cur as isize + delta).clamp(0, n - 1) as usize
        } else {
            (cur as isize + delta).rem_euclid(n) as usize
        };
        let idx = self.filtered[next];
        self.select_index(idx);
    }

    fn select_first(&mut self) {
        if let Some(&i) = self.filtered.first() {
            self.select_index(i);
        }
    }

    fn select_last(&mut self) {
        if let Some(&i) = self.filtered.last() {
            self.select_index(i);
        }
    }

    fn cycle_filter(&mut self) {
        self.filter = self.filter.next();
        self.rebuild_filter();
        self.status = format!("Lọc: {}", self.filter.label());
    }

    fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Threads => Focus::Messages,
            Focus::Messages => Focus::Input,
            Focus::Input => Focus::Threads,
        };
        if self.focus == Focus::Messages {
            self.ensure_msg_selection();
        }
    }

    fn cycle_focus_rev(&mut self) {
        self.focus = match self.focus {
            Focus::Threads => Focus::Input,
            Focus::Messages => Focus::Threads,
            Focus::Input => Focus::Messages,
        };
        if self.focus == Focus::Messages {
            self.ensure_msg_selection();
        }
    }

    fn escape(&mut self) {
        if self.reply_target.is_some() {
            self.reply_target = None;
            self.status = "Đã huỷ trả lời".into();
            return;
        }
        self.focus = match self.focus {
            Focus::Input => Focus::Messages,
            Focus::Messages => Focus::Threads,
            Focus::Threads => Focus::Threads,
        };
    }


    /// Recompute in-chat hits for the current thread (case-insensitive substring
    /// over sender + text). Keeps cursor on the same message when possible.
    fn recompute_find_hits(&mut self) {
        let q = self.find_query.to_lowercase();
        let cur_msg = self.find_hits.get(self.find_pos).copied();
        self.find_hits.clear();
        if q.is_empty() {
            self.find_pos = 0;
            return;
        }
        if let Some(t) = self.threads.get(self.selected_thread) {
            for (i, m) in t.messages.iter().enumerate() {
                let hay = format!("{} {}", m.sender, m.text).to_lowercase();
                if hay.contains(&q) {
                    self.find_hits.push(i);
                }
            }
        }
        self.find_pos = cur_msg
            .and_then(|m| self.find_hits.iter().position(|&h| h == m))
            .unwrap_or(0);
    }

    /// Jump selection to a hit (dir = +1 next, -1 prev), scrolling it into view.
    fn jump_find(&mut self, dir: isize) {
        if self.find_hits.is_empty() {
            self.status = "Không có kết quả khớp".into();
            return;
        }
        let n = self.find_hits.len() as isize;
        self.find_pos = ((self.find_pos as isize + dir).rem_euclid(n)) as usize;
        let msg_idx = self.find_hits[self.find_pos];
        self.selected_msg = Some(msg_idx);
        self.follow_tail = false;
        self.ensure_msg_visible();
        self.status = format!(
            "Kết quả {}/{} — n: tiếp, N: lùi, Esc: thoát",
            self.find_pos + 1,
            n
        );
    }

    /// Enter in-chat find mode (Ctrl+F). Thread filter `/` stays untouched.
    fn enter_find_mode(&mut self) {
        self.find_mode = true;
        // seed from thread-filter query if any, so `/keyword` + Ctrl+F just works
        if self.find_query.is_empty() && !self.search.is_empty() {
            self.find_query = self.search.clone();
        }
        self.recompute_find_hits();
        if !self.find_hits.is_empty() {
            self.jump_find(0);
        }
        self.status = "Tìm trong hội thoại: gõ từ khoá, Enter xong, n/N nhảy, Esc thoát".into();
    }

    fn exit_find_mode(&mut self) {
        self.find_mode = false;
        self.find_query.clear();
        self.find_hits.clear();
        self.find_pos = 0;
        self.status = "Đã thoát tìm kiếm".into();
    }

    fn ensure_msg_selection(&mut self) {
        let len = self
            .threads
            .get(self.selected_thread)
            .map(|t| t.messages.len())
            .unwrap_or(0);
        if len == 0 {
            self.selected_msg = None;
            return;
        }
        if self.selected_msg.map(|i| i >= len).unwrap_or(true) {
            self.selected_msg = Some(len - 1);
        }
    }

    fn move_message(&mut self, delta: isize) {
        let Some(len) = self
            .threads
            .get(self.selected_thread)
            .map(|t| t.messages.len())
        else {
            return;
        };
        if len == 0 {
            return;
        }
        let cur = self.selected_msg.unwrap_or(len - 1).min(len - 1);
        let next = ((cur as isize + delta).clamp(0, (len - 1) as isize)) as usize;
        self.selected_msg = Some(next);
        self.follow_tail = false;
        self.ensure_msg_visible();
    }

    fn ensure_msg_visible(&mut self) {
        let Some(idx) = self.selected_msg else {
            return;
        };
        let Some(&(start, end)) = self.msg_ranges.get(idx) else {
            return;
        };
        let h = self.chat_view_h.max(1);
        if start < self.msg_scroll {
            self.msg_scroll = start;
        } else if end > self.msg_scroll + h {
            self.msg_scroll = end.saturating_sub(h);
        }
    }

    fn scroll_by(&mut self, delta: isize) {
        let step = delta.unsigned_abs();
        if delta < 0 {
            self.msg_scroll = self.msg_scroll.saturating_sub(step);
        } else {
            self.msg_scroll = self.msg_scroll.saturating_add(step);
        }
        self.follow_tail = false;
    }

    fn scroll_page(&mut self, dir: isize) {
        let step = self.chat_view_h.max(1);
        self.scroll_by(dir * step as isize);
    }

    /// Newest message timestamp; empty threads sink to the bottom.
    fn thread_rank(t: &Thread) -> i64 {
        t.messages.iter().map(|m| m.ts).max().unwrap_or(0)
    }

    /// Pinned thread ids, shared with the GUI (`app_settings.json`).
    /// Read-only here; pin/unpin from the GUI context menu.
    fn load_pinned_ids() -> Vec<String> {
        std::fs::read_to_string("app_settings.json")
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("pinned_threads").cloned())
            .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
            .unwrap_or_default()
    }

    /// Re-sort: pinned first (GUI pin order), then by recency. Call after any merge.
    fn sort_threads(&mut self) {
        let current_id = self.current_thread().map(|t| t.id.clone());
        let pins = self.pinned_ids.clone();
        let rank = |t: &Thread| {
            let pin = pins.iter().position(|p| p == &t.id);
            (pin.is_none(), pin.unwrap_or(0), std::cmp::Reverse(Self::thread_rank(t)))
        };
        self.threads.sort_by_key(|a| rank(a));
        // Refresh pin list too (GUI may have changed it): keep order, drop stale ids.
        let live: std::collections::HashSet<&str> =
            self.threads.iter().map(|t| t.id.as_str()).collect();
        self.pinned_ids.retain(|p| live.contains(p.as_str()));
        self.rebuild_filter_prefer(current_id);
    }

    fn bump_thread(&mut self, thread_id: &str) {
        let selected_id = self.current_thread().map(|t| t.id.clone());
        if let Some(pos) = self.threads.iter().position(|t| t.id == thread_id) {
            if pos != 0 {
                let t = self.threads.remove(pos);
                self.threads.insert(0, t);
            }
        }
        self.rebuild_filter_prefer(selected_id);
    }

    fn persist_snapshot(&mut self, threads: &[Thread]) {
        if let Some(ref mut st) = self.store {
            let _ = crate::store::persist_threads(st, threads);
        } else if let Ok(mut st) = crate::store::MessageStore::open("zalo_history.db") {
            let _ = crate::store::persist_threads(&mut st, threads);
            self.store = Some(st);
        }
    }

    fn persist_current_snapshot(&mut self) {
        let Some(t) = self.threads.get(self.selected_thread) else {
            return;
        };
        let snapshot = vec![t.clone()];
        self.persist_snapshot(&snapshot);
    }

    fn persist_thread_by_id(&mut self, thread_id: &str) {
        let Some(t) = self.threads.iter().find(|t| t.id == thread_id) else {
            return;
        };
        let snapshot = vec![t.clone()];
        self.persist_snapshot(&snapshot);
    }

    fn persist_async(&self) {
        let snapshot = self.threads.clone();
        tokio::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                if let Ok(mut st) = crate::store::MessageStore::open("zalo_history.db") {
                    let _ = crate::store::persist_threads(&mut st, &snapshot);
                }
            })
            .await;
        });
    }

    fn spawn_poll(&mut self) {
        if self.poll_inflight || self.session.login.is_none() {
            return;
        }
        self.poll_inflight = true;
        let session = self.session_clone();
        let mut state = self.poll_state.clone();
        let mut threads = self.threads.clone();
        let tx = self.bg_tx.clone();
        tokio::spawn(async move {
            match chat::poll_new_messages(&session, &mut state, &mut threads).await {
                Ok(()) => {
                    let _ = tx.send(BgEvent::PollDone { threads, state });
                }
                Err(e) => {
                    logger::log_error("tui::poll_new_messages", &e.to_string());
                    let _ = tx.send(BgEvent::PollFailed(e.to_string()));
                }
            }
        });
    }

    fn spawn_refresh(&mut self) {
        self.status = "Đang làm mới...".into();
        let session = self.session_clone();
        let tx = self.bg_tx.clone();
        tokio::spawn(async move {
            let result = async {
                let mut list = chat::get_friends(&session).await?;
                let ids: Vec<String> = list.iter().map(|t| t.id.clone()).collect();
                match chat::get_last_msgs(&session, &ids).await {
                    Ok(resp) => chat::merge_previews(&mut list, &resp),
                    Err(e) => logger::log_error("tui::refresh::get_last_msgs", &e.to_string()),
                }
                if let Ok(mute_resp) = chat::get_mute_conversations(&session).await {
                    crate::unread::merge_mute_metadata(&mut list, &mute_resp);
                }
                Ok::<Vec<Thread>, anyhow::Error>(list)
            }
            .await;
            match result {
                Ok(list) => {
                    let _ = tx.send(BgEvent::ThreadsRefreshed(list));
                }
                Err(e) => {
                    logger::log_error("tui::refresh::get_friends", &e.to_string());
                    if crate::session::SessionStatus::from_error(&e)
                        == crate::session::SessionStatus::Expired
                    {
                        // Credentials are still on disk: re-login replaces them (#21).
                        let _ = tx.send(BgEvent::ActionFailed(
                            "Phiên đã hết hạn (máy chủ từ chối zpw_sek). Bấm 'x' để đăng xuất rồi đăng nhập lại."
                                .into(),
                        ));
                    } else {
                        let _ = tx.send(BgEvent::ActionFailed(format!("Lỗi làm mới: {e}")));
                    }
                }
            }
        });
    }

    fn spawn_history(&mut self) {
        let Some(t) = self.threads.get(self.selected_thread) else {
            return;
        };
        if !t.is_group {
            self.status = "Chỉ áp dụng cho nhóm chat".into();
            return;
        }
        let group_id = t.id.clone();
        self.status = "Đang tải lịch sử nhóm...".into();
        let session = self.session_clone();
        let tx = self.bg_tx.clone();
        let gid = group_id.clone();
        tokio::spawn(async move {
            match chat::get_group_history(&session, &gid, 50).await {
                Ok(resp) => {
                    let _ = tx.send(BgEvent::HistoryLoaded {
                        thread_id: gid,
                        resp,
                    });
                }
                Err(e) => {
                    logger::log_error("tui::get_group_history", &e.to_string());
                    let _ = tx.send(BgEvent::ActionFailed(format!("Lỗi tải lịch sử nhóm: {e}")));
                }
            }
        });
    }

    fn open_sticker_store(&mut self) {
        self.overlay = Overlay::Sticker;
        self.sticker.stage = 0;
        if !self.sticker.packs.is_empty() || self.sticker.loading {
            return;
        }
        self.sticker.loading = true;
        let session = self.session_clone();
        let tx = self.bg_tx.clone();
        tokio::spawn(async move {
            match chat::fetch_sticker_store_packs(&session).await {
                Ok(packs) => {
                    let _ = tx.send(BgEvent::StickerPacks(packs));
                }
                Err(e) => {
                    logger::log_error("tui::fetch_sticker_store_packs", &e.to_string());
                    let _ = tx.send(BgEvent::ActionFailed(format!("Lỗi tải kho nhãn dán: {e}")));
                }
            }
        });
    }

    fn sticker_move(&mut self, delta: isize) {
        let (len, idx) = if self.sticker.stage == 0 {
            (self.sticker.packs.len(), &mut self.sticker.pack_idx)
        } else {
            (self.sticker.items.len(), &mut self.sticker.item_idx)
        };
        if len == 0 {
            return;
        }
        let cur = (*idx).min(len - 1) as isize;
        *idx = ((cur + delta).clamp(0, (len - 1) as isize)) as usize;
    }

    fn sticker_enter(&mut self) {
        if self.sticker.stage == 0 {
            let Some(pack) = self.sticker.packs.get(self.sticker.pack_idx) else {
                return;
            };
            let cate_id = pack.id;
            let name = pack.name.clone();
            if self.sticker.loading {
                return;
            }
            self.sticker.loading = true;
            self.sticker.loaded_name = name.clone();
            let session = self.session_clone();
            let tx = self.bg_tx.clone();
            tokio::spawn(async move {
                match chat::fetch_sticker_pack_items(&session, cate_id, &name).await {
                    Ok(items) => {
                        let _ = tx.send(BgEvent::StickerItems(items));
                    }
                    Err(e) => {
                        logger::log_error("tui::fetch_sticker_pack_items", &e.to_string());
                        let _ = tx.send(BgEvent::ActionFailed(format!("Lỗi tải bộ nhãn dán: {e}")));
                    }
                }
            });
        } else {
            let Some(item) = self.sticker.items.get(self.sticker.item_idx).cloned() else {
                return;
            };
            self.overlay = Overlay::None;
            self.send_sticker(item);
        }
    }

    fn handle_sticker_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::F(2) => {
                self.overlay = Overlay::None;
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                if !self.sticker.items.is_empty() {
                    self.sticker.stage = 1 - self.sticker.stage;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.sticker_move(-1),
            KeyCode::Down | KeyCode::Char('j') => self.sticker_move(1),
            KeyCode::PageUp => self.sticker_move(-10),
            KeyCode::PageDown => self.sticker_move(10),
            KeyCode::Enter => self.sticker_enter(),
            _ => {}
        }
    }

    fn send_sticker(&mut self, item: StickerItem) {
        let Some(t) = self.threads.get(self.selected_thread) else {
            return;
        };
        let to_id = t.id.clone();
        let is_group = t.is_group;

        let now = crate::ui::clock();
        let ts = unix_now_ms();
        let msg = ChatMessage {
            id: format!("local_{ts}"),
            sender: "Tôi".into(),
            text: String::new(),
            time: now.clone(),
            is_outgoing: true,
            ts,
            sticker_id: Some(item.sticker_id),
            sticker_cate_id: Some(item.cate_id),
            sticker_type: Some(item.sticker_type),
            ..Default::default()
        };
        if let Some(t) = self.threads.get_mut(self.selected_thread) {
            t.messages.push(msg);
            t.preview = "[Nhãn dán]".into();
            t.time = now;
        }
        self.selected_msg = None;
        self.follow_tail = true;
        self.persist_current_snapshot();
        chat::save_threads_cache(&self.threads);

        let session = self.session_clone();
        let tx = self.bg_tx.clone();
        tokio::spawn(async move {
            match chat::send_sticker(
                &session,
                &to_id,
                item.sticker_id,
                item.cate_id,
                item.sticker_type,
                is_group,
            )
            .await
            {
                Ok(_) => {
                    let _ = tx.send(BgEvent::StickerSent { thread_id: to_id });
                }
                Err(e) => {
                    logger::log_error("tui::send_sticker", &e.to_string());
                    let _ = tx.send(BgEvent::ActionFailed(format!("Lỗi gửi nhãn dán: {e}")));
                }
            }
        });
        self.status = "Đã gửi nhãn dán".into();
    }

    fn send_attachment(&mut self, path: PathBuf) {
        if !path.is_file() {
            self.status = format!("Không tìm thấy tệp: {}", path.display());
            return;
        }
        let Some(t) = self.threads.get(self.selected_thread) else {
            return;
        };
        let to_id = t.id.clone();
        let is_group = t.is_group;
        let caption = self.input_text.trim().to_string();
        self.input_text.clear();
        self.status = format!("Đang gửi tệp {}...", path.display());

        let session = self.session_clone();
        let tx = self.bg_tx.clone();
        tokio::spawn(async move {
            match chat::send_attachment(&session, &to_id, is_group, &path, &caption).await {
                Ok((_resp, msg)) => {
                    let _ = tx.send(BgEvent::AttachmentSent {
                        thread_id: to_id,
                        msg,
                    });
                }
                Err(e) => {
                    logger::log_error("tui::send_attachment", &e.to_string());
                    let _ = tx.send(BgEvent::ActionFailed(format!("Lỗi gửi tệp: {e}")));
                }
            }
        });
    }

    fn submit_prompt(&mut self, kind: PromptKind, text: String) {
        match kind {
            PromptKind::Attachment => {
                let trimmed = text.trim().to_string();
                if trimmed.is_empty() {
                    self.status = "Chưa nhập đường dẫn tệp".into();
                    return;
                }
                self.send_attachment(PathBuf::from(trimmed));
            }
        }
    }


    /// Slash-command catalog: (name, args hint, description). Single source for
    /// `/help` text and the autocomplete popup.
    pub const SLASH_COMMANDS: [(&'static str, &'static str, &'static str); 10] = [
        ("note", "[xem|sửa text]", "xem/sửa ghi chú hội thoại"),
        ("watch", "[add|rm|list] [kw]", "theo dõi / bỏ từ khóa"),
        ("stats", "", "thống kê hội thoại"),
        ("script", "[tên]", "liệt kê / chạy script per-thread"),
        ("sum", "[câu hỏi]", "tóm tắt / hỏi đáp AI"),
        ("help", "", "hiện trợ giúp này"),
        ("find", "<từ khóa>", "tìm trong hội thoại (= Ctrl+F)"),
        ("privacy", "[on|off]", "cho phép LLM đọc ảnh/file của thread"),
        ("model", "[số|tên|provider...]", "xem / chọn model LLM"),
        ("provider", "[tên]", "xem / chọn nhà cung cấp LLM"),
    ];

    /// Completion candidates for the current input (`/xx` prefix match + script names).
    fn slash_candidates(&mut self) -> Vec<(String, String)> {
        let raw = self.input_text.clone();
        let text = raw.trim_start().to_string();
        if !text.starts_with('/') {
            return Vec::new();
        }
        let after = text[1..].trim_start().to_string();
        let mut out: Vec<(String, String)> = Vec::new();
        // a. builtin commands by prefix
        for (name, hint, desc) in Self::SLASH_COMMANDS {
            if name.starts_with(&after.to_lowercase()) {
                let show = if hint.is_empty() {
                    format!("/{name}")
                } else {
                    format!("/{name} {hint}")
                };
                out.push((show, desc.to_string()));
            }
        }
        // b. provider names after `/provider `
        if let Some(rest) = after.strip_prefix("provider").map(|s| s.trim_start()) {
            let mut provs: Vec<String> = Vec::new();
            for (d, _, _) in self.cached_models() {
                if let Some(p) = d.split('/').next() {
                    if !provs.iter().any(|x| x == p) {
                        provs.push(p.to_string());
                    }
                }
            }
            for p in provs {
                if rest.is_empty() || p.to_lowercase().contains(&rest.to_lowercase()) {
                    out.push((format!("/provider {p}"), "chọn provider".to_string()));
                    if out.len() >= 8 {
                        break;
                    }
                }
            }
            return out;
        }
        // b. model names after `/model ` (fuzzy contains on display/id)
        if let Some(rest) = after.strip_prefix("model").map(|s| s.trim_start()) {
            for (disp, _url, _id) in self.cached_models() {
                if rest.is_empty() || disp.to_lowercase().contains(&rest.to_lowercase()) {
                    out.push((format!("/model {disp}"), "chọn model".to_string()));
                    if out.len() >= 8 {
                        break;
                    }
                }
            }
            return out;
        }
        // c. script names after `/script ` (fuzzy contains)
        if let Some(rest) = after.strip_prefix("script").map(|s| s.trim_start()) {
            if let Some(t) = self.threads.get(self.selected_thread) {
                if let Ok(db) = crate::store::MessageStore::open(&crate::store::db_path_for_account(
                    self.session.login.as_ref().map(|l| l.uin.as_str()).unwrap_or(""),
                )) {
                    for s in db.list_thread_scripts(&t.id) {
                        if !s.enabled {
                            continue;
                        }
                        if rest.is_empty() || s.name.to_lowercase().contains(&rest.to_lowercase()) {
                            out.push((format!("/script {}", s.name), "chạy script".to_string()));
                        }
                    }
                }
            }
        }
        out.truncate(8);
        out
    }

    /// Console commands typed in the input box (`/note /watch /stats /script /sum /help`).
    /// Local-first: note/watch/stats/script run on SQLite + sandbox, no network.
    /// Returns true if the text was a console command (consumed, not sent).
    fn handle_slash(&mut self, raw: &str) -> bool {
        let text = raw.trim();
        if !text.starts_with('/') {
            return false;
        }
        let Some(t) = self.threads.get(self.selected_thread) else {
            self.status = "Chưa chọn hội thoại".into();
            self.input_text.clear();
            return true;
        };
        let tid = t.id.clone();
        let tname = t.name.clone();
        let mut parts = text[1..].splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("").to_lowercase();
        let arg = parts.next().unwrap_or("").trim().to_string();
        self.input_text.clear();
        match cmd.as_str() {
            "help" => {
                let lines: Vec<String> = Self::SLASH_COMMANDS
                    .iter()
                    .map(|(n, h, d)| {
                        if h.is_empty() {
                            format!("/{n} — {d}")
                        } else {
                            format!("/{n} {h} — {d}")
                        }
                    })
                    .collect();
                self.push_console_local(&tid, &format!("Lệnh console:\n{}", lines.join("\n")));
            }
            "find" => {
                if arg.trim().is_empty() {
                    self.enter_find_mode();
                } else {
                    self.find_query = arg.trim().to_string();
                    self.recompute_find_hits();
                    if self.find_hits.is_empty() {
                        self.push_console_local(&tid, &format!("Không thấy '{}'", self.find_query));
                    } else {
                        self.jump_find(0);
                        self.follow_tail = false;
                    }
                }
            }
            "privacy" => {
                let sub = arg.trim().to_lowercase();
                let db = crate::store::db_path_for_account(
                    self.session.login.as_ref().map(|l| l.uin.as_str()).unwrap_or(""),
                );
                match crate::store::MessageStore::open(&db) {
                    Ok(mut st) => {
                        if sub == "on" || sub == "off" {
                            let val = if sub == "on" { "true" } else { "false" };
                            match st.thread_kv_set(&tid, crate::thread_script::PRIVACY_KEY, val) {
                                Ok(()) => self.push_console_local(
                                    &tid,
                                    &format!(
                                        "Quyền đọc ảnh/file của thread: {} (mặc định TẮT)",
                                        if sub == "on" { "BẬT" } else { "TẮT" }
                                    ),
                                ),
                                Err(e) => self.push_console_local(&tid, &format!("Lỗi lưu: {e}")),
                            }
                        } else {
                            let on = crate::thread_script::privacy_allowed(&st, &tid);
                            self.push_console_local(
                                &tid,
                                &format!(
                                    "Quyền đọc ảnh/file hiện tại: {} — dùng `/privacy on|off`",
                                    if on { "BẬT" } else { "TẮT (mặc định)" }
                                ),
                            );
                        }
                    }
                    Err(e) => self.push_console_local(&tid, &format!("Lỗi DB: {e}")),
                }
            }
            "model" => {
                let models = self.cached_models();
                let q = arg.trim();
                if q.is_empty() {
                    // list with index numbers
                    let mut lines = vec![format!(
                        "Model đang dùng: {} — gõ `/model <số|tên>` để đổi",
                        self.model_label().to_string()
                    )];
                    for (i, (disp, url, _)) in models.iter().enumerate() {
                        lines.push(format!("{:>3}. {disp}\n      {url}", i + 1, disp = disp, url = url));
                    }
                    if models.is_empty() {
                        lines.push("(không thấy provider nào — kiểm tra opencode.json)".to_string());
                    }
                    self.push_console_local(&tid, &lines.join("\n"));
                } else {
                    // pick by number or substring (display or id)
                    let found = if let Ok(n) = q.parse::<usize>() {
                        models.get(n.saturating_sub(1)).cloned()
                    } else {
                        let ql = q.to_lowercase();
                        models
                            .iter()
                            .find(|(d, _, id)| {
                                d.to_lowercase().contains(&ql) || id.to_lowercase().contains(&ql)
                            })
                            .cloned()
                    };
                    match found {
                        Some((disp, url, id)) => {
                            self.llm_override = Some((url.clone(), id.clone()));
                            self.llm_label = Some(format!("manual:{disp}"));
                            // persist into settings for GUI/agent reuse
                            if let Ok(txt) = std::fs::read_to_string("app_settings.json") {
                                if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&txt) {
                                    if let Some(o) = v.as_object_mut() {
                                        o.insert("llm_base_url".into(), serde_json::Value::String(url.clone()));
                                        o.insert("llm_model".into(), serde_json::Value::String(id.clone()));
                                        let _ = std::fs::write("app_settings.json", serde_json::to_string_pretty(&v).unwrap_or_default());
                                    }
                                }
                            }
                            self.push_console_local(&tid, &format!("Đã chọn model: {disp}\nEndpoint: {url}"));
                        }
                        None => self.push_console_local(&tid, &format!("Không thấy model '{q}' — gõ `/model` xem danh sách")),
                    }
                }
            }
            "provider" => {
                // distinct provider names from list_models + registry
                let mut provs: Vec<String> = Vec::new();
                for (d, _, _) in self.cached_models() {
                    if let Some(p) = d.split('/').next() {
                        if !provs.iter().any(|x| x == p) {
                            provs.push(p.to_string());
                        }
                    }
                }
                provs.sort();
                let q = arg.trim();
                if q.is_empty() {
                    let mut lines = vec!["Nhà cung cấp khả dụng — gõ `/provider <tên>` để chọn:".to_string()];
                    for p in &provs {
                        lines.push(format!("- {p}"));
                    }
                    self.push_console_local(&tid, &lines.join("\n"));
                } else {
                    let ql = q.to_lowercase();
                    match provs.iter().find(|p| p.to_lowercase().contains(&ql)).cloned() {
                        Some(p) => {
                            // pick first model of that provider as default
                            let models = self.cached_models();
                            if let Some((disp, url, id)) = models.iter().find(|(d, _, _)| d.starts_with(&format!("{p}/"))).cloned() {
                                self.llm_override = Some((url.clone(), id.clone()));
                                self.llm_label = Some(format!("manual:{disp}"));
                                self.push_console_local(&tid, &format!("Đã chọn provider: {p} → model mặc định {disp}\nEndpoint: {url}\nGõ `/model <tên>` để đổi model trong provider"));
                            } else {
                                self.push_console_local(&tid, &format!("Provider {p} chưa có model nào"));
                            }
                        }
                        None => self.push_console_local(&tid, &format!("Không thấy provider '{q}'")),
                    }
                }
            }
            "note" => {
                let arg_n = arg.clone();
                if arg_n.is_empty() {
                    // view
                    let db = crate::store::db_path_for_account(
                        self.session.login.as_ref().map(|l| l.uin.as_str()).unwrap_or(""),
                    );
                    let note = crate::store::MessageStore::open(&db)
                        .map(|st| st.get_group_note(&tid))
                        .unwrap_or_default();
                    let shown = if note.trim().is_empty() {
                        "(chưa có ghi chú — /note <nội dung> để lưu)".to_string()
                    } else {
                        note
                    };
                    self.push_console_local(&tid, &format!("Ghi chú [{tname}]:\n{shown}"));
                } else {
                    // edit (async save like GUI debounce, but immediate here)
                    let tx = self.bg_tx.clone();
                    let uin = self
                        .session
                        .login
                        .as_ref()
                        .map(|l| l.uin.clone())
                        .unwrap_or_default();
                    tokio::spawn(async move {
                        let db = crate::store::db_path_for_account(&uin);
                        let res = (|| -> anyhow::Result<()> {
                            let mut st = crate::store::MessageStore::open(&db)?;
                            st.set_group_note(&tid, &arg_n, chrono::Local::now().timestamp())?;
                            Ok(())
                        })();
                        let line = match res {
                            Ok(()) => format!("Đã lưu ghi chú [{tname}]"),
                            Err(e) => format!("Lỗi lưu ghi chú: {e}"),
                        };
                        let _ = tx.send(BgEvent::ConsoleLine { thread_id: tid, text: line });
                    });
                    self.busy = true;
                    self.status = "Đang lưu ghi chú...".into();
                }
            }
            "watch" => {
                let arg_w = arg.clone();
                let tx = self.bg_tx.clone();
                let uin = self
                    .session
                    .login
                    .as_ref()
                    .map(|l| l.uin.clone())
                    .unwrap_or_default();
                tokio::spawn(async move {
                    let db = crate::store::db_path_for_account(&uin);
                    let line = (|| -> anyhow::Result<String> {
                        let mut st = crate::store::MessageStore::open(&db)?;
                        let mut it = arg_w.splitn(2, char::is_whitespace);
                        let sub = it.next().unwrap_or("").to_lowercase();
                        let rest = it.next().unwrap_or("").trim();
                        match sub.as_str() {
                            "add" if !rest.is_empty() => {
                                if st.add_group_watch(&tid, rest)? {
                                    Ok(format!("Đã theo dõi từ khóa: {rest}"))
                                } else {
                                    Ok(format!("Từ khóa đã có: {rest}"))
                                }
                            }
                            "rm" if !rest.is_empty() => {
                                st.remove_group_watch(&tid, rest)?;
                                Ok(format!("Đã bỏ theo dõi: {rest}"))
                            }
                            _ => {
                                let kws = st.list_group_watch(&tid);
                                if kws.is_empty() {
                                    Ok("(chưa theo dõi từ khóa nào — /watch add <kw>)".into())
                                } else {
                                    Ok(format!("Từ khóa đang theo dõi: {}", kws.join(", ")))
                                }
                            }
                        }
                    })();
                    let text = match line {
                        Ok(s) => s,
                        Err(e) => format!("Lỗi watch: {e}"),
                    };
                    let _ = tx.send(BgEvent::ConsoleLine { thread_id: tid, text });
                });
                self.busy = true;
                    self.status = "Đang xử lý watch...".into();
            }
            "stats" => {
                let db = crate::store::db_path_for_account(
                    self.session.login.as_ref().map(|l| l.uin.as_str()).unwrap_or(""),
                );
                let store = crate::store::MessageStore::open(&db).ok();
                let thread = self.threads.iter().find(|t| t.id == tid).cloned();
                let st = crate::group_stats::compute_group_stats(store.as_ref(), thread.as_ref());
                let top: Vec<String> = st
                    .top_senders
                    .iter()
                    .take(5)
                    .map(|(n, c)| format!("{n} ({c})"))
                    .collect();
                let peak = st
                    .peak_hour
                    .map(|h| format!("{:02}h ({} tin)", h, st.peak_hour_count))
                    .unwrap_or_else(|| "—".into());
                self.push_console_local(
                    &tid,
                    &format!(
                        "Thống kê [{tname}]: tổng {} · 7 ngày {} · ảnh {} file {} link {} voice {} · giờ sôi động {} · top: {}",
                        st.total,
                        st.last_7d,
                        st.photos,
                        st.files,
                        st.links,
                        st.voices,
                        peak,
                        if top.is_empty() { "—".into() } else { top.join(", ") }
                    ),
                );
            }
            "script" => {
                let arg_s = arg.clone();
                let list_only = arg_s.is_empty();
                if list_only {
                    // list scripts
                    let db = crate::store::db_path_for_account(
                        self.session.login.as_ref().map(|l| l.uin.as_str()).unwrap_or(""),
                    );
                    let names = crate::store::MessageStore::open(&db)
                        .map(|st| {
                            st.list_thread_scripts(&tid)
                                .iter()
                                .map(|s| format!("{}{}", s.name, if s.enabled { "" } else { " (tắt)" }))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let shown = if names.is_empty() {
                        "(chưa có script — tạo trong GUI tab Scripts)".to_string()
                    } else {
                        names.join(", ")
                    };
                    self.push_console_local(&tid, &format!("Scripts [{tname}]: {shown}"));
                } else {
                    // run script by name (async sandbox, outbox drained as console lines)
                    let arg_run = arg_s.clone();
                    let tx = self.bg_tx.clone();
                    let threads = self.threads.clone();
                    let uin = self
                        .session
                        .login
                        .as_ref()
                        .map(|l| l.uin.clone())
                        .unwrap_or_default();
                    tokio::spawn(async move {
                        let db = crate::store::db_path_for_account(&uin);
                        let line = (|| -> anyhow::Result<String> {
                            let st = crate::store::MessageStore::open(&db)?;
                            let scripts = st.list_thread_scripts(&tid);
                            let s = scripts
                                .iter()
                                .find(|s| s.name == arg_run && s.enabled)
                                .ok_or_else(|| anyhow::anyhow!("không thấy script '{arg_run}'"))?;
                            let ev = crate::thread_script::ScriptEvent::manual();
                            let out =
                                crate::thread_script::run_script(&db, &threads, &tid, s, &ev)?;
                            let mut shown = out.output.clone();
                            for o in &out.outbox {
                                shown.push_str(&format!("\n[queued → {}] {}", o.thread_id, o.text));
                            }
                            for t in &out.toasts {
                                shown.push_str(&format!("\n[toast] {t}"));
                            }
                            if shown.trim().is_empty() {
                                shown = "(chạy xong, không có output)".to_string();
                            }
                            Ok(format!("Script '{arg_run}':\n{shown}"))
                        })();
                        let text = match line {
                            Ok(s) => s,
                            Err(e) => format!("Lỗi script: {e}"),
                        };
                        let _ = tx.send(BgEvent::ConsoleLine { thread_id: tid, text });
                    });
                    self.busy = true;
                    self.status = format!("Đang chạy script '{arg_s}'...");
                }
            }
            "sum" => {
                let arg_q = arg.clone();
                let ov = self.llm_override.clone();
                let tx = self.bg_tx.clone();
                let threads = self.threads.clone();
                let uin = self
                    .session
                    .login
                    .as_ref()
                    .map(|l| l.uin.clone())
                    .unwrap_or_default();
                tokio::spawn(async move {
                    let db = crate::store::db_path_for_account(&uin);
                    let text = async {
                        let st = crate::store::MessageStore::open(&db)?;
                        let gctx =
                            crate::group_llm::build_group_context(&st, &threads, &tid);
                        if crate::thread_script::privacy_allowed(&st, &tid) {
                            if let Ok(mut st2) = crate::store::MessageStore::open(&db) {
                                let _ = st2.push_script_log(&tid, "privacy", "info", "TUI /sum WITH attachments (consent on)");
                            }
                        }
                        // Unified provider: env/CTDOZ_* > opencode.json > ollama local.
                        let p = if let Some((url, id)) = ov.clone() {
                            let (key, _) = crate::auth::resolve_key(&std::fs::read_to_string("app_settings.json").ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).and_then(|v| v.get("llm_api_key").and_then(|k| k.as_str()).map(str::to_string)).unwrap_or_default());
                            crate::llm::UnifiedProvider::from_parts(&url, &key, &id, "manual")
                        } else {
                            crate::llm::UnifiedProvider::from_settings("", "", "")
                        };
                        let (sys, user) = p.sys_and_user(&gctx, if arg_q.is_empty() { None } else { Some(arg_q.as_str()) });
                        crate::llm::chat_completions(&p.ep, &sys, &user).await
                    }
                    .await
                    .unwrap_or_else(|e| format!("{e}"));
                    let _ = tx.send(BgEvent::ConsoleLine { thread_id: tid, text });
                });
                self.busy = true;
                    self.status = "Đang tóm tắt...".into();
            }
            _ => {
                self.push_console_local(
                    &tid,
                    &format!("Lệnh lạ: /{cmd} — gõ /help"),
                );
            }
        }
        true
    }

    /// Push a console output line straight into the current thread (sync path).
    fn push_console_local(&mut self, thread_id: &str, text: &str) {
        let now = crate::ui::clock();
        let ts = unix_now_ms();
        if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
            t.messages.push(ChatMessage {
                id: format!("local_console_{ts}"),
                sender: "Console".into(),
                text: text.to_string(),
                time: now.clone(),
                is_outgoing: false,
                ts,
                ..Default::default()
            });
            t.preview = "Console output".into();
            t.time = now;
        }
        self.selected_msg = None;
        self.follow_tail = true;
        self.persist_thread_by_id(thread_id);
        self.bump_thread(thread_id);
        self.status = "Lệnh console xong".into();
    }

    /// Popup visible iff input starts with `/` and has candidates.
    fn slash_popup(&mut self) -> Vec<(String, String)> {
        if self.focus != Focus::Input {
            return Vec::new();
        }
        self.slash_candidates()
    }

    /// Move popup selection. Returns true if popup was active.
    fn cycle_slash(&mut self, dir: isize) -> bool {
        let n = self.slash_candidates().len();
        if n == 0 {
            return false;
        }
        self.slash_sel = ((self.slash_sel as isize + dir).rem_euclid(n as isize)) as usize;
        true
    }

    /// Accept current popup item into the input. Returns true if applied.
    fn apply_slash_completion(&mut self) -> bool {
        let items = self.slash_candidates();
        if items.is_empty() {
            return false;
        }
        let idx = self.slash_sel.min(items.len() - 1);
        let full = items[idx].0.clone();
        self.input_text = if full.contains(' ') || full.matches('/').count() > 1 {
            full
        } else {
            format!("{full} ")
        };
        self.slash_sel = 0;
        true
    }

    /// First Esc closes popup (keeps text); second Esc leaves input.
    fn hide_slash_popup(&mut self) -> bool {
        if self.input_text.trim_start().starts_with('/') && !self.slash_candidates().is_empty() {
            return true;
        }
        false
    }

    fn send_current(&mut self) {
        let text = self.input_text.trim().to_string();
        if text.is_empty() {
            return;
        }
        if text.starts_with('/') {
            self.handle_slash(&text);
            return;
        }
        let Some(t) = self.threads.get(self.selected_thread) else {
            self.status = "Chưa chọn hội thoại".into();
            return;
        };
        let to_id = t.id.clone();
        let is_group = t.is_group;

        self.input_text.clear();
        let reply = self.reply_target.take();
        let now = crate::ui::clock();
        let ts = unix_now_ms();
        let msg = ChatMessage {
            id: format!("local_{ts}"),
            sender: "Tôi".into(),
            text: text.clone(),
            time: now.clone(),
            is_outgoing: true,
            ts,
            reply_to_id: reply.as_ref().map(|m| m.id.clone()),
            reply_to_sender: reply.as_ref().map(|m| m.sender.clone()),
            reply_to_text: reply.as_ref().map(|m| m.text.clone()),
            ..Default::default()
        };
        if let Some(t) = self.threads.get_mut(self.selected_thread) {
            t.messages.push(msg);
            t.preview = text.clone();
            t.time = now;
        }
        self.selected_msg = None;
        self.follow_tail = true;
        self.persist_current_snapshot();
        chat::save_threads_cache(&self.threads);
        self.bump_thread(&to_id);

        let session = self.session_clone();
        let tx = self.bg_tx.clone();
        tokio::spawn(async move {
            let result = match (&reply, is_group) {
                (Some(q), true) => chat::send_quote_message(&session, &to_id, &text, q, true)
                    .await
                    .map(|_| ()),
                (Some(q), false) => chat::send_quote_message(&session, &to_id, &text, q, false)
                    .await
                    .map(|_| ()),
                (None, true) => chat::send_group_message(&session, &to_id, &text)
                    .await
                    .map(|_| ()),
                (None, false) => chat::send_message(&session, &to_id, &text)
                    .await
                    .map(|_| ()),
            };
            match result {
                Ok(()) => {
                    let _ = tx.send(BgEvent::MessageSent {
                        thread_id: to_id,
                        text,
                    });
                }
                Err(e) => {
                    logger::log_error("tui::send_message", &e.to_string());
                    let _ = tx.send(BgEvent::SendFailed(e.to_string()));
                }
            }
        });
        self.status = "Đang gửi tin nhắn...".into();
    }

    fn react_quick(&mut self, key: char) {
        let Some(idx) = key
            .to_digit(10)
            .map(|d| d as usize)
            .and_then(|d| d.checked_sub(1))
        else {
            return;
        };
        let Some(emoji) = QUICK_REACTIONS.get(idx).copied() else {
            return;
        };
        let Some(mi) = self.selected_msg else {
            return;
        };
        let (to_id, is_group, msg) = {
            let Some(t) = self.threads.get(self.selected_thread) else {
                return;
            };
            let Some(msg) = t.messages.get(mi) else {
                return;
            };
            (t.id.clone(), t.is_group, msg.clone())
        };
        if let Some(m) = self
            .threads
            .get_mut(self.selected_thread)
            .and_then(|t| t.messages.get_mut(mi))
        {
            let code = chat::reaction_code_and_type(emoji).0;
            if !m.reactions.iter().any(|r| r == code) {
                m.reactions.push(code.to_string());
            }
        }
        let session = self.session_clone();
        let tx = self.bg_tx.clone();
        let msg_id = msg.id.clone();
        tokio::spawn(async move {
            match chat::send_reaction(&session, &to_id, is_group, &msg, emoji).await {
                Ok(_) => {
                    let _ = tx.send(BgEvent::ReactionDone {
                        thread_id: to_id,
                        msg_id,
                        icon: emoji.to_string(),
                    });
                }
                Err(e) => {
                    logger::log_error("tui::send_reaction", &e.to_string());
                    let _ = tx.send(BgEvent::ActionFailed(format!("Lỗi thả cảm xúc: {e}")));
                }
            }
        });
        self.status = format!("Đã thả cảm xúc {emoji}");
    }

    fn quote_selected(&mut self) {
        let Some(mi) = self.selected_msg else {
            return;
        };
        let Some(t) = self.threads.get(self.selected_thread) else {
            return;
        };
        let Some(m) = t.messages.get(mi) else {
            return;
        };
        self.reply_target = Some(m.clone());
        self.focus = Focus::Input;
        self.status = "Đang trả lời tin nhắn (Ctrl+X để huỷ)".into();
    }

    fn confirm_delete_selected(&mut self) {
        let Some(mi) = self.selected_msg else {
            return;
        };
        let Some(t) = self.threads.get(self.selected_thread) else {
            return;
        };
        let Some(m) = t.messages.get(mi) else {
            return;
        };
        if m.id.is_empty() || m.id.starts_with("local_") {
            self.status = "Tin nhắn chưa đồng bộ, chưa thể xoá trên server".into();
            return;
        }
        let preview = m.text.chars().take(60).collect::<String>();
        self.confirm = Some(ConfirmAction {
            question: "Xoá tin nhắn này (chỉ phía bạn)?".into(),
            detail: preview,
            kind: ConfirmKind::DeleteMessage,
            thread_id: t.id.clone(),
            msg_id: m.id.clone(),
        });
    }

    fn open_selected_link(&mut self) {
        let Some(mi) = self.selected_msg else {
            return;
        };
        let url = self
            .threads
            .get(self.selected_thread)
            .and_then(|t| t.messages.get(mi))
            .and_then(|m| m.link_url.clone().or_else(|| m.link_src.clone()));
        match url {
            Some(u) if !u.is_empty() => {
                open_external_url(&u);
                self.status = format!("Mở liên kết: {u}");
            }
            _ => self.status = "Tin nhắn không có liên kết".into(),
        }
    }

    fn run_confirm(&mut self, action: ConfirmAction) {
        match action.kind {
            ConfirmKind::Logout => {
                let _ = std::fs::remove_file("session.json");
                let _ = std::fs::remove_file("threads_cache.json");
                self.session.logout();
                self.threads.clear();
                self.filtered.clear();
                self.list_state.select(None);
                self.unread.clear();
                self.selected_thread = 0;
                self.selected_msg = None;
                self.status = "Đã đăng xuất. Bấm 'q' để thoát.".into();
            }
            ConfirmKind::WipeLocalData => {
                // History only: the session and the thread list stay usable (#21).
                let _ = std::fs::remove_file("zalo_history.db");
                let _ = std::fs::remove_file("threads_cache.json");
                for t in self.threads.iter_mut() {
                    t.messages.clear();
                }
                self.status = "Đã xoá dữ liệu tin nhắn cục bộ (session giữ nguyên).".into();
            }
            ConfirmKind::DeleteMessage => {
                let thread_id = action.thread_id;
                let msg_id = action.msg_id;
                let Some(t) = self.threads.iter().find(|t| t.id == thread_id) else {
                    return;
                };
                let Some(msg) = t.messages.iter().find(|m| m.id == msg_id).cloned() else {
                    return;
                };
                let is_group = t.is_group;
                let session = self.session_clone();
                let tx = self.bg_tx.clone();
                tokio::spawn(async move {
                    match chat::delete_message_for_me(&session, &thread_id, is_group, &msg).await {
                        Ok(_) => {
                            let _ = tx.send(BgEvent::DeleteDone { thread_id, msg_id });
                        }
                        Err(e) => {
                            logger::log_error("tui::delete_message_for_me", &e.to_string());
                            let _ =
                                tx.send(BgEvent::ActionFailed(format!("Lỗi xoá tin nhắn: {e}")));
                        }
                    }
                });
                self.status = "Đang xoá tin nhắn...".into();
            }
        }
    }

    fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        if mods.contains(KeyModifiers::CONTROL) {
            match code {
                KeyCode::Char('c') => {
                    self.should_quit = true;
                    return;
                }
                KeyCode::Char('f') => {
                    self.enter_find_mode();
                    return;
                }
                KeyCode::Char('u') => {
                    self.input_text.clear();
                    return;
                }
                KeyCode::Char('b') => {
                    self.sidebar = self.sidebar.cycle();
                    self.status = format!("Sidebar: {}", self.sidebar.label());
                    return;
                }
                KeyCode::Char('x') => {
                    if self.reply_target.is_some() {
                        self.reply_target = None;
                        self.status = "Đã huỷ trả lời".into();
                    }
                    return;
                }
                _ => {}
            }
        }

        if let Some(action) = self.confirm.take() {
            match code {
                KeyCode::Char('y') | KeyCode::Enter => self.run_confirm(action),
                KeyCode::Char('n') | KeyCode::Esc => self.status = "Đã huỷ thao tác".into(),
                _ => self.confirm = Some(action),
            }
            return;
        }

        if let Some(mut prompt) = self.prompt.take() {
            match code {
                KeyCode::Esc => {
                    self.status = "Đã huỷ nhập".into();
                }
                KeyCode::Enter => {
                    let text = prompt.text.clone();
                    self.submit_prompt(prompt.kind, text);
                }
                KeyCode::Backspace => {
                    prompt.text.pop();
                    self.prompt = Some(prompt);
                }
                KeyCode::Char(c) => {
                    if !c.is_control() && !mods.contains(KeyModifiers::CONTROL) {
                        prompt.text.push(c);
                    }
                    self.prompt = Some(prompt);
                }
                _ => self.prompt = Some(prompt),
            }
            return;
        }

        match self.overlay {
            Overlay::Help => {
                if code == KeyCode::Esc
                    || code == KeyCode::F(1)
                    || code == KeyCode::Char('?')
                    || code == KeyCode::Char('q')
                {
                    self.overlay = Overlay::None;
                }
                return;
            }
            Overlay::Log => {
                match code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('l') => {
                        self.overlay = Overlay::None;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.log_scroll = self.log_scroll.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.log_scroll = self.log_scroll.saturating_add(1);
                    }
                    KeyCode::PageUp => self.log_scroll = self.log_scroll.saturating_sub(10),
                    KeyCode::PageDown => self.log_scroll = self.log_scroll.saturating_add(10),
                    _ => {}
                }
                return;
            }
            Overlay::Sticker => {
                self.handle_sticker_key(code);
                return;
            }
            Overlay::None => {}
        }

        if self.find_mode {
            match code {
                KeyCode::Esc => {
                    self.exit_find_mode();
                }
                KeyCode::Enter => {
                    self.find_mode = false;
                    self.recompute_find_hits();
                    if !self.find_hits.is_empty() {
                        self.jump_find(0);
                    } else {
                        self.status = "Không tìm thấy kết quả".into();
                    }
                }
                KeyCode::Backspace => {
                    self.find_query.pop();
                    self.recompute_find_hits();
                    if !self.find_hits.is_empty() {
                        self.jump_find(0);
                    }
                }
                KeyCode::Char('n') => self.jump_find(1),
                KeyCode::Char('N') => self.jump_find(-1),
                KeyCode::Char(c) => {
                    if !c.is_control() && !mods.contains(KeyModifiers::CONTROL) {
                        self.find_query.push(c);
                        self.recompute_find_hits();
                        if !self.find_hits.is_empty() {
                            self.jump_find(0);
                        }
                    }
                }
                _ => {}
            }
            return;
        }

        if self.search_mode {
            match code {
                KeyCode::Esc => {
                    self.search.clear();
                    self.search_mode = false;
                    self.rebuild_filter();
                }
                KeyCode::Enter => self.search_mode = false,
                KeyCode::Backspace => {
                    self.search.pop();
                    self.rebuild_filter();
                }
                KeyCode::Up | KeyCode::Char('k') => self.move_thread(-1),
                KeyCode::Down | KeyCode::Char('j') => self.move_thread(1),
                KeyCode::Char(c) => {
                    self.search.push(c);
                    self.rebuild_filter();
                }
                _ => {}
            }
            return;
        }

        match code {
            KeyCode::F(1) => {
                self.overlay = Overlay::Help;
                return;
            }
            KeyCode::Char('?') if self.focus != Focus::Input => {
                self.overlay = Overlay::Help;
                return;
            }
            KeyCode::F(2) => {
                self.open_sticker_store();
                return;
            }
            KeyCode::F(3) => {
                self.prompt = Some(PromptState {
                    kind: PromptKind::Attachment,
                    text: String::new(),
                });
                return;
            }
            KeyCode::Char('/') if self.focus != Focus::Input => {
                self.search_mode = true;
                self.status = "Tìm kiếm: gõ từ khoá, Enter để xong, Esc để xoá".into();
                return;
            }
            KeyCode::Char('f') if self.focus != Focus::Input => {
                self.cycle_filter();
                return;
            }
            KeyCode::Char('u') if self.focus != Focus::Input => {
                self.unread.clear();
                self.status = "Đã xoá thông báo chưa đọc".into();
                return;
            }
            KeyCode::Char('r') if self.focus != Focus::Input => {
                self.spawn_refresh();
                return;
            }
            KeyCode::Char('h') if self.focus != Focus::Input => {
                self.spawn_history();
                return;
            }
            KeyCode::Char('l') if self.focus != Focus::Input => {
                if logger::is_dev_mode() {
                    self.overlay = Overlay::Log;
                    self.log_scroll = 0;
                } else {
                    self.status = "Nhật ký chỉ có trong chế độ --dev".into();
                }
                return;
            }
            KeyCode::Char('w') if self.focus != Focus::Input => {
                self.confirm = Some(ConfirmAction {
                    question: "Xoá lịch sử tin nhắn cục bộ (zalo_history.db, threads_cache.json)?"
                        .into(),
                    detail: "Session đăng nhập giữ nguyên. Thao tác này không thể hoàn tác.".into(),
                    kind: ConfirmKind::WipeLocalData,
                    thread_id: String::new(),
                    msg_id: String::new(),
                });
                return;
            }
            KeyCode::Char('x') if self.focus != Focus::Input => {
                self.confirm = Some(ConfirmAction {
                    question: "Đăng xuất và xoá session.json, threads_cache.json?".into(),
                    detail: "Thao tác này không thể hoàn tác.".into(),
                    kind: ConfirmKind::Logout,
                    thread_id: String::new(),
                    msg_id: String::new(),
                });
                return;
            }
            KeyCode::Char('q') if self.focus != Focus::Input => {
                self.should_quit = true;
                return;
            }
            KeyCode::Tab => {
                self.cycle_focus();
                return;
            }
            KeyCode::BackTab => {
                self.cycle_focus_rev();
                return;
            }
            KeyCode::Esc => {
                self.escape();
                return;
            }
            _ => {}
        }

        match self.focus {
            Focus::Threads => match code {
                KeyCode::Up | KeyCode::Char('k') => self.move_thread(-1),
                KeyCode::Down | KeyCode::Char('j') => self.move_thread(1),
                KeyCode::PageUp => self.move_thread(-10),
                KeyCode::PageDown => self.move_thread(10),
                KeyCode::Home => self.select_first(),
                KeyCode::End => self.select_last(),
                KeyCode::Enter => {
                    self.focus = Focus::Messages;
                    self.ensure_msg_selection();
                }
                KeyCode::Char(c) if !c.is_control() && !mods.contains(KeyModifiers::CONTROL) => {
                    self.focus = Focus::Input;
                    self.input_text.push(c);
                }
                _ => {}
            },
            Focus::Messages => match code {
                KeyCode::Up | KeyCode::Char('k') => self.move_message(-1),
                KeyCode::Down | KeyCode::Char('j') => self.move_message(1),
                KeyCode::PageUp => self.scroll_page(-1),
                KeyCode::PageDown => self.scroll_page(1),
                KeyCode::Home => {
                    self.msg_scroll = 0;
                    self.follow_tail = false;
                }
                KeyCode::End => self.follow_tail = true,
                KeyCode::Enter => self.focus = Focus::Input,
                KeyCode::Char(c @ '1'..='6') => self.react_quick(c),
                KeyCode::Char('y') => self.quote_selected(),
                KeyCode::Char('d') => self.confirm_delete_selected(),
                KeyCode::Char('o') => self.open_selected_link(),
                KeyCode::Char(c) if !c.is_control() && !mods.contains(KeyModifiers::CONTROL) => {
                    self.focus = Focus::Input;
                    self.input_text.push(c);
                }
                _ => {}
            },
            Focus::Input => match code {
                KeyCode::Enter => {
                    if self.apply_slash_completion() {
                        return;
                    }
                    self.send_current();
                }
                KeyCode::Tab => {
                    self.apply_slash_completion();
                }
                KeyCode::Esc => {
                    if self.hide_slash_popup() {
                        self.slash_sel = 0;
                    } else {
                        self.focus = Focus::Messages;
                        self.ensure_msg_selection();
                    }
                }
                KeyCode::Up => {
                    self.cycle_slash(-1);
                }
                KeyCode::Down => {
                    self.cycle_slash(1);
                }
                KeyCode::Backspace => {
                    self.input_text.pop();
                    self.slash_sel = 0;
                }
                KeyCode::Char(c) => {
                    if !c.is_control() && !mods.contains(KeyModifiers::CONTROL) {
                        self.input_text.push(c);
                        self.slash_sel = 0;
                    }
                }
                _ => {}
            },
        }
    }

    fn handle_mouse(&mut self, m: MouseEvent) {
        let in_threads = (m.column as usize) < self.threads_width as usize;
        match m.kind {
            MouseEventKind::ScrollUp => {
                if in_threads {
                    self.move_thread(-1);
                } else {
                    self.scroll_by(-3);
                }
            }
            MouseEventKind::ScrollDown => {
                if in_threads {
                    self.move_thread(1);
                } else {
                    self.scroll_by(3);
                }
            }
            _ => {}
        }
    }

    /// Model label for the footer (`source/model`, resolved once).
    /// Cached model list (opencode.json read, 5s TTL — keeps typing smooth).
    fn cached_models(&mut self) -> Vec<(String, String, String)> {
        let fresh = self
            .cached_models
            .as_ref()
            .is_some_and(|(at, _)| at.elapsed() < std::time::Duration::from_secs(5));
        if !fresh {
            let list = crate::llm::list_models();
            self.cached_models = Some((std::time::Instant::now(), list));
        }
        self.cached_models.as_ref().map(|(_, l)| l.clone()).unwrap_or_default()
    }

    /// Provider honoring the session `/model` override (else global resolve).
    fn llm_provider(&self) -> crate::llm::UnifiedProvider {
        if let Some((url, id)) = self.llm_override.clone() {
            // key still via auth chain (env → keychain → settings)
            let (key, _) = crate::auth::resolve_key("");
            let settings_key = std::fs::read_to_string("app_settings.json")
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get("llm_api_key").and_then(|k| k.as_str()).map(str::to_string))
                .unwrap_or_default();
            let (key, _) = if !key.trim().is_empty() {
                (key, crate::auth::KeySource::Env)
            } else {
                crate::auth::resolve_key(&settings_key)
            };
            return crate::llm::UnifiedProvider::from_parts(&url, &key, &id, "manual");
        }
        crate::llm::UnifiedProvider::from_settings("", "", "")
    }

    fn model_label(&mut self) -> &str {
        if self.llm_label.is_none() {
            let ep = self.llm_provider().ep;
            self.llm_label = Some(format!("{}/{}", ep.source, ep.model));
        }
        self.llm_label.as_deref().unwrap_or("llm?")
    }

    fn refresh_pins(&mut self) {
        let fresh = Self::load_pinned_ids();
        if fresh != self.pinned_ids {
            self.pinned_ids = fresh;
        }
    }

    fn apply_polled(&mut self, polled: Vec<Thread>, state: chat::PollState) -> bool {
        self.refresh_pins();
        self.poll_state = state;
        let current_id = self.current_thread().map(|t| t.id.clone());

        let mut old: HashMap<String, (HashSet<String>, i64, usize, String)> = HashMap::new();
        for t in &self.threads {
            old.insert(
                t.id.clone(),
                (
                    t.messages.iter().map(|m| m.id.clone()).collect(),
                    t.messages.iter().map(|m| m.ts).max().unwrap_or(0),
                    t.messages.len(),
                    t.preview.clone(),
                ),
            );
        }

        let mut changed = false;
        for t in &polled {
            match old.get(&t.id) {
                Some((prev_ids, prev_max_ts, prev_len, prev_preview)) => {
                    if t.messages.len() != *prev_len || &t.preview != prev_preview {
                        changed = true;
                    }
                    let fresh = t
                        .messages
                        .iter()
                        .filter(|m| {
                            !m.is_outgoing
                                && !m.id.is_empty()
                                && !m.id.starts_with("local_")
                                && !prev_ids.contains(&m.id)
                                && !(*prev_max_ts > 0 && m.ts > 0 && m.ts <= *prev_max_ts)
                        })
                        .count();
                    if fresh > 0 && current_id.as_deref() != Some(t.id.as_str()) {
                        *self.unread.entry(t.id.clone()).or_insert(0) += fresh;
                        changed = true;
                    }
                }
                None => changed = true,
            }
        }

        // Merge into the live threads instead of replacing them, so optimistic
        // messages and history loaded while the poll was in flight survive.
        for incoming in polled {
            if let Some(pos) = self.threads.iter().position(|x| x.id == incoming.id) {
                chat::merge_thread_into(&mut self.threads[pos], incoming, false);
            } else {
                self.threads.push(incoming);
                changed = true;
            }
        }

        if let Some(id) = &current_id {
            self.unread.remove(id);
        }
        self.sort_threads();
        self.msg_ranges.clear();
        changed
    }

    fn apply_refresh(&mut self, list: Vec<Thread>) {
        self.refresh_pins();
        if list.is_empty() {
            self.status = "Không lấy được danh sách hội thoại".into();
            return;
        }
        for ft in list {
            if let Some(pos) = self.threads.iter().position(|t| t.id == ft.id) {
                chat::merge_thread_into(&mut self.threads[pos], ft, false);
            } else {
                self.threads.push(ft);
            }
        }
        self.sort_threads();
        chat::save_threads_cache(&self.threads);
        self.persist_async();
    }

    fn check_background_events(&mut self) {
        while let Ok(event) = self.bg_rx.try_recv() {
            match event {
                BgEvent::PollDone { threads, state } => {
                    self.poll_inflight = false;
                    let changed = self.apply_polled(threads, state);
                    self.status = format!("Cập nhật {}", crate::ui::clock());
                    if changed {
                        chat::save_threads_cache(&self.threads);
                        self.persist_async();
                    }
                }
                BgEvent::PollFailed(err) => {
                    self.poll_inflight = false;
                    self.status = format!("Lỗi cập nhật: {err}");
                }
                BgEvent::ThreadsRefreshed(list) => {
                    let n = list.len();
                    self.apply_refresh(list);
                    self.status = format!("Đã làm mới ({n} hội thoại)");
                }
                BgEvent::HistoryLoaded { thread_id, resp } => {
                    let mut count = 0usize;
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        chat::merge_group_history(t, &resp);
                        count = t.messages.len();
                    }
                    if count > 0 {
                        self.persist_thread_by_id(&thread_id);
                        chat::save_threads_cache(&self.threads);
                        self.selected_msg = None;
                        self.follow_tail = true;
                        self.status = format!("Đã tải lịch sử nhóm ({count} tin)");
                    } else {
                        self.status = "Không có tin nhắn mới từ nhóm".into();
                    }
                }
                BgEvent::MessageSent { thread_id, text } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.preview = text;
                    }
                    self.bump_thread(&thread_id);
                    self.status = "Đã gửi tin nhắn".into();
                }
                BgEvent::AttachmentSent { thread_id, msg } => {
                    let mut appended = false;
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        if !t.messages.iter().any(|m| m.id == msg.id) {
                            t.messages.push(msg.clone());
                            appended = true;
                        }
                        t.preview = if msg.text.is_empty() {
                            "[Tệp đính kèm]".into()
                        } else {
                            msg.text.clone()
                        };
                        t.time = msg.time.clone();
                    }
                    if appended {
                        self.bump_thread(&thread_id);
                        self.persist_thread_by_id(&thread_id);
                        chat::save_threads_cache(&self.threads);
                        self.follow_tail = true;
                        self.status = "Đã gửi tệp đính kèm".into();
                    }
                }
                BgEvent::StickerSent { thread_id } => {
                    self.bump_thread(&thread_id);
                    self.status = "Đã gửi nhãn dán".into();
                }
                BgEvent::ReactionDone {
                    thread_id,
                    msg_id,
                    icon,
                } => {
                    if let Some(m) = self
                        .threads
                        .iter_mut()
                        .find(|t| t.id == thread_id)
                        .and_then(|t| t.messages.iter_mut().find(|m| m.id == msg_id))
                    {
                        let code = chat::reaction_code_and_type(&icon).0;
                        if !m.reactions.iter().any(|r| r == code) {
                            m.reactions.push(code.to_string());
                        }
                    }
                    self.status = "Đã thả cảm xúc".into();
                }
                BgEvent::DeleteDone { thread_id, msg_id } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.messages.retain(|m| m.id != msg_id);
                    }
                    self.selected_msg = None;
                    self.persist_thread_by_id(&thread_id);
                    chat::save_threads_cache(&self.threads);
                    self.status = "Đã xoá tin nhắn".into();
                }
                BgEvent::StickerPacks(packs) => {
                    self.sticker.loading = false;
                    self.sticker.items.clear();
                    self.sticker.packs = packs;
                    self.sticker.pack_idx = 0;
                    self.sticker.item_idx = 0;
                    self.sticker.stage = 0;
                    self.status = format!("Kho nhãn dán: {} bộ", self.sticker.packs.len());
                }
                BgEvent::ConsoleLine { thread_id, text } => {
                    self.busy = false;
                    let now = crate::ui::clock();
                    let ts = unix_now_ms();
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.messages.push(ChatMessage {
                            id: format!("local_console_{ts}"),
                            sender: "Console".into(),
                            text,
                            time: now.clone(),
                            is_outgoing: false,
                            ts,
                            ..Default::default()
                        });
                        t.preview = "Console output".into();
                        t.time = now;
                    }
                    self.selected_msg = None;
                    self.follow_tail = true;
                    self.persist_thread_by_id(&thread_id);
                    self.bump_thread(&thread_id);
                    self.status = "Lệnh console xong".into();
                }
                BgEvent::StickerItems(items) => {
                    self.sticker.loading = false;
                    let n = items.len();
                    self.sticker.items = items;
                    self.sticker.item_idx = 0;
                    self.sticker.stage = 1;
                    self.status = if n == 0 {
                        "Bộ nhãn dán trống".into()
                    } else {
                        format!("Bộ nhãn dán: {n} sticker")
                    };
                }
                BgEvent::UpdateAvailable(info) => {
                    self.status = format!(
                        "🚀 Có bản cập nhật mới v{} trên GitHub (hiện tại: v{}) — chạy --check-update hoặc mở GUI để cập nhật",
                        info.latest_version, info.current_version
                    );
                    crate::updater::notify_os_new_update(
                        &info.latest_version,
                        &info.current_version,
                        &info.release_title,
                    );
                }
                BgEvent::SendFailed(err) => {
                    self.status = format!("Gửi tin nhắn thất bại: {err}");
                }
                BgEvent::ActionFailed(err) => {
                    self.status = err;
                }
            }
        }
    }
}

pub async fn run_tui(mut app: TuiApp<'_>) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp<'_>,
) -> Result<()> {
    {
        let tx_upd = app.bg_tx.clone();
        tokio::spawn(async move {
            if let Ok(info) = crate::gui::check_github_release_cli().await {
                if info.has_update {
                    let _ = tx_upd.send(BgEvent::UpdateAvailable(info));
                }
            }
        });
    }
    let mut last_poll = std::time::Instant::now();

    while !app.should_quit {
        app.check_background_events();
        terminal.draw(|f| ui_draw(f, app))?;

        if event::poll(Duration::from_millis(30))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.handle_key(key.code, key.modifiers);
                }
                Event::Mouse(m) => app.handle_mouse(m),
                _ => {}
            }
        }

        if last_poll.elapsed() >= Duration::from_secs(POLL_SECS) {
            last_poll = std::time::Instant::now();
            app.spawn_poll();
        }
    }
    Ok(())
}

/// Animated boot sequence (omp-style): gradient logo sweep + step checklist.
/// Total ~1.4s, skipped on keypress (run_loop clears `boot_done` on input).
fn render_splash(f: &mut Frame, area: Rect, app: &TuiApp) {
    f.render_widget(Clear, area);
    let elapsed = app.boot_at.elapsed();
    let ms = elapsed.as_millis() as u64;
    // Steps light up progressively: session → threads → history → ready.
    let steps = [
        (0, "Phiên đăng nhập"),
        (250, "Danh sách hội thoại"),
        (600, "Lịch sử tin nhắn"),
        (1000, "Sẵn sàng"),
    ];
    let cx = area.width / 2;
    let cy = area.height / 2;
    // Logo with traveling multi-stop gradient + hot highlight band.
    let sweep = (ms / 50) as usize;
    let nstops = theme::SWEEP.len();
    let lines: Vec<Line> = SPLASH_LOGO
        .iter()
        .enumerate()
        .map(|(li, s)| {
            let spans: Vec<Span> = s
                .chars()
                .enumerate()
                .map(|(ci, ch)| {
                    let pos = ci + li * 6 + sweep;
                    let band = pos % 36;
                    // base gradient cycles through stops
                    let stop = (pos / 6) % nstops;
                    let mut style = Style::default()
                        .fg(theme::SWEEP[stop])
                        .add_modifier(Modifier::BOLD);
                    // hot white band with cyan glow around it
                    if (10..16).contains(&band) {
                        style = Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD);
                    } else if band == 9 || band == 16 {
                        style = Style::default()
                            .fg(Color::Rgb(34, 211, 238))
                            .add_modifier(Modifier::BOLD);
                    }
                    Span::styled(ch.to_string(), style)
                })
                .collect();
            // Center: pad left so the block sits mid-screen.
            let pad = cx.saturating_sub((s.chars().count() / 2) as u16) as usize;
            let mut all = vec![Span::raw(" ".repeat(pad))];
            all.extend(spans);
            Line::from(all)
        })
        .collect();
    // Checklist under the logo.
    let mut rows: Vec<Line> = lines;
    rows.push(Line::raw(""));
    for (at, label) in steps {
        let (icon, style) = if ms >= at + 200 {
            ("✓", Style::default().fg(theme::OK))
        } else if ms >= at {
            (spinner_frame(), Style::default().fg(theme::ACCENT))
        } else {
            ("·", Style::default().fg(theme::MUTED))
        };
        rows.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("{icon} "), style),
            Span::styled(
                label,
                if ms >= at + 200 {
                    Style::default().fg(theme::TEXT)
                } else {
                    Style::default().fg(theme::MUTED)
                },
            ),
        ]));
    }
    // Progress bar.
    let total_ms: u64 = 1400;
    let frac = (ms.min(total_ms) as f32 / total_ms as f32).clamp(0.0, 1.0);
    let bar_w = 40.min(area.width.saturating_sub(10) as usize);
    let filled = (frac * bar_w as f32).round() as usize;
    rows.push(Line::raw(""));
    rows.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("{}{}", "━".repeat(filled), "─".repeat(bar_w - filled)),
            Style::default().fg(theme::ACCENT),
        ),
    ]));
    // Typewriter tagline.
    let tag = "native zalo client · rust · no electron";
    let nch = ((ms.saturating_sub(200) / 28) as usize).min(tag.chars().count());
    let shown: String = tag.chars().take(nch).collect();
    let caret = if nch < tag.chars().count() { "▊" } else { "" };
    rows.push(Line::raw(""));
    rows.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(shown, Style::default().fg(theme::MUTED).add_modifier(Modifier::ITALIC)),
        Span::styled(caret, Style::default().fg(theme::ACCENT)),
    ]));
    let h = (rows.len() + 2) as u16;
    let w = 56.min(area.width.saturating_sub(4));
    let popup = Rect {
        x: cx.saturating_sub(w / 2),
        y: cy.saturating_sub(h / 2),
        width: w,
        height: h,
    };
    f.render_widget(Clear, popup);
    let para = Paragraph::new(rows)
        .block(
            Block::default()
                .title(format!(" {} ctdozlite ", spinner_frame()))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::ACCENT)),
        )
        .alignment(Alignment::Left);
    f.render_widget(para, popup);
}
/// Current spinner frame from wall clock (10fps braille cycle).
fn spinner_frame() -> &'static str {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    SPINNER_FRAMES[((ms / 100) % SPINNER_FRAMES.len() as u128) as usize]
}

fn ui_draw(f: &mut Frame, app: &mut TuiApp<'_>) {
    let size = f.area();
    // Animated boot overlay (~1.4s, any key skips).
    if !app.boot_done {
        if app.boot_at.elapsed() < std::time::Duration::from_millis(1400) {
            render_splash(f, size, app);
            return;
        } else {
            app.boot_done = true;
        }
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(size);

    // 1. Header bar
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
        .unwrap_or_else(|| "Chưa đăng nhập".into());

    let mode_span = if logger::is_dev_mode() {
        Span::styled(
            " [DEV] ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " [PROD] ",
            Style::default().fg(Color::White).bg(Color::Green),
        )
    };

    let total_unread: usize = app.unread.values().sum();
    let mut header_spans = vec![
        Span::styled(
            " ctdozlite ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("| Tài khoản: "),
        Span::styled(
            user_name,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | Chưa đọc: "),
        Span::styled(
            total_unread.to_string(),
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled(&app.status, Style::default().fg(Color::Yellow)),
        Span::raw(" "),
        mode_span,
    ];
    if app.poll_inflight || app.busy {
        header_spans.push(Span::styled(
            format!(" [{} đang cập nhật] ", spinner_frame()),
            Style::default().fg(Color::Yellow),
        ));
    }

    let header = Paragraph::new(Line::from(header_spans))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .alignment(Alignment::Left);
    f.render_widget(header, chunks[0]);

    // 2. Body: thread list | chat area
    let (side_pct, chat_pct) = match app.sidebar {
        SidebarMode::Normal => (30, 70),
        SidebarMode::Wide => (50, 50),
        SidebarMode::Hidden => (0, 100),
    };
    let body_chunks = if app.sidebar == SidebarMode::Hidden {
        // Hidden: chat takes everything; keep a zero-width slice for index compat.
        let full = chunks[1];
        let empty = Rect { width: 0, ..full };
        vec![empty, full]
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(side_pct),
                Constraint::Percentage(chat_pct),
            ])
            .split(chunks[1])
            .to_vec()
    };
    app.threads_width = body_chunks[0].width;

    let selected_pos = app.list_state.selected();
    let thread_items: Vec<ListItem> = app
        .filtered
        .iter()
        .enumerate()
        .map(|(pos, &i)| {
            let t = &app.threads[i];
            let is_sel = Some(pos) == selected_pos;
            let tag = if t.name.contains("Cloud") {
                "[Cloud] "
            } else if t.is_group {
                "[Group] "
            } else {
                "[User]  "
            };
            let cursor = if is_sel { "▶ " } else { "  " };
            let name_style = if is_sel {
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT)
            };
            let preview_len = if is_sel { 30 } else { 22 };
            let preview_text = if t.preview.is_empty() {
                "- chưa có tin nhắn -".to_string()
            } else {
                let p: String = t.preview.chars().take(preview_len).collect();
                format!("{p}...")
            };
            let mut line1 = vec![
                Span::raw(cursor),
                Span::styled(tag, Style::default().fg(theme::MUTED)),
                Span::styled(t.name.clone(), name_style),
                Span::raw("  "),
                Span::styled(t.time.clone(), Style::default().fg(theme::MUTED)),
            ];
            if let Some(n) = app.unread.get(&t.id).copied().filter(|n| *n > 0) {
                line1.push(Span::raw(" "));
                line1.push(Span::styled(
                    format!("({n})"),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::LightRed)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            let line2 = Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    preview_text,
                    if is_sel {
                        Style::default().fg(theme::TEXT)
                    } else {
                        Style::default().fg(theme::MUTED)
                    },
                ),
            ]);
            ListItem::new(vec![Line::from(line1), line2])
        })
        .collect();

    let threads_title = if app.search_mode {
        format!(" Tìm: {}_ ", app.search)
    } else if !app.search.is_empty() {
        format!(
            " Hội thoại {}/{} [{}] tìm '{}' ",
            app.filtered.len(),
            app.threads.len(),
            app.filter.label(),
            app.search
        )
    } else if app.focus == Focus::Threads {
        format!(
            " Hội thoại {}/{} [{}] · Ctrl+B: {} ",
            app.filtered.len(),
            app.threads.len(),
            app.filter.label(),
            app.sidebar.label()
        )
    } else {
        format!(
            " Hội thoại {}/{} [{}] ",
            app.filtered.len(),
            app.threads.len(),
            app.filter.label()
        )
    };

    let threads_focused = app.focus == Focus::Threads && app.sidebar != SidebarMode::Hidden;
    let threads_border_style = if threads_focused {
        Style::default()
            .fg(theme::ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::BORDER)
    };

    let threads_list = List::new(thread_items)
        .block(
            Block::default()
                .title(threads_title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(threads_border_style),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(25, 40, 65))
                .add_modifier(Modifier::BOLD),
        );
    if app.sidebar != SidebarMode::Hidden {
        f.render_stateful_widget(threads_list, body_chunks[0], &mut app.list_state);
    }

    // 2b. Right column: contact header, messages, reply bar, input
    let reply_h = if app.reply_target.is_some() { 2 } else { 0 };
    let chat_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(reply_h),
            Constraint::Length(3),
        ])
        .split(body_chunks[1]);

    let (name, id, is_group, msg_count) = match app.threads.get(app.selected_thread) {
        Some(t) => (t.name.clone(), t.id.clone(), t.is_group, t.messages.len()),
        None => ("Chưa chọn hội thoại".to_string(), String::new(), false, 0),
    };
    let kind = if is_group { "Nhóm" } else { "Cá nhân" };
    let chat_header = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" {name} "),
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("[{kind}] "), Style::default().fg(Color::DarkGray)),
        Span::raw("| ID: "),
        Span::styled(id, Style::default().fg(Color::DarkGray)),
        Span::raw(format!(" | {msg_count} tin | ")),
        if app.follow_tail {
            Span::styled("theo dõi cuối", Style::default().fg(Color::Green))
        } else {
            Span::styled("đang xem lịch sử", Style::default().fg(Color::Yellow))
        },
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(chat_header, chat_chunks[0]);

    if app.focus == Focus::Messages {
        app.ensure_msg_selection();
    }

    let inner_w = chat_chunks[1].width.saturating_sub(2) as usize;
    let inner_h = chat_chunks[1].height.saturating_sub(2) as usize;
    let highlight = app.focus == Focus::Messages;
    let (lines, ranges) = match app.threads.get(app.selected_thread) {
        Some(t) => {
            let hl: std::collections::HashSet<usize> =
                app.find_hits.iter().copied().collect();
            build_chat_lines(t, inner_w, app.selected_msg, highlight, &hl)
        }
        None => (
            vec![Line::from(Span::styled(
                "  Chọn một hội thoại để bắt đầu.",
                Style::default().fg(Color::DarkGray),
            ))],
            Vec::new(),
        ),
    };
    let max_scroll = lines.len().saturating_sub(inner_h);
    if app.follow_tail {
        app.msg_scroll = max_scroll;
    } else {
        app.msg_scroll = app.msg_scroll.min(max_scroll);
        if app.msg_scroll >= max_scroll {
            app.follow_tail = true;
        }
    }
    app.msg_ranges = ranges;
    app.chat_view_h = inner_h;

    let pct = if lines.len() <= inner_h {
        format!("100%")
    } else {
        format!(
            "{}%",
            ((app.msg_scroll + inner_h) * 100 / lines.len().max(1)).min(100)
        )
    };

    let chat_border_style = if app.focus == Focus::Messages {
        Style::default()
            .fg(theme::ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::BORDER)
    };
    let chat_body = Paragraph::new(lines)
        .block(
            Block::default()
                .title(if app.find_query.is_empty() {
                    format!(" Nội dung tin nhắn [{pct}] ")
                } else {
                    format!(
                        " Tìm '{}' ({}/{}) [{pct}] ",
                        app.find_query,
                        if app.find_hits.is_empty() { 0 } else { app.find_pos + 1 },
                        app.find_hits.len()
                    )
                })
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(chat_border_style),
        )
        .scroll(((app.msg_scroll.min(u16::MAX as usize)) as u16, 0));
    f.render_widget(chat_body, chat_chunks[1]);

    if let Some(q) = &app.reply_target {
        let quote = format!(
            " {}: {}",
            q.sender,
            q.text
                .replace('\n', " ")
                .chars()
                .take(120)
                .collect::<String>()
        );
        let reply = Paragraph::new(quote)
            .block(
                Block::default()
                    .title(" Đang trả lời [Ctrl+X huỷ] ")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Magenta)),
            )
            .style(Style::default().fg(Color::Gray));
        f.render_widget(reply, chat_chunks[2]);
    }

    let input_title = if app.focus == Focus::Input {
        " Soạn tin nhắn: [ĐANG GÕ] (Enter: Gửi | Esc: Lùi | Ctrl+X: Huỷ trả lời) "
    } else {
        " Soạn tin nhắn: [Tab để nhập] "
    };
    let input_border_style = if app.focus == Focus::Input {
        Style::default()
            .fg(theme::ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::BORDER)
    };
    let cursor = if app.focus == Focus::Input { "_" } else { "" };
    // Slash-command popup: floating list right above the input box.
    {
        let items = app.slash_popup();
        if !items.is_empty() {
            let h = (items.len() as u16 + 2).min(chat_chunks[3].y.saturating_sub(1).max(4));
            let area = Rect {
                x: chat_chunks[3].x,
                y: chat_chunks[3].y.saturating_sub(h),
                width: chat_chunks[3].width,
                height: h,
            };
            f.render_widget(Clear, area);
            let rows: Vec<ListItem> = items
                .iter()
                .enumerate()
                .map(|(i, (cmd, desc))| {
                    let style = if i == app.slash_sel.min(items.len() - 1) {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!(" {cmd} "), style),
                        Span::styled(format!(" {desc}"), Style::default().fg(Color::DarkGray)),
                    ]))
                })
                .collect();
            let popup = List::new(rows).block(
                Block::default()
                    .title(" Lệnh (↑↓ chọn · Tab/Enter chèn · Esc đóng) ")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Cyan)),
            );
            f.render_widget(popup, area);
        }
    }

    let input_widget = Paragraph::new(format!(" {}{cursor}", app.input_text)).block(
        Block::default()
            .title(input_title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(input_border_style),
    );
    f.render_widget(input_widget, chat_chunks[3]);

    // 3. Footer: single context line (F1 opens the full help overlay).
    let hint = match app.focus {
        Focus::Threads => "[↑↓] chọn · [Enter] xem · [/] tìm · [r] mới · [Ctrl+B] panel · [F1] phím tắt · [q] thoát",
        Focus::Messages => "[↑↓] tin · [Enter] soạn · [Ctrl+F] tìm · [y] trả lời · [Ctrl+B] panel · [F1] phím tắt",
        Focus::Input => "[Enter] gửi · [Esc] lùi · [Tab] vùng · gõ / xem lệnh console",
    };
    let footer_line = if app.search_mode {
        Line::from(vec![
            Span::styled("Tìm: ", Style::default().fg(theme::ACCENT)),
            Span::styled(
                format!("{}_", app.search),
                Style::default().fg(theme::TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" (Enter xong · Esc xoá)", Style::default().fg(theme::MUTED)),
        ])
    } else if app.find_mode {
        Line::from(vec![
            Span::styled("Tìm trong chat: ", Style::default().fg(theme::ACCENT)),
            Span::styled(
                format!("{}_{}", app.find_query, ""),
                Style::default().fg(theme::TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" (n/N nhảy · Esc thoát)", Style::default().fg(theme::MUTED)),
        ])
    } else {
        Line::from(vec![
            Span::styled(hint, Style::default().fg(theme::MUTED)),
            Span::raw(" "),
        ])
    };
    // Prepend model indicator: `🤖 source/model`.
    let footer_line = {
        let mut spans = vec![
            Span::styled("🤖 ", Style::default().fg(theme::ACCENT)),
            Span::styled(app.model_label().to_string(), Style::default().fg(theme::TEXT)),
            Span::styled(" · ", Style::default().fg(theme::MUTED)),
        ];
        if let Line { spans: inner, .. } = footer_line {
            spans.extend(inner);
        }
        Line::from(spans)
    };
    let footer = Paragraph::new(footer_line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::BORDER)),
    );
    f.render_widget(footer, chunks[2]);

    // 4. Overlays
    match app.overlay {
        Overlay::Help => render_help(f, size),
        Overlay::Log => render_log_modal(f, size, app.log_scroll),
        Overlay::Sticker => render_sticker_overlay(f, size, app),
        Overlay::None => {}
    }
    if let Some(action) = &app.confirm {
        render_confirm(f, size, action);
    }
    if let Some(prompt) = &app.prompt {
        render_prompt(f, size, prompt);
    }
}

/// Split text into plain/link segments. Matches http(s):// and www. runs.
fn split_links(s: &str) -> Vec<(bool, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        let mut found: Option<(usize, usize)> = None;
        for pat in ["https://", "http://", "www."] {
            if let Some(i) = rest.find(pat) {
                match found {
                    Some((fi, _)) if fi <= i => {}
                    _ => {
                        // URL ends at whitespace
                        let tail = &rest[i..];
                        let len = tail.find(char::is_whitespace).unwrap_or(tail.len());
                        found = Some((i, i + len));
                    }
                }
            }
        }
        match found {
            Some((a, b)) => {
                if a > 0 {
                    out.push((false, rest[..a].to_string()));
                }
                out.push((true, rest[a..b].to_string()));
                rest = &rest[b..];
            }
            None => {
                out.push((false, rest.to_string()));
                break;
            }
        }
    }
    out
}

/// OSC8 hyperlink escape (Ctrl+click in iTerm2/Terminal/WezTerm/VSCode).
fn osc8(url: &str, label: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{label}\x1b]8;;\x1b\\")
}

/// Extra attachment lines under the body (icon + label, OSC8 when URL exists).
fn attachment_lines(m: &crate::chat::ChatMessage) -> Vec<(String, Option<String>)> {
    let mut v = Vec::new();
    let title = m.link_title.clone().unwrap_or_default();
    let fname = if !title.trim().is_empty() {
        title.trim().to_string()
    } else if m.text.trim().starts_with("[Hình ảnh") || m.text.trim() == "[Hình ảnh]" {
        String::new()
    } else {
        m.text.trim().chars().take(60).collect()
    };
    let has_url = m.link_url.as_deref().map(|u| !u.is_empty()).unwrap_or(false);
    let url = m.link_url.clone().filter(|u| !u.is_empty());
    let is_img = m.text.trim().starts_with("[Hình ảnh")
        || m.link_src.as_deref() == Some("Hình ảnh Zalo");
    if m.sticker_id.unwrap_or(0) > 0 {
        v.push((format!("🙂 Nhãn dán #{}", m.sticker_id.unwrap_or(0)), None));
    } else if is_img {
        let label = if fname.is_empty() { "Ảnh".into() } else { format!("Ảnh · {fname}") };
        v.push((format!("🖼 {label}  [o mở]"), url));
    } else if m.text.trim().starts_with("[Video") || m.text.trim().starts_with("[video") {
        v.push((format!("🎬 Video{}  [o mở]", if fname.is_empty() { String::new() } else { format!(" · {fname}") }), url));
    } else if m.text.trim().starts_with("[File") || m.text.trim().starts_with("[Tài liệu") {
        v.push((format!("📎 Tệp{}  [o mở]", if fname.is_empty() { String::new() } else { format!(" · {fname}") }), url));
    } else if has_url && !m.link_url.as_deref().unwrap_or("").contains("zdn.vn") {
        // generic link attachment (non-image CDN): show title + url line
        let label = if fname.is_empty() { "Liên kết".into() } else { fname };
        v.push((format!("🔗 {label}"), url));
    }
    let _ = has_url;
    v
}

fn build_chat_lines(
    thread: &Thread,
    width: usize,
    selected: Option<usize>,
    highlight: bool,
    find_hl: &std::collections::HashSet<usize>,
) -> (Vec<Line<'static>>, Vec<(usize, usize)>) {
    let width = width.max(20);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut ranges: Vec<(usize, usize)> = Vec::new();

    if thread.messages.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            format!("  Hội thoại '{}' đã sẵn sàng.", thread.name),
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            "  Bấm Tab để soạn tin nhắn, Enter để gửi.",
            Style::default().fg(Color::Cyan),
        )));
        return (lines, ranges);
    }

    let mut last_day: Option<i64> = None;
    for (idx, m) in thread.messages.iter().enumerate() {
        if let Some(day) = message_day(m.ts) {
            if last_day != Some(day) {
                if !lines.is_empty() {
                    lines.push(Line::raw(""));
                }
                lines.push(Line::from(Span::styled(
                    format!("--- {} ---", day_label(day)),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                )));
                last_day = Some(day);
            }
        }

        let start = lines.len();
        let sel = highlight && selected == Some(idx);

        let sender_name = if m.is_outgoing {
            "Tôi".to_string()
        } else if !m.sender.trim().is_empty() {
            m.sender.clone()
        } else if thread.is_group {
            "Thành viên".to_string()
        } else {
            thread.name.clone()
        };
        let time_disp = if m.time.is_empty() {
            "--:--".to_string()
        } else {
            m.time.clone()
        };
        let pin_part = if m.is_pinned { "* " } else { "" };
        let time_part = format!("[{time_disp}] ");
        let name_part = format!("{sender_name}: ");
        let prefix_len =
            pin_part.chars().count() + time_part.chars().count() + name_part.chars().count();

        let mut prefix: Vec<Span<'static>> = Vec::new();
        if m.is_pinned {
            prefix.push(Span::styled("* ", Style::default().fg(Color::Yellow)));
        }
        prefix.push(Span::styled(
            time_part,
            Style::default().fg(Color::DarkGray),
        ));
        let sender_style = if m.is_outgoing {
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme::sender_color(&sender_name))
                .add_modifier(Modifier::BOLD)
        };
        prefix.push(Span::styled(name_part, sender_style));

        if let (Some(qs), Some(qt)) = (&m.reply_to_sender, &m.reply_to_text) {
            let quote = format!("| {}: {}", qs, qt.replace('\n', " "));
            for chunk in wrap_text(&quote, width.saturating_sub(prefix_len).max(16)) {
                lines.push(Line::from(vec![
                    Span::styled(" ".repeat(prefix_len), Style::default()),
                    Span::styled(
                        chunk,
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ]));
            }
        }

        let mut body = if m.sticker_id.unwrap_or(0) > 0 {
            format!("[Nhãn dán #{}]", m.sticker_id.unwrap_or(0))
        } else {
            m.text.clone()
        };
        if let Some(url) = &m.link_url {
            if !url.is_empty() && !body.contains(url.as_str()) {
                if !body.is_empty() {
                    body.push(' ');
                }
                body.push_str(url);
            }
        }
        if !m.reactions.is_empty() {
            let icons: Vec<&str> = m.reactions.iter().map(|r| chat::icon_to_emoji(r)).collect();
            body.push_str("  ");
            body.push_str(&icons.join(" "));
        }
        if body.trim().is_empty() {
            body = "(không có nội dung)".to_string();
        }

        let avail = width.saturating_sub(prefix_len).max(10);
        let is_hit = find_hl.contains(&idx);
        let base_style = if is_hit {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT)
        };
        let link_style = if is_hit {
            base_style
        } else {
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::UNDERLINED)
        };
        // Body as linkified spans (wrap per segment to keep prefix alignment).
        let mut first = true;
        for (is_link, seg) in split_links(&body) {
            let style = if is_link { link_style } else { base_style };
            // OSC8 makes Ctrl+click work in modern terminals; plain text stays readable.
            let show = if is_link { osc8(&seg, &seg) } else { seg.clone() };
            for chunk in wrap_text(&show, avail) {
                if first {
                    let mut spans = prefix.clone();
                    // NOTE: OSC8 escape inside a wrapped chunk may split oddly on
                    // narrow widths; acceptable for a 1-line link preview.
                    spans.push(Span::styled(chunk.clone(), style));
                    lines.push(Line::from(spans));
                    first = false;
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(" ".repeat(prefix_len), Style::default()),
                        Span::styled(chunk.clone(), style),
                    ]));
                }
            }
        }
        // Attachment card lines (icon + label, `[o]` opens via open_selected_link).
        for (label, url) in attachment_lines(m) {
            let shown = match url {
                Some(u) => osc8(&u, &label),
                None => label,
            };
            lines.push(Line::from(vec![
                Span::styled(" ".repeat(prefix_len), Style::default()),
                Span::styled(shown, Style::default().fg(theme::ACCENT)),
            ]));
        }

        if sel {
            // Selected message pops: bright bg + bold white body.
            let bg = Style::default().bg(Color::Rgb(35, 48, 70));
            for (li, line) in lines.iter_mut().enumerate().skip(start) {
                *line = line.clone().style(bg);
                if li == start {
                    // First line: rebuild with bold bright sender prefix.
                    let mut spans = Vec::new();
                    for s in line.spans.iter() {
                        spans.push(Span::styled(
                            s.content.clone(),
                            s.style
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        ));
                    }
                    *line = Line::from(spans);
                }
            }
        }
        ranges.push((start, lines.len()));
    }

    (lines, ranges)
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(4);
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0usize;

    for word in text.split_whitespace() {
        let wl = word.chars().count();
        if wl > width {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                cur_len = 0;
            }
            let mut buf = String::new();
            let mut n = 0usize;
            for ch in word.chars() {
                buf.push(ch);
                n += 1;
                if n == width {
                    out.push(std::mem::take(&mut buf));
                    n = 0;
                }
            }
            if !buf.is_empty() {
                cur = buf;
                cur_len = n;
            }
            continue;
        }
        if cur_len > 0 && cur_len + 1 + wl > width {
            out.push(std::mem::take(&mut cur));
            cur_len = 0;
        }
        if cur_len > 0 {
            cur.push(' ');
            cur_len += 1;
        }
        cur.push_str(word);
        cur_len += wl;
    }

    if !cur.is_empty() {
        out.push(cur);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn normalize_ts_ms(ts: i64) -> i64 {
    if ts <= 0 {
        0
    } else if ts < 10_000_000_000 {
        ts * 1000
    } else {
        ts
    }
}

fn message_day(ts: i64) -> Option<i64> {
    let ms = normalize_ts_ms(ts);
    if ms <= 0 {
        return None;
    }
    Some((ms / 1000 + 7 * 3600).div_euclid(86_400))
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = (y + if m <= 2 { 1 } else { 0 }) as i32;
    (year, m, d)
}

fn day_label(day: i64) -> String {
    let today = (crate::login::unix_now() + 7 * 3600).div_euclid(86_400);
    let (y, m, d) = civil_from_days(day);
    if day == today {
        format!("Hôm nay {d:02}/{m:02}/{y}")
    } else if day == today - 1 {
        format!("Hôm qua {d:02}/{m:02}/{y}")
    } else {
        format!("{d:02}/{m:02}/{y}")
    }
}

fn unix_now_ms() -> i64 {
    crate::login::unix_now() * 1000
}

fn open_external_url(url: &str) {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        #[link(name = "shell32")]
        extern "system" {
            fn ShellExecuteW(
                hwnd: *mut std::ffi::c_void,
                lp_operation: *const u16,
                lp_file: *const u16,
                lp_parameters: *const u16,
                lp_directory: *const u16,
                n_show_cmd: i32,
            ) -> isize;
        }
        let op: Vec<u16> = "open\0".encode_utf16().collect();
        let mut file: Vec<u16> = trimmed.encode_utf16().collect();
        file.push(0);
        unsafe {
            let _ = ShellExecuteW(
                std::ptr::null_mut(),
                op.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                5,
            );
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(trimmed).spawn();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(trimmed).spawn();
    }
}

fn help_line(keys: &str, desc: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {keys:<14}"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(desc.to_string(), Style::default().fg(Color::White)),
    ])
}

fn render_help(f: &mut Frame, area: Rect) {
    let modal_area = centered_rect(80, 80, area);
    f.render_widget(Clear, modal_area);

    let mut content: Vec<Line<'static>> = Vec::new();
    content.push(Line::from(Span::styled(
        "=== PHÍM TẮT ZALO TUI ===",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    content.push(Line::raw(""));
    content.push(help_line(
        "Tab / BackTab",
        "Chuyển vùng: Danh sách -> Tin nhắn -> Soạn tin",
    ));
    content.push(help_line("F1 / ?", "Mở trợ giúp này"));
    content.push(help_line("q / Ctrl+C", "Thoát (q chỉ khi không gõ tin)"));
    content.push(Line::raw(""));
    content.push(Line::from(Span::styled(
        "-- Danh sách hội thoại --",
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )));
    content.push(help_line(
        "Up/Down j/k",
        "Chọn hội thoại (cuộn chuột cũng được)",
    ));
    content.push(help_line("PgUp/PgDn", "Nhảy 10 hội thoại"));
    content.push(help_line("Home / End", "Hội thoại đầu / cuối"));
    content.push(help_line("/", "Tìm theo tên, ID, nội dung, tin nhắn"));
    content.push(help_line("f", "Lọc: Tất cả -> Cá nhân -> Nhóm"));
    content.push(help_line("u", "Xoá tất cả thông báo chưa đọc"));
    content.push(help_line("r", "Làm mới danh sách từ server (nên)"));
    content.push(help_line("h", "Tải lịch sử nhóm (50 tin)"));
    content.push(help_line("x", "Đăng xuất (có xác nhận)"));
    content.push(help_line("w", "Xoá lịch sử tin nhắn cục bộ (có xác nhận)"));
    content.push(Line::raw(""));
    content.push(Line::from(Span::styled(
        "-- Tin nhắn --",
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )));
    content.push(help_line("Up/Down j/k", "Chọn tin nhắn"));
    content.push(help_line("PgUp/PgDn", "Cuộn nhanh"));
    content.push(help_line("Home / End", "Đầu / cuối hội thoại"));
    content.push(help_line(
        "1 .. 6",
        "Thả cảm xúc nhanh (Thích, Yêu, Haha, Wow, Buồn, Phẫn nộ)",
    ));
    content.push(help_line("y", "Trả lời tin đang chọn"));
    content.push(help_line("d", "Xoá tin nhắn (chỉ phía bạn)"));
    content.push(help_line("o", "Mở liên kết của tin nhắn"));
    content.push(Line::raw(""));
    content.push(Line::from(Span::styled(
        "-- Soạn tin / đính kèm --",
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )));
    content.push(help_line("Enter", "Gửi tin nhắn"));
    content.push(help_line("Ctrl+U", "Xoá toàn bộ ô soạn"));
    content.push(help_line("Ctrl+X", "Huỷ trả lời"));
    content.push(help_line("Esc", "Lùi về vùng trước"));
    content.push(help_line(
        "F2",
        "Kho nhãn dán: Enter chọn bộ, Enter lần 2 để gửi",
    ));
    content.push(help_line("F3", "Gửi tệp đính kèm bằng đường dẫn"));
    content.push(help_line("l", "Dev log inspector (chỉ chế độ --dev)"));
    content.push(Line::raw(""));
    content.push(Line::from(Span::styled(
        "Esc hoặc F1 để đóng trợ giúp.",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .title(" Trợ giúp (Esc/F1 để đóng) ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));
    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: true });
    f.render_widget(paragraph, modal_area);
}

fn render_sticker_overlay(f: &mut Frame, area: Rect, app: &TuiApp<'_>) {
    let modal_area = centered_rect(84, 80, area);
    f.render_widget(Clear, modal_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(modal_area);

    let loading = if app.sticker.loading {
        " (đang tải...)"
    } else {
        ""
    };
    let packs_title = if app.sticker.stage == 0 {
        format!(
            " Kho nhãn dán ({} bộ){loading} [ĐANG CHỌN] — Enter để mở bộ ",
            app.sticker.packs.len()
        )
    } else {
        format!(
            " Kho nhãn dán ({} bộ){loading} — Left/Right đổi cột ",
            app.sticker.packs.len()
        )
    };
    let pack_items: Vec<ListItem> = app
        .sticker
        .packs
        .iter()
        .map(|p| {
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", p.name), Style::default().fg(Color::White)),
                Span::styled(
                    format!("({} sticker)", p.total_image),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();
    let mut packs_state = ListState::default();
    if !app.sticker.packs.is_empty() {
        packs_state.select(Some(app.sticker.pack_idx.min(app.sticker.packs.len() - 1)));
    }
    let packs_border = if app.sticker.stage == 0 {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let packs_list = List::new(pack_items)
        .block(
            Block::default()
                .title(packs_title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(packs_border),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(25, 40, 65))
                .add_modifier(Modifier::BOLD),
        );
    f.render_stateful_widget(packs_list, chunks[0], &mut packs_state);

    let items_title = if app.sticker.items.is_empty() {
        format!(
            " Bộ '{}' — chọn một bộ bên trên rồi Enter ",
            app.sticker.loaded_name
        )
    } else if app.sticker.stage == 1 {
        format!(
            " Bộ '{}' ({} sticker) [ĐANG CHỌN] — Enter để gửi ",
            app.sticker.loaded_name,
            app.sticker.items.len()
        )
    } else {
        format!(
            " Bộ '{}' ({} sticker) ",
            app.sticker.loaded_name,
            app.sticker.items.len()
        )
    };
    let item_rows: Vec<ListItem> = app
        .sticker
        .items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let label = if it.label.trim().is_empty() {
                format!("Sticker #{}", it.sticker_id)
            } else {
                it.label.clone()
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {:>3}. ", i + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(label, Style::default().fg(Color::White)),
                Span::styled(
                    format!("  id={} cate={}", it.sticker_id, it.cate_id),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();
    let mut items_state = ListState::default();
    if !app.sticker.items.is_empty() {
        items_state.select(Some(app.sticker.item_idx.min(app.sticker.items.len() - 1)));
    }
    let items_border = if app.sticker.stage == 1 {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let items_list = List::new(item_rows)
        .block(
            Block::default()
                .title(items_title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(items_border),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(25, 40, 65))
                .add_modifier(Modifier::BOLD),
        );
    f.render_stateful_widget(items_list, chunks[1], &mut items_state);
}

fn render_confirm(f: &mut Frame, area: Rect, action: &ConfirmAction) {
    let modal_area = centered_rect(60, 26, area);
    f.render_widget(Clear, modal_area);

    let content = vec![
        Line::raw(""),
        Line::from(Span::styled(
            format!("  {}", action.question),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("  {}", action.detail),
            Style::default().fg(Color::White),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            "  [y/Enter] Xác nhận    [n/Esc] Huỷ",
            Style::default().fg(Color::Cyan),
        )),
    ];
    let paragraph = Paragraph::new(content).block(
        Block::default()
            .title(" Xác nhận ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Yellow)),
    );
    f.render_widget(paragraph, modal_area);
}

fn render_prompt(f: &mut Frame, area: Rect, prompt: &PromptState) {
    let modal_area = centered_rect(70, 24, area);
    f.render_widget(Clear, modal_area);

    let title = match prompt.kind {
        PromptKind::Attachment => " Gửi tệp đính kèm: nhập đường dẫn tệp ",
    };
    let content = vec![
        Line::raw(""),
        Line::from(Span::styled(
            format!("  {}_", prompt.text),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            "  [Enter] Gửi    [Esc] Huỷ",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let paragraph = Paragraph::new(content).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(paragraph, modal_area);
}

fn render_log_modal(f: &mut Frame, area: Rect, scroll: usize) {
    let modal_area = centered_rect(88, 80, area);
    f.render_widget(Clear, modal_area);

    let findings = logger::scan_logs();
    let entries = logger::get_entries();

    let mut content = Vec::new();

    content.push(Line::from(vec![Span::styled(
        "=== KẾT QUẢ QUÉT LOG & CHẨN ĐOÁN GIAO THỨC ===",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )]));
    for finding in &findings {
        let (icon, color) = match finding.level {
            FindingLevel::Info => ("[INFO] ", Color::Green),
            FindingLevel::Warning => ("[WARN] ", Color::Yellow),
            FindingLevel::Critical => ("[CRIT] ", Color::Red),
        };
        content.push(Line::from(vec![
            Span::styled(
                icon,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                finding.title.clone(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]));
        content.push(Line::from(vec![
            Span::raw("  Chỉ tiết: "),
            Span::styled(finding.detail.clone(), Style::default().fg(Color::White)),
        ]));
        content.push(Line::from(vec![
            Span::raw("  Gợi ý: "),
            Span::styled(finding.suggestion.clone(), Style::default().fg(Color::Cyan)),
        ]));
        content.push(Line::raw(""));
    }

    content.push(Line::from(vec![Span::styled(
        format!(
            "=== LƯU LƯỢNG MẠNG THỜI GIAN THỰC ({} requests) ===",
            entries.len()
        ),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )]));
    for e in entries.iter().rev() {
        let status_color = if e.is_error { Color::Red } else { Color::Green };
        let status_str = e
            .status
            .map(|s| s.to_string())
            .unwrap_or_else(|| "ERR".into());

        content.push(Line::from(vec![
            Span::styled(
                format!("[{}] ", e.timestamp),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{:<4} ", e.method),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<3} ", status_str),
                Style::default().fg(status_color),
            ),
            Span::styled(
                format!("({}ms) ", e.latency_ms),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(e.url.clone(), Style::default().fg(Color::White)),
        ]));
        if let Some(dec) = &e.decrypted_preview {
            let preview = dec.chars().take(80).collect::<String>();
            content.push(Line::from(vec![
                Span::raw("  + Decrypted: "),
                Span::styled(format!("{preview}..."), Style::default().fg(Color::Gray)),
            ]));
        }
        if let Some(err) = &e.error_msg {
            content.push(Line::from(vec![Span::styled(
                format!("  + Lỗi: {err}"),
                Style::default().fg(Color::Red),
            )]));
        }
    }

    let block = Block::default()
        .title(" Dev Inspector & Protocol Scanner (Esc/q để đóng, Up/Down để cuộn) ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Yellow));

    let p = Paragraph::new(content)
        .block(block)
        .scroll((scroll.min(u16::MAX as usize) as u16, 0))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_words_to_width() {
        assert_eq!(
            wrap_text("a b c", 3),
            vec!["a b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn breaks_long_words() {
        assert_eq!(
            wrap_text("abcdef", 4),
            vec!["abcd".to_string(), "ef".to_string()]
        );
    }

    #[test]
    fn message_day_skips_missing_ts() {
        assert_eq!(message_day(0), None);
        assert!(message_day(1_700_000_000_000).is_some());
    }

    #[test]
    fn draws_thread_and_messages_without_panicking() {
        let mut session = crate::session::Session::new(false, false, false).unwrap();
        let (tx, rx) = channel();
        let thread = Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: "u1".into(),
            name: "Test".into(),
            preview: "xin chao".into(),
            time: "12:00".into(),
            avatar: None,
            is_group: true,
            messages: vec![
                ChatMessage {
                    id: "m1".into(),
                    sender: "An".into(),
                    text: "xin chao".into(),
                    time: "12:00".into(),
                    is_outgoing: false,
                    ts: 1_700_000_000_000,
                    ..Default::default()
                },
                ChatMessage {
                    id: "m2".into(),
                    sender: "Tôi".into(),
                    text: "chao ban".into(),
                    time: "12:01".into(),
                    is_outgoing: true,
                    ts: 1_700_000_060_000,
                    reply_to_sender: Some("An".into()),
                    reply_to_text: Some("xin chao".into()),
                    reactions: vec!["/-strong".into()],
                    ..Default::default()
                },
            ],
            vanish: None,
        };
        let mut app = TuiApp {
            session: &mut session,
            threads: vec![thread],
            list_state: ListState::default(),
            selected_thread: 0,
            input_text: "hello".into(),
            focus: Focus::Messages,
            status: "test".into(),
            should_quit: false,
            store: None,
            poll_state: chat::PollState::default(),
            filtered: vec![0],
            filter: FilterTab::All,
            search: String::new(),
            search_mode: false,
            unread: HashMap::new(),
            selected_msg: Some(1),
            msg_scroll: 0,
            follow_tail: true,
            msg_ranges: Vec::new(),
            chat_view_h: 10,
            pinned_ids: TuiApp::load_pinned_ids(),
            find_query: String::new(),
            find_hits: Vec::new(),
            find_pos: 0,
            find_mode: false,
            slash_sel: 0,
            threads_width: 0,
            reply_target: None,
            overlay: Overlay::None,
            log_scroll: 0,
            sticker: StickerState::default(),
            prompt: None,
            confirm: None,
            poll_inflight: false,
            busy: false,
            boot_at: std::time::Instant::now(),
            boot_done: true,
            llm_label: None,
            llm_override: None,
            cached_models: None,
            sidebar: SidebarMode::default(),
            bg_rx: rx,
            bg_tx: tx,
        };
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| ui_draw(f, &mut app)).unwrap();
        assert!(app.msg_ranges.len() >= 2);
    }

    /// In-chat find: recompute hits, jump next/prev with wraparound, exit clears.
    #[test]
    fn find_hits_jump_wraps_and_exits() {
        let mut session = crate::session::Session::new(false, false, false).unwrap();
        let (tx, rx) = channel();
        let mkmsg = |sender: &str, text: &str| ChatMessage {
            id: format!("m_{text}"),
            sender: sender.into(),
            text: text.into(),
            time: "09:00".into(),
            is_outgoing: false,
            ts: 1_700_000_000_000,
            ..Default::default()
        };
        let thread = Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: "t1".into(),
            name: "Alpha".into(),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: false,
            messages: vec![
                mkmsg("An", "xin chao"),
                mkmsg("Tôi", "chao ban"),
                mkmsg("An", "bao gia san pham"),
            ],
            vanish: None,
        };
        let mut app = TuiApp {
            session: &mut session,
            threads: vec![thread],
            list_state: ListState::default(),
            selected_thread: 0,
            input_text: String::new(),
            focus: Focus::Messages,
            status: String::new(),
            should_quit: false,
            store: None,
            poll_state: chat::PollState::default(),
            filtered: vec![0],
            filter: FilterTab::All,
            search: String::new(),
            search_mode: false,
            unread: HashMap::new(),
            selected_msg: None,
            msg_scroll: 0,
            follow_tail: true,
            msg_ranges: Vec::new(),
            chat_view_h: 10,
            pinned_ids: TuiApp::load_pinned_ids(),
            find_query: "chao".into(),
            find_hits: Vec::new(),
            find_pos: 0,
            find_mode: true,
            slash_sel: 0,
            threads_width: 0,
            reply_target: None,
            overlay: Overlay::None,
            log_scroll: 0,
            sticker: StickerState::default(),
            prompt: None,
            confirm: None,
            poll_inflight: false,
            busy: false,
            boot_at: std::time::Instant::now(),
            boot_done: true,
            llm_label: None,
            llm_override: None,
            cached_models: None,
            sidebar: SidebarMode::default(),
            bg_rx: rx,
            bg_tx: tx,
        };
        app.recompute_find_hits();
        assert_eq!(app.find_hits, vec![0, 1]);
        app.jump_find(1);
        assert_eq!(app.find_pos, 1);
        assert_eq!(app.selected_msg, Some(1));
        app.jump_find(1); // wrap to first
        assert_eq!(app.find_pos, 0);
        app.jump_find(-1); // wrap to last
        assert_eq!(app.find_pos, 1);
        app.exit_find_mode();
        assert!(app.find_hits.is_empty() && app.find_query.is_empty() && !app.find_mode);
        // empty query clears hits
        app.find_query = String::new();
        app.recompute_find_hits();
        assert!(app.find_hits.is_empty());
    }

    /// Pinned threads stay on top in GUI pin order, then recency.
    #[test]
    fn sort_threads_pins_first() {
        let mut session = crate::session::Session::new(false, false, false).unwrap();
        let (tx, rx) = channel();
        let mk = |id: &str, ts: i64| Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: id.into(),
            name: id.into(),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: false,
            messages: if ts > 0 {
                vec![ChatMessage {
                    id: format!("m_{id}"),
                    sender: "A".into(),
                    text: "hi".into(),
                    time: String::new(),
                    is_outgoing: false,
                    ts,
                    ..Default::default()
                }]
            } else {
                Vec::new()
            },
            vanish: None,
        };
        let mut app = TuiApp {
            session: &mut session,
            threads: vec![mk("new", 200), mk("old", 100), mk("pinned-old", 50)],
            list_state: ListState::default(),
            selected_thread: 0,
            input_text: String::new(),
            focus: Focus::Threads,
            status: String::new(),
            should_quit: false,
            store: None,
            poll_state: chat::PollState::default(),
            filtered: vec![0, 1, 2],
            filter: FilterTab::All,
            search: String::new(),
            search_mode: false,
            unread: HashMap::new(),
            selected_msg: None,
            msg_scroll: 0,
            follow_tail: true,
            msg_ranges: Vec::new(),
            chat_view_h: 10,
            find_query: String::new(),
            find_hits: Vec::new(),
            find_pos: 0,
            find_mode: false,
            slash_sel: 0,
            threads_width: 0,
            reply_target: None,
            overlay: Overlay::None,
            log_scroll: 0,
            sticker: StickerState::default(),
            prompt: None,
            confirm: None,
            poll_inflight: false,
            busy: false,
            boot_at: std::time::Instant::now(),
            boot_done: true,
            llm_label: None,
            llm_override: None,
            cached_models: None,
            sidebar: SidebarMode::default(),
            pinned_ids: vec!["pinned-old".into()],
            bg_rx: rx,
            bg_tx: tx,
        };
        app.sort_threads();
        let ids: Vec<&str> = app.threads.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["pinned-old", "new", "old"]);
    }

    /// Recency sort puts newest-message threads first, keeping selection.
    #[test]
    fn sort_threads_orders_by_newest() {
        let mut session = crate::session::Session::new(false, false, false).unwrap();
        let (tx, rx) = channel();
        let mk = |id: &str, ts: i64| Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: id.into(),
            name: id.into(),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: false,
            messages: if ts > 0 {
                vec![ChatMessage {
                    id: format!("m_{id}"),
                    sender: "A".into(),
                    text: "hi".into(),
                    time: String::new(),
                    is_outgoing: false,
                    ts,
                    ..Default::default()
                }]
            } else {
                Vec::new()
            },
            vanish: None,
        };
        let mut app = TuiApp {
            session: &mut session,
            threads: vec![mk("old", 100), mk("empty", 0), mk("new", 200)],
            list_state: ListState::default(),
            selected_thread: 1,
            input_text: String::new(),
            focus: Focus::Threads,
            status: String::new(),
            should_quit: false,
            store: None,
            poll_state: chat::PollState::default(),
            filtered: vec![0, 1, 2],
            filter: FilterTab::All,
            search: String::new(),
            search_mode: false,
            unread: HashMap::new(),
            selected_msg: None,
            msg_scroll: 0,
            follow_tail: true,
            msg_ranges: Vec::new(),
            chat_view_h: 10,
            pinned_ids: TuiApp::load_pinned_ids(),
            find_query: String::new(),
            find_hits: Vec::new(),
            find_pos: 0,
            find_mode: false,
            slash_sel: 0,
            threads_width: 0,
            reply_target: None,
            overlay: Overlay::None,
            log_scroll: 0,
            sticker: StickerState::default(),
            prompt: None,
            confirm: None,
            poll_inflight: false,
            busy: false,
            boot_at: std::time::Instant::now(),
            boot_done: true,
            llm_label: None,
            llm_override: None,
            cached_models: None,
            sidebar: SidebarMode::default(),
            bg_rx: rx,
            bg_tx: tx,
        };
        app.sort_threads();
        let ids: Vec<&str> = app.threads.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["new", "old", "empty"]);
        // selection follows the same thread ("empty"), not the same index
        assert_eq!(app.threads[app.selected_thread].id, "empty");
    }

    /// Slash popup: prefix filter, Tab/Enter completion, Up/Down cycling.
    #[test]
    fn slash_popup_completes_and_cycles() {
        let mut session = crate::session::Session::new(false, false, false).unwrap();
        let (tx, rx) = channel();
        let thread = Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: "t1".into(),
            name: "Alpha".into(),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: false,
            messages: Vec::new(),
            vanish: None,
        };
        let mut app = TuiApp {
            session: &mut session,
            threads: vec![thread],
            list_state: ListState::default(),
            selected_thread: 0,
            input_text: "/s".into(),
            focus: Focus::Input,
            status: String::new(),
            should_quit: false,
            store: None,
            poll_state: chat::PollState::default(),
            filtered: vec![0],
            filter: FilterTab::All,
            search: String::new(),
            search_mode: false,
            unread: HashMap::new(),
            selected_msg: None,
            msg_scroll: 0,
            follow_tail: true,
            msg_ranges: Vec::new(),
            chat_view_h: 10,
            find_query: String::new(),
            find_hits: Vec::new(),
            find_pos: 0,
            find_mode: false,
            slash_sel: 0,
            threads_width: 0,
            reply_target: None,
            overlay: Overlay::None,
            log_scroll: 0,
            sticker: StickerState::default(),
            prompt: None,
            confirm: None,
            poll_inflight: false,
            busy: false,
            boot_at: std::time::Instant::now(),
            boot_done: true,
            llm_label: None,
            llm_override: None,
            cached_models: None,
            sidebar: SidebarMode::default(),
            pinned_ids: Vec::new(),
            bg_rx: rx,
            bg_tx: tx,
        };
        let cands = app.slash_candidates();
        let names: Vec<&str> = cands.iter().map(|(c, _)| c.as_str()).collect();
        assert!(names.iter().any(|c| c.starts_with("/stats")), "stats in {:?}", names);
        assert!(names.iter().any(|c| c.starts_with("/sum")), "sum in {:?}", names);
        assert!(names.iter().any(|c| c.starts_with("/script")), "script in {:?}", names);
        // cycle + complete first item
        assert!(app.cycle_slash(1));
        assert!(app.apply_slash_completion());
        assert!(app.input_text.starts_with('/'));
        // non-slash input: no popup
        app.input_text = "hello".into();
        assert!(app.slash_candidates().is_empty());
        assert!(!app.apply_slash_completion());
    }

    #[test]
    fn split_links_finds_urls() {
        let segs = super::split_links("xem https://a.vn/b và www.c.com nhé");
        assert_eq!(segs.len(), 5);
        assert!(segs[1].0 && segs[1].1 == "https://a.vn/b");
        assert!(segs[3].0 && segs[3].1 == "www.c.com");
        assert!(!segs[4].0 && segs[4].1 == " nhé");
        assert!(super::split_links("không có gì").iter().all(|(l, _)| !l));
    }

    /// `--thread` preselects by exact id, falls back to name substring, rejects unknown.
    #[test]
    fn select_thread_by_id_matches_exact_then_name() {
        let mut session = crate::session::Session::new(false, false, false).unwrap();
        let (tx, rx) = channel();
        let mk = |id: &str, name: &str| Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: id.into(),
            name: name.into(),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: false,
            messages: Vec::new(),
            vanish: None,
        };
        let mut app = TuiApp {
            session: &mut session,
            threads: vec![mk("t1", "Alpha"), mk("t2", "Beta Cloud")],
            list_state: ListState::default(),
            selected_thread: 0,
            input_text: String::new(),
            focus: Focus::Threads,
            status: String::new(),
            should_quit: false,
            store: None,
            poll_state: chat::PollState::default(),
            filtered: vec![0, 1],
            filter: FilterTab::All,
            search: String::new(),
            search_mode: false,
            unread: HashMap::new(),
            selected_msg: None,
            msg_scroll: 0,
            follow_tail: true,
            msg_ranges: Vec::new(),
            chat_view_h: 10,
            pinned_ids: TuiApp::load_pinned_ids(),
            find_query: String::new(),
            find_hits: Vec::new(),
            find_pos: 0,
            find_mode: false,
            slash_sel: 0,
            threads_width: 0,
            reply_target: None,
            overlay: Overlay::None,
            log_scroll: 0,
            sticker: StickerState::default(),
            prompt: None,
            confirm: None,
            poll_inflight: false,
            busy: false,
            boot_at: std::time::Instant::now(),
            boot_done: true,
            llm_label: None,
            llm_override: None,
            cached_models: None,
            sidebar: SidebarMode::default(),
            bg_rx: rx,
            bg_tx: tx,
        };
        assert!(app.select_thread_by_id("t2"));
        assert_eq!(app.selected_thread, 1);
        assert_eq!(app.focus, Focus::Messages);
        // name substring fallback
        app.focus = Focus::Threads;
        assert!(app.select_thread_by_id("alpha"));
        assert_eq!(app.selected_thread, 0);
        // unknown
        assert!(!app.select_thread_by_id("no-such-thread"));
    }

    /// `/help` and unknown commands are consumed locally; plain text is not.
    #[test]
    fn slash_commands_consumed_plain_text_passes_through() {
        let mut session = crate::session::Session::new(false, false, false).unwrap();
        let (tx, rx) = channel();
        let thread = Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: "t1".into(),
            name: "Alpha".into(),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: false,
            messages: Vec::new(),
            vanish: None,
        };
        let mut app = TuiApp {
            session: &mut session,
            threads: vec![thread],
            list_state: ListState::default(),
            selected_thread: 0,
            input_text: String::new(),
            focus: Focus::Input,
            status: String::new(),
            should_quit: false,
            store: None,
            poll_state: chat::PollState::default(),
            filtered: vec![0],
            filter: FilterTab::All,
            search: String::new(),
            search_mode: false,
            unread: HashMap::new(),
            selected_msg: None,
            msg_scroll: 0,
            follow_tail: true,
            msg_ranges: Vec::new(),
            chat_view_h: 10,
            pinned_ids: TuiApp::load_pinned_ids(),
            find_query: String::new(),
            find_hits: Vec::new(),
            find_pos: 0,
            find_mode: false,
            slash_sel: 0,
            threads_width: 0,
            reply_target: None,
            overlay: Overlay::None,
            log_scroll: 0,
            sticker: StickerState::default(),
            prompt: None,
            confirm: None,
            poll_inflight: false,
            busy: false,
            boot_at: std::time::Instant::now(),
            boot_done: true,
            llm_label: None,
            llm_override: None,
            cached_models: None,
            sidebar: SidebarMode::default(),
            bg_rx: rx,
            bg_tx: tx,
        };
        // plain text: not a command
        assert!(!app.handle_slash("hello world"));
        assert_eq!(app.input_text, "");
        // /help writes a console line into the thread
        assert!(app.handle_slash("/help"));
        assert_eq!(app.threads[0].messages.len(), 1);
        assert_eq!(app.threads[0].messages[0].sender, "Console");
        assert!(app.threads[0].messages[0].text.contains("/note"));
        // unknown command also consumed with guidance
        assert!(app.handle_slash("/nope arg"));
        assert_eq!(app.threads[0].messages.len(), 2);
    }

    /// The "clear local data" confirmation must reach the terminal with its Vietnamese
    /// text intact — it is the only guard in front of an irreversible delete (#21).
    #[test]
    fn wipe_confirmation_renders_its_vietnamese_text() {
        let mut session = crate::session::Session::new(false, false, false).unwrap();
        let (tx, rx) = channel();
        let mut app = TuiApp {
            session: &mut session,
            threads: Vec::new(),
            list_state: ListState::default(),
            selected_thread: 0,
            input_text: String::new(),
            focus: Focus::Threads,
            status: "test".into(),
            should_quit: false,
            store: None,
            poll_state: chat::PollState::default(),
            filtered: Vec::new(),
            filter: FilterTab::All,
            search: String::new(),
            search_mode: false,
            unread: HashMap::new(),
            selected_msg: None,
            msg_scroll: 0,
            follow_tail: true,
            msg_ranges: Vec::new(),
            chat_view_h: 10,
            pinned_ids: TuiApp::load_pinned_ids(),
            find_query: String::new(),
            find_hits: Vec::new(),
            find_pos: 0,
            find_mode: false,
            slash_sel: 0,
            threads_width: 0,
            reply_target: None,
            overlay: Overlay::None,
            log_scroll: 0,
            sticker: StickerState::default(),
            prompt: None,
            confirm: Some(ConfirmAction {
                question: "Xoá lịch sử tin nhắn cục bộ (zalo_history.db, threads_cache.json)?"
                    .into(),
                detail: "Session đăng nhập giữ nguyên. Thao tác này không thể hoàn tác.".into(),
                kind: ConfirmKind::WipeLocalData,
                thread_id: String::new(),
                msg_id: String::new(),
            }),
            poll_inflight: false,
            busy: false,
            boot_at: std::time::Instant::now(),
            boot_done: true,
            llm_label: None,
            llm_override: None,
            cached_models: None,
            sidebar: SidebarMode::default(),
            bg_rx: rx,
            bg_tx: tx,
        };
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| ui_draw(f, &mut app)).unwrap();

        let screen: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(
            screen.contains("Xác nhận"),
            "confirmation title missing from the frame"
        );
        assert!(
            screen.contains("Xoá lịch sử tin nhắn cục bộ"),
            "confirmation question missing from the frame"
        );
    }
}
