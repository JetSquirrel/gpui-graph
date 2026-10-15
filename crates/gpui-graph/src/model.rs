use gpui::{Hsla, SharedString};

/// A vertex in the graph. Build with [`Node::new`] and the builder methods.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Node {
    /// Unique id; edges refer to nodes by this.
    pub id: SharedString,
    /// Text drawn under the node.
    pub label: SharedString,
    /// Vertex type (e.g. `Person`). Nodes of one category share a palette color.
    pub category: SharedString,
    /// Arbitrary key/value pairs, carried along for the host app (details panels, tooltips).
    pub properties: Vec<(SharedString, SharedString)>,
    /// Community / cluster id, typically computed by the database (Louvain,
    /// label propagation, ...). Members are drawn together, outlined by a hull,
    /// and can be collapsed into one node.
    pub group: Option<SharedString>,
    /// Overrides the category / group color.
    pub color: Option<Hsla>,
    /// Overrides [`NodeSizing`](crate::NodeSizing) with a fixed radius, in world units.
    pub size: Option<f32>,
}

impl Node {
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        category: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            category: category.into(),
            properties: Vec::new(),
            group: None,
            color: None,
            size: None,
        }
    }

    pub fn property(mut self, key: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        self.properties.push((key.into(), value.into()));
        self
    }

    pub fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn size(mut self, radius: f32) -> Self {
        self.size = Some(radius);
        self
    }
}

/// A directed relationship between two nodes. Parallel edges and self-loops are allowed.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Edge {
    pub source: SharedString,
    pub target: SharedString,
    /// Relationship type (e.g. `ACTED_IN`), drawn at the midpoint.
    pub label: SharedString,
    pub properties: Vec<(SharedString, SharedString)>,
}

impl Edge {
    pub fn new(
        source: impl Into<SharedString>,
        target: impl Into<SharedString>,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            label: label.into(),
            properties: Vec::new(),
        }
    }

    pub fn property(mut self, key: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        self.properties.push((key.into(), value.into()));
        self
    }
}

/// A batch of nodes and edges, as returned by a query.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct GraphData {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl GraphData {
    pub fn new(nodes: Vec<Node>, edges: Vec<Edge>) -> Self {
        Self { nodes, edges }
    }
}
