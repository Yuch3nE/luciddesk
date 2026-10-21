use super::{Entry, Model, PADDING, RECT, ROW_HEIGHT};
use crate::pane::{
    canvas,
    native_graphics::{canvas_result, native_interface},
};
use windows_canvas::{
    ColorF, FontWeight, ParagraphAlignment, Rect, RoundedRect, TextFormat, TextLayout, WordWrapping,
};

pub(super) struct Painter {
    pub width: f32,
    layouts: Vec<TextLayout>,
    widths: Vec<f32>,
    enabled: Vec<bool>,
    available: f32,
    check: TextLayout,
    check_offset: f32,
    brushes: canvas::Brushes<7>,
}
impl Painter {
    pub fn new(rows: &[Entry]) -> windows::core::Result<Self> {
        let format = canvas_result(TextFormat::with_weight(
            &crate::pane::fonts::family(),
            crate::pane::control_style::TEXT,
            FontWeight(400),
        ))?
        .with_paragraph_alignment(ParagraphAlignment::Center)
        .with_word_wrapping(WordWrapping::NoWrap);
        canvas::ellipsis(&format)?;
        let icons = canvas_result(TextFormat::new(crate::pane::fonts::icon_family(), 12.0))?
            .with_paragraph_alignment(ParagraphAlignment::Center);
        let check = canvas_result(TextLayout::new("\u{e73e}", &icons, 16.0, ROW_HEIGHT))?;
        let check_offset = canvas::text_ink_center_offset(&check)?;
        let layouts: Vec<_> = rows
            .iter()
            .map(|row| canvas_result(TextLayout::new(&row.label, &format, 4096.0, ROW_HEIGHT)))
            .collect::<windows::core::Result<_>>()?;
        let widths: Vec<_> = layouts
            .iter()
            .map(|layout| layout.metrics().width_including_trailing_whitespace)
            .collect();
        let width = (widths.iter().copied().fold(176.0_f32, f32::max) + 52.0).min(480.0);
        Ok(Self {
            width,
            layouts,
            widths,
            available: 0.0,
            check,
            check_offset,
            enabled: rows.iter().map(|row| row.enabled).collect(),
            brushes: canvas::Brushes::default(),
        })
    }
    pub fn fit(&mut self, width: f32) -> windows::core::Result<()> {
        self.available = (width - 52.0).max(1.0);
        for layout in &self.layouts {
            let native: windows::Win32::Graphics::DirectWrite::IDWriteTextLayout =
                native_interface(layout.raw())?;
            unsafe {
                native.SetMaxWidth(self.available)?;
            }
        }
        Ok(())
    }
    pub fn truncated(&self, index: usize) -> bool {
        self.widths
            .get(index)
            .is_some_and(|width| *width > self.available)
    }
    pub fn paint(
        &mut self,
        context: &windows_canvas::ID2D1DeviceContext,
        bounds: RECT,
        scale: f32,
        model: &Model,
        highlights: &[f32],
        appearance: (bool, bool),
    ) -> windows::core::Result<()> {
        let (native, dark) = appearance;
        canvas::draw(context, scale, |target| {
            let width = (bounds.right - bounds.left) as f32 / scale;
            let height = (bounds.bottom - bounds.top) as f32 / scale;
            let colors = colors(dark);
            let ink = colors[1].r;
            let [
                background,
                text,
                disabled,
                border,
                hover,
                selected,
                accent_brush,
            ] = canvas_result(self.brushes.get(context, &target, colors))?;
            target.clear(ColorF::new(0.0, 0.0, 0.0, 0.0));
            let outer = RoundedRect {
                rect: Rect::from_xywh(0.5, 0.5, width - 1.0, height - 1.0),
                radius_x: 8.0,
                radius_y: 8.0,
            };
            if !native {
                target.fill_rounded_rect(&outer, background);
            }
            target.draw_rounded_rect(&outer, border, 1.0);
            target.push_clip(&Rect::from_xywh(
                PADDING,
                PADDING,
                width - 2.0 * PADDING,
                height - 2.0 * PADDING,
            ));
            for index in model.range() {
                let top = PADDING + (index - model.first) as f32 * ROW_HEIGHT;
                let row = RoundedRect {
                    rect: Rect::from_xywh(PADDING, top, width - 2.0 * PADDING, ROW_HEIGHT),
                    radius_x: 5.0,
                    radius_y: 5.0,
                };
                if model.current == Some(index) {
                    target.fill_rounded_rect(&row, selected);
                    target.fill_rounded_rect(
                        &RoundedRect {
                            rect: Rect::from_xywh(4.0, top + 7.0, 3.0, 18.0),
                            radius_x: 1.5,
                            radius_y: 1.5,
                        },
                        accent_brush,
                    );
                    target.clipped_layout(
                        &self.check,
                        width - 35.0,
                        top + self.check_offset,
                        accent_brush,
                    );
                }
                let progress = highlights.get(index).copied().unwrap_or(0.0);
                if progress > 0.0 {
                    hover.set_color(ColorF::new(
                        ink,
                        ink,
                        ink,
                        (if dark { 0.07 } else { 0.045 }) * progress,
                    ));
                    target.fill_rounded_rect(&row, hover);
                }
                target.clipped_layout(
                    &self.layouts[index],
                    12.0,
                    top,
                    if self.enabled[index] { text } else { disabled },
                );
            }
            target.pop_clip();
            if let Some((y, thumb)) = model.scrollbar(height) {
                target.fill_rounded_rect(
                    &RoundedRect {
                        rect: Rect::from_xywh(width - 3.0, y, 2.0, thumb),
                        radius_x: 1.0,
                        radius_y: 1.0,
                    },
                    border,
                );
            }
            target.finish()
        })
    }
}

fn colors(dark: bool) -> [ColorF; 7] {
    let ink = if dark { 0.95 } else { 0.10 };
    let background = if dark { 0.15 } else { 0.98 };
    let (red, green, blue) = if dark {
        (118_u8, 185_u8, 237_u8)
    } else {
        (0_u8, 103_u8, 192_u8)
    };
    let accent = ColorF::new(
        f32::from(red) / 255.0,
        f32::from(green) / 255.0,
        f32::from(blue) / 255.0,
        1.0,
    );
    [
        ColorF::new(background, background, background, 1.0),
        ColorF::new(ink, ink, ink, 1.0),
        ColorF::new(ink, ink, ink, 0.4),
        ColorF::new(ink, ink, ink, 0.14),
        ColorF::new(ink, ink, ink, 0.06),
        ColorF {
            a: if dark { 0.13 } else { 0.08 },
            ..accent
        },
        accent,
    ]
}
