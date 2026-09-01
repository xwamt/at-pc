//! End-to-End integration tests for at-pc Client/Server (C/S) Architecture.
//! Validates dynamic multi-terminal registration, session routing, tool forwarding,
//! explicit terminal_id overrides, offline detection, and MCP JSON-RPC protocol.

use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::ws_client::AgentWsClient;
use at_pc_protocol::models::{TerminalInfo, TerminalStatus};
use at_pc_server::config::ServerConfig;
use at_pc_server::mcp::handle_jsonrpc_request;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::handler::WsServerState;
use at_pc_server::ws::registry::TerminalRegistry;
use serde_json::json;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

/// Helper to start a test server bound to an ephemeral port with zero port race conditions
async fn spawn_test_server() -> (Arc<TerminalRegistry>, Arc<McpRouter>, String) {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let config = ServerConfig {
        ws_path: "/ws".to_string(),
        heartbeat_interval_secs: 5,
        ..Default::default()
    };
    let state = WsServerState {
        registry: registry.clone(),
        config,
        message_handler: Some(router.clone()),
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind ephemeral listener");
    let local_addr = listener.local_addr().expect("Failed to get local addr");
    let ws_url = format!("ws://{}/ws", local_addr);

    tokio::spawn(async move {
        let _ = at_pc_server::ws::start_ws_server_with_listener(state, listener).await;
    });
    sleep(Duration::from_millis(50)).await;

    (registry, router, ws_url)
}

#[tokio::test]
async fn test_full_cs_registration_and_mcp_routing() {
    let (_registry, router, ws_url) = spawn_test_server().await;

    // 1. Start Agent 1
    let agent1 = Arc::new(AgentWsClient::new(
        ws_url.clone(),
        TerminalInfo {
            terminal_id: "pc-agent-1".to_string(),
            hostname: "PC-ALPHA".to_string(),
            username: "alice".to_string(),
            lan_ip: "192.168.1.101".to_string(),
            os_version: "Windows 11".to_string(),
            agent_version: "0.3.0".to_string(),
        },
        Arc::new(AgentExecutor::new()),
    ));
    let agent1_clone = agent1.clone();
    tokio::spawn(async move {
        agent1_clone.run().await;
    });

    // 2. Start Agent 2
    let agent2 = Arc::new(AgentWsClient::new(
        ws_url.clone(),
        TerminalInfo {
            terminal_id: "pc-agent-2".to_string(),
            hostname: "PC-BETA".to_string(),
            username: "bob".to_string(),
            lan_ip: "192.168.1.102".to_string(),
            os_version: "Windows 10".to_string(),
            agent_version: "0.3.0".to_string(),
        },
        Arc::new(AgentExecutor::new()),
    ));
    let agent2_clone = agent2.clone();
    tokio::spawn(async move {
        agent2_clone.run().await;
    });

    // Wait for both agents to connect and register
    let mut registered = false;
    for _ in 0..30 {
        sleep(Duration::from_millis(100)).await;
        if router.list_terminals().await.len() == 2 {
            registered = true;
            break;
        }
    }
    assert!(registered, "Timed out waiting for both agents to register");

    // 3. Dynamic discovery via list_terminals()
    let list = router.list_terminals().await;
    assert_eq!(list.len(), 2);
    let ids: Vec<String> = list.iter().map(|t| t.info.terminal_id.clone()).collect();
    assert!(ids.contains(&"pc-agent-1".to_string()));
    assert!(ids.contains(&"pc-agent-2".to_string()));

    // 4. Test session targeting via select_terminal()
    // Select Agent 1
    let sel_res = router.select_terminal("pc-agent-1").await;
    assert!(sel_res.is_ok());
    assert_eq!(router.get_active_terminal_id().await, Some("pc-agent-1".to_string()));

    // Dispatch get_system_overview without explicit terminal_id -> routes to Agent 1
    let overview_res = router
        .dispatch_tool_call("get_system_overview", json!({}))
        .await
        .expect("get_system_overview failed on selected agent 1");
    assert!(
        overview_res.get("cpu").is_some() || overview_res.get("os_name").is_some(),
        "Unexpected overview payload: {:?}",
        overview_res
    );

    // Dispatch exec_cmd without explicit terminal_id -> routes to Agent 1
    let cmd_res = router
        .dispatch_tool_call("exec_cmd", json!({"command": "echo hello_from_agent1"}))
        .await
        .expect("exec_cmd failed on agent 1");
    let stdout = cmd_res.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
    assert!(stdout.contains("hello_from_agent1"), "Expected echo output, got: {}", stdout);

    // Switch session to Agent 2
    let sel_res2 = router.select_terminal("pc-agent-2").await;
    assert!(sel_res2.is_ok());
    assert_eq!(router.get_active_terminal_id().await, Some("pc-agent-2".to_string()));

    // Dispatch tool to Agent 2
    let cmd_res2 = router
        .dispatch_tool_call("exec_cmd", json!({"command": "echo hello_from_agent2"}))
        .await
        .expect("exec_cmd failed on agent 2");
    let stdout2 = cmd_res2.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
    assert!(stdout2.contains("hello_from_agent2"), "Expected echo output, got: {}", stdout2);

    // Clean up
    agent1.disconnect("test completed").await;
    agent2.disconnect("test completed").await;
}

#[tokio::test]
async fn test_explicit_terminal_id_override() {
    let (_registry, router, ws_url) = spawn_test_server().await;

    let agent1 = Arc::new(AgentWsClient::new(
        ws_url.clone(),
        TerminalInfo {
            terminal_id: "agent-alpha".to_string(),
            hostname: "ALPHA-NODE".to_string(),
            username: "user1".to_string(),
            lan_ip: "10.0.0.1".to_string(),
            os_version: "Linux".to_string(),
            agent_version: "0.3.0".to_string(),
        },
        Arc::new(AgentExecutor::new()),
    ));
    let a1_clone = agent1.clone();
    tokio::spawn(async move { a1_clone.run().await; });

    let agent2 = Arc::new(AgentWsClient::new(
        ws_url.clone(),
        TerminalInfo {
            terminal_id: "agent-beta".to_string(),
            hostname: "BETA-NODE".to_string(),
            username: "user2".to_string(),
            lan_ip: "10.0.0.2".to_string(),
            os_version: "Linux".to_string(),
            agent_version: "0.3.0".to_string(),
        },
        Arc::new(AgentExecutor::new()),
    ));
    let a2_clone = agent2.clone();
    tokio::spawn(async move { a2_clone.run().await; });

    // Wait for both to register
    for _ in 0..30 {
        sleep(Duration::from_millis(100)).await;
        if router.list_terminals().await.len() == 2 {
            break;
        }
    }

    // Set active session terminal to agent-beta
    router.select_terminal("agent-beta").await.unwrap();
    assert_eq!(router.get_active_terminal_id().await, Some("agent-beta".to_string()));

    // Explicit override to agent-alpha via tool arguments
    let res = router
        .dispatch_tool_call(
            "exec_cmd",
            json!({
                "terminal_id": "agent-alpha",
                "command": "echo override_to_alpha"
            }),
        )
        .await
        .expect("Explicit override tool dispatch failed");

    let stdout = res.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
    assert!(stdout.contains("override_to_alpha"));

    // Direct invocation via invoke_tool
    let direct_res = router
        .invoke_tool(
            "agent-alpha",
            "exec_cmd",
            json!({"command": "echo direct_invoke"}),
            10,
        )
        .await
        .expect("Direct invoke_tool failed");
    let direct_stdout = direct_res.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        direct_stdout.contains("direct_invoke"),
        "direct_stdout did not contain direct_invoke: {:?}",
        direct_res
    );

    agent1.disconnect("test completed").await;
    agent2.disconnect("test completed").await;
}

