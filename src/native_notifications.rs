//! Bounded macOS authorization and generic incoming-message notification bridge.
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::AsyncReadExt;

/// Authorization reported by the operating system helper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorizationStatus {
    /// The user has not chosen yet.
    NotDetermined,
    /// The user denied authorization.
    Denied,
    /// Alerts are authorized.
    Authorized,
    /// Quiet alerts are authorized.
    Provisional,
    /// Temporary authorization is available.
    Ephemeral,
    /// The operating system cannot report authorization.
    Unavailable,
}
impl AuthorizationStatus {
    /// Vietnamese label for the Settings panel.
    pub fn label(self) -> &'static str {
        match self {
            Self::NotDetermined => "Chưa yêu cầu quyền",
            Self::Denied => "Đã từ chối. Bạn có thể đổi trong Cài đặt hệ thống macOS.",
            Self::Authorized => "Đã cấp quyền",
            Self::Provisional => "Được phép thông báo im lặng",
            Self::Ephemeral => "Được cấp quyền tạm thời",
            Self::Unavailable => "Trạng thái quyền chưa khả dụng",
        }
    }
}
/// Bounded helper failures; raw helper output never reaches the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationError {
    /// Permission requests require an explicit user gesture.
    ExplicitUserActionRequired,
    /// This platform or installation has no executable helper.
    HelperUnavailable,
    /// The helper exceeded its deadline.
    Timeout,
    /// The helper exited unsuccessfully or returned invalid/oversized output.
    InvalidResponse,
    /// The child pipe could not be read.
    Io,
}
impl std::fmt::Display for NotificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ExplicitUserActionRequired => "Hãy bấm nút yêu cầu quyền để tiếp tục.",
            Self::HelperUnavailable => "Chưa tìm thấy trợ giúp quyền thông báo macOS.",
            Self::Timeout => "Đọc quyền thông báo quá thời gian. Hãy thử lại.",
            Self::InvalidResponse => "Trợ giúp quyền thông báo trả kết quả không hợp lệ.",
            Self::Io => "Lỗi đọc kết quả quyền thông báo. Hãy thử lại.",
        })
    }
}
impl std::error::Error for NotificationError {}

fn packaged_helper(executable: &Path) -> Option<PathBuf> {
    Some(
        executable
            .parent()?
            .parent()?
            .join("Helpers/DailyNotifier.app/Contents/MacOS/DailyNotifier"),
    )
}
fn helper_path() -> PathBuf {
    let packaged = std::env::current_exe()
        .ok()
        .and_then(|path| packaged_helper(&path));
    packaged.filter(|path| path.is_file()).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("native/macos/build/DailyNotifier.app/Contents/MacOS/DailyNotifier")
    })
}
async fn invoke(
    path: &Path,
    command: &str,
    deadline: Duration,
) -> Result<AuthorizationStatus, NotificationError> {
    let mut child = tokio::process::Command::new(path)
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| NotificationError::HelperUnavailable)?;
    let operation = async {
        let mut output = Vec::new();
        child
            .stdout
            .take()
            .ok_or(NotificationError::Io)?
            .take(4097)
            .read_to_end(&mut output)
            .await
            .map_err(|_| NotificationError::Io)?;
        if output.len() > 4096 {
            return Err(NotificationError::InvalidResponse);
        }
        if !child
            .wait()
            .await
            .map_err(|_| NotificationError::Io)?
            .success()
        {
            return Err(NotificationError::InvalidResponse);
        }
        parse_status(std::str::from_utf8(&output).map_err(|_| NotificationError::InvalidResponse)?)
    };
    tokio::time::timeout(deadline, operation)
        .await
        .map_err(|_| NotificationError::Timeout)?
}
fn parse_status(output: &str) -> Result<AuthorizationStatus, NotificationError> {
    match output.trim() {
        "notDetermined" => Ok(AuthorizationStatus::NotDetermined),
        "denied" => Ok(AuthorizationStatus::Denied),
        "authorized" => Ok(AuthorizationStatus::Authorized),
        "provisional" => Ok(AuthorizationStatus::Provisional),
        "ephemeral" => Ok(AuthorizationStatus::Ephemeral),
        "unavailable" => Ok(AuthorizationStatus::Unavailable),
        _ => Err(NotificationError::InvalidResponse),
    }
}
async fn platform_invoke(
    command: &str,
    deadline: Duration,
) -> Result<AuthorizationStatus, NotificationError> {
    if !cfg!(target_os = "macos") {
        return Err(NotificationError::HelperUnavailable);
    }
    invoke(&helper_path(), command, deadline).await
}
/// Read authorization without asking for permission or posting a notification.
///
/// # Errors
/// Returns a typed error for unsupported platforms, missing helpers, deadlines or invalid output.
pub async fn status() -> Result<AuthorizationStatus, NotificationError> {
    platform_invoke("--status", Duration::from_secs(10)).await
}
/// Request authorization only after the user explicitly clicks the permission button.
///
/// # Errors
/// Returns `ExplicitUserActionRequired` before spawning when the gesture is absent,
/// or a typed helper error otherwise. This function never posts notifications.
pub async fn request_if_user_action(
    explicit_user_action: bool,
) -> Result<AuthorizationStatus, NotificationError> {
    if !explicit_user_action {
        return Err(NotificationError::ExplicitUserActionRequired);
    }
    platform_invoke("--request", Duration::from_secs(120)).await
}

