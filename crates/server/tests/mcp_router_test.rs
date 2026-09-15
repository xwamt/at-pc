use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::TerminalInfo;
use at_pc_server::mcp::handle_jsonrpc_request;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::registry::TerminalRegistry;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_route_tool_to_target_terminal() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let (tx, mut rx) = mpsc::unbounded_channel();

    let info = TerminalInfo {
        terminal_id: "agent-007".to_string(),
        hostname: "PC-007".to_string(),
        username: "bond".to_string(),
        lan_ip: "192.168.1.50".to_string(),
        os_version: "Windows 11".to_string(),
        agent_version: "0.3.0".to_string(),
    };
    registry.register(info, tx).await;

    // Simulate router forwarding
    let router_clone = router.clone();
    tokio::spawn(async move {
        if let Some(ServerToAgentMessage::InvokeTool { call_id, tool_name, .. }) = rx.recv().await {
            assert_eq!(tool_name, "exec_powershell");
            router_clone.handle_tool_result(AgentToServerMessage::ToolResult {
                call_id,
                success: true,
                result: serde_json::json!({ "stdout": "hello", "exit_code": 0 }),
                error: None,
                duration_ms: 50,
            }).await;
        }
    });

    let res = router.invoke_tool("agent-007", "exec_powershell", serde_json::json!({"script": "echo hello"}), 5).await.unwrap();
    assert_eq!(res["stdout"], "hello");
}

#[tokio::test]
async fn test_select_terminal_and_session_memory() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let (tx1, _rx1) = mpsc::unbounded_channel();
    let (tx2, _rx2) = mpsc::unbounded_channel();

    let info1 = TerminalInfo {
        terminal_id: "pc-1".to_string(),
        hostname: "PC-ALPHA".to_string(),
        username: "user1".to_string(),
        lan_ip: "192.168.1.101".to_string(),
        os_version: "Windows 11".to_string(),
        agent_version: "0.3.0".to_string(),
    };
    let info2 = TerminalInfo {
        terminal_id: "pc-2".to_string(),
        hostname: "PC-BETA".to_string(),
        username: "user2".to_string(),
        lan_ip: "192.168.1.102".to_string(),
        os_version: "Windows 10".to_string(),
        agent_version: "0.3.0".to_string(),
    };
    registry.register(info1, tx1).await;
    registry.register(info2, tx2).await;

    // Initially no active terminal
    assert_eq!(router.get_active_terminal_id().await, None);
    assert!(router.get_active_terminal().await.is_none());

    // Select pc-2
    let res = router.select_terminal("pc-2").await;
    assert!(res.is_ok());
    let entry = res.unwrap();
    assert_eq!(entry.info.terminal_id, "pc-2");
    assert_eq!(router.get_active_terminal_id().await, Some("pc-2".to_string()));

    let active = router.get_active_terminal().await.unwrap();
    assert_eq!(active.info.terminal_id, "pc-2");

    // Select non-existent terminal
    let res_err = router.select_terminal("non-existent").await;
    assert!(res_err.is_err());
}

