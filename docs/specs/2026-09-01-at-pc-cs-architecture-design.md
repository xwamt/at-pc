# Design Specification: at-pc C/S Architecture Refactoring

## 1. 概述与背景 (Overview & Problem Statement)

在原有 `at-pc` 架构中，每台被协助的 PC 作为一个单体程序，自身启动一个独立的 HTTP/SSE MCP Server。随着接入终端数量增加，工程师/AI Agent 面临严重的配置与维护问题：
1. **静态配置繁重**：每加入一台新终端，都必须手动修改 AI Agent 的 MCP 配置文件并填入 IP/端口/PIN。
2. **网络变动不可控**：局域网 DHCP 导致 IP 变动后，配置直接失效。
3. **缺乏集中纳管**：无法集中查看多台终端的在线状态和系统概要。

本项目将 `at-pc` 重构成**服务端与代理端分离（Server-Agent）的 C/S 架构**：
* **代理端（`at-pc-agent`）**：运行在被协助 PC 上的轻量客户端，打包时预置服务端地址，启动后通过 WebSocket 反向连入服务端并持续发送心跳。
* **服务端（`at-pc-server`）**：集中运行的统一网关，负责维护动态终端池、接收心跳与状态同步，并对外提供唯一的 MCP 服务接口。
* **AI Agent**：仅需配置唯一的服务端 MCP 地址，即可通过 `list_terminals` / `select_terminal` 动态发现并统一操控所有在线终端。

---

## 2. 总体架构与通信模型 (Architecture & Communication)

```text
+---------------------------------------------------------------------------------------------------------+
|                                              整体系统架构                                                |
+---------------------------------------------------------------------------------------------------------+

  [ AI Agent (Cursor / Claude / Antigravity) ]
                     │
                     │  MCP 协议 (stdio 或 HTTP/SSE 监听 127.0.0.1:9800)
                     ▼
  +-----------------------------------------------------------------------------------------------------+
  |                                        crates/server (中心服务端)                                     |
  |                                                                                                     |
  |  +---------------------------+    +--------------------------------+    +------------------------+  |
  |  |    MCP Gateway 适配层      |    |       Session / Router 管理     |    |   Terminal Hub 连接池   |  |
  |  |  - list_terminals         |--->|  - 当前会话选中终端 (Active ID)  |--->|  - 保存各在线机器 WS 通道 |  |
  |  |  - select_terminal        |    |  - 路由调度与超时控制             |    |  - 心跳超时判定/离线剔除 |  |
  |  |  - 9 大诊断工具转发入口    |    +--------------------------------+    +-----------▲------------+  |
  |  +---------------------------+                                                       │              |
  +--------------------------------------------------------------------------------------┼--------------+
                                                                                         │
                                         WebSocket 双向长连接 (ws://server-ip:9801/ws)    │
                                 ┌───────────────────────────────────────────────────────┴──────────┐
                                 │                                                                  │
                                 ▼                                                                  ▼
  +----------------------------------------------+   +----------------------------------------------+
  |         crates/agent (终端 A - Windows)       |   |         crates/agent (终端 B - Windows)       |
  |                                              |   |                                              |
  |  +----------------------------------------+  |   |  +----------------------------------------+  |
  |  |        WS Client + 心跳/重连管理        |  |   |  |        WS Client + 心跳/重连管理        |  |
  |  |  - 周期上报 IP/OS/CPU/RAM/负载/Hostname |  |   |  |  - 周期上报 IP/OS/CPU/RAM/负载/Hostname |  |
  |  +-------------------▲--------------------+  |   |  +-------------------▲--------------------+  |
  |                      │                       |   |                      │                       |
  |  +-------------------▼--------------------+  |   |  +-------------------▼--------------------+  |
  |  |            本地诊断与执行引擎           |  |   |  |            本地诊断与执行引擎           |  |
  |  |  - sysinfo / powershell / screen / ... |  |   |  |  - sysinfo / powershell / screen / ... |  |
  |  +----------------------------------------+  |   |  +----------------------------------------+  |
  |  |        egui 界面 / 实时审计 / 一键断开   |  |   |  |        egui 界面 / 实时审计 / 一键断开   |  |
  |  +----------------------------------------+  |   |  +----------------------------------------+  |
  +----------------------------------------------+   +----------------------------------------------+
```

