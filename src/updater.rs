//! Bounded release downloads and transactional installation, independent of GUI and processes.

use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Resource limits for one release asset.
#[derive(Clone, Copy)]
pub struct DownloadLimits {
    /// Smallest accepted artifact.
    pub min_bytes: u64,
    /// Maximum buffered artifact size.
    pub max_bytes: u64,
    /// Deadline covering headers and the complete body.
    pub timeout: Duration,
}

impl Default for DownloadLimits {
    fn default() -> Self {
        Self {
            min_bytes: 512 * 1024,
            max_bytes: 512 * 1024 * 1024,
            timeout: Duration::from_secs(240),
        }
    }
}

/// Release metadata supplied by the release discovery layer.
pub struct DownloadRequest<'a> {
    /// HTTP asset location, without session cookies.
    pub url: &'a str,
    /// A single filename, never a path.
    pub asset_name: &'a str,
    /// Optional published SHA-256 hex digest.
    pub expected_sha256: Option<&'a str>,
    /// Optional size published by the release API.
    pub expected_size: Option<u64>,
    /// Resource bounds for this transfer.
    pub limits: DownloadLimits,
}

/// Explicit destination selected by the platform adapter, not remote metadata.
pub enum InstallTarget {
    /// Replace a portable executable and preserve its permissions.
    Portable(PathBuf),
    /// Save an installer for the platform launcher.
    Installer(PathBuf),
}

/// Reject absolute paths, traversal and cross-platform path separators.
///
/// # Errors
/// Returns an error unless the input is a single ordinary filename.
pub fn validate_asset_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':', '\0'])
        || name.chars().any(char::is_control)
    {
        return Err("Tên gói cập nhật không hợp lệ".into());
    }
    Ok(())
}

/// Verify asset filename safety, optional SHA-256 digest, and platform binary/archive magic bytes.
///
/// # Errors
/// Returns an error if the asset name is unsafe, the SHA-256 digest is malformed or mismatches,
/// or the binary/archive magic header does not match its file extension.
pub fn verify_asset_integrity(
    asset_name: &str,
    bytes: &[u8],
    expected_sha256: Option<&str>,
) -> Result<(), String> {
    validate_asset_name(asset_name)?;
    if let Some(expected) = expected_sha256 {
        let clean_expected = expected.trim().strip_prefix("sha256:").unwrap_or(expected.trim());
        if clean_expected.len() != 64 || !clean_expected.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Mã SHA-256 của gói cập nhật không hợp lệ".into());
        }
        let actual = format!("{:x}", Sha256::digest(bytes));
        if !actual.eq_ignore_ascii_case(clean_expected) {
            return Err("Mã kiểm tra toàn vẹn SHA-256 không khớp".into());
        }
    }
    let name = asset_name.to_ascii_lowercase();
    if name.ends_with(".exe") && !bytes.starts_with(b"MZ") {
        return Err("File cập nhật không có chữ ký PE hợp lệ".into());
    }
    if name.ends_with(".appimage") && !bytes.starts_with(b"\x7fELF") {
        return Err("File cập nhật không có chữ ký ELF hợp lệ".into());
    }
    if (name.ends_with(".tar.gz") || name.ends_with(".tgz")) && !bytes.starts_with(&[0x1f, 0x8b]) {
        return Err("File cập nhật .tar.gz không có chữ ký GZIP hợp lệ".into());
    }
    if name.ends_with(".deb") && !bytes.starts_with(b"!<arch>\n") {
        return Err("File cập nhật .deb không có chữ ký Debian archive hợp lệ".into());
    }
    if name.ends_with(".rpm") && !bytes.starts_with(&[0xed, 0xab, 0xee, 0xdb]) {
        return Err("File cập nhật .rpm không có chữ ký RPM hợp lệ".into());
    }
    if name.ends_with(".zip") && !bytes.starts_with(b"PK\x03\x04") {
        return Err("File cập nhật .zip không có chữ ký PKZIP hợp lệ".into());
    }
    Ok(())
}

