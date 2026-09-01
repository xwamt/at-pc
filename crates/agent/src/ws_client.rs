//! Agent WebSocket client module.
//! Manages connection lifecycle, auto-reconnect, registration handshake,
//! periodic heartbeat metrics, tool execution forwarding, and emergency disconnect.

use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::{HeartbeatMetrics, TerminalInfo};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

use crate::codec::{perform_client_handshake, WsMessage, WsReader, WsWriter};
use crate::executor::AgentExecutor;

/// Agent client connection lifecycle status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
}

/// Event listener interface for UI and audit logs
pub trait AgentEventListener: Send + Sync {
    fn on_status_change(&self, _status: ClientConnectionStatus) {}
    fn on_tool_start(&self, _call_id: &str, _tool_name: &str, _arguments: &serde_json::Value) {}
    fn on_tool_finish(&self, _call_id: &str, _tool_name: &str, _success: bool, _duration_ms: u64) {}
}

/// Default no-op event listener
#[derive(Debug, Default)]
pub struct NoopEventListener;
impl AgentEventListener for NoopEventListener {}

/// Parses WebSocket URL into (host_port, path)
pub fn parse_ws_url(url: &str) -> Result<(String, String), String> {
    let stripped = if let Some(s) = url.strip_prefix("ws://") {
        s
    } else if let Some(s) = url.strip_prefix("http://") {
        s
    } else {
        url
    };

    let (host, path) = match stripped.split_once('/') {
        Some((h, p)) => (h.to_string(), format!("/{}", p)),
        None => (stripped.to_string(), "/ws".to_string()),
    };

    if host.is_empty() {
        return Err("Host cannot be empty".to_string());
    }

    Ok((host, path))
}

/// Agent WebSocket client
pub struct AgentWsClient {
    pub server_url: String,
    pub auth_token: Option<String>,
    pub terminal_info: TerminalInfo,
    pub executor: Arc<AgentExecutor>,
    pub reconnect_interval_secs: u64,
    pub heartbeat_interval_secs: Arc<AtomicU64>,
    pub status: Arc<RwLock<ClientConnectionStatus>>,
    pub is_running: Arc<AtomicBool>,
    pub event_listener: Option<Arc<dyn AgentEventListener>>,
    outbound_tx: Arc<RwLock<Option<mpsc::UnboundedSender<AgentToServerMessage>>>>,
}

impl AgentWsClient {
    /// Creates a new AgentWsClient instance.
    pub fn new(
        server_url: String,
        terminal_info: TerminalInfo,
        executor: Arc<AgentExecutor>,
    ) -> Self {
        Self {
            server_url,
            auth_token: None,
            terminal_info,
            executor,
            reconnect_interval_secs: 5,
            heartbeat_interval_secs: Arc::new(AtomicU64::new(5)),
            status: Arc::new(RwLock::new(ClientConnectionStatus::Disconnected)),
            is_running: Arc::new(AtomicBool::new(true)),
            event_listener: None,
            outbound_tx: Arc::new(RwLock::new(None)),
        }
    }

    /// Sets optional auth token for registration
    pub fn with_auth_token(mut self, auth_token: Option<String>) -> Self {
        self.auth_token = auth_token;
        self
    }

    /// Sets reconnect interval in seconds
    pub fn with_reconnect_interval(mut self, secs: u64) -> Self {
        self.reconnect_interval_secs = if secs == 0 { 5 } else { secs };
        self
    }

    /// Sets event listener for status changes and audit logs
    pub fn with_listener(mut self, listener: Arc<dyn AgentEventListener>) -> Self {
        self.event_listener = Some(listener);
        self
    }

    /// Gets current connection status
    pub async fn status(&self) -> ClientConnectionStatus {
        *self.status.read().await
    }

    async fn set_status(&self, new_status: ClientConnectionStatus) {
        {
            let mut s = self.status.write().await;
            *s = new_status;
        }
        if let Some(ref l) = self.event_listener {
            l.on_status_change(new_status);
        }
    }

    /// Initiates graceful disconnect and stops the client
    pub async fn disconnect(&self, reason: &str) {
        info!("Disconnecting agent: {}", reason);
        self.is_running.store(false, Ordering::SeqCst);

        // Send Disconnect message if channel is open
        if let Some(ref tx) = *self.outbound_tx.read().await {
            let _ = tx.send(AgentToServerMessage::Disconnect {
                terminal_id: self.terminal_info.terminal_id.clone(),
                reason: reason.to_string(),
            });
        }

        // Kill all active subprocesses
        let killed = self.executor.kill_all_processes();
        if killed > 0 {
            info!("Emergency kill switch terminated {} subprocess(es)", killed);
        }

        self.set_status(ClientConnectionStatus::Disconnected).await;
    }

