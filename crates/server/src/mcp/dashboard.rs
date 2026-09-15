//! Embedded Web Dashboard & Management UI for at-pc-server.
//! Provides a responsive, zero-external-dependency web interface
//! to inspect active terminals, CPU/RAM metrics, and run quick diagnostics.
//! Decoupled from static frontend assets using rust-embed.

use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use rust_embed::Embed;
use serde::Deserialize;
use serde_json::{json, Value};
use std::borrow::Cow;

use crate::config::Role;
use super::McpHttpState;

/// Embedded static frontend assets bundled directly into binary release artifact
#[derive(Embed)]
#[folder = "frontend/"]
pub struct FrontendAssets;

/// Constant copy of dashboard HTML bundled via include_str for zero-runtime overhead
pub const DASHBOARD_HTML: &str = include_str!("../../frontend/index.html");

/// Retrieves the dashboard HTML, supporting hot-reload during development
pub fn get_dashboard_html() -> Cow<'static, str> {
    // 1. Hot reload from environment override if set
    if let Ok(dir) = std::env::var("AT_PC_FRONTEND_DIR") {
        let p = std::path::Path::new(&dir).join("index.html");
        if let Ok(content) = std::fs::read_to_string(p) {
            return Cow::Owned(content);
        }
    }
    // 2. Hot reload from workspace repository directory if present
    if let Ok(content) = std::fs::read_to_string("crates/server/frontend/index.html") {
        return Cow::Owned(content);
    }
    // 3. Fallback to bundled constant
    Cow::Borrowed(DASHBOARD_HTML)
}

/// Create dashboard sub-router
pub fn create_dashboard_router(state: McpHttpState) -> Router<McpHttpState> {
    let api_routes = Router::new()
        .route("/logs", get(api_get_logs))
        .route("/audit", get(api_get_audit_logs))
        .route("/terminals", get(api_list_terminals))
        .route("/terminals/:id/meta", post(api_update_terminal_meta))
        .route("/terminals/:id", axum::routing::delete(api_delete_terminal))
        .route("/terminals/:id/invoke", post(api_invoke_tool))
        .route("/terminals/:id/calls", get(api_list_terminal_pending_calls))
        .route("/terminals/:id/desktop/stream", post(api_start_desktop_stream))
        .route("/terminals/:id/desktop/stop", post(api_stop_desktop_stream))
        .route("/terminals/:id/desktop/frame", get(api_get_desktop_frame))
        .route("/terminals/:id/desktop/frame.jpg", get(api_get_desktop_frame_raw))
        .route("/terminals/:id/desktop/raw", get(api_get_desktop_frame_raw))
        .route("/terminals/:id/desktop/input", post(api_send_desktop_input))
        .route("/terminals/:id/calls/:call_id/cancel", post(api_cancel_terminal_call))
        .route("/calls", get(api_list_pending_calls))
        .route("/calls/:id/cancel", post(api_cancel_call))
        .layer(axum::middleware::from_fn_with_state(
            state,
            super::auth_middleware,
        ));

    Router::new()
        .route("/", get(dashboard_html_handler))
        .route("/dashboard", get(dashboard_html_handler))
        .route("/index.html", get(dashboard_html_handler))
        .route("/static/*path", get(static_asset_handler))
        .nest("/api", api_routes)
}

/// Handler for `GET /api/logs`
async fn api_get_logs() -> impl IntoResponse {
    let content = std::fs::read_to_string("at-pc-server.log")
        .unwrap_or_else(|_| "No log file found or empty".to_string());
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(200);
    let tail = lines[start..].join("\n");
    (StatusCode::OK, [("Content-Type", "text/plain; charset=utf-8")], tail)
}

/// Handler for `GET /` and `GET /dashboard`
async fn dashboard_html_handler() -> impl IntoResponse {
    Html(get_dashboard_html().into_owned())
}

