//! Immutable JSONL persistent audit logging.
//!
//! Records are serialized by callers, admitted to a bounded queue, and written by one
//! background thread through a reused file handle. Queue saturation applies backpressure;
//! accepted records are never silently discarded.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Barrier, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const DEFAULT_QUEUE_CAPACITY: usize = 1_024;
const AUDIT_TAIL_MAX_BYTES: usize = 4 * 1024 * 1024;
const IO_MAX_ATTEMPTS: usize = 5;
const IO_RETRY_BASE_DELAY: Duration = Duration::from_millis(10);
const IO_RETRY_MAX_DELAY: Duration = Duration::from_millis(160);
const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;
const DEFAULT_RETAIN_FILES: usize = 5;

type AuditResult = Result<(), String>;

struct WriterRuntime {
    factory: Arc<dyn AuditWriterFactory>,
    retry_policy: RetryPolicy,
    persist_error: Arc<Mutex<Option<String>>>,
    flush_interval: Duration,
    rotation: AuditRotation,
}

enum WriterCommand {
    Record(Vec<u8>),
    Barrier(mpsc::Sender<AuditResult>),
    Export(mpsc::Sender<Result<Vec<u8>, String>>),
    Tail {
        limit: usize,
        reply: mpsc::Sender<Vec<AuditRecord>>,
    },
    Shutdown(mpsc::Sender<AuditResult>),
}

#[derive(Debug)]
enum AdmissionState {
    Disabled,
    Running(SyncSender<WriterCommand>),
    ShuttingDown,
    Closed(AuditResult),
}

#[derive(Debug)]
struct Admission {
    state: AdmissionState,
    persist_error: Arc<Mutex<Option<String>>>,
    #[cfg(test)]
    record_probe: Option<mpsc::Sender<()>>,
    #[cfg(test)]
    shutdown_probe: Option<mpsc::Sender<()>>,
}

#[derive(Clone, Copy)]
struct RetryPolicy {
    max_attempts: usize,
    base_delay: Duration,
    max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: IO_MAX_ATTEMPTS,
            base_delay: IO_RETRY_BASE_DELAY,
            max_delay: IO_RETRY_MAX_DELAY,
        }
    }
}

trait AuditWriter: Send {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize>;
    fn flush(&mut self) -> io::Result<()>;
    fn sync_data(&mut self) -> io::Result<()>;
}

trait AuditWriterFactory: Send + Sync {
    fn open(&self, path: &Path) -> io::Result<Box<dyn AuditWriter>>;
}

struct FileAuditWriter(BufWriter<File>);

impl AuditWriter for FileAuditWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }

    fn sync_data(&mut self) -> io::Result<()> {
        self.0.flush()?;
        self.0.get_ref().sync_data()
    }
}

struct FileAuditWriterFactory;

impl AuditWriterFactory for FileAuditWriterFactory {
    fn open(&self, path: &Path) -> io::Result<Box<dyn AuditWriter>> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        open_audit_file(path)
            .map(|file| Box::new(FileAuditWriter(BufWriter::new(file))) as Box<dyn AuditWriter>)
    }
}

fn open_audit_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    restrict_audit_permissions(&file)?;
    Ok(file)
}

fn restrict_audit_permissions(_file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = _file.metadata()?.permissions();
        permissions.set_mode(0o600);
        _file.set_permissions(permissions)?;
    }
    Ok(())
}

fn archive_path(path: &Path, generation: usize) -> PathBuf {
    let mut name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("audit.jsonl"))
        .to_os_string();
    name.push(format!(".{generation}"));
    path.with_file_name(name)
}

fn ignore_not_found(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn rotate_log_files(path: &Path, retain_files: usize) -> io::Result<()> {
    let retain = retain_files.max(1);
    if retain == 1 {
        return ignore_not_found(std::fs::remove_file(path));
    }
    ignore_not_found(std::fs::remove_file(archive_path(path, retain - 1)))?;
    for generation in (1..retain - 1).rev() {
        ignore_not_found(std::fs::rename(
            archive_path(path, generation),
            archive_path(path, generation + 1),
        ))?;
    }
    ignore_not_found(std::fs::rename(path, archive_path(path, 1)))
}

/// Size-based rotation and retention for the on-disk JSONL audit trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditRotation {
    /// Rotate the active file once it exceeds this size in bytes.
    pub max_file_bytes: u64,
    /// Total files to keep, including the active log. Oldest archives are deleted.
    pub retain_files: usize,
}

impl Default for AuditRotation {
    fn default() -> Self {
        Self {
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            retain_files: DEFAULT_RETAIN_FILES,
        }
    }
}

/// A single immutable audit trail record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditRecord {
    pub id: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Thread-safe audit logger backed by one bounded background writer.
#[derive(Debug)]
pub struct AuditLogger {
    log_path: Option<PathBuf>,
    admission: Arc<Mutex<Admission>>,
    writer_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

/// Returns a prefix containing at most `max_scalars` Unicode scalar values.
fn truncate_to_scalars(value: &str, max_scalars: usize) -> (&str, bool) {
    match value.char_indices().nth(max_scalars) {
        Some((byte_index, _)) => (&value[..byte_index], true),
        None => (value, false),
    }
}

impl AuditLogger {
    /// Creates an audit logger. The writer opens the file once at start and then reuses the handle.
    pub fn new(log_path: Option<PathBuf>) -> Self {
        Self::with_rotation(log_path, AuditRotation::default())
    }

    /// Creates an audit logger with an explicit size-based rotation policy.
    pub fn with_rotation(log_path: Option<PathBuf>, rotation: AuditRotation) -> Self {
        Self::new_inner(log_path, DEFAULT_QUEUE_CAPACITY, None, rotation)
    }

    fn new_inner(
        log_path: Option<PathBuf>,
        queue_capacity: usize,
        start_gate: Option<Arc<Barrier>>,
        rotation: AuditRotation,
    ) -> Self {
        Self::new_configured(
            log_path,
            queue_capacity,
            start_gate,
            Arc::new(FileAuditWriterFactory),
            RetryPolicy::default(),
            DEFAULT_FLUSH_INTERVAL,
            rotation,
        )
    }

    fn new_configured(
        log_path: Option<PathBuf>,
        queue_capacity: usize,
        start_gate: Option<Arc<Barrier>>,
        writer_factory: Arc<dyn AuditWriterFactory>,
        retry_policy: RetryPolicy,
        flush_interval: Duration,
        rotation: AuditRotation,
    ) -> Self {
        let Some(worker_path) = log_path.clone() else {
            return Self {
                log_path,
                admission: Arc::new(Mutex::new(Admission {
                    state: AdmissionState::Disabled,
                    persist_error: Arc::new(Mutex::new(None)),
                    #[cfg(test)]
                    record_probe: None,
                    #[cfg(test)]
                    shutdown_probe: None,
                })),
                writer_thread: Arc::new(Mutex::new(None)),
            };
        };

        let (sender, receiver) = mpsc::sync_channel(queue_capacity.max(1));
        let persist_error = Arc::new(Mutex::new(None));
        let admission = Arc::new(Mutex::new(Admission {
            state: AdmissionState::Running(sender),
            persist_error: Arc::clone(&persist_error),
            #[cfg(test)]
            record_probe: None,
            #[cfg(test)]
            shutdown_probe: None,
        }));
        let writer_thread = std::thread::Builder::new()
            .name("at-pc-audit-writer".to_string())
            .spawn(move || {
                run_writer(
                    worker_path,
                    receiver,
                    start_gate,
                    WriterRuntime {
                        factory: writer_factory,
                        retry_policy,
                        persist_error,
                        flush_interval,
                        rotation,
                    },
                )
            })
            .expect("failed to start audit writer thread");

        Self {
            log_path,
            admission,
            writer_thread: Arc::new(Mutex::new(Some(writer_thread))),
        }
    }

