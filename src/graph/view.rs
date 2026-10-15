use std::{cell::Cell, collections::HashMap, rc::Rc};

use gpui::{
    App, BorderStyle, Bounds, Context, CursorStyle, DispatchPhase, Entity, EventEmitter, Hitbox,
    HitboxBehavior, Hsla, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, PathBuilder, Pixels, Point, Render, ScrollWheelEvent, SharedString,
    Styled as _, TextAlign, TextRun, Window, canvas, div, point, px, quad, size,
};
use gpui_base::Theme;

use super::layout::{Body, ForceConfig, Simulation, Vec2};
use super::model::{GraphData, Node};

/// AntV's categorical palette, so results look familiar to G6 users.
const PALETTE: [u32; 10] = [
    0x5b8ff9, 0x61ddaa, 0x65789b, 0xf6bd16, 0x7262fd, 0x78d3f8, 0x9661bc, 0xf6903d, 0x008685,
    0xf08bb4,
];

const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 6.;
/// Pointer travel (px) before a press becomes a drag instead of a click.
const DRAG_THRESHOLD: f32 = 3.;

#[derive(Clone, Debug)]
pub enum GraphEvent {
    SelectionChanged(Option<Node>),
    /// Double-click on a node: the usual "expand neighbors" gesture in graph explorers.
    NodeActivated(Node),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelMode {
    /// Only the hovered / selected node and its neighbors.
    Focus,
    /// Every node once the zoom level makes labels readable.
    Auto,
    Always,
}

struct EdgeItem {
    source: usize,
    target: usize,
    label: SharedString,
    /// Signed bend for parallel edges between the same pair (0 = straight).
    curvature: f32,
}

enum DragKind {
    Pan { start_pan: Point<Pixels> },
    Node { index: usize },
}

struct Drag {
    kind: DragKind,
    start: Point<Pixels>,
    moved: bool,
}

pub struct NetworkGraph {
    nodes: Vec<Node>,
    bodies: Vec<Body>,
    edges: Vec<EdgeItem>,
    links: Vec<(usize, usize)>,
    degrees: Vec<usize>,
    neighbors: Vec<Vec<usize>>,
    index: HashMap<SharedString, usize>,
    categories: Vec<SharedString>,
    sim: Simulation,

    /// Screen position = bounds.center + pan + world * zoom.
    pan: Point<Pixels>,
    zoom: f32,
    /// Keep fitting the view while the layout settles, until the user moves it.
    auto_fit: bool,
    bounds: Rc<Cell<Bounds<Pixels>>>,

    hovered: Option<usize>,
    selected: Option<usize>,
    drag: Option<Drag>,
    pointer: Option<Point<Pixels>>,

