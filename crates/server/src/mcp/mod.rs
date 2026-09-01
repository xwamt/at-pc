pub mod tools;

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
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
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tracing::{debug, info};

use crate::config::ServerConfig;
use crate::router::McpRouter;

/// Server state shared across MCP HTTP/SSE endpoints
#[derive(Clone)]
pub struct McpHttpState {
    pub router: Arc<McpRouter>,
    pub config: ServerConfig,
}

/// Create Axum router for the MCP HTTP/SSE gateway
pub fn create_mcp_http_router(router: Arc<McpRouter>, config: ServerConfig) -> Router {
    let state = McpHttpState { router, config };
    Router::new()
        .route("/sse", get(sse_handler))
        .route("/messages", post(messages_handler))
        .route("/health", get(health_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Check request authentication using ServerConfig.auth_token
fn is_request_authenticated(config: &ServerConfig, headers: &HeaderMap, query: Option<&str>) -> bool {
    let expected = match &config.auth_token {
        Some(t) if !t.is_empty() => t,
        _ => return true,
    };

    if let Some(auth_header) = headers.get("Authorization").and_then(|v| v.to_str().ok()) {
        let clean = auth_header.trim();
        if clean.eq_ignore_ascii_case(expected) {
            return true;
        }
        if let Some(bearer) = clean.strip_prefix("Bearer ") {
            if bearer.trim().eq_ignore_ascii_case(expected) {
                return true;
            }
        }
    }

    if let Some(q) = query {
        for pair in q.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if (k.eq_ignore_ascii_case("token") || k.eq_ignore_ascii_case("pin"))
                    && v.eq_ignore_ascii_case(expected)
                {
                    return true;
                }
            }
        }
    }

    false
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
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized: Invalid or missing token"));
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

    let stream = SseStream { rx };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// JSON-RPC message handler (`POST /messages`)
async fn messages_handler(State(state): State<McpHttpState>, req: Request) -> Response {
    if !is_request_authenticated(&state.config, req.headers(), req.uri().query()) {
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

    if let Some(arr) = payload.as_array() {
        let mut responses = Vec::new();
        for item in arr {
            if let Some(resp) = handle_jsonrpc_request(&state.router, item).await {
                responses.push(resp);
            }
        }
        (StatusCode::OK, Json(json!(responses))).into_response()
    } else if let Some(resp) = handle_jsonrpc_request(&state.router, &payload).await {
        (StatusCode::OK, Json(resp)).into_response()
    } else {
        (StatusCode::ACCEPTED, Json(json!({}))).into_response()
    }
}

/// Processes a single JSON-RPC 2.0 request
pub async fn handle_jsonrpc_request(router: &Arc<McpRouter>, request: &Value) -> Option<Value> {
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

            let dispatch_result = router.dispatch_tool_call(tool_name, tool_args).await;

            match dispatch_result {
                Ok(val) => {
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
                Err(err) => {
                    Ok(json!({
                        "content": [
                            {
                                "type": "text",
                                "text": format!("Error: {}", err)
                            }
                        ],
                        "isError": true
                    }))
                }
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
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    debug!("Starting MCP server over stdio");

    while let Some(line) = reader.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Ok(json_val) = serde_json::from_str::<Value>(trimmed) {
            if let Some(arr) = json_val.as_array() {
                let mut responses = Vec::new();
                for item in arr {
                    if let Some(resp) = handle_jsonrpc_request(&router, item).await {
                        responses.push(resp);
                    }
                }
                if !responses.is_empty() {
                    let out = serde_json::to_string(&responses)? + "\n";
                    stdout.write_all(out.as_bytes()).await?;
                    stdout.flush().await?;
                }
            } else if let Some(resp) = handle_jsonrpc_request(&router, &json_val).await {
                let out = serde_json::to_string(&resp)? + "\n";
                stdout.write_all(out.as_bytes()).await?;
                stdout.flush().await?;
            }
        }
    }

    Ok(())
}

/// Start MCP HTTP/SSE server on configured port
pub async fn start_mcp_http_server(
    router: Arc<McpRouter>,
    config: ServerConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = config.mcp_port;
    let app = create_mcp_http_router(router, config);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    info!("MCP HTTP/SSE gateway listening on http://0.0.0.0:{}", port);

    axum::serve(listener, app).await?;
    Ok(())
}