/// UI-owned sequence gate: at most one request is pending; stale completions are ignored.
#[derive(Default)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct AuthorizationState {
    next: u64,
    pub(crate) pending: Option<u64>,
    pub(crate) result: Option<Result<AuthorizationStatus, NotificationError>>,
}
impl AuthorizationState {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn begin(&mut self) -> Option<u64> {
        if self.pending.is_some() {
            return None;
        }
        self.next = self.next.checked_add(1)?;
        self.pending = Some(self.next);
        Some(self.next)
    }
    pub(crate) fn finish(
        &mut self,
        id: u64,
        result: Result<AuthorizationStatus, NotificationError>,
    ) {
        if self.pending == Some(id) {
            self.pending = None;
            self.result = Some(result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_gates_disabled_mute_foreground_outgoing_stale_account_and_duplicates() {
        for (enabled, muted, foreground, outgoing) in [
            (false, false, false, false),
            (true, true, false, false),
            (true, false, true, false),
            (true, false, false, true),
        ] {
            let mut gate = DeliveryGate::default();
            assert!(!gate.admit("a", "a", "t", "m", enabled, muted, foreground, outgoing));
            assert!(!gate.admit("a", "a", "t", "m", true, false, false, false));
        }
        let mut gate = DeliveryGate::default();
        assert!(!gate.admit("a", "b", "t", "m", true, false, false, false));
        assert!(!gate.admit("a", "a", "t", "", true, false, false, false));
        assert!(gate.admit("a", "a", "t", "m", true, false, false, false));
        assert!(!gate.admit("a", "a", "t", "m", true, false, false, false));
        assert!(gate.admit("b", "b", "t", "m", true, false, false, false));
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn delivery_helper_uses_notify_generic_payload_sound_and_terminal_errors() {
        for (output, expected) in [
            ("submitted", Ok(DeliveryResult::Submitted)),
            ("notAuthorized", Ok(DeliveryResult::NotAuthorized)),
            ("failed", Err(NotificationError::InvalidResponse)),
        ] {
            let fixture = Fixture::new(&format!(
                "cat > \"$0.payload\"; echo \"$1\" > \"$0.args\"; echo {output}"
            ));
            assert_eq!(
                deliver_at(
                    &fixture.0.join("helper"),
                    "account-private",
                    "thread-private",
                    "message-private",
                    true
                )
                .await,
                expected
            );
            assert_eq!(
                std::fs::read_to_string(fixture.0.join("helper.args")).unwrap(),
                "--notify\n"
            );
            let payload: serde_json::Value =
                serde_json::from_slice(&std::fs::read(fixture.0.join("helper.payload")).unwrap())
                    .unwrap();
            assert_eq!(payload["title"], "Zalo");
            assert_eq!(payload["body"], "You have a new message.");
            assert_eq!(payload["sound"], true);
            assert_eq!(payload["id"].as_str().unwrap().len(), 64);
            assert!(!payload.to_string().contains("private"));
        }
        assert_eq!(
            deliver_at(Path::new("/missing-fixture/helper"), "a", "t", "m", false).await,
            Err(NotificationError::HelperUnavailable)
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn delivery_helper_failed_exit_and_timeout_are_terminal() {
        let fixture = Fixture::new("cat >/dev/null; echo submitted; exit 3");
        assert_eq!(
            deliver_at(&fixture.0.join("helper"), "a", "t", "m", false).await,
            Err(NotificationError::InvalidResponse)
        );
        let fixture = Fixture::new("exec sleep 5");
        assert_eq!(
            deliver_at_with_deadline(
                &fixture.0.join("helper"),
                "a",
                "t",
                "m",
                false,
                Duration::from_millis(50)
            )
            .await,
            Err(NotificationError::Timeout)
        );
    }

    #[test]
    fn parser_accepts_all_statuses_and_rejects_unknown() {
        for (raw, expected) in [
            ("notDetermined", AuthorizationStatus::NotDetermined),
            ("denied", AuthorizationStatus::Denied),
            ("authorized", AuthorizationStatus::Authorized),
            ("provisional", AuthorizationStatus::Provisional),
            ("ephemeral", AuthorizationStatus::Ephemeral),
            ("unavailable", AuthorizationStatus::Unavailable),
        ] {
            assert_eq!(parse_status(&format!("{raw}\n")), Ok(expected));
        }
        for invalid in ["", "unknown", "authorized\ndenied", "submitted"] {
            assert_eq!(
                parse_status(invalid),
                Err(NotificationError::InvalidResponse)
            );
        }
    }

    #[tokio::test]
    async fn permission_requires_explicit_action_before_spawn() {
        assert_eq!(
            request_if_user_action(false).await,
            Err(NotificationError::ExplicitUserActionRequired)
        );
    }

    #[test]
    fn state_prevents_duplicates_and_ignores_stale_results_with_retry() {
        let mut state = AuthorizationState::default();
        let first = state.begin().unwrap();
        assert_eq!(state.begin(), None);
        state.finish(first + 1, Ok(AuthorizationStatus::Authorized));
        assert_eq!(state.pending, Some(first));
        state.finish(first, Err(NotificationError::Timeout));
        assert_eq!(state.result, Some(Err(NotificationError::Timeout)));
        let second = state.begin().unwrap();
        state.finish(first, Ok(AuthorizationStatus::Denied));
        assert_eq!(state.pending, Some(second));
        state.finish(second, Ok(AuthorizationStatus::Authorized));
        assert_eq!(state.result, Some(Ok(AuthorizationStatus::Authorized)));
    }

    #[test]
    fn every_helper_error_clears_loading_and_allows_retry() {
        let mut state = AuthorizationState::default();
        for error in [
            NotificationError::HelperUnavailable,
            NotificationError::Timeout,
            NotificationError::InvalidResponse,
            NotificationError::Io,
            NotificationError::ExplicitUserActionRequired,
        ] {
            let id = state.begin().unwrap();
            state.finish(id, Err(error));
            assert_eq!(state.pending, None);
            assert_eq!(state.result, Some(Err(error)));
        }
        assert!(state.begin().is_some());
    }

    #[test]
    fn packaged_path_relocates_with_parent_app() {
        assert_eq!(packaged_helper(Path::new("/tmp/Relocated.app/Contents/MacOS/zalo-rust")), Some(PathBuf::from("/tmp/Relocated.app/Contents/Helpers/DailyNotifier.app/Contents/MacOS/DailyNotifier")));
    }

    #[cfg(unix)]
    struct Fixture(PathBuf);
    #[cfg(unix)]
    impl Fixture {
        fn new(body: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "ctdozlite-notification-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&dir).unwrap();
            let path = dir.join("helper");
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            Self(dir)
        }
        async fn run(&self, command: &str) -> Result<AuthorizationStatus, NotificationError> {
            invoke(&self.0.join("helper"), command, Duration::from_secs(3)).await
        }
    }
    #[cfg(unix)]
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mock_executable_valid_denied_and_exact_args() {
        let fixture = Fixture::new("printf '%s\\n' \"$@\" >> \"$0.args\"; case \"$1\" in --status) echo denied;; --request) echo authorized;; *) exit 9;; esac");
        assert_eq!(
            fixture.run("--status").await,
            Ok(AuthorizationStatus::Denied)
        );
        assert_eq!(
            fixture.run("--request").await,
            Ok(AuthorizationStatus::Authorized)
        );
        assert_eq!(
            std::fs::read_to_string(fixture.0.join("helper.args")).unwrap(),
            "--status\n--request\n"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mock_executable_rejects_malformed_utf8_oversize_and_failed_exit() {
        for body in [
            "echo bogus".to_owned(),
            "printf '\\377'".to_owned(),
            format!("printf '{}'", "x".repeat(4097)),
            "echo authorized; exit 1".to_owned(),
        ] {
            assert_eq!(
                Fixture::new(&body).run("--status").await,
                Err(NotificationError::InvalidResponse),
                "{body}"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mock_executable_timeout_and_missing_helper() {
        assert_eq!(
            invoke(
                &Fixture::new("exec sleep 5").0.join("helper"),
                "--status",
                Duration::from_millis(100)
            )
            .await,
            Err(NotificationError::Timeout)
        );
        let fixture = Fixture::new("echo authorized");
        std::fs::remove_file(fixture.0.join("helper")).unwrap();
        assert_eq!(
            fixture.run("--status").await,
            Err(NotificationError::HelperUnavailable)
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[tokio::test]
    async fn unsupported_platform_does_not_launch_helper() {
        assert_eq!(status().await, Err(NotificationError::HelperUnavailable));
        assert_eq!(
            request_if_user_action(true).await,
            Err(NotificationError::HelperUnavailable)
        );
    }
}

/// Terminal outcome; a denied permission is not a helper failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryResult {
    /// The operating system accepted the generic alert.
    Submitted,
    /// The helper observed authorization that does not allow alerts.
    NotAuthorized,
}

/// Bounded, account-scoped stable-message deduplication owned by the UI.
#[derive(Default)]
pub(crate) struct DeliveryGate {
    account: String,
    seen: std::collections::HashSet<String>,
    order: std::collections::VecDeque<String>,
}
impl DeliveryGate {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn admit(
        &mut self,
        current: &str,
        account: &str,
        thread: &str,
        message: &str,
        enabled: bool,
        muted: bool,
        foreground: bool,
        outgoing: bool,
    ) -> bool {
        if current.is_empty() || account != current {
            return false;
        }
        if self.account != current {
            self.account = current.to_owned();
            self.seen.clear();
            self.order.clear();
        }
        if message.is_empty() || thread.is_empty() {
            return false;
        }
        let id = delivery_id(account, thread, message);
        // Mark even suppressed arrivals so changing focus/settings never replays history.
        if !self.seen.insert(id.clone()) {
            return false;
        }
        self.order.push_back(id);
        if self.order.len() > 2048 {
            if let Some(old) = self.order.pop_front() {
                self.seen.remove(&old);
            }
        }
        enabled && !muted && !foreground && !outgoing
    }
}
fn delivery_id(account: &str, thread: &str, message: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for value in [account, thread, message] {
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value.as_bytes());
    }
    format!("{:x}", hash.finalize())
}
async fn deliver_at(
    path: &Path,
    account: &str,
    thread: &str,
    message: &str,
    sound: bool,
) -> Result<DeliveryResult, NotificationError> {
    deliver_at_with_deadline(
        path,
        account,
        thread,
        message,
        sound,
        Duration::from_secs(22),
    )
    .await
}

async fn deliver_at_with_deadline(
    path: &Path,
    account: &str,
    thread: &str,
    message: &str,
    sound: bool,
    deadline: Duration,
) -> Result<DeliveryResult, NotificationError> {
    use tokio::io::AsyncWriteExt;
    let payload = serde_json::to_vec(&serde_json::json!({
        "title": "Zalo", "body": "You have a new message.",
        "id": delivery_id(account, thread, message), "sound": sound,
    }))
    .map_err(|_| NotificationError::InvalidResponse)?;
    let mut child = tokio::process::Command::new(path)
        .arg("--notify")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| NotificationError::HelperUnavailable)?;
    let operation = async {
        let mut stdin = child.stdin.take().ok_or(NotificationError::Io)?;
        stdin
            .write_all(&payload)
            .await
            .map_err(|_| NotificationError::Io)?;
        stdin.shutdown().await.map_err(|_| NotificationError::Io)?;
        drop(stdin);
        let mut output = Vec::new();
        child
            .stdout
            .take()
            .ok_or(NotificationError::Io)?
            .take(4097)
            .read_to_end(&mut output)
            .await
            .map_err(|_| NotificationError::Io)?;
        if output.len() > 4096
            || !child
                .wait()
                .await
                .map_err(|_| NotificationError::Io)?
                .success()
        {
            return Err(NotificationError::InvalidResponse);
        }
        match output.as_slice() {
            b"submitted\n" | b"submitted" => Ok(DeliveryResult::Submitted),
            b"notAuthorized\n" | b"notAuthorized" => Ok(DeliveryResult::NotAuthorized),
            _ => Err(NotificationError::InvalidResponse),
        }
    };
    tokio::time::timeout(deadline, operation)
        .await
        .map_err(|_| NotificationError::Timeout)?
}
/// Deliver generic content without requesting permission. Caller must admit the current runtime scope.
///
/// # Errors
/// Returns typed helper errors, never raw helper output or private message data.
pub async fn deliver(
    account: &str,
    thread: &str,
    message: &str,
    sound: bool,
) -> Result<DeliveryResult, NotificationError> {
    if !cfg!(target_os = "macos") || account.is_empty() || thread.is_empty() || message.is_empty() {
        return Err(NotificationError::HelperUnavailable);
    }
    deliver_at(&helper_path(), account, thread, message, sound).await
}
