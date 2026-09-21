use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use at_pc_protocol::Role;
use at_pc_server::audit::AuditLogger;
use at_pc_server::config::ServerConfig;
use at_pc_server::keystore::{KeyKind, KeyStore};
use at_pc_server::mcp::{create_mcp_http_router_with_state, McpHttpState};
use at_pc_server::rate_limit::AuthRateLimiter;
use at_pc_server::router::McpRouter;
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
    admin_id: String,
    _audit_file: PathBuf,
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
        "atpc-m1-test-{}-{}-{}",
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
            "test-root-admin".to_string(),
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

    let rate_limiter = Arc::new(AuthRateLimiter::default());
    let state = McpHttpState {
        router,
        config,
        log_file_path: dir.join("server.log"),
        audit_logger: Some(audit_logger),
        keystore: Some(keystore),
        rate_limiter,
        session_store: Arc::new(at_pc_server::session::SessionStore::default()),
        quota_tracker: Arc::new(at_pc_server::rate_limit::KeyQuotaTracker::default()),
        ticket_store: Arc::new(at_pc_server::DesktopTicketStore::default()),
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
        admin_id: admin_record.id,
        _audit_file: audit_file,
    }
}

fn peer_addr() -> SocketAddr {
    SocketAddr::from(([192, 168, 1, 50], 12345))
}

async fn send_request(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
    peer: SocketAddr,
) -> (StatusCode, HeaderMap, Value, String) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req_body = if let Some(b) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(serde_json::to_vec(&b).unwrap())
    } else {
        Body::empty()
    };
    let mut req = builder.body(req_body).unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let body_bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let text = String::from_utf8_lossy(&body_bytes).to_string();
    let json_val = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, headers, json_val, text)
}

use axum::http::HeaderMap;

