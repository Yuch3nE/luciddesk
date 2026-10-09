//! Pane geometry shared by rendering, hit testing, dragging and resizing.
use super::*;

type GeometryKey = (usize, usize, u32, u32, bool, u64, u64, &'static str);
#[derive(Clone, Default)]
pub(super) struct GeometryCache {
    key: Option<GeometryKey>,
    width: f32,
    height: f32,
    columns: usize,
    heights: Vec<f32>,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone)]
pub struct GroupModel {
    pub sort_orders: Rc<RefCell<HashMap<PanelId, (u8, bool)>>>,
    pub merge_preview: Vec<(PanelId, String)>,
    pub merge_occluded: bool,
    pub tabs: Vec<(PanelId, String)>,
    pub active_tab: PanelId,
    pub list_view: bool,
    pub fixed_grid: bool,
    pub free_layout: bool,
    pub folder_sort: (u8, bool),
    pub folder_columns: Option<[f32; 4]>,
    pub folder_visible_columns: u8,
    pub folder_navigation: [bool; 2],
    pub folder: Option<std::path::PathBuf>,
    pub folder_status: Option<String>,
    pub options: luciddesk_core::PaneOptions,
    pub theme: luciddesk_core::PanelTheme,
    pub dark: bool,

    pub hovered_item: Option<usize>,
    pub scrollbar: super::scrollbar::State,
    pub focused: bool,
    pub auto_hide: bool,
    pub locked: bool,
    pub reveal: f32,
    pub hovered_tab: Option<PanelId>,
    pub hovered_button: Option<usize>,
    pub pressed_button: Option<usize>,
    pub backdrop: luciddesk_core::Backdrop,
    pub native_material: bool,
    pub title: String,
    pub items: Vec<Item>,
    pub icon_size: f32,
    // Keyboard focus may point to an unselected item after Ctrl+navigation.
    pub selected: Option<usize>,
    pub selection: std::collections::BTreeSet<usize>,
    pub selection_anchor: Option<usize>,
    pub renaming: Option<ShellIdentity>,
    // Free icons use whole DIPs; grid/list views use rows. Never persisted.
    pub scroll: usize,
    pub scroll_x: f32,
    pub(super) geometry_cache: RefCell<GeometryCache>,
    pub collapsed: bool,
    pub loading: bool,
}

impl GroupModel {
    pub(super) fn content_header(&self) -> f32 { layout::HEADER }
    pub(super) fn header_button_x(&self, width: f32, button: usize) -> f32 {
        if self.tabs.len() > 1 && button >= 2 {
            width - 70.0 + (button - 2) as f32 * 32.0
        } else { layout::header_button_x(width, button) }
    }
    pub(super) fn header_button_enabled(&self, button: usize) -> bool {
        match button {
            0 | 1 if self.tabs.len() > 1 => false,
            2 | 3 => self.folder.is_some() && self.folder_navigation[button - 2],
            _ => true,
        }
    }

    pub(super) fn header_button(&self, width: f32, x: f32, y: f32) -> Option<usize> {
        layout::header_button(width, x, y).filter(|_| self.tabs.len() < 2).or_else(|| {
            if self.folder.is_none() || !(layout::HEADER_INSET..layout::HEADER - layout::HEADER_INSET).contains(&y) { return None; }
            (2..4).find(|&button| {
                let left = self.header_button_x(width, button);
                (left..left + 28.0).contains(&x)
            })
        })
    }

    pub(super) fn is_list(&self) -> bool {
        self.list_view
    }
    pub(super) fn list_columns(&self, width: f32) -> [f32; 5] {
        if self.folder.is_some() {
            super::columns::visible_bounds(width, self.folder_columns, self.folder_visible_columns)
        } else {
            [32.0, width, width, width, width]
        }
    }
    pub(super) fn clear_selection(&mut self) {
        self.selected = None;
        self.selection.clear();
        self.selection_anchor = None;
    }

