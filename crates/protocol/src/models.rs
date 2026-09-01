use serde::{Deserialize, Serialize};

/// Terminal basic metadata
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalInfo {
    pub terminal_id: String,
    pub hostname: String,
    pub username: String,
    pub lan_ip: String,
    pub os_version: String,
    pub agent_version: String,
}

/// Heartbeat metrics reported periodically by agent
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeartbeatMetrics {
    pub cpu_usage_percent: f32,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub uptime_secs: u64,
    pub timestamp: i64,
}

/// Tool invocation payload
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallPayload {
    pub call_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub timeout_secs: u64,
}

/// Tool execution result payload
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultPayload {
    pub call_id: String,
    pub success: bool,
    pub result: serde_json::Value,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// Terminal connection status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalStatus {
    Online,
    Busy,
    Offline,
}
