# Design Specification: at-pc (LAN-based Rust MCP Remote PC Troubleshooter)

## 1. Overview & Problem Statement

In daily office environments, non-technical colleagues frequently encounter computer issues (system lag, printing service failures, network anomalies, software crashes, configuration errors) and ask IT engineers for help. These colleagues typically do not know how to interact with AI or provide technical context.

**at-pc** is a lightweight, single-binary Windows desktop application written in Rust. It runs an HTTP/SSE Model Context Protocol (MCP) server directly on the colleague's computer within the local area network (LAN). An IT engineer's AI Agent (such as Claude Desktop, Cursor, Cline, or custom agent frameworks) can connect to this MCP server over the LAN, diagnose the machine using rich MCP tools, inspect logs, take screenshots, and run troubleshooting scripts to resolve issues automatically.

---

## 2. Architecture & Technical Stack

```
+-----------------------------------------------------------------------------------+
|                        Colleague Computer (Windows Target)                        |
|                                                                                   |
|  +-----------------------------------------------------------------------------+  |
|  |                    egui / eframe GUI (Zero External Runtime)                |  |
|  |  - Displays LAN IP, Port, Random 4-digit PIN, Session Status                |  |
|  |  - Live Audit Log Viewer (Real-time Tool Execution Stream)                  |  |
|  |  - [Copy MCP Config] button & [Emergency Stop / Disconnect] button          |  |
|  +-----------------------------------------------------------------------------+  |
|                                        | tokio::sync::mpsc (event stream)         |
|  +-------------------------------------v---------------------------------------+  |
|  |                        Axum + RMCP (HTTP / SSE Server)                      |  |
|  |  - Bind to 0.0.0.0:9800 (Auto-fallback to next free port if occupied)       |  |
|  |  - Auth Middleware: validates `Authorization: Bearer <PIN>` header          |  |
|  |  - MCP Endpoints: `/sse`, `/messages` (or Streamable HTTP POST)            |  |
|  +-----------------------------------------------------------------------------+  |
|                                        | tool invocation                          |
|  +-------------------------------------v---------------------------------------+  |
|  |                            Diagnostics & Execution Engine                   |  |
|  |  - sysinfo (OS, CPU, RAM, Disks, Network interfaces, DNS, Default Gateway)   |  |
|  |  - windows-rs (Windows Services inspection, control, Process management)     |  |
|  |  - std::process::Command (Silent PowerShell / CMD execution with timeout)   |  |
|  |  - xcap (High-performance multi-monitor screenshot capture)                 |  |
|  |  - file_ops (Safe text/log reading with line tailing & backup writes)       |  |
|  +-----------------------------------------------------------------------------+  |
+-----------------------------------------------------------------------------------+
                                         ^
                                         | LAN HTTP / SSE (Bearer <PIN>)
+-----------------------------------------------------------------------------------+
|                            Engineer Computer (AI Agent)                           |
|  - MCP Config: `http://<colleague_ip>:9800/sse` with Header `Authorization`      |
|  - Agent diagnoses, inspects screenshots, executes PowerShell, fixes service     |
+-----------------------------------------------------------------------------------+
```

### 2.1 Dependencies & Rust Crates

| Layer | Recommended Crates | Purpose |
| :--- | :--- | :--- |
| **GUI Framework** | `eframe`, `egui` | Native pure-Rust UI, ~5MB binary, zero WebView2 dependency |
| **Async & Network** | `tokio`, `axum`, `tower-http` | Async runtime, HTTP server & CORS/Auth middleware |
| **MCP SDK** | `rmcp` (or custom MCP JSON-RPC 2.0 SSE router) | Official Rust MCP implementation for Tools/Resources/Prompts |
| **System Info** | `sysinfo`, `local-ip-address` | CPU/RAM/Disk metrics, network IP, interface list |
| **Windows API** | `windows` / `windows-sys` | Windows Services (SCM API), elevated token checks |
| **Screen Capture** | `xcap` | Cross-platform, pure-Rust multi-screen screenshot capture |
| **Utilities** | `serde`, `serde_json`, `tracing`, `rand`, `base64` | Serialization, structured logging, PIN generation |

---

## 3. MCP Tool Definitions

The server registers 9 core diagnostic and remediation tools:

### 3.1 `get_system_overview`
* **Description**: Returns complete hardware and operating system status (OS version, CPU model/load, RAM usage, storage volumes, network interfaces, local IPs, DNS servers, and system uptime).
* **Inputs**: None
* **Returns**: JSON object with detailed hardware, OS, and network info.

### 3.2 `exec_powershell`
* **Description**: Executes a PowerShell script in a hidden background process (no popup window) with timeout protection and stdout/stderr capture.
* **Inputs**:
  * `script` (string, required): PowerShell command/script block.
  * `timeout_secs` (integer, optional, default: 30): Max execution time.
  * `cwd` (string, optional): Working directory.
* **Returns**: `{ stdout: string, stderr: string, exit_code: i32, duration_ms: u64 }`

### 3.3 `exec_cmd`
* **Description**: Executes standard Windows CMD command in the background.
* **Inputs**:
  * `command` (string, required): Command string to execute.
  * `timeout_secs` (integer, optional, default: 30): Max execution time.
  * `cwd` (string, optional): Working directory.
* **Returns**: `{ stdout: string, stderr: string, exit_code: i32, duration_ms: u64 }`

### 3.4 `list_processes`
* **Description**: Lists running processes on the system with PID, process name, memory footprint (MB), CPU usage, and binary path.
* **Inputs**:
  * `filter_name` (string, optional): Filter by process name substring.
  * `sort_by` (enum: `"cpu" | "memory" | "pid"`, optional, default: `"memory"`).
  * `limit` (integer, optional, default: 50): Max items returned.
* **Returns**: Array of process summary objects.

### 3.5 `kill_process`
* **Description**: Terminates a process by PID or exact executable name.
* **Inputs**:
  * `pid` (integer, optional): Target PID.
  * `process_name` (string, optional): Target process name (e.g., `notepad.exe`).
  * `force` (boolean, optional, default: true): Force terminate immediately.
* **Returns**: Success confirmation or error message.

### 3.6 `manage_service`
* **Description**: Inspects or controls Windows Services (e.g., `Spooler`, `wuauserv`, `Dnscache`, `LanmanWorkstation`).
* **Inputs**:
  * `service_name` (string, required): System name of the service.
  * `action` (enum: `"status" | "start" | "stop" | "restart"`, required).
* **Returns**: Service status details (Name, Display Name, Status: Running/Stopped, Start Type).

### 3.7 `read_text_file`
* **Description**: Reads a log or configuration file with line-limiting and tail-reading options to prevent overflowing context windows.
* **Inputs**:
  * `file_path` (string, required): Absolute path to target file.
  * `tail_lines` (integer, optional, default: 200): Read only the last N lines.
  * `max_bytes` (integer, optional, default: 512000): Max size limit.
* **Returns**: Text content, total line count, and truncated indicator.

### 3.8 `write_text_file`
* **Description**: Safely writes or patches a configuration file. Automatically creates a `.bak` backup before modification if requested.
* **Inputs**:
  * `file_path` (string, required): Absolute path to file.
  * `content` (string, required): Text content to write.
  * `create_backup` (boolean, optional, default: true): Whether to create a `.bak` timestamped backup.
* **Returns**: Success status and backup file path.

### 3.9 `capture_screen`
* **Description**: Takes a screenshot of the primary screen or all active monitors for visual AI multi-modal diagnostics (e.g. error popups, UI hangs).
* **Inputs**:
  * `display_index` (integer, optional, default: 0): Monitor index.
  * `format` (enum: `"jpeg" | "png"`, optional, default: `"jpeg"`).
  * `quality` (integer, optional, default: 80): JPEG compression quality (1-100).
* **Returns**: Base64 data URI image string + monitor dimensions.

---

## 4. Security & Safety Model

1. **Authentication**:
   * Every launch randomly generates a 4-digit numeric PIN.
   * All HTTP / SSE requests must supply `Authorization: Bearer <PIN>` (or `?token=<PIN>`).
   * Unauthorized requests are rejected with `401 Unauthorized`.
2. **Real-Time Audit Logging**:
   * Every tool call (tool name, parameters, execution timestamp, and status) is streamed in real-time to the colleague's UI audit list.
3. **Emergency Disconnect (Kill Switch)**:
   * Colleague can click the red **【断开协助 / Disconnect】** button at any time.
   * Clicking Disconnect immediately closes the HTTP/SSE listener, invalidates the PIN, cancels active subprocesses, and resets to an idle state.
4. **Command Execution Safety**:
   * Subprocesses run with strict timeouts to prevent hanging commands.
   * Non-interactive mode enforced (`-NonInteractive -NoProfile` for PowerShell) to prevent blocking on user input.

---

## 5. UI/UX Workflow

```
+-------------------------------------------------------------+
| at-pc 远程协助助手 v0.1.0                         [-] [x]   |
+-------------------------------------------------------------+
|                                                             |
|  状态: [● 正在等待工程师连接...]                             |
|                                                             |
|  +-------------------------------------------------------+  |
|  |  局域网 IP : 192.168.1.108                             |  |
|  |  服务端口  : 9800                                      |  |
|  |  连接 PIN  : 7391                                      |  |
|  +-------------------------------------------------------+  |
|                                                             |
|  [ 一键复制 MCP 配置 JSON ]       [ 重新生成 PIN / 切换端口 ]  |
|                                                             |
|  ----------------- 实时操作审计日志 --------------------    |
|  [15:12:01] 服务已在 0.0.0.0:9800 启动                      |
|  [15:12:30] 工程师 Agent 已连接 (IP: 192.168.1.50)           |
|  [15:12:31] 执行工具: get_system_overview                   |
|  [15:12:35] 执行工具: capture_screen (主屏幕截图)            |
|  [15:12:40] 执行工具: manage_service (重启 Print Spooler)    |
|                                                             |
|  +-------------------------------------------------------+  |
|  |               [ 🔴 立即断开协助 (Emergency Stop) ]      |  |
|  +-------------------------------------------------------+  |
+-------------------------------------------------------------+
```

---

## 6. Directory Structure (`at-pc`)

```
at-pc/
├── Cargo.toml
├── docs/
│   └── specs/
│       └── 2026-09-01-lan-pc-troubleshooter-mcp-design.md
├── src/
│   ├── main.rs                  # Application entrypoint & eframe GUI setup
│   ├── app/
│   │   ├── mod.rs               # egui UI state, layout & event loops
│   │   └── ui.rs                # UI rendering components
│   ├── server/
│   │   ├── mod.rs               # Axum HTTP/SSE server lifecycle
│   │   ├── auth.rs              # Bearer PIN token auth middleware
│   │   ├── sse.rs               # SSE transport & message router
│   │   └── state.rs             # Shared AppState & event broadcast channels
│   ├── tools/
│   │   ├── mod.rs               # MCP Tool registry & dispatcher
│   │   ├── sysinfo.rs           # System overview & hardware metrics
│   │   ├── command.rs           # PowerShell & CMD hidden executor
│   │   ├── process.rs           # Process listing & termination
│   │   ├── service.rs           # Windows Service control
│   │   ├── file_ops.rs          # Safe log/config read & write
│   │   └── screen.rs            # xcap screenshot capture
│   └── utils/
│       ├── network.rs           # Local LAN IP detection & port allocation
│       └── security.rs          # Random PIN generator & sanitizer
```

---

## 7. Verification & Testing Plan

1. **Unit & Capability Tests**:
   - `test_sysinfo`: Validate retrieval of OS, memory, disk, and network interfaces.
   - `test_command_execution`: Test PowerShell/CMD execution, timeout aborts, and exit code capture.
   - `test_file_ops`: Test tail read limit and safe backup creation.
   - `test_screen_capture`: Test xcap capture and JPEG base64 encoding.
2. **Integration Tests (MCP & Network)**:
   - Start server on localhost.
   - Test unauthorized requests (401 response).
   - Test authorized SSE connection with `Authorization: Bearer <PIN>`.
   - Send `tools/list` and `tools/call` JSON-RPC payloads and assert response schemas.
3. **End-to-End Test with Agent**:
   - Configure Claude Desktop / Cline / Cursor MCP config pointing to `at-pc` over LAN.
   - Agent performs automated diagnosis (gets system overview, captures screen, inspects services).
