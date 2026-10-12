//! Persistent, capability-free pending enrollment records.
//!
//! This store is deliberately independent from `TerminalRegistry` and
//! `TerminalMetaStore`. A record here has no WebSocket sender, cannot be
//! selected as a terminal, and cannot receive tools, desktop frames, or input.

use at_pc_protocol::messages::EnrollmentHello;
use at_pc_protocol::models::{EnrollmentStatus, PendingEnrollmentSummary};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use tokio::sync::{Mutex, RwLock};

const ID_BYTES: usize = 16;
const NONCE_BYTES: usize = 32;
const DEFAULT_CHALLENGE_TTL_SECS: i64 = 120;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingEnrollmentRecord {
    enrollment_id: String,
    stable_device_id: String,
    public_key: String,
    public_key_fingerprint: String,
    facts: at_pc_protocol::models::EnrollmentDeviceFacts,
    capabilities: Vec<String>,
    requested_at: i64,
    last_seen_at: i64,
    expires_at: i64,
    request_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_ip: Option<String>,
}

impl PendingEnrollmentRecord {
    fn summary(&self) -> PendingEnrollmentSummary {
        PendingEnrollmentSummary {
            enrollment_id: self.enrollment_id.clone(),
            stable_device_id: self.stable_device_id.clone(),
            device_fingerprint: self.public_key_fingerprint.clone(),
            facts: self.facts.clone(),
            capabilities: self.capabilities.clone(),
            status: EnrollmentStatus::Pending,
            requested_at: self.requested_at,
            last_seen_at: self.last_seen_at,
            expires_at: self.expires_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnrollmentChallenge {
    pub enrollment_id: String,
    pub server_nonce: String,
    pub expires_at: i64,
    pub hello: EnrollmentHello,
    pub source_ip: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EnrollmentUpsert {
    pub summary: PendingEnrollmentSummary,
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnrollmentStoreError {
    CapacityExceeded,
    IdentityConflict,
    ChallengeNotFound,
    ChallengeExpired,
    RandomnessUnavailable,
    Persistence(String),
}

impl std::fmt::Display for EnrollmentStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CapacityExceeded => write!(formatter, "pending enrollment capacity exceeded"),
            Self::IdentityConflict => write!(formatter, "device identity conflicts with an existing pending enrollment"),
            Self::ChallengeNotFound => write!(formatter, "enrollment challenge not found or already consumed"),
            Self::ChallengeExpired => write!(formatter, "enrollment challenge expired"),
            Self::RandomnessUnavailable => write!(formatter, "system randomness unavailable"),
            Self::Persistence(error) => write!(formatter, "pending enrollment persistence failed: {error}"),
        }
    }
}

impl std::error::Error for EnrollmentStoreError {}

/// A bounded persistent store of Pending-only enrollment requests.
#[derive(Debug)]
pub struct DeviceEnrollmentStore {
    records: RwLock<HashMap<String, PendingEnrollmentRecord>>,
    challenges: Mutex<HashMap<String, EnrollmentChallenge>>,
    file_path: Option<PathBuf>,
    pending_ttl_secs: i64,
    challenge_ttl_secs: i64,
    max_pending: usize,
}

impl DeviceEnrollmentStore {
    pub fn new(
        file_path: PathBuf,
        pending_ttl_secs: u64,
        challenge_ttl_secs: u64,
        max_pending: usize,
    ) -> Self {
        Self {
            records: RwLock::new(load_records(&file_path)),
            challenges: Mutex::new(HashMap::new()),
            file_path: Some(file_path),
            pending_ttl_secs: pending_ttl_secs.max(60) as i64,
            challenge_ttl_secs: challenge_ttl_secs.max(1) as i64,
            max_pending: max_pending.max(1),
        }
    }

    pub fn in_memory() -> Self {
        Self {
            records: RwLock::new(HashMap::new()),
            challenges: Mutex::new(HashMap::new()),
            file_path: None,
            pending_ttl_secs: 24 * 60 * 60,
            challenge_ttl_secs: DEFAULT_CHALLENGE_TTL_SECS,
            max_pending: 1_000,
        }
    }

    pub async fn issue_challenge(
        &self,
        hello: EnrollmentHello,
        source_ip: Option<String>,
    ) -> Result<EnrollmentChallenge, EnrollmentStoreError> {
        let now = chrono::Utc::now().timestamp();
        let fingerprint = fingerprint(&hello.public_key);
        {
            let mut records = self.records.write().await;
            prune_expired(&mut records, now);
            if records.values().any(|record| {
                record.stable_device_id == hello.stable_device_id
                    && record.public_key_fingerprint != fingerprint
            }) || records.values().any(|record| {
                record.public_key_fingerprint == fingerprint
                    && record.stable_device_id != hello.stable_device_id
            }) {
                return Err(EnrollmentStoreError::IdentityConflict);
            }
            let known = records.values().any(|record| {
                record.stable_device_id == hello.stable_device_id
                    && record.public_key_fingerprint == fingerprint
            });
            if !known && records.len() >= self.max_pending {
                return Err(EnrollmentStoreError::CapacityExceeded);
            }
        }

        let enrollment_id = random_base64url(ID_BYTES)?;
        let server_nonce = random_base64url(NONCE_BYTES)?;
        let challenge = EnrollmentChallenge {
            enrollment_id: enrollment_id.clone(),
            server_nonce,
            expires_at: now + self.challenge_ttl_secs.max(DEFAULT_CHALLENGE_TTL_SECS),
            hello,
            source_ip,
        };
        let mut challenges = self.challenges.lock().await;
        challenges.retain(|_, existing| existing.expires_at > now);
        challenges.insert(enrollment_id, challenge.clone());
        Ok(challenge)
    }

    /// Removes a challenge before verifying its signature, ensuring every nonce
    /// is single use even if the proof is invalid.
    pub async fn consume_challenge(
        &self,
        enrollment_id: &str,
    ) -> Result<EnrollmentChallenge, EnrollmentStoreError> {
        let now = chrono::Utc::now().timestamp();
        let challenge = self
            .challenges
            .lock()
            .await
            .remove(enrollment_id)
            .ok_or(EnrollmentStoreError::ChallengeNotFound)?;
        if challenge.expires_at <= now {
            return Err(EnrollmentStoreError::ChallengeExpired);
        }
        Ok(challenge)
    }

    pub async fn create_or_refresh_pending(
        &self,
        hello: EnrollmentHello,
        source_ip: Option<String>,
    ) -> Result<EnrollmentUpsert, EnrollmentStoreError> {
        let now = chrono::Utc::now().timestamp();
        let key_fingerprint = fingerprint(&hello.public_key);
        let (summary, created, snapshot) = {
            let mut records = self.records.write().await;
            prune_expired(&mut records, now);
            let matching_id = records
                .iter()
                .find(|(_, record)| record.stable_device_id == hello.stable_device_id)
                .map(|(id, _)| id.clone());
            let matching_key = records
                .iter()
                .find(|(_, record)| record.public_key_fingerprint == key_fingerprint)
                .map(|(id, _)| id.clone());
            if let Some(id) = matching_id.as_deref() {
                if records
                    .get(id)
                    .is_some_and(|record| record.public_key_fingerprint != key_fingerprint)
                {
                    return Err(EnrollmentStoreError::IdentityConflict);
                }
            }
            if let Some(id) = matching_key.as_deref() {
                if records
                    .get(id)
                    .is_some_and(|record| record.stable_device_id != hello.stable_device_id)
                {
                    return Err(EnrollmentStoreError::IdentityConflict);
                }
            }

            let existing_id = matching_id.or(matching_key);
            let created = existing_id.is_none();
            if created && records.len() >= self.max_pending {
                return Err(EnrollmentStoreError::CapacityExceeded);
            }
            let record_id = existing_id.unwrap_or_else(|| random_base64url(ID_BYTES).expect("randomness checked during enrollment"));
            let record = records.entry(record_id.clone()).or_insert_with(|| PendingEnrollmentRecord {
                enrollment_id: record_id,
                stable_device_id: hello.stable_device_id.clone(),
                public_key: hello.public_key.clone(),
                public_key_fingerprint: key_fingerprint.clone(),
                facts: hello.facts.clone(),
                capabilities: hello.capabilities.clone(),
                requested_at: now,
                last_seen_at: now,
                expires_at: now + self.pending_ttl_secs,
                request_count: 0,
                source_ip: source_ip.clone(),
            });
            record.facts = hello.facts;
            record.capabilities = hello.capabilities;
            record.last_seen_at = now;
            record.expires_at = now + self.pending_ttl_secs;
            record.request_count = record.request_count.saturating_add(1);
            record.source_ip = source_ip;
            (record.summary(), created, records.clone())
        };
        self.persist(snapshot).await?;
        Ok(EnrollmentUpsert { summary, created })
    }

    pub async fn list_pending(&self) -> Vec<PendingEnrollmentSummary> {
        let now = chrono::Utc::now().timestamp();
        let mut records = self.records.write().await;
        prune_expired(&mut records, now);
        let mut result = records
            .values()
            .map(PendingEnrollmentRecord::summary)
            .collect::<Vec<_>>();
        result.sort_unstable_by(|left, right| right.last_seen_at.cmp(&left.last_seen_at));
        result
    }

    pub async fn flush(&self) -> Result<(), EnrollmentStoreError> {
        let snapshot = self.records.read().await.clone();
        self.persist(snapshot).await
    }

    async fn persist(
        &self,
        records: HashMap<String, PendingEnrollmentRecord>,
    ) -> Result<(), EnrollmentStoreError> {
        let Some(path) = self.file_path.clone() else {
            return Ok(());
        };
        tokio::task::spawn_blocking(move || write_records(&path, &records))
            .await
            .map_err(|error| EnrollmentStoreError::Persistence(format!("writer task failed: {error}")))?
            .map_err(EnrollmentStoreError::Persistence)
    }
}

fn prune_expired(records: &mut HashMap<String, PendingEnrollmentRecord>, now: i64) {
    records.retain(|_, record| record.expires_at > now);
}

fn fingerprint(public_key: &str) -> String {
    let digest = Sha256::digest(public_key.as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn random_base64url(bytes_len: usize) -> Result<String, EnrollmentStoreError> {
    let mut bytes = vec![0_u8; bytes_len];
    getrandom::getrandom(&mut bytes).map_err(|_| EnrollmentStoreError::RandomnessUnavailable)?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn load_records(path: &Path) -> HashMap<String, PendingEnrollmentRecord> {
    match std::fs::read_to_string(path) {
        Ok(content) if !content.trim().is_empty() => serde_json::from_str(&content).unwrap_or_default(),
        _ => HashMap::new(),
    }
}

fn write_records(
    path: &Path,
    records: &HashMap<String, PendingEnrollmentRecord>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create enrollment directory failed: {error}"))?;
        }
    }
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| format!("create enrollment temp file failed: {error}"))?;
    serde_json::to_writer_pretty(&mut file, records)
        .map_err(|error| format!("serialize enrollment records failed: {error}"))?;
    file.write_all(b"\n")
        .map_err(|error| format!("write enrollment records failed: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("sync enrollment records failed: {error}"))?;
    std::fs::rename(&temporary, path)
        .map_err(|error| format!("replace enrollment records failed: {error}"))
}
