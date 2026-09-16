//! Integration and unit tests for Milestone 3 (P2 / v0.6.0):
//! Window lifecycle management suite (`list_windows`, `focus_window`, `close_window`)
//! and closed-loop action review verification (`StateDiff` / Review Loop).

use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::tools::window::{
    compute_state_diff, reset_window_mocks, set_mock_active_window, set_mock_windows, WindowState,
};
use at_pc_protocol::models::{StateDiff, WindowInfo};
use serde_json::json;

static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn test_window_tools_permission_toggle() {
    let _guard = TEST_LOCK.lock().await;
    reset_window_mocks();

    // 1. Default executor has computer_use = false
    let disabled = AgentExecutor::new();
    assert!(!disabled.enable_computer_use);

    let tools = [
        ("list_windows", json!({})),
        ("focus_window", json!({"title": "test"})),
        ("close_window", json!({"title": "test"})),
    ];

    for (tool_name, args) in tools {
        let err = disabled.execute(tool_name, args).await.unwrap_err();
        assert!(
            err.to_lowercase().contains("computer-use") && err.to_lowercase().contains("disabled"),
            "Expected disabled message for '{}', got: '{}'",
            tool_name,
            err
        );
    }

    // 2. Enabled executor allows window operations
    let enabled = AgentExecutor::new().with_computer_use(true);
    assert!(enabled.enable_computer_use);

    let res = enabled
        .execute("list_windows", json!({"only_visible": true}))
        .await;
    assert!(res.is_ok(), "list_windows failed: {:?}", res);
    let val = res.unwrap();
    assert!(
        val.is_array(),
        "Expected JSON array of windows, got: {:?}",
        val
    );
}

#[tokio::test]
async fn test_list_windows_structure_and_mock_filtering() {
    let _guard = TEST_LOCK.lock().await;
    reset_window_mocks();

    // Configure mock windows
    let mock_list = vec![
        WindowInfo {
            hwnd: 1001,
            pid: 1234,
            title: "Main Document - Word".to_string(),
            process_name: "WINWORD.EXE".to_string(),
            is_minimized: false,
            is_foreground: true,
            rect: [50, 50, 1024, 768],
            display_index: None,
        },
        WindowInfo {
            hwnd: 1002,
            pid: 5678,
            title: "Background Tool Window".to_string(),
            process_name: "tool.exe".to_string(),
            is_minimized: true, // Minimized
            is_foreground: false,
            rect: [0, 0, 100, 100],
            display_index: None,
        },
        WindowInfo {
            hwnd: 1003,
            pid: 9999,
            title: "   ".to_string(), // Empty / whitespace
            process_name: "system.exe".to_string(),
            is_minimized: false,
            is_foreground: false,
            rect: [0, 0, 50, 50],
            display_index: None,
        },
    ];

    set_mock_windows(Some(mock_list));

    let executor = AgentExecutor::new().with_computer_use(true);

    // 1. only_visible = true (default) filters minimized and empty titles
    let res = executor
        .execute("list_windows", json!({"only_visible": true}))
        .await
        .unwrap();
    let wins: Vec<WindowInfo> = serde_json::from_value(res).unwrap();
    assert_eq!(wins.len(), 1);
    assert_eq!(wins[0].hwnd, 1001);
    assert_eq!(wins[0].title, "Main Document - Word");
    assert!(wins[0].is_foreground);
    assert_eq!(wins[0].rect, [50, 50, 1024, 768]);

    // 2. only_visible = false returns all 3 windows
    let res_all = executor
        .execute("list_windows", json!({"only_visible": false}))
        .await
        .unwrap();
    let wins_all: Vec<WindowInfo> = serde_json::from_value(res_all).unwrap();
    assert_eq!(wins_all.len(), 3);

    reset_window_mocks();
}

#[tokio::test]
async fn test_focus_and_close_window_dispatch() {
    let _guard = TEST_LOCK.lock().await;
    reset_window_mocks();

    let mock_list = vec![
        WindowInfo {
            hwnd: 2001,
            pid: 4001,
            title: "Settings".to_string(),
            process_name: "settings.exe".to_string(),
            is_minimized: false,
            is_foreground: false,
            rect: [100, 100, 800, 600],
            display_index: None,
        },
        WindowInfo {
            hwnd: 2002,
            pid: 4002,
            title: "Calculator".to_string(),
            process_name: "calc.exe".to_string(),
            is_minimized: false,
            is_foreground: true,
            rect: [200, 200, 400, 500],
            display_index: None,
        },
    ];
    set_mock_windows(Some(mock_list));

    let executor = AgentExecutor::new().with_computer_use(true);

    // 1. Error when no parameter provided
    let err = executor
        .execute("focus_window", json!({}))
        .await
        .unwrap_err();
    assert!(err.contains("At least one parameter"));

    let err = executor
        .execute("close_window", json!({}))
        .await
        .unwrap_err();
    assert!(err.contains("At least one parameter"));

    // 2. Focus by title
    let focus_res = executor
        .execute("focus_window", json!({"title": "settings"}))
        .await
        .unwrap();
    assert_eq!(focus_res["success"], true);
    assert_eq!(focus_res["hwnd"], 2001);
    assert_eq!(focus_res["title"], "Settings");

    // 3. Focus by PID
    let focus_pid_res = executor
        .execute("focus_window", json!({"pid": 4002}))
        .await
        .unwrap();
    assert_eq!(focus_pid_res["success"], true);
    assert_eq!(focus_pid_res["hwnd"], 2002);

    // 4. Focus by HWND
    let focus_hwnd_res = executor
        .execute("focus_window", json!({"hwnd": 2001}))
        .await
        .unwrap();
    assert_eq!(focus_hwnd_res["success"], true);
    assert_eq!(focus_hwnd_res["hwnd"], 2001);

    // 5. Close by title
    let close_res = executor
        .execute("close_window", json!({"title": "calculator"}))
        .await
        .unwrap();
    assert_eq!(close_res["success"], true);
    assert_eq!(close_res["title"], "Calculator");

    // Verify window was closed from mock list
    let remaining_res = executor
        .execute("list_windows", json!({"only_visible": false}))
        .await
        .unwrap();
    let remaining: Vec<WindowInfo> = serde_json::from_value(remaining_res).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].title, "Settings");

    // 6. Non-existent window returns error
    let err_not_found = executor
        .execute("focus_window", json!({"title": "nonexistent"}))
        .await
        .unwrap_err();
    assert!(err_not_found.contains("not found"));

    reset_window_mocks();
}

