//! Node Manager tab — status card for the currently-active backend.

use eframe::egui;
use node_manager::NodeType;

use crate::App;

impl App {
    pub(crate) fn show_node_manager_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 30.0);

                ui.heading(
                    egui::RichText::new("Node Manager")
                        .size(26.0)
                        .strong()
                        .color(self.colors.text),
                );
                ui.label(
                    egui::RichText::new("Configure and monitor your CKB node")
                        .size(13.0)
                        .color(self.colors.text_muted),
                );

                ui.add_space(22.0);

                self.draw_node_card(ui);
            });
        });
    }

    fn draw_node_card(&self, ui: &mut egui::Ui) {
        let (icon, title, subtitle) = match self.node_config.node_type {
            NodeType::LightClient => (
                "\u{1F4A1}",
                "Light Node",
                "Header-only sync · Fast & lightweight",
            ),
            NodeType::FullNode => (
                "\u{1F5A5}",
                "Full Node",
                "Full chain verification · Local sovereignty",
            ),
            NodeType::PublicRpc => (
                "\u{1F310}",
                "Public RPC Node",
                "Remote endpoint · No local storage",
            ),
        };

        egui::Frame::new()
            .fill(self.colors.surface)
            .corner_radius(18.0)
            .inner_margin(egui::Margin::symmetric(22, 22))
            .stroke(egui::Stroke::new(1.0, self.colors.border))
            .show(ui, |ui| {
                // Header row: icon/title on the left, status pill on the right.
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icon).size(28.0));
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(title)
                                .size(17.0)
                                .strong()
                                .color(self.colors.text),
                        );
                        ui.label(
                            egui::RichText::new(subtitle)
                                .size(11.0)
                                .color(self.colors.text_muted),
                        );
                    });

                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            self.draw_status_pill(ui);
                        },
                    );
                });

                ui.add_space(18.0);

                // Metric grid — four columns with evenly-sized tiles.
                ui.columns(4, |cols| {
                    self.draw_metric(&mut cols[0], "Block Height", self.block_height_text());
                    self.draw_metric(&mut cols[1], "Peers", self.peers_text());
                    self.draw_metric(&mut cols[2], "RPC Port", self.rpc_port_text());
                    self.draw_metric(&mut cols[3], "DB Size", self.db_size_text());
                });
            });
    }

    fn draw_status_pill(&self, ui: &mut egui::Ui) {
        let (text, bg, fg) = if self.node_status.online {
            ("\u{25CF} ONLINE", self.colors.accent_tint, self.colors.accent)
        } else if self.node_config.node_type != NodeType::PublicRpc
            && self.node_process.is_some()
        {
            ("\u{25CC} STARTING", self.colors.warn_tint, self.colors.warn)
        } else {
            (
                "\u{25CB} OFFLINE",
                egui::Color32::from_rgba_unmultiplied(255, 77, 109, 30),
                self.colors.danger,
            )
        };

        egui::Frame::new()
            .fill(bg)
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(10, 4))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(text)
                        .size(10.5)
                        .family(egui::FontFamily::Monospace)
                        .color(fg),
                );
            });
    }

    fn draw_metric(&self, ui: &mut egui::Ui, label: &str, value: String) {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(label)
                    .size(10.0)
                    .family(egui::FontFamily::Monospace)
                    .color(self.colors.text_muted),
            );
            ui.add_space(3.0);
            ui.label(
                egui::RichText::new(value)
                    .size(15.0)
                    .strong()
                    .color(self.colors.text),
            );
        });
    }

    fn block_height_text(&self) -> String {
        self.node_status
            .tip_block
            .map(|n| format!("#{}", format_int(n)))
            .unwrap_or_else(|| "—".to_string())
    }

    fn peers_text(&self) -> String {
        match self.node_status.peer_count {
            Some(n) => format!("{} connected", n),
            None if self.node_config.node_type == NodeType::PublicRpc => "—".to_string(),
            None => "—".to_string(),
        }
    }

    fn rpc_port_text(&self) -> String {
        self.node_status
            .rpc_port
            .map(|p| p.to_string())
            .unwrap_or_else(|| "—".to_string())
    }

    fn db_size_text(&self) -> String {
        match self.node_status.db_size_bytes {
            Some(bytes) => format_bytes(bytes),
            None if self.node_config.node_type == NodeType::PublicRpc => "—".to_string(),
            None => "—".to_string(),
        }
    }
}

/// Formats an integer with thousands separators, e.g. `14298441` →
/// `"14,298,441"`.
fn format_int(n: u64) -> String {
    let raw = n.to_string();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    let chars: Vec<char> = raw.chars().collect();
    for (i, ch) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*ch);
    }
    out
}

/// Formats a byte count into KB/MB/GB with one decimal place.
fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}
