//! Set-of-Mark (SoM) visual annotation and grounding engine.
//!
//! Provides pure Rust visual annotation (semi-transparent bounding boxes and numbered badges)
//! over desktop screenshots, supporting UI Tree controls, visual edge/contour detection,
//! and grid division strategies. Enables direct semantic clicking via `mark_id`.

use at_pc_protocol::models::{DesktopInputEvent, MarkedScreenResponse, ScreenMark, UiElement};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ExtendedColorType, Rgba, RgbaImage};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

/// Monotonically increasing mark ID generator
static NEXT_MARK_ID: AtomicU32 = AtomicU32::new(1);

/// In-memory cache of currently active screen marks
static MARK_CACHE: OnceLock<Mutex<HashMap<u32, ScreenMark>>> = OnceLock::new();

/// Optional in-memory mock screen image for permissionless, deterministic testing
static MOCK_SCREEN_IMAGE: OnceLock<Mutex<Option<RgbaImage>>> = OnceLock::new();

fn get_mark_cache() -> &'static Mutex<HashMap<u32, ScreenMark>> {
    MARK_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Allocates a new unique mark ID
pub fn allocate_mark_id() -> u32 {
    NEXT_MARK_ID.fetch_add(1, Ordering::SeqCst)
}

/// Resets the mark ID sequence and cache (primarily for tests)
pub fn reset_mark_cache() {
    let mut cache = get_mark_cache().lock().unwrap();
    cache.clear();
    NEXT_MARK_ID.store(1, Ordering::SeqCst);
}

/// Retrieves a cached screen mark by its ID
pub fn get_cached_mark(id: u32) -> Option<ScreenMark> {
    get_mark_cache().lock().unwrap().get(&id).cloned()
}

/// Clears existing marks and stores a fresh set of marks
pub fn clear_and_store_marks(marks: &[ScreenMark]) {
    let mut cache = get_mark_cache().lock().unwrap();
    cache.clear();
    for m in marks {
        cache.insert(m.id, m.clone());
    }
}

/// Stores marks into cache without wiping previously cached marks
pub fn store_cached_marks(marks: &[ScreenMark]) {
    let mut cache = get_mark_cache().lock().unwrap();
    for m in marks {
        cache.insert(m.id, m.clone());
    }
}

/// Returns count of currently cached marks
pub fn cached_marks_count() -> usize {
    get_mark_cache().lock().unwrap().len()
}

/// Sets an in-memory mock screen image for headless / macOS permissionless testing
pub fn set_mock_screen_image(img: Option<RgbaImage>) {
    let mut lock = MOCK_SCREEN_IMAGE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap();
    *lock = img;
}

/// Returns the currently configured mock screen image, if any
pub fn get_mock_screen_image() -> Option<RgbaImage> {
    MOCK_SCREEN_IMAGE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .clone()
}

// =========================================================================
// Pure Rust 5x7 Bitmap Font & Drawing Engine
// =========================================================================

/// 8 vibrant, high-contrast colors for alternating mark badges and bounding boxes
pub const COLOR_PALETTE: &[(Rgba<u8>, Rgba<u8>)] = &[
    (Rgba([220, 38, 38, 255]), Rgba([255, 255, 255, 255])), // Vivid Red
    (Rgba([37, 99, 235, 255]), Rgba([255, 255, 255, 255])), // Vivid Blue
    (Rgba([22, 163, 74, 255]), Rgba([255, 255, 255, 255])), // Vivid Green
    (Rgba([217, 119, 6, 255]), Rgba([255, 255, 255, 255])), // Vivid Amber
    (Rgba([147, 51, 234, 255]), Rgba([255, 255, 255, 255])), // Vivid Purple
    (Rgba([13, 148, 136, 255]), Rgba([255, 255, 255, 255])), // Vivid Teal
    (Rgba([234, 88, 12, 255]), Rgba([255, 255, 255, 255])), // Vivid Orange
    (Rgba([219, 39, 119, 255]), Rgba([255, 255, 255, 255])), // Vivid Pink
];

/// Returns 5-pixel-wide, 7-pixel-high bitmap rows for ASCII characters
pub fn get_glyph_5x7(c: char) -> [u8; 7] {
    match c {
        '#' => [
            0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
        ],
        '3' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110,
        ],
        '4' => [
            0b10010, 0b10010, 0b10010, 0b11111, 0b00010, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        'A' | 'a' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' | 'b' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' | 'c' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' | 'd' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' | 'e' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' | 'f' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' | 'g' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' | 'h' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' | 'i' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'J' | 'j' => [
            0b00001, 0b00001, 0b00001, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        'K' | 'k' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' | 'l' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' | 'm' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' | 'n' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' | 'o' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' | 'p' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' | 'q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10011, 0b01111,
        ],
        'R' | 'r' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' | 's' => [
            0b01110, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' | 't' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' | 'u' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' | 'v' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' | 'w' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'X' | 'x' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' | 'y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' | 'z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '_' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
        ':' => [
            0b00000, 0b00100, 0b00000, 0b00000, 0b00100, 0b00000, 0b00000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100,
        ],
        '[' => [
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ],
        ']' => [
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ],
        ' ' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        _ => [
            0b11111, 0b10001, 0b10101, 0b10001, 0b10101, 0b10001, 0b11111,
        ],
    }
}

