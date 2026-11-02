//! Video helpers for the in-app player (`gui/media_view.rs`): locating `ffmpeg`, probing a file,
//! building the decoder command lines and parsing their output.
//!
//! QTzalo never bundles a decoder. When an `ffmpeg` executable is installed it is run as a child
//! process (arguments passed as a vector, never through a shell) that decodes a cached file into
//! raw frames / PCM on stdout. Without it the GUI falls back to the system video player.
//!
//! Everything that does not spawn a process is a pure function with unit tests.

use std::ffi::OsString;
use std::io::{self, BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Decoded frames fit inside a `MAX_FRAME_DIM × MAX_FRAME_DIM` box (aspect ratio kept).
pub const MAX_FRAME_DIM: u32 = 1280;

/// Frames larger than this per side are rejected by [`read_ppm_frame`] (defence in depth: the
/// scale filter already caps them at [`MAX_FRAME_DIM`]).
pub const MAX_ACCEPTED_FRAME_DIM: u32 = 2048;

/// Upper bound for the playback frame rate (higher source rates are resampled down).
pub const MAX_PLAYBACK_FPS: f64 = 30.0;

/// Frame rate used when the source does not report one.
pub const DEFAULT_PLAYBACK_FPS: f64 = 25.0;

/// Probing a file never takes longer than this; a stuck `ffmpeg` is killed.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// `CREATE_NO_WINDOW`: no console window flashes when the GUI starts `ffmpeg`.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
const FFMPEG_EXE: &str = "ffmpeg.exe";
#[cfg(not(windows))]
const FFMPEG_EXE: &str = "ffmpeg";

// ---------------------------------------------------------------------------
// ffmpeg discovery
// ---------------------------------------------------------------------------

static FFMPEG: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Path of a usable `ffmpeg` executable, looked up once per process and cached.
///
/// Order: `QTZALO_FFMPEG` (explicit override) → every `PATH` entry → WinGet's
/// `%LOCALAPPDATA%\Microsoft\WinGet\Links` → a bounded search of WinGet's package folders
/// (`…\WinGet\Packages\*FFmpeg*\**\bin\ffmpeg.exe`) → common install folders.
/// Only real executables qualify: `.cmd` / `.bat` shims are never run.
pub fn find_ffmpeg() -> Option<&'static Path> {
    FFMPEG.get_or_init(discover_ffmpeg).as_deref()
}

fn discover_ffmpeg() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    let local_app_data = env("LOCALAPPDATA").map(PathBuf::from);
    let candidates = ffmpeg_candidates(
        env("QTZALO_FFMPEG"),
        env("PATH"),
        local_app_data.as_deref(),
        env("ProgramFiles").map(PathBuf::from).as_deref(),
    );
    if let Some(found) = candidates.into_iter().find(|p| is_usable_ffmpeg(p)) {
        return Some(found);
    }
    let packages = local_app_data?.join("Microsoft").join("WinGet").join("Packages");
    search_winget_packages(&packages)
}

/// Candidate executable paths in lookup order (no file-system access; see [`find_ffmpeg`]).
pub fn ffmpeg_candidates(
    override_path: Option<OsString>,
    path_var: Option<OsString>,
    local_app_data: Option<&Path>,
    program_files: Option<&Path>,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = override_path {
        out.push(PathBuf::from(p));
    }
    if let Some(path) = path_var {
        for dir in std::env::split_paths(&path) {
            if dir.as_os_str().is_empty() || !dir.is_absolute() {
                continue; // a relative PATH entry would resolve against the working directory
            }
            out.push(dir.join(FFMPEG_EXE));
        }
    }
    if let Some(lad) = local_app_data {
        out.push(lad.join("Microsoft").join("WinGet").join("Links").join(FFMPEG_EXE));
    }
    if cfg!(windows) {
        if let Some(pf) = program_files {
            out.push(pf.join("ffmpeg").join("bin").join(FFMPEG_EXE));
        }
        out.push(PathBuf::from(r"C:\ffmpeg\bin").join(FFMPEG_EXE));
    } else {
        out.push(PathBuf::from("/usr/bin/ffmpeg"));
        out.push(PathBuf::from("/usr/local/bin/ffmpeg"));
        out.push(PathBuf::from("/opt/homebrew/bin/ffmpeg"));
    }
    out
}

