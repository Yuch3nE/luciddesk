//! Cached Canvas content. Live windows draw directly into the GPU swap chain.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use windows_canvas::{ColorF, Ellipse, Rect, RoundedRect, Vector2};

use super::native_graphics::canvas_result;
use super::{GroupModel, assets, canvas, layout::HEADER};
use luciddesk_window::controls::motion::Highlights;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};

use windows_canvas::ID2D1DeviceContext;

use windows::core::Result;
type ImageBitmap = (Weak<assets::Pixels>, windows_canvas::Bitmap, (u32, u32));
const IMAGE_UPLOADS_PER_FRAME: usize = 8;

#[derive(PartialEq)]
struct ViewportKey {
    width: u32,
    height: u32,
    scale: f32,
    scroll: usize,
    scroll_x: f32,
    list: bool,
    folder: bool,
    fixed_grid: bool,
    free_layout: bool,
    icon_size: f32,
    grid_scale: f32,
    icon_display: luciddesk_core::IconDisplay,
    items: (usize, usize, u64),
    image_revision: u64,
    tab: luciddesk_core::PanelId,
    font: (u64, u64, &'static str),
}

// A quiet, font-independent silhouette fills the same slot as the real icon.
// It also remains useful when Shell extraction fails; no perpetual loading animation.
fn draw_placeholder(
    target: &canvas::DrawPass<'_>,
    slot: Rect,
    folder: bool,
    fill: &windows_canvas::Brush,
    edge: &windows_canvas::Brush,
) {
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
            target.draw_line(
                Vector2::new(x + size * 0.34, y + size * top),
                Vector2::new(x + size * (0.34 + width), y + size * top),
                edge,
                stroke,
            );
        }
    }
}

struct TitleLayout {
    text: String,
    natural_width: f32,
    width_pixels: f32,
    scale: f32,
    layout: windows_canvas::TextLayout,
    prepared: Option<(canvas::ColorLayout, f32)>,
}

pub struct Renderer {
    frame_cache: std::cell::RefCell<canvas::FrameCache>,
    monitor: Option<super::monitor::Drawing>,
    family: String,
    font_revision: u64,
    language: &'static str,
    #[cfg(test)]
    offscreen_device: Option<windows_canvas::GpuDevice>,
    labels: windows_canvas::TextFormat,
    title: windows_canvas::TextFormat,
    menu_label: windows_canvas::TextFormat,
    title_layout: Option<TitleLayout>,
    pub(super) marquee: Option<luciddesk_core::RectDip>,
    details: windows_canvas::TextFormat,
    tab_title: windows_canvas::TextFormat,
    tab_title_active: windows_canvas::TextFormat,
    column_label_widths: [f32; 4],
    sort_icons: windows_canvas::TextFormat,
    menu_shortcut: windows_canvas::TextFormat,
    icons: windows_canvas::TextFormat,
    navigation_icons: windows_canvas::TextFormat,
    empty_icon: windows_canvas::TextFormat,
    target: Option<(u32, u32, Option<canvas::Offscreen>, ID2D1DeviceContext)>,
    brushes: std::cell::RefCell<canvas::Brushes<15>>,
    images: HashMap<usize, ImageBitmap>,
    viewport: Option<(ViewportKey, super::layout::Grid)>,
    visible_items: Vec<(usize, f32, f32)>,
    live_images: HashSet<usize>,
    buffered_images: Vec<usize>,
    size_labels: HashMap<u64, String>,
    #[cfg(test)]
    image_uploads: usize,
    buttons: Highlights<usize>,
    columns: Highlights<usize>,
    tabs: Highlights<luciddesk_core::PanelId>,
    items: Highlights<luciddesk_core::ShellIdentity>,
}

impl Renderer {
    pub(super) fn release_device_resources(&mut self) {
        self.images.clear();
        self.viewport = None;
        self.target = None;
        *self.brushes.get_mut() = Default::default();
        *self.frame_cache.get_mut() = Default::default();
        self.monitor = None;
    }

