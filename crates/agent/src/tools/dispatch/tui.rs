use at_pc_protocol::tools::AgentToolKind;
use serde_json::Value;

use super::{computer_use, DispatchContext};
use crate::tools::window;

pub fn handles(kind: AgentToolKind) -> bool {
    use AgentToolKind as K;
    matches!(kind, K::ListWindows | K::FocusWindow | K::CloseWindow)
}

pub fn dispatch(kind: AgentToolKind, ctx: &DispatchContext<'_>) -> Option<Result<Value, String>> {
    use AgentToolKind as K;
    if !handles(kind) {
        return None;
    }
    match kind {
        K::ListWindows => {
            let only_visible = ctx
                .arguments
                .get("only_visible")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            Some(
                window::list_windows(only_visible)
                    .and_then(|wins| serde_json::to_value(wins).map_err(|e| e.to_string())),
            )
        }
        K::FocusWindow => Some(computer_use(ctx.enable_computer_use, || {
            window::focus_window(
                ctx.arguments.get("title").and_then(Value::as_str),
                ctx.arguments
                    .get("pid")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32),
                ctx.arguments
                    .get("hwnd")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize),
            )
        })),
        K::CloseWindow => Some(computer_use(ctx.enable_computer_use, || {
            window::close_window(
                ctx.arguments.get("title").and_then(Value::as_str),
                ctx.arguments
                    .get("pid")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32),
                ctx.arguments
                    .get("hwnd")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize),
            )
        })),
        _ => unreachable!("tui claimed {kind:?} in handles() but has no dispatch arm"),
    }
}