    /// Redacts a raw token to a safe three-scalar prefix for safe auditing.
    pub fn redact_token(token: &str) -> String {
        let clean = token.trim();
        let (_, exceeds_short_token_limit) = truncate_to_scalars(clean, 4);
        if !exceeds_short_token_limit {
            "***".to_string()
        } else {
            let (prefix, _) = truncate_to_scalars(clean, 3);
            format!("{prefix}***")
        }
    }

    fn enqueue(admission: &Mutex<Admission>, command: WriterCommand) -> AuditResult {
        let admission = admission
            .lock()
            .map_err(|_| "audit admission lock poisoned".to_string())?;
        #[cfg(test)]
        let mut admission = admission;
        #[cfg(test)]
        if matches!(&command, WriterCommand::Record(_)) {
            if let Some(probe) = admission.record_probe.take() {
                let _ = probe.send(());
            }
        }
        match &admission.state {
            AdmissionState::Disabled => Ok(()),
            AdmissionState::Running(sender) => {
                if matches!(&command, WriterCommand::Record(_)) {
                    if let Ok(slot) = admission.persist_error.lock() {
                        if let Some(error) = slot.as_ref() {
                            return Err(error.clone());
                        }
                    }
                }
                sender
                    .send(command)
                    .map_err(|_| "audit writer stopped while admitting command".to_string())
            }
            AdmissionState::ShuttingDown | AdmissionState::Closed(_) => {
                Err("audit writer is shut down".to_string())
            }
        }
    }

    fn send_sync(&self, command: WriterCommand) -> AuditResult {
        Self::enqueue(&self.admission, command)
    }

    async fn send_async(&self, command: WriterCommand) -> AuditResult {
        let admission = Arc::clone(&self.admission);
        tokio::task::spawn_blocking(move || Self::enqueue(&admission, command))
            .await
            .map_err(|error| format!("audit enqueue task failed: {error}"))?
    }

    fn serialize_record(record: AuditRecord) -> Result<Vec<u8>, String> {
        let mut encoded = serde_json::to_vec(&record)
            .map_err(|error| format!("failed to serialize audit record: {error}"))?;
        encoded.push(b'\n');
        Ok(encoded)
    }

    fn try_log(&self, record: AuditRecord) -> AuditResult {
        Self::serialize_record(record)
            .and_then(|encoded| self.send_sync(WriterCommand::Record(encoded)))
    }

    async fn try_log_async(&self, record: AuditRecord) -> AuditResult {
        match Self::serialize_record(record) {
            Ok(encoded) => self.send_async(WriterCommand::Record(encoded)).await,
            Err(error) => Err(error),
        }
    }

    /// Enqueues an audit record. If the bounded queue is full, this call blocks until the
    /// single writer accepts it. Failures are emitted as errors rather than silently dropped.
    pub fn log(&self, record: AuditRecord) {
        if let Err(error) = self.try_log(record) {
            tracing::error!(%error, "audit record was not accepted");
        }
    }

    /// Async-compatible enqueue. Saturation waits on Tokio's blocking pool instead of blocking
    /// an async worker thread.
    pub async fn log_async(&self, record: AuditRecord) {
        if let Err(error) = self.try_log_async(record).await {
            tracing::error!(%error, "audit record was not accepted");
        }
    }

