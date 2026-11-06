//! Read-only reuse of the media Zalo PC has already downloaded on this machine.
//!
//! Zalo PC keeps its downloads as plain files under
//! `~/Library/Application Support/ZaloData/media/<own uid>/ZaloDownloads` (see
//! `re-notes/38_zalo_pc_media.md`). Two layouts carry enough to pair a file with a message:
//! - `picture/<thread>[_group]/z<global msg id>_<hash>.<jpg|jxl>`: exact by message id.
//! - `resource/<g?thread>/<picture|video|file>/<cli msg id>_<sender uid>_<g?thread>[_<hash>.<ext>]`:
//!   by client message id, or by sender plus a send time within a couple of seconds.
//!
//! Nothing here writes into Zalo's folders; the index only lists directory entries.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What kind of attachment a `resource/` entry holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Picture,
    Video,
    File,
}

#[derive(Debug, Clone)]
pub struct MediaEntry {
    /// Client message id (a millisecond clock value on the sender's device).
    pub cli_id: i64,
    pub sender: String,
    pub kind: MediaKind,
    pub path: PathBuf,
}

/// Directory index of Zalo PC's downloads for one account.
#[derive(Debug, Default)]
pub struct ZaloPcMedia {
    /// Global message id → picture file.
    pictures_by_msg: HashMap<String, PathBuf>,
    /// Bare thread id (no `g` / `_group`) → entries sorted by `cli_id`.
    by_thread: HashMap<String, Vec<MediaEntry>>,
}

/// Sender clock vs server timestamp: how far apart a message's `ts` and the file's client id may be.
const TS_WINDOW_MS: i64 = 2_500;

fn bare_thread(id: &str) -> &str {
    let id = id.trim_start_matches("group_");
    let id = id.strip_suffix("_group").unwrap_or(id);
    id.strip_prefix('g')
        .or_else(|| id.strip_prefix('G'))
        .unwrap_or(id)
}

/// `~/Library/Application Support/ZaloData/media/<uid>/ZaloDownloads` when it exists.
pub fn default_root(own_uid: &str) -> Option<PathBuf> {
    if own_uid.is_empty() || !cfg!(target_os = "macos") {
        return None;
    }
    let home = std::env::var_os("HOME")?;
    let p = PathBuf::from(home)
        .join("Library/Application Support/ZaloData/media")
        .join(own_uid)
        .join("ZaloDownloads");
    p.is_dir().then_some(p)
}

fn list_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (!name.starts_with('.')).then(|| (name, e.path()))
        })
        .collect()
}

fn list_dirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect()
}

/// `z8090717552569_b0ac…​.jxl` → `8090717552569`.
fn picture_msg_id(name: &str) -> Option<&str> {
    let rest = name.strip_prefix('z')?;
    let id = rest.split('_').next()?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && rest.len() > id.len())
        .then_some(id)
}

/// `1784951175547_5929513894379659594_g123[_hash.jxl]` → (cli id, sender uid).
fn resource_ids(name: &str) -> Option<(i64, &str)> {
    let mut it = name.split('_');
    let cli = it.next()?.parse::<i64>().ok()?;
    let sender = it.next()?;
    (!sender.is_empty() && sender.bytes().all(|b| b.is_ascii_digit())).then_some((cli, sender))
}

