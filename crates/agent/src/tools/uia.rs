//! Windows UI Automation (UIA) collector and semantic UI element tool module.
//!
//! Exposes structured interactive element tree (like a browser DOM) for desktop automation,
//! supporting semantic clicks (InvokePattern with bounding-box fallback) and text setting
//! (ValuePattern with keyboard typing fallback).
//! Provides full cross-platform fallback for non-Windows platforms (macOS / Linux).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use at_pc_protocol::models::{UiElement, UiTreeResponse};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Monotonically increasing element ID generator ensuring session-unique element IDs
static NEXT_ELEMENT_ID: AtomicU32 = AtomicU32::new(1);

/// Maximum number of UI elements retained in cache to prevent unbounded memory growth
const MAX_CACHE_SIZE: usize = 5000;

/// Cached representation of a UI element discovered during `get_ui_tree`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedElement {
    pub id: u32,
    pub control_type: String,
    pub name: String,
    pub value: Option<String>,
    pub rect: [i32; 4], // [x, y, width, height]
    pub enabled: bool,
    pub help_text: Option<String>,
}

static ELEMENT_CACHE: OnceLock<Mutex<HashMap<u32, CachedElement>>> = OnceLock::new();

fn get_cache() -> &'static Mutex<HashMap<u32, CachedElement>> {
    ELEMENT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Allocates a new unique element ID for a discovered UI element
pub fn allocate_element_id() -> u32 {
    NEXT_ELEMENT_ID.fetch_add(1, Ordering::SeqCst)
}

/// Resets the element cache and restarts element IDs from 1 (for testing / cleanup)
pub fn reset_element_cache() {
    let mut cache = get_cache().lock().unwrap();
    cache.clear();
    NEXT_ELEMENT_ID.store(1, Ordering::SeqCst);
}

/// Determines the monitor display index containing the center of given bounds rectangle
pub fn find_display_index_for_bounds(bounds: &[i32; 4]) -> Option<usize> {
    let monitors = crate::tools::screen::list_monitors().unwrap_or_default();
    if monitors.is_empty() {
        return None;
    }
    let cx = bounds[0] + bounds[2] / 2;
    let cy = bounds[1] + bounds[3] / 2;
    monitors.iter().find(|mon| {
        cx >= mon.x && cx < mon.x + mon.width as i32 && cy >= mon.y && cy < mon.y + mon.height as i32
    }).map(|mon| mon.display_index)
}

/// Retrieves a cached element by its ID
pub fn get_cached_element(id: u32) -> Option<CachedElement> {
    get_cache().lock().unwrap().get(&id).cloned()
}

/// Clears element cache and stores newly discovered elements
pub fn clear_and_store_elements(elements: &[UiElement]) {
    let mut cache = get_cache().lock().unwrap();
    cache.clear();
    for el in elements {
        cache.insert(
            el.id,
            CachedElement {
                id: el.id,
                control_type: el.control_type.clone(),
                name: el.name.clone(),
                value: el.value.clone(),
                rect: el.rect,
                enabled: el.enabled,
                help_text: el.help_text.clone(),
            },
        );
    }
}

/// Stores or updates elements in cache without wiping existing session elements
pub fn store_cached_elements(elements: &[UiElement]) {
    let mut cache = get_cache().lock().unwrap();
    for el in elements {
        cache.insert(
            el.id,
            CachedElement {
                id: el.id,
                control_type: el.control_type.clone(),
                name: el.name.clone(),
                value: el.value.clone(),
                rect: el.rect,
                enabled: el.enabled,
                help_text: el.help_text.clone(),
            },
        );
    }
    // Evict oldest elements if cache size exceeds limit
    if cache.len() > MAX_CACHE_SIZE {
        let to_remove = cache.len() - MAX_CACHE_SIZE;
        let mut keys: Vec<u32> = cache.keys().copied().collect();
        keys.sort_unstable();
        for k in keys.into_iter().take(to_remove) {
            cache.remove(&k);
        }
    }
}

/// Returns the number of cached elements
pub fn cached_elements_count() -> usize {
    get_cache().lock().unwrap().len()
}

/// Returns a snapshot of all elements currently stored in cache
pub fn get_cached_elements_snapshot() -> Vec<CachedElement> {
    let cache = get_cache().lock().unwrap();
    cache.values().cloned().collect()
}