    /// Main connection and reconnect loop
    pub async fn run(&self) {
        let (host, path) = match parse_ws_url(&self.server_url) {
            Ok(res) => res,
            Err(e) => {
                error!("Invalid server URL '{}': {}", self.server_url, e);
                return;
            }
        };

        while self.is_running.load(Ordering::SeqCst) {
            self.set_status(ClientConnectionStatus::Connecting).await;
            info!("Connecting to server WebSocket at {}{}...", host, path);

            match TcpStream::connect(&host).await {
                Ok(stream) => {
                    info!("Connected to server TCP. Performing WebSocket handshake...");
                    if let Err(e) = self.handshake_and_run_stream(stream, &host, &path).await {
                        warn!("Agent session ended with error: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Failed to connect to server at {}: {}", host, e);
                }
            }

            if !self.is_running.load(Ordering::SeqCst) {
                break;
            }

            self.set_status(ClientConnectionStatus::Reconnecting).await;
            info!(
                "Reconnecting in {} seconds...",
                self.reconnect_interval_secs
            );
            tokio::time::sleep(Duration::from_secs(self.reconnect_interval_secs)).await;
        }

        self.set_status(ClientConnectionStatus::Disconnected).await;
    }

    /// Performs HTTP WebSocket upgrade handshake on a stream and starts the agent session
    pub async fn handshake_and_run_stream<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
        &self,
        mut stream: S,
        host: &str,
        path: &str,
    ) -> Result<(), String> {
        perform_client_handshake(&mut stream, host, path)
            .await
            .map_err(|e| format!("WebSocket handshake failed: {}", e))?;
        self.run_with_stream(stream).await
    }

