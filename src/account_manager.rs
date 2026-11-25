//! Multi-account management for ctdozlite.
//!
//! Stores isolated account sessions, thread caches, and history databases in `accounts/<uin>.*`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Validate an account ID before using it as an app-owned file name.
///
/// # Errors
/// Rejects empty IDs and characters that can escape or alter an account path.
pub fn validate_uin(uin: &str) -> Result<()> {
    if uin.is_empty()
        || uin.len() > 128
        || !uin
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        anyhow::bail!("Mã tài khoản không hợp lệ");
    }
    Ok(())
}

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
static ACCOUNT_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

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

/// Record which account owns the UI and media written from now on (empty = signed out).
/// Bumps [`account_epoch`] whenever the active account changes so in-flight async tasks
/// from the previous account can be discarded cleanly.
pub fn set_active_uin(uin: &str) {
    let next = if is_safe_uin(uin) { uin } else { "" };
    if let Ok(mut guard) = ACTIVE_UIN.write() {
        if guard.as_str() != next {
            *guard = next.to_string();
            ACCOUNT_EPOCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

/// Monotonic generation counter incremented every time [`set_active_uin`] switches account.
pub fn account_epoch() -> u64 {
    ACCOUNT_EPOCH.load(std::sync::atomic::Ordering::SeqCst)
}

/// The account that currently owns the UI, or an empty string when signed out.
pub fn active_uin() -> String {
    ACTIVE_UIN.read().map(|g| g.clone()).unwrap_or_default()
}

/// Per-account Sync V2 Curve25519 identity key path (`accounts/<uin>/sync_identity.json`).
pub fn sync_identity_path_for(uin: &str) -> PathBuf {
    if is_safe_uin(uin) {
        let dir = account_dir(uin);
        let _ = std::fs::create_dir_all(&dir);
        dir.join("sync_identity.json")
    } else {
        PathBuf::from("sync_identity.json")
    }
}

/// Per-account Sync V1 RSA-2048 identity key path (`accounts/<uin>/sync_rsa_identity.json`).
pub fn sync_rsa_path_for(uin: &str) -> PathBuf {
    if is_safe_uin(uin) {
        let dir = account_dir(uin);
        let _ = std::fs::create_dir_all(&dir);
        dir.join("sync_rsa_identity.json")
    } else {
        PathBuf::from("sync_rsa_identity.json")
    }
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

/// The OS user's own Downloads folder, where photos and videos saved from a chat land.
///
/// Linux honours `XDG_DOWNLOAD_DIR` from `user-dirs.dirs`, so a localised folder name
/// (e.g. `~/Tải về`) is respected; other platforms use `<home>/Downloads`. Falls back to the
/// home directory, then the working directory, when no Downloads folder can be resolved.
pub fn os_downloads_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    #[cfg(target_os = "linux")]
    if let Some(home) = &home {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        if let Ok(contents) = std::fs::read_to_string(config.join("user-dirs.dirs")) {
            if let Some(dir) = parse_xdg_download_dir(&contents, home) {
                return dir;
            }
        }
    }
    match home {
        Some(home) => {
            let downloads = home.join("Downloads");
            if downloads.is_dir() {
                downloads
            } else {
                home
            }
        }
        None => PathBuf::from("."),
    }
}

/// Extract `XDG_DOWNLOAD_DIR` from a `user-dirs.dirs` file. The spec only allows `$HOME/...`
/// or absolute paths; a value equal to `$HOME` itself means "disabled" and is ignored.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_xdg_download_dir(contents: &str, home: &Path) -> Option<PathBuf> {
    let value = contents
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| l.strip_prefix("XDG_DOWNLOAD_DIR="))?
        .trim()
        .trim_matches('"');
    let path = if let Some(rest) = value.strip_prefix("$HOME") {
        let rest = rest.trim_start_matches('/');
        if rest.is_empty() {
            return None;
        }
        home.join(rest)
    } else if value.starts_with('/') {
        PathBuf::from(value)
    } else {
        return None;
    };
    Some(path)
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
            let file_stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
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

/// Remove legacy root cache/DB files (`threads_cache.json`, `zalo_history.db` + WAL/SHM)
/// so multi-account switches or QR logins never inherit another account's data.
pub fn remove_legacy_shared_caches() {
    for legacy in [
        "threads_cache.json",
        "zalo_history.db",
        "zalo_history.db-wal",
        "zalo_history.db-shm",
    ] {
        let _ = std::fs::remove_file(legacy);
    }
}

/// Preserve the active session while keeping history in its canonical account database.
///
/// # Errors
/// Returns an error for an invalid account, session ownership mismatch, or persistence failure.
pub fn save_current_account_snapshot(uin: &str) -> Result<()> {
    save_current_account_snapshot_in(Path::new("."), uin)
}

/// Return the original session bytes after verifying the saved login belongs to `uin`.
///
/// # Errors
/// Returns an error if the file is unreadable, malformed, or belongs to another account.
fn read_owned_session(path: &Path, uin: &str) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path)?;
    let saved: crate::session::SavedSession = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Dữ liệu phiên tài khoản không hợp lệ"))?;
    if saved.login.as_ref().map(|login| login.uin.as_str()) != Some(uin) {
        anyhow::bail!("Phiên không thuộc tài khoản đã chọn");
    }
    Ok(bytes)
}

