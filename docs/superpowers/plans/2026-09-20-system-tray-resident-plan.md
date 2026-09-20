# at-pc 系统托盘常驻与生命周期管理实现计划 (System Tray Resident Implementation Plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 `at-pc-server` 和 `at-pc-agent` 增加 Windows 任务栏通知区与 macOS 顶部菜单栏的状态栏常驻图标、操作菜单、窗口关闭隐藏到托盘以及优雅停机功能。

**Architecture:** 采用 Tauri 官方 `tray-icon` + `muda` 实现跨平台原生托盘与菜单。Agent 共享 `eframe` 主线程事件泵，拦截窗口关闭事件实现“隐藏到托盘”；Server 采用“主线程原生事件循环 + 后台 Tokio 异步多线程运行时”架构，通过跨线程通道联动持久化刷盘与优雅停机，同时通过 `--no-tray` 和 `--stdio` 保持对纯控制台/IDE 调用的无缝兼容。

**Tech Stack:** Rust 2021, `tray-icon 0.19`, `muda 0.15`, `winit 0.29`, `eframe 0.27`, `tokio 1.38`, `image 0.25`.

**Spec:** `docs/superpowers/specs/2026-09-20-system-tray-resident-design.md`

## Global Constraints

- **Rust Version**: Rust 2021 edition (compatible with `rustc 1.96.0`).
- **Dependencies Floor**: `tray-icon = "0.19"`, `muda = "0.15"`, `winit = "0.29"`.
- **Platform Parity**: macOS (MenuBar `NSStatusItem`) and Windows (Notification Area `Shell_NotifyIcon`).
- **Mode Isolation**: `--stdio` or `--no-tray` must never create or pop up tray icons.
- **Data Integrity**: Exiting from tray MUST trigger `abort_and_join_tasks` and `shutdown_persistence` cleanly before process exit.

---

### Task 1: Workspace 依赖与配置扩展 (Dependencies & Build Setup)

**Files:**
- Modify: `Cargo.toml:10-41`
- Modify: `crates/server/Cargo.toml:13-34`
- Modify: `crates/agent/Cargo.toml:17-41`

**Interfaces:**
- Consumes: Workspace root dependencies table
- Produces: `tray-icon`, `muda`, `winit` available in `server` and `agent` crates

- [ ] **Step 1: 在根 `Cargo.toml` 中添加 workspace 依赖**

在根目录 `Cargo.toml` 的 `[workspace.dependencies]` 添加：
```toml
tray-icon = "0.19"
muda = "0.15"
winit = "0.29"
```

- [ ] **Step 2: 在 `crates/server/Cargo.toml` 和 `crates/agent/Cargo.toml` 中引用依赖**

在 `crates/server/Cargo.toml` 的 `[dependencies]` 添加：
```toml
tray-icon = { workspace = true }
muda = { workspace = true }
winit = { workspace = true }
image = { workspace = true }
```

在 `crates/agent/Cargo.toml` 的 `[dependencies]` 添加：
```toml
tray-icon = { workspace = true }
muda = { workspace = true }
```

- [ ] **Step 3: 运行 `cargo check` 确保依赖解析与下载无冲突**

Run: `cargo check -p at-pc-server -p at-pc-agent`  
Expected: Finished dev compilation checks with exit code 0.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock crates/server/Cargo.toml crates/agent/Cargo.toml
git commit -m "build: add tray-icon, muda and winit dependencies"
```

---

### Task 2: 托盘跨平台辅助函数与图标加载模块 (Shared Tray Utilities)

**Files:**
- Create: `crates/server/src/tray.rs`
- Create: `crates/agent/src/app/tray.rs`
- Modify: `crates/server/src/lib.rs:1-20`

**Interfaces:**
- Consumes: `media/at-pc-icon.png`
- Produces: `load_tray_icon() -> Result<tray_icon::Icon, Box<dyn std::error::Error + Send + Sync>>`, `open_browser_url(url: &str)`, `locate_file_in_explorer(path: &std::path::Path)`

- [ ] **Step 1: 编写测试用例验证系统命令构建与图标加载**

在 `crates/server/src/tray.rs` 中编写单元测试：
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_tray_icon_succeeds() {
        let icon_res = load_tray_icon();
        assert!(icon_res.is_ok(), "Tray icon must load cleanly from media/at-pc-icon.png");
    }

    #[test]
    fn test_browser_command_non_empty() {
        let cmd = build_browser_command("http://127.0.0.1:9800");
        assert_eq!(cmd.get_program().to_string_lossy().is_empty(), false);
    }
}
```

