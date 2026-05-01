//! Node Manager tab — one card per backend (FullNode, LightClient,
//! PublicRpc) in a 2×2 grid. The currently-active backend renders live
//! metrics from the cached `NodeStatus`; the other cards show their
//! static config so the user knows the endpoint exists and can switch
//! to it.

use ckb_node::{NodeConfig, NodeType};
use eframe::egui;

use crate::App;

/// Vertical gap between the two rows of metric tiles inside a card.
const METRIC_ROW_GAP: f32 = 10.0;

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

                // 2×2 grid of backend cards. Top row pairs the two
                // local-node options (FullNode, LightClient); bottom row
                // holds PublicRpc with a reserved slot for a future
                // backend (e.g. Fiber Node).
                ui.columns(2, |cols| {
                    self.draw_backend_card(&mut cols[0], NodeType::FullNode);
                    self.draw_backend_card(&mut cols[1], NodeType::LightClient);
                });
                ui.add_space(14.0);
                ui.columns(2, |cols| {
                    self.draw_backend_card(&mut cols[0], NodeType::PublicRpc);
                    // cols[1] reserved for a future backend.
                });
            });
        });
    }

    fn draw_backend_card(&mut self, ui: &mut egui::Ui, backend: NodeType) {
        let active = self.qp_client.config().node_type == backend;

        let (icon, title, subtitle) = match backend {
            NodeType::LightClient => (
                "\u{1F4A1}",
                "Light Node",
                "FlyClient protocol · Fast & lightweight",
            ),
            NodeType::PublicRpc => (
                "\u{1F310}",
                "Public RPC Node",
                "Remote endpoint · No local storage",
            ),
            NodeType::FullNode => (
                "\u{1F5A5}",
                "Full Node",
                "Full chain verification · Local sovereignty",
            ),
        };

        // Hover-lift animation. egui has no CSS-style transform, so we
        // achieve the 2px lift by rendering the frame at a shifted
        // position. To avoid hover-edge jitter, the lift target is
        // driven by *previous-frame* hover (looked up from `Memory`)
        // and the hit rect for next-frame hover detection covers both
        // natural and lifted positions — so the cursor stays "inside"
        // regardless of the current lift offset.
        let backend_idx: u32 = match backend {
            NodeType::FullNode => 0,
            NodeType::LightClient => 1,
            NodeType::PublicRpc => 2,
        };
        let card_id = ui.id().with("node-card").with(backend_idx);
        let prev_hovered = ui
            .ctx()
            .memory(|m| m.data.get_temp::<bool>(card_id).unwrap_or(false));
        let lift_factor = ui.ctx().animate_bool_with_time(card_id, prev_hovered, 0.2);
        let y_offset = -2.0 * lift_factor;

        // Render the frame at the shifted position by allocating a child
        // Ui whose `max_rect` is offset upward. The Frame inside paints
        // and senses hover on that rect.
        let natural_min = ui.cursor().min;
        let avail = ui.available_size_before_wrap();
        let max_rect =
            egui::Rect::from_min_size(egui::pos2(natural_min.x, natural_min.y + y_offset), avail);
        let inner = ui.scope_builder(egui::UiBuilder::new().max_rect(max_rect), |ui| {
            egui::Frame::new()
                .fill(self.colors.surface)
                .corner_radius(18.0)
                .inner_margin(egui::Margin::symmetric(22, 22))
                .show(ui, |ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        // Icon in a rounded tile — gives the emoji
                        // structural weight and lets the title/subtitle
                        // stack align cleanly to its right edge.
                        self.draw_icon_tile(ui, icon);
                        ui.add_space(12.0);
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

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            self.draw_status_pill(ui, backend, active);
                        });
                    });

                    ui.add_space(18.0);

                    // 2×2 metric grid — same shape for every backend so cards
                    // line up at equal heights across the row. Compute the
                    // four `(label, value)` cells first, then render
                    // uniformly. Backend-specific affordances (e.g. the LC's
                    // editable Synced widget) live BELOW this grid as a
                    // full-width section.
                    let metrics = self.metric_cells(backend, active);
                    ui.columns(2, |cols| {
                        self.draw_metric(&mut cols[0], metrics[0].0, metrics[0].1.clone());
                        self.draw_metric(&mut cols[1], metrics[1].0, metrics[1].1.clone());
                        cols[0].add_space(METRIC_ROW_GAP);
                        cols[1].add_space(METRIC_ROW_GAP);
                        self.draw_metric(&mut cols[0], metrics[2].0, metrics[2].1.clone());
                        self.draw_metric(&mut cols[1], metrics[3].0, metrics[3].1.clone());
                    });

                    // Sync-bar footer for both local-node backends. Always
                    // rendered (even when the backend isn't active) so the
                    // two cards in the top row stay equal height. Inactive
                    // cards render the bar at 0% with a "—" percentage.
                    // PublicRpc skips it: there's nothing wallet-side to
                    // indicate about a remote endpoint's catch-up state.
                    if backend != NodeType::PublicRpc {
                        ui.add_space(14.0);
                        self.draw_sync_section(ui, backend, active);
                    }
                })
        });

        // The Frame's response is at the *lifted* (visual) rect.
        // Compute the natural rect by translating back, then take the
        // union as the hover-detection rect. Anchoring hover detection
        // to that union keeps the cursor "inside" the card regardless
        // of the lift offset, which prevents the cursor-exits-on-lift /
        // re-enters-on-fall jitter loop.
        let lifted_rect = inner.inner.response.rect;
        let natural_rect = lifted_rect.translate(egui::vec2(0.0, -y_offset));
        let hit_rect = natural_rect.union(lifted_rect);
        let hovered_now = ui
            .ctx()
            .pointer_hover_pos()
            .map(|p| hit_rect.contains(p))
            .unwrap_or(false);

        // Persist this frame's hover state to drive next frame's animation.
        ui.ctx()
            .memory_mut(|m| m.data.insert_temp(card_id, hovered_now));

        // Cursor: PointingHand whenever the cursor is over the hit rect.
        if hovered_now {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        // Border swap. Active card always wears `border2`; an inactive
        // card wears it on hover as a "this is what selecting me would
        // look like" preview.
        let stroke = if active {
            egui::Stroke::new(1.5, self.colors.border2)
        } else if hovered_now {
            egui::Stroke::new(1.0, self.colors.border2)
        } else {
            egui::Stroke::new(1.0, self.colors.border)
        };
        ui.painter()
            .rect_stroke(lifted_rect, 18.0, stroke, egui::StrokeKind::Inside);
    }

    /// Returns the metric tiles for a card. Every backend returns 4
    /// tiles (rendered 2×2). For FullNode and LightClient the second
    /// tile is the backend's "moving target" (LOCAL TIP for FullNode
    /// = `best_known_block_number`, TIP for LightClient = chain tip
    /// from `get_tip_header`) so the user sees both the synced height
    /// and the goalpost side by side. Inactive cards fall back to a
    /// "—" placeholder, except where a purely-static value (RPC URL
    /// hostname, default port) makes sense.
    fn metric_cells(&self, backend: NodeType, active: bool) -> Vec<(&'static str, String)> {
        const DASH: &str = "—";
        match backend {
            NodeType::PublicRpc => {
                let (active_url, fallback_url);
                let url: &str = if active {
                    active_url = self.qp_client.config().rpc_url.clone();
                    &active_url
                } else {
                    fallback_url =
                        NodeConfig::default_rpc_url_for(backend, self.qp_client.network());
                    fallback_url
                };
                let block_height = if active {
                    block_height_text(self.node_status.tip_block)
                } else {
                    DASH.into()
                };
                let port = if active {
                    port_text(self.node_status.rpc_port)
                } else {
                    port_text(crate::ckb::parse_rpc_port(url))
                };
                let peers = if active {
                    peers_text(self.node_status.peer_count)
                } else {
                    DASH.into()
                };
                vec![
                    ("Block Height", block_height),
                    ("Endpoint", hostname_of(url)),
                    ("Peers", peers),
                    ("Port", port),
                ]
            }
            NodeType::LightClient | NodeType::FullNode => {
                let tip_label = if backend == NodeType::FullNode {
                    "Local Tip"
                } else {
                    "Tip"
                };
                if active {
                    vec![
                        ("Sync", synced_value(backend, &self.node_status)),
                        (tip_label, target_tip_value(backend, &self.node_status)),
                        ("Peers", peers_text(self.node_status.peer_count)),
                        ("RPC Port", port_text(self.node_status.rpc_port)),
                    ]
                } else {
                    let url = NodeConfig::default_rpc_url_for(backend, self.qp_client.network());
                    vec![
                        ("Sync", DASH.into()),
                        (tip_label, DASH.into()),
                        ("Peers", DASH.into()),
                        ("RPC Port", port_text(crate::ckb::parse_rpc_port(url))),
                    ]
                }
            }
        }
    }

    /// Renders the backend's emoji inside a fixed-size rounded tile
    /// filled with `surface2`, matching the mockup's `.node-type-icon`.
    /// Painted manually (rather than via a Frame) for precise size and
    /// pixel-centered glyph placement.
    fn draw_icon_tile(&self, ui: &mut egui::Ui, icon: &str) {
        const ICON_BOX: f32 = 44.0;
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ICON_BOX, ICON_BOX), egui::Sense::hover());
        ui.painter().rect_filled(rect, 12.0, self.colors.surface2);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icon,
            egui::FontId::proportional(22.0),
            self.colors.text,
        );
    }

    fn draw_status_pill(&self, ui: &mut egui::Ui, backend: NodeType, active: bool) {
        // Only the active backend has live status. Inactive cards show a
        // neutral "STANDBY" pill so they don't fake data.
        let (text, bg, fg) = if !active {
            (
                "\u{25CB} STANDBY",
                self.colors.surface2,
                self.colors.text_muted,
            )
        } else if self.node_status.online {
            (
                "\u{25CF} ONLINE",
                self.colors.accent_tint,
                self.colors.accent,
            )
        } else if backend != NodeType::PublicRpc && self.local_node.has_local_process() {
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
                        .size(10.0)
                        .family(egui::FontFamily::Monospace)
                        .strong()
                        .color(fg),
                );
            });
    }

    /// Footer "Sync" section for both local-node cards (FullNode +
    /// LightClient). Always rendered — even when the backend isn't
    /// active — so both cards stay equal height across the top row.
    ///
    /// Progress source differs by backend:
    /// - LightClient: `synced_block / tip_block`. LC has no IBD
    ///   concept — sync is per-script, so we report the min cursor
    ///   across registered scripts against the chain tip.
    /// - FullNode: `tip_number / best_known_block_number` from the
    ///   `sync_state` RPC.
    ///
    /// Inactive cards stay at 0% / "—". The bar is painted by hand
    /// (4px) instead of `egui::ProgressBar` to keep the footer thin.
    ///
    /// LightClient adds a pencil at the end — clicking it swaps the
    /// row in-place for the rescan editor (input + Set / Cancel /
    /// Auto). FullNode and inactive cards omit the pencil.
    fn draw_sync_section(&mut self, ui: &mut egui::Ui, backend: NodeType, active: bool) {
        let muted = self.colors.text_muted;
        let accent = self.colors.accent;
        let tip = self.node_status.tip_block;
        let synced = self.node_status.synced_block;

        // Per-backend progress. Inactive cards (and PublicRpc) stay at
        // 0% / "—".
        let (pct, percent_text): (f32, String) = if !active || backend == NodeType::PublicRpc {
            (0.0, "—".to_string())
        } else {
            match backend {
                NodeType::LightClient => match (synced, tip) {
                    (Some(s), Some(t)) if t > 0 => {
                        let p = (s as f64 / t as f64).clamp(0.0, 1.0) as f32;
                        (p, format!("{:.1}%", p * 100.0))
                    }
                    _ => (0.0, "—".to_string()),
                },
                NodeType::FullNode => full_node_sync_view(self.node_status.sync_state.as_ref()),
                NodeType::PublicRpc => unreachable!(),
            }
        };

        // Pencil only on the active LC — editing the sync cursor on a
        // non-running backend is a no-op and would be misleading.
        let show_pencil = backend == NodeType::LightClient && active;

        // The `set_block_editing` flag lives on `App` (one per process),
        // but `draw_sync_section` is called for every local-node card.
        // Gate the edit UI on the same predicate as the pencil so toggling
        // edit mode from the LC card doesn't simultaneously render the
        // editor on the Full Node card sitting next to it.
        let in_edit_mode = show_pencil && self.set_block_editing;

        if !in_edit_mode {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Sync")
                        .size(11.0)
                        .family(egui::FontFamily::Monospace)
                        .color(muted),
                );

                // Reserve trailing room for percentage (+ pencil on LC)
                // before the bar so it stretches between label and trailing
                // controls without wrapping.
                let trailing_reserve = if show_pencil { 70.0 } else { 50.0 };
                let bar_width = (ui.available_width() - trailing_reserve).max(40.0);
                let bar_height = 4.0;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(bar_width, bar_height), egui::Sense::hover());
                let radius = bar_height * 0.5;
                let painter = ui.painter();
                painter.rect_filled(rect, radius, self.colors.surface2);
                let fill_w = rect.width() * pct;
                if fill_w > 0.0 {
                    // Gradient fill (`accent` → `accent2`) over the
                    // filled portion. Painted as a 2-triangle mesh
                    // because `Painter::rect_filled` only takes a
                    // single colour.
                    let fill_rect =
                        egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, rect.height()));
                    let right_color = lerp_color(accent, self.colors.accent2, pct);
                    let mut mesh = egui::Mesh::default();
                    mesh.colored_vertex(fill_rect.left_top(), accent);
                    mesh.colored_vertex(fill_rect.right_top(), right_color);
                    mesh.colored_vertex(fill_rect.right_bottom(), right_color);
                    mesh.colored_vertex(fill_rect.left_bottom(), accent);
                    mesh.add_triangle(0, 1, 2);
                    mesh.add_triangle(0, 2, 3);
                    painter.add(egui::Shape::mesh(mesh));
                }

                ui.label(
                    egui::RichText::new(percent_text)
                        .size(11.0)
                        .family(egui::FontFamily::Monospace)
                        .color(self.colors.text),
                );

                if show_pencil {
                    let pencil =
                        egui::Label::new(egui::RichText::new("\u{270E}").size(12.0).color(muted))
                            .sense(egui::Sense::click());
                    let resp = ui
                        .add(pencil)
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if resp.clicked() {
                        self.set_block_editing = true;
                        self.set_block_input = synced.map(|b| b.to_string()).unwrap_or_default();
                    }
                }
            });
        } else {
            // Edit mode: input + Set / Cancel / Auto, replacing the bar.
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.set_block_input)
                        .desired_width(120.0)
                        .font(egui::FontId::monospace(13.0))
                        .text_color(self.colors.text_muted),
                );

                // Validate: numeric and ≤ known tip (when tip is known).
                let parsed = self.set_block_input.trim().replace(',', "").parse::<u64>();
                let valid = matches!(&parsed, Ok(b) if tip.map_or(true, |t| *b <= t));

                let set_clicked = ui.add_enabled(valid, egui::Button::new("Set")).clicked();
                let cancel_clicked = ui.button("Cancel").clicked();
                // Auto-detect via a one-shot FullNodeClient against the
                // network's public endpoint. Disabled while a detection
                // is in flight or there are no accounts to look up.
                let auto_enabled =
                    self.earliest_funding_block_rx.is_none() && !self.accounts.is_empty();
                let auto_label = if self.earliest_funding_block_rx.is_some() {
                    "Auto…"
                } else {
                    "Auto"
                };
                let auto_clicked = ui
                    .add_enabled(auto_enabled, egui::Button::new(auto_label))
                    .clicked();
                let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));

                if set_clicked {
                    if let Ok(block) = parsed {
                        self.set_all_accounts_lock_script_block(block);
                        self.set_block_editing = false;
                        self.set_block_input.clear();
                    }
                } else if cancel_clicked || escape {
                    self.set_block_editing = false;
                    self.set_block_input.clear();
                } else if auto_clicked {
                    self.detect_earliest_funding_block_async();
                }
            });
        }
    }

    /// Renders one metric as a rounded tile spanning its column. The
    /// tile is filled with `surface2` — brighter than the card's
    /// `surface` fill — so each stat reads as a small panel sitting on
    /// top of the card, not carved into it. Inner padding is asymmetric
    /// — wider horizontally, tighter vertically — to keep the tile
    /// readable at the column widths the 2×2 grid produces.
    fn draw_metric(&self, ui: &mut egui::Ui, label: &str, value: String) {
        egui::Frame::new()
            .fill(self.colors.surface2)
            .corner_radius(10.0)
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(label.to_uppercase())
                            .size(9.0)
                            .family(egui::FontFamily::Monospace)
                            .color(self.colors.text_muted),
                    );
                    ui.add_space(3.0);
                    ui.label(
                        egui::RichText::new(value)
                            .size(12.5)
                            .strong()
                            .family(egui::FontFamily::Monospace)
                            .color(self.colors.text),
                    );
                });
            });
    }
}

