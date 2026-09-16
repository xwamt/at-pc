pub mod dashboard;
pub mod tools;

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::Stream;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tracing::debug;

use crate::config::ServerConfig;
use crate::router::McpRouter;

/// Server state shared across MCP HTTP/SSE endpoints
#[derive(Clone)]
pub struct McpHttpState {
    pub router: Arc<McpRouter>,
    pub config: ServerConfig,
    pub log_file_path: std::path::PathBuf,
}

/// Returns the server log path used by both the tracing writer and dashboard tail endpoint.
pub fn default_log_file_path() -> std::path::PathBuf {
    match std::env::current_dir() {
        Ok(path) if path != std::path::Path::new("/") => path.join("at-pc-server.log"),
        _ => std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|dir| dir.join("at-pc-server.log")))
            .unwrap_or_else(|| std::path::PathBuf::from("at-pc-server.log")),
    }
}

/// Create Axum router for the MCP HTTP/SSE gateway
pub fn create_mcp_http_router(router: Arc<McpRouter>, config: ServerConfig) -> Router {
    let cors = if config.allowed_origins.iter().any(|o| o == "*") {
        CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::DELETE,
                axum::http::Method::OPTIONS,
            ])
            .allow_headers([
                axum::http::header::AUTHORIZATION,
                axum::http::header::CONTENT_TYPE,
                axum::http::header::ACCEPT,
                axum::http::header::COOKIE,
                axum::http::HeaderName::from_static("mcp-session-id"),
                axum::http::HeaderName::from_static("x-session-id"),
            ])
    } else if !config.allowed_origins.is_empty() {
        let origins: Vec<axum::http::HeaderValue> = config
            .allowed_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::DELETE,
                axum::http::Method::OPTIONS,
            ])
            .allow_headers([
                axum::http::header::AUTHORIZATION,
                axum::http::header::CONTENT_TYPE,
                axum::http::header::ACCEPT,
                axum::http::header::COOKIE,
                axum::http::HeaderName::from_static("mcp-session-id"),
                axum::http::HeaderName::from_static("x-session-id"),
            ])
            .allow_credentials(true)
    } else {
        CorsLayer::new()
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::DELETE,
                axum::http::Method::OPTIONS,
            ])
            .allow_headers([
                axum::http::header::AUTHORIZATION,
                axum::http::header::CONTENT_TYPE,
                axum::http::header::ACCEPT,
                axum::http::header::COOKIE,
                axum::http::HeaderName::from_static("mcp-session-id"),
                axum::http::HeaderName::from_static("x-session-id"),
            ])
    };

    let state = McpHttpState {
        router,
        config: config.clone(),
        log_file_path: default_log_file_path(),
    };

    Router::new()
        .merge(dashboard::create_dashboard_router(state.clone()))
        .route("/", post(messages_handler))
        .route("/sse", get(sse_handler).post(messages_handler))
        .route("/messages", post(messages_handler).get(sse_handler))
        .route("/mcp", post(messages_handler).get(sse_handler))
        .route("/health", get(health_handler))
        .layer(cors)
        .with_state(state)
}

/// Axum middleware that enforces authentication and RBAC roles on API requests
pub async fn auth_middleware(
    State(state): State<McpHttpState>,
    mut req: Request,
    next: axum::middleware::Next,
) -> Response {
    // OPTIONS preflight requests must pass through to CORS layer unhindered
    if req.method() == axum::http::Method::OPTIONS {
        return next.run(req).await;
    }

    let token_opt = extract_auth_token(req.headers(), req.uri().query());
    let role_opt = match &token_opt {
        Some(token) => state.config.get_role_for_token(token),
        None => {
            if state.config.auth_token.is_none() && state.config.roles.is_empty() {
                Some(crate::config::Role::Admin)
            } else {
                None
            }
        }
    };

    let role = match role_opt {
        Some(r) => r,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Unauthorized: Invalid or missing authentication token" })),
            )
                .into_response();
        }
    };

    req.extensions_mut().insert(role);
    if let Some(t) = token_opt {
        req.extensions_mut().insert(t);
    }

    next.run(req).await
}

