//! Audio I/O for VoIP calls.
//!
//! Phase 1 (this file): device enumeration + mic->speaker pass-through on a
//! dedicated thread (Send-safe handle).
//! Phase 2 (TODO): Opus encode/decode + RTP packetization over the call's UDP socket.
//! Phase 3 (TODO): jitter buffer, echo cancellation, mixing, device selection.

use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};

/// Send-safe handle to a running audio session. Dropping it stops the thread.
pub struct AudioHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioHandle {
    /// Start mic->speaker pass-through on a dedicated thread.
    pub fn start() -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let thread = thread::spawn(move || {
            if let Err(e) = run_pass_through(stop_thread.clone()) {
                eprintln!("audio thread error: {e}");
            }
        });
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for AudioHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
/// Kết quả một lần thu tin nhắn thoại: file audio đã encode + thời lượng (giây).
/// `dir` (khi có) là thư mục tạm owner-only chứa `path`; caller nên gọi
/// [`VoiceRecord::cleanup`] sau khi upload xong (cả khi gửi thất bại).
pub struct VoiceRecord {
    /// Đường dẫn file audio tạm trong thư mục riêng (xem [`voice_temp_dir`]).
    pub path: std::path::PathBuf,
    /// MIME để upload (`audio/aac` cho AAC, `audio/mpeg` cho MP3 fallback).
    pub mime: String,
    /// Thời lượng thu, làm tròn lên giây.
    pub duration_secs: u64,
    /// Số mẫu PCM 16 kHz đã thu (để test/unit tính duration).
    pub samples: u64,
    /// Thư mục tạm owner-only chứa `path` (xoá cùng file khi cleanup).
    pub dir: Option<std::path::PathBuf>,
}

impl VoiceRecord {
    /// Xoá file audio tạm và thư mục chứa nó (nếu còn). Không bao giờ panic.
    pub fn cleanup(&self) {
        let _ = std::fs::remove_file(&self.path);
        if let Some(d) = self.dir.as_ref() {
            let _ = std::fs::remove_dir(d);
        }
    }
}

/// Tạo thư mục tạm owner-only `ctdozlite-voice-<pid>-<tag>[-n]/` dưới `temp_dir()`.
///
/// `create_dir` là atomic (fail nếu đã tồn tại) nên chống TOCTOU; từ chối
/// symlink gài sẵn; trên Unix đặt mode `0o700` ngay sau khi tạo để umask
/// thoáng cũng không làm lộ file voice trong `/tmp` chia sẻ.
pub(crate) fn voice_temp_dir(tag: i64) -> Result<std::path::PathBuf> {
    let base = std::env::temp_dir();
    for attempt in 0..17u32 {
        let suffix = if attempt == 0 {
            format!("{tag}")
        } else {
            format!("{tag}-{attempt}")
        };
        let dir = base.join(format!(
            "ctdozlite-voice-{}-{suffix}",
            std::process::id()
        ));
        // Từ chối symlink có sẵn (kẻ khác gài sẵn link trỏ ra ngoài).
        if let Ok(meta) = std::fs::symlink_metadata(&dir) {
            if meta.file_type().is_symlink() {
                anyhow::bail!("Thư mục tạm voice không an toàn (symlink)");
            }
            continue; // tên đã có thật → thử suffix khác
        }
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
                }
                // Kiểm tra lại sau khi tạo: phải là dir thật, không symlink.
                let meta = std::fs::symlink_metadata(&dir)
                    .map_err(|e| anyhow::anyhow!("Không stat được thư mục tạm voice ({e})"))?;
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    let _ = std::fs::remove_dir(&dir);
                    anyhow::bail!("Thư mục tạm voice không an toàn");
                }
                return Ok(dir);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => anyhow::bail!("Không tạo được thư mục tạm voice ({e})"),
        }
    }
    anyhow::bail!("Không tạo được thư mục tạm voice (trùng tên)")
}

