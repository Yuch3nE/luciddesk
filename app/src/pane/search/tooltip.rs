//! Search-row adapter for the shared native tooltip.
use super::*;

pub(super) struct Tooltip(super::super::tooltip::Tooltip);
impl Tooltip {
    pub(super) fn new(owner: HWND) -> Option<Self> {
        super::super::tooltip::Tooltip::new(owner).map(Self)
    }
    pub(super) fn hide(&mut self) {
        self.0.hide();
    }
    pub(super) fn show_for_row(&mut self, owner: HWND, text: &str, y: f32) {
        let s = scale(owner);
        self.0.show(
            text,
            RECT {
                left: (8.0 * s) as i32,
                top: (y * s) as i32,
                right: ((client_width(owner) - 8.0) * s) as i32,
                bottom: ((y + ROW) * s) as i32,
            },
        );
    }
}
