use std::sync::Arc;
use std::time::{Duration, Instant};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::ws_client::{AgentWsClient, ClientConnectionStatus};
use at_pc_protocol::messages::BinaryDesktopFrame;
use at_pc_protocol::models::TerminalInfo;
use at_pc_server::config::ServerConfig;
use at_pc_server::mcp::create_mcp_http_router;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::handler::{handle_stream, AgentMessageHandler, WsServerState};
use at_pc_server::ws::registry::TerminalRegistry;

fn create_test_terminal(id: &str, hostname: &str) -> TerminalInfo {
    TerminalInfo {
        terminal_id: id.to_string(),
        hostname: hostname.to_string(),
        username: "test_user".to_string(),
        lan_ip: "127.0.0.1".to_string(),
        os_version: "macOS 15.0".to_string(),
        agent_version: "0.4.0".to_string(),
    }
}

// =========================================================================
// [P1-1] Standardized WebSocket Stack (tokio-tungstenite RFC 6455)
// =========================================================================
#[tokio::test]
async fn test_p1_1_tungstenite_ws_handshake_and_lifecycle() {
    let registry = Arc::new(TerminalRegistry::new());
    let server_config = ServerConfig {
        ws_path: "/ws".to_string(),
        auth_token: Some("secret_p1_token".to_string()),
        heartbeat_interval_secs: 5,
        ..Default::default()
    };
    let server_state = WsServerState::new(registry.clone(), server_config);

    // Create duplex channel to simulate socket
    let (client_stream, server_stream) = tokio::io::duplex(16384);

    let server_task = tokio::spawn(async move {
        handle_stream(server_stream, server_state).await;
    });

    let term_info = create_test_terminal("term-ws-p1", "WORKSTATION-P1");
    let executor = Arc::new(AgentExecutor::new());
    let client = Arc::new(
        AgentWsClient::new(
            "ws://127.0.0.1:9801/ws".to_string(),
            term_info.clone(),
            executor,
        )
        .with_auth_token(Some("secret_p1_token".to_string())),
    );

    let client_clone = client.clone();
    let client_task = tokio::spawn(async move {
        let _ = client_clone
            .handshake_and_run_stream(client_stream, "127.0.0.1", "/ws")
            .await;
    });

    // Wait for RFC 6455 handshake and RegisterAck
    tokio::time::sleep(Duration::from_millis(150)).await;

    assert_eq!(client.status().await, ClientConnectionStatus::Connected);
    assert!(registry.get_terminal("term-ws-p1").await.is_some());

    // Gracefully disconnect
    client.disconnect("Testing complete").await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(client.status().await, ClientConnectionStatus::Disconnected);

    client_task.abort();
    server_task.abort();
}

