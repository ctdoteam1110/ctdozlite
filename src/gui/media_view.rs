//! In-app media windows (v0.1.2): play a chat video in QTzalo's own window, never in a browser.
//!
//! Flow: a video bubble calls [`ZaloGuiApp::open_video`] → `WorkerCmd::PrepareVideo` downloads
//! the file into `paths::video_cache_dir()` (reused when already cached) while a small
//! "Đang tải video… 45%" card shows progress (with "Huỷ") → `GuiEvent::Video*` comes back here.
//! With `ffmpeg` available ([`crate::video::find_ffmpeg`]) the video plays in a dedicated window
//! (`Video — <title>`); without it the cached file is opened with the system's default player.
//!
//! Playback: one `ffmpeg` child decodes the video to PPM frames at a constant rate (frame `i` is
//! due at `start + i / fps`), another decodes the audio to `f32` PCM for a `cpal` output stream.
//! The wall clock is the master clock; the audio callback drops samples it should already have
//! played after an underrun, so sound stays in step with the picture. Seeking restarts both
//! decoders with `-ss`. Closing the window (or dropping the app) kills both children.

use super::*;

use std::collections::VecDeque;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::Instant;

use crate::video::{self, MediaInfo};

/// Decoded frames waiting to be shown (each at most 1280×1280 RGBA ≈ 6.5 MB).
const FRAME_QUEUE: usize = 3;
/// Audio decoded ahead of the speaker, in seconds.
const AUDIO_AHEAD_SECS: f32 = 0.75;
/// Cap on samples the audio callback may still owe after an underrun.
const MAX_OWED_SAMPLES: usize = 48_000 * 2 * 4;
/// Height of the player's control bar.
const CONTROLS_H: f32 = 66.0;
/// Arrow keys seek by this many seconds.
const SEEK_STEP_SECS: f64 = 5.0;

/// Lucide glyphs not in `theme::lucide`.
const ICON_PAUSE: &str = "\u{e12e}";
const ICON_VOLUME: &str = "\u{e1ab}";
const ICON_VOLUME_X: &str = "\u{e1ac}";
const ICON_REPLAY: &str = "\u{e148}";

/// State of the media windows. Owned by `ZaloGuiApp::media`.
#[derive(Default)]
pub(super) struct MediaViewState {
    /// Video being downloaded for playback (the "Đang tải video…" card).
    pending: Option<PendingVideo>,
    /// A cached video ready to open on the next frame (needs the egui context).
    to_open: Option<OpenRequest>,
    /// The open player window.
    player: Option<VideoWindow>,
    /// Downloads started by clicking a file card: opened with the default app once finished.
    open_when_done: HashSet<String>,
    /// Image viewer window: `(identity of the shown image, initial window size)`.
    pub(super) image_window: Option<(String, [f32; 2])>,
    /// Demo-only setup (local thumbnails, `QTZALO_DEMO_*` hooks) has run.
    demo_started: bool,
}

struct PendingVideo {
    url: String,
    title: String,
    received: u64,
    total: Option<u64>,
}

struct OpenRequest {
    path: PathBuf,
    /// Original URL ("Tải về").
    url: String,
    title: String,
}

/// App icon for QTzalo's extra windows (decoded once).
pub(super) fn media_window_icon() -> Option<Arc<egui::IconData>> {
    static ICON: OnceLock<Option<Arc<egui::IconData>>> = OnceLock::new();
    ICON.get_or_init(|| load_app_icon().map(Arc::new)).clone()
}

/// `QTZALO_DEMO_VIDEO=<absolute path of a local video>`: the file the demo's video card plays.
fn demo_video_path() -> Option<PathBuf> {
    if !is_demo_mode() {
        return None;
    }
    let p = PathBuf::from(std::env::var_os("QTZALO_DEMO_VIDEO")?);
    (p.is_absolute() && p.is_file()).then_some(p)
}

impl ZaloGuiApp {
    /// Start playing a chat video. `url` and `title` come from a message and are untrusted.
    pub(super) fn open_video(&mut self, url: String, title: String) {
        if is_demo_mode() {
            match demo_video_path() {
                // The card's URL is kept for "Tải về", which only shows a toast in demo mode.
                Some(path) => self.media.to_open = Some(OpenRequest { path, url, title }),
                None => self.show_toast(
                    "Chế độ demo: không tải video thật (đặt QTZALO_DEMO_VIDEO=<tệp .mp4> để thử trình phát).",
                ),
            }
            return;
        }
        if self.media.pending.as_ref().is_some_and(|p| p.url == url) {
            return; // already preparing this one
        }
        let Some(tx) = WORKER_TX.get() else {
            self.show_toast("Không mở được video (tiến trình nền đã dừng).");
            return;
        };
        // Look `ffmpeg` up off the GUI thread while the video downloads (cached afterwards).
        let _ = std::thread::Builder::new()
            .name("qtzalo-ffmpeg-lookup".into())
            .spawn(|| {
                let _ = video::find_ffmpeg();
            });
        if tx.send(WorkerCmd::PrepareVideo { url: url.clone() }).is_ok() {
            self.media.pending = Some(PendingVideo {
                url,
                title,
                received: 0,
                total: None,
            });
        } else {
            self.show_toast("Không mở được video (tiến trình nền đã dừng).");
        }
    }

    pub(super) fn on_video_progress(&mut self, url: String, received: u64, total: Option<u64>) {
        if let Some(p) = self.media.pending.as_mut().filter(|p| p.url == url) {
            p.received = received;
            p.total = total;
        }
    }

    pub(super) fn on_video_ready(&mut self, url: String, path: PathBuf) {
        // Ignore results the user cancelled ("Huỷ") or replaced by another video.
        let Some(pending) = self.media.pending.take_if(|p| p.url == url) else {
            return;
        };
        // Only ever hand `ffmpeg` / the system player a file from the video cache.
        if !video::is_inside_dir(&path, &crate::paths::video_cache_dir()) {
            self.show_toast("Không mở được video: tệp không nằm trong thư mục cache.");
            return;
        }
        self.media.to_open = Some(OpenRequest {
            path,
            url,
            title: pending.title,
        });
    }

    pub(super) fn on_video_failed(&mut self, url: String, error: String) {
        if self.media.pending.take_if(|p| p.url == url).is_some() {
            self.show_toast(format!("Không mở được video: {error}"));
        }
    }

