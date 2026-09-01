use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::{HeartbeatMetrics, TerminalInfo, TerminalStatus};
use at_pc_server::config::ServerConfig;
use at_pc_server::ws::codec::{compute_accept_key, WsMessage, WsReader, WsWriter};
use at_pc_server::ws::handler::{handle_stream, WsServerState};
use at_pc_server::ws::registry::TerminalRegistry;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Test helper to perform client-side WebSocket handshake over any duplex stream
async fn client_ws_handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    host: &str,
    path: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let key = "dGhlIHNhbXBsZSBub25jZQ==";
    let request = format!(
        "GET {} HTTP/1.1\r\n\
        Host: {}\r\n\
        Upgrade: websocket\r\n\
        Connection: Upgrade\r\n\
        Sec-WebSocket-Key: {}\r\n\
        Sec-WebSocket-Version: 13\r\n\r\n",
        path, host, key
    );

    stream.write_all(request.as_bytes()).await?;

    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).await?;
    let resp = String::from_utf8_lossy(&buf[..n]);

    if !resp.starts_with("HTTP/1.1 101 Switching Protocols") {
        return Err(format!("Handshake rejected: {}", resp).into());
    }

    let expected_accept = compute_accept_key(key);
    if !resp.contains(&expected_accept) {
        return Err(format!("Invalid Sec-WebSocket-Accept in response: {}", resp).into());
    }

    Ok(())
}

#[tokio::test]
async fn test_ws_gateway_handshake_and_registration() {
    let registry = Arc::new(TerminalRegistry::new());
    let config = ServerConfig {
        ws_path: "/ws".to_string(),
        auth_token: Some("secret_token_123".to_string()),
        heartbeat_interval_secs: 5,
        ..Default::default()
    };
    let state = WsServerState::new(registry.clone(), config);

    // In-memory duplex connection
    let (mut client_stream, server_stream) = tokio::io::duplex(8192);

    let server_task = tokio::spawn(async move {
        handle_stream(server_stream, state).await;
    });

    client_ws_handshake(&mut client_stream, "127.0.0.1", "/ws").await.unwrap();

    let (reader, writer) = tokio::io::split(client_stream);
    let mut client_reader = WsReader::new(reader);
    let mut client_writer = WsWriter::new(writer);

    // 1. Send Register with correct auth token
    let info = TerminalInfo {
        terminal_id: "agent-pc-01".to_string(),
        hostname: "DESKTOP-ALPHA".to_string(),
        username: "user".to_string(),
        lan_ip: "192.168.1.10".to_string(),
        os_version: "Windows 11".to_string(),
        agent_version: "0.3.0".to_string(),
    };

    let reg_msg = AgentToServerMessage::Register {
        info: info.clone(),
        auth_token: Some("secret_token_123".to_string()),
    };
    client_writer
        .write_message(&WsMessage::Text(serde_json::to_string(&reg_msg).unwrap()))
        .await
        .unwrap();

    // 2. Expect RegisterAck
    let ack_msg = client_reader.read_message().await.unwrap();
    if let WsMessage::Text(text) = ack_msg {
        let ack: ServerToAgentMessage = serde_json::from_str(&text).unwrap();
        match ack {
            ServerToAgentMessage::RegisterAck { success, heartbeat_interval_secs, .. } => {
                assert!(success);
                assert_eq!(heartbeat_interval_secs, 5);
            }
            _ => panic!("Expected RegisterAck, got {:?}", ack),
        }
    } else {
        panic!("Expected Text message");
    }

    // 3. Verify in registry
    assert_eq!(registry.get_status("agent-pc-01").await, Some(TerminalStatus::Online));

    // 4. Send Heartbeat
    let metrics = HeartbeatMetrics {
        cpu_usage_percent: 15.0,
        memory_used_mb: 4096,
        memory_total_mb: 16384,
        uptime_secs: 7200,
        timestamp: 1725181000,
    };
    let hb_msg = AgentToServerMessage::Heartbeat {
        terminal_id: "agent-pc-01".to_string(),
        metrics,
    };
    client_writer
        .write_message(&WsMessage::Text(serde_json::to_string(&hb_msg).unwrap()))
        .await
        .unwrap();

    // 5. Expect HeartbeatAck
    let hb_ack_msg = client_reader.read_message().await.unwrap();
    if let WsMessage::Text(text) = hb_ack_msg {
        let ack: ServerToAgentMessage = serde_json::from_str(&text).unwrap();
        match ack {
            ServerToAgentMessage::HeartbeatAck { server_timestamp } => {
                assert!(server_timestamp > 0);
            }
            _ => panic!("Expected HeartbeatAck, got {:?}", ack),
        }
    }

    // 6. Send Disconnect
    let disc_msg = AgentToServerMessage::Disconnect {
        terminal_id: "agent-pc-01".to_string(),
        reason: "User closed agent app".to_string(),
    };
    client_writer
        .write_message(&WsMessage::Text(serde_json::to_string(&disc_msg).unwrap()))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(registry.get_status("agent-pc-01").await, Some(TerminalStatus::Offline));

    server_task.abort();
}

