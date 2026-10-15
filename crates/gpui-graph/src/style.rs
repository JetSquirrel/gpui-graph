use gpui::{App, Hsla};
use gpui_base::Theme;

/// AntV's categorical palette, so results look familiar to G6 users.
const ANTV: [u32; 10] = [
    0x5b8ff9, 0x61ddaa, 0x65789b, 0xf6bd16, 0x7262fd, 0x78d3f8, 0x9661bc, 0xf6903d, 0x008685,
    0xf08bb4,
];

/// Colors the graph paints with. By default they follow the gpui-base
/// [`Theme`] tokens every frame, so light/dark switches apply immediately.
#[derive(Clone, Debug)]
pub struct GraphColors {
    pub background: Hsla,
    /// Tooltip and label-chip background.
    pub surface: Hsla,
    pub foreground: Hsla,
    pub muted: Hsla,
    pub border: Hsla,
    pub edge: Hsla,
    /// Edges touching the hovered / selected node.
    pub edge_active: Hsla,
    /// Ring around the selected node.
    pub selection: Hsla,
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
            selection: t.ring,
        }
    }
}

/// Category colors, assigned in the order categories first appear.
#[derive(Clone, Debug)]
pub struct Palette(pub Vec<Hsla>);

impl Palette {
    pub fn get(&self, index: usize) -> Hsla {
        if self.0.is_empty() {
            return gpui::hsla(0., 0., 0.5, 1.);
        }
        self.0[index % self.0.len()]
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self(ANTV.iter().map(|&c| gpui::rgb(c).into()).collect())
    }
}

/// How node radius (in world units) is derived when a node has no explicit size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NodeSizing {
    Fixed(f32),
    /// Area grows with degree ("node importance"), clamped to `max`.
    Degree { min: f32, max: f32 },
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
