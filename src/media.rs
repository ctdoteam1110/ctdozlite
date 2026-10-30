//! Phase 2 VoIP media: Opus encode/decode + RTP over UDP.
//!
//! v1: captures mic, encodes Opus, sends RTP to `rtp_addr`; receives RTP,
//! decodes Opus, plays to speaker. Simple blocking loop with 20 ms frames.
//! No jitter buffer / resampling / echo cancellation yet.

use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use opus::{Channels as OpusChannels, Decoder, Encoder};
use std::collections::VecDeque;
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

/// Inbox for datagrams received on the RTCP socket. The ZRTP keepalive task in `call.rs` is the
/// permanent reader of that socket; it forwards everything that is not a ZRTP init reply here so
/// `run_media_loop` never competes with it for datagrams. Single active call assumed.
static RTCP_INBOX: Mutex<VecDeque<Vec<u8>>> = Mutex::new(VecDeque::new());
const RTCP_INBOX_MAX: usize = 64;

/// Queue a raw (still relay-framed) datagram read from the RTCP socket. Bounded; oldest is dropped.
pub fn push_rtcp_inbox(pkt: &[u8]) {
    if let Ok(mut q) = RTCP_INBOX.lock() {
        if q.len() >= RTCP_INBOX_MAX {
            q.pop_front();
        }
        q.push_back(pkt.to_vec());
    }
}

fn drain_rtcp_inbox() -> Vec<Vec<u8>> {
    RTCP_INBOX.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default()
}

fn clear_rtcp_inbox() {
    if let Ok(mut q) = RTCP_INBOX.lock() {
        q.clear();
    }
}

/// True if a raw relay-framed datagram is RTCP (PT 200..=207) containing a PLI or FIR.
#[cfg(test)]
fn wire_requests_keyframe(wire: &[u8]) -> bool {
    match unwrap_zalo_relay_rtp(wire) {
        Some(inner) => {
            matches!(inner.get(1), Some(b) if (200..=207).contains(b)) && rtcp_requests_keyframe(inner)
        }
        None => false,
    }
}

const VIDEO_PT: u8 = 97;

/// Zalo video wire format, observed in two live calls (`voip.log` 2026-10-30, peer -> us), NOT RFC 6184:
/// * Whole access unit as Annex-B (`00 00 00 01 <NAL>...`) in one RTP packet, marker = 1.
/// * Larger access units: `0x1C, flags, <chunk of the Annex-B AU>` with flags `0x80` = first, `0x40` = last,
///   `0x00` = middle (low 5 bits 0); marker only on the last fragment. The AU is split **evenly** into
///   `ceil(len / ~1200)` parts (captured: 2x1083, 3x1062 in one call, 853/853/851 in another; the largest
///   unfragmented payload was 1212 bytes).
/// * Payload type: `PT 98` was seen only on the fragments of the first keyframe (SPS/PPS/SEI/IDR); fragmented
///   P frames used `PT 97`. So "PT 98 = keyframe" is an inference from two samples; receive ignores the PT.
const VIDEO_PT_FRAG: u8 = 98;
const ZALO_FRAG_INDICATOR: u8 = 0x1C;
/// Max RTP payload per packet (2-byte fragment header included). Peer packets reached 1212; 1200 is the
/// threshold that reproduces its split sizes. Whether the relay/peer accepts it from us is unverified.
const RTP_MAX_PAYLOAD: usize = 1200;
const ZALO_FRAG_BUF_MAX: usize = 1 << 20;
const VIDEO_IDR_INTERVAL_FRAMES: u32 = 15;

