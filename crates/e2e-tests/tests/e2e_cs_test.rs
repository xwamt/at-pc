//! End-to-End integration tests for at-pc Client/Server (C/S) Architecture.
//! Validates dynamic multi-terminal registration, session routing, tool forwarding,
//! explicit terminal_id overrides, offline detection, and MCP JSON-RPC protocol.
//! Uses in-memory duplex streams to execute safely in any CI/sandbox environment
//! without requiring OS TCP port binding permissions.

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

/// Helper harness that links server and agent via an in-memory duplex channel.
struct TestCluster {
    #[allow(dead_code)]
    pub registry: Arc<TerminalRegistry>,
    pub router: Arc<McpRouter>,
    pub server_state: WsServerState,
}

impl TestCluster {
    fn new() -> Self {
        let registry = Arc::new(TerminalRegistry::new());
        let router = Arc::new(McpRouter::new(registry.clone()));
        let config = ServerConfig {
            ws_path: "/ws".to_string(),
            heartbeat_interval_secs: 5,
            ..Default::default()
        };
        let server_state = WsServerState {
            registry: registry.clone(),
            config,
            message_handler: Some(router.clone()),
        };
        Self {
            registry,
            router,
            server_state,
        }
    }

    /// Attaches an agent to the test cluster via a full in-memory duplex stream.
    /// Performs client and server HTTP/WebSocket upgrades and message loops.
    fn attach_agent(&self, agent: Arc<AgentWsClient>) {
        let state = self.server_state.clone();
        let (client_stream, server_stream) = tokio::io::duplex(32768);

        tokio::spawn(async move {
            at_pc_server::ws::handler::handle_stream(server_stream, state).await;
        });

        tokio::spawn(async move {
            let _ = agent
                .handshake_and_run_stream(client_stream, "localhost", "/ws")
                .await;
        });
    }
}

