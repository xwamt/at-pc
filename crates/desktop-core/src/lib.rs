//! Pure, in-memory desktop-frame processing shared by the agent and benchmarks.

use image::codecs::jpeg::JpegEncoder;
use image::{RgbImage, RgbaImage};
use std::io::Cursor;
use std::time::Duration;

/// Default stream output width. Sources wider than `target_width(scale)` are
/// scaled down continuously so 1920 and 1921 no longer sit on a half-scale cliff.
pub const DEFAULT_TARGET_WIDTH: u32 = 1280;

/// Output size after applying protocol `scale` and the default target width cap.
/// Dimensions are even (JPEG/YUV friendly) and never upscaled.
pub fn stream_output_size(orig_w: u32, orig_h: u32, scale: f32) -> (u32, u32) {
    if orig_w == 0 || orig_h == 0 {
        return (0, 0);
    }
    let scale = if scale.is_finite() && scale > 0.0 {
        scale.min(4.0)
    } else {
        1.0
    };
    let target_w = ((DEFAULT_TARGET_WIDTH as f32) * scale).round() as u32;
    let target_w = target_w.max(1);

    let (out_w, out_h) = if orig_w <= target_w {
        (orig_w, orig_h)
    } else {
        let out_w = target_w.min(orig_w);
        let out_h =
            ((f64::from(orig_h) * f64::from(out_w) / f64::from(orig_w)).round() as u32).max(1);
        (out_w, out_h)
    };
    (even_dim(out_w, orig_w), even_dim(out_h, orig_h))
}

fn even_dim(value: u32, orig: u32) -> u32 {
    if value <= 1 {
        value.min(orig)
    } else {
        (value & !1).min(orig).max(2.min(orig))
    }
}

/// Converts RGBA pixels to RGB, applying default (`scale = 1.0`) output sizing.
pub fn fast_rgba_to_rgb(rgba: &RgbaImage) -> (RgbImage, u32, u32) {
    fast_rgba_to_rgb_scaled(rgba, 1.0)
}

/// Converts RGBA pixels to RGB using protocol `scale` and nearest-neighbor
/// resampling to [`stream_output_size`].
pub fn fast_rgba_to_rgb_scaled(rgba: &RgbaImage, scale: f32) -> (RgbImage, u32, u32) {
    let orig_w = rgba.width();
    let orig_h = rgba.height();
    let (target_w, target_h) = stream_output_size(orig_w, orig_h, scale);
    let raw = rgba.as_raw();
    let src_stride = (orig_w as usize).saturating_mul(4);

    let mut rgb_data = Vec::with_capacity((target_w as usize) * (target_h as usize) * 3);
    if target_w == orig_w && target_h == orig_h {
        for chunk in raw.chunks_exact(4) {
            rgb_data.extend_from_slice(&chunk[..3]);
        }
    } else if target_w > 0 && target_h > 0 {
        for y in 0..target_h {
            let src_y = (u64::from(y) * u64::from(orig_h) / u64::from(target_h)) as u32;
            let row_offset = (src_y as usize).saturating_mul(src_stride);
            for x in 0..target_w {
                let src_x = (u64::from(x) * u64::from(orig_w) / u64::from(target_w)) as u32;
                let pixel_offset = row_offset + (src_x as usize) * 4;
                rgb_data.extend_from_slice(&raw[pixel_offset..pixel_offset + 3]);
            }
        }
    }

    let rgb_img = RgbImage::from_raw(target_w, target_h, rgb_data)
        .unwrap_or_else(|| image::DynamicImage::ImageRgba8(rgba.clone()).into_rgb8());
    (rgb_img, target_w, target_h)
}

/// Macroblock size for dirty-frame detection.
pub const BLOCK_SIZE: u32 = 64;

/// Changed-block ratio at or above this value is treated as a new frame.
/// Clock / cursor flicker typically dirties 1–2 of ~510 blocks on 1080p (~0.2–0.4%).
pub const DIRTY_BLOCK_RATIO_THRESHOLD: f64 = 0.005;

/// 64-bit word-wise hashes of each `BLOCK_SIZE`×`BLOCK_SIZE` tile in row-major order.
pub fn compute_block_hashes(raw_rgba: &[u8], width: u32, height: u32) -> Vec<u64> {
    compute_block_hashes_sized(raw_rgba, width, height, BLOCK_SIZE)
}

