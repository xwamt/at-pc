#![cfg_attr(
    all(target_os = "windows", feature = "gui"),
    windows_subsystem = "windows"
)]

//! `at-pc-agent` executable entrypoint.
//! Loads configuration, establishes WebSocket connections with the central server,
//! and runs either the headless service loop or the optional desktop GUI.

use at_pc_agent::config::AgentConfig;

#[cfg(feature = "gui")]
use at_pc_agent::app::{run_agent_app, AgentAppState};
#[cfg(feature = "gui")]
use at_pc_agent::executor::AgentExecutor;
#[cfg(feature = "gui")]
use at_pc_agent::ws_client::AgentWsClient;
#[cfg(feature = "gui")]
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Initialize tracing subscriber to file + stdout
    let log_file_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("at-pc-agent.log")))
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.join("at-pc-agent.log"))
        })
        .unwrap_or_else(|| std::env::temp_dir().join("at-pc-agent.log"));

    let file_appender = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)
        .or_else(|_| {
            let temp_p = std::env::temp_dir().join("at-pc-agent.log");
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(temp_p)
        })
        .ok();

    use std::io::Write;
    let file_shared = file_appender.map(|f| std::sync::Arc::new(std::sync::Mutex::new(f)));

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
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

    tracing::info!("Starting at-pc-agent v{}...", env!("CARGO_PKG_VERSION"));

    #[cfg(windows)]
    unsafe {
        #[cfg(feature = "gui")]
        {
            // GUI builds use the Windows subsystem, so attach to the parent console for CLI runs.
            if std::env::args().len() > 1 {
                use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
                let _ = AttachConsole(ATTACH_PARENT_PROCESS);
            }
        }

        // Screenshot and input coordinates need consistent DPI behavior in GUI and headless modes.
        use windows_sys::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("at-pc-agent v{}", env!("CARGO_PKG_VERSION"));
        println!("Lightweight Endpoint Agent for Remote Diagnostics & Control\n");
        println!("USAGE:");
        println!("    at-pc-agent [OPTIONS]\n");
        println!("OPTIONS:");
        println!("    -c, --config <PATH>       Path to TOML configuration file");
        println!("    -s, --server <URL>        Server WebSocket URL override (e.g. ws://192.168.1.100:9801/ws)");
        println!(
            "    -H, --headless            Run in headless background service mode without GUI"
        );
        println!("        --service             Alias for --headless");
        println!("    --enable-computer-use     Enable simulated mouse and keyboard MCP tools");
        println!("    --ca-cert <PATH>          Path to CA certificate for TLS/WSS verification");
        println!("    --client-cert <PATH>      Path to client certificate for mTLS");
        println!("    --client-key <PATH>       Path to client private key for mTLS");
        println!(
            "    --insecure                Skip TLS server certificate verification (testing only)"
        );
        println!("    -h, --help                Print help information\n");
        println!("ENVIRONMENT VARIABLES:");
        println!("    AT_PC_HEADLESS=1          Run in headless background service mode");
        println!("    AT_PC_SERVICE=1           Run in background service mode");
        println!("    AT_PC_ENABLE_COMPUTER_USE=1 Enable simulated mouse and keyboard MCP tools");
        return Ok(());
    }

    let mut custom_config_path = None;
    let mut cli_server_url = None;
    let mut headless = std::env::var("AT_PC_HEADLESS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
        || std::env::var("AT_PC_SERVICE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
    let mut cli_enable_cu = false;
    let mut cli_ca_cert = None;
    let mut cli_client_cert = None;
    let mut cli_client_key = None;
    let mut cli_insecure = false;

    for i in 0..args.len() {
        if (args[i] == "--config" || args[i] == "-c") && i + 1 < args.len() {
            custom_config_path = Some(std::path::PathBuf::from(&args[i + 1]));
        }
        if (args[i] == "--server" || args[i] == "-s") && i + 1 < args.len() {
            cli_server_url = Some(args[i + 1].clone());
        }
        if args[i] == "--headless" || args[i] == "-H" || args[i] == "--service" {
            headless = true;
        }
        if args[i] == "--enable-computer-use" {
            cli_enable_cu = true;
        }
        if args[i] == "--ca-cert" && i + 1 < args.len() {
            cli_ca_cert = Some(std::path::PathBuf::from(&args[i + 1]));
        }
        if args[i] == "--client-cert" && i + 1 < args.len() {
            cli_client_cert = Some(std::path::PathBuf::from(&args[i + 1]));
        }
        if args[i] == "--client-key" && i + 1 < args.len() {
            cli_client_key = Some(std::path::PathBuf::from(&args[i + 1]));
        }
        if args[i] == "--insecure" {
            cli_insecure = true;
        }
    }

    // 2. Load agent configuration
    let mut config = AgentConfig::load_from_file_or_default(custom_config_path.as_deref());
    if let Some(url) = cli_server_url {
        tracing::info!("Overriding server URL from CLI: {}", url);
        config.server.url = url;
    }
    if cli_enable_cu {
        config.enable_computer_use = true;
    }
    if let Some(ca) = cli_ca_cert {
        config.server.ca_cert_path = Some(ca);
    }
    if let Some(cc) = cli_client_cert {
        config.server.client_cert_path = Some(cc);
    }
    if let Some(ck) = cli_client_key {
        config.server.client_key_path = Some(ck);
    }
    if cli_insecure {
        config.server.insecure_skip_verify = true;
    }

    if headless {
        tracing::info!("Starting agent in headless / background service mode (no GUI)...");
        return at_pc_agent::run_headless(config).await;
    }

    #[cfg(feature = "gui")]
    {
        run_gui(config).await
    }

    #[cfg(not(feature = "gui"))]
    {
        tracing::info!("GUI feature is disabled; starting agent in headless mode...");
        at_pc_agent::run_headless(config).await
    }
}

#[cfg(feature = "gui")]
async fn run_gui(config: AgentConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let terminal_info = config.to_terminal_info();
    let server_url = config.server.url.clone();
    let auth_token = config.server.auth_token.clone();
    let reconnect_secs = config.server.reconnect_interval_secs;

    tracing::info!(
        "Terminal [{}] initialized. Target server: {}, reconnect interval: {}s (computer_use: {})",
        terminal_info.terminal_id,
        server_url,
        reconnect_secs,
        config.enable_computer_use
    );

    let state =
        Arc::new(AgentAppState::new(server_url.clone()).with_terminal_info(terminal_info.clone()));

    state.add_audit_log(
        "agent_startup",
        &format!(
            "Terminal [{}] targeting server: {}",
            terminal_info.terminal_id, server_url
        ),
        "STARTED",
    );

    let executor = Arc::new(AgentExecutor::default().with_computer_use(config.enable_computer_use));
    let ws_client = Arc::new(
        AgentWsClient::new(server_url, terminal_info, executor)
            .with_auth_token(auth_token)
            .with_reconnect_interval(reconnect_secs)
            .with_listener(state.clone())
            .with_tls_config(
                config.server.ca_cert_path,
                config.server.client_cert_path,
                config.server.client_key_path,
                config.server.insecure_skip_verify,
            ),
    );

    state.register_ws_client(ws_client.clone());

    let client = ws_client.clone();
    let client_task = tokio::spawn(async move {
        client.run().await;
    });

    // Keep native window creation on the main thread.
    if let Err(e) = run_agent_app(state.clone()) {
        tracing::error!("Agent GUI error: {}", e);
    }

    tracing::info!("GUI closed. Disconnecting agent...");
    client_task.abort();
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(400),
        ws_client.disconnect("Agent desktop window closed"),
    )
    .await;
    tracing::info!("at-pc-agent exited cleanly.");
    std::process::exit(0);
}
