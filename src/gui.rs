//! Complete native desktop GUI powered by `eframe` and `egui`.
//!
//! Designed with the UI/UX Pro Max Minimalism & Swiss Style system:
//! - Zero-idle-CPU reactive architecture (event-driven repaint, zero per-frame Thread cloning)
//! - Custom vector icons rendered via `egui::Painter` (zero texture/SVG bloat)
//! - High-contrast Light & Dark semantic color tokens (`#0068FF` / `#2563EB` primary, `#059669` online accent)
//! - Filterable conversation sidebar (`Tất cả`, `Cá nhân`, `Nhóm`), avatar badges, and grouped chat bubbles
//! - Built-in Zalo Sticker rendering & picker (`[Nhãn dán]`)

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use eframe::egui::{
    self, Align, Color32, CursorIcon, FontId, Id, Key, Layout, Margin, Pos2, Rect, RichText,
    Rounding, Sense, Stroke, TextureHandle, TextureOptions, Vec2,
};

use crate::chat::{ChatMessage, Thread};
use crate::logger::{self, FindingLevel, LogEntry, ScanFinding};
use crate::qr::QrLogin;
use crate::session::Session;

mod conv_info;
mod emoji_picker;
mod glass_button;
mod global_search;
mod messages;
mod modals;
mod people;
mod reaction_people;
mod stickers;
mod theme;
mod worker;
mod workspace;

use glass_button::{appear, reduce_motion, GlassState};
use messages::*;
use stickers::*;
use theme::*;
pub use theme::{emoji_to_tex_key, load_app_icon, lucide, REACTION_LIST};
pub use worker::WorkerCmd;
use worker::*;

fn merge_sent_message(thread: &mut Thread, message: ChatMessage) {
    crate::chat::merge_sent_message(thread, message);
}

pub enum GuiEvent {
    GroupPostingLoaded {
        thread_id: String,
        policy: Option<crate::chat::GroupPostingPolicy>,
    },
    /// Messages pinned inside a conversation (`/api/board/pin/list`).
    PinnedTopicsLoaded {
        thread_id: String,
        topics: Vec<crate::chat::PinnedTopic>,
    },
    GroupMembersLoaded {
        thread_id: String,
        members: Vec<crate::chat::GroupMember>,
    },
    /// Live reaction events from the WebSocket (`reacts` / `reactGroups`).
    ReactionsReceived(Vec<crate::reactions::ReactionEvent>),
    /// Friend aliases (biệt danh) keyed by uid (`/api/alias/list`).
    AliasesLoaded(HashMap<String, String>),
    /// Result of setting/removing one alias.
    AliasUpdated {
        friend_id: String,
        alias: String,
        res: Result<(), String>,
    },
    /// Groups the server lists in `removedsGroup` (left from another device, removed or
    /// disbanded): drop them here as Zalo does.
    GroupsRemoved(Vec<String>),
    /// Result of `WorkerCmd::LeaveGroup`.
    GroupLeft {
        thread_id: String,
        name: String,
        res: Result<(), String>,
    },
    /// Decoded whole-window wallpaper (`None` when picking/decoding failed or was cancelled).
    BackgroundImageReady {
        path: String,
        image: Option<egui::ColorImage>,
        picked: bool,
    },
    AccountInfoLoaded {
        uid: String,
        res: Result<crate::feed::SocialUserProfile, String>,
    },
    /// Terminal response for a Settings authorization action.
    NotificationAuthorization {
        request_id: u64,
        result: Result<
            crate::native_notifications::AuthorizationStatus,
            crate::native_notifications::NotificationError,
        >,
    },
    QrReady {
        qr_login: QrLogin,
        qr_image: egui::ColorImage,
    },
    QrScanned {
        name: String,
    },
    LoginSuccess {
        name: String,
        uin: String,
        avatar: Option<String>,
    },
    LoginFailed(String),
    ThreadsUpdated(Vec<Thread>),
    MessageSent {
        client_message_id: String,
        thread_id: String,
        text: String,
        msg_id: String,
    },
    StickerSent {
        client_message_id: String,
        thread_id: String,
        msg_id: String,
        sticker_id: u64,
        cate_id: u64,
        sticker_type: u32,
    },
    AttachmentSent {
        thread_id: String,
        msg: ChatMessage,
    },
    StickerImageReady {
        key: String,
        image: egui::ColorImage,
    },
    StickerStoreLoaded(Vec<crate::chat::StickerPackInfo>),
    StickerPackItemsLoaded {
        cate_id: u64,
        name: String,
        items: Vec<crate::chat::StickerItem>,
    },
    StickerSearchResults {
        keyword: String,
        items: Vec<crate::chat::StickerItem>,
    },
    StickerSuggestKeywordsLoaded(Vec<String>),
    ComposerStickerSuggestions {
        keyword: String,
        items: Vec<crate::chat::StickerItem>,
    },
    LinkPreviewReady(crate::chat::LinkMetadata),
    PhoneLookupSuccess(crate::chat::PhoneLookupResult),
    PhoneLookupFailed(String),
    FriendStatusChecked {
        user_id: String,
        status: crate::chat::FriendshipStatus,
        online: Option<(bool, String)>,
    },
    QuoteSent {
        client_message_id: String,
        thread_id: String,
        text: String,
        msg_id: String,
        quoted: ChatMessage,
    },
    Reacted {
        thread_id: String,
        msg_id: String,
        ts: i64,
        reaction: String,
    },
    MessageDeleted {
        thread_id: String,
        msg_id: String,
        ts: i64,
    },
    MessageRecalled {
        thread_id: String,
        msg_id: String,
        ts: i64,
    },
    SendFailed {
        thread_id: String,
        text: String,
        error: String,
    },
    SessionExpired(String),
    /// The server could not be reached: the session is intact, show it and retry (#21).
    SessionUnreachable(String),
    /// `zalo_history.db` + `threads_cache.json` were removed on request (#21).
    LocalDataWiped,
    CallStateChanged(String),
    IncomingCall {
        call_id: i64,
        caller_id: String,
        caller_name: String,
        is_video: bool,
        is_group: bool,
    },
    CallRejected {
        code: i64,
        reason: String,
    },
    StatusUpdate(String),
    RefreshFinished {
        threads: usize,
        messages: usize,
        errors: Vec<String>,
        manual: bool,
    },
    ScheduledListUpdated,
    SyncFinished {
        added: usize,
        errors: Vec<String>,
        cancelled: bool,
    },
    UpdateCheckFinished {
        result: Result<UpdateCheckResult, String>,
        silent: bool,
    },
    UpdateDownloadStatus {
        downloading: bool,
        message: String,
    },
    UpdateDownloadProgress {
        downloaded: u64,
        total: u64,
    },
    UpdateAppliedAndRestarting(String),
    SocialProfileLoaded(Result<crate::feed::SocialUserProfile, String>),
    SocialAvatarsLoaded(Result<Vec<crate::feed::FeedMedia>, String>),
    SocialTimelineLoaded(
        Result<
            (
                Vec<crate::feed::SocialUserProfile>,
                Vec<crate::feed::FeedItem>,
            ),
            String,
        >,
    ),
    MuteSnapshotSynced(HashMap<String, i64>),
    MuteEventsReceived(Vec<crate::chat::MuteControlEvent>),
    PhoneSyncProgress {
        progress: f32,
        status: String,
    },
    BackgroundHistoryFinished {
        added: usize,
        active_threads: usize,
        error: Option<String>,
        yielded: bool,
    },
    PhoneSyncFinished {
        threads: Vec<Thread>,
        added: usize,
        active_threads: usize,
        cancelled: bool,
        error: Option<String>,
    },
    ProxyUpdated(Result<Option<String>, String>),
    /// Attachment saved (`(file name, local path)`), keyed by its source URL.
    FileDownloaded {
        url: String,
        res: Result<(String, String), String>,
    },
    LoggedOut,
    HistoryReachedStart(String),
    AccountSwitched {
        uin: String,
        name: String,
        avatar: Option<String>,
        threads: Vec<Thread>,
    },
}

fn phone_sync_completion_message(
    added: usize,
    active_threads: usize,
    cancelled: bool,
    error: Option<&str>,
    phone_status: Option<&str>,
) -> String {
    if let Some(err) = error {
        return format!("⚠️ Đồng bộ thất bại: {err}");
    }
    if cancelled {
        if let Some(note) = phone_status.filter(|note| note.starts_with("Đã dừng")) {
            return note.to_string();
        }
        if let Some(note) = phone_status.filter(|note| note.contains("thành công")) {
            return format!("Đã dừng yêu cầu. {note}");
        }
    }
    if !cancelled {
        if let Some(note) = phone_status.filter(|note| note.contains("thành công")) {
            return format!("{note}. Cloud/Nhóm: {added} tin mới ({active_threads} hội thoại).");
        }
    }
    let prefix = if cancelled {
        "Đã dừng"
    } else {
        "Đã hoàn tất"
    };
    format!(
        "{prefix} yêu cầu: {added} tin mới từ Cloud/Nhóm ({active_threads} hội thoại). Lịch sử điện thoại chưa được tải vào ứng dụng."
    )
}

#[derive(Debug, Clone)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub release_title: String,
    pub release_notes: String,
    pub html_url: String,
    pub download_url: Option<String>,
    pub asset_name: Option<String>,
    pub asset_size: u64,
    pub expected_sha256: Option<String>,
    pub published_at: String,
}

/// Compare semantic versions (`"v0.0.2"` > `"0.0.1"`).
pub fn is_newer_semver(remote: &str, current: &str) -> bool {
    crate::updater::is_newer_semver(remote, current)
}

/// Query GitHub Releases API (`/repos/lebad280324/ctdozlite/releases/latest`) for updates.
pub async fn check_github_release_cli() -> Result<UpdateCheckResult, String> {
    worker::fetch_github_latest_release().await
}

#[derive(Debug, Clone)]
struct ActiveCallOverlay {
    partner_id: String,
    partner_name: String,
    is_video: bool,
    is_group: bool,
    state_label: String,
    started_at: std::time::Instant,
    mic_muted: bool,
    cam_enabled: bool,
    is_incoming_ringing: bool,
    error_code: Option<i64>,
    error_reason: Option<String>,
    speaker_view: bool,
    pinned_tile_idx: usize,
}

#[derive(Debug, Clone)]
struct ImageViewerModal {
    tex_key: String,
    hd_tex_key: Option<String>,
    full_url: String,
    title: String,
    sender: String,
    time: String,
    zoom: f32,
    pan: Vec2,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeedModalTab {
    Profile,
    Avatars,
    Packet50001,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContactsSubTab {
    Friends,
    Groups,
    FriendRequests,
    GroupInvites,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Account,
    Privacy,
    Sync,
    Appearance,
    Notifications,
    Messages,
    Utilities,
    Update,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct AppSettings {
    /// Conversations deleted on this Mac → deletion time (ms). Server refreshes must not bring
    /// them back unless a newer message arrives.
    deleted_threads: HashMap<String, i64>,
    /// Stickers sent recently (sticker id, category id, type), newest first.
    recent_stickers: Vec<(u64, u64, u32)>,
    /// Emoji picked recently in the composer's emoji panel, newest first.
    recent_emojis: Vec<String>,
    /// Attachments saved from chats: source URL → local file path (Zalo's "Đã có trên máy").
    downloaded_files: HashMap<String, String>,
    // 1. Cài đặt chung
    show_all_contacts: bool,
    language: String,
    autostart_on_boot: bool,
    remember_login: bool,
    download_dir: String,
    // 2. Quyền riêng tư
    show_birthday: String,
    show_online_status: bool,
    show_seen_status: bool,
    allow_message_from: String,
    block_typing: bool,
    block_delivered: bool,
    block_seen: bool,
    allow_call_from: String,
    allow_search_by_phone: bool,
    friend_src_qr: bool,
    friend_src_group: bool,
    friend_src_card: bool,
    // 3. Giao diện
    theme_mode: String, // "light" | "dark" | "system"
    use_avatar_as_wallpaper: bool,
    /// Liquid Glass: translucent UI over macOS vibrancy (blurred desktop).
    liquid_glass: bool,
    /// Icon set: "phosphor" | "phosphor-bold" | "phosphor-fill" | "lucide" (`theme::IconStyle`).
    icon_style: String,
    /// Whole-window wallpaper copied into the data dir (empty = none).
    app_background: String,
    /// UI zoom. egui sizes fonts by line height (ascent−descent), so the same point size reads
    /// ~16 % smaller than AppKit/Zalo text; 1.18 matches macOS text size.
    ui_scale: f32,
    // 4. Thông báo
    enable_notifications: bool,
    play_notification_sound: bool,
    // 5. Tin nhắn
    quick_messages_enabled: bool,
    hidden_chat_pin_enabled: bool,
    split_priority_inbox: bool,
    excel_paste_mode: String, // "image" | "text" | "ask"
    double_click_to_reply: bool,
    show_typing_indicator: bool,
    send_message_key: String, // "enter" | "cmd_enter"
    // 6. Tiện ích
    suggest_stickers_while_typing: bool,
    suggest_mentions: bool,
    enable_gif_search: bool,
    enable_sticker_keyword_search: bool,
    // 7. Thiết bị cuộc gọi (Mic & Camera)
    selected_mic_device: String,
    selected_camera_device: String,
    mic_gain_percent: u32,
    // 8. Quản lý hội thoại (Context menu: Ghim, Phân loại, Tắt thông báo, Ẩn)
    pinned_threads: Vec<String>,
    muted_threads: HashMap<String, i64>,
    hidden_threads: HashSet<String>,
    thread_labels: HashMap<String, String>,
    // 9. Cập nhật tự động qua GitHub Releases API
    auto_check_updates: bool,
    skipped_update_version: String,
    // 10. Proxy & Đa tài khoản (#64)
    proxy_url: String,
    multi_profile_name: String,
    info_drawer_open: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        let default_dl = std::env::var("USERPROFILE")
            .ok()
            .map(|p| format!(r"{p}\Downloads\Zalo Received Files"))
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| format!("{h}/Downloads/Zalo Received Files"))
            })
            .unwrap_or_else(|| "Downloads/Zalo Received Files".into());
        Self {
            deleted_threads: HashMap::new(),
            recent_stickers: Vec::new(),
            recent_emojis: Vec::new(),
            downloaded_files: HashMap::new(),
            show_all_contacts: false,
            language: "Tiếng Việt".into(),
            autostart_on_boot: true,
            remember_login: true,
            download_dir: default_dl,
            show_birthday: "Không hiện".into(),
            show_online_status: true,
            show_seen_status: true,
            block_typing: false,
            block_delivered: false,
            block_seen: false,
            allow_message_from: "Tất cả mọi người".into(),
            allow_call_from: "Bạn bè và người lạ từng liên hệ".into(),
            allow_search_by_phone: true,
            friend_src_qr: true,
            friend_src_group: true,
            friend_src_card: true,
            theme_mode: "dark".into(),
            use_avatar_as_wallpaper: false,
            liquid_glass: cfg!(target_os = "macos"),
            icon_style: "phosphor".into(),
            app_background: String::new(),
            ui_scale: if cfg!(target_os = "macos") { 1.18 } else { 1.0 },
            enable_notifications: true,
            play_notification_sound: false,
            quick_messages_enabled: true,
            hidden_chat_pin_enabled: true,
            split_priority_inbox: false,
            excel_paste_mode: "ask".into(),
            double_click_to_reply: true,
            show_typing_indicator: true,
            send_message_key: "enter".into(),
            suggest_stickers_while_typing: true,
            suggest_mentions: true,
            enable_gif_search: true,
            enable_sticker_keyword_search: true,
            selected_mic_device: String::new(),
            selected_camera_device: String::new(),
            mic_gain_percent: 100,
            pinned_threads: Vec::new(),
            muted_threads: HashMap::new(),
            hidden_threads: HashSet::new(),
            thread_labels: HashMap::new(),
            auto_check_updates: true,
            skipped_update_version: String::new(),
            proxy_url: String::new(),
            multi_profile_name: "acc2".into(),
            info_drawer_open: false,
        }
    }
}

impl AppSettings {
    fn load() -> Self {
        std::fs::read_to_string("app_settings.json")
            .ok()
            .and_then(|s| serde_json::from_str::<Self>(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write("app_settings.json", json);
        }
    }
}

/// A loaded-message jump stays bound to its conversation and exact reference.
struct QuoteJump {
    thread_id: String,
    reference: String,
    highlight_until: std::time::Instant,
    scroll_pending: bool,
}

impl QuoteJump {
    fn target_index(&self, thread_id: &str, messages: &[ChatMessage]) -> Option<usize> {
        if self.thread_id != thread_id || std::time::Instant::now() >= self.highlight_until {
            return None;
        }
        crate::message_navigation::unique_reference_index(messages, &self.reference)
    }
}

/// Bubbles rendered per step in the chat view; "Tải thêm tin nhắn cũ" adds another step.
const CHAT_RENDER_STEP: usize = 150;

/// Worker → GUI channel that wakes the UI on every send. The GUI no longer repaints on a
/// 1.2 s timer, so an event sent without a repaint would sit unseen until the next mouse move.
#[derive(Clone)]
pub struct GuiTx {
    tx: Sender<GuiEvent>,
    ctx: egui::Context,
}

impl GuiTx {
    /// Same contract as `Sender::send` (callers already `let _ =` it), plus the wake-up.
    #[allow(clippy::result_large_err)]
    pub fn send(&self, ev: GuiEvent) -> Result<(), std::sync::mpsc::SendError<GuiEvent>> {
        let res = self.tx.send(ev);
        self.ctx.request_repaint();
        res
    }
}

pub struct ZaloGuiApp {
    // Theme state
    dark_mode: bool,
    theme_dirty: bool,
    /// Whether the macOS vibrancy view is currently installed behind the window.
    vibrancy_on: bool,

    // Session state
    is_logged_in: bool,
    user_name: String,
    user_uin: String,
    user_avatar: Option<String>,

    // Login screen state
    login_tab: LoginTab,
    phone_input: String,
    password_input: String,
    login_in_progress: bool,
    login_status_msg: String,

    // QR state
    qr_texture: Option<TextureHandle>,
    qr_status: String,

    // Chat screen state
    threads: Vec<Thread>,
    selected_thread_index: usize,
    search_query: String,
    input_message: String,
    status_msg: String,

    // Reply / forward state
    replying_to: Option<ChatMessage>,
    forward_msg: Option<ChatMessage>,
    forward_filter: String,
    filter_starred_only: bool,
    active_call: Option<ActiveCallOverlay>,
    image_viewer: Option<ImageViewerModal>,

