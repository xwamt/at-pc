use std::sync::Arc;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use at_pc_protocol::messages::{AgentToServerMessage, BinaryDesktopFrame, ServerToAgentMessage};
use at_pc_protocol::models::{TerminalInfo, TerminalStatus};
use base64::Engine;
use crate::config::ServerConfig;
use crate::ws::codec::compute_accept_key;
use crate::ws::registry::TerminalRegistry;

/// Trait for handling tool results and agent messages
pub trait AgentMessageHandler: Send + Sync {
    fn handle_tool_result(&self, msg: AgentToServerMessage);
    fn handle_terminal_disconnected(&self, _terminal_id: &str, _reason: &str) {}
    #[allow(clippy::too_many_arguments)]
    fn handle_desktop_frame(
        &self,
        _terminal_id: &str,
        _display_index: u32,
        _width: u32,
        _height: u32,
        _format: &str,
        _data: &str,
        _timestamp: u64,
    ) {}
    fn handle_desktop_frame_binary(
        &self,
        terminal_id: &str,
        frame: BinaryDesktopFrame,
    ) {
        let b64 = base64::prelude::BASE64_STANDARD.encode(&frame.data);
        self.handle_desktop_frame(
            terminal_id,
            frame.display_index,
            frame.width,
            frame.height,
            "jpeg",
            &b64,
            frame.timestamp,
        );
    }
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

/// Perform HTTP handshake upgrade to WebSocket on any AsyncRead + AsyncWrite stream (helper)
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

/// Handle a full connected TCP stream upgraded to WebSocket using standard production-grade WebSocket stack
pub async fn handle_connection(stream: TcpStream, state: WsServerState) {
    handle_stream(stream, state).await;
}

/// Handle any connected async stream upgraded to WebSocket using tokio-tungstenite
pub async fn handle_stream<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    stream: S,
    state: WsServerState,
) {
    let ws_path = state.config.ws_path.clone();
    #[allow(clippy::result_large_err)]
    let callback = move |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
                         resp: tokio_tungstenite::tungstenite::handshake::server::Response| {
        let path = req.uri().path();
        if path == "/health" {
            let res = tokio_tungstenite::tungstenite::http::Response::builder()
                .status(200)
                .body(Some("OK".to_string()))
                .unwrap();
            return Err(res);
        }
        if !ws_path.is_empty() && path != ws_path {
            let res = tokio_tungstenite::tungstenite::http::Response::builder()
                .status(404)
                .body(Some("Not Found".to_string()))
                .unwrap();
            return Err(res);
        }
        Ok(resp)
    };

    let ws_connection = match tokio_tungstenite::accept_hdr_async(stream, callback).await {
        Ok(ws) => ws,
        Err(e) => {
            debug!("WebSocket handshake error: {}", e);
            return;
        }
    };

    let (mut ws_sink, mut ws_reader) = ws_connection.split();

    // 1. Initial Handshake: Wait for Register message
    let (terminal_info, initial_sender_tx, mut initial_sender_rx, session_id) =
        match wait_for_registration(&mut ws_reader, &mut ws_sink, &state).await {
            Ok(res) => res,
            Err(err_msg) => {
                warn!("WebSocket registration handshake failed: {}", err_msg);
                let _ = ws_sink.send(tokio_tungstenite::tungstenite::Message::Close(None)).await;
                return;
            }
        };

    let terminal_id = terminal_info.terminal_id.clone();
    info!(
        "Terminal [{}] ({}) successfully registered (session: {}) from IP: {}",
        terminal_id, terminal_info.hostname, session_id, terminal_info.lan_ip
    );

    // 2. Outgoing message forwarding task (MPSC channel -> WebSocket Sink)
    let (control_tx, mut control_rx) = mpsc::unbounded_channel::<tokio_tungstenite::tungstenite::Message>();
    let forwarder_tx = initial_sender_tx.clone();
    let term_id_for_send = terminal_id.clone();
    let forward_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;

                // Priority 0: RFC 6455 raw control frames (Pong, Close)
                Some(ctrl_msg) = control_rx.recv() => {
                    if let Err(e) = ws_sink.send(ctrl_msg).await {
                        warn!("Failed to send control WS message to terminal [{}]: {}", term_id_for_send, e);
                        break;
                    }
                }

                // Priority 1: Application messages from server to terminal
                Some(msg) = initial_sender_rx.recv() => {
                    let mut final_msg = msg;
                    if matches!(
                        final_msg,
                        ServerToAgentMessage::DesktopInput {
                            event: at_pc_protocol::models::DesktopInputEvent::MouseMove { .. }
                        }
                    ) {
                        while let Ok(next) = initial_sender_rx.try_recv() {
                            let is_mouse_move = matches!(
                                next,
                                ServerToAgentMessage::DesktopInput {
                                    event: at_pc_protocol::models::DesktopInputEvent::MouseMove { .. }
                                }
                            );
                            if is_mouse_move {
                                final_msg = next;
                            } else {
                                if let Ok(json) = serde_json::to_string(&final_msg) {
                                    let _ = ws_sink.send(tokio_tungstenite::tungstenite::Message::Text(json)).await;
                                }
                                final_msg = next;
                                break;
                            }
                        }
                    }

                    match serde_json::to_string(&final_msg) {
                        Ok(json) => {
                            let ws_msg = tokio_tungstenite::tungstenite::Message::Text(json);
                            if let Err(e) = ws_sink.send(ws_msg).await {
                                warn!("Failed to send WS message to terminal [{}]: {}", term_id_for_send, e);
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Failed to serialize message for [{}]: {}", term_id_for_send, e);
                        }
                    }
                }

                else => break,
            }
        }
        warn!("WebSocket forwarding task for [{}] exited.", term_id_for_send);
    });

    // 3. Incoming message loop (WebSocket Reader -> Registry & MessageHandler)
    let idle_timeout = std::time::Duration::from_secs(state.config.offline_threshold_secs.max(15) * 2);
    loop {
        let msg_res = match tokio::time::timeout(idle_timeout, ws_reader.next()).await {
            Ok(Some(res)) => res,
            Ok(None) => break,
            Err(_) => {
                warn!("WebSocket read timed out for terminal [{}] (no message for {}s)", terminal_id, idle_timeout.as_secs());
                break;
            }
        };

        match msg_res {
            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
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
            Ok(tokio_tungstenite::tungstenite::Message::Binary(bin)) => {
                if bin.starts_with(&BinaryDesktopFrame::MAGIC) {
                    match BinaryDesktopFrame::decode(&bin) {
                        Ok(frame) => {
                            if let Some(ref handler) = state.message_handler {
                                handler.handle_desktop_frame_binary(&terminal_id, frame);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to decode binary desktop frame from [{}]: {}", terminal_id, e);
                        }
                    }
                } else {
                    warn!("Received unexpected binary frame ({} bytes) from terminal [{}]", bin.len(), terminal_id);
                }
            }
            Ok(tokio_tungstenite::tungstenite::Message::Ping(payload)) => {
                debug!("Received ping from terminal [{}] ({} bytes); replying with Pong", terminal_id, payload.len());
                let _ = control_tx.send(tokio_tungstenite::tungstenite::Message::Pong(payload));
            }
            Ok(tokio_tungstenite::tungstenite::Message::Pong(_)) => {
                debug!("Received pong from terminal [{}]", terminal_id);
            }
            Ok(tokio_tungstenite::tungstenite::Message::Close(frame)) => {
                info!("Terminal [{}] closed connection gracefully", terminal_id);
                let _ = control_tx.send(tokio_tungstenite::tungstenite::Message::Close(frame));
                break;
            }
            Ok(tokio_tungstenite::tungstenite::Message::Frame(_)) => {}
            Err(e) => {
                debug!("WebSocket connection ended for [{}]: {}", terminal_id, e);
                break;
            }
        }
    }

    // 4. Cleanup on disconnect
    info!("Terminal [{}] disconnected. Cleaning up session {}.", terminal_id, session_id);
    state.registry.set_status_if_current(&terminal_id, session_id, TerminalStatus::Offline).await;
    if let Some(ref handler) = state.message_handler {
        handler.handle_terminal_disconnected(&terminal_id, "WebSocket connection closed");
    }
    forward_task.abort();
}

