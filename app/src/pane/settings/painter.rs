//! Shared rendering for the explicit settings control types.
use super::*;
use super::super::control_style as style;
use super::motions::{ControlMotion, MotionFrame};
use luciddesk_window::controls as widgets;
use widgets::{InlineControl, Tone};

const WINDOW_BRUSH_COUNT: usize = 13;
const WIDGET_BRUSH_COUNT: usize = Tone::ALL.len();

struct WidgetCanvas<'a, 'b> {
    pass: &'a canvas::DrawPass<'b>,
    painter: &'a Painter,
    brushes: [&'a windows_canvas::Brush; WIDGET_BRUSH_COUNT],
    leading_text: Option<(usize, f32)>,
    motion: ControlMotion,
    palette: &'a style::Palette,
    selection: f32,
    combo: bool,
}

impl WidgetCanvas<'_, '_> {
    fn frame(bounds: widgets::Bounds, radius: f32) -> RoundedRect {
        RoundedRect { rect: Rect::from_xywh(bounds.x, bounds.y, bounds.width, bounds.height), radius_x: radius, radius_y: radius }
    }

}

impl widgets::Canvas for WidgetCanvas<'_, '_> {
    type Error = windows::core::Error;

    fn palette(&self) -> &style::Palette { self.palette }

    fn fill(&mut self, bounds: widgets::Bounds, radius: f32, tone: widgets::Tone) {
        let hover = self.motion.hover.unwrap_or(1.0);
        let blend = if self.combo {
            self.motion.disclosure.map(|open| (Tone::Surface, Tone::Selection, open))
                .or_else(|| (tone == Tone::Hover && hover < 1.0).then_some((Tone::Surface, tone, hover)))
        } else if tone == Tone::Accent && self.motion.check.is_some() {
            self.motion.check.map(|check| (Tone::Surface, tone, check))
        } else {
            None
        };
        if let Some((from, to, progress)) = blend
            && self.fill_rgb(bounds, radius, widgets::mix_tones(from, to, self.palette, progress)).is_ok()
        {
            return;
        }
        let opacity = match tone {
            Tone::Hover => hover,
            Tone::Selection | Tone::Accent => self.selection,
            _ => 1.0,
        };
        let shape = Self::frame(bounds, radius);
        let _fade = self.pass.opacity_layer(&shape.rect, opacity);
        self.pass.fill_rounded_rect(&shape, self.brushes[tone as usize]);
    }

    fn fill_rgb(&mut self, bounds: widgets::Bounds, radius: f32, [r, g, b]: [f32; 3]) -> Result<(), Self::Error> {
        let brush = canvas_result(self.pass.create_solid_brush(ColorF { r, g, b, a: 1.0 }))?;
        self.pass.fill_rounded_rect(&Self::frame(bounds, radius), &brush);
        Ok(())
    }

    fn stroke(&mut self, bounds: widgets::Bounds, radius: f32, thickness: f32, tone: widgets::Tone) {
        self.pass.draw_rounded_rect(&Self::frame(bounds, radius), self.brushes[tone as usize], thickness);
    }

