//! Cached Canvas content. Live windows draw directly into the GPU swap chain.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use windows_canvas::{ColorF, Ellipse, Rect, RoundedRect, Vector2};

use super::native_graphics::canvas_result;
use super::{GroupModel, assets, canvas, layout::HEADER};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use windows_canvas::ID2D1DeviceContext;

use windows::core::Result;
type ImageBitmap = (Arc<assets::Pixels>, windows_canvas::Bitmap, (u32, u32));

// A quiet, font-independent silhouette fills the same slot as the real icon.
// It also remains useful when Shell extraction fails; no perpetual loading animation.
fn draw_placeholder(target: &canvas::DrawPass<'_>, slot: Rect, folder: bool,
    fill: &windows_canvas::Brush, edge: &windows_canvas::Brush) {
    let size = slot.right - slot.left;
    let x = slot.left;
    let y = slot.top;
    let stroke = (size * 0.035).clamp(1.0, 1.6);
    let shape = |left: f32, top: f32, width: f32, height: f32| RoundedRect {
        rect: Rect::from_xywh(x + left * size, y + top * size, width * size, height * size),
        radius_x: size * 0.06,
        radius_y: size * 0.06,
    };
    if folder {
        let tab = shape(0.10, 0.22, 0.38, 0.22);
        target.fill_rounded_rect(&tab, fill);
        target.draw_rounded_rect(&tab, edge, stroke);
        let body = shape(0.10, 0.34, 0.80, 0.48);
        target.fill_rounded_rect(&body, fill);
        target.draw_rounded_rect(&body, edge, stroke);
    } else {
        let body = shape(0.22, 0.10, 0.56, 0.80);
        target.fill_rounded_rect(&body, fill);
        target.draw_rounded_rect(&body, edge, stroke);
        for (top, width) in [(0.55, 0.32), (0.69, 0.22)] {
            target.draw_line(Vector2::new(x + size * 0.34, y + size * top),
                Vector2::new(x + size * (0.34 + width), y + size * top), edge, stroke);
        }
    }
}

struct TitleLayout {
    text: String,
    natural_width: f32,
    width_pixels: f32,
    scale: f32,
    layout: windows_canvas::TextLayout,
}

pub struct Renderer {
    family: String,
    language: &'static str,
    #[cfg(test)]
    offscreen_device: Option<windows_canvas::GpuDevice>,
    labels: windows_canvas::TextFormat,
    title: windows_canvas::TextFormat,
    title_layout: Option<TitleLayout>,
    pub(super) marquee: Option<luciddesk_core::RectDip>,
    details: windows_canvas::TextFormat,
    tab_title: windows_canvas::TextFormat,
    column_label_widths: [f32; 4],
    sort_icons: windows_canvas::TextFormat,
    menu_shortcut: windows_canvas::TextFormat,
    icons: windows_canvas::TextFormat,
    navigation_icons: windows_canvas::TextFormat,
    target: Option<(u32, u32, Option<canvas::Offscreen>, ID2D1DeviceContext)>,
    brushes: std::cell::RefCell<canvas::Brushes<9>>,
    images: HashMap<usize, ImageBitmap>,
    states: HashMap<(u32, u32, u32, i32), windows_canvas::Bitmap>,
}

impl Renderer {
    pub(super) fn menu_width(&self, rows: &[super::menu::Entry]) -> f32 {
        rows.iter().filter(|row| row.id != 0).fold(216.0_f32, |width, row| {
            let trailing = if !row.children.is_empty() { 24.0 } else if row.trailing.is_empty() { 0.0 } else { 72.0 };
            let measured = windows_canvas::TextLayout::new(row.label, &self.title, 4096.0, 64.0)
                .map(|layout| layout.metrics().width_including_trailing_whitespace).unwrap_or(166.0);
            width.max((measured + 54.0 + trailing).ceil())
        }).min(480.0)
    }

    #[cfg(test)]
    pub fn flyout(
        &mut self,
        width: u32,
        height: u32,
        scale: f32,
        rows: &[super::menu::Entry],
        selected: Option<usize>,
        native: bool,
        dark: bool,
    ) -> Result<Vec<u8>> {
        self.prepare(width, height, scale)?;
        let (_, _, bitmap, target) = self.target.as_ref().unwrap();
        let highlights: Vec<_> = (0..rows.len())
            .map(|i| if selected == Some(i) { 1.0 } else { 0.0 })
            .collect();
        self.paint_flyout(
            target,
            width,
            height,
            scale,
            rows,
            &highlights,
            native,
            dark,
        )?;
        bitmap.as_ref().unwrap().pixels()
    }

