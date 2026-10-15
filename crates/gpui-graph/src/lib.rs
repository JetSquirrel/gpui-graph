//! Force-directed network graph component for [GPUI](https://gpui.rs), built on
//! [gpui-base](https://gpui-kit.com/zh-CN/base/).
//!
//! [`NetworkGraph`] is a view: create it with `cx.new`, feed it [`GraphData`],
//! and listen for [`GraphEvent`]s. It handles layout (d3-force style), pan,
//! zoom, node dragging, hover focus and selection; the host app owns data
//! loading, panels and menus.
//!
//! Colors follow the gpui-base [`Theme`](gpui_base::Theme) unless overridden
//! with [`NetworkGraph::with_colors`].

mod geometry;
mod layout;
mod model;
mod style;
mod view;

pub use layout::ForceConfig;
pub use model::{Edge, GraphData, Node};
pub use style::{ColorBy, GraphColors, LabelMode, NodeSizing, Palette};
pub use view::{CategoryInfo, GraphEvent, GroupInfo, NetworkGraph, Selection, TooltipBuilder};
