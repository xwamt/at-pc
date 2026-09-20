//! Agent desktop UI and application state module.

#[cfg(feature = "gui")]
pub mod ui;

#[cfg(feature = "gui")]
pub use ui::{run_agent_app, setup_custom_fonts, AgentApp};

use crate::config::AgentConfig;
use crate::ws_client::{AgentEventListener, AgentWsClient, ClientConnectionStatus};
use at_pc_protocol::models::TerminalInfo;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// Maximum audit log records kept in memory
const MAX_AUDIT_LOGS: usize = 500;

/// Real-time tool execution audit log entry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentAuditLog {
    pub id: String,
    pub timestamp: String,
    pub tool_name: String,
    pub summary: String,
    pub status: String,
    pub duration_ms: Option<u64>,
}

/// Thread-safe shared state for Agent desktop UI and background services
pub struct AgentAppState {
    server_url: Arc<RwLock<String>>,
    terminal_info: Arc<RwLock<TerminalInfo>>,
    status: Arc<RwLock<ClientConnectionStatus>>,
    is_stopped: Arc<AtomicBool>,
    audit_logs: Arc<RwLock<Vec<AgentAuditLog>>>,
    ws_client: Arc<RwLock<Option<Arc<AgentWsClient>>>>,
    tokio_handle: Arc<RwLock<Option<tokio::runtime::Handle>>>,
}

impl AgentAppState {
    /// Creates a new `AgentAppState` instance with server URL.
    pub fn new(server_url: String) -> Self {
        let default_info = AgentConfig::default().to_terminal_info();
        let handle = tokio::runtime::Handle::try_current().ok();
        Self {
            server_url: Arc::new(RwLock::new(server_url)),
            terminal_info: Arc::new(RwLock::new(default_info)),
            status: Arc::new(RwLock::new(ClientConnectionStatus::Disconnected)),
            is_stopped: Arc::new(AtomicBool::new(false)),
            audit_logs: Arc::new(RwLock::new(Vec::new())),
            ws_client: Arc::new(RwLock::new(None)),
            tokio_handle: Arc::new(RwLock::new(handle)),
        }
    }

    /// Explicitly attaches a Tokio runtime handle to the state.
    pub fn with_runtime_handle(self, handle: tokio::runtime::Handle) -> Self {
        {
            let mut guard = self.tokio_handle.write().unwrap();
            *guard = Some(handle);
        }
        self
    }

    /// Spawns a future on the stored or ambient Tokio runtime handle.
    pub fn spawn_async<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let handle_opt = {
            let guard = self.tokio_handle.read().unwrap();
            guard.clone().or_else(|| tokio::runtime::Handle::try_current().ok())
        };

