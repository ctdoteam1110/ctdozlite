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
        // 1-byte relay media header (`0x04` = incoming RTP, `0x07` = P2P RTP, `0x0E` = incoming RTCP)
        0x04 | 0x07 => pkt.get(1..)?,
        // 5-byte relay media header (`0x03` / `0x05` + `token: u32 LE`)
        0x03 | 0x05 => pkt.get(5..)?,
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
) {
    let host = cpal::default_host();
    let input_dev = match host.default_input_device() {
        Some(d) => d,
        None => { eprintln!("media: no input device"); return; }
    };
    let output_dev = match host.default_output_device() {
        Some(d) => d,
        None => { eprintln!("media: no output device"); return; }
    };

    // Request 16kHz mono config if available, else default
    let in_cfg = match input_dev.default_input_config() {
        Ok(c) => c,
        Err(e) => { eprintln!("media: input config err {e}"); return; }
    };
    let out_cfg = match output_dev.default_output_config() {
        Ok(c) => c,
        Err(e) => { eprintln!("media: output config err {e}"); return; }
    };

    // Shared buffers
    let input_buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(FRAME_SAMPLES * 4)));
    let output_buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(FRAME_SAMPLES * 4)));
    let ib = input_buf.clone();

    // Input stream: accumulate samples
    let input_stream = match input_dev.build_input_stream(
        &in_cfg.into(),
        move |data: &[f32], _| {
            if let Ok(mut b) = ib.lock() {
                b.extend_from_slice(data);
            }
        },
        |e| eprintln!("input stream err {e}"),
        None,
    ) {
        Ok(s) => s,
        Err(e) => { eprintln!("media: build input err {e}"); return; }
    };
    if let Err(e) = input_stream.play() { eprintln!("media: play input err {e}"); return; }

    // Output stream: play from output_buf
    let ob2 = output_buf.clone();
    let output_stream = match output_dev.build_output_stream(
        &out_cfg.into(),
        move |data: &mut [f32], _| {
            if let Ok(mut b) = ob2.lock() {
                let n = data.len().min(b.len());
                for (i, slot) in data.iter_mut().enumerate() {
                    *slot = if i < n { b[i] } else { 0.0 };
                }
                if n > 0 { b.drain(0..n); }
            } else {
                for s in data.iter_mut() { *s = 0.0; }
            }
        },
        |e| eprintln!("output stream err {e}"),
        None,
    ) {
        Ok(s) => s,
        Err(e) => { eprintln!("media: build output err {e}"); return; }
    };
    if let Err(e) = output_stream.play() { eprintln!("media: play output err {e}"); return; }

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

    while !stop.load(Ordering::Relaxed) {
        // --- Capture: take 320 samples, encode, send RTP ---
        let frame: Vec<f32> = {
            let mut b = input_buf.lock().unwrap();
            if b.len() >= FRAME_SAMPLES {
                b.drain(0..FRAME_SAMPLES).collect()
            } else {
                Vec::new()
            }
        };
        if frame.len() == FRAME_SAMPLES && !mic_muted.load(Ordering::Relaxed) {
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
                            // Wrap in Zalo's 5-byte relay media header (`0x00893355` / `0x00893E02` in `ZaloCall.exe`):
                            // `[type_byte (0x03 caller / 0x05 callee), token: u32 LE, rtp_pkt..]`
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

        // Periodic RTCP Receiver Report (PT=201) over `udp_rtcp_sock` (`0x00894418` in `ZaloCall.exe`)
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
        let recv_len = if let Some(ref tsock) = udp_rtp_sock {
            tsock.try_recv_from(&mut buf).ok().map(|(n, _)| n)
        } else if let Some(ref fsock) = fallback_sock {
            fsock.recv_from(&mut buf).ok().map(|(n, _)| n)
        } else {
            None
        };
        if let Some(n) = recv_len {
            if let Some(inner_rtp) = unwrap_zalo_relay_rtp(&buf[..n]) {
                if let Some(payload) = extract_rtp_opus_payload(inner_rtp) {
                    let mut pcm = vec![0f32; FRAME_SAMPLES];
                    if decoder.decode_float(payload, &mut pcm, false).is_ok() {
                        if let Ok(mut b) = output_buf.lock() {
                            b.extend_from_slice(&pcm);
                        }
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(5));
    }

    drop(input_stream);
    drop(output_stream);
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