/// Extracts raw authentication token from Authorization header, Cookie, or Query parameter
pub fn extract_auth_token(headers: &HeaderMap, query: Option<&str>) -> Option<String> {
    // 1. Check Authorization: Bearer <token> or Authorization: <token>
    if let Some(auth_header) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        let clean = auth_header.trim();
        if clean.len() > 7
            && clean[..6].eq_ignore_ascii_case("bearer")
            && clean.as_bytes()[6].is_ascii_whitespace()
        {
            let t = clean[7..].trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        } else if !clean.is_empty() {
            return Some(clean.to_string());
        }
    }

    // 2. Check Cookie: token=<token> or Cookie: auth_token=<token>
    if let Some(cookie_header) = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
    {
        for pair in cookie_header.split(';') {
            if let Some((k, v)) = pair.trim().split_once('=') {
                if k.eq_ignore_ascii_case("token") || k.eq_ignore_ascii_case("auth_token") {
                    let val = v.trim();
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
        }
    }

    // 3. Check URL query string (?token=... or ?pin=...)
    if let Some(q) = query {
        for pair in q.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if k.eq_ignore_ascii_case("token") || k.eq_ignore_ascii_case("pin") {
                    let decoded = percent_decode(v).unwrap_or_else(|| v.to_string());
                    let clean = decoded.trim();
                    if !clean.is_empty() {
                        return Some(clean.to_string());
                    }
                }
            }
        }
    }

    None
}

/// Check request authentication using ServerConfig roles and auth_token
pub fn is_request_authenticated(
    config: &ServerConfig,
    headers: &HeaderMap,
    query: Option<&str>,
) -> bool {
    if config.auth_token.is_none() && config.roles.is_empty() {
        return true;
    }
    match extract_auth_token(headers, query) {
        Some(token) => config.get_role_for_token(&token).is_some(),
        None => false,
    }
}

fn percent_decode(input: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(input.len());
    let mut chars = input.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars.next()?;
            let h2 = chars.next()?;
            let hex = [h1, h2];
            let hex_str = std::str::from_utf8(&hex).ok()?;
            let val = u8::from_str_radix(hex_str, 16).ok()?;
            bytes.push(val);
        } else if b == b'+' {
            bytes.push(b' ');
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8(bytes).ok()
}

/// Health check endpoint
async fn health_handler(State(state): State<McpHttpState>, req: Request) -> Response {
    if !is_request_authenticated(&state.config, req.headers(), req.uri().query()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Unauthorized: Invalid or missing authentication token" })),
        )
            .into_response();
    }

    let terminals = state.router.list_terminals().await;
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "service": "at-pc-server",
            "version": env!("CARGO_PKG_VERSION"),
            "mcp_port": state.config.mcp_port,
            "ws_port": state.config.ws_port,
            "connected_terminals": terminals.len(),
        })),
    )
        .into_response()
}

/// SseStream wrapper for SSE events
struct SseStream {
    rx: mpsc::Receiver<Event>,
    _tx: mpsc::Sender<Event>,
}

impl Stream for SseStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(event)) => Poll::Ready(Some(Ok(event))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// SSE connection handler (`GET /sse`)
async fn sse_handler(
    State(state): State<McpHttpState>,
    req: Request,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, &'static str)> {
    if !is_request_authenticated(&state.config, req.headers(), req.uri().query()) {
        return Err((
            StatusCode::UNAUTHORIZED,
            "Unauthorized: Invalid or missing token",
        ));
    }

    let (tx, rx) = mpsc::channel::<Event>(32);

    let token_param = if let Some(token) = &state.config.auth_token {
        format!("?token={}", token)
    } else {
        String::new()
    };
    let endpoint_uri = format!("/messages{}", token_param);
    let initial_event = Event::default().event("endpoint").data(endpoint_uri);
    let _ = tx.send(initial_event).await;

    let stream = SseStream { rx, _tx: tx };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// JSON-RPC message handler (`POST /messages`)
async fn messages_handler(State(state): State<McpHttpState>, req: Request) -> Response {
    let token_opt = extract_auth_token(req.headers(), req.uri().query());
    let role = match &token_opt {
        Some(token) => match state.config.get_role_for_token(token) {
            Some(r) => r,
            None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32000,
                            "message": "Unauthorized: Invalid token"
                        }
                    })),
                )
                    .into_response();
            }
        },
        None => {
            if state.config.auth_token.is_none() && state.config.roles.is_empty() {
                crate::config::Role::Admin
            } else {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32000,
                            "message": "Unauthorized: Invalid or missing token"
                        }
                    })),
                )
                    .into_response();
            }
        }
    };

    let token_prefix = token_opt
        .as_deref()
        .map(crate::audit::AuditLogger::redact_token);
    let client_ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .or_else(|| req.headers().get("x-real-ip").and_then(|v| v.to_str().ok()))
        .map(|s| s.to_string());

    let session_id = req
        .headers()
        .get("mcp-session-id")
        .or_else(|| req.headers().get("x-session-id"))
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            req.uri().query().and_then(|q| {
                for pair in q.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        if k.eq_ignore_ascii_case("session_id") {
                            return Some(v.to_string());
                        }
                    }
                }
                None
            })
        });

    let bytes = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32700,
                        "message": format!("Failed to read request body: {}", e)
                    }
                })),
            )
                .into_response();
        }
    };

    let payload: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    }
                })),
            )
                .into_response();
        }
    };

    let effective_session_id = session_id.or_else(|| {
        if payload.get("method").and_then(|m| m.as_str()) == Some("initialize") {
            Some(format!("sess-{:x}", chrono::Utc::now().timestamp_micros()))
        } else {
            None
        }
    });

    let mut response = if let Some(arr) = payload.as_array() {
        let mut responses = Vec::new();
        for item in arr {
            if let Some(resp) = handle_jsonrpc_request_with_context(
                &state.router,
                item,
                effective_session_id.as_deref(),
                Some(role),
                client_ip.as_deref(),
                token_prefix.as_deref(),
            )
            .await
            {
                responses.push(resp);
            }
        }
        (StatusCode::OK, Json(json!(responses))).into_response()
    } else if let Some(resp) = handle_jsonrpc_request_with_context(
        &state.router,
        &payload,
        effective_session_id.as_deref(),
        Some(role),
        client_ip.as_deref(),
        token_prefix.as_deref(),
    )
    .await
    {
        (StatusCode::OK, Json(resp)).into_response()
    } else {
        (StatusCode::ACCEPTED, Json(json!({}))).into_response()
    };

    if let Some(ref sid) = effective_session_id {
        if let Ok(val) = axum::http::HeaderValue::from_str(sid) {
            response.headers_mut().insert("mcp-session-id", val);
        }
    }

    response
}

