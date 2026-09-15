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
    DesktopFrame {
        display_index: u32,
        width: u32,
        height: u32,
        format: String,
        data: String,
        timestamp: u64,
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
    StartDesktopStream {
        display_index: u32,
        fps: u32,
        quality: u8,
        scale: f32,
    },
    StopDesktopStream,
    DesktopInput {
        event: crate::models::DesktopInputEvent,
    },
}

/// Binary desktop frame streaming payload.
/// Format over WebSocket: [4B Magic 'DFRM'][4B Display][4B Width][4B Height][8B Timestamp][Raw JPEG bytes]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryDesktopFrame {
    pub display_index: u32,
    pub width: u32,
    pub height: u32,
    pub timestamp: u64,
    pub data: Vec<u8>,
}

impl BinaryDesktopFrame {
    pub const MAGIC: [u8; 4] = *b"DFRM";
    pub const HEADER_LEN: usize = 24;

    pub fn new(display_index: u32, width: u32, height: u32, timestamp: u64, data: Vec<u8>) -> Self {
        Self {
            display_index,
            width,
            height,
            timestamp,
            data,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::HEADER_LEN + self.data.len());
        buf.extend_from_slice(&Self::MAGIC);
        buf.extend_from_slice(&self.display_index.to_be_bytes());
        buf.extend_from_slice(&self.width.to_be_bytes());
        buf.extend_from_slice(&self.height.to_be_bytes());
        buf.extend_from_slice(&self.timestamp.to_be_bytes());
        buf.extend_from_slice(&self.data);
        buf
    }

    pub fn decode(payload: &[u8]) -> Result<Self, String> {
        if payload.len() < Self::HEADER_LEN {
            return Err(format!(
                "Binary desktop frame payload too short: {} bytes (expected >= {})",
                payload.len(),
                Self::HEADER_LEN
            ));
        }
        if payload[0..4] != Self::MAGIC {
            return Err(format!(
                "Invalid binary desktop frame magic: {:?} (expected {:?})",
                &payload[0..4],
                Self::MAGIC
            ));
        }
        let display_index = u32::from_be_bytes(payload[4..8].try_into().unwrap());
        let width = u32::from_be_bytes(payload[8..12].try_into().unwrap());
        let height = u32::from_be_bytes(payload[12..16].try_into().unwrap());
        let timestamp = u64::from_be_bytes(payload[16..24].try_into().unwrap());
        let data = payload[24..].to_vec();

        Ok(Self {
            display_index,
            width,
            height,
            timestamp,
            data,
        })
    }
}
