use super::*;

// ---------------------------------------------------------------------------
// Chat Bubble & Sticker Widget Rendering
// ---------------------------------------------------------------------------

pub(super) fn is_sticker_message(msg: &ChatMessage) -> Option<(u64, u64)> {
    if let Some(sid) = msg.sticker_id {
        if sid > 0 {
            return Some((sid, msg.sticker_cate_id.unwrap_or(0)));
        }
    }
    let check_ami = |s: &str| -> Option<(u64, u64)> {
        let pos = s.find("ami_fat_cat_")?;
        let num_str = s.get(pos + 12..pos + 14)?;
        let idx = num_str.parse::<u64>().ok()?;
        if (1..=15).contains(&idx) {
            Some((999000 + idx, AMI_FAT_CAT_CATE_ID))
        } else {
            None
        }
    };
    if let Some(hit) = check_ami(&msg.text)
        .or_else(|| msg.link_desc.as_deref().and_then(check_ami))
        .or_else(|| msg.link_title.as_deref().and_then(check_ami))
        .or_else(|| msg.link_url.as_deref().and_then(check_ami))
        .or_else(|| msg.link_thumb.as_deref().and_then(check_ami))
    {
        return Some(hit);
    }
    if msg.text.trim() == "[Nhãn dán]" {
        return Some((18009, 10130));
    }
    None
}

pub(super) fn render_sticker_widget(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
) -> bool {
    let Some((sid, cid)) = is_sticker_message(msg) else {
        return false;
    };

    // Store stickers load their animation frame strip (`stks_` keeps it apart from the static
    // `stk_` thumbnails cached by older builds).
    let (key, url) = if cid == 0 && (1..=30).contains(&sid) {
        (
            format!("stk_{cid}_{sid}"),
            format!("https://stc-chat.zdn.vn/images/stickers/default/thumb/{sid}.png"),
        )
    } else {
        (
            format!("stks_{cid}_{sid}"),
            format!("{}?eid={sid}&size=130", crate::endpoints::STICKER_SPRITE),
        )
    };

    if !sticker_textures.contains_key(&key) && loading_stickers.insert(key.clone()) {
        if let Some(tx) = WORKER_TX.get() {
            let _ = tx.send(WorkerCmd::FetchStickerImage {
                key: key.clone(),
                url,
            });
        }
    }

    if let Some(tex) = sticker_textures.get(&key) {
        paint_sticker_frames(ui, tex, Vec2::splat(116.0));
        true
    } else {
        false
    }
}

/// Draw a sticker texture; a horizontal frame strip plays at Zalo's 250 ms per frame
/// (first frame only with Reduce Motion).
pub(super) fn paint_sticker_frames(
    ui: &mut egui::Ui,
    tex: &TextureHandle,
    size: Vec2,
) -> egui::Response {
    let [w, h] = tex.size();
    let frames = if h > 0 && w >= 2 * h {
        (w / h).max(1)
    } else {
        1
    };
    let frame = if frames > 1 && !reduce_motion() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
        ((ui.input(|i| i.time) * 4.0) as usize) % frames
    } else {
        0
    };
    let step = 1.0 / frames as f32;
    let uv = Rect::from_min_max(
        Pos2::new(frame as f32 * step, 0.0),
        Pos2::new((frame + 1) as f32 * step, 1.0),
    );
    // Painted directly: `egui::Image` keeps the *texture's* aspect ratio, so a 16:1 frame
    // strip squeezed into the square slot came out as a thin 116×7 band.
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
    resp
}

#[derive(Debug, Clone)]
pub(super) struct CallCardView {
    pub(super) title: String,
    pub(super) status_badge: String,
    pub(super) reason: Option<String>,
    pub(super) is_video: bool,
    pub(super) is_missed_or_failed: bool,
}

/// Detect whether `msg` is a voice/video call log entry and parse its structured metadata.
pub(super) fn extract_call_card_view(msg: &ChatMessage) -> Option<CallCardView> {
    let raw = msg.text.trim();
    let is_call_id = msg.id.starts_with("call_");
    let is_call_text = raw == "Cuộc gọi"
        || raw == "Cuộc gọi thoại"
        || raw == "Cuộc gọi video"
        || raw.starts_with("📞")
        || raw.starts_with("📹")
        || raw.starts_with("Cuộc gọi đi")
        || raw.starts_with("Cuộc gọi đến")
        || raw.starts_with("Cuộc gọi video");
    if !is_call_id && !is_call_text {
        return None;
    }

    let lower = raw.to_lowercase();
    let is_video = lower.contains("video") || raw.starts_with("📹");
    let is_incoming = lower.contains("gọi đến") || (!lower.contains("gọi đi") && !msg.is_outgoing);

    // Parse optional duration `(Ns)`
    let secs_opt: Option<u64> = raw.find('(').and_then(|open| {
        let rest = &raw[open + 1..];
        let close = rest.find("s)")?;
        rest[..close].trim().parse::<u64>().ok()
    });

    // Parse optional failure/rejection reason after `—` or `-`
    let raw_reason = raw
        .split_once(" — ")
        .or_else(|| raw.split_once(" - "))
        .map(|(_, r)| r.trim().to_string())
        .filter(|r| !r.is_empty());

    let is_missed_or_failed = match secs_opt {
        Some(0) => true,
        Some(_) => raw_reason.is_some(),
        None => raw_reason.is_some() || (!msg.is_outgoing && raw == "Cuộc gọi"),
    };

    // Telegram-style wording, short enough for a compact card.
    let title = match (is_video, is_incoming) {
        (true, true) => "Cuộc gọi video đến",
        (true, false) => "Cuộc gọi video đi",
        (false, true) => "Cuộc gọi đến",
        (false, false) => "Cuộc gọi đi",
    }
    .to_string();

    let arrow = if is_incoming { "↙" } else { "↗" };
    let status_badge = match (is_missed_or_failed, secs_opt) {
        (true, _) => {
            let short = raw_reason
                .as_deref()
                .filter(|r| r.chars().count() <= 24)
                .map(str::to_string);
            let txt = match short {
                Some(r) => r,
                None if is_incoming => "Nhỡ".to_string(),
                None => "Không trả lời".to_string(),
            };
            format!("{arrow} {txt}")
        }
        (false, Some(s)) if s >= 60 => format!("{arrow} {} phút {:02} giây", s / 60, s % 60),
        (false, Some(s)) => format!("{arrow} {s} giây"),
        (false, None) => format!("{arrow} Đã kết thúc"),
    };

    // A longer reason (dropped connection, declined) goes under the status line.
    let reason = raw_reason
        .filter(|r| !is_missed_or_failed || r.chars().count() > 24)
        .map(|r| {
            if r.contains("gián đoạn") || r.contains("từ chối") {
                "Bị từ chối hoặc mất kết nối".to_string()
            } else if r.contains("đang bận") {
                "Người nhận đang bận".to_string()
            } else {
                r
            }
        });

    Some(CallCardView {
        title,
        status_badge,
        reason,
        is_video,
        is_missed_or_failed,
    })
}

/// `v` limited to `lo..=hi`, but never panics: when the space is narrower than `lo` (`hi < lo`),
/// the available width `hi` wins. Use instead of `f32::clamp` whenever `hi` depends on layout.
pub(super) fn fit_between(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi)
}

pub(super) fn render_call_card_widget(
    ui: &mut egui::Ui,
    card: &CallCardView,
    pal: &ThemePalette,
    card_width: f32,
    outgoing: bool,
) -> Option<BubbleAction> {
    // Ink of the bubble the card sits in (own bubbles are dark graphite with light text).
    let (ink, meta) = if outgoing {
        (pal.bubble_out_text, pal.bubble_out_meta)
    } else {
        (pal.bubble_in_text, pal.bubble_in_meta)
    };
    // Only a missed / failed call is red; everything else follows the bubble.
    let missed = Color32::from_rgb(239, 68, 68);
    let status_col = if card.is_missed_or_failed {
        missed
    } else {
        meta
    };

    // Compact like Telegram: icon, title, one status line. The whole card calls back.
    let text_w = (card_width - CALL_ICON_W).max(80.0);
    let resp = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);
            let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(26.0, 36.0), Sense::hover());
            let ic = icon_rect.center();
            if card.is_video {
                draw_icon_video(ui.painter(), ic, 21.0, ink);
            } else {
                draw_icon_phone(ui.painter(), ic, 21.0, ink);
            }
            ui.vertical(|ui| {
                ui.set_max_width(text_w);
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.add(
                    egui::Label::new(RichText::new(&card.title).size(13.5).color(ink).strong())
                        .wrap(),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(&card.status_badge)
                            .size(12.0)
                            .color(status_col),
                    )
                    .wrap(),
                );
                if let Some(reason) = &card.reason {
                    ui.add(egui::Label::new(RichText::new(reason).size(11.2).color(meta)).wrap());
                }
            });
        })
        .response;
    let click = ui.interact(resp.rect, resp.id.with("call_again"), Sense::click());
    let tip = if card.is_video {
        "Gọi video lại"
    } else {
        "Gọi lại"
    };
    if click
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(tip)
        .clicked()
    {
        Some(BubbleAction::CallAgain {
            is_video: card.is_video,
        })
    } else {
        None
    }
}

/// Icon column plus gap of a call card.
const CALL_ICON_W: f32 = 36.0;

/// Width a call card needs for its own text (no wider than `max_w`).
fn call_card_width(ui: &egui::Ui, card: &CallCardView, max_w: f32) -> f32 {
    let w = |t: &str, size: f32| {
        ui.fonts(|f| {
            f.layout_no_wrap(t.to_owned(), FontId::proportional(size), Color32::WHITE)
                .size()
                .x
        })
    };
    let mut text = w(&card.title, 13.5).max(w(&card.status_badge, 12.0));
    if let Some(r) = &card.reason {
        text = text.max(w(r, 11.2).min(180.0));
    }
    fit_between(CALL_ICON_W + text + 4.0, 140.0, max_w.min(260.0)).ceil()
}

/// Open an `http://` or `https://` URL in the user's default system browser.
pub(super) fn open_external_url(url: &str) {
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
                5, // SW_SHOW
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
/// Open a native OS file-picker dialog on a worker thread and return the selected path.
pub(super) fn pick_native_file_dialog(images_only: bool) -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        struct OpenFileNameW {
            l_struct_size: u32,
            hwnd_owner: *mut std::ffi::c_void,
            h_instance: *mut std::ffi::c_void,
            lpstr_filter: *const u16,
            lpstr_custom_filter: *mut u16,
            n_max_cust_filter: u32,
            n_filter_index: u32,
            lpstr_file: *mut u16,
            n_max_file: u32,
            lpstr_file_title: *mut u16,
            n_max_file_title: u32,
            lpstr_initial_dir: *const u16,
            lpstr_title: *const u16,
            flags: u32,
            n_file_offset: u16,
            n_file_extension: u16,
            lpstr_def_ext: *const u16,
            l_cust_data: usize,
            lpfn_hook: *const std::ffi::c_void,
            lp_template_name: *const u16,
            pv_reserved: *mut std::ffi::c_void,
            dw_reserved: u32,
            flags_ex: u32,
        }
        #[link(name = "comdlg32")]
        extern "system" {
            fn GetOpenFileNameW(lpofn: *mut OpenFileNameW) -> i32;
        }
        let filter_raw = if images_only {
            "Image Files (*.png;*.jpg;*.jpeg;*.webp;*.gif;*.bmp)\0*.png;*.jpg;*.jpeg;*.webp;*.gif;*.bmp\0All Files (*.*)\0*.*\0\0"
        } else {
            "All Files (*.*)\0*.*\0Video & Media (*.mp4;*.mov;*.mkv;*.mp3;*.wav)\0*.mp4;*.mov;*.mkv;*.mp3;*.wav\0Documents (*.pdf;*.zip;*.docx;*.xlsx;*.txt)\0*.pdf;*.zip;*.docx;*.xlsx;*.txt\0\0"
        };
        let title_raw = if images_only {
            "Chọn hình ảnh để gửi qua Zalo\0"
        } else {
            "Chọn video hoặc tập tin đính kèm để gửi qua Zalo\0"
        };
        let filter_u16: Vec<u16> = filter_raw.encode_utf16().collect();
        let title_u16: Vec<u16> = title_raw.encode_utf16().collect();
        let mut file_buf = vec![0u16; 1024];
        // OFN_EXPLORER (0x00080000) | OFN_FILEMUSTEXIST (0x00001000) | OFN_PATHMUSTEXIST (0x00000800) | OFN_NOCHANGEDIR (0x00000008)
        let mut ofn = OpenFileNameW {
            l_struct_size: std::mem::size_of::<OpenFileNameW>() as u32,
            hwnd_owner: std::ptr::null_mut(),
            h_instance: std::ptr::null_mut(),
            lpstr_filter: filter_u16.as_ptr(),
            lpstr_custom_filter: std::ptr::null_mut(),
            n_max_cust_filter: 0,
            n_filter_index: 1,
            lpstr_file: file_buf.as_mut_ptr(),
            n_max_file: file_buf.len() as u32,
            lpstr_file_title: std::ptr::null_mut(),
            n_max_file_title: 0,
            lpstr_initial_dir: std::ptr::null(),
            lpstr_title: title_u16.as_ptr(),
            flags: 0x0008_0000 | 0x0000_1000 | 0x0000_0800 | 0x0000_0008,
            n_file_offset: 0,
            n_file_extension: 0,
            lpstr_def_ext: std::ptr::null(),
            l_cust_data: 0,
            lpfn_hook: std::ptr::null(),
            lp_template_name: std::ptr::null(),
            pv_reserved: std::ptr::null_mut(),
            dw_reserved: 0,
            flags_ex: 0,
        };
        let ok = unsafe { GetOpenFileNameW(&mut ofn) };
        if ok != 0 {
            let end = file_buf
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(file_buf.len());
            let s = String::from_utf16_lossy(&file_buf[..end]);
            if !s.is_empty() {
                Some(std::path::PathBuf::from(s))
            } else {
                None
            }
        } else {
            None
        }
    }
    #[cfg(target_os = "macos")]
    {
        let prompt = if images_only {
            "POSIX path of (choose file with prompt \"Chọn hình ảnh để gửi qua Zalo\" of type {\"public.image\"})"
        } else {
            "POSIX path of (choose file with prompt \"Chọn tập tin đính kèm để gửi qua Zalo\")"
        };
        if let Ok(out) = std::process::Command::new("osascript")
            .args(["-e", prompt])
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return Some(std::path::PathBuf::from(s));
                }
            }
        }
        None
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let mut zcmd = std::process::Command::new("zenity");
        zcmd.arg("--file-selection");
        if images_only {
            zcmd.arg("--file-filter=Images | *.png *.jpg *.jpeg *.webp *.gif *.bmp");
        }
        if let Ok(out) = zcmd.output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return Some(std::path::PathBuf::from(s));
                }
            }
        }
        if let Ok(out) = std::process::Command::new("kdialog")
            .arg("--getopenfilename")
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return Some(std::path::PathBuf::from(s));
                }
            }
        }
        None
    }
}

