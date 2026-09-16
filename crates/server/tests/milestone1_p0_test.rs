use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tower::ServiceExt;

use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::TerminalInfo;
use at_pc_server::config::ServerConfig;
use at_pc_server::mcp::{create_mcp_http_router, run_stdio_server_with_streams};
use at_pc_server::router::McpRouter;
use at_pc_server::ws::handler::{handle_stream, WsServerState};
use at_pc_server::ws::registry::TerminalRegistry;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

fn create_test_terminal(id: &str, hostname: &str) -> TerminalInfo {
    TerminalInfo {
        terminal_id: id.to_string(),
        hostname: hostname.to_string(),
        username: "test_user".to_string(),
        lan_ip: "127.0.0.1".to_string(),
        os_version: "macOS 15.0".to_string(),
        agent_version: "0.3.1".to_string(),
    }
}

// =========================================================================
// [P0-1] Unified Authentication Middleware for /api/* Endpoints
// =========================================================================
#[tokio::test]
async fn test_p0_1_api_authentication_middleware() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry));

    let config = ServerConfig {
        auth_token: Some("secret_token_123".to_string()),
        ..Default::default()
    };
    let app = create_mcp_http_router(router, config);

    // 1. Unauthenticated request to /api/terminals -> 401 Unauthorized
    let req = Request::builder()
        .uri("/api/terminals")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 2. Unauthenticated request to /api/terminals/node-1/invoke -> 401 Unauthorized
    let invoke_body = json!({
        "tool": "exec_cmd",
        "arguments": { "command": "whoami" }
    });
    let req = Request::builder()
        .uri("/api/terminals/node-1/invoke")
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&invoke_body).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 3. Unauthenticated request to /api/logs -> 401 Unauthorized
    let req = Request::builder()
        .uri("/api/logs")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 4. Authenticated with Authorization: Bearer secret_token_123 -> 200 OK
    let req = Request::builder()
        .uri("/api/terminals")
        .method("GET")
        .header("Authorization", "Bearer secret_token_123")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 5. Authenticated with Cookie: token=secret_token_123 -> 200 OK
    let req = Request::builder()
        .uri("/api/terminals")
        .method("GET")
        .header("Cookie", "token=secret_token_123")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 6. Authenticated with Query parameter ?token=secret_token_123 -> 200 OK
    let req = Request::builder()
        .uri("/api/terminals?token=secret_token_123")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 7. Web Dashboard HTML endpoints remain accessible for user to enter token
    let req = Request::builder()
        .uri("/dashboard")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// =========================================================================
// [P0-2] Strict CORS Policy & Configurable Allowed Origins
// =========================================================================
#[tokio::test]
async fn test_p0_2_cors_policy_restricted_and_allowlist() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry));

    // 1. Default config has listen_host = 127.0.0.1 and restricted CORS
    let default_cfg = ServerConfig::default();
    assert_eq!(default_cfg.listen_host, "127.0.0.1");
    assert!(default_cfg.allowed_origins.is_empty());

    let app_default = create_mcp_http_router(router.clone(), default_cfg);

    // Origin: http://evil.com should NOT receive Access-Control-Allow-Origin
    let req = Request::builder()
        .uri("/health")
        .method("GET")
        .header("Origin", "http://evil.com")
        .body(Body::empty())
        .unwrap();
    let resp = app_default.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(!resp.headers().contains_key("access-control-allow-origin"));

    // 2. Config with allowed_origins allows specific origins
    let allow_cfg = ServerConfig {
        allowed_origins: vec!["https://trusted-admin.corp".to_string()],
        ..Default::default()
    };
    let app_allowed = create_mcp_http_router(router, allow_cfg);

    // Origin: https://trusted-admin.corp receives Access-Control-Allow-Origin
    let req = Request::builder()
        .uri("/health")
        .method("GET")
        .header("Origin", "https://trusted-admin.corp")
        .body(Body::empty())
        .unwrap();
    let resp = app_allowed.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("access-control-allow-origin").unwrap(),
        "https://trusted-admin.corp"
    );

    // Unlisted origin receives no allow header
    let req = Request::builder()
        .uri("/health")
        .method("GET")
        .header("Origin", "http://untrusted-hacker.com")
        .body(Body::empty())
        .unwrap();
    let resp = app_allowed.oneshot(req).await.unwrap();
    assert!(!resp.headers().contains_key("access-control-allow-origin"));
}