/// Is `p` a name we are willing to execute? On Windows only `*.exe` (never `.cmd` / `.bat` /
/// `.ps1` shims, which would run through a command interpreter).
pub fn is_acceptable_ffmpeg_name(p: &Path) -> bool {
    if !p.is_absolute() {
        return false;
    }
    if cfg!(windows) {
        p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    } else {
        p.file_name().is_some()
    }
}

fn is_usable_ffmpeg(p: &Path) -> bool {
    is_acceptable_ffmpeg_name(p) && p.is_file()
}

/// Bounded breadth-first search for `bin/ffmpeg.exe` under WinGet package folders whose name
/// mentions FFmpeg (`Gyan.FFmpeg_…`, `BtbN.FFmpeg_…`). At most 3 levels and 200 folders.
fn search_winget_packages(packages: &Path) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = std::fs::read_dir(packages)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|e| e.file_name().to_string_lossy().to_ascii_lowercase().contains("ffmpeg"))
        .map(|e| e.path())
        .collect();
    // Gyan's build first (the common `winget install ffmpeg` package), then by name.
    roots.sort_by_key(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase());
        (!name.as_deref().unwrap_or("").starts_with("gyan."), p.clone())
    });
    let mut budget = 200usize;
    for root in roots.into_iter().take(8) {
        let mut level = vec![root];
        for _depth in 0..=3 {
            let mut next = Vec::new();
            for dir in level {
                if budget == 0 {
                    return None;
                }
                budget -= 1;
                let candidate = dir.join("bin").join(FFMPEG_EXE);
                if is_usable_ffmpeg(&candidate) {
                    return Some(candidate);
                }
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    next.extend(
                        entries
                            .flatten()
                            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                            .map(|e| e.path()),
                    );
                }
            }
            level = next;
        }
    }
    None
}