        if let Some(handle) = handle_opt {
            handle.spawn(future);
        } else {
            tracing::warn!("Failed to spawn async task: no Tokio runtime handle available");
        }
    }

    /// Sets custom terminal information builder-style.
    pub fn with_terminal_info(self, info: TerminalInfo) -> Self {
        {
            let mut guard = self.terminal_info.write().unwrap();
            *guard = info;
        }
        self
    }

    /// Sets terminal info at runtime.
    pub fn set_terminal_info(&self, info: TerminalInfo) {
        let mut guard = self.terminal_info.write().unwrap();
        *guard = info;
    }

    /// Returns a clone of current terminal info.
    pub fn get_terminal_info(&self) -> TerminalInfo {
        self.terminal_info.read().unwrap().clone()
    }

    /// Returns the target server WebSocket URL.
    pub fn server_url(&self) -> String {
        self.server_url.read().unwrap().clone()
    }

    /// Sets the target server WebSocket URL.
    pub fn set_server_url(&self, url: String) {
        let mut guard = self.server_url.write().unwrap();
        *guard = url;
    }

    /// Returns the current client connection status.
    pub fn status(&self) -> ClientConnectionStatus {
        *self.status.read().unwrap()
    }

    /// Updates connection status.
    pub fn set_status(&self, status: ClientConnectionStatus) {
        let mut guard = self.status.write().unwrap();
        *guard = status;
    }

    /// Returns whether the agent has been stopped via emergency disconnect.
    pub fn is_stopped(&self) -> bool {
        self.is_stopped.load(Ordering::SeqCst)
    }

    /// Sets the stopped flag.
    pub fn set_stopped(&self, stopped: bool) {
        self.is_stopped.store(stopped, Ordering::SeqCst);
    }

    /// Registers the background WebSocket client handle for emergency disconnects.
    pub fn register_ws_client(&self, client: Arc<AgentWsClient>) {
        let mut guard = self.ws_client.write().unwrap();
        *guard = Some(client);
    }

    /// Appends a new audit log record with automatic timestamp.
    pub fn add_audit_log(&self, tool_name: &str, summary: &str, status: &str) {
        let entry = AgentAuditLog {
            id: format!("log-{}", chrono::Utc::now().timestamp_micros()),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            tool_name: tool_name.to_string(),
            summary: summary.to_string(),
            status: status.to_string(),
            duration_ms: None,
        };
        self.add_audit_log_entry(entry);
    }

    /// Appends an existing `AgentAuditLog` entry, enforcing buffer cap.
    pub fn add_audit_log_entry(&self, entry: AgentAuditLog) {
        let mut logs = self.audit_logs.write().unwrap();
        logs.push(entry);
        if logs.len() > MAX_AUDIT_LOGS {
            let drain_count = logs.len() - MAX_AUDIT_LOGS;
            logs.drain(0..drain_count);
        }
    }

    /// Returns all collected audit logs.
    pub fn get_audit_logs(&self) -> Vec<AgentAuditLog> {
        self.audit_logs.read().unwrap().clone()
    }

    /// Clears all audit logs.
    pub fn clear_audit_logs(&self) {
        let mut logs = self.audit_logs.write().unwrap();
        logs.clear();
    }

    /// Triggers emergency disconnect: stops client, kills active subprocesses, and marks stopped.
    pub fn trigger_emergency_disconnect(&self) {
        self.set_stopped(true);
        self.set_status(ClientConnectionStatus::Disconnected);

        let ws_client_opt = self.ws_client.read().unwrap().clone();
        if let Some(client) = ws_client_opt {
            let executor = client.executor.clone();
            executor.kill_all_processes();

            self.spawn_async(async move {
                client
                    .disconnect("Emergency disconnect triggered by user in Agent UI")
                    .await;
            });
        }

        self.add_audit_log(
            "emergency_disconnect",
            "Emergency disconnect triggered by user",
            "STOPPED",
        );
    }

    /// Resets stopped status and reconnects the client to the server.
    pub fn trigger_reconnect(&self) {
        self.set_stopped(false);
        self.set_status(ClientConnectionStatus::Connecting);

        let ws_client_opt = self.ws_client.read().unwrap().clone();
        if let Some(client) = ws_client_opt {
            client.reset_running();
            client.notify_reconnect();
            self.spawn_async(async move {
                client.run().await;
            });
        }

        self.add_audit_log(
            "agent_reconnect",
            "Reconnection triggered by user in Agent UI",
            "STARTED",
        );
    }

    /// Updates target server URL, optionally writes configuration to disk, and triggers reconnect.
    pub fn update_server_url(&self, new_url: String, persist: bool) -> Result<(), String> {
        let trimmed = new_url.trim().to_string();
        if trimmed.is_empty() {
            return Err("Server URL cannot be empty".to_string());
        }

        // Validate URL format
        if !trimmed.starts_with("ws://") && !trimmed.starts_with("wss://") {
            return Err("Server URL must start with 'ws://' or 'wss://'".to_string());
        }

        crate::ws_client::parse_ws_url(&trimmed)
            .map_err(|e| format!("Invalid WebSocket URL: {}", e))?;

        // 1. Update in-memory server URL
        self.set_server_url(trimmed.clone());

        // 2. Optionally persist to disk
        if persist {
            if let Err(e) = AgentConfig::save_server_url(&trimmed, None) {
                tracing::warn!("Failed to persist server URL to config file: {}", e);
            }
        }

        // 3. Update client and trigger reconnect
        let ws_client_opt = self.ws_client.read().unwrap().clone();
        if let Some(client) = ws_client_opt {
            self.set_stopped(false);
            self.set_status(ClientConnectionStatus::Connecting);

            let url_clone = trimmed.clone();
            self.spawn_async(async move {
                client.set_server_url(url_clone).await;
                client.reset_running();
                client.abort_active_session().await;
                client.run().await;
            });
        }

        self.add_audit_log(
            "server_url_updated",
            &format!("Server URL updated to: {}", trimmed),
            "UPDATED",
        );

        Ok(())
    }
}

const TOOL_ARGUMENT_SUMMARY_MAX_SCALARS: usize = 80;

/// Returns a prefix containing at most `max_scalars` Unicode scalar values.
///
/// The boolean is true when the input contained additional scalar values.
fn truncate_to_scalars(value: &str, max_scalars: usize) -> (&str, bool) {
    match value.char_indices().nth(max_scalars) {
        Some((byte_index, _)) => (&value[..byte_index], true),
        None => (value, false),
    }
}

impl AgentEventListener for AgentAppState {
    fn on_status_change(&self, status: ClientConnectionStatus) {
        self.set_status(status);
    }

    fn on_tool_start(&self, call_id: &str, tool_name: &str, arguments: &serde_json::Value) {
        let args_str = serde_json::to_string(arguments).unwrap_or_default();
        let (prefix, truncated) = truncate_to_scalars(&args_str, TOOL_ARGUMENT_SUMMARY_MAX_SCALARS);
        let summary = if truncated {
            format!("{prefix}...")
        } else {
            args_str
        };
        let entry = AgentAuditLog {
            id: call_id.to_string(),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            tool_name: tool_name.to_string(),
            summary,
            status: "STARTED".to_string(),
            duration_ms: None,
        };
        self.add_audit_log_entry(entry);
    }

    fn on_tool_finish(&self, call_id: &str, tool_name: &str, success: bool, duration_ms: u64) {
        let status = if success { "SUCCESS" } else { "FAILED" };
        let summary = format!("Finished in {}ms", duration_ms);
        let entry = AgentAuditLog {
            id: call_id.to_string(),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            tool_name: tool_name.to_string(),
            summary,
            status: status.to_string(),
            duration_ms: Some(duration_ms),
        };
        self.add_audit_log_entry(entry);
    }
}
