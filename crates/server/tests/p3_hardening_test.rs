use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use at_pc_protocol::Role;
use at_pc_server::audit::{AuditLogger, AuditRecord, ACTION_INSECURE_MODE};
use at_pc_server::config::{
    validate_boot_gate, validate_legacy_config_gate, BootGateError, ServerConfig,
};
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
use serde_json::{json, Value};
use tower::ServiceExt;

struct TestContext {
    dir: PathBuf,
    app: Router,
    admin_key: String,
    _admin_id: String,
    _audit_file: PathBuf,
    _keystore: Arc<KeyStore>,
}

impl Drop for TestContext {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

static TEST_DIR_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

async fn setup_test_context(quota_limit: u32) -> TestContext {
    let count = TEST_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "atpc-p3-test-{}-{}-{}",
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

    // Issue initial root admin key
    let (admin_record, admin_key) = keystore
        .issue(
            "p3-admin".to_string(),
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
    let router = Arc::new(McpRouter::new(registry).with_audit_logger(audit_logger.clone()));

    let config = ServerConfig {
        listen_host: "127.0.0.1".to_string(),
        keys_file_path: Some(keys_path),
        audit_log_path: Some(audit_file.clone()),
        ..Default::default()
    };

    let session_store = Arc::new(SessionStore::default());
    let quota_tracker = Arc::new(KeyQuotaTracker::new(quota_limit, Duration::from_secs(60)));
    let rate_limiter = Arc::new(AuthRateLimiter::default());

    let state = McpHttpState {
        router,
        config,
        log_file_path: dir.join("server.log"),
        audit_logger: Some(audit_logger),
        keystore: Some(keystore.clone()),
        rate_limiter,
        session_store,
        quota_tracker,
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
        _admin_id: admin_record.id,
        _audit_file: audit_file,
        _keystore: keystore,
    }
}

// ---------------------------------------------------------------------------
// G3-1: Non-loopback without TLS rejected, --allow-plaintext override & audit
// ---------------------------------------------------------------------------

#[test]
fn test_g3_1_tls_gate_and_allow_plaintext_override() {
    // 1. Non-loopback without TLS must fail boot gate
    let mut config = ServerConfig {
        listen_host: "0.0.0.0".to_string(),
        allow_plaintext: false,
        ..Default::default()
    };
    let res = validate_boot_gate(&config, true, false);
    assert_eq!(
        res,
        Err(BootGateError::TlsRequired {
            host: "0.0.0.0".to_string()
        })
    );

    // 2. Loopback without TLS succeeds
    config.listen_host = "127.0.0.1".to_string();
    assert_eq!(validate_boot_gate(&config, true, false), Ok(()));

    // 3. Non-loopback with TLS certificates configured succeeds
    config.listen_host = "0.0.0.0".to_string();
    config.tls_cert_path = Some(PathBuf::from("/etc/ssl/cert.pem"));
    config.tls_key_path = Some(PathBuf::from("/etc/ssl/key.pem"));
    assert_eq!(validate_boot_gate(&config, true, false), Ok(()));

    // 4. Non-loopback with allow_plaintext override succeeds
    let plain_cfg = ServerConfig {
        listen_host: "192.168.1.100".to_string(),
        allow_plaintext: true,
        ..Default::default()
    };
    assert_eq!(validate_boot_gate(&plain_cfg, true, false), Ok(()));

    // 5. Insecure mode audit logging
    let dir = std::env::temp_dir().join(format!(
        "atpc-g3-audit-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let audit_file = dir.join("audit.jsonl");
    let audit = AuditLogger::new(Some(audit_file.clone()));

    if plain_cfg.allow_plaintext && !at_pc_server::config::is_loopback(&plain_cfg.listen_host) {
        audit.log(AuditRecord {
            id: format!("audit-{}", at_pc_server::router::generate_call_id()),
            timestamp: chrono::Utc::now().to_rfc3339(),
            role: None,
            token_prefix: None,
            client_ip: None,
            terminal_id: None,
            action: ACTION_INSECURE_MODE.to_string(),
            tool_name: None,
            arguments: None,
            status: "WARN".to_string(),
            error: Some("Running on non-loopback without TLS (--allow-plaintext)".to_string()),
            duration_ms: Some(0),
        });
    }

    let _ = audit.flush();
    let contents = std::fs::read_to_string(&audit_file).unwrap();
    assert!(contents.contains(ACTION_INSECURE_MODE));
    assert!(contents.contains("Running on non-loopback without TLS"));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// G3-2: HttpOnly Session Cookie Login & Revocation Lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_g3_2_session_cookie_login_and_revocation() {
    let ctx = setup_test_context(100).await;
    let peer: SocketAddr = "127.0.0.1:45678".parse().unwrap();

    // 1. Unauthenticated request to /api/terminals returns 401
    let mut req = Request::builder()
        .method(Method::GET)
        .uri("/api/terminals")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 2. Login with wrong key fails (401) and does NOT set cookie
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&json!({ "key": "atpc_sk_badkey_1234567890123456789012345678901234567890123" })).unwrap()))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(!resp.headers().contains_key(header::SET_COOKIE));

    // 3. Login with valid root admin key succeeds (200) and returns HttpOnly SameSite=Strict cookie
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&json!({ "key": ctx.admin_key })).unwrap()))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let cookie_header = resp
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .expect("Set-Cookie header must be present")
        .to_string();
    assert!(cookie_header.contains("atpc_session=atpc_sess_"));
    assert!(cookie_header.contains("HttpOnly"));
    assert!(cookie_header.contains("SameSite=Strict"));

    let cookie_part = cookie_header.split(';').next().unwrap().trim();

    // 4. Access protected endpoint /api/terminals with session cookie
    let mut req = Request::builder()
        .method(Method::GET)
        .uri("/api/terminals")
        .header(header::COOKIE, cookie_part)
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 5. Logout endpoint invalidates the cookie
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/logout")
        .header(header::COOKIE, cookie_part)
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let logout_cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap();
    assert!(logout_cookie.contains("Max-Age=0"));

    // 6. Old session cookie is now rejected with 401
    let mut req = Request::builder()
        .method(Method::GET)
        .uri("/api/terminals")
        .header(header::COOKIE, cookie_part)
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// G3-3: Per-Key Quota and Rate Limit Headers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_g3_3_quota_and_ratelimit_headers() {
    // Setup context with tight limit of 3 requests per minute
    let ctx = setup_test_context(3).await;
    let peer: SocketAddr = "127.0.0.1:56789".parse().unwrap();

    // 1. Request 1: remaining should be 2
    let mut req = Request::builder()
        .method(Method::GET)
        .uri("/api/terminals")
        .header(header::AUTHORIZATION, format!("Bearer {}", ctx.admin_key))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("x-ratelimit-limit").and_then(|v| v.to_str().ok()),
        Some("3")
    );
    assert_eq!(
        resp.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok()),
        Some("2")
    );
    assert!(resp.headers().contains_key("x-ratelimit-reset"));

    // 2. Request 2: remaining should be 1
    let mut req = Request::builder()
        .method(Method::GET)
        .uri("/api/terminals")
        .header(header::AUTHORIZATION, format!("Bearer {}", ctx.admin_key))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok()),
        Some("1")
    );

    // 3. Request 3: remaining should be 0
    let mut req = Request::builder()
        .method(Method::GET)
        .uri("/api/terminals")
        .header(header::AUTHORIZATION, format!("Bearer {}", ctx.admin_key))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok()),
        Some("0")
    );

    // 4. Request 4: Quota exceeded -> 429 Too Many Requests + Retry-After
    let mut req = Request::builder()
        .method(Method::GET)
        .uri("/api/terminals")
        .header(header::AUTHORIZATION, format!("Bearer {}", ctx.admin_key))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(resp.headers().contains_key(header::RETRY_AFTER));
    assert_eq!(
        resp.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok()),
        Some("0")
    );
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body["error"].as_str().unwrap().contains("Request quota exceeded"));
}

