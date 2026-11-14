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
    failed_stickers: &HashMap<String, (u8, std::time::Instant)>,
) -> bool {
    let Some((sid, cid)) = is_sticker_message(msg) else {
        return false;
    };

    let key = format!("stk_{cid}_{sid}");
    let url = if cid == 0 && (1..=30).contains(&sid) {
        format!("https://stc-chat.zdn.vn/images/stickers/default/thumb/{sid}.png")
    } else {
        format!("https://api.zaloapp.com/api/emoticon/sticker/webpc?eid={sid}&size=130")
    };

    crate::gui::queue_sticker_fetch(sticker_textures, loading_stickers, failed_stickers, key.clone(), url);

    if let Some(tex) = sticker_textures.get(&key) {
        ui.image((tex.id(), Vec2::new(116.0, 116.0)));
        true
    } else {
        false
    }
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

    let title = if is_video {
        if is_incoming {
            if is_missed_or_failed {
                "Cuộc gọi video nhỡ".to_string()
            } else {
                "Cuộc gọi video đến".to_string()
            }
        } else {
            "Cuộc gọi video đi".to_string()
        }
    } else if is_incoming {
        if is_missed_or_failed {
            "Cuộc gọi thoại nhỡ".to_string()
        } else {
            "Cuộc gọi thoại đến".to_string()
        }
    } else {
        "Cuộc gọi thoại đi".to_string()
    };

    let arrow = if is_incoming { "↙" } else { "↗" };
    let status_badge = match (is_missed_or_failed, secs_opt) {
        // The title already names the missed/failed state; show the reason instead of repeating it.
        (true, _) => {
            let reason_txt = raw_reason
                .as_deref()
                .filter(|r| !r.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    if is_incoming {
                        "Bạn đã bỏ lỡ cuộc gọi này".to_string()
                    } else {
                        "Người nhận không nghe máy".to_string()
                    }
                });
            format!("{arrow} {reason_txt}")
        }
        (false, Some(s)) if s >= 60 => {
            format!("{arrow} Thời lượng: {} phút {:02} giây", s / 60, s % 60)
        }
        (false, Some(s)) => format!("{arrow} Thời lượng: {s} giây"),
        (false, None) => format!("{arrow} Đã kết thúc"),
    };

    // When the call is missed/failed the badge already carries the reason, so only surface a
    // separate reason line for calls that otherwise look successful.
    let reason = if is_missed_or_failed {
        None
    } else {
        raw_reason.map(|r| {
            if r.contains("gián đoạn") || r.contains("từ chối") {
                "Người nhận từ chối hoặc kết nối gián đoạn".to_string()
            } else if r.contains("đang bận") {
                "Người nhận đang bận hoặc từ chối".to_string()
            } else {
                r
            }
        })
    };

    Some(CallCardView {
        title,
        status_badge,
        reason,
        is_video,
        is_missed_or_failed,
    })
}

pub(super) fn render_call_card_widget(
    ui: &mut egui::Ui,
    card: &CallCardView,
    pal: &ThemePalette,
    card_width: f32,
) -> Option<BubbleAction> {
    let mut action = None;
    let accent = if card.is_missed_or_failed {
        Color32::from_rgb(239, 68, 68)
    } else {
        Color32::from_rgb(34, 197, 94)
    };

    ui.set_width(card_width);
    let text_col_w = (card_width - 52.0).max(110.0);
    ui.vertical(|ui| {
        ui.set_width(card_width);
        ui.horizontal(|ui| {
            let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::hover());
            ui.painter().circle_filled(
                icon_rect.center(),
                20.0,
                accent.linear_multiply(if pal.is_dark { 0.18 } else { 0.14 }),
            );
            ui.painter().circle_stroke(
                icon_rect.center(),
                20.0,
                Stroke::new(1.2_f32, accent.linear_multiply(0.55)),
            );
            if card.is_video {
                draw_icon_video(ui.painter(), icon_rect.center(), 18.0, accent);
            } else {
                draw_icon_phone(ui.painter(), icon_rect.center(), 18.0, accent);
            }

            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.set_width(text_col_w);
                ui.add(
                    egui::Label::new(
                        RichText::new(&card.title)
                            .size(13.8)
                            .color(pal.text_primary)
                            .strong(),
                    )
                    .wrap(),
                );
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(&card.status_badge)
                            .size(11.8)
                            .color(accent)
                            .strong(),
                    )
                    .wrap(),
                );
                if let Some(reason) = &card.reason {
                    ui.add_space(3.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(reason).size(11.2).color(pal.text_secondary),
                        )
                        .wrap(),
                    );
                }
            });
        });
        ui.add_space(8.0);
        let (sep_rect, _) = ui.allocate_exact_size(Vec2::new(card_width, 1.0), Sense::hover());
        ui.painter().line_segment(
            [sep_rect.left_center(), sep_rect.right_center()],
            Stroke::new(1.0_f32, pal.border),
        );
        ui.add_space(6.0);

        let btn_label = if card.is_video {
            format!("{}  Gọi video lại", lucide::VIDEO)
        } else {
            format!("{}  Gọi lại", lucide::PHONE)
        };
        let call_btn = egui::Button::new(
            RichText::new(btn_label)
                .size(12.5)
                .color(Color32::WHITE)
                .strong(),
        )
        .fill(pal.primary)
        .stroke(Stroke::new(1.0_f32, pal.primary_hover))
        .rounding(Rounding::same(7.0))
        .min_size(Vec2::new(card_width, 30.0));

        if ui
            .add(call_btn)
            .on_hover_cursor(CursorIcon::PointingHand)
            .clicked()
        {
            action = Some(BubbleAction::CallAgain {
                is_video: card.is_video,
            });
        }
    });
    action
}

/// Open a URL in the user's default system browser. Trả `Err` thân thiện để
/// caller toast cho user thay vì fail im lặng (#107).
pub(super) fn open_external_url(url: &str) -> Result<(), String> {
    open_external_url_with_monitor(url, true)
}

/// Mở URL; `monitor_exit=true` thì spawn watcher báo lỗi khi opener exit fail
/// (xdg-open start được nhưng không mở được URL vẫn toast, #2311).
pub(super) fn open_external_url_with_monitor(
    url: &str,
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))] monitor_exit: bool,
) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("Liên kết trống".into());
    }

    let is_local_file = trimmed.starts_with("file://")
        || std::path::Path::new(trimmed).exists()
        || (trimmed.len() >= 3
            && trimmed.as_bytes()[1] == b':'
            && (trimmed.as_bytes()[2] == b'\\' || trimmed.as_bytes()[2] == b'/'))
        || trimmed.starts_with(r"\\")
        || (cfg!(unix) && trimmed.starts_with('/'));

    // Chuẩn hoá link không scheme (www.x, x.com/abc) thay vì bỏ qua im lặng.
    let owned;
    let target = if is_local_file {
        owned = trimmed.strip_prefix("file://").unwrap_or(trimmed).to_string();
        &owned
    } else if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        trimmed
    } else if trimmed.contains('.') && !trimmed.contains(' ') && !trimmed.contains("://") {
        owned = format!("https://{trimmed}");
        &owned
    } else {
        return Err(format!("Không mở được liên kết: {trimmed}"));
    };
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
        let mut file: Vec<u16> = target.encode_utf16().collect();
        file.push(0);
        let rc = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                op.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                5, // SW_SHOW
            )
        };
        // ShellExecuteW trả <= 32 khi lỗi (không có handler, bị chặn AV/policy).
        // Không fallback qua `cmd /C start`: cmd diễn giải `&` trong URL thành
        // toán tử shell (`https://x/?a=1&calc` → chạy `calc`). Trả lỗi để caller
        // hiện toast thay vì thực thi lệnh ngoài ý muốn.
        if rc > 32 {
            return Ok(());
        }
        Err("Không mở được liên kết (hệ thống chặn mở URL)".into())
    }
    #[cfg(target_os = "macos")]
    {
        match std::process::Command::new("open").arg(target).spawn() {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Không mở được liên kết ({e})")),
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        match std::process::Command::new("xdg-open").arg(target).spawn() {
            Ok(mut child) => {
                if monitor_exit {
                    // Free function không có GuiTx: watcher log stderr; launch-fail
                    // vẫn trả Err để caller toast ngay (#2311).
                    std::thread::spawn(move || {
                        if let Ok(status) = child.wait() {
                            if !status.success() {
                                eprintln!("xdg-open exit {status}");
                            }
                        }
                    });
                }
                Ok(())
            }
            Err(e) => Err(format!("Không mở được liên kết ({e})")),
        }
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

/// Native OS folder-picker dialog. Dùng cho chọn thư mục (download dir, data
/// dir, migrate cả thư mục) — khác `pick_native_file_dialog` chỉ chọn được file
/// (#106: nút Open xám vì OFN_FILEMUSTEXIST / `choose file` không cho chọn folder).
pub(super) fn pick_native_folder_dialog(title: &str) -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        // IFileDialog với FOS_PICKFOLDERS qua PowerShell (tránh COM-FFI tay):
        // mở dialog chọn thư mục, in ra FullName khi OK.
        let title_esc = title.replace('\'', "''");
        let ps = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
             $d.Description = '{title_esc}'; $d.ShowNewFolderButton = $true; \
             if ($d.ShowDialog() -eq 'OK') {{ $d.SelectedPath }}"
        );
        if let Ok(out) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
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
    #[cfg(target_os = "macos")]
    {
        let prompt = format!("POSIX path of (choose folder with prompt \"{title}\")");
        if let Ok(out) = std::process::Command::new("osascript")
            .args(["-e", &prompt])
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
        if let Ok(out) = std::process::Command::new("zenity")
            .args(["--file-selection", "--directory", "--title", title])
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return Some(std::path::PathBuf::from(s));
                }
            }
        }
        if let Ok(out) = std::process::Command::new("kdialog")
            .args(["--getexistingdirectory", "."])
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

/// Kết quả folder picker: phân biệt user cancel với launch fail (#3609).
pub(super) enum FolderPick {
    Picked(std::path::PathBuf),
    Cancelled,
    LaunchFailed(String),
}

pub(super) fn pick_native_folder_dialog_result(title: &str) -> FolderPick {
    match pick_native_folder_dialog(title) {
        Some(p) => FolderPick::Picked(p),
        None => {
            // Thiếu helper → launch fail có hướng dẫn; user bấm Cancel vẫn là Cancel.
            #[cfg(target_os = "windows")]
            let missing = std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", "exit 0"])
                .output()
                .is_err();
            #[cfg(target_os = "macos")]
            let missing = std::process::Command::new("osascript")
                .args(["-e", "return 1"])
                .output()
                .is_err();
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            let missing = std::process::Command::new("zenity")
                .arg("--version")
                .output()
                .is_err()
                && std::process::Command::new("kdialog")
                    .arg("--version")
                    .output()
                    .is_err();
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            let hint = "Không mở được hộp chọn thư mục (thiếu zenity/kdialog) — hãy cài một trong hai rồi thử lại";
            #[cfg(target_os = "macos")]
            let hint = "Không mở được hộp chọn thư mục (osascript lỗi)";
            #[cfg(target_os = "windows")]
            let hint = "Không mở được hộp chọn thư mục (thiếu PowerShell)";
            if missing {
                return FolderPick::LaunchFailed(hint.into());
            }
            FolderPick::Cancelled
        }
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

    #[cfg(not(target_os = "windows"))]
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
        const CF_DIBV5: u32 = 17;
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

        // SAFETY: clipboard handles are inspected and released on this thread; locked data is copied before release.
        unsafe {
            let cf_png = RegisterClipboardFormatA(b"PNG\0".as_ptr());
            let cf_mime_png = RegisterClipboardFormatA(b"image/png\0".as_ptr());
            let has_png = cf_png != 0 && IsClipboardFormatAvailable(cf_png) != 0;
            let has_mime_png = cf_mime_png != 0 && IsClipboardFormatAvailable(cf_mime_png) != 0;
            let has_dib = IsClipboardFormatAvailable(CF_DIB) != 0
                || IsClipboardFormatAvailable(CF_DIBV5) != 0;
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

            // Validate while borrowed, then release handles before pixel decoding.
            for format in [CF_DIBV5, CF_DIB] {
                if IsClipboardFormatAvailable(format) == 0 {
                    continue;
                }
                let handle = GetClipboardData(format);
                if handle.is_null() {
                    continue;
                }
                let len = GlobalSize(handle);
                if !(40..=crate::clipboard_dib::MAX_RAW).contains(&len) {
                    continue;
                }
                let ptr = GlobalLock(handle);
                if ptr.is_null() {
                    continue;
                }
                // SAFETY: GlobalSize bounds this locked allocation; no borrowed data escapes.
                let borrowed = std::slice::from_raw_parts(ptr, len);
                let owned = crate::clipboard_dib::validate_dib(borrowed)
                    .ok()
                    .map(|_| borrowed.to_vec());
                GlobalUnlock(handle);
                if let Some(bytes) = owned {
                    CloseClipboard();
                    return crate::clipboard_dib::decode_dib(&bytes)
                        .ok()
                        .and_then(finalize_clipboard_rgba_image);
                }
            }

            CloseClipboard();
            None
        }
    }
    #[cfg(target_os = "macos")]
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let tmp = std::env::temp_dir().join(format!("zalo_mac_clip_{now}.png"));
        let tmp_str = tmp.to_str().unwrap_or("/tmp/zalo_mac_clip.png");
        let script = format!(
            "try\nset img to the clipboard as «class PNGf»\nset fp to open for access (POSIX file \"{tmp_str}\") with write permission\nset eof fp to 0\nwrite img to fp\nclose access fp\nreturn \"ok\"\non error\nreturn \"none\"\nend try"
        );
        if let Ok(out) = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            if s.trim() == "ok" && tmp.exists() {
                if let Ok(bytes) = std::fs::read(&tmp) {
                    let _ = std::fs::remove_file(&tmp);
                    if let Ok(dyn_img) = image::load_from_memory(&bytes) {
                        return finalize_clipboard_rgba_image(dyn_img.to_rgba8());
                    }
                }
                let _ = std::fs::remove_file(&tmp);
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
        if crate::chat::is_zalo_photo_cdn_url(url) {
            return None;
        }
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
    let url = after[..end].trim_end_matches(|c: char| matches!(c, ')' | ']' | ',' | ';' | '>'));
    if url.len() < 10 || crate::chat::is_zalo_photo_cdn_url(url) {
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

    let now_sec = (crate::login::unix_now() as i64) + tz_offset_sec;
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
        let pill_w = (text_w + 28.0).min((avail_w - 24.0).max(40.0));
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
                ui.set_max_width(pill_w);
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("{} {text}", lucide::SPARKLES))
                            .size(12.0)
                            .color(pal.text_secondary),
                    )
                    .wrap(),
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
/// Pure markdown helpers (không cần egui::Ui → test được bằng cargo test).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum MdBlock {
    Code { lang: String, body: String },
    Table { headers: Vec<String>, rows: Vec<Vec<String>> },
    Lines(Vec<String>),
}

/// Tách text thành các block: code fence / bảng / dòng thường.
/// Bảng = header `| a | b |` + dòng separator `| --- | --- |` + ≥1 dòng `|...|`.
/// Bảng hỏng cú pháp (separator sai, số cột lệch) → trả về Lines để vẽ plaintext.
fn split_md_blocks(raw: &str) -> Vec<MdBlock> {
    let mut out: Vec<MdBlock> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    let mut in_code = false;
    let mut code_lang = String::new();
    let mut code_body: Vec<String> = Vec::new();
    let flush = |pending: &mut Vec<String>, out: &mut Vec<MdBlock>| {
        if !pending.is_empty() {
            out.push(MdBlock::Lines(std::mem::take(pending)));
        }
    };
    let lines: Vec<&str> = raw.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_end();
        if in_code {
            if t.trim_start().starts_with("```") {
                in_code = false;
                out.push(MdBlock::Code { lang: std::mem::take(&mut code_lang), body: code_body.join("\n") });
                code_body.clear();
            } else {
                code_body.push(line.to_string());
            }
            i += 1;
            continue;
        }
        if let Some(lang) = t.trim_start().strip_prefix("```") {
            flush(&mut pending, &mut out);
            in_code = true;
            code_lang = lang.trim().to_string();
            i += 1;
            continue;
        }
        if is_table_header(t) && i + 1 < lines.len() && is_table_separator(lines[i + 1].trim_end()) {
            let headers = split_table_row(t);
            let mut rows: Vec<Vec<String>> = Vec::new();
            let mut j = i + 2;
            let mut ok = false;
            while j < lines.len() {
                let rl = lines[j].trim_end();
                if !rl.trim_start().starts_with('|') || !rl.trim_end().ends_with('|') {
                    break;
                }
                let cells = split_table_row(rl);
                if cells.len() != headers.len() {
                    break;
                }
                rows.push(cells);
                ok = true;
                j += 1;
            }
            if ok {
                flush(&mut pending, &mut out);
                out.push(MdBlock::Table { headers, rows });
                i = j;
                continue;
            }
        }
        pending.push(line.to_string());
        i += 1;
    }
    if in_code {
        // Fence không đóng → vẽ nguyên văn như code (vẫn đẹp hơn rớt mất nội dung).
        let mut all = vec![format!("```{}", code_lang)];
        all.extend(code_body);
        pending.extend(all);
        flush(&mut pending, &mut out);
    } else {
        flush(&mut pending, &mut out);
    }
    out
}

