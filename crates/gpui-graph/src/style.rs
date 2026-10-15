use gpui::{App, BoxShadow, Hsla, Pixels};
use gpui_base::{ColorTokens, TextStyleToken, Theme};

use crate::layout::with_fields;

/// Categorical palette for data, after AntV's. The base theme has no chart
/// roles, so this is the one place the graph defines raw colors; replace it
/// with [`NetworkGraph::with_palette`](crate::NetworkGraph::with_palette) to
/// match a product theme.
const DEFAULT_PALETTE: [u32; 10] = [
    0x5b8ff9, 0x61ddaa, 0x65789b, 0xf6bd16, 0x7262fd, 0x78d3f8, 0x9661bc, 0xf6903d, 0x008685,
    0xf08bb4,
];

/// Colors the graph paints with. By default they are derived from the
/// gpui-base [`Theme`] every frame, so appearance changes apply immediately.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct GraphColors {
    pub background: Hsla,
    /// Tooltip and label-chip background.
    pub surface: Hsla,
    pub foreground: Hsla,
    pub muted: Hsla,
    pub border: Hsla,
    pub edge: Hsla,
    /// Edges touching the hovered / selected item.
    pub edge_active: Hsla,
    /// Ring around the selected item.
    pub selection: Hsla,
    /// Keyboard focus indicator, distinct from selection.
    pub focus_ring: Hsla,
    /// Nodes without a group when coloring by group.
    pub ungrouped: Hsla,
    /// Text drawn on top of node fills; the one with better contrast is used.
    pub on_fill_light: Hsla,
    pub on_fill_dark: Hsla,
}

impl GraphColors {
    pub fn from_theme(cx: &App) -> Self {
        let t = Theme::global(cx).tokens.colors;
        Self {
            background: t.background,
            surface: t.surface,
            foreground: t.foreground,
            muted: t.muted_foreground,
            border: t.border,
            edge: t.muted_foreground.opacity(0.45),
            edge_active: t.foreground.opacity(0.8),
            selection: t.primary,
            focus_ring: t.ring,
            ungrouped: t.muted_foreground.opacity(0.6),
            on_fill_light: ColorTokens::light().background,
            on_fill_dark: ColorTokens::light().foreground,
        }
    }

    /// Readable text color on `fill`.
    pub fn on_fill(&self, fill: Hsla) -> Hsla {
        if fill.l > 0.62 {
            self.on_fill_dark
        } else {
            self.on_fill_light
        }
    }
}

with_fields!(GraphColors {
    background: Hsla => with_background,
    surface: Hsla => with_surface,
    foreground: Hsla => with_foreground,
    muted: Hsla => with_muted,
    border: Hsla => with_border,
    edge: Hsla => with_edge,
    edge_active: Hsla => with_edge_active,
    selection: Hsla => with_selection,
    focus_ring: Hsla => with_focus_ring,
    ungrouped: Hsla => with_ungrouped,
});

/// Type, radius and spacing scales the graph reads from the theme.
#[derive(Clone, Debug)]
pub(crate) struct GraphMetrics {
    /// Node labels.
    pub label: TextStyleToken,
    /// Edge labels, counts.
    pub caption: TextStyleToken,
    pub radius_sm: Pixels,
    pub radius_md: Pixels,
    pub space_xs: Pixels,
    pub space_sm: Pixels,
    pub space_md: Pixels,
    pub shadow: Vec<BoxShadow>,
}

impl GraphMetrics {
    pub fn from_theme(cx: &App) -> Self {
        let t = Theme::global(cx).tokens;
        Self {
            label: t.typography.sm,
            caption: t.typography.xs,
            radius_sm: t.radius.sm,
            radius_md: t.radius.md,
            space_xs: t.spacing.xs,
            space_sm: t.spacing.sm,
            space_md: t.spacing.md,
            shadow: t.shadow.md,
        }
    }
}

/// Category / group colors, assigned in the order they first appear.
#[derive(Clone, Debug)]
pub struct Palette(pub Vec<Hsla>);

impl Palette {
    /// Color for the `ix`-th category / group. An empty palette yields the theme's muted color.
    pub fn get(&self, ix: usize) -> Hsla {
        match self.0.len() {
            0 => ColorTokens::default().muted_foreground,
            n => self.0[ix % n],
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self(
            DEFAULT_PALETTE
                .iter()
                .map(|&c| gpui::rgb(c).into())
                .collect(),
        )
    }
}

/// How node radius (in world units) is derived when a node has no explicit size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NodeSizing {
    Fixed(f32),
    /// Area grows with degree ("node importance"), clamped to `max`.
    Degree {
        min: f32,
        max: f32,
    },
}

impl Default for NodeSizing {
    fn default() -> Self {
        Self::Degree { min: 7., max: 28. }
    }
}

impl NodeSizing {
    pub fn radius(&self, degree: usize) -> f32 {
        match *self {
            Self::Fixed(r) => r,
            Self::Degree { min, max } => (min + (degree as f32).sqrt() * 2.6).min(max),
        }
    }
}

/// Which node labels are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelMode {
    /// Only the hovered / selected node and its neighbors.
    Focus,
    /// Every node once the zoom makes labels readable; focus set otherwise.
    #[default]
    Auto,
    Always,
    Never,
}

/// What node colors encode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorBy {
    /// [`Node::category`](crate::Node::category), e.g. vertex type.
    #[default]
    Category,
    /// [`Node::group`](crate::Node::group), e.g. community.
    Group,
}