    info_drawer_open: bool,
    profile_menu_open: bool,
    /// "Xoá dữ liệu tin nhắn cục bộ" is pending confirmation (#21).
    wipe_confirm_open: bool,
    alias_editor: Option<AliasEditor>,
    /// Chat-list search expanded from its round button (stays open while it has text/focus).
    search_open: bool,
    search_focus_pending: bool,
    /// Composer emoji panel: open state, active tab, toggle button rect.
    emoji_picker_open: bool,
    emoji_tab: usize,
    emoji_btn_rect: Rect,
    /// Search results tab: 0 Tất cả, 1 Liên hệ, 2 Tin nhắn, 3 File.
    global_search_tab: u8,
    /// Composer / chat header rects from the last frame (they float over the message list).
    composer_rect_last: Rect,
    /// Rects of panels drawn over the message list last frame (sticker picker, staged
    /// attachments…); bubble hover actions must not trigger through them.
    overlay_rects_last: Vec<Rect>,
    header_rect_last: Rect,
    /// In-chat search results panel: query it was built for, hidden after a jump, page size.
    in_chat_results_query: String,
    in_chat_results_hidden: bool,
    in_chat_results_limit: usize,
    /// Info drawer sections the user collapsed / expanded with "Xem tất cả".
    drawer_collapsed: HashSet<String>,
    drawer_show_all: HashSet<String>,
    /// Inline confirmation for "Xoá lịch sử trò chuyện" in the info drawer.
    drawer_confirm_delete: bool,
    /// Thread id → message count already matched against Zalo PC's downloads.
    zpc_enriched: HashMap<String, usize>,
    /// "Rời nhóm" confirmation: (thread id, group name, leave silently).
    leave_group_confirm: Option<(String, String, bool)>,
    /// Group ids with a leave request in flight (the button shows a spinner).
    leaving_groups: HashSet<String>,
    /// Server display names of 1:1 threads whose `name` was replaced by an alias.
    alias_original_names: HashMap<String, String>,
    toast_notice: Option<(String, std::time::Instant)>,
    phone_modal_open: bool,
    phone_modal_input: String,
    phone_lookup_loading: bool,
    phone_lookup_result: Option<crate::chat::PhoneLookupResult>,
    /// Query looked up inline from the sidebar search (Zalo-style); no modal for these.
    phone_lookup_inline: Option<String>,
    phone_lookup_error: Option<String>,
    in_chat_search_open: bool,
    in_chat_search_query: String,
    in_chat_search_focus_pending: bool,
    // Sticker picker & image cache state
    sticker_picker_open: bool,
    sticker_tab: StickerTab,
    sticker_textures: HashMap<String, TextureHandle>,
    loading_stickers: HashSet<String>,
    emoji_textures: HashMap<String, TextureHandle>,
    app_logo_texture: Option<TextureHandle>,
    sticker_store_packs: Vec<crate::chat::StickerPackInfo>,
    sticker_store_loading: bool,
    sticker_store_filter: String,
    sticker_pack_items: HashMap<u64, Vec<crate::chat::StickerItem>>,
    loading_sticker_packs: HashSet<u64>,
    pinned_sticker_packs: Vec<(u64, String)>,
    sticker_search_query: String,
    sticker_search_results: Vec<crate::chat::StickerItem>,
    sticker_search_loading: bool,
    last_sticker_search_dispatched: String,
    last_sticker_search_edit_at: std::time::Instant,
    sticker_suggest_keywords: HashSet<String>,
    composer_sticker_cache: HashMap<String, Vec<crate::chat::StickerItem>>,
    composer_sticker_loading: HashSet<String>,
    dismissed_sticker_suggest_for: String,
    last_composer_text: String,
    last_composer_edit_at: std::time::Instant,
    composer_sticker_selected: Option<usize>,
    composer_focus_pending: bool,
    pending_clipboard_image: Option<(std::path::PathBuf, u32, u32, TextureHandle)>,
    /// Files pasted from the clipboard (Finder copy), staged until the user presses send.
    pending_paste_files: Vec<std::path::PathBuf>,
    /// `@` tags picked in the composer for the open group: `(uid, display name)`.
    composer_mentions: Vec<(String, String)>,
    /// Highlighted row in the `@` member popup.
    mention_selected: usize,
    /// Composer attach menu (photo / file / sticker / schedule) open.
    attach_menu_open: bool,
    /// Multi-select mode ("Chọn nhiều tin nhắn"): selected message ids of the open thread.
    select_mode: bool,
    selected_msgs: std::collections::HashSet<String>,
    /// Message shown in the "Chi tiết tin nhắn" dialog.
    msg_details: Option<ChatMessage>,
    /// Server-side pinned messages per thread id.
    pinned_topics: std::collections::HashMap<String, Vec<crate::chat::PinnedTopic>>,
    /// Thread whose pinned messages were last requested (fetch once per open).
    pinned_requested_for: String,
    /// Pinned bar expanded to list every pin.
    pinned_expanded: bool,
    /// Full member list per group thread id.
    group_members: std::collections::HashMap<String, Vec<crate::chat::GroupMember>>,
    /// "Thành viên" panel open for the selected group.
    members_panel_open: bool,
    /// "Thông tin tài khoản" card: (uid, fallback name, fallback avatar, loaded profile).
    account_card: Option<AccountCard>,
    /// Liquid glass send button: (thread id, state, since `ctx.input(time)`).
    send_fx: Option<(String, glass_button::GlassState, f64)>,
    /// Whole-window wallpaper texture and whether its load was requested.
    bg_texture: Option<TextureHandle>,
    bg_requested: bool,
    unread_counts: HashMap<String, usize>,
    /// Wall-clock ms when this GUI session started (unread counting for cursor-less threads).
    session_started_ms: i64,
    link_metadata_cache: HashMap<String, crate::chat::LinkMetadata>,
    loading_link_previews: HashSet<String>,
    dismissed_link_preview_for: String,
    friend_status_map: HashMap<String, crate::chat::FriendshipStatus>,
    checking_friend_status: HashSet<String>,
    user_online_map: HashMap<String, (bool, String)>,
    allow_quit: bool,
    settings_modal_open: bool,
    settings_tab: SettingsTab,
    notification_authorization: crate::native_notifications::AuthorizationState,
    settings: AppSettings,
    settings_blur_tex: Option<TextureHandle>,
    settings_blur_req_at: Option<std::time::Instant>,
    update_checking: bool,
    update_downloading: bool,
    update_progress: Option<(u64, u64)>,
    update_status: String,
    update_info: Option<UpdateCheckResult>,
    update_modal_open: bool,
    update_banner_dismissed: bool,
    last_notified_update_version: Option<String>,
    dev_mode: bool,
    dev_panel_open: bool,
    dev_tab: DevTab,
    selected_log: Option<LogEntry>,
    scanner_findings: Vec<ScanFinding>,
    feed_modal_open: bool,
    feed_modal_tab: FeedModalTab,
    feed_target_uid: String,
    feed_loading_profile: bool,
    feed_profile: Option<crate::feed::SocialUserProfile>,
    feed_profile_error: Option<String>,
    feed_loading_avatars: bool,
    feed_avatars: Vec<crate::feed::FeedMedia>,
    feed_avatars_error: Option<String>,
    feed_loading_timeline: bool,
    feed_items: Vec<crate::feed::FeedItem>,
    feed_friend_profiles: Vec<crate::feed::SocialUserProfile>,
    feed_search_query: String,
    feed_pkt_owner_id: String,
    feed_pkt_viewer_id: String,
    feed_pkt_tab: u32,
    feed_pkt_page: u32,
    feed_pkt_count: u32,
    contacts_view_open: bool,
    contacts_sub_tab: ContactsSubTab,
    contacts_search_query: String,
    schedule_modal_open: bool,
    schedule_custom_input: String,
    schedule_editing_id: Option<i64>,
    scheduled_items: Vec<crate::scheduler::ScheduledMessage>,
    phone_sync_progress: f32,
    phone_sync_status: String,
    /// Limit an explicit phone sync to the currently open conversation.
    phone_sync_current_only: bool,
    last_typing_sent_at: Option<(String, std::time::Instant)>,
    last_synced_icon_unread: Option<usize>,

    // Background channels
    event_rx: Receiver<GuiEvent>,
    #[allow(dead_code)]
    event_tx: Sender<GuiEvent>,
    /// (thread id, number of newest bubbles rendered) — see `CHAT_RENDER_STEP`.
    chat_render_window: (String, usize),
    quote_jump: Option<QuoteJump>,
    ime_active: bool,
    ime_just_committed: bool,
    /// The IME commit this frame came from Return: the composer treats it as Enter.
    ime_enter_commit: bool,
    /// Selection fill hidden while an IME word is being composed (restored afterwards).
    selection_fill_saved: Option<Color32>,
    history_reached_start: HashSet<String>,
    force_scroll_to_bottom: bool,
    /// Until this time (egui seconds) the open chat stays pinned to its newest message while
    /// images and previews finish loading, unless the user scrolls first.
    pin_chat_bottom_until: f64,
}

#[derive(Debug, PartialEq, Eq)]
enum LoginTab {
    QrCode,
    Password,
}

#[derive(Debug, PartialEq, Eq)]
enum DevTab {
    Logs,
    Scanner,
    CryptoState,
}

/// State of the "Thông tin tài khoản" card (Zalo-style profile popup).
/// "Đặt tên gợi nhớ" dialog state (Zalo friend alias).
pub(crate) struct AliasEditor {
    pub(crate) uid: String,
    /// The friend's real display name (shown as a hint and restored when the alias is removed).
    pub(crate) real_name: String,
    pub(crate) avatar: Option<String>,
    pub(crate) draft: String,
    pub(crate) saving: bool,
}

/// Friend aliases (biệt danh) keyed by uid. Global so renderers without `&ZaloApp`
/// (message bubbles, contact rows) can resolve names.
static ALIASES: std::sync::OnceLock<std::sync::RwLock<HashMap<String, String>>> =
    std::sync::OnceLock::new();

fn alias_store() -> &'static std::sync::RwLock<HashMap<String, String>> {
    ALIASES.get_or_init(|| std::sync::RwLock::new(HashMap::new()))
}

/// The alias this account set for `uid`, if any.
pub(crate) fn alias_for(uid: &str) -> Option<String> {
    if uid.is_empty() {
        return None;
    }
    alias_store().read().ok()?.get(uid).cloned()
}

/// Attachments saved on this Mac (source URL → local path), readable from bubble renderers.
/// Index of Zalo PC's downloads for the signed-in account (built in the background on login).
static ZALO_PC_MEDIA: std::sync::OnceLock<
    std::sync::RwLock<Option<std::sync::Arc<crate::zalo_pc_media::ZaloPcMedia>>>,
> = std::sync::OnceLock::new();

fn zalo_pc_media_slot(
) -> &'static std::sync::RwLock<Option<std::sync::Arc<crate::zalo_pc_media::ZaloPcMedia>>> {
    ZALO_PC_MEDIA.get_or_init(|| std::sync::RwLock::new(None))
}

pub(crate) fn zalo_pc_media() -> Option<std::sync::Arc<crate::zalo_pc_media::ZaloPcMedia>> {
    zalo_pc_media_slot().read().ok()?.clone()
}

/// Lists Zalo PC's download folders off the UI thread and publishes the index.
fn start_zalo_pc_media_scan(own_uid: String, ctx: egui::Context) {
    if let Ok(mut slot) = zalo_pc_media_slot().write() {
        *slot = None;
    }
    std::thread::spawn(move || {
        let Some(root) = crate::zalo_pc_media::default_root(&own_uid) else {
            return;
        };
        let idx = crate::zalo_pc_media::ZaloPcMedia::scan(&root);
        if !idx.is_empty() {
            if let Ok(mut slot) = zalo_pc_media_slot().write() {
                *slot = Some(std::sync::Arc::new(idx));
            }
            ctx.request_repaint();
        }
    });
}

static DOWNLOADED: std::sync::OnceLock<std::sync::RwLock<HashMap<String, String>>> =
    std::sync::OnceLock::new();

fn downloaded_store() -> &'static std::sync::RwLock<HashMap<String, String>> {
    DOWNLOADED.get_or_init(|| std::sync::RwLock::new(HashMap::new()))
}

/// Local path of an attachment already saved from `url`.
pub(crate) fn downloaded_path(url: &str) -> Option<String> {
    downloaded_store().read().ok()?.get(url).cloned()
}

pub(crate) fn set_downloaded_path(url: &str, path: Option<String>) {
    if let Ok(mut map) = downloaded_store().write() {
        match path {
            Some(p) => {
                map.insert(url.to_string(), p);
            }
            None => {
                map.remove(url);
            }
        }
    }
}

pub(crate) struct AccountCard {
    pub(crate) uid: String,
    pub(crate) name: String,
    pub(crate) avatar: Option<String>,
    pub(crate) loading: bool,
    pub(crate) profile: Option<crate::feed::SocialUserProfile>,
    pub(crate) error: Option<String>,
}

/// Paint `tex` over the whole window on the background layer (cover-cropped), with a light
/// scrim so text on translucent panels stays readable.
fn paint_wallpaper(ctx: &egui::Context, tex: &TextureHandle, dark: bool) {
    let screen = ctx.screen_rect();
    let sz = tex.size_vec2();
    let target = screen.width() / screen.height().max(1.0);
    let src = sz.x / sz.y.max(1.0);
    let uv = if src > target {
        let w = target / src;
        Rect::from_min_max(
            Pos2::new((1.0 - w) / 2.0, 0.0),
            Pos2::new((1.0 + w) / 2.0, 1.0),
        )
    } else {
        let h = src / target;
        Rect::from_min_max(
            Pos2::new(0.0, (1.0 - h) / 2.0),
            Pos2::new(1.0, (1.0 + h) / 2.0),
        )
    };
    let painter = ctx.layer_painter(egui::LayerId::background());
    painter.image(tex.id(), screen, uv, Color32::WHITE);
    let scrim = if dark {
        Color32::from_black_alpha(70)
    } else {
        Color32::from_white_alpha(40)
    };
    painter.rect_filled(screen, Rounding::ZERO, scrim);
}

/// Corner radius of the whole window on macOS (Telegram/Zalo-like).
/// Window corner in egui points (× the fixed 118 % zoom on screen). Rounder than Telegram at
/// the user's request; inner cards use `WINDOW_RADIUS - CARD_INSET`.
pub(crate) const WINDOW_RADIUS: f32 = 29.0;

/// Top inset reserved in the left sidebars for the traffic lights on macOS.
pub(crate) const CHROME_TOP_INSET: f32 = 34.0;

/// Resize from the dragged edges while keeping the opposite edges fixed.
#[cfg(any(target_os = "macos", test))]
fn resized_window_rect(
    start: Rect,
    delta: Vec2,
    direction: egui::ResizeDirection,
    minimum: Vec2,
) -> Rect {
    use egui::ResizeDirection::*;
    let mut next = start;
    if matches!(direction, West | NorthWest | SouthWest) {
        next.min.x = (start.min.x + delta.x).min(start.max.x - minimum.x);
    }
    if matches!(direction, East | NorthEast | SouthEast) {
        next.max.x = (start.max.x + delta.x).max(start.min.x + minimum.x);
    }
    if matches!(direction, North | NorthWest | NorthEast) {
        next.min.y = (start.min.y + delta.y).min(start.max.y - minimum.y);
    }
    if matches!(direction, South | SouthWest | SouthEast) {
        next.max.y = (start.max.y + delta.y).max(start.min.y + minimum.y);
    }
    next
}

/// macOS window chrome over the hidden title bar: traffic lights inside the list card and an
/// invisible drag strip right of them (as in Telegram).
/// Width the chat list's search row leaves for the traffic lights (52 px + gap).
pub(crate) const TRAFFIC_LIGHTS_ROW_W: f32 = 60.0;

fn render_window_chrome(ctx: &egui::Context) {
    // Traffic lights inside the list card, vertically centred on the search row
    // (card inset + 4 px margin + half of the 28 px row).
    let inset = ThemePalette::CARD_INSET;
    egui::Area::new(Id::new("traffic_lights"))
        .order(egui::Order::Foreground)
        .fixed_pos(Pos2::new(inset + 17.0, inset + 4.0 + 14.0 - 7.0))
        .show(ctx, |ui| render_traffic_lights(ui, ctx));
    // Drag handle: the thin band above the search row (the row itself holds the search box).
    let strip = Rect::from_min_max(Pos2::new(92.0, 0.0), Pos2::new(318.0, inset + 4.0));
    egui::Area::new(Id::new("window_drag_strip"))
        .order(egui::Order::Foreground)
        .fixed_pos(strip.min)
        .show(ctx, |ui| {
            let (_, resp) = ui.allocate_exact_size(strip.size(), Sense::click_and_drag());
            if resp.double_clicked() {
                let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            } else if resp.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        });
}

/// Paint the window body as a rounded rect on the background layer: wallpaper (cover-cropped,
/// plus scrim) when set, else the app background tint (translucent over vibrancy in glass mode).
/// Square while maximised/fullscreen, where macOS shows no corners either.
fn paint_window_body(
    ctx: &egui::Context,
    pal: &ThemePalette,
    tex: Option<&TextureHandle>,
    dark: bool,
) {
    // Glass without a wallpaper: leave the body fully transparent so only the blur shows.
    let screen = ctx.screen_rect();
    let square = ctx.input(|i| {
        i.viewport().fullscreen.unwrap_or(false) || i.viewport().maximized.unwrap_or(false)
    });
    let rounding = Rounding::same(if square { 0.0 } else { WINDOW_RADIUS });
    let painter = ctx.layer_painter(egui::LayerId::background());
    if pal.is_glass && tex.is_none() {
        edge_hairline(&painter, screen, rounding, dark);
        return;
    }
    if let Some(tex) = tex {
        let sz = tex.size_vec2();
        let target = screen.width() / screen.height().max(1.0);
        let src = sz.x / sz.y.max(1.0);
        let uv = if src > target {
            let w = target / src;
            Rect::from_min_max(
                Pos2::new((1.0 - w) / 2.0, 0.0),
                Pos2::new((1.0 + w) / 2.0, 1.0),
            )
        } else {
            let h = src / target;
            Rect::from_min_max(
                Pos2::new(0.0, (1.0 - h) / 2.0),
                Pos2::new(1.0, (1.0 + h) / 2.0),
            )
        };
        // 1 px bleed: straight edges stay fully opaque (only the rounded corners anti-alias).
        let mut img = egui::epaint::RectShape::filled(screen.expand(1.0), rounding, Color32::WHITE);
        img.fill_texture_id = tex.id();
        img.uv = uv;
        painter.add(img);
        let _ = dark; // wallpaper shown as-is (no scrim) so the background reads 100% clear
    } else {
        painter.rect_filled(screen, rounding, pal.bg_app);
    }
    edge_hairline(&painter, screen, rounding, dark);
}