/// Thu mic thành file MP3 (Zalo voice: `voicemp3`/`sendmp3` nhận `audio/mpeg`).
///
/// Luồng thu tái dùng pattern `media.rs`: cpal default input → downmix mono →
/// resample về 16 kHz → cập nhật `MIC_LEVEL_PERMILLE` cho meter. Encode MP3 bằng
/// `rusty_mp3` (pure-Rust, không thêm C-FFI); không dùng `ffmpeg` ngoài để khỏi
/// phụ thuộc máy user.
/// Đích resample của voice: mọi mic đều về mono 16 kHz trước khi encode.
const VOICE_TARGET_RATE: u32 = 16_000;
/// Giữ tối đa 10 phút thu (16 kHz mono).
const VOICE_MAX_SAMPLES: usize = 16000 * 600;

/// Resampler tuyến tính có nhớ phase, dùng chung cho callback mic (gọi nhiều
/// lần, mỗi lần một buffer) và cho test thuần.
struct RecResampler {
    /// Số frame input cho mỗi mẫu 16 kHz (`in_rate/16000`; < 1 khi upsample).
    ratio: f64,
    /// Frame mono input chưa tiêu thụ (đuôi carried qua callbacks).
    buf: Vec<f32>,
    /// Vị trí nguồn kế tiếp (tính theo `buf`), giữ phần lẻ qua callbacks.
    src: f64,
}

impl RecResampler {
    fn new(in_rate: u32) -> Self {
        Self {
            ratio: in_rate.max(1) as f64 / VOICE_TARGET_RATE as f64,
            buf: Vec::new(),
            src: 0.0,
        }
    }

    /// Đẩy thêm frame mono input, trả các mẫu 16 kHz hoàn chỉnh.
    fn push_frames(&mut self, frames: &[f32]) -> Vec<f32> {
        self.buf.extend_from_slice(frames);
        let len = self.buf.len();
        let mut out = Vec::with_capacity((frames.len() as f64 / self.ratio).ceil() as usize + 1);
        while self.src + 1.0 < len as f64 {
            let i0 = self.src as usize;
            let frac = (self.src - i0 as f64) as f32;
            let a = self.buf[i0];
            out.push(a + (self.buf[i0 + 1] - a) * frac);
            self.src += self.ratio;
        }
        let consumed = (self.src as usize).min(self.buf.len());
        self.buf.drain(..consumed);
        self.src -= consumed as f64;
        out
    }

    /// Xả đuôi còn lại khi dừng thu (không mất mẫu cuối).
    fn flush(&mut self) -> Vec<f32> {
        let mut out = Vec::new();
        let len = self.buf.len();
        while self.src < len as f64 {
            out.push(self.buf[(self.src as usize).min(len.saturating_sub(1))]);
            self.src += self.ratio;
        }
        self.buf.clear();
        self.src = 0.0;
        out
    }
}

/// Hàm thuần để test: mono `input` ở `in_rate` → mono 16 kHz.
#[cfg(test)]
pub(crate) fn resample_mono_to_16k(input: &[f32], in_rate: u32) -> Vec<f32> {
    let mut r = RecResampler::new(in_rate);
    let mut out = r.push_frames(input);
    out.extend(r.flush());
    out
}

/// State chia sẻ giữa `VoiceRecorder` và callback mic (`'static`).
#[derive(Clone)]
struct RecIngest {
    pcm: Arc<Mutex<Vec<f32>>>,
    resamp: Arc<Mutex<RecResampler>>,
    stop: Arc<AtomicBool>,
    last_peak: Arc<Mutex<f32>>,
}