#[tokio::test]
async fn test_desktop_streaming_bypasses_quota() {
    // Setup context with strict limit of 3 requests per minute
    let ctx = setup_test_context(3).await;
    let peer: SocketAddr = "127.0.0.1:56789".parse().unwrap();

    // 1. Send 10 rapid desktop frame streaming requests
    for _ in 0..10 {
        let mut req = Request::builder()
            .method(Method::GET)
            .uri("/api/terminals/dummy-term/desktop/frame.jpg")
            .header(header::AUTHORIZATION, format!("Bearer {}", ctx.admin_key))
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(peer));
        let resp = ctx.app.clone().oneshot(req).await.unwrap();
        // Returns 204 NO_CONTENT since dummy-term has no frames, but crucially NOT 429!
        assert_ne!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        // Should not have ratelimit headers
        assert!(!resp.headers().contains_key("x-ratelimit-remaining"));
    }

    // 2. Regular API requests should still have the full quota (3 remaining)
    let mut req = Request::builder()
        .method(Method::GET)
        .uri("/api/terminals")
        .header(header::AUTHORIZATION, format!("Bearer {}", ctx.admin_key))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok()),
        Some("2")
    );
}

// ---------------------------------------------------------------------------
// G3-4: Legacy Credentials Migration and Enforcement Gate
// ---------------------------------------------------------------------------

