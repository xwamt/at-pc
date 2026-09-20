pub mod audit;
pub mod bootstrap;
pub mod config;
pub mod keystore;
pub mod mcp;
pub mod meta_store;
pub mod rate_limit;
pub mod router;
pub mod session;
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
pub use keystore::{
    compute_hash, constant_time_compare, parse_credential, AuthError, KeyKind, KeyRecord, KeyStore,
    KeySummary, Principal,
};
pub use mcp::tools::get_mcp_tool_definitions;
pub use mcp::{create_mcp_http_router, handle_jsonrpc_request, run_stdio_server};
pub use meta_store::{TerminalMeta, TerminalMetaStore};
pub use rate_limit::{AuthRateLimiter, KeyQuotaTracker, QuotaResult, RateLimitDecision};
pub use router::McpRouter;
pub use session::SessionStore;
pub use shutdown::{
    abort_and_join_tasks, complete_or_hold_on_error, serve_with_graceful_shutdown,
    shutdown_persistence, shutdown_signal, start_mcp_http_server_with_shutdown,
    GRACEFUL_SHUTDOWN_TIMEOUT,
};
pub use ws::handler::{AgentMessageHandler, NoopMessageHandler, WsServerState};
pub use ws::registry::{TerminalEntry, TerminalRegistry, TerminalSession, TerminalStatus};
