//! System tray utilities, platform helpers, and tray controller for at-pc-server.

use muda::{Menu, MenuItem, PredefinedMenuItem};
use std::path::Path;
use std::process::Command;
use tracing::{error, info};
use tray_icon::{TrayIcon, TrayIconBuilder};

/// Loads the application tray icon from bundled media assets.
pub fn load_tray_icon() -> Result<tray_icon::Icon, Box<dyn std::error::Error + Send + Sync>> {
    let png_bytes = include_bytes!("../../../media/at-pc-icon.png");
    let img = image::load_from_memory(png_bytes)?.into_rgba8();
    let (width, height) = img.dimensions();
    let rgba = img.into_raw();
    let icon = tray_icon::Icon::from_rgba(rgba, width, height)?;
    Ok(icon)
}

/// Builds the platform-specific command to open a URL in the user's default browser.
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

/// Spawns the default system browser opening the specified URL.
pub fn open_browser_url(url: &str) {
    info!("Opening browser URL: {}", url);
    if let Err(e) = build_browser_command(url).spawn() {
        error!("Failed to open browser URL {}: {}", url, e);
    }
}

/// Builds the platform-specific command to reveal/select a file or directory in the file manager.
pub fn build_locate_command(path: &Path) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.args(["-R", &path.to_string_lossy()]);
        cmd
    }
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("explorer");
        cmd.args(["/select,", &path.to_string_lossy()]);
        cmd
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let mut cmd = Command::new("xdg-open");
        let dir = path.parent().unwrap_or(path);
        cmd.arg(dir);
        cmd
    }
}

/// Reveals a file in the system file explorer.
pub fn locate_file_in_explorer(path: &Path) {
    info!("Locating path in file manager: {}", path.display());
    if let Err(e) = build_locate_command(path).spawn() {
        error!("Failed to locate file {}: {}", path.display(), e);
    }
}

/// Decides whether the system tray should be initialized based on CLI flags and environment.
pub fn should_enable_tray(args: &[String]) -> bool {
    // CLI flags: --stdio (used by Cursor/IDE) and --no-tray explicitly disable tray
    if args.iter().any(|a| a == "--stdio" || a == "--no-tray") {
        return false;
    }

    // Check if running in headless Linux environment without display server
    #[cfg(target_os = "linux")]
    {
        if std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err() {
            return false;
        }
    }

    true
}

/// Manages the at-pc-server system tray icon and its native menu items.
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

/// Custom user events for waking up the winit event loop from tray / menu callbacks.
#[derive(Debug)]
pub enum ServerAppEvent {
    Menu(muda::MenuEvent),
    Tray(tray_icon::TrayIconEvent),
    ShutdownCompleted,
}

/// Runs the native system tray event loop on the main thread.
pub fn run_tray_event_loop(
    tray: ServerTray,
    web_port: u16,
    log_file_path: std::path::PathBuf,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    completion_proxy: winit::event_loop::EventLoopProxy<ServerAppEvent>,
    event_loop: winit::event_loop::EventLoop<ServerAppEvent>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let proxy = std::sync::Arc::new(std::sync::Mutex::new(completion_proxy));

    let proxy_menu = proxy.clone();
    muda::MenuEvent::set_event_handler(Some(Box::new(move |event| {
        if let Ok(p) = proxy_menu.lock() {
            let _ = p.send_event(ServerAppEvent::Menu(event));
        }
    })));

    let proxy_tray = proxy.clone();
    tray_icon::TrayIconEvent::set_event_handler(Some(Box::new(move |event| {
        if let Ok(p) = proxy_tray.lock() {
            let _ = p.send_event(ServerAppEvent::Tray(event));
        }
    })));

    let mut is_shutting_down = false;
    let _tray_keeper = tray;

    event_loop.run(move |event, target| {
        target.set_control_flow(winit::event_loop::ControlFlow::Wait);

        match event {
            winit::event::Event::UserEvent(ServerAppEvent::ShutdownCompleted) => {
                info!("Server shutdown completed cleanly. Exiting event loop.");
                target.exit();
            }
            winit::event::Event::UserEvent(ServerAppEvent::Menu(menu_event)) => {
                if menu_event.id == _tray_keeper.open_web_id {
                    let url = format!("http://127.0.0.1:{}", web_port);
                    open_browser_url(&url);
                } else if menu_event.id == _tray_keeper.open_logs_id {
                    locate_file_in_explorer(&log_file_path);
                } else if menu_event.id == _tray_keeper.quit_id {
                    if !is_shutting_down {
                        is_shutting_down = true;
                        info!("Quit requested from system tray. Initiating graceful shutdown...");
                        let _ = shutdown_tx.send(());
                    }
                }
            }
            _ => {}
        }
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_tray_icon_succeeds() {
        let icon_res = load_tray_icon();
        assert!(icon_res.is_ok(), "Tray icon must load cleanly from media/at-pc-icon.png");
    }

    #[test]
    fn test_browser_command_building() {
        let cmd = build_browser_command("http://127.0.0.1:9800");
        assert!(!cmd.get_program().to_string_lossy().is_empty());
    }

    #[test]
    fn test_locate_command_building() {
        let path = Path::new("/tmp/test.log");
        let cmd = build_locate_command(path);
        assert!(!cmd.get_program().to_string_lossy().is_empty());
    }

    #[test]
    fn test_should_enable_tray_logic() {
        assert!(!should_enable_tray(&["at-pc-server".into(), "--stdio".into()]));
        assert!(!should_enable_tray(&["at-pc-server".into(), "--no-tray".into()]));
        assert!(should_enable_tray(&["at-pc-server".into()]));
        assert!(should_enable_tray(&["at-pc-server".into(), "--host".into(), "127.0.0.1".into()]));
    }
}