fn is_table_header(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.ends_with('|') && t.contains('|') && split_table_row(line).len() >= 2
}

fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    if !t.starts_with('|') || !t.ends_with('|') {
        return false;
    }
    let cells: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).collect();
    if cells.len() < 2 {
        return false;
    }
    cells.iter().all(|c| {
        let c = c.trim_matches(':');
        !c.is_empty() && c.len() >= 3 && c.chars().all(|ch| ch == '-')
    })
}

fn split_table_row(line: &str) -> Vec<String> {
    line.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect()
}

/// Detect list số `1. ` (1-3 số, dấu chấm + space). Trả về (số, indent, nội dung).
fn parse_ordered_item(line: &str) -> Option<(String, usize, String)> {
    let indent = line.bytes().take_while(|&b| b == b' ').count().min(12);
    let t = line.trim_start();
    let dot = t.find(". ")?;
    let (num, _) = t.split_at(dot);
    if !num.is_empty() && num.len() <= 3 && num.chars().all(|c| c.is_ascii_digit()) {
        Some((num.to_string(), indent, t[dot + 2..].to_string()))
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InlineMdRun {
    Plain(String),
    Bold(String),
    Underline(String),
    Strike(String),
    Italic(String),
    Code(String),
    Mention(String),
    Link { label: String, url: String },
}

fn try_consume_zalo_bold_span(rest: &str) -> Option<(usize, &str)> {
    if !rest.starts_with('*') || rest.starts_with("**") {
        return None;
    }
    let after = &rest[1..];
    let end = after.find('*')?;
    if end == 0 || end > 120 {
        return None;
    }
    let inner = &after[..end];
    if inner.starts_with(char::is_whitespace)
        || inner.ends_with(char::is_whitespace)
        || inner.contains('\n')
        || inner.contains("http")
        || inner.contains('@')
    {
        return None;
    }
    Some((1 + end + 1, inner))
}

fn push_plain_run(runs: &mut Vec<InlineMdRun>, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(InlineMdRun::Plain(last)) = runs.last_mut() {
        last.push_str(s);
    } else {
        runs.push(InlineMdRun::Plain(s.to_string()));
    }
}

fn find_next_single_star(s: &str) -> Option<usize> {
    let mut offset = 0;
    while let Some(rel) = s[offset..].find('*') {
        let idx = offset + rel;
        if s.get(idx..idx + 2) != Some("**") {
            return Some(idx);
        }
        offset = idx + 2;
    }
    None
}

fn find_next_italic_underscore(s: &str) -> Option<usize> {
    let mut offset = 0;
    while let Some(rel) = s[offset..].find('_') {
        let idx = offset + rel;
        let is_double = s.get(idx..idx + 2) == Some("__");
        let prev_alnum = s[..idx].chars().next_back().is_some_and(char::is_alphanumeric);
        if !is_double && !prev_alnum {
            return Some(idx);
        }
        offset = idx + if is_double { 2 } else { 1 };
    }
    None
}

fn find_next_mention(s: &str) -> Option<usize> {
    let mut offset = 0;
    while let Some(rel) = s[offset..].find('@') {
        let idx = offset + rel;
        let prev_ident = s[..idx]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '.' || c == '_');
        if !prev_ident {
            return Some(idx);
        }
        offset = idx + 1;
    }
    None
}

fn find_next_bare_url(s: &str) -> Option<usize> {
    let mut best: Option<usize> = None;
    for scheme in ["https://", "http://"] {
        let mut offset = 0;
        while let Some(rel) = s[offset..].find(scheme) {
            let idx = offset + rel;
            let prev_alnum = s[..idx].chars().next_back().is_some_and(char::is_alphanumeric);
            if !prev_alnum {
                best = Some(best.map_or(idx, |b| b.min(idx)));
                break;
            }
            offset = idx + scheme.len();
        }
    }
    best
}

pub(crate) fn parse_inline_md_runs(text: &str, zalo_bold: bool) -> Vec<InlineMdRun> {
    let mut runs: Vec<InlineMdRun> = Vec::new();
    let mut rest = text;

    while !rest.is_empty() {
        let next_bold = rest.find("**");
        let next_under = rest.find("__");
        let next_strike = rest.find("~~");
        let next_code = rest.find('`');
        let next_star = find_next_single_star(rest);
        let next_underscore = find_next_italic_underscore(rest);
        let next_mention = find_next_mention(rest);
        let next_bare_url = find_next_bare_url(rest);
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
            next_star,
            next_underscore,
            next_mention,
            next_bare_url,
            next_link.map(|(lb, _, _)| lb),
        ]
        .into_iter()
        .flatten()
        .min();
        let Some(pos) = min_pos else {
            push_plain_run(&mut runs, rest);
            break;
        };

        if pos > 0 {
            push_plain_run(&mut runs, &rest[..pos]);
            rest = &rest[pos..];
            continue;
        }

        if let Some((0, rb_idx, rp_idx)) = next_link {
            let label = &rest[1..rb_idx];
            let url = &rest[rb_idx + 2..rp_idx];
            runs.push(InlineMdRun::Link {
                label: label.trim_matches('*').trim_matches('`').to_string(),
                url: url.to_string(),
            });
            rest = &rest[rp_idx + 1..];
            continue;
        }

        if next_bare_url == Some(0) {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let raw_token = &rest[..end];
            let clean_url = raw_token.trim_end_matches(|c: char| {
                matches!(c, '.' | ',' | '!' | '?' | ';' | ':' | ')' | ']' | '}' | '>')
            });
            if clean_url.len() > 8 {
                runs.push(InlineMdRun::Link {
                    label: clean_url.to_string(),
                    url: clean_url.to_string(),
                });
                rest = &rest[clean_url.len()..];
                continue;
            }
        }

        // Render @All and @Name mentions as styled chips/links in Zalo Blue
        if rest.starts_with('@') && next_mention == Some(0) {
            let after = &rest[1..];
            let is_all = after.starts_with("All")
                || after.starts_with("all")
                || after.starts_with("ALL")
                || after.starts_with("Tất cả")
                || after.starts_with("tất cả")
                || after.starts_with("Tat ca")
                || after.starts_with("tat ca");
            if is_all {
                let all_len = if after.starts_with("Tất cả") || after.starts_with("tất cả") {
                    "Tất cả".len()
                } else if after.starts_with("Tat ca") || after.starts_with("tat ca") {
                    6
                } else {
                    3
                };
                let next_char = after[all_len..].chars().next();
                if next_char.is_none()
                    || next_char.unwrap().is_whitespace()
                    || matches!(next_char.unwrap(), '.' | ',' | '!' | '?' | ':' | ';')
                {
                    runs.push(InlineMdRun::Mention(rest[..1 + all_len].to_string()));
                    rest = &after[all_len..];
                    continue;
                }
            }

            let mut end_idx = 0;
            let mut word_count = 0;
            let mut prev_is_space = false;
            for (idx, ch) in after.char_indices() {
                if ch == '\n'
                    || ch == '\r'
                    || matches!(
                        ch,
                        ',' | '.'
                            | '!'
                            | '?'
                            | ':'
                            | ';'
                            | '('
                            | ')'
                            | '['
                            | ']'
                            | '"'
                            | '\''
                            | '`'
                            | '<'
                            | '>'
                    )
                {
                    break;
                }
                if ch.is_whitespace() {
                    word_count += 1;
                    if word_count >= 5 || prev_is_space {
                        break;
                    }
                    prev_is_space = true;
                } else {
                    prev_is_space = false;
                }
                end_idx = idx + ch.len_utf8();
            }
            let candidate_name = after[..end_idx].trim_end();
            if !candidate_name.is_empty()
                && candidate_name.chars().next().is_some_and(char::is_alphanumeric)
            {
                let mention_len = 1 + candidate_name.len();
                runs.push(InlineMdRun::Mention(rest[..mention_len].to_string()));
                rest = &rest[mention_len..];
                continue;
            }
        }

        if rest.starts_with("**") {
            let after = &rest[2..];
            if let Some(end_b) = after.find("**") {
                runs.push(InlineMdRun::Bold(after[..end_b].to_string()));
                rest = &after[end_b + 2..];
                continue;
            }
        }

        if rest.starts_with("__") {
            let after = &rest[2..];
            if let Some(end_u) = after.find("__") {
                runs.push(InlineMdRun::Underline(after[..end_u].to_string()));
                rest = &after[end_u + 2..];
                continue;
            }
        }

        if rest.starts_with("~~") {
            let after = &rest[2..];
            if let Some(end_s) = after.find("~~") {
                runs.push(InlineMdRun::Strike(after[..end_s].to_string()));
                rest = &after[end_s + 2..];
                continue;
            }
        }
        if rest.starts_with('*') && !rest.starts_with("**") {
            if zalo_bold {
                if let Some((consumed, inner)) = try_consume_zalo_bold_span(rest) {
                    runs.push(InlineMdRun::Bold(inner.to_string()));
                    rest = &rest[consumed..];
                    continue;
                }
            }
            let after = &rest[1..];
            if let Some(end_i) = after.find('*') {
                if end_i > 0 && !zalo_bold {
                    runs.push(InlineMdRun::Italic(after[..end_i].to_string()));
                    rest = &after[end_i + 1..];
                    continue;
                }
            }
        }
        if rest.starts_with('_') && !rest.starts_with("__") && next_underscore == Some(0) {
            let after = &rest[1..];
            if let Some(end_i) = after.find('_') {
                let inner = &after[..end_i];
                if end_i > 0
                    && !inner.starts_with(char::is_whitespace)
                    && !inner.ends_with(char::is_whitespace)
                    && after.get(end_i..end_i + 2) != Some("__")
                {
                    runs.push(InlineMdRun::Italic(inner.to_string()));
                    rest = &after[end_i + 1..];
                    continue;
                }
            }
        }

        if rest.starts_with('`') {
            let after = &rest[1..];
            if let Some(end_c) = after.find('`') {
                runs.push(InlineMdRun::Code(after[..end_c].to_string()));
                rest = &after[end_c + 1..];
                continue;
            }
        }

        let mut chars = rest.chars();
        if let Some(ch) = chars.next() {
            let mut buf = [0u8; 4];
            push_plain_run(&mut runs, ch.encode_utf8(&mut buf));
            rest = chars.as_str();
        } else {
            break;
        }
    }

    runs
}

fn append_md_run_to_job(
    job: &mut egui::text::LayoutJob,
    run: &InlineMdRun,
    base_size: f32,
    base_color: Color32,
    pal: &ThemePalette,
) {
    match run {
        InlineMdRun::Plain(s) => job.append(
            s,
            0.0,
            egui::TextFormat {
                font_id: FontId::proportional(base_size),
                color: base_color,
                ..Default::default()
            },
        ),
        InlineMdRun::Bold(s) => job.append(
            s,
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
        ),
        InlineMdRun::Underline(s) => job.append(
            s,
            0.0,
            egui::TextFormat {
                font_id: FontId::proportional(base_size),
                color: base_color,
                underline: Stroke::new(1.0_f32, base_color),
                ..Default::default()
            },
        ),
        InlineMdRun::Strike(s) => job.append(
            s,
            0.0,
            egui::TextFormat {
                font_id: FontId::proportional(base_size),
                color: base_color.linear_multiply(0.75),
                strikethrough: Stroke::new(1.0_f32, base_color),
                ..Default::default()
            },
        ),
        InlineMdRun::Italic(s) => job.append(
            s,
            0.0,
            egui::TextFormat {
                font_id: FontId::proportional(base_size),
                color: base_color,
                italics: true,
                ..Default::default()
            },
        ),
        InlineMdRun::Code(s) => job.append(
            s,
            0.0,
            egui::TextFormat {
                font_id: FontId::monospace((base_size - 0.8).max(11.0)),
                color: pal.link_text,
                background: pal.code_bg,
                ..Default::default()
            },
        ),
        InlineMdRun::Mention(s) => job.append(
            s,
            0.0,
            egui::TextFormat {
                font_id: FontId::proportional(base_size),
                color: pal.link_text,
                background: pal.primary.linear_multiply(0.14),
                underline: Stroke::new(1.0_f32, pal.link_text.linear_multiply(0.6)),
                ..Default::default()
            },
        ),
        InlineMdRun::Link { label, .. } => job.append(
            label,
            0.0,
            egui::TextFormat {
                font_id: FontId::proportional(base_size),
                color: pal.link_text,
                underline: Stroke::new(1.0_f32, pal.link_text),
                ..Default::default()
            },
        ),
    }
}

#[allow(dead_code)]
fn render_markdown_inline_rich(
    ui: &mut egui::Ui,
    text: &str,
    base_size: f32,
    base_color: Color32,
    pal: &ThemePalette,
) {
    render_markdown_inline_rich_opts(ui, text, base_size, base_color, pal, true)
}

fn render_markdown_inline_rich_opts(
    ui: &mut egui::Ui,
    text: &str,
    base_size: f32,
    base_color: Color32,
    pal: &ThemePalette,
    zalo_bold: bool,
) {
    let runs = parse_inline_md_runs(text, zalo_bold);
    let links: Vec<&str> = runs
        .iter()
        .filter_map(|r| match r {
            InlineMdRun::Link { url, .. }
                if url.starts_with("http://") || url.starts_with("https://") =>
            {
                Some(url.as_str())
            }
            _ => None,
        })
        .collect();

    if links.len() <= 1 {
        let mut job = egui::text::LayoutJob::default();
        for run in &runs {
            append_md_run_to_job(&mut job, run, base_size, base_color, pal);
        }
        if let Some(&url) = links.first() {
            let resp = ui.add(egui::Label::new(job).wrap().sense(Sense::click()));
            if resp
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(url)
                .clicked()
            {
                let _ = open_external_url(url);
            }
        } else {
            ui.add(egui::Label::new(job).wrap());
        }
        return;
    }

    // Multiple clickable links on the same line: flush preceding styled runs and give each link its own click target.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut chunk = egui::text::LayoutJob::default();
        for run in &runs {
            if let InlineMdRun::Link { label, url } = run {
                if !chunk.is_empty() {
                    ui.add(egui::Label::new(std::mem::take(&mut chunk)).wrap());
                }
                let mut link_job = egui::text::LayoutJob::default();
                append_md_run_to_job(
                    &mut link_job,
                    &InlineMdRun::Link {
                        label: label.clone(),
                        url: url.clone(),
                    },
                    base_size,
                    base_color,
                    pal,
                );
                let resp = ui.add(egui::Label::new(link_job).wrap().sense(Sense::click()));
                if resp
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text(url.as_str())
                    .clicked()
                {
                    let _ = open_external_url(url);
                }
            } else {
                append_md_run_to_job(&mut chunk, run, base_size, base_color, pal);
            }
        }
        if !chunk.is_empty() {
            ui.add(egui::Label::new(chunk).wrap());
        }
    });
}

#[allow(dead_code)]
pub(super) fn render_chat_markdown_block(
    ui: &mut egui::Ui,
    raw: &str,
    text_col: Color32,
    pal: &ThemePalette,
) {
    render_chat_markdown_block_opts(ui, raw, text_col, pal, true, true)
}

/// Bản có toggle: `enabled=false` → plaintext nguyên văn (không parse gì);
/// `zalo_bold` điều khiển `*đậm*` kiểu Zalo (xem consume_zalo_bold).
pub(super) fn render_chat_markdown_block_opts(
    ui: &mut egui::Ui,
    raw: &str,
    text_col: Color32,
    pal: &ThemePalette,
    enabled: bool,
    zalo_bold: bool,
) {
    if !enabled {
        ui.add(egui::Label::new(RichText::new(raw).size(14.0).color(text_col)).wrap());
        return;
    }
    for block in split_md_blocks(raw) {
        match block {
            MdBlock::Code { lang, body } => render_code_block(ui, &lang, &body, pal),
            MdBlock::Table { headers, rows } => render_md_table(ui, &headers, &rows, text_col, pal, zalo_bold),
            MdBlock::Lines(lines) => {
                for line in &lines {
                    let trimmed = line.trim_end();
                    let t_start = trimmed.trim_start();
                    if let Some(h3) = t_start.strip_prefix("### ") {
                        render_markdown_inline_rich_opts(ui, h3, 15.0, text_col, pal, zalo_bold);
                    } else if let Some(h2) = t_start.strip_prefix("## ") {
                        render_markdown_inline_rich_opts(ui, h2, 16.0, text_col, pal, zalo_bold);
                    } else if let Some(h1) = t_start.strip_prefix("# ") {
                        render_markdown_inline_rich_opts(ui, h1, 17.0, text_col, pal, zalo_bold);
                    } else if let Some(quote) = t_start.strip_prefix("> ") {
                        ui.horizontal(|ui| {
                            let (bar_r, _) = ui.allocate_exact_size(Vec2::new(3.0, 16.0), Sense::hover());
                            ui.painter().rect_filled(bar_r, Rounding::same(1.5), pal.primary_hover);
                            render_markdown_inline_rich_opts(ui, quote, 13.8, text_col.linear_multiply(0.88), pal, zalo_bold);
                        });
                    } else if let Some((num, indent, body)) = parse_ordered_item(trimmed) {
                        ui.horizontal(|ui| {
                            ui.add_space(indent as f32 * 4.0);
                            ui.label(RichText::new(format!("{num}.")).size(14.0).color(text_col));
                            render_markdown_inline_rich_opts(ui, &body, 14.0, text_col, pal, zalo_bold);
                        });
                    } else if let Some(bullet) = t_start.strip_prefix("- ").or_else(|| t_start.strip_prefix("* ")) {
                        let bullet_line = format!("• {bullet}");
                        render_markdown_inline_rich_opts(ui, &bullet_line, 14.0, text_col, pal, zalo_bold);
                    } else {
                        render_markdown_inline_rich_opts(ui, trimmed, 14.0, text_col, pal, zalo_bold);
                    }
                }
            }
        }
    }
}

