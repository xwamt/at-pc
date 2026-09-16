//! P1-6 PART A: command execution must wait with timeout, not 10ms busy-poll.

use std::sync::Arc;
use std::time::{Duration, Instant};

use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::tools::command::{exec_cmd_with_call_async, exec_cmd_with_registry};
use at_pc_agent::tools::ProcessRegistry;

#[test]
fn command_rs_does_not_busy_poll_try_wait() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/tools/command.rs"));
    assert!(
        !src.contains("child.try_wait()"),
        "command wait must not busy-poll with try_wait()"
    );
    assert!(
        !src.contains("std::thread::sleep(poll_interval)"),
        "command wait must not sleep in a 10ms poll loop"
    );
    assert!(
        src.contains("tokio::process::Command") || src.contains("tokio::time::timeout"),
        "command wait must use tokio::process + timeout (or equivalent)"
    );
}

#[test]
fn command_runtime_path_does_not_spawn_nested_runtime() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/tools/command.rs"));
    assert!(
        !src.contains("std::thread::spawn"),
        "existing Tokio runtime must not spawn an extra OS thread for command wait"
    );
}

#[test]
fn executor_does_not_spawn_blocking_exec_cmd() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/executor.rs"));
    assert!(
        src.contains("exec_cmd_with_call_async"),
        "production exec_cmd must use the async command path, not spawn_blocking"
    );
    assert!(
        src.contains("exec_powershell_with_call_async"),
        "production exec_powershell must use the async command path, not spawn_blocking"
    );
    let spawn_blocking_idx = src
        .find("spawn_blocking")
        .expect("other tools may still block");
    let exec_cmd_async_idx = src
        .find("exec_cmd |")
        .or_else(|| src.find("\"exec_cmd\""))
        .expect("exec_cmd production branch");
    assert!(
        exec_cmd_async_idx < spawn_blocking_idx
            || src.contains("execute_shell_tool")
            || src.contains("tokio::spawn"),
        "production exec_cmd dispatch must be async, not wrapped in spawn_blocking around wait"
    );
}

#[test]
fn production_exec_cmd_does_not_wait_on_blocking_pool() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .expect("runtime");

    rt.block_on(async {
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (hold_tx, hold_rx) = std::sync::mpsc::channel::<()>();
        let blocker = tokio::task::spawn_blocking(move || {
            let _ = entered_tx.send(());
            let _ = hold_rx.recv();
        });
        entered_rx
            .recv()
            .expect("blocker must occupy the only spawn_blocking thread");

        let executor = AgentExecutor::new();
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            executor.execute(
                "exec_cmd",
                serde_json::json!({ "command": "echo p16_not_blocking" }),
            ),
        )
        .await;

        drop(hold_tx);
        let _ = blocker.await;

        let result = result
            .expect("production exec_cmd must not queue behind a saturated spawn_blocking pool");
        let val = result.expect("exec_cmd must succeed");
        assert!(
            val.get("stdout")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .contains("p16_not_blocking"),
            "stdout was: {val}"
        );
    });
}

#[test]
fn exec_cmd_captures_stdout() {
    let registry = Arc::new(ProcessRegistry::new());
    let result =
        exec_cmd_with_registry("echo at_p16_cmd", 5, None, &registry).expect("echo must succeed");
    assert_eq!(result.exit_code, 0);
    assert!(
        result.stdout.contains("at_p16_cmd"),
        "stdout was: {:?}",
        result.stdout
    );
}

#[test]
fn exec_cmd_timeout_kills_promptly() {
    let registry = Arc::new(ProcessRegistry::new());
    let sleep_cmd = if cfg!(windows) {
        "powershell -Command Start-Sleep -Seconds 8"
    } else {
        "sleep 8"
    };
    let start = Instant::now();
    let err = exec_cmd_with_registry(sleep_cmd, 1, None, &registry).expect_err("must time out");
    assert!(
        err.contains("timed out"),
        "timeout error should mention timeout, got: {err}"
    );
    assert!(
        start.elapsed() < Duration::from_millis(2500),
        "timeout must complete promptly, took {:?}",
        start.elapsed()
    );
    assert_eq!(
        registry.active_count(),
        0,
        "timed-out process must unregister"
    );
}

#[tokio::test]
async fn exec_cmd_from_runtime_does_not_need_nested_runtime() {
    let registry = Arc::new(ProcessRegistry::new());
    let result = exec_cmd_with_call_async("echo at_p16_async", 5, None, &registry, None)
        .await
        .expect("async exec_cmd must run on the existing runtime");
    assert_eq!(result.exit_code, 0);
    assert!(
        result.stdout.contains("at_p16_async"),
        "stdout was: {:?}",
        result.stdout
    );
}

#[tokio::test]
async fn exec_cmd_cancel_via_registry_unblocks() {
    let registry = Arc::new(ProcessRegistry::new());
    let sleep_cmd = if cfg!(windows) {
        "powershell -Command Start-Sleep -Seconds 10"
    } else {
        "sleep 10"
    };
    let call_id = "p16-cmd-cancel";
    let registry_task = registry.clone();
    let task = tokio::spawn(async move {
        exec_cmd_with_call_async(sleep_cmd, 30, None, &registry_task, Some(call_id)).await
    });

    tokio::time::sleep(Duration::from_millis(150)).await;
    let killed = registry.kill_call_processes(call_id);
    assert!(killed > 0, "cancel must find the registered child");

    let start = Instant::now();
    let res = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .expect("killed command must unblock")
        .expect("async command task must not panic");
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "killed wait must not busy-poll until the original timeout"
    );
    if let Ok(result) = res {
        assert_ne!(result.exit_code, 0, "killed process should not exit 0");
    }
}
