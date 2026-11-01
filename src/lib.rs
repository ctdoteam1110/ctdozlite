pub mod audio;
pub mod call;
pub mod chat;
pub mod clock;
/// Explicit outgoing mention selections and UTF-16 metadata.
pub mod composer_mentions;
pub mod crypto;
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
/// Strict validation of the existing last-online response.
pub mod presence;
pub mod qr;
pub mod reactions;
/// Durable account-owned incremental reaction inbox and replay.
pub mod reaction_journal;
/// Explicit validated import of account-bound original reaction aggregates.
pub mod original_reactions;
pub mod session;
pub mod socket;
pub mod store;
pub mod tui;
pub mod ui;
pub mod unread;
pub mod voip_log;

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

/// Native notification authorization without delivery.
pub mod native_notifications;

/// Account-isolated incremental history, explicit copy-on-write import, and bounded queries.
pub mod store_v2;
/// Pure local read-state transitions without network receipts.
pub mod read_receipts;
/// Strict source-derived remote delivery and read receipt parsing.
pub mod delivery_receipts;

/// Account-owned durable composer state without automatic resend.
pub mod composer_store;
/// Authentication generations and guarded asynchronous effects.
pub mod runtime_scope;