    /// Flushes all records admitted before this barrier and calls `sync_data` on the file.
    pub fn flush(&self) -> AuditResult {
        if self.log_path.is_none() {
            return Ok(());
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        self.send_sync(WriterCommand::Barrier(reply_tx))?;
        reply_rx
            .recv()
            .map_err(|_| "audit writer stopped before completing flush".to_string())?
    }

    /// Async-compatible flush/barrier.
    pub async fn flush_async(&self) -> AuditResult {
        if self.log_path.is_none() {
            return Ok(());
        }
        let admission = Arc::clone(&self.admission);
        tokio::task::spawn_blocking(move || {
            let (reply_tx, reply_rx) = mpsc::channel();
            Self::enqueue(&admission, WriterCommand::Barrier(reply_tx))?;
            reply_rx
                .recv()
                .map_err(|_| "audit writer stopped before completing flush".to_string())?
        })
        .await
        .map_err(|error| format!("audit flush task failed: {error}"))?
    }

    /// Flushes and terminates the writer. No new records are accepted after its linearization
    /// point at the admission-state transition to `ShuttingDown`.
    pub fn shutdown(&self) -> AuditResult {
        Self::shutdown_blocking(&self.admission, &self.writer_thread)
    }

    /// Async-compatible flush and shutdown.
    pub async fn shutdown_async(&self) -> AuditResult {
        let admission = Arc::clone(&self.admission);
        let writer_thread = Arc::clone(&self.writer_thread);
        tokio::task::spawn_blocking(move || Self::shutdown_blocking(&admission, &writer_thread))
            .await
            .map_err(|error| format!("audit shutdown task failed: {error}"))?
    }

    fn shutdown_blocking(
        admission: &Mutex<Admission>,
        writer_thread: &Mutex<Option<JoinHandle<()>>>,
    ) -> AuditResult {
        let mut admission = admission
            .lock()
            .map_err(|_| "audit admission lock poisoned".to_string())?;
        let sender = match std::mem::replace(&mut admission.state, AdmissionState::ShuttingDown) {
            AdmissionState::Disabled => {
                admission.state = AdmissionState::Disabled;
                return Ok(());
            }
            AdmissionState::Closed(result) => {
                admission.state = AdmissionState::Closed(result.clone());
                return result;
            }
            AdmissionState::ShuttingDown => {
                admission.state = AdmissionState::ShuttingDown;
                return Err("audit writer shutdown is already in progress".to_string());
            }
            AdmissionState::Running(sender) => sender,
        };

        #[cfg(test)]
        if let Some(probe) = admission.shutdown_probe.take() {
            let _ = probe.send(());
        }

        let (reply_tx, reply_rx) = mpsc::channel();
        let persistence_result = sender
            .send(WriterCommand::Shutdown(reply_tx))
            .map_err(|_| "audit writer stopped before shutdown was queued".to_string())
            .and_then(|()| {
                reply_rx
                    .recv()
                    .map_err(|_| "audit writer stopped before completing shutdown".to_string())?
            });

        let join_result = writer_thread
            .lock()
            .map_err(|_| "audit writer thread lock poisoned".to_string())
            .and_then(|mut writer| {
                writer
                    .take()
                    .map(|writer| {
                        writer
                            .join()
                            .map_err(|_| "audit writer thread panicked".to_string())
                    })
                    .unwrap_or(Ok(()))
            });
        let result = match (persistence_result, join_result) {
            (Err(persistence_error), Err(join_error)) => {
                Err(format!("{persistence_error}; {join_error}"))
            }
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        };
        admission.state = AdmissionState::Closed(result.clone());
        result
    }

    /// Reads recent records after a persistence barrier using a fixed-size tail window.
    pub fn read_recent(&self, limit: usize) -> Vec<AuditRecord> {
        Self::tail_blocking(&self.admission, self.log_path.as_deref(), limit)
    }

    /// Async-compatible barrier plus fixed-size tail read.
    pub async fn read_recent_async(&self, limit: usize) -> Vec<AuditRecord> {
        let admission = Arc::clone(&self.admission);
        let path = self.log_path.clone();
        tokio::task::spawn_blocking(move || Self::tail_blocking(&admission, path.as_deref(), limit))
            .await
            .unwrap_or_else(|error| {
                tracing::error!(%error, "audit tail read task failed");
                Vec::new()
            })
    }

    fn tail_blocking(
        admission: &Mutex<Admission>,
        log_path: Option<&Path>,
        limit: usize,
    ) -> Vec<AuditRecord> {
        let Some(path) = log_path else {
            return Vec::new();
        };
        let (reply_tx, reply_rx) = mpsc::channel();
        match Self::enqueue(
            admission,
            WriterCommand::Tail {
                limit,
                reply: reply_tx,
            },
        ) {
            Ok(()) => reply_rx.recv().unwrap_or_else(|_| {
                tracing::error!("audit writer stopped before completing tail read");
                read_recent_path(path, limit)
            }),
            Err(error) => {
                tracing::error!(%error, "failed to flush audit log before tail read");
                read_recent_path(path, limit)
            }
        }
    }

    /// Returns the concatenated JSONL contents of the active log and retained rotated files.
    pub fn export_jsonl(&self) -> Result<Vec<u8>, String> {
        Self::export_blocking(&self.admission, self.log_path.as_deref())
    }

    /// Async-compatible flush plus full JSONL export of retained files.
    pub async fn export_jsonl_async(&self) -> Result<Vec<u8>, String> {
        let admission = Arc::clone(&self.admission);
        let path = self.log_path.clone();
        tokio::task::spawn_blocking(move || Self::export_blocking(&admission, path.as_deref()))
            .await
            .map_err(|error| format!("audit export task failed: {error}"))?
    }

    fn export_blocking(
        admission: &Mutex<Admission>,
        log_path: Option<&Path>,
    ) -> Result<Vec<u8>, String> {
        if log_path.is_none() {
            return Ok(Vec::new());
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        Self::enqueue(admission, WriterCommand::Export(reply_tx))?;
        reply_rx
            .recv()
            .map_err(|_| "audit writer stopped before completing export".to_string())?
    }

    /// Returns the target log file path if set.
    pub fn log_path(&self) -> Option<&Path> {
        self.log_path.as_deref()
    }
}

impl Drop for AuditLogger {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            tracing::error!(%error, "failed to shut down audit writer during drop");
        }
    }
}

fn is_transient(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

fn retry_delay(policy: RetryPolicy, failed_attempt: usize) {
    if policy.base_delay.is_zero() {
        return;
    }
    let shift = failed_attempt.saturating_sub(1).min(16) as u32;
    let factor = 1_u32 << shift;
    let delay = policy
        .base_delay
        .checked_mul(factor)
        .unwrap_or(policy.max_delay)
        .min(policy.max_delay);
    std::thread::sleep(delay);
}

fn retry_io<T, F>(operation: &str, policy: RetryPolicy, mut attempt: F) -> Result<T, String>
where
    F: FnMut() -> io::Result<T>,
{
    let max_attempts = policy.max_attempts.max(1);
    for attempt_number in 1..=max_attempts {
        match attempt() {
            Ok(value) => return Ok(value),
            Err(error) if is_transient(&error) && attempt_number < max_attempts => {
                retry_delay(policy, attempt_number);
            }
            Err(error) => {
                let disposition = if is_transient(&error) {
                    "retry budget exhausted"
                } else {
                    "permanent failure"
                };
                return Err(format!(
                    "{operation} failed on attempt {attempt_number}/{max_attempts} ({disposition}): {error}"
                ));
            }
        }
    }
    unreachable!("retry loop always returns")
}

fn write_record(
    writer: &mut dyn AuditWriter,
    encoded: &[u8],
    retry_policy: RetryPolicy,
) -> AuditResult {
    let mut offset = 0;
    while offset < encoded.len() {
        let written = retry_io("failed to append audit record", retry_policy, || {
            writer.write(&encoded[offset..])
        })?;
        if written == 0 {
            return Err("failed to append audit record: writer returned zero bytes".to_string());
        }
        offset += written;
    }
    Ok(())
}

fn flush_writer(
    writer: &mut Option<Box<dyn AuditWriter>>,
    terminal_error: &Option<String>,
    retry_policy: RetryPolicy,
) -> AuditResult {
    let flush_result = if let Some(writer) = writer.as_mut() {
        retry_io("failed to flush audit log", retry_policy, || writer.flush()).and_then(|()| {
            retry_io("failed to sync audit log", retry_policy, || {
                writer.sync_data()
            })
        })
    } else {
        Ok(())
    };
    if let Some(error) = terminal_error {
        return Err(error.clone());
    }
    flush_result
}

fn poison_admission(persist_error: &Mutex<Option<String>>, error: &str) {
    if let Ok(mut slot) = persist_error.lock() {
        if slot.is_none() {
            *slot = Some(error.to_string());
        }
    }
}

fn set_terminal_error(
    terminal_error: &mut Option<String>,
    persist_error: &Mutex<Option<String>>,
    error: &str,
) {
    if terminal_error.is_none() {
        *terminal_error = Some(error.to_string());
    }
    poison_admission(
        persist_error,
        terminal_error.as_deref().expect("terminal error is set"),
    );
}

fn periodic_flush_writer(writer: &mut Option<Box<dyn AuditWriter>>) {
    if let Some(writer) = writer.as_mut() {
        if let Err(error) = writer.flush() {
            tracing::error!(%error, "audit writer failed a periodic flush");
        }
    }
}

fn persist_record(
    path: &Path,
    writer_factory: &dyn AuditWriterFactory,
    retry_policy: RetryPolicy,
    writer: &mut Option<Box<dyn AuditWriter>>,
    encoded: &[u8],
    rotation: AuditRotation,
    current_bytes: &mut u64,
) -> AuditResult {
    ensure_writer(path, writer_factory, retry_policy, writer, current_bytes)?;
    let record_len = encoded.len() as u64;
    if *current_bytes > 0
        && rotation.max_file_bytes > 0
        && current_bytes.saturating_add(record_len) > rotation.max_file_bytes
    {
        rotate_active_writer(path, retry_policy, writer, rotation, current_bytes)?;
        ensure_writer(path, writer_factory, retry_policy, writer, current_bytes)?;
    }
    write_record(
        writer.as_mut().expect("writer initialized").as_mut(),
        encoded,
        retry_policy,
    )?;
    *current_bytes = current_bytes.saturating_add(record_len);
    Ok(())
}

fn ensure_writer(
    path: &Path,
    writer_factory: &dyn AuditWriterFactory,
    retry_policy: RetryPolicy,
    writer: &mut Option<Box<dyn AuditWriter>>,
    current_bytes: &mut u64,
) -> AuditResult {
    if writer.is_none() {
        *writer = Some(retry_io(
            &format!("failed to open audit log {path:?}"),
            retry_policy,
            || writer_factory.open(path),
        )?);
        *current_bytes = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    }
    Ok(())
}

fn rotate_active_writer(
    path: &Path,
    retry_policy: RetryPolicy,
    writer: &mut Option<Box<dyn AuditWriter>>,
    rotation: AuditRotation,
    current_bytes: &mut u64,
) -> AuditResult {
    let flush_result = flush_writer(writer, &None, retry_policy);
    *writer = None;
    *current_bytes = 0;
    flush_result?;
    retry_io("failed to rotate audit log files", retry_policy, || {
        rotate_log_files(path, rotation.retain_files)
    })
}

fn run_writer(
    path: PathBuf,
    receiver: mpsc::Receiver<WriterCommand>,
    start_gate: Option<Arc<Barrier>>,
    runtime: WriterRuntime,
) {
    // Poison the shared persist-error slot directly. Do not lock `Admission` from this
    // thread: enqueue holds that mutex across a blocking send, and shutdown holds it
    // while waiting for the writer.
    if let Some(gate) = start_gate {
        gate.wait();
    }

    let WriterRuntime {
        factory: writer_factory,
        retry_policy,
        persist_error,
        flush_interval,
        rotation,
    } = runtime;

    let mut terminal_error = None;
    let mut writer = match retry_io(
        &format!("failed to open audit log {path:?}"),
        retry_policy,
        || writer_factory.open(&path),
    ) {
        Ok(opened) => Some(opened),
        Err(error) => {
            tracing::error!(%error, "audit writer failed to open log file at start");
            set_terminal_error(&mut terminal_error, &persist_error, &error);
            None
        }
    };
    let mut current_bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);

    let mut last_flush = Instant::now();
    loop {
        let wait = flush_interval.saturating_sub(last_flush.elapsed());
        let command = match receiver.recv_timeout(wait) {
            Ok(command) => command,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                periodic_flush_writer(&mut writer);
                last_flush = Instant::now();
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        let mut commands = vec![command];
        while let Ok(more) = receiver.try_recv() {
            commands.push(more);
        }

        let mut shutdown_reply = None;
        for command in commands {
            match command {
                WriterCommand::Record(encoded) => {
                    if let Err(error) = persist_record(
                        &path,
                        writer_factory.as_ref(),
                        retry_policy,
                        &mut writer,
                        &encoded,
                        rotation,
                        &mut current_bytes,
                    ) {
                        tracing::error!(
                            %error,
                            "audit writer reached a terminal persistence failure"
                        );
                        set_terminal_error(&mut terminal_error, &persist_error, &error);
                        writer = None;
                    }
                }
                WriterCommand::Barrier(reply) => {
                    let result = flush_writer(&mut writer, &terminal_error, retry_policy);
                    if let Err(error) = &result {
                        set_terminal_error(&mut terminal_error, &persist_error, error);
                    }
                    let _ = reply.send(result);
                }
                WriterCommand::Export(reply) => {
                    let result = match flush_writer(&mut writer, &terminal_error, retry_policy) {
                        Err(error) => {
                            set_terminal_error(&mut terminal_error, &persist_error, &error);
                            Err(error)
                        }
                        Ok(()) => export_jsonl_path(&path),
                    };
                    let _ = reply.send(result);
                }
                WriterCommand::Tail { limit, reply } => {
                    if let Err(error) = flush_writer(&mut writer, &terminal_error, retry_policy) {
                        set_terminal_error(&mut terminal_error, &persist_error, &error);
                        tracing::error!(%error, "failed to flush audit log before tail read");
                    }
                    let _ = reply.send(read_recent_path(&path, limit));
                }
                WriterCommand::Shutdown(reply) => {
                    shutdown_reply = Some(reply);
                    break;
                }
            }
        }

        if let Some(reply) = shutdown_reply {
            let result = flush_writer(&mut writer, &terminal_error, retry_policy);
            if let Err(error) = &result {
                set_terminal_error(&mut terminal_error, &persist_error, error);
            }
            let _ = reply.send(result);
            return;
        }

        if last_flush.elapsed() >= flush_interval {
            periodic_flush_writer(&mut writer);
            last_flush = Instant::now();
        }
    }

    if let Err(error) = flush_writer(&mut writer, &terminal_error, retry_policy) {
        tracing::error!(%error, "audit writer channel closed before a clean shutdown");
    }
}

fn parse_jsonl_records(lines: Vec<String>) -> Vec<AuditRecord> {
    lines
        .into_iter()
        .filter_map(|line| serde_json::from_str::<AuditRecord>(&line).ok())
        .collect()
}

fn open_existing_snapshot_file(path: &Path) -> Result<Option<File>, String> {
    match File::open(path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "failed to open audit snapshot {}: {error}",
            path.display()
        )),
    }
}

