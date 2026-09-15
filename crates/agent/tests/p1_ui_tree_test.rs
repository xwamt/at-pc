use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::tools::uia::{
    clear_and_store_elements, click_element, get_cached_element, get_ui_tree, set_element_text,
};
use at_pc_protocol::models::UiElement;
use serde_json::json;

static TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[test]
fn test_get_ui_tree_structure_and_caching() {
    let _lock = TEST_MUTEX.blocking_lock();
    let tree_res = get_ui_tree(Some(4), None);
    assert!(tree_res.is_ok(), "get_ui_tree should succeed: {:?}", tree_res.err());

    let tree = tree_res.unwrap();
    assert!(!tree.active_window.is_empty(), "active_window should not be empty");
    assert!(tree.window_bounds[2] > 0, "window width should be > 0");
    assert!(tree.window_bounds[3] > 0, "window height should be > 0");
    assert!(!tree.elements.is_empty(), "elements list should not be empty");
    assert_eq!(tree.total_elements, tree.elements.len());

    // Verify first element is cached
    let first_id = tree.elements[0].id;
    let cached = get_cached_element(first_id);
    assert!(cached.is_some(), "First element should be in cache");
    let c = cached.unwrap();
    assert_eq!(c.id, first_id);
    assert_eq!(c.control_type, tree.elements[0].control_type);
}

#[test]
fn test_element_cache_clear_and_lookup() {
    let _lock = TEST_MUTEX.blocking_lock();
    let mock_elements = vec![
        UiElement {
            id: 101,
            control_type: "Button".to_string(),
            name: "OK".to_string(),
            value: None,
            rect: [100, 100, 80, 30],
            enabled: true,
            help_text: None,
        },
        UiElement {
            id: 102,
            control_type: "Edit".to_string(),
            name: "Username".to_string(),
            value: Some("initial".to_string()),
            rect: [100, 150, 200, 30],
            enabled: true,
            help_text: Some("Enter username".to_string()),
        },
    ];

    at_pc_agent::tools::uia::store_cached_elements(&mock_elements);

    let el1 = get_cached_element(101).expect("Element 101 must exist");
    assert_eq!(el1.name, "OK");
    assert_eq!(el1.rect, [100, 100, 80, 30]);

    let el2 = get_cached_element(102).expect("Element 102 must exist");
    assert_eq!(el2.value.as_deref(), Some("initial"));

    assert!(get_cached_element(9999).is_none());
}

#[test]
fn test_click_element_not_found() {
    let _lock = TEST_MUTEX.blocking_lock();
    let res = click_element(888_888, None);
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("not found in UI element cache"));
}

#[test]
fn test_click_element_bounding_box_and_invoke() {
    let _lock = TEST_MUTEX.blocking_lock();
    let mock_elements = vec![UiElement {
        id: 201,
        control_type: "Button".to_string(),
        name: "Submit".to_string(),
        value: None,
        rect: [200, 200, 100, 40],
        enabled: true,
        help_text: None,
    }];
    at_pc_agent::tools::uia::store_cached_elements(&mock_elements);

    // Direct physical click mode
    let res = click_element(201, Some("click"));
    assert!(res.is_ok(), "click mode should succeed: {:?}", res.err());
    let val = res.unwrap();
    assert_eq!(val["success"], true);
    assert_eq!(val["element_id"], 201);
    assert_eq!(val["action"], "click");
    assert_eq!(val["coordinates"], json!([250, 220]));

    // Invoke mode
    let res_invoke = click_element(201, Some("invoke"));
    assert!(res_invoke.is_ok());
    let val_inv = res_invoke.unwrap();
    assert_eq!(val_inv["success"], true);
    assert_eq!(val_inv["element_id"], 201);
}

#[test]
fn test_set_element_text_not_found() {
    let _lock = TEST_MUTEX.blocking_lock();
    let res = set_element_text(777_777, "text");
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("not found in UI element cache"));
}