---

## 3. 工程组织与 Cargo Workspace 结构

重构后的工程采用多 Crate 架构：

```text
at-pc/
├── Cargo.toml                  # Workspace 根配置
├── crates/
│   ├── protocol/               # 公共数据结构、WebSocket 消息帧、RPC 协议
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── messages.rs     # 客户端与服务端交互的消息枚举
│   │       └── models.rs       # 终端信息、心跳指标、工具入参及出参
│   ├── server/                 # 中心服务端 (二进制产物: at-pc-server)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       ├── config.rs       # 服务端配置 (端口、鉴权 Token)
│   │       ├── ws/             # WebSocket 接入与长连接管理
│   │       │   ├── mod.rs
│   │       │   ├── handler.rs
│   │       │   └── registry.rs # 动态在线终端池与心跳检测
│   │       ├── router.rs       # 工具调用转发与响应匹配
│   │       └── mcp/            # MCP 协议服务 (stdio / SSE)
│   │           ├── mod.rs
│   │           └── tools.rs    # MCP 工具声明与分发
│   └── agent/                  # 客户端代理 (二进制产物: at-pc-agent)
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           ├── config.rs       # 配置文件读取 (agent_config.toml 或构建注入)
│           ├── ws_client.rs    # WebSocket 客户端、自动重连与心跳上报
│           ├── executor.rs     # 本地 MCP 工具分发与执行
│           ├── app/            # egui 桌面客户端 UI
│           │   ├── mod.rs
│           │   └── ui.rs
│           ├── tools/          # 诊断工具实现 (sysinfo, powershell, screen 等)
│           └── utils/
```

---

## 4. 协议设计契约 (`crates/protocol`)

### 4.1 核心消息定义

```rust
use serde::{Deserialize, Serialize};

/// 终端基础元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalInfo {
    pub terminal_id: String,
    pub hostname: String,
    pub username: String,
    pub lan_ip: String,
    pub os_version: String,
    pub agent_version: String,
}

/// 心跳运行指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatMetrics {
    pub cpu_usage_percent: f32,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub uptime_secs: u64,
    pub timestamp: i64,
}

/// 代理端发往服务端的消息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum AgentToServerMessage {
    Register {
        info: TerminalInfo,
        auth_token: Option<String>,
    },
    Heartbeat {
        terminal_id: String,
        metrics: HeartbeatMetrics,
    },
    ToolResult {
        call_id: String,
        success: bool,
        result: serde_json::Value,
        error: Option<String>,
        duration_ms: u64,
    },
    Disconnect {
        terminal_id: String,
        reason: String,
    },
}

/// 服务端发往代理端的消息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum ServerToAgentMessage {
    RegisterAck {
        success: bool,
        message: Option<String>,
        heartbeat_interval_secs: u64,
    },
    HeartbeatAck {
        server_timestamp: i64,
    },
    InvokeTool {
        call_id: String,
        tool_name: String,
        arguments: serde_json::Value,
        timeout_secs: u64,
    },
    CancelTool {
        call_id: String,
    },
}
```

---

## 5. 服务端详细设计 (`crates/server`)

### 5.1 终端连接池管理 (`TerminalRegistry`)
* 维护 `active_connections: Arc<RwLock<HashMap<String, TerminalSession>>>`。
* `TerminalSession` 包含：
  * `info: TerminalInfo`
  * `latest_metrics: HeartbeatMetrics`
  * `last_heartbeat_at: std::time::Instant`
  * `ws_sender: tokio::sync::mpsc::UnboundedSender<ServerToAgentMessage>`
  * `status: TerminalStatus` (`Online` | `Busy` | `Offline`)
* **心跳超时检测**：每 5 秒扫描一次，若 `last_heartbeat_at.elapsed() > 15s` 则置为 `Offline`；若连接异常断开则清理并广播事件。

