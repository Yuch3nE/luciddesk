//! Borrow a host draw pass; the host owns BeginDraw/EndDraw and the composition surface.
use windows::Win32::Graphics::{Direct2D::Common::D2D_RECT_F, Direct2D::*, DirectWrite::*};
use windows::core::{Interface, Result};
use windows_canvas as c;

pub fn canvas_result<T>(r: canvas_core::Result<T>) -> Result<T> {
    r.map_err(|e| windows::core::Error::from_hresult(windows::core::HRESULT(e.code().0)))
}
pub fn ellipsis(format: &c::TextFormat) -> Result<()> {
    use canvas_core::Interface as _;
    unsafe {
        let ptr = format.raw().as_raw();
        let native = IDWriteTextFormat::from_raw_borrowed(&ptr).unwrap();
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let sign = factory.CreateEllipsisTrimmingSign(native)?;
        native.SetTrimming(
            &DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                ..Default::default()
            },
            &sign,
        )
    }
}
/// Renderer-local, bounded solid colors; no resources cross the plugin boundary.
#[derive(Default)]
pub(crate) struct Brushes {
    context: Option<ID2D1DeviceContext>,
    colors: Vec<(c::ColorF, std::rc::Rc<c::Brush>)>,
}
impl Brushes {
    pub fn get(&mut self, target: &DrawPass<'_>, color: c::ColorF) -> canvas_core::Result<std::rc::Rc<c::Brush>> {
        if self.context.as_ref() != Some(target.native) {
            self.colors.clear();
            self.context = Some(target.native.clone());
        }
        if let Some((_, brush)) = self.colors.iter().find(|(cached, _)| *cached == color) {
            return Ok(brush.clone());
        }
        let brush = std::rc::Rc::new(target.create_solid_brush(color)?);
        // Theme changes or future dynamic colors must not grow the cache forever.
        if self.colors.len() == 64 { self.colors.clear(); }
        self.colors.push((color, brush.clone()));
        Ok(brush)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas_core::Interface as _;

    #[test]
    fn brushes_reuse_colors_bound_growth_and_rebuild_on_context_change() {
        let device = c::GpuDevice::new_warp().unwrap();
        let first = device.create_render_target(4, 4).unwrap();
        let second = device.create_render_target(4, 4).unwrap();
        let mut cache = Brushes::default();
        let red = c::ColorF::new(1.0, 0.0, 0.0, 1.0);
        let mut saved = None;
        first.draw(|session| {
            let ptr = session.raw().as_raw();
            let native = unsafe { ID2D1DeviceContext::from_raw_borrowed(&ptr).unwrap() };
            let target = DrawPass::new(session.raw(), native);
            let brush = cache.get(&target, red)?;
            assert!(std::rc::Rc::ptr_eq(&brush, &cache.get(&target, red)?));
            let blue = cache.get(&target, c::ColorF::new(0.0, 0.0, 1.0, 1.0))?;
            assert!(!std::rc::Rc::ptr_eq(&brush, &blue));
            assert!(std::rc::Rc::ptr_eq(&brush, &cache.get(&target, red)?));
            for i in 0..130 {
                cache.get(&target, c::ColorF::new(0.0, i as f32 / 130.0, 1.0, 1.0))?;
                assert!(cache.colors.len() <= 64);
            }
            // A brush in use survives eviction while a frame is being drawn.
            target.fill_rect(&c::Rect::from_xywh(0.0, 0.0, 2.0, 4.0), &*brush);
            target.fill_rect(&c::Rect::from_xywh(2.0, 0.0, 2.0, 4.0), &*blue);
            saved = Some(cache.get(&target, red)?);
            Ok(())
        }).unwrap();
        for row in first.read_pixels().unwrap().chunks_exact(16) {
            assert!(row[..8].chunks_exact(4).all(|p| p == [0, 0, 255, 255]));
            assert!(row[8..].chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
        }
        second.draw(|session| {
            let ptr = session.raw().as_raw();
            let native = unsafe { ID2D1DeviceContext::from_raw_borrowed(&ptr).unwrap() };
            let target = DrawPass::new(session.raw(), native);
            let brush = cache.get(&target, red)?;
            assert!(!std::rc::Rc::ptr_eq(saved.as_ref().unwrap(), &brush));
            assert_eq!(cache.colors.len(), 1);
            target.fill_rect(&c::Rect::from_xywh(0.0, 0.0, 4.0, 4.0), &*brush);
            Ok(())
        }).unwrap();
        assert!(second.read_pixels().unwrap().chunks_exact(4).all(|p| p == [0, 0, 255, 255]));
    }
}

pub struct DrawPass<'a> {
    session: c::DrawingSession<'a>,
    native: &'a ID2D1DeviceContext,
    clips: std::cell::Cell<usize>,
}
impl<'a> DrawPass<'a> {
    pub fn new(context: &'a c::ID2D1DeviceContext, native: &'a ID2D1DeviceContext) -> Self {
        Self {
            session: c::DrawingSession::from_borrowed_context(context, c::Matrix3x2::identity()),
            native,
            clips: std::cell::Cell::new(0),
        }
    }
    /// A single native arc, starting at twelve o'clock; reuse the host factory/device.
    pub fn progress_arc(
        &self,
        center: c::Vector2,
        radius: f32,
        fraction: f32,
        brush: &c::Brush,
        width: f32,
    ) -> Result<()> {
        use c::Paint;
        use canvas_core::Interface as _;
        if fraction <= 0.0 {
            return Ok(());
        }
        if fraction >= 1.0 {
            self.draw_ellipse(&c::Ellipse::circle(center, radius), brush, width);
            return Ok(());
        }
        let angle = fraction * std::f32::consts::TAU;
        unsafe {
            let geometry = self.native.GetFactory()?.CreatePathGeometry()?;
            let sink = geometry.Open()?;
            sink.BeginFigure(
                windows_numerics::Vector2 {
                    X: center.x,
                    Y: center.y - radius,
                },
                Common::D2D1_FIGURE_BEGIN_HOLLOW,
            );
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: windows_numerics::Vector2 {
                    X: center.x + radius * angle.sin(),
                    Y: center.y - radius * angle.cos(),
                },
                size: Common::D2D_SIZE_F {
                    width: radius,
                    height: radius,
                },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if fraction > 0.5 {
                    D2D1_ARC_SIZE_LARGE
                } else {
                    D2D1_ARC_SIZE_SMALL
                },
            });
            sink.EndFigure(Common::D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            let ptr = brush.as_raw_brush().as_raw();
            let native_brush = ID2D1Brush::from_raw_borrowed(&ptr).unwrap();
            self.native
                .DrawGeometry(&geometry, native_brush, width, None);
        }
        Ok(())
    }
    /// Build chart geometry with the host factory; no separate GPU device is needed.
    pub fn fill_polygon(
        &self,
        points: impl IntoIterator<Item = c::Vector2>,
        brush: &c::Brush,
    ) -> Result<()> {
        use c::Paint;
        use canvas_core::Interface as _;
        let mut points = points.into_iter();
        let Some(first) = points.next() else {
            return Ok(());
        };
        unsafe {
            let geometry = self.native.GetFactory()?.CreatePathGeometry()?;
            let sink = geometry.Open()?;
            sink.BeginFigure(
                windows_numerics::Vector2 {
                    X: first.x,
                    Y: first.y,
                },
                Common::D2D1_FIGURE_BEGIN_FILLED,
            );
            for point in points {
                sink.AddLine(windows_numerics::Vector2 {
                    X: point.x,
                    Y: point.y,
                });
            }
            sink.EndFigure(Common::D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
            let ptr = brush.as_raw_brush().as_raw();
            let brush = ID2D1Brush::from_raw_borrowed(&ptr).unwrap();
            self.native.FillGeometry(&geometry, brush, None);
        }
        Ok(())
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
        }
        self.clips.set(self.clips.get() + 1);
    }
    pub fn pop_clip(&self) {
        assert!(self.clips.get() > 0);
        unsafe {
            self.native.PopAxisAlignedClip();
        }
        self.clips.set(self.clips.get() - 1);
    }
    pub fn clipped_text(
        &self,
        text: &str,
        format: &c::TextFormat,
        rect: &c::Rect,
        brush: &c::Brush,
    ) {
        self.push_clip(rect);
        self.session.draw_text(text, format, rect, brush);
        self.pop_clip();
    }
}
impl<'a> std::ops::Deref for DrawPass<'a> {
    type Target = c::DrawingSession<'a>;
    fn deref(&self) -> &Self::Target {
        &self.session
    }
}
impl Drop for DrawPass<'_> {
    fn drop(&mut self) {
        while self.clips.get() > 0 {
            self.pop_clip();
        }
    }
}
