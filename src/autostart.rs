//! "Khởi động Zalo khi mở máy": register or remove the app as a login item.
//!
//! Linux uses the XDG autostart spec: a `ctdozlite.desktop` entry in
//! `$XDG_CONFIG_HOME/autostart` (default `~/.config/autostart`). Other platforms are
//! not implemented yet and report [`Outcome::Unsupported`].

use std::path::{Path, PathBuf};

/// File name of the autostart entry.
pub const ENTRY_NAME: &str = "ctdozlite.desktop";

/// What [`apply`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Entry written (enabled) or removed (disabled).
    Updated,
    /// Running from a cargo `target/` folder: the entry is left alone so a dev build
    /// never replaces the installed binary at login.
    SkippedDevBuild,
    /// No autostart support on this platform yet.
    Unsupported,
}

/// Bring the login item in line with `enabled`. Blocking file I/O: call it off the UI
/// thread (INV-3).
///
/// # Errors
/// Returns an error when the autostart folder or entry cannot be written or removed.
pub fn apply(enabled: bool) -> std::io::Result<Outcome> {
    #[cfg(target_os = "linux")]
    {
        let Some(dir) = autostart_dir() else {
            return Ok(Outcome::Unsupported);
        };
        let exe = std::env::current_exe()?;
        if is_dev_build(&exe) {
            return Ok(Outcome::SkippedDevBuild);
        }
        apply_in(&dir, &exe, enabled)?;
        Ok(Outcome::Updated)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = enabled;
        Ok(Outcome::Unsupported)
    }
}

/// `$XDG_CONFIG_HOME/autostart`, or `~/.config/autostart` (empty/relative override ignored).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn autostart_dir() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(config.join("autostart"))
}

/// A binary under a cargo `target/{debug,release}` folder.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn is_dev_build(exe: &Path) -> bool {
    exe.components()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|w| w[0].as_os_str() == "target" && matches!(w[1].as_os_str().to_str(), Some("debug" | "release")))
}

/// Write or remove `<dir>/ctdozlite.desktop` launching `exe`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn apply_in(dir: &Path, exe: &Path, enabled: bool) -> std::io::Result<()> {
    let entry = dir.join(ENTRY_NAME);
    if !enabled {
        return match std::fs::remove_file(&entry) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(&entry, desktop_entry(exe))
}

/// Desktop entry text; the exec path is quoted per the Desktop Entry spec.
fn desktop_entry(exe: &Path) -> String {
    let path = exe.to_string_lossy();
    let quoted = path
        .replace('\\', "\\\\\\\\")
        .replace('"', "\\\\\"")
        .replace('`', "\\\\`")
        .replace('$', "\\\\$");
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=ctdozlite\n\
         Comment=Zalo client (khởi động cùng hệ thống)\n\
         Exec=\"{quoted}\" --gui\n\
         Icon=ctdozlite\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_removes_the_entry() {
        let dir = std::env::temp_dir().join(format!("ctdoz-autostart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let exe = Path::new("/usr/bin/ctdozlite");
        apply_in(&dir, exe, true).unwrap();
        let text = std::fs::read_to_string(dir.join(ENTRY_NAME)).unwrap();
        assert!(text.contains("Exec=\"/usr/bin/ctdozlite\" --gui"));
        assert!(text.contains("X-GNOME-Autostart-enabled=true"));
        apply_in(&dir, exe, false).unwrap();
        assert!(!dir.join(ENTRY_NAME).exists());
        // Disabling again is a no-op, not an error.
        apply_in(&dir, exe, false).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dev_builds_are_detected() {
        assert!(is_dev_build(Path::new("/src/ctdozlite/target/debug/ctdozlite")));
        assert!(is_dev_build(Path::new("/src/ctdozlite/target/release/ctdozlite")));
        assert!(!is_dev_build(Path::new("/usr/bin/ctdozlite")));
        assert!(!is_dev_build(Path::new("/opt/target/ctdozlite")));
    }

    #[test]
    fn exec_path_is_quoted() {
        let text = desktop_entry(Path::new("/opt/my apps/ctdoz$lite"));
        assert!(text.contains("Exec=\"/opt/my apps/ctdoz\\\\$lite\" --gui"), "{text}");
    }
}
