use gpui::{
    App, AppContext as _, Context, Entity, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Pixels, Point, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, WindowAppearance, anchored,
    deferred, div, px,
};
use gpui_base::input::{InputEditorStyle, InputEvent, InputState};
use gpui_base::{Button, ColorTokens, Input, InputBase, SemanticThemeTokens, Theme, h_flex, v_flex};
use gpui_graph::{GraphEvent, LabelMode, NetworkGraph, Node, Selection};

use crate::source::{GraphSource, MemorySource, sample_movies};

struct ContextMenu {
    node: Node,
    position: Point<Pixels>,
}

pub struct GraphQueryApp {
    source: Box<dyn GraphSource>,
    query: Entity<InputState>,
    graph: Entity<NetworkGraph>,
    selection: Selection,
    menu: Option<ContextMenu>,
    status: SharedString,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl GraphQueryApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        apply_theme(window, cx);
        let query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search… e.g.  keanu depth 2   ·   *   ·   random 2000")
                .default_value("keanu depth 2")
        });
        let graph = cx.new(NetworkGraph::new);

        let subscriptions = vec![
            cx.subscribe_in(&query, window, |this, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.run_query(window, cx);
                }
            }),
            cx.subscribe(&graph, |this, _, ev: &GraphEvent, cx| match ev {
                GraphEvent::SelectionChanged(selection) => {
                    this.selection = selection.clone();
                    cx.notify();
                }
                GraphEvent::NodeActivated(node) => this.expand(&node.id, cx),
                GraphEvent::ContextMenu { target: Selection::Node(node), position } => {
                    this.menu = Some(ContextMenu { node: node.clone(), position: *position });
                    cx.notify();
                }
                GraphEvent::ContextMenu { .. } => {
                    this.menu = None;
                    cx.notify();
                }
            }),
            cx.observe_window_appearance(window, |this, window, cx| {
                apply_theme(window, cx);
                this.style_input(cx);
                cx.notify();
            }),
        ];

        let mut this = Self {
            source: Box::new(MemorySource::new(sample_movies())),
            query,
            graph,
            selection: Selection::None,
            menu: None,
            status: "".into(),
            error: None,
            _subscriptions: subscriptions,
        };
        this.style_input(cx);
        this.run_query(window, cx);
        this
    }

    fn style_input(&self, cx: &mut Context<Self>) {
        let t = Theme::global(cx).tokens.colors;
        self.query.update(cx, |state, _| {
            state.set_editor_style(InputEditorStyle {
                foreground: t.foreground,
                muted_foreground: t.muted_foreground,
                selection: t.selection,
                caret: t.foreground,
                ..InputEditorStyle::default()
            });
        });
    }

    fn run_query(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.query.read(cx).value();
        match self.source.query(&text) {
            Ok(data) => {
                self.error = None;
                self.graph.update(cx, |g, cx| g.set_data(data, cx));
            }
            Err(err) => self.error = Some(err.into()),
        }
        self.refresh_status(cx);
    }

    fn expand(&mut self, id: &str, cx: &mut Context<Self>) {
        let data = self.source.expand(id);
        self.graph.update(cx, |g, cx| g.merge(data, Some(id), cx));
        self.refresh_status(cx);
    }

    fn refresh_status(&mut self, cx: &mut Context<Self>) {
        let g = self.graph.read(cx);
        self.status = format!("{} nodes · {} edges", g.node_count(), g.edge_count()).into();
        cx.notify();
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::global(cx).tokens.colors;
        let query = self.query.clone();
        let graph = self.graph.read(cx);
        let label_mode = graph.label_mode();
        let arrows = graph.show_arrows();

        h_flex()
            .h(px(48.))
            .px_3()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(t.border)
            .bg(t.surface)
            .child(div().font_weight(FontWeight::SEMIBOLD).mr_2().child("Graph Query"))
            .child(
                InputBase::new("query")
                    .flex_1()
                    .h(px(30.))
                    .px_2()
                    .rounded_md()
                    .border_1()
                    .border_color(t.input)
                    .bg(t.background)
                    .text_sm()
                    .styles(move |s| s.focused(move |style| style.border_color(t.ring)))
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        query.update(cx, |state, cx| state.focus(window, cx));
                    })
                    .child(Input::new(&self.query)),
            )
            .child(button("run", "Run", true, cx).on_click(cx.listener(|this, _, window, cx| {
                this.run_query(window, cx);
            })))
            .child(div().w(px(1.)).h(px(20.)).bg(t.border))
            .child(button("fit", "Fit", false, cx).on_click(cx.listener(|this, _, _, cx| {
                this.graph.update(cx, |g, cx| g.fit_view(cx));
            })))
            .child(button("zoom-in", "+", false, cx).on_click(cx.listener(|this, _, _, cx| {
                this.graph.update(cx, |g, cx| g.zoom_by(1.25, cx));
            })))
            .child(button("zoom-out", "−", false, cx).on_click(cx.listener(|this, _, _, cx| {
                this.graph.update(cx, |g, cx| g.zoom_by(0.8, cx));
            })))
            .child(button("relayout", "Re-layout", false, cx).on_click(cx.listener(|this, _, _, cx| {
                this.graph.update(cx, |g, cx| g.relayout(cx));
            })))
            .child(
                button(
                    "labels",
                    match label_mode {
                        LabelMode::Auto => "Labels: auto",
                        LabelMode::Always => "Labels: all",
                        LabelMode::Focus => "Labels: focus",
                        LabelMode::Never => "Labels: off",
                    },
                    false,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.graph.update(cx, |g, cx| {
                        let next = match g.label_mode() {
                            LabelMode::Auto => LabelMode::Always,
                            LabelMode::Always => LabelMode::Focus,
                            LabelMode::Focus => LabelMode::Never,
                            LabelMode::Never => LabelMode::Auto,
                        };
                        g.set_label_mode(next, cx);
                    });
                    cx.notify();
                })),
            )
            .child(
                button("arrows", if arrows { "Arrows: on" } else { "Arrows: off" }, false, cx).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.graph.update(cx, |g, cx| g.set_show_arrows(!g.show_arrows(), cx));
                        cx.notify();
                    }),
                ),
            )
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::global(cx).tokens.colors;
        let graph = self.graph.read(cx);
        let legend = graph.categories();

        let details = match &self.selection {
            Selection::None => v_flex()
                .gap_1()
                .text_sm()
                .text_color(t.muted_foreground)
                .child("Click a node or edge to inspect it.")
                .child("Double-click a node to expand its neighbors.")
                .child("Right-click a node for more actions.")
                .child("Drag nodes · drag background to pan · scroll to zoom."),
            Selection::Edge(edge) => {
                let source = graph.node(&edge.source).map(|n| n.label.clone()).unwrap_or_default();
                let target = graph.node(&edge.target).map(|n| n.label.clone()).unwrap_or_default();
                v_flex()
                    .gap_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_xs().text_color(t.muted_foreground).child("Relationship"))
                            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(edge.label.clone())),
                    )
                    .child(
                        properties_table(t.border)
                            .child(kv_row("from", source, t.border, t.muted_foreground))
                            .child(kv_row("to", target, t.border, t.muted_foreground))
                            .children(edge.properties.iter().map(|(k, v)| {
                                kv_row(k.clone(), v.clone(), t.border, t.muted_foreground)
                            })),
                    )
            }
            Selection::Node(node) => {
                let color = graph.color_of(node);
                let pinned = graph.is_pinned(&node.id);
                let neighbors = graph.neighbors_of(&node.id);
                v_flex()
                    .gap_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .child(div().size_2p5().rounded_full().bg(color))
                                    .child(div().text_xs().text_color(t.muted_foreground).child(node.category.clone()))
                                    .children(pinned.then(|| {
                                        div().text_xs().text_color(t.muted_foreground).child("· pinned")
                                    })),
                            )
                            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(node.label.clone())),
                    )
                    .child(
                        properties_table(t.border)
                            .child(kv_row("id", node.id.clone(), t.border, t.muted_foreground))
                            .children(node.properties.iter().map(|(k, v)| {
                                kv_row(k.clone(), v.clone(), t.border, t.muted_foreground)
                            })),
                    )
                    .child(section_title("NEIGHBORS", t.muted_foreground))
                    .child(v_flex().gap_0p5().children(neighbors.into_iter().map(|n| {
                        let id = n.id.clone();
                        h_flex()
                            .id(SharedString::from(format!("nb-{}", n.id)))
                            .gap_2()
                            .items_center()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_sm()
                            .cursor_pointer()
                            .hover(|s| s.bg(t.accent))
                            .child(div().size_2().rounded_full().bg(graph.color_of(&n)))
                            .child(n.label.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.graph.update(cx, |g, cx| g.select_node(Some(&id), true, cx));
                            }))
                    })))
            }
        };

        v_flex()
            .id("sidebar")
            .w(px(300.))
            .h_full()
            .flex_shrink_0()
            .p_4()
            .gap_4()
            .border_l_1()
            .border_color(t.border)
            .bg(t.surface)
            .overflow_y_scroll()
            .child(section_title("LEGEND", t.muted_foreground))
            .child(v_flex().gap_1().children(legend.into_iter().map(|c| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_sm()
                    .child(div().size_2p5().rounded_full().bg(c.color))
                    .child(div().flex_1().child(c.name))
                    .child(div().text_color(t.muted_foreground).child(c.count.to_string()))
            })))
            .child(div().h(px(1.)).bg(t.border))
            .child(section_title("DETAILS", t.muted_foreground))
            .child(details)
    }

    fn context_menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let menu = self.menu.as_ref()?;
        let t = Theme::global(cx).tokens.colors;
        let id = menu.node.id.clone();
        let pinned = self.graph.read(cx).is_pinned(&id);

        let item = |key: &'static str, label: &'static str| {
            div()
                .id(key)
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(|s| s.bg(t.accent))
                .child(label)
        };
        let (expand_id, pin_id, hide_id) = (id.clone(), id.clone(), id.clone());
        Some(deferred(
            anchored().position(menu.position).snap_to_window().child(
                v_flex()
                    .id("graph-context-menu")
                    .min_w(px(160.))
                    .p_1()
                    .rounded_md()
                    .border_1()
                    .border_color(t.border)
                    .bg(t.surface)
                    .shadow_lg()
                    .text_sm()
                    .text_color(t.foreground)
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.menu = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .text_xs()
                            .text_color(t.muted_foreground)
                            .child(menu.node.label.clone()),
                    )
                    .child(item("expand", "Expand neighbors").on_click(cx.listener(move |this, _, _, cx| {
                        this.menu = None;
                        this.expand(&expand_id, cx);
                    })))
                    .child(item("pin", if pinned { "Unpin" } else { "Pin position" }).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.menu = None;
                            this.graph.update(cx, |g, cx| g.set_pinned(&pin_id, !pinned, cx));
                            cx.notify();
                        },
                    )))
                    .child(item("hide", "Hide node").on_click(cx.listener(move |this, _, _, cx| {
                        this.menu = None;
                        this.graph.update(cx, |g, cx| g.remove_nodes(&[&hide_id], cx));
                        this.refresh_status(cx);
                    }))),
            ),
        ))
    }
}

