//! Integration test for Milestone 3 Task 11:
//! `capture_screen` observation slimming, default 1280 dimension cap,
//! deduplication of redundant base64 strings in `ScreenCaptureResult`,
//! and serialization compatibility.

use at_pc_agent::tools::screen::{capture_screen, process_dynamic_image, ScreenCaptureResult};
use at_pc_agent::tools::som::set_mock_screen_image;
use image::{DynamicImage, Rgba, RgbaImage};
use std::sync::{Arc, Mutex};

static TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn test_default_dimension_cap_and_uncapped() {
    // Synthetic 2560x1600 image
    let img = RgbaImage::from_pixel(2560, 1600, Rgba([100, 150, 200, 255]));
    let dynamic_img = DynamicImage::ImageRgba8(img);

    // 1. None -> defaults to 1280 max dimension
    let (scaled, crop_opt, scale_opt) = process_dynamic_image(dynamic_img.clone(), None, None);
    assert_eq!(crop_opt, None);
    assert_eq!(scaled.width(), 1280);
    assert_eq!(scaled.height(), 800);
    assert!(scale_opt.is_some());
    assert!((scale_opt.unwrap() - 0.5).abs() < 0.01);

    // 2. Explicit Some(0) -> uncapped (keeps 2560x1600)
    let (uncapped, crop_opt, scale_opt) = process_dynamic_image(dynamic_img.clone(), Some(0), None);
    assert_eq!(crop_opt, None);
    assert_eq!(uncapped.width(), 2560);
    assert_eq!(uncapped.height(), 1600);
    assert_eq!(scale_opt, None);

    // 3. Explicit Some(960) -> caps at 960
    let (custom, crop_opt, scale_opt) = process_dynamic_image(dynamic_img, Some(960), None);
    assert_eq!(crop_opt, None);
    assert_eq!(custom.width(), 960);
    assert_eq!(custom.height(), 600);
    assert!(scale_opt.is_some());
}

#[test]
fn test_capture_screen_default_cap_and_base64_arc_sharing() {
    let _guard = TEST_LOCK.lock().unwrap();
    // Generate a realistic desktop mock image (2560x1600):
    // Desktop wallpaper with application windows, toolbars, and taskbar
    let mut mock = RgbaImage::from_pixel(2560, 1600, Rgba([45, 55, 72, 255]));
    // Window 1: (200, 150) size 1600x1100
    for y in 150..1250 {
        for x in 200..1800 {
            mock.put_pixel(x, y, Rgba([245, 247, 250, 255]));
        }
    }
    // Window 1 Titlebar
    for y in 150..220 {
        for x in 200..1800 {
            mock.put_pixel(x, y, Rgba([30, 41, 59, 255]));
        }
    }
    // Window 2: (600, 400) size 1200x900
    for y in 400..1300 {
        for x in 600..1800 {
            mock.put_pixel(x, y, Rgba([255, 255, 255, 255]));
        }
    }
    // Taskbar at bottom
    for y in 1520..1600 {
        for x in 0..2560 {
            mock.put_pixel(x, y, Rgba([15, 23, 42, 255]));
        }
    }
    set_mock_screen_image(Some(mock));

    // Capture without passing max_dimension -> should default to 1280
    let res = capture_screen(0, "jpeg", 80, None, None, None)
        .expect("capture_screen should succeed with mock image");

    assert_eq!(res.width, 1280);
    assert_eq!(res.height, 800);
    assert_eq!(res.original_width, Some(2560));
    assert_eq!(res.original_height, Some(1600));
    assert!(res.scale_factor.is_some());

    // Payload size check: at 1280x800, base64 payload is significantly smaller than 120KB
    let payload_bytes = res.base64_data.len();
    assert!(
        payload_bytes < 120 * 1024,
        "Payload size was {} bytes, expected < 120KB",
        payload_bytes
    );

    // Memory deduplication check:
    // raw_base64 and image_base64 must share the same Arc<str>
    assert!(
        Arc::ptr_eq(&res.raw_base64, &res.image_base64),
        "raw_base64 and image_base64 should point to the same Arc allocation"
    );
    // base64_data and data_uri must share the same Arc<str>
    assert!(
        Arc::ptr_eq(&res.base64_data, &res.data_uri),
        "base64_data and data_uri should point to the same Arc allocation"
    );

    let prefix = "data:image/jpeg;base64,";
    assert_eq!(&res.base64_data[prefix.len()..], &*res.raw_base64);

    // Clean up mock
    set_mock_screen_image(None);
}

