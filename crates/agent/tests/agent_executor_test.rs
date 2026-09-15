use at_pc_agent::executor::AgentExecutor;

#[tokio::test]
async fn test_executor_runs_system_overview() {
    let executor = AgentExecutor::new();
    let res = executor.execute("get_system_overview", serde_json::json!({})).await;
    assert!(res.is_ok(), "get_system_overview failed: {:?}", res.err());
    let val = res.unwrap();
    assert!(val.get("os_name").is_some() || val.get("os").is_some());
    assert!(val.get("cpu_model").is_some());
}

#[tokio::test]
async fn test_executor_exec_cmd() {
    let executor = AgentExecutor::new();
    let res = executor
        .execute("exec_cmd", serde_json::json!({ "command": "echo at_agent_test" }))
        .await;
    assert!(res.is_ok(), "exec_cmd failed: {:?}", res.err());
    let val = res.unwrap();
    assert_eq!(val.get("exit_code").and_then(|c| c.as_i64()), Some(0));
    assert!(val.get("stdout").and_then(|s| s.as_str()).unwrap().contains("at_agent_test"));
}

#[tokio::test]
async fn test_executor_file_ops() {
    let executor = AgentExecutor::new();
    let temp_file = std::env::temp_dir().join("at_agent_test_file.txt");
    let temp_path = temp_file.to_string_lossy().to_string();

    // 1. Write file
    let write_res = executor
        .execute(
            "write_text_file",
            serde_json::json!({
                "file_path": temp_path,
                "content": "line 1\nline 2\nline 3\n",
                "create_backup": false
            }),
        )
        .await;
    assert!(write_res.is_ok());

    // 2. Read file
    let read_res = executor
        .execute(
            "read_text_file",
            serde_json::json!({
                "file_path": temp_path,
                "tail_lines": 2
            }),
        )
        .await;
    assert!(read_res.is_ok());
    let read_val = read_res.unwrap();
    assert_eq!(read_val.get("total_lines").and_then(|l| l.as_u64()), Some(3));
    let content = read_val.get("content").and_then(|c| c.as_str()).unwrap();
    assert!(content.contains("line 2"));
    assert!(content.contains("line 3"));

    // Cleanup
    let _ = std::fs::remove_file(&temp_file);
}

#[tokio::test]
async fn test_executor_list_processes() {
    let executor = AgentExecutor::new();
    let res = executor
        .execute("list_processes", serde_json::json!({ "limit": 5 }))
        .await;
    assert!(res.is_ok());
    let val = res.unwrap();
    assert!(val.is_array());
    let list = val.as_array().unwrap();
    assert!(!list.is_empty());
    assert!(list.len() <= 5);
}

#[tokio::test]
async fn test_executor_unknown_tool() {
    let executor = AgentExecutor::new();
    let res = executor.execute("non_existent_tool", serde_json::json!({})).await;
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Unknown or unsupported tool"));
}

#[tokio::test]
async fn test_executor_directory_and_search() {
    let executor = AgentExecutor::new();

    // 1. list_directory
    let list_res = executor
        .execute(
            "list_directory",
            serde_json::json!({
                "path": ".",
                "recursive": false,
                "limit": 10
            }),
        )
        .await;
    assert!(list_res.is_ok(), "list_directory failed: {:?}", list_res.err());
    let list_val = list_res.unwrap();
    assert!(list_val.get("entries").and_then(|e| e.as_array()).is_some());

    // 2. search_files
    let search_res = executor
        .execute(
            "search_files",
            serde_json::json!({
                "base_path": ".",
                "pattern": "Cargo.*",
                "max_results": 5
            }),
        )
        .await;
    assert!(search_res.is_ok(), "search_files failed: {:?}", search_res.err());
    let search_val = search_res.unwrap();
    assert!(search_val.get("matches").and_then(|m| m.as_array()).is_some());
}

