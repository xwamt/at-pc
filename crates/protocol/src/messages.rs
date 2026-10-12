use crate::models::{
    EnrollmentDeviceFacts, EnrollmentStatus, HeartbeatMetrics, TerminalInfo,
};
use serde::{Deserialize, Serialize};

/// First enrollment message sent by an Agent that has no legacy registration
/// credential. The public key and nonce are proof inputs only; M1 does not use
/// them to authorize control-plane access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollmentHello {
    pub protocol_version: u16,
    pub stable_device_id: String,
    pub public_key: String,
    pub facts: EnrollmentDeviceFacts,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    pub client_nonce: String,
}

/// Enrollment messages are kept separate from the active Agent WebSocket
/// contract so Register/RegisterAck wire compatibility remains intact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum EnrollmentClientMessage {
    Hello(EnrollmentHello),
    Proof {
        enrollment_id: String,
        signature: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum EnrollmentServerMessage {
    Challenge {
        enrollment_id: String,
        server_nonce: String,
        expires_at: i64,
    },
    Status {
        enrollment_id: String,
        status: EnrollmentStatus,
        approval_phrase: String,
        retry_after_secs: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason_code: Option<String>,
    },
    Error {
        code: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after_secs: Option<u64>,
    },
}

#[derive(Serialize)]
struct EnrollmentProofTranscript<'a> {
    protocol_version: u16,
    enrollment_id: &'a str,
    server_nonce: &'a str,
    client_nonce: &'a str,
    stable_device_id: &'a str,
    public_key: &'a str,
    facts: &'a EnrollmentDeviceFacts,
    capabilities: &'a [String],
}

/// Produces the exact, versioned byte transcript signed by the device during
/// enrollment. Struct serialization fixes field order and prevents accidental
/// omission of security-relevant hello fields.
pub fn enrollment_proof_transcript(
    hello: &EnrollmentHello,
    enrollment_id: &str,
    server_nonce: &str,
) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&EnrollmentProofTranscript {
        protocol_version: hello.protocol_version,
        enrollment_id,
        server_nonce,
        client_nonce: &hello.client_nonce,
        stable_device_id: &hello.stable_device_id,
        public_key: &hello.public_key,
        facts: &hello.facts,
        capabilities: &hello.capabilities,
    })
    .map_err(|error| format!("failed to serialize enrollment proof transcript: {error}"))
}

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