#[test]
fn test_screen_capture_result_serialization_compatibility() {
    let raw_b64: Arc<str> = Arc::from("dGVzdA==");
    let data_uri: Arc<str> = Arc::from("data:image/jpeg;base64,dGVzdA==");

    let capture_res = ScreenCaptureResult {
        display_index: 0,
        width: 1280,
        height: 720,
        format: "jpeg".to_string(),
        base64_data: Arc::clone(&data_uri),
        raw_base64: Arc::clone(&raw_b64),
        image_base64: Arc::clone(&raw_b64),
        data_uri,
        file_path: Some("/tmp/shot.jpg".to_string()),
        original_width: Some(1920),
        original_height: Some(1080),
        scale_factor: Some(0.6667),
        crop: None,
    };

    // Verify serialization to JSON
    let json_val = serde_json::to_value(&capture_res).expect("Serialization to JSON must succeed");
    assert_eq!(json_val["display_index"], 0);
    assert_eq!(json_val["width"], 1280);
    assert_eq!(json_val["height"], 720);
    assert_eq!(json_val["format"], "jpeg");
    assert_eq!(json_val["base64_data"], "data:image/jpeg;base64,dGVzdA==");
    assert_eq!(json_val["raw_base64"], "dGVzdA==");
    assert_eq!(json_val["image_base64"], "dGVzdA==");
    assert_eq!(json_val["data_uri"], "data:image/jpeg;base64,dGVzdA==");
    assert_eq!(json_val["file_path"], "/tmp/shot.jpg");
    assert_eq!(json_val["original_width"], 1920);
    assert_eq!(json_val["original_height"], 1080);

    // Verify deserialization from JSON
    let deserialized: ScreenCaptureResult =
        serde_json::from_value(json_val).expect("Deserialization from JSON must succeed");
    assert_eq!(deserialized, capture_res);
}

#[test]
fn test_uncapped_vs_capped_payload_reduction() {
    let _guard = TEST_LOCK.lock().unwrap();
    // Generate high-detail desktop image (2560x1600)
    let mut mock = RgbaImage::from_pixel(2560, 1600, Rgba([255, 255, 255, 255]));
    for y in 0..1600 {
        for x in 0..2560 {
            let val = ((x ^ y) & 0xFF) as u8;
            mock.put_pixel(
                x,
                y,
                Rgba([val, val.wrapping_mul(3), val.wrapping_add(50), 255]),
            );
        }
    }
    set_mock_screen_image(Some(mock));

    // Full 2560x1600 uncapped
    let uncapped = capture_screen(0, "jpeg", 80, None, Some(0), None)
        .expect("uncapped capture should succeed");
    let uncapped_size = uncapped.base64_data.len();

    // Default capped to 1280
    let capped =
        capture_screen(0, "jpeg", 80, None, None, None).expect("capped capture should succeed");
    let capped_size = capped.base64_data.len();

    assert_eq!(uncapped.width, 2560);
    assert_eq!(capped.width, 1280);

    // Verify substantial reduction (at least 2.5x to 4x reduction)
    let reduction_ratio = uncapped_size as f64 / capped_size as f64;
    println!(
        "Payload reduction: uncapped={} KB, capped={} KB (ratio: {:.2}x)",
        uncapped_size / 1024,
        capped_size / 1024,
        reduction_ratio
    );
    assert!(
        reduction_ratio >= 2.5,
        "Expected at least 2.5x payload reduction, got {:.2}x",
        reduction_ratio
    );

    set_mock_screen_image(None);
}