/// Blends an RGBA pixel on an image using standard alpha compositing
#[inline]
pub fn blend_pixel(img: &mut RgbaImage, x: u32, y: u32, color: Rgba<u8>) {
    if x >= img.width() || y >= img.height() {
        return;
    }
    let alpha = color[3] as u32;
    if alpha == 0 {
        return;
    }
    if alpha == 255 {
        img.put_pixel(x, y, color);
        return;
    }
    let bg = img.get_pixel(x, y);
    let inv_a = 255 - alpha;
    let r = ((color[0] as u32 * alpha + bg[0] as u32 * inv_a) / 255) as u8;
    let g = ((color[1] as u32 * alpha + bg[1] as u32 * inv_a) / 255) as u8;
    let b = ((color[2] as u32 * alpha + bg[2] as u32 * inv_a) / 255) as u8;
    img.put_pixel(x, y, Rgba([r, g, b, 255]));
}

/// Fills a rectangular region on an image with alpha blending
pub fn draw_filled_rect(img: &mut RgbaImage, x: i32, y: i32, w: i32, h: i32, color: Rgba<u8>) {
    let img_w = img.width() as i32;
    let img_h = img.height() as i32;

    let x0 = x.clamp(0, img_w);
    let y0 = y.clamp(0, img_h);
    let x1 = (x + w).clamp(0, img_w);
    let y1 = (y + h).clamp(0, img_h);

    for py in y0..y1 {
        for px in x0..x1 {
            blend_pixel(img, px as u32, py as u32, color);
        }
    }
}

/// Draws an unfilled rectangular outline with configurable border thickness
pub fn draw_rect_outline(
    img: &mut RgbaImage,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    thickness: i32,
    color: Rgba<u8>,
) {
    if w <= 0 || h <= 0 {
        return;
    }
    let t = thickness.max(1);
    // Top border
    draw_filled_rect(img, x, y, w, t, color);
    // Bottom border
    draw_filled_rect(img, x, (y + h - t).max(y), w, t, color);
    // Left border
    draw_filled_rect(img, x, y, t, h, color);
    // Right border
    draw_filled_rect(img, (x + w - t).max(x), y, t, h, color);
}

/// Draws a text string using the embedded 5x7 font at a given scale factor
pub fn draw_text(
    img: &mut RgbaImage,
    start_x: i32,
    start_y: i32,
    text: &str,
    scale: u32,
    color: Rgba<u8>,
) {
    let s = scale.max(1) as i32;
    let mut cursor_x = start_x;

    for ch in text.chars() {
        let bitmap = get_glyph_5x7(ch);
        for (row_idx, &row) in bitmap.iter().enumerate() {
            let py = start_y + (row_idx as i32) * s;
            for col_idx in 0..5 {
                let px = cursor_x + col_idx * s;
                if (row & (1 << (4 - col_idx))) != 0 {
                    draw_filled_rect(img, px, py, s, s, color);
                }
            }
        }
        cursor_x += (5 + 1) * s;
    }
}

/// Draws a prominent badge box containing text (e.g. "#1") with border and solid background
#[allow(clippy::too_many_arguments)]
pub fn draw_badge(
    img: &mut RgbaImage,
    x: i32,
    y: i32,
    text: &str,
    badge_bg: Rgba<u8>,
    badge_border: Rgba<u8>,
    text_color: Rgba<u8>,
    scale: u32,
) {
    let s = scale.max(1) as i32;
    let char_count = text.chars().count() as i32;
    let text_w = char_count * (5 * s + s) - s;
    let text_h = 7 * s;

    let pad_x = 3 * s;
    let pad_y = 2 * s;
    let badge_w = text_w + pad_x * 2;
    let badge_h = text_h + pad_y * 2;

    let img_w = img.width() as i32;
    let img_h = img.height() as i32;

    let bx = x.clamp(0, (img_w - badge_w).max(0));
    let by = y.clamp(0, (img_h - badge_h).max(0));

    // Solid badge background
    draw_filled_rect(img, bx, by, badge_w, badge_h, badge_bg);
    // Badge outline border
    draw_rect_outline(img, bx, by, badge_w, badge_h, 1, badge_border);
    // Draw text inside badge
    draw_text(img, bx + pad_x, by + pad_y, text, scale, text_color);
}

/// Annotates an RGBA image with translucent highlights and numbered badges for each ScreenMark
pub fn annotate_image_with_marks(
    img: &mut RgbaImage,
    marks: &[ScreenMark],
    crop_offset: [i32; 2],
    scale_factor: Option<f32>,
) {
    annotate_image_with_marks_ext(img, marks, crop_offset, scale_factor, None);
}

