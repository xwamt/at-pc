use at_pc_agent::app::AgentAppState;
use at_pc_agent::ws_client::{AgentEventListener, ClientConnectionStatus};
use at_pc_protocol::models::TerminalInfo;
use std::sync::Arc;

#[test]
fn test_agent_app_state_audit_stream() {
    let state = Arc::new(AgentAppState::new("ws://127.0.0.1:9801/ws".to_string()));
    state.add_audit_log("exec_powershell", "Get-Process", "SUCCESS");
    let logs = state.get_audit_logs();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].tool_name, "exec_powershell");
    assert_eq!(logs[0].summary, "Get-Process");
    assert_eq!(logs[0].status, "SUCCESS");
}

#[test]
fn test_agent_app_state_listener_events() {
    let state = Arc::new(AgentAppState::new("ws://127.0.0.1:9801/ws".to_string()));
    assert_eq!(state.status(), ClientConnectionStatus::Disconnected);

    // Test on_status_change
    state.on_status_change(ClientConnectionStatus::Connected);
    assert_eq!(state.status(), ClientConnectionStatus::Connected);

    // Test on_tool_start
    let args = serde_json::json!({ "command": "dir" });
    state.on_tool_start("call-1", "exec_cmd", &args);

    let logs = state.get_audit_logs();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].tool_name, "exec_cmd");
    assert_eq!(logs[0].status, "STARTED");

    // Test on_tool_finish
    state.on_tool_finish("call-1", "exec_cmd", true, 42);
    let logs = state.get_audit_logs();
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[1].tool_name, "exec_cmd");
    assert_eq!(logs[1].status, "SUCCESS");
    assert_eq!(logs[1].duration_ms, Some(42));
}

#[test]
fn test_agent_app_state_emergency_disconnect() {
    let state = Arc::new(AgentAppState::new("ws://127.0.0.1:9801/ws".to_string()));
    assert!(!state.is_stopped());

    state.trigger_emergency_disconnect();
    assert!(state.is_stopped());
    assert_eq!(state.status(), ClientConnectionStatus::Disconnected);

    let logs = state.get_audit_logs();
    assert!(logs.iter().any(|l| l.tool_name == "emergency_disconnect"));

    // Reconnect
    state.trigger_reconnect();
    assert!(!state.is_stopped());
    assert_eq!(state.status(), ClientConnectionStatus::Connecting);
    let logs_after = state.get_audit_logs();
    assert!(logs_after.iter().any(|l| l.tool_name == "agent_reconnect"));
}

#[test]
fn test_agent_app_state_terminal_info() {
    let mut info = TerminalInfo {
        terminal_id: "agent-test-1".to_string(),
        hostname: "Test-PC".to_string(),
        username: "admin".to_string(),
        lan_ip: "192.168.1.50".to_string(),
        os_version: "macOS 15.0".to_string(),
        agent_version: "0.3.0".to_string(),
    };

    let state = Arc::new(
        AgentAppState::new("ws://127.0.0.1:9801/ws".to_string())
            .with_terminal_info(info.clone())
    );

    assert_eq!(state.get_terminal_info().terminal_id, "agent-test-1");
    assert_eq!(state.server_url(), "ws://127.0.0.1:9801/ws");

    info.hostname = "Updated-PC".to_string();
    state.set_terminal_info(info);
    assert_eq!(state.get_terminal_info().hostname, "Updated-PC");
}

#[tokio::test]
async fn test_agent_app_state_update_server_url() {
    let state = Arc::new(AgentAppState::new("ws://127.0.0.1:9801/ws".to_string()));

    // 1. Invalid URLs rejected
    assert!(state.update_server_url("".to_string(), false).is_err());
    assert!(state.update_server_url("http://127.0.0.1:9801".to_string(), false).is_err());
    assert!(state.update_server_url("ftp://127.0.0.1".to_string(), false).is_err());
    assert_eq!(state.server_url(), "ws://127.0.0.1:9801/ws");

    // 2. Valid URL accepted
    let new_url = "ws://192.168.1.88:9801/ws".to_string();
    assert!(state.update_server_url(new_url.clone(), false).is_ok());
    assert_eq!(state.server_url(), "ws://192.168.1.88:9801/ws");

    // 3. Audit log contains update event
    let logs = state.get_audit_logs();
    assert!(logs.iter().any(|l| l.tool_name == "server_url_updated"));
}
