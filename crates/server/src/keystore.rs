//! API Key Store implementation for the at-pc server gate.
//!
//! Provides generation, secure hashing (sha256 with 16-byte salt), constant-time
//! verification, and lifecycle management (issue, revoke, rotate, list) for
//! Secret Keys (`atpc_sk_...`) and Device Tokens (`atpc_dv_...`).
//!
//! Permissions on Unix: keys.json is created and maintained with 0600 permissions.

use crate::config::Role;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;

pub const KEY_PREFIX: &str = "atpc_";
pub const KEY_ID_LEN: usize = 8;
pub const KEY_SECRET_LEN: usize = 43;

/// Crockford base32 alphabet (lowercase): 0-9 and 22 letters, excluding I, L, O, U.
pub const CROCKFORD32: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Key kind: SecretKey (`sk`) or DeviceToken (`dv`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyKind {
    #[serde(rename = "sk")]
    SecretKey,
    #[serde(rename = "dv")]
    DeviceToken,
}

impl KeyKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            KeyKind::SecretKey => "sk",
            KeyKind::DeviceToken => "dv",
        }
    }
}

impl std::fmt::Display for KeyKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for KeyKind {
    type Err = AuthError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "sk" => Ok(KeyKind::SecretKey),
            "dv" => Ok(KeyKind::DeviceToken),
            _ => Err(AuthError::InvalidFormat),
        }
    }
}

fn default_hash_algo() -> String {
    "sha256-salt-v1".to_string()
}

/// Persistent record of a key stored in keys.json.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyRecord {
    pub id: String,
    pub name: String,
    pub kind: KeyKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    pub salt: String,
    pub hash: String,
    #[serde(default = "default_hash_algo")]
    pub hash_algo: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
}

impl KeyRecord {
    /// Sanitized view without salt or hash.
    pub fn to_summary(&self) -> KeySummary {
        KeySummary {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind,
            role: self.role,
            scopes: self.scopes.clone(),
            device_id: self.device_id.clone(),
            hash_algo: self.hash_algo.clone(),
            created_at: self.created_at.clone(),
            expires_at: self.expires_at.clone(),
            revoked_at: self.revoked_at.clone(),
            last_used_at: self.last_used_at.clone(),
        }
    }
}

/// Sanitized view of a key without secret, salt, or hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeySummary {
    pub id: String,
    pub name: String,
    pub kind: KeyKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    pub hash_algo: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
}

/// Authenticated subject returned after verifying a credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub id: String,
    pub name: String,
    pub kind: KeyKind,
    pub role: Option<Role>,
    pub scopes: Vec<String>,
    pub device_id: Option<String>,
}

impl Principal {
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    pub fn matches_device(&self, client_device_id: Option<&str>) -> bool {
        match (&self.device_id, client_device_id) {
            (Some(expected), Some(actual)) => expected == actual,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }
}

/// Authentication and verification errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    InvalidFormat,
    KeyNotFound,
    HashMismatch,
    Revoked,
    Expired,
    WrongKind,
    MissingScope,
    DeviceMismatch,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFormat => write!(f, "invalid credential format"),
            Self::KeyNotFound => write!(f, "key not found"),
            Self::HashMismatch => write!(f, "hash mismatch"),
            Self::Revoked => write!(f, "key has been revoked"),
            Self::Expired => write!(f, "key has expired"),
            Self::WrongKind => write!(f, "wrong key kind"),
            Self::MissingScope => write!(f, "missing required scope"),
            Self::DeviceMismatch => write!(f, "device ID mismatch"),
        }
    }
}

impl std::error::Error for AuthError {}

/// Encodes 5 bytes as 8 lowercase Crockford base32 characters.
pub fn crockford_base32_id(bytes: &[u8; 5]) -> String {
    let mut buf = [0u8; KEY_ID_LEN];
    let acc = u64::from_be_bytes([
        0,
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        0,
        0,
    ]);
    for (i, slot) in buf.iter_mut().enumerate() {
        let shift = 35 - (i as u32) * 5;
        let index = ((acc >> shift) & 0x1f) as usize;
        *slot = CROCKFORD32[index];
    }
    String::from_utf8(buf.to_vec()).expect("alphabet is ASCII")
}

