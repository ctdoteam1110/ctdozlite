//! Native source-linked H.264 encoding and bounded decoding for call workers.

use anyhow::{bail, ensure, Context, Result};
use openh264::decoder::{Decoder, DecoderConfig};
use openh264::encoder::{
    BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, Level, Profile, RateControlMode,
};
use openh264::formats::{RgbSliceU8, YUVBuffer, YUVSource};
use openh264::OpenH264API;
use std::collections::BTreeMap;

const MAX_UNIT: usize = 1024 * 1024;
const WIDTH: usize = 320;
const HEIGHT: usize = 240;

/// A baseline 320×240, 15 fps H.264 encoder.
pub struct VideoEncoder {
    encoder: Encoder,
}
impl VideoEncoder {
    /// Creates the native encoder.
    /// # Errors
    /// Returns an error if OpenH264 initialization fails.
    pub fn new() -> Result<Self> {
        let config = EncoderConfig::new()
            .bitrate(BitRate::from_bps(260_000))
            .max_frame_rate(FrameRate::from_hz(15.0))
            .rate_control_mode(RateControlMode::Bitrate)
            .profile(Profile::Baseline)
            .level(Level::Level_3_0)
            .intra_frame_period(IntraFramePeriod::from_num_frames(15))
            .skip_frames(false)
            .num_threads(1);
        Ok(Self {
            encoder: Encoder::with_api_config(OpenH264API::from_source(), config)
                .context("initialize native H264 encoder")?,
        })
    }
    /// Fits packed RGB into a 4:3 frame and encodes one Annex-B access unit.
    /// # Errors
    /// Returns an error for invalid RGB dimensions or a native encoder failure.
    pub fn encode_rgb(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
        force_idr: bool,
    ) -> Result<Vec<u8>> {
        let (width, height) = (width as usize, height as usize);
        ensure!(
            width > 0 && height > 0 && width <= 1920 && height <= 1088,
            "invalid RGB dimensions"
        );
        let expected = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(3))
            .context("RGB dimensions overflow")?;
        ensure!(
            rgb.len() == expected,
            "RGB byte length does not match dimensions"
        );
        let (fit_width, fit_height) = if width * HEIGHT > height * WIDTH {
            (WIDTH, (height * WIDTH / width).max(1))
        } else {
            ((width * HEIGHT / height).max(1), HEIGHT)
        };
        let mut fitted = vec![0; WIDTH * HEIGHT * 3];
        let (left, top) = ((WIDTH - fit_width) / 2, (HEIGHT - fit_height) / 2);
        for y in 0..fit_height {
            for x in 0..fit_width {
                let src = ((y * height / fit_height) * width + x * width / fit_width) * 3;
                let dst = ((y + top) * WIDTH + x + left) * 3;
                fitted[dst..dst + 3].copy_from_slice(&rgb[src..src + 3]);
            }
        }
        let yuv = YUVBuffer::from_rgb8_source(RgbSliceU8::new(&fitted, (WIDTH, HEIGHT)));
        if force_idr {
            self.encoder.force_intra_frame();
        }
        let encoded = self
            .encoder
            .encode(&yuv)
            .context("encode native H264 frame")?
            .to_vec();
        let nals = annexb_nals(&encoded)?;
        let mut normalized = Vec::with_capacity(encoded.len());
        for nal in nals {
            // Match the existing Annex-B normalization: AUD and SEI are not sent.
            if matches!(nal[0] & 0x1f, 6 | 9) {
                continue;
            }
            normalized.extend_from_slice(&[0, 0, 0, 1]);
            normalized.extend_from_slice(nal);
        }
        ensure!(
            normalized.len() <= MAX_UNIT,
            "encoded H264 access unit exceeds limit"
        );
        Ok(normalized)
    }
}