    pub label_mode: LabelMode,
    pub show_arrows: bool,
    pub show_edge_labels: bool,
}

impl EventEmitter<GraphEvent> for NetworkGraph {}

impl NetworkGraph {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            nodes: Vec::new(),
            bodies: Vec::new(),
            edges: Vec::new(),
            links: Vec::new(),
            degrees: Vec::new(),
            neighbors: Vec::new(),
            index: HashMap::new(),
            categories: Vec::new(),
            sim: Simulation::new(ForceConfig::default()),
            pan: Point::default(),
            zoom: 1.,
            auto_fit: true,
            bounds: Rc::new(Cell::new(Bounds::default())),
            hovered: None,
            selected: None,
            drag: None,
            pointer: None,
            label_mode: LabelMode::Auto,
            show_arrows: true,
            show_edge_labels: true,
        }
    }

    // ---- data -------------------------------------------------------------

    /// Replace the whole graph with a new query result.
    pub fn set_data(&mut self, data: GraphData, cx: &mut Context<Self>) {
        self.nodes.clear();
        self.bodies.clear();
        self.edges.clear();
        self.index.clear();
        self.categories.clear();
        self.hovered = None;
        self.drag = None;
        if self.selected.take().is_some() {
            cx.emit(GraphEvent::SelectionChanged(None));
        }
        self.merge(data, None, cx);
        self.pan = Point::default();
        self.zoom = 1.;
        self.auto_fit = true;
    }

    /// Add nodes/edges to the current graph (e.g. expanding a node). New nodes
    /// start around `anchor` so the existing layout is not disturbed much.
    pub fn merge(&mut self, data: GraphData, anchor: Option<&str>, cx: &mut Context<Self>) {
        let anchor_pos = anchor
            .and_then(|id| self.index.get(id))
            .map(|&i| self.bodies[i].pos);
        let first_new = self.nodes.len();
        for node in data.nodes {
            if self.index.contains_key(&node.id) {
                continue;
            }
            if !self.categories.contains(&node.category) {
                self.categories.push(node.category.clone());
            }
            self.index.insert(node.id.clone(), self.nodes.len());
            self.nodes.push(node);
            self.bodies.push(Body::default());
        }
        match anchor_pos {
            Some(center) => {
                let added = self.nodes.len() - first_new;
                for (k, body) in self.bodies[first_new..].iter_mut().enumerate() {
                    let a = k as f32 / added.max(1) as f32 * std::f32::consts::TAU;
                    body.pos = Vec2::new(center.x + 30. * a.cos(), center.y + 30. * a.sin());
                }
            }
            None => Simulation::initialize_positions(&mut self.bodies, first_new),
        }

        for edge in data.edges {
            let (Some(&s), Some(&t)) = (self.index.get(&edge.source), self.index.get(&edge.target))
            else {
                continue;
            };
            let duplicate = self.edges.iter().any(|e| {
                e.source == s && e.target == t && e.label == edge.label
            });
            if !duplicate {
                self.edges.push(EdgeItem { source: s, target: t, label: edge.label, curvature: 0. });
            }
        }
        self.rebuild_topology();
        self.sim.reheat(if anchor_pos.is_some() { 0.5 } else { 1. });
        cx.notify();
    }

    fn rebuild_topology(&mut self) {
        let n = self.nodes.len();
        self.links = self.edges.iter().map(|e| (e.source, e.target)).collect();
        self.degrees = vec![0; n];
        self.neighbors = vec![Vec::new(); n];
        for &(s, t) in &self.links {
            self.degrees[s] += 1;
            self.degrees[t] += 1;
            if s != t {
                self.neighbors[s].push(t);
                self.neighbors[t].push(s);
            }
        }
        for list in &mut self.neighbors {
            list.sort_unstable();
            list.dedup();
        }
        for (i, body) in self.bodies.iter_mut().enumerate() {
            body.radius = node_radius(self.degrees[i]);
        }
        // Fan out parallel edges so multi-relationships stay distinguishable.
        let mut groups: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (i, e) in self.edges.iter().enumerate() {
            groups.entry((e.source.min(e.target), e.source.max(e.target))).or_default().push(i);
        }
        for list in groups.values() {
            let count = list.len() as f32;
            for (k, &i) in list.iter().enumerate() {
                let e = &mut self.edges[i];
                let mut c = if count > 1. { k as f32 - (count - 1.) / 2. } else { 0. };
                // Normalise direction so A->B and B->A bend to opposite sides.
                if e.source > e.target {
                    c = -c;
                }
                e.curvature = c;
            }
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn categories(&self) -> Vec<(SharedString, Hsla, usize)> {
        self.categories
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let count = self.nodes.iter().filter(|n| &n.category == c).count();
                (c.clone(), category_color(i), count)
            })
            .collect()
    }

    pub fn neighbors_of(&self, id: &str) -> Vec<Node> {
        self.index
            .get(id)
            .map(|&i| self.neighbors[i].iter().map(|&j| self.nodes[j].clone()).collect())
            .unwrap_or_default()
    }

    pub fn color_of(&self, node: &Node) -> Hsla {
        let ix = self.categories.iter().position(|c| c == &node.category).unwrap_or(0);
        category_color(ix)
    }

    // ---- viewport -----------------------------------------------------------

    pub fn select(&mut self, id: Option<&str>, center: bool, cx: &mut Context<Self>) {
        let ix = id.and_then(|id| self.index.get(id).copied());
        if ix == self.selected {
            return;
        }
        self.selected = ix;
        if let (Some(i), true) = (ix, center) {
            let p = self.bodies[i].pos;
            self.auto_fit = false;
            self.pan = point(px(-p.x * self.zoom), px(-p.y * self.zoom));
        }
        cx.emit(GraphEvent::SelectionChanged(ix.map(|i| self.nodes[i].clone())));
        cx.notify();
    }

    pub fn fit_view(&mut self, cx: &mut Context<Self>) {
        self.auto_fit = true;
        self.apply_fit(1.);
        cx.notify();
    }

    pub fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        self.auto_fit = false;
        let center = self.bounds.get().center();
        self.zoom_around(center, factor);
        cx.notify();
    }

    /// Restart the layout from scratch (d3 `simulation.alpha(1).restart()`).
    pub fn relayout(&mut self, cx: &mut Context<Self>) {
        for b in &mut self.bodies {
            b.fixed = None;
        }
        Simulation::initialize_positions(&mut self.bodies, 0);
        self.sim.reheat(1.);
        self.auto_fit = true;
        cx.notify();
    }

    fn apply_fit(&mut self, blend: f32) {
        let bounds = self.bounds.get();
        if self.bodies.is_empty() || bounds.size.width <= px(0.) {
            return;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for b in &self.bodies {
            x0 = x0.min(b.pos.x - b.radius);
            y0 = y0.min(b.pos.y - b.radius);
            x1 = x1.max(b.pos.x + b.radius);
            y1 = y1.max(b.pos.y + b.radius);
        }
        let padding = 48.;
        let w = (bounds.size.width.as_f32() - padding * 2.).max(1.);
        let h = (bounds.size.height.as_f32() - padding * 2.).max(1.);
        let zoom = (w / (x1 - x0).max(1.)).min(h / (y1 - y0).max(1.)).clamp(MIN_ZOOM, 2.);
        let (cx, cy) = ((x0 + x1) / 2., (y0 + y1) / 2.);
        let target = point(px(-cx * zoom), px(-cy * zoom));
        self.zoom += (zoom - self.zoom) * blend;
        self.pan = self.pan + (target - self.pan) * blend;
    }

    fn zoom_around(&mut self, anchor: Point<Pixels>, factor: f32) {
        let world = self.to_world(anchor);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let center = self.bounds.get().center();
        self.pan = anchor - center - point(px(world.x * self.zoom), px(world.y * self.zoom));
    }

    fn to_screen(&self, p: Vec2) -> Point<Pixels> {
        self.bounds.get().center() + self.pan + point(px(p.x * self.zoom), px(p.y * self.zoom))
    }

    fn to_world(&self, p: Point<Pixels>) -> Vec2 {
        let local = p - self.bounds.get().center() - self.pan;
        Vec2::new(local.x.as_f32() / self.zoom, local.y.as_f32() / self.zoom)
    }

    fn hit_node(&self, p: Point<Pixels>) -> Option<usize> {
        let w = self.to_world(p);
        let slop = 3. / self.zoom;
        // Topmost first: nodes are painted in index order.
        self.bodies.iter().enumerate().rev().find_map(|(i, b)| {
            let (dx, dy) = (b.pos.x - w.x, b.pos.y - w.y);
            let r = b.radius + slop;
            (dx * dx + dy * dy <= r * r).then_some(i)
        })
    }

    // ---- input --------------------------------------------------------------

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        let hit = self.hit_node(ev.position);
        if ev.click_count >= 2 {
            if let Some(i) = hit {
                cx.emit(GraphEvent::NodeActivated(self.nodes[i].clone()));
            }
            return;
        }
        let kind = match hit {
            Some(index) => DragKind::Node { index },
            None => DragKind::Pan { start_pan: self.pan },
        };
        self.drag = Some(Drag { kind, start: ev.position, moved: false });
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, inside: bool, cx: &mut Context<Self>) {
        self.pointer = inside.then_some(ev.position);
        if let Some(drag) = &mut self.drag {
            let delta = ev.position - drag.start;
            if !drag.moved {
                let d = delta.x.as_f32().hypot(delta.y.as_f32());
                if d < DRAG_THRESHOLD {
                    return;
                }
                drag.moved = true;
            }
            match drag.kind {
                DragKind::Pan { start_pan } => {
                    self.auto_fit = false;
                    self.pan = start_pan + delta;
                }
                DragKind::Node { index } => {
                    let w = self.to_world(ev.position);
                    self.bodies[index].fixed = Some(w);
                    self.bodies[index].pos = w;
                    self.sim.alpha_target = 0.3;
                    self.sim.reheat(0.3);
                    self.auto_fit = false;
                }
            }
            cx.notify();
            return;
        }
        let hovered = if inside { self.hit_node(ev.position) } else { None };
        if hovered != self.hovered {
            self.hovered = hovered;
            cx.notify();
        } else if hovered.is_some() {
            cx.notify(); // keep the tooltip following the pointer
        }
    }

    fn on_mouse_up(&mut self, ev: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        match drag.kind {
            DragKind::Node { index } => {
                // Release like d3's drag example: the node rejoins the simulation.
                self.bodies[index].fixed = None;
                self.sim.alpha_target = 0.;
                if !drag.moved {
                    let id = self.nodes[index].id.clone();
                    self.select(Some(&id), false, cx);
                }
            }
            DragKind::Pan { .. } => {
                if !drag.moved && self.hit_node(ev.position).is_none() {
                    self.select(None, false, cx);
                }
            }
        }
        cx.notify();
    }

    fn on_scroll(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = ev.delta.pixel_delta(px(20.));
        // Pinch/precise devices send small pixel deltas; wheels send lines.
        let factor = (-delta.y.as_f32() * 0.0025).exp();
        self.auto_fit = false;
        self.zoom_around(ev.position, factor);
        cx.notify();
    }

    // ---- rendering ----------------------------------------------------------

    fn focus_set(&self) -> Option<(usize, &[usize])> {
        self.hovered
            .or(self.selected)
            .map(|i| (i, self.neighbors[i].as_slice()))
    }

    fn paint_graph(&self, bounds: Bounds<Pixels>, colors: &Colors, window: &mut Window, cx: &mut App) {
        let zoom = self.zoom;
        let focus = self.focus_set();
        let in_focus = |i: usize| match focus {
            None => true,
            Some((f, ns)) => i == f || ns.binary_search(&i).is_ok(),
        };
        let edge_in_focus = |e: &EdgeItem| match focus {
            None => true,
            Some((f, _)) => e.source == f || e.target == f,
        };
        let visible = bounds.dilate(px(40.));

        // Edges: batched into a few paths by state to keep draw calls low.
        let stroke = (1.2 * zoom.sqrt()).clamp(0.6, 2.);
        let mut normal = PathBuilder::stroke(px(stroke));
        let mut dimmed = PathBuilder::stroke(px(stroke));
        let mut active = PathBuilder::stroke(px(stroke * 1.6));
        let mut arrows_normal = PathBuilder::fill();
        let mut arrows_dimmed = PathBuilder::fill();
        let mut arrows_active = PathBuilder::fill();
        let (mut has_normal, mut has_dimmed, mut has_active) = (false, false, false);
        let mut edge_labels = Vec::new();

        for e in &self.edges {
            let (a, b) = (self.to_screen(self.bodies[e.source].pos), self.to_screen(self.bodies[e.target].pos));
            if !segment_may_intersect(visible, a, b) {
                continue;
            }
            let state = match focus {
                None => 0,
                Some(_) if edge_in_focus(e) => 2,
                Some(_) => 1,
            };
            let (path, arrows, flag) = match state {
                0 => (&mut normal, &mut arrows_normal, &mut has_normal),
                1 => (&mut dimmed, &mut arrows_dimmed, &mut has_dimmed),
                _ => (&mut active, &mut arrows_active, &mut has_active),
            };
            *flag = true;
            let rs = self.bodies[e.source].radius * zoom;
            let rt = self.bodies[e.target].radius * zoom;
            if e.source == e.target {
                draw_self_loop(path, a, rs);
                continue;
            }
            let geom = EdgeGeometry::new(a, b, e.curvature * 28. * zoom.min(1.5));
            let start = geom.point_from_start(rs);
            let arrow = 6. + stroke * 2.;
            let gap = if self.show_arrows { rt + arrow } else { rt };
            let end = geom.point_from_end(gap);
            path.move_to(start);
            match geom.ctrl {
                Some(ctrl) => path.curve_to(end, ctrl),
                None => path.line_to(end),
            }
            if self.show_arrows && zoom > 0.25 {
                let tip = geom.point_from_end(rt);
                let dir = geom.end_direction();
                let (nx, ny) = (-dir.1, dir.0);
                let base = (tip.x.as_f32() - dir.0 * arrow, tip.y.as_f32() - dir.1 * arrow);
                arrows.add_polygon(
                    &[
                        tip,
                        point(px(base.0 + nx * arrow * 0.45), px(base.1 + ny * arrow * 0.45)),
                        point(px(base.0 - nx * arrow * 0.45), px(base.1 - ny * arrow * 0.45)),
                    ],
                    true,
                );
            }
            let label_visible = self.show_edge_labels
                && !e.label.is_empty()
                && ((state == 2 && focus.is_some()) || (state == 0 && zoom >= 1.1));
            if label_visible {
                edge_labels.push((geom.midpoint(), e.label.clone(), state == 2));
            }
        }

        for (path, color, has) in [
            (dimmed, colors.edge.opacity(0.15), has_dimmed),
            (normal, colors.edge, has_normal),
            (active, colors.edge_active, has_active),
        ] {
            if has {
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            }
        }
        for (path, color, has) in [
            (arrows_dimmed, colors.edge.opacity(0.15), has_dimmed),
            (arrows_normal, colors.edge, has_normal),
            (arrows_active, colors.edge_active, has_active),
        ] {
            if has {
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            }
        }

        let label_size = px(11.);
        for (pos, text, active) in edge_labels {
            let color = if active { colors.foreground } else { colors.muted };
            let line = shape(&text, label_size, color, window);
            let w = line.width;
            let bg = Bounds::new(
                point(pos.x - w / 2. - px(3.), pos.y - px(8.)),
                size(w + px(6.), px(16.)),
            );
            window.paint_quad(gpui::fill(bg, colors.background.opacity(0.85)).corner_radii(px(3.)));
            let _ = line.paint(point(pos.x - w / 2., pos.y - px(7.)), px(14.), TextAlign::Left, None, window, cx);
        }

        // Nodes, then labels on top of every node.
        let mut labels = Vec::new();
        for (i, b) in self.bodies.iter().enumerate() {
            let c = self.to_screen(b.pos);
            let r = b.radius * zoom;
            if !visible.contains(&c) && r < 200. {
                continue;
            }
            let base = category_color(self.category_index(i));
            let faded = !in_focus(i);
            let fill = if faded { base.opacity(0.2) } else { base };
            let is_selected = self.selected == Some(i);
            let is_hovered = self.hovered == Some(i);
            let ring = if is_selected { (r * 0.18).clamp(2., 4.) } else { (r * 0.12).clamp(1., 2.) };
            let border_color = if is_selected {
                colors.selection
            } else if faded {
                colors.background.opacity(0.4)
            } else {
                colors.background
            };
            let rr = r + if is_hovered { 2. } else { 0. };
            let node_bounds = Bounds::new(point(c.x - px(rr), c.y - px(rr)), size(px(rr * 2.), px(rr * 2.)));
            if is_selected || is_hovered {
                let halo = rr + 6.;
                let halo_bounds = Bounds::new(point(c.x - px(halo), c.y - px(halo)), size(px(halo * 2.), px(halo * 2.)));
                window.paint_quad(gpui::fill(halo_bounds, base.opacity(0.25)).corner_radii(px(halo)));
            }
            window.paint_quad(quad(node_bounds, px(rr), fill, px(ring), border_color, BorderStyle::Solid));

            let show_label = match self.label_mode {
                LabelMode::Always => true,
                LabelMode::Auto => (focus.is_none() && zoom >= 0.7) || (focus.is_some() && !faded),
                LabelMode::Focus => focus.is_some() && !faded,
            };
            if show_label && !self.nodes[i].label.is_empty() {
                labels.push((c, rr, i, faded, is_selected || is_hovered));
            }
        }

        let font_size = px((12. * zoom.sqrt()).clamp(10., 14.));
        for (c, r, i, faded, strong) in labels {
            let text = truncate(&self.nodes[i].label, 24);
            let color = if faded { colors.muted.opacity(0.4) } else if strong { colors.foreground } else { colors.muted };
            let line = shape(&text, font_size, color, window);
            let origin = point(c.x - line.width / 2., c.y + px(r) + px(3.));
            if strong {
                let bg = Bounds::new(origin - point(px(4.), px(1.)), size(line.width + px(8.), font_size + px(4.)));
                window.paint_quad(gpui::fill(bg, colors.background.opacity(0.9)).corner_radii(px(3.)));
            }
            let _ = line.paint(origin, font_size + px(2.), TextAlign::Left, None, window, cx);
        }
    }

    fn category_index(&self, i: usize) -> usize {
        let c = &self.nodes[i].category;
        self.categories.iter().position(|x| x == c).unwrap_or(0)
    }

    fn tooltip(&self, colors: &Colors) -> Option<impl IntoElement> {
        if self.drag.as_ref().is_some_and(|d| d.moved) {
            return None;
        }
        let i = self.hovered?;
        let pointer = self.pointer?;
        let origin = self.bounds.get().origin;
        let node = &self.nodes[i];
        let color = category_color(self.category_index(i));
        Some(
            div()
                .absolute()
                .left(pointer.x - origin.x + px(14.))
                .top(pointer.y - origin.y + px(14.))
                .max_w(px(280.))
                .px_2()
                .py_1p5()
                .rounded_md()
                .border_1()
                .border_color(colors.border)
                .bg(colors.surface)
                .shadow_md()
                .text_xs()
                .text_color(colors.foreground)
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(div().size_2().rounded_full().bg(color))
                        .child(div().text_color(colors.muted).child(node.category.clone())),
                )
                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(node.label.clone()))
                .child(
                    div()
                        .text_color(colors.muted)
                        .child(format!("{} relationships · double-click to expand", self.degrees[i])),
                ),
        )
    }
}