/// Annotates an RGBA image with translucent highlights and numbered badges with optional monitor desktop offset
pub fn annotate_image_with_marks_ext(
    img: &mut RgbaImage,
    marks: &[ScreenMark],
    crop_offset: [i32; 2],
    scale_factor: Option<f32>,
    monitor_offset: Option<[i32; 2]>,
) {
    let scale = scale_factor.unwrap_or(1.0);
    let img_w = img.width() as i32;
    let img_h = img.height() as i32;
    let mon_off = monitor_offset.unwrap_or([0, 0]);

    for (idx, mark) in marks.iter().enumerate() {
        let (base_color, text_color) = COLOR_PALETTE[idx % COLOR_PALETTE.len()];

        // Transform physical rect coordinates to current image coordinates
        let draw_x = (((mark.rect[0] - mon_off[0] - crop_offset[0]) as f32) * scale).round() as i32;
        let draw_y = (((mark.rect[1] - mon_off[1] - crop_offset[1]) as f32) * scale).round() as i32;
        let draw_w = ((mark.rect[2] as f32) * scale).round().max(4.0) as i32;
        let draw_h = ((mark.rect[3] as f32) * scale).round().max(4.0) as i32;

        // Skip mark if completely out of current image bounds
        if draw_x + draw_w <= 0 || draw_x >= img_w || draw_y + draw_h <= 0 || draw_y >= img_h {
            continue;
        }

        // 1. Semi-transparent fill highlight (alpha = 45)
        let fill_color = Rgba([base_color[0], base_color[1], base_color[2], 45]);
        draw_filled_rect(img, draw_x, draw_y, draw_w, draw_h, fill_color);

        // 2. Solid 2px outline border
        draw_rect_outline(img, draw_x, draw_y, draw_w, draw_h, 2, base_color);

        // 3. Prominent numeric badge (e.g. "#1")
        let badge_text = format!("#{}", mark.id);
        let badge_scale = if img_w < 640 { 1 } else { 2 };
        let s = badge_scale as i32;
        let badge_h = 7 * s + 4 * s;

        // Position badge just above or inside top-left corner
        let badge_y = if draw_y >= badge_h + 2 {
            draw_y - badge_h + 1
        } else {
            draw_y + 2
        };

        draw_badge(
            img,
            draw_x,
            badge_y,
            &badge_text,
            base_color,
            Rgba([0, 0, 0, 255]),
            text_color,
            badge_scale,
        );
    }
}

// =========================================================================
// Set-of-Mark Generation Strategies
// =========================================================================

/// Strategy: Builds visual marks from UI Tree interactive elements
pub fn generate_marks_from_ui_elements(
    elements: &[UiElement],
    screen_bounds: [i32; 4],
) -> Vec<ScreenMark> {
    let mut marks = Vec::new();
    let max_w = screen_bounds[2];
    let max_h = screen_bounds[3];

    for el in elements {
        let w = el.rect[2];
        let h = el.rect[3];

        // Filter: element must have non-trivial dimensions
        if w < 12 || h < 12 {
            continue;
        }
        // Filter: omit huge root/container elements covering > 95% of window/screen
        if max_w > 0 && max_h > 0 && w >= (max_w * 95) / 100 && h >= (max_h * 95) / 100 {
            continue;
        }

        let id = allocate_mark_id();
        let cx = el.rect[0] + w / 2;
        let cy = el.rect[1] + h / 2;

        let label = if !el.name.trim().is_empty() {
            Some(format!("{}: {}", el.control_type, el.name))
        } else {
            Some(el.control_type.clone())
        };

        marks.push(ScreenMark {
            id,
            rect: el.rect,
            center: [cx, cy],
            label,
            control_type: Some(el.control_type.clone()),
        });
    }

    marks
}

/// Strategy: Builds visual grid marks dividing screen or target area into NxN cells
pub fn generate_marks_from_grid(
    width: u32,
    height: u32,
    divisions: u32,
    offset: [i32; 2],
) -> Vec<ScreenMark> {
    let div = divisions.clamp(2, 8) as i32;
    let w = width as i32;
    let h = height as i32;
    let cell_w = (w / div).max(1);
    let cell_h = (h / div).max(1);

    let mut marks = Vec::new();

    for r in 0..div {
        for c in 0..div {
            let id = allocate_mark_id();
            let rx = offset[0] + c * cell_w;
            let ry = offset[1] + r * cell_h;
            let actual_w = if c == div - 1 { w - c * cell_w } else { cell_w };
            let actual_h = if r == div - 1 { h - r * cell_h } else { cell_h };

            let cx = rx + actual_w / 2;
            let cy = ry + actual_h / 2;
            let label = format!("Grid R{}C{}", r + 1, c + 1);

            marks.push(ScreenMark {
                id,
                rect: [rx, ry, actual_w, actual_h],
                center: [cx, cy],
                label: Some(label),
                control_type: Some("GridCell".to_string()),
            });
        }
    }

    marks
}

/// Maximum width for visual box edge detection to avoid CPU-intensive passes on 2.5K/4K displays.
pub const SOM_MAX_DETECTION_WIDTH: u32 = 1280;