/// Wait for the first message to be a valid Register payload
async fn wait_for_registration<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    reader: &mut futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<S>>,
    sink: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<S>, tokio_tungstenite::tungstenite::Message>,
    state: &WsServerState,
) -> Result<
    (
        TerminalInfo,
        mpsc::UnboundedSender<ServerToAgentMessage>,
        mpsc::UnboundedReceiver<ServerToAgentMessage>,
        u64,
    ),
    String,
> {
    let first_msg_opt = tokio::time::timeout(std::time::Duration::from_secs(10), reader.next())
        .await
        .map_err(|_| "Timed out waiting for initial Register message".to_string())?;

    let first_msg = match first_msg_opt {
        Some(Ok(m)) => m,
        Some(Err(e)) => return Err(format!("WebSocket read error during handshake: {}", e)),
        None => return Err("Stream closed before Register message".to_string()),
    };

    let text = match first_msg {
        tokio_tungstenite::tungstenite::Message::Text(t) => t,
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
                    let _ = sink.send(tokio_tungstenite::tungstenite::Message::Text(json)).await;
                    return Err("Authentication failed".to_string());
                }
            }

            // Create outbound channel for this terminal session
            let (tx, rx) = mpsc::unbounded_channel::<ServerToAgentMessage>();
            let session_id = state.registry.register(info.clone(), tx.clone()).await;

            // Send success RegisterAck
            let ack = ServerToAgentMessage::RegisterAck {
                success: true,
                message: None,
                heartbeat_interval_secs: state.config.heartbeat_interval_secs,
            };
            let json = serde_json::to_string(&ack).unwrap();
            sink
                .send(tokio_tungstenite::tungstenite::Message::Text(json))
                .await
                .map_err(|e| format!("Failed to send RegisterAck: {}", e))?;

            Ok((info, tx, rx, session_id))
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
            if let Some(ref handler) = state.message_handler {
                handler.handle_terminal_disconnected(target_id, &reason);
            }
        }
        AgentToServerMessage::Register { info, .. } => {
            debug!("Re-registering terminal [{}]", info.terminal_id);
            state.registry.register(info, tx.clone()).await;
        }
        AgentToServerMessage::DesktopFrame {
            display_index,
            width,
            height,
            format,
            data,
            timestamp,
        } => {
            if let Some(handler) = &state.message_handler {
                handler.handle_desktop_frame(
                    terminal_id,
                    display_index,
                    width,
                    height,
                    &format,
                    &data,
                    timestamp,
                );
            }
        }
    }
}
