use at_pc_protocol::messages::{AgentToServerMessage, BinaryDesktopFrame, ServerToAgentMessage};
use at_pc_protocol::models::{HeartbeatMetrics, TerminalInfo, TerminalStatus};
use at_pc_server::config::ServerConfig;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::handler::{handle_stream, WsServerState};
use at_pc_server::ws::registry::TerminalRegistry;
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

async fn connect_client(
    state: WsServerState,
) -> tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream> {
    let (client_stream, server_stream) = tokio::io::duplex(8192);
    tokio::spawn(async move { handle_stream(server_stream, state).await });
    let (client_ws, response) = tokio_tungstenite::client_async("ws://127.0.0.1/ws", client_stream)
        .await
        .expect("Client handshake failed");
    assert_eq!(response.status(), 101);
    client_ws
}

async fn send_json(
    client_ws: &mut tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
    message: &AgentToServerMessage,
) {
    client_ws
        .send(Message::Text(serde_json::to_string(message).unwrap()))
        .await
        .unwrap();
}

async fn receive_server_message(
    client_ws: &mut tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
) -> ServerToAgentMessage {
    match client_ws.next().await.unwrap().unwrap() {
        Message::Text(text) => serde_json::from_str(&text).unwrap(),
        other => panic!("Expected Text message, got {other:?}"),
    }
}

fn terminal_info(id: &str) -> TerminalInfo {
    TerminalInfo {
        terminal_id: id.to_string(),
        hostname: "TEST-HOST".to_string(),
        username: "user".to_string(),
        lan_ip: "192.168.1.10".to_string(),
        os_version: "Windows 11".to_string(),
        agent_version: "0.3.0".to_string(),
    }
}

#[tokio::test]
async fn test_ws_gateway_handshake_and_registration() {
    let registry = Arc::new(TerminalRegistry::new());
    let state = WsServerState::new(
        registry.clone(),
        ServerConfig {
            ws_path: "/ws".to_string(),
            auth_token: Some("secret_token_123".to_string()),
            heartbeat_interval_secs: 5,
            ..Default::default()
        },
    );
    let mut client_ws = connect_client(state).await;

    send_json(
        &mut client_ws,
        &AgentToServerMessage::Register {
            info: terminal_info("agent-pc-01"),
            auth_token: Some("secret_token_123".to_string()),
        },
    )
    .await;
    match receive_server_message(&mut client_ws).await {
        ServerToAgentMessage::RegisterAck {
            success,
            heartbeat_interval_secs,
            ..
        } => {
            assert!(success);
            assert_eq!(heartbeat_interval_secs, 5);
        }
        other => panic!("Expected RegisterAck, got {other:?}"),
    }
    assert_eq!(
        registry.get_status("agent-pc-01").await,
        Some(TerminalStatus::Online)
    );

    send_json(
        &mut client_ws,
        &AgentToServerMessage::Heartbeat {
            terminal_id: "agent-pc-01".to_string(),
            metrics: HeartbeatMetrics {
                cpu_usage_percent: 15.0,
                memory_used_mb: 4096,
                memory_total_mb: 16384,
                uptime_secs: 7200,
                timestamp: 1725181000,
            },
        },
    )
    .await;
    match receive_server_message(&mut client_ws).await {
        ServerToAgentMessage::HeartbeatAck { server_timestamp } => {
            assert!(server_timestamp > 0)
        }
        other => panic!("Expected HeartbeatAck, got {other:?}"),
    }

    send_json(
        &mut client_ws,
        &AgentToServerMessage::Disconnect {
            terminal_id: "agent-pc-01".to_string(),
            reason: "User closed agent app".to_string(),
        },
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        registry.get_status("agent-pc-01").await,
        Some(TerminalStatus::Offline)
    );
}

#[tokio::test]
async fn test_ws_gateway_auth_failure() {
    let registry = Arc::new(TerminalRegistry::new());
    let state = WsServerState::new(
        registry,
        ServerConfig {
            ws_path: "/ws".to_string(),
            auth_token: Some("correct_secret".to_string()),
            ..Default::default()
        },
    );
    let mut client_ws = connect_client(state).await;

    send_json(
        &mut client_ws,
        &AgentToServerMessage::Register {
            info: terminal_info("bad-agent"),
            auth_token: Some("wrong_secret".to_string()),
        },
    )
    .await;
    match receive_server_message(&mut client_ws).await {
        ServerToAgentMessage::RegisterAck {
            success, message, ..
        } => {
            assert!(!success);
            assert!(message.unwrap().contains("Invalid authentication token"));
        }
        other => panic!("Expected failed RegisterAck, got {other:?}"),
    }
}

#[tokio::test]
async fn test_ws_gateway_tool_result_dispatch() {
    let registry = Arc::new(TerminalRegistry::new());
    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = call_count.clone();
    let handler = Arc::new(move |msg: AgentToServerMessage| {
        if let AgentToServerMessage::ToolResult {
            call_id, success, ..
        } = msg
        {
            if call_id == "call-abc" && success {
                call_count_clone.fetch_add(1, Ordering::SeqCst);
            }
        }
    });
    let state = WsServerState::with_handler(
        registry,
        ServerConfig {
            ws_path: "/ws".to_string(),
            ..Default::default()
        },
        handler,
    );
    let mut client_ws = connect_client(state).await;

    send_json(
        &mut client_ws,
        &AgentToServerMessage::Register {
            info: terminal_info("worker-node"),
            auth_token: None,
        },
    )
    .await;
    assert!(matches!(
        receive_server_message(&mut client_ws).await,
        ServerToAgentMessage::RegisterAck { success: true, .. }
    ));

    send_json(
        &mut client_ws,
        &AgentToServerMessage::ToolResult {
            call_id: "call-abc".to_string(),
            success: true,
            result: serde_json::json!({ "stdout": "test output" }),
            error: None,
            duration_ms: 120,
        },
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_ws_gateway_binary_desktop_frame_reaches_router_cache() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let state = WsServerState::with_handler(
        registry,
        ServerConfig {
            ws_path: "/ws".to_string(),
            ..Default::default()
        },
        router.clone(),
    );
    let mut client_ws = connect_client(state).await;

    send_json(
        &mut client_ws,
        &AgentToServerMessage::Register {
            info: terminal_info("binary-stream-node"),
            auth_token: None,
        },
    )
    .await;
    assert!(matches!(
        receive_server_message(&mut client_ws).await,
        ServerToAgentMessage::RegisterAck { success: true, .. }
    ));

    let jpeg_bytes = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0xff, 0xd9];
    let frame = BinaryDesktopFrame::new(2, 2560, 1440, 1_725_180_000_123, jpeg_bytes.clone());
    client_ws
        .send(Message::Binary(frame.encode()))
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if let Some((display_index, width, height, timestamp, raw_bytes)) = router
                .get_latest_desktop_frame_raw("binary-stream-node")
                .await
            {
                assert_eq!(display_index, 2);
                assert_eq!(width, 2560);
                assert_eq!(height, 1440);
                assert_eq!(timestamp, 1_725_180_000_123);
                assert_eq!(raw_bytes, jpeg_bytes);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Binary desktop frame did not reach router cache");
}