// Minimal valid H.264 Baseline SPS/PPS/IDR / P-slice NAL units (RFC 6184)
const H264_SPS: &[u8] = &[0x67, 0x42, 0xC0, 0x1E, 0xD9, 0x01, 0x41, 0xFB, 0x01, 0x10, 0x00, 0x00, 0x03, 0x00, 0x10, 0x00, 0x00, 0x03, 0x03, 0xC0, 0xF1, 0x42, 0x99, 0x60];
const H264_PPS: &[u8] = &[0x68, 0xCE, 0x06, 0xE2];
const H264_IDR: &[u8] = &[0x65, 0x88, 0x84, 0x00, 0x10, 0xFF, 0xFE, 0xF6, 0xF0, 0xFE, 0x05, 0x36, 0x56, 0x04, 0x50, 0x96, 0x7B, 0x3F];
const H264_PSLICE: &[u8] = &[0x41, 0x9A, 0x24, 0x6C, 0x41, 0xFF];

/// Build a 12-byte-header RTP v2 packet.
fn build_rtp_packet(pt: u8, marker: bool, seq: u16, ts: u32, ssrc: u32, payload: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(12 + payload.len());
    p.push(0x80);
    p.push(if marker { 0x80 | pt } else { pt });
    p.extend_from_slice(&seq.to_be_bytes());
    p.extend_from_slice(&ts.to_be_bytes());
    p.extend_from_slice(&ssrc.to_be_bytes());
    p.extend_from_slice(payload);
    p
}

/// Concatenate NAL units (no start code) into one Annex-B access unit.
fn annexb_au(nals: &[&[u8]]) -> Vec<u8> {
    let mut v = Vec::new();
    for n in nals {
        v.extend_from_slice(&[0, 0, 0, 1]);
        v.extend_from_slice(n);
    }
    v
}

fn is_annexb(p: &[u8]) -> bool {
    p.starts_with(&[0, 0, 0, 1]) || p.starts_with(&[0, 0, 1])
}

fn is_zalo_frag(p: &[u8]) -> bool {
    p.len() >= 2 && (p[0] & 0x1F) == 28 && (p[1] & 0x1F) == 0
}

