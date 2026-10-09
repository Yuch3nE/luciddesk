//! Fixed per-renderer palettes: reuse GPU brushes and recolor only changed slots.
use windows_canvas::{Brush, ColorF, DrawingSession, ID2D1DeviceContext};

pub(crate) struct Brushes<const N: usize> {
    cached: Option<(ID2D1DeviceContext, [ColorF; N], [Brush; N])>,
}

impl<const N: usize> Default for Brushes<N> {
    fn default() -> Self { Self { cached: None } }
}

impl<const N: usize> Brushes<N> {
    pub fn get(
        &mut self,
        context: &ID2D1DeviceContext,
        session: &DrawingSession<'_>,
        colors: [ColorF; N],
    ) -> canvas_core::Result<&[Brush; N]> {
        if self.cached.as_ref().is_none_or(|(old, _, _)| old != context) {
            // Release device-dependent resources before creating replacements.
            self.cached = None;
            let brushes: Vec<_> = colors.iter().map(|color| session.create_solid_brush(*color))
                .collect::<canvas_core::Result<_>>()?;
            self.cached = Some((context.clone(), colors, brushes.try_into().ok().unwrap()));
        }
        let (_, previous, brushes) = self.cached.as_mut().unwrap();
        for ((brush, old), color) in brushes.iter().zip(previous).zip(colors) {
            if *old != color {
                brush.set_color(color);
                *old = color;
            }
        }
        Ok(brushes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::{canvas, native_graphics::canvas_result};
    use windows_canvas::{Paint, Rect};

    #[test]
    fn palette_reuses_brushes_recolors_pixels_and_rebuilds_for_a_new_context() {
        let _sta = crate::pane::test_support::apartment();
        let device = windows_canvas::GpuDevice::new_warp().unwrap();
        let first = canvas::Offscreen::new(&device, 4, 4).unwrap();
        let second = canvas::Offscreen::new(&device, 4, 4).unwrap();
        let mut palette = Brushes::<1>::default();
        let mut draw = |surface: &canvas::Offscreen, color| {
            canvas::draw(&surface.target, 1.0, |frame| {
                let [brush] = canvas_result(palette.get(&surface.target, &frame, [color]))?;
                let identity = brush.as_raw_brush().clone();
                frame.fill_rect(&Rect::from_xywh(0.0, 0.0, 4.0, 4.0), brush);
                frame.finish()?;
                Ok(identity)
            }).unwrap()
        };
        let red = ColorF::new(1.0, 0.0, 0.0, 1.0);
        let blue = ColorF::new(0.0, 0.0, 1.0, 1.0);
        let identity = draw(&first, red);
        assert!(first.pixels().unwrap().chunks_exact(4).all(|p| p == [0, 0, 255, 255]));
        assert_eq!(draw(&first, red), identity);
        assert_eq!(draw(&first, blue), identity);
        assert!(first.pixels().unwrap().chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
        assert_ne!(draw(&second, red), identity);
        assert!(second.pixels().unwrap().chunks_exact(4).all(|p| p == [0, 0, 255, 255]));
    }
}
