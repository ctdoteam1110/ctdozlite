//! Bounded media decoding. Native codecs run in a short-lived macOS helper.
use anyhow::{anyhow, bail, Context, Result};
use image::{ImageDecoder, ImageFormat, ImageReader, RgbaImage};
use std::io::Cursor;

pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_EDGE: u32 = 480;

/// Image texture quality; full images retain raster dimensions within the shared pixel budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageQuality {
    Preview,
    Full,
}

impl ImageQuality {
    /// Separate full textures from preview entries in the account-private cache.
    pub fn for_texture_key(key: &str) -> Self {
        if key.starts_with("hd_") {
            Self::Full
        } else {
            Self::Preview
        }
    }
}
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

fn decode_raster(bytes: &[u8], quality: ImageQuality) -> Result<RgbaImage> {
    let format = image::guess_format(bytes)?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        bail!("Native image codec required");
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    limits.max_alloc = Some(96 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        bail!("Image dimensions exceed preview limits");
    }
    let orientation = decoder.orientation()?;
    let mut decoded = image::DynamicImage::from_decoder(decoder)?;
    decoded.apply_orientation(orientation);
    if quality == ImageQuality::Preview
        && (decoded.width() > MAX_EDGE || decoded.height() > MAX_EDGE)
    {
        decoded = decoded.thumbnail(MAX_EDGE, MAX_EDGE);
    }
    Ok(decoded.into_rgba8())
}

/// Decode at most 16 MiB into an orientation-correct preview with edge <=480.
/// CPU decoding uses the blocking pool; native decoding has a 10-second timeout.
#[cfg(test)]
pub async fn decode_preview(bytes: &[u8]) -> Result<RgbaImage> {
    decode_image(bytes, ImageQuality::Preview).await
}

/// Decode bounded image data at the requested quality. Native codecs retain the helper cap.
///
/// # Errors
/// Returns an error for oversized, malformed, unsupported or timed-out input.
pub async fn decode_image(bytes: &[u8], quality: ImageQuality) -> Result<RgbaImage> {
    if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
        bail!("Image input exceeds preview limits");
    }
    let owned = bytes.to_vec();
    let raster = tokio::task::spawn_blocking(move || decode_raster(&owned, quality))
        .await
        .context("Image decode task failed")?;
    match raster {
        Ok(image) => Ok(image),
        Err(_) => decode_native(bytes, quality).await,
    }
}

#[cfg(target_os = "macos")]
async fn decode_native(bytes: &[u8], quality: ImageQuality) -> Result<RgbaImage> {
    use std::process::Stdio;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let packaged = std::env::current_exe().ok().and_then(|path| {
        path.parent()?
            .parent()
            .map(|contents| contents.join("Resources/MediaPreview"))
    });
    let helper = packaged.filter(|path| path.is_file()).unwrap_or_else(|| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("native/macos/build/MediaPreview")
    });
    let mut child = tokio::process::Command::new(helper)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("Media preview helper unavailable")?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("Missing helper input"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Missing helper output"))?;
    let operation = async {
        let write = async move {
            stdin.write_all(bytes).await?;
            stdin.shutdown().await?;
            drop(stdin);
            Ok::<_, std::io::Error>(())
        };
        let read = async {
            let mut output = Vec::new();
            stdout
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut output)
                .await?;
            Ok::<_, std::io::Error>(output)
        };
        let (_, output) = tokio::try_join!(write, read)?;
        if output.len() > 8 * 1024 * 1024 {
            bail!("Helper output exceeds preview limits");
        }
        let status = child.wait().await?;
        if !status.success() {
            bail!("Native media preview failed");
        }
        Ok::<_, anyhow::Error>(output)
    };
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), operation)
        .await
        .context("Media preview helper timed out")??;
    tokio::task::spawn_blocking(move || decode_raster(&output, quality))
        .await
        .context("Preview decode task failed")?
}