/// True if the Annex-B access unit contains an IDR slice (NAL type 5).
fn au_has_idr(au: &[u8]) -> bool {
    let mut i = 0usize;
    while i + 4 < au.len() {
        if au[i] == 0 && au[i + 1] == 0 && au[i + 2] == 1 {
            if au[i + 3] & 0x1F == 5 {
                return true;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    false
}

/// Split one Annex-B access unit into Zalo video payloads: `(pt, payload)` per RTP packet.
/// Fits in one packet -> `PT 97`. Otherwise it is split evenly into `ceil(len / (max_payload - 2))` fragments;
/// those use `PT 98` for a keyframe and `PT 97` for other frames (inferred, see `VIDEO_PT_FRAG`).
fn packetize_zalo_au(au: &[u8], max_payload: usize) -> Vec<(u8, Vec<u8>)> {
    if au.is_empty() {
        return Vec::new();
    }
    if au.len() <= max_payload || max_payload < 3 {
        return vec![(VIDEO_PT, au.to_vec())];
    }
    let parts = au.len().div_ceil(max_payload - 2);
    let chunk = au.len().div_ceil(parts);
    let n = au.len().div_ceil(chunk); // actual number of fragments produced by `chunks(chunk)`
    let pt = if au_has_idr(au) { VIDEO_PT_FRAG } else { VIDEO_PT };
    au.chunks(chunk)
        .enumerate()
        .map(|(i, c)| {
            let mut flags = 0u8;
            if i == 0 {
                flags |= 0x80;
            }
            if i + 1 == n {
                flags |= 0x40;
            }
            let mut v = Vec::with_capacity(2 + c.len());
            v.push(ZALO_FRAG_INDICATOR);
            v.push(flags);
            v.extend_from_slice(c);
            (pt, v)
        })
        .collect()
}

/// Feed one video RTP payload to the decoder channel. Dispatch is by content (unambiguous: RFC 6184
/// never starts with byte `0x00` and never uses FU type 0): Annex-B access unit, Zalo fragment, else RFC 6184.
fn depacketize_h264(
    payload: &[u8],
    frag_buf: &mut Vec<u8>,
    h264_tx: &std::sync::mpsc::SyncSender<Vec<u8>>,
) {
    if payload.is_empty() {
        return;
    }
    if is_annexb(payload) {
        let _ = h264_tx.try_send(payload.to_vec());
        return;
    }
    if is_zalo_frag(payload) {
        let flags = payload[1];
        let body = &payload[2..];
        if flags & 0x80 != 0 {
            frag_buf.clear();
            frag_buf.extend_from_slice(body);
        } else if !frag_buf.is_empty() {
            frag_buf.extend_from_slice(body);
        } else {
            return; // first fragment lost: drop the rest of this access unit
        }
        if frag_buf.len() > ZALO_FRAG_BUF_MAX {
            frag_buf.clear();
            return;
        }
        if flags & 0x40 != 0 {
            let done = std::mem::take(frag_buf);
            if !done.is_empty() {
                let _ = h264_tx.try_send(done);
            }
        }
        return;
    }
    depacketize_rfc6184_h264(payload, frag_buf, h264_tx);
}

/// RFC 6184 packetization of one NAL unit (header byte included, no start code).
/// No longer used on the wire (the peer sends/expects the Zalo format above); kept to test the RFC 6184 receive path.
/// Returns one single-NAL payload when it fits in `max_payload`, otherwise FU-A fragments.
#[cfg(test)]
fn packetize_h264_nal(nal: &[u8], max_payload: usize) -> Vec<Vec<u8>> {
    if nal.is_empty() {
        return Vec::new();
    }
    if nal.len() <= max_payload || max_payload < 3 {
        return vec![nal.to_vec()];
    }
    let indicator = (nal[0] & 0xE0) | 28;
    let nal_type = nal[0] & 0x1F;
    let body = &nal[1..];
    let chunk = max_payload - 2;
    let n = body.len().div_ceil(chunk);
    body.chunks(chunk)
        .enumerate()
        .map(|(i, c)| {
            let mut fu_hdr = nal_type;
            if i == 0 {
                fu_hdr |= 0x80;
            }
            if i + 1 == n {
                fu_hdr |= 0x40;
            }
            let mut v = Vec::with_capacity(2 + c.len());
            v.push(indicator);
            v.push(fu_hdr);
            v.extend_from_slice(c);
            v
        })
        .collect()
}

/// True if a (compound) RTCP packet contains a PLI (PSFB fmt 1) or FIR (PSFB fmt 4).
pub(crate) fn rtcp_requests_keyframe(rtcp: &[u8]) -> bool {
    let mut i = 0usize;
    while i + 4 <= rtcp.len() {
        if (rtcp[i] & 0xC0) != 0x80 {
            return false;
        }
        let fmt = rtcp[i] & 0x1F;
        let pt = rtcp[i + 1];
        let words = u16::from_be_bytes([rtcp[i + 2], rtcp[i + 3]]) as usize;
        if pt == 206 && (fmt == 1 || fmt == 4) {
            return true;
        }
        i += (words + 1) * 4;
    }
    false
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
    let mut vid_frame_no: u32 = 0;
    clear_rtcp_inbox();
    let mut wl = crate::voip_log::WireLog::new();
    crate::voip_log::line(
        "CALL",
        format!(
            "media start rtp={rtp_addr} rtcp={rtcp_addr} video={is_video} caller={is_caller} relay_token={} tx_video_pt={VIDEO_PT} tx_frag_pt={VIDEO_PT_FRAG} video_format=annexb-au tx_audio_pt=112 max_payload={RTP_MAX_PAYLOAD}",
            relay_token != 0
        ),
    );
    let want_keyframe = AtomicBool::new(false);
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
                            wl.rtp(">", type_byte, wire.len(), &rtp_addr, &rtp_pkt);
                        } else {
                            let _ = tsock.try_send_to(&rtp_pkt, target);
                            wl.rtp(">", rtp_pkt[0], rtp_pkt.len(), &rtp_addr, &rtp_pkt);
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

        // --- Video: synthetic H.264 stream (90 kHz clock, 15 fps) in the Zalo wire format ---
        // One Annex-B access unit per frame: `PT 97` when it fits `RTP_MAX_PAYLOAD`, else `PT 98` fragments
        // (see `packetize_zalo_au`); marker bit only on the last packet of the frame.
        // A PLI/FIR from the peer (RTCP inbox, or RTP socket if the relay mixes them) forces an immediate SPS+PPS+IDR.
        if is_video && last_video_tx.elapsed() >= Duration::from_millis(66) {
            last_video_tx = std::time::Instant::now();
            let send_idr = vid_frame_no % VIDEO_IDR_INTERVAL_FRAMES == 0
                || want_keyframe.swap(false, Ordering::Relaxed);
            let nals: &[&[u8]] = if send_idr {
                &[H264_SPS, H264_PPS, H264_IDR]
            } else {
                &[H264_PSLICE]
            };
            let payloads = packetize_zalo_au(&annexb_au(nals), RTP_MAX_PAYLOAD);
            let last = payloads.len().saturating_sub(1);
            for (i, (vpt, payload)) in payloads.iter().enumerate() {
                let vpkt = build_rtp_packet(*vpt, i == last, vid_seq, vid_ts, vid_ssrc, payload);
                vid_seq = vid_seq.wrapping_add(1);
                if let (Some(ref tsock), Some(target)) = (&udp_rtp_sock, rtp_sock_addr) {
                    if relay_token != 0 {
                        let type_byte: u8 = if is_caller { 0x0D } else { 0x0E };
                        let mut wire = Vec::with_capacity(5 + vpkt.len());
                        wire.push(type_byte);
                        wire.extend_from_slice(&relay_token.to_le_bytes());
                        wire.extend_from_slice(&vpkt);
                        let _ = tsock.try_send_to(&wire, target);
                        wl.rtp(">", type_byte, wire.len(), &rtp_addr, &vpkt);
                    } else {
                        let _ = tsock.try_send_to(&vpkt, target);
                        wl.rtp(">", vpkt[0], vpkt.len(), &rtp_addr, &vpkt);
                    }
                }
            }
            vid_frame_no = vid_frame_no.wrapping_add(1);
            vid_ts = vid_ts.wrapping_add(6000); // 90000 Hz / 15 fps
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
                    wl.rtcp(">", crate::voip_log::Via::RtcpSock, rtcp_type, wire.len(), &rtcp_addr, &rtcp_pkt);
                } else {
                    let _ = rtcp_s.try_send_to(&rtcp_pkt, target);
                    wl.rtcp(">", crate::voip_log::Via::RtcpSock, rtcp_pkt[0], rtcp_pkt.len(), &rtcp_addr, &rtcp_pkt);
                }
            }
        }

        // --- RTCP from the keepalive task's socket reader: PLI / FIR -> force a keyframe ---
        for wire in drain_rtcp_inbox() {
            match unwrap_zalo_relay_rtp(&wire) {
                Some(inner) if matches!(inner.get(1), Some(b) if (200..=207).contains(b)) => {
                    if wl.rtcp("<", crate::voip_log::Via::RtcpSock, wire[0], wire.len(), &rtcp_addr, inner) {
                        want_keyframe.store(true, Ordering::Relaxed);
                        crate::call::trace_line("RTCP<", format!("len={} PLI/FIR -> force IDR", wire.len()));
                    }
                }
                Some(inner) => {
                    // Media RTP that arrived on the RTCP socket: worth knowing (rtcp-mux?).
                    wl.rtp("<", wire[0], wire.len(), &rtcp_addr, inner);
                }
                None => wl.unknown(crate::voip_log::Via::RtcpSock, wire.len(), &rtcp_addr, &wire),
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
            let peer_s = peer.to_string();
            if unwrap_zalo_relay_rtp(&buf[..n]).is_none() {
                wl.unknown(crate::voip_log::Via::RtpSock, n, &peer_s, &buf[..n]);
            }
            if let Some(inner_rtp) = unwrap_zalo_relay_rtp(&buf[..n]) {
                // RTCP (RFC 5761 range 200..=207): react to PLI / FIR by forcing a keyframe.
                if matches!(inner_rtp.get(1), Some(b) if (200..=207).contains(b)) {
                    wl.rtcp("<", crate::voip_log::Via::RtpSock, buf[0], n, &peer_s, inner_rtp);
                    if rtcp_requests_keyframe(inner_rtp) {
                        want_keyframe.store(true, Ordering::Relaxed);
                        crate::call::trace_line("RTCP<", format!("from={peer} PLI/FIR -> force IDR"));
                    }
                    continue;
                }
                let pt = inner_rtp.get(1).map(|b| b & 0x7F).unwrap_or(0);
                wl.rtp("<", buf[0], n, &peer_s, inner_rtp);
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
                } else if pt == 96 || pt == 97 || pt == VIDEO_PT_FRAG || buf[0] == 0x0E {
                    if let Some(payload) = extract_rtp_opus_payload(inner_rtp) {
                        depacketize_h264(payload, &mut fu_a_buf, &h264_tx);
                    }
                }
            }
        }
        wl.tick();
        thread::sleep(Duration::from_millis(5));
    }
    crate::voip_log::line("CALL", "media stop");

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
        // Without these ffmpeg probes ~5 MB / 5 s of stream before the first frame; with 15 fps video
        // that is ~3.8 s measured vs ~1.0 s (synthetic stream, see re-notes/08_video_call.md).
        "-probesize",
        "32",
        "-analyzeduration",
        "0",
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
    .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            crate::voip_log::line("VDEC", format!("ffmpeg spawn failed: {e}"));
            return;
        }
    };
    crate::voip_log::line("VDEC", "ffmpeg decoder started");
    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        return;
    };
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        return;
    };
    if let Some(mut stderr) = child.stderr.take() {
        // ffmpeg diagnostics (decode errors, missing reference frames) -> voip.log, bounded.
        thread::spawn(move || {
            let mut buf = [0u8; 512];
            let mut lines = 0u32;
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                lines += 1;
                if lines <= 20 {
                    crate::voip_log::line(
                        "VDEC",
                        format!("ffmpeg: {}", String::from_utf8_lossy(&buf[..n]).trim()),
                    );
                }
            }
        });
    }

    let stop_writer = stop.clone();
    thread::spawn(move || {
        let mut fed = 0u64;
        while !stop_writer.load(Ordering::Relaxed) {
            match h264_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(nal) => {
                    if stdin.write_all(&nal).is_err() {
                        crate::voip_log::line("VDEC", format!("ffmpeg stdin closed after {fed} access units"));
                        break;
                    }
                    let _ = stdin.flush();
                    fed += 1;
                    if fed == 1 || fed % 100 == 0 {
                        crate::voip_log::line("VDEC", format!("fed {fed} access units to ffmpeg"));
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    let mut raw_frame = vec![0u8; FRAME_BYTES];
    let mut decoded = 0u64;
    while !stop.load(Ordering::Relaxed) {
        if stdout.read_exact(&mut raw_frame).is_ok() {
            decoded += 1;
            if decoded == 1 || decoded % 100 == 0 {
                crate::voip_log::line("VDEC", format!("decoded frame #{decoded}"));
            }
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
    crate::voip_log::line("VDEC", format!("decoder stop, {decoded} frames decoded"));
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

    #[test]
    fn fu_a_roundtrip_and_marker_layout() {
        let mut nal = vec![0x65u8];
        nal.extend((0..3000u32).map(|i| (i % 251) as u8));
        let parts = packetize_h264_nal(&nal, 1200);
        assert!(parts.len() >= 3);
        assert!(parts.iter().all(|p| p.len() <= 1200));
        assert_eq!(parts[0][0], (0x65 & 0xE0) | 28);
        assert_eq!(parts[0][1], 0x80 | 5);
        assert_eq!(parts.last().unwrap()[1], 0x40 | 5);
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);
        let mut buf = Vec::new();
        for p in &parts {
            depacketize_rfc6184_h264(p, &mut buf, &tx);
        }
        let out = rx.try_recv().expect("reassembled NAL");
        let mut expect = vec![0, 0, 0, 1];
        expect.extend_from_slice(&nal);
        assert_eq!(out, expect);
    }

    #[test]
    fn small_nal_is_single_packet() {
        let parts = packetize_h264_nal(H264_PPS, 1200);
        assert_eq!(parts, vec![H264_PPS.to_vec()]);
    }

    #[test]
    fn detects_pli_and_fir_in_compound_rtcp() {
        // RR (len=1) followed by PLI (fmt 1, len=2)
        let mut c = vec![0x80, 201, 0, 1, 0, 0, 0, 1];
        c.extend_from_slice(&[0x81, 206, 0, 2, 0, 0, 0, 1, 0, 0, 0, 2]);
        assert!(rtcp_requests_keyframe(&c));
        let fir = [0x84, 206, 0, 4, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2, 1, 0, 0, 0];
        assert!(rtcp_requests_keyframe(&fir));
        let rr = [0x80, 201, 0, 1, 0, 0, 0, 1];
        assert!(!rtcp_requests_keyframe(&rr));
        // NACK (RTPFB 205) must not trigger a keyframe
        let nack = [0x81, 205, 0, 3, 0, 0, 0, 1, 0, 0, 0, 2, 0, 5, 0, 0];
        assert!(!rtcp_requests_keyframe(&nack));
    }

    #[test]
    fn rtp_marker_bit_only_when_requested() {
        let a = build_rtp_packet(97, true, 1, 2, 3, &[9]);
        assert_eq!(a[1], 0x80 | 97);
        let b = build_rtp_packet(97, false, 1, 2, 3, &[9]);
        assert_eq!(b[1], 97);
        assert_eq!(b.len(), 13);
    }

    #[test]
    fn wire_pli_through_relay_header_forces_keyframe() {
        let pli = [0x81u8, 206, 0, 2, 0, 0, 0, 1, 0, 0, 0, 2];
        let mut w = vec![0x0D, 0x78, 0x56, 0x34, 0x12];
        w.extend_from_slice(&pli);
        assert!(wire_requests_keyframe(&w));
        let mut raw = Vec::new();
        raw.extend_from_slice(&pli);
        assert!(wire_requests_keyframe(&raw));
        // RTP video packet and ZRTP-like junk must not trigger
        let rtp = build_rtp_packet(97, true, 1, 2, 3, &[0x41, 0x9A]);
        assert!(!wire_requests_keyframe(&rtp));
        assert!(!wire_requests_keyframe(&[0x00, 0x0B, 1, 2, 3]));
    }

    #[test]
    fn rtcp_inbox_is_bounded_and_drains() {
        clear_rtcp_inbox();
        for i in 0..(RTCP_INBOX_MAX + 10) {
            push_rtcp_inbox(&[i as u8]);
        }
        let v = drain_rtcp_inbox();
        assert_eq!(v.len(), RTCP_INBOX_MAX);
        assert_eq!(v[0], vec![10u8]);
        assert!(drain_rtcp_inbox().is_empty());
    }

    fn au_of(len: usize) -> Vec<u8> {
        let mut au = annexb_au(&[H264_SPS, H264_PPS]);
        au.extend_from_slice(&[0, 0, 0, 1, 0x65]);
        au.extend((0..len as u32).map(|i| (i % 249) as u8 | 1));
        au
    }

    #[test]
    fn small_au_is_one_pt97_packet() {
        let au = annexb_au(&[H264_PSLICE]);
        let parts = packetize_zalo_au(&au, RTP_MAX_PAYLOAD);
        assert_eq!(parts, vec![(VIDEO_PT, au)]);
    }

    #[test]
    fn large_au_fragments_like_the_peer_and_roundtrips() {
        let au = au_of(2800);
        let parts = packetize_zalo_au(&au, RTP_MAX_PAYLOAD);
        assert!(parts.len() >= 3);
        // keyframe (has an IDR slice) -> PT 98 fragments, each within the payload limit
        assert!(parts.iter().all(|(pt, p)| *pt == VIDEO_PT_FRAG && p.len() <= RTP_MAX_PAYLOAD));
        // header bytes exactly as captured: 1c 80 / 1c 00 / 1c 40
        assert_eq!(&parts[0].1[..2], &[0x1C, 0x80]);
        assert_eq!(&parts[1].1[..2], &[0x1C, 0x00]);
        assert_eq!(&parts.last().unwrap().1[..2], &[0x1C, 0x40]);
        // even split: fragment sizes differ by at most 1 except possibly the last (peer: 1083/1083, 1062x3)
        let sizes: Vec<usize> = parts.iter().map(|(_, p)| p.len()).collect();
        let (mx, mn) = (sizes.iter().max().unwrap(), sizes.iter().min().unwrap());
        assert!(mx - mn <= 2, "uneven split {sizes:?}");
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);
        let mut buf = Vec::new();
        for (_, p) in &parts {
            depacketize_h264(p, &mut buf, &tx);
        }
        assert_eq!(rx.try_recv().expect("reassembled AU"), au);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn peer_capture_shapes_are_decoded() {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);
        let mut buf = Vec::new();
        // PT 97 payload as captured: whole AU, Annex-B, NAL 0x21
        let whole = [0, 0, 0, 1, 0x21, 0xE1, 0x04, 0x06, 0x38];
        depacketize_h264(&whole, &mut buf, &tx);
        assert_eq!(rx.try_recv().unwrap(), whole.to_vec());
        // PT 98 fragments as captured: 1c 80 00 00 00 01 27 42 ..., 1c 00 ..., 1c 40 ...
        depacketize_h264(&[0x1C, 0x80, 0, 0, 0, 1, 0x27, 0x42, 0x00], &mut buf, &tx);
        depacketize_h264(&[0x1C, 0x00, 0x06, 0xF1], &mut buf, &tx);
        assert!(rx.try_recv().is_err());
        depacketize_h264(&[0x1C, 0x40, 0xDE, 0x3B], &mut buf, &tx);
        assert_eq!(
            rx.try_recv().unwrap(),
            vec![0, 0, 0, 1, 0x27, 0x42, 0x00, 0x06, 0xF1, 0xDE, 0x3B]
        );
    }

    #[test]
    fn lost_first_fragment_drops_the_access_unit() {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);
        let mut buf = Vec::new();
        depacketize_h264(&[0x1C, 0x00, 1, 2], &mut buf, &tx);
        depacketize_h264(&[0x1C, 0x40, 3, 4], &mut buf, &tx);
        assert!(rx.try_recv().is_err());
        // a new access unit still works afterwards
        depacketize_h264(&[0x1C, 0xC0, 0, 0, 0, 1, 0x41], &mut buf, &tx);
        assert_eq!(rx.try_recv().unwrap(), vec![0, 0, 0, 1, 0x41]);
    }

    #[test]
    fn rfc6184_still_accepted_on_receive() {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);
        let mut buf = Vec::new();
        depacketize_h264(&[0x41, 0x9A, 0x24], &mut buf, &tx);
        assert_eq!(rx.try_recv().unwrap(), vec![0, 0, 0, 1, 0x41, 0x9A, 0x24]);
    }
}