/// Internal helper that performs edge and contour detection on an image in its local coordinate system.
fn detect_visual_candidate_boxes(img: &RgbaImage) -> Vec<[i32; 4]> {
    let w = img.width();
    let h = img.height();
    if w < 24 || h < 24 {
        return Vec::new();
    }

    let block_size: u32 = 16;
    let grid_cols = w.div_ceil(block_size) as usize;
    let grid_rows = h.div_ceil(block_size) as usize;

    let mut active_blocks = vec![false; grid_rows * grid_cols];

    for gy in 0..grid_rows {
        let y_start = (gy as u32) * block_size;
        let y_end = (y_start + block_size).min(h);
        for gx in 0..grid_cols {
            let x_start = (gx as u32) * block_size;
            let x_end = (x_start + block_size).min(w);

            let mut edge_count = 0;
            for py in y_start..y_end {
                for px in x_start..x_end {
                    let p0 = img.get_pixel(px, py);
                    let l0 = (p0[0] as i32 * 299 + p0[1] as i32 * 587 + p0[2] as i32 * 114) / 1000;

                    let mut has_edge = false;
                    if px + 1 < w {
                        let p_right = img.get_pixel(px + 1, py);
                        let lr = (p_right[0] as i32 * 299
                            + p_right[1] as i32 * 587
                            + p_right[2] as i32 * 114)
                            / 1000;
                        let max_c_diff = (p0[0] as i32 - p_right[0] as i32)
                            .abs()
                            .max((p0[1] as i32 - p_right[1] as i32).abs())
                            .max((p0[2] as i32 - p_right[2] as i32).abs());
                        if (l0 - lr).abs() > 28 || max_c_diff > 36 {
                            has_edge = true;
                        }
                    }
                    if !has_edge && py + 1 < h {
                        let p_down = img.get_pixel(px, py + 1);
                        let ld = (p_down[0] as i32 * 299
                            + p_down[1] as i32 * 587
                            + p_down[2] as i32 * 114)
                            / 1000;
                        let max_c_diff = (p0[0] as i32 - p_down[0] as i32)
                            .abs()
                            .max((p0[1] as i32 - p_down[1] as i32).abs())
                            .max((p0[2] as i32 - p_down[2] as i32).abs());
                        if (l0 - ld).abs() > 28 || max_c_diff > 36 {
                            has_edge = true;
                        }
                    }
                    if has_edge {
                        edge_count += 1;
                    }
                }
            }

            if edge_count >= 5 {
                active_blocks[gy * grid_cols + gx] = true;
            }
        }
    }

    let mut visited = vec![false; grid_rows * grid_cols];
    let mut candidate_boxes: Vec<[i32; 4]> = Vec::new();

    for gy in 0..grid_rows {
        for gx in 0..grid_cols {
            let idx = gy * grid_cols + gx;
            if !active_blocks[idx] || visited[idx] {
                continue;
            }

            let mut queue = vec![(gx, gy)];
            visited[idx] = true;

            let mut min_gx = gx;
            let mut max_gx = gx;
            let mut min_gy = gy;
            let mut max_gy = gy;
            let mut block_count = 0;

            while let Some((cx, cy)) = queue.pop() {
                block_count += 1;
                min_gx = min_gx.min(cx);
                max_gx = max_gx.max(cx);
                min_gy = min_gy.min(cy);
                max_gy = max_gy.max(cy);

                let neighbors = [
                    (cx.wrapping_sub(1), cy),
                    (cx + 1, cy),
                    (cx, cy.wrapping_sub(1)),
                    (cx, cy + 1),
                ];
                for (nx, ny) in neighbors {
                    if nx < grid_cols && ny < grid_rows {
                        let n_idx = ny * grid_cols + nx;
                        if active_blocks[n_idx] && !visited[n_idx] {
                            visited[n_idx] = true;
                            queue.push((nx, ny));
                        }
                    }
                }
            }

            if block_count >= 1 && block_count <= (grid_cols * grid_rows / 4).max(4) {
                let bx = (min_gx as u32 * block_size) as i32;
                let by = (min_gy as u32 * block_size) as i32;
                let bw = (((max_gx - min_gx + 1) as u32 * block_size).min(w)) as i32;
                let bh = (((max_gy - min_gy + 1) as u32 * block_size).min(h)) as i32;

                if bw >= 16
                    && bh >= 16
                    && (bw as f32) <= (w as f32 * 0.85)
                    && (bh as f32) <= (h as f32 * 0.6)
                {
                    candidate_boxes.push([bx, by, bw, bh]);
                }
            }
        }
    }

    let mut filtered_boxes: Vec<[i32; 4]> = Vec::new();
    for b in candidate_boxes {
        let has_overlap = filtered_boxes.iter().any(|fb| {
            let ix0 = b[0].max(fb[0]);
            let iy0 = b[1].max(fb[1]);
            let ix1 = (b[0] + b[2]).min(fb[0] + fb[2]);
            let iy1 = (b[1] + b[3]).min(fb[1] + fb[3]);
            let iw = (ix1 - ix0).max(0);
            let ih = (iy1 - iy0).max(0);
            let inter_area = iw * ih;
            let union_area = b[2] * b[3] + fb[2] * fb[3] - inter_area;
            union_area > 0 && (inter_area as f32 / union_area as f32) > 0.4
        });
        if !has_overlap {
            filtered_boxes.push(b);
        }
    }

    filtered_boxes
}

