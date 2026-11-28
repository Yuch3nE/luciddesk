//! Canvas draw-pass lifetime, clipping and the few native operations absent from Canvas 0.100.
use super::native_graphics::{canvas_result, native_interface};
use luciddesk_window::control_style::ElevationGradient;
use windows::Win32::Graphics::Direct2D::{
    Common::D2D_RECT_F, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
    ID2D1DeviceContext, ID2D1Image,
};
use windows::core::Result;
use windows_canvas as c;

mod brushes;
pub(super) use brushes::Brushes;
mod cache;
pub(super) use cache::FrameCache;

/// Canvas does not expose ellipsis trimming; keep this native operation at the boundary.
pub fn ellipsis(format: &c::TextFormat) -> Result<()> {
    ellipsis_delimiter(format, 0)
}

/// Center the visible glyphs, rather than the font's asymmetric line box.
pub fn text_ink_center_offset(layout: &c::TextLayout) -> Result<f32> {
    use windows::Win32::Graphics::DirectWrite::IDWriteTextLayout;
    let native: IDWriteTextLayout = native_interface(layout.raw())?;
    let ink = unsafe { native.GetOverhangMetrics()? };
    Ok((ink.top - ink.bottom) * 0.5)
}

/// Preserve the final path component or extension when trimming long labels.
pub fn ellipsis_delimiter(format: &c::TextFormat, delimiter: u32) -> Result<()> {
    apply_fallback(format)?;
    use windows::Win32::Graphics::DirectWrite::*;
    unsafe {
        let native: IDWriteTextFormat = native_interface(format.raw())?;
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let sign = factory.CreateEllipsisTrimmingSign(&native)?;
        native.SetTrimming(
            &DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter,
                delimiterCount: u32::from(delimiter != 0),
                ..Default::default()
            },
            &sign,
        )
    }
}

/// Prefer the language default for missing text glyphs, then Windows fallback.
/// The format owns the mapping; no COM objects survive in thread-local caches.
pub fn apply_fallback(format: &c::TextFormat) -> Result<()> {
    use windows::Win32::Graphics::DirectWrite::*;
    use windows::core::PCWSTR;
    unsafe {
        let factory: IDWriteFactory2 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let builder = factory.CreateFontFallbackBuilder()?;
        let wide: Vec<u16> = crate::i18n::default_font()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // Private-use icons keep their dedicated font mapping.
        builder.AddMapping(
            &[
                DWRITE_UNICODE_RANGE {
                    first: 0,
                    last: 0xd7ff,
                },
                DWRITE_UNICODE_RANGE {
                    first: 0xf900,
                    last: 0xeffff,
                },
            ],
            &[wide.as_ptr()],
            None,
            PCWSTR::null(),
            PCWSTR::null(),
            1.0,
        )?;
        builder.AddMappings(&factory.GetSystemFontFallback()?)?;
        let native: IDWriteTextFormat1 = native_interface(format.raw())?;
        native.SetFontFallback(&builder.CreateFontFallback()?)
    }
}

pub struct DrawPass<'a> {
    cache: Option<&'a std::cell::RefCell<FrameCache>>,
    session: c::DrawingSession<'a>,
    native: ID2D1DeviceContext,
    active: bool,
    scale: f32,
    clips: std::cell::Cell<usize>,
}

pub(super) struct BitmapBrush {
    brush: windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush1,
    width: u32,
    height: u32,
}

pub(super) struct OpacityLayer<'a>(&'a ID2D1DeviceContext);

pub(super) struct TransformGuard<'a> {
    native: &'a ID2D1DeviceContext,
    previous: windows_numerics::Matrix3x2,
}

impl Drop for TransformGuard<'_> {
    fn drop(&mut self) {
        unsafe { self.native.SetTransform(&self.previous) };
    }
}

/// Schedule control repainting only while the last visible frame was moving.
#[derive(Default)]
pub(super) struct ControlRedraw(bool);

impl ControlRedraw {
    pub const TIMER: usize = 0x4c46;

    pub fn tick(&mut self, hwnd: windows_sys::Win32::Foundation::HWND) {
        self.sync(hwnd, self.0);
        if self.0 {
            unsafe { windows_sys::Win32::Graphics::Gdi::InvalidateRect(hwnd, std::ptr::null(), 0); }
        }
    }

    pub fn sync(&mut self, hwnd: windows_sys::Win32::Foundation::HWND, playing: bool) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible, KillTimer, SetTimer, USER_TIMER_MINIMUM};
        let playing = playing && unsafe { IsWindowVisible(hwnd) != 0 && IsIconic(hwnd) == 0 };
        if playing == self.0 { return; }
        unsafe {
            if playing {
                self.0 = SetTimer(hwnd, Self::TIMER, USER_TIMER_MINIMUM, None) != 0;
            } else {
                KillTimer(hwnd, Self::TIMER);
                self.0 = false;
            }
        }
    }
}

