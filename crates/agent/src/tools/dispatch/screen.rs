use at_pc_protocol::tools::AgentToolKind;
use serde_json::Value;

use super::{computer_use, flexible_u32, handled, parse_crop, usize_arg, DispatchContext};
use crate::tools::{screen, som};

pub fn handles(kind: AgentToolKind) -> bool {
    use AgentToolKind as K;
    matches!(
        kind,
        K::CaptureScreen | K::ListMonitors | K::GetMarkedScreen | K::ClickMark
    )
}

pub fn dispatch(kind: AgentToolKind, ctx: &DispatchContext<'_>) -> Option<Result<Value, String>> {
    use AgentToolKind as K;
    if !handles(kind) {
        return None;
    }
    match kind {
        K::CaptureScreen => handled(|| {
            let display_index = usize_arg(ctx.arguments, "display_index").unwrap_or(0);
            let format = ctx
                .arguments
                .get("format")
                .and_then(Value::as_str)
                .unwrap_or("jpeg");
            let quality = ctx
                .arguments
                .get("quality")
                .and_then(Value::as_u64)
                .map(|value| value as u8)
                .unwrap_or(80);
            let save_path = ctx
                .arguments
                .get("save_path")
                .or_else(|| ctx.arguments.get("save_to_file"))
                .or_else(|| ctx.arguments.get("file_path"))
                .and_then(Value::as_str);
            let max_dimension = ctx
                .arguments
                .get("max_dimension")
                .or_else(|| ctx.arguments.get("max_size"))
                .and_then(Value::as_u64)
                .map(|value| value as u32);
            let crop = parse_crop(ctx.arguments);
            serde_json::to_value(screen::capture_screen(
                display_index,
                format,
                quality,
                save_path,
                max_dimension,
                crop,
            )?)
            .map_err(|e| e.to_string())
        }),
        K::ListMonitors => {
            handled(|| serde_json::to_value(screen::list_monitors()?).map_err(|e| e.to_string()))
        }
        K::GetMarkedScreen => handled(|| {
            let display_index = usize_arg(ctx.arguments, "display_index").unwrap_or(0);
            let format = ctx
                .arguments
                .get("format")
                .and_then(Value::as_str)
                .unwrap_or("jpeg");
            let quality = ctx
                .arguments
                .get("quality")
                .and_then(Value::as_u64)
                .map(|value| value as u8)
                .unwrap_or(80);
            let max_dimension = ctx
                .arguments
                .get("max_dimension")
                .or_else(|| ctx.arguments.get("max_size"))
                .and_then(Value::as_u64)
                .map(|value| value as u32);
            let strategy = ctx.arguments.get("strategy").and_then(Value::as_str);
            let grid_divisions = ctx
                .arguments
                .get("grid_divisions")
                .or_else(|| ctx.arguments.get("divisions"))
                .and_then(Value::as_u64)
                .map(|value| value as u32);
            let window_title = ctx.arguments.get("window_title").and_then(Value::as_str);
            serde_json::to_value(som::get_marked_screen(
                display_index,
                format,
                quality,
                max_dimension,
                parse_crop(ctx.arguments),
                strategy,
                grid_divisions,
                window_title,
            )?)
            .map_err(|e| e.to_string())
        }),
        K::ClickMark => Some(computer_use(ctx.enable_computer_use, || {
            let mark_id = flexible_u32(ctx.arguments, "mark_id", "id")
                .ok_or_else(|| "Missing required parameter 'mark_id'".to_string())?;
            let button = match ctx.arguments.get("button") {
                Some(Value::String(value)) => Some(value.as_str()),
                Some(Value::Number(value)) => match value.as_u64() {
                    Some(1) => Some("middle"),
                    Some(2) => Some("right"),
                    _ => Some("left"),
                },
                _ => None,
            };
            let count = ctx
                .arguments
                .get("count")
                .and_then(Value::as_u64)
                .map(|value| value as u8);
            som::click_mark(mark_id, button, count)
        })),
        _ => unreachable!("screen claimed {kind:?} in handles() but has no dispatch arm"),
    }
}
