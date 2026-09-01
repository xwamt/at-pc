# at-pc (LAN-based Rust MCP Remote Troubleshooter) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a lightweight, single-binary Windows desktop application in Rust that hosts an authenticated HTTP/SSE Model Context Protocol (MCP) server on LAN, enabling an IT engineer's AI Agent to diagnose, inspect, and repair colleague PC issues remotely.

**Architecture:** The application pairs an `eframe`/`egui` lightweight native desktop UI with an embedded `axum`/`tokio` HTTP/SSE server. The server exposes 9 diagnostic and remediation MCP tools (system metrics, silent PowerShell/CMD execution, process & service management, safe file/log I/O, and multi-monitor screen capture) protected by dynamic 4-digit PIN authentication, real-time UI audit logging, and an emergency disconnect kill switch.

**Tech Stack:** Rust 2021, `eframe` / `egui`, `tokio`, `axum`, `tower-http`, `sysinfo`, `xcap`, `image`, `serde`, `serde_json`, `rand`, `local-ip-address`, `windows-sys` (target Windows).

**Spec:** [`at-pc/docs/specs/2026-09-01-lan-pc-troubleshooter-mcp-design.md`](file:///Users/clkj/项目/at/at-pc/docs/specs/2026-09-01-lan-pc-troubleshooter-mcp-design.md)

## Global Constraints

- Must compile as a standalone binary with zero external runtime requirements (no Node.js, Python, or WebView2 required).
- Must enforce non-blocking, non-interactive execution with strict timeout limits on all subprocesses.
- All HTTP / SSE endpoints must strictly validate dynamic PIN authentication.
- Must provide clear cross-platform stubs/fallbacks so unit tests and development pass on both macOS/Linux host machines and Windows targets.

---

### Task 1: Project Scaffolding & Cargo Setup

**Files:**
- Create: `at-pc/Cargo.toml`
- Create: `at-pc/src/main.rs`
- Create: `at-pc/src/lib.rs`
- Create: `at-pc/src/utils/mod.rs`
- Create: `at-pc/src/tools/mod.rs`
- Create: `at-pc/src/server/mod.rs`
- Create: `at-pc/src/app/mod.rs`

**Interfaces:**
- Produces: Base module tree and compiled binary skeleton.

- [ ] **Step 1: Create Cargo.toml with dependencies**

```toml
[package]
name = "at-pc"
version = "0.1.0"
edition = "2021"
description = "LAN-based Rust MCP Remote PC Troubleshooter"

[dependencies]
tokio = { version = "1.38", features = ["full"] }
axum = { version = "0.7", features = ["macros"] }
tower-http = { version = "0.5", features = ["cors", "trace"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
sysinfo = "0.30"
local-ip-address = "0.6"
rand = "0.8"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
eframe = { version = "0.27", default-features = false, features = ["default_fonts", "glow"] }
egui = "0.27"
base64 = "0.22"
xcap = "0.0.14"
image = { version = "0.25", default-features = false, features = ["jpeg", "png"] }
chrono = { version = "0.4", features = ["serde"] }

[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.52", features = [
    "Win32_System_Services",
    "Win32_Security",
    "Win32_Foundation",
    "Win32_System_Threading"
] }

[dev-dependencies]
reqwest = { version = "0.12", features = ["json", "stream"] }
```

- [ ] **Step 2: Create minimal module structure and lib.rs / main.rs**

Create `at-pc/src/lib.rs`, `at-pc/src/utils/mod.rs`, `at-pc/src/tools/mod.rs`, `at-pc/src/server/mod.rs`, `at-pc/src/app/mod.rs`.

- [ ] **Step 3: Verify build**

Run: `cargo check --manifest-path at-pc/Cargo.toml`
Expected: PASS (Finished dev profile)

---

### Task 2: Utility & Security Core (Network IP Detection & PIN Authentication)

**Files:**
- Create: `at-pc/src/utils/network.rs`
- Create: `at-pc/src/utils/security.rs`
- Create: `at-pc/src/server/auth.rs`
- Test: `at-pc/tests/test_utils_security.rs`

**Interfaces:**
- Produces:
  - `utils::network::get_lan_ip() -> String`
  - `utils::network::find_available_port(start: u16) -> u16`
  - `utils::security::generate_pin() -> String`
  - `utils::security::verify_pin(expected: &str, provided: &str) -> bool`
  - `server::auth::validate_auth_header(expected_pin: &str, header_val: Option<&str>) -> bool`

- [ ] **Step 1: Write failing unit test for security and network utils**

```rust
// at-pc/tests/test_utils_security.rs
use at_pc::utils::{network, security};

#[test]
fn test_pin_generation_format() {
    let pin = security::generate_pin();
    assert_eq!(pin.len(), 4);
    assert!(pin.chars().all(|c| c.is_ascii_digit()));
}

#[test]
fn test_pin_verification() {
    assert!(security::verify_pin("1234", "1234"));
    assert!(!security::verify_pin("1234", "0000"));
    assert!(!security::verify_pin("1234", "123"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test test_utils_security --manifest-path at-pc/Cargo.toml`
Expected: FAIL (modules not found / not implemented)

- [ ] **Step 3: Implement `network.rs`, `security.rs`, and `auth.rs`**

Implement PIN generation with `rand::Rng`, local IP resolution via `local-ip-address` with `127.0.0.1` fallback, and port search using `std::net::TcpListener::bind`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test test_utils_security --manifest-path at-pc/Cargo.toml`
Expected: PASS

---

### Task 3: Diagnostics Engine - System Overview & Process Management

**Files:**
- Create: `at-pc/src/tools/sysinfo.rs`
- Create: `at-pc/src/tools/process.rs`
- Test: `at-pc/tests/test_tools_system.rs`

**Interfaces:**
- Produces:
  - `tools::sysinfo::get_system_overview() -> SystemOverview`
  - `tools::process::list_processes(filter: Option<&str>, sort_by: Option<&str>, limit: usize) -> Vec<ProcessInfo>`
  - `tools::process::kill_process(pid: Option<u32>, name: Option<&str>, force: bool) -> Result<String, String>`

- [ ] **Step 1: Write failing unit test for sysinfo and process listing**

```rust
// at-pc/tests/test_tools_system.rs
use at_pc::tools::{sysinfo, process};

#[test]
fn test_get_system_overview() {
    let overview = sysinfo::get_system_overview();
    assert!(!overview.os_name.is_empty());
    assert!(overview.total_memory_mb > 0);
    assert!(!overview.cpu_model.is_empty());
}

#[test]
fn test_list_processes() {
    let procs = process::list_processes(None, Some("memory"), 10);
    assert!(!procs.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test test_tools_system --manifest-path at-pc/Cargo.toml`
Expected: FAIL

- [ ] **Step 3: Implement `sysinfo.rs` and `process.rs`**

Use `sysinfo::System` to gather CPU, memory, disks, networks, and running processes. Implement sort by memory/cpu/pid and process termination logic.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test test_tools_system --manifest-path at-pc/Cargo.toml`
Expected: PASS

---

### Task 4: Diagnostics Engine - Silent Command Execution & File Operations

**Files:**
- Create: `at-pc/src/tools/command.rs`
- Create: `at-pc/src/tools/file_ops.rs`
- Test: `at-pc/tests/test_tools_cmd_file.rs`

**Interfaces:**
- Produces:
  - `tools::command::exec_powershell(script: &str, timeout_secs: u64, cwd: Option<&str>) -> Result<CommandResult, String>`
  - `tools::command::exec_cmd(command: &str, timeout_secs: u64, cwd: Option<&str>) -> Result<CommandResult, String>`
  - `tools::file_ops::read_text_file(file_path: &str, tail_lines: Option<usize>, max_bytes: Option<usize>) -> Result<FileContentResult, String>`
  - `tools::file_ops::write_text_file(file_path: &str, content: &str, create_backup: bool) -> Result<FileWriteResult, String>`

- [ ] **Step 1: Write failing unit test for command runner and file ops**

```rust
// at-pc/tests/test_tools_cmd_file.rs
use at_pc::tools::{command, file_ops};
use std::io::Write;

#[test]
fn test_read_and_write_file_with_backup() {
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("at_pc_test_file.txt");
    
    // Write initial
    let _ = file_ops::write_text_file(test_file.to_str().unwrap(), "Hello World\nLine 2", false);
    let read_res = file_ops::read_text_file(test_file.to_str().unwrap(), Some(10), None).unwrap();
    assert_eq!(read_res.content, "Hello World\nLine 2");
    
    // Overwrite with backup
    let _ = file_ops::write_text_file(test_file.to_str().unwrap(), "Modified", true);
    let _ = std::fs::remove_file(test_file);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test test_tools_cmd_file --manifest-path at-pc/Cargo.toml`
Expected: FAIL

- [ ] **Step 3: Implement `command.rs` and `file_ops.rs`**

- `command.rs`: Handle background non-interactive execution with `CREATE_NO_WINDOW` on Windows (and standard `tokio::process::Command` with `tokio::time::timeout` on Unix/testing).
- `file_ops.rs`: Safe line tailing, file size checks, and automatic `.bak` timestamped backups.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test test_tools_cmd_file --manifest-path at-pc/Cargo.toml`
Expected: PASS

---

### Task 5: Diagnostics Engine - Windows Service Control & Screen Capture

**Files:**
- Create: `at-pc/src/tools/service.rs`
- Create: `at-pc/src/tools/screen.rs`
- Test: `at-pc/tests/test_tools_service_screen.rs`

**Interfaces:**
- Produces:
  - `tools::service::manage_service(service_name: &str, action: &str) -> Result<ServiceStatusResult, String>`
  - `tools::screen::capture_screen(display_index: usize, format: &str, quality: u8) -> Result<ScreenCaptureResult, String>`

- [ ] **Step 1: Write failing unit test for screen capture and service control stubs**

```rust
// at-pc/tests/test_tools_service_screen.rs
use at_pc::tools::{service, screen};

#[test]
fn test_capture_screen_returns_base64() {
    let res = screen::capture_screen(0, "jpeg", 75);
    // On headless test runners, verify it handles gracefully or returns valid image
    match res {
        Ok(capture) => {
            assert!(!capture.base64_data.is_empty());
            assert!(capture.width > 0);
        }
        Err(e) => {
            println!("Screen capture not supported in headless test environment: {}", e);
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test test_tools_service_screen --manifest-path at-pc/Cargo.toml`
Expected: FAIL

- [ ] **Step 3: Implement `service.rs` and `screen.rs`**

- `service.rs`: Use Windows SCM API on `target_os = "windows"` via `windows-sys` (or PowerShell `Get-Service`/`Restart-Service` fallback); cross-platform stub for non-windows tests.
- `screen.rs`: Use `xcap::Monitor` to grab frame, compress to JPEG with `image` crate, encode to Base64 data URI.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test test_tools_service_screen --manifest-path at-pc/Cargo.toml`
Expected: PASS

---

### Task 6: MCP JSON-RPC Server & SSE Transport

**Files:**
- Create: `at-pc/src/server/state.rs`
- Create: `at-pc/src/server/sse.rs`
- Create: `at-pc/src/server/router.rs`
- Modify: `at-pc/src/tools/mod.rs` (Dispatch table for MCP Tools)
- Modify: `at-pc/src/server/mod.rs` (Server lifecycle & shutdown control)
- Test: `at-pc/tests/test_mcp_server.rs`

**Interfaces:**
- Produces:
  - `server::state::AppState` with audit log channels & active session tracking
  - `server::router::create_mcp_router(state: Arc<AppState>) -> axum::Router`
  - `server::start_server(state: Arc<AppState>, port: u16) -> tokio::task::JoinHandle<()>`
  - `server::stop_server()`

- [ ] **Step 1: Write failing integration test for MCP SSE protocol and Auth**

```rust
// at-pc/tests/test_mcp_server.rs
use at_pc::server::{state::AppState, router::create_mcp_router};
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::test]
async fn test_mcp_tools_list_and_auth() {
    let state = Arc::new(AppState::new("9999".to_string(), 9811));
    let app = create_mcp_router(state);
    
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    
    let client = reqwest::Client::new();
    // 1. Unauthorized request
    let resp = client.post(format!("http://{}/messages", addr))
        .json(&serde_json::json!({"jsonrpc": "2.0", "method": "tools/list", "id": 1}))
        .send().await.unwrap();
    assert_eq!(resp.status(), 401);
    
    // 2. Authorized request
    let resp = client.post(format!("http://{}/messages", addr))
        .header("Authorization", "Bearer 9999")
        .json(&serde_json::json!({"jsonrpc": "2.0", "method": "tools/list", "id": 1}))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test test_mcp_server --manifest-path at-pc/Cargo.toml`
Expected: FAIL

- [ ] **Step 3: Implement MCP SSE endpoints and tool dispatcher**

Implement MCP JSON-RPC 2.0 protocol (`initialize`, `tools/list`, `tools/call`) handling all 9 diagnostic tools, broadcasting each invocation to the AppState audit channel.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test test_mcp_server --manifest-path at-pc/Cargo.toml`
Expected: PASS

---

### Task 7: egui Desktop UI & Real-Time Audit Log Viewer

**Files:**
- Create: `at-pc/src/app/ui.rs`
- Create: `at-pc/src/app/state.rs`
- Modify: `at-pc/src/app/mod.rs`
- Modify: `at-pc/src/main.rs`

**Interfaces:**
- Produces: Complete desktop GUI with IP/Port/PIN display, real-time audit stream, Copy MCP Config button, and Emergency Disconnect button.

- [ ] **Step 1: Implement `app/ui.rs` rendering component**

Design `egui` layout:
- Header: Status badge (`🟢 等待工程师连接...` / `🔵 工程师正在协助...`).
- Info Card: LAN IP, Port, PIN (large font), copyable MCP JSON configuration.
- Audit Log Box: Scrollable, timestamped list of real-time tool executions.
- Footer: Big Red `[ 🔴 立即断开协助 (Emergency Stop) ]` button.

- [ ] **Step 2: Connect UI event loops with Tokio Background Server**

In `main.rs`, initialize the background `tokio` runtime, start the Axum MCP server on detected LAN IP and free port, and launch `eframe::run_native`.

- [ ] **Step 3: Verify build and run smoke check**

Run: `cargo check --manifest-path at-pc/Cargo.toml`
Expected: PASS

- [ ] **Step 4: End-to-end integration test**

Run all automated unit and integration tests:
Run: `cargo test --manifest-path at-pc/Cargo.toml`
Expected: All tests PASS.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-09-01-at-pc-implementation.md` and `at-pc/docs/plans/2026-09-01-at-pc-implementation.md`.

Two execution options:

1. **Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** - Execute tasks in this session using `executing-plans`, batch execution with checkpoints.

**Which approach?**
