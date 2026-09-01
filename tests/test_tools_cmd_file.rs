use at_pc::tools::{command, file_ops};

#[test]
fn test_read_and_write_file_with_backup() {
    let temp_dir = std::env::temp_dir();
    let file_name = format!("at_pc_test_file_{}.txt", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
    let test_file = temp_dir.join(file_name);
    let test_file_str = test_file.to_str().unwrap();

    // 1. Write initial file
    let write_res = file_ops::write_text_file(test_file_str, "Hello World\nLine 2", false).expect("write initial failed");
    assert!(write_res.success);
    assert_eq!(write_res.bytes_written, 18);
    assert!(write_res.backup_path.is_none());

    // 2. Read file
    let read_res = file_ops::read_text_file(test_file_str, Some(10), None).expect("read failed");
    assert_eq!(read_res.content, "Hello World\nLine 2");
    assert_eq!(read_res.total_lines, 2);
    assert!(!read_res.truncated);

    // 3. Overwrite with backup
    let overwrite_res = file_ops::write_text_file(test_file_str, "Modified", true).expect("overwrite failed");
    assert!(overwrite_res.success);
    assert!(overwrite_res.backup_path.is_some());

    let backup_path = overwrite_res.backup_path.unwrap();
    assert!(std::path::Path::new(&backup_path).exists(), "Backup file should exist");

    let backup_content = std::fs::read_to_string(&backup_path).expect("failed to read backup");
    assert_eq!(backup_content, "Hello World\nLine 2");

    // 4. Verify overwritten content
    let read_res2 = file_ops::read_text_file(test_file_str, None, None).expect("read overwritten failed");
    assert_eq!(read_res2.content, "Modified");
    assert_eq!(read_res2.total_lines, 1);

    // Cleanup
    let _ = std::fs::remove_file(&test_file);
    let _ = std::fs::remove_file(&backup_path);
}

#[test]
fn test_read_file_tail_lines() {
    let temp_dir = std::env::temp_dir();
    let file_name = format!("at_pc_test_tail_{}.txt", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
    let test_file = temp_dir.join(file_name);
    let test_file_str = test_file.to_str().unwrap();

    let content = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6";
    let _ = file_ops::write_text_file(test_file_str, content, false).expect("write failed");

    let read_res = file_ops::read_text_file(test_file_str, Some(3), None).expect("read tail failed");
    assert_eq!(read_res.total_lines, 6);
    assert!(read_res.truncated);
    assert_eq!(read_res.content, "line 4\nline 5\nline 6");

    let _ = std::fs::remove_file(&test_file);
}

#[test]
fn test_read_file_default_tail_200_lines() {
    let temp_dir = std::env::temp_dir();
    let file_name = format!("at_pc_test_default_tail_{}.txt", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
    let test_file = temp_dir.join(file_name);
    let test_file_str = test_file.to_str().unwrap();

    // Generate file with 300 lines
    let lines: Vec<String> = (1..=300).map(|i| format!("line {}", i)).collect();
    let content = lines.join("\n");
    let _ = file_ops::write_text_file(test_file_str, &content, false).expect("write failed");

    // 1. None tail_lines -> defaults to last 200 lines
    let read_default = file_ops::read_text_file(test_file_str, None, None).expect("read default tail failed");
    assert_eq!(read_default.total_lines, 300);
    assert!(read_default.truncated, "Default read on 300-line file should be truncated");
    let default_lines: Vec<&str> = read_default.content.lines().collect();
    assert_eq!(default_lines.len(), 200);
    assert_eq!(default_lines.first(), Some(&"line 101"));
    assert_eq!(default_lines.last(), Some(&"line 300"));

    // 2. Some(0) tail_lines -> unlimited (all 300 lines)
    let read_all = file_ops::read_text_file(test_file_str, Some(0), None).expect("read all lines failed");
    assert_eq!(read_all.total_lines, 300);
    assert!(!read_all.truncated, "Some(0) read should not be truncated");
    let all_lines: Vec<&str> = read_all.content.lines().collect();
    assert_eq!(all_lines.len(), 300);
    assert_eq!(all_lines.first(), Some(&"line 1"));
    assert_eq!(all_lines.last(), Some(&"line 300"));

    let _ = std::fs::remove_file(&test_file);
}

#[test]
fn test_read_file_max_bytes() {
    let temp_dir = std::env::temp_dir();
    let file_name = format!("at_pc_test_bytes_{}.txt", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
    let test_file = temp_dir.join(file_name);
    let test_file_str = test_file.to_str().unwrap();

    let content = "1234567890abcdefghij"; // 20 bytes
    let _ = file_ops::write_text_file(test_file_str, content, false).expect("write failed");

    let read_res = file_ops::read_text_file(test_file_str, None, Some(10)).expect("read bytes failed");
    assert_eq!(read_res.content.len(), 10);
    assert!(read_res.truncated);

    let _ = std::fs::remove_file(&test_file);
}

#[test]
fn test_read_nonexistent_file() {
    let res = file_ops::read_text_file("/path/to/definitely/nonexistent/file_xyz.txt", None, None);
    assert!(res.is_err(), "Reading nonexistent file should return Err");
}

#[test]
fn test_exec_cmd_echo() {
    let res = command::exec_cmd("echo hello_cmd", 10, None).expect("exec_cmd failed");
    assert_eq!(res.exit_code, 0);
    assert!(res.stdout.trim().contains("hello_cmd"));
}

#[test]
fn test_exec_powershell_echo() {
    let res = command::exec_powershell("echo hello_ps", 10, None).expect("exec_powershell failed");
    assert_eq!(res.exit_code, 0);
    assert!(res.stdout.trim().contains("hello_ps"));
}

#[test]
fn test_exec_cmd_with_cwd() {
    let temp_dir = std::env::temp_dir();
    let cwd_str = temp_dir.to_str().unwrap();

    #[cfg(windows)]
    let cmd_str = "cd";
    #[cfg(not(windows))]
    let cmd_str = "pwd";

    let res = command::exec_cmd(cmd_str, 10, Some(cwd_str)).expect("exec_cmd with cwd failed");
    assert_eq!(res.exit_code, 0);
    assert!(!res.stdout.is_empty());
}

#[test]
fn test_exec_cmd_timeout() {
    #[cfg(windows)]
    let long_cmd = "ping -n 5 127.0.0.1 > nul";
    #[cfg(not(windows))]
    let long_cmd = "sleep 3";

    let res = command::exec_cmd(long_cmd, 1, None);
    assert!(res.is_err(), "Command should have timed out");
    let err_msg = res.unwrap_err();
    assert!(err_msg.contains("timed out"), "Error should mention timeout: {}", err_msg);
}

#[test]
fn test_mcp_dispatch_read_text_file() {
    use at_pc::tools::dispatch_mcp_tool;
    use serde_json::json;

    let temp_dir = std::env::temp_dir();
    let file_name = format!("at_pc_test_mcp_read_{}.txt", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
    let test_file = temp_dir.join(file_name);
    let test_file_str = test_file.to_str().unwrap();

    let lines: Vec<String> = (1..=250).map(|i| format!("line {}", i)).collect();
    let content = lines.join("\n");
    let _ = file_ops::write_text_file(test_file_str, &content, false).expect("write failed");

    // Dispatch without tail_lines -> defaults to 200
    let res_default = dispatch_mcp_tool("read_text_file", json!({ "file_path": test_file_str })).expect("dispatch failed");
    assert_eq!(res_default["total_lines"], 250);
    assert_eq!(res_default["truncated"], true);
    let text = res_default["content"].as_str().unwrap();
    assert_eq!(text.lines().count(), 200);

    // Dispatch with tail_lines = 0 -> unlimited
    let res_all = dispatch_mcp_tool("read_text_file", json!({ "file_path": test_file_str, "tail_lines": 0 })).expect("dispatch failed");
    assert_eq!(res_all["total_lines"], 250);
    assert_eq!(res_all["truncated"], false);
    let text_all = res_all["content"].as_str().unwrap();
    assert_eq!(text_all.lines().count(), 250);

    let _ = std::fs::remove_file(&test_file);
}

#[test]
fn test_read_file_multibyte_utf8_truncation() {
    let temp_dir = std::env::temp_dir();
    let file_name = format!(
        "at_pc_test_utf8_{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let test_file = temp_dir.join(file_name);
    let test_file_str = test_file.to_str().unwrap();

    // Chinese characters are 3 bytes each, Emoji are 4 bytes each
    // "你好世界🚀🎉"
    // "你好世界" -> 4 * 3 = 12 bytes
    // "🚀🎉" -> 2 * 4 = 8 bytes
    // Total = 20 bytes
    let content = "你好世界🚀🎉";
    let _ = file_ops::write_text_file(test_file_str, content, false).expect("write failed");

    // Test truncating at every single byte offset (1..=20) to ensure zero panics on non-boundary slices
    for max_b in 1..=content.len() {
        let read_res = file_ops::read_text_file(test_file_str, None, Some(max_b));
        assert!(read_res.is_ok(), "read_text_file should not panic on max_bytes={}", max_b);
        let res = read_res.unwrap();
        assert!(res.bytes_read <= max_b);
        assert_eq!(res.bytes_read, res.content.len());
        // Verify res.content is valid UTF-8 and is a prefix of content
        assert!(content.starts_with(&res.content));
    }

    // Specific boundary checks:
    // max_bytes = 4: should yield "你" (3 bytes), not panic at byte 4 of "好" (bytes 3..6)
    let res_4 = file_ops::read_text_file(test_file_str, None, Some(4)).unwrap();
    assert_eq!(res_4.content, "你");
    assert_eq!(res_4.bytes_read, 3);
    assert!(res_4.truncated);

    // max_bytes = 14: 12 bytes ("你好世界") + 2 bytes into "🚀" (bytes 12..16) -> should truncate to "你好世界" (12 bytes)
    let res_14 = file_ops::read_text_file(test_file_str, None, Some(14)).unwrap();
    assert_eq!(res_14.content, "你好世界");
    assert_eq!(res_14.bytes_read, 12);
    assert!(res_14.truncated);

    // max_bytes = 16: exactly includes "你好世界🚀" (16 bytes)
    let res_16 = file_ops::read_text_file(test_file_str, None, Some(16)).unwrap();
    assert_eq!(res_16.content, "你好世界🚀");
    assert_eq!(res_16.bytes_read, 16);
    assert!(res_16.truncated);

    // Cleanup
    let _ = std::fs::remove_file(&test_file);
}
