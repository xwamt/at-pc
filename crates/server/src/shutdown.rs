//! Graceful process shutdown helpers for the HTTP gateway and background tasks.

use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tracing::{error, info};

use crate::audit::AuditLogger;
use crate::config::ServerConfig;
use crate::mcp::create_mcp_http_router;
use crate::meta_store::TerminalMetaStore;
use crate::router::McpRouter;

/// Shared HTTP/HTTPS drain deadline after a shutdown signal.
pub const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

const ADDR_IN_USE_RETRY_LIMIT: u32 = 10;
const ADDR_IN_USE_RETRY_DELAY: Duration = Duration::from_millis(300);

/// Bind `addr`, retrying `AddrInUse` so HTTP and HTTPS share one starter path.
async fn bind_tcp_with_addr_in_use_retry(
    addr: std::net::SocketAddr,
) -> Result<TcpListener, std::io::Error> {
    let mut attempts = 0;
    loop {
        match TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(error)
                if error.kind() == std::io::ErrorKind::AddrInUse
                    && attempts < ADDR_IN_USE_RETRY_LIMIT =>
            {
                attempts += 1;
                tokio::time::sleep(ADDR_IN_USE_RETRY_DELAY).await;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Completes when the process should stop (Ctrl+C / SIGTERM).
///
/// Install or listener failures log then [`std::future::pending`] — they must not
/// be treated as a shutdown request.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        complete_or_hold_on_error(tokio::signal::ctrl_c().await, "Ctrl+C").await;
    };

    #[cfg(unix)]
    {
        let terminate = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut signal) => {
                    if signal.recv().await.is_none() {
                        error!("SIGTERM listener closed");
                        std::future::pending::<()>().await;
                    }
                }
                Err(error) => {
                    complete_or_hold_on_error(Err(error), "SIGTERM").await;
                }
            }
        };
        tokio::select! {
            () = ctrl_c => {}
            () = terminate => {}
        }
    }

    #[cfg(not(unix))]
    ctrl_c.await;
}

/// After a failed signal install, wait forever instead of treating it as shutdown.
pub async fn complete_or_hold_on_error(result: Result<(), std::io::Error>, what: &'static str) {
    if let Err(error) = result {
        error!(%error, "failed to listen for {what}");
        std::future::pending::<()>().await;
    }
}

/// Serve `app` until `shutdown` completes, then drain in-flight requests up to
/// [`GRACEFUL_SHUTDOWN_TIMEOUT`].
pub async fn serve_with_graceful_shutdown(
    listener: TcpListener,
    app: axum::Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(), std::io::Error> {
    let (graceful_tx, graceful_rx) = tokio::sync::oneshot::channel::<()>();
    let (fired_tx, fired_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        shutdown.await;
        let _ = graceful_tx.send(());
        let _ = fired_tx.send(());
    });

    let serve = axum::serve(listener, app).with_graceful_shutdown(async {
        let _ = graceful_rx.await;
    });

    tokio::select! {
        result = serve => result,
        () = async {
            let _ = fired_rx.await;
            tokio::time::sleep(GRACEFUL_SHUTDOWN_TIMEOUT).await;
        } => Ok(()),
    }
}

/// Abort background tasks (sweep / WS / extra HTTP) and wait for them to finish.
pub async fn abort_and_join_tasks(handles: impl IntoIterator<Item = tokio::task::JoinHandle<()>>) {
    let handles: Vec<_> = handles.into_iter().collect();
    for handle in &handles {
        handle.abort();
    }
    for handle in handles {
        let _ = handle.await;
    }
}

/// Flush and terminate MetaStore / AuditLogger after producers have stopped.
pub async fn shutdown_persistence(
    meta_store: &TerminalMetaStore,
    audit_logger: Option<&AuditLogger>,
) -> Result<(), String> {
    meta_store.shutdown().await?;
    if let Some(logger) = audit_logger {
        logger.shutdown_async().await?;
    }
    Ok(())
}

/// Start the MCP HTTP/SSE (or HTTPS) gateway and stop it with graceful shutdown.
pub async fn start_mcp_http_server_with_shutdown(
    router: Arc<McpRouter>,
    config: ServerConfig,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = config.mcp_port;
    let host = config.listen_host.clone();
    let ip: std::net::IpAddr = host
        .parse()
        .unwrap_or_else(|_| std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));
    let addr = std::net::SocketAddr::new(ip, port);
    let app = create_mcp_http_router(router, config.clone());
    let rustls_config = match (&config.tls_cert_path, &config.tls_key_path) {
        (Some(cert_path), Some(key_path)) => {
            crate::tls::ensure_crypto_provider();
            Some(
                axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path)
                    .await
                    .map_err(|e| format!("Failed to load TLS config for HTTP server: {}", e))?,
            )
        }
        _ => None,
    };
    let listener = bind_tcp_with_addr_in_use_retry(addr).await?;

    if let Some(rustls_config) = rustls_config {
        info!(
            "MCP HTTPS/SSE gateway listening on https://{}:{} (TLS encrypted)",
            host, port
        );
        let std_listener = listener.into_std()?;
        let handle = axum_server::Handle::new();
        let shutdown_handle = handle.clone();
        tokio::spawn(async move {
            shutdown.await;
            shutdown_handle.graceful_shutdown(Some(GRACEFUL_SHUTDOWN_TIMEOUT));
        });
        axum_server::from_tcp_rustls(std_listener, rustls_config)
            .handle(handle)
            .serve(app.into_make_service())
            .await?;
    } else {
        info!("MCP HTTP/SSE gateway listening on http://{}:{}", host, port);
        serve_with_graceful_shutdown(listener, app, shutdown).await?;
    }

    Ok(())
}
