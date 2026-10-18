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
