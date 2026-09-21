use at_pc_protocol::tools::agent_tool;
pub use at_pc_protocol::tools::Role;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

fn default_listen_host() -> String {
    "0.0.0.0".to_string()
}

/// Returns a suitable directory for runtime data and logs.
/// Checks AT_PC_DATA_DIR -> current working directory (if not / and writable) ->
/// XDG data dir (~/.local/share/at-pc) on Unix -> exe directory -> temp_dir.
pub fn default_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("AT_PC_DATA_DIR") {
        let p = PathBuf::from(dir);
        if std::fs::create_dir_all(&p).is_ok() {
            return p;
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if cwd != std::path::Path::new("/") {
            return cwd;
        }
    }
    #[cfg(unix)]
    {
        if let Ok(home) = std::env::var("HOME") {
            let xdg = PathBuf::from(home).join(".local/share/at-pc");
            if std::fs::create_dir_all(&xdg).is_ok() {
                return xdg;
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if parent != std::path::Path::new("/") {
                return parent.to_path_buf();
            }
        }
    }
    std::env::temp_dir()
}

fn default_audit_log_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("AT_PC_AUDIT_FILE") {
        return Some(PathBuf::from(path));
    }
    let default_file = "audit.jsonl";
    if let Ok(cwd) = std::env::current_dir() {
        if cwd != std::path::Path::new("/") {
            return Some(cwd.join(default_file));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if parent != std::path::Path::new("/") {
                return Some(parent.join(default_file));
            }
        }
    }
    Some(default_data_dir().join(default_file))
}

/// Checks whether a role is authorized for a shared Agent tool or explicit Server meta-tool.
/// Unknown names fail closed for every role.
pub fn is_tool_allowed_for_role(role: Role, tool_name: &str) -> bool {
    if let Some(spec) = agent_tool(tool_name) {
        return role >= spec.required_role;
    }

    let required_role = match tool_name {
        "list_terminals" | "select_terminal" | "get_active_terminal" | "list_pending_calls" => {
            Role::Viewer
        }
        "rename_terminal" | "cancel_tool" | "cancel_task" => Role::Operator,
        _ => return false,
    };
    role >= required_role
}

fn default_ws_port() -> u16 {
    9801
}

fn default_ws_path() -> String {
    "/ws".to_string()
}

fn default_mcp_port() -> u16 {
    9800
}

fn default_heartbeat_interval_secs() -> u64 {
    5
}

fn default_offline_threshold_secs() -> u64 {
    15
}

fn default_sweep_interval_secs() -> u64 {
    5
}

/// Configuration for the centralized at-pc server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// Host address to listen on (default: "0.0.0.0")
    #[serde(default = "default_listen_host")]
    pub listen_host: String,
    /// Port for WebSocket server (default: 9801)
    #[serde(default = "default_ws_port")]
    pub ws_port: u16,
    /// Path for WebSocket endpoint (default: "/ws")
    #[serde(default = "default_ws_path")]
    pub ws_path: String,
    /// Port for MCP HTTP/SSE server (default: 9800)
    #[serde(default = "default_mcp_port")]
    pub mcp_port: u16,
    /// Optional authentication token required for agents to register
    #[serde(default)]
    pub auth_token: Option<String>,
    /// Optional list of allowed origins for CORS. If empty, cross-origin requests are blocked.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// Heartbeat expected interval in seconds (default: 5)
    #[serde(default = "default_heartbeat_interval_secs")]
    pub heartbeat_interval_secs: u64,
    /// Threshold in seconds before a terminal without heartbeat is marked offline (default: 15)
    #[serde(default = "default_offline_threshold_secs")]
    pub offline_threshold_secs: u64,
    /// Offline sweep interval in seconds (default: 5)
    #[serde(default = "default_sweep_interval_secs")]
    pub sweep_interval_secs: u64,
    /// Path for persistent terminal metadata store file (default: "terminals_meta.json")
    #[serde(default)]
    pub meta_store_path: Option<std::path::PathBuf>,
    /// Optional TLS certificate file path (PEM format) for HTTPS / WSS
    #[serde(default)]
    pub tls_cert_path: Option<PathBuf>,
    /// Optional TLS private key file path (PEM format) for HTTPS / WSS
    #[serde(default)]
    pub tls_key_path: Option<PathBuf>,
    /// Optional CA certificate for client certificate verification (mTLS)
    #[serde(default)]
    pub tls_client_ca_path: Option<PathBuf>,
    /// Map of auth tokens to RBAC roles (e.g. {"viewer-token": Role::Viewer, "admin-token": Role::Admin})
    #[serde(default)]
    pub roles: HashMap<String, Role>,
    /// Path to persistent audit log file (JSONL format)
    #[serde(default = "default_audit_log_path")]
    pub audit_log_path: Option<PathBuf>,
    /// Optional path for the server runtime log file (default: "at-pc-server.log")
    #[serde(default)]
    pub log_file_path: Option<PathBuf>,
    /// Path to persistent keys file (`<data_dir>/keys.json`).
    #[serde(default)]
    pub keys_file_path: Option<PathBuf>,
    /// Disable the first-boot root API key bootstrap (`--no-bootstrap`).
    #[serde(default)]
    pub bootstrap_disabled: bool,
    /// Explicitly opt out of the authentication boot gate (`--insecure-no-auth`).
    /// NOT recommended: every endpoint becomes reachable without credentials.
    #[serde(default)]
    pub insecure_no_auth: bool,
    /// Allow non-TLS plaintext transport on non-loopback listeners (`--allow-plaintext`).
    /// Enforced in M3: non-loopback without TLS will fail startup unless this is true.
    #[serde(default)]
    pub allow_plaintext: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_host: "0.0.0.0".to_string(),
            ws_port: 9801,
            ws_path: "/ws".to_string(),
            mcp_port: 9800,
            auth_token: None,
            allowed_origins: Vec::new(),
            heartbeat_interval_secs: 5,
            offline_threshold_secs: 15,
            sweep_interval_secs: 5,
            meta_store_path: None,
            tls_cert_path: None,
            tls_key_path: None,
            tls_client_ca_path: None,
            roles: HashMap::new(),
            audit_log_path: default_audit_log_path(),
            log_file_path: None,
            keys_file_path: None,
            bootstrap_disabled: false,
            insecure_no_auth: false,
            allow_plaintext: false,
        }
    }
}