/// The window has no outline at all (user request): no system shadow rim, no drawn hairline.
fn edge_hairline(_painter: &egui::Painter, _screen: Rect, _rounding: Rounding, _dark: bool) {}

#[cfg(target_os = "macos")]
fn disable_native_window_shadow(window: &impl raw_window_handle::HasWindowHandle) {
    use raw_window_handle::RawWindowHandle;
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };
    // SAFETY: winit's live NSView on the main thread; plain AppKit messages.
    unsafe {
        let view = appkit.ns_view.as_ptr() as *mut objc2::runtime::AnyObject;
        let win: *mut objc2::runtime::AnyObject = objc2::msg_send![view, window];
        if !win.is_null() {
            let _: () = objc2::msg_send![win, setHasShadow: false];
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn disable_native_window_shadow(_window: &impl raw_window_handle::HasWindowHandle) {}

/// macOS-style window buttons: 12 px red/yellow/green dots, glyphs shown while the group is
/// hovered (as in AppKit). Close quits like the old ✕ button; zoom toggles maximise.
fn render_traffic_lights(ui: &mut egui::Ui, ctx: &egui::Context) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(52.0, 14.0), Sense::hover());
    let group_hovered = ui.rect_contains_pointer(rect.expand(4.0));
    let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
    let specs = [
        (
            Color32::from_rgb(255, 95, 87),
            Color32::from_rgb(77, 0, 0),
            "Đóng",
        ),
        (
            Color32::from_rgb(254, 188, 46),
            Color32::from_rgb(90, 50, 0),
            "Thu nhỏ",
        ),
        (
            Color32::from_rgb(40, 200, 64),
            Color32::from_rgb(0, 70, 0),
            "Phóng to",
        ),
    ];
    for (i, (fill, glyph, tip)) in specs.into_iter().enumerate() {
        let c = Pos2::new(rect.left() + 6.0 + i as f32 * 20.0, rect.center().y);
        let hit = Rect::from_center_size(c, Vec2::splat(14.0));
        let resp = ui
            .interact(hit, ui.id().with(("traffic_light", i)), Sense::click())
            .on_hover_text(tip);
        let fill = if focused || group_hovered {
            fill
        } else {
            Color32::from_gray(90)
        };
        ui.painter().circle_filled(c, 6.0, fill);
        ui.painter()
            .circle_stroke(c, 6.0, Stroke::new(0.5_f32, Color32::from_black_alpha(50)));
        if group_hovered {
            let st = Stroke::new(1.2_f32, glyph);
            match i {
                0 => {
                    ui.painter()
                        .line_segment([c + Vec2::new(-2.5, -2.5), c + Vec2::new(2.5, 2.5)], st);
                    ui.painter()
                        .line_segment([c + Vec2::new(2.5, -2.5), c + Vec2::new(-2.5, 2.5)], st);
                }
                1 => {
                    ui.painter()
                        .line_segment([c + Vec2::new(-3.0, 0.0), c + Vec2::new(3.0, 0.0)], st);
                }
                _ => {
                    ui.painter()
                        .line_segment([c + Vec2::new(-2.5, 2.5), c + Vec2::new(2.5, -2.5)], st);
                    ui.painter()
                        .line_segment([c + Vec2::new(-2.5, -0.5), c + Vec2::new(-2.5, 2.5)], st);
                    ui.painter()
                        .line_segment([c + Vec2::new(-2.5, 2.5), c + Vec2::new(0.5, 2.5)], st);
                }
            }
        }
        if resp.clicked() {
            match i {
                0 => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                1 => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                _ => {
                    let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                }
            }
        }
    }
}

