//! Shared UI helpers: gradient background, navigation items, status display.

use eframe::egui;

use crate::types::{Status, Tab};
use crate::App;

impl App {
    /// Draw a gradient background effect.
    pub(crate) fn draw_gradient_bg(&self, ui: &mut egui::Ui) {
        let rect = ui.clip_rect();
        let painter = ui.painter();

        // Subtle gradient overlay
        painter.rect_filled(rect, 0.0, self.colors.bg);

        // Add subtle glow effects at corners
        let glow1_center = rect.left_top() + egui::vec2(rect.width() * 0.12, rect.height() * 0.18);
        let glow2_center =
            rect.right_bottom() - egui::vec2(rect.width() * 0.12, rect.height() * 0.22);

        // Draw gradient circles
        for i in (0..30).rev() {
            let alpha = (1.0 - (i as f32 / 30.0)).powi(2) * 0.05;
            let radius = rect.width().min(rect.height()) * 0.4 * (i as f32 / 30.0);

            painter.circle_filled(
                glow1_center,
                radius,
                egui::Color32::from_rgba_unmultiplied(0, 255, 180, (alpha * 255.0) as u8),
            );

            painter.circle_filled(
                glow2_center,
                radius,
                egui::Color32::from_rgba_unmultiplied(0, 200, 255, (alpha * 255.0) as u8),
            );
        }
    }

    pub(crate) fn draw_nav_item(&mut self, ui: &mut egui::Ui, tab: Tab, icon: &str, label: &str) {
        let is_active = self.active_tab == tab;

        let response =
            ui.allocate_response(egui::vec2(ui.available_width(), 36.0), egui::Sense::click());

        if response.clicked() {
            self.active_tab = tab;
        }

        let rect = response.rect;
        let painter = ui.painter();

        // Inset rect for rounded background (matching mockup .nav-item padding)
        let inner = egui::Rect::from_min_size(
            rect.min + egui::vec2(10.0, 0.0),
            egui::vec2(rect.width() - 20.0, rect.height()),
        );

        if is_active {
            painter.rect_filled(
                inner,
                9.0,
                egui::Color32::from_rgba_unmultiplied(0, 255, 180, 26),
            );
        } else if response.hovered() {
            painter.rect_filled(
                inner,
                9.0,
                egui::Color32::from_rgba_unmultiplied(0, 255, 180, 15),
            );
        }

        let text_color = if is_active {
            self.colors.accent
        } else if response.hovered() {
            self.colors.text
        } else {
            self.colors.text_muted
        };

        // Icon
        painter.text(
            inner.left_center() + egui::vec2(14.0, 0.0),
            egui::Align2::LEFT_CENTER,
            icon,
            egui::FontId::proportional(15.0),
            text_color,
        );

        // Label
        painter.text(
            inner.left_center() + egui::vec2(34.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            text_color,
        );
    }

    pub(crate) fn show_status(&self, ui: &mut egui::Ui) {
        match &self.status {
            Status::None => {}
            Status::Info(msg) => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("\u{2139}\u{fe0f}").color(self.colors.accent2));
                    ui.label(egui::RichText::new(msg).color(self.colors.accent2));
                });
            }
            Status::Error(msg) => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("\u{274c}").color(self.colors.danger));
                    ui.label(egui::RichText::new(msg).color(self.colors.danger));
                });
            }
        }
    }
}