// =========================================================================
// [P0-4] Fail-Fast on Terminal Disconnection
// =========================================================================
#[tokio::test]
async fn test_p0_4_fail_fast_when_terminal_disconnects() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let (tx, mut rx) = mpsc::unbounded_channel();

    let term = create_test_terminal("fast-fail-node", "FAST-FAIL-PC");
    registry.register(term, tx).await;

    // Launch tool invocation with 35s timeout in background
    let router_clone = router.clone();
    let invoke_handle = tokio::spawn(async move {
        let start = Instant::now();
        let res = router_clone
            .invoke_tool(
                "fast-fail-node",
                "exec_cmd",
                json!({"command": "sleep 30"}),
                35,
            )
            .await;
        (res, start.elapsed())
    });

    // Wait for the message to reach the terminal channel
    let msg = rx.recv().await.expect("Expected InvokeTool message");
    assert!(matches!(msg, ServerToAgentMessage::InvokeTool { .. }));

    // Immediately simulate disconnect
    let aborted =
        router.abort_pending_calls_for_terminal("fast-fail-node", "Agent process terminated");
    assert_eq!(aborted, 1);

    // Verify the invocation failed immediately (< 100ms) without waiting 35s
    let (res, elapsed) = invoke_handle.await.unwrap();
    assert!(
        elapsed.as_millis() < 100,
        "Fail-fast took too long: {:?}",
        elapsed
    );
    assert!(res.is_err());
    let err_msg = res.unwrap_err();
    assert!(err_msg.contains("disconnected"), "Error msg: {}", err_msg);
    assert!(
        err_msg.contains("Agent process terminated"),
        "Error msg: {}",
        err_msg
    );
}

#[tokio::test]
async fn test_p0_4_fail_fast_via_ws_connection_drop() {
    let registry = Arc::new(TerminalRegistry::new());
    let config = ServerConfig {
        ws_path: "/ws".to_string(),
        auth_token: None,
        ..Default::default()
    };
    let router = Arc::new(McpRouter::new(registry.clone()));
    let state = WsServerState::with_handler(registry.clone(), config.clone(), router.clone());

    // Duplex stream simulating TCP connection
    let (client_stream, server_stream) = tokio::io::duplex(8192);

    let server_task = tokio::spawn(async move {
        handle_stream(server_stream, state).await;
    });

    let (mut client_ws, _) = tokio_tungstenite::client_async("ws://127.0.0.1/ws", client_stream)
        .await
        .expect("Client handshake failed");

    // 1. Register
    let reg_msg = AgentToServerMessage::Register {
        info: create_test_terminal("ws-fail-fast-node", "WS-NODE"),
        auth_token: None,
    };
    client_ws
        .send(Message::Text(serde_json::to_string(&reg_msg).unwrap()))
        .await
        .unwrap();

    let ack_msg = client_ws.next().await.unwrap().unwrap();
    assert!(matches!(ack_msg, Message::Text(_)));

    // 2. Invoke tool on this terminal
    let router_clone = router.clone();
    let invoke_handle = tokio::spawn(async move {
        let start = Instant::now();
        let res = router_clone
            .invoke_tool(
                "ws-fail-fast-node",
                "exec_cmd",
                json!({"command": "sleep 30"}),
                35,
            )
            .await;
        (res, start.elapsed())
    });

    // Read the InvokeTool on client
    let invoke_ws_msg = client_ws.next().await.unwrap().unwrap();
    assert!(matches!(invoke_ws_msg, Message::Text(_)));

    // 3. Client crashes / connection drops
    drop(client_ws);

    // Verify fail-fast immediately triggers (< 150ms)
    let (res, elapsed) = invoke_handle.await.unwrap();
    assert!(
        elapsed.as_millis() < 150,
        "Fail-fast via WS drop took too long: {:?}",
        elapsed
    );
    assert!(res.is_err());
    let err_str = res.unwrap_err();
    assert!(err_str.contains("disconnected"), "Error was: {}", err_str);

    let _ = server_task.await;
}