/// Deliver banners through the app-owned helper so clicks retain conversation identity.
fn post_os_notification(thread_id: &str, title: &str, body: &str, sound: bool) {
    #[cfg(target_os = "macos")]
    {
        let title = truncate_chars(title, 120).into_owned();
        let body = truncate_chars(body, 320).into_owned();
        let identity = crate::crypto::md5_hex(thread_id).repeat(2);
        std::thread::spawn(move || {
            use std::io::Write;
            let helper = std::env::current_exe().ok().and_then(|p| {
                Some(
                    p.parent()?
                        .parent()?
                        .join("Helpers/DailyNotifier.app/Contents/MacOS/DailyNotifier"),
                )
            });
            if let Some(helper) = helper {
                if let Ok(mut child) = std::process::Command::new(helper)
                    .arg("--notify")
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                {
                    let payload = serde_json::json!({"title":title, "body":body, "id":identity, "sound":sound});
                    if let Some(mut input) = child.stdin.take() {
                        let _ = input.write_all(payload.to_string().as_bytes());
                    }
                    let _ = child.wait();
                }
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (thread_id, title, body, sound);
}

/// Receive opaque notification targets without filesystem work on the UI thread.
fn notification_click() -> Option<String> {
    static RECEIVER: std::sync::OnceLock<std::sync::Mutex<std::sync::mpsc::Receiver<String>>> =
        std::sync::OnceLock::new();
    let receiver = RECEIVER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let Some(home) = std::env::var_os("HOME") else {
                return;
            };
            let path = std::path::PathBuf::from(home)
                .join("Library/Application Support/ctdozlite/notification-click");
            loop {
                if let Ok(id) = std::fs::read_to_string(&path) {
                    let _ = std::fs::remove_file(&path);
                    if tx.send(id).is_err() {
                        break;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        });
        std::sync::Mutex::new(rx)
    });
    receiver.lock().ok()?.try_recv().ok()
}

/// Liquid Glass relies on macOS `NSVisualEffectView`; other platforms keep opaque themes.
fn glass_supported() -> bool {
    cfg!(target_os = "macos")
}

/// Install (`on`) or remove the blurred-desktop vibrancy view behind the egui surface.
/// Returns whether the requested state was applied.
///
/// `window_vibrancy` inserts its `NSVisualEffectView` as a *subview* of the given view. That
/// suits webviews, but here the given view is winit's OpenGL content view: a subview always
/// draws above its parent, so the blur covered the whole egui surface. We therefore move the
/// blur view up to the window's frame view and order it *below* the content view.
#[cfg(target_os = "macos")]
fn set_window_vibrancy(window: &impl raw_window_handle::HasWindowHandle, on: bool) -> bool {
    use objc2_app_kit::{NSView, NSWindowOrderingMode};
    use raw_window_handle::RawWindowHandle;
    use window_vibrancy::{NSVisualEffectMaterial, NSVisualEffectState};

    /// Tag `window_vibrancy` assigns to its blur view (`NS_VIEW_TAG_BLUR_VIEW`, not exported).
    const BLUR_VIEW_TAG: isize = 91_376_254;

    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return false;
    };
    // SAFETY: winit hands out a live `NSView*`; we are on the main thread (egui update / setup).
    let gl_view: &NSView = unsafe { appkit.ns_view.cast().as_ref() };
    let Some(frame_view) = (unsafe { gl_view.superview() }) else {
        return false;
    };
    // SAFETY: plain AppKit view-hierarchy calls on the main thread.
    unsafe {
        if let Some(old) = frame_view.viewWithTag(BLUR_VIEW_TAG) {
            old.removeFromSuperview();
        }
    }
    if !on {
        return true;
    }
    if window_vibrancy::apply_vibrancy(
        window,
        NSVisualEffectMaterial::UnderWindowBackground,
        Some(NSVisualEffectState::Active),
        Some(WINDOW_RADIUS as f64),
    )
    .is_err()
    {
        return false;
    }
    // SAFETY: as above; `blur` is retained across the re-parenting.
    unsafe {
        let Some(blur) = gl_view.viewWithTag(BLUR_VIEW_TAG) else {
            return false;
        };
        blur.removeFromSuperview();
        blur.setFrame(gl_view.frame());
        // Round the blur itself: window_vibrancy's `setCornerRadius` is private API that current
        // macOS ignores, which left square glass corners around the rounded window body.
        blur.setWantsLayer(true);
        let layer: *mut objc2::runtime::AnyObject = objc2::msg_send![&*blur, layer];
        if !layer.is_null() {
            // AppKit points: scale by the UI zoom so it matches the corner egui draws.
            let _: () = objc2::msg_send![layer, setCornerRadius: (WINDOW_RADIUS * 1.18) as f64];
            let _: () = objc2::msg_send![layer, setMasksToBounds: true];
        }
        frame_view.addSubview_positioned_relativeTo(
            &blur,
            NSWindowOrderingMode::NSWindowBelow,
            Some(gl_view),
        );
    }
    true
}

#[cfg(not(target_os = "macos"))]
fn set_window_vibrancy(_window: &impl raw_window_handle::HasWindowHandle, _on: bool) -> bool {
    false
}

/// Every user-visible string in `threads` (names + message bodies), for font coverage checks.
fn thread_texts(threads: &[Thread]) -> impl Iterator<Item = &str> {
    threads.iter().flat_map(|t| {
        std::iter::once(t.name.as_str()).chain(t.messages.iter().map(|m| m.text.as_str()))
    })
}

impl ZaloGuiApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        mut session: Session,
        dev_mode: bool,
        initial_threads: Vec<Thread>,
    ) -> Self {
        setup_custom_fonts(&cc.egui_ctx);
        let mut loaded_settings = AppSettings::load();
        theme::set_icon_style(
            &cc.egui_ctx,
            theme::IconStyle::from_key(&loaded_settings.icon_style),
        );
        if let Ok(mut map) = downloaded_store().write() {
            *map = loaded_settings.downloaded_files.clone();
        }
        if session.proxy_url.is_none() && !loaded_settings.proxy_url.trim().is_empty() {
            let _ = session.set_proxy(Some(&loaded_settings.proxy_url));
        } else if let Some(ref p) = session.proxy_url {
            if loaded_settings.proxy_url.trim().is_empty() {
                loaded_settings.proxy_url = p.clone();
            }
        }
        crate::media::set_selected_mic_device(&loaded_settings.selected_mic_device);
        crate::media::set_selected_camera_device(&loaded_settings.selected_camera_device);
        crate::media::set_mic_gain_percent(loaded_settings.mic_gain_percent);
        crate::session::set_privacy_blocker(
            loaded_settings.block_typing,
            loaded_settings.block_delivered,
            loaded_settings.block_seen,
        );
        let dark_mode = loaded_settings.theme_mode != "light";
        let glass = glass_supported() && loaded_settings.liquid_glass;
        let base_palette = ThemePalette::load_with_custom_override(dark_mode);
        if glass {
            base_palette.into_glass().apply_to_ctx(&cc.egui_ctx);
        } else {
            base_palette.apply_to_ctx(&cc.egui_ctx);
        }
        let vibrancy_on = glass && set_window_vibrancy(cc, true);
        // Borderless transparent window: AppKit's shadow is computed for the rectangular frame and
        // shows as a thin square outline around the rounded body. Drop it; the body draws its own
        // rounded hairline instead.
        disable_native_window_shadow(cc);

        let (tx, rx) = channel();

        // A session the server already refused is not "logged in": jump straight to the
        // login prompt instead of re-testing it (#21).
        let is_logged_in = session.login.is_some() && !session.is_expired_marked();
        let (user_name, user_uin, user_avatar) = session
            .login
            .as_ref()
            .map(|l| {
                (
                    l.display_name.clone(),
                    l.uin.clone(),
                    if l.avatar.is_empty() {
                        None
                    } else {
                        Some(l.avatar.clone())
                    },
                )
            })
            .unwrap_or_default();

        // main() already read threads_cache.json and hydrated it from SQLite; reading, merging,
        // hydrating and rewriting the cache again here doubled startup I/O.
        let mut threads = initial_threads;
        if let Some(li) = session.login.as_ref() {
            if !li.send2me_id.is_empty() {
                for t in threads.iter_mut() {
                    if !t.is_group && t.name.contains("Cloud của tôi") {
                        t.id = li.send2me_id.clone();
                    }
                }
            }
        }
        let now_ts = crate::login::unix_now();
        for t in threads.iter_mut() {
            if let Some(&until) = loaded_settings.muted_threads.get(&t.id) {
                if until == -1 || until > now_ts {
                    t.presentation.muted = true;
                    t.unread.muted = true;
                    t.unread.muted_until = (until > 0).then_some(until);
                }
            } else if t.unread.is_muted() {
                t.presentation.muted = true;
            }
        }
        crate::chat::deduplicate_threads(&mut threads, None);
        // Conversations deleted here stay deleted across restarts: the worker's poll keeps its
        // own copy and rewrites them (with their old messages) into the cache and database.
        threads.retain_mut(
            |t| match tombstone_for(&loaded_settings.deleted_threads, &t.id) {
                Some(at) => apply_delete_cutoff(t, at, true),
                None => true,
            },
        );

        let selected_thread_index = threads
            .iter()
            .enumerate()
            .max_by_key(|(_, t)| t.messages.last().map(|m| m.ts).unwrap_or(0))
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        let app = Self {
            dark_mode,
            theme_dirty: false,
            vibrancy_on,
            is_logged_in,
            user_name,
            user_uin,
            user_avatar,
            login_tab: LoginTab::QrCode,
            phone_input: String::new(),
            password_input: String::new(),
            login_in_progress: false,
            login_status_msg: String::new(),
            qr_texture: None,
            qr_status: "Đang tạo mã QR đăng nhập…".into(),
            threads,
            selected_thread_index,
            search_query: String::new(),
            input_message: String::new(),
            replying_to: None,
            forward_msg: None,
            forward_filter: String::new(),
            filter_starred_only: false,
            active_call: None,
            image_viewer: None,
            status_msg: "Trực tuyến · Sẵn sàng".into(),
            info_drawer_open: loaded_settings.info_drawer_open,
            profile_menu_open: false,
            wipe_confirm_open: false,
            alias_editor: None,
            search_open: false,
            search_focus_pending: false,
            emoji_picker_open: false,
            emoji_tab: 0,
            emoji_btn_rect: Rect::NOTHING,
            global_search_tab: 0,
            composer_rect_last: Rect::NOTHING,
            overlay_rects_last: Vec::new(),
            header_rect_last: Rect::NOTHING,
            in_chat_results_query: String::new(),
            in_chat_results_hidden: false,
            in_chat_results_limit: 20,
            drawer_collapsed: HashSet::new(),
            drawer_show_all: HashSet::new(),
            drawer_confirm_delete: false,
            leave_group_confirm: None,
            zpc_enriched: HashMap::new(),
            leaving_groups: HashSet::new(),
            alias_original_names: HashMap::new(),
            toast_notice: None,
            phone_modal_open: false,
            phone_modal_input: String::new(),
            phone_lookup_loading: false,
            phone_lookup_result: None,
            phone_lookup_inline: None,
            phone_lookup_error: None,
            in_chat_search_open: false,
            in_chat_search_query: String::new(),
            in_chat_search_focus_pending: false,
            sticker_picker_open: false,
            sticker_tab: StickerTab::Popular,
            sticker_textures: HashMap::new(),
            loading_stickers: HashSet::new(),
            emoji_textures: load_default_emoji_textures(&cc.egui_ctx),
            app_logo_texture: load_app_logo_texture(&cc.egui_ctx),
            sticker_store_packs: Vec::new(),
            sticker_store_loading: false,
            sticker_store_filter: String::new(),
            sticker_pack_items: FEATURED_PACK_TABS
                .iter()
                .filter_map(|&(cid, _)| builtin_pack_stickers(cid).map(|items| (cid, items)))
                .collect(),
            loading_sticker_packs: HashSet::new(),
            pinned_sticker_packs: FEATURED_PACK_TABS
                .iter()
                .map(|&(cid, title)| (cid, title.to_string()))
                .collect(),
            sticker_search_query: String::new(),
            sticker_search_results: Vec::new(),
            sticker_search_loading: false,
            last_sticker_search_dispatched: String::new(),
            last_sticker_search_edit_at: std::time::Instant::now(),
            sticker_suggest_keywords: HashSet::new(),
            composer_sticker_cache: HashMap::new(),
            composer_sticker_loading: HashSet::new(),
            dismissed_sticker_suggest_for: String::new(),
            last_composer_text: String::new(),
            last_composer_edit_at: std::time::Instant::now(),
            composer_sticker_selected: None,
            composer_focus_pending: false,
            pending_clipboard_image: None,
            pending_paste_files: Vec::new(),
            composer_mentions: Vec::new(),
            mention_selected: 0,
            attach_menu_open: false,
            select_mode: false,
            selected_msgs: std::collections::HashSet::new(),
            msg_details: None,
            pinned_topics: std::collections::HashMap::new(),
            pinned_requested_for: String::new(),
            pinned_expanded: false,
            group_members: std::collections::HashMap::new(),
            members_panel_open: false,
            account_card: None,
            bg_texture: None,
            send_fx: None,
            bg_requested: false,
            unread_counts: HashMap::new(),
            session_started_ms: (crate::login::unix_now() * 1000),
            link_metadata_cache: HashMap::new(),
            loading_link_previews: HashSet::new(),
            dismissed_link_preview_for: String::new(),
            friend_status_map: HashMap::new(),
            checking_friend_status: HashSet::new(),
            user_online_map: HashMap::new(),
            allow_quit: false,
            settings_modal_open: false,
            settings_tab: SettingsTab::General,
            notification_authorization: Default::default(),
            settings: loaded_settings,
            settings_blur_tex: None,
            settings_blur_req_at: None,
            update_checking: false,
            update_downloading: false,
            update_progress: None,
            update_status: String::new(),
            update_info: None,
            update_modal_open: false,
            update_banner_dismissed: false,
            last_notified_update_version: None,
            dev_mode,
            dev_panel_open: false,
            dev_tab: DevTab::Logs,
            selected_log: None,
            scanner_findings: Vec::new(),
            feed_modal_open: false,
            feed_modal_tab: FeedModalTab::Profile,
            feed_target_uid: String::new(),
            feed_loading_profile: false,
            feed_profile: None,
            feed_profile_error: None,
            feed_loading_avatars: false,
            feed_avatars: Vec::new(),
            feed_avatars_error: None,
            feed_loading_timeline: false,
            feed_items: Vec::new(),
            feed_friend_profiles: Vec::new(),
            feed_search_query: String::new(),
            feed_pkt_owner_id: String::new(),
            feed_pkt_viewer_id: String::new(),
            feed_pkt_tab: 0,
            feed_pkt_page: 1,
            feed_pkt_count: 20,
            contacts_view_open: false,
            contacts_sub_tab: ContactsSubTab::Friends,
            contacts_search_query: String::new(),
            schedule_modal_open: false,
            schedule_custom_input: String::new(),
            schedule_editing_id: None,
            scheduled_items: crate::scheduler::list_pending_scheduled("zalo_history.db", None)
                .unwrap_or_default(),
            phone_sync_progress: 0.0,
            phone_sync_status: String::new(),
            phone_sync_current_only: false,
            last_typing_sent_at: None,
            last_synced_icon_unread: None,
            event_rx: rx,
            event_tx: tx.clone(),
            chat_render_window: (String::new(), CHAT_RENDER_STEP),
            quote_jump: None,
            ime_active: false,
            ime_just_committed: false,
            ime_enter_commit: false,
            selection_fill_saved: None,
            history_reached_start: HashSet::new(),
            force_scroll_to_bottom: true,
            pin_chat_bottom_until: f64::INFINITY,
        };
        let mut app = app;
        let _ = app.reload_custom_sticker_packs();

        // Clean up any leftover `.old` executable from a previous in-place self-update
        if let Ok(exe_path) = std::env::current_exe() {
            let old_path = exe_path.with_extension("exe.old");
            let _ = std::fs::remove_file(&old_path);
            let old_unix = exe_path.with_extension("old");
            let _ = std::fs::remove_file(&old_unix);
        }

        app.publish_known_threads();
        spawn_session_worker(
            session,
            GuiTx {
                tx,
                ctx: cc.egui_ctx.clone(),
            },
            cc.egui_ctx.clone(),
        );
        ensure_cjk_font(&cc.egui_ctx, thread_texts(&app.threads));

        if let Some(wtx) = WORKER_TX.get() {
            if is_logged_in {
                let _ = wtx.send(WorkerCmd::RefreshThreads);
            } else {
                let _ = wtx.send(WorkerCmd::RequestQr);
            }
            let _ = wtx.send(WorkerCmd::CheckForUpdates { silent: true });
        }
        app
    }

    fn palette(&self) -> ThemePalette {
        let base = ThemePalette::load_with_custom_override(self.dark_mode);
        // A wallpaper also needs translucent surfaces to be visible behind the panels.
        if self.glass_active() || self.bg_texture.is_some() {
            base.into_glass()
        } else {
            base
        }
    }

    /// Liquid Glass needs the vibrancy backdrop; without it translucent panels would show
    /// the raw (unblurred) desktop, so glass colors follow the installed vibrancy state.
    fn glass_active(&self) -> bool {
        self.vibrancy_on
    }

    pub(super) fn reload_custom_sticker_packs(&mut self) -> usize {
        let packs = load_custom_sticker_packs();
        let count = packs.len();
        for p in packs {
            if !self
                .pinned_sticker_packs
                .iter()
                .any(|(cid, _)| *cid == p.cate_id)
            {
                self.pinned_sticker_packs.push((p.cate_id, p.name.clone()));
            }
            if let Some(items) = builtin_pack_stickers(p.cate_id) {
                self.sticker_pack_items.insert(p.cate_id, items);
            }
        }
        count
    }

    pub(super) fn refresh_scheduled_items(&mut self) {
        self.scheduled_items =
            crate::scheduler::list_pending_scheduled("zalo_history.db", None).unwrap_or_default();
    }

    /// Hand the worker's poll timer the thread ids/names it maps incoming messages onto.
    fn publish_known_threads(&self) {
        if let Ok(mut known) = KNOWN_THREADS.lock() {
            *known = self
                .threads
                .iter()
                .map(|t| (t.id.clone(), t.name.clone(), t.is_group))
                .collect();
        }
    }

    fn persist_threads_async(&self) {
        self.publish_known_threads();
        let snapshot = self.threads.clone();
        if let Some(wtx) = WORKER_TX.get() {
            let _ = wtx.send(WorkerCmd::PersistThreads(snapshot));
        }
    }

    fn check_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                GuiEvent::NotificationAuthorization { request_id, result } => {
                    self.notification_authorization.finish(request_id, result);
                }
                GuiEvent::QrReady { qr_image, .. } => {
                    self.qr_texture =
                        Some(ctx.load_texture("qr_code_tex", qr_image, TextureOptions::NEAREST));
                    self.qr_status = "Mở ứng dụng Zalo trên điện thoại và quét mã QR".into();
                    self.login_in_progress = false;
                }
                GuiEvent::QrScanned { name } => {
                    self.qr_status =
                        format!("Đã quét bởi {name} — Vui lòng bấm 'Đăng nhập' trên điện thoại…");
                }
                GuiEvent::LoginSuccess { name, uin, avatar } => {
                    self.is_logged_in = true;
                    if let Some(tx) = WORKER_TX.get() {
                        let _ = tx.send(WorkerCmd::FetchAliases);
                    }
                    // Same account (session restore at start-up): keep the conversations already
                    // loaded from the local database instead of emptying the list and rebuilding
                    // it from the server, which made every launch look like a full reload.
                    let switched_account = !self.user_uin.is_empty() && self.user_uin != uin;
                    let first_login = self.user_uin.is_empty() && self.threads.is_empty();
                    // Groups left from the phone (or removed) disappear here too.
                    if let Some(tx) = WORKER_TX.get() {
                        let gids: Vec<String> = self
                            .threads
                            .iter()
                            .filter(|t| t.is_group)
                            .map(|t| t.id.clone())
                            .collect();
                        if !gids.is_empty() {
                            let _ = tx.send(WorkerCmd::CheckGroupMembership { thread_ids: gids });
                        }
                    }
                    if switched_account || first_login || zalo_pc_media().is_none() {
                        start_zalo_pc_media_scan(uin.clone(), ctx.clone());
                        self.zpc_enriched.clear();
                    }
                    self.user_name = name;
                    self.user_uin = uin;
                    if avatar.is_some() {
                        self.user_avatar = avatar;
                    }
                    if switched_account || first_login {
                        self.threads.clear();
                        self.unread_counts.clear();
                        self.history_reached_start.clear();
                        self.selected_thread_index = 0;
                    }
                    self.login_in_progress = false;
                    self.composer_sticker_cache.retain(|_, v| !v.is_empty());
                    self.status_msg = "Trực tuyến · Đồng bộ thời gian thực".into();
                }
                GuiEvent::LoginFailed(err) => {
                    self.login_in_progress = false;
                    self.login_status_msg = format!("Lỗi đăng nhập: {err}");
                }
                GuiEvent::ThreadsUpdated(updates) => {
                    ensure_cjk_font(ctx, thread_texts(&updates));
                    if self.is_logged_in && !updates.is_empty() {
                        let prev_selected = self.threads.get(self.selected_thread_index).cloned();
                        let live_ids: HashSet<String> = self
                            .threads
                            .iter()
                            .chain(updates.iter())
                            .filter(|u| !u.id.is_empty() && u.id != "0")
                            .map(|u| u.id.clone())
                            .collect();
                        let update_ids: HashSet<String> = updates
                            .iter()
                            .filter(|u| !u.id.is_empty() && u.id != "0")
                            .map(|u| u.id.clone())
                            .collect();

                        let active_tid = prev_selected
                            .as_ref()
                            .map(|t| t.id.clone())
                            .unwrap_or_default();
                        let is_focused = ctx.input(|i| i.focused);
                        let mut new_incoming_alert = false;
                        // (conversation name, newest incoming text) for the OS banner.
                        let mut alert_preview: Option<(String, String, String)> = None;

                        let now_ts = crate::login::unix_now();
                        for mut nt in updates {
                            if let Some(deleted_at) =
                                tombstone_for(&self.settings.deleted_threads, &nt.id)
                            {
                                // Deleted here: the cut-off stays for good, because the worker
                                // keeps its pre-deletion copy and resends it on every refresh.
                                let revived = !self.threads.iter().any(|t| t.id == nt.id);
                                if !apply_delete_cutoff(&mut nt, deleted_at, revived) {
                                    continue;
                                }
                            }
                            self.apply_alias_to_thread(&mut nt);
                            let settings_muted_until = self
                                .settings
                                .muted_threads
                                .get(&nt.id)
                                .copied()
                                .filter(|&u| u == -1 || u > now_ts);
                            if let Some(until) = settings_muted_until {
                                nt.presentation.muted = true;
                                nt.unread.muted = true;
                                nt.unread.muted_until = (until > 0).then_some(until);
                            } else if nt.unread.is_muted() {
                                nt.presentation.muted = true;
                            }
                            if let Some(existing) = self.threads.iter_mut().find(|t| t.id == nt.id)
                            {
                                let is_thread_muted = settings_muted_until.is_some()
                                    || existing.presentation.muted
                                    || existing.unread.is_muted()
                                    || nt.presentation.muted
                                    || nt.unread.is_muted();
                                let old_len = existing.messages.len();
                                let old_newest_ts =
                                    existing.messages.iter().map(|m| m.ts).max().unwrap_or(0);
                                let old_oldest_ts = existing
                                    .messages
                                    .iter()
                                    .map(|m| m.ts)
                                    .filter(|t| *t > 0)
                                    .min()
                                    .unwrap_or(i64::MAX);
                                crate::chat::merge_thread_into(existing, nt, true);
                                if is_thread_muted {
                                    existing.presentation.muted = true;
                                    existing.unread.muted = true;
                                    if let Some(until) = settings_muted_until {
                                        existing.unread.muted_until = (until > 0).then_some(until);
                                    }
                                }
                                let added = existing.messages.len().saturating_sub(old_len);
                                if added > 0 {
                                    // Count only genuinely new incoming messages: newer than the
                                    // thread's read cursor, or (no cursor yet) newer than anything
                                    // held before and received after this app session started.
                                    // History pages merged at startup used to be counted, so read
                                    // conversations came back as unread on every launch.
                                    let cursor = existing.unread.last_seen_id.clone();
                                    let started = self.session_started_ms;
                                    let inc_cnt = existing
                                        .messages
                                        .iter()
                                        .rev()
                                        .take(added)
                                        .filter(|m| !m.is_outgoing)
                                        .filter(|m| match &cursor {
                                            Some(c) => crate::unread::compare_ids(&m.id, c)
                                                .is_some_and(|o| o.is_gt()),
                                            None => m.ts > old_newest_ts && m.ts >= started,
                                        })
                                        .count();
                                    if existing.id == active_tid && is_focused {
                                        // Arrived while this conversation is on screen: read.
                                        let msgs = existing.messages.clone();
                                        existing.unread.read_visible(&msgs);
                                    }
                                    if inc_cnt > 0 && (existing.id != active_tid || !is_focused) {
                                        *self
                                            .unread_counts
                                            .entry(existing.id.clone())
                                            .or_insert(0) += inc_cnt;
                                        if !is_thread_muted {
                                            new_incoming_alert = true;
                                            if let Some(m) = existing
                                                .messages
                                                .iter()
                                                .rev()
                                                .find(|m| !m.is_outgoing)
                                            {
                                                let who =
                                                    if existing.is_group && !m.sender.is_empty() {
                                                        format!("{}: ", m.sender)
                                                    } else {
                                                        String::new()
                                                    };
                                                alert_preview = Some((
                                                    existing.id.clone(),
                                                    existing.name.clone(),
                                                    format!("{who}{}", m.text.trim()),
                                                ));
                                            }
                                        }
                                    }
                                    // Only history pages are "older messages"; a new incoming
                                    // message used to trigger this toast too.
                                    let older = existing
                                        .messages
                                        .iter()
                                        .filter(|m| m.ts > 0 && m.ts < old_oldest_ts)
                                        .count();
                                    if existing.id == self.chat_render_window.0 && older > 0 {
                                        self.chat_render_window.1 += older;
                                        self.show_toast(format!("Đã tải thêm {older} tin nhắn cũ"));
                                    }
                                }
                            } else if let Some(existing) = self.threads.iter_mut().find(|t| {
                                !update_ids.contains(&t.id)
                                    && crate::chat::threads_represent_same_contact(t, &nt)
                            }) {
                                crate::chat::merge_thread_into(existing, nt, true);
                            } else if !nt.id.is_empty() && nt.id != "0" {
                                self.threads.push(nt);
                            }
                        }

                        if new_incoming_alert && self.settings.enable_notifications {
                            let total_unread: usize = self.unread_counts.values().sum();
                            let title = if total_unread > 0 {
                                format!("({total_unread}) ctdozlite")
                            } else {
                                "ctdozlite".to_string()
                            };
                            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
                            if !is_focused {
                                ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                                    egui::UserAttentionType::Informational,
                                ));
                                // System banner (previously only the title and Dock bounce).
                                if let Some((thread_id, title, body)) = alert_preview.take() {
                                    post_os_notification(
                                        &thread_id,
                                        &title,
                                        &body,
                                        self.settings.play_notification_sound,
                                    );
                                }
                            }
                        }
                        crate::chat::deduplicate_threads(&mut self.threads, Some(&live_ids));

                        // Restore selected thread by exact `id` first so contacts sharing the
                        // same Zalo display name never jump to each other's chat window (#63).
                        if let Some(prev) = prev_selected {
                            if let Some(idx) = self.threads.iter().position(|t| t.id == prev.id) {
                                self.selected_thread_index = idx;
                            } else if let Some(idx) = self
                                .threads
                                .iter()
                                .position(|t| crate::chat::threads_represent_same_contact(t, &prev))
                            {
                                self.selected_thread_index = idx;
                            }
                        }
                        if self.selected_thread_index >= self.threads.len() {
                            self.selected_thread_index = self.threads.len().saturating_sub(1);
                        }
                        self.persist_threads_async();
                    }
                    self.status_msg = format!("Trực tuyến · Cập nhật {}", crate::ui::clock());
                }
                GuiEvent::SessionExpired(reason) => {
                    self.is_logged_in = false;
                    self.qr_texture = None;
                    self.qr_status =
                        format!("{reason} — Quét mã QR bên dưới để khôi phục phiên trực tuyến.");
                    self.status_msg = format!("{reason} (Lịch sử tin nhắn vẫn được bảo toàn)");
                }
                GuiEvent::SessionUnreachable(reason) => {
                    // Only the network failed: stay logged in, keep the thread list, and
                    // let the next poll retry (#21).
                    self.status_msg = format!("⚠️ {reason} — đang thử lại…");
                }
                GuiEvent::LocalDataWiped => {
                    self.threads.clear();
                    self.selected_thread_index = 0;
                    self.status_msg = "Đã xoá dữ liệu tin nhắn cục bộ (session giữ nguyên)".into();
                }
                GuiEvent::LoggedOut => {
                    self.is_logged_in = false;
                    self.user_name.clear();
                    self.user_uin.clear();
                    self.threads.clear();
                    self.selected_thread_index = 0;
                    self.qr_texture = None;
                    self.qr_status = "Đang xin mã QR mới…".into();
                    self.status_msg = "Đã đăng xuất".into();
                }
                GuiEvent::AccountSwitched {
                    uin,
                    name,
                    avatar,
                    threads,
                } => {
                    self.is_logged_in = true;
                    self.user_uin = uin;
                    self.user_name = name.clone();
                    self.user_avatar = avatar;
                    self.threads = threads;
                    self.selected_thread_index = 0;
                    self.status_msg = format!("Đã chuyển sang tài khoản {name}");
                    self.show_toast(format!("Đã chuyển sang tài khoản {name}"));
                    self.history_reached_start.clear();
                }
                GuiEvent::PinnedTopicsLoaded { thread_id, topics } => {
                    self.pinned_topics.insert(thread_id, topics);
                }
                GuiEvent::GroupPostingLoaded { thread_id, policy } => {
                    ctx.data_mut(|d| {
                        d.insert_temp(Id::new(("posting_ready", &thread_id)), policy.is_some())
                    });
                    if let Some(policy) = policy {
                        if let Some(thread) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                            thread.presentation.posting = policy;
                        }
                    } else {
                        self.show_toast("Chưa kiểm tra được quyền gửi tin của nhóm.");
                    }
                }
                GuiEvent::GroupMembersLoaded { thread_id, members } => {
                    self.group_members.insert(thread_id, members);
                }
                GuiEvent::ReactionsReceived(events) => {
                    self.apply_live_reactions(ctx, events);
                }
                GuiEvent::AliasesLoaded(map) => {
                    if let Ok(mut store) = alias_store().write() {
                        *store = map;
                    }
                    let mut threads = std::mem::take(&mut self.threads);
                    for t in threads.iter_mut() {
                        self.apply_alias_to_thread(t);
                    }
                    self.threads = threads;
                }
                GuiEvent::GroupsRemoved(ids) => {
                    let mut n = 0;
                    for gid in ids {
                        let bare = bare_thread_id(&gid).to_string();
                        let found: Vec<String> = self
                            .threads
                            .iter()
                            .filter(|t| t.is_group && bare_thread_id(&t.id) == bare)
                            .map(|t| t.id.clone())
                            .collect();
                        for tid in found {
                            self.remove_thread_locally(&tid);
                            n += 1;
                        }
                    }
                    if n > 0 {
                        self.show_toast(format!("Đã ẩn {n} nhóm bạn không còn tham gia"));
                    }
                }
                GuiEvent::GroupLeft {
                    thread_id,
                    name,
                    res,
                } => {
                    self.leaving_groups.remove(&thread_id);
                    match res {
                        Ok(()) => {
                            self.remove_thread_locally(&thread_id);
                            self.show_toast(format!("👋 Đã rời nhóm \"{name}\""));
                        }
                        Err(e) => {
                            self.show_toast(format!("Không rời được nhóm \"{name}\": {e}"));
                        }
                    }
                    ctx.request_repaint();
                }
                GuiEvent::AliasUpdated {
                    friend_id,
                    alias,
                    res,
                } => {
                    if let Some(ed) = self.alias_editor.as_mut().filter(|e| e.uid == friend_id) {
                        ed.saving = false;
                    }
                    match res {
                        Ok(()) => {
                            let alias = alias.trim().to_string();
                            if let Ok(mut store) = alias_store().write() {
                                if alias.is_empty() {
                                    store.remove(&friend_id);
                                } else {
                                    store.insert(friend_id.clone(), alias.clone());
                                }
                            }
                            let mut threads = std::mem::take(&mut self.threads);
                            for t in threads.iter_mut().filter(|t| t.id == friend_id) {
                                if alias.is_empty() {
                                    if let Some(orig) = self.alias_original_names.remove(&friend_id)
                                    {
                                        t.name = orig;
                                    }
                                } else {
                                    self.apply_alias_to_thread(t);
                                }
                            }
                            self.threads = threads;
                            self.alias_editor = None;
                            self.show_toast(if alias.is_empty() {
                                "Đã xoá tên gợi nhớ"
                            } else {
                                "Đã đổi tên gợi nhớ"
                            });
                        }
                        Err(e) => self.show_toast(format!("Không đổi được tên gợi nhớ: {e}")),
                    }
                }
                GuiEvent::BackgroundImageReady {
                    path,
                    image,
                    picked,
                } => match image {
                    Some(img) => {
                        self.bg_texture =
                            Some(ctx.load_texture("app_background", img, TextureOptions::LINEAR));
                        if self.settings.app_background != path {
                            self.settings.app_background = path;
                            self.settings.save();
                        }
                        self.theme_dirty = true;
                    }
                    None if picked => self.show_toast("Không đọc được ảnh nền đã chọn"),
                    None => {}
                },
                GuiEvent::AccountInfoLoaded { uid, res } => {
                    if let Some(card) = self.account_card.as_mut().filter(|c| c.uid == uid) {
                        card.loading = false;
                        match res {
                            Ok(p) => card.profile = Some(p),
                            Err(e) => card.error = Some(e),
                        }
                    }
                }
                GuiEvent::MessageSent {
                    client_message_id,
                    thread_id,
                    text,
                    msg_id,
                } => {
                    self.finish_send_fx(ctx, &thread_id, true);
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.preview = text.clone();
                        let now = crate::ui::clock();
                        t.time = now.clone();
                        let final_id = if msg_id.is_empty() {
                            format!("out_{client_message_id}")
                        } else {
                            msg_id
                        };
                        merge_sent_message(
                            t,
                            ChatMessage {
                                id: final_id,
                                client_message_id,
                                sender: "Tôi".into(),
                                text,
                                time: now,
                                is_outgoing: true,
                                ts: (crate::login::unix_now() * 1000),
                                ..Default::default()
                            },
                        );
                        self.persist_threads_async();
                    }
                    self.status_msg = format!("Đã gửi tin nhắn lúc {}", crate::ui::clock());
                }
                GuiEvent::StickerSent {
                    client_message_id,
                    thread_id,
                    msg_id,
                    sticker_id,
                    cate_id,
                    sticker_type,
                } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.preview = "[Nhãn dán]".into();
                        let now = crate::ui::clock();
                        t.time = now.clone();
                        let final_id = if msg_id.is_empty() {
                            format!("out_{client_message_id}")
                        } else {
                            msg_id
                        };
                        merge_sent_message(
                            t,
                            ChatMessage {
                                id: final_id,
                                client_message_id,
                                sender: "Tôi".into(),
                                text: "[Nhãn dán]".into(),
                                time: now,
                                is_outgoing: true,
                                ts: (crate::login::unix_now() * 1000),
                                sticker_id: Some(sticker_id),
                                sticker_cate_id: Some(cate_id),
                                sticker_type: Some(sticker_type),
                                ..Default::default()
                            },
                        );
                        self.persist_threads_async();
                    }
                    self.status_msg = format!("Đã gửi nhãn dán lúc {}", crate::ui::clock());
                }
                GuiEvent::AttachmentSent { thread_id, msg } => {
                    self.finish_send_fx(ctx, &thread_id, true);
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.preview = msg.text.clone();
                        t.time = msg.time.clone();
                        merge_sent_message(t, msg);
                        self.persist_threads_async();
                    }
                    self.status_msg = format!("Đã gửi đính kèm lúc {}", crate::ui::clock());
                }
                GuiEvent::QuoteSent {
                    client_message_id,
                    thread_id,
                    text,
                    msg_id,
                    quoted,
                } => {
                    self.finish_send_fx(ctx, &thread_id, true);
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.preview = text.clone();
                        let now = crate::ui::clock();
                        t.time = now.clone();
                        let final_id = if msg_id.is_empty() {
                            format!("out_{client_message_id}")
                        } else {
                            msg_id
                        };
                        merge_sent_message(
                            t,
                            ChatMessage {
                                id: final_id,
                                client_message_id,
                                sender: "Tôi".into(),
                                text,
                                time: now,
                                is_outgoing: true,
                                ts: (crate::login::unix_now() * 1000),
                                reply_to_id: Some(quoted.id.clone()),
                                reply_to_sender: Some(quoted.sender.clone()),
                                reply_to_text: Some(quoted.text.clone()),
                                ..Default::default()
                            },
                        );
                        self.persist_threads_async();
                    }
                    self.status_msg = format!("Đã gửi trả lời lúc {}", crate::ui::clock());
                }
                GuiEvent::Reacted {
                    thread_id,
                    msg_id,
                    ts,
                    reaction,
                } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        if let Some(m) = t.messages.iter_mut().find(|m| {
                            (!m.id.is_empty() && m.id == msg_id) || (m.id.is_empty() && m.ts == ts)
                        }) {
                            m.reactions = vec![reaction];
                        }
                        self.persist_threads_async();
                    }
                    self.status_msg = "Đã thả cảm xúc".into();
                }
                GuiEvent::MessageDeleted {
                    thread_id,
                    msg_id,
                    ts,
                } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.messages.retain(|m| {
                            !((!msg_id.is_empty() && m.id == msg_id)
                                || (msg_id.is_empty() && m.ts == ts))
                        });
                        if let Some(last) = t.messages.last() {
                            t.preview = last.text.clone();
                        }
                        self.persist_threads_async();
                    }
                    self.status_msg = "Đã xóa tin nhắn ở phía bạn".into();
                }
                GuiEvent::MessageRecalled {
                    thread_id,
                    msg_id,
                    ts,
                } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        if let Some(m) = t.messages.iter_mut().find(|m| {
                            (!msg_id.is_empty() && m.id == msg_id)
                                || (msg_id.is_empty() && m.ts == ts)
                        }) {
                            m.text = "[Tin nhắn đã thu hồi]".into();
                            m.sticker_id = None;
                            m.link_url = None;
                            m.link_thumb = None;
                        }
                        if let Some(last) = t.messages.last() {
                            t.preview = last.text.clone();
                        }
                        self.persist_threads_async();
                    }
                    self.status_msg = "Đã thu hồi tin nhắn với mọi người".into();
                }
                GuiEvent::StickerImageReady { key, image } => {
                    self.loading_stickers.remove(&key);
                    let tex = ctx.load_texture(&key, image, TextureOptions::LINEAR);
                    self.sticker_textures.insert(key, tex);
                }
                GuiEvent::StickerStoreLoaded(packs) => {
                    self.sticker_store_loading = false;
                    let count = packs.len();
                    self.sticker_store_packs = packs;
                    self.status_msg = format!("Đã tải Kho Nhãn Dán Zalo ({count} bộ nhãn)");
                }
                GuiEvent::StickerPackItemsLoaded {
                    cate_id,
                    name,
                    items,
                } => {
                    self.loading_sticker_packs.remove(&cate_id);
                    let count = items.len();
                    if !self
                        .pinned_sticker_packs
                        .iter()
                        .any(|(id, _)| *id == cate_id)
                    {
                        self.pinned_sticker_packs.insert(0, (cate_id, name.clone()));
                    }
                    self.sticker_pack_items.insert(cate_id, items);
                    self.status_msg = format!("Đã mở bộ nhãn \"{name}\" ({count} nhãn dán)");
                }
                GuiEvent::StickerSearchResults { keyword, items } => {
                    self.sticker_search_loading = false;
                    let count = items.len();
                    self.sticker_search_results = items;
                    self.status_msg =
                        format!("Tìm thấy {count} nhãn dán cho từ khóa \"{keyword}\"");
                }
                GuiEvent::StickerSuggestKeywordsLoaded(kws) => {
                    for k in kws {
                        self.sticker_suggest_keywords.insert(k);
                    }
                }
                GuiEvent::ComposerStickerSuggestions { keyword, items } => {
                    self.composer_sticker_loading.remove(&keyword);
                    self.composer_sticker_cache.insert(keyword, items);
                }
                GuiEvent::LinkPreviewReady(meta) => {
                    self.loading_link_previews.remove(&meta.url);
                    if !meta.thumb.is_empty() {
                        let key = format!("thumb_{}", crate::crypto::md5_hex(&meta.thumb));
                        if !self.sticker_textures.contains_key(&key)
                            && self.loading_stickers.insert(key.clone())
                        {
                            if let Some(tx) = WORKER_TX.get() {
                                let _ = tx.send(WorkerCmd::FetchStickerImage {
                                    key,
                                    url: meta.thumb.clone(),
                                });
                            }
                        }
                    }
                    let mut updated_any = false;
                    for t in &mut self.threads {
                        for m in &mut t.messages {
                            let matches_url = m.link_url.as_deref() == Some(meta.url.as_str())
                                || crate::chat::extract_first_url(&m.text).as_deref()
                                    == Some(meta.url.as_str());
                            if matches_url {
                                if m.link_url.is_none() {
                                    m.link_url = Some(meta.url.clone());
                                    updated_any = true;
                                }
                                if (m.link_title.is_none()
                                    || m.link_title.as_deref() == Some(meta.url.as_str()))
                                    && !meta.title.is_empty()
                                {
                                    m.link_title = Some(meta.title.clone());
                                    updated_any = true;
                                }
                                if m.link_desc.is_none() && !meta.desc.is_empty() {
                                    m.link_desc = Some(meta.desc.clone());
                                    updated_any = true;
                                }
                                let old_thumb_expired = m
                                    .link_thumb
                                    .as_deref()
                                    .map(|th| {
                                        th.is_empty()
                                            || (th.contains("zpcloud.zdn.vn")
                                                && !self.sticker_textures.contains_key(&format!(
                                                    "thumb_{}",
                                                    crate::crypto::md5_hex(th)
                                                )))
                                    })
                                    .unwrap_or(true);
                                if old_thumb_expired && !meta.thumb.is_empty() {
                                    m.link_thumb = Some(meta.thumb.clone());
                                    updated_any = true;
                                }
                                if m.link_src.is_none() && !meta.src.is_empty() {
                                    m.link_src = Some(meta.src.clone());
                                    updated_any = true;
                                }
                            }
                        }
                    }
                    self.link_metadata_cache.insert(meta.url.clone(), meta);
                    if updated_any {
                        self.persist_threads_async();
                    }
                }
                GuiEvent::FriendStatusChecked {
                    user_id,
                    status,
                    online,
                } => {
                    self.checking_friend_status.remove(&user_id);
                    self.friend_status_map.insert(user_id.clone(), status);
                    if let Some(onl) = online {
                        self.user_online_map.insert(user_id.clone(), onl);
                    }
                    if let Some(ref mut prof) = self.phone_lookup_result {
                        if prof.user_id == user_id {
                            prof.friendship = status;
                        }
                    }
                }
                GuiEvent::PhoneLookupSuccess(res) => {
                    self.phone_lookup_loading = false;
                    self.phone_lookup_error = None;
                    if self.phone_lookup_inline.is_none() {
                        self.phone_modal_open = true;
                        self.show_toast(format!("Đã tìm thấy tài khoản: {}", res.display_name));
                    }
                    self.friend_status_map
                        .insert(res.user_id.clone(), res.friendship);
                    self.phone_lookup_result = Some(res);
                }
                GuiEvent::PhoneLookupFailed(err) => {
                    self.phone_lookup_loading = false;
                    self.phone_lookup_result = None;
                    if self.phone_lookup_inline.is_none() {
                        self.phone_modal_open = true;
                    }
                    self.phone_lookup_error = Some(err);
                }
                GuiEvent::SendFailed {
                    thread_id,
                    text,
                    error,
                } => {
                    self.finish_send_fx(ctx, &thread_id, false);
                    if self.input_message.is_empty() && !text.is_empty() {
                        self.input_message = text;
                    }
                    self.status_msg = format!("Không gửi được: {error}");
                }
                GuiEvent::CallStateChanged(state) => {
                    if state == "free" {
                        // Keep modal open if there is a rejection reason so the user can read it
                        let has_error = self
                            .active_call
                            .as_ref()
                            .and_then(|c| c.error_reason.as_ref())
                            .is_some();
                        self.sticker_textures.remove("call_remote_video");
                        self.sticker_textures.remove("call_remote_video_1");
                        self.sticker_textures.remove("call_remote_video_2");
                        self.sticker_textures.remove("call_remote_video_3");
                        self.sticker_textures.remove("call_local_video");
                        if !has_error {
                            self.active_call = None;
                        }
                    } else if let Some(ref mut call) = self.active_call {
                        if state == "incall" || state == "connected" {
                            call.is_incoming_ringing = false;
                        }
                        if call.error_reason.is_none() {
                            call.state_label = match state.as_str() {
                                "calling" | "ringing" => "Đang đổ chuông…".into(),
                                "incoming_ringing" => "Cuộc gọi đến đang đổ chuông…".into(),
                                "connecting" => "Đang kết nối luồng âm thanh/video…".into(),
                                "incall" | "connected" => {
                                    "Đang trong cuộc gọi (Pure-Rust VoIP)".into()
                                }
                                _ => format!("Đang kết nối ({state})…"),
                            };
                        }
                    }
                }
                GuiEvent::IncomingCall {
                    call_id: _,
                    caller_id,
                    caller_name,
                    is_video,
                    is_group,
                } => {
                    let resolved_name = self
                        .threads
                        .iter()
                        .find(|t| t.id == caller_id)
                        .map(|t| t.name.clone())
                        .unwrap_or(caller_name);
                    self.status_msg = format!(
                        "📲 Cuộc gọi {} đến từ {resolved_name}…",
                        if is_video { "video" } else { "thoại" }
                    );
                    self.active_call = Some(ActiveCallOverlay {
                        partner_id: caller_id,
                        partner_name: resolved_name,
                        is_video,
                        is_group,
                        state_label: "Cuộc gọi đến đang đổ chuông…".into(),
                        started_at: std::time::Instant::now(),
                        mic_muted: false,
                        cam_enabled: is_video,
                        is_incoming_ringing: true,
                        error_code: None,
                        error_reason: None,
                        speaker_view: false,
                        pinned_tile_idx: 0,
                    });
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                        egui::UserAttentionType::Critical,
                    ));
                    if self.settings.enable_notifications {
                        if let Some(call) = &self.active_call {
                            post_os_notification(
                                &call.partner_id,
                                &call.partner_name,
                                if call.is_video {
                                    "Cuộc gọi video đến"
                                } else {
                                    "Cuộc gọi thoại đến"
                                },
                                true,
                            );
                        }
                    }
                }
                GuiEvent::CallRejected { code, reason } => {
                    self.status_msg = format!("⚠️ Từ chối cuộc gọi (mã {code}): {reason}");
                    if let Some(ref mut call) = self.active_call {
                        call.state_label = "Không thể thực hiện cuộc gọi".into();
                        call.error_code = Some(code);
                        call.error_reason = Some(reason);
                    }
                }
                GuiEvent::StatusUpdate(msg) => {
                    self.status_msg = msg;
                }
                GuiEvent::HistoryReachedStart(tid) => {
                    self.history_reached_start.insert(tid);
                    self.show_toast("Đã hiển thị toàn bộ lịch sử tin nhắn của cuộc trò chuyện này");
                }
                GuiEvent::RefreshFinished {
                    threads,
                    messages,
                    errors,
                    manual,
                } => {
                    let msg = if errors.is_empty() {
                        format!("✅ Đã làm mới {threads} hội thoại ({messages} tin nhắn)")
                    } else {
                        format!(
                            "⚠️ Làm mới {threads} hội thoại · {} lỗi (vd. {})",
                            errors.len(),
                            errors[0]
                        )
                    };
                    self.status_msg = msg.clone();
                    if manual {
                        self.show_toast(msg);
                    }
                }
                GuiEvent::ScheduledListUpdated => {
                    self.refresh_scheduled_items();
                }
                GuiEvent::SyncFinished {
                    added,
                    errors,
                    cancelled,
                } => {
                    let head = if cancelled {
                        "Đã dừng đồng bộ"
                    } else {
                        "Đồng bộ xong"
                    };
                    let mut msg = format!("{head}: lưu thêm {added} tin cũ");
                    if let Some(first) = errors.first() {
                        msg.push_str(&format!(" · {} lỗi (vd. {first})", errors.len()));
                    }
                    self.status_msg = msg.clone();
                    self.show_toast(msg);
                }
                GuiEvent::UpdateCheckFinished { result, silent } => {
                    self.update_checking = false;
                    match result {
                        Ok(info) => {
                            if info.has_update {
                                let msg = format!(
                                    "🚀 Có bản cập nhật mới v{} trên GitHub (hiện tại: v{})",
                                    info.latest_version, info.current_version
                                );
                                self.update_status = msg.clone();
                                let is_skipped = silent
                                    && !self.settings.skipped_update_version.is_empty()
                                    && self.settings.skipped_update_version == info.latest_version;
                                let already_notified = self.last_notified_update_version.as_deref()
                                    == Some(info.latest_version.as_str());
                                if !is_skipped {
                                    self.show_toast(msg);
                                    if !already_notified || !silent {
                                        self.last_notified_update_version =
                                            Some(info.latest_version.clone());
                                        self.update_modal_open = true;
                                        self.update_banner_dismissed = false;
                                        if self.settings.enable_notifications {
                                            crate::updater::notify_os_new_update(
                                                &info.latest_version,
                                                &info.current_version,
                                                &info.release_title,
                                            );
                                        }
                                    }
                                }
                            } else {
                                self.update_status = format!(
                                    "✅ Bạn đang sử dụng phiên bản mới nhất (v{})",
                                    info.current_version
                                );
                                if !silent {
                                    self.show_toast(self.update_status.clone());
                                }
                            }
                            self.update_info = Some(info);
                        }
                        Err(e) => {
                            self.update_status = format!("⚠️ Không kiểm tra được cập nhật: {e}");
                            if !silent {
                                self.show_toast(self.update_status.clone());
                            }
                        }
                    }
                }
                GuiEvent::UpdateDownloadStatus {
                    downloading,
                    message,
                } => {
                    self.update_downloading = downloading;
                    if !downloading {
                        self.update_progress = None;
                    }
                    self.update_status = message.clone();
                    self.show_toast(message);
                }
                GuiEvent::UpdateDownloadProgress { downloaded, total } => {
                    self.update_downloading = true;
                    self.update_progress = Some((downloaded, total));
                }
                GuiEvent::UpdateAppliedAndRestarting(ver) => {
                    self.update_downloading = false;
                    self.show_toast(format!("✅ Đã cập nhật lên v{ver} — Đang khởi động lại…"));
                    self.allow_quit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    std::process::exit(0);
                }
                GuiEvent::SocialProfileLoaded(res) => {
                    self.feed_loading_profile = false;
                    match res {
                        Ok(prof) => {
                            self.feed_profile = Some(prof);
                            self.feed_profile_error = None;
                        }
                        Err(e) => {
                            self.feed_profile_error = Some(e);
                        }
                    }
                }
                GuiEvent::SocialAvatarsLoaded(res) => {
                    self.feed_loading_avatars = false;
                    match res {
                        Ok(photos) => {
                            self.feed_avatars = photos;
                            self.feed_avatars_error = None;
                        }
                        Err(e) => {
                            self.feed_avatars_error = Some(e);
                        }
                    }
                }
                GuiEvent::SocialTimelineLoaded(res) => {
                    self.feed_loading_timeline = false;
                    match res {
                        Ok((profiles, items)) => {
                            self.feed_friend_profiles = profiles;
                            self.feed_items = items;
                        }
                        Err(e) => {
                            self.feed_profile_error = Some(e);
                        }
                    }
                }
                GuiEvent::MuteSnapshotSynced(mute_map) => {
                    self.settings.muted_threads = mute_map.clone();
                    self.settings.save();
                    let now_ts = crate::login::unix_now();
                    for t in &mut self.threads {
                        if let Some(&until) = mute_map.get(&t.id) {
                            let active = until == -1 || until > now_ts;
                            t.presentation.muted = active;
                            t.unread.muted = active;
                            t.unread.muted_until = (until > 0).then_some(until);
                            t.unread.settings_fresh = true;
                            t.unread.fresh = true;
                        } else {
                            t.presentation.muted = false;
                            t.unread.muted = false;
                            t.unread.muted_until = None;
                            t.unread.settings_fresh = true;
                            t.unread.fresh = true;
                        }
                    }
                    self.persist_threads_async();
                }
                GuiEvent::MuteEventsReceived(events) => {
                    let mut changed = false;
                    for ev in events {
                        if ev.muted {
                            let until_val = ev.muted_until.unwrap_or(-1);
                            self.settings
                                .muted_threads
                                .insert(ev.thread_id.clone(), until_val);
                        } else {
                            self.settings.muted_threads.remove(&ev.thread_id);
                        }
                        if let Some(t) = self.threads.iter_mut().find(|t| t.id == ev.thread_id) {
                            t.presentation.muted = ev.muted;
                            t.unread.muted = ev.muted;
                            t.unread.muted_until = ev.muted_until;
                            t.unread.settings_fresh = true;
                            t.unread.fresh = true;
                        }
                        changed = true;
                    }
                    if changed {
                        self.settings.save();
                        self.persist_threads_async();
                    }
                }
                GuiEvent::PhoneSyncProgress { progress, status } => {
                    self.phone_sync_progress = progress;
                    self.phone_sync_status = status.clone();
                    self.status_msg = status;
                }
                GuiEvent::BackgroundHistoryFinished {
                    added,
                    active_threads,
                    error,
                    yielded,
                } => {
                    // A startup refresh must never replace the phone transfer's status or result.
                    if !yielded
                        && !crate::history_sync::PHONE_SYNC_RUNNING
                            .load(std::sync::atomic::Ordering::SeqCst)
                    {
                        self.status_msg = if let Some(error) = error {
                            format!("Không thể làm mới lịch sử máy chủ: {error}")
                        } else {
                            format!("Đã làm mới Cloud/Nhóm: {added} tin cập nhật ({active_threads} hội thoại)")
                        };
                    }
                }
                GuiEvent::PhoneSyncFinished {
                    threads,
                    added,
                    active_threads,
                    cancelled,
                    error,
                } => {
                    self.phone_sync_progress = 0.0;
                    if !threads.is_empty() {
                        for mut nt in threads {
                            if let Some(at) = tombstone_for(&self.settings.deleted_threads, &nt.id)
                            {
                                let revived = !self.threads.iter().any(|t| t.id == nt.id);
                                if !apply_delete_cutoff(&mut nt, at, revived) {
                                    continue;
                                }
                            }
                            if let Some(existing) = self.threads.iter_mut().find(|t| t.id == nt.id)
                            {
                                crate::chat::merge_thread_into(existing, nt, true);
                            } else if !nt.id.is_empty() && nt.id != "0" {
                                self.threads.push(nt);
                            }
                        }
                        self.persist_threads_async();
                    }
                    let msg = phone_sync_completion_message(
                        added,
                        active_threads,
                        cancelled,
                        error.as_deref(),
                        crate::history_sync::last_mobile_sync_status().as_deref(),
                    );
                    self.phone_sync_status = msg.clone();
                    self.status_msg = msg.clone();
                    self.show_toast(msg);
                }
                GuiEvent::ProxyUpdated(res) => match res {
                    Ok(Some(url)) => {
                        let redacted = crate::session::redact_proxy_url(&url);
                        self.settings.proxy_url = url;
                        self.settings.save();
                        let msg = format!("🌐 Đã áp dụng Proxy: {redacted}");
                        self.status_msg = msg.clone();
                        self.show_toast(msg);
                    }
                    Ok(None) => {
                        self.settings.proxy_url.clear();
                        self.settings.save();
                        let msg = "🌐 Đã tắt Proxy (kết nối trực tiếp)".to_string();
                        self.status_msg = msg.clone();
                        self.show_toast(msg);
                    }
                    Err(e) => {
                        let msg = format!("⚠️ Lỗi cấu hình Proxy: {e}");
                        self.status_msg = msg.clone();
                        self.show_toast(msg);
                    }
                },
                GuiEvent::FileDownloaded { url, res } => match res {
                    Ok((fname, path)) => {
                        let msg = format!("✅ Đã lưu tập tin \"{fname}\" tại {path}");
                        self.status_msg = msg.clone();
                        self.show_toast(msg);
                        // Remember it so the file card shows "Đã có trên máy" + folder button.
                        set_downloaded_path(&url, Some(path.clone()));
                        self.settings.downloaded_files.insert(url, path);
                        self.settings.save();
                    }
                    Err(e) => {
                        let msg = format!("⚠️ Lỗi lưu tập tin: {e}");
                        self.status_msg = msg.clone();
                        self.show_toast(msg);
                    }
                },
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Lightweight Background Session Worker (2-thread Tokio pool, off-UI disk I/O)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Main eframe::App Implementation
// ---------------------------------------------------------------------------

