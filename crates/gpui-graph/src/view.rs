use std::{cell::Cell, collections::HashMap, rc::Rc};

use gpui::{
    AnyElement, App, BorderStyle, Bounds, ContentMask, Context, CursorStyle, DispatchPhase, Entity,
    EventEmitter, FontWeight, Hitbox, HitboxBehavior, Hsla, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, PathBuilder, Pixels, Point,
    Render, ScrollWheelEvent, SharedString, ShapedLine, Styled as _, TextAlign, TextRun, Window,
    canvas, div, point, px, quad, size,
};

use crate::geometry::{EdgeGeometry, draw_self_loop, segment_may_intersect};
use crate::layout::{Body, ForceConfig, Simulation, Vec2};
use crate::model::{Edge, GraphData, Node};
use crate::style::{GraphColors, LabelMode, NodeSizing, Palette};

const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 6.;
/// Pointer travel (px) before a press becomes a drag instead of a click.
const DRAG_THRESHOLD: f32 = 3.;
/// Pointer distance (px) within which an edge counts as hit.
const EDGE_HIT_SLOP: f32 = 4.;

/// What the user has selected or is pointing at.
#[derive(Clone, Debug, Default)]
pub enum Selection {
    #[default]
    None,
    Node(Node),
    Edge(Edge),
}

#[derive(Clone, Debug)]
pub enum GraphEvent {
    SelectionChanged(Selection),
    /// Double-click on a node: the usual "expand neighbors" gesture in graph explorers.
    NodeActivated(Node),
    /// Right-click. `position` is in window coordinates, ready for a popover anchor.
    ContextMenu { target: Selection, position: Point<Pixels> },
}

/// A category present in the graph, for legends.
#[derive(Clone, Debug)]
pub struct CategoryInfo {
    pub name: SharedString,
    pub color: Hsla,
    pub count: usize,
}

/// Builds tooltip content for the hovered item; `None` hides the tooltip.
pub type TooltipBuilder = Rc<dyn Fn(&Selection, &GraphColors, &mut Window, &mut App) -> Option<AnyElement>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Node(usize),
    Edge(usize),
}

struct EdgeItem {
    data: Edge,
    source: usize,
    target: usize,
    /// Signed bend for parallel edges between the same pair (0 = straight).
    curvature: f32,
}

enum DragKind {
    Pan { start_pan: Point<Pixels> },
    Node { index: usize, was_pinned: bool },
}

struct Drag {
    kind: DragKind,
    start: Point<Pixels>,
    moved: bool,
}

