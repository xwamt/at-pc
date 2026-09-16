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

/// Remote desktop input event types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", content = "data")]
pub enum DesktopInputEvent {
    MouseMove { x: u32, y: u32 },
    MouseMovePixel { x: i32, y: i32 },
    MouseDown { button: u8 }, // 0: left, 1: middle, 2: right
    MouseUp { button: u8 },
    MouseClick { button: u8, count: u8 },
    MouseWheel { delta_y: i32 },
    KeyDown { key_code: u32, key: String },
    KeyUp { key_code: u32, key: String },
    TypeText { text: String },
}

/// UI Element in structured UI tree
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiElement {
    pub id: u32,
    #[serde(rename = "type")]
    pub control_type: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub rect: [i32; 4], // [x, y, width, height]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help_text: Option<String>,
}

/// Structured UI Tree response representation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiTreeResponse {
    pub active_window: String,
    pub window_bounds: [i32; 4], // [x, y, width, height]
    pub elements: Vec<UiElement>,
    pub total_elements: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_matched: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_index: Option<usize>,
}

/// Detailed window metadata for desktop window lifecycle management
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub hwnd: usize,
    pub pid: u32,
    pub title: String,
    pub process_name: String,
    pub is_minimized: bool,
    pub is_foreground: bool,
    pub rect: [i32; 4], // [x, y, width, height]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_index: Option<usize>,
}

/// Detailed UIA element change in state diff
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiElementModification {
    pub id: u32,
    pub name: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}

/// Rich UI Automation DOM State Diff
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UiStateDiff {
    pub has_changes: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added_elements: Vec<UiElement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_elements: Vec<UiElement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified_elements: Vec<UiElementModification>,
    pub summary: String,
}

/// Action review loop state difference detection
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StateDiff {
    pub foreground_changed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_window: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_window: Option<String>,
    pub modal_dialog_detected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialog_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_diff: Option<UiStateDiff>,
}

/// Individual visual mark in Set-of-Mark (SoM) annotated screen
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenMark {
    pub id: u32,
    pub rect: [i32; 4],   // [x, y, width, height]
    pub center: [i32; 2], // [x, y]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_type: Option<String>,
}

/// Response containing annotated screenshot with Set-of-Mark badges and mapping table
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkedScreenResponse {
    pub display_index: usize,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub base64_data: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw_base64: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub image_base64: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub data_uri: String,
    pub marks: Vec<ScreenMark>,
    pub total_marks: usize,
    pub source: String, // e.g. "ui_tree", "grid", "contours", "hybrid"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_factor: Option<f32>,
}

/// Display monitor specification and virtual desktop placement
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub display_index: usize,
    pub name: String,
    pub is_primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}
