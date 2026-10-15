mod app;
mod source;

use gpui::{App, AppContext as _, KeyBinding, WindowBounds, WindowOptions, actions, px, size};

actions!(gpui_graph, [Quit]);

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        gpui_base::init(cx);
        gpui_graph::init(cx);
        app::init(cx);
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1280.), px(820.)), cx)),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            cx.new(|cx| app::GraphQueryApp::new(window, cx))
        })
        .expect("failed to open window");
        cx.activate(true);
    });
}
