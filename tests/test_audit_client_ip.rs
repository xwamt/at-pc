use at_pc::server::router::create_mcp_router;
use at_pc::server::state::{AppState, AuditLogStatus};
use std::sync::Arc;
use tokio::net::TcpListener;

#[test]
fn test_audit_log_status_serialization_and_deserialization() {
    // Uppercase serialization
    assert_eq!(
        serde_json::to_string(&AuditLogStatus::Success).unwrap(),
        "\"SUCCESS\""
    );
    assert_eq!(
        serde_json::to_string(&AuditLogStatus::Failed).unwrap(),
        "\"FAILED\""
    );
    assert_eq!(
        serde_json::to_string(&AuditLogStatus::Error).unwrap(),
        "\"ERROR\""
    );
    assert_eq!(
        serde_json::to_string(&AuditLogStatus::Stopped).unwrap(),
        "\"STOPPED\""
    );
    assert_eq!(
        serde_json::to_string(&AuditLogStatus::Started).unwrap(),
        "\"STARTED\""
    );

    // Case-insensitive deserialization
    assert_eq!(
        serde_json::from_str::<AuditLogStatus>("\"success\"").unwrap(),
        AuditLogStatus::Success
    );
    assert_eq!(
        serde_json::from_str::<AuditLogStatus>("\"SUCCESS\"").unwrap(),
        AuditLogStatus::Success
    );
    assert_eq!(
        serde_json::from_str::<AuditLogStatus>("\"failed\"").unwrap(),
        AuditLogStatus::Failed
    );
    assert_eq!(
        serde_json::from_str::<AuditLogStatus>("\"FAILED\"").unwrap(),
        AuditLogStatus::Failed
    );
    assert_eq!(
        serde_json::from_str::<AuditLogStatus>("\"error\"").unwrap(),
        AuditLogStatus::Error
    );
    assert_eq!(
        serde_json::from_str::<AuditLogStatus>("\"ERROR\"").unwrap(),
        AuditLogStatus::Error
    );
    assert_eq!(
        serde_json::from_str::<AuditLogStatus>("\"stopped\"").unwrap(),
        AuditLogStatus::Stopped
    );
    assert_eq!(
        serde_json::from_str::<AuditLogStatus>("\"STOPPED\"").unwrap(),
        AuditLogStatus::Stopped
    );
    assert_eq!(
        serde_json::from_str::<AuditLogStatus>("\"started\"").unwrap(),
        AuditLogStatus::Started
    );
    assert_eq!(
        serde_json::from_str::<AuditLogStatus>("\"STARTED\"").unwrap(),
        AuditLogStatus::Started
    );
}

#[tokio::test]
async fn test_sse_connect_broadcasts_agent_connected_with_client_ip() {
    let state = Arc::new(AppState::new("8888".to_string(), 9820));
    let mut rx = state.subscribe_audit();
    let app = create_mcp_router(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let client = reqwest::Client::builder().no_proxy().build().unwrap();

    let resp = client
        .get(format!("http://{}/sse", addr))
        .header("Authorization", "Bearer 8888")
        .header("X-Forwarded-For", "192.168.1.188")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let event = tokio::time::timeout(tokio::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("should receive connect event")
        .unwrap();

    assert_eq!(event.tool_name, "agent_connected");
    assert_eq!(event.status, AuditLogStatus::Success);
    assert_eq!(event.client_ip.as_deref(), Some("192.168.1.188"));
    assert!(event
        .message
        .as_ref()
        .unwrap()
        .contains("工程师 Agent 已连接 (IP: 192.168.1.188)"));
}

#[tokio::test]
async fn test_tools_call_captures_client_ip_and_uppercase_status() {
    let state = Arc::new(AppState::new("8888".to_string(), 9821));
    let mut rx = state.subscribe_audit();
    let app = create_mcp_router(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let client = reqwest::Client::builder().no_proxy().build().unwrap();

    // 1. Successful tool call with X-Forwarded-For
    let resp = client
        .post(format!("http://{}/messages", addr))
        .header("Authorization", "Bearer 8888")
        .header("X-Forwarded-For", "10.0.0.55, 10.0.0.1")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "get_system_overview",
                "arguments": {}
            },
            "id": 100
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let event_ok = tokio::time::timeout(tokio::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("should receive success audit log")
        .unwrap();

    assert_eq!(event_ok.tool_name, "get_system_overview");
    assert_eq!(event_ok.status, AuditLogStatus::Success);
    assert_eq!(event_ok.status.as_str(), "SUCCESS");
    assert_eq!(event_ok.client_ip.as_deref(), Some("10.0.0.55"));

    // 2. Failed tool call with X-Real-IP
    let resp_err = client
        .post(format!("http://{}/messages", addr))
        .header("Authorization", "Bearer 8888")
        .header("X-Real-IP", "172.16.0.99")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "non_existent_tool_test",
                "arguments": {}
            },
            "id": 101
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_err.status(), 200);

    let event_err = tokio::time::timeout(tokio::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("should receive error audit log")
        .unwrap();

    assert_eq!(event_err.tool_name, "non_existent_tool_test");
    assert_eq!(event_err.status, AuditLogStatus::Error);
    assert_eq!(event_err.status.as_str(), "ERROR");
    assert_eq!(event_err.client_ip.as_deref(), Some("172.16.0.99"));

    // 3. Tool call without proxy headers fallback to TCP socket IP (127.0.0.1)
    let resp_direct = client
        .post(format!("http://{}/messages", addr))
        .header("Authorization", "Bearer 8888")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "get_system_overview",
                "arguments": {}
            },
            "id": 102
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_direct.status(), 200);

    let event_direct = tokio::time::timeout(tokio::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("should receive direct socket audit log")
        .unwrap();

    assert_eq!(event_direct.tool_name, "get_system_overview");
    assert_eq!(event_direct.status, AuditLogStatus::Success);
    assert_eq!(event_direct.client_ip.as_deref(), Some("127.0.0.1"));
}
