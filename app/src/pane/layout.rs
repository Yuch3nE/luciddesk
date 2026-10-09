//! A single layout is shared by painting, hit testing, scrolling and keyboard navigation.
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub viewport_width: f32,
    pub horizontal_limit: f32,
    pub content_top: f32,
    pub columns: usize,
    pub cell_width: f32,
    pub cell_height: f32,
    pub icon_size: f32,
    pub text_scale: f32,
    pub visible_rows: usize,
    pub scroll_limit: Option<usize>,
}

pub const DESKTOP_ICON_SIZE: f32 = 48.0;

pub const HEADER: f32 = 40.0;
pub const HEADER_INSET: f32 = 6.0;
pub const LIST_HEADER: f32 = 28.0;
pub const LIST_ROW: f32 = 32.0;
pub const LIST_CELL_WIDTH: f32 = 396.0;

/// Relative boundaries for name, type, modified date, size, and the right edge.
pub fn list_columns(width: f32) -> [f32; 5] {
    let icon = 30.0_f32.min(width * 0.2);
    let content = (width - icon).max(0.0);
    let modified = (content * 0.33).clamp(90.0, 150.0).min(content * 0.34);
    let kind = (content * 0.20).clamp(60.0, 120.0).min(content * 0.22);
    let size = (content * 0.18).clamp(56.0, 80.0).min(content * 0.22);
    [
        icon,
        width - modified - kind - size,
        width - modified - size,
        width - size,
        width,
    ]
}
// Label top relative to the icon-size baseline, shared by paint and geometry.
pub const LABEL_OFFSET: f32 = 6.0;
pub const PADDING: f32 = 12.0;
pub const HEADER_BUTTONS_WIDTH: f32 = 70.0;

/// Shared grid metrics for rendering and the first desktop pane.
pub fn desktop_grid(width: f32, height: f32, icon_size: f32, grid_scale: f32) -> Grid {
    let factor = grid_scale / 100.0;
    let mut grid = Grid::system(width, height, icon_size, (88.0, 96.0));
    grid.cell_width *= factor;
    grid.cell_height *= factor;
    grid.icon_size *= factor;
    grid.text_scale = factor;
    grid.columns = ((width - PADDING * 2.0) / grid.cell_width).floor().max(1.0) as usize;
    grid.visible_rows = ((height - grid.content_top - PADDING) / grid.cell_height).floor().max(1.0) as usize;
    grid
}

pub fn title_area(width: f32) -> (f32, f32) {
    // Relax the left margin gradually on narrow panes. Switching between two
    // layouts at a fixed width makes the title grow while the pane shrinks.
    let right = width - HEADER_BUTTONS_WIDTH - 4.0;
    let left = ((width - 80.0) / 2.0).clamp(14.0, HEADER_BUTTONS_WIDTH + 4.0);
    (left, (right - left).max(1.0))
}

pub fn header_button_x(width: f32, button: usize) -> f32 {
    if button >= 2 { return 6.0 + (button - 2) as f32 * 32.0; }
    // Visual order: collapse, menu.
    width - HEADER_BUTTONS_WIDTH + button as f32 * 32.0
}

pub fn header_button(width: f32, x: f32, y: f32) -> Option<usize> {
    if !(HEADER_INSET..HEADER - HEADER_INSET).contains(&y) {
        return None;
    }
    (0..2).find(|&button| {
        let left = header_button_x(width, button);
        (left..left + 28.0).contains(&x)
    })
}

