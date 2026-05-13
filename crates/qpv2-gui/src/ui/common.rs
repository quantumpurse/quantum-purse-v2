//! Shared UI helpers: gradient background, navigation items, status display.

use eframe::egui;

use crate::types::{Status, Tab, TransactionStatus};
use crate::App;

impl App {
    /// Mockup-faithful background for the Unlocked screen: solid
    /// `colors.bg` plus three soft radial tints (accent / accent2 /
    /// accent3) — matches `body::before` in `mockup-ui.html`. Stripped
    /// of the lattice, constellation, and edge graph used by the lock
    /// screen so the dashboard reads as a calm dark canvas instead of
    /// a busy puzzle.
    pub(crate) fn draw_unlocked_bg(&self, ui: &mut egui::Ui) {
        let rect = ui.clip_rect();
        let painter = ui.painter();

        // Solid base.
        painter.rect_filled(rect, 0.0, self.colors.bg);

        // Three soft radials. Positions and relative sizes mirror the
        // mockup's CSS `body::before`:
        //   12% / 18% — accent  (top-left)
        //   88% / 78% — accent2 (bottom-right)
        //   50% / 50% — accent3 (center, smaller)
        //
        // The mockup's `radial-gradient(... 0%, transparent 60%)`
        // truncates each gradient at 60% of its ellipse extent —
        // roughly 30% of the viewport's smallest dimension. Our
        // earlier 0.50 / 0.35 spread the tails into the center and
        // across each other, lifting the average bg tint to ~10% and
        // flattening contrast against the hero. 0.30 / 0.20 keep the
        // glows tucked at their anchors so most of the bg stays the
        // solid `colors.bg` the mockup intends.
        let corner_radius = rect.width().min(rect.height()) * 0.30;
        let center_radius = rect.width().min(rect.height()) * 0.20;

        // Peak alphas mirror the mockup's CSS exactly — 5%, 5%, 3%.
        // Earlier values (26/26/16) were almost 2× the spec, lifting
        // the bg close to the hero's brightness and flattening the
        // contrast. The hero is the focal point; the bg is supposed
        // to be barely-there atmosphere.
        draw_smooth_glow(
            painter,
            egui::pos2(
                rect.left() + rect.width() * 0.12,
                rect.top() + rect.height() * 0.18,
            ),
            corner_radius,
            self.colors.accent,
            13,
        );
        draw_smooth_glow(
            painter,
            egui::pos2(
                rect.left() + rect.width() * 0.88,
                rect.bottom() - rect.height() * 0.22,
            ),
            corner_radius,
            self.colors.accent2,
            13,
        );
        draw_smooth_glow(
            painter,
            egui::pos2(
                rect.left() + rect.width() * 0.5,
                rect.top() + rect.height() * 0.5,
            ),
            center_radius,
            self.colors.accent3,
            8,
        );
    }

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

/// Builds a radial-glow triangle-fan mesh (center at `peak_alpha`, 48
/// transparent edge vertices on a circle of `max_radius`) and returns
/// it. Callers either pass it to `Painter::add` directly via
/// [`draw_smooth_glow`], or stash it in a reserved shape index via
/// `Painter::set` when the destination rect isn't known until after
/// content layout completes (see the dashboard hero, where the card's
/// final size is only known after `Frame::show` returns).
///
/// 48 segments give a smooth circle outline; egui's per-triangle
/// linear color interpolation does the rest, producing a continuous
/// alpha falloff with no banding rings.
pub(crate) fn smooth_glow_mesh(
    center: egui::Pos2,
    max_radius: f32,
    base: egui::Color32,
    peak_alpha: u8,
) -> egui::Mesh {
    const SEGMENTS: usize = 48;

    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(
        center,
        egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), peak_alpha),
    );
    let edge = egui::Color32::TRANSPARENT;
    for i in 0..SEGMENTS {
        let theta = (i as f32 / SEGMENTS as f32) * std::f32::consts::TAU;
        mesh.colored_vertex(
            egui::pos2(
                center.x + max_radius * theta.cos(),
                center.y + max_radius * theta.sin(),
            ),
            edge,
        );
    }
    for i in 0..SEGMENTS {
        let v1 = (i + 1) as u32;
        let v2 = ((i + 1) % SEGMENTS + 1) as u32;
        mesh.add_triangle(0, v1, v2);
    }
    mesh
}

/// Convenience wrapper: build the mesh and add it to the painter
/// immediately. For the unlocked-bg radials and any other call site
/// where the rect is known before painting.
pub(crate) fn draw_smooth_glow(
    painter: &egui::Painter,
    center: egui::Pos2,
    max_radius: f32,
    base: egui::Color32,
    peak_alpha: u8,
) {
    painter.add(egui::Shape::mesh(smooth_glow_mesh(
        center, max_radius, base, peak_alpha,
    )));
}

