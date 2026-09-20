use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, Level};

use at_pc_server::config::ServerConfig;
use at_pc_server::mcp::run_stdio_server;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::{start_ws_server_with_state, TerminalRegistry, WsServerState};
use at_pc_server::{
    abort_and_join_tasks, shutdown_persistence, shutdown_signal,
    start_mcp_http_server_with_shutdown,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().collect();

    // Check for help flag
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("at-pc-server v{}", env!("CARGO_PKG_VERSION"));
        println!("Central Gateway & MCP Router for Multi-Terminal PC Assistance\n");
        println!("USAGE:");
        println!("    at-pc-server [OPTIONS]\n");
        println!("OPTIONS:");
        println!("    --config <PATH>       Path to TOML configuration file");
        println!("    --meta-file <PATH>    Path to JSON terminal metadata file");
        println!("    --host <HOST>         Listen host address (default: 0.0.0.0)");
        println!("    --ws-port <PORT>      WebSocket listen port (default: 9801)");
        println!("    --mcp-port <PORT>     MCP HTTP/SSE listen port (default: 9800)");
        println!("    --auth-token <TOKEN>  Authentication token for agent registration");
        println!("    --tls-cert <PATH>     Path to TLS certificate file for HTTPS / WSS");
        println!("    --tls-key <PATH>      Path to TLS private key file for HTTPS / WSS");
        println!("    --tls-ca <PATH>       Path to client CA certificate for mTLS");
        println!("    --audit-file <PATH>   Path to JSONL audit log file (default: audit.jsonl)");
        println!("    --stdio               Run MCP server in stdio mode (for local IDE / Cursor)");
        println!("    -h, --help            Print help information");
        return Ok(());
    }

    let stdio_mode = args.iter().any(|a| a == "--stdio");

    // Configure logging (if in stdio mode, log to stderr only to avoid corrupting stdio JSON-RPC)
    let log_file_path = at_pc_server::mcp::default_log_file_path();

    let file_appender = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)
        .ok();

    if stdio_mode {
        tracing_subscriber::fmt()
            .with_max_level(Level::WARN)
            .with_writer(std::io::stderr)
            .init();
    } else {
        use std::io::Write;
        let file_shared = file_appender.map(|f| std::sync::Arc::new(std::sync::Mutex::new(f)));
        tracing_subscriber::fmt()
            .with_max_level(Level::INFO)
            .with_writer(move || {
                struct DualWriter {
                    file: Option<std::sync::Arc<std::sync::Mutex<std::fs::File>>>,
                }
                impl Write for DualWriter {
                    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                        let _ = std::io::stdout().write(buf);
                        if let Some(ref f_arc) = self.file {
                            if let Ok(mut f) = f_arc.lock() {
                                let _ = f.write_all(buf);
                            }
                        }
                        Ok(buf.len())
                    }
                    fn flush(&mut self) -> std::io::Result<()> {
                        let _ = std::io::stdout().flush();
                        if let Some(ref f_arc) = self.file {
                            if let Ok(mut f) = f_arc.lock() {
                                let _ = f.flush();
                            }
                        }
                        Ok(())
                    }
                }
                DualWriter {
                    file: file_shared.clone(),
                }
            })
            .init();
    }

    // 1. Load configuration
    let mut config = ServerConfig::default();

    // Check for --config
    for i in 0..args.len() {
        if args[i] == "--config" && i + 1 < args.len() {
            match ServerConfig::from_file(&args[i + 1]) {
                Ok(cfg) => {
                    info!("Loaded configuration from {}", args[i + 1]);
                    config = cfg;
                }
                Err(e) => {
                    error!("Failed to load config file {}: {}", args[i + 1], e);
                    std::process::exit(1);
                }
            }
        }
    }

    // Override with CLI flags
    for i in 0..args.len() {
        if args[i] == "--host" && i + 1 < args.len() {
            config.listen_host = args[i + 1].clone();
        }
        if args[i] == "--ws-port" && i + 1 < args.len() {
            if let Ok(p) = args[i + 1].parse::<u16>() {
                config.ws_port = p;
            }
        }
        if args[i] == "--mcp-port" && i + 1 < args.len() {
            if let Ok(p) = args[i + 1].parse::<u16>() {
                config.mcp_port = p;
            }
        }
        if args[i] == "--meta-file" && i + 1 < args.len() {
            config.meta_store_path = Some(std::path::PathBuf::from(&args[i + 1]));
        }
        if args[i] == "--auth-token" && i + 1 < args.len() {
            config.auth_token = Some(args[i + 1].clone());
        }
        if args[i] == "--tls-cert" && i + 1 < args.len() {
            config.tls_cert_path = Some(std::path::PathBuf::from(&args[i + 1]));
        }
        if args[i] == "--tls-key" && i + 1 < args.len() {
            config.tls_key_path = Some(std::path::PathBuf::from(&args[i + 1]));
        }
        if args[i] == "--tls-ca" && i + 1 < args.len() {
            config.tls_client_ca_path = Some(std::path::PathBuf::from(&args[i + 1]));
        }
        if args[i] == "--audit-file" && i + 1 < args.len() {
            config.audit_log_path = Some(std::path::PathBuf::from(&args[i + 1]));
        }
    }

    info!("Starting at-pc-server v{}", env!("CARGO_PKG_VERSION"));
    info!(
        "Listen host: {}, WebSocket port: {}, MCP port: {}",
        config.listen_host, config.ws_port, config.mcp_port
    );

    // 2. Initialize Terminal Registry & sweep task
    let meta_file =
        config
            .meta_store_path
            .clone()
            .unwrap_or_else(|| match std::env::current_dir() {
                Ok(p) if p != std::path::Path::new("/") => p.join("terminals_meta.json"),
                _ => std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.join("terminals_meta.json")))
                    .unwrap_or_else(|| std::path::PathBuf::from("terminals_meta.json")),
            });
    let meta_store = Arc::new(at_pc_server::meta_store::TerminalMetaStore::new(meta_file));
    let registry = Arc::new(TerminalRegistry::with_store(
        Duration::from_secs(config.offline_threshold_secs),
        Arc::clone(&meta_store),
    ));
    // 3. Initialize MCP Router & Audit Logger
    let audit_logger = config
        .audit_log_path
        .clone()
        .map(|p| Arc::new(at_pc_server::audit::AuditLogger::new(Some(p))));
    let mut mcp_router = McpRouter::new(registry.clone());
    if let Some(ref al) = audit_logger {
        mcp_router = mcp_router.with_audit_logger(al.clone());
    }
    let router = Arc::new(mcp_router);

    let sweep_handle = registry.clone().start_sweep_task_with_handler(
        Duration::from_secs(config.sweep_interval_secs),
        router.clone(),
    );

    // 4. Start WebSocket Gateway in background
    let ws_state = WsServerState::with_handler(registry.clone(), config.clone(), router.clone());
    let ws_port = config.ws_port;
    let ws_handle = tokio::spawn(async move {
        if let Err(e) = start_ws_server_with_state(ws_state, ws_port).await {
            error!("WebSocket server error: {}", e);
        }
    });

    // 5. Run MCP gateway until transport completion or signal, then stop producers before
    // flushing the persistence writers.
    let mut background_http = None;
    let gateway_result = if stdio_mode {
        let http_router = router.clone();
        let http_config = config.clone();
        background_http = Some(tokio::spawn(async move {
            if let Err(e) =
                start_mcp_http_server_with_shutdown(http_router, http_config, shutdown_signal())
                    .await
            {
                error!("MCP HTTP/Web gateway error in stdio mode: {}", e);
            }
        }));
        tokio::select! {
            result = run_stdio_server(router.clone()) => result,
            () = shutdown_signal() => Ok(()),
        }
    } else {
        start_mcp_http_server_with_shutdown(router.clone(), config, shutdown_signal()).await
    };

    let mut handles = vec![ws_handle, sweep_handle];
    if let Some(handle) = background_http {
        handles.push(handle);
    }
    abort_and_join_tasks(handles).await;

    if let Err(error) = shutdown_persistence(&meta_store, audit_logger.as_deref()).await {
        error!(%error, "failed to durably shut down persistence writers");
    }

    gateway_result
}
