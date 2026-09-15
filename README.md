# AT-PC

> 高性能远程 PC 运维 Agent、C/S 架构网关与 Computer Use / MCP 自动化引擎。

AT-PC 是 AT (Antigravity Terminal) 系列的远程终端与桌面控制组件，基于纯 Rust 构建。支持高并发 WebSocket 控制信令、实时多显示器画面捕获与推流、Windows UI Automation (UIA)、Set-of-Marks (SOM) 视觉定位、系统进程/服务治理，以及原生 MCP (Model Context Protocol) 接口暴露。

---

## 架构拓扑

```text
at-pc/
├── crates/
│   ├── protocol/      # 共享通讯协议 (JSON-RPC 2.0、C/S WebSocket 消息帧、数据模型)
│   ├── agent/         # 客户端 Agent (egui 桌面 UI、UIA、SOM、截图流、命令执行、进程审计)
│   └── server/        # 服务端网关 (Axum Web 控制台、MCP 工具路由、终端注册表、多屏幕流汇聚)
├── media/             # 项目图标与视觉资源
└── scripts/           # 打包与自动化脚本
```

### 核心模块职责

| Crate | 职责 |
| :--- | :--- |
| `at-pc-protocol` | 定义客户端与服务端之间的通讯帧协议，提供强类型消息编解码、错误定义与序列化支持。 |
| `at-pc-agent` | 运行于受控端机器的守护程序，具备系统级操作能力（屏幕捕获、鼠标键盘模拟、进程/服务管理、UIA 语义树提取与 SOM 标记）。 |
| `at-pc-server` | 集中管理与路由节点，内置 MCP Server、嵌入式 Web 控制面板以及多终端会话分发。 |

---

## 核心特性

- **C/S 架构与双向安全通道**：基于 WebSocket (TLS) 的轻量级高频信令通讯，配备动态 PIN 码认证与实时状态探活机制。
- **多显示器管理与流畅推流**：支持多物理显示器检测、切换与区域截图推拉，专为大屏与多屏办公/运维场景设计。
- **Computer Use 与视觉辅助**：
  - **Set-of-Marks (SOM)**：对截屏内容自动执行标记定位，极大提升视觉模型（VLM）点击决策精度；
  - **Windows UIA**：实时获取当前窗口层次与控件语义树，结构化快速定位目标输入框与按钮。
- **全生命周期审计与安全开关**：内置单终端审计日志、命令执行黑白名单以及一键紧急断开 (Emergency Kill Switch)。
- **MCP 原生集成**：作为 MCP Server 向上层 AI Agent 暴露标准工具集，实现零摩擦的自动化远程操作。

---

## 构建与测试

### 环境要求

- Rust 1.78+ (Edition 2021)
- 对应平台的 C++ 构建工具链（Windows / macOS / Linux）

### 常用命令

```bash
# 检查与编译整个工作区
cargo check --workspace
cargo build --workspace

# 执行全部测试用例
cargo test --workspace

# 启动 Server 服务端
cargo run -p at-pc-server

# 启动 Agent 客户端
cargo run -p at-pc-agent
```

---

## 开源协议

本项目属于 AT 系列开源项目，采用 MIT / Apache-2.0 协议分发。
