# at-pc v0.2.0 E2E Validation Protocol & Checklist (Cursor / Claude Desktop)

**Version:** `0.2.0`  
**Date:** 2026-09-01  
**Target Environments:** Windows 10/11 (Primary), macOS Sonoma/Sequoia (Secondary)  
**Supported MCP Clients:** Cursor IDE (v0.40+), Claude Desktop (v0.7+), AT-OpsAgent  

---

## 1. Overview & Test Objective

This document provides a comprehensive 7-step End-to-End (E2E) manual validation protocol for **`at-pc` v0.2.0**.
It verifies that all functional and security gap-closure capabilities (R1–R7) operate correctly in a real LAN environment when paired with an AI Agent running inside Cursor IDE or Claude Desktop.

---

## 2. Pre-requisites & Environment Setup

### 2.1 Host Machine (Troubleshooted PC)
- **OS:** Windows 10 / 11 (x86_64) or macOS (arm64 / x86_64)
- **Binary:** `at-pc` executable built with `cargo build --release`
- **Network:** Connected to local Wi-Fi / Ethernet LAN (obtain local IP, e.g. `192.168.1.50`)

### 2.2 Client Machine (Troubleshooter / Engineer)
- **Network:** Same LAN subnet (or routable to Host IP)
- **Client Application:** Cursor IDE with MCP support or Claude Desktop
- **MCP Client Config Format:**
  ```json
  {
    "mcpServers": {
      "at-pc": {
        "url": "http://<HOST_LAN_IP>:<PORT>/sse",
        "headers": {
          "Authorization": "Bearer <PIN>"
        }
      }
    }
  }
  ```

---

## 3. The 7-Step E2E Verification Protocol

| Step | Test Case | Target Capability | Expected Outcome | Status |
|:----:|:----------|:-----------------|:-----------------|:------:|
| **1** | **App Startup & MCP Config Generation** | GUI Initialization & Config Generation | Port bound (default 9800), 4-digit PIN generated, JSON snippet copyable | `[ ]` |
| **2** | **MCP Connection & Session Tracking** | SSE Handshake & Client IP Audit (R2) | Cursor connects via SSE, active session badge updates to 1, audit log shows `agent_connected` with client IP | `[ ]` |
| **3** | **Network-Enriched System Overview** | `get_system_overview` (R4) | Agent receives OS info, CPU/Memory metrics, disk space, and full network info (`local_ips`, `dns_servers`, `default_gateway`) | `[ ]` |
| **4** | **Screen Capture Diagnostics** | `capture_screen` Tool | Base64-encoded JPEG image returned to Agent; UI displays tool call and client IP in audit stream | `[ ]` |
| **5** | **File Inspection & Service Diagnostics** | `read_text_file` (R3) & `manage_service` | `read_text_file` defaults to tail 200 lines without overflowing context; `manage_service` reports Windows service status | `[ ]` |
| **6** | **Emergency Stop & Kill Switch Verification** | Kill Switch & PIN Invalidation (R1) | Disconnect button invalidates PIN, kills active child processes, all subsequent Agent tool calls immediately fail with `401 Unauthorized` | `[ ]` |
| **7** | **In-App Session Restart & Re-Authentication** | Session Restart & Rotation (R5, R6) | "重新开始协助" generates a fresh PIN and respawns server; Agent reconnects with new PIN; old PIN permanently rejected | `[ ]` |

---

## 4. Detailed Step-by-Step Test Procedure

### Step 1: App Startup & MCP Configuration
1. Double-click or run `./at-pc` on the Host machine.
2. Verify the GUI window appears titled **"💻 at-pc 远程协助助手"** with subtitle **"LAN MCP Remote Troubleshooter (v0.2.0)"**.
3. Verify the status banner displays: `🟢 等待工程师连接... (0 个活跃会话)`.
4. Click the **"📋 复制配置"** (Copy MCP Config) button.
5. Paste into a text editor and verify valid JSON structure:
   ```json
   {
     "mcpServers": {
       "at-pc": {
         "url": "http://192.168.x.x:9800/sse",
         "headers": {
           "Authorization": "Bearer XXXX"
         }
       }
     }
   }
   ```
- [ ] **Pass / Fail**: ____________________

---

### Step 2: Cursor IDE MCP Connection & Client IP Audit
1. On the Engineer machine, open Cursor Settings -> **Features** -> **MCP** -> **Add New MCP Server** (or edit `~/.cursor/mcp.json`).
2. Add the copied config for `at-pc` and click **Save & Refresh**.
3. Observe the `at-pc` desktop application:
   - Status badge transitions to: `🔵 工程师正在协助... (已连接 1 个会话)`.
   - Audit log streams an entry:
     ```text
     [16:40:00] [INFO] [agent_connected] SUCCESS - Client connected from 192.168.x.y
     ```
