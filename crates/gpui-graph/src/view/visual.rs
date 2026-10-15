//! The graph as drawn: model nodes, minus members of collapsed groups, plus
//! one super node per collapsed group. Edges touching a super node are merged.
//!
//! Layout runs on this graph. Positions are written back to the model before
//! every structural change ([`NetworkGraph::sync_back`](super::NetworkGraph)),
//! so rebuilding is cheap and never loses the layout.

use std::collections::HashMap;

use crate::layout::Body;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Vertex {
    Node(usize),
    Group(usize),
}

pub(super) struct VisualEdge {
    pub source: usize,
    pub target: usize,
    /// Model edge indices drawn by this edge; more than one for merged edges.
    pub edges: Vec<usize>,
    /// Signed bend for parallel edges between the same pair (0 = straight).
    pub curvature: f32,
}

#[derive(Default)]
pub(super) struct Visual {
    pub vertices: Vec<Vertex>,
    pub bodies: Vec<Body>,
    pub edges: Vec<VisualEdge>,
    pub links: Vec<(usize, usize)>,
    pub degrees: Vec<usize>,
    pub neighbors: Vec<Vec<usize>>,
    /// Model node → the vertex that shows it (itself, or its collapsed group).
    pub node_slot: Vec<usize>,
    /// Group → its super node, when collapsed.
    pub group_slot: Vec<Option<usize>>,
    /// Visible members of each expanded group, for the cluster force and hulls.
    pub clusters: Vec<Vec<usize>>,
    /// Group index of each entry in `clusters`.
    pub cluster_groups: Vec<usize>,
}

pub(super) struct GroupInput<'a> {
    pub collapsed: bool,
    pub body: &'a Body,
}

impl Visual {
    pub fn build(
        node_bodies: &[Body],
        node_group: &[Option<usize>],
        groups: &[GroupInput],
        edges: &[(usize, usize)],
    ) -> Self {
        let mut v = Visual {
            node_slot: vec![usize::MAX; node_bodies.len()],
            group_slot: vec![None; groups.len()],
            ..Default::default()
        };
        let mut members: Vec<Vec<usize>> = vec![Vec::new(); groups.len()];

        for (i, body) in node_bodies.iter().enumerate() {
            if node_group[i].is_some_and(|g| groups[g].collapsed) {
                continue;
            }
            v.node_slot[i] = v.vertices.len();
            if let Some(g) = node_group[i] {
                members[g].push(v.vertices.len());
            }
            v.vertices.push(Vertex::Node(i));
            v.bodies.push(body.clone());
        }
        for (g, group) in groups.iter().enumerate() {
            if group.collapsed {
                v.group_slot[g] = Some(v.vertices.len());
                v.vertices.push(Vertex::Group(g));
                v.bodies.push(group.body.clone());
            }
        }
        for (i, slot) in v.node_slot.iter_mut().enumerate() {
            if *slot == usize::MAX {
                *slot = v.group_slot[node_group[i].expect("hidden node has a group")].unwrap();
            }
        }

        // Edges between plain nodes stay one-to-one; edges touching a super node
        // merge per direction. Edges inside a collapsed group disappear.
        let mut merged: HashMap<(usize, usize), usize> = HashMap::new();
        for (ei, &(s, t)) in edges.iter().enumerate() {
            let (vs, vt) = (v.node_slot[s], v.node_slot[t]);
            let touches_group = matches!(v.vertices[vs], Vertex::Group(_)) || matches!(v.vertices[vt], Vertex::Group(_));
            if !touches_group {
                v.edges.push(VisualEdge { source: vs, target: vt, edges: vec![ei], curvature: 0. });
                continue;
            }
            if vs == vt {
                continue;
            }
            match merged.get(&(vs, vt)) {
                Some(&k) => v.edges[k].edges.push(ei),
                None => {
                    merged.insert((vs, vt), v.edges.len());
                    v.edges.push(VisualEdge { source: vs, target: vt, edges: vec![ei], curvature: 0. });
                }
            }
        }

        let n = v.vertices.len();
        v.links = v.edges.iter().map(|e| (e.source, e.target)).collect();
        v.degrees = vec![0; n];
        v.neighbors = vec![Vec::new(); n];
        for &(s, t) in &v.links {
            v.degrees[s] += 1;
            v.degrees[t] += 1;
            if s != t {
                v.neighbors[s].push(t);
                v.neighbors[t].push(s);
            }
        }
        for list in &mut v.neighbors {
            list.sort_unstable();
            list.dedup();
        }

        // Fan out parallel edges so multi-relationships stay distinguishable.
        let mut pairs: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (i, e) in v.edges.iter().enumerate() {
            pairs.entry((e.source.min(e.target), e.source.max(e.target))).or_default().push(i);
        }
        for list in pairs.values() {
            let count = list.len() as f32;
            for (k, &i) in list.iter().enumerate() {
                let e = &mut v.edges[i];
                let mut c = if count > 1. { k as f32 - (count - 1.) / 2. } else { 0. };
                // Normalise direction so A->B and B->A bend to opposite sides.
                if e.source > e.target {
                    c = -c;
                }
                e.curvature = c;
            }
        }

        for (g, m) in members.into_iter().enumerate() {
            if !groups[g].collapsed && !m.is_empty() {
                v.clusters.push(m);
                v.cluster_groups.push(g);
            }
        }
        v
    }
}
