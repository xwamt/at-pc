use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::fmt;
use std::sync::LazyLock;

/// Role required to invoke a tool. Roles are ordered from least to most privileged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Operator,
    Admin,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Viewer => "viewer",
            Self::Operator => "operator",
            Self::Admin => "admin",
        })
    }
}

/// Security-relevant effect category. This is descriptive; `required_role` is authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRisk {
    ReadOnly,
    SystemMutation,
    ComputerControl,
}

/// Typed Agent dispatch key. Adding a tool requires updating every exhaustive consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolKind {
    GetSystemOverview,
    ExecPowershell,
    ExecCmd,
    ListProcesses,
    KillProcess,
    ManageService,
    ReadTextFile,
    WriteTextFile,
    CaptureScreen,
    ListMonitors,
    ListDirectory,
    SearchFiles,
    ListNetworkConnections,
    TestNetwork,
    GetEventLogs,
    MouseClick,
    MouseMove,
    MouseDrag,
    MouseScroll,
    TypeText,
    PressKey,
    KeyDown,
    KeyUp,
    Hotkey,
    GetUiTree,
    ClickElement,
    SetElementText,
    BatchActions,
    ListWindows,
    FocusWindow,
    CloseWindow,
    GetMarkedScreen,
    ClickMark,
}

pub const ALL_AGENT_TOOL_KINDS: [AgentToolKind; 33] = [
    AgentToolKind::GetSystemOverview,
    AgentToolKind::ExecPowershell,
    AgentToolKind::ExecCmd,
    AgentToolKind::ListProcesses,
    AgentToolKind::KillProcess,
    AgentToolKind::ManageService,
    AgentToolKind::ReadTextFile,
    AgentToolKind::WriteTextFile,
    AgentToolKind::CaptureScreen,
    AgentToolKind::ListMonitors,
    AgentToolKind::ListDirectory,
    AgentToolKind::SearchFiles,
    AgentToolKind::ListNetworkConnections,
    AgentToolKind::TestNetwork,
    AgentToolKind::GetEventLogs,
    AgentToolKind::MouseClick,
    AgentToolKind::MouseMove,
    AgentToolKind::MouseDrag,
    AgentToolKind::MouseScroll,
    AgentToolKind::TypeText,
    AgentToolKind::PressKey,
    AgentToolKind::KeyDown,
    AgentToolKind::KeyUp,
    AgentToolKind::Hotkey,
    AgentToolKind::GetUiTree,
    AgentToolKind::ClickElement,
    AgentToolKind::SetElementText,
    AgentToolKind::BatchActions,
    AgentToolKind::ListWindows,
    AgentToolKind::FocusWindow,
    AgentToolKind::CloseWindow,
    AgentToolKind::GetMarkedScreen,
    AgentToolKind::ClickMark,
];

impl AgentToolKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GetSystemOverview => "get_system_overview",
            Self::ExecPowershell => "exec_powershell",
            Self::ExecCmd => "exec_cmd",
            Self::ListProcesses => "list_processes",
            Self::KillProcess => "kill_process",
            Self::ManageService => "manage_service",
            Self::ReadTextFile => "read_text_file",
            Self::WriteTextFile => "write_text_file",
            Self::CaptureScreen => "capture_screen",
            Self::ListMonitors => "list_monitors",
            Self::ListDirectory => "list_directory",
            Self::SearchFiles => "search_files",
            Self::ListNetworkConnections => "list_network_connections",
            Self::TestNetwork => "test_network",
            Self::GetEventLogs => "get_event_logs",
            Self::MouseClick => "mouse_click",
            Self::MouseMove => "mouse_move",
            Self::MouseDrag => "mouse_drag",
            Self::MouseScroll => "mouse_scroll",
            Self::TypeText => "type_text",
            Self::PressKey => "press_key",
            Self::KeyDown => "key_down",
            Self::KeyUp => "key_up",
            Self::Hotkey => "hotkey",
            Self::GetUiTree => "get_ui_tree",
            Self::ClickElement => "click_element",
            Self::SetElementText => "set_element_text",
            Self::BatchActions => "batch_actions",
            Self::ListWindows => "list_windows",
            Self::FocusWindow => "focus_window",
            Self::CloseWindow => "close_window",
            Self::GetMarkedScreen => "get_marked_screen",
            Self::ClickMark => "click_mark",
        }
    }
}

