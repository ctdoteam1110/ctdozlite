//! Multi-account management for ctdozlite.
//!
//! Stores isolated account sessions, thread caches, and history databases in `accounts/<uin>.*`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountProfile {
    pub uin: String,
    /// Human-readable label: profile name, else phone number, else a generic label.
    /// Never the raw account id.
    pub display_name: String,
    /// Phone number of the account, empty when unknown.
    pub phone: String,
    pub avatar: Option<String>,
    pub last_active_ts: i64,
    pub is_current: bool,
}

impl AccountProfile {
    /// Secondary line under the label: the phone number when it adds information,
    /// otherwise the account id.
    pub fn subtitle(&self) -> String {
        if !self.phone.is_empty() && self.phone != self.display_name {
            self.phone.clone()
        } else {
            format!("ID: {}", self.uin)
        }
    }
}

/// Label shown for an account that has neither a profile name nor a phone number.
const UNNAMED_ACCOUNT_LABEL: &str = "Tài khoản Zalo";

/// Pick the label of a saved account: profile name, else phone number, else a generic
/// label. A name that merely repeats the account id counts as missing.
fn account_label(uin: &str, display_name: &str, phone: &str) -> String {
    let name = display_name.trim();
    if !name.is_empty() && name != uin {
        return name.to_string();
    }
    let phone = phone.trim();
    if !phone.is_empty() {
        return phone.to_string();
    }
    UNNAMED_ACCOUNT_LABEL.to_string()
}

pub fn accounts_dir() -> PathBuf {
    PathBuf::from("accounts")
}

/// Copy the live `session.json` over the snapshot of `uin` when it is newer.
///
/// The profile name, avatar and phone number are filled in after login, so a snapshot
/// taken earlier would keep labelling the account by its id. Returns `true` when the
/// snapshot was (re)written.
///
/// # Errors
/// Returns an error when the live session cannot be read or the snapshot cannot be written.
pub fn refresh_session_snapshot(uin: &str) -> Result<bool> {
    if !is_safe_uin(uin) {
        return Ok(false);
    }
    let live = Path::new("session.json");
    let snapshot = session_snapshot_path(uin);
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let Some(live_ts) = modified(live) else {
        return Ok(false);
    };
    if modified(&snapshot).is_some_and(|snap_ts| snap_ts >= live_ts) {
        return Ok(false);
    }
    let text = std::fs::read_to_string(live)?;
    // The file on disk may already belong to another account (login in progress).
    let owner = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.pointer("/login/uin").and_then(|u| u.as_str()).map(str::to_string));
    if owner.as_deref() != Some(uin) {
        return Ok(false);
    }
    std::fs::create_dir_all(accounts_dir())?;
    crate::data_dir::write_private_atomic(&snapshot, text.as_bytes())?;
    Ok(true)
}

/// Legacy shared image cache, still used while no account is signed in.
const SHARED_MEDIA_CACHE_DIR: &str = "stickers_cache";

