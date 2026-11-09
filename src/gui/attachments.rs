//! Composer attachment queue: native multi-file / folder pickers, clipboard file lists and
//! thumbnail preparation for the pending-attachment strip above the composer.

use std::path::{Path, PathBuf};

use eframe::egui;

/// Upper bound on queued attachments, matching the official Zalo PC composer limit.
pub const MAX_PENDING_ATTACHMENTS: usize = 50;

/// Longest edge (px) of the decoded preview kept for each queued image.
const THUMB_EDGE: u32 = 128;

/// Which native dialog the attach controls open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickMode {
    /// Multi-select dialog filtered to image types.
    Images,
    /// Multi-select dialog for any file type.
    Files,
    /// Folder dialog; every visible top-level file inside it is queued.
    Folder,
}

/// A file ready to be queued, with an optional decoded preview produced off the UI thread.
pub struct PreparedAttachment {
    pub path: PathBuf,
    /// `(width, height, thumbnail)` when the file decodes as an image.
    pub preview: Option<(u32, u32, egui::ColorImage)>,
}

/// A file waiting in the composer strip until the user presses Enter.
pub struct PendingAttachment {
    pub path: PathBuf,
    /// Original pixel size for images.
    pub dims: Option<(u32, u32)>,
    pub thumb: Option<egui::TextureHandle>,
}

/// Result of resolving a pick or paste into queueable files.
pub struct PreparedBatch {
    pub items: Vec<PreparedAttachment>,
    /// Files dropped because the batch exceeded [`MAX_PENDING_ATTACHMENTS`].
    pub truncated: usize,
}

/// Lowercase extensions sent as photos rather than generic files.
pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"))
}

/// Still-photo types Zalo groups into an album; GIFs are sent standalone like Zalo Web.
fn is_album_photo(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp"))
}

/// For each path, its `(index, total)` slot in the album, or `None` when it is sent
/// standalone. An album forms only when at least two still photos are queued.
pub fn plan_album_slots(paths: &[PathBuf]) -> Vec<Option<(u32, u32)>> {
    let total = paths.iter().filter(|p| is_album_photo(p)).count();
    if total < 2 {
        return vec![None; paths.len()];
    }
    let total = total as u32;
    let mut next = 0u32;
    paths
        .iter()
        .map(|p| {
            is_album_photo(p).then(|| {
                let slot = next;
                next += 1;
                (slot, total)
            })
        })
        .collect()
}

/// Reorder a queue so its album photos are sent back to back, starting where the first one
/// was queued; everything else keeps its relative order. Album members must be consecutive
/// in the conversation to render as one grid, so a document or GIF queued between two photos
/// is sent right after the album instead of splitting it.
pub fn album_send_order(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    if paths.iter().filter(|p| is_album_photo(p)).count() < 2 {
        return paths;
    }
    let Some(first) = paths.iter().position(|p| is_album_photo(p)) else {
        return paths;
    };
    let (photos, others): (Vec<PathBuf>, Vec<PathBuf>) =
        paths.into_iter().partition(|p| is_album_photo(p));
    let mut others = others.into_iter();
    let mut out: Vec<PathBuf> = others.by_ref().take(first).collect();
    out.extend(photos);
    out.extend(others);
    out
}

/// Split a Win32 `GetOpenFileNameW` multi-select buffer into absolute paths.
///
/// With `OFN_ALLOWMULTISELECT | OFN_EXPLORER` the buffer is either one full path
/// (`C:\a\b.png\0\0`) or the directory followed by bare names (`C:\a\0b.png\0c.png\0\0`).
// Only the Win32 dialog produces this format; tests exercise it on every platform.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse_multiselect_buffer(buf: &[u16]) -> Vec<PathBuf> {
    let mut parts: Vec<String> = Vec::new();
    for chunk in buf.split(|&c| c == 0) {
        if chunk.is_empty() {
            break;
        }
        parts.push(String::from_utf16_lossy(chunk));
    }
    match parts.len() {
        0 => Vec::new(),
        1 => vec![PathBuf::from(parts.remove(0))],
        _ => {
            let dir = PathBuf::from(parts.remove(0));
            parts.into_iter().map(|name| dir.join(name)).collect()
        }
    }
}

