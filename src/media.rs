//! Phase 2 VoIP media: Opus encode/decode + RTP over UDP.
//!
//! v1: captures mic, encodes Opus, sends RTP to `rtp_addr`; receives RTP,
//! decodes Opus, plays to speaker. Simple blocking loop with 20 ms frames.
//! No jitter buffer / resampling / echo cancellation yet.

use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use opus::{Channels as OpusChannels, Decoder, Encoder};
use std::net::UdpSocket;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const SAMPLE_RATE: u32 = 16000;
const FRAME_SAMPLES: usize = 320; // 20 ms @ 16 kHz

pub struct MediaHandle {
    stop: Arc<AtomicBool>,
    mic_muted: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl MediaHandle {
    pub fn start(rtp_addr: String, local_port: u16) -> Result<Self> {
        Self::start_with_sockets(
            rtp_addr,
            String::new(),
            0,
            0,
            true,
            0,
            None,
            None,
            local_port,
            false,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start_with_sockets(
        rtp_addr: String,
        rtcp_addr: String,
        from_uid_u32: u32,
        partner_uid_u32: u32,
        is_caller: bool,
        relay_token: u32,
        udp_rtp_sock: Option<Arc<tokio::net::UdpSocket>>,
        udp_rtcp_sock: Option<Arc<tokio::net::UdpSocket>>,
        local_port: u16,
        is_video: bool,
        gui_tx: Option<std::sync::mpsc::Sender<crate::gui::GuiEvent>>,
        egui_ctx: Option<eframe::egui::Context>,
    ) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let mic_muted = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();

        let fallback_sock = if udp_rtp_sock.is_none() {
            let s = UdpSocket::bind(format!("0.0.0.0:{local_port}"))?;
            let _ = s.set_read_timeout(Some(Duration::from_millis(10)));
            Some(Arc::new(s))
        } else {
            None
        };

        let stop_t = stop.clone();
        let mute_t = mic_muted.clone();
        let t = thread::spawn(move || {
            run_media_loop(
                stop_t,
                mute_t,
                udp_rtp_sock,
                udp_rtcp_sock,
                fallback_sock,
                rtp_addr,
                rtcp_addr,
                from_uid_u32,
                partner_uid_u32,
                is_caller,
                relay_token,
                is_video,
                gui_tx,
                egui_ctx,
            );
        });
        threads.push(t);

        Ok(Self {
            stop,
            mic_muted,
            threads,
        })
    }

    pub fn set_mic_muted(&self, muted: bool) {
        self.mic_muted.store(muted, Ordering::Relaxed);
    }
}

impl Drop for MediaHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// Unwrap a Zalo UDP relay packet (`0x00893524..0x008936D1` in `ZaloCall.exe`) into its inner RTP slice.
pub fn unwrap_zalo_relay_rtp(pkt: &[u8]) -> Option<&[u8]> {
    if pkt.len() < 12 {
        return None;
    }
    let inner = match pkt[0] {
        // 1-byte relay media header (`0x04` = incoming audio RTP, `0x07` = P2P RTP, `0x0E` = incoming video RTP)
        0x04 | 0x07 | 0x0E => pkt.get(1..)?,
        // 5-byte relay media header (`0x03` / `0x05` / `0x0D` / `0x0F` + `token: u32 LE`)
        0x03 | 0x05 | 0x0D | 0x0F => pkt.get(5..)?,
        // 9-byte `0x7F` relay media header (`0x7F, 0x00, role, call_id: u32 LE, cmd: u16 LE`)
        0x7F if pkt.len() > 9 => {
            let cmd = u16::from_le_bytes([pkt[7], pkt[8]]);
            if (6..=10).contains(&cmd) {
                pkt.get(9..)?
            } else {
                return None;
            }
        }
        // 25-byte `0x02` relay media header (`cmd == 0x0032` at `0x008937C8`)
        0x02 if pkt.len() > 25 && u16::from_le_bytes([pkt[18], pkt[19]]) == 0x0032 => {
            pkt.get(25..)?
        }
        // Standard raw RTP v2 (`V=2` in top 2 bits)
        b if (b & 0xC0) == 0x80 => pkt,
        _ => return None,
    };
    if inner.len() >= 12 && (inner[0] & 0xC0) == 0x80 {
        Some(inner)
    } else {
        None
    }
}

/// Extract the Opus payload slice from a standard RFC 3550 RTP v2 packet (handling CSRC & header extensions).
fn extract_rtp_opus_payload(rtp: &[u8]) -> Option<&[u8]> {
    if rtp.len() < 12 || (rtp[0] & 0xC0) != 0x80 {
        return None;
    }
    let cc = (rtp[0] & 0x0F) as usize;
    let has_ext = (rtp[0] & 0x10) != 0;
    let has_pad = (rtp[0] & 0x20) != 0;
    let mut offset = 12 + cc * 4;
    if has_ext {
        if rtp.len() < offset + 4 {
            return None;
        }
        let ext_words = u16::from_be_bytes([rtp[offset + 2], rtp[offset + 3]]) as usize;
        offset += 4 + ext_words * 4;
    }
    let mut end = rtp.len();
    if has_pad && end > offset {
        let pad_len = rtp[end - 1] as usize;
        if pad_len > 0 && pad_len <= end - offset {
            end -= pad_len;
        }
    }
    if offset < end {
        Some(&rtp[offset..end])
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn run_media_loop(
    stop: Arc<AtomicBool>,
    mic_muted: Arc<AtomicBool>,
    udp_rtp_sock: Option<Arc<tokio::net::UdpSocket>>,
    udp_rtcp_sock: Option<Arc<tokio::net::UdpSocket>>,
    fallback_sock: Option<Arc<UdpSocket>>,
    rtp_addr: String,
    rtcp_addr: String,
    from_uid_u32: u32,
    partner_uid_u32: u32,
    is_caller: bool,
    relay_token: u32,
    is_video: bool,
    gui_tx: Option<std::sync::mpsc::Sender<crate::gui::GuiEvent>>,
    egui_ctx: Option<eframe::egui::Context>,
) {
    let (h264_tx, h264_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(120);
    if let (Some(tx), Some(ctx)) = (gui_tx, egui_ctx) {
        let stop_v = stop.clone();
        thread::spawn(move || {
            run_h264_ffmpeg_decoder(stop_v, h264_rx, tx, ctx);
        });
    }
    let mut fu_a_buf: Vec<u8> = Vec::with_capacity(65536);
    let host = cpal::default_host();
    let input_dev = match host.default_input_device() {
        Some(d) => d,
        None => { eprintln!("media: no input device"); return; }
    };
    let output_dev = match host.default_output_device() {
        Some(d) => d,
        None => { eprintln!("media: no output device"); return; }
    };

    let in_cfg = match input_dev.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("media: input config err {e}");
            return;
        }
    };
    let out_cfg = match output_dev.default_output_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("media: output config err {e}");
            return;
        }
    };
    let in_rate = in_cfg.sample_rate().0.max(8000);
    let in_channels = (in_cfg.channels() as usize).max(1);
    let out_rate = out_cfg.sample_rate().0.max(8000);
    let out_channels = (out_cfg.channels() as usize).max(1);

    // Shared 16kHz mono buffers
    let input_buf: Arc<Mutex<Vec<f32>>> =
        Arc::new(Mutex::new(Vec::with_capacity(FRAME_SAMPLES * 8)));
    let output_buf: Arc<Mutex<Vec<f32>>> =
        Arc::new(Mutex::new(Vec::with_capacity(FRAME_SAMPLES * 8)));
    let ib = input_buf.clone();

    // Input stream: downmix stereo -> mono and downsample `in_rate` -> 16000 Hz
    let input_stream = match input_dev.build_input_stream(
        &in_cfg.into(),
        move |data: &[f32], _| {
            if let Ok(mut b) = ib.lock() {
                let total_frames = data.len() / in_channels;
                let step = (in_rate as f32 / SAMPLE_RATE as f32).max(1.0);
                let out_frames = (total_frames as f32 / step) as usize;
                for i in 0..out_frames {
                    let src_idx = ((i as f32 * step) as usize).min(total_frames.saturating_sub(1));
                    let mut sum = 0.0_f32;
                    for ch in 0..in_channels {
                        sum += data[src_idx * in_channels + ch];
                    }
                    b.push(sum / in_channels as f32);
                }
                if b.len() > FRAME_SAMPLES * 25 {
                    let drop_n = b.len() - FRAME_SAMPLES * 25;
                    b.drain(0..drop_n);
                }
            }
        },
        |e| eprintln!("input stream err {e}"),
        None,
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("media: build input err {e}");
            return;
        }
    };
    if let Err(e) = input_stream.play() {
        eprintln!("media: play input err {e}");
    }