/// 64-bit word-wise hashes of each `block_size`×`block_size` tile in row-major order.
pub fn compute_block_hashes_sized(
    raw_rgba: &[u8],
    width: u32,
    height: u32,
    block_size: u32,
) -> Vec<u64> {
    if width == 0 || height == 0 || block_size == 0 {
        return Vec::new();
    }
    let cols = width.div_ceil(block_size);
    let rows = height.div_ceil(block_size);
    let stride = (width as usize).saturating_mul(4);
    let mut hashes = Vec::with_capacity((cols * rows) as usize);

    const MULTIPLIER: u64 = 0x9e3779b97f4a7c15;

    for by in 0..rows {
        let y0 = by * block_size;
        let y1 = (y0 + block_size).min(height);
        for bx in 0..cols {
            let x0 = bx * block_size;
            let x1 = (x0 + block_size).min(width);
            let mut hash: u64 = 0xcbf29ce484222325;
            for y in y0..y1 {
                let row = (y as usize).saturating_mul(stride);
                let start = row + (x0 as usize) * 4;
                let end = row + (x1 as usize) * 4;
                if end > raw_rgba.len() {
                    break;
                }
                let mut chunk = &raw_rgba[start..end];
                // 8 bytes at a time
                while chunk.len() >= 8 {
                    let word = u64::from_le_bytes(chunk[..8].try_into().unwrap());
                    hash = hash.rotate_left(13) ^ word.wrapping_mul(MULTIPLIER);
                    chunk = &chunk[8..];
                }
                // Remaining trailing bytes (<8 bytes)
                for &byte in chunk {
                    hash = hash.rotate_left(13) ^ (u64::from(byte)).wrapping_mul(MULTIPLIER);
                }
                // Rotate by 1 bit at row boundary to prevent multi-row cancellation
                // for symmetric 32-bit pixel pairs (32 words * 13 mod 64 = 32).
                hash = hash.rotate_left(1);
            }
            hashes.push(hash);
        }
    }
    hashes
}

/// Fraction of tiles whose hash differs. Empty previous or a length mismatch
/// is treated as a full change (first frame / resize).
pub fn dirty_block_ratio(previous: &[u64], current: &[u64]) -> f64 {
    if previous.is_empty() || current.is_empty() || previous.len() != current.len() {
        return 1.0;
    }
    let changed = previous
        .iter()
        .zip(current.iter())
        .filter(|(a, b)| a != b)
        .count();
    changed as f64 / current.len() as f64
}

/// True when enough tiles changed to justify a full re-encode.
pub fn is_frame_dirty(previous: &[u64], current: &[u64]) -> bool {
    dirty_block_ratio(previous, current) >= DIRTY_BLOCK_RATIO_THRESHOLD
}

/// Computes the legacy sampling hash used by the desktop stream's dirty-frame check.
#[deprecated(note = "Replaced by word-wise compute_block_hashes for tile-based dirty checking")]
pub fn compute_sample_hash(raw_rgba: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for chunk in raw_rgba.chunks(256) {
        if let Some(&byte) = chunk.first() {
            hash = hash.wrapping_mul(0x100000001b3) ^ u64::from(byte);
        }
    }
    hash
}

/// Idle desktop streams send a lightweight (non-JPEG) keepalive at this interval.
pub const STREAM_KEEPALIVE_INTERVAL: Duration = Duration::from_millis(500);

/// What the stream loop should do with a captured frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameSendDecision {
    /// Unchanged and keepalive is not due: skip convert, encode, and send.
    Skip,
    /// Unchanged but keepalive is due: send a header-only frame, skip JPEG.
    Keepalive,
    /// Enough tiles changed: convert + JPEG encode + send.
    Encode,
}

/// Decides skip / keepalive / encode. Dirty frames always encode; unchanged
/// frames never JPEG-encode and only keepalive after `keepalive_interval`.
pub fn decide_frame_send(
    is_dirty: bool,
    elapsed_since_send: Duration,
    keepalive_interval: Duration,
) -> FrameSendDecision {
    if is_dirty {
        FrameSendDecision::Encode
    } else if elapsed_since_send >= keepalive_interval {
        FrameSendDecision::Keepalive
    } else {
        FrameSendDecision::Skip
    }
}

/// Returns whether a frame should be encoded and sent.
///
/// Changed frames are always sent. Static frames are skipped until the
/// keepalive interval has elapsed; the exact boundary is sent.
pub fn should_send_frame(
    current_hash: u64,
    last_hash: u64,
    elapsed_since_send: Duration,
    keepalive_interval: Duration,
) -> bool {
    decide_frame_send(
        current_hash != last_hash,
        elapsed_since_send,
        keepalive_interval,
    ) != FrameSendDecision::Skip
}

