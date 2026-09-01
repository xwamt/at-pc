//! MCP JSON-RPC router and endpoint handlers.

use crate::server::sse::{extract_client_ip, is_request_authenticated, sse_handler};
use crate::server::state::{AppState, AuditLogEntry, AuditLogStatus};
use crate::tools::{dispatch_mcp_tool, get_mcp_tool_definitions};
use axum::{
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;
use tower_http::cors::CorsLayer;

/// Builds the Axum router for the MCP HTTP/SSE server.
pub fn create_mcp_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/sse", get(sse_handler))
        .route("/messages", post(messages_handler))
        .route("/health", get(health_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Authenticated health and status check endpoint.
async fn health_handler(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Response {
    let headers = req.headers().clone();
    let query_str = req.uri().query().map(|s| s.to_string());

    if !is_request_authenticated(&state, &headers, query_str.as_deref()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Unauthorized: Invalid or missing PIN"
            })),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "service": "at-pc",
            "port": state.port,
            "connected_clients": state.connected_client_count(),
        })),
    )
        .into_response()
}

/// JSON-RPC message handler (`POST /messages`).
pub async fn messages_handler(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Response {
    let headers = req.headers().clone();
    let query_str = req.uri().query().map(|s| s.to_string());
    let client_ip = extract_client_ip(&req);

    // 1. Authenticate request before parsing body
    if !is_request_authenticated(&state, &headers, query_str.as_deref()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32000,
                    "message": "Unauthorized: Invalid or missing PIN"
                }
            })),
        )
            .into_response();
    }

    // 2. Read request body bytes
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

    // 3. Parse JSON-RPC payload
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

    // 4. Handle batch or single JSON-RPC request
    if let Some(arr) = payload.as_array() {
        let mut responses = Vec::new();
        for item in arr {
            if let Some(resp) = handle_single_jsonrpc_request(&state, item, &client_ip) {
                responses.push(resp);
            }
        }
        (StatusCode::OK, Json(json!(responses))).into_response()
    } else if let Some(resp) = handle_single_jsonrpc_request(&state, &payload, &client_ip) {
        (StatusCode::OK, Json(resp)).into_response()
    } else {
        (StatusCode::ACCEPTED, Json(json!({}))).into_response()
    }
}

/// Processes a single JSON-RPC 2.0 request.
fn handle_single_jsonrpc_request(state: &AppState, request: &Value, client_ip: &str) -> Option<Value> {
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
                    "name": "at-pc",
                    "version": "0.1.0"
                }
            });
            Ok(res)
        }

        "notifications/initialized" => {
            // Notification from client: no response required in standard JSON-RPC if id is null
            let _ = id.as_ref()?;
            Ok(json!({}))
        }

        "ping" => Ok(json!({})),

        "tools/list" => {
            let tools = get_mcp_tool_definitions();
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

            let start = Instant::now();
            let dispatch_result = dispatch_mcp_tool(tool_name, tool_args.clone());
            let duration_ms = start.elapsed().as_millis() as u64;

            match dispatch_result {
                Ok(val) => {
                    let text = if let Some(s) = val.as_str() {
                        s.to_string()
                    } else {
                        serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string())
                    };

                    state.broadcast_audit(AuditLogEntry::new(
                        tool_name,
                        tool_args,
                        AuditLogStatus::Success,
                        Some(duration_ms),
                        Some(client_ip.to_string()),
                        None,
                    ));

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
                    state.broadcast_audit(AuditLogEntry::new(
                        tool_name,
                        tool_args,
                        AuditLogStatus::Error,
                        Some(duration_ms),
                        Some(client_ip.to_string()),
                        Some(err.clone()),
                    ));

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

    // If id is null (notification) and no error, return None
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
