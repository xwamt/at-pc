use serde_json::{json, Value};

/// Returns JSON schemas for all MCP tools exposed by the central server
/// (terminal management tools + 9 forwarded diagnostic tools).
pub fn get_mcp_tool_definitions() -> Vec<Value> {
    vec![
        // 1. list_terminals
        json!({
            "name": "list_terminals",
            "description": "Lists all connected and known PC terminals with their hardware specifications, IP, OS, status, and performance metrics.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),

        // 2. select_terminal
        json!({
            "name": "select_terminal",
            "description": "Sets the active target terminal for subsequent diagnostic commands in this session (accepts terminal ID, custom alias, or hostname).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "terminal_id": {
                        "type": "string",
                        "description": "The unique terminal ID, custom alias, or hostname to set as active target."
                    }
                },
                "required": ["terminal_id"]
            }
        }),

        // 3. rename_terminal
        json!({
            "name": "rename_terminal",
            "description": "Sets a persistent custom name, alias, notes, and tags for a terminal to easily identify and select it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "terminal_id": {
                        "type": "string",
                        "description": "Target terminal ID, existing custom alias, or hostname."
                    },
                    "custom_name": {
                        "type": "string",
                        "description": "User-friendly custom alias / name for the terminal (e.g. 'Finance-PC-01', 'Dev-Build-Server')."
                    },
                    "notes": {
                        "type": "string",
                        "description": "Optional notes or remarks regarding this terminal."
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional tags or categories for grouping (e.g. ['Finance', 'Win11'])."
                    }
                },
                "required": ["terminal_id"]
            }
        }),

        // 4. get_active_terminal
        json!({
            "name": "get_active_terminal",
            "description": "Gets information about the currently selected active terminal.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),

        // 5. cancel_tool
        json!({
            "name": "cancel_tool",
            "description": "Forcibly cancels an in-flight tool call or long-running command execution on a terminal by call_id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "call_id": {
                        "type": "string",
                        "description": "The unique call ID of the in-flight tool execution to cancel."
                    }
                },
                "required": ["call_id"]
            }
        }),

        // 6. list_pending_calls
        json!({
            "name": "list_pending_calls",
            "description": "Lists all currently in-flight remote tool calls and background command executions across all terminals.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional terminal ID to filter active calls."
                    }
                },
                "required": []
            }
        }),

        // 4. get_system_overview
        json!({
            "name": "get_system_overview",
            "description": "Returns complete hardware and operating system status (OS version, CPU model/load, RAM usage, storage volumes, network interfaces, local IPs, default gateway, DNS servers, and system uptime).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": []
            }
        }),

        // 5. exec_powershell
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["script"]
            }
        }),

        // 6. exec_cmd
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["command"]
            }
        }),

        // 7. list_processes
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": []
            }
        }),

        // 8. kill_process
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": []
            }
        }),

        // 9. manage_service
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["service_name", "action"]
            }
        }),

        // 10. read_text_file
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["file_path"]
            }
        }),

        // 11. write_text_file
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["file_path", "content"]
            }
        }),

        // 12. capture_screen
        json!({
            "name": "capture_screen",
            "description": "Captures a screenshot of the specified monitor display for visual multi-modal diagnostics. Recommended only when visual inspection is required (e.g. error popups or non-scriptable GUIs). Supports resolution downsampling (max_dimension) and ROI cropping (crop) to minimize token consumption. Result fields include: 'raw_base64' and 'image_base64' (pure base64 strings safe for base64.b64decode to disk), 'data_uri' (Data URL with scheme prefix for web display), and optional 'server_save_path' to automatically write the image directly to local server disk.",
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
                    "save_path": {
                        "type": "string",
                        "description": "Optional disk file path to save the screenshot on the client terminal disk (e.g. 'C:\\temp\\screen.jpg')."
                    },
                    "server_save_path": {
                        "type": "string",
                        "description": "Optional file path on the central server/host machine to automatically save the decoded image directly to disk (e.g. '/tmp/screen.jpg'). Recommended when capturing screenshots for LLM artifacts or verification to avoid manual base64 decoding."
                    },
                    "max_dimension": {
                        "type": "integer",
                        "description": "Optional maximum bounding dimension (e.g. 1280) to downsample large 2K/4K screenshots and conserve tokens."
                    },
                    "crop": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Optional [x, y, width, height] region of interest to capture only a target window or dialog."
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": []
            }
        }),

        // list_monitors
        json!({
            "name": "list_monitors",
            "description": "Lists all connected physical and virtual display monitors on the target terminal with their display index, name, primary flag, screen bounds (x, y, width, height), and DPI scale factor.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": []
            }
        }),

        // 13. list_directory
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["path"]
            }
        }),

        // 14. search_files
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["base_path", "pattern"]
            }
        }),

        // 15. list_network_connections
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": []
            }
        }),

        // 16. test_network
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["target_host"]
            }
        }),

        // 17. get_event_logs
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": []
            }
        }),

        // 18. mouse_click
        json!({
            "name": "mouse_click",
            "description": "Simulates a mouse click (left, middle, or right) at the specified screen coordinates or current cursor position with single/double click support. Coordinates default to physical screen pixels.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "x": {
                        "type": "integer",
                        "description": "Optional X screen coordinate (physical pixels by default, matching capture_screen resolution)."
                    },
                    "y": {
                        "type": "integer",
                        "description": "Optional Y screen coordinate (physical pixels by default, matching capture_screen resolution)."
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
                        "description": "Optional Set-of-Mark (SoM) mark ID from get_marked_screen to click directly without calculating coordinates."
                    },
                    "display_index": {
                        "type": "integer",
                        "description": "Optional zero-based display index. If provided, coordinates (x, y) are treated as local pixels relative to that monitor."
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": []
            }
        }),

        // 19. mouse_move
        json!({
            "name": "mouse_move",
            "description": "Moves the remote mouse cursor to screen coordinates (physical screen pixels by default).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "x": {
                        "type": "integer",
                        "description": "Target X screen coordinate (physical screen pixels by default)."
                    },
                    "y": {
                        "type": "integer",
                        "description": "Target Y screen coordinate (physical screen pixels by default)."
                    },
                    "coord_mode": {
                        "type": "string",
                        "enum": ["pixel", "normalized", "normalized_1000"],
                        "description": "Coordinate system mode: 'pixel' (default), 'normalized' (0-65535), or 'normalized_1000' (0-1000)."
                    },
                    "display_index": {
                        "type": "integer",
                        "description": "Optional zero-based display index. If provided, coordinates (x, y) are treated as local pixels relative to that monitor."
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["x", "y"]
            }
        }),

        // 20. mouse_drag
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
                        "description": "Target destination X coordinate (physical screen pixels by default)."
                    },
                    "end_y": {
                        "type": "integer",
                        "description": "Target destination Y coordinate (physical screen pixels by default)."
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["end_x", "end_y"]
            }
        }),

        // 21. mouse_scroll
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["delta_y"]
            }
        }),

        // 22. type_text
        json!({
            "name": "type_text",
            "description": "Types a sequence of Unicode characters as simulated keyboard text input on the active window.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Text content to type into the focused UI element."
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["text"]
            }
        }),

        // 23. press_key
        json!({
            "name": "press_key",
            "description": "Simulates pressing and immediately releasing a keyboard key (e.g. 'enter', 'tab', 'escape', 'space', 'backspace', 'delete', 'f1'-'f12', 'up', 'down', 'left', 'right').",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Key identifier to press (e.g. 'enter', 'escape', 'tab', 'space', 'backspace', 'ctrl', 'alt', 'shift', 'win')."
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["key"]
            }
        }),

        // 24. key_down
        json!({
            "name": "key_down",
            "description": "Simulates holding down a keyboard key until released by key_up.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Key identifier to hold down."
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["key"]
            }
        }),

        // 25. key_up
        json!({
            "name": "key_up",
            "description": "Simulates releasing a previously held keyboard key.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Key identifier to release."
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["key"]
            }
        }),

        // 26. hotkey
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["keys"]
            }
        }),

        // 27. get_ui_tree
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": []
            }
        }),

        // 28. click_element
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["element_id"]
            }
        }),

        // 29. set_element_text
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["element_id", "text"]
            }
        }),

        // batch_actions
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["actions"]
            }
        }),

        // 30. list_windows
        json!({
            "name": "list_windows",
            "description": "Lists all open and visible desktop windows on the target terminal with their HWND, PID, process name, title, minimized status, foreground status, and bounding rectangle coordinates.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "only_visible": {
                        "type": "boolean",
                        "description": "Whether to filter and return only visible non-minimized windows with titles (default: true)."
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": []
            }
        }),

        // 31. focus_window
        json!({
            "name": "focus_window",
            "description": "Activates and brings a desktop window to the foreground on the target terminal, restoring it if minimized. Target window can be specified by title substring, PID, or HWND.",
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": []
            }
        }),

        // 32. close_window
        json!({
            "name": "close_window",
            "description": "Sends a graceful close signal (WM_CLOSE on Windows / AppleScript or SIGTERM on non-Windows) to close a window on the target terminal without forcibly killing the process. Target window can be specified by title substring, PID, or HWND.",
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": []
            }
        }),

        // 32. get_marked_screen
        json!({
            "name": "get_marked_screen",
            "description": "Captures the screen on the target terminal and generates a Set-of-Mark (SoM) annotated image with numbered bounding boxes overlaid on UI elements, custom canvas regions, or grid divisions. Returns marked image Base64 and mark ID to physical coordinate mapping. Result provides 'raw_base64' and 'image_base64' (pure base64 strings safe for decoding), 'data_uri' (Data URL for HTML display), and optional 'server_save_path' to save image directly to local server disk.",
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
                    "server_save_path": {
                        "type": "string",
                        "description": "Optional file path on the central server/host machine to automatically save the decoded marked screenshot directly to disk (e.g. '/tmp/som.jpg')."
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": []
            }
        }),

        // 33. click_mark
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
                    },
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": ["mark_id"]
            }
        }),
    ]
}