impl eframe::App for ZaloGuiApp {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        // macOS: always clear to transparent; `paint_window_body` draws the rounded window.
        if self.glass_active() || cfg!(target_os = "macos") {
            [0.0; 4] // let the vibrancy view show through
        } else {
            visuals.panel_fill.to_normalized_gamma_f32()
        }
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Some(identity) = notification_click() {
            if let Some(index) = self
                .threads
                .iter()
                .position(|t| crate::crypto::md5_hex(&t.id).repeat(2) == identity)
            {
                self.selected_thread_index = index;
                self.contacts_view_open = false;
                self.feed_modal_open = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
        let frame: &eframe::Frame = frame;
        // An opaque wallpaper covers the blur; keeping the blur under it left a light 1 px ring
        // at the anti-aliased window edge.
        let want_glass =
            glass_supported() && self.settings.liquid_glass && self.bg_texture.is_none();
        if want_glass != self.vibrancy_on {
            self.vibrancy_on = set_window_vibrancy(frame, want_glass) && want_glass;
            self.theme_dirty = true;
        }
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Q))
            || ctx.input(|i| i.viewport().close_requested())
        {
            self.allow_quit = true;
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::EndCall);
            }
            std::process::exit(0);
        }
        if ctx.input(|i| {
            (i.modifiers.ctrl || i.modifiers.command) && i.modifiers.shift && i.key_pressed(Key::P)
        }) {
            self.settings_modal_open = true;
            self.settings_tab = SettingsTab::Privacy;
        }
        if self.theme_dirty {
            self.palette().apply_to_ctx(ctx);
            self.theme_dirty = false;
        }
        // Fixed UI size (not user-adjustable): 118 % on macOS matches system text size.
        let want_zoom = if cfg!(target_os = "macos") { 1.18 } else { 1.0 };
        if (ctx.zoom_factor() - want_zoom).abs() > 0.001 {
            ctx.set_zoom_factor(want_zoom);
        }
        // Whole-window wallpaper: decode once in the worker, then paint under every panel.
        if self.settings.app_background.is_empty() {
            if self.bg_texture.take().is_some() {
                self.theme_dirty = true;
            }
            self.bg_requested = false;
        } else if !self.bg_requested {
            self.bg_requested = true;
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::LoadBackgroundImage {
                    path: self.settings.app_background.clone(),
                });
            }
        }
        // Rounded window body (macOS): the window is transparent, so painting the background as
        // a rounded rect gives Telegram/Zalo-style corners. Wallpaper replaces the plain fill.
        if cfg!(target_os = "macos") {
            let pal_now = self.palette();
            paint_window_body(ctx, &pal_now, self.bg_texture.as_ref(), self.dark_mode);
        } else if let Some(tex) = &self.bg_texture {
            paint_wallpaper(ctx, tex, self.dark_mode);
        }

        self.check_events(ctx);
        self.mark_open_thread_read(ctx);
        self.fill_open_thread_from_zalo_pc();

        self.ime_just_committed = false;
        // Only valid for the frame of the commit (the composer reads it later in this frame).
        self.ime_enter_commit = false;
        ctx.input_mut(|i| {
            let mut ime_commit_in_frame = false;
            let mut ime_space_commit = false;

            #[cfg(target_os = "macos")]
            let space_is_down = {
                #[link(name = "CoreGraphics", kind = "framework")]
                extern "C" {
                    fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
                }
                // state_id = 0 (kCGEventSourceStateCombinedSessionState), key = 49 (kVK_Space)
                unsafe { CGEventSourceKeyState(0, 49) }
            };
            #[cfg(not(target_os = "macos"))]
            let space_is_down = false;
            // Return / keypad Enter held: macOS Telex uses that press to commit the underlined
            // word and never forwards it, so the send has to be inferred from the commit.
            #[cfg(target_os = "macos")]
            let return_is_down = {
                #[link(name = "CoreGraphics", kind = "framework")]
                extern "C" {
                    fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
                }
                // 36 = kVK_Return, 76 = kVK_ANSI_KeypadEnter.
                unsafe { CGEventSourceKeyState(0, 36) || CGEventSourceKeyState(0, 76) }
            };
            #[cfg(not(target_os = "macos"))]
            let return_is_down = false;

            let space_in_events = i.events.iter().any(|e| match e {
                egui::Event::Key {
                    key: Key::Space,
                    pressed: true,
                    ..
                } => true,
                egui::Event::Text(s) if s == " " => true,
                _ => false,
            }) || i.key_down(Key::Space);

            // Check whether Enter, non-space punctuation/navigation, or a mouse click
            // occurred in the same frame so we only auto-append a trailing space when
            // Spacebar triggered the macOS Vietnamese Telex IME commit (#71).
            let enter_pressed = i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key {
                        key: Key::Enter,
                        pressed: true,
                        ..
                    }
                )
            });
            let non_space_trigger = i.events.iter().any(|e| match e {
                egui::Event::Text(s) => s != " ",
                egui::Event::Key {
                    key, pressed: true, ..
                } => !matches!(key, Key::Space | Key::Backspace),
                _ => false,
            });
            let mouse_clicked =
                i.pointer.any_pressed() || i.pointer.any_released() || i.pointer.any_click();
            let space_pressed = space_is_down || space_in_events;
            let mut was_composing = self.ime_active;

            for event in &mut i.events {
                match event {
                    egui::Event::Ime(egui::ImeEvent::Enabled) => {
                        self.ime_active = true;
                        was_composing = true;
                    }
                    egui::Event::Ime(egui::ImeEvent::Preedit(text)) => {
                        self.ime_active = !text.is_empty();
                        if !text.is_empty() {
                            was_composing = true;
                        }
                    }
                    egui::Event::Ime(egui::ImeEvent::Commit(ref mut text)) => {
                        let inferred_macos_space_commit = cfg!(target_os = "macos")
                            && was_composing
                            && !enter_pressed
                            && !return_is_down
                            && !non_space_trigger
                            && !mouse_clicked;
                        if (space_pressed || inferred_macos_space_commit)
                            && !text.is_empty()
                            && !text.ends_with(' ')
                            && !text.chars().any(crate::gui::theme::is_cjk)
                        {
                            text.push(' ');
                            ime_space_commit = true;
                        }
                        self.ime_active = false;
                        self.ime_just_committed = true;
                        ime_commit_in_frame = true;
                        if return_is_down && !space_pressed {
                            // One Return both commits the word and sends (as in Zalo).
                            self.ime_enter_commit = true;
                        }
                    }
                    egui::Event::Ime(egui::ImeEvent::Disabled) => {
                        self.ime_active = false;
                    }
                    _ => {}
                }
            }

            if ime_space_commit {
                // Prevent duplicate space insertion if winit also forwarded a Space event in the same frame.
                i.events.retain(|e| {
                    !matches!(e, egui::Event::Text(s) if s == " ")
                        && !matches!(
                            e,
                            egui::Event::Key {
                                key: Key::Space,
                                pressed: true,
                                ..
                            }
                        )
                });
            }

            if self.ime_active || ime_commit_in_frame {
                // Suppress Enter key events during IME composition/commit so pressing Enter
                // to accept an IME candidate does not blur TextEdit or send prematurely (#71).
                i.events.retain(|e| {
                    !matches!(
                        e,
                        egui::Event::Key {
                            key: Key::Enter,
                            ..
                        }
                    )
                });
            }
        });

        // macOS Telex keeps the word being typed as IME preedit; egui holds it selected so the
        // next preedit replaces it, which painted a blue selection box over the word. Hide the
        // selection fill while composing (text fields draw the native-style underline instead).
        let composing = self.ime_active;
        ctx.style_mut(|st| {
            if composing {
                if st.visuals.selection.bg_fill != Color32::TRANSPARENT {
                    self.selection_fill_saved = Some(st.visuals.selection.bg_fill);
                    st.visuals.selection.bg_fill = Color32::TRANSPARENT;
                }
            } else if let Some(fill) = self.selection_fill_saved.take() {
                if st.visuals.selection.bg_fill == Color32::TRANSPARENT {
                    st.visuals.selection.bg_fill = fill;
                }
            }
        });

        // No timer repaint: the worker owns the poll timer and every GuiTx send wakes the UI.

        let pal = self.palette();

        Self::handle_frameless_resize(ctx);
        if cfg!(target_os = "macos") {
            // Custom traffic lights and drag region sit inside the rounded list card.
            render_window_chrome(ctx);
            let _ = frame;
        } else {
            self.render_custom_title_bar(ctx, &pal);
        }
        // Dev Inspector Panel
        if self.dev_mode && self.dev_panel_open {
            egui::TopBottomPanel::bottom("dev_inspector")
                .resizable(true)
                .default_height(250.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong("Dev Inspector & Protocol Scanner");
                        ui.separator();
                        ui.selectable_value(
                            &mut self.dev_tab,
                            DevTab::Logs,
                            "Lưu lượng mạng (Logs)",
                        );
                        ui.selectable_value(
                            &mut self.dev_tab,
                            DevTab::Scanner,
                            "Quét lỗi hệ thống (Scanner)",
                        );
                        ui.selectable_value(
                            &mut self.dev_tab,
                            DevTab::CryptoState,
                            "Trạng thái Crypto & Keys",
                        );

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Làm mới quét").clicked() {
                                self.scanner_findings = logger::scan_logs();
                            }
                            if ui.button("Xóa log").clicked() {
                                logger::clear_entries();
                                self.selected_log = None;
                                self.scanner_findings.clear();
                            }
                        });
                    });
                    ui.separator();

                    match self.dev_tab {
                        DevTab::Logs => self.render_dev_logs(ui),
                        DevTab::Scanner => self.render_dev_scanner(ui),
                        DevTab::CryptoState => self.render_dev_crypto(ui),
                    }
                });
        }

        // Main Workspace
        if !self.is_logged_in {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(pal.bg_chat_canvas))
                .show(ctx, |ui| {
                    self.render_login_screen(ui, &pal);
                });
        } else {
            self.render_chat_workspace(ctx, &pal);
        }

        // Auto-notification modal for new GitHub Releases (works on both Login & Chat screens)
        self.render_update_notification_modal(ctx, &pal);

        // Square 1 px frame for the borderless window on Windows/Linux. On macOS the window is
        // rounded, and this foreground rectangle was the straight light outline around it.
        if !cfg!(target_os = "macos") && !ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
            let screen = ctx.screen_rect();
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                Id::new("window_border_overlay"),
            ))
            .rect_stroke(
                screen.shrink(0.5),
                Rounding::ZERO,
                Stroke::new(1.0_f32, pal.border),
            );
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        std::process::exit(0);
    }
}

