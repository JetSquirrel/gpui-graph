# gpui-graph

Force-directed network graph component for [GPUI](https://www.gpui.rs), built on
[gpui-base](https://gpui-kit.com/base/). Made for graph query and exploration tools.

- d3-force style layout (Barnes–Hut many-body, collision, link, gravity, community clustering)
- Pan, zoom, drag, hover focus, node / edge selection, parallel edges, self-loops, arrows, labels
- Incremental `merge` for "expand neighbors", `remove_nodes`, pinning
- Communities from your data (`Node::group`): hulls, color by group, collapse into super nodes
- Keyboard operable (arrow-key selection, Enter, Shift-F10, Esc, zoom / fit), focus-visible rings
- Follows the gpui-base theme tokens and honors reduced motion

## Usage

```toml
[dependencies]
gpui-graph = "0.1"
gpui-base = "=0.8.0"
gpui = { package = "gpui-pre", version = "=0.3.9" } # must match gpui-base
```

```rust
use gpui_graph::{Edge, GraphData, NetworkGraph, NetworkGraphEvent, Node, Selection};

// once at startup
gpui_base::init(cx);
gpui_graph::init(cx); // key bindings

let graph = cx.new(|cx| {
    NetworkGraph::new(cx).with_data(GraphData::new(
        vec![
            Node::new("a", "Alice", "Person").group("c1"),
            Node::new("x", "Acme", "Company").group("c1"),
        ],
        vec![Edge::new("a", "x", "WORKS_AT")],
    ))
});

let subscription = cx.subscribe(&graph, |this, graph, event: &NetworkGraphEvent, cx| match event {
    NetworkGraphEvent::SelectionChanged(Selection::Node(node)) => { /* details panel */ }
    NetworkGraphEvent::NodeActivated(node) => {
        // double-click / Enter: fetch neighbors, then
        // graph.update(cx, |g, cx| g.merge(neighbors, Some(&node.id), cx));
    }
    NetworkGraphEvent::ContextMenu { target, position } => { /* show your menu */ }
    _ => {}
});

// the graph fills its parent, so give the parent a size
div().flex_1().child(graph.clone())
```

See the [repository](https://github.com/JetSquirrel/gpui-graph) for the full API
overview and the `query-app` example.

## License

Apache-2.0