impl Drop for OpacityLayer<'_> {
    fn drop(&mut self) {
        unsafe { self.0.PopLayer() };
    }
}

pub fn draw<T>(
    target: &c::ID2D1DeviceContext,
    scale: f32,
    paint: impl FnOnce(DrawPass<'_>) -> Result<T>,
) -> Result<T> {
    draw_inner(target, scale, None, paint)
}

pub(super) fn draw_cached<T>(
    target: &c::ID2D1DeviceContext,
    scale: f32,
    cache: &std::cell::RefCell<FrameCache>,
    paint: impl FnOnce(DrawPass<'_>) -> Result<T>,
) -> Result<T> {
    cache.borrow_mut().begin(target, scale);
    draw_inner(target, scale, Some(cache), paint)
}

fn draw_inner<T>(
    target: &c::ID2D1DeviceContext,
    scale: f32,
    cache: Option<&std::cell::RefCell<FrameCache>>,
    paint: impl FnOnce(DrawPass<'_>) -> Result<T>,
) -> Result<T> {
    let native: ID2D1DeviceContext = native_interface(target)?;
    unsafe {
        native.SetDpi(96.0 * scale, 96.0 * scale);
        native.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        native.BeginDraw();
    }
    let session = c::DrawingSession::from_borrowed_context(target, c::Matrix3x2::identity());
    paint(DrawPass {
        cache,
        session,
        native,
        active: true,
        scale,
        clips: std::cell::Cell::new(0),
    })
}

impl<'a> std::ops::Deref for DrawPass<'a> {
    type Target = c::DrawingSession<'a>;
    fn deref(&self) -> &Self::Target {
        &self.session
    }
}

impl DrawPass<'_> {
    pub(super) fn fill_highlight(&self, shape: &c::RoundedRect, brush: &c::Brush, amount: f32) {
        with_brush_opacity(brush, amount, || self.fill_rounded_rect(shape, brush));
    }

    pub(super) fn opacity_layer(&self, bounds: &c::Rect, opacity: f32) -> Option<OpacityLayer<'_>> {
        use windows::Win32::Graphics::Direct2D::{D2D1_LAYER_PARAMETERS1, ID2D1Layer};
        if opacity >= 1.0 { return None; }
        let parameters = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: bounds.left, top: bounds.top, right: bounds.right, bottom: bounds.bottom,
            },
            maskTransform: windows_numerics::Matrix3x2 { M11: 1.0, M22: 1.0, ..Default::default() },
            opacity: opacity.max(0.0),
            ..Default::default()
        };
        unsafe { self.native.PushLayer(&parameters, None::<&ID2D1Layer>) };
        Some(OpacityLayer(&self.native))
    }

    pub(super) fn bitmap_brush(
        &self, pixels: &[u8], width: u32, height: u32,
    ) -> Result<BitmapBrush> {
        use windows::Win32::Graphics::Direct2D::{
            Common::{D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT},
            D2D1_BITMAP_BRUSH_PROPERTIES1, D2D1_BITMAP_PROPERTIES1, D2D1_INTERPOLATION_MODE_LINEAR,
        };
        use windows::Win32::{Foundation::E_INVALIDARG, Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM};
        let invalid = || windows::core::Error::from_hresult(E_INVALIDARG);
        let pitch = width.checked_mul(4).ok_or_else(invalid)?;
        if width == 0 || height == 0 || (pitch as usize).checked_mul(height as usize) != Some(pixels.len()) {
            return Err(invalid());
        }
        let bitmap_properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };
        let brush_properties = D2D1_BITMAP_BRUSH_PROPERTIES1 {
            interpolationMode: D2D1_INTERPOLATION_MODE_LINEAR,
            ..Default::default()
        };
        let brush = unsafe {
            let bitmap = self.native.CreateBitmap(
                D2D_SIZE_U { width, height }, Some(pixels.as_ptr().cast()), pitch, &bitmap_properties,
            )?;
            self.native.CreateBitmapBrush(&bitmap, Some(&brush_properties), None)?
        };
        Ok(BitmapBrush { brush, width, height })
    }

    pub(super) fn fill_bitmap_rounded_rect(
        &self,
        shape: &c::RoundedRect,
        brush: &BitmapBrush,
    ) {
        use windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT;
        let c::Rect { left, top, right, bottom } = shape.rect;
        if right <= left || bottom <= top {
            return;
        }
        let transform = windows_numerics::Matrix3x2 {
            M11: (right - left) / brush.width as f32,
            M22: (bottom - top) / brush.height as f32,
            M31: left,
            M32: top,
            ..Default::default()
        };
        let rounded = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left, top, right, bottom },
            radiusX: shape.radius_x,
            radiusY: shape.radius_y,
        };
        unsafe {
            brush.brush.SetTransform(&transform);
            self.native.FillRoundedRectangle(&rounded, &*brush.brush);
        }
    }

    pub(super) fn draw_elevation_border(
        &self,
        shape: &c::RoundedRect,
        stops: ElevationGradient,
        stable: bool,
        thickness: f32,
    ) {
        use windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT;
        let inset = thickness * 0.5;
        let rect = D2D_RECT_F {
            left: shape.rect.left + inset,
            top: shape.rect.top + inset,
            right: shape.rect.right - inset,
            bottom: shape.rect.bottom - inset,
        };
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return;
        }
        let gradient = match self.cache.filter(|_| stable) {
            Some(cache) => cache.borrow_mut().gradient(&self.native, stops),
            None => create_elevation_gradient(&self.native, stops),
        };
        let Ok(gradient) = gradient else { return; };
        unsafe {
            gradient.SetStartPoint(windows_numerics::Vector2::new(rect.left, rect.top));
            gradient.SetEndPoint(windows_numerics::Vector2::new(rect.left, rect.bottom));
            let rounded = D2D1_ROUNDED_RECT {
                rect,
                radiusX: (shape.radius_x - inset).max(0.0),
                radiusY: (shape.radius_y - inset).max(0.0),
            };
            self.native.DrawRoundedRectangle(&rounded, &*gradient, thickness, None);
        }
    }

    pub(super) fn translated(&self, dx: f32, dy: f32) -> Option<TransformGuard<'_>> {
        if dx.abs() < 0.01 && dy.abs() < 0.01 {
            return None;
        }
        let mut previous = windows_numerics::Matrix3x2::default();
        unsafe {
            self.native.GetTransform(&raw mut previous);
            let mut next = previous;
            next.M31 += dx;
            next.M32 += dy;
            self.native.SetTransform(&next);
        }
        Some(TransformGuard {
            native: &self.native,
            previous,
        })
    }

    pub(super) fn rotated_icon(
        &self, text: &str, format: &c::TextFormat, bounds: &c::Rect, brush: &c::Brush, radians: f32,
    ) {
        let x = (bounds.left + bounds.right) * 0.5;
        let y = (bounds.top + bounds.bottom) * 0.5;
        let (sin, cos) = radians.sin_cos();
        let mut previous = windows_numerics::Matrix3x2::default();
        self.push_clip(bounds);
        unsafe {
            self.native.GetTransform(&raw mut previous);
            let rotation = windows_numerics::Matrix3x2 {
                M11: cos,
                M12: sin,
                M21: -sin,
                M22: cos,
                M31: x - x * cos + y * sin + previous.M31,
                M32: y - x * sin - y * cos + previous.M32,
            };
            self.native.SetTransform(&rotation);
        }
        self.clipped_icon(text, format, bounds, brush);
        unsafe { self.native.SetTransform(&previous) };
        self.pop_clip();
    }

    pub fn cached_solid_brush(&self, color: c::ColorF) -> canvas_core::Result<std::rc::Rc<c::Brush>> {
        match self.cache {
            Some(cache) => cache.borrow_mut().brush(&self.session, color),
            None => self.session.create_solid_brush(color).map(std::rc::Rc::new),
        }
    }
    pub fn plugin_context(&self) -> *mut std::ffi::c_void {
        use windows::core::Interface;
        self.native.as_raw()
    }

    pub(super) fn draw_color_layout(
        &self, prepared: &ColorLayout, x: f32, y: f32, brush: &c::Brush,
    ) -> Result<()> {
        use c::Paint;
        use windows::Win32::Graphics::{
            Direct2D::{D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_DRAW_TEXT_OPTIONS_NONE, ID2D1Brush},
            DirectWrite::IDWriteTextLayout,
        };
        let layout: IDWriteTextLayout = native_interface(prepared.layout.raw())?;
        let brush: ID2D1Brush = native_interface(brush.as_raw_brush())?;
        let options = if super::interface_flags::TITLE_EMOJI.get() {
            D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT
        } else {
            D2D1_DRAW_TEXT_OPTIONS_NONE
        };
        let draw = |y| unsafe {
            self.native.DrawTextLayout(windows_numerics::Vector2::new(x, y), &layout, &brush, options);
        };
        let (width, height) = prepared.size;
        self.push_clip(&c::Rect::from_xywh(x, y, width, height));
        if prepared.spans.is_empty() {
            draw(y);
        } else {
            for &(left, right, offset) in &prepared.spans {
                self.push_clip(&c::Rect::from_xywh(x + left, y, right - left, height));
                draw(y - offset);
                self.pop_clip();
            }
        }
        self.pop_clip();
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn clipped_color_layout(
        &self, text: &str, layout: &c::TextLayout, x: f32, y: f32, brush: &c::Brush,
    ) -> Result<()> {
        self.draw_color_layout(&ColorLayout::new(text, layout, self.scale)?, x, y, brush)
    }

    pub(super) fn clipped_color_text(
        &self, text: &str, format: &c::TextFormat, bounds: &c::Rect, brush: &c::Brush,
    ) -> Result<()> {
        let size = (bounds.width(), bounds.height());
        if text.is_empty() || size.0 <= 0.0 || size.1 <= 0.0 {
            return Ok(());
        }
        if let Some(cache) = self.cache {
            let prepared = cache.borrow_mut().color_text(text, format, size)?;
            return self.draw_color_layout(&prepared, bounds.left, bounds.top, brush);
        }
        let layout = canvas_result(c::TextLayout::new(text, format, size.0, size.1))?;
        let prepared = ColorLayout::new(text, &layout, self.scale)?;
        self.draw_color_layout(&prepared, bounds.left, bounds.top, brush)
    }

    /// Center the visible glyph, not the font's line box. MDL2's em-square
    /// placement differs from the UI text font and from Segoe Fluent Icons.
    pub fn clipped_icon(
        &self,
        text: &str,
        format: &c::TextFormat,
        bounds: &c::Rect,
        brush: &c::Brush,
    ) {
        let size = (bounds.right - bounds.left, bounds.bottom - bounds.top);
        let result = match self.cache {
            Some(cache) => cache.borrow_mut().icon(text, format, size),
            None => c::TextLayout::new(text, format, size.0, size.1).map(|layout| {
                let offset = text_ink_center_offset(&layout).unwrap_or(0.0);
                (layout, offset)
            }),
        };
        let Ok((layout, offset)) = result else {
            self.clipped_text(text, format, bounds, brush);
            return;
        };
        self.push_clip(bounds);
        self.session.draw_text_layout(
            c::Vector2::new(bounds.left, bounds.top + offset),
            &layout,
            brush,
        );
        self.pop_clip();
    }

    pub(super) fn clipped_native_icon(
        &self,
        layout: &windows::Win32::Graphics::DirectWrite::IDWriteTextLayout,
        bounds: &c::Rect,
        brush: &c::Brush,
    ) -> Result<()> {
        use c::Paint;
        use windows::Win32::Graphics::Direct2D::{D2D1_DRAW_TEXT_OPTIONS_NONE, ID2D1Brush};
        let native_brush: ID2D1Brush = native_interface(brush.as_raw_brush())?;
        let ink = unsafe { layout.GetOverhangMetrics()? };
        let origin = windows_numerics::Vector2::new(
            bounds.left + (ink.left - ink.right) * 0.5,
            bounds.top + (ink.top - ink.bottom) * 0.5,
        );
        self.push_clip(bounds);
        unsafe {
            self.native.DrawTextLayout(origin, layout, &native_brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
        }
        self.pop_clip();
        Ok(())
    }

    pub fn clipped_text(
        &self,
        text: &str,
        format: &c::TextFormat,
        bounds: &c::Rect,
        brush: &c::Brush,
    ) {
        self.push_clip(bounds);
        self.session.draw_text(text, format, bounds, brush);
        self.pop_clip();
    }

    pub(super) fn clipped_cached_text(
        &self, text: &str, format: &c::TextFormat, bounds: &c::Rect, brush: &c::Brush,
    ) {
        let size = (bounds.width(), bounds.height());
        if text.is_empty() || size.0 <= 0.0 || size.1 <= 0.0 {
            return;
        }
        let layout = self.cache.and_then(|cache| {
            cache.borrow_mut().text_layout(text, format, size).ok()
        });
        if let Some(layout) = layout {
            self.push_clip(bounds);
            self.session.draw_text_layout(c::Vector2::new(bounds.left, bounds.top), &layout, brush);
            self.pop_clip();
        } else {
            self.clipped_text(text, format, bounds, brush);
        }
    }
    pub fn clipped_layout(&self, layout: &c::TextLayout, x: f32, y: f32, brush: &c::Brush) {
        let metrics = layout.metrics();
        self.clipped_layout_with_size(layout, x, y, (metrics.layout_width, metrics.layout_height), brush);
    }
    pub(super) fn clipped_layout_with_size(
        &self, layout: &c::TextLayout, x: f32, y: f32, size: (f32, f32), brush: &c::Brush,
    ) {
        self.push_clip(&c::Rect::from_xywh(x, y, size.0, size.1));
        self.session
            .draw_text_layout(c::Vector2::new(x, y), layout, brush);
        self.pop_clip();
    }
    pub fn push_clip(&self, r: &c::Rect) {
        unsafe {
            self.native.PushAxisAlignedClip(
                &D2D_RECT_F {
                    left: r.left,
                    top: r.top,
                    right: r.right,
                    bottom: r.bottom,
                },
                D2D1_ANTIALIAS_MODE_ALIASED,
            );
        };
        self.clips.set(self.clips.get() + 1);
    }
    pub fn pop_clip(&self) {
        assert!(self.clips.get() > 0);
        unsafe { self.native.PopAxisAlignedClip() };
        self.clips.set(self.clips.get() - 1);
    }
    pub fn finish(mut self) -> Result<()> {
        while self.clips.get() > 0 {
            self.pop_clip();
        }
        self.active = false;
        unsafe { self.native.EndDraw(None, None) }
    }
}

/// DirectWrite shaping plus the clipped spans needed to center mixed emoji ink.
pub(super) struct ColorLayout {
    layout: c::TextLayout,
    size: (f32, f32),
    spans: Vec<(f32, f32, f32)>,
}

impl ColorLayout {
    pub(super) fn new(text: &str, layout: &c::TextLayout, scale: f32) -> Result<Self> {
        use windows::Win32::Graphics::DirectWrite::{
            DWRITE_CLUSTER_METRICS, DWRITE_FACTORY_TYPE_SHARED, DWRITE_HIT_TEST_METRICS,
            DWRITE_LINE_METRICS, DWRITE_LINE_SPACING_METHOD_UNIFORM, DWriteCreateFactory,
            IDWriteFactory, IDWriteTextLayout,
        };

        let size = layout.max_size();
        let mut prepared = Self { layout: layout.clone(), size, spans: Vec::new() };
        if !text.chars().any(emoji_character) {
            return Ok(prepared);
        }

        let native_layout: IDWriteTextLayout = native_interface(layout.raw())?;
        let (width, height) = size;
        let ranges = unsafe {
            // Keep shaping and trimming in one layout. Only move complete emoji
            // clusters, including joined sequences, within their original columns.
            let wide: Vec<_> = text.encode_utf16().collect();
            let mut clusters = vec![DWRITE_CLUSTER_METRICS::default(); wide.len()];
            let mut count = 0;
            native_layout.GetClusterMetrics(Some(&mut clusters), &raw mut count)?;
            let mut plain = Vec::new();
            let mut at = 0;
            let mut has_emoji = false;
            for cluster in &clusters[..count as usize] {
                let end = at + cluster.length as usize;
                if emoji_cluster(&wide[at..end]) {
                    has_emoji = true;
                } else {
                    plain.extend_from_slice(&wide[at..end]);
                }
                at = end;
            }
            if !has_emoji { return Ok(prepared); }
            let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let mut line = [DWRITE_LINE_METRICS::default(); 1];
            let mut lines = 0;
            native_layout.GetLineMetrics(Some(&mut line), &raw mut lines)?;
            let ink_center = |text: &[u16]| -> Result<f32> {
                let probe = factory.CreateTextLayout(
                    text,
                    &native_layout,
                    1_000_000.0,
                    height,
                )?;
                // Keep the original mixed-font baseline when measuring each run.
                probe.SetLineSpacing(
                    DWRITE_LINE_SPACING_METHOD_UNIFORM,
                    line[0].height,
                    line[0].baseline,
                )?;
                let ink = probe.GetOverhangMetrics()?;
                Ok((height + ink.bottom - ink.top) * 0.5)
            };
            let reference = if plain
                .iter()
                .all(|ch| matches!(*ch, 9 | 10 | 13 | 32 | 0x200d | 0xfe0e | 0xfe0f))
            {
                None
            } else {
                Some(ink_center(&plain)?)
            };
            let mut position = 0usize;
            let mut ranges = Vec::new();
            for cluster in &clusters[..count as usize] {
                let end = position + cluster.length as usize;
                if emoji_cluster(&wide[position..end]) {
                    let mut hit = [DWRITE_HIT_TEST_METRICS::default(); 2];
                    let mut hits = 0;
                    native_layout.HitTestTextRange(
                        position as u32,
                        u32::from(cluster.length),
                        0.0,
                        0.0,
                        Some(&mut hit),
                        &raw mut hits,
                    )?;
                    for bounds in &hit[..hits as usize] {
                        if bounds.isText.as_bool() && bounds.width > 0.0 {
                            let left = bounds.left.clamp(0.0, width);
                            let right =
                                (bounds.left + bounds.width).clamp(left, width);
                            let offset = if let Some(reference) = reference {
                                ink_center(&wide[position..end])? - reference
                            } else {
                                0.0
                            };
                            ranges.push((left, right, offset));
                        }
                    }
                }
                position = end;
            }
            ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
            ranges
        };

        let mut left = 0.0;
        let mut add_span = |start, end, offset| {
            if end <= start { return; }
            if let Some((_, last_end, last_offset)) = prepared.spans.last_mut()
                && *last_end == start && *last_offset == offset
            {
                *last_end = end;
            } else {
                prepared.spans.push((start, end, offset));
            }
        };
        for (start, end, offset) in ranges {
            add_span(left, start, 0.0);
            // Align visible glyph centers, snapped to physical pixels.
            add_span(start.max(left), end, (offset * scale).round() / scale);
            left = left.max(end);
        }
        add_span(left, width, 0.0);
        if prepared.spans.as_slice() == [(0.0, width, 0.0)] {
            prepared.spans.clear();
        }
        Ok(prepared)
    }
}

struct BrushOpacityGuard {
    native: windows::Win32::Graphics::Direct2D::ID2D1Brush,
    previous: f32,
}

impl Drop for BrushOpacityGuard {
    fn drop(&mut self) {
        unsafe { self.native.SetOpacity(self.previous) };
    }
}

/// Fade one primitive while preserving the palette brush for its next draw.
pub(super) fn with_brush_opacity(brush: &c::Brush, amount: f32, draw: impl FnOnce()) {
    use c::Paint;
    use windows::Win32::Graphics::Direct2D::ID2D1Brush;
    if amount >= 1.0 {
        draw();
    } else if amount > 0.0
        && let Ok(native) = native_interface::<ID2D1Brush>(brush.as_raw_brush())
    {
        let previous = unsafe { native.GetOpacity() };
        unsafe { native.SetOpacity(previous * amount) };
        let _restore = BrushOpacityGuard { native, previous };
        draw();
    }
}

fn create_elevation_gradient(
    context: &ID2D1DeviceContext,
    stops: ElevationGradient,
) -> Result<windows::Win32::Graphics::Direct2D::ID2D1LinearGradientBrush> {
    use windows::Win32::Graphics::Direct2D::{
        Common::{D2D1_COLOR_F, D2D1_GRADIENT_STOP}, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2,
        D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, ID2D1RenderTarget,
    };
    let stop = |position, [r, g, b, a]: [f32; 4]| D2D1_GRADIENT_STOP { position, color: D2D1_COLOR_F { r, g, b, a } };
    let stops = [stop(0.0, stops.top), stop(stops.stop.clamp(0.05, 0.95), stops.mid), stop(1.0, stops.bottom)];
    unsafe {
        let collection = ID2D1RenderTarget::CreateGradientStopCollection(
            context, &stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP,
        )?;
        let properties = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: windows_numerics::Vector2::new(0.0, 0.0),
            endPoint: windows_numerics::Vector2::new(0.0, 1.0),
        };
        context.CreateLinearGradientBrush(&properties, None, &collection)
    }
}