/// One decoded frame with native visible dimensions and packed RGBA pixels.
pub struct DecodedFrame {
    /// Visible width in pixels.
    pub width: usize,
    /// Visible height in pixels.
    pub height: usize,
    /// Packed RGBA pixels; alpha is always opaque.
    pub rgba: Vec<u8>,
}

/// Validates parameter sets before allowing native decoder allocation.
pub struct VideoDecoder {
    decoder: Option<Decoder>,
    sps: BTreeMap<u32, (Vec<u8>, SpsInfo)>,
    pps: BTreeMap<u32, (Vec<u8>, u32)>,
    synchronized: bool,
}
impl VideoDecoder {
    /// Creates an unsynchronized decoder without allocating native frame buffers.
    /// # Errors
    /// Returns an error if native setup fails.
    pub fn new() -> Result<Self> {
        Ok(Self {
            decoder: None,
            sps: BTreeMap::new(),
            pps: BTreeMap::new(),
            synchronized: false,
        })
    }
    /// Drops the native decoder, parameter sets, and synchronization state.
    /// # Errors
    /// Returns an error if decoder reset fails.
    pub fn reset(&mut self) -> Result<()> {
        *self = Self::new()?;
        Ok(())
    }
    /// Decodes a bounded Annex-B unit, waiting for parameter sets and a valid IDR.
    /// # Errors
    /// Returns an error on invalid, oversized, or undecodable input.
    pub fn decode(&mut self, annexb: &[u8]) -> Result<Option<DecodedFrame>> {
        let result = self.decode_inner(annexb);
        if result.is_err() {
            self.decoder = None;
            self.synchronized = false;
        }
        result
    }