/// Download and validate an asset before any installation destination is touched.
/// The caller supplies a dedicated, cookie-free client and a progress callback.
///
/// # Errors
/// Rejects malformed metadata, HTTP failures, incomplete bodies, resource limits,
/// unsupported executable signatures, and checksum mismatches.
pub async fn download_release(
    client: &reqwest::Client,
    request: &DownloadRequest<'_>,
    mut progress: impl FnMut(u64, u64),
) -> Result<Vec<u8>, String> {
    validate_asset_name(request.asset_name)?;
    let limits = request.limits;
    if limits.min_bytes > limits.max_bytes || limits.max_bytes > usize::MAX as u64 {
        return Err("Giới hạn dung lượng cập nhật không hợp lệ".into());
    }
    if let Some(hash) = request.expected_sha256 {
        let clean_hash = hash.trim().strip_prefix("sha256:").unwrap_or(hash.trim());
        if clean_hash.len() != 64 || !clean_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Mã SHA-256 của gói cập nhật không hợp lệ".into());
        }
    }
    if request
        .expected_size
        .is_some_and(|size| size > limits.max_bytes)
    {
        return Err("Gói cập nhật vượt giới hạn dung lượng".into());
    }
    let transfer = async {
        let mut response = client
            .get(request.url)
            .timeout(limits.timeout)
            .send()
            .await
            .map_err(|e| format!("Tải gói cập nhật thất bại: {e}"))?
            .error_for_status()
            .map_err(|e| format!("Máy chủ cập nhật trả về lỗi: {e}"))?;
        let length = response.content_length();
        if length.is_some_and(|size| size > limits.max_bytes) {
            return Err("Gói cập nhật vượt giới hạn dung lượng".into());
        }
        if let (Some(expected), Some(actual)) = (request.expected_size, length) {
            if expected != actual {
                return Err("Dung lượng gói cập nhật không khớp metadata".into());
            }
        }
        let total = request.expected_size.or(length).unwrap_or(0);
        let mut bytes = Vec::new();
        let mut hasher = Sha256::new();
        let mut last_progress = std::time::Instant::now();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("Ngắt kết nối khi đang tải cập nhật: {e}"))?
        {
            if (chunk.len() as u64) > limits.max_bytes.saturating_sub(bytes.len() as u64) {
                return Err("Gói cập nhật vượt giới hạn dung lượng".into());
            }
            hasher.update(&chunk);
            bytes.extend_from_slice(&chunk);
            if last_progress.elapsed() >= Duration::from_millis(80) {
                progress(bytes.len() as u64, total);
                last_progress = std::time::Instant::now();
            }
        }
        let actual = bytes.len() as u64;
        if actual < limits.min_bytes {
            return Err("Gói cập nhật tải về quá nhỏ".into());
        }
        if length.is_some_and(|size| size != actual)
            || request.expected_size.is_some_and(|size| size != actual)
        {
            return Err("Gói cập nhật tải về chưa hoàn tất".into());
        }
        if let Some(expected) = request.expected_sha256 {
            let clean_expected = expected.trim().strip_prefix("sha256:").unwrap_or(expected.trim());
            let actual_hash = format!("{:x}", hasher.finalize());
            if !actual_hash.eq_ignore_ascii_case(clean_expected) {
                return Err("Mã kiểm tra toàn vẹn SHA-256 không khớp".into());
            }
        }
        verify_asset_integrity(request.asset_name, &bytes, None)?;
        progress(actual, total.max(actual));
        Ok(bytes)
    };
    tokio::time::timeout(limits.timeout, transfer)
        .await
        .map_err(|_| "Tải gói cập nhật quá thời gian cho phép".to_string())?
}

static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);

fn staging_file(destination: &Path) -> Result<(PathBuf, File), String> {
    let parent = destination
        .parent()
        .ok_or("Đường dẫn cập nhật thiếu thư mục")?;
    for _ in 0..100 {
        let id = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".ctdo-update-{}-{id}", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Tạo file cập nhật tạm thất bại: {error}")),
        }
    }
    Err("Không tạo được file cập nhật tạm riêng biệt".into())
}

