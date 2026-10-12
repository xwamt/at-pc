use std::net::SocketAddr;
use std::sync::Arc;

use at_pc_server::config::ServerConfig;
use at_pc_server::mcp::create_mcp_http_router;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::registry::TerminalRegistry;
use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use serde_json::Value;
use tower::ServiceExt;

const VALID_TOKEN: &str = "test_bearer_token_secret_123";

fn setup_test_app(auth_token: Option<&str>) -> Router {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry));
    let config = ServerConfig {
        auth_token: auth_token.map(|s| s.to_string()),
        ..Default::default()
    };
    create_mcp_http_router(router, config)
}

fn loopback_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 12345))
}

fn non_loopback_addr() -> SocketAddr {
    SocketAddr::from(([192, 168, 1, 100], 12345))
}

#[tokio::test]
async fn test_loopback_health_without_credentials_returns_minimal_payload() {
    let app = setup_test_app(Some(VALID_TOKEN));
    let mut req = Request::builder()
        .uri("/health")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(loopback_addr()));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    let obj = json.as_object().expect("response must be a JSON object");

    // Keys must be EXACTLY ["status"] with value "ok"
    let mut keys: Vec<String> = obj.keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, vec!["status".to_string()]);
    assert_eq!(json["status"], "ok");

    // Must NOT leak server version, internal ports, or terminal counts
    assert!(
        json.get("version").is_none(),
        "unauthenticated health response must not include version"
    );
    assert!(
        json.get("mcp_port").is_none(),
        "unauthenticated health response must not include mcp_port"
    );
    assert!(
        json.get("ws_port").is_none(),
        "unauthenticated health response must not include ws_port"
    );
    assert!(
        json.get("connected_terminals").is_none(),
        "unauthenticated health response must not include connected_terminals"
    );
}

#[tokio::test]
async fn test_loopback_health_with_valid_bearer_token_returns_full_payload() {
    let app = setup_test_app(Some(VALID_TOKEN));
    let mut req = Request::builder()
        .uri("/health")
        .method("GET")
        .header(header::AUTHORIZATION, format!("Bearer {}", VALID_TOKEN))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(loopback_addr()));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(json["status"], "ok");
    assert!(
        json.get("connected_terminals").is_some(),
        "authenticated health response must include connected_terminals"
    );
    assert!(json.get("version").is_some());
    assert!(json.get("mcp_port").is_some());
    assert!(json.get("ws_port").is_some());
}

#[tokio::test]
async fn test_non_loopback_health_without_credentials_returns_401() {
    let app = setup_test_app(Some(VALID_TOKEN));
    let mut req = Request::builder()
        .uri("/health")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(non_loopback_addr()));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_non_loopback_health_with_valid_bearer_token_returns_200() {
    let app = setup_test_app(Some(VALID_TOKEN));
    let mut req = Request::builder()
        .uri("/health")
        .method("GET")
        .header(header::AUTHORIZATION, format!("Bearer {}", VALID_TOKEN))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(non_loopback_addr()));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(json["status"], "ok");
    assert!(
        json.get("connected_terminals").is_some(),
        "authenticated health response must include connected_terminals"
    );
}

#[tokio::test]
async fn test_non_loopback_api_terminals_without_credentials_returns_401() {
    let app = setup_test_app(Some(VALID_TOKEN));
    let mut req = Request::builder()
        .uri("/api/terminals")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(non_loopback_addr()));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_non_loopback_dashboard_root_without_credentials_returns_200() {
    let app = setup_test_app(Some(VALID_TOKEN));
    let mut req = Request::builder()
        .uri("/")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(non_loopback_addr()));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET / must be exempt from authentication to serve dashboard login shell"
    );

    let body_bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let body_str = String::from_utf8_lossy(&body_bytes);
    assert!(
        body_str.contains("<!DOCTYPE html>")
            || body_str.contains("<html")
            || body_str.contains("at-pc"),
        "root endpoint must return dashboard HTML shell"
    );
}

#[tokio::test]
async fn test_dev_mode_loopback_allowed_but_non_loopback_gated() {
    // Open dev mode: no auth_token and no roles configured
    let app = setup_test_app(None);

    // Loopback origin can access /api/terminals in open dev mode
    let mut req_loopback = Request::builder()
        .uri("/api/terminals")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    req_loopback
        .extensions_mut()
        .insert(ConnectInfo(loopback_addr()));
    let resp_loopback = app.clone().oneshot(req_loopback).await.unwrap();
    assert_eq!(resp_loopback.status(), StatusCode::OK);

    // Non-loopback origin without credentials is STILL rejected with 401
    let mut req_non_loopback = Request::builder()
        .uri("/api/terminals")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    req_non_loopback
        .extensions_mut()
        .insert(ConnectInfo(non_loopback_addr()));
    let resp_non_loopback = app.oneshot(req_non_loopback).await.unwrap();
    assert_eq!(resp_non_loopback.status(), StatusCode::UNAUTHORIZED);
}
