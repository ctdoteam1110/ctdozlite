//! Linux system-tray icon (StatusNotifierItem) that shows a small red dot while there are
//! unread messages.
//!
//! The tray runs on its own thread with a current-thread Tokio runtime, so D-Bus traffic never
//! touches the UI thread. The UI only pushes the unread total through a non-blocking channel.

use eframe::egui;
use ksni::TrayMethods;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;

/// Pixel sizes offered to the tray host; it picks the closest one to its panel height.
const ICON_SIZES: [u32; 3] = [22, 32, 64];

/// Set by the tray's "Thoát" item; the UI then quits instead of hiding to the tray.
static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Whether a tray host (StatusNotifierWatcher) currently shows our icon. Closing the
/// window only hides it while this is true, so the user can always get it back.
static HOST_ONLINE: AtomicBool = AtomicBool::new(false);

/// Take (and clear) a pending "Thoát" request from the tray menu.
pub fn take_quit_request() -> bool {
    QUIT_REQUESTED.swap(false, Ordering::AcqRel)
}

/// `true` while the tray icon is shown by a host.
pub fn host_online() -> bool {
    HOST_ONLINE.load(Ordering::Acquire)
}

/// UI-side handle; dropping it shuts the tray thread down.
pub struct TrayHandle {
    tx: mpsc::UnboundedSender<usize>,
    last_sent: Option<usize>,
}

impl TrayHandle {
    /// Forward the unread total to the tray when it changed since the last call. Never blocks.
    pub fn set_unread(&mut self, unread: usize) {
        if self.last_sent == Some(unread) {
            return;
        }
        self.last_sent = Some(unread);
        let _ = self.tx.send(unread);
    }
}

/// Start the tray thread. Returns `None` when the thread cannot be spawned; a desktop without
/// a tray host is not an error, the icon simply never shows.
pub fn spawn(ctx: egui::Context) -> Option<TrayHandle> {
    let (tx, mut rx) = mpsc::unbounded_channel::<usize>();
    std::thread::Builder::new()
        .name("ctdozlite-tray".into())
        .spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            rt.block_on(async move {
                let tray = AppTray {
                    unread: 0,
                    icons: render_icons(),
                    ctx,
                };
                // `assume_sni_available` keeps the service alive when the tray host (e.g. the
                // GNOME AppIndicator extension) starts after us or is restarted.
                let handle = match tray.assume_sni_available(true).spawn().await {
                    Ok(handle) => {
                        // `watcher_offline` flips this back when no host is running.
                        HOST_ONLINE.store(true, Ordering::Release);
                        handle
                    }
                    Err(err) => {
                        crate::logger::log_error("gui::tray::spawn", &err.to_string());
                        return;
                    }
                };
                while let Some(unread) = rx.recv().await {
                    if handle.update(|t| t.unread = unread).await.is_none() {
                        break;
                    }
                }
                handle.shutdown().await;
            });
        })
        .ok()?;
    Some(TrayHandle {
        tx,
        last_sent: None,
    })
}

/// Pre-rendered icons so the tray host's frequent property reads do no image work.
struct TrayIcons {
    normal: Vec<ksni::Icon>,
    unread: Vec<ksni::Icon>,
}

struct AppTray {
    unread: usize,
    icons: TrayIcons,
    ctx: egui::Context,
}

impl AppTray {
    fn show_window(&self) {
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.ctx.request_repaint();
    }
}

impl ksni::Tray for AppTray {
    fn id(&self) -> String {
        "ctdozlite".into()
    }