/// Encodes an RGB frame with the same allocation and encoder used by the agent.
pub fn encode_jpeg(rgb: &RgbImage, quality: u8) -> image::ImageResult<Vec<u8>> {
    let mut jpeg_bytes = Vec::with_capacity((rgb.width() * rgb.height() / 5) as usize);
    let mut cursor = Cursor::new(&mut jpeg_bytes);
    let encoder = JpegEncoder::new_with_quality(&mut cursor, quality);
    rgb.write_with_encoder(encoder)?;
    Ok(jpeg_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, Rgba};

    #[test]
    fn rgba_conversion_drops_alpha_without_scaling_at_1920_or_below() {
        let source = RgbaImage::from_raw(
            2,
            2,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        )
        .unwrap();

        let (rgb, width, height) = fast_rgba_to_rgb(&source);

        assert_eq!((width, height), (2, 2));
        assert_eq!(rgb.as_raw(), &[1, 2, 3, 5, 6, 7, 9, 10, 11, 13, 14, 15]);
    }

    #[test]
    fn stream_output_size_keeps_sources_at_or_below_target_width() {
        assert_eq!(stream_output_size(800, 600, 1.0), (800, 600));
        assert_eq!(stream_output_size(1280, 720, 1.0), (1280, 720));
        assert_eq!(stream_output_size(801, 601, 1.0), (800, 600));
    }

    #[test]
    fn stream_output_size_uses_continuous_target_width_not_1920_cliff() {
        let (w1920, h1920) = stream_output_size(1920, 1080, 1.0);
        let (w1921, h1921) = stream_output_size(1921, 1080, 1.0);
        assert_eq!((w1920, h1920), (1280, 720));
        assert_eq!(w1921, 1280);
        assert_eq!(h1921 % 2, 0);
        let pixels_1920 = u64::from(w1920) * u64::from(h1920);
        let pixels_1921 = u64::from(w1921) * u64::from(h1921);
        let max = pixels_1920.max(pixels_1921);
        let min = pixels_1920.min(pixels_1921);
        assert!(
            max * 100 < min * 110,
            "1920 vs 1921 output pixels must stay within 10% ({pixels_1920} vs {pixels_1921})"
        );
    }

    #[test]
    fn stream_output_size_applies_protocol_scale_to_target_width() {
        assert_eq!(stream_output_size(3840, 2160, 1.0), (1280, 720));
        assert_eq!(stream_output_size(3840, 2160, 0.5), (640, 360));
        assert_eq!(stream_output_size(3840, 2160, 2.0), (2560, 1440));
        assert_eq!(
            stream_output_size(1920, 1080, 0.0),
            stream_output_size(1920, 1080, 1.0)
        );
        assert_eq!(
            stream_output_size(1920, 1080, f32::NAN),
            stream_output_size(1920, 1080, 1.0)
        );
    }

    #[test]
    fn rgba_conversion_nearest_neighbor_follows_output_size() {
        let source = RgbaImage::from_fn(1920, 4, |x, y| Rgba([x as u8, y as u8, 9, 255]));
        let (expected_w, expected_h) = stream_output_size(1920, 4, 1.0);
        let (rgb, width, height) = fast_rgba_to_rgb_scaled(&source, 1.0);
        assert_eq!((width, height), (expected_w, expected_h));
        assert_eq!(rgb.get_pixel(0, 0).0, [0, 0, 9]);
        let src_x = (expected_w - 1) as u64 * 1920 / expected_w as u64;
        assert_eq!(
            rgb.get_pixel(expected_w - 1, 0).0[0],
            source.get_pixel(src_x as u32, 0).0[0]
        );
    }

    #[test]
    #[allow(deprecated)]
    fn sample_hash_is_deterministic_and_observes_each_sampled_byte() {
        let mut bytes = vec![0_u8; 513];
        let initial = compute_sample_hash(&bytes);
        assert_eq!(initial, compute_sample_hash(&bytes));

        bytes[256] = 1;
        assert_ne!(initial, compute_sample_hash(&bytes));

        let sampled_change = compute_sample_hash(&bytes);
        bytes[255] = 99;
        assert_eq!(sampled_change, compute_sample_hash(&bytes));
    }

    #[test]
    fn frame_decision_covers_changed_static_and_keepalive_boundary() {
        let keepalive = Duration::from_secs(1);
        assert!(should_send_frame(2, 1, Duration::from_millis(0), keepalive));
        assert!(!should_send_frame(
            1,
            1,
            Duration::from_millis(999),
            keepalive
        ));
        assert!(should_send_frame(1, 1, keepalive, keepalive));
        assert!(should_send_frame(
            1,
            1,
            Duration::from_millis(1001),
            keepalive
        ));
    }

    #[test]
    fn idle_keepalive_skips_jpeg_and_fires_at_interval() {
        let keepalive = STREAM_KEEPALIVE_INTERVAL;
        assert_eq!(
            decide_frame_send(true, Duration::ZERO, keepalive),
            FrameSendDecision::Encode
        );
        assert_eq!(
            decide_frame_send(false, Duration::from_millis(499), keepalive),
            FrameSendDecision::Skip
        );
        assert_eq!(
            decide_frame_send(false, keepalive, keepalive),
            FrameSendDecision::Keepalive
        );
        assert_eq!(
            decide_frame_send(true, keepalive, keepalive),
            FrameSendDecision::Encode
        );
        assert!(!should_send_frame(
            1,
            1,
            Duration::from_millis(250),
            keepalive
        ));
    }

    fn solid_rgba(width: u32, height: u32, pixel: [u8; 4]) -> (Vec<u8>, u32, u32) {
        let mut raw = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            raw.extend_from_slice(&pixel);
        }
        (raw, width, height)
    }

    fn paint_rect(raw: &mut [u8], width: u32, x0: u32, y0: u32, rw: u32, rh: u32, pixel: [u8; 4]) {
        for y in y0..y0 + rh {
            for x in x0..x0 + rw {
                let i = ((y * width + x) * 4) as usize;
                raw[i..i + 4].copy_from_slice(&pixel);
            }
        }
    }

    #[test]
    fn identical_frames_have_zero_dirty_ratio() {
        let (raw, w, h) = solid_rgba(1920, 1080, [10, 20, 30, 255]);
        let first = compute_block_hashes(&raw, w, h);
        let second = compute_block_hashes(&raw, w, h);
        assert!(!first.is_empty());
        assert_eq!(first, second);
        assert_eq!(dirty_block_ratio(&first, &second), 0.0);
        assert!(!is_frame_dirty(&first, &second));
    }

    #[test]
    fn clock_sized_change_stays_below_dirty_threshold() {
        let (mut raw, w, h) = solid_rgba(1920, 1080, [10, 20, 30, 255]);
        let baseline = compute_block_hashes(&raw, w, h);
        // Taskbar clock digit (~10×16) sits inside a single 64×64 tile.
        paint_rect(&mut raw, w, 1880, 1040, 10, 16, [200, 200, 200, 255]);
        let changed = compute_block_hashes(&raw, w, h);
        let ratio = dirty_block_ratio(&baseline, &changed);
        assert!(
            ratio > 0.0 && ratio < DIRTY_BLOCK_RATIO_THRESHOLD,
            "clock flicker ratio {ratio} should be detected but below {}",
            DIRTY_BLOCK_RATIO_THRESHOLD
        );
        assert!(!is_frame_dirty(&baseline, &changed));
    }

    #[test]
    fn cursor_sized_change_stays_below_dirty_threshold() {
        let (mut raw, w, h) = solid_rgba(1920, 1080, [10, 20, 30, 255]);
        let baseline = compute_block_hashes(&raw, w, h);
        paint_rect(&mut raw, w, 960, 540, 32, 32, [255, 0, 0, 255]);
        let changed = compute_block_hashes(&raw, w, h);
        let ratio = dirty_block_ratio(&baseline, &changed);
        assert!(
            ratio > 0.0 && ratio < DIRTY_BLOCK_RATIO_THRESHOLD,
            "cursor flicker ratio {ratio} should not force a full re-encode"
        );
        assert!(!is_frame_dirty(&baseline, &changed));
    }

    #[test]
    fn large_region_change_is_dirty() {
        let (mut raw, w, h) = solid_rgba(1920, 1080, [10, 20, 30, 255]);
        let baseline = compute_block_hashes(&raw, w, h);
        paint_rect(&mut raw, w, 100, 100, 400, 200, [1, 2, 3, 255]);
        let changed = compute_block_hashes(&raw, w, h);
        assert!(dirty_block_ratio(&baseline, &changed) >= DIRTY_BLOCK_RATIO_THRESHOLD);
        assert!(is_frame_dirty(&baseline, &changed));
    }

    #[test]
    fn first_frame_and_resize_count_as_dirty() {
        let (raw, w, h) = solid_rgba(128, 128, [1, 2, 3, 255]);
        let hashes = compute_block_hashes(&raw, w, h);
        assert!(is_frame_dirty(&[], &hashes));
        assert_eq!(dirty_block_ratio(&[], &hashes), 1.0);
        assert_eq!(dirty_block_ratio(&hashes[..1], &hashes), 1.0);
    }

    #[test]
    fn jpeg_encoder_produces_decodable_frame_with_original_dimensions() {
        let rgb = RgbImage::from_fn(32, 24, |x, y| {
            image::Rgb([(x * 7) as u8, (y * 9) as u8, ((x + y) * 3) as u8])
        });

        let encoded = encode_jpeg(&rgb, 60).unwrap();
        assert_eq!(&encoded[..2], &[0xff, 0xd8]);
        assert_eq!(&encoded[encoded.len() - 2..], &[0xff, 0xd9]);
        assert_eq!(
            image::load_from_memory(&encoded).unwrap().dimensions(),
            (32, 24)
        );
    }
}
