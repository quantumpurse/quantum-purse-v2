//! Shared UI helpers: gradient background, navigation items, status display.

use eframe::egui;

use crate::types::{Status, Tab, TransactionStatus};
use crate::App;

impl App {
    pub(crate) fn draw_gradient_bg(&self, ui: &mut egui::Ui) {
        let rect = ui.clip_rect();
        let painter = ui.painter();

        // 1. Deep fill.
        painter.rect_filled(rect, 0.0, self.colors.bg);

        // 2. Ambient corner glows — softer and smaller.
        let glow_radius = rect.width().min(rect.height()) * 0.55;
        draw_soft_glow(
            painter,
            egui::pos2(
                rect.left() + rect.width() * 0.15,
                rect.top() + rect.height() * 0.20,
            ),
            glow_radius,
            egui::Color32::from_rgb(0, 255, 180),
        );
        draw_soft_glow(
            painter,
            egui::pos2(
                rect.left() + rect.width() * 0.82,
                rect.bottom() - rect.height() * 0.22,
            ),
            glow_radius * 0.9,
            egui::Color32::from_rgb(0, 200, 255),
        );

        // 3. Lattice — low-alpha dots at a 48-px grid.
        let spacing = 48.0;
        let lattice = egui::Color32::from_rgba_unmultiplied(0, 255, 180, 8);
        let mut gx = rect.left();
        while gx < rect.right() {
            let mut gy = rect.top();
            while gy < rect.bottom() {
                painter.circle_filled(egui::pos2(gx, gy), 0.7, lattice);
                gy += spacing;
            }
            gx += spacing;
        }

        // 4. Constellation nodes.
        let stars: [(f32, f32, f32, u8, bool); 22] = [
            (0.06, 0.12, 1.8, 160, false),
            (0.11, 0.19, 0.9, 80, false),
            (0.18, 0.09, 1.2, 110, false),
            (0.24, 0.16, 2.2, 180, false),
            (0.31, 0.10, 0.7, 55, true),
            (0.20, 0.28, 1.0, 85, false),
            (0.12, 0.36, 1.4, 120, false),
            (0.28, 0.40, 0.8, 60, true),
            (0.72, 0.06, 1.1, 95, false),
            (0.79, 0.14, 2.0, 170, true),
            (0.86, 0.09, 0.8, 65, false),
            (0.90, 0.22, 1.5, 130, false),
            (0.78, 0.24, 0.9, 75, false),
            (0.68, 0.34, 1.3, 110, true),
            (0.88, 0.34, 0.7, 50, false),
            (0.62, 0.74, 1.6, 140, false),
            (0.71, 0.82, 2.3, 185, true),
            (0.83, 0.87, 1.1, 90, false),
            (0.79, 0.73, 0.8, 65, false),
            (0.88, 0.78, 0.7, 55, false),
            (0.14, 0.82, 1.3, 110, true),
            (0.22, 0.88, 0.9, 75, false),
        ];
        let accent = egui::Color32::from_rgb(0, 255, 180);
        let accent2 = egui::Color32::from_rgb(0, 200, 255);
        for (xr, yr, r, alpha, is_cyan) in stars {
            let pos = egui::pos2(
                rect.left() + xr * rect.width(),
                rect.top() + yr * rect.height(),
            );
            let base = if is_cyan { accent2 } else { accent };
            let color = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha);
            painter.circle_filled(pos, r, color);
        }

        // 5. Sparse edges — imply a signature-graph / Merkle-adjacent
        // structure without drawing a full tree. Each `(a, b, alpha)`
        // connects `stars[a]` → `stars[b]`.
        let edges: [(usize, usize, u8); 8] = [
            (0, 2, 22),
            (2, 3, 28),
            (3, 6, 18),
            (8, 9, 30),
            (9, 11, 22),
            (11, 13, 20),
            (15, 16, 30),
            (20, 21, 22),
        ];
        for (a, b, alpha) in edges {
            let (xa, ya, _, _, cyan_a) = stars[a];
            let (xb, yb, _, _, _) = stars[b];
            let pa = egui::pos2(
                rect.left() + xa * rect.width(),
                rect.top() + ya * rect.height(),
            );
            let pb = egui::pos2(
                rect.left() + xb * rect.width(),
                rect.top() + yb * rect.height(),
            );
            let base = if cyan_a { accent2 } else { accent };
            let color = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha);
            painter.line_segment([pa, pb], egui::Stroke::new(0.6, color));
        }
    }

    pub(crate) fn draw_nav_item(&mut self, ui: &mut egui::Ui, tab: Tab, icon: &str, label: &str) {
        let is_active = self.active_tab == tab;

        let response =
            ui.allocate_response(egui::vec2(ui.available_width(), 36.0), egui::Sense::click());

        if response.clicked() {
            if self.active_tab != tab
                && matches!(
                    self.tx_status,
                    TransactionStatus::Success(_) | TransactionStatus::Error(_)
                )
            {
                self.tx_status = TransactionStatus::Idle;
            }
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

/// Paints a radial glow as seven concentric discs whose per-disc alpha
/// is intentionally low; blended via `Color32::from_rgba_unmultiplied`
/// they compound at the center and fade naturally at the rim. Cheaper
/// and less aggressive than the original 30-ring falloff.
fn draw_soft_glow(
    painter: &egui::Painter,
    center: egui::Pos2,
    max_radius: f32,
    base: egui::Color32,
) {
    // (scale_of_max_radius, per-disc alpha)
    const RINGS: [(f32, u8); 7] = [
        (1.00, 3),
        (0.80, 4),
        (0.62, 5),
        (0.46, 6),
        (0.32, 7),
        (0.20, 8),
        (0.10, 10),
    ];
    for (scale, alpha) in RINGS {
        let color = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha);
        painter.circle_filled(center, max_radius * scale, color);
    }
}
