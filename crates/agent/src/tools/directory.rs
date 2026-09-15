//! Directory browsing and file search diagnostic tools.
//! Provides safe filesystem directory listing, metadata inspection,
//! and wildcard/pattern file search with depth and entry limits.

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Detailed entry information for files and directories.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirEntryInfo {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_file: bool,
    pub is_symlink: bool,
    pub size_bytes: u64,
    pub modified_time: Option<String>,
    pub readonly: bool,
}

/// Result of directory listing operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListDirectoryResult {
    pub path: String,
    pub total_entries: usize,
    pub truncated: bool,
    pub entries: Vec<DirEntryInfo>,
}

/// Result of file search operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchFilesResult {
    pub base_path: String,
    pub pattern: String,
    pub matches: Vec<DirEntryInfo>,
    pub total_found: usize,
}

fn format_system_time(time: std::time::SystemTime) -> Option<String> {
    let dt: DateTime<Local> = time.into();
    Some(dt.format("%Y-%m-%d %H:%M:%S").to_string())
}

fn read_entry_metadata(path: &Path, file_name: String) -> DirEntryInfo {
    let mut is_dir = false;
    let mut is_file = false;
    let mut is_symlink = false;
    let mut size_bytes = 0;
    let mut modified_time = None;
    let mut readonly = false;

    if let Ok(meta) = fs::symlink_metadata(path) {
        is_symlink = meta.file_type().is_symlink();
        is_dir = meta.is_dir();
        is_file = meta.is_file();
        size_bytes = meta.len();
        readonly = meta.permissions().readonly();
        if let Ok(modified) = meta.modified() {
            modified_time = format_system_time(modified);
        }
    }

    DirEntryInfo {
        name: file_name,
        path: path.to_string_lossy().to_string(),
        is_dir,
        is_file,
        is_symlink,
        size_bytes,
        modified_time,
        readonly,
    }
}

/// Lists files and subdirectories within a given path.
///
/// # Arguments
/// * `path` - Target directory path.
/// * `recursive` - Whether to recursively list subdirectories (default: false).
/// * `max_depth` - Maximum recursion depth (default: 1 if non-recursive, 3 if recursive).
/// * `limit` - Maximum number of entries to return (default: 100).
pub fn list_directory(
    dir_path: &str,
    recursive: Option<bool>,
    max_depth: Option<usize>,
    limit: Option<usize>,
) -> Result<ListDirectoryResult, String> {
    let base = Path::new(dir_path);
    if !base.exists() {
        return Err(format!("Directory '{}' does not exist", dir_path));
    }
    if !base.is_dir() {
        return Err(format!("Path '{}' is not a directory", dir_path));
    }

    let is_recursive = recursive.unwrap_or(false);
    let depth_limit = max_depth.unwrap_or(if is_recursive { 3 } else { 1 }).max(1);
    let entry_limit = limit.unwrap_or(100).clamp(1, 1000);

    let mut entries = Vec::new();
    let mut queue: Vec<(PathBuf, usize)> = vec![(base.to_path_buf(), 1)];
    let mut truncated = false;

    while let Some((curr_dir, depth)) = queue.pop() {
        let read_dir = match fs::read_dir(&curr_dir) {
            Ok(rd) => rd,
            Err(e) => {
                tracing::warn!("Failed to read directory '{:?}': {}", curr_dir, e);
                continue;
            }
        };

        let mut sub_entries: Vec<(PathBuf, String, bool)> = Vec::new();
        for item in read_dir.flatten() {
            let p = item.path();
            let name = item.file_name().to_string_lossy().to_string();
            let is_d = p.is_dir();
            sub_entries.push((p, name, is_d));
        }

        // Sort: directories first, then alphabetical
        sub_entries.sort_by(|a, b| {
            if a.2 == b.2 {
                a.1.to_lowercase().cmp(&b.1.to_lowercase())
            } else if a.2 {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        });

        for (p, name, is_d) in sub_entries {
            if entries.len() >= entry_limit {
                truncated = true;
                break;
            }

            let info = read_entry_metadata(&p, name);
            entries.push(info);

            if is_recursive && is_d && depth < depth_limit {
                queue.push((p, depth + 1));
            }
        }

        if truncated {
            break;
        }
    }

    Ok(ListDirectoryResult {
        path: dir_path.to_string(),
        total_entries: entries.len(),
        truncated,
        entries,
    })
}

/// Recursively searches for files matching a pattern or substring within `base_path`.
///
/// # Arguments
/// * `base_path` - Root directory to start search.
/// * `pattern` - Filename search query or simple glob (supports `*`).
/// * `max_results` - Maximum results returned (default: 50).
/// * `max_depth` - Maximum search depth (default: 5).
pub fn search_files(
    base_path: &str,
    pattern: &str,
    max_results: Option<usize>,
    max_depth: Option<usize>,
) -> Result<SearchFilesResult, String> {
    let base = Path::new(base_path);
    if !base.exists() {
        return Err(format!("Base directory '{}' does not exist", base_path));
    }
    if !base.is_dir() {
        return Err(format!("Path '{}' is not a directory", base_path));
    }

    let pattern_clean = pattern.trim().to_lowercase();
    if pattern_clean.is_empty() {
        return Err("Search pattern cannot be empty".to_string());
    }

    let limit = max_results.unwrap_or(50).clamp(1, 500);
    let depth_limit = max_depth.unwrap_or(5).clamp(1, 15);

    let mut matches = Vec::new();
    let mut queue: Vec<(PathBuf, usize)> = vec![(base.to_path_buf(), 1)];

    while let Some((curr_dir, depth)) = queue.pop() {
        let read_dir = match fs::read_dir(&curr_dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };

        for item in read_dir.flatten() {
            let p = item.path();
            let name = item.file_name().to_string_lossy().to_string();
            let name_lower = name.to_lowercase();
            let is_d = p.is_dir();

            let is_match = if pattern_clean.contains('*') {
                let parts: Vec<&str> = pattern_clean.split('*').filter(|s| !s.is_empty()).collect();
                if parts.is_empty() {
                    true
                } else {
                    let mut pos = 0;
                    let mut all_found = true;
                    for part in parts {
                        if let Some(idx) = name_lower[pos..].find(part) {
                            pos += idx + part.len();
                        } else {
                            all_found = false;
                            break;
                        }
                    }
                    all_found
                }
            } else {
                name_lower.contains(&pattern_clean)
            };

            if is_match {
                matches.push(read_entry_metadata(&p, name));
                if matches.len() >= limit {
                    break;
                }
            }

            if is_d && depth < depth_limit {
                queue.push((p, depth + 1));
            }
        }

        if matches.len() >= limit {
            break;
        }
    }

    Ok(SearchFilesResult {
        base_path: base_path.to_string(),
        pattern: pattern.to_string(),
        total_found: matches.len(),
        matches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_directory_basic() {
        let res = list_directory(".", Some(false), Some(1), Some(50)).unwrap();
        assert!(!res.entries.is_empty());
        assert_eq!(res.path, ".");
    }

    #[test]
    fn test_search_files_cargo() {
        let res = search_files(".", "Cargo.*", Some(10), Some(3)).unwrap();
        assert!(res.matches.iter().any(|m| m.name.starts_with("Cargo")));
    }
}
