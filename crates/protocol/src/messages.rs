use serde::{Deserialize, Serialize};
use crate::models::{HeartbeatMetrics, TerminalInfo};

/// Messages sent from Agent (Client) to Server
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum AgentToServerMessage {
    Register {
        info: TerminalInfo,
        auth_token: Option<String>,
    },
    Heartbeat {
        terminal_id: String,
        metrics: HeartbeatMetrics,
    },
    ToolResult {
        call_id: String,
        success: bool,
        result: serde_json::Value,
        error: Option<String>,
        duration_ms: u64,
    },
    Disconnect {
        terminal_id: String,
        reason: String,
    },
}

/// Messages sent from Server to Agent (Client)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum ServerToAgentMessage {
    RegisterAck {
        success: bool,
        message: Option<String>,
        heartbeat_interval_secs: u64,
    },
    HeartbeatAck {
        server_timestamp: i64,
    },
    InvokeTool {
        call_id: String,
        tool_name: String,
        arguments: serde_json::Value,
        timeout_secs: u64,
    },
    CancelTool {
        call_id: String,
    },
}