    pub fn paint_flyout(
        &self,
        target: &ID2D1DeviceContext,
        width: u32,
        height: u32,
        scale: f32,
        rows: &[super::menu::Entry],
        highlights: &[f32],
        native: bool,
        dark: bool,
    ) -> Result<()> {
        canvas::draw(target, scale, |target| {
            let ink = if dark { 0.95 } else { 0.10 };
            let text = canvas_result(target.create_solid_brush(ColorF::new(ink, ink, ink, 1.0)))?;
            let subtle =
                canvas_result(target.create_solid_brush(ColorF::new(ink, ink, ink, 0.13)))?;
            target.clear(ColorF::new(0.0, 0.0, 0.0, 0.0));
            let width = width as f32 / scale;
            if !native {
                let background = canvas_result(target.create_solid_brush(ColorF::new(
                    if dark { 0.09 } else { 0.96 },
                    if dark { 0.10 } else { 0.96 },
                    if dark { 0.12 } else { 0.96 },
                    1.0,
                )))?;
                target.fill_rounded_rect(&RoundedRect {
                    rect: Rect::from_xywh(0.0, 0.0, width, height as f32 / scale),
                    radius_x: super::menu::CORNER_RADIUS + 0.5,
                    radius_y: super::menu::CORNER_RADIUS + 0.5,
                }, &background);
            }
            target.draw_rounded_rect(
                &RoundedRect {
                    rect: Rect::from_xywh(0.5, 0.5, width - 1.0, height as f32 / scale - 1.0),
                    radius_x: super::menu::CORNER_RADIUS,
                    radius_y: super::menu::CORNER_RADIUS,
                },
                &subtle,
                1.0,
            );
            for (index, row) in rows.iter().enumerate() {
                let top = super::menu::row_top(rows, index);
                if row.id == 0 {
                    target.fill_rect(
                        &Rect::from_xywh(12.0, top + 4.0, width - 24.0, 1.0),
                        &subtle,
                    );
                    continue;
                }
                let progress = highlights.get(index).copied().unwrap_or(0.0);
                if progress > 0.0 {
                    let hover = canvas_result(target.create_solid_brush(ColorF::new(
                        ink,
                        ink,
                        ink,
                        (if dark { 0.09 } else { 0.05 }) * progress,
                    )))?;
                    target.fill_rounded_rect(
                        &RoundedRect {
                            rect: Rect::from_xywh(
                                4.0,
                                top + 2.0,
                                width - 8.0,
                                super::menu::ROW_HEIGHT - 4.0,
                            ),
                            radius_x: 5.0,
                            radius_y: 5.0,
                        },
                        &hover,
                    );
                }
                let trailing_width = if !row.children.is_empty() { 24.0 } else if row.trailing.is_empty() { 0.0 } else { 72.0 };
                for (label, left, available) in [
                    (row.icon, 12.0, 20.0),
                    (row.label, 38.0, width - 50.0 - trailing_width),
                ] {
                    if label.chars().next().is_some_and(|ch| ('\u{e000}'..='\u{f8ff}').contains(&ch)) {
                        target.clipped_icon(label, &self.icons,
                            &Rect::from_xywh(left, top, available, super::menu::ROW_HEIGHT), &text);
                        continue;
                    }
                    target.clipped_text(
                        label,
                        &self.title,
                        &Rect::from_xywh(left, top, available, super::menu::ROW_HEIGHT),
                        &text,
                    );
                }
                if !row.children.is_empty() {
                    target.clipped_icon("\u{e76c}", &self.icons,
                        &Rect::from_xywh(width - 28.0, top, 16.0, super::menu::ROW_HEIGHT), &text);
                } else if trailing_width > 0.0 {
                    target.clipped_text(
                        row.trailing,
                        &self.menu_shortcut,
                        &Rect::from_xywh(width - 76.0, top, 64.0, super::menu::ROW_HEIGHT),
                        &text,
                    );
                }
            }
            target.finish()
        })
    }
    pub fn new() -> Result<Self> {
        use windows_canvas::{ParagraphAlignment, TextAlignment, TextFormat, WordWrapping};
        let (family, size) = assets::font();
        let labels = canvas_result(TextFormat::new(&family, size))?
            .with_alignment(TextAlignment::Center)
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(WordWrapping::Wrap);
        canvas::ellipsis(&labels)?;
        let title = canvas_result(TextFormat::new(&family, size + 1.0))?
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(WordWrapping::NoWrap);
        canvas::ellipsis(&title)?;
        let details = canvas_result(TextFormat::new(&family, 12.0))?
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(WordWrapping::NoWrap);
        canvas::ellipsis(&details)?;
        let tab_title = canvas_result(TextFormat::new(&family, 12.0))?
            .with_alignment(TextAlignment::Center)
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(WordWrapping::NoWrap);
        canvas::ellipsis(&tab_title)?;
        let mut column_label_widths = [0.0; 4];
        for (index, name) in [crate::i18n::text("ui-name"), crate::i18n::text("ui-type"), crate::i18n::text("ui-modified"), crate::i18n::text("ui-size")].iter().enumerate() {
            column_label_widths[index] = canvas_result(windows_canvas::TextLayout::new(
                name, &details, 256.0, super::layout::LIST_HEADER,
            ))?.metrics().width_including_trailing_whitespace;
        }
        Ok(Self {
            family: family.clone(),
            language: crate::i18n::language(),
            #[cfg(test)]
            offscreen_device: None,
            labels,
            title,
            title_layout: None,
            marquee: None,
            details,
            tab_title,
            column_label_widths,
            sort_icons: canvas_result(TextFormat::new(super::fonts::icon_family(), 10.0))?
                .with_alignment(TextAlignment::Center)
                .with_paragraph_alignment(ParagraphAlignment::Center)
                .with_word_wrapping(WordWrapping::NoWrap),
            menu_shortcut: canvas_result(TextFormat::new(&family, 12.0))?
                .with_alignment(TextAlignment::Trailing)
                .with_paragraph_alignment(ParagraphAlignment::Center)
                .with_word_wrapping(WordWrapping::NoWrap),
            icons: canvas_result(TextFormat::new(super::fonts::icon_family(), 12.0))?
                .with_alignment(windows_canvas::TextAlignment::Center)
                .with_paragraph_alignment(ParagraphAlignment::Center)
                .with_word_wrapping(WordWrapping::NoWrap),
            navigation_icons: canvas_result(TextFormat::new(super::fonts::icon_family(), 12.0))?
                .with_alignment(TextAlignment::Center)
                .with_paragraph_alignment(ParagraphAlignment::Center)
                .with_word_wrapping(WordWrapping::NoWrap),
            target: None,
            brushes: Default::default(),
            images: HashMap::new(),
            states: HashMap::new(),
        })
    }

