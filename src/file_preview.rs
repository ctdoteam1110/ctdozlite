//! Thumbnails for file attachments (PDF, Word, Excel, PowerPoint, text, images…), shown on the
//! chat's file cards like Zalo's document preview.
//!
//! macOS renders them with Quick Look (`qlmanage -t`), the same generator Finder uses, so every
//! type the system can preview works without bundling document parsers. Elsewhere there is no
//! generator and cards keep their plain badge.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Remote files larger than this are not fetched just to draw a thumbnail.
pub const MAX_PREVIEW_DOWNLOAD_BYTES: u64 = 25 * 1024 * 1024;
/// Longest edge of the generated thumbnail, in pixels.
pub const THUMB_PX: u32 = 480;

const PREVIEWABLE: &[&str] = &[
    "pdf", "doc", "docx", "rtf", "odt", "pages", "txt", "md", "csv", "xls", "xlsx", "numbers",
    "ods", "ppt", "pptx", "key", "odp", "png", "jpg", "jpeg", "gif", "heic", "webp", "bmp", "tif",
    "tiff", "svg", "json", "xml", "html", "htm", "log",
];

/// Whether Quick Look can draw a meaningful thumbnail for this file name.
pub fn previewable(file_name: &str) -> bool {
    Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| PREVIEWABLE.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Texture/cache key for the thumbnail of the attachment at `url`.
pub fn thumb_key(url: &str) -> String {
    format!("fthumb_{}", crate::crypto::md5_hex(url))
}

/// Private folder (relative to the app data directory) for files fetched only to preview them.
pub fn cache_dir(key: &str) -> PathBuf {
    Path::new("file_cache").join(key)
}

/// A file name safe to create inside `cache_dir`: no path separators or parent references.
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '\0') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        "tep".to_string()
    } else {
        trimmed.chars().take(180).collect()
    }
}

/// PNG bytes of a Quick Look thumbnail for `path`, or `None` when the system has no generator
/// for the type or it took too long.
#[cfg(target_os = "macos")]
pub async fn quicklook_png(path: &Path) -> Option<Vec<u8>> {
    let out_dir = std::env::temp_dir().join(format!(
        "ctdozlite-ql-{}",
        crate::crypto::md5_hex(&path.to_string_lossy())
    ));
    let _ = std::fs::create_dir_all(&out_dir);
    let child = tokio::process::Command::new("/usr/bin/qlmanage")
        .arg("-t")
        .arg("-s")
        .arg(THUMB_PX.to_string())
        .arg("-o")
        .arg(&out_dir)
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .status();
    let ok = matches!(
        tokio::time::timeout(Duration::from_secs(20), child).await,
        Ok(Ok(st)) if st.success()
    );
    // qlmanage names its output `<file name>.png`.
    let png_path = path
        .file_name()
        .map(|n| out_dir.join(format!("{}.png", n.to_string_lossy())));
    let bytes = if ok {
        png_path.as_ref().and_then(|p| std::fs::read(p).ok())
    } else {
        None
    };
    let _ = std::fs::remove_dir_all(&out_dir);
    bytes.filter(|b| !b.is_empty())
}

#[cfg(not(target_os = "macos"))]
pub async fn quicklook_png(_path: &Path) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn office_pdf_and_text_files_are_previewable_but_archives_are_not() {
        for name in [
            "a.pdf",
            "B.DOCX",
            "sheet.xlsx",
            "deck.pptx",
            "notes.txt",
            "x.Numbers",
        ] {
            assert!(previewable(name), "{name}");
        }
        for name in ["a.zip", "b.rar", "setup.exe", "noext", "movie.mp4"] {
            assert!(!previewable(name), "{name}");
        }
    }

    #[test]
    fn cached_file_names_cannot_escape_the_cache_folder() {
        assert_eq!(safe_file_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(
            safe_file_name("Tiểu luận: bản 1.pdf"),
            "Tiểu luận_ bản 1.pdf"
        );
        assert_eq!(safe_file_name("..."), "tep");
        assert!(!safe_file_name("a/b\\c").contains(['/', '\\']));
    }
}