/// Fast strided nearest-neighbor downsampling for RGBA image (<0.3ms for 2560x1600 -> 1280x800).
/// Bypasses imageops::resize generic interpolation overhead which took ~10ms.
pub fn fast_downsample_rgba(img: &RgbaImage, target_w: u32, target_h: u32) -> RgbaImage {
    let src_w = img.width() as usize;
    let src_h = img.height() as usize;
    let target_w_us = target_w as usize;
    let target_h_us = target_h as usize;
    let src_raw = img.as_raw();
    let mut dst = vec![0u8; target_w_us * target_h_us * 4];

    // Center-aligned nearest-neighbor coordinate mapping: floor((i + 0.5) * ratio)
    // Aligns 100% with standard nearest-neighbor sampling without the generic overhead.
    let x_indices: Vec<usize> = (0..target_w_us)
        .map(|x| (((x * 2 + 1) * src_w) / (target_w_us * 2)).min(src_w - 1))
        .collect();

    for dy in 0..target_h_us {
        let sy = (((dy * 2 + 1) * src_h) / (target_h_us * 2)).min(src_h - 1);
        let src_row_offset = sy * src_w * 4;
        let dst_row_offset = dy * target_w_us * 4;
        let dst_row = &mut dst[dst_row_offset..dst_row_offset + target_w_us * 4];

        for (dx, &sx) in x_indices.iter().enumerate() {
            let src_idx = src_row_offset + sx * 4;
            let dst_idx = dx * 4;
            dst_row[dst_idx..dst_idx + 4].copy_from_slice(&src_raw[src_idx..src_idx + 4]);
        }
    }

    RgbaImage::from_raw(target_w, target_h, dst)
        .unwrap_or_else(|| RgbaImage::new(target_w, target_h))
}

/// Strategy: Detects visual bounding boxes for non-accessible Canvas, games, and 自绘 applications.
/// If image width > 1280, automatically downsamples before edge detection to reduce pass runtime
/// from ~10ms to <3ms, then scales detected bounding boxes back to the original coordinate system.
pub fn detect_visual_boxes(
    img: &RgbaImage,
    offset: [i32; 2],
    scale_factor: f32,
) -> Vec<ScreenMark> {
    let w = img.width();
    let h = img.height();
    if w < 24 || h < 24 {
        return Vec::new();
    }

    let scale = if scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };

    let local_boxes = if w > SOM_MAX_DETECTION_WIDTH {
        let ds_w = SOM_MAX_DETECTION_WIDTH;
        let ds_h = ((h as f64 * ds_w as f64) / w as f64).round().max(1.0) as u32;
        // Fast strided nearest-neighbor downsampling for edge/contour detection (<0.3ms on 2.5K)
        let ds_img = fast_downsample_rgba(img, ds_w, ds_h);
        let ds_boxes = detect_visual_candidate_boxes(&ds_img);

        let scale_x = w as f32 / ds_w as f32;
        let scale_y = h as f32 / ds_h as f32;

        ds_boxes
            .into_iter()
            .map(|b| {
                let bx = (b[0] as f32 * scale_x).round() as i32;
                let by = (b[1] as f32 * scale_y).round() as i32;
                let bw = (b[2] as f32 * scale_x).round() as i32;
                let bh = (b[3] as f32 * scale_y).round() as i32;
                [bx, by, bw, bh]
            })
            .collect()
    } else {
        detect_visual_candidate_boxes(img)
    };

    let mut marks = Vec::new();
    for b in local_boxes.into_iter().take(32) {
        let phys_x = offset[0] + ((b[0] as f32) / scale).round() as i32;
        let phys_y = offset[1] + ((b[1] as f32) / scale).round() as i32;
        let phys_w = ((b[2] as f32) / scale).round() as i32;
        let phys_h = ((b[3] as f32) / scale).round() as i32;

        let id = allocate_mark_id();
        let cx = phys_x + phys_w / 2;
        let cy = phys_y + phys_h / 2;
        marks.push(ScreenMark {
            id,
            rect: [phys_x, phys_y, phys_w, phys_h],
            center: [cx, cy],
            label: Some(format!("VisualBox #{}", id)),
            control_type: Some("VisualElement".to_string()),
        });
    }

    marks
}

// =========================================================================
// Main Processing & Capture API
// =========================================================================

/// Processes an in-memory image to generate Set-of-Mark annotations and response.
/// Allows full headless testing without physical monitors or OS permissions.
#[allow(clippy::too_many_arguments)]
pub fn generate_marked_screen_from_image(
    dynamic_img: DynamicImage,
    strategy_opt: Option<&str>,
    grid_divisions: Option<u32>,
    elements_override: Option<&[UiElement]>,
    format_str: &str,
    quality: u8,
    max_dimension: Option<u32>,
    crop: Option<[u32; 4]>,
    display_index: usize,
) -> Result<MarkedScreenResponse, String> {
    generate_marked_screen_from_image_ext(
        dynamic_img,
        strategy_opt,
        grid_divisions,
        elements_override,
        format_str,
        quality,
        max_dimension,
        crop,
        display_index,
        None,
        None,
    )
}