// ---------------------------------------------------------------------------
// Screen Renderers
// ---------------------------------------------------------------------------

impl ZaloGuiApp {
    fn handle_frameless_resize(ctx: &egui::Context) {
        if ctx.input(|i| {
            i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
        }) {
            return;
        }
        let screen = ctx.screen_rect();
        let (pos, press_pos, _primary_down) = ctx.input(|i| {
            let press = i.events.iter().find_map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } => Some(*pos),
                _ => None,
            });
            (
                i.pointer.interact_pos().or_else(|| i.pointer.latest_pos()),
                press,
                i.pointer.primary_down(),
            )
        });
        let Some(pos) = pos else {
            return;
        };
        // A fast drag may press, move and release within one frame; hit-test its press position.
        let hit_pos = press_pos.unwrap_or(pos);
        #[cfg(target_os = "macos")]
        {
            let resize_id = Id::new("macos_borderless_resize");
            type ResizeDrag = (Rect, Pos2, egui::ResizeDirection, CursorIcon);
            if let Some((start, pointer, direction, cursor)) =
                ctx.data(|data| data.get_temp::<ResizeDrag>(resize_id))
            {
                if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                    let global = outer.min + pos.to_vec2();
                    let next = resized_window_rect(
                        start,
                        global - pointer,
                        direction,
                        Vec2::new(480.0, 360.0),
                    );
                    ctx.set_cursor_icon(cursor);
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(next.min));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(next.size()));
                    if _primary_down {
                        ctx.request_repaint();
                    }
                }
                if !_primary_down {
                    ctx.data_mut(|data| data.remove::<ResizeDrag>(resize_id));
                }
                return;
            }
        }
        let border = 5.0_f32;
        let corner = 9.0_f32;
        let left = (hit_pos.x - screen.left()).abs() <= border;
        let right = (screen.right() - hit_pos.x).abs() <= border;
        let top = (hit_pos.y - screen.top()).abs() <= border;
        let bottom = (screen.bottom() - hit_pos.y).abs() <= border;

        let left_c = (hit_pos.x - screen.left()).abs() <= corner;
        let right_c = (screen.right() - hit_pos.x).abs() <= corner;
        let top_c = (hit_pos.y - screen.top()).abs() <= corner;
        let bottom_c = (screen.bottom() - hit_pos.y).abs() <= corner;

        let dir_and_cursor = if top_c && left_c && (top || left) {
            Some((egui::ResizeDirection::NorthWest, CursorIcon::ResizeNwSe))
        } else if top_c && right_c && (top || right) {
            Some((egui::ResizeDirection::NorthEast, CursorIcon::ResizeNeSw))
        } else if bottom_c && left_c && (bottom || left) {
            Some((egui::ResizeDirection::SouthWest, CursorIcon::ResizeNeSw))
        } else if bottom_c && right_c && (bottom || right) {
            Some((egui::ResizeDirection::SouthEast, CursorIcon::ResizeNwSe))
        } else if left {
            Some((egui::ResizeDirection::West, CursorIcon::ResizeHorizontal))
        } else if right {
            Some((egui::ResizeDirection::East, CursorIcon::ResizeHorizontal))
        } else if top {
            Some((egui::ResizeDirection::North, CursorIcon::ResizeVertical))
        } else if bottom {
            Some((egui::ResizeDirection::South, CursorIcon::ResizeVertical))
        } else {
            None
        };

        if let Some((dir, cursor)) = dir_and_cursor {
            ctx.set_cursor_icon(cursor);
            if press_pos.is_some() {
                #[cfg(not(target_os = "macos"))]
                ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
                #[cfg(target_os = "macos")]
                if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                    // winit does not implement BeginResize on macOS. Track desktop coordinates
                    // because dragging a top/left edge also moves the window under the pointer.
                    let global = outer.min + hit_pos.to_vec2();
                    if _primary_down {
                        ctx.data_mut(|data| {
                            data.insert_temp(
                                Id::new("macos_borderless_resize"),
                                (outer, global, dir, cursor),
                            )
                        });
                    }
                    let next =
                        resized_window_rect(outer, pos - hit_pos, dir, Vec2::new(480.0, 360.0));
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(next.min));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(next.size()));
                }
            }
        }
    }

    fn render_custom_title_bar(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        let is_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let total_unread: usize = self.unread_counts.values().sum();
        if self.last_synced_icon_unread != Some(total_unread) {
            self.last_synced_icon_unread = Some(total_unread);
            // macOS: keep the bundle icon (drawn on Apple's icon grid) and show the count as the
            // Dock's own red badge; replacing the icon made it larger than every other Dock icon.
            #[cfg(target_os = "macos")]
            set_dock_badge(total_unread);
            #[cfg(not(target_os = "macos"))]
            if let Some(icon) = crate::gui::theme::load_app_icon_with_badge(total_unread) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(std::sync::Arc::new(icon))));
            }
        }

        egui::TopBottomPanel::top("custom_title_bar")
            .exact_height(32.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::none()
                    // macOS: the rounded window body underneath supplies the colour.
                    .fill(if cfg!(target_os = "macos") { Color32::TRANSPARENT } else { pal.bg_app })
                    .inner_margin(Margin {
                        left: 12.0,
                        right: 0.0,
                        top: 0.0,
                        bottom: 0.0,
                    })
                    ,
            )
            .show(ctx, |ui| {
                let bar_rect = ui.max_rect();
                let mut buttons_left_x = bar_rect.right();

                ui.horizontal_centered(|ui| {
                    // 0. macOS traffic lights (close / minimise / zoom), like native windows.
                    if cfg!(target_os = "macos") {
                        render_traffic_lights(ui, ctx);
                        ui.add_space(10.0);
                    }
                    // 1. App Logo (20x20)
                    let (logo_rect, _) =
                        ui.allocate_exact_size(Vec2::new(20.0, 20.0), Sense::hover());
                    if let Some(tex) = &self.app_logo_texture {
                        ui.painter().image(
                            tex.id(),
                            logo_rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    } else {
                        ui.painter()
                            .rect_filled(logo_rect, Rounding::same(10.0), pal.primary);
                    }

                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("ctdozlite")
                            .color(pal.text_primary)
                            .size(13.5)
                            .strong(),
                    );

                    // Subtle "Native v<version>" pill badge
                    egui::Frame::none()
                        .fill(pal.primary.linear_multiply(0.14))
                        .rounding(Rounding::same(10.0))
                        .inner_margin(Margin::symmetric(5.0, 1.5))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("Native v{}", env!("CARGO_PKG_VERSION")))
                                    .color(pal.primary)
                                    .size(10.5)
                                    .strong(),
                            );
                        });

                    if total_unread > 0 {
                        ui.add_space(4.0);
                        egui::Frame::none()
                            .fill(pal.danger)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(Margin::symmetric(6.0, 1.5))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("{total_unread} tin mới"))
                                        .color(Color32::WHITE)
                                        .size(10.5)
                                        .strong(),
                                );
                            });
                    } else if self.is_logged_in && !self.user_name.is_empty() {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!("·  {}", self.user_name))
                                .color(pal.text_muted)
                                .size(12.0),
                        );
                    }

                    // 2. Right-side Window Control Buttons
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;

                        // Windows/Linux-style controls; macOS uses traffic lights on the left.
                        if !cfg!(target_os = "macos") {
                        // Close Button (46x32)
                        let (close_rect, close_resp) =
                            ui.allocate_exact_size(Vec2::new(46.0, 32.0), Sense::click());
                        if close_resp.hovered() {
                            ui.painter().rect_filled(
                                close_rect,
                                Rounding::ZERO,
                                Color32::from_rgb(232, 17, 35),
                            );
                        }
                        let close_fg = if close_resp.hovered() {
                            Color32::WHITE
                        } else {
                            pal.text_secondary
                        };
                        let cc = close_rect.center();
                        let d = 4.6_f32;
                        ui.painter().line_segment(
                            [Pos2::new(cc.x - d, cc.y - d), Pos2::new(cc.x + d, cc.y + d)],
                            Stroke::new(1.4_f32, close_fg),
                        );
                        ui.painter().line_segment(
                            [Pos2::new(cc.x + d, cc.y - d), Pos2::new(cc.x - d, cc.y + d)],
                            Stroke::new(1.4_f32, close_fg),
                        );
                        let close_resp = close_resp.on_hover_text("Đóng ứng dụng (Ctrl+Q)");
                        if close_resp.clicked() || close_resp.secondary_clicked() {
                            self.allow_quit = true;
                            if let Some(tx) = WORKER_TX.get() {
                                let _ = tx.send(WorkerCmd::EndCall);
                            }
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            std::process::exit(0);
                        }

                        // Maximize / Restore Button (46x32)
                        let (max_rect, max_resp) =
                            ui.allocate_exact_size(Vec2::new(46.0, 32.0), Sense::click());
                        if max_resp.hovered() {
                            ui.painter()
                                .rect_filled(max_rect, Rounding::ZERO, pal.bg_hover);
                        }
                        let max_fg = if max_resp.hovered() {
                            pal.text_primary
                        } else {
                            pal.text_secondary
                        };
                        let mc = max_rect.center();
                        if is_maximized {
                            let back = Rect::from_center_size(
                                Pos2::new(mc.x + 1.2, mc.y - 1.2),
                                Vec2::splat(7.8),
                            );
                            let front = Rect::from_center_size(
                                Pos2::new(mc.x - 1.2, mc.y + 1.2),
                                Vec2::splat(7.8),
                            );
                            ui.painter()
                                .rect_stroke(back, Rounding::same(1.0), Stroke::new(1.25_f32, max_fg));
                            ui.painter().rect_filled(
                                front,
                                Rounding::same(1.0),
                                if max_resp.hovered() {
                                    pal.bg_hover
                                } else {
                                    pal.bg_app
                                },
                            );
                            ui.painter()
                                .rect_stroke(front, Rounding::same(1.0), Stroke::new(1.25_f32, max_fg));
                        } else {
                            let sq = Rect::from_center_size(mc, Vec2::splat(9.5));
                            ui.painter()
                                .rect_stroke(sq, Rounding::same(1.2), Stroke::new(1.3_f32, max_fg));
                        }
                        if max_resp
                            .on_hover_text(if is_maximized {
                                "Khôi phục kích thước"
                            } else {
                                "Phóng to toàn màn hình"
                            })
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
                        }

                        // Minimize Button (46x32)
                        let (min_rect, min_resp) =
                            ui.allocate_exact_size(Vec2::new(46.0, 32.0), Sense::click());
                        if min_resp.hovered() {
                            ui.painter()
                                .rect_filled(min_rect, Rounding::ZERO, pal.bg_hover);
                        }
                        let min_fg = if min_resp.hovered() {
                            pal.text_primary
                        } else {
                            pal.text_secondary
                        };
                        let mnc = min_rect.center();
                        ui.painter().line_segment(
                            [Pos2::new(mnc.x - 5.0, mnc.y), Pos2::new(mnc.x + 5.0, mnc.y)],
                            Stroke::new(1.4_f32, min_fg),
                        );
                        if min_resp.on_hover_text("Thu nhỏ").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }

                        }

                        // Quick Theme Toggle (36x32)
                        let (th_rect, th_resp) =
                            ui.allocate_exact_size(Vec2::new(36.0, 32.0), Sense::click());
                        if th_resp.hovered() {
                            ui.painter()
                                .rect_filled(th_rect, Rounding::ZERO, pal.bg_hover);
                        }
                        let th_icon = if self.dark_mode {
                            lucide::SUN
                        } else {
                            lucide::MOON
                        };
                        ui.painter().text(
                            th_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            th_icon,
                            FontId::proportional(13.5),
                            if th_resp.hovered() {
                                pal.text_primary
                            } else {
                                pal.text_muted
                            },
                        );
                        if th_resp
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .on_hover_text("Chuyển đổi giao diện Sáng / Tối")
                            .clicked()
                        {
                            self.dark_mode = !self.dark_mode;
                            self.theme_dirty = true;
                        }

                        buttons_left_x = th_rect.left();

                        if let Some(ref upd) = self.update_info {
                            if upd.has_update {
                                ui.add_space(6.0);
                                let badge_txt = if self.update_downloading {
                                    let (dl, tot) = self.update_progress.unwrap_or((0, 0));
                                    if tot > 0 {
                                        let pct = ((dl as f64 / tot as f64) * 100.0).clamp(0.0, 100.0);
                                        format!("⏳ Đang cập nhật {pct:.0}%")
                                    } else {
                                        "⏳ Đang tải cập nhật…".to_string()
                                    }
                                } else {
                                    format!("🚀 Cập nhật mới v{}", upd.latest_version)
                                };
                                let upd_pill = egui::Button::new(
                                    RichText::new(badge_txt)
                                        .size(11.2)
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .fill(pal.accent_online)
                                .rounding(Rounding::same(10.0));
                                let r = ui
                                    .add(upd_pill)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text(format!(
                                        "Đã có phiên bản v{} trên GitHub Releases — Bấm để xem chi tiết & cập nhật tự động",
                                        upd.latest_version
                                    ));
                                buttons_left_x = r.rect.left() - 8.0;
                                if r.clicked() {
                                    self.update_modal_open = true;
                                    self.update_banner_dismissed = false;
                                }
                            }
                        }

                        if !self.is_logged_in && !self.threads.is_empty() {
                            ui.add_space(8.0);
                            let hist_btn = egui::Button::new(
                                RichText::new("Xem lịch sử đã lưu")
                                    .size(11.5)
                                    .color(pal.primary)
                                    .strong(),
                            )
                            .fill(pal.primary.linear_multiply(0.12))
                            .stroke(Stroke::new(1.0_f32, pal.primary))
                            .rounding(Rounding::same(10.0));
                            let r = ui
                                .add(hist_btn)
                                .on_hover_cursor(CursorIcon::PointingHand);
                            buttons_left_x = r.rect.left() - 8.0;
                            if r.clicked() {
                                self.is_logged_in = true;
                            }
                        }
                    });
                });

                // 3. Interactive drag / double-click region covering everything left of the control buttons
                // On macOS the traffic lights sit at the far left; keep them clickable.
                let drag_left = bar_rect.left() + if cfg!(target_os = "macos") { 82.0 } else { 0.0 };
                let drag_rect = Rect::from_min_max(
                    Pos2::new(drag_left, bar_rect.min.y),
                    Pos2::new((buttons_left_x - 2.0).max(bar_rect.left() + 40.0), bar_rect.max.y),
                );
                let drag_resp = ui.interact(
                    drag_rect,
                    Id::new("custom_title_bar_drag"),
                    Sense::click_and_drag(),
                );
                if drag_resp.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
                } else if drag_resp.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
            });
    }

    fn render_login_screen(&mut self, ui: &mut egui::Ui, pal: &ThemePalette) {
        ui.vertical_centered(|ui| {
            let avail_h = ui.available_height();
            ui.add_space((avail_h * 0.08).max(20.0));
            if let Some(tex) = &self.app_logo_texture {
                ui.image((tex.id(), Vec2::new(64.0, 64.0)));
                ui.add_space(10.0);
            }
            ui.label(
                RichText::new("Đăng nhập Zalo")
                    .color(pal.primary)
                    .size(26.0)
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Khởi chạy nhẹ, bảo mật và đồng bộ tin nhắn thời gian thực")
                    .color(pal.text_secondary)
                    .size(13.5),
            );
            ui.add_space(18.0);

            egui::Frame::none()
                .fill(pal.bg_elevated)
                .rounding(Rounding::same(16.0))
                .stroke(Stroke::new(1.0_f32, pal.border))
                .inner_margin(Margin::symmetric(28.0, 24.0))
                .show(ui, |ui| {
                    ui.set_width(360.0);
                    ui.vertical_centered(|ui| {
                        // Segmented Tab Switcher
                        egui::Frame::none()
                            .fill(pal.bg_input)
                            .rounding(Rounding::same(10.0))
                            .inner_margin(Margin::same(4.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let tab_w = (ui.available_width() - 8.0) / 2.0;
                                    for (tab, title) in [
                                        (LoginTab::QrCode, "Quét mã QR"),
                                        (LoginTab::Password, "Số điện thoại"),
                                    ] {
                                        let active = self.login_tab == tab;
                                        let btn = egui::Button::new(
                                            RichText::new(title)
                                                .size(13.0)
                                                .color(if active {
                                                    Color32::WHITE
                                                } else {
                                                    pal.text_secondary
                                                })
                                                .strong(),
                                        )
                                        .fill(if active {
                                            pal.primary
                                        } else {
                                            Color32::TRANSPARENT
                                        })
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(7.0))
                                        .min_size(Vec2::new(tab_w, 32.0));

                                        if ui
                                            .add(btn)
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            self.login_tab = tab;
                                        }
                                    }
                                });
                            });

                        ui.add_space(18.0);

                        match self.login_tab {
                            LoginTab::QrCode => {
                                if let Some(tex) = &self.qr_texture {
                                    egui::Frame::none()
                                        .fill(Color32::WHITE)
                                        .rounding(Rounding::same(12.0))
                                        .stroke(Stroke::new(1.0_f32, pal.border))
                                        .inner_margin(Margin::same(14.0))
                                        .show(ui, |ui| {
                                            ui.image((tex.id(), Vec2::new(240.0, 240.0)));
                                        });
                                } else {
                                    let (rect, _) = ui.allocate_exact_size(
                                        Vec2::new(268.0, 268.0),
                                        Sense::hover(),
                                    );
                                    ui.painter().rect_filled(
                                        rect,
                                        Rounding::same(12.0),
                                        pal.bg_input,
                                    );
                                    ui.painter().rect_stroke(
                                        rect,
                                        Rounding::same(12.0),
                                        Stroke::new(1.0_f32, pal.border),
                                    );
                                    ui.painter().text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "Đang tải mã QR…",
                                        FontId::proportional(13.5),
                                        pal.text_secondary,
                                    );
                                }

                                ui.add_space(14.0);
                                ui.label(
                                    RichText::new(&self.qr_status)
                                        .color(pal.text_primary)
                                        .size(13.5),
                                );
                                ui.add_space(12.0);

                                let refresh_btn = egui::Button::new(
                                    RichText::new("Tạo / Làm mới mã QR")
                                        .size(13.5)
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .fill(pal.primary)
                                .rounding(Rounding::same(8.0))
                                .min_size(Vec2::new(200.0, 36.0));

                                if ui
                                    .add(refresh_btn)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    self.login_in_progress = true;
                                    self.qr_status = "Đang xin mã QR…".into();
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::RequestQr);
                                    }
                                }
                            }
                            LoginTab::Password => {
                                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                    ui.label(
                                        RichText::new("Số điện thoại")
                                            .size(13.0)
                                            .color(pal.text_secondary)
                                            .strong(),
                                    );
                                    ui.add_space(4.0);
                                    ui.add_sized(
                                        [ui.available_width(), 34.0],
                                        egui::TextEdit::singleline(&mut self.phone_input)
                                            .hint_text("Ví dụ: 0912345678"),
                                    );
                                    ui.add_space(12.0);

                                    ui.label(
                                        RichText::new("Mật khẩu")
                                            .size(13.0)
                                            .color(pal.text_secondary)
                                            .strong(),
                                    );
                                    ui.add_space(4.0);
                                    ui.add_sized(
                                        [ui.available_width(), 34.0],
                                        egui::TextEdit::singleline(&mut self.password_input)
                                            .password(true)
                                            .hint_text("Nhập mật khẩu Zalo"),
                                    );
                                    ui.add_space(16.0);

                                    if !self.login_status_msg.is_empty() {
                                        ui.label(
                                            RichText::new(&self.login_status_msg)
                                                .color(pal.danger)
                                                .size(12.5),
                                        );
                                        ui.add_space(8.0);
                                    }

                                    let btn = egui::Button::new(
                                        RichText::new("Đăng nhập")
                                            .color(Color32::WHITE)
                                            .size(13.0)
                                            .strong(),
                                    )
                                    .fill(pal.primary)
                                    .rounding(Rounding::same(8.0))
                                    .min_size(Vec2::new(ui.available_width(), 38.0));

                                    if ui
                                        .add_enabled(!self.login_in_progress, btn)
                                        .on_hover_cursor(CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        self.login_in_progress = true;
                                        self.login_status_msg = "Đang kết nối…".into();
                                        if let Some(tx) = WORKER_TX.get() {
                                            let _ = tx.send(WorkerCmd::LoginPassword {
                                                phone: self.phone_input.clone(),
                                                pass: self.password_input.clone(),
                                            });
                                        }
                                    }
                                });
                            }
                        }
                    });
                });
        });
    }

    /// Show the alias instead of the server display name for a 1:1 thread (as Zalo does),
    /// remembering the real name so removing the alias can restore it.
    fn apply_alias_to_thread(&mut self, t: &mut crate::chat::Thread) {
        if t.is_group {
            return;
        }
        if let Some(alias) = alias_for(&t.id) {
            if t.name != alias {
                let real = std::mem::replace(&mut t.name, alias);
                self.alias_original_names.insert(t.id.clone(), real);
            }
        }
    }

    /// Apply live reactions to the matching messages, persist them, and notify (in the
    /// background) when someone reacts to one of your messages.
    fn apply_live_reactions(
        &mut self,
        ctx: &egui::Context,
        events: Vec<crate::reactions::ReactionEvent>,
    ) {
        let norm = |id: &str| -> String {
            id.trim_start_matches("group_")
                .trim_start_matches('g')
                .to_string()
        };
        let me = self.user_uin.clone();
        let mut changed = false;
        let mut notice: Option<(String, String, String)> = None;
        for ev in events {
            // 1:1: a reaction *to* you names you as the target; the conversation is the reactor.
            let thread_key = if !me.is_empty() && norm(&ev.thread_id) == me {
                ev.sender_id.clone()
            } else {
                ev.thread_id.clone()
            };
            let key = norm(&thread_key);
            let Some(t) = self.threads.iter_mut().find(|t| norm(&t.id) == key) else {
                continue;
            };
            let Some(m) = t.messages.iter_mut().find(|m| {
                (!ev.message_id.is_empty() && m.id == ev.message_id)
                    || (!ev.client_message_id.is_empty()
                        && (m.client_message_id == ev.client_message_id
                            || m.id == ev.client_message_id))
            }) else {
                continue;
            };
            if m.apply_reaction(&ev) {
                changed = true;
                if m.is_outgoing && ev.sender_id != me && ev.r_type >= 0 {
                    let who = crate::gui::alias_for(&ev.sender_id)
                        .or_else(|| {
                            self.group_members
                                .get(&t.id)
                                .and_then(|ms| ms.iter().find(|gm| gm.id == ev.sender_id))
                                .map(|gm| gm.name.clone())
                        })
                        .unwrap_or_else(|| {
                            if t.is_group {
                                "Ai đó".to_string()
                            } else {
                                t.name.clone()
                            }
                        });
                    let snippet: String = m.text.chars().take(60).collect();
                    notice = Some((
                        t.id.clone(),
                        t.name.clone(),
                        format!("{who} đã bày tỏ cảm xúc {} với: {snippet}", ev.emoji),
                    ));
                }
            }
        }
        if changed {
            self.persist_threads_async();
            ctx.request_repaint();
        }
        if let Some((thread_id, title, body)) = notice {
            let focused = ctx.input(|i| i.focused);
            if !focused && self.settings.enable_notifications {
                post_os_notification(
                    &thread_id,
                    &title,
                    &body,
                    self.settings.play_notification_sound,
                );
            }
        }
    }

    /// Real (server) display name of a 1:1 thread, ignoring any alias.
    pub(crate) fn real_thread_name(&self, id: &str, shown: &str) -> String {
        self.alias_original_names
            .get(id)
            .cloned()
            .unwrap_or_else(|| shown.to_string())
    }

    /// The conversation on screen counts as read once the window has focus: messages that came
    /// in while ctdozlite was in the background kept their unread badge after returning.
    /// Gives empty messages of the open conversation (old phone-sync rows that only kept an id)
    /// the photo, video or file Zalo PC already has on disk. Runs again only when the
    /// conversation gains messages; paths are only read, never written.
    fn fill_open_thread_from_zalo_pc(&mut self) {
        let Some(idx) = zalo_pc_media() else { return };
        let Some(t) = self.threads.get_mut(self.selected_thread_index) else {
            return;
        };
        if self.zpc_enriched.get(&t.id) == Some(&t.messages.len()) {
            return;
        }
        self.zpc_enriched.insert(t.id.clone(), t.messages.len());
        let me = self.user_uin.clone();
        let mut taken: HashSet<std::path::PathBuf> = HashSet::new();
        let mut changed = false;
        for m in t.messages.iter_mut() {
            let empty = m.link_url.as_deref().unwrap_or("").is_empty()
                && m.link_thumb.as_deref().unwrap_or("").is_empty()
                && m.sticker_id.is_none()
                && matches!(m.text.trim(), "" | "[Hình ảnh]" | "[Video]" | "[Tập tin]");
            if !empty {
                continue;
            }
            let sender = if m.is_outgoing {
                me.clone()
            } else {
                crate::chat::sidebar_sender_id(m)
                    .or_else(|| {
                        m.sender
                            .bytes()
                            .all(|b| b.is_ascii_digit())
                            .then(|| m.sender.clone())
                    })
                    .unwrap_or_default()
            };
            let ts = crate::clock::normalize_ts_ms(m.ts);
            let Some((kind, path)) =
                idx.attachment_for(&t.id, &m.id, &m.client_message_id, &sender, ts, &taken)
            else {
                continue;
            };
            let path_s = path.to_string_lossy().into_owned();
            taken.insert(path.to_path_buf());
            use crate::zalo_pc_media::MediaKind;
            match kind {
                MediaKind::Picture => {
                    m.text = "[Hình ảnh]".into();
                    m.link_src = Some("Hình ảnh Zalo".into());
                    m.link_thumb = Some(path_s);
                }
                MediaKind::Video | MediaKind::File => {
                    let is_video = kind == MediaKind::Video;
                    m.text = if is_video { "[Video]" } else { "[Tập tin]" }.into();
                    m.link_src = Some(
                        if is_video {
                            "Video Zalo"
                        } else {
                            "Tập tin Zalo"
                        }
                        .into(),
                    );
                    m.link_title = Some(
                        if is_video {
                            "Video từ Zalo PC"
                        } else {
                            "Tệp từ Zalo PC"
                        }
                        .into(),
                    );
                    if let Ok(meta) = std::fs::metadata(path) {
                        m.link_desc = Some(format!("{:.1} MB", meta.len() as f64 / 1_048_576.0));
                    }
                    m.link_url = Some(path_s.clone());
                    // The file is already on this machine: the card offers Mở / Hiện trong Finder.
                    set_downloaded_path(&path_s, Some(path_s.clone()));
                }
            }
            changed = true;
        }
        if changed {
            self.persist_threads_async();
        }
    }

    fn mark_open_thread_read(&mut self, ctx: &egui::Context) {
        // Only while the conversation is actually on screen.
        if !ctx.input(|i| i.focused) || self.contacts_view_open || self.feed_modal_open {
            return;
        }
        let Some(t) = self.threads.get_mut(self.selected_thread_index) else {
            return;
        };
        if self.unread_counts.get(&t.id).copied().unwrap_or(0) == 0 {
            return;
        }
        let tid = t.id.clone();
        let is_group = t.is_group;
        let msgs = t.messages.clone();
        t.unread.read_visible(&msgs);
        let last_mid = msgs.last().map(|m| m.id.clone()).unwrap_or_default();
        self.unread_counts.remove(&tid);
        self.persist_threads_async();
        let total_unread: usize = self.unread_counts.values().sum();
        let title = if total_unread > 0 {
            format!("({total_unread}) ctdozlite")
        } else {
            "ctdozlite".to_string()
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        if !self.settings.block_seen && !last_mid.is_empty() {
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::SendSeen {
                    thread_id: tid,
                    is_group,
                    last_msg_id: last_mid,
                });
            }
        }
    }

    fn show_toast(&mut self, msg: impl Into<String>) {
        let text = msg.into();
        self.status_msg = text.clone();
        self.toast_notice = Some((text, std::time::Instant::now()));
    }
}