#[tokio::test]
async fn test_session_scoped_active_terminal() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let (tx1, _rx1) = mpsc::unbounded_channel();
    let (tx2, _rx2) = mpsc::unbounded_channel();

    let info1 = TerminalInfo {
        terminal_id: "agent-1".to_string(),
        hostname: "HOST-1".to_string(),
        username: "user1".to_string(),
        lan_ip: "192.168.1.10".to_string(),
        os_version: "Windows 11".to_string(),
        agent_version: "0.3.0".to_string(),
    };
    let info2 = TerminalInfo {
        terminal_id: "agent-2".to_string(),
        hostname: "HOST-2".to_string(),
        username: "user2".to_string(),
        lan_ip: "192.168.1.20".to_string(),
        os_version: "Windows 10".to_string(),
        agent_version: "0.3.0".to_string(),
    };
    registry.register(info1, tx1).await;
    registry.register(info2, tx2).await;

    // Session A selects agent-1, Session B selects agent-2
    assert!(router.select_terminal_for_session("session-a", "agent-1").await.is_ok());
    assert!(router.select_terminal_for_session("session-b", "agent-2").await.is_ok());

    assert_eq!(
        router.get_active_terminal_id_for_session(Some("session-a")).await,
        Some("agent-1".to_string())
    );
    assert_eq!(
        router.get_active_terminal_id_for_session(Some("session-b")).await,
        Some("agent-2".to_string())
    );

    // Target resolution respects session
    assert_eq!(
        router.resolve_target_terminal_with_session(None, Some("session-a")).await.unwrap(),
        "agent-1"
    );
    assert_eq!(
        router.resolve_target_terminal_with_session(None, Some("session-b")).await.unwrap(),
        "agent-2"
    );

    // Explicit override takes precedence over session
    assert_eq!(
        router.resolve_target_terminal_with_session(Some("agent-2"), Some("session-a")).await.unwrap(),
        "agent-2"
    );
}

#[tokio::test]
async fn test_dispatch_tool_call_meta_tools() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let (tx, _rx) = mpsc::unbounded_channel();

    let info = TerminalInfo {
        terminal_id: "pc-test".to_string(),
        hostname: "TEST-HOST".to_string(),
        username: "admin".to_string(),
        lan_ip: "10.0.0.5".to_string(),
        os_version: "Windows 11".to_string(),
        agent_version: "0.3.0".to_string(),
    };
    registry.register(info, tx).await;

    // 1. list_terminals
    let list_res = router.dispatch_tool_call("list_terminals", json!({})).await.unwrap();
    let arr = list_res.as_array().expect("expected array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["info"]["terminal_id"], "pc-test");

    // 2. select_terminal
    let select_res = router.dispatch_tool_call("select_terminal", json!({ "terminal_id": "pc-test" })).await.unwrap();
    assert_eq!(select_res["info"]["terminal_id"], "pc-test");

    // 3. get_active_terminal
    let active_res = router.dispatch_tool_call("get_active_terminal", json!({})).await.unwrap();
    assert_eq!(active_res["info"]["terminal_id"], "pc-test");
}

#[tokio::test]
async fn test_dispatch_tool_call_routing_and_fallback() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let (tx, mut rx) = mpsc::unbounded_channel();

    let info = TerminalInfo {
        terminal_id: "solo-node".to_string(),
        hostname: "SOLO".to_string(),
        username: "admin".to_string(),
        lan_ip: "10.0.0.10".to_string(),
        os_version: "Windows 11".to_string(),
        agent_version: "0.3.0".to_string(),
    };
    registry.register(info, tx).await;

    let router_clone = router.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let ServerToAgentMessage::InvokeTool { call_id, tool_name, .. } = msg {
                if tool_name == "get_system_overview" {
                    router_clone.handle_tool_result(AgentToServerMessage::ToolResult {
                        call_id,
                        success: true,
                        result: json!({ "os": "Windows 11", "cpu_cores": 8 }),
                        error: None,
                        duration_ms: 10,
                    }).await;
                }
            }
        }
    });

    // Single online node should auto fallback even if no active_terminal_id is selected
    let res = router.dispatch_tool_call("get_system_overview", json!({})).await.unwrap();
    assert_eq!(res["cpu_cores"], 8);

    // Explicit terminal_id in arguments
    let res_explicit = router.dispatch_tool_call("get_system_overview", json!({ "terminal_id": "solo-node" })).await.unwrap();
    assert_eq!(res_explicit["cpu_cores"], 8);
}

