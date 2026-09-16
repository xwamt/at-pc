use at_pc_agent::tools::{dispatch_tool, get_mcp_tool_definitions, resolve_dispatch_kind};
use at_pc_protocol::tools::tool_registry;
use serde_json::json;
use std::collections::HashSet;

#[test]
fn agent_tools_list_is_exactly_the_shared_registry() {
    let listed = get_mcp_tool_definitions();
    let listed_names = listed
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<HashSet<_>>();
    let registry_names = tool_registry()
        .iter()
        .map(|spec| spec.name)
        .collect::<HashSet<_>>();

    assert_eq!(listed.len(), tool_registry().len());
    assert_eq!(listed_names, registry_names);
    for tool in listed {
        let spec = at_pc_protocol::tools::agent_tool(tool["name"].as_str().unwrap()).unwrap();
        assert_eq!(tool, spec.as_mcp_definition());
    }
}

#[test]
fn every_forwarded_tool_is_recognized_by_typed_agent_dispatch() {
    for spec in tool_registry() {
        assert_eq!(resolve_dispatch_kind(spec.name).unwrap(), spec.agent_kind());
    }
}

#[test]
fn agent_rejects_unknown_server_only_and_cancel_alias_names() {
    for name in ["not_a_tool", "cancel_tool", "cancel_task"] {
        let error = dispatch_tool(name, json!({})).unwrap_err();
        assert!(error.contains("Unknown or unsupported tool"), "{error}");
    }
}