### 5.2 异步调用调度与超时
* 服务端维护 `pending_calls: Arc<DashMap<String, oneshot::Sender<ToolResult>>>`。
* 当 Agent 发起工具调用：
  1. 生成唯一 `call_id`。
  2. 创建 `oneshot::channel`，并将 `tx` 存入 `pending_calls`。
  3. 将 `InvokeTool` 投递给目标终端的 `ws_sender`。
  4. 使用 `tokio::time::timeout` 异步等待代理端返回或超时（默认 35 秒）。
  5. 代理端返回 `ToolResult` 时，通过 `call_id` 触发 `tx.send()` 唤醒并响应 Agent。

### 5.3 对外 MCP 工具集

1. **终端发现与选择类**：
   * `list_terminals`: 返回所有已注册终端及其详细状态（ID、主机名、IP、操作系统、CPU/内存指标、在线状态）。
   * `select_terminal(terminal_id)`: 设定当前会话活跃终端。
   * `get_active_terminal`: 获取当前选中的默认终端详情。
2. **转发诊断工具类**（均带可选 `terminal_id` 参数）：
   * `get_system_overview(terminal_id?)`
   * `exec_powershell(script, timeout_secs?, cwd?, terminal_id?)`
   * `exec_cmd(command, timeout_secs?, cwd?, terminal_id?)`
   * `list_processes(filter_name?, sort_by?, limit?, terminal_id?)`
   * `kill_process(pid?, process_name?, force?, terminal_id?)`
   * `manage_service(service_name, action, terminal_id?)`
   * `read_text_file(file_path, tail_lines?, max_bytes?, terminal_id?)`
   * `write_text_file(file_path, content, create_backup?, terminal_id?)`
   * `capture_screen(display_index?, format?, quality?, terminal_id?)`

---

## 6. 代理端设计与打包分发 (`crates/agent`)

### 6.1 预置配置与打包支持
1. **外置配置文件 (`agent_config.toml`)**：
   ```toml
   [server]
   url = "ws://192.168.1.100:9801/ws"
   auth_token = "at-pc-secret-2026"
   reconnect_interval_secs = 5

   [device]
   device_id = "auto"   # 为 auto 时自动组合 hostname 与网卡 MAC 保证唯一性
   device_name = ""     # 留空自动使用系统计算机名
   ```
2. **构建时注入支持**：
   * 通过 `build.rs` 支持环境变量（如 `DEFAULT_SERVER_URL`），在打包时直接将服务端地址内嵌至二进制文件。

### 6.2 运行生命周期
1. 启动时加载配置（优先同目录下 `agent_config.toml`，缺省使用内嵌默认值）。
2. 在后台异步建立 WebSocket 连接并发送 `Register` 握手。
3. 注册成功后启动心跳任务（每 5 秒发送 `Heartbeat`）。
4. 收到 `InvokeTool` 指令后，在本地异步线程池中执行对应工具，并将进度和操作记录推送到本地 egui 审计日志列表中。
5. 执行完成后组装 `ToolResult` 发送回服务端。
6. 用户在本地界面点击【断开协助】时，立刻发送 `Disconnect`，断开长连接并取消正在运行的任务。

---

## 7. 验收与验证计划 (Verification Plan)

### 7.1 单元测试与协议测试
* `crates/protocol`: 验证所有消息序列化与反序列化测试。
* `crates/server`: 验证 `TerminalRegistry` 注册、心跳更新、离线检测、call_id 匹配调度逻辑。
* `crates/agent`: 验证配置解析、默认设备指纹生成、执行引擎本地调用。

### 7.2 集成联调与端到端验证
1. 启动 `at-pc-server`，监听 WS 端口 `9801` 及 MCP 接口。
2. 启动 2 个 `at-pc-agent` 实例（模拟不同终端）。
3. 验证服务端实时记录 2 台终端在线状态与心跳。
4. 在 MCP 客户端调用 `list_terminals` 确认返回两台终端信息。
5. 调用 `select_terminal` 选中终端 A，执行 `get_system_overview` 和 `exec_powershell`，确认仅在终端 A 执行且返回预期输出。
6. 显式传 `terminal_id` 指定终端 B 执行 `capture_screen`，确认路由准确。
7. 模拟终端 A 异常退出，验证服务端检测到离线并在 `list_terminals` 中正确标记。
