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
            "description": "Sets the active target terminal for subsequent diagnostic commands in this session.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "terminal_id": {
                        "type": "string",
                        "description": "The unique terminal ID to set as active target."
                    }
                },
                "required": ["terminal_id"]
            }
        }),

        // 3. get_active_terminal
        json!({
            "name": "get_active_terminal",
            "description": "Gets information about the currently selected active terminal.",
            "inputSchema": {
                "type": "object",
                "properties": {},
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
            "description": "Captures a screenshot of the specified monitor display for visual multi-modal diagnostics.",
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
                    "terminal_id": {
                        "type": "string",
                        "description": "Optional target terminal ID (defaults to active terminal)."
                    }
                },
                "required": []
            }
        }),
    ]
}
