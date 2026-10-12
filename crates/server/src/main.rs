use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, Level};

use at_pc_server::bootstrap;
use at_pc_server::config::{self, ServerConfig};
use at_pc_server::mcp::run_stdio_server;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::{start_ws_server_with_state, TerminalRegistry, WsServerState};
use at_pc_server::{
    abort_and_join_tasks, shutdown_persistence, shutdown_signal,
    start_mcp_http_server_with_shutdown_and_keystore, AuthRateLimiter, KeyStore,
};

/// CLI arguments for server bootstrap and authentication gate
#[derive(Debug, Clone, Default)]
pub struct Args {
    /// Disable first-boot root API key bootstrap
    pub no_bootstrap: bool,
    /// Opt out of authentication gate (allow non-loopback without credentials)
    pub insecure_no_auth: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let raw_args: Vec<String> = std::env::args().collect();

    // Check for help flag
    if raw_args.iter().any(|a| a == "--help" || a == "-h") {
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
        println!(
            "    --log-file <PATH>     Path to server runtime log file (default: at-pc-server.log)"
        );
        println!(
            "    --keys-file <PATH>    Path to persistent keys file (default: <data_dir>/keys.json)"
        );
        println!("    --no-bootstrap        Disable first-boot root API key bootstrap");
        println!(
            "    --insecure-no-auth    Opt out of authentication gate (allow non-loopback without credentials)"
        );
        println!(
            "    --allow-plaintext     Allow non-loopback listener without TLS (traffic is unencrypted)"
        );
        println!(
            "    --migrate-legacy-credentials  Migrate legacy [roles] and auth_token into KeyStore"
        );
        println!("    --stdio               Run MCP server in stdio mode (for local IDE / Cursor)");
        println!("    --no-tray             Run without system tray icon in console mode");
        println!("    -h, --help            Print help information");
        return Ok(());
    }

    let stdio_mode = raw_args.iter().any(|a| a == "--stdio");

    // Configure logging (if in stdio mode, log to stderr only to avoid corrupting stdio JSON-RPC)
    let mut custom_log_file = std::env::var("AT_PC_LOG_FILE")
        .ok()
        .map(std::path::PathBuf::from);
    for i in 0..raw_args.len() {
        if raw_args[i] == "--log-file" && i + 1 < raw_args.len() {
            custom_log_file = Some(std::path::PathBuf::from(&raw_args[i + 1]));
        }
    }
    let log_file_path = custom_log_file.unwrap_or_else(at_pc_server::mcp::default_log_file_path);

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

    // Check for explicit --config or default search paths
    let explicit_config = (0..raw_args.len())
        .find(|&i| raw_args[i] == "--config" && i + 1 < raw_args.len())
        .map(|i| std::path::PathBuf::from(&raw_args[i + 1]));

    if let Some(path) = explicit_config {
        match ServerConfig::from_file(&path) {
            Ok(cfg) => {
                info!("Loaded configuration from {}", path.display());
                config = cfg;
            }
            Err(e) => {
                error!("Failed to load config file {}: {}", path.display(), e);
                std::process::exit(1);
            }
        }
    } else if let Ok(env_path) = std::env::var("AT_PC_CONFIG") {
        let path = std::path::PathBuf::from(env_path);
        if path.is_file() {
            match ServerConfig::from_file(&path) {
                Ok(cfg) => {
                    info!("Loaded configuration from AT_PC_CONFIG: {}", path.display());
                    config = cfg;
                }
                Err(e) => {
                    error!("Failed to load config file {}: {}", path.display(), e);
                    std::process::exit(1);
                }
            }
        }
    } else {
        // Probe default config locations
        let candidates = [
            std::path::PathBuf::from("server_config.toml"),
            #[cfg(unix)]
            std::path::PathBuf::from("/etc/at-pc/server_config.toml"),
        ];
        for candidate in candidates {
            if candidate.is_file() {
                if let Ok(cfg) = ServerConfig::from_file(&candidate) {
                    info!("Loaded default configuration from {}", candidate.display());
                    config = cfg;
                    break;
                }
            }
        }
    }