impl Grid {
    pub fn list(width: f32, height: f32) -> Self {
        let top = HEADER + LIST_HEADER;
        Self {
            viewport_width: width,
            horizontal_limit: 0.0,
            content_top: top,
            columns: 1,
            cell_width: (width - PADDING * 2.0).max(1.0),
            cell_height: LIST_ROW,
            icon_size: 20.0,
            text_scale: 1.0,
            visible_rows: ((height - top - PADDING) / LIST_ROW).floor().max(1.0) as usize,
            scroll_limit: None,
        }
    }
    pub fn system(width: f32, height: f32, icon_size: f32, spacing: (f32, f32)) -> Self {
        let cell_width = spacing.0.max(icon_size + 16.0);
        let cell_height = spacing.1.max(icon_size + 34.0);
        Self {
            viewport_width: width,
            horizontal_limit: 0.0,
            content_top: HEADER + PADDING,
            scroll_limit: None,
            columns: ((width - PADDING * 2.0) / cell_width).floor().max(1.0) as usize,
            cell_width,
            cell_height,
            icon_size,
            text_scale: 1.0,
            visible_rows: ((height - HEADER - PADDING * 2.0) / cell_height)
                .floor()
                .max(1.0) as usize,
        }
    }
    #[allow(clippy::cast_precision_loss)]
    pub fn cell(self, index: usize, scroll: usize) -> (f32, f32) {
        (
            PADDING + (index % self.columns) as f32 * self.cell_width,
            self.content_top + ((index / self.columns) as f32 - scroll as f32) * self.cell_height,
        )
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn hit(self, x: f32, y: f32, scroll: usize, count: usize) -> Option<usize> {
        if x < PADDING || y < self.content_top {
            return None;
        }
        let column = ((x - PADDING) / self.cell_width).floor() as usize;
        if column >= self.columns {
            return None;
        }
        let row = ((y - self.content_top) / self.cell_height).floor() as usize + scroll;
        let index = row * self.columns + column;
        (index < count).then_some(index)
    }

    /// Candidate cells intersecting a vertical viewport; callers retain exact clipping.
    pub fn visible_indices(self, scroll: usize, top: f32, bottom: f32, count: usize) -> std::ops::Range<usize> {
        let first = ((top - self.content_top) / self.cell_height + scroll as f32).floor().max(0.0) as usize;
        let end = ((bottom - self.content_top) / self.cell_height + scroll as f32).ceil().max(0.0) as usize;
        let start = first.saturating_mul(self.columns).min(count);
        start..end.saturating_mul(self.columns).min(count).max(start)
    }

    pub fn max_scroll(self, count: usize) -> usize {
        if let Some(limit) = self.scroll_limit {
            return limit;
        }
        count
            .div_ceil(self.columns)
            .saturating_sub(self.visible_rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shrinking_title_space_is_continuous_across_dpi_and_icon_thresholds() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for icon_width in [0.0_f32, 24.0] {
                let mut previous = f32::MAX;
                for pixels in (112..=900).rev() {
                    let (_, area) = title_area(pixels as f32 / scale);
                    let available = (area - icon_width).max(1.0);
                    assert!(available <= previous, "shrinking must not reveal more text");
                    if previous != f32::MAX {
                        assert!(previous - available <= 1.0 / scale + 0.001);
                    }
                    previous = available;
                }
            }
        }
    }
    #[test]
    fn resize_reflows_and_hit_testing_tracks_scrolled_rows() {
        let wide = Grid::system(500.0, 260.0, 48.0, (88.0, 96.0));
        let narrow = Grid::system(290.0, 260.0, 48.0, (88.0, 96.0));
        assert_eq!(wide.columns, 5);
        assert_eq!(narrow.columns, 3);
        let (x, y) = narrow.cell(7, 1);
        assert_eq!(narrow.hit(x + 20.0, y + 20.0, 1, 12), Some(7));
        assert_eq!(narrow.hit(20.0, 20.0, 0, 12), None);
        assert_eq!(narrow.hit(280.0, 60.0, 0, 12), None);
        assert_eq!(narrow.max_scroll(12), 2);
    }
}

/// Resize freely, snapping within 8 DIP of whole cells while retaining the opposite edge.
/// Account for header,
/// padding and physical-pixel rounding at fractional DPI.
pub fn resize_pane(
    rect: &mut windows_sys::Win32::Foundation::RECT,
    edge: u32,
    cell: (f32, f32),
    scale: f32,
    collapsed: bool,
    row_contents: &[f32],
) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let extent = |pixels: i32, step: f32, inset: f32| {
        let cells = ((pixels as f32 / scale - inset) / step).round().max(1.0);
        let minimum = ((inset + step) * scale).ceil() as i32;
        let proposed = pixels.max(minimum);
        let snapped = ((inset + cells * step) * scale).ceil() as i32;
        if (proposed - snapped).abs() as f32 <= 8.0 * scale {
            snapped
        } else {
            proposed
        }
    };
    if matches!(
        edge,
        WMSZ_LEFT | WMSZ_RIGHT | WMSZ_TOPLEFT | WMSZ_TOPRIGHT | WMSZ_BOTTOMLEFT | WMSZ_BOTTOMRIGHT
    ) {
        let width = extent(rect.right - rect.left, cell.0, PADDING * 2.0);
        if matches!(edge, WMSZ_LEFT | WMSZ_TOPLEFT | WMSZ_BOTTOMLEFT) {
            rect.left = rect.right - width;
        } else {
            rect.right = rect.left + width;
        }
    }
    if matches!(
        edge,
        WMSZ_TOP | WMSZ_BOTTOM | WMSZ_TOPLEFT | WMSZ_TOPRIGHT | WMSZ_BOTTOMLEFT | WMSZ_BOTTOMRIGHT
    ) {
        let height = if collapsed {
            (HEADER * scale).ceil() as i32
        } else if row_contents.is_empty() {
            extent(rect.bottom - rect.top, cell.1, HEADER + PADDING * 2.0)
        } else {
            snap_content_height(rect.bottom - rect.top, cell.1, row_contents, scale)
        };
        if matches!(edge, WMSZ_TOP | WMSZ_TOPLEFT | WMSZ_TOPRIGHT) {
            rect.top = rect.bottom - height;
        } else {
            rect.bottom = rect.top + height;
        }
    }
}