/// Downmix interleaved `data` (mỗi frame `channels` mẫu) về mono, resample về
/// 16 kHz, append vào `pcm`, cập nhật meter. `sample` convert 1 mẫu về f32.
fn ingest_interleaved<S: Copy>(
    st: &RecIngest,
    data: &[S],
    channels: usize,
    sample: fn(S) -> f32,
) {
    if st.stop.load(Ordering::Relaxed) {
        return;
    }
    let ch = channels.max(1);
    let total_frames = data.len() / ch;
    if total_frames == 0 {
        return;
    }
    // Downmix về mono trước để resampler chỉ làm việc trên 1 kênh.
    let mut mono = Vec::with_capacity(total_frames);
    for f in 0..total_frames {
        let mut sum = 0.0f32;
        for c in 0..ch {
            sum += sample(data[f * ch + c]);
        }
        mono.push(sum / ch as f32);
    }
    let mut peak = 0.0f32;
    if let (Ok(mut resamp), Ok(mut pcm)) = (st.resamp.lock(), st.pcm.lock()) {
        for v in resamp.push_frames(&mono) {
            let a = v.abs();
            if a > peak {
                peak = a;
            }
            pcm.push(v);
        }
        if pcm.len() > VOICE_MAX_SAMPLES {
            let drop_n = pcm.len() - VOICE_MAX_SAMPLES;
            pcm.drain(..drop_n);
        }
    }
    crate::media::set_mic_level_from_peak(peak);
    if let Ok(mut g) = st.last_peak.lock() {
        *g = peak;
    }
}

/// Xóa meter của recorder, nhưng không đè level của call đang active: chỉ xóa
/// khi mức hiện tại đúng là mức recorder đã đặt (so sánh sau lượng tử hóa
/// permille — call active ghi giá trị khác sẽ không khớp nên được giữ nguyên).
fn clear_recorder_meter(last_peak: &Arc<Mutex<f32>>) {
    let ours = last_peak
        .lock()
        .map(|g| (g.clamp(0.0, 1.0) * 1000.0) as u32)
        .unwrap_or(u32::MAX);
    if crate::media::mic_level_permille() == ours {
        crate::media::set_mic_level_from_peak(0.0);
    }
}
pub struct VoiceRecorder {
    stop: Arc<AtomicBool>,
    // Giữ `Stream` trực tiếp: cpal::Stream không Send nên không spawn thread giữ
    // được — callback mic sống chừng nào struct còn sống (GUI giữ tới lúc gửi/huỷ).
    _stream: Option<cpal::Stream>,
    pcm: Arc<Mutex<Vec<f32>>>,
    resamp: Arc<Mutex<RecResampler>>,
    last_peak: Arc<Mutex<f32>>,
    error: Arc<Mutex<Option<String>>>,
    started_at: std::time::Instant,
}

impl Drop for VoiceRecorder {
    /// Mọi đường dừng (cancel / stop_and_encode Ok+Err / rớt giữa chừng) đều
    /// tắt callback và xóa meter của recorder (không đè call đang active).
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        clear_recorder_meter(&self.last_peak);
    }
}

