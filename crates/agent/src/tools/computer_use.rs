//! Computer-Use MCP tools implementation for simulated mouse and keyboard input.
//! Provides fine-grained control over mouse movement, clicks, dragging, scrolling,
//! text typing, individual key presses, and multi-key shortcuts.

use at_pc_protocol::models::DesktopInputEvent;
use serde_json::{json, Value};
use std::time::Duration;

use crate::input::inject_input_event;

/// Parses mouse button argument (supports string "left", "middle", "right" or integer 0, 1, 2)
fn parse_button(val: Option<&Value>) -> u8 {
    match val {
        Some(Value::String(s)) => match s.trim().to_lowercase().as_str() {
            "middle" | "center" | "m" => 1,
            "right" | "r" => 2,
            _ => 0, // default left
        },
        Some(Value::Number(n)) => n.as_u64().map(|v| v as u8).unwrap_or(0),
        _ => 0,
    }
}

/// Resolves coordinate input into appropriate DesktopInputEvent (pixel vs normalized)
fn resolve_mouse_move_event(
    arguments: &Value,
    x_key: &str,
    y_key: &str,
) -> Option<DesktopInputEvent> {
    let x = arguments
        .get(x_key)
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
        .map(|v| v as i32)?;
    let y = arguments
        .get(y_key)
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
        .map(|v| v as i32)?;
    let mode = arguments
        .get("coord_mode")
        .or_else(|| arguments.get("coordinate_mode"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "pixel".to_string());

    let display_index = arguments
        .get("display_index")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    if let Some(idx) = display_index {
        let monitors = crate::tools::screen::list_monitors().unwrap_or_default();
        if let Some(mon) = monitors.iter().find(|m| m.display_index == idx) {
            let (global_x, global_y) = match mode.as_str() {
                "normalized" | "norm" | "normalized_65535" => {
                    let gx = mon.x + ((x.clamp(0, 65535) as i64 * mon.width as i64) / 65535) as i32;
                    let gy =
                        mon.y + ((y.clamp(0, 65535) as i64 * mon.height as i64) / 65535) as i32;
                    (gx, gy)
                }
                "normalized_1000" => {
                    let gx = mon.x + ((x.clamp(0, 1000) as i64 * mon.width as i64) / 1000) as i32;
                    let gy = mon.y + ((y.clamp(0, 1000) as i64 * mon.height as i64) / 1000) as i32;
                    (gx, gy)
                }
                _ => {
                    let gx = mon.x + x;
                    let gy = mon.y + y;
                    (gx, gy)
                }
            };
            return Some(DesktopInputEvent::MouseMovePixel {
                x: global_x,
                y: global_y,
            });
        }
    }

    match mode.as_str() {
        "normalized" | "norm" | "normalized_65535" => Some(DesktopInputEvent::MouseMove {
            x: x.clamp(0, 65535) as u32,
            y: y.clamp(0, 65535) as u32,
        }),
        "normalized_1000" => {
            let nx = (x.clamp(0, 1000) as u32 * 65535) / 1000;
            let ny = (y.clamp(0, 1000) as u32 * 65535) / 1000;
            Some(DesktopInputEvent::MouseMove { x: nx, y: ny })
        }
        _ => {
            // Default: physical pixel coordinates
            Some(DesktopInputEvent::MouseMovePixel { x, y })
        }
    }
}

/// Executes simulated mouse click at current or specified coordinate
pub fn execute_mouse_click(arguments: &Value) -> Result<Value, String> {
    let button = parse_button(arguments.get("button"));
    let count = arguments
        .get("count")
        .and_then(|v| v.as_u64())
        .map(|v| v as u8)
        .unwrap_or(1)
        .max(1);

    // If mark_id is provided, resolve directly from Set-of-Mark (SoM) cache
    let mark_val_opt = arguments
        .get("mark_id")
        .or_else(|| arguments.get("mark"))
        .filter(|v| !v.is_null());

    if let Some(mark_val) = mark_val_opt {
        let mark_id = match mark_val {
            Value::Number(n) => n.as_u64().map(|v| v as u32),
            Value::String(s) => {
                let trimmed = s.trim().trim_start_matches('#');
                if trimmed.is_empty() {
                    None
                } else {
                    trimmed.parse::<u32>().ok()
                }
            }
            _ => None,
        };

        if let Some(mid) = mark_id {
            let mark = crate::tools::som::get_cached_mark(mid).ok_or_else(|| {
                format!(
                    "Mark #{} not found in mark cache. Call get_marked_screen first to generate marks.",
                    mid
                )
            })?;

            let px = mark.center[0];
            let py = mark.center[1];

            inject_input_event(DesktopInputEvent::MouseMovePixel { x: px, y: py })?;
            std::thread::sleep(Duration::from_millis(10));
            inject_input_event(DesktopInputEvent::MouseClick { button, count })?;

            return Ok(json!({
                "success": true,
                "action": "mouse_click",
                "mark_id": mid,
                "x": px,
                "y": py,
                "button": button,
                "count": count,
                "label": mark.label,
                "control_type": mark.control_type
            }));
        } else if arguments.get("x").is_none() && arguments.get("y").is_none() {
            return Err("Invalid 'mark_id' parameter format".to_string());
        }
        // If mark_id was invalid/empty but x and y are present, fall through to x, y coordinate click
    }

    let move_ev = resolve_mouse_move_event(arguments, "x", "y");
    let (px, py) = match &move_ev {
        Some(DesktopInputEvent::MouseMovePixel { x, y }) => (Some(*x), Some(*y)),
        Some(DesktopInputEvent::MouseMove { x, y }) => (Some(*x as i32), Some(*y as i32)),
        _ => (None, None),
    };

    if let Some(ev) = move_ev {
        inject_input_event(ev)?;
        std::thread::sleep(Duration::from_millis(10));
    }

    inject_input_event(DesktopInputEvent::MouseClick { button, count })?;

    let mut resp = json!({
        "success": true,
        "action": "mouse_click",
        "x": px,
        "y": py,
        "button": button,
        "count": count
    });
    if let Some(idx) = arguments.get("display_index").and_then(|v| v.as_u64()) {
        resp["display_index"] = json!(idx);
    }

    Ok(resp)
}

/// Executes mouse cursor movement
pub fn execute_mouse_move(arguments: &Value) -> Result<Value, String> {
    let move_ev = resolve_mouse_move_event(arguments, "x", "y")
        .ok_or_else(|| "Missing required parameters 'x' and 'y'".to_string())?;

    let (px, py) = match &move_ev {
        DesktopInputEvent::MouseMovePixel { x, y } => (*x, *y),
        DesktopInputEvent::MouseMove { x, y } => (*x as i32, *y as i32),
        _ => (0, 0),
    };

    inject_input_event(move_ev)?;

    let mut resp = json!({
        "success": true,
        "action": "mouse_move",
        "x": px,
        "y": py
    });
    if let Some(idx) = arguments.get("display_index").and_then(|v| v.as_u64()) {
        resp["display_index"] = json!(idx);
    }

    Ok(resp)
}

/// Executes mouse drag operation from start coordinate to end coordinate
pub fn execute_mouse_drag(arguments: &Value) -> Result<Value, String> {
    let start_ev = resolve_mouse_move_event(arguments, "start_x", "start_y")
        .or_else(|| resolve_mouse_move_event(arguments, "from_x", "from_y"));

    let end_ev = resolve_mouse_move_event(arguments, "end_x", "end_y")
        .or_else(|| resolve_mouse_move_event(arguments, "to_x", "to_y"))
        .ok_or_else(|| {
            "Missing required destination coordinates 'end_x' and 'end_y'".to_string()
        })?;

    let (sx, sy) = match &start_ev {
        Some(DesktopInputEvent::MouseMovePixel { x, y }) => (Some(*x), Some(*y)),
        Some(DesktopInputEvent::MouseMove { x, y }) => (Some(*x as i32), Some(*y as i32)),
        _ => (None, None),
    };
    let (ex, ey) = match &end_ev {
        DesktopInputEvent::MouseMovePixel { x, y } => (*x, *y),
        DesktopInputEvent::MouseMove { x, y } => (*x as i32, *y as i32),
        _ => (0, 0),
    };

    let button = parse_button(arguments.get("button"));

    if let Some(sev) = start_ev {
        inject_input_event(sev)?;
        std::thread::sleep(Duration::from_millis(15));
    }

    inject_input_event(DesktopInputEvent::MouseDown { button })?;
    std::thread::sleep(Duration::from_millis(20));
    inject_input_event(end_ev)?;
    std::thread::sleep(Duration::from_millis(20));
    inject_input_event(DesktopInputEvent::MouseUp { button })?;

    let mut resp = json!({
        "success": true,
        "action": "mouse_drag",
        "start_x": sx,
        "start_y": sy,
        "end_x": ex,
        "end_y": ey,
        "button": button
    });
    if let Some(idx) = arguments.get("display_index").and_then(|v| v.as_u64()) {
        resp["display_index"] = json!(idx);
    }

    Ok(resp)
}

/// Executes mouse scroll wheel event
pub fn execute_mouse_scroll(arguments: &Value) -> Result<Value, String> {
    let delta_y = arguments
        .get("delta_y")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32)
        .ok_or_else(|| "Missing required parameter 'delta_y'".to_string())?;

    let move_ev = resolve_mouse_move_event(arguments, "x", "y");
    let (px, py) = match &move_ev {
        Some(DesktopInputEvent::MouseMovePixel { x, y }) => (Some(*x), Some(*y)),
        Some(DesktopInputEvent::MouseMove { x, y }) => (Some(*x as i32), Some(*y as i32)),
        _ => (None, None),
    };

    if let Some(ev) = move_ev {
        inject_input_event(ev)?;
        std::thread::sleep(Duration::from_millis(10));
    }

    inject_input_event(DesktopInputEvent::MouseWheel { delta_y })?;

    let mut resp = json!({
        "success": true,
        "action": "mouse_scroll",
        "delta_y": delta_y,
        "x": px,
        "y": py
    });
    if let Some(idx) = arguments.get("display_index").and_then(|v| v.as_u64()) {
        resp["display_index"] = json!(idx);
    }

    Ok(resp)
}

