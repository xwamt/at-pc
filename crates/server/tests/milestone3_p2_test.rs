//! Integration tests for Milestone 3 (v1.0.0):
//! [P2-1] Computer-Use MCP tools and permission toggle
//! [P2-2] Native diagnostics (manage_service, get_event_logs)
//! [P2-3] WSS/TLS transport encryption end-to-end
//! [P2-4] Role-Based Access Control (RBAC) tiers and immutable JSONL audit logging

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::input::{key_name_to_code, try_key_name_to_code};
use at_pc_agent::ws_client::AgentWsClient;
use at_pc_protocol::models::TerminalInfo;
use at_pc_server::audit::AuditLogger;
use at_pc_server::config::{Role, ServerConfig};
use at_pc_server::router::McpRouter;
use at_pc_server::ws::{start_ws_server_with_listener, TerminalRegistry, WsServerState};

// ============================================================================
// [P2-1] Computer-Use MCP Tools & Permission Toggle
// ============================================================================

#[tokio::test]
async fn test_p2_1_computer_use_permission_toggle() {
    // 1. Default executor has computer_use = false
    let disabled_executor = AgentExecutor::default();
    assert!(!disabled_executor.enable_computer_use);

    let tools = [
        ("mouse_click", json!({"x": 100, "y": 100})),
        ("mouse_move", json!({"x": 200, "y": 200})),
        ("mouse_drag", json!({"to_x": 300, "to_y": 300})),
        ("mouse_scroll", json!({"delta_y": -5})),
        ("type_text", json!({"text": "Hello"})),
        ("press_key", json!({"key": "enter"})),
        ("key_down", json!({"key": "shift"})),
        ("key_up", json!({"key": "shift"})),
        ("hotkey", json!({"keys": ["ctrl", "c"]})),
    ];

    for (tool_name, args) in tools {
        let err = disabled_executor
            .execute(tool_name, args)
            .await
            .unwrap_err();
        assert!(
            err.to_lowercase().contains("computer-use") && err.to_lowercase().contains("disabled"),
            "Expected disabled message for '{}', got: '{}'",
            tool_name,
            err
        );
    }

    // 2. Enabled executor allows invocation of all 9 tools
    let enabled_executor = AgentExecutor::default().with_computer_use(true);
    assert!(enabled_executor.enable_computer_use);

    // 2a. mouse_move
    let res = enabled_executor.execute("mouse_move", json!({"x": 50, "y": 50})).await;
    assert!(res.is_ok(), "mouse_move failed: {:?}", res);

    // 2b. mouse_click
    let res = enabled_executor.execute("mouse_click", json!({"x": 50, "y": 50, "button": "left", "count": 1})).await;
    assert!(res.is_ok(), "mouse_click failed: {:?}", res);

    // 2c. mouse_drag with end_x / end_y and alias to_x / to_y
    let res = enabled_executor.execute("mouse_drag", json!({"start_x": 10, "start_y": 10, "end_x": 100, "end_y": 100})).await;
    assert!(res.is_ok(), "mouse_drag with end_x failed: {:?}", res);
    let res = enabled_executor.execute("mouse_drag", json!({"from_x": 10, "from_y": 10, "to_x": 100, "to_y": 100})).await;
    assert!(res.is_ok(), "mouse_drag with to_x failed: {:?}", res);

    // 2d. mouse_scroll
    let res = enabled_executor.execute("mouse_scroll", json!({"delta_y": -3})).await;
    assert!(res.is_ok(), "mouse_scroll failed: {:?}", res);

    // 2e. type_text
    let res = enabled_executor.execute("type_text", json!({"text": "test input"})).await;
    assert!(res.is_ok(), "type_text failed: {:?}", res);

    // 2f. press_key
    let res = enabled_executor.execute("press_key", json!({"key": "enter"})).await;
    assert!(res.is_ok(), "press_key failed: {:?}", res);

    // 2g. key_down & key_up
    let res = enabled_executor.execute("key_down", json!({"key": "shift"})).await;
    assert!(res.is_ok(), "key_down failed: {:?}", res);
    let res = enabled_executor.execute("key_up", json!({"key": "shift"})).await;
    assert!(res.is_ok(), "key_up failed: {:?}", res);

    // 2h. hotkey
    let res = enabled_executor.execute("hotkey", json!({"keys": ["ctrl", "c"]})).await;
    assert!(res.is_ok(), "hotkey failed: {:?}", res);

    // 3. Enabled executor rejects invalid keys with explicit error
    let err = enabled_executor.execute("press_key", json!({"key": "nonexistent_fake_key"})).await.unwrap_err();
    assert!(err.contains("Unknown or unsupported key"), "Expected key error, got: {}", err);

    let err = enabled_executor.execute("hotkey", json!({"keys": ["ctrl", "fake_key_in_hotkey"]})).await.unwrap_err();
    assert!(err.contains("Unknown or unsupported key"), "Expected hotkey key error, got: {}", err);
}

