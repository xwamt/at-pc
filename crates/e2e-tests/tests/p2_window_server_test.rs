//! Integration tests for Milestone 3 (P2 / v0.6.0):
//! Window lifecycle management MCP tool exposure, RBAC authorization,
//! and end-to-end WebSocket tool invocation routing.

use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::tools::window::{reset_window_mocks, set_mock_windows};
use at_pc_agent::ws_client::AgentWsClient;
use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::{TerminalInfo, WindowInfo};
use at_pc_server::config::{is_tool_allowed_for_role, Role, ServerConfig};
use at_pc_server::mcp::tools::get_mcp_tool_definitions;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::handler::{handle_stream, WsServerState};
use at_pc_server::ws::registry::TerminalRegistry;

fn create_test_terminal(id: &str, hostname: &str) -> TerminalInfo {
    TerminalInfo {
        terminal_id: id.to_string(),
        hostname: hostname.to_string(),
        username: "test_user".to_string(),
        lan_ip: "127.0.0.1".to_string(),
        os_version: "macOS 15.0".to_string(),
        agent_version: "0.6.0".to_string(),
    }
}

// =========================================================================
// 1. Verify MCP tool schemas in server definition
// =========================================================================
#[test]
fn test_mcp_tool_definitions_include_window_tools() {
    let tools = get_mcp_tool_definitions();

    let window_tools = ["list_windows", "focus_window", "close_window"];
    for name in &window_tools {
        let tool = tools
            .iter()
            .find(|t| t["name"] == *name)
            .unwrap_or_else(|| panic!("Tool '{}' not found in MCP tool definitions", name));

        assert!(!tool["description"].as_str().unwrap().is_empty());
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].get("terminal_id").is_some());
    }

    // Specific schema property checks
    let list_tool = tools.iter().find(|t| t["name"] == "list_windows").unwrap();
    assert!(list_tool["inputSchema"]["properties"]
        .get("only_visible")
        .is_some());

    let focus_tool = tools.iter().find(|t| t["name"] == "focus_window").unwrap();
    assert!(focus_tool["inputSchema"]["properties"]
        .get("title")
        .is_some());
    assert!(focus_tool["inputSchema"]["properties"].get("pid").is_some());
    assert!(focus_tool["inputSchema"]["properties"]
        .get("hwnd")
        .is_some());

    let close_tool = tools.iter().find(|t| t["name"] == "close_window").unwrap();
    assert!(close_tool["inputSchema"]["properties"]
        .get("title")
        .is_some());
    assert!(close_tool["inputSchema"]["properties"].get("pid").is_some());
    assert!(close_tool["inputSchema"]["properties"]
        .get("hwnd")
        .is_some());
}

// =========================================================================
// 2. Verify RBAC tier classification and role enforcement
// =========================================================================
#[tokio::test]
async fn test_window_tools_rbac_permission_enforcement() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = McpRouter::new(registry.clone());

    let window_tools = ["list_windows", "focus_window", "close_window"];

    // Viewer Role: FORBIDDEN for all window tools
    for tool_name in &window_tools {
        assert!(
            !is_tool_allowed_for_role(Role::Viewer, tool_name),
            "Viewer should not have permission for '{}'",
            tool_name
        );

        let res = router
            .dispatch_tool_call_with_role(
                tool_name,
                json!({}),
                None,
                Some(Role::Viewer),
                Some("127.0.0.1"),
                Some("view***"),
            )
            .await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("Forbidden") && err.contains("viewer"),
            "Expected forbidden message, got: {}",
            err
        );
    }

    // 3. Operator Role: FORBIDDEN for all window tools
    for tool_name in &window_tools {
        assert!(
            !is_tool_allowed_for_role(Role::Operator, tool_name),
            "Operator should not have permission for '{}'",
            tool_name
        );

        let res = router
            .dispatch_tool_call_with_role(
                tool_name,
                json!({}),
                None,
                Some(Role::Operator),
                Some("127.0.0.1"),
                Some("oper***"),
            )
            .await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("Forbidden") && err.contains("operator"),
            "Expected forbidden message, got: {}",
            err
        );
    }

    // 4. Admin Role: ALLOWED for all window tools
    for tool_name in &window_tools {
        assert!(
            is_tool_allowed_for_role(Role::Admin, tool_name),
            "Admin must have permission for '{}'",
            tool_name
        );
    }
}

