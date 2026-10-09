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
    pub horizontal: bool,
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
        let height = height - model.horizontal_inset(width) - layout::PADDING - top;
        if max == 0 || height <= 0.0 || width < 32.0 { return None; }
        let page = grid.visible_rows.max(1);
        let visible = if model.free_layout && !model.is_list() { page } else { model.content_rows(grid).saturating_sub(max).max(1) };
        let thumb_height = (height * visible as f32 / (max + visible) as f32).max(16.0).min(height);
        Some(Self {
            horizontal: false,
            left: width - 16.0, top, height, thumb_height, max, page,
            thumb_top: top + (height - thumb_height) * model.scroll.min(max) as f32 / max as f32,
        })
    }
    pub fn horizontal(model: &GroupModel, width: f32, height: f32) -> Option<Self> {
        let max = model.horizontal_max(width).ceil() as usize;
        if model.collapsed || model.reveal < 1.0 || max == 0 || width < 32.0 { return None; }
        let page = (width - layout::PADDING * 2.0).max(1.0) as usize;
        let length = (width - 32.0).max(1.0);
        let thumb_height = (length * page as f32 / (max + page) as f32).max(16.0).min(length);
        Some(Self { horizontal: true, left: height - 16.0, top: 12.0, height: length,
            thumb_top: 12.0 + (length - thumb_height) * model.horizontal_offset(width) / max as f32,
            thumb_height, max, page })
    }
    pub fn axis(self, x: f32, y: f32) -> f32 { if self.horizontal { x } else { y } }
    pub fn rect(self, x: f32, y: f32, width: f32, height: f32) -> (f32, f32, f32, f32) {
        if self.horizontal { (y, x, height, width) } else { (x, y, width, height) }
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        let (x, y) = if self.horizontal { (y, x) } else { (x, y) };
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
        let _sta = crate::pane::test_support::apartment();
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

#[cfg(test)]
mod horizontal_tests {
    use super::*;
    use luciddesk_core::{PanelId, GridPosition, PointDip};

    #[test]
    fn narrow_manual_panes_scroll_without_moving_icons_and_keep_coordinates_reversible() {
        let _sta = crate::pane::test_support::apartment();
        for free in [false, true] {
            let mut state = crate::pane::tests::test_state();
            let id = PanelId::new(1);
            crate::pane::fixed_grid::toggle(&mut state.workspace, id).unwrap();
            let key = crate::pane::ordered_desktop_items(&state.workspace, id)[0].identity().persistent_key();
            crate::pane::fixed_grid::place(&mut state.workspace, id, &[key.clone()], GridPosition::new(7, 1));
            if free { crate::pane::free_layout::toggle(&mut state.workspace, id).unwrap(); }
            let positions = state.workspace.desktop_items().to_vec();
            let mut rect = state.workspace.panel(id).unwrap().rect();
            rect.width = 220.0;
            state.workspace.panel_mut(id).unwrap().set_rect(rect);
            state.store.save_workspace(&state.workspace).unwrap();
            let restored = state.store.load_workspace().unwrap();
            assert_eq!(restored.desktop_items(), positions);
            assert_eq!(restored.panel(id).unwrap().rect().width, 220.0);
            let mut model = crate::pane::create_model(&state, id).unwrap();
            let index = model.items.iter().position(|i| i.identity.persistent_key() == key).unwrap();
            let grid = model.grid(220.0, 350.0);
            assert!(grid.horizontal_limit > 0.0);
            assert_eq!(grid.columns, crate::pane::fixed_grid::columns(&state.workspace, id));
            model.ensure_visible(grid, index, 1.0);
            let bar = Bar::horizontal(&model, 220.0, 350.0).unwrap();
            assert!(bar.contains(bar.thumb_top + 1.0, bar.left + 1.0));
            assert_eq!(bar.drag_to(bar.top + bar.height - bar.thumb_height, 0.0), bar.max);
            let grid = model.grid(220.0, 350.0);
            let (x, y) = model.cell(grid, index);
            assert!(x >= layout::PADDING - 0.01 && x + grid.cell_width <= 220.0 - layout::PADDING + 0.01);
            for dpi in [1.0, 1.25, 1.5, 2.0] {
                let px = ((x + 8.0) * dpi).round() / dpi;
                let py = ((y + 8.0) * dpi).round() / dpi;
                assert_eq!(model.hit(grid, px, py, dpi), Some(index));
            }
            let (slot, destination) = model.drop_destination(grid, x + 8.0, y + 8.0, PointDip::new(8.0, 8.0));
            if free {
                let expected = model.items[index].details.free_position.unwrap();
                let actual = destination.unwrap();
                assert!((actual.x - expected.x).abs() < 0.01 && (actual.y - expected.y).abs() < 0.01);
            } else {
                assert_eq!(crate::pane::fixed_grid::position(slot, grid.columns), GridPosition::new(7, 1));
            }
            assert!(Bar::horizontal(&model, model.content_width() + 10.0, 350.0).is_none());
            model.list_view = true;
            assert_eq!(model.horizontal_offset(220.0), 0.0);
            assert!(Bar::horizontal(&model, 220.0, 350.0).is_none());
        }
    }
}

#[cfg(test)]
#[test]
fn native_resize_and_horizontal_wheel_do_not_change_saved_icon_positions() {
    use crate::pane::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let _sta = test_support::apartment();
    let mut app = tests::test_state();
    let id = PanelId::new(1);
    fixed_grid::toggle(&mut app.workspace, id).unwrap();
    let key = ordered_desktop_items(&app.workspace, id)[0].identity().persistent_key();
    fixed_grid::place(&mut app.workspace, id, &[key], GridPosition::new(9, 0));
    let mut rect = app.workspace.panel(id).unwrap().rect();
    rect.width = 220.0;
    app.workspace.panel_mut(id).unwrap().set_rect(rect);
    let state = Rc::new(RefCell::new(app));
    create_view(&state, id).unwrap();
    let (hwnd, model, positions, writes) = {
        let s = state.borrow();
        (s.views[0].window.hwnd().cast(), Rc::clone(&s.views[0].model), s.workspace.desktop_items().to_vec(), s.store.change_count())
    };
    let mut limits = MINMAXINFO::default();
    unsafe { SendMessageW(hwnd, WM_GETMINMAXINFO, 0, (&raw mut limits) as isize); }
    let dpi = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
    assert!(limits.ptMinTrackSize.x as f32 / dpi < model.borrow().content_width());
    unsafe { SendMessageW(hwnd, WM_MOUSEHWHEEL, (120u32 << 16) as usize, 0); }
    assert!(model.borrow().scroll_x > 0.0);
    let right = model.borrow().scroll_x;
    unsafe { SendMessageW(hwnd, WM_MOUSEWHEEL, ((120u32 << 16) | 4) as usize, 0); }
    assert!(model.borrow().scroll_x < right);
    let s = state.borrow();
    assert_eq!(s.workspace.desktop_items(), positions);
    assert_eq!(s.store.change_count(), writes);
}
