use gpui::{
    App, AppContext as _, Axis, Context, Entity, FocusHandle, Focusable as _, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, KeyBinding, MouseButton, ParentElement as _, Pixels,
    Point, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    Window, WindowAppearance, actions, anchored, deferred, div, prelude::FluentBuilder as _, px,
};
use gpui_base::input::{InputEditorStyle, InputEvent, InputState};
use gpui_base::{
    Button, ColorTokens, Input, InputBase, Popover, SemanticThemeTokens, Switch, SwitchThumb,
    SwitchTrack, Theme, Toggle, ToggleGroup, h_flex, h_resizable, resizable_panel, v_flex,
};
use gpui_graph::{ColorBy, LabelMode, NetworkGraph, NetworkGraphEvent, Selection};

use crate::source::{GraphSource, MemorySource, sample_movies};

const MENU_CONTEXT: &str = "GraphContextMenu";

actions!(query_app, [FocusSearch, CloseMenu, MenuNext, MenuPrevious, MenuConfirm]);

pub fn init(cx: &mut App) {
    let menu = Some(MENU_CONTEXT);
    cx.bind_keys([
        KeyBinding::new(if cfg!(target_os = "macos") { "cmd-l" } else { "ctrl-l" }, FocusSearch, None),
        KeyBinding::new("escape", CloseMenu, menu),
        KeyBinding::new("down", MenuNext, menu),
        KeyBinding::new("up", MenuPrevious, menu),
        KeyBinding::new("enter", MenuConfirm, menu),
    ]);
}

/// Everything the context menu can do. Clicks and keyboard both go through
/// [`GraphQueryApp::run_command`].
#[derive(Clone)]
enum MenuCommand {
    Expand(SharedString),
    SetPinned(SharedString, bool),
    CollapseGroup(SharedString),
    ExpandGroup(SharedString),
    Hide(Vec<SharedString>),
}