// =========================================================================
// 3. Verify router forwards window tool calls and correlates responses
// =========================================================================
#[tokio::test]
async fn test_router_forwards_and_correlates_window_tool_calls() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let term = create_test_terminal("term-win-route", "WIN-HOST");
    registry.register(term, tx).await;

    // 1. Dispatch list_windows
    let router_clone = router.clone();
    let invoke_handle = tokio::spawn(async move {
        router_clone
            .dispatch_tool_call(
                "list_windows",
                json!({ "terminal_id": "term-win-route", "only_visible": true }),
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
            assert_eq!(tool_name, "list_windows");
            assert_eq!(arguments["only_visible"], true);

            // Agent responds with tool result
            router
                .handle_tool_result(AgentToServerMessage::ToolResult {
                    call_id,
                    success: true,
                    result: json!([
                        {
                            "hwnd": 12345,
                            "pid": 555,
                            "title": "Document - Text Editor",
                            "process_name": "textedit",
                            "is_minimized": false,
                            "is_foreground": true,
                            "rect": [0, 0, 800, 600]
                        }
                    ]),
                    error: None,
                    duration_ms: 15,
                })
                .await;
        }
        _ => panic!("Expected InvokeTool variant"),
    }

    let result = invoke_handle
        .await
        .unwrap()
        .expect("Dispatch should succeed");
    let wins: Vec<WindowInfo> = serde_json::from_value(result).unwrap();
    assert_eq!(wins.len(), 1);
    assert_eq!(wins[0].hwnd, 12345);
    assert_eq!(wins[0].title, "Document - Text Editor");

    // 2. Dispatch focus_window
    let router_clone2 = router.clone();
    let focus_handle = tokio::spawn(async move {
        router_clone2
            .dispatch_tool_call(
                "focus_window",
                json!({ "terminal_id": "term-win-route", "hwnd": 12345 }),
            )
            .await
    });

    let msg2 = rx
        .recv()
        .await
        .expect("Expected InvokeTool message for focus_window");
    match msg2 {
        ServerToAgentMessage::InvokeTool {
            call_id,
            tool_name,
            arguments,
            ..
        } => {
            assert_eq!(tool_name, "focus_window");
            assert_eq!(arguments["hwnd"], 12345);

            router
                .handle_tool_result(AgentToServerMessage::ToolResult {
                    call_id,
                    success: true,
                    result: json!({ "success": true, "action": "focus_window", "hwnd": 12345 }),
                    error: None,
                    duration_ms: 10,
                })
                .await;
        }
        _ => panic!("Expected InvokeTool variant"),
    }

    let focus_res = focus_handle
        .await
        .unwrap()
        .expect("Focus dispatch should succeed");
    assert_eq!(focus_res["success"], true);
    assert_eq!(focus_res["action"], "focus_window");
    assert_eq!(focus_res["hwnd"], 12345);

    // 3. Dispatch close_window
    let router_clone3 = router.clone();
    let close_handle = tokio::spawn(async move {
        router_clone3
            .dispatch_tool_call(
                "close_window",
                json!({ "terminal_id": "term-win-route", "hwnd": 12345 }),
            )
            .await
    });

    let msg3 = rx
        .recv()
        .await
        .expect("Expected InvokeTool message for close_window");
    match msg3 {
        ServerToAgentMessage::InvokeTool {
            call_id,
            tool_name,
            arguments,
            ..
        } => {
            assert_eq!(tool_name, "close_window");
            assert_eq!(arguments["hwnd"], 12345);

            router
                .handle_tool_result(AgentToServerMessage::ToolResult {
                    call_id,
                    success: true,
                    result: json!({ "success": true, "action": "close_window", "hwnd": 12345 }),
                    error: None,
                    duration_ms: 12,
                })
                .await;
        }
        _ => panic!("Expected InvokeTool variant"),
    }

    let close_res = close_handle
        .await
        .unwrap()
        .expect("Close dispatch should succeed");
    assert_eq!(close_res["success"], true);
    assert_eq!(close_res["action"], "close_window");
}

// =========================================================================
// 4. End-to-end WebSocket window tool invocation pipeline
// =========================================================================
#[tokio::test]
async fn test_e2e_ws_window_tool_invocation_pipeline() {
    reset_window_mocks();

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

    let server_task = tokio::spawn(async move {
        handle_stream(server_stream, server_state).await;
    });

    // 2. Set mock windows in agent
    let mock_list = vec![WindowInfo {
        hwnd: 8888,
        pid: 999,
        title: "Terminal E2E Window".to_string(),
        process_name: "testapp".to_string(),
        is_minimized: false,
        is_foreground: true,
        rect: [100, 100, 600, 400],
        display_index: None,
    }];
    set_mock_windows(Some(mock_list));

    // 3. Start real agent connected to server
    let agent_term = create_test_terminal("e2e-win-agent", "AGENT-WIN-HOST");
    let agent_executor = Arc::new(AgentExecutor::new().with_computer_use(true));

    let ws_client = Arc::new(AgentWsClient::new(
        "ws://127.0.0.1:9801/ws".to_string(),
        agent_term,
        agent_executor,
    ));

    let client_clone = ws_client.clone();
    let client_task = tokio::spawn(async move {
        let _ = client_clone
            .handshake_and_run_stream(client_stream, "127.0.0.1", "/ws")
            .await;
    });

    // Wait for agent to register
    tokio::time::sleep(Duration::from_millis(150)).await;

    // 4. Execute list_windows through server McpRouter
    let list_res = router
        .dispatch_tool_call(
            "list_windows",
            json!({ "terminal_id": "e2e-win-agent", "only_visible": true }),
        )
        .await;

    assert!(list_res.is_ok(), "list_windows e2e failed: {:?}", list_res);
    let wins: Vec<WindowInfo> = serde_json::from_value(list_res.unwrap()).unwrap();
    assert_eq!(wins.len(), 1);
    assert_eq!(wins[0].hwnd, 8888);
    assert_eq!(wins[0].title, "Terminal E2E Window");

    // 5. Execute focus_window through server McpRouter
    let focus_res = router
        .dispatch_tool_call(
            "focus_window",
            json!({ "terminal_id": "e2e-win-agent", "hwnd": 8888 }),
        )
        .await;
    assert!(
        focus_res.is_ok(),
        "focus_window e2e failed: {:?}",
        focus_res
    );
    let fval = focus_res.unwrap();
    assert_eq!(fval["success"], true);
    assert_eq!(fval["hwnd"], 8888);

    ws_client.disconnect("Testing finished").await;
    client_task.abort();
    server_task.abort();
    reset_window_mocks();
}