/// Extended in-memory Set-of-Mark generator supporting window title targeting and multi-monitor offsets.
#[allow(clippy::too_many_arguments)]
pub fn generate_marked_screen_from_image_ext(
    dynamic_img: DynamicImage,
    strategy_opt: Option<&str>,
    grid_divisions: Option<u32>,
    elements_override: Option<&[UiElement]>,
    format_str: &str,
    quality: u8,
    max_dimension: Option<u32>,
    crop: Option<[u32; 4]>,
    display_index: usize,
    window_title: Option<&str>,
    monitor_offset: Option<[i32; 2]>,
) -> Result<MarkedScreenResponse, String> {
    let orig_width = dynamic_img.width();
    let orig_height = dynamic_img.height();
    let mon_off = monitor_offset.unwrap_or([0, 0]);

    // 1. Apply ROI cropping and downscaling
    let (processed_img, effective_crop, scale_factor) =
        crate::tools::screen::process_dynamic_image(dynamic_img, max_dimension, crop);

    let final_width = processed_img.width();
    let final_height = processed_img.height();
    let has_resized = final_width != orig_width || final_height != orig_height;

    let crop_offset = match effective_crop {
        Some([cx, cy, _, _]) => [cx as i32, cy as i32],
        None => [0, 0],
    };

    let requested_strategy = strategy_opt.unwrap_or("auto").trim().to_lowercase();

    // 2. Generate marks according to selected strategy
    let (gw, gh, goff) = match effective_crop {
        Some([cx, cy, cw, ch]) => (cw, ch, [mon_off[0] + cx as i32, mon_off[1] + cy as i32]),
        None => (orig_width, orig_height, mon_off),
    };

    let (marks, effective_strategy) = match requested_strategy.as_str() {
        "grid" => {
            let divs = grid_divisions.unwrap_or(4);
            let m = generate_marks_from_grid(gw, gh, divs, goff);
            (m, "grid".to_string())
        }
        "contours" | "boxes" | "edges" => {
            let rgba = processed_img.to_rgba8();
            let mut m = detect_visual_boxes(
                &rgba,
                [mon_off[0] + crop_offset[0], mon_off[1] + crop_offset[1]],
                scale_factor.unwrap_or(1.0),
            );
            if m.is_empty() {
                m = generate_marks_from_grid(gw, gh, 4, goff);
            }
            (m, "contours".to_string())
        }
        "ui_tree" => {
            let elements = if let Some(els) = elements_override {
                els.to_vec()
            } else {
                let tree = crate::tools::uia::get_ui_tree(Some(5), window_title)?;
                tree.elements
            };
            let m = generate_marks_from_ui_elements(
                &elements,
                [
                    mon_off[0],
                    mon_off[1],
                    orig_width as i32,
                    orig_height as i32,
                ],
            );
            (m, "ui_tree".to_string())
        }
        "hybrid" => {
            // Explicit hybrid strategy: merge UI tree controls with non-overlapping visual boxes
            let elements = if let Some(els) = elements_override {
                els.to_vec()
            } else if let Ok(tree) = crate::tools::uia::get_ui_tree(Some(5), window_title) {
                tree.elements
            } else {
                Vec::new()
            };

            let mut ui_marks = generate_marks_from_ui_elements(
                &elements,
                [
                    mon_off[0],
                    mon_off[1],
                    orig_width as i32,
                    orig_height as i32,
                ],
            );
            let rgba = processed_img.to_rgba8();
            let visual_marks = detect_visual_boxes(
                &rgba,
                [mon_off[0] + crop_offset[0], mon_off[1] + crop_offset[1]],
                scale_factor.unwrap_or(1.0),
            );

            let mut added_visual = 0;
            for vm in visual_marks {
                let overlaps = ui_marks.iter().any(|um| {
                    let ix0 = vm.rect[0].max(um.rect[0]);
                    let iy0 = vm.rect[1].max(um.rect[1]);
                    let ix1 = (vm.rect[0] + vm.rect[2]).min(um.rect[0] + um.rect[2]);
                    let iy1 = (vm.rect[1] + vm.rect[3]).min(um.rect[1] + um.rect[3]);
                    let iw = (ix1 - ix0).max(0);
                    let ih = (iy1 - iy0).max(0);
                    let inter = iw * ih;
                    let union = vm.rect[2] * vm.rect[3] + um.rect[2] * um.rect[3] - inter;
                    union > 0 && (inter as f32 / union as f32) > 0.25
                });
                if !overlaps {
                    ui_marks.push(vm);
                    added_visual += 1;
                }
            }

            if !ui_marks.is_empty() {
                let eff = if added_visual > 0 && ui_marks.len() > added_visual {
                    "hybrid".to_string()
                } else if added_visual > 0 {
                    "contours".to_string()
                } else {
                    "ui_tree".to_string()
                };
                (ui_marks, eff)
            } else {
                let grid_marks =
                    generate_marks_from_grid(gw, gh, grid_divisions.unwrap_or(4), goff);
                (grid_marks, "grid".to_string())
            }
        }
        _ => {
            // "auto" strategy: prefer UI tree; augment with visual boxes if available; fallback to grid if none
            let elements = if let Some(els) = elements_override {
                els.to_vec()
            } else if let Ok(tree) = crate::tools::uia::get_ui_tree(Some(5), window_title) {
                tree.elements
            } else {
                Vec::new()
            };

            let mut ui_marks = generate_marks_from_ui_elements(
                &elements,
                [
                    mon_off[0],
                    mon_off[1],
                    orig_width as i32,
                    orig_height as i32,
                ],
            );
            let rgba = processed_img.to_rgba8();
            let visual_marks = detect_visual_boxes(
                &rgba,
                [mon_off[0] + crop_offset[0], mon_off[1] + crop_offset[1]],
                scale_factor.unwrap_or(1.0),
            );

            let mut added_visual = 0;
            for vm in visual_marks {
                let overlaps = ui_marks.iter().any(|um| {
                    let ix0 = vm.rect[0].max(um.rect[0]);
                    let iy0 = vm.rect[1].max(um.rect[1]);
                    let ix1 = (vm.rect[0] + vm.rect[2]).min(um.rect[0] + um.rect[2]);
                    let iy1 = (vm.rect[1] + vm.rect[3]).min(um.rect[1] + um.rect[3]);
                    let iw = (ix1 - ix0).max(0);
                    let ih = (iy1 - iy0).max(0);
                    let inter = iw * ih;
                    let union = vm.rect[2] * vm.rect[3] + um.rect[2] * um.rect[3] - inter;
                    union > 0 && (inter as f32 / union as f32) > 0.25
                });
                if !overlaps {
                    ui_marks.push(vm);
                    added_visual += 1;
                }
            }

            if !ui_marks.is_empty() {
                let eff = if added_visual > 0 && ui_marks.len() > added_visual {
                    "hybrid".to_string()
                } else if added_visual > 0 {
                    "contours".to_string()
                } else {
                    "ui_tree".to_string()
                };
                (ui_marks, eff)
            } else {
                let grid_marks =
                    generate_marks_from_grid(gw, gh, grid_divisions.unwrap_or(4), goff);
                (grid_marks, "grid".to_string())
            }
        }
    };

    // Filter marks by crop region if ROI was requested
    let filtered_marks = if let Some([cx, cy, cw, ch]) = effective_crop {
        let x0 = mon_off[0] + cx as i32;
        let y0 = mon_off[1] + cy as i32;
        let x1 = x0 + cw as i32;
        let y1 = y0 + ch as i32;
        marks
            .into_iter()
            .filter(|m| {
                let mx0 = m.rect[0];
                let my0 = m.rect[1];
                let mx1 = m.rect[0] + m.rect[2];
                let my1 = m.rect[1] + m.rect[3];
                mx0 < x1 && mx1 > x0 && my0 < y1 && my1 > y0
            })
            .collect()
    } else {
        marks
    };

    // Assign clean sequential 1-based IDs (#1, #2, #3...) to all active marks
    let mut final_marks = filtered_marks;
    for (i, m) in final_marks.iter_mut().enumerate() {
        let new_id = (i + 1) as u32;
        m.id = new_id;
        if let Some(ref mut l) = m.label {
            if l.starts_with("VisualBox #") {
                *l = format!("VisualBox #{}", new_id);
            }
        }
    }

    // Store marks into global memory cache for subsequent click_mark / mouse_click
    clear_and_store_marks(&final_marks);

    // 3. Render Set-of-Mark visual overlays onto image
    let mut annotated_rgba = processed_img.to_rgba8();
    annotate_image_with_marks_ext(
        &mut annotated_rgba,
        &final_marks,
        crop_offset,
        scale_factor,
        Some(mon_off),
    );

    // 4. Encode image to format
    let fmt_clean = format_str.trim().to_lowercase();
    let is_png = fmt_clean == "png";
    let effective_format = if is_png { "png" } else { "jpeg" };

    let mut buf = Cursor::new(Vec::new());
    if is_png {
        let dyn_ann = DynamicImage::ImageRgba8(annotated_rgba);
        dyn_ann
            .write_to(&mut buf, image::ImageFormat::Png)
            .map_err(|e| format!("Failed to encode PNG marked image: {}", e))?;
    } else {
        let rgb_img = DynamicImage::ImageRgba8(annotated_rgba).to_rgb8();
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
            .map_err(|e| format!("Failed to encode JPEG marked image: {}", e))?;
    }

    let raw_bytes = buf.into_inner();
    let base64_encoded = BASE64_STANDARD.encode(&raw_bytes);
    let base64_data = format!("data:image/{};base64,{}", effective_format, base64_encoded);
    let total_marks = final_marks.len();

    Ok(MarkedScreenResponse {
        display_index,
        width: final_width,
        height: final_height,
        format: effective_format.to_string(),
        base64_data: base64_data.clone(),
        raw_base64: base64_encoded.clone(),
        image_base64: base64_encoded,
        data_uri: base64_data,
        marks: final_marks,
        total_marks,
        source: effective_strategy,
        original_width: if has_resized { Some(orig_width) } else { None },
        original_height: if has_resized { Some(orig_height) } else { None },
        scale_factor,
    })
}

