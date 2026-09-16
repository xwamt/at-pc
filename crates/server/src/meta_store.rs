use at_pc_protocol::TerminalInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, RwLock};
use tracing::{debug, error, info, warn};

const META_QUEUE_CAPACITY: usize = 64;
const META_DEBOUNCE: Duration = Duration::from_millis(500);
const SHUTDOWN_PERSIST_MAX_ATTEMPTS: usize = 4;
const SHUTDOWN_PERSIST_INITIAL_BACKOFF: Duration = Duration::from_millis(50);

/// Metadata stored persistently for each terminal.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TerminalMeta {
    pub terminal_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_known_info: Option<TerminalInfo>,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug)]
struct StoreState {
    records: Arc<HashMap<String, TerminalMeta>>,
    version: u64,
}

#[derive(Clone)]
struct Snapshot {
    records: Arc<HashMap<String, TerminalMeta>>,
    version: u64,
}

enum WriterCommand {
    Update(Snapshot),
    Flush(Snapshot, oneshot::Sender<Result<(), String>>),
    Shutdown(Snapshot, oneshot::Sender<Result<(), String>>),
}

#[derive(Debug)]
struct StoreInner {
    is_in_memory: bool,
    state: RwLock<StoreState>,
    sender: Mutex<Option<SyncSender<WriterCommand>>>,
    writer_thread: Mutex<Option<JoinHandle<()>>>,
    #[cfg(test)]
    persisted_writes: Arc<AtomicU64>,
    #[cfg(test)]
    persist_faults: Arc<PersistFaults>,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct PersistFaults {
    remaining_failures: AtomicU64,
    attempts: AtomicU64,
}

/// File-backed metadata store with short in-memory critical sections and one disk writer.
#[derive(Debug, Clone)]
pub struct TerminalMetaStore {
    inner: Arc<StoreInner>,
}

#[cfg(test)]
struct PersistGate {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

impl TerminalMetaStore {
    /// Creates a persistent store. Loading is synchronous startup work; all writes are handled
    /// by a single background writer and coalesced in approximately 500 ms windows.
    pub fn new<P: AsRef<Path>>(file_path: P) -> Self {
        Self::new_inner(file_path.as_ref().to_path_buf(), META_DEBOUNCE, None)
    }

    fn new_inner(
        file_path: PathBuf,
        debounce: Duration,
        #[cfg(test)] persist_gate: Option<PersistGate>,
        #[cfg(not(test))] _persist_gate: Option<()>,
    ) -> Self {
        let records = Self::load_from_disk(&file_path);
        let (sender, receiver) = mpsc::sync_channel(META_QUEUE_CAPACITY);
        let persisted_writes = Arc::new(AtomicU64::new(0));
        #[cfg(test)]
        let persist_faults = Arc::new(PersistFaults::default());
        let worker_path = file_path.clone();
        let worker_writes = Arc::clone(&persisted_writes);
        #[cfg(test)]
        let worker_faults = Arc::clone(&persist_faults);
        let writer_thread = std::thread::Builder::new()
            .name("at-pc-meta-writer".to_string())
            .spawn(move || {
                run_writer(
                    worker_path,
                    receiver,
                    debounce,
                    worker_writes,
                    #[cfg(test)]
                    worker_faults,
                    #[cfg(test)]
                    persist_gate,
                )
            })
            .expect("failed to start terminal metadata writer thread");

        Self {
            inner: Arc::new(StoreInner {
                is_in_memory: false,
                state: RwLock::new(StoreState {
                    records: Arc::new(records),
                    version: 0,
                }),
                sender: Mutex::new(Some(sender)),
                writer_thread: Mutex::new(Some(writer_thread)),
                #[cfg(test)]
                persisted_writes,
                #[cfg(test)]
                persist_faults,
            }),
        }
    }

