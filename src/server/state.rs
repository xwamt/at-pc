//! Shared application state, active session tracking, and audit logging channels.

use crate::tools::process_registry::ProcessRegistry;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use tokio::sync::{broadcast, watch};

/// Status for audit log entries. Serializes to uppercase and supports case-insensitive deserialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AuditLogStatus {
    #[serde(alias = "success", alias = "SUCCESS")]
    Success,
    #[serde(alias = "failed", alias = "FAILED")]
    Failed,
    #[serde(alias = "error", alias = "ERROR")]
    Error,
    #[serde(alias = "stopped", alias = "STOPPED")]
    Stopped,
    #[serde(alias = "started", alias = "STARTED")]
    Started,
}

impl AuditLogStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuditLogStatus::Success => "SUCCESS",
            AuditLogStatus::Failed => "FAILED",
            AuditLogStatus::Error => "ERROR",
            AuditLogStatus::Stopped => "STOPPED",
            AuditLogStatus::Started => "STARTED",
        }
    }
}

impl std::fmt::Display for AuditLogStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<&str> for AuditLogStatus {
    fn from(s: &str) -> Self {
        match s.to_ascii_uppercase().as_str() {
            "SUCCESS" => AuditLogStatus::Success,
            "FAILED" => AuditLogStatus::Failed,
            "ERROR" => AuditLogStatus::Error,
            "STOPPED" => AuditLogStatus::Stopped,
            "STARTED" => AuditLogStatus::Started,
            _ => AuditLogStatus::Success,
        }
    }
}

impl From<String> for AuditLogStatus {
    fn from(s: String) -> Self {
        AuditLogStatus::from(s.as_str())
    }
}

impl PartialEq<&str> for AuditLogStatus {
    fn eq(&self, other: &&str) -> bool {
        self.as_str().eq_ignore_ascii_case(other)
    }
}

impl PartialEq<str> for AuditLogStatus {
    fn eq(&self, other: &str) -> bool {
        self.as_str().eq_ignore_ascii_case(other)
    }
}

impl PartialEq<String> for AuditLogStatus {
    fn eq(&self, other: &String) -> bool {
        self.as_str().eq_ignore_ascii_case(other)
    }
}

impl PartialEq<AuditLogStatus> for &str {
    fn eq(&self, other: &AuditLogStatus) -> bool {
        other.as_str().eq_ignore_ascii_case(self)
    }
}

impl PartialEq<AuditLogStatus> for String {
    fn eq(&self, other: &AuditLogStatus) -> bool {
        other.as_str().eq_ignore_ascii_case(self)
    }
}

/// Audit log entry representing an MCP tool invocation or system lifecycle event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditLogEntry {
    pub timestamp: String,
    pub tool_name: String,
    pub parameters: serde_json::Value,
    pub status: AuditLogStatus,
    pub duration_ms: Option<u64>,
    pub client_ip: Option<String>,
    pub message: Option<String>,
}

impl AuditLogEntry {
    /// Creates a new audit log entry with current local time.
    pub fn new(
        tool_name: impl Into<String>,
        parameters: serde_json::Value,
        status: impl Into<AuditLogStatus>,
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
    /// 4-digit PIN required for authentication, wrapped in RwLock for dynamic invalidation and rotation.
    pub pin: RwLock<String>,
    /// Active listening port.
    pub port: AtomicU16,
    /// Number of active connected clients.
    pub connected_clients: AtomicUsize,
    /// Broadcast channel for real-time audit log streaming to UI and subscribers.
    pub audit_sender: broadcast::Sender<AuditLogEntry>,
    /// Watch channel to signal graceful server shutdown / emergency disconnect.
    pub shutdown_sender: RwLock<watch::Sender<bool>>,
    /// Whether the server has been marked as stopped / killed.
    pub is_stopped: AtomicBool,
    /// Registry tracking spawned command subprocesses for clean emergency termination.
    pub process_registry: Arc<ProcessRegistry>,
}

impl AppState {
    /// Creates a new `AppState` instance.
    pub fn new(pin: String, port: u16) -> Self {
        let (audit_sender, _) = broadcast::channel(1024);
        let (shutdown_sender, _) = watch::channel(false);

        Self {
            pin: RwLock::new(pin),
            port: AtomicU16::new(port),
            connected_clients: AtomicUsize::new(0),
            audit_sender,
            shutdown_sender: RwLock::new(shutdown_sender),
            is_stopped: AtomicBool::new(false),
            process_registry: ProcessRegistry::global(),
        }
    }

    /// Creates an `AppState` with an explicit `ProcessRegistry`.
    pub fn with_registry(pin: String, port: u16, registry: Arc<ProcessRegistry>) -> Self {
        let (audit_sender, _) = broadcast::channel(1024);
        let (shutdown_sender, _) = watch::channel(false);

        Self {
            pin: RwLock::new(pin),
            port: AtomicU16::new(port),
            connected_clients: AtomicUsize::new(0),
            audit_sender,
            shutdown_sender: RwLock::new(shutdown_sender),
            is_stopped: AtomicBool::new(false),
            process_registry: registry,
        }
    }

    /// Returns a copy of the current active PIN.
    pub fn get_pin(&self) -> String {
        self.pin.read().map(|p| p.clone()).unwrap_or_default()
    }