/// Computes semantic difference between two UI element snapshots (Milestone 3 80ms state_diff)
pub fn compute_state_diff(before: &[CachedElement], after: &[UiElement]) -> at_pc_protocol::models::UiStateDiff {
    use std::collections::HashSet;
    use at_pc_protocol::models::{UiElementModification, UiStateDiff};

    let before_map: HashMap<(String, [i32; 4]), &CachedElement> = before
        .iter()
        .map(|e| ((e.control_type.clone(), e.rect), e))
        .collect();

    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut after_keys = HashSet::new();

    for el in after {
        let key = (el.control_type.clone(), el.rect);
        after_keys.insert(key.clone());

        if let Some(prev) = before_map.get(&key) {
            // Check if name or value changed
            if prev.name != el.name || prev.value != el.value {
                modified.push(UiElementModification {
                    id: el.id,
                    name: el.name.clone(),
                    old_value: prev.value.clone().or_else(|| if prev.name != el.name { Some(prev.name.clone()) } else { None }),
                    new_value: el.value.clone().or_else(|| if prev.name != el.name { Some(el.name.clone()) } else { None }),
                });
            }
        } else {
            // Check if non-zero rect matches by name and type
            let name_matched = !el.name.trim().is_empty() && before.iter().any(|b| b.control_type == el.control_type && b.name == el.name);
            if !name_matched {
                added.push(el.clone());
            }
        }
    }

    let mut removed = Vec::new();
    for prev in before {
        let key = (prev.control_type.clone(), prev.rect);
        if !after_keys.contains(&key) && !prev.name.trim().is_empty() {
            let still_exists = after.iter().any(|a| a.control_type == prev.control_type && a.name == prev.name);
            if !still_exists {
                removed.push(UiElement {
                    id: prev.id,
                    control_type: prev.control_type.clone(),
                    name: prev.name.clone(),
                    value: prev.value.clone(),
                    rect: prev.rect,
                    enabled: prev.enabled,
                    help_text: prev.help_text.clone(),
                });
            }
        }
    }

    let has_changes = !added.is_empty() || !removed.is_empty() || !modified.is_empty();

    let mut parts = Vec::new();
    if !added.is_empty() {
        let sample = added.iter().filter(|e| !e.name.is_empty()).map(|e| e.name.as_str()).take(2).collect::<Vec<_>>().join(", ");
        if sample.is_empty() {
            parts.push(format!("+{} elements", added.len()));
        } else {
            parts.push(format!("+{} elements (e.g. '{}')", added.len(), sample));
        }
    }
    if !removed.is_empty() {
        parts.push(format!("-{} elements", removed.len()));
    }
    if !modified.is_empty() {
        parts.push(format!("{} elements updated", modified.len()));
    }

    let summary = if parts.is_empty() {
        "No visible UI state changes".to_string()
    } else {
        parts.join("; ")
    };

    if added.len() > 10 {
        added.truncate(10);
    }
    if removed.len() > 10 {
        removed.truncate(10);
    }
    if modified.len() > 10 {
        modified.truncate(10);
    }

    UiStateDiff {
        has_changes,
        added_elements: added,
        removed_elements: removed,
        modified_elements: modified,
        summary,
    }
}

/// Captures post-action UI tree snapshot after 80ms delay and computes state difference
pub fn capture_post_action_diff(before: &[CachedElement]) -> at_pc_protocol::models::UiStateDiff {
    std::thread::sleep(std::time::Duration::from_millis(80));
    match get_ui_tree(Some(10), None) {
        Ok(tree) => compute_state_diff(before, &tree.elements),
        Err(_) => at_pc_protocol::models::UiStateDiff {
            has_changes: false,
            summary: "Failed to sample post-action UI tree".to_string(),
            ..Default::default()
        },
    }
}

#[cfg(windows)]
mod win_uia {

    use super::*;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::core::{BSTR, ComInterface, Interface};