/// Handler for `GET /static/*path` to serve decoupled static assets
async fn static_asset_handler(Path(path): Path<String>) -> impl IntoResponse {
    let disk_bytes = if let Ok(dir) = std::env::var("AT_PC_FRONTEND_DIR") {
        let p = std::path::Path::new(&dir).join(&path);
        std::fs::read(p).ok()
    } else {
        let p = std::path::Path::new("crates/server/frontend").join(&path);
        std::fs::read(p).ok()
    };

    let data_opt = disk_bytes.or_else(|| FrontendAssets::get(&path).map(|c| c.data.to_vec()));

    if let Some(data) = data_opt {
        let mime = match std::path::Path::new(&path).extension().and_then(|ext| ext.to_str()) {
            Some("html") | Some("htm") => "text/html; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            Some("js") => "application/javascript; charset=utf-8",
            Some("json") => "application/json",
            Some("svg") => "image/svg+xml",
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            _ => "application/octet-stream",
        };
        (
            StatusCode::OK,
            [("Content-Type", mime)],
            data,
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            [("Content-Type", "text/plain; charset=utf-8")],
            "Asset Not Found",
        )
            .into_response()
    }
}

/// Handler for `GET /api/terminals`
async fn api_list_terminals(State(state): State<McpHttpState>) -> impl IntoResponse {
    let entries = state.router.list_terminals().await;
    Json(entries)
}

#[derive(Deserialize)]
struct UpdateMetaRequest {
    custom_name: Option<String>,
    notes: Option<String>,
    tags: Option<Vec<String>>,
}

/// Handler for `POST /api/terminals/:id/meta`
async fn api_update_terminal_meta(
    State(state): State<McpHttpState>,
    Path(terminal_id): Path<String>,
    Json(payload): Json<UpdateMetaRequest>,
) -> impl IntoResponse {
    match state
        .router
        .registry()
        .update_terminal_meta(&terminal_id, payload.custom_name, payload.notes, payload.tags)
        .await
    {
        Ok(meta) => (StatusCode::OK, Json(json!({ "success": true, "meta": meta }))),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": err })),
        ),
    }
}

/// Handler for `DELETE /api/terminals/:id`
async fn api_delete_terminal(
    State(state): State<McpHttpState>,
    Extension(role): Extension<Role>,
    token_ext: Option<Extension<String>>,
    headers: HeaderMap,
    Path(terminal_id): Path<String>,
) -> Response {
    let client_ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let token_prefix = token_ext.as_ref().map(|Extension(t)| crate::audit::AuditLogger::redact_token(t));

    if role != Role::Admin {
        if let Some(logger) = state.router.audit_logger() {
            logger.log(crate::audit::AuditRecord {
                id: format!("audit-{}", crate::router::generate_call_id()),
                timestamp: chrono::Utc::now().to_rfc3339(),
                role: Some(role.to_string()),
                token_prefix,
                client_ip,
                terminal_id: Some(terminal_id.clone()),
                action: "api:delete_terminal".to_string(),
                tool_name: None,
                arguments: None,
                status: "DENIED".to_string(),
                error: Some("Forbidden: Only Admin role can delete terminals".to_string()),
                duration_ms: Some(0),
            });
        }
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "success": false, "error": "Forbidden: Only Admin role can delete terminals" })),
        )
            .into_response();
    }

    state.router.abort_pending_calls_for_terminal(&terminal_id, "Terminal removed via API");
    let removed = state.router.registry().remove_terminal(&terminal_id).await;

    if let Some(logger) = state.router.audit_logger() {
        logger.log(crate::audit::AuditRecord {
            id: format!("audit-{}", crate::router::generate_call_id()),
            timestamp: chrono::Utc::now().to_rfc3339(),
            role: Some(role.to_string()),
            token_prefix,
            client_ip,
            terminal_id: Some(terminal_id),
            action: "api:delete_terminal".to_string(),
            tool_name: None,
            arguments: None,
            status: "SUCCESS".to_string(),
            error: None,
            duration_ms: Some(0),
        });
    }

    Json(json!({ "success": true, "removed": removed })).into_response()
}