/// A `Command` for `ffmpeg` with stdin closed and (on Windows) no console window.
pub fn ffmpeg_command(ffmpeg: &Path) -> Command {
    let mut cmd = Command::new(ffmpeg);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// `file:<path>`: the input is always read through the `file` protocol, so a crafted name can
/// never select another protocol or demuxer prefix.
pub fn file_input_arg(path: &Path) -> OsString {
    let mut s = OsString::from("file:");
    s.push(path.as_os_str());
    s
}

/// Demuxers allowed for a cached chat video: real media containers only, so a playlist /
/// `concat` script saved under a `.mp4` name is refused instead of opening other files.
pub const FORMAT_WHITELIST: &str = "mov,mp4,m4a,3gp,3g2,mj2,matroska,webm,avi,flv,mpegts";

/// Shared input options: no stdin, quiet, only the `file` protocol and media demuxers.
fn input_args(path: &Path, start_secs: f64, loglevel: &str) -> Vec<OsString> {
    let mut a: Vec<OsString> = ["-nostdin", "-hide_banner", "-loglevel", loglevel]
        .iter()
        .map(OsString::from)
        .collect();
    a.push("-protocol_whitelist".into());
    a.push("file".into());
    a.push("-format_whitelist".into());
    a.push(FORMAT_WHITELIST.into());
    if start_secs > 0.0 {
        a.push("-ss".into());
        a.push(format!("{start_secs:.3}").into());
    }
    a.push("-i".into());
    a.push(file_input_arg(path));
    a
}

/// `ffmpeg -i <file>` with no output: prints the container / stream summary on stderr.
pub fn probe_args(path: &Path) -> Vec<OsString> {
    input_args(path, 0.0, "info")
}

/// Decode the video stream from `start_secs` into PPM (`P6`, RGB24) frames on stdout, at a
/// constant `fps` (frame `i` is shown at `start_secs + i / fps`), scaled to fit
/// [`MAX_FRAME_DIM`]². Audio, subtitles and data streams are ignored.
pub fn video_decode_args(path: &Path, start_secs: f64, fps: f64) -> Vec<OsString> {
    let mut a = input_args(path, start_secs, "error");
    for s in ["-an", "-sn", "-dn"] {
        a.push(s.into());
    }
    a.push("-vf".into());
    a.push(
        format!(
            "fps={fps:.3},scale=w=min({m}\\,iw):h=min({m}\\,ih):force_original_aspect_ratio=decrease",
            m = MAX_FRAME_DIM
        )
        .into(),
    );
    for s in ["-f", "image2pipe", "-vcodec", "ppm", "pipe:1"] {
        a.push(s.into());
    }
    a
}

/// Decode the audio stream from `start_secs` into interleaved little-endian `f32` PCM at the
/// output device's `sample_rate` / `channels` on stdout.
pub fn audio_decode_args(path: &Path, start_secs: f64, sample_rate: u32, channels: u16) -> Vec<OsString> {
    let mut a = input_args(path, start_secs, "error");
    for s in ["-vn", "-sn", "-dn", "-ac"] {
        a.push(s.into());
    }
    a.push(channels.to_string().into());
    a.push("-ar".into());
    a.push(sample_rate.to_string().into());
    for s in ["-f", "f32le", "pipe:1"] {
        a.push(s.into());
    }
    a
}

/// Spawn `ffmpeg` with `args`, stdout piped (the decoded stream), stderr discarded.
pub fn spawn_decoder(ffmpeg: &Path, args: &[OsString]) -> io::Result<Child> {
    ffmpeg_command(ffmpeg)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
}

/// Run `ffmpeg -i <file>` and parse the summary. Killed after [`PROBE_TIMEOUT`].
pub fn probe(ffmpeg: &Path, path: &Path) -> Result<MediaInfo, String> {
    let mut child = ffmpeg_command(ffmpeg)
        .args(probe_args(path))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("không chạy được ffmpeg: {e}"))?;
    let mut stderr = child.stderr.take();
    // Read stderr on a helper thread so a chatty ffmpeg can never block on a full pipe.
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stderr.as_mut() {
            let _ = s.take(256 * 1024).read_to_end(&mut buf);
        }
        buf
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < PROBE_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(15));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("ffmpeg không phản hồi khi đọc video".into());
            }
        }
    }
    let text = String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned();
    let info = parse_ffmpeg_info(&text);
    if info.has_video || info.has_audio {
        Ok(info)
    } else {
        Err("tệp không có luồng video hay âm thanh đọc được".into())
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// What `ffmpeg -i` reports about a file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaInfo {
    pub duration: Option<f64>,
    /// Coded size of the first (non cover-art) video stream.
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub has_video: bool,
    pub has_audio: bool,
    /// Display rotation in degrees, normalised to `0..360`.
    pub rotation: u32,
}

impl MediaInfo {
    /// Size as shown on screen (`ffmpeg` auto-rotates, so 90° / 270° swap the axes).
    pub fn display_size(&self) -> Option<(u32, u32)> {
        let (w, h) = (self.width?, self.height?);
        Some(if self.rotation == 90 || self.rotation == 270 { (h, w) } else { (w, h) })
    }
}

/// Parse the stderr summary of `ffmpeg -i <file>`.
pub fn parse_ffmpeg_info(stderr: &str) -> MediaInfo {
    let mut info = MediaInfo::default();
    let mut fps_from_tbr = None;
    for line in stderr.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Duration:") {
            if info.duration.is_none() {
                let value = rest.split(',').next().unwrap_or("").trim();
                info.duration = parse_hms(value);
            }
            continue;
        }
        if t.starts_with("Stream #") {
            if let Some(idx) = t.find("Video:") {
                if t.contains("(attached pic)") {
                    continue; // cover art in an audio file
                }
                let first = !info.has_video;
                info.has_video = true;
                if first {
                    for part in t[idx + "Video:".len()..].split(',') {
                        let part = part.trim();
                        if info.width.is_none() {
                            if let Some((w, h)) = part.split_whitespace().next().and_then(parse_dims) {
                                info.width = Some(w);
                                info.height = Some(h);
                            }
                        }
                        if let Some(v) = part.strip_suffix(" fps") {
                            info.fps = parse_rate(v);
                        } else if let Some(v) = part.strip_suffix(" tbr") {
                            fps_from_tbr = parse_rate(v);
                        }
                    }
                }
            } else if t.contains("Audio:") {
                info.has_audio = true;
            }
            continue;
        }
        if let Some(pos) = t.find("rotation of ") {
            let v = t[pos + "rotation of ".len()..].split_whitespace().next().unwrap_or("");
            if let Ok(deg) = v.parse::<f64>() {
                info.rotation = normalize_rotation(deg);
            }
        } else if t.starts_with("rotate") && info.rotation == 0 {
            if let Some((_, v)) = t.split_once(':') {
                if let Ok(deg) = v.trim().parse::<f64>() {
                    info.rotation = normalize_rotation(deg);
                }
            }
        }
    }
    if info.fps.is_none() {
        info.fps = fps_from_tbr;
    }
    info
}

