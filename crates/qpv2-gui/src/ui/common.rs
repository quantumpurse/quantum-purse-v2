//! Shared UI helpers: gradient background, navigation items, status display.

use eframe::egui;

use crate::types::{AppColors, Status, Tab, TransactionStatus};
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
            egui::Color32::from_rgb(255, 160, 30),
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

    pub(crate) fn draw_gradient_bg(&self, ui: &mut egui::Ui, animate: bool) {
        let rect = ui.clip_rect();
        let painter = ui.painter();
        let t = if animate {
            ui.input(|i| i.time) as f32
        } else {
            0.0
        };

        // 1. Deep fill.
        painter.rect_filled(rect, 0.0, self.colors.bg);

        // 2. Ambient corner glows — slow orbital drift when animated.
        let glow_radius = rect.width().min(rect.height()) * 0.55;
        let glow1_x = 0.15 + 0.03 * (t * 0.2).sin();
        let glow1_y = 0.20 + 0.03 * (t * 0.15).cos();
        draw_soft_glow(
            painter,
            egui::pos2(
                rect.left() + rect.width() * glow1_x,
                rect.top() + rect.height() * glow1_y,
            ),
            glow_radius,
            egui::Color32::from_rgb(0, 255, 180),
        );
        let glow2_x = 0.82 + 0.03 * (t * 0.17).cos();
        let glow2_y = 0.78 + 0.03 * (t * 0.22).sin();
        draw_soft_glow(
            painter,
            egui::pos2(
                rect.left() + rect.width() * glow2_x,
                rect.top() + rect.height() * glow2_y,
            ),
            glow_radius * 0.9,
            egui::Color32::from_rgb(255, 160, 30),
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

        // 4. Constellation nodes — each star twinkles at its own phase.
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
        for (i, (xr, yr, r, alpha, is_cyan)) in stars.iter().enumerate() {
            let phase = i as f32 * 1.7;
            let twinkle = if animate {
                0.6 + 0.4 * (t * 0.8 + phase).sin()
            } else {
                1.0
            };
            let a = (*alpha as f32 * twinkle).clamp(0.0, 255.0) as u8;
            let pos = egui::pos2(
                rect.left() + xr * rect.width(),
                rect.top() + yr * rect.height(),
            );
            let base = if *is_cyan { accent2 } else { accent };
            let color = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), a);
            painter.circle_filled(pos, *r, color);
        }

        // 5. Sparse edges — alpha follows their connected stars.
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
            let twinkle = if animate {
                let phase_a = a as f32 * 1.7;
                let phase_b = b as f32 * 1.7;
                0.5 * ((t * 0.8 + phase_a).sin() + (t * 0.8 + phase_b).sin()) * 0.5 + 0.5
            } else {
                1.0
            };
            let a_mod = (alpha as f32 * twinkle).clamp(0.0, 255.0) as u8;
            let pa = egui::pos2(
                rect.left() + xa * rect.width(),
                rect.top() + ya * rect.height(),
            );
            let pb = egui::pos2(
                rect.left() + xb * rect.width(),
                rect.top() + yb * rect.height(),
            );
            let base = if cyan_a { accent2 } else { accent };
            let color =
                egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), a_mod);
            painter.line_segment([pa, pb], egui::Stroke::new(0.6, color));
        }

        if animate {
            ui.ctx().request_repaint();
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

        let inner = egui::Rect::from_min_size(
            rect.min + egui::vec2(14.0, 0.0),
            egui::vec2(rect.width() - 28.0, rect.height()),
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

    pub(crate) fn paint_entanglement_divider(&self, ui: &mut egui::Ui) {
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), egui::Sense::hover());
        let painter = ui.painter();
        let mid = rect.center().x;
        let y = rect.center().y;

        let steps = 64;
        for i in 0..steps {
            let t0 = i as f32 / steps as f32;
            let t1 = (i + 1) as f32 / steps as f32;
            let x0 = rect.left() + t0 * w;
            let x1 = rect.left() + t1 * w;

            let fade = |t: f32| -> f32 {
                let d = ((rect.left() + t * w) - mid).abs() / (w * 0.5);
                (1.0 - d).clamp(0.0, 1.0).powi(2)
            };
            let a = ((fade(t0) + fade(t1)) * 0.5 * 0.5 * 255.0) as u8;

            let c = egui::Color32::from_rgba_unmultiplied(
                self.colors.accent.r(),
                self.colors.accent.g(),
                self.colors.accent.b(),
                a,
            );
            painter.line_segment(
                [egui::pos2(x0, y), egui::pos2(x1, y)],
                egui::Stroke::new(1.0, c),
            );
        }
    }
}

pub(crate) struct CardHover {
    id: egui::Id,
    pub(crate) lift: f32,
    pub(crate) fill: egui::Color32,
    pub(crate) stroke: egui::Stroke,
}

impl CardHover {
    pub(crate) fn new(ui: &egui::Ui, tag: impl std::hash::Hash, colors: &AppColors) -> Self {
        let id = ui.id().with(tag);
        let prev_hovered = ui
            .ctx()
            .memory(|m| m.data.get_temp::<bool>(id).unwrap_or(false));
        let lift = ui.ctx().animate_bool_with_time(id, prev_hovered, 0.2);

        let accent_tint = egui::Color32::from_rgba_unmultiplied(
            colors.accent.r(),
            colors.accent.g(),
            colors.accent.b(),
            10,
        );

        let fill = if prev_hovered {
            accent_tint
        } else {
            colors.surface
        };
        let stroke = if prev_hovered {
            egui::Stroke::new(1.0, colors.border2)
        } else {
            egui::Stroke::new(1.0, colors.border)
        };

        Self {
            id,
            lift,
            fill,
            stroke,
        }
    }

