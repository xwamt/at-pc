use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::config::ServerConfig;
use crate::keystore::KeyStore;
use crate::rate_limit::{AuthRateLimiter, RateLimitDecision};
use crate::ws::registry::TerminalRegistry;
use at_pc_protocol::messages::{
    enrollment_proof_transcript, AgentToServerMessage, BinaryDesktopFrame, EnrollmentClientMessage,
    EnrollmentHello, EnrollmentServerMessage, ServerToAgentMessage,
};
use at_pc_protocol::models::{TerminalInfo, TerminalStatus};

/// Trait for handling tool results and agent messages
pub trait AgentMessageHandler: Send + Sync {
    fn handle_tool_result(&self, msg: AgentToServerMessage);
    fn handle_terminal_disconnected(&self, _terminal_id: &str, _reason: &str) {}
    fn handle_desktop_frame_binary(&self, _terminal_id: &str, _frame: BinaryDesktopFrame) {}
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
    pub keystore: Option<Arc<KeyStore>>,
    pub rate_limiter: Arc<AuthRateLimiter>,
    /// Records temporary legacy-registration risk events until M2 enrollment
    /// replaces unauthenticated WebSocket registration.
    pub audit_logger: Option<Arc<crate::audit::AuditLogger>>,
    /// Pending-only enrollment records. This store never receives a terminal
    /// sender and is intentionally separate from TerminalRegistry.
    pub enrollment_store: Arc<crate::enrollment_store::DeviceEnrollmentStore>,
}

impl WsServerState {
    pub fn new(registry: Arc<TerminalRegistry>, config: ServerConfig) -> Self {
        Self {
            registry,
            config,
            message_handler: None,
            keystore: None,
            rate_limiter: Arc::new(AuthRateLimiter::default()),
            audit_logger: None,
            enrollment_store: Arc::new(crate::enrollment_store::DeviceEnrollmentStore::in_memory()),
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
            keystore: None,
            rate_limiter: Arc::new(AuthRateLimiter::default()),
            audit_logger: None,
            enrollment_store: Arc::new(crate::enrollment_store::DeviceEnrollmentStore::in_memory()),
        }
    }

    pub fn with_keystore(mut self, keystore: Arc<KeyStore>) -> Self {
        self.keystore = Some(keystore);
        self
    }

    pub fn with_rate_limiter(mut self, rate_limiter: Arc<AuthRateLimiter>) -> Self {
        self.rate_limiter = rate_limiter;
        self
    }

    pub fn with_audit_logger(mut self, audit_logger: Arc<crate::audit::AuditLogger>) -> Self {
        self.audit_logger = Some(audit_logger);
        self
    }

    pub fn with_enrollment_store(
        mut self,
        enrollment_store: Arc<crate::enrollment_store::DeviceEnrollmentStore>,
    ) -> Self {
        self.enrollment_store = enrollment_store;
        self
    }
}

/// Handle a full connected TCP stream upgraded to WebSocket using standard production-grade WebSocket stack
pub async fn handle_connection(stream: TcpStream, state: WsServerState) {
    let peer_addr = stream.peer_addr().ok();
    handle_stream_with_peer(stream, state, peer_addr).await;
}

/// Handle any connected async stream upgraded to WebSocket using tokio-tungstenite
pub async fn handle_stream<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    stream: S,
    state: WsServerState,
) {
    handle_stream_with_peer(stream, state, None).await;
}

