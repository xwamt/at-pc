pub mod audit;
pub mod config;
pub mod mcp;
pub mod meta_store;
pub mod router;
pub mod shutdown;
pub(crate) mod tail;
pub mod tls;
pub mod ws;

pub use audit::{AuditLogger, AuditRecord};
pub use config::{is_tool_allowed_for_role, Role, ServerConfig};
pub use mcp::tools::get_mcp_tool_definitions;
pub use mcp::{create_mcp_http_router, handle_jsonrpc_request, run_stdio_server};
pub use meta_store::{TerminalMeta, TerminalMetaStore};
pub use router::McpRouter;
pub use shutdown::{
    abort_and_join_tasks, complete_or_hold_on_error, serve_with_graceful_shutdown,
    shutdown_persistence, shutdown_signal, start_mcp_http_server_with_shutdown,
    GRACEFUL_SHUTDOWN_TIMEOUT,
};
pub use ws::handler::{AgentMessageHandler, NoopMessageHandler, WsServerState};
pub use ws::registry::{TerminalEntry, TerminalRegistry, TerminalSession, TerminalStatus};