#[tokio::test]
async fn test_full_cs_registration_and_mcp_routing() {
    let cluster = TestCluster::new();
    let router = cluster.router.clone();

    // 1. Start Agent 1
    let agent1 = Arc::new(AgentWsClient::new(
        "ws://localhost/ws".to_string(),
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
    cluster.attach_agent(agent1.clone());

    // 2. Start Agent 2
    let agent2 = Arc::new(AgentWsClient::new(
        "ws://localhost/ws".to_string(),
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
    cluster.attach_agent(agent2.clone());

    // Wait for both agents to register in registry
    let mut online = false;
    for _ in 0..40 {
        sleep(Duration::from_millis(50)).await;
        let terms = router.list_terminals().await;
        if terms.len() == 2 && terms.iter().all(|t| t.status == TerminalStatus::Online) {
            online = true;
            break;
        }
    }
    assert!(online, "Both agents failed to register as online");

    // Initially with 2 online agents and no active selection, dispatching without terminal_id should fail
    let err_res = router
        .dispatch_tool_call("exec_cmd", json!({"command": "whoami"}))
        .await;
    assert!(err_res.is_err());
    assert!(err_res.unwrap_err().contains("Multiple terminals online"));

    // Select Agent 1 as active terminal
    let sel_res = router.select_terminal("pc-agent-1").await;
    assert!(sel_res.is_ok());
    assert_eq!(
        router.get_active_terminal_id().await,
        Some("pc-agent-1".to_string())
    );

    // Dispatch exec_cmd without explicit terminal_id -> routes to Agent 1
    let cmd_res = router
        .dispatch_tool_call("exec_cmd", json!({"command": "echo hello_from_agent1"}))
        .await
        .expect("exec_cmd failed on agent 1");
    let stdout = cmd_res.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        stdout.contains("hello_from_agent1"),
        "Expected echo output, got: {}",
        stdout
    );

    // Switch session to Agent 2
    let sel_res2 = router.select_terminal("pc-agent-2").await;
    assert!(sel_res2.is_ok());
    assert_eq!(
        router.get_active_terminal_id().await,
        Some("pc-agent-2".to_string())
    );

    // Dispatch tool to Agent 2
    let cmd_res2 = router
        .dispatch_tool_call("exec_cmd", json!({"command": "echo hello_from_agent2"}))
        .await
        .expect("exec_cmd failed on agent 2");
    let stdout2 = cmd_res2
        .get("stdout")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        stdout2.contains("hello_from_agent2"),
        "Expected echo output, got: {}",
        stdout2
    );

    // Clean up
    agent1.disconnect("test completed").await;
    agent2.disconnect("test completed").await;
}

#[tokio::test]
async fn test_explicit_terminal_id_override() {
    let cluster = TestCluster::new();
    let router = cluster.router.clone();

    let agent1 = Arc::new(AgentWsClient::new(
        "ws://localhost/ws".to_string(),
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
    cluster.attach_agent(agent1.clone());

    let agent2 = Arc::new(AgentWsClient::new(
        "ws://localhost/ws".to_string(),
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
    cluster.attach_agent(agent2.clone());

    // Wait for both to register
    for _ in 0..40 {
        sleep(Duration::from_millis(50)).await;
        if router.list_terminals().await.len() == 2 {
            break;
        }
    }

    // Set active session terminal to agent-beta
    router.select_terminal("agent-beta").await.unwrap();
    assert_eq!(
        router.get_active_terminal_id().await,
        Some("agent-beta".to_string())
    );

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
    let direct_stdout = direct_res
        .get("stdout")
        .and_then(|v| v.as_str())
        .unwrap_or("");
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
    let cluster = TestCluster::new();
    let router = cluster.router.clone();

    let agent_online = Arc::new(AgentWsClient::new(
        "ws://localhost/ws".to_string(),
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
    cluster.attach_agent(agent_online.clone());

    let agent_offline = Arc::new(AgentWsClient::new(
        "ws://localhost/ws".to_string(),
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
    cluster.attach_agent(agent_offline.clone());

    // Wait for both to register
    for _ in 0..40 {
        sleep(Duration::from_millis(50)).await;
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
    assert!(
        is_offline_or_removed,
        "Expected disconnected agent to be offline or removed"
    );

    // Tool invocation to disconnected agent must fail
    let call_res = router
        .invoke_tool("agent-to-disconnect", "get_system_overview", json!({}), 5)
        .await;
    assert!(
        call_res.is_err(),
        "Tool invocation on offline agent should fail"
    );

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
    let stdout = online_res
        .get("stdout")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(stdout.contains("still_alive"));

    agent_online.disconnect("test completed").await;
}

#[tokio::test]
async fn test_mcp_jsonrpc_protocol_flow_e2e() {
    let cluster = TestCluster::new();
    let router = cluster.router.clone();

    let agent = Arc::new(AgentWsClient::new(
        "ws://localhost/ws".to_string(),
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
    cluster.attach_agent(agent.clone());

    // Wait for agent to connect
    for _ in 0..40 {
        sleep(Duration::from_millis(50)).await;
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
    let init_resp = handle_jsonrpc_request(&router, &init_req)
        .await
        .expect("initialize failed");
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
    let list_resp = handle_jsonrpc_request(&router, &list_req)
        .await
        .expect("tools/list failed");
    let tools = list_resp["result"]["tools"]
        .as_array()
        .expect("tools must be array");
    let tool_names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
        .collect();
    assert!(tool_names.contains(&"list_terminals"));
    assert!(tool_names.contains(&"select_terminal"));
    assert!(tool_names.contains(&"get_system_overview"));
    assert!(tool_names.contains(&"exec_cmd"));
    assert!(tool_names.contains(&"list_directory"));
    assert!(tool_names.contains(&"search_files"));
    assert!(tool_names.contains(&"test_network"));

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
    let call_list_resp = handle_jsonrpc_request(&router, &call_list_req)
        .await
        .expect("tools/call list_terminals failed");
    let content_text = call_list_resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
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
    let call_select_resp = handle_jsonrpc_request(&router, &call_select_req)
        .await
        .expect("tools/call select_terminal failed");
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
    let call_exec_resp = handle_jsonrpc_request(&router, &call_exec_req)
        .await
        .expect("tools/call exec_cmd failed");
    let exec_out = call_exec_resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    assert!(exec_out.contains("mcp_rpc_success"));

    agent.disconnect("test completed").await;
}

#[tokio::test]
async fn test_concurrent_multi_terminal_tool_invocations() {
    let cluster = TestCluster::new();
    let router = cluster.router.clone();

    let agent1 = Arc::new(AgentWsClient::new(
        "ws://localhost/ws".to_string(),
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
    cluster.attach_agent(agent1.clone());

    let agent2 = Arc::new(AgentWsClient::new(
        "ws://localhost/ws".to_string(),
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
    cluster.attach_agent(agent2.clone());

    // Wait for both to register
    for _ in 0..40 {
        sleep(Duration::from_millis(50)).await;
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
