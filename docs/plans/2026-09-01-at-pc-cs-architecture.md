# at-pc C/S Architecture Refactoring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor `at-pc` from a single-machine standalone MCP server into a multi-terminal C/S architecture with Cargo Workspace (`crates/protocol`, `crates/server`, `crates/agent`), dynamic WebSocket heartbeat registration, and a centralized MCP Gateway.

**Architecture:** 
- `crates/protocol`: Shared serde types for WebSocket envelopes (`AgentToServerMessage`, `ServerToAgentMessage`), terminal metadata, heartbeat metrics, and tool execution payloads.
- `crates/server`: Axum-based WebSocket hub managing online terminal pool, heartbeat tracking, call_id routing, and exposed MCP Server (stdio/SSE) with session management (`list_terminals`, `select_terminal`, and 9 forwarded diagnostic tools).
- `crates/agent`: Lightweight client application with egui GUI, automatic config reading (`agent_config.toml`), WebSocket client with heartbeat and auto-reconnect, local diagnostics executor, audit log, and emergency disconnect.

**Tech Stack:** Rust (Edition 2021, Tokio, Axum, Tokiotungstenite / Fastwebsockets, Egui/Eframe, Sysinfo, Xcap, Serde, Tracing)

**Spec:** [docs/superpowers/specs/2026-09-01-at-pc-cs-architecture-design.md](file:///Users/clkj/项目/at/docs/superpowers/specs/2026-09-01-at-pc-cs-architecture-design.md)

## Global Constraints
- Target platform: Windows & macOS (cross-platform development with Windows API conditional compilation `cfg(windows)`).
- Rust version: 2021 edition.
- Communication protocol: WebSocket with JSON frames.
- Heartbeat interval: 5 seconds; Offline threshold: 15 seconds.
- No regression on existing 9 diagnostic tools (`get_system_overview`, `exec_powershell`, `exec_cmd`, `list_processes`, `kill_process`, `manage_service`, `read_text_file`, `write_text_file`, `capture_screen`).

---

### Task 1: Cargo Workspace Initialization & Protocol Crate (`crates/protocol`)

**Files:**
- Modify: `at-pc/Cargo.toml`
- Create: `at-pc/crates/protocol/Cargo.toml`
- Create: `at-pc/crates/protocol/src/lib.rs`
- Create: `at-pc/crates/protocol/src/models.rs`
- Create: `at-pc/crates/protocol/src/messages.rs`
- Test: `at-pc/crates/protocol/tests/protocol_test.rs`

**Interfaces:**
- Consumes: None (base dependency).
- Produces: `TerminalInfo`, `HeartbeatMetrics`, `AgentToServerMessage`, `ServerToAgentMessage`, `ToolCallPayload`, `ToolResultPayload`.

- [ ] **Step 1: Write the failing protocol test**

```rust
// at-pc/crates/protocol/tests/protocol_test.rs
use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::{HeartbeatMetrics, TerminalInfo};

#[test]
fn test_register_message_serialization() {
    let info = TerminalInfo {
        terminal_id: "test-pc-01".to_string(),
        hostname: "DESKTOP-TEST".to_string(),
        username: "user".to_string(),
        lan_ip: "192.168.1.100".to_string(),
        os_version: "Windows 11".to_string(),
        agent_version: "0.3.0".to_string(),
    };
    let msg = AgentToServerMessage::Register {
        info,
        auth_token: Some("secret123".to_string()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("test-pc-01"));
    let deserialized: AgentToServerMessage = serde_json::from_str(&json).unwrap();
    match deserialized {
        AgentToServerMessage::Register { info, auth_token } => {
            assert_eq!(info.terminal_id, "test-pc-01");
            assert_eq!(auth_token, Some("secret123".to_string()));
        }
        _ => panic!("Expected Register variant"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p at-pc-protocol`
Expected: FAIL (crate or module not found).

- [ ] **Step 3: Implement `at-pc/Cargo.toml` and `crates/protocol`**

Create `at-pc/crates/protocol/Cargo.toml`, `src/lib.rs`, `src/models.rs`, and `src/messages.rs` defining `TerminalInfo`, `HeartbeatMetrics`, `AgentToServerMessage`, and `ServerToAgentMessage`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p at-pc-protocol`
Expected: PASS.

- [ ] **Step 5: Commit protocol changes**

```bash
git add at-pc/Cargo.toml at-pc/crates/protocol
git commit -m "feat(protocol): add shared C/S message frames and models"
```

---

### Task 2: Server Crate - Terminal Registry & WebSocket Gateway (`crates/server`)

**Files:**
- Create: `at-pc/crates/server/Cargo.toml`
- Create: `at-pc/crates/server/src/config.rs`
- Create: `at-pc/crates/server/src/ws/registry.rs`
- Create: `at-pc/crates/server/src/ws/handler.rs`
- Create: `at-pc/crates/server/src/ws/mod.rs`
- Test: `at-pc/crates/server/tests/registry_test.rs`

**Interfaces:**
- Consumes: `at-pc-protocol` messages.
- Produces: `TerminalRegistry`, `TerminalSession`, `WsGatewayRouter`.

- [ ] **Step 1: Write the failing registry unit test**

```rust
// at-pc/crates/server/tests/registry_test.rs
use at_pc_protocol::models::{HeartbeatMetrics, TerminalInfo};
use at_pc_server::ws::registry::{TerminalRegistry, TerminalStatus};
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_terminal_registration_and_heartbeat() {
    let registry = Arc::new(TerminalRegistry::new());
    let (tx, _rx) = mpsc::unbounded_channel();
    
    let info = TerminalInfo {
        terminal_id: "node-1".to_string(),
        hostname: "HOST-1".to_string(),
        username: "admin".to_string(),
        lan_ip: "10.0.0.2".to_string(),
        os_version: "Windows 10".to_string(),
        agent_version: "0.3.0".to_string(),
    };

    registry.register(info.clone(), tx).await;
    assert_eq!(registry.get_status("node-1").await, Some(TerminalStatus::Online));

    let metrics = HeartbeatMetrics {
        cpu_usage_percent: 12.5,
        memory_used_mb: 2048,
        memory_total_mb: 8192,
        uptime_secs: 3600,
        timestamp: 1725180000,
    };
    registry.update_heartbeat("node-1", metrics).await.unwrap();
    
    let list = registry.list_terminals().await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].info.terminal_id, "node-1");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p at-pc-server --test registry_test`
Expected: FAIL.

- [ ] **Step 3: Implement TerminalRegistry and Axum WebSocket handler**

Implement `TerminalRegistry` with async lock, heartbeat tracking, offline sweeping task, and WebSocket connection handshake handler.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p at-pc-server --test registry_test`
Expected: PASS.