    /// Creates an in-memory store that never writes to disk.
    pub fn in_memory() -> Self {
        Self {
            inner: Arc::new(StoreInner {
                is_in_memory: true,
                state: RwLock::new(StoreState {
                    records: Arc::new(HashMap::new()),
                    version: 0,
                }),
                sender: Mutex::new(None),
                writer_thread: Mutex::new(None),
                #[cfg(test)]
                persisted_writes: Arc::new(AtomicU64::new(0)),
                #[cfg(test)]
                persist_faults: Arc::new(PersistFaults::default()),
            }),
        }
    }

    fn load_from_disk(path: &Path) -> HashMap<String, TerminalMeta> {
        if !path.exists() || !path.is_file() {
            debug!(?path, "metadata file does not exist yet; starting empty");
            return HashMap::new();
        }

        match std::fs::read_to_string(path) {
            Ok(content) if content.trim().is_empty() => HashMap::new(),
            Ok(content) => match serde_json::from_str::<HashMap<String, TerminalMeta>>(&content) {
                Ok(data) => {
                    info!(records = data.len(), ?path, "loaded terminal metadata");
                    data
                }
                Err(parse_error) => {
                    warn!(?path, %parse_error, "failed to parse metadata file; starting empty");
                    HashMap::new()
                }
            },
            Err(read_error) => {
                warn!(?path, %read_error, "failed to read metadata file; starting empty");
                HashMap::new()
            }
        }
    }

    fn current_sender(&self) -> Result<Option<SyncSender<WriterCommand>>, String> {
        self.inner
            .sender
            .lock()
            .map_err(|_| "metadata writer sender lock poisoned".to_string())
            .map(|sender| sender.clone())
    }

    async fn send_command(&self, command: WriterCommand) -> Result<(), String> {
        if self.inner.is_in_memory {
            return Ok(());
        }
        let sender = self
            .current_sender()?
            .ok_or_else(|| "metadata writer is shut down".to_string())?;
        match sender.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(command)) => tokio::task::spawn_blocking(move || {
                sender.send(command).map_err(|_| {
                    "metadata writer stopped while applying queue backpressure".to_string()
                })
            })
            .await
            .map_err(|join_error| format!("metadata enqueue task failed: {join_error}"))?,
            Err(TrySendError::Disconnected(_)) => {
                Err("metadata writer is disconnected".to_string())
            }
        }
    }

    fn snapshot_locked(state: &StoreState) -> Snapshot {
        Snapshot {
            records: Arc::clone(&state.records),
            version: state.version,
        }
    }

    async fn current_snapshot(&self) -> Snapshot {
        let state = self.inner.state.read().await;
        Self::snapshot_locked(&state)
    }

    /// Records a registration. Returning means the in-memory update is visible and its snapshot
    /// was accepted by the writer; durability requires [`Self::flush`].
    pub async fn record_registration(&self, info: &TerminalInfo) {
        let snapshot = {
            let mut state = self.inner.state.write().await;
            let records = Arc::make_mut(&mut state.records);
            let now = chrono::Utc::now().timestamp();
            let entry = records
                .entry(info.terminal_id.clone())
                .or_insert_with(|| TerminalMeta {
                    terminal_id: info.terminal_id.clone(),
                    custom_name: None,
                    notes: None,
                    tags: Vec::new(),
                    last_known_info: Some(info.clone()),
                    created_at: now,
                    updated_at: now,
                });
            entry.last_known_info = Some(info.clone());
            entry.updated_at = now;
            state.version = state.version.wrapping_add(1);
            if !self.inner.is_in_memory {
                Some(Self::snapshot_locked(&state))
            } else {
                None
            }
        };

        if let Some(snapshot) = snapshot {
            if let Err(write_error) = self.send_command(WriterCommand::Update(snapshot)).await {
                error!(%write_error, terminal_id = %info.terminal_id, "metadata update is only in memory");
            }
        }
    }

    pub async fn get(&self, terminal_id: &str) -> Option<TerminalMeta> {
        self.inner
            .state
            .read()
            .await
            .records
            .get(terminal_id)
            .cloned()
    }

    pub async fn list_all(&self) -> Arc<HashMap<String, TerminalMeta>> {
        self.inner.state.read().await.records.clone()
    }