impl Render for GraphQueryApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::global(cx).tokens.colors;
        v_flex()
            .size_full()
            .bg(t.background)
            .text_color(t.foreground)
            .child(self.toolbar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(div().flex_1().h_full().min_w_0().child(self.graph.clone()))
                    .child(self.sidebar(cx)),
            )
            .child(
                h_flex()
                    .h(px(26.))
                    .px_3()
                    .gap_3()
                    .items_center()
                    .border_t_1()
                    .border_color(t.border)
                    .bg(t.surface)
                    .text_xs()
                    .text_color(t.muted_foreground)
                    .child(self.status.clone())
                    .children(self.error.clone().map(|e| div().text_color(t.destructive).child(e))),
            )
            .children(self.context_menu(cx))
    }
}

fn apply_theme(window: &Window, cx: &mut App) {
    let dark = matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark);
    Theme::global_mut(cx).tokens = SemanticThemeTokens {
        colors: if dark { ColorTokens::dark() } else { ColorTokens::light() },
        ..Default::default()
    };
}

fn button(id: &'static str, label: &'static str, primary: bool, cx: &App) -> Button {
    let t = Theme::global(cx).tokens.colors;
    let (bg, fg, hover) = if primary {
        (t.primary, t.primary_foreground, t.primary.opacity(0.85))
    } else {
        (t.background, t.foreground, t.accent)
    };
    Button::new(id)
        .h(px(30.))
        .px_3()
        .flex()
        .items_center()
        .rounded_md()
        .border_1()
        .border_color(if primary { t.primary } else { t.border })
        .bg(bg)
        .text_color(fg)
        .text_sm()
        .hover(move |s| s.bg(hover))
        .child(label)
}

fn section_title(title: &'static str, color: Hsla) -> impl IntoElement {
    div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(color).child(title)
}

fn properties_table(border: Hsla) -> gpui::Div {
    v_flex().rounded_md().border_1().border_color(border).text_sm()
}

fn kv_row(key: impl Into<SharedString>, value: SharedString, border: Hsla, muted: Hsla) -> impl IntoElement {
    h_flex()
        .px_2()
        .py_1()
        .gap_2()
        .border_b_1()
        .border_color(border)
        .child(div().w(px(72.)).flex_shrink_0().text_color(muted).child(key.into()))
        .child(div().flex_1().min_w_0().child(value))
}
