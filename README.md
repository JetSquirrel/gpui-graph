# gpui-graph

A force-directed network graph component for [GPUI](https://www.gpui.rs), built on
[gpui-base](https://gpui-kit.com/base/). Designed for graph query and exploration
applications.

- Force simulation with Barnes–Hut repulsion, collision detection, links,
  gravity, and community clustering.
- Pan, zoom, node dragging, hover highlighting, node and edge selection,
  parallel edges, self-loops, arrows, and labels.
- Incremental updates for expanding neighbors, node removal, and pinning.
- Data-driven communities with hulls, group colors, and collapsible super nodes.
- Keyboard navigation, visible focus indicators, and reduced-motion support.
- Styling based on gpui-base theme tokens, with configurable colors and palettes.

## Quick start

Use stable Rust with the components specified in
[`rust-toolchain.toml`](rust-toolchain.toml). macOS is the primary platform for
running the examples and requires the Xcode Command Line Tools. CI also tests
the library on Linux; see [CONTRIBUTING.md](CONTRIBUTING.md) for setup and checks.

```sh
cargo run --locked                 # Launch query-app (the default member)
cargo run -p basic --locked        # Launch the minimal example
cargo test -p gpui-graph --locked   # Run library tests
```

The query application includes a query input, legend, community list, details
panel, and context menus. It uses an in-memory data source, so no database or
credentials are required. Try `*` to show all sample data, `Matrix depth 2` to
explore a neighborhood, or `random 500` to generate a larger graph.

## Installation

```toml
[dependencies]
gpui-graph = "0.1"
gpui-base = "=0.8.1"
# Must match the GPUI snapshot used by gpui-base.
gpui = { package = "gpui-pre", version = "=0.3.10" }
```

When developing inside this repository, use the workspace dependency or
`gpui-graph = { path = "crates/gpui-graph" }` from the repository root.

## Usage

Initialize both libraries once when the application starts:

```rust
gpui_base::init(cx);
gpui_graph::init(cx); // Register graph keyboard bindings.
```

Create the graph inside the owning view's context:

```rust
use gpui::AppContext as _;
use gpui_graph::{Edge, GraphData, LabelMode, NetworkGraph, Node};

let graph = cx.new(|cx| {
    NetworkGraph::new(cx)
        .with_data(GraphData::new(
            vec![
                Node::new("a", "Alice", "Person").group("team"),
                Node::new("x", "Acme", "Company").group("team"),
            ],
            vec![Edge::new("a", "x", "WORKS_AT")],
        ))
        .with_label_mode(LabelMode::Auto)
});
```

Subscribe to events to connect the graph to application UI and data loading:

```rust
use gpui_graph::{NetworkGraphEvent, Selection};

let subscription = cx.subscribe(
    &graph,
    |this, graph, event: &NetworkGraphEvent, cx| match event {
        NetworkGraphEvent::SelectionChanged(Selection::Node(node)) => {
            // Update your details panel.
        }
        NetworkGraphEvent::NodeActivated(node) => {
            // Double-click or Enter: load neighbors through your data source,
            // then merge them around the activated node:
            // graph.update(cx, |g, cx| g.merge(neighbors, Some(&node.id), cx));
        }
        NetworkGraphEvent::ContextMenu { target, position } => {
            // Show a menu at the supplied window coordinates.
        }
        _ => {}
    },
);
```

Store the subscription in the owning view to keep it active. The graph fills
its parent, so give the parent a size, for example
`div().flex_1().child(graph.clone())` inside a sized flex container.

These snippets are excerpts from a host view. See the
[complete basic example](examples/basic/src/main.rs) for imports, subscription
storage, rendering, and window setup, or the [API documentation](https://docs.rs/gpui-graph)
for type and method details.

## API overview

| Area | API |
| --- | --- |
| Builders | `with_data`, `with_force_config`, `with_label_mode`, `with_color_by`, `with_palette`, `with_node_sizing`, `with_colors`, `with_tooltip`; boolean builders: `arrows`, `edge_labels`, `group_hulls`, `sticky_drag`, `tooltips` |
| Data | `set_data` (replace data and reset the viewport), `merge(data, anchor)` (incremental update), `remove_nodes`, `clear`, `data`, `node_count`, `edge_count` |
| Viewport | `fit_view`, `zoom_by`, `relayout`, `select_node(id, center)` |
| Pinning | `set_pinned`, `is_pinned`, `unpin_all` |
| Inspection | `node`, `degree`, `neighbors_of`, `edges_of`, `categories`, `color_of(node, cx)`, `resolved_colors`, `selection`, `zoom`, `is_simulating`, `label_mode`, `color_by`, `has_arrows`, `has_edge_labels`, `has_group_hulls`, `has_tooltips`, `is_sticky_drag` |
| Runtime settings | `set_label_mode`, `set_color_by`, `set_arrows`, `set_edge_labels`, `set_group_hulls`, `set_sticky_drag`, `set_tooltips`, `set_force_config`, `set_colors` |
| Communities | `groups`, `group`, `group_members`, `collapse_group`, `expand_group`, `toggle_group`, `collapse_all_groups`, `expand_all_groups`, `set_group_label` |
| Events (`NetworkGraphEvent`) | `SelectionChanged(Selection)`, `NodeActivated(Node)`, `GroupToggled { group, collapsed }`, `ContextMenu { target, position }` |
| Actions (`gpui_graph::actions`) | `ZoomIn`, `ZoomOut`, `FitView`, `ClearSelection`, `SelectLeft`, `SelectRight`, `SelectUp`, `SelectDown`, `Activate`, `OpenContextMenu` |

Value builders use `with_<field>`, boolean builders use the field name, boolean
readers use `is_` / `has_`, and mutations use `set_<field>`. Public structs and
enums are `#[non_exhaustive]`; construct `ForceConfig` and `GraphColors` through
their provided constructors and `with_*` builders.

### Communities

The component reads community assignments from `Node::group`; it does not run
community detection. Supply assignments from your data source, such as a
community property computed by Neo4j GDS Louvain.

Groups can have distinct colors, rounded convex hulls, and labels. The layout
can pull group members together through `ForceConfig::group_strength`; set it
to zero to disable this force.

Collapsing a group creates a super node with a member count and a double ring.
Edges connected to it are bundled by direction, drawn thicker, and labeled
`×N`. Double-click the super node to expand it; members recover their relative
positions from before the collapse.

Collapsing changes only the visual representation. `data()` and `node_count()`
still describe the full graph, and selection is preserved by ID across collapse
and expansion. `Selection::Group(GroupInfo)` represents a selected super node;
`Selection::EdgeBundle(Vec<Edge>)` represents a selected bundle.

### Styling and layout

Colors, typography, corners, spacing, and shadows come from gpui-base theme
tokens and follow light/dark mode and font scaling. Override graph colors with
`GraphColors`. The default categorical palette is AntV; replace it with
`with_palette`. Node text automatically uses the higher-contrast color.

Node sizing defaults to `NodeSizing::Degree { min, max }`. Use
`NodeSizing::Fixed(radius)` for uniform nodes. Customize tooltips with
`with_tooltip(|selection, colors, window, cx| Some(element))`; return `None` to
hide a tooltip.

`ForceConfig` exposes d3-force-style settings such as `link_distance`, `charge`,
`theta`, `collide_padding`, `gravity`, and `velocity_decay`.

## Interaction

Scroll to zoom around the pointer, drag the background to pan, and drag nodes
to reposition them. Click a node or edge to select it. Double-click a node to
emit `NodeActivated`, or double-click a collapsed community to expand it.
Right-click emits `ContextMenu`. Hovering a node highlights its immediate
neighbors. The viewport automatically fits the running layout until the user
interacts with the viewport.

The graph is a Tab stop. These shortcuts apply while it has keyboard focus:

| Key | Action |
| --- | --- |
| Arrow keys | Select the nearest node in that direction, starting from the viewport center if nothing is selected; pan to reveal it when needed |
| Enter | Activate a node (`NodeActivated`) or toggle the selected community |
| Shift+F10 | Emit `ContextMenu` for the selection at its screen position |
| Esc | Clear selection |
| `=` / `-` / `0` | Zoom in / zoom out / fit the graph |

Keyboard interaction shows focus rings around the graph and selected item.
Pointer interaction hides them. The grab cursor appears during dragging;
hover feedback uses a halo.

With the system's reduced-motion preference enabled, the simulation settles
without animated layout steps, using up to 120 ms per frame. Viewport fitting
jumps directly to its target.

## Project structure

```text
crates/gpui-graph/       Reusable graph component
examples/basic/         Minimal embedding example
examples/query-app/     Interactive graph query application
.github/workflows/      CI and tagged release workflows
```

The library implementation lives in `crates/gpui-graph/src/`:

| File | Responsibility |
| --- | --- |
| `layout.rs` | Force simulation: links, Barnes–Hut many-body repulsion, spatial-hash collision detection, gravity, group clustering, and alpha cooling |
| `view/mod.rs` | `NetworkGraph`: full model, persistent positions, community state, public API, input, and viewport |
| `view/visual.rs` | Visible graph derived from the model and collapsed state, including super nodes and bundled edges; the layout runs on this graph |
| `view/paint.rs` | Canvas rendering: community hulls, edge paths batched by state, nodes, labels, and tooltips |
| `geometry.rs` | Straight and quadratic edge geometry, hit testing, convex hulls, and rounded outlines |
| `style.rs` | `GraphColors`, `Palette`, `NodeSizing`, `LabelMode`, and `ColorBy` |
| `model.rs` | `Node`, `Edge`, and `GraphData` |
| `actions.rs` | Graph actions and keyboard bindings |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development conventions, validation
commands, and the release process.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
