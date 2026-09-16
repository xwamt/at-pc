use at_pc_protocol::tools::AgentToolKind;
use serde_json::Value;

use super::{handled, required_str, usize_arg, DispatchContext};
use crate::tools::{event_log, network, service, sysinfo};

pub fn handles(kind: AgentToolKind) -> bool {
    use AgentToolKind as K;
    matches!(
        kind,
        K::GetSystemOverview
            | K::ManageService
            | K::GetEventLogs
            | K::ListNetworkConnections
            | K::TestNetwork
    )
}

pub fn dispatch(kind: AgentToolKind, ctx: &DispatchContext<'_>) -> Option<Result<Value, String>> {
    use AgentToolKind as K;
    if !handles(kind) {
        return None;
    }
    match kind {
        K::GetSystemOverview => handled(|| {
            serde_json::to_value(sysinfo::get_system_overview()).map_err(|e| e.to_string())
        }),
        K::ManageService => handled(|| {
            let service_name = required_str(ctx.arguments, "service_name")?;
            let action = required_str(ctx.arguments, "action")?;
            serde_json::to_value(service::manage_service(service_name, action)?)
                .map_err(|e| e.to_string())
        }),
        K::GetEventLogs => handled(|| {
            let log_name = ctx.arguments.get("log_name").and_then(Value::as_str);
            let level = ctx.arguments.get("level").and_then(Value::as_str);
            let hours_back = ctx.arguments.get("hours_back").and_then(Value::as_u64);
            let limit = usize_arg(ctx.arguments, "limit");
            serde_json::to_value(event_log::get_event_logs(
                log_name, level, hours_back, limit,
            )?)
            .map_err(|e| e.to_string())
        }),
        K::ListNetworkConnections => handled(|| {
            let state = ctx.arguments.get("state").and_then(Value::as_str);
            let port = ctx
                .arguments
                .get("port")
                .and_then(Value::as_u64)
                .map(|value| value as u16);
            let limit = usize_arg(ctx.arguments, "limit");
            serde_json::to_value(network::list_network_connections(state, port, limit)?)
                .map_err(|e| e.to_string())
        }),
        K::TestNetwork => handled(|| {
            let target_host = required_str(ctx.arguments, "target_host")?;
            let port = ctx
                .arguments
                .get("port")
                .and_then(Value::as_u64)
                .map(|value| value as u16);
            let timeout_ms = ctx.arguments.get("timeout_ms").and_then(Value::as_u64);
            serde_json::to_value(network::test_network(target_host, port, timeout_ms)?)
                .map_err(|e| e.to_string())
        }),
        _ => unreachable!("system claimed {kind:?} in handles() but has no dispatch arm"),
    }
}