fn same_open_file(left: &File, right: &File) -> bool {
    let (Ok(f1), Ok(f2)) = (left.try_clone(), right.try_clone()) else {
        return false;
    };
    match (
        same_file::Handle::from_file(f1),
        same_file::Handle::from_file(f2),
    ) {
        (Ok(h1), Ok(h2)) => h1 == h2,
        _ => false,
    }
}

struct SnapshotFiles {
    current: Option<File>,
    /// Newest archive first: `.1`, `.2`, …
    archives: Vec<File>,
}

fn open_snapshot_files(path: &Path) -> Result<SnapshotFiles, String> {
    let current = open_existing_snapshot_file(path)?;
    #[cfg(test)]
    {
        fire_toctou_after_archive_enum(path);
        fire_toctou_after_current_tail(path);
    }
    let mut archives = Vec::new();
    for generation in 1..=1024 {
        match open_existing_snapshot_file(&archive_path(path, generation))? {
            None => break,
            Some(file) => {
                let duplicate = current
                    .as_ref()
                    .is_some_and(|current| same_open_file(current, &file))
                    || archives.iter().any(|opened| same_open_file(opened, &file));
                if !duplicate {
                    archives.push(file);
                }
            }
        }
    }
    Ok(SnapshotFiles { current, archives })
}

fn snapshot_read_error(error: &io::Error, what: &str) -> String {
    if error.kind() == io::ErrorKind::NotFound {
        format!("audit snapshot incomplete: {what} disappeared during read")
    } else {
        format!("failed to read audit {what}: {error}")
    }
}

fn read_recent_path(path: &Path, limit: usize) -> Vec<AuditRecord> {
    if limit == 0 {
        return Vec::new();
    }
    let snapshot = match open_snapshot_files(path) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            tracing::error!(%error, "failed to open audit snapshot for tail read");
            return Vec::new();
        }
    };
    let mut files = Vec::new();
    if let Some(current) = snapshot.current {
        files.push(current);
    }
    files.extend(snapshot.archives);
    let mut remaining = limit;
    let mut newest_first = Vec::new();
    for mut file in files {
        if remaining == 0 {
            break;
        }
        let file_len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        let records =
            crate::tail::read_last_n_lines_from_file(&mut file, remaining, AUDIT_TAIL_MAX_BYTES)
                .map(parse_jsonl_records)
                .unwrap_or_default();
        let cap_bound = file_len > AUDIT_TAIL_MAX_BYTES as u64 && records.len() < remaining;
        remaining = remaining.saturating_sub(records.len());
        newest_first.push(records);
        if cap_bound {
            break;
        }
    }
    let mut oldest_first = Vec::new();
    for chunk in newest_first.into_iter().rev() {
        oldest_first.extend(chunk);
    }
    oldest_first
}

fn append_exported_jsonl(out: &mut Vec<u8>, file: &mut File, what: &str) -> Result<(), String> {
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| snapshot_read_error(&error, what))?;
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    out.extend(bytes);
    Ok(())
}

