//! Integration tests for Milestone 3 (P2 / v0.8.0):
//! Remote desktop multi-monitor streaming plumbing:
//! - `api_start_desktop_stream` accepts `display_index` in request payload and passes to Agent
//! - `router.start_desktop_stream` dispatches `ServerToAgentMessage::StartDesktopStream` with specified `display_index`
//! - `api_get_desktop_frame` and `api_get_desktop_frame_raw` return `display_index` for multi-monitor video feeds

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

use at_pc_protocol::messages::{BinaryDesktopFrame, ServerToAgentMessage};
use at_pc_protocol::models::TerminalInfo;
use at_pc_server::config::ServerConfig;
use at_pc_server::mcp::create_mcp_http_router;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::handler::AgentMessageHandler;
use at_pc_server::ws::registry::TerminalRegistry;

fn create_test_terminal(id: &str, hostname: &str) -> TerminalInfo {
    TerminalInfo {
        terminal_id: id.to_string(),
        hostname: hostname.to_string(),
        username: "test_user".to_string(),
        lan_ip: "127.0.0.1".to_string(),
        os_version: "macOS 15.0".to_string(),
        agent_version: "1.0.0".to_string(),
    }
}

// =========================================================================
// 1. Router start_desktop_stream dispatches specified display_index
// =========================================================================
#[tokio::test]
async fn test_router_start_desktop_stream_dispatches_specified_display_index() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let term = create_test_terminal("term-stream-router", "STREAM-HOST");
    registry.register(term, tx).await;

    // Test primary display 0
    router
        .start_desktop_stream("term-stream-router", 15, 60, 0, 1.0)
        .await
        .expect("start_desktop_stream should succeed");

    let msg = rx
        .recv()
        .await
        .expect("Expected StartDesktopStream message");
    match msg {
        ServerToAgentMessage::StartDesktopStream {
            display_index,
            fps,
            quality,
            scale,
        } => {
            assert_eq!(display_index, 0);
            assert_eq!(fps, 15);
            assert_eq!(quality, 60);
            assert_eq!(scale, 1.0);
        }
        _ => panic!("Expected StartDesktopStream, got {:?}", msg),
    }

    // Test secondary display 1 with custom fps and quality
    router
        .start_desktop_stream("term-stream-router", 30, 85, 1, 1.0)
        .await
        .expect("start_desktop_stream should succeed");

    let msg2 = rx
        .recv()
        .await
        .expect("Expected StartDesktopStream message");
    match msg2 {
        ServerToAgentMessage::StartDesktopStream {
            display_index,
            fps,
            quality,
            scale,
        } => {
            assert_eq!(display_index, 1);
            assert_eq!(fps, 30);
            assert_eq!(quality, 85);
            assert_eq!(scale, 1.0);
        }
        _ => panic!("Expected StartDesktopStream, got {:?}", msg2),
    }

    // Test tertiary display 2 with 0 fps/quality fallback
    router
        .start_desktop_stream("term-stream-router", 0, 0, 2, 1.0)
        .await
        .expect("start_desktop_stream should succeed");

    let msg3 = rx
        .recv()
        .await
        .expect("Expected StartDesktopStream message");
    match msg3 {
        ServerToAgentMessage::StartDesktopStream {
            display_index,
            fps,
            quality,
            ..
        } => {
            assert_eq!(display_index, 2);
            assert_eq!(fps, 15, "0 fps should fallback to 15");
            assert_eq!(quality, 60, "0 quality should fallback to 60");
        }
        _ => panic!("Expected StartDesktopStream, got {:?}", msg3),
    }

    router
        .start_desktop_stream("term-stream-router", 15, 60, 0, 0.5)
        .await
        .expect("start_desktop_stream should succeed");
    let msg4 = rx
        .recv()
        .await
        .expect("Expected StartDesktopStream message");
    match msg4 {
        ServerToAgentMessage::StartDesktopStream { scale, .. } => {
            assert_eq!(scale, 0.5, "router must pass protocol scale through");
        }
        _ => panic!("Expected StartDesktopStream, got {:?}", msg4),
    }
}