    fn title(&self) -> String {
        "ctdozlite".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Communications
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        if self.unread > 0 {
            self.icons.unread.clone()
        } else {
            self.icons.normal.clone()
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let description = if self.unread > 0 {
            format!("{} tin nhắn chưa đọc", self.unread)
        } else {
            "Không có tin nhắn mới".into()
        };
        ksni::ToolTip {
            title: "ctdozlite".into(),
            description,
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.show_window();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        let status = if self.unread > 0 {
            format!("{} tin nhắn chưa đọc", self.unread)
        } else {
            "Không có tin nhắn mới".into()
        };
        vec![
            StandardItem {
                label: status,
                enabled: false,
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Mở ctdozlite".into(),
                activate: Box::new(|t: &mut Self| t.show_window()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Thoát".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| {
                    QUIT_REQUESTED.store(true, Ordering::Release);
                    t.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    t.ctx.request_repaint();
                }),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn watcher_online(&self) {
        HOST_ONLINE.store(true, Ordering::Release);
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        // No host shows the icon: closing the window must quit again, or the user could
        // not bring it back. If the window is hidden right now, show it.
        HOST_ONLINE.store(false, Ordering::Release);
        self.show_window();
        // Keep running: the host may come back (extension reload, shell restart).
        true
    }
}

fn render_icons() -> TrayIcons {
    let base = image::load_from_memory(crate::gui::theme::APP_ICON_PNG)
        .map(|img| img.into_rgba8())
        .ok();
    let render = |with_dot: bool| -> Vec<ksni::Icon> {
        let Some(base) = &base else {
            return Vec::new();
        };
        ICON_SIZES
            .iter()
            .map(|&size| {
                let mut img = image::imageops::resize(
                    base,
                    size,
                    size,
                    image::imageops::FilterType::Lanczos3,
                );
                if with_dot {
                    draw_unread_dot(&mut img);
                }
                to_ksni_icon(img)
            })
            .collect()
    };
    TrayIcons {
        normal: render(false),
        unread: render(true),
    }
}

/// Anti-aliased red dot with a white ring in the bottom-right corner, about 40% of the icon.
pub(crate) fn draw_unread_dot(img: &mut image::RgbaImage) {
    let size = img.width().min(img.height()) as f32;
    let radius = size * 0.2;
    let ring = (size * 0.06).max(1.0);
    let cx = size - radius - ring;
    let cy = size - radius - ring;
    let (w, h) = img.dimensions();
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            // Coverage of a 1px-wide edge for anti-aliasing.
            let outer = (radius + ring + 0.5 - dist).clamp(0.0, 1.0);
            if outer <= 0.0 {
                continue;
            }
            let inner = (radius + 0.5 - dist).clamp(0.0, 1.0);
            blend(img.get_pixel_mut(x, y), [255, 255, 255], outer);
            blend(img.get_pixel_mut(x, y), [230, 30, 40], inner);
        }
    }
}

fn blend(px: &mut image::Rgba<u8>, rgb: [u8; 3], alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let dst_a = px[3] as f32 / 255.0;
    let out_a = alpha + dst_a * (1.0 - alpha);
    for c in 0..3 {
        let v = (rgb[c] as f32 * alpha + px[c] as f32 * dst_a * (1.0 - alpha)) / out_a;
        px[c] = v.round().clamp(0.0, 255.0) as u8;
    }
    px[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

/// SNI pixmaps are ARGB32 in network byte order.
fn to_ksni_icon(img: image::RgbaImage) -> ksni::Icon {
    let (width, height) = img.dimensions();
    let mut data = img.into_raw();
    for px in data.chunks_exact_mut(4) {
        px.rotate_right(1);
    }
    ksni::Icon {
        width: width as i32,
        height: height as i32,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_dot_paints_bottom_right_red_and_leaves_top_left() {
        let mut img = image::RgbaImage::from_pixel(32, 32, image::Rgba([0, 0, 255, 255]));
        draw_unread_dot(&mut img);
        let centre = img.get_pixel(32 - 9, 32 - 9);
        assert!(centre[0] > 200 && centre[1] < 80 && centre[2] < 80, "{centre:?}");
        assert_eq!(*img.get_pixel(2, 2), image::Rgba([0, 0, 255, 255]));
    }

    #[test]
    fn ksni_icon_is_argb() {
        let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 4]));
        let icon = to_ksni_icon(img);
        assert_eq!(icon.data, vec![4, 1, 2, 3]);
    }
}
