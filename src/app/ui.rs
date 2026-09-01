//! UI rendering and layout components using egui/eframe.

use crate::app::state::GuiState;
use crate::server::state::{AppState, AuditLogStatus};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Main application window for at-pc remote troubleshooter.
pub struct PcTroubleshooterApp {
    pub state: GuiState,
}

impl PcTroubleshooterApp {
    /// Creates a new `PcTroubleshooterApp` instance.
    pub fn new(lan_ip: String, server_state: Arc<AppState>) -> Self {
        Self {
            state: GuiState::new(lan_ip, server_state),
        }
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

impl eframe::App for PcTroubleshooterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll and collect new real-time audit logs
        self.state.poll_audit_logs();

        // Request repaint every 250ms for responsive status updates and timers
        ctx.request_repaint_after(Duration::from_millis(250));

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);

            // Header Section
            ui.horizontal(|ui| {
                ui.heading("💻 at-pc 远程协助助手");
            });
            ui.label(
                egui::RichText::new("LAN MCP Remote Troubleshooter (v0.1.0)")
                    .weak()
                    .size(11.0),
            );

            ui.add_space(4.0);

            // Status Badge
            let (status_text, status_color) = if self.state.is_stopped {
                (
                    "🔴 服务已停止 (Service Stopped / Disconnected)".to_string(),
                    egui::Color32::from_rgb(220, 60, 60),
                )
            } else if self.state.connected_clients() > 0 {
                let count = self.state.connected_clients();
                (
                    format!("🔵 工程师正在协助... (已连接 {} 个会话)", count),
                    egui::Color32::from_rgb(50, 140, 240),
                )
            } else {
                (
                    "🟢 等待工程师连接... (0 个活跃会话)".to_string(),
                    egui::Color32::from_rgb(40, 180, 80),
                )
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
                                .size(14.0),
                        );
                    });
                });

            ui.add_space(4.0);

            // Info Card: LAN IP, Port, PIN
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::same(12.0))
                .rounding(8.0)
                .show(ui, |ui| {
                    egui::Grid::new("connection_info_grid")
                        .num_columns(2)
                        .spacing([24.0, 8.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("局域网 IP:").strong());
                            ui.label(
                                egui::RichText::new(&self.state.lan_ip)
                                    .monospace()
                                    .size(14.0),
                            );
                            ui.end_row();

                            ui.label(egui::RichText::new("服务端口:").strong());
                            ui.label(
                                egui::RichText::new(format!("{}", self.state.port))
                                    .monospace()
                                    .size(14.0),
                            );
                            ui.end_row();

                            ui.label(egui::RichText::new("连接 PIN:").strong());
                            ui.label(
                                egui::RichText::new(&self.state.pin)
                                    .monospace()
                                    .strong()
                                    .size(24.0)
                                    .color(egui::Color32::from_rgb(255, 175, 50)),
                            );
                            ui.end_row();
                        });
                });

            ui.add_space(4.0);

            // Action Buttons: Copy MCP Config JSON & Rotate PIN
            ui.horizontal(|ui| {
                let copy_btn = ui.button(
                    egui::RichText::new("📋 一键复制 MCP 配置 JSON")
                        .strong()
                        .size(13.0),
                );

                if copy_btn.clicked() {
                    let config_json = self.state.generate_mcp_config();
                    ctx.output_mut(|o| o.copied_text = config_json);
                    self.state.last_copied_time = Some(Instant::now());
                }

                let rotate_btn = ui.add_enabled(
                    !self.state.is_stopped,
                    egui::Button::new(
                        egui::RichText::new("🔄 重新生成 PIN / 切换端口")
                            .strong()
                            .size(13.0),
                    ),
                );

                if rotate_btn.clicked() {
                    if self.state.connected_clients() > 0 {
                        self.state.show_rotate_confirm = true;
                    } else {
                        self.state.rotate_credentials(None, None);
                    }
                }
            });

            if let Some(copied_at) = self.state.last_copied_time {
                if copied_at.elapsed().as_secs() < 3 {
                    ui.label(
                        egui::RichText::new("✓ 已复制到剪贴板!")
                            .color(egui::Color32::from_rgb(50, 200, 80))
                            .strong(),
                    );
                }
            }

            ui.add_space(6.0);
            ui.separator();

            // Real-Time Audit Log Header
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("📜 实时操作审计日志 (Audit Log)").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("共 {} 条记录", self.state.audit_logs.len()))
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
                    let max_scroll_height = (ui.available_height() - 75.0).max(120.0);
                    egui::ScrollArea::vertical()
                        .max_height(max_scroll_height)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            if self.state.audit_logs.is_empty() {
                                ui.add_space(30.0);
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            "暂无操作记录，等待工程师连接并调用工具...",
                                        )
                                        .weak(),
                                    );
                                });
                                ui.add_space(30.0);
                            } else {
                                for entry in &self.state.audit_logs {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("[{}]", entry.timestamp))
                                                .weak()
                                                .monospace()
                                                .size(11.0),
                                        );

                                        let (status_tag, tag_color) = match entry.status {
                                            AuditLogStatus::Success => {
                                                ("✓ SUCCESS", egui::Color32::from_rgb(60, 200, 90))
                                            }
                                            AuditLogStatus::Started => {
                                                ("▶ STARTED", egui::Color32::from_rgb(80, 170, 240))
                                            }
                                            AuditLogStatus::Stopped => {
                                                ("⏹ STOPPED", egui::Color32::from_rgb(220, 120, 50))
                                            }
                                            AuditLogStatus::Failed => {
                                                ("✗ FAILED", egui::Color32::from_rgb(240, 70, 70))
                                            }
                                            AuditLogStatus::Error => {
                                                ("✗ ERROR", egui::Color32::from_rgb(240, 70, 70))
                                            }
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

                                        if let Some(ip) = &entry.client_ip {
                                            ui.label(
                                                egui::RichText::new(format!("[IP: {}]", ip))
                                                    .weak()
                                                    .size(11.0),
                                            );
                                        }

                                        if let Some(msg) = &entry.message {
                                            ui.label(
                                                egui::RichText::new(format!("- {}", msg)).size(11.0),
                                            );
                                        }
                                    });
                                    ui.add_space(2.0);
                                }
                            }
                        });
                });

            ui.add_space(8.0);

            // Emergency Stop / Session Restart Section (Footer)
            if self.state.is_stopped {
                let restart_btn = egui::Button::new(
                    egui::RichText::new("🟢 重新开始协助 (Start New Session)")
                        .color(egui::Color32::WHITE)
                        .strong()
                        .size(14.0),
                )
                .fill(egui::Color32::from_rgb(40, 160, 70))
                .min_size(egui::vec2(ui.available_width(), 36.0));

                if ui.add(restart_btn).clicked() {
                    self.state.restart_session(None);
                }

                ui.add_space(4.0);

                let disabled_stop_btn = egui::Button::new(
                    egui::RichText::new("🔴 立即断开协助 (已停止)")
                        .color(egui::Color32::from_rgb(180, 180, 180))
                        .size(12.0),
                )
                .fill(egui::Color32::from_rgb(80, 80, 80))
                .min_size(egui::vec2(ui.available_width(), 26.0));

                let _ = ui.add_enabled(false, disabled_stop_btn);
            } else {
                let disconnect_btn = egui::Button::new(
                    egui::RichText::new("🔴 立即断开协助 (Emergency Stop)")
                        .color(egui::Color32::WHITE)
                        .strong()
                        .size(14.0),
                )
                .fill(egui::Color32::from_rgb(200, 40, 40))
                .min_size(egui::vec2(ui.available_width(), 36.0));

                if ui.add_enabled(true, disconnect_btn).clicked() {
                    self.state.trigger_emergency_stop();
                }
            }
        });

        // Confirmation Modal for PIN Rotation
        if self.state.show_rotate_confirm {
            egui::Window::new("⚠️ 确认重新生成 PIN / 切换端口")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(format!(
                        "当前有 {} 个活跃工程师会话正在协助中。\n重新生成 PIN 将导致当前会话立即断开失效。\n确定要继续重新生成并轮换吗？",
                        self.state.connected_clients()
                    ));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("确定重新生成 (Confirm)").clicked() {
                            self.state.rotate_credentials(None, None);
                            self.state.show_rotate_confirm = false;
                        }
                        if ui.button("取消 (Cancel)").clicked() {
                            self.state.show_rotate_confirm = false;
                        }
                    });
                });
        }
    }
}

/// Runs the native eframe desktop application.
pub fn run_app(lan_ip: String, server_state: Arc<AppState>) -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([500.0, 640.0])
            .with_min_inner_size([420.0, 480.0])
            .with_title("at-pc 远程协助助手"),
        ..Default::default()
    };

    eframe::run_native(
        "at-pc 远程协助助手",
        native_options,
        Box::new(move |cc| {
            setup_custom_fonts(&cc.egui_ctx);
            Box::new(PcTroubleshooterApp::new(lan_ip, server_state))
        }),
    )
}