pub(super) fn emoji_cluster(wide: &[u16]) -> bool {
    !wide.contains(&0xfe0e)
        && char::decode_utf16(wide.iter().copied())
            .filter_map(std::result::Result::ok)
            .any(emoji_character)
}

pub(super) fn emoji_character(ch: char) -> bool {
    matches!(ch as u32, 0x1f000..=0x1faff | 0x2600..=0x27bf | 0x20e3)
}
impl Drop for DrawPass<'_> {
    fn drop(&mut self) {
        if self.active {
            while self.clips.get() > 0 {
                self.pop_clip();
            }
            unsafe {
                let _ = self.native.EndDraw(None, None);
            }
        }
    }
}

/// CPU readback for Shell drag images and tests; live windows draw directly to GPU surfaces.
pub struct Offscreen {
    canvas: c::RenderTarget,
    context: ID2D1DeviceContext,
    image: ID2D1Image,
    pub target: c::ID2D1DeviceContext,
}
impl Offscreen {
    pub fn new(device: &c::GpuDevice, width: u32, height: u32) -> Result<Self> {
        let canvas = canvas_result(device.create_render_target(width, height))?;
        let mut captured = None;
        canvas_result(canvas.draw(|session| {
            captured = Some((|| -> Result<_> {
                let context: ID2D1DeviceContext = native_interface(session.raw())?;
                let image = unsafe { context.GetTarget()? };
                Ok((context, image, session.raw().clone()))
            })());
            Ok(())
        }))?;
        let (context, image, target) =
            captured.expect("Canvas invokes the draw closure synchronously")?;
        unsafe {
            context.SetTarget(&image);
        }
        Ok(Self {
            canvas,
            context,
            image,
            target,
        })
    }
    pub fn pixels(&self) -> Result<Vec<u8>> {
        unsafe {
            self.context.SetTarget(None::<&ID2D1Image>);
        }
        let result = canvas_result(self.canvas.read_pixels());
        unsafe {
            self.context.SetTarget(&self.image);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_folder_emoji_and_latin_text_have_aligned_visual_centers() {
        let device = c::GpuDevice::new_warp().unwrap();
        for family in ["Segoe UI", "Microsoft YaHei UI"] {
            for scale in [1.0, 1.5, 2.0] {
                let format = c::TextFormat::new(family, 14.0)
                    .unwrap()
                    .with_paragraph_alignment(c::ParagraphAlignment::Center)
                    .with_word_wrapping(c::WordWrapping::NoWrap);
                apply_fallback(&format).unwrap();
                let layout = c::TextLayout::new("📁AI", &format, 100.0, 40.0).unwrap();
                let boundary = (layout.caret_bounds(2, false).left * scale).round() as usize;
                let width = (100.0 * scale) as u32;
                let height = (40.0 * scale) as u32;
                let surface = Offscreen::new(&device, width, height).unwrap();
                draw(&surface.target, scale, |frame| {
                    frame.clear(c::ColorF::default());
                    let ink = canvas_result(frame.create_solid_brush(WHITE))?;
                    frame.clipped_color_layout("📁AI", &layout, 0.0, 0.0, &ink)?;
                    frame.finish()
                })
                .unwrap();
                let pixels = surface.pixels().unwrap();
                let center = |emoji: bool| {
                    let rows: Vec<_> = (0..height as usize)
                        .filter(|y| {
                            pixels[*y * width as usize * 4..(*y + 1) * width as usize * 4]
                                .chunks_exact(4)
                                .enumerate()
                                .any(|(x, p)| {
                                    p[3] > 100
                                        && if emoji {
                                            x < boundary
                                                && p[..3].iter().max().unwrap()
                                                    - p[..3].iter().min().unwrap()
                                                    > 40
                                        } else {
                                            x >= boundary && p[0] == p[1] && p[1] == p[2]
                                        }
                                })
                        })
                        .collect();
                    (*rows.first().unwrap() + *rows.last().unwrap()) as f32 / 2.0
                };
                let (emoji, text) = (center(true), center(false));
                assert!(
                    (emoji - text).abs() <= 1.0,
                    "{family} scale={scale}: emoji center={emoji}, text center={text}"
                );
            }
        }
    }

    #[test]
    fn text_uses_grayscale_on_both_opaque_and_transparent_surfaces() {
        let device = windows_canvas::GpuDevice::new().unwrap();
        let target = Offscreen::new(&device, 220, 50).unwrap();
        let format = c::TextFormat::new(super::super::assets::UI_FONT, 17.0).unwrap();
        for alpha in [0.0, 1.0] {
            draw(&target.target, 1.0, |frame| {
                frame.clear(c::ColorF {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: alpha,
                });
                let ink = canvas_result(frame.create_solid_brush(WHITE))?;
                frame.clipped_text(
                    "Grayscale ABC 123",
                    &format,
                    &bounds(4.0, 4.0, 216.0, 46.0),
                    &ink,
                );
                frame.finish()
            })
            .unwrap();
            let pixels = target.pixels().unwrap();
            assert!(pixels.chunks_exact(4).any(|p| p[0] > 0));
            assert!(
                pixels
                    .chunks_exact(4)
                    .all(|p| p[0] == p[1] && p[1] == p[2] && p[0] <= p[3])
            );
        }
    }

    fn bounds(left: f32, top: f32, right: f32, bottom: f32) -> c::Rect {
        c::Rect {
            left,
            top,
            right,
            bottom,
        }
    }
    const WHITE: c::ColorF = c::ColorF {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };

    #[test]
    fn icon_ink_stays_centered_for_mdl2_and_fluent_at_multiple_scales() {
        let device = c::GpuDevice::new_warp().unwrap();
        for family in ["Segoe MDL2 Assets", super::super::fonts::icon_family()] {
            let format = c::TextFormat::new(family, 20.0)
                .unwrap()
                .with_alignment(c::TextAlignment::Center)
                .with_paragraph_alignment(c::ParagraphAlignment::Center);
            for scale in [1.0, 1.5, 2.0] {
                let size = (64.0 * scale) as u32;
                let surface = Offscreen::new(&device, size, size).unwrap();
                for glyph in [
                    "\u{e790}", "\u{f0e2}", "\u{e721}", "\u{e70d}", "\u{e70e}", "\u{e73e}",
                    "\u{e8bb}", "\u{e713}", "\u{f2b7}", "\u{e8d2}", "\u{e8b7}",
                    "\u{e890}", "\u{e81c}", "\u{e946}", "\u{e9d9}",
                ] {
                    draw(&surface.target, scale, |frame| {
                        frame.clear(c::ColorF::new(0.0, 0.0, 0.0, 0.0));
                        let ink = canvas_result(frame.create_solid_brush(WHITE))?;
                        frame.clipped_icon(glyph, &format, &bounds(8.0, 8.0, 48.0, 48.0), &ink);
                        frame.finish()
                    })
                    .unwrap();
                    let pixels = surface.pixels().unwrap();
                    let rows: Vec<_> = (0..size as usize)
                        .filter(|y| {
                            pixels[y * size as usize * 4..(y + 1) * size as usize * 4]
                                .chunks_exact(4)
                                .any(|pixel| pixel[3] > 16)
                        })
                        .collect();
                    let center = (*rows.first().unwrap() + *rows.last().unwrap() + 1) as f32 / 2.0;
                    assert!(
                        (center - 28.0 * scale).abs() <= 1.0,
                        "{family} {glyph} scale={scale} ink center={center}"
                    );
                }
            }
        }
    }

    #[test]
    fn failed_bitmap_upload_unwinds_clip_and_draw_before_next_frame() {
        let device = c::GpuDevice::new_warp().unwrap();
        let surface = Offscreen::new(&device, 64, 64).unwrap();
        let failed = draw(&surface.target, 1.0, |frame| {
            frame.push_clip(&bounds(0.0, 0.0, 8.0, 8.0));
            canvas_result(frame.create_bitmap(&[0; 4], 4, 4))?;
            frame.finish()
        });
        assert!(failed.is_err());
        draw(&surface.target, 1.0, |frame| {
            frame.clear(c::ColorF::default());
            let brush = canvas_result(frame.create_solid_brush(WHITE))?;
            frame.fill_rect(&bounds(0.0, 0.0, 64.0, 64.0), &brush);
            frame.finish()
        })
        .unwrap();
        assert!(
            surface
                .pixels()
                .unwrap()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [255; 4])
        );
    }

    #[test]
    fn text_remains_inside_its_bounds_at_multiple_dpi() {
        let device = c::GpuDevice::new_warp().unwrap();
        let format = c::TextFormat::new("Microsoft YaHei UI", 24.0).unwrap();
        let format = format.with_word_wrapping(c::WordWrapping::NoWrap);
        for (scale, edge) in [(1.0, 32usize), (1.5, 48), (2.0, 64)] {
            let surface = Offscreen::new(&device, 192, 128).unwrap();
            draw(&surface.target, scale, |frame| {
                frame.clear(c::ColorF::default());
                let brush = canvas_result(frame.create_solid_brush(WHITE))?;
                frame.clipped_text(
                    "中文名称 ABCDEFG",
                    &format,
                    &bounds(0.0, 0.0, 32.0, 32.0),
                    &brush,
                );
                frame.finish()
            })
            .unwrap();
            let pixels = surface.pixels().unwrap();
            assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
            for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
                if index % 192 >= edge || index / 192 >= edge {
                    assert_eq!(pixel[3], 0, "text escaped the clipping rectangle");
                }
            }
        }
    }