/// Detect `Ctrl+V` or `Shift+Insert` reliably on Windows even when `egui-winit` swallows `Key::V`
/// because the system clipboard contains an image instead of UTF-8 text.
pub(super) fn is_clipboard_paste_triggered(ctx: &egui::Context) -> bool {
    let egui_paste = ctx.input(|i| {
        (i.modifiers.ctrl && i.key_pressed(Key::V))
            || (i.modifiers.shift && i.key_pressed(Key::Insert))
            || i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)))
    });

    #[cfg(target_os = "windows")]
    {
        #[link(name = "user32")]
        extern "system" {
            fn GetAsyncKeyState(v_key: i32) -> i16;
        }
        static WAS_PASTE_DOWN: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);

        let is_focused = ctx.input(|i| i.focused);
        if !is_focused {
            WAS_PASTE_DOWN.store(false, std::sync::atomic::Ordering::SeqCst);
            return egui_paste;
        }
        let (win32_down, modifier_held) = unsafe {
            let ctrl = (GetAsyncKeyState(0x11) as u16 & 0x8000) != 0; // VK_CONTROL
            let v_key = (GetAsyncKeyState(0x56) as u16 & 0x8000) != 0; // 'V'
            let shift = (GetAsyncKeyState(0x10) as u16 & 0x8000) != 0; // VK_SHIFT
            let ins = (GetAsyncKeyState(0x2D) as u16 & 0x8000) != 0; // VK_INSERT
            ((ctrl && v_key) || (shift && ins), ctrl || shift)
        };
        if modifier_held {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        let prev = WAS_PASTE_DOWN.swap(win32_down, std::sync::atomic::Ordering::SeqCst);
        let edge_triggered = win32_down && !prev;
        return egui_paste || edge_triggered;
    }

    // macOS: egui-winit turns ⌘V into a `Paste` event only when the clipboard holds *text*.
    // A screenshot copied to the clipboard is image-only, so nothing arrived and pasting it
    // did nothing (Finder copies worked because they also carry the path as text).
    #[cfg(target_os = "macos")]
    {
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
        }
        static WAS_PASTE_DOWN: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        let (focused, cmd) = ctx.input(|i| (i.focused, i.modifiers.command));
        if cmd && focused {
            // Poll while ⌘ is held so the V press is seen even without a winit event.
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        // SAFETY: read-only key state query; 0 = combined session state, 9 = kVK_ANSI_V.
        let v_down = unsafe { CGEventSourceKeyState(0, 9) };
        let down = focused && cmd && v_down;
        let prev = WAS_PASTE_DOWN.swap(down, std::sync::atomic::Ordering::SeqCst);
        egui_paste || (down && !prev)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        egui_paste
    }
}

pub(super) fn finalize_clipboard_rgba_image(
    rgba: image::RgbaImage,
) -> Option<(std::path::PathBuf, u32, u32, egui::ColorImage)> {
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let _ = std::fs::create_dir_all("stickers_cache");
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = std::path::PathBuf::from(format!("stickers_cache/clipboard_{now_ms}.png"));
    rgba.save(&path).ok()?;

    let thumb = if width > 240 || height > 240 {
        image::imageops::thumbnail(&rgba, 240, 240)
    } else {
        rgba
    };
    let (tw, th) = thumb.dimensions();
    let col_img =
        egui::ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], thumb.as_raw());
    Some((path, width, height, col_img))
}

/// Read an image from the Windows Clipboard supporting:
/// 1. Registered `"PNG"` / `"image/png"` streams (Snipping Tool, ShareX, Chrome/Edge, Discord)
/// 2. `CF_HDROP` (`15`) copied image files from Windows Explorer or ShareX
/// 3. `CF_DIB` (`8`) / `CF_DIBV5` (`17`) 24-bit & 32-bit bitmaps (`BI_RGB` & `BI_BITFIELDS`)
pub(super) fn read_clipboard_image_to_png(
) -> Option<(std::path::PathBuf, u32, u32, egui::ColorImage)> {
    #[cfg(target_os = "windows")]
    {
        const CF_DIB: u32 = 8;
        const CF_HDROP: u32 = 15;
        #[link(name = "user32")]
        extern "system" {
            fn IsClipboardFormatAvailable(format: u32) -> i32;
            fn RegisterClipboardFormatA(lpsz_format: *const u8) -> u32;
            fn OpenClipboard(h_wnd_new_owner: *mut std::ffi::c_void) -> i32;
            fn CloseClipboard() -> i32;
            fn GetClipboardData(u_format: u32) -> *mut std::ffi::c_void;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GlobalLock(h_mem: *mut std::ffi::c_void) -> *const u8;
            fn GlobalUnlock(h_mem: *mut std::ffi::c_void) -> i32;
            fn GlobalSize(h_mem: *mut std::ffi::c_void) -> usize;
        }
        #[link(name = "shell32")]
        extern "system" {
            fn DragQueryFileW(
                h_drop: *mut std::ffi::c_void,
                i_file: u32,
                lpsz_file: *mut u16,
                cch: u32,
            ) -> u32;
        }

        unsafe {
            let cf_png = RegisterClipboardFormatA(b"PNG\0".as_ptr());
            let cf_mime_png = RegisterClipboardFormatA(b"image/png\0".as_ptr());
            let has_png = cf_png != 0 && IsClipboardFormatAvailable(cf_png) != 0;
            let has_mime_png = cf_mime_png != 0 && IsClipboardFormatAvailable(cf_mime_png) != 0;
            let has_dib = IsClipboardFormatAvailable(CF_DIB) != 0;
            let has_hdrop = IsClipboardFormatAvailable(CF_HDROP) != 0;

            if !has_png && !has_mime_png && !has_dib && !has_hdrop {
                return None;
            }

            // Retry OpenClipboard briefly in case egui-winit/arboard or ShareX is releasing the lock
            let mut opened = false;
            for _ in 0..6 {
                if OpenClipboard(std::ptr::null_mut()) != 0 {
                    opened = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(8));
            }
            if !opened {
                return None;
            }

            // 1. Try raw PNG stream first (preserves exact RGBA and avoids DIB header quirks)
            for fmt in [cf_png, cf_mime_png] {
                if fmt != 0 && IsClipboardFormatAvailable(fmt) != 0 {
                    let hmem = GetClipboardData(fmt);
                    if !hmem.is_null() {
                        let ptr = GlobalLock(hmem);
                        let len = GlobalSize(hmem);
                        if !ptr.is_null() && len > 8 {
                            let bytes = std::slice::from_raw_parts(ptr, len).to_vec();
                            GlobalUnlock(hmem);
                            if let Ok(dyn_img) = image::load_from_memory(&bytes) {
                                CloseClipboard();
                                return finalize_clipboard_rgba_image(dyn_img.to_rgba8());
                            }
                        } else if !ptr.is_null() {
                            GlobalUnlock(hmem);
                        }
                    }
                }
            }

            // 2. Try CF_HDROP (copied image file in Windows Explorer or ShareX)
            if has_hdrop {
                let hdrop = GetClipboardData(CF_HDROP);
                if !hdrop.is_null() {
                    let mut buf = vec![0u16; 1024];
                    let chars =
                        DragQueryFileW(hdrop, 0, buf.as_mut_ptr(), buf.len() as u32) as usize;
                    if chars > 0 && chars < buf.len() {
                        let file_path =
                            std::path::PathBuf::from(String::from_utf16_lossy(&buf[..chars]));
                        if let Ok(dyn_img) = image::open(&file_path) {
                            CloseClipboard();
                            let rgba = dyn_img.to_rgba8();
                            let (width, height) = rgba.dimensions();
                            let thumb = if width > 240 || height > 240 {
                                image::imageops::thumbnail(&rgba, 240, 240)
                            } else {
                                rgba
                            };
                            let (tw, th) = thumb.dimensions();
                            let col_img = egui::ColorImage::from_rgba_unmultiplied(
                                [tw as usize, th as usize],
                                thumb.as_raw(),
                            );
                            return Some((file_path, width, height, col_img));
                        }
                    }
                }
            }

            // 3. Fallback to CF_DIB (24-bit BGR or 32-bit BGRA/BGRX, BI_RGB or BI_BITFIELDS)
            if has_dib {
                let hmem = GetClipboardData(CF_DIB);
                if !hmem.is_null() {
                    let ptr = GlobalLock(hmem);
                    let len = GlobalSize(hmem);
                    if !ptr.is_null() && len >= 40 {
                        let slice = std::slice::from_raw_parts(ptr, len);
                        let bi_size =
                            u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as usize;
                        let width_i = i32::from_le_bytes([slice[4], slice[5], slice[6], slice[7]]);
                        let height_i =
                            i32::from_le_bytes([slice[8], slice[9], slice[10], slice[11]]);
                        let bit_count = u16::from_le_bytes([slice[14], slice[15]]);
                        let compression =
                            u32::from_le_bytes([slice[16], slice[17], slice[18], slice[19]]);

                        if width_i > 0
                            && height_i != 0
                            && (bit_count == 24 || bit_count == 32)
                            && bi_size >= 40
                            && bi_size <= slice.len()
                        {
                            let width = width_i as u32;
                            let top_down = height_i < 0;
                            let height = height_i.unsigned_abs();
                            let mut offset = bi_size;
                            if (compression == 3 || compression == 6) && bi_size == 40 {
                                offset += if compression == 6 { 16 } else { 12 };
                            }
                            let bytes_per_px = (bit_count / 8) as usize;
                            let row_stride = (width as usize * bytes_per_px + 3) & !3;
                            let needed =
                                offset.saturating_add(row_stride.saturating_mul(height as usize));
                            if needed <= slice.len() {
                                let mut rgba = image::RgbaImage::new(width, height);
                                let mut any_nonzero_alpha = false;
                                if bytes_per_px == 4 {
                                    for y in 0..height {
                                        let src_y =
                                            if top_down { y } else { height - 1 - y } as usize;
                                        let row = &slice[offset + src_y * row_stride..];
                                        for x in 0..width {
                                            if row[x as usize * 4 + 3] != 0 {
                                                any_nonzero_alpha = true;
                                                break;
                                            }
                                        }
                                        if any_nonzero_alpha {
                                            break;
                                        }
                                    }
                                }
                                for y in 0..height {
                                    let src_y = if top_down { y } else { height - 1 - y } as usize;
                                    let row = &slice[offset + src_y * row_stride..];
                                    for x in 0..width {
                                        let p = x as usize * bytes_per_px;
                                        let b = row[p];
                                        let g = row[p + 1];
                                        let r = row[p + 2];
                                        let a = if bytes_per_px == 4 && any_nonzero_alpha {
                                            row[p + 3]
                                        } else {
                                            255
                                        };
                                        rgba.put_pixel(x, y, image::Rgba([r, g, b, a]));
                                    }
                                }
                                GlobalUnlock(hmem);
                                CloseClipboard();
                                return finalize_clipboard_rgba_image(rgba);
                            }
                        }
                    }
                    if !ptr.is_null() {
                        GlobalUnlock(hmem);
                    }
                }
            }

            CloseClipboard();
            None
        }
    }
    #[cfg(target_os = "macos")]
    {
        // Screenshots and copied images land on NSPasteboard as PNG and/or TIFF.
        for uti in ["public.png", "public.tiff"] {
            if let Some(bytes) = macos_pasteboard::data_for_type(uti) {
                if let Ok(dyn_img) = image::load_from_memory(&bytes) {
                    return finalize_clipboard_rgba_image(dyn_img.to_rgba8());
                }
            }
        }
        None
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Wayland (`wl-paste --type image/png`) or X11 (`xclip -selection clipboard -t image/png -o`)
        for (prog, args) in [
            ("wl-paste", &["--no-newline", "--type", "image/png"][..]),
            (
                "xclip",
                &["-selection", "clipboard", "-t", "image/png", "-o"][..],
            ),
        ] {
            if let Ok(out) = std::process::Command::new(prog).args(args).output() {
                if out.status.success() && out.stdout.len() > 8 {
                    if let Ok(dyn_img) = image::load_from_memory(&out.stdout) {
                        return finalize_clipboard_rgba_image(dyn_img.to_rgba8());
                    }
                }
            }
        }
        None
    }
}

/// Whether a pasted file should go out as a photo (image upload) rather than a file.
pub(super) fn is_image_path(p: &std::path::Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp"))
}

/// Files copied in the OS file manager (Finder `⌘C`), in clipboard order. Checked before the
/// image reader: a copied file also puts its *icon* on the pasteboard as TIFF.
pub(super) fn read_clipboard_file_paths() -> Vec<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        macos_pasteboard::file_paths()
            .into_iter()
            .filter(|p| p.is_file())
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
mod macos_pasteboard {
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::{NSString, NSURL};

    /// Raw bytes of the first pasteboard item carrying `uti`.
    pub(super) fn data_for_type(uti: &str) -> Option<Vec<u8>> {
        // SAFETY: read-only queries on the general pasteboard; AppKit allows this off-main too.
        unsafe {
            let pb = NSPasteboard::generalPasteboard();
            let data = pb.dataForType(&NSString::from_str(uti))?;
            Some(data.bytes().to_vec())
        }
    }

    /// `public.file-url` of every pasteboard item, converted to local paths.
    pub(super) fn file_paths() -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        // SAFETY: as above.
        unsafe {
            let pb = NSPasteboard::generalPasteboard();
            let Some(items) = pb.pasteboardItems() else {
                return out;
            };
            let file_url = NSString::from_str("public.file-url");
            for item in items.iter() {
                let Some(s) = item.stringForType(&file_url) else {
                    continue;
                };
                if let Some(path) = NSURL::URLWithString(&s).and_then(|u| u.path()) {
                    out.push(std::path::PathBuf::from(path.to_string()));
                }
            }
        }
        out
    }
}

/// Format a URL for clean UI display by stripping tracking query strings (`?_r=1&utm_...`)
/// and truncating to `max_chars`.
pub(super) fn clean_display_url(url: &str, max_chars: usize) -> String {
    let trimmed = url.trim();
    let no_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let base = no_scheme
        .split('?')
        .next()
        .unwrap_or(no_scheme)
        .trim_end_matches('/');
    truncate_chars(base, max_chars).into_owned()
}

/// Extract `(user_comment, url, domain)` from a message when `msg.link_url` is set or when
/// `msg.text` contains an embedded `http://` / `https://` URL.
///
/// Note: `user_comment` only contains extra text typed by the sender *outside* the link's own
/// `link_title` / `link_desc` (so `link_desc` is rendered inside the Link Preview Card rather than
/// duplicated as a large paragraph above the card).
pub(super) fn extract_clean_domain(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("Liên kết")
        .trim()
        .strip_prefix("www.")
        .unwrap_or_else(|| {
            url.split("://")
                .nth(1)
                .unwrap_or(url)
                .split('/')
                .next()
                .unwrap_or("Liên kết")
                .trim()
        })
        .to_string()
}

pub(super) fn extract_message_link_view(msg: &ChatMessage) -> Option<(String, String, String)> {
    if let Some(url) = msg.link_url.as_deref().filter(|u| !u.is_empty()) {
        let desc_trim = msg.link_desc.as_deref().unwrap_or("").trim();
        let title_trim = msg.link_title.as_deref().unwrap_or("").trim();
        let url_base = url.split('?').next().unwrap_or(url);

        let mut kept_lines = Vec::new();
        for raw_line in msg.text.lines() {
            let mut line = raw_line.trim().to_string();
            if let Some(pos) = line.find("http://").or_else(|| line.find("https://")) {
                let after = &line[pos..];
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                let found = &after[..end];
                if found == url || found.starts_with(url_base) {
                    line = format!("{}{}", &line[..pos], &after[end..])
                        .trim()
                        .to_string();
                }
            }
            if line.is_empty()
                || line == url
                || line == title_trim
                || line == desc_trim
                || line == "[Chia sẻ]"
                || line == "[Liên kết]"
                || line == "[Hình ảnh]"
                || (!desc_trim.is_empty() && desc_trim.contains(&line))
                || (!title_trim.is_empty() && title_trim.contains(&line))
            {
                continue;
            }
            kept_lines.push(line);
        }
        let user_comment = kept_lines.join("\n").trim().to_string();
        let domain = extract_clean_domain(url);
        return Some((user_comment, url.to_string(), domain));
    }
    let text = msg.text.trim();
    let pos = text.find("https://").or_else(|| text.find("http://"))?;
    let after = &text[pos..];
    let end = after.find(char::is_whitespace).unwrap_or(after.len());
    let url = after[..end].trim_end_matches([')', ']', ',', ';']);
    if url.len() < 10 {
        return None;
    }
    let caption = format!("{}{}", &text[..pos], &after[end..])
        .trim()
        .to_string();
    let domain = extract_clean_domain(url);
    Some((caption, url.to_string(), domain))
}

