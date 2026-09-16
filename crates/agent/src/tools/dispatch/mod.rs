//! Domain routers for the agent tool dispatcher.

pub mod file;
pub mod input;
pub mod process;
pub mod screen;
pub mod system;
pub mod tui;
pub mod uia;

use at_pc_protocol::tools::AgentToolKind;
use serde_json::Value;
use std::sync::Arc;

use super::ProcessRegistry;

/// Shared inputs for domain `dispatch` functions.
pub struct DispatchContext<'a> {
    pub call_id: &'a str,
    pub arguments: &'a Value,
    pub registry: &'a Arc<ProcessRegistry>,
    pub enable_computer_use: bool,
}

pub fn dispatch_kind(
    kind: AgentToolKind,
    ctx: &DispatchContext<'_>,
) -> Option<Result<Value, String>> {
    system::dispatch(kind, ctx)
        .or_else(|| process::dispatch(kind, ctx))
        .or_else(|| file::dispatch(kind, ctx))
        .or_else(|| screen::dispatch(kind, ctx))
        .or_else(|| uia::dispatch(kind, ctx))
        .or_else(|| input::dispatch(kind, ctx))
        .or_else(|| tui::dispatch(kind, ctx))
}

pub type DomainHandleFn = fn(AgentToolKind) -> bool;

pub fn domain_handle_fns() -> [(&'static str, DomainHandleFn); 7] {
    [
        ("system", system::handles),
        ("process", process::handles),
        ("file", file::handles),
        ("screen", screen::handles),
        ("uia", uia::handles),
        ("input", input::handles),
        ("tui", tui::handles),
    ]
}

fn required_str<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Missing required parameter '{name}'"))
}

fn usize_arg(arguments: &Value, name: &str) -> Option<usize> {
    arguments
        .get(name)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
}

fn flexible_u32(arguments: &Value, primary: &str, alias: &str) -> Option<u32> {
    arguments
        .get(primary)
        .or_else(|| arguments.get(alias))
        .and_then(|value| match value {
            Value::Number(number) => number.as_u64().map(|value| value as u32),
            Value::String(text) => text.trim().trim_start_matches('#').parse().ok(),
            _ => None,
        })
}

fn parse_crop(arguments: &Value) -> Option<[u32; 4]> {
    if let Some(values) = arguments.get("crop").and_then(Value::as_array) {
        return (values.len() == 4)
            .then(|| std::array::from_fn(|index| values[index].as_u64().unwrap_or(0) as u32));
    }
    let values = ["crop_x", "crop_y", "crop_width", "crop_height"]
        .map(|name| arguments.get(name).and_then(Value::as_u64));
    match values {
        [Some(x), Some(y), Some(width), Some(height)] => {
            Some([x as u32, y as u32, width as u32, height as u32])
        }
        _ => None,
    }
}

fn computer_use(
    enabled: bool,
    operation: impl FnOnce() -> Result<Value, String>,
) -> Result<Value, String> {
    if !enabled {
        return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
    }
    operation()
}

fn handled(operation: impl FnOnce() -> Result<Value, String>) -> Option<Result<Value, String>> {
    Some(operation())
}
