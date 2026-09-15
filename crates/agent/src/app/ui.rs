//! UI rendering and layout components using egui/eframe for at-pc-agent.

use crate::app::AgentAppState;
use crate::ws_client::ClientConnectionStatus;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Main application window for at-pc terminal agent.
pub struct AgentApp {
    pub state: Arc<AgentAppState>,
    pub server_url_input: String,
    pub is_editing_server: bool,
    pub feedback_msg: Option<(String, bool)>,
    pub copied_id_timer: Option<Instant>,
    pub filter_failed_only: bool,
}

impl AgentApp {
    /// Creates a new `AgentApp` instance.
    pub fn new(state: Arc<AgentAppState>) -> Self {
        let initial_url = state.server_url();
        Self {
            state,
            server_url_input: initial_url,
            is_editing_server: false,
            feedback_msg: None,
            copied_id_timer: None,
            filter_failed_only: false,
        }
    }
}

/// Applies the Obsidian Cyber-Ops modern dark design tokens to egui context.
pub fn setup_modern_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    // Base background colors
    visuals.override_text_color = Some(egui::Color32::from_rgb(241, 245, 249)); // #f1f5f9
    visuals.panel_fill = egui::Color32::from_rgb(11, 15, 25); // Obsidian deep dark #0b0f19
    visuals.window_fill = egui::Color32::from_rgb(17, 22, 34); // #111622
    visuals.extreme_bg_color = egui::Color32::from_rgb(8, 12, 20); // #080c14

    let rounding = egui::Rounding::same(7.0);

    // Non-interactive surfaces (cards, panels)
    visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(21, 28, 44); // Card surface #151c2c
    visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(39, 53, 79)); // Subtle border #27354f
    visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);

    // Inactive button/inputs
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(26, 35, 54);
    visuals.widgets.inactive.bg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 62, 92));
    visuals.widgets.inactive.rounding = rounding;
    visuals.widgets.inactive.fg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(226, 232, 240));

    // Hovered button/inputs (Neon Cyan highlight)
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(33, 45, 71);
    visuals.widgets.hovered.bg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(56, 189, 248));
    visuals.widgets.hovered.rounding = rounding;
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);

    // Active button/inputs
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(14, 116, 144);
    visuals.widgets.active.bg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(56, 189, 248));
    visuals.widgets.active.rounding = rounding;
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);

    // Selection highlight
    visuals.selection.bg_fill = egui::Color32::from_rgb(14, 116, 144);
    visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(56, 189, 248));

    ctx.set_visuals(visuals);
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
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 9.0);

            // 1. Header Section
            ui.horizontal(|ui| {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("💻")
                        .size(20.0),
                );
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("at-pc 终端代理")
                                .strong()
                                .size(16.0)
                                .color(egui::Color32::from_rgb(241, 245, 249)),
                        );
                        ui.label(
                            egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                .size(10.0)
                                .color(egui::Color32::from_rgb(56, 189, 248)),
                        );
                    });
                    ui.label(
                        egui::RichText::new("LAN PC Diagnostics & Assistance Client · Obsidian Edition")
                            .weak()
                            .size(10.0),
                    );
                });
            });

            ui.add_space(2.0);

            // 2. Dynamic Status Banner
            let (status_icon, status_title, status_sub, banner_bg, border_color, text_color) = if is_stopped {
                (
                    "🔴",
                    "代理已切断连接 (Agent Stopped)",
                    "所有远程控制指令已被拦截并停止",
                    egui::Color32::from_rgba_premultiplied(127, 29, 29, 65),
                    egui::Color32::from_rgb(239, 68, 68),
                    egui::Color32::from_rgb(254, 202, 202),
                )
            } else {
                match status {
                    ClientConnectionStatus::Connected => (
                        "🟢",
                        "已连接中央控制网关",
                        "实时链路就绪，等待运维指令",
                        egui::Color32::from_rgba_premultiplied(6, 78, 59, 65),
                        egui::Color32::from_rgb(16, 185, 129),
                        egui::Color32::from_rgb(167, 243, 208),
                    ),
                    ClientConnectionStatus::Connecting => (
                        "🟡",
                        "正在连接中央网关...",
                        "尝试建立安全握手通道",
                        egui::Color32::from_rgba_premultiplied(120, 53, 15, 65),
                        egui::Color32::from_rgb(245, 158, 11),
                        egui::Color32::from_rgb(253, 230, 138),
                    ),
                    ClientConnectionStatus::Reconnecting => (
                        "🟡",
                        "重新连接中央网关中...",
                        "网络波动，正在按指数退避策略重试",
                        egui::Color32::from_rgba_premultiplied(120, 53, 15, 65),
                        egui::Color32::from_rgb(245, 158, 11),
                        egui::Color32::from_rgb(253, 230, 138),
                    ),
                    ClientConnectionStatus::Disconnected => (
                        "⚪",
                        "未连接到网关 (Disconnected)",
                        "请检查网关地址或网络连通性",
                        egui::Color32::from_rgba_premultiplied(51, 65, 85, 65),
                        egui::Color32::from_rgb(100, 116, 139),
                        egui::Color32::from_rgb(203, 213, 225),
                    ),
                }
            };

            egui::Frame::none()
                .fill(banner_bg)
                .stroke(egui::Stroke::new(1.0, border_color))
                .inner_margin(egui::Margin::symmetric(12.0, 7.0))
                .rounding(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(status_icon).size(13.0));
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(status_title)
                                    .color(text_color)
                                    .strong()
                                    .size(12.0),
                            );
                            ui.label(
                                egui::RichText::new(status_sub)
                                    .color(egui::Color32::from_rgb(148, 163, 184))
                                    .size(10.0),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let ping_tag = if status == ClientConnectionStatus::Connected && !is_stopped {
                                "● 正常"
                            } else {
                                "○ 待命"
                            };
                            ui.label(
                                egui::RichText::new(ping_tag)
                                    .color(text_color)
                                    .size(10.0)
                                    .monospace(),
                            );
                        });
                    });
                });

            ui.add_space(2.0);

            // 3. Info Card: Terminal Metadata
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(21, 28, 44))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(39, 53, 79)))
                .inner_margin(egui::Margin::same(11.0))
                .rounding(9.0)
                .show(ui, |ui| {
                    // Card Header with 1-Click Copy
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("🖥️ 本机终端:")
                                .color(egui::Color32::from_rgb(148, 163, 184))
                                .size(12.0),
                        );
                        ui.label(
                            egui::RichText::new(&terminal_info.terminal_id)
                                .monospace()
                                .strong()
                                .size(13.0)
                                .color(egui::Color32::from_rgb(245, 158, 11)), // Amber accent
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let is_copied = self
                                .copied_id_timer
                                .map(|t| t.elapsed() < Duration::from_millis(1500))
                                .unwrap_or(false);

                            let copy_btn_text = if is_copied {
                                egui::RichText::new("✓ 已复制")
                                    .color(egui::Color32::from_rgb(52, 211, 153))
                                    .size(11.0)
                                    .strong()
                            } else {
                                egui::RichText::new("📋 复制 ID")
                                    .size(11.0)
                                    .color(egui::Color32::from_rgb(203, 213, 225))
                            };

                            if ui.button(copy_btn_text).clicked() {
                                ctx.output_mut(|o| o.copied_text = terminal_info.terminal_id.clone());
                                self.copied_id_timer = Some(Instant::now());
                            }
                        });
                    });

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // Grid details
                    egui::Grid::new("terminal_metadata_grid_obsidian")
                        .num_columns(2)
                        .spacing([18.0, 7.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("主机名称:").color(egui::Color32::from_rgb(148, 163, 184)).size(11.0));
                            ui.label(
                                egui::RichText::new(&terminal_info.hostname)
                                    .monospace()
                                    .size(11.0)
                                    .color(egui::Color32::from_rgb(241, 245, 249)),
                            );
                            ui.end_row();

                            ui.label(egui::RichText::new("局域网 IP:").color(egui::Color32::from_rgb(148, 163, 184)).size(11.0));
                            ui.label(
                                egui::RichText::new(&terminal_info.lan_ip)
                                    .monospace()
                                    .size(11.0)
                                    .color(egui::Color32::from_rgb(56, 189, 248)), // Cyan
                            );
                            ui.end_row();

                            ui.label(egui::RichText::new("操作系统:").color(egui::Color32::from_rgb(148, 163, 184)).size(11.0));
                            ui.label(
                                egui::RichText::new(&terminal_info.os_version)
                                    .size(11.0)
                                    .color(egui::Color32::from_rgb(226, 232, 240)),
                            );
                            ui.end_row();

                            ui.label(egui::RichText::new("网关地址:").color(egui::Color32::from_rgb(148, 163, 184)).size(11.0));
                            ui.horizontal(|ui| {
                                if !self.is_editing_server {
                                    ui.label(
                                        egui::RichText::new(&server_url)
                                            .monospace()
                                            .size(11.0)
                                            .color(egui::Color32::from_rgb(148, 163, 184)),
                                    );
                                    if ui.button(egui::RichText::new("✏️ 修改").size(10.0)).clicked() {
                                        self.is_editing_server = true;
                                        self.server_url_input = server_url.clone();
                                        self.feedback_msg = None;
                                    }
                                } else {
                                    let edit_resp = ui.add(
                                        egui::TextEdit::singleline(&mut self.server_url_input)
                                            .desired_width(160.0)
                                            .font(egui::TextStyle::Monospace),
                                    );
                                    if edit_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                        match self.state.update_server_url(self.server_url_input.clone(), true) {
                                            Ok(_) => {
                                                self.is_editing_server = false;
                                                self.feedback_msg = Some(("配置已生效并重连中...".to_string(), false));
                                            }
                                            Err(e) => {
                                                self.feedback_msg = Some((e, true));
                                            }
                                        }
                                    }

                                    if ui
                                        .button(egui::RichText::new("💾 保存").color(egui::Color32::from_rgb(52, 211, 153)).size(10.0))
                                        .clicked()
                                    {
                                        match self.state.update_server_url(self.server_url_input.clone(), true) {
                                            Ok(_) => {
                                                self.is_editing_server = false;
                                                self.feedback_msg = Some(("配置已保存并立即生效".to_string(), false));
                                            }
                                            Err(e) => {
                                                self.feedback_msg = Some((e, true));
                                            }
                                        }
                                    }

                                    if ui.button(egui::RichText::new("❌").size(10.0)).clicked() {
                                        self.is_editing_server = false;
                                        self.server_url_input = server_url.clone();
                                        self.feedback_msg = None;
                                    }
                                }
                            });
                            ui.end_row();
                        });

                    if let Some((ref msg, is_err)) = self.feedback_msg {
                        let color = if is_err {
                            egui::Color32::from_rgb(244, 63, 94)
                        } else {
                            egui::Color32::from_rgb(52, 211, 153)
                        };
                        ui.add_space(3.0);
                        ui.label(egui::RichText::new(msg).color(color).size(11.0));
                    }
                });

            ui.add_space(3.0);

            // 4. Real-Time Audit Log Header & Action Bar
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("📜 实时指令审计流")
                        .strong()
                        .size(12.0)
                        .color(egui::Color32::from_rgb(226, 232, 240)),
                );

                let display_logs: Vec<_> = if self.filter_failed_only {
                    logs.iter().filter(|l| l.status == "FAILED" || l.status == "STOPPED").cloned().collect()
                } else {
                    logs.clone()
                };

                ui.label(
                    egui::RichText::new(format!("({} 条)", display_logs.len()))
                        .size(11.0)
                        .color(egui::Color32::from_rgb(148, 163, 184)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(egui::RichText::new("🗑 清空").size(10.0)).clicked() {
                        self.state.clear_audit_logs();
                    }

                    let filter_label = if self.filter_failed_only {
                        egui::RichText::new("仅异常 [开]").color(egui::Color32::from_rgb(251, 113, 133)).size(10.0)
                    } else {
                        egui::RichText::new("过滤异常").size(10.0)
                    };
                    if ui.button(filter_label).clicked() {
                        self.filter_failed_only = !self.filter_failed_only;
                    }
                });
            });

            // 5. Scrollable Audit Log Box
            let display_logs: Vec<_> = if self.filter_failed_only {
                logs.iter().filter(|l| l.status == "FAILED" || l.status == "STOPPED").cloned().collect()
            } else {
                logs.clone()
            };

            egui::Frame::none()
                .fill(egui::Color32::from_rgb(12, 17, 28))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(30, 41, 59)))
                .inner_margin(egui::Margin::same(7.0))
                .rounding(8.0)
                .show(ui, |ui| {
                    let max_scroll_height = (ui.available_height() - 60.0).max(110.0);
                    egui::ScrollArea::vertical()
                        .max_height(max_scroll_height)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            if display_logs.is_empty() {
                                ui.add_space(24.0);
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            "暂无匹配的工具调用审计记录 · 等待中央服务器派发任务...",
                                        )
                                        .color(egui::Color32::from_rgb(100, 116, 139))
                                        .size(11.0),
                                    );
                                });
                                ui.add_space(24.0);
                            } else {
                                for entry in &display_logs {
                                    let (status_text, bg_col, border_col, fg_col) = match entry.status.as_str() {
                                        "SUCCESS" => (
                                            "✓ 成功",
                                            egui::Color32::from_rgba_premultiplied(16, 185, 129, 30),
                                            egui::Color32::from_rgb(16, 185, 129),
                                            egui::Color32::from_rgb(52, 211, 153),
                                        ),
                                        "STARTED" => (
                                            "▶ 进行中",
                                            egui::Color32::from_rgba_premultiplied(56, 189, 248, 30),
                                            egui::Color32::from_rgb(56, 189, 248),
                                            egui::Color32::from_rgb(56, 189, 248),
                                        ),
                                        "STOPPED" => (
                                            "⏹ 已终止",
                                            egui::Color32::from_rgba_premultiplied(245, 158, 11, 30),
                                            egui::Color32::from_rgb(245, 158, 11),
                                            egui::Color32::from_rgb(251, 191, 36),
                                        ),
                                        "FAILED" => (
                                            "✗ 失败",
                                            egui::Color32::from_rgba_premultiplied(244, 63, 94, 30),
                                            egui::Color32::from_rgb(244, 63, 94),
                                            egui::Color32::from_rgb(251, 113, 133),
                                        ),
                                        _ => (
                                            entry.status.as_str(),
                                            egui::Color32::from_rgba_premultiplied(100, 116, 139, 30),
                                            egui::Color32::from_rgb(100, 116, 139),
                                            egui::Color32::from_rgb(203, 213, 225),
                                        ),
                                    };

                                    egui::Frame::none()
                                        .fill(egui::Color32::from_rgb(17, 24, 39))
                                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(30, 41, 59)))
                                        .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                                        .rounding(6.0)
                                        .show(ui, |ui| {
                                            ui.horizontal_wrapped(|ui| {
                                                // Timestamp
                                                ui.label(
                                                    egui::RichText::new(&entry.timestamp)
                                                        .color(egui::Color32::from_rgb(100, 116, 139))
                                                        .monospace()
                                                        .size(10.0),
                                                );

                                                // Status Capsule Chip
                                                egui::Frame::none()
                                                    .fill(bg_col)
                                                    .stroke(egui::Stroke::new(1.0, border_col))
                                                    .inner_margin(egui::Margin::symmetric(5.0, 1.0))
                                                    .rounding(4.0)
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            egui::RichText::new(status_text)
                                                                .color(fg_col)
                                                                .strong()
                                                                .size(9.5),
                                                        );
                                                    });

                                                // Tool Name
                                                ui.label(
                                                    egui::RichText::new(&entry.tool_name)
                                                        .strong()
                                                        .monospace()
                                                        .size(11.0)
                                                        .color(egui::Color32::from_rgb(241, 245, 249)),
                                                );

                                                // Duration tag
                                                if let Some(dur) = entry.duration_ms {
                                                    ui.label(
                                                        egui::RichText::new(format!("{}ms", dur))
                                                            .color(egui::Color32::from_rgb(148, 163, 184))
                                                            .size(10.0),
                                                    );
                                                }

                                                // Summary content
                                                if !entry.summary.is_empty() {
                                                    ui.label(
                                                        egui::RichText::new(format!("- {}", entry.summary))
                                                            .color(egui::Color32::from_rgb(148, 163, 184))
                                                            .size(10.5),
                                                    );
                                                }
                                            });
                                        });
                                    ui.add_space(2.0);
                                }
                            }
                        });
                });

            ui.add_space(4.0);

            // 6. Emergency Action Area (Footer)
            if is_stopped {
                let reconnect_btn = egui::Button::new(
                    egui::RichText::new("🟢 恢复远程协助连接 (Restart Assistance)")
                        .color(egui::Color32::WHITE)
                        .strong()
                        .size(12.5),
                )
                .fill(egui::Color32::from_rgb(16, 149, 103))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(52, 211, 153)))
                .min_size(egui::vec2(ui.available_width(), 34.0));

                if ui.add(reconnect_btn).clicked() {
                    self.state.trigger_reconnect();
                }
            } else {
                let disconnect_btn = egui::Button::new(
                    egui::RichText::new("🔴 立即切断远程协助 (Emergency Disconnect)")
                        .color(egui::Color32::WHITE)
                        .strong()
                        .size(12.5),
                )
                .fill(egui::Color32::from_rgb(190, 24, 93))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(251, 113, 133)))
                .min_size(egui::vec2(ui.available_width(), 34.0));

                if ui.add_enabled(true, disconnect_btn).clicked() {
                    self.state.trigger_emergency_disconnect();
                }
            }

            // Safety notice footer
            ui.add_space(1.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("🔒 审计日志本地留存 · 零外部隐蔽通道 · 遇到异常可随时一键切断")
                        .weak()
                        .size(9.5)
                        .color(egui::Color32::from_rgb(100, 116, 139)),
                );
            });
        });
    }
}

fn load_app_icon() -> Option<egui::IconData> {
    let png_bytes = include_bytes!("../../../../media/at-pc-icon.png");
    if let Ok(img) = image::load_from_memory(png_bytes) {
        let rgba = img.into_rgba8();
        let (width, height) = rgba.dimensions();
        return Some(egui::IconData {
            rgba: rgba.into_raw(),
            width,
            height,
        });
    }
    None
}

/// Runs the native eframe desktop application for Agent.
pub fn run_agent_app(state: Arc<AgentAppState>) -> eframe::Result<()> {
    let mut builder = egui::ViewportBuilder::default()
        .with_inner_size([500.0, 620.0])
        .with_min_inner_size([420.0, 460.0])
        .with_title("at-pc 终端代理");

    if let Some(icon) = load_app_icon() {
        builder = builder.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport: builder,
        ..Default::default()
    };

    eframe::run_native(
        "at-pc 终端代理",
        native_options,
        Box::new(move |cc| {
            setup_custom_fonts(&cc.egui_ctx);
            setup_modern_theme(&cc.egui_ctx);
            Box::new(AgentApp::new(state))
        }),
    )
}