/// Convert days since 1970-01-01 into `(year, month, day)` (Howard Hinnant's algorithm).
pub(super) fn civil_from_days(days: i64) -> (i32, u32, u32) {
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

/// Normalize a millisecond or second timestamp to milliseconds (`> 0`).
pub(super) fn normalize_ts_ms(ts: i64) -> i64 {
    if ts <= 0 {
        0
    } else if ts < 10_000_000_000 {
        ts * 1000
    } else {
        ts
    }
}

/// Return `true` when a date/time separator pill should be inserted before `curr`.
pub(super) fn should_show_time_divider(prev: Option<&ChatMessage>, curr: &ChatMessage) -> bool {
    let Some(prev) = prev else {
        return true;
    };
    let prev_ms = normalize_ts_ms(prev.ts);
    let curr_ms = normalize_ts_ms(curr.ts);
    if prev_ms > 0 && curr_ms > 0 {
        let tz_offset_sec = 7 * 3600; // UTC+7 (Vietnam)
        let prev_day = (prev_ms / 1000 + tz_offset_sec) / 86_400;
        let curr_day = (curr_ms / 1000 + tz_offset_sec) / 86_400;
        if prev_day != curr_day {
            return true;
        }
        return (curr_ms - prev_ms).abs() >= 30 * 60 * 1000;
    }
    false
}

/// Format the label for a centered date/time separator pill.
pub(super) fn format_time_divider_label(msg: &ChatMessage) -> String {
    let ts_ms = normalize_ts_ms(msg.ts);
    if ts_ms <= 0 {
        return if msg.time.is_empty() {
            "Cuộc trò chuyện".to_string()
        } else {
            format!("Hôm nay • {}", msg.time)
        };
    }
    let tz_offset_sec = 7 * 3600;
    let local_sec = ts_ms / 1000 + tz_offset_sec;
    let msg_day = local_sec.div_euclid(86_400);
    let sec_of_day = local_sec.rem_euclid(86_400);
    let hh = sec_of_day / 3600;
    let mm = (sec_of_day % 3600) / 60;

    let now_sec = crate::login::unix_now() + tz_offset_sec;
    let today_day = now_sec.div_euclid(86_400);
    let (y, m, d) = civil_from_days(msg_day);

    if msg_day == today_day {
        format!("Hôm nay • {hh:02}:{mm:02}")
    } else if msg_day == today_day - 1 {
        format!("Hôm qua • {hh:02}:{mm:02}")
    } else {
        format!("{d:02}/{m:02}/{y}")
    }
}

/// Render a centered Zalo-style compact date pill (no side hairlines).
pub(super) fn render_date_divider(ui: &mut egui::Ui, msg: &ChatMessage, pal: &ThemePalette) {
    let label = format_time_divider_label(msg);
    let avail_w = ui.available_width();
    ui.horizontal(|ui| {
        let text_w = ui.fonts(|f| {
            f.layout_no_wrap(
                label.clone(),
                FontId::proportional(11.5),
                pal.text_secondary,
            )
            .size()
            .x
        });
        let pill_w = text_w + 26.0;
        let left_space = ((avail_w - pill_w) * 0.5).max(0.0);
        if left_space > 0.0 {
            ui.add_space(left_space);
        }
        let pill_bg = if pal.is_dark {
            Color32::from_rgba_unmultiplied(30, 34, 41, 235)
        } else {
            Color32::from_rgba_unmultiplied(219, 228, 240, 215)
        };
        egui::Frame::none()
            .fill(pill_bg)
            .rounding(Rounding::same(10.0))
            .inner_margin(Margin::symmetric(13.0, 3.5))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(label)
                        .size(11.4)
                        .color(if pal.is_dark {
                            Color32::from_rgb(215, 218, 224)
                        } else {
                            pal.text_secondary
                        })
                        .strong(),
                );
            });
    });
}

/// Detect whether `msg` is a system/event notification rather than a user speech bubble.
pub(super) fn is_system_notice_message(msg: &ChatMessage) -> bool {
    if msg.id.starts_with("sys_") {
        return true;
    }
    let t = msg.text.trim();
    t.starts_with("[Hệ thống]")
        || t.starts_with("[Thông báo]")
        || (t.starts_with("Bạn và ") && t.contains("đã trở thành bạn bè"))
        || t.contains("đã thêm bạn vào nhóm")
        || t.contains("đã tham gia nhóm")
        || t.contains("đã rời khỏi nhóm")
        || t.contains("đã đổi tên nhóm")
}

/// Render a centered system event pill (e.g., friend connected, group event).
pub(super) fn render_system_notice_pill(ui: &mut egui::Ui, msg: &ChatMessage, pal: &ThemePalette) {
    let avail_w = ui.available_width();
    let text = msg.text.trim();
    ui.horizontal(|ui| {
        let text_w = ui.fonts(|f| {
            f.layout_no_wrap(
                text.to_string(),
                FontId::proportional(12.0),
                pal.text_secondary,
            )
            .size()
            .x
        });
        let pill_w = (text_w + 28.0).min(avail_w - 40.0);
        let left_pad = ((avail_w - pill_w) * 0.5).max(0.0);
        if left_pad > 0.0 {
            ui.add_space(left_pad);
        }
        egui::Frame::none()
            .fill(pal.bg_elevated)
            .stroke(Stroke::new(1.0_f32, pal.border))
            .rounding(Rounding::same(12.0))
            .inner_margin(Margin::symmetric(12.0, 4.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("{} {text}", lucide::SPARKLES))
                        .size(12.0)
                        .color(pal.text_secondary),
                );
            });
    });
}

/// Return `true` when two adjacent messages belong to the same visual bubble group.
/// Render chat message text with full Markdown support:
/// - Fenced code blocks (` ```lang ... ``` `)
/// - Headings (`#`, `##`, `###`)
/// - Bullet lists (`- `, `* `) and blockquotes (`> `)
/// - Inline `**bold**`, `*italic*`, `~~strikethrough~~`, `` `inline code` ``, and `[link](url)`
fn strip_markdown_syntax_for_width(s: &str) -> String {
    s.replace("```", "")
        .replace("**", "")
        .replace("__", "")
        .replace("~~", "")
        .replace('`', "")
}

/// Names that render as an `@mention` in the open group (members, their aliases, the account
/// owner and "All"), longest first so "@Lê Dũng Bá" wins over "@Lê".
static MENTION_NAMES: std::sync::RwLock<Vec<String>> = std::sync::RwLock::new(Vec::new());

/// Set the mention names for the conversation being drawn (empty outside groups).
pub(super) fn set_mention_names(mut names: Vec<String>) {
    names.retain(|n| !n.trim().is_empty());
    names.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    names.dedup();
    if let Ok(mut g) = MENTION_NAMES.write() {
        *g = names;
    }
}

/// Byte ranges of `@Name` tags in `text` for the given names (longest match at each `@`).
pub(super) fn find_mentions(text: &str, names: &[String]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(rel) = text[i..].find('@') {
        let at = i + rel;
        let after = &text[at + 1..];
        let prev_ok =
            at == 0 || text[..at].ends_with(|c: char| c.is_whitespace() || "([{".contains(c));
        let hit = names.iter().find(|n| {
            after.starts_with(n.as_str())
                && after[n.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric())
        });
        match hit {
            Some(n) if prev_ok => {
                let end = at + 1 + n.len();
                out.push((at, end));
                i = end;
            }
            _ => i = at + 1,
        }
    }
    out
}

/// Append plain text, colouring `@mentions` like Zalo (blue, medium weight).
fn append_with_mentions(
    job: &mut egui::text::LayoutJob,
    ctx: &egui::Context,
    text: &str,
    size: f32,
    color: Color32,
    pal: &ThemePalette,
) {
    let plain = |job: &mut egui::text::LayoutJob, t: &str| {
        if !t.is_empty() {
            super::emoji_picker::append_text_with_emoji(
                job,
                ctx,
                t,
                FontId::proportional(size),
                color,
            );
        }
    };
    let spans = MENTION_NAMES
        .read()
        .map(|names| {
            if names.is_empty() {
                Vec::new()
            } else {
                find_mentions(text, &names)
            }
        })
        .unwrap_or_default();
    if spans.is_empty() {
        plain(job, text);
        return;
    }
    let mention_col = if pal.is_dark {
        Color32::from_rgb(97, 175, 239)
    } else {
        Color32::from_rgb(0, 104, 255)
    };
    let mut last = 0;
    for (a, b) in spans {
        plain(job, &text[last..a]);
        job.append(
            &text[a..b],
            0.0,
            egui::TextFormat {
                font_id: FontId::proportional(size),
                color: mention_col,
                ..Default::default()
            },
        );
        last = b;
    }
    plain(job, &text[last..]);
}

fn render_markdown_inline_rich(
    ui: &mut egui::Ui,
    text: &str,
    base_size: f32,
    base_color: Color32,
    pal: &ThemePalette,
) {
    let mut job = egui::text::LayoutJob::default();
    let mut first_link: Option<String> = None;
    let mut rest = text;

    while !rest.is_empty() {
        let next_bold = rest.find("**");
        let next_under = rest.find("__");
        let next_strike = rest.find("~~");
        let next_code = rest.find('`');
        let next_italic = rest.find('*').filter(|&i| rest.get(i..i + 2) != Some("**"));
        let next_link = rest.find('[').and_then(|lb| {
            let after_lb = &rest[lb + 1..];
            let rb = after_lb.find("](")?;
            let after_rb = &after_lb[rb + 2..];
            let rp = after_rb.find(')')?;
            Some((lb, lb + 1 + rb, lb + 1 + rb + 2 + rp))
        });

        let min_pos = [
            next_bold,
            next_under,
            next_strike,
            next_code,
            next_italic,
            next_link.map(|(lb, _, _)| lb),
        ]
        .into_iter()
        .flatten()
        .min();
        let Some(pos) = min_pos else {
            append_with_mentions(&mut job, ui.ctx(), rest, base_size, base_color, pal);
            break;
        };

        if pos > 0 {
            append_with_mentions(&mut job, ui.ctx(), &rest[..pos], base_size, base_color, pal);
            rest = &rest[pos..];
            continue;
        }

        if let Some((0, rb_idx, rp_idx)) = next_link {
            let label = &rest[1..rb_idx];
            let url = &rest[rb_idx + 2..rp_idx];
            if first_link.is_none() && (url.starts_with("http://") || url.starts_with("https://")) {
                first_link = Some(url.to_string());
            }
            job.append(
                label.trim_matches('*').trim_matches('`'),
                0.0,
                egui::TextFormat {
                    font_id: FontId::proportional(base_size),
                    color: pal.primary_hover,
                    underline: Stroke::new(1.0_f32, pal.primary_hover),
                    ..Default::default()
                },
            );
            rest = &rest[rp_idx + 1..];
            continue;
        }

        if rest.starts_with("**") {
            let after = &rest[2..];
            if let Some(end_b) = after.find("**") {
                job.append(
                    &after[..end_b],
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::proportional(base_size + 0.4),
                        color: if pal.is_dark {
                            Color32::WHITE
                        } else {
                            pal.text_primary
                        },
                        ..Default::default()
                    },
                );
                rest = &after[end_b + 2..];
                continue;
            }
        }

        if rest.starts_with("__") {
            let after = &rest[2..];
            if let Some(end_u) = after.find("__") {
                job.append(
                    &after[..end_u],
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::proportional(base_size),
                        color: base_color,
                        underline: Stroke::new(1.0_f32, base_color),
                        ..Default::default()
                    },
                );
                rest = &after[end_u + 2..];
                continue;
            }
        }

        if rest.starts_with("~~") {
            let after = &rest[2..];
            if let Some(end_s) = after.find("~~") {
                job.append(
                    &after[..end_s],
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::proportional(base_size),
                        color: base_color.linear_multiply(0.75),
                        strikethrough: Stroke::new(1.0_f32, base_color),
                        ..Default::default()
                    },
                );
                rest = &after[end_s + 2..];
                continue;
            }
        }
        if rest.starts_with('*') && !rest.starts_with("**") {
            let after = &rest[1..];
            if let Some(end_i) = after.find('*') {
                if end_i > 0 {
                    job.append(
                        &after[..end_i],
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::proportional(base_size),
                            color: base_color,
                            italics: true,
                            ..Default::default()
                        },
                    );
                    rest = &after[end_i + 1..];
                    continue;
                }
            }
        }

        if rest.starts_with('`') {
            let after = &rest[1..];
            if let Some(end_c) = after.find('`') {
                job.append(
                    &after[..end_c],
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::monospace((base_size - 0.8).max(11.0)),
                        color: pal.primary_hover,
                        background: pal.bg_input,
                        ..Default::default()
                    },
                );
                rest = &after[end_c + 1..];
                continue;
            }
        }

        let mut chars = rest.chars();
        if let Some(ch) = chars.next() {
            let mut buf = [0u8; 4];
            job.append(
                ch.encode_utf8(&mut buf),
                0.0,
                egui::TextFormat {
                    font_id: FontId::proportional(base_size),
                    color: base_color,
                    ..Default::default()
                },
            );
            rest = chars.as_str();
        } else {
            break;
        }
    }

    // Lay the job out here so the colour emoji can be painted into their slots.
    job.wrap.max_width = ui.available_width();
    let galley = ui.fonts(|f| f.layout_job(job));
    if let Some(url) = first_link {
        let resp = ui.add(egui::Label::new(galley.clone()).sense(Sense::click()));
        super::emoji_picker::paint_emoji_overlay(ui.painter(), ui.ctx(), &galley, resp.rect.min);
        if resp
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(&url)
            .clicked()
        {
            open_external_url(&url);
        }
    } else {
        let resp = ui.add(egui::Label::new(galley.clone()));
        super::emoji_picker::paint_emoji_overlay(ui.painter(), ui.ctx(), &galley, resp.rect.min);
    }
}

pub(super) fn render_chat_markdown_block(
    ui: &mut egui::Ui,
    raw: &str,
    text_col: Color32,
    pal: &ThemePalette,
) {
    let mut in_code_block = false;
    let mut code_lines: Vec<&str> = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim_start().starts_with("```") {
            if in_code_block {
                let code_text = code_lines.join("\n");
                code_lines.clear();
                in_code_block = false;
                egui::Frame::none()
                    .fill(pal.bg_input)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .rounding(Rounding::same(10.0))
                    .inner_margin(Margin::symmetric(10.0, 7.0))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(code_text)
                                    .font(FontId::monospace(12.6))
                                    .color(pal.primary_hover),
                            )
                            .wrap(),
                        );
                    });
            } else {
                in_code_block = true;
            }
            continue;
        }
        if in_code_block {
            code_lines.push(line);
            continue;
        }
        let t_start = trimmed.trim_start();
        if let Some(h3) = t_start.strip_prefix("### ") {
            render_markdown_inline_rich(ui, h3, 15.0, text_col, pal);
        } else if let Some(h2) = t_start.strip_prefix("## ") {
            render_markdown_inline_rich(ui, h2, 16.0, text_col, pal);
        } else if let Some(h1) = t_start.strip_prefix("# ") {
            render_markdown_inline_rich(ui, h1, 17.0, text_col, pal);
        } else if let Some(quote) = t_start.strip_prefix("> ") {
            ui.horizontal(|ui| {
                let (bar_r, _) = ui.allocate_exact_size(Vec2::new(3.0, 16.0), Sense::hover());
                ui.painter()
                    .rect_filled(bar_r, Rounding::same(1.5), pal.primary_hover);
                render_markdown_inline_rich(ui, quote, 13.8, text_col.linear_multiply(0.88), pal);
            });
        } else if let Some(bullet) = t_start
            .strip_prefix("- ")
            .or_else(|| t_start.strip_prefix("* "))
        {
            let bullet_line = format!("• {bullet}");
            render_markdown_inline_rich(ui, &bullet_line, 14.0, text_col, pal);
        } else {
            render_markdown_inline_rich(ui, trimmed, 14.0, text_col, pal);
        }
    }
    if !code_lines.is_empty() {
        let code_text = code_lines.join("\n");
        egui::Frame::none()
            .fill(pal.bg_input)
            .stroke(Stroke::new(1.0_f32, pal.border))
            .rounding(Rounding::same(10.0))
            .inner_margin(Margin::symmetric(10.0, 7.0))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(code_text)
                            .font(FontId::monospace(12.6))
                            .color(pal.primary_hover),
                    )
                    .wrap(),
                );
            });
    }
}

pub(super) fn same_message_cluster(a: &ChatMessage, b: &ChatMessage) -> bool {
    if is_system_notice_message(a) || is_system_notice_message(b) {
        return false;
    }
    if a.is_outgoing != b.is_outgoing || a.sender != b.sender {
        return false;
    }
    if should_show_time_divider(Some(a), b) {
        return false;
    }
    let a_ms = normalize_ts_ms(a.ts);
    let b_ms = normalize_ts_ms(b.ts);
    if a_ms > 0 && b_ms > 0 && (b_ms - a_ms).abs() > 10 * 60 * 1000 {
        return false;
    }
    true
}

