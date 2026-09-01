//! MCP HTTP/SSE server and protocol handling module.

pub mod auth;
pub mod router;
pub mod sse;
pub mod state;

pub use router::create_mcp_router;
pub use state::{AppState, AuditLogEntry};

use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Starts the Axum MCP HTTP/SSE server on the specified port.
///
/// Listens on `0.0.0.0:<port>` (fallback to `127.0.0.1:<port>` if binding 0.0.0.0 fails).
/// Gracefully terminates when `state.trigger_shutdown()` is invoked.
pub async fn start_server(state: Arc<AppState>, port: u16) -> JoinHandle<()> {
    let app = create_mcp_router(state.clone());
    let mut shutdown_rx = state.subscribe_shutdown();

    let listener = match TcpListener::bind(format!("0.0.0.0:{}", port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("Failed to bind 0.0.0.0:{}, trying 127.0.0.1: {e}", port);
            TcpListener::bind(format!("127.0.0.1:{}", port))
                .await
                .expect("Failed to bind MCP server TCP listener")
        }
    };

    let server_task = tokio::spawn(async move {
        let serve_future = axum::serve(listener, app);

        let graceful_future = serve_future.with_graceful_shutdown(async move {
            while !*shutdown_rx.borrow_and_update() {
                if shutdown_rx.changed().await.is_err() {
                    break;
                }
            }
            tracing::info!("MCP Server received shutdown signal, draining connections...");
        });

        if let Err(e) = graceful_future.await {
            tracing::error!("MCP Server error: {e}");
        }
    });

    server_task
}

/// Triggers graceful shutdown on the running MCP server.
pub fn stop_server(state: &AppState) {
    state.trigger_shutdown();
}
