pub mod handler;
pub mod registry;

use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

use crate::config::ServerConfig;
pub use handler::{
    handle_connection, handle_stream, handle_stream_with_peer, AgentMessageHandler,
    NoopMessageHandler, WsServerState,
};
pub use registry::{TerminalEntry, TerminalRegistry, TerminalSession, TerminalStatus};

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
    let state = WsServerState::with_handler(registry, config, message_handler);
    start_ws_server_with_state(state, port).await
}

/// Start WebSocket server with given WsServerState on a specified port
pub async fn start_ws_server_with_state(
    state: WsServerState,
    port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ip: std::net::IpAddr = state
        .config
        .listen_host
        .parse()
        .unwrap_or_else(|_| std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));
    let addr = std::net::SocketAddr::new(ip, port);
    let mut attempts = 0;
    let listener = loop {
        match TcpListener::bind(addr).await {
            Ok(l) => break l,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && attempts < 10 => {
                attempts += 1;
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
            Err(e) => return Err(e.into()),
        }
    };
    start_ws_server_with_listener(state, listener).await
}

/// Start WebSocket server with given WsServerState on an existing TcpListener
pub async fn start_ws_server_with_listener(
    state: WsServerState,
    listener: TcpListener,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let local_addr = listener.local_addr()?;
    let tls_acceptor = crate::tls::create_tls_acceptor(&state.config)?;

    if tls_acceptor.is_some() {
        info!(
            "WebSocket gateway listening on wss://{}{} (TLS encrypted)",
            local_addr, state.config.ws_path
        );
    } else {
        info!(
            "WebSocket gateway listening on ws://{}{}",
            local_addr, state.config.ws_path
        );
    }

    loop {
        let (socket, peer_addr) = listener.accept().await?;
        let peer_addr = socket.peer_addr().ok().or(Some(peer_addr));
        let state_clone = state.clone();
        let acceptor_clone = tls_acceptor.clone();

        tokio::spawn(async move {
            if let Some(addr) = peer_addr {
                tracing::debug!("New TCP connection from {}", addr);
            }
            if let Some(acceptor) = acceptor_clone {
                match acceptor.accept(socket).await {
                    Ok(tls_stream) => {
                        handle_stream_with_peer(tls_stream, state_clone, peer_addr).await;
                    }
                    Err(e) => {
                        tracing::debug!("TLS handshake failed: {}", e);
                    }
                }
            } else {
                handle_stream_with_peer(socket, state_clone, peer_addr).await;
            }
        });
    }
}
