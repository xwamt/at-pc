use at_pc::server::{
    router::create_mcp_router,
    start_server,
    state::{AppState, AuditLogEntry},
};
use at_pc::tools::get_mcp_tool_definitions;
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::test]
async fn test_mcp_tools_list_and_auth() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .try_init();

    let state = Arc::new(AppState::new("9999".to_string(), 9811));
    let app = create_mcp_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::builder().no_proxy().build().unwrap();

    // 1. Unauthorized request (no header)
    let resp = client
        .post(format!("http://{}/messages", addr))
        .json(&serde_json::json!({"jsonrpc": "2.0", "method": "tools/list", "id": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // 2. Unauthorized request (wrong PIN)
    let resp = client
        .post(format!("http://{}/messages", addr))
        .header("Authorization", "Bearer 0000")
        .json(&serde_json::json!({"jsonrpc": "2.0", "method": "tools/list", "id": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // 3. Authorized request (Authorization header)
    let resp = client
        .post(format!("http://{}/messages", addr))
        .header("Authorization", "Bearer 9999")
        .json(&serde_json::json!({"jsonrpc": "2.0", "method": "tools/list", "id": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], 1);

    let tools = body["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 9);
    assert_eq!(tools.len(), get_mcp_tool_definitions().len());

    let tool_names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();

    assert!(tool_names.contains(&"get_system_overview"));
    assert!(tool_names.contains(&"exec_powershell"));
    assert!(tool_names.contains(&"exec_cmd"));
    assert!(tool_names.contains(&"list_processes"));
    assert!(tool_names.contains(&"kill_process"));
    assert!(tool_names.contains(&"manage_service"));
    assert!(tool_names.contains(&"read_text_file"));
    assert!(tool_names.contains(&"write_text_file"));
    assert!(tool_names.contains(&"capture_screen"));

    // 4. Authorized request with query param (?token=9999)
    let resp_query = client
        .post(format!("http://{}/messages?token=9999", addr))
        .json(&serde_json::json!({"jsonrpc": "2.0", "method": "tools/list", "id": 2}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_query.status(), 200);
}

#[tokio::test]
async fn test_mcp_initialize_and_ping() {
    let state = Arc::new(AppState::new("1234".to_string(), 9812));
    let app = create_mcp_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::builder().no_proxy().build().unwrap();

    // Initialize
    let resp = client
        .post(format!("http://{}/messages", addr))
        .header("Authorization", "Bearer 1234")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0.0"}
            },
            "id": 1
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["result"]["serverInfo"]["name"], "at-pc");
    assert!(body["result"]["capabilities"]["tools"].is_object());

    // Ping
    let resp_ping = client
        .post(format!("http://{}/messages", addr))
        .header("Authorization", "Bearer 1234")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "ping",
            "id": 2
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_ping.status(), 200);
    let ping_body: serde_json::Value = resp_ping.json().await.unwrap();
    assert_eq!(ping_body["id"], 2);
    assert_eq!(ping_body["result"], serde_json::json!({}));
}

#[tokio::test]
async fn test_mcp_tools_call_and_audit_log() {
    let state = Arc::new(AppState::new("4321".to_string(), 9813));
    let mut audit_rx = state.audit_sender.subscribe();
    let app = create_mcp_router(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::builder().no_proxy().build().unwrap();

    // Call get_system_overview
    let resp = client
        .post(format!("http://{}/messages", addr))
        .header("Authorization", "Bearer 4321")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "get_system_overview",
                "arguments": {}
            },
            "id": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["id"], 10);
    assert_eq!(body["result"]["isError"], false);
    let content_arr = body["result"]["content"].as_array().expect("content array");
    assert!(!content_arr.is_empty());
    assert_eq!(content_arr[0]["type"], "text");

    // Verify audit log entry was broadcast
    let audit_entry: AuditLogEntry = audit_rx.recv().await.unwrap();
    assert_eq!(audit_entry.tool_name, "get_system_overview");
    assert_eq!(audit_entry.status, "success");

    // Call exec_cmd
    let resp_cmd = client
        .post(format!("http://{}/messages", addr))
        .header("Authorization", "Bearer 4321")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "exec_cmd",
                "arguments": {
                    "command": "echo test_audit_mcp"
                }
            },
            "id": 11
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_cmd.status(), 200);

    let audit_entry_cmd: AuditLogEntry = audit_rx.recv().await.unwrap();
    assert_eq!(audit_entry_cmd.tool_name, "exec_cmd");
    assert_eq!(audit_entry_cmd.status, "success");

    // Call nonexistent tool
    let resp_invalid = client
        .post(format!("http://{}/messages", addr))
        .header("Authorization", "Bearer 4321")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "non_existent_tool_xyz",
                "arguments": {}
            },
            "id": 12
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_invalid.status(), 200);
    let invalid_body: serde_json::Value = resp_invalid.json().await.unwrap();
    assert_eq!(invalid_body["result"]["isError"], true);

    let audit_entry_err: AuditLogEntry = audit_rx.recv().await.unwrap();
    assert_eq!(audit_entry_err.tool_name, "non_existent_tool_xyz");
    assert_eq!(audit_entry_err.status, "error");
}

#[tokio::test]
async fn test_mcp_sse_transport_and_connection_tracking() {
    let state = Arc::new(AppState::new("5555".to_string(), 9814));
    let app = create_mcp_router(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::builder().no_proxy().build().unwrap();

    // 1. Unauthorized SSE connection
    let resp = client
        .get(format!("http://{}/sse", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // 2. Authorized SSE connection
    let mut resp_sse = client
        .get(format!("http://{}/sse", addr))
        .header("Authorization", "Bearer 5555")
        .send()
        .await
        .unwrap();
    assert_eq!(resp_sse.status(), 200);
    assert_eq!(
        resp_sse.headers().get("content-type").unwrap().to_str().unwrap(),
        "text/event-stream"
    );

    // Verify endpoint event is received in stream
    let chunk = resp_sse.chunk().await.unwrap().expect("first SSE chunk");
    let text = String::from_utf8_lossy(&chunk);
    assert!(text.contains("event: endpoint"));
    assert!(text.contains("/messages"));
}

#[tokio::test]
async fn test_server_lifecycle_and_shutdown() {
    let state = Arc::new(AppState::new("7777".to_string(), 0));
    // Find an available port
    let test_port = at_pc::utils::network::find_available_port(19800);
    let handle = start_server(state.clone(), test_port).await;

    // Give server a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let resp = client
        .post(format!("http://127.0.0.1:{}/messages", test_port))
        .header("Authorization", "Bearer 7777")
        .json(&serde_json::json!({"jsonrpc": "2.0", "method": "ping", "id": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Trigger shutdown
    state.trigger_shutdown();

    // Wait for server task to finish
    tokio::time::timeout(tokio::time::Duration::from_secs(3), handle)
        .await
        .expect("Server should shut down gracefully within 3 seconds")
        .unwrap();
}
