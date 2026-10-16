use gpui::{
    AnyElement, App, BorderStyle, Bounds, FontWeight, Hsla, IntoElement, ParentElement as _,
    PathBuilder, Pixels, Point, ShapedLine, SharedString, Styled as _, TextAlign, TextRun, Window,
    div, point, px, quad, size,
};

use std::{cmp::Ordering, collections::HashSet};

use super::visual::Vertex;
use super::{Item, NetworkGraph};
use crate::geometry::{circles_hull, draw_self_loop, segment_may_intersect, smooth_closed};
use crate::style::{GraphColors, GraphMetrics, LabelMode};

#[derive(Clone, Copy, PartialEq)]
enum EdgeState {
    Normal = 0,
    Dimmed = 1,
    Active = 2,
    Selected = 3,
}

impl NetworkGraph {
    fn vertex_color(&self, v: usize, cx: &App) -> Hsla {
        match self.vis.vertices[v] {
            Vertex::Node(i) => self.color_of(&self.nodes[i], cx),
            Vertex::Group(g) => self.group_color(g),
        }
    }

    pub(super) fn paint_graph(
        &self,
        bounds: Bounds<Pixels>,
        colors: &GraphColors,
        metrics: &GraphMetrics,
        show_focus: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let zoom = self.zoom;
        let selected = self.selected_item();
        let focus = self.hovered.or(selected);
        let vis = &self.vis;
        let in_focus = |v: usize| match focus {
            None => true,
            Some(Item::Vertex(f)) => v == f || vis.neighbors[f].binary_search(&v).is_ok(),
            Some(Item::Edge(e)) => v == vis.edges[e].source || v == vis.edges[e].target,
        };
        let visible = bounds.dilate(px(40.));

        if self.group_hulls {
            self.paint_hulls(focus.is_some(), metrics, window, cx);
        }

        // Edges are batched into one path per state to keep draw calls low;
        // merged edges get their own path because their width varies.
        let stroke = (1.2 * zoom.sqrt()).clamp(0.6, 2.);
        let mut batches: Vec<(PathBuilder, PathBuilder, bool)> = [1., 1., 1.6, 2.2]
            .iter()
            .map(|w| {
                (
                    PathBuilder::stroke(px(stroke * w)),
                    PathBuilder::fill(),
                    false,
                )
            })
            .collect();
        let mut bundles: Vec<(PathBuilder, PathBuilder, EdgeState)> = Vec::new();
        let mut edge_labels = Vec::new();

        for (ei, e) in vis.edges.iter().enumerate() {
            let geom = self.edge_geometry(ei);
            let (a, b) = (geom.eval(0.), geom.eval(1.));
            if !segment_may_intersect(visible, a, b) {
                continue;
            }
            let state = if selected == Some(Item::Edge(ei)) {
                EdgeState::Selected
            } else {
                match focus {
                    None => EdgeState::Normal,
                    Some(Item::Vertex(f)) if e.source == f || e.target == f => EdgeState::Active,
                    Some(Item::Edge(x)) if x == ei => EdgeState::Active,
                    Some(_) => EdgeState::Dimmed,
                }
            };
            let count = e.edges.len();
            let width_scale = if count > 1 {
                1. + (count as f32).ln() * 0.8
            } else {
                1.
            };
            let mut own = (count > 1).then(|| {
                let w = stroke
                    * width_scale
                    * if state == EdgeState::Normal || state == EdgeState::Dimmed {
                        1.
                    } else {
                        1.4
                    };
                (PathBuilder::stroke(px(w.min(10.))), PathBuilder::fill())
            });
            let (path, arrows) = match &mut own {
                Some((p, a)) => (p, a),
                None => {
                    let (p, a, used) = &mut batches[state as usize];
                    *used = true;
                    (p, a)
                }
            };

            let rs = vis.bodies[e.source].radius * zoom;
            let rt = vis.bodies[e.target].radius * zoom;
            if e.source == e.target {
                draw_self_loop(path, a, rs);
            } else {
                let arrow = (6. + stroke * 2.) * width_scale.sqrt();
                let gap = if self.arrows { rt + arrow } else { rt };
                path.move_to(geom.point_from_start(rs));
                let end = geom.point_from_end(gap);
                match geom.ctrl {
                    Some(ctrl) => path.curve_to(end, ctrl),
                    None => path.line_to(end),
                }
                if self.arrows && zoom > 0.25 {
                    let tip = geom.point_from_end(rt);
                    let (dx, dy) = geom.end_direction();
                    let (nx, ny) = (-dy * arrow * 0.45, dx * arrow * 0.45);
                    let (bx, by) = (tip.x.as_f32() - dx * arrow, tip.y.as_f32() - dy * arrow);
                    arrows.add_polygon(
                        &[
                            tip,
                            point(px(bx + nx), px(by + ny)),
                            point(px(bx - nx), px(by - ny)),
                        ],
                        true,
                    );
                }
            }
            if let Some((p, a)) = own {
                bundles.push((p, a, state));
            }

            let emphasized = matches!(state, EdgeState::Active | EdgeState::Selected);
            let text: SharedString = if count > 1 {
                format!("×{count}").into()
            } else {
                self.edges[e.edges[0]].data.label.clone()
            };
            let label_visible = self.edge_labels
                && !text.is_empty()
                && e.source != e.target
                && (emphasized
                    || (state == EdgeState::Normal && (zoom >= 1.1 || (count > 1 && zoom >= 0.4))));
            if label_visible {
                edge_labels.push((geom.midpoint(), text, emphasized));
            }
        }

        let edge_colors = [
            colors.edge,
            colors.edge.opacity(0.15),
            colors.edge_active,
            colors.selection,
        ];
        let paint = |path: PathBuilder, color: Hsla, window: &mut Window| {
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        };
        // Dimmed first so emphasized edges land on top.
        for state in [
            EdgeState::Dimmed,
            EdgeState::Normal,
            EdgeState::Active,
            EdgeState::Selected,
        ] {
            for (p, a, _) in bundles.iter_mut().filter(|b| b.2 == state) {
                paint(
                    std::mem::replace(p, PathBuilder::fill()),
                    edge_colors[state as usize],
                    window,
                );
                paint(
                    std::mem::replace(a, PathBuilder::fill()),
                    edge_colors[state as usize],
                    window,
                );
            }
            let (p, a, used) = std::mem::replace(
                &mut batches[state as usize],
                (PathBuilder::fill(), PathBuilder::fill(), false),
            );
            if used {
                paint(p, edge_colors[state as usize], window);
                paint(a, edge_colors[state as usize], window);
            }
        }

        let caption = &metrics.caption;
        for (pos, text, active) in edge_labels {
            let color = if active {
                colors.foreground
            } else {
                colors.muted
            };
            let line = shape(&text, caption.size, color, window);
            let (w, h, pad) = (line.width, caption.line_height, metrics.space_xs);
            let chip = Bounds::new(
                point(pos.x - w / 2. - pad, pos.y - h / 2.),
                size(w + pad * 2., h),
            );
            window.paint_quad(
                gpui::fill(chip, colors.background.opacity(0.85)).corner_radii(metrics.radius_sm),
            );
            let _ = line.paint(
                point(pos.x - w / 2., pos.y - h / 2.),
                h,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }

        // Vertices, then labels above all of them.
        let mut labels = Vec::new();
        for (v, b) in vis.bodies.iter().enumerate() {
            let c = self.to_screen(b.pos);
            let r = b.radius * zoom;
            if !visible.contains(&c) && r < 200. {
                continue;
            }
            let base = self.vertex_color(v, cx);
            let faded = !in_focus(v);
            let is_selected = selected == Some(Item::Vertex(v));
            let is_hovered = self.hovered == Some(Item::Vertex(v));
            let is_group = matches!(vis.vertices[v], Vertex::Group(_));
            let rr = r + if is_hovered { 2. } else { 0. };

            if is_selected || is_hovered {
                let halo = rr + 6.;
                window.paint_quad(
                    gpui::fill(circle(c, halo), base.opacity(0.25)).corner_radii(px(halo)),
                );
            }
            if is_group {
                // A second, outer ring marks a collapsed group.
                let outer = rr + 3.;
                let ring_color = if faded {
                    base.opacity(0.15)
                } else {
                    base.opacity(0.6)
                };
                window.paint_quad(quad(
                    circle(c, outer),
                    px(outer),
                    gpui::transparent_black(),
                    px(1.5),
                    ring_color,
                    BorderStyle::Solid,
                ));
            }
            let ring = if is_selected {
                (r * 0.18).clamp(2., 4.)
            } else {
                (r * 0.12).clamp(1., 2.)
            };
            let border = if is_selected {
                colors.selection
            } else if faded {
                colors.background.opacity(0.4)
            } else {
                colors.background
            };
            let fill = if faded { base.opacity(0.2) } else { base };
            window.paint_quad(quad(
                circle(c, rr),
                px(rr),
                fill,
                px(ring),
                border,
                BorderStyle::Solid,
            ));
            if show_focus && is_selected {
                // Keyboard focus gets its own ring outside the selection ring.
                let f = rr + if is_group { 7. } else { 4. };
                window.paint_quad(quad(
                    circle(c, f),
                    px(f),
                    gpui::transparent_black(),
                    px(2.),
                    colors.focus_ring,
                    BorderStyle::Solid,
                ));
            }

            match vis.vertices[v] {
                Vertex::Group(g) if rr > 8. => {
                    let text: SharedString = self.groups[g].members.len().to_string().into();
                    let font = px((rr * 0.6).clamp(9., 18.));
                    let on_fill = colors.on_fill(base).opacity(if faded { 0.4 } else { 1. });
                    let line = shape(&text, font, on_fill, window);
                    let origin = point(c.x - line.width / 2., c.y - font * 0.6);
                    let _ = line.paint(origin, font * 1.2, TextAlign::Left, None, window, cx);
                }
                Vertex::Node(_) if b.fixed.is_some() && rr > 6. => {
                    // Small pin marker for nodes that will not move with the layout.
                    let pin = (rr * 0.22).clamp(2., 4.);
                    window.paint_quad(
                        gpui::fill(circle(c, pin), colors.background.opacity(0.9))
                            .corner_radii(px(pin)),
                    );
                }
                _ => {}
            }

            let show_label = match self.label_mode {
                LabelMode::Never => false,
                _ if is_group => true,
                LabelMode::Always => true,
                LabelMode::Auto => (focus.is_none() && zoom >= 0.7) || (focus.is_some() && !faded),
                LabelMode::Focus => focus.is_some() && !faded,
            };
            let text = match vis.vertices[v] {
                Vertex::Node(i) => self.nodes[i].label.clone(),
                Vertex::Group(g) => self.groups[g].label.clone(),
            };
            if show_label && !text.is_empty() {
                labels.push(LabelCandidate {
                    vertex: v,
                    center: c,
                    radius: rr,
                    text,
                    faded,
                    emphasized: is_selected || is_hovered || is_group,
                    // A faded group label must not crowd out the focused nodes' labels.
                    forced: is_selected || is_hovered || (is_group && !faded),
                });
            }
        }

        // Labels follow the theme's type scale, growing slightly with zoom.
        let font_size = (metrics.label.size * zoom.sqrt())
            .max(metrics.caption.size)
            .min(metrics.label.size * 1.2);
        let line_height = font_size * (metrics.label.line_height / metrics.label.size);
        // Except in `LabelMode::Always`, labels that would overlap one already
        // drawn are left out, in `label_order`. Zooming in spreads nodes apart,
        // so more labels fit. Labels shown last frame win ties, so they do not
        // flicker while the layout is still moving.
        let avoid_overlap = self.label_mode != LabelMode::Always;
        let previous = self.shown_labels.take();
        labels.sort_by(|a, b| label_order(a, b, &previous));
        let mut placed = LabelGrid::default();
        let mut shown = HashSet::with_capacity(labels.len());
        for label in labels {
            let text = truncate(&label.text, 24);
            let color = if label.faded {
                colors.muted.opacity(0.4)
            } else if label.emphasized {
                colors.foreground
            } else {
                colors.muted
            };
            let line = shape(&text, font_size, color, window);
            let c = label.center;
            let origin = point(
                c.x - line.width / 2.,
                c.y + px(label.radius) + metrics.space_xs,
            );
            let chip = (label.emphasized && !label.faded).then(|| {
                let pad = metrics.space_xs;
                Bounds::new(
                    origin - point(pad, px(0.)),
                    size(line.width + pad * 2., line_height),
                )
            });
            // Reserve the chip's area too, so no label slides under its edge.
            let rect = chip.unwrap_or(Bounds::new(origin, size(line.width, line_height)));
            if !placed.insert(rect, label.forced || !avoid_overlap) {
                continue;
            }
            shown.insert(label.vertex);
            if let Some(chip) = chip {
                window.paint_quad(
                    gpui::fill(chip, colors.background.opacity(0.9))
                        .corner_radii(metrics.radius_sm),
                );
            }
            let _ = line.paint(origin, line_height, TextAlign::Left, None, window, cx);
        }
        self.shown_labels.replace(shown);

        if show_focus {
            // Focus-visible ring for the graph itself, inset so ancestors cannot clip it.
            let ring = Bounds::new(
                bounds.origin + point(px(1.), px(1.)),
                bounds.size - size(px(2.), px(2.)),
            );
            window.paint_quad(quad(
                ring,
                metrics.radius_sm,
                gpui::transparent_black(),
                px(2.),
                colors.focus_ring,
                BorderStyle::Solid,
            ));
        }
    }

    /// Rounded convex hulls behind each expanded group with two or more visible members.
    fn paint_hulls(&self, dimmed: bool, metrics: &GraphMetrics, window: &mut Window, cx: &mut App) {
        let zoom = self.zoom;
        let padding = (14. * zoom.sqrt()).clamp(6., 20.);
        for (members, &g) in self.vis.clusters.iter().zip(&self.vis.cluster_groups) {
            if members.len() < 2 {
                continue;
            }
            let circles: Vec<(Point<Pixels>, f32)> = members
                .iter()
                .map(|&v| {
                    (
                        self.to_screen(self.vis.bodies[v].pos),
                        self.vis.bodies[v].radius * zoom,
                    )
                })
                .collect();
            let hull = circles_hull(&circles, padding);
            if hull.len() < 3 {
                continue;
            }
            let color = self.group_color(g);
            let (fill_alpha, line_alpha) = if dimmed { (0.04, 0.15) } else { (0.08, 0.35) };
            let mut fill = PathBuilder::fill();
            smooth_closed(&mut fill, &hull);
            if let Ok(path) = fill.build() {
                window.paint_path(path, color.opacity(fill_alpha));
            }
            let mut outline = PathBuilder::stroke(px(1.));
            smooth_closed(&mut outline, &hull);
            if let Ok(path) = outline.build() {
                window.paint_path(path, color.opacity(line_alpha));
            }

            if self.label_mode != LabelMode::Never {
                let top =
                    hull.iter().copied().fold(
                        (0., f32::MAX),
                        |best, p| if p.1 < best.1 { p } else { best },
                    );
                let text = truncate(&self.groups[g].label, 32);
                let caption = &metrics.caption;
                let line = shape(
                    &text,
                    caption.size,
                    color.opacity(if dimmed { 0.4 } else { 0.9 }),
                    window,
                );
                let origin = point(
                    px(top.0) - line.width / 2.,
                    px(top.1) - caption.line_height - metrics.space_xs,
                );
                let _ = line.paint(
                    origin,
                    caption.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
        }
    }

    pub(super) fn tooltip_element(
        &self,
        colors: &GraphColors,
        metrics: &GraphMetrics,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        if !self.tooltips || self.drag.as_ref().is_some_and(|d| d.moved) {
            return None;
        }
        let item = self.hovered?;
        let pointer = self.pointer?;
        let content = match &self.tooltip {
            Some(builder) => builder(&self.item_selection(item), colors, window, cx)?,
            None => self
                .default_tooltip(item, colors, metrics, cx)
                .into_any_element(),
        };
        let origin = self.bounds.get().origin;
        let offset = metrics.space_md;
        Some(
            div()
                .absolute()
                .left(pointer.x - origin.x + offset)
                .top(pointer.y - origin.y + offset)
                .child(content)
                .into_any_element(),
        )
    }

    fn default_tooltip(
        &self,
        item: Item,
        colors: &GraphColors,
        metrics: &GraphMetrics,
        cx: &App,
    ) -> impl IntoElement {
        let (swatch, kicker, title, detail): (Option<Hsla>, SharedString, SharedString, String) =
            match item {
                Item::Vertex(v) => match self.vis.vertices[v] {
                    Vertex::Node(i) => {
                        let node = &self.nodes[i];
                        let mut kicker = node.category.to_string();
                        if let Some(g) = self.node_group[i] {
                            kicker = format!("{kicker} · {}", self.groups[g].label);
                        }
                        (
                            Some(self.vertex_color(v, cx)),
                            kicker.into(),
                            node.label.clone(),
                            format!("{} relationships", self.degrees[i]),
                        )
                    }
                    Vertex::Group(g) => (
                        Some(self.group_color(g)),
                        "Group".into(),
                        self.groups[g].label.clone(),
                        format!("{} members", self.groups[g].members.len()),
                    ),
                },
                Item::Edge(e) => {
                    let ve = &self.vis.edges[e];
                    let name = |v: usize| match self.vis.vertices[v] {
                        Vertex::Node(i) => self.nodes[i].label.clone(),
                        Vertex::Group(g) => self.groups[g].label.clone(),
                    };
                    let title: SharedString = if ve.edges.len() > 1 {
                        format!("{} relationships", ve.edges.len()).into()
                    } else {
                        self.edges[ve.edges[0]].data.label.clone()
                    };
                    (
                        None,
                        "Relationship".into(),
                        title,
                        format!("{} → {}", name(ve.source), name(ve.target)),
                    )
                }
            };
        div()
            .max_w_72()
            .px(metrics.space_sm)
            .py(metrics.space_xs)
            .rounded(metrics.radius_md)
            .border_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .shadow(metrics.shadow.clone())
            .text_size(metrics.caption.size)
            .line_height(metrics.caption.line_height)
            .text_color(colors.foreground)
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .children(swatch.map(|c| div().size_2().rounded_full().bg(c)))
                    .child(div().text_color(colors.muted).child(kicker)),
            )
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
            .child(div().text_color(colors.muted).child(detail))
    }
}

fn circle(c: Point<Pixels>, r: f32) -> Bounds<Pixels> {
    Bounds::new(
        point(c.x - px(r), c.y - px(r)),
        size(px(r * 2.), px(r * 2.)),
    )
}

fn shape(text: &SharedString, font_size: Pixels, color: Hsla, window: &mut Window) -> ShapedLine {
    let run = TextRun {
        len: text.len(),
        font: window.text_style().font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(text.clone(), font_size, &[run], None)
}

fn truncate(s: &SharedString, max: usize) -> SharedString {
    if s.chars().count() <= max {
        return s.clone();
    }
    let t: String = s.chars().take(max - 1).collect();
    format!("{t}…").into()
}

/// A node or group label waiting for a place on screen.
struct LabelCandidate {
    vertex: usize,
    center: Point<Pixels>,
    radius: f32,
    text: SharedString,
    faded: bool,
    /// Drawn on a chip in the foreground color.
    emphasized: bool,
    /// Drawn even if it overlaps another label.
    forced: bool,
}

/// Placement priority: forced labels, then labels of items in focus, then
/// labels shown last frame, then bigger nodes.
fn label_order(a: &LabelCandidate, b: &LabelCandidate, previous: &HashSet<usize>) -> Ordering {
    b.forced
        .cmp(&a.forced)
        .then(a.faded.cmp(&b.faded))
        .then(
            previous
                .contains(&b.vertex)
                .cmp(&previous.contains(&a.vertex)),
        )
        .then(b.radius.total_cmp(&a.radius))
}

/// Label rectangles already drawn this frame, bucketed by a coarse grid so a
/// new label is tested only against its neighbors.
#[derive(Default)]
struct LabelGrid {
    rects: Vec<Bounds<Pixels>>,
    cells: std::collections::HashMap<(i32, i32), Vec<usize>>,
}

impl LabelGrid {
    const CELL: f32 = 96.;
    /// Breathing room between neighboring labels.
    const GAP: f32 = 2.;

    fn cells(rect: Bounds<Pixels>) -> impl Iterator<Item = (i32, i32)> {
        let cell = |v: Pixels| (f32::from(v) / Self::CELL).floor() as i32;
        let (x0, x1) = (cell(rect.left()), cell(rect.right()));
        let (y0, y1) = (cell(rect.top()), cell(rect.bottom()));
        (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
    }

    /// Records `rect` unless it overlaps a recorded one; `force` records it
    /// regardless. Returns whether the label should be drawn.
    fn insert(&mut self, rect: Bounds<Pixels>, force: bool) -> bool {
        let padded = Bounds::new(
            rect.origin - point(px(Self::GAP), px(Self::GAP)),
            rect.size + size(px(Self::GAP * 2.), px(Self::GAP * 2.)),
        );
        if !force {
            let hit = Self::cells(padded).any(|key| {
                self.cells
                    .get(&key)
                    .is_some_and(|ids| ids.iter().any(|&i| self.rects[i].intersects(&padded)))
            });
            if hit {
                return false;
            }
        }
        let id = self.rects.len();
        self.rects.push(rect);
        for key in Self::cells(rect) {
            self.cells.entry(key).or_default().push(id);
        }
        true
    }
}

#[cfg(test)]
mod label_grid_tests {
    use std::collections::HashSet;

    use super::{LabelCandidate, LabelGrid, label_order};
    use gpui::{Bounds, point, px, size};

    fn candidate(vertex: usize, radius: f32, faded: bool, forced: bool) -> LabelCandidate {
        LabelCandidate {
            vertex,
            center: point(px(0.), px(0.)),
            radius,
            text: "".into(),
            faded,
            emphasized: forced,
            forced,
        }
    }

    #[test]
    fn focused_labels_beat_faded_bigger_ones() {
        let mut labels = [
            candidate(0, 30., true, false),  // big, faded
            candidate(1, 8., false, false),  // small neighbor in focus
            candidate(2, 10., false, true),  // hovered
            candidate(3, 12., false, false), // in focus, shown last frame
            candidate(4, 12., false, false), // in focus, same size, new
        ];
        let previous = HashSet::from([3]);
        labels.sort_by(|a, b| label_order(a, b, &previous));
        let order: Vec<usize> = labels.iter().map(|l| l.vertex).collect();
        assert_eq!(order, [2, 3, 4, 1, 0]);
    }

    fn rect(x: f32, y: f32) -> Bounds<gpui::Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(60.), px(14.)))
    }

    #[test]
    fn overlapping_labels_are_left_out_unless_forced() {
        let mut grid = LabelGrid::default();
        assert!(grid.insert(rect(0., 0.), false));
        assert!(!grid.insert(rect(30., 5.), false), "overlaps the first");
        assert!(grid.insert(rect(0., 40.), false), "clear of it");
        assert!(
            grid.insert(rect(30., 5.), true),
            "forced labels always draw"
        );
        // Across a grid cell boundary.
        assert!(grid.insert(rect(200., 90.), false));
        assert!(!grid.insert(rect(220., 100.), false));
    }
}
