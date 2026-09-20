# at-pc 系统托盘常驻与生命周期管理架构设计规范 (System Tray Resident & Lifecycle Design)

**文档编号**：`SPEC-2026-09-20-TRAY-RESIDENT`  
**创建日期**：2026-09-20  
**状态**：Approved (已评审)  
**适用范围**：`at-pc-server`、`at-pc-agent`

---

## 1. 背景与问题定义 (Problem Statement)

`at-pc` 作为轻量跨平台桌面诊断与远程控制工具套件，包含两端：
* **`at-pc-server`**：中央网关、MCP 协议路由与 Web 控制台服务。
* **`at-pc-agent`**：部署在受控/协助终端的原生端侧代理。

### 现状痛点
1. **Server 端无状态栏常驻与退出通道**：
   * 当前 Server 端为纯控制台进程（`#[tokio::main]`），在日常桌面环境双击运行或后台启动后，Windows 任务栏通知区（右下角）和 macOS 状态栏（右上角）没有任何图标常驻。
   * 用户无法直观确认 Server 是否处于运行状态、监听端口是多少；当需要终止服务时，普通用户甚至找不到退出入口，导致后台进程残留（例如通过 CLI 启动后终端关闭仍在后台运行）。
2. **Agent 端关闭即硬杀，无法常驻协助**：
   * 当前 Agent 端主窗口点击右上角“X”关闭按钮时，直接调用 `std::process::exit(0)`，导致正在进行的远程连接或后台就绪状态意外中断。
   * 缺少“关闭窗口最小化到托盘”的工具软件通用行为，缺乏托盘菜单进行重连、切断或复制终端 ID 的快捷操作。

### 目标与预期成果
* 为 **`at-pc-server`** 和 **`at-pc-agent`** 增加跨平台（Windows、macOS）的系统托盘常驻图标与原生操作菜单。
* Server 托盘支持：运行状态展示、一键打开嵌入式 Web 控制台、定位运行日志、优雅保存并退出。
* Agent 托盘支持：窗口关闭拦截到托盘（Close-to-Tray）、左键唤醒窗口、连接状态实时动态更新、复制终端 ID、切断协助/重连、彻底退出。
* 保持 CLI/无头调用兼容性：提供 `--no-tray` 标志，在 `--stdio` 或无图形会话环境下自动/显式禁用托盘。

---

## 2. 交互与菜单功能规范 (User Experience & Menu Spec)

### 2.1 Server 端托盘交互
* **托盘图标**：使用 `media/at-pc-icon.png` 生成高分屏适配图标。
* **菜单项结构**：
  1. `[信息]` `at-pc-server v1.0.0 (运行中)` —— 只读置灰，带版本与运行态标识。
  2. `[信息]` `Web 端口: 9800 · WS 端口: 9801` —— 只读置灰，展示实际监听端口。
  3. `── 分割线 ──`
  4. `[操作]` `🌐 打开 Web 控制台` —— 在系统默认浏览器中唤起 `http://127.0.0.1:{mcp_port}`。
  5. `[操作]` `📁 查看运行日志` —— 在文件管理器中打开/定位 `at-pc-server.log`。
  6. `── 分割线 ──`
  7. `[操作]` `🛑 退出服务器` —— 广播优雅退出信号，等待持久化落盘后终止进程。
* **命令行与运行模式控制**：
  * 新增命令行参数 `--no-tray`：显式禁用托盘，以纯终端/控制台模式运行。
  * 若指定了 `--stdio`（供 Cursor/IDE 调用），默认**禁用托盘**。
  * 若运行环境缺少 GUI 显示支持（如 Linux 无 DISPLAY/WAYLAND、Windows Session 0 服务模式），自动禁用托盘。

### 2.2 Agent 端托盘交互
* **窗口关闭拦截 (Close-to-Tray)**：
  * 点击窗口右上角“X”关闭按钮时，拦截关闭事件，将主窗口隐藏（`Visible(false)`），进程与后台 WebSocket 保持运行。
* **托盘点击唤醒**：
  * 左键点击托盘图标，或在托盘菜单中点击 `🖥️ 显示主界面`，主窗口立即重新展示（`Visible(true)`）并获取焦点（`Focus`）。