    fn decode_inner(&mut self, annexb: &[u8]) -> Result<Option<DecodedFrame>> {
        let nals = annexb_nals(annexb)?;
        // Subset SPS and MVC/SVC extensions must not bypass the dimension preflight.
        ensure!(
            nals.iter().all(|nal| matches!(nal[0] & 0x1f, 1 | 5..=12)),
            "unsupported H264 extension NAL"
        );
        // Validate every SPS before forwarding any byte to the native allocator.
        for nal in &nals {
            if nal[0] & 0x1f == 7 {
                ensure!(nal.len() <= 65536, "SPS exceeds limit");
                let info = inspect_sps(nal)?;
                if self.sps.get(&info.id).is_some_and(|old| old.0 != *nal) {
                    self.decoder = None;
                    self.synchronized = false;
                }
                let old_len = self.sps.get(&info.id).map_or(0, |old| old.0.len());
                ensure!(
                    self.cached_bytes() - old_len + nal.len() <= MAX_UNIT,
                    "parameter-set cache exceeds limit"
                );
                self.sps.insert(info.id, (nal.to_vec(), info));
            }
        }
        for nal in &nals {
            if nal[0] & 0x1f == 8 {
                ensure!(nal.len() <= 65536, "PPS exceeds limit");
                let rbsp = unescape(&nal[1..])?;
                let mut bits = Bits::new(&rbsp);
                let id = bits.ue()?;
                let sps_id = bits.ue()?;
                ensure!(id < 256 && sps_id < 32, "invalid parameter-set ID");
                if self.pps.get(&id).is_some_and(|old| old.0 != *nal) {
                    self.decoder = None;
                    self.synchronized = false;
                }
                let old_len = self.pps.get(&id).map_or(0, |old| old.0.len());
                ensure!(
                    self.cached_bytes() - old_len + nal.len() <= MAX_UNIT,
                    "parameter-set cache exceeds limit"
                );
                self.pps.insert(id, (nal.to_vec(), sps_id));
            }
        }
        ensure!(
            self.sps
                .values()
                .map(|x| x.0.len())
                .chain(self.pps.values().map(|x| x.0.len()))
                .sum::<usize>()
                <= MAX_UNIT,
            "parameter-set cache exceeds limit"
        );
        let mut has_slice = false;
        let mut has_idr = false;
        let mut expected = None;
        for nal in &nals {
            if matches!(nal[0] & 0x1f, 1 | 5) {
                has_slice = true;
                has_idr |= nal[0] & 0x1f == 5;
                // Only the first three Exp-Golomb fields are needed before native decoding.
                let rbsp = unescape_prefix(&nal[1..], 64)?;
                let mut bits = Bits::new(&rbsp);
                bits.ue()?;
                let slice_type = bits.ue()?;
                ensure!(slice_type <= 9, "invalid H264 slice type");
                let pps_id = bits.ue()?;
                let Some((_, sps_id)) = self.pps.get(&pps_id) else {
                    return Ok(None);
                };
                let Some((_, info)) = self.sps.get(sps_id) else {
                    return Ok(None);
                };
                ensure!(
                    expected.is_none_or(|dim| dim == (info.width, info.height)),
                    "mixed video dimensions in access unit"
                );
                expected = Some((info.width, info.height));
            }
        }
        if !has_slice || (!self.synchronized && !has_idr) {
            return Ok(None);
        }
        let prepend_sets = self.decoder.is_none() || has_idr;
        let set_bytes = if prepend_sets {
            self.sps
                .values()
                .map(|x| x.0.len() + 4)
                .chain(self.pps.values().map(|x| x.0.len() + 4))
                .sum()
        } else {
            0
        };
        let slice_bytes: usize = nals
            .iter()
            .filter(|nal| !matches!(nal[0] & 0x1f, 7 | 8))
            .map(|nal| nal.len() + 4)
            .sum();
        ensure!(
            set_bytes + slice_bytes <= MAX_UNIT,
            "H264 unit exceeds limit"
        );
        let mut packet = Vec::with_capacity(set_bytes + slice_bytes);
        if prepend_sets {
            for nal in self
                .sps
                .values()
                .map(|x| &x.0)
                .chain(self.pps.values().map(|x| &x.0))
            {
                ensure!(
                    packet.len() + 4 + nal.len() <= MAX_UNIT,
                    "parameter sets exceed unit limit"
                );
                packet.extend_from_slice(&[0, 0, 0, 1]);
                packet.extend_from_slice(nal);
            }
        }
        for nal in nals {
            if matches!(nal[0] & 0x1f, 7 | 8) {
                continue;
            }
            ensure!(
                packet.len() + 4 + nal.len() <= MAX_UNIT,
                "H264 unit exceeds limit"
            );
            packet.extend_from_slice(&[0, 0, 0, 1]);
            packet.extend_from_slice(nal);
        }
        if self.decoder.is_none() {
            self.decoder = Some(
                Decoder::with_api_config(OpenH264API::from_source(), DecoderConfig::new())
                    .context("initialize native H264 decoder")?,
            );
        }
        let decoder = self.decoder.as_mut().context("missing H264 decoder")?;
        let Some(yuv) = decoder
            .decode(&packet)
            .context("decode native H264 frame")?
        else {
            return Ok(None);
        };
        let (width, height) = yuv.dimensions();
        ensure!(
            width > 0
                && height > 0
                && width <= 1920
                && height <= 1080
                && expected == Some((width, height)),
            "invalid decoded video dimensions"
        );
        let (sy, su, sv) = yuv.strides();
        let chroma_width = width.div_ceil(2);
        let chroma_height = height.div_ceil(2);
        ensure!(
            sy >= width
                && su >= chroma_width
                && sv >= chroma_width
                && sy <= 4096
                && su <= 4096
                && sv <= 4096,
            "invalid decoded video strides"
        );
        ensure!(
            yuv.y().len() >= sy * height
                && yuv.u().len() >= su * chroma_height
                && yuv.v().len() >= sv * chroma_height,
            "truncated decoded video planes"
        );
        let mut rgba = vec![0; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let luma = i32::from(yuv.y()[y * sy + x]) - 16;
                let u = i32::from(yuv.u()[(y / 2) * su + x / 2]) - 128;
                let v = i32::from(yuv.v()[(y / 2) * sv + x / 2]) - 128;
                let dst = (y * width + x) * 4;
                rgba[dst] = ((298 * luma + 409 * v + 128) >> 8).clamp(0, 255) as u8;
                rgba[dst + 1] = ((298 * luma - 100 * u - 208 * v + 128) >> 8).clamp(0, 255) as u8;
                rgba[dst + 2] = ((298 * luma + 516 * u + 128) >> 8).clamp(0, 255) as u8;
                rgba[dst + 3] = 255;
            }
        }
        self.synchronized = true;
        Ok(Some(DecodedFrame {
            width,
            height,
            rgba,
        }))
    }

    fn cached_bytes(&self) -> usize {
        self.sps
            .values()
            .map(|x| x.0.len())
            .chain(self.pps.values().map(|x| x.0.len()))
            .sum()
    }
}