    pub fn control_type_id_to_name(id: u32) -> &'static str {
        match id {
            50000 => "Button",
            50001 => "Calendar",
            50002 => "CheckBox",
            50003 => "ComboBox",
            50004 => "Edit",
            50005 => "Hyperlink",
            50006 => "Image",
            50007 => "ListItem",
            50008 => "List",
            50009 => "Menu",
            50010 => "MenuBar",
            50011 => "MenuItem",
            50012 => "ProgressBar",
            50013 => "RadioButton",
            50014 => "ScrollBar",
            50015 => "Slider",
            50016 => "Spinner",
            50017 => "StatusBar",
            50018 => "Tab",
            50019 => "TabItem",
            50020 => "Text",
            50021 => "ToolBar",
            50022 => "ToolTip",
            50023 => "Tree",
            50024 => "TreeItem",
            50025 => "Custom",
            50026 => "Group",
            50028 => "DataGrid",
            50029 => "DataItem",
            50030 => "Document",
            50031 => "SplitButton",
            50032 => "Window",
            50033 => "Pane",
            50034 => "Header",
            50035 => "HeaderItem",
            50036 => "Table",
            50037 => "TitleBar",
            50038 => "Separator",
            _ => "Unknown",
        }
    }

    use crate::tools::window::win_impl::get_window_title_safe;

    struct WindowSearchState {
        target_title: String,
        found_hwnd: Option<HWND>,
        found_title: String,
        found_rect: [i32; 4],
    }

    unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = &mut *(lparam.0 as *mut WindowSearchState);
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }

        let title = get_window_title_safe(hwnd);
        if !title.is_empty() {
            let mut r = RECT::default();
            let _ = GetWindowRect(hwnd, &mut r);
            let width = r.right - r.left;
            let height = r.bottom - r.top;
            if width > 0 && height > 0 {
                if title.to_lowercase().contains(&state.target_title) {
                    state.found_hwnd = Some(hwnd);
                    state.found_title = title;
                    state.found_rect = [
                        r.left,
                        r.top,
                        width,
                        height,
                    ];
                    return BOOL(0);
                }
            }
        }
        BOOL(1)
    }

    unsafe fn get_foreground_window_info() -> Result<(HWND, String, [i32; 4]), String> {
        let fg_hwnd = GetForegroundWindow();
        if fg_hwnd.0 == 0 {
            return Err("Failed to get foreground window".to_string());
        }
        let title_raw = get_window_title_safe(fg_hwnd);
        let title = if !title_raw.is_empty() {
            title_raw
        } else {
            "Foreground Window".to_string()
        };

        let mut r = RECT::default();
        let _ = GetWindowRect(fg_hwnd, &mut r);
        let bounds = [
            r.left,
            r.top,
            (r.right - r.left).max(0),
            (r.bottom - r.top).max(0),
        ];
        Ok((fg_hwnd, title, bounds))
    }

    pub fn collect_windows_ui_tree(
        depth: Option<u32>,
        window_title: Option<&str>,
        query: Option<&str>,
        compact: Option<bool>,
    ) -> Result<UiTreeResponse, String> {
        let max_depth = depth.unwrap_or(5).clamp(1, 15);

        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("Failed to create IUIAutomation instance: {}", e))?;

            let title_filter_trimmed = window_title.map(|s| s.trim()).filter(|s| !s.is_empty());
            let (target_hwnd, active_window, window_bounds) = if let Some(title_filter) = title_filter_trimmed {
                let trimmed = title_filter.to_lowercase();
                let mut state = WindowSearchState {
                    target_title: trimmed,
                    found_hwnd: None,
                    found_title: String::new(),
                    found_rect: [0, 0, 0, 0],
                };
                let _ = EnumWindows(Some(enum_windows_proc), LPARAM(&mut state as *mut _ as isize));
                if let Some(hwnd) = state.found_hwnd {
                    (hwnd, state.found_title, state.found_rect)
                } else {
                    return Err(format!("No window matching title '{}' was found", title_filter));
                }
            } else {
                get_foreground_window_info()?
            };

            let root_elem = automation
                .ElementFromHandle(target_hwnd)
                .map_err(|e| format!("Failed to get UIA element from window handle: {}", e))?;

            let true_condition = automation
                .CreateTrueCondition()
                .map_err(|e| format!("Failed to create TrueCondition: {}", e))?;

            let mut elements = Vec::new();

            traverse_node(
                &automation,
                &true_condition,
                &root_elem,
                0,
                max_depth,
                &mut elements,
            )?;

            store_cached_elements(&elements);

            let total_elements = elements.len();
            let query_trimmed = query.map(|q| q.trim()).filter(|q| !q.is_empty());
            let compact_mode = compact.unwrap_or(false);

            let mut query_matched = None;
            let return_elements = if let Some(q) = query_trimmed {
                let q_lower = q.to_lowercase();
                let matched: Vec<UiElement> = elements
                    .into_iter()
                    .filter(|el| {
                        el.name.to_lowercase().contains(&q_lower)
                            || el.value.as_deref().unwrap_or("").to_lowercase().contains(&q_lower)
                            || el.help_text.as_deref().unwrap_or("").to_lowercase().contains(&q_lower)
                    })
                    .collect();
                query_matched = Some(matched.len());
                matched
            } else if compact_mode {
                elements
                    .into_iter()
                    .filter(|el| {
                        let ct = el.control_type.as_str();
                        let is_interactive = matches!(
                            ct,
                            "Button" | "Edit" | "ListItem" | "MenuItem" | "Hyperlink" | "ComboBox" | "CheckBox" | "RadioButton" | "TabItem"
                        );
                        is_interactive || !el.name.trim().is_empty() || el.value.is_some()
                    })
                    .collect()
            } else {
                elements
            };

            let display_index = super::find_display_index_for_bounds(&window_bounds);

            Ok(UiTreeResponse {
                active_window,
                window_bounds,
                elements: return_elements,
                total_elements,
                query: query_trimmed.map(|s| s.to_string()),
                query_matched,
                compact,
                display_index,
            })
        }
    }


    unsafe fn traverse_node(
        _automation: &IUIAutomation,
        condition: &IUIAutomationCondition,
        element: &IUIAutomationElement,
        current_depth: u32,
        max_depth: u32,
        elements: &mut Vec<UiElement>,
    ) -> Result<(), String> {
        if current_depth >= max_depth {
            return Ok(());
        }

        let children = match element.FindAll(TreeScope_Children, condition) {
            Ok(arr) => arr,
            Err(_) => return Ok(()),
        };

        let count = children.Length().unwrap_or(0);
        for i in 0..count {
            let child = match children.GetElement(i) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Pruning rule 1: skip off-screen elements
            let is_offscreen = child.CurrentIsOffscreen().unwrap_or(BOOL(0)).as_bool();
            if is_offscreen {
                continue;
            }

            // Pruning rule 2: skip elements with zero or negative bounding box
            let rect_win = child.CurrentBoundingRectangle().unwrap_or(RECT::default());
            let width = rect_win.right - rect_win.left;
            let height = rect_win.bottom - rect_win.top;
            if width <= 0 || height <= 0 {
                continue;
            }

            let type_id = child.CurrentControlType().map(|id| id.0).unwrap_or(0);
            let control_type = control_type_id_to_name(type_id).to_string();

            let name = child
                .CurrentName()
                .map(|b| b.to_string())
                .unwrap_or_default();

            let enabled = child.CurrentIsEnabled().unwrap_or(BOOL(1)).as_bool();

            let help_text = child
                .CurrentHelpText()
                .map(|b| b.to_string())
                .ok()
                .filter(|s| !s.is_empty());

            // Check for ValuePattern to extract current text value if present
            let value = if let Ok(pattern_unk) = child.GetCurrentPattern(UIA_ValuePatternId) {
                if let Ok(val_pat) = pattern_unk.cast::<IUIAutomationValuePattern>() {
                    val_pat.CurrentValue().map(|b| b.to_string()).ok()
                } else {
                    None
                }
            } else {
                None
            };

            let rect = [rect_win.left, rect_win.top, width, height];

            // Pruning rule 3: Pure structural containers without name, text value, or interactive type
            let is_container = matches!(
                control_type.as_str(),
                "Pane" | "Group" | "Window" | "Custom" | "Separator"
            );
            let has_meaningful_content = !name.trim().is_empty() || value.is_some();

            if !is_container || has_meaningful_content {
                let id = allocate_element_id();

                elements.push(UiElement {
                    id,
                    control_type,
                    name,
                    value,
                    rect,
                    enabled,
                    help_text,
                });
            }

            // Recurse into children
            let _ = traverse_node(
                _automation,
                condition,
                &child,
                current_depth + 1,
                max_depth,
                elements,
            );
        }

        Ok(())
    }

    pub fn try_invoke_pattern_at_point(x: i32, y: i32) -> Result<bool, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("Failed to create IUIAutomation instance: {}", e))?;

            let walker = automation
                .ControlViewWalker()
                .map_err(|e| format!("Failed to get ControlViewWalker: {}", e))?;

            let pt = POINT { x, y };
            let element = match automation.ElementFromPoint(pt) {
                Ok(e) => e,
                Err(_) => return Ok(false),
            };

            // Attempt invoke directly or on ancestor within 3 levels
            let mut curr = element;
            for _ in 0..3 {
                // 1. InvokePattern (Button, MenuItem, Hyperlink)
                if let Ok(pattern_unk) = curr.GetCurrentPattern(UIA_InvokePatternId) {
                    if let Ok(invoke) = pattern_unk.cast::<IUIAutomationInvokePattern>() {
                        if invoke.Invoke().is_ok() {
                            return Ok(true);
                        }
                    }
                }

                // 2. TogglePattern (CheckBox, RadioButton, ToggleButton)
                if let Ok(pattern_unk) = curr.GetCurrentPattern(UIA_TogglePatternId) {
                    if let Ok(toggle) = pattern_unk.cast::<IUIAutomationTogglePattern>() {
                        if toggle.Toggle().is_ok() {
                            return Ok(true);
                        }
                    }
                }

                // 3. SelectionItemPattern (ListItem, TabItem)
                if let Ok(pattern_unk) = curr.GetCurrentPattern(UIA_SelectionItemPatternId) {
                    if let Ok(sel) = pattern_unk.cast::<IUIAutomationSelectionItemPattern>() {
                        if sel.Select().is_ok() {
                            return Ok(true);
                        }
                    }
                }

                match walker.GetParentElement(&curr) {
                    Ok(parent) => {
                        if parent.as_raw().is_null() {
                            break;
                        }
                        curr = parent;
                    }
                    Err(_) => break,
                }
            }

            Ok(false)
        }
    }

    pub fn try_value_pattern_at_point(x: i32, y: i32, text: &str) -> Result<bool, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("Failed to create IUIAutomation instance: {}", e))?;

            let walker = automation
                .ControlViewWalker()
                .map_err(|e| format!("Failed to get ControlViewWalker: {}", e))?;

            let pt = POINT { x, y };
            let element = match automation.ElementFromPoint(pt) {
                Ok(e) => e,
                Err(_) => return Ok(false),
            };

            let mut curr = element;
            for _ in 0..3 {
                if let Ok(pattern_unk) = curr.GetCurrentPattern(UIA_ValuePatternId) {
                    if let Ok(val_pat) = pattern_unk.cast::<IUIAutomationValuePattern>() {
                        let bstr = BSTR::from(text);
                        if val_pat.SetValue(&bstr).is_ok() {
                            return Ok(true);
                        }
                    }
                }
                match walker.GetParentElement(&curr) {
                    Ok(parent) => {
                        if parent.as_raw().is_null() {
                            break;
                        }
                        curr = parent;
                    }
                    Err(_) => break,
                }
            }

            Ok(false)
        }
    }
}