* **菜单项结构**：
  1. `[信息]` `at-pc 终端代理 v1.0.0` —— 只读置灰。
  2. `[信息]` `终端 ID: {terminal_id_short}` —— 只读置灰，展示本机终端 ID 前缀。
  3. `[状态]` `连接状态: 🟢 已连接 / 🟡 连接中 / 🔴 离线` —— 跟随实际底层连接状态实时更新。
  4. `── 分割线 ──`
  5. `[操作]` `🖥️ 显示主界面` —— 显示并聚焦 Agent 桌面窗口。
  6. `[操作]` `📋 复制终端 ID` —— 将当前完整的 `terminal_id` 写入系统剪贴板。
  7. `[操作]` `🔴 切断协助 / 🔄 立即重连` —— 根据当前状态动态切换动作，与 UI 上的控制逻辑保持一致。
  8. `── 分割线 ──`
  9. `[操作]` `🛑 退出 Agent` —— 彻底断开 WebSocket、记录退出审计日志并安全退出进程。
* **Headless 模式说明**：
  * 当以 `--headless` 启动但运行于桌面环境时，默认展示托盘以方便查看状态和随时退出；若同时指定 `--no-tray`，则完全纯后台运行。

---

## 3. 技术架构与事件循环模型 (Architecture & Event Loop)

### 3.1 跨平台底层库选型
选用 Tauri 官方维护的工业级跨平台组件：
* `tray-icon = "0.19"`：跨平台原生托盘实现（macOS 使用 `NSStatusBar` / `NSStatusItem`，Windows 使用 `Shell_NotifyIconW`）。
* `muda = "0.15"`：跨平台原生菜单实现（支持 macOS 原生 `NSMenu`、Windows 原生 Win32 菜单与暗黑主题匹配）。
* `winit = "0.29"`：事件泵驱动库（与 `eframe 0.27` 底层保持一致）。

### 3.2 平台事件循环与 Tokio 协同架构

由于 macOS 强制要求所有 Cocoa UI 与状态栏操作必须在**主线程 (Main Thread)** 运行，架构分别对 Agent 与 Server 进行针对性设计：

#### A. Agent 端事件循环协同
Agent 端本就依赖 `eframe::run_native` 在主线程驱动 `winit` 循环：
1. 在 `AgentApp` 初始化时创建 `tray_icon::TrayIcon` 与 `muda::Menu`。
2. 托盘事件通过 `muda::MenuEvent::receiver()` 和 `tray_icon::TrayIconEvent::receiver()` 派发。
3. 在 `AgentApp::update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame)` 每一帧：
   * 轮询通道事件，若收到“显示主界面”或左键点击托盘，调用：
     ```rust
     ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
     ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
     ```
   * 拦截窗口关闭事件：
     ```rust
     if ctx.input(|i| i.viewport().close_requested()) {
         ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
         ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
     }
     ```
   * 若收到“退出 Agent”菜单事件，主动切断 WebSocket，保存状态并调用：
     ```rust
     ctx.send_viewport_cmd(egui::ViewportCommand::Close);
     ```
   * 根据 `state.status()` 动态调用 `muda::MenuItem::set_text` 更新托盘中的连接状态文案。

#### B. Server 端事件循环协同
Server 端原本为纯异步命令行服务，改造后采用“主线程托盘泵 + 后台 Tokio 多线程运行时”的双层模型：
1. **模式判定**：在 `main()` 启动阶段根据 CLI 参数（`--stdio` / `--no-tray`）与环境决定是否启用托盘。
2. **启用托盘模式**：
   * 创建一个退出通知通道 `(shutdown_tx, shutdown_rx)`（基于 `tokio::sync::broadcast`）。
   * 启动后台 Tokio 运行时（`tokio::runtime::Builder::new_multi_thread().enable_all().build()`），在异步运行时中启动现有的 WebSocket 网关、MCP HTTP/SSE 路由、定时清理任务。
   * **主线程** 构建 `tray_icon::TrayIcon` 与 `muda::Menu`，并运行原生平台事件泵（`winit::event_loop::EventLoop`）。
   * 菜单事件处理：
     * 点击【打开 Web 控制台】：异步执行系统浏览器唤起命令。
     * 点击【查看日志】：异步在系统文件管理器中定位日志文件。
     * 点击【退出服务器】：向 Tokio 异步运行时广播停机通知，主线程退出事件泵。
   * 后台异步运行时收到停机信号后，执行 `abort_and_join_tasks` 以及 `shutdown_persistence`，将元数据和审计日志落盘，最后结束进程。
