//! Integration and unit tests for Milestone 4 (P3 / v1.0.0):
//! Set-of-Mark (SoM) visual annotation engine, non-accessible / Canvas UI grounding fallback,
//! direct semantic clicking via `mark_id`, and closed-loop review integration.

use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::tools::computer_use::execute_mouse_click;
use at_pc_agent::tools::som::{
    annotate_image_with_marks, cached_marks_count, clear_and_store_marks, click_mark,
    detect_visual_boxes, draw_filled_rect, draw_rect_outline,
    generate_marked_screen_from_image, generate_marked_screen_from_image_ext,
    generate_marks_from_grid, generate_marks_from_ui_elements,
    get_cached_mark, get_marked_screen, reset_mark_cache, set_mock_screen_image, store_cached_marks,
};
use at_pc_agent::tools::uia::{click_element, reset_element_cache};
use at_pc_agent::tools::window::{
    reset_window_mocks, set_mock_active_window, WindowState,
};
use at_pc_protocol::models::{ScreenMark, UiElement};
use image::{DynamicImage, Rgba, RgbaImage};
use serde_json::json;

static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn test_mark_cache_crud_and_lookup() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();
    assert_eq!(cached_marks_count(), 0);

    let mark1 = ScreenMark {
        id: 10,
        rect: [100, 200, 150, 40],
        center: [175, 220],
        label: Some("Button: Submit".to_string()),
        control_type: Some("Button".to_string()),
    };
    let mark2 = ScreenMark {
        id: 20,
        rect: [300, 200, 200, 40],
        center: [400, 220],
        label: Some("Edit: Search".to_string()),
        control_type: Some("Edit".to_string()),
    };

    store_cached_marks(&[mark1.clone(), mark2.clone()]);
    assert_eq!(cached_marks_count(), 2);

    let retrieved1 = get_cached_mark(10).expect("Mark 10 should exist");
    assert_eq!(retrieved1, mark1);
    let retrieved2 = get_cached_mark(20).expect("Mark 20 should exist");
    assert_eq!(retrieved2, mark2);
    assert!(get_cached_mark(999).is_none());

    // Clear and store replacement
    let mark3 = ScreenMark {
        id: 30,
        rect: [0, 0, 50, 50],
        center: [25, 25],
        label: None,
        control_type: Some("Icon".to_string()),
    };
    clear_and_store_marks(&[mark3.clone()]);
    assert_eq!(cached_marks_count(), 1);
    assert!(get_cached_mark(10).is_none());
    assert_eq!(get_cached_mark(30), Some(mark3));

    reset_mark_cache();
    assert_eq!(cached_marks_count(), 0);
}

#[tokio::test]
async fn test_grid_marks_generation_math() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();

    // 4x4 grid over 1920x1080
    let marks = generate_marks_from_grid(1920, 1080, 4, [0, 0]);
    assert_eq!(marks.len(), 16);

    // First cell (R1C1)
    let cell0 = &marks[0];
    assert_eq!(cell0.rect, [0, 0, 480, 270]);
    assert_eq!(cell0.center, [240, 135]);
    assert_eq!(cell0.label, Some("Grid R1C1".to_string()));

    // Last cell (R4C4)
    let cell15 = &marks[15];
    assert_eq!(cell15.rect[0], 480 * 3);
    assert_eq!(cell15.rect[1], 270 * 3);
    assert_eq!(cell15.label, Some("Grid R4C4".to_string()));

    // Verify all marks have unique monotonically increasing IDs
    let mut ids: Vec<u32> = marks.iter().map(|m| m.id).collect();
    let original_len = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), original_len);
}

