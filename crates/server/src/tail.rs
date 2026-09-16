use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// Reads at most `max_bytes` from the end of a file and returns its last `max_lines` lines.
/// If the byte window begins in the middle of a line, that partial leading line is discarded.
pub(crate) fn read_tail_lines(
    path: &Path,
    max_lines: usize,
    max_bytes: usize,
) -> io::Result<Vec<String>> {
    let mut file = File::open(path)?;
    read_tail_lines_from_file(&mut file, max_lines, max_bytes)
}

pub(crate) fn read_tail_lines_from_file(
    file: &mut File,
    max_lines: usize,
    max_bytes: usize,
) -> io::Result<Vec<String>> {
    if max_lines == 0 || max_bytes == 0 {
        return Ok(Vec::new());
    }

    let file_len = file.metadata()?.len();
    let window_len = file_len.min(max_bytes as u64);
    let window_start = file_len.saturating_sub(window_len);
    file.seek(SeekFrom::Start(window_start))?;

    let mut bytes = Vec::with_capacity(window_len as usize);
    file.take(window_len).read_to_end(&mut bytes)?;
    if window_start > 0 {
        match bytes.iter().position(|byte| *byte == b'\n') {
            Some(index) => bytes.drain(..=index),
            None => return Ok(Vec::new()),
        };
    }

    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    Ok(lines[start..]
        .iter()
        .map(|line| (*line).to_string())
        .collect())
}

/// Reads the last `max_lines` lines using a growing end window so cost stays O(limit),
/// not O(file). `max_bytes` is a safety cap against a single huge line.
pub(crate) fn read_last_n_lines_from_file(
    file: &mut File,
    max_lines: usize,
    max_bytes: usize,
) -> io::Result<Vec<String>> {
    const CHUNK: usize = 8 * 1024;
    if max_lines == 0 || max_bytes == 0 {
        return Ok(Vec::new());
    }
    let file_len = match file.metadata()?.len() {
        0 => return Ok(Vec::new()),
        len => len as usize,
    };
    let cap = file_len.min(max_bytes);
    let mut window = CHUNK.min(cap);
    loop {
        let lines = read_tail_lines_from_file(file, max_lines, window)?;
        if lines.len() >= max_lines || window >= cap {
            return Ok(lines);
        }
        window = cap.min(window.saturating_mul(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn tail_is_bounded_and_discards_a_partial_leading_line() {
        let path = std::env::temp_dir().join(format!(
            "at_pc_tail_{}_{}.log",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let mut file = File::create(&path).unwrap();
        writeln!(file, "head-sentinel-{}", "x".repeat(4_096)).unwrap();
        for index in 0..250 {
            writeln!(file, "line-{index:03}").unwrap();
        }
        file.flush().unwrap();

        let lines = read_tail_lines(&path, 200, 2_048).unwrap();
        assert!(lines.len() <= 200);
        assert_eq!(lines.last().map(String::as_str), Some("line-249"));
        assert!(!lines.iter().any(|line| line.contains("head-sentinel")));
        assert!(lines.first().is_some_and(|line| line.starts_with("line-")));
        let _ = std::fs::remove_file(path);
    }
}