struct ContextMenu {
    title: SharedString,
    items: Vec<(SharedString, MenuCommand)>,
    highlighted: usize,
    position: Point<Pixels>,
    focus: FocusHandle,
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
                .placeholder("Search… e.g. keanu depth 2, *, random 2000")
                .default_value("keanu depth 2")
        });
        let graph = cx.new(NetworkGraph::new);

        let subscriptions = vec![
            cx.subscribe_in(&query, window, |this, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.run_query(window, cx);
                }
            }),
            cx.subscribe_in(&graph, window, |this, _, ev: &NetworkGraphEvent, window, cx| match ev {
                NetworkGraphEvent::SelectionChanged(selection) => {
                    this.selection = selection.clone();
                    cx.notify();
                }
                NetworkGraphEvent::NodeActivated(node) => this.expand(&node.id, cx),
                NetworkGraphEvent::ContextMenu { target, position } => {
                    this.open_menu(target, *position, window, cx);
                }
                _ => cx.notify(),
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

    // ---- context menu -------------------------------------------------------

    fn open_menu(&mut self, target: &Selection, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let graph = self.graph.read(cx);
        let (title, items) = match target {
            Selection::Node(node) => {
                let id = node.id.clone();
                let pinned = graph.is_pinned(&id);
                let mut items = vec![
                    ("Expand neighbors".into(), MenuCommand::Expand(id.clone())),
                    (
                        if pinned { "Unpin" } else { "Pin position" }.into(),
                        MenuCommand::SetPinned(id.clone(), !pinned),
                    ),
                ];
                if let Some(group) = node.group.clone() {
                    items.push(("Collapse community".into(), MenuCommand::CollapseGroup(group)));
                }
                items.push(("Hide".into(), MenuCommand::Hide(vec![id])));
                (node.label.clone(), items)
            }
            Selection::Group(group) => {
                let members = graph.group_members(&group.id).into_iter().map(|n| n.id).collect();
                (
                    group.label.clone(),
                    vec![
                        ("Expand community".into(), MenuCommand::ExpandGroup(group.id.clone())),
                        ("Hide members".into(), MenuCommand::Hide(members)),
                    ],
                )
            }
            _ => return,
        };
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.menu = Some(ContextMenu { title, items, highlighted: 0, position, focus });
        cx.notify();
    }

    /// Close the menu and return focus to the graph it was opened from.
    fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            let focus = self.graph.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
            cx.notify();
        }
    }

    fn run_command(&mut self, command: MenuCommand, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(window, cx);
        match command {
            MenuCommand::Expand(id) => self.expand(&id, cx),
            MenuCommand::SetPinned(id, pinned) => self.graph.update(cx, |g, cx| g.set_pinned(&id, pinned, cx)),
            MenuCommand::CollapseGroup(id) => self.graph.update(cx, |g, cx| g.collapse_group(&id, cx)),
            MenuCommand::ExpandGroup(id) => self.graph.update(cx, |g, cx| g.expand_group(&id, cx)),
            MenuCommand::Hide(ids) => {
                let ids: Vec<&str> = ids.iter().map(|s| s.as_ref()).collect();
                self.graph.update(cx, |g, cx| g.remove_nodes(&ids, cx));
                self.refresh_status(cx);
            }
        }
    }

    fn on_menu_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(menu) = &mut self.menu {
            let n = menu.items.len() as isize;
            menu.highlighted = (menu.highlighted as isize + delta).rem_euclid(n) as usize;
            cx.notify();
        }
    }

    fn on_menu_confirm(&mut self, _: &MenuConfirm, window: &mut Window, cx: &mut Context<Self>) {
        let command = self.menu.as_ref().map(|m| m.items[m.highlighted].1.clone());
        if let Some(command) = command {
            self.run_command(command, window, cx);
        }
    }

    fn on_focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.query.update(cx, |state, cx| state.focus(window, cx));
    }

    // ---- regions --------------------------------------------------------------

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::global(cx).tokens.colors;
        let query = self.query.clone();

        h_flex()
            .h_12()
            .px_3()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(t.border)
            .bg(t.surface)
            .child(div().font_weight(FontWeight::SEMIBOLD).mr_2().child("Graph query"))
            .child(
                InputBase::new("query")
                    .flex_1()
                    .h_8()
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
            .child(primary_button("run", "Run", cx).on_click(cx.listener(|this, _, window, cx| {
                this.run_query(window, cx);
            })))
            .child(divider(t.border))
            .child(ghost_button("fit", "Fit", cx).on_click(cx.listener(|this, _, _, cx| {
                this.graph.update(cx, |g, cx| g.fit_view(cx));
            })))
            .child(
                ghost_button("zoom-out", "−", cx)
                    .accessibility_label("Zoom out")
                    .on_click(cx.listener(|this, _, _, cx| this.graph.update(cx, |g, cx| g.zoom_by(0.8, cx)))),
            )
            .child(
                ghost_button("zoom-in", "+", cx)
                    .accessibility_label("Zoom in")
                    .on_click(cx.listener(|this, _, _, cx| this.graph.update(cx, |g, cx| g.zoom_by(1.25, cx)))),
            )
            .child(ghost_button("relayout", "Re-layout", cx).on_click(cx.listener(|this, _, _, cx| {
                this.graph.update(cx, |g, cx| g.relayout(cx));
            })))
            .child(self.render_view_options(cx))
    }

    /// Display settings live in one popover instead of a row of stateful buttons.
    fn render_view_options(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::global(cx).tokens.colors;
        let graph = self.graph.clone();
        Popover::new("view-options")
            .trigger(ghost_button("view-trigger", "View", cx))
            .content(move |_, _, cx| {
                let g = graph.read(cx);
                let (label_mode, color_by) = (g.label_mode(), g.color_by());
                let (arrows, edge_labels, hulls) = (g.has_arrows(), g.has_edge_labels(), g.has_group_hulls());
                let set = |f: fn(&mut NetworkGraph, bool, &mut Context<NetworkGraph>)| {
                    let graph = graph.clone();
                    move |on: bool, _: &mut Window, cx: &mut App| graph.update(cx, |g, cx| f(g, on, cx))
                };
                v_flex()
                    .id("view-options-content")
                    .w_72()
                    .p_3()
                    .gap_3()
                    .rounded_md()
                    .border_1()
                    .border_color(t.border)
                    .bg(t.surface)
                    .text_sm()
                    .text_color(t.foreground)
                    .child(option_row(
                        "Labels",
                        segmented(
                            "label-mode",
                            &[
                                ("Auto", LabelMode::Auto),
                                ("All", LabelMode::Always),
                                ("Focus", LabelMode::Focus),
                                ("Off", LabelMode::Never),
                            ],
                            label_mode,
                            {
                                let graph = graph.clone();
                                move |mode, cx| graph.update(cx, |g, cx| g.set_label_mode(mode, cx))
                            },
                            cx,
                        ),
                    ))
                    .child(option_row(
                        "Color by",
                        segmented(
                            "color-by",
                            &[("Type", ColorBy::Category), ("Community", ColorBy::Group)],
                            color_by,
                            {
                                let graph = graph.clone();
                                move |c, cx| graph.update(cx, |g, cx| g.set_color_by(c, cx))
                            },
                            cx,
                        ),
                    ))
                    .child(option_row("Arrows", switch("arrows", arrows, set(NetworkGraph::set_arrows), cx)))
                    .child(option_row(
                        "Edge labels",
                        switch("edge-labels", edge_labels, set(NetworkGraph::set_edge_labels), cx),
                    ))
                    .child(option_row(
                        "Community outlines",
                        switch("hulls", hulls, set(NetworkGraph::set_group_hulls), cx),
                    ))
            })
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::global(cx).tokens.colors;
        let graph = self.graph.read(cx);
        let legend = graph.categories();
        let groups = graph.groups();

        v_flex()
            .id("sidebar")
            .size_full()
            .p_4()
            .gap_4()
            .border_l_1()
            .border_color(t.border)
            .bg(t.surface)
            .overflow_y_scroll()
            .child(section_title("Types", t.muted_foreground))
            .child(v_flex().gap_1().children(legend.into_iter().map(|c| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_sm()
                    .child(div().size_2p5().rounded_full().bg(c.color))
                    .child(div().flex_1().child(c.name))
                    .child(div().text_color(t.muted_foreground).child(c.count.to_string()))
            })))
            .when(!groups.is_empty(), |this| {
                this.child(div().h(px(1.)).bg(t.border))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(div().flex_1().child(section_title("Communities", t.muted_foreground)))
                            .child(ghost_button("collapse-all", "Collapse all", cx).on_click(cx.listener(
                                |this, _, _, cx| this.graph.update(cx, |g, cx| g.collapse_all_groups(cx)),
                            )))
                            .child(ghost_button("expand-all", "Expand all", cx).on_click(cx.listener(
                                |this, _, _, cx| this.graph.update(cx, |g, cx| g.expand_all_groups(cx)),
                            ))),
                    )
                    .child(v_flex().gap_0p5().children(groups.into_iter().map(|g| {
                        let id = g.id.clone();
                        Button::new(SharedString::from(format!("group-{}", g.id)))
                            .w_full()
                            .flex()
                            .gap_2()
                            .items_center()
                            .px_1()
                            .py_0p5()
                            .rounded_md()
                            .text_sm()
                            .hover(|s| s.bg(t.accent))
                            .accessibility_label(format!(
                                "{} {}",
                                if g.collapsed { "Expand" } else { "Collapse" },
                                g.label
                            ))
                            .child(div().w_3().text_color(t.muted_foreground).child(if g.collapsed { "▸" } else { "▾" }))
                            .child(div().size_2p5().rounded_full().bg(g.color))
                            .child(div().flex_1().child(g.label))
                            .child(div().text_color(t.muted_foreground).child(g.size.to_string()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.graph.update(cx, |g, cx| g.toggle_group(&id, cx));
                            }))
                    })))
            })
            .child(div().h(px(1.)).bg(t.border))
            .child(section_title("Details", t.muted_foreground))
            .child(self.render_details(cx))
    }

    fn render_details(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::global(cx).tokens.colors;
        let graph = self.graph.read(cx);
        match &self.selection {
            Selection::Node(node) => {
                let color = graph.color_of(node, cx);
                let pinned = graph.is_pinned(&node.id);
                let neighbors = graph.neighbors_of(&node.id);
                v_flex()
                    .gap_3()
                    .child(heading(node.category.clone(), node.label.clone(), Some(color), t.muted_foreground)
                        .when(pinned, |this| this.child(div().text_xs().text_color(t.muted_foreground).child("Pinned"))))
                    .child(
                        properties_table(t.border)
                            .child(kv_row("id", node.id.clone(), t.border, t.muted_foreground))
                            .children(node.properties.iter().map(|(k, v)| kv_row(k.clone(), v.clone(), t.border, t.muted_foreground))),
                    )
                    .child(section_title("Neighbors", t.muted_foreground))
                    .child(v_flex().gap_0p5().children(neighbors.into_iter().map(|n| {
                        let id = n.id.clone();
                        Button::new(SharedString::from(format!("neighbor-{}", n.id)))
                            .w_full()
                            .flex()
                            .gap_2()
                            .items_center()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_sm()
                            .hover(|s| s.bg(t.accent))
                            .child(div().size_2().rounded_full().bg(graph.color_of(&n, cx)))
                            .child(n.label.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.graph.update(cx, |g, cx| g.select_node(Some(&id), true, cx));
                            }))
                    })))
                    .into_any_element()
            }
            Selection::Group(group) => {
                let id = group.id.clone();
                let members = graph.group_members(&group.id);
                let shown = members.len().min(50);
                v_flex()
                    .gap_3()
                    .child(heading("Community (collapsed)".into(), group.label.clone(), Some(group.color), t.muted_foreground))
                    .child(primary_button("expand-group", "Expand", cx).on_click(cx.listener(move |this, _, _, cx| {
                        this.graph.update(cx, |g, cx| g.expand_group(&id, cx));
                    })))
                    .child(section_title("Members", t.muted_foreground))
                    .child(v_flex().gap_0p5().text_sm().children(members.into_iter().take(shown).map(|n| {
                        h_flex()
                            .gap_2()
                            .items_center()
                            .px_2()
                            .child(div().size_2().rounded_full().bg(graph.color_of(&n, cx)))
                            .child(n.label.clone())
                    })))
                    .when(group.size > shown, |this| {
                        this.child(div().text_xs().text_color(t.muted_foreground).child(format!("{} more not shown", group.size - shown)))
                    })
                    .into_any_element()
            }
            Selection::EdgeBundle(edges) => {
                let mut by_label: Vec<(SharedString, usize)> = Vec::new();
                for e in edges {
                    match by_label.iter_mut().find(|(l, _)| l == &e.label) {
                        Some((_, n)) => *n += 1,
                        None => by_label.push((e.label.clone(), 1)),
                    }
                }
                v_flex()
                    .gap_3()
                    .child(heading("Merged relationships".into(), format!("{} edges", edges.len()).into(), None, t.muted_foreground))
                    .child(properties_table(t.border).children(
                        by_label.into_iter().map(|(label, n)| kv_row(label, n.to_string().into(), t.border, t.muted_foreground)),
                    ))
                    .into_any_element()
            }
            Selection::Edge(edge) => {
                let source = graph.node(&edge.source).map(|n| n.label.clone()).unwrap_or_default();
                let target = graph.node(&edge.target).map(|n| n.label.clone()).unwrap_or_default();
                v_flex()
                    .gap_3()
                    .child(heading("Relationship".into(), edge.label.clone(), None, t.muted_foreground))
                    .child(
                        properties_table(t.border)
                            .child(kv_row("from", source, t.border, t.muted_foreground))
                            .child(kv_row("to", target, t.border, t.muted_foreground))
                            .children(edge.properties.iter().map(|(k, v)| kv_row(k.clone(), v.clone(), t.border, t.muted_foreground))),
                    )
                    .into_any_element()
            }
            _ => v_flex()
                .gap_1()
                .text_sm()
                .text_color(t.muted_foreground)
                .child("Select a node or relationship to see its properties.")
                .child("Double-click a node to expand it. Right-click for more actions.")
                .child("Keyboard: arrows move the selection, Enter expands, Shift-F10 opens actions.")
                .into_any_element(),
        }
    }

    /// Shown over the graph when there is nothing to draw.
    fn render_empty_state(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.graph.read(cx).node_count() > 0 {
            return None;
        }
        let t = Theme::global(cx).tokens.colors;
        let (title, hint): (SharedString, SharedString) = match &self.error {
            Some(err) => ("No results".into(), err.clone()),
            None => ("No results".into(), "Try * to load everything, or another keyword.".into()),
        };
        Some(
            v_flex()
                .absolute()
                .inset_0()
                .items_center()
                .justify_center()
                .gap_1()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().text_sm().text_color(t.muted_foreground).child(hint)),
        )
    }

    fn render_context_menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let menu = self.menu.as_ref()?;
        let t = Theme::global(cx).tokens.colors;
        Some(deferred(
            anchored().position(menu.position).snap_to_window().child(
                v_flex()
                    .id("graph-context-menu")
                    .track_focus(&menu.focus)
                    .key_context(MENU_CONTEXT)
                    .on_action(cx.listener(|this, _: &CloseMenu, window, cx| this.close_menu(window, cx)))
                    .on_action(cx.listener(|this, _: &MenuNext, _, cx| this.on_menu_step(1, cx)))
                    .on_action(cx.listener(|this, _: &MenuPrevious, _, cx| this.on_menu_step(-1, cx)))
                    .on_action(cx.listener(Self::on_menu_confirm))
                    .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close_menu(window, cx)))
                    .min_w_48()
                    .p_1()
                    .rounded_md()
                    .border_1()
                    .border_color(t.border)
                    .bg(t.surface)
                    .shadow_lg()
                    .text_sm()
                    .text_color(t.foreground)
                    .child(div().px_2().py_1().text_xs().text_color(t.muted_foreground).child(menu.title.clone()))
                    .children(menu.items.iter().enumerate().map(|(ix, (label, command))| {
                        let command = command.clone();
                        Button::new(("menu-item", ix))
                            .w_full()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .when(ix == menu.highlighted, |this| this.bg(t.accent))
                            .hover(|s| s.bg(t.accent))
                            .child(label.clone())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.run_command(command.clone(), window, cx);
                            }))
                    })),
            ),
        ))
    }
}