fn remove_staging(path: &Path) {
    // Cleanup must not hide the primary download/install failure.
    let _ = fs::remove_file(path);
}

/// Portable installation staged without replacing or launching the running executable.
/// Dropping an uncommitted transaction preserves the original executable.
pub struct DeferredPortableInstall {
    prepared: PreparedInstall,
}

impl DeferredPortableInstall {
    /// Replace the executable and launch only after the caller has joined its worker.
    ///
    /// # Errors
    /// Returns replacement, launch or explicit rollback errors.
    pub fn launch_and_finish(
        self,
        launch: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Result<(), String> {
        self.prepared.finish(launch)
    }

    /// Discard the staged update and preserve the original executable.
    ///
    /// # Errors
    /// Returns a rollback error if replacement happened before cancellation.
    pub fn abort(mut self) -> Result<(), String> {
        self.prepared.rollback()
    }
}

/// Stage validated portable bytes without replacing or launching any executable.
///
/// # Errors
/// Returns filesystem errors while preparing private sibling files.
pub fn stage_portable_release(
    bytes: &[u8],
    destination: &Path,
) -> Result<DeferredPortableInstall, String> {
    Ok(DeferredPortableInstall {
        prepared: prepare_install(bytes, destination, true)?,
    })
}

struct PreparedInstall {
    destination: PathBuf,
    stage: PathBuf,
    backup: Option<PathBuf>,
    installed: bool,
    committed: bool,
}

impl PreparedInstall {
    fn finish(mut self, launch: impl FnOnce(&Path) -> Result<(), String>) -> Result<(), String> {
        fs::rename(&self.stage, &self.destination)
            .map_err(|error| format!("Thay thế gói cập nhật thất bại: {error}"))?;
        self.installed = true;
        if let Err(error) = launch(&self.destination) {
            self.rollback().map_err(|rollback| {
                format!("Khởi chạy thất bại: {error}; hoàn tác thất bại: {rollback}")
            })?;
            return Err(format!("Khởi chạy thất bại, đã hoàn tác cập nhật: {error}"));
        }
        self.committed = true;
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), String> {
        if self.installed && !self.committed {
            match &self.backup {
                Some(backup) => fs::rename(backup, &self.destination),
                None => fs::remove_file(&self.destination),
            }
            .map_err(|error| format!("Khôi phục gói cập nhật cũ thất bại: {error}"))?;
            self.installed = false;
        }
        Ok(())
    }
}

impl Drop for PreparedInstall {
    fn drop(&mut self) {
        // A retained backup is the recovery source if rollback itself fails.
        if self.rollback().is_err() {
            return;
        }
        remove_staging(&self.stage);
        if let Some(backup) = &self.backup {
            remove_staging(backup);
        }
    }
}

fn prepare_install(
    bytes: &[u8],
    destination: &Path,
    portable: bool,
) -> Result<PreparedInstall, String> {
    let metadata = match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_file() => Some(metadata),
        Ok(_) => return Err("Đích cập nhật không phải file thông thường".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !portable => None,
        Err(error) => return Err(format!("Đọc đích cập nhật thất bại: {error}")),
    };
    let (stage, mut file) = staging_file(destination)?;
    let prepared = (|| {
        file.write_all(bytes)?;
        if portable {
            if let Some(metadata) = &metadata {
                file.set_permissions(metadata.permissions())?;
            }
        }
        file.sync_all()
    })();
    drop(file);
    if let Err(error) = prepared {
        remove_staging(&stage);
        return Err(format!("Ghi file cập nhật thất bại: {error}"));
    }
    let backup = if metadata.is_some() {
        let (path, file) = match staging_file(destination) {
            Ok(value) => value,
            Err(error) => {
                remove_staging(&stage);
                return Err(error);
            }
        };
        drop(file);
        // A private sibling copy keeps the original destination present until atomic replacement.
        if let Err(error) = fs::copy(destination, &path) {
            remove_staging(&stage);
            remove_staging(&path);
            return Err(format!("Sao lưu bản cập nhật cũ thất bại: {error}"));
        }
        Some(path)
    } else {
        None
    };
    Ok(PreparedInstall {
        destination: destination.to_path_buf(),
        stage,
        backup,
        installed: false,
        committed: false,
    })
}

/// Atomically install already-validated bytes and invoke the injected launcher.
/// Existing bytes are restored if replacement or launch fails. A rollback failure
/// is returned explicitly and the backup is retained for recovery.
///
/// # Errors
/// Returns filesystem or launcher errors; never falls back to another destination.
pub fn install_release(
    bytes: &[u8],
    target: &InstallTarget,
    launch: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let (destination, portable) = match target {
        InstallTarget::Portable(path) => (path, true),
        InstallTarget::Installer(path) => (path, false),
    };
    prepare_install(bytes, destination, portable)?.finish(launch)
}

/// Metadata parsed from GitHub Releases REST API (`/repos/{owner}/{repo}/releases/latest`)
/// or the static `latest.json` release manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubReleaseMetadata {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub release_title: String,
    pub release_notes: String,
    pub html_url: String,
    pub download_url: Option<String>,
    pub asset_name: Option<String>,
    pub asset_size: u64,
    pub expected_sha256: Option<String>,
    pub published_at: String,
    pub sha256sums_url: Option<String>,
}