    // Override with CLI flags
    for i in 0..raw_args.len() {
        if raw_args[i] == "--host" && i + 1 < raw_args.len() {
            config.listen_host = raw_args[i + 1].clone();
        }
        if raw_args[i] == "--ws-port" && i + 1 < raw_args.len() {
            if let Ok(p) = raw_args[i + 1].parse::<u16>() {
                config.ws_port = p;
            }
        }
        if raw_args[i] == "--mcp-port" && i + 1 < raw_args.len() {
            if let Ok(p) = raw_args[i + 1].parse::<u16>() {
                config.mcp_port = p;
            }
        }
        if raw_args[i] == "--meta-file" && i + 1 < raw_args.len() {
            config.meta_store_path = Some(std::path::PathBuf::from(&raw_args[i + 1]));
        }
        if raw_args[i] == "--auth-token" && i + 1 < raw_args.len() {
            config.auth_token = Some(raw_args[i + 1].clone());
        }
        if raw_args[i] == "--tls-cert" && i + 1 < raw_args.len() {
            config.tls_cert_path = Some(std::path::PathBuf::from(&raw_args[i + 1]));
        }
        if raw_args[i] == "--tls-key" && i + 1 < raw_args.len() {
            config.tls_key_path = Some(std::path::PathBuf::from(&raw_args[i + 1]));
        }
        if raw_args[i] == "--tls-ca" && i + 1 < raw_args.len() {
            config.tls_client_ca_path = Some(std::path::PathBuf::from(&raw_args[i + 1]));
        }
        if raw_args[i] == "--audit-file" && i + 1 < raw_args.len() {
            config.audit_log_path = Some(std::path::PathBuf::from(&raw_args[i + 1]));
        }
        if raw_args[i] == "--log-file" && i + 1 < raw_args.len() {
            config.log_file_path = Some(std::path::PathBuf::from(&raw_args[i + 1]));
        }
        if raw_args[i] == "--keys-file" && i + 1 < raw_args.len() {
            config.keys_file_path = Some(std::path::PathBuf::from(&raw_args[i + 1]));
        }
    }

    if config.keys_file_path.is_none() {
        if let Ok(path) = std::env::var("AT_PC_KEYS_FILE") {
            config.keys_file_path = Some(std::path::PathBuf::from(path));
        }
    }

    if raw_args.iter().any(|a| a == "--allow-plaintext")
        || std::env::var("AT_PC_ALLOW_PLAINTEXT")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    {
        config.allow_plaintext = true;
    }

