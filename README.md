<div align="center">

<img src="media/at-pc-icon-256.png" width="128" height="128" alt="AT-PC Logo" />

# AT-PC (Antigravity PC)

**高性能远程 PC 运维 Agent · C/S 分布式网关 · Computer Use & 原生 MCP 自动化引擎**

[![Release](https://img.shields.io/github/v/release/xwamt/at-pc?color=blue&logo=github)](https://github.com/xwamt/at-pc/releases)
[![Rust](https://img.shields.io/badge/rust-1.88%2B%20%7C%201.96.0%20pinned-orange?logo=rust)](rust-toolchain.toml)
[![Protocol](https://img.shields.io/badge/mcp-JSON--RPC%202.0%20%7C%20SSE%20%7C%20Stdio-green)](crates/protocol)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey)]()
[![License](https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue.svg)](#-开源协议)

[特性总览](#-核心特性) • [系统架构](#-系统架构) • [快速上手](#-快速上手指南) • [MCP 工具矩阵](#-mcp-工具矩阵39-项) • [配置说明](#-配置指南) • [性能指标](#-性能工程与硬门禁) • [门禁与首启密钥](#-门禁与首启密钥) • [安全机制](#-企业级安全体系) • [制品下载](#-发布制品清单)

</div>

---

## 📖 项目简介

**AT-PC** 是 AT (Antigravity Terminal) 系列专为现代 AI Agent（如 Cursor IDE、Claude Desktop、AT-OpsAgent）与企业级远程桌面运维打造的高性能终端与控制网关。

整个系统基于 **纯 Rust** 研发，深度结合了 **C/S 分布式解耦架构**、**原生 Model Context Protocol (MCP)** 以及 **Computer Use / 视觉增强技术**。通过毫秒级的双向安全信令通道，AI 能够安全、精准、高效地执行多物理屏幕感知、UI 语义树遍历、键鼠模拟操作、进程治理与自动化诊断排障。

---

## ✨ 核心特性

- 🌐 **现代化 C/S 分布式架构**：
  - 支持远程受控终端通过双向 WebSocket 接入；TLS/WSS 与可选 mTLS 由部署配置决定；
  - 动态心跳探活（5s 间隔）、异常离线感知与平滑注销；客户端按 `reconnect_interval_secs` 固定间隔自动重连，修改地址或手动重连可立即唤醒一次尝试；
  - 内置单页 Web 管理控制台，支持多屏推流预览、终端重命名与状态监控。
- 🤖 **原生 MCP (Model Context Protocol) 支持**：
  - 原生提供符合 MCP 规范的标准服务接口，支持用于本地 IDE 的 `stdio` 模式，以及用于分布式集群的 `HTTP / SSE` 模式；
  - 向上层大模型暴露 39+ 项标准化诊断与操作工具，覆盖排障诊断与具身交互生命周期；
  - 支持会话绑定、指定设备定向执行和多终端选择提示。
- 🖥️ **Computer Use 与视觉增强**：
  - **多物理显示器支持**：枚举多物理屏，支持指定屏幕截图与推流，避免多屏坐标偏移；
  - **Set-of-Marks (SoM) 标记增强**：对截屏元素执行语义网格化标记，辅助视觉模型定位；
  - **Windows UI Automation (UIA) 语义树**：提取窗口控件树与层次信息，支持按语义定位输入框、按钮与菜单项；
  - **macOS Cocoa 原生窗口审查**：通过原生 API 获取活动窗口与前台应用元数据；
  - **高精度输入模拟**：支持移动、拖拽、点击、组合快捷键和文本输入；具身操作必须在受控端显式开启。
- ⚡ **性能工程与门禁**：
  - **64-bit 分块 hash 与静态帧跳过**：静态桌面跳过 JPEG 编码，仅在 keepalive 到期时发送轻量帧；
  - **自适应输出缩放**：默认流输出宽度上限为 1280，减少高分辨率桌面的编码和传输成本；
  - **进程列表 TTL 缓存**：高频读取可在 1000 ms TTL 内复用缓存；
  - **性能合同**：CI 维护 1080p hash ≤ 1 ms、2560 宽 hash ≤ 2 ms、2560 SoM ≤ 8 ms、5k 元数据更新平均 ≤ 50 µs、5k 列表 ≤ 15 ms 的明确阈值；
  - **持续验证**：`./scripts/check_perf_thresholds.sh` 编译探针并执行 release 性能断言。
- 🛡️ **安全准入与可审计性**：
  - **具身操作安全阀门**：受控端默认锁定模拟输入能力，需显式声明开启；
  - **服务端追加审计**：服务端将关键操作追加写入 JSONL；Agent GUI 显示最多 500 条、可清空的会话内活动记录；
  - **本地紧急断开**：受控端 GUI/托盘可停止 Agent 会话、桌面流和已追踪子进程；用户可显式重新连接。

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
# 安全默认：仅监听本机，MCP 端口 9800，WebSocket 端口 9801。
./at-pc-server-macos --host 127.0.0.1 --ws-port 9801 --mcp-port 9800

# 访问内嵌 Web 管理控制台。
open http://localhost:9800/
```

> 对外部署必须配置 TLS/WSS 与操作端凭据。`--host 0.0.0.0 --allow-plaintext` 仅用于隔离测试网络，会暴露未加密流量，不能作为生产启动方式。

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

# 具身操作安全准入开关位于根层。默认 false 禁用鼠标键盘注入；
# 可通过启动参数 --enable-computer-use 或环境变量 AT_PC_ENABLE_COMPUTER_USE=1 开启。
enable_computer_use = false

[server]
# 中心服务端 WebSocket 连接地址 (局域网或公网 IP)
url = "ws://127.0.0.1:9801/ws"

# [LEGACY] 设备/共享 Token 仅用于旧版本兼容，当前 WebSocket 注册不会在服务端强制校验。
# 请勿把此配置当作设备身份或授权边界；后续 Pending Enrollment + mTLS 将替代它。
# device_token = "atpc_dv_<id>_<secret>"
# auth_token = "<legacy-shared-token>"

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
```

### 服务端配置 (`server_config.example.toml`)

```toml
# at-pc-server 服务端配置示例 (v1.0.0)

# 默认仅监听本机；对外部署请显式配置 TLS、凭据与受控监听地址
listen_host = "127.0.0.1"

# 非回环监听且未配置 TLS 时必须保持 false；仅隔离测试可显式改为 true
allow_plaintext = false

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

### 当前 CI 性能合同

下列阈值来自 `thresholds.toml`，由 `./scripts/check_perf_thresholds.sh` 与对应 release 测试共同验证；它们是上界合同，不是所有硬件上的固定实测值。

| 性能场景 | 当前阈值 |
| :--- | :--- |
| 1080p 屏幕分块 hash | ≤ 1.00 ms |
| 2560 宽屏幕分块 hash | ≤ 2.00 ms |
| 2560 宽 SoM 检测 | ≤ 8.00 ms |
| 5,000 终端元数据更新平均值 | ≤ 50 µs |
| 5,000 终端列表读取 | ≤ 15 ms |

性能报告必须同时记录 commit、硬件、操作系统、样本数与原始结果；有关 4K 流、JPEG 体积、进程缓存和真实集群吞吐的优化将以独立基准与实机复检报告为准。

---

## 🔑 门禁与首启密钥

服务端自 v1.x 起内置 **安全双门禁**：
1. **API Key 凭据门禁**：非回环监听（如 `--host 0.0.0.0`）且无凭据时，服务**拒绝启动**。`/health` 对回环来源免凭据，本地开发无影响。
2. **TLS 传输加密门禁**：非回环监听在未配置 TLS 证书时，必须显式携带 `--allow-plaintext` 命令行参数（或配置文件指定 `allow_plaintext = true`），否则出于传输安全考虑**拒绝启动**。生产环境强烈建议配置 TLS 证书。

### ① 首启自动生成 root key

首次启动（数据目录内无 `bootstrap_key.txt` 且无 `keys.json`）时，server 自动生成 root API Key，格式为 `atpc_sk_<8字符ID>_<43字符secret>`，并通过**两个通道**提供给运维：

- **启动日志（stdout / `docker logs at-pc-server`）**：首启时直接打印，前缀为 `atpc_sk_`。
- **文件落盘**：写入 `<data_dir>/bootstrap_key.txt`，权限 `0600`。

建议**抄录 key 后即删除该文件**，避免长期明文留存（删除后服务端重启不会重新生成孤儿 root key，已签发凭据持久化保存在 `keys.json` 中）：

```bash
# 查看（仅 root 可读）
sudo cat /var/lib/at-pc/bootstrap_key.txt
# 抄录后删除
sudo rm /var/lib/at-pc/bootstrap_key.txt
```

### ② 数据目录持久化语义（Docker）

容器启动参数由 `docker-compose.yml` 自动探测 `/etc/at-pc/server_config.toml`（即 compose 挂载的文件）。

- 具名卷 `at-pc-data`（挂载到 `/var/lib/at-pc`）**保留** → 不会重新 bootstrap，沿用既有 key；
- **删除该卷**（`docker compose down -v` 或手动删 `at-pc-data`）→ 旧 key 全部失效，下次启动重新首启生成新 root key。

### ③ 本地开发如何免门禁

- **完全免门禁（开放态）**：`--host 127.0.0.1 --no-bootstrap`（回环监听 + 无凭据 = 开放态）。
- **公网部署无凭据**：直接拒绝启动，需提供首启生成的 key。
- **显式豁免（不建议）**：`--insecure-no-auth` 可强制关闭门禁，仅限隔离测试环境。

### ④ 操作端 API Key 与当前设备接入状态

- **操作端凭据 (`atpc_sk_...`)**：供用户通过 Web 控制台或 MCP 协议访问 `:9800`，具有 Admin / Operator / Viewer 角色权限；不得把它配置为受控端身份。
- **当前设备接入的限制**：v1.0.0 的 WebSocket 注册尚未在服务端验证 `device_token`、绑定 `terminal_id` 或拒绝操作端凭据。该行为已被记录为临时风险基线，不能作为安全边界使用。
- **后续迁移方向**：新版本将采用“待批准设备注册 + mTLS 设备证书”。用户只需启动 Agent，管理员批准后自动上线，无需复制设备 Token；详情见 `docs/plans/2026-09-29-comprehensive-optimization-delivery-plan.md`。

---

## 🔒 企业级安全体系

1. **具身操作显式确认 (Explicit Consent)**：
   - 客户端模拟鼠标/键盘能力受到双重锁定（配置文件 `enable_computer_use = true` 或启动参数 `--enable-computer-use`）。未授权模式下任何注入请求均会被就地拦截并告警。
2. **服务端审计 (Audit Trails)**：
   - 命令执行、文件改动、系统服务切换等服务端操作以追加 JSONL 记录，内容包含发起会话、客户端 IP、操作指令与执行结果。
   - 高低频事件按现有服务端策略过滤；审计文件具备权限收紧、轮转与导出能力，但不宣称不可变存储。
3. **本地紧急断开 (Kill Switch)**：
   - 本地用户可通过 Agent GUI 或系统托盘停止当前 Agent 会话、桌面流和已追踪子进程；重新连接必须由本地用户显式触发。

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
