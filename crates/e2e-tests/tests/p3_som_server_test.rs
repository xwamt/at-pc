//! Integration tests for Milestone 4 (P3 / v1.0.0):
//! Set-of-Mark (SoM) MCP tool schemas, RBAC authorization,
//! and end-to-end WebSocket tool invocation routing.

use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::tools::som::{reset_mark_cache, set_mock_screen_image, store_cached_marks};
use at_pc_agent::ws_client::AgentWsClient;
use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::{MarkedScreenResponse, ScreenMark, TerminalInfo};
use at_pc_server::config::{is_tool_allowed_for_role, Role, ServerConfig};
use at_pc_server::mcp::tools::get_mcp_tool_definitions;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::handler::{handle_stream, WsServerState};
use at_pc_server::ws::registry::TerminalRegistry;
use image::{Rgba, RgbaImage};

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

// =========================================================================
// 1. Verify MCP tool schemas in server definition
// =========================================================================
#[test]
fn test_mcp_tool_definitions_include_som_tools() {
    let tools = get_mcp_tool_definitions();

    let som_tools = ["get_marked_screen", "click_mark"];
    for name in &som_tools {
        let tool = tools
            .iter()
            .find(|t| t["name"] == *name)
            .unwrap_or_else(|| panic!("Tool '{}' not found in MCP tool definitions", name));

        assert!(!tool["description"].as_str().unwrap().is_empty());
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].get("terminal_id").is_some());
    }

    // Specific schema property checks for get_marked_screen
    let screen_tool = tools
        .iter()
        .find(|t| t["name"] == "get_marked_screen")
        .unwrap();
    let screen_props = &screen_tool["inputSchema"]["properties"];
    assert!(screen_props.get("strategy").is_some());
    assert!(screen_props.get("grid_divisions").is_some());
    assert!(screen_props.get("crop").is_some());
    assert!(screen_props.get("max_dimension").is_some());
    let strat_enum = screen_props["strategy"]["enum"].as_array().unwrap();
    assert!(strat_enum.iter().any(|v| v == "hybrid"));

    // Specific schema property checks for click_mark
    let click_tool = tools.iter().find(|t| t["name"] == "click_mark").unwrap();
    let click_req = click_tool["inputSchema"]["required"].as_array().unwrap();
    assert!(click_req.iter().any(|r| r == "mark_id"));
    let click_props = &click_tool["inputSchema"]["properties"];
    assert!(click_props.get("button").is_some());
    assert!(click_props.get("count").is_some());

    // Verify mouse_click schema also includes mark_id
    let mouse_tool = tools.iter().find(|t| t["name"] == "mouse_click").unwrap();
    assert!(mouse_tool["inputSchema"]["properties"]
        .get("mark_id")
        .is_some());
}

// =========================================================================
// 2. Verify RBAC tier classification and role enforcement
// =========================================================================
#[tokio::test]
async fn test_som_tools_rbac_permission_enforcement() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = McpRouter::new(registry.clone());

    // Viewer Role: ALLOWED on get_marked_screen, FORBIDDEN on click_mark
    assert!(
        is_tool_allowed_for_role(Role::Viewer, "get_marked_screen"),
        "Viewer must have access to diagnostic get_marked_screen"
    );
    assert!(
        !is_tool_allowed_for_role(Role::Viewer, "click_mark"),
        "Viewer must NOT have permission to click marks"
    );

    let viewer_click = router
        .dispatch_tool_call_with_role(
            "click_mark",
            json!({"mark_id": 1}),
            None,
            Some(Role::Viewer),
            Some("127.0.0.1"),
            Some("view***"),
        )
        .await;
    assert!(viewer_click.is_err());
    assert!(viewer_click
        .unwrap_err()
        .contains("Forbidden: Role 'viewer' is not authorized to execute tool 'click_mark'"));

    // 3. Operator Role: ALLOWED on get_marked_screen, FORBIDDEN on click_mark
    assert!(
        is_tool_allowed_for_role(Role::Operator, "get_marked_screen"),
        "Operator must have access to diagnostic get_marked_screen"
    );
    assert!(
        !is_tool_allowed_for_role(Role::Operator, "click_mark"),
        "Operator must NOT have permission to click marks"
    );

    let operator_click = router
        .dispatch_tool_call_with_role(
            "click_mark",
            json!({"mark_id": 1}),
            None,
            Some(Role::Operator),
            Some("127.0.0.1"),
            Some("oper***"),
        )
        .await;
    assert!(operator_click.is_err());
    assert!(operator_click
        .unwrap_err()
        .contains("Forbidden: Role 'operator' is not authorized to execute tool 'click_mark'"));

    // 4. Admin Role: ALLOWED on both
    assert!(is_tool_allowed_for_role(Role::Admin, "get_marked_screen"));
    assert!(is_tool_allowed_for_role(Role::Admin, "click_mark"));
}