/// Captures screenshot and generates Set-of-Mark annotated response.
/// Uses in-memory mock if set via `set_mock_screen_image`, otherwise captures physical monitor.
#[allow(clippy::too_many_arguments)]
pub fn get_marked_screen(
    display_index: usize,
    format_str: &str,
    quality: u8,
    max_dimension: Option<u32>,
    crop: Option<[u32; 4]>,
    strategy: Option<&str>,
    grid_divisions: Option<u32>,
    window_title: Option<&str>,
) -> Result<MarkedScreenResponse, String> {
    let (dynamic_img, monitor_offset) = if let Some(mock) = get_mock_screen_image() {
        let offset = if let Ok(monitors) = crate::tools::screen::list_monitors() {
            if let Some(mon) = monitors.iter().find(|m| m.display_index == display_index) {
                [mon.x, mon.y]
            } else {
                [0, 0]
            }
        } else {
            [0, 0]
        };
        (DynamicImage::ImageRgba8(mock), offset)
    } else if crate::tools::screen::is_mock_monitors_set() {
        let monitors = crate::tools::screen::list_monitors()?;
        if monitors.is_empty() {
            return Err("No active displays/monitors found on this system".to_string());
        }
        if display_index >= monitors.len() {
            return Err(format!(
                "Invalid display index {}: system has {} display(s)",
                display_index,
                monitors.len()
            ));
        }
        let mon = &monitors[display_index];
        let mon_x = mon.x;
        let mon_y = mon.y;
        let mock = RgbaImage::from_pixel(
            mon.width.max(1),
            mon.height.max(1),
            Rgba([240, 240, 240, 255]),
        );
        (DynamicImage::ImageRgba8(mock), [mon_x, mon_y])
    } else {
        let monitors =
            xcap::Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {}", e))?;
        if monitors.is_empty() {
            return Err("No active displays/monitors found on this system".to_string());
        }
        if display_index >= monitors.len() {
            return Err(format!(
                "Invalid display index {}: system has {} display(s)",
                display_index,
                monitors.len()
            ));
        }
        let mon = &monitors[display_index];
        let mon_x = mon.x().unwrap_or(0);
        let mon_y = mon.y().unwrap_or(0);
        let rgba_image = mon.capture_image().map_err(|e| {
            format!(
                "Failed to capture screen on display {}: {}",
                display_index, e
            )
        })?;
        (DynamicImage::ImageRgba8(rgba_image), [mon_x, mon_y])
    };

    generate_marked_screen_from_image_ext(
        dynamic_img,
        strategy,
        grid_divisions,
        None,
        format_str,
        quality,
        max_dimension,
        crop,
        display_index,
        window_title,
        Some(monitor_offset),
    )
}