/// Builds a rounded-rectangle mesh with bilinear color interpolation
/// across `(tl, tr, br, bl)`. Use to paint a gradient that *follows*
/// the rounded outline — egui can't clip a regular mesh to rounded
/// corners, so a rectangular gradient mesh leaks past the curve into
/// the panel bg. This mesh's perimeter traces the actual rounded
/// shape (8 segments per corner arc), so there's nothing to leak.
///
/// Triangulation: a single fan from the center vertex out to each
/// consecutive pair of perimeter vertices. Per-vertex color comes
/// from bilinear interpolation across the bounding rect.
pub(crate) fn rounded_rect_gradient_mesh(
    rect: egui::Rect,
    radius: f32,
    tl: egui::Color32,
    tr: egui::Color32,
    br: egui::Color32,
    bl: egui::Color32,
) -> egui::Mesh {
    const ARC_SEGS: usize = 8;

    let mut mesh = egui::Mesh::default();

    // Center vertex — averaged corner color.
    let center = rect.center();
    let center_color = avg4(tl, tr, br, bl);
    let center_idx = mesh.vertices.len() as u32;
    mesh.colored_vertex(center, center_color);

    // Perimeter vertices, clockwise. Each corner arc sweeps a
    // quarter circle around its inset center; arc start angles are
    // chosen so vertices land on the rounded outline going clockwise
    // from the TL corner.
    let mut perim: Vec<u32> = Vec::new();
    let corners = [
        (
            egui::pos2(rect.left() + radius, rect.top() + radius),
            std::f32::consts::PI,
        ),
        (
            egui::pos2(rect.right() - radius, rect.top() + radius),
            1.5 * std::f32::consts::PI,
        ),
        (
            egui::pos2(rect.right() - radius, rect.bottom() - radius),
            0.0,
        ),
        (
            egui::pos2(rect.left() + radius, rect.bottom() - radius),
            0.5 * std::f32::consts::PI,
        ),
    ];
    for (arc_center, start_angle) in &corners {
        for i in 0..=ARC_SEGS {
            let t = i as f32 / ARC_SEGS as f32;
            let angle = start_angle + t * std::f32::consts::FRAC_PI_2;
            let p = egui::pos2(
                arc_center.x + radius * angle.cos(),
                arc_center.y + radius * angle.sin(),
            );
            let color = bilinear(p, rect, tl, tr, br, bl);
            let idx = mesh.vertices.len() as u32;
            mesh.colored_vertex(p, color);
            perim.push(idx);
        }
    }

    // Fan triangles.
    let n = perim.len();
    for i in 0..n {
        let v1 = perim[i];
        let v2 = perim[(i + 1) % n];
        mesh.add_triangle(center_idx, v1, v2);
    }

    mesh
}

fn avg4(a: egui::Color32, b: egui::Color32, c: egui::Color32, d: egui::Color32) -> egui::Color32 {
    let avg = |w: u8, x: u8, y: u8, z: u8| ((w as u16 + x as u16 + y as u16 + z as u16) / 4) as u8;
    egui::Color32::from_rgba_unmultiplied(
        avg(a.r(), b.r(), c.r(), d.r()),
        avg(a.g(), b.g(), c.g(), d.g()),
        avg(a.b(), b.b(), c.b(), d.b()),
        avg(a.a(), b.a(), c.a(), d.a()),
    )
}

fn bilinear(
    p: egui::Pos2,
    rect: egui::Rect,
    tl: egui::Color32,
    tr: egui::Color32,
    br: egui::Color32,
    bl: egui::Color32,
) -> egui::Color32 {
    let u = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    let v = ((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
    let top = lerp_color(tl, tr, u);
    let bot = lerp_color(bl, br, u);
    lerp_color(top, bot, v)
}

/// Clamps every vertex in `mesh` to lie inside a rounded rect.
/// Vertices outside the bounding rect are clamped to its edge;
/// vertices in a corner region beyond the arc are projected onto the
/// arc. Call this on circular glow meshes before painting to prevent
/// leakage past rounded card corners.
pub(crate) fn clamp_mesh_to_rounded_rect(mesh: &mut egui::Mesh, rect: egui::Rect, radius: f32) {
    for vertex in &mut mesh.vertices {
        vertex.pos = clamp_to_rounded_rect(vertex.pos, rect, radius);
    }
}

fn clamp_to_rounded_rect(p: egui::Pos2, rect: egui::Rect, radius: f32) -> egui::Pos2 {
    let x = p.x.clamp(rect.left(), rect.right());
    let y = p.y.clamp(rect.top(), rect.bottom());

    let corners = [
        (rect.left() + radius, rect.top() + radius),
        (rect.right() - radius, rect.top() + radius),
        (rect.right() - radius, rect.bottom() - radius),
        (rect.left() + radius, rect.bottom() - radius),
    ];

    for &(cx, cy) in &corners {
        let in_corner_x = if cx <= rect.center().x {
            x < cx
        } else {
            x > cx
        };
        let in_corner_y = if cy <= rect.center().y {
            y < cy
        } else {
            y > cy
        };

        if in_corner_x && in_corner_y {
            let dx = x - cx;
            let dy = y - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > radius {
                return egui::pos2(cx + dx * radius / dist, cy + dy * radius / dist);
            }
        }
    }

    egui::pos2(x, y)
}

/// Linearly interpolates between two RGBA colours at fraction `t`
/// (clamped to `[0, 1]`). Used by the gradient mesh builders here and
/// by the node-manager sync-bar gradient.
pub(crate) fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round() as u8;
    egui::Color32::from_rgba_unmultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
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