fn render_code_block(ui: &mut egui::Ui, lang: &str, body: &str, pal: &ThemePalette) {
    let title = if lang.is_empty() { "code".to_string() } else { lang.to_string() };
    egui::Frame::none()
        .fill(pal.bg_input)
        .stroke(Stroke::new(1.0_f32, pal.border))
        .rounding(Rounding::same(6.0))
        .inner_margin(Margin::symmetric(10.0, 7.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).size(11.5).color(pal.text_muted).monospace());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("📋 Copy").on_hover_text("Sao chép code").clicked() {
                        ui.ctx().copy_text(body.to_string());
                    }
                });
            });
            ui.add_space(2.0);
            ui.add(egui::Label::new(RichText::new(body).font(FontId::monospace(12.6)).color(pal.primary_hover)).wrap());
        });
}

fn render_md_table(ui: &mut egui::Ui, headers: &[String], rows: &[Vec<String>], text_col: Color32, pal: &ThemePalette, zalo_bold: bool) {
    let table_id = ui.next_auto_id().with(("md_table", headers.len(), rows.len()));
    egui::Frame::none()
        .fill(pal.bg_input)
        .stroke(Stroke::new(1.0_f32, pal.border))
        .rounding(Rounding::same(6.0))
        .inner_margin(Margin::symmetric(8.0, 6.0))
        .show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .id_salt(table_id.with("scroll"))
                .show(ui, |ui| {
                    egui::Grid::new(table_id)
                        .striped(true)
                        .spacing([10.0, 4.0])
                        .show(ui, |ui| {
                            for h in headers {
                                ui.label(RichText::new(h).size(13.5).color(text_col).strong());
                            }
                            ui.end_row();
                            for row in rows {
                                for cell in row {
                                    render_markdown_inline_rich_opts(
                                        ui, cell, 13.5, text_col, pal, zalo_bold,
                                    );
                                }
                                ui.end_row();
                            }
                        });
                });
        });
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

/// Cách render markdown của một bubble (truyền từ settings → ít đổi callsite nhất).
#[derive(Debug, Clone, Copy)]
pub(crate) struct MdOptions {
    pub enabled: bool,
    pub zalo_bold: bool,
}

impl Default for MdOptions {
    fn default() -> Self {
        Self { enabled: true, zalo_bold: true }
    }
}

pub(super) fn render_message_body(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    thread_name: &str,
    pal: &ThemePalette,
    outgoing: bool,
    max_text_width: f32,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
    failed_stickers: &HashMap<String, (u8, std::time::Instant)>,
    preview: Option<&str>,
    preview_loading: bool,
    preview_error: Option<&str>,
    md: MdOptions,
) -> Option<BubbleAction> {
    let mut body_action = None;
    let (text_col, meta_col) = if outgoing {
        (pal.bubble_out_text, pal.bubble_out_meta)
    } else {
        (pal.bubble_in_text, pal.bubble_in_meta)
    };

    let trimmed_text = msg.text.trim();
    if trimmed_text == crate::chat::VANISH_EXPIRED_TEXT {
        ui.label(
            RichText::new(format!("{} {}", lucide::BAN, crate::chat::VANISH_EXPIRED_TEXT))
                .color(meta_col)
                .italics()
                .size(13.8),
        );
        return None;
    }
    if trimmed_text == "[Tin nhắn đã thu hồi]" {
        ui.label(
            RichText::new(format!("{} Tin nhắn đã được thu hồi", lucide::BAN))
                .color(meta_col)
                .italics()
                .size(13.8),
        );
        return None;
    }
    let is_e2ee = msg
        .history_source
        .as_ref()
        .and_then(|v| v.get("isE2EE"))
        .and_then(|x| x.as_i64())
        == Some(1)
        || trimmed_text.contains("[Mã hóa đầu cuối]");
    if is_e2ee {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("{} Mã hóa đầu cuối", lucide::LOCK))
                    .size(11.0)
                    .color(meta_col)
                    .strong(),
            );
        });
        ui.add_space(2.0);
    }
    let is_anti_recalled = trimmed_text.starts_with("[Đã thu hồi]");
    if is_anti_recalled {
        egui::Frame::none()
            .fill(pal.warning.linear_multiply(0.14))
            .rounding(Rounding::same(6.0))
            .inner_margin(Margin::symmetric(6.0, 2.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("{} Đã thu hồi (giữ lại)", lucide::SHIELD_CHECK))
                        .size(11.0)
                        .color(pal.warning)
                        .strong(),
                );
            });
        ui.add_space(4.0);
    }

    let sender_label = if msg.is_outgoing {
        "Bạn".to_string()
    } else if !msg.sender.is_empty() && msg.sender != "0" {
        msg.sender.clone()
    } else {
        thread_name.to_string()
    };

    // Dedicated inline Photo Card when the message is an uploaded/received Zalo photo (#72)
    let is_zalo_photo = is_zalo_photo_message(msg);
    if is_zalo_photo {
        if let Some(thumb_url) = photo_primary_url(msg) {
            let key = format!("thumb_{}", crate::crypto::md5_hex(thumb_url));
            crate::gui::queue_sticker_fetch(
                sticker_textures,
                loading_stickers,
                failed_stickers,
                key.clone(),
                thumb_url.to_string(),
            );
            let open_target = msg
                .link_url
                .as_deref()
                .filter(|u| !u.is_empty() && crate::chat::is_safe_external_image_url(u))
                .or(Some(thumb_url).filter(|u| crate::chat::is_safe_external_image_url(u)))
                .unwrap_or(thumb_url)
                .to_string();
            let title_label = msg
                .link_title
                .as_deref()
                .map(str::trim)
                .filter(|t| {
                    !t.is_empty()
                        && !t.starts_with("http://")
                        && !t.starts_with("https://")
                        && !t.ends_with(".zdn.vn")
                        && !t.ends_with(".zadn.vn")
                        && !t.ends_with(".dlfl.vn")
                        && !t.contains("zpcloud")
                        && !t.contains("zpg-r")
                        && !t.contains("photo-stal")
                        && !t.contains("photo-zmp")
                })
                .unwrap_or("Hình ảnh")
                .to_string();

            if let Some(tex) = sticker_textures.get(&key) {
                let [tw, th] = tex.size();
                if tw <= 2 || th <= 2 {
                    // Expired or empty tombstone image on Zalo CDN (e.g. 1x1 jpeg)
                    let ph_w = max_text_width.min(260.0).max(110.0);
                    let ph_frame = egui::Frame::none()
                        .fill(text_col.linear_multiply(0.05))
                        .stroke(Stroke::new(1.0_f32, pal.border))
                        .rounding(Rounding::same(12.0))
                        .inner_margin(Margin::symmetric(16.0, 14.0))
                        .show(ui, |ui| {
                            ui.set_min_width(ph_w);
                            ui.vertical_centered(|ui| {
                                let (ir, _) =
                                    ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                                draw_icon_image(
                                    ui.painter(),
                                    ir.center(),
                                    22.0,
                                    text_col.linear_multiply(0.4),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("Ảnh đã hết hạn trên Zalo")
                                        .size(12.5)
                                        .color(text_col.linear_multiply(0.6)),
                                );
                            });
                        });
                    if ui
                        .interact(
                            ph_frame.response.rect,
                            Id::new(("photo_exp_click", &msg.id, msg.ts)),
                            Sense::click(),
                        )
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text("Bấm để thử mở liên kết ảnh gốc")
                        .clicked()
                    {
                        let _ = open_external_url(&open_target);
                    }
                } else {
                    let tw_f = tw as f32;
                    let th_f = th as f32;
                    let max_w = max_text_width.min(360.0).max(110.0);
                    let max_h = 340.0_f32;
                    let scale = (max_w / tw_f).min(max_h / th_f).min(1.0);
                    let disp_w = (tw_f * scale).clamp(max_w.min(140.0), max_w);
                    let disp_h = (th_f * scale).clamp(90.0, max_h);
                    let img = egui::Image::new(tex)
                        .fit_to_exact_size(Vec2::new(disp_w, disp_h))
                        .rounding(Rounding::same(12.0))
                        .sense(Sense::click());
                    let resp = ui
                        .add(img)
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text("Bấm để xem ảnh phóng to trong ứng dụng");
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
                        let pill_r =
                            Rect::from_center_size(resp.rect.center(), Vec2::new(108.0, 26.0));
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
                }
            } else {
                // Loading / Placeholder frame with visual image icon and loading feedback
                let disp_w = max_text_width.min(280.0).max(110.0);
                let disp_h = 150.0_f32;
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(disp_w, disp_h), Sense::click());
                let is_hover = resp.hovered();
                let bg_col = if is_hover {
                    text_col.linear_multiply(0.09)
                } else {
                    text_col.linear_multiply(0.05)
                };
                let stroke_col = if is_hover { pal.primary } else { pal.border };
                ui.painter().rect_filled(rect, Rounding::same(12.0), bg_col);
                ui.painter().rect_stroke(
                    rect,
                    Rounding::same(12.0),
                    Stroke::new(1.0_f32, stroke_col),
                );

                let icon_center = rect.center() - Vec2::new(0.0, 14.0);
                draw_icon_image(
                    ui.painter(),
                    icon_center,
                    26.0,
                    text_col.linear_multiply(0.45),
                );
                let label_text = if title_label != "Hình ảnh" {
                    title_label.clone()
                } else {
                    "Đang tải hình ảnh...".to_string()
                };
                ui.painter().text(
                    rect.center() + Vec2::new(0.0, 20.0),
                    egui::Align2::CENTER_CENTER,
                    label_text,
                    FontId::proportional(12.5),
                    text_col.linear_multiply(0.7),
                );

                if resp
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("Đang tải ảnh · Bấm để xem ảnh phóng to")
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
                render_chat_markdown_block_opts(ui, cap, text_col, pal, md.enabled, md.zalo_bold);
            }
            return body_action;
        }
    }
    // Classified File / Video / Voice / Doc Attachment Card (#74 follow-up)
    if let Some(kind) = classify_attachment(msg) {
        let url_str = msg.link_url.as_deref().unwrap_or_default();
        let resolved_file_name = crate::chat::resolve_attachment_file_name(
            msg.link_title.as_deref(),
            trimmed_text,
            url_str,
        );
        return render_attachment_card(
            ui,
            msg,
            kind,
            &resolved_file_name,
            text_col,
            pal,
            max_text_width,
            sticker_textures,
            loading_stickers,
            failed_stickers,
            preview,
            preview_loading,
            preview_error,
        );
    }

    // Compact structured cards for Location, Contact, Poll/Vote, and Album messages.
    if msg.kind == crate::message_model::MessageKind::Location
        || trimmed_text.starts_with("[Vị trí]")
    {
        egui::Frame::none()
            .fill(pal.bg_elevated)
            .stroke(Stroke::new(1.0_f32, pal.border))
            .rounding(Rounding::same(8.0))
            .inner_margin(Margin::symmetric(10.0, 7.0))
            .show(ui, |ui| {
                ui.set_max_width(max_text_width);
                ui.label(
                    RichText::new("📍 Vị trí đã chia sẻ")
                        .size(13.0)
                        .color(text_col)
                        .strong(),
                );
                let detail = trimmed_text.strip_prefix("[Vị trí]").unwrap_or(trimmed_text).trim();
                if !detail.is_empty() {
                    ui.add(egui::Label::new(RichText::new(detail).size(12.2).color(meta_col)).wrap());
                }
            });
        return None;
    }
    if msg.kind == crate::message_model::MessageKind::Contact
        || trimmed_text.starts_with("[Danh thiếp]")
    {
        egui::Frame::none()
            .fill(pal.bg_elevated)
            .stroke(Stroke::new(1.0_f32, pal.border))
            .rounding(Rounding::same(8.0))
            .inner_margin(Margin::symmetric(10.0, 7.0))
            .show(ui, |ui| {
                ui.set_max_width(max_text_width);
                ui.label(
                    RichText::new(format!("{} Danh thiếp Zalo", lucide::USER))
                        .size(13.0)
                        .color(text_col)
                        .strong(),
                );
                let detail = trimmed_text
                    .strip_prefix("[Danh thiếp]")
                    .unwrap_or(trimmed_text)
                    .trim();
                if !detail.is_empty() {
                    ui.add(egui::Label::new(RichText::new(detail).size(12.2).color(meta_col)).wrap());
                }
            });
        return None;
    }
    if trimmed_text.starts_with("[Khảo sát]")
        || trimmed_text.starts_with("[Bình chọn]")
        || trimmed_text.starts_with("[Album]")
    {
        let title_tag = if trimmed_text.starts_with("[Album]") {
            format!("{} Album ảnh", lucide::IMAGE)
        } else {
            "📊 Bình chọn nhóm".to_string()
        };
        let rest = trimmed_text
            .split_once(']')
            .map(|(_, r)| r.trim())
            .unwrap_or(trimmed_text);
        egui::Frame::none()
            .fill(pal.bg_elevated)
            .stroke(Stroke::new(1.0_f32, pal.border))
            .rounding(Rounding::same(8.0))
            .inner_margin(Margin::symmetric(10.0, 7.0))
            .show(ui, |ui| {
                ui.set_max_width(max_text_width);
                ui.label(RichText::new(title_tag).size(13.0).color(text_col).strong());
                if !rest.is_empty() {
                    ui.add(egui::Label::new(RichText::new(rest).size(12.5).color(meta_col)).wrap());
                }
            });
        return None;
    }

    let Some((caption, url, domain)) = extract_message_link_view(msg) else {
        let display_text = if let Some(stripped) = msg.text.strip_prefix("[Đã thu hồi] ") {
            stripped
        } else {
            &msg.text
        };
        render_chat_markdown_block_opts(ui, display_text, text_col, pal, md.enabled, md.zalo_bold);
        return None;
    };

    let link_col = pal.link_text;

    // Auto-enrich link metadata if title or thumbnail is missing/expired
    let needs_meta_enrich = msg.link_title.is_none()
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
        render_chat_markdown_block_opts(ui, &caption, text_col, pal, md.enabled, md.zalo_bold);
        ui.add_space(4.0);
    }

    // 2. Optional compact thumbnail directly inside the bubble (no nested card box)
    if let Some(thumb_url) = msg.link_thumb.as_deref().filter(|u| !u.is_empty()) {
        let key = format!("thumb_{}", crate::crypto::md5_hex(thumb_url));
        if thumb_url.starts_with("http://") || thumb_url.starts_with("https://") {
            crate::gui::queue_sticker_fetch(
                sticker_textures,
                loading_stickers,
                failed_stickers,
                key.clone(),
                thumb_url.to_string(),
            );
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
                .rounding(Rounding::same(6.0))
                .sense(Sense::click());
            if ui
                .add(img)
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(&url)
                .clicked()
            {
                let _ = open_external_url(&url);
            }
            ui.add_space(5.0);
        }
    }

    // 3. Bold Title (exact Zalo PC style: no globe icon, no vertical accent bar, no nested border)
    let header_title = msg
        .link_title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty() && *t != domain)
        .unwrap_or_else(|| {
            url.strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
                .unwrap_or(&url)
        });
    let title_resp = ui
        .add(
            egui::Label::new(
                RichText::new(truncate_chars(header_title, 95))
                    .size(13.8)
                    .color(text_col)
                    .strong(),
            )
            .wrap()
            .sense(Sense::click()),
        )
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(&url);
    if title_resp.clicked() {
        let _ = open_external_url(&url);
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
            let _ = open_external_url(&url);
        }
    }

    // 5. URL rút gọn click được + domain (#111: trước đây chỉ hiện domain,
    //    user không thấy/không copy được link gốc).
    ui.add_space(2.5);
    let url_line = format!("{} · {}", clean_display_url(&url, 60), domain);
    let dom_resp = ui
        .add(
            egui::Label::new(RichText::new(&url_line).size(12.6).color(link_col))
                .wrap()
                .sense(Sense::click()),
        )
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(&url);
    if dom_resp.clicked() {
        let _ = open_external_url(&url);
    }
    dom_resp.context_menu(|ui| {
        if ui.button("📋 Sao chép liên kết").clicked() {
            ui.ctx().copy_text(url.clone());
            ui.close_menu();
        }
    });

    body_action
}

/// Classified attachment kind for the file/video/voice/doc bubble (#74 follow-up).
/// Pure filename + `link_src`/text-prefix heuristics; unknown extensions stay `GenericFile`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AttachmentKind {
    Video,
    Voice,
    Audio,
    Pdf,
    Word,
    Excel,
    PowerPoint,
    Archive,
    Text,
    GenericFile,
}