/// Computes the Full Node sync bar state from a `sync_state` RPC
/// reading: `(pct, percent_text)` driving the painted bar and its
/// trailing percentage. `None` (no reading yet) maps to the same
/// placeholder an inactive card would show.
fn full_node_sync_view(sync_state: Option<&ckb_jsonrpc_types::SyncState>) -> (f32, String) {
    let Some(s) = sync_state else {
        return (0.0, "—".to_string());
    };
    let tip = s.tip_number.value();
    let best = s.best_known_block_number.value();

    if best > 0 {
        let p = (tip as f64 / best as f64).clamp(0.0, 1.0) as f32;
        (p, format!("{:.1}%", p * 100.0))
    } else {
        (0.0, "—".to_string())
    }
}

/// Renders the "Sync" tile value — the **synced/validated** height
/// (left side of the previous `X / Y` format).
/// FullNode → `tip_number` from sync_state. LightClient → min cursor
/// across registered scripts.
fn synced_value(backend: NodeType, status: &crate::types::NodeStatus) -> String {
    match backend {
        NodeType::FullNode => match status.sync_state.as_ref() {
            Some(s) => format!("#{}", format_int(s.tip_number.value())),
            None => "—".into(),
        },
        NodeType::LightClient => match status.synced_block {
            Some(s) => format!("#{}", format_int(s)),
            None => "—".into(),
        },
        NodeType::PublicRpc => "—".into(),
    }
}

