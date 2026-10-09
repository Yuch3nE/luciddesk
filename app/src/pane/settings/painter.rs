//! Shared rendering for the explicit settings control types.
use super::*;

struct CachedPreview {
    context: ID2D1DeviceContext,
    material: Backdrop,
    dark: bool,
    bitmap: windows_canvas::Bitmap,
}

pub(super) struct Painter {
    preview: RefCell<Option<CachedPreview>>,
    brushes: RefCell<super::super::canvas::Brushes<15>>,
    icon_bitmap: RefCell<Option<(ID2D1DeviceContext, windows_canvas::Bitmap)>>,
    #[cfg(test)]
    icon_uploads: std::cell::Cell<usize>,
    app_icon: super::super::assets::Pixels,
    formats: Vec<windows_canvas::TextFormat>,
    button_format: windows_canvas::TextFormat,
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
        ]
        .iter()
        .enumerate()
        {
            let format = canvas_result(TextFormat::with_weight(
                if matches!(i, 4 | 5) {
                    super::super::fonts::icon_family()
                } else {
                    &family
                },
                *size,
                FontWeight(if i == 2 || i == 3 { 600 } else { 400 }),
            ))?
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(if matches!(i, 1 | 6) {
                WordWrapping::Wrap
            } else {
                WordWrapping::NoWrap
            });
            let format = if matches!(i, 4 | 5) {
                format.with_alignment(windows_canvas::TextAlignment::Center)
            } else if i == Tokens::VALUE_TEXT {
                format.with_alignment(windows_canvas::TextAlignment::Trailing)
            } else {
                format
            };
            super::super::canvas::ellipsis(&format)?;
            formats.push(format);
        }
        let button_format = canvas_result(TextFormat::new(&family, 14.0))?
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(WordWrapping::NoWrap)
            .with_alignment(windows_canvas::TextAlignment::Center);
        super::super::canvas::ellipsis(&button_format)?;
        Ok(Self {
            preview: RefCell::new(None),
            brushes: Default::default(),
            icon_bitmap: RefCell::new(None),
            #[cfg(test)]
            icon_uploads: std::cell::Cell::new(0),
            app_icon: {
                // Use the straight-alpha PNG frame; smaller DIB frames target Shell compatibility.
                let icon = crate::app_icon::load(256, 256).map_err(|message| {
                    windows::core::Error::new(windows::Win32::Foundation::E_FAIL, message)
                })?;
                super::super::assets::icon_pixels(windows::Win32::UI::WindowsAndMessaging::HICON(
                    icon.0,
                ))?
            },
            formats,
            button_format,
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
    pub(super) fn paint(
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
    ) -> windows::core::Result<()> {
        {
            let context = t;
            if s.app_icon.is_none() {
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
                let [nav_selection, nav_hover, nav_selection_hover] =
                    components::navigation_colors(s.material, dark);
                let [card, card_border, ink, muted, disabled, border, accent, slider_track,
                    slider_border, slider_surface, scroll_thumb, nav_selected, nav_hovered,
                    nav_selected_hovered, page_background] = canvas_result(brushes.get(context, &t, [
                        chrome.card, chrome.card_border,
                        color(palette.ink), color(palette.muted),
                        ColorF { a: 0.55, ..color(palette.muted) },
                        ColorF { a: if native { 0.45 } else { 1.0 }, ..color(palette.border) },
                        color(palette.accent),
                        // Light sliders need opaque strokes on translucent cards.
                        ColorF { a: if dark && native { 0.45 } else { 1.0 }, ..color(palette.slider_track) },
                        ColorF { a: if dark && native { 0.45 } else { 1.0 }, ..color(palette.slider_border) },
                        if dark { chrome.card } else { color(0xffffff) },
                        color(palette.scroll_thumb), nav_selection, nav_hover, nav_selection_hover,
                        ColorF { a: if native { if dark { 0.12 } else { 0.22 } } else { 1.0 },
                            ..color(if dark { 0x242424 } else { 0xf9f9f9 }) },
                    ]))?;
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
                t.fill_rounded_rect(
                    &RoundedRect {
                        rect: Rect::from_xywh(Tokens::content_x() - 24.0, 0.0, width - Tokens::content_x() + 24.0, height),
                        radius_x: 8.0,
                        radius_y: 8.0,
                    },
                    page_background,
                );
                for r in &s.separators {
                    let _clip = ContentClip::new(&t, s, r.left >= Tokens::content_x() && !s.fixed_list());
                    t.fill_rect(r, border);
                }
                for r in &s.cards {
                    let _clip = ContentClip::new(&t, s, r.left >= Tokens::content_x() && !s.fixed_list());
                    let rr = RoundedRect {
                        rect: *r,
                        radius_x: Tokens::CARD_RADIUS,
                        radius_y: Tokens::CARD_RADIUS,
                    };
                    t.fill_rounded_rect(&rr, card);
                    t.draw_rounded_rect(&rr, card_border, 1.0);
                }
                if let Some(bounds) = &s.app_icon {
                    let _clip = ContentClip::new(&t, s, true);
                    let mut cached = self.icon_bitmap.borrow_mut();
                    if cached.as_ref().is_none_or(|(old, _)| old != context) {
                        *cached = None;
                        let pixels = &self.app_icon;
                        *cached = Some((context.clone(), canvas_result(t.create_bitmap(
                            &pixels.data, pixels.width, pixels.height,
                        ))?));
                        #[cfg(test)]
                        self.icon_uploads.set(self.icon_uploads.get() + 1);
                    }
                    t.draw_bitmap(&cached.as_ref().unwrap().1, bounds, 1.0);
                }
                for (r, material) in &s.previews {
                    let _clip = ContentClip::new(&t, s, r.left >= Tokens::content_x() && !s.fixed_list());
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
                        t.create_solid_brush(super::super::theme::panel_border(dark, *material)),
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
                    // Desktop Mica panes expose one continuous material surface.
                    if !matches!(material.base(), Backdrop::Mica | Backdrop::MicaAlt) {
                        let layer = canvas_result(t.create_solid_brush(ColorF {
                            a: if dark { 0.10 } else { 0.48 },
                            ..color(0xffffff)
                        }))?;
                        t.fill_rounded_rect(
                            &RoundedRect {
                                rect: Rect::from_xywh(
                                    panel.left + 4.0,
                                    panel.top + 27.0,
                                    172.0,
                                    69.0,
                                ),
                                radius_x: 3.0,
                                radius_y: 3.0,
                            },
                            &layer,
                        );
                    }
                    t.clipped_text(
                        crate::i18n::text("ui-desktop-panel"),
                        &self.formats[0],
                        &Rect::from_xywh(panel.left + 10.0, panel.top + 2.0, 120.0, 24.0),
                        ink,
                    );
                    for i in 0..3 {
                        let left = panel.left + 18.0 + i as f32 * 52.0;
                        t.fill_rounded_rect(
                            &RoundedRect {
                                rect: Rect::from_xywh(left, panel.top + 39.0, 28.0, 28.0),
                                radius_x: 5.0,
                                radius_y: 5.0,
                            },
                            accent,
                        );
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
                for (i, c) in s.controls.iter().enumerate() {
                    if s.fixed_list() && Scene::list_row(c) && s.viewport.is_some_and(|v| c.bounds.bottom <= v.top || c.bounds.top >= v.bottom) { continue; }
                    let _clip = ContentClip::new(
                        &t,
                        s,
                        c.bounds.left >= Tokens::content_x()
                            && !matches!(c.kind, ControlKind::Caption)
                            && (!s.fixed_list() || Scene::list_row(c)),
                    );
                    if matches!(c.action, Action::FontSearch) {
                        let r = c.bounds;
                        let background = canvas_result(t.create_solid_brush(ColorF {
                            a: if dark { 0.04 } else { 0.45 }, ..color(0xffffff)
                        }))?;
                        let frame = RoundedRect { rect: r, radius_x: 8.0, radius_y: 8.0 };
                        t.fill_rounded_rect(&frame, &background);
                        t.draw_rounded_rect(&frame, border, 1.0);
                        let hint = canvas_result(t.create_solid_brush(ColorF {
                            a: 1.0, ..color(if dark { 0xb8b8b8 } else { 0x606060 })
                        }))?;
                        t.clipped_text(&c.label, &self.formats[1],
                            &Rect::from_xywh(r.left + 16.0, r.top, r.right - r.left - 56.0, r.bottom - r.top), &hint);
                        t.clipped_icon(if c.selected { "\u{e711}" } else { "\u{e721}" }, &self.formats[4],
                            &Rect::from_xywh(r.right - 32.0, r.top, 16.0, r.bottom - r.top), &hint);
                        let focused = unsafe {
                            let focus = windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus();
                            let owner = windows_sys::Win32::UI::WindowsAndMessaging::GetWindow(focus,
                                windows_sys::Win32::UI::WindowsAndMessaging::GW_OWNER);
                            !focus.is_null() && windows_sys::Win32::UI::WindowsAndMessaging::GetPropW(owner,
                                windows_sys::w!("LucidDesk.FontSearch")) == focus
                        };
                        if focused {
                            t.draw_line(Vector2::new(r.left + 8.0, r.bottom - 1.0),
                                Vector2::new(r.right - 8.0, r.bottom - 1.0), accent, 2.0);
                        }
                        continue;
                    }
                    if !c.enabled && c.is_toggle() {
                        let r = c.bounds;
                        t.draw_rounded_rect(
                            &RoundedRect {
                                rect: r,
                                radius_x: 12.0,
                                radius_y: 12.0,
                            },
                            border,
                            1.0,
                        );
                        t.fill_ellipse(
                            &toggle_thumb(r, if c.selected { 1.0 } else { 0.0 }),
                            muted,
                        );
                        continue;
                    }
                    if let ControlKind::Slider(slider) = c.kind {
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
                        let channel_brush = if let Some(channel) = slider.channel {
                            Some(canvas_result(t.create_solid_brush(color(match channel {
                                0 => {
                                    if dark {
                                        0xef8d8d
                                    } else {
                                        0xb83d42
                                    }
                                }
                                1 => {
                                    if dark {
                                        0x8dccaa
                                    } else {
                                        0x287b50
                                    }
                                }
                                _ => {
                                    if dark {
                                        0x88baf0
                                    } else {
                                        0x266eae
                                    }
                                }
                            })))?)
                        } else {
                            None
                        };
                        let slider_ink = channel_brush.as_ref().unwrap_or(accent);
                        t.fill_rounded_rect(&filled, slider_ink);
                        t.fill_ellipse(
                            &Ellipse {
                                center: Vector2 { x: cx, y: cy },
                                radius_x: 8.0,
                                radius_y: 8.0,
                            },
                            slider_surface,
                        );
                        t.draw_ellipse(
                            &Ellipse {
                                center: Vector2 { x: cx, y: cy },
                                radius_x: 8.0,
                                radius_y: 8.0,
                            },
                            slider_border,
                            1.0,
                        );
                        t.fill_ellipse(
                            &Ellipse {
                                center: Vector2 { x: cx, y: cy },
                                radius_x: 5.0,
                                radius_y: 5.0,
                            },
                            slider_ink,
                        );
                        if focus == Some(i) {
                            t.draw_rounded_rect(
                                &RoundedRect {
                                    rect: r,
                                    radius_x: 5.0,
                                    radius_y: 5.0,
                                },
                                accent,
                                2.0,
                            );
                        }
                        continue;
                    }
                    if matches!(c.kind, ControlKind::Combo) {
                        let rounded = RoundedRect {
                            rect: c.bounds,
                            radius_x: Style::COMBO_RADIUS,
                            radius_y: Style::COMBO_RADIUS,
                        };
                        t.fill_rounded_rect(
                            &rounded,
                            if c.enabled && hover == Some(i) {
                                hovered
                            } else {
                                card
                            },
                        );
                        t.draw_rounded_rect(&rounded, border, 1.0);
                        if c.enabled {
                            t.draw_line(
                                Vector2 {
                                    x: c.bounds.left + 4.0,
                                    y: c.bounds.bottom - 1.0,
                                },
                                Vector2 {
                                    x: c.bounds.right - 4.0,
                                    y: c.bounds.bottom - 1.0,
                                },
                                if focus == Some(i) { accent } else { muted },
                                if focus == Some(i) { 2.0 } else { 1.0 },
                            );
                        }
                        let text = if c.enabled { ink } else { muted };
                        t.clipped_text(
                            &c.label,
                            &self.formats[1],
                            &Rect::from_xywh(
                                c.bounds.left + 12.0,
                                c.bounds.top,
                                c.bounds.right - c.bounds.left - 44.0,
                                Style::COMBO_HEIGHT,
                            ),
                            text,
                        );
                        t.clipped_icon(
                            "\u{e70d}",
                            &self.formats[4],
                            &Rect::from_xywh(
                                c.bounds.right - 28.0,
                                c.bounds.top,
                                Style::ICON_SLOT,
                                Style::COMBO_HEIGHT,
                            ),
                            text,
                        );
                        continue;
                    }
                    let navigation = matches!(c.kind, ControlKind::Navigation);
                    let caption = matches!(c.kind, ControlKind::Caption);
                    let plain = c.kind.is_row();

                    let material = matches!(c.action, Action::Change(Event::Material(_)))
                        && c.bounds.bottom - c.bounds.top >= 80.0;
                    let rr = RoundedRect {
                        rect: c.bounds,
                        radius_x: if c.is_toggle() {
                            (c.bounds.bottom - c.bounds.top) / 2.0
                        } else if caption {
                            0.0
                        } else {
                            Style::RADIUS
                        },
                        radius_y: if c.is_toggle() {
                            (c.bounds.bottom - c.bounds.top) / 2.0
                        } else if caption {
                            0.0
                        } else {
                            Style::RADIUS
                        },
                    };
                    let progress =
                        toggles
                            .get(&i)
                            .copied()
                            .unwrap_or(if c.selected { 1.0 } else { 0.0 });
                    if c.is_toggle() {
                        let off = color(if dark { 0x383838 } else { 0xffffff });
                        let on = color(palette.accent);
                        let brush = canvas_result(t.create_solid_brush(ColorF {
                            r: off.r + (on.r - off.r) * progress,
                            g: off.g + (on.g - off.g) * progress,
                            b: off.b + (on.b - off.b) * progress,
                            a: 1.0,
                        }))?;
                        t.fill_rounded_rect(&rr, &brush);
                    } else if (!navigation && !caption && !plain) || c.selected || hover == Some(i)
                    {
                        t.fill_rounded_rect(
                            &rr,
                            if navigation && c.selected && c.enabled && hover == Some(i) {
                                nav_selected_hovered
                            } else if navigation && c.selected {
                                nav_selected
                            } else if navigation && c.enabled && hover == Some(i) {
                                nav_hovered
                            } else if c.selected && c.enabled && hover == Some(i) {
                                nav_selected_hovered
                            } else if c.selected {
                                selected
                            } else if c.enabled && hover == Some(i) {
                                hovered
                            } else {
                                card
                            },
                        );
                    }
                    if (!navigation && !caption && !plain) || focus == Some(i) {
                        t.draw_rounded_rect(
                            &rr,
                            if focus == Some(i) || c.selected {
                                accent
                            } else {
                                border
                            },
                            if focus == Some(i) { 2.0 } else { 1.0 },
                        );
                    }
                    if navigation && c.selected {
                        t.fill_rounded_rect(
                            &RoundedRect {
                                rect: Rect::from_xywh(
                                    c.bounds.left + 3.0,
                                    c.bounds.top + 11.0,
                                    3.0,
                                    18.0,
                                ),
                                radius_x: 1.5,
                                radius_y: 1.5,
                            },
                            accent,
                        );
                    }
                    if let Action::ColorPreset(value) = c.action {
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
                        for j in 0..3 {
                            t.fill_rounded_rect(
                                &RoundedRect {
                                    rect: Rect::from_xywh(
                                        preview.left + 8.0 + j as f32 * 18.0,
                                        preview.top + 20.0,
                                        12.0,
                                        12.0,
                                    ),
                                    radius_x: 3.0,
                                    radius_y: 3.0,
                                },
                                accent,
                            );
                        }
                        let center = Vector2 {
                            x: r.left + 23.0,
                            y: r.bottom - 18.0,
                        };
                        t.draw_ellipse(
                            &Ellipse {
                                center,
                                radius_x: 8.0,
                                radius_y: 8.0,
                            },
                            if c.selected { accent } else { muted },
                            1.5,
                        );
                        if c.selected {
                            t.fill_ellipse(
                                &Ellipse {
                                    center,
                                    radius_x: 4.0,
                                    radius_y: 4.0,
                                },
                                accent,
                            );
                        }
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
                    } else if c.is_toggle() {
                        let off = color(if dark { 0xf5f5f5 } else { 0x666666 });
                        let on = color(if dark { 0x202020 } else { 0xffffff });
                        let brush = canvas_result(t.create_solid_brush(ColorF {
                            r: off.r + (on.r - off.r) * progress,
                            g: off.g + (on.g - off.g) * progress,
                            b: off.b + (on.b - off.b) * progress,
                            a: 1.0,
                        }))?;
                        t.fill_ellipse(&toggle_thumb(c.bounds, progress), &brush);
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
                            t.clipped_icon(glyph, &self.formats[4], &c.bounds, brush);
                        }
                    } else {
                        let mut bounds = c.bounds;
                        if navigation {
                            bounds.left += Style::NAV_TEXT_INSET;
                        } else if plain {
                            bounds.left += Style::ROW_INSET;
                            if matches!(c.action, Action::Font(_) | Action::Language(_)) {
                                bounds.left += 24.0;
                                if c.selected {
                                    t.clipped_icon("\u{e73e}", &self.formats[4],
                                        &Rect::from_xywh(c.bounds.left + 12.0, c.bounds.top, 16.0, c.bounds.bottom - c.bounds.top), accent);
                                }
                            }
                        }
                        if !navigation {
                            let back = c.kind.is_back();
                            let forward = matches!(c.kind, ControlKind::ForwardRow);
                            if back || forward {
                                // Center the compact back button's icon and label as one group.
                                let left = if back && !plain {
                                    controls::centered_icon_left(
                                        c.bounds,
                                        self.label_width(&c.label)?,
                                    )
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
                        }
                        t.clipped_text(
                            &c.label,
                            if navigation || plain || c.kind.is_back() {
                                &self.formats[1]
                            } else {
                                &self.button_format
                            },
                            &bounds,
                            if c.enabled { ink } else { disabled },
                        );
                    }
                }
                for (r, text, size) in &s.text {
                    let _clip = ContentClip::new(&t, s, r.left >= Tokens::content_x() && !s.fixed_list());
                    if matches!(*size, 4 | 5) {
                        t.clipped_icon(text, &self.formats[*size], r, ink);
                        continue;
                    }
                    t.clipped_text(
                        text,
                        &self.formats[*size],
                        r,
                        if matches!(*size, 0 | 6) { muted } else { ink },
                    );
                }
                if let Some(thumb) = s.scroll_thumb() {
                    t.fill_rounded_rect(
                        &RoundedRect {
                            rect: thumb,
                            radius_x: 2.0,
                            radius_y: 2.0,
                        },
                        scroll_thumb,
                    );
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