impl ServerConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_file(
        path: impl AsRef<std::path::Path>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Checks if TLS transport encryption is configured and active
    pub fn is_tls_enabled(&self) -> bool {
        self.tls_cert_path.is_some() && self.tls_key_path.is_some()
    }

    /// True when at least one credential (auth_token or a [roles] entry) is
    /// configured. This is the precondition the boot gate checks before
    /// allowing a non-loopback listener to start.
    pub fn has_credentials(&self) -> bool {
        let has_primary = self
            .auth_token
            .as_deref()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false);
        has_primary || !self.roles.is_empty()
    }

    /// Resolves the RBAC role associated with an authentication token
    pub fn get_role_for_token(&self, token: &str) -> Option<Role> {
        let clean = token.trim();
        if clean.is_empty() {
            return None;
        }

        // 1. Check explicit token-to-role mappings
        if let Some(role) = self.roles.get(clean) {
            return Some(*role);
        }

        // 2. Check legacy / primary auth_token (default to Admin role)
        if let Some(ref primary) = self.auth_token {
            if !primary.is_empty() && primary == clean {
                return Some(Role::Admin);
            }
        }

        // 3. Open dev mode (no auth_token and no roles defined) defaults to Admin
        if self.auth_token.is_none() && self.roles.is_empty() {
            return Some(Role::Admin);
        }

        None
    }
}

/// True if the listen host only accepts loopback-origin connections
/// (`127.0.0.0/8`, `::1`, or the literal name `localhost`).
///
/// `0.0.0.0` / `::` (unspecified / all interfaces) are NOT loopback: a server
/// bound to them is reachable from the network and therefore gated.
pub fn is_loopback(host: &str) -> bool {
    let trimmed = host.trim();
    if trimmed.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match trimmed.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

/// Why the boot gate refused to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootGateError {
    /// Non-loopback listener without any credential configured.
    MissingCredentials { host: String },
    /// Non-loopback listener without TLS and without explicit plaintext exemption.
    TlsRequired { host: String },
    /// Plaintext [roles] present in config when KeyStore is active.
    PlaintextRolesNotAllowed,
}

impl std::fmt::Display for BootGateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootGateError::MissingCredentials { host } => write!(
                f,
                "refusing to start: listening on non-loopback host '{host}' without any credential configured.\n\
                 Solutions:\n\
                 \x20 1. Remove --no-bootstrap so the server generates a root API key on first boot\n\
                 \x20    (printed once on stdout and stored in <data_dir>/bootstrap_key.txt, mode 0600)\n\
                 \x20 2. Provide a credential via --auth-token or [roles] in the config file\n\
                 \x20 3. Bind a loopback interface instead: --host 127.0.0.1 (local development)\n\
                 \x20 4. Explicitly opt out with --insecure-no-auth (NOT recommended: every endpoint is exposed)"
            ),
            BootGateError::TlsRequired { host } => write!(
                f,
                "refusing to start: listening on non-loopback host '{host}' without TLS enabled.\n\
                 Traffic across the network would be unencrypted.\n\
                 Solutions:\n\
                 \x20 1. Configure TLS via --tls-cert and --tls-key\n\
                 \x20 2. Bind a loopback interface instead: --host 127.0.0.1\n\
                 \x20 3. Explicitly allow unencrypted traffic with --allow-plaintext"
            ),
            BootGateError::PlaintextRolesNotAllowed => write!(
                f,
                "refusing to start: plaintext [roles] or auth_token found in configuration while KeyStore is active.\n\
                 Solutions:\n\
                 \x20 1. Run with --migrate-legacy-credentials to migrate existing credentials\n\
                 \x20 2. Remove [roles] section and auth_token from your server configuration file"
            ),
        }
    }
}