    #[cfg(test)]
    fn prepare(&mut self, width: u32, height: u32, _scale: f32) -> Result<()> {
        if self
            .target
            .as_ref()
            .is_none_or(|(w, h, bitmap, _)| *w != width || *h != height || bitmap.is_none())
        {
            if self.offscreen_device.is_none() {
                self.offscreen_device = Some(super::native_graphics::gpu_device()?);
            }
            let bitmap =
                canvas::Offscreen::new(self.offscreen_device.as_ref().unwrap(), width, height)?;
            let target = bitmap.target.clone();
            self.target = Some((width, height, Some(bitmap), target));
            self.images.clear();
            self.states.clear();
        }
        Ok(())
    }

    /// CPU readback is reserved for small flyouts, export and regression tests.
    #[cfg(test)]
    pub fn pixels(
        &mut self,
        width: u32,
        height: u32,
        scale: f32,
        model: &GroupModel,
    ) -> Result<Vec<u8>> {
        self.prepare(width, height, scale)?;
        self.draw(width, height, scale, model, model.grid(width as f32 / scale, height as f32 / scale))?;
        self.target.as_ref().unwrap().2.as_ref().unwrap().pixels()
    }

    pub fn paint(
        &mut self,
        target: &ID2D1DeviceContext,
        width: u32,
        height: u32,
        scale: f32,
        model: &GroupModel,
    ) -> Result<()> {
        if self
            .target
            .as_ref()
            .is_none_or(|(_, _, bitmap, old)| bitmap.is_some() || old != target)
        {
            self.images.clear();
            self.states.clear();
        }
        self.target = Some((width, height, None, target.clone()));
        let height_dip = height as f32 / scale;
        let grid = model.grid(width as f32 / scale, height_dip);
        // Keep the viewport plus one row for smooth scrolling, not every icon
        // ever visited in a large mapped folder. Source identity still detects
        // replaced icons without re-uploading unchanged visible textures.
        let candidates = model.visible_indices(grid, HEADER - grid.cell_height, height_dip + grid.cell_height);
        let live: HashSet<_> = candidates
            .map(|index| (index, &model.items[index]))
            .filter(|(index, _)| {
                let (_, y) = model.cell(grid, *index);
                height_dip > HEADER + 1.0
                    && y + grid.cell_height * 2.0 > HEADER
                    && y < height_dip + grid.cell_height
            })
            .filter_map(|(_, item)| {
                item.image
                    .as_ref()
                    .map(|image| Arc::as_ptr(image) as usize)
            })
            .collect();
        self.images
            .retain(|key, _| live.contains(key));
        self.draw(width, height, scale, model, grid)
    }

