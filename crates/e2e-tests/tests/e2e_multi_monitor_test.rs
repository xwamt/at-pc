//! Milestone 4 (P3-2): Server E2E MCP Full-Pipeline Regression Test Suite
//! Validates full closed-loop multi-monitor operations across client-server WebSocket duplex connection:
//! - Step 1: list_monitors -> verify multiple monitors returned with bounds and primary flags
//! - Step 2: capture_screen with display_index: 1 -> verify screenshot returned with display_index: 1
//! - Step 3: get_marked_screen with display_index: 1 -> verify marks with display_index: 1 offset by monitor 1 origin
//! - Step 4: click_mark -> verify mark clicked successfully
//! - Step 5: mouse_click with display_index: 1, x: 50, y: 50 -> verify successfully routed and translated
//! - Step 6: list_windows -> verify windows returned with appropriate display_index attribution
//! - Step 7: JSON-RPC MCP protocol regression for multi-monitor tool invocations
//! - Step 8: Negative origin dual-monitor E2E regression over WebSocket

use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;

use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::tools::screen::{reset_mock_monitors, set_mock_monitors};
use at_pc_agent::tools::som::reset_mark_cache;
use at_pc_agent::tools::window::{reset_window_mocks, set_mock_windows};
use at_pc_agent::ws_client::AgentWsClient;
use at_pc_protocol::models::{MonitorInfo, TerminalInfo, WindowInfo};
use at_pc_server::config::ServerConfig;
use at_pc_server::mcp::handle_jsonrpc_request;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::handler::WsServerState;
use at_pc_server::ws::registry::TerminalRegistry;

static TEST_LOCK: Mutex<()> = Mutex::const_new(());

/// Helper harness that links server and agent via an in-memory duplex channel.
struct TestCluster {
    #[allow(dead_code)]
    pub registry: Arc<TerminalRegistry>,
    pub router: Arc<McpRouter>,
    pub server_state: WsServerState,
}

impl TestCluster {
    fn new() -> Self {
        let registry = Arc::new(TerminalRegistry::new());
        let router = Arc::new(McpRouter::new(registry.clone()));
        let config = ServerConfig {
            ws_path: "/ws".to_string(),
            heartbeat_interval_secs: 5,
            ..Default::default()
        };
        let server_state = WsServerState {
            registry: registry.clone(),
            config,
            message_handler: Some(router.clone()),
        };
        Self {
            registry,
            router,
            server_state,
        }
    }

    /// Attaches an agent to the test cluster via a full in-memory duplex stream.
    fn attach_agent(&self, agent: Arc<AgentWsClient>) {
        let state = self.server_state.clone();
        let (client_stream, server_stream) = tokio::io::duplex(65536);

        tokio::spawn(async move {
            at_pc_server::ws::handler::handle_stream(server_stream, state).await;
        });

        tokio::spawn(async move {
            let _ = agent
                .handshake_and_run_stream(client_stream, "localhost", "/ws")
                .await;
        });
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
            scale_factor: 1.0,
        },
        MonitorInfo {
            display_index: 1,
            name: "Secondary DELL 2K Monitor".to_string(),
            is_primary: false,
            x: 1920,
            y: 0,
            width: 2560,
            height: 1440,
            scale_factor: 1.25,
        },
    ]
}

fn create_negative_origin_setup() -> Vec<MonitorInfo> {
    vec![
        MonitorInfo {
            display_index: 0,
            name: "Primary Central Display".to_string(),
            is_primary: true,
            x: 0,
            y: 0,
            width: 2560,
            height: 1440,
            scale_factor: 1.0,
        },
        MonitorInfo {
            display_index: 1,
            name: "Secondary Portrait Left Display".to_string(),
            is_primary: false,
            x: -1080,
            y: 0,
            width: 1080,
            height: 1920,
            scale_factor: 1.0,
        },
    ]
}

