pub mod account_manager;
pub mod audio;
pub mod call;
pub mod chat;
pub mod clock;
pub mod crypto;
pub mod doc_preview;
pub mod endpoints;
pub mod feed;
pub mod gui;
pub mod history_sync;
pub mod logger;
pub mod login;
pub mod media;
pub mod message_model;
/// Exact quoted-message references in loaded conversations.
pub mod message_navigation;
pub mod param_cipher;
pub mod qr;
pub mod reactions;
pub mod session;
pub mod socket;
pub mod store;
pub mod group_llm;
pub mod group_stats;
pub mod group_watch;
pub mod tui;
pub mod ui;
pub mod unread;
pub mod voip_log;
pub mod zproto_host;

/// Writable storage selection and private atomic session writes.
pub mod data_dir;
/// Bounded media downloads, cache isolation, and resource admission.
pub mod media_resources;
/// Bounded release downloads and transactional installation.
pub mod updater;
/// Scheduled message queue backed by SQLite (#38, #24).
pub mod scheduler;
/// Local HTTP REST API, MCP JSON-RPC 2.0 server, and auto-reply rules (#35).
pub mod api_mcp;
/// Fire-and-forget OS desktop banners + notification sound for chat events.
/// Shared backend for chat/message/reaction banners
/// (DailyNotifier is intentionally NOT used: its payload is hardcoded generic).
pub mod os_notify;
pub mod thread_script;

/// Native notification authorization without delivery.
pub mod native_notifications;

#[cfg(any(windows, test))]
mod clipboard_dib;