/// Processes a single JSON-RPC 2.0 request
pub async fn handle_jsonrpc_request(router: &Arc<McpRouter>, request: &Value) -> Option<Value> {
    handle_jsonrpc_request_with_context(router, request, None, None, None, None).await
}

/// Processes a single JSON-RPC 2.0 request with session isolation context
pub async fn handle_jsonrpc_request_with_session(
    router: &Arc<McpRouter>,
    request: &Value,
    session_id: Option<&str>,
) -> Option<Value> {
    handle_jsonrpc_request_with_context(router, request, session_id, None, None, None).await
}

/// Processes a single JSON-RPC 2.0 request with session, role, and audit context
pub async fn handle_jsonrpc_request_with_context(
    router: &Arc<McpRouter>,
    request: &Value,
    session_id: Option<&str>,
    role: Option<crate::config::Role>,
    client_ip: Option<&str>,
    token_prefix: Option<&str>,
) -> Option<Value> {
    let method = request.get("method").and_then(|m| m.as_str())?;
    let id = request.get("id").cloned();
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    let result = match method {
        "initialize" => {
            let res = json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    },
                    "prompts": {
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": "at-pc-server",
                    "version": env!("CARGO_PKG_VERSION")
                }
            });
            Ok(res)
        }

        "notifications/initialized" => {
            let _ = id.as_ref()?;
            Ok(json!({}))
        }

        "ping" => Ok(json!({})),

        "prompts/list" => Ok(json!({
            "prompts": [
                {
                    "name": "desktop_automation_strategy",
                    "description": "Recommended tier hierarchy and operational best practices for at-pc desktop automation and IT operations.",
                    "arguments": []
                }
            ]
        })),

        "prompts/get" => {
            let prompt_name = params
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or_default();

            match prompt_name {
                "desktop_automation_strategy" => {
                    let text = "# at-pc Desktop Automation & Operations Guidelines\n\
When operating on at-pc agent terminals, follow this 3-tier hierarchy:\n\n\
1. **Tier 1 (Terminal / API First - 100% Accuracy, Zero Vision Tokens)**:\n   \
   - For diagnostics, system queries, service management, configuration editing, or process control:\n     \
     Always use `get_system_overview`, `list_processes`, `manage_service`, `exec_powershell`, `exec_cmd`, `read_text_file`, `write_text_file`.\n   \
   - Never use screenshot or mouse clicks to operate system settings or control panels if an API/command tool is available.\n\n\
2. **Tier 2 (Structured UI Tree / DOM - Token-Efficient, Deterministic Semantic Interaction)**:\n   \
   - When interacting with native desktop applications, call `get_ui_tree` first to inspect the window's interactive element tree.\n   \
   - Use `click_element(element_id)` for background invocation without mouse movement, or center click fallback.\n   \
   - Use `set_element_text(element_id, text)` to set text programmatically without typing lag or keyboard focus drift.\n\n\
3. **Tier 3 (Visual ROI & Set-of-Mark Fallback - Use Only When Needed)**:\n   \
   - Only call `capture_screen` or `get_marked_screen` when interacting with custom-drawn Canvas, games, or legacy non-accessible windows.\n   \
   - Always supply `crop` or `max_dimension` when capturing screens to conserve visual tokens.\n   \
   - Use `get_marked_screen` to obtain visually annotated Set-of-Mark screenshots with numbered badges (#1, #2...).\n   \
   - Directly invoke `click_mark(mark_id)` or `mouse_click(mark_id=N)` to click without calculating pixel coordinates.";

                    Ok(json!({
                        "description": "Recommended tier hierarchy and operational best practices for at-pc desktop automation and IT operations.",
                        "messages": [
                            {
                                "role": "user",
                                "content": {
                                    "type": "text",
                                    "text": text
                                }
                            }
                        ]
                    }))
                }
                unknown => Err(json!({
                    "code": -32602,
                    "message": format!("Prompt '{}' not found", unknown)
                })),
            }
        }

        "tools/list" => {
            let tools = tools::get_mcp_tool_definitions();
            Ok(json!({ "tools": tools }))
        }

        "tools/call" => {
            let tool_name = params
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or_default();

            let tool_args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));

            let dispatch_result = router
                .dispatch_tool_call_with_role(
                    tool_name,
                    tool_args.clone(),
                    session_id,
                    role,
                    client_ip,
                    token_prefix,
                )
                .await;

            match dispatch_result {
                Ok(val) => {
                    // Check if response is from capture_screen or contains image data
                    if tool_name == "capture_screen"
                        || tool_name == "get_marked_screen"
                        || val.get("raw_base64").is_some()
                        || val.get("image_base64").is_some()
                        || val.get("base64_data").is_some()
                    {
                        let b64_raw = val
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

                        if let Some(clean_b64) = b64_raw {
                            // Check if server_save_path argument was provided to save directly on server disk
                            let mut server_saved_note = String::new();
                            if let Some(ssp) =
                                tool_args.get("server_save_path").and_then(|v| v.as_str())
                            {
                                use base64::Engine;
                                if let Ok(decoded_bytes) =
                                    base64::engine::general_purpose::STANDARD.decode(clean_b64)
                                {
                                    let path = std::path::Path::new(ssp.trim());
                                    if let Some(parent) = path.parent() {
                                        if !parent.as_os_str().is_empty() && !parent.exists() {
                                            let _ = std::fs::create_dir_all(parent);
                                        }
                                    }
                                    match std::fs::write(path, &decoded_bytes) {
                                        Ok(_) => {
                                            server_saved_note = format!(
                                                "\nSaved to server host disk: {}",
                                                ssp.trim()
                                            );
                                        }
                                        Err(e) => {
                                            server_saved_note = format!("\nWarning: Failed to save to server host disk '{}': {}", ssp.trim(), e);
                                        }
                                    }
                                }
                            }

                            let fmt = val
                                .get("format")
                                .and_then(|v| v.as_str())
                                .unwrap_or("jpeg")
                                .to_lowercase();
                            let mime_type = if fmt == "png" {
                                "image/png"
                            } else {
                                "image/jpeg"
                            };

                            let width = val.get("width").and_then(|v| v.as_u64()).unwrap_or(0);
                            let height = val.get("height").and_then(|v| v.as_u64()).unwrap_or(0);
                            let display_idx = val
                                .get("display_index")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let file_path_note = val
                                .get("file_path")
                                .and_then(|v| v.as_str())
                                .map(|p| format!("\nSaved to: {}", p))
                                .unwrap_or_default();

                            let scale_note = if let (Some(ow), Some(oh)) = (
                                val.get("original_width").and_then(|v| v.as_u64()),
                                val.get("original_height").and_then(|v| v.as_u64()),
                            ) {
                                format!(" (downscaled from {}x{})", ow, oh)
                            } else {
                                String::new()
                            };

                            let crop_note =
                                if let Some(crop) = val.get("crop").and_then(|v| v.as_array()) {
                                    if crop.len() == 4 {
                                        format!(
                                            " (ROI crop: [{}, {}, {}, {}])",
                                            crop[0].as_u64().unwrap_or(0),
                                            crop[1].as_u64().unwrap_or(0),
                                            crop[2].as_u64().unwrap_or(0),
                                            crop[3].as_u64().unwrap_or(0),
                                        )
                                    } else {
                                        String::new()
                                    }
                                } else {
                                    String::new()
                                };

                            let text_summary = format!(
                                "Screenshot captured successfully: display {}, resolution {}x{}{}{}, format: {}.{}{}",
                                display_idx, width, height, scale_note, crop_note, mime_type, file_path_note, server_saved_note
                            );

                            Ok(json!({
                                "content": [
                                    {
                                        "type": "text",
                                        "text": text_summary
                                    },
                                    {
                                        "type": "image",
                                        "data": clean_b64,
                                        "mimeType": mime_type
                                    }
                                ],
                                "isError": false
                            }))
                        } else {
                            let text = if let Some(s) = val.as_str() {
                                s.to_string()
                            } else {
                                serde_json::to_string_pretty(&val)
                                    .unwrap_or_else(|_| val.to_string())
                            };

                            Ok(json!({
                                "content": [
                                    {
                                        "type": "text",
                                        "text": text
                                    }
                                ],
                                "isError": false
                            }))
                        }
                    } else {
                        let text = if let Some(s) = val.as_str() {
                            s.to_string()
                        } else {
                            serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string())
                        };

                        Ok(json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": text
                                }
                            ],
                            "isError": false
                        }))
                    }
                }
                Err(err) => Ok(json!({
                    "content": [
                        {
                            "type": "text",
                            "text": format!("Error: {}", err)
                        }
                    ],
                    "isError": true
                })),
            }
        }

        unknown => Err(json!({
            "code": -32601,
            "message": format!("Method '{}' not found", unknown)
        })),
    };

    let req_id = match id {
        Some(id_val) if !id_val.is_null() => id_val,
        _ => return None,
    };

    match result {
        Ok(res) => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": res
        })),
        Err(err_obj) => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "error": err_obj
        })),
    }
}

