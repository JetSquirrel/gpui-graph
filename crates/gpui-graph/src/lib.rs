//! Force-directed network graph component for [GPUI](https://gpui.rs), built on
//! [gpui-base](https://gpui-kit.com/zh-CN/base/).
//!
//! [`NetworkGraph`] is a stateful view (an `Entity`): it owns the layout,
//! viewport, hover and selection state across frames. Create it with `cx.new`,
//! feed it [`GraphData`] and subscribe to [`NetworkGraphEvent`]s. The host app
//! owns data loading, panels and menus; it answers events such as
//! [`NetworkGraphEvent::ContextMenu`] with its own UI.
//!
//! Call [`init`] once at startup to register the keyboard bindings (arrow keys
//! move the selection, Enter activates, Escape clears, `=`/`-`/`0` zoom and
//! fit, Shift-F10 opens the context menu). The graph is a tab stop.
//!
//! Colors follow the gpui-base [`Theme`](gpui_base::Theme) unless overridden
//! with [`NetworkGraph::with_colors`].

pub mod actions;
mod geometry;
mod layout;
mod model;
mod style;
mod view;

pub use layout::ForceConfig;
pub use model::{Edge, GraphData, Node};
pub use style::{ColorBy, GraphColors, LabelMode, NodeSizing, Palette};
pub use actions::init;
pub use view::{CategoryInfo, GroupInfo, NetworkGraph, NetworkGraphEvent, Selection, TooltipBuilder};