fn create_test_windows() -> Vec<WindowInfo> {
    vec![
        WindowInfo {
            hwnd: 101,
            pid: 2001,
            title: "VS Code Main Workspace".to_string(),
            process_name: "code.exe".to_string(),
            is_minimized: false,
            is_foreground: true,
            rect: [100, 100, 1200, 800], // center: (700, 500) -> Mon 0
            display_index: None,
        },
        WindowInfo {
            hwnd: 102,
            pid: 2002,
            title: "Chrome Secondary Monitor".to_string(),
            process_name: "chrome.exe".to_string(),
            is_minimized: false,
            is_foreground: false,
            rect: [2100, 200, 1000, 700], // center: (2600, 550) -> Mon 1
            display_index: None,
        },
        WindowInfo {
            hwnd: 103,
            pid: 2003,
            title: "Offscreen Background Worker".to_string(),
            process_name: "daemon.exe".to_string(),
            is_minimized: false,
            is_foreground: false,
            rect: [90000, 90000, 400, 400], // center: (90200, 90200) -> Off-screen
            display_index: None,
        },
    ]
}

// =========================================================================
// Full Closed-Loop E2E Multi-Monitor Workflow (Steps 1 to 6)
// =========================================================================
#[tokio::test]
async fn test_full_closed_loop_e2e_multi_monitor_workflow() {
    let _guard = TEST_LOCK.lock().await;
    reset_mock_monitors();
    reset_window_mocks();
    reset_mark_cache();

    // 0. Setup mock environment
    set_mock_monitors(Some(create_dual_monitor_setup()));
    set_mock_windows(Some(create_test_windows()));

    let cluster = TestCluster::new();
    let router = cluster.router.clone();

    // Connect Agent to Server via in-memory duplex channel
    let terminal_id = "term-e2e-multimonitor-01";
    let agent = Arc::new(AgentWsClient::new(
        "ws://localhost/ws".to_string(),
        TerminalInfo {
            terminal_id: terminal_id.to_string(),
            hostname: "OPS-DUAL-RIG".to_string(),
            username: "sysadmin".to_string(),
            lan_ip: "10.0.1.88".to_string(),
            os_version: "Windows 11 Enterprise".to_string(),
            agent_version: "1.0.0".to_string(),
        },
        Arc::new(AgentExecutor::new().with_computer_use(true)),
    ));
    cluster.attach_agent(agent.clone());

    // Wait for agent registration
    let mut ready = false;
    for _ in 0..40 {
        sleep(Duration::from_millis(50)).await;
        let terms = router.list_terminals().await;
        if terms.iter().any(|t| t.info.terminal_id == terminal_id) {
            ready = true;
            break;
        }
    }
    assert!(ready, "Agent failed to register in registry");
    router.select_terminal(terminal_id).await.unwrap();

    // ---------------------------------------------------------------------
    // Step 1: `list_monitors` -> verify multiple monitors returned with proper fields
    // ---------------------------------------------------------------------
    let mon_val = router
        .dispatch_tool_call("list_monitors", json!({}))
        .await
        .expect("Step 1: list_monitors failed");
    let monitors = mon_val.as_array().expect("list_monitors must return array");
    assert_eq!(monitors.len(), 2, "Expected exactly 2 monitors");

    // Monitor 0 validation
    assert_eq!(monitors[0]["display_index"], 0);
    assert_eq!(monitors[0]["is_primary"], true);
    assert_eq!(monitors[0]["x"], 0);
    assert_eq!(monitors[0]["y"], 0);
    assert_eq!(monitors[0]["width"], 1920);
    assert_eq!(monitors[0]["height"], 1080);
    assert_eq!(monitors[0]["scale_factor"], 1.0);

    // Monitor 1 validation
    assert_eq!(monitors[1]["display_index"], 1);
    assert_eq!(monitors[1]["is_primary"], false);
    assert_eq!(monitors[1]["x"], 1920);
    assert_eq!(monitors[1]["y"], 0);
    assert_eq!(monitors[1]["width"], 2560);
    assert_eq!(monitors[1]["height"], 1440);
    assert_eq!(monitors[1]["scale_factor"], 1.25);

    // ---------------------------------------------------------------------
    // Step 2: `capture_screen` with `display_index: 1` -> verify screenshot of monitor 1
    // ---------------------------------------------------------------------
    let cap_val = router
        .dispatch_tool_call(
            "capture_screen",
            json!({ "display_index": 1, "format": "jpeg" }),
        )
        .await
        .expect("Step 2: capture_screen on monitor 1 failed");
    assert_eq!(
        cap_val["display_index"], 1,
        "capture_screen must return display_index: 1"
    );
    let orig_w = cap_val["original_width"]
        .as_u64()
        .or_else(|| cap_val["width"].as_u64())
        .unwrap_or(0);
    assert_eq!(
        orig_w, 2560,
        "Expected width matching monitor 1 width"
    );
    let orig_h = cap_val["original_height"]
        .as_u64()
        .or_else(|| cap_val["height"].as_u64())
        .unwrap_or(0);
    assert_eq!(
        orig_h, 1440,
        "Expected height matching monitor 1 height"
    );
    assert_eq!(cap_val["format"], "jpeg");
    let base64_str = cap_val["base64_data"].as_str().unwrap_or("");
    assert!(
        base64_str.starts_with("data:image/jpeg;base64,"),
        "Invalid data URI format"
    );

    // ---------------------------------------------------------------------
    // Step 3: `get_marked_screen` with `display_index: 1`
    // -> verify marked response with display_index: 1 and marks offset by monitor 1 origin (1920)
    // ---------------------------------------------------------------------
    let marked_val = router
        .dispatch_tool_call(
            "get_marked_screen",
            json!({
                "display_index": 1,
                "strategy": "grid",
                "grid_divisions": 3
            }),
        )
        .await
        .expect("Step 3: get_marked_screen on monitor 1 failed");

    assert_eq!(
        marked_val["display_index"], 1,
        "get_marked_screen must return display_index: 1"
    );
    let marks = marked_val["marks"]
        .as_array()
        .expect("Marks array must be present");
    assert_eq!(marks.len(), 9, "Grid 3x3 strategy should generate 9 marks");

    // Verify all marks are properly offset by Monitor 1's origin (x >= 1920)
    for m in marks {
        let center_x = m["center"][0].as_i64().expect("Center x should be integer");
        let rect_x = m["rect"][0].as_i64().expect("Rect x should be integer");
        assert!(
            center_x >= 1920,
            "Mark center x ({}) must be >= monitor 1 origin (1920)",
            center_x
        );
        assert!(
            rect_x >= 1920,
            "Mark rect x ({}) must be >= monitor 1 origin (1920)",
            rect_x
        );
    }

    // ---------------------------------------------------------------------
    // Step 4: `click_mark` -> verify mark clicked successfully
    // ---------------------------------------------------------------------
    let target_mark_id = marks[4]["id"].as_u64().expect("Mark ID required") as u32;
    let expected_center_x = marks[4]["center"][0].as_i64().unwrap();
    let expected_center_y = marks[4]["center"][1].as_i64().unwrap();

    let click_mark_res = router
        .dispatch_tool_call(
            "click_mark",
            json!({
                "mark_id": target_mark_id,
                "button": "left",
                "count": 1
            }),
        )
        .await
        .expect("Step 4: click_mark failed");

    assert_eq!(click_mark_res["success"], true);
    assert_eq!(click_mark_res["action"], "click_mark");
    assert_eq!(click_mark_res["mark_id"], target_mark_id);
    assert_eq!(click_mark_res["coordinates"][0], expected_center_x);
    assert_eq!(click_mark_res["coordinates"][1], expected_center_y);

    // ---------------------------------------------------------------------
    // Step 5: `mouse_click` with `display_index: 1, x: 50, y: 50`
    // -> verify successfully routed and translated to global coordinates (1920 + 50 = 1970, 50)
    // ---------------------------------------------------------------------
    let mouse_click_res = router
        .dispatch_tool_call(
            "mouse_click",
            json!({
                "display_index": 1,
                "x": 50,
                "y": 50,
                "coord_mode": "pixel"
            }),
        )
        .await
        .expect("Step 5: mouse_click failed");

    assert_eq!(mouse_click_res["success"], true);
    assert_eq!(mouse_click_res["action"], "mouse_click");
    assert_eq!(mouse_click_res["display_index"], 1);
    assert_eq!(mouse_click_res["x"], 1920 + 50); // 1970
    assert_eq!(mouse_click_res["y"], 50);

    // ---------------------------------------------------------------------
    // Step 6: `list_windows` -> verify windows returned with appropriate display_index
    // ---------------------------------------------------------------------
    let win_val = router
        .dispatch_tool_call("list_windows", json!({ "only_visible": false }))
        .await
        .expect("Step 6: list_windows failed");

    let windows = win_val.as_array().expect("list_windows must return array");
    assert_eq!(windows.len(), 3);

    let win_main = windows.iter().find(|w| w["hwnd"] == 101).unwrap();
    assert_eq!(
        win_main["display_index"], 0,
        "VS Code Main must be assigned display_index 0"
    );

    let win_sec = windows.iter().find(|w| w["hwnd"] == 102).unwrap();
    assert_eq!(
        win_sec["display_index"], 1,
        "Chrome Secondary must be assigned display_index 1"
    );

    let win_off = windows.iter().find(|w| w["hwnd"] == 103).unwrap();
    assert!(
        win_off.get("display_index").is_none() || win_off["display_index"].is_null(),
        "Offscreen window should have null/None display_index"
    );

    // Clean up
    agent.disconnect("e2e test completed").await;
    reset_window_mocks();
    reset_mock_monitors();
    reset_mark_cache();
}