#[tokio::test]
async fn test_offline_detection_and_agent_disconnect() {
    let (_registry, router, ws_url) = spawn_test_server().await;

    let agent_online = Arc::new(AgentWsClient::new(
        ws_url.clone(),
        TerminalInfo {
            terminal_id: "agent-stay-online".to_string(),
            hostname: "PERSISTENT-HOST".to_string(),
            username: "admin".to_string(),
            lan_ip: "10.0.0.10".to_string(),
            os_version: "macOS".to_string(),
            agent_version: "0.3.0".to_string(),
        },
        Arc::new(AgentExecutor::new()),
    ));
    let a_on = agent_online.clone();
    tokio::spawn(async move { a_on.run().await; });

    let agent_offline = Arc::new(AgentWsClient::new(
        ws_url.clone(),
        TerminalInfo {
            terminal_id: "agent-to-disconnect".to_string(),
            hostname: "TRANSIENT-HOST".to_string(),
            username: "guest".to_string(),
            lan_ip: "10.0.0.20".to_string(),
            os_version: "macOS".to_string(),
            agent_version: "0.3.0".to_string(),
        },
        Arc::new(AgentExecutor::new()),
    ));
    let a_off = agent_offline.clone();
    tokio::spawn(async move { a_off.run().await; });

    // Wait for both to register
    for _ in 0..30 {
        sleep(Duration::from_millis(100)).await;
        if router.list_terminals().await.len() == 2 {
            break;
        }
    }

    assert_eq!(router.list_terminals().await.len(), 2);

    // Disconnect agent_offline gracefully
    agent_offline.disconnect("Graceful test shutdown").await;
    sleep(Duration::from_millis(300)).await;

    // Verify registry reflects disconnect or removal
    let term = router.get_terminal("agent-to-disconnect").await;
    let is_offline_or_removed = match term {
        Some(t) => t.status == TerminalStatus::Offline,
        None => true,
    };
    assert!(is_offline_or_removed, "Expected disconnected agent to be offline or removed");

    // Tool invocation to disconnected agent must fail
    let call_res = router
        .invoke_tool(
            "agent-to-disconnect",
            "get_system_overview",
            json!({}),
            5,
        )
        .await;
    assert!(call_res.is_err(), "Tool invocation on offline agent should fail");

    // Tool invocation to agent_online continues to succeed
    let online_res = router
        .invoke_tool(
            "agent-stay-online",
            "exec_cmd",
            json!({"command": "echo still_alive"}),
            5,
        )
        .await
        .expect("Tool call on online agent should succeed");
    let stdout = online_res.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
    assert!(stdout.contains("still_alive"));

    agent_online.disconnect("test completed").await;
}

