//! Saving attachments (files, videos, original photos) into the user's "Thư mục lưu file".
//!
//! Everything about a download that arrives in a chat message — the URL and the file name — is
//! **untrusted data** chosen by whoever sent the message. This module keeps the policy in small
//! pure functions (unit-tested below) and does the I/O in [`download_to_dir`]:
//!
//! - only `https://` URLs, or `http://` on Zalo's own CDN hosts ([`download_url_allowed`]),
//!   re-checked after redirects;
//! - the file name is sanitised for every OS ([`sanitize_filename`]) and never overwrites an
//!   existing file ([`unique_candidate`], `name (1).ext`, `name (2).ext`, …);
//! - data is streamed into `<name>.part` (never held in RAM) and moved into place only when
//!   complete, without replacing anything that appeared meanwhile;
//! - on Windows the result gets a `Zone.Identifier` stream (`ZoneId=3`, Mark-of-the-Web) so
//!   SmartScreen / Office treat it as coming from the Internet;
//! - executables and scripts are never opened directly ([`is_dangerous_extension`],
//!   [`open_downloaded`] reveals them in the folder instead).

use std::path::{Path, PathBuf};

/// Largest attachment we accept (Zalo itself caps files at 1 GB).
pub const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Longest file name (in characters, extension included) we create.
pub const MAX_NAME_CHARS: usize = 150;

/// Extensions that run code when "opened". Downloads with these are only revealed in the
/// folder, never launched.
const DANGEROUS_EXTENSIONS: &[&str] = &[
    // Windows executables, installers and script hosts
    "exe", "msi", "msp", "msix", "msixbundle", "appx", "appxbundle", "bat", "cmd", "com", "scr",
    "pif", "cpl", "dll", "sys", "drv", "ocx", "ps1", "psm1", "psd1", "ps1xml", "vbs", "vbe", "js",
    "jse", "wsf", "wsh", "ws", "wsc", "hta", "lnk", "url", "scf", "reg", "inf", "msc", "gadget",
    "application", "appref-ms", "jar", "chm", "xll", "settingcontent-ms", "library-ms",
    "search-ms", "diagcab", "iso", "img", "vhd", "vhdx",
    // Unix / macOS launchers
    "sh", "bash", "zsh", "csh", "ksh", "command", "tool", "desktop", "appimage", "run", "app",
    "pkg", "terminal", "workflow", "scpt",
];

/// Characters Windows forbids in file names (plus the Unix separator).
const FORBIDDEN_CHARS: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Zalo's own static/CDN hosts (`*.zalo.me`, `*.zaloapp.com`, `*.zdn.vn`, `*.zadn.vn`).
pub fn is_zalo_cdn_host(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    crate::session::is_zalo_host(&h)
        || h == "zdn.vn"
        || h.ends_with(".zdn.vn")
        || h == "zadn.vn"
        || h.ends_with(".zadn.vn")
}

/// May this URL be downloaded? `https://` with a host always; plain `http://` only for Zalo's
/// CDN hosts. Everything else (`file:`, `ftp:`, `data:`, foreign `http://`, credentials in the
/// URL) is refused.
pub fn download_url_allowed(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url.trim()) else {
        return false;
    };
    if !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    match u.scheme() {
        "https" => u.host_str().is_some_and(|h| !h.is_empty()),
        "http" => u.host_str().is_some_and(is_zalo_cdn_host),
        _ => false,
    }
}

