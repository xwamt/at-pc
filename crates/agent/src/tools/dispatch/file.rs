use at_pc_protocol::tools::AgentToolKind;
use serde_json::Value;

use super::{handled, required_str, usize_arg, DispatchContext};
use crate::tools::{directory, file_ops};

pub fn handles(kind: AgentToolKind) -> bool {
    use AgentToolKind as K;
    matches!(
        kind,
        K::ReadTextFile | K::WriteTextFile | K::ListDirectory | K::SearchFiles
    )
}

pub fn dispatch(kind: AgentToolKind, ctx: &DispatchContext<'_>) -> Option<Result<Value, String>> {
    use AgentToolKind as K;
    if !handles(kind) {
        return None;
    }
    match kind {
        K::ReadTextFile => handled(|| {
            let file_path = required_str(ctx.arguments, "file_path")?;
            let tail_lines = ctx
                .arguments
                .get("tail_lines")
                .and_then(Value::as_u64)
                .map(|value| value as usize);
            let max_bytes = ctx
                .arguments
                .get("max_bytes")
                .and_then(Value::as_u64)
                .map(|value| value as usize);
            serde_json::to_value(file_ops::read_text_file(file_path, tail_lines, max_bytes)?)
                .map_err(|e| e.to_string())
        }),
        K::WriteTextFile => handled(|| {
            let file_path = required_str(ctx.arguments, "file_path")?;
            let content = required_str(ctx.arguments, "content")?;
            let create_backup = ctx
                .arguments
                .get("create_backup")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            serde_json::to_value(file_ops::write_text_file(
                file_path,
                content,
                create_backup,
            )?)
            .map_err(|e| e.to_string())
        }),
        K::ListDirectory => handled(|| {
            let path = required_str(ctx.arguments, "path")?;
            let recursive = ctx.arguments.get("recursive").and_then(Value::as_bool);
            let max_depth = usize_arg(ctx.arguments, "max_depth");
            let limit = usize_arg(ctx.arguments, "limit");
            serde_json::to_value(directory::list_directory(
                path, recursive, max_depth, limit,
            )?)
            .map_err(|e| e.to_string())
        }),
        K::SearchFiles => handled(|| {
            let base_path = required_str(ctx.arguments, "base_path")?;
            let pattern = required_str(ctx.arguments, "pattern")?;
            let max_results = usize_arg(ctx.arguments, "max_results");
            let max_depth = usize_arg(ctx.arguments, "max_depth");
            serde_json::to_value(directory::search_files(
                base_path,
                pattern,
                max_results,
                max_depth,
            )?)
            .map_err(|e| e.to_string())
        }),
        _ => unreachable!("file claimed {kind:?} in handles() but has no dispatch arm"),
    }
}