    // Output stream: upsample 16000 Hz mono -> `out_rate` and duplicate across `out_channels`
    let ob2 = output_buf.clone();
    let output_stream = match output_dev.build_output_stream(
        &out_cfg.into(),
        move |data: &mut [f32], _| {
            if let Ok(mut b) = ob2.lock() {
                let out_frames = data.len() / out_channels;
                let step = SAMPLE_RATE as f32 / out_rate as f32;
                let mut consumed = 0usize;
                for i in 0..out_frames {
                    let src_idx = (i as f32 * step) as usize;
                    let sample = if src_idx < b.len() {
                        consumed = consumed.max(src_idx + 1);
                        b[src_idx]
                    } else {
                        0.0
                    };
                    for ch in 0..out_channels {
                        data[i * out_channels + ch] = sample;
                    }
                }
                if consumed > 0 {
                    let drain_n = consumed.min(b.len());
                    b.drain(0..drain_n);
                }
            } else {
                for s in data.iter_mut() {
                    *s = 0.0;
                }
            }
        },
        |e| eprintln!("output stream err {e}"),
        None,
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("media: build output err {e}");
            return;
        }
    };
    if let Err(e) = output_stream.play() {
        eprintln!("media: play output err {e}");
    }

    // Opus codecs
    let mut encoder = match Encoder::new(SAMPLE_RATE, OpusChannels::Mono, opus::Application::Voip) {
        Ok(e) => e,
        Err(e) => { eprintln!("media: encoder err {e}"); return; }
    };
    let mut decoder = match Decoder::new(SAMPLE_RATE, OpusChannels::Mono) {
        Ok(d) => d,
        Err(e) => { eprintln!("media: decoder err {e}"); return; }
    };

    let mut seq: u16 = 0;
    let mut ts: u32 = 0;
    let ssrc = if from_uid_u32 != 0 {
        from_uid_u32
    } else {
        0x5A41_4C4F
    };
    let rtp_sock_addr = rtp_addr.parse::<std::net::SocketAddr>().ok();
    let rtcp_sock_addr = rtcp_addr.parse::<std::net::SocketAddr>().ok();
    let mut last_rtcp = std::time::Instant::now();
    let mut last_rtp_tx = std::time::Instant::now();
    let mut last_video_tx = std::time::Instant::now();
    let mut vid_seq: u16 = 0;
    let mut vid_ts: u32 = 0;
    let vid_ssrc = ssrc.wrapping_add(1);

    while !stop.load(Ordering::Relaxed) {
        // --- Capture: take 320 samples (or emit comfort-noise every 20ms so peer never sees 0kbps) ---
        let mut frame: Vec<f32> = {
            let mut b = input_buf.lock().unwrap();
            if b.len() >= FRAME_SAMPLES {
                b.drain(0..FRAME_SAMPLES).collect()
            } else {
                Vec::new()
            }
        };
        if frame.is_empty() && last_rtp_tx.elapsed() >= Duration::from_millis(20) {
            frame = vec![0.0_f32; FRAME_SAMPLES];
        }
        if frame.len() == FRAME_SAMPLES {
            last_rtp_tx = std::time::Instant::now();
            if mic_muted.load(Ordering::Relaxed) {
                frame.fill(0.0);
            }
            let mut out = vec![0u8; 4000];
            match encoder.encode_float(&frame, &mut out) {
                Ok(n) => {
                    // Build inner RTP header (12 bytes, PT=112 matching Zalo's opus/16000/1 codec)
                    let mut rtp_pkt = Vec::with_capacity(12 + n);
                    rtp_pkt.push(0x80); // V=2, P=0, X=0, CC=0
                    rtp_pkt.push(112); // payload type 112 (opus/16000/1)
                    rtp_pkt.extend_from_slice(&seq.to_be_bytes());
                    rtp_pkt.extend_from_slice(&ts.to_be_bytes());
                    rtp_pkt.extend_from_slice(&ssrc.to_be_bytes());
                    rtp_pkt.extend_from_slice(&out[..n]);

                    if let (Some(ref tsock), Some(target)) = (&udp_rtp_sock, rtp_sock_addr) {
                        if relay_token != 0 {
                            let type_byte: u8 = if is_caller { 0x03 } else { 0x05 };
                            let mut wire = Vec::with_capacity(5 + rtp_pkt.len());
                            wire.push(type_byte);
                            wire.extend_from_slice(&relay_token.to_le_bytes());
                            wire.extend_from_slice(&rtp_pkt);
                            let _ = tsock.try_send_to(&wire, target);
                        } else {
                            let _ = tsock.try_send_to(&rtp_pkt, target);
                        }
                    } else if let Some(ref fsock) = fallback_sock {
                        let _ = fsock.send_to(&rtp_pkt, &rtp_addr);
                    }
                    seq = seq.wrapping_add(1);
                    ts = ts.wrapping_add(FRAME_SAMPLES as u32);
                }
                Err(e) => eprintln!("media: encode err {e}"),
            }
        }

        // --- Video RTP Keepalive / Synthetic H.264 Stream (`PT = 97`, 90kHz clock, 15 fps) ---
        // Prevents the remote Zalo client's GCC/Video watchdog from showing "Kết nối không ổn định"
        if is_video && last_video_tx.elapsed() >= Duration::from_millis(66) {
            last_video_tx = std::time::Instant::now();
            // Minimal valid H.264 Baseline SPS/PPS/IDR / P-slice NAL units (RFC 6184)
            const H264_SPS: &[u8] = &[0x67, 0x42, 0xC0, 0x1E, 0xD9, 0x01, 0x41, 0xFB, 0x01, 0x10, 0x00, 0x00, 0x03, 0x00, 0x10, 0x00, 0x00, 0x03, 0x03, 0xC0, 0xF1, 0x42, 0x99, 0x60];
            const H264_PPS: &[u8] = &[0x68, 0xCE, 0x06, 0xE2];
            const H264_IDR: &[u8] = &[0x65, 0x88, 0x84, 0x00, 0x10, 0xFF, 0xFE, 0xF6, 0xF0, 0xFE, 0x05, 0x36, 0x56, 0x04, 0x50, 0x96, 0x7B, 0x3F];
            const H264_PSLICE: &[u8] = &[0x41, 0x9A, 0x24, 0x6C, 0x41, 0xFF];

            let send_vid_nal = |nal: &[u8], marker: bool, vseq: &mut u16, vts: u32| {
                let mut vpkt = Vec::with_capacity(12 + nal.len());
                vpkt.push(0x80);
                vpkt.push(if marker { 0x80 | 97 } else { 97 }); // PT = 97 (h264)
                vpkt.extend_from_slice(&vseq.to_be_bytes());
                vpkt.extend_from_slice(&vts.to_be_bytes());
                vpkt.extend_from_slice(&vid_ssrc.to_be_bytes());
                vpkt.extend_from_slice(nal);
                *vseq = vseq.wrapping_add(1);
                if let (Some(ref tsock), Some(target)) = (&udp_rtp_sock, rtp_sock_addr) {
                    if relay_token != 0 {
                        let type_byte: u8 = if is_caller { 0x0D } else { 0x0E };
                        let mut wire = Vec::with_capacity(5 + vpkt.len());
                        wire.push(type_byte);
                        wire.extend_from_slice(&relay_token.to_le_bytes());
                        wire.extend_from_slice(&vpkt);
                        let _ = tsock.try_send_to(&wire, target);
                    } else {
                        let _ = tsock.try_send_to(&vpkt, target);
                    }
                }
            };
            if vid_seq % 15 == 0 {
                send_vid_nal(H264_SPS, false, &mut vid_seq, vid_ts);
                send_vid_nal(H264_PPS, false, &mut vid_seq, vid_ts);
                send_vid_nal(H264_IDR, true, &mut vid_seq, vid_ts);
            } else {
                send_vid_nal(H264_PSLICE, true, &mut vid_seq, vid_ts);
            }
            vid_ts = vid_ts.wrapping_add(6000); // 90000 Hz / 15 fps = 6000 ticks
        }
        if last_rtcp.elapsed() >= Duration::from_secs(2) {
            last_rtcp = std::time::Instant::now();
            if let (Some(ref rtcp_s), Some(target)) = (&udp_rtcp_sock, rtcp_sock_addr) {
                let mut rtcp_pkt = [0u8; 32];
                rtcp_pkt[0] = 0x81; // V=2, RC=1
                rtcp_pkt[1] = 201; // PT=201 (Receiver Report)
                rtcp_pkt[2..4].copy_from_slice(&7u16.to_be_bytes());
                rtcp_pkt[4..8].copy_from_slice(&ssrc.to_be_bytes());
                rtcp_pkt[8..12].copy_from_slice(&partner_uid_u32.to_be_bytes());
                if relay_token != 0 {
                    let rtcp_type: u8 = if is_caller { 0x0D } else { 0x0F };
                    let mut wire = Vec::with_capacity(5 + rtcp_pkt.len());
                    wire.push(rtcp_type);
                    wire.extend_from_slice(&relay_token.to_le_bytes());
                    wire.extend_from_slice(&rtcp_pkt);
                    let _ = rtcp_s.try_send_to(&wire, target);
                } else {
                    let _ = rtcp_s.try_send_to(&rtcp_pkt, target);
                }
            }
        }

        // --- Receive: unwrap Zalo relay framing, parse RTP, decode Opus, play ---
        let mut buf = [0u8; 4000];
        for _ in 0..8 {
            let recv_info = if let Some(tsock) = &udp_rtp_sock {
                tsock.try_recv_from(&mut buf).ok()
            } else if let Some(fsock) = &fallback_sock {
                fsock.recv_from(&mut buf).ok()
            } else {
                None
            };
            let Some((n, peer)) = recv_info else {
                break;
            };
            if let Some(inner_rtp) = unwrap_zalo_relay_rtp(&buf[..n]) {
                let pt = inner_rtp.get(1).map(|b| b & 0x7F).unwrap_or(0);
                static LOGGED_RX: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(0);
                let cnt = LOGGED_RX.fetch_add(1, Ordering::Relaxed);
                if cnt < 16 || cnt % 100 == 0 {
                    crate::call::trace_line(
                        "RTP<",
                        format!(
                            "from={peer} len={n} outer=0x{:02x} pt={pt}",
                            buf[0]
                        ),
                    );
                }
                if pt == 112 {
                    if let Some(payload) = extract_rtp_opus_payload(inner_rtp) {
                        let mut pcm = vec![0f32; FRAME_SAMPLES];
                        if decoder.decode_float(payload, &mut pcm, false).is_ok() {
                            if let Ok(mut b) = output_buf.lock() {
                                b.extend_from_slice(&pcm);
                            }
                        }
                    }
                } else if pt == 96 || pt == 97 || buf[0] == 0x0E {
                    if let Some(payload) = extract_rtp_opus_payload(inner_rtp) {
                        depacketize_rfc6184_h264(payload, &mut fu_a_buf, &h264_tx);
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(5));
    }

    drop(input_stream);
    drop(output_stream);
}

/// Reassemble RFC 6184 H.264 RTP payloads (Single NAL 1..23, STAP-A 24, FU-A 28) into Annex-B (`00 00 00 01`) byte stream.
fn depacketize_rfc6184_h264(
    payload: &[u8],
    fu_a_buf: &mut Vec<u8>,
    h264_tx: &std::sync::mpsc::SyncSender<Vec<u8>>,
) {
    if payload.is_empty() {
        return;
    }
    let nal_type = payload[0] & 0x1F;
    match nal_type {
        1..=23 => {
            let mut annexb = Vec::with_capacity(4 + payload.len());
            annexb.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
            annexb.extend_from_slice(payload);
            let _ = h264_tx.try_send(annexb);
        }
        24 => {
            // STAP-A: multiple NAL units prefixed by u16 BE length
            let mut idx = 1usize;
            let mut annexb = Vec::new();
            while idx + 2 <= payload.len() {
                let sz = u16::from_be_bytes([payload[idx], payload[idx + 1]]) as usize;
                idx += 2;
                if sz > 0 && idx + sz <= payload.len() {
                    annexb.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
                    annexb.extend_from_slice(&payload[idx..idx + sz]);
                    idx += sz;
                } else {
                    break;
                }
            }
            if !annexb.is_empty() {
                let _ = h264_tx.try_send(annexb);
            }
        }
        28 if payload.len() >= 2 => {
            // FU-A fragmented NAL unit
            let fu_ind = payload[0];
            let fu_hdr = payload[1];
            let start_bit = (fu_hdr & 0x80) != 0;
            let end_bit = (fu_hdr & 0x40) != 0;
            let orig_nal_hdr = (fu_ind & 0xE0) | (fu_hdr & 0x1F);
            if start_bit {
                fu_a_buf.clear();
                fu_a_buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, orig_nal_hdr]);
                fu_a_buf.extend_from_slice(&payload[2..]);
            } else if !fu_a_buf.is_empty() {
                fu_a_buf.extend_from_slice(&payload[2..]);
                if end_bit {
                    let completed = std::mem::take(fu_a_buf);
                    let _ = h264_tx.try_send(completed);
                }
            }
        }
        _ => {}
    }
}

