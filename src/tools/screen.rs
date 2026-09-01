//! Screen capture diagnostic tool.
//! Captures screenshots from active monitors using `xcap`,
//! compresses to JPEG or PNG, and encodes to Base64 data URI format.

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ExtendedColorType};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use xcap::Monitor;

/// Result of a screen capture operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScreenCaptureResult {
    pub display_index: usize,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub base64_data: String,
}

/// Captures a screenshot of the specified monitor display index.
///
/// # Arguments
/// * `display_index` - Zero-based index of the display monitor (default: 0).
/// * `format` - Image format: `"jpeg"` (or `"jpg"`) or `"png"`. Defaults to `"jpeg"`.
/// * `quality` - Compression quality (1-100) for JPEG encoding (default: 80).
pub fn capture_screen(
    display_index: usize,
    format: &str,
    quality: u8,
) -> Result<ScreenCaptureResult, String> {
    let monitors = Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {}", e))?;

    if monitors.is_empty() {
        return Err("No active displays/monitors found on this system".to_string());
    }

    if display_index >= monitors.len() {
        return Err(format!(
            "Invalid display index {}: system has {} display(s) (indices 0..{})",
            display_index,
            monitors.len(),
            monitors.len() - 1
        ));
    }

    let monitor = &monitors[display_index];
    let rgba_image = monitor
        .capture_image()
        .map_err(|e| format!("Failed to capture screen on display {}: {}", display_index, e))?;

    let width = rgba_image.width();
    let height = rgba_image.height();

    let fmt_normalized = format.trim().to_lowercase();
    let is_png = fmt_normalized == "png";
    let effective_format = if is_png { "png" } else { "jpeg" };

    let mut buf = Cursor::new(Vec::new());
    let dynamic_img = DynamicImage::ImageRgba8(rgba_image);

    if is_png {
        dynamic_img
            .write_to(&mut buf, image::ImageFormat::Png)
            .map_err(|e| format!("Failed to encode PNG image: {}", e))?;
    } else {
        let rgb_img = dynamic_img.to_rgb8();
        let effective_quality = if quality == 0 {
            80
        } else {
            quality.clamp(1, 100)
        };
        let mut encoder = JpegEncoder::new_with_quality(&mut buf, effective_quality);
        encoder
            .encode(
                rgb_img.as_raw(),
                rgb_img.width(),
                rgb_img.height(),
                ExtendedColorType::Rgb8,
            )
            .map_err(|e| format!("Failed to encode JPEG image: {}", e))?;
    }

    let raw_bytes = buf.into_inner();
    let base64_encoded = BASE64_STANDARD.encode(&raw_bytes);
    let base64_data = format!("data:image/{};base64,{}", effective_format, base64_encoded);

    Ok(ScreenCaptureResult {
        display_index,
        width,
        height,
        format: effective_format.to_string(),
        base64_data,
    })
}
