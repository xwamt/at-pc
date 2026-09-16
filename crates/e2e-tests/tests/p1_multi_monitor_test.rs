//! Integration tests for Milestone 2 (P1 / v0.7.5):
//! MCP interactive tools multi-monitor enhancement:
//! - `mouse_click`, `mouse_move`, `mouse_drag`, `mouse_scroll` with `display_index` local-to-global mapping
//! - `list_windows` with multi-monitor `display_index` attribution
//! - `get_ui_tree` with display attribution
//! - MCP schema verification for `display_index` on mouse tools

use serde_json::json;
use std::sync::Arc;

use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::tools::screen::{reset_mock_monitors, set_mock_monitors};
use at_pc_agent::tools::window::{reset_window_mocks, set_mock_windows};
use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::{MonitorInfo, TerminalInfo, UiTreeResponse, WindowInfo};
use at_pc_server::mcp::tools::get_mcp_tool_definitions;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::registry::TerminalRegistry;

static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn create_test_terminal(id: &str, hostname: &str) -> TerminalInfo {
    TerminalInfo {
        terminal_id: id.to_string(),
        hostname: hostname.to_string(),
        username: "test_user".to_string(),
        lan_ip: "127.0.0.1".to_string(),
        os_version: "macOS 15.0".to_string(),
        agent_version: "1.0.0".to_string(),
    }
}

fn create_dual_monitor_setup() -> Vec<MonitorInfo> {
    vec![
        MonitorInfo {
            display_index: 0,
            name: "Primary Built-in Display".to_string(),
            is_primary: true,
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 2.0,
        },
        MonitorInfo {
            display_index: 1,
            name: "Secondary DELL 4K Monitor".to_string(),
            is_primary: false,
            x: 1920,
            y: 0,
            width: 2560,
            height: 1440,
            scale_factor: 1.5,
        },
    ]
}

// =========================================================================
// 1. MCP Tool Schema: mouse tools include display_index
// =========================================================================
#[test]
fn test_mcp_tool_definitions_include_display_index_on_mouse_tools() {
    let tools = get_mcp_tool_definitions();

    let mouse_tools = ["mouse_click", "mouse_move", "mouse_drag", "mouse_scroll"];
    for tool_name in &mouse_tools {
        let tool = tools
            .iter()
            .find(|t| t["name"] == *tool_name)
            .unwrap_or_else(|| panic!("Tool '{}' not found in MCP definitions", tool_name));

        let props = &tool["inputSchema"]["properties"];
        assert!(
            props.get("display_index").is_some(),
            "Tool '{}' schema must include display_index property",
            tool_name
        );
        let prop = &props["display_index"];
        assert_eq!(prop["type"], "integer");
        assert!(
            prop["description"]
                .as_str()
                .unwrap()
                .contains("display index"),
            "Tool '{}' display_index description must mention display index",
            tool_name
        );
    }
}

