//! `at-pc-agent` library root.
//! Provides AgentConfig, AgentExecutor, AgentWsClient, diagnostic tools, and WebSocket client engine.

pub mod app;
pub mod codec;
pub mod config;
pub mod executor;
pub mod tools;
pub mod ws_client;

pub use app::{run_agent_app, setup_custom_fonts, AgentApp, AgentAppState, AgentAuditLog};
pub use config::{AgentConfig, DeviceConfig, ServerConfig};
pub use executor::AgentExecutor;
pub use ws_client::{AgentEventListener, AgentWsClient, ClientConnectionStatus};
