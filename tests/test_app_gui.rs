use at_pc::app::{GuiState, PcTroubleshooterApp};
use at_pc::server::{AppState, AuditLogEntry};
use std::sync::Arc;

#[test]
fn test_gui_state_mcp_config_generation() {
    let state = Arc::new(AppState::new("1234".to_string(), 9800));
    let gui_state = GuiState::new("192.168.1.105".to_string(), state);

    let config_json = gui_state.generate_mcp_config();
    let parsed: serde_json::Value = serde_json::from_str(&config_json).expect("valid json");

    assert_eq!(
        parsed["mcpServers"]["at-pc"]["url"],
        "http://192.168.1.105:9800/sse"
    );
    assert_eq!(
        parsed["mcpServers"]["at-pc"]["headers"]["Authorization"],
        "Bearer 1234"
    );
}

#[test]
fn test_gui_audit_logs_streaming() {
    let state = Arc::new(AppState::new("5678".to_string(), 9801));
    let mut gui_state = GuiState::new("10.0.0.2".to_string(), state.clone());

    assert_eq!(gui_state.audit_logs.len(), 0);

    // Broadcast multiple audit entries
    state.broadcast_audit(AuditLogEntry::new(
        "get_system_overview",
        serde_json::json!({}),
        "SUCCESS",
        Some(12),
        Some("10.0.0.100".to_string()),
        Some("System metrics fetched".to_string()),
    ));

    state.broadcast_audit(AuditLogEntry::new(
        "capture_screen",
        serde_json::json!({"display_index": 0}),
        "SUCCESS",
        Some(85),
        Some("10.0.0.100".to_string()),
        Some("Screen captured".to_string()),
    ));

    state.broadcast_audit(AuditLogEntry::new(
        "kill_process",
        serde_json::json!({"pid": 1234}),
        "FAILED",
        Some(5),
        Some("10.0.0.100".to_string()),
        Some("Process not found".to_string()),
    ));

    gui_state.poll_audit_logs();

    assert_eq!(gui_state.audit_logs.len(), 3);
    assert_eq!(gui_state.audit_logs[0].tool_name, "get_system_overview");
    assert_eq!(gui_state.audit_logs[0].status, "SUCCESS");
    assert_eq!(gui_state.audit_logs[1].tool_name, "capture_screen");
    assert_eq!(gui_state.audit_logs[1].status, "SUCCESS");
    assert_eq!(gui_state.audit_logs[2].tool_name, "kill_process");
    assert_eq!(gui_state.audit_logs[2].status, "FAILED");
}

#[test]
fn test_gui_emergency_stop() {
    let state = Arc::new(AppState::new("9999".to_string(), 9802));
    let shutdown_rx = state.subscribe_shutdown();
    let mut gui_state = GuiState::new("127.0.0.1".to_string(), state);

    assert!(!gui_state.is_stopped);
    assert!(!*shutdown_rx.borrow());

    gui_state.trigger_emergency_stop();

    assert!(gui_state.is_stopped);
    assert!(*shutdown_rx.borrow());
    assert!(gui_state
        .audit_logs
        .iter()
        .any(|e| e.tool_name == "emergency_stop"));
}

#[test]
fn test_pc_troubleshooter_app_creation() {
    let state = Arc::new(AppState::new("4321".to_string(), 9803));
    let app = PcTroubleshooterApp::new("192.168.0.50".to_string(), state);

    assert_eq!(app.state.lan_ip, "192.168.0.50");
    assert_eq!(app.state.port, 9803);
    assert_eq!(app.state.pin, "4321");
    assert_eq!(app.state.connected_clients(), 0);
}