impl VoiceRecorder {
    /// Mở mic mặc định và bắt đầu thu. Lỗi (không mic / không quyền macOS /
    /// thiết bị bận) trả `Err` message thân thiện để GUI toast — không panic.
    pub fn start() -> Result<Self> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let input_dev = host.default_input_device().ok_or_else(|| {
            anyhow::anyhow!("Không tìm thấy micro trên máy. Hãy cắm micro rồi thử lại.")
        })?;
        let in_cfg = input_dev
            .default_input_config()
            .map_err(|e| mic_friendly_error("Không mở được micro", &e.to_string()))?;
        let in_rate = in_cfg.sample_rate().0.max(1);
        let in_channels = (in_cfg.channels() as usize).max(1);
        let sample_format = in_cfg.sample_format();
        let in_cfg_std: cpal::StreamConfig = in_cfg.into();
        let pcm: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(16000 * 8)));
        let resamp: Arc<Mutex<RecResampler>> =
            Arc::new(Mutex::new(RecResampler::new(in_rate)));
        let last_peak: Arc<Mutex<f32>> = Arc::new(Mutex::new(0.0));
        let stop = Arc::new(AtomicBool::new(false));
        let st = RecIngest {
            pcm: pcm.clone(),
            resamp: resamp.clone(),
            stop: stop.clone(),
            last_peak: last_peak.clone(),
        };
        let error = Arc::new(Mutex::new(None::<String>));
        // Mỗi nhánh match cần err-callback riêng (closure không Copy).
        let mk_err = |err_cb: Arc<Mutex<Option<String>>>| {
            move |e: cpal::StreamError| {
                if let Ok(mut g) = err_cb.lock() {
                    if g.is_none() {
                        *g = Some(
                            mic_friendly_error("Lỗi micro khi đang thu", &e.to_string()).to_string(),
                        );
                    }
                }
            }
        };
        // Build theo đúng sample format thiết bị quảng cáo (mic chỉ nhận
        // integer sẽ fail nếu ép f32), convert về f32 rồi xử lý chung.
        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                let st = st.clone();
                input_dev.build_input_stream(
                    &in_cfg_std,
                    move |data: &[f32], _| ingest_interleaved(&st, data, in_channels, |v| v),
                    mk_err(error.clone()),
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let st = st.clone();
                input_dev.build_input_stream(
                    &in_cfg_std,
                    move |data: &[i16], _| {
                        ingest_interleaved(&st, data, in_channels, |v| v as f32 / 32768.0)
                    },
                    mk_err(error.clone()),
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let st = st.clone();
                input_dev.build_input_stream(
                    &in_cfg_std,
                    move |data: &[u16], _| {
                        ingest_interleaved(&st, data, in_channels, |v| {
                            (v as f32 - 32768.0) / 32768.0
                        })
                    },
                    mk_err(error.clone()),
                    None,
                )
            }
            fmt => {
                return Err(mic_friendly_error(
                    "Không mở được micro để thu",
                    &format!("định dạng mẫu không hỗ trợ ({fmt:?})"),
                ));
            }
        }
        .map_err(|e| mic_friendly_error("Không mở được micro để thu", &e.to_string()))?;
        stream.play().map_err(|e| {
            anyhow::anyhow!("Không bắt đầu thu được ({e}). Kiểm tra quyền Micro trong System Settings → Privacy & Security → Microphone.")
        })?;
        Ok(Self {
            stop,
            _stream: Some(stream),
            pcm,
            resamp,
            last_peak,
            error,
            started_at: std::time::Instant::now(),
        })
    }

    /// Số mẫu 16 kHz đã thu (để render timer).
    pub fn samples(&self) -> u64 {
        self.pcm.lock().map(|b| b.len() as u64).unwrap_or(0)
    }

    /// Thời gian đã thu (để render timer).
    pub fn elapsed(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }

    /// Lỗi bất đồng bộ từ mic callback (nếu có).
    pub fn take_error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|mut g| g.take())
    }

    /// Hủy thu, bỏ dữ liệu (nút X). `Drop` xóa meter của recorder.
    pub fn cancel(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        drop(self._stream.take());
    }

    /// Dừng thu, encode PCM → AAC (ffmpeg) hoặc MP3 fallback, ghi vào thư
    /// mục tạm owner-only. Trả `Err` thân thiện khi chưa thu được mẫu nào.
    /// Caller (worker) MUST gọi [`VoiceRecord::cleanup`] sau upload, cả khi lỗi.
    /// `Drop` xóa meter của recorder trên mọi đường về (Ok lẫn Err).
    pub fn stop_and_encode(mut self) -> Result<VoiceRecord> {
        self.stop.store(true, Ordering::Relaxed);
        // Drop stream: callback mic dừng ngay, không cần join thread.
        drop(self._stream.take());
        // Xả đuôi resampler (không mất mẫu cuối dở dang giữa callback).
        if let (Ok(mut resamp), Ok(mut pcm)) = (self.resamp.lock(), self.pcm.lock()) {
            pcm.extend(resamp.flush());
        }
        let samples_vec: Vec<f32> = self.pcm.lock().map(|b| b.clone()).unwrap_or_default();
        if samples_vec.is_empty() {
            anyhow::bail!("Chưa thu được âm thanh nào — hãy giữ micro và nói to hơn.");
        }
        let duration_secs = (samples_vec.len() as u64).div_ceil(16000).max(1);
        let tag = crate::login::unix_now();
        let samples_n = samples_vec.len() as u64;
        // Ưu tiên AAC/M4A đúng chuẩn Zalo; không có ffmpeg thì MP3 fallback.
        // Encode vào thư mục riêng owner-only; M4A fail giữa chừng → dọn dir.
        match encode_mono_aac_16k(&samples_vec, tag) {
            Ok((path, dir)) => {
                return Ok(VoiceRecord {
                    path,
                    mime: "audio/aac".into(),
                    duration_secs,
                    samples: samples_n,
                    dir: Some(dir),
                });
            }
            Err(_) => {
                // fallback MP3 bên dưới (dir của lần M4A fail đã tự dọn).
            }
        }
        let dir = voice_temp_dir(tag)?;
        let mp3_bytes = encode_mono_mp3_16k(&samples_vec).inspect_err(|_| {
            let _ = std::fs::remove_dir(&dir);
        })?;
        let path = dir.join(format!("ctdozlite_voice_{tag}.mp3"));
        if let Err(e) = std::fs::write(&path, &mp3_bytes) {
            let _ = std::fs::remove_dir(&dir);
            anyhow::bail!("Không ghi được file thoại tạm ({e})");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(VoiceRecord {
            path,
            mime: "audio/mpeg".into(),
            duration_secs,
            samples: samples_n,
            dir: Some(dir),
        })
    }
}

