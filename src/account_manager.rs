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
    pub display_name: String,
    pub avatar: Option<String>,
    pub last_active_ts: i64,
    pub is_current: bool,
}

pub fn accounts_dir() -> PathBuf {
    PathBuf::from("accounts")
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
                            let display_name = login
                                .get("display_name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Người dùng Zalo")
                                .to_string();
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

/// Preserve the active session while keeping history in its canonical account database.
///
/// # Errors
/// Returns an error for an invalid account, session ownership mismatch, or persistence failure.
pub fn save_current_account_snapshot(uin: &str) -> Result<()> {
    save_current_account_snapshot_in(Path::new("."), uin)
}

fn read_owned_session(path: &Path, uin: &str) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path)?;
    let saved: crate::session::SavedSession = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Dữ liệu phiên tài khoản không hợp lệ"))?;
    if saved.login.as_ref().map(|login| login.uin.as_str()) != Some(uin) {
        anyhow::bail!("Phiên không thuộc tài khoản đã chọn");
    }
    Ok(bytes)
}

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
    switch_to_account_in(Path::new("."), current_uin, target_uin)
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

fn remove_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ctdozlite-account-test-{}-{}",
            std::process::id(),
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
    fn account_open_rejects_path_traversal_before_creating_files() {
        let root = root();
        for uin in ["", "../outside", "..\\outside", "C:outside", "user/a"] {
            assert!(crate::store::MessageStore::open_account_in(&root, uin).is_err());
        }
        assert_eq!(std::fs::read_dir(root.join("accounts")).unwrap().count(), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn switch_claims_legacy_for_current_account_without_overwriting_canonical_history() {
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
