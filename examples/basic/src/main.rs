//! The smallest useful embedding of `NetworkGraph`.

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Window, WindowBounds, WindowOptions, div, px, size,
};
use gpui_graph::{Edge, GraphData, NetworkGraph, NetworkGraphEvent, Node, Selection};

struct Example {
    graph: Entity<NetworkGraph>,
    selected: String,
    _subscription: Subscription,
}

impl Example {
    fn new(cx: &mut Context<Self>) -> Self {
        let nodes = vec![
            Node::new("alice", "Alice", "Person"),
            Node::new("bob", "Bob", "Person"),
            Node::new("carol", "Carol", "Person"),
            Node::new("acme", "Acme Inc.", "Company").property("founded", "1999"),
            Node::new("globex", "Globex", "Company"),
        ];
        let edges = vec![
            Edge::new("alice", "acme", "WORKS_AT"),
            Edge::new("bob", "acme", "WORKS_AT"),
            Edge::new("carol", "globex", "WORKS_AT"),
            Edge::new("alice", "bob", "KNOWS"),
            Edge::new("bob", "carol", "KNOWS"),
            Edge::new("acme", "globex", "COMPETES_WITH"),
        ];
        let graph = cx.new(|cx| NetworkGraph::new(cx).with_data(GraphData::new(nodes, edges)));
        let subscription = cx.subscribe(&graph, |this, _, event: &NetworkGraphEvent, cx| {
            if let NetworkGraphEvent::SelectionChanged(selection) = event {
                this.selected = match selection {
                    Selection::None => "nothing".into(),
                    Selection::Node(n) => format!("node {}", n.label),
                    Selection::Edge(e) => format!("edge {} → {}", e.source, e.target),
                    _ => "other".into(),
                };
                cx.notify();
            }
        });
        Self { graph, selected: "nothing".into(), _subscription: subscription }
    }
}

impl Render for Example {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().p_2().text_sm().child(format!("Selected: {}", self.selected)))
            .child(div().flex_1().child(self.graph.clone()))
    }
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        gpui_base::init(cx);
        gpui_graph::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(800.), px(600.)), cx)),
            ..Default::default()
        };
        cx.open_window(options, |_, cx| cx.new(Example::new)).unwrap();
        cx.activate(true);
    });
}