#[derive(Clone, Copy)]
struct SpsInfo {
    id: u32,
    width: usize,
    height: usize,
}

fn annexb_nals(bytes: &[u8]) -> Result<Vec<&[u8]>> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_UNIT,
        "invalid H264 access-unit size"
    );
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let prefix = if bytes[i..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if bytes[i..].starts_with(&[0, 0, 1]) {
            3
        } else {
            i += 1;
            continue;
        };
        ensure!(starts.len() < 1024, "too many H264 NAL units");
        starts.push((i, i + prefix));
        i += prefix;
    }
    ensure!(
        starts
            .first()
            .is_some_and(|x| bytes[..x.0].iter().all(|b| *b == 0)),
        "missing Annex-B start code"
    );
    let mut nals = Vec::with_capacity(starts.len());
    for (index, (_, begin)) in starts.iter().enumerate() {
        let mut end = starts.get(index + 1).map_or(bytes.len(), |x| x.0);
        while end > *begin && bytes[end - 1] == 0 {
            end -= 1;
        }
        ensure!(end > *begin, "empty H264 NAL unit");
        let nal = &bytes[*begin..end];
        ensure!(
            nal[0] & 0x80 == 0 && (1..=23).contains(&(nal[0] & 0x1f)),
            "invalid H264 NAL header"
        );
        nals.push(nal);
    }
    Ok(nals)
}

fn unescape(bytes: &[u8]) -> Result<Vec<u8>> {
    unescape_prefix(bytes, bytes.len())
}

fn unescape_prefix(bytes: &[u8], limit: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(bytes.len().min(limit));
    let mut zeros = 0;
    let mut i = 0;
    while i < bytes.len() && out.len() < limit {
        let byte = bytes[i];
        if zeros >= 2 && byte == 3 {
            ensure!(
                bytes.get(i + 1).is_none_or(|n| *n <= 3),
                "invalid H264 emulation prevention"
            );
            zeros = 0;
            i += 1;
            continue;
        }
        out.push(byte);
        zeros = if byte == 0 { zeros + 1 } else { 0 };
        i += 1;
    }
    Ok(out)
}

struct Bits<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Bits<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn read(&mut self, count: usize) -> Result<u32> {
        ensure!(
            count <= 32 && self.offset + count <= self.bytes.len() * 8,
            "truncated H264 bitstream"
        );
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1)
                | u32::from((self.bytes[self.offset / 8] >> (7 - self.offset % 8)) & 1);
            self.offset += 1;
        }
        Ok(value)
    }
    fn ue(&mut self) -> Result<u32> {
        let mut zeros = 0;
        while self.read(1)? == 0 {
            zeros += 1;
            ensure!(zeros <= 31, "H264 Exp-Golomb overflow");
        }
        Ok(((1u64 << zeros) - 1 + u64::from(self.read(zeros)?)) as u32)
    }
    fn se(&mut self) -> Result<i64> {
        let code = i64::from(self.ue()?);
        Ok(if code & 1 == 0 {
            -(code / 2)
        } else {
            (code + 1) / 2
        })
    }
}

