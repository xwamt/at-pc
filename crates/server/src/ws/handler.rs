use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::{TerminalInfo, TerminalStatus};
use crate::config::ServerConfig;
use crate::ws::codec::{compute_accept_key, WsMessage, WsReader, WsWriter};
use crate::ws::registry::TerminalRegistry;

/// Trait for handling tool results and agent messages
pub trait AgentMessageHandler: Send + Sync {
    fn handle_tool_result(&self, msg: AgentToServerMessage);
}

/// No-op implementation of AgentMessageHandler
#[derive(Debug, Clone, Default)]
pub struct NoopMessageHandler;

impl AgentMessageHandler for NoopMessageHandler {
    fn handle_tool_result(&self, _msg: AgentToServerMessage) {}
}

impl<F> AgentMessageHandler for F
where
    F: Fn(AgentToServerMessage) + Send + Sync,
{
    fn handle_tool_result(&self, msg: AgentToServerMessage) {
        self(msg);
    }
}

/// Shared state for WebSocket server
#[derive(Clone)]
pub struct WsServerState {
    pub registry: Arc<TerminalRegistry>,
    pub config: ServerConfig,
    pub message_handler: Option<Arc<dyn AgentMessageHandler>>,
}

impl WsServerState {
    pub fn new(registry: Arc<TerminalRegistry>, config: ServerConfig) -> Self {
        Self {
            registry,
            config,
            message_handler: None,
        }
    }

    pub fn with_handler(
        registry: Arc<TerminalRegistry>,
        config: ServerConfig,
        handler: Arc<dyn AgentMessageHandler>,
    ) -> Self {
        Self {
            registry,
            config,
            message_handler: Some(handler),
        }
    }
}

/// Perform HTTP handshake upgrade to WebSocket on any AsyncRead + AsyncWrite stream
pub async fn perform_ws_handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    expected_path: &str,
) -> Result<bool, std::io::Error> {
    let mut buffer = [0u8; 4096];
    let mut total_read = 0;

    while total_read < buffer.len() {
        let n = stream.read(&mut buffer[total_read..]).await?;
        if n == 0 {
            return Ok(false);
        }
        total_read += n;

        if let Some(pos) = buffer[..total_read].windows(4).position(|w| w == b"\r\n\r\n") {
            let request_str = String::from_utf8_lossy(&buffer[..pos]);
            
            // Check request line and headers
            let lines: Vec<&str> = request_str.lines().collect();
            if lines.is_empty() {
                return Ok(false);
            }

            let first_line = lines[0];
            let mut parts = first_line.split_whitespace();
            let method = parts.next().unwrap_or("");
            let path = parts.next().unwrap_or("");

            if method != "GET" {
                let resp = "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n";
                stream.write_all(resp.as_bytes()).await?;
                return Ok(false);
            }

            if path == "/health" {
                let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nOK";
                stream.write_all(resp.as_bytes()).await?;
                return Ok(false);
            }

            if path != expected_path && !expected_path.is_empty() {
                let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
                stream.write_all(resp.as_bytes()).await?;
                return Ok(false);
            }

            // Extract Sec-WebSocket-Key
            let mut sec_ws_key = None;
            for line in &lines[1..] {
                if let Some((k, v)) = line.split_once(':') {
                    if k.trim().eq_ignore_ascii_case("Sec-WebSocket-Key") {
                        sec_ws_key = Some(v.trim());
                        break;
                    }
                }
            }

            let sec_key = match sec_ws_key {
                Some(k) => k,
                None => {
                    let resp = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n";
                    stream.write_all(resp.as_bytes()).await?;
                    return Ok(false);
                }
            };

            let accept_key = compute_accept_key(sec_key);
            let response = format!(
                "HTTP/1.1 101 Switching Protocols\r\n\
                Upgrade: websocket\r\n\
                Connection: Upgrade\r\n\
                Sec-WebSocket-Accept: {}\r\n\r\n",
                accept_key
            );

            stream.write_all(response.as_bytes()).await?;
            stream.flush().await?;
            return Ok(true);
        }
    }

    Ok(false)
}