#[cfg(not(target_os = "macos"))]
async fn decode_native(_: &[u8], _: ImageQuality) -> Result<RgbaImage> {
    Err(anyhow!("Unsupported media preview format"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rejects_empty_and_oversized_input() {
        assert!(decode_preview(&[]).await.is_err());
        assert!(decode_preview(&vec![0; MAX_INPUT_BYTES + 1]).await.is_err());
    }
    #[test]
    fn hd_texture_key_selects_full_quality_only_for_explicit_viewer_requests() {
        assert_eq!(
            ImageQuality::for_texture_key("hd_fixture"),
            ImageQuality::Full
        );
        assert_eq!(
            ImageQuality::for_texture_key("preview_fixture"),
            ImageQuality::Preview
        );
        assert_eq!(
            ImageQuality::for_texture_key("shd_fixture"),
            ImageQuality::Preview
        );
    }

    #[test]
    fn raster_thumbnail_is_bounded() {
        let image = image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            2048,
            1024,
            image::Rgba([10, 20, 30, 255]),
        ));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        let result = decode_raster(bytes.get_ref(), ImageQuality::Preview).unwrap();
        assert_eq!(result.dimensions(), (480, 240));
        let full = decode_raster(bytes.get_ref(), ImageQuality::Full).unwrap();
        assert_eq!(full.dimensions(), (2048, 1024));
    }
}

/// Recognize photo cards while excluding explicit video metadata before any source retry.
pub(super) fn is_photo_message(message: &crate::chat::ChatMessage) -> bool {
    let text = message.text.trim();
    message.link_src.as_deref() != Some("Video Zalo")
        && !text.starts_with("[Video]")
        && (message.link_src.as_deref() == Some("Hình ảnh Zalo")
            || text == "[Hình ảnh]"
            || text.starts_with("[Hình ảnh] "))
}

/// Prefer namespaced authoritative wire identity over a potentially shared canonical alias.
pub(super) fn message_identity(message: &crate::chat::ChatMessage) -> Option<String> {
    fn scalar(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::String(value) if !value.is_empty() => Some(value.clone()),
            serde_json::Value::Number(value) => Some(value.to_string()),
            _ => None,
        }
    }
    for (namespace, value) in [("server", message.wire_ref.server_id.as_ref()), ("client", message.wire_ref.client_id.as_ref())] {
        if let Some(value) = value.and_then(scalar) { return Some(format!("{namespace}:{value}")); }
    }
    (!message.id.is_empty()).then(|| format!("canonical:{}",message.id))
}

/// Build a photo resource identity consistently for thumbnail and original requests.
pub(super) fn photo_key_for_message(account: &str, thread: &str, message: &crate::chat::ChatMessage) -> crate::media_resources::MediaKey {
    photo_key(account, thread, &message_identity(message).unwrap_or_default())
}

/// Build the account/thread/message/quality texture identity, independent of signed URLs.
pub(super) fn photo_key(
    account: &str,
    thread: &str,
    message: &str,
) -> crate::media_resources::MediaKey {
    crate::media_resources::MediaKey {
        account: account.into(),
        thread: thread.into(),
        message: message.into(),
        quality: crate::media_resources::MediaQuality::Thumbnail,
    }
}

/// Fetch a photo thumbnail, then retry its unchanged source only after fetch/decode failure.
///
/// # Errors
/// Returns a bounded transport, cancellation or decoder error. This API is photo-only.
pub(super) async fn load_photo(
    client: &reqwest::Client,
    key: &crate::media_resources::MediaKey,
    thumbnail: &str,
    source: Option<&str>,
    generation: &crate::media_resources::RequestGeneration,
    ticket: u64,
) -> Result<RgbaImage, crate::media_resources::MediaError> {
    load_photo_at(
        client,
        key,
        thumbnail,
        source,
        generation,
        ticket,
        std::path::Path::new("media_cache"),
    )
    .await
}