#[cfg(test)]
mod resize_tests {
    use super::*;
    use windows_sys::Win32::{Foundation::RECT, UI::WindowsAndMessaging::*};

    #[test]
    fn every_resize_edge_preserves_anchor_and_snaps_at_fractional_dpi() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for edge in [
                WMSZ_LEFT,
                WMSZ_RIGHT,
                WMSZ_TOP,
                WMSZ_BOTTOM,
                WMSZ_TOPLEFT,
                WMSZ_TOPRIGHT,
                WMSZ_BOTTOMLEFT,
                WMSZ_BOTTOMRIGHT,
            ] {
                let mut r = RECT {
                    left: -500,
                    top: -200,
                    right: -500 + (293.0 * scale) as i32,
                    bottom: -200 + (259.0 * scale) as i32,
                };
                let old = r;
                resize_pane(&mut r, edge, (88.0, 96.0), scale, false, &[]);
                if !matches!(edge, WMSZ_TOP | WMSZ_BOTTOM) {
                    assert_eq!(
                        r.right - r.left,
                        ((24.0 + 3.0 * 88.0) * scale).ceil() as i32
                    );
                    if matches!(edge, WMSZ_LEFT | WMSZ_TOPLEFT | WMSZ_BOTTOMLEFT) {
                        assert_eq!(r.right, old.right);
                    } else {
                        assert_eq!(r.left, old.left);
                    }
                } else {
                    assert_eq!((r.left, r.right), (old.left, old.right));
                }
                if !matches!(edge, WMSZ_LEFT | WMSZ_RIGHT) {
                    assert_eq!(
                        r.bottom - r.top,
                        ((64.0 + 2.0 * 96.0) * scale).ceil() as i32
                    );
                    if matches!(edge, WMSZ_TOP | WMSZ_TOPLEFT | WMSZ_TOPRIGHT) {
                        assert_eq!(r.bottom, old.bottom);
                    } else {
                        assert_eq!(r.top, old.top);
                    }
                } else {
                    assert_eq!((r.top, r.bottom), (old.top, old.bottom));
                }
            }
            // Away from the grid, retain the exact requested size. Moving beyond
            // the capture distance releases the snap instead of forcing another cell.
            for width in [268.0, 308.0, 332.0] {
                let mut free = RECT {
                    left: -100,
                    top: -50,
                    right: -100 + (width * scale) as i32,
                    bottom: -50 + (281.0 * scale) as i32,
                };
                let original = free;
                resize_pane(&mut free, WMSZ_BOTTOMRIGHT, (88.0, 96.0), scale, false, &[]);
                assert_eq!(
                    (free.left, free.top, free.right, free.bottom),
                    (original.left, original.top, original.right, original.bottom)
                );
            }
            let mut r = RECT {
                left: 0,
                top: 0,
                right: 1,
                bottom: 1,
            };
            resize_pane(&mut r, WMSZ_BOTTOMRIGHT, (88.0, 96.0), scale, false, &[]);
            let grid = Grid::system(
                r.right as f32 / scale,
                r.bottom as f32 / scale,
                48.0,
                (88.0, 96.0),
            );
            assert_eq!((grid.columns, grid.visible_rows), (1, 1));
            assert_eq!(r.right, (112.0 * scale).ceil() as i32);
            assert_eq!(r.bottom, (160.0 * scale).ceil() as i32);
            resize_pane(&mut r, WMSZ_BOTTOM, (88.0, 96.0), scale, true, &[]);
            assert_eq!(r.bottom, (HEADER * scale).ceil() as i32);
        }
    }
}