/// Encodes 32 bytes as 43 chars of base64url without padding.
pub fn base64url_secret(bytes: &[u8; 32]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Encodes byte slice into lowercase hex string.
pub fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

/// Parses credential string formatted as `atpc_<kind>_<id>_<secret>`.
pub fn parse_credential(cred: &str) -> Result<(KeyKind, String, String), AuthError> {
    let rest = cred.strip_prefix(KEY_PREFIX).ok_or(AuthError::InvalidFormat)?;
    let (kind_str, rest) = rest.split_once('_').ok_or(AuthError::InvalidFormat)?;
    let kind = match kind_str {
        "sk" => KeyKind::SecretKey,
        "dv" => KeyKind::DeviceToken,
        _ => return Err(AuthError::InvalidFormat),
    };
    let (id, secret) = rest.split_once('_').ok_or(AuthError::InvalidFormat)?;
    if id.len() != KEY_ID_LEN || secret.len() != KEY_SECRET_LEN {
        return Err(AuthError::InvalidFormat);
    }
    if !id.bytes().all(|b| CROCKFORD32.contains(&b)) {
        return Err(AuthError::InvalidFormat);
    }
    if !secret.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err(AuthError::InvalidFormat);
    }
    Ok((kind, id.to_string(), secret.to_string()))
}

/// Computes sha256(salt_hex || secret) and returns lowercase hex string.
pub fn compute_hash(salt_hex: &str, secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt_hex.as_bytes());
    hasher.update(secret.as_bytes());
    let result = hasher.finalize();
    hex_encode(&result)
}

/// Constant-time comparison on byte slices.
pub fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// KeyStore manager.
#[derive(Clone, Debug)]
pub struct KeyStore {
    path: PathBuf,
    records: Arc<RwLock<HashMap<String, KeyRecord>>>,
    last_persisted_used: Arc<Mutex<HashMap<String, Instant>>>,
}