#[tokio::test]
async fn test_mcp_jsonrpc_protocol_flow_e2e() {
    let (_registry, router, ws_url) = spawn_test_server().await;

    let agent = Arc::new(AgentWsClient::new(
        ws_url.clone(),
        TerminalInfo {
            terminal_id: "e2e-mcp-agent".to_string(),
            hostname: "MCP-NODE-01".to_string(),
            username: "runner".to_string(),
            lan_ip: "10.1.1.50".to_string(),
            os_version: "macOS".to_string(),
            agent_version: "0.3.0".to_string(),
        },
        Arc::new(AgentExecutor::new()),
    ));
    let agent_clone = agent.clone();
    tokio::spawn(async move { agent_clone.run().await; });

    // Wait for agent to connect
    for _ in 0..30 {
        sleep(Duration::from_millis(100)).await;
        if router.list_terminals().await.len() == 1 {
            break;
        }
    }

    // 1. JSON-RPC 'initialize'
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    let init_resp = handle_jsonrpc_request(&router, &init_req).await.expect("initialize failed");
    assert_eq!(init_resp.get("id").and_then(|v| v.as_i64()), Some(1));
    assert_eq!(
        init_resp["result"]["serverInfo"]["name"].as_str(),
        Some("at-pc-server")
    );

    // 2. JSON-RPC 'tools/list'
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    let list_resp = handle_jsonrpc_request(&router, &list_req).await.expect("tools/list failed");
    let tools = list_resp["result"]["tools"].as_array().expect("tools must be array");
    let tool_names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
        .collect();
    assert!(tool_names.contains(&"list_terminals"));
    assert!(tool_names.contains(&"select_terminal"));
    assert!(tool_names.contains(&"get_system_overview"));
    assert!(tool_names.contains(&"exec_cmd"));

    // 3. JSON-RPC 'tools/call' -> 'list_terminals'
    let call_list_req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "list_terminals",
            "arguments": {}
        }
    });
    let call_list_resp = handle_jsonrpc_request(&router, &call_list_req).await.expect("tools/call list_terminals failed");
    let content_text = call_list_resp["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(content_text.contains("e2e-mcp-agent"));

    // 4. JSON-RPC 'tools/call' -> 'select_terminal'
    let call_select_req = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "select_terminal",
            "arguments": {
                "terminal_id": "e2e-mcp-agent"
            }
        }
    });
    let call_select_resp = handle_jsonrpc_request(&router, &call_select_req).await.expect("tools/call select_terminal failed");
    assert_eq!(call_select_resp["result"]["isError"].as_bool(), Some(false));

    // 5. JSON-RPC 'tools/call' -> forwarded 'exec_cmd'
    let call_exec_req = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "exec_cmd",
            "arguments": {
                "command": "echo mcp_rpc_success"
            }
        }
    });
    let call_exec_resp = handle_jsonrpc_request(&router, &call_exec_req).await.expect("tools/call exec_cmd failed");
    let exec_out = call_exec_resp["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(exec_out.contains("mcp_rpc_success"));

    agent.disconnect("test completed").await;
}