impl Render for NetworkGraph {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.sim.is_active() && !self.bodies.is_empty() {
            // Big graphs settle with fewer frames if we tick a few times per frame.
            let ticks = if self.bodies.len() > 1500 { 1 } else { 2 };
            for _ in 0..ticks {
                self.sim.tick(&mut self.bodies, &self.links, &self.degrees);
            }
            if self.auto_fit {
                self.apply_fit(0.25);
            }
            window.request_animation_frame();
        }

        let colors = Colors::from_theme(cx);
        let entity = cx.entity();
        let bounds_cell = self.bounds.clone();
        let paint_colors = colors.clone();

        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(colors.background)
            .child(
                canvas(
                    move |bounds, window, _| {
                        bounds_cell.set(bounds);
                        window.insert_hitbox(bounds, HitboxBehavior::Normal)
                    },
                    move |bounds, hitbox: Hitbox, window, cx| {
                        register_input(&entity, &hitbox, window, cx);
                        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                            entity.update(cx, |this, cx| this.paint_graph(bounds, &paint_colors, window, cx));
                        });
                    },
                )
                .size_full(),
            )
            .children(self.tooltip(&colors))
    }
}

fn register_input(entity: &Entity<NetworkGraph>, hitbox: &Hitbox, window: &mut Window, cx: &mut App) {
    let graph = entity.read(cx);
    let cursor = match (&graph.drag, graph.hovered) {
        (Some(d), _) if d.moved => CursorStyle::ClosedHand,
        (_, Some(_)) => CursorStyle::PointingHand,
        _ => CursorStyle::Arrow,
    };
    window.set_cursor_style(cursor, hitbox);

    let (e, h) = (entity.clone(), hitbox.clone());
    window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble && ev.button == MouseButton::Left && h.is_hovered(window) {
            e.update(cx, |this, cx| this.on_mouse_down(ev, cx));
        }
    });
    let (e, h) = (entity.clone(), hitbox.clone());
    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            let inside = h.is_hovered(window);
            let tracking = e.read(cx).drag.is_some() || e.read(cx).hovered.is_some() || inside;
            if tracking {
                e.update(cx, |this, cx| this.on_mouse_move(ev, inside, cx));
            }
        }
    });
    let e = entity.clone();
    window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble && ev.button == MouseButton::Left {
            e.update(cx, |this, cx| this.on_mouse_up(ev, cx));
        }
    });
    let (e, h) = (entity.clone(), hitbox.clone());
    window.on_mouse_event(move |ev: &ScrollWheelEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble && h.should_handle_scroll(window) {
            e.update(cx, |this, cx| this.on_scroll(ev, cx));
        }
    });
}

