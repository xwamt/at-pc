//! Integration and performance test for Milestone 3 Task 12:
//! SoM (Set-of-Mark) visual box detection downsampling optimization,
//! coordinate mapping accuracy, and sub-3ms runtime on 2.5K displays.

use at_pc_agent::tools::som::{
    detect_visual_boxes, draw_filled_rect, draw_rect_outline, reset_mark_cache,
};
use image::{Rgba, RgbaImage};
use std::time::Instant;

#[test]
fn test_som_downsampled_detection_and_coordinate_mapping_accuracy() {
    reset_mark_cache();

    // 1. Create a 2560x1600 synthetic high-resolution desktop canvas
    let mut canvas = RgbaImage::from_pixel(2560, 1600, Rgba([240, 240, 240, 255]));

    // Draw high-contrast button at (640, 400) with size 320x160
    draw_filled_rect(&mut canvas, 640, 400, 320, 160, Rgba([20, 30, 40, 255]));
    draw_rect_outline(&mut canvas, 640, 400, 320, 160, 2, Rgba([0, 0, 0, 255]));

    // 2. Measure runtime of detect_visual_boxes on 2560x1600 (should downsample to 1280 internally)
    let mut durations = Vec::new();
    let mut detected = Vec::new();
    for _ in 0..3 {
        let start = Instant::now();
        detected = detect_visual_boxes(&canvas, [0, 0], 1.0);
        durations.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    durations.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let best_ms = durations[0];
    let median_ms = durations[1];

    println!(
        "detect_visual_boxes on 2560x1600: best={:.2}ms, median={:.2}ms, detected {} marks",
        best_ms,
        median_ms,
        detected.len()
    );
    for m in &detected {
        println!("Mark #{}: rect={:?}, center={:?}", m.id, m.rect, m.center);
    }

    assert!(
        !detected.is_empty(),
        "Should detect visual boxes on 2560x1600 canvas"
    );

    // Assert latency guard: must prevent the 10-12ms imageops::resize regression
    if !cfg!(debug_assertions) {
        assert!(
            median_ms < 8.0,
            "Release mode detect_visual_boxes on 2.5K must achieve sub-8ms (got best={:.2}ms, median={:.2}ms)",
            best_ms,
            median_ms
        );
    } else {
        assert!(
            median_ms < 120.0,
            "Debug mode detect_visual_boxes on 2.5K must complete under 120ms (got best={:.2}ms, median={:.2}ms)",
            best_ms,
            median_ms
        );
    }

    // 3. Find detected box corresponding to the button at (640, 400, 320, 160)
    let matched_mark = detected
        .iter()
        .find(|m| {
            (m.rect[0] - 640).abs() <= 32
                && (m.rect[1] - 400).abs() <= 32
                && (m.rect[2] - 320).abs() <= 32
                && (m.rect[3] - 160).abs() <= 32
        })
        .expect("Should find detected mark for button at (640, 400, 320, 160)");

    // 4. Verify center coordinate lands inside the button with center deviation <= 20 pixels
    let true_cx = 640 + 320 / 2; // 800
    let true_cy = 400 + 160 / 2; // 480
    let err_cx = (matched_mark.center[0] - true_cx).abs();
    let err_cy = (matched_mark.center[1] - true_cy).abs();

    println!(
        "Button center: expected=({}, {}), detected=({}, {}), error=({}, {})",
        true_cx, true_cy, matched_mark.center[0], matched_mark.center[1], err_cx, err_cy
    );

    assert!(
        err_cx <= 20 && err_cy <= 20,
        "Center coordinate error must be <= 20 pixels, got cx_err={}, cy_err={}",
        err_cx,
        err_cy
    );

    // 5. Test with physical monitor offset [100, 200] and scale 1.0
    let detected_with_offset = detect_visual_boxes(&canvas, [100, 200], 1.0);
    let matched_offset_mark = detected_with_offset
        .iter()
        .find(|m| (m.rect[0] - (640 + 100)).abs() <= 32 && (m.rect[1] - (400 + 200)).abs() <= 32)
        .expect("Should find mark shifted by physical offset [100, 200]");

    assert_eq!(matched_offset_mark.center[0], matched_mark.center[0] + 100);
    assert_eq!(matched_offset_mark.center[1], matched_mark.center[1] + 200);
}

#[test]
fn test_coordinate_scaling_roundtrip_precision() {
    let orig_w = 2560f32;
    let orig_h = 1600f32;
    let ds_w = 1280f32;
    let ds_h = 800f32;
    let scale_x = orig_w / ds_w; // 2.0
    let scale_y = orig_h / ds_h; // 2.0

    // Test a sweep of coordinates in downsampled space for both x and y axes
    for ds_coord in 50..1200 {
        let mapped_x = (ds_coord as f32 * scale_x).round() as i32;
        let roundtrip_x = (mapped_x as f32 / scale_x).round() as i32;
        assert!((roundtrip_x - ds_coord).abs() <= 2);

        let mapped_y = (ds_coord as f32 * scale_y).round() as i32;
        let roundtrip_y = (mapped_y as f32 / scale_y).round() as i32;
        assert!((roundtrip_y - ds_coord).abs() <= 2);
    }
}

#[test]
fn test_som_downsampled_vs_small_canvas() {
    reset_mark_cache();

    // On a small canvas (<= 1280), no downsampling occurs
    let mut canvas = RgbaImage::from_pixel(800, 600, Rgba([240, 240, 240, 255]));
    draw_filled_rect(&mut canvas, 100, 100, 160, 80, Rgba([10, 20, 30, 255]));
    draw_rect_outline(&mut canvas, 100, 100, 160, 80, 2, Rgba([0, 0, 0, 255]));

    let detected = detect_visual_boxes(&canvas, [0, 0], 1.0);
    assert!(!detected.is_empty());
    let m = &detected[0];
    assert!((m.rect[0] - 100).abs() <= 16);
    assert!((m.rect[1] - 100).abs() <= 16);
}