    /// Click on a file / voice card: open the finished download, or download it into
    /// "Thư mục lưu file" and open it with its default app when done.
    pub(super) fn download_and_open(&mut self, url: String, suggested_name: String, thread_id: String) {
        match self.downloads.get(&url) {
            Some(DownloadState::Done(path)) => {
                let path = path.clone();
                self.open_downloaded_path(&path);
                return;
            }
            Some(DownloadState::InProgress { .. }) => {
                self.media.open_when_done.insert(url);
                return;
            }
            _ => {}
        }
        self.start_attachment_download(url.clone(), suggested_name, thread_id);
        if matches!(self.downloads.get(&url), Some(DownloadState::InProgress { .. })) {
            self.media.open_when_done.insert(url);
        }
    }

    /// Draw open media windows (called every frame from the workspace).
    pub(super) fn render_media_windows(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        if is_demo_mode() && !self.media.demo_started {
            self.media.demo_started = true;
            self.start_demo_media(ctx);
        }
        self.open_finished_downloads();
        if let Some(req) = self.media.to_open.take() {
            self.start_player(ctx, req);
        }
        self.render_video_pending(ctx, pal);
        self.render_video_window(ctx, pal);
    }

    fn open_finished_downloads(&mut self) {
        if self.media.open_when_done.is_empty() {
            return;
        }
        let downloads = &self.downloads;
        let mut finished = Vec::new();
        self.media.open_when_done.retain(|url| match downloads.get(url) {
            Some(DownloadState::InProgress { .. }) => true,
            Some(DownloadState::Done(path)) => {
                finished.push(path.clone());
                false
            }
            _ => false,
        });
        for path in finished {
            self.open_downloaded_path(&path);
        }
    }

    fn start_player(&mut self, ctx: &egui::Context, req: OpenRequest) {
        // Stop the previous video (kills its decoders) before starting another.
        self.media.player = None;
        match video::find_ffmpeg() {
            Some(ffmpeg) => {
                self.media.player = Some(VideoWindow::new(ctx, ffmpeg.to_path_buf(), req));
            }
            None if is_demo_mode() => {
                self.show_toast("Chế độ demo: không tìm thấy ffmpeg để phát video.");
            }
            None => {
                let _ = crate::download::open_downloaded(&req.path);
                self.show_toast("Máy chưa có ffmpeg — đã mở video bằng trình phát mặc định của máy.");
            }
        }
    }

    /// Demo only: local stand-ins for the sample thumbnails and the screenshot hooks
    /// (`QTZALO_DEMO_IMAGE=1` opens the image viewer, `QTZALO_DEMO_VIDEO=<file>` the player,
    /// `QTZALO_DEMO_VIDEO_PENDING=1` the download card). Nothing is fetched or written.
    fn start_demo_media(&mut self, ctx: &egui::Context) {
        let key = |url: &str| format!("thumb_{}", crate::crypto::md5_hex(url));
        for (url, image) in [
            (demo::DEMO_PHOTO_URL, demo::demo_photo_image()),
            (demo::DEMO_VIDEO_THUMB_URL, demo::demo_video_thumb_image()),
        ] {
            let tex = ctx.load_texture(key(url), image, TextureOptions::LINEAR);
            self.sticker_textures.insert(key(url), tex);
        }
        if std::env::var("QTZALO_DEMO_IMAGE").as_deref() == Ok("1") {
            self.image_viewer = Some(ImageViewerModal {
                tex_key: key(demo::DEMO_PHOTO_URL),
                hd_tex_key: None,
                full_url: demo::DEMO_PHOTO_URL.into(),
                title: "Ảnh chụp màn hình bảng màu".into(),
                sender: "Anh Minh".into(),
                time: String::new(),
                zoom: 1.0,
                pan: Vec2::ZERO,
            });
        }
        // `QTZALO_DEMO_VIDEO_PENDING=1`: show the "Đang tải video… 45%" card (no download runs).
        if std::env::var("QTZALO_DEMO_VIDEO_PENDING").as_deref() == Ok("1") {
            self.media.pending = Some(PendingVideo {
                url: demo::DEMO_VIDEO_URL.into(),
                title: "demo-onboarding.mp4".into(),
                received: 3_963_000,
                total: Some(8_806_000),
            });
        }
        if let Some(path) = demo_video_path() {
            self.media.to_open = Some(OpenRequest {
                path,
                url: demo::DEMO_VIDEO_URL.into(),
                title: "demo-onboarding.mp4".into(),
            });
        }
    }