pub(super) fn classify_attachment(msg: &crate::chat::ChatMessage) -> Option<AttachmentKind> {
    if !is_zalo_file_or_video_message(msg) {
        return None;
    }
    let trimmed = msg.text.trim();
    let is_video = msg.kind == crate::message_model::MessageKind::Video
        || msg.link_src.as_deref() == Some("Video Zalo")
        || trimmed.starts_with("[Video]")
        || msg.link_url.as_deref().is_some_and(|u| {
            let l = u.to_ascii_lowercase();
            l.contains("video") || l.contains(".dlmd.me")
        });
    if is_video {
        return Some(AttachmentKind::Video);
    }
    if msg.kind == crate::message_model::MessageKind::Voice
        || trimmed.starts_with("[Tin nhắn thoại]")
    {
        return Some(AttachmentKind::Voice);
    }
    let name = crate::chat::resolve_attachment_file_name(
        msg.link_title.as_deref(),
        trimmed,
        msg.link_url.as_deref().unwrap_or_default(),
    );
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if ext.is_empty() || ext == name.to_ascii_lowercase() {
        return Some(AttachmentKind::GenericFile);
    }
    Some(match ext.as_str() {
        "mp4" | "mov" | "mkv" | "avi" | "webm" => AttachmentKind::Video,
        "amr" | "awb" | "opus" => AttachmentKind::Voice,
        "mp3" | "m4a" | "wav" | "ogg" | "flac" | "aac" => AttachmentKind::Audio,
        "pdf" => AttachmentKind::Pdf,
        "doc" | "docx" | "odt" | "rtf" => AttachmentKind::Word,
        "xls" | "xlsx" | "csv" | "ods" => AttachmentKind::Excel,
        "ppt" | "pptx" | "odp" | "key" => AttachmentKind::PowerPoint,
        "zip" | "rar" | "7z" | "tar" | "gz" => AttachmentKind::Archive,
        "txt" | "md" | "log" => AttachmentKind::Text,
        _ => AttachmentKind::GenericFile,
    })
}

pub(super) fn attachment_kind_label(kind: AttachmentKind) -> (&'static str, &'static str) {
    match kind {
        AttachmentKind::Video => (crate::gui::lucide::FILM, "Video"),
        AttachmentKind::Voice => (crate::gui::lucide::MIC, "Tin nhắn thoại"),
        AttachmentKind::Audio => (crate::gui::lucide::MIC, "Âm thanh"),
        AttachmentKind::Pdf => (crate::gui::lucide::FILE_TEXT, "Tài liệu PDF"),
        AttachmentKind::Word => (crate::gui::lucide::FILE_TEXT, "Tài liệu Word"),
        AttachmentKind::Excel => (crate::gui::lucide::FILE_TEXT, "Bảng tính Excel"),
        AttachmentKind::PowerPoint => (crate::gui::lucide::FILE_TEXT, "Bài thuyết trình"),
        AttachmentKind::Archive => (crate::gui::lucide::FILE_TEXT, "Tập tin nén"),
        AttachmentKind::Text => (crate::gui::lucide::FILE_TEXT, "Tập tin văn bản"),
        AttachmentKind::GenericFile => (crate::gui::lucide::PAPERCLIP, "Tập tin"),
    }
}

/// Cache key for a video-bubble thumbnail derived from the attachment URL.
pub(super) fn video_thumb_key(url: &str) -> String {
    format!("vid_{}", crate::crypto::md5_hex(url))
}
/// Render one classified attachment bubble: video w/ thumbnail, voice w/ play, doc/file card.
#[allow(clippy::too_many_arguments)]
fn render_attachment_card(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    kind: AttachmentKind,
    file_name: &str,
    text_col: Color32,
    pal: &ThemePalette,
    max_text_width: f32,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
    failed_stickers: &HashMap<String, (u8, std::time::Instant)>,
    preview: Option<&str>,
    preview_loading: bool,
    preview_error: Option<&str>,
) -> Option<BubbleAction> {
    match kind {
        AttachmentKind::Video => render_video_card(
            ui,
            msg,
            file_name,
            text_col,
            pal,
            max_text_width,
            sticker_textures,
            loading_stickers,
            failed_stickers,
        ),
        AttachmentKind::Voice | AttachmentKind::Audio => {
            render_voice_card(ui, msg, kind, file_name, text_col, pal, max_text_width)
        }
        _ => render_doc_card(
            ui,
            msg,
            kind,
            file_name,
            text_col,
            pal,
            preview,
            preview_loading,
            preview_error,
        ),
    }
}

pub(crate) fn compute_video_thumb_size(tw: usize, th: usize, max_text_width: f32) -> (f32, f32) {
    let tw_f = (tw.max(1)) as f32;
    let th_f = (th.max(1)) as f32;
    if th_f > tw_f {
        // Portrait / vertical video (e.g. 9:16 or 3:4)
        let max_h = 260.0_f32;
        let max_w = max_text_width.min(220.0);
        let scale = (max_w / tw_f).min(max_h / th_f);
        let w = (tw_f * scale).clamp(140.0, max_w).ceil();
        let h = (th_f * scale).clamp(160.0, max_h).ceil();
        (w, h)
    } else {
        // Landscape / horizontal video (e.g. 16:9 or 4:3)
        let max_w = max_text_width.min(280.0);
        let max_h = 180.0_f32;
        let scale = (max_w / tw_f).min(max_h / th_f);
        let w = (tw_f * scale).clamp(160.0, max_w).ceil();
        let h = (th_f * scale).clamp(100.0, max_h).ceil();
        (w, h)
    }
}

fn render_video_thumb_block(
    ui: &mut egui::Ui,
    tex: &TextureHandle,
    key: &str,
    msg: &ChatMessage,
    title_str: &str,
    text_col: Color32,
    max_text_width: f32,
) -> Option<BubbleAction> {
    let [tw, th] = tex.size();
    if tw <= 2 || th <= 2 {
        return None;
    }
    let (thumb_w, img_h) = compute_video_thumb_size(tw, th, max_text_width);
    let img_resp = ui
        .add(
            egui::Image::new(tex)
                .fit_to_exact_size(Vec2::new(thumb_w, img_h))
                .rounding(Rounding::same(8.0))
                .sense(Sense::click()),
        )
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Bấm để xem video");
    let pill_r = egui::Rect::from_center_size(img_resp.rect.center(), Vec2::new(46.0, 46.0));
    ui.painter()
        .circle_filled(pill_r.center(), 23.0, Color32::from_black_alpha(140));
    draw_icon_play(
        ui.painter(),
        pill_r.center() + Vec2::new(2.0, 0.0),
        20.0,
        Color32::WHITE,
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new(truncate_chars(title_str, 60))
            .size(12.5)
            .color(text_col),
    );
    if img_resp.clicked() {
        if let Some(url) = msg.link_url.as_deref().filter(|u| !u.is_empty()) {
            return Some(BubbleAction::OpenVideoPlayer {
                url: url.to_string(),
                title: title_str.to_string(),
                sender: msg.sender.clone(),
                time: msg.time.clone(),
                thumb_key: Some(key.to_string()),
            });
        }
    }
    None
}

/// Video bubble: inline thumbnail (Zalo `link_thumb`, photo CDN, or ffmpeg frame) + play overlay.
/// Missing ffmpeg or missing thumbnail falls back to the plain clickable card, never a crash.
#[allow(clippy::too_many_arguments)]
fn render_video_card(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    file_name: &str,
    text_col: Color32,
    pal: &ThemePalette,
    max_text_width: f32,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
    failed_stickers: &HashMap<String, (u8, std::time::Instant)>,
) -> Option<BubbleAction> {
    let raw_title = msg
        .link_title
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(file_name);
    let title_str = if raw_title.len() > 20 && !raw_title.contains(' ') && !raw_title.contains('.') {
        "Video".to_string()
    } else {
        raw_title.to_string()
    };
    let thumb_url = msg
        .link_thumb
        .as_deref()
        .filter(|u| {
            !u.is_empty()
                && (u.starts_with("http://") || u.starts_with("https://"))
                && !u.to_ascii_lowercase().contains("/renewmedia")
        })
        .or_else(|| {
            msg.link_url
                .as_deref()
                .filter(|u| !u.is_empty() && crate::chat::is_zalo_photo_cdn_url(u))
        });
    if let Some(thumb_url) = thumb_url {
        let key = format!("thumb_{}", crate::crypto::md5_hex(thumb_url));
        crate::gui::queue_sticker_fetch(
            sticker_textures,
            loading_stickers,
            failed_stickers,
            key.clone(),
            thumb_url.to_string(),
        );
        if let Some(tex) = sticker_textures.get(&key) {
            if let Some(action) = render_video_thumb_block(
                ui,
                tex,
                &key,
                msg,
                &title_str,
                text_col,
                max_text_width,
            ) {
                return Some(action);
            }
            return None;
        }
    }
    if let Some(url) = msg.link_url.as_deref().filter(|u| !u.is_empty()) {
        let key = video_thumb_key(url);
        if !sticker_textures.contains_key(&key) && loading_stickers.insert(key.clone()) {
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::FetchVideoThumbnail {
                    key: key.clone(),
                    url: url.to_string(),
                });
            }
        }
        if let Some(tex) = sticker_textures.get(&key) {
            if let Some(action) = render_video_thumb_block(
                ui,
                tex,
                &key,
                msg,
                &title_str,
                text_col,
                max_text_width,
            ) {
                return Some(action);
            }
            return None;
        }
    }
    render_video_fallback_card(ui, msg, &title_str, file_name, text_col, pal)
}

fn render_video_fallback_card(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    title_str: &str,
    _file_name: &str,
    text_col: Color32,
    pal: &ThemePalette,
) -> Option<BubbleAction> {
    let link_col = if pal.is_dark {
        Color32::from_rgb(97, 175, 239)
    } else {
        pal.primary
    };
    let (icon, kind_label) = attachment_kind_label(AttachmentKind::Video);
    ui.label(
        RichText::new(format!("{icon}  {}", truncate_chars(title_str, 48)))
            .size(13.8)
            .color(text_col)
            .strong(),
    );
    ui.add_space(2.0);
    let sub_resp = ui
        .add(
            egui::Label::new(
                RichText::new(format!("{kind_label} · Bấm để xem"))
                    .size(12.5)
                    .color(link_col),
            )
            .sense(Sense::click()),
        )
        .on_hover_cursor(CursorIcon::PointingHand);
    if sub_resp.clicked() {
        if let Some(url) = msg.link_url.as_deref().filter(|u| !u.is_empty()) {
            return Some(BubbleAction::OpenVideoPlayer {
                url: url.to_string(),
                title: title_str.to_string(),
                sender: msg.sender.clone(),
                time: msg.time.clone(),
                thumb_key: None,
            });
        }
    }
    None
}

/// Voice bubble: play button + duration. Playback downloads bytes on the worker
/// then opens them in the OS default player (no new audio dep).
fn render_voice_card(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    kind: AttachmentKind,
    file_name: &str,
    text_col: Color32,
    pal: &ThemePalette,
    max_text_width: f32,
) -> Option<BubbleAction> {
    let (icon, kind_label) = attachment_kind_label(kind);
    let duration = voice_duration_secs(msg);
    let mut action = None;
    ui.horizontal(|ui| {
        let play_resp = ui
            .add(
                egui::Button::new("")
                    .min_size(Vec2::new(34.0, 34.0))
                    .rounding(Rounding::same(17.0))
                    .fill(pal.primary),
            )
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Bấm để nghe tin nhắn thoại");
        draw_icon_play(
            ui.painter(),
            play_resp.rect.center() + Vec2::new(1.5, 0.0),
            15.0,
            pal.primary_fg,
        );
        if play_resp.clicked() {
            if let Some(url) = msg.link_url.as_deref().filter(|u| !u.is_empty()) {
                action = Some(BubbleAction::PlayVoice {
                    url: url.to_string(),
                    file_name: file_name.to_string(),
                    msg_id: msg.id.clone(),
                });
            }
        }
        ui.vertical(|ui| {
            ui.label(
                RichText::new(format!("{icon}  {kind_label}"))
                    .size(12.8)
                    .color(text_col)
                    .strong(),
            );
            // Vector waveform bars deterministically seeded by message id
            let wave_w = (max_text_width - 52.0).clamp(56.0, 148.0);
            let bar_count = ((wave_w / 5.0) as usize).clamp(8, 24);
            let (wave_rect, _) = ui.allocate_exact_size(Vec2::new(wave_w, 16.0), Sense::hover());
            let mut seed = msg
                .id
                .bytes()
                .fold(0x9E37_79B9_7F4A_7C15_u64, |acc, b| acc.rotate_left(5) ^ (b as u64));
            for i in 0..bar_count {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                let amp = 0.25 + ((seed >> 48) as f32 / 65535.0) * 0.72;
                let bh = (14.0 * amp).max(3.5);
                let bx = wave_rect.left() + (i as f32) * 5.0 + 1.5;
                if bx + 2.5 <= wave_rect.right() {
                    let bar_r = Rect::from_center_size(
                        Pos2::new(bx, wave_rect.center().y),
                        Vec2::new(2.5, bh),
                    );
                    ui.painter().rect_filled(bar_r, Rounding::same(1.2), pal.primary);
                }
            }
            ui.label(
                RichText::new(format_duration_mmss(duration))
                    .size(11.5)
                    .color(text_col.linear_multiply(0.75)),
            );
        });
    });
    action
}

/// File/doc bubble: per-type icon + name + meta line + "Mở"/"Lưu về máy" buttons.
/// PDF first-page render was evaluated and deliberately skipped: no pdf-render dep exists
/// in the tree and adding a heavy renderer for a thumbnail is not worth it, so PDF/office
/// files get a classified card + open-outside instead. Preview text (when extracted)
/// renders inline below the buttons.
/// Extensions whose text can be extracted in-app (matches `doc_preview::extract_text`).
pub(super) fn previewable_ext(file_name: &str) -> bool {
    matches!(
        file_name
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "pdf" | "docx" | "xlsx" | "pptx" | "txt" | "csv" | "md" | "log" | "rtf"
    )
}

fn render_doc_card(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    kind: AttachmentKind,
    file_name: &str,
    text_col: Color32,
    pal: &ThemePalette,
    preview: Option<&str>,
    preview_loading: bool,
    preview_error: Option<&str>,
) -> Option<BubbleAction> {
    let (icon, kind_label) = attachment_kind_label(kind);
    let mut action = None;

    // 1. Header: 40x40 Icon Badge on left + File Name & Metadata on right
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);
        let (badge_rect, _) = ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::hover());
        let badge_bg = if pal.is_dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 14)
        } else {
            pal.bg_elevated
        };
        ui.painter().rect_filled(badge_rect, Rounding::same(8.0), badge_bg);
        ui.painter().rect_stroke(badge_rect, Rounding::same(8.0), Stroke::new(1.0_f32, pal.border));

        let icon_col = match kind {
            AttachmentKind::Pdf => Color32::from_rgb(235, 87, 87),
            AttachmentKind::Word => Color32::from_rgb(45, 120, 240),
            AttachmentKind::Excel => Color32::from_rgb(39, 174, 96),
            AttachmentKind::PowerPoint => Color32::from_rgb(242, 153, 74),
            AttachmentKind::Archive => Color32::from_rgb(242, 201, 76),
            _ => pal.primary,
        };
        draw_lucide_icon(ui.painter(), badge_rect.center(), icon, 20.0, icon_col);

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
            let display_name = truncate_chars(file_name, 50);
            ui.add(
                egui::Label::new(
                    RichText::new(display_name)
                        .size(13.5)
                        .color(text_col)
                        .strong(),
                )
                .wrap(),
            )
            .on_hover_text(file_name);

            let ext = if file_name.contains('.') {
                file_name
                    .rsplit('.')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_uppercase()
            } else {
                String::new()
            };
            let meta = if ext.is_empty() {
                kind_label.to_string()
            } else {
                format!("{kind_label} · {ext}")
            };
            ui.label(
                RichText::new(meta)
                    .size(11.5)
                    .color(text_col.linear_multiply(0.7)),
            );
        });
    });

    ui.add_space(8.0);

    // 2. Action buttons: harmonious dimensions, matching rounding and styling
    let secondary_btn_fill = if pal.is_dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 14)
    } else {
        pal.bg_elevated
    };
    let primary_btn_fill = pal.primary;
    let secondary_btn_stroke = Stroke::new(1.0_f32, pal.border);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
        let open_resp = ui
            .add(
                egui::Button::new(
                    RichText::new("Mở")
                        .size(12.0)
                        .color(Color32::WHITE)
                        .strong(),
                )
                .fill(primary_btn_fill)
                .stroke(Stroke::NONE)
                .rounding(Rounding::same(6.0))
                .min_size(Vec2::new(64.0, 28.0)),
            )
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Mở bằng ứng dụng mặc định / trình duyệt");
        if open_resp.clicked() {
            if let Some(url) = msg.link_url.as_deref().filter(|u| !u.is_empty()) {
                if !url.to_ascii_lowercase().contains("/renewmedia") {
                    let _ = open_external_url(url);
                } else {
                    action = Some(BubbleAction::DownloadFile {
                        url: url.to_string(),
                        file_name: file_name.to_string(),
                    });
                }
            }
        }
        let save_resp = ui
            .add(
                egui::Button::new(
                    RichText::new("Lưu về máy")
                        .size(12.0)
                        .color(text_col),
                )
                .fill(secondary_btn_fill)
                .stroke(secondary_btn_stroke)
                .rounding(Rounding::same(6.0))
                .min_size(Vec2::new(96.0, 28.0)),
            )
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Tải và lưu tập tin về máy tính");
        if save_resp.clicked() {
            if let Some(url) = msg.link_url.as_deref().filter(|u| !u.is_empty()) {
                action = Some(BubbleAction::DownloadFile {
                    url: url.to_string(),
                    file_name: file_name.to_string(),
                });
            }
        }
    });
    // Inline preview snippet (text đã trích, session-only) hoặc nút "Xem trước".
    if let Some(text) = preview {
        let snippet = crate::doc_preview::snippet_lines(text, 6);
        ui.add_space(2.0);
        egui::Frame::none()
            .fill(pal.bg_elevated)
            .inner_margin(Margin::symmetric(8.0, 6.0))
            .stroke(Stroke::new(1.0_f32, pal.border))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(snippet)
                        .size(11.5)
                        .color(text_col.linear_multiply(0.75)),
                );
            });
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let full_resp = ui
                .add(
                    egui::Button::new(
                        RichText::new("Xem đầy đủ").size(12.5).color(text_col),
                    )
                    .fill(secondary_btn_fill)
                    .stroke(secondary_btn_stroke)
                    .rounding(Rounding::same(6.0))
                    .min_size(Vec2::new(92.0, 26.0)),
                )
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text("Mở toàn bộ nội dung trong app");
            if full_resp.clicked() {
                action = Some(BubbleAction::OpenDocViewer {
                    msg_id: msg.id.clone(),
                    file_name: file_name.to_string(),
                });
            }
        });
    } else if preview_loading {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(
                RichText::new("Đang trích nội dung…")
                    .size(12.0)
                    .color(text_col.linear_multiply(0.7)),
            );
        });
    } else if let Some(err) = preview_error {
        ui.add_space(2.0);
        ui.label(
            RichText::new(format!("Lỗi: {err}"))
                .size(12.0)
                .color(pal.danger_text),
        );
    } else if previewable_ext(file_name) {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let prev_resp = ui
                .add(
                    egui::Button::new(
                        RichText::new("Xem trước").size(12.5).color(text_col),
                    )
                    .fill(secondary_btn_fill)
                    .stroke(secondary_btn_stroke)
                    .rounding(Rounding::same(6.0))
                    .min_size(Vec2::new(92.0, 26.0)),
                )
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text("Đọc nội dung ngay trong app");
            if prev_resp.clicked() {
                if let Some(url) = msg.link_url.as_deref().filter(|u| !u.is_empty()) {
                    action = Some(BubbleAction::FetchDocPreview {
                        url: url.to_string(),
                        file_name: file_name.to_string(),
                        msg_id: msg.id.clone(),
                    });
                }
            }
        });
    }
    ui.add_space(4.0);
    action
}

