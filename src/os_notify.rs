//! Fire-and-forget OS desktop banners + notification sound for chat events.
//!
//! The updater previously owned the only OS-notify path
//! (`updater::notify_os_new_update`: `osascript` / `notify-send` / PowerShell
//! `NotifyIcon`). Chat notifications reuse that exact delivery here so there is
//! a single banner backend. The `DailyNotifier` helper is intentionally NOT
//! used: its payload is a hardcoded generic string and cannot carry chat text.

#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::path::Path;

#[cfg(target_os = "macos")]
extern "C" {
    fn ctdozlite_macos_notify_setup(request_if_undetermined: std::ffi::c_int);
    fn ctdozlite_macos_notify(
        title_utf8: *const std::ffi::c_char,
        body_utf8: *const std::ffi::c_char,
        sound_utf8: *const std::ffi::c_char,
    ) -> std::ffi::c_int;
}

/// Register the native macOS `UNUserNotificationCenter` delegate when running inside `.app`.
pub fn init_native_notifications(request_permission: bool) {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: Checks for a valid `.app` bundle before touching UserNotifications.
        unsafe {
            ctdozlite_macos_notify_setup(if request_permission { 1 } else { 0 });
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = request_permission;
}

/// Collapse whitespace/newlines and truncate to `max_chars` characters
/// (char boundary safe). Appends "…" when truncated.
pub(crate) fn compact_text(raw: &str, max_chars: usize) -> String {
    let collapsed = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let mut out: String = collapsed.chars().take(max_chars).collect();
    out.push('…');
    out
}

/// Escape for embedding inside a double-quoted AppleScript string literal.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn escape_osascript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Legacy single-quote escaper, kept for unit-test compatibility.
/// The Windows notifier no longer interpolates text into PowerShell code
/// (see `dispatch`); it passes title/body via environment variables instead.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn escape_powershell_single(s: &str) -> String {
    s.replace('\'', "''")
}

#[allow(unused_variables)]
fn dispatch(title: &str, body: &str, linux_icon: &str, mac_sound: Option<&str>) {
    let title = compact_text(title, 80);
    let body = compact_text(body, 240);
    if title.is_empty() && body.is_empty() {
        return;
    }

    #[cfg(target_os = "macos")]
    {
        let strip_nul = |s: &str| s.replace('\0', " ");
        let c_title = std::ffi::CString::new(strip_nul(&title)).ok();
        let c_body = std::ffi::CString::new(strip_nul(&body)).ok();
        let c_sound = mac_sound.and_then(|s| std::ffi::CString::new(strip_nul(s)).ok());
        if let (Some(t), Some(b)) = (c_title.as_ref(), c_body.as_ref()) {
            let sound_ptr = c_sound.as_ref().map_or(std::ptr::null(), |s| s.as_ptr());
            // SAFETY: Pointers are NUL-terminated `CString`s valid for the FFI call.
            unsafe {
                let _ = ctdozlite_macos_notify(t.as_ptr(), b.as_ptr(), sound_ptr);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("notify-send")
            .args(["-a", "ctdozlite", "-i", linux_icon, "--", &title, &body])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // Pass notification text as data (environment), never as PowerShell code:
        // even doubled `''` escaping is fragile against smart-quote/encoding
        // edge cases (`’; Start-Process calc; ’x`). The script below contains
        // no interpolated text; it reads the literal values from the environment.
        let ps_script =
            "Add-Type -AssemblyName System.Windows.Forms; \
             $n = New-Object System.Windows.Forms.NotifyIcon; \
             $n.Icon = [System.Drawing.SystemIcons]::Information; \
             $n.BalloonTipTitle = $env:CTDOZLITE_NOTIFY_TITLE; \
             $n.BalloonTipText = $env:CTDOZLITE_NOTIFY_BODY; \
             $n.Visible = $true; \
             $n.ShowBalloonTip(6000); \
             Start-Sleep -Seconds 6; \
             $n.Dispose();";
        let _ = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                ps_script,
            ])
            .env("CTDOZLITE_NOTIFY_TITLE", &title)
            .env("CTDOZLITE_NOTIFY_BODY", &body)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (title, body, linux_icon, mac_sound);
    }
}