#[allow(clippy::too_many_arguments)] // UI render entry point; shares state borrowed from the app
pub(super) fn render_message_body(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    thread_name: &str,
    pal: &ThemePalette,
    outgoing: bool,
    max_text_width: f32,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
) -> Option<BubbleAction> {
    let mut body_action = None;
    let (text_col, meta_col) = if outgoing {
        (pal.bubble_out_text, pal.bubble_out_meta)
    } else {
        (pal.bubble_in_text, pal.bubble_in_meta)
    };

    let trimmed_text = msg.text.trim();
    if trimmed_text == "[Tin nhắn đã thu hồi]" {
        ui.label(
            RichText::new(format!("{} Tin nhắn đã được thu hồi", lucide::BAN))
                .color(meta_col)
                .italics()
                .size(13.0),
        );
        return None;
    }

    let sender_label = if msg.is_outgoing {
        "Bạn".to_string()
    } else if !msg.sender.is_empty() && msg.sender != "0" {
        msg.sender.clone()
    } else {
        thread_name.to_string()
    };

    if is_oa_message(msg) {
        return render_oa_card(
            ui,
            msg,
            text_col,
            meta_col,
            sticker_textures,
            loading_stickers,
        );
    }

    // Dedicated inline Photo Card when the message is an uploaded/received Zalo photo (#72)
    let is_zalo_photo = is_zalo_photo_message(msg);
    if is_zalo_photo {
        if let Some(thumb_url) = photo_primary_url(msg) {
            let key = format!("thumb_{}", crate::crypto::md5_hex(thumb_url));
            if !sticker_textures.contains_key(&key) && loading_stickers.insert(key.clone()) {
                if let Some(tx) = WORKER_TX.get() {
                    let _ = tx.send(WorkerCmd::FetchStickerImage {
                        key: key.clone(),
                        url: thumb_url.to_string(),
                    });
                }
            }
            let open_target = msg
                .link_url
                .as_deref()
                .filter(|u| !u.is_empty())
                .unwrap_or(thumb_url)
                .to_string();
            let title_label = msg
                .link_title
                .as_deref()
                .map(str::trim)
                .filter(|t| {
                    !t.is_empty() && !t.starts_with("http://") && !t.starts_with("https://")
                })
                .unwrap_or("Hình ảnh")
                .to_string();
            if let Some(tex) = sticker_textures.get(&key) {
                let [tw, th] = tex.size();
                let tw_f = (tw.max(1)) as f32;
                let th_f = (th.max(1)) as f32;
                let max_w = max_text_width.clamp(220.0, 360.0);
                let max_h = 340.0_f32;
                let scale = (max_w / tw_f).min(max_h / th_f).min(1.0);
                let disp_w = (tw_f * scale).clamp(160.0, max_w);
                let disp_h = (th_f * scale).clamp(120.0, max_h);
                let img = egui::Image::new(tex)
                    .fit_to_exact_size(Vec2::new(disp_w, disp_h))
                    .rounding(Rounding::same(12.0))
                    .sense(Sense::click());
                let resp = ui
                    .add(img)
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("Bấm để xem ảnh trong ứng dụng");
                ui.painter().rect_stroke(
                    resp.rect,
                    Rounding::same(12.0),
                    Stroke::new(
                        if resp.hovered() { 1.6_f32 } else { 1.0_f32 },
                        if resp.hovered() {
                            pal.primary
                        } else {
                            pal.border
                        },
                    ),
                );
                if resp.hovered() {
                    ui.painter().rect_filled(
                        resp.rect,
                        Rounding::same(12.0),
                        Color32::from_rgba_unmultiplied(6, 10, 20, 88),
                    );
                    let pill_r = Rect::from_center_size(resp.rect.center(), Vec2::new(108.0, 26.0));
                    ui.painter().rect_filled(
                        pill_r,
                        Rounding::same(13.0),
                        Color32::from_rgba_unmultiplied(15, 23, 42, 225),
                    );
                    ui.painter().text(
                        pill_r.center(),
                        egui::Align2::CENTER_CENTER,
                        "Phóng to ảnh",
                        FontId::proportional(11.5),
                        Color32::WHITE,
                    );
                }
                if resp.clicked() {
                    body_action = Some(BubbleAction::OpenImagePopup {
                        tex_key: key,
                        full_url: open_target,
                        title: title_label,
                        sender: sender_label,
                        time: msg.time.clone(),
                    });
                }
            } else {
                let ph_frame = egui::Frame::none()
                    .fill(text_col.linear_multiply(0.08))
                    .rounding(Rounding::same(10.0))
                    .inner_margin(Margin::symmetric(12.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (ir, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                            draw_icon_image(ui.painter(), ir.center(), 18.0, text_col);
                            ui.label(
                                RichText::new(&title_label)
                                    .size(13.5)
                                    .color(text_col)
                                    .strong(),
                            );
                        });
                    });
                if ui
                    .interact(
                        ph_frame.response.rect,
                        Id::new(("photo_ph_click", &msg.id, msg.ts)),
                        Sense::click(),
                    )
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("Bấm để xem hình ảnh")
                    .clicked()
                {
                    body_action = Some(BubbleAction::OpenImagePopup {
                        tex_key: key,
                        full_url: open_target,
                        title: title_label,
                        sender: sender_label,
                        time: msg.time.clone(),
                    });
                }
            }
            if let Some(cap) = extract_photo_user_caption(msg) {
                ui.add_space(5.0);
                render_chat_markdown_block(ui, cap, text_col, pal);
            }
            return body_action;
        }
    }
    // Dedicated File / Video Attachment Card (#74)
    let is_zalo_file_or_video = is_zalo_file_or_video_message(msg);
    if is_zalo_file_or_video {
        let is_vid =
            msg.link_src.as_deref() == Some("Video Zalo") || trimmed_text.starts_with("[Video]");
        let url_str = msg.link_url.as_deref().unwrap_or_default();
        let resolved_file_name = crate::chat::resolve_attachment_file_name(
            msg.link_title.as_deref(),
            trimmed_text,
            url_str,
        );
        let title_str = if is_vid {
            msg.link_title
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(trimmed_text)
                .to_string()
        } else {
            resolved_file_name.clone()
        };
        let link_col = if pal.is_dark {
            Color32::from_rgb(97, 175, 239)
        } else {
            pal.primary
        };
        let url = msg
            .link_url
            .as_deref()
            .filter(|u| !u.is_empty())
            .map(str::to_string);
        // Zalo-style: badge, name, then size + "Đã có trên máy" and folder / save buttons.
        let saved = url.as_deref().and_then(crate::gui::downloaded_path);
        let size = super::conv_info::size_from_title(msg.link_title.as_deref());
        let opens_externally = is_vid
            && url
                .as_deref()
                .is_some_and(|u| !u.to_ascii_lowercase().contains("/renewmedia"));
        let mut act: Option<BubbleAction> = None;
        let download = |url: &str| BubbleAction::DownloadFile {
            url: url.to_string(),
            file_name: resolved_file_name.clone(),
        };
        // Document preview above the name, like Zalo: Quick Look renders the first page.
        let thumb_key = url
            .as_deref()
            .filter(|_| !is_vid && crate::file_preview::previewable(&resolved_file_name))
            .map(crate::file_preview::thumb_key);
        if let (Some(k), Some(u)) = (thumb_key.as_ref(), url.as_deref()) {
            let near = Rect::from_min_size(
                ui.cursor().min,
                Vec2::new(ui.available_width().max(1.0), 160.0),
            );
            if !sticker_textures.contains_key(k)
                && ui.is_rect_visible(near)
                && loading_stickers.insert(k.clone())
            {
                if let Some(tx) = WORKER_TX.get() {
                    let _ = tx.send(WorkerCmd::FetchFileThumbnail {
                        key: k.clone(),
                        url: u.to_string(),
                        file_name: resolved_file_name.clone(),
                        local_path: saved.clone(),
                    });
                }
            }
        }
        if let Some(tex) = thumb_key.as_ref().and_then(|k| sticker_textures.get(k)) {
            let w = ui.available_width().max(120.0);
            let [tw, th] = tex.size();
            let img_aspect = th.max(1) as f32 / tw.max(1) as f32;
            let box_h = (w * 0.62).clamp(110.0, 190.0);
            // Tall pages are cropped to their top; wide slides are shown whole.
            let (h, uv_bottom) = if w * img_aspect >= box_h {
                (box_h, box_h / (w * img_aspect))
            } else {
                ((w * img_aspect).max(80.0), 1.0)
            };
            let (r, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
            let round = Rounding::same(10.0);
            ui.painter()
                .rect_filled(r, round, text_col.linear_multiply(0.08));
            let mut shape = egui::epaint::RectShape::filled(r, round, Color32::WHITE);
            shape.fill_texture_id = tex.id();
            shape.uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, uv_bottom));
            ui.painter().add(egui::Shape::Rect(shape));
            ui.painter().rect_stroke(
                r,
                round,
                Stroke::new(1.0_f32, text_col.linear_multiply(0.12)),
            );
            let resp = resp
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(if saved.is_some() {
                    "Bấm để mở tệp"
                } else {
                    "Bấm để tải và lưu tập tin với tên gốc"
                });
            if resp.clicked() {
                if let Some(path) = saved.clone() {
                    act = Some(BubbleAction::OpenFile(path));
                } else if let Some(u) = url.as_deref() {
                    act = Some(download(u));
                }
            }
            ui.add_space(8.0);
        }
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (badge, badge_resp) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::click());
            super::conv_info::paint_file_badge(ui.painter(), badge, &title_str, is_vid);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                let name_resp = ui
                    .add(
                        egui::Label::new(
                            RichText::new(&title_str)
                                .size(13.0)
                                .color(text_col)
                                .strong(),
                        )
                        .wrap()
                        .sense(Sense::click()),
                    )
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text(if saved.is_some() {
                        "Bấm để mở tệp"
                    } else if opens_externally {
                        "Bấm để mở video"
                    } else {
                        "Bấm để tải và lưu tập tin với tên gốc"
                    });
                if name_resp.clicked() || badge_resp.clicked() {
                    if let Some(path) = saved.clone() {
                        act = Some(BubbleAction::OpenFile(path));
                    } else if let Some(u) = url.as_deref() {
                        if opens_externally {
                            open_external_url(u);
                        } else {
                            act = Some(download(u));
                        }
                    }
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if !size.is_empty() {
                        ui.label(RichText::new(&size).size(11.5).color(meta_col));
                    }
                    if saved.is_some() {
                        ui.label(
                            RichText::new(format!("{} Đã có trên máy", lucide::CHECK))
                                .size(11.5)
                                .color(pal.accent_online),
                        );
                    } else if thread_name.contains("Cloud của tôi") {
                        // Like Zalo's My Documents: the file lives in the account's cloud.
                        ui.label(
                            RichText::new(format!("{} Đã có trên Cloud", lucide::CLOUD))
                                .size(11.5)
                                .color(meta_col),
                        );
                    } else {
                        ui.label(
                            RichText::new(if is_vid {
                                "Video đính kèm"
                            } else {
                                "Tập tin Zalo"
                            })
                            .size(11.5)
                            .color(link_col),
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let icon_btn = |ui: &mut egui::Ui, icon: &str, tip: &str| {
                            let (r, resp) =
                                ui.allocate_exact_size(Vec2::new(28.0, 24.0), Sense::click());
                            let hot = resp.hovered();
                            ui.painter().rect_stroke(
                                r,
                                Rounding::same(6.0),
                                Stroke::new(1.0_f32, if hot { link_col } else { meta_col }),
                            );
                            draw_lucide_icon(
                                ui.painter(),
                                r.center(),
                                icon,
                                14.0,
                                if hot { link_col } else { text_col },
                            );
                            resp.on_hover_text(tip)
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .clicked()
                        };
                        if let Some(u) = url.as_deref() {
                            if icon_btn(
                                ui,
                                lucide::DOWNLOAD,
                                if saved.is_some() {
                                    "Lưu lại về máy"
                                } else {
                                    "Lưu về máy"
                                },
                            ) {
                                act = Some(download(u));
                            }
                        }
                        if let Some(path) = saved.clone() {
                            if icon_btn(ui, lucide::FOLDER_OPEN, "Hiển thị trong thư mục") {
                                act = Some(BubbleAction::RevealFile(path));
                            }
                        }
                    });
                });
            });
        });
        if act.is_some() {
            return act;
        }
        return None;
    }

    let Some((caption, url, domain)) = extract_message_link_view(msg) else {
        render_chat_markdown_block(ui, &msg.text, text_col, pal);
        return None;
    };

    let link_col = if pal.is_dark {
        Color32::from_rgb(97, 175, 239) // #61AFEF Zalo/Atom clean link blue
    } else {
        pal.primary
    };

    // Auto-enrich link metadata if title or thumbnail is missing/expired
    let title_is_url = msg
        .link_title
        .as_deref()
        .is_some_and(|t| t.trim().starts_with("http://") || t.trim().starts_with("https://"));
    let needs_meta_enrich = msg.link_title.is_none()
        || title_is_url
        || msg.link_thumb.is_none()
        || msg
            .link_thumb
            .as_deref()
            .map(|th| {
                th.contains("zpcloud.zdn.vn")
                    && !sticker_textures
                        .contains_key(&format!("thumb_{}", crate::crypto::md5_hex(th)))
            })
            .unwrap_or(false);
    if needs_meta_enrich {
        let meta_key = format!("linkmeta_{}", crate::crypto::md5_hex(&url));
        if loading_stickers.insert(meta_key) {
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::ParseLinkPreview { url: url.clone() });
            }
        }
    }

    // 1. Optional user comment above the link
    if !caption.is_empty() && caption != url {
        render_chat_markdown_block(ui, &caption, text_col, pal);
        ui.add_space(4.0);
    }

    // 1b. The raw URL as a blue link, like Zalo PC shows it above the preview.
    let url_resp = ui
        .add(
            egui::Label::new(
                RichText::new(truncate_chars(&url, 140))
                    .size(13.0)
                    .color(link_col)
                    .underline(),
            )
            .wrap()
            .sense(Sense::click()),
        )
        .on_hover_cursor(CursorIcon::PointingHand);
    if url_resp.clicked() {
        open_external_url(&url);
    }
    ui.add_space(5.0);

    // 2. Optional compact thumbnail directly inside the bubble (no nested card box)
    if let Some(thumb_url) = msg.link_thumb.as_deref().filter(|u| !u.is_empty()) {
        let key = format!("thumb_{}", crate::crypto::md5_hex(thumb_url));
        if !sticker_textures.contains_key(&key)
            && (thumb_url.starts_with("http://") || thumb_url.starts_with("https://"))
            && loading_stickers.insert(key.clone())
        {
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::FetchStickerImage {
                    key: key.clone(),
                    url: thumb_url.to_string(),
                });
            }
        }
        if let Some(tex) = sticker_textures.get(&key) {
            let [tw, th] = tex.size();
            let aspect = if tw > 0 && th > 0 {
                (th as f32 / tw as f32).clamp(0.45, 0.85)
            } else {
                0.56
            };
            let thumb_w = max_text_width.min(260.0);
            let img_h = (thumb_w * aspect).min(132.0);
            let img = egui::Image::new(tex)
                .fit_to_exact_size(Vec2::new(thumb_w, img_h))
                .rounding(Rounding::same(10.0))
                .sense(Sense::click());
            if ui
                .add(img)
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(&url)
                .clicked()
            {
                open_external_url(&url);
            }
            ui.add_space(5.0);
        }
    }

    // 3. Bold Title (exact Zalo PC style: no globe icon, no vertical accent bar, no nested border)
    let header_title = msg
        .link_title
        .as_deref()
        .map(str::trim)
        .filter(|t| {
            !t.is_empty() && *t != domain && !t.starts_with("http://") && !t.starts_with("https://")
        })
        .unwrap_or("");
    if !header_title.is_empty() {
        let title_resp = ui
            .add(
                egui::Label::new(
                    RichText::new(truncate_chars(header_title, 95))
                        .size(13.0)
                        .color(text_col)
                        .strong(),
                )
                .wrap()
                .sense(Sense::click()),
            )
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(&url);
        if title_resp.clicked() {
            open_external_url(&url);
        }
    }

    // 4. Optional short description (only if concise and distinct)
    let desc_opt = msg
        .link_desc
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty() && *d != header_title && *d != url && caption.is_empty());
    if let Some(desc) = desc_opt {
        ui.add_space(2.0);
        let d_resp = ui
            .add(
                egui::Label::new(
                    RichText::new(truncate_chars(desc, 110))
                        .size(12.3)
                        .color(meta_col),
                )
                .wrap()
                .sense(Sense::click()),
            )
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(&url);
        if d_resp.clicked() {
            open_external_url(&url);
        }
    }

    // 5. Clean Domain line (`docs.google.com`, `drive.google.com`, `tiktok.com`)
    ui.add_space(2.5);
    let dom_resp = ui
        .add(
            egui::Label::new(RichText::new(&domain).size(12.6).color(link_col))
                .sense(Sense::click()),
        )
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(&url);
    if dom_resp.clicked() {
        open_external_url(&url);
    }

    body_action
}

