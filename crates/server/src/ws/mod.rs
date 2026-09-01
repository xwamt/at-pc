pub mod codec;
pub mod handler;
pub mod registry;

use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

pub use codec::{compute_accept_key, WsMessage, WsReader, WsWriter};
pub use handler::{
    handle_connection, handle_stream, perform_ws_handshake, AgentMessageHandler,
    NoopMessageHandler, WsServerState,
};
pub use registry::{TerminalEntry, TerminalRegistry, TerminalSession, TerminalStatus};
use crate::config::ServerConfig;

/// Helper function to start the standalone WebSocket server with registry and message handler
pub async fn start_ws_server<H: AgentMessageHandler + 'static>(
    registry: Arc<TerminalRegistry>,
    message_handler: Arc<H>,
    port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = ServerConfig {
        ws_port: port,
        ..Default::default()
    };
    let state = WsServerState {
        registry,
        config,
        message_handler: Some(message_handler),
    };
    start_ws_server_with_state(state, port).await
}

/// Start WebSocket server with given WsServerState on a specified port
pub async fn start_ws_server_with_state(
    state: WsServerState,
    port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    info!(
        "WebSocket gateway listening on ws://0.0.0.0:{}{}",
        port, state.config.ws_path
    );

    loop {
        let (socket, peer_addr) = listener.accept().await?;
        let state_clone = state.clone();
        tokio::spawn(async move {
            tracing::debug!("New TCP connection from {}", peer_addr);
            handle_connection(socket, state_clone).await;
        });
    }
}
