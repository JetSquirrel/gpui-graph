use std::{cell::RefCell, rc::Rc};

use gpui::{AppContext as _, TestAppContext};
use gpui_graph::{Edge, GraphData, NetworkGraphEvent, NetworkGraph, Node, Selection};

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
        cx.subscribe(&graph, move |_, e: &NetworkGraphEvent, _| sink.borrow_mut().push(e.clone())).detach();
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
    assert!(matches!(events.last(), Some(NetworkGraphEvent::SelectionChanged(Selection::None))));
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

fn communities() -> GraphData {
    GraphData::new(
        vec![
            Node::new("a1", "A1", "P").group("A"),
            Node::new("a2", "A2", "P").group("A"),
            Node::new("a3", "A3", "P").group("A"),
            Node::new("b1", "B1", "P").group("B"),
            Node::new("b2", "B2", "P").group("B"),
            Node::new("x", "X", "Q"),
        ],
        vec![
            Edge::new("a1", "a2", "R"),
            Edge::new("a2", "a3", "R"),
            Edge::new("a1", "b1", "R"),
            Edge::new("a2", "b1", "R"),
            Edge::new("b1", "b2", "R"),
            Edge::new("x", "a3", "R"),
        ],
    )
}

#[gpui::test]
fn groups_come_from_node_data(cx: &mut TestAppContext) {
    let graph = cx.new(|cx| NetworkGraph::new(cx).with_data(communities()));
    graph.read_with(cx, |g, _| {
        let groups: Vec<_> = g.groups().into_iter().map(|g| (g.id.to_string(), g.size, g.collapsed)).collect();
        assert_eq!(groups, [("A".into(), 3, false), ("B".into(), 2, false)]);
        assert_eq!(g.group_members("B").len(), 2);
    });
}

#[gpui::test]
fn collapse_expand_keeps_model_and_emits_events(cx: &mut TestAppContext) {
    let graph = cx.new(|cx| NetworkGraph::new(cx).with_data(communities()));
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|cx| {
        cx.subscribe(&graph, move |_, e: &NetworkGraphEvent, _| sink.borrow_mut().push(e.clone())).detach();
    });

    graph.update(cx, |g, cx| {
        g.select_node(Some("a1"), false, cx);
        g.collapse_group("A", cx);
        assert!(g.group("A").unwrap().collapsed);
        // The model is untouched: collapsing only changes what is drawn.
        assert_eq!((g.node_count(), g.edge_count()), (6, 6));
        // Selection is kept by identity even while its node is hidden.
        assert!(matches!(g.selection(), Selection::Node(n) if n.id.as_ref() == "a1"));

        g.collapse_all_groups(cx);
        g.expand_all_groups(cx);
        assert!(g.groups().iter().all(|g| !g.collapsed));
        assert!(matches!(g.selection(), Selection::Node(n) if n.id.as_ref() == "a1"));
    });

    let toggles: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|e| match e {
            NetworkGraphEvent::GroupToggled { group, collapsed } => Some((group.to_string(), *collapsed)),
            _ => None,
        })
        .collect();
    assert_eq!(toggles, [("A".into(), true), ("B".into(), true), ("A".into(), false), ("B".into(), false)]);
}

#[gpui::test]
fn merge_into_collapsed_group_and_remove_last_member(cx: &mut TestAppContext) {
    let graph = cx.new(|cx| NetworkGraph::new(cx).with_data(communities()));
    graph.update(cx, |g, cx| {
        g.collapse_group("B", cx);
        g.merge(
            GraphData::new(vec![Node::new("b3", "B3", "P").group("B")], vec![Edge::new("b3", "x", "R")]),
            Some("b1"),
            cx,
        );
        let b = g.group("B").unwrap();
        assert_eq!((b.size, b.collapsed), (3, true));

        g.remove_nodes(&["b1", "b2", "b3"], cx);
        assert!(g.group("B").is_none());
        assert_eq!(g.groups().len(), 1);
        assert_eq!(g.node_count(), 4);
    });
}

#[gpui::test]
fn keyboard_navigation_selects_activates_and_clears(cx: &mut TestAppContext) {
    use gpui::Focusable as _;
    use gpui_graph::actions::{Activate, ClearSelection, SelectRight};

    cx.update(|cx| {
        gpui_base::init(cx);
        gpui_graph::init(cx);
    });
    let (graph, cx) = cx.add_window_view(|_, cx| NetworkGraph::new(cx).with_data(communities()));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let handle = graph.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    });

    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&graph, move |_, e: &NetworkGraphEvent, _| sink.borrow_mut().push(e.clone())).detach();
    });

    // First arrow press picks the node nearest the viewport center.
    cx.dispatch_action(SelectRight);
    let first = graph.read_with(cx, |g, _| match g.selection() {
        Selection::Node(n) => n.id.clone(),
        other => panic!("expected a node, got {other:?}"),
    });

    cx.dispatch_action(Activate);
    assert!(events.borrow().iter().any(|e| matches!(e, NetworkGraphEvent::NodeActivated(n) if n.id == first)));

    cx.dispatch_action(ClearSelection);
    graph.read_with(cx, |g, _| assert!(matches!(g.selection(), Selection::None)));
}
