use gpui::{App, KeyBinding, actions};

/// Key context set on a focused [`NetworkGraph`](crate::NetworkGraph).
pub const KEY_CONTEXT: &str = "NetworkGraph";

actions!(
    network_graph,
    [
        /// Zoom in around the viewport center.
        ZoomIn,
        /// Zoom out around the viewport center.
        ZoomOut,
        /// Fit the whole graph into view.
        FitView,
        /// Clear the selection.
        ClearSelection,
        /// Move the selection to the nearest item on the left.
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        /// Same as double-click: activate a node, or expand / collapse a group.
        Activate,
        /// Emit a context-menu event for the selection, as a right-click would.
        OpenContextMenu,
    ]
);

/// Register the default key bindings. Call once at startup, after `gpui_base::init`.
pub fn init(cx: &mut App) {
    let ctx = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("=", ZoomIn, ctx),
        KeyBinding::new("+", ZoomIn, ctx),
        KeyBinding::new("-", ZoomOut, ctx),
        KeyBinding::new("0", FitView, ctx),
        KeyBinding::new("escape", ClearSelection, ctx),
        KeyBinding::new("left", SelectLeft, ctx),
        KeyBinding::new("right", SelectRight, ctx),
        KeyBinding::new("up", SelectUp, ctx),
        KeyBinding::new("down", SelectDown, ctx),
        KeyBinding::new("enter", Activate, ctx),
        KeyBinding::new("shift-f10", OpenContextMenu, ctx),
    ]);
}