    #[test]
    fn primitive_fade_matches_layer_alpha_and_restores_the_brush() {
        let _sta = crate::pane::test_support::apartment();
        let device = c::GpuDevice::new_warp().unwrap();
        let surface = Offscreen::new(&device, 96, 64).unwrap();
        let render = |layered| {
            draw(&surface.target, 1.25, |pass| {
                pass.clear(c::ColorF::new(0.0, 0.0, 0.0, 0.0));
                let ink = canvas_result(pass.create_solid_brush(c::ColorF::new(0.25, 0.5, 1.0, 0.5)))?;
                let shape = c::RoundedRect {
                    rect: c::Rect::from_xywh(8.0, 8.0, 24.0, 24.0), radius_x: 6.0, radius_y: 6.0,
                };
                if layered {
                    let _fade = pass.opacity_layer(&shape.rect, 0.5);
                    pass.fill_rounded_rect(&shape, &ink);
                } else {
                    pass.fill_highlight(&shape, &ink, 0.5);
                }
                pass.fill_rect(&c::Rect::from_xywh(40.0, 8.0, 24.0, 24.0), &ink);
                pass.finish()
            }).unwrap();
            surface.pixels().unwrap()
        };
        let old = render(true);
        let new = render(false);
        let center = |x: usize| (25 * 96 + x) * 4 + 3;
        assert_eq!(new[center(25)], 64);
        assert_eq!(new[center(60)], 128);
        assert!(new.iter().zip(old).all(|(a, b)| a.abs_diff(b) <= 1));
    }

