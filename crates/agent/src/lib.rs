//! `at-pc-agent` library root.
//! Provides AgentConfig, AgentExecutor, AgentWsClient, diagnostic tools, and WebSocket client engine.

pub mod app;
pub mod config;
pub mod executor;
pub mod input;
pub mod stream;
pub mod tls;
pub mod tools;
pub mod ws_client;

use std::sync::Arc;

#[cfg(feature = "gui")]
pub use app::{run_agent_app, setup_custom_fonts, AgentApp};
pub use app::{AgentAppState, AgentAuditLog};
pub use config::{AgentConfig, DeviceConfig, ServerConfig};
pub use executor::AgentExecutor;
pub use input::inject_input_event;
pub use stream::DesktopStreamController;
pub use ws_client::{AgentEventListener, AgentWsClient, ClientConnectionStatus};

/// Runs the agent in headless background service mode without initializing any GUI window.
pub async fn run_headless(
    config: AgentConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let terminal_info = config.to_terminal_info();
    let server_url = config.server.url.clone();
    let auth_token = config.server.auth_token.clone();
    let reconnect_secs = config.server.reconnect_interval_secs;

    tracing::info!(
        "Starting headless agent [{}] targeting server: {}",
        terminal_info.terminal_id,
        server_url
    );

    let executor = Arc::new(AgentExecutor::default().with_computer_use(config.enable_computer_use));
    let ws_client = Arc::new(
        AgentWsClient::new(server_url, terminal_info, executor)
            .with_auth_token(auth_token)
            .with_reconnect_interval(reconnect_secs)
            .with_tls_config(
                config.server.ca_cert_path,
                config.server.client_cert_path,
                config.server.client_key_path,
                config.server.insecure_skip_verify,
            ),
    );

    let client = ws_client.clone();
    let client_task = tokio::spawn(async move {
        client.run().await;
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received termination signal. Stopping agent service...");
        }
    }

    client_task.abort();
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        ws_client.disconnect("Headless service terminated"),
    )
    .await;
    tracing::info!("Headless agent stopped cleanly.");
    Ok(())
}
