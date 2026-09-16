use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use at_pc_protocol::messages::ServerToAgentMessage;
use at_pc_protocol::models::{DesktopInputEvent, TerminalInfo};
use at_pc_server::audit::AuditLogger;
use at_pc_server::config::{Role, ServerConfig};
use at_pc_server::mcp::create_mcp_http_router;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::registry::TerminalRegistry;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tokio::sync::mpsc;
use tower::ServiceExt;

static TEST_SEQ: AtomicU64 = AtomicU64::new(0);

struct TempFileGuard(PathBuf);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn unique_audit_path() -> TempFileGuard {
    let path = std::env::temp_dir().join(format!(
        "at-pc-audit-filter-{}-{}.jsonl",
        std::process::id(),
        TEST_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    TempFileGuard(path)
}

fn create_test_terminal(id: &str) -> TerminalInfo {
    TerminalInfo {
        terminal_id: id.to_string(),
        hostname: "test-host".to_string(),
        username: "test-user".to_string(),
        lan_ip: "127.0.0.1".to_string(),
        os_version: "macOS 15.0".to_string(),
        agent_version: "1.0.0".to_string(),
    }
}

#[tokio::test]
async fn test_mouse_move_event_does_not_write_to_audit_logger() {
    let guard = unique_audit_path();
    let audit_path = guard.0.clone();

    let mut roles = HashMap::new();
    roles.insert("admin-token".to_string(), Role::Admin);

    let config = ServerConfig {
        roles,
        audit_log_path: Some(audit_path.clone()),
        ..Default::default()
    };

    let logger = Arc::new(AuditLogger::new(Some(audit_path.clone())));
    let registry = Arc::new(TerminalRegistry::new());
    let (tx, mut rx) = mpsc::unbounded_channel();
    registry.register(create_test_terminal("term-1"), tx).await;

    let router = Arc::new(McpRouter::new(registry).with_audit_logger(Arc::clone(&logger)));
    let app = create_mcp_http_router(router, config);

    // Send 10 high-frequency MouseMove events
    for i in 0..10 {
        let event = DesktopInputEvent::MouseMove {
            x: 100 + i * 10,
            y: 200 + i * 10,
        };
        let req = Request::builder()
            .method("POST")
            .uri("/api/terminals/term-1/desktop/input")
            .header("Authorization", "Bearer admin-token")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_string(&event).unwrap()))
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Terminal should receive the message without failure
        let received = rx.recv().await.expect("terminal should receive DesktopInput");
        match received {
            ServerToAgentMessage::DesktopInput { event: ev } => {
                assert_eq!(ev, event);
            }
            other => panic!("expected DesktopInput, got {:?}", other),
        }
    }

    // Flush audit logger to disk barrier
    logger.flush_async().await.unwrap();

    // Verify 0 audit records were written for MouseMove events
    let recent = logger.read_recent_async(100).await;
    assert_eq!(
        recent.len(),
        0,
        "MouseMove events must NOT generate audit records, found: {:?}",
        recent
    );

    // Verify file on disk has 0 records (empty or non-existent)
    if audit_path.exists() {
        let content = std::fs::read_to_string(&audit_path).unwrap();
        assert!(
            content.trim().is_empty(),
            "audit.jsonl should be empty for MouseMove events, but got:\n{}",
            content
        );
    }
}