- [ ] **Step 2: 运行测试并验证失败（文件尚不存在）**

Run: `cargo test -p at-pc-server --lib tray::tests`  
Expected: FAIL (module `tray` does not exist).

- [ ] **Step 3: 实现 `crates/server/src/tray.rs`**

```rust
//! System tray utilities and platform helpers for at-pc-server.

use std::path::Path;
use std::process::Command;
use tracing::{error, info};

pub fn load_tray_icon() -> Result<tray_icon::Icon, Box<dyn std::error::Error + Send + Sync>> {
    let png_bytes = include_bytes!("../../../media/at-pc-icon.png");
    let img = image::load_from_memory(png_bytes)?.into_rgba8();
    let (width, height) = img.dimensions();
    let rgba = img.into_raw();
    let icon = tray_icon::Icon::from_rgba(rgba, width, height)?;
    Ok(icon)
}

pub fn build_browser_command(url: &str) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg(url);
        cmd
    }
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/c", "start", "", url]);
        cmd
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(url);
        cmd
    }
}

pub fn open_browser_url(url: &str) {
    info!("Opening browser URL: {}", url);
    if let Err(e) = build_browser_command(url).spawn() {
        error!("Failed to open browser URL {}: {}", url, e);
    }
}

pub fn locate_file_in_explorer(path: &Path) {
    info!("Locating path in file manager: {}", path.display());
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").args(["-R", &path.to_string_lossy()]).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("explorer").args(["/select,", &path.to_string_lossy()]).spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let dir = path.parent().unwrap_or(path);
        let _ = Command::new("xdg-open").arg(dir).spawn();
    }
}
```

并在 `crates/server/src/lib.rs` 中声明 `pub mod tray;`。

- [ ] **Step 4: 运行测试验证通过**

Run: `cargo test -p at-pc-server --lib tray::tests`  
Expected: PASS (2 tests pass).

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/tray.rs crates/server/src/lib.rs
git commit -m "feat(server): add tray icon loader and platform launch utilities"
```

---

### Task 3: Server 端托盘模式解析与事件循环整合 (Server Tray & Event Loop)

**Files:**
- Modify: `crates/server/src/tray.rs`
- Modify: `crates/server/src/main.rs:15-227`

**Interfaces:**
- Consumes: `ServerConfig`, `tray_icon::TrayIcon`, `muda::Menu`, `tokio::sync::broadcast`
- Produces: `run_server_with_tray(config: ServerConfig, meta_file: PathBuf, audit_path: Option<PathBuf>) -> Result<()>`

- [ ] **Step 1: 编写命令行参数判定测试**

在 `crates/server/src/tray.rs` 中增加测试：
```rust
#[test]
fn test_should_enable_tray_logic() {
    // --stdio disables tray
    assert_eq!(should_enable_tray(&["at-pc-server".into(), "--stdio".into()]), false);
    // --no-tray disables tray
    assert_eq!(should_enable_tray(&["at-pc-server".into(), "--no-tray".into()]), false);
    // default enables tray
    assert_eq!(should_enable_tray(&["at-pc-server".into()]), true);
}
```

- [ ] **Step 2: 实现 `should_enable_tray` 函数**

```rust
pub fn should_enable_tray(args: &[String]) -> bool {
    if args.iter().any(|a| a == "--stdio" || a == "--no-tray") {
        return false;
    }
    // Check if running in headless environment (e.g. Linux without display)
    #[cfg(target_os = "linux")]
    {
        if std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err() {
            return false;
        }
    }
    true
}
```

- [ ] **Step 3: 运行测试验证通过**

Run: `cargo test -p at-pc-server --lib tray::tests::test_should_enable_tray_logic`  
Expected: PASS.

- [ ] **Step 4: 构建 Server 托盘菜单与事件循环机制**

在 `crates/server/src/tray.rs` 中实现 `ServerTray`:
```rust
use muda::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

