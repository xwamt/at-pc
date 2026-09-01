use serde::{Deserialize, Serialize};

/// Configuration for the centralized at-pc server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Port for WebSocket server (default: 9801)
    pub ws_port: u16,
    /// Path for WebSocket endpoint (default: "/ws")
    pub ws_path: String,
    /// Port for MCP HTTP/SSE server (default: 9800)
    pub mcp_port: u16,
    /// Optional authentication token required for agents to register
    pub auth_token: Option<String>,
    /// Heartbeat expected interval in seconds (default: 5)
    pub heartbeat_interval_secs: u64,
    /// Threshold in seconds before a terminal without heartbeat is marked offline (default: 15)
    pub offline_threshold_secs: u64,
    /// Offline sweep interval in seconds (default: 5)
    pub sweep_interval_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            ws_port: 9801,
            ws_path: "/ws".to_string(),
            mcp_port: 9800,
            auth_token: None,
            heartbeat_interval_secs: 5,
            offline_threshold_secs: 15,
            sweep_interval_secs: 5,
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
}