    /// Runs agent session over an established upgraded WebSocket stream
    pub async fn run_with_stream<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
        &self,
        stream: S,
    ) -> Result<(), String> {
        let (reader, writer) = tokio::io::split(stream);
        let mut ws_reader = WsReader::new(reader);
        let mut ws_writer = WsWriter::new(writer);

        // 1. Send Register frame
        let reg_msg = AgentToServerMessage::Register {
            info: self.terminal_info.clone(),
            auth_token: self.auth_token.clone(),
        };
        let reg_json = serde_json::to_string(&reg_msg).map_err(|e| e.to_string())?;
        ws_writer
            .write_message(&WsMessage::Text(reg_json))
            .await
            .map_err(|e| format!("Failed to send Register message: {}", e))?;

        // 2. Await RegisterAck
        let ack_msg = tokio::time::timeout(Duration::from_secs(10), ws_reader.read_message())
            .await
            .map_err(|_| "Timed out waiting for RegisterAck from server".to_string())?
            .map_err(|e| format!("Error reading RegisterAck: {}", e))?;

        let ack_text = match ack_msg {
            WsMessage::Text(t) => t,
            _ => return Err("Expected text RegisterAck message from server".to_string()),
        };

        let server_ack: ServerToAgentMessage = serde_json::from_str(&ack_text)
            .map_err(|e| format!("Invalid RegisterAck JSON: {}", e))?;

        match server_ack {
            ServerToAgentMessage::RegisterAck {
                success,
                message,
                heartbeat_interval_secs,
            } => {
                if !success {
                    let err = message.unwrap_or_else(|| "Registration rejected by server".to_string());
                    return Err(format!("Registration failed: {}", err));
                }
                if heartbeat_interval_secs > 0 {
                    self.heartbeat_interval_secs
                        .store(heartbeat_interval_secs, Ordering::SeqCst);
                }
            }
            _ => return Err("First message from server must be RegisterAck".to_string()),
        }

        self.set_status(ClientConnectionStatus::Connected).await;
        info!(
            "Terminal [{}] successfully registered with server",
            self.terminal_info.terminal_id
        );

        // 3. Setup Outgoing Channel
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<AgentToServerMessage>();
        {
            let mut tx_guard = self.outbound_tx.write().await;
            *tx_guard = Some(outbound_tx.clone());
        }

        // 4. Spawn WebSocket Writer Task
        let write_task = tokio::spawn(async move {
            while let Some(msg) = outbound_rx.recv().await {
                if let Ok(json) = serde_json::to_string(&msg) {
                    if let Err(e) = ws_writer.write_message(&WsMessage::Text(json)).await {
                        warn!("Failed to write WS message: {}", e);
                        break;
                    }
                }
            }
        });

        // 5. Spawn Heartbeat Task
        let hb_tx = outbound_tx.clone();
        let term_id = self.terminal_info.terminal_id.clone();
        let hb_interval = self.heartbeat_interval_secs.load(Ordering::SeqCst);
        let hb_running = self.is_running.clone();

        let heartbeat_task = tokio::spawn(async move {
            let mut sys = sysinfo::System::new_all();
            while hb_running.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_secs(hb_interval)).await;
                if !hb_running.load(Ordering::SeqCst) {
                    break;
                }

                sys.refresh_cpu();
                sys.refresh_memory();

                let metrics = HeartbeatMetrics {
                    cpu_usage_percent: sys.global_cpu_info().cpu_usage(),
                    memory_used_mb: sys.used_memory() / (1024 * 1024),
                    memory_total_mb: sys.total_memory() / (1024 * 1024),
                    uptime_secs: sysinfo::System::uptime(),
                    timestamp: chrono::Utc::now().timestamp(),
                };

                let hb_msg = AgentToServerMessage::Heartbeat {
                    terminal_id: term_id.clone(),
                    metrics,
                };

                if hb_tx.send(hb_msg).is_err() {
                    break;
                }
            }
        });

        // 6. Incoming Message Loop
        let mut session_error = None;
        while self.is_running.load(Ordering::SeqCst) {
            match ws_reader.read_message().await {
                Ok(WsMessage::Text(text)) => {
                    match serde_json::from_str::<ServerToAgentMessage>(&text) {
                        Ok(server_msg) => {
                            self.handle_server_message(server_msg, &outbound_tx).await;
                        }
                        Err(e) => {
                            warn!("Failed to parse server message: {} (raw: {})", e, text);
                        }
                    }
                }
                Ok(WsMessage::Ping(payload)) => {
                    let _ = ws_writer_ping_response(&outbound_tx, payload).await;
                }
                Ok(WsMessage::Pong(_)) => {}
                Ok(WsMessage::Close) => {
                    info!("Server requested WebSocket close");
                    break;
                }
                Ok(WsMessage::Binary(_)) => {
                    warn!("Received unexpected binary frame from server");
                }
                Err(e) => {
                    debug!("WebSocket read error: {}", e);
                    session_error = Some(e.to_string());
                    break;
                }
            }
        }

        // Cleanup
        {
            let mut tx_guard = self.outbound_tx.write().await;
            *tx_guard = None;
        }
        heartbeat_task.abort();
        write_task.abort();

        if let Some(err) = session_error {
            Err(err)
        } else {
            Ok(())
        }
    }

    async fn handle_server_message(
        &self,
        msg: ServerToAgentMessage,
        outbound_tx: &mpsc::UnboundedSender<AgentToServerMessage>,
    ) {
        match msg {
            ServerToAgentMessage::InvokeTool {
                call_id,
                tool_name,
                arguments,
                timeout_secs,
            } => {
                info!("Executing tool '{}' (call_id: {})", tool_name, call_id);
                if let Some(ref l) = self.event_listener {
                    l.on_tool_start(&call_id, &tool_name, &arguments);
                }

                let executor = self.executor.clone();
                let tx = outbound_tx.clone();
                let listener = self.event_listener.clone();
                let cid = call_id.clone();
                let tname = tool_name.clone();

                tokio::spawn(async move {
                    let start = std::time::Instant::now();
                    let timeout = if timeout_secs == 0 { 35 } else { timeout_secs };
                    let exec_fut = executor.execute(&tname, arguments);

                    let (success, result, error) = match tokio::time::timeout(Duration::from_secs(timeout), exec_fut).await {
                        Ok(Ok(val)) => (true, val, None),
                        Ok(Err(err)) => (false, serde_json::json!({}), Some(err)),
                        Err(_) => (
                            false,
                            serde_json::json!({}),
                            Some(format!("Tool '{}' timed out after {}s", tname, timeout)),
                        ),
                    };

                    let duration_ms = start.elapsed().as_millis() as u64;
                    if let Some(ref l) = listener {
                        l.on_tool_finish(&cid, &tname, success, duration_ms);
                    }

                    let res_msg = AgentToServerMessage::ToolResult {
                        call_id: cid,
                        success,
                        result,
                        error,
                        duration_ms,
                    };
                    let _ = tx.send(res_msg);
                });
            }
            ServerToAgentMessage::CancelTool { call_id } => {
                info!("Server requested tool cancellation for call_id: {}", call_id);
                self.executor.cancel(&call_id);
            }
            ServerToAgentMessage::HeartbeatAck { server_timestamp } => {
                debug!("Heartbeat acknowledged by server at timestamp: {}", server_timestamp);
            }
            ServerToAgentMessage::RegisterAck { .. } => {
                debug!("Received subsequent RegisterAck");
            }
        }
    }
}

async fn ws_writer_ping_response(
    _tx: &mpsc::UnboundedSender<AgentToServerMessage>,
    _payload: Vec<u8>,
) -> Result<(), ()> {
    Ok(())
}