/// Last non-empty path segment of `url`, percent-decoded (no query / fragment).
pub fn name_from_url(url: &str) -> Option<String> {
    let u = reqwest::Url::parse(url.trim()).ok()?;
    let seg = u.path_segments()?.rev().find(|s| !s.is_empty())?;
    let decoded = percent_decode(seg);
    let trimmed = decoded.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Windows device names that cannot be used as a file stem (with any extension).
fn is_reserved_windows_stem(stem: &str) -> bool {
    let s = stem.trim_end_matches([' ', '.']).to_ascii_uppercase();
    matches!(s.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || ((s.starts_with("COM") || s.starts_with("LPT"))
            && s.len() == 4
            && matches!(s.as_bytes()[3], b'1'..=b'9'))
        // Superscript digits are also reserved (COM¹, LPT²…).
        || ((s.starts_with("COM") || s.starts_with("LPT"))
            && s.chars().count() == 4
            && matches!(s.chars().nth(3), Some('¹' | '²' | '³')))
}

/// Split `name` into (stem, extension-with-dot). A leading dot is part of the stem.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Make an untrusted file name safe to create on Windows, macOS and Linux.
///
/// Removes path separators, `: * ? " < > |`, control and bidi-override characters, leading
/// dots/spaces (no hidden files, no `..`) and trailing dots/spaces; renames Windows device
/// names (`CON`, `COM1`, …); caps the length at [`MAX_NAME_CHARS`] while keeping a short
/// extension. Returns `fallback` when nothing usable is left.
pub fn sanitize_filename(raw: &str, fallback: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if FORBIDDEN_CHARS.contains(&c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        // Bidi controls (e.g. U+202E) can disguise `exe` as `pdf`; zero-width chars hide text.
        .filter(|c| {
            !matches!(
                *c,
                '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
            )
        })
        .collect();
    let mut name = cleaned
        .trim_start_matches(['.', ' ', '_'])
        .trim_end_matches(['.', ' '])
        .to_string();
    // Collapse the runs of underscores the replacement may have produced.
    while name.contains("__") {
        name = name.replace("__", "_");
    }

    if name.chars().count() > MAX_NAME_CHARS {
        let (stem, ext) = split_ext(&name);
        let ext = if ext.chars().count() <= 16 { ext } else { "" };
        let keep = MAX_NAME_CHARS - ext.chars().count();
        let stem: String = stem.chars().take(keep).collect();
        name = format!("{}{}", stem.trim_end_matches(['.', ' ']), ext);
    }

    // Windows checks the part before the *first* dot: `NUL.tar.gz` is reserved too.
    let first = name.split('.').next().unwrap_or("");
    if is_reserved_windows_stem(first) {
        name = format!("_{name}");
    }

    if name.is_empty() || name.chars().all(|c| c == '_' || c == '.' || c == ' ') {
        return fallback.to_string();
    }
    name
}

/// Default name when a message carries no usable file name: `tep_<YYYYmmdd_HHMMSS>`.
pub fn fallback_name() -> String {
    format!("tep_{}", chrono::Local::now().format("%Y%m%d_%H%M%S"))
}

/// The `n`-th candidate for `name` in `dir`: `name`, then `stem (1).ext`, `stem (2).ext`, …
pub fn unique_candidate(dir: &Path, name: &str, n: u32) -> PathBuf {
    if n == 0 {
        return dir.join(name);
    }
    let (stem, ext) = split_ext(name);
    dir.join(format!("{stem} ({n}){ext}"))
}

/// First candidate whose final path and `.part` path are both free according to `taken`.
pub fn pick_unique_path(dir: &Path, name: &str, taken: impl Fn(&Path) -> bool) -> PathBuf {
    for n in 0..10_000 {
        let p = unique_candidate(dir, name, n);
        if !taken(&p) && !taken(&part_path(&p)) {
            return p;
        }
    }
    unique_candidate(dir, &format!("{}_{}", fallback_name(), std::process::id()), 0)
}

/// `<final>.part`, the temporary file a download is streamed into.
pub fn part_path(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_os_string();
    s.push(".part");
    PathBuf::from(s)
}

/// `true` for extensions that execute code when opened (case-insensitive).
pub fn is_dangerous_extension(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().trim_end_matches(['.', ' ']).to_ascii_lowercase())
        .unwrap_or_default();
    match name.rsplit_once('.') {
        Some((_, ext)) => DANGEROUS_EXTENSIONS.contains(&ext),
        None => false,
    }
}

/// Validate the configured download folder: it must be an absolute path.
pub fn validate_target_dir(dir: &str) -> Result<PathBuf, String> {
    let trimmed = dir.trim();
    if trimmed.is_empty() {
        return Err("Chưa chọn thư mục lưu file (Cài đặt → Thư mục lưu file).".into());
    }
    let p = PathBuf::from(trimmed);
    if !p.is_absolute() {
        return Err(format!(
            "Thư mục lưu file phải là đường dẫn tuyệt đối: {trimmed}"
        ));
    }
    Ok(p)
}

/// Contents of the Windows `Zone.Identifier` stream (Mark-of-the-Web, Internet zone).
/// Only scheme + host are recorded: the query string of a CDN link may carry access tokens.
pub fn zone_identifier_contents(url: &str) -> String {
    let host_url = reqwest::Url::parse(url.trim())
        .ok()
        .and_then(|u| u.host_str().map(|h| format!("{}://{}/", u.scheme(), h)));
    match host_url {
        Some(h) => format!("[ZoneTransfer]\r\nZoneId=3\r\nHostUrl={h}\r\n"),
        None => "[ZoneTransfer]\r\nZoneId=3\r\n".to_string(),
    }
}

/// Write Mark-of-the-Web on Windows (no-op elsewhere). Failures are ignored: FAT/exFAT drives
/// and network shares have no alternate data streams.
pub fn write_mark_of_the_web(path: &Path, url: &str) {
    #[cfg(target_os = "windows")]
    {
        let mut ads = path.as_os_str().to_os_string();
        ads.push(":Zone.Identifier");
        let _ = std::fs::write(PathBuf::from(ads), zone_identifier_contents(url));
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (path, url);
    }
}

/// Human readable size: `512 B`, `1,5 KB`, `12,3 MB`, `1,02 GB` (Vietnamese decimal comma).
pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    let s = if v >= 100.0 {
        format!("{v:.0}")
    } else if v >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    };
    format!("{} {}", s.replace('.', ","), UNITS[unit])
}

