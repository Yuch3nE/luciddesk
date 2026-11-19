//! Common control layout and painting, with event handling owned by the host.
use crate::control_style::{self as style, Palette};
pub(crate) mod gdi;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Bounds {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn inset(self, x: f32, y: f32) -> Self {
        Self::new(
            self.x + x,
            self.y + y,
            (self.width - 2.0 * x).max(0.0),
            (self.height - 2.0 * y).max(0.0),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Tone {
    Surface,
    Hover,
    Border,
    Ink,
    Disabled,
    Accent,
    OnAccent,
    AccentHover,
    ScrollThumb,
    Muted,
    DisabledSurface,
    DisabledBorder,
    HoverBorder,
    AccentInk,
    Selection,
}

impl Tone {
    pub const ALL: [Self; 15] = [
        Self::Surface,
        Self::Hover,
        Self::Border,
        Self::Ink,
        Self::Disabled,
        Self::Accent,
        Self::OnAccent,
        Self::AccentHover,
        Self::ScrollThumb,
        Self::Muted,
        Self::DisabledSurface,
        Self::DisabledBorder,
        Self::HoverBorder,
        Self::AccentInk,
        Self::Selection,
    ];

    pub const fn rgb(self, palette: &Palette) -> u32 {
        match self {
            Self::Surface => palette.button,
            Self::Hover => palette.button_hover,
            Self::Border => palette.border,
            Self::Ink => palette.ink,
            Self::Disabled => palette.disabled,
            Self::Accent => palette.accent,
            Self::OnAccent => palette.on_accent,
            Self::AccentHover => palette.primary_hover,
            Self::ScrollThumb => palette.scroll_thumb,
            Self::Muted => palette.muted,
            Self::DisabledSurface => palette.disabled_surface,
            Self::DisabledBorder => palette.disabled_border,
            Self::HoverBorder => palette.hover_border,
            Self::AccentInk => palette.accent_ink,
            Self::Selection => palette.selection,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Alignment {
    Center,
    Leading,
    Icon,
    ActionIcon,
    Supporting,
}

pub trait Canvas {
    type Error;

    fn fill(&mut self, bounds: Bounds, radius: f32, tone: Tone);
    fn fill_rgb(&mut self, bounds: Bounds, radius: f32, rgb: [f32; 3]) -> Result<(), Self::Error>;
    fn stroke(&mut self, bounds: Bounds, radius: f32, thickness: f32, tone: Tone);
    fn text(&mut self, bounds: Bounds, label: &str, alignment: Alignment, tone: Tone);
}

#[derive(Clone, Copy)]
pub struct State {
    pub enabled: bool,
    pub active: bool,
    pub focused: bool,
}

impl State {
    fn border(self) -> (f32, Tone) {
        if !self.enabled {
            (1.0, Tone::DisabledBorder)
        } else if self.focused {
            (style::FOCUS_STROKE, Tone::Accent)
        } else if self.active {
            (1.0, Tone::HoverBorder)
        } else {
            (1.0, Tone::Border)
        }
    }

    fn text(self) -> Tone {
        if self.enabled {
            Tone::Ink
        } else {
            Tone::Disabled
        }
    }

    fn surface(self) -> Tone {
        if !self.enabled {
            Tone::DisabledSurface
        } else if self.active {
            Tone::Hover
        } else {
            Tone::Surface
        }
    }

    fn focus(self, canvas: &mut impl Canvas, bounds: Bounds) {
        if self.enabled && self.focused {
            canvas.stroke(bounds, style::RADIUS, style::FOCUS_STROKE, Tone::Accent);
        }
    }
}

#[derive(Clone, Copy)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Selected,
}

impl ButtonKind {
    pub fn paint(self, canvas: &mut impl Canvas, bounds: Bounds, label: &str, state: State) {
        let primary = matches!(self, Self::Primary);
        let selected = state.enabled && matches!(self, Self::Selected);
        let (fill, text) = match (self, state.enabled, state.active) {
            (Self::Primary, true, true) => (Tone::AccentHover, Tone::OnAccent),
            (Self::Primary, true, false) => (Tone::Accent, Tone::OnAccent),
            (Self::Selected, true, _) => (Tone::Selection, Tone::AccentInk),
            _ => (state.surface(), state.text()),
        };
        canvas.fill(bounds, style::RADIUS, fill);
        if primary && state.enabled {
            if state.focused {
                let inset = style::PRIMARY_FOCUS_INSET;
                canvas.stroke(
                    bounds.inset(inset, inset),
                    style::RADIUS - inset,
                    style::FOCUS_STROKE,
                    Tone::OnAccent,
                );
            }
        } else {
            let (thickness, tone) = state.border();
            canvas.stroke(
                bounds,
                style::RADIUS,
                thickness,
                if selected { Tone::Accent } else { tone },
            );
        }
        canvas.text(
            bounds.inset(style::BUTTON_PADDING / 2.0, 0.0),
            label,
            Alignment::Center,
            text,
        );
    }
}

pub struct TextField;

impl TextField {
    pub fn paint(canvas: &mut impl Canvas, bounds: Bounds, state: State) {
        canvas.fill(
            bounds,
            style::RADIUS,
            if state.enabled {
                Tone::Surface
            } else {
                Tone::DisabledSurface
            },
        );
        let (thickness, tone) = state.border();
        canvas.stroke(bounds, style::RADIUS, thickness, tone);
    }
}

pub struct ComboBox;

impl ComboBox {
    pub fn paint(canvas: &mut impl Canvas, bounds: Bounds, label: &str, state: State, open: bool) {
        let expanded = open && state.enabled;
        canvas.fill(
            bounds,
            style::RADIUS,
            if expanded {
                Tone::Selection
            } else {
                state.surface()
            },
        );
        let (thickness, tone) = State {
            focused: state.focused || open,
            ..state
        }
        .border();
        canvas.stroke(bounds, style::RADIUS, thickness, tone);
        let inset = if bounds.width < 112.0 { 8.0 } else { 12.0 };
        let icon_width = style::ICON_SLOT;
        let label_width =
            (bounds.width - 2.0 * inset - icon_width - style::ICON_GAP / 2.0).max(1.0);
        canvas.text(
            Bounds::new(bounds.x + inset, bounds.y, label_width, bounds.height),
            label,
            Alignment::Leading,
            state.text(),
        );
        canvas.text(
            Bounds::new(
                bounds.x + bounds.width - inset - icon_width,
                bounds.y,
                icon_width,
                bounds.height,
            ),
            if open {
                style::CHEVRON_UP
            } else {
                style::CHEVRON_DOWN
            },
            Alignment::Icon,
            if expanded {
                Tone::AccentInk
            } else {
                state.text()
            },
        );
    }
}

/// A destination in a sidebar or a horizontal navigation row.
pub struct NavigationItem<'a> {
    pub icon: &'a str,
    pub selected: bool,
}

impl NavigationItem<'_> {
    pub const HEIGHT: f32 = 40.0;
    pub const LABEL_INSET: f32 = 48.0;
    pub const END_INSET: f32 = 12.0;

    pub fn paint(self, canvas: &mut impl Canvas, bounds: Bounds, label: &str, state: State) {
        let selected = state.enabled && self.selected;
        if selected || (state.enabled && state.active) {
            canvas.fill(bounds, style::RADIUS, if selected { Tone::Selection } else { Tone::Hover });
        }
        if selected {
            canvas.fill(
                Bounds::new(bounds.x + 3.0, bounds.y + (bounds.height - 18.0) / 2.0, 3.0, 18.0),
                1.5,
                Tone::Accent,
            );
        }
        state.focus(canvas, bounds);
        let tone = if selected { Tone::AccentInk } else { state.text() };
        canvas.text(
            Bounds::new(bounds.x + 18.0, bounds.y, 20.0, bounds.height),
            self.icon,
            Alignment::ActionIcon,
            tone,
        );
        canvas.text(
            Bounds::new(
                bounds.x + Self::LABEL_INSET,
                bounds.y,
                (bounds.width - Self::LABEL_INSET - Self::END_INSET).max(1.0),
                bounds.height,
            ),
            label,
            Alignment::Leading,
            tone,
        );
    }
}

/// Low-emphasis actions share the same frame and icon grid.
#[derive(Clone, Copy)]
pub enum InlineControl<'a> {
    Quiet(&'a str),
    Icon { glyph: &'a str, open: bool },
    Disclosure { expanded: bool, tinted: bool },
    Link(&'a str),
}

impl InlineControl<'_> {
    const COMPACT_INSET: f32 = 4.0;
    pub const DISCLOSURE_LABEL_INSET: f32 =
        Self::COMPACT_INSET + style::ICON_SLOT + style::ICON_GAP;

    pub fn paint<C: Canvas>(self, canvas: &mut C, bounds: Bounds, label: &str, state: State) {
        let selected = state.enabled
            && matches!(
                self,
                Self::Icon { open: true, .. }
                    | Self::Disclosure { expanded: true, tinted: true }
            );
        let plain_disclosure = matches!(self, Self::Disclosure { tinted: false, .. });
        if selected || state.enabled && state.active && !plain_disclosure {
            canvas.fill(
                bounds,
                style::RADIUS,
                if selected {
                    Tone::Selection
                } else {
                    Tone::Hover
                },
            );
        }
        state.focus(canvas, bounds);
        let accent = if state.enabled {
            Tone::AccentInk
        } else {
            Tone::Disabled
        };
        let icon_slot = |x| Bounds::new(x, bounds.y, style::ICON_SLOT, bounds.height);
        let leading = |canvas: &mut C, icon: &str, inset: f32, text: Tone| {
            let label_inset = inset + style::ICON_SLOT + style::ICON_GAP;
            canvas.text(icon_slot(bounds.x + inset), icon, Alignment::Icon, text);
            canvas.text(
                Bounds::new(
                    bounds.x + label_inset,
                    bounds.y,
                    (bounds.width - label_inset - style::ICON_GAP).max(1.0),
                    bounds.height,
                ),
                label,
                Alignment::Leading,
                text,
            );
        };
        match self {
            Self::Quiet(icon) => leading(canvas, icon, 12.0, accent),
            Self::Icon { glyph, .. } => canvas.text(
                bounds,
                glyph,
                Alignment::ActionIcon,
                if selected { accent } else { state.text() },
            ),
            Self::Disclosure { expanded, tinted } => leading(
                canvas,
                if expanded {
                    style::CHEVRON_DOWN
                } else {
                    style::CHEVRON_RIGHT
                },
                Self::COMPACT_INSET,
                if !tinted && state.active { accent } else { state.text() },
            ),
            Self::Link(glyph) => {
                canvas.text(
                    Bounds::new(
                        bounds.x + 8.0,
                        bounds.y,
                        (bounds.width - 40.0).max(1.0),
                        bounds.height,
                    ),
                    label,
                    Alignment::Supporting,
                    accent,
                );
                canvas.text(
                    icon_slot(bounds.x + bounds.width - 24.0),
                    glyph,
                    Alignment::Icon,
                    accent,
                );
            }
        }
    }
}

pub struct CheckBox;

impl CheckBox {
    pub fn paint(
        canvas: &mut impl Canvas,
        bounds: Bounds,
        label: &str,
        state: State,
        checked: bool,
    ) {
        if state.enabled && state.active {
            canvas.fill(bounds, style::RADIUS, Tone::Hover);
        }
        state.focus(canvas, bounds);
        let mark = Bounds::new(
            bounds.x + 4.0,
            bounds.y + (bounds.height - 18.0) / 2.0,
            18.0,
            18.0,
        );
        if checked {
            canvas.fill(
                mark,
                3.0,
                if state.enabled {
                    Tone::Accent
                } else {
                    Tone::Disabled
                },
            );
            canvas.text(mark, style::CHECK_MARK, Alignment::Icon, Tone::OnAccent);
        } else {
            canvas.fill(mark, 3.0, state.surface());
            canvas.stroke(
                mark,
                3.0,
                1.0,
                if state.enabled {
                    if state.active {
                        Tone::HoverBorder
                    } else {
                        Tone::Border
                    }
                } else {
                    Tone::DisabledBorder
                },
            );
        }
        canvas.text(
            Bounds::new(
                bounds.x + 30.0,
                bounds.y,
                (bounds.width - 34.0).max(1.0),
                bounds.height,
            ),
            label,
            Alignment::Leading,
            state.text(),
        );
    }
}

pub struct Toggle {
    pub selected: bool,
    pub progress: f32,
}

impl Toggle {
    pub fn thumb(bounds: Bounds, progress: f32) -> Bounds {
        let radius = (bounds.height / 2.0 - 4.0).max(1.0);
        let x = bounds.x
            + bounds.height / 2.0
            + (bounds.width - bounds.height) * progress.clamp(0.0, 1.0);
        Bounds::new(
            x - radius,
            bounds.y + bounds.height / 2.0 - radius,
            radius * 2.0,
            radius * 2.0,
        )
    }

    pub fn paint<C: Canvas>(
        self,
        canvas: &mut C,
        bounds: Bounds,
        state: State,
        dark: bool,
        palette: &Palette,
    ) -> Result<(), C::Error> {
        let progress = self.progress.clamp(0.0, 1.0);
        let radius = bounds.height / 2.0;
        let thumb = Self::thumb(bounds, progress);
        let thumb_radius = thumb.height / 2.0;
        let (thickness, tone) = state.border();
        if state.enabled {
            let mix = |from: u32, to: u32| {
                [16, 8, 0].map(|shift| {
                    let a = ((from >> shift) & 255) as f32 / 255.0;
                    let b = ((to >> shift) & 255) as f32 / 255.0;
                    a + (b - a) * progress
                })
            };
            canvas.fill_rgb(
                bounds,
                radius,
                mix(
                    if state.active {
                        palette.button_hover
                    } else {
                        palette.button
                    },
                    palette.accent,
                ),
            )?;
            canvas.stroke(
                bounds,
                radius,
                thickness,
                if self.selected { Tone::Accent } else { tone },
            );
            canvas.fill_rgb(
                thumb,
                thumb_radius,
                mix(
                    if dark { palette.ink } else { palette.muted },
                    palette.on_accent,
                ),
            )?;
        } else {
            canvas.fill(bounds, radius, Tone::DisabledSurface);
            canvas.stroke(bounds, radius, thickness, tone);
            canvas.fill(thumb, thumb_radius, Tone::Disabled);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct Scrollbar {
    pub thumb: Bounds,
    top: f32,
    travel: f32,
    maximum: f32,
}

impl Scrollbar {
    pub const GUTTER: f32 = 16.0;
    pub const THUMB_WIDTH: f32 = 4.0;

    pub fn new(viewport: Bounds, maximum: f32, offset: f32) -> Option<Self> {
        let track = viewport.height - Self::GUTTER;
        if maximum <= 0.0 || track <= 0.0 {
            return None;
        }
        let height = (track * track / (track + maximum)).clamp(28.0_f32.min(track), track);
        let travel = track - height;
        let top = viewport.y + Self::GUTTER / 2.0;
        let thumb = Bounds::new(
            viewport.x + viewport.width - 10.0,
            top + travel * offset.clamp(0.0, maximum) / maximum,
            Self::THUMB_WIDTH,
            height,
        );
        Some(Self {
            thumb,
            top,
            travel,
            maximum,
        })
    }

    pub fn grab(self, y: f32) -> f32 {
        let delta = y - self.thumb.y;
        if (0.0..=self.thumb.height).contains(&delta) {
            delta
        } else {
            self.thumb.height / 2.0
        }
    }

    pub fn drag_to(self, y: f32, grab: f32) -> f32 {
        ((y - grab - self.top) / self.travel.max(1.0) * self.maximum).clamp(0.0, self.maximum)
    }

    pub fn paint_thumb(canvas: &mut impl Canvas, bounds: Bounds) {
        canvas.fill(bounds, Self::THUMB_WIDTH / 2.0, Tone::ScrollThumb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecordingCanvas {
        fills: Vec<(Bounds, Tone)>,
        borders: Vec<(f32, Tone)>,
        labels: Vec<(Bounds, Tone)>,
        icons: Vec<String>,
    }

    impl Canvas for RecordingCanvas {
        type Error = std::convert::Infallible;

        fn fill(&mut self, bounds: Bounds, _: f32, tone: Tone) {
            self.fills.push((bounds, tone));
        }

        fn fill_rgb(&mut self, _: Bounds, _: f32, _: [f32; 3]) -> Result<(), Self::Error> {
            Ok(())
        }

        fn stroke(&mut self, _: Bounds, _: f32, thickness: f32, tone: Tone) {
            self.borders.push((thickness, tone));
        }

        fn text(&mut self, bounds: Bounds, label: &str, alignment: Alignment, tone: Tone) {
            self.labels.push((bounds, tone));
            if matches!(alignment, Alignment::Icon | Alignment::ActionIcon) {
                self.icons.push(label.into());
            }
        }
    }

    #[test]
    fn checkbox_uses_a_font_glyph_for_its_check_mark() {
        let bounds = Bounds::new(0.0, 0.0, 120.0, style::HEIGHT);
        let state = State {
            enabled: true,
            active: false,
            focused: false,
        };
        let mut checked = RecordingCanvas::default();
        CheckBox::paint(&mut checked, bounds, "Enabled", state, true);
        assert_eq!(checked.icons, [style::CHECK_MARK]);
        let mut unchecked = RecordingCanvas::default();
        CheckBox::paint(&mut unchecked, bounds, "Enabled", state, false);
        assert!(unchecked.icons.is_empty());
    }

    #[test]
    fn fields_distinguish_hover_focus_and_disabled_without_moving_content() {
        let bounds = Bounds::new(0.0, 0.0, 160.0, style::HEIGHT);
        let mut field = RecordingCanvas::default();
        let hover = State {
            enabled: true,
            active: true,
            focused: false,
        };
        TextField::paint(&mut field, bounds, hover);
        assert_eq!(field.fills, [(bounds, Tone::Surface)]);
        assert_eq!(field.borders, [(1.0, Tone::HoverBorder)]);

        let mut expanded = RecordingCanvas::default();
        ComboBox::paint(&mut expanded, bounds, "Group", hover, true);
        assert_eq!(expanded.fills, [(bounds, Tone::Selection)]);
        assert_eq!(expanded.borders, [(style::FOCUS_STROKE, Tone::Accent)]);

        let mut disabled = RecordingCanvas::default();
        let state = State {
            enabled: false,
            active: true,
            focused: true,
        };
        ComboBox::paint(&mut disabled, bounds, "Group", state, true);
        assert_eq!(disabled.fills, [(bounds, Tone::DisabledSurface)]);
        assert_eq!(disabled.borders, [(1.0, Tone::DisabledBorder)]);
        assert_eq!(expanded.labels[0].0, disabled.labels[0].0);
        assert_eq!(expanded.labels[1].0, disabled.labels[1].0);
        assert!(
            disabled
                .labels
                .iter()
                .all(|(_, tone)| *tone == Tone::Disabled)
        );
    }

    #[test]
    fn inline_selection_keeps_icon_grid_and_disabled_actions_do_not_highlight() {
        let bounds = Bounds::new(16.0, 24.0, 148.0, style::HEIGHT);
        let active = State {
            enabled: true,
            active: false,
            focused: false,
        };
        let mut expanded = RecordingCanvas::default();
        InlineControl::Disclosure { expanded: true, tinted: true }.paint(&mut expanded, bounds, "Rules", active);
        assert_eq!(expanded.fills, [(bounds, Tone::Selection)]);
        assert_eq!(
            expanded.labels[1].0.x,
            bounds.x + InlineControl::DISCLOSURE_LABEL_INSET
        );

        let mut navigation = RecordingCanvas::default();
        NavigationItem { icon: "icon", selected: true }.paint(&mut navigation, bounds, "Rules", active);
        let marker = Bounds::new(bounds.x + 3.0, bounds.y + (bounds.height - 18.0) / 2.0, 3.0, 18.0);
        assert_eq!(navigation.fills, [(bounds, Tone::Selection), (marker, Tone::Accent)]);
        assert_eq!(navigation.icons, ["icon"]);
        assert_eq!(navigation.labels[1].0.x, bounds.x + NavigationItem::LABEL_INSET);
        assert!(navigation.labels.iter().all(|(_, tone)| *tone == Tone::AccentInk));

        let mut disabled = RecordingCanvas::default();
        let state = State {
            enabled: false,
            active: true,
            focused: true,
        };
        NavigationItem {
            icon: "icon",
            selected: true,
        }
        .paint(&mut disabled, bounds, "Rules", state);
        assert!(disabled.fills.is_empty());
        assert!(disabled.borders.is_empty());
        assert!(
            disabled
                .labels
                .iter()
                .all(|(_, tone)| *tone == Tone::Disabled)
        );
    }

    #[test]
    fn plain_disclosure_uses_only_text_and_focus_to_show_pointer_activity() {
        let bounds = Bounds::new(16.0, 24.0, 148.0, style::HEIGHT);
        for (expanded, glyph) in [(false, style::CHEVRON_RIGHT), (true, style::CHEVRON_DOWN)] {
            for active in [false, true] {
                let mut canvas = RecordingCanvas::default();
                let state = State { enabled: true, active, focused: true };
                InlineControl::Disclosure { expanded, tinted: false }
                    .paint(&mut canvas, bounds, "Skipped icons (1)", state);
                assert!(canvas.fills.is_empty());
                assert_eq!(canvas.icons, [glyph]);
                assert_eq!(canvas.borders, [(style::FOCUS_STROKE, Tone::Accent)]);
                assert_eq!(canvas.labels[1].1, if active { Tone::AccentInk } else { Tone::Ink });
            }
        }
    }

    #[test]
    fn selected_choices_use_the_same_tint_and_keep_the_focus_outline() {
        let bounds = Bounds::new(0.0, 0.0, 104.0, style::HEIGHT);
        let mut canvas = RecordingCanvas::default();
        let state = State {
            enabled: true,
            active: true,
            focused: true,
        };
        ButtonKind::Selected.paint(&mut canvas, bounds, "Choice", state);
        assert_eq!(canvas.fills, [(bounds, Tone::Selection)]);
        assert_eq!(canvas.borders, [(style::FOCUS_STROKE, Tone::Accent)]);
        assert_eq!(canvas.labels[0].1, Tone::AccentInk);
    }

    #[test]
    fn scrollbar_drag_maps_the_full_range_and_hides_when_content_fits() {
        let viewport = Bounds::new(20.0, 30.0, 160.0, 240.0);
        assert!(Scrollbar::new(viewport, 0.0, 0.0).is_none());
        let bar = Scrollbar::new(viewport, 480.0, 120.0).unwrap();
        let grab = bar.grab(bar.thumb.y + 5.0);
        assert_eq!(grab, 5.0);
        assert!((bar.drag_to(bar.thumb.y + grab, grab) - 120.0).abs() < 0.01);
        assert_eq!(bar.drag_to(viewport.y, grab), 0.0);
        assert_eq!(bar.drag_to(viewport.y + viewport.height, grab), 480.0);
    }
}