    let migrate_legacy = raw_args.iter().any(|a| a == "--migrate-legacy-credentials");
    if migrate_legacy {
        let keys_file = config
            .keys_file_path
            .clone()
            .unwrap_or_else(|| config::default_data_dir().join("keys.json"));
        let keystore = match at_pc_server::keystore::KeyStore::load_or_create(&keys_file) {
            Ok(ks) => ks,
            Err(e) => {
                eprintln!("Failed to load keystore at {}: {e}", keys_file.display());
                std::process::exit(1);
            }
        };
        println!("============================================================");
        println!(
            "Migrating legacy credentials into KeyStore ({})",
            keys_file.display()
        );
        println!("============================================================");
        let existing_names: std::collections::HashSet<String> = keystore
            .list()
            .into_iter()
            .filter(|k| k.revoked_at.is_none())
            .map(|k| k.name)
            .collect();

        let mut count = 0;
        for role in config.roles.values() {
            let key_name = format!("migrated-{}", role.to_string().to_ascii_lowercase());
            if existing_names.contains(&key_name) {
                println!(
                    "Credential for role '{}' already migrated ({key_name}), skipping.",
                    role
                );
                continue;
            }
            match keystore.issue(
                key_name,
                at_pc_server::keystore::KeyKind::SecretKey,
                Some(*role),
                vec!["*".to_string()],
                None,
                None,
            ) {
                Ok((rec, plaintext)) => {
                    println!("Migrated credential for role '{}':", role);
                    println!("  ID        : {}", rec.id);
                    println!("  Key       : {}", plaintext);
                    println!("  Role      : {}", role);
                    count += 1;
                }
                Err(e) => {
                    eprintln!("  Failed to migrate credential for role '{}': {e}", role);
                }
            }
        }
        if let Some(ref primary) = config.auth_token {
            if !primary.is_empty() {
                let key_name = "migrated-agent".to_string();
                if existing_names.contains(&key_name) {
                    println!("Legacy auth_token already migrated ({key_name}), skipping.");
                } else {
                    match keystore.issue(
                        key_name,
                        at_pc_server::keystore::KeyKind::DeviceToken,
                        None,
                        vec!["terminal".to_string()],
                        None,
                        None,
                    ) {
                        Ok((rec, plaintext)) => {
                            println!("Migrated legacy auth_token to Device Token:");
                            println!("  ID        : {}", rec.id);
                            println!("  Token     : {}", plaintext);
                            count += 1;
                        }
                        Err(e) => {
                            eprintln!("  Failed to migrate auth_token: {e}");
                        }
                    }
                }
            }
        }
        println!("============================================================");
        println!("Migration complete: {count} credential(s) migrated to KeyStore.");
        println!("Important: Please remove [roles] and auth_token from your config file.");
        println!("============================================================");
        std::process::exit(0);
    }

    let args = Args {
        no_bootstrap: raw_args.iter().any(|a| a == "--no-bootstrap"),
        insecure_no_auth: raw_args.iter().any(|a| a == "--insecure-no-auth"),
    };

    config.bootstrap_disabled = args.no_bootstrap;
    config.insecure_no_auth = args.insecure_no_auth;

    let keys_file = config
        .keys_file_path
        .clone()
        .unwrap_or_else(|| config::default_data_dir().join("keys.json"));

    // The bootstrap key file has to live exactly where `KeyStore::load_or_create` looks for
    // it — beside the keys file. Writing it to the default data dir while `--keys-file` points
    // elsewhere (or is relative) produces an *empty* key store, a banner advertising a root key
    // that never authenticates, and — because an empty store counts as "no credentials
    // configured" on a loopback listener — a console that answers 200 to unauthenticated
    // requests. Both sides now resolve the location through the same helper.
    let bootstrap_dir = bootstrap::keystore_sidecar_dir(&keys_file);

    if !config.bootstrap_disabled {
        match bootstrap::ensure_bootstrap_key_with_keys_file(&bootstrap_dir, Some(&keys_file)) {
            Ok(bootstrap::BootstrapOutcome::Generated(ref key)) => {
                let file = bootstrap::bootstrap_key_path(&bootstrap_dir);
                bootstrap::print_bootstrap_banner_with_file(key, &file);
            }
            Ok(bootstrap::BootstrapOutcome::Existing(_))
            | Ok(bootstrap::BootstrapOutcome::Skipped) => {}
            Err(e) => {
                // Fatal: a key store that exists but cannot be used must never be papered over.
                // Continuing would boot with the API gate effectively unconfigured, which looks
                // identical to a healthy server from the outside.
                eprintln!("FATAL: cannot establish the root API key: {e}");
                eprintln!("Refusing to start.");
                std::process::exit(1);
            }
        }
    }