/// Executes Unicode text typing
pub fn execute_type_text(arguments: &Value) -> Result<Value, String> {
    let text = arguments
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter 'text'".to_string())?;

    inject_input_event(DesktopInputEvent::TypeText {
        text: text.to_string(),
    })?;

    Ok(json!({
        "success": true,
        "action": "type_text",
        "length": text.chars().count()
    }))
}

/// Executes single key press (press and release)
pub fn execute_press_key(arguments: &Value) -> Result<Value, String> {
    let key = arguments
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter 'key'".to_string())?;

    let code = crate::input::try_key_name_to_code(key)
        .ok_or_else(|| format!("Unknown or unsupported key '{}'", key))?;

    inject_input_event(DesktopInputEvent::KeyDown {
        key_code: code,
        key: key.to_string(),
    })?;
    std::thread::sleep(Duration::from_millis(20));
    inject_input_event(DesktopInputEvent::KeyUp {
        key_code: code,
        key: key.to_string(),
    })?;

    Ok(json!({
        "success": true,
        "action": "press_key",
        "key": key,
        "key_code": code
    }))
}

/// Executes key down (hold)
pub fn execute_key_down(arguments: &Value) -> Result<Value, String> {
    let key = arguments
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter 'key'".to_string())?;

    let code = crate::input::try_key_name_to_code(key)
        .ok_or_else(|| format!("Unknown or unsupported key '{}'", key))?;

    inject_input_event(DesktopInputEvent::KeyDown {
        key_code: code,
        key: key.to_string(),
    })?;

    Ok(json!({
        "success": true,
        "action": "key_down",
        "key": key,
        "key_code": code
    }))
}