#[derive(Clone)]
struct Colors {
    background: Hsla,
    surface: Hsla,
    foreground: Hsla,
    muted: Hsla,
    border: Hsla,
    edge: Hsla,
    edge_active: Hsla,
    selection: Hsla,
}

impl Colors {
    fn from_theme(cx: &App) -> Self {
        let t = Theme::global(cx).tokens.colors;
        Self {
            background: t.background,
            surface: t.surface,
            foreground: t.foreground,
            muted: t.muted_foreground,
            border: t.border,
            edge: t.muted_foreground.opacity(0.45),
            edge_active: t.foreground.opacity(0.8),
            selection: t.ring,
        }
    }
}

pub fn category_color(i: usize) -> Hsla {
    let hex = PALETTE[i % PALETTE.len()];
    gpui::rgb(hex).into()
}

fn node_radius(degree: usize) -> f32 {
    // "Node importance": area grows with degree, like G6's map-node-size.
    (7. + (degree as f32).sqrt() * 2.6).min(28.)
}

fn shape(text: &SharedString, font_size: Pixels, color: Hsla, window: &mut Window) -> gpui::ShapedLine {
    let run = TextRun {
        len: text.len(),
        font: window.text_style().font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window.text_system().shape_line(text.clone(), font_size, &[run], None)
}

fn truncate(s: &SharedString, max: usize) -> SharedString {
    if s.chars().count() <= max {
        return s.clone();
    }
    let t: String = s.chars().take(max - 1).collect();
    format!("{t}…").into()
}

fn segment_may_intersect(b: Bounds<Pixels>, p: Point<Pixels>, q: Point<Pixels>) -> bool {
    let (x0, x1) = (p.x.min(q.x), p.x.max(q.x));
    let (y0, y1) = (p.y.min(q.y), p.y.max(q.y));
    !(x1 < b.left() || x0 > b.right() || y1 < b.top() || y0 > b.bottom())
}

fn draw_self_loop(path: &mut PathBuilder, c: Point<Pixels>, r: f32) {
    let s = r.max(6.);
    let start = point(c.x + px(r * 0.6), c.y - px(r * 0.8));
    let end = point(c.x + px(r * 0.8), c.y - px(r * 0.6));
    path.move_to(start);
    path.cubic_bezier_to(end, point(c.x + px(s * 1.2), c.y - px(s * 3.)), point(c.x + px(s * 3.), c.y - px(s * 1.2)));
}

/// A straight or quadratic edge between two screen points.
struct EdgeGeometry {
    a: Point<Pixels>,
    b: Point<Pixels>,
    ctrl: Option<Point<Pixels>>,
}

impl EdgeGeometry {
    fn new(a: Point<Pixels>, b: Point<Pixels>, bend: f32) -> Self {
        let ctrl = (bend.abs() > 0.01).then(|| {
            let (dx, dy) = ((b.x - a.x).as_f32(), (b.y - a.y).as_f32());
            let len = dx.hypot(dy).max(1.);
            let (nx, ny) = (-dy / len, dx / len);
            // Control point offset is twice the desired mid-curve offset.
            point(
                px((a.x + b.x).as_f32() / 2. + nx * bend * 2.),
                px((a.y + b.y).as_f32() / 2. + ny * bend * 2.),
            )
        });
        Self { a, b, ctrl }
    }

    fn eval(&self, t: f32) -> Point<Pixels> {
        match self.ctrl {
            None => self.a + (self.b - self.a) * t,
            Some(c) => {
                let u = 1. - t;
                let f = |a: Pixels, c: Pixels, b: Pixels| px(u * u * a.as_f32() + 2. * u * t * c.as_f32() + t * t * b.as_f32());
                point(f(self.a.x, c.x, self.b.x), f(self.a.y, c.y, self.b.y))
            }
        }
    }

    fn approx_len(&self) -> f32 {
        let d = |p: Point<Pixels>, q: Point<Pixels>| (q.x - p.x).as_f32().hypot((q.y - p.y).as_f32());
        match self.ctrl {
            None => d(self.a, self.b),
            Some(c) => (d(self.a, c) + d(c, self.b) + d(self.a, self.b)) / 2.,
        }
    }

    fn point_from_start(&self, dist: f32) -> Point<Pixels> {
        self.eval((dist / self.approx_len().max(1.)).min(0.5))
    }

    fn point_from_end(&self, dist: f32) -> Point<Pixels> {
        self.eval(1. - (dist / self.approx_len().max(1.)).min(0.5))
    }

    fn midpoint(&self) -> Point<Pixels> {
        self.eval(0.5)
    }

    /// Unit tangent at the target end.
    fn end_direction(&self) -> (f32, f32) {
        let from = self.ctrl.unwrap_or(self.a);
        let (dx, dy) = ((self.b.x - from.x).as_f32(), (self.b.y - from.y).as_f32());
        let len = dx.hypot(dy).max(1e-3);
        (dx / len, dy / len)
    }
}