/// Handle any connected async stream with known peer address upgraded to WebSocket using tokio-tungstenite
pub async fn handle_stream_with_peer<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    stream: S,
    state: WsServerState,
    peer_addr: Option<SocketAddr>,
) {
    if let Some(addr) = peer_addr {
        if matches!(
            state.rate_limiter.check(addr.ip()),
            RateLimitDecision::Banned { .. }
        ) {
            warn!(
                "Rejected WebSocket connection from banned IP: {}",
                addr.ip()
            );
            return;
        }
    }

    let ws_path = state.config.ws_path.clone();
    let enrollment_path = state.config.enrollment_path.clone();
    let accepted_path = Arc::new(std::sync::Mutex::new(None::<String>));
    let callback_path = Arc::clone(&accepted_path);
    #[allow(clippy::result_large_err)]
    let callback =
        move |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
              resp: tokio_tungstenite::tungstenite::handshake::server::Response| {
            let path = req.uri().path();
            if path == "/health" {
                let res = tokio_tungstenite::tungstenite::http::Response::builder()
                    .status(200)
                    .body(Some("OK".to_string()))
                    .unwrap();
                return Err(res);
            }
            if path != ws_path && path != enrollment_path {
                let res = tokio_tungstenite::tungstenite::http::Response::builder()
                    .status(404)
                    .body(Some("Not Found".to_string()))
                    .unwrap();
                return Err(res);
            }
            if let Ok(mut accepted) = callback_path.lock() {
                *accepted = Some(path.to_string());
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

    let requested_path = accepted_path
        .lock()
        .ok()
        .and_then(|path| path.clone())
        .unwrap_or_default();
    if requested_path == state.config.enrollment_path {
        handle_enrollment_session(ws_connection, state, peer_addr).await;
        return;
    }

    let (mut ws_sink, mut ws_reader) = ws_connection.split();

    // 1. Initial Handshake: Wait for Register message
    let (terminal_info, initial_sender_tx, mut initial_sender_rx, session_id) =
        match wait_for_registration(&mut ws_reader, &mut ws_sink, &state, peer_addr).await {
            Ok(res) => res,
            Err(err_msg) => {
                warn!("WebSocket registration handshake failed: {}", err_msg);
                let _ = ws_sink
                    .send(tokio_tungstenite::tungstenite::Message::Close(None))
                    .await;
                return;
            }
        };

    let terminal_id = terminal_info.terminal_id.clone();
    info!(
        "Terminal [{}] ({}) successfully registered (session: {}) from IP: {}",
        terminal_id, terminal_info.hostname, session_id, terminal_info.lan_ip
    );

    // 2. Outgoing message forwarding task (MPSC channel -> WebSocket Sink)
    let (control_tx, mut control_rx) =
        mpsc::unbounded_channel::<tokio_tungstenite::tungstenite::Message>();
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
                    let is_coalesceable_mouse = |m: &ServerToAgentMessage| matches!(
                        m,
                        ServerToAgentMessage::DesktopInput {
                            event: at_pc_protocol::models::DesktopInputEvent::MouseMove { .. }
                                | at_pc_protocol::models::DesktopInputEvent::MouseMovePixel { .. }
                        }
                    );
                    if is_coalesceable_mouse(&final_msg) {
                        while let Ok(next) = initial_sender_rx.try_recv() {
                            if is_coalesceable_mouse(&next) {
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
        warn!(
            "WebSocket forwarding task for [{}] exited.",
            term_id_for_send
        );
    });

    // 3. Incoming message loop (WebSocket Reader -> Registry & MessageHandler)
    let idle_timeout =
        std::time::Duration::from_secs(state.config.offline_threshold_secs.max(15) * 2);
    loop {
        let msg_res = match tokio::time::timeout(idle_timeout, ws_reader.next()).await {
            Ok(Some(res)) => res,
            Ok(None) => break,
            Err(_) => {
                warn!(
                    "WebSocket read timed out for terminal [{}] (no message for {}s)",
                    terminal_id,
                    idle_timeout.as_secs()
                );
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
                            warn!(
                                "Failed to decode binary desktop frame from [{}]: {}",
                                terminal_id, e
                            );
                        }
                    }
                } else {
                    warn!(
                        "Received unexpected binary frame ({} bytes) from terminal [{}]",
                        bin.len(),
                        terminal_id
                    );
                }
            }
            Ok(tokio_tungstenite::tungstenite::Message::Ping(payload)) => {
                debug!(
                    "Received ping from terminal [{}] ({} bytes); replying with Pong",
                    terminal_id,
                    payload.len()
                );
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
    info!(
        "Terminal [{}] disconnected. Cleaning up session {}.",
        terminal_id, session_id
    );
    state
        .registry
        .set_status_if_current(&terminal_id, session_id, TerminalStatus::Offline)
        .await;
    if let Some(ref handler) = state.message_handler {
        handler.handle_terminal_disconnected(&terminal_id, "WebSocket connection closed");
    }
    forward_task.abort();
}

async fn handle_enrollment_session<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    ws_connection: tokio_tungstenite::WebSocketStream<S>,
    state: WsServerState,
    peer_addr: Option<SocketAddr>,
) {
    const HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
    const MAX_ENROLLMENT_MESSAGE_BYTES: usize = 16 * 1024;

    if !crate::config::is_loopback(&state.config.listen_host) && !state.config.is_tls_enabled() {
        let (mut sink, _) = ws_connection.split();
        let _ = send_enrollment_error(
            &mut sink,
            "tls_required",
            "Enrollment requires WSS on a non-loopback listener",
            None,
        )
        .await;
        return;
    }

    let (mut sink, mut reader) = ws_connection.split();
    let first = match tokio::time::timeout(HANDSHAKE_TIMEOUT, reader.next()).await {
        Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))))
            if text.len() <= MAX_ENROLLMENT_MESSAGE_BYTES => text,
        Ok(Some(Ok(_))) => {
            let _ = send_enrollment_error(
                &mut sink,
                "invalid_request",
                "Enrollment requires a bounded text Hello message",
                None,
            )
            .await;
            return;
        }
        Ok(Some(Err(error))) => {
            debug!(%error, "Enrollment WebSocket read failed");
            return;
        }
        Ok(None) => return,
        Err(_) => {
            let _ = send_enrollment_error(
                &mut sink,
                "invalid_request",
                "Timed out waiting for enrollment Hello",
                None,
            )
            .await;
            return;
        }
    };

    let hello = match serde_json::from_str::<EnrollmentClientMessage>(&first) {
        Ok(EnrollmentClientMessage::Hello(hello)) => hello,
        _ => {
            let _ = send_enrollment_error(
                &mut sink,
                "invalid_request",
                "First enrollment message must be Hello",
                None,
            )
            .await;
            return;
        }
    };
    if let Err(code) = validate_enrollment_hello(&hello) {
        let _ = send_enrollment_error(&mut sink, code, "Enrollment Hello is invalid", None).await;
        return;
    }

    let source_ip = peer_addr.map(|address| address.ip().to_string());
    let challenge = match state
        .enrollment_store
        .issue_challenge(hello, source_ip.clone())
        .await
    {
        Ok(challenge) => challenge,
        Err(error) => {
            let code = match error {
                crate::enrollment_store::EnrollmentStoreError::CapacityExceeded => {
                    "capacity_exceeded"
                }
                crate::enrollment_store::EnrollmentStoreError::IdentityConflict => "identity_conflict",
                _ => "internal_error",
            };
            audit_enrollment(
                &state,
                crate::audit::ACTION_ENROLLMENT_REJECTED,
                None,
                "DENIED",
                Some(error.to_string()),
                serde_json::json!({ "reason_code": code }),
                source_ip,
            );
            let _ = send_enrollment_error(
                &mut sink,
                code,
                "Enrollment request cannot be accepted",
                Some(60),
            )
            .await;
            return;
        }
    };

    if send_enrollment_message(
        &mut sink,
        EnrollmentServerMessage::Challenge {
            enrollment_id: challenge.enrollment_id.clone(),
            server_nonce: challenge.server_nonce.clone(),
            expires_at: challenge.expires_at,
        },
    )
    .await
    .is_err()
    {
        return;
    }

    let proof = match tokio::time::timeout(HANDSHAKE_TIMEOUT, reader.next()).await {
        Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))))
            if text.len() <= MAX_ENROLLMENT_MESSAGE_BYTES => {
                serde_json::from_str::<EnrollmentClientMessage>(&text).ok()
            }
        _ => None,
    };
    let Some(EnrollmentClientMessage::Proof {
        enrollment_id,
        signature,
    }) = proof
    else {
        let _ = send_enrollment_error(
            &mut sink,
            "invalid_request",
            "Enrollment requires a bounded Proof message",
            None,
        )
        .await;
        return;
    };
    if enrollment_id != challenge.enrollment_id || signature.len() > 256 {
        let _ = send_enrollment_error(
            &mut sink,
            "invalid_request",
            "Enrollment proof does not match its challenge",
            None,
        )
        .await;
        return;
    }

    let challenge = match state.enrollment_store.consume_challenge(&enrollment_id).await {
        Ok(challenge) => challenge,
        Err(_error) => {
            let _ = send_enrollment_error(
                &mut sink,
                "proof_invalid",
                "Enrollment challenge is unavailable",
                None,
            )
            .await;
            return;
        }
    };
    if !verify_enrollment_proof(&challenge.hello, &challenge.enrollment_id, &challenge.server_nonce, &signature) {
        audit_enrollment(
            &state,
            crate::audit::ACTION_ENROLLMENT_REJECTED,
            Some(challenge.enrollment_id),
            "DENIED",
            Some("invalid enrollment proof".to_string()),
            serde_json::json!({ "reason_code": "proof_invalid" }),
            challenge.source_ip,
        );
        let _ = send_enrollment_error(
            &mut sink,
            "proof_invalid",
            "Enrollment proof is invalid",
            None,
        )
        .await;
        return;
    }

    let upsert = match state
        .enrollment_store
        .create_or_refresh_pending(challenge.hello, challenge.source_ip.clone())
        .await
    {
        Ok(upsert) => upsert,
        Err(_error) => {
            let _ = send_enrollment_error(
                &mut sink,
                "identity_conflict",
                "Enrollment identity cannot be stored",
                Some(60),
            )
            .await;
            return;
        }
    };
    let action = if upsert.created {
        crate::audit::ACTION_ENROLLMENT_REQUESTED
    } else {
        crate::audit::ACTION_ENROLLMENT_REFRESHED
    };
    audit_enrollment(
        &state,
        action,
        Some(upsert.summary.enrollment_id.clone()),
        "PENDING",
        None,
        serde_json::json!({
            "protocol_version": 1,
            "capability_count": upsert.summary.capabilities.len(),
            "device_fingerprint": upsert.summary.device_fingerprint,
            "created": upsert.created,
        }),
        challenge.source_ip,
    );
    let _ = send_enrollment_message(
        &mut sink,
        EnrollmentServerMessage::Status {
            enrollment_id: upsert.summary.enrollment_id,
            status: at_pc_protocol::models::EnrollmentStatus::Pending,
            approval_phrase: enrollment_phrase(&upsert.summary.device_fingerprint),
            retry_after_secs: 60,
            reason_code: None,
        },
    )
    .await;
}

