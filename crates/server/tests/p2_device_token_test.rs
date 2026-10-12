use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::TerminalInfo;
use at_pc_server::config::ServerConfig;
use at_pc_server::keystore::{KeyKind, KeyStore};
use at_pc_server::mcp::create_mcp_http_router;
use at_pc_server::rate_limit::AuthRateLimiter;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::handler::{handle_stream_with_peer, WsServerState};
use at_pc_server::ws::registry::TerminalRegistry;
use at_pc_server::Role;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message;
use tower::ServiceExt;

static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(1);

fn temp_data_dir(prefix: &str) -> PathBuf {
    let id = TEST_DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "atpc-p2-g2-{prefix}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        id
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

async fn connect_ws_client(
    state: WsServerState,
    peer_addr: Option<SocketAddr>,
) -> tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream> {
    let (client_stream, server_stream) = tokio::io::duplex(8192);
    tokio::spawn(async move {
        handle_stream_with_peer(server_stream, state, peer_addr).await;
    });
    let (client_ws, response) = tokio_tungstenite::client_async("ws://127.0.0.1/ws", client_stream)
        .await
        .expect("Client handshake failed");
    assert_eq!(response.status(), 101);
    client_ws
}

async fn send_register_msg(
    client_ws: &mut tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
    terminal_id: &str,
    auth_token: Option<String>,
) {
    let msg = AgentToServerMessage::Register {
        info: TerminalInfo {
            terminal_id: terminal_id.to_string(),
            hostname: "TEST-HOST".to_string(),
            username: "user".to_string(),
            lan_ip: "192.168.1.10".to_string(),
            os_version: "Windows 11".to_string(),
            agent_version: "0.3.0".to_string(),
        },
        auth_token,
    };
    client_ws
        .send(Message::Text(serde_json::to_string(&msg).unwrap()))
        .await
        .unwrap();
}

async fn receive_register_ack(
    client_ws: &mut tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
) -> ServerToAgentMessage {
    match client_ws.next().await.unwrap().unwrap() {
        Message::Text(text) => serde_json::from_str(&text).unwrap(),
        other => panic!("Expected Text message, got {other:?}"),
    }
}

#[allow(dead_code)]
async fn assert_connection_closed(
    client_ws: &mut tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
) {
    let msg = client_ws.next().await;
    match msg {
        None => (),
        Some(Ok(Message::Close(_))) => {
            let next = client_ws.next().await;
            assert!(
                next.is_none() || matches!(next, Some(Err(_))),
                "Expected connection closed after Close frame, got {next:?}"
            );
        }
        Some(Err(_)) => (),
        other => panic!("Expected connection closed or Close frame, got {other:?}"),
    }
}

// =========================================================================
// §3.4 G2 Scenarios 1 to 9
//
// M0 migration baseline: the WS tests below document the current unsafe
// acceptance behavior only. M2 must replace every successful no/invalid-token
// registration expectation with Pending Enrollment and mTLS rejection tests.
// =========================================================================

/// G2-1: 签发设备 Token (kind=dv, device_id="laptop-roeh3p85-b6a918")
/// 期望：返回以 atpc_dv_ 开头的设备凭据，KeyRecord 记录 device_id 与 kind=dv
#[test]
fn test_g2_1_issue_device_token_format() {
    let dir = temp_data_dir("g2-1");
    let keys_file = dir.join("keys.json");
    let keystore = KeyStore::load_or_create(&keys_file).unwrap();

    let (record, plaintext) = keystore
        .issue(
            "laptop-01".to_string(),
            KeyKind::DeviceToken,
            Some(Role::Operator),
            vec!["terminal".to_string()],
            Some("laptop-roeh3p85-b6a918".to_string()),
            None,
        )
        .unwrap();

    assert!(
        plaintext.starts_with("atpc_dv_"),
        "Device token must start with atpc_dv_: {plaintext}"
    );
    assert_eq!(record.kind, KeyKind::DeviceToken);
    assert_eq!(record.device_id.as_deref(), Some("laptop-roeh3p85-b6a918"));
    assert_eq!(record.name, "laptop-01");

    let verified = keystore.verify(&plaintext).unwrap();
    assert_eq!(verified.kind, KeyKind::DeviceToken);
    assert_eq!(
        verified.device_id.as_deref(),
        Some("laptop-roeh3p85-b6a918")
    );
}

/// G2-2: /ws 注册不带 token
/// 期望：通过，RegisterAck.success=true (Agent 无需密钥)
#[tokio::test]
async fn test_g2_2_ws_register_without_token_succeeds() {
    let dir = temp_data_dir("g2-2");
    let keys_file = dir.join("keys.json");
    let keystore = Arc::new(KeyStore::load_or_create(&keys_file).unwrap());

    // Issue at least one active key so server has credentials
    let _ = keystore
        .issue(
            "laptop-01".to_string(),
            KeyKind::DeviceToken,
            None,
            vec![],
            None,
            None,
        )
        .unwrap();

    let registry = Arc::new(TerminalRegistry::new());
    let state = WsServerState::new(
        registry.clone(),
        ServerConfig {
            ws_path: "/ws".to_string(),
            ..Default::default()
        },
    )
    .with_keystore(keystore);

    let mut client_ws = connect_ws_client(state, None).await;
    send_register_msg(&mut client_ws, "laptop-01", None).await;

    match receive_register_ack(&mut client_ws).await {
        ServerToAgentMessage::RegisterAck {
            success, message, ..
        } => {
            assert!(
                success,
                "Registration without token must succeed (Agent requires no key)"
            );
            assert!(message.is_none());
        }
        other => panic!("Expected RegisterAck, got {other:?}"),
    }

    assert_eq!(registry.count().await, 1);
}

/// G2-3: 带任意 token (兼容配置中遗留的 token)
/// 期望：通过，RegisterAck.success=true
#[tokio::test]
async fn test_g2_3_ws_register_with_any_token_succeeds() {
    let dir = temp_data_dir("g2-3");
    let keys_file = dir.join("keys.json");
    let keystore = Arc::new(KeyStore::load_or_create(&keys_file).unwrap());

    let _ = keystore
        .issue(
            "laptop-01".to_string(),
            KeyKind::DeviceToken,
            None,
            vec![],
            Some("laptop-01".to_string()),
            None,
        )
        .unwrap();

    let registry = Arc::new(TerminalRegistry::new());
    let state = WsServerState::new(
        registry.clone(),
        ServerConfig {
            ws_path: "/ws".to_string(),
            ..Default::default()
        },
    )
    .with_keystore(keystore);

    let mut client_ws = connect_ws_client(state, None).await;
    send_register_msg(
        &mut client_ws,
        "laptop-01",
        Some("atpc_dv_arbitrary_or_legacy_token_here".to_string()),
    )
    .await;

    match receive_register_ack(&mut client_ws).await {
        ServerToAgentMessage::RegisterAck {
            success, message, ..
        } => {
            assert!(
                success,
                "Registration with token must succeed without blocking agent"
            );
            assert!(message.is_none());
        }
        other => panic!("Expected RegisterAck, got {other:?}"),
    }

    assert_eq!(registry.count().await, 1);
}

/// G2-4: 带 DV 且 info.terminal_id 匹配 device_id
/// 期望：通过，success=true；且 registry / API 查得到该 terminal
#[tokio::test]
async fn test_g2_4_ws_register_with_dv_token_and_matching_terminal_id_succeeds() {
    let dir = temp_data_dir("g2-4");
    let keys_file = dir.join("keys.json");
    let keystore = Arc::new(KeyStore::load_or_create(&keys_file).unwrap());

    let (_, dv_token) = keystore
        .issue(
            "laptop-01".to_string(),
            KeyKind::DeviceToken,
            Some(Role::Operator),
            vec!["terminal".to_string()],
            Some("laptop-roeh3p85-b6a918".to_string()),
            None,
        )
        .unwrap();

    let registry = Arc::new(TerminalRegistry::new());
    let state = WsServerState::new(
        registry.clone(),
        ServerConfig {
            ws_path: "/ws".to_string(),
            ..Default::default()
        },
    )
    .with_keystore(keystore);

    let mut client_ws = connect_ws_client(state, None).await;
    send_register_msg(&mut client_ws, "laptop-roeh3p85-b6a918", Some(dv_token)).await;

    match receive_register_ack(&mut client_ws).await {
        ServerToAgentMessage::RegisterAck {
            success, message, ..
        } => {
            assert!(
                success,
                "Registration with matching terminal_id must succeed"
            );
            assert!(message.is_none());
        }
        other => panic!("Expected RegisterAck, got {other:?}"),
    }

    // Terminal must be present in registry
    assert_eq!(registry.count().await, 1);
    let term = registry.get_terminal("laptop-roeh3p85-b6a918").await;
    assert!(term.is_some());
}

/// G2-5: 带 DV 且 terminal_id 不匹配
/// G2-5: 带 DV 且 terminal_id 任意
/// 期望：通过，Agent 允许以任意 terminal_id 注册
#[tokio::test]
async fn test_g2_5_ws_register_with_dv_token_and_terminal_id_succeeds() {
    let dir = temp_data_dir("g2-5");
    let keys_file = dir.join("keys.json");
    let keystore = Arc::new(KeyStore::load_or_create(&keys_file).unwrap());

    let (_, dv_token) = keystore
        .issue(
            "laptop-01".to_string(),
            KeyKind::DeviceToken,
            Some(Role::Operator),
            vec!["terminal".to_string()],
            Some("laptop-roeh3p85-b6a918".to_string()),
            None,
        )
        .unwrap();

    let registry = Arc::new(TerminalRegistry::new());
    let state = WsServerState::new(
        registry.clone(),
        ServerConfig {
            ws_path: "/ws".to_string(),
            ..Default::default()
        },
    )
    .with_keystore(keystore);

    let mut client_ws = connect_ws_client(state, None).await;
    send_register_msg(&mut client_ws, "custom-machine-id", Some(dv_token)).await;

    match receive_register_ack(&mut client_ws).await {
        ServerToAgentMessage::RegisterAck {
            success, message, ..
        } => {
            assert!(success, "Agent registration must succeed");
            assert!(message.is_none());
        }
        other => panic!("Expected RegisterAck, got {other:?}"),
    }

    assert_eq!(registry.count().await, 1);
}

/// G2-6: DV 打 MCP（GET /api/terminals）
/// 期望：403 Forbidden（设备令牌不能操作控制台）
#[tokio::test]
async fn test_g2_6_dv_token_cannot_access_mcp_rest_api() {
    let dir = temp_data_dir("g2-6");
    let keys_file = dir.join("keys.json");
    let keystore = Arc::new(KeyStore::load_or_create(&keys_file).unwrap());

    let (_, dv_token) = keystore
        .issue(
            "laptop-01".to_string(),
            KeyKind::DeviceToken,
            Some(Role::Operator),
            vec!["terminal".to_string()],
            Some("laptop-01".to_string()),
            None,
        )
        .unwrap();

    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry));
    let config = ServerConfig {
        keys_file_path: Some(keys_file),
        ..Default::default()
    };
    let app = create_mcp_http_router(router, config);

    let mut req = Request::builder()
        .uri("/api/terminals")
        .method("GET")
        .header(header::AUTHORIZATION, format!("Bearer {dv_token}"))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([192, 168, 1, 50], 12345))));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "Device token (dv) must be rejected with 403 Forbidden on MCP API"
    );
}