/// Official Account broadcast (`chat.list.action`), drawn like Zalo's article card.
pub(super) fn is_oa_message(msg: &ChatMessage) -> bool {
    msg.link_src.as_deref() == Some(crate::chat::OA_LINK_SRC)
}

/// Start loading `url` into `sticker_textures` under `thumb_<md5>`; returns the key.
fn request_remote_image(
    url: &str,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading: &mut HashSet<String>,
) -> String {
    let key = format!("thumb_{}", crate::crypto::md5_hex(url));
    if !sticker_textures.contains_key(&key) && loading.insert(key.clone()) {
        if let Some(tx) = WORKER_TX.get() {
            let _ = tx.send(WorkerCmd::FetchStickerImage {
                key: key.clone(),
                url: url.to_string(),
            });
        }
    }
    key
}

/// Paint `tex` into `rect`, cropped to fill it (no letterboxing).
fn paint_cover(painter: &egui::Painter, rect: Rect, tex: &TextureHandle, round: Rounding) {
    let [tw, th] = tex.size();
    let (tw, th) = (tw.max(1) as f32, th.max(1) as f32);
    let scale = (rect.width() / tw).max(rect.height() / th);
    let (vis_w, vis_h) = (rect.width() / (tw * scale), rect.height() / (th * scale));
    let uv = Rect::from_center_size(Pos2::new(0.5, 0.5), Vec2::new(vis_w, vis_h));
    let mut shape = egui::epaint::RectShape::filled(rect, round, Color32::WHITE);
    shape.fill_texture_id = tex.id();
    shape.uv = uv;
    painter.add(egui::Shape::Rect(shape));
}

/// Banner, title, description, then one row per further article (thumbnail + title).
fn render_oa_card(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    text_col: Color32,
    meta_col: Color32,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
) -> Option<BubbleAction> {
    let w = ui.available_width().max(160.0);
    let placeholder = text_col.linear_multiply(0.08);
    let main_href = msg.link_url.clone().unwrap_or_default();
    let mut open: Option<String> = None;
    let link_hint = |resp: egui::Response, href: &str| {
        if href.is_empty() {
            resp
        } else {
            resp.on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text("Bấm để mở bài viết")
        }
    };

    if let Some(thumb) = msg.link_thumb.as_deref().filter(|u| !u.is_empty()) {
        let key = request_remote_image(thumb, sticker_textures, loading_stickers);
        let tex = sticker_textures.get(&key);
        let h = tex
            .map(|t| {
                let [tw, th] = t.size();
                w * th.max(1) as f32 / tw.max(1) as f32
            })
            .unwrap_or(w * 0.56)
            .clamp(110.0, 280.0);
        let (r, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
        let round = Rounding::same(10.0);
        match tex {
            Some(t) => paint_cover(ui.painter(), r, t, round),
            None => {
                ui.painter().rect_filled(r, round, placeholder);
            }
        }
        if link_hint(resp, &main_href).clicked() {
            open = Some(main_href.clone());
        }
        ui.add_space(8.0);
    }
    if let Some(title) = msg.link_title.as_deref().filter(|t| !t.is_empty()) {
        let resp = ui.add(
            egui::Label::new(RichText::new(title).size(14.5).strong().color(text_col))
                .wrap()
                .sense(Sense::click()),
        );
        if link_hint(resp, &main_href).clicked() {
            open = Some(main_href.clone());
        }
    }
    if let Some(desc) = msg.link_desc.as_deref().filter(|d| !d.is_empty()) {
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(
                RichText::new(desc)
                    .size(13.0)
                    .color(text_col.linear_multiply(0.85)),
            )
            .wrap(),
        );
    }
    for item in &msg.oa_items {
        ui.add_space(6.0);
        let (sep, _) = ui.allocate_exact_size(Vec2::new(w, 1.0), Sense::hover());
        ui.painter()
            .rect_filled(sep, 0.0, meta_col.linear_multiply(0.35));
        ui.add_space(6.0);
        let row = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (r, _) = ui.allocate_exact_size(Vec2::splat(46.0), Sense::hover());
            let round = Rounding::same(6.0);
            match (!item.thumb.is_empty())
                .then(|| request_remote_image(&item.thumb, sticker_textures, loading_stickers))
                .and_then(|k| sticker_textures.get(&k))
            {
                Some(t) => paint_cover(ui.painter(), r, t, round),
                None => {
                    ui.painter().rect_filled(r, round, placeholder);
                }
            }
            ui.add(egui::Label::new(RichText::new(&item.title).size(13.0).color(text_col)).wrap());
        });
        let resp = ui.interact(
            row.response.rect,
            Id::new(("oa_item", &msg.id, &item.href, &item.title)),
            Sense::click(),
        );
        if link_hint(resp, &item.href).clicked() {
            open = Some(item.href.clone());
        }
    }
    if let Some(url) = open.filter(|u| !u.is_empty()) {
        open_external_url(&url);
    }
    None
}

pub(super) fn is_zalo_file_or_video_message(msg: &ChatMessage) -> bool {
    let trimmed = msg.text.trim();
    matches!(
        msg.link_src.as_deref(),
        Some("Tập tin Zalo") | Some("Video Zalo")
    ) || trimmed.starts_with("[Tập tin]")
        || trimmed.starts_with("[Video]")
        || trimmed.starts_with("[Tin nhắn thoại]")
        || msg
            .link_url
            .as_deref()
            .is_some_and(|u| u.to_ascii_lowercase().contains("/renewmedia"))
}

pub(super) fn photo_primary_url(msg: &ChatMessage) -> Option<&str> {
    msg.link_thumb
        .as_deref()
        .filter(|u| !u.is_empty())
        .or_else(|| msg.link_url.as_deref().filter(|u| !u.is_empty()))
        .or_else(|| {
            let t = msg.text.trim();
            crate::chat::is_zalo_photo_cdn_url(t).then_some(t)
        })
}

pub(super) fn is_zalo_photo_message(msg: &ChatMessage) -> bool {
    if is_oa_message(msg) || is_sticker_message(msg).is_some() || is_zalo_file_or_video_message(msg)
    {
        return false;
    }
    // A shared web link (e.g. Google Docs) whose preview thumbnail Zalo re-hosts on its photo
    // CDN is a link card, not a photo: only its thumbnail lives on the CDN.
    // Older builds stored such links with `link_src = "Hình ảnh Zalo"` / "[Hình ảnh]", so the
    // URL decides, not the stored label.
    let text_has_web_url = msg
        .text
        .split_whitespace()
        .any(|w| crate::chat::is_shared_web_link(w.trim()));
    if msg
        .link_url
        .as_deref()
        .is_some_and(crate::chat::is_shared_web_link)
        || text_has_web_url
    {
        return false;
    }
    let trimmed = msg.text.trim();
    msg.link_src.as_deref() == Some("Hình ảnh Zalo")
        || trimmed == "[Hình ảnh]"
        || trimmed.starts_with("[Hình ảnh] ")
        || msg
            .link_url
            .as_deref()
            .is_some_and(crate::chat::is_zalo_photo_cdn_url)
        || msg
            .link_thumb
            .as_deref()
            .is_some_and(crate::chat::is_zalo_photo_cdn_url)
        || crate::chat::is_zalo_photo_cdn_url(trimmed)
}

pub(super) fn extract_photo_user_caption(msg: &ChatMessage) -> Option<&str> {
    let is_valid_caption = |s: &str| -> bool {
        let t = s.trim();
        !t.is_empty()
            && t != "[Hình ảnh]"
            && !t.starts_with("http://")
            && !t.starts_with("https://")
            && Some(t) != msg.link_src.as_deref()
    };
    if let Some(desc) = msg.link_desc.as_deref().map(str::trim) {
        if is_valid_caption(desc) {
            return Some(desc);
        }
    }
    let trimmed = msg.text.trim();
    let stripped = trimmed
        .strip_prefix("[Hình ảnh]")
        .map(str::trim)
        .unwrap_or(trimmed);
    if is_valid_caption(stripped) {
        Some(stripped)
    } else {
        None
    }
}

/// Lay out ordinary text with metadata tucked into the last line, as in Telegram.
fn render_compact_text(
    ui: &mut egui::Ui,
    text: &str,
    metadata: &str,
    width: f32,
    color: Color32,
    meta_color: Color32,
) {
    let mut job = egui::text::LayoutJob::default();
    super::emoji_picker::append_text_with_emoji(
        &mut job,
        ui.ctx(),
        text,
        FontId::proportional(14.0),
        color,
    );
    job.wrap.max_width = width;
    let galley = ui.fonts(|f| f.layout_job(job));
    let meta =
        ui.fonts(|f| f.layout_no_wrap(metadata.to_owned(), FontId::proportional(9.5), meta_color));
    let end = galley.pos_from_ccursor(egui::text::CCursor::new(text.chars().count()));
    let fits = metadata.is_empty() || end.min.x + 7.0 + meta.size().x <= width;
    let height = galley.size().y + if fits { 0.0 } else { 11.0 };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    ui.painter().galley(rect.min, galley.clone(), color);
    super::emoji_picker::paint_emoji_overlay(ui.painter(), ui.ctx(), &galley, rect.min);
    if !metadata.is_empty() {
        ui.painter().galley(
            Pos2::new(rect.right() - meta.size().x, rect.bottom() - meta.size().y),
            meta,
            meta_color,
        );
    }
}