pub struct ServerTray {
    pub _tray: TrayIcon,
    pub open_web_id: muda::MenuId,
    pub open_logs_id: muda::MenuId,
    pub quit_id: muda::MenuId,
}

impl ServerTray {
    pub fn new(web_port: u16, ws_port: u16) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let menu = Menu::new();
        let title_item = MenuItem::with_id(
            "title",
            format!("at-pc-server v{} (运行中)", env!("CARGO_PKG_VERSION")),
            false,
            None,
        );
        let port_item = MenuItem::with_id(
            "ports",
            format!("Web 端口: {} · WS 端口: {}", web_port, ws_port),
            false,
            None,
        );

        let open_web = MenuItem::new("🌐 打开 Web 控制台", true, None);
        let open_logs = MenuItem::new("📁 查看运行日志", true, None);
        let quit_item = MenuItem::new("🛑 退出服务器", true, None);

        let open_web_id = open_web.id().clone();
        let open_logs_id = open_logs.id().clone();
        let quit_id = quit_item.id().clone();

        menu.append(&title_item)?;
        menu.append(&port_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&open_web)?;
        menu.append(&open_logs)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit_item)?;

        let icon = load_tray_icon()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(format!("at-pc-server (Port: {})", web_port))
            .with_icon(icon)
            .build()?;

        Ok(Self {
            _tray: tray,
            open_web_id,
            open_logs_id,
            quit_id,
        })
    }
}
```

- [ ] **Step 5: 修改 `crates/server/src/main.rs` 区分托盘运行模式与纯控制台模式**

修改 `crates/server/src/main.rs`：
1. 识别 `--no-tray` 参数并更新 `--help` 说明。
2. 若 `should_enable_tray(&args)` 为 `false`：沿用现有的 `tokio::select!` 与 `shutdown_signal()` 运行。
3. 若为 `true`：
   * 启动后台 Tokio 运行时，启动 WebSocket、MCP HTTP、持久化落盘和 sweep 任务。
   * 主线程初始化 `ServerTray` 和 `winit::event_loop::EventLoopBuilder::new().build()?`。
   * 在主线程运行 `event_loop.run(...)`。使用 `muda::MenuEvent::receiver()` 监听点击：
     * `open_web_id` -> 调用 `open_browser_url(&format!("http://127.0.0.1:{}", config.mcp_port))`
     * `open_logs_id` -> 调用 `locate_file_in_explorer(&log_file_path)`
     * `quit_id` -> 发送停机广播，等待持久化完成，调用 `target.exit()`。

- [ ] **Step 6: 编译并验证 `at-pc-server`**

Run: `cargo check -p at-pc-server`  
Expected: Compiles with 0 errors.

- [ ] **Step 7: Commit**

```bash
git add crates/server/src/tray.rs crates/server/src/main.rs
git commit -m "feat(server): integrate system tray with graceful shutdown and console fallback"
```

---

### Task 4: Agent 端托盘组件与窗口拦截 (Agent Close-to-Tray & Tray Menu)

**Files:**
- Create: `crates/agent/src/app/tray.rs`
- Modify: `crates/agent/src/app/mod.rs:1-40`
- Modify: `crates/agent/src/app/ui.rs:600-657`
- Modify: `crates/agent/src/main.rs:105-175`

**Interfaces:**
- Consumes: `AgentAppState`, `ClientConnectionStatus`, `tray_icon`, `muda`
- Produces: `AgentTrayManager`, close-to-tray viewport handling

- [ ] **Step 1: 编写 Agent 托盘状态文案格式化测试**

在 `crates/agent/src/app/tray.rs` 中编写测试：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ws_client::ClientConnectionStatus;

    #[test]
    fn test_format_connection_status_text() {
        assert_eq!(format_status_menu_text(&ClientConnectionStatus::Connected), "连接状态: 🟢 已连接");
        assert_eq!(format_status_menu_text(&ClientConnectionStatus::Connecting), "连接状态: 🟡 连接中");
        assert_eq!(format_status_menu_text(&ClientConnectionStatus::Disconnected), "连接状态: 🔴 离线");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p at-pc-agent --lib app::tray::tests`  