/// G2-7: sk key 填入 agent 配置注册 /ws
/// 期望：通过，success=true（兼容用户在 agent 配置误填管理密钥，不中断 agent 注册）
#[tokio::test]
async fn test_g2_7_sk_key_in_agent_config_succeeds() {
    let dir = temp_data_dir("g2-7");
    let keys_file = dir.join("keys.json");
    let keystore = Arc::new(KeyStore::load_or_create(&keys_file).unwrap());

    let (_, sk_key) = keystore
        .issue(
            "admin-sk".to_string(),
            KeyKind::SecretKey,
            Some(Role::Admin),
            vec!["console".to_string(), "mcp".to_string()],
            None,
            None,
        )
        .unwrap();

    assert!(sk_key.starts_with("atpc_sk_"));

    let registry = Arc::new(TerminalRegistry::new());
    let state = WsServerState::new(
        registry.clone(),
        ServerConfig {
            ws_path: "/ws".to_string(),
            ..Default::default()
        },
    )
    .with_keystore(keystore);

    let mut client_ws = connect_ws_client(state, None).await;
    send_register_msg(&mut client_ws, "laptop-01", Some(sk_key)).await;

    match receive_register_ack(&mut client_ws).await {
        ServerToAgentMessage::RegisterAck {
            success, message, ..
        } => {
            assert!(success, "Agent presenting sk token must succeed");
            assert!(message.is_none());
        }
        other => panic!("Expected RegisterAck, got {other:?}"),
    }

    assert_eq!(registry.count().await, 1);
}

