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
