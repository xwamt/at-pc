//! Agent WebSocket client module.
//! Manages connection lifecycle, auto-reconnect, registration handshake,
//! periodic heartbeat metrics, tool execution forwarding, and emergency disconnect.

use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::{HeartbeatMetrics, TerminalInfo};
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};

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

/// Checks if URL scheme indicates TLS / WSS
pub fn is_tls_url(url: &str) -> bool {
    url.starts_with("wss://") || url.starts_with("https://")
}

/// Parses WebSocket URL into (host_port, path)
pub fn parse_ws_url(url: &str) -> Result<(String, String), String> {
    let stripped = if let Some(s) = url.strip_prefix("ws://") {
        s
    } else if let Some(s) = url.strip_prefix("http://") {
        s
    } else if let Some(s) = url.strip_prefix("wss://") {
        s
    } else if let Some(s) = url.strip_prefix("https://") {
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
    server_url: Arc<RwLock<String>>,
    pub auth_token: Option<String>,
    pub terminal_info: TerminalInfo,
    pub executor: Arc<AgentExecutor>,
    pub reconnect_interval_secs: u64,
    pub heartbeat_interval_secs: Arc<AtomicU64>,
    pub status: Arc<RwLock<ClientConnectionStatus>>,
    pub is_running: Arc<AtomicBool>,
    pub event_listener: Option<Arc<dyn AgentEventListener>>,
    pub stream_controller: Arc<crate::stream::DesktopStreamController>,
    pub ca_cert_path: Option<std::path::PathBuf>,
    pub client_cert_path: Option<std::path::PathBuf>,
    pub client_key_path: Option<std::path::PathBuf>,
    pub insecure_skip_verify: bool,
    outbound_tx: Arc<RwLock<Option<mpsc::UnboundedSender<AgentToServerMessage>>>>,
    loop_lock: Arc<Mutex<()>>,
    input_tx: mpsc::UnboundedSender<at_pc_protocol::models::DesktopInputEvent>,
}

impl AgentWsClient {
    /// Creates a new AgentWsClient instance.
    pub fn new(
        server_url: String,
        terminal_info: TerminalInfo,
        executor: Arc<AgentExecutor>,
    ) -> Self {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel::<at_pc_protocol::models::DesktopInputEvent>();
        std::thread::Builder::new()
            .name("desktop-input-worker".to_string())
            .spawn(move || {
                while let Some(event) = input_rx.blocking_recv() {
                    if let Err(e) = crate::input::inject_input_event(event) {
                        tracing::warn!("Failed to inject remote desktop input event: {}", e);
                    }
                }
            })
            .expect("failed to spawn desktop-input-worker");

        Self {
            server_url: Arc::new(RwLock::new(server_url)),
            auth_token: None,
            terminal_info,
            executor,
            reconnect_interval_secs: 5,
            heartbeat_interval_secs: Arc::new(AtomicU64::new(5)),
            status: Arc::new(RwLock::new(ClientConnectionStatus::Disconnected)),
            is_running: Arc::new(AtomicBool::new(true)),
            event_listener: None,
            stream_controller: Arc::new(crate::stream::DesktopStreamController::new()),
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
            insecure_skip_verify: false,
            outbound_tx: Arc::new(RwLock::new(None)),
            loop_lock: Arc::new(Mutex::new(())),
            input_tx,
        }
    }

    /// Sets optional auth token for registration
    pub fn with_auth_token(mut self, auth_token: Option<String>) -> Self {
        self.auth_token = auth_token;
        self
    }

    /// Sets TLS configuration for encrypted WSS communication
    pub fn with_tls_config(
        mut self,
        ca_cert_path: Option<std::path::PathBuf>,
        client_cert_path: Option<std::path::PathBuf>,
        client_key_path: Option<std::path::PathBuf>,
        insecure_skip_verify: bool,
    ) -> Self {
        self.ca_cert_path = ca_cert_path;
        self.client_cert_path = client_cert_path;
        self.client_key_path = client_key_path;
        self.insecure_skip_verify = insecure_skip_verify;
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

    /// Resets the client running flag so it can reconnect after disconnect
    pub fn reset_running(&self) {
        self.is_running.store(true, Ordering::SeqCst);
    }

    /// Returns whether the client loop is actively running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Returns the current target server URL
    pub async fn server_url(&self) -> String {
        let guard = self.server_url.read().await;
        guard.clone()
    }

    /// Updates target server URL for future connection attempts
    pub async fn set_server_url(&self, new_url: String) {
        let mut guard = self.server_url.write().await;
        *guard = new_url;
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

        self.stream_controller.stop();

        {
            let mut tx_guard = self.outbound_tx.write().await;
            *tx_guard = None;
        }

        self.set_status(ClientConnectionStatus::Disconnected).await;
    }

    /// Gracefully aborts active connection stream to trigger fast reconnect
    pub async fn abort_active_session(&self) {
        if let Some(ref tx) = *self.outbound_tx.read().await {
            let _ = tx.send(AgentToServerMessage::Disconnect {
                terminal_id: self.terminal_info.terminal_id.clone(),
                reason: "Reconnecting to updated server URL".to_string(),
            });
        }
        self.stream_controller.stop();
    }

    /// Main connection and reconnect loop
    pub async fn run(&self) {
        let _guard = match self.loop_lock.try_lock() {
            Ok(g) => g,
            Err(_) => {
                debug!("AgentWsClient::run() loop already active in another task; skipping duplicate loop");
                return;
            }
        };

        while self.is_running.load(Ordering::SeqCst) {
            let current_url = self.server_url().await;
            let (host, path) = match parse_ws_url(&current_url) {
                Ok(res) => res,
                Err(e) => {
                    error!("Invalid server URL '{}': {}", current_url, e);
                    tokio::time::sleep(Duration::from_secs(self.reconnect_interval_secs)).await;
                    continue;
                }
            };

            let is_tls = is_tls_url(&current_url);
            self.set_status(ClientConnectionStatus::Connecting).await;
            info!("Connecting to server WebSocket at {}{} (TLS: {})...", host, path, is_tls);

            let connect_timeout = Duration::from_secs(5);
            let connect_addr = if host.contains(':') {
                host.clone()
            } else {
                format!("{}:9801", host)
            };

            match tokio::time::timeout(connect_timeout, TcpStream::connect(&connect_addr)).await {
                Ok(Ok(stream)) => {
                    info!("Connected to server TCP. Performing WebSocket handshake...");
                    let res = if is_tls {
                        let host_no_port = host.split(':').next().unwrap_or(&host);
                        match rustls_pki_types::ServerName::try_from(host_no_port.to_string()) {
                            Ok(server_name) => {
                                match crate::tls::create_tls_connector(
                                    self.ca_cert_path.as_deref(),
                                    self.client_cert_path.as_deref(),
                                    self.client_key_path.as_deref(),
                                    self.insecure_skip_verify,
                                ) {
                                    Ok(connector) => {
                                        match connector.connect(server_name, stream).await {
                                            Ok(tls_stream) => {
                                                self.handshake_and_run_stream(tls_stream, &host, &path).await
                                            }
                                            Err(e) => Err(format!("TLS handshake failed with {}: {}", host_no_port, e)),
                                        }
                                    }
                                    Err(e) => Err(format!("Failed to build TLS connector: {}", e)),
                                }
                            }
                            Err(e) => Err(format!("Invalid DNS/IP server name '{}': {}", host_no_port, e)),
                        }
                    } else {
                        self.handshake_and_run_stream(stream, &host, &path).await
                    };

                    if let Err(e) = res {
                        warn!("Agent session ended with error: {}", e);
                    }
                }
                Ok(Err(e)) => {
                    warn!("Failed to connect to server at {}: {}", connect_addr, e);
                }
                Err(_) => {
                    warn!("Connection to server at {} timed out after {}s", connect_addr, connect_timeout.as_secs());
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

    /// Performs HTTP WebSocket upgrade handshake on an arbitrary AsyncRead + AsyncWrite stream and starts the agent session
    pub async fn handshake_and_run_stream<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
        &self,
        stream: S,
        host: &str,
        path: &str,
    ) -> Result<(), String> {
        let host_clean = host
            .strip_prefix("ws://")
            .or_else(|| host.strip_prefix("wss://"))
            .unwrap_or(host);
        let normalized_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{}", path)
        };
        let url_str = format!("ws://{}{}", host_clean, normalized_path);

        let handshake_timeout = Duration::from_secs(5);
        let (ws_stream, _response) = tokio::time::timeout(
            handshake_timeout,
            tokio_tungstenite::client_async(&url_str, stream),
        )
        .await
        .map_err(|_| format!("WebSocket handshake timed out after {}s", handshake_timeout.as_secs()))?
        .map_err(|e| format!("WebSocket client handshake failed: {}", e))?;

        self.run_with_ws_stream(ws_stream).await
    }

    /// Compatibility helper for running on an already-framed stream
    pub async fn run_with_stream<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
        &self,
        stream: S,
    ) -> Result<(), String> {
        let ws_stream = tokio_tungstenite::WebSocketStream::from_raw_socket(
            stream,
            tokio_tungstenite::tungstenite::protocol::Role::Client,
            None,
        )
        .await;
        self.run_with_ws_stream(ws_stream).await
    }

    /// Runs agent session over an established tokio-tungstenite WebSocket stream
    pub async fn run_with_ws_stream<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
        &self,
        ws_stream: tokio_tungstenite::WebSocketStream<S>,
    ) -> Result<(), String> {
        let (mut ws_sink, mut ws_reader) = ws_stream.split();

        // 1. Send Register frame
        let reg_msg = AgentToServerMessage::Register {
            info: self.terminal_info.clone(),
            auth_token: self.auth_token.clone(),
        };
        let reg_json = serde_json::to_string(&reg_msg).map_err(|e| e.to_string())?;
        ws_sink
            .send(tokio_tungstenite::tungstenite::Message::Text(reg_json))
            .await
            .map_err(|e| format!("Failed to send Register message: {}", e))?;

        // 2. Await RegisterAck
        let ack_msg_opt = tokio::time::timeout(Duration::from_secs(10), ws_reader.next())
            .await
            .map_err(|_| "Timed out waiting for RegisterAck from server".to_string())?;

        let ack_msg = match ack_msg_opt {
            Some(Ok(m)) => m,
            Some(Err(e)) => return Err(format!("Error reading RegisterAck: {}", e)),
            None => return Err("Stream closed before RegisterAck was received".to_string()),
        };

        let ack_text = match ack_msg {
            tokio_tungstenite::tungstenite::Message::Text(t) => t.to_string(),
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

        // 3. Setup Outgoing Channels (Pong + Control + Binary)
        let (pong_tx, mut pong_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<AgentToServerMessage>();
        let (binary_tx, mut binary_rx) = mpsc::channel::<Vec<u8>>(2);

        {
            let mut tx_guard = self.outbound_tx.write().await;
            *tx_guard = Some(outbound_tx.clone());
        }

        // 4. Spawn WebSocket Writer Task with Biased Priority Scheduling
        let write_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;

                    // Priority 0: RFC 6455 Pong control frames (must be sent immediately)
                    Some(pong_payload) = pong_rx.recv() => {
                        let ws_msg = tokio_tungstenite::tungstenite::Message::Pong(pong_payload);
                        if let Err(e) = ws_sink.send(ws_msg).await {
                            warn!("Failed to write pong WS message: {}", e);
                            break;
                        }
                    }

                    // Priority 1: Control & tool execution messages
                    Some(msg) = outbound_rx.recv() => {
                        match serde_json::to_string(&msg) {
                            Ok(json) => {
                                let ws_msg = tokio_tungstenite::tungstenite::Message::Text(json);
                                if let Err(e) = ws_sink.send(ws_msg).await {
                                    warn!("Failed to write control WS message: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("Failed to serialize outbound message: {}", e);
                            }
                        }
                    }

                    // Priority 2: Binary remote desktop frames
                    Some(frame_bytes) = binary_rx.recv() => {
                        let ws_msg = tokio_tungstenite::tungstenite::Message::Binary(frame_bytes);
                        if let Err(e) = ws_sink.send(ws_msg).await {
                            warn!("Failed to write binary desktop frame: {}", e);
                            break;
                        }
                    }

                    else => break,
                }
            }
        });

        // 5. Spawn Heartbeat Task
        let hb_tx = outbound_tx.clone();
        let term_id = self.terminal_info.terminal_id.clone();
        let hb_interval = self.heartbeat_interval_secs.load(Ordering::SeqCst);
        let hb_running = self.is_running.clone();

        let heartbeat_task = tokio::spawn(async move {
            let mut sys = sysinfo::System::new();
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
        let hb_interval = self.heartbeat_interval_secs.load(Ordering::SeqCst);
        let read_timeout = Duration::from_secs(hb_interval.max(5) * 4);

        while self.is_running.load(Ordering::SeqCst) {
            match tokio::time::timeout(read_timeout, ws_reader.next()).await {
                Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text)))) => {
                    match serde_json::from_str::<ServerToAgentMessage>(&text) {
                        Ok(server_msg) => {
                            self.handle_server_message(server_msg, &outbound_tx, &binary_tx).await;
                        }
                        Err(e) => {
                            warn!("Failed to parse server message: {} (raw: {})", e, text);
                        }
                    }
                }
                Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(payload)))) => {
                    debug!("Received WS ping from server ({} bytes); replying with Pong", payload.len());
                    let _ = pong_tx.send(payload.to_vec());
                }
                Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Pong(_)))) => {
                    debug!("Received WS pong from server");
                }
                Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_)))) => {
                    info!("Server requested WebSocket close");
                    break;
                }
                Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(_)))) => {
                    warn!("Received unexpected binary frame from server");
                }
                Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Frame(_)))) => {}
                Ok(Some(Err(e))) => {
                    debug!("WebSocket read error: {}", e);
                    session_error = Some(e.to_string());
                    break;
                }
                Ok(None) => {
                    info!("WebSocket stream closed by server");
                    break;
                }
                Err(_) => {
                    warn!(
                        "WebSocket read timed out (no message received for {}s). Server connection presumed dead.",
                        read_timeout.as_secs()
                    );
                    session_error = Some("Connection timed out (no heartbeat from server)".to_string());
                    break;
                }
            }
        }

        // Cleanup
        self.stream_controller.stop();
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
        binary_tx: &mpsc::Sender<Vec<u8>>,
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
                    let exec_fut = executor.execute_with_call_id(&cid, &tname, arguments);

                    let (success, result, error) = match tokio::time::timeout(Duration::from_secs(timeout), exec_fut).await {
                        Ok(Ok(val)) => (true, val, None),
                        Ok(Err(err)) => (false, serde_json::json!({}), Some(err)),
                        Err(_) => {
                            executor.cancel(&cid).await;
                            (
                                false,
                                serde_json::json!({}),
                                Some(format!("Tool '{}' timed out after {}s", tname, timeout)),
                            )
                        }
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
                self.executor.cancel(&call_id).await;
            }
            ServerToAgentMessage::HeartbeatAck { server_timestamp } => {
                debug!("Heartbeat acknowledged by server at timestamp: {}", server_timestamp);
            }
            ServerToAgentMessage::RegisterAck { .. } => {
                debug!("Received subsequent RegisterAck");
            }
            ServerToAgentMessage::StartDesktopStream {
                display_index,
                fps,
                quality,
                ..
            } => {
                info!(
                    "Server requested start desktop stream (display: {}, fps: {}, quality: {})",
                    display_index, fps, quality
                );
                self.stream_controller
                    .start_binary(display_index, fps, quality, binary_tx.clone());
            }
            ServerToAgentMessage::StopDesktopStream => {
                info!("Server requested stop desktop stream");
                self.stream_controller.stop();
            }
            ServerToAgentMessage::DesktopInput { event } => {
                if let Err(e) = self.input_tx.send(event) {
                    tracing::warn!("Failed to queue remote desktop input event: {}", e);
                }
            }
        }
    }
}