/// Download progress snapshot passed to the progress callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub received: u64,
    /// From `Content-Length`, when the server sent one.
    pub total: Option<u64>,
}

/// Stream `url` into `dir` under a sanitised, non-colliding name derived from
/// `suggested_name` (falling back to the URL's last segment, then [`fallback_name`]).
///
/// `progress` is called at most every ~200 ms. Returns the final path.
pub async fn download_to_dir(
    client: &reqwest::Client,
    url: &str,
    suggested_name: &str,
    dir: &Path,
    mut progress: impl FnMut(Progress),
) -> Result<PathBuf, String> {
    use tokio::io::AsyncWriteExt;

    let url = url.trim();
    if !download_url_allowed(url) {
        return Err("Liên kết tải không an toàn hoặc không hợp lệ (chỉ chấp nhận https).".into());
    }
    if !dir.is_absolute() {
        return Err("Thư mục lưu file phải là đường dẫn tuyệt đối.".into());
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Không tạo được thư mục lưu file {}: {e}", dir.display()))?;

    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Không kết nối được máy chủ tải tệp: {e}"))?;
    if !download_url_allowed(resp.url().as_str()) {
        return Err("Máy chủ chuyển hướng tới địa chỉ không an toàn — đã huỷ tải.".into());
    }
    if !resp.status().is_success() {
        return Err(format!("Máy chủ trả về lỗi HTTP {}.", resp.status().as_u16()));
    }
    let total = resp.content_length();
    if total.is_some_and(|n| n > MAX_DOWNLOAD_BYTES) {
        return Err(format!(
            "Tệp quá lớn ({}), giới hạn {}.",
            format_bytes(total.unwrap_or(0)),
            format_bytes(MAX_DOWNLOAD_BYTES)
        ));
    }

    let raw_name = if suggested_name.trim().is_empty() {
        name_from_url(url).unwrap_or_default()
    } else {
        suggested_name.to_string()
    };
    let name = sanitize_filename(&raw_name, &fallback_name());

    // Reserve `<final>.part` atomically (create_new) so two concurrent downloads of the same
    // name never share a temp file.
    let (mut final_path, part, file) = {
        let mut attempt = 0;
        loop {
            let candidate = pick_unique_path(dir, &name, |p| p.exists());
            let part = part_path(&candidate);
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&part) {
                Ok(f) => break (candidate, part, f),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 20 => {
                    attempt += 1;
                }
                Err(e) => {
                    return Err(format!("Không ghi được vào {}: {e}", dir.display()));
                }
            }
        }
    };

    let result: Result<(), String> = async {
        let mut out = tokio::fs::File::from_std(file);
        let mut received: u64 = 0;
        let mut last_report = std::time::Instant::now();
        progress(Progress { received, total });
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| format!("Mất kết nối khi đang tải: {e}"))?
        {
            received += chunk.len() as u64;
            if received > MAX_DOWNLOAD_BYTES {
                return Err(format!("Tệp vượt giới hạn {}.", format_bytes(MAX_DOWNLOAD_BYTES)));
            }
            out.write_all(&chunk)
                .await
                .map_err(|e| format!("Lỗi ghi đĩa: {e}"))?;
            if last_report.elapsed() >= std::time::Duration::from_millis(200) {
                last_report = std::time::Instant::now();
                progress(Progress { received, total });
            }
        }
        if let Some(t) = total {
            if received != t {
                return Err(format!(
                    "Tải chưa đủ dữ liệu ({} / {}).",
                    format_bytes(received),
                    format_bytes(t)
                ));
            }
        }
        out.flush().await.map_err(|e| format!("Lỗi ghi đĩa: {e}"))?;
        out.sync_all().await.map_err(|e| format!("Lỗi ghi đĩa: {e}"))?;
        progress(Progress { received, total });
        Ok(())
    }
    .await;

    if let Err(e) = result {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }

    // Move into place without replacing a file that appeared meanwhile.
    for _ in 0..20 {
        match place_without_overwrite(&part, &final_path) {
            Ok(()) => {
                write_mark_of_the_web(&final_path, url);
                return Ok(final_path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                final_path = pick_unique_path(dir, &name, |p| p.exists() && p != part.as_path());
            }
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                return Err(format!("Không đổi tên được tệp tạm: {e}"));
            }
        }
    }
    let _ = std::fs::remove_file(&part);
    Err("Không tìm được tên tệp trống trong thư mục lưu file.".into())
}