// =========================================================================
// [P0-5] MCP Stdio Mode Non-blocking Asynchronous Processing
// =========================================================================
#[tokio::test]
async fn test_p0_5_stdio_async_concurrency() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let (tx, mut rx) = mpsc::unbounded_channel();

    let term = create_test_terminal("stdio-term-01", "STDIO-PC");
    registry.register(term, tx).await;

    // Simulate agent responding after 300ms (slow command)
    let router_agent = router.clone();
    tokio::spawn(async move {
        if let Some(ServerToAgentMessage::InvokeTool { call_id, .. }) = rx.recv().await {
            tokio::time::sleep(Duration::from_millis(300)).await;
            router_agent
                .handle_tool_result(AgentToServerMessage::ToolResult {
                    call_id,
                    success: true,
                    result: json!({"output": "slow command done"}),
                    error: None,
                    duration_ms: 300,
                })
                .await;
        }
    });

    let (client_in, server_in) = tokio::io::duplex(4096);
    let (server_out, client_out) = tokio::io::duplex(4096);

    let router_clone = router.clone();
    let stdio_handle = tokio::spawn(async move {
        let _ = run_stdio_server_with_streams(router_clone, server_in, server_out).await;
    });

    let mut client_writer = client_in;
    let mut client_reader = BufReader::new(client_out).lines();

    // 1. Send slow tool invocation: exec_cmd on stdio-term-01
    let slow_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "exec_cmd",
            "arguments": {
                "terminal_id": "stdio-term-01",
                "command": "slow_cmd"
            }
        }
    });
    client_writer
        .write_all((serde_json::to_string(&slow_req).unwrap() + "\n").as_bytes())
        .await
        .unwrap();

    // 2. Immediately send ping request
    let ping_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "ping"
    });
    client_writer
        .write_all((serde_json::to_string(&ping_req).unwrap() + "\n").as_bytes())
        .await
        .unwrap();

    // 3. Receive first response - it MUST be ping (id: 2) within 100ms, before slow call (300ms) finishes!
    let start = Instant::now();
    let first_line = client_reader
        .next_line()
        .await
        .unwrap()
        .expect("Expected line from stdio");
    let elapsed = start.elapsed();

    let first_resp: Value = serde_json::from_str(&first_line).unwrap();
    assert_eq!(
        first_resp["id"], 2,
        "First response should be ping (id 2), proving non-blocking stdio!"
    );
    assert!(
        elapsed.as_millis() < 100,
        "Ping response took too long: {:?}",
        elapsed
    );

    // Clean up
    drop(client_writer);
    let _ = stdio_handle.await;
}

#[tokio::test]
async fn test_p0_1_auth_token_case_sensitivity_and_bearer_scheme() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry));

    let config = ServerConfig {
        auth_token: Some("Secret_Token_AbC".to_string()),
        ..Default::default()
    };
    let app = create_mcp_http_router(router, config);

    // 1. Lowercase "bearer " scheme name -> 200 OK
    let req = Request::builder()
        .uri("/api/terminals")
        .method("GET")
        .header("Authorization", "bearer Secret_Token_AbC")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. Uppercase "BEARER " scheme name -> 200 OK
    let req = Request::builder()
        .uri("/api/terminals")
        .method("GET")
        .header("Authorization", "BEARER Secret_Token_AbC")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 3. Wrong casing of secret token -> 401 Unauthorized
    let req = Request::builder()
        .uri("/api/terminals")
        .method("GET")
        .header("Authorization", "Bearer secret_token_abc")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 4. URL percent-encoded token in query -> 200 OK
    let req = Request::builder()
        .uri("/api/terminals?token=Secret%5FToken%5FAbC")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_p0_2_cors_preflight_on_authenticated_api() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry));

    let config = ServerConfig {
        auth_token: Some("my_secret_token".to_string()),
        allowed_origins: vec!["https://trusted-admin.corp".to_string()],
        ..Default::default()
    };
    let app = create_mcp_http_router(router.clone(), config);

    // Browser sends OPTIONS preflight to /api/terminals without Authorization header
    let req = Request::builder()
        .uri("/api/terminals")
        .method("OPTIONS")
        .header("Origin", "https://trusted-admin.corp")
        .header("Access-Control-Request-Method", "GET")
        .header(
            "Access-Control-Request-Headers",
            "authorization,content-type",
        )
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    // Must NOT return 401 Unauthorized; preflight must succeed with CORS headers
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("access-control-allow-origin").unwrap(),
        "https://trusted-admin.corp"
    );

    // Wildcard CORS config allows any origin
    let wildcard_config = ServerConfig {
        auth_token: Some("token_123".to_string()),
        allowed_origins: vec!["*".to_string()],
        ..Default::default()
    };
    let wildcard_app = create_mcp_http_router(router, wildcard_config);

    let req = Request::builder()
        .uri("/health")
        .method("GET")
        .header("Origin", "https://any-external-domain.io")
        .header("Authorization", "Bearer token_123")
        .body(Body::empty())
        .unwrap();
    let resp = wildcard_app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("access-control-allow-origin").unwrap(),
        "*"
    );
}