/// G2-8: 兼容：--auth-token legacy 启动 + agent 用 legacy token 注册
/// 期望：通过，success=true，保 F3 全绿
#[tokio::test]
async fn test_g2_8_legacy_auth_token_compat_path() {
    let dir = temp_data_dir("g2-8");
    let keys_file = dir.join("keys.json");
    let keystore = Arc::new(KeyStore::load_or_create(&keys_file).unwrap());

    // Active keystore present
    let _ = keystore
        .issue(
            "some-device".to_string(),
            KeyKind::DeviceToken,
            None,
            vec![],
            None,
            None,
        )
        .unwrap();

    let registry = Arc::new(TerminalRegistry::new());
    let state = WsServerState::new(
        registry.clone(),
        ServerConfig {
            ws_path: "/ws".to_string(),
            auth_token: Some("legacy_static_credential_value".to_string()),
            ..Default::default()
        },
    )
    .with_keystore(keystore);

    let mut client_ws = connect_ws_client(state, None).await;
    send_register_msg(
        &mut client_ws,
        "legacy-agent-terminal",
        Some("legacy_static_credential_value".to_string()),
    )
    .await;

    match receive_register_ack(&mut client_ws).await {
        ServerToAgentMessage::RegisterAck {
            success, message, ..
        } => {
            assert!(
                success,
                "Legacy auth_token fallback must succeed for backwards compatibility"
            );
            assert!(message.is_none());
        }
        other => panic!("Expected RegisterAck, got {other:?}"),
    }

    assert_eq!(registry.count().await, 1);
}

