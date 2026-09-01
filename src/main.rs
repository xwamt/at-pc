use at_pc::app::run_app;
use at_pc::server::{start_server, AppState, AuditLogEntry};
use at_pc::utils::{network, security};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing subscriber for logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting at-pc remote troubleshooter...");

    // Auto-detect local LAN IP
    let lan_ip = network::get_lan_ip();

    // Allocate available port starting from 9800
    let port = network::find_available_port(9800);

    // Generate random 4-digit PIN
    let pin = security::generate_pin();

    tracing::info!(
        "Server initialized on http://{}:{} with PIN [{}]",
        lan_ip,
        port,
        pin
    );

    // Create shared backend AppState
    let state = Arc::new(AppState::new(pin.clone(), port));

    // Record server starting event in audit log stream
    let startup_log = AuditLogEntry::new(
        "server_startup",
        serde_json::json!({ "lan_ip": lan_ip, "port": port }),
        "STARTED",
        None,
        None,
        Some(format!("服务已在 0.0.0.0:{} 启动", port)),
    );
    state.broadcast_audit(startup_log);

    // Spawn Axum MCP HTTP/SSE server in background tokio task
    let _server_handle = start_server(state.clone(), port).await;

    // Launch egui desktop application window on main thread
    if let Err(e) = run_app(lan_ip, state.clone()) {
        tracing::error!("GUI application error: {e}");
    }

    // Trigger graceful shutdown when GUI window is closed
    state.trigger_shutdown();
    tracing::info!("at-pc application exited cleanly.");

    Ok(())
}
