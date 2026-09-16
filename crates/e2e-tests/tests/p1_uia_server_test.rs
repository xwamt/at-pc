use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::ws_client::{AgentWsClient, ClientConnectionStatus};
use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::{TerminalInfo, UiTreeResponse};
use at_pc_server::config::{Role, ServerConfig};
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
        agent_version: "0.5.0".to_string(),
    }
}

// =========================================================================
// 1. Verify MCP tool schemas in server definition
// =========================================================================
#[test]
fn test_mcp_tool_definitions_include_uia_tools() {
    let tools = get_mcp_tool_definitions();

    let uia_tool_names = ["get_ui_tree", "click_element", "set_element_text"];
    for name in &uia_tool_names {
        let tool = tools
            .iter()
            .find(|t| t["name"] == *name)
            .unwrap_or_else(|| panic!("Tool '{}' not found in MCP tool definitions", name));

        assert!(!tool["description"].as_str().unwrap().is_empty());
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].get("terminal_id").is_some());
    }

    // Specific schema checks
    let click_tool = tools.iter().find(|t| t["name"] == "click_element").unwrap();
    let click_req = click_tool["inputSchema"]["required"].as_array().unwrap();
    assert!(click_req.iter().any(|r| r == "element_id"));

    let text_tool = tools
        .iter()
        .find(|t| t["name"] == "set_element_text")
        .unwrap();
    let text_req = text_tool["inputSchema"]["required"].as_array().unwrap();
    assert!(text_req.iter().any(|r| r == "element_id"));
    assert!(text_req.iter().any(|r| r == "text"));
}

// =========================================================================
// 2. Verify router forwards UIA tool calls and correlates responses
// =========================================================================
#[tokio::test]
async fn test_router_forwards_and_correlates_uia_tool_calls() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let term = create_test_terminal("term-uia-route", "UIA-HOST");
    registry.register(term, tx).await;

    // 1. Dispatch get_ui_tree
    let router_clone = router.clone();
    let invoke_handle = tokio::spawn(async move {
        router_clone
            .dispatch_tool_call(
                "get_ui_tree",
                json!({ "terminal_id": "term-uia-route", "depth": 4, "window_title": "Notepad" }),
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
            assert_eq!(tool_name, "get_ui_tree");
            assert_eq!(arguments["depth"], 4);
            assert_eq!(arguments["window_title"], "Notepad");

            // Agent responds with mock UiTreeResponse
            router
                .handle_tool_result(AgentToServerMessage::ToolResult {
                    call_id,
                    success: true,
                    result: json!({
                        "active_window": "Notepad - Untitled",
                        "window_bounds": [100, 100, 800, 600],
                        "elements": [
                            {
                                "id": 1,
                                "type": "Edit",
                                "name": "Text Editor",
                                "rect": [100, 150, 800, 550],
                                "enabled": true
                            }
                        ],
                        "total_elements": 1
                    }),
                    error: None,
                    duration_ms: 25,
                })
                .await;
        }
        _ => panic!("Expected InvokeTool variant"),
    }

    let result_val = invoke_handle
        .await
        .unwrap()
        .expect("Invocation should succeed");
    let tree: UiTreeResponse =
        serde_json::from_value(result_val).expect("Must deserialize as UiTreeResponse");
    assert_eq!(tree.active_window, "Notepad - Untitled");
    assert_eq!(tree.total_elements, 1);
    assert_eq!(tree.elements[0].control_type, "Edit");
}

// =========================================================================
// 3. Verify RBAC permissions for UIA tools
// =========================================================================
#[tokio::test]
async fn test_uia_rbac_permission_enforcement() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));

    // Viewer tests:
    // get_ui_tree is inspection tool -> allowed for Viewer
    // (will fail with 'not found in registry' rather than 'Forbidden' RBAC error)
    let viewer_tree = router
        .dispatch_tool_call_with_role(
            "get_ui_tree",
            json!({ "terminal_id": "nonexistent" }),
            None,
            Some(Role::Viewer),
            None,
            None,
        )
        .await;
    assert!(viewer_tree.is_err());
    assert!(
        !viewer_tree.unwrap_err().contains("Forbidden"),
        "Viewer should be authorized to execute get_ui_tree"
    );

    // click_element and set_element_text are mutating tools -> forbidden for Viewer
    let viewer_click = router
        .dispatch_tool_call_with_role(
            "click_element",
            json!({ "element_id": 1 }),
            None,
            Some(Role::Viewer),
            None,
            None,
        )
        .await;
    assert!(viewer_click.is_err());
    assert!(viewer_click
        .unwrap_err()
        .contains("Forbidden: Role 'viewer' is not authorized to execute tool 'click_element'"));

    let viewer_text = router
        .dispatch_tool_call_with_role(
            "set_element_text",
            json!({ "element_id": 1, "text": "abc" }),
            None,
            Some(Role::Viewer),
            None,
            None,
        )
        .await;
    assert!(viewer_text.is_err());
    assert!(viewer_text
        .unwrap_err()
        .contains("Forbidden: Role 'viewer' is not authorized to execute tool 'set_element_text'"));

    // Operator tests:
    // get_ui_tree -> allowed
    let operator_tree = router
        .dispatch_tool_call_with_role(
            "get_ui_tree",
            json!({ "terminal_id": "nonexistent" }),
            None,
            Some(Role::Operator),
            None,
            None,
        )
        .await;
    assert!(operator_tree.is_err());
    assert!(!operator_tree.unwrap_err().contains("Forbidden"));

    // click_element and set_element_text -> forbidden for Operator
    let operator_click = router
        .dispatch_tool_call_with_role(
            "click_element",
            json!({ "element_id": 1 }),
            None,
            Some(Role::Operator),
            None,
            None,
        )
        .await;
    assert!(operator_click.is_err());
    assert!(operator_click
        .unwrap_err()
        .contains("Forbidden: Role 'operator' is not authorized to execute tool 'click_element'"));

    let operator_text = router
        .dispatch_tool_call_with_role(
            "set_element_text",
            json!({ "element_id": 1, "text": "abc" }),
            None,
            Some(Role::Operator),
            None,
            None,
        )
        .await;
    assert!(operator_text.is_err());
    assert!(operator_text.unwrap_err().contains(
        "Forbidden: Role 'operator' is not authorized to execute tool 'set_element_text'"
    ));

    // Admin tests:
    // All allowed through RBAC
    for tool in &["get_ui_tree", "click_element", "set_element_text"] {
        let admin_res = router
            .dispatch_tool_call_with_role(
                tool,
                json!({ "terminal_id": "nonexistent", "element_id": 1, "text": "test" }),
                None,
                Some(Role::Admin),
                None,
                None,
            )
            .await;
        assert!(admin_res.is_err());
        assert!(
            !admin_res.unwrap_err().contains("Forbidden"),
            "Admin should be authorized for {}",
            tool
        );
    }
}

