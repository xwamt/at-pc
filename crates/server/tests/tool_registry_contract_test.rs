use at_pc_protocol::tools::{tool_registry, Role};
use at_pc_server::config::is_tool_allowed_for_role;
use at_pc_server::mcp::tools::get_mcp_tool_definitions;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::registry::TerminalRegistry;
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;

#[test]
fn server_agent_subset_matches_registry_and_schema_projection() {
    let tools = get_mcp_tool_definitions();
    let names = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(names.len(), tools.len(), "Server tool names must be unique");

    let registry_names = tool_registry()
        .iter()
        .map(|spec| spec.name)
        .collect::<HashSet<_>>();
    let forwarded_names = names
        .iter()
        .copied()
        .filter(|name| McpRouter::is_forwarded_tool(name))
        .collect::<HashSet<_>>();
    assert_eq!(forwarded_names, registry_names);
    assert_eq!(tools.len(), tool_registry().len() + 6);

    for spec in tool_registry() {
        let tool = tools.iter().find(|tool| tool["name"] == spec.name).unwrap();
        assert!(tool["inputSchema"]["properties"]["terminal_id"].is_object());
        let has_server_save = tool["inputSchema"]["properties"]
            .get("server_save_path")
            .is_some();
        assert_eq!(
            has_server_save,
            matches!(spec.name, "capture_screen" | "get_marked_screen")
        );
    }
}

#[test]
fn registry_drives_rbac_and_router_forwarding_for_every_agent_tool() {
    for spec in tool_registry() {
        assert!(McpRouter::is_forwarded_tool(spec.name));
        for role in [Role::Viewer, Role::Operator, Role::Admin] {
            assert_eq!(
                is_tool_allowed_for_role(role, spec.name),
                role >= spec.required_role,
                "RBAC mismatch for {} and {role}",
                spec.name
            );
        }
    }
}

#[test]
fn unknown_tools_fail_closed_for_every_role() {
    assert!(!McpRouter::is_forwarded_tool("not_a_tool"));
    for role in [Role::Viewer, Role::Operator, Role::Admin] {
        assert!(!is_tool_allowed_for_role(role, "not_a_tool"));
    }
}

#[tokio::test]
async fn cancel_task_is_an_unlisted_non_forwarded_operator_alias() {
    let names = get_mcp_tool_definitions()
        .into_iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect::<HashSet<_>>();
    assert!(names.contains("cancel_tool"));
    assert!(!names.contains("cancel_task"));
    assert!(!McpRouter::is_forwarded_tool("cancel_tool"));
    assert!(!McpRouter::is_forwarded_tool("cancel_task"));
    assert!(!is_tool_allowed_for_role(Role::Viewer, "cancel_task"));
    assert!(is_tool_allowed_for_role(Role::Operator, "cancel_task"));
    assert!(is_tool_allowed_for_role(Role::Admin, "cancel_task"));

    let router = McpRouter::new(Arc::new(TerminalRegistry::new()));
    for name in ["cancel_tool", "cancel_task"] {
        let error = router
            .dispatch_tool_call_with_role(
                name,
                json!({"call_id":"missing-call"}),
                None,
                Some(Role::Operator),
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(error.contains("Pending tool call"), "{name}: {error}");
        assert!(!error.contains("Unknown or unsupported"), "{name}: {error}");
    }
}

#[tokio::test]
async fn router_rejects_unknown_tools_with_and_without_rbac() {
    let router = McpRouter::new(Arc::new(TerminalRegistry::new()));
    let denied = router
        .dispatch_tool_call_with_role("not_a_tool", json!({}), None, Some(Role::Admin), None, None)
        .await
        .unwrap_err();
    assert!(denied.contains("Forbidden"));

    let unknown = router
        .dispatch_tool_call("not_a_tool", json!({}))
        .await
        .unwrap_err();
    assert!(unknown.contains("Unknown or unsupported"));
}