#[tokio::test]
async fn test_ws_gateway_auth_failure() {
    let registry = Arc::new(TerminalRegistry::new());
    let config = ServerConfig {
        ws_path: "/ws".to_string(),
        auth_token: Some("correct_secret".to_string()),
        ..Default::default()
    };
    let state = WsServerState::new(registry.clone(), config);

    let (mut client_stream, server_stream) = tokio::io::duplex(8192);

    tokio::spawn(async move {
        handle_stream(server_stream, state).await;
    });

    client_ws_handshake(&mut client_stream, "127.0.0.1", "/ws").await.unwrap();

    let (reader, writer) = tokio::io::split(client_stream);
    let mut client_reader = WsReader::new(reader);
    let mut client_writer = WsWriter::new(writer);

    let info = TerminalInfo {
        terminal_id: "bad-agent".to_string(),
        hostname: "EVIL".to_string(),
        username: "hacker".to_string(),
        lan_ip: "10.0.0.99".to_string(),
        os_version: "Linux".to_string(),
        agent_version: "0.3.0".to_string(),
    };

    let reg_msg = AgentToServerMessage::Register {
        info,
        auth_token: Some("wrong_secret".to_string()),
    };
    client_writer
        .write_message(&WsMessage::Text(serde_json::to_string(&reg_msg).unwrap()))
        .await
        .unwrap();

    let ack_msg = client_reader.read_message().await.unwrap();
    if let WsMessage::Text(text) = ack_msg {
        let ack: ServerToAgentMessage = serde_json::from_str(&text).unwrap();
        match ack {
            ServerToAgentMessage::RegisterAck { success, message, .. } => {
                assert!(!success);
                assert!(message.unwrap().contains("Invalid authentication token"));
            }
            _ => panic!("Expected failed RegisterAck"),
        }
    }
}

#[tokio::test]
async fn test_ws_gateway_tool_result_dispatch() {
    let registry = Arc::new(TerminalRegistry::new());
    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = call_count.clone();

    let handler = Arc::new(move |msg: AgentToServerMessage| {
        if let AgentToServerMessage::ToolResult { call_id, success, .. } = msg {
            if call_id == "call-abc" && success {
                call_count_clone.fetch_add(1, Ordering::SeqCst);
            }
        }
    });

    let config = ServerConfig {
        ws_path: "/ws".to_string(),
        ..Default::default()
    };
    let state = WsServerState::with_handler(registry.clone(), config, handler);

    let (mut client_stream, server_stream) = tokio::io::duplex(8192);

    tokio::spawn(async move {
        handle_stream(server_stream, state).await;
    });

    client_ws_handshake(&mut client_stream, "127.0.0.1", "/ws").await.unwrap();

    let (reader, writer) = tokio::io::split(client_stream);
    let mut client_reader = WsReader::new(reader);
    let mut client_writer = WsWriter::new(writer);

    let info = TerminalInfo {
        terminal_id: "worker-node".to_string(),
        hostname: "WORKER-1".to_string(),
        username: "runner".to_string(),
        lan_ip: "10.0.0.5".to_string(),
        os_version: "Windows 10".to_string(),
        agent_version: "0.3.0".to_string(),
    };

    let reg = AgentToServerMessage::Register { info, auth_token: None };
    client_writer
        .write_message(&WsMessage::Text(serde_json::to_string(&reg).unwrap()))
        .await
        .unwrap();

    let _ = client_reader.read_message().await.unwrap(); // RegisterAck

    // Send ToolResult
    let result_msg = AgentToServerMessage::ToolResult {
        call_id: "call-abc".to_string(),
        success: true,
        result: serde_json::json!({ "stdout": "test output" }),
        error: None,
        duration_ms: 120,
    };
    client_writer
        .write_message(&WsMessage::Text(serde_json::to_string(&result_msg).unwrap()))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}
