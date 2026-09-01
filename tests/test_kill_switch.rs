use at_pc::server::state::AppState;
use at_pc::tools::process_registry::ProcessRegistry;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn test_emergency_stop_invalidates_pin() {
    let state = Arc::new(AppState::new("1234".to_string(), 9800));
    assert_eq!(state.get_pin(), "1234");
    assert!(state.verify_pin("1234"));
    assert!(!state.is_stopped());

    state.trigger_emergency_stop();

    assert!(state.is_stopped());
    assert!(!state.verify_pin("1234"));
    assert!(!state.verify_pin(""));
    assert_eq!(state.get_pin(), "");
}

#[test]
fn test_process_registry_register_and_kill_all() {
    let registry = ProcessRegistry::new();

    // Spawn a long-running sleep command
    #[cfg(unix)]
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn sleep on unix");

    #[cfg(windows)]
    let mut child = Command::new("powershell")
        .args(["-Command", "Start-Sleep -Seconds 30"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn sleep on windows");

    let pid = child.id();
    let proc_id = 1001;

    registry.register_process(proc_id, pid);
    assert_eq!(registry.active_count(), 1);

    // Terminate all registered active processes
    let killed_count = registry.kill_all_active();
    assert_eq!(killed_count, 1);
    assert_eq!(registry.active_count(), 0);

    // Wait briefly and verify process actually exited / terminated
    std::thread::sleep(Duration::from_millis(150));
    let exit_res = child.try_wait().expect("try_wait failed");
    assert!(exit_res.is_some(), "Process should be terminated after kill_all_active");
}

#[test]
fn test_emergency_stop_kills_active_processes_in_state() {
    let state = Arc::new(AppState::new("5555".to_string(), 9801));

    #[cfg(unix)]
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn sleep on unix");

    #[cfg(windows)]
    let mut child = Command::new("powershell")
        .args(["-Command", "Start-Sleep -Seconds 30"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn sleep on windows");

    let pid = child.id();
    let proc_id = 2002;

    state.process_registry.register_process(proc_id, pid);
    assert_eq!(state.process_registry.active_count(), 1);

    // Trigger emergency stop via AppState
    state.trigger_emergency_stop();

    assert!(state.is_stopped());
    assert!(!state.verify_pin("5555"));
    assert_eq!(state.process_registry.active_count(), 0);

    std::thread::sleep(Duration::from_millis(150));
    let exit_res = child.try_wait().expect("try_wait failed");
    assert!(exit_res.is_some(), "Child process should be terminated after emergency stop");
}