impl Render for GraphQueryApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::global(cx).tokens.colors;
        let sidebar_width = window.rem_size() * 20.;
        v_flex()
            .id("query-app")
            .on_action(cx.listener(Self::on_focus_search))
            .size_full()
            .bg(t.background)
            .text_color(t.foreground)
            .child(self.render_toolbar(cx))
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("workspace")
                        .child(
                            resizable_panel().child(
                                div()
                                    .relative()
                                    .size_full()
                                    .child(self.graph.clone())
                                    .children(self.render_empty_state(cx)),
                            ),
                        )
                        .child(
                            resizable_panel()
                                .size(sidebar_width)
                                .size_range(sidebar_width * 0.7..sidebar_width * 2.)
                                .child(self.render_sidebar(cx)),
                        ),
                ),
            )
            .child(
                h_flex()
                    .h_7()
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
            .children(self.render_context_menu(cx))
    }
}

fn apply_theme(window: &Window, cx: &mut App) {
    let dark = matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark);
    Theme::global_mut(cx).tokens = SemanticThemeTokens {
        colors: if dark { ColorTokens::dark() } else { ColorTokens::light() },
        ..Default::default()
    };
}

/// The single default action in a region.
fn primary_button(id: &'static str, label: &'static str, cx: &App) -> Button {
    let t = Theme::global(cx).tokens.colors;
    Button::new(id)
        .h_8()
        .px_3()
        .flex()
        .items_center()
        .rounded_md()
        .bg(t.primary)
        .text_color(t.primary_foreground)
        .text_sm()
        .hover(move |s| s.bg(t.primary.opacity(0.85)))
        .child(label)
}