#[tokio::test]
async fn test_invoke_tool_timeout() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let (tx, mut rx) = mpsc::unbounded_channel();

    let info = TerminalInfo {
        terminal_id: "slow-node".to_string(),
        hostname: "SLOW".to_string(),
        username: "user".to_string(),
        lan_ip: "10.0.0.20".to_string(),
        os_version: "Windows 11".to_string(),
        agent_version: "0.3.0".to_string(),
    };
    registry.register(info, tx).await;

    // Do not respond in rx
    tokio::spawn(async move {
        while let Some(_msg) = rx.recv().await {
            // Never reply
        }
    });

    let res = router.invoke_tool("slow-node", "exec_cmd", json!({ "command": "sleep 10" }), 1).await;
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("timed out"));
}

#[tokio::test]
async fn test_mcp_jsonrpc_protocol_flow() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let (tx, mut rx) = mpsc::unbounded_channel();

    let info = TerminalInfo {
        terminal_id: "mcp-agent".to_string(),
        hostname: "MCP-HOST".to_string(),
        username: "tester".to_string(),
        lan_ip: "192.168.1.200".to_string(),
        os_version: "Windows 11".to_string(),
        agent_version: "0.3.0".to_string(),
    };
    registry.register(info, tx).await;

    let router_clone = router.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let ServerToAgentMessage::InvokeTool { call_id, tool_name, .. } = msg {
                if tool_name == "exec_powershell" {
                    router_clone.handle_tool_result(AgentToServerMessage::ToolResult {
                        call_id,
                        success: true,
                        result: json!({ "stdout": "powershell output", "exit_code": 0 }),
                        error: None,
                        duration_ms: 15,
                    }).await;
                }
            }
        }
    });

    // 1. initialize
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    let init_resp = handle_jsonrpc_request(&router, &init_req).await.unwrap();
    assert_eq!(init_resp["id"], 1);
    assert_eq!(init_resp["result"]["serverInfo"]["name"], "at-pc-server");

    // 2. tools/list
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    let list_resp = handle_jsonrpc_request(&router, &list_req).await.unwrap();
    let tools = list_resp["result"]["tools"].as_array().unwrap();
    assert!(tools.len() >= 12); // 3 server meta tools + 9 diagnostic tools

    // 3. tools/call list_terminals
    let call_list_req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "list_terminals",
            "arguments": {}
        }
    });
    let call_list_resp = handle_jsonrpc_request(&router, &call_list_req).await.unwrap();
    assert_eq!(call_list_resp["result"]["isError"], false);

    // 4. tools/call exec_powershell
    let call_ps_req = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "exec_powershell",
            "arguments": {
                "terminal_id": "mcp-agent",
                "script": "Get-Date"
            }
        }
    });
    let call_ps_resp = handle_jsonrpc_request(&router, &call_ps_req).await.unwrap();
    assert_eq!(call_ps_resp["result"]["isError"], false);
    let text = call_ps_resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("powershell output"));

    // 5. tools/call capture_screen (MCP Image Format Verification)
    let (tx_screen, mut rx_screen) = mpsc::unbounded_channel();
    registry.register(TerminalInfo {
        terminal_id: "screen-agent".to_string(),
        hostname: "HOST-SCREEN".to_string(),
        username: "user".to_string(),
        lan_ip: "192.168.1.199".to_string(),
        os_version: "macOS".to_string(),
        agent_version: "1.0.0".to_string(),
    }, tx_screen).await;

    let router_for_screen = router.clone();
    tokio::spawn(async move {
        if let Some(ServerToAgentMessage::InvokeTool { call_id, tool_name, .. }) = rx_screen.recv().await {
            assert_eq!(tool_name, "capture_screen");
            router_for_screen.handle_tool_result(AgentToServerMessage::ToolResult {
                call_id,
                success: true,
                result: json!({
                    "display_index": 0,
                    "width": 1920,
                    "height": 1080,
                    "format": "jpeg",
                    "base64_data": "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAYEBQYFBAYEBQUFBAYFBh...",
                    "raw_base64": "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAYEBQYFBAYEBQUFBAYFBh...",
                    "file_path": "/tmp/test_screen.jpg"
                }),
                error: None,
                duration_ms: 120,
            }).await;
        }
    });

    let call_screen_req = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "capture_screen",
            "arguments": {
                "terminal_id": "screen-agent"
            }
        }
    });
    let call_screen_resp = handle_jsonrpc_request(&router, &call_screen_req).await.unwrap();
    assert_eq!(call_screen_resp["result"]["isError"], false);
    let contents = call_screen_resp["result"]["content"].as_array().unwrap();
    assert_eq!(contents.len(), 2);
    // Content 0: text summary
    assert_eq!(contents[0]["type"], "text");
    assert!(contents[0]["text"].as_str().unwrap().contains("1920x1080"));
    assert!(contents[0]["text"].as_str().unwrap().contains("/tmp/test_screen.jpg"));
    // Content 1: MCP image
    assert_eq!(contents[1]["type"], "image");
    assert_eq!(contents[1]["mimeType"], "image/jpeg");
    let img_data = contents[1]["data"].as_str().unwrap();
    assert!(!img_data.starts_with("data:"));
    assert!(img_data.starts_with("/9j/"));
}