impl KeyStore {
    /// Loads an existing keys.json file or creates a new one.
    ///
    /// If the file does not exist, checks if `<data_dir>/bootstrap_key.txt` exists.
    /// If so, migrates the bootstrap key into an Admin record named `bootstrap-root`,
    /// writes `keys.json` with 0600 permissions, and deletes `bootstrap_key.txt`.
    pub fn load_or_create(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("create data dir {}: {e}", parent.display()))?;
            }
        }

        let records = if path.exists() {
            Self::load_from_disk(path)?
        } else {
            let mut map = HashMap::new();
            let data_dir = path.parent().unwrap_or_else(|| Path::new("."));
            let bootstrap_file = data_dir.join(crate::bootstrap::BOOTSTRAP_KEY_FILE);
            if bootstrap_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&bootstrap_file) {
                    if let Some(key_line) = content.lines().map(str::trim).find(|l| l.starts_with("atpc_sk_")) {
                        if let Ok((kind, id, secret)) = parse_credential(key_line) {
                            let mut salt_bytes = [0u8; 16];
                            getrandom::getrandom(&mut salt_bytes)
                                .map_err(|e| format!("CSPRNG error: {e}"))?;
                            let salt = hex_encode(&salt_bytes);
                            let hash = compute_hash(&salt, &secret);
                            let record = KeyRecord {
                                id: id.clone(),
                                name: "bootstrap-root".to_string(),
                                kind,
                                role: Some(Role::Admin),
                                scopes: vec!["console".to_string(), "mcp".to_string()],
                                device_id: None,
                                salt,
                                hash,
                                hash_algo: default_hash_algo(),
                                created_at: chrono::Utc::now().to_rfc3339(),
                                expires_at: None,
                                revoked_at: None,
                                last_used_at: None,
                            };
                            map.insert(id, record);
                        }
                    }
                }
            }
            Self::save_to_disk(path, &map)?;
            map
        };

        Ok(Self {
            path: path.to_path_buf(),
            records: Arc::new(RwLock::new(records)),
            last_persisted_used: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Returns true if the keystore contains at least one active (unrevoked) key.
    pub fn has_active_keys(&self) -> bool {
        let records = match self.records.read() {
            Ok(r) => r,
            Err(poisoned) => poisoned.into_inner(),
        };
        records.values().any(|r| r.revoked_at.is_none())
    }

    /// Verifies credential against the keystore:
    /// - Parses credential into (kind, id, secret)
    /// - Looks up record by id O(1)
    /// - Checks kind
    /// - Performs constant-time hash comparison
    /// - Verifies record is not revoked
    /// - Verifies record is not expired
    /// - Throttles last_used_at update (at most once every 60s per key)
    /// - Returns Principal
    pub fn verify(&self, credential: &str) -> Result<Principal, AuthError> {
        let (kind, id, secret) = parse_credential(credential)?;

        let record = {
            let records = self.records.read().map_err(|_| AuthError::KeyNotFound)?;
            records.get(&id).cloned().ok_or(AuthError::KeyNotFound)?
        };

        if record.kind != kind {
            return Err(AuthError::WrongKind);
        }

        let computed_hash = compute_hash(&record.salt, &secret);
        if !constant_time_compare(computed_hash.as_bytes(), record.hash.as_bytes()) {
            return Err(AuthError::HashMismatch);
        }

        if record.revoked_at.is_some() {
            return Err(AuthError::Revoked);
        }

        if let Some(ref exp_str) = record.expires_at {
            if let Ok(exp) = chrono::DateTime::parse_from_rfc3339(exp_str) {
                if exp <= chrono::Utc::now() {
                    return Err(AuthError::Expired);
                }
            } else {
                return Err(AuthError::Expired);
            }
        }

        self.touch_last_used(&id);

        Ok(Principal {
            id: record.id,
            name: record.name,
            kind: record.kind,
            role: record.role,
            scopes: record.scopes,
            device_id: record.device_id,
        })
    }

    /// Issues a new key and persists it atomically with 0600 permissions.
    /// Returns (record, plaintext_key).
    pub fn issue(
        &self,
        name: String,
        kind: KeyKind,
        role: Option<Role>,
        scopes: Vec<String>,
        device_id: Option<String>,
        expires_at: Option<String>,
    ) -> Result<(KeyRecord, String), String> {
        let mut records = self.records.write().map_err(|e| e.to_string())?;

        let mut id_bytes = [0u8; 5];
        let mut id;
        loop {
            getrandom::getrandom(&mut id_bytes)
                .map_err(|e| format!("CSPRNG error: {e}"))?;
            id = crockford_base32_id(&id_bytes);
            if !records.contains_key(&id) {
                break;
            }
        }

        let mut secret_bytes = [0u8; 32];
        getrandom::getrandom(&mut secret_bytes)
            .map_err(|e| format!("CSPRNG error: {e}"))?;
        let secret = base64url_secret(&secret_bytes);

        let mut salt_bytes = [0u8; 16];
        getrandom::getrandom(&mut salt_bytes)
            .map_err(|e| format!("CSPRNG error: {e}"))?;
        let salt = hex_encode(&salt_bytes);

        let hash = compute_hash(&salt, &secret);
        let plaintext_key = format!("atpc_{}_{}_{}", kind.as_str(), id, secret);
        let created_at = chrono::Utc::now().to_rfc3339();

        let record = KeyRecord {
            id: id.clone(),
            name,
            kind,
            role,
            scopes,
            device_id,
            salt,
            hash,
            hash_algo: default_hash_algo(),
            created_at,
            expires_at,
            revoked_at: None,
            last_used_at: None,
        };

        records.insert(id, record.clone());
        Self::save_to_disk(&self.path, &records)?;

        Ok((record, plaintext_key))
    }

    /// Revokes an existing key by id.
    ///
    /// Guard: Cannot revoke or delete the LAST active admin key
    /// (`kind == SecretKey && role == Admin && revoked_at.is_none()`).
    pub fn revoke(&self, id: &str) -> Result<(), String> {
        let mut records = self.records.write().map_err(|e| e.to_string())?;

        let is_active_admin = {
            let record = records.get(id).ok_or_else(|| format!("key '{}' not found", id))?;
            record.kind == KeyKind::SecretKey
                && record.role == Some(Role::Admin)
                && record.revoked_at.is_none()
        };

        if is_active_admin {
            let active_admin_count = records
                .values()
                .filter(|r| {
                    r.kind == KeyKind::SecretKey
                        && r.role == Some(Role::Admin)
                        && r.revoked_at.is_none()
                })
                .count();
            if active_admin_count <= 1 {
                return Err("cannot revoke the last active admin key".to_string());
            }
        }

        let record = records.get_mut(id).unwrap();
        record.revoked_at = Some(chrono::Utc::now().to_rfc3339());
        Self::save_to_disk(&self.path, &records)?;
        Ok(())
    }

    /// Rotates a key:
    /// - Issues a new key with same name, kind, role, scopes, device_id.
    /// - If `grace_hours == Some(0)`: revokes old key immediately.
    /// - Otherwise sets old key's `expires_at = now + grace_hours` (defaulting to 24 hours).
    pub fn rotate(&self, id: &str, grace_hours: Option<u64>) -> Result<(KeyRecord, String), String> {
        let mut records = self.records.write().map_err(|e| e.to_string())?;
        let old_record = records.get(id).cloned().ok_or_else(|| format!("key '{}' not found", id))?;

        // Generate new key
        let mut id_bytes = [0u8; 5];
        let mut new_id;
        loop {
            getrandom::getrandom(&mut id_bytes)
                .map_err(|e| format!("CSPRNG error: {e}"))?;
            new_id = crockford_base32_id(&id_bytes);
            if !records.contains_key(&new_id) {
                break;
            }
        }

        let mut secret_bytes = [0u8; 32];
        getrandom::getrandom(&mut secret_bytes)
            .map_err(|e| format!("CSPRNG error: {e}"))?;
        let secret = base64url_secret(&secret_bytes);

        let mut salt_bytes = [0u8; 16];
        getrandom::getrandom(&mut salt_bytes)
            .map_err(|e| format!("CSPRNG error: {e}"))?;
        let salt = hex_encode(&salt_bytes);

        let hash = compute_hash(&salt, &secret);
        let plaintext_key = format!("atpc_{}_{}_{}", old_record.kind.as_str(), new_id, secret);
        let created_at = chrono::Utc::now().to_rfc3339();

        let new_record = KeyRecord {
            id: new_id.clone(),
            name: old_record.name.clone(),
            kind: old_record.kind,
            role: old_record.role,
            scopes: old_record.scopes.clone(),
            device_id: old_record.device_id.clone(),
            salt,
            hash,
            hash_algo: default_hash_algo(),
            created_at,
            expires_at: None,
            revoked_at: None,
            last_used_at: None,
        };

        records.insert(new_id, new_record.clone());

        // Update old record
        let old_entry = records.get_mut(id).unwrap();
        let now = chrono::Utc::now();
        match grace_hours {
            Some(0) => {
                old_entry.revoked_at = Some(now.to_rfc3339());
            }
            Some(h) => {
                old_entry.expires_at = Some((now + chrono::Duration::hours(h as i64)).to_rfc3339());
            }
            None => {
                old_entry.expires_at = Some((now + chrono::Duration::hours(24)).to_rfc3339());
            }
        }

        Self::save_to_disk(&self.path, &records)?;
        Ok((new_record, plaintext_key))
    }

    /// Deletes a key from the keystore.
    /// Guard: Cannot delete the last active admin key.
    pub fn delete(&self, id: &str) -> Result<(), String> {
        let mut records = self.records.write().map_err(|e| e.to_string())?;

        let is_active_admin = {
            let record = records.get(id).ok_or_else(|| format!("key '{}' not found", id))?;
            record.kind == KeyKind::SecretKey
                && record.role == Some(Role::Admin)
                && record.revoked_at.is_none()
        };

        if is_active_admin {
            let active_admin_count = records
                .values()
                .filter(|r| {
                    r.kind == KeyKind::SecretKey
                        && r.role == Some(Role::Admin)
                        && r.revoked_at.is_none()
                })
                .count();
            if active_admin_count <= 1 {
                return Err("cannot delete the last active admin key".to_string());
            }
        }

        records.remove(id);
        Self::save_to_disk(&self.path, &records)?;
        Ok(())
    }

    /// Returns list of all key summaries, sorted by creation date.
    pub fn list(&self) -> Vec<KeySummary> {
        let records = match self.records.read() {
            Ok(r) => r,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut list: Vec<KeySummary> = records.values().map(|r| r.to_summary()).collect();
        list.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        list
    }

    /// Returns summary for a specific key id.
    pub fn get_summary(&self, id: &str) -> Option<KeySummary> {
        let records = match self.records.read() {
            Ok(r) => r,
            Err(poisoned) => poisoned.into_inner(),
        };
        records.get(id).map(|r| r.to_summary())
    }

    /// Returns the storage file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn touch_last_used(&self, id: &str) {
        let now_instant = Instant::now();
        let should_persist = {
            let mut tracker = match self.last_persisted_used.lock() {
                Ok(t) => t,
                Err(poisoned) => poisoned.into_inner(),
            };
            match tracker.get(id) {
                Some(&last) if now_instant.duration_since(last) < Duration::from_secs(60) => false,
                _ => {
                    tracker.insert(id.to_string(), now_instant);
                    true
                }
            }
        };

        if should_persist {
            let now_rfc3339 = chrono::Utc::now().to_rfc3339();
            if let Ok(mut records) = self.records.write() {
                if let Some(record) = records.get_mut(id) {
                    record.last_used_at = Some(now_rfc3339);
                    let _ = Self::save_to_disk(&self.path, &records);
                }
            }
        }
    }

    fn load_from_disk(path: &Path) -> Result<HashMap<String, KeyRecord>, String> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read keys file {}: {e}", path.display()))?;
        if content.trim().is_empty() {
            return Ok(HashMap::new());
        }

        #[derive(Deserialize)]
        struct FileFormat {
            #[serde(default)]
            keys: Vec<KeyRecord>,
        }

        if let Ok(file_format) = serde_json::from_str::<FileFormat>(&content) {
            let mut map = HashMap::new();
            for key in file_format.keys {
                map.insert(key.id.clone(), key);
            }
            return Ok(map);
        }

        if let Ok(keys) = serde_json::from_str::<Vec<KeyRecord>>(&content) {
            let mut map = HashMap::new();
            for key in keys {
                map.insert(key.id.clone(), key);
            }
            return Ok(map);
        }

        Err(format!("failed to parse keys file {}", path.display()))
    }

    fn save_to_disk(path: &Path, records: &HashMap<String, KeyRecord>) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create dir {}: {e}", parent.display()))?;
            }
        }

        let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_path = path.with_extension(format!("tmp.{}.{}", std::process::id(), counter));
        let write_result = (|| {
            #[cfg(unix)]
            let mut file = {
                use std::os::unix::fs::OpenOptionsExt;
                std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(&temp_path)
                    .map_err(|e| format!("failed to open temp file {}: {e}", temp_path.display()))?
            };
            #[cfg(not(unix))]
            let mut file = std::fs::File::create(&temp_path)
                .map_err(|e| format!("failed to open temp file {}: {e}", temp_path.display()))?;

            let mut keys_list: Vec<&KeyRecord> = records.values().collect();
            keys_list.sort_by(|a, b| a.created_at.cmp(&b.created_at));

            let data = serde_json::json!({
                "version": 1,
                "keys": keys_list,
            });

            serde_json::to_writer_pretty(&mut file, &data)
                .map_err(|e| format!("failed to serialize keys: {e}"))?;
            file.write_all(b"\n")
                .map_err(|e| format!("failed to write newline: {e}"))?;
            file.flush()
                .map_err(|e| format!("failed to flush temp file: {e}"))?;
            file.sync_all()
                .map_err(|e| format!("failed to sync temp file: {e}"))?;
            drop(file);

            std::fs::rename(&temp_path, path)
                .map_err(|e| format!("failed to rename {}: {e}", path.display()))?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            }

            Ok(())
        })();

        if write_result.is_err() {
            let _ = std::fs::remove_file(&temp_path);
        }
        write_result
    }
}