/// Low-emphasis toolbar action.
fn ghost_button(id: &'static str, label: &'static str, cx: &App) -> Button {
    let t = Theme::global(cx).tokens.colors;
    Button::new(id)
        .h_8()
        .px_2p5()
        .flex()
        .items_center()
        .rounded_md()
        .text_color(t.foreground)
        .text_sm()
        .hover(move |s| s.bg(t.accent))
        .child(label)
}

fn divider(color: Hsla) -> impl IntoElement {
    div().w(px(1.)).h_5().bg(color)
}

fn option_row(label: &'static str, control: impl IntoElement) -> impl IntoElement {
    h_flex().gap_3().items_center().justify_between().child(label).child(control)
}

fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: &[(&'static str, T)],
    current: T,
    on_select: impl Fn(T, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let t = Theme::global(cx).tokens.colors;
    ToggleGroup::new(id)
        .axis(Axis::Horizontal)
        .flex()
        .p_0p5()
        .gap_0p5()
        .rounded_md()
        .bg(t.muted)
        .children(options.iter().enumerate().map(|(ix, &(label, value))| {
            let on_select = on_select.clone();
            let pressed = value == current;
            Toggle::new((id, ix))
                .pressed(pressed)
                .px_2()
                .py_0p5()
                .rounded_sm()
                .text_xs()
                .text_color(if pressed { t.foreground } else { t.muted_foreground })
                .when(pressed, |this| this.bg(t.background).shadow_sm())
                .child(label)
                .on_change(move |_, _, _, cx| on_select(value, cx))
        }))
}

fn switch(
    id: &'static str,
    checked: bool,
    on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let t = Theme::global(cx).tokens.colors;
    Switch::new(id)
        .checked(checked)
        .on_change(move |next, _, window, cx| on_change(next, window, cx))
        .child(
            SwitchTrack::new((id, 0usize))
                .checked(checked)
                .w_9()
                .h_5()
                .p_0p5()
                .rounded_full()
                .bg(if checked { t.primary } else { t.input })
                .child(
                    SwitchThumb::new(checked)
                        .size_4()
                        .rounded_full()
                        .bg(t.background)
                        .when(checked, |this| this.ml_4()),
                ),
        )
}

fn section_title(title: &'static str, color: Hsla) -> impl IntoElement {
    div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(color).child(title)
}

fn heading(kicker: SharedString, title: SharedString, swatch: Option<Hsla>, muted: Hsla) -> gpui::Div {
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .children(swatch.map(|c| div().size_2p5().rounded_full().bg(c)))
                .child(div().text_xs().text_color(muted).child(kicker)),
        )
        .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(title))
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
        .child(div().w_20().flex_shrink_0().text_color(muted).child(key.into()))
        .child(div().flex_1().min_w_0().child(value))
}
