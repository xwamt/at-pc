//! File operations diagnostic tools.
//! Provides safe file reading with tail-reading, line/byte limiting,
//! and safe file writing with automatic timestamped `.bak` backups.

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
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

/// Default tail lines for file reads (200 lines).
pub const DEFAULT_TAIL_LINES: usize = 200;

/// Reads a text file safely with options for tail-reading and maximum byte limits.
///
/// # Arguments
/// * `file_path` - Path to the file.
/// * `tail_lines` - If `None`, defaults to reading the last 200 lines. If `Some(0)`, line-tailing is disabled (full file read up to `max_bytes`). If `Some(n)` (n > 0), returns only the last N lines.
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

    let mut file =
        fs::File::open(path).map_err(|e| format!("Failed to open file '{}': {}", file_path, e))?;
    let file_len = file
        .metadata()
        .map_err(|e| format!("Failed to get metadata for '{}': {}", file_path, e))?
        .len();

    if file_len == 0 {
        return Ok(FileContentResult {
            content: String::new(),
            total_lines: 0,
            truncated: false,
            bytes_read: 0,
        });
    }

    let effective_max_bytes = match max_bytes {
        Some(b) if b > 0 => b,
        _ => DEFAULT_MAX_BYTES,
    };
    let effective_tail_lines = tail_lines.unwrap_or(DEFAULT_TAIL_LINES);

    // If tail_lines is Some(0), disable line-tailing: read from beginning up to effective_max_bytes
    if effective_tail_lines == 0 {
        let to_read = (effective_max_bytes as u64 + 1).min(file_len) as usize;
        let mut buf = vec![0u8; to_read];
        file.read_exact(&mut buf)
            .map_err(|e| format!("Failed to read file '{}': {}", file_path, e))?;

        let truncated = buf.len() > effective_max_bytes || file_len > effective_max_bytes as u64;
        if buf.len() > effective_max_bytes {
            buf.truncate(effective_max_bytes);
        }

        let mut content = String::from_utf8_lossy(&buf).to_string();
        if truncated && content.len() > effective_max_bytes {
            let mut end = effective_max_bytes;
            while !content.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            content.truncate(end);
        }

        let total_lines = content.lines().count();
        let bytes_read = content.len();

        return Ok(FileContentResult {
            content,
            total_lines,
            truncated,
            bytes_read,
        });
    }

    // Tailing with effective_tail_lines > 0
    const CHUNK_SIZE: usize = 64 * 1024;

    // Fast path: if file is within a single 64KB chunk, read whole file and slice lines
    if file_len <= CHUNK_SIZE as u64 {
        let mut buf = vec![0u8; file_len as usize];
        file.read_exact(&mut buf)
            .map_err(|e| format!("Failed to read file '{}': {}", file_path, e))?;
        let text = String::from_utf8_lossy(&buf).to_string();
        let lines: Vec<&str> = text.lines().collect();
        let total_lines = lines.len();

        let mut truncated = false;
        let mut content = if total_lines > effective_tail_lines {
            truncated = true;
            let start = total_lines - effective_tail_lines;
            lines[start..].join("\n")
        } else {
            lines.join("\n")
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
        return Ok(FileContentResult {
            content,
            total_lines,
            truncated,
            bytes_read,
        });
    }

    // Check if the file ends with a trailing newline (\r?\n)
    let check_len = (2u64).min(file_len) as usize;
    let mut last_bytes = vec![0u8; check_len];
    file.seek(SeekFrom::Start(file_len - check_len as u64))
        .map_err(|e| format!("Seek error on '{}': {}", file_path, e))?;
    file.read_exact(&mut last_bytes)
        .map_err(|e| format!("Read error on '{}': {}", file_path, e))?;

    let mut search_end = file_len;
    if last_bytes.last() == Some(&b'\n') {
        search_end -= 1;
        if last_bytes.len() >= 2 && last_bytes[last_bytes.len() - 2] == b'\r' {
            search_end -= 1;
        }
    }

    // Backward seek streaming for large files
    let mut pos = search_end;
    let mut lines_found = 0;
    let mut start_pos = 0u64;
    let mut reached_start = false;
    let mut chunk_buf = vec![0u8; CHUNK_SIZE];

    while pos > 0 && lines_found < effective_tail_lines {
        let read_len = (pos as usize).min(CHUNK_SIZE);
        let read_start = pos - read_len as u64;

        file.seek(SeekFrom::Start(read_start))
            .map_err(|e| format!("Seek error on '{}': {}", file_path, e))?;
        file.read_exact(&mut chunk_buf[..read_len])
            .map_err(|e| format!("Read error on '{}': {}", file_path, e))?;

        let mut idx = read_len;
        while idx > 0 {
            idx -= 1;
            let b = chunk_buf[idx];
            if b == b'\n' {
                lines_found += 1;
                if lines_found == effective_tail_lines {
                    start_pos = read_start + idx as u64 + 1;
                    break;
                }
            }
        }

        if read_start == 0 {
            reached_start = true;
            if lines_found < effective_tail_lines {
                start_pos = 0;
            }
            break;
        }

        pos = read_start;
    }

    let mut truncated = !reached_start || start_pos > 0;

    // Ensure we don't read more than effective_max_bytes from start_pos to file_len
    if (file_len - start_pos) > effective_max_bytes as u64 {
        start_pos = file_len - effective_max_bytes as u64;
        truncated = true;
    }

    let tail_len = (file_len - start_pos) as usize;
    let mut tail_buf = vec![0u8; tail_len];
    file.seek(SeekFrom::Start(start_pos))
        .map_err(|e| format!("Seek error on '{}': {}", file_path, e))?;
    file.read_exact(&mut tail_buf)
        .map_err(|e| format!("Read error on '{}': {}", file_path, e))?;

    // If start_pos was byte-clamped into the middle of a multi-byte UTF-8 sequence,
    // skip leading continuation bytes (0x80..=0xBF) so we never emit invalid replacement characters.
    let mut slice = &tail_buf[..];
    if start_pos > 0 {
        while !slice.is_empty() && (slice[0] & 0xC0 == 0x80) {
            slice = &slice[1..];
        }
    }

    let raw_text = String::from_utf8_lossy(slice).to_string();
    let lines: Vec<&str> = raw_text.lines().collect();

    let mut content = if lines.len() > effective_tail_lines {
        lines[lines.len() - effective_tail_lines..].join("\n")
    } else {
        lines.join("\n")
    };

    if content.len() > effective_max_bytes {
        truncated = true;
        let mut end = effective_max_bytes;
        while !content.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        content.truncate(end);
    }

    let total_lines = if reached_start && start_pos == 0 {
        lines.len()
    } else {
        effective_tail_lines.min(lines.len())
    };

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
        message: format!(
            "Successfully wrote {} bytes to '{}'",
            content.len(),
            file_path
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_read_text_file_basic_tail() {
        let temp_dir =
            std::env::temp_dir().join(format!("at_test_file_ops_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_path = temp_dir.join("test_tail.txt");

        let content = "line 1\nline 2\nline 3\nline 4\nline 5\n";
        std::fs::write(&file_path, content).unwrap();

        let res = read_text_file(file_path.to_str().unwrap(), Some(2), None).unwrap();
        assert_eq!(res.total_lines, 5);
        assert!(res.truncated);
        assert_eq!(res.content, "line 4\nline 5");

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_read_text_file_zero_tail_reads_from_start() {
        let temp_dir =
            std::env::temp_dir().join(format!("at_test_file_ops_zero_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_path = temp_dir.join("test_zero.txt");

        let content = "first line\nsecond line\nthird line\n";
        std::fs::write(&file_path, content).unwrap();

        let res = read_text_file(file_path.to_str().unwrap(), Some(0), Some(15)).unwrap();
        assert!(res.truncated);
        assert_eq!(res.content, "first line\nseco");

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_read_text_file_simulated_large_file_backward_seek() {
        let temp_dir =
            std::env::temp_dir().join(format!("at_test_file_ops_large_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_path = temp_dir.join("test_large_2gb.log");

        // Create a sparse 2GB file
        let mut file = std::fs::File::create(&file_path).unwrap();
        let target_len = 2 * 1024 * 1024 * 1024u64; // 2 GB
        file.set_len(target_len).unwrap();

        // Write some known lines near the very end of this 2GB file
        let end_lines = (1..=300)
            .map(|i| format!("log line {:05} timestamp=2026-09-05 info=simulated", i))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let end_bytes = end_lines.as_bytes();
        let write_offset = target_len - end_bytes.len() as u64;

        file.seek(SeekFrom::Start(write_offset)).unwrap();
        file.write_all(end_bytes).unwrap();
        file.flush().unwrap();
        drop(file);

        let start_time = std::time::Instant::now();
        let res = read_text_file(file_path.to_str().unwrap(), Some(200), None).unwrap();
        let elapsed = start_time.elapsed();

        assert!(res.truncated);
        assert_eq!(res.total_lines, 200);
        assert!(res.content.contains("log line 00101"));
        assert!(res.content.contains("log line 00300"));
        assert!(!res.content.contains("log line 00100"));

        // Must complete instantaneously (< 100ms) without reading 2GB into memory
        assert!(
            elapsed.as_millis() < 100,
            "Reading tail took too long: {:?}",
            elapsed
        );

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_read_text_file_trailing_empty_lines() {
        let temp_dir =
            std::env::temp_dir().join(format!("at_test_file_ops_empty_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_path = temp_dir.join("test_empty_lines.txt");

        // "line 1\nline 2\n\n" -> 3 lines: "line 1", "line 2", ""
        let content = "line 1\nline 2\n\n";
        std::fs::write(&file_path, content).unwrap();

        let res = read_text_file(file_path.to_str().unwrap(), Some(2), None).unwrap();
        assert_eq!(res.total_lines, 3);
        assert!(res.truncated);
        assert_eq!(res.content, "line 2\n");

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_read_text_file_utf8_multibyte_backward_seek() {
        let temp_dir =
            std::env::temp_dir().join(format!("at_test_file_ops_utf8_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_path = temp_dir.join("test_utf8.log");

        // Write 70KB file with multibyte Chinese characters to trigger backward seek (> 64KB)
        let mut file = std::fs::File::create(&file_path).unwrap();
        let padding =
            "这是一段用于填充大小的中文测试日志，确保文件超过64KB分块阈值。\n".repeat(1200);
        file.write_all(padding.as_bytes()).unwrap();

        let tail_part = "尾部日志行 001：测试终端状态\n尾部日志行 002：逆向流式读取完成\n";
        file.write_all(tail_part.as_bytes()).unwrap();
        file.flush().unwrap();
        drop(file);

        // Read last 2 lines with a tight max_bytes limit that clamps into multibyte characters
        let res = read_text_file(file_path.to_str().unwrap(), Some(2), Some(80)).unwrap();
        assert!(res.truncated);
        // Ensure no Unicode replacement character (U+FFFD) is present due to broken character boundary
        assert!(
            !res.content.contains('\u{FFFD}'),
            "Content contains broken UTF-8 replacement char: {}",
            res.content
        );
        assert!(res.content.contains("逆向流式读取完成"));

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