/// Best-effort voice duration in seconds from `history_source` params (`duration`, `fileSize`).
/// Unknown durations render as `--:--`; never fabricates a value.
fn voice_duration_secs(msg: &ChatMessage) -> Option<u64> {
    let from_src = msg.history_source.as_ref().and_then(|v| {
        v.get("meta")
            .and_then(|m| m.get("attachsList"))
            .and_then(|a| a.as_array())
            .and_then(|a| a.first())
            .or(Some(v))
    });
    from_src.and_then(|node| {
        let raw: u64 = node
            .get("duration")
            .or_else(|| node.get("dur"))
            .or_else(|| node.get("time"))
            .and_then(|d| match d {
                serde_json::Value::Number(n) => {
                    n.as_u64().or_else(|| n.as_i64().map(|i| i.max(0) as u64))
                }
                serde_json::Value::String(s) => s.trim().parse::<u64>().ok(),
                _ => None,
            })?;
        // Params voice Zalo là ms (vd 1610); >3600 thì chắc chắn là ms.
        Some(if raw > 3600 { raw / 1000 } else { raw })
    })
}

fn format_duration_mmss(secs: Option<u64>) -> String {
    match secs {
        Some(s) => format!("{:02}:{:02}", s / 60, s % 60),
        None => "--:--".to_string(),
    }
}


pub(super) fn is_zalo_file_or_video_message(msg: &ChatMessage) -> bool {
    let trimmed = msg.text.trim();
    msg.kind == crate::message_model::MessageKind::Video
        || msg.kind == crate::message_model::MessageKind::File
        || matches!(
            msg.link_src.as_deref(),
            Some("Tập tin Zalo") | Some("Video Zalo")
        ) || trimmed.starts_with("[Tập tin]")
        || trimmed.starts_with("[Video]")
        || trimmed.starts_with("[Tin nhắn thoại]")
        || msg.link_url.as_deref().is_some_and(|u| {
            let l = u.to_ascii_lowercase();
            l.contains("/renewmedia") || l.contains("video-") || l.contains(".dlmd.me")
        })
}

pub(super) fn photo_primary_url(msg: &ChatMessage) -> Option<&str> {
    if let Some(th) = msg
        .link_thumb
        .as_deref()
        .filter(|u| !u.is_empty() && crate::chat::is_zalo_photo_cdn_url(u))
    {
        return Some(th);
    }
    if let Some(lu) = msg
        .link_url
        .as_deref()
        .filter(|u| !u.is_empty() && crate::chat::is_zalo_photo_cdn_url(u))
    {
        return Some(lu);
    }
    let t = msg.text.trim();
    if crate::chat::is_zalo_photo_cdn_url(t) {
        return Some(t);
    }
    if let Some(pos) = t.find("https://").or_else(|| t.find("http://")) {
        let after = &t[pos..];
        let end = after.find(char::is_whitespace).unwrap_or(after.len());
        let candidate =
            after[..end].trim_end_matches(|c: char| matches!(c, ')' | ']' | ',' | ';' | '>'));
        if crate::chat::is_zalo_photo_cdn_url(candidate) {
            return Some(candidate);
        }
    }
    None
}

