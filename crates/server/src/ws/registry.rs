use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

use at_pc_protocol::messages::ServerToAgentMessage;
use at_pc_protocol::models::{HeartbeatMetrics, TerminalInfo};
pub use at_pc_protocol::models::TerminalStatus;

/// Active terminal session stored in memory
#[derive(Debug, Clone)]
pub struct TerminalSession {
    pub info: TerminalInfo,
    pub status: TerminalStatus,
    pub latest_metrics: Option<HeartbeatMetrics>,
    pub last_heartbeat_at: Instant,
    pub ws_sender: mpsc::UnboundedSender<ServerToAgentMessage>,
}

/// Snapshot view of a terminal entry for API and MCP consumption
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TerminalEntry {
    pub info: TerminalInfo,
    pub status: TerminalStatus,
    pub latest_metrics: Option<HeartbeatMetrics>,
    pub last_heartbeat_elapsed_secs: u64,
}

/// Central registry managing all online/offline terminals and WebSocket sender channels
pub struct TerminalRegistry {
    sessions: Arc<RwLock<HashMap<String, TerminalSession>>>,
    offline_threshold: Duration,
}

impl Default for TerminalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalRegistry {
    /// Create a new registry with default 15-second offline threshold
    pub fn new() -> Self {
        Self::with_threshold(Duration::from_secs(15))
    }

    /// Create a new registry with custom offline threshold
    pub fn with_threshold(offline_threshold: Duration) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            offline_threshold,
        }
    }

    /// Register a newly connected terminal or re-register an existing one
    pub async fn register(
        &self,
        info: TerminalInfo,
        ws_sender: mpsc::UnboundedSender<ServerToAgentMessage>,
    ) {
        let terminal_id = info.terminal_id.clone();
        let mut sessions = self.sessions.write().await;

        let session = TerminalSession {
            info,
            status: TerminalStatus::Online,
            latest_metrics: None,
            last_heartbeat_at: Instant::now(),
            ws_sender,
        };

        info!("Registered terminal: {}", terminal_id);
        sessions.insert(terminal_id, session);
    }

    /// Remove a terminal session entirely from the registry
    pub async fn unregister(&self, terminal_id: &str) -> Option<TerminalSession> {
        let mut sessions = self.sessions.write().await;
        let removed = sessions.remove(terminal_id);
        if removed.is_some() {
            info!("Unregistered terminal: {}", terminal_id);
        }
        removed
    }

    /// Update terminal heartbeat metrics and refresh last heartbeat timestamp
    pub async fn update_heartbeat(
        &self,
        terminal_id: &str,
        metrics: HeartbeatMetrics,
    ) -> Result<(), String> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(terminal_id) {
            session.latest_metrics = Some(metrics);
            session.last_heartbeat_at = Instant::now();
            if session.status == TerminalStatus::Offline {
                session.status = TerminalStatus::Online;
            }
            debug!("Updated heartbeat for terminal: {}", terminal_id);
            Ok(())
        } else {
            Err(format!("Terminal not found: {}", terminal_id))
        }
    }

    /// Get current status of a terminal
    pub async fn get_status(&self, terminal_id: &str) -> Option<TerminalStatus> {
        let sessions = self.sessions.read().await;
        sessions.get(terminal_id).map(|s| s.status)
    }

    /// Set status of a terminal (e.g. Busy or Offline)
    pub async fn set_status(&self, terminal_id: &str, status: TerminalStatus) -> bool {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(terminal_id) {
            session.status = status;
            true
        } else {
            false
        }
    }

    /// Get sender channel for sending messages to a terminal
    pub async fn get_sender(
        &self,
        terminal_id: &str,
    ) -> Option<mpsc::UnboundedSender<ServerToAgentMessage>> {
        let sessions = self.sessions.read().await;
        sessions.get(terminal_id).map(|s| s.ws_sender.clone())
    }

    /// Send a message to a specific terminal
    pub async fn send_to_terminal(
        &self,
        terminal_id: &str,
        msg: ServerToAgentMessage,
    ) -> Result<(), String> {
        let sender = self
            .get_sender(terminal_id)
            .await
            .ok_or_else(|| format!("Terminal {} not found or disconnected", terminal_id))?;

        sender
            .send(msg)
            .map_err(|_| format!("Failed to send message to terminal {}", terminal_id))
    }

    /// List all registered terminals
    pub async fn list_terminals(&self) -> Vec<TerminalEntry> {
        let sessions = self.sessions.read().await;
        let mut entries: Vec<TerminalEntry> = sessions
            .values()
            .map(|s| TerminalEntry {
                info: s.info.clone(),
                status: s.status,
                latest_metrics: s.latest_metrics.clone(),
                last_heartbeat_elapsed_secs: s.last_heartbeat_at.elapsed().as_secs(),
            })
            .collect();

        // Sort deterministically by terminal_id
        entries.sort_by(|a, b| a.info.terminal_id.cmp(&b.info.terminal_id));
        entries
    }

    /// Get details of a single terminal
    pub async fn get_terminal(&self, terminal_id: &str) -> Option<TerminalEntry> {
        let sessions = self.sessions.read().await;
        sessions.get(terminal_id).map(|s| TerminalEntry {
            info: s.info.clone(),
            status: s.status,
            latest_metrics: s.latest_metrics.clone(),
            last_heartbeat_elapsed_secs: s.last_heartbeat_at.elapsed().as_secs(),
        })
    }

    /// Total count of registered terminals
    pub async fn count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.len()
    }

    /// Scan and mark terminals with expired heartbeat as Offline
    pub async fn sweep_offline(&self) -> Vec<String> {
        let mut sessions = self.sessions.write().await;
        let threshold = self.offline_threshold;
        let mut offline_ids = Vec::new();

        for (id, session) in sessions.iter_mut() {
            if session.status != TerminalStatus::Offline && session.last_heartbeat_at.elapsed() > threshold {
                session.status = TerminalStatus::Offline;
                offline_ids.push(id.clone());
                warn!("Terminal {} heartbeat timed out; marked Offline", id);
            }
        }

        offline_ids
    }

    /// Start a background sweep task that runs at periodic intervals
    pub fn start_sweep_task(
        self: Arc<Self>,
        sweep_interval: Duration,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(sweep_interval);
            loop {
                interval.tick().await;
                self.sweep_offline().await;
            }
        })
    }
}