fn validate_enrollment_hello(hello: &EnrollmentHello) -> Result<(), &'static str> {
    if hello.protocol_version != 1 {
        return Err("invalid_version");
    }
    if hello.stable_device_id.len() < 8
        || hello.stable_device_id.len() > 128
        || hello.public_key.len() != 43
        || hello.client_nonce.len() < 16
        || hello.client_nonce.len() > 128
        || hello.facts.hostname.len() > 128
        || hello.facts.os_version.len() > 256
        || hello.facts.agent_version.len() > 64
        || hello.facts.lan_ip.as_ref().is_some_and(|ip| ip.len() > 64)
        || hello.capabilities.len() > 32
        || hello.capabilities.iter().any(|capability| capability.is_empty() || capability.len() > 64)
    {
        return Err("invalid_request");
    }
    Ok(())
}

fn verify_enrollment_proof(
    hello: &EnrollmentHello,
    enrollment_id: &str,
    server_nonce: &str,
    signature: &str,
) -> bool {
    let public_key = match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&hello.public_key) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    let key_bytes: [u8; 32] = match public_key.as_slice().try_into() {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    let verifying_key = match VerifyingKey::from_bytes(&key_bytes) {
        Ok(key) => key,
        Err(_) => return false,
    };
    let signature = match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(signature) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    let signature = match Signature::from_slice(&signature) {
        Ok(signature) => signature,
        Err(_) => return false,
    };
    let transcript = match enrollment_proof_transcript(hello, enrollment_id, server_nonce) {
        Ok(transcript) => transcript,
        Err(_) => return false,
    };
    verifying_key.verify(&transcript, &signature).is_ok()
}

