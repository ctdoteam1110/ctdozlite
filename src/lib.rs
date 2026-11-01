pub mod audio;
pub mod call;
pub mod chat;
pub mod clock;
pub mod crypto;
pub mod endpoints;
pub mod gui;
pub mod logger;
pub mod login;
pub mod media;
pub mod message_model;
pub mod param_cipher;
pub mod qr;
pub mod reactions;
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