#[test]
fn test_p2_1_key_mapping_resolution() {
    assert!(try_key_name_to_code("enter").is_some());
    assert!(try_key_name_to_code("Return").is_some());
    assert!(try_key_name_to_code("tab").is_some());
    assert!(try_key_name_to_code("space").is_some());
    assert!(try_key_name_to_code("backspace").is_some());
    assert!(try_key_name_to_code("escape").is_some());
    assert!(try_key_name_to_code("esc").is_some());
    assert!(try_key_name_to_code("ctrl").is_some());
    assert!(try_key_name_to_code("alt").is_some());
    assert!(try_key_name_to_code("shift").is_some());
    assert!(try_key_name_to_code("f1").is_some());
    assert!(try_key_name_to_code("f12").is_some());
    assert!(try_key_name_to_code("a").is_some());
    assert!(try_key_name_to_code("z").is_some());
    assert!(try_key_name_to_code("0").is_some());
    assert!(try_key_name_to_code(".").is_some());
    assert!(try_key_name_to_code(",").is_some());
    assert!(try_key_name_to_code("-").is_some());
    assert!(try_key_name_to_code("insert").is_some());

    assert!(try_key_name_to_code("nonexistent_dummy_key").is_none());
    assert_eq!(key_name_to_code("nonexistent_dummy_key"), 0);
}

// ============================================================================
// [P2-2] Native Diagnostics (manage_service, get_event_logs)
// ============================================================================

#[tokio::test]
async fn test_p2_2_service_and_event_logs_diagnostics() {
    let executor = AgentExecutor::default();

    // 1. Test manage_service (status of a service)
    let s_res = executor
        .execute("manage_service", json!({"service_name": "test_service", "action": "status"}))
        .await;
    assert!(s_res.is_ok(), "manage_service failed: {:?}", s_res);
    let s_val = s_res.unwrap();
    assert!(
        s_val.get("name").is_some() || s_val.get("status").is_some(),
        "Unexpected manage_service response structure: {:?}",
        s_val
    );

    // 2. Test get_event_logs
    let e_res = executor
        .execute("get_event_logs", json!({"log_name": "Application", "limit": 5}))
        .await;
    assert!(e_res.is_ok(), "get_event_logs failed: {:?}", e_res);
    let e_val = e_res.unwrap();
    assert!(
        e_val.get("events").is_some() || e_val.get("total_events").is_some(),
        "Unexpected get_event_logs response structure: {:?}",
        e_val
    );
}

// ============================================================================
// [P2-3] Full-Chain WSS/TLS Transport Encryption
// ============================================================================

