//! Milestone 4 (P3-1): Agent Virtual Multi-Monitor Unit Test Suite
//! Validates coordinate translation, drag operations, window attribution, and UIA display matching
//! across multiple multi-monitor topologies:
//! - Case 1: Horizontal Dual-Monitor (Monitor 0: 1920x1080 at (0,0); Monitor 1: 1920x1080 at (1920, 0))
//! - Case 2: Negative Origin Dual-Monitor (Monitor 0: 2560x1440 at (0,0); Monitor 1: 1080x1920 at (-1080, 0))
//! - Case 3: Mixed High-DPI Dual-Monitor (Monitor 0: 3840x2160 at (0,0) scale 2.0; Monitor 1: 1920x1080 at (3840, 0) scale 1.0)

use std::sync::Mutex;
use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::tools::screen::{reset_mock_monitors, set_mock_monitors};
use at_pc_agent::tools::uia::find_display_index_for_bounds;
use at_pc_agent::tools::window::{list_windows, reset_window_mocks, set_mock_windows};
use at_pc_protocol::models::{MonitorInfo, WindowInfo};
use serde_json::json;

static SUITE_LOCK: Mutex<()> = Mutex::new(());

/// Case 1: Horizontal Dual-Monitor
/// Monitor 0: 1920x1080 at (0, 0)
/// Monitor 1: 1920x1080 at (1920, 0)
fn create_horizontal_dual_topology() -> Vec<MonitorInfo> {
    vec![
        MonitorInfo {
            display_index: 0,
            name: "Monitor 0 (Primary Horizontal)".to_string(),
            is_primary: true,
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
        },
        MonitorInfo {
            display_index: 1,
            name: "Monitor 1 (Secondary Horizontal)".to_string(),
            is_primary: false,
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
        },
    ]
}

/// Case 2: Negative Origin Dual-Monitor
/// Monitor 0: 2560x1440 at (0, 0)
/// Monitor 1: 1080x1920 at (-1080, 0)
fn create_negative_origin_topology() -> Vec<MonitorInfo> {
    vec![
        MonitorInfo {
            display_index: 0,
            name: "Monitor 0 (Primary Main)".to_string(),
            is_primary: true,
            x: 0,
            y: 0,
            width: 2560,
            height: 1440,
            scale_factor: 1.0,
        },
        MonitorInfo {
            display_index: 1,
            name: "Monitor 1 (Secondary Portrait Left)".to_string(),
            is_primary: false,
            x: -1080,
            y: 0,
            width: 1080,
            height: 1920,
            scale_factor: 1.0,
        },
    ]
}

/// Case 3: Mixed High-DPI Dual-Monitor
/// Monitor 0: 3840x2160 at (0, 0) with scale 2.0
/// Monitor 1: 1920x1080 at (3840, 0) with scale 1.0
fn create_mixed_hidpi_topology() -> Vec<MonitorInfo> {
    vec![
        MonitorInfo {
            display_index: 0,
            name: "Monitor 0 (4K Retina 200%)".to_string(),
            is_primary: true,
            x: 0,
            y: 0,
            width: 3840,
            height: 2160,
            scale_factor: 2.0,
        },
        MonitorInfo {
            display_index: 1,
            name: "Monitor 1 (1080p Standard 100%)".to_string(),
            is_primary: false,
            x: 3840,
            y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
        },
    ]
}

fn create_sample_window(hwnd: usize, title: &str, rect: [i32; 4]) -> WindowInfo {
    WindowInfo {
        hwnd,
        pid: 1000 + hwnd as u32,
        title: title.to_string(),
        process_name: format!("{}.exe", title.to_lowercase()),
        is_minimized: false,
        is_foreground: false,
        rect,
        display_index: None,
    }
}