pub(super) fn is_zalo_photo_message(msg: &ChatMessage) -> bool {
    if is_sticker_message(msg).is_some() || is_zalo_file_or_video_message(msg) {
        return false;
    }
    // msgType photo là authoritative (#118): server đã bảo đây là ảnh thì render
    // ảnh, kể cả khi link_url lạ/không thuộc allowlist CDN (host mới, renewmedia).
    let trimmed = msg.text.trim();
    if msg.link_src.as_deref() == Some("Hình ảnh Zalo")
        || trimmed == "[Hình ảnh]"
        || trimmed.starts_with("[Hình ảnh] ")
    {
        return true;
    }
    // Marked photo (link_src/[Hình ảnh]) giữ photo card + viewer kể cả khi
    // link_url extensionless/host lạ — mở bằng photo_primary_url an toàn (#2508).
    let marked_photo = msg.link_src.as_deref() == Some("Hình ảnh Zalo")
        || trimmed == "[Hình ảnh]"
        || trimmed.starts_with("[Hình ảnh] ");
    // If link_url points to a non-image webpage (e.g. YouTube, Google Docs, News), it is a rich link card, not a photo message.
    if !marked_photo {
        if let Some(lu) = msg.link_url.as_deref() {
            if !lu.is_empty() && !crate::chat::is_zalo_photo_cdn_url(lu) {
                return false;
            }
        }
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
        || photo_primary_url(msg).is_some()
}

pub(super) fn extract_photo_user_caption(msg: &ChatMessage) -> Option<&str> {
    let is_valid_caption = |s: &str| -> bool {
        let t = s.trim();
        if t.is_empty() || t == "[Hình ảnh]" {
            return false;
        }
        let lower = t.to_ascii_lowercase();
        if [
            ".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".jfif", ".tiff",
        ]
        .iter()
        .any(|ext| lower.ends_with(ext))
        {
            return false;
        }
        !t.starts_with("http://")
            && !t.starts_with("https://")
            && !t.ends_with(".zdn.vn")
            && !t.ends_with(".zadn.vn")
            && !t.ends_with(".dlfl.vn")
            && !t.contains("photo-stal")
            && !t.contains("zpcloud")
            && !t.contains("zpg-r")
            && Some(t) != msg.link_src.as_deref()
    };
    if let Some(desc) = msg.link_desc.as_deref().map(str::trim) {
        if is_valid_caption(desc) {
            return Some(desc);
        }
    }
    let mut trimmed = msg.text.trim();
    if let Some(s) = trimmed.strip_prefix("[Đã thu hồi]") {
        trimmed = s.trim();
    }
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
/// Badge `⏳ còn …` cho tin trong thread bật vanish. `None` = không hiển thị.
pub(super) fn vanish_badge_label(remain_ms: Option<i64>) -> Option<String> {
    let ms = remain_ms?;
    if ms <= 0 {
        return None;
    }
    Some(format!("⏳ {}", format_vanish_remaining(ms)))
}

/// Format thời gian còn lại ngắn gọn: `59p`, `23g`, `6ng`.
pub(super) fn format_vanish_remaining(remain_ms: i64) -> String {
    if remain_ms <= 0 {
        return "hết hạn".to_string();
    }
    let secs = remain_ms / 1000;
    if secs < 3600 {
        let m = (secs / 60).max(1);
        format!("{m}p")
    } else if secs < 86_400 {
        format!("{}g", secs / 3600)
    } else {
        format!("{}ng", secs / 86_400)
    }
}

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
    failed_stickers: &HashMap<String, (u8, std::time::Instant)>,
    emoji_textures: &HashMap<String, TextureHandle>,
    idx: usize,
    vanish_remain: Option<i64>,
    pending_translate: bool,
    preview: Option<&str>,
    preview_loading: bool,
    preview_error: Option<&str>,
    md: MdOptions,
) -> Option<BubbleAction> {
    let avail_w = (ui.available_width() - 8.0).max(140.0);
    let max_bubble_width = (avail_w * 0.72).clamp(140.0, 520.0).min(avail_w);
    let max_text_width = (max_bubble_width - 24.0).max(90.0);
    // Tin đã biến mất: vô hiệu mọi media/link card — chỉ render text hệ thống.
    let vanished = msg.text.trim() == crate::chat::VANISH_EXPIRED_TEXT;
    let has_sticker = !vanished && is_sticker_message(msg).is_some();
    let call_card_opt = if vanished { None } else { extract_call_card_view(msg) };
    let is_call_card = call_card_opt.is_some();
    let is_photo_card = !vanished && !has_sticker && is_zalo_photo_message(msg);
    let is_file_card = !vanished && !has_sticker && is_zalo_file_or_video_message(msg);
    let has_link_card = !vanished
        && !has_sticker
        && !is_photo_card
        && !is_file_card
        && extract_message_link_view(msg).is_some();
    let has_interactive_card = is_call_card || has_link_card || is_photo_card || is_file_card;

    let show_timestamp = !msg.time.is_empty()
        && (!next_same_sender
            || is_latest_outgoing
            || has_interactive_card
            || !msg.reactions.is_empty()
            || msg.is_starred);

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

    let sender_is_name = !msg.sender.is_empty()
        && msg.sender != "0"
        && !msg.sender.chars().all(|c| c.is_ascii_digit());
    let show_group_sender = !msg.is_outgoing && !prev_same_sender && is_group && sender_is_name;

    // Pre-measure exact content width for both outgoing and incoming bubbles so every bubble,
    // photo, and call card hugs its content tightly without horizontal overflow or inner/outer frame mismatch.
    let content_width = if has_sticker {
        116.0_f32
    } else if is_call_card {
        max_text_width.min(256.0)
    } else if is_photo_card {
        let thumb_u = photo_primary_url(msg).unwrap_or("");
        let tkey = format!("thumb_{}", crate::crypto::md5_hex(thumb_u));
        let photo_w = if let Some(tex) = sticker_textures.get(&tkey) {
            let [tw, th] = tex.size();
            if tw <= 2 || th <= 2 {
                max_text_width.min(260.0).max(110.0)
            } else {
                let tw_f = tw as f32;
                let th_f = th as f32;
                let max_w = max_text_width.min(360.0).max(110.0);
                let max_h = 340.0_f32;
                let scale = (max_w / tw_f).min(max_h / th_f).min(1.0);
                (tw_f * scale).clamp(max_w.min(140.0), max_w).ceil()
            }
        } else {
            max_text_width.min(280.0).max(110.0)
        };
        if let Some(cap_str) = photo_caption {
            let cap_w = ui.fonts(|f| {
                f.layout(
                    cap_str.to_string(),
                    FontId::proportional(14.2),
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
    } else if is_file_card || has_link_card {
        let is_video = classify_attachment(msg) == Some(AttachmentKind::Video);
        let video_thumb_dim = if is_video {
            let thumb_url = msg
                .link_thumb
                .as_deref()
                .filter(|u| {
                    !u.is_empty()
                        && (u.starts_with("http://") || u.starts_with("https://"))
                        && !u.to_ascii_lowercase().contains("/renewmedia")
                })
                .or_else(|| {
                    msg.link_url
                        .as_deref()
                        .filter(|u| !u.is_empty() && crate::chat::is_zalo_photo_cdn_url(u))
                });
            let key = thumb_url
                .map(|u| format!("thumb_{}", crate::crypto::md5_hex(u)))
                .or_else(|| {
                    msg.link_url
                        .as_deref()
                        .filter(|u| !u.is_empty())
                        .map(video_thumb_key)
                });
            key.and_then(|k| sticker_textures.get(&k)).map(|tex| {
                let [tw, th] = tex.size();
                compute_video_thumb_size(tw, th, max_text_width)
            })
        } else {
            None
        };
        if let Some((vw, _)) = video_thumb_dim {
            vw.min(max_text_width)
        } else {
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
                let w = if is_file_card {
                    (measure(title, 13.5) + 58.0).max(180.0)
                } else {
                    measure(title, 13.8)
                        .max(measure(&cap, 13.8))
                        .max(measure(desc, 12.3))
                        .max(measure(&dom, 12.6))
                        + 8.0
                };
                let max_w = if is_file_card { max_text_width.min(380.0) } else { max_text_width.min(360.0) };
                let preferred_min = if is_file_card { 280.0_f32 } else { 160.0_f32 };
                let min_card_w = preferred_min.min(max_w);
                w.clamp(min_card_w, max_w)
                    .min(max_text_width)
                    .ceil()
            }
        }
    } else {
        let display_str = if msg.text.trim() == "[Tin nhắn đã thu hồi]" {
            "  Tin nhắn đã được thu hồi".to_string()
        } else if let Some(stripped) = msg.text.strip_prefix("[Đã thu hồi] ") {
            strip_markdown_syntax_for_width(stripped)
        } else {
            strip_markdown_syntax_for_width(&msg.text)
        };
        let text_w = ui.fonts(|f| {
            f.layout(
                display_str,
                FontId::proportional(14.2),
                Color32::WHITE,
                max_text_width,
            )
            .size()
            .x
        });
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
                    truncate_chars(&msg.sender, 22).into_owned(),
                    FontId::proportional(12.0),
                    Color32::WHITE,
                )
                .size()
                .x
            }) + 8.0
        } else {
            0.0_f32
        };
        let marks_w = if !msg.reactions.is_empty() || msg.is_starred {
            78.0_f32
        } else {
            0.0_f32
        };
        let ts_min_w = if show_timestamp && is_latest_outgoing && msg.is_outgoing {
            96.0_f32
        } else {
            44.0_f32
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
        12.0_f32
    };
    let v_margin = if has_sticker || is_frameless_photo {
        2.0_f32
    } else {
        10.0_f32
    };
    let total_bubble_w = content_width + h_margin * 2.0;

    if msg.is_outgoing {
        let out_resp = ui.vertical(|ui| {
            let row_resp = ui.horizontal(|ui| {
                // Zero horizontal spacing before the bubble so `left_pad + total_bubble_w`
                // never ratchets `ScrollArea` wider across frames.
                ui.spacing_mut().item_spacing.x = 0.0;
                let left_pad = (ui.available_width() - total_bubble_w - 4.0).max(0.0);
                if left_pad > 0.0 {
                    ui.add_space(left_pad);
                }

                let fill = if has_sticker || is_frameless_photo {
                    Color32::TRANSPARENT
                } else if let Some(card) = &call_card_opt {
                    if card.is_missed_or_failed {
                        if pal.is_dark {
                            Color32::from_rgb(38, 23, 30)
                        } else {
                            Color32::from_rgb(254, 242, 242)
                        }
                    } else {
                        pal.bg_elevated
                    }
                } else {
                    pal.bubble_out_bg
                };
                let is_anti_recalled = msg.text.trim().starts_with("[Đã thu hồi]");
                let stroke = if has_sticker || is_frameless_photo {
                    Stroke::NONE
                } else if is_anti_recalled {
                    Stroke::new(1.2_f32, pal.warning)
                } else if let Some(card) = &call_card_opt {
                    let border_col = if card.is_missed_or_failed {
                        Color32::from_rgb(239, 68, 68).linear_multiply(0.55)
                    } else {
                        Color32::from_rgb(34, 197, 94).linear_multiply(0.55)
                    };
                    Stroke::new(1.2_f32, border_col)
                } else if !pal.is_dark {
                    Stroke::new(1.0_f32, Color32::from_rgb(199, 224, 255))
                } else {
                    Stroke::NONE
                };
                let rounding = Rounding::same(12.0);

                let mut card_action = None;
                let mut quote_action = None;
                let mut reaction_action = None;
                let fr = egui::Frame::none()
                    .fill(fill)
                    .stroke(stroke)
                    .rounding(rounding)
                    .inner_margin(Margin::symmetric(h_margin, v_margin))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = ui.ctx().style().spacing.item_spacing;
                        ui.set_min_width(content_width);
                        ui.set_max_width(content_width);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            quote_action = render_quote_block(ui, msg, pal, true, content_width);
                            if let Some(card) = &call_card_opt {
                                card_action = render_call_card_widget(ui, card, pal, content_width);
                            } else if !render_sticker_widget(
                                ui,
                                msg,
                                sticker_textures,
                                loading_stickers,
                                failed_stickers,
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
                                    failed_stickers,
                                    preview,
                                    preview_loading,
                                    preview_error,
                                    md,
                                );
                            }
                            render_translation(ui, msg, pal);
                            if let Some(act) = render_reactions(ui, msg, pal, emoji_textures) {
                                reaction_action = Some(act);
                            }
                            let is_pending_or_failed = matches!(
                                msg.delivery,
                                crate::message_model::DeliveryState::Pending
                                    | crate::message_model::DeliveryState::Failed
                            );
                            if (show_timestamp || is_pending_or_failed)
                                && !has_sticker
                                && !is_frameless_photo
                            {
                                ui.add_space(if has_interactive_card { 8.0 } else { 3.5 });
                                let (status_str, status_col) = match msg.delivery {
                                    crate::message_model::DeliveryState::Pending => (
                                        format!("{} · Đang gửi…", msg.time),
                                        pal.text_muted,
                                    ),
                                    crate::message_model::DeliveryState::Failed => (
                                        format!("{} · Lỗi gửi", msg.time),
                                        pal.danger_text,
                                    ),
                                    crate::message_model::DeliveryState::Read if is_latest_outgoing => {
                                        (format!("{} · Đã xem", msg.time), pal.primary)
                                    }
                                    _ if is_latest_outgoing => {
                                        (format!("{} · Đã nhận", msg.time), pal.bubble_out_meta)
                                    }
                                    _ => (msg.time.clone(), pal.bubble_out_meta),
                                };
                                // Nhãn "(đã sửa)": tooltip liệt kê bản cũ; click handled
                                // qua menu "Xem lịch sử sửa" (BubbleAction::ShowEditHistory).
                                let status_str = if msg.edited {
                                    format!("{status_str} · (đã sửa)")
                                } else {
                                    status_str
                                };
                                ui.label(
                                    RichText::new(status_str).size(11.0).color(status_col),
                                )
                                .on_hover_text(if msg.edited {
                                    "Tin này đã được sửa (thu hồi + gửi lại). Mở menu chuột phải → Xem lịch sử sửa."
                                } else {
                                    ""
                                });
                            }
                            if !vanished {
                                if let Some(badge) = vanish_badge_label(vanish_remain) {
                                    if show_timestamp && !has_sticker && !is_frameless_photo {
                                        ui.add_space(1.0);
                                    }
                                    ui.label(
                                        RichText::new(badge).size(10.5).color(pal.text_muted).italics(),
                                    );
                                }
                            }
                        });
                    });
                if fr.response.double_clicked() {
                    return Some(BubbleAction::Reply);
                }
                reaction_action
                    .or(quote_action)
                    .or(card_action)
                    .or_else(|| {
                        bubble_context_menu(
                            ui,
                            &fr.response,
                            msg,
                            idx,
                            pal,
                            emoji_textures,
                            has_interactive_card,
                            "Bạn",
                            pending_translate,
                        )
                    })
            });

            row_resp.inner
        });
        out_resp.inner
    } else {
        ui.horizontal_top(|ui| {
            let display_sender = if sender_is_name {
                &msg.sender
            } else {
                thread_name
            };
            let sender_accent = avatar_color_for(display_sender, false);

            // Sender Avatar (only on first message of a consecutive group)
            if !prev_same_sender {
                let (av_rect, _) = ui.allocate_exact_size(Vec2::new(30.0, 30.0), Sense::hover());
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
                ui.add_space(30.0);
            }

            let fill = if has_sticker || is_frameless_photo {
                Color32::TRANSPARENT
            } else if let Some(card) = &call_card_opt {
                if card.is_missed_or_failed {
                    if pal.is_dark {
                        Color32::from_rgb(38, 23, 30)
                    } else {
                        Color32::from_rgb(254, 242, 242)
                    }
                } else {
                    pal.bg_elevated
                }
            } else {
                pal.bubble_in_bg
            };
            let stroke = if has_sticker || is_frameless_photo {
                Stroke::NONE
            } else if msg.text.trim().starts_with("[Đã thu hồi]") {
                Stroke::new(1.2_f32, pal.warning)
            } else if let Some(card) = &call_card_opt {
                let border_col = if card.is_missed_or_failed {
                    Color32::from_rgb(239, 68, 68).linear_multiply(0.55)
                } else {
                    Color32::from_rgb(34, 197, 94).linear_multiply(0.55)
                };
                Stroke::new(1.2_f32, border_col)
            } else {
                Stroke::new(1.0_f32, pal.bubble_in_border)
            };
            let rounding = Rounding::same(12.0);

            ui.vertical(|ui| {
                ui.set_max_width(total_bubble_w);
                let mut card_action = None;
                let mut quote_action = None;
                let mut reaction_action = None;
                let fr = egui::Frame::none()
                    .fill(fill)
                    .stroke(stroke)
                    .rounding(rounding)
                    .inner_margin(Margin::symmetric(h_margin, v_margin))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = ui.ctx().style().spacing.item_spacing;
                        ui.set_min_width(content_width);
                        ui.set_max_width(content_width);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            if show_group_sender {
                                ui.label(
                                    RichText::new(truncate_chars(display_sender, 22))
                                        .color(sender_accent)
                                        .size(12.0)
                                        .strong(),
                                );
                                ui.add_space(2.0);
                            }
                            quote_action = render_quote_block(ui, msg, pal, false, content_width);
                            if let Some(card) = &call_card_opt {
                                card_action = render_call_card_widget(ui, card, pal, content_width);
                            } else if !render_sticker_widget(
                                ui,
                                msg,
                                sticker_textures,
                                loading_stickers,
                                failed_stickers,
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
                                    failed_stickers,
                                    preview,
                                    preview_loading,
                                    preview_error,
                                    md,
                                );
                            }
                            render_translation(ui, msg, pal);
                            if let Some(act) = render_reactions(ui, msg, pal, emoji_textures) {
                                reaction_action = Some(act);
                            }
                            if show_timestamp && !has_sticker && !is_frameless_photo {
                                ui.add_space(if has_interactive_card { 8.0 } else { 3.5 });
                                let ts = if msg.edited {
                                    format!("{} · (đã sửa)", msg.time)
                                } else {
                                    msg.time.clone()
                                };
                                ui.label(
                                    RichText::new(ts)
                                        .color(pal.bubble_in_meta)
                                        .size(11.0),
                                );
                            }
                            if !vanished {
                                if let Some(badge) = vanish_badge_label(vanish_remain) {
                                    if show_timestamp && !has_sticker && !is_frameless_photo {
                                        ui.add_space(1.0);
                                    }
                                    ui.label(
                                        RichText::new(badge).size(10.5).color(pal.bubble_in_meta).italics(),
                                    );
                                }
                            }
                        });
                    });
                if fr.response.double_clicked() {
                    return Some(BubbleAction::Reply);
                }
                reaction_action
                    .or(quote_action)
                    .or(card_action)
                    .or_else(|| {
                        bubble_context_menu(
                            ui,
                            &fr.response,
                            msg,
                            idx,
                            pal,
                            emoji_textures,
                            has_interactive_card,
                            display_sender,
                            pending_translate,
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
    #[allow(dead_code)]
    Mention(String),
    JumpToQuoted(String),
    Forward,
    /// Open the "who reacted" dialog for this message.
    ShowReactions,
    React(&'static str),
    TogglePin,
    ToggleStar,
    ScheduleReminder,
    /// Mở editor sửa tin (chỉ text của mình, trong 15 phút).
    EditMessage,
    /// Mở modal lịch sử các bản cũ của tin đã sửa.
    ShowEditHistory,
    /// WaveA1: nhắc về tin này (modal chọn giờ). Xử lý ở workspace.
    RemindMessage,
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
    OpenVideoPlayer {
        url: String,
        title: String,
        sender: String,
        time: String,
        thumb_key: Option<String>,
    },
    DownloadFile {
        url: String,
        file_name: String,
    },
    /// Tải + trích text xem trước tài liệu trong app (chỉ khi bấm, không tự fetch).
    FetchDocPreview {
        url: String,
        file_name: String,
        msg_id: String,
    },
    /// Mở popup xem đầy đủ text đã trích (session-only).
    OpenDocViewer {
        msg_id: String,
        file_name: String,
    },
    PlayVoice {
        url: String,
        file_name: String,
        msg_id: String,
    },
    Translate,
    TranslateFrom(&'static str),
}

fn menu_item(ui: &mut egui::Ui, icon: &str, label: &str, text_color: Color32) -> egui::Response {
    let text = RichText::new(format!("{icon}   {label}"))
        .size(13.0)
        .color(text_color);
    let btn = egui::Button::new(text)
        .fill(Color32::TRANSPARENT)
        .min_size(Vec2::new(ui.available_width(), 26.0));
    ui.add(btn)
}

/// Right-click menu on a chat bubble / row: quote/reply, forward/share, copy, pin, star, delete-for-me.
pub(super) fn bubble_context_menu(
    ui: &mut egui::Ui,
    bubble: &egui::Response,
    msg: &ChatMessage,
    idx: usize,
    pal: &ThemePalette,
    emoji_textures: &HashMap<String, TextureHandle>,
    _has_interactive_card: bool,
    _display_sender: &str,
    pending_translate: bool,
) -> Option<BubbleAction> {
    let ctx_menu_key = Id::new("active_bubble_context_menu");
    let open_time_key = Id::new("bubble_context_menu_open_time");
    let translate_sub_key = Id::new("bubble_translate_submenu");
    let this_key = format!("{}_{}", msg.id, msg.ts);

    let pointer_pos = ui
        .ctx()
        .input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()));
    let in_bubble = pointer_pos.map_or(false, |p| bubble.rect.contains(p));

    let secondary_clicked = ui.ctx().input(|i| {
        i.pointer.button_clicked(egui::PointerButton::Secondary)
            || (i.pointer.button_clicked(egui::PointerButton::Primary)
                && (i.modifiers.ctrl || i.modifiers.command))
    });

    if in_bubble && secondary_clicked {
        let click_pos = pointer_pos.unwrap_or(bubble.rect.left_top());
        let current_time = ui.input(|i| i.time);
        ui.ctx().data_mut(|d| {
            d.insert_temp(ctx_menu_key, (this_key.clone(), click_pos, idx));
            d.insert_temp(open_time_key, current_time);
            d.remove::<String>(Id::new("active_hover_reaction_bubble"));
        });
        ui.ctx().request_repaint();
    }

    let active_menu: Option<(String, Pos2, usize)> = ui.ctx().data(|d| d.get_temp(ctx_menu_key));
    let is_this_active = active_menu
        .as_ref()
        .map_or(false, |(k, _, _)| k == &this_key);

    if !is_this_active {
        return None;
    }

    let (_, menu_pos, _) = active_menu.unwrap();
    let mut action = None;
    let mut close_menu = false;

    let screen = ui.ctx().screen_rect();
    let menu_w = 180.0_f32;
    let menu_h = 280.0_f32;
    let pos = Pos2::new(
        menu_pos.x.clamp(
            screen.left() + 8.0,
            (screen.right() - menu_w - 8.0).max(screen.left() + 8.0),
        ),
        menu_pos.y.clamp(
            screen.top() + 8.0,
            (screen.bottom() - menu_h - 8.0).max(screen.top() + 8.0),
        ),
    );

    egui::Area::new(Id::new(("active_ctx_menu_area", idx, msg.ts, &msg.id)))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ui.ctx(), |ui| {
            egui::Frame::none()
                .fill(if pal.is_dark {
                    Color32::from_rgb(33, 37, 43)
                } else {
                    Color32::WHITE
                })
                .stroke(Stroke::new(1.0_f32, pal.border))
                .rounding(Rounding::same(8.0))
                .shadow(egui::epaint::Shadow {
                    offset: Vec2::new(0.0, 4.0),
                    blur: 16.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(if pal.is_dark { 130 } else { 45 }),
                })
                .inner_margin(Margin::symmetric(10.0, 10.0))
                .show(ui, |ui| {
                    ui.set_width(menu_w);
                    ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);

                    // Tin đã biến mất: vô hiệu mọi thao tác media/link (chỉ cho xoá).
                    let vanished =
                        msg.text.trim() == crate::chat::VANISH_EXPIRED_TEXT;
                    // 0. Hàng emoji thả cảm xúc (giống Zalo gốc khi chuột phải)
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 0.0);
                        for &(code, key, tooltip) in &REACTION_LIST {
                            let btn_resp = if let Some(tex) = emoji_textures.get(key) {
                                let img =
                                    egui::Image::new(tex).fit_to_exact_size(Vec2::splat(18.0));
                                ui.add(egui::ImageButton::new(img).rounding(Rounding::same(4.0)))
                            } else {
                                ui.add(
                                    egui::Button::new(RichText::new(code).size(15.0))
                                        .fill(Color32::TRANSPARENT)
                                        .rounding(Rounding::same(4.0)),
                                )
                            };
                            let clicked = btn_resp.clicked();
                            btn_resp.on_hover_text(tooltip);
                            if clicked {
                                action = Some(BubbleAction::React(code));
                                close_menu = true;
                            }
                        }
                    });
                    ui.separator();
                    // 1. Trả lời (Quote)
                    if !vanished
                        && menu_item(ui, lucide::QUOTE, "Trả lời", pal.text_primary).clicked()
                    {
                        action = Some(BubbleAction::Reply);
                        close_menu = true;
                    }

                    // 2. Chia sẻ (Forward)
                    if !vanished
                        && menu_item(ui, lucide::FORWARD, "Chia sẻ", pal.text_primary).clicked()
                    {
                        action = Some(BubbleAction::Forward);
                        close_menu = true;
                    }

                    // 3. Copy tin nhắn
                    if !vanished
                        && menu_item(ui, lucide::COPY, "Copy tin nhắn", pal.text_primary)
                            .clicked()
                    {
                        ui.ctx().copy_text(msg.text.clone());
                        close_menu = true;
                    }
                    // 3b. Dịch tin nhắn (qua MyMemory). Có bản dịch → "Ẩn bản
                    // dịch"; đang pending → disable; còn lại chọn nguồn
                    // ("Tự động" = heuristic; Latin ngoài en nên chọn tay vì
                    // MyMemory autodetect không đáng tin).
                    if !vanished && !msg.text.trim().is_empty() {
                        if msg.translation.is_some() {
                            if menu_item(ui, "🌐", "Ẩn bản dịch", pal.text_primary).clicked() {
                                action = Some(BubbleAction::Translate);
                                close_menu = true;
                            }
                        } else if pending_translate {
                            ui.add_enabled(
                                false,
                                egui::Button::new(
                                    RichText::new("🌐   Đang dịch…").size(13.0).color(pal.text_muted),
                                ),
                            );
                        } else {
                            let tr_lbl = format!("Dịch tin nhắn {}", lucide::CHEVRON_RIGHT);
                            let resp = menu_item(ui, lucide::GLOBE, &tr_lbl, pal.text_primary);
                            if resp.clicked() {
                                action = Some(BubbleAction::TranslateFrom("auto"));
                                close_menu = true;
                            }
                            // Hover mở dải chọn nguồn ngay dưới (trong cùng menu).
                            if resp.hovered() {
                                ui.ctx().data_mut(|d| {
                                    d.insert_temp(translate_sub_key, true);
                                });
                            }
                            let sub_open: bool = ui
                                .ctx()
                                .data(|d| d.get_temp(translate_sub_key).unwrap_or(false));
                            if sub_open {
                                for (code, label) in [
                                    ("auto", "Tự động"),
                                    ("en", "Tiếng Anh"),
                                    ("zh", "Tiếng Trung"),
                                    ("ja", "Tiếng Nhật"),
                                    ("ko", "Tiếng Hàn"),
                                    ("fr", "Tiếng Pháp"),
                                    ("es", "Tây Ban Nha"),
                                    ("de", "Tiếng Đức"),
                                    ("ru", "Tiếng Nga"),
                                ] {
                                    if menu_item(ui, "·", label, pal.text_primary).clicked() {
                                        action = Some(BubbleAction::TranslateFrom(code));
                                        close_menu = true;
                                    }
                                }
                            }
                        }
                    }

                    let pin_label = if msg.is_pinned {
                        "Bỏ ghim tin nhắn"
                    } else {
                        "Ghim tin nhắn"
                    };
                    if menu_item(ui, lucide::PIN, pin_label, pal.text_primary).clicked() {
                        action = Some(BubbleAction::TogglePin);
                        close_menu = true;
                    }

                    // 5. Lưu tin (đồng bộ 2 chiều với star) — WaveA2.
                    let star_label = if msg.is_starred {
                        "Bỏ lưu tin nhắn"
                    } else {
                        "Lưu tin nhắn"
                    };
                    if menu_item(ui, lucide::STAR, star_label, pal.text_primary).clicked() {
                        action = Some(BubbleAction::ToggleStar);
                        close_menu = true;
                    }

                    // 5b. Nhắc tôi về tin này (WaveA1 nhắc hẹn trên tin nhắn).
                    if !vanished
                        && menu_item(ui, lucide::BELL, "Nhắc tôi về tin này", pal.text_primary)
                            .clicked()
                    {
                        action = Some(BubbleAction::RemindMessage);
                        close_menu = true;
                    }

                    // 5b. Hẹn giờ / Nhắc hẹn cho tin nhắn này (#94)
                    if !vanished
                        && menu_item(ui, lucide::BELL, "Hẹn giờ / Nhắc hẹn", pal.text_primary)
                            .clicked()
                    {
                        action = Some(BubbleAction::ScheduleReminder);
                        close_menu = true;
                    }

                    // 6. Nếu là ảnh: Xem ảnh phóng to
                    if !vanished && is_zalo_photo_message(msg) {
                        if let Some(thumb_url) = photo_primary_url(msg) {
                            if menu_item(ui, lucide::SEARCH, "Xem ảnh phóng to", pal.text_primary)
                                .clicked()
                            {
                                action = Some(BubbleAction::OpenImagePopup {
                                    tex_key: format!("thumb_{}", crate::crypto::md5_hex(thumb_url)),
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
                                close_menu = true;
                            }
                        }
                    }

                    // 7. Nếu là file / video / voice: Lưu tập tin (+ Nghe thoại cho voice)
                    if !vanished && is_zalo_file_or_video_message(msg) {
                        if let Some(url) = msg.link_url.as_deref().filter(|u| !u.is_empty()) {
                            let resolved_name = crate::chat::resolve_attachment_file_name(
                                msg.link_title.as_deref(),
                                msg.text.trim(),
                                url,
                            );
                            let label = if resolved_name.chars().count() > 18 {
                                format!("Lưu \"{}…\"", truncate_chars(&resolved_name, 16))
                            } else {
                                format!("Lưu \"{resolved_name}\"")
                            };
                            if matches!(
                                classify_attachment(msg),
                                Some(AttachmentKind::Voice | AttachmentKind::Audio)
                            ) && menu_item(ui, lucide::MIC, "Nghe tin nhắn thoại", pal.text_primary)
                                .clicked()
                            {
                                action = Some(BubbleAction::PlayVoice {
                                    url: url.to_string(),
                                    file_name: resolved_name.clone(),
                                    msg_id: msg.id.clone(),
                                });
                                close_menu = true;
                            }
                            if menu_item(ui, lucide::PAPERCLIP, &label, pal.text_primary).clicked()
                            {
                                action = Some(BubbleAction::DownloadFile {
                                    url: url.to_string(),
                                    file_name: resolved_name,
                                });
                                close_menu = true;
                            }
                        }
                    }

                    ui.separator();

                    // 7b. Sửa tin (WaveA3): chỉ text của mình trong 15 phút.
                    // recall + gửi mới (hạn chế: bên kia thấy thu hồi + tin mới).
                    {
                        let now_ms = crate::login::unix_now() * 1000;
                        if crate::chat::can_edit_message(msg, now_ms)
                            && menu_item(ui, lucide::SLIDERS, "Sửa", pal.text_primary).clicked()
                        {
                            action = Some(BubbleAction::EditMessage);
                            close_menu = true;
                        }
                        if msg.is_outgoing
                            && msg.edited
                            && menu_item(
                                ui,
                                "🕘",
                                "Xem lịch sử sửa",
                                pal.text_primary,
                            )
                            .clicked()
                        {
                            action = Some(BubbleAction::ShowEditHistory);
                            close_menu = true;
                        }
                    }

                    ui.separator();
                    if !vanished
                        && msg.is_outgoing
                        && msg.text.trim() != "[Tin nhắn đã thu hồi]"
                        && !msg.text.starts_with("[Đã thu hồi]")
                    {
                        if menu_item(ui, lucide::BAN, "Thu hồi", Color32::from_rgb(249, 115, 22))
                            .clicked()
                        {
                            action = Some(BubbleAction::RecallForEveryone);
                            close_menu = true;
                        }
                    }

                    // 9. Xóa chỉ ở phía tôi
                    if menu_item(
                        ui,
                        lucide::TRASH_2,
                        "Xóa chỉ ở phía tôi",
                        Color32::from_rgb(239, 68, 68),
                    )
                    .clicked()
                    {
                        action = Some(BubbleAction::DeleteForMe);
                        close_menu = true;
                    }

                    let menu_rect = ui.min_rect();
                    let pointer_pos = ui
                        .ctx()
                        .input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()));
                    let open_time: f64 =
                        ui.ctx().data(|d| d.get_temp(open_time_key)).unwrap_or(0.0);
                    if ui.input(|i| i.time) > open_time + 0.05 {
                        let pressed = ui.ctx().input(|i| i.pointer.any_pressed());
                        if pressed {
                            if let Some(pos) = pointer_pos {
                                if !menu_rect.expand(4.0).contains(pos) {
                                    close_menu = true;
                                }
                            }
                        }
                    }
                    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
                        close_menu = true;
                    }
                });
        });

    if close_menu {
        ui.ctx()
            .data_mut(|d| d.remove::<(String, Pos2, usize)>(ctx_menu_key));
        ui.ctx()
            .data_mut(|d| d.remove::<bool>(translate_sub_key));
        ui.ctx().request_repaint();
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
    let (_text_col, meta_col) = if outgoing {
        (pal.bubble_out_text, pal.bubble_out_meta)
    } else {
        (pal.bubble_in_text, pal.bubble_in_meta)
    };
    let bar_col = Color32::from_rgb(102, 166, 255); // #66A6FF
    let bg_col = if outgoing && pal.is_dark {
        Color32::from_rgba_unmultiplied(0, 0, 0, 42)
    } else if outgoing {
        Color32::from_rgba_unmultiplied(255, 255, 255, 150)
    } else if pal.is_dark {
        Color32::from_rgba_unmultiplied(0, 0, 0, 42)
    } else {
        Color32::from_rgb(229, 241, 255)
    };
    let inner_col_w = (max_w - 28.0).max(72.0);
    let frame = egui::Frame::none()
        .fill(bg_col)
        .rounding(Rounding::same(8.0))
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

#[allow(dead_code)]
pub(super) fn render_hover_reaction_bar(
    ui: &mut egui::Ui,
    bubble_rect: Rect,
    msg: &ChatMessage,
    idx: usize,
    outgoing: bool,
    pal: &ThemePalette,
    emoji_textures: &HashMap<String, TextureHandle>,
) -> Option<BubbleAction> {
    // Suppress hover bar if any context menu is open
    let ctx_menu_open: bool = ui.ctx().data(|d| {
        d.get_temp::<(String, Pos2, usize)>(Id::new("active_bubble_context_menu"))
            .is_some()
    });
    if ctx_menu_open || ui.memory(|m| m.any_popup_open()) {
        return None;
    }

    let bar_w = 236.0_f32;
    let bar_h = 32.0_f32;

    let clip_right = ui.clip_rect().right();
    let clip_left = ui.clip_rect().left();
    // Vùng hẹp hơn bar (drawer mở + cửa sổ nhỏ): min>max sẽ panic f32::clamp.
    // Không đủ chỗ thì dính sát mép trái, bar tràn ra cũng không crash.
    let lo = clip_left + 8.0;
    let hi = (clip_right - bar_w - 8.0).max(lo);

    let x = if outgoing {
        (bubble_rect.right() - bar_w).clamp(lo, hi)
    } else {
        let desired_x = (bubble_rect.right() - bar_w).max(bubble_rect.left());
        desired_x.clamp(lo, hi)
    };

    // If there is enough room above the bubble, place above; otherwise place below
    let y = if bubble_rect.top() >= bar_h + 6.0 {
        bubble_rect.top() - bar_h - 4.0
    } else {
        bubble_rect.bottom() + 4.0
    };

    let bar_rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(bar_w, bar_h));
    // The unified hover zone covers both the message bubble and the floating bar + 10px buffer
    let hover_zone = bubble_rect.union(bar_rect).expand2(Vec2::new(10.0, 10.0));

    let hover_id = Id::new("active_hover_reaction_bubble");
    let msg_key = format!("{}_{}", msg.id, msg.ts);

    let pointer_pos = ui
        .ctx()
        .input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()));
    let in_hover_zone = pointer_pos.map_or(false, |p| hover_zone.contains(p));
    let in_bubble = pointer_pos.map_or(false, |p| {
        bubble_rect.expand2(Vec2::new(4.0, 4.0)).contains(p)
    });

    // Active hover state: (message key, bar rect, frame it was last shown). The bar floats over the
    // previous message, so without the rect a neighbour's bubble under the bar stole the hover and
    // the bar vanished as soon as the pointer reached it.
    let hover_state_id = hover_id.with("state");
    let frame_now = ui.ctx().cumulative_pass_nr();
    let active_state: Option<(String, Rect, u64)> = ui.ctx().data(|d| d.get_temp(hover_state_id));
    let active_key: Option<String> = ui.ctx().data(|d| d.get_temp(hover_id));

    let pointer_on_other_bar = match (&active_state, pointer_pos) {
        (Some((key, rect, seen)), Some(p)) => {
            key != &msg_key
                && frame_now.saturating_sub(*seen) <= 2
                && rect.expand2(Vec2::new(10.0, 10.0)).contains(p)
        }
        _ => false,
    };

    let should_show = if active_key.as_deref() == Some(&msg_key) {
        if in_hover_zone {
            true
        } else {
            ui.ctx().data_mut(|d| {
                d.remove::<String>(hover_id);
                d.remove::<(String, Rect, u64)>(hover_state_id);
            });
            false
        }
    } else if in_bubble && !pointer_on_other_bar {
        ui.ctx()
            .data_mut(|d| d.insert_temp(hover_id, msg_key.clone()));
        true
    } else {
        false
    };

    if !should_show {
        return None;
    }
    ui.ctx()
        .data_mut(|d| d.insert_temp(hover_state_id, (msg_key.clone(), bar_rect, frame_now)));

    let mut action = None;

    egui::Area::new(Id::new(("hover_reaction_bar", idx, msg.ts, &msg.id)))
        .order(egui::Order::Foreground)
        .fixed_pos(Pos2::new(x, y))
        .show(ui.ctx(), |ui| {
            egui::Frame::none()
                .fill(if pal.is_dark {
                    Color32::from_rgb(33, 37, 43)
                } else {
                    Color32::WHITE
                })
                .stroke(Stroke::new(1.0_f32, pal.border))
                .rounding(Rounding::same(16.0))
                .shadow(egui::epaint::Shadow {
                    offset: Vec2::new(0.0, 3.0),
                    blur: 10.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(if pal.is_dark { 110 } else { 35 }),
                })
                .inner_margin(Margin::symmetric(8.0, 4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);

                        for &(code, key, tooltip) in &REACTION_LIST {
                            let btn_resp = if let Some(tex) = emoji_textures.get(key) {
                                let img =
                                    egui::Image::new(tex).fit_to_exact_size(Vec2::splat(20.0));
                                ui.add(egui::ImageButton::new(img).rounding(Rounding::same(6.0)))
                            } else {
                                ui.add(
                                    egui::Button::new(RichText::new(code).size(16.0))
                                        .fill(Color32::TRANSPARENT)
                                        .rounding(Rounding::same(6.0)),
                                )
                            };

                            let clicked = btn_resp.clicked();
                            btn_resp.on_hover_text(tooltip);
                            if clicked {
                                action = Some(BubbleAction::React(code));
                            }
                        }

                        let (sep_rect, _) =
                            ui.allocate_exact_size(Vec2::new(6.0, 16.0), Sense::hover());
                        ui.painter().line_segment(
                            [sep_rect.center_top(), sep_rect.center_bottom()],
                            Stroke::new(1.0_f32, pal.border),
                        );

                        // Quick Reply (Quote)
                        let reply_btn = egui::Button::new(
                            RichText::new(lucide::QUOTE)
                                .size(13.5)
                                .color(pal.text_secondary),
                        )
                        .fill(Color32::TRANSPARENT)
                        .min_size(Vec2::splat(22.0))
                        .rounding(Rounding::same(6.0));

                        let r_resp = ui.add(reply_btn);
                        let r_clicked = r_resp.clicked();
                        r_resp.on_hover_text("Trả lời");
                        if r_clicked {
                            action = Some(BubbleAction::Reply);
                        }

                        // Quick Forward / Share
                        let fwd_btn = egui::Button::new(
                            RichText::new(lucide::FORWARD)
                                .size(13.5)
                                .color(pal.text_secondary),
                        )
                        .fill(Color32::TRANSPARENT)
                        .min_size(Vec2::splat(22.0))
                        .rounding(Rounding::same(6.0));

                        let f_resp = ui.add(fwd_btn);
                        let f_clicked = f_resp.clicked();
                        f_resp.on_hover_text("Chia sẻ");
                        if f_clicked {
                            action = Some(BubbleAction::Forward);
                        }
                    });
                });
        });

    if action.is_some() {
        ui.ctx().data_mut(|d| d.remove::<String>(hover_id));
        ui.ctx().request_repaint();
    }

    action
}

