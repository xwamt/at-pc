<div align="center">

<img src="media/at-pc-icon-256.png" width="128" height="128" alt="AT-PC Logo" />

# AT-PC (Antigravity PC)

**高性能远程 PC 运维 Agent · C/S 分布式网关 · Computer Use & 原生 MCP 自动化引擎**

[![Release](https://img.shields.io/github/v/release/xwamt/at-pc?color=blue&logo=github)](https://github.com/xwamt/at-pc/releases)
[![Rust](https://img.shields.io/badge/rust-1.88%2B%20%7C%201.96.0%20pinned-orange?logo=rust)](rust-toolchain.toml)
[![Protocol](https://img.shields.io/badge/mcp-JSON--RPC%202.0%20%7C%20SSE%20%7C%20Stdio-green)](crates/protocol)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey)]()
[![License](https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue.svg)](#-开源协议)

[特性总览](#-核心特性) • [系统架构](#-系统架构) • [快速上手](#-快速上手指南) • [MCP 工具矩阵](#-mcp-工具矩阵39-项) • [配置说明](#-配置指南) • [性能指标](#-性能工程与硬门禁) • [安全机制](#-企业级安全体系) • [制品下载](#-发布制品清单)

</div>

---

## 📖 项目简介

**AT-PC** 是 AT (Antigravity Terminal) 系列专为现代 AI Agent（如 Cursor IDE、Claude Desktop、AT-OpsAgent）与企业级远程桌面运维打造的高性能终端与控制网关。

整个系统基于 **纯 Rust** 研发，深度结合了 **C/S 分布式解耦架构**、**原生 Model Context Protocol (MCP)** 以及 **Computer Use / 视觉增强技术**。通过毫秒级的双向安全信令通道，AI 能够安全、精准、高效地执行多物理屏幕感知、UI 语义树遍历、键鼠模拟操作、进程治理与自动化诊断排障。

---

## ✨ 核心特性

- 🌐 **现代化 C/S 分布式架构**：
  - 支持成百上千台远程受控终端通过双向 WebSocket (TLS/mTLS) 安全接入；
  - 动态心跳探活（5s 间隔）、异常离线感知与平滑注销、客户端断线指数退避重连；
  - 内置高性能单页 Web 管理控制台，支持多屏推流预览、终端重命名与状态监控。
- 🤖 **原生 MCP (Model Context Protocol) 支持**：
  - 原生提供符合 MCP 规范的标准服务接口，支持用于本地 IDE 的 `stdio` 模式，以及用于分布式集群的 `HTTP / SSE` 模式；
  - 向上层大模型暴露 **39+ 项标准化诊断与操作工具**，覆盖排障诊断与具身交互全生命周期；
  - 智能多终端路由调度：支持基于上下文的会话绑定、指定设备定向执行（`target_terminal`）与自动故障转移。
- 🖥️ **全能力 Computer Use 与视觉增强**：
  - **多物理显示器支持**：原生枚举多物理屏，支持指定屏幕截图与推流，规避多屏拼接场景下的坐标偏移；
  - **Set-of-Marks (SoM) 标记增强**：对截屏元素执行自动语义网格化标记，成倍提升视觉多模态大模型（VLM）点击定位精度；
  - **Windows UI Automation (UIA) 语义树**：秒级提取窗口控件树与层次信息，直接按语义定位输入框、按钮与菜单项；
  - **macOS Cocoa 原生窗口审查**：微秒级（0.26ms）提取当前活动窗口与前台应用元数据；
  - **高精度输入模拟**：平滑移动、拖拽、点击、组合快捷键输入，配合智能输入队列保障无丢字。
- ⚡ **极致性能工程**：
  - **64-bit 词宽哈希变化检测**：在 1080p 分辨率下仅需 **0.48 ms** 完成脏块检测，静态桌面几乎零 CPU 占用；
  - **自适应降采样**：默认降采样算法使截屏网络传输体积骤降 **70%+**；
  - **进程列表 TTL 缓存**：引入 1000ms 自适应热缓存，高频连续查询响应提升 **13x ~ 15x**（70ms → 5ms）；
  - **5000+ 终端扩展**：注册中心基于 `Arc/COW` 并发设计，万级查询仍保持 O(1) 延迟；
  - **硬性 CI 性能门禁**：内置性能基准测试与硬性指标断言，严防性能劣化。
- 🛡️ **企业级安全准入与可审计**：
  - **具身操作安全阀门**：受控端默认锁定模拟输入能力，需显式声明开启，防止越权；
  - **不可篡改审计流水**：客户端与服务端双重落盘记录审计日志（JSONL 格式），智能豁免高频无害移动，确保排障可追溯；
  - **一键紧急制动 (Emergency Kill Switch)**：受控端 GUI 提供硬件级紧急断开按钮，一键阻断所有远端控制通道。

---

## 🏗️ 系统架构

AT-PC 采用高内聚、低耦合的 Workspace 多 Crate 架构：

```text
at-pc/
├── crates/
│   ├── protocol/          # 核心共享通讯协议 (JSON-RPC 2.0、C/S WebSocket 消息帧、数据模型)
│   ├── server/            # 中心网关服务 (Axum HTTP/SSE、MCP 工具路由、终端注册中心)
│   │   └── frontend/      # 内嵌 Web 管理控制台 (现代单页应用，零外部静态依赖)
│   ├── agent/             # 受控端守护程序 (egui 桌面 UI、UIA、SOM、输入注入、进程审计)
│   ├── desktop-core/      # 跨平台桌面底层引擎 (词宽哈希、截屏降采样、Cocoa/UIA 交互)
│   ├── build-support/     # 跨平台构建支持 (Windows 资源嵌入、静态资源打包)
│   ├── benchmarks/        # 性能基准测试套件与 CI 性能门禁探针
│   └── e2e-tests/         # C/S 链路全流程自动化集成测试
├── media/                 # 视觉资源与项目 Logo
├── scripts/               # 跨平台全自动打包流水线与质量门禁脚本
└── dist/                  # 全平台预编译制品输出目录
```

### 核心模块职责矩阵

| 模块 / Crate | 职责与技术栈 |
| :--- | :--- |
| **`at-pc-protocol`** | 统一强类型协议层。定义 WebSocket 双向心跳/信令消息帧、JSON-RPC 2.0 规范、MCP 工具入参与出参模型，提供高效的序列化与反序列化支持。 |
| **`at-pc-server`** | 核心网关与调度枢纽。基于 Axum + Tokio 构建，承担多 Agent 的 WebSocket 会话汇聚、动态终端注册表、MCP 请求路由转发、RBAC 角色鉴权及内嵌 Web 管理面板。 |
| **`at-pc-agent`** | 运行于受控机器的高权限 Agent。支持原生桌面窗口（基于 egui）与无头静默服务模式；负责执行底层系统调用、屏幕捕获、键鼠模拟注入与本地安全审计。 |
| **`at-pc-desktop-core`**| 跨平台桌面高性能抽象。封装 xcap、Windows UI Automation (UIA)、macOS Cocoa 窗口层级、64-bit 词宽屏幕脏块哈希算法及自适应 JPEG/PNG 编码。 |
| **`at-pc-benchmarks`**  | 性能质量基准。包含 1080p/2K/4K 脏块哈希检测、SoM 标记、注册表并发压测，并在 CI 中强制执行 `thresholds.toml` 硬性阈值校验。 |

---

## 🚀 快速上手指南

### 方式一：直接下载使用预编译制品（推荐）

从 [GitHub Releases](https://github.com/xwamt/at-pc/releases) 获取最新版制品：

- **macOS (Apple Silicon M 系列)**：下载并解压 `at-pc-macos-arm64.zip`
- **macOS (Intel x86_64)**：下载并解压 `at-pc-macos-x86_64.zip`
- **Windows (x86_64)**：下载并解压 `at-pc-windows-x86_64.zip`

解压后即可获得服务端与客户端二进制程序及预置配置文件。

---

### 方式二：从源码构建

#### 环境要求
- **Rust 1.96.0**（由 `rust-toolchain.toml` 锁定，最低支持 1.88+）；
- **Node.js 20+**（构建嵌入式 Web 管理控制台资源时需要）；
- 平台原生编译工具链：Windows 需 MSVC / Windows SDK；macOS 需 Xcode Command Line Tools；Linux 需 Clang、pkg-config、PipeWire、X11/Wayland 开发包。

```bash
# 克隆代码仓库
git clone https://github.com/xwamt/at-pc.git
cd at-pc

# 编译整个工作区（包含内嵌 Web 静态资源）
cargo build --workspace --release --locked

# 一键执行全平台打包流水线（生成完整 dist 目录）
python3 scripts/package_dist.py --all
```

---

### 🏃‍♂️ 运行与部署

#### 1. 启动中心网关服务端 (`at-pc-server`)

```bash
# 启动服务端 (默认监听本地，MCP 端口 9800，WebSocket 端口 9801)
./at-pc-server-macos --host 0.0.0.0 --ws-port 9801 --mcp-port 9800

# 访问内嵌 Web 管理控制台
open http://localhost:9800/
```

#### 2. 启动受控端守护程序 (`at-pc-agent`)

```bash
# 桌面交互 GUI 模式 (开启 Computer Use 具身操作能力)
./at-pc-agent-macos --server ws://<服务端IP>:9801/ws --enable-computer-use

# Windows 后台静默服务模式
at-pc-agent.exe --server ws://<服务端IP>:9801/ws --headless --enable-computer-use
```

#### 3. 接入 AI 客户端 (Cursor IDE / Claude Desktop)

通过在 AI 客户端中配置 `at-pc-server`，即可直接让 AI 具备控制远程 PC 的能力：

**Cursor IDE 配置 (`~/.cursor/mcp.json`)：**

```json
{
  "mcpServers": {
    "at-pc": {
      "command": "/path/to/at-pc-server-macos",
      "args": ["--stdio"]
    }
  }
}
```

**Claude Desktop 配置 (`claude_desktop_config.json`)：**

```json
{
  "mcpServers": {
    "at-pc": {
      "command": "/path/to/at-pc-server-macos",
      "args": ["--stdio"]
    }
  }
}
```

**远程 HTTP / SSE 网关接入：**

```json
{
  "mcpServers": {
    "at-pc-remote": {
      "url": "http://<Server-IP>:9800/sse"
    }
  }
}
```

---

## 🛠️ MCP 工具矩阵（39+ 项）

AT-PC 向上层智能体提供了工业级全方位的工具箱，所有工具均支持会话隔离与多终端定向调度：

| 领域分类 | 工具名称 (MCP Tool) | 功能说明 | 权限/安全要求 |
| :--- | :--- | :--- | :--- |
| **屏幕与视觉** | `capture_screen` | 捕获物理屏幕截屏，支持指定显示器与自适应降采样（默认 1280 缩放） | 读只权限 |
| | `list_monitors` | 枚举当前设备所有物理显示器（分辨率、主副屏标识、坐标原点） | 读只权限 |
| | `get_marked_screen`| 执行 Set-of-Marks (SoM) 视觉标记，返回打标后的图像及坐标映射表 | 读只权限 |
| **具身操作模拟** | `mouse_click` | 在指定屏幕坐标执行鼠标左键/右键/中键单击、双击 | 具身安全准入 (`enable_computer_use`) |
| | `mouse_move` | 平滑移动鼠标光标至目标绝对坐标（服务端智能合并高频抖动） | 具身安全准入 |
| | `mouse_drag` | 按下鼠标按键并拖拽移动至目标坐标 | 具身安全准入 |
| | `mouse_scroll` | 在当前光标位置触发垂直或水平滚轮滚动 | 具身安全准入 |
| | `type_text` | 注入模拟键盘文本输入（支持中文及 Unicode 字符流） | 具身安全准入 |
| | `key_press` | 触发单个按键（Enter、Escape、Tab、退格等）点击 | 具身安全准入 |
| | `key_sequence` | 触发复合快捷键序列（如 Ctrl+C、Command+Space、Alt+Tab） | 具身安全准入 |
| **窗口与 UI 树** | `get_ui_tree` | 基于 Windows UI Automation (UIA) 提取当前活动窗口控件语义层次树 | 读只权限 |
| | `list_windows` | 列出系统中所有顶层可视窗口及其进程所属关系 | 读只权限 |
| | `focus_window` | 激活并置顶指定的应用窗口 | 运维权限 |
| | `get_window_info`| 获取目标窗口的边界矩形、DPI 缩放与层级状态 | 读只权限 |
| | `review_windows` | (macOS) 微秒级快速审查当前所有前台可见窗口元数据 | 读只权限 |
| **系统与进程** | `list_processes` | 列出系统进程列表（内置 1000ms TTL 缓存，支持按 CPU/内存排序） | 读只权限 |
| | `kill_process` | 根据 PID 或进程名称安全终止异常进程 | 运维管理权限 |
| | `get_system_overview`| 获取 CPU 占用率、内存余量、主板/系统版本、网络 IP 清单与开机时间 | 读只权限 |
| | `list_services` | (Windows) 查询系统 Windows 服务运行状态与启动类型 | 读只权限 |
| | `control_service`| (Windows) 启动、暂停、重启或停止指定 Windows 服务的运行 | 运维管理权限 |
| | `get_event_logs` | 读取操作系统近期关键事件与错误日志 | 读只权限 |
| **命令与排障** | `execute_command`| 执行受控 Shell 命令，具备静默无黑框运行与严格超时熔断机制 | 管理员权限 / 审计记录 |
| | `read_text_file` | 安全读取受控端文本文件，支持尾部按行裁剪（tail）与 UTF-8 截断保护 | 读只权限 |
| | `write_text_file`| 写入或更新受控端文件内容 | 运维管理权限 |
| | `list_directory` | 浏览受控端指定目录树及文件属性 | 读只权限 |
| | `get_file_info` | 查询文件大小、修改时间、哈希校验及读写权限 | 读只权限 |
| **网关与多终端** | `list_terminals` | 列出当前网关所有在线/离线的受控终端清单 | 基础权限 |
| | `select_terminal`| 为当前会话显式绑定默认受控终端设备 | 基础权限 |
| | `rename_terminal`| 修改指定终端的自定义名称与备注标签，持久化到 MetaStore | 基础权限 |
| | `get_active_terminal`| 查看当前会话所绑定的活动终端元数据 | 基础权限 |

---

## ⚙️ 配置指南

### 受控端配置 (`agent_config.toml`)

受控端启动时会自动在当前运行目录或系统标准位置读取 `agent_config.toml`：

```toml
# at-pc-agent 终端配置文件 (v1.0.0)

[server]
# 中心服务端 WebSocket 连接地址 (局域网或公网 IP)
url = "ws://127.0.0.1:9801/ws"

# 可选：连接鉴权 Token (若服务端开启了认证则需一致)
# auth_token = "admin-secret-token"

# 断线自动重连间隔 (秒)
reconnect_interval_secs = 5

# TLS / WSS 安全连接配置 (可选)
# insecure_skip_verify = false   # 若使用自签名证书测试可设为 true
# ca_cert_path = "server.crt"    # 自定义 CA 证书路径

[device]
# 终端唯一标识：默认为 "auto" (按主机名 + MAC 自动生成唯一 ID)
device_id = "auto"

# 终端友好显示名称 (留空则自动读取计算机名)
device_name = "开发部-PC-01"

# 具身操作安全准入开关 (默认 false 禁用鼠标键盘注入，设为 true 开启)
enable_computer_use = true
```

### 服务端配置 (`server_config.example.toml`)

```toml
# at-pc-server 服务端配置示例 (v1.0.0)

# 监听 IP 地址：设为 "0.0.0.0" 允许外部网络访问
listen_host = "0.0.0.0"

# WebSocket 终端连接端口与路径
ws_port = 9801
ws_path = "/ws"

# MCP HTTP/SSE 网关及 Web 控制台监听端口
mcp_port = 9800

# 心跳与离线超时剔除配置 (秒)
heartbeat_interval_secs = 5
offline_threshold_secs = 15
sweep_interval_secs = 5

# 持久化文件存储路径
meta_store_path = "terminals_meta.json"
audit_log_path = "audit.jsonl"

# 基于角色的访问控制 (RBAC) Token 映射
[roles]
# "viewer-token" = "viewer"
# "operator-token" = "operator"
# "admin-token" = "admin"
```

---

## 📊 性能工程与硬门禁

AT-PC 在工程落地中推行极致的性能标准，并设立了由 CI 硬性强制执行的性能门禁套件：

```bash
# 执行全套性能基准探针与硬性指标断言
./scripts/check_perf_thresholds.sh
```

### 关键性能指标基准

| 性能场景 | 优化前状态 | AT-PC v1.0.0 实测 | 收益与加速比 |
| :--- | :--- | :--- | :--- |
| **1080p 屏幕脏块哈希检测** | 逐字节 1.25 ms | **0.48 ms**（64-bit 词宽向量比对） | 🚀 **2.6x 提速**，极低 CPU 占用 |
| **屏幕传输单帧网络体积** | 原生 2560 档 ~360 KB | **106 KB**（默认降采样）/ **72 KB** | 📉 **Payload 锐减 70%+** |
| **高频进程列表检索响应** | 无缓存 70.4 ms | **5.4 ms**（1000ms TTL 热命中） | ⚡ **13.0x ~ 15.2x 提速** |
| **macOS 活动窗口元数据提取**| AppleScript ~150 ms | **0.26 ms**（原生 Cocoa API） | 🏎️ **微秒级响应** |
| **5000+ 终端元数据更新时延**| O(n) 锁竞争 45 ms | **< 15 µs**（`Arc/COW` 无锁快照） | 📈 **支持企业级超大规模集群** |

---

## 🔒 企业级安全体系

1. **具身操作显式确认 (Explicit Consent)**：
   - 客户端模拟鼠标/键盘能力受到双重锁定（配置文件 `enable_computer_use = true` 或启动参数 `--enable-computer-use`）。未授权模式下任何注入请求均会被就地拦截并告警。
2. **全生命周期不可篡改审计 (Audit Trails)**：
   - 每一次命令执行、文件改动、系统服务切换均以不可篡改追加写模式记录到 `audit.jsonl`，记录内容包含发起会话、客户端 IP、操作指令与执行结果。
   - 对高频无害动作（如单纯移动鼠标）实施智能过滤，避免审计日志爆炸。
3. **紧急断开保护 (Kill Switch)**：
   - 无论处于何种受控状态，本地用户均可通过点击受控端 GUI 的“紧急断开”按钮或系统托盘，瞬间强制切断 WebSocket 信道并重置当前安全 Token。

---

## 📦 发布制品清单

最新预编译全系列二进制包可直接在 [GitHub Releases (v1.0.0)](https://github.com/xwamt/at-pc/releases/tag/v1.0.0) 中获取。每次发布均附带 `SHA256SUMS` 校验和文件供验证完整性：

| 制品包名称 | 适用操作系统 | 架构体系 | 包含核心内容 |
| :--- | :--- | :--- | :--- |
| **`at-pc-macos-arm64.zip`** | macOS 11.0+ | Apple Silicon (`arm64`) | `at-pc-server-macos`, `at-pc-agent-macos`, 配置文件模板, 文档 |
| **`at-pc-macos-x86_64.zip`** | macOS 10.15+ | Intel (`x86_64`) | `at-pc-server-macos`, `at-pc-agent-macos`, 配置文件模板, 文档 |
| **`at-pc-windows-x86_64.zip`** | Windows 10/11 / Server | 64-bit (`x86_64`) | `at-pc-server.exe`, `at-pc-agent.exe`, 配置文件模板, 文档 |

---

## 📄 开源协议

本项目基于 **MIT** 或 **Apache-2.0** 双协议开源。详细信息请参阅许可证文件。