fn enrollment_phrase(fingerprint: &str) -> String {
    let first = fingerprint.get(0..4).unwrap_or("0000");
    let second = fingerprint.get(4..8).unwrap_or("0000");
    format!("pending-{first}-{second}")
}

fn audit_enrollment(
    state: &WsServerState,
    action: &str,
    enrollment_id: Option<String>,
    status: &str,
    error: Option<String>,
    arguments: serde_json::Value,
    source_ip: Option<String>,
) {
    let Some(logger) = &state.audit_logger else {
        return;
    };
    logger.log(crate::audit::AuditRecord {
        id: format!("audit-{}", crate::router::generate_call_id()),
        timestamp: chrono::Utc::now().to_rfc3339(),
        role: None,
        token_prefix: None,
        client_ip: source_ip,
        terminal_id: enrollment_id,
        action: action.to_string(),
        tool_name: None,
        arguments: Some(arguments),
        status: status.to_string(),
        error,
        duration_ms: Some(0),
    });
}

async fn send_enrollment_message<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<S>,
        tokio_tungstenite::tungstenite::Message,
    >,
    message: EnrollmentServerMessage,
) -> Result<(), String> {
    let text = serde_json::to_string(&message).map_err(|error| error.to_string())?;
    sink.send(tokio_tungstenite::tungstenite::Message::Text(text))
        .await
        .map_err(|error| error.to_string())
}