#[test]
fn test_set_element_text_and_cache_update() {
    let _lock = TEST_MUTEX.blocking_lock();
    let mock_elements = vec![UiElement {
        id: 301,
        control_type: "Edit".to_string(),
        name: "Search".to_string(),
        value: None,
        rect: [300, 300, 150, 35],
        enabled: true,
        help_text: None,
    }];
    at_pc_agent::tools::uia::store_cached_elements(&mock_elements);

    let res = set_element_text(301, "new search query");
    assert!(res.is_ok(), "set_element_text should succeed: {:?}", res.err());
    let val = res.unwrap();
    assert_eq!(val["success"], true);
    assert_eq!(val["element_id"], 301);
    assert_eq!(val["action"], "set_text");
    assert_eq!(val["text"], "new search query");

    // Verify cache was updated with new value
    let updated = get_cached_element(301).unwrap();
    assert_eq!(updated.value.as_deref(), Some("new search query"));
}

#[tokio::test]
async fn test_executor_uia_tool_dispatch_and_permission_toggle() {
    let _lock = TEST_MUTEX.lock().await;
    // 1. Without computer-use enabled
    let executor_disabled = AgentExecutor::new().with_computer_use(false);

    // get_ui_tree is read-only diagnostic, should work without computer_use
    let tree_res = executor_disabled
        .execute("get_ui_tree", json!({ "depth": 3 }))
        .await;
    assert!(tree_res.is_ok(), "get_ui_tree should be allowed without computer_use: {:?}", tree_res.err());

    // click_element requires computer_use
    let click_res = executor_disabled
        .execute("click_element", json!({ "element_id": 1 }))
        .await;
    assert!(click_res.is_err());
    assert!(click_res.unwrap_err().contains("Computer-use operations are disabled"));

    // set_element_text requires computer_use
    let text_res = executor_disabled
        .execute("set_element_text", json!({ "element_id": 1, "text": "hello" }))
        .await;
    assert!(text_res.is_err());
    assert!(text_res.unwrap_err().contains("Computer-use operations are disabled"));

    // 2. With computer-use enabled
    let executor_enabled = AgentExecutor::new().with_computer_use(true);

    // Seed cache
    clear_and_store_elements(&[UiElement {
        id: 401,
        control_type: "Button".to_string(),
        name: "TestButton".to_string(),
        value: None,
        rect: [50, 50, 60, 25],
        enabled: true,
        help_text: None,
    }]);

    let click_ok = executor_enabled
        .execute("click_element", json!({ "element_id": 401, "action_type": "click" }))
        .await;
    assert!(click_ok.is_ok(), "click_element should succeed when enabled: {:?}", click_ok.err());

    let text_ok = executor_enabled
        .execute("set_element_text", json!({ "element_id": 401, "text": "agent input" }))
        .await;
    assert!(text_ok.is_ok(), "set_element_text should succeed when enabled: {:?}", text_ok.err());
}

#[test]
fn test_get_ui_tree_window_title_filtering() {
    let _lock = TEST_MUTEX.blocking_lock();
    // Non-existent window title must return Err
    let not_found_res = get_ui_tree(Some(3), Some("nonexistent_window_filter_xyz123_456"));
    assert!(not_found_res.is_err(), "Non-existent window title must return error");
    let err = not_found_res.unwrap_err();
    assert!(err.contains("No window matching title"), "Error should explain window was not found: {}", err);

    // Empty or whitespace window title should fall back gracefully to active window
    let empty_title_res = get_ui_tree(Some(3), Some("   "));
    assert!(empty_title_res.is_ok(), "Whitespace title should fall back to active window: {:?}", empty_title_res.err());
}

#[test]
fn test_get_ui_tree_depth_filtering() {
    let _lock = TEST_MUTEX.blocking_lock();
    // Depth 1 should return only top-level container/titlebar
    let d1_res = get_ui_tree(Some(1), None).expect("Depth 1 should succeed");
    assert!(d1_res.elements.len() <= 3, "Depth 1 should return pruned shallow elements: {}", d1_res.elements.len());

    // Depth 2+ includes child controls (Buttons, Edit)
    let d2_res = get_ui_tree(Some(3), None).expect("Depth 3 should succeed");
    assert!(d2_res.elements.len() >= d1_res.elements.len(), "Depth 3 should return deeper elements");
    assert!(d2_res.elements.iter().any(|e| e.control_type == "Edit" || e.control_type == "Button"), "Depth 3 should include interactive controls");
}

