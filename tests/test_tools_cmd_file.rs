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