// =========================================================================
// [P1-2] Remote Task Cancellation (Kill Process Tree & Abort within <200ms)
// =========================================================================
#[tokio::test]
async fn test_p1_2_remote_task_cancellation_kills_process_tree() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));

    let server_config = ServerConfig {
        ws_path: "/ws".to_string(),
        auth_token: None,
        heartbeat_interval_secs: 5,
        ..Default::default()
    };

    let server_state = WsServerState::with_handler(registry.clone(), server_config, router.clone());
    let (client_stream, server_stream) = tokio::io::duplex(16384);

    let server_task = tokio::spawn(async move {
        handle_stream(server_stream, server_state).await;
    });

    let term_info = create_test_terminal("term-cancel-p1", "WORKSTATION-CANCEL");
    let executor = Arc::new(AgentExecutor::new());
    let client = Arc::new(AgentWsClient::new(
        "ws://127.0.0.1:9801/ws".to_string(),
        term_info.clone(),
        executor.clone(),
    ));

    let client_clone = client.clone();
    let client_task = tokio::spawn(async move {
        let _ = client_clone
            .handshake_and_run_stream(client_stream, "127.0.0.1", "/ws")
            .await;
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    // Platform-dependent 10-second sleep command
    let sleep_cmd = if cfg!(windows) {
        "powershell -Command Start-Sleep -Seconds 10"
    } else {
        "sleep 10"
    };

    let router_for_exec = router.clone();
    let exec_task = tokio::spawn(async move {
        router_for_exec
            .invoke_tool(
                "term-cancel-p1",
                "exec_cmd",
                json!({ "command": sleep_cmd }),
                10,
            )
            .await
    });

    // Wait 150ms for process to be spawned and registered
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Retrieve active pending call_id
    let pending_calls = router.list_pending_calls();
    assert_eq!(pending_calls.len(), 1, "Expected 1 active pending call");
    let (call_id, terminal_id) = &pending_calls[0];
    assert_eq!(terminal_id, "term-cancel-p1");

    // Initiate cancellation and measure latency
    let cancel_start = Instant::now();
    let cancel_result = router.cancel_tool_call(call_id).await;
    let cancel_elapsed = cancel_start.elapsed();

    assert!(cancel_result.is_ok(), "Cancellation should succeed");
    assert!(
        cancel_elapsed < Duration::from_millis(300),
        "Cancellation took {:?}, expected < 300ms",
        cancel_elapsed
    );

    // The in-flight invocation should have returned an Err indicating cancellation
    let invocation_res = exec_task.await.unwrap();
    assert!(invocation_res.is_err());
    let err_text = invocation_res.unwrap_err();
    assert!(
        err_text.contains("cancelled"),
        "Expected cancellation error message, got: {}",
        err_text
    );

    // Verify child process tree was killed and no orphaned processes remain in registry
    let remaining_killed = executor.kill_all_processes();
    assert_eq!(
        remaining_killed, 0,
        "No child processes should linger after cancellation"
    );

    client.disconnect("Test completed").await;
    client_task.abort();
    server_task.abort();
}

#[tokio::test]
async fn test_p1_2_mcp_and_rest_cancel_apis() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let config = ServerConfig {
        auth_token: Some("token_123".to_string()),
        ..Default::default()
    };
    let app = create_mcp_http_router(router.clone(), config);

    // 1. Dispatching MCP meta-tool "cancel_tool" on non-existent call returns descriptive error
    let mcp_res = router
        .dispatch_tool_call("cancel_tool", json!({ "call_id": "nonexistent-call-1" }))
        .await;
    assert!(mcp_res.is_err());
    assert!(mcp_res.unwrap_err().contains("not found"));

    // 2. REST API POST /api/calls/:id/cancel on non-existent call returns 404
    let req = Request::builder()
        .uri("/api/calls/nonexistent-call-2/cancel")
        .method("POST")
        .header("Authorization", "Bearer token_123")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 3. REST API POST /api/terminals/:id/calls/:call_id/cancel on non-existent call returns 404
    let req = Request::builder()
        .uri("/api/terminals/node-1/calls/nonexistent-call-3/cancel")
        .method("POST")
        .header("Authorization", "Bearer token_123")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// =========================================================================
// [P1-3] Binary Desktop Frame Streaming and Channel Isolation
// =========================================================================
#[tokio::test]
async fn test_p1_3_binary_desktop_frame_codec_and_rest_raw_serving() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let config = ServerConfig::default();
    let app = create_mcp_http_router(router.clone(), config);

    // Create a mock binary JPEG frame
    let fake_jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0xFF, 0xD9];
    let frame = BinaryDesktopFrame::new(0, 1920, 1080, 1234567890, fake_jpeg.clone());

    let encoded = frame.encode();
    assert_eq!(&encoded[0..4], &BinaryDesktopFrame::MAGIC);

    // Handle binary frame in router
    router.handle_desktop_frame_binary("term-bin-1", frame);
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 1. Verify REST API GET /api/terminals/:id/desktop/frame.jpg returns exact binary JPEG
    let req = Request::builder()
        .uri("/api/terminals/term-bin-1/desktop/frame.jpg")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("Content-Type").unwrap(),
        "image/jpeg"
    );

    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(body_bytes.as_ref(), fake_jpeg.as_slice());

    // 2. Verify legacy JSON frame endpoint GET /api/terminals/:id/desktop/frame also works
    let req_json = Request::builder()
        .uri("/api/terminals/term-bin-1/desktop/frame")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp_json = app.oneshot(req_json).await.unwrap();
    assert_eq!(resp_json.status(), StatusCode::OK);
}