#[test]
fn test_click_element_invalid_action_type() {
    let _lock = TEST_MUTEX.blocking_lock();
    let mock_elements = vec![UiElement {
        id: 501,
        control_type: "Button".to_string(),
        name: "TestButton501".to_string(),
        value: None,
        rect: [100, 100, 50, 30],
        enabled: true,
        help_text: None,
    }];
    at_pc_agent::tools::uia::store_cached_elements(&mock_elements);

    let res = click_element(501, Some("unsupported_action"));
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("Invalid action_type"), "Should reject unknown action_type: {}", err);
}

#[tokio::test]
async fn test_click_and_set_text_with_string_element_id() {
    let _lock = TEST_MUTEX.lock().await;
    let executor = AgentExecutor::new().with_computer_use(true);

    let mock_elements = vec![UiElement {
        id: 601,
        control_type: "Edit".to_string(),
        name: "QueryBox".to_string(),
        value: Some("old value".to_string()),
        rect: [120, 120, 100, 30],
        enabled: true,
        help_text: None,
    }];
    at_pc_agent::tools::uia::store_cached_elements(&mock_elements);

    // Call click_element with string ID with leading hash "#601"
    let click_res = executor
        .execute("click_element", json!({ "element_id": "#601", "action_type": "click" }))
        .await;
    assert!(click_res.is_ok(), "click_element with '#601' should succeed: {:?}", click_res.err());

    // Call set_element_text with string ID "601"
    let set_res = executor
        .execute("set_element_text", json!({ "element_id": "601", "text": "replaced text" }))
        .await;
    assert!(set_res.is_ok(), "set_element_text with '601' should succeed: {:?}", set_res.err());

    let cached = get_cached_element(601).expect("Element 601 should exist");
    assert_eq!(cached.value.as_deref(), Some("replaced text"));
}

#[tokio::test]
async fn test_set_element_text_empty_string_clearing() {
    let _lock = TEST_MUTEX.lock().await;
    let executor = AgentExecutor::new().with_computer_use(true);

    let mock_elements = vec![UiElement {
        id: 701,
        control_type: "Edit".to_string(),
        name: "ClearMe".to_string(),
        value: Some("initial text".to_string()),
        rect: [200, 200, 150, 30],
        enabled: true,
        help_text: None,
    }];
    at_pc_agent::tools::uia::store_cached_elements(&mock_elements);

    let res = executor
        .execute("set_element_text", json!({ "element_id": 701, "text": "" }))
        .await;
    assert!(res.is_ok(), "Setting text to empty string should succeed: {:?}", res.err());

    let cached = get_cached_element(701).unwrap();
    assert_eq!(cached.value.as_deref(), Some(""));
}

#[test]
fn test_session_unique_element_ids_monotonic() {
    let _lock = TEST_MUTEX.blocking_lock();

    let tree1 = get_ui_tree(Some(2), None).expect("First tree call should succeed");
    let max_id_1 = tree1.elements.iter().map(|e| e.id).max().unwrap_or(0);

    let tree2 = get_ui_tree(Some(2), None).expect("Second tree call should succeed");
    let min_id_2 = tree2.elements.iter().map(|e| e.id).min().unwrap_or(0);

    assert!(
        min_id_2 > max_id_1,
        "New tree calls should allocate session-unique monotonic IDs (tree1 max: {}, tree2 min: {})",
        max_id_1,
        min_id_2
    );

    // Elements from tree1 must still be retrievable from cache
    let first_elem_id = tree1.elements[0].id;
    assert!(
        get_cached_element(first_elem_id).is_some(),
        "Elements from previous tree call must remain cached across queries"
    );
}