async fn send_enrollment_error<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<S>,
        tokio_tungstenite::tungstenite::Message,
    >,
    code: &str,
    message: &str,
    retry_after_secs: Option<u64>,
) -> Result<(), String> {
    send_enrollment_message(
        sink,
        EnrollmentServerMessage::Error {
            code: code.to_string(),
            message: message.to_string(),
            retry_after_secs,
        },
    )
    .await
}

async fn send_register_reject<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<S>,
        tokio_tungstenite::tungstenite::Message,
    >,
    message: &str,
    heartbeat_interval_secs: u64,
) {
    let ack = ServerToAgentMessage::RegisterAck {
        success: false,
        message: Some(message.to_string()),
        heartbeat_interval_secs,
    };
    if let Ok(json) = serde_json::to_string(&ack) {
        let _ = sink
            .send(tokio_tungstenite::tungstenite::Message::Text(json))
            .await;
    }
}

/// Wait for the first message to be a valid Register payload
async fn wait_for_registration<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    reader: &mut futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<S>>,
    sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<S>,
        tokio_tungstenite::tungstenite::Message,
    >,
    state: &WsServerState,
    peer_addr: Option<SocketAddr>,
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

    if let Some(addr) = peer_addr {
        if matches!(
            state.rate_limiter.check(addr.ip()),
            RateLimitDecision::Banned { .. } | RateLimitDecision::RateLimited { .. }
        ) {
            send_register_reject(
                sink,
                "Rate limit exceeded",
                state.config.heartbeat_interval_secs,
            )
            .await;
            return Err("Rate limit exceeded".to_string());
        }
    }

    match msg {
        // M0 intentionally preserves the legacy acceptance behavior so existing
        // deployments remain reachable while M1/M2 introduce Pending Enrollment
        // and mTLS. This branch is a risk baseline, not an authorization path.
        AgentToServerMessage::Register { info, auth_token } => {
            if let Some(logger) = &state.audit_logger {
                logger.log(crate::audit::AuditRecord {
                    id: format!("audit-{}", crate::router::generate_call_id()),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    role: None,
                    token_prefix: auth_token
                        .as_deref()
                        .map(crate::audit::AuditLogger::redact_token),
                    client_ip: peer_addr.map(|addr| addr.to_string()),
                    terminal_id: Some(info.terminal_id.clone()),
                    action: crate::audit::ACTION_WS_UNAUTHENTICATED_REGISTRATION_BASELINE
                        .to_string(),
                    tool_name: None,
                    arguments: Some(serde_json::json!({
                        "credential_present": auth_token.is_some(),
                        "auth_enforced": false,
                        "baseline": "temporary_until_m2",
                    })),
                    status: "WARN".to_string(),
                    error: Some(
                        "legacy WebSocket registration accepted without credential validation"
                            .to_string(),
                    ),
                    duration_ms: Some(0),
                });
            }
            warn!(
                terminal_id = %info.terminal_id,
                credential_present = auth_token.is_some(),
                "Legacy WebSocket registration accepted without credential validation; M2 enrollment is pending"
            );
            if let Some(addr) = peer_addr {
                state.rate_limiter.record_success(addr.ip());
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
            sink.send(tokio_tungstenite::tungstenite::Message::Text(json))
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
                debug!(
                    "Received ToolResult from [{}], but no handler is configured",
                    terminal_id
                );
            }
        }
        AgentToServerMessage::Disconnect {
            terminal_id: did,
            reason,
        } => {
            let target_id = if did.is_empty() { terminal_id } else { &did };
            info!("Terminal [{}] initiated disconnect: {}", target_id, reason);
            state
                .registry
                .set_status(target_id, TerminalStatus::Offline)
                .await;
            if let Some(ref handler) = state.message_handler {
                handler.handle_terminal_disconnected(target_id, &reason);
            }
        }
        AgentToServerMessage::Register { info, .. } => {
            debug!("Re-registering terminal [{}]", info.terminal_id);
            state.registry.register(info, tx.clone()).await;
        }
    }
}
