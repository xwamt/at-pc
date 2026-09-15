use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

use at_pc_protocol::messages::ServerToAgentMessage;
use at_pc_protocol::models::{HeartbeatMetrics, TerminalInfo};
pub use at_pc_protocol::models::TerminalStatus;

use std::sync::atomic::{AtomicU64, Ordering};
static SESSION_SEQ: AtomicU64 = AtomicU64::new(1);

use crate::meta_store::{TerminalMeta, TerminalMetaStore};

/// Active terminal session stored in memory
#[derive(Debug, Clone)]
pub struct TerminalSession {
    pub session_id: u64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub status: TerminalStatus,
    pub latest_metrics: Option<HeartbeatMetrics>,
    pub last_heartbeat_elapsed_secs: u64,
}

/// Central registry managing all online/offline terminals and WebSocket sender channels
pub struct TerminalRegistry {
    sessions: Arc<RwLock<HashMap<String, TerminalSession>>>,
    meta_store: Arc<TerminalMetaStore>,
    offline_threshold: Duration,
}

impl Default for TerminalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalRegistry {
    /// Create a new registry with default 15-second offline threshold and in-memory meta store
    pub fn new() -> Self {
        Self::with_threshold(Duration::from_secs(15))
    }

    /// Create a new registry with custom offline threshold and in-memory meta store
    pub fn with_threshold(offline_threshold: Duration) -> Self {
        Self::with_store(offline_threshold, Arc::new(TerminalMetaStore::in_memory()))
    }

