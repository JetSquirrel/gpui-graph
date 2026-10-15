use std::{cell::RefCell, rc::Rc};

use gpui::{AppContext as _, TestAppContext};
use gpui_graph::{Edge, GraphData, GraphEvent, NetworkGraph, Node, Selection};

fn sample() -> GraphData {
    GraphData::new(
        vec![
            Node::new("a", "A", "X"),
            Node::new("b", "B", "X"),
            Node::new("c", "C", "Y"),
        ],
        vec![
            Edge::new("a", "b", "R"),
            Edge::new("b", "a", "R"),
            Edge::new("b", "c", "S"),
            Edge::new("a", "missing", "R"),
        ],
    )
}

#[gpui::test]
fn loads_data_and_drops_dangling_edges(cx: &mut TestAppContext) {
    let graph = cx.new(|cx| NetworkGraph::new(cx).with_data(sample()));
    graph.read_with(cx, |g, _| {
        assert_eq!(g.node_count(), 3);
        assert_eq!(g.edge_count(), 3);
        assert_eq!(g.degree("b"), 3);
        let cats: Vec<_> = g.categories().into_iter().map(|c| (c.name.to_string(), c.count)).collect();
        assert_eq!(cats, [("X".into(), 2), ("Y".into(), 1)]);
    });
}

#[gpui::test]
fn merge_skips_existing_nodes_and_duplicate_edges(cx: &mut TestAppContext) {
    let graph = cx.new(|cx| NetworkGraph::new(cx).with_data(sample()));
    graph.update(cx, |g, cx| {
        let more = GraphData::new(
            vec![Node::new("c", "C again", "Y"), Node::new("d", "D", "Z")],
            vec![Edge::new("b", "c", "S"), Edge::new("c", "d", "T")],
        );
        g.merge(more, Some("c"), cx);
        assert_eq!(g.node_count(), 4);
        assert_eq!(g.edge_count(), 4);
        assert_eq!(g.node("c").unwrap().label.as_ref(), "C");
        assert_eq!(g.neighbors_of("c").len(), 2);
    });
}

#[gpui::test]
fn remove_nodes_remaps_and_clears_lost_selection(cx: &mut TestAppContext) {
    let graph = cx.new(|cx| NetworkGraph::new(cx).with_data(sample()));
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|cx| {
        cx.subscribe(&graph, move |_, e: &GraphEvent, _| sink.borrow_mut().push(e.clone())).detach();
    });

    graph.update(cx, |g, cx| g.select_node(Some("c"), false, cx));
    graph.update(cx, |g, cx| g.remove_nodes(&["a"], cx));
    graph.read_with(cx, |g, _| {
        assert_eq!(g.node_count(), 2);
        assert_eq!(g.edge_count(), 1);
        // Selection survives because "c" was kept, and still points at "c".
        assert!(matches!(g.selection(), Selection::Node(n) if n.id.as_ref() == "c"));
        assert!(g.node("a").is_none());
        assert_eq!(g.degree("b"), 1);
    });

    graph.update(cx, |g, cx| g.remove_nodes(&["c"], cx));
    graph.read_with(cx, |g, _| assert!(matches!(g.selection(), Selection::None)));
    let events = events.borrow();
    assert!(matches!(events.last(), Some(GraphEvent::SelectionChanged(Selection::None))));
}

#[gpui::test]
fn pinning_and_data_roundtrip(cx: &mut TestAppContext) {
    let graph = cx.new(|cx| NetworkGraph::new(cx).with_data(sample()));
    graph.update(cx, |g, cx| {
        g.set_pinned("a", true, cx);
        assert!(g.is_pinned("a"));
        g.unpin_all(cx);
        assert!(!g.is_pinned("a"));
        let data = g.data();
        assert_eq!((data.nodes.len(), data.edges.len()), (3, 3));
        g.clear(cx);
        assert_eq!(g.node_count(), 0);
    });
}