// =========================================================================
// Step 7: MCP JSON-RPC Protocol Regression for Multi-Monitor Tools
// =========================================================================
#[tokio::test]
async fn test_mcp_jsonrpc_multi_monitor_regression() {
    let _guard = TEST_LOCK.lock().await;
    reset_mock_monitors();
    reset_window_mocks();

    set_mock_monitors(Some(create_dual_monitor_setup()));

    let cluster = TestCluster::new();
    let router = cluster.router.clone();

    let terminal_id = "term-jsonrpc-multimon";
    let agent = Arc::new(AgentWsClient::new(
        "ws://localhost/ws".to_string(),
        TerminalInfo {
            terminal_id: terminal_id.to_string(),
            hostname: "JSONRPC-RIG".to_string(),
            username: "admin".to_string(),
            lan_ip: "10.0.1.99".to_string(),
            os_version: "macOS 15.0".to_string(),
            agent_version: "1.0.0".to_string(),
        },
        Arc::new(AgentExecutor::new().with_computer_use(true)),
    ));
    cluster.attach_agent(agent.clone());

    for _ in 0..40 {
        sleep(Duration::from_millis(50)).await;
        if router.list_terminals().await.len() == 1 {
            break;
        }
    }
    router.select_terminal(terminal_id).await.unwrap();

    // 7.1 JSON-RPC: list_monitors
    let rpc_list_monitors = json!({
        "jsonrpc": "2.0",
        "id": "req-monitors-01",
        "method": "tools/call",
        "params": {
            "name": "list_monitors",
            "arguments": {}
        }
    });
    let resp = handle_jsonrpc_request(&router, &rpc_list_monitors)
        .await
        .expect("JSON-RPC response expected");
    assert_eq!(resp["id"], "req-monitors-01");
    let content = &resp["result"]["content"][0]["text"];
    let content_json: serde_json::Value = serde_json::from_str(content.as_str().unwrap()).unwrap();
    assert_eq!(content_json.as_array().unwrap().len(), 2);

    // 7.2 JSON-RPC: mouse_click with display_index: 1
    let rpc_mouse_click = json!({
        "jsonrpc": "2.0",
        "id": "req-click-02",
        "method": "tools/call",
        "params": {
            "name": "mouse_click",
            "arguments": {
                "display_index": 1,
                "x": 200,
                "y": 300,
                "coord_mode": "pixel"
            }
        }
    });
    let resp2 = handle_jsonrpc_request(&router, &rpc_mouse_click)
        .await
        .expect("JSON-RPC response expected");
    assert_eq!(resp2["id"], "req-click-02");
    let content2 = &resp2["result"]["content"][0]["text"];
    let click_val: serde_json::Value = serde_json::from_str(content2.as_str().unwrap()).unwrap();
    assert_eq!(click_val["success"], true);
    assert_eq!(click_val["display_index"], 1);
    assert_eq!(click_val["x"], 1920 + 200); // 2120
    assert_eq!(click_val["y"], 300);

    agent.disconnect("jsonrpc test complete").await;
    reset_mock_monitors();
}

