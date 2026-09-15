# at-pc 项目全量优化任务计划与里程碑路线图

本文档基于对 `at-pc`（包含 `crates/protocol`、`crates/server`、`crates/agent`）的系统性审查，针对安全性、协议鲁棒性、系统资源占用、推流效率、工程架构及企业落地等多维度问题，制定全量优化项及对应版本的里程碑实施计划。

---

## 里程碑概览 (Milestone Overview)

```
┌──────────────────────────────────────────────────────────────────────────────────┐
│ Milestone 1: v0.3.1 紧急安全加固与生产底线防护 (P0 - Security & Crash Prevention) │
│ 目标：彻底阻断未授权任意代码执行漏洞，消除大文件读取 OOM 崩溃隐患，实现 Stdio 与断网快速响应。 │
└────────────────────────────────────────┬─────────────────────────────────────────┘
                                         ▼
┌──────────────────────────────────────────────────────────────────────────────────┐
│ Milestone 2: v0.4.0 协议标准重构与性能解耦 (P1 - Protocol & Architecture)        │
│ 目标：全面废弃自研 WS 编解码器迁移标准库，实现真正的任务取消、二进制推流、无头服务与前端解耦。│
└────────────────────────────────────────┬─────────────────────────────────────────┘
                                         ▼
┌──────────────────────────────────────────────────────────────────────────────────┐
│ Milestone 3: v1.0.0 企业级合规与 AI Computer-Use 进阶 (P2 - Enterprise & Vision) │
│ 目标：对 MCP 暴露键鼠控制赋能 AI Computer-Use，原生 Win32 API 替代冷启动，全链路 WSS/mTLS。  │
└──────────────────────────────────────────────────────────────────────────────────┘
```

---

## Milestone 1: v0.3.1 紧急安全加固与生产底线防护 (P0)

> **目标**：解决现网高危安全隐患，防止通过 Web 控制台无感渗透内网终端；解决大文件引发的代理端 OOM 崩溃；修复 Stdio 串行阻塞与掉线延迟挂起。  
> **周期预估**：3 ~ 4 天  
> **发布版本**：`v0.3.1`

### 任务分解与实施计划

#### [P0-1] Web 管理控制台与 REST API 统一鉴权中间件
- **问题现状**：[`crates/server/src/mcp/mod.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/mcp/mod.rs) 仅对 `/sse` 和 `/messages` 进行鉴权，[`dashboard.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/mcp/dashboard.rs) 的所有端点（尤其是 `POST /api/terminals/:id/invoke` 与桌面控制）完全对外裸奔，攻击者可在局域网无凭证执行特权命令。
- **改动位置**：[`crates/server/src/mcp/mod.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/mcp/mod.rs)、[`crates/server/src/mcp/dashboard.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/mcp/dashboard.rs)
- **具体实施**：
  1. 将鉴权逻辑抽取为 Axum 中间件 `auth_middleware`；
  2. 对 `/api/*` 以及非静态页面路由强制应用鉴权；
  3. 支持通过 `Authorization: Bearer <token>`、Cookie 或 URL Query 参数 `token` 进行鉴权；
  4. Web Dashboard 页面在初次载入时若未提供 Token，弹出密码验证模态窗，通过后缓存至 `sessionStorage`。
- **验收标准**：
  - 未带 Token 的 `POST /api/terminals/:id/invoke` 严格返回 HTTP 401 Unauthorized；
  - 静态页面与 Web 资源在鉴权通过前不展示敏感终端拓扑。