    /// "Đang tải video… 45%" card (bottom-right of the main window) with "Huỷ".
    fn render_video_pending(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        let Some(p) = self.media.pending.as_ref() else {
            return;
        };
        let fraction = p
            .total
            .filter(|t| *t > 0)
            .map(|t| (p.received as f64 / t as f64).clamp(0.0, 1.0) as f32);
        let headline = match fraction {
            Some(f) => format!("Đang tải video… {:.0}%", f * 100.0),
            None if p.received > 0 => {
                format!("Đang tải video… {}", crate::download::format_bytes(p.received))
            }
            None => "Đang tải video…".to_string(),
        };
        let detail = match p.total {
            Some(t) if t > 0 => format!(
                "{} / {}",
                crate::download::format_bytes(p.received),
                crate::download::format_bytes(t)
            ),
            _ => "Video sẽ mở trong cửa sổ của QTzalo".to_string(),
        };
        let title = truncate_chars(&p.title, 40).into_owned();
        let mut cancel = false;
        egui::Area::new(Id::new("qtzalo_video_prepare_card"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-22.0, -92.0))
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(pal.bg_elevated)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .rounding(Rounding::same(12.0))
                    .inner_margin(Margin::symmetric(14.0, 12.0))
                    .shadow(egui::epaint::Shadow {
                        offset: Vec2::new(0.0, 4.0),
                        blur: 16.0,
                        spread: 0.0,
                        color: pal.shadow,
                    })
                    .show(ui, |ui| {
                        ui.set_width(290.0);
                        ui.horizontal(|ui| {
                            let (tile, _) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
                            ui.painter().rect_filled(tile, Rounding::same(9.0), pal.primary);
                            draw_lucide_icon(ui.painter(), tile.center(), lucide::FILM, 17.0, Color32::WHITE);
                            ui.add_space(4.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(
                                    RichText::new(&headline)
                                        .size(13.5)
                                        .color(pal.text_primary)
                                        .strong(),
                                );
                                ui.add(
                                    egui::Label::new(RichText::new(&title).size(12.0).color(pal.text_muted))
                                        .truncate(),
                                );
                            });
                        });
                        ui.add_space(9.0);
                        let (bar, _) =
                            ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
                        ui.painter().rect_filled(bar, Rounding::same(3.0), pal.border);
                        let fill = match fraction {
                            Some(f) => Rect::from_min_size(bar.min, Vec2::new(bar.width() * f, bar.height())),
                            None => {
                                let t = (ui.input(|i| i.time) as f32 * 0.7).fract();
                                let seg = bar.width() * 0.3;
                                let x0 = bar.left() - seg + (bar.width() + seg) * t;
                                Rect::from_min_max(
                                    Pos2::new(x0.max(bar.left()), bar.top()),
                                    Pos2::new((x0 + seg).min(bar.right()), bar.bottom()),
                                )
                            }
                        };
                        if fill.width() > 0.5 {
                            ui.painter().rect_filled(fill, Rounding::same(3.0), pal.primary);
                        }
                        ui.add_space(7.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&detail).size(11.5).color(pal.text_muted));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let btn = egui::Button::new(
                                    RichText::new("Huỷ").size(12.0).color(pal.text_primary),
                                )
                                .fill(pal.bg_hover)
                                .stroke(Stroke::new(1.0_f32, pal.border))
                                .rounding(Rounding::same(6.0))
                                .min_size(Vec2::new(56.0, 26.0));
                                if ui
                                    .add(btn)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text("Không mở video này")
                                    .clicked()
                                {
                                    cancel = true;
                                }
                            });
                        });
                    });
            });
        if cancel {
            self.media.pending = None;
        } else {
            ctx.request_repaint_after(Duration::from_millis(if fraction.is_some() { 250 } else { 50 }));
        }
    }

    /// The player window (`Video — <title>`), shown while `self.media.player` is set.
    fn render_video_window(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        let Some(mut win) = self.media.player.take() else {
            return;
        };
        let (frame, repaint) = win.player.update();
        if let Some(image) = frame {
            match &mut win.texture {
                Some(tex) => tex.set(image, TextureOptions::LINEAR),
                None => {
                    win.texture = Some(ctx.load_texture("qtzalo_video_frame", image, TextureOptions::LINEAR))
                }
            }
        }
        let can_download = win.url.starts_with("https://") || win.url.starts_with("http://");
        let dl_state = if can_download { self.downloads.get(&win.url).cloned() } else { None };
        let mut out = VideoUiOut::default();

        let window_title = format!("Video — {}", win.title);
        let mut builder = egui::ViewportBuilder::default()
            .with_title(window_title.clone())
            .with_inner_size(win.window_size)
            .with_min_inner_size([440.0, 320.0]);
        if let Some(icon) = media_window_icon() {
            builder = builder.with_icon(icon);
        }
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("qtzalo_video_window"),
            builder,
            |vctx, class| {
                let embedded = class == egui::ViewportClass::Embedded;
                if vctx.input(|i| i.viewport().close_requested()) {
                    out.close = true;
                }
                // Once the size is known, fit the window to the video's aspect ratio (once).
                if !win.resized {
                    if let Some(size) = win.player.info.as_ref().and_then(MediaInfo::display_size) {
                        win.resized = true;
                        if !embedded {
                            let target = video_window_size(vctx, size);
                            vctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target));
                        }
                    }
                }
                vctx.input(|i| {
                    out.toggle |= i.key_pressed(Key::Space);
                    out.close |= i.key_pressed(Key::Escape);
                    out.mute |= i.key_pressed(Key::M);
                    if i.key_pressed(Key::ArrowLeft) {
                        out.seek_by = Some(-SEEK_STEP_SECS);
                    }
                    if i.key_pressed(Key::ArrowRight) {
                        out.seek_by = Some(SEEK_STEP_SECS);
                    }
                });
                if embedded {
                    let mut open = true;
                    egui::Window::new(window_title.as_str())
                        .open(&mut open)
                        .default_size([720.0, 480.0])
                        .show(vctx, |ui| {
                            let h = (ui.available_width() * 9.0 / 16.0).max(200.0);
                            ui.allocate_ui(Vec2::new(ui.available_width(), h), |ui| {
                                video_stage(ui, &win, pal, &mut out);
                            });
                            video_controls(ui, &mut win, pal, dl_state.as_ref(), can_download, &mut out);
                        });
                    out.close |= !open;
                } else {
                    egui::TopBottomPanel::bottom("qtzalo_video_controls")
                        .exact_height(CONTROLS_H)
                        .frame(
                            egui::Frame::none()
                                .fill(pal.bg_sidebar)
                                .stroke(Stroke::new(1.0_f32, pal.border))
                                .inner_margin(Margin::symmetric(14.0, 7.0)),
                        )
                        .show(vctx, |ui| {
                            video_controls(ui, &mut win, pal, dl_state.as_ref(), can_download, &mut out);
                        });
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none().fill(Color32::BLACK))
                        .show(vctx, |ui| video_stage(ui, &win, pal, &mut out));
                }
            },
        );

        if out.close {
            // Dropping the window kills both decoders and stops the audio stream.
            return;
        }
        if out.toggle {
            win.player.toggle();
        }
        if let Some(delta) = out.seek_by {
            let t = win.player.position() + delta;
            win.player.seek(t);
        }
        if let Some(t) = out.seek {
            win.player.seek(t);
        }
        if out.mute {
            let muted = !win.player.muted;
            win.player.set_muted(muted);
        }
        if out.open_external {
            win.player.pause();
            if crate::download::open_downloaded(&win.path) == crate::download::OpenOutcome::RevealedOnly {
                self.show_toast("Không mở được tệp này bằng trình phát khác.");
            }
        }
        if let Some(action) = out.download {
            match action {
                BubbleAction::OpenDownloaded(path) => self.open_downloaded_path(&path),
                BubbleAction::Download { url, .. } => {
                    let thread_id = self
                        .threads
                        .get(self.selected_thread_index)
                        .map(|t| t.id.clone())
                        .unwrap_or_default();
                    let lower = win.title.to_lowercase();
                    let name = if [".mp4", ".mov", ".m4v", ".webm", ".mkv", ".3gp"]
                        .iter()
                        .any(|ext| lower.ends_with(ext))
                    {
                        win.title.clone()
                    } else {
                        String::new()
                    };
                    self.start_attachment_download(url, name, thread_id);
                }
                _ => {}
            }
        }
        if let Some(delay) = repaint {
            ctx.request_repaint_after(delay);
        }
        self.media.player = Some(win);
    }
}

/// Initial player size for a `(w, h)` video: native size (never upscaled), shrunk to fit 80% of
/// the monitor, at least 560 px wide, plus the control bar.
fn video_window_size(ctx: &egui::Context, (w, h): (u32, u32)) -> Vec2 {
    let monitor = ctx
        .input(|i| i.viewport().monitor_size)
        .filter(|m| m.x > 100.0 && m.y > 100.0)
        .unwrap_or(Vec2::new(1366.0, 768.0));
    let max_w = (monitor.x * 0.8).min(1280.0);
    let max_h = (monitor.y * 0.8 - CONTROLS_H).clamp(200.0, 860.0);
    let (vw, vh) = video::fit_within(w.max(1) as f32, h.max(1) as f32, max_w, max_h);
    Vec2::new(vw.clamp(560.0, max_w.max(560.0)), (vh + CONTROLS_H).max(320.0))
}