/// Spawn a headless `ffmpeg` pipe to decode incoming H.264 Annex-B NAL units into `480×360` RGBA frames.
fn run_h264_ffmpeg_decoder(
    stop: Arc<AtomicBool>,
    h264_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    gui_tx: std::sync::mpsc::Sender<crate::gui::GuiEvent>,
    egui_ctx: eframe::egui::Context,
) {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};

    const VID_W: usize = 480;
    const VID_H: usize = 360;
    const FRAME_BYTES: usize = VID_W * VID_H * 4;

    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-fflags",
        "nobuffer",
        "-flags",
        "low_delay",
        "-f",
        "h264",
        "-i",
        "pipe:0",
        "-vf",
        "scale=480:360:force_original_aspect_ratio=decrease,pad=480:360:(ow-iw)/2:(oh-ih)/2",
        "-pix_fmt",
        "rgba",
        "-f",
        "rawvideo",
        "pipe:1",
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let Ok(mut child) = cmd.spawn() else {
        return;
    };
    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        return;
    };
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        return;
    };

    let stop_writer = stop.clone();
    thread::spawn(move || {
        while !stop_writer.load(Ordering::Relaxed) {
            match h264_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(nal) => {
                    if stdin.write_all(&nal).is_err() {
                        break;
                    }
                    let _ = stdin.flush();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    let mut raw_frame = vec![0u8; FRAME_BYTES];
    while !stop.load(Ordering::Relaxed) {
        if stdout.read_exact(&mut raw_frame).is_ok() {
            let color_img =
                eframe::egui::ColorImage::from_rgba_unmultiplied([VID_W, VID_H], &raw_frame);
            let _ = gui_tx.send(crate::gui::GuiEvent::StickerImageReady {
                key: "call_remote_video".to_string(),
                image: color_img,
            });
            egui_ctx.request_repaint();
        } else {
            break;
        }
    }
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unwrap_zalo_relay_rtp_all_formats() {
        let mut sample_rtp = vec![0x80, 112, 0x00, 0x01, 0x00, 0x00, 0x01, 0x40, 0x12, 0x34, 0x56, 0x78];
        sample_rtp.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);

        // 1. Raw RTP (0x80)
        let r1 = unwrap_zalo_relay_rtp(&sample_rtp).expect("raw rtp");
        assert_eq!(extract_rtp_opus_payload(r1), Some(&[0xAA, 0xBB, 0xCC, 0xDD][..]));

        // 2. 1-byte relay header (0x04 incoming RTP)
        let mut p4 = vec![0x04];
        p4.extend_from_slice(&sample_rtp);
        let r2 = unwrap_zalo_relay_rtp(&p4).expect("0x04 relay rtp");
        assert_eq!(extract_rtp_opus_payload(r2), Some(&[0xAA, 0xBB, 0xCC, 0xDD][..]));

        // 3. 5-byte relay header (0x03 caller / 0x05 callee + 4-byte token LE)
        let mut p5 = vec![0x03, 0x78, 0x56, 0x34, 0x12];
        p5.extend_from_slice(&sample_rtp);
        let r3 = unwrap_zalo_relay_rtp(&p5).expect("0x03 relay rtp");
        assert_eq!(extract_rtp_opus_payload(r3), Some(&[0xAA, 0xBB, 0xCC, 0xDD][..]));

        // 4. 9-byte 0x7F relay header (cmd = 6)
        let mut p9 = vec![0x7F, 0x00, 0x00, 0x11, 0x22, 0x33, 0x44, 0x06, 0x00];
        p9.extend_from_slice(&sample_rtp);
        let r4 = unwrap_zalo_relay_rtp(&p9).expect("0x7F relay rtp");
        assert_eq!(extract_rtp_opus_payload(r4), Some(&[0xAA, 0xBB, 0xCC, 0xDD][..]));
    }
}