#[test]
fn test_g3_4_legacy_credentials_migration_and_gate() {
    let dir = std::env::temp_dir().join(format!(
        "atpc-g3-migrate-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let keys_path = dir.join("keys.json");

    let mut config = ServerConfig::default();
    config.roles.insert("legacy-secret-admin".to_string(), Role::Admin);
    config.roles.insert("legacy-secret-viewer".to_string(), Role::Viewer);
    config.auth_token = Some("legacy-device-token".to_string());

    // 1. Initial keystore has no keys -> validate_legacy_config_gate succeeds
    assert_eq!(validate_legacy_config_gate(&config, false), Ok(()));

    // 2. Perform migration into KeyStore
    let keystore = KeyStore::load_or_create(&keys_path).unwrap();
    let mut migrated_count = 0;
    for (_token, role) in &config.roles {
        let (rec, plaintext) = keystore
            .issue(
                format!("migrated-{}", role.to_string().to_ascii_lowercase()),
                KeyKind::SecretKey,
                Some(*role),
                vec!["*".to_string()],
                None,
                None,
            )
            .unwrap();
        assert!(plaintext.starts_with("atpc_sk_"));
        assert_eq!(rec.role, Some(*role));
        migrated_count += 1;
    }
    if let Some(ref _primary) = config.auth_token {
        let (rec, plaintext) = keystore
            .issue(
                "migrated-agent".to_string(),
                KeyKind::DeviceToken,
                None,
                vec!["terminal".to_string()],
                None,
                None,
            )
            .unwrap();
        assert!(plaintext.starts_with("atpc_dv_"));
        assert_eq!(rec.kind, KeyKind::DeviceToken);
        migrated_count += 1;
    }
    assert_eq!(migrated_count, 3);
    assert!(keystore.has_active_keys());

    // 3. Once KeyStore has active keys, booting with plaintext roles in config MUST be rejected
    assert_eq!(
        validate_legacy_config_gate(&config, true),
        Err(BootGateError::PlaintextRolesNotAllowed)
    );

    // 4. Clearing roles while leaving auth_token MUST still be rejected (P1-12)
    config.roles.clear();
    assert_eq!(
        validate_legacy_config_gate(&config, true),
        Err(BootGateError::PlaintextRolesNotAllowed)
    );

    // 5. Once both plaintext roles and auth_token are cleared, gate passes
    config.auth_token = None;
    assert_eq!(validate_legacy_config_gate(&config, true), Ok(()));

    let _ = std::fs::remove_dir_all(&dir);
}
