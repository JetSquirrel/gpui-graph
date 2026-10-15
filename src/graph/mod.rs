//! A network graph (force-directed) component built on GPUI + gpui-base.

mod layout;
mod model;
mod view;

pub use model::{Edge, GraphData, Node};
pub use view::{GraphEvent, LabelMode, NetworkGraph};
