//! Screen capture diagnostic tool.
//! Captures screenshots from active monitors using `xcap`,
//! compresses to JPEG or PNG, and encodes to Base64 data URI format.

use at_pc_protocol::models::MonitorInfo;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ExtendedColorType};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::sync::{Arc, Mutex, OnceLock};
use xcap::Monitor;

/// Default maximum dimension for screen capture to cap LLM token usage and network payload
pub const DEFAULT_MAX_DIMENSION: u32 = 1280;

fn is_empty_arc_str(s: &Arc<str>) -> bool {
    s.is_empty()
}

static MOCK_MONITORS: OnceLock<Mutex<Option<Vec<MonitorInfo>>>> = OnceLock::new();

/// Sets an in-memory mock monitor list for testing environments
pub fn set_mock_monitors(monitors: Option<Vec<MonitorInfo>>) {
    let mut lock = MOCK_MONITORS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap();
    *lock = monitors;
}

/// Clears mock monitor configuration
pub fn reset_mock_monitors() {
    set_mock_monitors(None);
}

/// Checks whether in-memory mock monitors are configured
pub fn is_mock_monitors_set() -> bool {
    if let Some(lock) = MOCK_MONITORS.get() {
        lock.lock().unwrap().is_some()
    } else {
        false
    }
}

/// Lists all connected physical and virtual display monitors on the target terminal with their display index,
/// name, primary flag, screen bounds (x, y, width, height), and DPI scale factor.
pub fn list_monitors() -> Result<Vec<MonitorInfo>, String> {
    if let Some(lock) = MOCK_MONITORS.get() {
        if let Some(ref mock) = *lock.lock().unwrap() {
            return Ok(mock.clone());
        }
    }

    if let Some(mock_img) = crate::tools::som::get_mock_screen_image() {
        return Ok(vec![MonitorInfo {
            display_index: 0,
            name: "Mock Display 0".to_string(),
            is_primary: true,
            x: 0,
            y: 0,
            width: mock_img.width(),
            height: mock_img.height(),
            scale_factor: 1.0,
        }]);
    }

    match Monitor::all() {
        Ok(monitors) if !monitors.is_empty() => {
            let mut list = Vec::with_capacity(monitors.len());
            for (i, m) in monitors.iter().enumerate() {
                list.push(MonitorInfo {
                    display_index: i,
                    name: m.name().unwrap_or_else(|_| format!("Display {}", i)),
                    is_primary: m.is_primary().unwrap_or(i == 0),
                    x: m.x().unwrap_or(0),
                    y: m.y().unwrap_or(0),
                    width: m.width().unwrap_or(0),
                    height: m.height().unwrap_or(0),
                    scale_factor: m.scale_factor().unwrap_or(1.0) as f64,
                });
            }
            Ok(list)
        }
        _ => {
            // Support mock monitors if mock screen image is present or in headless environments
            // where Monitor::all() is empty, providing at least 1 mock primary monitor.
            Ok(vec![MonitorInfo {
                display_index: 0,
                name: "Primary Display".to_string(),
                is_primary: true,
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                scale_factor: 1.0,
            }])
        }
    }
}

/// Result of a screen capture operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScreenCaptureResult {
    pub display_index: usize,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub base64_data: Arc<str>,
    #[serde(default, skip_serializing_if = "is_empty_arc_str")]
    pub raw_base64: Arc<str>,
    #[serde(default, skip_serializing_if = "is_empty_arc_str")]
    pub image_base64: Arc<str>,
    #[serde(default, skip_serializing_if = "is_empty_arc_str")]
    pub data_uri: Arc<str>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_factor: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<[u32; 4]>,
}