    fn text(&mut self, bounds: widgets::Bounds, label: &str, alignment: widgets::Alignment, tone: widgets::Tone) {
        let mut bounds = Rect::from_xywh(bounds.x, bounds.y, bounds.width, bounds.height);
        let brush = self.brushes[tone as usize];
        match alignment {
            widgets::Alignment::Center => self.pass.clipped_text(label, &self.painter.button_format, &bounds, brush),
            widgets::Alignment::Leading => {
                let format = if let Some((format, width)) = self.leading_text {
                    bounds.right = (bounds.right - width).max(bounds.left + 1.0);
                    &self.painter.formats[format]
                } else {
                    &self.painter.formats[Tokens::CONTROL_TEXT]
                };
                self.pass.clipped_text(label, format, &bounds, brush);
            }
            widgets::Alignment::Icon => {
                let format = &self.painter.formats[4];
                if let Some(progress) = self.motion.disclosure
                    && matches!(label, style::CHEVRON_RIGHT | style::CHEVRON_DOWN | style::CHEVRON_UP)
                {
                    let (glyph, length) = if self.combo {
                        (style::CHEVRON_DOWN, std::f32::consts::PI)
                    } else {
                        (style::CHEVRON_RIGHT, std::f32::consts::FRAC_PI_2)
                    };
                    self.pass.rotated_icon(glyph, format, &bounds, brush, progress * length);
                } else {
                    let opacity = if label == style::CHECK_MARK { self.motion.check.unwrap_or(1.0) } else { 1.0 };
                    let _fade = self.pass.opacity_layer(&bounds, opacity);
                    self.pass.clipped_icon(label, format, &bounds, brush);
                }
            }
            widgets::Alignment::ActionIcon => {
                let label = if label == fonts::DRAG_HANDLE_ICON {
                    let font = self.painter.drag_handle.get_or_init(|| fonts::DragHandleFont::new(20.0).ok());
                    if font.as_ref().is_some_and(|font| {
                        self.pass.clipped_native_icon(&font.layout, &bounds, brush).is_ok()
                    }) {
                        return;
                    }
                    fonts::DRAG_HANDLE_FALLBACK
                } else {
                    label
                };
                self.pass.clipped_icon(label, &self.painter.formats[5], &bounds, brush);
            }
            widgets::Alignment::Supporting => self.pass.clipped_text(label, &self.painter.formats[6], &bounds, brush),
        }
    }

}

struct CachedPreview {
    context: ID2D1DeviceContext,
    material: Backdrop,
    dark: bool,
    bitmap: windows_canvas::Bitmap,
}

pub(super) struct Painter {
    preview: RefCell<Option<CachedPreview>>,
    brushes: RefCell<super::super::canvas::Brushes<{ WINDOW_BRUSH_COUNT + WIDGET_BRUSH_COUNT }>>,
    icon_bitmap: RefCell<Option<(ID2D1DeviceContext, windows_canvas::Bitmap)>>,
    #[cfg(test)]
    icon_uploads: std::cell::Cell<usize>,
    formats: Vec<windows_canvas::TextFormat>,
    button_format: windows_canvas::TextFormat,
    new_group_format: windows_canvas::TextFormat,
    drag_handle: std::cell::OnceCell<Option<fonts::DragHandleFont>>,
}
impl Drop for Painter {
    fn drop(&mut self) { super::components::release_text_metrics(); }
}

impl Painter {
    pub(super) fn new() -> windows::core::Result<Self> {
        use windows_canvas::{FontWeight, ParagraphAlignment, TextFormat, WordWrapping};
        let family = super::super::fonts::family();
        let mut formats = vec![];
        for (i, size) in [
            12.0,
            14.0,
            20.0,
            28.0,
            Style::ICON,
            Style::NAV_ICON,
            12.0,
            14.0,
            Style::NAV_HEADING,
            Style::TEXT_SIZE,
            14.0,
            14.0,
            11.0,
            13.0,
            12.0,
            Tokens::SECTION_TITLE_SIZE,
            12.0,
            SectionHeader::TITLE_SIZE,
        ]
        .iter()
        .enumerate()
        {
            let format = canvas_result(TextFormat::with_weight(
                if matches!(i, 4 | 5 | Tokens::CAPTION_ICON) {
                    super::super::fonts::icon_family()
                } else {
                    &family
                },
                *size,
                match i {
                    Tokens::GROUP_TITLE => SectionHeader::TITLE_WEIGHT,
                    2 | 3 | Tokens::SECTION_TITLE => FontWeight(600),
                    _ => FontWeight::NORMAL,
                },
            ))?
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(if matches!(i, 1 | 6 | Tokens::STATUS_GOOD | Tokens::STATUS_ERROR | Tokens::TITLE_BADGE | Tokens::SECTION_TITLE | Tokens::GROUP_TITLE) {
                WordWrapping::Wrap
            } else {
                WordWrapping::NoWrap
            });
            let format = if matches!(i, 4 | 5 | Tokens::CAPTION_ICON) {
                format.with_alignment(windows_canvas::TextAlignment::Center)
            } else if i == Tokens::CONTROL_TEXT {
                format.with_alignment(windows_canvas::TextAlignment::Leading)
            } else if matches!(i, Tokens::VALUE_TEXT | Tokens::SECONDARY_VALUE) {
                format.with_alignment(windows_canvas::TextAlignment::Trailing)
            } else {
                format
            };
            super::super::canvas::ellipsis(&format)?;
            formats.push(format);
        }
        let control_format = |alignment, weight| -> windows::core::Result<TextFormat> {
            let format = canvas_result(TextFormat::with_weight(&family, Style::TEXT_SIZE, weight))?
                .with_paragraph_alignment(ParagraphAlignment::Center)
                .with_word_wrapping(WordWrapping::NoWrap)
                .with_alignment(alignment);
            super::super::canvas::ellipsis(&format)?;
            Ok(format)
        };
        let button_format = control_format(windows_canvas::TextAlignment::Center, FontWeight::NORMAL)?;
        let new_group_format = control_format(windows_canvas::TextAlignment::Leading, DestinationChange::NEW_GROUP_WEIGHT)?;
        Ok(Self {
            preview: RefCell::new(None),
            brushes: Default::default(),
            icon_bitmap: RefCell::new(None),
            #[cfg(test)]
            icon_uploads: std::cell::Cell::new(0),
            formats,
            button_format,
            new_group_format,
            drag_handle: std::cell::OnceCell::new(),
        })
    }
    pub(super) fn label_width(&self, text: &str) -> windows::core::Result<f32> {
        let layout = canvas_result(windows_canvas::TextLayout::new(
            text,
            &self.formats[1],
            4096.0,
            64.0,
        ))?;
        Ok(layout.metrics().width)
    }
    #[cfg(test)]
    pub(super) fn paint(
        &self, t: &ID2D1DeviceContext, s: &Scene, width: f32, height: f32,
        scale: f32, dark: bool, native: bool, hover: Option<usize>, focus: Option<usize>,
        toggles: &HashMap<usize, f32>,
    ) -> windows::core::Result<()> {
        self.paint_animated(t, s, width, height, scale, dark, native, hover, focus, toggles, &MotionFrame::default())
    }

