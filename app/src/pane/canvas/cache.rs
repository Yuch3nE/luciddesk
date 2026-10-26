//! Renderer-owned, bounded scratch resources. No process/thread-global COM cache.
use super::{c, text_ink_center_offset};
use std::collections::VecDeque;

const BRUSH_LIMIT: usize = 64;
const ICON_LIMIT: usize = 64;

struct Icon {
    text: String,
    format: c::TextFormat,
    size: (f32, f32),
    layout: c::TextLayout,
    offset: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::{canvas, native_graphics::canvas_result};
    use std::cell::RefCell;

    fn frame(surface: &canvas::Offscreen, cache: Option<&RefCell<FrameCache>>,
        format: &c::TextFormat, scale: f32, phase: usize) {
        let paint = |pass: canvas::DrawPass<'_>| {
            pass.clear(c::ColorF::new(0.0, 0.0, 0.0, 0.0));
            for index in 0..16 {
                let brush = canvas_result(pass.cached_solid_brush(c::ColorF::new(
                    (phase % 10) as f32 / 10.0, index as f32 / 16.0, 0.7, 0.8)))?;
                let rect = c::Rect::from_xywh((index % 8 * 30 + phase % 3) as f32,
                    (index / 8 * 30) as f32, 28.0, 28.0);
                pass.fill_rect(&rect, &*brush);
                pass.clipped_icon("\u{e712}", format, &rect, &brush);
            }
            pass.finish()
        };
        match cache {
            Some(cache) => canvas::draw_cached(&surface.target, scale, cache, paint),
            None => canvas::draw(&surface.target, scale, paint),
        }.unwrap();
    }

    #[test]
    fn cached_frames_match_pixels_and_reuse_objects_across_animation() {
        let _sta = crate::pane::test_support::apartment();
        let device = c::GpuDevice::new_warp().unwrap();
        let surface = canvas::Offscreen::new(&device, 480, 120).unwrap();
        let format = c::TextFormat::new("Segoe Fluent Icons", 16.0).unwrap();
        let cache = RefCell::new(FrameCache::default());
        for (iteration, scale) in [1.0, 1.25, 1.5, 2.0].into_iter().enumerate() {
            for phase in 0..12 {
                frame(&surface, None, &format, scale, phase);
                let expected = surface.pixels().unwrap();
                frame(&surface, Some(&cache), &format, scale, phase);
                assert_eq!(expected, surface.pixels().unwrap());
                assert_eq!(cache.borrow().creations, (16, iteration + 1));
            }
        }
        let weak = std::rc::Rc::downgrade(&cache.borrow().brushes[0].1);
        let other = canvas::Offscreen::new(&device, 480, 120).unwrap();
        frame(&other, Some(&cache), &format, 2.0, 0);
        assert!(weak.upgrade().is_none(), "old-context brushes must be released");
        assert_eq!(cache.borrow().creations, (32, 5));
        let weak = std::rc::Rc::downgrade(&cache.borrow().brushes[0].1);
        drop(cache);
        assert!(weak.upgrade().is_none(), "renderer destruction releases cached brushes");
    }

    #[test]
    fn caches_are_bounded_and_icon_keys_include_format_and_bounds() {
        let _sta = crate::pane::test_support::apartment();
        let device = c::GpuDevice::new_warp().unwrap();
        let surface = canvas::Offscreen::new(&device, 4, 4).unwrap();
        let format = c::TextFormat::new("Segoe Fluent Icons", 16.0).unwrap();
        let other = c::TextFormat::new("Segoe MDL2 Assets", 20.0).unwrap();
        let cache = RefCell::new(FrameCache::default());
        canvas::draw_cached(&surface.target, 1.0, &cache, |pass| {
            for _ in 0..100 { canvas_result(pass.cached_solid_brush(c::ColorF::new(1.0, 0.0, 0.0, 1.0)))?; }
            pass.finish()
        }).unwrap();
        let mut cache = cache.borrow_mut();
        assert_eq!(cache.brushes.len(), BRUSH_LIMIT);
        for width in 1..=100 { cache.icon("\u{e712}", &format, (width as f32, 28.0)).unwrap(); }
        assert_eq!(cache.icons.len(), ICON_LIMIT);
        let before = cache.creations;
        cache.icon("\u{e712}", &format, (100.0, 28.0)).unwrap();
        assert_eq!(cache.creations, before);
        cache.icon("\u{e712}", &other, (100.0, 28.0)).unwrap();
        cache.icon("\u{e711}", &format, (100.0, 28.0)).unwrap();
        cache.icon("\u{e712}", &format, (100.0, 29.0)).unwrap();
        assert_eq!(cache.creations.1, before.1 + 3);
        cache.begin(&surface.target, 1.5);
        assert!(cache.icons.is_empty());
    }

    #[test]
    #[ignore = "Manual drawing benchmark; no timing thresholds in CI"]
    fn cached_and_uncached_frame_latency() {
        let _sta = crate::pane::test_support::apartment();
        let device = c::GpuDevice::new().unwrap();
        let surface = canvas::Offscreen::new(&device, 480, 120).unwrap();
        let format = c::TextFormat::new("Segoe Fluent Icons", 16.0).unwrap();
        let cache = RefCell::new(FrameCache::default());
        for cached in [false, true, false, true] {
            let state = cached.then_some(&cache);
            for phase in 0..30 { frame(&surface, state, &format, 1.5, phase); }
            let before = cache.borrow().creations;
            let mut times = Vec::new();
            for phase in 0..300 {
                let start = std::time::Instant::now();
                frame(&surface, state, &format, 1.5, phase);
                times.push(start.elapsed().as_micros());
            }
            times.sort_unstable();
            let after = cache.borrow().creations;
            println!("cached={cached} p50_us={} p95_us={} cached_brush_creations={} cached_layout_creations={}",
                times[150], times[285], after.0-before.0, after.1-before.1);
        }
    }
}