#[derive(Deserialize)]
struct InvokeRequest {
    tool: String,
    arguments: Option<Value>,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// Handler for `POST /api/terminals/:id/invoke`
async fn api_invoke_tool(
    State(state): State<McpHttpState>,
    Extension(role): Extension<Role>,
    token_ext: Option<Extension<String>>,
    headers: HeaderMap,
    Path(terminal_id): Path<String>,
    Json(payload): Json<InvokeRequest>,
) -> Response {
    let client_ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let token_prefix = token_ext.as_ref().map(|Extension(t)| crate::audit::AuditLogger::redact_token(t));

    if !crate::config::is_tool_allowed_for_role(role, &payload.tool) {
        if let Some(logger) = state.router.audit_logger() {
            logger.log(crate::audit::AuditRecord {
                id: format!("audit-{}", crate::router::generate_call_id()),
                timestamp: chrono::Utc::now().to_rfc3339(),
                role: Some(role.to_string()),
                token_prefix,
                client_ip,
                terminal_id: Some(terminal_id.clone()),
                action: format!("api:invoke:{}", payload.tool),
                tool_name: Some(payload.tool.clone()),
                arguments: payload.arguments.clone(),
                status: "DENIED".to_string(),
                error: Some(format!("Forbidden: Role '{}' is not authorized to execute tool '{}'", role, payload.tool)),
                duration_ms: Some(0),
            });
        }
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "success": false,
                "error": format!("Forbidden: Role '{}' is not authorized to execute tool '{}'", role, payload.tool)
            })),
        )
            .into_response();
    }

    let args = payload.arguments.unwrap_or_else(|| json!({}));
    let timeout = payload
        .timeout_secs
        .or_else(|| args.get("timeout_secs").and_then(|v| v.as_u64()))
        .unwrap_or(30);

    let start_time = std::time::Instant::now();
    let res = state
        .router
        .invoke_tool(&terminal_id, &payload.tool, args.clone(), timeout)
        .await;
    let elapsed_ms = start_time.elapsed().as_millis() as u64;

    if let Some(logger) = state.router.audit_logger() {
        match &res {
            Ok(_) => {
                logger.log(crate::audit::AuditRecord {
                    id: format!("audit-{}", crate::router::generate_call_id()),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    role: Some(role.to_string()),
                    token_prefix,
                    client_ip,
                    terminal_id: Some(terminal_id.clone()),
                    action: format!("api:invoke:{}", payload.tool),
                    tool_name: Some(payload.tool.clone()),
                    arguments: Some(args.clone()),
                    status: "SUCCESS".to_string(),
                    error: None,
                    duration_ms: Some(elapsed_ms),
                });
            }
            Err(e) => {
                logger.log(crate::audit::AuditRecord {
                    id: format!("audit-{}", crate::router::generate_call_id()),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    role: Some(role.to_string()),
                    token_prefix,
                    client_ip,
                    terminal_id: Some(terminal_id),
                    action: format!("api:invoke:{}", payload.tool),
                    tool_name: Some(payload.tool.clone()),
                    arguments: Some(args.clone()),
                    status: "FAILED".to_string(),
                    error: Some(e.clone()),
                    duration_ms: Some(elapsed_ms),
                });
            }
        }
    }

    match res {
        Ok(mut val) => {
            // Check if server_save_path was requested for screenshot or som
            if payload.tool == "capture_screen" || payload.tool == "get_marked_screen" {
                if let Some(ssp) = args.get("server_save_path").and_then(|v| v.as_str()) {
                    let clean = ssp.trim();
                    let b64_opt = val
                        .get("raw_base64")
                        .or_else(|| val.get("image_base64"))
                        .and_then(|v| v.as_str())
                        .or_else(|| {
                            val.get("base64_data").and_then(|v| v.as_str()).map(|s| {
                                if let Some(idx) = s.find(";base64,") {
                                    &s[idx + 8..]
                                } else if let Some(idx) = s.find(',') {
                                    &s[idx + 1..]
                                } else {
                                    s
                                }
                            })
                        });
                    if let Some(b64) = b64_opt {
                        use base64::Engine;
                        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                            let path = std::path::Path::new(clean);
                            if let Some(parent) = path.parent() {
                                if !parent.as_os_str().is_empty() && !parent.exists() {
                                    let _ = std::fs::create_dir_all(parent);
                                }
                            }
                            if std::fs::write(path, &bytes).is_ok() {
                                if let Some(obj) = val.as_object_mut() {
                                    obj.insert("server_file_path".to_string(), json!(clean));
                                }
                            }
                        }
                    }
                }
            }
            (StatusCode::OK, Json(json!({ "success": true, "result": val }))).into_response()
        }
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "success": false, "error": err }))).into_response(),
    }
}