/// What the player UI asked for this frame (applied after the viewport closure).
#[derive(Default)]
struct VideoUiOut {
    close: bool,
    toggle: bool,
    mute: bool,
    open_external: bool,
    seek: Option<f64>,
    seek_by: Option<f64>,
    download: Option<BubbleAction>,
}

struct VideoWindow {
    title: String,
    /// Original URL for "Tải về" (only a web URL shows the button).
    url: String,
    /// The cached file being played.
    path: PathBuf,
    player: Player,
    texture: Option<TextureHandle>,
    /// Initial window size (kept constant so egui never fights the user's resizing).
    window_size: [f32; 2],
    resized: bool,
    /// Position under the pointer while the seek bar is dragged.
    drag_pos: Option<f64>,
}

impl VideoWindow {
    fn new(ctx: &egui::Context, ffmpeg: PathBuf, req: OpenRequest) -> Self {
        let title = req.title.trim();
        let title = if title.is_empty() { "Video" } else { title };
        Self {
            title: truncate_chars(title, 60).into_owned(),
            url: req.url,
            path: req.path.clone(),
            player: Player::new(ffmpeg, req.path, ctx.clone()),
            texture: None,
            window_size: [960.0, 540.0 + CONTROLS_H],
            resized: false,
            drag_pos: None,
        }
    }
}

/// Video area: the current frame letterboxed on black, plus state overlays. Click toggles play.
fn video_stage(ui: &mut egui::Ui, win: &VideoWindow, pal: &ThemePalette, out: &mut VideoUiOut) {
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, Rounding::ZERO, Color32::BLACK);
    if let Some(tex) = &win.texture {
        let [w, h] = tex.size();
        let (w, h) = (w.max(1) as f32, h.max(1) as f32);
        let s = (rect.width() / w).min(rect.height() / h);
        painter.image(
            tex.id(),
            Rect::from_center_size(rect.center(), Vec2::new(w * s, h * s)),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    let p = &win.player;
    let c = rect.center();
    let big_button = |icon: &str, hovered: bool| {
        painter.circle_filled(
            c,
            34.0,
            if hovered {
                pal.primary
            } else {
                Color32::from_rgba_unmultiplied(15, 23, 42, 190)
            },
        );
        painter.circle_stroke(c, 34.0, Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 200)));
        let nudge = if icon == ICON_PLAY { 3.0 } else { 0.0 };
        draw_lucide_icon(&painter, c + Vec2::new(nudge, 0.0), icon, 30.0, Color32::WHITE);
    };
    if let Some(err) = &p.error {
        painter.text(
            c - Vec2::new(0.0, 14.0),
            egui::Align2::CENTER_CENTER,
            "Không phát được video trong QTzalo",
            FontId::proportional(15.0),
            Color32::WHITE,
        );
        painter.text(
            c + Vec2::new(0.0, 10.0),
            egui::Align2::CENTER_CENTER,
            truncate_chars(err, 90),
            FontId::proportional(12.0),
            Color32::from_gray(190),
        );
        painter.text(
            c + Vec2::new(0.0, 32.0),
            egui::Align2::CENTER_CENTER,
            "Dùng “Mở bằng trình phát khác” bên dưới.",
            FontId::proportional(12.0),
            Color32::from_gray(190),
        );
        return;
    }
    if p.buffering || p.info.is_none() {
        egui::Spinner::new()
            .size(30.0)
            .color(Color32::WHITE)
            .paint_at(ui, Rect::from_center_size(c, Vec2::splat(30.0)));
        if win.texture.is_none() {
            painter.text(
                c + Vec2::new(0.0, 34.0),
                egui::Align2::CENTER_CENTER,
                "Đang mở video…",
                FontId::proportional(13.0),
                Color32::from_gray(210),
            );
        }
    } else if p.ended {
        big_button(ICON_REPLAY, resp.hovered());
        painter.text(
            c + Vec2::new(0.0, 52.0),
            egui::Align2::CENTER_CENTER,
            "Xem lại",
            FontId::proportional(13.0),
            Color32::WHITE,
        );
    } else if !p.want_playing {
        big_button(ICON_PLAY, resp.hovered());
    }
    if p.info.is_some()
        && resp
            .on_hover_cursor(CursorIcon::PointingHand)
            .clicked()
    {
        out.toggle = true;
    }
}