// =========================================================================
// 2. HTTP POST /api/terminals/:id/desktop/stream accepts display_index
// =========================================================================
#[tokio::test]
async fn test_api_start_desktop_stream_accepts_display_index_payload() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let config = ServerConfig::default();
    let app = create_mcp_http_router(router.clone(), config);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let term = create_test_terminal("term-api-stream", "API-STREAM-HOST");
    registry.register(term, tx).await;

    // Case 2.1: Explicit display_index: 1 in request payload
    let req_body = json!({
        "fps": 20,
        "quality": 75,
        "display_index": 1,
        "scale": 0.75
    });
    let req = Request::builder()
        .uri("/api/terminals/term-api-stream/desktop/stream")
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["success"], true);

    let received = rx.recv().await.expect("Expected message on channel");
    match received {
        ServerToAgentMessage::StartDesktopStream {
            display_index,
            fps,
            quality,
            scale,
        } => {
            assert_eq!(
                display_index, 1,
                "Agent must receive specified display_index"
            );
            assert_eq!(fps, 20);
            assert_eq!(quality, 75);
            assert_eq!(scale, 0.75, "dashboard must pass scale through to agent");
        }
        _ => panic!("Expected StartDesktopStream, got {:?}", received),
    }

    // Case 2.2: Omitted display_index defaults to 0
    let req_body2 = json!({
        "fps": 15,
        "quality": 60
    });
    let req2 = Request::builder()
        .uri("/api/terminals/term-api-stream/desktop/stream")
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&req_body2).unwrap()))
        .unwrap();

    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);

    let received2 = rx.recv().await.expect("Expected message on channel");
    match received2 {
        ServerToAgentMessage::StartDesktopStream {
            display_index,
            scale,
            ..
        } => {
            assert_eq!(display_index, 0, "Omitted display_index must default to 0");
            assert_eq!(scale, 1.0, "Omitted scale must default to 1.0");
        }
        _ => panic!("Expected StartDesktopStream, got {:?}", received2),
    }

    // Case 2.3: Completely empty body defaults to display_index: 0
    let req3 = Request::builder()
        .uri("/api/terminals/term-api-stream/desktop/stream")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let resp3 = app.oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::OK);

    let received3 = rx.recv().await.expect("Expected message on channel");
    match received3 {
        ServerToAgentMessage::StartDesktopStream {
            display_index,
            fps,
            quality,
            scale,
        } => {
            assert_eq!(
                display_index, 0,
                "Empty payload must default to display_index 0"
            );
            assert_eq!(fps, 15);
            assert_eq!(quality, 60);
            assert_eq!(scale, 1.0, "Empty payload must default to scale 1.0");
        }
        _ => panic!("Expected StartDesktopStream, got {:?}", received3),
    }
}

