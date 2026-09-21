use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use at_pc_protocol::Role;
use at_pc_server::audit::AuditLogger;
use at_pc_server::config::ServerConfig;
use at_pc_server::desktop_ticket::{DesktopTicketStore, TicketError};
use at_pc_server::keystore::{KeyKind, KeyStore};
use at_pc_server::mcp::{create_mcp_http_router_with_state, McpHttpState};
use at_pc_server::rate_limit::{AuthRateLimiter, KeyQuotaTracker};
use at_pc_server::router::McpRouter;
use at_pc_server::session::SessionStore;
use at_pc_server::ws::registry::TerminalRegistry;
use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::Value;
use tower::ServiceExt;

struct TestContext {
    dir: PathBuf,
    app: Router,
    admin_key: String,
    ticket_store: Arc<DesktopTicketStore>,
    registry: Arc<TerminalRegistry>,
}

impl Drop for TestContext {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

static TEST_DIR_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

async fn setup_test_context() -> TestContext {
    let count = TEST_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "atpc-ticket-test-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        count
    ));
    std::fs::create_dir_all(&dir).unwrap();

    let keys_path = dir.join("keys.json");
    let keystore = Arc::new(KeyStore::load_or_create(&keys_path).unwrap());

    let (_, admin_key) = keystore
        .issue(
            "admin".to_string(),
            KeyKind::SecretKey,
            Some(Role::Admin),
            vec!["console".to_string(), "mcp".to_string()],
            None,
            None,
        )
        .unwrap();

    let audit_file = dir.join("audit.jsonl");
    let audit_logger = Arc::new(AuditLogger::new(Some(audit_file.clone())));

    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()).with_audit_logger(audit_logger.clone()));

    let config = ServerConfig {
        listen_host: "127.0.0.1".to_string(),
        keys_file_path: Some(keys_path),
        audit_log_path: Some(audit_file.clone()),
        ..Default::default()
    };

    let session_store = Arc::new(SessionStore::default());
    let quota_tracker = Arc::new(KeyQuotaTracker::new(120, Duration::from_secs(60)));
    let rate_limiter = Arc::new(AuthRateLimiter::default());
    let ticket_store = Arc::new(DesktopTicketStore::default());

    let state = McpHttpState {
        router,
        config,
        log_file_path: dir.join("server.log"),
        audit_logger: Some(audit_logger),
        keystore: Some(keystore),
        rate_limiter,
        session_store,
        quota_tracker,
        ticket_store: ticket_store.clone(),
    };

    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let app = create_mcp_http_router_with_state(state, cors);

    TestContext {
        dir,
        app,
        admin_key,
        ticket_store,
        registry,
    }
}

#[tokio::test]
async fn test_desktop_ticket_issue_and_consume_lifecycle() {
    let ctx = setup_test_context().await;
    let peer: SocketAddr = "127.0.0.1:54321".parse().unwrap();

    // 1. Issue ticket for non-existent terminal -> 404
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/terminals/non-existent/desktop/ticket")
        .header(header::AUTHORIZATION, format!("Bearer {}", ctx.admin_key))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 2. Register a mock terminal
    let term_info = at_pc_protocol::models::TerminalInfo {
        terminal_id: "term-mock-1".to_string(),
        hostname: "MOCK-PC".to_string(),
        username: "user".to_string(),
        lan_ip: "192.168.1.100".to_string(),
        os_version: "Windows 11".to_string(),
        agent_version: "1.0.0".to_string(),
    };
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    ctx.registry.register(term_info, tx).await;

    // 3. Issue ticket for registered terminal -> 200 OK with ticket
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/terminals/term-mock-1/desktop/ticket")
        .header(header::AUTHORIZATION, format!("Bearer {}", ctx.admin_key))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let ticket = body["ticket"].as_str().expect("ticket string present");
    assert!(ticket.starts_with("rdt_"));
    assert_eq!(body["expires_in"], 30);
    assert_eq!(body["ws_path"], "/api/terminals/term-mock-1/desktop/ws");

    // 4. Consume ticket from ticket_store -> succeeds once, yields Role::Admin
    let role = ctx.ticket_store.consume(ticket, "term-mock-1").unwrap();
    assert_eq!(role, Role::Admin);

    // 5. Single-use: consuming again must return NotFound
    assert_eq!(
        ctx.ticket_store.consume(ticket, "term-mock-1"),
        Err(TicketError::NotFound)
    );
}

#[tokio::test]
async fn test_desktop_ws_endpoint_rejects_missing_ticket() {
    let ctx = setup_test_context().await;
    let peer: SocketAddr = "127.0.0.1:54321".parse().unwrap();

    // 1. Missing ticket query -> 401 UNAUTHORIZED
    let mut req = Request::builder()
        .method(Method::GET)
        .uri("/api/terminals/term-mock-1/desktop/ws")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 2. Bogus ticket query -> 403 FORBIDDEN
    let mut req = Request::builder()
        .method(Method::GET)
        .uri("/api/terminals/term-mock-1/desktop/ws?ticket=rdt_nonexistent")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