/// Executes key up (release)
pub fn execute_key_up(arguments: &Value) -> Result<Value, String> {
    let key = arguments
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter 'key'".to_string())?;

    let code = crate::input::try_key_name_to_code(key)
        .ok_or_else(|| format!("Unknown or unsupported key '{}'", key))?;

    inject_input_event(DesktopInputEvent::KeyUp {
        key_code: code,
        key: key.to_string(),
    })?;

    Ok(json!({
        "success": true,
        "action": "key_up",
        "key": key,
        "key_code": code
    }))
}

/// Executes multi-key shortcut / hotkey (e.g. ["ctrl", "c"], ["alt", "tab"])
pub fn execute_hotkey(arguments: &Value) -> Result<Value, String> {
    let keys_val = arguments
        .get("keys")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Missing required array parameter 'keys'".to_string())?;

    let keys: Vec<String> = keys_val
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    if keys.is_empty() {
        return Err("Parameter 'keys' must not be empty".to_string());
    }

    let mut key_codes: Vec<(String, u32)> = Vec::with_capacity(keys.len());
    for k in &keys {
        let code = crate::input::try_key_name_to_code(k)
            .ok_or_else(|| format!("Unknown or unsupported key in hotkey: '{}'", k))?;
        key_codes.push((k.clone(), code));
    }

    // 1. Press all keys down in order
    for (k, code) in &key_codes {
        inject_input_event(DesktopInputEvent::KeyDown {
            key_code: *code,
            key: k.clone(),
        })?;
        std::thread::sleep(Duration::from_millis(15));
    }

    std::thread::sleep(Duration::from_millis(20));

    // 2. Release all keys in reverse order
    for (k, code) in key_codes.iter().rev() {
        inject_input_event(DesktopInputEvent::KeyUp {
            key_code: *code,
            key: k.clone(),
        })?;
        std::thread::sleep(Duration::from_millis(15));
    }

    Ok(json!({
        "success": true,
        "action": "hotkey",
        "keys": keys
    }))
}