/// Forward/dispatch classification for a shared tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchKind {
    Agent(AgentToolKind),
}

/// Canonical contract for one Agent-executed MCP tool.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub required_role: Role,
    pub risk: ToolRisk,
    pub dispatch: DispatchKind,
}

impl ToolSpec {
    pub fn as_mcp_definition(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": self.input_schema,
        })
    }

    pub const fn agent_kind(&self) -> AgentToolKind {
        match self.dispatch {
            DispatchKind::Agent(kind) => kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRegistryError(pub String);

impl fmt::Display for ToolRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ToolRegistryError {}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required })
}

fn spec(
    kind: AgentToolKind,
    description: &'static str,
    required_role: Role,
    risk: ToolRisk,
    mut properties: Value,
    required: &[&str],
) -> ToolSpec {
    let property_map = properties
        .as_object_mut()
        .expect("built-in tool properties must be an object");
    for (name, schema) in property_map {
        let schema = schema
            .as_object_mut()
            .expect("built-in tool property schema must be an object");
        schema.entry("description").or_insert_with(|| {
            Value::String(property_description(kind, name.as_str()).to_string())
        });
    }

    ToolSpec {
        name: kind.as_str(),
        description,
        input_schema: object(properties, required),
        required_role,
        risk,
        dispatch: DispatchKind::Agent(kind),
    }
}

fn property_description(kind: AgentToolKind, name: &str) -> &'static str {
    match (kind, name) {
        (_, "timeout_secs") => "Execution timeout in seconds (default: 30).",
        (_, "timeout_ms") => "Probe timeout in milliseconds (default: 3000).",
        (_, "cwd") => "Optional working directory.",
        (_, "limit") => "Maximum number of records to return.",
        (_, "display_index") => "Zero-based display index (default: 0); coordinate tools interpret coordinates relative to that monitor.",
        (_, "format") => "Image format: jpeg or png (default: jpeg).",
        (_, "quality") => "JPEG quality from 1 to 100 (default: 80).",
        (_, "max_dimension") => "Optional maximum image dimension used for downsampling.",
        (_, "crop") => "Optional [x, y, width, height] region in physical pixels.",
        (_, "coord_mode") => "Coordinate system: pixel (default), normalized 0-65535, or normalized_1000 0-1000.",
        (AgentToolKind::MouseScroll, "delta_y") => "Scroll delta; positive scrolls up and negative scrolls down.",
        (AgentToolKind::MouseClick | AgentToolKind::MouseDrag | AgentToolKind::ClickMark, "button") => "Mouse button: left (default), middle, or right.",
        (AgentToolKind::MouseClick | AgentToolKind::ClickMark, "count") => "Click count: 1 for single click or 2 for double click (default: 1).",
        (AgentToolKind::PressKey | AgentToolKind::KeyDown | AgentToolKind::KeyUp, "key") => "Key identifier such as enter, tab, escape, backspace, ctrl, alt, shift, an arrow key, or f1-f12.",
        (AgentToolKind::Hotkey, "keys") => "Ordered key names for a shortcut, for example [\"ctrl\", \"c\"].",
        (AgentToolKind::CaptureScreen, "save_path") => "Optional path on the Agent machine where the screenshot is saved.",
        (AgentToolKind::GetMarkedScreen, "strategy") => "Marking strategy: auto, ui_tree, grid, contours, or hybrid (default: auto).",
        (AgentToolKind::GetMarkedScreen, "grid_divisions") => "Grid row/column count when grid marking is used (default: 4).",
        (AgentToolKind::ListProcesses, "sort_by") => "Sort field: memory (default), cpu, pid, or name.",
        (AgentToolKind::ReadTextFile, "tail_lines") => "Read the last N lines (default: 200); use 0 for all lines.",
        (AgentToolKind::ReadTextFile, "max_bytes") => "Maximum bytes to return (default: 512000).",
        (AgentToolKind::WriteTextFile, "create_backup") => "Create a timestamped .bak file before writing (default: true).",
        (AgentToolKind::ListDirectory, "recursive") => "Whether to recurse into subdirectories (default: false).",
        (AgentToolKind::ListDirectory | AgentToolKind::SearchFiles, "max_depth") => "Maximum directory traversal depth.",
        (AgentToolKind::GetEventLogs, "hours_back") => "Look-back window in hours (default: 24).",
        (AgentToolKind::GetUiTree, "depth") => "Maximum UI tree traversal depth (default: 5).",
        (AgentToolKind::GetUiTree, "compact") => "Prune empty structural containers (default: false).",
        (AgentToolKind::ClickElement | AgentToolKind::SetElementText | AgentToolKind::BatchActions, "with_diff") => "Return an 80ms post-action UI state diff (default: true).",
        (AgentToolKind::BatchActions, "actions") => "Ordered action objects to execute sequentially.",
        (AgentToolKind::BatchActions, "delay_ms") => "Delay between actions in milliseconds (default: 40).",
        (AgentToolKind::ListWindows, "only_visible") => "Return only visible, titled, non-minimized windows (default: true).",
        (_, "pid") => "Process identifier.",
        (_, "hwnd") => "Native window handle.",
        (_, "title") => "Case-insensitive window-title substring.",
        (_, "x") => "Horizontal screen coordinate.",
        (_, "y") => "Vertical screen coordinate.",
        (_, "text") => "Text value.",
        (_, "file_path") => "Target file path.",
        (_, "path") => "Target directory path.",
        (_, "filter") => "Optional substring filter.",
        (_, "filter_name") => "Alias for filter.",
        (_, "state") => "Optional connection-state filter.",
        (_, "port") => "Optional TCP/UDP port number.",
        (_, "window_title") => "Optional title filter selecting a window.",
        (_, "mark_id") => "Set-of-Mark identifier returned by get_marked_screen.",
        (_, "element_id") => "UI element identifier returned by get_ui_tree.",
        _ => "Tool input parameter.",
    }
}

