//! Native camera acquisition and in-process H.264 for calls.

pub mod camera;
pub mod codec;
pub mod decode_queue;
pub mod ingress;
pub mod session;

/// Embedded third-party video notices, available even in a single-file test build.
pub fn license_notices() -> &'static str {
    concat!(
        include_str!("../../packaging/licenses/OpenH264.txt"),
        "\n\n",
        include_str!("../../packaging/licenses/OpenH264-Rust.txt"),
        "\n\n",
        include_str!("../../packaging/licenses/Nokhwa.txt"),
    )
}

/// Exercise the bundled encoder and decoder without camera, session, network or tools.
///
/// # Errors
/// Returns an error if the native codec fails to produce a 320×240 frame.
pub fn self_test() -> anyhow::Result<serde_json::Value> {
    let pixels = vec![80_u8; 320 * 240 * 3];
    let started = std::time::Instant::now();
    let mut encoder = codec::VideoEncoder::new()?;
    let mut decoder = codec::VideoDecoder::new()?;
    let mut decoded = 0;
    let mut bytes = 0;
    for frame in 0..10 {
        let au = encoder.encode_rgb(&pixels, 320, 240, frame == 0 || frame == 5)?;
        bytes += au.len();
        if let Some(image) = decoder.decode(&au)? {
            anyhow::ensure!(
                image.width == 320 && image.height == 240,
                "unexpected native video dimensions"
            );
            decoded += 1;
        }
    }
    anyhow::ensure!(decoded > 0, "native codec produced no decoded video frames");
    Ok(serde_json::json!({
        "codec": "OpenH264 (bundled source)", "encoded_frames": 10,
        "decoded_frames": decoded, "width": 320, "height": 240,
        "encoded_bytes": bytes, "elapsed_ms": started.elapsed().as_millis(),
        "external_tools": false,
    }))
}