/// Vẽ bản dịch dưới tin gốc: đường kẻ + text nghiêng màu muted.
pub(super) fn render_translation(ui: &mut egui::Ui, msg: &ChatMessage, pal: &ThemePalette) {
    let Some(tr) = msg.translation.as_deref().filter(|t| !t.trim().is_empty()) else {
        return;
    };
    ui.add_space(4.0);
    ui.separator();
    ui.label(
        RichText::new(format!("🌐 {tr}"))
            .size(12.5)
            .color(pal.text_muted)
            .italics(),
    );
}

pub(super) fn render_reactions(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    pal: &ThemePalette,
    emoji_textures: &HashMap<String, TextureHandle>,
) -> Option<BubbleAction> {
    // Pin badge display parked until server-side pinning is fully integrated (avoid showing mock data).
    let has_pin = false;
    let has_star = msg.is_starred;
    let has_react = !msg.reactions.is_empty() || !msg.reaction_summaries.is_empty();

    if !has_pin && !has_star && !has_react {
        return None;
    }

    // Clicking the summary pill opens the "who reacted" dialog.
    let mut action: Option<BubbleAction> = None;
    ui.add_space(3.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);

        if has_star {
            egui::Frame::none()
                .fill(pal.reaction_star.linear_multiply(0.2))
                .stroke(Stroke::new(1.0_f32, pal.reaction_star))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::symmetric(5.0, 2.0))
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(lucide::STAR)
                                .size(11.0)
                                .color(pal.reaction_star)
                                .strong(),
                        )
                        .selectable(false)
                        .wrap_mode(egui::TextWrapMode::Extend),
                    )
                    .on_hover_text("Tin nhắn đã đánh dấu");
                });
        }

        if has_react {
            let mut items: Vec<(String, u64)> = Vec::new();
            if !msg.reaction_summaries.is_empty() {
                for s in &msg.reaction_summaries {
                    if s.count > 0 {
                        if let Some(pos) = items.iter().position(|(e, _)| e == &s.emoji) {
                            items[pos].1 = items[pos].1.saturating_add(s.count);
                        } else {
                            items.push((s.emoji.clone(), s.count));
                        }
                    }
                }
            } else {
                for r in &msg.reactions {
                    if let Some(pos) = items.iter().position(|(e, _)| e == r) {
                        items[pos].1 = items[pos].1.saturating_add(1);
                    } else {
                        items.push((r.clone(), 1));
                    }
                }
            }
            // Most used first, so the pill shows the dominant icons.
            items.sort_by(|a, b| b.1.cmp(&a.1));
            let total: u64 = items.iter().map(|(_, c)| *c).sum();

            if total > 0 {
                // One compact pill like Zalo: up to 3 icons + the total. Separate pills per icon
                // grew wider than a sticker bubble and were clipped at the window edge.
            let fr_resp = egui::Frame::none()
                .fill(pal.bg_elevated)
                .stroke(Stroke::new(1.0_f32, pal.border))
                .rounding(Rounding::same(17.5))
                .inner_margin(Margin::symmetric(8.0, 3.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
                            for (emoji, _) in items.iter().take(3) {
                                let key = emoji_to_tex_key(emoji);
                                if let Some(tex) = emoji_textures.get(key) {
                                    ui.add(
                                        egui::Image::new(tex).fit_to_exact_size(Vec2::splat(15.0)),
                                    );
                                } else {
                                    ui.label(RichText::new(emoji).size(12.5));
                                }
                            }
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(total.to_string())
                                    .size(11.0)
                                    .color(pal.text_primary)
                                    .strong(),
                            );
                        });
                    })
                    .response;
                let pill_resp = ui.interact(
                    fr_resp.rect,
                    Id::new(("react_pill", &msg.id, msg.ts)),
                    Sense::click(),
                );
                if pill_resp.clicked() {
                    action = Some(BubbleAction::ShowReactions);
                }
                pill_resp
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_ui(|ui| {
                        // Per-reaction breakdown with the real icons instead of plain text.
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);
                            for (emoji, count) in &items {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);
                                    match emoji_textures.get(emoji_to_tex_key(emoji)) {
                                        Some(tex) => {
                                            ui.add(
                                                egui::Image::new(tex)
                                                    .fit_to_exact_size(Vec2::splat(18.0)),
                                            );
                                        }
                                        None => {
                                            ui.label(RichText::new(emoji).size(14.0));
                                        }
                                    }
                                    ui.label(RichText::new(count.to_string()).size(12.5).strong());
                                });
                            }
                        });
                    });
            }
        }
    });

    action
}