    /// Updates the active PIN to a new value.
    pub fn set_pin(&self, new_pin: String) {
        if let Ok(mut lock) = self.pin.write() {
            *lock = new_pin;
        }
    }

    /// Invalidates the active PIN by clearing it to an empty string.
    pub fn invalidate_pin(&self) {
        if let Ok(mut lock) = self.pin.write() {
            lock.clear();
        }
    }

    /// Verifies a provided PIN against the live mutable PIN.
    /// Returns false if the server is stopped, or if either PIN is empty.
    pub fn verify_pin(&self, provided: &str) -> bool {
        if self.is_stopped() {
            return false;
        }
        let current_pin = self.get_pin();
        if current_pin.is_empty() || provided.is_empty() {
            return false;
        }
        crate::utils::security::verify_pin(&current_pin, provided)
    }

    /// Returns whether the server is marked as stopped.
    pub fn is_stopped(&self) -> bool {
        self.is_stopped.load(Ordering::SeqCst)
    }

    /// Triggers the emergency stop procedure:
    /// 1. Marks `is_stopped` as true.
    /// 2. Invalidates the PIN immediately so all in-flight and subsequent requests fail.
    /// 3. Forcibly terminates all active command subprocesses.
    /// 4. Broadcasts an audit event with status STOPPED.
    /// 5. Signals graceful server shutdown.
    pub fn trigger_emergency_stop(&self) {
        self.is_stopped.store(true, Ordering::SeqCst);
        self.invalidate_pin();
        self.process_registry.kill_all_active();

        let stop_entry = AuditLogEntry::new(
            "emergency_stop",
            serde_json::json!({}),
            AuditLogStatus::Stopped,
            None,
            None,
            Some("服务已通过紧急熔断开关停止".to_string()),
        );
        self.broadcast_audit(stop_entry);
        self.trigger_shutdown();
    }

    /// Subscribes to the audit log broadcast channel.
    pub fn subscribe_audit(&self) -> broadcast::Receiver<AuditLogEntry> {
        self.audit_sender.subscribe()
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

    /// Decrements the active client count and returns the updated count, preventing underflow.
    pub fn decrement_clients(&self) -> usize {
        let _ = self
            .connected_clients
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                Some(v.saturating_sub(1))
            });
        self.connected_clients.load(Ordering::SeqCst)
    }

    /// Returns the current active connected client count.
    pub fn connected_client_count(&self) -> usize {
        self.connected_clients.load(Ordering::SeqCst)
    }

    /// Returns the active listening port.
    pub fn get_port(&self) -> u16 {
        self.port.load(Ordering::SeqCst)
    }

    /// Updates the active listening port.
    pub fn set_port(&self, port: u16) {
        self.port.store(port, Ordering::SeqCst);
    }

    /// Rotates the security PIN and optionally updates the listening port.
    ///
    /// Generates a new 4-digit PIN (or uses provided `new_pin`), optionally updates port,
    /// updates internal state, and broadcasts a `pin_rotated` audit log entry.
    pub fn rotate_credentials(&self, new_pin: Option<String>, new_port: Option<u16>) -> (String, u16) {
        let pin = new_pin.unwrap_or_else(crate::utils::security::generate_pin);
        let port = new_port.unwrap_or_else(|| self.get_port());
        self.set_pin(pin.clone());
        self.set_port(port);

        let entry = AuditLogEntry::new(
            "pin_rotated",
            serde_json::json!({ "port": port, "pin_length": pin.len() }),
            AuditLogStatus::Success,
            None,
            None,
            Some(format!("PIN 已轮换，服务端口: {}", port)),
        );
        self.broadcast_audit(entry);

        (pin, port)
    }

    /// Restarts a session after emergency stop or manual reset.
    ///
    /// Resets `is_stopped` to false, creates a fresh shutdown watch channel,
    /// generates a new PIN, updates port (if specified), and broadcasts `session_restarted` audit entry.
    pub fn restart_session(&self, port: Option<u16>) -> String {
        self.is_stopped.store(false, Ordering::SeqCst);
        let (new_shutdown_tx, _) = watch::channel(false);
        if let Ok(mut lock) = self.shutdown_sender.write() {
            *lock = new_shutdown_tx;
        }

        let new_pin = crate::utils::security::generate_pin();
        self.set_pin(new_pin.clone());

        let port = port.unwrap_or_else(|| self.get_port());
        self.set_port(port);

        let entry = AuditLogEntry::new(
            "session_restarted",
            serde_json::json!({ "port": port }),
            AuditLogStatus::Started,
            None,
            None,
            Some(format!("会话已重新启动，新 PIN 已生成，监听端口: {}", port)),
        );
        self.broadcast_audit(entry);

        new_pin
    }

    /// Triggers graceful server shutdown.
    pub fn trigger_shutdown(&self) {
        if let Ok(sender) = self.shutdown_sender.read() {
            let _ = sender.send(true);
        }
    }

    /// Subscribes to the shutdown watch channel.
    pub fn subscribe_shutdown(&self) -> watch::Receiver<bool> {
        self.shutdown_sender
            .read()
            .map(|s| s.subscribe())
            .unwrap_or_else(|_| {
                let (_, rx) = watch::channel(false);
                rx
            })
    }
}