/// Split newline-separated dialog output (zenity, kdialog, osascript) into paths.
#[cfg_attr(windows, allow(dead_code))]
pub fn parse_line_paths(stdout: &str) -> Vec<PathBuf> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Parse a `text/uri-list` clipboard payload into local file paths, skipping comments,
/// non-`file://` URIs and undecodable entries.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
pub fn parse_uri_list(payload: &str) -> Vec<PathBuf> {
    payload
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.strip_prefix("file://"))
        .map(|rest| {
            // `file://host/path` → drop the authority; `file:///path` has an empty one.
            match rest.find('/') {
                Some(idx) => &rest[idx..],
                None => rest,
            }
        })
        .filter_map(percent_decode)
        .map(PathBuf::from)
        .collect()
}

#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// List the visible, non-empty regular files directly inside `dir`, sorted by name
/// (case-insensitive). Subfolders are not descended into.
///
/// # Errors
/// Returns an error when `dir` cannot be read.
pub fn list_folder_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let hidden_name = entry.file_name().to_string_lossy().starts_with('.');
        if hidden_name {
            continue;
        }
        // `fs::metadata` follows symlinks so linked files are included like Explorer does.
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.len() == 0 || is_os_hidden(&meta) {
            continue;
        }
        files.push(path);
    }
    files.sort_by_key(|p| {
        p.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });
    Ok(files)
}

#[cfg(windows)]
fn is_os_hidden(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    meta.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
}

#[cfg(not(windows))]
fn is_os_hidden(_meta: &std::fs::Metadata) -> bool {
    false
}

/// Append `incoming` to `queue`, skipping paths already queued and anything beyond
/// `capacity`. Returns `(added, rejected)`.
pub fn merge_into_queue(
    queue: &mut Vec<PathBuf>,
    incoming: Vec<PathBuf>,
    capacity: usize,
) -> (usize, usize) {
    let mut added = 0;
    let mut rejected = 0;
    for path in incoming {
        if queue.contains(&path) {
            continue;
        }
        if queue.len() >= capacity {
            rejected += 1;
            continue;
        }
        queue.push(path);
        added += 1;
    }
    (added, rejected)
}

/// Decode a small preview for image files; other files return `None`.
pub fn prepare_one(path: PathBuf) -> PreparedAttachment {
    let preview = if is_image_path(&path) {
        image::open(&path).ok().map(|img| {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let thumb = if w > THUMB_EDGE || h > THUMB_EDGE {
                image::imageops::thumbnail(&rgba, THUMB_EDGE, THUMB_EDGE)
            } else {
                rgba
            };
            let (tw, th) = thumb.dimensions();
            let col = egui::ColorImage::from_rgba_unmultiplied(
                [tw as usize, th as usize],
                thumb.as_raw(),
            );
            (w, h, col)
        })
    } else {
        None
    };
    PreparedAttachment { path, preview }
}

/// Resolve paths into a capped batch with previews. Blocking: call from a worker thread.
pub fn prepare_batch(paths: Vec<PathBuf>) -> PreparedBatch {
    let truncated = paths.len().saturating_sub(MAX_PENDING_ATTACHMENTS);
    let items = paths
        .into_iter()
        .take(MAX_PENDING_ATTACHMENTS)
        .map(prepare_one)
        .collect();
    PreparedBatch { items, truncated }
}

/// Open the native dialog for `mode`. Returns `None` when the user cancels; a chosen
/// folder with no eligible files yields `Some(vec![])`. Blocking: call from a worker thread.
pub fn pick_attachments(mode: PickMode) -> Option<Vec<PathBuf>> {
    let picked = match mode {
        PickMode::Images => pick_native_files(true, true),
        PickMode::Files => pick_native_files(false, true),
        PickMode::Folder => {
            let dir = pick_native_folder()?;
            return Some(list_folder_files(&dir).unwrap_or_else(|e| {
                crate::logger::log_error("gui::pick_folder", &format!("{}: {e}", dir.display()));
                Vec::new()
            }));
        }
    };
    (!picked.is_empty()).then_some(picked)
}

/// Single-file native dialog for settings flows (history import). Blocking.
pub fn pick_single_file() -> Option<PathBuf> {
    pick_native_files(false, false).into_iter().next()
}

/// Native folder dialog. Blocking.
pub fn pick_folder() -> Option<PathBuf> {
    pick_native_folder()
}

