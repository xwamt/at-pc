use at_pc_protocol::tools::AgentToolKind;
use serde_json::Value;

use super::{computer_use, DispatchContext};
use crate::tools::computer_use as computer_use_ops;

pub fn handles(kind: AgentToolKind) -> bool {
    use AgentToolKind as K;
    matches!(
        kind,
        K::MouseClick
            | K::MouseMove
            | K::MouseDrag
            | K::MouseScroll
            | K::TypeText
            | K::PressKey
            | K::KeyDown
            | K::KeyUp
            | K::Hotkey
    )
}

pub fn dispatch(kind: AgentToolKind, ctx: &DispatchContext<'_>) -> Option<Result<Value, String>> {
    use AgentToolKind as K;
    if !handles(kind) {
        return None;
    }
    let gated = |operation: fn(&Value) -> Result<Value, String>| {
        computer_use(ctx.enable_computer_use, || operation(ctx.arguments))
    };
    match kind {
        K::MouseClick => Some(gated(computer_use_ops::execute_mouse_click)),
        K::MouseMove => Some(gated(computer_use_ops::execute_mouse_move)),
        K::MouseDrag => Some(gated(computer_use_ops::execute_mouse_drag)),
        K::MouseScroll => Some(gated(computer_use_ops::execute_mouse_scroll)),
        K::TypeText => Some(gated(computer_use_ops::execute_type_text)),
        K::PressKey => Some(gated(computer_use_ops::execute_press_key)),
        K::KeyDown => Some(gated(computer_use_ops::execute_key_down)),
        K::KeyUp => Some(gated(computer_use_ops::execute_key_up)),
        K::Hotkey => Some(gated(computer_use_ops::execute_hotkey)),
        _ => unreachable!("input claimed {kind:?} in handles() but has no dispatch arm"),
    }
}
