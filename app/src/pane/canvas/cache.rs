//! Renderer-owned, bounded scratch resources. No process/thread-global COM cache.
use super::{ColorLayout, c, canvas_result, text_ink_center_offset};
use luciddesk_window::control_style::ElevationGradient;
use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;
use windows::Win32::Graphics::Direct2D::{ID2D1DeviceContext, ID2D1LinearGradientBrush};

const BRUSH_LIMIT: usize = 64;
const ICON_LIMIT: usize = 64;
const GRADIENT_LIMIT: usize = 16;
const TEXT_LIMIT: usize = 512;
const STALE_TEXT_THRESHOLD: usize = 128;
const STALE_TEXT_FRAMES: u64 = 2;

struct Icon {
    text: String,
    format: c::TextFormat,
    size: (f32, f32),
    layout: c::TextLayout,
    offset: f32,
}

#[derive(Clone, Copy, Hash, Eq, PartialEq)]
struct TextKey {
    format: usize,
    width: u32,
    height: u32,
    text_hash: u64,
}

struct TextEntry {
    text: String,
    // Retain the immutable format to keep its COM identity from being recycled.
    _format: c::TextFormat,
    layout: c::TextLayout,
    color: Option<Rc<ColorLayout>>,
    used: u64,
    frame: u64,
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
                assert!(cache.borrow().creations.0 <= BRUSH_LIMIT);
                assert_eq!(cache.borrow().creations.1, iteration + 1);
            }
        }
        assert_eq!(cache.borrow().creations, (BRUSH_LIMIT, 4));
        let weak = Rc::downgrade(&cache.borrow().brushes[0].1);
        let other = canvas::Offscreen::new(&device, 480, 120).unwrap();
        frame(&other, Some(&cache), &format, 2.0, 0);
        assert!(weak.upgrade().is_none(), "old-context brushes must be released");
        assert_eq!(cache.borrow().creations, (BRUSH_LIMIT + 16, 5));
        let weak = Rc::downgrade(&cache.borrow().brushes[0].1);
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
            let first = canvas_result(pass.cached_solid_brush(c::ColorF::new(1.0, 0.0, 0.0, 1.0)))?;
            for _ in 0..100 {
                let same = canvas_result(pass.cached_solid_brush(c::ColorF::new(1.0, 0.0, 0.0, 1.0)))?;
                assert!(Rc::ptr_eq(&first, &same));
            }
            pass.finish()
        }).unwrap();
        assert_eq!(cache.borrow().brushes.len(), 1);
        assert_eq!(cache.borrow().creations.0, 1);
        canvas::draw_cached(&surface.target, 1.0, &cache, |pass| {
            let pinned = canvas_result(pass.cached_solid_brush(c::ColorF::new(1.0, 0.0, 0.0, 1.0)))?;
            for index in 1..=100 {
                canvas_result(pass.cached_solid_brush(c::ColorF::new(index as f32 / 100.0, 0.5, 0.25, 1.0)))?;
            }
            assert!(
                cache.borrow().brushes.iter().any(|(color, brush)| Rc::ptr_eq(brush, &pinned) && *color == c::ColorF::new(1.0, 0.0, 0.0, 1.0))
                    || Rc::strong_count(&pinned) == 1,
                "a brush still borrowed in the current pass must never be recolored"
            );
            drop(pinned);
            pass.finish()
        }).unwrap();
        let mut cache = cache.borrow_mut();
        assert_eq!(cache.brushes.len(), BRUSH_LIMIT);
        assert_eq!(cache.creations.0, BRUSH_LIMIT);
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
    fn cached_row_text_matches_draw_text_at_fractional_dpi() {
        let _sta = crate::pane::test_support::apartment();
        let device = c::GpuDevice::new_warp().unwrap();
        let surface = canvas::Offscreen::new(&device, 360, 150).unwrap();
        let cache = RefCell::new(FrameCache::default());
        for alignment in [c::TextAlignment::Leading, c::TextAlignment::Trailing] {
            let format = c::TextFormat::new("Segoe UI", 12.0).unwrap()
                .with_alignment(alignment)
                .with_paragraph_alignment(c::ParagraphAlignment::Center)
                .with_word_wrapping(c::WordWrapping::NoWrap);
            canvas::ellipsis(&format).unwrap();
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let draw = |cached| {
                    canvas::draw_cached(&surface.target, scale, &cache, |pass| {
                        pass.clear(c::ColorF::new(0.0, 0.0, 0.0, 0.0));
                        let ink = canvas_result(pass.cached_solid_brush(c::ColorF::new(1.0, 1.0, 1.0, 1.0)))?;
                        for (row, text) in ["A long document name · 项目资料", "مرحبا · Project", "12.5 MB"].iter().enumerate() {
                            let bounds = c::Rect::from_xywh(3.5, row as f32 * 23.0 + 2.5, 101.25, 22.0);
                            if cached {
                                pass.clipped_cached_text(text, &format, &bounds, &ink);
                            } else {
                                pass.clipped_text(text, &format, &bounds, &ink);
                            }
                        }
                        pass.finish()
                    }).unwrap();
                    surface.pixels().unwrap()
                };
                let original = draw(false);
                assert!(original.iter().any(|&channel| channel != 0));
                assert_eq!(draw(true), original, "alignment={alignment:?}, scale={scale}");
                assert_eq!(draw(true), original);
            }
        }
    }

    #[test]
    fn text_cache_keys_content_format_and_bounds_and_releases_on_scale_change() {
        let _sta = crate::pane::test_support::apartment();
        let device = c::GpuDevice::new_warp().unwrap();
        let surface = canvas::Offscreen::new(&device, 4, 4).unwrap();
        let mut cache = FrameCache::default();
        cache.begin(&surface.target, 1.0);
        let format = c::TextFormat::new("Segoe UI", 12.0).unwrap();
        let other = c::TextFormat::new("Segoe UI", 14.0).unwrap();
        let title = cache.color_text("📁 Title", &format, (80.0, 28.0)).unwrap();
        assert!(Rc::ptr_eq(&title, &cache.color_text("📁 Title", &format, (80.0, 28.0)).unwrap()));
        assert!(!Rc::ptr_eq(&title, &cache.color_text("📁 Title", &other, (80.0, 28.0)).unwrap()));
        assert!(!Rc::ptr_eq(&title, &cache.color_text("📁 Titles", &format, (80.0, 28.0)).unwrap()));
        assert!(!Rc::ptr_eq(&title, &cache.color_text("📁 Title", &format, (81.0, 28.0)).unwrap()));
        // Fill to exact capacity (4 entries above + TEXT_LIMIT - 4), touch the hot
        // title, then overflow by one to verify LRU keeps the hot entry alive.
        for width in 1..=(TEXT_LIMIT - 4) {
            cache.text_layout("row", &format, (width as f32, 24.0)).unwrap();
        }
        assert_eq!(cache.text.len(), TEXT_LIMIT);
        assert!(Rc::ptr_eq(&title, &cache.color_text("📁 Title", &format, (80.0, 28.0)).unwrap()));
        cache.text_layout("row", &format, ((TEXT_LIMIT + 1) as f32, 24.0)).unwrap();
        assert_eq!(cache.text.len(), TEXT_LIMIT);
        assert!(Rc::ptr_eq(&title, &cache.color_text("📁 Title", &format, (80.0, 28.0)).unwrap()));
        // Advancing past STALE_TEXT_FRAMES prunes idle resize entries while keeping active ones.
        for _ in 0..=STALE_TEXT_FRAMES {
            cache.begin(&surface.target, 1.0);
            assert!(Rc::ptr_eq(&title, &cache.color_text("📁 Title", &format, (80.0, 28.0)).unwrap()));
        }
        assert_eq!(cache.text.len(), 1);
        cache.begin(&surface.target, 1.25);
        assert!(cache.text.is_empty());
        assert_eq!(Rc::strong_count(&title), 1);
    }

    #[test]
    fn reused_elevation_gradient_renders_each_border_with_its_own_bounds() {
        use luciddesk_window::control_style::{ElevationKind, Palette};

        let _sta = crate::pane::test_support::apartment();
        let device = c::GpuDevice::new_warp().unwrap();
        let surface = canvas::Offscreen::new(&device, 220, 170).unwrap();
        let cache = RefCell::new(FrameCache::default());
        let stops = Palette::for_theme(false).elevation_gradient(ElevationKind::Control, 0.0, 0.0);
        let draw = |stable| {
            canvas::draw_cached(&surface.target, 1.25, &cache, |pass| {
                pass.clear(c::ColorF::new(1.0, 1.0, 1.0, 1.0));
                for (left, top, width, height) in [(10.0, 10.0, 85.0, 25.0), (55.0, 45.0, 115.0, 65.0)] {
                    let shape = c::RoundedRect {
                        rect: c::Rect::from_xywh(left, top, width, height),
                        radius_x: 5.0,
                        radius_y: 5.0,
                    };
                    pass.draw_elevation_border(&shape, stops, stable, 1.0);
                }
                pass.finish()
            }).unwrap();
            surface.pixels().unwrap()
        };
        let original = draw(false);
        assert!(original.iter().any(|&channel| channel < 255));
        assert_eq!(draw(true), original);
        let gradient = cache.borrow().gradients[0].1.clone();
        assert_eq!(draw(true), original);
        assert_eq!(cache.borrow().gradients.len(), 1);
        assert_eq!(cache.borrow().gradients[0].1, gradient);
    }

    #[test]
    #[ignore = "Manual offscreen benchmark; timings are not CI assertions"]
    fn rendering_hot_paths_latency() {
        let _sta = crate::pane::test_support::apartment();
        let device = c::GpuDevice::new().unwrap();
        let surface = canvas::Offscreen::new(&device, 700, 820).unwrap();
        let cache = RefCell::new(FrameCache::default());
        for layered in [true, false] {
            let mut times = Vec::with_capacity(360);
            for frame in 0..384 {
                let start = std::time::Instant::now();
                canvas::draw_cached(&surface.target, 1.25, &cache, |pass| {
                    pass.clear(c::ColorF::new(0.0, 0.0, 0.0, 0.0));
                    let ink = canvas_result(pass.cached_solid_brush(c::ColorF::new(1.0, 1.0, 1.0, 0.14)))?;
                    for row in 0..16 {
                        let shape = c::RoundedRect {
                            rect: c::Rect::from_xywh(6.0, row as f32 * 40.0 + 4.0, 540.0, 38.0),
                            radius_x: 4.0,
                            radius_y: 4.0,
                        };
                        let alpha = 0.2 + ((frame + row) % 11) as f32 * 0.05;
                        if layered {
                            let _fade = pass.opacity_layer(&shape.rect, alpha);
                            pass.fill_rounded_rect(&shape, &*ink);
                        } else {
                            pass.fill_highlight(&shape, &ink, alpha);
                        }
                    }
                    pass.finish()
                }).unwrap();
                if frame >= 24 { times.push(start.elapsed().as_micros()); }
            }
            times.sort_unstable();
            println!("highlight_fades layered={layered} p50_us={} p95_us={}", times[180], times[342]);
        }
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
    frame: u64,
    brushes: VecDeque<(c::ColorF, Rc<c::Brush>)>,
    icons: VecDeque<Icon>,
    gradients: VecDeque<(ElevationGradient, ID2D1LinearGradientBrush)>,
    text: HashMap<TextKey, TextEntry>,
    text_clock: u64,
    #[cfg(test)]
    creations: (usize, usize),
}