    pub(crate) fn apply_lift(&self, ui: &mut egui::Ui) {
        ui.add_space(-3.0 * self.lift);
    }

    pub(crate) fn commit(&self, response: &egui::Response) {
        let hovered = response.hovered();
        response
            .ctx
            .memory_mut(|m| m.data.insert_temp(self.id, hovered));
        if hovered {
            response.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
}

/// Paints a quarter-circle accent in the top-right corner of `rect`,
/// respecting the card's rounded corner. The shape's boundary follows
/// the card's corner arc where they overlap, so no fill bleeds past
/// the rounded edge.
pub(crate) fn paint_corner_accent(
    painter: &egui::Painter,
    rect: egui::Rect,
    corner_radius: f32,
    color: egui::Color32,
) {
    const SIZE: f32 = 70.0;
    const ARC_SEGS: usize = 16;

    let tinted = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 18);

    let mut perimeter: Vec<egui::Pos2> = Vec::new();

    // 1. Card's TR corner arc: from (right - r, top) curving to (right, top + r).
    let arc_cx = rect.right() - corner_radius;
    let arc_cy = rect.top() + corner_radius;
    for i in 0..=ARC_SEGS {
        let t = i as f32 / ARC_SEGS as f32;
        let angle = -std::f32::consts::FRAC_PI_2 + t * std::f32::consts::FRAC_PI_2;
        perimeter.push(egui::pos2(
            arc_cx + corner_radius * angle.cos(),
            arc_cy + corner_radius * angle.sin(),
        ));
    }

    // 2. Right edge down to where the accent arc begins.
    perimeter.push(egui::pos2(rect.right(), rect.top() + SIZE));

    // 3. Accent quarter-circle arc (70px, centered at top-right corner):
    //    from (right, top + 70) curving to (right - 70, top).
    for i in 1..=ARC_SEGS {
        let t = i as f32 / ARC_SEGS as f32;
        let angle = std::f32::consts::FRAC_PI_2 + t * std::f32::consts::FRAC_PI_2;
        perimeter.push(egui::pos2(
            rect.right() + SIZE * angle.cos(),
            rect.top() + SIZE * angle.sin(),
        ));
    }

    // 4. Top edge back to start (right - 70, top) → (right - r, top)
    //    Only needed if SIZE > corner_radius (always true: 70 > 18).
    // perimeter closes back to first vertex via the fan.

    // Triangulate as fan from centroid.
    let cx: f32 = perimeter.iter().map(|p| p.x).sum::<f32>() / perimeter.len() as f32;
    let cy: f32 = perimeter.iter().map(|p| p.y).sum::<f32>() / perimeter.len() as f32;

    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(egui::pos2(cx, cy), tinted);
    for p in &perimeter {
        mesh.colored_vertex(*p, tinted);
    }
    let n = perimeter.len();
    for i in 0..n {
        mesh.add_triangle(0, (i + 1) as u32, ((i + 1) % n + 1) as u32);
    }

    painter.add(egui::Shape::mesh(mesh));
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

/// Builds a radial glow mesh whose outer perimeter traces a rounded
/// rect. Each perimeter vertex is colored based on its actual distance
/// from `center` relative to `max_radius`, so the glow fills the card
/// without dimming at the edges (unlike `clamp_mesh_to_rounded_rect`
/// which moves vertices inward but keeps their original transparent
/// color).
pub(crate) fn glow_mesh_clipped_to_rounded_rect(
    center: egui::Pos2,
    max_radius: f32,
    base: egui::Color32,
    peak_alpha: u8,
    rect: egui::Rect,
    corner_radius: f32,
) -> egui::Mesh {
    const ARC_SEGS: usize = 8;

    let mut mesh = egui::Mesh::default();

    // Center vertex at full brightness.
    let center_color =
        egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), peak_alpha);
    let center_idx = mesh.vertices.len() as u32;
    mesh.colored_vertex(center, center_color);

    // Perimeter vertices tracing the rounded rect (clockwise from TL).
    let mut perim: Vec<u32> = Vec::new();
    let corners = [
        (
            egui::pos2(rect.left() + corner_radius, rect.top() + corner_radius),
            std::f32::consts::PI,
        ),
        (
            egui::pos2(rect.right() - corner_radius, rect.top() + corner_radius),
            1.5 * std::f32::consts::PI,
        ),
        (
            egui::pos2(rect.right() - corner_radius, rect.bottom() - corner_radius),
            0.0,
        ),
        (
            egui::pos2(rect.left() + corner_radius, rect.bottom() - corner_radius),
            0.5 * std::f32::consts::PI,
        ),
    ];
    for (arc_center, start_angle) in &corners {
        for i in 0..=ARC_SEGS {
            let t = i as f32 / ARC_SEGS as f32;
            let angle = start_angle + t * std::f32::consts::FRAC_PI_2;
            let p = egui::pos2(
                arc_center.x + corner_radius * angle.cos(),
                arc_center.y + corner_radius * angle.sin(),
            );
            // Color based on radial distance from glow center.
            let dist = center.distance(p);
            let falloff = (1.0 - (dist / max_radius).clamp(0.0, 1.0)).powi(2);
            let alpha = (peak_alpha as f32 * falloff).round() as u8;
            let color = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha);
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