    #[test]
    fn rounded_bitmap_brush_preserves_the_preview_mapping() {
        let _sta = crate::pane::test_support::apartment();
        let device = c::GpuDevice::new_warp().unwrap();
        let surface = Offscreen::new(&device, 96, 80).unwrap();
        let pixels: Vec<u8> = (0..12).flat_map(|y| {
            (0..16).flat_map(move |x| [x * 12, y * 16, 96, 255])
        }).collect();
        let shape = c::RoundedRect {
            rect: c::Rect::from_xywh(8.0, 8.0, 48.0, 36.0),
            radius_x: 8.0,
            radius_y: 8.0,
        };
        let render = |rounded| {
            draw(&surface.target, 1.25, |pass| {
                pass.clear(c::ColorF::default());
                if rounded {
                    let brush = pass.bitmap_brush(&pixels, 16, 12)?;
                    pass.fill_bitmap_rounded_rect(&shape, &brush);
                } else {
                    let bitmap = canvas_result(pass.create_bitmap(&pixels, 16, 12))?;
                    pass.draw_bitmap(&bitmap, &shape.rect, 1.0);
                }
                pass.finish()
            }).unwrap();
            surface.pixels().unwrap()
        };
        let original = render(false);
        let rounded = render(true);
        assert_eq!(rounded[(10 * 96 + 10) * 4 + 3], 0);
        for y in 20_usize..45 {
            let interior = (y * 96 + 20) * 4..(y * 96 + 60) * 4;
            assert!(rounded[interior.clone()].iter().zip(&original[interior]).all(|(a, b)| a.abs_diff(*b) <= 1));
        }
    }
}