async fn load_photo_at(
    client: &reqwest::Client,
    key: &crate::media_resources::MediaKey,
    thumbnail: &str,
    source: Option<&str>,
    generation: &crate::media_resources::RequestGeneration,
    ticket: u64,
    root: &std::path::Path,
) -> Result<RgbaImage, crate::media_resources::MediaError> {
    use crate::media_resources::MediaError;
    if key.account.is_empty() || key.thread.is_empty() || key.message.is_empty() {
        return Err(MediaError::AccountMismatch);
    }
    match load_one(client, key.clone(), thumbnail, generation, ticket, root).await {
        Ok(image) => Ok(image),
        Err(MediaError::Cancelled) => Err(MediaError::Cancelled),
        Err(error) => match source.filter(|url| !url.is_empty() && *url != thumbnail) {
            Some(url) => {
                generation.check(ticket)?;
                {
                    let mut source_key = key.clone();
                    source_key.quality = crate::media_resources::MediaQuality::Preview;
                    load_one(client, source_key, url, generation, ticket, root).await
                }
            }
            None => Err(error),
        },
    }
}

async fn load_one(
    client: &reqwest::Client,
    key: crate::media_resources::MediaKey,
    url: &str,
    generation: &crate::media_resources::RequestGeneration,
    ticket: u64,
    root: &std::path::Path,
) -> Result<RgbaImage, crate::media_resources::MediaError> {
    use crate::media_resources::{
        fetch_bounded, AtomicPrivateFile, MediaError, PrivateMediaCache, PREVIEW_LIMIT,
    };
    generation.check(ticket)?;
    let quality = if key.quality == crate::media_resources::MediaQuality::Original {
        ImageQuality::Full
    } else {
        ImageQuality::Preview
    };
    let disk_key = key.clone();
    let read_root = root.to_owned();
    let write_root = root.to_owned();
    let cached = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let cache = PrivateMediaCache::new(&read_root, &disk_key.account).ok()?;
        let path = cache.path_for(&disk_key).ok()?;
        let file = std::fs::File::open(path).ok()?;
        let mut bytes = Vec::new();
        file.take(PREVIEW_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        (bytes.len() <= PREVIEW_LIMIT).then_some(bytes)
    })
    .await
    .map_err(|_| MediaError::Io)?;
    if let Some(bytes) = cached {
        if let Ok(image) = decode_image(&bytes, quality).await {
            generation.check(ticket)?;
            return Ok(image);
        }
    }
    let bytes = fetch_bounded(client, url, PREVIEW_LIMIT, generation, ticket).await?;
    let image = decode_image(&bytes, quality)
        .await
        .map_err(|_| MediaError::Decoder)?;
    generation.check(ticket)?;
    let generation_copy = generation.clone();
    tokio::task::spawn_blocking(move || {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let Ok(cache) = PrivateMediaCache::new(&write_root, &key.account) else {
            return;
        };
        let Ok(path) = cache.path_for(&key) else {
            return;
        };
        let suffix = format!(
            "preview-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        if let Ok(mut file) = AtomicPrivateFile::create(&path, &suffix) {
            if file.write_all(&bytes).is_ok() && generation_copy.is_current(ticket) {
                let _ = file.commit();
            }
        }
        let _ = cache.prune();
    })
    .await
    .map_err(|_| MediaError::Io)?;
    generation.check(ticket)?;
    Ok(image)
}

