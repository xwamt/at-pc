//! `at-pc-agent` executable entrypoint.
//! Loads configuration, establishes WebSocket connection with the central server,
//! broadcasts live execution metrics and tool logs to the UI state,
//! and renders the egui desktop interface.

use at_pc_agent::app::{run_agent_app, AgentAppState};
use at_pc_agent::config::AgentConfig;
use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::ws_client::AgentWsClient;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting at-pc-agent v{}...", env!("CARGO_PKG_VERSION"));

    // 2. Load agent configuration
    let config = AgentConfig::load_from_file_or_default(None);
    let terminal_info = config.to_terminal_info();
    let server_url = config.server.url.clone();
    let auth_token = config.server.auth_token.clone();
    let reconnect_secs = config.server.reconnect_interval_secs;

    tracing::info!(
        "Terminal [{}] initialized. Target server: {}, reconnect interval: {}s",
        terminal_info.terminal_id,
        server_url,
        reconnect_secs
    );

    // 3. Create UI State
    let state = Arc::new(
        AgentAppState::new(server_url.clone())
            .with_terminal_info(terminal_info.clone())
    );

    state.add_audit_log(
        "agent_startup",
        &format!(
            "Terminal [{}] targeting server: {}",
            terminal_info.terminal_id, server_url
        ),
        "STARTED",
    );

    // 4. Create Executor & WS Client
    let executor = Arc::new(AgentExecutor::default());
    let ws_client = Arc::new(
        AgentWsClient::new(server_url, terminal_info, executor)
            .with_auth_token(auth_token)
            .with_reconnect_interval(reconnect_secs)
            .with_listener(state.clone())
    );

    state.register_ws_client(ws_client.clone());

    // 5. Spawn background WS Client task
    let client = ws_client.clone();
    let client_task = tokio::spawn(async move {
        client.run().await;
    });

    // 6. Launch desktop GUI window on main thread
    if let Err(e) = run_agent_app(state.clone()) {
        tracing::error!("Agent GUI error: {}", e);
    }

    // 7. Gracefully disconnect on window close
    tracing::info!("GUI closed. Disconnecting agent...");
    ws_client.disconnect("Agent desktop window closed").await;
    let _ = client_task.await;
    tracing::info!("at-pc-agent exited cleanly.");

    Ok(())
}