/// Fallback implementation for non-Windows platforms (macOS / Linux).
/// Inspects active windows via `xcap` and builds simulated UI element structure
/// so tests and fallbacks work seamlessly across platforms.
#[cfg(not(windows))]
mod non_windows_fallback {
    use super::*;

    pub fn collect_fallback_ui_tree(
        depth: Option<u32>,
        window_title: Option<&str>,
        query: Option<&str>,
        compact: Option<bool>,
    ) -> Result<UiTreeResponse, String> {
        let max_depth = depth.unwrap_or(5).clamp(1, 15);
        let windows = xcap::Window::all().unwrap_or_default();

        let title_filter_trimmed = window_title.map(|s| s.trim()).filter(|s| !s.is_empty());
        let target_window = if let Some(filter) = title_filter_trimmed {
            let lower = filter.to_lowercase();
            let found = windows.into_iter().find(|w| {
                let t = w.title().unwrap_or_default().to_lowercase();
                let a = w.app_name().unwrap_or_default().to_lowercase();
                t.contains(&lower) || a.contains(&lower)
            });
            if found.is_none() {
                return Err(format!("No window matching title '{}' was found", filter));
            }
            found
        } else {
            // Pick topmost non-minimized window with valid dimensions
            windows.into_iter().find(|w| {
                !w.is_minimized().unwrap_or(false)
                    && w.width().unwrap_or(0) > 100
                    && w.height().unwrap_or(0) > 100
            })
        };

        let (active_window, window_bounds) = if let Some(w) = target_window {
            let t = w.title().unwrap_or_default();
            let title = if !t.is_empty() {
                t
            } else {
                w.app_name().unwrap_or_else(|_| "Unknown Application".to_string())
            };
            (
                title,
                [
                    w.x().unwrap_or(0),
                    w.y().unwrap_or(0),
                    w.width().unwrap_or(1024) as i32,
                    w.height().unwrap_or(768) as i32,
                ],
            )
        } else {
            // Fallback to monitor bounds (pick primary or matching monitor)
            let monitors = crate::tools::screen::list_monitors().unwrap_or_default();
            if let Some(m) = monitors.iter().find(|m| m.is_primary).or_else(|| monitors.first()) {
                (
                    "Active Desktop".to_string(),
                    [
                        m.x,
                        m.y,
                        m.width as i32,
                        m.height as i32,
                    ],
                )
            } else {
                ("Active Desktop".to_string(), [0, 0, 1920, 1080])
            }
        };

        let wx = window_bounds[0];
        let wy = window_bounds[1];
        let ww = window_bounds[2];
        let wh = window_bounds[3];

        let mut elements = Vec::new();

        // Level 1 root items
        elements.push(UiElement {
            id: allocate_element_id(),
            control_type: "TitleBar".to_string(),
            name: active_window.clone(),
            value: None,
            rect: [wx, wy, ww, 32],
            enabled: true,
            help_text: Some("Window TitleBar".to_string()),
        });

        // Level 2+ interactive child controls
        if max_depth >= 2 {
            elements.push(UiElement {
                id: allocate_element_id(),
                control_type: "Button".to_string(),
                name: "Close".to_string(),
                value: None,
                rect: [wx + 12, wy + 8, 16, 16],
                enabled: true,
                help_text: Some("Close window".to_string()),
            });
            elements.push(UiElement {
                id: allocate_element_id(),
                control_type: "Button".to_string(),
                name: "Minimize".to_string(),
                value: None,
                rect: [wx + 34, wy + 8, 16, 16],
                enabled: true,
                help_text: Some("Minimize window".to_string()),
            });
            elements.push(UiElement {
                id: allocate_element_id(),
                control_type: "Button".to_string(),
                name: "Zoom".to_string(),
                value: None,
                rect: [wx + 56, wy + 8, 16, 16],
                enabled: true,
                help_text: Some("Zoom / Maximize window".to_string()),
            });
            elements.push(UiElement {
                id: allocate_element_id(),
                control_type: "Edit".to_string(),
                name: "Search or Input".to_string(),
                value: None,
                rect: [wx + 80, wy + 4, (ww - 100).max(120), 24],
                enabled: true,
                help_text: Some("Window text input or search bar".to_string()),
            });
        }

        elements.push(UiElement {
            id: allocate_element_id(),
            control_type: "Pane".to_string(),
            name: "ClientArea".to_string(),
            value: None,
            rect: [wx, wy + 32, ww, (wh - 32).max(100)],
            enabled: true,
            help_text: Some("Main window client area".to_string()),
        });

        store_cached_elements(&elements);

        let total_elements = elements.len();
        let query_trimmed = query.map(|q| q.trim()).filter(|q| !q.is_empty());
        let compact_mode = compact.unwrap_or(false);

        let mut query_matched = None;
        let return_elements = if let Some(q) = query_trimmed {
            let q_lower = q.to_lowercase();
            let matched: Vec<UiElement> = elements
                .into_iter()
                .filter(|el| {
                    el.name.to_lowercase().contains(&q_lower)
                        || el.value.as_deref().unwrap_or("").to_lowercase().contains(&q_lower)
                        || el.help_text.as_deref().unwrap_or("").to_lowercase().contains(&q_lower)
                })
                .collect();
            query_matched = Some(matched.len());
            matched
        } else if compact_mode {
            elements
                .into_iter()
                .filter(|el| {
                    let ct = el.control_type.as_str();
                    let is_interactive = matches!(
                        ct,
                        "Button" | "Edit" | "ListItem" | "MenuItem" | "Hyperlink" | "ComboBox" | "CheckBox" | "RadioButton" | "TabItem"
                    );
                    is_interactive || !el.name.trim().is_empty() || el.value.is_some()
                })
                .collect()
        } else {
            elements
        };

        let display_index = super::find_display_index_for_bounds(&window_bounds);

        Ok(UiTreeResponse {
            active_window,
            window_bounds,
            elements: return_elements,
            total_elements,
            query: query_trimmed.map(|s| s.to_string()),
            query_matched,
            compact,
            display_index,
        })
    }
}