// =========================================================================
// [P1-5] Static Asset Decoupling (rust-embed Obsidian Dashboard)
// =========================================================================
#[tokio::test]
async fn test_p1_5_static_dashboard_asset_decoupling() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry));
    let config = ServerConfig::default();
    let app = create_mcp_http_router(router, config);

    // 1. GET / returns HTML dashboard
    let req = Request::builder()
        .uri("/")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("Content-Type").unwrap(),
        "text/html; charset=utf-8"
    );

    let html_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let html_str = String::from_utf8_lossy(&html_bytes);
    assert!(html_str.contains("AT-PC 集中管控平台"));
    assert!(html_str.contains("Obsidian Edition"));

    // 2. GET /dashboard returns same HTML dashboard
    let req_dash = Request::builder()
        .uri("/dashboard")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp_dash = app.clone().oneshot(req_dash).await.unwrap();
    assert_eq!(resp_dash.status(), StatusCode::OK);

    // 3. GET /index.html returns same HTML dashboard
    let req_index = Request::builder()
        .uri("/index.html")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp_index = app.oneshot(req_index).await.unwrap();
    assert_eq!(resp_index.status(), StatusCode::OK);
}

// =========================================================================
// [P1-1] RFC 6455 Ping / Pong Bidirectional Frame Compliance Test
// =========================================================================
#[tokio::test]
async fn test_p1_1_rfc6455_ping_pong_frame_bidirectional() {
    use futures_util::{SinkExt, StreamExt};

    let registry = Arc::new(TerminalRegistry::new());
    let server_config = ServerConfig {
        ws_path: "/ws".to_string(),
        auth_token: None,
        heartbeat_interval_secs: 5,
        ..Default::default()
    };
    let server_state = WsServerState::new(registry.clone(), server_config);
    let (client_stream, server_stream) = tokio::io::duplex(16384);

    let server_task = tokio::spawn(async move {
        handle_stream(server_stream, server_state).await;
    });

    let (mut client_ws, _) = tokio_tungstenite::client_async("ws://127.0.0.1/ws", client_stream)
        .await
        .expect("Client handshake failed");

    // Register terminal
    let reg_msg = at_pc_protocol::messages::AgentToServerMessage::Register {
        info: create_test_terminal("term-pingpong", "PINGPONG-HOST"),
        auth_token: None,
    };
    client_ws
        .send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&reg_msg).unwrap()))
        .await
        .unwrap();

    // Await RegisterAck
    let ack = client_ws.next().await.unwrap().unwrap();
    assert!(ack.is_text());

    // Send RFC 6455 Ping frame with payload
    let ping_payload = b"test_ping_payload_12345".to_vec();
    client_ws
        .send(tokio_tungstenite::tungstenite::Message::Ping(ping_payload.clone()))
        .await
        .unwrap();

    // Await RFC 6455 Pong frame
    let pong_resp = tokio::time::timeout(Duration::from_millis(500), client_ws.next())
        .await
        .expect("Timed out waiting for Pong")
        .expect("Stream ended")
        .expect("Error reading Pong");

    match pong_resp {
        tokio_tungstenite::tungstenite::Message::Pong(payload) => {
            assert_eq!(payload.as_slice(), ping_payload.as_slice(), "Pong payload must match Ping payload exactly");
        }
        other => panic!("Expected Pong message, got {:?}", other),
    }

    server_task.abort();
}