#[cfg(test)]
mod delivery_regressions {
    use super::*;
    #[tokio::test]
    async fn failed_thumbnail_retries_unchanged_source_after_fetch_or_decode_error() {
        use std::io::{Read, Write};
        for status in ["404 Not Found", "200 OK"] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let mut png = Cursor::new(Vec::new());
            image::DynamicImage::new_rgba8(12, 8)
                .write_to(&mut png, ImageFormat::Png)
                .unwrap();
            let png = png.into_inner();
            let server = std::thread::spawn(move || {
                let mut paths = Vec::new();
                for index in 0..2 {
                    let (mut socket, _) = listener.accept().unwrap();
                    let mut request = [0; 2048];
                    let length = socket.read(&mut request).unwrap();
                    paths.push(
                        String::from_utf8_lossy(&request[..length])
                            .lines()
                            .next()
                            .unwrap()
                            .to_owned(),
                    );
                    let body = if index == 0 {
                        b"broken".as_slice()
                    } else {
                        png.as_slice()
                    };
                    let code = if index == 0 { status } else { "200 OK" };
                    write!(
                        socket,
                        "HTTP/1.1 {code}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .unwrap();
                    socket.write_all(body).unwrap();
                }
                paths
            });
            let root = std::env::temp_dir().join(format!(
                "zl-photo-{}-{}",
                std::process::id(),
                address.port()
            ));
            let generation = crate::media_resources::RequestGeneration::default();
            let key = photo_key("account-fixture", "thread-fixture", "message-fixture");
            let source = format!("http://{address}/source?token=a%2Fb&size=0");
            let result = load_photo_at(
                &reqwest::Client::new(),
                &key,
                &format!("http://{address}/thumbnail"),
                Some(&source),
                &generation,
                generation.capture(),
                &root,
            )
            .await
            .unwrap();
            assert_eq!(result.dimensions(), (12, 8));
            assert_eq!(
                server.join().unwrap(),
                vec![
                    "GET /thumbnail HTTP/1.1",
                    "GET /source?token=a%2Fb&size=0 HTTP/1.1"
                ]
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn video_card_never_uses_photo_source_retry() {
        let source = include_str!("messages.rs");
        let photo = source.find("if is_zalo_photo {").unwrap();
        let command = source.find("WorkerCmd::FetchPhotoPreview").unwrap();
        let video = source[command..].find("Video Zalo").unwrap() + command;
        assert!(photo < command && command < video);
        assert_eq!(source.matches("WorkerCmd::FetchPhotoPreview").count(), 1);
        let mut message = crate::chat::ChatMessage {
            text: "[Hình ảnh]".into(),
            ..Default::default()
        };
        assert!(is_photo_message(&message));
        message.link_src = Some("Video Zalo".into());
        assert!(!is_photo_message(&message));
        message.link_src = Some("Hình ảnh Zalo".into());
        message.text = "[Video]".into();
        assert!(!is_photo_message(&message));
    }
    #[test]
    fn texture_identity_isolates_account_thread_message_and_quality() {
        let first = photo_key("a", "t", "m");
        for key in [
            photo_key("b", "t", "m"),
            photo_key("a", "u", "m"),
            photo_key("a", "t", "n"),
        ] {
            assert_ne!(first.cache_id(), key.cache_id());
        }
        let mut preview = first.clone();
        preview.quality = crate::media_resources::MediaQuality::Preview;
        assert_ne!(first.cache_id(), preview.cache_id());
    }
}

#[cfg(test)]
mod authoritative_identity_regressions {
    use super::*;
    #[tokio::test]
    async fn shared_canonical_alias_with_distinct_server_ids_fetches_both_photos() {
        use std::io::{Read,Write};
        let first = crate::chat::ChatMessage { id: "shared-client-alias".into(), wire_ref: crate::message_model::WireMessageRef { server_id: Some(serde_json::json!("server-A")), ..Default::default() }, ..Default::default() };
        let mut second = first.clone(); second.wire_ref.server_id = Some(serde_json::json!("server-B"));
        let key_a = photo_key_for_message("account", "thread", &first);
        let key_b = photo_key_for_message("account", "thread", &second);
        assert_ne!(key_a.cache_id(), key_b.cache_id());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut png = Cursor::new(Vec::new());
            image::DynamicImage::new_rgba8(4,3).write_to(&mut png, ImageFormat::Png).unwrap();
            let bytes = png.into_inner();
            let mut paths = Vec::new();
            for _ in 0..2 {
                let (mut socket,_) = listener.accept().unwrap(); let mut request=[0;1024];
                let size = socket.read(&mut request).unwrap();
                paths.push(String::from_utf8_lossy(&request[..size]).lines().next().unwrap().to_owned());
                write!(socket,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len()).unwrap();
                socket.write_all(&bytes).unwrap();
            }
            paths
        });
        let root = std::env::temp_dir().join(format!("zl-photo-alias-{}-{}",std::process::id(),address.port()));
        let generation = crate::media_resources::RequestGeneration::default();
        let client = reqwest::Client::new();
        for (key, suffix) in [(key_a,"a"),(key_b,"b")] {
            let url = format!("http://{address}/{suffix}");
            load_photo_at(&client,&key,&url,None,&generation,generation.capture(),&root).await.unwrap();
        }
        assert_eq!(server.join().unwrap(),vec!["GET /a HTTP/1.1","GET /b HTTP/1.1"]);
        std::fs::remove_dir_all(root).unwrap();
        assert!(message_identity(&crate::chat::ChatMessage::default()).is_none());
    }
}

/// A private viewer copy that is neither a saved original nor a cache-pruning candidate.
pub(super) struct NativePhotoLease(std::path::PathBuf);
impl NativePhotoLease {
    /// Return the verified local path while this lease remains alive.
    pub(super) fn path(&self) -> &std::path::Path { &self.0 }
}
impl Drop for NativePhotoLease {
    fn drop(&mut self) {
        // Best-effort cleanup also runs if the owning viewer task is cancelled.
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Validate through the bounded Original pipeline, then lease a private local copy for a viewer.
///
/// # Errors
/// Returns bounded download/decode, cache validation or private-file errors.
pub(super) async fn prepare_native_photo(client: &reqwest::Client, key: crate::media_resources::MediaKey, url: &str, root: &std::path::Path, generation: &crate::media_resources::RequestGeneration, ticket: u64) -> Result<NativePhotoLease, crate::media_resources::MediaError> {
    use crate::media_resources::{AtomicPrivateFile, PrivateMediaCache, MediaError, MediaQuality, PREVIEW_LIMIT};
    if key.quality != MediaQuality::Original { return Err(MediaError::Decoder); }
    let _validated = load_one(client, key.clone(), url, generation, ticket, root).await?;
    generation.check(ticket)?;
    let root = root.to_owned();
    let generation = generation.clone();
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let cache = PrivateMediaCache::new(&root, &key.account)?;
        let source = cache.path_for(&key)?;
        let metadata = std::fs::symlink_metadata(&source).map_err(|_| MediaError::Io)?;
        if !metadata.is_file() { return Err(MediaError::Io); }
        if metadata.len() == 0 || metadata.len() > PREVIEW_LIMIT as u64 { return Err(MediaError::TooLarge); }
        let mut bytes = Vec::new();
        std::fs::File::open(&source).map_err(|_| MediaError::Io)?.take(PREVIEW_LIMIT as u64 + 1).read_to_end(&mut bytes).map_err(|_| MediaError::Io)?;
        if bytes.len() > PREVIEW_LIMIT { return Err(MediaError::TooLarge); }
        generation.check(ticket)?;
        let name = format!("viewer-{}-{}.media",std::process::id(),NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed));
        let directory = source.parent().ok_or(MediaError::Io)?.canonicalize().map_err(|_| MediaError::Io)?;
        let lease = directory.join(name);
        let mut file = AtomicPrivateFile::create(&lease,"viewer")?;
        file.write_all(&bytes)?;
        generation.check(ticket)?;
        file.commit()?;
        Ok(NativePhotoLease(lease))
    }).await.map_err(|_| MediaError::Io)?
}