#[tokio::test]
async fn test_ui_elements_mark_generation() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();

    let elements = vec![
        // 1. Root desktop window (huge container -> should be pruned)
        UiElement {
            id: 1,
            control_type: "Window".to_string(),
            name: "Desktop".to_string(),
            value: None,
            rect: [0, 0, 1920, 1080],
            enabled: true,
            help_text: None,
        },
        // 2. Tiny decorator element (w < 12 -> should be pruned)
        UiElement {
            id: 2,
            control_type: "Separator".to_string(),
            name: "".to_string(),
            value: None,
            rect: [50, 50, 4, 100],
            enabled: true,
            help_text: None,
        },
        // 3. Actionable Button
        UiElement {
            id: 3,
            control_type: "Button".to_string(),
            name: "Save Document".to_string(),
            value: None,
            rect: [100, 120, 140, 36],
            enabled: true,
            help_text: None,
        },
        // 4. Actionable Edit input
        UiElement {
            id: 4,
            control_type: "Edit".to_string(),
            name: "Search Bar".to_string(),
            value: Some("query".to_string()),
            rect: [300, 120, 260, 36],
            enabled: true,
            help_text: None,
        },
    ];

    let marks = generate_marks_from_ui_elements(&elements, [0, 0, 1920, 1080]);
    assert_eq!(marks.len(), 2, "Expected exactly 2 actionable marks");

    let m_btn = &marks[0];
    assert_eq!(m_btn.rect, [100, 120, 140, 36]);
    assert_eq!(m_btn.center, [170, 138]);
    assert_eq!(m_btn.label, Some("Button: Save Document".to_string()));
    assert_eq!(m_btn.control_type, Some("Button".to_string()));

    let m_edit = &marks[1];
    assert_eq!(m_edit.rect, [300, 120, 260, 36]);
    assert_eq!(m_edit.center, [430, 138]);
    assert_eq!(m_edit.label, Some("Edit: Search Bar".to_string()));
}

#[tokio::test]
async fn test_visual_box_detection_on_synthetic_canvas() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();

    // Create a 640x480 light gray canvas representing an Electron/Canvas custom UI
    let mut canvas = RgbaImage::from_pixel(640, 480, Rgba([240, 240, 240, 255]));

    // Draw two distinct button-like dark boxes with high contrast
    // Box 1: Button at (100, 100), size 120x40
    draw_filled_rect(&mut canvas, 100, 100, 120, 40, Rgba([30, 30, 30, 255]));
    draw_rect_outline(&mut canvas, 100, 100, 120, 40, 2, Rgba([0, 0, 0, 255]));

    // Box 2: Button at (300, 250), size 160x50
    draw_filled_rect(&mut canvas, 300, 250, 160, 50, Rgba([20, 80, 200, 255]));
    draw_rect_outline(&mut canvas, 300, 250, 160, 50, 2, Rgba([10, 40, 150, 255]));

    let detected = detect_visual_boxes(&canvas, [0, 0], 1.0);
    assert!(!detected.is_empty(), "Should detect visual boxes on synthetic canvas");

    // Verify at least one detected box approximates Box 1
    let found_b1 = detected.iter().any(|m| {
        (m.rect[0] - 100).abs() <= 20
            && (m.rect[1] - 100).abs() <= 20
            && (m.rect[2] - 120).abs() <= 30
            && (m.rect[3] - 40).abs() <= 25
    });
    assert!(found_b1, "Box 1 should be detected, got: {:?}", detected);
}

#[tokio::test]
async fn test_annotate_image_with_marks_modifies_pixels() {
    let _guard = TEST_LOCK.lock().await;

    let mut img = RgbaImage::from_pixel(400, 300, Rgba([255, 255, 255, 255]));
    let mark = ScreenMark {
        id: 1,
        rect: [50, 50, 100, 60],
        center: [100, 80],
        label: Some("Button".to_string()),
        control_type: Some("Button".to_string()),
    };

    annotate_image_with_marks(&mut img, &[mark], [0, 0], Some(1.0));

    // 1. Inside the box: should have semi-transparent tint (not pure white)
    let center_px = img.get_pixel(100, 80);
    assert_ne!(center_px, &Rgba([255, 255, 255, 255]));

    // 2. On the border: should be solid outline
    let border_px = img.get_pixel(50, 70);
    assert_ne!(border_px, &Rgba([255, 255, 255, 255]));

    // 3. Inside the badge: should be solid badge background
    let badge_px = img.get_pixel(52, 42);
    assert_ne!(badge_px, &Rgba([255, 255, 255, 255]));

    // 4. Outside the box and badge: should remain untouched pure white
    let outside_px = img.get_pixel(10, 10);
    assert_eq!(outside_px, &Rgba([255, 255, 255, 255]));
}

