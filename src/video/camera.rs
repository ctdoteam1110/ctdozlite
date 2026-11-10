//! Native camera discovery and RGB capture isolated in a same-executable helper.
//!
//! A killed helper releases blocking driver reads before its owned pipe reader is joined.

use anyhow::{anyhow, bail, Context, Result};
use nokhwa::{
    pixel_format::RgbFormat,
    utils::{CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType},
    Camera, FormatDecoder,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    sync::{mpsc, Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const LIST_FLAG: &str = "--native-camera-list";
const CAPTURE_FLAG: &str = "--native-camera-capture";
const MAX_LIST_BYTES: usize = 1024 * 1024;
const MAX_DEVICES: usize = 128;
const MAX_ID_BYTES: usize = 4096;
const MAX_WIDTH: u32 = 1920;
const MAX_HEIGHT: u32 = 1088;
const MAGIC: &[u8; 8] = b"ZCAMRGB1";
const HEADER_LEN: usize = 20;
const CAMERA_STOPPED: &str =
    "Camera đã dừng hoặc không khả dụng. Kiểm tra kết nối, quyền camera và định dạng được hỗ trợ.";

/// A native backend identifier and its human-readable camera name.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CameraDevice {
    /// Backend-qualified device key; display names are never used as native indices.
    pub id: String,
    /// Camera name reported by the operating system.
    pub label: String,
}

/// An owned, tightly packed RGB888 camera frame.
#[derive(Debug)]
pub struct RawFrame {
    /// Image width in pixels, bounded to 1920.
    pub width: u32,
    /// Image height in pixels, bounded to 1088.
    pub height: u32,
    /// Row-major RGB bytes, exactly `width * height * 3` bytes.
    pub rgb: Vec<u8>,
}

/// Resolve an exact native key or an unambiguous legacy display name.
///
/// # Errors
/// Returns an error for unavailable devices, ambiguous labels or an empty device list.
pub fn resolve_device(devices: &[CameraDevice], selection: &str) -> Result<String> {
    validate_devices(devices)?;
    if selection.is_empty() {
        return devices
            .first()
            .map(|device| device.id.clone())
            .ok_or_else(|| anyhow!("Không tìm thấy camera."));
    }
    if let Some(device) = devices.iter().find(|device| device.id == selection) {
        return Ok(device.id.clone());
    }
    let mut matching = devices.iter().filter(|device| device.label == selection);
    let first = matching
        .next()
        .ok_or_else(|| anyhow!("Camera đã chọn không còn khả dụng."))?;
    if matching.next().is_some() {
        bail!("Có nhiều camera cùng tên. Hãy chọn lại camera bằng mã thiết bị.");
    }
    Ok(first.id.clone())
}

/// Discover native devices in a helper without opening their capture streams.
///
/// # Errors
/// Returns an error after five seconds, on invalid/oversized output or backend failure.
/// Call this from a worker; never from the UI thread (INV-3).
pub fn list_devices() -> Result<Vec<CameraDevice>> {
    let mut helper = HelperChild::spawn(&[LIST_FLAG])?;
    let stdout = helper
        .child
        .stdout
        .take()
        .context("Không đọc được danh sách camera.")?;
    let (tx, rx) = mpsc::sync_channel(1);
    let reader = thread::Builder::new()
        .name("native-camera-list".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let result = stdout
                .take((MAX_LIST_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map(|_| bytes);
            // The owner may already have timed out and closed the receiver.
            let _ = tx.send(result);
        })
        .context("Không tạo được luồng đọc camera.")?;
    let output = rx.recv_timeout(Duration::from_secs(5));
    let cleanup = helper.stop();
    let joined = reader.join();
    cleanup?;
    joined.map_err(|_| anyhow!("Luồng đọc danh sách camera đã dừng."))?;
    let output = output
        .context("Camera không trả lời trong 5 giây.")?
        .context("Không đọc được danh sách camera.")?;
    parse_devices(&output)
}

/// Execute a private native helper command before loading application credentials.
///
/// # Errors
/// Returns an error for invalid helper arguments or unavailable native capture.
/// `args` includes the executable name; `false` means normal application startup.
pub fn run_helper_if_requested(args: &[String]) -> Result<bool> {
    match args.get(1).map(String::as_str) {
        Some(LIST_FLAG) => {
            if args.len() != 2 {
                bail!("Tham số liệt kê camera không hợp lệ.");
            }
            let devices = native_devices()?;
            let bytes = serde_json::to_vec(&devices)?;
            if bytes.len() > MAX_LIST_BYTES {
                bail!("Danh sách camera vượt giới hạn.");
            }
            std::io::stdout().lock().write_all(&bytes)?;
            Ok(true)
        }
        Some(CAPTURE_FLAG) => {
            if args.len() != 3 {
                bail!("Tham số mở camera không hợp lệ.");
            }
            capture_native(&args[2])?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

#[derive(Default)]
struct FrameSlot {
    latest: Option<RawFrame>,
    error: Option<String>,
}

type Mailbox = Arc<(Mutex<FrameSlot>, Condvar)>;

/// Owns one capture helper and its bounded, coalescing pipe reader.
pub struct CameraProcess {
    helper: HelperChild,
    reader: Option<JoinHandle<()>>,
    mailbox: Mailbox,
    stopped: bool,
}

impl CameraProcess {
    /// Start capture for an exact key returned by native discovery.
    ///
    /// # Errors
    /// Returns an error if the key is malformed or the helper cannot be started.
    /// Device-open failures are reported by `poll_frame` without blocking the caller.
    pub fn spawn(device_id: &str) -> Result<Self> {
        parse_index(device_id)?;
        let mut helper = HelperChild::spawn(&[CAPTURE_FLAG, device_id])?;
        let stdout = helper
            .child
            .stdout
            .take()
            .context("Không đọc được hình ảnh camera.")?;
        let mailbox: Mailbox = Arc::default();
        let producer = Arc::clone(&mailbox);
        let reader = thread::Builder::new()
            .name("native-camera-frames".into())
            .spawn(move || read_frames(stdout, producer))
            .context("Không tạo được luồng đọc camera.")?;
        Ok(Self {
            helper,
            reader: Some(reader),
            mailbox,
            stopped: false,
        })
    }

    /// Take the newest available frame, waiting at most `timeout` for pipe input.
    ///
    /// # Errors
    /// Returns an error and stops/reaps the helper on malformed input or disconnection.
    /// This blocking method belongs on the media worker, not the UI thread (INV-3).
    pub fn poll_frame(&mut self, timeout: Duration) -> Result<Option<RawFrame>> {
        if self.stopped {
            bail!("Camera đã dừng.");
        }
        let (lock, ready) = &*self.mailbox;
        let state = lock.lock().unwrap_or_else(|error| error.into_inner());
        let (mut state, _) = ready
            .wait_timeout_while(state, timeout, |state| {
                state.latest.is_none() && state.error.is_none()
            })
            .unwrap_or_else(|error| error.into_inner());
        if let Some(frame) = state.latest.take() {
            return Ok(Some(frame));
        }
        let error = state.error.take();
        drop(state);
        if let Some(error) = error {
            self.stop()?;
            bail!(error);
        }
        Ok(None)
    }

    /// Kill and reap the helper, then join its pipe reader; repeated calls are safe.
    ///
    /// # Errors
    /// Returns an error if the operating system cannot terminate/reap the helper,
    /// or the pipe reader panicked. Driver reads run only in the killed process.
    pub fn stop(&mut self) -> Result<()> {
        self.stopped = true;
        self.helper.stop()?;
        if let Some(reader) = self.reader.take() {
            reader
                .join()
                .map_err(|_| anyhow!("Luồng đọc camera đã dừng bất thường."))?;
        }
        let mut slot = self
            .mailbox
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        slot.latest = None;
        Ok(())
    }
}

impl Drop for CameraProcess {
    fn drop(&mut self) {
        // Cleanup cannot report errors during unwinding; stop remains available explicitly.
        let _ = self.stop();
    }
}

struct HelperChild {
    child: Child,
    reaped: bool,
}

impl HelperChild {
    fn spawn(args: &[&str]) -> Result<Self> {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args(args)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            // No backend diagnostics may corrupt RGB stdout or accumulate without bounds.
            .stderr(Stdio::null());
        for key in ["SystemRoot", "WINDIR", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW avoids a console flash.
        }
        Ok(Self {
            child: command
                .spawn()
                .context("Không khởi chạy được camera gốc.")?,
            reaped: false,
        })
    }

    fn stop(&mut self) -> Result<()> {
        if self.reaped {
            return Ok(());
        }
        if self.child.try_wait()?.is_none() {
            // The owned helper spawns no descendants; killing it closes the sole pipe writer.
            if let Err(error) = self.child.kill() {
                if self.child.try_wait()?.is_none() {
                    return Err(error).context("Không dừng được camera gốc.");
                }
            }
        }
        self.child
            .wait()
            .context("Không thu hồi được camera gốc.")?;
        self.reaped = true;
        Ok(())
    }
}

impl Drop for HelperChild {
    fn drop(&mut self) {
        // Every early return must still release the native driver process.
        let _ = self.stop();
    }
}

fn read_frames(mut reader: impl Read, mailbox: Mailbox) {
    loop {
        let result = read_frame(&mut reader);
        // A single replaceable slot prevents a slow encoder from accumulating raw frames.
        // This short lock protects metadata/ownership only; pipe I/O occurs before it.
        let mut slot = mailbox.0.lock().unwrap_or_else(|error| error.into_inner());
        match result {
            Ok(frame) => slot.latest = Some(frame),
            Err(error) => {
                slot.error = Some(error.to_string());
                mailbox.1.notify_one();
                break;
            }
        }
        mailbox.1.notify_one();
    }
}

fn rgb_len(width: u32, height: u32) -> Result<usize> {
    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
        bail!("Kích thước hình ảnh camera vượt giới hạn 1920×1088.");
    }
    Ok(width as usize * height as usize * 3)
}

fn read_frame(reader: &mut impl Read) -> Result<RawFrame> {
    let mut header = [0_u8; HEADER_LEN];
    reader.read_exact(&mut header).context(CAMERA_STOPPED)?;
    if &header[..8] != MAGIC {
        bail!("Dữ liệu camera không đúng phiên bản.");
    }
    let width = u32::from_le_bytes(header[8..12].try_into()?);
    let height = u32::from_le_bytes(header[12..16].try_into()?);
    let declared_len = u32::from_le_bytes(header[16..20].try_into()?) as usize;
    let expected_len = rgb_len(width, height)?;
    if declared_len != expected_len {
        bail!("Độ dài hình ảnh camera không hợp lệ.");
    }
    let mut rgb = vec![0; expected_len];
    reader.read_exact(&mut rgb).context(CAMERA_STOPPED)?;
    Ok(RawFrame { width, height, rgb })
}

fn write_frame(writer: &mut impl Write, frame: &RawFrame) -> Result<()> {
    let len = rgb_len(frame.width, frame.height)?;
    if frame.rgb.len() != len {
        bail!("Độ dài hình ảnh camera không hợp lệ.");
    }
    let mut header = [0; HEADER_LEN];
    header[..8].copy_from_slice(MAGIC);
    header[8..12].copy_from_slice(&frame.width.to_le_bytes());
    header[12..16].copy_from_slice(&frame.height.to_le_bytes());
    header[16..20].copy_from_slice(&(len as u32).to_le_bytes());
    writer.write_all(&header)?;
    writer.write_all(&frame.rgb)?;
    writer.flush()?;
    Ok(())
}

fn index_key(index: &CameraIndex) -> String {
    match index {
        CameraIndex::Index(index) => format!("nokhwa:index:{index}"),
        CameraIndex::String(index) => format!("nokhwa:string:{index}"),
    }
}

fn parse_index(key: &str) -> Result<CameraIndex> {
    if key.len() > MAX_ID_BYTES || key.contains('\0') {
        bail!("Mã camera không hợp lệ.");
    }
    if let Some(index) = key.strip_prefix("nokhwa:index:") {
        return Ok(CameraIndex::Index(
            index.parse().context("Mã camera không hợp lệ.")?,
        ));
    }
    if let Some(index) = key
        .strip_prefix("nokhwa:string:")
        .filter(|id| !id.is_empty())
    {
        return Ok(CameraIndex::String(index.to_owned()));
    }
    bail!("Mã camera không hợp lệ.")
}

fn validate_devices(devices: &[CameraDevice]) -> Result<()> {
    if devices.len() > MAX_DEVICES {
        bail!("Danh sách camera vượt giới hạn 128 thiết bị.");
    }
    for (position, device) in devices.iter().enumerate() {
        parse_index(&device.id)?;
        if device.label.len() > MAX_ID_BYTES
            || devices[..position]
                .iter()
                .any(|other| other.id == device.id)
        {
            bail!("Danh sách camera không hợp lệ.");
        }
    }
    Ok(())
}

fn parse_devices(bytes: &[u8]) -> Result<Vec<CameraDevice>> {
    if bytes.len() > MAX_LIST_BYTES {
        bail!("Danh sách camera vượt giới hạn 1 MiB.");
    }
    let devices: Vec<CameraDevice> = serde_json::from_slice(bytes).context(CAMERA_STOPPED)?;
    validate_devices(&devices)?;
    Ok(devices)
}

fn native_devices() -> Result<Vec<CameraDevice>> {
    // Enumeration does not call initialization: macOS permission is requested only on capture.
    let devices = nokhwa::query(nokhwa::utils::ApiBackend::Auto)
        .context("Không liệt kê được camera gốc trên hệ điều hành này.")?
        .into_iter()
        .map(|info| CameraDevice {
            id: index_key(info.index()),
            label: info.human_name(),
        })
        .collect::<Vec<_>>();
    validate_devices(&devices)?;
    Ok(devices)
}

fn nearest_format(formats: impl IntoIterator<Item = CameraFormat>) -> Result<CameraFormat> {
    formats
        .into_iter()
        .filter(|format| {
            let width = format.resolution().width();
            let height = format.resolution().height();
            let valid_chroma = match format.format() {
                FrameFormat::YUYV => width % 2 == 0,
                FrameFormat::NV12 => width % 2 == 0 && height % 2 == 0,
                _ => true,
            };
            RgbFormat::FORMATS.contains(&format.format())
                && valid_chroma
                && rgb_len(width, height).is_ok()
                && format.frame_rate() > 0
        })
        .min_by_key(|format| {
            let resolution = format.resolution();
            let distance = u64::from(resolution.width().abs_diff(640)).pow(2)
                + u64::from(resolution.height().abs_diff(480)).pow(2);
            (distance, format.frame_rate().abs_diff(15))
        })
        .ok_or_else(|| anyhow!("Camera không hỗ trợ định dạng RGB/YUYV/NV12/MJPEG trong giới hạn."))
}

fn decode_rgb(buffer: &nokhwa::Buffer) -> Result<RawFrame> {
    let resolution = buffer.resolution();
    let width = resolution.width();
    let height = resolution.height();
    let expected = rgb_len(width, height)?;
    let pixels = expected / 3;
    let data = buffer.buffer();
    if data.len() > MAX_WIDTH as usize * MAX_HEIGHT as usize * 4 {
        bail!("Dữ liệu camera vượt giới hạn.");
    }
    let rgb = if buffer.source_frame_format() == FrameFormat::MJPEG {
        // JPEG carries its own dimensions; enforce those before allocating decoded pixels.
        let mut reader =
            image::ImageReader::with_format(std::io::Cursor::new(data), image::ImageFormat::Jpeg);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(width);
        limits.max_image_height = Some(height);
        limits.max_alloc = Some(expected as u64 * 4);
        reader.limits(limits);
        let image = reader.decode().context(CAMERA_STOPPED)?;
        if image.width() != width || image.height() != height {
            bail!("Kích thước JPEG của camera không hợp lệ.");
        }
        image.into_rgb8().into_raw()
    } else {
        let input_len = match buffer.source_frame_format() {
            FrameFormat::YUYV if width % 2 == 0 => pixels * 2,
            FrameFormat::NV12 if width % 2 == 0 && height % 2 == 0 => pixels * 3 / 2,
            FrameFormat::RAWRGB | FrameFormat::RAWBGR => expected,
            FrameFormat::GRAY => pixels,
            _ => bail!("Định dạng hình ảnh camera không hợp lệ."),
        };
        if data.len() != input_len {
            bail!("Độ dài dữ liệu camera không hợp lệ.");
        }
        let mut rgb = vec![0; expected];
        if buffer.source_frame_format() == FrameFormat::RAWBGR {
            // nokhwa-core 0.1.9 incorrectly requires even dimensions for packed BGR888.
            // BGR has no chroma subsampling: every validated three-byte pixel stands alone.
            for (input, output) in data.chunks_exact(3).zip(rgb.chunks_exact_mut(3)) {
                output.copy_from_slice(&[input[2], input[1], input[0]]);
            }
        } else {
            buffer
                .decode_image_to_buffer::<RgbFormat>(&mut rgb)
                .context(CAMERA_STOPPED)?;
        }
        rgb
    };
    if rgb.len() != expected {
        bail!("Độ dài hình ảnh camera không hợp lệ.");
    }
    Ok(RawFrame { width, height, rgb })
}

fn capture_native(device_id: &str) -> Result<()> {
    let index = parse_index(device_id)?;
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = mpsc::sync_channel(1);
        nokhwa::nokhwa_initialize(move |granted| {
            // The helper can be killed while a system permission dialog is still open.
            let _ = tx.send(granted);
        });
        if !rx.recv_timeout(Duration::from_secs(120)).unwrap_or(false) {
            bail!("Chưa được cấp quyền truy cập camera.");
        }
    }
    if !native_devices()?
        .iter()
        .any(|device| device.id == device_id)
    {
        bail!("Camera đã chọn không còn khả dụng.");
    }
    let initial = RequestedFormat::new::<RgbFormat>(RequestedFormatType::None);
    let mut camera = Camera::new(index, initial).context(CAMERA_STOPPED)?;
    let format = nearest_format(camera.compatible_camera_formats().context(CAMERA_STOPPED)?)?;
    camera
        .set_camera_requset(RequestedFormat::new::<RgbFormat>(
            RequestedFormatType::Exact(format),
        ))
        .context(CAMERA_STOPPED)?;
    camera.open_stream().context(CAMERA_STOPPED)?;
    let mut stdout = std::io::stdout().lock();
    let cadence = Duration::from_millis(1000 / 15);
    loop {
        let started = Instant::now();
        let buffer = camera.frame().context(CAMERA_STOPPED)?;
        write_frame(&mut stdout, &decode_rgb(&buffer)?)?;
        if let Some(delay) = cadence.checked_sub(started.elapsed()) {
            thread::sleep(delay);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nokhwa::utils::Resolution;

    fn frame_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        write_frame(
            &mut bytes,
            &RawFrame {
                width: 2,
                height: 1,
                rgb: vec![1, 2, 3, 4, 5, 6],
            },
        )
        .unwrap();
        bytes
    }

    #[test]
    fn reads_rgb_frame_when_header_is_valid() {
        let frame = read_frame(&mut frame_bytes().as_slice()).unwrap();
        assert_eq!((frame.width, frame.height), (2, 1));
        assert_eq!(frame.rgb, [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn rejects_frame_when_header_or_payload_is_invalid() {
        for len in [0, 7, 19, 20, 25] {
            assert!(read_frame(&mut &frame_bytes()[..len]).is_err());
        }
        for (offset, value) in [(0, 0), (7, b'2'), (8, 0), (16, 255)] {
            let mut bytes = frame_bytes();
            bytes[offset] = value;
            assert!(read_frame(&mut bytes.as_slice()).is_err());
        }
    }

    #[test]
    fn rejects_oversized_dimensions_before_reading_payload() {
        let mut bytes = frame_bytes();
        bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_frame(&mut bytes.as_slice()).is_err());
        assert!(rgb_len(MAX_WIDTH, MAX_HEIGHT).is_ok());
        assert!(rgb_len(MAX_WIDTH + 1, 1).is_err());
        assert!(rgb_len(1, MAX_HEIGHT + 1).is_err());
    }

    #[test]
    fn coalesces_frames_when_consumer_is_slow() {
        let mut bytes = frame_bytes();
        let mut second = frame_bytes();
        *second.last_mut().unwrap() = 99;
        bytes.extend(second);
        let mailbox: Mailbox = Arc::default();
        read_frames(bytes.as_slice(), Arc::clone(&mailbox));
        let mut slot = mailbox.0.lock().unwrap();
        assert_eq!(slot.latest.take().unwrap().rgb.last(), Some(&99));
        assert!(slot.error.is_some());
    }

    #[test]
    fn resolves_exact_ids_when_names_are_duplicated() {
        let devices = vec![
            CameraDevice {
                id: "nokhwa:index:0".into(),
                label: "USB Camera".into(),
            },
            CameraDevice {
                id: "nokhwa:index:1".into(),
                label: "USB Camera".into(),
            },
        ];
        assert_eq!(
            resolve_device(&devices, "nokhwa:index:1").unwrap(),
            "nokhwa:index:1"
        );
        assert_eq!(resolve_device(&devices, "").unwrap(), "nokhwa:index:0");
        assert!(resolve_device(&devices, "USB Camera").is_err());
        assert!(resolve_device(&devices, "missing").is_err());
        assert!(resolve_device(&[], "").is_err());
    }

    #[test]
    fn preserves_string_native_indices_without_shell_parsing() {
        let index = CameraIndex::String("/dev/video2; $camera with spaces".into());
        assert_eq!(parse_index(&index_key(&index)).unwrap(), index);
        assert!(parse_index("camera-name").is_err());
        assert!(parse_index("nokhwa:index:-1").is_err());
        assert!(parse_index("nokhwa:string:\0").is_err());
    }

    #[test]
    fn bounds_device_json_and_count() {
        assert!(parse_devices(&vec![b' '; MAX_LIST_BYTES + 1]).is_err());
        let devices = (0..=MAX_DEVICES)
            .map(|index| CameraDevice {
                id: format!("nokhwa:index:{index}"),
                label: "Camera".into(),
            })
            .collect::<Vec<_>>();
        assert!(parse_devices(&serde_json::to_vec(&devices).unwrap()).is_err());
        let duplicate = vec![devices[0].clone(), devices[0].clone()];
        assert!(parse_devices(&serde_json::to_vec(&duplicate).unwrap()).is_err());
    }

    #[test]
    fn negotiates_nearest_supported_native_format() {
        let formats = [
            CameraFormat::new_from(4000, 3000, FrameFormat::MJPEG, 30),
            CameraFormat::new_from(1280, 720, FrameFormat::NV12, 30),
            CameraFormat::new_from(640, 480, FrameFormat::YUYV, 30),
            CameraFormat::new_from(640, 480, FrameFormat::MJPEG, 15),
        ];
        assert_eq!(nearest_format(formats).unwrap(), formats[3]);
        assert!(nearest_format([formats[0]]).is_err());
    }

    #[test]
    fn selects_usable_color_mode_when_gray_has_closer_frame_rate() {
        let gray = CameraFormat::new_from(640, 480, FrameFormat::GRAY, 15);
        let yuyv = CameraFormat::new_from(640, 480, FrameFormat::YUYV, 30);
        assert_eq!(nearest_format([gray, yuyv]).unwrap(), yuyv);
        assert!(nearest_format([gray]).is_err());
    }

    #[test]
    fn excludes_subsampled_modes_when_dimensions_cannot_be_decoded() {
        let odd_yuyv = CameraFormat::new_from(639, 480, FrameFormat::YUYV, 15);
        let odd_nv12_width = CameraFormat::new_from(639, 480, FrameFormat::NV12, 15);
        let odd_nv12_height = CameraFormat::new_from(640, 479, FrameFormat::NV12, 15);
        let bgr = CameraFormat::new_from(639, 479, FrameFormat::RAWBGR, 30);
        assert!(nearest_format([odd_yuyv]).is_err());
        assert!(nearest_format([odd_nv12_width]).is_err());
        assert!(nearest_format([odd_nv12_height]).is_err());
        assert_eq!(
            nearest_format([odd_yuyv, odd_nv12_height, bgr]).unwrap(),
            bgr
        );
        let odd_height_yuyv = CameraFormat::new_from(640, 479, FrameFormat::YUYV, 15);
        assert_eq!(nearest_format([odd_height_yuyv]).unwrap(), odd_height_yuyv);
    }

    #[test]
    fn decodes_native_rgb_yuyv_and_nv12_without_external_tools() {
        let raw = nokhwa::Buffer::new(
            Resolution::new(2, 1),
            &[1, 2, 3, 4, 5, 6],
            FrameFormat::RAWRGB,
        );
        assert_eq!(decode_rgb(&raw).unwrap().rgb, [1, 2, 3, 4, 5, 6]);
        let bgr = nokhwa::Buffer::new(
            Resolution::new(2, 1),
            &[3, 2, 1, 6, 5, 4],
            FrameFormat::RAWBGR,
        );
        assert_eq!(decode_rgb(&bgr).unwrap().rgb, [1, 2, 3, 4, 5, 6]);
        let yuyv = nokhwa::Buffer::new(
            Resolution::new(2, 1),
            &[16, 128, 16, 128],
            FrameFormat::YUYV,
        );
        assert_eq!(decode_rgb(&yuyv).unwrap().rgb.len(), 6);
        let nv12 = nokhwa::Buffer::new(
            Resolution::new(2, 2),
            &[16, 16, 16, 16, 128, 128],
            FrameFormat::NV12,
        );
        assert_eq!(decode_rgb(&nv12).unwrap().rgb.len(), 12);
        let truncated = nokhwa::Buffer::new(Resolution::new(2, 2), &[16], FrameFormat::NV12);
        assert!(decode_rgb(&truncated).is_err());
    }

    #[test]
    fn decodes_jpeg_only_when_embedded_dimensions_match_native_frame() {
        let image = image::RgbImage::from_raw(2, 2, vec![128; 12]).unwrap();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode_image(&image)
            .unwrap();
        let matching = nokhwa::Buffer::new(Resolution::new(2, 2), &jpeg, FrameFormat::MJPEG);
        assert_eq!(decode_rgb(&matching).unwrap().rgb.len(), 12);
        let mismatched = nokhwa::Buffer::new(Resolution::new(1, 1), &jpeg, FrameFormat::MJPEG);
        assert!(decode_rgb(&mismatched).is_err());
    }

    #[test]
    fn leaves_normal_startup_untouched() {
        assert!(!run_helper_if_requested(&["app".into(), "--gui".into()]).unwrap());
        assert!(
            run_helper_if_requested(&["app".into(), LIST_FLAG.into(), "extra".into()]).is_err()
        );
        assert!(run_helper_if_requested(&["app".into(), CAPTURE_FLAG.into()]).is_err());
    }

    #[test]
    fn stops_and_reaps_blocked_helper_within_two_seconds() {
        const CHILD_ENV: &str = "CTDO_CAMERA_LIFECYCLE_TEST_CHILD";
        if std::env::var_os(CHILD_ENV).is_some() {
            // The test harness is a controlled helper with no driver or credential access.
            thread::sleep(Duration::from_secs(60));
            return;
        }
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "video::camera::tests::stops_and_reaps_blocked_helper_within_two_seconds",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let reader = thread::spawn(move || {
            started_tx.send(()).unwrap();
            let mut output = Vec::new();
            // A blocked owned pipe read must reach EOF when its sole writer is killed.
            stdout.take(1024).read_to_end(&mut output).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut process = CameraProcess {
            helper: HelperChild {
                child,
                reaped: false,
            },
            reader: Some(reader),
            mailbox: Arc::default(),
            stopped: false,
        };
        let started = Instant::now();
        process.stop().unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(process.helper.reaped);
        assert!(process.reader.is_none());
        process.stop().unwrap();
    }
}
