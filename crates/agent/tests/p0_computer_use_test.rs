use at_pc_agent::executor::AgentExecutor;
use at_pc_protocol::models::DesktopInputEvent;
use serde_json::json;

#[test]
fn test_mouse_move_pixel_protocol_serialization() {
    let ev = DesktopInputEvent::MouseMovePixel { x: 1920, y: 1080 };
    let json_str = serde_json::to_string(&ev).unwrap();
    assert!(json_str.contains("MouseMovePixel"));
    assert!(json_str.contains("1920"));
    assert!(json_str.contains("1080"));

    let deser: DesktopInputEvent = serde_json::from_str(&json_str).unwrap();
    assert_eq!(
        deser,
        DesktopInputEvent::MouseMovePixel { x: 1920, y: 1080 }
    );
}

#[tokio::test]
async fn test_computer_use_mouse_tools_coord_modes() {
    let executor = AgentExecutor::new().with_computer_use(true);

    // 1. mouse_move with default pixel coord_mode
    let move_res = executor
        .execute(
            "mouse_move",
            json!({
                "x": 800,
                "y": 600
            }),
        )
        .await;
    assert!(move_res.is_ok(), "mouse_move failed: {:?}", move_res.err());
    let val = move_res.unwrap();
    assert_eq!(val["success"], true);
    assert_eq!(val["x"], 800);
    assert_eq!(val["y"], 600);

    // 2. mouse_click with normalized_1000 coord_mode
    let click_res = executor
        .execute(
            "mouse_click",
            json!({
                "x": 500,
                "y": 500,
                "coord_mode": "normalized_1000",
                "button": "left"
            }),
        )
        .await;
    assert!(
        click_res.is_ok(),
        "mouse_click failed: {:?}",
        click_res.err()
    );
    let click_val = click_res.unwrap();
    assert_eq!(click_val["success"], true);
    assert_eq!(click_val["button"], 0);

    // 3. mouse_drag with pixel coordinates
    let drag_res = executor
        .execute(
            "mouse_drag",
            json!({
                "start_x": 100,
                "start_y": 100,
                "end_x": 300,
                "end_y": 300,
                "coord_mode": "pixel"
            }),
        )
        .await;
    assert!(drag_res.is_ok(), "mouse_drag failed: {:?}", drag_res.err());
    let drag_val = drag_res.unwrap();
    assert_eq!(drag_val["success"], true);
    assert_eq!(drag_val["start_x"], 100);
    assert_eq!(drag_val["end_x"], 300);

    // 4. mouse_scroll with coordinates
    let scroll_res = executor
        .execute(
            "mouse_scroll",
            json!({
                "delta_y": -120,
                "x": 400,
                "y": 400
            }),
        )
        .await;
    assert!(
        scroll_res.is_ok(),
        "mouse_scroll failed: {:?}",
        scroll_res.err()
    );
    let scroll_val = scroll_res.unwrap();
    assert_eq!(scroll_val["success"], true);
    assert_eq!(scroll_val["delta_y"], -120);
}

#[tokio::test]
#[ignore = "requires physical display and screen recording permission"]
async fn test_capture_screen_downsampling_and_crop() {
    let executor = AgentExecutor::new();

    // 1. Base capture
    let base_res = executor.execute("capture_screen", json!({})).await;
    if let Err(e) = &base_res {
        // May fail in headless CI environments without a physical display
        eprintln!(
            "Skipping screen capture test in headless environment: {}",
            e
        );
        return;
    }
    let base_val = base_res.unwrap();
    let base_w = base_val["width"].as_u64().unwrap() as u32;
    let base_h = base_val["height"].as_u64().unwrap() as u32;
    assert!(base_w > 0 && base_h > 0);

    // 2. Downsampling with max_dimension
    let max_dim = 640;
    let downscaled_res = executor
        .execute(
            "capture_screen",
            json!({
                "max_dimension": max_dim
            }),
        )
        .await;
    assert!(downscaled_res.is_ok());
    let down_val = downscaled_res.unwrap();
    let down_w = down_val["width"].as_u64().unwrap() as u32;
    let down_h = down_val["height"].as_u64().unwrap() as u32;
    assert!(down_w <= max_dim);
    assert!(down_h <= max_dim);
    if base_w > max_dim || base_h > max_dim {
        assert!(down_val["original_width"].is_number());
        assert!(down_val["scale_factor"].is_number());
    }

    // 3. ROI Cropping
    let crop_res = executor
        .execute(
            "capture_screen",
            json!({
                "crop": [10, 10, 100, 80]
            }),
        )
        .await;
    assert!(crop_res.is_ok());
    let crop_val = crop_res.unwrap();
    let crop_w = crop_val["width"].as_u64().unwrap() as u32;
    let crop_h = crop_val["height"].as_u64().unwrap() as u32;
    assert_eq!(crop_w, 100);
    assert_eq!(crop_h, 80);
    assert_eq!(crop_val["crop"], json!([10, 10, 100, 80]));
}
