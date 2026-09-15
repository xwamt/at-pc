//! Batch/Macro UI action sequence execution with terminal-side 80ms state_diff.
//!
//! Allows executing a compound sequence of UI actions (click, type, press key, hotkey, wait)
//! in a single round-trip without multi-turn LLM latency, then returns the 80ms post-action UI state diff.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

use crate::tools::uia;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchActionStep {
    pub action: String,
    #[serde(default)]
    pub element_id: Option<u32>,
    #[serde(default)]
    pub action_type: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub keys: Option<Vec<String>>,
    #[serde(default)]
    pub x: Option<u32>,
    #[serde(default)]
    pub y: Option<u32>,
    #[serde(default)]
    pub button: Option<String>,
    #[serde(default)]
    pub count: Option<u8>,
    #[serde(default)]
    pub ms: Option<u64>,
    #[serde(default)]
    pub title: Option<String>,
}

/// Executes a sequential batch of UI actions with inter-step delay and returns post-action state diff
pub fn execute_batch_actions(arguments: &Value) -> Result<Value, String> {
    let actions_val = arguments
        .get("actions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Missing required array parameter 'actions'".to_string())?;

    if actions_val.is_empty() {
        return Err("Parameter 'actions' must contain at least one action step".to_string());
    }

    let interval_ms = arguments
        .get("delay_ms")
        .or_else(|| arguments.get("interval_ms"))
        .and_then(|v| v.as_u64())
        .unwrap_or(40);

    let with_diff = arguments
        .get("with_diff")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let start_time = Instant::now();
    let before_snapshot = if with_diff {
        Some(uia::get_cached_elements_snapshot())
    } else {
        None
    };

    let mut executed_count = 0;
    let mut step_results = Vec::new();

    for (idx, step_val) in actions_val.iter().enumerate() {
        let step: BatchActionStep = serde_json::from_value(step_val.clone())
            .map_err(|e| format!("Invalid batch action step #{}: {}", idx + 1, e))?;

        let step_start = Instant::now();
        match step.action.trim().to_lowercase().as_str() {
            "click_element" | "click" => {
                let eid = step.element_id.ok_or_else(|| {
                    format!("Step #{}: Missing 'element_id' for click_element", idx + 1)
                })?;
                uia::click_element_with_diff(eid, step.action_type.as_deref(), Some(false))?;
                step_results.push(json!({
                    "step": idx + 1,
                    "action": "click_element",
                    "id": eid,
                    "duration_ms": step_start.elapsed().as_millis() as u64
                }));
            }
            "set_element_text" | "set_text" => {
                let eid = step.element_id.ok_or_else(|| {
                    format!("Step #{}: Missing 'element_id' for set_element_text", idx + 1)
                })?;
                let txt = step.text.as_deref().ok_or_else(|| {
                    format!("Step #{}: Missing 'text' for set_element_text", idx + 1)
                })?;
                uia::set_element_text_with_diff(eid, txt, Some(false))?;
                step_results.push(json!({
                    "step": idx + 1,
                    "action": "set_element_text",
                    "id": eid,
                    "duration_ms": step_start.elapsed().as_millis() as u64
                }));
            }
            "type_text" | "type" => {
                let txt = step.text.as_deref().ok_or_else(|| {
                    format!("Step #{}: Missing 'text' for type_text", idx + 1)
                })?;
                crate::tools::computer_use::execute_type_text(&json!({ "text": txt }))?;
                step_results.push(json!({
                    "step": idx + 1,
                    "action": "type_text",
                    "length": txt.chars().count(),
                    "duration_ms": step_start.elapsed().as_millis() as u64
                }));
            }
            "press_key" | "key" => {
                let k = step.key.as_deref().ok_or_else(|| {
                    format!("Step #{}: Missing 'key' for press_key", idx + 1)
                })?;
                crate::tools::computer_use::execute_press_key(&json!({ "key": k }))?;
                step_results.push(json!({
                    "step": idx + 1,
                    "action": "press_key",
                    "key": k,
                    "duration_ms": step_start.elapsed().as_millis() as u64
                }));
            }
            "hotkey" => {
                let ks = step.keys.as_ref().ok_or_else(|| {
                    format!("Step #{}: Missing 'keys' array for hotkey", idx + 1)
                })?;
                crate::tools::computer_use::execute_hotkey(&json!({ "keys": ks }))?;
                step_results.push(json!({
                    "step": idx + 1,
                    "action": "hotkey",
                    "keys": ks,
                    "duration_ms": step_start.elapsed().as_millis() as u64
                }));
            }
            "mouse_click" => {
                let mut args = json!({
                    "button": step.button.unwrap_or_else(|| "left".to_string()),
                    "count": step.count.unwrap_or(1),
                });
                if let (Some(x), Some(y)) = (step.x, step.y) {
                    args["x"] = json!(x);
                    args["y"] = json!(y);
                }
                crate::tools::computer_use::execute_mouse_click(&args)?;
                step_results.push(json!({
                    "step": idx + 1,
                    "action": "mouse_click",
                    "duration_ms": step_start.elapsed().as_millis() as u64
                }));
            }
            "wait" | "sleep" => {
                let ms = step.ms.unwrap_or(100);
                std::thread::sleep(Duration::from_millis(ms));
                step_results.push(json!({
                    "step": idx + 1,
                    "action": "wait",
                    "ms": ms
                }));
            }
            "focus_window" => {
                let title = step.title.as_deref().unwrap_or_default();
                crate::tools::window::focus_window(Some(title), None, None)?;
                step_results.push(json!({
                    "step": idx + 1,
                    "action": "focus_window",
                    "title": title
                }));
            }
            other => {
                return Err(format!(
                    "Unknown batch action '{}' at step #{}. Supported actions: click_element, set_element_text, type_text, press_key, hotkey, mouse_click, wait, focus_window",
                    other,
                    idx + 1
                ));
            }
        }
        executed_count += 1;

        if interval_ms > 0 && idx < actions_val.len() - 1 {
            std::thread::sleep(Duration::from_millis(interval_ms));
        }
    }

    let mut res = json!({
        "success": true,
        "steps_executed": executed_count,
        "total_steps": actions_val.len(),
        "duration_ms": start_time.elapsed().as_millis() as u64,
        "step_details": step_results,
    });

    if let Some(before) = before_snapshot {
        let diff = uia::capture_post_action_diff(&before);
        res["state_diff"] = serde_json::to_value(diff).unwrap_or(Value::Null);
    }

    Ok(res)
}
