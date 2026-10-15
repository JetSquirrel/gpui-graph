use gpui::SharedString;

/// A vertex returned by a graph query.
#[derive(Clone, Debug)]
pub struct Node {
    pub id: SharedString,
    pub label: SharedString,
    /// Vertex label / type in the database (e.g. `Person`, `Movie`). Drives the color.
    pub category: SharedString,
    pub properties: Vec<(SharedString, SharedString)>,
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
        }
    }

    pub fn property(mut self, key: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        self.properties.push((key.into(), value.into()));
        self
    }
}

/// A directed relationship between two vertices.
#[derive(Clone, Debug)]
pub struct Edge {
    pub source: SharedString,
    pub target: SharedString,
    /// Relationship type (e.g. `ACTED_IN`).
    pub label: SharedString,
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
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct GraphData {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}