impl std::error::Error for BootGateError {}

/// Startup gate: a non-loopback listener must not boot without credentials or without TLS (unless explicitly exempted).
pub fn validate_boot_gate(
    cfg: &ServerConfig,
    has_creds: bool,
    insecure_no_auth: bool,
) -> Result<(), BootGateError> {
    if is_loopback(&cfg.listen_host) {
        return Ok(());
    }
    // --insecure-no-auth provides total unauthenticated rollback / dev mode
    if insecure_no_auth {
        return Ok(());
    }
    if !has_creds {
        return Err(BootGateError::MissingCredentials {
            host: cfg.listen_host.clone(),
        });
    }
    if !cfg.is_tls_enabled() && !cfg.allow_plaintext {
        return Err(BootGateError::TlsRequired {
            host: cfg.listen_host.clone(),
        });
    }
    Ok(())
}

/// Legacy config migration gate: reject plaintext [roles] or auth_token once KeyStore has active keys.
pub fn validate_legacy_config_gate(
    cfg: &ServerConfig,
    has_active_keys: bool,
) -> Result<(), BootGateError> {
    if has_active_keys {
        if !cfg.roles.is_empty() {
            return Err(BootGateError::PlaintextRolesNotAllowed);
        }
        if let Some(ref tok) = cfg.auth_token {
            if !tok.is_empty() {
                return Err(BootGateError::PlaintextRolesNotAllowed);
            }
        }
    }
    Ok(())
}

/// Minimum Role required to access an HTTP endpoint (A7 / G1-3).
///
/// If None, the endpoint has no specific handler-level RBAC gate (handled by auth_layer or public).
pub fn handler_min_role(path: &str, method: &str) -> Option<Role> {
    let method = method.to_ascii_uppercase();
    let trimmed_path = path.split('?').next().unwrap_or(path);

    if trimmed_path == "/api/logs" && method == "GET" {
        return Some(Role::Admin);
    }
    if trimmed_path == "/api/keys" {
        return Some(Role::Admin);
    }
    if trimmed_path.starts_with("/api/keys/") {
        return Some(Role::Admin);
    }
    if trimmed_path == "/api/auth/me" && method == "GET" {
        return Some(Role::Viewer);
    }
    if trimmed_path == "/api/terminals" && method == "GET" {
        return Some(Role::Viewer);
    }
    if method == "POST" && trimmed_path.starts_with("/api/terminals/") && trimmed_path.ends_with("/meta") {
        return Some(Role::Operator);
    }
    if trimmed_path == "/api/calls" && method == "GET" {
        return Some(Role::Viewer);
    }
    if method == "GET" && trimmed_path.starts_with("/api/terminals/") && trimmed_path.ends_with("/calls") {
        return Some(Role::Viewer);
    }
    if method == "POST" && trimmed_path.starts_with("/api/terminals/") && (trimmed_path.ends_with("/desktop/start") || trimmed_path.ends_with("/desktop/stream")) {
        return Some(Role::Operator);
    }
    if method == "POST" && trimmed_path.starts_with("/api/terminals/") && trimmed_path.ends_with("/desktop/stop") {
        return Some(Role::Operator);
    }
    if method == "GET" && trimmed_path.starts_with("/api/terminals/") && trimmed_path.ends_with("/desktop/frame") {
        return Some(Role::Viewer);
    }
    if method == "GET" && trimmed_path.starts_with("/api/terminals/") && trimmed_path.ends_with("/desktop/raw") {
        return Some(Role::Viewer);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_config_rejects_unknown_fields() {
        let toml_with_typo = r#"
            listen_host = "127.0.0.1"
            mcp_portt = 19999
        "#;
        let result: Result<ServerConfig, _> = toml::from_str(toml_with_typo);
        assert!(result.is_err(), "misspelled config keys must fail deserialization");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unknown field `mcp_portt`") || err_msg.contains("mcp_portt"),
            "error must mention misspelled field, got: {err_msg}"
        );
    }

    #[test]
    fn server_config_accepts_valid_config() {
        let valid_toml = r#"
            listen_host = "127.0.0.1"
            mcp_port = 19999
            allow_plaintext = true
        "#;
        let config: ServerConfig = toml::from_str(valid_toml).expect("valid toml");
        assert_eq!(config.listen_host, "127.0.0.1");
        assert_eq!(config.mcp_port, 19999);
        assert!(config.allow_plaintext);
    }
}