/// Sets (or clears, for 0) the unread count on the app's Dock tile. Must run on the main thread,
/// which egui's update loop is.
#[cfg(target_os = "macos")]
fn set_dock_badge(unread: usize) {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSString;
    let label = match unread {
        0 => None,
        1..=99 => Some(unread.to_string()),
        _ => Some("99+".to_string()),
    };
    objc2::rc::autoreleasepool(|_| unsafe {
        let Some(cls) = AnyClass::get("NSApplication") else {
            return;
        };
        let app: *mut AnyObject = msg_send![cls, sharedApplication];
        if app.is_null() {
            return;
        }
        let tile: *mut AnyObject = msg_send![app, dockTile];
        if tile.is_null() {
            return;
        }
        match label {
            Some(l) => {
                let s = NSString::from_str(&l);
                let _: () = msg_send![tile, setBadgeLabel: &*s];
            }
            None => {
                let _: () = msg_send![tile, setBadgeLabel: std::ptr::null_mut::<AnyObject>()];
            }
        }
    });
}

/// Bare thread id for tombstone lookups (`g123` / `group_123` / `123_group` → `123`).
fn bare_thread_id(id: &str) -> &str {
    let id = id.trim_start_matches("group_");
    let id = id.strip_suffix("_group").unwrap_or(id);
    id.strip_prefix('g')
        .or_else(|| id.strip_prefix('G'))
        .unwrap_or(id)
}