/// Rename `from` → `to`, failing with `AlreadyExists` instead of replacing `to`.
///
/// Uses a hard link (atomic "create if absent" on NTFS/ext4/APFS); on file systems without hard
/// links (FAT/exFAT) falls back to check-then-rename.
fn place_without_overwrite(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::hard_link(from, to) {
        Ok(()) => {
            let _ = std::fs::remove_file(from);
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(e),
        Err(_) => {
            if to.exists() {
                return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
            }
            std::fs::rename(from, to)
        }
    }
}

/// Open a folder in the system file manager.
pub fn open_folder(dir: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(dir).spawn();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

/// Show `path` selected in its folder (Windows/macOS) or open the folder (Linux).
pub fn reveal_in_folder(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path.as_os_str());
        let _ = std::process::Command::new("explorer").arg(arg).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Some(dir) = path.parent() {
            open_folder(dir);
        }
    }
}

/// What the "Mở" button did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenOutcome {
    /// Opened with the default application.
    Opened,
    /// Executable/script: only revealed in its folder.
    RevealedOnly,
}

/// "Mở" for a finished download: opens documents/media with the default app, but only reveals
/// executables and scripts in their folder ([`is_dangerous_extension`]).
pub fn open_downloaded(path: &Path) -> OpenOutcome {
    if is_dangerous_extension(path) {
        reveal_in_folder(path);
        return OpenOutcome::RevealedOnly;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = std::process::Command::new("rundll32.exe")
            .arg("shell32.dll,ShellExec_RunDLL")
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
    OpenOutcome::Opened
}

/// Total size in bytes of the regular files under `dir` (recursive, symlinks not followed).
pub fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

/// Delete everything *inside* `dir` (files and sub-folders; symlinks are removed, never
/// followed) but keep `dir` itself. Returns (bytes freed, entries that could not be removed).
pub fn clear_dir_contents(dir: &Path) -> (u64, usize) {
    let mut freed = 0u64;
    let mut failed = 0usize;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else {
            failed += 1;
            continue;
        };
        if ft.is_dir() {
            let size = dir_size(&path);
            match std::fs::remove_dir_all(&path) {
                Ok(()) => freed += size,
                Err(_) => failed += 1,
            }
        } else {
            let size = if ft.is_file() {
                entry.metadata().map(|m| m.len()).unwrap_or(0)
            } else {
                0
            };
            match std::fs::remove_file(&path) {
                Ok(()) => freed += size,
                Err(_) => failed += 1,
            }
        }
    }
    (freed, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_policy() {
        assert!(download_url_allowed("https://f21-zpc.zdn.vn/123/file.pdf?token=x"));
        assert!(download_url_allowed("https://example.com/a.zip"));
        assert!(download_url_allowed("http://dl.zdn.vn/a.mp4"));
        assert!(download_url_allowed("http://zalo.me/x"));
        assert!(download_url_allowed("http://cover.zadn.vn/x.jpg"));
        assert!(!download_url_allowed("http://example.com/a.zip"));
        assert!(!download_url_allowed("http://zdn.vn.evil.com/a.zip"));
        assert!(!download_url_allowed("http://evilzdn.vn/a.zip"));
        assert!(!download_url_allowed("file:///etc/passwd"));
        assert!(!download_url_allowed("ftp://zdn.vn/a"));
        assert!(!download_url_allowed("data:text/plain,hi"));
        assert!(!download_url_allowed("javascript:alert(1)"));
        assert!(!download_url_allowed("https://user:pw@example.com/a"));
        assert!(!download_url_allowed("not a url"));
        assert!(!download_url_allowed(""));
    }

    #[test]
    fn sanitize_strips_paths_and_forbidden_chars() {
        let fb = "tep_x";
        assert_eq!(sanitize_filename("../../etc/passwd", fb), "etc_passwd");
        assert_eq!(sanitize_filename("..\\..\\Windows\\win.ini", fb), "Windows_win.ini");
        assert_eq!(sanitize_filename("C:\\Users\\a\\b.txt", fb), "C_Users_a_b.txt");
        assert_eq!(sanitize_filename("a<b>c:d\"e|f?g*h.txt", fb), "a_b_c_d_e_f_g_h.txt");
        assert_eq!(sanitize_filename("bad\u{0}name\n.txt", fb), "bad_name_.txt");
        assert_eq!(sanitize_filename("report.pdf. . .", fb), "report.pdf");
        assert_eq!(sanitize_filename("   spaced  .docx  ", fb), "spaced  .docx");
        assert_eq!(sanitize_filename(".hidden", fb), "hidden");
        assert_eq!(sanitize_filename("Báo cáo tháng 10.xlsx", fb), "Báo cáo tháng 10.xlsx");
    }

    #[test]
    fn sanitize_removes_bidi_overrides() {
        // "invoice\u{202E}fdp.exe" renders as "invoiceexe.pdf".
        let s = sanitize_filename("invoice\u{202E}fdp.exe", "f");
        assert_eq!(s, "invoicefdp.exe");
        assert!(is_dangerous_extension(Path::new(&s)));
    }

    #[test]
    fn sanitize_falls_back_when_empty() {
        assert_eq!(sanitize_filename("", "tep_1"), "tep_1");
        assert_eq!(sanitize_filename("...", "tep_1"), "tep_1");
        assert_eq!(sanitize_filename("///", "tep_1"), "tep_1");
        assert_eq!(sanitize_filename("  . ", "tep_1"), "tep_1");
        assert!(fallback_name().starts_with("tep_"));
    }

    #[test]
    fn sanitize_renames_windows_device_names() {
        assert_eq!(sanitize_filename("CON", "f"), "_CON");
        assert_eq!(sanitize_filename("con.txt", "f"), "_con.txt");
        assert_eq!(sanitize_filename("NUL.tar.gz", "f"), "_NUL.tar.gz");
        assert_eq!(sanitize_filename("nul.gz", "f"), "_nul.gz");
        assert_eq!(sanitize_filename("COM1.log", "f"), "_COM1.log");
        assert_eq!(sanitize_filename("lpt9", "f"), "_lpt9");
        assert_eq!(sanitize_filename("COM0.log", "f"), "COM0.log");
        assert_eq!(sanitize_filename("COMPUTER.txt", "f"), "COMPUTER.txt");
        assert_eq!(sanitize_filename("aux", "f"), "_aux");
    }

    #[test]
    fn sanitize_caps_length_and_keeps_extension() {
        let long = format!("{}.pdf", "ả".repeat(400));
        let s = sanitize_filename(&long, "f");
        assert_eq!(s.chars().count(), MAX_NAME_CHARS);
        assert!(s.ends_with(".pdf"));
        let no_ext = "x".repeat(300);
        assert_eq!(sanitize_filename(&no_ext, "f").chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn unique_names_never_collide() {
        let dir = Path::new("/d");
        assert_eq!(unique_candidate(dir, "a.txt", 0), Path::new("/d/a.txt"));
        assert_eq!(unique_candidate(dir, "a.txt", 2), Path::new("/d/a (2).txt"));
        assert_eq!(unique_candidate(dir, "archive.tar.gz", 1), Path::new("/d/archive.tar (1).gz"));
        assert_eq!(unique_candidate(dir, "README", 1), Path::new("/d/README (1)"));
        assert_eq!(unique_candidate(dir, ".env", 1), Path::new("/d/.env (1)"));

        let taken = [
            PathBuf::from("/d/a.txt"),
            PathBuf::from("/d/a (1).txt.part"),
            PathBuf::from("/d/a (2).txt"),
        ];
        let p = pick_unique_path(dir, "a.txt", |p| taken.iter().any(|t| t == p));
        assert_eq!(p, Path::new("/d/a (3).txt"));
        assert_eq!(pick_unique_path(dir, "b.txt", |_| false), Path::new("/d/b.txt"));
        assert_eq!(part_path(Path::new("/d/b.txt")), Path::new("/d/b.txt.part"));
    }

    #[test]
    fn dangerous_extensions() {
        for n in ["a.exe", "A.EXE", "x.pdf.exe", "s.ps1", "s.bat", "l.lnk", "j.jar", "x.msi",
            "r.reg", "h.hta", "x.scr", "x.js", "x.cpl", "x.dll", "x.vbs", "x.desktop", "x.sh",
            "x.exe.", "x.iso"]
        {
            assert!(is_dangerous_extension(Path::new(n)), "{n}");
        }
        for n in ["a.pdf", "a.docx", "a.mp4", "a.jpg", "a.zip", "README", "exe", "a.exe.txt"] {
            assert!(!is_dangerous_extension(Path::new(n)), "{n}");
        }
    }

    #[test]
    fn names_from_urls() {
        assert_eq!(
            name_from_url("https://f.zdn.vn/a/B%C3%A1o%20c%C3%A1o.pdf?x=1#y").as_deref(),
            Some("Báo cáo.pdf")
        );
        assert_eq!(name_from_url("https://f.zdn.vn/a/b/").as_deref(), Some("b"));
        assert_eq!(name_from_url("https://f.zdn.vn/").as_deref(), None);
        assert_eq!(name_from_url("https://f.zdn.vn/%2e%2e%2fx").as_deref(), Some("../x"));
        assert_eq!(name_from_url("https://f.zdn.vn/100%").as_deref(), Some("100%"));
    }

    #[test]
    fn target_dir_must_be_absolute() {
        assert!(validate_target_dir("").is_err());
        assert!(validate_target_dir("Downloads/Zalo").is_err());
        #[cfg(unix)]
        assert!(validate_target_dir("/home/u/Downloads").is_ok());
        #[cfg(windows)]
        assert!(validate_target_dir(r"C:\Users\u\Downloads").is_ok());
    }

    #[test]
    fn zone_identifier_omits_query() {
        let z = zone_identifier_contents("https://f21.zdn.vn/p/file.exe?token=SECRET");
        assert!(z.starts_with("[ZoneTransfer]\r\nZoneId=3\r\n"));
        assert!(z.contains("HostUrl=https://f21.zdn.vn/"));
        assert!(!z.contains("SECRET"));
    }

    #[test]
    fn bytes_are_formatted() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1536), "1,50 KB");
        assert_eq!(format_bytes(15 * 1024 * 1024), "15,0 MB");
        assert_eq!(format_bytes(300 * 1024 * 1024), "300 MB");
    }

    #[test]
    fn clearing_a_dir_keeps_the_dir_and_spares_outside_files() {
        let base = std::env::temp_dir().join(format!("qtzalo_dl_test_{}", std::process::id()));
        let cache = base.join("stickers_cache");
        std::fs::create_dir_all(cache.join("sub")).unwrap();
        std::fs::write(cache.join("a.png"), b"12345").unwrap();
        std::fs::write(cache.join("sub").join("b.png"), b"123").unwrap();
        std::fs::write(base.join("session.json"), b"keep").unwrap();
        assert_eq!(dir_size(&cache), 8);
        let (freed, failed) = clear_dir_contents(&cache);
        assert_eq!((freed, failed), (8, 0));
        assert!(cache.is_dir());
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
        assert_eq!(std::fs::read(base.join("session.json")).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn placing_never_overwrites() {
        let base = std::env::temp_dir().join(format!("qtzalo_place_test_{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let part = base.join("x.txt.part");
        let fin = base.join("x.txt");
        std::fs::write(&fin, b"old").unwrap();
        std::fs::write(&part, b"new").unwrap();
        let err = place_without_overwrite(&part, &fin).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&fin).unwrap(), b"old");
        let fin2 = base.join("x (1).txt");
        place_without_overwrite(&part, &fin2).unwrap();
        assert_eq!(std::fs::read(&fin2).unwrap(), b"new");
        assert!(!part.exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
