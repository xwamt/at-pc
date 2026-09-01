//! GUI state and helper structures for at-pc desktop application.

use crate::server::state::{AppState, AuditLogEntry};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;

/// State for the egui desktop UI.
pub struct GuiState {
    /// Local LAN IP address.
    pub lan_ip: String,
    /// Active server port.
    pub port: u16,
    /// 4-digit security PIN.
    pub pin: String,
    /// Shared backend AppState.
    pub server_state: Arc<AppState>,
    /// Real-time audit logs collected by the UI.
    pub audit_logs: Vec<AuditLogEntry>,
    /// Broadcast receiver for receiving audit logs.
    pub audit_rx: broadcast::Receiver<AuditLogEntry>,
    /// Timestamp when MCP config was last copied (for showing temporary "Copied!" feedback).
    pub last_copied_time: Option<Instant>,
    /// Whether the server has been manually stopped/disconnected.
    pub is_stopped: bool,
}

impl GuiState {
    /// Creates a new `GuiState` instance.
    pub fn new(lan_ip: String, server_state: Arc<AppState>) -> Self {
        let audit_rx = server_state.audit_sender.subscribe();
        let port = server_state.port;
        let pin = server_state.pin.clone();

        Self {
            lan_ip,
            port,
            pin,
            server_state,
            audit_logs: Vec::new(),
            audit_rx,
            last_copied_time: None,
            is_stopped: false,
        }
    }

    /// Polls and drains incoming audit log entries from the broadcast channel.
    pub fn poll_audit_logs(&mut self) {
        loop {
            match self.audit_rx.try_recv() {
                Ok(entry) => {
                    self.audit_logs.push(entry);
                }
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    // channel lagged; continue draining remaining logs
                    continue;
                }
                Err(broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Closed) => {
                    break;
                }
            }
        }
    }

    /// Returns the number of currently connected clients.
    pub fn connected_clients(&self) -> usize {
        self.server_state.connected_client_count()
    }

    /// Generates the standard MCP JSON configuration snippet for Claude Desktop/Cursor/Cline.
    pub fn generate_mcp_config(&self) -> String {
        let url = format!("http://{}:{}/sse", self.lan_ip, self.port);
        let config = serde_json::json!({
            "mcpServers": {
                "at-pc": {
                    "url": url,
                    "headers": {
                        "Authorization": format!("Bearer {}", self.pin)
                    }
                }
            }
        });
        serde_json::to_string_pretty(&config).unwrap_or_default()
    }

    /// Triggers emergency stop and disconnects all sessions.
    pub fn trigger_emergency_stop(&mut self) {
        self.is_stopped = true;
        self.server_state.trigger_shutdown();
        let stop_entry = AuditLogEntry::new(
            "emergency_stop",
            serde_json::json!({}),
            "STOPPED",
            None,
            None,
            Some("用户触发紧急断开，服务已停止。".to_string()),
        );
        self.audit_logs.push(stop_entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gui_state_init_and_mcp_config() {
        let app_state = Arc::new(AppState::new("8888".to_string(), 9800));
        let mut gui_state = GuiState::new("192.168.1.100".to_string(), app_state.clone());

        assert_eq!(gui_state.lan_ip, "192.168.1.100");
        assert_eq!(gui_state.port, 9800);
        assert_eq!(gui_state.pin, "8888");
        assert_eq!(gui_state.connected_clients(), 0);
        assert!(!gui_state.is_stopped);

        let config_str = gui_state.generate_mcp_config();
        let config: serde_json::Value = serde_json::from_str(&config_str).unwrap();
        assert_eq!(
            config["mcpServers"]["at-pc"]["url"],
            "http://192.168.1.100:9800/sse"
        );
        assert_eq!(
            config["mcpServers"]["at-pc"]["headers"]["Authorization"],
            "Bearer 8888"
        );

        // Test polling audit logs
        let entry = AuditLogEntry::new(
            "get_system_overview",
            serde_json::json!({}),
            "SUCCESS",
            Some(15),
            Some("192.168.1.50".to_string()),
            Some("System overview fetched".to_string()),
        );
        app_state.broadcast_audit(entry.clone());

        gui_state.poll_audit_logs();
        assert_eq!(gui_state.audit_logs.len(), 1);
        assert_eq!(gui_state.audit_logs[0].tool_name, "get_system_overview");

        // Test emergency stop
        gui_state.trigger_emergency_stop();
        assert!(gui_state.is_stopped);
        assert_eq!(gui_state.audit_logs.len(), 2);
        assert_eq!(gui_state.audit_logs[1].tool_name, "emergency_stop");
    }
}
