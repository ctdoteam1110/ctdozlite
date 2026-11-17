//! Select writable storage before opening sessions, caches, logs, or databases.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

/// Where the default (non `--data-dir`) storage lives on each platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform<'a> {
    /// `~/Library/Application Support/ctdozlite`.
    MacOs,
    /// `$XDG_DATA_HOME/ctdozlite`, else `~/.local/share/ctdozlite`.
    Linux { xdg_data_home: Option<&'a Path> },
    /// The working directory (legacy behaviour, kept for Windows).
    Other,
}

/// Resolve paths without changing process state, including in parallel tests.
///
/// # Errors
/// Returns an error for invalid storage options or a missing/relative HOME on macOS/Linux.
pub fn resolve(
    args: &[String],
    cwd: &Path,
    executable: &Path,
    home: Option<&Path>,
    platform: Platform<'_>,
) -> Result<(PathBuf, bool)> {
    let mut explicit = None;
    let mut profile_name = None;
    let mut i = 0;
    while i < args.len() {
        let value = if args[i] == "--data-dir" {
            i += 1;
            Some(
                args.get(i)
                    .filter(|value| !value.starts_with('-'))
                    .context("--data-dir yêu cầu đường dẫn")?
                    .as_str(),
            )
        } else {
            args[i].strip_prefix("--data-dir=")
        };
        if let Some(value) = value {
            if value.is_empty() {
                bail!("--data-dir yêu cầu đường dẫn không rỗng");
            }
            if explicit.is_some() {
                bail!("--data-dir chỉ được chỉ định một lần");
            }
            explicit = Some(PathBuf::from(value));
            i += 1;
            continue;
        }
        let prof_val = if args[i] == "--profile" {
            i += 1;
            Some(
                args.get(i)
                    .filter(|value| !value.starts_with('-'))
                    .context("--profile yêu cầu tên hồ sơ tài khoản")?
                    .as_str(),
            )
        } else {
            args[i].strip_prefix("--profile=")
        };
        if let Some(prof) = prof_val {
            let safe: String = prof
                .trim()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if safe.is_empty() {
                bail!("--profile chỉ chấp nhận ký tự chữ, số, '-' hoặc '_'");
            }
            profile_name = Some(safe);
        }
        i += 1;
    }
    if let Some(path) = explicit {
        return Ok((
            if path.is_absolute() || path.to_str().is_some_and(|s| s.starts_with('/')) {
                path
            } else {
                cwd.join(path)
            },
            true,
        ));
    }
    // macOS and Linux: GUI, CLI, TUI, agent and MCP share one per-user folder whatever the
    // working directory, so launching from the app menu or a terminal sees the same session
    // and SQLite. `--data-dir` still wins. Windows keeps the working directory.
    let _ = executable;
    let absolute_home = || -> Result<&Path> {
        let home = home.context("Cần HOME để xác định thư mục dữ liệu")?;
        if !home.is_absolute() && !home.to_str().is_some_and(|s| s.starts_with('/')) {
            bail!("HOME phải là đường dẫn tuyệt đối");
        }
        Ok(home)
    };
    let (base, is_private) = match platform {
        Platform::MacOs => (
            absolute_home()?.join("Library/Application Support/ctdozlite"),
            true,
        ),
        Platform::Linux { xdg_data_home } => {
            // The XDG spec says a relative XDG_DATA_HOME is invalid and must be ignored.
            let data_home = match xdg_data_home.filter(|p| p.is_absolute()) {
                Some(dir) => dir.to_path_buf(),
                None => absolute_home()?.join(".local/share"),
            };
            (data_home.join("ctdozlite"), true)
        }
        Platform::Other => (cwd.to_path_buf(), false),
    };
    if let Some(prof) = profile_name {
        return Ok((base.join("profiles").join(prof), true));
    }
    Ok((base, is_private))
}

/// Validate storage before any persistent state is opened.
///
/// # Errors
/// Returns an error if creation, permissions, or the writable probe fails.
pub fn prepare(path: &Path, private: bool) -> Result<()> {
    if private {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
            let mut builder = std::fs::DirBuilder::new();
            builder
                .recursive(true)
                .mode(0o700)
                .create(path)
                .with_context(|| format!("Không tạo được thư mục dữ liệu {}", path.display()))?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).with_context(
                || format!("Không đặt được quyền thư mục dữ liệu {}", path.display()),
            )?;
        }
        #[cfg(not(unix))]
        std::fs::create_dir_all(path)
            .with_context(|| format!("Không tạo được thư mục dữ liệu {}", path.display()))?;
    }
    let (probe_path, probe) = create_private_file(path, ".storage-probe")
        .with_context(|| format!("Thư mục dữ liệu không ghi được: {}", path.display()))?;
    drop(probe);
    std::fs::remove_file(&probe_path).context("Không xóa được tệp kiểm tra quyền ghi")?;
    Ok(())
}