#[allow(clippy::too_many_arguments)] // UI render entry point; shares state borrowed from the app
pub(super) fn render_chat_bubble(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    prev_same_sender: bool,
    next_same_sender: bool,
    is_latest_outgoing: bool,
    is_group: bool,
    thread_name: &str,
    sender_avatar: Option<&str>,
    pal: &ThemePalette,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
    emoji_textures: &HashMap<String, TextureHandle>,
    idx: usize,
) -> Option<BubbleAction> {
    let avail_w = ui.available_width();
    let max_bubble_width = (avail_w * 0.72).clamp(140.0, 520.0);
    let max_text_width = (max_bubble_width - 24.0).max(90.0);
    let has_sticker = is_sticker_message(msg).is_some();
    let call_card_opt = extract_call_card_view(msg);
    let is_call_card = call_card_opt.is_some();
    let is_photo_card = !has_sticker && is_zalo_photo_message(msg);
    let is_file_card = !has_sticker && is_zalo_file_or_video_message(msg);
    let is_oa_card = is_oa_message(msg);
    let has_link_card = !has_sticker
        && !is_oa_card
        && !is_photo_card
        && !is_file_card
        && extract_message_link_view(msg).is_some();
    let has_interactive_card =
        is_call_card || has_link_card || is_photo_card || is_file_card || is_oa_card;

    let compact_text = !has_sticker
        && !has_interactive_card
        && msg.reply_to_text.is_none()
        && msg.reactions.is_empty()
        && !msg.is_pinned
        && !msg.is_starred
        && !msg.text.contains(['`', '*', '[', ']'])
        && !msg.text.contains("http")
        && !msg.text.trim().is_empty();
    let show_timestamp = !msg.time.is_empty();
    let metadata = if !show_timestamp {
        String::new()
    } else if msg.is_outgoing && is_latest_outgoing {
        format!("{} · Đã nhận", msg.time)
    } else {
        msg.time.clone()
    };

    let photo_caption = if is_photo_card {
        extract_photo_user_caption(msg)
    } else {
        None
    };
    let has_photo_caption = photo_caption.is_some();
    let is_frameless_photo = is_photo_card
        && !has_photo_caption
        && msg.reply_to_text.is_none()
        && (msg.is_outgoing || !is_group);

    // Zalo shows the alias (tên gợi nhớ) you set for a friend in place of their group name.
    let sender_alias = if is_group && !msg.is_outgoing {
        crate::chat::sidebar_sender_id(msg).and_then(|uid| crate::gui::alias_for(&uid))
    } else {
        None
    };
    let sender_label: &str = sender_alias.as_deref().unwrap_or(&msg.sender);
    let sender_is_name = !sender_label.is_empty()
        && sender_label != "0"
        && !sender_label.chars().all(|c| c.is_ascii_digit());
    let show_group_sender = !msg.is_outgoing && !prev_same_sender && is_group && sender_is_name;

    // Pre-measure exact content width for both outgoing and incoming bubbles so every bubble,
    // photo, and call card hugs its content tightly without horizontal overflow or inner/outer frame mismatch.
    let content_width = if has_sticker {
        116.0_f32
    } else if let Some(card) = call_card_opt.as_ref() {
        call_card_width(ui, card, max_text_width)
    } else if is_photo_card {
        let thumb_u = photo_primary_url(msg).unwrap_or("");
        let tkey = format!("thumb_{}", crate::crypto::md5_hex(thumb_u));
        let photo_w = if let Some(tex) = sticker_textures.get(&tkey) {
            let [tw, th] = tex.size();
            let tw_f = (tw.max(1)) as f32;
            let th_f = (th.max(1)) as f32;
            let max_w = max_text_width.clamp(220.0, 360.0);
            let max_h = 340.0_f32;
            let scale = (max_w / tw_f).min(max_h / th_f).min(1.0);
            (tw_f * scale).clamp(160.0, max_w).ceil()
        } else {
            240.0_f32
        };
        if let Some(cap_str) = photo_caption {
            let cap_w = ui.fonts(|f| {
                f.layout(
                    cap_str.to_string(),
                    FontId::proportional(13.0),
                    Color32::WHITE,
                    max_text_width,
                )
                .size()
                .x
            });
            photo_w.max(cap_w + 4.0).min(max_text_width).ceil()
        } else {
            photo_w
        }
    } else if is_oa_card {
        max_text_width.min(340.0).ceil()
    } else if is_file_card {
        // Badge (44) + gap + name, and room for size/status + the two buttons underneath.
        let name = crate::chat::resolve_attachment_file_name(
            msg.link_title.as_deref(),
            msg.text.trim(),
            msg.link_url.as_deref().unwrap_or_default(),
        );
        let name_w = ui.fonts(|f| {
            f.layout(
                truncate_chars(&name, 90).into_owned(),
                FontId::proportional(13.0),
                Color32::WHITE,
                max_text_width.min(340.0) - 54.0,
            )
            .size()
            .x
        });
        // A narrow chat (small window, info panel open) makes the upper bound smaller than 250;
        // `f32::clamp` panics on an inverted range and aborted the app inside AppKit's draw.
        let has_preview = msg
            .link_url
            .as_deref()
            .filter(|u| !u.is_empty())
            .is_some_and(|u| sticker_textures.contains_key(&crate::file_preview::thumb_key(u)));
        let wanted = if has_preview {
            330.0
        } else {
            name_w.max(190.0) + 54.0 + 8.0
        };
        fit_between(wanted, 250.0, max_text_width.min(360.0)).ceil()
    } else if has_link_card {
        let has_loaded_thumb = msg
            .link_thumb
            .as_deref()
            .filter(|u| !u.is_empty())
            .map(|u| sticker_textures.contains_key(&format!("thumb_{}", crate::crypto::md5_hex(u))))
            .unwrap_or(false);
        if has_loaded_thumb {
            max_text_width.min(260.0)
        } else {
            let (cap, url, dom) = extract_message_link_view(msg).unwrap_or_default();
            let title = msg
                .link_title
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    if !url.is_empty() {
                        &url
                    } else {
                        msg.text.trim()
                    }
                });
            let desc = msg.link_desc.as_deref().unwrap_or("");
            let measure = |s: &str, sz: f32| -> f32 {
                if s.is_empty() {
                    0.0
                } else {
                    ui.fonts(|f| {
                        f.layout(
                            truncate_chars(s, 110).into_owned(),
                            FontId::proportional(sz),
                            Color32::WHITE,
                            max_text_width.min(340.0),
                        )
                        .size()
                        .x
                    })
                }
            };
            let w = measure(title, 13.8)
                .max(measure(&cap, 13.8))
                .max(measure(desc, 12.3))
                .max(measure(&dom, 12.6))
                + 8.0;
            fit_between(w, 135.0, max_text_width.min(340.0)).ceil()
        }
    } else {
        let display_str = if msg.text.trim() == "[Tin nhắn đã thu hồi]" {
            "  Tin nhắn đã được thu hồi".to_string()
        } else {
            strip_markdown_syntax_for_width(&msg.text)
        };
        // Must match the size the body is drawn with (`render_markdown_inline_rich(.., 14.0, ..)`);
        // measuring at 13 made bubbles narrower than their text, so the last word wrapped.
        let text_w = {
            let mut job = egui::text::LayoutJob::default();
            super::emoji_picker::append_text_with_emoji(
                &mut job,
                ui.ctx(),
                &display_str,
                FontId::proportional(14.0),
                Color32::WHITE,
            );
            job.wrap.max_width = max_text_width;
            ui.fonts(|f| f.layout_job(job)).size().x
        };
        let quote_w = match msg.reply_to_text.as_ref() {
            Some(qt) => {
                let q_body_w = ui.fonts(|f| {
                    f.layout(
                        truncate_chars(qt, 80).into_owned(),
                        FontId::proportional(12.0),
                        Color32::WHITE,
                        (max_text_width - 28.0).max(80.0),
                    )
                    .size()
                    .x
                });
                let who = msg
                    .reply_to_sender
                    .as_deref()
                    .filter(|sn| !sn.is_empty() && !sn.chars().all(|c| c.is_ascii_digit()))
                    .unwrap_or("Tin nhắn trích dẫn");
                let q_who_w = ui.fonts(|f| {
                    f.layout_no_wrap(who.to_string(), FontId::proportional(11.5), Color32::WHITE)
                        .size()
                        .x
                });
                q_body_w.max(q_who_w) + 30.0
            }
            None => 0.0_f32,
        };
        let sender_w = if show_group_sender {
            ui.fonts(|f| {
                f.layout_no_wrap(
                    truncate_chars(sender_label, 22).into_owned(),
                    FontId::proportional(12.0),
                    Color32::WHITE,
                )
                .size()
                .x
            }) + 8.0
        } else {
            0.0_f32
        };
        let marks_w = if !msg.reactions.is_empty() || msg.is_pinned || msg.is_starred {
            78.0_f32
        } else {
            0.0_f32
        };
        let ts_min_w = if show_timestamp && is_latest_outgoing && msg.is_outgoing {
            96.0_f32
        } else {
            44.0_f32
        };
        let text_w = if compact_text && show_timestamp {
            let meta_w = ui
                .fonts(|f| {
                    f.layout_no_wrap(metadata.clone(), FontId::proportional(9.5), Color32::WHITE)
                })
                .size()
                .x;
            (text_w + meta_w + 8.0).min(max_text_width)
        } else {
            text_w
        };
        text_w
            .max(quote_w)
            .max(sender_w)
            .max(marks_w)
            .max(ts_min_w)
            .min(max_text_width)
            .ceil()
            + 4.0
    };

    let h_margin = if has_sticker || is_frameless_photo {
        2.0_f32
    } else {
        10.0_f32
    };
    let v_margin = if has_sticker || is_frameless_photo {
        2.0_f32
    } else {
        5.0_f32
    };
    let total_bubble_w = content_width + h_margin * 2.0;

    if msg.is_outgoing {
        let out_resp = ui.vertical(|ui| {
            let row_resp = ui.horizontal(|ui| {
                let left_pad = (ui.available_width() - total_bubble_w - 6.0).max(0.0);
                if left_pad > 0.0 {
                    ui.add_space(left_pad);
                }

                let fill = if has_sticker || is_frameless_photo {
                    Color32::TRANSPARENT
                } else {
                    // Call cards use the normal bubble colour (system palette), no tint.
                    pal.bubble_out_bg
                };
                let stroke = if has_sticker || is_frameless_photo {
                    Stroke::NONE
                } else {
                    // Telegram-like: no outline on bubbles, call cards included.
                    Stroke::NONE
                };
                // 16 px corners with a small "tail" corner on the sender's side.
                let rounding = Rounding {
                    nw: 12.0,
                    ne: 12.0,
                    sw: 12.0,
                    se: if next_same_sender { 10.0 } else { 4.0 },
                };

                let mut card_action = None;
                let mut quote_action = None;
                let fr = egui::Frame::none()
                    .fill(fill)
                    .stroke(stroke)
                    .rounding(rounding)
                    .inner_margin(Margin::symmetric(h_margin, v_margin))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.set_min_width(content_width);
                        ui.set_max_width(content_width);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            quote_action = render_quote_block(ui, msg, pal, true, content_width);
                            if compact_text {
                                render_compact_text(
                                    ui,
                                    msg.text.trim(),
                                    &metadata,
                                    content_width,
                                    if msg.is_outgoing {
                                        pal.bubble_out_text
                                    } else {
                                        pal.bubble_in_text
                                    },
                                    if msg.is_outgoing {
                                        pal.bubble_out_meta
                                    } else {
                                        pal.bubble_in_meta
                                    },
                                );
                            } else if let Some(card) = &call_card_opt {
                                card_action =
                                    render_call_card_widget(ui, card, pal, content_width, true);
                            } else if !render_sticker_widget(
                                ui,
                                msg,
                                sticker_textures,
                                loading_stickers,
                            ) {
                                card_action = render_message_body(
                                    ui,
                                    msg,
                                    thread_name,
                                    pal,
                                    true,
                                    content_width,
                                    sticker_textures,
                                    loading_stickers,
                                );
                            }
                            render_reactions(ui, msg, pal, emoji_textures);
                            if show_timestamp
                                && !compact_text
                                && !has_sticker
                                && !is_frameless_photo
                            {
                                ui.add_space(3.5);
                                let ts_txt = if is_latest_outgoing {
                                    format!("{} · Đã nhận", msg.time)
                                } else {
                                    msg.time.clone()
                                };
                                ui.label(
                                    RichText::new(ts_txt).size(11.0).color(pal.bubble_out_meta),
                                );
                            }
                        });
                    });
                if fr.response.double_clicked() {
                    return Some(BubbleAction::Reply);
                }
                reaction_corner_chip(ui, fr.response.rect, msg, pal);
                let quick =
                    quick_react_overlay(ui, &fr.response, msg, idx, true, pal, emoji_textures);
                quote_action.or(card_action).or(quick).or_else(|| {
                    bubble_context_menu(
                        ui,
                        &fr.response,
                        msg,
                        idx,
                        pal,
                        emoji_textures,
                        has_interactive_card,
                    )
                })
            });

            row_resp.inner
        });
        out_resp.inner
    } else {
        ui.horizontal_top(|ui| {
            let display_sender = if sender_is_name {
                sender_label
            } else {
                thread_name
            };
            let sender_accent = avatar_color_for(display_sender, false);

            // Sender Avatar (only on first message of a consecutive group); clicking it opens
            // the sender's account card.
            let mut avatar_action = None;
            if !prev_same_sender {
                let (av_rect, av_resp) =
                    ui.allocate_exact_size(Vec2::new(30.0, 30.0), Sense::click());
                if av_resp
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("Xem thông tin tài khoản")
                    .clicked()
                {
                    avatar_action = Some(BubbleAction::OpenSenderProfile);
                }
                paint_user_avatar(
                    ui.painter(),
                    av_rect.center(),
                    15.0,
                    display_sender,
                    false,
                    sender_avatar,
                    sticker_textures,
                    loading_stickers,
                );
            } else {
                // Same allocation as the avatar so the item spacing after it matches and every
                // bubble of the run lines up (`add_space` skipped that spacing).
                ui.allocate_exact_size(Vec2::new(30.0, 1.0), Sense::hover());
            }

            let fill = if has_sticker || is_frameless_photo {
                Color32::TRANSPARENT
            } else {
                pal.bubble_in_bg
            };
            let stroke = Stroke::NONE;
            let rounding = Rounding {
                nw: 12.0,
                ne: 12.0,
                sw: if next_same_sender { 10.0 } else { 4.0 },
                se: 12.0,
            };

            ui.vertical(|ui| {
                ui.set_max_width(total_bubble_w);
                let mut card_action = None;
                let mut quote_action = None;
                let fr = egui::Frame::none()
                    .fill(fill)
                    .stroke(stroke)
                    .rounding(rounding)
                    .inner_margin(Margin::symmetric(h_margin, v_margin))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.set_min_width(content_width);
                        ui.set_max_width(content_width);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            if show_group_sender {
                                ui.label(
                                    RichText::new(truncate_chars(sender_label, 22))
                                        .color(sender_accent)
                                        .size(12.0)
                                        .strong(),
                                );
                                ui.add_space(2.0);
                            }
                            quote_action = render_quote_block(ui, msg, pal, false, content_width);
                            if compact_text {
                                render_compact_text(
                                    ui,
                                    msg.text.trim(),
                                    &metadata,
                                    content_width,
                                    if msg.is_outgoing {
                                        pal.bubble_out_text
                                    } else {
                                        pal.bubble_in_text
                                    },
                                    if msg.is_outgoing {
                                        pal.bubble_out_meta
                                    } else {
                                        pal.bubble_in_meta
                                    },
                                );
                            } else if let Some(card) = &call_card_opt {
                                card_action =
                                    render_call_card_widget(ui, card, pal, content_width, false);
                            } else if !render_sticker_widget(
                                ui,
                                msg,
                                sticker_textures,
                                loading_stickers,
                            ) {
                                card_action = render_message_body(
                                    ui,
                                    msg,
                                    thread_name,
                                    pal,
                                    false,
                                    content_width,
                                    sticker_textures,
                                    loading_stickers,
                                );
                            }
                            render_reactions(ui, msg, pal, emoji_textures);
                            if show_timestamp
                                && !compact_text
                                && !has_sticker
                                && !is_frameless_photo
                            {
                                ui.add_space(3.5);
                                ui.label(
                                    RichText::new(&msg.time)
                                        .color(pal.bubble_in_meta)
                                        .size(11.0),
                                );
                            }
                        });
                    });
                if fr.response.double_clicked() {
                    return Some(BubbleAction::Reply);
                }
                reaction_corner_chip(ui, fr.response.rect, msg, pal);
                let quick =
                    quick_react_overlay(ui, &fr.response, msg, idx, false, pal, emoji_textures);
                avatar_action
                    .or(quote_action)
                    .or(card_action)
                    .or(quick)
                    .or_else(|| {
                        bubble_context_menu(
                            ui,
                            &fr.response,
                            msg,
                            idx,
                            pal,
                            emoji_textures,
                            has_interactive_card,
                        )
                    })
            })
            .inner
        })
        .inner
    }
}

#[derive(Clone, PartialEq)]
pub(super) enum BubbleAction {
    Reply,
    /// Click on the sender's avatar beside an incoming bubble: open their account card.
    OpenSenderProfile,
    JumpToQuoted(String),
    Forward,
    React(&'static str),
    TogglePin,
    ToggleStar,
    DeleteForMe,
    RecallForEveryone,
    CallAgain {
        is_video: bool,
    },
    OpenImagePopup {
        tex_key: String,
        full_url: String,
        title: String,
        sender: String,
        time: String,
    },
    DownloadFile {
        url: String,
        file_name: String,
    },
    /// Open a saved attachment with its default app.
    OpenFile(String),
    /// Show a saved attachment in Finder / Explorer.
    RevealFile(String),
    /// Enter multi-select mode starting with this message.
    SelectMany,
    /// Show the "Chi tiết tin nhắn" dialog.
    ShowDetails,
}

/// One Zalo-style context-menu row: icon + label (full width, rounded hover). Returns clicked;
/// copy-only rows close the menu themselves.
pub(super) fn menu_item(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    color: Option<Color32>,
) -> bool {
    let fg = color.unwrap_or_else(|| ui.visuals().text_color());
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(220.0), 34.0),
        Sense::click(),
    );
    if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            Rounding::same(8.0),
            ui.visuals().widgets.hovered.weak_bg_fill,
        );
    }
    ui.painter().text(
        rect.left_center() + Vec2::new(14.0, 0.0),
        egui::Align2::LEFT_CENTER,
        icon,
        FontId::proportional(14.0),
        fg,
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(40.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.5),
        fg,
    );
    let clicked = resp.on_hover_cursor(CursorIcon::PointingHand).clicked();
    if clicked {
        ui.close_menu();
    }
    clicked
}

/// Zalo-style quick reactions: while the pointer is over a bubble, a 👍 button sits on its
/// bottom edge (click = 👍). Hovering the button opens the six-reaction strip *below* the
/// bubble, beside the button, so it never covers the message text.
/// Screen rects floating over the message list (composer, chat header): the hover reaction
/// button must not open through them.
static REACT_BLOCKERS: std::sync::RwLock<Vec<Rect>> = std::sync::RwLock::new(Vec::new());

pub(super) fn set_react_blockers(rects: Vec<Rect>) {
    if let Ok(mut g) = REACT_BLOCKERS.write() {
        *g = rects;
    }
}

