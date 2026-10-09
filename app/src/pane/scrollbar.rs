//! Shared scrollbar geometry for painting, pointer hit testing, and scrolling.
use super::{GroupModel, layout};

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct State {
    pub hovered: bool,
    pub dragging: bool,
    pub expansion: f32,
}

impl State {
    pub fn animate(&mut self, motion: &mut super::animation::Motion, now: std::time::Instant,
        enabled: bool, visible: bool) -> bool {
        let target = if visible && (self.hovered || self.dragging) { 1.0 } else { 0.0 };
        self.expansion = motion.retarget_with_duration(target, now, enabled,
            super::animation::TOGGLE_DURATION).clamp(0.0, 1.0);
        (self.expansion - target).abs() > 0.0001
    }
}

pub(super) fn animations_enabled() -> bool {
    let mut enabled = 1i32;
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            windows_sys::Win32::UI::WindowsAndMessaging::SPI_GETCLIENTAREAANIMATION,
            0, (&raw mut enabled).cast(), 0);
    }
    enabled != 0
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Bar {
    pub left: f32,
    pub top: f32,
    pub height: f32,
    pub thumb_top: f32,
    pub thumb_height: f32,
    pub max: usize,
    pub page: usize,
}

impl Bar {
    pub const WIDTH: f32 = 11.0;
    pub fn for_model(model: &GroupModel, width: f32, height: f32) -> Option<Self> {
        if model.collapsed || model.reveal < 1.0 { return None; }
        Self::for_grid(model, model.grid(width, height), width, height)
    }
    pub fn for_grid(model: &GroupModel, grid: layout::Grid, width: f32, height: f32) -> Option<Self> {
        if model.collapsed || model.reveal < 1.0 { return None; }
        let max = grid.max_scroll(model.items.len());
        let top = grid.content_top + 4.0;
        let height = height - layout::PADDING - top;
        if max == 0 || height <= 0.0 || width < 32.0 { return None; }
        let page = grid.visible_rows.max(1);
        let visible = if model.free_layout && !model.is_list() { page } else { model.content_rows(grid).saturating_sub(max).max(1) };
        let thumb_height = (height * visible as f32 / (max + visible) as f32).max(16.0).min(height);
        Some(Self {
            left: width - 16.0, top, height, thumb_height, max, page,
            thumb_top: top + (height - thumb_height) * model.scroll.min(max) as f32 / max as f32,
        })
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        (self.left..self.left + Self::WIDTH).contains(&x)
            && (self.top..self.top + self.height).contains(&y)
    }
    pub fn on_thumb(self, y: f32) -> bool {
        (self.thumb_top..self.thumb_top + self.thumb_height).contains(&y)
    }
    pub fn drag_to(self, y: f32, grab_offset: f32) -> usize {
        let travel = self.height - self.thumb_height;
        if travel <= 0.0 { return 0; }
        (((y - grab_offset - self.top) / travel).clamp(0.0, 1.0) * self.max as f32).round() as usize
    }
    pub fn page_to(self, scroll: usize, y: f32) -> usize {
        if y < self.thumb_top { scroll.saturating_sub(self.page) }
        else { scroll.saturating_add(self.page).min(self.max) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hover_expands_and_reverses_smoothly_and_drag_keeps_it_open() {
        let _sta = luciddesk_shell::ShellApartment::initialize_sta().unwrap();
        let start = std::time::Instant::now();
        let at = |ms| start + std::time::Duration::from_millis(ms);
        let mut state = State { hovered: true, ..Default::default() };
        let mut motion = super::super::animation::Motion::settled(0.0, start);
        assert!(state.animate(&mut motion, start, true, true));
        assert_eq!(state.expansion, 0.0);
        assert!(state.animate(&mut motion, at(60), true, true));
        let expanded = state.expansion;
        assert!(expanded > 0.0 && expanded < 1.0);
        state.hovered = false;
        assert!(state.animate(&mut motion, at(60), true, true));
        assert!((state.expansion - expanded).abs() < 0.0001);
        assert!(state.animate(&mut motion, at(90), true, true));
        let shrinking = state.expansion;
        assert!(shrinking > 0.0 && shrinking < expanded);
        state.dragging = true;
        assert!(state.animate(&mut motion, at(90), true, true));
        assert!((state.expansion - shrinking).abs() < 0.0001);
        assert!(!state.animate(&mut motion, at(250), true, true));
        assert_eq!(state.expansion, 1.0);
        state.dragging = false;
        assert!(!state.animate(&mut motion, at(260), false, true));
        assert_eq!(state.expansion, 0.0);
        state.hovered = true;
        assert!(!state.animate(&mut motion, at(270), true, false));
        assert_eq!(state.expansion, 0.0);
    }
}
