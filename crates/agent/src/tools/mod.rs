//! MCP Tools implementation, schema definitions, and dynamic dispatcher module.

pub mod batch;
pub mod command;
pub mod computer_use;
pub mod directory;
pub mod event_log;
pub mod file_ops;
pub mod network;
pub mod process;
pub mod process_registry;
pub mod screen;
pub mod service;
pub mod som;
pub mod sysinfo;
pub mod uia;
pub mod window;

pub use process_registry::ProcessRegistry;

use serde_json::{json, Value};

/// Returns JSON schemas for all 9 diagnostic and remediation MCP tools.
pub fn get_mcp_tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "get_system_overview",
            "description": "Returns complete hardware and operating system status (OS version, CPU model/load, RAM usage, storage volumes, network interfaces, local IPs, default gateway, DNS servers, and system uptime).",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "exec_powershell",
            "description": "Executes a PowerShell script in a hidden background process with timeout protection and stdout/stderr capture.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "script": {
                        "type": "string",
                        "description": "PowerShell command or script block to execute."
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Execution timeout in seconds (default: 30)."
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Optional working directory."
                    }
                },
                "required": ["script"]
            }
        }),
        json!({
            "name": "exec_cmd",
            "description": "Executes a shell / Windows CMD command silently in the background with timeout protection.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Command string to execute."
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Execution timeout in seconds (default: 30)."
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Optional working directory."
                    }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "list_processes",
            "description": "Lists running processes on the system with PID, process name, memory footprint (MB), CPU usage, and binary path.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "filter": {
                        "type": "string",
                        "description": "Substring filter matched against process name, PID, or command."
                    },
                    "filter_name": {
                        "type": "string",
                        "description": "Alias for filter."
                    },
                    "sort_by": {
                        "type": "string",
                        "enum": ["memory", "cpu", "pid", "name"],
                        "description": "Field to sort processes by (default: 'memory')."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of processes to return (default: 50)."
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "kill_process",
            "description": "Terminates a running process by PID or executable name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pid": {
                        "type": "integer",
                        "description": "Target process ID."
                    },
                    "name": {
                        "type": "string",
                        "description": "Target process executable name (e.g. 'notepad.exe')."
                    },
                    "process_name": {
                        "type": "string",
                        "description": "Alias for name."
                    },
                    "force": {
                        "type": "boolean",
                        "description": "Whether to forcibly terminate immediately (default: true)."
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "manage_service",
            "description": "Inspects status or controls system services (start, stop, restart).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "service_name": {
                        "type": "string",
                        "description": "System name of the service (e.g. 'Spooler', 'wuauserv')."
                    },
                    "action": {
                        "type": "string",
                        "enum": ["status", "start", "stop", "restart"],
                        "description": "Action to perform on the service."
                    }
                },
                "required": ["service_name", "action"]
            }
        }),
        json!({
            "name": "read_text_file",
            "description": "Reads a log or configuration file with tail-reading (default: last 200 lines) and byte limits to prevent context window overflow. Pass tail_lines=0 for unlimited lines.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Absolute or relative path to the target file."
                    },
                    "tail_lines": {
                        "type": "integer",
                        "description": "Read only the last N lines (default: 200). Pass 0 to disable line tailing and read the full file."
                    },
                    "max_bytes": {
                        "type": "integer",
                        "description": "Maximum byte size limit (default: 512,000)."
                    }
                },
                "required": ["file_path"]
            }
        }),
        json!({
            "name": "write_text_file",
            "description": "Safely writes or patches a configuration file, with automatic timestamped .bak backup creation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the target file."
                    },
                    "content": {
                        "type": "string",
                        "description": "Text content to write."
                    },
                    "create_backup": {
                        "type": "boolean",
                        "description": "Whether to create a timestamped .bak backup first (default: true)."
                    }
                },
                "required": ["file_path", "content"]
            }
        }),
        json!({
            "name": "capture_screen",
            "description": "Captures a screenshot of the specified monitor display for visual multi-modal diagnostics. Supports resolution downsampling (max_dimension) and ROI cropping (crop) to minimize token consumption.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "display_index": {
                        "type": "integer",
                        "description": "Zero-based index of the display monitor (default: 0)."
                    },
                    "format": {
                        "type": "string",
                        "enum": ["jpeg", "png"],
                        "description": "Image format ('jpeg' or 'png', default: 'jpeg')."
                    },
                    "quality": {
                        "type": "integer",
                        "description": "JPEG compression quality from 1 to 100 (default: 80)."
                    },
                    "max_dimension": {
                        "type": "integer",
                        "description": "Optional maximum bounding dimension (e.g. 1280) to downsample large 2K/4K screenshots and conserve tokens."
                    },
                    "crop": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Optional [x, y, width, height] region of interest to capture only a target window or dialog."
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "list_monitors",
            "description": "Lists all connected physical and virtual display monitors on the target terminal with their display index, name, primary flag, screen bounds (x, y, width, height), and DPI scale factor.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "list_directory",
            "description": "Lists contents and metadata of a filesystem directory (names, file sizes, modification timestamps, directory/symlink flags).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path to list."
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "Whether to list subdirectories recursively (default: false)."
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Maximum recursion depth (default: 1 if non-recursive, 3 if recursive)."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of entries to return (default: 100)."
                    }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "search_files",
            "description": "Searches for files matching a wildcard or substring pattern within a base directory.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "base_path": {
                        "type": "string",
                        "description": "Base directory to start search."
                    },
                    "pattern": {
                        "type": "string",
                        "description": "Filename pattern or wildcard (e.g. '*.log', 'error*')."
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum matching files to return (default: 50)."
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Maximum directory search depth (default: 5)."
                    }
                },
                "required": ["base_path", "pattern"]
            }
        }),
        json!({
            "name": "list_network_connections",
            "description": "Lists active TCP/UDP connections and listening ports with local/remote IP, ports, state, and binding PID.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "state": {
                        "type": "string",
                        "description": "Filter by connection state (e.g. 'LISTEN', 'ESTABLISHED')."
                    },
                    "port": {
                        "type": "integer",
                        "description": "Filter by local or remote port number."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of records to return (default: 100)."
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "test_network",
            "description": "Tests target network reachability, DNS resolution, and TCP port connectivity with latency measurement.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target_host": {
                        "type": "string",
                        "description": "Target hostname or IP address to probe."
                    },
                    "port": {
                        "type": "integer",
                        "description": "Optional TCP port to test connection."
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Probe timeout in milliseconds (default: 3000)."
                    }
                },
                "required": ["target_host"]
            }
        }),
        json!({
            "name": "get_event_logs",
            "description": "Queries recent operating system event logs (Windows Event Log or system log) for recent Error, Critical, or Warning events.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "log_name": {
                        "type": "string",
                        "description": "Event log channel (e.g. 'System', 'Application', default: 'System')."
                    },
                    "level": {
                        "type": "string",
                        "enum": ["Error", "Critical", "Warning", "All"],
                        "description": "Filter event severity level (default: 'Error')."
                    },
                    "hours_back": {
                        "type": "integer",
                        "description": "Look back window in hours (default: 24)."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum event entries to return (default: 20)."
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "mouse_click",
            "description": "Simulates a mouse click (left, middle, or right) at the specified screen coordinates or current cursor position. Coordinates default to physical screen pixels.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "x": {
                        "type": "integer",
                        "description": "Optional X screen coordinate (physical pixels by default, or normalized if coord_mode is set)."
                    },
                    "y": {
                        "type": "integer",
                        "description": "Optional Y screen coordinate (physical pixels by default, or normalized if coord_mode is set)."
                    },
                    "coord_mode": {
                        "type": "string",
                        "enum": ["pixel", "normalized", "normalized_1000"],
                        "description": "Coordinate system mode: 'pixel' (default, physical screen pixels), 'normalized' (0-65535), or 'normalized_1000' (0-1000)."
                    },
                    "button": {
                        "type": "string",
                        "enum": ["left", "middle", "right"],
                        "description": "Mouse button to click (default: 'left')."
                    },
                    "count": {
                        "type": "integer",
                        "description": "Number of clicks: 1 for single click, 2 for double click (default: 1)."
                    },
                    "mark_id": {
                        "type": "integer",
                        "description": "Optional Set-of-Mark (SoM) mark ID from get_marked_screen to click directly, bypassing coordinate calculation."
                    },
                    "display_index": {
                        "type": "integer",
                        "description": "Optional zero-based display index. If provided, coordinates (x, y) are treated as local pixels relative to that monitor."
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "mouse_move",
            "description": "Moves the remote mouse cursor to screen coordinates (physical screen pixels by default).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "x": {
                        "type": "integer",
                        "description": "Target X screen coordinate (physical pixels by default)."
                    },
                    "y": {
                        "type": "integer",
                        "description": "Target Y screen coordinate (physical pixels by default)."
                    },
                    "coord_mode": {
                        "type": "string",
                        "enum": ["pixel", "normalized", "normalized_1000"],
                        "description": "Coordinate system mode: 'pixel' (default), 'normalized' (0-65535), or 'normalized_1000' (0-1000)."
                    },
                    "display_index": {
                        "type": "integer",
                        "description": "Optional zero-based display index. If provided, coordinates (x, y) are treated as local pixels relative to that monitor."
                    }
                },
                "required": ["x", "y"]
            }
        }),
        json!({
            "name": "mouse_drag",
            "description": "Performs a mouse drag operation from start coordinates to end coordinates.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "start_x": {
                        "type": "integer",
                        "description": "Optional start X screen coordinate (defaults to current position)."
                    },
                    "start_y": {
                        "type": "integer",
                        "description": "Optional start Y screen coordinate (defaults to current position)."
                    },
                    "end_x": {
                        "type": "integer",
                        "description": "Target destination X coordinate (physical pixels by default)."
                    },
                    "end_y": {
                        "type": "integer",
                        "description": "Target destination Y coordinate (physical pixels by default)."
                    },
                    "coord_mode": {
                        "type": "string",
                        "enum": ["pixel", "normalized", "normalized_1000"],
                        "description": "Coordinate system mode: 'pixel' (default), 'normalized' (0-65535), or 'normalized_1000' (0-1000)."
                    },
                    "button": {
                        "type": "string",
                        "enum": ["left", "middle", "right"],
                        "description": "Mouse button to hold during drag (default: 'left')."
                    },
                    "display_index": {
                        "type": "integer",
                        "description": "Optional zero-based display index. If provided, coordinates are treated as local pixels relative to that monitor."
                    }
                },
                "required": ["end_x", "end_y"]
            }
        }),
        json!({
            "name": "mouse_scroll",
            "description": "Simulates a mouse scroll wheel rotation at current or specified coordinate.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "delta_y": {
                        "type": "integer",
                        "description": "Scroll delta (positive: scroll up, negative: scroll down)."
                    },
                    "x": {
                        "type": "integer",
                        "description": "Optional X coordinate before scrolling."
                    },
                    "y": {
                        "type": "integer",
                        "description": "Optional Y coordinate before scrolling."
                    },
                    "display_index": {
                        "type": "integer",
                        "description": "Optional zero-based display index. If provided, coordinates (x, y) are treated as local pixels relative to that monitor."
                    }
                },
                "required": ["delta_y"]
            }
        }),
        json!({
            "name": "type_text",
            "description": "Types a sequence of Unicode characters as simulated keyboard text input on the active window.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Text content to type into the focused UI element."
                    }
                },
                "required": ["text"]
            }
        }),
        json!({
            "name": "press_key",
            "description": "Simulates pressing and immediately releasing a keyboard key (e.g. 'enter', 'tab', 'escape', 'space', 'backspace', 'delete', 'f1'-'f12', 'up', 'down', 'left', 'right').",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Key identifier to press (e.g. 'enter', 'escape', 'tab', 'space', 'backspace', 'ctrl', 'alt', 'shift', 'win')."
                    }
                },
                "required": ["key"]
            }
        }),
        json!({
            "name": "key_down",
            "description": "Simulates holding down a keyboard key until released by key_up.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Key identifier to hold down."
                    }
                },
                "required": ["key"]
            }
        }),
        json!({
            "name": "key_up",
            "description": "Simulates releasing a previously held keyboard key.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Key identifier to release."
                    }
                },
                "required": ["key"]
            }
        }),
        json!({
            "name": "hotkey",
            "description": "Simulates a keyboard shortcut by pressing multiple keys simultaneously (e.g. ['ctrl', 'c'], ['alt', 'tab'], ['ctrl', 'alt', 'delete']).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "keys": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of key names to press together in order (e.g. ['ctrl', 'c'])."
                    }
                },
                "required": ["keys"]
            }
        }),
        json!({
            "name": "get_ui_tree",
            "description": "Returns a structured, pruned interactive UI element tree (like a browser DOM) for the active window. Elements include unique IDs, names, control types, bounding rects, and current values.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "depth": {
                        "type": "integer",
                        "description": "Maximum tree depth to traverse (default: 5)."
                    },
                    "window_title": {
                        "type": "string",
                        "description": "Optional title filter to target a specific window."
                    },
                    "query": {
                        "type": "string",
                        "description": "Optional search substring to filter elements by name, value, or help_text. Saves tokens (<150 tokens) by returning only matching elements."
                    },
                    "compact": {
                        "type": "boolean",
                        "description": "Optional flag to prune empty structural containers and return only interactive elements (default: false)."
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "click_element",
            "description": "Performs a semantic click on a UI element by its ID (obtained from get_ui_tree). Uses InvokePattern if available (background direct click without mouse movement), with fallback to physical center click. Includes 80ms state_diff post-action verification.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "element_id": {
                        "type": "integer",
                        "description": "The unique ID of the target UI element returned by get_ui_tree."
                    },
                    "action_type": {
                        "type": "string",
                        "enum": ["invoke", "click"],
                        "description": "Action type: 'invoke' (pattern invoke with fallback) or 'click' (direct physical bounding box center click). Default: 'invoke'."
                    },
                    "with_diff": {
                        "type": "boolean",
                        "description": "Whether to capture and return 80ms post-action UI state diff (default: true)."
                    }
                },
                "required": ["element_id"]
            }
        }),
        json!({
            "name": "set_element_text",
            "description": "Sets the text value of an editable UI element by its ID (obtained from get_ui_tree). Uses ValuePattern if available, with fallback to focusing and typing text via simulated keyboard. Includes 80ms state_diff post-action verification.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "element_id": {
                        "type": "integer",
                        "description": "The unique ID of the target UI element returned by get_ui_tree."
                    },
                    "text": {
                        "type": "string",
                        "description": "The text string to set or type into the target element."
                    },
                    "with_diff": {
                        "type": "boolean",
                        "description": "Whether to capture and return 80ms post-action UI state diff (default: true)."
                    }
                },
                "required": ["element_id", "text"]
            }
        }),
        json!({
            "name": "batch_actions",
            "description": "Executes a sequential batch of UI actions (click_element, set_element_text, type_text, press_key, hotkey, mouse_click, wait, focus_window) in a single client round-trip with terminal-side 80ms state_diff. Eliminates multi-turn LLM network latency for complex UI flows.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "actions": {
                        "type": "array",
                        "items": { "type": "object" },
                        "description": "Ordered array of action objects to execute sequentially."
                    },
                    "delay_ms": {
                        "type": "integer",
                        "description": "Delay between action steps in milliseconds (default: 40)."
                    },
                    "with_diff": {
                        "type": "boolean",
                        "description": "Whether to capture and return 80ms post-action UI state diff (default: true)."
                    }
                },
                "required": ["actions"]
            }
        }),
        json!({
            "name": "list_windows",
            "description": "Lists all open and visible desktop windows with their HWND, PID, process name, title, minimized status, foreground status, and bounding rectangle coordinates.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "only_visible": {
                        "type": "boolean",
                        "description": "Whether to filter and return only visible non-minimized windows with titles (default: true)."
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "focus_window",
            "description": "Activates and brings a desktop window to the foreground, restoring it if minimized. Target window can be specified by title substring, PID, or HWND.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Case-insensitive substring of the window title to match and activate."
                    },
                    "pid": {
                        "type": "integer",
                        "description": "Process ID owning the window to activate."
                    },
                    "hwnd": {
                        "type": "integer",
                        "description": "Window handle (HWND) to activate directly."
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "close_window",
            "description": "Sends a graceful close signal (WM_CLOSE on Windows / AppleScript or SIGTERM on non-Windows) to close a window without forcibly killing the process. Target window can be specified by title substring, PID, or HWND.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Case-insensitive substring of the window title to match and close."
                    },
                    "pid": {
                        "type": "integer",
                        "description": "Process ID owning the window to close."
                    },
                    "hwnd": {
                        "type": "integer",
                        "description": "Window handle (HWND) to close directly."
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "get_marked_screen",
            "description": "Captures the screen and generates a Set-of-Mark (SoM) annotated image with numbered bounding boxes overlaid on UI elements, custom canvas regions, or grid divisions. Returns marked image Base64 and mark ID to physical coordinate mapping.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "display_index": {
                        "type": "integer",
                        "description": "Zero-based index of the display monitor (default: 0)."
                    },
                    "format": {
                        "type": "string",
                        "enum": ["jpeg", "png"],
                        "description": "Image format ('jpeg' or 'png', default: 'jpeg')."
                    },
                    "quality": {
                        "type": "integer",
                        "description": "JPEG compression quality from 1 to 100 (default: 80)."
                    },
                    "max_dimension": {
                        "type": "integer",
                        "description": "Optional maximum bounding dimension to downsample screenshot."
                    },
                    "crop": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Optional [x, y, width, height] region of interest."
                    },
                    "strategy": {
                        "type": "string",
                        "enum": ["auto", "ui_tree", "grid", "contours", "hybrid"],
                        "description": "Marking strategy: 'auto' (hybrid UI tree with visual fallback), 'ui_tree' (accessible elements), 'grid' (uniform coordinate grid), 'contours' (visual bounding box detection), or 'hybrid' (combined accessible elements and visual boxes). Default: 'auto'."
                    },
                    "grid_divisions": {
                        "type": "integer",
                        "description": "Number of grid rows/cols if grid strategy is used (default: 4)."
                    },
                    "window_title": {
                        "type": "string",
                        "description": "Optional title filter to target a specific window."
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "click_mark",
            "description": "Simulates a mouse click directly on a Set-of-Mark (SoM) visual mark ID (obtained from get_marked_screen), eliminating pixel coordinate calculation errors.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mark_id": {
                        "type": "integer",
                        "description": "The unique numeric ID of the target mark returned by get_marked_screen."
                    },
                    "button": {
                        "type": "string",
                        "enum": ["left", "middle", "right"],
                        "description": "Mouse button to click (default: 'left')."
                    },
                    "count": {
                        "type": "integer",
                        "description": "Number of clicks: 1 for single click, 2 for double click (default: 1)."
                    }
                },
                "required": ["mark_id"]
            }
        }),
    ]
}