// =========================================================================
// 1. Case 1: Horizontal Dual-Monitor Local-to-Global Coordinate Translation
// =========================================================================
#[tokio::test]
async fn test_case1_horizontal_dual_monitor_coordinates() {
    let _guard = SUITE_LOCK.lock().unwrap();
    reset_mock_monitors();
    set_mock_monitors(Some(create_horizontal_dual_topology()));

    let executor = AgentExecutor::new().with_computer_use(true);

    // --- Monitor 0 (0, 0, 1920, 1080) ---
    // 1.1 Pixel mode
    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 0, "x": 100, "y": 200, "coord_mode": "pixel" }),
        )
        .await
        .unwrap();
    assert_eq!(res["success"], true);
    assert_eq!(res["x"], 100);
    assert_eq!(res["y"], 200);
    assert_eq!(res["display_index"], 0);

    let res = executor
        .execute(
            "mouse_move",
            json!({ "display_index": 0, "x": 500, "y": 600, "coord_mode": "pixel" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 500);
    assert_eq!(res["y"], 600);
    assert_eq!(res["display_index"], 0);

    // 1.2 Normalized mode (0..65535)
    // local (32767, 32767) -> (0 + (32767*1920)/65535, 0 + (32767*1080)/65535) = (959, 539)
    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 0, "x": 32767, "y": 32767, "coord_mode": "normalized" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 959);
    assert_eq!(res["y"], 539);

    // local (65535, 65535) -> (1920, 1080)
    let res = executor
        .execute(
            "mouse_move",
            json!({ "display_index": 0, "x": 65535, "y": 65535, "coord_mode": "normalized" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 1920);
    assert_eq!(res["y"], 1080);

    // 1.3 Normalized_1000 mode (0..1000)
    // local (500, 500) -> (960, 540)
    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 0, "x": 500, "y": 500, "coord_mode": "normalized_1000" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 960);
    assert_eq!(res["y"], 540);

    // --- Monitor 1 (1920, 0, 1920, 1080) ---
    // 1.4 Pixel mode
    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 1, "x": 100, "y": 200, "coord_mode": "pixel" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 1920 + 100); // 2020
    assert_eq!(res["y"], 200);
    assert_eq!(res["display_index"], 1);

    let res = executor
        .execute(
            "mouse_move",
            json!({ "display_index": 1, "x": 300, "y": 400, "coord_mode": "pixel" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 1920 + 300); // 2220
    assert_eq!(res["y"], 400);

    // 1.5 Normalized mode
    // local (0, 0) -> (1920, 0)
    let res = executor
        .execute(
            "mouse_move",
            json!({ "display_index": 1, "x": 0, "y": 0, "coord_mode": "normalized" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 1920);
    assert_eq!(res["y"], 0);

    // local (65535, 65535) -> (1920 + 1920 = 3840, 1080)
    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 1, "x": 65535, "y": 65535, "coord_mode": "normalized" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 3840);
    assert_eq!(res["y"], 1080);

    // 1.6 Normalized_1000 mode
    // local (500, 500) -> (1920 + 960 = 2880, 540)
    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 1, "x": 500, "y": 500, "coord_mode": "normalized_1000" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 2880);
    assert_eq!(res["y"], 540);

    reset_mock_monitors();
}

// =========================================================================
// 2. Case 2: Negative Origin Dual-Monitor Local-to-Global Coordinate Translation
// =========================================================================
#[tokio::test]
async fn test_case2_negative_origin_dual_monitor_coordinates() {
    let _guard = SUITE_LOCK.lock().unwrap();
    reset_mock_monitors();
    set_mock_monitors(Some(create_negative_origin_topology()));

    let executor = AgentExecutor::new().with_computer_use(true);

    // --- Monitor 0 (0, 0, 2560, 1440) ---
    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 0, "x": 500, "y": 500, "coord_mode": "pixel" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 500);
    assert_eq!(res["y"], 500);

    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 0, "x": 500, "y": 500, "coord_mode": "normalized_1000" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 1280);
    assert_eq!(res["y"], 720);

    // --- Monitor 1 (-1080, 0, 1080, 1920) (Negative origin on the left) ---
    // 2.1 Pixel mode with negative offset
    // local (100, 200) -> global x: -1080 + 100 = -980, y: 0 + 200 = 200
    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 1, "x": 100, "y": 200, "coord_mode": "pixel" }),
        )
        .await
        .unwrap();
    assert_eq!(res["success"], true);
    assert_eq!(res["x"], -980);
    assert_eq!(res["y"], 200);
    assert_eq!(res["display_index"], 1);

    // local (0, 0) -> global x: -1080, y: 0
    let res = executor
        .execute(
            "mouse_move",
            json!({ "display_index": 1, "x": 0, "y": 0, "coord_mode": "pixel" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], -1080);
    assert_eq!(res["y"], 0);

    // local (1080, 1920) -> global x: -1080 + 1080 = 0, y: 1920
    let res = executor
        .execute(
            "mouse_move",
            json!({ "display_index": 1, "x": 1080, "y": 1920, "coord_mode": "pixel" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 0);
    assert_eq!(res["y"], 1920);

    // 2.2 Normalized mode on negative offset monitor
    // local (0, 0) -> global (-1080, 0)
    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 1, "x": 0, "y": 0, "coord_mode": "normalized" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], -1080);
    assert_eq!(res["y"], 0);

    // local (65535, 65535) -> global (-1080 + 1080 = 0, 1920)
    let res = executor
        .execute(
            "mouse_move",
            json!({ "display_index": 1, "x": 65535, "y": 65535, "coord_mode": "normalized" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 0);
    assert_eq!(res["y"], 1920);

    // 2.3 Normalized_1000 mode on negative offset monitor
    // local (500, 500) -> x: -1080 + 540 = -540, y: 0 + 960 = 960
    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 1, "x": 500, "y": 500, "coord_mode": "normalized_1000" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], -540);
    assert_eq!(res["y"], 960);

    // local (0, 500) -> x: -1080, y: 960
    let res = executor
        .execute(
            "mouse_move",
            json!({ "display_index": 1, "x": 0, "y": 500, "coord_mode": "normalized_1000" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], -1080);
    assert_eq!(res["y"], 960);

    reset_mock_monitors();
}

// =========================================================================
// 3. Case 3: Mixed High-DPI Dual-Monitor Local-to-Global Coordinate Translation
// =========================================================================
#[tokio::test]
async fn test_case3_mixed_hidpi_dual_monitor_coordinates() {
    let _guard = SUITE_LOCK.lock().unwrap();
    reset_mock_monitors();
    set_mock_monitors(Some(create_mixed_hidpi_topology()));

    let executor = AgentExecutor::new().with_computer_use(true);

    // --- Monitor 0 (0, 0, 3840, 2160, scale 2.0) ---
    // 3.1 Pixel mode
    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 0, "x": 200, "y": 300, "coord_mode": "pixel" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 200);
    assert_eq!(res["y"], 300);

    // 3.2 Normalized_1000 mode: local (500, 500) -> (1920, 1080)
    let res = executor
        .execute(
            "mouse_move",
            json!({ "display_index": 0, "x": 500, "y": 500, "coord_mode": "normalized_1000" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 1920);
    assert_eq!(res["y"], 1080);

    // 3.3 Normalized mode: local (65535, 65535) -> (3840, 2160)
    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 0, "x": 65535, "y": 65535, "coord_mode": "normalized" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 3840);
    assert_eq!(res["y"], 2160);

    // --- Monitor 1 (3840, 0, 1920, 1080, scale 1.0) ---
    // 3.4 Pixel mode
    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 1, "x": 100, "y": 200, "coord_mode": "pixel" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 3840 + 100); // 3940
    assert_eq!(res["y"], 200);

    let res = executor
        .execute(
            "mouse_move",
            json!({ "display_index": 1, "x": 500, "y": 600, "coord_mode": "pixel" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 3840 + 500); // 4340
    assert_eq!(res["y"], 600);

    // 3.5 Normalized_1000 mode: local (500, 500) -> (3840 + 960 = 4800, 540)
    let res = executor
        .execute(
            "mouse_click",
            json!({ "display_index": 1, "x": 500, "y": 500, "coord_mode": "normalized_1000" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 4800);
    assert_eq!(res["y"], 540);

    // 3.6 Normalized mode: local (65535, 65535) -> (3840 + 1920 = 5760, 1080)
    let res = executor
        .execute(
            "mouse_move",
            json!({ "display_index": 1, "x": 65535, "y": 65535, "coord_mode": "normalized" }),
        )
        .await
        .unwrap();
    assert_eq!(res["x"], 5760);
    assert_eq!(res["y"], 1080);

    reset_mock_monitors();
}

// =========================================================================
// 4. Mouse Drag with display_index across Dual Monitors
// =========================================================================
#[tokio::test]
async fn test_mouse_drag_start_and_end_with_display_index() {
    let _guard = SUITE_LOCK.lock().unwrap();
    reset_mock_monitors();

    let executor = AgentExecutor::new().with_computer_use(true);

    // 4.1 Drag on Horizontal Secondary Monitor (Case 1)
    set_mock_monitors(Some(create_horizontal_dual_topology()));
    let res = executor
        .execute(
            "mouse_drag",
            json!({
                "display_index": 1,
                "start_x": 100,
                "start_y": 150,
                "end_x": 500,
                "end_y": 600,
                "coord_mode": "pixel"
            }),
        )
        .await
        .unwrap();
    assert_eq!(res["success"], true);
    assert_eq!(res["start_x"], 1920 + 100); // 2020
    assert_eq!(res["start_y"], 150);
    assert_eq!(res["end_x"], 1920 + 500); // 2420
    assert_eq!(res["end_y"], 600);
    assert_eq!(res["display_index"], 1);

    // 4.2 Drag with normalized_1000 on Horizontal Secondary Monitor
    let res = executor
        .execute(
            "mouse_drag",
            json!({
                "display_index": 1,
                "start_x": 100,
                "start_y": 100,
                "end_x": 900,
                "end_y": 900,
                "coord_mode": "normalized_1000"
            }),
        )
        .await
        .unwrap();
    assert_eq!(res["start_x"], 1920 + 192); // 2112
    assert_eq!(res["start_y"], 108);
    assert_eq!(res["end_x"], 1920 + 1728); // 3648
    assert_eq!(res["end_y"], 972);

    // 4.3 Drag on Negative Origin Monitor (Case 2)
    set_mock_monitors(Some(create_negative_origin_topology()));
    let res = executor
        .execute(
            "mouse_drag",
            json!({
                "display_index": 1,
                "start_x": 80,
                "start_y": 100,
                "end_x": 580,
                "end_y": 900,
                "coord_mode": "pixel"
            }),
        )
        .await
        .unwrap();
    assert_eq!(res["success"], true);
    assert_eq!(res["start_x"], -1080 + 80); // -1000
    assert_eq!(res["start_y"], 100);
    assert_eq!(res["end_x"], -1080 + 580); // -500
    assert_eq!(res["end_y"], 900);
    assert_eq!(res["display_index"], 1);

    reset_mock_monitors();
}

// =========================================================================
// 5. Window Management: list_windows Display Attribution
// =========================================================================
#[test]
fn test_list_windows_display_attribution_topologies() {
    let _guard = SUITE_LOCK.lock().unwrap();
    reset_mock_monitors();
    reset_window_mocks();

    // 5.1 Negative Origin Topology attribution
    // Mon 0: (0, 0, 2560, 1440), Mon 1: (-1080, 0, 1080, 1920)
    set_mock_monitors(Some(create_negative_origin_topology()));

    let windows = vec![
        // Window 1: inside Monitor 0 [100, 100, 800, 600] -> center (500, 400)
        create_sample_window(1, "VS Code on Primary", [100, 100, 800, 600]),
        // Window 2: inside Negative-Origin Monitor 1 [-800, 200, 600, 800] -> center (-500, 600)
        create_sample_window(2, "Terminal on Negative Left", [-800, 200, 600, 800]),
        // Window 3: off-screen completely [50000, 50000, 400, 400] -> center (50200, 50200)
        create_sample_window(3, "Offscreen Hidden Tool", [50000, 50000, 400, 400]),
    ];
    set_mock_windows(Some(windows));

    let listed = list_windows(false).expect("list_windows should succeed");
    assert_eq!(listed.len(), 3);

    let w1 = listed.iter().find(|w| w.hwnd == 1).unwrap();
    assert_eq!(w1.display_index, Some(0), "Window on Mon 0 should be attributed to display_index 0");

    let w2 = listed.iter().find(|w| w.hwnd == 2).unwrap();
    assert_eq!(w2.display_index, Some(1), "Window on negative Mon 1 should be attributed to display_index 1");

    let w3 = listed.iter().find(|w| w.hwnd == 3).unwrap();
    assert_eq!(w3.display_index, None, "Off-screen window should have None display_index");

    // 5.2 Horizontal Dual Topology attribution
    // Mon 0: (0, 0, 1920, 1080), Mon 1: (1920, 0, 1920, 1080)
    set_mock_monitors(Some(create_horizontal_dual_topology()));
    let windows_horiz = vec![
        create_sample_window(10, "Browser on Primary", [200, 200, 800, 600]),
        create_sample_window(20, "Grafana on Secondary", [2100, 200, 800, 600]),
        create_sample_window(30, "Virtual Space Window", [-5000, -5000, 300, 300]),
    ];
    set_mock_windows(Some(windows_horiz));

    let listed_h = list_windows(false).expect("list_windows should succeed");
    assert_eq!(listed_h.len(), 3);

    let wh0 = listed_h.iter().find(|w| w.hwnd == 10).unwrap();
    assert_eq!(wh0.display_index, Some(0));

    let wh1 = listed_h.iter().find(|w| w.hwnd == 20).unwrap();
    assert_eq!(wh1.display_index, Some(1));

    let wh_off = listed_h.iter().find(|w| w.hwnd == 30).unwrap();
    assert_eq!(wh_off.display_index, None);

    reset_window_mocks();
    reset_mock_monitors();
}

// =========================================================================
// 6. UIA Module: find_display_index_for_bounds
// =========================================================================
#[test]
fn test_find_display_index_for_bounds_uia_module() {
    let _guard = SUITE_LOCK.lock().unwrap();
    reset_mock_monitors();

    // 6.1 Horizontal Dual-Monitor
    set_mock_monitors(Some(create_horizontal_dual_topology()));

    // Center at (350, 300) -> on Monitor 0
    assert_eq!(find_display_index_for_bounds(&[100, 100, 500, 400]), Some(0));
    // Center at (2250, 300) -> on Monitor 1
    assert_eq!(find_display_index_for_bounds(&[2000, 100, 500, 400]), Some(1));
    // Center at (10050, 10050) -> Off-screen
    assert_eq!(find_display_index_for_bounds(&[10000, 10000, 100, 100]), None);

    // 6.2 Negative Origin Dual-Monitor
    set_mock_monitors(Some(create_negative_origin_topology()));

    // Center at (500, 400) -> on Monitor 0 (0..2560, 0..1440)
    assert_eq!(find_display_index_for_bounds(&[200, 200, 600, 400]), Some(0));
    // Center at (-700, 450) -> on Monitor 1 (-1080..0, 0..1920)
    assert_eq!(find_display_index_for_bounds(&[-900, 200, 400, 500]), Some(1));
    // Boundary check near top-left of negative monitor: rect [-1080, 0, 100, 100] -> center (-1030, 50)
    assert_eq!(find_display_index_for_bounds(&[-1080, 0, 100, 100]), Some(1));
    // Boundary check right outside left of negative monitor: rect [-1200, 0, 100, 100] -> center (-1150, 50)
    assert_eq!(find_display_index_for_bounds(&[-1200, 0, 100, 100]), None);

    // 6.3 Empty monitors returns None
    reset_mock_monitors();
    set_mock_monitors(Some(vec![]));
    assert_eq!(find_display_index_for_bounds(&[100, 100, 200, 200]), None);

    reset_mock_monitors();
}