pub(super) fn quick_react_overlay(
    ui: &mut egui::Ui,
    bubble: &egui::Response,
    msg: &ChatMessage,
    idx: usize,
    outgoing: bool,
    pal: &ThemePalette,
    emoji_textures: &HashMap<String, TextureHandle>,
) -> Option<BubbleAction> {
    let ctx = ui.ctx().clone();
    let chat_clip = ui.clip_rect();
    let id = Id::new(("quick_react", idx, msg.ts, &msg.id));
    let picker_open = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let pointer = ctx.pointer_hover_pos();
    // Like / reply / forward sit in a row beside the bubble, centred on it vertically and
    // 8 pt away (Zalo): the like button nearest the bubble.
    const BTN: f32 = 28.0;
    const GAP: f32 = 6.0;
    const OFFSET: f32 = 8.0;
    let group_w = BTN * 3.0 + GAP * 2.0;
    let cy = bubble.rect.center().y.min(bubble.rect.top() + 26.0);
    let group_rect = if outgoing {
        Rect::from_min_size(
            Pos2::new(bubble.rect.left() - OFFSET - group_w, cy - BTN / 2.0),
            Vec2::new(group_w, BTN),
        )
    } else {
        Rect::from_min_size(
            Pos2::new(bubble.rect.right() + OFFSET, cy - BTN / 2.0),
            Vec2::new(group_w, BTN),
        )
    };
    let btn_rect = if outgoing {
        Rect::from_min_size(
            Pos2::new(group_rect.right() - BTN, group_rect.top()),
            Vec2::splat(BTN),
        )
    } else {
        Rect::from_min_size(group_rect.min, Vec2::splat(BTN))
    };
    // The gap between bubble and buttons counts as "still hovering".
    let hover_zone = bubble.rect.union(group_rect).expand(4.0);
    // Only when the bubble is actually under the pointer: modals, the floating drawer and the
    // chat header sit on higher layers and must not trigger the strip through them.
    let own_layers = [
        ui.layer_id(),
        egui::LayerId::new(egui::Order::Foreground, id.with("btn")),
        egui::LayerId::new(egui::Order::Tooltip, id.with("picker")),
    ];
    let blocked = |p: Pos2| {
        REACT_BLOCKERS
            .read()
            .map(|r| r.iter().any(|b| b.is_positive() && b.contains(p)))
            .unwrap_or(false)
    };
    // While scrolling (and briefly after) the strip stays shut: it popped up under the moving
    // pointer and then swallowed the scroll.
    let scroll_key = Id::new("quick_react_last_scroll");
    let now = ctx.input(|i| i.time);
    if ctx.input(|i| i.smooth_scroll_delta != Vec2::ZERO || i.raw_scroll_delta != Vec2::ZERO) {
        ctx.data_mut(|d| d.insert_temp(scroll_key, now));
    }
    let scrolling = ctx
        .data(|d| d.get_temp::<f64>(scroll_key))
        .is_some_and(|t| now - t < 0.4);
    if scrolling {
        ctx.data_mut(|d| d.insert_temp(id, false));
        return None;
    }
    let near = pointer.is_some_and(|p| {
        let top = ctx.layer_id_at(p);
        top.is_none_or(|l| own_layers.contains(&l)) && !blocked(p) && hover_zone.contains(p)
    });
    // Hover intent: the chips wait a moment before appearing (a pointer passing over the
    // list no longer flashes them) and linger briefly after leaving so they can be reached.
    const SHOW_DELAY: f64 = 0.2;
    const LINGER: f64 = 0.25;
    let since_key = id.with("hover_since");
    let seen_key = id.with("hover_seen");
    let since = if near {
        let t = ctx.data(|d| d.get_temp::<f64>(since_key)).unwrap_or(now);
        ctx.data_mut(|d| d.insert_temp(since_key, t));
        Some(t)
    } else {
        ctx.data_mut(|d| d.remove::<f64>(since_key));
        None
    };
    let armed = since.is_some_and(|t| now - t >= SHOW_DELAY);
    if armed {
        ctx.data_mut(|d| d.insert_temp(seen_key, now));
    } else if let Some(t) = since {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(
            (SHOW_DELAY - (now - t)).max(0.0) + 0.01,
        ));
    }
    let lingering = ctx
        .data(|d| d.get_temp::<f64>(seen_key))
        .is_some_and(|t| now - t < LINGER);
    if lingering && !armed {
        ctx.request_repaint_after(std::time::Duration::from_millis(60));
    }
    if !armed && !lingering && !picker_open {
        return None;
    }
    let mut action = None;
    let mut reply_clicked = false;
    let mut forward_clicked = false;
    let shown = appear(&ctx, id.with("btns"), 0.16);
    // Solid chips (a translucent hover fill let the wallpaper darken them).
    let chip_fill = if pal.is_dark {
        Color32::from_rgb(52, 54, 62)
    } else {
        Color32::WHITE
    };
    let chip_hover = if pal.is_dark {
        Color32::from_rgb(66, 68, 78)
    } else {
        Color32::from_rgb(244, 244, 247)
    };
    let shadow = egui::epaint::Shadow {
        offset: Vec2::new(0.0, 2.0),
        blur: 8.0,
        spread: 0.0,
        color: Color32::from_black_alpha(if pal.is_dark { 90 } else { 40 }),
    };
    let slide = if outgoing { 6.0 } else { -6.0 } * (1.0 - shown);
    let btn_resp = egui::Area::new(id.with("btn"))
        .order(egui::Order::Foreground)
        // A new area's first (sizing) frame spans the rest of the screen; pin its size so it
        // never lies over the bubble and steals its clicks.
        .default_size(Vec2::new(group_w, BTN))
        .fixed_pos(group_rect.min + Vec2::new(slide, 0.0))
        .show(&ctx, |ui| {
            ui.set_opacity(shown);
            ui.spacing_mut().item_spacing = Vec2::new(GAP, 0.0);
            // One round chip: grows and lifts while hovered.
            let chip = |ui: &mut egui::Ui,
                        key: &str,
                        tip: &str,
                        paint: &dyn Fn(&egui::Painter, Pos2, f32, bool)| {
                let (r, resp) = ui.allocate_exact_size(Vec2::splat(BTN), Sense::click());
                let t = ui.ctx().animate_bool_with_time(
                    resp.id.with(("chip_hover", key)),
                    resp.hovered(),
                    if reduce_motion() { 0.0 } else { 0.12 },
                );
                let rad = BTN / 2.0 * (1.0 + 0.12 * t);
                let c = r.center() - Vec2::new(0.0, 2.0 * t);
                let circle = Rect::from_center_size(c, Vec2::splat(rad * 2.0));
                ui.painter()
                    .add(shadow.as_shape(circle, Rounding::same(rad)));
                ui.painter()
                    .circle_filled(c, rad, if t > 0.5 { chip_hover } else { chip_fill });
                paint(ui.painter(), c, 1.0 + 0.12 * t, t > 0.5);
                resp.on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text(tip)
            };
            let icon = |glyph: &'static str| {
                move |p: &egui::Painter, c: Pos2, k: f32, hot: bool| {
                    draw_lucide_icon(
                        p,
                        c,
                        glyph,
                        14.0 * k,
                        if hot {
                            pal.text_primary
                        } else {
                            pal.text_secondary
                        },
                    );
                }
            };
            // Zalo's reply glyph is a closing quotation mark.
            let quote_paint = |p: &egui::Painter, c: Pos2, k: f32, hot: bool| {
                p.text(
                    c + Vec2::new(0.0, 5.0 * k),
                    egui::Align2::CENTER_CENTER,
                    "\u{201D}",
                    FontId::proportional(26.0 * k),
                    if hot {
                        pal.text_primary
                    } else {
                        pal.text_secondary
                    },
                );
            };
            let like_paint = |p: &egui::Painter, c: Pos2, k: f32, _hot: bool| {
                if let Some(tex) = emoji_textures.get("like") {
                    p.image(
                        tex.id(),
                        Rect::from_center_size(c, Vec2::splat(17.0 * k)),
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                } else {
                    p.text(
                        c,
                        egui::Align2::CENTER_CENTER,
                        "👍",
                        FontId::proportional(14.0 * k),
                        pal.text_primary,
                    );
                }
            };
            // Exact-size row: `ui.horizontal` would stretch the area across the bubble and
            // swallow clicks meant for it (e.g. the quote block).
            ui.set_max_width(group_w);
            ui.allocate_ui_with_layout(
                Vec2::new(group_w, BTN),
                Layout::left_to_right(Align::Center),
                |ui| {
                    if outgoing {
                        forward_clicked =
                            chip(ui, "fwd", "Chia sẻ", &icon(lucide::FORWARD)).clicked();
                        reply_clicked = chip(ui, "reply", "Trả lời", &quote_paint).clicked();
                        chip(ui, "like", "Thích", &like_paint)
                    } else {
                        let like = chip(ui, "like", "Thích", &like_paint);
                        reply_clicked = chip(ui, "reply", "Trả lời", &quote_paint).clicked();
                        forward_clicked =
                            chip(ui, "fwd", "Chia sẻ", &icon(lucide::FORWARD)).clicked();
                        like
                    }
                },
            )
            .inner
        })
        .inner;
    if btn_resp.clicked() {
        action = Some(BubbleAction::React(REACTION_LIST[0].0));
    } else if reply_clicked {
        action = Some(BubbleAction::Reply);
    } else if forward_clicked {
        action = Some(BubbleAction::Forward);
    }
    // The reaction strip opens after resting on 👍 for a moment and closes a moment after
    // the pointer leaves it, instead of snapping open/shut on every pass.
    const PICKER_DELAY: f64 = 0.35;
    const PICKER_GRACE: f64 = 0.3;
    let like_key = id.with("like_since");
    let like_since = if btn_resp.hovered() {
        let t = ctx.data(|d| d.get_temp::<f64>(like_key)).unwrap_or(now);
        ctx.data_mut(|d| d.insert_temp(like_key, t));
        Some(t)
    } else {
        ctx.data_mut(|d| d.remove::<f64>(like_key));
        None
    };
    let like_armed = like_since.is_some_and(|t| now - t >= PICKER_DELAY);
    if let Some(t) = like_since.filter(|_| !like_armed) {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(
            (PICKER_DELAY - (now - t)).max(0.0) + 0.01,
        ));
    }
    let mut keep_open = like_armed;
    if like_armed || picker_open {
        // Reaction strip pops up above the like button.
        let (pivot, pos) = (
            egui::Align2::CENTER_BOTTOM,
            Pos2::new(btn_rect.center().x, btn_rect.top() - 6.0),
        );
        let pop = appear(&ctx, id.with("picker"), 0.35);
        let picker = egui::Area::new(id.with("picker"))
            .order(egui::Order::Tooltip)
            // Stay inside the conversation: near the left edge it spilled over the chat list.
            .constrain_to(chat_clip)
            .pivot(pivot)
            .fixed_pos(pos + Vec2::new(0.0, (1.0 - pop) * -6.0))
            .show(&ctx, |ui| {
                ui.set_opacity(pop);
                egui::Frame::none()
                    .fill(pal.bg_elevated)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .rounding(Rounding::same(17.0))
                    .shadow(ctx.style().visuals.popup_shadow)
                    .inner_margin(Margin::symmetric(6.0, 3.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);
                            // Zalo-like: emojis pop in one after another (back-out overshoot)
                            // and grow + lift while hovered.
                            let back_out = |t: f32| {
                                let (c1, c3) = (1.70158_f32, 2.70158_f32);
                                1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
                            };
                            for (i, &(code, key, tooltip)) in REACTION_LIST.iter().enumerate() {
                                let (slot, resp) =
                                    ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
                                let ti = (pop * 1.75 - i as f32 * 0.15).clamp(0.0, 1.0);
                                let grow = back_out(ti).max(0.0);
                                let hover = ui.ctx().animate_bool_with_time(
                                    resp.id.with("react_hover"),
                                    resp.hovered(),
                                    if reduce_motion() { 0.0 } else { 0.12 },
                                );
                                let size = 22.0 * grow + 8.0 * hover;
                                let center = slot.center() - Vec2::new(0.0, 5.0 * hover);
                                if size > 0.5 {
                                    if let Some(tex) = emoji_textures.get(key) {
                                        ui.painter().image(
                                            tex.id(),
                                            Rect::from_center_size(center, Vec2::splat(size)),
                                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                            Color32::WHITE,
                                        );
                                    } else {
                                        ui.painter().text(
                                            center,
                                            egui::Align2::CENTER_CENTER,
                                            code,
                                            FontId::proportional(size * 0.8),
                                            pal.text_primary,
                                        );
                                    }
                                }
                                if resp
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text(tooltip)
                                    .clicked()
                                {
                                    action = Some(BubbleAction::React(code));
                                }
                            }
                        });
                    });
            });
        // Stay open only over the strip, the like button or the short corridor between them;
        // the strip's bounding box used to cover the reply/share buttons, so moving there kept
        // it open.
        let strip = picker.response.rect.expand(4.0);
        let corridor = Rect::from_min_max(
            Pos2::new(btn_rect.left(), strip.bottom() - 2.0),
            Pos2::new(btn_rect.right(), btn_rect.top() + 2.0),
        );
        keep_open |= pointer.is_some_and(|p| {
            strip.contains(p) || btn_rect.expand(3.0).contains(p) || corridor.contains(p)
        });
    }
    let inside_key = id.with("picker_inside");
    if keep_open {
        ctx.data_mut(|d| d.insert_temp(inside_key, now));
    } else if picker_open {
        let left_at = ctx.data(|d| d.get_temp::<f64>(inside_key)).unwrap_or(now);
        if now - left_at < PICKER_GRACE {
            keep_open = true;
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
    }
    if action.is_some() {
        keep_open = false;
    }
    ctx.data_mut(|d| d.insert_temp(id, keep_open));
    action
}

/// Right-click menu on a chat bubble (Zalo layout). Reactions live on the hover 👍 button.
pub(super) fn bubble_context_menu(
    ui: &mut egui::Ui,
    bubble: &egui::Response,
    msg: &ChatMessage,
    idx: usize,
    _pal: &ThemePalette,
    _emoji_textures: &HashMap<String, TextureHandle>,
    _has_interactive_card: bool,
) -> Option<BubbleAction> {
    // egui attributes a click to widgets laid out on the *previous* frame, so a response that
    // only exists once the secondary button is down never receives it (the menu never opened
    // on right-click / two-finger tap). Detect the click from raw input instead and draw the
    // menu ourselves at the pointer.
    let ctx = ui.ctx().clone();
    let menu_id = Id::new(("bubble_menu", idx, msg.ts, &msg.id));
    let open_key = Id::new("bubble_menu_open");
    let more_key = menu_id.with("more");
    let sec = ui.input(|i| {
        i.pointer
            .button_clicked(egui::PointerButton::Secondary)
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    if let Some(p) = sec.filter(|p| bubble.rect.contains(*p)) {
        ctx.data_mut(|d| {
            d.insert_temp(open_key, (menu_id, p));
            d.insert_temp(more_key, false);
        });
    }
    let (_, at) = ctx
        .data(|d| d.get_temp::<(Id, Pos2)>(open_key))
        .filter(|(id, _)| *id == menu_id)?;
    let mut action = None;
    let close = false;
    let mut show_more = ctx.data(|d| d.get_temp::<bool>(more_key)).unwrap_or(false);
    let fade = appear(&ctx, menu_id, 0.14);
    let area = egui::Area::new(menu_id)
        .order(egui::Order::Foreground)
        .fixed_pos(at + Vec2::new(0.0, (1.0 - fade) * -6.0))
        .constrain(true)
        .show(&ctx, |ui| {
            ui.set_opacity(fade);
            egui::Frame::popup(ui.style())
                .rounding(Rounding::same(14.0))
                .inner_margin(Margin::same(6.0))
                .show(ui, |ui| {
                    // Zalo-style menu: icon rows grouped by separators.
                    ui.set_min_width(230.0);
                    ui.set_max_width(260.0);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    if menu_item(ui, lucide::REPLY, "Trả lời", None) {
                        action = Some(BubbleAction::Reply);
                    }
                    if menu_item(ui, lucide::FORWARD, "Chia sẻ", None) {
                        action = Some(BubbleAction::Forward);
                    }
                    ui.separator();
                    if menu_item(ui, lucide::COPY, "Copy tin nhắn", None) {
                        ui.ctx().copy_text(msg.text.clone());
                    }
                    ui.separator();
                    let pin_label = if msg.is_pinned {
                        "Bỏ ghim tin nhắn"
                    } else {
                        "Ghim tin nhắn"
                    };
                    if menu_item(ui, lucide::PIN, pin_label, None) {
                        action = Some(BubbleAction::TogglePin);
                    }
                    let star_label = if msg.is_starred {
                        "Bỏ đánh dấu tin nhắn"
                    } else {
                        "Đánh dấu tin nhắn"
                    };
                    if menu_item(ui, lucide::STAR, star_label, None) {
                        action = Some(BubbleAction::ToggleStar);
                    }
                    if menu_item(ui, lucide::LIST_CHECKS, "Chọn nhiều tin nhắn", None) {
                        action = Some(BubbleAction::SelectMany);
                    }
                    if menu_item(ui, lucide::INFO, "Xem chi tiết", None) {
                        action = Some(BubbleAction::ShowDetails);
                    }
                    if menu_item(
                        ui,
                        lucide::ELLIPSIS,
                        if show_more {
                            "Tuỳ chọn khác ▾"
                        } else {
                            "Tuỳ chọn khác ›"
                        },
                        None,
                    ) {
                        show_more = !show_more;
                    }
                    if show_more {
                        ui.indent("more_opts", |ui| {
                            ui.set_min_width(220.0);
                            let mut any = false;
                            if is_zalo_photo_message(msg) {
                                if let Some(thumb_url) = photo_primary_url(msg) {
                                    any = true;
                                    if menu_item(ui, lucide::SEARCH, "Xem ảnh phóng to", None) {
                                        action = Some(BubbleAction::OpenImagePopup {
                                            tex_key: format!(
                                                "thumb_{}",
                                                crate::crypto::md5_hex(thumb_url)
                                            ),
                                            full_url: msg
                                                .link_url
                                                .clone()
                                                .filter(|u| !u.is_empty())
                                                .unwrap_or_else(|| thumb_url.to_string()),
                                            title: msg
                                                .link_title
                                                .clone()
                                                .filter(|t| {
                                                    !t.is_empty()
                                                        && !t.starts_with("http://")
                                                        && !t.starts_with("https://")
                                                })
                                                .unwrap_or_else(|| "Hình ảnh".to_string()),
                                            sender: if msg.is_outgoing {
                                                "Bạn".to_string()
                                            } else {
                                                msg.sender.clone()
                                            },
                                            time: msg.time.clone(),
                                        });
                                    }
                                }
                            }
                            if is_zalo_file_or_video_message(msg) {
                                if let Some(url) = msg.link_url.as_deref().filter(|u| !u.is_empty())
                                {
                                    any = true;
                                    let resolved_name = crate::chat::resolve_attachment_file_name(
                                        msg.link_title.as_deref(),
                                        msg.text.trim(),
                                        url,
                                    );
                                    if menu_item(ui, lucide::DOWNLOAD, "Lưu tập tin về máy", None)
                                    {
                                        action = Some(BubbleAction::DownloadFile {
                                            url: url.to_string(),
                                            file_name: resolved_name,
                                        });
                                    }
                                }
                            }
                            if !any {
                                ui.label(RichText::new("Không có tuỳ chọn khác").size(12.5).weak());
                            }
                        });
                    }
                    ui.separator();
                    // Zalo keeps "Thu hồi" next to delete in the main menu, for your own messages.
                    if msg.is_outgoing
                        && msg.text.trim() != "[Tin nhắn đã thu hồi]"
                        && menu_item(
                            ui,
                            lucide::UNDO,
                            "Thu hồi",
                            Some(Color32::from_rgb(249, 115, 22)),
                        )
                    {
                        action = Some(BubbleAction::RecallForEveryone);
                    }
                    if menu_item(
                        ui,
                        lucide::TRASH,
                        "Xóa chỉ ở phía tôi",
                        Some(Color32::from_rgb(220, 60, 60)),
                    ) {
                        action = Some(BubbleAction::DeleteForMe);
                    }
                });
        });
    let outside = ui.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|q| !area.response.rect.contains(q))
    });
    if action.is_some() || close || outside || ui.input(|i| i.key_pressed(Key::Escape)) {
        ctx.data_mut(|d| d.remove::<(Id, Pos2)>(open_key));
    } else {
        ctx.data_mut(|d| d.insert_temp(more_key, show_more));
    }
    action
}