    /// Create a new registry with custom threshold and explicit meta store
    pub fn with_store(offline_threshold: Duration, meta_store: Arc<TerminalMetaStore>) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            meta_store,
            offline_threshold,
        }
    }

    /// Access the underlying metadata store
    pub fn meta_store(&self) -> &Arc<TerminalMetaStore> {
        &self.meta_store
    }

    /// Register a newly connected terminal or re-register an existing one
    pub async fn register(
        &self,
        info: TerminalInfo,
        ws_sender: mpsc::UnboundedSender<ServerToAgentMessage>,
    ) -> u64 {
        let terminal_id = info.terminal_id.clone();
        self.meta_store.record_registration(&info).await;
        let session_id = SESSION_SEQ.fetch_add(1, Ordering::SeqCst);
        let mut sessions = self.sessions.write().await;

        let session = TerminalSession {
            session_id,
            info,
            status: TerminalStatus::Online,
            latest_metrics: None,
            last_heartbeat_at: Instant::now(),
            ws_sender,
        };

        info!("Registered terminal: {} (session: {})", terminal_id, session_id);
        sessions.insert(terminal_id, session);
        session_id
    }

    /// Remove a terminal session entirely from the registry
    pub async fn unregister(&self, terminal_id: &str) -> Option<TerminalSession> {
        let mut sessions = self.sessions.write().await;
        let removed = sessions.remove(terminal_id);
        if removed.is_some() {
            info!("Unregistered terminal: {}", terminal_id);
        }
        let _ = self.meta_store.remove(terminal_id).await;
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

    /// Set status of a terminal only if the current session matches session_id
    pub async fn set_status_if_current(
        &self,
        terminal_id: &str,
        session_id: u64,
        status: TerminalStatus,
    ) -> bool {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(terminal_id) {
            if session.session_id == session_id {
                session.status = status;
                return true;
            }
        }
        false
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

        if sender.send(msg).is_err() {
            warn!("Failed to send message to terminal [{}]: channel receiver closed. Marking Offline.", terminal_id);
            self.set_status(terminal_id, TerminalStatus::Offline).await;
            return Err(format!("Failed to send message to terminal {}", terminal_id));
        }
        Ok(())
    }

    /// List all registered terminals (including active sessions and persisted offline terminals)
    pub async fn list_terminals(&self) -> Vec<TerminalEntry> {
        let sessions = self.sessions.read().await;
        let mut entries: Vec<TerminalEntry> = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        for s in sessions.values() {
            seen_ids.insert(s.info.terminal_id.clone());
            let meta = self.meta_store.get(&s.info.terminal_id).await;
            entries.push(TerminalEntry {
                info: s.info.clone(),
                custom_name: meta.as_ref().and_then(|m| m.custom_name.clone()),
                notes: meta.as_ref().and_then(|m| m.notes.clone()),
                tags: meta.as_ref().map(|m| m.tags.clone()).unwrap_or_default(),
                status: s.status,
                latest_metrics: s.latest_metrics.clone(),
                last_heartbeat_elapsed_secs: s.last_heartbeat_at.elapsed().as_secs(),
            });
        }

        // Include any persisted terminals that are currently offline and not in active sessions
        let all_meta = self.meta_store.list_all().await;
        for (id, meta) in all_meta {
            if !seen_ids.contains(&id) {
                if let Some(info) = meta.last_known_info {
                    entries.push(TerminalEntry {
                        info,
                        custom_name: meta.custom_name,
                        notes: meta.notes,
                        tags: meta.tags,
                        status: TerminalStatus::Offline,
                        latest_metrics: None,
                        last_heartbeat_elapsed_secs: 999999,
                    });
                }
            }
        }

        // Sort deterministically by terminal_id
        entries.sort_by(|a, b| a.info.terminal_id.cmp(&b.info.terminal_id));
        entries
    }

    /// Get details of a single terminal
    pub async fn get_terminal(&self, terminal_id: &str) -> Option<TerminalEntry> {
        let sessions = self.sessions.read().await;
        if let Some(s) = sessions.get(terminal_id) {
            let meta = self.meta_store.get(terminal_id).await;
            return Some(TerminalEntry {
                info: s.info.clone(),
                custom_name: meta.as_ref().and_then(|m| m.custom_name.clone()),
                notes: meta.as_ref().and_then(|m| m.notes.clone()),
                tags: meta.as_ref().map(|m| m.tags.clone()).unwrap_or_default(),
                status: s.status,
                latest_metrics: s.latest_metrics.clone(),
                last_heartbeat_elapsed_secs: s.last_heartbeat_at.elapsed().as_secs(),
            });
        }

        if let Some(meta) = self.meta_store.get(terminal_id).await {
            if let Some(info) = meta.last_known_info {
                return Some(TerminalEntry {
                    info,
                    custom_name: meta.custom_name,
                    notes: meta.notes,
                    tags: meta.tags,
                    status: TerminalStatus::Offline,
                    latest_metrics: None,
                    last_heartbeat_elapsed_secs: 999999,
                });
            }
        }

        None
    }

    /// Update custom metadata (name, notes, tags) for a terminal
    pub async fn update_terminal_meta(
        &self,
        terminal_id: &str,
        custom_name: Option<String>,
        notes: Option<String>,
        tags: Option<Vec<String>>,
    ) -> Result<TerminalMeta, String> {
        self.meta_store
            .update_meta(terminal_id, custom_name, notes, tags)
            .await
    }

    /// Explicitly remove a terminal from both session registry and persistent metadata store
    pub async fn remove_terminal(&self, terminal_id: &str) -> bool {
        let unreg = self.unregister(terminal_id).await.is_some();
        let meta_rem = self.meta_store.remove(terminal_id).await.unwrap_or(false);
        unreg || meta_rem
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
        self.start_sweep_task_with_handler(sweep_interval, Arc::new(crate::ws::handler::NoopMessageHandler))
    }

    /// Start a background sweep task that notifies a message handler when terminals are swept offline
    pub fn start_sweep_task_with_handler<H: crate::ws::handler::AgentMessageHandler + 'static>(
        self: Arc<Self>,
        sweep_interval: Duration,
        handler: Arc<H>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(sweep_interval);
            loop {
                interval.tick().await;
                let offline_ids = self.sweep_offline().await;
                for tid in offline_ids {
                    handler.handle_terminal_disconnected(&tid, "Heartbeat timed out");
                }
            }
        })
    }
}