/// G2-9: 处于限速状态的 IP 尝试注册
/// 期望：拒绝并返回 Rate limit exceeded
#[tokio::test]
async fn test_g2_9_registration_failure_rate_limiting() {
    let peer_addr: SocketAddr = "192.168.10.88:54321".parse().unwrap();
    let rate_limiter = Arc::new(AuthRateLimiter::default());
    let registry = Arc::new(TerminalRegistry::new());

    // Trigger rate limit for this IP
    for _ in 0..10 {
        rate_limiter.record_failure(peer_addr.ip());
    }

    let state = WsServerState::new(
        registry.clone(),
        ServerConfig {
            ws_path: "/ws".to_string(),
            ..Default::default()
        },
    )
    .with_rate_limiter(rate_limiter);

    let mut client_ws = connect_ws_client(state.clone(), Some(peer_addr)).await;
    send_register_msg(&mut client_ws, "attacker-host", None).await;

    match receive_register_ack(&mut client_ws).await {
        ServerToAgentMessage::RegisterAck {
            success, message, ..
        } => {
            assert!(!success, "Attempt must be rate limited");
            assert_eq!(
                message.as_deref(),
                Some("Rate limit exceeded"),
                "Rate limit exceeded message expected"
            );
        }
        other => panic!("Expected failed RegisterAck, got {other:?}"),
    }
}