Expected: FAIL (module not created yet).

- [ ] **Step 3: 实现 `crates/agent/src/app/tray.rs`**

```rust
//! System tray manager and event polling for at-pc-agent.

use crate::ws_client::ClientConnectionStatus;
use muda::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

pub fn format_status_menu_text(status: &ClientConnectionStatus) -> String {
    match status {
        ClientConnectionStatus::Connected => "连接状态: 🟢 已连接".to_string(),
        ClientConnectionStatus::Connecting => "连接状态: 🟡 连接中".to_string(),
        ClientConnectionStatus::Disconnected => "连接状态: 🔴 离线".to_string(),
    }
}

pub fn load_tray_icon() -> Result<tray_icon::Icon, Box<dyn std::error::Error + Send + Sync>> {
    let png_bytes = include_bytes!("../../../../media/at-pc-icon.png");
    let img = image::load_from_memory(png_bytes)?.into_rgba8();
    let (width, height) = img.dimensions();
    let rgba = img.into_raw();
    let icon = tray_icon::Icon::from_rgba(rgba, width, height)?;
    Ok(icon)
}

pub struct AgentTray {
    pub _tray: TrayIcon,
    pub status_item: MenuItem,
    pub show_window_id: muda::MenuId,
    pub copy_id_id: muda::MenuId,
    pub toggle_conn_id: muda::MenuId,
    pub quit_id: muda::MenuId,
}

impl AgentTray {
    pub fn new(terminal_id: &str, initial_status: ClientConnectionStatus) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let menu = Menu::new();
        let title_item = MenuItem::with_id(
            "title",
            format!("at-pc 终端代理 v{}", env!("CARGO_PKG_VERSION")),
            false,
            None,
        );
        let id_display = if terminal_id.len() > 16 {
            format!("{}...", &terminal_id[..16])
        } else {
            terminal_id.to_string()
        };
        let id_item = MenuItem::with_id("term_id", format!("终端 ID: {}", id_display), false, None);
        let status_item = MenuItem::with_id("status", format_status_menu_text(&initial_status), false, None);

        let show_window = MenuItem::new("🖥️ 显示主界面", true, None);
        let copy_id = MenuItem::new("📋 复制终端 ID", true, None);
        let toggle_conn = MenuItem::new("🔴 切断协助 / 🔄 重连", true, None);
        let quit_item = MenuItem::new("🛑 退出 Agent", true, None);

        let show_window_id = show_window.id().clone();
        let copy_id_id = copy_id.id().clone();
        let toggle_conn_id = toggle_conn.id().clone();
        let quit_id = quit_item.id().clone();

        menu.append(&title_item)?;
        menu.append(&id_item)?;
        menu.append(&status_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&show_window)?;
        menu.append(&copy_id)?;
        menu.append(&toggle_conn)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit_item)?;

        let icon = load_tray_icon()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(format!("at-pc 终端代理 ({})", terminal_id))
            .with_icon(icon)
            .build()?;

        Ok(Self {
            _tray: tray,
            status_item,
            show_window_id,
            copy_id_id,
            toggle_conn_id,
            quit_id,
        })
    }

    pub fn update_status(&self, status: &ClientConnectionStatus) {
        let _ = self.status_item.set_text(format_status_menu_text(status));
    }
}
```