// =========================================================================
// 3. HTTP GET /api/terminals/:id/desktop/frame returns display_index
// =========================================================================
#[tokio::test]
async fn test_api_get_desktop_frame_returns_display_index() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let config = ServerConfig::default();
    let app = create_mcp_http_router(router.clone(), config);

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let term = create_test_terminal("term-frame-test", "FRAME-HOST");
    registry.register(term, tx).await;

    // Simulate Agent delivering desktop frame for secondary monitor (display_index = 1)
    let fake_jpeg = b"mock_display1_data".to_vec();
    router.handle_desktop_frame_binary(
        "term-frame-test",
        BinaryDesktopFrame {
            display_index: 1,
            width: 2560,
            height: 1440,
            timestamp: 1001,
            data: fake_jpeg.clone(),
        },
    );

    // 3.1 JSON Frame API: GET /api/terminals/:id/desktop/frame
    let req = Request::builder()
        .uri("/api/terminals/term-frame-test/desktop/frame")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["success"], true);
    assert_eq!(
        body_json["display_index"], 1,
        "Frame JSON must include display_index = 1"
    );
    assert_eq!(body_json["width"], 2560);
    assert_eq!(body_json["height"], 1440);
    assert_eq!(body_json["format"], "jpeg");
    assert_eq!(
        body_json["data"],
        base64::prelude::BASE64_STANDARD.encode(&fake_jpeg)
    );
    assert_eq!(body_json["timestamp"], 1001);

    // 3.2 Raw JPEG Frame API: GET /api/terminals/:id/desktop/frame.jpg
    let req_raw = Request::builder()
        .uri("/api/terminals/term-frame-test/desktop/frame.jpg")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp_raw = app.clone().oneshot(req_raw).await.unwrap();
    assert_eq!(resp_raw.status(), StatusCode::OK);
    assert_eq!(
        resp_raw
            .headers()
            .get("X-Display-Index")
            .unwrap()
            .to_str()
            .unwrap(),
        "1",
        "Raw frame headers must include X-Display-Index: 1"
    );
    assert_eq!(
        resp_raw
            .headers()
            .get("X-Frame-Width")
            .unwrap()
            .to_str()
            .unwrap(),
        "2560"
    );
    assert_eq!(
        resp_raw
            .headers()
            .get("X-Frame-Height")
            .unwrap()
            .to_str()
            .unwrap(),
        "1440"
    );

    // 3.3 Switch display: Simulate Agent delivering binary frame for display 0
    let fake_jpeg = vec![
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0xFF, 0xD9,
    ];
    router.handle_desktop_frame_binary(
        "term-frame-test",
        BinaryDesktopFrame {
            display_index: 0,
            width: 1920,
            height: 1080,
            timestamp: 1002,
            data: fake_jpeg.clone(),
        },
    );

    let req_switched = Request::builder()
        .uri("/api/terminals/term-frame-test/desktop/frame")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp_switched = app.oneshot(req_switched).await.unwrap();
    assert_eq!(resp_switched.status(), StatusCode::OK);

    let body_switched = axum::body::to_bytes(resp_switched.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json_switched: serde_json::Value = serde_json::from_slice(&body_switched).unwrap();
    assert_eq!(json_switched["success"], true);
    assert_eq!(
        json_switched["display_index"], 0,
        "Frame must reflect switch to display_index = 0"
    );
    assert_eq!(json_switched["width"], 1920);
    assert_eq!(json_switched["height"], 1080);
    assert_eq!(json_switched["timestamp"], 1002);
}

#[tokio::test]
async fn empty_keepalive_frame_does_not_clobber_cached_jpeg() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let jpeg = b"real-jpeg-bytes".to_vec();
    router.handle_desktop_frame_binary(
        "term-keepalive",
        BinaryDesktopFrame {
            display_index: 0,
            width: 1280,
            height: 720,
            timestamp: 1000,
            data: jpeg.clone(),
        },
    );
    router.handle_desktop_frame_binary(
        "term-keepalive",
        BinaryDesktopFrame {
            display_index: 0,
            width: 1280,
            height: 720,
            timestamp: 2000,
            data: Vec::new(),
        },
    );

    let raw = router
        .get_latest_desktop_frame_raw("term-keepalive")
        .await
        .expect("cached frame");
    assert_eq!(
        raw.4, jpeg,
        "keepalive must not replace JPEG with empty data"
    );
    assert_eq!(raw.3, 2000, "keepalive may refresh the timestamp");
}

#[tokio::test]
async fn empty_keepalive_without_cache_does_not_insert_frame() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    router.handle_desktop_frame_binary(
        "term-empty-ka",
        BinaryDesktopFrame {
            display_index: 0,
            width: 1280,
            height: 720,
            timestamp: 1000,
            data: Vec::new(),
        },
    );

    let raw = router.get_latest_desktop_frame_raw("term-empty-ka").await;
    assert!(
        raw.is_none(),
        "empty keepalive with no cache must not insert raw_bytes"
    );
}