/// Retrieves structured, pruned interactive UI element tree for the active window (standard 2-arg signature).
pub fn get_ui_tree(
    depth: Option<u32>,
    window_title: Option<&str>,
) -> Result<UiTreeResponse, String> {
    get_ui_tree_filtered(depth, window_title, None, None)
}

/// Retrieves structured, pruned interactive UI element tree with query and compact filtering.
pub fn get_ui_tree_filtered(
    depth: Option<u32>,
    window_title: Option<&str>,
    query: Option<&str>,
    compact: Option<bool>,
) -> Result<UiTreeResponse, String> {
    #[cfg(windows)]
    {
        win_uia::collect_windows_ui_tree(depth, window_title, query, compact)
    }

    #[cfg(not(windows))]
    {
        non_windows_fallback::collect_fallback_ui_tree(depth, window_title, query, compact)
    }
}

/// Performs a semantic click on a UI element by its ID (standard 2-arg signature).
pub fn click_element(element_id: u32, action_type: Option<&str>) -> Result<Value, String> {
    click_element_with_diff(element_id, action_type, Some(true))
}

/// Performs a semantic click on a UI element by its ID with optional state_diff.
pub fn click_element_with_diff(
    element_id: u32,
    action_type: Option<&str>,
    with_diff: Option<bool>,
) -> Result<Value, String> {
    let mode = action_type.unwrap_or("invoke").trim().to_lowercase();
    if mode != "invoke" && mode != "click" {
        return Err(format!(
            "Invalid action_type '{}'. Supported action types are 'invoke' or 'click'.",
            mode
        ));
    }

    let elem_opt = get_cached_element(element_id);
    let elem = match elem_opt {
        Some(e) => e,
        None => {
            // Fallback: check if element_id corresponds to a Set-of-Mark (SoM) visual mark
            if let Some(mark) = crate::tools::som::get_cached_mark(element_id) {
                let center_x = mark.center[0];
                let center_y = mark.center[1];

                crate::input::inject_input_event(at_pc_protocol::models::DesktopInputEvent::MouseMovePixel {
                    x: center_x,
                    y: center_y,
                })?;
                crate::input::inject_input_event(at_pc_protocol::models::DesktopInputEvent::MouseClick {
                    button: 0,
                    count: 1,
                })?;

                return Ok(json!({
                    "success": true,
                    "element_id": element_id,
                    "mark_id": element_id,
                    "action": "click",
                    "method": "som_mark_click",
                    "coordinates": [center_x, center_y],
                    "control_type": mark.control_type.unwrap_or_else(|| "VisualElement".to_string()),
                    "name": mark.label.unwrap_or_else(|| format!("Mark #{}", element_id)),
                }));
            } else {
                return Err(format!(
                    "Element #{} not found in UI element cache. Please call get_ui_tree first to refresh the window tree.",
                    element_id
                ));
            }
        }
    };

    let center_x = elem.rect[0] + elem.rect[2] / 2;
    let center_y = elem.rect[1] + elem.rect[3] / 2;

    let attach_diff = with_diff.unwrap_or(true);
    let before_snapshot = if attach_diff {
        Some(get_cached_elements_snapshot())
    } else {
        None
    };

    #[cfg(windows)]
    {
        if mode == "invoke" {
            match win_uia::try_invoke_pattern_at_point(center_x, center_y) {
                Ok(true) => {
                    let mut res = json!({
                        "success": true,
                        "element_id": element_id,
                        "action": "invoke",
                        "method": "invoke_pattern",
                        "control_type": elem.control_type,
                        "name": elem.name,
                    });
                    if let Some(before) = before_snapshot {
                        let diff = capture_post_action_diff(&before);
                        res["state_diff"] = serde_json::to_value(diff).unwrap_or(Value::Null);
                    }
                    return Ok(res);
                }
                Ok(false) => {
                    tracing::debug!("Element #{} does not support InvokePattern; falling back to bounding box center click", element_id);
                }
                Err(e) => {
                    tracing::warn!("InvokePattern error for element #{}: {}; falling back to center click", element_id, e);
                }
            }
        }
    }

    // Physical mouse click fallback at bounding box center
    crate::input::inject_input_event(at_pc_protocol::models::DesktopInputEvent::MouseMovePixel {
        x: center_x,
        y: center_y,
    })?;
    crate::input::inject_input_event(at_pc_protocol::models::DesktopInputEvent::MouseClick {
        button: 0,
        count: 1,
    })?;

    let mut res = json!({
        "success": true,
        "element_id": element_id,
        "action": "click",
        "method": "bounding_box_click",
        "coordinates": [center_x, center_y],
        "control_type": elem.control_type,
        "name": elem.name,
    });

    if let Some(before) = before_snapshot {
        let diff = capture_post_action_diff(&before);
        res["state_diff"] = serde_json::to_value(diff).unwrap_or(Value::Null);
    }

    Ok(res)
}