/// Compare semantic versions (`"v0.0.4"` > `"0.0.3"`).
pub fn is_newer_semver(remote: &str, current: &str) -> bool {
    let parse = |s: &str| -> (u64, u64, u64, bool) {
        let trimmed = s.trim().trim_start_matches(['v', 'V']);
        let (core, rest) = trimmed
            .split_once(['-', '+'])
            .map_or((trimmed, ""), |(c, r)| (c, r));
        let is_stable = !trimmed.contains('-') || rest.is_empty();
        let mut parts = core.split('.').filter_map(|p| p.trim().parse::<u64>().ok());
        (
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
            is_stable,
        )
    };
    parse(remote) > parse(current)
}

/// Score a release asset filename for a target `(os, arch)` pair.
/// Returns `<= 0` when the asset is incompatible with the platform.
pub fn select_platform_asset_score(name: &str, target_os: &str, target_arch: &str) -> i32 {
    let n_low = name.to_ascii_lowercase();
    if n_low.is_empty()
        || n_low.ends_with(".txt")
        || n_low.ends_with(".json")
        || n_low.ends_with(".sig")
        || n_low.ends_with(".asc")
        || n_low.ends_with(".sha256")
    {
        return -1;
    }
    let has_arm64 = n_low.contains("arm64") || n_low.contains("aarch64");
    let has_x64 = n_low.contains("x86_64") || n_low.contains("amd64") || n_low.contains("intel");
    let arch_bonus = match target_arch {
        "aarch64" | "arm64" => {
            if has_arm64 {
                10
            } else if has_x64 {
                -40
            } else {
                0
            }
        }
        "x86_64" | "amd64" => {
            if has_x64 {
                10
            } else if has_arm64 {
                -60
            } else {
                0
            }
        }
        _ => 0,
    };

    match target_os {
        "windows" => {
            if n_low.contains("macos") || n_low.contains("darwin") || n_low.contains("linux") {
                return 0;
            }
            let base = if n_low.ends_with(".exe") && n_low.contains("portable") {
                100
            } else if n_low.ends_with(".exe")
                && (n_low.contains("setup") || n_low.contains("installer"))
            {
                80
            } else if n_low.ends_with(".exe") {
                70
            } else if n_low.ends_with(".msi") {
                65
            } else if n_low.ends_with(".zip") && n_low.contains("windows") {
                60
            } else {
                return 0;
            };
            (base + arch_bonus).max(0)
        }
        "macos" => {
            if n_low.contains("windows") || n_low.contains("linux") {
                return 0;
            }
            let is_dmg = n_low.ends_with(".dmg");
            let is_mac_tar = (n_low.ends_with(".tar.gz") || n_low.ends_with(".tgz"))
                && (n_low.contains("macos") || n_low.contains("darwin") || n_low.contains("apple"));
            if !is_dmg && !is_mac_tar {
                return 0;
            }
            let fmt_offset = if is_dmg { 0 } else { -15 };
            let base = if (target_arch == "aarch64" || target_arch == "arm64") && has_arm64 {
                110
            } else if (target_arch == "x86_64" || target_arch == "amd64") && has_x64 {
                110
            } else if !has_arm64 && !has_x64 {
                100
            } else {
                50
            };
            (base + fmt_offset).max(1)
        }
        _ => {
            if n_low.contains("macos")
                || n_low.contains("darwin")
                || n_low.contains("apple")
                || n_low.contains("windows")
                || n_low.contains("win64")
                || n_low.contains("win32")
            {
                return 0;
            }
            let base = if n_low.ends_with(".appimage") {
                90
            } else if n_low.ends_with(".deb") && !n_low.contains("legacy") {
                80
            } else if (n_low.ends_with(".tar.gz") || n_low.ends_with(".tgz"))
                && !n_low.contains("legacy")
            {
                70
            } else if n_low.ends_with(".rpm") {
                60
            } else if n_low.ends_with(".deb") || n_low.ends_with(".tar.gz") {
                45
            } else {
                return 0;
            };
            (base + arch_bonus).max(0)
        }
    }
}

