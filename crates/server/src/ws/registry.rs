use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

use at_pc_protocol::messages::ServerToAgentMessage;
pub use at_pc_protocol::models::TerminalStatus;
use at_pc_protocol::models::{HeartbeatMetrics, TerminalInfo};

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

#[cfg(test)]
#[derive(Debug)]
struct MetaReadGate {
    entered: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
}

#[cfg(test)]
impl Default for MetaReadGate {
    fn default() -> Self {
        Self {
            entered: tokio::sync::Semaphore::new(0),
            release: tokio::sync::Semaphore::new(0),
        }
    }
}

#[cfg(test)]
impl MetaReadGate {
    async fn wait(&self) {
        self.entered.add_permits(1);
        self.release
            .acquire()
            .await
            .expect("meta read gate should remain open")
            .forget();
    }
}

/// Central registry managing all online/offline terminals and WebSocket sender channels
pub struct TerminalRegistry {
    sessions: Arc<RwLock<HashMap<String, TerminalSession>>>,
    meta_store: Arc<TerminalMetaStore>,
    offline_threshold: Duration,
    #[cfg(test)]
    meta_read_gate: Option<Arc<MetaReadGate>>,
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
            #[cfg(test)]
            meta_read_gate: None,
        }
    }

    #[cfg(test)]
    fn with_meta_read_gate(
        offline_threshold: Duration,
        meta_store: Arc<TerminalMetaStore>,
        meta_read_gate: Arc<MetaReadGate>,
    ) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            meta_store,
            offline_threshold,
            meta_read_gate: Some(meta_read_gate),
        }
    }

    #[cfg(test)]
    async fn wait_at_meta_read_gate(&self) {
        if let Some(gate) = &self.meta_read_gate {
            gate.wait().await;
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

        info!(
            "Registered terminal: {} (session: {})",
            terminal_id, session_id
        );
        sessions.insert(terminal_id, session);
        session_id
    }

    /// Remove a terminal session entirely from the registry
    pub async fn unregister(&self, terminal_id: &str) -> Option<TerminalSession> {
        let removed = {
            let mut sessions = self.sessions.write().await;
            sessions.remove(terminal_id)
        };
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
            return Err(format!(
                "Failed to send message to terminal {}",
                terminal_id
            ));
        }
        Ok(())
    }

    /// List all registered terminals (including active sessions and persisted offline terminals)
    pub async fn list_terminals(&self) -> Vec<TerminalEntry> {
        // Snapshot sessions before reading metadata so registry writers are never blocked by
        // metadata I/O. A session present in this snapshot wins over persisted offline data.
        let session_snapshot: Vec<(TerminalInfo, TerminalStatus, Option<HeartbeatMetrics>, u64)> = {
            let sessions = self.sessions.read().await;
            let mut snapshot = Vec::with_capacity(sessions.len());
            for session in sessions.values() {
                snapshot.push((
                    session.info.clone(),
                    session.status,
                    session.latest_metrics.clone(),
                    session.last_heartbeat_at.elapsed().as_secs(),
                ));
            }
            snapshot
        };

        #[cfg(test)]
        self.wait_at_meta_read_gate().await;

        // Fast in-memory metadata read (Arc<HashMap> read lock is <1µs)
        let all_meta = self.meta_store.list_all().await;
        let mut entries = Vec::with_capacity(all_meta.len().max(session_snapshot.len()));

        for (info, status, latest_metrics, elapsed) in session_snapshot {
            let meta = all_meta.get(&info.terminal_id);
            entries.push(TerminalEntry {
                info,
                custom_name: meta.and_then(|m| m.custom_name.clone()),
                notes: meta.and_then(|m| m.notes.clone()),
                tags: meta.map(|m| m.tags.clone()).unwrap_or_default(),
                status,
                latest_metrics,
                last_heartbeat_elapsed_secs: elapsed,
            });
        }

        // Include persisted terminals absent from active session snapshot as offline.
        if entries.len() < all_meta.len() {
            let active_ids: std::collections::HashSet<&str> = entries
                .iter()
                .map(|e| e.info.terminal_id.as_str())
                .collect();
            let mut offline_entries = Vec::new();
            for (terminal_id, meta) in all_meta.iter() {
                if !active_ids.contains(terminal_id.as_str()) {
                    if let Some(info) = &meta.last_known_info {
                        offline_entries.push(TerminalEntry {
                            info: info.clone(),
                            custom_name: meta.custom_name.clone(),
                            notes: meta.notes.clone(),
                            tags: meta.tags.clone(),
                            status: TerminalStatus::Offline,
                            latest_metrics: None,
                            last_heartbeat_elapsed_secs: 999999,
                        });
                    }
                }
            }
            drop(active_ids);
            entries.extend(offline_entries);
        }

        // Sort deterministically by terminal_id
        entries.sort_unstable_by(|a, b| a.info.terminal_id.cmp(&b.info.terminal_id));
        entries
    }

    /// Get details of a single terminal
    pub async fn get_terminal(&self, terminal_id: &str) -> Option<TerminalEntry> {
        let session_snapshot = {
            let sessions = self.sessions.read().await;
            sessions.get(terminal_id).map(|s| {
                (
                    s.info.clone(),
                    s.status,
                    s.latest_metrics.clone(),
                    s.last_heartbeat_at.elapsed().as_secs(),
                )
            })
        };

        #[cfg(test)]
        self.wait_at_meta_read_gate().await;

        let meta = self.meta_store.get(terminal_id).await;
        if let Some((info, status, latest_metrics, elapsed)) = session_snapshot {
            return Some(TerminalEntry {
                info,
                custom_name: meta.as_ref().and_then(|m| m.custom_name.clone()),
                notes: meta.as_ref().and_then(|m| m.notes.clone()),
                tags: meta.map(|m| m.tags).unwrap_or_default(),
                status,
                latest_metrics,
                last_heartbeat_elapsed_secs: elapsed,
            });
        }

        if let Some(meta) = meta {
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
            if session.status != TerminalStatus::Offline
                && session.last_heartbeat_at.elapsed() > threshold
            {
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
        self.start_sweep_task_with_handler(
            sweep_interval,
            Arc::new(crate::ws::handler::NoopMessageHandler),
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use tokio::time::timeout;

    fn terminal_info(terminal_id: &str) -> TerminalInfo {
        TerminalInfo {
            terminal_id: terminal_id.to_string(),
            hostname: format!("host-{terminal_id}"),
            username: "tester".to_string(),
            lan_ip: "127.0.0.1".to_string(),
            os_version: "test-os".to_string(),
            agent_version: "test-agent".to_string(),
        }
    }

    fn heartbeat(timestamp: i64) -> HeartbeatMetrics {
        HeartbeatMetrics {
            cpu_usage_percent: 12.5,
            memory_used_mb: 1024,
            memory_total_mb: 4096,
            uptime_secs: 60,
            timestamp,
        }
    }

    async fn wait_until_meta_read_is_paused(gate: &MetaReadGate) {
        timeout(Duration::from_secs(1), gate.entered.acquire())
            .await
            .expect("registry read should reach the metadata slow path")
            .expect("meta read gate should remain open")
            .forget();
    }

    async fn assert_registry_writes_make_progress(registry: Arc<TerminalRegistry>) {
        let (tx, _rx) = mpsc::unbounded_channel();
        let register_registry = Arc::clone(&registry);
        let register = tokio::spawn(async move {
            register_registry
                .register(terminal_info("node-b"), tx)
                .await;
        });

        let heartbeat_registry = Arc::clone(&registry);
        let heartbeat = tokio::spawn(async move {
            heartbeat_registry
                .update_heartbeat("node-a", heartbeat(42))
                .await
        });

        timeout(Duration::from_secs(1), async {
            register.await.expect("register task should complete");
            heartbeat
                .await
                .expect("heartbeat task should complete")
                .expect("heartbeat should find node-a");
        })
        .await
        .expect("sessions writers must not wait for the metadata slow path");
    }

    #[tokio::test]
    async fn list_terminals_releases_sessions_lock_before_meta_read_and_uses_snapshots() {
        let meta_store = Arc::new(TerminalMetaStore::in_memory());
        let gate = Arc::new(MetaReadGate::default());
        let registry = Arc::new(TerminalRegistry::with_meta_read_gate(
            Duration::from_secs(15),
            Arc::clone(&meta_store),
            Arc::clone(&gate),
        ));

        let (tx, _rx) = mpsc::unbounded_channel();
        registry.register(terminal_info("node-a"), tx).await;
        registry
            .update_terminal_meta(
                "node-a",
                Some("primary".to_string()),
                None,
                Some(vec!["online".to_string()]),
            )
            .await
            .unwrap();
        meta_store
            .record_registration(&terminal_info("node-c"))
            .await;
        meta_store
            .update_meta("node-d", Some("metadata-only".to_string()), None, None)
            .await
            .unwrap();

        let list_registry = Arc::clone(&registry);
        let list = tokio::spawn(async move { list_registry.list_terminals().await });
        wait_until_meta_read_is_paused(&gate).await;

        assert_registry_writes_make_progress(Arc::clone(&registry)).await;
        gate.release.add_permits(1);

        let entries = list.await.expect("list task should complete");
        let ids: Vec<_> = entries
            .iter()
            .map(|entry| entry.info.terminal_id.as_str())
            .collect();
        assert_eq!(ids, vec!["node-a", "node-b", "node-c"]);
        assert_eq!(ids.iter().copied().collect::<HashSet<_>>().len(), ids.len());

        let node_a = &entries[0];
        assert_eq!(node_a.status, TerminalStatus::Online);
        assert_eq!(node_a.custom_name.as_deref(), Some("primary"));
        assert_eq!(node_a.tags, vec!["online".to_string()]);
        assert!(node_a.latest_metrics.is_none());

        // node-b registered after the sessions snapshot, so the later metadata snapshot
        // supplements it as offline rather than mixing it into the earlier session view.
        assert_eq!(entries[1].status, TerminalStatus::Offline);
        assert_eq!(entries[2].status, TerminalStatus::Offline);

        assert_eq!(
            registry.get_status("node-b").await,
            Some(TerminalStatus::Online)
        );
        assert!(registry
            .sessions
            .read()
            .await
            .get("node-a")
            .and_then(|session| session.latest_metrics.as_ref())
            .is_some());
    }

    #[tokio::test]
    async fn get_terminal_releases_sessions_lock_before_meta_read_and_keeps_session_snapshot() {
        let meta_store = Arc::new(TerminalMetaStore::in_memory());
        let gate = Arc::new(MetaReadGate::default());
        let registry = Arc::new(TerminalRegistry::with_meta_read_gate(
            Duration::from_secs(15),
            meta_store,
            Arc::clone(&gate),
        ));

        let (tx, _rx) = mpsc::unbounded_channel();
        registry.register(terminal_info("node-a"), tx).await;

        let get_registry = Arc::clone(&registry);
        let get = tokio::spawn(async move { get_registry.get_terminal("node-a").await });
        wait_until_meta_read_is_paused(&gate).await;

        assert_registry_writes_make_progress(Arc::clone(&registry)).await;
        gate.release.add_permits(1);

        let entry = get
            .await
            .expect("get task should complete")
            .expect("node-a should come from the sessions snapshot");
        assert_eq!(entry.info.terminal_id, "node-a");
        assert_eq!(entry.status, TerminalStatus::Online);
        assert!(entry.latest_metrics.is_none());

        assert!(registry
            .sessions
            .read()
            .await
            .get("node-a")
            .and_then(|session| session.latest_metrics.as_ref())
            .is_some());
        assert_eq!(
            registry.get_status("node-b").await,
            Some(TerminalStatus::Online)
        );
    }
}
