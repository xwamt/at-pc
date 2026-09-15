pub mod messages;
pub mod models;

pub use messages::{AgentToServerMessage, BinaryDesktopFrame, ServerToAgentMessage};
pub use models::{
    DesktopInputEvent, HeartbeatMetrics, MonitorInfo, StateDiff, TerminalInfo, TerminalStatus,
    ToolCallPayload, ToolResultPayload, UiElement, UiTreeResponse, WindowInfo,
};