#[tokio::test]
async fn test_generate_marked_screen_from_image_strategies_and_downsampling() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();

    let raw_img = RgbaImage::from_pixel(1920, 1080, Rgba([240, 240, 240, 255]));
    let dynamic_img = DynamicImage::ImageRgba8(raw_img);

    // 1. Test Grid strategy with 3x3 divisions and max_dimension downscaling to 960
    let res = generate_marked_screen_from_image(
        dynamic_img.clone(),
        Some("grid"),
        Some(3),
        None,
        "jpeg",
        85,
        Some(960),
        None,
        0,
    )
    .expect("generate_marked_screen_from_image failed");

    assert_eq!(res.width, 960);
    assert_eq!(res.height, 540);
    assert_eq!(res.original_width, Some(1920));
    assert_eq!(res.original_height, Some(1080));
    assert!(res.scale_factor.is_some());
    assert_eq!(res.total_marks, 9);
    assert_eq!(res.marks.len(), 9);
    assert_eq!(res.source, "grid");
    assert!(res.base64_data.starts_with("data:image/jpeg;base64,"));
    assert!(!res.raw_base64.is_empty());

    // Verify marks retained physical coordinates (not scaled down)
    assert_eq!(res.marks[0].rect[2], 1920 / 3);
    assert_eq!(res.marks[0].rect[3], 1080 / 3);

    // 2. Test ROI Cropping strategy
    let crop_res = generate_marked_screen_from_image(
        dynamic_img,
        Some("grid"),
        Some(2),
        None,
        "png",
        90,
        None,
        Some([200, 200, 400, 300]),
        0,
    )
    .expect("Crop generation failed");

    assert_eq!(crop_res.width, 400);
    assert_eq!(crop_res.height, 300);
    assert_eq!(crop_res.format, "png");
    assert!(crop_res.base64_data.starts_with("data:image/png;base64,"));
    assert_eq!(crop_res.total_marks, 4);
}

#[tokio::test]
async fn test_get_marked_screen_with_in_memory_mock() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();

    // Install synthetic 800x600 mock image (bypasses screen recording permissions)
    let mock = RgbaImage::from_pixel(800, 600, Rgba([200, 220, 240, 255]));
    set_mock_screen_image(Some(mock));

    let res = get_marked_screen(0, "jpeg", 80, None, None, Some("grid"), Some(4), None)
        .expect("get_marked_screen with mock image should succeed");

    assert_eq!(res.width, 800);
    assert_eq!(res.height, 600);
    assert_eq!(res.total_marks, 16);
    assert_eq!(cached_marks_count(), 16);

    // Clean up mock
    set_mock_screen_image(None);
}

#[tokio::test]
async fn test_click_mark_dispatch_and_execution() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();

    // 1. Cache a test mark
    let test_mark = ScreenMark {
        id: 42,
        rect: [100, 100, 80, 40],
        center: [140, 120],
        label: Some("Test Mark".to_string()),
        control_type: Some("Button".to_string()),
    };
    store_cached_marks(&[test_mark]);

    // 2. Click existing mark
    let click_res = click_mark(42, Some("left"), Some(1)).expect("click_mark should succeed");
    assert_eq!(click_res["success"], true);
    assert_eq!(click_res["action"], "click_mark");
    assert_eq!(click_res["mark_id"], 42);
    assert_eq!(click_res["coordinates"], json!([140, 120]));
    assert_eq!(click_res["button"], 0);

    // 3. Click non-existent mark -> returns descriptive error
    let err_res = click_mark(999, None, None);
    assert!(err_res.is_err());
    assert!(err_res.unwrap_err().contains("Mark #999 not found in mark cache"));
}

#[tokio::test]
async fn test_mouse_click_with_mark_id() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();

    let test_mark = ScreenMark {
        id: 7,
        rect: [250, 350, 100, 50],
        center: [300, 375],
        label: Some("OK Button".to_string()),
        control_type: Some("Button".to_string()),
    };
    store_cached_marks(&[test_mark]);

    // 1. mouse_click with integer mark_id
    let res = execute_mouse_click(&json!({
        "mark_id": 7,
        "button": "left"
    }))
    .expect("mouse_click with mark_id should succeed");

    assert_eq!(res["success"], true);
    assert_eq!(res["action"], "mouse_click");
    assert_eq!(res["mark_id"], 7);
    assert_eq!(res["x"], 300);
    assert_eq!(res["y"], 375);

    // 2. mouse_click with string mark "#7"
    let res_str = execute_mouse_click(&json!({
        "mark": "#7"
    }))
    .expect("mouse_click with string #7 should succeed");
    assert_eq!(res_str["mark_id"], 7);
}