    fn layout_title(
        &mut self,
        text: &str,
        available: f32,
        scale: f32,
    ) -> Result<windows_canvas::TextLayout> {
        if self
            .title_layout
            .as_ref()
            .is_none_or(|cached| cached.text != text)
        {
            let layout = canvas_result(windows_canvas::TextLayout::new(
                text,
                &self.title,
                1_000_000.0,
                HEADER,
            ))?;
            self.title_layout = Some(TitleLayout {
                text: text.into(),
                natural_width: layout.metrics().width_including_trailing_whitespace,
                width_pixels: 0.0,
                scale,
                layout,
            });
        }
        let cached = self.title_layout.as_mut().unwrap();
        // Draw this same layout instead of reshaping with DrawText and a
        // fractional rectangle; leave one physical pixel around a full title.
        let available_pixels = (available * scale).floor().max(1.0);
        let pixels = available_pixels.min((cached.natural_width * scale).ceil() + 1.0);
        // Shrink immediately, but require four spare physical pixels before
        // growing again so size jitter cannot toggle a final glyph and ellipsis.
        // Use uncapped space here so even a fully visible title can recover.
        if cached.width_pixels == 0.0
            || cached.scale != scale
            || pixels < cached.width_pixels
            || available_pixels >= cached.width_pixels + 4.0
        {
            if cached.width_pixels != pixels || cached.scale != scale {
                cached.layout.set_max_size(pixels / scale, HEADER);
            }
            cached.width_pixels = pixels;
            cached.scale = scale;
        }
        Ok(cached.layout.clone())
    }