/// Select storage once before workers or persistent state are opened.
///
/// # Errors
/// Returns an error when process paths or writable storage cannot be selected.
pub fn initialize(args: &[String]) -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("Không xác định được thư mục làm việc")?;
    let executable =
        std::env::current_exe().context("Không xác định được đường dẫn chương trình")?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let platform = if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(target_os = "linux") {
        Platform::Linux {
            xdg_data_home: xdg_data_home.as_deref(),
        }
    } else {
        Platform::Other
    };
    let (path, private) = resolve(args, &cwd, &executable, home.as_deref(), platform)?;
    if matches!(platform, Platform::Linux { .. }) && !has_explicit_data_dir(args) {
        // Before 0.1.3 Linux stored everything in the working directory, so data sits in
        // whichever folder the app was started from (usually HOME for menu launches).
        let candidates = legacy_candidates(&path, &cwd, home.as_deref());
        match migrate_legacy_data(&path, &candidates) {
            Ok(Some(from)) => eprintln!(
                "📦 Đã chuyển dữ liệu cũ từ {} sang {} (bản gốc vẫn được giữ nguyên)",
                from.display(),
                path.display()
            ),
            Ok(None) => {}
            Err(e) => eprintln!("⚠️ Không chuyển được dữ liệu cũ sang {}: {e:#}", path.display()),
        }
    }
    prepare(&path, private)?;
    // Legacy storage callers use relative paths; switch once before creating workers.
    std::env::set_current_dir(&path)
        .with_context(|| format!("Không chọn được thư mục dữ liệu {}", path.display()))?;
    Ok(path)
}

fn has_explicit_data_dir(args: &[String]) -> bool {
    args.iter()
        .any(|a| a == "--data-dir" || a.starts_with("--data-dir="))
}

/// Folders that may hold pre-XDG data for `target`: the working directory and HOME, each
/// with the same `profiles/<name>` suffix when `target` is a profile folder.
fn legacy_candidates(target: &Path, cwd: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let profile_suffix = target
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "profiles"))
        .and(target.file_name())
        .map(|name| Path::new("profiles").join(name));
    let mut out: Vec<PathBuf> = Vec::new();
    for base in std::iter::once(cwd).chain(home) {
        let dir = match &profile_suffix {
            Some(suffix) => base.join(suffix),
            None => base.to_path_buf(),
        };
        if dir != target && !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// Top-level entries that make up a data folder. Logs and crash dumps stay behind.
fn is_data_entry(name: &str) -> bool {
    const NAMES: [&str; 15] = [
        "zalo_history.db",
        "zalo_history.db-wal",
        "zalo_history.db-shm",
        "app_settings.json",
        "theme.json",
        "custom_stickers.json",
        "autoreply_rules.json",
        "sync_identity.json",
        "sync_rsa_identity.json",
        "store_packs.json",
        "suggest_keywords_v2.json",
        "accounts",
        "stickers_cache",
        "bridge",
        "profiles",
    ];
    NAMES.contains(&name)
        || (name.starts_with("session") && name.ends_with(".json"))
        || (name.starts_with("threads_cache") && name.ends_with(".json"))
}

/// A folder counts as holding app data when it has a session, a history DB or accounts.
fn has_app_data(dir: &Path) -> bool {
    ["session.json", "zalo_history.db", "accounts"]
        .iter()
        .any(|name| dir.join(name).exists())
}

/// Newest modification time among the markers, used to pick between several legacy folders.
fn data_mtime(dir: &Path) -> std::time::SystemTime {
    ["session.json", "zalo_history.db", "accounts", "app_settings.json"]
        .iter()
        .filter_map(|name| std::fs::metadata(dir.join(name)).ok()?.modified().ok())
        .max()
        .unwrap_or(std::time::UNIX_EPOCH)
}

/// Copy legacy data into a new, empty `target`. Picks the most recently used candidate,
/// copies into a sibling staging folder, then renames it into place so a failed copy never
/// leaves a half-filled `target`. The source is left untouched.
///
/// Returns the folder the data came from, or `None` when there was nothing to migrate
/// (target already in use, or no candidate holds app data).
///
/// # Errors
/// Returns an error when copying or the final rename fails; `target` is then left as it was.
pub fn migrate_legacy_data(target: &Path, candidates: &[PathBuf]) -> Result<Option<PathBuf>> {
    let target_is_empty = match std::fs::read_dir(target) {
        Ok(mut entries) => entries.next().is_none(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => return Err(e).context("Không đọc được thư mục dữ liệu mới"),
    };
    if !target_is_empty {
        return Ok(None);
    }
    let Some(source) = candidates
        .iter()
        .filter(|dir| has_app_data(dir))
        .max_by_key(|dir| data_mtime(dir))
    else {
        return Ok(None);
    };
    let parent = target
        .parent()
        .context("Thư mục dữ liệu mới không có thư mục cha")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("Không tạo được {}", parent.display()))?;
    let staging = parent.join(format!(
        ".ctdozlite-migrating-{}-{}",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> Result<()> {
        std::fs::create_dir(&staging)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            let name = entry.file_name();
            if !name.to_str().is_some_and(is_data_entry) {
                continue;
            }
            copy_tree(&entry.path(), &staging.join(&name))?;
        }
        if target.exists() {
            // Known empty from the check above.
            std::fs::remove_dir(target)?;
        }
        std::fs::rename(&staging, target)?;
        Ok(())
    })();
    if let Err(e) = result {
        // Best effort: the staging copy is disposable; keep the original error.
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e).with_context(|| format!("Chuyển dữ liệu từ {} thất bại", source.display()));
    }
    Ok(Some(source.clone()))
}