/// Captures a screenshot of the specified monitor display index with optional cropping and downsampling.
///
/// # Arguments
/// * `display_index` - Zero-based index of the display monitor (default: 0).
/// * `format` - Image format: `"jpeg"` (or `"jpg"`) or `"png"`. Defaults to `"jpeg"`.
/// * `quality` - Compression quality (1-100) for JPEG encoding (default: 80).
/// * `save_path` - Optional disk file path to save the captured image (e.g. `"/tmp/screen.jpg"`).
/// * `max_dimension` - Optional maximum bounding dimension (e.g. 1280) to downsample large 2K/4K screenshots and conserve tokens.
/// * `crop` - Optional `[x, y, width, height]` region of interest to capture only a target window or dialog.
pub fn capture_screen(
    display_index: usize,
    format: &str,
    quality: u8,
    save_path: Option<&str>,
    max_dimension: Option<u32>,
    crop: Option<[u32; 4]>,
) -> Result<ScreenCaptureResult, String> {
    let (orig_width, orig_height, dynamic_img) =
        if let Some(mock_img) = crate::tools::som::get_mock_screen_image() {
            let mons = list_monitors().unwrap_or_default();
            if !mons.is_empty() && display_index >= mons.len() {
                return Err(format!(
                    "Invalid display index {}: system has {} display(s) (indices 0..{})",
                    display_index,
                    mons.len(),
                    mons.len() - 1
                ));
            }
            (
                mock_img.width(),
                mock_img.height(),
                DynamicImage::ImageRgba8(mock_img),
            )
        } else if let Some(lock) = MOCK_MONITORS.get() {
            if let Some(ref mock_list) = *lock.lock().unwrap() {
                if mock_list.is_empty() {
                    return Err("No active displays/monitors found on this system".to_string());
                }
                if display_index >= mock_list.len() {
                    return Err(format!(
                        "Invalid display index {}: system has {} display(s) (indices 0..{})",
                        display_index,
                        mock_list.len(),
                        mock_list.len() - 1
                    ));
                }
                let mon = &mock_list[display_index];
                let img = image::RgbaImage::from_pixel(
                    mon.width.max(1),
                    mon.height.max(1),
                    image::Rgba([128, 128, 128, 255]),
                );
                (mon.width, mon.height, DynamicImage::ImageRgba8(img))
            } else {
                let monitors =
                    Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {}", e))?;
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
                let rgba_image = monitor.capture_image().map_err(|e| {
                    format!(
                        "Failed to capture screen on display {}: {}",
                        display_index, e
                    )
                })?;
                (
                    rgba_image.width(),
                    rgba_image.height(),
                    DynamicImage::ImageRgba8(rgba_image),
                )
            }
        } else {
            let monitors =
                Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {}", e))?;
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
            let rgba_image = monitor.capture_image().map_err(|e| {
                format!(
                    "Failed to capture screen on display {}: {}",
                    display_index, e
                )
            })?;
            (
                rgba_image.width(),
                rgba_image.height(),
                DynamicImage::ImageRgba8(rgba_image),
            )
        };

    let (dynamic_img, effective_crop, scale_factor) =
        process_dynamic_image(dynamic_img, max_dimension, crop);

    let final_width = dynamic_img.width();
    let final_height = dynamic_img.height();

    let fmt_normalized = format.trim().to_lowercase();
    let is_png = fmt_normalized == "png";
    let effective_format = if is_png { "png" } else { "jpeg" };

    let mut buf = Cursor::new(Vec::new());

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

    let mut saved_file_path = None;
    if let Some(sp) = save_path {
        let clean = sp.trim();
        if !clean.is_empty() {
            let path = std::path::Path::new(clean);
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            std::fs::write(path, &raw_bytes)
                .map_err(|e| format!("Failed to save screenshot to '{}': {}", clean, e))?;
            saved_file_path = Some(clean.to_string());
        }
    }

    let raw_base64_str = BASE64_STANDARD.encode(&raw_bytes);
    let raw_arc: Arc<str> = Arc::from(raw_base64_str);
    let data_uri_arc: Arc<str> = Arc::from(format!(
        "data:image/{};base64,{}",
        effective_format, raw_arc
    ));

    let has_resized = final_width != orig_width || final_height != orig_height;

    Ok(ScreenCaptureResult {
        display_index,
        width: final_width,
        height: final_height,
        format: effective_format.to_string(),
        base64_data: Arc::clone(&data_uri_arc),
        raw_base64: Arc::clone(&raw_arc),
        image_base64: Arc::clone(&raw_arc),
        data_uri: data_uri_arc,
        file_path: saved_file_path,
        original_width: if has_resized { Some(orig_width) } else { None },
        original_height: if has_resized { Some(orig_height) } else { None },
        scale_factor,
        crop: effective_crop,
    })
}