/// Copied files on the clipboard (Explorer / Finder / file managers), in clipboard order.
pub fn read_clipboard_file_list() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        win::clipboard_hdrop_paths()
    }
    #[cfg(target_os = "macos")]
    {
        // Finder puts every copied file on the pasteboard as an NSURL; JXA reads them all.
        let script = "ObjC.import('AppKit');\
            var pb = $.NSPasteboard.generalPasteboard;\
            var urls = pb.readObjectsForClassesOptions($([$.NSURL]), $({}));\
            var out = [];\
            if (urls) { for (var i = 0; i < urls.count; i++) {\
              var u = urls.objectAtIndex(i); if (u.isFileURL) out.push(ObjC.unwrap(u.path)); } }\
            out.join('\\n');";
        std::process::Command::new("osascript")
            .args(["-l", "JavaScript", "-e", script])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| parse_line_paths(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.is_file())
            .collect()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        for (prog, args) in [
            ("wl-paste", &["--no-newline", "--type", "text/uri-list"][..]),
            (
                "xclip",
                &["-selection", "clipboard", "-t", "text/uri-list", "-o"][..],
            ),
        ] {
            if let Ok(out) = std::process::Command::new(prog).args(args).output() {
                if out.status.success() {
                    let paths: Vec<PathBuf> = parse_uri_list(&String::from_utf8_lossy(&out.stdout))
                        .into_iter()
                        .filter(|p| p.is_file())
                        .collect();
                    if !paths.is_empty() {
                        return paths;
                    }
                }
            }
        }
        Vec::new()
    }
}

#[cfg(target_os = "windows")]
fn pick_native_files(images_only: bool, multi: bool) -> Vec<PathBuf> {
    win::open_file_dialog(images_only, multi)
}

#[cfg(target_os = "windows")]
fn pick_native_folder() -> Option<PathBuf> {
    win::browse_for_folder()
}

