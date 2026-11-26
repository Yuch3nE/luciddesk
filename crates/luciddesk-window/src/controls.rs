//! Common control layout and painting, with event handling owned by the host.
use crate::control_style::{self as style, Palette};
pub(crate) mod gdi;
pub mod motion;

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
    Success,
    Danger,
    Info,
}

impl Tone {
    pub const ALL: [Self; 18] = [
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
        Self::Success,
        Self::Danger,
        Self::Info,
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
            Self::Success => palette.success,
            Self::Danger => palette.danger,
            Self::Info => palette.info,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Alignment {
    Center,
    SelectedCenter,
    Leading,
    Icon,
    ActionIcon,
    Supporting,
    Badge,
}

pub trait Canvas {
    type Error;

    fn fill(&mut self, bounds: Bounds, radius: f32, tone: Tone);
    fn fill_rgb(&mut self, bounds: Bounds, radius: f32, rgb: [f32; 3]) -> Result<(), Self::Error>;
    fn fill_rgba(&mut self, bounds: Bounds, radius: f32, rgba: [f32; 4]) {
        if rgba[3] >= 0.99 {
            let _ = self.fill_rgb(bounds, radius, [rgba[0], rgba[1], rgba[2]]);
        }
    }
    fn stroke(&mut self, bounds: Bounds, radius: f32, thickness: f32, tone: Tone);
    fn stroke_rgb(
        &mut self,
        bounds: Bounds,
        radius: f32,
        thickness: f32,
        _rgb: [f32; 3],
    ) -> Result<(), Self::Error> {
        self.stroke(bounds, radius, thickness, Tone::Border);
        Ok(())
    }
    fn stroke_rgba(&mut self, bounds: Bounds, radius: f32, thickness: f32, rgba: [f32; 4]) {
        if rgba[3] >= 0.99 {
            let _ = self.stroke_rgb(bounds, radius, thickness, [rgba[0], rgba[1], rgba[2]]);
        } else {
            self.stroke(bounds, radius, thickness, Tone::Border);
        }
    }
    fn elevation_border(
        &mut self,
        bounds: Bounds,
        radius: f32,
        kind: style::ElevationKind,
        hover: f32,
        press: f32,
    ) {
        match kind {
            style::ElevationKind::Accent => {}
            style::ElevationKind::Control | style::ElevationKind::TextField => {
                self.stroke_between(
                    bounds,
                    radius,
                    1.0,
                    Tone::Border,
                    Tone::HoverBorder,
                    hover.max(press),
                );
            }
        }
    }
    fn fill_circle(&mut self, _cx: f32, _cy: f32, _radius: f32, _rgba: [f32; 4]) {}
    fn stroke_circle(
        &mut self,
        _cx: f32,
        _cy: f32,
        _radius: f32,
        _thickness: f32,
        _rgba: [f32; 4],
    ) {}
    fn icon_transformed(
        &mut self,
        bounds: Bounds,
        glyph: &str,
        tone: Tone,
        dx: f32,
        dy: f32,
        _radians: f32,
    ) {
        self.text(
            Bounds::new(bounds.x + dx, bounds.y + dy, bounds.width, bounds.height),
            glyph,
            Alignment::Icon,
            tone,
        );
    }
    fn text(&mut self, bounds: Bounds, label: &str, alignment: Alignment, tone: Tone);
    fn palette(&self) -> &Palette;

    fn stroke_between(
        &mut self,
        bounds: Bounds,
        radius: f32,
        thickness: f32,
        from: Tone,
        to: Tone,
        progress: f32,
    ) {
        if progress <= 0.0 || progress >= 1.0 {
            self.stroke(bounds, radius, thickness, if progress <= 0.0 { from } else { to });
            return;
        }
        let rgb = mix_tones(from, to, self.palette(), progress);
        if self.stroke_rgb(bounds, radius, thickness, rgb).is_err() {
            self.stroke(bounds, radius, thickness, if progress < 0.5 { from } else { to });
        }
    }

    fn fill_between(&mut self, bounds: Bounds, radius: f32, from: Tone, to: Tone, progress: f32) {
        if progress <= 0.0 || progress >= 1.0 {
            self.fill(bounds, radius, if progress <= 0.0 { from } else { to });
            return;
        }
        let rgb = mix_tones(from, to, self.palette(), progress);
        if self.fill_rgb(bounds, radius, rgb).is_err() {
            self.fill(bounds, radius, if progress < 0.5 { from } else { to });
        }
    }
}

pub fn mix_tones(from: Tone, to: Tone, palette: &Palette, progress: f32) -> [f32; 3] {
    let from = from.rgb(palette);
    let to = to.rgb(palette);
    [16, 8, 0].map(|shift| {
        let a = ((from >> shift) & 255) as f32;
        let b = ((to >> shift) & 255) as f32;
        (a + (b - a) * progress) / 255.0
    })
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
        self.paint_with_hover(canvas, bounds, label, state, f32::from(u8::from(state.active)));
    }

    pub fn paint_with_hover(self, canvas: &mut impl Canvas, bounds: Bounds, label: &str, state: State, hover: f32) {
        self.paint_animated(canvas, bounds, label, state, hover, 0.0);
    }

    pub fn paint_animated(
        self,
        canvas: &mut impl Canvas,
        bounds: Bounds,
        label: &str,
        state: State,
        hover: f32,
        press: f32,
    ) {
        let primary = matches!(self, Self::Primary);
        let selected = state.enabled && matches!(self, Self::Selected);
        let hover = hover.clamp(0.0, 1.0);
        let press = press.clamp(0.0, 1.0);
        let text = match (self, state.enabled) {
            (Self::Primary, true) => {
                let effective = (hover * (1.0 - 0.65 * press)).clamp(0.0, 1.0);
                if press > 0.0 {
                    let base = mix_tones(Tone::Accent, Tone::AccentHover, canvas.palette(), effective);
                    let darkened = base.map(|channel| (channel * (1.0 - 0.12 * press)).clamp(0.0, 1.0));
                    if canvas.fill_rgb(bounds, style::RADIUS, darkened).is_err() {
                        canvas.fill(bounds, style::RADIUS, Tone::Accent);
                    }
                } else {
                    canvas.fill_between(bounds, style::RADIUS, Tone::Accent, Tone::AccentHover, effective);
                }
                Tone::OnAccent
            }
            (Self::Secondary, true) => {
                if press > 0.0 {
                    let hovered = mix_tones(Tone::Surface, Tone::Hover, canvas.palette(), hover.max(0.5));
                    let edge = mix_tones(Tone::Hover, Tone::Border, canvas.palette(), 0.28);
                    let pressed = std::array::from_fn(|i| hovered[i] + (edge[i] - hovered[i]) * press);
                    if canvas.fill_rgb(bounds, style::RADIUS, pressed).is_err() {
                        canvas.fill(bounds, style::RADIUS, Tone::Hover);
                    }
                } else {
                    canvas.fill_between(bounds, style::RADIUS, Tone::Surface, Tone::Hover, hover);
                }
                Tone::Ink
            }
            (Self::Selected, true) => {
                if press > 0.0 {
                    canvas.fill_between(bounds, style::RADIUS, Tone::Selection, Tone::Accent, 0.10 * press);
                } else {
                    canvas.fill(bounds, style::RADIUS, Tone::Selection);
                }
                Tone::AccentInk
            }
            (_, false) => {
                canvas.fill(bounds, style::RADIUS, Tone::DisabledSurface);
                Tone::Disabled
            }
        };
        if primary && state.enabled {
            if state.focused {
                let inset = style::PRIMARY_FOCUS_INSET;
                canvas.stroke(
                    bounds.inset(inset, inset),
                    style::RADIUS - inset,
                    style::FOCUS_STROKE,
                    Tone::OnAccent,
                );
            } else {
                canvas.elevation_border(
                    bounds,
                    style::RADIUS,
                    style::ElevationKind::Accent,
                    hover,
                    press,
                );
            }
        } else if !state.enabled {
            canvas.stroke(bounds, style::RADIUS, 1.0, Tone::DisabledBorder);
        } else if state.focused {
            canvas.stroke(bounds, style::RADIUS, style::FOCUS_STROKE, Tone::Accent);
        } else if selected {
            canvas.stroke(bounds, style::RADIUS, 1.0, Tone::Accent);
        } else {
            canvas.elevation_border(
                bounds,
                style::RADIUS,
                style::ElevationKind::Control,
                hover,
                press,
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
        if !state.enabled {
            canvas.stroke(bounds, style::RADIUS, 1.0, Tone::DisabledBorder);
        } else if state.focused {
            canvas.stroke(bounds, style::RADIUS, style::FOCUS_STROKE, Tone::Accent);
        } else {
            canvas.elevation_border(
                bounds,
                style::RADIUS,
                style::ElevationKind::TextField,
                f32::from(u8::from(state.active)),
                0.0,
            );
        }
    }
}

pub struct ComboBox;

impl ComboBox {
    pub fn paint(canvas: &mut impl Canvas, bounds: Bounds, label: &str, state: State, open: bool) {
        Self::paint_animated(
            canvas,
            bounds,
            label,
            state,
            open,
            f32::from(u8::from(state.active)),
            0.0,
        );
    }

    pub fn paint_animated(
        canvas: &mut impl Canvas,
        bounds: Bounds,
        label: &str,
        state: State,
        open: bool,
        hover: f32,
        press: f32,
    ) {
        let expanded = open && state.enabled;
        let hover = hover.clamp(0.0, 1.0);
        let press = press.clamp(0.0, 1.0);
        if !state.enabled {
            canvas.fill(bounds, style::RADIUS, Tone::DisabledSurface);
            canvas.stroke(bounds, style::RADIUS, 1.0, Tone::DisabledBorder);
        } else {
            canvas.fill(
                bounds,
                style::RADIUS,
                if expanded {
                    Tone::Selection
                } else {
                    state.surface()
                },
            );
            if state.focused || open {
                canvas.stroke(bounds, style::RADIUS, style::FOCUS_STROKE, Tone::Accent);
            } else {
                canvas.elevation_border(
                    bounds,
                    style::RADIUS,
                    style::ElevationKind::Control,
                    hover,
                    press,
                );
            }
        }
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
        let nudge = if state.enabled && !open { 1.0 * hover + 0.75 * press } else { 0.0 };
        canvas.text(
            Bounds::new(
                bounds.x + bounds.width - inset - icon_width,
                bounds.y + nudge,
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
            } else if state.enabled {
                Tone::Muted
            } else {
                Tone::Disabled
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
    pub const HEIGHT: f32 = 36.0;
    pub const LABEL_INSET: f32 = 48.0;
    pub const END_INSET: f32 = 12.0;
    pub const SIDEBAR_ICON_INSET: f32 = 12.0;
    pub const SIDEBAR_LABEL_INSET: f32 = 40.0;

    pub fn paint(self, canvas: &mut impl Canvas, bounds: Bounds, label: &str, state: State) {
        self.paint_insets(canvas, bounds, label, state, [18.0, Self::LABEL_INSET], true);
    }

    pub fn paint_sidebar(self, canvas: &mut impl Canvas, bounds: Bounds, label: &str, state: State) {
        self.paint_insets(canvas, bounds, label, state, [Self::SIDEBAR_ICON_INSET, Self::SIDEBAR_LABEL_INSET], true);
    }

    pub fn paint_sidebar_with_indicator(
        self,
        canvas: &mut impl Canvas,
        bounds: Bounds,
        label: &str,
        state: State,
        draw_indicator: bool,
    ) {
        self.paint_insets(
            canvas,
            bounds,
            label,
            state,
            [Self::SIDEBAR_ICON_INSET, Self::SIDEBAR_LABEL_INSET],
            draw_indicator,
        );
    }

    fn paint_insets(
        self,
        canvas: &mut impl Canvas,
        bounds: Bounds,
        label: &str,
        state: State,
        [icon_inset, label_inset]: [f32; 2],
        draw_indicator: bool,
    ) {
        let selected = state.enabled && self.selected;
        if selected || (state.enabled && state.active) {
            canvas.fill(bounds, style::RADIUS, if selected { Tone::Selection } else { Tone::Hover });
        }
        if selected && draw_indicator {
            canvas.fill(
                Bounds::new(bounds.x + 3.0, bounds.y + (bounds.height - 16.0) / 2.0, 3.0, 16.0),
                1.5,
                Tone::Accent,
            );
        }
        state.focus(canvas, bounds);
        let tone = if selected { Tone::AccentInk } else { state.text() };
        canvas.text(
            Bounds::new(bounds.x + icon_inset, bounds.y, 20.0, bounds.height),
            self.icon,
            Alignment::ActionIcon,
            tone,
        );
        canvas.text(
            Bounds::new(
                bounds.x + label_inset,
                bounds.y,
                (bounds.width - label_inset - Self::END_INSET).max(1.0),
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
    Icon { glyph: &'a str, selected: bool },
    Disclosure { expanded: bool, tinted: bool },
    SectionDisclosure { expanded: bool },
    Link(&'a str),
}

impl InlineControl<'_> {
    const COMPACT_INSET: f32 = 4.0;
    pub const DISCLOSURE_LABEL_INSET: f32 =
        Self::COMPACT_INSET + style::ICON_SLOT + style::ICON_GAP;
    pub const SECTION_DISCLOSURE_PADDING: f32 = style::BUTTON_GAP;

    pub fn paint<C: Canvas>(self, canvas: &mut C, bounds: Bounds, label: &str, state: State) {
        let selected = state.enabled
            && matches!(
                self,
                Self::Icon { selected: true, .. }
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
        if matches!(self, Self::SectionDisclosure { .. }) {
            if state.enabled && state.focused {
                canvas.stroke(bounds, style::RADIUS, 1.0, Tone::Accent);
            }
        } else {
            state.focus(canvas, bounds);
        }
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
            Self::SectionDisclosure { expanded } => leading(
                canvas,
                if expanded { style::CHEVRON_DOWN } else { style::CHEVRON_RIGHT },
                Self::COMPACT_INSET + Self::SECTION_DISCLOSURE_PADDING,
                state.text(),
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
        Self::paint_animated(
            canvas,
            bounds,
            label,
            state,
            checked,
            f32::from(u8::from(state.active)),
            0.0,
        );
    }

    pub fn paint_animated(
        canvas: &mut impl Canvas,
        bounds: Bounds,
        label: &str,
        state: State,
        checked: bool,
        hover: f32,
        press: f32,
    ) {
        let hover = hover.clamp(0.0, 1.0);
        let press = press.clamp(0.0, 1.0);
        if state.enabled && state.active {
            canvas.fill(bounds, style::RADIUS, Tone::Hover);
        }
        state.focus(canvas, bounds);
        let shrink = if state.enabled { 0.8 * press } else { 0.0 };
        let mark = Bounds::new(
            bounds.x + 4.0 + shrink,
            bounds.y + (bounds.height - 18.0) / 2.0 + shrink,
            18.0 - 2.0 * shrink,
            18.0 - 2.0 * shrink,
        );
        if checked {
            if state.enabled {
                canvas.fill_between(mark, 4.0, Tone::Accent, Tone::AccentHover, hover * (1.0 - 0.6 * press));
            } else {
                canvas.fill(mark, 4.0, Tone::Disabled);
            }
            canvas.text(mark, style::CHECK_MARK, Alignment::Icon, Tone::OnAccent);
        } else {
            canvas.fill(mark, 4.0, state.surface());
            if state.enabled {
                let mix = mix_tones(
                    Tone::Border,
                    Tone::Muted,
                    canvas.palette(),
                    (0.55 + 0.35 * hover.max(press)).clamp(0.0, 1.0),
                );
                if canvas.stroke_rgb(mark, 4.0, 1.0, mix).is_err() {
                    canvas.stroke_between(
                        mark,
                        4.0,
                        1.0,
                        Tone::Border,
                        Tone::HoverBorder,
                        hover.max(press),
                    );
                }
            } else {
                canvas.stroke(mark, 4.0, 1.0, Tone::DisabledBorder);
            }
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

    pub fn thumb_with_feedback(bounds: Bounds, progress: f32, hover: f32, press: f32) -> Bounds {
        let progress = progress.clamp(0.0, 1.0);
        let hover = hover.clamp(0.0, 1.0);
        let press = press.clamp(0.0, 1.0);
        let base_radius = (bounds.height / 2.0 - 4.0).max(1.0);
        let radius = (base_radius + 0.8 * hover.max(press)).min(bounds.height / 2.0 - 2.5);
        let stretch = 4.0 * press;
        let width = (radius * 2.0 + stretch).min(bounds.width - 6.0);
        let travel = (bounds.width - bounds.height).max(0.0);
        let center_x = bounds.x + bounds.height / 2.0 + travel * progress;
        // Anchor the elongation toward the thumb's active side so pressing feels elastic.
        let shift = (0.5 - progress) * stretch;
        let x = (center_x - width / 2.0 + shift)
            .clamp(bounds.x + 3.0, bounds.x + bounds.width - 3.0 - width);
        Bounds::new(
            x,
            bounds.y + bounds.height / 2.0 - radius,
            width,
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
        self.paint_animated(
            canvas,
            bounds,
            state,
            dark,
            palette,
            f32::from(u8::from(state.active)),
            0.0,
        )
    }

    pub fn paint_animated<C: Canvas>(
        self,
        canvas: &mut C,
        bounds: Bounds,
        state: State,
        _dark: bool,
        palette: &Palette,
        hover: f32,
        press: f32,
    ) -> Result<(), C::Error> {
        let progress = self.progress.clamp(0.0, 1.0);
        let hover = hover.clamp(0.0, 1.0);
        let press = press.clamp(0.0, 1.0);
        let radius = bounds.height / 2.0;
        let thumb = if state.enabled {
            Self::thumb_with_feedback(bounds, progress, hover, press)
        } else {
            Self::thumb(bounds, progress)
        };
        let thumb_radius = thumb.height / 2.0;
        let (thickness, tone) = state.border();
        if state.enabled {
            let mix_rgb = |a: [f32; 3], b: [f32; 3], t: f32| {
                std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
            };
            let off_fill = mix_rgb(
                Palette::unpack_rgb(palette.page_background),
                Palette::unpack_rgb(palette.button_hover),
                hover.max(press),
            );
            let on_fill = mix_rgb(
                Palette::unpack_rgb(palette.accent),
                Palette::unpack_rgb(palette.primary_hover),
                (hover * (1.0 - 0.6 * press)).clamp(0.0, 1.0),
            );
            let fill = mix_rgb(off_fill, on_fill, progress);
            canvas.fill_rgb(bounds, radius, fill)?;
            if state.focused {
                canvas.stroke(bounds, radius, style::FOCUS_STROKE, Tone::Accent);
            } else {
                let off_border = mix_rgb(
                    Palette::unpack_rgb(palette.toggle_off_border),
                    Palette::unpack_rgb(palette.toggle_off_border_hover),
                    hover.max(press),
                );
                let border = mix_rgb(off_border, on_fill, progress);
                if canvas.stroke_rgb(bounds, radius, thickness, border).is_err() {
                    canvas.stroke(
                        bounds,
                        radius,
                        thickness,
                        if self.selected { Tone::Accent } else { tone },
                    );
                }
            }
            let thumb_off = Palette::unpack_rgb(palette.toggle_off_thumb);
            let thumb_on = Palette::unpack_rgb(palette.on_accent);
            canvas.fill_rgb(thumb, thumb_radius, mix_rgb(thumb_off, thumb_on, progress))?;
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
        canvas.fill(bounds, bounds.width / 2.0, Tone::ScrollThumb);
    }

    pub fn thumb_growth(hover: f32) -> f32 {
        3.0 * hover
    }
}

/// Fluent 2 recessed track and elevated gliding pill for segmented choices.
pub struct SegmentedTrack;

impl SegmentedTrack {
    pub fn paint_track(canvas: &mut impl Canvas, bounds: Bounds) {
        let dark = canvas.palette().dark;
        let fill = if dark {
            [0.0, 0.0, 0.0, 0.28]
        } else {
            [0.0, 0.0, 0.0, 0.052]
        };
        let border = canvas.palette().neutral(if dark { 0.072 } else { 0.065 });
        let radius = style::RADIUS + 2.0;
        canvas.fill_rgba(bounds, radius, fill);
        canvas.stroke_rgba(bounds, radius, 1.0, border);
    }

    pub fn paint_divider(canvas: &mut impl Canvas, x: f32, center_y: f32) {
        let dark = canvas.palette().dark;
        let color = canvas.palette().neutral(if dark { 0.072 } else { 0.065 });
        canvas.fill_rgba(Bounds::new(x, center_y - 7.0, 1.0, 14.0), 0.0, color);
    }

    pub fn paint_pill(canvas: &mut impl Canvas, bounds: Bounds, hover: f32, press: f32) {
        let fill = Palette::unpack_rgb(canvas.palette().segment_active);
        let _ = canvas.fill_rgb(bounds, style::RADIUS, fill);
        canvas.elevation_border(
            bounds,
            style::RADIUS,
            style::ElevationKind::Control,
            hover,
            press,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_item(
        canvas: &mut impl Canvas,
        bounds: Bounds,
        label: &str,
        state: State,
        selected: bool,
        selection: f32,
        hover: f32,
        press: f32,
        paint_active_pill: bool,
    ) {
        let hover = hover.clamp(0.0, 1.0);
        let press = press.clamp(0.0, 1.0);
        let selection = selection.clamp(0.0, 1.0);
        let dark = canvas.palette().dark;
        let wash_alpha = (if dark { 0.055 } else { 0.036 }) * hover * (1.0 - selection)
            + (if dark { 0.04 } else { 0.03 }) * if selected { 0.0 } else { press };
        if state.enabled && wash_alpha > 0.002 {
            let wash = canvas.palette().neutral(wash_alpha);
            canvas.fill_rgba(bounds, style::RADIUS, wash);
        }
        if paint_active_pill && selection > 0.0 {
            Self::paint_pill(canvas, bounds, hover, press);
        }
        if state.focused {
            canvas.stroke(bounds, style::RADIUS, style::FOCUS_STROKE, Tone::Accent);
        }
        let alignment = if selected && state.enabled {
            Alignment::SelectedCenter
        } else {
            Alignment::Center
        };
        canvas.text(
            Bounds::new(
                bounds.x + 6.0,
                bounds.y,
                (bounds.width - 12.0).max(1.0),
                bounds.height,
            ),
            label,
            alignment,
            state.text(),
        );
    }
}

/// Fluent 2 slider with optional RGB channel rail tint and radial hover/press halo.
#[derive(Clone, Copy)]
pub struct SliderControl {
    pub value: f32,
    pub max: f32,
    pub centered: bool,
    pub channel: Option<u8>,
    pub inset: f32,
}

impl SliderControl {
    pub fn paint(
        self,
        canvas: &mut impl Canvas,
        bounds: Bounds,
        state: State,
        thumb: motion::SliderThumb,
        hover: f32,
        press: f32,
    ) {
        let palette = canvas.palette();
        let dark = palette.dark;
        let track_color = palette.neutral(if dark { 0.20 } else { 0.16 });
        let thumb_border = palette.neutral(if dark { 0.24 } else { 0.20 });
        let thumb_fill = Palette::rgba(palette.slider_thumb, 1.0);
        let ink_rgb = match self.channel {
            Some(0) => palette.danger,
            Some(1) => palette.success,
            Some(_) => palette.info,
            None => palette.accent,
        };
        let muted_rgba = Palette::rgba(palette.muted, 1.0);
        let cy = bounds.y + bounds.height * 0.5;
        let left = bounds.x + self.inset;
        let right = bounds.x + bounds.width - self.inset;
        let fraction = if self.max > 0.0 {
            (self.value / self.max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let cx = left + (right - left) * fraction;
        let rail = Bounds::new(left, cy - 2.0, (right - left).max(0.0), 4.0);
        canvas.fill_rgba(rail, 2.0, track_color);
        if self.channel.is_some() {
            canvas.fill_rgba(rail, 2.0, Palette::rgba(ink_rgb, if dark { 0.22 } else { 0.16 }));
        }
        let (fill_start, fill_width) = if self.centered {
            let middle = (left + right) * 0.5;
            canvas.fill_rgba(Bounds::new(middle - 0.5, cy - 5.0, 1.0, 10.0), 0.0, muted_rgba);
            (cx.min(middle), (cx - middle).abs())
        } else {
            (left, (cx - left).max(0.0))
        };
        canvas.fill_rgba(
            Bounds::new(fill_start, cy - 2.0, fill_width, 4.0),
            2.0,
            Palette::rgba(ink_rgb, 1.0),
        );
        let halo_alpha = (0.12 * hover.clamp(0.0, 1.0) + 0.10 * press.clamp(0.0, 1.0)).clamp(0.0, 0.24);
        if state.enabled && halo_alpha > 0.01 {
            canvas.fill_circle(cx, cy, thumb.outer + 3.5, Palette::rgba(ink_rgb, halo_alpha));
        }
        canvas.fill_circle(cx, cy, thumb.outer, thumb_fill);
        canvas.stroke_circle(cx, cy, thumb.outer, 1.0, thumb_border);
        canvas.fill_circle(cx, cy, thumb.inner, Palette::rgba(ink_rgb, 1.0));
        if state.focused {
            canvas.stroke(bounds, style::RADIUS, style::FOCUS_STROKE, Tone::Accent);
        }
    }
}

/// Compact capsule with leading status dot (success or danger).
pub struct StatusPill;

impl StatusPill {
    pub fn paint(canvas: &mut impl Canvas, bounds: Bounds, good: bool) {
        let palette = canvas.palette();
        let (rgb, alpha) = if good {
            (palette.success, if palette.dark { 0.14 } else { 0.09 })
        } else {
            (palette.danger, if palette.dark { 0.15 } else { 0.09 })
        };
        let radius = bounds.height * 0.5;
        canvas.fill_rgba(bounds, radius, Palette::rgba(rgb, alpha));
        canvas.fill_circle(
            bounds.x + 11.0,
            bounds.y + bounds.height * 0.5,
            3.0,
            Palette::rgba(rgb, 1.0),
        );
    }
}

/// Informational capsule badge with leading info glyph.
pub struct TitleBadge;

impl TitleBadge {
    pub fn paint(canvas: &mut impl Canvas, bounds: Bounds, label: &str) {
        let palette = canvas.palette();
        let radius = bounds.height * 0.5;
        let fill = Palette::rgba(palette.accent, if palette.dark { 0.13 } else { 0.085 });
        let border = Palette::rgba(palette.accent, if palette.dark { 0.24 } else { 0.18 });
        canvas.fill_rgba(bounds, radius, fill);
        canvas.stroke_rgba(bounds, radius, 1.0, border);
        canvas.text(
            Bounds::new(bounds.x + 7.0, bounds.y, style::ICON_SLOT, bounds.height),
            style::INFO,
            Alignment::Icon,
            Tone::Info,
        );
        canvas.text(
            Bounds::new(
                bounds.x + 25.0,
                bounds.y,
                (bounds.width - 32.0).max(1.0),
                bounds.height,
            ),
            label,
            Alignment::Badge,
            Tone::Info,
        );
    }
}

/// Elevated subpage back button with left-nudging arrow micro-motion.
pub struct BackButton;

impl BackButton {
    pub fn paint(
        canvas: &mut impl Canvas,
        bounds: Bounds,
        label: &str,
        icon_left: f32,
        state: State,
        hover: f32,
        press: f32,
    ) {
        let hover = hover.clamp(0.0, 1.0);
        let press = press.clamp(0.0, 1.0);
        if state.enabled {
            canvas.fill_between(bounds, style::RADIUS, Tone::Surface, Tone::Hover, hover);
            if state.focused {
                canvas.stroke(bounds, style::RADIUS, style::FOCUS_STROKE, Tone::Accent);
            } else {
                canvas.elevation_border(
                    bounds,
                    style::RADIUS,
                    style::ElevationKind::Control,
                    hover,
                    press,
                );
            }
        } else {
            canvas.fill(bounds, style::RADIUS, Tone::DisabledSurface);
            canvas.stroke(bounds, style::RADIUS, 1.0, Tone::DisabledBorder);
        }
        let nudge = if state.enabled {
            -2.0 * hover - 1.0 * press
        } else {
            0.0
        };
        canvas.icon_transformed(
            Bounds::new(icon_left, bounds.y, style::ICON_SLOT, bounds.height),
            style::BACK_ARROW,
            state.text(),
            nudge,
            0.0,
            0.0,
        );
        let text_left = icon_left + style::ICON_SLOT + style::ICON_GAP;
        canvas.text(
            Bounds::new(
                text_left,
                bounds.y,
                (bounds.x + bounds.width - text_left).max(1.0),
                bounds.height,
            ),
            label,
            Alignment::Leading,
            state.text(),
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkIcon {
    Reset,
    Forward,
    External,
}

/// Secondary card-footer link with hover micro-motion tailored to its action icon.
pub struct FooterLink;

impl FooterLink {
    pub fn paint(
        canvas: &mut impl Canvas,
        bounds: Bounds,
        label: &str,
        icon: LinkIcon,
        state: State,
        hover: f32,
    ) {
        let hover = hover.clamp(0.0, 1.0);
        if state.enabled && hover > 0.0 {
            let dark = canvas.palette().dark;
            let wash = canvas.palette().neutral((if dark { 0.06 } else { 0.04 }) * hover);
            canvas.fill_rgba(bounds, style::RADIUS, wash);
        }
        if state.focused {
            canvas.stroke(bounds, style::RADIUS, style::FOCUS_STROKE, Tone::Accent);
        }
        let tone = if state.enabled {
            Tone::AccentInk
        } else {
            Tone::Disabled
        };
        canvas.text(
            Bounds::new(
                bounds.x + 8.0,
                bounds.y,
                (bounds.width - 40.0).max(1.0),
                bounds.height,
            ),
            label,
            Alignment::Supporting,
            tone,
        );
        let slot = Bounds::new(
            bounds.x + bounds.width - 24.0,
            bounds.y,
            style::ICON_SLOT,
            bounds.height,
        );
        let motion = if state.enabled { hover } else { 0.0 };
        match icon {
            LinkIcon::Reset => {
                canvas.icon_transformed(slot, style::RESET_ICON, tone, 0.0, 0.0, -0.48 * motion);
            }
            LinkIcon::Forward => {
                canvas.icon_transformed(slot, style::CHEVRON_RIGHT, tone, 2.5 * motion, 0.0, 0.0);
            }
            LinkIcon::External => {
                canvas.icon_transformed(
                    slot,
                    style::EXTERNAL_LINK,
                    tone,
                    1.5 * motion,
                    -1.0 * motion,
                    0.0,
                );
            }
        }
    }
}

/// Fluent 2 search input chrome: elevated text field + search icon + optional clear glyph.
pub struct SearchField;

impl SearchField {
    pub fn paint(
        canvas: &mut impl Canvas,
        bounds: Bounds,
        placeholder: &str,
        has_clear: bool,
        state: State,
    ) {
        TextField::paint(canvas, bounds, state);
        if !placeholder.is_empty() {
            canvas.text(
                Bounds::new(
                    bounds.x + 40.0,
                    bounds.y,
                    (bounds.width - 80.0).max(1.0),
                    bounds.height,
                ),
                placeholder,
                Alignment::Leading,
                Tone::Muted,
            );
        }
        canvas.text(
            Bounds::new(bounds.x + 16.0, bounds.y, style::ICON_SLOT, bounds.height),
            style::SEARCH_ICON,
            Alignment::Icon,
            Tone::Muted,
        );
        if has_clear {
            canvas.text(
                Bounds::new(
                    bounds.x + bounds.width - 32.0,
                    bounds.y,
                    style::ICON_SLOT,
                    bounds.height,
                ),
                style::CLEAR_ICON,
                Alignment::Icon,
                Tone::Muted,
            );
        }
    }
}

/// Compact confirmation/status banner pill with leading checkmark.
pub struct InfoBanner;

impl InfoBanner {
    pub fn paint(canvas: &mut impl Canvas, bounds: Bounds, label: &str) {
        canvas.fill(bounds, style::RADIUS, Tone::Selection);
        let icon = Bounds::new(bounds.x + 12.0, bounds.y, style::ICON_SLOT, bounds.height);
        canvas.text(icon, style::CHECK_MARK, Alignment::Icon, Tone::AccentInk);
        let text_left = icon.x + icon.width + style::ICON_GAP;
        canvas.text(
            Bounds::new(
                text_left,
                bounds.y,
                (bounds.x + bounds.width - text_left - 12.0).max(1.0),
                bounds.height,
            ),
            label,
            Alignment::Leading,
            Tone::Ink,
        );
    }
}

/// Actionable navigation/drill-down row (`Open >`) with subtle hover fill and chevron nudge.
pub struct ForwardRow;

impl ForwardRow {
    pub fn paint(
        canvas: &mut impl Canvas,
        bounds: Bounds,
        label: &str,
        state: State,
        hover: f32,
    ) {
        let hover = if state.enabled { hover.clamp(0.0, 1.0) } else { 0.0 };
        if hover > 0.001 {
            let wash = canvas.palette().subtle_hover(hover);
            canvas.fill_rgba(bounds, style::RADIUS, wash);
        }
        if state.focused {
            canvas.stroke(bounds, style::RADIUS, style::FOCUS_STROKE, Tone::Accent);
        }
        let icon_x = bounds.x + bounds.width - 22.0;
        let icon_tone = if state.enabled {
            Tone::Muted
        } else {
            Tone::Disabled
        };
        canvas.icon_transformed(
            Bounds::new(icon_x, bounds.y, style::ICON_SLOT, bounds.height),
            style::CHEVRON_RIGHT,
            icon_tone,
            2.5 * hover,
            0.0,
            0.0,
        );
        let text_left = bounds.x + style::ROW_INSET;
        let text_right = (icon_x - 6.0).max(text_left + 1.0);
        canvas.text(
            Bounds::new(text_left, bounds.y, text_right - text_left, bounds.height),
            label,
            Alignment::Leading,
            state.text(),
        );
    }
}

/// Selectable Fluent 2 list row (e.g., font/option picker) with optional accent bar, checkmark, and tag pill.
pub struct ListRow;

impl ListRow {
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        canvas: &mut impl Canvas,
        bounds: Bounds,
        label: &str,
        show_selection_indicator: bool,
        tag: Option<(&str, f32)>,
        state: State,
        select_amount: f32,
        hover_amount: f32,
    ) {
        let hover = if state.enabled {
            hover_amount.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let select = select_amount.clamp(0.0, 1.0);
        let hover_only = hover * (1.0 - select);
        let select_only = select * (1.0 - hover);
        let both = select * hover;
        let palette = canvas.palette();
        let combined_alpha = hover_only * (if palette.dark { 0.05 } else { 0.032 })
            + select_only * (if palette.dark { 0.07 } else { 0.045 })
            + both * (if palette.dark { 0.09 } else { 0.060 });
        if combined_alpha > 0.001 {
            let wash = palette.neutral(combined_alpha);
            canvas.fill_rgba(bounds, style::RADIUS, wash);
        }
        if state.focused {
            canvas.stroke(bounds, style::RADIUS, style::FOCUS_STROKE, Tone::Accent);
        }
        let text_left = bounds.x + style::ROW_INSET;
        let mut text_right = bounds.x + bounds.width;
        if show_selection_indicator {
            text_right -= 32.0;
            if select > 0.01 {
                let bar_h = 6.0 + 10.0 * select;
                let bar = Bounds::new(
                    bounds.x + 3.0,
                    bounds.y + (bounds.height - bar_h) * 0.5,
                    3.0,
                    bar_h,
                );
                let accent = Palette::rgba(canvas.palette().accent, select);
                canvas.fill_rgba(bar, 1.5, accent);
                canvas.text(
                    Bounds::new(
                        bounds.x + bounds.width - 28.0,
                        bounds.y,
                        style::ICON_SLOT,
                        bounds.height,
                    ),
                    style::CHECK_MARK,
                    Alignment::Icon,
                    Tone::Accent,
                );
            }
        }
        if let Some((tag_label, tag_text_width)) = tag {
            let tag_w = tag_text_width + 14.0;
            let tag_bounds = Bounds::new(
                text_right - tag_w,
                bounds.y + (bounds.height - 20.0) * 0.5,
                tag_w,
                20.0,
            );
            let fill = canvas.palette().specimen_fill();
            let edge = canvas.palette().divider();
            canvas.fill_rgba(tag_bounds, 10.0, fill);
            canvas.stroke_rgba(tag_bounds, 10.0, 1.0, edge);
            canvas.text(
                Bounds::new(
                    tag_bounds.x + 7.0,
                    tag_bounds.y,
                    tag_text_width.max(1.0),
                    tag_bounds.height,
                ),
                tag_label,
                Alignment::Supporting,
                Tone::Muted,
            );
            text_right = tag_bounds.x - 8.0;
        }
        canvas.text(
            Bounds::new(
                text_left,
                bounds.y,
                (text_right - text_left).max(1.0),
                bounds.height,
            ),
            label,
            Alignment::Leading,
            state.text(),
        );
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
        fn palette(&self) -> &Palette {
            static PALETTE: Palette = Palette::for_theme(false);
            &PALETTE
        }
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

        let grip = Bounds::new(bounds.x, bounds.y, style::HEIGHT, style::HEIGHT);
        for (hover, selected, surface, ink) in [
            (false, false, None, Tone::Ink),
            (true, false, Some(Tone::Hover), Tone::Ink),
            (true, true, Some(Tone::Selection), Tone::AccentInk),
        ] {
            let mut canvas = RecordingCanvas::default();
            InlineControl::Icon { glyph: style::ARROW_UP, selected }.paint(
                &mut canvas,
                grip,
                "",
                State { active: hover, ..active },
            );
            assert_eq!(canvas.icons, [style::ARROW_UP]);
            assert_eq!(canvas.labels, [(grip, ink)]);
            assert_eq!(canvas.fills.first().copied(), surface.map(|tone| (grip, tone)));
            assert!(canvas.borders.is_empty());
        }

        let mut navigation = RecordingCanvas::default();
        NavigationItem { icon: "icon", selected: true }.paint(&mut navigation, bounds, "Rules", active);
        let marker = Bounds::new(bounds.x + 3.0, bounds.y + (bounds.height - 16.0) / 2.0, 3.0, 16.0);
        assert_eq!(navigation.fills, [(bounds, Tone::Selection), (marker, Tone::Accent)]);
        assert_eq!(navigation.icons, ["icon"]);
        assert_eq!(navigation.labels[1].0.x, bounds.x + NavigationItem::LABEL_INSET);
        assert!(navigation.labels.iter().all(|(_, tone)| *tone == Tone::AccentInk));

        let mut sidebar = RecordingCanvas::default();
        NavigationItem { icon: "icon", selected: true }.paint_sidebar(&mut sidebar, bounds, "Rules", active);
        assert_eq!(sidebar.fills, navigation.fills);
        assert_eq!(sidebar.labels[0].0.x, bounds.x + NavigationItem::SIDEBAR_ICON_INSET);
        assert_eq!(sidebar.labels[1].0.x, bounds.x + NavigationItem::SIDEBAR_LABEL_INSET);

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
    fn section_disclosure_uses_a_hover_surface_and_a_thin_focus_outline() {
        let bounds = Bounds::new(16.0, 24.0, 160.0, style::HEIGHT);
        let idle = State { enabled: true, active: false, focused: false };
        let mut expanded = RecordingCanvas::default();
        InlineControl::SectionDisclosure { expanded: true }.paint(&mut expanded, bounds, "Skipped icons", idle);
        assert!(expanded.fills.is_empty());
        assert!(expanded.borders.is_empty());
        assert_eq!(expanded.icons, [style::CHEVRON_DOWN]);
        assert_eq!(expanded.labels[1].0.x, bounds.x + InlineControl::DISCLOSURE_LABEL_INSET + InlineControl::SECTION_DISCLOSURE_PADDING);

        let mut hover = RecordingCanvas::default();
        InlineControl::SectionDisclosure { expanded: false }
            .paint(&mut hover, bounds, "Skipped icons", State { active: true, ..idle });
        assert_eq!(hover.fills, [(bounds, Tone::Hover)]);
        assert!(hover.borders.is_empty());
        assert_eq!(hover.icons, [style::CHEVRON_RIGHT]);

        let mut keyboard = RecordingCanvas::default();
        InlineControl::SectionDisclosure { expanded: true }
            .paint(&mut keyboard, bounds, "Skipped icons", State { focused: true, ..idle });
        assert!(keyboard.fills.is_empty());
        assert_eq!(keyboard.borders, [(1.0, Tone::Accent)]);
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

    #[test]
    fn toggle_thumb_feedback_expands_stretches_and_stays_inside_track() {
        let bounds = Bounds::new(10.0, 20.0, 40.0, 20.0);
        let rest = Toggle::thumb(bounds, 0.0);
        let hovered = Toggle::thumb_with_feedback(bounds, 0.0, 1.0, 0.0);
        let pressed = Toggle::thumb_with_feedback(bounds, 0.0, 1.0, 1.0);
        assert!(hovered.height > rest.height);
        assert!(pressed.width > pressed.height);
        for progress in [0.0, 0.5, 1.0] {
            for hover in [0.0, 0.5, 1.0] {
                for press in [0.0, 0.5, 1.0] {
                    let thumb = Toggle::thumb_with_feedback(bounds, progress, hover, press);
                    assert!(thumb.x >= bounds.x + 3.0 - 0.001);
                    assert!(thumb.x + thumb.width <= bounds.x + bounds.width - 3.0 + 0.001);
                    let center_y = thumb.y + thumb.height * 0.5;
                    assert!((center_y - (bounds.y + bounds.height * 0.5)).abs() < 0.001);
                }
            }
        }
    }

    #[test]
    fn shared_fluent2_controls_render_through_canvas_tokens() {
        let mut canvas = RecordingCanvas::default();
        let active = State {
            enabled: true,
            active: true,
            focused: false,
        };
        SegmentedTrack::paint_track(&mut canvas, Bounds::new(0.0, 0.0, 200.0, 32.0));
        SegmentedTrack::paint_pill(&mut canvas, Bounds::new(3.0, 3.0, 96.0, 26.0), 0.0, 0.0);
        SegmentedTrack::paint_item(
            &mut canvas,
            Bounds::new(3.0, 3.0, 96.0, 26.0),
            "Light",
            active,
            true,
            1.0,
            0.0,
            0.0,
            false,
        );
        TitleBadge::paint(&mut canvas, Bounds::new(0.0, 40.0, 120.0, 20.0), "New only");
        BackButton::paint(&mut canvas, Bounds::new(0.0, 70.0, 96.0, 32.0), "Back", 12.0, active, 1.0, 0.0);
        FooterLink::paint(&mut canvas, Bounds::new(0.0, 110.0, 200.0, 32.0), "Reset", LinkIcon::Reset, active, 1.0);
        SearchField::paint(&mut canvas, Bounds::new(0.0, 150.0, 220.0, 32.0), "Search fonts", true, active);
        InfoBanner::paint(&mut canvas, Bounds::new(0.0, 190.0, 200.0, 32.0), "Saved");
        ForwardRow::paint(&mut canvas, Bounds::new(0.0, 230.0, 96.0, 32.0), "Open", active, 1.0);
        ListRow::paint(&mut canvas, Bounds::new(0.0, 270.0, 220.0, 36.0), "Segoe UI", true, Some(("Default", 44.0)), active, 1.0, 0.5);
        assert!(canvas.icons.contains(&style::INFO.to_owned()));
        assert!(canvas.icons.contains(&style::BACK_ARROW.to_owned()));
        assert!(canvas.icons.contains(&style::RESET_ICON.to_owned()));
        assert!(canvas.icons.contains(&style::SEARCH_ICON.to_owned()));
        assert!(canvas.icons.contains(&style::CLEAR_ICON.to_owned()));
        assert!(canvas.icons.contains(&style::CHEVRON_RIGHT.to_owned()));
        assert!(canvas.icons.contains(&style::CHECK_MARK.to_owned()));

        let light = Palette::for_theme(false).elevation_gradient(style::ElevationKind::Control, 0.0, 0.0);
        let dark = Palette::for_theme(true).elevation_gradient(style::ElevationKind::Control, 0.0, 0.0);
        assert!(light.bottom[3] > light.top[3]);
        assert!(dark.top[3] > dark.bottom[3]);
    }
}