/// Prepare the account history store before atomically saving a copy of its active session.
///
/// # Errors
/// Returns an error if the account or session is invalid, or if local persistence fails.
fn save_current_account_snapshot_in(root: &Path, uin: &str) -> Result<()> {
    validate_uin(uin)?;
    let session_path = root.join("session.json");
    let session = read_owned_session(&session_path, uin)?;
    std::fs::create_dir_all(root.join("accounts"))?;
    crate::store::MessageStore::open_account_in(root, uin)?;
    crate::data_dir::write_private_atomic(&root.join(format!("accounts/{uin}.json")), &session)
}

/// Validate the target session before preserving or replacing the active session.
///
/// # Errors
/// Returns an error if either session is invalid or persistence fails.
pub fn switch_to_account(current_uin: &str, target_uin: &str) -> Result<()> {
    switch_to_account_in(Path::new("."), current_uin, target_uin)?;
    set_active_uin(target_uin);
    Ok(())
}

/// Switch accounts below an explicit application root, keeping history files isolated.
///
/// # Errors
/// Returns an error before replacing the active session if target validation or snapshotting fails.
pub fn switch_to_account_in(root: &Path, current_uin: &str, target_uin: &str) -> Result<()> {
    validate_uin(target_uin)?;
    let target = read_owned_session(
        &root.join(format!("accounts/{target_uin}.json")),
        target_uin,
    )?;
    if !current_uin.is_empty() {
        save_current_account_snapshot_in(root, current_uin)?;
    }
    // Canonical databases and caches stay in accounts/; never copy live SQLite files.
    remove_if_exists(&root.join("threads_cache.json"))?;
    crate::data_dir::write_private_atomic(&root.join("session.json"), &target)
}

/// Remove the active and saved session on explicit logout, retaining account history.
///
/// # Errors
/// Returns an error if a session cannot be removed or the account ID is invalid.
pub fn logout_account(uin: &str) -> Result<()> {
    logout_account_in(Path::new("."), uin)
}

/// Remove account credentials below an explicit application root while preserving history.
///
/// # Errors
/// Returns an error if a session cannot be removed or the account ID is invalid.
pub fn logout_account_in(root: &Path, uin: &str) -> Result<()> {
    if !uin.is_empty() {
        validate_uin(uin)?;
        remove_if_exists(&root.join(format!("accounts/{uin}.json")))?;
    }
    remove_if_exists(&root.join("session.json"))
}