#[cfg(target_os = "macos")]
fn pick_native_files(images_only: bool, multi: bool) -> Vec<PathBuf> {
    let (prompt, of_type) = if images_only {
        (
            "Chọn hình ảnh để gửi qua Zalo",
            " of type {\"public.image\"}",
        )
    } else {
        ("Chọn tập tin đính kèm để gửi qua Zalo", "")
    };
    let multi_clause = if multi {
        " with multiple selections allowed"
    } else {
        ""
    };
    let script = format!(
        "set fs to (choose file with prompt \"{prompt}\"{of_type}{multi_clause})\n\
         if class of fs is not list then set fs to {{fs}}\n\
         set out to \"\"\n\
         repeat with f in fs\n\
         set out to out & POSIX path of f & linefeed\n\
         end repeat\n\
         return out"
    );
    run_dialog_command("osascript", &["-e", &script]).unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn pick_native_folder() -> Option<PathBuf> {
    run_dialog_command(
        "osascript",
        &[
            "-e",
            "POSIX path of (choose folder with prompt \"Chọn thư mục để gửi qua Zalo\")",
        ],
    )
    .unwrap_or_default()
    .into_iter()
    .next()
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn pick_native_files(images_only: bool, multi: bool) -> Vec<PathBuf> {
    let mut zenity_args = vec!["--file-selection"];
    if multi {
        zenity_args.extend(["--multiple", "--separator=\n"]);
    }
    if images_only {
        zenity_args.push("--file-filter=Images | *.png *.jpg *.jpeg *.webp *.gif *.bmp");
    }
    // Fall back to kdialog only when zenity is not installed, never after a cancel.
    if let Some(picked) = run_dialog_command("zenity", &zenity_args) {
        return picked;
    }
    let mut kdialog_args = vec!["--getopenfilename"];
    if multi || images_only {
        // kdialog takes the start dir before the filter; "." keeps its default location.
        kdialog_args.push(".");
    }
    if images_only {
        kdialog_args.push("*.png *.jpg *.jpeg *.webp *.gif *.bmp|Images");
    }
    if multi {
        kdialog_args.extend(["--multiple", "--separate-output"]);
    }
    run_dialog_command("kdialog", &kdialog_args).unwrap_or_default()
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn pick_native_folder() -> Option<PathBuf> {
    run_dialog_command("zenity", &["--file-selection", "--directory"])
        .or_else(|| run_dialog_command("kdialog", &["--getexistingdirectory"]))?
        .into_iter()
        .next()
}

/// Run a dialog helper and parse the chosen paths. `None` means the program is not
/// installed (try another helper); `Some(empty)` means the user cancelled.
#[cfg(not(target_os = "windows"))]
fn run_dialog_command(prog: &str, args: &[&str]) -> Option<Vec<PathBuf>> {
    match std::process::Command::new(prog).args(args).output() {
        Ok(out) if out.status.success() => {
            Some(parse_line_paths(&String::from_utf8_lossy(&out.stdout)))
        }
        // A non-zero exit is the user cancelling; there is nothing to report.
        Ok(_) => Some(Vec::new()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            crate::logger::log_error("gui::file_dialog", &format!("{prog}: {e}"));
            Some(Vec::new())
        }
    }
}

#[cfg(target_os = "windows")]
mod win {
    use std::ffi::c_void;
    use std::path::PathBuf;

    /// Characters reserved for `GetOpenFileNameW` results: 50 long paths fit comfortably.
    const MULTISELECT_BUF_CHARS: usize = 64 * 1024;

    #[repr(C)]
    struct OpenFileNameW {
        l_struct_size: u32,
        hwnd_owner: *mut c_void,
        h_instance: *mut c_void,
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
        lpfn_hook: *const c_void,
        lp_template_name: *const u16,
        pv_reserved: *mut c_void,
        dw_reserved: u32,
        flags_ex: u32,
    }

    #[repr(C)]
    struct BrowseInfoW {
        hwnd_owner: *mut c_void,
        pidl_root: *const c_void,
        psz_display_name: *mut u16,
        lpsz_title: *const u16,
        ul_flags: u32,
        lpfn: *const c_void,
        l_param: isize,
        i_image: i32,
    }

    #[link(name = "comdlg32")]
    extern "system" {
        fn GetOpenFileNameW(lpofn: *mut OpenFileNameW) -> i32;
        fn CommDlgExtendedError() -> u32;
    }
    #[link(name = "shell32")]
    extern "system" {
        fn SHBrowseForFolderW(lpbi: *mut BrowseInfoW) -> *mut c_void;
        fn SHGetPathFromIDListW(pidl: *const c_void, psz_path: *mut u16) -> i32;
        fn DragQueryFileW(h_drop: *mut c_void, i_file: u32, lpsz_file: *mut u16, cch: u32) -> u32;
    }
    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(pv_reserved: *mut c_void, dw_co_init: u32) -> i32;
        fn CoUninitialize();
        fn CoTaskMemFree(pv: *mut c_void);
    }
    #[link(name = "user32")]
    extern "system" {
        fn IsClipboardFormatAvailable(format: u32) -> i32;
        fn OpenClipboard(h_wnd_new_owner: *mut c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(u_format: u32) -> *mut c_void;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub(super) fn open_file_dialog(images_only: bool, multi: bool) -> Vec<PathBuf> {
        let filter_raw = if images_only {
            "Image Files (*.png;*.jpg;*.jpeg;*.webp;*.gif;*.bmp)\0*.png;*.jpg;*.jpeg;*.webp;*.gif;*.bmp\0All Files (*.*)\0*.*\0"
        } else {
            "All Files (*.*)\0*.*\0Video & Media (*.mp4;*.mov;*.mkv;*.mp3;*.wav)\0*.mp4;*.mov;*.mkv;*.mp3;*.wav\0Documents (*.pdf;*.zip;*.docx;*.xlsx;*.txt)\0*.pdf;*.zip;*.docx;*.xlsx;*.txt\0"
        };
        let title = match (images_only, multi) {
            (true, _) => "Chọn hình ảnh để gửi qua Zalo (có thể chọn nhiều)",
            (false, true) => "Chọn tập tin đính kèm để gửi qua Zalo (có thể chọn nhiều)",
            (false, false) => "Chọn tập tin",
        };
        // `wide` appends the final NUL, giving the double-NUL the filter list requires.
        let filter_u16 = wide(filter_raw);
        let title_u16 = wide(title);
        let mut file_buf = vec![0u16; MULTISELECT_BUF_CHARS];
        const OFN_NOCHANGEDIR: u32 = 0x0000_0008;
        const OFN_ALLOWMULTISELECT: u32 = 0x0000_0200;
        const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
        const OFN_FILEMUSTEXIST: u32 = 0x0000_1000;
        const OFN_EXPLORER: u32 = 0x0008_0000;
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
            flags: OFN_EXPLORER
                | (if multi { OFN_ALLOWMULTISELECT } else { 0 })
                | OFN_FILEMUSTEXIST
                | OFN_PATHMUSTEXIST
                | OFN_NOCHANGEDIR,
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
        // SAFETY: every pointer in `ofn` references a buffer that outlives this call.
        let ok = unsafe { GetOpenFileNameW(&mut ofn) };
        if ok == 0 {
            // SAFETY: plain query of the thread-local common dialog error code.
            let code = unsafe { CommDlgExtendedError() };
            if code != 0 {
                crate::logger::log_error(
                    "gui::file_dialog",
                    &format!("GetOpenFileNameW failed: 0x{code:04X}"),
                );
            }
            return Vec::new();
        }
        super::parse_multiselect_buffer(&file_buf)
    }

    pub(super) fn browse_for_folder() -> Option<PathBuf> {
        const COINIT_APARTMENTTHREADED: u32 = 0x2;
        const BIF_RETURNONLYFSDIRS: u32 = 0x0001;
        const BIF_NEWDIALOGSTYLE: u32 = 0x0040;
        const MAX_PATH_CHARS: usize = 260;

        // SAFETY: COM is initialised on this blocking worker thread and balanced below;
        // the PIDL is freed with CoTaskMemFree as SHBrowseForFolderW requires.
        unsafe {
            let hr = CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
            let com_ready = hr >= 0;
            let title = wide("Chọn thư mục");
            let mut display = vec![0u16; MAX_PATH_CHARS];
            let mut bi = BrowseInfoW {
                hwnd_owner: std::ptr::null_mut(),
                pidl_root: std::ptr::null(),
                psz_display_name: display.as_mut_ptr(),
                lpsz_title: title.as_ptr(),
                ul_flags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
                lpfn: std::ptr::null(),
                l_param: 0,
                i_image: 0,
            };
            let pidl = SHBrowseForFolderW(&mut bi);
            let mut result = None;
            if !pidl.is_null() {
                let mut path_buf = vec![0u16; MAX_PATH_CHARS];
                if SHGetPathFromIDListW(pidl, path_buf.as_mut_ptr()) != 0 {
                    let end = path_buf
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(path_buf.len());
                    if end > 0 {
                        result = Some(PathBuf::from(String::from_utf16_lossy(&path_buf[..end])));
                    }
                }
                CoTaskMemFree(pidl);
            }
            if com_ready {
                CoUninitialize();
            }
            result
        }
    }

    pub(super) fn clipboard_hdrop_paths() -> Vec<PathBuf> {
        const CF_HDROP: u32 = 15;
        const QUERY_COUNT: u32 = 0xFFFF_FFFF;
        // SAFETY: the clipboard is opened and closed on this thread; DragQueryFileW copies
        // each name into an owned buffer before the handle is released.
        unsafe {
            if IsClipboardFormatAvailable(CF_HDROP) == 0 {
                return Vec::new();
            }
            let mut opened = false;
            for _ in 0..6 {
                if OpenClipboard(std::ptr::null_mut()) != 0 {
                    opened = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            if !opened {
                return Vec::new();
            }
            let mut paths = Vec::new();
            let hdrop = GetClipboardData(CF_HDROP);
            if !hdrop.is_null() {
                let count = DragQueryFileW(hdrop, QUERY_COUNT, std::ptr::null_mut(), 0);
                for idx in 0..count {
                    let needed = DragQueryFileW(hdrop, idx, std::ptr::null_mut(), 0) as usize;
                    if needed == 0 {
                        continue;
                    }
                    let mut buf = vec![0u16; needed + 1];
                    let got =
                        DragQueryFileW(hdrop, idx, buf.as_mut_ptr(), buf.len() as u32) as usize;
                    if got > 0 && got <= needed {
                        paths.push(PathBuf::from(String::from_utf16_lossy(&buf[..got])));
                    }
                }
            }
            CloseClipboard();
            paths
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_send_order_keeps_photos_together() {
        let p = |s: &str| PathBuf::from(s);
        let queued = vec![p("a.pdf"), p("1.jpg"), p("b.gif"), p("2.png"), p("c.zip")];
        assert_eq!(
            album_send_order(queued),
            vec![p("a.pdf"), p("1.jpg"), p("2.png"), p("b.gif"), p("c.zip")]
        );
        let single = vec![p("a.pdf"), p("1.jpg"), p("b.gif")];
        assert_eq!(album_send_order(single.clone()), single);
    }

    fn u16z(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn multiselect_single_full_path() {
        let mut buf = u16z("C:\\pics\\a.png");
        buf.extend([0, 0, 0]);
        assert_eq!(
            parse_multiselect_buffer(&buf),
            vec![PathBuf::from("C:\\pics\\a.png")]
        );
    }

    #[test]
    fn multiselect_dir_then_names() {
        let mut buf = u16z("C:\\pics");
        buf.push(0);
        buf.extend(u16z("a.png"));
        buf.push(0);
        buf.extend(u16z("ảnh 2.jpg"));
        buf.extend([0, 0, 0, 0]);
        assert_eq!(
            parse_multiselect_buffer(&buf),
            vec![
                PathBuf::from("C:\\pics").join("a.png"),
                PathBuf::from("C:\\pics").join("ảnh 2.jpg")
            ]
        );
    }

    #[test]
    fn multiselect_empty_buffer() {
        assert!(parse_multiselect_buffer(&[0, 0]).is_empty());
        assert!(parse_multiselect_buffer(&[]).is_empty());
    }

    #[test]
    fn line_paths_skip_blank_lines() {
        assert_eq!(
            parse_line_paths("/a/b.png\n\n  /c d/e.pdf  \n"),
            vec![PathBuf::from("/a/b.png"), PathBuf::from("/c d/e.pdf")]
        );
    }

    #[test]
    fn uri_list_decodes_local_files_only() {
        let payload = "# comment\r\nfile:///home/u/My%20Doc.pdf\r\nhttps://x/y\r\nfile://host/tmp/%E1%BA%A3nh.png\nfile:///bad%zz\n";
        assert_eq!(
            parse_uri_list(payload),
            vec![
                PathBuf::from("/home/u/My Doc.pdf"),
                PathBuf::from("/tmp/ảnh.png")
            ]
        );
    }

    #[test]
    fn image_extension_detection_is_case_insensitive() {
        assert!(is_image_path(Path::new("x/Y.JPG")));
        assert!(is_image_path(Path::new("a.webp")));
        assert!(!is_image_path(Path::new("a.pdf")));
        assert!(!is_image_path(Path::new("noext")));
    }

    #[test]
    fn album_needs_two_still_photos() {
        let one = vec![PathBuf::from("a.jpg"), PathBuf::from("b.pdf"), PathBuf::from("c.gif")];
        assert_eq!(plan_album_slots(&one), vec![None, None, None]);
        assert!(plan_album_slots(&[]).is_empty());
    }

    #[test]
    fn album_slots_skip_gif_video_and_files() {
        let paths = vec![
            PathBuf::from("1.PNG"),
            PathBuf::from("doc.pdf"),
            PathBuf::from("2.jpeg"),
            PathBuf::from("anim.gif"),
            PathBuf::from("clip.mp4"),
            PathBuf::from("3.webp"),
        ];
        assert_eq!(
            plan_album_slots(&paths),
            vec![Some((0, 3)), None, Some((1, 3)), None, None, Some((2, 3))]
        );
    }

    #[test]
    fn merge_dedupes_and_caps() {
        let mut q = vec![PathBuf::from("a")];
        let (added, rejected) = merge_into_queue(
            &mut q,
            vec![
                PathBuf::from("a"),
                PathBuf::from("b"),
                PathBuf::from("c"),
                PathBuf::from("d"),
            ],
            3,
        );
        assert_eq!((added, rejected), (2, 1));
        assert_eq!(
            q,
            vec![PathBuf::from("a"), PathBuf::from("b"), PathBuf::from("c")]
        );
    }

    #[test]
    fn folder_listing_is_flat_sorted_and_skips_hidden_or_empty() {
        let dir = std::env::temp_dir().join(format!(
            "ctdoz_attach_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dir.join("sub")).expect("create temp dir");
        std::fs::write(dir.join("b.txt"), b"b").expect("write b");
        std::fs::write(dir.join("A.png"), b"a").expect("write A");
        std::fs::write(dir.join(".hidden"), b"h").expect("write hidden");
        std::fs::write(dir.join("empty.bin"), b"").expect("write empty");
        std::fs::write(dir.join("sub").join("nested.txt"), b"n").expect("write nested");

        let listed = list_folder_files(&dir).expect("list folder");
        let names: Vec<String> = listed
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(names, vec!["A.png".to_string(), "b.txt".to_string()]);
    }

    #[test]
    fn folder_listing_errors_on_missing_dir() {
        assert!(list_folder_files(Path::new("/definitely/not/here/ctdoz")).is_err());
    }

    #[test]
    fn prepare_batch_truncates_to_limit() {
        let paths: Vec<PathBuf> = (0..MAX_PENDING_ATTACHMENTS + 7)
            .map(|i| PathBuf::from(format!("missing_{i}.pdf")))
            .collect();
        let batch = prepare_batch(paths);
        assert_eq!(batch.items.len(), MAX_PENDING_ATTACHMENTS);
        assert_eq!(batch.truncated, 7);
        assert!(batch.items.iter().all(|i| i.preview.is_none()));
    }
}