/// Simulates clicking directly on a visual mark ID obtained from `get_marked_screen`
pub fn click_mark(mark_id: u32, button: Option<&str>, count: Option<u8>) -> Result<Value, String> {
    let mark = get_cached_mark(mark_id).ok_or_else(|| {
        format!(
            "Mark #{} not found in mark cache. Call get_marked_screen first to generate and view marks.",
            mark_id
        )
    })?;

    let btn_num = match button.map(|s| s.trim().to_lowercase()).as_deref() {
        Some("middle") | Some("center") | Some("m") | Some("1") => 1,
        Some("right") | Some("r") | Some("2") => 2,
        _ => 0, // default left click
    };
    let click_count = count.unwrap_or(1).max(1);

    let target_x = mark.center[0];
    let target_y = mark.center[1];

    crate::input::inject_input_event(DesktopInputEvent::MouseMovePixel {
        x: target_x,
        y: target_y,
    })?;
    std::thread::sleep(std::time::Duration::from_millis(10));
    crate::input::inject_input_event(DesktopInputEvent::MouseClick {
        button: btn_num,
        count: click_count,
    })?;

    Ok(serde_json::json!({
        "success": true,
        "action": "click_mark",
        "mark_id": mark_id,
        "coordinates": [mark.center[0], mark.center[1]],
        "button": btn_num,
        "count": click_count,
        "label": mark.label,
        "control_type": mark.control_type,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mark_cache_operations() {
        reset_mark_cache();
        assert_eq!(cached_marks_count(), 0);

        let m1 = ScreenMark {
            id: 1,
            rect: [50, 50, 100, 30],
            center: [100, 65],
            label: Some("Button A".to_string()),
            control_type: Some("Button".to_string()),
        };
        store_cached_marks(std::slice::from_ref(&m1));
        assert_eq!(cached_marks_count(), 1);

        let retrieved = get_cached_mark(1).expect("Mark 1 should exist");
        assert_eq!(retrieved, m1);

        reset_mark_cache();
        assert_eq!(cached_marks_count(), 0);
        assert!(get_cached_mark(1).is_none());
    }

    #[test]
    fn test_grid_marks_generation() {
        reset_mark_cache();
        let marks = generate_marks_from_grid(800, 600, 4, [100, 100]);
        assert_eq!(marks.len(), 16);
        assert_eq!(marks[0].rect[0], 100);
        assert_eq!(marks[0].rect[1], 100);
        assert_eq!(marks[0].rect[2], 200);
        assert_eq!(marks[0].rect[3], 150);
        assert_eq!(marks[0].label, Some("Grid R1C1".to_string()));
    }

    #[test]
    fn test_drawing_and_glyph_rendering() {
        let mut img = RgbaImage::from_pixel(100, 100, Rgba([255, 255, 255, 255]));

        // Draw filled rect with semi-transparency
        draw_filled_rect(&mut img, 10, 10, 20, 20, Rgba([255, 0, 0, 128]));
        let p = img.get_pixel(15, 15);
        assert_eq!(p[0], 255);
        assert!(p[1] < 200);

        // Draw badge with text
        draw_badge(
            &mut img,
            50,
            50,
            "#1",
            Rgba([220, 38, 38, 255]),
            Rgba([255, 255, 255, 255]),
            Rgba([255, 255, 255, 255]),
            1,
        );
        let badge_pixel = img.get_pixel(52, 52);
        assert_eq!(badge_pixel[0], 220);
        assert_eq!(badge_pixel[1], 38);
    }
}