#### [P0-2] 严格 CORS 策略与服务器监听地址白名单
- **问题现状**：[`crates/server/src/mcp/mod.rs#L45`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/mcp/mod.rs#L45) 配置了 `CorsLayer::permissive()`，任何第三方网站脚本均可发起跨站攻击。同时服务默认强绑 `0.0.0.0`。
- **改动位置**：[`crates/server/src/config.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/config.rs)、[`crates/server/src/main.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/main.rs)
- **具体实施**：
  1. 在 `ServerConfig` 中引入 `listen_host: String`（默认支持配置 `127.0.0.1` 或 `0.0.0.0`）；
  2. 废除 `CorsLayer::permissive()`，仅允许同源或配置了 `allowed_origins` 的白名单来源访问 API；
  3. CLI 增加 `--host` 参数。
- **验收标准**：
  - 跨域发起的未经许可请求被浏览器拦截；
  - 本地运行 `--host 127.0.0.1` 时局域网外部探测端口直接拒绝连接。

#### [P0-3] `read_text_file` 逆向流式 Tail 读取（消除 OOM）
- **问题现状**：[`crates/agent/src/tools/file_ops.rs#L53`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/agent/src/tools/file_ops.rs#L53) 使用 `fs::read(path)` 一次性将全部文件载入内存后再取最后 200 行，遇超大日志直接导致 Agent 内存溢出崩溃。
- **改动位置**：[`crates/agent/src/tools/file_ops.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/agent/src/tools/file_ops.rs)
- **具体实施**：
  1. 使用 `std::fs::File` 配合 `std::io::Seek`；
  2. 当指定 `tail_lines` 时，从 `SeekFrom::End(0)` 开始以 64KB 块反向向前检索换行符 `\n`；
  3. 检索满所需行数后即停止读取，内存开销上限恒定在 `max_bytes`（默认 512KB）以内。
- **验收标准**：
  - 对 2GB 模拟超大日志文件执行 `read_text_file(tail_lines=200)`，Agent 内存增量不超过 2MB，并在 50ms 内瞬间返回尾部内容。

#### [P0-4] 终端断连即时失败 (Fail-fast)
- **问题现状**：终端异常崩溃或网线拔出时，服务端在途指令必须等待 35 秒超时才能响应 AI Agent。
- **改动位置**：[`crates/server/src/ws/handler.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/ws/handler.rs)、[`crates/server/src/router.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/router.rs)
- **具体实施**：
  1. 在 `McpRouter` 中增加 `abort_pending_calls_for_terminal(terminal_id: &str, reason: &str)`；
  2. 在 `ws/handler.rs` 捕获连接断开（或离线清理）时，遍历 `pending_calls`，将属于该终端的所有等待 channel 立即发送 `Err("Terminal disconnected unexpectedly")`。
- **验收标准**：
  - 工具执行中强制关闭 Agent 进程，服务端在 100ms 内向 MCP 客户端返回明确的断连错误，无需等待超时。

#### [P0-5] MCP Stdio 模式异步化处理
- **问题现状**：[`crates/server/src/mcp/mod.rs#L377-L402`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/mcp/mod.rs#L377-L402) 单线程同步等待每个请求处理完毕，慢速指令会导致整个 MCP 管道阻塞。
- **改动位置**：[`crates/server/src/mcp/mod.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/mcp/mod.rs)
- **具体实施**：
  1. Stdio 读取行循环只负责 JSON 解析与任务派发，每个请求调用通过 `tokio::spawn` 异步执行；
  2. 建立 `mpsc::channel<String>` 汇聚响应，由单一后台任务顺序向 `stdout` 输出并 flush。
- **验收标准**：
  - 在执行耗时 20 秒的 `exec_powershell` 期间，向 Stdio 发送 `tools/list` 或 `ping` 请求能立刻得到响应。

#### [P0-6] 清理配置硬编码内网开发机 IP
- **改动位置**：[`crates/agent/src/config.rs#L31`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/agent/src/config.rs#L31)
- **具体实施**：将回退的编译宏默认 IP 从 `192.168.66.195` 修正为安全回环地址 `ws://127.0.0.1:9801/ws`。

---

## Milestone 2: v0.4.0 协议标准重构与性能解耦 (P1)

> **目标**：重构底层传输层，消除自研 WebSocket 编解码器的维护负担与分片缺陷；实现多粒度任务取消；重构推流链路降低 CPU 与带宽消耗；实现 Agent 无头服务化与前端工程解耦。  
> **周期预估**：5 ~ 7 天  
> **发布版本**：`v0.4.0`

### 任务分解与实施计划

#### [P1-1] 迁移业界标准 WebSocket 协议栈
- **问题现状**：[`agent/src/codec.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/agent/src/codec.rs) 与 [`server/src/ws/codec.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/ws/codec.rs) 存在 400 余行高度重复且不完善的简易编解码代码，不支持分片帧合并，易受畸变包影响。
- **改动位置**：`Cargo.toml`、[`crates/server/src/ws/`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/ws)、[`crates/agent/src/ws_client.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/agent/src/ws_client.rs)
- **具体实施**：
  1. 服务端接入 `axum::extract::ws`（或 `tokio-tungstenite`）；
  2. Agent 端采用 `tokio-tungstenite`；
  3. 彻底删除自研 `codec.rs`，获得自动分片合并、掩码校验、RFC 6455 规范 Ping/Pong 链路保活能力。
- **验收标准**：
  - 传输大于 16MB 的大截屏或文件时，自动分片拼包无丢包、无解析错误；
  - 自动响应 Ping 帧，通过主流反向代理（Nginx、Traefik）长连接不断流。

#### [P1-2] 建立细粒度任务取消（CancelTool）机制
- **问题现状**：Agent 端的 `cancel(&call_id)` 为空桩，且 `spawn_blocking` 超时后未杀子进程或中断任务。
- **改动位置**：[`crates/agent/src/executor.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/agent/src/executor.rs)、[`crates/agent/src/ws_client.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/agent/src/ws_client.rs)
- **具体实施**：
  1. `AgentExecutor` 维护 `active_calls: Arc<Mutex<HashMap<String, CallHandle>>>`；
  2. `CallHandle` 记录对应的子进程 PID（若为命令行）或 `tokio::task::AbortHandle`；
  3. 收到 `CancelTool { call_id }` 或自身超时时，主动触发对应进程树终止与任务 Abort。
- **验收标准**：
  - 发起耗时 60 秒的死循环脚本后调用取消，子进程在 200ms 内被杀掉，系统负载立即回落。

#### [P1-3] 桌面推流二进制优化与脏矩形重采样
- **问题现状**：当前为全屏 JPEG 压缩后 Base64 编码，嵌入 JSON 传输，产生 33% 冗余体积与较高 CPU 消耗。
- **改动位置**：[`crates/protocol/src/messages.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/protocol/src/messages.rs)、[`crates/agent/src/stream.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/agent/src/stream.rs)、[`crates/server/src/mcp/dashboard.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/mcp/dashboard.rs)
- **具体实施**：
  1. 在 WebSocket 上发送二进制帧：`[4B Header Type][4B Display][4B Width][4B Height][8B Timestamp][Raw JPEG/WebP bytes]`；
  2. 浏览器端使用 `Blob` + `createImageBitmap` 或 `OffscreenCanvas` 进行硬件加速绘制；
  3. 引入简单的首帧对比机制，画面完全静态时不重复编码发送，节省带宽。
- **验收标准**：
  - 推流 CPU 消耗降低 30% 以上，网络吞吐减少约 35%，画面帧率在弱网下更平稳。

#### [P1-4] Agent 无头模式（Headless）与系统服务运行支持
- **问题现状**：Agent 强依赖 egui 窗口，无法作为 Windows 服务或无人值守服务器守护进程后台运行。
- **改动位置**：[`crates/agent/src/main.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/agent/src/main.rs)、[`crates/agent/src/lib.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/agent/src/lib.rs)
- **具体实施**：
  1. 剥离通信核心服务与 GUI 层的生命周期；
  2. 增加启动参数 `--headless`：仅启动后台 WS 客户端与本地执行引擎，不初始化 eframe/Glow 窗口；
  3. 针对 Windows 平台提供 `--service install/uninstall/run` 支持，注册为 Windows Service。
- **验收标准**：
  - 在无显示器连接或未登录桌面的 Windows Server / PC 上能正常开机自启并连接中心服务端。

#### [P1-5] Web 控制台前后端解耦（构建期嵌入）
- **问题现状**：[`crates/server/src/mcp/dashboard.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/server/src/mcp/dashboard.rs) 将 2000 行 HTML/JS 代码作为内嵌字符串写入 `.rs` 文件，维护极其困难。
- **改动位置**：新建 `crates/server/frontend/`，修改 `crates/server/build.rs`
- **具体实施**：
  1. 前端拆分为标准的前端项目（HTML/CSS/JS 或轻量前端工具链）；
  2. 使用 `rust-embed` crate 在编译阶段将前端静态产物打包进二进制产物；
  3. 开发期支持从本地目录动态载入前端文件以实现热重载，发布期保持单一独立二进制。
- **验收标准**：
  - 维持服务端“单可执行文件零外部依赖”的分发优势，同时使前端代码支持模块化开发与语法高亮检查。

#### [P1-6] 进程与系统状态采样缓存优化
- **问题现状**：每次调用 `list_processes` 重建全部 `System` 结构体，Windows 下耗时 300~500ms。
- **改动位置**：[`crates/agent/src/tools/process.rs`](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/crates/agent/src/tools/process.rs)
- **具体实施**：
  1. 维护长驻进程的 `System` 静态/共享单例；
  2. 仅调用 `sys.refresh_processes()`，避免扫描全量硬件；
  3. 两次采样间隔保证 CPU 使用率计算准确。
- **验收标准**：
  - `list_processes` 调用耗时从平均 450ms 降低至 50ms 以内，且返回真实的 CPU 百分比数据。

---

## Milestone 3: v1.0.0 企业级合规与 AI Computer-Use 进阶 (P2)

> **目标**：对 MCP 暴露键鼠控制工具赋能多模态 AI Agent；使用 Win32 原生 API 彻底消灭 PowerShell 启动延迟；全链路 TLS/WSS 与企业级 RBAC 审计。  
> **周期预估**：7 ~ 10 天  
> **发布版本**：`v1.0.0`

### 任务分解与实施计划

#### [P2-1] 暴露 MCP Computer-Use 键鼠自动化工具
- **实施内容**：
  1. 在 `crates/server/src/mcp/tools.rs` 中增加受控工具声明：
     - `mouse_click(x, y, button, count, terminal_id?)`
     - `mouse_move(x, y, terminal_id?)`
     - `type_text(text, terminal_id?)`
     - `press_key(key, terminal_id?)`
  2. 配合 `capture_screen`，赋予 Claude 3.5 / Computer-Use Agent 完整的视觉-行动自动化排障能力；
  3. 增加安全开关：默认关闭键鼠操作，需在配置中显式开启 `enable_computer_use = true`。

#### [P2-2] 原生 Win32 API 替代 PowerShell 进程
- **实施内容**：
  1. 服务控制：通过 `windows-sys` 的 SCM（`OpenSCManagerW`, `OpenServiceW`, `ControlService`）直接启停和查询服务；
  2. 事件日志：通过 `EvtQuery` / `EvtNext` 直接读取 Windows 事件日志；
  3. 彻底告别调用 PowerShell 的进程创建开销，将系统工具平均响应时间压制在 15ms 以内。

#### [P2-3] 全链路 WSS / HTTPS 加密与双向证书认证 (mTLS)
- **实施内容**：
  1. 在服务端集成 `rustls`，支持配置 SSL/TLS 证书和私钥；
  2. Agent 与 Server 支持通过 `wss://` 建立长连接，所有屏幕帧、指令、敏感文件流实现传输加密；
  3. 支持 mTLS（客户端证书认证），彻底杜绝非法终端接入。

#### [P2-4] 权限分离与细粒度 RBAC 审计
- **实施内容**：
  1. 区分两套 Token：`agent_token`（终端接入凭证）与 `operator_token`（AI/运维操作凭证）；
  2. 提供“只读诊断模式”（Read-Only Mode）：禁止执行写文件、杀进程、服务启停或命令执行；
  3. 审计日志持久化保存并支持轮转导出，满足企业等保合规要求。

---

## 验证与验收矩阵 (Verification Matrix)

| 阶段 | 验证项 | 验证手段 | 预期目标 |
| :--- | :--- | :--- | :--- |
| **M1** | API 鉴权拦截 | 编写自动化测试，调用无 Token 的 REST/Dashboard API | 严格返回 HTTP 401，阻断非授权访问 |
| **M1** | 大文件逆向 Tail | 构造 2GB 模拟日志文件测试 `read_text_file` | 读取时间 < 50ms，内存峰值增量 < 2MB |
| **M1** | 断线快速失败 | 运行中 kill 掉 Agent 进程 | 服务端 100ms 内向 MCP 客户端返回明确错误 |
| **M1** | Stdio 异步响应 | 并发发送慢命令与 ping 指令 | ping 请求无延迟即时返回 |
| **M2** | WebSocket 标准化 | 注入分片帧与 Ping/Pong 测试 | 无掉线、拼包正确、兼容反向代理 |
| **M2** | 任务强制取消 | 下发死循环脚本后触发 CancelTool | 200ms 内终止子进程并回收资源 |
| **M2** | 无头与服务模式 | Windows Server 下以服务自启并反向连入 | 无图形上下文下所有工具执行正常 |
| **M3** | Computer-Use 工具 | 通过 MCP 客户端调用 `mouse_click` 与 `type_text` | 远端准确响应并执行输入，界面产生相应操作 |
| **M3** | Win32 原生调用 | 压测 service 与 event_log 工具 | 单次查询延迟从 >1000ms 降至 <20ms |

---

## 实施建议

建议优先立即启动 **Milestone 1 (v0.3.1)**，修补 REST API 鉴权空缺、大文件 OOM 以及 Stdio 串行阻塞等影响生产可用性与系统安全的 P0 缺陷；待版本稳定后，按计划平滑切入 **Milestone 2** 的底层标准协议重构。