impl FrameCache {
    pub fn begin(&mut self, context: &c::ID2D1DeviceContext, scale: f32) {
        self.frame = self.frame.wrapping_add(1);
        let context_changed = self.context.as_ref() != Some(context);
        if context_changed {
            self.brushes.clear();
            self.gradients.clear();
            self.context = Some(context.clone());
        }
        if context_changed || self.scale != scale {
            self.icons.clear();
            self.clear_text();
            self.scale = scale;
        } else if self.text.len() > STALE_TEXT_THRESHOLD {
            let frame = self.frame;
            self.text.retain(|_, entry| frame.wrapping_sub(entry.frame) <= STALE_TEXT_FRAMES);
        }
    }

    pub fn clear_text(&mut self) {
        self.text.clear();
    }

    fn text_entry(&mut self, text: &str, format: &c::TextFormat, size: (f32, f32))
        -> canvas_core::Result<&mut TextEntry>
    {
        use canvas_core::Interface as _;
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let key = TextKey {
            format: format.raw().as_raw() as usize,
            width: size.0.to_bits(),
            height: size.1.to_bits(),
            text_hash: hasher.finish(),
        };
        self.text_clock = self.text_clock.wrapping_add(1);
        let clock = self.text_clock;
        let frame = self.frame;
        let hit = self.text.get_mut(&key).is_some_and(|entry| {
            if entry.text == text {
                entry.used = clock;
                entry.frame = frame;
                true
            } else {
                false
            }
        });
        if !hit {
            let layout = c::TextLayout::new(text, format, size.0, size.1)?;
            if !self.text.contains_key(&key) && self.text.len() >= TEXT_LIMIT {
                if let Some(oldest) = self
                    .text
                    .iter()
                    .min_by_key(|(_, entry)| entry.used)
                    .map(|(old_key, _)| *old_key)
                {
                    self.text.remove(&oldest);
                }
            }
            self.text.insert(key, TextEntry {
                text: text.into(),
                _format: format.clone(),
                layout,
                color: None,
                used: clock,
                frame,
            });
        }
        Ok(self.text.get_mut(&key).unwrap())
    }

