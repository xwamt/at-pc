//! File operations diagnostic tools.
//! Provides safe file reading with tail-reading, line/byte limiting,
//! and safe file writing with automatic timestamped `.bak` backups.

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Result of a file read operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileContentResult {
    pub content: String,
    pub total_lines: usize,
    pub truncated: bool,
    pub bytes_read: usize,
}

/// Result of a file write operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileWriteResult {
    pub success: bool,
    pub bytes_written: usize,
    pub backup_path: Option<String>,
    pub message: String,
}

/// Default byte limit for file reads (500 KB).
pub const DEFAULT_MAX_BYTES: usize = 512_000;

/// Reads a text file safely with options for tail-reading and maximum byte limits.
///
/// # Arguments
/// * `file_path` - Path to the file.
/// * `tail_lines` - If specified, returns only the last N lines.
/// * `max_bytes` - If specified, restricts output size to at most N bytes (default: 512,000).
pub fn read_text_file(
    file_path: &str,
    tail_lines: Option<usize>,
    max_bytes: Option<usize>,
) -> Result<FileContentResult, String> {
    let path = Path::new(file_path);
    if !path.exists() {
        return Err(format!("File '{}' does not exist", file_path));
    }
    if !path.is_file() {
        return Err(format!("Path '{}' is not a file", file_path));
    }

    let raw_bytes = fs::read(path).map_err(|e| format!("Failed to read file '{}': {}", file_path, e))?;
    let text = String::from_utf8_lossy(&raw_bytes).to_string();

    let lines: Vec<&str> = text.lines().collect();
    let total_lines = lines.len();

    let mut truncated = false;
    let mut content = if let Some(n) = tail_lines {
        if n > 0 && total_lines > n {
            truncated = true;
            let start = total_lines - n;
            lines[start..].join("\n")
        } else {
            text
        }
    } else {
        text
    };

    let effective_max_bytes = match max_bytes {
        Some(b) if b > 0 => b,
        _ => DEFAULT_MAX_BYTES,
    };

    if content.len() > effective_max_bytes {
        truncated = true;
        let mut end = effective_max_bytes;
        while !content.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        content.truncate(end);
    }

    let bytes_read = content.len();

    Ok(FileContentResult {
        content,
        total_lines,
        truncated,
        bytes_read,
    })
}

/// Safely writes content to a file, optionally creating a timestamped `.bak` backup first.
///
/// # Arguments
/// * `file_path` - Path to the target file.
/// * `content` - Text content to write.
/// * `create_backup` - If true and the file exists, creates a timestamped `.bak` copy.
pub fn write_text_file(
    file_path: &str,
    content: &str,
    create_backup: bool,
) -> Result<FileWriteResult, String> {
    let path = Path::new(file_path);
    let mut backup_path = None;

    if create_backup && path.exists() {
        let timestamp = Local::now().format("%Y%m%d_%H%M%S_%3f").to_string();
        let backup_file_name = format!("{}.{}.bak", file_path, timestamp);
        fs::copy(path, &backup_file_name)
            .map_err(|e| format!("Failed to create backup at '{}': {}", backup_file_name, e))?;
        backup_path = Some(backup_file_name);
    }

    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory '{}': {}", parent.display(), e))?;
        }
    }

    fs::write(path, content)
        .map_err(|e| format!("Failed to write to file '{}': {}", file_path, e))?;

    Ok(FileWriteResult {
        success: true,
        bytes_written: content.len(),
        backup_path,
        message: format!("Successfully wrote {} bytes to '{}'", content.len(), file_path),
    })
}