fn export_jsonl_path(path: &Path) -> Result<Vec<u8>, String> {
    let snapshot = open_snapshot_files(path)?;
    let mut out = Vec::new();
    for mut file in snapshot.archives.into_iter().rev() {
        append_exported_jsonl(&mut out, &mut file, "rotated file")?;
    }
    if let Some(mut current) = snapshot.current {
        append_exported_jsonl(&mut out, &mut current, "current file")?;
    }
    Ok(out)
}

#[cfg(test)]
type TocTouHook = Box<dyn FnOnce(&Path)>;

#[cfg(test)]
thread_local! {
    static TOCTOU_AFTER_ARCHIVE_ENUM: std::cell::RefCell<Option<TocTouHook>> =
        std::cell::RefCell::new(None);
    static TOCTOU_AFTER_CURRENT_TAIL: std::cell::RefCell<Option<TocTouHook>> =
        std::cell::RefCell::new(None);
}

#[cfg(test)]
fn fire_toctou_after_archive_enum(path: &Path) {
    TOCTOU_AFTER_ARCHIVE_ENUM.with(|slot| {
        if let Some(hook) = slot.borrow_mut().take() {
            hook(path);
        }
    });
}

#[cfg(test)]
fn fire_toctou_after_current_tail(path: &Path) {
    TOCTOU_AFTER_CURRENT_TAIL.with(|slot| {
        if let Some(hook) = slot.borrow_mut().take() {
            hook(path);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::time::Duration;

    static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

    fn test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "at_pc_audit_{name}_{}_{}.jsonl",
            std::process::id(),
            TEST_SEQ.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn record(id: impl Into<String>) -> AuditRecord {
        AuditRecord {
            id: id.into(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            role: None,
            token_prefix: None,
            client_ip: None,
            terminal_id: None,
            action: "test".to_string(),
            tool_name: None,
            arguments: None,
            status: "SUCCESS".to_string(),
            error: None,
            duration_ms: None,
        }
    }

    fn assert_redacted(token: &str, expected: &str) {
        let redacted = AuditLogger::redact_token(token);
        let clean = token.trim();
        assert_eq!(redacted, expected);
        assert!(!redacted.contains('\u{FFFD}'));
        assert_ne!(redacted, clean, "redaction must not return the full token");
        assert!(
            !redacted.contains(clean),
            "redaction must not contain the full token"
        );
    }

    #[derive(Default)]
    struct FaultState {
        open_failures: AtomicUsize,
        write_failures: AtomicUsize,
        disk_full_writes: AtomicUsize,
        open_attempts: AtomicUsize,
        write_attempts: AtomicUsize,
    }

    struct FaultFactory {
        state: Arc<FaultState>,
    }

    struct FaultWriter {
        inner: Box<dyn AuditWriter>,
        state: Arc<FaultState>,
    }

    fn consume_failure(remaining: &AtomicUsize) -> bool {
        remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                value.checked_sub(1)
            })
            .is_ok()
    }

    impl AuditWriterFactory for FaultFactory {
        fn open(&self, path: &Path) -> io::Result<Box<dyn AuditWriter>> {
            self.state.open_attempts.fetch_add(1, Ordering::SeqCst);
            if consume_failure(&self.state.open_failures) {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "injected transient open failure",
                ));
            }
            let inner = FileAuditWriterFactory.open(path)?;
            Ok(Box::new(FaultWriter {
                inner,
                state: Arc::clone(&self.state),
            }))
        }
    }

    impl AuditWriter for FaultWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.state.write_attempts.fetch_add(1, Ordering::SeqCst);
            if consume_failure(&self.state.disk_full_writes) {
                return Err(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "injected disk full",
                ));
            }
            if consume_failure(&self.state.write_failures) {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "injected transient write failure",
                ));
            }
            self.inner.write(bytes)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }

        fn sync_data(&mut self) -> io::Result<()> {
            self.inner.sync_data()
        }
    }

    fn fault_logger(path: PathBuf, state: Arc<FaultState>, max_attempts: usize) -> AuditLogger {
        AuditLogger::new_configured(
            Some(path),
            DEFAULT_QUEUE_CAPACITY,
            None,
            Arc::new(FaultFactory { state }),
            RetryPolicy {
                max_attempts,
                base_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            },
            DEFAULT_FLUSH_INTERVAL,
            AuditRotation::default(),
        )
    }

    #[derive(Default)]
    struct FlushSpyCounts {
        flush: AtomicUsize,
        sync_data: AtomicUsize,
    }

    struct FlushSpy {
        pending: Vec<u8>,
        durable: Arc<Mutex<Vec<u8>>>,
        counts: Arc<FlushSpyCounts>,
    }

    struct FlushSpyFactory {
        durable: Arc<Mutex<Vec<u8>>>,
        counts: Arc<FlushSpyCounts>,
    }

    impl FlushSpy {
        fn persist_pending(&mut self) {
            self.durable
                .lock()
                .expect("flush spy durable lock")
                .extend_from_slice(&self.pending);
            self.pending.clear();
        }
    }

    impl AuditWriterFactory for FlushSpyFactory {
        fn open(&self, _path: &Path) -> io::Result<Box<dyn AuditWriter>> {
            Ok(Box::new(FlushSpy {
                pending: Vec::new(),
                durable: Arc::clone(&self.durable),
                counts: Arc::clone(&self.counts),
            }))
        }
    }

    impl AuditWriter for FlushSpy {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.pending.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.counts.flush.fetch_add(1, Ordering::SeqCst);
            self.persist_pending();
            Ok(())
        }

        fn sync_data(&mut self) -> io::Result<()> {
            self.counts.sync_data.fetch_add(1, Ordering::SeqCst);
            self.persist_pending();
            Ok(())
        }
    }

    #[test]
    fn redact_token_uses_unicode_scalar_boundaries_without_leaking_tokens() {
        let cases = [
            ("abcd", "***"),
            ("abcde", "abc***"),
            ("中文中文", "***"),
            ("中文中文中", "中文中***"),
            ("éééé", "***"),
            ("ééééé", "ééé***"),
            ("😀😀😀😀", "***"),
            ("😀😀😀😀😀", "😀😀😀***"),
            ("e\u{301}e\u{301}", "***"),
            ("e\u{301}e\u{301}e", "e\u{301}e***"),
            ("  abcde  ", "abc***"),
        ];
        for (token, expected) in cases {
            assert_redacted(token, expected);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_records_are_all_visible_after_flush() {
        let path = test_path("concurrent");
        let logger = Arc::new(AuditLogger::new(Some(path.clone())));
        let mut tasks = Vec::new();
        for worker in 0..8 {
            let logger = Arc::clone(&logger);
            tasks.push(tokio::spawn(async move {
                for item in 0..100 {
                    logger.log_async(record(format!("{worker}-{item}"))).await;
                }
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        logger.flush_async().await.unwrap();
        let records = logger.read_recent_async(1_000).await;
        assert_eq!(records.len(), 800);
        logger.shutdown_async().await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn full_queue_applies_backpressure_without_dropping_records() {
        let path = test_path("backpressure");
        let gate = Arc::new(Barrier::new(2));
        let logger = Arc::new(AuditLogger::new_inner(
            Some(path.clone()),
            1,
            Some(Arc::clone(&gate)),
            AuditRotation::default(),
        ));
        logger.try_log(record("first")).unwrap();
        let (probe_tx, probe_rx) = mpsc::channel();
        logger.admission.lock().unwrap().record_probe = Some(probe_tx);
        let blocked_logger = Arc::clone(&logger);
        let blocked =
            tokio::spawn(async move { blocked_logger.try_log_async(record("second")).await });
        tokio::task::spawn_blocking(move || probe_rx.recv_timeout(Duration::from_secs(1)))
            .await
            .unwrap()
            .unwrap();
        assert!(
            !blocked.is_finished(),
            "second producer must wait while queue is full"
        );
        let (progress_tx, progress_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let _ = progress_tx.send(());
        });
        tokio::time::timeout(Duration::from_millis(100), progress_rx)
            .await
            .expect("async backpressure must not block a Tokio worker")
            .expect("progress task must complete on the single worker");
        gate.wait();
        blocked.await.unwrap().unwrap();
        logger.flush_async().await.unwrap();
        let records = logger.read_recent_async(10).await;
        assert_eq!(
            records
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        logger.shutdown_async().await.unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn admission_and_shutdown_are_linearized_in_both_orders() {
        let admitted_path = test_path("admission_wins");
        let gate = Arc::new(Barrier::new(2));
        let logger = Arc::new(AuditLogger::new_inner(
            Some(admitted_path.clone()),
            1,
            Some(Arc::clone(&gate)),
            AuditRotation::default(),
        ));
        logger.try_log(record("first")).unwrap();
        let (record_tx, record_rx) = mpsc::channel();
        logger.admission.lock().unwrap().record_probe = Some(record_tx);
        let producer_logger = Arc::clone(&logger);
        let producer =
            tokio::spawn(async move { producer_logger.try_log_async(record("admitted")).await });
        record_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let shutdown_logger = Arc::clone(&logger);
        let shutdown = tokio::spawn(async move { shutdown_logger.shutdown_async().await });
        gate.wait();
        producer.await.unwrap().unwrap();
        shutdown.await.unwrap().unwrap();
        assert_eq!(
            read_recent_path(&admitted_path, 10)
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "admitted"]
        );

        let shutdown_path = test_path("shutdown_wins");
        let gate = Arc::new(Barrier::new(2));
        let logger = Arc::new(AuditLogger::new_inner(
            Some(shutdown_path.clone()),
            1,
            Some(Arc::clone(&gate)),
            AuditRotation::default(),
        ));
        logger.try_log(record("before-shutdown")).unwrap();
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        logger.admission.lock().unwrap().shutdown_probe = Some(shutdown_tx);
        let shutdown_logger = Arc::clone(&logger);
        let shutdown = tokio::spawn(async move { shutdown_logger.shutdown_async().await });
        shutdown_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let producer_logger = Arc::clone(&logger);
        let rejected =
            tokio::spawn(async move { producer_logger.try_log_async(record("rejected")).await });
        gate.wait();
        shutdown.await.unwrap().unwrap();
        assert!(rejected.await.unwrap().is_err());
        assert_eq!(
            read_recent_path(&shutdown_path, 10)
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["before-shutdown"]
        );
        let _ = std::fs::remove_file(admitted_path);
        let _ = std::fs::remove_file(shutdown_path);
    }

    #[test]
    fn one_transient_open_failure_recovers_without_reordering() {
        let path = test_path("retry_open");
        let state = Arc::new(FaultState::default());
        state.open_failures.store(1, Ordering::SeqCst);
        let logger = fault_logger(path.clone(), Arc::clone(&state), 3);
        logger.try_log(record("first")).unwrap();
        logger.try_log(record("second")).unwrap();
        logger.flush().unwrap();
        assert_eq!(state.open_attempts.load(Ordering::SeqCst), 2);
        assert_eq!(
            read_recent_path(&path, 10)
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        logger.shutdown().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn multiple_transient_write_failures_recover_without_reordering() {
        let path = test_path("retry_write");
        let state = Arc::new(FaultState::default());
        state.write_failures.store(3, Ordering::SeqCst);
        let logger = fault_logger(path.clone(), Arc::clone(&state), 5);
        logger.try_log(record("first")).unwrap();
        logger.try_log(record("second")).unwrap();
        logger.flush().unwrap();
        assert!(state.write_attempts.load(Ordering::SeqCst) >= 5);
        assert_eq!(
            read_recent_path(&path, 10)
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        logger.shutdown().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn retry_exhaustion_is_sticky_for_barrier_and_shutdown() {
        let path = test_path("retry_exhausted");
        let state = Arc::new(FaultState::default());
        state.write_failures.store(3, Ordering::SeqCst);
        let logger = fault_logger(path.clone(), state, 3);
        logger.try_log(record("never-durable")).unwrap();
        let flush_error = logger.flush().unwrap_err();
        assert!(flush_error.contains("retry budget exhausted"));
        let shutdown_error = logger.shutdown().unwrap_err();
        assert_eq!(shutdown_error, flush_error);
        assert_eq!(logger.shutdown().unwrap_err(), flush_error);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn try_log_fails_after_sticky_persist_error() {
        let path = test_path("sticky_try_log");
        let state = Arc::new(FaultState::default());
        state.write_failures.store(3, Ordering::SeqCst);
        let logger = fault_logger(path.clone(), state, 3);
        logger.try_log(record("never-durable")).unwrap();
        let flush_error = logger.flush().unwrap_err();
        assert!(flush_error.contains("retry budget exhausted"));
        let log_error = logger.try_log(record("after-failure")).unwrap_err();
        assert_eq!(log_error, flush_error);
        assert_eq!(logger.shutdown().unwrap_err(), flush_error);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn shutdown_is_a_durable_barrier_and_rejects_later_records() {
        let path = test_path("shutdown");
        let logger = AuditLogger::new(Some(path.clone()));
        logger
            .try_log_async(record("before-shutdown"))
            .await
            .unwrap();
        logger.shutdown_async().await.unwrap();
        assert!(logger
            .try_log_async(record("after-shutdown"))
            .await
            .is_err());
        let records = read_recent_path(&path, 10);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "before-shutdown");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn queued_records_are_not_dropped_after_write_failure() {
        let path = test_path("queued_not_dropped");
        let state = Arc::new(FaultState::default());
        state.write_failures.store(3, Ordering::SeqCst);
        let gate = Arc::new(Barrier::new(2));
        let logger = AuditLogger::new_configured(
            Some(path.clone()),
            DEFAULT_QUEUE_CAPACITY,
            Some(Arc::clone(&gate)),
            Arc::new(FaultFactory {
                state: Arc::clone(&state),
            }),
            RetryPolicy {
                max_attempts: 3,
                base_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            },
            DEFAULT_FLUSH_INTERVAL,
            AuditRotation::default(),
        );

        logger.try_log(record("A")).unwrap();
        logger.try_log(record("B")).unwrap();
        gate.wait();

        let flush_error = logger.flush().unwrap_err();
        assert!(flush_error.contains("retry budget exhausted"));

        let ids: Vec<String> = read_recent_path(&path, 10)
            .into_iter()
            .map(|item| item.id)
            .collect();
        assert!(
            ids.iter().any(|id| id == "B"),
            "accepted record B must not be silently discarded after A fails; persisted ids={ids:?}"
        );

        let shutdown_error = logger.shutdown().unwrap_err();
        assert_eq!(shutdown_error, flush_error);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn writer_flushes_buffered_records_without_an_explicit_barrier() {
        let path = test_path("periodic_flush");
        let durable = Arc::new(Mutex::new(Vec::new()));
        let counts = Arc::new(FlushSpyCounts::default());
        let logger = AuditLogger::new_configured(
            Some(path.clone()),
            DEFAULT_QUEUE_CAPACITY,
            None,
            Arc::new(FlushSpyFactory {
                durable: Arc::clone(&durable),
                counts: Arc::clone(&counts),
            }),
            RetryPolicy::default(),
            Duration::from_millis(20),
            AuditRotation::default(),
        );

        logger.try_log(record("periodic")).unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            let flushed = durable.lock().expect("flush spy durable lock").clone();
            if !flushed.is_empty() {
                let text = String::from_utf8_lossy(&flushed);
                assert!(
                    text.contains("\"id\":\"periodic\""),
                    "periodic flush must persist the admitted record, got {text:?}"
                );
                assert!(
                    counts.flush.load(Ordering::SeqCst) > 0,
                    "periodic wait must call flush"
                );
                assert_eq!(
                    counts.sync_data.load(Ordering::SeqCst),
                    0,
                    "periodic wait must not fsync"
                );
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "writer must flush buffered records without an explicit barrier"
            );
            std::thread::sleep(Duration::from_millis(5));
        }

        logger.shutdown().unwrap();
        assert!(
            counts.sync_data.load(Ordering::SeqCst) > 0,
            "shutdown barrier must fsync"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn writer_flushes_on_deadline_during_sustained_writes() {
        let path = test_path("deadline_flush");
        let durable = Arc::new(Mutex::new(Vec::new()));
        let counts = Arc::new(FlushSpyCounts::default());
        let logger = Arc::new(AuditLogger::new_configured(
            Some(path.clone()),
            DEFAULT_QUEUE_CAPACITY,
            None,
            Arc::new(FlushSpyFactory {
                durable: Arc::clone(&durable),
                counts: Arc::clone(&counts),
            }),
            RetryPolicy::default(),
            Duration::from_millis(20),
            AuditRotation::default(),
        ));

        let stop = Arc::new(AtomicBool::new(false));
        let producer_logger = Arc::clone(&logger);
        let producer_stop = Arc::clone(&stop);
        let producer = std::thread::spawn(move || {
            let mut n = 0u64;
            while !producer_stop.load(Ordering::Relaxed) {
                producer_logger
                    .try_log(record(format!("busy-{n}")))
                    .unwrap();
                n += 1;
            }
        });

        let deadline = std::time::Instant::now() + Duration::from_millis(300);
        loop {
            if counts.flush.load(Ordering::SeqCst) > 0 {
                assert_eq!(
                    counts.sync_data.load(Ordering::SeqCst),
                    0,
                    "deadline flush during sustained writes must not fsync"
                );
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "writer must flush on a clock during sustained writes, not only after idle"
            );
            std::thread::sleep(Duration::from_millis(5));
        }

        stop.store(true, Ordering::Relaxed);
        producer.join().unwrap();

        let syncs_before_barrier = counts.sync_data.load(Ordering::SeqCst);
        logger.flush().unwrap();
        assert!(
            counts.sync_data.load(Ordering::SeqCst) > syncs_before_barrier,
            "explicit barrier must fsync"
        );

        logger.shutdown().unwrap();
        let _ = std::fs::remove_file(path);
    }

    fn archive_path_for(path: &Path, generation: usize) -> PathBuf {
        let mut name = path
            .file_name()
            .expect("audit path has a file name")
            .to_os_string();
        name.push(format!(".{generation}"));
        path.with_file_name(name)
    }

    fn cleanup_rotated(path: &Path) {
        let _ = std::fs::remove_file(path);
        for generation in 1..16 {
            let _ = std::fs::remove_file(archive_path_for(path, generation));
        }
    }

    fn parse_jsonl_file(path: &Path) -> Vec<AuditRecord> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<AuditRecord>(line).ok())
            .collect()
    }

    fn write_padded_record(file: &mut std::fs::File, id: &str, pad_bytes: usize) {
        use std::io::Write;
        let mut item = record(id);
        item.arguments = Some(serde_json::json!({ "pad": "x".repeat(pad_bytes) }));
        let mut encoded = serde_json::to_vec(&item).unwrap();
        encoded.push(b'\n');
        file.write_all(&encoded).unwrap();
    }

    #[test]
    fn rotates_when_size_exceeded_and_deletes_oldest_beyond_retain() {
        let path = test_path("rotate");
        let logger = AuditLogger::with_rotation(
            Some(path.clone()),
            AuditRotation {
                max_file_bytes: 400,
                retain_files: 3,
            },
        );

        let mut written = 0usize;
        let second_archive = archive_path_for(&path, 2);
        while !second_archive.exists() {
            logger
                .try_log(record(format!("rotate-{written:04}")))
                .unwrap();
            written += 1;
            if written.is_multiple_of(4) {
                logger.flush().unwrap();
            }
            assert!(
                written < 200,
                "writer must rotate within a bounded number of small records"
            );
        }
        for extra in 0..24 {
            logger.try_log(record(format!("extra-{extra:04}"))).unwrap();
        }
        logger.flush().unwrap();

        assert!(path.exists(), "active log must exist after rotation");
        assert!(
            archive_path_for(&path, 1).exists(),
            "most recent rotated file must be retained"
        );
        assert!(
            archive_path_for(&path, 2).exists(),
            "second rotated file must be retained"
        );
        assert!(
            !archive_path_for(&path, 3).exists(),
            "oldest file beyond retain N must be deleted"
        );
        assert!(
            std::fs::metadata(&path).unwrap().len() <= 400 || parse_jsonl_file(&path).len() <= 6,
            "active file should stay near the configured size after rotation"
        );

        logger.shutdown().unwrap();
        cleanup_rotated(&path);
    }

    #[test]
    fn read_recent_spans_current_and_previous_rotated_file() {
        let path = test_path("span");
        let logger = AuditLogger::with_rotation(
            Some(path.clone()),
            AuditRotation {
                max_file_bytes: 400,
                retain_files: 3,
            },
        );

        let mut written = 0usize;
        let first_archive = archive_path_for(&path, 1);
        while !first_archive.exists() {
            logger
                .try_log(record(format!("span-{written:04}")))
                .unwrap();
            written += 1;
            if written.is_multiple_of(4) {
                logger.flush().unwrap();
            }
            assert!(
                written < 200,
                "writer must produce a rotated file for a spanning tail read"
            );
        }
        logger
            .try_log(record(format!("span-{written:04}")))
            .unwrap();
        logger.flush().unwrap();

        let rotated_records = parse_jsonl_file(&first_archive);
        let current_records = parse_jsonl_file(&path);
        assert!(
            !rotated_records.is_empty(),
            "rotated file must contain records"
        );
        assert!(
            !current_records.is_empty(),
            "active file must contain records after rotation"
        );

        let needed = current_records.len() + 1;
        let recent = logger.read_recent(needed);
        assert_eq!(recent.len(), needed);
        assert_eq!(
            recent.last().map(|item| item.id.as_str()),
            current_records.last().map(|item| item.id.as_str()),
            "tail must end with the newest active-file record"
        );
        assert!(
            recent
                .iter()
                .any(|item| rotated_records.iter().any(|rotated| rotated.id == item.id)),
            "read_recent must include at least one record from the previous rotated file"
        );

        logger.shutdown().unwrap();
        cleanup_rotated(&path);
    }

    #[test]
    fn read_recent_does_not_backfill_rotated_files_across_a_capped_current_tail() {
        let path = test_path("cap_hole");
        let archive = archive_path_for(&path, 1);

        let mut rotated = std::fs::File::create(&archive).unwrap();
        for index in 0..5 {
            write_padded_record(&mut rotated, &format!("archive-{index}"), 32);
        }
        use std::io::Write;
        rotated.flush().unwrap();

        let mut current = std::fs::File::create(&path).unwrap();
        for index in 0..8 {
            write_padded_record(&mut current, &format!("middle-{index}"), 700_000);
        }
        for index in 0..8 {
            write_padded_record(&mut current, &format!("newest-{index}"), 700_000);
        }
        current.flush().unwrap();

        let current_records = parse_jsonl_file(&path);
        assert_eq!(current_records.len(), 16);
        assert!(
            std::fs::metadata(&path).unwrap().len() > AUDIT_TAIL_MAX_BYTES as u64,
            "current file must exceed the tail byte cap so the unread middle exists"
        );

        let recent = read_recent_path(&path, current_records.len() + 5);
        let ids: Vec<&str> = recent.iter().map(|item| item.id.as_str()).collect();
        assert!(
            !ids.is_empty(),
            "capped current tail must still return the contiguous newest records"
        );
        assert!(
            ids.iter().all(|id| !id.starts_with("archive-")),
            "must not prepend rotated records across an unread hole in current, got {ids:?}"
        );
        assert_eq!(
            recent,
            current_records[current_records.len() - recent.len()..],
            "returned records must be a contiguous newest suffix of the current file"
        );
        assert!(
            recent.len() < current_records.len(),
            "byte cap must bind so some current-file middle records stay unread; got {}",
            recent.len()
        );

        cleanup_rotated(&path);
    }

    #[test]
    fn disk_full_poisons_admission_and_does_not_silently_drop() {
        let path = test_path("disk_full");
        let state = Arc::new(FaultState::default());
        state.disk_full_writes.store(1, Ordering::SeqCst);
        let logger = fault_logger(path.clone(), state, 3);
        logger.try_log(record("never-durable")).unwrap();
        let flush_error = logger.flush().unwrap_err();
        assert!(
            flush_error.contains("permanent failure"),
            "disk-full must be a terminal persistence error, got {flush_error}"
        );
        assert!(
            flush_error.contains("injected disk full"),
            "disk-full error must be surfaced rather than swallowed, got {flush_error}"
        );
        let rejected = logger.try_log(record("after-disk-full")).unwrap_err();
        assert_eq!(rejected, flush_error);
        assert_eq!(logger.shutdown().unwrap_err(), flush_error);
        cleanup_rotated(&path);
    }

    #[cfg(unix)]
    #[test]
    fn audit_files_use_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let path = test_path("perms");
        let logger = AuditLogger::new(Some(path.clone()));
        logger.try_log(record("secret")).unwrap();
        logger.flush().unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "audit log files must be owner-read/write only");
        logger.shutdown().unwrap();
        cleanup_rotated(&path);
    }

    fn parse_jsonl_bytes(bytes: &[u8]) -> Vec<AuditRecord> {
        String::from_utf8_lossy(bytes)
            .lines()
            .filter(|line| !line.is_empty())
            .filter_map(|line| serde_json::from_str::<AuditRecord>(line).ok())
            .collect()
    }

    fn write_record_line(path: &Path, id: &str) {
        let mut encoded = serde_json::to_vec(&record(id)).unwrap();
        encoded.push(b'\n');
        std::fs::write(path, encoded).unwrap();
    }

    fn rotate_to_empty_current(path: &Path) {
        rotate_log_files(path, 5).unwrap();
        File::create(path).unwrap();
    }

    fn export_bytes_from_generations(path: &Path, highest_archive: usize) -> Vec<u8> {
        let mut expected = Vec::new();
        for generation in (1..=highest_archive).rev() {
            let mut bytes = std::fs::read(archive_path_for(path, generation)).unwrap();
            if !bytes.is_empty() && !bytes.ends_with(b"\n") {
                bytes.push(b'\n');
            }
            expected.extend(bytes);
        }
        let mut current = std::fs::read(path).unwrap();
        if !current.is_empty() && !current.ends_with(b"\n") {
            current.push(b'\n');
        }
        expected.extend(current);
        expected
    }

    #[test]
    fn export_jsonl_does_not_treat_rotation_notfound_as_complete_empty_snapshot() {
        let path = test_path("toctou_export");
        write_record_line(&path, "keep-me");
        TOCTOU_AFTER_ARCHIVE_ENUM.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(rotate_to_empty_current));
        });

        let result = export_jsonl_path(&path);
        let _ = std::fs::remove_file(archive_path_for(&path, 1));
        let _ = std::fs::remove_file(&path);

        let bytes = result.expect("missing rotated current must not look like a complete snapshot");
        let exported = parse_jsonl_bytes(&bytes);
        let keep_count = exported.iter().filter(|item| item.id == "keep-me").count();
        assert_eq!(
            keep_count, 1,
            "export must retain the pre-rotation current record exactly once, got {exported:?}"
        );
    }

    #[test]
    fn read_recent_does_not_duplicate_current_when_it_rotates_to_archive() {
        let path = test_path("toctou_tail");
        write_record_line(&path, "once-only");
        TOCTOU_AFTER_CURRENT_TAIL.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(rotate_to_empty_current));
        });

        let recent = read_recent_path(&path, 10);
        cleanup_rotated(&path);

        let ids: Vec<&str> = recent.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["once-only"],
            "read_recent must not replay current as .1 after a post-flush rotation"
        );
    }

    #[test]
    fn export_jsonl_concatenates_oldest_archive_through_current() {
        let path = test_path("export_span");
        let logger = AuditLogger::with_rotation(
            Some(path.clone()),
            AuditRotation {
                max_file_bytes: 400,
                retain_files: 4,
            },
        );

        let mut written = 0usize;
        let second_archive = archive_path_for(&path, 2);
        while !second_archive.exists() {
            logger
                .try_log(record(format!("export-{written:04}")))
                .unwrap();
            written += 1;
            if written.is_multiple_of(4) {
                logger.flush().unwrap();
            }
            assert!(
                written < 200,
                "writer must produce .1 and .2 for a multi-file export"
            );
        }
        logger.flush().unwrap();

        let oldest = parse_jsonl_file(&second_archive);
        let middle = parse_jsonl_file(&archive_path_for(&path, 1));
        let current = parse_jsonl_file(&path);
        assert!(
            !oldest.is_empty() && !middle.is_empty() && !current.is_empty(),
            "each generation must contain records before export"
        );

        let exported_bytes = logger.export_jsonl().unwrap();
        assert_eq!(
            exported_bytes,
            export_bytes_from_generations(&path, 2),
            "export bytes must be oldest archive → … → current"
        );

        let exported = parse_jsonl_bytes(&exported_bytes);
        assert!(
            oldest.iter().all(|item| exported.contains(item)),
            "export must include records from the oldest archive"
        );
        assert!(
            middle.iter().all(|item| exported.contains(item)),
            "export must include records from the .1 archive"
        );
        assert!(
            current.iter().all(|item| exported.contains(item)),
            "export must include records from the current file"
        );

        logger.shutdown().unwrap();
        cleanup_rotated(&path);
    }
}