    pub(super) fn menu_width(&self, rows: &[super::menu::Entry]) -> f32 {
        rows.iter()
            .filter(|row| row.id != 0)
            .fold(216.0_f32, |width, row| {
                let trailing = row.trailing_width();
                let measured =
                    windows_canvas::TextLayout::new(&row.label, &self.menu_label, 4096.0, 64.0)
                        .map(|layout| layout.metrics().width_including_trailing_whitespace)
                        .unwrap_or(166.0);
                width.max((measured + 54.0 + trailing).ceil())
            })
            .min(480.0)
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
        canvas::draw_cached(target, scale, &self.frame_cache, |target| {
            use luciddesk_window::control_style::{self as style, Palette};
            let rgba = |[r, g, b, a]: [f32; 4]| ColorF::new(r, g, b, a);
            let palette = Palette::for_theme(dark);
            let text = canvas_result(target.cached_solid_brush(rgba(Palette::rgba(palette.ink, 1.0))))?;
            let disabled =
                canvas_result(target.cached_solid_brush(rgba(Palette::rgba(palette.disabled, 1.0))))?;
            let border =
                canvas_result(target.cached_solid_brush(rgba(palette.flyout_border())))?;
            let divider =
                canvas_result(target.cached_solid_brush(rgba(palette.divider())))?;
            target.clear(ColorF::new(0.0, 0.0, 0.0, 0.0));
            let width = width as f32 / scale;
            if !native {
                let background =
                    canvas_result(target.cached_solid_brush(rgba(Palette::rgba(palette.flyout, 1.0))))?;
                target.fill_rounded_rect(
                    &RoundedRect {
                        rect: Rect::from_xywh(0.0, 0.0, width, height as f32 / scale),
                        radius_x: super::menu::CORNER_RADIUS + 0.5,
                        radius_y: super::menu::CORNER_RADIUS + 0.5,
                    },
                    &*background,
                );
            }
            target.draw_rounded_rect(
                &RoundedRect {
                    rect: Rect::from_xywh(0.5, 0.5, width - 1.0, height as f32 / scale - 1.0),
                    radius_x: super::menu::CORNER_RADIUS,
                    radius_y: super::menu::CORNER_RADIUS,
                },
                &*border,
                1.0,
            );
            for (index, row) in rows.iter().enumerate() {
                let top = super::menu::row_top(rows, index);
                if row.id == 0 {
                    target.fill_rect(
                        &Rect::from_xywh(12.0, top + 4.0, width - 24.0, 1.0),
                        &*divider,
                    );
                    continue;
                }
                let progress = highlights.get(index).copied().unwrap_or(0.0);
                if progress > 0.0 {
                    let hover = canvas_result(
                        target.cached_solid_brush(rgba(palette.flyout_hover(progress))),
                    )?;
                    target.fill_rounded_rect(
                        &RoundedRect {
                            rect: Rect::from_xywh(
                                4.0,
                                top + 2.0,
                                width - 8.0,
                                super::menu::ROW_HEIGHT - 4.0,
                            ),
                            radius_x: style::RADIUS,
                            radius_y: style::RADIUS,
                        },
                        &*hover,
                    );
                }
                let text = if row.enabled { &text } else { &disabled };
                let trailing_width = row.trailing_width();
                for (label, left, available) in [
                    (row.icon, 12.0, 20.0),
                    (row.label.as_ref(), 38.0, width - 50.0 - trailing_width),
                ] {
                    if label
                        .chars()
                        .next()
                        .is_some_and(|ch| ('\u{e000}'..='\u{f8ff}').contains(&ch))
                    {
                        target.clipped_icon(
                            label,
                            &self.icons,
                            &Rect::from_xywh(left, top, available, super::menu::ROW_HEIGHT),
                            &text,
                        );
                        continue;
                    }
                    target.clipped_cached_text(
                        label,
                        &self.menu_label,
                        &Rect::from_xywh(left, top, available, super::menu::ROW_HEIGHT),
                        &text,
                    );
                }
                if !row.children.is_empty() {
                    target.clipped_icon(
                        "\u{e76c}",
                        &self.icons,
                        &Rect::from_xywh(width - 28.0, top, 16.0, super::menu::ROW_HEIGHT),
                        &text,
                    );
                } else if row.trailing_is_icon() {
                    target.clipped_icon(
                        row.trailing,
                        &self.icons,
                        &Rect::from_xywh(width - 28.0, top, 16.0, super::menu::ROW_HEIGHT),
                        &text,
                    );
                } else if trailing_width > 0.0 {
                    target.clipped_cached_text(
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
        use windows_canvas::{FontWeight, ParagraphAlignment, TextAlignment, TextFormat, WordWrapping};
        let (family, size) = assets::font();
        let labels = canvas_result(TextFormat::new(&family, size))?
            .with_alignment(TextAlignment::Center)
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(WordWrapping::Wrap);
        canvas::ellipsis(&labels)?;
        let title = canvas_result(TextFormat::with_weight(&family, size + 1.0, FontWeight(600)))?
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(WordWrapping::NoWrap);
        canvas::ellipsis(&title)?;
        let menu_label = canvas_result(TextFormat::new(&family, size + 1.0))?
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(WordWrapping::NoWrap);
        canvas::ellipsis(&menu_label)?;
        let details = canvas_result(TextFormat::new(&family, 12.0))?
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(WordWrapping::NoWrap);
        canvas::ellipsis(&details)?;
        let tab_title = canvas_result(TextFormat::new(&family, 12.0))?
            .with_alignment(TextAlignment::Center)
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(WordWrapping::NoWrap);
        canvas::ellipsis(&tab_title)?;
        let tab_title_active = canvas_result(TextFormat::with_weight(&family, 12.0, FontWeight(600)))?
            .with_alignment(TextAlignment::Center)
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(WordWrapping::NoWrap);
        canvas::ellipsis(&tab_title_active)?;
        let mut column_label_widths = [0.0; 4];
        for (index, name) in [
            crate::i18n::text("ui-name"),
            crate::i18n::text("ui-type"),
            crate::i18n::text("ui-modified"),
            crate::i18n::text("ui-size"),
        ]
        .iter()
        .enumerate()
        {
            column_label_widths[index] = canvas_result(windows_canvas::TextLayout::new(
                name,
                &details,
                256.0,
                super::layout::LIST_HEADER,
            ))?
            .metrics()
            .width_including_trailing_whitespace;
        }
        Ok(Self {
            buttons: Highlights::default(),
            columns: Highlights::default(),
            tabs: Highlights::default(),
            items: Highlights::default(),
            monitor: None,
            family: family.clone(),
            font_revision: assets::font_revision(),
            language: crate::i18n::language(),
            #[cfg(test)]
            offscreen_device: None,
            labels,
            title,
            menu_label,
            title_layout: None,
            marquee: None,
            details,
            tab_title,
            tab_title_active,
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
            empty_icon: canvas_result(TextFormat::new(super::fonts::icon_family(), 28.0))?
                .with_alignment(TextAlignment::Center)
                .with_paragraph_alignment(ParagraphAlignment::Center)
                .with_word_wrapping(WordWrapping::NoWrap),
            target: None,
            frame_cache: Default::default(),
            brushes: Default::default(),
            images: HashMap::new(),
            viewport: None,
            visible_items: Vec::new(),
            live_images: HashSet::new(),
            buffered_images: Vec::new(),
            size_labels: HashMap::new(),
            #[cfg(test)]
            image_uploads: 0,
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
            self.viewport = None;
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
        let grid = self.sync_viewport(width, height, scale, model);
        self.draw(width, height, scale, model, grid, false, usize::MAX)?;
        self.target.as_ref().unwrap().2.as_ref().unwrap().pixels()
    }

    #[cfg(test)]
    pub fn paint(
        &mut self, target: &ID2D1DeviceContext, width: u32, height: u32, scale: f32, model: &GroupModel,
    ) -> Result<()> {
        self.paint_to(target, width, height, scale, model, false, usize::MAX).map(|_| ())
    }

    pub(super) fn paint_animated(
        &mut self,
        target: &ID2D1DeviceContext,
        width: u32,
        height: u32,
        scale: f32,
        model: &GroupModel,
        animate: bool,
    ) -> Result<bool> {
        self.paint_to(target, width, height, scale, model, animate, IMAGE_UPLOADS_PER_FRAME)
    }

    fn paint_to(
        &mut self,
        target: &ID2D1DeviceContext,
        width: u32,
        height: u32,
        scale: f32,
        model: &GroupModel,
        animate: bool,
        upload_budget: usize,
    ) -> Result<bool> {
        if self
            .target
            .as_ref()
            .is_none_or(|(_, _, bitmap, old)| bitmap.is_some() || old != target)
        {
            self.images.clear();
            self.viewport = None;
        }
        self.target = Some((width, height, None, target.clone()));
        let grid = self.sync_viewport(width, height, scale, model);
        self.draw(width, height, scale, model, grid, animate, upload_budget)
    }

    fn sync_viewport(&mut self, width: u32, height: u32, scale: f32, model: &GroupModel) -> super::layout::Grid {
        let list = model.is_list();
        let key = ViewportKey {
            width,
            height,
            scale,
            scroll: model.scroll,
            scroll_x: model.scroll_x,
            list,
            folder: model.folder.is_some(),
            fixed_grid: model.fixed_grid,
            free_layout: model.free_layout,
            icon_size: model.icon_size,
            grid_scale: model.options.grid_scale,
            icon_display: model.icon_display,
            items: (model.items.as_ptr() as usize, model.items.len(), model.items_revision),
            image_revision: model.image_revision,
            tab: model.active_tab,
            font: (super::fonts::revision(), assets::font_revision(), crate::i18n::default_font()),
        };
        if let Some((previous, grid)) = &self.viewport && *previous == key {
            return *grid;
        }
        let width_dip = width as f32 / scale;
        let height_dip = height as f32 / scale;
        let grid = model.grid(width_dip, height_dip);
        self.viewport = Some((key, grid));
        self.visible_items.clear();
        self.live_images.clear();
        self.buffered_images.clear();
        if height_dip <= HEADER + 1.0 {
            self.images.clear();
            return grid;
        }
        // Retain already-uploaded textures one cell beyond the viewport. Do not
        // pre-upload hidden icons; cap the spare cache even for overlapping items
        // in free layouts, while never evicting a currently visible texture.
        const MAX_BUFFERED_IMAGES: usize = 64;
        for index in model.visible_indices(grid, HEADER, height_dip) {
            let (x, y) = model.cell(grid, index);
            if (!list || y >= grid.content_top) && y + grid.cell_height > HEADER && y < height_dip
                && x + grid.cell_width > 6.0 && x < width_dip - 6.0
            {
                self.visible_items.push((index, x, y));
            }
        }
        for index in model.visible_indices(grid, HEADER - grid.cell_height, height_dip + grid.cell_height) {
            let (x, y) = model.cell(grid, index);
            if y + grid.cell_height * 2.0 <= HEADER
                || y >= height_dip + grid.cell_height
                || x + grid.cell_width <= 6.0 - grid.cell_width
                || x >= width_dip - 6.0 + grid.cell_width
            {
                continue;
            }
            let Some(image) = &model.items[index].image else { continue };
            let key = Arc::as_ptr(image) as usize;
            if x + grid.cell_width > 6.0 && x < width_dip - 6.0
                && y + grid.cell_height > HEADER && y < height_dip
            {
                self.live_images.insert(key);
            } else if self.images.contains_key(&key) {
                self.buffered_images.push(key);
            }
        }
        let mut retained = 0;
        for &key in &self.buffered_images {
            if retained == MAX_BUFFERED_IMAGES { break; }
            if self.live_images.insert(key) { retained += 1; }
        }
        self.images.retain(|key, _| self.live_images.contains(key));
        grid
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
                prepared: None,
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
                cached.prepared = None;
            }
            cached.width_pixels = pixels;
            cached.scale = scale;
        }
        Ok(cached.layout.clone())
    }

    #[allow(clippy::too_many_lines)]
    fn draw(
        &mut self,
        width: u32,
        height: u32,
        scale: f32,
        model: &GroupModel,
        grid: super::layout::Grid,
        animate: bool,
        mut upload_budget: usize,
    ) -> Result<bool> {
        if model.monitor.is_some() && self.monitor.is_none() {
            self.monitor = Some(super::monitor::Drawing::new()?);
        }
        if self.font_revision != assets::font_revision()
            || self.family != super::fonts::family()
            || self.language != crate::i18n::language()
        {
            let fresh = Self::new()?;
            self.family = fresh.family;
            self.font_revision = fresh.font_revision;
            self.language = fresh.language;
            self.labels = fresh.labels;
            self.title = fresh.title;
            self.menu_label = fresh.menu_label;
            self.tab_title = fresh.tab_title;
            self.tab_title_active = fresh.tab_title_active;
            self.details = fresh.details;
            self.menu_shortcut = fresh.menu_shortcut;
            self.empty_icon = fresh.empty_icon;
            self.column_label_widths = fresh.column_label_widths;
            self.title_layout = None;
            self.frame_cache.borrow_mut().clear_text();
        }

        let now = std::time::Instant::now();
        let mut playing = false;
        let w = width as f32 / scale;
        let (mut title_left, mut title_space) = super::layout::title_area(w);
        if model.folder.is_some() {
            title_space = (title_space - (74.0 - title_left).max(0.0)).max(1.0);
            title_left = title_left.max(74.0);
        }
        let show_icon = model.folder.is_some() && title_space >= 42.0;
        let icon_width = if model.folder.is_some() { 24.0 } else { 0.0 };
        let title = self.layout_title(&model.title, (title_space - icon_width).max(1.0), scale)?;
        let prepared_title = if model.tabs.len() < 2 && model.merge_preview.is_empty() {
            let cached = self.title_layout.as_mut().unwrap();
            if cached.prepared.is_none() {
                cached.prepared = Some((
                    canvas::ColorLayout::new(&cached.text, &cached.layout, scale)?,
                    canvas::text_ink_center_offset(&cached.layout)?,
                ));
            }
            cached.prepared.as_ref()
        } else {
            None
        };
        let group_left =
            ((title_left + (title_space - title.max_size().0 - icon_width).max(0.0) / 2.0) * scale)
                .floor()
                / scale;
        {
            let (_, _, _, target) = self.target.as_ref().unwrap();
            let context = target;
            let mut brushes = self.brushes.borrow_mut();
            canvas::draw_cached(target, scale, &self.frame_cache, |target| {
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
                let [
                    background,
                    outline,
                    white,
                    dim,
                    placeholder_fill,
                    placeholder_edge,
                    selection,
                    hover,
                    inactive,
                    item_hover,
                    item_selected,
                    item_inactive,
                    item_hot_selected,
                    item_border,
                    focus_ring,
                ] = canvas_result(
                    brushes.get(
                        context,
                        &target,
                        [
                            if !model.native_material {
                                super::theme::mica_fallback(model.backdrop, model.dark)
                            } else {
                                None
                            }
                            .unwrap_or_else(|| {
                                if model.monitor.is_some()
                                    && !model.native_material
                                    && matches!(
                                        model.backdrop,
                                        luciddesk_core::Backdrop::Translucent { .. }
                                    )
                                {
                                    if model.dark {
                                        ColorF::new(0.075, 0.082, 0.10, opacity)
                                    } else {
                                        ColorF::new(0.985, 0.987, 0.99, opacity)
                                    }
                                } else {
                                    ColorF::new(base, base, base, opacity)
                                }
                            }),
                            super::theme::panel_border(model.backdrop),
                            ColorF::new(ink, ink, ink, 1.0),
                            ColorF::new(ink, ink, ink, 0.85),
                            ColorF::new(ink, ink, ink, 0.08),
                            ColorF::new(ink, ink, ink, 0.48),
                            ColorF::new(0.55, 0.75, 1.0, 0.25),
                            ColorF::new(0.7, 0.85, 1.0, 0.12),
                            ColorF::new(0.75, 0.8, 0.85, 0.16),
                            // Adapt item highlight tint to light/dark contrast while keeping
                            // the established neutral dark-mode alpha steps intact.
                            ColorF::new(
                                ink,
                                ink,
                                ink,
                                if contrast.light_text { 26.0 } else { 16.0 } / 255.0,
                            ),
                            ColorF::new(
                                ink,
                                ink,
                                ink,
                                if contrast.light_text { 54.0 } else { 32.0 } / 255.0,
                            ),
                            ColorF::new(
                                ink,
                                ink,
                                ink,
                                if contrast.light_text { 43.0 } else { 22.0 } / 255.0,
                            ),
                            ColorF::new(
                                ink,
                                ink,
                                ink,
                                if contrast.light_text { 61.0 } else { 40.0 } / 255.0,
                            ),
                            ColorF::new(ink, ink, ink, 0.14),
                            ColorF::new(
                                ink,
                                ink,
                                ink,
                                if contrast.light_text { 0.72 } else { 0.65 },
                            ),
                        ],
                    ),
                )?;
                target.clear(ColorF::new(0.0, 0.0, 0.0, 0.0));
                let rounded = RoundedRect {
                    rect: Rect::from_xywh(0.5, 0.5, w - 1.0, h - 1.0),
                    radius_x: crate::pane::interface_flags::pane_corner_radius(model.options.corner_radius),
                    radius_y: crate::pane::interface_flags::pane_corner_radius(model.options.corner_radius),
                };
                {
                    target.fill_rounded_rect(&rounded, background);
                    if model.options.border && !crate::pane::interface_flags::PANEL_SHADOW.get() {
                        // One physical pixel, fully inside the client bounds at every DPI.
                        let stroke = 1.0 / scale;
                        let inset = stroke * 0.5;
                        let radius = (crate::pane::interface_flags::pane_corner_radius(model.options.corner_radius) - inset).max(0.0);
                        target.draw_rounded_rect(
                            &RoundedRect {
                                rect: Rect::from_xywh(inset, inset, w - stroke, h - stroke),
                                radius_x: radius,
                                radius_y: radius,
                            },
                            outline,
                            stroke,
                        );
                    }
                    if show_icon && model.tabs.len() < 2 && model.merge_preview.is_empty() {
                        target.clipped_icon(
                            "\u{e8b7}",
                            &self.icons,
                            &Rect::from_xywh(group_left, 0.0, 18.0, HEADER),
                            white,
                        );
                    }
                    if let Some((layout, offset)) = prepared_title {
                        target.draw_color_layout(layout, group_left + icon_width, *offset, white)?;
                    }
                    for button in 0..if !model.merge_preview.is_empty() {
                        0
                    } else if model.folder.is_some() {
                        4
                    } else {
                        2
                    } {
                        let top = super::layout::HEADER_INSET;
                        let button_height = HEADER - top * 2.0;
                        let center_y = HEADER / 2.0;
                        if !model.show_header_actions() && button < 2 {
                            continue;
                        }
                        let x = model.header_button_x(w, button);
                        let enabled = model.header_button_enabled(button);
                        let feedback = self.buttons.sample(&button, enabled && model.hovered_button == Some(button),
                            false, enabled && model.pressed_button == Some(button), now, animate && enabled);
                        playing |= feedback.playing;
                        let glyph = canvas_result(target.cached_solid_brush(ColorF::new(
                            ink,
                            ink,
                            ink,
                            if !enabled {
                                0.3
                            } else {
                                (0.85 + 0.15 * feedback.hover - 0.22 * feedback.selected).clamp(0.45, 1.0)
                            },
                        )))?;
                        let pill_alpha = 0.07 * feedback.hover + 0.06 * feedback.selected;
                        if pill_alpha > 0.0 {
                            let fill = canvas_result(
                                target.cached_solid_brush(ColorF::new(ink, ink, ink, pill_alpha)),
                            )?;
                            target.fill_rounded_rect(
                                &RoundedRect {
                                    rect: Rect::from_xywh(x + 1.0, top, 26.0, button_height),
                                    radius_x: 5.0,
                                    radius_y: 5.0,
                                },
                                &*fill,
                            );
                        }
                        let center = x + 14.0;
                        if button >= 2 {
                            target.clipped_icon(
                                if button == 2 { "\u{e72b}" } else { "\u{e80f}" },
                                &self.navigation_icons,
                                &Rect::from_xywh(x, top, 28.0, button_height),
                                &*glyph,
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
                                    &*glyph,
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
                                    &*glyph,
                                );
                            }
                        }
                    }
                }
                let chrome = super::theme::material_chrome(model.backdrop, model.dark);
                let visible_tabs = if model.merge_preview.is_empty() {
                    super::tabs::strip(model, w)
                } else {
                    Vec::new()
                };
                for &(id, bounds) in &visible_tabs {
                    let rect = Rect::from_xywh(bounds.x, bounds.y, bounds.width, bounds.height);
                    let active = id == model.active_tab;
                    let feedback = self.tabs.sample(&id, model.hovered_tab == Some(id), false, active, now, animate);
                    playing |= feedback.playing;
                    let radius = crate::pane::interface_flags::pane_corner_radius(model.options.corner_radius)
                        .min(6.0).min(bounds.width * 0.5);
                    let shape = RoundedRect { rect, radius_x: radius, radius_y: radius };
                    let hover = feedback.hover * (1.0 - feedback.selected);
                    if hover > 0.0 || feedback.selected > 0.0 {
                        let blend = |active: f32, hovered: f32| {
                            let strength = feedback.selected + hover;
                            (active * feedback.selected + hovered * hover) / strength
                        };
                        let fill = canvas_result(target.cached_solid_brush(ColorF::new(
                            blend(chrome.tab_active.r, chrome.tab_hover.r),
                            blend(chrome.tab_active.g, chrome.tab_hover.g),
                            blend(chrome.tab_active.b, chrome.tab_hover.b),
                            chrome.tab_active.a * feedback.selected + chrome.tab_hover.a * hover,
                        )))?;
                        target.fill_rounded_rect(&shape, &*fill);
                    }
                    if feedback.selected > 0.0 {
                        let indicator_w = (bounds.width * 0.36)
                            .clamp(12.0, 24.0)
                            .min((bounds.width - 8.0).max(0.0));
                        if indicator_w >= 6.0 {
                            let indicator = canvas_result(target.cached_solid_brush(ColorF::new(
                                ink,
                                ink,
                                ink,
                                0.65 * feedback.selected,
                            )))?;
                            let ix = ((bounds.x + (bounds.width - indicator_w) * 0.5) * scale).round() / scale;
                            let iy = ((rect.bottom - 3.0) * scale).round() / scale;
                            target.fill_rounded_rect(
                                &RoundedRect {
                                    rect: Rect::from_xywh(ix, iy, indicator_w, 2.0),
                                    radius_x: 1.0,
                                    radius_y: 1.0,
                                },
                                &*indicator,
                            );
                        }
                    }
                    let text = model
                        .tabs
                        .iter()
                        .find(|(tab, _)| *tab == id)
                        .map_or("", |(_, title)| title.as_str());
                    let padding = 8.0_f32.min(bounds.width * 0.15);
                    let tab_brush = if active {
                        None
                    } else {
                        Some(canvas_result(target.cached_solid_brush(ColorF::new(
                            ink,
                            ink,
                            ink,
                            0.62 + 0.23 * feedback.hover,
                        )))?)
                    };
                    target.clipped_color_text(
                        text,
                        if active { &self.tab_title_active } else { &self.tab_title },
                        &Rect::from_xywh(
                            rect.left + padding,
                            rect.top,
                            bounds.width - padding * 2.0,
                            bounds.height,
                        ),
                        tab_brush.as_deref().unwrap_or(white),
                    )?;
                }
                if !visible_tabs.is_empty() {
                    let (overflow_left, overflow_right) = super::tabs::overflow(model, w);
                    if overflow_left || overflow_right {
                        let chevron = canvas_result(target.cached_solid_brush(ColorF::new(
                            ink, ink, ink, 0.48,
                        )))?;
                        let top = super::layout::HEADER_INSET;
                        let height = HEADER - top * 2.0;
                        if overflow_left {
                            target.clipped_icon(
                                "\u{e76b}",
                                &self.sort_icons,
                                &Rect::from_xywh(0.0, top, 10.0, height),
                                &*chevron,
                            );
                        }
                        if overflow_right {
                            let right_edge = visible_tabs.last().unwrap().1.x + visible_tabs.last().unwrap().1.width;
                            target.clipped_icon(
                                "\u{e76c}",
                                &self.sort_icons,
                                &Rect::from_xywh((right_edge - 2.0).min(w - 10.0), top, 10.0, height),
                                &*chevron,
                            );
                        }
                    }
                }
                for (text, incoming, active, bounds) in super::tabs::merge_strip(model, w) {
                    // Keep the preview aligned with real tabs; inset the one-pixel
                    // outline so it remains crisp and inside the header at every DPI.
                    let stroke = 1.0 / scale;
                    let inset = stroke * 0.5;
                    let rect = Rect::from_xywh(bounds.x + inset, bounds.y + inset,
                        (bounds.width - stroke).max(1.0), (bounds.height - stroke).max(1.0));
                    let radius = crate::pane::interface_flags::pane_corner_radius(model.options.corner_radius)
                        .min(rect.height() * 0.5);
                    let shape = RoundedRect { rect, radius_x: radius, radius_y: radius };
                    let fill = canvas_result(target.cached_solid_brush(if incoming {
                        chrome.tab_incoming
                    } else if active {
                        chrome.tab_active
                    } else {
                        chrome.tab_inactive
                    }))?;
                    target.fill_rounded_rect(&shape, &*fill);
                    if incoming {
                        let edge = canvas_result(target.cached_solid_brush(
                            ColorF::new(ink, ink, ink, 0.22)))?;
                        target.draw_rounded_rect(&shape, &*edge, stroke);
                    }
                    target.clipped_color_text(
                        &text,
                        &self.tab_title,
                        &Rect::from_xywh(
                            bounds.x + 12.0,
                            bounds.y,
                            (bounds.width - 24.0).max(1.0),
                            bounds.height,
                        ),
                        if active || incoming { white } else { dim },
                    )?;
                }
                if super::interface_flags::HEADER_DIVIDER.get()
                    && !model.collapsed
                    && h > model.content_header() + 1.0
                {
                    let divider = canvas_result(target.cached_solid_brush(
                        super::theme::panel_divider(model.dark, model.backdrop),
                    ))?;
                    let inset = super::layout::HEADER_INSET;
                    let divider_y =
                        ((model.content_header() - inset + 4.0) * scale).round() / scale;
                    target.fill_rect(
                        &Rect::from_xywh(inset, divider_y, (w - inset * 2.0).max(0.0), 1.0 / scale),
                        &*divider,
                    );
                }
                if let Some(options) = model
                    .monitor
                    .as_ref()
                    .filter(|_| h > model.content_header() + 1.0)
                {
                    self.monitor.as_mut().unwrap().paint(
                        &target,
                        options,
                        Rect::from_xywh(
                            6.0,
                            model.content_header() + 6.0,
                            (w - 12.0).max(1.0),
                            (h - model.content_header() - 12.0).max(1.0),
                        ),
                        model.dark,
                        ink,
                    )?;
                } else if h > model.content_header() + 1.0 {
                    target.push_clip(&{
                        Rect::from_xywh(
                            6.0,
                            model.content_header(),
                            w - 12.0,
                            (h - model.horizontal_inset(w) - model.content_header() - 6.0).max(0.0),
                        )
                    });
                    let list = model.is_list();
                    let list_scrollbar = list && super::scrollbar::Bar::for_grid(model, grid, w, h).is_some();
                    let size_right_inset = if list_scrollbar { 16.0 } else { 8.0 };
                    let columns = model.list_columns(grid.cell_width);
                    if list && model.folder.is_some() {
                        let list_divider = canvas_result(target.cached_solid_brush(
                            super::theme::panel_divider(model.dark, model.backdrop),
                        ))?;
                        for column in 1..4 {
                            if model.folder_visible_columns & (1 << column) == 0 {
                                continue;
                            }
                            let boundary = columns[column];
                            target.fill_rect(
                                &Rect::from_xywh(
                                    super::layout::PADDING + boundary,
                                    grid.content_top - super::layout::LIST_HEADER + 6.0,
                                    1.0 / scale,
                                    super::layout::LIST_HEADER - 12.0,
                                ),
                                &*list_divider,
                            );
                        }
                        for (column, name) in [
                            crate::i18n::text("ui-name"),
                            crate::i18n::text("ui-type"),
                            crate::i18n::text("ui-modified"),
                            crate::i18n::text("ui-size"),
                        ]
                        .iter()
                        .enumerate()
                        {
                            if column > 0 && model.folder_visible_columns & (1 << column) == 0 {
                                continue;
                            }
                            let col_feedback = self.columns.sample(
                                &column,
                                model.hovered_column == Some(column),
                                false,
                                false,
                                now,
                                animate,
                            );
                            playing |= col_feedback.playing;
                            let top = grid.content_top - super::layout::LIST_HEADER;
                            if col_feedback.hover > 0.0 {
                                let col_fill = canvas_result(target.cached_solid_brush(
                                    ColorF::new(ink, ink, ink, 0.06 * col_feedback.hover),
                                ))?;
                                target.fill_rounded_rect(
                                    &RoundedRect {
                                        rect: Rect::from_xywh(
                                            super::layout::PADDING + columns[column] + 1.0,
                                            top + 2.0,
                                            (columns[column + 1] - columns[column] - 2.0).max(0.0),
                                            super::layout::LIST_HEADER - 5.0,
                                        ),
                                        radius_x: 4.0,
                                        radius_y: 4.0,
                                    },
                                    &*col_fill,
                                );
                            }
                            let inset = if column == 0 { 0.0 } else { 8.0 };
                            let right_inset = if column == 3 { size_right_inset } else { 8.0 };
                            let left = super::layout::PADDING + columns[column] + inset;
                            let available =
                                (columns[column + 1] - columns[column] - inset - right_inset).max(1.0);
                            let sorted =
                                model.folder_sort.0 as usize == column && available >= 30.0;
                            let text_width = if sorted { available - 18.0 } else { available };
                            if column == 3 {
                                let text_left = if sorted { left + 18.0 } else { left };
                                target.clipped_cached_text(
                                    name,
                                    &self.menu_shortcut,
                                    &Rect::from_xywh(
                                        text_left,
                                        top,
                                        text_width,
                                        super::layout::LIST_HEADER,
                                    ),
                                    dim,
                                );
                                if sorted {
                                    let icon_left = (text_left + text_width
                                        - self.column_label_widths[column].min(text_width)
                                        - 18.0)
                                        .max(left);
                                    target.clipped_icon(
                                        if model.folder_sort.1 {
                                            "\u{e70d}"
                                        } else {
                                            "\u{e70e}"
                                        },
                                        &self.sort_icons,
                                        &Rect::from_xywh(
                                            icon_left,
                                            top,
                                            14.0,
                                            super::layout::LIST_HEADER,
                                        ),
                                        dim,
                                    );
                                }
                            } else {
                                target.clipped_cached_text(
                                    name,
                                    &self.details,
                                    &Rect::from_xywh(
                                        left,
                                        top,
                                        text_width,
                                        super::layout::LIST_HEADER,
                                    ),
                                    dim,
                                );
                                if sorted {
                                    target.clipped_icon(
                                        if model.folder_sort.1 {
                                            "\u{e70d}"
                                        } else {
                                            "\u{e70e}"
                                        },
                                        &self.sort_icons,
                                        &Rect::from_xywh(
                                            left + self.column_label_widths[column].min(text_width)
                                                + 4.0,
                                            top,
                                            14.0,
                                            super::layout::LIST_HEADER,
                                        ),
                                        dim,
                                    );
                                }
                            }
                        }
                        target.fill_rect(
                            &Rect::from_xywh(
                                super::layout::PADDING,
                                grid.content_top - 1.0 / scale,
                                grid.cell_width,
                                1.0 / scale,
                            ),
                            &*list_divider,
                        );
                    }
                    for &(index, x, y) in &self.visible_items {
                        let item = &model.items[index];
                        let bitmap = if let Some(image) = &item.image {
                            let key = Arc::as_ptr(image) as usize;
                            let ratio =
                                grid.icon_size * scale / image.width.max(image.height) as f32;
                            let size = (
                                (image.width as f32 * ratio).round().max(1.0) as u32,
                                (image.height as f32 * ratio).round().max(1.0) as u32,
                            );
                            if self.images.get(&key).is_none_or(|(source, _, old_size)| {
                                source.as_ptr() != Arc::as_ptr(image) || *old_size != size
                            }) {
                                if upload_budget == 0 {
                                    // Schedule another frame even when animations are disabled.
                                    playing = true;
                                } else {
                                    upload_budget -= 1;
                                    let pixels = super::scaled_icons::resample(image, size.0, size.1)?;
                                    let bitmap = canvas_result(target.create_bitmap(
                                        &pixels.data,
                                        pixels.width,
                                        pixels.height,
                                    ))?;
                                    self.images.insert(key, (Arc::downgrade(image), bitmap, size));
                                    #[cfg(test)]
                                    {
                                        self.image_uploads += 1;
                                    }
                                    if luciddesk_diagnostics::enabled(
                                        luciddesk_diagnostics::Level::Trace,
                                    ) && item
                                        .identity
                                        .persistent_key()
                                        .to_ascii_lowercase()
                                        .contains("645ff040-5081-101b-9f08-00aa002f954e")
                                    {
                                        luciddesk_diagnostics::emit!(
                                            luciddesk_diagnostics::Level::Trace,
                                            "pane.render",
                                            "recycle-render-upload hash={:x} size={:?}",
                                            image.data.iter().fold(0u64, |h, b| h
                                                .wrapping_mul(31)
                                                .wrapping_add(u64::from(*b))),
                                            size
                                        );
                                    }
                                }
                            }
                            // An earlier size may be reused until its replacement is ready.
                            self.images.get(&key).map(|(_, bitmap, _)| (bitmap, size))
                        } else {
                            None
                        };
                        let feedback = self.items.sample(&item.identity, model.hovered_item == Some(index), false,
                            model.selection.contains(&index), now, animate && self.marquee.is_none());
                        playing |= feedback.playing;
                        let keyboard_focus = model.focused && model.selected == Some(index);
                        if feedback.hover > 0.0 || feedback.selected > 0.0 || keyboard_focus {
                            let bounds = model.selection_bounds(grid, index, scale);
                            let inset_y = if list { 1.0 } else { 0.0 };
                            let rect = Rect::from_xywh(
                                bounds.x,
                                bounds.y + inset_y,
                                bounds.width,
                                (bounds.height - inset_y * 2.0).max(1.0),
                            );
                            let width = (bounds.width * scale).round();
                            let height = (bounds.height * scale).round();
                            // Preserve the old square fallback for very large selections.
                            let fallback = !(1.0..=4096.0).contains(&width) || !(1.0..=4096.0).contains(&height);
                            let radius = if fallback {
                                0.0
                            } else {
                                (if list { 4.0_f32 } else { 5.0_f32 }).min(rect.height() * 0.5)
                            };
                            let shape = RoundedRect { rect, radius_x: radius, radius_y: radius };
                            let hot = if model.focused { feedback.hover } else { 0.0 };
                            let (normal, hot_brush) = if fallback {
                                let brush = if model.focused { selection } else { inactive };
                                (brush, brush)
                            } else {
                                let normal = if model.focused { item_selected } else { item_inactive };
                                (normal, item_hot_selected)
                            };
                            target.fill_highlight(&shape, if fallback { hover } else { item_hover },
                                feedback.hover * (1.0 - feedback.selected));
                            target.fill_highlight(&shape, normal, feedback.selected * (1.0 - hot));
                            target.fill_highlight(&shape, hot_brush, feedback.selected * hot);
                            if !fallback && (feedback.selected > 0.0 || keyboard_focus) {
                                let stroke = 1.0 / scale;
                                let inset = stroke * 0.5;
                                let stroke_shape = RoundedRect {
                                    rect: Rect::from_xywh(
                                        rect.left + inset,
                                        rect.top + inset,
                                        (rect.width() - stroke).max(0.0),
                                        (rect.height() - stroke).max(0.0),
                                    ),
                                    radius_x: (radius - inset).max(0.0),
                                    radius_y: (radius - inset).max(0.0),
                                };
                                if keyboard_focus {
                                    target.draw_rounded_rect(&stroke_shape, focus_ring, stroke);
                                } else {
                                    canvas::with_brush_opacity(item_border, feedback.selected, || {
                                        target.draw_rounded_rect(&stroke_shape, item_border, stroke);
                                    });
                                }
                            }
                        }
                        if let Some((bitmap, size)) = bitmap {
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
                                y + grid.icon_top() + (grid.icon_size - ih) / 2.0
                            }) * scale)
                                .round()
                                / scale;
                            target.draw_bitmap(
                                bitmap,
                                &Rect::from_xywh(left, top, iw, ih),
                                1.0,
                            );
                        } else {
                            let left = if list {
                                x + 4.0
                            } else {
                                x + (grid.cell_width - grid.icon_size) / 2.0
                            };
                            let top = if list {
                                y + (grid.cell_height - grid.icon_size) / 2.0
                            } else {
                                y + grid.icon_top()
                            };
                            draw_placeholder(
                                &target,
                                Rect::from_xywh(
                                    (left * scale).round() / scale,
                                    (top * scale).round() / scale,
                                    grid.icon_size,
                                    grid.icon_size,
                                ),
                                item.details.folder,
                                placeholder_fill,
                                placeholder_edge,
                            );
                        }

                        if list {
                            let size = if model.folder.is_none() || model.folder_visible_columns & (1 << 3) == 0
                                || item.details.folder
                            {
                                ""
                            } else if let Some(bytes) = item.details.size {
                                if self.size_labels.len() >= 512 && !self.size_labels.contains_key(&bytes) {
                                    self.size_labels.clear();
                                }
                                self.size_labels.entry(bytes)
                                    .or_insert_with(|| super::folder::size_text(Some(bytes), false))
                                    .as_str()
                            } else {
                                "—"
                            };
                            for (column, text) in [
                                item.label.as_str(),
                                item.details.kind_text(),
                                item.details.modified.as_str(),
                                size,
                            ]
                            .iter()
                            .enumerate()
                            {
                                if column > 0 && model.folder.is_none() {
                                    continue;
                                }
                                if column > 0 && model.folder_visible_columns & (1 << column) == 0 {
                                    continue;
                                }
                                if column == 0 && model.renaming.as_ref() == Some(&item.identity) {
                                    continue;
                                }
                                let inset = if column == 0 { 0.0 } else { 8.0 };
                                let right_inset = if column == 3 { size_right_inset } else { 8.0 };
                                target.clipped_cached_text(
                                    text,
                                    if column == 3 {
                                        &self.menu_shortcut
                                    } else {
                                        &self.details
                                    },
                                    &Rect::from_xywh(
                                        x + columns[column] + inset,
                                        y,
                                        (columns[column + 1] - columns[column] - inset - right_inset)
                                            .max(1.0),
                                        grid.cell_height,
                                    ),
                                    if column == 0 { white } else { dim },
                                );
                            }
                            continue;
                        }
                        if model.renaming.as_ref() == Some(&item.identity) || model.icon_display.icon_only {
                            continue;
                        }
                        let label = super::label::prepared_layout(
                            &item.label,
                            (grid.cell_width * scale).round() as u32,
                            (96.0 * scale).round() as u32,
                            2,
                            grid.text_scale,
                        )?;
                        let label_x = (x * scale).round() / scale + 2.0;
                        let label_y = ((y + grid.icon_size + crate::pane::layout::LABEL_OFFSET) * scale)
                            .round()
                            / scale;
                        if model.native_material && !model.options.text_protection && opacity == 0.0 {
                            let shadow = canvas_result(target.cached_solid_brush(
                                if contrast.light_text {
                                    ColorF::new(0.0, 0.0, 0.0, 0.52)
                                } else {
                                    ColorF::new(1.0, 1.0, 1.0, 0.45)
                                },
                            ))?;
                            target.clipped_layout_with_size(&label.layout, label_x, label_y + 1.0 / scale, label.clip_size, &*shadow);
                        }
                        target.clipped_layout_with_size(&label.layout, label_x, label_y, label.clip_size, white);
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
                        let empty_top = model.content_header();
                        let empty_height = (h - empty_top - 6.0).max(1.0);
                        let muted = canvas_result(target.cached_solid_brush(ColorF::new(
                            ink, ink, ink, 0.56,
                        )))?;
                        if empty_height >= 84.0 {
                            let glyph = canvas_result(target.cached_solid_brush(ColorF::new(
                                ink, ink, ink, 0.26,
                            )))?;
                            let center_y = empty_top + empty_height * 0.5;
                            target.clipped_icon(
                                if model.folder.is_some() { "\u{e8b7}" } else { "\u{e8f1}" },
                                &self.empty_icon,
                                &Rect::from_xywh(20.0, center_y - 34.0, (w - 40.0).max(1.0), 32.0),
                                &*glyph,
                            );
                            target.clipped_text(
                                text,
                                &self.labels,
                                &Rect::from_xywh(20.0, center_y + 2.0, (w - 40.0).max(1.0), 36.0),
                                &*muted,
                            );
                        } else {
                            target.clipped_text(
                                text,
                                &self.labels,
                                &Rect::from_xywh(20.0, empty_top, (w - 40.0).max(1.0), empty_height),
                                &*muted,
                            );
                        }
                    }
                    target.pop_clip();
                    for bar in [
                        super::scrollbar::Bar::for_grid(model, grid, w, h),
                        super::scrollbar::Bar::horizontal(model, w, h),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        let expansion = model.scrollbar.expansion.clamp(0.0, 1.0);
                        let center = bar.left + super::scrollbar::Bar::WIDTH / 2.0;
                        if expansion > 0.0 {
                            let track_width = 2.0 + 4.0 * expansion;
                            let track = canvas_result(target.cached_solid_brush(ColorF::new(
                                ink,
                                ink,
                                ink,
                                0.07 * expansion,
                            )))?;
                            target.fill_rounded_rect(
                                &RoundedRect {
                                    rect: {
                                        let (x, y, w, h) = bar.rect(
                                            center - track_width / 2.0,
                                            bar.top,
                                            track_width,
                                            bar.height,
                                        );
                                        Rect::from_xywh(x, y, w, h)
                                    },
                                    radius_x: track_width / 2.0,
                                    radius_y: track_width / 2.0,
                                },
                                &*track,
                            );
                        }
                        let width = 2.0 + 2.0 * expansion;
                        let thumb = canvas_result(target.cached_solid_brush(ColorF::new(
                            ink,
                            ink,
                            ink,
                            0.42 + 0.40 * expansion,
                        )))?;
                        target.fill_rounded_rect(
                            &RoundedRect {
                                rect: {
                                    let (x, y, w, h) = bar.rect(
                                        center - width / 2.0,
                                        bar.thumb_top,
                                        width,
                                        bar.thumb_height,
                                    );
                                    Rect::from_xywh(x, y, w, h)
                                },
                                radius_x: width / 2.0,
                                radius_y: width / 2.0,
                            },
                            &*thumb,
                        );
                    }
                }
                if let Some(rect) = self.marquee.filter(|_| !model.collapsed) {
                    let bounds = Rect::from_xywh(rect.x, rect.y, rect.width, rect.height);
                    let fill = canvas_result(
                        target.cached_solid_brush(ColorF::new(0.2, 0.55, 1.0, 0.18)),
                    )?;
                    let border =
                        canvas_result(target.cached_solid_brush(ColorF::new(0.3, 0.65, 1.0, 0.9)))?;
                    target.fill_rect(&bounds, &*fill);
                    target.draw_rect(&bounds, &*border, 1.0 / scale);
                }
                // The real-desktop regression samples composition content, not
                // Acrylic's changing background tint or a separate child HWND.
                #[cfg(test)]
                if model.title == "Shell visibility regression" {
                    let marker = canvas_result(target.cached_solid_brush(ColorF::new(1.0, 0.0, 1.0, 1.0)))?;
                    target.fill_rect(&Rect::from_xywh(10.0, 45.0, 30.0, 25.0), &*marker);
                }
                target.finish()
            })?;
        }
        self.buttons.finish_frame();
        self.columns.finish_frame();
        self.tabs.finish_frame();
        self.items.finish_frame();
        Ok(playing)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod bench;

pub(crate) mod debug;