fn normalize_rotation(deg: f64) -> u32 {
    let r = (deg.round() as i64).rem_euclid(360);
    // Snap to the nearest quarter turn.
    (((r + 45) / 90 * 90) % 360) as u32
}

/// `"1920x1080"` → `(1920, 1080)`; rejects hex-looking tokens like `0x31637661`.
fn parse_dims(token: &str) -> Option<(u32, u32)> {
    let (w, h) = token.split_once('x')?;
    if w.is_empty() || h.is_empty() || !w.bytes().all(|b| b.is_ascii_digit()) || !h.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

/// `"29.97"`, `"25"`, `"1k"` → frames per second.
fn parse_rate(v: &str) -> Option<f64> {
    let v = v.trim();
    let (num, mul) = match v.strip_suffix('k') {
        Some(n) => (n, 1000.0),
        None => (v, 1.0),
    };
    let r = num.parse::<f64>().ok()? * mul;
    (r.is_finite() && r > 0.0).then_some(r)
}

/// `"00:01:23.45"` → `83.45` seconds. `"N/A"` → `None`.
pub fn parse_hms(s: &str) -> Option<f64> {
    let mut parts = s.trim().split(':');
    let h = parts.next()?.parse::<f64>().ok()?;
    let m = parts.next()?.parse::<f64>().ok()?;
    let sec = parts.next()?.parse::<f64>().ok()?;
    if parts.next().is_some() || !(0.0..60.0).contains(&m) || !(0.0..60.0).contains(&sec) || h < 0.0 {
        return None;
    }
    let total = h * 3600.0 + m * 60.0 + sec;
    total.is_finite().then_some(total)
}

/// Playback frame rate for a source rate: the source rate, capped at [`MAX_PLAYBACK_FPS`]
/// (and at least 1), or [`DEFAULT_PLAYBACK_FPS`] when unknown.
pub fn choose_playback_fps(source: Option<f64>) -> f64 {
    match source {
        Some(f) if f.is_finite() && f > 0.0 => f.clamp(1.0, MAX_PLAYBACK_FPS),
        _ => DEFAULT_PLAYBACK_FPS,
    }
}

/// Scale `(w, h)` down (never up) to fit inside `max_w × max_h`, keeping the aspect ratio.
pub fn fit_within(w: f32, h: f32, max_w: f32, max_h: f32) -> (f32, f32) {
    if w <= 0.0 || h <= 0.0 {
        return (max_w.max(0.0), max_h.max(0.0));
    }
    let s = (max_w / w).min(max_h / h).min(1.0);
    (w * s, h * s)
}

/// `65.4` → `"1:05"`, `3725.0` → `"1:02:05"`.
pub fn format_clock(secs: f64) -> String {
    let total = if secs.is_finite() && secs > 0.0 { secs.floor() as u64 } else { 0 };
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// One `P6` frame from an `image2pipe` / `ppm` stream: `(width, height, rgb24 bytes)`.
///
/// `Ok(None)` at a clean end of stream (no bytes before the next header). Headers with a
/// maxval other than 255, or a side larger than `max_dim`, are rejected.
pub fn read_ppm_frame<R: BufRead>(r: &mut R, max_dim: u32) -> io::Result<Option<(u32, u32, Vec<u8>)>> {
    let bad = |msg: &str| io::Error::new(io::ErrorKind::InvalidData, msg.to_string());
    let Some(magic) = read_token(r)? else {
        return Ok(None);
    };
    if magic != "P6" {
        return Err(bad("không phải khung PPM"));
    }
    let mut num = |what: &str| -> io::Result<u32> {
        read_token(r)?
            .and_then(|t| t.parse::<u32>().ok())
            .ok_or_else(|| bad(what))
    };
    let w = num("thiếu chiều rộng")?;
    let h = num("thiếu chiều cao")?;
    let maxval = num("thiếu maxval")?;
    if w == 0 || h == 0 || w > max_dim || h > max_dim || maxval != 255 {
        return Err(bad("kích thước khung không hợp lệ"));
    }
    // `read_token` consumed exactly one whitespace byte after maxval; pixel data follows.
    let mut data = vec![0u8; w as usize * h as usize * 3];
    r.read_exact(&mut data)?;
    Ok(Some((w, h, data)))
}

/// Next whitespace-delimited header token (skipping `#` comments). Consumes the single
/// whitespace byte that ends the token. `Ok(None)` on EOF before any token byte.
fn read_token<R: BufRead>(r: &mut R) -> io::Result<Option<String>> {
    let mut tok = Vec::new();
    let mut in_comment = false;
    loop {
        let mut byte = [0u8; 1];
        if r.read(&mut byte)? == 0 {
            if tok.is_empty() {
                return Ok(None);
            }
            break;
        }
        let b = byte[0];
        if in_comment {
            in_comment = b != b'\n';
            continue;
        }
        if b.is_ascii_whitespace() {
            if tok.is_empty() {
                continue;
            }
            break;
        }
        if b == b'#' && tok.is_empty() {
            in_comment = true;
            continue;
        }
        if tok.len() >= 16 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "PPM header token too long"));
        }
        tok.push(b);
    }
    Ok(Some(String::from_utf8_lossy(&tok).into_owned()))
}