/// Quoted-message block shown at the top of a reply bubble.
pub(super) fn render_quote_block(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    pal: &ThemePalette,
    outgoing: bool,
    max_w: f32,
) -> Option<BubbleAction> {
    let qt = msg.reply_to_text.as_ref()?;
    let (text_col, meta_col) = if outgoing {
        (pal.bubble_out_text, pal.bubble_out_meta)
    } else {
        (pal.bubble_in_text, pal.bubble_in_meta)
    };
    // Own bubbles are dark graphite in both themes, so their quote uses light ink.
    let bar_col = if outgoing {
        Color32::from_rgb(226, 226, 232)
    } else {
        pal.primary
    };
    let bg_col = if outgoing {
        Color32::from_rgba_unmultiplied(0, 0, 0, 46)
    } else {
        text_col.linear_multiply(0.06)
    };
    let inner_col_w = (max_w - 28.0).max(72.0);
    let frame = egui::Frame::none()
        .fill(bg_col)
        .rounding(Rounding::same(7.0))
        .inner_margin(Margin::symmetric(8.0, 5.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (bar_rect, _) = ui.allocate_exact_size(Vec2::new(3.0, 28.0), Sense::hover());
                ui.painter()
                    .rect_filled(bar_rect, Rounding::same(1.5), bar_col);
                ui.vertical(|ui| {
                    ui.set_width(inner_col_w);
                    let who = msg
                        .reply_to_sender
                        .as_deref()
                        .filter(|sn| !sn.is_empty() && !sn.chars().all(|c| c.is_ascii_digit()))
                        .unwrap_or("Tin nhắn trích dẫn");
                    ui.label(RichText::new(who).color(bar_col).size(11.5).strong());
                    ui.add(
                        egui::Label::new(
                            RichText::new(truncate_chars(qt, 84))
                                .color(meta_col)
                                .size(12.0),
                        )
                        .wrap(),
                    );
                });
            });
        });
    let reference = crate::message_navigation::message_reply_reference(msg);
    let response = ui.interact(
        frame.response.rect,
        frame.response.id.with("quote_jump"),
        Sense::click(),
    );
    let clicked = response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Đi tới tin nhắn gốc đã tải")
        .clicked();
    ui.add_space(5.0);
    clicked.then(|| BubbleAction::JumpToQuoted(reference.unwrap_or_default()))
}

pub(super) fn render_reactions(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    pal: &ThemePalette,
    emoji_textures: &HashMap<String, TextureHandle>,
) {
    let _ = emoji_textures;
    let has_pin = msg.is_pinned;
    let has_star = msg.is_starred;
    let has_react = !msg.reactions.is_empty();

    if !has_pin && !has_star && !has_react {
        return;
    }
    // Reactions are drawn on the bubble's bottom edge (`reaction_corner_chip`); keep room so the
    // chip does not cover the last line or the time.
    if has_react {
        ui.add_space(8.0);
    }
    if !has_pin && !has_star {
        return;
    }

    ui.add_space(3.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);

        if has_pin {
            egui::Frame::none()
                .fill(pal.primary.linear_multiply(0.18))
                .stroke(Stroke::new(1.0_f32, pal.primary))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::symmetric(5.0, 2.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("{} Đã ghim", lucide::PIN))
                            .size(11.0)
                            .color(pal.primary)
                            .strong(),
                    );
                });
        }

        if has_star {
            egui::Frame::none()
                .fill(Color32::from_rgb(234, 179, 8).linear_multiply(0.2))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(234, 179, 8)))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::symmetric(5.0, 2.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("{} Đã lưu", lucide::STAR))
                            .size(11.0)
                            .color(Color32::from_rgb(234, 179, 8))
                            .strong(),
                    );
                });
        }
    });
}

/// Zalo-style reaction chip straddling the bubble's bottom-right edge (half inside, half
/// outside): up to three emoji plus the total count. Clicking it lists who reacted.
pub(super) fn reaction_corner_chip(
    ui: &mut egui::Ui,
    bubble: Rect,
    msg: &ChatMessage,
    pal: &ThemePalette,
) {
    if msg.reactions.is_empty() {
        return;
    }
    let items: Vec<(String, u64)> = if msg.reaction_summaries.is_empty() {
        msg.reactions.iter().map(|r| (r.clone(), 1)).collect()
    } else {
        msg.reaction_summaries
            .iter()
            .map(|r| (r.emoji.clone(), r.count))
            .collect()
    };
    let total: u64 = items.iter().map(|(_, n)| *n).sum();
    let shown: Vec<&String> = items.iter().map(|(e, _)| e).take(3).collect();
    let count_txt = total.to_string();
    let galley =
        ui.fonts(|f| f.layout_no_wrap(count_txt, FontId::proportional(11.0), pal.text_secondary));
    let h = 22.0;
    let w = 8.0 + shown.len() as f32 * 16.0 + 4.0 + galley.size().x + 8.0;
    let rect = Rect::from_min_size(
        Pos2::new(bubble.right() - 10.0 - w, bubble.bottom() - h / 2.0),
        Vec2::new(w, h),
    );
    let resp = ui.interact(
        rect,
        Id::new(("reaction_chip", &msg.id, msg.ts)),
        Sense::click(),
    );
    let fill = if pal.is_dark {
        Color32::from_rgb(52, 54, 62)
    } else {
        Color32::WHITE
    };
    let shadow = egui::epaint::Shadow {
        offset: Vec2::new(0.0, 1.0),
        blur: 6.0,
        spread: 0.0,
        color: Color32::from_black_alpha(if pal.is_dark { 90 } else { 40 }),
    };
    let rounding = Rounding::same(h / 2.0);
    ui.painter().add(shadow.as_shape(rect, rounding));
    ui.painter().rect_filled(
        rect,
        rounding,
        if resp.hovered() {
            if pal.is_dark {
                Color32::from_rgb(66, 68, 78)
            } else {
                Color32::from_rgb(244, 244, 247)
            }
        } else {
            fill
        },
    );
    let mut x = rect.left() + 8.0 + 8.0;
    for e in &shown {
        if let Some(tex) = super::emoji_picker::emoji_texture_shared(ui.ctx(), e) {
            ui.painter().image(
                tex.id(),
                Rect::from_center_size(Pos2::new(x, rect.center().y), Vec2::splat(15.0)),
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            ui.ctx().request_repaint();
        }
        x += 16.0;
    }
    ui.painter().galley(
        Pos2::new(x - 8.0 + 4.0, rect.center().y - galley.size().y / 2.0),
        galley,
        pal.text_secondary,
    );
    if resp
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Xem ai đã thả biểu cảm")
        .clicked()
    {
        ui.ctx().data_mut(|d| {
            d.insert_temp(Id::new("reaction_people_message"), msg.clone());
            d.insert_temp(
                Id::new("reaction_people_message").with("filter"),
                String::new(),
            );
        });
    }
}

#[cfg(test)]
mod quote_navigation_tests {
    use super::*;

    #[test]
    fn call_cards_use_short_telegram_style_wording() {
        let missed = ChatMessage {
            id: "call_1".into(),
            text: "Cuộc gọi".into(),
            is_outgoing: false,
            ..Default::default()
        };
        let card = extract_call_card_view(&missed).expect("call card");
        assert_eq!(card.title, "Cuộc gọi đến");
        assert_eq!(card.status_badge, "↙ Nhỡ");
        assert!(card.is_missed_or_failed);

        let out = ChatMessage {
            id: "call_2".into(),
            text: "📹 Cuộc gọi video đi (133s)".into(),
            is_outgoing: true,
            ..Default::default()
        };
        let card = extract_call_card_view(&out).expect("call card");
        assert_eq!(card.title, "Cuộc gọi video đi");
        assert_eq!(card.status_badge, "↗ 2 phút 13 giây");
        assert!(!card.is_missed_or_failed);
    }

    #[test]
    fn fit_between_never_panics_when_space_is_narrower_than_minimum() {
        // Crash 2026-11-07: file card in a 222 px chat column (min 250).
        assert_eq!(fit_between(300.0, 250.0, 222.386_47), 222.386_47);
        assert_eq!(fit_between(100.0, 250.0, 360.0), 250.0);
        assert_eq!(fit_between(300.0, 250.0, 360.0), 300.0);
        assert_eq!(fit_between(500.0, 250.0, 360.0), 360.0);
    }

    #[test]
    fn quote_navigation_bubble_propagates_action_for_both_directions() {
        for is_outgoing in [false, true] {
            let ctx = egui::Context::default();
            let msg = ChatMessage {
                id: "reply-1".into(),
                text: "Reply body".into(),
                sender: "Sender".into(),
                reply_to_id: Some("original-1".into()),
                reply_to_text: Some("Original text".into()),
                is_outgoing,
                ..Default::default()
            };
            let mut position = egui::pos2(30.0, 30.0);
            let mut action = None;
            for frame in 0..4 {
                let mut events = vec![egui::Event::PointerMoved(position)];
                if frame >= 2 {
                    events.push(egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: frame == 2,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(600.0, 400.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            action = render_chat_bubble(
                                ui,
                                &msg,
                                false,
                                false,
                                false,
                                false,
                                "Thread",
                                None,
                                &ThemePalette::light(),
                                &HashMap::new(),
                                &mut HashSet::new(),
                                &HashMap::new(),
                                0,
                            );
                        });
                    },
                );
                if frame == 0 {
                    position = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.job.text == "Original text" => {
                                Some(text.pos + Vec2::new(4.0, 4.0))
                            }
                            _ => None,
                        })
                        .expect("Quote must be rendered in the bubble");
                }
            }
            assert!(
                matches!(action, Some(BubbleAction::JumpToQuoted(ref reference)) if reference == "original-1"),
                "outgoing={is_outgoing} some={} react={} reply={}",
                action.is_some(),
                matches!(action, Some(BubbleAction::React(_))),
                matches!(action, Some(BubbleAction::Reply))
            );
        }
    }

    #[test]
    fn secondary_click_on_bubble_opens_context_menu() {
        let ctx = egui::Context::default();
        let msg = ChatMessage {
            id: "m1".into(),
            text: "hello".into(),
            sender: "A".into(),
            ..Default::default()
        };
        let pos = egui::pos2(60.0, 30.0);
        let frames: [Option<bool>; 4] = [None, Some(true), Some(false), None];
        let mut opened = false;
        let mut drew_reply = false;
        for pressed in frames {
            let mut events = vec![egui::Event::PointerMoved(pos)];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(600.0, 400.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let _ = render_chat_bubble(
                            ui,
                            &msg,
                            false,
                            false,
                            false,
                            false,
                            "Thread",
                            None,
                            &ThemePalette::light(),
                            &HashMap::new(),
                            &mut HashSet::new(),
                            &HashMap::new(),
                            0,
                        );
                    });
                },
            );
            opened |= ctx.data(|d| {
                d.get_temp::<(Id, egui::Pos2)>(Id::new("bubble_menu_open"))
                    .is_some()
            });
            drew_reply |= out.shapes.iter().any(
                |cs| matches!(&cs.shape, egui::Shape::Text(t) if t.galley.job.text == "Trả lời"),
            );
        }
        assert!(
            opened,
            "right-click / two-finger tap must open the bubble menu"
        );
        assert!(drew_reply, "the opened menu must render its items");
    }

    #[test]
    fn quote_navigation_emits_exact_reference_when_clicked() {
        let ctx = egui::Context::default();
        let msg = ChatMessage {
            reply_to_id: Some("00123".into()),
            reply_to_text: Some("Original text".into()),
            ..Default::default()
        };
        let mut action = None;
        for pressed in [None, Some(true), Some(false)] {
            let mut events = vec![egui::Event::PointerMoved(egui::pos2(30.0, 30.0))];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton {
                    pos: egui::pos2(30.0, 30.0),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        action = render_quote_block(ui, &msg, &ThemePalette::light(), false, 300.0);
                    });
                },
            );
        }
        assert!(
            matches!(action, Some(BubbleAction::JumpToQuoted(reference)) if reference == "00123")
        );
    }

    #[test]
    fn quote_navigation_advertises_click_when_reference_exists() {
        let ctx = egui::Context::default();
        let msg = ChatMessage {
            reply_to_id: Some("original-1".into()),
            reply_to_text: Some("Original text".into()),
            ..Default::default()
        };
        for _ in 0..2 {
            let output = ctx.run(
                egui::RawInput {
                    events: vec![egui::Event::PointerMoved(egui::pos2(30.0, 30.0))],
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        render_quote_block(ui, &msg, &ThemePalette::light(), false, 300.0);
                    });
                },
            );
            if output.platform_output.cursor_icon == egui::CursorIcon::PointingHand {
                return;
            }
        }
        panic!("Quote with an exact reference must advertise a clickable target");
    }
}

#[cfg(test)]
mod link_card_tests {
    use super::*;

    #[test]
    fn web_link_with_cdn_thumbnail_is_a_link_card_not_a_photo() {
        // Zalo re-hosts link preview thumbnails on its photo CDN; the message is still a link.
        let msg = ChatMessage {
            id: "l1".into(),
            text: "https://docs.google.com/document/d/abc/edit".into(),
            link_url: Some("https://docs.google.com/document/d/abc/edit".into()),
            link_title: Some("Nội dung Thiết kế Landing page".into()),
            link_thumb: Some("https://photo-stal-12.zdn.vn/jpg/preview.jpg".into()),
            ..Default::default()
        };
        assert!(!is_zalo_photo_message(&msg));
        let (_, url, domain) = extract_message_link_view(&msg).expect("link view");
        assert_eq!(url, "https://docs.google.com/document/d/abc/edit");
        assert_eq!(domain, "docs.google.com");
    }

    #[test]
    fn zalo_photo_stays_a_photo() {
        let msg = ChatMessage {
            id: "p1".into(),
            text: "[Hình ảnh]".into(),
            link_url: Some("https://photo-stal-12.zdn.vn/jpg/full.jpg".into()),
            link_thumb: Some("https://photo-stal-12.zdn.vn/jpg/thumb.jpg".into()),
            ..Default::default()
        };
        assert!(is_zalo_photo_message(&msg));
    }
}

#[cfg(test)]
mod mention_tests {
    use super::find_mentions;

    #[test]
    fn finds_longest_known_name_after_at() {
        let names = vec![
            "Lê Dũng Bá".to_string(),
            "Lê".to_string(),
            "All".to_string(),
        ];
        let t = "@Lê Dũng Bá cảm ơn phòng HTCN, mail a@b.com @All";
        let spans = find_mentions(t, &names);
        assert_eq!(spans.len(), 2);
        assert_eq!(&t[spans[0].0..spans[0].1], "@Lê Dũng Bá");
        assert_eq!(&t[spans[1].0..spans[1].1], "@All");
        assert!(find_mentions("@Lênin", &names).is_empty());
    }
}