fn inspect_sps(nal: &[u8]) -> Result<SpsInfo> {
    let rbsp = unescape(&nal[1..])?;
    let mut bits = Bits::new(&rbsp);
    let profile = bits.read(8)?;
    bits.read(8)?;
    bits.read(8)?;
    let id = bits.ue()?;
    ensure!(id < 32, "invalid SPS ID");
    if matches!(
        profile,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        ensure!(bits.ue()? == 1, "only 4:2:0 H264 is supported");
        ensure!(
            bits.ue()? == 0 && bits.ue()? == 0,
            "only 8-bit H264 is supported"
        );
        bits.read(1)?;
        if bits.read(1)? != 0 {
            for index in 0..8 {
                if bits.read(1)? != 0 {
                    let mut last = 8i64;
                    let mut next = 8i64;
                    for _ in 0..if index < 6 { 16 } else { 64 } {
                        if next != 0 {
                            next = (last + bits.se()?).rem_euclid(256);
                        }
                        if next != 0 {
                            last = next;
                        }
                    }
                }
            }
        }
    } else {
        ensure!(matches!(profile, 66 | 77 | 88), "unsupported H264 profile");
    }
    ensure!(bits.ue()? <= 12, "invalid H264 frame number size");
    match bits.ue()? {
        0 => {
            ensure!(bits.ue()? <= 12, "invalid H264 POC size");
        }
        1 => {
            bits.read(1)?;
            bits.se()?;
            bits.se()?;
            let count = bits.ue()?;
            ensure!(count <= 255, "invalid H264 POC cycle");
            for _ in 0..count {
                bits.se()?;
            }
        }
        2 => {}
        _ => bail!("invalid H264 POC type"),
    }
    ensure!(bits.ue()? <= 16, "too many H264 reference frames");
    bits.read(1)?;
    let mb_width = u64::from(bits.ue()?) + 1;
    let map_height = u64::from(bits.ue()?) + 1;
    let frame_only = bits.read(1)?;
    let factor = 2 - u64::from(frame_only);
    if frame_only == 0 {
        bits.read(1)?;
    }
    bits.read(1)?;
    let (mut crop_x, mut crop_y) = (0u64, 0u64);
    if bits.read(1)? != 0 {
        crop_x = (u64::from(bits.ue()?) + u64::from(bits.ue()?)) * 2;
        crop_y = (u64::from(bits.ue()?) + u64::from(bits.ue()?)) * 2 * factor;
    }
    bits.read(1)?;
    let coded_width = mb_width * 16;
    let coded_height = map_height * 16 * factor;
    ensure!(
        coded_width <= 1920
            && coded_height <= 1088
            && crop_x < coded_width
            && crop_y < coded_height,
        "H264 coded dimensions exceed limit"
    );
    let width = (coded_width - crop_x) as usize;
    let height = (coded_height - crop_y) as usize;
    ensure!(
        width <= 1920 && height <= 1080,
        "H264 visible dimensions exceed limit"
    );
    Ok(SpsInfo { id, width, height })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_header_unescape_ignores_coded_data_and_accepts_terminal_prevention() {
        assert_eq!(unescape(&[0, 0, 3]).unwrap(), vec![0, 0]);
        assert!(unescape(&[0, 0, 3, 4]).is_err());
        let mut nal = vec![1; 1024 * 1024];
        nal.extend_from_slice(&[0, 0, 3, 4]);
        assert_eq!(unescape_prefix(&nal, 64).unwrap(), vec![1; 64]);
    }

    fn red_rgb(width: usize, height: usize) -> Vec<u8> {
        [240, 20, 20].repeat(width * height)
    }

    #[test]
    fn native_roundtrip_preserves_dimensions_color_and_black_content() {
        let mut rgb = red_rgb(320, 240);
        for y in 0..240 {
            for x in 0..80 {
                rgb[(y * 320 + x) * 3..(y * 320 + x) * 3 + 3].fill(0);
            }
        }
        let encoded = VideoEncoder::new()
            .unwrap()
            .encode_rgb(&rgb, 320, 240, true)
            .unwrap();
        let frame = VideoDecoder::new()
            .unwrap()
            .decode(&encoded)
            .unwrap()
            .expect("actual native frame");
        assert_eq!(
            (frame.width, frame.height, frame.rgba.len()),
            (320, 240, 320 * 240 * 4)
        );
        let red = &frame.rgba[(120 * 320 + 160) * 4..][..4];
        assert!(
            red[0] > 190 && red[1] < 55 && red[2] < 55 && red[3] == 255,
            "{red:?}"
        );
        assert!(frame.rgba[(120 * 320 + 20) * 4] < 20);
    }

    #[test]
    fn scales_non_four_three_input_with_black_padding() {
        let encoded = VideoEncoder::new()
            .unwrap()
            .encode_rgb(&red_rgb(640, 360), 640, 360, true)
            .unwrap();
        let frame = VideoDecoder::new()
            .unwrap()
            .decode(&encoded)
            .unwrap()
            .unwrap();
        assert!(frame.rgba[(10 * 320 + 160) * 4] < 20);
        assert!(frame.rgba[(120 * 320 + 160) * 4] > 190);
        assert!(frame.rgba[(230 * 320 + 160) * 4] < 20);
    }

    #[test]
    fn forced_idr_contains_fresh_parameter_sets_and_decodes_independently() {
        let mut encoder = VideoEncoder::new().unwrap();
        encoder
            .encode_rgb(&red_rgb(320, 240), 320, 240, false)
            .unwrap();
        encoder
            .encode_rgb(&red_rgb(320, 240), 320, 240, false)
            .unwrap();
        let forced = encoder
            .encode_rgb(&[20, 220, 20].repeat(320 * 240), 320, 240, true)
            .unwrap();
        let frame = VideoDecoder::new()
            .unwrap()
            .decode(&forced)
            .unwrap()
            .unwrap();
        assert!(frame.rgba[(120 * 320 + 160) * 4 + 1] > 170);
    }

    #[test]
    fn rejects_invalid_rgb_and_compressed_input() {
        let mut encoder = VideoEncoder::new().unwrap();
        assert!(encoder.encode_rgb(&[], 0, 240, false).is_err());
        assert!(encoder.encode_rgb(&[0; 3], 320, 240, false).is_err());
        let mut decoder = VideoDecoder::new().unwrap();
        assert!(decoder.decode(&vec![0; 1024 * 1024 + 1]).is_err());
        assert!(decoder.decode(&[0, 0, 1, 0x67, 0xff]).is_err());
        assert!(decoder.decode(&[0, 0, 1, 0x65, 0xff]).unwrap().is_none());
    }

    #[test]
    fn accepts_parameter_sets_delivered_separately_before_idr() {
        let encoded = VideoEncoder::new()
            .unwrap()
            .encode_rgb(&red_rgb(320, 240), 320, 240, true)
            .unwrap();
        let nals = annexb_nals(&encoded).unwrap();
        let mut decoder = VideoDecoder::new().unwrap();
        let mut slices = Vec::new();
        for nal in nals {
            let mut unit = vec![0, 0, 0, 1];
            unit.extend_from_slice(nal);
            if matches!(nal[0] & 0x1f, 7 | 8) {
                assert!(decoder.decode(&unit).unwrap().is_none());
            } else {
                slices.extend(unit);
            }
        }
        assert!(decoder.decode(&slices).unwrap().is_some());
        decoder.reset().unwrap();
        assert!(decoder.decode(&slices).unwrap().is_none());
    }

    #[test]
    fn decodes_access_unit_with_multiple_idr_slices() {
        let config = EncoderConfig::new()
            .profile(Profile::Baseline)
            .max_frame_rate(FrameRate::from_hz(15.0))
            .rate_control_mode(RateControlMode::Off)
            .skip_frames(false)
            .num_threads(1)
            .max_slice_len(600);
        let mut encoder = Encoder::with_api_config(OpenH264API::from_source(), config).unwrap();
        let rgb: Vec<u8> = (0..320 * 240 * 3)
            .map(|i| ((i * 73 + i / 17) % 256) as u8)
            .collect();
        let yuv = YUVBuffer::from_rgb8_source(RgbSliceU8::new(&rgb, (320, 240)));
        let encoded = encoder.encode(&yuv).unwrap().to_vec();
        assert!(
            annexb_nals(&encoded)
                .unwrap()
                .iter()
                .filter(|nal| nal[0] & 0x1f == 5)
                .count()
                > 1
        );
        let frame = VideoDecoder::new()
            .unwrap()
            .decode(&encoded)
            .unwrap()
            .unwrap();
        assert_eq!((frame.width, frame.height), (320, 240));
    }

    // Synthetic baseline SPS fixtures encode only the fields needed for dimension bounds.
    fn baseline_sps(mb_width: u32, mb_height: u32, crop_bottom: u32) -> Vec<u8> {
        fn ue(bits: &mut Vec<bool>, value: u32) {
            let value = value + 1;
            let count = 32 - value.leading_zeros();
            bits.extend(std::iter::repeat_n(false, (count - 1) as usize));
            for shift in (0..count).rev() {
                bits.push(value & (1 << shift) != 0);
            }
        }
        let mut bits = Vec::new();
        ue(&mut bits, 0); // SPS ID
        ue(&mut bits, 0); // log2_max_frame_num_minus4
        ue(&mut bits, 0);
        ue(&mut bits, 0); // POC type, log2_max_pic_order_cnt_lsb_minus4
        ue(&mut bits, 1);
        bits.push(false); // one reference, no frame gaps
        ue(&mut bits, mb_width - 1);
        ue(&mut bits, mb_height - 1);
        bits.extend([true, true, crop_bottom > 0]);
        if crop_bottom > 0 {
            for crop in [0, 0, 0, crop_bottom] {
                ue(&mut bits, crop);
            }
        }
        bits.extend([false, true]); // no VUI, RBSP stop bit
        while bits.len() % 8 != 0 {
            bits.push(false);
        }
        let mut nal = vec![0x67, 66, 0xc0, 30];
        for chunk in bits.chunks_exact(8) {
            nal.push(
                chunk
                    .iter()
                    .fold(0u8, |acc, bit| (acc << 1) | u8::from(*bit)),
            );
        }
        let mut unit = vec![0, 0, 0, 1];
        unit.extend(nal);
        unit
    }

    #[test]
    fn rejects_oversized_sps_before_native_decoder_is_created() {
        let mut decoder = VideoDecoder::new().unwrap();
        assert!(decoder.decode(&baseline_sps(120, 68, 4)).unwrap().is_none()); // 1920×1080 visible
        assert!(decoder.decode(&baseline_sps(121, 68, 4)).is_err());
        assert!(decoder.decode(&baseline_sps(120, 69, 12)).is_err());
        assert!(decoder.decode(&baseline_sps(120, 68, 0)).is_err()); // 1088 visible
        assert!(decoder.decoder.is_none());
    }

    #[test]
    fn handles_malformed_annexb_without_panicking() {
        let mut decoder = VideoDecoder::new().unwrap();
        for unit in [
            &[][..],
            &[0, 0, 1][..],
            &[0, 0, 1, 0xe7, 1][..],
            &[0, 0, 1, 0x67, 0, 0, 3][..],
            &[0, 0, 1, 0x67, 66, 0, 30, 0][..],
            &[0, 0, 1, 0x6f, 66, 0, 30, 1][..],
        ] {
            assert!(decoder.decode(unit).is_err());
        }
    }
}