/// Renders the "Local Tip" / "Tip" tile value — the backend's
/// **moving target** (right side of the previous `X / Y` format).
/// FullNode → `best_known_block_number` (the local node's view of
/// the network's best chain head, advances during header sync as new
/// headers stream in). LightClient → `tip_block` from
/// `get_tip_header` (the chain tip the LC is talking to).
fn target_tip_value(backend: NodeType, status: &crate::types::NodeStatus) -> String {
    match backend {
        NodeType::FullNode => match status.sync_state.as_ref() {
            Some(s) => format!("#{}", format_int(s.best_known_block_number.value())),
            None => "—".into(),
        },
        NodeType::LightClient => match status.tip_block {
            Some(t) => format!("#{}", format_int(t)),
            None => "—".into(),
        },
        NodeType::PublicRpc => "—".into(),
    }
}

fn block_height_text(tip: Option<u64>) -> String {
    tip.map(|n| format!("#{}", format_int(n)))
        .unwrap_or_else(|| "—".to_string())
}

fn peers_text(count: Option<usize>) -> String {
    count
        .map(|n| format!("{} connected", n))
        .unwrap_or_else(|| "—".to_string())
}

fn port_text(port: Option<u16>) -> String {
    port.map(|p| p.to_string())
        .unwrap_or_else(|| "—".to_string())
}

/// Strips scheme + path to return just the hostname of an RPC URL.
fn hostname_of(url: &str) -> String {
    let stripped = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    stripped
        .split('/')
        .next()
        .unwrap_or(stripped)
        .split(':')
        .next()
        .unwrap_or(stripped)
        .to_string()
}

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

/// Linearly interpolates between two RGBA colours at fraction `t`
/// (clamped to `[0, 1]`). Used for the sync-bar gradient endpoint so
/// the fill ranges smoothly from `accent` at `pct = 0` to `accent2` at
/// `pct = 1`.
fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round() as u8;
    egui::Color32::from_rgba_unmultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}