/// Extract a 64-character lowercase SHA-256 hex digest from `SHA256SUMS.txt` content for `asset_name`.
pub fn parse_sha256sums_for_asset(sha256sums_text: &str, asset_name: &str) -> Option<String> {
    for raw_line in sha256sums_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Support BSD-style `SHA256 (filename) = <hash>`
        if let Some(rest) = line.strip_prefix("SHA256 (").or_else(|| line.strip_prefix("sha256 ("))
        {
            if let Some((fname, hash_part)) = rest.split_once(") =") {
                let clean_fname = fname
                    .trim()
                    .trim_start_matches('*')
                    .trim_start_matches("./");
                let base_fname = clean_fname.rsplit(['/', '\\']).next().unwrap_or(clean_fname);
                let hash = hash_part
                    .trim()
                    .strip_prefix("sha256:")
                    .unwrap_or(hash_part.trim());
                if (clean_fname == asset_name || base_fname == asset_name)
                    && hash.len() == 64
                    && hash.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return Some(hash.to_ascii_lowercase());
                }
            }
        }
        let mut parts = line.split_whitespace();
        let Some(raw_hash) = parts.next() else { continue };
        let Some(fname) = parts.next() else { continue };
        let hash = raw_hash.strip_prefix("sha256:").unwrap_or(raw_hash);
        let clean_fname = fname
            .trim_matches(['`', '"', '\''])
            .trim_start_matches('*')
            .trim_start_matches("./");
        let base_fname = clean_fname.rsplit(['/', '\\']).next().unwrap_or(clean_fname);
        if (clean_fname == asset_name || base_fname == asset_name)
            && hash.len() == 64
            && hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