/// Handler for `POST /api/calls/:id/cancel`
async fn api_cancel_call(
    State(state): State<McpHttpState>,
    Extension(role): Extension<Role>,
    token_ext: Option<Extension<String>>,
    headers: HeaderMap,
    Path(call_id): Path<String>,
) -> impl IntoResponse {
    let client_ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let token_prefix = token_ext.as_ref().map(|Extension(t)| crate::audit::AuditLogger::redact_token(t));

    if role == Role::Viewer {
        if let Some(logger) = state.router.audit_logger() {
            logger.log(crate::audit::AuditRecord {
                id: format!("audit-{}", crate::router::generate_call_id()),
                timestamp: chrono::Utc::now().to_rfc3339(),
                role: Some(role.to_string()),
                token_prefix,
                client_ip,
                terminal_id: None,
                action: format!("api:cancel_call:{}", call_id),
                tool_name: None,
                arguments: None,
                status: "DENIED".to_string(),
                error: Some("Forbidden: Viewer role cannot cancel tool calls".to_string()),
                duration_ms: Some(0),
            });
        }
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "success": false, "error": "Forbidden: Viewer role cannot cancel tool calls" })),
        );
    }
    match state.router.cancel_tool_call(&call_id).await {
        Ok(()) => {
            if let Some(logger) = state.router.audit_logger() {
                logger.log(crate::audit::AuditRecord {
                    id: format!("audit-{}", crate::router::generate_call_id()),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    role: Some(role.to_string()),
                    token_prefix,
                    client_ip,
                    terminal_id: None,
                    action: format!("api:cancel_call:{}", call_id),
                    tool_name: None,
                    arguments: None,
                    status: "SUCCESS".to_string(),
                    error: None,
                    duration_ms: Some(0),
                });
            }
            (
                StatusCode::OK,
                Json(json!({ "success": true, "message": format!("Tool call '{}' cancelled", call_id) })),
            )
        }
        Err(err) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": err })),
        ),
    }
}

/// Handler for `POST /api/terminals/:id/calls/:call_id/cancel`
async fn api_cancel_terminal_call(
    State(state): State<McpHttpState>,
    Extension(role): Extension<Role>,
    token_ext: Option<Extension<String>>,
    headers: HeaderMap,
    Path((terminal_id, call_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let client_ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let token_prefix = token_ext.as_ref().map(|Extension(t)| crate::audit::AuditLogger::redact_token(t));

    if role == Role::Viewer {
        if let Some(logger) = state.router.audit_logger() {
            logger.log(crate::audit::AuditRecord {
                id: format!("audit-{}", crate::router::generate_call_id()),
                timestamp: chrono::Utc::now().to_rfc3339(),
                role: Some(role.to_string()),
                token_prefix,
                client_ip,
                terminal_id: Some(terminal_id),
                action: format!("api:cancel_call:{}", call_id),
                tool_name: None,
                arguments: None,
                status: "DENIED".to_string(),
                error: Some("Forbidden: Viewer role cannot cancel tool calls".to_string()),
                duration_ms: Some(0),
            });
        }
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "success": false, "error": "Forbidden: Viewer role cannot cancel tool calls" })),
        );
    }
    match state.router.cancel_tool_call(&call_id).await {
        Ok(()) => {
            if let Some(logger) = state.router.audit_logger() {
                logger.log(crate::audit::AuditRecord {
                    id: format!("audit-{}", crate::router::generate_call_id()),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    role: Some(role.to_string()),
                    token_prefix,
                    client_ip,
                    terminal_id: Some(terminal_id),
                    action: format!("api:cancel_call:{}", call_id),
                    tool_name: None,
                    arguments: None,
                    status: "SUCCESS".to_string(),
                    error: None,
                    duration_ms: Some(0),
                });
            }
            (
                StatusCode::OK,
                Json(json!({ "success": true, "message": format!("Tool call '{}' cancelled", call_id) })),
            )
        }
        Err(err) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": err })),
        ),
    }
}

