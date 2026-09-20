pub mod audit;
pub mod bootstrap;
pub mod config;
pub mod mcp;
pub mod meta_store;
pub mod router;
pub mod shutdown;
pub(crate) mod tail;
pub mod tls;
#[cfg(feature = "tray")]
pub mod tray;
pub mod ws;

pub use audit::{AuditLogger, AuditRecord};
pub use bootstrap::{
    bootstrap_key_path, ensure_bootstrap_key, generate_api_key, print_bootstrap_banner,
    BootstrapOutcome,
};
pub use config::{
    is_loopback, is_tool_allowed_for_role, validate_boot_gate, BootGateError, Role, ServerConfig,
};
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