    pub(super) fn paint_animated(
        &self,
        t: &ID2D1DeviceContext,
        s: &Scene,
        width: f32,
        height: f32,
        scale: f32,
        dark: bool,
        native: bool,
        hover: Option<usize>,
        focus: Option<usize>,
        toggles: &std::collections::HashMap<usize, f32>,
        motion: &MotionFrame,
    ) -> windows::core::Result<()> {
        {
            let context = t;
            if s.app_icon.is_none() && s.caption_icon.is_none() {
                self.icon_bitmap.borrow_mut().take();
            }
            if s.previews.is_empty() {
                self.preview.borrow_mut().take();
            }
            let mut brushes = self.brushes.borrow_mut();
            super::super::canvas::draw(t, scale, |t| {
                let color = |v: u32| ColorF {
                    r: ((v >> 16) & 255) as f32 / 255.0,
                    g: ((v >> 8) & 255) as f32 / 255.0,
                    b: (v & 255) as f32 / 255.0,
                    a: 1.0,
                };
                let palette = components::Palette::for_theme(dark);
                let bg = color(palette.background);
                let mut chrome = super::super::theme::material_chrome(s.material, dark);
                if !matches!(
                    s.material.base(),
                    Backdrop::Acrylic | Backdrop::Mica | Backdrop::MicaAlt
                ) {
                    chrome.card = ColorF {
                        a: if native {
                            if dark { 0.65 } else { 0.72 }
                        } else {
                            1.0
                        },
                        ..color(if dark { 0x2b2b2b } else { 0xffffff })
                    };
                    chrome.card_border = ColorF {
                        a: if native { 0.45 } else { 1.0 },
                        ..color(palette.border)
                    };
                }
                if !dark {
                    // Give settings a stable surface hierarchy while retaining native translucency.
                    chrome.card = ColorF {
                        a: if native { chrome.card.a.max(0.80) } else { 1.0 },
                        ..color(0xfafcfe)
                    };
                    chrome.card_border = ColorF {
                        a: if native { 0.80 } else { 1.0 },
                        ..color(0xd4dce5)
                    };
                }
                let [nav_selection, nav_hover, nav_selection_hover] =
                    components::navigation_colors(s.material, dark);
                let window_colors = [
                        chrome.card, chrome.card_border,
                        // Light sliders need opaque strokes on translucent cards.
                        ColorF { a: if dark && native { 0.45 } else { 1.0 }, ..color(if dark { 0x424242 } else { 0x8995a4 }) },
                        ColorF { a: if dark && native { 0.45 } else { 1.0 }, ..color(if dark { 0x424242 } else { 0xa3aebb }) },
                        if dark { chrome.card } else { color(0xffffff) },
                        nav_selection, nav_hover, nav_selection_hover,
                        ColorF { a: if native { if dark { 0.12 } else { 0.55 } } else { 1.0 },
                            ..color(palette.page_background) },
                        color(if dark { 0xef8d8d } else { 0xb83d42 }),
                        color(if dark { 0x8dccaa } else { 0x287b50 }),
                        color(if dark { 0x88baf0 } else { 0x266eae }),
                        ColorF { a: 0.45, ..color(palette.border) },
                ];
                let colors = std::array::from_fn(|index| {
                    if index < WINDOW_BRUSH_COUNT {
                        window_colors[index]
                    } else {
                        color(Tone::ALL[index - WINDOW_BRUSH_COUNT].rgb(&palette))
                    }
                });
                let brushes = canvas_result(brushes.get(context, &t, colors))?;
                let [card, card_border, slider_track, slider_border, slider_surface, nav_selected,
                    nav_hovered, nav_selected_hovered, page_background, red, green, blue, rule_divider] =
                    std::array::from_fn(|index| &brushes[index]);
                let widget_brushes: [_; WIDGET_BRUSH_COUNT] = std::array::from_fn(|index| &brushes[WINDOW_BRUSH_COUNT + index]);
                let brush = |tone: Tone| widget_brushes[tone as usize];
                let (ink, muted, disabled, border, accent, button, button_hover) = (
                    brush(Tone::Ink), brush(Tone::Muted), brush(Tone::Disabled), brush(Tone::Border),
                    brush(Tone::Accent), brush(Tone::Surface), brush(Tone::Hover),
                );
                let widget_canvas = WidgetCanvas {
                    pass: &t, painter: self, brushes: widget_brushes, leading_text: None,
                    motion: ControlMotion::default(), palette: &palette, selection: 1.0,
                    combo: false,
                };
                let selected = nav_selected;
                let hovered = nav_hovered;

                t.clear(if native {
                    ColorF {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    }
                } else {
                    bg
                });
                let page_left = s.content_left - Tokens::MARGIN;
                let page_top = s.content_top.min(height);
                t.fill_rounded_rect(
                    &RoundedRect {
                        rect: Rect::from_xywh(page_left, page_top, width - page_left, height - page_top),
                        radius_x: Tokens::CARD_RADIUS,
                        radius_y: Tokens::CARD_RADIUS,
                    },
                    page_background,
                );
                if s.footer_height > 0.0 {
                    t.fill_rect(
                        &Rect::from_xywh(s.content_left, height - s.footer_height, (width - s.content_left - Tokens::MARGIN).min(Tokens::MAX_WIDTH), 1.0),
                        if dark { border } else { card_border },
                    );
                }
                if s.controls.iter().any(|control| matches!(control.kind, ControlKind::Tab(_))) {
                    t.fill_rect(
                        &Rect::from_xywh(s.content_left, page_top - 1.0,
                            (width - s.content_left - Tokens::MARGIN).min(Tokens::MAX_WIDTH), 1.0),
                        card_border,
                    );
                }
                for r in &s.cards {
                    let _clip = ContentClip::new(&t, s, r.left >= s.content_left && !s.fixed_list());
                    let rr = RoundedRect {
                        rect: *r,
                        radius_x: Tokens::CARD_RADIUS,
                        radius_y: Tokens::CARD_RADIUS,
                    };
                    t.fill_rounded_rect(&rr, card);
                    t.draw_rounded_rect(&rr, card_border, 1.0);
                }
                let rule_dividers = s.rule_rows.first().zip(s.rule_rows.last())
                    .map(|(first, last)| first.separators.start..last.separators.end);
                for (index, r) in s.separators.iter().enumerate() {
                    let _clip = ContentClip::new(&t, s, r.left >= s.content_left && !s.fixed_list());
                    let _disclosure = s.disclosure_separator_clip(index).map(|clip| ContentClip::rect(&t, clip));
                    let line = if rule_dividers.as_ref().is_some_and(|range| range.contains(&index)) {
                        rule_divider
                    } else if dark {
                        border
                    } else {
                        card_border
                    };
                    t.fill_rect(r, line);
                }
                for header in &s.section_headers {
                    let _clip = ContentClip::new(&t, s, true);
                    let [icon, title, summary] = header.cells();
                    let accent_ink = brush(Tone::AccentInk);
                    t.clipped_icon(header.icon, &self.formats[4], &icon, accent_ink);
                    t.clipped_text(&header.title, &self.formats[Tokens::GROUP_TITLE], &title, accent_ink);
                    t.clipped_text(&header.summary, &self.formats[Tokens::SECONDARY_VALUE], &summary, muted);
                }
                for change in &s.destination_changes {
                    let _clip = ContentClip::new(&t, s, true);
                    let [_, arrow, _] = change.cells();
                    for ((bounds, destination), (format, color)) in change.destinations().into_iter()
                        .zip([(&self.formats[Tokens::CONTROL_TEXT], ink), (&self.new_group_format, brush(Tone::AccentInk))])
                    {
                        let [group, rule] = destination.cells(bounds);
                        t.clipped_text(&destination.group, format, &group, color);
                        t.clipped_text(&destination.rule, &self.formats[0], &rule, muted);
                    }
                    t.clipped_icon(style::CHEVRON_RIGHT, &self.formats[4], &arrow, muted);
                }
                for (bounds, content) in s.app_icon.iter().map(|r| (r, true))
                    .chain(s.caption_icon.iter().map(|r| (r, false))) {
                    let _clip = ContentClip::new(&t, s, content);
                    let mut cached = self.icon_bitmap.borrow_mut();
                    if cached.as_ref().is_none_or(|(old, _)| old != context) {
                        *cached = None;
                        // Share the caption/About texture; the GPU owns
                        // its copy after upload, so no CPU buffer is retained.
                        let icon = crate::app_icon::load(256, 256).map_err(|message| {
                            windows::core::Error::new(windows::Win32::Foundation::E_FAIL, message)
                        })?;
                        let pixels = super::super::assets::icon_pixels(
                            windows::Win32::UI::WindowsAndMessaging::HICON(icon.0),
                        )?;
                        *cached = Some((context.clone(), canvas_result(t.create_bitmap(
                            &pixels.data, pixels.width, pixels.height,
                        ))?));
                        #[cfg(test)]
                        self.icon_uploads.set(self.icon_uploads.get() + 1);
                    }
                    t.draw_bitmap(&cached.as_ref().unwrap().1, bounds, 1.0);
                }
                for (r, material) in &s.previews {
                    let _clip = ContentClip::new(&t, s, r.left >= s.content_left && !s.fixed_list());
                    // Keep only the current image. Theme, strength, custom color or
                    // device/context changes invalidate it; hover and scroll do not.
                    let mut cached = self.preview.borrow_mut();
                    if cached.as_ref().is_none_or(|cached| {
                        cached.context != *context
                            || cached.material != *material
                            || cached.dark != dark
                    }) {
                        let pixels = preview::pixels(*material, dark);
                        let bitmap = canvas_result(t.create_bitmap(&pixels, 400, 240))?;
                        *cached = Some(CachedPreview {
                            context: context.clone(),
                            material: *material,
                            dark,
                            bitmap,
                        });
                    }
                    t.draw_bitmap(&cached.as_ref().unwrap().bitmap, r, 1.0);
                    drop(cached);
                    let panel = Rect::from_xywh(r.left + 10.0, r.top + 10.0, 180.0, 100.0);
                    let preview_edge = canvas_result(
                        t.create_solid_brush(super::super::theme::panel_border(*material)),
                    )?;
                    let stroke = 1.0 / scale;
                    let inset = stroke * 0.5;
                    t.draw_rounded_rect(
                        &RoundedRect {
                            rect: Rect::from_xywh(
                                panel.left + inset,
                                panel.top + inset,
                                180.0 - stroke,
                                100.0 - stroke,
                            ),
                            radius_x: (4.0 - inset).max(0.0),
                            radius_y: (4.0 - inset).max(0.0),
                        },
                        &preview_edge,
                        stroke,
                    );
                    t.fill_rounded_rect(&RoundedRect {
                        rect: Rect::from_xywh(panel.left, panel.top + 28.0, 180.0, 0.75),
                        radius_x: 0.0, radius_y: 0.0,
                    }, &preview_edge);
                    t.clipped_text(
                        crate::i18n::text("ui-desktop-panel"),
                        &self.formats[0],
                        &Rect::from_xywh(panel.left + 20.0, panel.top + 2.0, 140.0, 24.0),
                        ink,
                    );
                    for i in 0..3 {
                        let left = panel.left + 18.0 + i as f32 * 52.0;
                        let glyph = if i < 2 { style::FOLDER } else { style::DOCUMENT };
                        t.clipped_icon(glyph, &self.formats[5],
                            &Rect::from_xywh(left, panel.top + 39.0, 28.0, 32.0), accent);
                        t.fill_rounded_rect(
                            &RoundedRect {
                                rect: Rect::from_xywh(left - 2.0, panel.top + 77.0, 32.0, 3.0),
                                radius_x: 1.5,
                                radius_y: 1.5,
                            },
                            muted,
                        );
                    }
                }
                if s.fixed_list() && let Some(r) = s.viewport {
                    t.draw_rounded_rect(&RoundedRect { rect: r, radius_x: Tokens::CARD_RADIUS, radius_y: Tokens::CARD_RADIUS }, card_border, 1.0);
                }
                let floating = s.floating_rule.map(|index| &s.rule_rows[index]);
                let mut grip_canvas = WidgetCanvas { ..widget_canvas };
                let paint_control = |i: usize, c: &Control, raised: bool| -> windows::core::Result<()> {
                    let _navigation_clip = matches!(c.kind, ControlKind::Navigation(_)).then(|| ContentClip::navigation(&t, s));
                    if s.fixed_list() && Scene::list_row(c) && s.viewport.is_some_and(|v| c.bounds.bottom <= v.top || c.bounds.top >= v.bottom) { return Ok(()); }
                    let _clip = ContentClip::new(
                        &t,
                        s,
                        c.bounds.left >= s.content_left
                            && !Scene::fixed_control(c)
                            && (!s.fixed_list() || Scene::list_row(c)),
                    );
                    let animation = motion.controls.get(&i).copied().filter(|_| !raised).unwrap_or_default();
                    let mut widget_canvas = WidgetCanvas {
                        motion: animation,
                        combo: matches!(c.kind, ControlKind::Combo),
                        ..widget_canvas
                    };
                    let state = widgets::State { enabled: c.enabled, active: !raised && (hover == Some(i)
                        || animation.hover.is_some_and(|progress| progress > 0.0)), focused: !raised && focus == Some(i) };
                    let bounds = components::widget_bounds(c.bounds);
                    if let ControlKind::Navigation(icon) | ControlKind::Tab(icon) = c.kind {
                        let mut navigation = WidgetCanvas { ..widget_canvas };
                        navigation.selection = animation.selection.unwrap_or(1.0);
                        navigation.brushes[Tone::Hover as usize] = nav_hovered;
                        navigation.brushes[Tone::Selection as usize] = if state.active {
                            nav_selected_hovered
                        } else {
                            nav_selected
                        };
                        if !c.selected {
                            navigation.brushes[Tone::AccentInk as usize] = ink;
                        }
                        let indicated = c.selected || animation.selection.is_some_and(|opacity| opacity > 0.0);
                        let item = NavigationItem { icon, selected: indicated };
                        if matches!(c.kind, ControlKind::Navigation(_)) {
                            item.paint_sidebar(&mut navigation, bounds, &c.label, state);
                        } else {
                            item.paint(&mut navigation, bounds, &c.label, state);
                        }
                        return Ok(());
                    }
                    if matches!(c.action, Action::FontSearch | Action::Classification(classification::Command::Input(_))) {
                        let r = c.bounds;
                        let font_search = matches!(c.action, Action::FontSearch);
                        let focused = if font_search {
                            unsafe {
                                let edit = windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus();
                                let owner = windows_sys::Win32::UI::WindowsAndMessaging::GetWindow(edit,
                                    windows_sys::Win32::UI::WindowsAndMessaging::GW_OWNER);
                                !edit.is_null() && windows_sys::Win32::UI::WindowsAndMessaging::GetPropW(owner,
                                    windows_sys::w!("LucidDesk.FontSearch")) == edit
                            }
                        } else { state.focused };
                        widgets::TextField::paint(&mut widget_canvas, bounds, widgets::State { focused, ..state });
                        if font_search {
                            t.clipped_text(&c.label, &self.formats[1],
                                &Rect::from_xywh(r.left + 40.0, r.top, r.right - r.left - 80.0, r.bottom - r.top), muted);
                            t.clipped_icon("\u{e721}", &self.formats[4],
                                &Rect::from_xywh(r.left + 16.0, r.top, 16.0, r.bottom - r.top), muted);
                            if c.selected {
                                t.clipped_icon("\u{e711}", &self.formats[4],
                                    &Rect::from_xywh(r.right - 32.0, r.top, 16.0, r.bottom - r.top), muted);
                            }
                        }
                        return Ok(());
                    }
                    let material = matches!(c.action, Action::Change(Event::Material(_))) && bounds.height >= 80.0;
                    match c.kind {
                        ControlKind::PrimaryButton => {
                            widgets::ButtonKind::Primary.paint_with_hover(&mut widget_canvas, bounds, &c.label,
                                state, animation.hover.unwrap_or(f32::from(u8::from(state.active))));
                            return Ok(());
                        }
                        ControlKind::Button if !c.label.is_empty() && !material => {
                            let button = if c.selected { widgets::ButtonKind::Selected } else { widgets::ButtonKind::Secondary };
                            button.paint_with_hover(&mut widget_canvas, bounds, &c.label, state,
                                animation.hover.unwrap_or(f32::from(u8::from(state.active))));
                            return Ok(());
                        }
                        ControlKind::Combo => {
                            widgets::ComboBox::paint(&mut widget_canvas, bounds, &c.label, state, controls::dropdown_is_open(c.bounds));
                            return Ok(());
                        }
                        ControlKind::Checkbox => {
                            let checked = animation.check.map_or(c.selected, |progress| progress > 0.0);
                            widgets::CheckBox::paint(&mut widget_canvas, bounds, &c.label, state, checked);
                            return Ok(());
                        }
                        ControlKind::Toggle => {
                            let selected = c.selected;
                            let progress = toggles.get(&i).copied().filter(|_| state.enabled)
                                .unwrap_or(if selected { 1.0 } else { 0.0 });
                            widgets::Toggle { selected, progress }.paint(&mut widget_canvas, bounds, state, dark, &palette)?;
                            return Ok(());
                        }
                        _ => {}
                    }
                    let inline = match c.kind {
                        ControlKind::QuietButton(icon) => Some(InlineControl::Quiet(icon)),
                        ControlKind::IconButton(glyph) => {
                            let open = controls::dropdown_is_open(c.bounds);
                            widget_canvas.selection = animation.disclosure.unwrap_or(1.0);
                            Some(InlineControl::Icon {
                                glyph, selected: animation.disclosure.map_or(open, |progress| progress > 0.0),
                            })
                        }
                        ControlKind::RuleDisclosure { missing_target } => {
                            let mut section_canvas = WidgetCanvas { leading_text: Some((Tokens::GROUP_TITLE, 0.0)), ..widget_canvas };
                            if missing_target {
                                section_canvas.brushes[Tone::Ink as usize] = red;
                                section_canvas.brushes[Tone::AccentInk as usize] = red;
                            }
                            InlineControl::Disclosure { expanded: c.selected, tinted: false }
                                .paint(&mut section_canvas, bounds, &c.label, state);
                            return Ok(());
                        }
                        ControlKind::SectionDisclosure { summary_width } => {
                            let reserved = summary_width + InlineControl::SECTION_DISCLOSURE_PADDING;
                            let mut section_canvas = WidgetCanvas { leading_text: Some((Tokens::SECTION_TITLE, reserved)), ..widget_canvas };
                            InlineControl::SectionDisclosure { expanded: c.selected }
                                .paint(&mut section_canvas, bounds, &c.label, state);
                            return Ok(());
                        }
                        ControlKind::FooterLink => Some(InlineControl::Link(match c.action {
                            Action::SolidReset | Action::StrengthReset => "\u{e777}",
                            Action::BackupAdvanced => "\u{e76c}",
                            _ => "\u{e8a7}",
                        })),
                        _ => None,
                    };
                    if let Some(inline) = inline {
                        inline.paint(&mut widget_canvas, bounds, &c.label, state);
                        return Ok(());
                    }
                    if let ControlKind::Slider(slider) = c.kind {
                        let thumb = animation.slider.unwrap_or_default();
                        let r = c.bounds;
                        let cy = (r.top + r.bottom) / 2.0;
                        let left = r.left + Style::SLIDER_INSET;
                        let right = r.right - Style::SLIDER_INSET;
                        let value = slider.value;
                        let max = slider.max;
                        let cx = left + (right - left) * value / max;
                        let rail = RoundedRect {
                            rect: Rect::from_xywh(left, cy - 2.0, right - left, 4.0),
                            radius_x: 2.0,
                            radius_y: 2.0,
                        };
                        t.fill_rounded_rect(&rail, slider_track);
                        let (fill_start, fill_width) = if slider.centered {
                            let middle = (left + right) * 0.5;
                            t.fill_rect(
                                &Rect::from_xywh(middle - 0.5, cy - 5.0, 1.0, 10.0),
                                muted,
                            );
                            (cx.min(middle), (cx - middle).abs())
                        } else {
                            (left, (cx - left).max(0.0))
                        };
                        let filled = RoundedRect {
                            rect: Rect::from_xywh(fill_start, cy - 2.0, fill_width, 4.0),
                            radius_x: 2.0,
                            radius_y: 2.0,
                        };
                        let channel_brush = slider.channel.map(|channel| match channel {
                            0 => red, 1 => green, _ => blue,
                        });
                        let slider_ink = channel_brush.unwrap_or(accent);
                        t.fill_rounded_rect(&filled, slider_ink);
                        t.fill_ellipse(
                            &Ellipse {
                                center: Vector2 { x: cx, y: cy },
                                radius_x: thumb.outer,
                                radius_y: thumb.outer,
                            },
                            slider_surface,
                        );
                        t.draw_ellipse(
                            &Ellipse {
                                center: Vector2 { x: cx, y: cy },
                                radius_x: thumb.outer,
                                radius_y: thumb.outer,
                            },
                            slider_border,
                            1.0,
                        );
                        t.fill_ellipse(
                            &Ellipse {
                                center: Vector2 { x: cx, y: cy },
                                radius_x: thumb.inner,
                                radius_y: thumb.inner,
                            },
                            slider_ink,
                        );
                        if focus == Some(i) {
                            t.draw_rounded_rect(
                                &RoundedRect {
                                    rect: r,
                                    radius_x: Style::RADIUS,
                                    radius_y: Style::RADIUS,
                                },
                                accent,
                                style::FOCUS_STROKE,
                            );
                        }
                        return Ok(());
                    }
                    let caption = matches!(c.kind, ControlKind::Caption);
                    let plain = c.kind.is_row();
                    let standard = matches!(c.kind, ControlKind::Button | ControlKind::Combo);

                    let rr = RoundedRect {
                        rect: c.bounds,
                        radius_x: if caption { 0.0 } else { Style::RADIUS },
                        radius_y: if caption { 0.0 } else { Style::RADIUS },
                    };
                    if plain {
                        let hover = animation.hover.unwrap_or(f32::from(u8::from(c.enabled && hover == Some(i))));
                        let selection = animation.selection.unwrap_or(f32::from(u8::from(c.selected)));
                        t.fill_highlight(&rr, hovered, hover * (1.0 - selection));
                        t.fill_highlight(&rr, selected, selection * (1.0 - hover));
                        t.fill_highlight(&rr, nav_selected_hovered, selection * hover);
                    } else if matches!(c.kind, ControlKind::BackButton) {
                        let hover = animation.hover.unwrap_or(f32::from(u8::from(state.active)));
                        t.fill_rounded_rect(&rr, if dark { card } else { slider_surface });
                        t.fill_highlight(&rr, hovered, hover);
                    } else if !caption || c.selected || hover == Some(i) {
                        t.fill_rounded_rect(
                            &rr,
                            if c.selected && c.enabled && hover == Some(i) {
                                nav_selected_hovered
                            } else if c.selected {
                                selected
                            } else if standard {
                                if c.enabled && hover == Some(i) { button_hover } else { button }
                            } else if c.enabled && hover == Some(i) {
                                hovered
                            } else if !dark {
                                slider_surface
                            } else {
                                card
                            },
                        );
                    }
                    if (!caption && !plain) || focus == Some(i) {
                        t.draw_rounded_rect(
                            &rr,
                            if focus == Some(i) || c.selected {
                                accent
                            } else {
                                border
                            },
                            if focus == Some(i) { style::FOCUS_STROKE } else { 1.0 },
                        );
                    }
                    let swatch = match c.action {
                        Action::ColorPreset(value) => Some(value),
                        Action::SolidColor if c.label.is_empty() => match s.material {
                            Backdrop::Solid { color, .. } => Some(color), _ => None,
                        },
                        _ => None,
                    };
                    if let Some(value) = swatch {
                        if c.selected {
                            t.draw_rounded_rect(&rr, accent, 2.0);
                        }
                        let chip = canvas_result(t.create_solid_brush(color(value)))?;
                        t.fill_rounded_rect(
                            &RoundedRect {
                                rect: Rect::from_xywh(
                                    c.bounds.left + 5.0,
                                    c.bounds.top + 5.0,
                                    c.bounds.right - c.bounds.left - 10.0,
                                    c.bounds.bottom - c.bounds.top - 10.0,
                                ),
                                radius_x: 3.0,
                                radius_y: 3.0,
                            },
                            &chip,
                        );
                    }
                    if material {
                        let r = c.bounds;
                        let inner = Rect::from_xywh(
                            r.left + 14.0,
                            r.top + 12.0,
                            r.right - r.left - 28.0,
                            56.0,
                        );
                        let tint = match c.action {
                            Action::Change(Event::Material(Backdrop::Acrylic)) => 0x5e819d,
                            Action::Change(Event::Material(Backdrop::Mica)) => 0x646b85,
                            Action::Change(Event::Material(Backdrop::Solid { color, .. })) => color,
                            _ => 0x7e718d,
                        };
                        let swatch = canvas_result(t.create_solid_brush(color(tint)))?;
                        t.fill_rounded_rect(
                            &RoundedRect {
                                rect: inner,
                                radius_x: 5.0,
                                radius_y: 5.0,
                            },
                            &swatch,
                        );
                        let glass = canvas_result(t.create_solid_brush(ColorF {
                            a: if matches!(
                                c.action,
                                Action::Change(Event::Material(Backdrop::Acrylic))
                            ) {
                                0.55
                            } else {
                                0.88
                            },
                            ..color(if dark { 0x242832 } else { 0xf1f5fb })
                        }))?;
                        let preview = Rect::from_xywh(
                            inner.left + 8.0,
                            inner.top + 8.0,
                            inner.right - inner.left - 16.0,
                            40.0,
                        );
                        t.fill_rounded_rect(
                            &RoundedRect {
                                rect: preview,
                                radius_x: 5.0,
                                radius_y: 5.0,
                            },
                            &glass,
                        );
                        let center_x = (preview.left + preview.right) * 0.5;
                        t.fill_rounded_rect(
                            &RoundedRect {
                                rect: Rect::from_xywh(
                                    center_x - 14.0,
                                    preview.top + 8.0,
                                    28.0,
                                    2.0,
                                ),
                                radius_x: 1.0,
                                radius_y: 1.0,
                            },
                            muted,
                        );
                        for (index, glyph) in [style::FOLDER, style::DOCUMENT, style::PHOTO].into_iter().enumerate() {
                            t.clipped_icon(glyph, &self.formats[4],
                                &Rect::from_xywh(preview.left + 8.0 + index as f32 * 18.0,
                                    preview.top + 18.0, 14.0, 16.0),
                                accent,
                            );
                        }
                        t.clipped_icon(
                            if c.selected { style::RADIO_ON } else { style::RADIO_OFF },
                            &self.formats[5],
                            &Rect::from_xywh(r.left + 15.0, r.bottom - 33.0, 16.0, 30.0),
                            if c.selected { accent } else { muted },
                        );
                        t.clipped_text(
                            &c.label,
                            &self.formats[1],
                            &Rect::from_xywh(
                                r.left + 42.0,
                                r.bottom - 35.0,
                                r.right - r.left - 50.0,
                                30.0,
                            ),
                            ink,
                        );
                    } else if caption {
                        if hover == Some(i) && matches!(c.action, Action::Window(SC_CLOSE)) {
                            let red = canvas_result(t.create_solid_brush(color(0xc42b1c)))?;
                            t.fill_rect(&c.bounds, &red);
                        }
                        let white = canvas_result(t.create_solid_brush(color(0xffffff)))?;
                        let brush =
                            if hover == Some(i) && matches!(c.action, Action::Window(SC_CLOSE)) {
                                &white
                            } else {
                                ink
                            };
                        if let Action::Window(command) = c.action {
                            let glyph = match command {
                                SC_MINIMIZE => "\u{e921}",
                                SC_MAXIMIZE => "\u{e922}",
                                SC_RESTORE => "\u{e923}",
                                SC_CLOSE => "\u{e8bb}",
                                _ => "",
                            };
                            t.clipped_icon(glyph, &self.formats[Tokens::CAPTION_ICON], &c.bounds, brush);
                        }
                    } else {
                        let mut bounds = c.bounds;
                        if plain {
                            bounds.left += Style::ROW_INSET;
                            if let Action::Font(name) = &c.action {
                                bounds.right -= 32.0;
                                if c.selected {
                                    t.clipped_icon(style::CHECK_MARK, &self.formats[4],
                                        &Rect::from_xywh(c.bounds.right - 28.0, c.bounds.top, 16.0, c.bounds.bottom - c.bounds.top), accent);
                                }
                                if name == crate::i18n::default_font() {
                                    let label = crate::i18n::text("ui-default");
                                    let width = self.label_width(label)? + 12.0;
                                    let tag = Rect::from_xywh(bounds.right - width, (bounds.top + bounds.bottom - 22.0) / 2.0, width, 22.0);
                                    t.clipped_text(label, &self.formats[6], &tag, muted);
                                    bounds.right = tag.left - 8.0;
                                }
                            }
                        }
                        let back = c.kind.is_back();
                        let forward = matches!(c.kind, ControlKind::ForwardRow);
                        if back || forward {
                            // Center the compact back button's icon and label as one group.
                            let left = if back && !plain {
                                controls::centered_icon_left(c.bounds, self.label_width(&c.label)?)
                            } else if back {
                                bounds.left
                            } else {
                                c.bounds.right - 28.0
                            };
                            t.clipped_icon(
                                if back { "\u{e72b}" } else { "\u{e76c}" },
                                &self.formats[4],
                                &Rect::from_xywh(
                                    left,
                                    c.bounds.top,
                                    16.0,
                                    c.bounds.bottom - c.bounds.top,
                                ),
                                if c.enabled { ink } else { disabled },
                            );
                            if back {
                                bounds.left = left + Style::ICON_SLOT + Style::ICON_GAP;
                            } else {
                                bounds.right -= 40.0;
                            }
                        }
                        t.clipped_text(
                            &c.label,
                            if plain || c.kind.is_back() {
                                &self.formats[1]
                            } else {
                                &self.button_format
                            },
                            &bounds,
                            if c.enabled { ink } else { disabled },
                        );
                    }
                    Ok(())
                };
                let paint_text = |index: usize, (r, text, size): &(Rect, String, usize)| {
                    let _navigation_clip = s.navigation_text.contains(&index).then(|| ContentClip::navigation(&t, s));
                    let _clip = ContentClip::new(&t, s, *size != Tokens::CAPTION_TEXT && r.left >= s.content_left && !s.fixed_list());
                    let _disclosure = s.disclosure_text_clip(index).map(|clip| ContentClip::rect(&t, clip));
                    if *size == Tokens::TITLE_BADGE {
                        t.clipped_icon(style::INFO, &self.formats[4],
                            &Rect::from_xywh(r.left + 8.0, r.top, Style::ICON_SLOT, r.bottom - r.top), blue);
                        let mut label = *r;
                        label.left += 28.0;
                        label.right -= 8.0;
                        t.clipped_text(text, &self.formats[*size], &label, blue);
                        return;
                    }
                    if matches!(*size, 4 | 5) {
                        t.clipped_icon(text, &self.formats[*size], r, ink);
                        return;
                    }
                    t.clipped_text(
                        text,
                        &self.formats[*size],
                        r,
                        match *size {
                            Tokens::STATUS_GOOD => green,
                            Tokens::STATUS_ERROR => red,
                            0 | 6 | 8 | Tokens::SECONDARY_VALUE => muted,
                            _ => ink,
                        },
                    );
                };
                let mut paint_grip = |index: usize, row: &RuleRow| {
                    let _clip = ContentClip::new(&t, s, true);
                    grip_canvas.motion.hover = motion.grip.filter(|(row, _)| *row == index).map(|(_, value)| value);
                    let active = s.rule_grip_hover == Some(index) || grip_canvas.motion.hover.is_some_and(|value| value > 0.0);
                    grip_canvas.brushes[Tone::Ink as usize] = if active { ink } else { muted };
                    InlineControl::Icon {
                        glyph: fonts::DRAG_HANDLE_ICON,
                        selected: s.rule_grip_pressed == Some(index) || s.floating_rule == Some(index),
                    }
                    .paint(
                        &mut grip_canvas,
                        components::widget_bounds(row.grip()),
                        "",
                        widgets::State { enabled: true, active, focused: false },
                    );
                };
                for (index, control) in s.controls.iter().enumerate() {
                    if floating.is_none_or(|row| !row.controls.contains(&index)) {
                        paint_control(index, control, false)?;
                    }
                }
                for (index, text) in s.text.iter().enumerate() {
                    if floating.is_none_or(|row| !row.text.contains(&index)) {
                        paint_text(index, text);
                    }
                }
                if s.rule_rows.len() > 1 {
                    for (index, row) in s.rule_rows.iter().enumerate() {
                        if s.floating_rule != Some(index) {
                            paint_grip(index, row);
                        }
                    }
                }
                if let (Some(index), Some(row)) = (s.floating_rule, floating) {
                    let _clip = ContentClip::new(&t, s, true);
                    let surface = RoundedRect { rect: row.visual_bounds(), radius_x: Style::RADIUS, radius_y: Style::RADIUS };
                    t.fill_rounded_rect(&surface, brush(Tone::Selection));
                    t.draw_rounded_rect(&surface, accent, 1.0);
                    for index in row.controls.clone() {
                        paint_control(index, &s.controls[index], true)?;
                    }
                    for index in row.text.clone() {
                        paint_text(index, &s.text[index]);
                    }
                    paint_grip(index, row);
                }
                if let Some(bounds) = &s.activity {
                    let _clip = ContentClip::new(&t, s, true);
                    let angle = motion.activity.unwrap_or(0.0);
                    t.rotated_icon(widgets::motion::ACTIVITY_GLYPH, &self.formats[5], bounds, accent, angle);
                }
                let paint_scrollbar = |bar: widgets::Scrollbar, index: usize| {
                    let mut thumb = bar.thumb;
                    let growth = widgets::Scrollbar::thumb_growth(motion.scrollbars[index]);
                    thumb.x -= growth;
                    thumb.width += growth;
                    widgets::Scrollbar::paint_thumb(&mut WidgetCanvas { ..widget_canvas }, thumb);
                };
                if let Some(bar) = s.scrollbar() {
                    paint_scrollbar(bar, 0);
                }
                if let Some(bar) = s.navigation_scrollbar() {
                    paint_scrollbar(bar, 1);
                }
                t.finish()
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_icon_upload_is_reused_and_released_when_not_visible() {
        let _sta = crate::pane::test_support::apartment();
        let device = windows_canvas::GpuDevice::new_warp().unwrap();
        let first = super::super::super::canvas::Offscreen::new(&device, 800, 600).unwrap();
        let second = super::super::super::canvas::Offscreen::new(&device, 800, 600).unwrap();
        let painter = Painter::new().unwrap();
        let mut scene = scene(800.0, 600.0, 0, true,
            (PanelTheme::System, Backdrop::Acrylic), Default::default());
        scene.app_icon = Some(Rect::from_xywh(300.0, 100.0, 64.0, 64.0));
        let paint = |target, scene: &Scene, dark| painter.paint(target, scene, 800.0, 600.0,
            1.0, dark, false, None, None, &Default::default()).unwrap();
        paint(&first.target, &scene, false);
        let pixels = first.pixels().unwrap();
        paint(&first.target, &scene, false);
        assert_eq!(first.pixels().unwrap(), pixels);
        paint(&first.target, &scene, true);
        assert_eq!(painter.icon_uploads.get(), 1);
        paint(&second.target, &scene, false);
        assert_eq!(painter.icon_uploads.get(), 2);
        assert_eq!(second.pixels().unwrap(), pixels);
        scene.app_icon = None;
        paint(&second.target, &scene, false);
        assert!(painter.icon_bitmap.borrow().is_none());
    }
}