#[tokio::test]
async fn test_p2_3_wss_tls_transport_encryption_e2e() {
    let temp_dir = std::env::temp_dir().join(format!("at_pc_tls_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    // 1. Generate self-signed certificate using rcgen
    let subject_alt_names = vec!["127.0.0.1".to_string(), "localhost".to_string()];
    let cert = rcgen::generate_simple_self_signed(subject_alt_names).unwrap();
    let cert_pem = cert.cert.pem();
    let key_pem = cert.key_pair.serialize_pem();

    let cert_path = temp_dir.join("server.crt");
    let key_path = temp_dir.join("server.key");
    std::fs::write(&cert_path, cert_pem).unwrap();
    std::fs::write(&key_path, key_pem).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_port = listener.local_addr().unwrap().port();
    let config = ServerConfig {
        ws_port,
        tls_cert_path: Some(cert_path.clone()),
        tls_key_path: Some(key_path.clone()),
        ..Default::default()
    };

    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let ws_state = WsServerState::with_handler(registry.clone(), config.clone(), router.clone());

    // 2. Start WSS server
    tokio::spawn(async move {
        let _ = start_ws_server_with_listener(ws_state, listener).await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    // 3. Connect AgentWsClient using wss:// and insecure_skip_verify
    let terminal_id = format!("tls-term-{}", std::process::id());
    let info = TerminalInfo {
        terminal_id: terminal_id.clone(),
        hostname: "tls-agent-host".to_string(),
        username: "testuser".to_string(),
        lan_ip: "127.0.0.1".to_string(),
        os_version: "test-os".to_string(),
        agent_version: "1.0.0".to_string(),
    };

    let executor = Arc::new(AgentExecutor::default());
    let agent_url = format!("wss://127.0.0.1:{}/ws", ws_port);
    let ws_client = Arc::new(
        AgentWsClient::new(agent_url, info, executor)
            .with_tls_config(None, None, None, true) // insecure_skip_verify = true for self-signed
            .with_reconnect_interval(1),
    );

    let client_run = ws_client.clone();
    let client_task = tokio::spawn(async move {
        client_run.run().await;
    });

    // 4. Wait for agent to register over encrypted TLS
    let mut registered = false;
    for _ in 0..50 {
        if registry.get_terminal(&terminal_id).await.is_some() {
            registered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(registered, "Agent failed to register over WSS/TLS within 5 seconds");

    // 5. Dispatch a tool invocation over TLS and verify execution
    let tool_res = router
        .invoke_tool(&terminal_id, "get_system_overview", json!({}), 10)
        .await;
    assert!(
        tool_res.is_ok(),
        "Tool invocation over TLS failed: {:?}",
        tool_res
    );
    let val = tool_res.unwrap();
    assert!(val.get("host_name").is_some() || val.get("os_name").is_some(), "Unexpected get_system_overview payload: {:?}", val);

    // Clean up
    client_task.abort();
    let _ = ws_client.disconnect("Test completed").await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_p2_3_wss_tls_untrusted_cert_rejected() {
    let temp_dir = std::env::temp_dir().join(format!("at_pc_tls_untrusted_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    // 1. Generate self-signed certificate
    let subject_alt_names = vec!["127.0.0.1".to_string(), "localhost".to_string()];
    let cert = rcgen::generate_simple_self_signed(subject_alt_names).unwrap();
    let cert_pem = cert.cert.pem();
    let key_pem = cert.key_pair.serialize_pem();

    let cert_path = temp_dir.join("untrusted.crt");
    let key_path = temp_dir.join("untrusted.key");
    std::fs::write(&cert_path, cert_pem).unwrap();
    std::fs::write(&key_path, key_pem).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_port = listener.local_addr().unwrap().port();
    let config = ServerConfig {
        ws_port,
        tls_cert_path: Some(cert_path.clone()),
        tls_key_path: Some(key_path.clone()),
        ..Default::default()
    };

    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let ws_state = WsServerState::with_handler(registry.clone(), config.clone(), router.clone());

    tokio::spawn(async move {
        let _ = start_ws_server_with_listener(ws_state, listener).await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    // 2. Connect Agent with insecure_skip_verify = false (and no CA)
    let terminal_id = format!("tls-untrusted-term-{}", std::process::id());
    let info = TerminalInfo {
        terminal_id: terminal_id.clone(),
        hostname: "untrusted-agent-host".to_string(),
        username: "testuser".to_string(),
        lan_ip: "127.0.0.1".to_string(),
        os_version: "test-os".to_string(),
        agent_version: "1.0.0".to_string(),
    };

    let executor = Arc::new(AgentExecutor::default());
    let agent_url = format!("wss://127.0.0.1:{}/ws", ws_port);
    let ws_client = Arc::new(
        AgentWsClient::new(agent_url, info, executor)
            .with_tls_config(None, None, None, false) // STRICT certificate verification!
            .with_reconnect_interval(1),
    );

    let client_run = ws_client.clone();
    let client_task = tokio::spawn(async move {
        client_run.run().await;
    });

    // 3. Confirm that agent NEVER registers because self-signed certificate is untrusted
    let mut was_registered = false;
    for _ in 0..12 {
        if registry.get_terminal(&terminal_id).await.is_some() {
            was_registered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(!was_registered, "Agent with insecure_skip_verify=false should NEVER accept self-signed cert");

    client_task.abort();
    let _ = ws_client.disconnect("Test completed").await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}

// ============================================================================
// [P2-4] Role-Based Access Control (RBAC) & Audit Logging
// ============================================================================

#[tokio::test]
async fn test_p2_4_rbac_router_permission_enforcement() {
    let registry = Arc::new(TerminalRegistry::new());
    let temp_audit = std::env::temp_dir().join(format!("test_audit_{}.jsonl", std::process::id()));
    let audit_logger = Arc::new(AuditLogger::new(Some(temp_audit.clone())));
    let router = McpRouter::new(registry).with_audit_logger(audit_logger.clone());

    // 1. Viewer Role permissions
    // Viewer CAN list terminals
    let res = router
        .dispatch_tool_call_with_role(
            "list_terminals",
            json!({}),
            None,
            Some(Role::Viewer),
            Some("127.0.0.1"),
            Some("view***"),
        )
        .await;
    assert!(res.is_ok(), "Viewer should be allowed to list_terminals");

    // Viewer CANNOT kill_process
    let res = router
        .dispatch_tool_call_with_role(
            "kill_process",
            json!({"pid": 1234}),
            None,
            Some(Role::Viewer),
            Some("127.0.0.1"),
            Some("view***"),
        )
        .await;
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("Forbidden") && err.contains("viewer"), "Got error: {}", err);

    // Viewer CANNOT exec_powershell
    let res = router
        .dispatch_tool_call_with_role(
            "exec_powershell",
            json!({"command": "Get-Process"}),
            None,
            Some(Role::Viewer),
            Some("127.0.0.1"),
            Some("view***"),
        )
        .await;
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Forbidden"));

    // Viewer CANNOT mouse_click
    let res = router
        .dispatch_tool_call_with_role(
            "mouse_click",
            json!({"x": 10, "y": 20}),
            None,
            Some(Role::Viewer),
            Some("127.0.0.1"),
            Some("view***"),
        )
        .await;
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Forbidden"));

    // 2. Operator Role permissions
    // Operator CANNOT exec_powershell
    let res = router
        .dispatch_tool_call_with_role(
            "exec_powershell",
            json!({"command": "Get-Process"}),
            None,
            Some(Role::Operator),
            Some("127.0.0.1"),
            Some("oper***"),
        )
        .await;
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Forbidden"));

    // Operator CANNOT kill_process
    let res = router
        .dispatch_tool_call_with_role(
            "kill_process",
            json!({"pid": 9999}),
            None,
            Some(Role::Operator),
            Some("127.0.0.1"),
            Some("oper***"),
        )
        .await;
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Forbidden"));

    // 3. Admin Role permissions
    // Admin passes RBAC check for all tools
    let res = router
        .dispatch_tool_call_with_role(
            "kill_process",
            json!({"terminal_id": "nonexistent", "pid": 123}),
            None,
            Some(Role::Admin),
            Some("127.0.0.1"),
            Some("adm***"),
        )
        .await;
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(!err.contains("Forbidden"), "Admin should pass RBAC, got: {}", err);

    // 4. Verify persistent audit log records
    let recent = audit_logger.read_recent(10);
    assert!(!recent.is_empty(), "Audit log should contain recorded entries");
    let denied_records: Vec<_> = recent.iter().filter(|r| r.status == "DENIED").collect();
    assert!(
        !denied_records.is_empty(),
        "Audit log should have recorded DENIED authorization events"
    );
    assert_eq!(denied_records[0].role, Some("viewer".to_string()));

    let _ = std::fs::remove_file(&temp_audit);
}

#[tokio::test]
async fn test_p2_4_dashboard_rbac_and_audit_api() {
    let mut roles = HashMap::new();
    roles.insert("viewer-token".to_string(), Role::Viewer);
    roles.insert("operator-token".to_string(), Role::Operator);
    roles.insert("admin-token".to_string(), Role::Admin);

    let temp_audit = std::env::temp_dir().join(format!("dash_audit_{}.jsonl", std::process::id()));
    let config = ServerConfig {
        roles,
        audit_log_path: Some(temp_audit.clone()),
        ..Default::default()
    };

    let audit_logger = Arc::new(AuditLogger::new(Some(temp_audit.clone())));
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry).with_audit_logger(audit_logger.clone()));

    let app = at_pc_server::mcp::create_mcp_http_router(router, config);

    // 1. Viewer cannot delete terminal
    let req = Request::builder()
        .method("DELETE")
        .uri("/api/terminals/t1")
        .header("Authorization", "Bearer viewer-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 2. Viewer cannot send desktop input
    let req = Request::builder()
        .method("POST")
        .uri("/api/terminals/t1/desktop/input")
        .header("Authorization", "Bearer viewer-token")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&at_pc_protocol::models::DesktopInputEvent::MouseMove { x: 10, y: 10 }).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 3. Viewer cannot cancel calls
    let req = Request::builder()
        .method("POST")
        .uri("/api/calls/call-1/cancel")
        .header("Authorization", "Bearer viewer-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 4. Viewer cannot view audit trail
    let req = Request::builder()
        .method("GET")
        .uri("/api/audit")
        .header("Authorization", "Bearer viewer-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 5. Admin can view audit trail
    let req = Request::builder()
        .method("GET")
        .uri("/api/audit")
        .header("Authorization", "Bearer admin-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 6. Verify dashboard operations generated audit records
    let logs = audit_logger.read_recent(10);
    assert!(!logs.is_empty(), "Dashboard operations should generate audit logs");
    let delete_denied = logs.iter().find(|l| l.action == "api:delete_terminal");
    assert!(delete_denied.is_some(), "Expected api:delete_terminal audit entry");
    let dd = delete_denied.unwrap();
    assert_eq!(dd.status, "DENIED");
    assert_eq!(dd.role.as_deref(), Some("viewer"));
    assert_eq!(dd.token_prefix.as_deref(), Some("vie***"));

    let desktop_denied = logs.iter().find(|l| l.action == "api:desktop_input");
    assert!(desktop_denied.is_some(), "Expected api:desktop_input audit entry");

    let _ = std::fs::remove_file(&temp_audit);
}