// =========================================================================
// 3. Verify router forwards and correlates SoM tool calls
// =========================================================================
#[tokio::test]
async fn test_router_forwards_and_correlates_som_tool_calls() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let term = create_test_terminal("term-som-route", "TEST-SOM-HOST");
    registry.register(term, tx).await;

    // 1. Dispatch get_marked_screen
    let router_clone = router.clone();
    let invoke_handle = tokio::spawn(async move {
        router_clone
            .dispatch_tool_call(
                "get_marked_screen",
                json!({ "terminal_id": "term-som-route", "strategy": "hybrid" }),
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
            assert_eq!(tool_name, "get_marked_screen");
            assert_eq!(arguments["strategy"], "hybrid");

            router
                .handle_tool_result(AgentToServerMessage::ToolResult {
                    call_id,
                    success: true,
                    result: json!({
                        "display_index": 0,
                        "width": 1280,
                        "height": 720,
                        "format": "jpeg",
                        "base64_data": "data:image/jpeg;base64,dGVzdA==",
                        "marks": [
                            {"id": 1, "rect": [100, 100, 80, 40], "center": [140, 120], "label": "Save Button"}
                        ],
                        "total_marks": 1,
                        "source": "hybrid"
                    }),
                    error: None,
                    duration_ms: 15,
                })
                .await;
        }
        _ => panic!("Expected InvokeTool variant"),
    }

    let screen_res = invoke_handle
        .await
        .unwrap()
        .expect("Dispatch should succeed");
    assert_eq!(screen_res["total_marks"], 1);
    assert_eq!(screen_res["marks"][0]["id"], 1);
    assert_eq!(screen_res["marks"][0]["label"], "Save Button");

    // 2. Dispatch click_mark
    let router_clone2 = router.clone();
    let click_handle = tokio::spawn(async move {
        router_clone2
            .dispatch_tool_call(
                "click_mark",
                json!({ "terminal_id": "term-som-route", "mark_id": 1 }),
            )
            .await
    });

    let msg2 = rx
        .recv()
        .await
        .expect("Expected InvokeTool message for click_mark");
    match msg2 {
        ServerToAgentMessage::InvokeTool {
            call_id,
            tool_name,
            arguments,
            ..
        } => {
            assert_eq!(tool_name, "click_mark");
            assert_eq!(arguments["mark_id"], 1);

            router
                .handle_tool_result(AgentToServerMessage::ToolResult {
                    call_id,
                    success: true,
                    result: json!({
                        "success": true,
                        "action": "click_mark",
                        "mark_id": 1,
                        "coordinates": [140, 120]
                    }),
                    error: None,
                    duration_ms: 10,
                })
                .await;
        }
        _ => panic!("Expected InvokeTool variant"),
    }

    let click_res = click_handle
        .await
        .unwrap()
        .expect("Click dispatch should succeed");
    assert_eq!(click_res["success"], true);
    assert_eq!(click_res["action"], "click_mark");
    assert_eq!(click_res["mark_id"], 1);
}

// =========================================================================
// 4. End-to-end WebSocket tool invocation pipeline
// =========================================================================
#[tokio::test]
async fn test_e2e_ws_som_tool_invocation_pipeline() {
    reset_mark_cache();
    let mock = RgbaImage::from_pixel(640, 480, Rgba([245, 245, 245, 255]));
    set_mock_screen_image(Some(mock));

    let mark = ScreenMark {
        id: 55,
        rect: [150, 150, 100, 50],
        center: [200, 175],
        label: Some("E2E Button".to_string()),
        control_type: Some("Button".to_string()),
    };
    store_cached_marks(&[mark]);

    // 1. Setup Server and duplex stream
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let server_config = ServerConfig {
        ws_path: "/ws".to_string(),
        auth_token: None,
        heartbeat_interval_secs: 5,
        ..Default::default()
    };
    let server_state = WsServerState::with_handler(registry.clone(), server_config, router.clone());

    let (client_stream, server_stream) = tokio::io::duplex(16384);

    let _server_task = tokio::spawn(async move {
        handle_stream(server_stream, server_state).await;
    });

    // 2. Start Agent WS Client connected to duplex stream
    let agent_term = create_test_terminal("e2e-som-agent", "SOM-E2E-HOST");
    let agent_executor = Arc::new(AgentExecutor::new().with_computer_use(true));

    let ws_client = Arc::new(AgentWsClient::new(
        "ws://127.0.0.1:9801/ws".to_string(),
        agent_term,
        agent_executor,
    ));

    let client_clone = ws_client.clone();
    let _client_task = tokio::spawn(async move {
        let _ = client_clone
            .handshake_and_run_stream(client_stream, "127.0.0.1", "/ws")
            .await;
    });

    // Wait for agent to register
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 3. Dispatch get_marked_screen through McpRouter
    let screen_result = router
        .dispatch_tool_call(
            "get_marked_screen",
            json!({"terminal_id": "e2e-som-agent", "strategy": "grid", "grid_divisions": 2}),
        )
        .await
        .expect("get_marked_screen over WS should succeed");

    let screen_resp: MarkedScreenResponse = serde_json::from_value(screen_result).unwrap();
    assert_eq!(screen_resp.total_marks, 4);
    assert_eq!(screen_resp.width, 640);
    assert_eq!(screen_resp.height, 480);
    assert_eq!(screen_resp.source, "grid");

    // 4. Dispatch click_mark through McpRouter
    let click_result = router
        .dispatch_tool_call(
            "click_mark",
            json!({"terminal_id": "e2e-som-agent", "mark_id": 1}),
        )
        .await
        .expect("click_mark over WS should succeed");

    assert_eq!(click_result["success"], true);
    assert_eq!(click_result["action"], "click_mark");
    assert_eq!(click_result["mark_id"], 1);

    // 5. Dispatch mouse_click with mark_id through McpRouter
    let mouse_result = router
        .dispatch_tool_call(
            "mouse_click",
            json!({"terminal_id": "e2e-som-agent", "mark_id": 1, "button": "left"}),
        )
        .await
        .expect("mouse_click with mark_id over WS should succeed");

    assert_eq!(mouse_result["success"], true);
    assert_eq!(mouse_result["action"], "mouse_click");
    assert_eq!(mouse_result["mark_id"], 1);

    set_mock_screen_image(None);
}
