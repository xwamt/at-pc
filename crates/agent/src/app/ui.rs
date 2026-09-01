//! UI rendering and layout components using egui/eframe for at-pc-agent.

use crate::app::AgentAppState;
use crate::ws_client::ClientConnectionStatus;
use std::sync::Arc;
use std::time::Duration;

/// Main application window for at-pc terminal agent.
pub struct AgentApp {
    pub state: Arc<AgentAppState>,
}

impl AgentApp {
    /// Creates a new `AgentApp` instance.
    pub fn new(state: Arc<AgentAppState>) -> Self {
        Self { state }
    }
}

/// Configures custom / system CJK fonts for Chinese character rendering.
pub fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Standard system font paths on Windows, macOS, and Linux
    let font_paths = [
        // Windows
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyh.ttf",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "C:\\Windows\\Fonts\\simhei.ttf",
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        // Linux
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    ];

    for path in font_paths {
        if let Ok(font_data) = std::fs::read(path) {
            fonts.font_data.insert(
                "system_cjk".to_owned(),
                egui::FontData::from_owned(font_data),
            );
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.push("system_cjk".to_owned());
            }
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                family.push("system_cjk".to_owned());
            }
            break;
        }
    }

    ctx.set_fonts(fonts);
}

impl eframe::App for AgentApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request repaint every 250ms for responsive status updates and audit logs
        ctx.request_repaint_after(Duration::from_millis(250));

        let terminal_info = self.state.get_terminal_info();
        let server_url = self.state.server_url();
        let status = self.state.status();
        let is_stopped = self.state.is_stopped();
        let logs = self.state.get_audit_logs();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);

            // Header Section
            ui.horizontal(|ui| {
                ui.heading("💻 at-pc 终端代理");
            });
            ui.label(
                egui::RichText::new(format!(
                    "Terminal Agent C/S Edition (v{})",
                    env!("CARGO_PKG_VERSION")
                ))
                .weak()
                .size(11.0),
            );

            ui.add_space(4.0);

            // Status Badge
            let (status_text, status_color) = if is_stopped {
                (
                    "🔴 代理已停止 (Agent Stopped / Disconnected)".to_string(),
                    egui::Color32::from_rgb(220, 60, 60),
                )
            } else {
                match status {
                    ClientConnectionStatus::Connected => (
                        format!("🟢 已连接到服务器 [{}]", server_url),
                        egui::Color32::from_rgb(40, 180, 80),
                    ),
                    ClientConnectionStatus::Connecting => (
                        format!("🟡 正在连接服务器 [{}]...", server_url),
                        egui::Color32::from_rgb(240, 180, 50),
                    ),
                    ClientConnectionStatus::Reconnecting => (
                        format!("🟡 重新连接服务器 [{}] 中...", server_url),
                        egui::Color32::from_rgb(240, 180, 50),
                    ),
                    ClientConnectionStatus::Disconnected => (
                        "🔴 未连接服务器 (Disconnected)".to_string(),
                        egui::Color32::from_rgb(220, 60, 60),
                    ),
                }
            };

            egui::Frame::group(ui.style())
                .fill(egui::Color32::from_black_alpha(25))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .rounding(6.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(status_text)
                                .color(status_color)
                                .strong()
                                .size(13.0),
                        );
                    });
                });

            ui.add_space(4.0);

            // Info Card: Terminal Metadata
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::same(10.0))
                .rounding(8.0)
                .show(ui, |ui| {
                    egui::Grid::new("terminal_metadata_grid")
                        .num_columns(2)
                        .spacing([16.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("终端标识 (Terminal ID):").strong());
                            ui.label(
                                egui::RichText::new(&terminal_info.terminal_id)
                                    .monospace()
                                    .strong()
                                    .size(13.0)
                                    .color(egui::Color32::from_rgb(255, 175, 50)),
                            );
                            ui.end_row();

                            ui.label(egui::RichText::new("主机名称 (Hostname):").strong());
                            ui.label(
                                egui::RichText::new(&terminal_info.hostname)
                                    .monospace()
                                    .size(12.0),
                            );
                            ui.end_row();

                            ui.label(egui::RichText::new("局域网 IP (LAN IP):").strong());
                            ui.label(
                                egui::RichText::new(&terminal_info.lan_ip)
                                    .monospace()
                                    .size(12.0),
                            );
                            ui.end_row();

                            ui.label(egui::RichText::new("操作系统 (OS Version):").strong());
                            ui.label(
                                egui::RichText::new(&terminal_info.os_version)
                                    .size(12.0),
                            );
                            ui.end_row();

                            ui.label(egui::RichText::new("服务器地址 (Server URL):").strong());
                            ui.label(
                                egui::RichText::new(&server_url)
                                    .monospace()
                                    .size(12.0),
                            );
                            ui.end_row();
                        });
                });

            ui.add_space(6.0);
            ui.separator();

            // Real-Time Audit Log Header
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("📜 实时工具调用审计 (Tool Execution Audit)").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("共 {} 条记录", logs.len()))
                            .weak()
                            .size(11.0),
                    );
                });
            });

            // Scrollable Audit Log Box
            egui::Frame::group(ui.style())
                .fill(egui::Color32::from_black_alpha(40))
                .inner_margin(egui::Margin::same(8.0))
                .rounding(6.0)
                .show(ui, |ui| {
                    let max_scroll_height = (ui.available_height() - 55.0).max(120.0);
                    egui::ScrollArea::vertical()
                        .max_height(max_scroll_height)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            if logs.is_empty() {
                                ui.add_space(20.0);
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            "暂无工具调用记录，等待中央服务器派发任务...",
                                        )
                                        .weak(),
                                    );
                                });
                                ui.add_space(20.0);
                            } else {
                                for entry in &logs {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("[{}]", entry.timestamp))
                                                .weak()
                                                .monospace()
                                                .size(11.0),
                                        );

                                        let (status_tag, tag_color) = match entry.status.as_str() {
                                            "SUCCESS" => (
                                                "✓ SUCCESS",
                                                egui::Color32::from_rgb(60, 200, 90),
                                            ),
                                            "STARTED" => (
                                                "▶ STARTED",
                                                egui::Color32::from_rgb(80, 170, 240),
                                            ),
                                            "STOPPED" => (
                                                "⏹ STOPPED",
                                                egui::Color32::from_rgb(220, 120, 50),
                                            ),
                                            "FAILED" => (
                                                "✗ FAILED",
                                                egui::Color32::from_rgb(240, 70, 70),
                                            ),
                                            _ => (
                                                entry.status.as_str(),
                                                egui::Color32::from_rgb(200, 200, 200),
                                            ),
                                        };

                                        ui.label(
                                            egui::RichText::new(status_tag)
                                                .color(tag_color)
                                                .strong()
                                                .monospace()
                                                .size(11.0),
                                        );

                                        ui.label(
                                            egui::RichText::new(&entry.tool_name)
                                                .strong()
                                                .monospace()
                                                .size(12.0),
                                        );

                                        if let Some(dur) = entry.duration_ms {
                                            ui.label(
                                                egui::RichText::new(format!("({}ms)", dur))
                                                    .weak()
                                                    .size(11.0),
                                            );
                                        }

                                        if !entry.summary.is_empty() {
                                            ui.label(
                                                egui::RichText::new(format!("- {}", entry.summary))
                                                    .size(11.0),
                                            );
                                        }
                                    });
                                    ui.add_space(2.0);
                                }
                            }
                        });
                });

            ui.add_space(6.0);

            // Emergency Disconnect Section (Footer)
            if is_stopped {
                let disabled_stop_btn = egui::Button::new(
                    egui::RichText::new("🔴 已断开连接 (Emergency Disconnected)")
                        .color(egui::Color32::from_rgb(180, 180, 180))
                        .size(13.0),
                )
                .fill(egui::Color32::from_rgb(80, 80, 80))
                .min_size(egui::vec2(ui.available_width(), 34.0));

                let _ = ui.add_enabled(false, disabled_stop_btn);
            } else {
                let disconnect_btn = egui::Button::new(
                    egui::RichText::new("🔴 立即断开连接 (Emergency Disconnect)")
                        .color(egui::Color32::WHITE)
                        .strong()
                        .size(13.0),
                )
                .fill(egui::Color32::from_rgb(200, 40, 40))
                .min_size(egui::vec2(ui.available_width(), 34.0));

                if ui.add_enabled(true, disconnect_btn).clicked() {
                    self.state.trigger_emergency_disconnect();
                }
            }
        });
    }
}

/// Runs the native eframe desktop application for Agent.
pub fn run_agent_app(state: Arc<AgentAppState>) -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([500.0, 620.0])
            .with_min_inner_size([420.0, 460.0])
            .with_title("at-pc 终端代理"),
        ..Default::default()
    };

    eframe::run_native(
        "at-pc 终端代理",
        native_options,
        Box::new(move |cc| {
            setup_custom_fonts(&cc.egui_ctx);
            Box::new(AgentApp::new(state))
        }),
    )
}
