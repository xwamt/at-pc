use at_pc::app::GuiState;
use at_pc::server::state::AppState;
use at_pc::server::restart_server;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn test_pin_rotation_and_session_restart() {
    let state = Arc::new(AppState::new("1111".to_string(), 9800));
    state.trigger_emergency_stop();
    assert!(state.is_stopped());

    let new_pin = state.restart_session(None);
    assert_eq!(new_pin.len(), 4);
    assert!(!state.is_stopped());
    assert!(state.verify_pin(&new_pin));
}

#[test]
fn test_rotate_credentials_and_audit() {
    let state = Arc::new(AppState::new("1234".to_string(), 9800));
    let mut audit_rx = state.subscribe_audit();

    let (new_pin, port) = state.rotate_credentials(None, Some(9850));
    assert_eq!(new_pin.len(), 4);
    assert_eq!(port, 9850);
    assert_eq!(state.get_port(), 9850);
    assert!(state.verify_pin(&new_pin));
    assert!(!state.verify_pin("1234") || new_pin == "1234");

    let event = audit_rx.try_recv().expect("audit event emitted");
    assert_eq!(event.tool_name, "pin_rotated");
    assert_eq!(event.status, "SUCCESS");
}

#[test]
fn test_gui_state_rotation_and_confirmation() {
    let app_state = Arc::new(AppState::new("2222".to_string(), 9805));
    let mut gui_state = GuiState::new("192.168.1.120".to_string(), app_state.clone());

    // With 0 clients, rotate directly
    assert_eq!(gui_state.connected_clients(), 0);
    assert!(!gui_state.show_rotate_confirm);
    let (p1, _) = gui_state.rotate_credentials(None, None);
    assert_eq!(p1.len(), 4);
    assert_eq!(gui_state.pin, p1);
    assert!(gui_state.generate_mcp_config().contains(&p1));

    // Simulate 1 connected client
    app_state.increment_clients();
    assert_eq!(gui_state.connected_clients(), 1);

    // When clients > 0, UI sets show_rotate_confirm flag
    gui_state.show_rotate_confirm = true;
    assert!(gui_state.show_rotate_confirm);

    // Confirm rotation
    let (p2, _) = gui_state.rotate_credentials(Some("9999".to_string()), None);
    gui_state.show_rotate_confirm = false;
    assert_eq!(p2, "9999");
    assert_eq!(gui_state.pin, "9999");
    assert!(gui_state.generate_mcp_config().contains("Bearer 9999"));
}

#[test]
fn test_gui_state_session_restart_after_emergency_stop() {
    let app_state = Arc::new(AppState::new("3333".to_string(), 9810));
    let mut gui_state = GuiState::new("127.0.0.1".to_string(), app_state.clone());

    gui_state.trigger_emergency_stop();
    assert!(gui_state.is_stopped);
    assert!(app_state.is_stopped());
    assert_eq!(app_state.get_pin(), "");

    let new_pin = gui_state.restart_session(Some(9812));
    assert_eq!(new_pin.len(), 4);
    assert!(!gui_state.is_stopped);
    assert!(!app_state.is_stopped());
    assert_eq!(gui_state.pin, new_pin);
    assert_eq!(gui_state.port, 9812);
    assert_eq!(app_state.get_port(), 9812);
    assert!(app_state.verify_pin(&new_pin));
    assert!(gui_state.generate_mcp_config().contains("9812"));
    assert!(gui_state.generate_mcp_config().contains(&new_pin));
}

#[tokio::test]
async fn test_restart_server_task_spawn_and_shutdown() {
    let state = Arc::new(AppState::new("7777".to_string(), 19825));

    // Emergency stop
    state.trigger_emergency_stop();
    assert!(state.is_stopped());

    // Restart session
    let new_pin = state.restart_session(Some(19826));
    assert!(!state.is_stopped());
    assert_eq!(state.get_port(), 19826);
    assert!(state.verify_pin(&new_pin));

    // Respawn server via restart_server
    let server_handle = restart_server(state.clone(), 19826);

    // Small sleep to ensure task is running
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Trigger shutdown and ensure server task terminates gracefully
    state.trigger_shutdown();
    let result = tokio::time::timeout(Duration::from_secs(2), server_handle).await;
    assert!(result.is_ok(), "Server should gracefully terminate on shutdown signal");
}
