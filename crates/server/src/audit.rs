//! Immutable JSONL Persistent Audit Logging Module.
//! Records sensitive actions, tool executions, authorization decisions,
//! and administrative interventions to a durable append-only log file.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A single immutable audit trail record
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
    pub status: String, // "SUCCESS" | "FAILED" | "DENIED"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Thread-safe audit logger that appends JSONL records to disk
#[derive(Debug)]
pub struct AuditLogger {
    log_path: Option<PathBuf>,
    lock: Mutex<()>,
}

impl AuditLogger {
    /// Creates a new AuditLogger targeting the given optional file path
    pub fn new(log_path: Option<PathBuf>) -> Self {
        Self {
            log_path,
            lock: Mutex::new(()),
        }
    }

    /// Redacts a raw token to a safe prefix (e.g., "adm-***") for safe auditing
    pub fn redact_token(token: &str) -> String {
        let clean = token.trim();
        if clean.len() <= 4 {
            "***".to_string()
        } else {
            format!("{}***", &clean[..3.min(clean.len())])
        }
    }

    /// Appends an audit record to the persistent log file
    pub fn log(&self, record: AuditRecord) {
        let path = match &self.log_path {
            Some(p) => p,
            None => return,
        };

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        if let Ok(_guard) = self.lock.lock() {
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
                if let Ok(json_str) = serde_json::to_string(&record) {
                    let _ = writeln!(file, "{}", json_str);
                }
            }
        }
    }

    /// Reads recent audit records from the log file (tail N records)
    pub fn read_recent(&self, limit: usize) -> Vec<AuditRecord> {
        let path = match &self.log_path {
            Some(p) => p,
            None => return Vec::new(),
        };

        if let Ok(content) = std::fs::read_to_string(path) {
            let mut records: Vec<AuditRecord> = content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|line| serde_json::from_str::<AuditRecord>(line).ok())
                .collect();
            let start = records.len().saturating_sub(limit.max(1));
            records.drain(start..).collect()
        } else {
            Vec::new()
        }
    }

    /// Returns the target log file path if set
    pub fn log_path(&self) -> Option<&Path> {
        self.log_path.as_deref()
    }
}