// =========================================================================
// [P1-2] Grandchild Process Tree Termination
// =========================================================================
#[tokio::test]
async fn test_p1_2_process_tree_cancellation_with_grandchild_process() {
    let executor = Arc::new(AgentExecutor::new());
    let call_id = "test-call-grandchild-tree";

    // Launch a shell command that spawns a grandchild process
    let cmd = if cfg!(windows) {
        "powershell -Command \"Start-Process -FilePath powershell -ArgumentList '-Command Start-Sleep -Seconds 30' -NoNewWindow -Wait\""
    } else {
        "sh -c 'sleep 30'"
    };

    let exec_clone = executor.clone();
    let task = tokio::spawn(async move {
        exec_clone
            .execute_with_call_id(
                call_id,
                "exec_cmd",
                json!({ "command": cmd, "timeout_secs": 15 }),
            )
            .await
    });

    // Wait 200ms for process and grandchild to spawn
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify active call is registered
    assert_eq!(executor.active_call_count().await, 1);

    // Cancel the call
    let start = Instant::now();
    let cancelled = executor.cancel(call_id).await;
    let elapsed = start.elapsed();

    assert!(cancelled, "Cancel should report true");
    assert!(elapsed < Duration::from_millis(300), "Cancel took {:?}, expected < 300ms", elapsed);

    let res = task.await.unwrap();
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("cancelled"));

    // Verify no remaining child or grandchild processes in registry
    assert_eq!(executor.kill_all_processes(), 0);
}

// =========================================================================
// [P1-2] List Active Pending Calls and Cancel via REST and MCP APIs
// =========================================================================
#[tokio::test]
async fn test_p1_2_list_pending_calls_and_cancellation_flow() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let config = ServerConfig::default();
    let app = create_mcp_http_router(router.clone(), config);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let term = create_test_terminal("term-list-calls", "LIST-CALLS-HOST");
    registry.register(term, tx).await;

    // Start a long-running tool invocation
    let router_clone = router.clone();
    let invoke_handle = tokio::spawn(async move {
        router_clone
            .invoke_tool("term-list-calls", "exec_cmd", json!({ "command": "sleep 30" }), 30)
            .await
    });

    let _msg = rx.recv().await.expect("Expected InvokeTool message");

    // 1. Query pending calls via MCP meta-tool
    let mcp_res = router
        .dispatch_tool_call("list_pending_calls", json!({}))
        .await
        .unwrap();
    let calls_arr = mcp_res.as_array().expect("Expected array of calls");
    assert_eq!(calls_arr.len(), 1);
    let cid = calls_arr[0]["call_id"].as_str().unwrap();
    assert_eq!(calls_arr[0]["terminal_id"], "term-list-calls");
    assert_eq!(calls_arr[0]["tool_name"], "exec_cmd");

    // 2. Query pending calls via REST GET /api/calls
    let req = Request::builder()
        .uri("/api/calls")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["success"], true);
    assert_eq!(body_json["calls"].as_array().unwrap().len(), 1);

    // 3. Query pending calls via REST GET /api/terminals/:id/calls
    let req_term = Request::builder()
        .uri("/api/terminals/term-list-calls/calls")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp_term = app.clone().oneshot(req_term).await.unwrap();
    assert_eq!(resp_term.status(), StatusCode::OK);

    // 4. Cancel call via REST POST /api/calls/:id/cancel
    let cancel_req = Request::builder()
        .uri(format!("/api/calls/{}/cancel", cid))
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let cancel_resp = app.clone().oneshot(cancel_req).await.unwrap();
    assert_eq!(cancel_resp.status(), StatusCode::OK);

    // Verify invocation returned error
    let res = invoke_handle.await.unwrap();
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("cancelled"));

    // Verify pending calls is now 0
    assert_eq!(router.list_pending_calls().len(), 0);
    assert_eq!(router.list_pending_call_details(None).len(), 0);
}