- [ ] **Step 4: 运行测试验证格式化逻辑通过**

Run: `cargo test -p at-pc-agent --lib app::tray::tests`  
Expected: PASS.

- [ ] **Step 5: 在 `AgentApp` 中集成托盘轮询与窗口关闭拦截**

修改 `crates/agent/src/app/ui.rs`:
1. 在 `AgentApp` 结构体中添加 `pub tray: Option<AgentTray>` 与 `pub should_quit: bool`。
2. 在 `AgentApp::update` 开头处理 `muda::MenuEvent::receiver()` 和 `tray_icon::TrayIconEvent::receiver()`：
   * 托盘左键点击或收到 `show_window_id`：
     ```rust
     ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
     ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
     ```
   * 收到 `copy_id_id`：调用 `ctx.output_mut(|o| o.copied_text = self.state.terminal_info().terminal_id.clone());`
   * 收到 `toggle_conn_id`：根据当前状态切换触发重连或断开。
   * 收到 `quit_id`：设置 `self.should_quit = true;` 并发送 `egui::ViewportCommand::Close`。
3. 拦截窗口关闭事件：
   ```rust
   if ctx.input(|i| i.viewport().close_requested()) && !self.should_quit {
       ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
       ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
   }
   ```
4. 定期调用 `tray.update_status(&self.state.status())` 保证托盘状态最新。

- [ ] **Step 6: 在 `crates/agent/src/main.rs` 中支持 `--no-tray` 命令行参数**

在 CLI 解析中增加 `--no-tray`，并传递给 GUI 启动配置；若指定 `--no-tray` 则不创建 `AgentTray`。

- [ ] **Step 7: 运行 `cargo check -p at-pc-agent` 确认编译通过**

Run: `cargo check -p at-pc-agent`  
Expected: Finished dev compilation checks with exit code 0.

- [ ] **Step 8: Commit**

```bash
git add crates/agent/src/app/tray.rs crates/agent/src/app/mod.rs crates/agent/src/app/ui.rs crates/agent/src/main.rs
git commit -m "feat(agent): add system tray with close-to-tray window interception"
```

---

### Task 5: 全面测试、端到端集成与发布打包验证 (Verification & Dist Assembly)

**Files:**
- Modify: `scripts/package_dist.py` (if needed for packaging)
- Test: Workspace unit tests and local binary validation

- [ ] **Step 1: 运行整个 Workspace 的自动化测试**

Run: `cargo test --workspace`  
Expected: All unit tests pass across all crates.

- [ ] **Step 2: 本机运行验证 `at-pc-server` 托盘功能**

Run: `cargo run -p at-pc-server`  
手动验证：
1. 观察 macOS 顶部状态栏是否出现 at-pc 状态项图标。
2. 点击菜单项“打开 Web 控制台”，浏览器打开 `http://127.0.0.1:9800`。
3. 点击“退出服务器”，确认服务安全退出，日志和元数据成功落盘。
4. 运行 `cargo run -p at-pc-server -- --no-tray`，验证无状态栏图标且 `Ctrl+C` 正常退出。

- [ ] **Step 3: 本机运行验证 `at-pc-agent` 托盘与最小化功能**

Run: `cargo run -p at-pc-agent`  
手动验证：
1. 主窗口启动，菜单栏出现 Agent 状态图标。
2. 点击主窗口“X”关闭按钮，主窗口消失，托盘依然存在。
3. 点击托盘菜单【显示主界面】，主窗口重新出现并置顶。
4. 点击托盘菜单【退出 Agent】，客户端彻底断开退出。

- [ ] **Step 4: 验证打包构建脚本**

Run: `python3 scripts/package_dist.py --help`  
Expected: Clean execution and build assembly passes without errors.

- [ ] **Step 5: Commit**

```bash
git commit -am "chore: complete system tray implementation and verification"
```