3. **禁用托盘模式**：
   * 维持原有逻辑不变，直接在当前线程使用 Tokio 异步运行时驱动，接收控制台 `Ctrl+C` / `SIGINT` / `SIGTERM` 信号。

---

## 4. 跨平台辅助指令与资源处理 (Platform Operations)

### 4.1 唤起默认浏览器与文件管理器（零额外庞大依赖）
* **打开 Web 控制台**：
  * macOS: `std::process::Command::new("open").arg(&url).spawn()`
  * Windows: `std::process::Command::new("cmd").args(["/c", "start", "", &url]).spawn()`
  * Linux: `std::process::Command::new("xdg-open").arg(&url).spawn()`
* **定位日志文件**：
  * macOS: `std::process::Command::new("open").args(["-R", &log_path]).spawn()`
  * Windows: `std::process::Command::new("explorer").args(["/select,", &log_path]).spawn()`
  * Linux: `std::process::Command::new("xdg-open").arg(&log_dir).spawn()`

### 4.2 托盘图标加载
* 复用 `media/at-pc-icon.png`：
  * 通过 `image::load_from_memory` 读取内置图标。
  * 将图片转换为 RGBA 像素流，构建 `tray_icon::Icon::from_rgba(rgba, width, height)`。
  * macOS 自动适配 Retina 高分屏（生成清晰的 32x32 / 44x44 图标），Windows 自动适配 100%~250% DPI 显示。

---

## 5. 错误处理与边缘场景 (Edge Cases & Fault Tolerance)

| 场景 | 行为定义 |
|---|---|
| **无桌面/无头环境（如 SSH、Docker、CI）启动 Server** | 检测到平台无法初始化托盘事件循环时，自动降级为 `--no-tray` 纯控制台模式，记录 Warning 日志，绝不导致服务启动崩溃。 |
| **Cursor / IDE 通过 `--stdio` 启动 Server** | 强制屏蔽托盘初始化，确保 stdio 传输纯净无干扰，且不在用户系统状态栏留下无用图标。 |
| **用户在隐藏到托盘状态下多次启动 Agent** | 维持单实例或者新实例提醒；从托盘随时可以通过左键或菜单唤醒已隐藏的主窗口。 |
| **退出过程中持久化写入耗时** | 托盘点击退出后立即将托盘菜单项置灰或显示“正在退出...”，异步运行时完成持久化刷盘后立即退出进程，超时保护 3 秒兜底强制终止。 |

---

## 6. 测试与验证计划 (Verification Plan)

### 6.1 自动化测试
* **CLI 参数与决策测试**：
  * 编写单元测试验证 `parse_tray_mode(args, env)` 函数在 `--stdio`、`--no-tray`、默认桌面模式下的判定准确性。
* **优雅停机信号测试**：
  * 测试托盘退出通道触发后，Tokio 服务端正确收到信号并依次 join WebSocket、MCP Server 和元数据落盘。
* **Agent 状态格式化测试**：
  * 验证不同 `ClientConnectionStatus`（Connecting, Connected, Disconnected）生成的托盘文本格式正确。

### 6.2 本机手动实测验证
* **Server 端验证**：
  1. 运行 `cargo run -p at-pc-server`，检查 macOS 顶部菜单栏是否出现状态栏图标。
  2. 点击图标弹出菜单，检查信息项、端口项是否准确。
  3. 点击【打开 Web 控制台】，验证系统默认浏览器是否成功打开 `http://127.0.0.1:9800` 并展现 Dashboard。
  4. 点击【查看运行日志】，验证 Finder/文件管理器是否正确定位到日志。
  5. 点击【退出服务器】，验证进程退出、端口释放、元数据文件正确落盘。
  6. 运行 `cargo run -p at-pc-server -- --no-tray`，验证无状态栏图标且支持控制台 `Ctrl+C` 退出。
* **Agent 端验证**：
  1. 运行 `cargo run -p at-pc-agent`，主界面正常显示，菜单栏出现 Agent 图标。
  2. 点击主窗口右上角“X”关闭按钮，验证主窗口隐藏消失，但进程未退出且顶部托盘图标仍在。
  3. 点击托盘图标或菜单【显示主界面】，验证主窗口重新恢复并置顶。
  4. 点击托盘【复制终端 ID】，在任意输入框粘贴验证 ID 正确。
  5. 点击【退出 Agent】，验证 WebSocket 断开，进程彻底退出。
