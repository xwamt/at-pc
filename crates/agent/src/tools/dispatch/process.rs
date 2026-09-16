use at_pc_protocol::tools::AgentToolKind;
use serde_json::{json, Value};

use super::{handled, required_str, DispatchContext};
use crate::tools::{command, process};

pub fn handles(kind: AgentToolKind) -> bool {
    use AgentToolKind as K;
    matches!(
        kind,
        K::ExecPowershell | K::ExecCmd | K::ListProcesses | K::KillProcess
    )
}

pub fn dispatch(kind: AgentToolKind, ctx: &DispatchContext<'_>) -> Option<Result<Value, String>> {
    use AgentToolKind as K;
    if !handles(kind) {
        return None;
    }
    let call_id_opt = (!ctx.call_id.is_empty()).then_some(ctx.call_id);
    match kind {
        K::ExecPowershell => handled(|| {
            let script = required_str(ctx.arguments, "script")?;
            let timeout_secs = ctx
                .arguments
                .get("timeout_secs")
                .and_then(Value::as_u64)
                .unwrap_or(30);
            let cwd = ctx.arguments.get("cwd").and_then(Value::as_str);
            let result = command::exec_powershell_with_call(
                script,
                timeout_secs,
                cwd,
                ctx.registry,
                call_id_opt,
            )?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }),
        K::ExecCmd => handled(|| {
            let command_text = required_str(ctx.arguments, "command")?;
            let timeout_secs = ctx
                .arguments
                .get("timeout_secs")
                .and_then(Value::as_u64)
                .unwrap_or(30);
            let cwd = ctx.arguments.get("cwd").and_then(Value::as_str);
            let result = command::exec_cmd_with_call(
                command_text,
                timeout_secs,
                cwd,
                ctx.registry,
                call_id_opt,
            )?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }),
        K::ListProcesses => handled(|| {
            let filter = ctx
                .arguments
                .get("filter")
                .or_else(|| ctx.arguments.get("filter_name"))
                .and_then(Value::as_str);
            let sort_by = ctx.arguments.get("sort_by").and_then(Value::as_str);
            let limit = ctx
                .arguments
                .get("limit")
                .and_then(Value::as_u64)
                .map(|value| value as usize)
                .unwrap_or(50);
            serde_json::to_value(process::list_processes(filter, sort_by, limit))
                .map_err(|e| e.to_string())
        }),
        K::KillProcess => handled(|| {
            let pid = ctx
                .arguments
                .get("pid")
                .and_then(Value::as_u64)
                .map(|value| value as u32);
            let name = ctx
                .arguments
                .get("name")
                .or_else(|| ctx.arguments.get("process_name"))
                .and_then(Value::as_str);
            let force = ctx
                .arguments
                .get("force")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let message = process::kill_process(pid, name, force)?;
            Ok(json!({"message": message, "success": true}))
        }),
        _ => unreachable!("process claimed {kind:?} in handles() but has no dispatch arm"),
    }
}