/// Handler for `GET /api/calls`
async fn api_list_pending_calls(State(state): State<McpHttpState>) -> impl IntoResponse {
    let calls = state.router.list_pending_call_details(None);
    Json(json!({ "success": true, "calls": calls }))
}

/// Handler for `GET /api/terminals/:id/calls`
async fn api_list_terminal_pending_calls(
    State(state): State<McpHttpState>,
    Path(terminal_id): Path<String>,
) -> impl IntoResponse {
    let calls = state.router.list_pending_call_details(Some(&terminal_id));
    Json(json!({ "success": true, "calls": calls }))
}

#[derive(Deserialize)]
struct StartStreamRequest {
    fps: Option<u32>,
    quality: Option<u8>,
    display_index: Option<u32>,
}

/// Handler for `POST /api/terminals/:id/desktop/stream`
async fn api_start_desktop_stream(
    State(state): State<McpHttpState>,
    Path(terminal_id): Path<String>,
    payload: Option<Json<StartStreamRequest>>,
) -> impl IntoResponse {
    let req = payload
        .map(|j| j.0)
        .unwrap_or(StartStreamRequest {
            fps: Some(15),
            quality: Some(60),
            display_index: Some(0),
        });
    let fps = req.fps.unwrap_or(15);
    let quality = req.quality.unwrap_or(60);
    let display_index = req.display_index.unwrap_or(0);
    match state
        .router
        .start_desktop_stream(&terminal_id, fps, quality, display_index)
        .await
    {
        Ok(()) => Json(json!({ "success": true, "message": "Stream started" })),
        Err(e) => Json(json!({ "success": false, "error": e })),
    }
}

/// Handler for `POST /api/terminals/:id/desktop/stop`
async fn api_stop_desktop_stream(
    State(state): State<McpHttpState>,
    Path(terminal_id): Path<String>,
) -> impl IntoResponse {
    match state.router.stop_desktop_stream(&terminal_id).await {
        Ok(()) => Json(json!({ "success": true, "message": "Stream stopped" })),
        Err(e) => Json(json!({ "success": false, "error": e })),
    }
}

/// Handler for `GET /api/terminals/:id/desktop/frame`
async fn api_get_desktop_frame(
    State(state): State<McpHttpState>,
    Path(terminal_id): Path<String>,
) -> impl IntoResponse {
    if let Some(frame) = state.router.get_latest_desktop_frame(&terminal_id).await {
        Json(json!({
            "success": true,
            "display_index": frame.display_index,
            "width": frame.width,
            "height": frame.height,
            "format": frame.format,
            "data": frame.data,
            "timestamp": frame.timestamp,
        }))
    } else {
        Json(json!({ "success": false, "error": "No frame available yet" }))
    }
}