/// Deletion time (ms) recorded for a conversation, whatever spelling of its id is used.
fn tombstone_for(deleted: &HashMap<String, i64>, id: &str) -> Option<i64> {
    deleted.get(id).copied().or_else(|| {
        let bare = bare_thread_id(id);
        deleted
            .iter()
            .find(|(k, _)| bare_thread_id(k) == bare)
            .map(|(_, v)| *v)
    })
}

/// Drops the messages a "Xóa hội thoại" removed (sent at or before `deleted_at`, in ms). Returns
/// false when nothing newer is left, so the conversation stays hidden. A conversation that comes
/// back (`revived`) has its sidebar line rebuilt from the surviving messages, as Zalo does, instead
/// of keeping the server's placeholder preview.
fn apply_delete_cutoff(nt: &mut crate::chat::Thread, deleted_at: i64, revived: bool) -> bool {
    nt.messages
        .retain(|m| crate::clock::normalize_ts_ms(m.ts) > deleted_at);
    if nt.messages.is_empty() {
        return false;
    }
    if revived {
        nt.presentation.last_message_ts = 0;
        nt.presentation.last_message_id.clear();
        nt.presentation.last_sender.clear();
        nt.presentation.last_sender_id.clear();
        nt.refresh_sidebar_presentation();
        if let Some(m) = nt
            .messages
            .iter()
            .max_by_key(|m| crate::clock::normalize_ts_ms(m.ts))
        {
            nt.preview = m.text.trim().to_string();
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{is_newer_semver, phone_sync_completion_message};

    #[test]
    fn tombstones_match_any_spelling_of_a_group_id() {
        let mut m = std::collections::HashMap::new();
        m.insert("2576214780450068931".to_string(), 5_i64);
        assert_eq!(super::tombstone_for(&m, "g2576214780450068931"), Some(5));
        assert_eq!(
            super::tombstone_for(&m, "group_2576214780450068931"),
            Some(5)
        );
        assert_eq!(
            super::tombstone_for(&m, "2576214780450068931_group"),
            Some(5)
        );
        assert_eq!(super::tombstone_for(&m, "123"), None);
    }

    #[test]
    fn deleted_conversation_keeps_only_messages_after_the_deletion() {
        use crate::chat::{ChatMessage, Thread};
        const T0: i64 = 1_793_948_000_000;
        let msg = |id: &str, ts: i64, out: bool, text: &str| ChatMessage {
            id: id.into(),
            ts,
            is_outgoing: out,
            text: text.into(),
            ..Default::default()
        };
        let mut t = Thread {
            id: "g1".into(),
            is_group: true,
            preview: "Nhóm · 954 thành viên".into(),
            messages: vec![
                msg("1", T0 - 1_000, true, "cũ"),
                msg("2", T0, false, "cũ 2"),
            ],
            ..Default::default()
        };
        // Nothing newer than the deletion: stays hidden.
        assert!(!super::apply_delete_cutoff(&mut t.clone(), T0, true));
        t.messages.push(msg("3", T0 + 1_000, false, "mới"));
        assert!(super::apply_delete_cutoff(&mut t, T0, true));
        assert_eq!(t.messages.len(), 1);
        assert_eq!(t.preview, "mới");
        assert!(!t.presentation.last_outgoing);
        // Seconds-based timestamps are compared in ms too.
        let mut s = Thread {
            messages: vec![msg("4", T0 / 1000, false, "x")],
            ..Default::default()
        };
        assert!(!super::apply_delete_cutoff(&mut s, T0, false));
    }

    #[test]
    fn borderless_resize_when_dragging_corner_keeps_opposite_corner_fixed() {
        use eframe::egui::{pos2, vec2, Rect, ResizeDirection};
        let start = Rect::from_min_size(pos2(100.0, 80.0), vec2(1020.0, 700.0));
        let next = super::resized_window_rect(
            start,
            vec2(120.0, 50.0),
            ResizeDirection::SouthEast,
            vec2(480.0, 360.0),
        );
        assert_eq!(next.min, start.min);
        assert_eq!(next.size(), vec2(1140.0, 750.0));
        let next = super::resized_window_rect(
            start,
            vec2(-50.0, -40.0),
            ResizeDirection::NorthWest,
            vec2(480.0, 360.0),
        );
        assert_eq!(next.max, start.max);
        assert_eq!(next.min, pos2(50.0, 40.0));
    }

    #[test]
    fn borderless_resize_when_dragging_past_minimum_preserves_anchor() {
        use eframe::egui::{pos2, vec2, Rect, ResizeDirection};
        let start = Rect::from_min_size(pos2(100.0, 80.0), vec2(1020.0, 700.0));
        let minimum = vec2(480.0, 360.0);
        for direction in [ResizeDirection::NorthWest, ResizeDirection::SouthEast] {
            let delta = if direction == ResizeDirection::NorthWest {
                vec2(2000.0, 2000.0)
            } else {
                vec2(-2000.0, -2000.0)
            };
            let next = super::resized_window_rect(start, delta, direction, minimum);
            assert_eq!(next.size(), minimum);
            if direction == ResizeDirection::NorthWest {
                assert_eq!(next.max, start.max);
            } else {
                assert_eq!(next.min, start.min);
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn borderless_resize_when_fast_drag_events_share_a_frame_finishes_on_release() {
        use eframe::egui::{
            pos2, vec2, Context, Event, Modifiers, PointerButton, RawInput, Rect, ViewportCommand,
            ViewportId,
        };
        let ctx = Context::default();
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1020.0, 700.0))),
            events: vec![
                Event::PointerMoved(pos2(1018.0, 350.0)),
                Event::PointerButton {
                    pos: pos2(1018.0, 350.0),
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                Event::PointerMoved(pos2(900.0, 350.0)),
                Event::PointerButton {
                    pos: pos2(900.0, 350.0),
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&ViewportId::ROOT)
            .unwrap()
            .outer_rect = Some(Rect::from_min_size(pos2(100.0, 80.0), vec2(1020.0, 700.0)));
        let output = ctx.run(input, super::ZaloGuiApp::handle_frameless_resize);
        assert!(output.viewport_output[&ViewportId::ROOT].commands.iter().any(|command| matches!(command, ViewportCommand::InnerSize(size) if *size == vec2(902.0, 700.0))));
        let output = ctx.run(
            RawInput::default(),
            super::ZaloGuiApp::handle_frameless_resize,
        );
        assert!(!output.viewport_output[&ViewportId::ROOT]
            .commands
            .iter()
            .any(|command| matches!(command, ViewportCommand::InnerSize(_))));
    }

    #[test]
    fn sent_message_reconciles_when_server_push_arrives_first() {
        let mut thread = crate::chat::Thread::default();
        let server = crate::chat::ChatMessage {
            id: "server-a".into(),
            client_message_id: "client-a".into(),
            text: "Server push".into(),
            is_outgoing: true,
            delivery: crate::message_model::DeliveryState::Delivered,
            ..Default::default()
        };
        super::merge_sent_message(&mut thread, server.clone());
        for local_id in ["out_client-a", "server-a"] {
            super::merge_sent_message(
                &mut thread,
                crate::chat::ChatMessage {
                    id: local_id.into(),
                    client_message_id: "client-a".into(),
                    text: "Local echo".into(),
                    is_outgoing: true,
                    ..Default::default()
                },
            );
            assert_eq!(thread.messages.len(), 1);
            assert_eq!(thread.messages[0].id, "server-a");
            assert_eq!(thread.messages[0].delivery, server.delivery);
        }
    }

    #[test]
    fn delayed_local_ack_preserves_server_chronology() {
        let mut thread = crate::chat::Thread::default();
        for (id, client, ts) in [
            ("server-a", "client-a", 1000),
            ("server-b", "client-b", 2000),
        ] {
            thread.messages.push(crate::chat::ChatMessage {
                id: id.into(),
                client_message_id: client.into(),
                ts,
                time: ts.to_string(),
                is_outgoing: true,
                ..Default::default()
            });
        }
        for ack_id in ["out_client-a", "server-a"] {
            super::merge_sent_message(
                &mut thread,
                crate::chat::ChatMessage {
                    id: ack_id.into(),
                    client_message_id: "client-a".into(),
                    ts: 3000,
                    time: "3000".into(),
                    is_outgoing: true,
                    ..Default::default()
                },
            );
            let chronology: Vec<_> = thread
                .messages
                .iter()
                .map(|message| (message.id.as_str(), message.ts, message.time.as_str()))
                .collect();
            assert_eq!(
                chronology,
                [("server-a", 1000, "1000"), ("server-b", 2000, "2000")]
            );
        }
    }

    #[test]
    fn semver_comparison_handles_v_prefix_and_prerelease() {
        assert!(is_newer_semver("v0.0.2", "0.0.1"));
        assert!(is_newer_semver("0.1.0", "0.0.9"));
        assert!(is_newer_semver("V1.0.0-beta.1", "0.9.9"));
        assert!(!is_newer_semver("v0.0.1", "0.0.1"));
        assert!(!is_newer_semver("0.0.1", "0.0.2"));
    }

    #[test]
    fn phone_sync_completion_does_not_claim_phone_messages_were_imported() {
        let none = phone_sync_completion_message(0, 0, false, None, None);
        assert!(none.contains("Lịch sử điện thoại chưa được tải vào ứng dụng"));
        assert!(none.contains("0 tin mới từ Cloud/Nhóm"));

        let cloud = phone_sync_completion_message(12, 3, false, None, None);
        assert!(cloud.contains("12 tin mới từ Cloud/Nhóm (3 hội thoại)"));
        assert!(cloud.contains("Lịch sử điện thoại chưa được tải vào ứng dụng"));

        let failed = phone_sync_completion_message(0, 0, false, Some("network"), None);
        assert!(failed.contains("Đồng bộ thất bại: network"));
    }

    #[test]
    fn completed_phone_import_reports_phone_and_cloud_counts_separately() {
        let result = phone_sync_completion_message(
            2,
            1,
            false,
            None,
            Some("Đã giải mã & đồng bộ thành công 7 tin nhắn từ điện thoại"),
        );
        assert!(result.contains("7 tin nhắn từ điện thoại"));
        assert!(result.contains("Cloud/Nhóm: 2 tin mới"));
        assert!(!result.contains("chưa được tải"));
    }

    #[test]
    fn cancelled_phone_import_does_not_claim_committed_history_was_never_loaded() {
        let note = "Đã dừng đồng bộ điện thoại; dữ liệu đã nhập được giữ lại";
        let result = phone_sync_completion_message(0, 0, true, None, Some(note));
        assert_eq!(result, note);
        assert!(!result.contains("chưa được tải"));
    }
}