#[test]
fn test_state_diff_computation_logic() {
    let base = WindowState {
        hwnd: 100,
        title: "Browser - Google Chrome".to_string(),
        is_dialog: false,
    };

    // 1. Identical window before and after => None
    let diff = compute_state_diff(&base, &base);
    assert_eq!(diff, None);

    // 2. Switched foreground window
    let switched = WindowState {
        hwnd: 200,
        title: "Code Editor - VS Code".to_string(),
        is_dialog: false,
    };
    let diff = compute_state_diff(&base, &switched).expect("StateDiff expected on window switch");
    assert_eq!(
        diff,
        StateDiff {
            foreground_changed: true,
            previous_window: Some("Browser - Google Chrome".to_string()),
            current_window: Some("Code Editor - VS Code".to_string()),
            modal_dialog_detected: false,
            dialog_title: None,
            ui_diff: None,
        }
    );

    // 3. New modal dialog detected
    let dialog = WindowState {
        hwnd: 300,
        title: "Save Changes Confirm Dialog".to_string(),
        is_dialog: true,
    };
    let diff = compute_state_diff(&base, &dialog).expect("StateDiff expected on modal dialog");
    assert_eq!(
        diff,
        StateDiff {
            foreground_changed: true,
            previous_window: Some("Browser - Google Chrome".to_string()),
            current_window: Some("Save Changes Confirm Dialog".to_string()),
            modal_dialog_detected: true,
            dialog_title: Some("Save Changes Confirm Dialog".to_string()),
            ui_diff: None,
        }
    );
}

#[tokio::test]
async fn test_executor_review_loop_attaches_state_diff() {
    let _guard = TEST_LOCK.lock().await;
    reset_window_mocks();

    let executor = AgentExecutor::new().with_computer_use(true);

    // 1. When no state change occurs, mouse_click does NOT contain state_diff
    let initial_state = WindowState {
        hwnd: 500,
        title: "Same Window".to_string(),
        is_dialog: false,
    };
    set_mock_active_window(Some(initial_state.clone()));

    let res = executor
        .execute("mouse_click", json!({"x": 100, "y": 100}))
        .await
        .unwrap();
    assert_eq!(res["success"], true);
    assert!(
        res.get("state_diff").is_none(),
        "Expected no state_diff when active window is unchanged, got: {:?}",
        res.get("state_diff")
    );

    // 2. When active window changes, mouse_click automatically attaches state_diff
    // Simulate active window transition in review loop
    let changed_state = WindowState {
        hwnd: 600,
        title: "New Window After Click".to_string(),
        is_dialog: false,
    };

    // Set mock before click
    set_mock_active_window(Some(initial_state.clone()));

    // Spawn a task that simulates window transition shortly after click injection
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        set_mock_active_window(Some(changed_state));
    });

    let res_changed = executor
        .execute("mouse_click", json!({"x": 200, "y": 200}))
        .await
        .unwrap();
    assert_eq!(res_changed["success"], true);
    let diff = res_changed
        .get("state_diff")
        .expect("Expected state_diff in mouse_click result");
    assert_eq!(diff["foreground_changed"], true);
    assert_eq!(diff["previous_window"], "Same Window");
    assert_eq!(diff["current_window"], "New Window After Click");

    // 3. Test press_key with modal dialog appearance
    let dialog_state = WindowState {
        hwnd: 700,
        title: "Confirm Exit Dialog".to_string(),
        is_dialog: true,
    };
    set_mock_active_window(Some(initial_state));

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        set_mock_active_window(Some(dialog_state));
    });

    let res_key = executor
        .execute("press_key", json!({"key": "escape"}))
        .await
        .unwrap();
    assert_eq!(res_key["success"], true);
    let diff_key = res_key
        .get("state_diff")
        .expect("Expected state_diff on modal dialog");
    assert_eq!(diff_key["modal_dialog_detected"], true);
    assert_eq!(diff_key["dialog_title"], "Confirm Exit Dialog");

    reset_window_mocks();
}
