//! Integration tests for Milestone 1 (P0 / v0.7.0):
//! Multi-monitor topology discovery (`list_monitors`), MCP schema,
//! RBAC authorization, and end-to-end router forwarding.

use serde_json::json;
use std::sync::Arc;

use at_pc_agent::tools::screen::{reset_mock_monitors, set_mock_monitors};
use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::{MonitorInfo, TerminalInfo};
use at_pc_server::config::{is_tool_allowed_for_role, Role};
use at_pc_server::mcp::tools::get_mcp_tool_definitions;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::registry::TerminalRegistry;

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
// 1. Verify MCP tool schema for list_monitors
// =========================================================================
#[test]
fn test_mcp_tool_definitions_include_list_monitors() {
    let tools = get_mcp_tool_definitions();

    let tool = tools
        .iter()
        .find(|t| t["name"] == "list_monitors")
        .expect("list_monitors tool should be present in get_mcp_tool_definitions()");

    let desc = tool["description"]
        .as_str()
        .expect("description should be a string");
    assert!(desc.contains("connected physical and virtual display monitors"));

    let schema = &tool["inputSchema"];
    assert_eq!(schema["type"], "object");
    assert!(schema["properties"].get("terminal_id").is_some());
}

// =========================================================================
// 2. Verify RBAC tier classification and role enforcement
// =========================================================================
#[test]
fn test_list_monitors_rbac_permission_enforcement() {
    assert!(
        is_tool_allowed_for_role(Role::Viewer, "list_monitors"),
        "Viewer role must have access to list_monitors"
    );
    assert!(
        is_tool_allowed_for_role(Role::Operator, "list_monitors"),
        "Operator role must have access to list_monitors"
    );
    assert!(
        is_tool_allowed_for_role(Role::Admin, "list_monitors"),
        "Admin role must have access to list_monitors"
    );
}

// =========================================================================
// 3. Verify router forwards and correlates list_monitors tool calls
// =========================================================================
#[tokio::test]
async fn test_router_forwards_and_correlates_list_monitors_tool_calls() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let term = create_test_terminal("term-monitor-route", "TEST-MULTI-MON-HOST");
    registry.register(term, tx).await;

    // Spawn router dispatch for list_monitors
    let router_clone = router.clone();
    let invoke_handle = tokio::spawn(async move {
        router_clone
            .dispatch_tool_call(
                "list_monitors",
                json!({ "terminal_id": "term-monitor-route" }),
            )
            .await
    });

    let msg = rx.recv().await.expect("Expected InvokeTool message");
    match msg {
        ServerToAgentMessage::InvokeTool {
            call_id, tool_name, ..
        } => {
            assert_eq!(tool_name, "list_monitors");

            let mock_monitors = vec![
                MonitorInfo {
                    display_index: 0,
                    name: "Built-in Display".to_string(),
                    is_primary: true,
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                    scale_factor: 2.0,
                },
                MonitorInfo {
                    display_index: 1,
                    name: "DELL U2720Q".to_string(),
                    is_primary: false,
                    x: 1920,
                    y: 0,
                    width: 3840,
                    height: 2160,
                    scale_factor: 1.5,
                },
            ];

            router
                .handle_tool_result(AgentToServerMessage::ToolResult {
                    call_id,
                    success: true,
                    result: serde_json::to_value(&mock_monitors).unwrap(),
                    error: None,
                    duration_ms: 5,
                })
                .await;
        }
        _ => panic!("Expected InvokeTool variant"),
    }

    let result_val = invoke_handle
        .await
        .unwrap()
        .expect("Dispatch should succeed");
    let monitors: Vec<MonitorInfo> =
        serde_json::from_value(result_val).expect("Should deserialize to Vec<MonitorInfo>");

    assert_eq!(monitors.len(), 2);
    assert_eq!(monitors[0].display_index, 0);
    assert_eq!(monitors[0].name, "Built-in Display");
    assert!(monitors[0].is_primary);
    assert_eq!(monitors[0].x, 0);
    assert_eq!(monitors[0].y, 0);
    assert_eq!(monitors[0].width, 1920);
    assert_eq!(monitors[0].height, 1080);
    assert_eq!(monitors[0].scale_factor, 2.0);

    assert_eq!(monitors[1].display_index, 1);
    assert_eq!(monitors[1].name, "DELL U2720Q");
    assert!(!monitors[1].is_primary);
    assert_eq!(monitors[1].x, 1920);
    assert_eq!(monitors[1].y, 0);
    assert_eq!(monitors[1].width, 3840);
    assert_eq!(monitors[1].height, 2160);
    assert_eq!(monitors[1].scale_factor, 1.5);
}

// =========================================================================
// 4. Verify agent tools dispatch for list_monitors with mock
// =========================================================================
#[test]
fn test_agent_tools_dispatch_list_monitors() {
    let mock_monitors = vec![
        MonitorInfo {
            display_index: 0,
            name: "Display 0".to_string(),
            is_primary: true,
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
        },
        MonitorInfo {
            display_index: 1,
            name: "Display 1".to_string(),
            is_primary: false,
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
        },
    ];

    set_mock_monitors(Some(mock_monitors.clone()));

    let res = at_pc_agent::tools::dispatch_tool("list_monitors", json!({}))
        .expect("dispatch_tool list_monitors should succeed");

    let parsed: Vec<MonitorInfo> =
        serde_json::from_value(res).expect("Result should parse to Vec<MonitorInfo>");
    assert_eq!(parsed, mock_monitors);

    reset_mock_monitors();
}