/// Message lỗi mic thân thiện, đặc biệt khi macOS từ chối quyền (không crash).
fn mic_friendly_error(ctx: &str, raw: &str) -> anyhow::Error {
    if raw.contains("Permission") || raw.contains("permission") || raw.contains("NotAllowed") {
        anyhow::anyhow!(
            "{ctx}: Zalo Lite chưa được cấp quyền Micro. Mở System Settings → Privacy & Security → Microphone và bật cho ứng dụng, rồi thử lại."
        )
    } else {
        anyhow::anyhow!("{ctx} ({raw}). Kiểm tra micro có bị app khác giữ không.")
    }
}

/// Encode PCM mono 16 kHz float `[-1, 1]` → MP3 bytes (pure-Rust `rusty_mp3`,
/// CBR 64 kbps mono — đủ cho voice, file nhẹ).
pub fn encode_mono_mp3_16k(pcm_mono_16k: &[f32]) -> Result<Vec<u8>> {
    if pcm_mono_16k.is_empty() {
        anyhow::bail!("Không có mẫu âm thanh để encode");
    }
    let mut enc = rusty_mp3::Mp3Encoder::new(rusty_mp3::Mp3EncoderConfig {
        bitrate_kbps: 64,
        vbr_quality: None,
    });
    enc.push_pcm_f32(pcm_mono_16k, 1, 16000)
        .map_err(|e| anyhow::anyhow!("encode MP3 thất bại ({e:?})"))?;
    enc.finish();
    let mut out = Vec::new();
    loop {
        match enc.next_packet() {
            Ok(pkt) => out.extend_from_slice(&pkt),
            Err(rusty_mp3::Error::Eof) => break,
            Err(rusty_mp3::Error::Again) => {
                anyhow::bail!("encode MP3 thất bại (Again sau finish)")
            }
            Err(e) => anyhow::bail!("encode MP3 thất bại ({e:?})"),
        }
    }
    if out.is_empty() {
        anyhow::bail!("encode MP3 cho ra 0 byte");
    }
    Ok(out)
}