#[tokio::test]
async fn test_click_element_fallback_to_mark_id() {
    let _guard = TEST_LOCK.lock().await;
    reset_element_cache();
    reset_mark_cache();

    let test_mark = ScreenMark {
        id: 15,
        rect: [400, 500, 120, 30],
        center: [460, 515],
        label: Some("Canvas Element".to_string()),
        control_type: Some("CustomControl".to_string()),
    };
    store_cached_marks(&[test_mark]);

    // When element is not in UIA cache, click_element falls back to SoM mark cache
    let res = click_element(15, None).expect("click_element should fallback to mark_id 15");
    assert_eq!(res["success"], true);
    assert_eq!(res["method"], "som_mark_click");
    assert_eq!(res["mark_id"], 15);
    assert_eq!(res["coordinates"], json!([460, 515]));
}

#[tokio::test]
async fn test_executor_som_dispatch_and_permission_toggle() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();
    let mock = RgbaImage::from_pixel(640, 480, Rgba([255, 255, 255, 255]));
    set_mock_screen_image(Some(mock));

    // 1. Without computer-use enabled: get_marked_screen (read-only) succeeds, click_mark fails
    let executor_disabled = AgentExecutor::new();
    assert!(!executor_disabled.enable_computer_use);

    let screen_res = executor_disabled
        .execute("get_marked_screen", json!({"strategy": "grid", "grid_divisions": 3}))
        .await;
    assert!(screen_res.is_ok(), "get_marked_screen should succeed even with computer_use disabled");

    let click_err = executor_disabled
        .execute("click_mark", json!({"mark_id": 1}))
        .await
        .unwrap_err();
    assert!(click_err.contains("Computer-use operations are disabled"));

    // 2. With computer-use enabled: click_mark succeeds
    let executor_enabled = AgentExecutor::new().with_computer_use(true);
    let click_res = executor_enabled
        .execute("click_mark", json!({"mark_id": 1}))
        .await;
    assert!(click_res.is_ok(), "click_mark should succeed when computer_use is enabled: {:?}", click_res);

    set_mock_screen_image(None);
}

#[tokio::test]
async fn test_executor_review_loop_attaches_state_diff_to_click_mark() {
    let _guard = TEST_LOCK.lock().await;
    reset_window_mocks();
    reset_mark_cache();

    let mark = ScreenMark {
        id: 99,
        rect: [100, 100, 50, 50],
        center: [125, 125],
        label: Some("Launch App".to_string()),
        control_type: Some("Button".to_string()),
    };
    store_cached_marks(&[mark]);

    // Initial window state
    set_mock_active_window(Some(WindowState {
        hwnd: 1001,
        title: "Main Dashboard".to_string(),
        is_dialog: false,
    }));

    let executor = AgentExecutor::new().with_computer_use(true);

    // Spawn task to simulate window focus change during action execution
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        set_mock_active_window(Some(WindowState {
            hwnd: 1002,
            title: "Settings Dialog".to_string(),
            is_dialog: true,
        }));
    });

    let res = executor
        .execute("click_mark", json!({"mark_id": 99}))
        .await
        .expect("click_mark execution failed");

    let diff = res.get("state_diff").expect("click_mark should attach state_diff on window switch");
    assert_eq!(diff["foreground_changed"], true);
    assert_eq!(diff["previous_window"], "Main Dashboard");
    assert_eq!(diff["current_window"], "Settings Dialog");
    assert_eq!(diff["modal_dialog_detected"], true);
    assert_eq!(diff["dialog_title"], "Settings Dialog");

    reset_window_mocks();
}