/// Seek bar + play/pause, time, mute, "Mở bằng trình phát khác" and "Tải về".
fn video_controls(
    ui: &mut egui::Ui,
    win: &mut VideoWindow,
    pal: &ThemePalette,
    dl_state: Option<&DownloadState>,
    can_download: bool,
    out: &mut VideoUiOut,
) {
    let duration = win.player.duration();
    let ready = win.player.info.is_some() && win.player.error.is_none();
    let pos = win.drag_pos.unwrap_or_else(|| win.player.position());

    // Seek bar.
    let (bar, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::click_and_drag());
    let track = Rect::from_center_size(bar.center(), Vec2::new(bar.width() - 12.0, 4.0));
    let frac = duration.map_or(0.0, |d| (pos / d).clamp(0.0, 1.0)) as f32;
    let painter = ui.painter();
    painter.rect_filled(track, Rounding::same(2.0), pal.border);
    painter.rect_filled(
        Rect::from_min_size(track.min, Vec2::new(track.width() * frac, track.height())),
        Rounding::same(2.0),
        pal.primary,
    );
    let knob = Pos2::new(track.left() + track.width() * frac, track.center().y);
    let hot = resp.hovered() || resp.dragged();
    painter.circle_filled(knob, if hot { 7.0 } else { 5.5 }, pal.primary);
    painter.circle_stroke(knob, if hot { 7.0 } else { 5.5 }, Stroke::new(1.5_f32, Color32::WHITE));
    if let (true, Some(d)) = (ready, duration) {
        let at = |x: f32| (((x - track.left()) / track.width()).clamp(0.0, 1.0) as f64) * d;
        if resp.dragged() || resp.drag_started() {
            if let Some(ptr) = resp.interact_pointer_pos() {
                win.drag_pos = Some(at(ptr.x));
            }
        }
        if resp.drag_stopped() {
            if let Some(t) = win.drag_pos.take() {
                out.seek = Some(t);
            }
        } else if resp.clicked() {
            if let Some(ptr) = resp.interact_pointer_pos() {
                out.seek = Some(at(ptr.x));
            }
            win.drag_pos = None;
        }
        if let Some(ptr) = resp.hover_pos() {
            let _ = resp
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text_at_pointer(video::format_clock(at(ptr.x)));
        }
    }

    ui.add_space(5.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let btn = |text: &str, size: f32, color: Color32| {
            egui::Button::new(RichText::new(text).size(size).color(color))
                .fill(pal.bg_elevated)
                .stroke(Stroke::new(1.0_f32, pal.border))
                .rounding(Rounding::same(6.0))
                .min_size(Vec2::new(34.0, 28.0))
        };
        let playing = win.player.is_playing();
        let (icon, tip) = if win.player.ended {
            (ICON_REPLAY, "Xem lại")
        } else if playing {
            (ICON_PAUSE, "Tạm dừng (Space)")
        } else {
            (ICON_PLAY, "Phát (Space)")
        };
        if ui
            .add_enabled(ready, btn(icon, 15.0, pal.text_primary))
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(tip)
            .clicked()
        {
            out.toggle = true;
        }
        let total = duration.map_or_else(|| "--:--".to_string(), video::format_clock);
        ui.label(
            RichText::new(format!("{} / {total}", video::format_clock(pos)))
                .size(12.5)
                .color(pal.text_secondary)
                .monospace(),
        );

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if can_download {
                let (label, tip) = match dl_state {
                    None => (format!("{ICON_DOWNLOAD} Tải về"), "Tải video về thư mục lưu file".to_string()),
                    Some(s @ DownloadState::InProgress { .. }) => (
                        match s.fraction() {
                            Some(f) => format!("Đang tải {:.0}%", f * 100.0),
                            None => "Đang tải…".to_string(),
                        },
                        download_progress_label(s, false),
                    ),
                    Some(DownloadState::Done(p)) => {
                        (format!("{} Mở tệp đã tải", lucide::CHECK), p.display().to_string())
                    }
                    Some(DownloadState::Failed(e)) => {
                        (format!("{ICON_RETRY} Tải lại"), format!("Tải về thất bại: {e}"))
                    }
                };
                let busy = matches!(dl_state, Some(DownloadState::InProgress { .. }));
                if busy {
                    ui.ctx().request_repaint_after(Duration::from_millis(250));
                }
                if ui
                    .add_enabled(!busy, btn(&label, 12.0, pal.text_primary))
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text(tip)
                    .clicked()
                {
                    out.download = Some(match dl_state {
                        Some(DownloadState::Done(p)) => BubbleAction::OpenDownloaded(p.clone()),
                        _ => BubbleAction::Download {
                            url: win.url.clone(),
                            suggested_name: String::new(),
                        },
                    });
                }
            }
            if ui
                .add(btn(
                    &format!("{} Mở bằng trình phát khác", lucide::EXTERNAL_LINK),
                    12.0,
                    pal.text_primary,
                ))
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text("Mở video bằng trình phát mặc định của máy")
                .clicked()
            {
                out.open_external = true;
            }
            let has_audio = win.player.has_audio();
            let (icon, tip) = match (has_audio, win.player.muted) {
                (false, _) => (ICON_VOLUME_X, "Video không có âm thanh"),
                (true, true) => (ICON_VOLUME_X, "Bật tiếng (M)"),
                (true, false) => (ICON_VOLUME, "Tắt tiếng (M)"),
            };
            if ui
                .add_enabled(has_audio, btn(icon, 15.0, pal.text_primary))
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(tip)
                .on_disabled_hover_text(tip)
                .clicked()
            {
                out.mute = true;
            }
        });
    });
}

// ---------------------------------------------------------------------------
// Playback engine
// ---------------------------------------------------------------------------

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Wait for a killed child on a helper thread so the GUI never blocks and no zombie is left.
fn reap(mut child: Child) {
    let spawned = std::thread::Builder::new()
        .name("qtzalo-ffmpeg-reap".into())
        .spawn(move || {
            let _ = child.wait();
        });
    drop(spawned);
}

/// A decoder process that can be killed from any thread, even before it was spawned.
#[derive(Default)]
struct ChildSlot {
    stopped: AtomicBool,
    child: Mutex<Option<Child>>,
}