    pub(super) fn select_item(&mut self, index: usize, ctrl: bool, shift: bool) {
        if index >= self.items.len() {
            return;
        }
        if shift {
            let anchor = self
                .selection_anchor
                .or(self.selected)
                .unwrap_or(index)
                .min(self.items.len() - 1);
            if !ctrl {
                self.selection.clear();
            }
            self.selection.extend(anchor.min(index)..=anchor.max(index));
            self.selection_anchor = Some(anchor);
        } else {
            if ctrl {
                if !self.selection.remove(&index) {
                    self.selection.insert(index);
                }
            } else {
                self.selection.clear();
                self.selection.insert(index);
            }
            self.selection_anchor = Some(index);
        }
        self.selected = Some(index);
    }

    pub(super) fn select_all(&mut self) {
        self.selection = (0..self.items.len()).collect();
        if !self.items.is_empty() {
            self.selected = Some(self.selected.unwrap_or(0).min(self.items.len() - 1));
            self.selection_anchor = self.selected;
        } else {
            self.clear_selection();
        }
    }

    pub(super) fn selected_identities(&self) -> Vec<ShellIdentity> {
        self.selection
            .iter()
            .filter_map(|&index| self.items.get(index))
            .map(|item| item.identity.clone())
            .collect()
    }

    pub(super) fn replace_items(&mut self, items: Vec<Item>) {
        let focus = self
            .selected
            .and_then(|i| self.items.get(i))
            .map(|i| i.identity.clone());
        let anchor = self
            .selection_anchor
            .and_then(|i| self.items.get(i))
            .map(|i| i.identity.clone());
        // Borrow identities while remapping; avoid cloning paths and an O(items × selection) scan.
        let identities: std::collections::HashSet<_> = self.selection.iter()
            .filter_map(|&index| self.items.get(index).map(|item| &item.identity)).collect();
        let selection = items.iter().enumerate()
            .filter_map(|(index, item)| identities.contains(&item.identity).then_some(index)).collect();
        let same_geometry = self.items.len() == items.len() && self.items.iter().zip(&items).all(|(old, new)|
            old.label == new.label && old.details.grid_position == new.details.grid_position && old.details.free_position == new.details.free_position);
        let cache = self.geometry_cache.get_mut();
        if same_geometry {
            if let Some(key) = &mut cache.key { key.0 = items.as_ptr() as usize; }
        } else { cache.key = None; }
        self.items = items;
        self.selected =
            focus.and_then(|identity| self.items.iter().position(|i| i.identity == identity));
        self.selection_anchor =
            anchor.and_then(|identity| self.items.iter().position(|i| i.identity == identity));
        self.selection = selection;
    }

    fn icon_grid(&self, width: f32, height: f32) -> layout::Grid {
        layout::desktop_grid(width, height, self.icon_size, self.options.grid_scale)
    }
    pub(super) fn resize_cell(&self) -> (f32, f32) {
        if self.is_list() {
            return (layout::LIST_CELL_WIDTH, layout::LIST_ROW);
        }
        let grid = self.icon_grid(0.0, 0.0);
        (grid.cell_width, grid.cell_height)
    }

