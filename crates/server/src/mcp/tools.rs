use at_pc_protocol::tools::{extend_input_schema, string_property, tool_registry, AgentToolKind};
use serde_json::{json, Value};

/// Returns MCP schemas exposed by the central server.
/// Server-only meta-tools stay explicit; all forwarded Agent tools are projected from protocol.
pub fn get_mcp_tool_definitions() -> Vec<Value> {
    let mut tools = server_only_tool_definitions();
    tools.extend(tool_registry().iter().map(server_agent_definition));
    tools
}

fn server_only_tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "list_terminals",
            "description": "Lists all connected and known PC terminals with hardware, network, OS, status, and performance metadata.",
            "inputSchema": {"type":"object","properties":{},"required":[]}
        }),
        json!({
            "name": "select_terminal",
            "description": "Sets the active target terminal for subsequent commands in this session.",
            "inputSchema": {"type":"object","properties":{"terminal_id":{"type":"string","description":"Terminal ID, custom alias, or hostname."}},"required":["terminal_id"]}
        }),
        json!({
            "name": "rename_terminal",
            "description": "Sets persistent custom metadata for a terminal.",
            "inputSchema": {"type":"object","properties":{
                "terminal_id":{"type":"string","description":"Target terminal ID, existing alias, or hostname."},"custom_name":{"type":"string","description":"Optional user-friendly persistent terminal name."},
                "notes":{"type":"string","description":"Optional notes about the terminal."},"tags":{"type":"array","items":{"type":"string"},"description":"Optional grouping tags."}
            },"required":["terminal_id"]}
        }),
        json!({
            "name": "get_active_terminal",
            "description": "Gets the currently selected active terminal.",
            "inputSchema": {"type":"object","properties":{},"required":[]}
        }),
        json!({
            "name": "cancel_tool",
            "description": "Cancels an in-flight remote tool call by call_id.",
            "inputSchema": {"type":"object","properties":{"call_id":{"type":"string","description":"Unique call ID of the in-flight execution to cancel."}},"required":["call_id"]}
        }),
        json!({
            "name": "list_pending_calls",
            "description": "Lists in-flight remote tool calls, optionally filtered by terminal.",
            "inputSchema": {"type":"object","properties":{"terminal_id":{"type":"string","description":"Optional terminal ID used to filter pending calls."}},"required":[]}
        }),
    ]
}

fn server_agent_definition(spec: &at_pc_protocol::tools::ToolSpec) -> Value {
    let mut additions = vec![(
        "terminal_id",
        string_property("Optional target terminal ID (defaults to the active terminal)."),
    )];
    if matches!(
        spec.agent_kind(),
        AgentToolKind::CaptureScreen | AgentToolKind::GetMarkedScreen
    ) {
        additions.push((
            "server_save_path",
            string_property(
                "Optional path on the central server where the decoded image is saved.",
            ),
        ));
    }
    let input_schema = extend_input_schema(&spec.input_schema, additions)
        .expect("built-in Server tool schema extension must be valid");
    json!({
        "name": spec.name,
        "description": spec.description,
        "inputSchema": input_schema,
    })
}