// =========================================================================
// 4. End-to-End WebSocket tool invocation of UIA tools on live agent
// =========================================================================
#[tokio::test]
async fn test_e2e_ws_uia_tool_invocation_pipeline() {
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

    let term_info = create_test_terminal("term-e2e-uia", "WORKSTATION-E2E-UIA");
    let executor = Arc::new(AgentExecutor::new().with_computer_use(true));
    let client = Arc::new(AgentWsClient::new(
        "ws://127.0.0.1:9801/ws".to_string(),
        term_info.clone(),
        executor,
    ));

    let client_clone = client.clone();
    let client_task = tokio::spawn(async move {
        let _ = client_clone
            .handshake_and_run_stream(client_stream, "127.0.0.1", "/ws")
            .await;
    });

    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(client.status().await, ClientConnectionStatus::Connected);

    // 1. Dispatch get_ui_tree via router
    let tree_val = router
        .dispatch_tool_call(
            "get_ui_tree",
            json!({ "terminal_id": "term-e2e-uia", "depth": 3 }),
        )
        .await
        .expect("get_ui_tree invocation should succeed over WS");

    let tree: UiTreeResponse = serde_json::from_value(tree_val).expect("Valid UiTreeResponse");
    assert!(!tree.active_window.is_empty());
    assert!(tree.total_elements > 0);
    let target_elem_id = tree.elements[0].id;

    // 2. Dispatch click_element via router with string element_id (e.g. "#1")
    let click_val = router
        .dispatch_tool_call(
            "click_element",
            json!({ "terminal_id": "term-e2e-uia", "element_id": format!("#{}", target_elem_id), "action_type": "click" }),
        )
        .await
        .expect("click_element invocation should succeed over WS");

    assert_eq!(click_val["success"], true);
    assert_eq!(click_val["element_id"], target_elem_id);

    // 3. Dispatch set_element_text via router with string element_id
    let text_val = router
        .dispatch_tool_call(
            "set_element_text",
            json!({ "terminal_id": "term-e2e-uia", "element_id": target_elem_id.to_string(), "text": "e2e typing test" }),
        )
        .await
        .expect("set_element_text invocation should succeed over WS");

    assert_eq!(text_val["success"], true);
    assert_eq!(text_val["element_id"], target_elem_id);
    assert_eq!(text_val["text"], "e2e typing test");

    client.disconnect("Testing finished").await;
    client_task.abort();
    server_task.abort();
}

// =========================================================================
// 5. Verify MCP prompts/list and prompts/get for desktop automation guidelines
// =========================================================================
#[tokio::test]
async fn test_mcp_desktop_automation_prompts_and_guidelines() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry));

    // 1. Initialize returns prompts capability
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    let init_resp = at_pc_server::mcp::handle_jsonrpc_request(&router, &init_req)
        .await
        .expect("initialize should return response");
    assert_eq!(
        init_resp["result"]["capabilities"]["prompts"]["listChanged"],
        false
    );

    // 2. prompts/list returns desktop_automation_strategy
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "prompts/list",
        "params": {}
    });
    let list_resp = at_pc_server::mcp::handle_jsonrpc_request(&router, &list_req)
        .await
        .expect("prompts/list should return response");
    let prompts = list_resp["result"]["prompts"]
        .as_array()
        .expect("Prompts array");
    assert!(prompts
        .iter()
        .any(|p| p["name"] == "desktop_automation_strategy"));

    // 3. prompts/get returns tier hierarchy guidelines
    let get_req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "prompts/get",
        "params": {
            "name": "desktop_automation_strategy"
        }
    });
    let get_resp = at_pc_server::mcp::handle_jsonrpc_request(&router, &get_req)
        .await
        .expect("prompts/get should return response");
    let content_text = get_resp["result"]["messages"][0]["content"]["text"]
        .as_str()
        .expect("Guidelines text");
    assert!(content_text.contains("Tier 1"));
    assert!(content_text.contains("Tier 2"));
    assert!(content_text.contains("get_ui_tree"));
}