#[tokio::test]
async fn test_capture_screen_server_save_path_and_image_base64() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));

    let (tx_screen, mut rx_screen) = mpsc::unbounded_channel();
    registry
        .register(
            TerminalInfo {
                terminal_id: "screen-agent-save".to_string(),
                hostname: "HOST-SAVE".to_string(),
                username: "user".to_string(),
                lan_ip: "192.168.1.200".to_string(),
                os_version: "Windows 11".to_string(),
                agent_version: "1.0.0".to_string(),
            },
            tx_screen,
        )
        .await;

    let valid_jpeg_b64 = "/9j/4AAQSkZJRgABAQEASABIAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////wgALCAABAAEBAREA/8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPxA=";

    let router_for_screen = router.clone();
    tokio::spawn(async move {
        if let Some(ServerToAgentMessage::InvokeTool {
            call_id,
            tool_name,
            ..
        }) = rx_screen.recv().await
        {
            assert_eq!(tool_name, "capture_screen");
            router_for_screen
                .handle_tool_result(AgentToServerMessage::ToolResult {
                    call_id,
                    success: true,
                    result: json!({
                        "display_index": 0,
                        "width": 1,
                        "height": 1,
                        "format": "jpeg",
                        "base64_data": format!("data:image/jpeg;base64,{}", valid_jpeg_b64),
                        "raw_base64": valid_jpeg_b64,
                        "image_base64": valid_jpeg_b64,
                        "data_uri": format!("data:image/jpeg;base64,{}", valid_jpeg_b64),
                    }),
                    error: None,
                    duration_ms: 50,
                })
                .await;
        }
    });

    let temp_save_file = std::env::temp_dir().join("at_pc_test_server_save.jpg");
    let temp_save_str = temp_save_file.to_string_lossy().to_string();

    let call_screen_req = json!({
        "jsonrpc": "2.0",
        "id": 101,
        "method": "tools/call",
        "params": {
            "name": "capture_screen",
            "arguments": {
                "terminal_id": "screen-agent-save",
                "server_save_path": temp_save_str
            }
        }
    });

    let call_screen_resp = handle_jsonrpc_request(&router, &call_screen_req).await.unwrap();
    assert_eq!(call_screen_resp["result"]["isError"], false);
    let contents = call_screen_resp["result"]["content"].as_array().unwrap();
    assert_eq!(contents.len(), 2);
    assert!(contents[0]["text"].as_str().unwrap().contains("Saved to server host disk"));

    assert!(temp_save_file.exists());
    let written = std::fs::read(&temp_save_file).unwrap();
    assert!(written.starts_with(&[0xff, 0xd8, 0xff]));
    let _ = std::fs::remove_file(temp_save_file);
}