// =========================================================================
// Step 8: Negative Origin Dual-Monitor E2E Regression over WebSocket
// =========================================================================
#[tokio::test]
async fn test_negative_origin_dual_monitor_e2e_regression() {
    let _guard = TEST_LOCK.lock().await;
    reset_mock_monitors();
    reset_window_mocks();

    set_mock_monitors(Some(create_negative_origin_setup()));

    let cluster = TestCluster::new();
    let router = cluster.router.clone();

    let terminal_id = "term-negative-origin";
    let agent = Arc::new(AgentWsClient::new(
        "ws://localhost/ws".to_string(),
        TerminalInfo {
            terminal_id: terminal_id.to_string(),
            hostname: "NEG-ORIGIN-HOST".to_string(),
            username: "root".to_string(),
            lan_ip: "10.0.1.77".to_string(),
            os_version: "Linux 6.8".to_string(),
            agent_version: "1.0.0".to_string(),
        },
        Arc::new(AgentExecutor::new().with_computer_use(true)),
    ));
    cluster.attach_agent(agent.clone());

    for _ in 0..40 {
        sleep(Duration::from_millis(50)).await;
        if router.list_terminals().await.len() == 1 {
            break;
        }
    }
    router.select_terminal(terminal_id).await.unwrap();

    // 8.1 list_monitors on negative origin topology
    let mon_val = router
        .dispatch_tool_call("list_monitors", json!({}))
        .await
        .expect("list_monitors failed on negative origin setup");
    let monitors = mon_val.as_array().unwrap();
    assert_eq!(monitors[1]["display_index"], 1);
    assert_eq!(monitors[1]["x"], -1080);
    assert_eq!(monitors[1]["y"], 0);

    // 8.2 mouse_click on negative origin monitor (display_index: 1, x: 100, y: 150)
    // -> global coordinates (-1080 + 100 = -980, 150)
    let click_res = router
        .dispatch_tool_call(
            "mouse_click",
            json!({
                "display_index": 1,
                "x": 100,
                "y": 150,
                "coord_mode": "pixel"
            }),
        )
        .await
        .expect("mouse_click on negative origin monitor failed");

    assert_eq!(click_res["success"], true);
    assert_eq!(click_res["display_index"], 1);
    assert_eq!(click_res["x"], -980);
    assert_eq!(click_res["y"], 150);

    // 8.3 capture_screen with invalid display_index: 2 should return descriptive error
    let cap_err = router
        .dispatch_tool_call("capture_screen", json!({ "display_index": 2 }))
        .await;
    assert!(cap_err.is_err(), "Invalid display index 2 should error");
    let err_msg = cap_err.unwrap_err();
    assert!(
        err_msg.contains("Invalid display index 2"),
        "Error message should mention invalid display index: {}",
        err_msg
    );

    agent.disconnect("negative origin test complete").await;
    reset_mock_monitors();
}