/// Remove a session artifact idempotently so interrupted cleanup can be retried.
///
/// # Errors
/// Returns filesystem errors other than a missing file.
fn remove_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Delete a saved account and all of its isolated history/cache/sync files from `accounts/`
pub fn delete_saved_account(uin: &str) -> Result<()> {
    if !is_safe_uin(uin) {
        return Ok(());
    }
    let dir = accounts_dir();
    let _ = std::fs::remove_file(dir.join(format!("{uin}.json")));
    let _ = std::fs::remove_file(dir.join(format!("{uin}_threads.json")));
    let _ = std::fs::remove_file(dir.join(format!("{uin}_history.db")));
    let _ = std::fs::remove_file(dir.join(format!("{uin}_history.db-wal")));
    let _ = std::fs::remove_file(dir.join(format!("{uin}_history.db-shm")));
    let _ = std::fs::remove_dir_all(account_dir(uin));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_download_dir_resolves_home_relative_and_absolute_values() {
        let home = Path::new("/home/u");
        let file = "# comment\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_DOWNLOAD_DIR=\"$HOME/Tải về\"\n";
        assert_eq!(
            parse_xdg_download_dir(file, home),
            Some(PathBuf::from("/home/u/Tải về"))
        );
        assert_eq!(
            parse_xdg_download_dir("XDG_DOWNLOAD_DIR=\"/data/dl\"", home),
            Some(PathBuf::from("/data/dl"))
        );
    }

    #[test]
    fn xdg_download_dir_ignores_missing_disabled_and_relative_values() {
        let home = Path::new("/home/u");
        assert_eq!(parse_xdg_download_dir("XDG_MUSIC_DIR=\"$HOME/Music\"", home), None);
        assert_eq!(parse_xdg_download_dir("XDG_DOWNLOAD_DIR=\"$HOME/\"", home), None);
        assert_eq!(parse_xdg_download_dir("XDG_DOWNLOAD_DIR=\"dl\"", home), None);
        assert_eq!(parse_xdg_download_dir("#XDG_DOWNLOAD_DIR=\"/x\"", home), None);
    }

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

    #[test]
    fn per_account_db_and_sync_key_paths_enforce_safe_uin() {
        assert_eq!(
            crate::store::db_path_for_account("../escape"),
            "zalo_history.db"
        );
        assert_eq!(crate::store::db_path_for_account(""), "zalo_history.db");
        assert_eq!(
            sync_identity_path_for("../escape"),
            PathBuf::from("sync_identity.json")
        );
        assert_eq!(
            sync_rsa_path_for("../escape"),
            PathBuf::from("sync_rsa_identity.json")
        );
        assert_eq!(sync_identity_path_for(""), PathBuf::from("sync_identity.json"));
        let test_uin = "test_isolation_999";
        let id_path = sync_identity_path_for(test_uin);
        let rsa_path = sync_rsa_path_for(test_uin);
        assert_eq!(
            id_path,
            PathBuf::from("accounts").join(test_uin).join("sync_identity.json")
        );
        assert_eq!(
            rsa_path,
            PathBuf::from("accounts").join(test_uin).join("sync_rsa_identity.json")
        );
        let _ = std::fs::remove_dir_all(account_dir(test_uin));
    }

    fn root() -> PathBuf {
        static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ctdozlite-account-test-{}-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(path.join("accounts")).unwrap();
        path
    }

    fn session_file(path: &Path, uin: &str) {
        let session = crate::session::Session::new(false, false, false).unwrap();
        let saved = crate::session::SavedSession {
            imei: "synthetic-device".into(),
            computer_name: "fixture".into(),
            secret_key_b64: None,
            session_key: None,
            zpw_sek: None,
            login: Some(crate::session::LoginInfo {
                uin: uin.into(),
                ..Default::default()
            }),
            domains: session.domains,
            expired_at: None,
            proxy_url: None,
        };
        std::fs::write(path, serde_json::to_vec(&saved).unwrap()).unwrap();
    }

    #[test]
    fn logout_removes_both_sessions_and_preserves_history() {
        let root = root();
        session_file(&root.join("session.json"), "user-a");
        session_file(&root.join("accounts/user-a.json"), "user-a");
        session_file(&root.join("accounts/user-b.json"), "user-b");
        std::fs::write(root.join("accounts/user-a_history.db"), b"retained history").unwrap();
        logout_account_in(&root, "user-a").unwrap();
        assert!(!root.join("session.json").exists());
        assert!(!root.join("accounts/user-a.json").exists());
        assert!(root.join("accounts/user-b.json").exists());
        assert_eq!(
            std::fs::read(root.join("accounts/user-a_history.db")).unwrap(),
            b"retained history"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn switch_preserves_current_session_when_target_is_missing_or_wrong_owner() {
        let root = root();
        session_file(&root.join("session.json"), "user-a");
        let original = std::fs::read(root.join("session.json")).unwrap();
        assert!(switch_to_account_in(&root, "user-a", "user-b").is_err());
        assert_eq!(std::fs::read(root.join("session.json")).unwrap(), original);
        session_file(&root.join("accounts/user-b.json"), "user-c");
        assert!(switch_to_account_in(&root, "user-a", "user-b").is_err());
        assert_eq!(std::fs::read(root.join("session.json")).unwrap(), original);
        assert!(!root.join("accounts/user-a.json").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    /// Verify that malformed account IDs leave the account directory empty.
    fn account_open_rejects_path_traversal_before_creating_files() {
        let root = root();
        for uin in ["", "../outside", "..\\outside", "C:outside", "user/a"] {
            assert!(crate::store::MessageStore::open_account_in(&root, uin).is_err());
        }
        assert_eq!(std::fs::read_dir(root.join("accounts")).unwrap().count(), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    /// Exercise a switch with preexisting history for both accounts and an owned legacy database.
    fn switch_imports_owned_legacy_without_overwriting_canonical_history() {
        let root = root();
        session_file(&root.join("session.json"), "user-a");
        session_file(&root.join("accounts/user-b.json"), "user-b");
        for (path, msg_id) in [
            ("zalo_history.db", "msg-legacy-a"),
            ("accounts/user-a_history.db", "msg-canonical-a"),
            ("accounts/user-b_history.db", "msg-canonical-b"),
        ] {
            let store =
                crate::store::MessageStore::open(root.join(path).to_str().unwrap()).unwrap();
            store.conn.execute("INSERT INTO messages(msg_id, thread_id, ts, is_outgoing) VALUES (?1, 'thread-1', 1, 0)", [msg_id]).unwrap();
            if path == "zalo_history.db" {
                store.conn.execute("INSERT INTO account_store_metadata(key,value) VALUES ('legacy_owner','user-a')", []).unwrap();
            }
        }
        switch_to_account_in(&root, "user-a", "user-b").unwrap();
        let active: crate::session::SavedSession =
            serde_json::from_slice(&std::fs::read(root.join("session.json")).unwrap()).unwrap();
        assert_eq!(active.login.unwrap().uin, "user-b");
        let saved: crate::session::SavedSession =
            serde_json::from_slice(&std::fs::read(root.join("accounts/user-a.json")).unwrap())
                .unwrap();
        assert_eq!(saved.login.unwrap().uin, "user-a");
        let a = crate::store::MessageStore::open_account_in(&root, "user-a").unwrap();
        assert_eq!(a.messages_for("thread-1", 10).unwrap().len(), 2);
        let b = crate::store::MessageStore::open_account_in(&root, "user-b").unwrap();
        let b_messages = b.messages_for("thread-1", 10).unwrap();
        assert_eq!(b_messages.len(), 1);
        assert_eq!(b_messages[0].msg_id, "msg-canonical-b");
        drop(a);
        drop(b);
        std::fs::remove_dir_all(root).unwrap();
    }
}