/// An interactive, force-directed network graph.
///
/// ```ignore
/// let graph = cx.new(|cx| {
///     NetworkGraph::new(cx)
///         .with_data(GraphData::new(nodes, edges))
///         .with_label_mode(LabelMode::Auto)
/// });
/// cx.subscribe(&graph, |this, _, event: &GraphEvent, cx| { /* ... */ }).detach();
/// ```
///
/// Place it in a sized container: it fills its parent.
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

    hovered: Option<Item>,
    selected: Option<Item>,
    drag: Option<Drag>,
    pointer: Option<Point<Pixels>>,

    label_mode: LabelMode,
    show_arrows: bool,
    show_edge_labels: bool,
    /// Dragged nodes stay where they are dropped.
    pin_on_drag: bool,
    palette: Palette,
    sizing: NodeSizing,
    colors: Option<GraphColors>,
    tooltip: Option<TooltipBuilder>,
    show_tooltip: bool,
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
            label_mode: LabelMode::default(),
            show_arrows: true,
            show_edge_labels: true,
            pin_on_drag: false,
            palette: Palette::default(),
            sizing: NodeSizing::default(),
            colors: None,
            tooltip: None,
            show_tooltip: true,
        }
    }

    // ---- construction options -----------------------------------------------

    pub fn with_data(mut self, data: GraphData) -> Self {
        self.load(data, None);
        self
    }

    pub fn with_force_config(mut self, config: ForceConfig) -> Self {
        self.sim.config = config;
        self
    }

    pub fn with_label_mode(mut self, mode: LabelMode) -> Self {
        self.label_mode = mode;
        self
    }

    pub fn with_arrows(mut self, show: bool) -> Self {
        self.show_arrows = show;
        self
    }

    pub fn with_edge_labels(mut self, show: bool) -> Self {
        self.show_edge_labels = show;
        self
    }

    pub fn with_pin_on_drag(mut self, pin: bool) -> Self {
        self.pin_on_drag = pin;
        self
    }

    pub fn with_palette(mut self, palette: Palette) -> Self {
        self.palette = palette;
        self
    }

    pub fn with_node_sizing(mut self, sizing: NodeSizing) -> Self {
        self.sizing = sizing;
        self
    }

    /// Fixed colors instead of following the gpui-base theme.
    pub fn with_colors(mut self, colors: GraphColors) -> Self {
        self.colors = Some(colors);
        self
    }

    /// Replace the built-in tooltip. Return `None` from the builder to hide it.
    pub fn with_tooltip(
        mut self,
        builder: impl Fn(&Selection, &GraphColors, &mut Window, &mut App) -> Option<AnyElement> + 'static,
    ) -> Self {
        self.tooltip = Some(Rc::new(builder));
        self
    }

    pub fn without_tooltip(mut self) -> Self {
        self.show_tooltip = false;
        self
    }

    // ---- runtime settings ---------------------------------------------------

    pub fn label_mode(&self) -> LabelMode {
        self.label_mode
    }

    pub fn set_label_mode(&mut self, mode: LabelMode, cx: &mut Context<Self>) {
        self.label_mode = mode;
        cx.notify();
    }

    pub fn show_arrows(&self) -> bool {
        self.show_arrows
    }

    pub fn set_show_arrows(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_arrows = show;
        cx.notify();
    }

    pub fn show_edge_labels(&self) -> bool {
        self.show_edge_labels
    }

    pub fn set_show_edge_labels(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_edge_labels = show;
        cx.notify();
    }

    pub fn set_pin_on_drag(&mut self, pin: bool) {
        self.pin_on_drag = pin;
    }

    pub fn set_force_config(&mut self, config: ForceConfig, cx: &mut Context<Self>) {
        self.sim.config = config;
        self.sim.reheat(0.5);
        cx.notify();
    }

    pub fn set_colors(&mut self, colors: Option<GraphColors>, cx: &mut Context<Self>) {
        self.colors = colors;
        cx.notify();
    }

    // ---- data -------------------------------------------------------------

    /// Replace the whole graph (a new query result) and reset the viewport.
    pub fn set_data(&mut self, data: GraphData, cx: &mut Context<Self>) {
        let had_selection = self.selected.is_some();
        self.load(data, None);
        if had_selection {
            cx.emit(GraphEvent::SelectionChanged(Selection::None));
        }
        cx.notify();
    }

    /// Add nodes and edges to the current graph, e.g. expanding a node. Existing
    /// ids are kept; new nodes start around `anchor` so the layout stays stable.
    pub fn merge(&mut self, data: GraphData, anchor: Option<&str>, cx: &mut Context<Self>) {
        self.insert(data, anchor);
        cx.notify();
    }

    /// Remove nodes and every edge touching them.
    pub fn remove_nodes(&mut self, ids: &[&str], cx: &mut Context<Self>) {
        let doomed: Vec<bool> = self.nodes.iter().map(|n| ids.contains(&n.id.as_ref())).collect();
        if !doomed.contains(&true) {
            return;
        }
        let selection_lost = match self.selected {
            Some(Item::Node(i)) => doomed[i],
            Some(Item::Edge(e)) => doomed[self.edges[e].source] || doomed[self.edges[e].target],
            None => false,
        };
        let selected_id = self.selected_node_id();
        let selected_edge = match self.selected {
            Some(Item::Edge(e)) if !selection_lost => Some((self.edges[e].data.source.clone(), self.edges[e].data.target.clone(), self.edges[e].data.label.clone())),
            _ => None,
        };

        let mut remap = vec![usize::MAX; self.nodes.len()];
        let mut nodes = Vec::new();
        let mut bodies = Vec::new();
        for (i, (node, body)) in self.nodes.drain(..).zip(self.bodies.drain(..)).enumerate() {
            if !doomed[i] {
                remap[i] = nodes.len();
                nodes.push(node);
                bodies.push(body);
            }
        }
        self.nodes = nodes;
        self.bodies = bodies;
        self.edges.retain_mut(|e| {
            let keep = !doomed[e.source] && !doomed[e.target];
            e.source = remap[e.source];
            e.target = remap[e.target];
            keep
        });
        self.index = self.nodes.iter().enumerate().map(|(i, n)| (n.id.clone(), i)).collect();
        self.categories.retain(|c| self.nodes.iter().any(|n| &n.category == c));
        self.hovered = None;
        self.drag = None;
        self.selected = if selection_lost {
            None
        } else if let Some(id) = selected_id {
            self.index.get(&id).map(|&i| Item::Node(i))
        } else {
            selected_edge.and_then(|(s, t, l)| {
                self.edges
                    .iter()
                    .position(|e| e.data.source == s && e.data.target == t && e.data.label == l)
                    .map(Item::Edge)
            })
        };
        self.rebuild_topology();
        self.sim.reheat(0.3);
        if selection_lost {
            cx.emit(GraphEvent::SelectionChanged(Selection::None));
        }
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_data(GraphData::default(), cx);
    }

    /// The current graph contents, e.g. for export.
    pub fn data(&self) -> GraphData {
        GraphData {
            nodes: self.nodes.clone(),
            edges: self.edges.iter().map(|e| e.data.clone()).collect(),
        }
    }

    fn load(&mut self, data: GraphData, anchor: Option<&str>) {
        self.nodes.clear();
        self.bodies.clear();
        self.edges.clear();
        self.index.clear();
        self.categories.clear();
        self.hovered = None;
        self.selected = None;
        self.drag = None;
        self.insert(data, anchor);
        self.pan = Point::default();
        self.zoom = 1.;
        self.auto_fit = true;
    }

    fn insert(&mut self, data: GraphData, anchor: Option<&str>) {
        let anchor_pos = anchor.and_then(|id| self.index.get(id)).map(|&i| self.bodies[i].pos);
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
            let (Some(&s), Some(&t)) = (self.index.get(&edge.source), self.index.get(&edge.target)) else {
                continue;
            };
            let duplicate = self.edges.iter().any(|e| {
                e.source == s && e.target == t && e.data.label == edge.label
            });
            if !duplicate {
                self.edges.push(EdgeItem { data: edge, source: s, target: t, curvature: 0. });
            }
        }
        self.rebuild_topology();
        self.sim.reheat(if anchor_pos.is_some() { 0.5 } else { 1. });
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
            body.radius = self.nodes[i].size.unwrap_or_else(|| self.sizing.radius(self.degrees[i]));
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

    // ---- queries ------------------------------------------------------------

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.index.get(id).map(|&i| &self.nodes[i])
    }

    pub fn degree(&self, id: &str) -> usize {
        self.index.get(id).map_or(0, |&i| self.degrees[i])
    }

    pub fn neighbors_of(&self, id: &str) -> Vec<Node> {
        self.index
            .get(id)
            .map(|&i| self.neighbors[i].iter().map(|&j| self.nodes[j].clone()).collect())
            .unwrap_or_default()
    }

    /// Edges touching a node, in either direction.
    pub fn edges_of(&self, id: &str) -> Vec<Edge> {
        let Some(&i) = self.index.get(id) else { return Vec::new() };
        self.edges
            .iter()
            .filter(|e| e.source == i || e.target == i)
            .map(|e| e.data.clone())
            .collect()
    }

    pub fn categories(&self) -> Vec<CategoryInfo> {
        self.categories
            .iter()
            .enumerate()
            .map(|(i, c)| CategoryInfo {
                name: c.clone(),
                color: self.palette.get(i),
                count: self.nodes.iter().filter(|n| &n.category == c).count(),
            })
            .collect()
    }

    /// The color a node is painted with.
    pub fn color_of(&self, node: &Node) -> Hsla {
        node.color.unwrap_or_else(|| {
            let ix = self.categories.iter().position(|c| c == &node.category).unwrap_or(0);
            self.palette.get(ix)
        })
    }

    pub fn selection(&self) -> Selection {
        self.item_selection(self.selected)
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Whether the layout is still moving.
    pub fn is_simulating(&self) -> bool {
        self.sim.is_active()
    }

    fn item_selection(&self, item: Option<Item>) -> Selection {
        match item {
            None => Selection::None,
            Some(Item::Node(i)) => Selection::Node(self.nodes[i].clone()),
            Some(Item::Edge(e)) => Selection::Edge(self.edges[e].data.clone()),
        }
    }

    fn selected_node_id(&self) -> Option<SharedString> {
        match self.selected {
            Some(Item::Node(i)) => Some(self.nodes[i].id.clone()),
            _ => None,
        }
    }

    // ---- selection & viewport ---------------------------------------------

    /// Select a node by id (`None` clears), optionally centering it.
    pub fn select_node(&mut self, id: Option<&str>, center: bool, cx: &mut Context<Self>) {
        let item = id.and_then(|id| self.index.get(id).copied()).map(Item::Node);
        if let (Some(Item::Node(i)), true) = (item, center) {
            let p = self.bodies[i].pos;
            self.auto_fit = false;
            self.pan = point(px(-p.x * self.zoom), px(-p.y * self.zoom));
            cx.notify();
        }
        self.set_selected(item, cx);
    }

    fn set_selected(&mut self, item: Option<Item>, cx: &mut Context<Self>) {
        if item == self.selected {
            return;
        }
        self.selected = item;
        cx.emit(GraphEvent::SelectionChanged(self.item_selection(item)));
        cx.notify();
    }

    /// Pin or release a node at its current position.
    pub fn set_pinned(&mut self, id: &str, pinned: bool, cx: &mut Context<Self>) {
        if let Some(&i) = self.index.get(id) {
            self.bodies[i].fixed = pinned.then_some(self.bodies[i].pos);
            self.sim.reheat(0.1);
            cx.notify();
        }
    }

    pub fn is_pinned(&self, id: &str) -> bool {
        self.index.get(id).is_some_and(|&i| self.bodies[i].fixed.is_some())
    }

    pub fn unpin_all(&mut self, cx: &mut Context<Self>) {
        for b in &mut self.bodies {
            b.fixed = None;
        }
        self.sim.reheat(0.3);
        cx.notify();
    }

    /// Fit everything into view and keep fitting while the layout settles.
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

    fn edge_geometry(&self, e: &EdgeItem) -> EdgeGeometry {
        let a = self.to_screen(self.bodies[e.source].pos);
        let b = self.to_screen(self.bodies[e.target].pos);
        EdgeGeometry::new(a, b, e.curvature * 28. * self.zoom.min(1.5))
    }

    fn hit_test(&self, p: Point<Pixels>) -> Option<Item> {
        let w = self.to_world(p);
        let slop = 3. / self.zoom;
        // Topmost first: nodes are painted in index order, above edges.
        let node = self.bodies.iter().enumerate().rev().find_map(|(i, b)| {
            let (dx, dy) = (b.pos.x - w.x, b.pos.y - w.y);
            let r = b.radius + slop;
            (dx * dx + dy * dy <= r * r).then_some(i)
        });
        if let Some(i) = node {
            return Some(Item::Node(i));
        }
        let mut best = (EDGE_HIT_SLOP, None);
        for (i, e) in self.edges.iter().enumerate() {
            let d = if e.source == e.target {
                let c = self.to_screen(self.bodies[e.source].pos);
                let s = (self.bodies[e.source].radius * self.zoom).max(6.);
                let loop_center = point(c.x + px(s * 1.3), c.y - px(s * 1.3));
                let r = (p - loop_center).x.as_f32().hypot((p - loop_center).y.as_f32());
                (r - s * 0.9).abs()
            } else {
                self.edge_geometry(e).distance_to(p)
            };
            if d < best.0 {
                best = (d, Some(i));
            }
        }
        best.1.map(Item::Edge)
    }

    // ---- input --------------------------------------------------------------

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        let hit = self.hit_test(ev.position);
        if ev.button == MouseButton::Right {
            if hit.is_some() {
                self.set_selected(hit, cx);
            }
            cx.emit(GraphEvent::ContextMenu { target: self.item_selection(hit), position: ev.position });
            return;
        }
        if ev.click_count >= 2 {
            if let Some(Item::Node(i)) = hit {
                cx.emit(GraphEvent::NodeActivated(self.nodes[i].clone()));
            }
            return;
        }
        let kind = match hit {
            Some(Item::Node(index)) => DragKind::Node { index, was_pinned: self.bodies[index].fixed.is_some() },
            _ => DragKind::Pan { start_pan: self.pan },
        };
        self.drag = Some(Drag { kind, start: ev.position, moved: false });
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, inside: bool, cx: &mut Context<Self>) {
        self.pointer = inside.then_some(ev.position);
        if let Some(drag) = &mut self.drag {
            let delta = ev.position - drag.start;
            if !drag.moved {
                if delta.x.as_f32().hypot(delta.y.as_f32()) < DRAG_THRESHOLD {
                    return;
                }
                drag.moved = true;
            }
            match drag.kind {
                DragKind::Pan { start_pan } => {
                    self.auto_fit = false;
                    self.pan = start_pan + delta;
                }
                DragKind::Node { index, .. } => {
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
        let hovered = if inside { self.hit_test(ev.position) } else { None };
        if hovered != self.hovered || hovered.is_some() {
            // Also re-render while hovering so the tooltip follows the pointer.
            self.hovered = hovered;
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, ev: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        match drag.kind {
            DragKind::Node { index, was_pinned } => {
                self.sim.alpha_target = 0.;
                if !drag.moved {
                    self.set_selected(Some(Item::Node(index)), cx);
                } else if !(self.pin_on_drag || was_pinned) {
                    self.bodies[index].fixed = None;
                }
            }
            DragKind::Pan { .. } => {
                if !drag.moved {
                    let hit = self.hit_test(ev.position);
                    self.set_selected(hit, cx);
                }
            }
        }
        cx.notify();
    }

    fn on_scroll(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = ev.delta.pixel_delta(px(20.));
        let factor = (-delta.y.as_f32() * 0.0025).exp();
        self.auto_fit = false;
        self.zoom_around(ev.position, factor);
        cx.notify();
    }

    // ---- painting -----------------------------------------------------------

    fn node_color(&self, i: usize) -> Hsla {
        self.color_of(&self.nodes[i])
    }

    fn paint_graph(&self, bounds: Bounds<Pixels>, colors: &GraphColors, window: &mut Window, cx: &mut App) {
        let zoom = self.zoom;
        let focus = self.hovered.or(self.selected);
        let in_focus = |i: usize| match focus {
            None => true,
            Some(Item::Node(f)) => i == f || self.neighbors[f].binary_search(&i).is_ok(),
            Some(Item::Edge(e)) => i == self.edges[e].source || i == self.edges[e].target,
        };
        let visible = bounds.dilate(px(40.));

        // Edges are batched into one path per state to keep draw calls low.
        #[derive(Clone, Copy, PartialEq)]
        enum State {
            Normal,
            Dimmed,
            Active,
            Selected,
        }
        let stroke = (1.2 * zoom.sqrt()).clamp(0.6, 2.);
        let mut lines: Vec<(PathBuilder, PathBuilder, bool)> = [1., 1., 1.6, 2.2]
            .iter()
            .map(|w| (PathBuilder::stroke(px(stroke * w)), PathBuilder::fill(), false))
            .collect();
        let mut edge_labels = Vec::new();

        for (ei, e) in self.edges.iter().enumerate() {
            let geom = self.edge_geometry(e);
            let (a, b) = (geom.eval(0.), geom.eval(1.));
            if !segment_may_intersect(visible, a, b) {
                continue;
            }
            let state = if self.selected == Some(Item::Edge(ei)) {
                State::Selected
            } else {
                match focus {
                    None => State::Normal,
                    Some(Item::Node(f)) if e.source == f || e.target == f => State::Active,
                    Some(Item::Edge(x)) if x == ei => State::Active,
                    Some(_) => State::Dimmed,
                }
            };
            let (path, arrows, used) = &mut lines[state as usize];
            *used = true;
            let rs = self.bodies[e.source].radius * zoom;
            let rt = self.bodies[e.target].radius * zoom;
            if e.source == e.target {
                draw_self_loop(path, a, rs);
                continue;
            }
            let arrow = 6. + stroke * 2.;
            let gap = if self.show_arrows { rt + arrow } else { rt };
            path.move_to(geom.point_from_start(rs));
            let end = geom.point_from_end(gap);
            match geom.ctrl {
                Some(ctrl) => path.curve_to(end, ctrl),
                None => path.line_to(end),
            }
            if self.show_arrows && zoom > 0.25 {
                let tip = geom.point_from_end(rt);
                let (dx, dy) = geom.end_direction();
                let (nx, ny) = (-dy * arrow * 0.45, dx * arrow * 0.45);
                let (bx, by) = (tip.x.as_f32() - dx * arrow, tip.y.as_f32() - dy * arrow);
                arrows.add_polygon(
                    &[tip, point(px(bx + nx), px(by + ny)), point(px(bx - nx), px(by - ny))],
                    true,
                );
            }
            let emphasized = matches!(state, State::Active | State::Selected);
            let label_visible = self.show_edge_labels
                && !e.data.label.is_empty()
                && (emphasized || (state == State::Normal && zoom >= 1.1));
            if label_visible {
                edge_labels.push((geom.midpoint(), e.data.label.clone(), emphasized));
            }
        }

        let edge_colors = [colors.edge, colors.edge.opacity(0.15), colors.edge_active, colors.selection];
        // Dimmed first so emphasized edges land on top.
        for state in [State::Dimmed, State::Normal, State::Active, State::Selected] {
            let (path, arrows, used) = std::mem::replace(
                &mut lines[state as usize],
                (PathBuilder::fill(), PathBuilder::fill(), false),
            );
            if !used {
                continue;
            }
            let color = edge_colors[state as usize];
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
            if let Ok(arrows) = arrows.build() {
                window.paint_path(arrows, color);
            }
        }

        let label_size = px(11.);
        for (pos, text, active) in edge_labels {
            let color = if active { colors.foreground } else { colors.muted };
            let line = shape(&text, label_size, color, window);
            let w = line.width;
            let chip = Bounds::new(point(pos.x - w / 2. - px(3.), pos.y - px(8.)), size(w + px(6.), px(16.)));
            window.paint_quad(gpui::fill(chip, colors.background.opacity(0.85)).corner_radii(px(3.)));
            let _ = line.paint(point(pos.x - w / 2., pos.y - px(7.)), px(14.), TextAlign::Left, None, window, cx);
        }

        // Nodes, then labels above every node.
        let mut labels = Vec::new();
        for (i, b) in self.bodies.iter().enumerate() {
            let c = self.to_screen(b.pos);
            let r = b.radius * zoom;
            if !visible.contains(&c) && r < 200. {
                continue;
            }
            let base = self.node_color(i);
            let faded = !in_focus(i);
            let is_selected = self.selected == Some(Item::Node(i));
            let is_hovered = self.hovered == Some(Item::Node(i));
            let ring = if is_selected { (r * 0.18).clamp(2., 4.) } else { (r * 0.12).clamp(1., 2.) };
            let border = if is_selected {
                colors.selection
            } else if faded {
                colors.background.opacity(0.4)
            } else {
                colors.background
            };
            let rr = r + if is_hovered { 2. } else { 0. };
            if is_selected || is_hovered {
                let halo = rr + 6.;
                window.paint_quad(gpui::fill(circle(c, halo), base.opacity(0.25)).corner_radii(px(halo)));
            }
            let fill = if faded { base.opacity(0.2) } else { base };
            window.paint_quad(quad(circle(c, rr), px(rr), fill, px(ring), border, BorderStyle::Solid));
            if b.fixed.is_some() && rr > 6. {
                // Small pin marker for nodes that will not move with the layout.
                let pin = (rr * 0.22).clamp(2., 4.);
                window.paint_quad(gpui::fill(circle(c, pin), colors.background.opacity(0.9)).corner_radii(px(pin)));
            }

            let show_label = match self.label_mode {
                LabelMode::Always => true,
                LabelMode::Auto => (focus.is_none() && zoom >= 0.7) || (focus.is_some() && !faded),
                LabelMode::Focus => focus.is_some() && !faded,
                LabelMode::Never => false,
            };
            if show_label && !self.nodes[i].label.is_empty() {
                labels.push((c, rr, i, faded, is_selected || is_hovered));
            }
        }

        let font_size = px((12. * zoom.sqrt()).clamp(10., 14.));
        for (c, r, i, faded, strong) in labels {
            let text = truncate(&self.nodes[i].label, 24);
            let color = if faded {
                colors.muted.opacity(0.4)
            } else if strong {
                colors.foreground
            } else {
                colors.muted
            };
            let line = shape(&text, font_size, color, window);
            let origin = point(c.x - line.width / 2., c.y + px(r) + px(3.));
            if strong {
                let chip = Bounds::new(origin - point(px(4.), px(1.)), size(line.width + px(8.), font_size + px(4.)));
                window.paint_quad(gpui::fill(chip, colors.background.opacity(0.9)).corner_radii(px(3.)));
            }
            let _ = line.paint(origin, font_size + px(2.), TextAlign::Left, None, window, cx);
        }
    }

    fn tooltip(&self, colors: &GraphColors, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        if !self.show_tooltip || self.drag.as_ref().is_some_and(|d| d.moved) {
            return None;
        }
        let item = self.hovered?;
        let pointer = self.pointer?;
        let selection = self.item_selection(Some(item));
        let content = match &self.tooltip {
            Some(builder) => builder(&selection, colors, window, cx)?,
            None => self.default_tooltip(item, colors).into_any_element(),
        };
        let origin = self.bounds.get().origin;
        Some(
            div()
                .absolute()
                .left(pointer.x - origin.x + px(14.))
                .top(pointer.y - origin.y + px(14.))
                .child(content)
                .into_any_element(),
        )
    }

    fn default_tooltip(&self, item: Item, colors: &GraphColors) -> impl IntoElement {
        let (swatch, kicker, title, detail) = match item {
            Item::Node(i) => {
                let node = &self.nodes[i];
                (
                    Some(self.node_color(i)),
                    node.category.clone(),
                    node.label.clone(),
                    format!("{} relationships", self.degrees[i]),
                )
            }
            Item::Edge(e) => {
                let e = &self.edges[e];
                (
                    None,
                    "Relationship".into(),
                    e.data.label.clone(),
                    format!("{} → {}", self.nodes[e.source].label, self.nodes[e.target].label),
                )
            }
        };
        div()
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
                    .children(swatch.map(|c| div().size_2().rounded_full().bg(c)))
                    .child(div().text_color(colors.muted).child(kicker)),
            )
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
            .child(div().text_color(colors.muted).child(detail))
    }
}

impl Render for NetworkGraph {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.sim.is_active() && !self.bodies.is_empty() {
            // Small graphs can afford two ticks per frame and settle twice as fast.
            let ticks = if self.bodies.len() > 1500 { 1 } else { 2 };
            for _ in 0..ticks {
                self.sim.tick(&mut self.bodies, &self.links, &self.degrees);
            }
            if self.auto_fit {
                self.apply_fit(0.25);
            }
            window.request_animation_frame();
        }

        let colors = self.colors.clone().unwrap_or_else(|| GraphColors::from_theme(cx));
        let tooltip = self.tooltip(&colors, window, cx);
        let entity = cx.entity();
        let bounds_cell = self.bounds.clone();
        let background = colors.background;

        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(background)
            .child(
                canvas(
                    move |bounds, window, _| {
                        bounds_cell.set(bounds);
                        window.insert_hitbox(bounds, HitboxBehavior::Normal)
                    },
                    move |bounds, hitbox: Hitbox, window, cx| {
                        register_input(&entity, &hitbox, window, cx);
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            entity.update(cx, |this, cx| this.paint_graph(bounds, &colors, window, cx));
                        });
                    },
                )
                .size_full(),
            )
            .children(tooltip)
    }
}

/// Mouse handling is registered at the window level during paint so drags keep
/// tracking outside the graph's bounds.
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
        let button_ok = matches!(ev.button, MouseButton::Left | MouseButton::Right);
        if phase == DispatchPhase::Bubble && button_ok && h.is_hovered(window) {
            e.update(cx, |this, cx| this.on_mouse_down(ev, cx));
        }
    });
    let (e, h) = (entity.clone(), hitbox.clone());
    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble {
            return;
        }
        let inside = h.is_hovered(window);
        let graph = e.read(cx);
        if inside || graph.drag.is_some() || graph.hovered.is_some() {
            e.update(cx, |this, cx| this.on_mouse_move(ev, inside, cx));
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

fn circle(c: Point<Pixels>, r: f32) -> Bounds<Pixels> {
    Bounds::new(point(c.x - px(r), c.y - px(r)), size(px(r * 2.), px(r * 2.)))
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
    window.text_system().shape_line(text.clone(), font_size, &[run], None)
}

fn truncate(s: &SharedString, max: usize) -> SharedString {
    if s.chars().count() <= max {
        return s.clone();
    }
    let t: String = s.chars().take(max - 1).collect();
    format!("{t}…").into()
}
