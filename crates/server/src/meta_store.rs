use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use serde::{Deserialize, Serialize};
use at_pc_protocol::models::TerminalInfo;

/// Metadata stored persistently for each terminal
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

/// Thread-safe file-backed metadata store for terminal names, notes, and tags
#[derive(Debug, Clone)]
pub struct TerminalMetaStore {
    file_path: PathBuf,
    is_in_memory: bool,
    records: Arc<RwLock<HashMap<String, TerminalMeta>>>,
}

impl TerminalMetaStore {
    /// Create a persistent store backed by the specified file path
    pub fn new<P: AsRef<Path>>(file_path: P) -> Self {
        let path_buf = file_path.as_ref().to_path_buf();
        let records = Self::load_from_disk(&path_buf);
        Self {
            file_path: path_buf,
            is_in_memory: false,
            records: Arc::new(RwLock::new(records)),
        }
    }

    /// Create an in-memory store that never writes to disk (ideal for unit testing)
    pub fn in_memory() -> Self {
        Self {
            file_path: PathBuf::new(),
            is_in_memory: true,
            records: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load existing records from disk
    fn load_from_disk(path: &Path) -> HashMap<String, TerminalMeta> {
        if !path.exists() || !path.is_file() {
            debug!("Metadata file '{:?}' does not exist yet. Starting empty.", path);
            return HashMap::new();
        }

        match std::fs::read_to_string(path) {
            Ok(content) => {
                if content.trim().is_empty() {
                    return HashMap::new();
                }
                match serde_json::from_str::<HashMap<String, TerminalMeta>>(&content) {
                    Ok(data) => {
                        info!("Loaded {} terminal metadata records from {:?}", data.len(), path);
                        data
                    }
                    Err(e) => {
                        warn!("Failed to parse metadata file {:?}: {}. Starting empty.", path, e);
                        HashMap::new()
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read metadata file {:?}: {}. Starting empty.", path, e);
                HashMap::new()
            }
        }
    }

    /// Atomically write current records to disk
    fn save_to_disk(&self, records: &HashMap<String, TerminalMeta>) -> Result<(), String> {
        if self.is_in_memory || self.file_path.as_os_str().is_empty() {
            return Ok(());
        }

        if let Some(parent) = self.file_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent directory for {:?}: {}", parent, e))?;
            }
        }

        let json_str = serde_json::to_string_pretty(records)
            .map_err(|e| format!("Failed to serialize metadata records: {}", e))?;

        // Write to temporary file first, then atomically rename to prevent corruption
        let tmp_file_path = self.file_path.with_extension(format!("tmp.{}", std::process::id()));
        if let Err(e) = std::fs::write(&tmp_file_path, json_str.as_bytes()) {
            return Err(format!("Failed to write temporary metadata file {:?}: {}", tmp_file_path, e));
        }

        if let Err(e) = std::fs::rename(&tmp_file_path, &self.file_path) {
            let _ = std::fs::remove_file(&tmp_file_path);
            return Err(format!("Failed to atomically rename metadata file to {:?}: {}", self.file_path, e));
        }

        debug!("Persisted {} terminal metadata records to {:?}", records.len(), self.file_path);
        Ok(())
    }

    /// Record newly seen or updated terminal registration
    pub async fn record_registration(&self, info: &TerminalInfo) {
        let mut map = self.records.write().await;
        let now = chrono::Utc::now().timestamp();
        let entry = map.entry(info.terminal_id.clone()).or_insert_with(|| TerminalMeta {
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

        let _ = self.save_to_disk(&map);
    }

    /// Get metadata for a specific terminal
    pub async fn get(&self, terminal_id: &str) -> Option<TerminalMeta> {
        let map = self.records.read().await;
        map.get(terminal_id).cloned()
    }

    /// List all known metadata records
    pub async fn list_all(&self) -> HashMap<String, TerminalMeta> {
        let map = self.records.read().await;
        map.clone()
    }

    /// Update terminal custom name, notes, and tags
    pub async fn update_meta(
        &self,
        terminal_id: &str,
        custom_name: Option<String>,
        notes: Option<String>,
        tags: Option<Vec<String>>,
    ) -> Result<TerminalMeta, String> {
        let mut map = self.records.write().await;
        let now = chrono::Utc::now().timestamp();

        let meta = map.entry(terminal_id.to_string()).or_insert_with(|| TerminalMeta {
            terminal_id: terminal_id.to_string(),
            custom_name: None,
            notes: None,
            tags: Vec::new(),
            last_known_info: None,
            created_at: now,
            updated_at: now,
        });

        if let Some(cn) = custom_name {
            let trimmed = cn.trim().to_string();
            meta.custom_name = if trimmed.is_empty() { None } else { Some(trimmed) };
        }

        if let Some(n) = notes {
            let trimmed = n.trim().to_string();
            meta.notes = if trimmed.is_empty() { None } else { Some(trimmed) };
        }

        if let Some(ts) = tags {
            let mut clean_tags = Vec::new();
            for t in ts {
                let clean = t.trim().to_string();
                if !clean.is_empty() && !clean_tags.contains(&clean) {
                    clean_tags.push(clean);
                }
            }
            meta.tags = clean_tags;
        }

        meta.updated_at = now;
        let result = meta.clone();

        self.save_to_disk(&map)?;
        Ok(result)
    }

    /// Remove a terminal from metadata storage
    pub async fn remove(&self, terminal_id: &str) -> Result<bool, String> {
        let mut map = self.records.write().await;
        let removed = map.remove(terminal_id).is_some();
        if removed {
            self.save_to_disk(&map)?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_store_operations() {
        let store = TerminalMetaStore::in_memory();

        // 1. Initially empty
        assert!(store.get("term-1").await.is_none());

        // 2. Update custom name & notes
        let meta = store
            .update_meta(
                "term-1",
                Some("财务-主控机".to_string()),
                Some("常驻财务室".to_string()),
                Some(vec!["财务".to_string(), "Win11".to_string()]),
            )
            .await
            .expect("update should succeed");

        assert_eq!(meta.custom_name.as_deref(), Some("财务-主控机"));
        assert_eq!(meta.notes.as_deref(), Some("常驻财务室"));
        assert_eq!(meta.tags, vec!["财务".to_string(), "Win11".to_string()]);

        // 3. Get matches updated values
        let retrieved = store.get("term-1").await.expect("must exist");
        assert_eq!(retrieved.custom_name.as_deref(), Some("财务-主控机"));

        // 4. Clear custom name with empty string
        let updated = store
            .update_meta("term-1", Some("".to_string()), None, None)
            .await
            .unwrap();
        assert!(updated.custom_name.is_none());
        assert_eq!(updated.notes.as_deref(), Some("常驻财务室"));

        // 5. Remove
        assert!(store.remove("term-1").await.unwrap());
        assert!(store.get("term-1").await.is_none());
    }

    #[tokio::test]
    async fn test_file_backed_persistence_and_reload() {
        let temp_dir = std::env::temp_dir().join(format!("at_pc_meta_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_path = temp_dir.join("terminals_meta.json");

        // 1. Create store and add record
        {
            let store = TerminalMetaStore::new(&file_path);
            let res = store
                .update_meta(
                    "pc-dev-01",
                    Some("研发编译机".to_string()),
                    Some("配备32G内存".to_string()),
                    Some(vec!["研发".to_string()]),
                )
                .await;
            assert!(res.is_ok());
        }

        // 2. Reopen store from same file and verify persisted content
        {
            let store_reloaded = TerminalMetaStore::new(&file_path);
            let rec = store_reloaded.get("pc-dev-01").await.expect("persisted record must exist");
            assert_eq!(rec.custom_name.as_deref(), Some("研发编译机"));
            assert_eq!(rec.notes.as_deref(), Some("配备32G内存"));
            assert_eq!(rec.tags, vec!["研发".to_string()]);
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
