//! System tray manager and event polling for at-pc-agent.

use crate::ws_client::ClientConnectionStatus;
use muda::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

/// Formats the connection status label shown in the tray menu.
pub fn format_status_menu_text(status: &ClientConnectionStatus) -> String {
    match status {
        ClientConnectionStatus::Connected => "连接状态: 🟢 已连接".to_string(),
        ClientConnectionStatus::Connecting => "连接状态: 🟡 连接中".to_string(),
        ClientConnectionStatus::Reconnecting => "连接状态: 🟡 正在重连".to_string(),
        ClientConnectionStatus::Disconnected => "连接状态: 🔴 离线".to_string(),
    }
}

/// Formats the toggle connection button text according to current status.
pub fn format_toggle_conn_text(status: &ClientConnectionStatus) -> &'static str {
    match status {
        ClientConnectionStatus::Connected => "🔴 切断协助 (Disconnect)",
        ClientConnectionStatus::Connecting
        | ClientConnectionStatus::Reconnecting
        | ClientConnectionStatus::Disconnected => "🔄 立即重连 (Reconnect)",
    }
}

/// Loads the application tray icon from bundled media assets.
pub fn load_tray_icon() -> Result<tray_icon::Icon, Box<dyn std::error::Error + Send + Sync>> {
    let png_bytes = include_bytes!("../../../../media/at-pc-icon.png");
    let img = image::load_from_memory(png_bytes)?.into_rgba8();
    let (width, height) = img.dimensions();
    let rgba = img.into_raw();
    let icon = tray_icon::Icon::from_rgba(rgba, width, height)?;
    Ok(icon)
}

/// Controller managing the native system tray item and menu for at-pc-agent.
pub struct AgentTray {
    pub _tray: TrayIcon,
    pub status_item: MenuItem,
    pub toggle_conn_item: MenuItem,
    pub show_window_id: muda::MenuId,
    pub copy_id_id: muda::MenuId,
    pub toggle_conn_id: muda::MenuId,
    pub quit_id: muda::MenuId,
}

impl AgentTray {
    pub fn new(
        terminal_id: &str,
        initial_status: ClientConnectionStatus,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let menu = Menu::new();
        let title_item = MenuItem::with_id(
            "title",
            format!("at-pc 终端代理 v{}", env!("CARGO_PKG_VERSION")),
            false,
            None,
        );
        let id_display = if terminal_id.len() > 18 {
            format!("{}...", &terminal_id[..18])
        } else {
            terminal_id.to_string()
        };
        let id_item = MenuItem::with_id("term_id", format!("终端 ID: {}", id_display), false, None);
        let status_item =
            MenuItem::with_id("status", format_status_menu_text(&initial_status), false, None);

        let show_window = MenuItem::new("🖥️ 显示主界面", true, None);
        let copy_id = MenuItem::new("📋 复制终端 ID", true, None);
        let toggle_conn_item = MenuItem::new(format_toggle_conn_text(&initial_status), true, None);
        let quit_item = MenuItem::new("🛑 退出 Agent", true, None);

        let show_window_id = show_window.id().clone();
        let copy_id_id = copy_id.id().clone();
        let toggle_conn_id = toggle_conn_item.id().clone();
        let quit_id = quit_item.id().clone();

        menu.append(&title_item)?;
        menu.append(&id_item)?;
        menu.append(&status_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&show_window)?;
        menu.append(&copy_id)?;
        menu.append(&toggle_conn_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit_item)?;

        let icon = load_tray_icon()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(format!("at-pc 终端代理 ({})", id_display))
            .with_icon(icon)
            .build()?;

        Ok(Self {
            _tray: tray,
            status_item,
            toggle_conn_item,
            show_window_id,
            copy_id_id,
            toggle_conn_id,
            quit_id,
        })
    }

    /// Updates the dynamic status labels in the tray menu.
    pub fn update_status(&self, status: &ClientConnectionStatus) {
        let _ = self.status_item.set_text(format_status_menu_text(status));
        let _ = self
            .toggle_conn_item
            .set_text(format_toggle_conn_text(status));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_connection_status_text() {
        assert_eq!(
            format_status_menu_text(&ClientConnectionStatus::Connected),
            "连接状态: 🟢 已连接"
        );
        assert_eq!(
            format_status_menu_text(&ClientConnectionStatus::Connecting),
            "连接状态: 🟡 连接中"
        );
        assert_eq!(
            format_status_menu_text(&ClientConnectionStatus::Reconnecting),
            "连接状态: 🟡 正在重连"
        );
        assert_eq!(
            format_status_menu_text(&ClientConnectionStatus::Disconnected),
            "连接状态: 🔴 离线"
        );
    }

    #[test]
    fn test_format_toggle_conn_text() {
        assert!(format_toggle_conn_text(&ClientConnectionStatus::Connected).contains("切断协助"));
        assert!(format_toggle_conn_text(&ClientConnectionStatus::Disconnected).contains("立即重连"));
        assert!(format_toggle_conn_text(&ClientConnectionStatus::Connecting).contains("立即重连"));
    }

    #[test]
    fn test_load_tray_icon_succeeds() {
        let icon_res = load_tray_icon();
        assert!(
            icon_res.is_ok(),
            "Agent tray icon must load cleanly from media/at-pc-icon.png"
        );
    }
}