    // An unreadable key store is a hard startup failure. Silently downgrading to `None`
    // (the previous `.ok()`) turned "credentials exist on disk but cannot be parsed" into
    // "no credentials configured", which is exactly the state the boot gate is meant to
    // refuse — and it left the loopback auto-admin fallback able to hand out Admin.
    let keystore = match at_pc_server::keystore::KeyStore::load_or_create(&keys_file) {
        Ok(ks) => Some(Arc::new(ks)),
        Err(e) => {
            eprintln!(
                "FATAL: failed to load key store at {}: {e}",
                keys_file.display()
            );
            eprintln!(
                "Refusing to start: an unreadable key store would otherwise disable API authentication."
            );
            std::process::exit(1);
        }
    };
    let has_keystore_keys = keystore
        .as_ref()
        .map(|k| k.has_active_keys())
        .unwrap_or(false);

    // Reject plaintext legacy credentials (both [roles] and auth_token) if KeyStore is active
    if let Err(e) = config::validate_legacy_config_gate(&config, has_keystore_keys) {
        eprintln!("{e}");
        std::process::exit(1);
    }

    let has_creds = config.has_credentials() || has_keystore_keys;

    if let Err(e) = config::validate_boot_gate(&config, has_creds, config.insecure_no_auth) {
        eprintln!("{e}");
        std::process::exit(1);
    }

    if config.insecure_no_auth {
        tracing::warn!("--insecure-no-auth is active: server is running in unauthenticated mode!");
    }
    if config.allow_plaintext && !config::is_loopback(&config.listen_host) {
        eprintln!(
            "WARNING: Running on non-loopback interface '{}' without TLS (--allow-plaintext active)! Traffic is unencrypted.",
            config.listen_host
        );
    }

    #[cfg(feature = "tray")]
    let enable_tray = at_pc_server::tray::should_enable_tray(&raw_args);
    #[cfg(not(feature = "tray"))]
    let enable_tray = false;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    #[cfg(feature = "tray")]
    if enable_tray {
        let (shutdown_broadcast_tx, shutdown_broadcast_rx) =
            tokio::sync::broadcast::channel::<()>(1);
        let mut builder = winit::event_loop::EventLoopBuilder::<at_pc_server::tray::ServerAppEvent>::with_user_event();
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            builder.with_activation_policy(ActivationPolicy::Accessory);
        }

        match builder.build() {
            Ok(event_loop) => {
                match at_pc_server::tray::ServerTray::new(config.mcp_port, config.ws_port) {
                    Ok(tray) => {
                        let proxy = event_loop.create_proxy();
                        let completion_proxy = proxy.clone();
                        let server_config = config.clone();
                        let pipeline_keystore = keystore.clone();

                        rt.spawn(async move {
                            if let Err(e) = run_server_pipeline(
                                server_config,
                                pipeline_keystore,
                                stdio_mode,
                                Some(shutdown_broadcast_rx),
                                Some(completion_proxy),
                            )
                            .await
                            {
                                error!("Server error: {}", e);
                            }
                        });

                        at_pc_server::tray::run_tray_event_loop(
                            tray,
                            config.mcp_port,
                            log_file_path,
                            shutdown_broadcast_tx,
                            proxy,
                            event_loop,
                        )
                    }
                    Err(e) => {
                        error!(
                            "Failed to initialize server tray ({e}); falling back to console mode"
                        );
                        rt.block_on(run_server_pipeline(
                            config, keystore, stdio_mode, None, None,
                        ))
                    }
                }
            }
            Err(e) => {
                error!("Failed to create event loop for tray ({e}); falling back to console mode");
                rt.block_on(run_server_pipeline(
                    config, keystore, stdio_mode, None, None,
                ))
            }
        }
    } else {
        rt.block_on(run_server_pipeline(
            config, keystore, stdio_mode, None, None,
        ))
    }

    #[cfg(not(feature = "tray"))]
    {
        let _ = enable_tray;
        rt.block_on(run_server_pipeline(
            config, keystore, stdio_mode, None, None,
        ))
    }
}

#[cfg(feature = "tray")]
type TrayCompletionProxy =
    Option<winit::event_loop::EventLoopProxy<at_pc_server::tray::ServerAppEvent>>;