#[derive(Default)]
pub(crate) struct FrameCache {
    context: Option<c::ID2D1DeviceContext>,
    scale: f32,
    brushes: Vec<(c::ColorF, std::rc::Rc<c::Brush>)>,
    icons: VecDeque<Icon>,
    #[cfg(test)]
    creations: (usize, usize),
}

impl FrameCache {
    pub fn begin(&mut self, context: &c::ID2D1DeviceContext, scale: f32) {
        if self.context.as_ref() != Some(context) {
            self.brushes.clear();
            self.icons.clear();
            self.context = Some(context.clone());
        }
        if self.scale != scale {
            self.icons.clear();
            self.scale = scale;
        }
    }

    // Each draw-pass allocates distinct slots. Recolor only when that slot is
    // reused in a later pass; never mutate a brush already handed out this pass.
    pub fn brush(&mut self, session: &c::DrawingSession<'_>, slot: usize, color: c::ColorF)
        -> canvas_core::Result<std::rc::Rc<c::Brush>>
    {
        if let Some((old, brush)) = self.brushes.get_mut(slot) {
            if *old != color { brush.set_color(color); *old = color; }
            return Ok(brush.clone());
        }
        let brush = std::rc::Rc::new(session.create_solid_brush(color)?);
        #[cfg(test)] { self.creations.0 += 1; }
        if slot < BRUSH_LIMIT { self.brushes.push((color, brush.clone())); }
        Ok(brush)
    }

    // Only immutable icon formats are passed here. Retain the format itself so
    // a released COM pointer cannot be recycled into a false cache hit.
    pub fn icon(&mut self, text: &str, format: &c::TextFormat, size: (f32, f32))
        -> canvas_core::Result<(c::TextLayout, f32)>
    {
        if let Some(index) = self.icons.iter().position(|entry|
            entry.text == text && entry.format.raw() == format.raw() && entry.size == size)
        {
            let entry = self.icons.remove(index).unwrap();
            let result = (entry.layout.clone(), entry.offset);
            self.icons.push_back(entry);
            return Ok(result);
        }
        let layout = c::TextLayout::new(text, format, size.0, size.1)?;
        let offset = text_ink_center_offset(&layout).unwrap_or(0.0);
        #[cfg(test)] { self.creations.1 += 1; }
        if self.icons.len() == ICON_LIMIT { self.icons.pop_front(); }
        self.icons.push_back(Icon {
            text: text.into(), format: format.clone(), size, layout: layout.clone(), offset,
        });
        Ok((layout, offset))
    }
}