- [ ] **Step 5: Commit server registry**

```bash
git add at-pc/crates/server
git commit -m "feat(server): implement terminal registry and ws gateway"
```

---

### Task 3: Server Crate - MCP Tool Router & Gateway (`crates/server`)

**Files:**
- Create: `at-pc/crates/server/src/router.rs`
- Create: `at-pc/crates/server/src/mcp/tools.rs`
- Create: `at-pc/crates/server/src/mcp/mod.rs`
- Create: `at-pc/crates/server/src/main.rs`
- Test: `at-pc/crates/server/tests/mcp_router_test.rs`

**Interfaces:**
- Consumes: `TerminalRegistry`, `at-pc-protocol`.
- Produces: `dispatch_tool_call(terminal_id, tool_name, args) -> Result<Value>`, `list_terminals`, `select_terminal`.

- [ ] **Step 1: Write the failing MCP Router test**

```rust
// at-pc/crates/server/tests/mcp_router_test.rs
use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::TerminalInfo;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::registry::TerminalRegistry;
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_route_tool_to_target_terminal() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let (tx, mut rx) = mpsc::unbounded_channel();

    let info = TerminalInfo {
        terminal_id: "agent-007".to_string(),
        hostname: "PC-007".to_string(),
        username: "bond".to_string(),
        lan_ip: "192.168.1.50".to_string(),
        os_version: "Windows 11".to_string(),
        agent_version: "0.3.0".to_string(),
    };
    registry.register(info, tx).await;

    // Simulate router forwarding
    let router_clone = router.clone();
    tokio::spawn(async move {
        if let Some(ServerToAgentMessage::InvokeTool { call_id, tool_name, .. }) = rx.recv().await {
            assert_eq!(tool_name, "exec_powershell");
            router_clone.handle_tool_result(AgentToServerMessage::ToolResult {
                call_id,
                success: true,
                result: serde_json::json!({ "stdout": "hello", "exit_code": 0 }),
                error: None,
                duration_ms: 50,
            }).await;
        }
    });

    let res = router.invoke_tool("agent-007", "exec_powershell", serde_json::json!({"script": "echo hello"}), 5).await.unwrap();
    assert_eq!(res["stdout"], "hello");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p at-pc-server --test mcp_router_test`