#[tokio::test]
async fn test_non_mouse_move_events_write_to_audit_logger() {
    let guard = unique_audit_path();
    let audit_path = guard.0.clone();

    let mut roles = HashMap::new();
    roles.insert("admin-token".to_string(), Role::Admin);

    let config = ServerConfig {
        roles,
        audit_log_path: Some(audit_path.clone()),
        ..Default::default()
    };

    let logger = Arc::new(AuditLogger::new(Some(audit_path.clone())));
    let registry = Arc::new(TerminalRegistry::new());
    let (tx, mut rx) = mpsc::unbounded_channel();
    registry.register(create_test_terminal("term-1"), tx).await;

    let router = Arc::new(McpRouter::new(registry).with_audit_logger(Arc::clone(&logger)));
    let app = create_mcp_http_router(router, config);

    // 1. Send MouseClick
    let click = DesktopInputEvent::MouseClick {
        button: 0,
        count: 1,
    };
    let req = Request::builder()
        .method("POST")
        .uri("/api/terminals/term-1/desktop/input")
        .header("Authorization", "Bearer admin-token")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&click).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = rx.recv().await.unwrap();

    // 2. Send KeyDown / KeyPress
    let key_down = DesktopInputEvent::KeyDown {
        key_code: 13,
        key: "Enter".to_string(),
    };
    let req = Request::builder()
        .method("POST")
        .uri("/api/terminals/term-1/desktop/input")
        .header("Authorization", "Bearer admin-token")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&key_down).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = rx.recv().await.unwrap();

    // 3. Send KeyUp
    let key_up = DesktopInputEvent::KeyUp {
        key_code: 13,
        key: "Enter".to_string(),
    };
    let req = Request::builder()
        .method("POST")
        .uri("/api/terminals/term-1/desktop/input")
        .header("Authorization", "Bearer admin-token")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&key_up).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = rx.recv().await.unwrap();

    // 4. Send MouseMove (which should be skipped)
    let move_ev = DesktopInputEvent::MouseMove { x: 50, y: 50 };
    let req = Request::builder()
        .method("POST")
        .uri("/api/terminals/term-1/desktop/input")
        .header("Authorization", "Bearer admin-token")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&move_ev).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = rx.recv().await.unwrap();

    logger.flush_async().await.unwrap();

    // Verify exactly 3 records exist (MouseClick, KeyDown, KeyUp) and NOT MouseMove
    let records = logger.read_recent_async(100).await;
    assert_eq!(
        records.len(),
        3,
        "Expected 3 audit records (MouseClick, KeyDown, KeyUp), found {}",
        records.len()
    );

    // Records are ordered newest-first in read_recent
    for record in &records {
        assert_eq!(record.action, "api:desktop_input");
        assert_eq!(record.status, "SUCCESS");
        assert_eq!(record.terminal_id.as_deref(), Some("term-1"));
        assert_eq!(record.role.as_deref(), Some("admin"));
    }

    let actions: Vec<String> = records
        .iter()
        .map(|r| {
            let val = r.arguments.as_ref().unwrap();
            val.get("action").unwrap().as_str().unwrap().to_string()
        })
        .collect();

    assert!(
        actions.contains(&"MouseClick".to_string()),
        "Missing MouseClick in audit records"
    );
    assert!(
        actions.contains(&"KeyDown".to_string()),
        "Missing KeyDown in audit records"
    );
    assert!(
        actions.contains(&"KeyUp".to_string()),
        "Missing KeyUp in audit records"
    );
    assert!(
        !actions.contains(&"MouseMove".to_string()),
        "MouseMove MUST NOT be in audit records"
    );
}

#[tokio::test]
async fn test_failed_mouse_move_is_not_audited_but_failed_click_is_audited() {
    let guard = unique_audit_path();
    let audit_path = guard.0.clone();

    let mut roles = HashMap::new();
    roles.insert("admin-token".to_string(), Role::Admin);

    let config = ServerConfig {
        roles,
        audit_log_path: Some(audit_path.clone()),
        ..Default::default()
    };

    let logger = Arc::new(AuditLogger::new(Some(audit_path.clone())));
    let registry = Arc::new(TerminalRegistry::new());
    // NOTE: do not register "term-offline", so send_desktop_input will fail

    let router = Arc::new(McpRouter::new(registry).with_audit_logger(Arc::clone(&logger)));
    let app = create_mcp_http_router(router, config);

    // 1. Send MouseMove to offline terminal -> fails with 500
    let move_ev = DesktopInputEvent::MouseMove { x: 100, y: 100 };
    let req = Request::builder()
        .method("POST")
        .uri("/api/terminals/term-offline/desktop/input")
        .header("Authorization", "Bearer admin-token")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&move_ev).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

    logger.flush_async().await.unwrap();
    let records = logger.read_recent_async(10).await;
    assert_eq!(
        records.len(),
        0,
        "Failed MouseMove must NOT be written to audit log"
    );

    // 2. Send MouseClick to offline terminal -> fails with 500, but SHOULD be logged
    let click = DesktopInputEvent::MouseClick {
        button: 0,
        count: 1,
    };
    let req = Request::builder()
        .method("POST")
        .uri("/api/terminals/term-offline/desktop/input")
        .header("Authorization", "Bearer admin-token")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&click).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

    logger.flush_async().await.unwrap();
    let records = logger.read_recent_async(10).await;
    assert_eq!(
        records.len(),
        1,
        "Failed MouseClick MUST be written to audit log"
    );
    assert_eq!(records[0].status, "FAILED");
    assert_eq!(records[0].action, "api:desktop_input");
}
