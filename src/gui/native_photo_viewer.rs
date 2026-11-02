//! Explicit native photo viewing with account-private, lifetime-owned local files.
use crate::media_resources::{MediaError, MediaKey, MediaQuality, RequestGeneration};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

fn packaged_helper(executable: &Path) -> Option<PathBuf> {
    Some(
        executable
            .parent()?
            .parent()?
            .join("Resources/DailyMediaViewer"),
    )
}
fn helper_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| packaged_helper(&path))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("native/macos/build/DailyMediaViewer")
        })
}

/// Open only from an explicit photo-viewer button; the egui viewer stays available on failure.
///
/// # Errors
/// Returns account/quality validation, bounded download/decode, private-file or helper errors.
pub(super) async fn open(
    client: &reqwest::Client,
    current_account: &str,
    key: MediaKey,
    url: &str,
) -> Result<(), MediaError> {
    if !cfg!(target_os = "macos") {
        return Err(MediaError::Io);
    }
    let helper = crate::runtime_scope::spawn_blocking(helper_path)
        .await
        .map_err(|_| MediaError::Io)?
        .map_err(|_| MediaError::Cancelled)?;
    open_at(
        client,
        current_account,
        key,
        url,
        Path::new("media_cache"),
        &helper,
    )
    .await
}
async fn open_at(
    client: &reqwest::Client,
    current_account: &str,
    key: MediaKey,
    url: &str,
    root: &Path,
    helper: &Path,
) -> Result<(), MediaError> {
    if current_account.is_empty()
        || current_account != key.account
        || key.thread.is_empty()
        || key.message.is_empty()
    {
        return Err(MediaError::AccountMismatch);
    }
    if key.quality != MediaQuality::Original {
        return Err(MediaError::Decoder);
    }
    let generation = RequestGeneration::default();
    let file = super::media_preview::prepare_native_photo(
        client,
        key,
        url,
        root,
        &generation,
        generation.capture(),
    )
    .await?;
    // No shell and no remote URL reaches the helper. The lease is held until child exit/drop.
    let mut child = {
        let _guard = crate::runtime_scope::mutation().map_err(|_| MediaError::Cancelled)?;
        tokio::process::Command::new(helper)
            .arg("--file")
            .arg(file.path())
            .arg("--mime")
            .arg("image/*")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| MediaError::Io)?
    };
    let result = child.wait().await.map_err(|_| MediaError::Io)?;
    if result.success() {
        Ok(())
    } else {
        Err(MediaError::Decoder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};
    struct Fixture {
        root: PathBuf,
        helper: PathBuf,
        key: MediaKey,
    }
    impl Fixture {
        fn new(body: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "zl-native-viewer-{}-{} with 'quote'",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            let helper = root.join("fixture helper");
            std::fs::write(&helper, format!("#!/bin/sh\n{body}\n")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            let mut key = super::super::media_preview::photo_key(
                "fixture-account",
                "fixture-thread",
                "fixture-message",
            );
            key.quality = MediaQuality::Original;
            Self { root, helper, key }
        }
        fn cache(&self) -> PathBuf {
            self.root.join("private cache; untouched")
        }
        fn seed(&self) -> PathBuf {
            let cache =
                crate::media_resources::PrivateMediaCache::new(&self.cache(), &self.key.account)
                    .unwrap();
            let path = cache.path_for(&self.key).unwrap();
            let mut png = Cursor::new(Vec::new());
            image::DynamicImage::new_rgba8(7, 5)
                .write_to(&mut png, image::ImageFormat::Png)
                .unwrap();
            let mut file =
                crate::media_resources::AtomicPrivateFile::create(&path, "fixture").unwrap();
            file.write_all(png.get_ref()).unwrap();
            file.commit().unwrap();
            path
        }
        fn args(&self) -> Vec<String> {
            std::fs::read_to_string(self.helper.with_extension("args"))
                .unwrap()
                .lines()
                .map(str::to_owned)
                .collect()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    const CAPTURE: &str = "printf '%s\\n' \"$@\" > \"$0.args\"; test \"$1\" = '--file' && test -f \"$2\" && test \"$3\" = '--mime' && test \"$4\" = 'image/*'";

    #[cfg(unix)]
    #[tokio::test]
    async fn exact_arguments_private_lease_cleanup_and_original_cache_retention() {
        let fixture = Fixture::new(CAPTURE);
        let original = fixture.seed();
        open_at(
            &reqwest::Client::new(),
            "fixture-account",
            fixture.key.clone(),
            "http://127.0.0.1:1/not-fetched",
            &fixture.cache(),
            &fixture.helper,
        )
        .await
        .unwrap();
        let args = fixture.args();
        assert_eq!(args.len(), 4);
        assert_eq!(args[0], "--file");
        assert_eq!(args[2], "--mime");
        assert_eq!(args[3], "image/*");
        assert!(args[1].contains(" with 'quote'"));
        assert!(!Path::new(&args[1]).exists());
        assert!(original.is_file());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&original).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(original.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn failed_missing_and_cancelled_helpers_release_only_the_owned_lease() {
        let fixture = Fixture::new(&format!("{CAPTURE}; exit 3"));
        let original = fixture.seed();
        assert_eq!(
            open_at(
                &reqwest::Client::new(),
                "fixture-account",
                fixture.key.clone(),
                "unused",
                &fixture.cache(),
                &fixture.helper
            )
            .await,
            Err(MediaError::Decoder)
        );
        assert!(!Path::new(&fixture.args()[1]).exists());
        assert!(original.exists());
        assert_eq!(
            open_at(
                &reqwest::Client::new(),
                "fixture-account",
                fixture.key.clone(),
                "unused",
                &fixture.cache(),
                &fixture.root.join("missing helper")
            )
            .await,
            Err(MediaError::Io)
        );
        let fixture = Fixture::new(&format!("echo $$ > \"$0.pid\"; {CAPTURE}; exec sleep 30"));
        let original = fixture.seed();
        let key = fixture.key.clone();
        let cache = fixture.cache();
        let helper = fixture.helper.clone();
        let task = tokio::spawn(async move {
            open_at(
                &reqwest::Client::new(),
                "fixture-account",
                key,
                "unused",
                &cache,
                &helper,
            )
            .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while !fixture.helper.with_extension("args").exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let lease = fixture.args()[1].clone();
        assert!(Path::new(&lease).exists());
        let pid = std::fs::read_to_string(fixture.helper.with_extension("pid"))
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap()
            .to_string();
        let alive = || {
            std::process::Command::new("/bin/kill")
                .arg("-0")
                .arg(&pid)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        };
        assert!(alive());
        task.abort();
        let _ = task.await;
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while alive() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!Path::new(&lease).exists());
        assert!(original.exists());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn account_mismatch_and_invalid_cache_never_launch_a_helper() {
        let fixture = Fixture::new(CAPTURE);
        assert_eq!(
            open_at(
                &reqwest::Client::new(),
                "another-account",
                fixture.key.clone(),
                "unused",
                &fixture.cache(),
                &fixture.helper
            )
            .await,
            Err(MediaError::AccountMismatch)
        );
        assert!(!fixture.helper.with_extension("args").exists());
        let original = fixture.seed();
        std::fs::write(&original, b"corrupt image").unwrap();
        assert!(open_at(
            &reqwest::Client::new(),
            "fixture-account",
            fixture.key.clone(),
            "unused",
            &fixture.cache(),
            &fixture.helper
        )
        .await
        .is_err());
        assert!(!fixture.helper.with_extension("args").exists());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn original_source_query_is_unchanged_when_native_viewer_prepares_a_photo() {
        let fixture = Fixture::new(CAPTURE);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 2048];
            let size = socket.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..size])
                .lines()
                .next()
                .unwrap()
                .to_owned();
            let mut png = Cursor::new(Vec::new());
            image::DynamicImage::new_rgba8(5, 3)
                .write_to(&mut png, image::ImageFormat::Png)
                .unwrap();
            let bytes = png.into_inner();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .unwrap();
            socket.write_all(&bytes).unwrap();
            request
        });
        open_at(
            &reqwest::Client::new(),
            "fixture-account",
            fixture.key.clone(),
            &format!("http://{address}/source?token=a%2Fb&size=0"),
            &fixture.cache(),
            &fixture.helper,
        )
        .await
        .unwrap();
        assert_eq!(
            server.join().unwrap(),
            "GET /source?token=a%2Fb&size=0 HTTP/1.1"
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_cache_is_rejected_before_native_process_launch() {
        let fixture = Fixture::new(CAPTURE);
        let source = fixture.seed();
        let target = source.with_extension("actual");
        std::fs::rename(&source, &target).unwrap();
        std::os::unix::fs::symlink(&target, &source).unwrap();
        assert_eq!(
            open_at(
                &reqwest::Client::new(),
                "fixture-account",
                fixture.key.clone(),
                "unused",
                &fixture.cache(),
                &fixture.helper
            )
            .await,
            Err(MediaError::Io)
        );
        assert!(!fixture.helper.with_extension("args").exists());
        assert!(target.exists());
    }

    #[test]
    fn packaged_helper_relocates_and_native_dispatch_remains_an_explicit_button_action() {
        assert_eq!(
            packaged_helper(Path::new("/Applications/Client.app/Contents/MacOS/Client")).unwrap(),
            Path::new("/Applications/Client.app/Contents/Resources/DailyMediaViewer")
        );
        let modal = include_str!("modals.rs");
        assert_eq!(modal.matches("WorkerCmd::OpenNativePhoto").count(), 1);
        assert!(modal.contains("Mở trình xem gốc"));
        assert!(!include_str!("messages.rs").contains("WorkerCmd::OpenNativePhoto"));
    }
}