#[tokio::test]
async fn test_mouse_click_null_mark_id_fallback_to_coordinates() {
    let _guard = TEST_LOCK.lock().await;

    // 1. Calling mouse_click with mark_id: null should not error, but fallback to x, y click
    let res = execute_mouse_click(&json!({
        "mark_id": null,
        "x": 500,
        "y": 400,
        "button": "left"
    }));
    assert!(res.is_ok(), "mouse_click with null mark_id must succeed by falling through to coordinate click: {:?}", res.err());
    let val = res.unwrap();
    assert_eq!(val["success"], true);
    assert_eq!(val["action"], "mouse_click");
    assert_eq!(val["x"], 500);
    assert_eq!(val["y"], 400);

    // 2. Calling mouse_click with empty string mark should also fallback to x, y click
    let res_empty = execute_mouse_click(&json!({
        "mark": "",
        "x": 300,
        "y": 200
    }));
    assert!(res_empty.is_ok());
    assert_eq!(res_empty.unwrap()["x"], 300);

    // 3. Calling mouse_click with invalid mark format and no coordinates should error
    let res_err = execute_mouse_click(&json!({
        "mark_id": "not-a-number"
    }));
    assert!(res_err.is_err());
    assert!(res_err.unwrap_err().contains("Invalid 'mark_id' parameter format"));
}

#[tokio::test]
async fn test_click_element_invalid_action_type_error() {
    let _guard = TEST_LOCK.lock().await;
    reset_element_cache();
    reset_mark_cache();

    // Cache mark 77
    let test_mark = ScreenMark {
        id: 77,
        rect: [100, 100, 50, 50],
        center: [125, 125],
        label: Some("Fallback Mark".to_string()),
        control_type: Some("Button".to_string()),
    };
    store_cached_marks(&[test_mark]);

    // Invalid action_type should be rejected upfront before executing the click
    let err = click_element(77, Some("invalid_action_type"));
    assert!(err.is_err());
    assert!(err.unwrap_err().contains("Invalid action_type 'invalid_action_type'"));
}

#[tokio::test]
async fn test_click_mark_numeric_button_support() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();

    let mark = ScreenMark {
        id: 5,
        rect: [200, 200, 80, 40],
        center: [240, 220],
        label: Some("Context Menu Target".to_string()),
        control_type: Some("Button".to_string()),
    };
    store_cached_marks(&[mark]);

    // String "right"
    let r1 = click_mark(5, Some("right"), None).unwrap();
    assert_eq!(r1["button"], 2);

    // String "2" (numeric string)
    let r2 = click_mark(5, Some("2"), None).unwrap();
    assert_eq!(r2["button"], 2);

    // Middle click string "1"
    let r3 = click_mark(5, Some("1"), None).unwrap();
    assert_eq!(r3["button"], 1);

    // Default left click
    let r4 = click_mark(5, None, None).unwrap();
    assert_eq!(r4["button"], 0);

    // Executor dispatch with numeric button: 2
    let executor = AgentExecutor::new().with_computer_use(true);
    let r5 = executor.execute("click_mark", json!({"mark_id": 5, "button": 2})).await.unwrap();
    assert_eq!(r5["button"], 2);
}

#[tokio::test]
async fn test_hybrid_strategy_merges_ui_and_visual_marks() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();

    // Create 640x480 canvas
    let mut canvas = RgbaImage::from_pixel(640, 480, Rgba([250, 250, 250, 255]));

    // Draw visual box at bottom (300, 300, 150, 50)
    draw_filled_rect(&mut canvas, 300, 300, 150, 50, Rgba([20, 20, 20, 255]));
    draw_rect_outline(&mut canvas, 300, 300, 150, 50, 2, Rgba([0, 0, 0, 255]));

    // Native UI element at top (50, 50, 100, 30) - no overlap with visual box
    let ui_elem = UiElement {
        id: 1,
        control_type: "Button".to_string(),
        name: "Native Toolbar Button".to_string(),
        value: None,
        rect: [50, 50, 100, 30],
        enabled: true,
        help_text: None,
    };

    let res = generate_marked_screen_from_image_ext(
        DynamicImage::ImageRgba8(canvas),
        Some("hybrid"),
        None,
        Some(&[ui_elem]),
        "jpeg",
        80,
        None,
        None,
        0,
        None,
        None,
    )
    .expect("hybrid marking should succeed");

    assert_eq!(res.source, "hybrid");
    assert!(res.total_marks >= 2, "Expected both UI element and visual box marks in hybrid mode");
    assert_eq!(res.marks[0].id, 1);
    assert_eq!(res.marks[1].id, 2);
}