    /// Updates metadata. `Ok` means the new value is visible in memory and accepted by the
    /// background writer, not necessarily on disk. Call [`Self::flush`] for durable completion.
    pub async fn update_meta(
        &self,
        terminal_id: &str,
        custom_name: Option<String>,
        notes: Option<String>,
        tags: Option<Vec<String>>,
    ) -> Result<TerminalMeta, String> {
        let (result, snapshot) = {
            let mut state = self.inner.state.write().await;
            let records = Arc::make_mut(&mut state.records);
            let now = chrono::Utc::now().timestamp();
            let meta = records
                .entry(terminal_id.to_string())
                .or_insert_with(|| TerminalMeta {
                    terminal_id: terminal_id.to_string(),
                    custom_name: None,
                    notes: None,
                    tags: Vec::new(),
                    last_known_info: None,
                    created_at: now,
                    updated_at: now,
                });

            if let Some(custom_name) = custom_name {
                let trimmed = custom_name.trim().to_string();
                meta.custom_name = (!trimmed.is_empty()).then_some(trimmed);
            }
            if let Some(notes) = notes {
                let trimmed = notes.trim().to_string();
                meta.notes = (!trimmed.is_empty()).then_some(trimmed);
            }
            if let Some(tags) = tags {
                let mut clean_tags = Vec::new();
                for tag in tags {
                    let clean = tag.trim().to_string();
                    if !clean.is_empty() && !clean_tags.contains(&clean) {
                        clean_tags.push(clean);
                    }
                }
                meta.tags = clean_tags;
            }
            meta.updated_at = now;
            let result = meta.clone();
            state.version = state.version.wrapping_add(1);
            let snapshot = if !self.inner.is_in_memory {
                Some(Self::snapshot_locked(&state))
            } else {
                None
            };
            (result, snapshot)
        };

        if let Some(snapshot) = snapshot {
            self.send_command(WriterCommand::Update(snapshot)).await?;
        }
        Ok(result)
    }

    /// Removes metadata. `Ok(true)` has the same in-memory-plus-admission semantics as update.
    pub async fn remove(&self, terminal_id: &str) -> Result<bool, String> {
        let snapshot = {
            let mut state = self.inner.state.write().await;
            if !state.records.contains_key(terminal_id) {
                return Ok(false);
            }
            let records = Arc::make_mut(&mut state.records);
            if records.remove(terminal_id).is_none() {
                return Ok(false);
            }
            state.version = state.version.wrapping_add(1);
            if !self.inner.is_in_memory {
                Some(Self::snapshot_locked(&state))
            } else {
                None
            }
        };
        if let Some(snapshot) = snapshot {
            self.send_command(WriterCommand::Update(snapshot)).await?;
        }
        Ok(true)
    }

    /// Persists the latest in-memory snapshot atomically and waits for file sync completion.
    pub async fn flush(&self) -> Result<(), String> {
        if self.inner.is_in_memory {
            return Ok(());
        }
        let snapshot = self.current_snapshot().await;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send_command(WriterCommand::Flush(snapshot, reply_tx))
            .await?;
        reply_rx
            .await
            .map_err(|_| "metadata writer stopped before completing flush".to_string())?
    }

    /// Flushes the latest snapshot and terminates the writer. Call only after producers stop.
    pub async fn shutdown(&self) -> Result<(), String> {
        if self.inner.is_in_memory {
            return Ok(());
        }
        let snapshot = self.current_snapshot().await;
        let sender = self
            .inner
            .sender
            .lock()
            .map_err(|_| "metadata writer sender lock poisoned".to_string())?
            .take();
        let writer = self
            .inner
            .writer_thread
            .lock()
            .map_err(|_| "metadata writer thread lock poisoned".to_string())?
            .take();
        let result = if let Some(sender) = sender {
            let (reply_tx, reply_rx) = oneshot::channel();
            send_owned_command(sender, WriterCommand::Shutdown(snapshot, reply_tx)).await?;
            reply_rx
                .await
                .map_err(|_| "metadata writer stopped before completing shutdown".to_string())?
        } else {
            Ok(())
        };
        if let Some(writer) = writer {
            tokio::task::spawn_blocking(move || writer.join())
                .await
                .map_err(|join_error| format!("metadata join task failed: {join_error}"))?
                .map_err(|_| "metadata writer thread panicked".to_string())?;
        }
        result
    }