/// Handler for `GET /api/terminals/:id/desktop/frame.jpg` and `/raw` to stream binary JPEG frames without base64
async fn api_get_desktop_frame_raw(
    State(state): State<McpHttpState>,
    Path(terminal_id): Path<String>,
) -> Response {
    if let Some((display_index, width, height, timestamp, raw_bytes)) =
        state.router.get_latest_desktop_frame_raw(&terminal_id).await
    {
        (
            StatusCode::OK,
            [
                ("Content-Type", "image/jpeg"),
                ("X-Display-Index", &display_index.to_string()),
                ("X-Frame-Width", &width.to_string()),
                ("X-Frame-Height", &height.to_string()),
                ("X-Frame-Timestamp", &timestamp.to_string()),
            ],
            raw_bytes,
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            [("Content-Type", "text/plain; charset=utf-8")],
            "No frame available yet",
        )
            .into_response()
    }
}

/// Handler for `POST /api/terminals/:id/desktop/input`
async fn api_send_desktop_input(
    State(state): State<McpHttpState>,
    Extension(role): Extension<Role>,
    token_ext: Option<Extension<String>>,
    headers: HeaderMap,
    Path(terminal_id): Path<String>,
    Json(event): Json<at_pc_protocol::models::DesktopInputEvent>,
) -> Response {
    let client_ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let token_prefix = token_ext.as_ref().map(|Extension(t)| crate::audit::AuditLogger::redact_token(t));

    if role != Role::Admin {
        if let Some(logger) = state.router.audit_logger() {
            logger.log(crate::audit::AuditRecord {
                id: format!("audit-{}", crate::router::generate_call_id()),
                timestamp: chrono::Utc::now().to_rfc3339(),
                role: Some(role.to_string()),
                token_prefix,
                client_ip,
                terminal_id: Some(terminal_id.clone()),
                action: "api:desktop_input".to_string(),
                tool_name: None,
                arguments: serde_json::to_value(&event).ok(),
                status: "DENIED".to_string(),
                error: Some("Forbidden: Only Admin role can send remote desktop input".to_string()),
                duration_ms: Some(0),
            });
        }
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "success": false, "error": "Forbidden: Only Admin role can send remote desktop input" })),
        )
            .into_response();
    }

    match state.router.send_desktop_input(&terminal_id, event.clone()).await {
        Ok(()) => {
            if let Some(logger) = state.router.audit_logger() {
                logger.log(crate::audit::AuditRecord {
                    id: format!("audit-{}", crate::router::generate_call_id()),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    role: Some(role.to_string()),
                    token_prefix,
                    client_ip,
                    terminal_id: Some(terminal_id),
                    action: "api:desktop_input".to_string(),
                    tool_name: None,
                    arguments: serde_json::to_value(&event).ok(),
                    status: "SUCCESS".to_string(),
                    error: None,
                    duration_ms: Some(0),
                });
            }
            (StatusCode::OK, Json(json!({ "success": true }))).into_response()
        }
        Err(e) => {
            if let Some(logger) = state.router.audit_logger() {
                logger.log(crate::audit::AuditRecord {
                    id: format!("audit-{}", crate::router::generate_call_id()),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    role: Some(role.to_string()),
                    token_prefix,
                    client_ip,
                    terminal_id: Some(terminal_id.clone()),
                    action: "api:desktop_input".to_string(),
                    tool_name: None,
                    arguments: serde_json::to_value(&event).ok(),
                    status: "FAILED".to_string(),
                    error: Some(e.clone()),
                    duration_ms: Some(0),
                });
            }
            tracing::warn!("api_send_desktop_input failed for [{}]: {}", terminal_id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "success": false, "error": e }))).into_response()
        }
    }
}

#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<usize>,
}

/// Handler for `GET /api/audit`
async fn api_get_audit_logs(
    State(state): State<McpHttpState>,
    Extension(role): Extension<Role>,
    query: Option<Query<AuditQuery>>,
) -> impl IntoResponse {
    if role == Role::Viewer {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "success": false, "error": "Forbidden: Viewer role cannot view audit trail" })),
        );
    }
    let limit = query.and_then(|q| q.0.limit).unwrap_or(100);
    let records = state
        .router
        .audit_logger()
        .map(|l| l.read_recent(limit))
        .unwrap_or_default();
    (StatusCode::OK, Json(json!({ "success": true, "records": records })))
}