// The media cache is written by worker tasks and probed by the UI when it paints
// avatars, so the active account id is a tiny shared-read value rather than a
// message; the critical section only clones or replaces one short string.
static ACTIVE_UIN: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// `true` when `uin` is safe to embed in a file name (ASCII letters, digits, `-`, `_`).
///
/// Account ids come from `session.json`, which is untrusted input for path building.
pub fn is_safe_uin(uin: &str) -> bool {
    !uin.is_empty()
        && uin.len() <= 64
        && uin
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Record which account owns the media written from now on (empty = signed out).
pub fn set_active_uin(uin: &str) {
    let next = if is_safe_uin(uin) { uin } else { "" };
    if let Ok(mut guard) = ACTIVE_UIN.write() {
        if guard.as_str() != next {
            *guard = next.to_string();
        }
    }
}

/// The account that currently owns the UI, or an empty string when signed out.
pub fn active_uin() -> String {
    ACTIVE_UIN.read().map(|g| g.clone()).unwrap_or_default()
}

/// Saved session snapshot of `uin` (`accounts/<uin>.json`).
pub fn session_snapshot_path(uin: &str) -> PathBuf {
    accounts_dir().join(format!("{uin}.json"))
}

/// Saved thread-list snapshot of `uin` (`accounts/<uin>_threads.json`).
pub fn threads_snapshot_path(uin: &str) -> PathBuf {
    accounts_dir().join(format!("{uin}_threads.json"))
}

/// Private storage root of one account (`accounts/<uin>/`).
pub fn account_dir(uin: &str) -> PathBuf {
    accounts_dir().join(uin)
}

/// Image cache directory of `uin`, or the shared legacy cache for an empty/unsafe id.
///
/// The directory is not created; use [`ensure_media_cache_dir`] before writing.
pub fn media_cache_dir_for(uin: &str) -> PathBuf {
    if is_safe_uin(uin) {
        account_dir(uin).join("media_cache")
    } else {
        PathBuf::from(SHARED_MEDIA_CACHE_DIR)
    }
}

/// Image cache directory of the active account (avatars, thumbnails, stickers).
pub fn media_cache_dir() -> PathBuf {
    media_cache_dir_for(&active_uin())
}

/// [`media_cache_dir`], created on demand so callers can write into it directly.
pub fn ensure_media_cache_dir() -> PathBuf {
    let dir = media_cache_dir();
    // Best effort: a missing cache directory only costs a re-download later.
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Folder that receives files downloaded by `uin`, nested under the user's download root
/// so attachments of different accounts never mix.
pub fn account_download_dir(base: &Path, uin: &str) -> PathBuf {
    if is_safe_uin(uin) {
        base.join(uin)
    } else {
        base.to_path_buf()
    }
}

/// List all saved accounts in the `accounts/` directory.
pub fn list_saved_accounts(current_uin: &str) -> Vec<AccountProfile> {
    let dir = accounts_dir();
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            if file_stem.ends_with("_threads") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(login) = val.get("login") {
                        let uin = login
                            .get("uin")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        if !uin.is_empty() {
                            let phone = login
                                .get("phone")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .trim()
                                .to_string();
                            let display_name = account_label(
                                &uin,
                                login
                                    .get("display_name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default(),
                                &phone,
                            );
                            let avatar = login
                                .get("avatar")
                                .and_then(|v| v.as_str())
                                .map(str::to_string);
                            let is_current = uin == current_uin;
                            let last_active_ts = entry
                                .metadata()
                                .and_then(|m| m.modified())
                                .ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_secs() as i64)
                                .unwrap_or(0);
                            out.push(AccountProfile {
                                uin,
                                display_name,
                                phone,
                                avatar,
                                last_active_ts,
                                is_current,
                            });
                        }
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| b.last_active_ts.cmp(&a.last_active_ts));
    out
}

/// Save the current session and caches into `accounts/<uin>.*`
pub fn save_current_account_snapshot(uin: &str) -> Result<()> {
    if uin.is_empty() {
        return Ok(());
    }
    let dir = accounts_dir();
    std::fs::create_dir_all(&dir)?;
    if Path::new("session.json").exists() {
        let _ = std::fs::copy("session.json", dir.join(format!("{uin}.json")));
    }
    if Path::new("threads_cache.json").exists() {
        let _ = std::fs::copy("threads_cache.json", dir.join(format!("{uin}_threads.json")));
    }
    if Path::new("zalo_history.db").exists() {
        let _ = std::fs::copy("zalo_history.db", dir.join(format!("{uin}_history.db")));
    }
    Ok(())
}

/// Switch to a target account:
/// 1. Saves current account snapshot to accounts/<current_uin>.*
/// 2. Restores accounts/<target_uin>.* into session.json, threads_cache.json, zalo_history.db
pub fn switch_to_account(current_uin: &str, target_uin: &str) -> Result<()> {
    if target_uin.is_empty() {
        anyhow::bail!("Target UIN is empty");
    }
    let dir = accounts_dir();
    if !current_uin.is_empty() {
        let _ = save_current_account_snapshot(current_uin);
    }
    let _ = std::fs::remove_file("session.json");
    let _ = std::fs::remove_file("threads_cache.json");
    let _ = std::fs::remove_file("zalo_history.db");

    let target_session = dir.join(format!("{target_uin}.json"));
    if !target_session.exists() {
        anyhow::bail!("Không tìm thấy dữ liệu phiên của tài khoản {target_uin}");
    }
    std::fs::copy(&target_session, "session.json")?;

    let target_threads = dir.join(format!("{target_uin}_threads.json"));
    if target_threads.exists() {
        let _ = std::fs::copy(&target_threads, "threads_cache.json");
    }

    let target_db = dir.join(format!("{target_uin}_history.db"));
    if target_db.exists() {
        let _ = std::fs::copy(&target_db, "zalo_history.db");
    }
    Ok(())
}

/// Delete a saved account from `accounts/`
pub fn delete_saved_account(uin: &str) -> Result<()> {
    if uin.is_empty() {
        return Ok(());
    }
    let dir = accounts_dir();
    let _ = std::fs::remove_file(dir.join(format!("{uin}.json")));
    let _ = std::fs::remove_file(dir.join(format!("{uin}_threads.json")));
    let _ = std::fs::remove_file(dir.join(format!("{uin}_history.db")));
    if is_safe_uin(uin) {
        // Best effort: the per-account folder only holds re-downloadable media cache.
        let _ = std::fs::remove_dir_all(account_dir(uin));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_cache_is_scoped_per_account() {
        assert_eq!(
            media_cache_dir_for("1001"),
            PathBuf::from("accounts").join("1001").join("media_cache")
        );
        assert_ne!(media_cache_dir_for("1001"), media_cache_dir_for("1002"));
    }

    #[test]
    fn media_cache_falls_back_to_shared_dir_when_uin_is_empty_or_unsafe() {
        assert_eq!(media_cache_dir_for(""), PathBuf::from("stickers_cache"));
        assert_eq!(
            media_cache_dir_for("../escape"),
            PathBuf::from("stickers_cache")
        );
    }

    #[test]
    fn unsafe_uin_is_rejected_for_path_building() {
        assert!(is_safe_uin("user-1_a"));
        for bad in ["", "..", "a/b", "a\\b", "a b", "a.b"] {
            assert!(!is_safe_uin(bad), "accepted {bad:?}");
        }
        assert!(!is_safe_uin(&"9".repeat(65)));
    }

    #[test]
    fn account_label_prefers_name_then_phone_and_never_shows_the_id() {
        assert_eq!(account_label("1001", "An", "0900000001"), "An");
        assert_eq!(account_label("1001", "  ", "0900000001"), "0900000001");
        assert_eq!(account_label("1001", "1001", "0900000001"), "0900000001");
        assert_eq!(account_label("1001", "", ""), "Tài khoản Zalo");
        assert_eq!(account_label("1001", "1001", ""), "Tài khoản Zalo");
    }

    #[test]
    fn subtitle_shows_phone_only_when_it_is_not_already_the_label() {
        let mut profile = AccountProfile {
            uin: "1001".into(),
            display_name: "An".into(),
            phone: "0900000001".into(),
            avatar: None,
            last_active_ts: 0,
            is_current: false,
        };
        assert_eq!(profile.subtitle(), "0900000001");
        profile.display_name = "0900000001".into();
        assert_eq!(profile.subtitle(), "ID: 1001");
        profile.phone.clear();
        assert_eq!(profile.subtitle(), "ID: 1001");
    }

    #[test]
    fn download_dir_is_nested_per_account() {
        let base = Path::new("downloads");
        assert_eq!(account_download_dir(base, "1001"), base.join("1001"));
        assert_eq!(account_download_dir(base, ""), base.to_path_buf());
        assert_eq!(account_download_dir(base, "../x"), base.to_path_buf());
    }
}
