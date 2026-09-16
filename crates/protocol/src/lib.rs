pub mod messages;
pub mod models;
pub mod tools;

pub use messages::{AgentToServerMessage, BinaryDesktopFrame, ServerToAgentMessage};
pub use models::{
    DesktopInputEvent, HeartbeatMetrics, MonitorInfo, StateDiff, TerminalInfo, TerminalStatus,
    ToolCallPayload, ToolResultPayload, UiElement, UiTreeResponse, WindowInfo,
};
pub use tools::{
    agent_tool, agent_tool_definitions, tool_registry, AgentToolKind, DispatchKind, Role, ToolRisk,
    ToolSpec, ALL_AGENT_TOOL_KINDS,
};
