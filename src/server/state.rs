//! Shared application state, active session tracking, and audit logging channels.

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{broadcast, watch};

/// Audit log entry representing an MCP tool invocation or system lifecycle event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditLogEntry {
    pub timestamp: String,
    pub tool_name: String,
    pub parameters: serde_json::Value,
    pub status: String,
    pub duration_ms: Option<u64>,
    pub client_ip: Option<String>,
    pub message: Option<String>,
}

impl AuditLogEntry {
    /// Creates a new audit log entry with current local time.
    pub fn new(
        tool_name: impl Into<String>,
        parameters: serde_json::Value,
        status: impl Into<String>,
        duration_ms: Option<u64>,
        client_ip: Option<String>,
        message: Option<String>,
    ) -> Self {
        let timestamp = Local::now().format("%H:%M:%S").to_string();
        Self {
            timestamp,
            tool_name: tool_name.into(),
            parameters,
            status: status.into(),
            duration_ms,
            client_ip,
            message,
        }
    }
}

/// Shared application state across HTTP/SSE server and GUI.
pub struct AppState {
    /// 4-digit PIN required for authentication.
    pub pin: String,
    /// Active listening port.
    pub port: u16,
    /// Number of active connected clients.
    pub connected_clients: AtomicUsize,
    /// Broadcast channel for real-time audit log streaming to UI and subscribers.
    pub audit_sender: broadcast::Sender<AuditLogEntry>,
    /// Watch channel to signal graceful server shutdown / emergency disconnect.
    pub shutdown_sender: watch::Sender<bool>,
}

impl AppState {
    /// Creates a new `AppState` instance.
    pub fn new(pin: String, port: u16) -> Self {
        let (audit_sender, _) = broadcast::channel(1024);
        let (shutdown_sender, _) = watch::channel(false);

        Self {
            pin,
            port,
            connected_clients: AtomicUsize::new(0),
            audit_sender,
            shutdown_sender,
        }
    }

    /// Broadcasts an audit log entry to all subscribers.
    pub fn broadcast_audit(&self, entry: AuditLogEntry) {
        // Ignore error if there are no active receivers
        let _ = self.audit_sender.send(entry);
    }

    /// Increments the active client count and returns the updated count.
    pub fn increment_clients(&self) -> usize {
        self.connected_clients.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Decrements the active client count and returns the updated count.
    pub fn decrement_clients(&self) -> usize {
        let prev = self.connected_clients.fetch_sub(1, Ordering::SeqCst);
        if prev > 0 {
            prev - 1
        } else {
            0
        }
    }

    /// Returns the current active connected client count.
    pub fn connected_client_count(&self) -> usize {
        self.connected_clients.load(Ordering::SeqCst)
    }

    /// Triggers graceful server shutdown.
    pub fn trigger_shutdown(&self) {
        let _ = self.shutdown_sender.send(true);
    }

    /// Subscribes to the shutdown watch channel.
    pub fn subscribe_shutdown(&self) -> watch::Receiver<bool> {
        self.shutdown_sender.subscribe()
    }
}