/// Is `path` inside `dir` (after resolving `..` / links)? Used to only ever hand `ffmpeg`
/// files from the video cache.
pub fn is_inside_dir(path: &Path, dir: &Path) -> bool {
    match (path.canonicalize(), dir.canonicalize()) {
        (Ok(p), Ok(d)) => p.starts_with(&d) && p != d,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'file:C:\\cache\\videos\\abc.mp4':
  Metadata:
    major_brand     : isom
  Duration: 00:01:23.45, start: 0.000000, bitrate: 4999 kb/s
  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 1920x1080 [SAR 1:1 DAR 16:9], 4999 kb/s, 29.97 fps, 29.97 tbr, 90k tbn (default)
      Metadata:
        handler_name    : VideoHandler
      Side data:
        displaymatrix: rotation of -90.00 degrees
  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 48000 Hz, stereo, fltp, 128 kb/s (default)
At least one output file must be specified
";

    #[test]
    fn parses_full_summary() {
        let i = parse_ffmpeg_info(SAMPLE);
        assert!((i.duration.unwrap() - 83.45).abs() < 1e-9);
        assert_eq!((i.width, i.height), (Some(1920), Some(1080)));
        assert!((i.fps.unwrap() - 29.97).abs() < 1e-9);
        assert!(i.has_video && i.has_audio);
        assert_eq!(i.rotation, 270);
        assert_eq!(i.display_size(), Some((1080, 1920)));
    }

    #[test]
    fn parses_tbr_fallback_old_rotate_tag_and_missing_duration() {
        let s = "  Duration: N/A, bitrate: N/A\n\
                 \x20 Stream #0:0: Video: vp9, yuv420p, 640x360, 30 tbr, 1k tbn\n\
                 \x20     rotate          : 90\n";
        let i = parse_ffmpeg_info(s);
        assert_eq!(i.duration, None);
        assert_eq!((i.width, i.height), (Some(640), Some(360)));
        assert_eq!(i.fps, Some(30.0));
        assert!(!i.has_audio);
        assert_eq!(i.rotation, 90);
        assert_eq!(i.display_size(), Some((360, 640)));
    }

    #[test]
    fn skips_cover_art_and_hex_tokens() {
        let s = "  Stream #0:0: Audio: mp3, 44100 Hz, stereo, fltp, 320 kb/s\n\
                 \x20 Stream #0:1: Video: mjpeg (Baseline), yuvj420p(pc), 500x500 [SAR 1:1 DAR 1:1], 90k tbr (attached pic)\n";
        let i = parse_ffmpeg_info(s);
        assert!(i.has_audio);
        assert!(!i.has_video);
        assert_eq!(i.width, None);
        assert_eq!(parse_dims("0x31637661"), None);
        assert_eq!(parse_dims("0x31637661)"), None);
        assert_eq!(parse_dims("1280x720"), Some((1280, 720)));
        assert_eq!(parse_dims("x720"), None);
    }

    #[test]
    fn hms_and_clock() {
        assert_eq!(parse_hms("00:00:05.00"), Some(5.0));
        assert_eq!(parse_hms("01:02:03.5"), Some(3723.5));
        assert_eq!(parse_hms("N/A"), None);
        assert_eq!(parse_hms("00:61:00"), None);
        assert_eq!(format_clock(0.0), "0:00");
        assert_eq!(format_clock(65.9), "1:05");
        assert_eq!(format_clock(3725.0), "1:02:05");
        assert_eq!(format_clock(f64::NAN), "0:00");
        assert_eq!(format_clock(-3.0), "0:00");
    }

    #[test]
    fn playback_fps_and_fit() {
        assert_eq!(choose_playback_fps(None), DEFAULT_PLAYBACK_FPS);
        assert_eq!(choose_playback_fps(Some(59.94)), MAX_PLAYBACK_FPS);
        assert_eq!(choose_playback_fps(Some(24.0)), 24.0);
        assert_eq!(choose_playback_fps(Some(0.2)), 1.0);
        assert_eq!(choose_playback_fps(Some(f64::INFINITY)), DEFAULT_PLAYBACK_FPS);
        assert_eq!(fit_within(1920.0, 1080.0, 960.0, 960.0), (960.0, 540.0));
        assert_eq!(fit_within(640.0, 360.0, 960.0, 960.0), (640.0, 360.0));
        assert_eq!(fit_within(1080.0, 1920.0, 800.0, 600.0), (337.5, 600.0));
    }

    #[test]
    fn reads_ppm_frames_and_rejects_bad_headers() {
        let mut stream = Vec::new();
        stream.extend_from_slice(b"P6\n2 1\n255\n");
        stream.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        stream.extend_from_slice(b"P6 # comment\n1 1 255\n");
        stream.extend_from_slice(&[9, 9, 9]);
        let mut r = io::BufReader::new(&stream[..]);
        assert_eq!(read_ppm_frame(&mut r, 16).unwrap(), Some((2, 1, vec![1, 2, 3, 4, 5, 6])));
        assert_eq!(read_ppm_frame(&mut r, 16).unwrap(), Some((1, 1, vec![9, 9, 9])));
        assert_eq!(read_ppm_frame(&mut r, 16).unwrap(), None);

        let mut big = io::BufReader::new(&b"P6\n5000 10\n255\n"[..]);
        assert!(read_ppm_frame(&mut big, MAX_ACCEPTED_FRAME_DIM).is_err());
        let mut deep = io::BufReader::new(&b"P6\n1 1\n65535\n"[..]);
        assert!(read_ppm_frame(&mut deep, 16).is_err());
        let mut wrong = io::BufReader::new(&b"P5\n1 1\n255\n\0"[..]);
        assert!(read_ppm_frame(&mut wrong, 16).is_err());
        let mut truncated = io::BufReader::new(&b"P6\n2 2\n255\n\x01\x02"[..]);
        assert!(read_ppm_frame(&mut truncated, 16).is_err());
    }

    #[test]
    fn decoder_args_are_hardened() {
        let p = if cfg!(windows) { Path::new(r"C:\cache\videos\a.mp4") } else { Path::new("/cache/videos/a.mp4") };
        let v: Vec<String> = video_decode_args(p, 12.5, 25.0)
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        for flag in ["-nostdin", "-hide_banner", "-protocol_whitelist", "-an", "image2pipe"] {
            assert!(v.iter().any(|a| a == flag), "missing {flag}");
        }
        let wl = v.iter().position(|a| a == "-protocol_whitelist").unwrap();
        assert_eq!(v[wl + 1], "file");
        let fw = v.iter().position(|a| a == "-format_whitelist").unwrap();
        assert!(!v[fw + 1].contains("concat") && !v[fw + 1].contains("hls"));
        let ss = v.iter().position(|a| a == "-ss").unwrap();
        let i = v.iter().position(|a| a == "-i").unwrap();
        assert!(ss < i && wl < i && fw < i, "seek and whitelists are input options");
        assert_eq!(v[ss + 1], "12.500");
        assert!(v[i + 1].starts_with("file:"));
        assert!(v.iter().any(|a| a.starts_with("fps=25.000,scale=")));
        assert_eq!(v.last().map(String::as_str), Some("pipe:1"));

        let a: Vec<String> = audio_decode_args(p, 0.0, 48000, 2)
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert!(!a.iter().any(|x| x == "-ss"), "no seek at the start");
        let ar = a.iter().position(|x| x == "-ar").unwrap();
        assert_eq!(a[ar + 1], "48000");
        assert!(a.windows(2).any(|w| w[0] == "-f" && w[1] == "f32le"));
    }

    #[test]
    fn candidates_order_and_shim_rejection() {
        let (path_var, lad, pf, override_p) = if cfg!(windows) {
            (
                OsString::from(r"C:\Users\u\.local\bin;relative\dir;C:\Tools"),
                PathBuf::from(r"C:\Users\u\AppData\Local"),
                PathBuf::from(r"C:\Program Files"),
                OsString::from(r"D:\ff\ffmpeg.exe"),
            )
        } else {
            (
                OsString::from("/home/u/.local/bin:relative/dir:/opt/tools"),
                PathBuf::from("/home/u/.local/share"),
                PathBuf::from("/opt"),
                OsString::from("/srv/ff/ffmpeg"),
            )
        };
        let c = ffmpeg_candidates(Some(override_p.clone()), Some(path_var), Some(&lad), Some(&pf));
        assert_eq!(c[0], PathBuf::from(override_p));
        assert_eq!(c[1].file_name().unwrap(), FFMPEG_EXE);
        assert!(c.iter().all(|p| p.is_absolute()), "relative PATH entries are skipped");
        assert!(c.iter().any(|p| p.ends_with(Path::new("WinGet").join("Links").join(FFMPEG_EXE))));
        if cfg!(windows) {
            assert!(is_acceptable_ffmpeg_name(Path::new(r"C:\x\ffmpeg.exe")));
            assert!(is_acceptable_ffmpeg_name(Path::new(r"C:\x\FFMPEG.EXE")));
            assert!(!is_acceptable_ffmpeg_name(Path::new(r"C:\Users\u\.local\bin\ffmpeg.cmd")));
            assert!(!is_acceptable_ffmpeg_name(Path::new(r"C:\x\ffmpeg.bat")));
            assert!(!is_acceptable_ffmpeg_name(Path::new("ffmpeg.exe")));
        }
    }

    #[test]
    fn inside_dir_check() {
        let base = std::env::temp_dir().join(format!("qtzalo_video_test_{}", std::process::id()));
        let inner = base.join("videos");
        std::fs::create_dir_all(&inner).unwrap();
        let f = inner.join("a.mp4");
        std::fs::write(&f, b"x").unwrap();
        let outside = base.join("b.mp4");
        std::fs::write(&outside, b"y").unwrap();
        assert!(is_inside_dir(&f, &inner));
        assert!(!is_inside_dir(&outside, &inner));
        assert!(!is_inside_dir(&inner.join("..").join("b.mp4"), &inner));
        assert!(!is_inside_dir(&inner.join("missing.mp4"), &inner));
        let _ = std::fs::remove_dir_all(&base);
    }
}