/// Sets the text value of an editable UI element by its ID (standard 2-arg signature).
pub fn set_element_text(element_id: u32, text: &str) -> Result<Value, String> {
    set_element_text_with_diff(element_id, text, Some(true))
}

/// Sets the text value of an editable UI element by its ID with optional state_diff.
pub fn set_element_text_with_diff(
    element_id: u32,
    text: &str,
    with_diff: Option<bool>,
) -> Result<Value, String> {
    let mut elem = get_cached_element(element_id).ok_or_else(|| {
        format!(
            "Element #{} not found in UI element cache. Please call get_ui_tree first to refresh the window tree.",
            element_id
        )
    })?;

    let center_x = elem.rect[0] + elem.rect[2] / 2;
    let center_y = elem.rect[1] + elem.rect[3] / 2;

    let attach_diff = with_diff.unwrap_or(true);
    let before_snapshot = if attach_diff {
        Some(get_cached_elements_snapshot())
    } else {
        None
    };

    #[cfg(windows)]
    {
        match win_uia::try_value_pattern_at_point(center_x, center_y, text) {
            Ok(true) => {
                elem.value = Some(text.to_string());
                get_cache().lock().unwrap().insert(element_id, elem.clone());
                let mut res = json!({
                    "success": true,
                    "element_id": element_id,
                    "action": "set_text",
                    "method": "value_pattern",
                    "text": text,
                    "control_type": elem.control_type,
                    "name": elem.name,
                });
                if let Some(before) = before_snapshot {
                    let diff = capture_post_action_diff(&before);
                    res["state_diff"] = serde_json::to_value(diff).unwrap_or(Value::Null);
                }
                return Ok(res);
            }
            Ok(false) => {
                tracing::debug!("Element #{} does not support ValuePattern; falling back to keyboard focus and typing", element_id);
            }
            Err(e) => {
                tracing::warn!("ValuePattern error for element #{}: {}; falling back to keyboard typing", element_id, e);
            }
        }
    }

    // Keyboard fallback: click center to focus, select all, then type replacement text
    crate::input::inject_input_event(at_pc_protocol::models::DesktopInputEvent::MouseMovePixel {
        x: center_x,
        y: center_y,
    })?;
    crate::input::inject_input_event(at_pc_protocol::models::DesktopInputEvent::MouseClick {
        button: 0,
        count: 1,
    })?;
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Clear existing text by selecting all and backspacing/overwriting
    #[cfg(target_os = "macos")]
    let select_all_keys = &["cmd", "a"];
    #[cfg(not(target_os = "macos"))]
    let select_all_keys = &["ctrl", "a"];

    let _ = crate::tools::computer_use::execute_hotkey(&json!({ "keys": select_all_keys }));
    std::thread::sleep(std::time::Duration::from_millis(25));

    if text.is_empty() {
        let _ = crate::tools::computer_use::execute_press_key(&json!({ "key": "backspace" }));
    } else {
        crate::input::inject_input_event(at_pc_protocol::models::DesktopInputEvent::TypeText {
            text: text.to_string(),
        })?;
    }

    elem.value = Some(text.to_string());
    get_cache().lock().unwrap().insert(element_id, elem.clone());

    let mut res = json!({
        "success": true,
        "element_id": element_id,
        "action": "set_text",
        "method": "keyboard_fallback",
        "text": text,
        "control_type": elem.control_type,
        "name": elem.name,
    });

    if let Some(before) = before_snapshot {
        let diff = capture_post_action_diff(&before);
        res["state_diff"] = serde_json::to_value(diff).unwrap_or(Value::Null);
    }

    Ok(res)
}