#[cfg(test)]
mod quote_navigation_tests {
    use super::*;

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
                                &HashMap::new(),
                                0,
                                None,
                                false,
                                None,
                                false,
                                None,
                                MdOptions::default(),
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
                matches!(action, Some(BubbleAction::JumpToQuoted(reference)) if reference == "original-1")
            );
        }
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

    #[test]
    fn bubble_context_menu_opens_on_secondary_click_and_emits_reply_action() {
        let ctx = egui::Context::default();
        let msg = ChatMessage {
            id: "msg-ctx-test".into(),
            text: "Hello right click test".into(),
            sender: "Tester".into(),
            is_outgoing: false,
            ts: 123456789,
            ..Default::default()
        };

        let bubble_rect = Rect::from_min_size(Pos2::new(50.0, 50.0), Vec2::new(120.0, 40.0));
        let click_pos = Pos2::new(60.0, 60.0);
        let screen = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)));

        // Frame 1: Pointer moved to bubble
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::PointerMoved(click_pos)],
                screen_rect: screen,
                time: Some(1.0),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let bubble_resp =
                        ui.interact(bubble_rect, Id::new("dummy_bubble"), Sense::hover());
                    let _ = bubble_context_menu(
                        ui,
                        &bubble_resp,
                        &msg,
                        0,
                        &ThemePalette::light(),
                        &HashMap::new(),
                        false,
                        "Tester",
                        false,
                    );
                });
            },
        );

        // Frame 2: Secondary button pressed
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Secondary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                screen_rect: screen,
                time: Some(1.05),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let bubble_resp =
                        ui.interact(bubble_rect, Id::new("dummy_bubble"), Sense::hover());
                    let _ = bubble_context_menu(
                        ui,
                        &bubble_resp,
                        &msg,
                        0,
                        &ThemePalette::light(),
                        &HashMap::new(),
                        false,
                        "Tester",
                        false,
                    );
                });
            },
        );

        // Frame 3: Secondary button released -> Context menu is created in ctx.data
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Secondary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                screen_rect: screen,
                time: Some(1.1),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let bubble_resp =
                        ui.interact(bubble_rect, Id::new("dummy_bubble"), Sense::hover());
                    let _ = bubble_context_menu(
                        ui,
                        &bubble_resp,
                        &msg,
                        0,
                        &ThemePalette::light(),
                        &HashMap::new(),
                        false,
                        "Tester",
                        false,
                    );
                });
            },
        );

        // Frame 4: Next frame where Area shapes are painted
        let output = ctx.run(
            egui::RawInput {
                screen_rect: screen,
                time: Some(1.15),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let bubble_resp =
                        ui.interact(bubble_rect, Id::new("dummy_bubble"), Sense::hover());
                    let _ = bubble_context_menu(
                        ui,
                        &bubble_resp,
                        &msg,
                        0,
                        &ThemePalette::light(),
                        &HashMap::new(),
                        false,
                        "Tester",
                        false,
                    );
                });
            },
        );

        // Find "Trả lời" menu item position from output shapes
        let mut reply_pos = None;
        for shape in &output.shapes {
            if let egui::Shape::Text(text) = &shape.shape {
                if text.galley.job.text.contains("Trả lời") {
                    reply_pos = Some(text.pos + Vec2::new(10.0, 5.0));
                    break;
                }
            }
        }
        let reply_pos = reply_pos.expect("Context menu should be open with 'Trả lời' text");

        // Frame 5: Move mouse to "Trả lời" and press Primary
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(reply_pos),
                    egui::Event::PointerButton {
                        pos: reply_pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                screen_rect: screen,
                time: Some(1.2),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let bubble_resp =
                        ui.interact(bubble_rect, Id::new("dummy_bubble"), Sense::hover());
                    let _ = bubble_context_menu(
                        ui,
                        &bubble_resp,
                        &msg,
                        0,
                        &ThemePalette::light(),
                        &HashMap::new(),
                        false,
                        "Tester",
                        false,
                    );
                });
            },
        );

        // Frame 6: Release Primary -> Reply action triggered!
        let mut action = None;
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: reply_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                screen_rect: screen,
                time: Some(1.25),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let bubble_resp =
                        ui.interact(bubble_rect, Id::new("dummy_bubble"), Sense::hover());
                    action = bubble_context_menu(
                        ui,
                        &bubble_resp,
                        &msg,
                        0,
                        &ThemePalette::light(),
                        &HashMap::new(),
                        false,
                        "Tester",
                        false,
                    );
                });
            },
        );

        assert!(matches!(action, Some(BubbleAction::Reply)));
    }


    #[test]
    fn extract_photo_user_caption_filters_raw_image_filenames() {
        let raw_filename_msg = ChatMessage {
            text: "[Hình ảnh] msedge_uuRZ2mxGXV.png".into(),
            ..Default::default()
        };
        assert_eq!(extract_photo_user_caption(&raw_filename_msg), None);

        let clipboard_msg = ChatMessage {
            text: "[Hình ảnh] clipboard_1730932200000.png".into(),
            ..Default::default()
        };
        assert_eq!(extract_photo_user_caption(&clipboard_msg), None);

        let real_caption_msg = ChatMessage {
            text: "[Hình ảnh] Xem ảnh này nè".into(),
            ..Default::default()
        };
        assert_eq!(
            extract_photo_user_caption(&real_caption_msg),
            Some("Xem ảnh này nè")
        );
    }
}

#[cfg(test)]
mod attachment_card_tests {
    use super::*;

    fn file_msg(text: &str, title: Option<&str>, url: &str) -> ChatMessage {
        ChatMessage {
            text: text.into(),
            link_url: Some(url.into()),
            link_title: title.map(str::to_string),
            link_src: Some("Tập tin Zalo".into()),
            ..Default::default()
        }
    }

    #[test]
    fn classifies_video_voice_and_office_docs() {
        let video = ChatMessage {
            text: "[Video] clip.mp4".into(),
            link_src: Some("Video Zalo".into()),
            ..Default::default()
        };
        assert_eq!(classify_attachment(&video), Some(AttachmentKind::Video));

        let voice = ChatMessage {
            text: "[Tin nhắn thoại]".into(),
            kind: crate::message_model::MessageKind::Voice,
            ..Default::default()
        };
        assert_eq!(classify_attachment(&voice), Some(AttachmentKind::Voice));

        let pdf = file_msg(
            "[Tập tin] Bao_cao.pdf",
            Some("Bao_cao.pdf"),
            "https://f16-zpc.zdn.vn/renewMedia?p=abc",
        );
        assert_eq!(classify_attachment(&pdf), Some(AttachmentKind::Pdf));

        let docx = file_msg(
            "[Tập tin] De_cuong.docx",
            Some("De_cuong.docx"),
            "https://f16-zpc.zdn.vn/renewMedia?p=def",
        );
        assert_eq!(classify_attachment(&docx), Some(AttachmentKind::Word));

        let xlsx = file_msg(
            "[Tập tin] Bang.xlsx",
            Some("Bang.xlsx"),
            "https://f16-zpc.zdn.vn/renewMedia?p=ghi",
        );
        assert_eq!(classify_attachment(&xlsx), Some(AttachmentKind::Excel));

        let pptx = file_msg(
            "[Tập tin] Slide.pptx",
            Some("Slide.pptx"),
            "https://f16-zpc.zdn.vn/renewMedia?p=jkl",
        );
        assert_eq!(classify_attachment(&pptx), Some(AttachmentKind::PowerPoint));
    }

    #[test]
    fn voice_duration_and_format_helpers() {
        assert_eq!(format_duration_mmss(Some(75)), "01:15");
        assert_eq!(format_duration_mmss(None), "--:--");
        let no_dur = ChatMessage {
            text: "[Tin nhắn thoại]".into(),
            ..Default::default()
        };
        assert_eq!(voice_duration_secs(&no_dur), None);
        let with_dur = ChatMessage {
            text: "[Tin nhắn thoại]".into(),
            history_source: Some(serde_json::json!({"duration": 12})),
            ..Default::default()
        };
        assert_eq!(voice_duration_secs(&with_dur), Some(12));
    }

    #[test]
    fn video_thumb_key_is_stable_and_namespaced() {
        let url = "https://example.com/video.mp4";
        let k1 = video_thumb_key(url);
        assert!(k1.starts_with("vid_"));
        assert_eq!(k1, video_thumb_key(url));
        assert_ne!(k1, format!("thumb_{}", crate::crypto::md5_hex(url)));
    }

    #[test]
    fn doc_labels_cover_office_kinds() {
        assert_eq!(attachment_kind_label(AttachmentKind::Pdf).1, "Tài liệu PDF");
        assert_eq!(attachment_kind_label(AttachmentKind::Word).1, "Tài liệu Word");
        assert_eq!(attachment_kind_label(AttachmentKind::Excel).1, "Bảng tính Excel");
        assert_eq!(
            attachment_kind_label(AttachmentKind::PowerPoint).1,
            "Bài thuyết trình"
        );
    }
}

#[cfg(test)]
mod md_block_tests {
    use super::*;

    #[test]
    fn splits_code_fence_with_lang() {
        let blocks = split_md_blocks("hello\n```rust\nlet x = 1;\nlet y = 2;\n```\nbye");
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[0], MdBlock::Lines(l) if l == &vec!["hello".to_string()]));
        match &blocks[1] {
            MdBlock::Code { lang, body } => {
                assert_eq!(lang, "rust");
                assert!(body.contains("let x = 1;") && body.contains("let y = 2;"));
            }
            other => panic!("expected code, got {other:?}"),
        }
        assert!(matches!(&blocks[2], MdBlock::Lines(l) if l == &vec!["bye".to_string()]));
    }

    #[test]
    fn unterminated_fence_keeps_content_as_text() {
        let blocks = split_md_blocks("```py\nprint(1)");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::Lines(l) => assert!(l.iter().any(|s| s.contains("print(1)"))),
            other => panic!("expected lines fallback, got {other:?}"),
        }
    }

    #[test]
    fn parses_table_and_rejects_col_mismatch() {
        let good = "| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |";
        match &split_md_blocks(good)[..] {
            [MdBlock::Table { headers, rows }] => {
                assert_eq!(headers, &vec!["a".to_string(), "b".to_string()]);
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0], vec!["1".to_string(), "2".to_string()]);
            }
            other => panic!("expected table, got {other:?}"),
        }
        let bad_sep = "| a | b |\n| xxx | yyy |\n| 1 | 2 |";
        assert!(matches!(&split_md_blocks(bad_sep)[..], [MdBlock::Lines(_)]));
        let mismatch = "| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 |";
        let blocks = split_md_blocks(mismatch);
        assert!(matches!(&blocks[0], MdBlock::Table { rows, .. } if rows.len() == 1));
        assert!(matches!(&blocks[1], MdBlock::Lines(_)));
    }

    #[test]
    fn parses_ordered_list_items() {
        assert_eq!(
            parse_ordered_item("1. mua sữa"),
            Some(("1".to_string(), 0, "mua sữa".to_string()))
        );
        assert_eq!(
            parse_ordered_item("  12. indent"),
            Some(("12".to_string(), 2, "indent".to_string()))
        );
        assert_eq!(parse_ordered_item("- bullet"), None);
        assert_eq!(parse_ordered_item("1.missing space"), None);
        assert_eq!(parse_ordered_item("1234. too long"), None);
    }

    #[test]
    fn parses_inline_mentions_italic_snake_case_and_multi_links() {
        let runs = parse_inline_md_runs("Chào @All và @An, mail a@b.com nhé!", false);
        assert!(runs.contains(&InlineMdRun::Mention("@All".into())));
        assert!(runs.contains(&InlineMdRun::Mention("@An".into())));
        assert!(!runs.contains(&InlineMdRun::Mention("@b.com".into())));

        let style_runs = parse_inline_md_runs("_nghiêng_ foo_bar_baz *đậm* `code`", true);
        assert!(style_runs.contains(&InlineMdRun::Italic("nghiêng".into())));
        assert!(style_runs.contains(&InlineMdRun::Bold("đậm".into())));
        assert!(style_runs.contains(&InlineMdRun::Code("code".into())));
        assert!(!style_runs.iter().any(|r| matches!(r, InlineMdRun::Italic(s) if s == "bar")));

        let link_runs = parse_inline_md_runs("Xem https://a.vn/1 và https://b.vn/2 nhé.", false);
        let urls: Vec<&str> = link_runs
            .iter()
            .filter_map(|r| match r {
                InlineMdRun::Link { url, .. } => Some(url.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(urls, vec!["https://a.vn/1", "https://b.vn/2"]);
    }

    #[test]
    fn outgoing_doc_and_link_bubbles_stay_within_narrow_scroll_width() {
        let ctx = egui::Context::default();
        let pal = ThemePalette::dark();
        pal.apply_to_ctx_with_anim(&ctx, false, 1.0);

        let doc_msg = ChatMessage {
            id: "doc-test-msg".into(),
            text: "GIGABYTE GA-H61M-DS2 REV. 4.01.rar".into(),
            sender: "Me".into(),
            is_outgoing: true,
            kind: crate::message_model::MessageKind::File,
            link_title: Some("GIGABYTE GA-H61M-DS2 REV. 4.01.rar".to_string()),
            link_url: Some("https://example.invalid/files/board.rar".to_string()),
            link_src: Some("Tập tin Zalo".into()),
            ts: 1730932200000,
            ..Default::default()
        };

        let link_msg = ChatMessage {
            id: "link-test-msg".into(),
            text: "05. [Coding] Computer Scientist - Google Drive".into(),
            sender: "Me".into(),
            is_outgoing: true,
            link_title: Some("05. [Coding] Computer Scientist - Google Drive".to_string()),
            link_url: Some(
                "https://drive.google.com/drive/mobile/folders/1LzMJCdxaZnQFkfm4M1_0o".to_string(),
            ),
            ts: 1730932200000,
            ..Default::default()
        };

        let screen = Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 500.0)));
        let empty_tex = HashMap::new();
        let mut loading = HashSet::new();
        let failed = HashMap::new();
        let mut seen_widths = Vec::new();

        for frame in 0..5 {
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: screen,
                    time: Some(frame as f64 * 0.05),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let stream_w = (ui.available_width() - 36.0).max(160.0);
                        egui::ScrollArea::vertical()
                            .max_width(ui.available_width())
                            .show(ui, |ui| {
                                egui::Frame::none()
                                    .inner_margin(Margin::symmetric(18.0, 12.0))
                                    .show(ui, |ui| {
                                        ui.set_width(stream_w);
                                        ui.set_max_width(stream_w);
                                        let _ = render_chat_bubble(
                                            ui,
                                            &doc_msg,
                                            false,
                                            false,
                                            true,
                                            false,
                                            "Cloud của tôi",
                                            None,
                                            &pal,
                                            &empty_tex,
                                            &mut loading,
                                            &failed,
                                            &empty_tex,
                                            0,
                                            None,
                                            false,
                                            None,
                                            false,
                                            None,
                                            MdOptions::default(),
                                        );
                                        let _ = render_chat_bubble(
                                            ui,
                                            &link_msg,
                                            false,
                                            false,
                                            false,
                                            false,
                                            "Cloud của tôi",
                                            None,
                                            &pal,
                                            &empty_tex,
                                            &mut loading,
                                            &failed,
                                            &empty_tex,
                                            1,
                                            None,
                                            false,
                                            None,
                                            false,
                                            None,
                                            MdOptions::default(),
                                        );
                                        seen_widths.push(ui.min_rect().width());
                                    });
                            });
                    });
                },
            );
            // Buttons in Dark Mode must never paint bright white/light-gray fill (#FFFFFF / #E6E6E6).
            let white_button_rects = out.shapes.iter().filter(|s| {
                matches!(
                    &s.shape,
                    egui::Shape::Rect(r)
                        if r.fill.r() > 210 && r.fill.g() > 210 && r.fill.b() > 210
                )
            }).count();
            assert_eq!(white_button_rects, 0, "Dark mode doc card must not render white buttons");
        }
        assert!(
            seen_widths.iter().all(|&w| w <= 290.0),
            "Bubbles must not ratchet past narrow stream width: {seen_widths:?}"
        );
        assert!(
            (seen_widths[4] - seen_widths[0]).abs() < 1.0,
            "Bubble width must stay stable across frames without ratcheting: {seen_widths:?}"
        );
    }

    #[test]
    fn test_compute_video_thumb_size_portrait_and_landscape() {
        // Portrait 9:16 (e.g. 720x1280)
        let (pw, ph) = compute_video_thumb_size(720, 1280, 300.0);
        assert!(ph > pw, "Portrait video must have height > width: ({pw}, {ph})");
        assert!(pw >= 140.0 && pw <= 220.0, "Portrait width within bounds: {pw}");
        assert!(ph <= 260.0, "Portrait height capped at 260: {ph}");

        // Landscape 16:9 (e.g. 1280x720)
        let (lw, lh) = compute_video_thumb_size(1280, 720, 300.0);
        assert!(lw > lh, "Landscape video must have width > height: ({lw}, {lh})");
        assert!(lw >= 160.0 && lw <= 280.0, "Landscape width within bounds: {lw}");
        assert!(lh <= 180.0, "Landscape height capped at 180: {lh}");
    }
}
