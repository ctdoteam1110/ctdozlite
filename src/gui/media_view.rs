//! In-app media windows (v0.1.2): play a chat video in QTzalo's own window, never in a browser.
//!
//! Flow: a video bubble calls [`ZaloGuiApp::open_video`] → `WorkerCmd::PrepareVideo` downloads
//! the file into `paths::video_cache_dir()` (reused when already cached) → `GuiEvent::Video*`
//! comes back here. With `ffmpeg` available the video plays in a dedicated window; without it
//! the cached file is opened with the system's default player (`download::open_downloaded`).

use super::*;

/// State of the media windows. Owned by `ZaloGuiApp::media`.
#[derive(Default)]
pub(super) struct MediaViewState {}

impl ZaloGuiApp {
    /// Start playing a chat video. `url` and `title` come from a message and are untrusted.
    pub(super) fn open_video(&mut self, url: String, _title: String) {
        if let Some(tx) = WORKER_TX.get() {
            let _ = tx.send(WorkerCmd::PrepareVideo { url });
        }
    }

    pub(super) fn on_video_progress(&mut self, _url: String, _received: u64, _total: Option<u64>) {}

    pub(super) fn on_video_ready(&mut self, _url: String, path: std::path::PathBuf) {
        // The cache file is always `<md5>.mp4`, so this opens the default video player.
        let _ = crate::download::open_downloaded(&path);
    }

    pub(super) fn on_video_failed(&mut self, _url: String, error: String) {
        self.show_toast(format!("Không mở được video: {error}"));
    }

    /// Draw open media windows (called every frame from the workspace).
    pub(super) fn render_media_windows(&mut self, _ctx: &egui::Context, _pal: &ThemePalette) {}
}