#[tokio::test]
async fn test_executor_network_tools() {
    let executor = AgentExecutor::new();

    // 1. list_network_connections
    let conn_res = executor
        .execute(
            "list_network_connections",
            serde_json::json!({ "limit": 10 }),
        )
        .await;
    assert!(conn_res.is_ok(), "list_network_connections failed: {:?}", conn_res.err());

    // 2. test_network
    let net_res = executor
        .execute(
            "test_network",
            serde_json::json!({
                "target_host": "127.0.0.1",
                "timeout_ms": 1000
            }),
        )
        .await;
    assert!(net_res.is_ok(), "test_network failed: {:?}", net_res.err());
    let net_val = net_res.unwrap();
    assert_eq!(net_val.get("reachable").and_then(|r| r.as_bool()), Some(true));
}

#[tokio::test]
async fn test_executor_event_logs() {
    let executor = AgentExecutor::new();
    let res = executor
        .execute(
            "get_event_logs",
            serde_json::json!({
                "log_name": "System",
                "level": "Error",
                "limit": 5
            }),
        )
        .await;
    assert!(res.is_ok(), "get_event_logs failed: {:?}", res.err());
    let val = res.unwrap();
    assert_eq!(val.get("log_name").and_then(|l| l.as_str()), Some("System"));
    assert!(val.get("events").and_then(|e| e.as_array()).is_some());
}

#[tokio::test]
async fn test_executor_cancel_terminates_subprocess() {
    let executor = AgentExecutor::new();
    let sleep_cmd = if cfg!(windows) {
        "powershell -Command Start-Sleep -Seconds 10"
    } else {
        "sleep 10"
    };

    let exec_clone = executor.clone();
    let call_id = "test-call-cancel-direct";
    let task = tokio::spawn(async move {
        exec_clone
            .execute_with_call_id(
                call_id,
                "exec_cmd",
                serde_json::json!({ "command": sleep_cmd }),
            )
            .await
    });

    // Wait for process to spawn
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let start = std::time::Instant::now();
    let cancelled = executor.cancel(call_id).await;
    let elapsed = start.elapsed();

    assert!(cancelled, "Cancel should report true for active call");
    assert!(
        elapsed < std::time::Duration::from_millis(300),
        "Cancellation must execute in under 300ms, took {:?}",
        elapsed
    );

    let res = task.await.unwrap();
    assert!(res.is_err(), "Cancelled task must return Err: {:?}", res);
    assert!(res.unwrap_err().contains("cancelled"));

    let remaining = executor.kill_all_processes();
    assert_eq!(remaining, 0, "No child processes should be orphaned");
}

#[tokio::test]
async fn test_executor_command_timeout_does_not_deadlock() {
    let executor = AgentExecutor::new();
    let sleep_cmd = if cfg!(windows) {
        "powershell -Command Start-Sleep -Seconds 5"
    } else {
        "sleep 5"
    };

    let start = std::time::Instant::now();
    // Execute command with 1 second timeout
    let res = executor
        .execute(
            "exec_cmd",
            serde_json::json!({
                "command": sleep_cmd,
                "timeout_secs": 1
            }),
        )
        .await;

    let elapsed = start.elapsed();
    assert!(res.is_err(), "Command should fail with timeout");
    assert!(
        res.unwrap_err().contains("timed out"),
        "Error message should mention timeout"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(2500),
        "Timeout must complete promptly without hanging, took {:?}",
        elapsed
    );

    // Verify subsequent tool call immediately executes without thread pool starvation
    let post_res = executor
        .execute(
            "exec_cmd",
            serde_json::json!({ "command": "echo post_timeout_success" }),
        )
        .await;
    assert!(post_res.is_ok(), "Subsequent call must succeed immediately: {:?}", post_res.err());
    let val = post_res.unwrap();
    assert!(
        val.get("stdout")
            .and_then(|s| s.as_str())
            .unwrap()
            .contains("post_timeout_success"),
        "Expected output from subsequent command"
    );
}
