use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::tools::uia::{
    compute_state_diff, get_ui_tree_filtered, store_cached_elements, CachedElement,
};
use at_pc_protocol::models::UiElement;
use serde_json::json;

static TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn test_ui_tree_query_and_compact_filtering() {
    let _lock = TEST_MUTEX.lock().await;

    // Call fallback ui tree with query filter
    let tree_res = get_ui_tree_filtered(Some(3), None, Some("button"), None);
    assert!(tree_res.is_ok(), "Query filtering should succeed");
    let tree = tree_res.unwrap();
    assert_eq!(tree.query.as_deref(), Some("button"));
    assert!(tree.query_matched.is_some());
    for el in &tree.elements {
        let name_match = el.name.to_lowercase().contains("button");
        let type_match = el.control_type.to_lowercase().contains("button");
        assert!(name_match || type_match, "Element must match query filter");
    }

    // Call with compact=true
    let compact_res = get_ui_tree_filtered(Some(3), None, None, Some(true));
    assert!(compact_res.is_ok(), "Compact filtering should succeed");
    let compact_tree = compact_res.unwrap();
    assert!(compact_tree.compact.unwrap_or(false));
}

#[test]
fn test_compute_state_diff_logic() {
    let before = vec![
        CachedElement {
            id: 1,
            control_type: "Button".to_string(),
            name: "Send".to_string(),
            value: None,
            rect: [10, 10, 50, 30],
            enabled: true,
            help_text: None,
        },
        CachedElement {
            id: 2,
            control_type: "Edit".to_string(),
            name: "Input".to_string(),
            value: Some("Draft".to_string()),
            rect: [10, 50, 200, 30],
            enabled: true,
            help_text: None,
        },
    ];

    let after = vec![
        UiElement {
            id: 1,
            control_type: "Button".to_string(),
            name: "Send".to_string(),
            value: None,
            rect: [10, 10, 50, 30],
            enabled: true,
            help_text: None,
        },
        UiElement {
            id: 2,
            control_type: "Edit".to_string(),
            name: "Input".to_string(),
            value: Some("Sent!".to_string()), // modified value
            rect: [10, 50, 200, 30],
            enabled: true,
            help_text: None,
        },
        UiElement {
            id: 3, // newly added
            control_type: "Text".to_string(),
            name: "Message delivered".to_string(),
            value: None,
            rect: [10, 90, 150, 20],
            enabled: true,
            help_text: None,
        },
    ];

    let diff = compute_state_diff(&before, &after);
    assert!(diff.has_changes);
    assert_eq!(diff.added_elements.len(), 1);
    assert_eq!(diff.added_elements[0].name, "Message delivered");
    assert_eq!(diff.modified_elements.len(), 1);
    assert_eq!(diff.modified_elements[0].name, "Input");
    assert_eq!(diff.modified_elements[0].old_value.as_deref(), Some("Draft"));
    assert_eq!(diff.modified_elements[0].new_value.as_deref(), Some("Sent!"));
    assert!(diff.removed_elements.is_empty());
}

#[tokio::test]
async fn test_batch_actions_execution_and_diff() {
    let _lock = TEST_MUTEX.lock().await;
    let executor = AgentExecutor::new().with_computer_use(true);

    // Prepare mock cached element for clicking
    let mock_elements = vec![UiElement {
        id: 9901,
        control_type: "Button".to_string(),
        name: "TestButton".to_string(),
        value: None,
        rect: [50, 50, 100, 30],
        enabled: true,
        help_text: None,
    }];
    store_cached_elements(&mock_elements);

    let batch_req = json!({
        "actions": [
            {
                "action": "click_element",
                "element_id": 9901,
                "action_type": "click"
            },
            {
                "action": "wait",
                "ms": 50
            }
        ],
        "with_diff": true
    });

    let res = executor.execute("batch_actions", batch_req).await;
    assert!(res.is_ok(), "batch_actions should succeed: {:?}", res.err());
    let val = res.unwrap();
    assert_eq!(val["success"], true);
    assert_eq!(val["total_steps"], 2);
    assert_eq!(val["steps_executed"], 2);
    assert!(val["step_details"].is_array());
    assert!(val["state_diff"].is_object());
}

#[tokio::test]
async fn test_click_element_with_diff_response() {
    let _lock = TEST_MUTEX.lock().await;
    let executor = AgentExecutor::new().with_computer_use(true);

    let mock_elements = vec![UiElement {
        id: 9902,
        control_type: "Button".to_string(),
        name: "DiffButton".to_string(),
        value: None,
        rect: [100, 100, 80, 40],
        enabled: true,
        help_text: None,
    }];
    store_cached_elements(&mock_elements);

    let res = executor
        .execute("click_element", json!({ "element_id": 9902, "with_diff": true }))
        .await;
    assert!(res.is_ok(), "click_element with diff should succeed: {:?}", res.err());
    let val = res.unwrap();
    assert_eq!(val["success"], true);
    assert!(val["state_diff"].is_object());
}