- [ ] **Pass / Fail**: ____________________

---

### Step 3: Enriched System Overview (`get_system_overview`)
1. In Cursor Composer / Chat, prompt the AI Agent:
   > *"Please inspect the host system overview and network configuration using at-pc."*
2. Verify the agent invokes `get_system_overview`.
3. Inspect JSON response received by Agent:
   - System fields present: `os`, `kernel_version`, `hostname`, `cpu_count`, `cpu_usage_percent`, `memory_total_mb`, `memory_used_mb`.
   - Network fields present (R4):
     - `local_ips`: array of IP strings (e.g. `["192.168.1.50"]`)
     - `dns_servers`: array of DNS server IPs (e.g. `["192.168.1.1"]` or public DNS)
     - `default_gateway`: gateway IP string (e.g. `"192.168.1.1"`)
4. Verify Host GUI audit log shows:
   ```text
   [16:41:00] [INFO] [get_system_overview] SUCCESS (Client: 192.168.x.y)
   ```
- [ ] **Pass / Fail**: ____________________

---

### Step 4: Screen Capture Verification (`capture_screen`)
1. In Cursor, prompt the AI Agent:
   > *"Capture a screenshot of the user's primary monitor to verify visual status."*
2. Verify the Agent invokes `capture_screen` (default `format: "jpeg"`, `quality: 75`).
3. Verify Agent receives a valid Base64 data string and successfully renders or inspects the image.
4. Verify Host GUI audit log updates with `[capture_screen]` and client IP.
- [ ] **Pass / Fail**: ____________________

---

### Step 5: Log File Reading & Service Diagnostics
1. Test default tail behavior (R3):
   - In Cursor, ask the Agent to read a large system or application log file without specifying `tail_lines`.
   - Verify the tool returns **at most 200 lines** (the default tail limit) rather than overloading the context window with megabytes of text.
2. Test service management (Windows / PowerShell):
   - Ask the Agent to query the status of a common service (e.g. `Spooler` on Windows):
     `manage_service(service_name="Spooler", action="status")`
   - Verify response reports `{"status": "Running"}` or `{"status": "Stopped"}`.
- [ ] **Pass / Fail**: ____________________

---

### Step 6: Emergency Stop (Kill Switch) Verification (R1)
1. On the Host PC, the user clicks **"🔴 紧急停止协助 (Emergency Stop)"**.
2. Verify immediate UI changes:
   - Status badge turns red: `🔴 服务已停止 (Service Stopped / Disconnected)`.
   - Disconnect button becomes disabled.
   - Audit log records: `[EMERGENCY] [emergency_stop] STOPPED - Emergency stop activated by user`.
3. In Cursor, prompt the Agent to invoke any tool (e.g. `ping` or `list_processes`).
4. Verify that:
   - The tool call fails immediately.
   - Server returns HTTP status `401 Unauthorized`.
   - Any long-running subprocess spawned earlier is terminated.
- [ ] **Pass / Fail**: ____________________

---

### Step 7: In-App Session Restart & PIN Rotation (R5, R6)
1. On the Host PC, verify the green button **"🟢 重新开始协助 (Start New Session)"** is now visible and active.
2. Click **"🟢 重新开始协助"**.
3. Verify:
   - A new 4-digit PIN is generated (e.g. `7890` different from original `1234`).
   - Server restarts and status returns to `🟢 等待工程师连接... (0 个活跃会话)`.
   - Audit log shows session start divider and `[pin_rotated]`.
4. Attempt to run a tool in Cursor with the **old PIN**:
   - Verify it continues to fail with `401 Unauthorized`.
5. Update Cursor MCP settings with the **new PIN**:
   - Verify Cursor reconnects successfully (`200 OK` / SSE connection established).
   - Agent is once again able to call tools (`get_system_overview`, etc.).
- [ ] **Pass / Fail**: ____________________

---

## 5. Sign-off Criteria for v0.2.0 Release

All items below must be satisfied for release sign-off:
- [x] Full automated test suite passes: `59 passed; 0 failed` across unit and integration tests.
- [x] Package version updated to `0.2.0` in `Cargo.toml` and verified in MCP protocol handshake (`serverInfo.version`).
- [ ] Manual 7-step checklist fully executed on at least one physical/virtual Windows machine.
- [ ] No credential leak in exported logs.
- [ ] Zero unhandled panics during concurrent tool execution or abrupt disconnection.

**Tester Name / Signature:** __________________________________  
**Test Date:** __________________  
**Target OS & Build:** ________________________________________  
**Result:** [ ] PASS  [ ] FAIL
