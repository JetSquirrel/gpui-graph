mod paint;
mod visual;

use std::{cell::Cell, collections::HashMap, rc::Rc};

use gpui::{
    AnyElement, App, Bounds, ContentMask, Context, CursorStyle, DispatchPhase, Entity,
    EventEmitter, Hitbox, HitboxBehavior, Hsla, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render, ScrollWheelEvent,
    SharedString, Styled as _, Window, canvas, div, point, px,
};

use crate::geometry::EdgeGeometry;
use crate::layout::{Body, ForceConfig, Simulation, Vec2};
use crate::model::{Edge, GraphData, Node};
use crate::style::{ColorBy, GraphColors, LabelMode, NodeSizing, Palette};
use visual::{GroupInput, Vertex, Visual};

const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 6.;
/// Pointer travel (px) before a press becomes a drag instead of a click.
const DRAG_THRESHOLD: f32 = 3.;
/// Pointer distance (px) within which an edge counts as hit.
const EDGE_HIT_SLOP: f32 = 4.;

/// What the user has selected or is pointing at.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub enum Selection {
    #[default]
    None,
    Node(Node),
    Edge(Edge),
    /// Several edges drawn as one, e.g. from a node into a collapsed group.
    EdgeBundle(Vec<Edge>),
    /// A collapsed group's super node.
    Group(GroupInfo),
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum GraphEvent {
    SelectionChanged(Selection),
    /// Double-click on a node: the usual "expand neighbors" gesture in graph explorers.
    NodeActivated(Node),
    /// A group was collapsed or expanded, by the user or through the API.
    GroupToggled { group: SharedString, collapsed: bool },
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

/// A group (community) present in the graph.
#[derive(Clone, Debug)]
pub struct GroupInfo {
    pub id: SharedString,
    pub label: SharedString,
    pub color: Hsla,
    /// Number of member nodes.
    pub size: usize,
    pub collapsed: bool,
}

/// Builds tooltip content for the hovered item; `None` hides the tooltip.
pub type TooltipBuilder =
    Rc<dyn Fn(&Selection, &GraphColors, &mut Window, &mut App) -> Option<AnyElement>>;

/// A hovered / hit item, by index into the visual graph. Transient.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Vertex(usize),
    Edge(usize),
}

/// A selected item by stable identity, so it survives collapse/expand and merges.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SelectionKey {
    Node(SharedString),
    Group(SharedString),
    /// Model edge indices of the selected visual edge.
    Edges(Vec<usize>),
}

struct ModelEdge {
    data: Edge,
    source: usize,
    target: usize,
}

struct GroupState {
    id: SharedString,
    label: SharedString,
    members: Vec<usize>,
    collapsed: bool,
    /// Super node body while collapsed.
    body: Body,
    /// Members' centroid when collapsed; expanding moves them by the super node's travel.
    anchor: Vec2,
}

enum DragKind {
    Pan { start_pan: Point<Pixels> },
    Vertex { index: usize, was_pinned: bool },
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
    // Model: every node and edge, with persisted positions.
    nodes: Vec<Node>,
    node_bodies: Vec<Body>,
    edges: Vec<ModelEdge>,
    index: HashMap<SharedString, usize>,
    degrees: Vec<usize>,
    neighbors: Vec<Vec<usize>>,
    categories: Vec<SharedString>,
    groups: Vec<GroupState>,
    group_index: HashMap<SharedString, usize>,
    node_group: Vec<Option<usize>>,

    // What is drawn and simulated.
    vis: Visual,
    sim: Simulation,

    /// Screen position = bounds.center + pan + world * zoom.
    pan: Point<Pixels>,
    zoom: f32,
    /// Keep fitting the view while the layout settles, until the user moves it.
    auto_fit: bool,
    bounds: Rc<Cell<Bounds<Pixels>>>,

    hovered: Option<Item>,
    selected: Option<SelectionKey>,
    drag: Option<Drag>,
    pointer: Option<Point<Pixels>>,

    label_mode: LabelMode,
    color_by: ColorBy,
    show_arrows: bool,
    show_edge_labels: bool,
    show_group_hulls: bool,
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
            node_bodies: Vec::new(),
            edges: Vec::new(),
            index: HashMap::new(),
            degrees: Vec::new(),
            neighbors: Vec::new(),
            categories: Vec::new(),
            groups: Vec::new(),
            group_index: HashMap::new(),
            node_group: Vec::new(),
            vis: Visual::default(),
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
            color_by: ColorBy::default(),
            show_arrows: true,
            show_edge_labels: true,
            show_group_hulls: true,
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
        self.load(data);
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