#[tokio::test]
async fn test_p0_4_fail_fast_via_heartbeat_sweep() {
    let registry = Arc::new(TerminalRegistry::with_threshold(Duration::from_millis(20)));
    let router = Arc::new(McpRouter::new(registry.clone()));
    let (tx, mut rx) = mpsc::unbounded_channel();

    let term = create_test_terminal("sweep-node-01", "SWEEP-PC");
    registry.register(term, tx).await;

    // Launch tool invocation in background
    let router_clone = router.clone();
    let invoke_handle = tokio::spawn(async move {
        let start = Instant::now();
        let res = router_clone
            .invoke_tool(
                "sweep-node-01",
                "exec_cmd",
                json!({"command": "sleep 30"}),
                35,
            )
            .await;
        (res, start.elapsed())
    });

    let msg = rx.recv().await.expect("Expected InvokeTool message");
    assert!(matches!(msg, ServerToAgentMessage::InvokeTool { .. }));

    // Wait for heartbeat threshold to expire
    tokio::time::sleep(Duration::from_millis(35)).await;

    // Start sweep task with router as message handler
    let sweep_handle = registry
        .clone()
        .start_sweep_task_with_handler(Duration::from_millis(20), router.clone());

    let (res, elapsed) = invoke_handle.await.unwrap();
    assert!(
        elapsed.as_millis() < 200,
        "Fail-fast via sweep took too long: {:?}",
        elapsed
    );
    assert!(res.is_err());
    let err_msg = res.unwrap_err();
    assert!(
        err_msg.contains("Heartbeat timed out"),
        "Error was: {}",
        err_msg
    );

    sweep_handle.abort();
}

#[tokio::test]
async fn test_p0_5_stdio_jsonrpc_parse_error_response() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry));

    let (client_in, server_in) = tokio::io::duplex(4096);
    let (server_out, client_out) = tokio::io::duplex(4096);

    let stdio_handle = tokio::spawn(async move {
        let _ = run_stdio_server_with_streams(router, server_in, server_out).await;
    });

    let mut client_writer = client_in;
    let mut client_reader = BufReader::new(client_out).lines();

    // 1. Send malformed JSON
    client_writer
        .write_all(b"{\"invalid json line\n")
        .await
        .unwrap();

    let err_line = client_reader
        .next_line()
        .await
        .unwrap()
        .expect("Expected response for parse error");
    let err_val: Value = serde_json::from_str(&err_line).unwrap();
    assert_eq!(err_val["jsonrpc"], "2.0");
    assert_eq!(
        err_val["error"]["code"], -32700,
        "Expected JSON-RPC parse error code -32700"
    );

    // 2. Send empty batch []
    client_writer.write_all(b"[]\n").await.unwrap();

    let empty_batch_line = client_reader
        .next_line()
        .await
        .unwrap()
        .expect("Expected response for empty batch");
    let empty_batch_val: Value = serde_json::from_str(&empty_batch_line).unwrap();
    assert_eq!(empty_batch_val["jsonrpc"], "2.0");
    assert_eq!(
        empty_batch_val["error"]["code"], -32600,
        "Expected invalid request code -32600 for empty batch"
    );

    drop(client_writer);
    let _ = stdio_handle.await;
}