#[cfg(not(feature = "tray"))]
type TrayCompletionProxy = Option<()>;

async fn run_server_pipeline(
    config: ServerConfig,
    keystore: Option<Arc<KeyStore>>,
    stdio_mode: bool,
    shutdown_rx: Option<tokio::sync::broadcast::Receiver<()>>,
    completion_proxy: TrayCompletionProxy,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
                    .and_then(|p| {
                        p.parent()
                            .filter(|d| *d != std::path::Path::new("/"))
                            .map(|d| d.join("terminals_meta.json"))
                    })
                    .unwrap_or_else(|| {
                        at_pc_server::config::default_data_dir().join("terminals_meta.json")
                    }),
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

    if (!at_pc_server::config::is_loopback(&config.listen_host))
        && (config.allow_plaintext || config.insecure_no_auth)
    {
        if let Some(ref al) = audit_logger {
            al.log(at_pc_server::audit::AuditRecord {
                id: format!("audit-{}", at_pc_server::router::generate_call_id()),
                timestamp: chrono::Utc::now().to_rfc3339(),
                role: None,
                token_prefix: None,
                client_ip: None,
                terminal_id: None,
                action: at_pc_server::audit::ACTION_INSECURE_MODE.to_string(),
                tool_name: None,
                arguments: None,
                status: "WARN".to_string(),
                error: Some(if config.insecure_no_auth {
                    "Running on non-loopback without authentication (--insecure-no-auth)"
                        .to_string()
                } else {
                    "Running on non-loopback without TLS (--allow-plaintext)".to_string()
                }),
                duration_ms: Some(0),
            });
        }
    }

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
    let rate_limiter = Arc::new(AuthRateLimiter::default());

    let mut ws_state =
        WsServerState::with_handler(registry.clone(), config.clone(), router.clone());
    if let Some(ref logger) = audit_logger {
        ws_state = ws_state.with_audit_logger(logger.clone());
    }
    ws_state.keystore = keystore.clone();
    ws_state.rate_limiter = rate_limiter.clone();
    let ws_port = config.ws_port;
    let ws_handle = tokio::spawn(async move {
        if let Err(e) = start_ws_server_with_state(ws_state, ws_port).await {
            error!("WebSocket server error: {}", e);
        }
    });

    // 5. Run MCP gateway until transport completion or signal, then stop producers before
    // flushing the persistence writers.
    let shutdown_future = async move {
        if let Some(mut rx) = shutdown_rx {
            tokio::select! {
                () = shutdown_signal() => {},
                _ = rx.recv() => {},
            }
        } else {
            shutdown_signal().await;
        }
    };

    let mut background_http = None;
    let gateway_result = if stdio_mode {
        let http_router = router.clone();
        let http_config = config.clone();
        let http_keystore = keystore.clone();
        background_http = Some(tokio::spawn(async move {
            if let Err(e) = start_mcp_http_server_with_shutdown_and_keystore(
                http_router,
                http_config,
                http_keystore,
                shutdown_signal(),
            )
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
        start_mcp_http_server_with_shutdown_and_keystore(
            router.clone(),
            config,
            keystore.clone(),
            shutdown_future,
        )
        .await
    };

    let mut handles = vec![ws_handle, sweep_handle];
    if let Some(handle) = background_http {
        handles.push(handle);
    }
    abort_and_join_tasks(handles).await;

    if let Err(error) = shutdown_persistence(&meta_store, audit_logger.as_deref()).await {
        error!(%error, "failed to durably shut down persistence writers");
    }

    #[cfg(feature = "tray")]
    if let Some(proxy) = completion_proxy {
        let _ = proxy.send_event(at_pc_server::tray::ServerAppEvent::ShutdownCompleted);
    }
    #[cfg(not(feature = "tray"))]
    let _ = completion_proxy;

    gateway_result
}