/// Run standalone MCP server over stdio transport
pub async fn run_stdio_server(
    router: Arc<McpRouter>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    run_stdio_server_with_streams(router, stdin, stdout).await
}

/// Generic implementation of stdio server supporting any AsyncRead + AsyncWrite streams
pub async fn run_stdio_server_with_streams<R, W>(
    router: Arc<McpRouter>,
    reader_stream: R,
    mut writer_stream: W,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let mut lines = BufReader::new(reader_stream).lines();
    let (tx, mut rx) = mpsc::channel::<String>(256);

    let writer_handle = tokio::spawn(async move {
        while let Some(out) = rx.recv().await {
            if let Err(e) = writer_stream.write_all(out.as_bytes()).await {
                tracing::error!("Failed to write to stdio stdout: {}", e);
                break;
            }
            if let Err(e) = writer_stream.flush().await {
                tracing::error!("Failed to flush stdio stdout: {}", e);
                break;
            }
        }
    });

    debug!("Starting MCP server over stdio (async dispatcher)");

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let json_val = match serde_json::from_str::<Value>(trimmed) {
            Ok(v) => v,
            Err(e) => {
                debug!("Failed to parse line as JSON on stdio: {}", e);
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    }
                });
                if let Ok(out) = serde_json::to_string(&err_resp) {
                    let _ = tx.send(out + "\n").await;
                }
                continue;
            }
        };

        let router = router.clone();
        let tx = tx.clone();

        tokio::spawn(async move {
            if let Some(arr) = json_val.as_array() {
                if arr.is_empty() {
                    let err_resp = json!({
                        "jsonrpc": "2.0",
                        "id": Value::Null,
                        "error": {
                            "code": -32600,
                            "message": "Invalid Request: empty batch"
                        }
                    });
                    if let Ok(out) = serde_json::to_string(&err_resp) {
                        let _ = tx.send(out + "\n").await;
                    }
                } else {
                    let mut responses = Vec::new();
                    for item in arr {
                        if let Some(resp) = handle_jsonrpc_request(&router, item).await {
                            responses.push(resp);
                        }
                    }
                    if !responses.is_empty() {
                        if let Ok(out) = serde_json::to_string(&responses) {
                            let _ = tx.send(out + "\n").await;
                        }
                    }
                }
            } else if let Some(resp) = handle_jsonrpc_request(&router, &json_val).await {
                if let Ok(out) = serde_json::to_string(&resp) {
                    let _ = tx.send(out + "\n").await;
                }
            }
        });
    }

    drop(tx);
    let _ = writer_handle.await;
    Ok(())
}