/// Parse either a GitHub Releases API JSON object (`/releases/latest`) or a `latest.json`
/// manifest object into [`GitHubReleaseMetadata`].
pub fn parse_github_release_value(
    rel: &serde_json::Value,
    current_ver: &str,
    repo_slug: &str,
    target_os: &str,
    target_arch: &str,
) -> GitHubReleaseMetadata {
    let raw_tag = rel
        .get("tag_name")
        .or_else(|| rel.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or(current_ver);
    let latest_ver = raw_tag.trim().trim_start_matches(['v', 'V']).to_string();
    let has_update = is_newer_semver(&latest_ver, current_ver);

    let release_title = rel
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("ctdozlite v{latest_ver}"));

    let release_notes = rel
        .get("body")
        .or_else(|| rel.get("notes"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let html_url = rel
        .get("html_url")
        .or_else(|| rel.get("release_url"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://github.com/{repo_slug}/releases/tag/v{latest_ver}"));

    let published_at = rel
        .get("published_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .split('T')
        .next()
        .unwrap_or("")
        .to_string();

    let mut best_url: Option<String> = None;
    let mut best_name: Option<String> = None;
    let mut best_size: u64 = 0;
    let mut best_sha256: Option<String> = None;
    let mut best_score: i32 = 0;
    let mut sha256sums_url: Option<String> = None;

    if let Some(assets) = rel.get("assets").and_then(|v| v.as_array()) {
        for a in assets {
            let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let dl = a
                .get("browser_download_url")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let sz = a.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            if name.eq_ignore_ascii_case("SHA256SUMS.txt") && !dl.is_empty() {
                sha256sums_url = Some(dl.to_string());
                continue;
            }
            if name.is_empty() || dl.is_empty() {
                continue;
            }
            let score = select_platform_asset_score(name, target_os, target_arch);
            if score > best_score {
                best_score = score;
                best_url = Some(dl.to_string());
                best_name = Some(name.to_string());
                best_size = sz;
                // Extract SHA-256 from `latest.json` (`sha256`) or GitHub API asset (`digest: "sha256:..."`)
                best_sha256 = a
                    .get("sha256")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        a.get("digest")
                            .and_then(|v| v.as_str())
                            .and_then(|d| d.strip_prefix("sha256:"))
                    })
                    .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
                    .map(|h| h.to_ascii_lowercase());
            }
        }
    }

    // Legacy single-asset `latest.json` fallback when `assets` array is absent
    if best_url.is_none() && rel.get("assets").is_none() {
        let fallback_name = match target_os {
            "windows" => format!("ctdozlite-{latest_ver}-portable-windows-x86_64.exe"),
            "macos" => match target_arch {
                "aarch64" => format!("ctdozlite-{latest_ver}-macos-arm64.dmg"),
                "x86_64" => format!("ctdozlite-{latest_ver}-macos-x86_64.dmg"),
                _ => format!("ctdozlite-{latest_ver}-macos.dmg"),
            },
            _ => format!("ctdozlite-{latest_ver}-x86_64.AppImage"),
        };
        best_url = Some(format!(
            "https://github.com/{repo_slug}/releases/download/v{latest_ver}/{fallback_name}"
        ));
        best_name = Some(fallback_name);
        best_sha256 = rel
            .get("sha256")
            .and_then(|v| v.as_str())
            .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(|h| h.to_ascii_lowercase());
    }

    if best_sha256.is_none() {
        if let Some(ref name) = best_name {
            best_sha256 = parse_sha256sums_for_asset(&release_notes, name);
        }
    }

    GitHubReleaseMetadata {
        current_version: current_ver.to_string(),
        latest_version: latest_ver,
        has_update,
        release_title,
        release_notes,
        html_url,
        download_url: best_url,
        asset_name: best_name,
        asset_size: best_size,
        expected_sha256: best_sha256,
        published_at,
        sha256sums_url,
    }
}

/// Dispatch a non-blocking native OS desktop notification announcing a new GitHub release.
pub fn notify_os_new_update(latest_version: &str, current_version: &str, release_title: &str) {
    let title = format!("ctdozlite — Có bản cập nhật mới v{latest_version}");
    let body = if release_title.trim().is_empty() {
        format!("Phiên bản mới v{latest_version} đã sẵn sàng trên GitHub (hiện tại: v{current_version}). Mở ứng dụng để cập nhật tự động.")
    } else {
        format!("{release_title} đã sẵn sàng trên GitHub (hiện tại: v{current_version}). Mở ứng dụng để cập nhật ngay!")
    };

    #[cfg(target_os = "macos")]
    {
        let esc_title = title.replace('\\', "\\\\").replace('"', "\\\"");
        let esc_body = body.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "display notification \"{esc_body}\" with title \"{esc_title}\" sound name \"Glass\""
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("notify-send")
            .args([
                "-a",
                "ctdozlite",
                "-i",
                "software-update-available",
                &title,
                &body,
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let esc_title = title.replace('\'', "''");
        let esc_body = body.replace('\'', "''");
        let ps_script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $n = New-Object System.Windows.Forms.NotifyIcon; \
             $n.Icon = [System.Drawing.SystemIcons]::Information; \
             $n.BalloonTipTitle = '{esc_title}'; \
             $n.BalloonTipText = '{esc_body}'; \
             $n.Visible = $true; \
             $n.ShowBalloonTip(6000); \
             Start-Sleep -Seconds 6; \
             $n.Dispose();"
        );
        let _ = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &ps_script,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

#[cfg(test)]
mod github_api_tests {
    use super::*;

    #[test]
    fn test_is_newer_semver_comparisons() {
        assert!(is_newer_semver("0.0.4", "0.0.3"));
        assert!(is_newer_semver("v0.0.4", "0.0.3"));
        assert!(is_newer_semver("v0.1.0", "v0.0.9"));
        assert!(!is_newer_semver("0.0.3", "0.0.3"));
        assert!(!is_newer_semver("0.0.2", "0.0.3"));
        assert!(is_newer_semver("0.0.4", "0.0.4-rc.1"));
    }

    #[test]
    fn test_parse_github_release_api_payload_with_native_digest() {
        let api_json = serde_json::json!({
            "tag_name": "v0.0.4",
            "name": "ctdozlite v0.0.4",
            "body": "- Sửa lỗi Windows Defender\n- Tự động thông báo cập nhật qua GitHub API",
            "html_url": "https://github.com/ctdoteam1110/ctdozlite/releases/tag/v0.0.4",
            "published_at": "2026-11-02T03:00:00Z",
            "assets": [
                {
                    "name": "SHA256SUMS.txt",
                    "size": 512,
                    "browser_download_url": "https://github.com/ctdoteam1110/ctdozlite/releases/download/v0.0.4/SHA256SUMS.txt"
                },
                {
                    "name": "ctdozlite-0.0.4-setup-windows-x86_64.exe",
                    "size": 9500000,
                    "digest": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
                    "browser_download_url": "https://github.com/ctdoteam1110/ctdozlite/releases/download/v0.0.4/ctdozlite-0.0.4-setup-windows-x86_64.exe"
                },
                {
                    "name": "ctdozlite-0.0.4-portable-windows-x86_64.exe",
                    "size": 8400000,
                    "digest": "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
                    "browser_download_url": "https://github.com/ctdoteam1110/ctdozlite/releases/download/v0.0.4/ctdozlite-0.0.4-portable-windows-x86_64.exe"
                },
                {
                    "name": "ctdozlite-0.0.4-macos-arm64.dmg",
                    "size": 10200000,
                    "digest": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
                    "browser_download_url": "https://github.com/ctdoteam1110/ctdozlite/releases/download/v0.0.4/ctdozlite-0.0.4-macos-arm64.dmg"
                },
                {
                    "name": "ctdozlite-0.0.4-macos-arm64.tar.gz",
                    "size": 9800000,
                    "digest": "sha256:3333333333333333333333333333333333333333333333333333333333333333",
                    "browser_download_url": "https://github.com/ctdoteam1110/ctdozlite/releases/download/v0.0.4/ctdozlite-0.0.4-macos-arm64.tar.gz"
                }
            ]
        });

        let win_meta = parse_github_release_value(
            &api_json,
            "0.0.3",
            "ctdoteam1110/ctdozlite",
            "windows",
            "x86_64",
        );
        assert!(win_meta.has_update);
        assert_eq!(win_meta.latest_version, "0.0.4");
        assert_eq!(
            win_meta.asset_name.as_deref(),
            Some("ctdozlite-0.0.4-portable-windows-x86_64.exe")
        );
        assert_eq!(
            win_meta.expected_sha256.as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
        );
        assert_eq!(win_meta.published_at, "2026-11-02");

        let mac_meta = parse_github_release_value(
            &api_json,
            "0.0.3",
            "ctdoteam1110/ctdozlite",
            "macos",
            "aarch64",
        );
        assert_eq!(
            mac_meta.asset_name.as_deref(),
            Some("ctdozlite-0.0.4-macos-arm64.dmg")
        );
        assert_eq!(
            mac_meta.expected_sha256.as_deref(),
            Some("2222222222222222222222222222222222222222222222222222222222222222")
        );
    }

    #[test]
    fn test_platform_asset_scoring_across_os_and_formats() {
        // macOS prefers .dmg over .tar.gz and never selects Linux .tar.gz
        assert!(
            select_platform_asset_score("ctdozlite-0.1.0-macos-arm64.dmg", "macos", "aarch64")
                > select_platform_asset_score(
                    "ctdozlite-0.1.0-macos-arm64.tar.gz",
                    "macos",
                    "aarch64"
                )
        );
        assert!(
            select_platform_asset_score("ctdozlite-0.1.0-macos-arm64.tar.gz", "macos", "aarch64")
                > 0
        );
        assert_eq!(
            select_platform_asset_score("ctdozlite-0.1.0-linux-x86_64.tar.gz", "macos", "aarch64"),
            0
        );

        // Linux never selects macOS .tar.gz or Windows .exe
        assert_eq!(
            select_platform_asset_score("ctdozlite-0.1.0-macos-arm64.tar.gz", "linux", "x86_64"),
            0
        );
        assert!(
            select_platform_asset_score("ctdozlite-0.1.0-x86_64.AppImage", "linux", "x86_64")
                > select_platform_asset_score("ctdozlite_0.1.0_amd64.deb", "linux", "x86_64")
        );
    }

    #[test]
    fn test_verify_asset_integrity_checks_magic_and_sha256() {
        let pe_bytes = b"MZ\x90\x00portable-exe-payload";
        let pe_hash = format!("{:x}", Sha256::digest(pe_bytes));
        assert!(verify_asset_integrity(
            "ctdozlite-0.1.0-portable-windows-x86_64.exe",
            pe_bytes,
            Some(&pe_hash)
        )
        .is_ok());
        assert!(verify_asset_integrity(
            "ctdozlite-0.1.0-portable-windows-x86_64.exe",
            b"not-a-pe",
            None
        )
        .is_err());
        assert!(verify_asset_integrity(
            "ctdozlite-0.1.0-macos-arm64.tar.gz",
            &[0x1f, 0x8b, 0x08, 0x00],
            None
        )
        .is_ok());
        assert!(verify_asset_integrity(
            "ctdozlite-0.1.0-macos-arm64.tar.gz",
            b"plain-text",
            None
        )
        .is_err());
    }

    #[test]
    fn test_parse_sha256sums_for_asset() {
        let sums = "\
abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789  ctdozlite-0.0.4-portable-windows-x86_64.exe\n\
2222222222222222222222222222222222222222222222222222222222222222 *./ctdozlite-0.0.4-macos-arm64.dmg\n\
SHA256 (dist/ctdozlite-0.0.4-macos-arm64.tar.gz) = 3333333333333333333333333333333333333333333333333333333333333333\n";
        assert_eq!(
            parse_sha256sums_for_asset(sums, "ctdozlite-0.0.4-macos-arm64.dmg").as_deref(),
            Some("2222222222222222222222222222222222222222222222222222222222222222")
        );
        assert_eq!(
            parse_sha256sums_for_asset(sums, "ctdozlite-0.0.4-macos-arm64.tar.gz").as_deref(),
            Some("3333333333333333333333333333333333333333333333333333333333333333")
        );
    }
}