/// Dispatches an MCP tool call by name with parsed JSON arguments.
pub fn dispatch_tool(name: &str, arguments: Value) -> Result<Value, String> {
    dispatch_tool_with_registry(name, arguments, &ProcessRegistry::global())
}

/// Dispatches an MCP tool call using the specified ProcessRegistry for subprocesses.
pub fn dispatch_tool_with_registry(
    name: &str,
    arguments: Value,
    registry: &std::sync::Arc<ProcessRegistry>,
) -> Result<Value, String> {
    dispatch_tool_with_call_id("", name, arguments, registry)
}

/// Dispatches an MCP tool call associated with a specific invocation call ID for fine-grained cancellation.
pub fn dispatch_tool_with_call_id(
    call_id: &str,
    name: &str,
    arguments: Value,
    registry: &std::sync::Arc<ProcessRegistry>,
) -> Result<Value, String> {
    dispatch_tool_with_call_id_and_options(call_id, name, arguments, registry, false)
}

/// Dispatches an MCP tool call associated with a specific invocation call ID and runtime options.
pub fn dispatch_tool_with_call_id_and_options(
    call_id: &str,
    name: &str,
    arguments: Value,
    registry: &std::sync::Arc<ProcessRegistry>,
    enable_computer_use: bool,
) -> Result<Value, String> {
    let call_id_opt = if call_id.is_empty() { None } else { Some(call_id) };
    match name {
        "get_system_overview" => {
            let overview = sysinfo::get_system_overview();
            serde_json::to_value(overview).map_err(|e| e.to_string())
        }

        "exec_powershell" => {
            let script = arguments
                .get("script")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'script'".to_string())?;

            let timeout_secs = arguments
                .get("timeout_secs")
                .and_then(|v| v.as_u64())
                .unwrap_or(30);

            let cwd = arguments.get("cwd").and_then(|v| v.as_str());

            let res = command::exec_powershell_with_call(script, timeout_secs, cwd, registry, call_id_opt)?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        "exec_cmd" => {
            let cmd_str = arguments
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'command'".to_string())?;

            let timeout_secs = arguments
                .get("timeout_secs")
                .and_then(|v| v.as_u64())
                .unwrap_or(30);

            let cwd = arguments.get("cwd").and_then(|v| v.as_str());

            let res = command::exec_cmd_with_call(cmd_str, timeout_secs, cwd, registry, call_id_opt)?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        "list_processes" => {
            let filter = arguments
                .get("filter")
                .or_else(|| arguments.get("filter_name"))
                .and_then(|v| v.as_str());

            let sort_by = arguments.get("sort_by").and_then(|v| v.as_str());

            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|l| l as usize)
                .unwrap_or(50);

            let procs = process::list_processes(filter, sort_by, limit);
            serde_json::to_value(procs).map_err(|e| e.to_string())
        }

        "kill_process" => {
            let pid = arguments
                .get("pid")
                .and_then(|v| v.as_u64().map(|p| p as u32));

            let name = arguments
                .get("name")
                .or_else(|| arguments.get("process_name"))
                .and_then(|v| v.as_str());

            let force = arguments
                .get("force")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            let msg = process::kill_process(pid, name, force)?;
            Ok(json!({ "message": msg, "success": true }))
        }

        "manage_service" => {
            let service_name = arguments
                .get("service_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'service_name'".to_string())?;

            let action = arguments
                .get("action")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'action'".to_string())?;

            let res = service::manage_service(service_name, action)?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        "read_text_file" => {
            let file_path = arguments
                .get("file_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'file_path'".to_string())?;

            let tail_lines = arguments
                .get("tail_lines")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);

            let max_bytes = arguments
                .get("max_bytes")
                .and_then(|v| v.as_u64())
                .map(|b| b as usize);

            let res = file_ops::read_text_file(file_path, tail_lines, max_bytes)?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        "write_text_file" => {
            let file_path = arguments
                .get("file_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'file_path'".to_string())?;

            let content = arguments
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'content'".to_string())?;

            let create_backup = arguments
                .get("create_backup")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            let res = file_ops::write_text_file(file_path, content, create_backup)?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        "capture_screen" => {
            let display_index = arguments
                .get("display_index")
                .and_then(|v| v.as_u64())
                .map(|d| d as usize)
                .unwrap_or(0);

            let format = arguments
                .get("format")
                .and_then(|v| v.as_str())
                .unwrap_or("jpeg");

            let quality = arguments
                .get("quality")
                .and_then(|v| v.as_u64())
                .map(|q| q as u8)
                .unwrap_or(80);

            let save_path = arguments
                .get("save_path")
                .or_else(|| arguments.get("save_to_file"))
                .or_else(|| arguments.get("file_path"))
                .and_then(|v| v.as_str());

            let max_dimension = arguments
                .get("max_dimension")
                .or_else(|| arguments.get("max_size"))
                .and_then(|v| v.as_u64())
                .map(|d| d as u32);

            let crop = if let Some(arr) = arguments.get("crop").and_then(|v| v.as_array()) {
                if arr.len() == 4 {
                    let c0 = arr[0].as_u64().unwrap_or(0) as u32;
                    let c1 = arr[1].as_u64().unwrap_or(0) as u32;
                    let c2 = arr[2].as_u64().unwrap_or(0) as u32;
                    let c3 = arr[3].as_u64().unwrap_or(0) as u32;
                    Some([c0, c1, c2, c3])
                } else {
                    None
                }
            } else if let (Some(cx), Some(cy), Some(cw), Some(ch)) = (
                arguments.get("crop_x").and_then(|v| v.as_u64()),
                arguments.get("crop_y").and_then(|v| v.as_u64()),
                arguments.get("crop_width").and_then(|v| v.as_u64()),
                arguments.get("crop_height").and_then(|v| v.as_u64()),
            ) {
                Some([cx as u32, cy as u32, cw as u32, ch as u32])
            } else {
                None
            };

            let res = screen::capture_screen(display_index, format, quality, save_path, max_dimension, crop)?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        "list_monitors" => {
            let res = screen::list_monitors()?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        "list_directory" => {
            let path = arguments
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'path'".to_string())?;

            let recursive = arguments.get("recursive").and_then(|v| v.as_bool());
            let max_depth = arguments
                .get("max_depth")
                .and_then(|v| v.as_u64())
                .map(|d| d as usize);
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|l| l as usize);

            let res = directory::list_directory(path, recursive, max_depth, limit)?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        "search_files" => {
            let base_path = arguments
                .get("base_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'base_path'".to_string())?;

            let pattern = arguments
                .get("pattern")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'pattern'".to_string())?;

            let max_results = arguments
                .get("max_results")
                .and_then(|v| v.as_u64())
                .map(|m| m as usize);
            let max_depth = arguments
                .get("max_depth")
                .and_then(|v| v.as_u64())
                .map(|d| d as usize);

            let res = directory::search_files(base_path, pattern, max_results, max_depth)?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        "list_network_connections" => {
            let state = arguments.get("state").and_then(|v| v.as_str());
            let port = arguments
                .get("port")
                .and_then(|v| v.as_u64())
                .map(|p| p as u16);
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|l| l as usize);

            let res = network::list_network_connections(state, port, limit)?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        "test_network" => {
            let target_host = arguments
                .get("target_host")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'target_host'".to_string())?;

            let port = arguments
                .get("port")
                .and_then(|v| v.as_u64())
                .map(|p| p as u16);

            let timeout_ms = arguments.get("timeout_ms").and_then(|v| v.as_u64());

            let res = network::test_network(target_host, port, timeout_ms)?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        "get_event_logs" => {
            let log_name = arguments.get("log_name").and_then(|v| v.as_str());
            let level = arguments.get("level").and_then(|v| v.as_str());
            let hours_back = arguments.get("hours_back").and_then(|v| v.as_u64());
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|l| l as usize);

            let res = event_log::get_event_logs(log_name, level, hours_back, limit)?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        "mouse_click" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            computer_use::execute_mouse_click(&arguments)
        }

        "mouse_move" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            computer_use::execute_mouse_move(&arguments)
        }

        "mouse_drag" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            computer_use::execute_mouse_drag(&arguments)
        }

        "mouse_scroll" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            computer_use::execute_mouse_scroll(&arguments)
        }

        "type_text" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            computer_use::execute_type_text(&arguments)
        }

        "press_key" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            computer_use::execute_press_key(&arguments)
        }

        "key_down" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            computer_use::execute_key_down(&arguments)
        }

        "key_up" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            computer_use::execute_key_up(&arguments)
        }

        "hotkey" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            computer_use::execute_hotkey(&arguments)
        }

        "get_ui_tree" => {
            let depth = arguments.get("depth").and_then(|v| match v {
                Value::Number(n) => n.as_u64().map(|d| d as u32),
                Value::String(s) => s.trim().parse::<u32>().ok(),
                _ => None,
            });
            let window_title = arguments.get("window_title").and_then(|v| v.as_str());
            let query = arguments.get("query").or_else(|| arguments.get("filter")).and_then(|v| v.as_str());
            let compact = arguments.get("compact").and_then(|v| v.as_bool());
            let res = uia::get_ui_tree_filtered(depth, window_title, query, compact)?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        "click_element" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            let element_id = arguments
                .get("element_id")
                .or_else(|| arguments.get("id"))
                .and_then(|v| match v {
                    Value::Number(n) => n.as_u64().map(|x| x as u32),
                    Value::String(s) => s.trim().trim_start_matches('#').parse::<u32>().ok(),
                    _ => None,
                })
                .ok_or_else(|| "Missing required parameter 'element_id'".to_string())?;
            let action_type = arguments.get("action_type").and_then(|v| v.as_str());
            let with_diff = arguments.get("with_diff").and_then(|v| v.as_bool());
            uia::click_element_with_diff(element_id, action_type, with_diff)
        }

        "set_element_text" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            let element_id = arguments
                .get("element_id")
                .or_else(|| arguments.get("id"))
                .and_then(|v| match v {
                    Value::Number(n) => n.as_u64().map(|x| x as u32),
                    Value::String(s) => s.trim().trim_start_matches('#').parse::<u32>().ok(),
                    _ => None,
                })
                .ok_or_else(|| "Missing required parameter 'element_id'".to_string())?;
            let text = match arguments.get("text").or_else(|| arguments.get("value")) {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                Some(Value::Bool(b)) => b.to_string(),
                _ => return Err("Missing required parameter 'text'".to_string()),
            };
            let with_diff = arguments.get("with_diff").and_then(|v| v.as_bool());
            uia::set_element_text_with_diff(element_id, &text, with_diff)
        }

        "batch_actions" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            batch::execute_batch_actions(&arguments)
        }

        "list_windows" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            let only_visible = arguments
                .get("only_visible")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let wins = window::list_windows(only_visible)?;
            serde_json::to_value(wins).map_err(|e| e.to_string())
        }

        "focus_window" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            let title = arguments.get("title").and_then(|v| v.as_str());
            let pid = arguments.get("pid").and_then(|v| v.as_u64()).map(|p| p as u32);
            let hwnd = arguments.get("hwnd").and_then(|v| v.as_u64()).map(|h| h as usize);
            window::focus_window(title, pid, hwnd)
        }

        "close_window" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            let title = arguments.get("title").and_then(|v| v.as_str());
            let pid = arguments.get("pid").and_then(|v| v.as_u64()).map(|p| p as u32);
            let hwnd = arguments.get("hwnd").and_then(|v| v.as_u64()).map(|h| h as usize);
            window::close_window(title, pid, hwnd)
        }

        "get_marked_screen" => {
            let display_index = arguments
                .get("display_index")
                .and_then(|v| v.as_u64())
                .map(|d| d as usize)
                .unwrap_or(0);

            let format = arguments
                .get("format")
                .and_then(|v| v.as_str())
                .unwrap_or("jpeg");

            let quality = arguments
                .get("quality")
                .and_then(|v| v.as_u64())
                .map(|q| q as u8)
                .unwrap_or(80);

            let max_dimension = arguments
                .get("max_dimension")
                .or_else(|| arguments.get("max_size"))
                .and_then(|v| v.as_u64())
                .map(|d| d as u32);

            let crop = if let Some(arr) = arguments.get("crop").and_then(|v| v.as_array()) {
                if arr.len() == 4 {
                    let c0 = arr[0].as_u64().unwrap_or(0) as u32;
                    let c1 = arr[1].as_u64().unwrap_or(0) as u32;
                    let c2 = arr[2].as_u64().unwrap_or(0) as u32;
                    let c3 = arr[3].as_u64().unwrap_or(0) as u32;
                    Some([c0, c1, c2, c3])
                } else {
                    None
                }
            } else {
                None
            };

            let strategy = arguments.get("strategy").and_then(|v| v.as_str());
            let grid_divisions = arguments
                .get("grid_divisions")
                .or_else(|| arguments.get("divisions"))
                .and_then(|v| v.as_u64())
                .map(|d| d as u32);
            let window_title = arguments.get("window_title").and_then(|v| v.as_str());

            let res = som::get_marked_screen(
                display_index,
                format,
                quality,
                max_dimension,
                crop,
                strategy,
                grid_divisions,
                window_title,
            )?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        "click_mark" => {
            if !enable_computer_use {
                return Err("Computer-use operations are disabled on this agent. Set 'enable_computer_use = true' in config or pass '--enable-computer-use' flag to enable.".to_string());
            }
            let mark_id = arguments
                .get("mark_id")
                .or_else(|| arguments.get("id"))
                .and_then(|v| match v {
                    Value::Number(n) => n.as_u64().map(|x| x as u32),
                    Value::String(s) => s.trim().trim_start_matches('#').parse::<u32>().ok(),
                    _ => None,
                })
                .ok_or_else(|| "Missing required parameter 'mark_id'".to_string())?;

            let button = match arguments.get("button") {
                Some(Value::String(s)) => Some(s.as_str()),
                Some(Value::Number(n)) => match n.as_u64() {
                    Some(1) => Some("middle"),
                    Some(2) => Some("right"),
                    _ => Some("left"),
                },
                _ => None,
            };
            let count = arguments
                .get("count")
                .and_then(|v| v.as_u64())
                .map(|c| c as u8);

            som::click_mark(mark_id, button, count)
        }

        unknown => Err(format!("Unknown or unsupported tool '{}'", unknown)),
    }
}