// =========================================================================
// 2. Mouse actions with display_index translate local to global coordinates
// =========================================================================
#[tokio::test]
async fn test_mouse_actions_translate_local_to_global_with_display_index() {
    let _guard = TEST_LOCK.lock().await;
    reset_mock_monitors();
    set_mock_monitors(Some(create_dual_monitor_setup()));

    let executor = AgentExecutor::new().with_computer_use(true);

    // 2.1 mouse_click pixel mode on display 1: local (100, 200) -> global (1920+100=2020, 0+200=200)
    let click_res = executor
        .execute(
            "mouse_click",
            json!({
                "display_index": 1,
                "x": 100,
                "y": 200,
                "button": "left"
            }),
        )
        .await
        .expect("mouse_click on display 1 should succeed");

    assert_eq!(click_res["success"], true);
    assert_eq!(click_res["x"], 2020);
    assert_eq!(click_res["y"], 200);
    assert_eq!(click_res["display_index"], 1);

    // 2.2 mouse_click normalized_1000 mode on display 1 (2560x1440):
    // local (500, 500) -> 50% width = 1280, 50% height = 720
    // global -> x: 1920 + 1280 = 3200, y: 0 + 720 = 720
    let click_norm_res = executor
        .execute(
            "mouse_click",
            json!({
                "display_index": 1,
                "x": 500,
                "y": 500,
                "coord_mode": "normalized_1000",
                "button": "right"
            }),
        )
        .await
        .expect("mouse_click normalized_1000 should succeed");

    assert_eq!(click_norm_res["success"], true);
    assert_eq!(click_norm_res["x"], 3200);
    assert_eq!(click_norm_res["y"], 720);
    assert_eq!(click_norm_res["button"], 2);

    // 2.3 mouse_move on display 1: local (300, 400) -> global (2220, 400)
    let move_res = executor
        .execute(
            "mouse_move",
            json!({
                "display_index": 1,
                "x": 300,
                "y": 400
            }),
        )
        .await
        .expect("mouse_move on display 1 should succeed");

    assert_eq!(move_res["success"], true);
    assert_eq!(move_res["x"], 2220);
    assert_eq!(move_res["y"], 400);
    assert_eq!(move_res["display_index"], 1);

    // 2.4 mouse_drag on display 1: local (100, 150) to (500, 600)
    // -> global start (2020, 150) to end (2420, 600)
    let drag_res = executor
        .execute(
            "mouse_drag",
            json!({
                "display_index": 1,
                "start_x": 100,
                "start_y": 150,
                "end_x": 500,
                "end_y": 600
            }),
        )
        .await
        .expect("mouse_drag on display 1 should succeed");

    assert_eq!(drag_res["success"], true);
    assert_eq!(drag_res["start_x"], 2020);
    assert_eq!(drag_res["start_y"], 150);
    assert_eq!(drag_res["end_x"], 2420);
    assert_eq!(drag_res["end_y"], 600);
    assert_eq!(drag_res["display_index"], 1);

    // 2.5 mouse_scroll on display 1: local (250, 350) -> global (2170, 350)
    let scroll_res = executor
        .execute(
            "mouse_scroll",
            json!({
                "display_index": 1,
                "x": 250,
                "y": 350,
                "delta_y": -120
            }),
        )
        .await
        .expect("mouse_scroll on display 1 should succeed");

    assert_eq!(scroll_res["success"], true);
    assert_eq!(scroll_res["x"], 2170);
    assert_eq!(scroll_res["y"], 350);
    assert_eq!(scroll_res["delta_y"], -120);
    assert_eq!(scroll_res["display_index"], 1);

    reset_mock_monitors();
}

// =========================================================================
// 3. list_windows returns display_index for windows on different displays
// =========================================================================
#[tokio::test]
async fn test_list_windows_returns_display_index_for_multi_displays() {
    let _guard = TEST_LOCK.lock().await;
    reset_mock_monitors();
    set_mock_monitors(Some(create_dual_monitor_setup()));
    reset_window_mocks();

    // Window 1: On primary display (center: 100 + 400 = 500, 100 + 300 = 400 -> within display 0)
    // Window 2: On secondary display (center: 2000 + 400 = 2400, 100 + 300 = 400 -> within display 1)
    // Window 3: Outside any monitor bounds (center: 6000, 6000 -> None)
    let mock_list = vec![
        WindowInfo {
            hwnd: 1001,
            pid: 10,
            title: "Browser on Primary".to_string(),
            process_name: "chrome.exe".to_string(),
            is_minimized: false,
            is_foreground: true,
            rect: [100, 100, 800, 600],
            display_index: None,
        },
        WindowInfo {
            hwnd: 1002,
            pid: 20,
            title: "Editor on Secondary".to_string(),
            process_name: "code.exe".to_string(),
            is_minimized: false,
            is_foreground: false,
            rect: [2000, 100, 800, 600],
            display_index: None,
        },
        WindowInfo {
            hwnd: 1003,
            pid: 30,
            title: "Off-screen Window".to_string(),
            process_name: "tool.exe".to_string(),
            is_minimized: false,
            is_foreground: false,
            rect: [5800, 5800, 400, 400],
            display_index: None,
        },
    ];

    set_mock_windows(Some(mock_list));

    let executor = AgentExecutor::new().with_computer_use(true);
    let res = executor
        .execute("list_windows", json!({ "only_visible": false }))
        .await
        .expect("list_windows should succeed");

    let windows: Vec<WindowInfo> =
        serde_json::from_value(res).expect("Should deserialize as Vec<WindowInfo>");
    assert_eq!(windows.len(), 3);

    // Verify window on display 0
    let win0 = windows.iter().find(|w| w.hwnd == 1001).unwrap();
    assert_eq!(win0.display_index, Some(0));

    // Verify window on display 1
    let win1 = windows.iter().find(|w| w.hwnd == 1002).unwrap();
    assert_eq!(win1.display_index, Some(1));

    // Verify off-screen window
    let win2 = windows.iter().find(|w| w.hwnd == 1003).unwrap();
    assert_eq!(win2.display_index, None);

    reset_window_mocks();
    reset_mock_monitors();
}