impl ChildSlot {
    /// Keep `child` so [`Self::kill`] can stop it. Returns `false` (and kills it) when the slot
    /// was already stopped.
    fn install(&self, mut child: Child) -> bool {
        let mut slot = lock(&self.child);
        if self.stopped.load(Ordering::SeqCst) {
            let _ = child.kill();
            reap(child);
            return false;
        }
        *slot = Some(child);
        true
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    fn kill(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(mut child) = lock(&self.child).take() {
            let _ = child.kill();
            reap(child);
        }
    }

    /// Reap a decoder whose output ended (called on its reader thread).
    fn finish(&self) {
        if let Some(mut child) = lock(&self.child).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct Frame {
    pts: f64,
    image: egui::ColorImage,
}

/// One decoding run from `start` (a seek restarts it). Dropping it kills its decoders.
struct Segment {
    frames: Receiver<Frame>,
    error: Arc<Mutex<Option<String>>>,
    /// Set by the GUI when it waits for a frame; the decoder then wakes it up.
    starved: Arc<AtomicBool>,
    video: Arc<ChildSlot>,
    audio: Option<Arc<ChildSlot>>,
}

impl Segment {
    fn start(
        ffmpeg: &Path,
        path: &Path,
        start: f64,
        fps: f64,
        audio: Option<(Arc<AudioShared>, u64)>,
        wake: &egui::Context,
    ) -> Self {
        let (tx, rx) = sync_channel(FRAME_QUEUE);
        let error = Arc::new(Mutex::new(None));
        let starved = Arc::new(AtomicBool::new(true));
        let video_slot = Arc::new(ChildSlot::default());
        spawn_video_decoder(VideoJob {
            ffmpeg: ffmpeg.to_path_buf(),
            path: path.to_path_buf(),
            start,
            fps,
            tx,
            slot: video_slot.clone(),
            error: error.clone(),
            starved: starved.clone(),
            wake: wake.clone(),
        });
        let audio = audio.map(|(shared, generation)| {
            let slot = Arc::new(ChildSlot::default());
            spawn_audio_decoder(
                ffmpeg.to_path_buf(),
                path.to_path_buf(),
                start,
                shared,
                generation,
                slot.clone(),
            );
            slot
        });
        Self {
            frames: rx,
            error,
            starved,
            video: video_slot,
            audio,
        }
    }

    fn error(&self) -> Option<String> {
        lock(&self.error).clone()
    }
}

impl Drop for Segment {
    fn drop(&mut self) {
        self.video.kill();
        if let Some(a) = &self.audio {
            a.kill();
        }
        // `frames` drops after this, which unblocks a decoder waiting on the full queue.
    }
}

struct VideoJob {
    ffmpeg: PathBuf,
    path: PathBuf,
    start: f64,
    fps: f64,
    tx: SyncSender<Frame>,
    slot: Arc<ChildSlot>,
    error: Arc<Mutex<Option<String>>>,
    starved: Arc<AtomicBool>,
    wake: egui::Context,
}

/// Decoder thread: `ffmpeg … -f image2pipe -vcodec ppm pipe:1` → `Frame`s on a bounded queue.
fn spawn_video_decoder(job: VideoJob) {
    let error = job.error.clone();
    let spawned = std::thread::Builder::new()
        .name("qtzalo-video-decode".into())
        .spawn(move || {
            let VideoJob {
                ffmpeg,
                path,
                start,
                fps,
                tx,
                slot,
                error,
                starved,
                wake,
            } = job;
            let wake_gui = || wake.request_repaint_of(egui::ViewportId::ROOT);
            let mut child = match video::spawn_decoder(&ffmpeg, &video::video_decode_args(&path, start, fps)) {
                Ok(c) => c,
                Err(e) => {
                    *lock(&error) = Some(format!("không chạy được ffmpeg: {e}"));
                    drop(tx);
                    wake_gui();
                    return;
                }
            };
            let stdout = child.stdout.take();
            if !slot.install(child) {
                return;
            }
            let Some(stdout) = stdout else {
                slot.kill();
                return;
            };
            let mut reader = BufReader::with_capacity(1 << 20, stdout);
            let mut index: u64 = 0;
            loop {
                match video::read_ppm_frame(&mut reader, video::MAX_ACCEPTED_FRAME_DIM) {
                    Ok(Some((w, h, rgb))) => {
                        let image = egui::ColorImage::from_rgb([w as usize, h as usize], &rgb);
                        let frame = Frame {
                            pts: start + index as f64 / fps,
                            image,
                        };
                        index += 1;
                        if tx.send(frame).is_err() {
                            break; // the segment was dropped
                        }
                        if starved.swap(false, Ordering::SeqCst) {
                            wake_gui();
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        if index == 0 && !slot.is_stopped() {
                            *lock(&error) = Some(format!("không giải mã được video ({e})"));
                        }
                        break;
                    }
                }
            }
            if index == 0 && !slot.is_stopped() && lock(&error).is_none() {
                *lock(&error) = Some("ffmpeg không trả về khung hình nào".into());
            }
            drop(reader);
            slot.finish();
            drop(tx);
            wake_gui();
        });
    if spawned.is_err() {
        *lock(&error) = Some("không tạo được luồng giải mã".into());
    }
}

/// Decoder thread for the sound: `ffmpeg … -f f32le pipe:1` at the output device's format into
/// [`AudioShared::buf`], at most [`AUDIO_AHEAD_SECS`] ahead. Drops its data after a seek.
fn spawn_audio_decoder(
    ffmpeg: PathBuf,
    path: PathBuf,
    start: f64,
    shared: Arc<AudioShared>,
    generation: u64,
    slot: Arc<ChildSlot>,
) {
    let spawned = std::thread::Builder::new()
        .name("qtzalo-audio-decode".into())
        .spawn(move || {
            // The output thread publishes the device format; wait for it (bounded).
            let waited = Instant::now();
            let format = loop {
                if let Some(f) = shared.format.get() {
                    break *f;
                }
                if slot.is_stopped() || waited.elapsed() > Duration::from_secs(3) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            let Some((rate, channels)) = format else {
                return; // no usable output device: play video only
            };
            let args = video::audio_decode_args(&path, start, rate, channels);
            let Ok(mut child) = video::spawn_decoder(&ffmpeg, &args) else {
                return;
            };
            let stdout = child.stdout.take();
            if !slot.install(child) {
                return;
            }
            let Some(mut stdout) = stdout else {
                slot.kill();
                return;
            };
            let ch = channels.max(1) as usize;
            let frame_bytes = 4 * ch;
            let ahead = ((rate as f32 * AUDIO_AHEAD_SECS) as usize).max(1024) * ch;
            let mut carry: Vec<u8> = Vec::with_capacity(32 * 1024);
            let mut chunk = vec![0u8; 16 * 1024];
            'decode: loop {
                // Back-pressure: sleep until the speaker has used some of the buffer.
                {
                    let mut buf = lock(&shared.buf);
                    loop {
                        if slot.is_stopped() || shared.stop.load(Ordering::Relaxed) || buf.generation != generation {
                            break 'decode;
                        }
                        if buf.samples.len() < ahead {
                            break;
                        }
                        buf = shared
                            .space
                            .wait_timeout(buf, Duration::from_millis(200))
                            .map(|(g, _)| g)
                            .unwrap_or_else(|e| e.into_inner().0);
                    }
                }
                let n = match stdout.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                carry.extend_from_slice(&chunk[..n]);
                let whole = carry.len() / frame_bytes * frame_bytes;
                {
                    let mut buf = lock(&shared.buf);
                    if buf.generation != generation {
                        break; // a seek replaced this segment
                    }
                    buf.samples.extend((0..whole).step_by(4).map(|i| {
                        f32::from_le_bytes([carry[i], carry[i + 1], carry[i + 2], carry[i + 3]])
                    }));
                }
                carry.drain(..whole);
            }
            drop(stdout);
            slot.finish();
        });
    drop(spawned);
}

#[derive(Default)]
struct AudioBuf {
    /// Segment the samples belong to (bumped on every seek).
    generation: u64,
    samples: VecDeque<f32>,
    /// Samples the callback had to fill with silence while playing; dropped from the next data
    /// so the sound catches up with the picture.
    owed: usize,
}

/// State shared between the player, the audio decoder and the `cpal` callback.
#[derive(Default)]
struct AudioShared {
    buf: Mutex<AudioBuf>,
    /// Signalled when the callback consumed samples (wakes the decoder).
    space: Condvar,
    playing: AtomicBool,
    muted: AtomicBool,
    stop: AtomicBool,
    /// `(sample rate, channels)` of the output stream, or `None` without a usable device.
    format: OnceLock<Option<(u32, u16)>>,
}

/// The `cpal` output stream (on its own thread, since streams are not `Send` everywhere).
struct AudioOut {
    shared: Arc<AudioShared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl AudioOut {
    fn start() -> Option<Self> {
        let shared = Arc::new(AudioShared::default());
        let s = shared.clone();
        let thread = std::thread::Builder::new()
            .name("qtzalo-video-audio".into())
            .spawn(move || run_audio_output(s))
            .ok()?;
        Some(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// Start a new segment: drop queued sound and stay silent until the picture starts.
    fn reset(&self, generation: u64) {
        self.shared.playing.store(false, Ordering::SeqCst);
        let mut buf = lock(&self.shared.buf);
        buf.generation = generation;
        buf.samples.clear();
        buf.owed = 0;
        self.shared.space.notify_all();
    }
}

impl Drop for AudioOut {
    fn drop(&mut self) {
        self.shared.playing.store(false, Ordering::SeqCst);
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.space.notify_all();
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
        }
    }
}

fn audio_stream_error(_err: cpal::StreamError) {}

fn run_audio_output(shared: Arc<AudioShared>) {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let unavailable = |shared: &AudioShared| {
        let _ = shared.format.set(None);
    };
    let Some(device) = cpal::default_host().default_output_device() else {
        return unavailable(&shared);
    };
    let Ok(supported) = device.default_output_config() else {
        return unavailable(&shared);
    };
    let channels = supported.channels().max(1);
    let rate = supported.sample_rate().0;
    let config: cpal::StreamConfig = supported.config();
    let ch = channels as usize;
    let s = shared.clone();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| fill_audio(&s, data, ch),
            audio_stream_error,
            None,
        ),
        cpal::SampleFormat::I16 => {
            let mut scratch: Vec<f32> = Vec::new();
            device.build_output_stream(
                &config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    scratch.resize(data.len(), 0.0);
                    fill_audio(&s, &mut scratch, ch);
                    for (d, v) in data.iter_mut().zip(&scratch) {
                        *d = (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    }
                },
                audio_stream_error,
                None,
            )
        }
        _ => return unavailable(&shared),
    };
    let Ok(stream) = stream else {
        return unavailable(&shared);
    };
    if stream.play().is_err() {
        return unavailable(&shared);
    }
    let _ = shared.format.set(Some((rate, channels)));
    while !shared.stop.load(Ordering::SeqCst) {
        std::thread::park_timeout(Duration::from_millis(500));
    }
    drop(stream);
}

/// `cpal` callback: interleaved samples from the buffer (silence while paused or muted).
fn fill_audio(shared: &AudioShared, out: &mut [f32], channels: usize) {
    if !shared.playing.load(Ordering::Relaxed) {
        out.fill(0.0);
        return;
    }
    let muted = shared.muted.load(Ordering::Relaxed);
    let ch = channels.max(1);
    let mut buf = lock(&shared.buf);
    // Catch up after an underrun: drop what should already have been heard.
    let pay = (buf.owed.min(buf.samples.len()) / ch) * ch;
    buf.samples.drain(..pay);
    buf.owed -= pay;
    let n = (out.len().min(buf.samples.len()) / ch) * ch;
    for (o, s) in out[..n].iter_mut().zip(buf.samples.drain(..n)) {
        *o = if muted { 0.0 } else { s };
    }
    out[n..].fill(0.0);
    buf.owed = (buf.owed + (out.len() - n)).min(MAX_OWED_SAMPLES / ch * ch);
    drop(buf);
    shared.space.notify_one();
}

/// Playback state for one file: probing, the current [`Segment`], the clock and the audio.
struct Player {
    ffmpeg: PathBuf,
    path: PathBuf,
    wake: egui::Context,
    probe: Option<Receiver<Result<MediaInfo, String>>>,
    info: Option<MediaInfo>,
    fps: f64,
    error: Option<String>,
    segment: Option<Segment>,
    audio: Option<AudioOut>,
    audio_generation: u64,
    /// Position at `clock_started` (or the paused position).
    clock_base: f64,
    clock_started: Option<Instant>,
    want_playing: bool,
    /// Waiting for the first frame of the current segment.
    buffering: bool,
    ended: bool,
    muted: bool,
    ever_shown: bool,
    next: Option<Frame>,
    last_pts: f64,
}

impl Player {
    fn new(ffmpeg: PathBuf, path: PathBuf, wake: egui::Context) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let (ff, p, w) = (ffmpeg.clone(), path.clone(), wake.clone());
        let spawned = std::thread::Builder::new()
            .name("qtzalo-video-probe".into())
            .spawn(move || {
                let _ = tx.send(video::probe(&ff, &p));
                w.request_repaint_of(egui::ViewportId::ROOT);
            });
        let error = spawned.is_err().then(|| "không tạo được luồng đọc video".to_string());
        Self {
            ffmpeg,
            path,
            wake,
            probe: Some(rx),
            info: None,
            fps: video::DEFAULT_PLAYBACK_FPS,
            error,
            segment: None,
            audio: None,
            audio_generation: 0,
            clock_base: 0.0,
            clock_started: None,
            want_playing: true,
            buffering: true,
            ended: false,
            muted: false,
            ever_shown: false,
            next: None,
            last_pts: 0.0,
        }
    }

    fn duration(&self) -> Option<f64> {
        self.info
            .as_ref()
            .and_then(|i| i.duration)
            .filter(|d| d.is_finite() && *d > 0.0)
    }

    fn position(&self) -> f64 {
        let p = self.clock_base + self.clock_started.map_or(0.0, |t| t.elapsed().as_secs_f64());
        match self.duration() {
            Some(d) => p.min(d),
            None => p,
        }
    }

    fn is_playing(&self) -> bool {
        self.want_playing && !self.ended && self.error.is_none()
    }

    fn has_audio(&self) -> bool {
        self.audio
            .as_ref()
            .is_some_and(|a| !matches!(a.shared.format.get(), Some(None)))
    }

    fn set_audio_playing(&self, on: bool) {
        if let Some(a) = &self.audio {
            a.shared.playing.store(on, Ordering::SeqCst);
        }
    }

    fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if let Some(a) = &self.audio {
            a.shared.muted.store(muted, Ordering::SeqCst);
        }
    }

    fn poll_probe(&mut self) {
        let Some(rx) = &self.probe else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("không đọc được thông tin video".to_string()),
        };
        self.probe = None;
        match result {
            Ok(info) if info.has_video => {
                self.fps = video::choose_playback_fps(info.fps);
                if info.has_audio {
                    self.audio = AudioOut::start();
                }
                self.info = Some(info);
                self.start_segment(0.0);
            }
            Ok(_) => self.error = Some("tệp không có hình ảnh để phát".into()),
            Err(e) => self.error = Some(e),
        }
    }

    fn start_segment(&mut self, at: f64) {
        self.segment = None; // kills the previous decoders first
        self.next = None;
        self.audio_generation += 1;
        let generation = self.audio_generation;
        let audio = self.audio.as_ref().map(|a| {
            a.reset(generation);
            (a.shared.clone(), generation)
        });
        self.segment = Some(Segment::start(&self.ffmpeg, &self.path, at, self.fps, audio, &self.wake));
        self.clock_base = at;
        self.clock_started = None;
        self.buffering = true;
        self.ended = false;
    }

    fn toggle(&mut self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    fn play(&mut self) {
        if self.error.is_some() || self.info.is_none() {
            return;
        }
        self.want_playing = true;
        if self.ended {
            self.start_segment(0.0);
            return;
        }
        if !self.buffering && self.clock_started.is_none() {
            self.clock_started = Some(Instant::now());
            self.set_audio_playing(true);
        }
    }

    fn pause(&mut self) {
        self.want_playing = false;
        if let Some(t) = self.clock_started.take() {
            self.clock_base += t.elapsed().as_secs_f64();
        }
        self.set_audio_playing(false);
    }

    /// Restart decoding at `t` (frame-accurate: `-ss` before `-i` decodes from the previous
    /// keyframe and discards up to `t`). Keeps the play / pause state.
    fn seek(&mut self, t: f64) {
        if self.info.is_none() || self.error.is_some() {
            return;
        }
        let max = self.duration().map_or(f64::MAX, |d| (d - 0.1).max(0.0));
        self.start_segment(t.clamp(0.0, max));
    }

    fn finish_playback(&mut self) {
        let end = self
            .duration()
            .unwrap_or(self.last_pts + 1.0 / self.fps)
            .max(self.clock_base);
        self.clock_started = None;
        self.clock_base = end;
        self.ended = true;
        self.want_playing = false;
        self.set_audio_playing(false);
    }

    /// Advance playback. Returns the newest frame that became due (to upload) and when the GUI
    /// should come back (`None` = only on input: paused, ended or failed).
    fn update(&mut self) -> (Option<egui::ColorImage>, Option<Duration>) {
        self.poll_probe();
        if self.error.is_some() {
            return (None, None);
        }
        let Some(seg) = self.segment.as_ref() else {
            return (None, Some(Duration::from_millis(30))); // still probing
        };
        let mut newest = None;
        let mut finished = false;
        loop {
            if self.next.is_none() {
                match seg.frames.try_recv() {
                    Ok(f) => self.next = Some(f),
                    Err(TryRecvError::Empty) => {
                        seg.starved.store(true, Ordering::SeqCst);
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        finished = true;
                        break;
                    }
                }
            }
            let Some(pts) = self.next.as_ref().map(|f| f.pts) else {
                break;
            };
            if self.buffering {
                // The first frame of a segment shows at once and starts the clock.
                self.buffering = false;
                self.clock_base = pts;
                if self.want_playing {
                    self.clock_started = Some(Instant::now());
                    self.set_audio_playing(true);
                }
            } else if pts > self.position() + 0.004 {
                break;
            }
            if let Some(f) = self.next.take() {
                self.last_pts = f.pts;
                newest = Some(f.image);
            }
        }
        if newest.is_some() {
            self.ever_shown = true;
        }

        let mut repaint = None;
        if finished && self.next.is_none() {
            if self.buffering {
                // Nothing decoded in this segment: a broken file, or a seek past the end.
                self.buffering = false;
                match seg.error() {
                    Some(e) if !self.ever_shown => {
                        self.error = Some(e);
                        return (newest, None);
                    }
                    _ => self.finish_playback(),
                }
            } else if self.clock_started.is_some() {
                let end_at = self.last_pts + 1.0 / self.fps;
                let pos = self.position();
                if pos + 0.004 >= end_at || self.duration().is_some_and(|d| pos >= d) {
                    self.finish_playback();
                } else {
                    repaint = Some(Duration::from_secs_f64((end_at - pos).clamp(0.001, 0.1)));
                }
            }
        }
        if repaint.is_none() {
            repaint = if self.buffering {
                Some(Duration::from_millis(15))
            } else if self.clock_started.is_some() {
                Some(match &self.next {
                    Some(f) => Duration::from_secs_f64((f.pts - self.position()).clamp(0.001, 0.1)),
                    None => Duration::from_millis(10),
                })
            } else {
                None
            };
        }
        (newest, repaint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared(samples: &[f32]) -> AudioShared {
        let s = AudioShared::default();
        s.playing.store(true, Ordering::SeqCst);
        lock(&s.buf).samples.extend(samples.iter().copied());
        s
    }

    #[test]
    fn audio_fill_pauses_mutes_and_catches_up() {
        // Paused: silence, nothing consumed.
        let s = shared(&[0.1, 0.2, 0.3, 0.4]);
        s.playing.store(false, Ordering::SeqCst);
        let mut out = [9.0f32; 4];
        fill_audio(&s, &mut out, 2);
        assert_eq!(out, [0.0; 4]);
        assert_eq!(lock(&s.buf).samples.len(), 4);

        // Playing with an underrun: plays what exists, owes the rest (whole frames only).
        s.playing.store(true, Ordering::SeqCst);
        let mut out = [9.0f32; 6];
        fill_audio(&s, &mut out, 2);
        assert_eq!(out, [0.1, 0.2, 0.3, 0.4, 0.0, 0.0]);
        assert_eq!(lock(&s.buf).owed, 2);

        // Late data: the owed frame is skipped so sound lines up with the picture again.
        lock(&s.buf).samples.extend([1.0, 1.0, 0.5, 0.6]);
        let mut out = [9.0f32; 2];
        fill_audio(&s, &mut out, 2);
        assert_eq!(out, [0.5, 0.6]);
        assert_eq!(lock(&s.buf).owed, 0);

        // Muted: consumed but silent.
        lock(&s.buf).samples.extend([0.7, 0.8]);
        s.muted.store(true, Ordering::SeqCst);
        let mut out = [9.0f32; 2];
        fill_audio(&s, &mut out, 2);
        assert_eq!(out, [0.0, 0.0]);
        assert!(lock(&s.buf).samples.is_empty());
    }

    #[test]
    fn child_slot_kills_late_children() {
        let slot = ChildSlot::default();
        slot.kill();
        assert!(slot.is_stopped());
        // A child installed after `kill` is stopped at once (no orphan decoder).
        let cmd = if cfg!(windows) { "ping" } else { "sleep" };
        let args: &[&str] = if cfg!(windows) { &["-n", "30", "127.0.0.1"] } else { &["30"] };
        if let Ok(child) = std::process::Command::new(cmd)
            .args(args)
            .stdout(std::process::Stdio::null())
            .spawn()
        {
            assert!(!slot.install(child));
            assert!(lock(&slot.child).is_none());
        }
    }
}
