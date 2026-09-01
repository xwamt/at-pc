//! Server-Sent Events (SSE) transport implementation for MCP.

use crate::server::auth::validate_auth_header;
use crate::server::state::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
};
use futures_core::Stream;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

/// Extracts `token` or `pin` value from URL query string if present.
pub fn extract_token_from_query(raw_query: Option<&str>) -> Option<String> {
    let q = raw_query?;
    for pair in q.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k.eq_ignore_ascii_case("token") || k.eq_ignore_ascii_case("pin") {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Helper to verify request authentication via Authorization header or URL query token.
pub fn is_request_authenticated(
    state: &AppState,
    headers: &HeaderMap,
    raw_query: Option<&str>,
) -> bool {
    if state.is_stopped() {
        return false;
    }

    let current_pin = state.get_pin();
    if current_pin.is_empty() {
        return false;
    }

    let auth_header = headers.get("Authorization").and_then(|v| v.to_str().ok());
    if validate_auth_header(&current_pin, auth_header) {
        return true;
    }

    if let Some(token) = extract_token_from_query(raw_query) {
        if state.verify_pin(&token) {
            return true;
        }
    }

    false
}

/// RAII Guard that decrements connected clients counter on stream drop.
struct ClientSessionGuard {
    state: Arc<AppState>,
}

impl Drop for ClientSessionGuard {
    fn drop(&mut self) {
        self.state.decrement_clients();
    }
}

/// Stream wrapping mpsc receiver, sender reference, and the RAII guard.
struct SseStreamWithGuard {
    rx: mpsc::Receiver<Event>,
    _tx: mpsc::Sender<Event>,
    _guard: ClientSessionGuard,
}

impl Stream for SseStreamWithGuard {
    type Item = Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(event)) => Poll::Ready(Some(Ok(event))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// SSE endpoint handler (`GET /sse`).
pub async fn sse_handler(
    State(state): State<Arc<AppState>>,
    req: axum::extract::Request,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, &'static str)> {
    if !is_request_authenticated(&state, req.headers(), req.uri().query()) {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized: Invalid or missing PIN"));
    }

    // Increment active client count
    state.increment_clients();

    let (tx, rx) = mpsc::channel::<Event>(32);

    // Send initial endpoint announcement
    let endpoint_uri = format!("/messages?token={}", state.get_pin());
    let initial_event = Event::default().event("endpoint").data(endpoint_uri);
    let _ = tx.send(initial_event).await;

    let stream = SseStreamWithGuard {
        rx,
        _tx: tx,
        _guard: ClientSessionGuard {
            state: state.clone(),
        },
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