/// Encode PCM mono 16 kHz float → file AAC (.aac ADTS) qua ffmpeg (stdin raw f32le).
/// Trả `(file, dir)`: file nằm trong thư mục tạm owner-only mới tạo.
/// Err khi không có ffmpeg hoặc ffmpeg fail → caller fallback MP3.
/// Luôn dọn dir khi fail (không để lại file mồ côi trong `/tmp` chia sẻ).
pub fn encode_mono_aac_16k(
    pcm_mono_16k: &[f32],
    tag: i64,
) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    use std::io::Write;
    if pcm_mono_16k.is_empty() {
        anyhow::bail!("Không có mẫu âm thanh để encode");
    }
    let dir = voice_temp_dir(tag)?;
    let path = dir.join(format!("ctdozlite_voice_{tag}.aac"));
    let spawn_r = std::process::Command::new("ffmpeg")
        .args([
            "-y", "-loglevel", "error",
            "-f", "f32le", "-ar", "16000", "-ac", "1", "-i", "pipe:0",
            "-c:a", "aac", "-b:a", "64k", "-f", "adts",
            path.to_string_lossy().as_ref(),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let mut child = match spawn_r {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_dir(&dir);
            anyhow::bail!("Không chạy được ffmpeg ({e})");
        }
    };
    let mut pcm_bytes = Vec::with_capacity(pcm_mono_16k.len() * 4);
    for s in pcm_mono_16k {
        pcm_bytes.extend_from_slice(&s.to_le_bytes());
    }
    let write_r = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("Không mở được stdin của ffmpeg"))
        .and_then(|mut stdin| {
            stdin
                .write_all(&pcm_bytes)
                .map_err(|e| anyhow::anyhow!("Ghi PCM vào ffmpeg thất bại ({e})"))
        });
    if let Err(e) = write_r {
        let _ = child.wait();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
        return Err(e);
    }
    let status = child.wait().map_err(|e| anyhow::anyhow!("ffmpeg lỗi ({e})"))?;
    if !status.success() || path.metadata().map(|m| m.len()).unwrap_or(0) == 0 {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
        anyhow::bail!("ffmpeg encode AAC thất bại");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok((path, dir))
}

fn run_pass_through(stop: Arc<AtomicBool>) -> Result<()> {
    let host = cpal::default_host();
    let input_dev = host
        .default_input_device()
        .ok_or_else(|| anyhow::anyhow!("no default input device"))?;
    let output_dev = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no default output device"))?;

    let in_cfg = input_dev.default_input_config()?;
    let out_cfg = output_dev.default_output_config()?;

    let channels = in_cfg.channels() as usize;

    let ring: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(48000)));
    let ring_in = ring.clone();
    let ring_out = ring.clone();

    let input_stream = input_dev.build_input_stream(
        &in_cfg.into(),
        move |data: &[f32], _| {
            if let Ok(mut buf) = ring_in.lock() {
                buf.extend_from_slice(data);
                if buf.len() > 48000 * channels {
                    let drop = buf.len() - 48000 * channels;
                    buf.drain(0..drop);
                }
            }
        },
        |e| eprintln!("input stream error: {e}"),
        None,
    )?;
    input_stream.play()?;

    let output_stream = output_dev.build_output_stream(
        &out_cfg.into(),
        move |data: &mut [f32], _| {
            if let Ok(mut buf) = ring_out.lock() {
                let n = data.len().min(buf.len());
                for (i, slot) in data.iter_mut().enumerate() {
                    *slot = if i < n { buf[i] } else { 0.0 };
                }
                if n > 0 {
                    buf.drain(0..n);
                }
            } else {
                for s in data.iter_mut() {
                    *s = 0.0;
                }
            }
        },
        |e| eprintln!("output stream error: {e}"),
        None,
    )?;
    output_stream.play()?;

    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    drop(input_stream);
    drop(output_stream);
    Ok(())
}