/// Post a generic chat OS banner (no bundled sound; sound goes through
/// [`play_notification_sound`] so the settings toggle stays authoritative).
pub fn notify(title: &str, body: &str) {
    dispatch(title, body, "dialog-information", None);
}

/// Post the updater banner, preserving its historical `Glass` sound on macOS.
pub fn notify_update(title: &str, body: &str) {
    dispatch(title, body, "software-update-available", Some("Glass"));
}

/// Post one banner per new-message thread (at most 3), plus a summary banner
/// when more threads have unread mail. No-op on an empty slice.
pub fn notify_chat_batch(banners: &[(String, String)]) {
    if banners.is_empty() {
        return;
    }
    for (title, body) in banners.iter().take(3) {
        notify(title, body);
    }
    if banners.len() > 3 {
        notify(
            "ctdozlite",
            &format!("… và {} hội thoại khác có tin nhắn mới", banners.len() - 3),
        );
    }
}

/// Banner body for a reaction on one of my own messages.
/// `reactor` is the display name (or uid fallback), `emoji` the reaction glyph
/// (empty for legacy payloads → generic "cảm xúc"), `snippet` the reacted-to
/// message text (may be empty).
pub(crate) fn reaction_banner_body(reactor: &str, emoji: &str, snippet: &str) -> String {
    let reactor = reactor.trim();
    let what = {
        let e = emoji.trim();
        if e.is_empty() {
            "cảm xúc".to_string()
        } else {
            e.to_string()
        }
    };
    let mut body = format!("{reactor} đã thả {what} vào tin nhắn của bạn");
    let snippet = compact_text(snippet, 120);
    if !snippet.is_empty() {
        body.push_str(&format!(": {snippet}"));
    }
    body
}

/// Play the notification sound once, fire-and-forget. Only call when both
/// `enable_notifications` and `play_notification_sound` are on.
pub fn play_notification_sound() {
    #[cfg(target_os = "macos")]
    {
        for candidate in [
            "/System/Library/Sounds/Glass.aiff",
            "/System/Library/Sounds/Ping.aiff",
        ] {
            if Path::new(candidate).exists() {
                let _ = std::process::Command::new("afplay")
                    .arg(candidate)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                break;
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let mut played = false;
        let ogg = "/usr/share/sounds/freedesktop/stereo/message.oga";
        if Path::new(ogg).exists() {
            played = std::process::Command::new("paplay")
                .arg(ogg)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .is_ok();
        }
        if !played {
            let _ = std::process::Command::new("canberra-gtk-play")
                .args(["-i", "message"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                "[System.Media.SystemSounds]::Asterisk.Play();",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_collapses_whitespace_and_truncates_on_char_boundary() {
        assert_eq!(compact_text("  chào\n  bạn\t\tnhé  ", 100), "chào bạn nhé");
        assert_eq!(compact_text("", 10), "");
        // Emoji are multi-byte: truncation must not split them.
        let s = "❤️👍😂abc";
        assert_eq!(compact_text(s, 3), "❤️👍…");
        assert_eq!(compact_text(s, 10), s);
    }

    #[test]
    fn osascript_escape_handles_quotes_and_backslashes() {
        assert_eq!(escape_osascript(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    #[test]
    fn powershell_escape_doubles_single_quotes() {
        assert_eq!(escape_powershell_single("l'amie"), "l''amie");
    }

    #[test]
    fn reaction_body_prefers_emoji_and_appends_snippet() {
        let body = reaction_banner_body("An", "❤️", "gặp nhau nhé");
        assert_eq!(body, "An đã thả ❤️ vào tin nhắn của bạn: gặp nhau nhé");
        let generic = reaction_banner_body("uid-123", "", "");
        assert_eq!(generic, "uid-123 đã thả cảm xúc vào tin nhắn của bạn");
    }
}