/// Helper function to process an in-memory DynamicImage by applying ROI cropping and resolution downsampling.
/// This allows pure in-memory testing without needing physical displays or OS screen recording permissions.
///
/// Default `max_dimension` is set to `DEFAULT_MAX_DIMENSION` (1280) when `None` is provided,
/// reducing payload volume and token costs for LLMs. Pass `Some(0)` to explicitly disable downsampling.
pub fn process_dynamic_image(
    mut dynamic_img: DynamicImage,
    max_dimension: Option<u32>,
    crop: Option<[u32; 4]>,
) -> (DynamicImage, Option<[u32; 4]>, Option<f32>) {
    let orig_width = dynamic_img.width();
    let orig_height = dynamic_img.height();

    // 1. Apply ROI cropping if specified
    let effective_crop = if let Some([cx, cy, cw, ch]) = crop {
        if cx < orig_width && cy < orig_height && cw > 0 && ch > 0 {
            let actual_w = cw.min(orig_width - cx);
            let actual_h = ch.min(orig_height - cy);
            dynamic_img = dynamic_img.crop_imm(cx, cy, actual_w, actual_h);
            Some([cx, cy, actual_w, actual_h])
        } else {
            None
        }
    } else {
        None
    };

    // 2. Apply resolution downsampling: defaults to 1280 if not specified (None).
    // Some(0) explicitly disables downsampling (uncapped).
    let effective_max_dim = match max_dimension {
        None => Some(DEFAULT_MAX_DIMENSION),
        Some(0) => None,
        Some(dim) => Some(dim),
    };

    let pre_scale_w = dynamic_img.width();
    let pre_scale_h = dynamic_img.height();
    let mut scale_factor = None;

    if let Some(max_dim) = effective_max_dim {
        if max_dim > 0 && (pre_scale_w > max_dim || pre_scale_h > max_dim) {
            let (new_w, new_h) = if pre_scale_w >= pre_scale_h {
                let nw = max_dim;
                let nh = ((pre_scale_h as f64 * max_dim as f64) / pre_scale_w as f64)
                    .round()
                    .max(1.0) as u32;
                (nw, nh)
            } else {
                let nh = max_dim;
                let nw = ((pre_scale_w as f64 * max_dim as f64) / pre_scale_w as f64)
                    .round()
                    .max(1.0) as u32;
                (nw, nh)
            };
            let factor = new_w as f32 / pre_scale_w as f32;
            scale_factor = Some(factor);
            // Optimize resizing: convert to RGB8 before resizing to eliminate
            // redundant alpha channel processing in Triangle interpolation
            let rgb_img = dynamic_img.to_rgb8();
            let resized = image::imageops::resize(
                &rgb_img,
                new_w,
                new_h,
                image::imageops::FilterType::Triangle,
            );
            dynamic_img = DynamicImage::ImageRgb8(resized);
        }
    }

    (dynamic_img, effective_crop, scale_factor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn test_in_memory_image_downsample_and_crop() {
        // Create a synthetic 1920x1080 image in memory (no display/screen recording permission required)
        let img = RgbaImage::from_pixel(1920, 1080, Rgba([255, 0, 0, 255]));
        let dynamic_img = DynamicImage::ImageRgba8(img);

        // Test 1: Downsampling only
        let (downscaled, crop_res, scale) =
            process_dynamic_image(dynamic_img.clone(), Some(960), None);
        assert_eq!(crop_res, None);
        assert_eq!(downscaled.width(), 960);
        assert_eq!(downscaled.height(), 540);
        assert!(scale.is_some());
        assert!((scale.unwrap() - 0.5).abs() < 0.01);

        // Test 2: Cropping only
        let (cropped, crop_res, scale) =
            process_dynamic_image(dynamic_img.clone(), None, Some([100, 100, 400, 300]));
        assert_eq!(crop_res, Some([100, 100, 400, 300]));
        assert_eq!(cropped.width(), 400);
        assert_eq!(cropped.height(), 300);
        assert_eq!(scale, None);

        // Test 3: Cropping then downsampling
        let (both, crop_res, scale) =
            process_dynamic_image(dynamic_img.clone(), Some(200), Some([100, 100, 400, 300]));
        assert_eq!(crop_res, Some([100, 100, 400, 300]));
        assert_eq!(both.width(), 200);
        assert_eq!(both.height(), 150);
        assert!(scale.is_some());
        assert!((scale.unwrap() - 0.5).abs() < 0.01);

        // Test 4: Default downsampling when max_dimension is None (caps at 1280)
        let (default_scaled, crop_res, scale) =
            process_dynamic_image(dynamic_img.clone(), None, None);
        assert_eq!(crop_res, None);
        assert_eq!(default_scaled.width(), 1280);
        assert_eq!(default_scaled.height(), 720);
        assert!(scale.is_some());
        assert!((scale.unwrap() - (1280.0 / 1920.0)).abs() < 0.01);

        // Test 5: Explicitly uncapped via Some(0)
        let (uncapped, crop_res, scale) = process_dynamic_image(dynamic_img, Some(0), None);
        assert_eq!(crop_res, None);
        assert_eq!(uncapped.width(), 1920);
        assert_eq!(uncapped.height(), 1080);
        assert_eq!(scale, None);
    }

    #[test]
    fn test_list_monitors_with_mock() {
        let mock_monitors = vec![
            MonitorInfo {
                display_index: 0,
                name: "Primary Display".to_string(),
                is_primary: true,
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                scale_factor: 1.0,
            },
            MonitorInfo {
                display_index: 1,
                name: "Secondary Display".to_string(),
                is_primary: false,
                x: 1920,
                y: 0,
                width: 2560,
                height: 1440,
                scale_factor: 1.25,
            },
        ];

        set_mock_monitors(Some(mock_monitors.clone()));
        let result = list_monitors().expect("list_monitors should succeed");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], mock_monitors[0]);
        assert_eq!(result[1], mock_monitors[1]);
        reset_mock_monitors();
    }

    #[test]
    fn test_list_monitors_live_or_fallback() {
        reset_mock_monitors();
        let result = list_monitors().expect("list_monitors should succeed");
        assert!(!result.is_empty(), "Should return at least 1 monitor");
        assert_eq!(result[0].display_index, 0);
        assert!(result[0].width > 0);
        assert!(result[0].height > 0);
    }
}
