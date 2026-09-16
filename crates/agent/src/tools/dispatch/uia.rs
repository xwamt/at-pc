use at_pc_protocol::tools::AgentToolKind;
use serde_json::Value;

use super::{computer_use, flexible_u32, handled, DispatchContext};
use crate::tools::{batch, uia};

pub fn handles(kind: AgentToolKind) -> bool {
    use AgentToolKind as K;
    matches!(
        kind,
        K::GetUiTree | K::ClickElement | K::SetElementText | K::BatchActions
    )
}

pub fn dispatch(kind: AgentToolKind, ctx: &DispatchContext<'_>) -> Option<Result<Value, String>> {
    use AgentToolKind as K;
    if !handles(kind) {
        return None;
    }
    match kind {
        K::GetUiTree => handled(|| {
            let depth = ctx.arguments.get("depth").and_then(|value| match value {
                Value::Number(number) => number.as_u64().map(|value| value as u32),
                Value::String(text) => text.trim().parse::<u32>().ok(),
                _ => None,
            });
            let window_title = ctx.arguments.get("window_title").and_then(Value::as_str);
            let query = ctx
                .arguments
                .get("query")
                .or_else(|| ctx.arguments.get("filter"))
                .and_then(Value::as_str);
            let compact = ctx.arguments.get("compact").and_then(Value::as_bool);
            serde_json::to_value(uia::get_ui_tree_filtered(
                depth,
                window_title,
                query,
                compact,
            )?)
            .map_err(|e| e.to_string())
        }),
        K::ClickElement => Some(computer_use(ctx.enable_computer_use, || {
            let element_id = flexible_u32(ctx.arguments, "element_id", "id")
                .ok_or_else(|| "Missing required parameter 'element_id'".to_string())?;
            let action_type = ctx.arguments.get("action_type").and_then(Value::as_str);
            let with_diff = ctx.arguments.get("with_diff").and_then(Value::as_bool);
            uia::click_element_with_diff(element_id, action_type, with_diff)
        })),
        K::SetElementText => Some(computer_use(ctx.enable_computer_use, || {
            let element_id = flexible_u32(ctx.arguments, "element_id", "id")
                .ok_or_else(|| "Missing required parameter 'element_id'".to_string())?;
            let text = match ctx
                .arguments
                .get("text")
                .or_else(|| ctx.arguments.get("value"))
            {
                Some(Value::String(value)) => value.clone(),
                Some(Value::Number(value)) => value.to_string(),
                Some(Value::Bool(value)) => value.to_string(),
                _ => return Err("Missing required parameter 'text'".to_string()),
            };
            let with_diff = ctx.arguments.get("with_diff").and_then(Value::as_bool);
            uia::set_element_text_with_diff(element_id, &text, with_diff)
        })),
        K::BatchActions => Some(computer_use(ctx.enable_computer_use, || {
            batch::execute_batch_actions(ctx.arguments)
        })),
        _ => unreachable!("uia claimed {kind:?} in handles() but has no dispatch arm"),
    }
}
