//! P1-6 PART A: graceful HTTP shutdown, background task abort, persistence flush.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::routing::get;
use axum::Router;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use at_pc_server::audit::{AuditLogger, AuditRecord};
use at_pc_server::meta_store::TerminalMetaStore;
use at_pc_server::{
    abort_and_join_tasks, complete_or_hold_on_error, serve_with_graceful_shutdown,
    shutdown_persistence, shutdown_signal, GRACEFUL_SHUTDOWN_TIMEOUT,
};

fn unique_path(prefix: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("at-pc-p16-{prefix}-{}-{nonce}", std::process::id()))
}

async fn http_get(addr: std::net::SocketAddr, path: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.expect("read");
    String::from_utf8_lossy(&buf).to_string()
}

#[test]
fn main_wires_signal_and_graceful_shutdown() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs"));
    assert!(
        src.contains("shutdown_signal") || src.contains("tokio::signal"),
        "server must wait on tokio::signal"
    );
    assert!(
        src.contains("serve_with_graceful_shutdown")
            || src.contains("with_graceful_shutdown")
            || src.contains("start_mcp_http_server_with_shutdown"),
        "HTTP serve path must use axum graceful shutdown"
    );
    assert!(
        src.contains("abort_and_join_tasks") || src.contains(".abort()"),
        "sweep/WS JoinHandles must be aborted on shutdown"
    );
    assert!(
        src.contains("shutdown_persistence")
            || src.contains("meta_store.shutdown")
            || src.contains("shutdown_async"),
        "persistence writers must flush/shutdown"
    );
}

#[test]
fn shutdown_signal_is_public() {
    // Ensures the helper exists as a named Future factory for axum.
    let _ = shutdown_signal;
}

#[tokio::test]
async fn http_server_drains_and_stops_on_shutdown_signal() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel::<()>();

    let app = Router::new().route("/ping", get(|| async { "pong" }));
    let server = tokio::spawn(async move {
        serve_with_graceful_shutdown(listener, app, async move {
            let _ = rx.await;
        })
        .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    let body = http_get(addr, "/ping").await;
    assert!(body.contains("pong"), "live server must answer: {body}");

    tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("server must stop after shutdown")
        .expect("server task must not panic")
        .expect("serve_with_graceful_shutdown must succeed");

    let connect = tokio::time::timeout(
        Duration::from_millis(400),
        tokio::net::TcpStream::connect(addr),
    )
    .await;
    if let Ok(Ok(_)) = connect {
        panic!("listener must not accept new connections after shutdown");
    }
}

#[tokio::test]
async fn abort_and_join_stops_background_tasks_promptly() {
    let handle = tokio::spawn(async {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
    let start = Instant::now();
    abort_and_join_tasks(std::iter::once(handle)).await;
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "abort/join must not wait on the task sleep"
    );
}

#[tokio::test]
async fn shutdown_persistence_flushes_meta_and_audit() {
    let meta_path = unique_path("meta.json");
    let audit_path = unique_path("audit.jsonl");
    let meta = TerminalMetaStore::new(&meta_path);
    let logger = AuditLogger::new(Some(audit_path.clone()));

    meta.update_meta("term-p16", Some("graceful".into()), None, None)
        .await
        .unwrap();
    logger.log(AuditRecord {
        id: "p16-audit".to_string(),
        timestamp: "2026-09-15T00:00:00Z".to_string(),
        role: None,
        token_prefix: None,
        client_ip: None,
        terminal_id: Some("term-p16".into()),
        action: "shutdown_test".to_string(),
        tool_name: None,
        arguments: None,
        status: "SUCCESS".to_string(),
        error: None,
        duration_ms: None,
    });

    shutdown_persistence(&meta, Some(&logger))
        .await
        .expect("persistence shutdown");

    let meta_bytes = std::fs::read_to_string(&meta_path).expect("meta file");
    assert!(
        meta_bytes.contains("term-p16") && meta_bytes.contains("graceful"),
        "meta store must flush before shutdown: {meta_bytes}"
    );
    let audit_bytes = std::fs::read_to_string(&audit_path).expect("audit file");
    assert!(
        audit_bytes.contains("p16-audit"),
        "audit logger must flush before shutdown: {audit_bytes}"
    );

    let _ = std::fs::remove_file(meta_path);
    let _ = std::fs::remove_file(audit_path);
}

#[test]
fn mcp_http_starter_is_not_duplicated() {
    let mcp = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/mcp/mod.rs"));
    let lib = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"));
    assert!(
        !mcp.contains("pub async fn start_mcp_http_server("),
        "non-graceful start_mcp_http_server must be removed, not kept as a second implementation"
    );
    let reexports_old_name = lib.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.contains("start_mcp_http_server") && !trimmed.contains("with_shutdown")
    });
    assert!(
        !reexports_old_name,
        "lib.rs must not re-export the non-graceful start_mcp_http_server"
    );
}