    #[cfg(test)]
    fn persisted_write_count(&self) -> u64 {
        self.inner.persisted_writes.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    fn fail_next_persist_attempts(&self, count: u64) {
        self.inner
            .persist_faults
            .remaining_failures
            .store(count, Ordering::Relaxed);
    }

    #[cfg(test)]
    fn persist_attempt_count(&self) -> u64 {
        self.inner.persist_faults.attempts.load(Ordering::Relaxed)
    }
}

async fn send_owned_command(
    sender: SyncSender<WriterCommand>,
    command: WriterCommand,
) -> Result<(), String> {
    match sender.try_send(command) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(command)) => tokio::task::spawn_blocking(move || {
            sender
                .send(command)
                .map_err(|_| "metadata writer stopped during shutdown".to_string())
        })
        .await
        .map_err(|join_error| format!("metadata shutdown enqueue failed: {join_error}"))?,
        Err(TrySendError::Disconnected(_)) => Err("metadata writer is disconnected".to_string()),
    }
}

fn newer_snapshot(current: Option<Snapshot>, candidate: Snapshot) -> Snapshot {
    match current {
        Some(current) if current.version > candidate.version => current,
        _ => candidate,
    }
}

fn persist_snapshot(
    path: &Path,
    snapshot: &Snapshot,
    persisted_version: &mut u64,
    persisted_writes: &AtomicU64,
    #[cfg(test)] persist_faults: &PersistFaults,
    #[cfg(test)] persist_gate: Option<&PersistGate>,
) -> Result<(), String> {
    if snapshot.version <= *persisted_version {
        return Ok(());
    }
    #[cfg(test)]
    {
        persist_faults.attempts.fetch_add(1, Ordering::Relaxed);
        if persist_faults
            .remaining_failures
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err("injected metadata persistence failure".to_string());
        }
    }
    #[cfg(test)]
    if let Some(gate) = persist_gate {
        let _ = gate.entered.send(());
        let _ = gate.release.recv();
    }

    atomic_write(path, &snapshot.records)?;
    *persisted_version = snapshot.version;
    persisted_writes.fetch_add(1, Ordering::Relaxed);
    debug!(
        records = snapshot.records.len(),
        version = snapshot.version,
        ?path,
        "persisted terminal metadata"
    );
    Ok(())
}

fn persist_snapshot_for_shutdown(
    path: &Path,
    snapshot: &Snapshot,
    persisted_version: &mut u64,
    persisted_writes: &AtomicU64,
    #[cfg(test)] persist_faults: &PersistFaults,
    #[cfg(test)] persist_gate: Option<&PersistGate>,
) -> Result<(), String> {
    let mut backoff = SHUTDOWN_PERSIST_INITIAL_BACKOFF;
    for attempt in 1..=SHUTDOWN_PERSIST_MAX_ATTEMPTS {
        match persist_snapshot(
            path,
            snapshot,
            persisted_version,
            persisted_writes,
            #[cfg(test)]
            persist_faults,
            #[cfg(test)]
            persist_gate,
        ) {
            Ok(()) => return Ok(()),
            Err(write_error) if attempt < SHUTDOWN_PERSIST_MAX_ATTEMPTS => {
                warn!(
                    %write_error,
                    attempt,
                    max_attempts = SHUTDOWN_PERSIST_MAX_ATTEMPTS,
                    backoff_ms = backoff.as_millis(),
                    "metadata shutdown persistence failed; retrying"
                );
                std::thread::sleep(backoff);
                backoff = backoff.saturating_mul(2);
            }
            Err(write_error) => return Err(write_error),
        }
    }
    unreachable!("shutdown persistence attempt range is non-empty")
}

