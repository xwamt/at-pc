use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::ws_client::{AgentEventListener, AgentWsClient, ClientConnectionStatus};
use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::{TerminalInfo, TerminalStatus};
use at_pc_server::config::ServerConfig;
use at_pc_server::ws::handler::{handle_stream, WsServerState};
use at_pc_server::ws::registry::TerminalRegistry;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
struct TestListener {
    status_changes: AtomicUsize,
    tools_started: AtomicUsize,
    tools_finished: AtomicUsize,
}

impl AgentEventListener for TestListener {
    fn on_status_change(&self, _status: ClientConnectionStatus) {
        self.status_changes.fetch_add(1, Ordering::SeqCst);
    }

    fn on_tool_start(&self, _call_id: &str, _tool_name: &str, _arguments: &serde_json::Value) {
        self.tools_started.fetch_add(1, Ordering::SeqCst);
    }

    fn on_tool_finish(&self, _call_id: &str, _tool_name: &str, _success: bool, _duration_ms: u64) {
        self.tools_finished.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn test_agent_ws_client_handshake_and_registration() {
    let registry = Arc::new(TerminalRegistry::new());
    let server_config = ServerConfig {
        ws_path: "/ws".to_string(),
        auth_token: Some("secret123".to_string()),
        heartbeat_interval_secs: 5,
        ..Default::default()
    };
    let server_state = WsServerState::new(registry.clone(), server_config);

    // In-memory duplex connection
    let (client_stream, server_stream) = tokio::io::duplex(8192);

    let server_task = tokio::spawn(async move {
        handle_stream(server_stream, server_state).await;
    });

    let terminal_info = TerminalInfo {
        terminal_id: "agent-test-01".to_string(),
        hostname: "HOST-01".to_string(),
        username: "testuser".to_string(),
        lan_ip: "192.168.1.100".to_string(),
        os_version: "macOS".to_string(),
        agent_version: "0.3.0".to_string(),
    };

    let listener = Arc::new(TestListener::default());
    let executor = Arc::new(AgentExecutor::new());

    let client = Arc::new(
        AgentWsClient::new(
            "ws://127.0.0.1:9801/ws".to_string(),
            terminal_info.clone(),
            executor,
        )
        .with_auth_token(Some("secret123".to_string()))
        .with_listener(listener.clone()),
    );

    let client_clone = client.clone();
    let client_task = tokio::spawn(async move {
        let _ = client_clone
            .handshake_and_run_stream(client_stream, "127.0.0.1", "/ws")
            .await;
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    // Verify registration succeeded on server registry
    assert_eq!(
        registry.get_status("agent-test-01").await,
        Some(TerminalStatus::Online)
    );
    assert_eq!(client.status().await, ClientConnectionStatus::Connected);
    assert!(listener.status_changes.load(Ordering::SeqCst) >= 1);

    // Gracefully disconnect
    client.disconnect("Test completed").await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(
        registry.get_status("agent-test-01").await,
        Some(TerminalStatus::Offline)
    );

    client_task.abort();
    server_task.abort();
}

#[tokio::test]
async fn test_agent_ws_client_invokes_and_returns_tool_result() {
    let registry = Arc::new(TerminalRegistry::new());
    let tool_result_received = Arc::new(AtomicBool::new(false));
    let tool_result_received_clone = tool_result_received.clone();

    let handler = Arc::new(move |msg: AgentToServerMessage| {
        if let AgentToServerMessage::ToolResult {
            call_id,
            success,
            result,
            ..
        } = msg
        {
            if call_id == "call-test-99" && success && result.get("stdout").is_some() {
                tool_result_received_clone.store(true, Ordering::SeqCst);
            }
        }
    });

    let server_config = ServerConfig {
        ws_path: "/ws".to_string(),
        auth_token: None,
        heartbeat_interval_secs: 5,
        ..Default::default()
    };
    let server_state = WsServerState::with_handler(registry.clone(), server_config, handler);

    let (client_stream, server_stream) = tokio::io::duplex(8192);

    let server_task = tokio::spawn(async move {
        handle_stream(server_stream, server_state).await;
    });

    let terminal_info = TerminalInfo {
        terminal_id: "agent-test-tool".to_string(),
        hostname: "HOST-TOOL".to_string(),
        username: "testuser".to_string(),
        lan_ip: "10.0.0.1".to_string(),
        os_version: "macOS".to_string(),
        agent_version: "0.3.0".to_string(),
    };

    let listener = Arc::new(TestListener::default());
    let executor = Arc::new(AgentExecutor::new());

    let client = Arc::new(
        AgentWsClient::new(
            "ws://127.0.0.1:9801/ws".to_string(),
            terminal_info.clone(),
            executor,
        )
        .with_listener(listener.clone()),
    );

    let client_clone = client.clone();
    let client_task = tokio::spawn(async move {
        let _ = client_clone
            .handshake_and_run_stream(client_stream, "127.0.0.1", "/ws")
            .await;
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    // Send InvokeTool from Server to Agent
    let tx = registry.get_sender("agent-test-tool").await.unwrap();
    let invoke_msg = ServerToAgentMessage::InvokeTool {
        call_id: "call-test-99".to_string(),
        tool_name: "exec_cmd".to_string(),
        arguments: serde_json::json!({ "command": "echo agent_ws_success" }),
        timeout_secs: 10,
    };
    tx.send(invoke_msg).unwrap();

    // Wait for tool execution and result transmission
    tokio::time::sleep(Duration::from_millis(250)).await;

    assert!(tool_result_received.load(Ordering::SeqCst));
    assert_eq!(listener.tools_started.load(Ordering::SeqCst), 1);
    assert_eq!(listener.tools_finished.load(Ordering::SeqCst), 1);

    client.disconnect("Done").await;
    client_task.abort();
    server_task.abort();
}
