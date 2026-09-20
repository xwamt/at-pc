use at_pc_protocol::tools::agent_tool;
pub use at_pc_protocol::tools::Role;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

fn default_listen_host() -> String {
    "0.0.0.0".to_string()
}

fn default_audit_log_path() -> Option<PathBuf> {
    let default_file = "audit.jsonl";
    if let Ok(cwd) = std::env::current_dir() {
        if cwd != std::path::Path::new("/") {
            return Some(cwd.join(default_file));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return Some(parent.join(default_file));
        }
    }
    Some(PathBuf::from(default_file))
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

/// Configuration for the centralized at-pc server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Host address to listen on (default: "0.0.0.0")
    #[serde(default = "default_listen_host")]
    pub listen_host: String,
    /// Port for WebSocket server (default: 9801)
    pub ws_port: u16,
    /// Path for WebSocket endpoint (default: "/ws")
    pub ws_path: String,
    /// Port for MCP HTTP/SSE server (default: 9800)
    pub mcp_port: u16,
    /// Optional authentication token required for agents to register
    pub auth_token: Option<String>,
    /// Optional list of allowed origins for CORS. If empty, cross-origin requests are blocked.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// Heartbeat expected interval in seconds (default: 5)
    pub heartbeat_interval_secs: u64,
    /// Threshold in seconds before a terminal without heartbeat is marked offline (default: 15)
    pub offline_threshold_secs: u64,
    /// Offline sweep interval in seconds (default: 5)
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
        }
    }
}

impl ServerConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Checks if TLS transport encryption is configured and active
    pub fn is_tls_enabled(&self) -> bool {
        self.tls_cert_path.is_some() && self.tls_key_path.is_some()
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