// =========================================================================
// 4. get_ui_tree response contains display_index
// =========================================================================
#[tokio::test]
async fn test_get_ui_tree_response_contains_display_index() {
    let _guard = TEST_LOCK.lock().await;
    reset_mock_monitors();
    set_mock_monitors(Some(create_dual_monitor_setup()));

    // Test find_display_index_for_bounds directly
    let bounds_disp0 = [100, 100, 800, 600]; // Center: (500, 400) -> Disp 0
    let bounds_disp1 = [2000, 100, 800, 600]; // Center: (2400, 400) -> Disp 1
    let bounds_outside = [6000, 6000, 400, 400]; // Center: (6200, 6200) -> None

    assert_eq!(
        at_pc_agent::tools::uia::find_display_index_for_bounds(&bounds_disp0),
        Some(0)
    );
    assert_eq!(
        at_pc_agent::tools::uia::find_display_index_for_bounds(&bounds_disp1),
        Some(1)
    );
    assert_eq!(
        at_pc_agent::tools::uia::find_display_index_for_bounds(&bounds_outside),
        None
    );

    let executor = AgentExecutor::new().with_computer_use(true);
    let res = executor
        .execute("get_ui_tree", json!({ "depth": 2 }))
        .await
        .expect("get_ui_tree should succeed");

    let tree: UiTreeResponse =
        serde_json::from_value(res).expect("Should deserialize as UiTreeResponse");
    // Fallback bounds or desktop bounds resolve to a valid display
    assert!(tree.display_index.is_some());
    assert_eq!(tree.display_index, Some(0));

    reset_mock_monitors();
}

// =========================================================================
// 5. Router forwards tool calls with display_index end-to-end
// =========================================================================
#[tokio::test]
async fn test_router_forwards_mouse_click_with_display_index() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let term = create_test_terminal("term-p1-route", "TEST-P1-HOST");
    registry.register(term, tx).await;

    let router_clone = router.clone();
    let invoke_handle = tokio::spawn(async move {
        router_clone
            .dispatch_tool_call(
                "mouse_click",
                json!({
                    "terminal_id": "term-p1-route",
                    "display_index": 1,
                    "x": 200,
                    "y": 300,
                    "button": "left"
                }),
            )
            .await
    });

    let msg = rx.recv().await.expect("Expected InvokeTool message");
    match msg {
        ServerToAgentMessage::InvokeTool {
            call_id,
            tool_name,
            arguments,
            ..
        } => {
            assert_eq!(tool_name, "mouse_click");
            assert_eq!(arguments["display_index"], 1);
            assert_eq!(arguments["x"], 200);
            assert_eq!(arguments["y"], 300);

            router
                .handle_tool_result(AgentToServerMessage::ToolResult {
                    call_id,
                    success: true,
                    result: json!({
                        "success": true,
                        "action": "mouse_click",
                        "x": 2120,
                        "y": 300,
                        "display_index": 1,
                        "button": 0,
                        "count": 1
                    }),
                    error: None,
                    duration_ms: 12,
                })
                .await;
        }
        _ => panic!("Expected InvokeTool variant"),
    }

    let result_val = invoke_handle
        .await
        .unwrap()
        .expect("Dispatch should succeed");
    assert_eq!(result_val["success"], true);
    assert_eq!(result_val["display_index"], 1);
    assert_eq!(result_val["x"], 2120);
    assert_eq!(result_val["y"], 300);
}
