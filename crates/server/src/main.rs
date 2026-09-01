use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, Level};

use at_pc_server::config::ServerConfig;
use at_pc_server::mcp::{run_stdio_server, start_mcp_http_server};
use at_pc_server::router::McpRouter;
use at_pc_server::ws::{start_ws_server_with_state, TerminalRegistry, WsServerState};

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
        println!("    --ws-port <PORT>      WebSocket listen port (default: 9801)");
        println!("    --mcp-port <PORT>     MCP HTTP/SSE listen port (default: 9800)");
        println!("    --auth-token <TOKEN>  Authentication token for agent registration");
        println!("    --stdio               Run MCP server in stdio mode (for local IDE / Cursor)");
        println!("    -h, --help            Print help information");
        return Ok(());
    }

    let stdio_mode = args.iter().any(|a| a == "--stdio");

    // Configure logging (if in stdio mode, log to stderr only to avoid corrupting stdio JSON-RPC)
    if stdio_mode {
        tracing_subscriber::fmt()
            .with_max_level(Level::WARN)
            .with_writer(std::io::stderr)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(Level::INFO)
            .with_writer(std::io::stdout)
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
        if args[i] == "--auth-token" && i + 1 < args.len() {
            config.auth_token = Some(args[i + 1].clone());
        }
    }

    info!("Starting at-pc-server v{}", env!("CARGO_PKG_VERSION"));
    info!("WebSocket port: {}, MCP port: {}", config.ws_port, config.mcp_port);

    // 2. Initialize Terminal Registry & sweep task
    let registry = Arc::new(TerminalRegistry::with_threshold(Duration::from_secs(
        config.offline_threshold_secs,
    )));
    let _sweep_handle = registry
        .clone()
        .start_sweep_task(Duration::from_secs(config.sweep_interval_secs));

    // 3. Initialize MCP Router
    let router = Arc::new(McpRouter::new(registry.clone()));

    // 4. Start WebSocket Gateway in background
    let ws_state = WsServerState::with_handler(registry.clone(), config.clone(), router.clone());
    let ws_port = config.ws_port;
    tokio::spawn(async move {
        if let Err(e) = start_ws_server_with_state(ws_state, ws_port).await {
            error!("WebSocket server error: {}", e);
        }
    });

    // 5. Run MCP gateway (stdio or HTTP/SSE)
    if stdio_mode {
        run_stdio_server(router).await?;
    } else {
        start_mcp_http_server(router, config).await?;
    }

    Ok(())
}