#[tokio::test]
async fn test_concurrent_multi_terminal_tool_invocations() {
    let (_registry, router, ws_url) = spawn_test_server().await;

    let agent1 = Arc::new(AgentWsClient::new(
        ws_url.clone(),
        TerminalInfo {
            terminal_id: "concurrent-agent-1".to_string(),
            hostname: "CONCURRENT-1".to_string(),
            username: "worker1".to_string(),
            lan_ip: "10.2.2.1".to_string(),
            os_version: "Linux".to_string(),
            agent_version: "0.3.0".to_string(),
        },
        Arc::new(AgentExecutor::new()),
    ));
    let a1 = agent1.clone();
    tokio::spawn(async move { a1.run().await; });

    let agent2 = Arc::new(AgentWsClient::new(
        ws_url.clone(),
        TerminalInfo {
            terminal_id: "concurrent-agent-2".to_string(),
            hostname: "CONCURRENT-2".to_string(),
            username: "worker2".to_string(),
            lan_ip: "10.2.2.2".to_string(),
            os_version: "Linux".to_string(),
            agent_version: "0.3.0".to_string(),
        },
        Arc::new(AgentExecutor::new()),
    ));
    let a2 = agent2.clone();
    tokio::spawn(async move { a2.run().await; });

    // Wait for both to register
    for _ in 0..30 {
        sleep(Duration::from_millis(100)).await;
        if router.list_terminals().await.len() == 2 {
            break;
        }
    }

    // Run parallel tool calls to both agents
    let router1 = router.clone();
    let router2 = router.clone();

    let fut1 = tokio::spawn(async move {
        router1
            .invoke_tool(
                "concurrent-agent-1",
                "exec_cmd",
                json!({"command": "echo conc_result_1"}),
                10,
            )
            .await
    });

    let fut2 = tokio::spawn(async move {
        router2
            .invoke_tool(
                "concurrent-agent-2",
                "exec_cmd",
                json!({"command": "echo conc_result_2"}),
                10,
            )
            .await
    });

    let (res1, res2) = tokio::join!(fut1, fut2);
    let val1 = res1.unwrap().expect("Agent 1 concurrent execution failed");
    let val2 = res2.unwrap().expect("Agent 2 concurrent execution failed");

    let stdout1 = val1.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
    let stdout2 = val2.get("stdout").and_then(|v| v.as_str()).unwrap_or("");

    assert!(stdout1.contains("conc_result_1"));
    assert!(stdout2.contains("conc_result_2"));

    agent1.disconnect("test completed").await;
    agent2.disconnect("test completed").await;
}