    pub fn text_layout(&mut self, text: &str, format: &c::TextFormat, size: (f32, f32))
        -> canvas_core::Result<c::TextLayout>
    {
        Ok(self.text_entry(text, format, size)?.layout.clone())
    }

    pub fn color_text(&mut self, text: &str, format: &c::TextFormat, size: (f32, f32))
        -> windows::core::Result<Rc<ColorLayout>>
    {
        let scale = self.scale;
        let entry = canvas_result(self.text_entry(text, format, size))?;
        if let Some(prepared) = &entry.color {
            return Ok(prepared.clone());
        }
        let prepared = Rc::new(ColorLayout::new(text, &entry.layout, scale)?);
        entry.color = Some(prepared.clone());
        Ok(prepared)
    }

    pub fn gradient(&mut self, context: &ID2D1DeviceContext, stops: ElevationGradient)
        -> windows::core::Result<ID2D1LinearGradientBrush>
    {
        if let Some((_, brush)) = self.gradients.iter().find(|(key, _)| *key == stops) {
            return Ok(brush.clone());
        }
        let brush = super::create_elevation_gradient(context, stops)?;
        if self.gradients.len() == GRADIENT_LIMIT {
            self.gradients.pop_front();
        }
        self.gradients.push_back((stops, brush.clone()));
        Ok(brush)
    }

    // Deduplicate solid brushes by color. At capacity, recolor only an idle LRU
    // brush (strong_count == 1) so live brushes in the current pass never change.
    pub fn brush(&mut self, session: &c::DrawingSession<'_>, color: c::ColorF)
        -> canvas_core::Result<Rc<c::Brush>>
    {
        if let Some(index) = self.brushes.iter().position(|(cached, _)| *cached == color) {
            let entry = self.brushes.remove(index).unwrap();
            let brush = entry.1.clone();
            self.brushes.push_back(entry);
            return Ok(brush);
        }
        if self.brushes.len() < BRUSH_LIMIT {
            let brush = Rc::new(session.create_solid_brush(color)?);
            #[cfg(test)] { self.creations.0 += 1; }
            self.brushes.push_back((color, brush.clone()));
            return Ok(brush);
        }
        if let Some(index) = self.brushes.iter().position(|(_, brush)| Rc::strong_count(brush) == 1) {
            let (_, brush) = self.brushes.remove(index).unwrap();
            brush.set_color(color);
            self.brushes.push_back((color, brush.clone()));
            return Ok(brush);
        }
        #[cfg(test)] { self.creations.0 += 1; }
        session.create_solid_brush(color).map(Rc::new)
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