#[tokio::test]
async fn test_multi_monitor_offset_math() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();

    let raw_img = RgbaImage::from_pixel(1920, 1080, Rgba([255, 255, 255, 255]));
    let dynamic_img = DynamicImage::ImageRgba8(raw_img);

    // Second monitor at (1920, 0)
    let res = generate_marked_screen_from_image_ext(
        dynamic_img,
        Some("grid"),
        Some(2),
        None,
        "jpeg",
        80,
        None,
        None,
        1,
        None,
        Some([1920, 0]),
    )
    .expect("generation with monitor offset should succeed");

    assert_eq!(res.display_index, 1);
    assert_eq!(res.total_marks, 4);

    // Top-left cell on monitor 1 should start at x = 1920 in physical coordinates
    assert_eq!(res.marks[0].rect[0], 1920);
    assert_eq!(res.marks[0].rect[1], 0);
    assert_eq!(res.marks[0].center[0], 1920 + 1920 / 4);

    // Top-right cell on monitor 1 should start at x = 1920 + 960
    assert_eq!(res.marks[1].rect[0], 1920 + 960);
}

#[tokio::test]
async fn test_sequential_mark_numbering_across_repeated_captures() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();

    let mock = RgbaImage::from_pixel(640, 480, Rgba([255, 255, 255, 255]));
    set_mock_screen_image(Some(mock));

    // Capture 1
    let res1 = get_marked_screen(0, "jpeg", 80, None, None, Some("grid"), Some(2), None).unwrap();
    assert_eq!(res1.total_marks, 4);
    let ids1: Vec<u32> = res1.marks.iter().map(|m| m.id).collect();
    assert_eq!(ids1, vec![1, 2, 3, 4]);

    // Capture 2: marks must still start from 1, not balloon to 5, 6, 7, 8
    let res2 = get_marked_screen(0, "jpeg", 80, None, None, Some("grid"), Some(2), None).unwrap();
    assert_eq!(res2.total_marks, 4);
    let ids2: Vec<u32> = res2.marks.iter().map(|m| m.id).collect();
    assert_eq!(ids2, vec![1, 2, 3, 4], "Fresh screen capture marks must be sequentially numbered starting at 1");

    set_mock_screen_image(None);
}

#[tokio::test]
async fn test_single_actionable_element_retention() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();

    let canvas = RgbaImage::from_pixel(640, 480, Rgba([255, 255, 255, 255]));

    // Only 1 single actionable button
    let single_elem = UiElement {
        id: 1,
        control_type: "Button".to_string(),
        name: "OK".to_string(),
        value: None,
        rect: [250, 200, 100, 40],
        enabled: true,
        help_text: None,
    };

    let res = generate_marked_screen_from_image_ext(
        DynamicImage::ImageRgba8(canvas),
        Some("auto"),
        None,
        Some(&[single_elem]),
        "jpeg",
        80,
        None,
        None,
        0,
        None,
        None,
    )
    .expect("auto marking with single element should succeed");

    // Single button must NOT be discarded in favor of a 16-cell grid
    assert_eq!(res.source, "ui_tree");
    assert_eq!(res.total_marks, 1);
    assert_eq!(res.marks[0].id, 1);
    assert_eq!(res.marks[0].label, Some("Button: OK".to_string()));
}

#[tokio::test]
async fn test_chromatic_contrast_edge_detection() {
    let _guard = TEST_LOCK.lock().await;
    reset_mark_cache();

    // Saturated green background: RGB(0, 100, 0) -> luminance ~58.7
    let mut canvas = RgbaImage::from_pixel(400, 300, Rgba([0, 100, 0, 255]));

    // Saturated red button: RGB(200, 0, 0) -> luminance ~59.8 (nearly identical luminance!)
    draw_filled_rect(&mut canvas, 100, 100, 120, 50, Rgba([200, 0, 0, 255]));

    let marks = detect_visual_boxes(&canvas, [0, 0], 1.0);
    assert!(
        !marks.is_empty(),
        "Chromatic contrast between saturated red and green must be detected despite similar luminance"
    );
}