/// Recursive copy that keeps file permissions and skips symlinks (never followed out of the
/// data folder).
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        std::fs::create_dir(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        std::fs::set_permissions(to, meta.permissions())?;
    } else {
        std::fs::copy(from, to)?;
    }
    Ok(())
}

fn create_private_file(parent: &Path, prefix: &str) -> std::io::Result<(PathBuf, std::fs::File)> {
    for _ in 0..100 {
        let path = parent.join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "Không tạo được tệp kiểm tra quyền ghi",
    ))
}

/// Replace a session only after the complete private temporary file is flushed.
///
/// # Errors
/// Returns an error if creating, writing, flushing, or replacing the file fails.
pub fn write_private_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let (temporary, mut file) = create_private_file(parent, ".session-tmp")
        .with_context(|| format!("Cannot create temporary session in {}", parent.display()))?;
    let result = (|| -> Result<()> {
        file.write_all(contents).context("Cannot write session")?;
        file.sync_all().context("Cannot flush session")?;
        drop(file);
        std::fs::rename(&temporary, path)
            .with_context(|| format!("Cannot replace session {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        // Preserve the original failure if best-effort temporary cleanup also fails.
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ctdozlite-storage-test-{}-{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn resolves_bundle_cli_and_explicit_paths() {
        let cwd = Path::new("/work");
        let exe = Path::new("/Applications/Zalo.app/Contents/MacOS/ctdozlite");
        assert_eq!(
            resolve(&[], cwd, exe, Some(Path::new("/Users/fixture")), Platform::MacOs).unwrap(),
            (
                PathBuf::from("/Users/fixture/Library/Application Support/ctdozlite"),
                true
            )
        );
        assert_eq!(
            resolve(&[], cwd, Path::new("/bin/ctdozlite"), Some(Path::new("/Users/fixture")), Platform::MacOs).unwrap(),
            (
                PathBuf::from("/Users/fixture/Library/Application Support/ctdozlite"),
                true
            )
        );
        assert_eq!(
            resolve(&[], cwd, exe, None, Platform::Other).unwrap(),
            (cwd.to_path_buf(), false)
        );
        assert_eq!(
            resolve(
                &["--data-dir".into(), "fixture".into()],
                cwd,
                exe,
                None,
                Platform::MacOs
            )
            .unwrap(),
            (cwd.join("fixture"), true)
        );
        assert!(resolve(&[], cwd, exe, None, Platform::MacOs).is_err());
        assert!(resolve(&["--data-dir".into()], cwd, exe, None, Platform::MacOs).is_err());
        assert!(resolve(&["--data-dir=".into()], cwd, exe, None, Platform::MacOs).is_err());
        assert!(resolve(
            &["--data-dir=a".into(), "--data-dir=b".into()],
            cwd,
            exe,
            None,
            Platform::MacOs
        )
        .is_err());
        assert_eq!(
            resolve(
                &["--profile".into(), "acc_2".into()],
                cwd,
                Path::new("/bin/ctdozlite"),
                Some(Path::new("/Users/fixture")),
                Platform::MacOs
            )
            .unwrap(),
            (
                PathBuf::from("/Users/fixture/Library/Application Support/ctdozlite/profiles/acc_2"),
                true
            )
        );
    }

    #[test]
    fn preparation_retains_existing_data_and_secures_directory() {
        let root = fixture();
        let path = root.join("storage");
        prepare(&path, true).unwrap();
        std::fs::write(path.join("session.json"), b"existing fixture").unwrap();
        prepare(&path, true).unwrap();
        assert_eq!(
            std::fs::read(path.join("session.json")).unwrap(),
            b"existing fixture"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 1);
        assert!(prepare(&path.join("session.json"), true).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn explicit_path_overrides_bundle_and_relative_home_is_rejected() {
        let cwd = Path::new("/work");
        let exe = Path::new("/Applications/Fixture.app/Contents/MacOS/ctdozlite");
        assert_eq!(
            resolve(&["--data-dir=/fixture-data".into()], cwd, exe, None, Platform::MacOs).unwrap(),
            (PathBuf::from("/fixture-data"), true)
        );
        assert!(resolve(&[], cwd, exe, Some(Path::new("relative-home")), Platform::MacOs).is_err());
        // macOS CLI (không bundle) cũng về Application Support chung với GUI.
        assert_eq!(
            resolve(
                &[],
                cwd,
                Path::new("/Fixture.app/Contents/MacOS-extra/ctdozlite"),
                Some(Path::new("/Users/fixture")),
                Platform::MacOs
            )
            .unwrap(),
            (
                PathBuf::from("/Users/fixture/Library/Application Support/ctdozlite"),
                true
            )
        );
        // Thiếu HOME vẫn lỗi (không đoán được Application Support).
        assert!(resolve(
            &[],
            cwd,
            Path::new("/Fixture.app/Contents/MacOS-extra/ctdozlite"),
            None,
            Platform::MacOs
        )
        .is_err());
    }

    #[test]
    fn legacy_preparation_keeps_existing_permissions_and_probe_files() {
        let root = fixture();
        let sentinel = root.join(format!(
            ".storage-probe-{}-{}",
            std::process::id(),
            NEXT_FILE.load(Ordering::Relaxed)
        ));
        std::fs::write(&sentinel, b"retained fixture").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        prepare(&root, false).unwrap();
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"retained fixture");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn spaced_storage_option_rejects_flags_but_explicit_dash_paths_are_valid() {
        let cwd = Path::new("/work");
        let exe = Path::new("/bin/ctdozlite");
        for flag in ["-d", "-v", "-V", "--logout"] {
            assert!(
                resolve(&["--data-dir".into(), flag.into()], cwd, exe, None, Platform::MacOs).is_err(),
                "accepted {flag}"
            );
        }
        for args in [
            vec!["--data-dir=-d".into()],
            vec!["--data-dir".into(), "./-d".into()],
        ] {
            let (path, private) = resolve(&args, cwd, exe, None, Platform::MacOs).unwrap();
            assert_eq!(path, cwd.join("-d"));
            assert!(private);
        }
    }
    #[test]
    fn atomic_write_is_private_and_failed_replace_retains_destination() {
        let root = fixture();
        let path = root.join("session.json");
        write_private_atomic(&path, b"first fixture").unwrap();
        write_private_atomic(&path, b"second fixture").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second fixture");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let occupied = root.join("occupied");
        std::fs::create_dir(&occupied).unwrap();
        std::fs::write(occupied.join("keep"), b"retained").unwrap();
        assert!(write_private_atomic(&occupied, b"replacement").is_err());
        assert_eq!(std::fs::read(occupied.join("keep")).unwrap(), b"retained");
        assert!(write_private_atomic(&root.join("missing/session.json"), b"fixture").is_err());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn linux_uses_xdg_data_home_then_local_share_regardless_of_cwd() {
        let exe = Path::new("/usr/bin/ctdozlite");
        let home = Some(Path::new("/home/fixture"));
        let linux = |xdg| Platform::Linux { xdg_data_home: xdg };
        for cwd in [Path::new("/work"), Path::new("/home/fixture")] {
            assert_eq!(
                resolve(&[], cwd, exe, home, linux(None)).unwrap(),
                (PathBuf::from("/home/fixture/.local/share/ctdozlite"), true)
            );
        }
        assert_eq!(
            resolve(&[], Path::new("/work"), exe, home, linux(Some(Path::new("/data/xdg"))))
                .unwrap(),
            (PathBuf::from("/data/xdg/ctdozlite"), true)
        );
        // A relative XDG_DATA_HOME is invalid per spec and ignored.
        assert_eq!(
            resolve(&[], Path::new("/work"), exe, home, linux(Some(Path::new("rel"))))
                .unwrap(),
            (PathBuf::from("/home/fixture/.local/share/ctdozlite"), true)
        );
        assert_eq!(
            resolve(&["--profile=acc2".into()], Path::new("/work"), exe, home, linux(None))
                .unwrap(),
            (
                PathBuf::from("/home/fixture/.local/share/ctdozlite/profiles/acc2"),
                true
            )
        );
        assert_eq!(
            resolve(&["--data-dir=/d".into()], Path::new("/work"), exe, home, linux(None))
                .unwrap(),
            (PathBuf::from("/d"), true)
        );
        assert!(resolve(&[], Path::new("/work"), exe, None, linux(None)).is_err());
    }

    #[test]
    fn legacy_candidates_cover_cwd_and_home_with_profile_suffix() {
        let target = Path::new("/h/.local/share/ctdozlite");
        assert_eq!(
            legacy_candidates(target, Path::new("/work"), Some(Path::new("/h"))),
            vec![PathBuf::from("/work"), PathBuf::from("/h")]
        );
        assert_eq!(
            legacy_candidates(target, Path::new("/h"), Some(Path::new("/h"))),
            vec![PathBuf::from("/h")]
        );
        assert_eq!(
            legacy_candidates(&target.join("profiles/acc2"), Path::new("/w"), None),
            vec![PathBuf::from("/w/profiles/acc2")]
        );
    }

    #[test]
    fn migration_copies_only_data_entries_and_keeps_the_source() {
        let root = fixture();
        let old = root.join("home");
        std::fs::create_dir_all(old.join("accounts/42/media_cache")).unwrap();
        std::fs::write(old.join("session.json"), b"s").unwrap();
        std::fs::write(old.join("accounts/42_history.db"), b"db").unwrap();
        std::fs::write(old.join("accounts/42/media_cache/a.png"), b"png").unwrap();
        std::fs::write(old.join("crash.log"), b"log").unwrap();
        std::fs::write(old.join("Documents.txt"), b"user file").unwrap();
        let target = root.join("share/ctdozlite");

        let from = migrate_legacy_data(&target, std::slice::from_ref(&old)).unwrap();
        assert_eq!(from, Some(old.clone()));
        assert_eq!(std::fs::read(target.join("session.json")).unwrap(), b"s");
        assert_eq!(
            std::fs::read(target.join("accounts/42/media_cache/a.png")).unwrap(),
            b"png"
        );
        assert!(!target.join("crash.log").exists());
        assert!(!target.join("Documents.txt").exists());
        // Source untouched; no staging folder left behind.
        assert!(old.join("session.json").exists());
        assert_eq!(std::fs::read_dir(root.join("share")).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_skips_a_target_in_use_and_folders_without_app_data() {
        let root = fixture();
        let old = root.join("old");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("session.json"), b"old").unwrap();
        let target = root.join("new");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("session.json"), b"current").unwrap();
        assert_eq!(migrate_legacy_data(&target, &[old.clone()]).unwrap(), None);
        assert_eq!(std::fs::read(target.join("session.json")).unwrap(), b"current");

        let empty_target = root.join("fresh");
        let no_data = root.join("plain");
        std::fs::create_dir_all(&no_data).unwrap();
        std::fs::write(no_data.join("notes.txt"), b"x").unwrap();
        assert_eq!(migrate_legacy_data(&empty_target, &[no_data]).unwrap(), None);
        assert!(!empty_target.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_picks_the_most_recently_used_candidate() {
        let root = fixture();
        let older = root.join("older");
        let newer = root.join("newer");
        for (dir, body) in [(&older, b"older"), (&newer, b"newer")] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("session.json"), body).unwrap();
        }
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(older.join("session.json"))
            .unwrap()
            .set_modified(past)
            .unwrap();
        let target = root.join("target");
        let from = migrate_legacy_data(&target, &[older, newer.clone()]).unwrap();
        assert_eq!(from, Some(newer));
        assert_eq!(std::fs::read(target.join("session.json")).unwrap(), b"newer");
        std::fs::remove_dir_all(root).unwrap();
    }
}