    fn geometry(&self) -> std::cell::Ref<'_, GeometryCache> {
        let key = (self.items.as_ptr() as usize, self.items.len(), self.icon_size.to_bits(),
            self.options.grid_scale.to_bits(), self.free_layout, fonts::revision(), assets::font_revision(), crate::i18n::default_font());
        if self.geometry_cache.borrow().key != Some(key) {
            let grid = self.icon_grid(0.0, 0.0);
            let mut cached = GeometryCache { key: Some(key), columns: 1, ..Default::default() };
            for item in &self.items {
                if let Some(p) = item.details.grid_position { cached.columns = cached.columns.max(p.column as usize + 1); }
                if self.free_layout {
                    let height = layout::icon_row_height(grid, [item.label.as_str()].into_iter());
                    cached.heights.push(height);
                    if let Some(p) = item.details.free_position {
                        cached.width = cached.width.max(p.x + grid.cell_width);
                        cached.height = cached.height.max(p.y + height);
                    }
                } else if let Some(p) = item.details.grid_position {
                    cached.width = cached.width.max((p.column as f32 + 1.0) * grid.cell_width);
                }
            }
            *self.geometry_cache.borrow_mut() = cached;
        }
        self.geometry_cache.borrow()
    }
    fn free_height(&self, _grid: layout::Grid) -> f32 { self.geometry().height }
    fn row_content(&self, grid: layout::Grid, row: usize) -> f32 {
        if self.fixed_grid && !self.is_list() {
            let first = self.items.partition_point(|i| i.details.grid_position.map_or(0, |p| p.row as usize) < row);
            let last = self.items.partition_point(|i| i.details.grid_position.map_or(0, |p| p.row as usize) <= row);
            return layout::icon_row_height(grid, self.items[first..last].iter().map(|i| i.label.as_str()));
        }
        let start = row * grid.columns;
        layout::icon_row_height(grid,
            self.items[start..(start + grid.columns).min(self.items.len())].iter().map(|item| item.label.as_str()))
    }

    pub(super) fn minimum_row_content(&self, grid: layout::Grid) -> Option<f32> {
        if self.collapsed { return None; }
        if self.free_layout && !self.is_list() {
            return Some(layout::icon_row_height(grid, self.items.iter().map(|i| i.label.as_str())));
        }
        if self.is_list() { return (self.scroll < self.items.len()).then_some(layout::LIST_ROW); }
        (self.scroll < self.content_rows(grid)).then(|| self.row_content(grid, self.scroll))
    }

    #[cfg(test)]
    pub(super) fn row_contents(&self, grid: layout::Grid) -> Vec<f32> {
        if self.free_layout && !self.is_list() { return vec![layout::icon_row_height(grid, self.items.iter().map(|i| i.label.as_str()))]; }
        if self.is_list() { return vec![layout::LIST_ROW; self.items.len()]; }
        (0..self.content_rows(grid)).map(|row| self.row_content(grid, row)).collect()
    }
    pub(super) fn resize_rows(&self, grid: layout::Grid, height: f32) -> Vec<f32> {
        let count = self.content_rows(grid);
        let start = self.scroll.min(count.saturating_sub(1));
        let needed = ((height - layout::HEADER - layout::PADDING * 2.0) / grid.cell_height).max(0.0).floor() as usize + 2;
        (start..count.min(start.saturating_add(needed.max(1))))
            .map(|row| self.row_content(grid, row)).collect()
    }

    pub(super) fn grid(&self, width: f32, height: f32) -> layout::Grid {
        if self.is_list() {
            let mut grid = layout::Grid::list(width, height);
            if self.folder.is_none() {
                grid.content_top = layout::HEADER + layout::PADDING;
                grid.visible_rows = ((height - grid.content_top - layout::PADDING)
                    / layout::LIST_ROW).floor().max(1.0) as usize;
            }
            return grid;
        }
        let height = height - self.horizontal_inset(width);
        let mut grid = self.icon_grid(width, height);
        grid.horizontal_limit = self.horizontal_max(width);
        if self.fixed_grid {
            grid.columns = grid.columns.max(self.geometry().columns);
        }
        if self.free_layout {
            let available = (height - grid.content_top - layout::PADDING).max(1.0);
            grid.scroll_limit = Some((self.free_height(grid) - available).ceil().max(0.0) as usize);
            grid.visible_rows = available.floor().max(1.0) as usize;
            return grid;
        }
        if !self.items.is_empty() {
            let count = self.content_rows(grid);
            let available = height - self.content_header() - layout::PADDING;
            // Positive row heights mean only a viewport-sized suffix can fit at
            // the end; measuring earlier labels cannot affect the scroll limit.
            let candidates = (available.max(0.0) / grid.cell_height).ceil() as usize + 2;
            let start = self.scroll.min(count - 1);
            let visible: Vec<_> = (start..(start + candidates).min(count))
                .map(|row| self.row_content(grid, row)).collect();
            grid.visible_rows = layout::fitting_rows(&visible, grid.cell_height, available);
            let tail_start = count.saturating_sub(candidates);
            let tail: Vec<_> = (tail_start..count).map(|row| self.row_content(grid, row)).collect();
            grid.scroll_limit = Some(tail_start + (0..tail.len())
                .find(|start| layout::fitting_rows(&tail[*start..], grid.cell_height, available) >= tail.len() - start)
                .unwrap_or(tail.len() - 1));
        }
        grid
    }
    pub(super) fn cell(&self, grid: layout::Grid, index: usize) -> (f32, f32) {
        if self.free_layout && !self.is_list() {
            if let Some(p) = self.items[index].details.free_position {
                return (layout::PADDING + p.x - self.scroll_x.clamp(0.0, grid.horizontal_limit), grid.content_top + p.y - self.scroll as f32);
            }
        }
        if self.fixed_grid && !self.is_list() {
            if let Some(p) = self.items[index].details.grid_position {
                return (layout::PADDING + p.column as f32 * grid.cell_width - self.scroll_x.clamp(0.0, grid.horizontal_limit),
                    grid.content_top + (p.row as f32 - self.scroll as f32) * grid.cell_height);
            }
        }
        grid.cell(index, self.scroll)
    }
    pub(super) fn content_rows(&self, grid: layout::Grid) -> usize {
        if self.free_layout && !self.is_list() { return (self.free_height(grid) / grid.cell_height).ceil() as usize; }
        if self.fixed_grid && !self.is_list() {
            self.items.last().and_then(|i| i.details.grid_position).map_or(0, |p| p.row as usize + 1)
        } else { self.items.len().div_ceil(grid.columns) }
    }
    pub(super) fn horizontal_max(&self, width: f32) -> f32 {
        if !self.fixed_grid || self.is_list() { return 0.0; }
        (self.content_width() - width).max(0.0)
    }
    pub(super) fn horizontal_offset(&self, width: f32) -> f32 {
        self.scroll_x.clamp(0.0, self.horizontal_max(width))
    }
    pub(super) fn horizontal_inset(&self, width: f32) -> f32 {
        if self.horizontal_max(width) > 0.0 { 16.0 } else { 0.0 }
    }
    pub(super) fn content_width(&self) -> f32 { self.geometry().width + layout::PADDING * 2.0 }
    pub(super) fn visible_indices(&self, grid: layout::Grid, top: f32, bottom: f32) -> impl DoubleEndedIterator<Item = usize> + '_ {
        let free = self.free_layout && !self.is_list();
        let range = if free { 0..self.items.len() }
        else if !self.fixed_grid || self.is_list() { grid.visible_indices(self.scroll, top, bottom, self.items.len()) }
        else {
            let first = ((top - grid.content_top) / grid.cell_height + self.scroll as f32).floor().max(0.0) as u32;
            let end = ((bottom - grid.content_top) / grid.cell_height + self.scroll as f32).ceil().max(0.0) as u32;
            self.items.partition_point(|i| i.details.grid_position.is_some_and(|p| p.row < first))..
                self.items.partition_point(|i| i.details.grid_position.is_some_and(|p| p.row < end))
        };
        range.filter(move |&index| {
            if !free { return true; }
            let (_, y) = self.cell(grid, index);
            y < bottom && y + self.geometry().heights[index] > top
        })
    }
    /// Client DIPs to a drop destination. Free scroll is in DIPs; grid scroll is in rows.
    pub(super) fn drop_destination(&self, grid: layout::Grid, x: f32, y: f32,
        offset: luciddesk_core::PointDip) -> (usize, Option<luciddesk_core::PointDip>) {
        if self.free_layout && !self.is_list() {
            return (0, Some(luciddesk_core::PointDip::new(
                x - layout::PADDING + self.scroll_x.clamp(0.0, grid.horizontal_limit) - offset.x,
                y - grid.content_top + self.scroll as f32 - offset.y,
            )));
        }
        let at = if self.fixed_grid && !self.is_list() {
            let column = ((x - layout::PADDING + self.scroll_x.clamp(0.0, grid.horizontal_limit)) / grid.cell_width).floor().max(0.0) as usize;
            let row = ((y - grid.content_top) / grid.cell_height).floor().max(0.0) as usize + self.scroll;
            row * grid.columns + column.min(grid.columns - 1)
        } else {
            grid.hit(x, y, self.scroll, self.items.len()).unwrap_or(self.items.len())
        };
        (at, None)
    }

    pub(super) fn next_selection(&self, key: u16, grid: layout::Grid) -> Option<usize> {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
        if !self.fixed_grid || self.is_list() || self.selected.is_none() || matches!(key, VK_HOME | VK_END) {
            return keyboard::next_selection(key, self.selected, self.items.len(), grid.columns, grid.visible_rows);
        }
        let at = self.selected?;
        if self.free_layout {
            let (x, y) = self.cell(grid, at);
            let page = grid.visible_rows as f32;
            return (0..self.items.len()).filter_map(|i| {
                let (px, py) = self.cell(grid, i); let dx = px-x; let dy = py-y;
                let score = match key {
                    VK_LEFT if dx < 0.0 => (dy.abs(), -dx), VK_RIGHT if dx > 0.0 => (dy.abs(), dx),
                    VK_UP if dy < 0.0 => (dx.abs(), -dy), VK_DOWN if dy > 0.0 => (dx.abs(), dy),
                    VK_PRIOR if dy < 0.0 => ((dy+page).abs(), dx.abs()), VK_NEXT if dy > 0.0 => ((dy-page).abs(), dx.abs()),
                    _ => return None,
                }; Some((score, i))
            }).min_by(|a,b| a.0.0.total_cmp(&b.0.0).then_with(|| a.0.1.total_cmp(&b.0.1)).then(a.1.cmp(&b.1))).map(|(_, i)| i).or(Some(at));
        }
        let here = self.items.get(at)?.details.grid_position?;
        let page = grid.visible_rows.max(1) as i64;
        self.items.iter().enumerate().filter_map(|(index, item)| {
            let p = item.details.grid_position?;
            let dx = i64::from(p.column) - i64::from(here.column);
            let dy = i64::from(p.row) - i64::from(here.row);
            let score = match key {
                VK_LEFT if dx < 0 => (dy.abs(), -dx),
                VK_RIGHT if dx > 0 => (dy.abs(), dx),
                VK_UP if dy < 0 => (dx.abs(), -dy),
                VK_DOWN if dy > 0 => (dx.abs(), dy),
                VK_PRIOR if dy < 0 => ((dy + page).abs(), dx.abs()),
                VK_NEXT if dy > 0 => ((dy - page).abs(), dx.abs()),
                _ => return None,
            };
            Some((score, index))
        }).min().map(|(_, index)| index).or(Some(at))
    }
    pub(super) fn ensure_visible(&mut self, grid: layout::Grid, index: usize, scale: f32) {
        if self.fixed_grid && !self.is_list() {
            let (x, _) = self.cell(grid, index);
            let offset = self.scroll_x.clamp(0.0, grid.horizontal_limit);
            let right = grid.viewport_width - layout::PADDING;
            self.scroll_x = if x < layout::PADDING { offset + x - layout::PADDING }
                else if x + grid.cell_width > right { offset + x + grid.cell_width - right } else { offset };
            self.scroll_x = self.scroll_x.clamp(0.0, self.horizontal_max(grid.viewport_width));
        }
        if self.free_layout && !self.is_list() {
            let bounds = self.selection_bounds(grid,index,scale);
            let top = bounds.y - grid.content_top + self.scroll as f32;
            let bottom = top + bounds.height;
            let page = grid.visible_rows as f32;
            if top < self.scroll as f32 { self.scroll = top.floor().max(0.0) as usize; }
            else if bottom > self.scroll as f32 + page { self.scroll = (bottom-page).ceil().max(0.0) as usize; }
        } else {
            let row = self.item_row(grid,index); let rows = grid.visible_rows.max(1);
            if row < self.scroll { self.scroll = row; }
            else if row >= self.scroll + rows { self.scroll = row-rows+1; }
        }
        self.scroll = self.scroll.min(grid.max_scroll(self.items.len()));
    }
    pub(super) fn item_row(&self, grid: layout::Grid, index: usize) -> usize {
        if self.free_layout && !self.is_list() { return self.items[index].details.free_position.map_or(0, |p| (p.y / grid.cell_height) as usize); }
        if self.fixed_grid && !self.is_list() { self.items[index].details.grid_position.map_or(0, |p| p.row as usize) }
        else { index / grid.columns }
    }
    pub(super) fn selection_bounds(&self, grid: layout::Grid, index: usize, scale: f32) -> RectDip {
        let (x, y) = self.cell(grid, index);
        if self.is_list() {
            return RectDip {
                x,
                y,
                width: grid.cell_width,
                height: grid.cell_height,
            };
        }
        let height = theme::selection_height(
            grid.icon_size,
            label::scaled_content_height(
                &self.items[index].label,
                (grid.cell_width * scale).round() as u32,
                (96.0 * scale).round() as u32,
                grid.text_scale,
            ) / scale,
            grid.cell_height,
        );
        RectDip {
            x,
            y,
            width: grid.cell_width,
            height,
        }
    }

    pub(super) fn hit(&self, grid: layout::Grid, x: f32, y: f32, scale: f32) -> Option<usize> {
        if self.collapsed {
            return None;
        }
        let contains = |index| {
            let r = self.selection_bounds(grid, index, scale);
            x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
        };
        if self.free_layout && !self.is_list() {
            if y < grid.content_top { return None; }
            return (0..self.items.len()).rev().find(|&i| contains(i));
        }
        if self.fixed_grid && !self.is_list() {
            if x < layout::PADDING || y < grid.content_top { return None; }
            let column = ((x - layout::PADDING + self.scroll_x.clamp(0.0, grid.horizontal_limit)) / grid.cell_width).floor() as u32;
            let row = ((y - grid.content_top) / grid.cell_height).floor() as u32 + self.scroll as u32;
            return self.items.binary_search_by_key(&(row, column), |i| {
                let p = i.details.grid_position.unwrap_or_default(); (p.row, p.column)
            }).ok().filter(|&index| contains(index));
        }
        grid.hit(x, y, self.scroll, self.items.len())
            .filter(|&index| contains(index))
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    #[test]
    fn geometry_cache_survives_scrolling_and_image_refresh_but_tracks_layout_changes() {
        let _sta = crate::pane::test_support::apartment();
        let state = crate::pane::tests::test_state();
        let mut model = create_model(&state, PanelId::new(1)).unwrap();
        model.fixed_grid = true;
        model.free_layout = true;
        model.items[0].details.free_position = Some(luciddesk_core::PointDip::new(500.0, 300.0));
        model.grid(220.0, 300.0);
        let heights = model.geometry_cache.borrow().heights.as_ptr();
        for _ in 0..20 { model.scroll_x += 1.0; model.grid(240.0, 350.0); }
        assert_eq!(model.geometry_cache.borrow().heights.as_ptr(), heights);
        let mut refreshed = model.items.clone();
        refreshed[0].image = Some(Arc::new(assets::Pixels { width: 1, height: 1, data: vec![0, 0, 255, 255] }));
        model.replace_items(refreshed);
        model.grid(240.0, 350.0);
        assert_eq!(model.geometry_cache.borrow().heights.as_ptr(), heights);
        let width = model.content_width();
        let mut items = model.items.clone();
        items[0].details.free_position.as_mut().unwrap().x += 100.0;
        model.replace_items(items);
        assert_eq!(model.content_width(), width + 100.0);
        let old = model.content_width();
        model.options.grid_scale *= 1.5;
        assert!(model.content_width() > old);
    }
    #[test]
    fn bounded_resize_measurement_matches_full_sparse_grid_at_every_dpi() {
        let _sta = crate::pane::test_support::apartment();
        let state = crate::pane::tests::test_state();
        let mut model = create_model(&state, PanelId::new(1)).unwrap();
        model.fixed_grid = true;
        for (at, item) in model.items.iter_mut().enumerate() {
            item.details.grid_position = Some(GridPosition::new(0, at as u32 * 5000));
        }
        for scroll in [0, 4999, 5000, 9999, 10000] {
            model.scroll = scroll;
            let grid = model.grid(300.0, 400.0);
            let all = model.row_contents(grid);
            let start = scroll.min(all.len().saturating_sub(1));
            for scale in [1.0, 1.25, 1.5, 2.0] {
                for height in [120.0, 211.0, 317.0, 499.0, 800.0] {
                    let rows = model.resize_rows(grid, height);
                    assert!(rows.len() < 12);
                    for edge in [WMSZ_TOP, WMSZ_BOTTOM, WMSZ_LEFT, WMSZ_RIGHT, WMSZ_TOPLEFT, WMSZ_TOPRIGHT, WMSZ_BOTTOMLEFT, WMSZ_BOTTOMRIGHT] {
                        let mut expected = RECT { left: -300, top: 71, right: -300 + (300.0 * scale) as i32, bottom: 71 + (height * scale) as i32 };
                        let mut actual = expected;
                        layout::resize_pane(&mut expected, edge, model.resize_cell(), scale, false, &all[start..]);
                        layout::resize_pane(&mut actual, edge, model.resize_cell(), scale, false, &rows);
                        assert_eq!((actual.left, actual.top, actual.right, actual.bottom), (expected.left, expected.top, expected.right, expected.bottom), "edge={edge} dpi={scale} scroll={scroll} height={height}");
                    }
                }
            }
        }
    }

    #[test]
    fn warm_geometry_matches_cold_measurement_after_content_and_mode_changes() {
        let _sta = crate::pane::test_support::apartment();
        let state = crate::pane::tests::test_state();
        let mut model = create_model(&state, PanelId::new(1)).unwrap();
        model.fixed_grid = true;
        model.free_layout = true;
        for (index, item) in model.items.iter_mut().enumerate() {
            item.details.grid_position = Some(GridPosition::new(index as u32 * 3, index as u32));
            item.details.free_position = Some(luciddesk_core::PointDip::new(index as f32 * 230.0, index as f32 * 170.0));
        }
        model.grid(220.0, 300.0);
        for change in 0..9 {
            let mut items = model.items.clone();
            match change {
                0 => items[0].label = "A long renamed document 多语言文件名 that wraps across lines.txt".into(),
                1 => { items.remove(2); }
                2 => items.reverse(),
                3 => items[0].details.free_position = Some(luciddesk_core::PointDip::new(800.0, 900.0)),
                4 => model.icon_size *= 1.5,
                5 => model.options.grid_scale *= 1.25,
                6 => { model.free_layout = false; items[0].details.grid_position = Some(GridPosition::new(15, 0)); }
                7 => { model.free_layout = true; items.push(items[0].clone()); }
                _ => items.clear(),
            }
            model.replace_items(items);
            let mut cold = model.clone();
            cold.geometry_cache = RefCell::new(GeometryCache::default());
            let actual = model.geometry();
            let expected = cold.geometry();
            assert_eq!((actual.width, actual.height, actual.columns, &actual.heights),
                (expected.width, expected.height, expected.columns, &expected.heights), "change={change}");
        }
    }

    #[test]
    fn font_invalidation_refreshes_geometry_and_empty_resize_is_safe() {
        let _sta = crate::pane::test_support::apartment();
        let state = crate::pane::tests::test_state();
        let mut model = create_model(&state, PanelId::new(1)).unwrap();
        model.free_layout = true;
        let before = model.geometry().key;
        assets::invalidate_font();
        assert_ne!(model.geometry().key, before);
        model.replace_items(Vec::new());
        model.free_layout = false;
        for fixed in [false, true] {
            model.fixed_grid = fixed;
            model.scroll = 10000;
            let grid = model.grid(220.0, 300.0);
            assert!(model.resize_rows(grid, 0.0).is_empty());
            assert!(model.resize_rows(grid, 800.0).is_empty());
            assert_eq!(model.horizontal_max(220.0), 0.0);
        }
    }
}
