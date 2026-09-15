use std::sync::Arc;
use tokio::sync::mpsc;
use serde_json::json;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt; // for oneshot

use at_pc_protocol::models::TerminalInfo;
use at_pc_server::config::ServerConfig;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::registry::TerminalRegistry;
use at_pc_server::mcp::create_mcp_http_router;

fn create_sample_terminal(id: &str, hostname: &str) -> TerminalInfo {
    TerminalInfo {
        terminal_id: id.to_string(),
        hostname: hostname.to_string(),
        username: "admin".to_string(),
        lan_ip: "192.168.1.50".to_string(),
        os_version: "Windows 11 Pro".to_string(),
        agent_version: "0.3.0".to_string(),
    }
}

#[tokio::test]
async fn test_terminal_naming_and_meta_integration() {
    let registry = Arc::new(TerminalRegistry::new());
    let (tx, _rx) = mpsc::unbounded_channel();
    let term = create_sample_terminal("pc-finance-01", "DESKTOP-7K8L2P");

    // 1. Register terminal
    registry.register(term, tx).await;

    // Initially custom_name is None
    let list = registry.list_terminals().await;
    assert_eq!(list.len(), 1);
    assert!(list[0].custom_name.is_none());
    assert_eq!(list[0].info.hostname, "DESKTOP-7K8L2P");

    // 2. Set custom name, notes, and tags
    let updated = registry
        .update_terminal_meta(
            "pc-finance-01",
            Some("财务部-出纳主控机".to_string()),
            Some("常驻财务室302".to_string()),
            Some(vec!["财务".to_string(), "关键设备".to_string()]),
        )
        .await
        .expect("update_terminal_meta should succeed");

    assert_eq!(updated.custom_name.as_deref(), Some("财务部-出纳主控机"));
    assert_eq!(updated.notes.as_deref(), Some("常驻财务室302"));
    assert_eq!(updated.tags, vec!["财务".to_string(), "关键设备".to_string()]);

    // 3. Verify list_terminals now includes custom name
    let list2 = registry.list_terminals().await;
    assert_eq!(list2.len(), 1);
    assert_eq!(list2[0].custom_name.as_deref(), Some("财务部-出纳主控机"));
    assert_eq!(list2[0].notes.as_deref(), Some("常驻财务室302"));
    assert_eq!(list2[0].tags, vec!["财务".to_string(), "关键设备".to_string()]);

    // 4. Verify get_terminal
    let single = registry.get_terminal("pc-finance-01").await.unwrap();
    assert_eq!(single.custom_name.as_deref(), Some("财务部-出纳主控机"));
}

#[tokio::test]
async fn test_mcp_router_select_by_custom_name() {
    let registry = Arc::new(TerminalRegistry::new());
    let (tx1, _rx1) = mpsc::unbounded_channel();
    let (tx2, _rx2) = mpsc::unbounded_channel();

    let term1 = create_sample_terminal("node-a", "DESKTOP-AAA");
    let term2 = create_sample_terminal("node-b", "DESKTOP-BBB");

    registry.register(term1, tx1).await;
    registry.register(term2, tx2).await;

    // Set custom names
    registry
        .update_terminal_meta("node-a", Some("研发-张三".to_string()), None, None)
        .await
        .unwrap();
    registry
        .update_terminal_meta("node-b", Some("运维-李四".to_string()), None, None)
        .await
        .unwrap();

    let router = Arc::new(McpRouter::new(registry));

    // Select by exact custom_name
    let selected = router.select_terminal("研发-张三").await.expect("should find by alias");
    assert_eq!(selected.info.terminal_id, "node-a");
    assert_eq!(router.get_active_terminal_id().await, Some("node-a".to_string()));

    // Select by other custom_name
    let selected2 = router.select_terminal("运维-李四").await.expect("should find by alias");
    assert_eq!(selected2.info.terminal_id, "node-b");
    assert_eq!(router.get_active_terminal_id().await, Some("node-b".to_string()));

    // Select by raw terminal_id still works
    let selected3 = router.select_terminal("node-a").await.expect("should find by raw id");
    assert_eq!(selected3.info.terminal_id, "node-a");
}

#[tokio::test]
async fn test_mcp_rename_terminal_tool_dispatch() {
    let registry = Arc::new(TerminalRegistry::new());
    let (tx, _rx) = mpsc::unbounded_channel();
    let term = create_sample_terminal("node-rename-test", "WIN-SRV-99");
    registry.register(term, tx).await;

    let router = Arc::new(McpRouter::new(registry));

    // Dispatch rename_terminal MCP tool
    let args = json!({
        "terminal_id": "node-rename-test",
        "custom_name": "核心网关机",
        "notes": "机房A柜03号",
        "tags": ["核心", "网络"]
    });

    let res = router
        .dispatch_tool_call("rename_terminal", args)
        .await
        .expect("rename_terminal should succeed");

    assert_eq!(res["custom_name"], "核心网关机");
    assert_eq!(res["notes"], "机房A柜03号");

    // Verify list_terminals
    let list = router.list_terminals().await;
    assert_eq!(list[0].custom_name.as_deref(), Some("核心网关机"));
}

#[tokio::test]
async fn test_dashboard_meta_rest_api() {
    let registry = Arc::new(TerminalRegistry::new());
    let (tx, _rx) = mpsc::unbounded_channel();
    let term = create_sample_terminal("api-term-01", "HOST-API-TEST");
    registry.register(term, tx).await;

    let router = Arc::new(McpRouter::new(registry));
    let app = create_mcp_http_router(router, ServerConfig::default());

    // 1. POST /api/terminals/api-term-01/meta
    let payload = json!({
        "custom_name": "API重命名测试",
        "notes": "通过HTTP接口更新",
        "tags": ["API", "测试"]
    });

    let req = Request::builder()
        .uri("/api/terminals/api-term-01/meta")
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. GET /api/terminals and verify custom_name in JSON
    let req2 = Request::builder()
        .uri("/api/terminals")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp2.into_body(), 1024 * 1024).await.unwrap();
    let terminals_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(terminals_json[0]["custom_name"], "API重命名测试");
    assert_eq!(terminals_json[0]["notes"], "通过HTTP接口更新");

    // 3. DELETE /api/terminals/api-term-01
    let req3 = Request::builder()
        .uri("/api/terminals/api-term-01")
        .method("DELETE")
        .body(Body::empty())
        .unwrap();

    let resp3 = app.clone().oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::OK);

    // Verify deletion
    let req4 = Request::builder()
        .uri("/api/terminals")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp4 = app.oneshot(req4).await.unwrap();
    let body_bytes4 = axum::body::to_bytes(resp4.into_body(), 1024 * 1024).await.unwrap();
    let terminals_json4: serde_json::Value = serde_json::from_slice(&body_bytes4).unwrap();
    assert_eq!(terminals_json4.as_array().unwrap().len(), 0);
}
