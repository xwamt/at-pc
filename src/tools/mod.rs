//! MCP Tools implementation, schema definitions, and dynamic dispatcher module.

pub mod command;
pub mod file_ops;
pub mod process;
pub mod process_registry;
pub mod screen;
pub mod service;
pub mod sysinfo;

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
                    }
                },
                "required": []
            }
        }),
    ]
}

/// Dispatches an MCP tool call by name with parsed JSON arguments.
pub fn dispatch_mcp_tool(name: &str, arguments: Value) -> Result<Value, String> {
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

            let res = command::exec_powershell(script, timeout_secs, cwd)?;
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

            let res = command::exec_cmd(cmd_str, timeout_secs, cwd)?;
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

            let res = screen::capture_screen(display_index, format, quality)?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }

        unknown => Err(format!("Unknown or unsupported tool '{}'", unknown)),
    }
}