static TOOL_REGISTRY: LazyLock<Vec<ToolSpec>> = LazyLock::new(|| {
    use AgentToolKind as K;
    use Role::{Admin, Operator, Viewer};
    use ToolRisk::{ComputerControl, ReadOnly, SystemMutation};

    let specs = vec![
        spec(K::GetSystemOverview, "Returns complete hardware and operating system status.", Viewer, ReadOnly, json!({}), &[]),
        spec(K::ExecPowershell, "Executes a PowerShell script with timeout protection and output capture.", Admin, SystemMutation, json!({"script":{"type":"string"},"timeout_secs":{"type":"integer"},"cwd":{"type":"string"}}), &["script"]),
        spec(K::ExecCmd, "Executes a shell or Windows CMD command with timeout protection.", Admin, SystemMutation, json!({"command":{"type":"string"},"timeout_secs":{"type":"integer"},"cwd":{"type":"string"}}), &["command"]),
        spec(K::ListProcesses, "Lists running processes with PID, resource use, and binary path.", Viewer, ReadOnly, json!({"filter":{"type":"string"},"filter_name":{"type":"string"},"sort_by":{"type":"string","enum":["memory","cpu","pid","name"]},"limit":{"type":"integer"}}), &[]),
        spec(K::KillProcess, "Terminates a running process by PID or executable name.", Admin, SystemMutation, json!({"pid":{"type":"integer"},"name":{"type":"string"},"process_name":{"type":"string"},"force":{"type":"boolean"}}), &[]),
        spec(K::ManageService, "Inspects or controls a system service.", Operator, SystemMutation, json!({"service_name":{"type":"string"},"action":{"type":"string","enum":["status","start","stop","restart"]}}), &["service_name","action"]),
        spec(K::ReadTextFile, "Reads a text file with optional tail and byte limits.", Viewer, ReadOnly, json!({"file_path":{"type":"string"},"tail_lines":{"type":"integer"},"max_bytes":{"type":"integer"}}), &["file_path"]),
        spec(K::WriteTextFile, "Writes a text file with optional timestamped backup creation.", Admin, SystemMutation, json!({"file_path":{"type":"string"},"content":{"type":"string"},"create_backup":{"type":"boolean"}}), &["file_path","content"]),
        spec(K::CaptureScreen, "Captures a monitor screenshot with JPEG/PNG output, downsampling, cropping, and optional Agent-side saving; results include raw/image Base64 and a browser-ready data URI.", Viewer, ReadOnly, json!({"display_index":{"type":"integer"},"format":{"type":"string","enum":["jpeg","png"]},"quality":{"type":"integer"},"save_path":{"type":"string"},"max_dimension":{"type":"integer"},"crop":{"type":"array","items":{"type":"integer"}}}), &[]),
        spec(K::ListMonitors, "Lists connected physical and virtual display monitors, bounds, primary status, and DPI scale.", Viewer, ReadOnly, json!({}), &[]),
        spec(K::ListDirectory, "Lists directory contents and filesystem metadata.", Viewer, ReadOnly, json!({"path":{"type":"string"},"recursive":{"type":"boolean"},"max_depth":{"type":"integer"},"limit":{"type":"integer"}}), &["path"]),
        spec(K::SearchFiles, "Searches for files under a base directory.", Viewer, ReadOnly, json!({"base_path":{"type":"string"},"pattern":{"type":"string"},"max_results":{"type":"integer"},"max_depth":{"type":"integer"}}), &["base_path","pattern"]),
        spec(K::ListNetworkConnections, "Lists active TCP/UDP connections and listening ports.", Viewer, ReadOnly, json!({"state":{"type":"string"},"port":{"type":"integer"},"limit":{"type":"integer"}}), &[]),
        spec(K::TestNetwork, "Tests DNS, reachability, and optional TCP connectivity.", Viewer, ReadOnly, json!({"target_host":{"type":"string"},"port":{"type":"integer"},"timeout_ms":{"type":"integer"}}), &["target_host"]),
        spec(K::GetEventLogs, "Queries recent operating-system event logs.", Viewer, ReadOnly, json!({"log_name":{"type":"string"},"level":{"type":"string","enum":["Error","Critical","Warning","All"]},"hours_back":{"type":"integer"},"limit":{"type":"integer"}}), &[]),
        spec(K::MouseClick, "Clicks a mouse button at coordinates, the current cursor, or a mark.", Admin, ComputerControl, json!({"x":{"type":"integer"},"y":{"type":"integer"},"coord_mode":{"type":"string","enum":["pixel","normalized","normalized_1000"]},"button":{"type":"string","enum":["left","middle","right"]},"count":{"type":"integer"},"mark_id":{"type":"integer"},"display_index":{"type":"integer"}}), &[]),
        spec(K::MouseMove, "Moves the mouse cursor to screen coordinates.", Admin, ComputerControl, json!({"x":{"type":"integer"},"y":{"type":"integer"},"coord_mode":{"type":"string","enum":["pixel","normalized","normalized_1000"]},"display_index":{"type":"integer"}}), &["x","y"]),
        spec(K::MouseDrag, "Drags a mouse button from an optional start to an end coordinate.", Admin, ComputerControl, json!({"start_x":{"type":"integer"},"start_y":{"type":"integer"},"end_x":{"type":"integer"},"end_y":{"type":"integer"},"coord_mode":{"type":"string","enum":["pixel","normalized","normalized_1000"]},"button":{"type":"string","enum":["left","middle","right"]},"display_index":{"type":"integer"}}), &["end_x","end_y"]),
        spec(K::MouseScroll, "Rotates the mouse wheel at the current or specified coordinate.", Admin, ComputerControl, json!({"delta_y":{"type":"integer"},"x":{"type":"integer"},"y":{"type":"integer"},"display_index":{"type":"integer"}}), &["delta_y"]),
        spec(K::TypeText, "Types Unicode text into the active window.", Admin, ComputerControl, json!({"text":{"type":"string"}}), &["text"]),
        spec(K::PressKey, "Presses and releases a named keyboard key such as enter, tab, escape, an arrow, or f1-f12.", Admin, ComputerControl, json!({"key":{"type":"string"}}), &["key"]),
        spec(K::KeyDown, "Holds down a keyboard key until key_up.", Admin, ComputerControl, json!({"key":{"type":"string"}}), &["key"]),
        spec(K::KeyUp, "Releases a previously held keyboard key.", Admin, ComputerControl, json!({"key":{"type":"string"}}), &["key"]),
        spec(K::Hotkey, "Presses a keyboard shortcut.", Admin, ComputerControl, json!({"keys":{"type":"array","items":{"type":"string"}}}), &["keys"]),
        spec(K::GetUiTree, "Returns a filtered interactive UI element tree for a window.", Viewer, ReadOnly, json!({"depth":{"type":"integer"},"window_title":{"type":"string"},"query":{"type":"string"},"compact":{"type":"boolean"}}), &[]),
        spec(K::ClickElement, "Clicks a UI element by the ID returned from get_ui_tree.", Admin, ComputerControl, json!({"element_id":{"type":"integer"},"action_type":{"type":"string","enum":["invoke","click"]},"with_diff":{"type":"boolean"}}), &["element_id"]),
        spec(K::SetElementText, "Sets text on a UI element by ID.", Admin, ComputerControl, json!({"element_id":{"type":"integer"},"text":{"type":"string"},"with_diff":{"type":"boolean"}}), &["element_id","text"]),
        spec(K::BatchActions, "Executes sequential click, text, key, wait, and window-focus actions in one Agent round trip with optional state diff.", Admin, ComputerControl, json!({"actions":{"type":"array","items":{"type":"object"}},"delay_ms":{"type":"integer"},"with_diff":{"type":"boolean"}}), &["actions"]),
        spec(K::ListWindows, "Lists open desktop windows and their process and geometry metadata.", Admin, ComputerControl, json!({"only_visible":{"type":"boolean"}}), &[]),
        spec(K::FocusWindow, "Restores and activates a window selected by title, PID, or handle.", Admin, ComputerControl, json!({"title":{"type":"string"},"pid":{"type":"integer"},"hwnd":{"type":"integer"}}), &[]),
        spec(K::CloseWindow, "Gracefully closes a window selected by title, PID, or handle.", Admin, ComputerControl, json!({"title":{"type":"string"},"pid":{"type":"integer"},"hwnd":{"type":"integer"}}), &[]),
        spec(K::GetMarkedScreen, "Captures a Set-of-Mark annotated screenshot with numbered UI regions, image Base64/data URI output, and a mark-to-coordinate mapping.", Viewer, ReadOnly, json!({"display_index":{"type":"integer"},"format":{"type":"string","enum":["jpeg","png"]},"quality":{"type":"integer"},"max_dimension":{"type":"integer"},"crop":{"type":"array","items":{"type":"integer"}},"strategy":{"type":"string","enum":["auto","ui_tree","grid","contours","hybrid"]},"grid_divisions":{"type":"integer"},"window_title":{"type":"string"}}), &[]),
        spec(K::ClickMark, "Clicks a Set-of-Mark visual mark by ID.", Admin, ComputerControl, json!({"mark_id":{"type":"integer"},"button":{"type":"string","enum":["left","middle","right"]},"count":{"type":"integer"}}), &["mark_id"]),
    ];

    validate_tool_registry(&specs).expect("invalid built-in Agent tool registry");
    specs
});