#[tokio::test]
async fn test_g1_2_keystore_lifecycle_e2e() {
    let ctx = setup_test_context().await;
    let peer = peer_addr();

    // 1. Issue a new key (viewer)
    let (status, _, create_resp, _) = send_request(
        &ctx.app,
        Method::POST,
        "/api/keys",
        Some(&ctx.admin_key),
        Some(json!({
            "name": "ops-viewer",
            "kind": "sk",
            "role": "viewer",
            "scopes": ["console", "mcp"]
        })),
        peer,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let viewer_key = create_resp["plaintext"]
        .as_str()
        .expect("must return plaintext")
        .to_string();
    let viewer_id = create_resp["key"]["id"]
        .as_str()
        .expect("must return key id")
        .to_string();

    assert!(
        viewer_key.starts_with("atpc_sk_"),
        "key must start with atpc_sk_"
    );

    // Verify key works
    let (status, _, _, _) = send_request(
        &ctx.app,
        Method::GET,
        "/api/terminals",
        Some(&viewer_key),
        None,
        peer,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 2. Plaintext never leaks in list
    let (status, _, list_resp, _) = send_request(
        &ctx.app,
        Method::GET,
        "/api/keys",
        Some(&ctx.admin_key),
        None,
        peer,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let keys_array = list_resp.as_array().expect("must be array");
    for k in keys_array {
        assert!(k.get("plaintext").is_none());
        assert!(k.get("hash").is_none());
        assert!(k.get("salt").is_none());
    }

    // Check keys.json on disk: contains sha256-salt-v1, 0600 on unix, never plaintext
    let keys_disk = std::fs::read_to_string(ctx.dir.join("keys.json")).unwrap();
    assert!(
        !keys_disk.contains(&viewer_key),
        "keys.json must not leak plaintext"
    );
    assert!(keys_disk.contains("sha256-salt-v1"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(ctx.dir.join("keys.json")).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    }

    // 3. Immediate Revocation
    let (status, _, _, _) = send_request(
        &ctx.app,
        Method::DELETE,
        &format!("/api/keys/{}", viewer_id),
        Some(&ctx.admin_key),
        None,
        peer,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Revoked key returns 401 immediately
    let (status, _, _, _) = send_request(
        &ctx.app,
        Method::GET,
        "/api/terminals",
        Some(&viewer_key),
        None,
        peer,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 4. Key Rotation with grace_hours = 0 (immediate revocation)
    // First create a new key to rotate
    let (_, _, new_k_resp, _) = send_request(
        &ctx.app,
        Method::POST,
        "/api/keys",
        Some(&ctx.admin_key),
        Some(json!({
            "name": "rotate-test",
            "kind": "sk",
            "role": "viewer",
            "scopes": ["console"]
        })),
        peer,
    )
    .await;
    let old_key = new_k_resp["plaintext"].as_str().unwrap().to_string();
    let old_id = new_k_resp["key"]["id"].as_str().unwrap().to_string();

    let (status, _, rot_resp, _) = send_request(
        &ctx.app,
        Method::POST,
        &format!("/api/keys/{}/rotate", old_id),
        Some(&ctx.admin_key),
        Some(json!({ "grace_hours": 0 })),
        peer,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rotated_key = rot_resp["plaintext"].as_str().unwrap().to_string();

    // New key works
    let (status, _, _, _) = send_request(
        &ctx.app,
        Method::GET,
        "/api/terminals",
        Some(&rotated_key),
        None,
        peer,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Old key immediately fails with 401
    let (status, _, _, _) = send_request(
        &ctx.app,
        Method::GET,
        "/api/terminals",
        Some(&old_key),
        None,
        peer,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 5. Guard: Last admin key cannot be revoked
    let (status, _, _, _) = send_request(
        &ctx.app,
        Method::DELETE,
        &format!("/api/keys/{}", ctx.admin_id),
        Some(&ctx.admin_key),
        None,
        peer,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "cannot revoke last active admin key"
    );
}

#[tokio::test]
async fn test_g1_3_rbac_matrix_for_nine_handlers() {
    let ctx = setup_test_context().await;
    let peer = peer_addr();

    // Create Viewer key
    let (_, _, v_resp, _) = send_request(
        &ctx.app,
        Method::POST,
        "/api/keys",
        Some(&ctx.admin_key),
        Some(json!({
            "name": "viewer-user",
            "kind": "sk",
            "role": "viewer",
            "scopes": ["console", "mcp"]
        })),
        peer,
    )
    .await;
    let viewer_key = v_resp["plaintext"].as_str().unwrap().to_string();

    // Create Operator key
    let (_, _, o_resp, _) = send_request(
        &ctx.app,
        Method::POST,
        "/api/keys",
        Some(&ctx.admin_key),
        Some(json!({
            "name": "operator-user",
            "kind": "sk",
            "role": "operator",
            "scopes": ["console", "mcp"]
        })),
        peer,
    )
    .await;
    let operator_key = o_resp["plaintext"].as_str().unwrap().to_string();

    let admin_key = ctx.admin_key.clone();

    struct Case {
        method: Method,
        path: &'static str,
        body: Option<Value>,
        viewer_status: StatusCode,
        operator_status: StatusCode,
        admin_status: StatusCode,
    }

    let cases = vec![
        // 1. GET /api/logs: Viewer 403, Operator 403, Admin 200
        Case {
            method: Method::GET,
            path: "/api/logs",
            body: None,
            viewer_status: StatusCode::FORBIDDEN,
            operator_status: StatusCode::FORBIDDEN,
            admin_status: StatusCode::OK,
        },
        // 2. GET /api/terminals: Viewer 200, Operator 200, Admin 200
        Case {
            method: Method::GET,
            path: "/api/terminals",
            body: None,
            viewer_status: StatusCode::OK,
            operator_status: StatusCode::OK,
            admin_status: StatusCode::OK,
        },
        // 3. POST /api/terminals/:id/meta: Viewer 403, Operator 200/400 (not 403), Admin 200/400
        Case {
            method: Method::POST,
            path: "/api/terminals/non_existent/meta",
            body: Some(serde_json::json!({ "custom_name": "t1" })),
            viewer_status: StatusCode::FORBIDDEN,
            operator_status: StatusCode::OK,
            admin_status: StatusCode::OK,
        },
        // 4. GET /api/calls: Viewer 200, Operator 200, Admin 200
        Case {
            method: Method::GET,
            path: "/api/calls",
            body: None,
            viewer_status: StatusCode::OK,
            operator_status: StatusCode::OK,
            admin_status: StatusCode::OK,
        },
        // 5. GET /api/terminals/:id/calls: Viewer 200, Operator 200, Admin 200
        Case {
            method: Method::GET,
            path: "/api/terminals/t1/calls",
            body: None,
            viewer_status: StatusCode::OK,
            operator_status: StatusCode::OK,
            admin_status: StatusCode::OK,
        },
        // 6. POST /api/terminals/:id/desktop/start: Viewer 403, Operator allowed (not 403), Admin allowed
        Case {
            method: Method::POST,
            path: "/api/terminals/t1/desktop/start",
            body: None,
            viewer_status: StatusCode::FORBIDDEN,
            operator_status: StatusCode::OK, // handler returns json success: false error
            admin_status: StatusCode::OK,
        },
        // 7. POST /api/terminals/:id/desktop/stop: Viewer 403, Operator allowed, Admin allowed
        Case {
            method: Method::POST,
            path: "/api/terminals/t1/desktop/stop",
            body: None,
            viewer_status: StatusCode::FORBIDDEN,
            operator_status: StatusCode::OK,
            admin_status: StatusCode::OK,
        },
        // 8. GET /api/terminals/:id/desktop/frame: Viewer 200, Operator 200, Admin 200
        Case {
            method: Method::GET,
            path: "/api/terminals/t1/desktop/frame",
            body: None,
            viewer_status: StatusCode::OK,
            operator_status: StatusCode::OK,
            admin_status: StatusCode::OK,
        },
        // 9. GET /api/terminals/:id/desktop/raw: Viewer 204 (No Content when no frame), Operator 204, Admin 204
        Case {
            method: Method::GET,
            path: "/api/terminals/t1/desktop/raw",
            body: None,
            viewer_status: StatusCode::NO_CONTENT,
            operator_status: StatusCode::NO_CONTENT,
            admin_status: StatusCode::NO_CONTENT,
        },
    ];

    for c in cases {
        // Viewer
        let (s, _, _, _) = send_request(
            &ctx.app,
            c.method.clone(),
            c.path,
            Some(&viewer_key),
            c.body.clone(),
            peer,
        )
        .await;
        assert_eq!(
            s, c.viewer_status,
            "Viewer on {} {} should be {}",
            c.method, c.path, c.viewer_status
        );

        // Operator
        let (s, _, _, _) = send_request(
            &ctx.app,
            c.method.clone(),
            c.path,
            Some(&operator_key),
            c.body.clone(),
            peer,
        )
        .await;
        assert_eq!(
            s, c.operator_status,
            "Operator on {} {} should be {}",
            c.method, c.path, c.operator_status
        );

        // Admin
        let (s, _, _, _) = send_request(
            &ctx.app,
            c.method.clone(),
            c.path,
            Some(&admin_key),
            c.body.clone(),
            peer,
        )
        .await;
        assert_eq!(
            s, c.admin_status,
            "Admin on {} {} should be {}",
            c.method, c.path, c.admin_status
        );
    }
}

#[tokio::test]
async fn test_g1_4_rate_limiting_http_gate() {
    let ctx = setup_test_context().await;
    let rate_test_ip = SocketAddr::from(([192, 168, 88, 99], 5555));
    let bad_key = "atpc_sk_00000000_ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ";

    // 1 to 9 failures return 401
    for i in 1..=9 {
        let (status, _, _, _) = send_request(
            &ctx.app,
            Method::GET,
            "/api/terminals",
            Some(bad_key),
            None,
            rate_test_ip,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "attempt {i} must be 401");
    }

    // 10th failure returns 429 with Retry-After
    let (status, headers, _, _) = send_request(
        &ctx.app,
        Method::GET,
        "/api/terminals",
        Some(bad_key),
        None,
        rate_test_ip,
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "10th attempt must be 429");
    assert!(
        headers.contains_key(header::RETRY_AFTER),
        "429 must include Retry-After header"
    );

    // Valid authentication resets the failure count
    let (status, _, _, _) = send_request(
        &ctx.app,
        Method::GET,
        "/api/terminals",
        Some(&ctx.admin_key),
        None,
        rate_test_ip,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "valid key must succeed");

    // After reset, next failure returns 401 (not 429)
    let (status, _, _, _) = send_request(
        &ctx.app,
        Method::GET,
        "/api/terminals",
        Some(bad_key),
        None,
        rate_test_ip,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "failure after reset must return 401");
}

#[tokio::test]
async fn test_g1_query_warning_header() {
    let ctx = setup_test_context().await;
    let peer = peer_addr();

    let uri = format!("/api/terminals?token={}", ctx.admin_key);
    let mut req = Request::builder()
        .method(Method::GET)
        .uri(&uri)
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));

    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let warning = resp
        .headers()
        .get("warning")
        .and_then(|v| v.to_str().ok())
        .expect("query auth must include Warning header");
    assert!(warning.contains("199"));
    assert!(warning.contains("deprecated"));
}

#[tokio::test]
async fn test_m3_session_login_logout_and_quota() {
    let ctx = setup_test_context().await;
    let peer = peer_addr();

    // 1. Quota headers present on authenticated request
    let (status, headers, _, _) = send_request(
        &ctx.app,
        Method::GET,
        "/api/auth/me",
        Some(&ctx.admin_key),
        None,
        peer,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get("x-ratelimit-limit").unwrap(), "120");
    assert!(headers.get("x-ratelimit-remaining").is_some());
    assert!(headers.get("x-ratelimit-reset").is_some());

    // 2. Login with bad key fails with 401
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&json!({ "key": "invalid_key" })).unwrap()))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 3. Login with valid admin key returns 200, role admin, and Set-Cookie with atpc_session
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

    let body_bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["role"], "admin");

    // Extract session token
    let session_cookie = cookie_header.split(';').next().unwrap().trim();

    // 4. Access authenticated endpoint using session cookie
    let mut req = Request::builder()
        .method(Method::GET)
        .uri("/api/auth/me")
        .header(header::COOKIE, session_cookie)
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let me_bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let me_json: Value = serde_json::from_slice(&me_bytes).unwrap();
    assert_eq!(me_json["id"], ctx.admin_id);
    assert_eq!(me_json["role"], "admin");

    // 5. Logout clears the cookie and revokes the session
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/logout")
        .header(header::COOKIE, session_cookie)
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let logout_cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .expect("Set-Cookie header must be present");
    assert!(logout_cookie.contains("Max-Age=0"));

    // 6. Old session cookie no longer valid
    let mut req = Request::builder()
        .method(Method::GET)
        .uri("/api/auth/me")
        .header(header::COOKIE, session_cookie)
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let resp = ctx.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