/// List available input (microphone) device names.
pub fn list_input_devices() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let mut out = Vec::new();
    for d in host.input_devices()? {
        if let Ok(name) = d.name() {
            out.push(name);
        }
    }
    Ok(out)
}

/// List available output (speaker) device names.
pub fn list_output_devices() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let mut out = Vec::new();
    for d in host.output_devices()? {
        if let Ok(name) = d.name() {
            out.push(name);
        }
    }
    Ok(out)
}

/// Open the default mic and speaker and run pass-through for `seconds` (smoke test).
pub fn run_loopback(seconds: u64) -> Result<()> {
    let session = AudioHandle::start()?;
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    drop(session);
    Ok(())
}

#[cfg(test)]
mod voice_tests {
    use super::*;

    #[test]
    fn encode_1s_silence_produces_mp3_bytes() {
        // 1 giây im lặng mono 16 kHz → MP3 hợp lệ, bắt đầu bằng frame sync.
        let pcm = vec![0.0f32; 16000];
        let mp3 = encode_mono_mp3_16k(&pcm).expect("encode ok");
        assert!(!mp3.is_empty());
        assert_eq!(mp3[0], 0xFF, "MP3 frame sync");
        assert!(mp3[1] & 0xE0 == 0xE0, "MP3 frame sync bits");
    }

    #[test]
    fn encode_rejects_empty_pcm() {
        assert!(encode_mono_mp3_16k(&[]).is_err());
    }

    fn sine_1s(in_rate: u32, freq: f32) -> Vec<f32> {
        (0..in_rate)
            .map(|n| (2.0 * std::f32::consts::PI * freq * n as f32 / in_rate as f32).sin())
            .collect()
    }

    /// 8 kHz → 16 kHz phải giữ đúng duration (gấp đôi số mẫu, ±1), không còn
    /// bug cũ phát nhanh gấp đôi.
    #[test]
    fn resample_8k_to_16k_preserves_duration() {
        let out = resample_mono_to_16k(&sine_1s(8000, 440.0), 8000);
        assert!(
            (out.len() as i64 - 16000).abs() <= 1,
            "8k 1s -> 16k mẫu, got {}",
            out.len()
        );
        // Sine 440 Hz còn nguyên hình (không phải noise/clamp sai): RMS > 0.5.
        let rms = (out.iter().map(|v| v * v).sum::<f32>() / out.len() as f32).sqrt();
        assert!(rms > 0.5, "sine méo sau upsample, rms={rms}");
    }

    /// 44.1 kHz → 16 kHz đúng số mẫu ±1.
    #[test]
    fn resample_441k_to_16k_frame_count() {
        let out = resample_mono_to_16k(&sine_1s(44100, 440.0), 44100);
        assert!(
            (out.len() as i64 - 16000).abs() <= 1,
            "44.1k 1s -> 16k mẫu, got {}",
            out.len()
        );
    }

    /// Phase accumulator qua callbacks: chia buffer thành từng chunk nhỏ phải
    /// cho cùng số mẫu (±1) như đẩy một lần — không mất ~0.57s/phút ở 44.1 kHz.
    #[test]
    fn resampler_phase_survives_callback_boundaries() {
        let input = sine_1s(44100, 440.0);
        let one_shot = resample_mono_to_16k(&input, 44100);
        let mut r = RecResampler::new(44100);
        let mut chunked = Vec::new();
        for chunk in input.chunks(128) {
            chunked.extend(r.push_frames(chunk));
        }
        chunked.extend(r.flush());
        assert!(
            (chunked.len() as i64 - one_shot.len() as i64).abs() <= 1,
            "chunked {} vs one-shot {}",
            chunked.len(),
            one_shot.len()
        );
        assert!(
            (chunked.len() as i64 - 16000).abs() <= 2,
            "44.1k 1s chunked -> 16k mẫu, got {}",
            chunked.len()
        );
    }
}