/// Handle a full connected TCP stream upgraded to WebSocket
pub async fn handle_connection(stream: TcpStream, state: WsServerState) {
    handle_stream(stream, state).await;
}

/// Handle any connected async stream upgraded to WebSocket (useful for TCP and in-memory duplex testing)
pub async fn handle_stream<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    mut stream: S,
    state: WsServerState,
) {
    let ws_path = state.config.ws_path.clone();
    match perform_ws_handshake(&mut stream, &ws_path).await {
        Ok(true) => {}
        Ok(false) => return,
        Err(e) => {
            warn!("WebSocket handshake error: {}", e);
            return;
        }
    }

    let (reader, writer) = tokio::io::split(stream);
    let mut ws_reader = WsReader::new(reader);
    let mut ws_writer = WsWriter::new(writer);

    // 1. Initial Handshake: Wait for Register message
    let (terminal_info, initial_sender_tx, mut initial_sender_rx) =
        match wait_for_registration(&mut ws_reader, &mut ws_writer, &state).await {
            Ok(res) => res,
            Err(err_msg) => {
                warn!("WebSocket registration handshake failed: {}", err_msg);
                let _ = ws_writer.write_message(&WsMessage::Close).await;
                return;
            }
        };

    let terminal_id = terminal_info.terminal_id.clone();
    info!(
        "Terminal [{}] ({}) successfully registered from IP: {}",
        terminal_id, terminal_info.hostname, terminal_info.lan_ip
    );

    // 2. Outgoing message forwarding task (MPSC channel -> WebSocket Writer)
    let forwarder_tx = initial_sender_tx.clone();
    let term_id_for_send = terminal_id.clone();
    let forward_task = tokio::spawn(async move {
        while let Some(msg) = initial_sender_rx.recv().await {
            match serde_json::to_string(&msg) {
                Ok(json) => {
                    if let Err(e) = ws_writer.write_message(&WsMessage::Text(json)).await {
                        warn!("Failed to send WS message to terminal [{}]: {}", term_id_for_send, e);
                        break;
                    }
                }
                Err(e) => {
                    error!("Failed to serialize message for [{}]: {}", term_id_for_send, e);
                }
            }
        }
    });

    // 3. Incoming message loop (WebSocket Reader -> Registry & MessageHandler)
    loop {
        match ws_reader.read_message().await {
            Ok(WsMessage::Text(text)) => {
                match serde_json::from_str::<AgentToServerMessage>(&text) {
                    Ok(agent_msg) => {
                        handle_agent_message(agent_msg, &terminal_id, &state, &forwarder_tx).await;
                    }
                    Err(e) => {
                        warn!(
                            "Invalid message format from terminal [{}]: {} (raw: {})",
                            terminal_id, e, text
                        );
                    }
                }
            }
            Ok(WsMessage::Ping(_payload)) => {
                debug!("Received ping from terminal [{}]", terminal_id);
                let _ = forwarder_tx.send(ServerToAgentMessage::HeartbeatAck {
                    server_timestamp: chrono::Utc::now().timestamp(),
                });
            }
            Ok(WsMessage::Pong(_)) => {
                debug!("Received pong from terminal [{}]", terminal_id);
            }
            Ok(WsMessage::Close) => {
                info!("Terminal [{}] closed connection gracefully", terminal_id);
                break;
            }
            Ok(WsMessage::Binary(_)) => {
                warn!("Received unexpected binary frame from terminal [{}]", terminal_id);
            }
            Err(e) => {
                debug!("WebSocket connection ended for [{}]: {}", terminal_id, e);
                break;
            }
        }
    }

    // 4. Cleanup on disconnect
    info!("Terminal [{}] disconnected. Cleaning up session.", terminal_id);
    state.registry.set_status(&terminal_id, TerminalStatus::Offline).await;
    forward_task.abort();
}