#[test]
fn https_uses_same_addr_in_use_retry_as_http() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/shutdown.rs"));
    assert!(
        src.contains("bind_tcp_with_addr_in_use_retry")
            || src.matches("ErrorKind::AddrInUse").count() >= 2,
        "HTTPS starter must retry AddrInUse like HTTP instead of a one-shot bind"
    );
    assert!(
        src.contains("from_tcp_rustls") || src.contains("from_tcp("),
        "HTTPS must serve from the retried listener, not axum_server::bind_rustls(addr)"
    );
}

#[test]
fn http_and_https_share_shutdown_deadline() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/shutdown.rs"));
    assert!(
        src.contains("GRACEFUL_SHUTDOWN_TIMEOUT"),
        "HTTP and HTTPS drain must share one shutdown deadline constant"
    );
    assert_eq!(GRACEFUL_SHUTDOWN_TIMEOUT, Duration::from_secs(10));
    let http_serve = src
        .find("serve_with_graceful_shutdown")
        .expect("HTTP serve helper");
    let https = src
        .find("graceful_shutdown(Some")
        .expect("HTTPS axum_server drain");
    assert!(
        src[http_serve..].contains("GRACEFUL_SHUTDOWN_TIMEOUT")
            || src.contains("timeout(GRACEFUL_SHUTDOWN_TIMEOUT"),
        "HTTP drain must use GRACEFUL_SHUTDOWN_TIMEOUT, not wait forever"
    );
    assert!(
        src[https..https + 80].contains("GRACEFUL_SHUTDOWN_TIMEOUT"),
        "HTTPS drain must use GRACEFUL_SHUTDOWN_TIMEOUT"
    );
}

#[tokio::test]
async fn http_graceful_shutdown_is_bounded() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel::<()>();
    let app = Router::new().route(
        "/hang",
        get(|| async {
            std::future::pending::<()>().await;
            "never"
        }),
    );
    let server = tokio::spawn(async move {
        serve_with_graceful_shutdown(listener, app, async move {
            let _ = rx.await;
        })
        .await
    });

    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect hang");
    stream
        .write_all(b"GET /hang HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
        .await
        .expect("write hang");
    tokio::time::sleep(Duration::from_millis(50)).await;

    tx.send(()).unwrap();
    let bound = GRACEFUL_SHUTDOWN_TIMEOUT + Duration::from_secs(2);
    tokio::time::timeout(bound, server)
        .await
        .expect("HTTP graceful drain must be bounded")
        .expect("server task must not panic")
        .expect("bounded shutdown must succeed");
}

#[tokio::test]
async fn signal_install_error_does_not_complete_shutdown() {
    let err = std::io::Error::other("install failed");
    let held = complete_or_hold_on_error(Err(err), "Ctrl+C");
    let timed_out = tokio::time::timeout(Duration::from_millis(50), held)
        .await
        .is_err();
    assert!(
        timed_out,
        "signal install errors must pending(), not complete the shutdown future"
    );
}

#[test]
fn shutdown_signal_holds_on_install_or_closed_listener() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/shutdown.rs"));
    assert!(
        src.contains("complete_or_hold_on_error") || src.contains("pending()"),
        "shutdown_signal must not return on install Err"
    );
    assert!(
        src.contains("pending::<") || src.contains("pending()"),
        "closed SIGTERM stream / install failure must keep waiting"
    );
}