Expected: FAIL.

- [ ] **Step 3: Implement McpRouter and MCP tool endpoints**

Implement `McpRouter` using `DashMap<CallId, oneshot::Sender>` for pending calls, `list_terminals`, `select_terminal`, session memory, and full MCP JSON-RPC router.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p at-pc-server --test mcp_router_test`
Expected: PASS.

- [ ] **Step 5: Commit server MCP router**

```bash
git add at-pc/crates/server
git commit -m "feat(server): add MCP tool router and dynamic dispatcher"
```

---

### Task 4: Agent Crate - Configuration, Execution Engine & WebSocket Client (`crates/agent`)

**Files:**
- Create: `at-pc/crates/agent/Cargo.toml`
- Create: `at-pc/crates/agent/src/config.rs`
- Migrate/Adapt: `at-pc/crates/agent/src/tools/` (`mod.rs`, `sysinfo.rs`, `command.rs`, `process.rs`, `service.rs`, `file_ops.rs`, `screen.rs`)
- Create: `at-pc/crates/agent/src/executor.rs`
- Create: `at-pc/crates/agent/src/ws_client.rs`
- Test: `at-pc/crates/agent/tests/agent_executor_test.rs`

**Interfaces:**
- Consumes: `at-pc-protocol`, config (`agent_config.toml` or embedded env).
- Produces: `AgentWsClient`, `AgentExecutor`.

- [ ] **Step 1: Write the failing Agent Executor test**

```rust
// at-pc/crates/agent/tests/agent_executor_test.rs
use at_pc_agent::executor::AgentExecutor;