    pub fn with_color_by(mut self, color_by: ColorBy) -> Self {
        self.color_by = color_by;
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

    /// Outline each expanded group with a rounded hull (default on).
    pub fn with_group_hulls(mut self, show: bool) -> Self {
        self.show_group_hulls = show;
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

    pub fn color_by(&self) -> ColorBy {
        self.color_by
    }

    pub fn set_color_by(&mut self, color_by: ColorBy, cx: &mut Context<Self>) {
        self.color_by = color_by;
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

    pub fn show_group_hulls(&self) -> bool {
        self.show_group_hulls
    }

    pub fn set_show_group_hulls(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_group_hulls = show;
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
        self.load(data);
        if had_selection {
            cx.emit(GraphEvent::SelectionChanged(Selection::None));
        }
        cx.notify();
    }

    /// Add nodes and edges to the current graph, e.g. expanding a node. Existing
    /// ids are kept; new nodes start around `anchor` so the layout stays stable.
    /// New members of a collapsed group join it without expanding it.
    pub fn merge(&mut self, data: GraphData, anchor: Option<&str>, cx: &mut Context<Self>) {
        self.sync_back();
        self.insert(data, anchor);
        cx.notify();
    }

    /// Remove nodes and every edge touching them.
    pub fn remove_nodes(&mut self, ids: &[&str], cx: &mut Context<Self>) {
        let doomed: Vec<bool> = self.nodes.iter().map(|n| ids.contains(&n.id.as_ref())).collect();
        if !doomed.contains(&true) {
            return;
        }
        self.sync_back();
        let mut remap = vec![usize::MAX; self.nodes.len()];
        let mut kept = 0;
        for (i, &d) in doomed.iter().enumerate() {
            if !d {
                remap[i] = kept;
                kept += 1;
            }
        }
        let mut i = 0;
        self.nodes.retain(|_| (!doomed[i], i += 1).0);
        let mut i = 0;
        self.node_bodies.retain(|_| (!doomed[i], i += 1).0);
        self.edges.retain_mut(|e| {
            let keep = !doomed[e.source] && !doomed[e.target];
            e.source = remap[e.source];
            e.target = remap[e.target];
            keep
        });
        self.index = self.nodes.iter().enumerate().map(|(i, n)| (n.id.clone(), i)).collect();
        self.categories.retain(|c| self.nodes.iter().any(|n| &n.category == c));
        // Model edge indices shifted; an edge selection cannot be carried over.
        if matches!(self.selected, Some(SelectionKey::Edges(_))) {
            self.selected = None;
        }
        self.rebuild();
        self.sim.reheat(0.3);
        self.validate_selection(cx);
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_data(GraphData::default(), cx);
    }

    /// The current graph contents (including nodes inside collapsed groups).
    pub fn data(&self) -> GraphData {
        GraphData {
            nodes: self.nodes.clone(),
            edges: self.edges.iter().map(|e| e.data.clone()).collect(),
        }
    }

    fn load(&mut self, data: GraphData) {
        self.nodes.clear();
        self.node_bodies.clear();
        self.edges.clear();
        self.index.clear();
        self.categories.clear();
        self.groups.clear();
        self.group_index.clear();
        self.vis = Visual::default();
        self.hovered = None;
        self.selected = None;
        self.drag = None;
        self.insert(data, None);
        self.pan = Point::default();
        self.zoom = 1.;
        self.auto_fit = true;
    }

    /// Requires the model to be in sync with the visual graph (see `sync_back`).
    fn insert(&mut self, data: GraphData, anchor: Option<&str>) {
        let anchor_pos = anchor
            .and_then(|id| self.index.get(id))
            .map(|&i| self.vis.bodies[self.vis.node_slot[i]].pos);
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
            self.node_bodies.push(Body::default());
        }
        match anchor_pos {
            Some(center) => {
                let added = self.nodes.len() - first_new;
                for (k, body) in self.node_bodies[first_new..].iter_mut().enumerate() {
                    let a = k as f32 / added.max(1) as f32 * std::f32::consts::TAU;
                    body.pos = Vec2::new(center.x + 30. * a.cos(), center.y + 30. * a.sin());
                }
            }
            None => Simulation::initialize_positions(&mut self.node_bodies, first_new),
        }

        for edge in data.edges {
            let (Some(&s), Some(&t)) = (self.index.get(&edge.source), self.index.get(&edge.target)) else {
                continue;
            };
            let duplicate = self.edges.iter().any(|e| e.source == s && e.target == t && e.data.label == edge.label);
            if !duplicate {
                self.edges.push(ModelEdge { data: edge, source: s, target: t });
            }
        }
        self.rebuild();
        self.sim.reheat(if anchor_pos.is_some() { 0.5 } else { 1. });
    }

    /// Persist simulated positions from the visual graph into the model.
    fn sync_back(&mut self) {
        for (v, vertex) in self.vis.vertices.iter().enumerate() {
            let body = self.vis.bodies[v].clone();
            match *vertex {
                Vertex::Node(i) => self.node_bodies[i] = body,
                Vertex::Group(g) => self.groups[g].body = body,
            }
        }
    }

    /// Recompute model topology and groups, then the visual graph.
    fn rebuild(&mut self) {
        let n = self.nodes.len();
        self.degrees = vec![0; n];
        self.neighbors = vec![Vec::new(); n];
        for e in &self.edges {
            self.degrees[e.source] += 1;
            self.degrees[e.target] += 1;
            if e.source != e.target {
                self.neighbors[e.source].push(e.target);
                self.neighbors[e.target].push(e.source);
            }
        }
        for list in &mut self.neighbors {
            list.sort_unstable();
            list.dedup();
        }
        for (i, body) in self.node_bodies.iter_mut().enumerate() {
            body.radius = self.nodes[i].size.unwrap_or_else(|| self.sizing.radius(self.degrees[i]));
        }

        // Groups keep their state (collapsed, position, label) across rebuilds.
        for g in &mut self.groups {
            g.members.clear();
        }
        self.node_group = vec![None; n];
        for (i, node) in self.nodes.iter().enumerate() {
            let Some(id) = &node.group else { continue };
            let g = *self.group_index.entry(id.clone()).or_insert_with(|| {
                self.groups.push(GroupState {
                    id: id.clone(),
                    label: id.clone(),
                    members: Vec::new(),
                    collapsed: false,
                    body: Body::default(),
                    anchor: Vec2::default(),
                });
                self.groups.len() - 1
            });
            self.groups[g].members.push(i);
            self.node_group[i] = Some(g);
        }
        if self.groups.iter().any(|g| g.members.is_empty()) {
            self.groups.retain(|g| !g.members.is_empty());
            self.group_index = self.groups.iter().enumerate().map(|(g, s)| (s.id.clone(), g)).collect();
            for (i, node) in self.nodes.iter().enumerate() {
                self.node_group[i] = node.group.as_ref().map(|id| self.group_index[id]);
            }
        }
        for g in &mut self.groups {
            g.body.radius = (12. + (g.members.len() as f32).sqrt() * 3.).min(48.);
        }

        let groups: Vec<GroupInput> = self
            .groups
            .iter()
            .map(|g| GroupInput { collapsed: g.collapsed, body: &g.body })
            .collect();
        let links: Vec<(usize, usize)> = self.edges.iter().map(|e| (e.source, e.target)).collect();
        self.vis = Visual::build(&self.node_bodies, &self.node_group, &groups, &links);
        self.hovered = None;
        self.drag = None;
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

    pub fn groups(&self) -> Vec<GroupInfo> {
        (0..self.groups.len()).map(|g| self.group_info(g)).collect()
    }

    pub fn group(&self, id: &str) -> Option<GroupInfo> {
        self.group_index.get(id).map(|&g| self.group_info(g))
    }

    /// Member nodes of a group.
    pub fn group_members(&self, id: &str) -> Vec<Node> {
        self.group_index
            .get(id)
            .map(|&g| self.groups[g].members.iter().map(|&i| self.nodes[i].clone()).collect())
            .unwrap_or_default()
    }

    /// The color a node is painted with.
    pub fn color_of(&self, node: &Node) -> Hsla {
        if let Some(c) = node.color {
            return c;
        }
        match self.color_by {
            ColorBy::Category => {
                let ix = self.categories.iter().position(|c| c == &node.category).unwrap_or(0);
                self.palette.get(ix)
            }
            ColorBy::Group => match node.group.as_ref().and_then(|g| self.group_index.get(g)) {
                Some(&g) => self.group_color(g),
                None => gpui::hsla(0., 0., 0.62, 1.),
            },
        }
    }

    pub fn selection(&self) -> Selection {
        match &self.selected {
            None => Selection::None,
            Some(SelectionKey::Node(id)) => self.node(id).cloned().map_or(Selection::None, Selection::Node),
            Some(SelectionKey::Group(id)) => self.group(id).map_or(Selection::None, Selection::Group),
            Some(SelectionKey::Edges(edges)) => self.edge_selection(edges),
        }
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Whether the layout is still moving.
    pub fn is_simulating(&self) -> bool {
        self.sim.is_active()
    }

    fn group_info(&self, g: usize) -> GroupInfo {
        let s = &self.groups[g];
        GroupInfo {
            id: s.id.clone(),
            label: s.label.clone(),
            color: self.group_color(g),
            size: s.members.len(),
            collapsed: s.collapsed,
        }
    }

    fn group_color(&self, g: usize) -> Hsla {
        self.palette.get(g)
    }

    fn edge_selection(&self, edges: &[usize]) -> Selection {
        match edges {
            [] => Selection::None,
            [e] => Selection::Edge(self.edges[*e].data.clone()),
            many => Selection::EdgeBundle(many.iter().map(|&e| self.edges[e].data.clone()).collect()),
        }
    }

    fn key_of(&self, item: Item) -> SelectionKey {
        match item {
            Item::Vertex(v) => match self.vis.vertices[v] {
                Vertex::Node(i) => SelectionKey::Node(self.nodes[i].id.clone()),
                Vertex::Group(g) => SelectionKey::Group(self.groups[g].id.clone()),
            },
            Item::Edge(e) => SelectionKey::Edges(self.vis.edges[e].edges.clone()),
        }
    }

    fn item_selection(&self, item: Item) -> Selection {
        match item {
            Item::Vertex(v) => match self.vis.vertices[v] {
                Vertex::Node(i) => Selection::Node(self.nodes[i].clone()),
                Vertex::Group(g) => Selection::Group(self.group_info(g)),
            },
            Item::Edge(e) => self.edge_selection(&self.vis.edges[e].edges),
        }
    }

    /// The selection as a visual item, if it is currently drawn.
    fn selected_item(&self) -> Option<Item> {
        match self.selected.as_ref()? {
            SelectionKey::Node(id) => {
                let slot = self.vis.node_slot[*self.index.get(id)?];
                matches!(self.vis.vertices[slot], Vertex::Node(_)).then_some(Item::Vertex(slot))
            }
            SelectionKey::Group(id) => self.vis.group_slot[*self.group_index.get(id)?].map(Item::Vertex),
            SelectionKey::Edges(edges) => {
                self.vis.edges.iter().position(|e| &e.edges == edges).map(Item::Edge)
            }
        }
    }

    /// Drop a selection whose target no longer exists, notifying listeners.
    fn validate_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected.is_some() && matches!(self.selection(), Selection::None) {
            self.selected = None;
            cx.emit(GraphEvent::SelectionChanged(Selection::None));
        }
    }

    // ---- selection & viewport ---------------------------------------------

    /// Select a node by id (`None` clears), optionally centering it.
    pub fn select_node(&mut self, id: Option<&str>, center: bool, cx: &mut Context<Self>) {
        let key = id.filter(|id| self.index.contains_key(*id)).map(|id| SelectionKey::Node(id.to_string().into()));
        if let (Some(id), true) = (id, center) {
            if let Some(&i) = self.index.get(id) {
                let p = self.vis.bodies[self.vis.node_slot[i]].pos;
                self.auto_fit = false;
                self.pan = point(px(-p.x * self.zoom), px(-p.y * self.zoom));
                cx.notify();
            }
        }
        self.set_selected(key, cx);
    }

    fn set_selected(&mut self, key: Option<SelectionKey>, cx: &mut Context<Self>) {
        if key == self.selected {
            return;
        }
        self.selected = key;
        cx.emit(GraphEvent::SelectionChanged(self.selection()));
        cx.notify();
    }

    /// Pin or release a node at its current position.
    pub fn set_pinned(&mut self, id: &str, pinned: bool, cx: &mut Context<Self>) {
        let Some(&i) = self.index.get(id) else { return };
        let v = self.vis.node_slot[i];
        if matches!(self.vis.vertices[v], Vertex::Node(_)) {
            let body = &mut self.vis.bodies[v];
            body.fixed = pinned.then_some(body.pos);
        } else {
            let body = &mut self.node_bodies[i];
            body.fixed = pinned.then_some(body.pos);
        }
        self.sim.reheat(0.1);
        cx.notify();
    }

    pub fn is_pinned(&self, id: &str) -> bool {
        self.index.get(id).is_some_and(|&i| {
            let v = self.vis.node_slot[i];
            match self.vis.vertices[v] {
                Vertex::Node(_) => self.vis.bodies[v].fixed.is_some(),
                Vertex::Group(_) => self.node_bodies[i].fixed.is_some(),
            }
        })
    }

    pub fn unpin_all(&mut self, cx: &mut Context<Self>) {
        for b in self.vis.bodies.iter_mut().chain(&mut self.node_bodies) {
            b.fixed = None;
        }
        self.sim.reheat(0.3);
        cx.notify();
    }

    // ---- groups -------------------------------------------------------------

    /// Collapse a group into a single super node.
    pub fn collapse_group(&mut self, id: &str, cx: &mut Context<Self>) {
        self.set_groups_collapsed(|g| g.id.as_ref() == id, true, cx);
    }

    pub fn expand_group(&mut self, id: &str, cx: &mut Context<Self>) {
        self.set_groups_collapsed(|g| g.id.as_ref() == id, false, cx);
    }

    pub fn toggle_group(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(&g) = self.group_index.get(id) {
            let collapse = !self.groups[g].collapsed;
            self.set_groups_collapsed(|s| s.id.as_ref() == id, collapse, cx);
        }
    }

    /// Community overview: every group becomes one node.
    pub fn collapse_all_groups(&mut self, cx: &mut Context<Self>) {
        self.set_groups_collapsed(|_| true, true, cx);
    }

    pub fn expand_all_groups(&mut self, cx: &mut Context<Self>) {
        self.set_groups_collapsed(|_| true, false, cx);
    }

    pub fn set_group_label(&mut self, id: &str, label: impl Into<SharedString>, cx: &mut Context<Self>) {
        if let Some(&g) = self.group_index.get(id) {
            self.groups[g].label = label.into();
            cx.notify();
        }
    }

    fn set_groups_collapsed(
        &mut self,
        filter: impl Fn(&GroupState) -> bool,
        collapse: bool,
        cx: &mut Context<Self>,
    ) {
        let targets: Vec<usize> = (0..self.groups.len())
            .filter(|&g| filter(&self.groups[g]) && self.groups[g].collapsed != collapse)
            .collect();
        if targets.is_empty() {
            return;
        }
        self.sync_back();
        for &g in &targets {
            let members = self.groups[g].members.clone();
            if collapse {
                let n = members.len().max(1) as f32;
                let (sx, sy) = members.iter().fold((0., 0.), |(x, y), &i| {
                    (x + self.node_bodies[i].pos.x, y + self.node_bodies[i].pos.y)
                });
                let centroid = Vec2::new(sx / n, sy / n);
                let group = &mut self.groups[g];
                group.anchor = centroid;
                group.body.pos = centroid;
                group.body.vel = Vec2::default();
                group.body.fixed = None;
            } else {
                let group = &self.groups[g];
                let (dx, dy) = (group.body.pos.x - group.anchor.x, group.body.pos.y - group.anchor.y);
                for i in members {
                    let b = &mut self.node_bodies[i];
                    b.pos = Vec2::new(b.pos.x + dx, b.pos.y + dy);
                    b.vel = Vec2::default();
                    if let Some(f) = b.fixed {
                        b.fixed = Some(Vec2::new(f.x + dx, f.y + dy));
                    }
                }
            }
            self.groups[g].collapsed = collapse;
        }
        // A selected super node no longer exists once expanded.
        if let (false, Some(SelectionKey::Group(id))) = (collapse, &self.selected) {
            if targets.iter().any(|&g| &self.groups[g].id == id) {
                self.selected = None;
                cx.emit(GraphEvent::SelectionChanged(Selection::None));
            }
        }
        self.rebuild();
        self.sim.reheat(0.4);
        for g in targets {
            cx.emit(GraphEvent::GroupToggled { group: self.groups[g].id.clone(), collapsed: collapse });
        }
        cx.notify();
    }

    // ---- viewport -----------------------------------------------------------

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
        for b in &mut self.vis.bodies {
            b.fixed = None;
        }
        Simulation::initialize_positions(&mut self.vis.bodies, 0);
        self.sim.reheat(1.);
        self.auto_fit = true;
        cx.notify();
    }

    fn apply_fit(&mut self, blend: f32) {
        let bounds = self.bounds.get();
        if self.vis.bodies.is_empty() || bounds.size.width <= px(0.) {
            return;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for b in &self.vis.bodies {
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

    fn edge_geometry(&self, e: usize) -> EdgeGeometry {
        let e = &self.vis.edges[e];
        let a = self.to_screen(self.vis.bodies[e.source].pos);
        let b = self.to_screen(self.vis.bodies[e.target].pos);
        EdgeGeometry::new(a, b, e.curvature * 28. * self.zoom.min(1.5))
    }

    fn hit_test(&self, p: Point<Pixels>) -> Option<Item> {
        let w = self.to_world(p);
        let slop = 3. / self.zoom;
        // Topmost first: vertices are painted in index order, above edges.
        let vertex = self.vis.bodies.iter().enumerate().rev().find_map(|(i, b)| {
            let (dx, dy) = (b.pos.x - w.x, b.pos.y - w.y);
            let r = b.radius + slop;
            (dx * dx + dy * dy <= r * r).then_some(i)
        });
        if let Some(v) = vertex {
            return Some(Item::Vertex(v));
        }
        let mut best = (EDGE_HIT_SLOP, None);
        for (i, e) in self.vis.edges.iter().enumerate() {
            let d = if e.source == e.target {
                let c = self.to_screen(self.vis.bodies[e.source].pos);
                let s = (self.vis.bodies[e.source].radius * self.zoom).max(6.);
                let center = point(c.x + px(s * 1.3), c.y - px(s * 1.3));
                let r = (p.x - center.x).as_f32().hypot((p.y - center.y).as_f32());
                (r - s * 0.9).abs()
            } else {
                self.edge_geometry(i).distance_to(p)
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
            if let Some(item) = hit {
                self.set_selected(Some(self.key_of(item)), cx);
            }
            let target = hit.map_or(Selection::None, |item| self.item_selection(item));
            cx.emit(GraphEvent::ContextMenu { target, position: ev.position });
            return;
        }
        if ev.click_count >= 2 {
            match hit {
                Some(Item::Vertex(v)) => match self.vis.vertices[v] {
                    Vertex::Node(i) => cx.emit(GraphEvent::NodeActivated(self.nodes[i].clone())),
                    Vertex::Group(g) => {
                        let id = self.groups[g].id.clone();
                        self.expand_group(&id, cx);
                    }
                },
                _ => {}
            }
            return;
        }
        let kind = match hit {
            Some(Item::Vertex(index)) => {
                DragKind::Vertex { index, was_pinned: self.vis.bodies[index].fixed.is_some() }
            }
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
                DragKind::Vertex { index, .. } => {
                    let w = self.to_world(ev.position);
                    self.vis.bodies[index].fixed = Some(w);
                    self.vis.bodies[index].pos = w;
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
            DragKind::Vertex { index, was_pinned } => {
                self.sim.alpha_target = 0.;
                if !drag.moved {
                    let key = self.key_of(Item::Vertex(index));
                    self.set_selected(Some(key), cx);
                } else if !(self.pin_on_drag || was_pinned) {
                    self.vis.bodies[index].fixed = None;
                }
            }
            DragKind::Pan { .. } => {
                if !drag.moved {
                    let key = self.hit_test(ev.position).map(|item| self.key_of(item));
                    self.set_selected(key, cx);
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
}

impl Render for NetworkGraph {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.sim.is_active() && !self.vis.bodies.is_empty() {
            // Small graphs can afford two ticks per frame and settle twice as fast.
            let ticks = if self.vis.bodies.len() > 1500 { 1 } else { 2 };
            let vis = &mut self.vis;
            for _ in 0..ticks {
                self.sim.tick(&mut vis.bodies, &vis.links, &vis.degrees, &vis.clusters);
            }
            if self.auto_fit {
                self.apply_fit(0.25);
            }
            window.request_animation_frame();
        }

        let colors = self.colors.clone().unwrap_or_else(|| GraphColors::from_theme(cx));
        let tooltip = self.tooltip_element(&colors, window, cx);
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
