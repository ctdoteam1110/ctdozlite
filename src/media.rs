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
        Self::start_with_sockets(rtp_addr, String::new(), 0, 0, None, None, local_port)
    }

    pub fn start_with_sockets(
        rtp_addr: String,
        rtcp_addr: String,
        from_uid_u32: u32,
        partner_uid_u32: u32,
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
                    // Build RTP header (12 bytes, PT=112 matching Zalo's opus/16000/1 codec)
                    let mut pkt = Vec::with_capacity(12 + n);
                    pkt.push(0x80); // V=2, P=0, X=0, CC=0
                    pkt.push(112); // payload type 112 (opus/16000/1)
                    pkt.extend_from_slice(&seq.to_be_bytes());
                    pkt.extend_from_slice(&ts.to_be_bytes());
                    pkt.extend_from_slice(&ssrc.to_be_bytes());
                    pkt.extend_from_slice(&out[..n]);
                    if let (Some(ref tsock), Some(target)) = (&udp_rtp_sock, rtp_sock_addr) {
                        let _ = tsock.try_send_to(&pkt, target);
                    } else if let Some(ref fsock) = fallback_sock {
                        let _ = fsock.send_to(&pkt, &rtp_addr);
                    }
                    seq = seq.wrapping_add(1);
                    ts = ts.wrapping_add(FRAME_SAMPLES as u32);
                }
                Err(e) => eprintln!("media: encode err {e}"),
            }
        }

        // Periodic RTCP Receiver Report (PT=201) over `udp_rtcp_sock` so the relay keeps the RTCP leg warm
        if last_rtcp.elapsed() >= Duration::from_secs(2) {
            last_rtcp = std::time::Instant::now();
            if let (Some(ref rtcp_s), Some(target)) = (&udp_rtcp_sock, rtcp_sock_addr) {
                let mut rtcp_pkt = [0u8; 32];
                rtcp_pkt[0] = 0x81; // V=2, RC=1
                rtcp_pkt[1] = 201; // PT=201 (Receiver Report)
                rtcp_pkt[2..4].copy_from_slice(&7u16.to_be_bytes());
                rtcp_pkt[4..8].copy_from_slice(&ssrc.to_be_bytes());
                rtcp_pkt[8..12].copy_from_slice(&partner_uid_u32.to_be_bytes());
                let _ = rtcp_s.try_send_to(&rtcp_pkt, target);
            }
        }

        // --- Receive: parse RTP, decode, play ---
        let mut buf = [0u8; 4000];
        let recv_len = if let Some(ref tsock) = udp_rtp_sock {
            tsock.try_recv_from(&mut buf).ok().map(|(n, _)| n)
        } else if let Some(ref fsock) = fallback_sock {
            fsock.recv_from(&mut buf).ok().map(|(n, _)| n)
        } else {
            None
        };
        if let Some(n) = recv_len {
            // Skip 21-byte Zalo ZRTP control packets (`buf[0] == 0x01 || buf[0] == 0x02`)
            if n > 12 && (buf[0] & 0xC0) == 0x80 {
                let payload = &buf[12..n];
                let mut pcm = vec![0f32; FRAME_SAMPLES];
                if decoder.decode_float(payload, &mut pcm, false).is_ok() {
                    if let Ok(mut b) = output_buf.lock() {
                        b.extend_from_slice(&pcm);
                    }
                }
            }
        }

        thread::sleep(Duration::from_millis(5));
    }

    drop(input_stream);
    drop(output_stream);
}