#[tokio::test]
async fn test_executor_runs_system_overview() {
    let executor = AgentExecutor::new();
    let res = executor.execute("get_system_overview", serde_json::json!({})).await;
    assert!(res.is_ok());
    let val = res.unwrap();
    assert!(val.get("os_name").is_some() || val.get("os").is_some());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p at-pc-agent --test agent_executor_test`
Expected: FAIL.

- [ ] **Step 3: Implement config loader, adapt diagnostic tools, executor & ws_client**

1. Implement `config.rs` loading `agent_config.toml` with default fallback.
2. Adapt the 9 diagnostic tools into `crates/agent/src/tools/`.
3. Implement `executor.rs` matching `tool_name` and executing tool asynchronously.
4. Implement `ws_client.rs` connecting to Server WS, sending `Register`, maintaining 5s `Heartbeat`, executing incoming `InvokeTool`, and sending `ToolResult`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p at-pc-agent --test agent_executor_test`
Expected: PASS.

- [ ] **Step 5: Commit agent backend engine**

```bash
git add at-pc/crates/agent
git commit -m "feat(agent): implement config loader, execution engine and ws client"
```

---

### Task 5: Agent Crate - egui Desktop UI & Emergency Disconnect (`crates/agent`)

**Files:**
- Create: `at-pc/crates/agent/src/app/mod.rs`
- Create: `at-pc/crates/agent/src/app/ui.rs`
- Create: `at-pc/crates/agent/src/main.rs`
- Test: `at-pc/crates/agent/tests/agent_app_state_test.rs`

**Interfaces:**
- Consumes: `AgentWsClient`, `AgentExecutor`, `AppState`.
- Produces: `run_agent_app()`, desktop binary `at-pc-agent`.

- [ ] **Step 1: Write failing AppState audit log test**

```rust
// at-pc/crates/agent/tests/agent_app_state_test.rs
use at_pc_agent::app::AgentAppState;
use std::sync::Arc;

#[test]
fn test_agent_app_state_audit_stream() {
    let state = Arc::new(AgentAppState::new("ws://127.0.0.1:9801/ws".to_string()));
    state.add_audit_log("exec_powershell", "Get-Process", "SUCCESS");
    let logs = state.get_audit_logs();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].tool_name, "exec_powershell");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p at-pc-agent --test agent_app_state_test`
Expected: FAIL.

- [ ] **Step 3: Implement egui UI, audit logger, and emergency disconnect**

Implement `AgentAppState` and `eframe` window rendering server connection status, terminal ID, live audit logs, and an emergency disconnect button.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p at-pc-agent --test agent_app_state_test`
Expected: PASS.

- [ ] **Step 5: Commit agent UI**

```bash
git add at-pc/crates/agent
git commit -m "feat(agent): implement egui UI, audit logs, and emergency disconnect"
```

---

### Task 6: End-to-End Integration & Multi-Terminal Verification

**Files:**
- Create: `at-pc/tests/e2e_cs_test.rs`

- [ ] **Step 1: Write End-to-End Integration Test**

```rust
// at-pc/tests/e2e_cs_test.rs
use at_pc_agent::executor::AgentExecutor;
use at_pc_agent::ws_client::AgentWsClient;
use at_pc_protocol::models::TerminalInfo;
use at_pc_server::config::ServerConfig;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::registry::TerminalRegistry;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn test_full_cs_registration_and_mcp_routing() {
    // 1. Start Server WS Hub
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry.clone()));
    let port = 19801;
    let ws_url = format!("ws://127.0.0.1:{}/ws", port);
    
    // Spawn server in background
    let registry_clone = registry.clone();
    let router_clone = router.clone();
    tokio::spawn(async move {
        at_pc_server::ws::start_ws_server(registry_clone, router_clone, port).await.unwrap();
    });
    sleep(Duration::from_millis(200)).await;

    // 2. Start Agent 1
    let agent1 = AgentWsClient::new(
        ws_url.clone(),
        TerminalInfo {
            terminal_id: "pc-agent-1".to_string(),
            hostname: "PC-ALPHA".to_string(),
            username: "alice".to_string(),
            lan_ip: "192.168.1.101".to_string(),
            os_version: "Windows 11".to_string(),
            agent_version: "0.3.0".to_string(),
        },
        Arc::new(AgentExecutor::new()),
    );
    tokio::spawn(async move { agent1.run().await; });

    // 3. Start Agent 2
    let agent2 = AgentWsClient::new(
        ws_url.clone(),
        TerminalInfo {
            terminal_id: "pc-agent-2".to_string(),
            hostname: "PC-BETA".to_string(),
            username: "bob".to_string(),
            lan_ip: "192.168.1.102".to_string(),
            os_version: "Windows 10".to_string(),
            agent_version: "0.3.0".to_string(),
        },
        Arc::new(AgentExecutor::new()),
    );
    tokio::spawn(async move { agent2.run().await; });

    sleep(Duration::from_millis(500)).await;

    // 4. Verify Server lists both terminals
    let list = router.list_terminals().await;
    assert_eq!(list.len(), 2);

    // 5. Test Router forwarding to Agent 1
    let res = router.invoke_tool("pc-agent-1", "get_system_overview", serde_json::json!({}), 10).await.unwrap();
    assert!(res.get("cpu").is_some() || res.get("os_name").is_some());
}
```

- [ ] **Step 2: Run End-to-End Test**

Run: `cargo test --test e2e_cs_test`
Expected: PASS.

- [ ] **Step 3: Commit full test suite and clean up deprecated single-binary structure**

```bash
git add at-pc/
git commit -m "feat(at-pc): complete C/S architecture refactor with e2e tests"
```