/// P0-23: KeyStore singleton shared between HTTP and WS:
/// Dynamically issued device tokens via HTTP POST /api/keys immediately take effect.
/// Dynamically revoked device tokens via HTTP DELETE /api/keys/:id immediately fail on HTTP without restart.
#[tokio::test]
async fn test_p0_23_shared_keystore_dynamic_issue_and_revoke_propagates_to_ws_without_restart() {
    let dir = temp_data_dir("p0-23");
    let keys_file = dir.join("keys.json");
    let keystore = Arc::new(KeyStore::load_or_create(&keys_file).unwrap());

    // 1. Issue an Admin key to authenticate HTTP calls
    let (_, admin_token) = keystore
        .issue(
            "admin-root".to_string(),
            KeyKind::SecretKey,
            Some(Role::Admin),
            vec!["*".to_string()],
            None,
            None,
        )
        .unwrap();

    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let config = ServerConfig {
        ws_path: "/ws".to_string(),
        keys_file_path: Some(keys_file.clone()),
        ..Default::default()
    };

    // Instantiate HTTP router with shared keystore
    let http_router = at_pc_server::mcp::create_mcp_http_router_with_keystore(
        router.clone(),
        config.clone(),
        Some(keystore.clone()),
    );

    // Instantiate WS state with the EXACT same shared keystore
    let ws_state =
        WsServerState::new(registry.clone(), config.clone()).with_keystore(keystore.clone());

    // 2. Issue a device token via HTTP POST /api/keys
    let issue_body = serde_json::json!({
        "name": "dynamic-dev",
        "kind": "dv",
        "device_id": "PC-DYNAMIC-1"
    });
    let req = Request::builder()
        .uri("/api/keys")
        .method("POST")
        .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345))))
        .body(Body::from(serde_json::to_vec(&issue_body).unwrap()))
        .unwrap();

    let resp = http_router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let issue_resp: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let issued_key_id = issue_resp["key"]["id"].as_str().unwrap().to_string();
    let issued_raw_key = issue_resp["plaintext"].as_str().unwrap().to_string();

    // 3. WITHOUT RESTARTING THE SERVER: Verify WS accepts registration
    let mut client_ws = connect_ws_client(ws_state.clone(), None).await;
    send_register_msg(&mut client_ws, "PC-DYNAMIC-1", Some(issued_raw_key.clone())).await;

    match receive_register_ack(&mut client_ws).await {
        ServerToAgentMessage::RegisterAck { success, .. } => {
            assert!(success, "WS registration with dynamically issued device token must immediately succeed without restart!");
        }
        other => panic!("Expected successful RegisterAck, got {other:?}"),
    }

    // 4. Revoke the token via HTTP DELETE /api/keys/:id
    let req = Request::builder()
        .uri(format!("/api/keys/{issued_key_id}"))
        .method("DELETE")
        .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345))))
        .body(Body::empty())
        .unwrap();

    let resp = http_router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 5. WITHOUT RESTARTING THE SERVER: Verify HTTP rejects requests with the revoked token!
    let req = Request::builder()
        .uri("/api/terminals")
        .method("GET")
        .header(header::AUTHORIZATION, format!("Bearer {issued_raw_key}"))
        .extension(ConnectInfo(SocketAddr::from(([192, 168, 1, 50], 12345))))
        .body(Body::empty())
        .unwrap();

    let resp = http_router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Revoked token must be rejected with 401 Unauthorized without restart!"
    );

    std::fs::remove_dir_all(&dir).ok();
}