/// Row bounds shared by manual resizing and control-plan fitting (logical DPI).
pub fn icon_row_height<'a>(grid: Grid, labels: impl Iterator<Item = &'a str>) -> f32 {
    labels.map(|text| super::theme::selection_height(
        grid.icon_size,
        super::label::scaled_content_height(text, grid.cell_width.round() as u32, 96, grid.text_scale),
        grid.cell_height,
    )).fold(grid.icon_size + LABEL_OFFSET + 1.0, f32::max)
}

pub fn pane_minimum(cell: (f32, f32), collapsed: bool, tabbed: bool, first_row: Option<f32>) -> (f32, f32) {
    ((cell.0 + PADDING * 2.0).max(if tabbed { luciddesk_core::RectDip::MIN_WIDTH } else { 0.0 }),
     if collapsed { HEADER } else { HEADER + PADDING * 2.0 + first_row.unwrap_or(cell.1) })
}

pub fn icon_width(columns: usize, grid: Grid, tabbed: bool) -> f32 {
    let minimum = pane_minimum((grid.cell_width, grid.cell_height), false, tabbed, None).0;
    let columns = columns.max(((minimum - PADDING * 2.0) / grid.cell_width).ceil() as usize);
    PADDING * 2.0 + columns as f32 * grid.cell_width
}

pub fn pane_content_height(rows: usize, step: f32, content: &[f32]) -> f32 {
    let last = content.get(rows.saturating_sub(1)).copied().unwrap_or(step);
    HEADER + PADDING * 2.0 + rows.saturating_sub(1) as f32 * step + last
}

fn snap_content_height(pixels: i32, step: f32, content: &[f32], scale: f32) -> i32 {
    let minimum = (pane_content_height(1, step, content) * scale).ceil() as i32;
    let proposed = pixels.max(minimum);
    let estimated = ((proposed as f32 / scale - HEADER - PADDING * 2.0) / step)
        .floor()
        .max(0.0) as usize
        + 1;
    let nearest = (estimated.saturating_sub(1).max(1)..=estimated + 1)
        .map(|rows| (pane_content_height(rows, step, content) * scale).ceil() as i32)
        .min_by_key(|height| (height - proposed).abs())
        .unwrap_or(proposed);
    if (nearest - proposed).abs() as f32 <= 8.0 * scale {
        nearest
    } else {
        proposed
    }
}

pub fn fitting_rows(content: &[f32], step: f32, available: f32) -> usize {
    content
        .iter()
        .enumerate()
        .take_while(|(index, height)| *index as f32 * step + **height <= available + 0.01)
        .count()
        .max(1)
}

#[cfg(test)]
mod content_tests {
    use super::*;
    #[test]
    fn last_row_gap_is_removed_and_padding_can_shrink_before_scrolling() {
        let content = [85.0, 69.0, 85.0];
        let height = pane_content_height(3, 96.0, &content);
        assert_eq!(height, 341.0);
        assert_eq!(fitting_rows(&content, 96.0, height - HEADER - PADDING), 3);
        assert_eq!(
            fitting_rows(&content, 96.0, height - HEADER - PADDING - 11.0),
            3
        );
        assert_eq!(
            fitting_rows(&content, 96.0, height - HEADER - PADDING - 13.0),
            2
        );
        assert_eq!(pane_content_height(2, 96.0, &content), 229.0);
        for scale in [1.0, 1.25, 1.5, 2.0] {
            assert_eq!(
                snap_content_height(((height + 5.0) * scale) as i32, 96.0, &content, scale),
                (height * scale).ceil() as i32
            );
        }
    }
}