/// Wait for the first message to be a valid Register payload
async fn wait_for_registration<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut WsReader<R>,
    writer: &mut WsWriter<W>,
    state: &WsServerState,
) -> Result<
    (
        TerminalInfo,
        mpsc::UnboundedSender<ServerToAgentMessage>,
        mpsc::UnboundedReceiver<ServerToAgentMessage>,
    ),
    String,
> {
    let first_msg = tokio::time::timeout(std::time::Duration::from_secs(10), reader.read_message())
        .await
        .map_err(|_| "Timed out waiting for initial Register message".to_string())?
        .map_err(|e| format!("WebSocket read error during handshake: {}", e))?;

    let text = match first_msg {
        WsMessage::Text(t) => t,
        _ => return Err("Expected Text message containing Register frame".to_string()),
    };

    let msg: AgentToServerMessage = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse Register message: {}", e))?;

    match msg {
        AgentToServerMessage::Register { info, auth_token } => {
            // Check auth_token if configured
            if let Some(expected_token) = &state.config.auth_token {
                if auth_token.as_ref() != Some(expected_token) {
                    let ack = ServerToAgentMessage::RegisterAck {
                        success: false,
                        message: Some("Invalid authentication token".to_string()),
                        heartbeat_interval_secs: state.config.heartbeat_interval_secs,
                    };
                    let json = serde_json::to_string(&ack).unwrap();
                    let _ = writer.write_message(&WsMessage::Text(json)).await;
                    return Err("Authentication failed".to_string());
                }
            }

            // Create outbound channel for this terminal session
            let (tx, rx) = mpsc::unbounded_channel::<ServerToAgentMessage>();
            state.registry.register(info.clone(), tx.clone()).await;

            // Send success RegisterAck
            let ack = ServerToAgentMessage::RegisterAck {
                success: true,
                message: None,
                heartbeat_interval_secs: state.config.heartbeat_interval_secs,
            };
            let json = serde_json::to_string(&ack).unwrap();
            writer
                .write_message(&WsMessage::Text(json))
                .await
                .map_err(|e| format!("Failed to send RegisterAck: {}", e))?;

            Ok((info, tx, rx))
        }
        _ => Err("First message must be Register".to_string()),
    }
}

/// Dispatch incoming AgentToServerMessage
async fn handle_agent_message(
    msg: AgentToServerMessage,
    terminal_id: &str,
    state: &WsServerState,
    tx: &mpsc::UnboundedSender<ServerToAgentMessage>,
) {
    match msg {
        AgentToServerMessage::Heartbeat {
            terminal_id: hid,
            metrics,
        } => {
            let target_id = if hid.is_empty() { terminal_id } else { &hid };
            if let Err(e) = state.registry.update_heartbeat(target_id, metrics).await {
                warn!("Failed to update heartbeat for [{}]: {}", target_id, e);
            }
            // Send HeartbeatAck
            let ack = ServerToAgentMessage::HeartbeatAck {
                server_timestamp: chrono::Utc::now().timestamp(),
            };
            let _ = tx.send(ack);
        }
        AgentToServerMessage::ToolResult { .. } => {
            if let Some(handler) = &state.message_handler {
                handler.handle_tool_result(msg);
            } else {
                debug!("Received ToolResult from [{}], but no handler is configured", terminal_id);
            }
        }
        AgentToServerMessage::Disconnect { terminal_id: did, reason } => {
            let target_id = if did.is_empty() { terminal_id } else { &did };
            info!("Terminal [{}] initiated disconnect: {}", target_id, reason);
            state.registry.set_status(target_id, TerminalStatus::Offline).await;
        }
        AgentToServerMessage::Register { info, .. } => {
            debug!("Re-registering terminal [{}]", info.terminal_id);
            state.registry.register(info, tx.clone()).await;
        }
    }
}