    #[allow(clippy::too_many_lines)]
    fn draw(&mut self, width: u32, height: u32, scale: f32, model: &GroupModel, grid: super::layout::Grid) -> Result<()> {
        if self.family != super::fonts::family() || self.language != crate::i18n::language() {
            let fresh = Self::new()?;
            self.family = fresh.family;
            self.language = fresh.language;
            self.labels = fresh.labels;
            self.title = fresh.title;
            self.tab_title = fresh.tab_title;
            self.details = fresh.details;
            self.menu_shortcut = fresh.menu_shortcut;
            self.column_label_widths = fresh.column_label_widths;
            self.title_layout = None;
            self.states.clear();
        }

        let mut live_states = HashSet::new();
        let w = width as f32 / scale;
        let (mut title_left, mut title_space) = super::layout::title_area(w);
        if model.folder.is_some() {
            title_space = (title_space - (74.0 - title_left).max(0.0)).max(1.0);
            title_left = title_left.max(74.0);
        }
        let show_icon = model.folder.is_some() && title_space >= 42.0;
        let icon_width = if model.folder.is_some() { 24.0 } else { 0.0 };
        let title = self.layout_title(&model.title, (title_space - icon_width).max(1.0), scale)?;
        let group_left =
            ((title_left + (title_space - title.max_size().0 - icon_width).max(0.0) / 2.0) * scale)
                .floor()
                / scale;
        {
            let (_, _, _, target) = self.target.as_ref().unwrap();
            let context = target;
            let mut brushes = self.brushes.borrow_mut();
            canvas::draw(target, scale, |target| {
                let (w, h) = (width as f32 / scale, height as f32 / scale);
                let contrast = super::theme::panel_contrast(
                    model.backdrop,
                    model.dark,
                    model.options.text,
                    model.native_material,
                );
                let opacity = if model.native_material {
                    if model.options.text_protection {
                        contrast.scrim
                    } else {
                        0.0
                    }
                } else {
                    1.0
                };
                let base = contrast.base();
                let ink = contrast.ink();
                let [background, outline, white, dim, placeholder_fill, placeholder_edge, selection, hover, inactive] =
                    canvas_result(brushes.get(context, &target, [
                        if !model.native_material { super::theme::mica_fallback(model.backdrop, model.dark) } else { None }
                            .unwrap_or(ColorF::new(base, base, base, opacity)),
                        super::theme::panel_border(model.dark, model.backdrop),
                        ColorF::new(ink, ink, ink, 1.0),
                        ColorF::new(ink, ink, ink, 0.85),
                        ColorF::new(ink, ink, ink, 0.08),
                        ColorF::new(ink, ink, ink, 0.48),
                        ColorF::new(0.55, 0.75, 1.0, 0.25),
                        ColorF::new(0.7, 0.85, 1.0, 0.12),
                        ColorF::new(0.75, 0.8, 0.85, 0.16),
                    ]))?;
                target.clear(ColorF::new(0.0, 0.0, 0.0, 0.0));
                let rounded = RoundedRect {
                    rect: Rect::from_xywh(0.5, 0.5, w - 1.0, h - 1.0),
                    radius_x: model.options.corner_radius,
                    radius_y: model.options.corner_radius,
                };
                {
                    target.fill_rounded_rect(&rounded, background);
                    if model.options.border {
                        // One physical pixel, fully inside the client bounds at every DPI.
                        let stroke = 1.0 / scale;
                        let inset = stroke * 0.5;
                        let radius = (model.options.corner_radius - inset).max(0.0);
                        target.draw_rounded_rect(&RoundedRect {
                            rect: Rect::from_xywh(inset, inset, w - stroke, h - stroke),
                            radius_x: radius,
                            radius_y: radius,
                        }, outline, stroke);
                    }
                    if show_icon && model.tabs.len() < 2 && model.merge_preview.is_empty() {
                        target.clipped_icon(
                            "\u{e8b7}",
                            &self.icons,
                            &Rect::from_xywh(group_left, 0.0, 18.0, HEADER),
                            white,
                        );
                    }
                    if model.tabs.len() < 2 && model.merge_preview.is_empty() {
                        target.clipped_color_layout(&model.title, &title, group_left + icon_width, canvas::text_ink_center_offset(&title)?, white)?;
                    }
                    for button in 0..if !model.merge_preview.is_empty() { 0 } else if model.folder.is_some() { 4 } else { 2 } {
                        let top = super::layout::HEADER_INSET;
                        let button_height = HEADER - top * 2.0;
                        let center_y = HEADER / 2.0;
                        if model.tabs.len() > 1 && button < 2 { continue; }
                        let x = model.header_button_x(w, button);
                        let enabled = model.header_button_enabled(button);
                        let hovered = enabled && model.hovered_button == Some(button);
                        let glyph = canvas_result(target.create_solid_brush(ColorF::new(
                            ink,
                            ink,
                            ink,
                            if !enabled { 0.3 } else if hovered { 1.0 } else { 0.85 },
                        )))?;
                        if hovered {
                            let fill = canvas_result(
                                target.create_solid_brush(ColorF::new(ink, ink, ink, 0.07)),
                            )?;
                            target.fill_rounded_rect(
                                &RoundedRect {
                                    rect: Rect::from_xywh(x + 1.0, top, 26.0, button_height),
                                    radius_x: 5.0,
                                    radius_y: 5.0,
                                },
                                &fill,
                            );
                        }
                        let center = x + 14.0;
                        if button >= 2 {
                            target.clipped_icon(
                                if button == 2 { "\u{e72b}" } else { "\u{e80f}" },
                                &self.navigation_icons,
                                &Rect::from_xywh(x, top, 28.0, button_height),
                                &glyph,
                            );
                        } else if button == 0 {
                            let amount = model.reveal.clamp(0.0, 1.0);
                            let points = [
                                (center - 2.0 - 2.0 * amount, 15.0 + 2.0 * amount),
                                (center + 2.0 - 2.0 * amount, 19.0 + 2.0 * amount),
                                (center - 2.0 + 6.0 * amount, 23.0 - 6.0 * amount),
                            ];
                            for pair in points.windows(2) {
                                target.draw_line(
                                    Vector2 {
                                        x: center + (pair[0].0 - center) * 1.25,
                                        y: center_y + (pair[0].1 - 19.0) * 1.25,
                                    },
                                    Vector2 {
                                        x: center + (pair[1].0 - center) * 1.25,
                                        y: center_y + (pair[1].1 - 19.0) * 1.25,
                                    },
                                    &glyph,
                                    1.5,
                                );
                            }
                        } else {
                            for offset in [-4.5, 0.0, 4.5] {
                                target.fill_ellipse(
                                    &Ellipse {
                                        center: Vector2 {
                                            x: center + offset,
                                            y: center_y,
                                        },
                                        radius_x: 1.1,
                                        radius_y: 1.1,
                                    },
                                    &glyph,
                                );
                            }
                        }
                    }
                }
                let mut chrome = super::theme::material_chrome(model.backdrop, model.dark);
                if !matches!(model.backdrop.base(), luciddesk_core::Backdrop::Acrylic | luciddesk_core::Backdrop::Mica | luciddesk_core::Backdrop::MicaAlt) {
                    chrome.tab_active = ColorF::new(ink, ink, ink, 0.14);
                    chrome.tab_inactive = ColorF::new(ink, ink, ink, 0.04);
                    chrome.tab_hover = ColorF::new(ink, ink, ink, 0.09);
                    chrome.tab_incoming = ColorF::new(ink, ink, ink, 0.09);
                }
                for (id, bounds) in super::tabs::strip(model, w).into_iter().filter(|_| model.merge_preview.is_empty()) {
                    let rect = Rect::from_xywh(bounds.x, bounds.y, bounds.width, bounds.height);
                    let fill = canvas_result(target.create_solid_brush(
                        if id == model.active_tab { chrome.tab_active } else if model.hovered_tab == Some(id) { chrome.tab_hover } else { chrome.tab_inactive }))?;
                    target.fill_rounded_rect(&RoundedRect { rect, radius_x: model.options.corner_radius, radius_y: model.options.corner_radius }, &fill);
                    if id == model.active_tab || model.hovered_tab == Some(id) {
                        let edge = canvas_result(target.create_solid_brush(chrome.card_border))?;
                        target.draw_rounded_rect(&RoundedRect { rect, radius_x: model.options.corner_radius, radius_y: model.options.corner_radius }, &edge, 1.0);
                    }
                    let text = model.tabs.iter().find(|(tab, _)| *tab == id).map_or("", |(_, title)| title.as_str());
                    target.clipped_color_text(text, &self.tab_title, &Rect::from_xywh(rect.left + 10.0, rect.top, (bounds.width - 20.0).max(1.0), bounds.height),
                        if id == model.active_tab { white } else { dim })?;
                }
                for (text, incoming, active, bounds) in super::tabs::merge_strip(model, w) {
                    let rect = Rect::from_xywh(bounds.x, bounds.y, bounds.width, bounds.height);
                    let fill = canvas_result(target.create_solid_brush(
                        if incoming { chrome.tab_incoming } else if active { chrome.tab_active } else { chrome.tab_inactive }))?;
                    target.fill_rounded_rect(&RoundedRect { rect, radius_x: 5.0, radius_y: 5.0 }, &fill);
                    if incoming {
                        let edge = canvas_result(target.create_solid_brush(ColorF::new(ink, ink, ink, 0.24 + chrome.border.a)))?;
                        // Short neutral dashes keep provisional tabs distinct without an accent color.
                        let mut x = rect.left + 5.0;
                        while x < rect.right - 5.0 {
                            for y in [rect.top + 0.5, rect.bottom - 0.5] {
                                target.draw_line(Vector2::new(x, y), Vector2::new((x + 4.0).min(rect.right - 5.0), y), &edge, 1.0);
                            }
                            x += 7.0;
                        }
                        let mut y = rect.top + 5.0;
                        while y < rect.bottom - 5.0 {
                            for x in [rect.left + 0.5, rect.right - 0.5] {
                                target.draw_line(Vector2::new(x, y), Vector2::new(x, (y + 4.0).min(rect.bottom - 5.0)), &edge, 1.0);
                            }
                            y += 7.0;
                        }
                    }
                    target.clipped_color_text(&text, &self.tab_title, &Rect::from_xywh(rect.left + 10.0, rect.top,
                        (bounds.width - 20.0).max(1.0), bounds.height), if active || incoming { white } else { dim })?;
                }
                if super::header_divider::enabled() && !model.collapsed && h > model.content_header() + 1.0 {
                    let divider = canvas_result(target.create_solid_brush(super::theme::panel_divider(model.dark, model.backdrop)))?;
                    let inset = super::layout::HEADER_INSET;
                    let divider_y = ((model.content_header() - inset + 4.0) * scale).round() / scale;
                    target.fill_rect(&Rect::from_xywh(inset, divider_y, (w - inset * 2.0).max(0.0), 1.0 / scale), &divider);
                }
                if h > model.content_header() + 1.0 {
                    target.push_clip(&{
                        Rect::from_xywh(6.0, model.content_header(), w - 12.0, (h - model.content_header() - 6.0).max(0.0))
                    });
                    let list = model.is_list();
                    let columns = model.list_columns(grid.cell_width);
                    if list && model.folder.is_some() {
                        for column in 1..4 {
                            if model.folder_visible_columns & (1 << column) == 0 { continue; }
                            let boundary = columns[column];
                            target.fill_rect(
                                &Rect::from_xywh(
                                    super::layout::PADDING + boundary,
                                    grid.content_top - super::layout::LIST_HEADER + 6.0,
                                    1.0, super::layout::LIST_HEADER - 12.0,
                                ),
                                hover,
                            );
                        }
                        for (column, name) in [crate::i18n::text("ui-name"), crate::i18n::text("ui-type"), crate::i18n::text("ui-modified"), crate::i18n::text("ui-size")].iter().enumerate()
                        {
                            if column > 0 && model.folder_visible_columns & (1 << column) == 0 { continue; }
                            let inset = if column == 0 { 0.0 } else { 8.0 };
                            let left = super::layout::PADDING + columns[column] + inset;
                            let top = grid.content_top - super::layout::LIST_HEADER;
                            let available = (columns[column + 1] - columns[column] - inset - 8.0).max(1.0);
                            let sorted = model.folder_sort.0 as usize == column && available >= 30.0;
                            let text_width = if sorted { available - 18.0 } else { available };
                            target.clipped_text(
                                name,
                                &self.details,
                                &Rect::from_xywh(
                                    left, top, text_width,
                                    super::layout::LIST_HEADER,
                                ),
                                dim,
                            );
                            if sorted {
                                target.clipped_icon(
                                    if model.folder_sort.1 { "\u{e70d}" } else { "\u{e70e}" },
                                    &self.sort_icons,
                                    &Rect::from_xywh(
                                        left + self.column_label_widths[column].min(text_width) + 4.0,
                                        top, 14.0, super::layout::LIST_HEADER,
                                    ),
                                    dim,
                                );
                            }
                        }
                        target.fill_rect(
                            &Rect::from_xywh(
                                super::layout::PADDING,
                                grid.content_top - 1.0,
                                grid.cell_width,
                                1.0,
                            ),
                            hover,
                        );
                    }
                    for index in model.visible_indices(grid, HEADER, h) {
                        let item = &model.items[index];
                        let (x, y) = model.cell(grid, index);
                        if list && y < grid.content_top {
                            continue;
                        }
                        if y + grid.cell_height <= { HEADER } || y >= h {
                            continue;
                        }
                        if model.selection.contains(&index) || model.hovered_item == Some(index) {
                            let bounds = model.selection_bounds(grid, index, scale);
                            let selection_height = bounds.height;
                            let selection_width = bounds.width;
                            let selection_x = bounds.x;
                            let state = if model.selection.contains(&index) {
                                if !model.focused {
                                    5
                                } else if model.hovered_item == Some(index) {
                                    6
                                } else {
                                    3
                                }
                            } else {
                                2
                            };
                            let key = (
                                (selection_width * scale).round() as u32,
                                (selection_height * scale).round() as u32,
                                (96.0 * scale).round() as u32,
                                state,
                            );
                            live_states.insert(key);
                            if !self.states.contains_key(&key)
                                && let Some(pixels) =
                                    super::theme::selection(key.0, key.1, key.2, key.3)
                            {
                                let bitmap = canvas_result(target.create_bitmap(
                                    &pixels.data,
                                    key.0,
                                    key.1,
                                ))?;
                                if self.states.len() >= 64 {
                                    if let Some(old) = self.states.keys().next().copied() {
                                        self.states.remove(&old);
                                    }
                                }
                                self.states.insert(key, bitmap);
                            }
                            if let Some(bitmap) = self.states.get(&key) {
                                target.draw_bitmap(
                                    bitmap,
                                    &Rect::from_xywh(
                                        selection_x,
                                        y,
                                        selection_width,
                                        selection_height,
                                    ),
                                    1.0,
                                );
                            } else {
                                target.fill_rounded_rect(
                                    &RoundedRect {
                                        rect: Rect::from_xywh(
                                            selection_x,
                                            y,
                                            selection_width,
                                            selection_height,
                                        ),
                                        radius_x: 0.0,
                                        radius_y: 0.0,
                                    },
                                    if model.selection.contains(&index) {
                                        if model.focused { selection } else { inactive }
                                    } else {
                                        hover
                                    },
                                );
                            }
                        }
                        if let Some(image) = &item.image {
                            let key = Arc::as_ptr(image) as usize;
                            let ratio =
                                grid.icon_size * scale / image.width.max(image.height) as f32;
                            let size = (
                                (image.width as f32 * ratio).round().max(1.0) as u32,
                                (image.height as f32 * ratio).round().max(1.0) as u32,
                            );
                            if self.images.get(&key).is_none_or(|(source, _, old_size)| {
                                !Arc::ptr_eq(source, image) || *old_size != size
                            }) {
                                let pixels = super::scaled_icons::resample(image, size.0, size.1)?;
                                let bitmap = canvas_result(target.create_bitmap(
                                    &pixels.data,
                                    pixels.width,
                                    pixels.height,
                                ))?;
                                self.images
                                    .insert(key, (Arc::clone(image), bitmap, size));
                                if luciddesk_diagnostics::enabled(luciddesk_diagnostics::Level::Trace)
                                    && item.identity.persistent_key()
                                        .to_ascii_lowercase()
                                        .contains("645ff040-5081-101b-9f08-00aa002f954e")
                                {
                                    luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Trace, "pane.render",
                                        "recycle-render-upload hash={:x} size={:?}",
                                        image.data.iter().fold(0u64, |h, b| h
                                            .wrapping_mul(31)
                                            .wrapping_add(u64::from(*b))),
                                        size
                                    );
                                }
                            }
                            let (iw, ih) = (size.0 as f32 / scale, size.1 as f32 / scale);
                            let left = ((if list {
                                x + 4.0
                            } else {
                                x + (grid.cell_width - iw) / 2.0
                            }) * scale)
                                .round()
                                / scale;
                            let top = ((if list {
                                y + (grid.cell_height - ih) / 2.0
                            } else {
                                y + 2.0 + (grid.icon_size - ih) / 2.0
                            }) * scale)
                                .round()
                                / scale;
                            target.draw_bitmap(
                                &self.images[&key].1,
                                &Rect::from_xywh(left, top, iw, ih),
                                1.0,
                            );
                        } else {
                            let left = if list { x + 4.0 } else { x + (grid.cell_width - grid.icon_size) / 2.0 };
                            let top = if list { y + (grid.cell_height - grid.icon_size) / 2.0 } else { y + 2.0 };
                            draw_placeholder(&target,
                                Rect::from_xywh((left * scale).round() / scale, (top * scale).round() / scale,
                                    grid.icon_size, grid.icon_size),
                                item.details.folder, placeholder_fill, placeholder_edge);

                        }

                        if list {
                            let size = super::folder::size_text(item.details.size, item.details.folder);
                            for (column, text) in
                                [item.label.as_str(), item.details.kind_text(), item.details.modified.as_str(), size.as_str()]
                                    .iter()
                                    .enumerate()
                            {
                                if column > 0 && model.folder.is_none() {
                                    continue;
                                }
                                if column > 0 && model.folder_visible_columns & (1 << column) == 0 { continue; }
                                if column == 0 && model.renaming.as_ref() == Some(&item.identity) {
                                    continue;
                                }
                                let inset = if column == 0 { 0.0 } else { 8.0 };
                                target.clipped_text(
                                    text,
                                    if column == 3 { &self.menu_shortcut } else { &self.details },
                                    &Rect::from_xywh(
                                        x + columns[column] + inset,
                                        y,
                                        (columns[column + 1] - columns[column] - inset - 8.0).max(1.0),
                                        grid.cell_height,
                                    ),
                                    if column == 0 { white } else { dim },
                                );
                            }
                            continue;
                        }
                        if model.renaming.as_ref() == Some(&item.identity) {
                            continue;
                        }
                        let (layout, _) = super::label::layout_scaled(
                            &item.label,
                            (grid.cell_width * scale).round() as u32,
                            (96.0 * scale).round() as u32,
                            2,
                            grid.text_scale,
                        )?;
                        target.clipped_layout(
                            &layout,
                            (x * scale).round() / scale + 2.0,
                            ((y + grid.icon_size + crate::pane::layout::LABEL_OFFSET) * scale)
                                .round()
                                / scale,
                            white,
                        );
                        continue;
                    }
                    if model.items.is_empty() {
                        let text = if let Some(status) = &model.folder_status {
                            status.as_str()
                        } else if model.folder.is_some() {
                            if model.loading {
                                crate::i18n::text("ui-loading-folder")
                            } else {
                                crate::i18n::text("ui-this-folder-is-empty")
                            }
                        } else if model.loading {
                            crate::i18n::text("ui-loading-desktop-items")
                        } else {
                            crate::i18n::text("ui-drag-icons-into-this-group")
                        };
                        target.clipped_text(
                            text,
                            &self.labels,
                            &Rect::from_xywh(20.0, 0.0, (w - 40.0).max(1.0), h),
                            dim,
                        );
                    }
                    target.pop_clip();
                    if let Some(bar) = super::scrollbar::Bar::for_grid(model, grid, w, h) {
                        let expansion = model.scrollbar.expansion.clamp(0.0, 1.0);
                        let center = bar.left + super::scrollbar::Bar::WIDTH / 2.0;
                        if expansion > 0.0 {
                            let track_width = 2.0 + 4.0 * expansion;
                            let track = canvas_result(target.create_solid_brush(ColorF::new(ink, ink, ink, 0.06 * expansion)))?;
                            target.fill_rounded_rect(
                                &RoundedRect {
                                    rect: Rect::from_xywh(center - track_width / 2.0, bar.top, track_width, bar.height),
                                    radius_x: track_width / 2.0, radius_y: track_width / 2.0,
                                }, &track,
                            );
                        }
                        let width = 2.0 + 2.0 * expansion;
                        let thumb = canvas_result(target.create_solid_brush(ColorF::new(ink, ink, ink, 0.85 + 0.15 * expansion)))?;
                        target.fill_rounded_rect(
                            &RoundedRect {
                                rect: Rect::from_xywh(
                                    center - width / 2.0,
                                    bar.thumb_top, width, bar.thumb_height,
                                ),
                                radius_x: width / 2.0,
                                radius_y: width / 2.0,
                            },
                            &thumb,
                        );
                    }
                }
                if let Some(rect) = self.marquee.filter(|_| !model.collapsed) {
                    let bounds = Rect::from_xywh(rect.x, rect.y, rect.width, rect.height);
                    let fill = canvas_result(target.create_solid_brush(ColorF::new(0.2, 0.55, 1.0, 0.18)))?;
                    let border = canvas_result(target.create_solid_brush(ColorF::new(0.3, 0.65, 1.0, 0.9)))?;
                    target.fill_rect(&bounds, &fill);
                    target.draw_rect(&bounds, &border, 1.0 / scale);
                }
                let result = target.finish();
                // Retain only backgrounds used by this frame, including after resize,
                // DPI changes, deselection, or collapsing the content.
                self.states.retain(|key, _| live_states.contains(key));
                result
            })
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod bench;

pub(crate) mod debug;