pub fn tool_registry() -> &'static [ToolSpec] {
    &TOOL_REGISTRY
}

pub fn agent_tool(name: &str) -> Option<&'static ToolSpec> {
    TOOL_REGISTRY.iter().find(|spec| spec.name == name)
}

pub fn agent_tool_definitions() -> Vec<Value> {
    TOOL_REGISTRY
        .iter()
        .map(ToolSpec::as_mcp_definition)
        .collect()
}

pub fn validate_tool_registry(specs: &[ToolSpec]) -> Result<(), ToolRegistryError> {
    let mut names = HashSet::new();
    let mut kinds = HashSet::new();
    for spec in specs {
        if !names.insert(spec.name) {
            return Err(ToolRegistryError(format!(
                "duplicate tool name '{}'",
                spec.name
            )));
        }
        let kind = spec.agent_kind();
        if spec.name != kind.as_str() {
            return Err(ToolRegistryError(format!(
                "tool '{}' does not match dispatch kind '{}'",
                spec.name,
                kind.as_str()
            )));
        }
        if !kinds.insert(kind) {
            return Err(ToolRegistryError(format!(
                "duplicate dispatch kind '{kind:?}'"
            )));
        }
        let schema = spec.input_schema.as_object().ok_or_else(|| {
            ToolRegistryError(format!(
                "tool '{}' input schema is not an object",
                spec.name
            ))
        })?;
        if schema.get("type") != Some(&Value::String("object".to_string())) {
            return Err(ToolRegistryError(format!(
                "tool '{}' input schema type must be object",
                spec.name
            )));
        }
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                ToolRegistryError(format!("tool '{}' properties must be an object", spec.name))
            })?;
        for (property_name, property_schema) in properties {
            let property = property_schema.as_object().ok_or_else(|| {
                ToolRegistryError(format!(
                    "tool '{}' property '{}' schema must be an object",
                    spec.name, property_name
                ))
            })?;
            let property_type = property
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ToolRegistryError(format!(
                        "tool '{}' property '{}' must declare a string type",
                        spec.name, property_name
                    ))
                })?;
            if !matches!(
                property_type,
                "array" | "boolean" | "integer" | "number" | "object" | "string" | "null"
            ) {
                return Err(ToolRegistryError(format!(
                    "tool '{}' property '{}' has unsupported type '{}'",
                    spec.name, property_name, property_type
                )));
            }
            if property
                .get("description")
                .and_then(Value::as_str)
                .is_none_or(|description| description.trim().is_empty())
            {
                return Err(ToolRegistryError(format!(
                    "tool '{}' property '{}' has no description",
                    spec.name, property_name
                )));
            }
        }

        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                ToolRegistryError(format!("tool '{}' required must be an array", spec.name))
            })?;
        let mut required_names = HashSet::new();
        for required_name in required {
            let required_name = required_name.as_str().ok_or_else(|| {
                ToolRegistryError(format!(
                    "tool '{}' required entries must be strings",
                    spec.name
                ))
            })?;
            if !properties.contains_key(required_name) {
                return Err(ToolRegistryError(format!(
                    "tool '{}' requires unknown property '{}'",
                    spec.name, required_name
                )));
            }
            if !required_names.insert(required_name) {
                return Err(ToolRegistryError(format!(
                    "tool '{}' repeats required property '{}'",
                    spec.name, required_name
                )));
            }
        }
    }
    if kinds.len() != ALL_AGENT_TOOL_KINDS.len()
        || ALL_AGENT_TOOL_KINDS
            .iter()
            .any(|kind| !kinds.contains(kind))
    {
        return Err(ToolRegistryError(
            "registry does not cover every AgentToolKind exactly once".to_string(),
        ));
    }
    Ok(())
}

/// Clone a canonical schema and add optional properties without changing its required set.
pub fn extend_input_schema(
    schema: &Value,
    additions: impl IntoIterator<Item = (&'static str, Value)>,
) -> Result<Value, ToolRegistryError> {
    let mut extended = schema.clone();
    let properties = extended
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| ToolRegistryError("input schema has no object properties".to_string()))?;
    for (name, property) in additions {
        if properties.insert(name.to_string(), property).is_some() {
            return Err(ToolRegistryError(format!(
                "schema property '{name}' is already defined"
            )));
        }
    }
    Ok(extended)
}

/// Convenience constructor for schema adapters outside this crate.
pub fn string_property(description: &'static str) -> Value {
    let mut property = Map::new();
    property.insert("type".to_string(), Value::String("string".to_string()));
    property.insert(
        "description".to_string(),
        Value::String(description.to_string()),
    );
    Value::Object(property)
}