impl ZaloPcMedia {
    /// Builds the index from a `ZaloDownloads` folder. Missing folders simply yield nothing.
    pub fn scan(root: &Path) -> Self {
        let mut out = Self::default();
        for (_, dir) in list_dirs(&root.join("picture")) {
            for (name, path) in list_files(&dir) {
                if let Some(id) = picture_msg_id(&name) {
                    out.pictures_by_msg.insert(id.to_string(), path);
                }
            }
        }
        for (thread, tdir) in list_dirs(&root.join("resource")) {
            let key = bare_thread(&thread).to_string();
            let mut entries = Vec::new();
            for (sub, kind) in [
                ("picture", MediaKind::Picture),
                ("video", MediaKind::Video),
                ("file", MediaKind::File),
            ] {
                for (name, path) in list_files(&tdir.join(sub)) {
                    if let Some((cli_id, sender)) = resource_ids(&name) {
                        entries.push(MediaEntry {
                            cli_id,
                            sender: sender.to_string(),
                            kind,
                            path,
                        });
                    }
                }
            }
            if !entries.is_empty() {
                entries.sort_by_key(|e| e.cli_id);
                out.by_thread.entry(key).or_default().extend(entries);
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.pictures_by_msg.is_empty() && self.by_thread.is_empty()
    }

    pub fn len(&self) -> usize {
        self.pictures_by_msg.len() + self.by_thread.values().map(Vec::len).sum::<usize>()
    }

    /// The attachment Zalo PC saved for one message, if it can be paired without guessing:
    /// exact global id, exact client id, or the only entry from that sender near `ts`.
    /// `taken` holds paths already given to other messages so a burst of photos is not
    /// paired twice.
    pub fn attachment_for(
        &self,
        thread_id: &str,
        msg_id: &str,
        cli_id: &str,
        sender_uid: &str,
        ts_ms: i64,
        taken: &HashSet<PathBuf>,
    ) -> Option<(MediaKind, &Path)> {
        if let Some(p) = self
            .pictures_by_msg
            .get(msg_id)
            .filter(|p| !taken.contains(*p))
        {
            return Some((MediaKind::Picture, p));
        }
        let entries = self.by_thread.get(bare_thread(thread_id))?;
        for id in [cli_id, msg_id] {
            if let Ok(c) = id.parse::<i64>() {
                if let Some(e) = entries
                    .iter()
                    .find(|e| e.cli_id == c && !taken.contains(&e.path))
                {
                    return Some((e.kind, &e.path));
                }
            }
        }
        if sender_uid.is_empty() || ts_ms <= 0 {
            return None;
        }
        let start = entries.partition_point(|e| e.cli_id < ts_ms - TS_WINDOW_MS);
        let mut near = entries[start..]
            .iter()
            .take_while(|e| e.cli_id <= ts_ms + TS_WINDOW_MS)
            .filter(|e| e.sender == sender_uid && !taken.contains(&e.path));
        let first = near.next()?;
        // Two candidates in the window (an album): ambiguous, leave it.
        near.next()
            .is_none()
            .then_some((first.kind, first.path.as_path()))
    }
}

/// Decodes formats the `image` crate lacks (Zalo PC stores most photos as JPEG XL) through
/// macOS ImageIO, returning PNG bytes. `None` elsewhere or when ImageIO cannot read it.
#[cfg(target_os = "macos")]
pub fn decode_to_png_with_imageio(bytes: &[u8]) -> Option<Vec<u8>> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSData;
    objc2::rc::autoreleasepool(|_| unsafe {
        let data = NSData::with_bytes(bytes);
        let rep_cls = AnyClass::get("NSBitmapImageRep")?;
        let rep: *mut AnyObject = msg_send![rep_cls, imageRepWithData: &*data];
        if rep.is_null() {
            return None;
        }
        let dict_cls = AnyClass::get("NSDictionary")?;
        let props: *mut AnyObject = msg_send![dict_cls, dictionary];
        // NSBitmapImageFileTypePNG = 4.
        let png: *mut AnyObject =
            msg_send![rep, representationUsingType: 4usize, properties: props];
        if png.is_null() {
            return None;
        }
        let len: usize = msg_send![png, length];
        let ptr: *const u8 = msg_send![png, bytes];
        if ptr.is_null() || len == 0 {
            return None;
        }
        Some(std::slice::from_raw_parts(ptr, len).to_vec())
    })
}

#[cfg(not(target_os = "macos"))]
pub fn decode_to_png_with_imageio(_bytes: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn indexes_both_layouts_and_pairs_only_unambiguous_entries() {
        let root = std::env::temp_dir().join(format!("zpc_media_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        touch(&root.join("picture/777_group/z8090717552569_abc.jxl"));
        touch(&root.join("picture/777_group/.rescache"));
        touch(&root.join("resource/g777/picture/1784951175547_111_g777_ff.jxl"));
        touch(&root.join("resource/g777/file/1784085857192_222_g777"));
        // Album: two photos from one sender a few ms apart.
        touch(&root.join("resource/g777/picture/1784000000000_333_g777_a.jxl"));
        touch(&root.join("resource/g777/picture/1784000000004_333_g777_b.jxl"));
        let idx = ZaloPcMedia::scan(&root);
        let none = HashSet::new();

        // Exact global id.
        let (k, p) = idx
            .attachment_for("g777", "8090717552569", "", "", 0, &none)
            .unwrap();
        assert_eq!(k, MediaKind::Picture);
        assert!(p.ends_with("z8090717552569_abc.jxl"));
        // Exact client id, any thread id spelling.
        let (k, _) = idx
            .attachment_for("group_777", "x", "1784085857192", "", 0, &none)
            .unwrap();
        assert_eq!(k, MediaKind::File);
        // Same sender within the window (server ts later than the sender's clock).
        let (k, p) = idx
            .attachment_for("777", "y", "", "111", 1_784_951_177_000, &none)
            .unwrap();
        assert_eq!(k, MediaKind::Picture);
        // Already used by another message: not handed out twice.
        let taken: HashSet<PathBuf> = [p.to_path_buf()].into_iter().collect();
        assert!(idx
            .attachment_for("777", "y", "", "111", 1_784_951_177_000, &taken)
            .is_none());
        // Other sender, or outside the window: nothing.
        assert!(idx
            .attachment_for("777", "y", "", "999", 1_784_951_177_000, &none)
            .is_none());
        assert!(idx
            .attachment_for("777", "y", "", "111", 1_784_951_190_000, &none)
            .is_none());
        // Album is ambiguous by time alone.
        assert!(idx
            .attachment_for("777", "z", "", "333", 1_784_000_001_000, &none)
            .is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Manual check against a real Zalo PC photo: `ZPC_JXL=/path/to/file.jxl cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn imageio_decodes_real_jxl() {
        let Ok(path) = std::env::var("ZPC_JXL") else {
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        assert!(image::load_from_memory(&bytes).is_err());
        let png = decode_to_png_with_imageio(&bytes).expect("ImageIO decodes JPEG XL");
        let img = image::load_from_memory(&png).unwrap();
        assert!(img.width() > 0 && img.height() > 0);
        eprintln!("decoded {}x{}", img.width(), img.height());
    }

    #[test]
    fn parses_names() {
        assert_eq!(
            picture_msg_id("z8090717552569_b0ac.jxl"),
            Some("8090717552569")
        );
        assert_eq!(picture_msg_id("zinstant"), None);
        assert_eq!(
            resource_ids("1784951175547_5929513894379659594_g1_h.jxl"),
            Some((1784951175547, "5929513894379659594"))
        );
        assert_eq!(
            bare_thread("7290848013428486445_group"),
            "7290848013428486445"
        );
        assert_eq!(bare_thread("g123"), "123");
    }
}
