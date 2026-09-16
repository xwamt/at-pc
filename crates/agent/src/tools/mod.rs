//! MCP tool definitions and typed dispatcher.

pub mod batch;
pub mod command;
pub mod computer_use;
pub mod directory;
pub mod dispatch;
pub mod event_log;
pub mod file_ops;
pub mod network;
pub mod process;
pub mod process_registry;
pub mod screen;
pub mod service;
pub mod som;
pub mod sysinfo;
pub mod uia;
pub mod window;

pub use process_registry::ProcessRegistry;

use at_pc_protocol::tools::{agent_tool, agent_tool_definitions, AgentToolKind};
use serde_json::Value;

/// Returns the protocol-owned definitions for every Agent tool.
pub fn get_mcp_tool_definitions() -> Vec<Value> {
    agent_tool_definitions()
}

/// Resolves a public tool name to the typed key consumed by the dispatcher.
pub fn resolve_dispatch_kind(name: &str) -> Result<AgentToolKind, String> {
    agent_tool(name)
        .map(|spec| spec.agent_kind())
        .ok_or_else(|| format!("Unknown or unsupported tool '{}'", name))
}

pub fn dispatch_tool(name: &str, arguments: Value) -> Result<Value, String> {
    dispatch_tool_with_registry(name, arguments, &ProcessRegistry::global())
}

pub fn dispatch_tool_with_registry(
    name: &str,
    arguments: Value,
    registry: &std::sync::Arc<ProcessRegistry>,
) -> Result<Value, String> {
    dispatch_tool_with_call_id("", name, arguments, registry)
}

pub fn dispatch_tool_with_call_id(
    call_id: &str,
    name: &str,
    arguments: Value,
    registry: &std::sync::Arc<ProcessRegistry>,
) -> Result<Value, String> {
    dispatch_tool_with_call_id_and_options(call_id, name, arguments, registry, false)
}

pub fn dispatch_tool_with_call_id_and_options(
    call_id: &str,
    name: &str,
    arguments: Value,
    registry: &std::sync::Arc<ProcessRegistry>,
    enable_computer_use: bool,
) -> Result<Value, String> {
    let kind = resolve_dispatch_kind(name)?;
    let ctx = dispatch::DispatchContext {
        call_id,
        arguments: &arguments,
        registry,
        enable_computer_use,
    };
    dispatch::dispatch_kind(kind, &ctx)
        .unwrap_or_else(|| Err(format!("Unknown or unsupported tool '{}'", name)))
}