fn atomic_write(path: &Path, records: &HashMap<String, TerminalMeta>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|write_error| {
                format!("failed to create metadata directory {parent:?}: {write_error}")
            })?;
        }
    }

    let temp_path = path.with_extension(format!("tmp.{}", std::process::id()));
    let write_result = (|| {
        let mut file = File::create(&temp_path).map_err(|write_error| {
            format!("failed to create metadata temp file {temp_path:?}: {write_error}")
        })?;
        serde_json::to_writer_pretty(&mut file, records)
            .map_err(|write_error| format!("failed to serialize metadata: {write_error}"))?;
        file.write_all(b"\n")
            .map_err(|write_error| format!("failed to finish metadata temp file: {write_error}"))?;
        file.flush()
            .map_err(|write_error| format!("failed to flush metadata temp file: {write_error}"))?;
        file.sync_all()
            .map_err(|write_error| format!("failed to sync metadata temp file: {write_error}"))?;
        std::fs::rename(&temp_path, path).map_err(|write_error| {
            format!("failed to atomically rename metadata file to {path:?}: {write_error}")
        })?;
        sync_parent_directory(path)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    write_result
}

fn sync_parent_directory(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    #[cfg(unix)]
    {
        let directory = File::open(parent).map_err(|sync_error| {
            format!("failed to open metadata parent directory {parent:?}: {sync_error}")
        })?;
        directory.sync_all().map_err(|sync_error| {
            format!("failed to sync metadata parent directory {parent:?}: {sync_error}")
        })?;
    }

    #[cfg(not(unix))]
    warn!(
        ?parent,
        "metadata parent directory sync is unsupported on this platform; rename durability is best-effort"
    );

    Ok(())
}

fn run_writer(
    path: PathBuf,
    receiver: mpsc::Receiver<WriterCommand>,
    debounce: Duration,
    persisted_writes: Arc<AtomicU64>,
    #[cfg(test)] persist_faults: Arc<PersistFaults>,
    #[cfg(test)] persist_gate: Option<PersistGate>,
) {
    let mut pending: Option<Snapshot> = None;
    let mut deadline: Option<Instant> = None;
    let mut persisted_version = 0;

    loop {
        let command = match deadline {
            Some(deadline) => {
                let timeout = deadline.saturating_duration_since(Instant::now());
                match receiver.recv_timeout(timeout) {
                    Ok(command) => Some(command),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        if let Some(snapshot) = pending.take() {
                            if let Err(write_error) = persist_snapshot(
                                &path,
                                &snapshot,
                                &mut persisted_version,
                                &persisted_writes,
                                #[cfg(test)]
                                &persist_faults,
                                #[cfg(test)]
                                persist_gate.as_ref(),
                            ) {
                                error!(%write_error, "metadata writer disconnected before pending data persisted");
                            }
                        }
                        return;
                    }
                }
            }
            None => match receiver.recv() {
                Ok(command) => Some(command),
                Err(_) => return,
            },
        };

        match command {
            Some(WriterCommand::Update(snapshot)) => {
                pending = Some(newer_snapshot(pending, snapshot));
                deadline.get_or_insert_with(|| Instant::now() + debounce);
            }
            Some(WriterCommand::Flush(snapshot, reply)) => {
                let snapshot = newer_snapshot(pending.take(), snapshot);
                let result = persist_snapshot(
                    &path,
                    &snapshot,
                    &mut persisted_version,
                    &persisted_writes,
                    #[cfg(test)]
                    &persist_faults,
                    #[cfg(test)]
                    persist_gate.as_ref(),
                );
                if result.is_err() {
                    pending = Some(snapshot);
                    deadline = Some(Instant::now() + debounce);
                } else {
                    deadline = None;
                }
                let _ = reply.send(result);
            }
            Some(WriterCommand::Shutdown(snapshot, reply)) => {
                let snapshot = newer_snapshot(pending.take(), snapshot);
                let result = persist_snapshot_for_shutdown(
                    &path,
                    &snapshot,
                    &mut persisted_version,
                    &persisted_writes,
                    #[cfg(test)]
                    &persist_faults,
                    #[cfg(test)]
                    persist_gate.as_ref(),
                );
                let _ = reply.send(result);
                return;
            }
            None => {
                let Some(snapshot) = pending.take() else {
                    deadline = None;
                    continue;
                };
                match persist_snapshot(
                    &path,
                    &snapshot,
                    &mut persisted_version,
                    &persisted_writes,
                    #[cfg(test)]
                    &persist_faults,
                    #[cfg(test)]
                    persist_gate.as_ref(),
                ) {
                    Ok(()) => deadline = None,
                    Err(write_error) => {
                        error!(%write_error, "metadata background persistence failed; retrying");
                        pending = Some(snapshot);
                        deadline = Some(Instant::now() + debounce);
                    }
                }
            }
        }

        // Drain an immediately available burst without moving the first-update deadline.
        while pending.is_some() {
            match receiver.try_recv() {
                Ok(WriterCommand::Update(snapshot)) => {
                    pending = Some(newer_snapshot(pending, snapshot));
                }
                Ok(command @ (WriterCommand::Flush(_, _) | WriterCommand::Shutdown(_, _))) => {
                    // Preserve command order by handling the barrier in the outer loop. A
                    // zero-capacity handoff is unavailable, so stop draining here; barriers sent
                    // after this burst will be received on the next iteration.
                    match command {
                        WriterCommand::Flush(snapshot, reply) => {
                            let snapshot = newer_snapshot(pending.take(), snapshot);
                            let result = persist_snapshot(
                                &path,
                                &snapshot,
                                &mut persisted_version,
                                &persisted_writes,
                                #[cfg(test)]
                                &persist_faults,
                                #[cfg(test)]
                                persist_gate.as_ref(),
                            );
                            if result.is_err() {
                                pending = Some(snapshot);
                                deadline = Some(Instant::now() + debounce);
                            } else {
                                deadline = None;
                            }
                            let _ = reply.send(result);
                        }
                        WriterCommand::Shutdown(snapshot, reply) => {
                            let snapshot = newer_snapshot(pending.take(), snapshot);
                            let result = persist_snapshot_for_shutdown(
                                &path,
                                &snapshot,
                                &mut persisted_version,
                                &persisted_writes,
                                #[cfg(test)]
                                &persist_faults,
                                #[cfg(test)]
                                persist_gate.as_ref(),
                            );
                            let _ = reply.send(result);
                            return;
                        }
                        WriterCommand::Update(_) => unreachable!(),
                    }
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::time::timeout;

    static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

    fn test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "at_pc_meta_{name}_{}_{}.json",
            std::process::id(),
            TEST_SEQ.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn terminal_info(terminal_id: &str) -> TerminalInfo {
        TerminalInfo {
            terminal_id: terminal_id.to_string(),
            hostname: terminal_id.to_string(),
            username: "tester".to_string(),
            lan_ip: "127.0.0.1".to_string(),
            os_version: "Windows 11".to_string(),
            agent_version: "test".to_string(),
        }
    }

    #[tokio::test]
    async fn in_memory_store_operations() {
        let store = TerminalMetaStore::in_memory();
        assert!(store.get("term-1").await.is_none());
        let meta = store
            .update_meta(
                "term-1",
                Some("财务-主控机".to_string()),
                Some("常驻财务室".to_string()),
                Some(vec!["财务".to_string(), "Win11".to_string()]),
            )
            .await
            .unwrap();
        assert_eq!(meta.custom_name.as_deref(), Some("财务-主控机"));
        assert!(store.remove("term-1").await.unwrap());
        assert!(store.get("term-1").await.is_none());
    }

    #[tokio::test]
    async fn flush_makes_file_backed_updates_reloadable() {
        let path = test_path("reload");
        let store = TerminalMetaStore::new(&path);
        store
            .update_meta("pc-dev-01", Some("研发编译机".to_string()), None, None)
            .await
            .unwrap();
        store.flush().await.unwrap();
        let reloaded = TerminalMetaStore::new(&path);
        assert_eq!(
            reloaded
                .get("pc-dev-01")
                .await
                .unwrap()
                .custom_name
                .as_deref(),
            Some("研发编译机")
        );
        store.shutdown().await.unwrap();
        reloaded.shutdown().await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_updates_flush_the_final_in_memory_snapshot() {
        let path = test_path("concurrent");
        let store = TerminalMetaStore::new(&path);
        let mut tasks = Vec::new();
        for index in 0..100 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                store
                    .update_meta("shared", Some(format!("name-{index}")), None, None)
                    .await
                    .unwrap();
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        let expected = store.get("shared").await.unwrap();
        store.flush().await.unwrap();
        let persisted: HashMap<String, TerminalMeta> =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(persisted.get("shared"), Some(&expected));
        store.shutdown().await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn burst_updates_are_coalesced_into_one_disk_write() {
        let path = test_path("debounce");
        let store = TerminalMetaStore::new_inner(path.clone(), Duration::from_millis(100), None);
        for index in 0..40 {
            store
                .update_meta("shared", Some(format!("name-{index}")), None, None)
                .await
                .unwrap();
        }
        tokio::time::sleep(Duration::from_millis(180)).await;
        store.flush().await.unwrap();
        assert_eq!(store.persisted_write_count(), 1);
        store.shutdown().await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn shutdown_retries_initial_failure_and_persists_latest_snapshot() {
        let path = test_path("shutdown_retry_recovers");
        let store = TerminalMetaStore::new_inner(path.clone(), Duration::from_secs(60), None);
        store.fail_next_persist_attempts(1);
        store
            .update_meta("shared", Some("stale".to_string()), None, None)
            .await
            .unwrap();
        store
            .update_meta("shared", Some("latest".to_string()), None, None)
            .await
            .unwrap();

        store.shutdown().await.unwrap();

        let persisted: HashMap<String, TerminalMeta> =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            persisted
                .get("shared")
                .and_then(|meta| meta.custom_name.as_deref()),
            Some("latest")
        );
        assert_eq!(store.persist_attempt_count(), 2);
        assert_eq!(store.persisted_write_count(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn shutdown_returns_err_after_bounded_persistence_failures() {
        let path = test_path("shutdown_retry_exhausted");
        let store = TerminalMetaStore::new_inner(path.clone(), Duration::from_secs(60), None);
        store.fail_next_persist_attempts(SHUTDOWN_PERSIST_MAX_ATTEMPTS as u64);
        store
            .update_meta("shared", Some("latest".to_string()), None, None)
            .await
            .unwrap();

        let error = store.shutdown().await.unwrap_err();

        assert_eq!(error, "injected metadata persistence failure");
        assert_eq!(
            store.persist_attempt_count(),
            SHUTDOWN_PERSIST_MAX_ATTEMPTS as u64
        );
        assert_eq!(store.persisted_write_count(), 0);
        assert!(!path.exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn disk_persistence_never_holds_the_records_lock() {
        let path = test_path("unlocked");
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let store = TerminalMetaStore::new_inner(
            path.clone(),
            Duration::from_millis(10),
            Some(PersistGate {
                entered: entered_tx,
                release: release_rx,
            }),
        );
        store.record_registration(&terminal_info("node-a")).await;
        tokio::task::spawn_blocking(move || entered_rx.recv())
            .await
            .unwrap()
            .unwrap();

        timeout(Duration::from_millis(200), async {
            assert!(store.get("node-a").await.is_some());
            store
                .update_meta("node-a", Some("online".to_string()), None, None)
                .await
                .unwrap();
        })
        .await
        .expect("in-memory reads and writes must proceed while disk persistence is blocked");

        // Release the blocked write and pre-authorize any flush/shutdown persistence passes.
        for _ in 0..4 {
            release_tx.send(()).unwrap();
        }
        store.flush().await.unwrap();
        store.shutdown().await.unwrap();
        let _ = std::fs::remove_file(path);
    }
}
