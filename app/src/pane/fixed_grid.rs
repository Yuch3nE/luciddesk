//! Ordinary pane coordinates. Compact ordering remains an explicit separate mode.
use super::*;
use std::collections::HashSet;

pub(super) fn columns(w: &Workspace, id: PanelId) -> usize {
    let width = w.panel(id).map_or(260.0, |p| p.rect().width);
    layout::desktop_grid(width, 0.0, layout::DESKTOP_ICON_SIZE, w.pane_options().grid_scale).columns
}
pub(super) fn position(slot: usize, columns: usize) -> GridPosition {
    GridPosition::new((slot % columns.max(1)) as u32, (slot / columns.max(1)) as u32)
}
pub(super) fn extent(w: &Workspace, id: PanelId) -> (usize, usize) {
    if w.panel(id).is_some_and(Panel::free_layout) {
        let (right, bottom) = free_layout::extent(w, id); let g = free_layout::grid(w);
        return ((right / g.cell_width).ceil() as usize, (bottom / g.cell_height).ceil() as usize);
    }
    w.desktop_items().iter().filter_map(|item| match item.placement() {
        DesktopPlacement::Pane { pane_id, position } if *pane_id == id => Some((position.column as usize + 1, position.row as usize + 1)),
        _ => None,
    }).fold((0, 0), |a, b| (a.0.max(b.0), a.1.max(b.1)))
}
pub(super) fn minimum_width(w: &Workspace, id: PanelId) -> f32 {
    let members = w.tab_group(id).map_or_else(|| vec![id], |g| g.members.clone());
    let g = free_layout::grid(w);
    members.into_iter().filter(|id| w.panel(*id).is_some_and(Panel::fixed_grid)).map(|id| {
        let width = if w.panel(id).is_some_and(Panel::free_layout) { free_layout::width(w,id) }
            else { extent(w,id).0 as f32 * g.cell_width };
        if width > 0.0 { width + layout::PADDING * 2.0 } else { 0.0 }
    }).fold(0.0, f32::max)
}
pub(super) fn set_position(item: &mut luciddesk_core::DesktopItem, id: PanelId, position: GridPosition, free: bool, grid: layout::Grid) {
    item.set_placement(DesktopPlacement::Pane { pane_id: id, position });
    if free { item.set_pane_position(Some(luciddesk_core::PointDip::new(position.column as f32 * grid.cell_width, position.row as f32 * grid.cell_height))); }
}
pub(super) fn toggle(w: &mut Workspace, id: PanelId) -> Result<(), String> {
    let panel = w.panel(id).ok_or("panel does not exist")?;
    if !panel.supports_tabs() || panel.locked() { return Err("only unlocked desktop panels support fixed positions".into()); }
    let enabled = !panel.fixed_grid();
    let columns = columns(w, id);
    let keys: Vec<_> = ordered_desktop_items(w, id).into_iter().map(|i| i.identity().clone()).collect();
    for (index, identity) in keys.iter().enumerate() {
        w.desktop_item_mut(identity).unwrap().set_placement(DesktopPlacement::Pane {
            pane_id: id, position: if enabled { position(index, columns) } else { GridPosition::new(index as u32, 0) },
        });
    }
    w.panel_mut(id).unwrap().set_free_layout(false);
    w.panel_mut(id).unwrap().set_fixed_grid(enabled);
    Ok(())
}

/// Fill empty cells without compacting other icons. Dropping one existing icon
/// on another swaps the two; a multi-item drop fills available cells from there.
pub(super) fn place(w: &mut Workspace, id: PanelId, keys: &[String], start: GridPosition) {
    let columns = columns(w, id).max(start.column as usize + 1);
    let mut occupied: HashSet<_> = w.desktop_items().iter().filter_map(|item| match item.placement() {
        DesktopPlacement::Pane { pane_id, position } if *pane_id == id && !keys.contains(&item.identity().persistent_key()) => Some((position.column, position.row)),
        _ => None,
    }).collect();
    let free = w.panel(id).is_some_and(Panel::free_layout);
    let g = free_layout::grid(w);
    if free {
        for item in ordered_desktop_items(w, id).into_iter().filter(|i| !keys.contains(&i.identity().persistent_key())) {
            let p = free_layout::point(item, g);
            for row in (p.y / g.cell_height).floor() as u32..((p.y + layout::icon_row_height(g, [item.display_name()].into_iter())) / g.cell_height).ceil() as u32 {
                for col in (p.x / g.cell_width).floor() as u32..((p.x + g.cell_width) / g.cell_width).ceil() as u32 { occupied.insert((col, row)); }
            }
        }
    }
    if !free && keys.len() == 1 {
        let old = w.desktop_items().iter().find(|i| i.identity().persistent_key() == keys[0]).and_then(|i| match i.placement() {
            DesktopPlacement::Pane { pane_id, position } if *pane_id == id => Some(*position), _ => None,
        });
        if let Some(old) = old {
            if let Some(other) = w.desktop_items_mut().iter_mut().find(|i| matches!(i.placement(), DesktopPlacement::Pane { pane_id, position } if *pane_id == id && *position == start) && i.identity().persistent_key() != keys[0]) {
                other.set_placement(DesktopPlacement::Pane { pane_id: id, position: old });
                occupied.remove(&(start.column, start.row));
                occupied.insert((old.column, old.row));
            }
        }
    }
    let mut slot = start.row as usize * columns + start.column as usize;
    for key in keys {
        let mut cell = position(slot, columns);
        while occupied.contains(&(cell.column, cell.row)) { slot += 1; cell = position(slot, columns); }
        if let Some(item) = w.desktop_items_mut().iter_mut().find(|i| i.identity().persistent_key() == *key) {
            item.set_placement(DesktopPlacement::Pane { pane_id: id, position: cell });
            if free { item.set_pane_position(Some(luciddesk_core::PointDip::new(cell.column as f32 * g.cell_width, cell.row as f32 * g.cell_height))); }
            occupied.insert((cell.column, cell.row));
        }
        slot += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PaneApp {
        let mut s = crate::pane::tests::test_state();
        let id = PanelId::new(1);
        let identities: Vec<_> = s.workspace.desktop_items().iter().map(|i| i.identity().clone()).collect();
        for (at, identity) in identities.iter().enumerate() {
            s.workspace.desktop_item_mut(identity).unwrap().set_placement(DesktopPlacement::Pane {
                pane_id: id, position: GridPosition::new(at as u32, 0),
            });
        }
        toggle(&mut s.workspace, id).unwrap();
        s
    }
    fn cell(w: &Workspace, key: &str) -> GridPosition {
        match w.desktop_items().iter().find(|i| i.identity().persistent_key() == key).unwrap().placement() {
            DesktopPlacement::Pane { position, .. } => *position, _ => panic!("not in pane"),
        }
    }
    #[test]
    fn deleting_refreshing_and_reloading_keep_sparse_cells_without_repeat_writes() {
        let mut s = fixture(); let id = PanelId::new(1);
        let keys: Vec<_> = ordered_desktop_items(&s.workspace, id).iter().map(|i| i.identity().persistent_key()).collect();
        place(&mut s.workspace, id, &keys[..1], GridPosition::new(1, 4));
        let saved = cell(&s.workspace, &keys[0]);
        let inventory: Vec<_> = s.workspace.desktop_items().iter().filter(|i| i.identity().persistent_key() != keys[1]).cloned().collect();
        s.workspace.reconcile_desktop_items(inventory);
        normalize_pane_orders(&mut s);
        assert_eq!(cell(&s.workspace, &keys[0]), saved);
        let inventory: Vec<_> = s.workspace.desktop_items().iter().rev().map(|i| DesktopItem::new(i.identity().clone(), i.display_name())).collect();
        s.workspace.reconcile_desktop_items(inventory);
        assert_eq!(cell(&s.workspace, &keys[0]), saved);
        s.store.save_workspace(&s.workspace).unwrap();
        let restored = s.store.load_workspace().unwrap();
        assert_eq!(restored.panels(), s.workspace.panels());
        for key in [&keys[0], &keys[2]] { assert_eq!(cell(&restored, key), cell(&s.workspace, key)); }
        let changes = s.store.change_count();
        s.store.save_workspace(&restored).unwrap();
        assert_eq!(changes, s.store.change_count());
        s.workspace = restored;
        let model = create_model(&s, id).unwrap();
        let grid = model.grid(600.0, 180.0);
        assert_eq!(model.content_rows(grid), 5);
        assert!(grid.max_scroll(model.items.len()) > 0);
        let index = model.items.iter().position(|i| i.identity.persistent_key() == keys[0]).unwrap();
        let (x, y) = model.cell(grid, index);
        assert_eq!(model.hit(grid, x + 5.0, y + 5.0, 1.0), Some(index));
        assert_eq!(model.hit(grid, layout::PADDING + 1.0, grid.content_top + grid.cell_height * 3.0 + 1.0, 1.0), None);
        assert!(model.visible_indices(grid, y, y + grid.cell_height).any(|i| i == index));
    }
    #[test]
    fn dragging_swaps_two_icons_and_new_items_fill_holes_only() {
        let mut s = fixture(); let id = PanelId::new(1);
        let keys: Vec<_> = ordered_desktop_items(&s.workspace, id).iter().map(|i| i.identity().persistent_key()).collect();
        let a = cell(&s.workspace, &keys[0]); let b = cell(&s.workspace, &keys[1]);
        place(&mut s.workspace, id, &keys[..1], b);
        assert_eq!(cell(&s.workspace, &keys[0]), b);
        assert_eq!(cell(&s.workspace, &keys[1]), a);
        place(&mut s.workspace, id, &keys[..1], GridPosition::new(0, 3));
        let identity = ShellIdentity::Namespace { parsing_name: "test:new-fixed-icon".into() };
        let mut inventory = s.workspace.desktop_items().to_vec();
        inventory.push(DesktopItem::new(identity.clone(), "New"));
        s.workspace.reconcile_desktop_items(inventory);
        place(&mut s.workspace, id, &[identity.persistent_key()], GridPosition::default());
        assert_eq!(cell(&s.workspace, &keys[0]), GridPosition::new(0, 3));
        assert_eq!(cell(&s.workspace, &keys[1]), a);
        assert_eq!(cell(&s.workspace, &identity.persistent_key()), b);
    }
    #[test]
    fn disabling_fixed_mode_compacts_and_folder_panels_reject_it() {
        let mut s = fixture(); let id = PanelId::new(1);
        let keys: Vec<_> = ordered_desktop_items(&s.workspace, id).iter().map(|i| i.identity().persistent_key()).collect();
        place(&mut s.workspace, id, &keys[..1], GridPosition::new(0, 4));
        toggle(&mut s.workspace, id).unwrap();
        assert!(!s.workspace.panel(id).unwrap().fixed_grid());
        assert!(ordered_desktop_items(&s.workspace, id).iter().enumerate().all(|(index, item)| matches!(item.placement(), DesktopPlacement::Pane { position, .. } if *position == GridPosition::new(index as u32, 0))));
        s.workspace.panel_mut(id).unwrap().set_folder(Some("C:/folder".into()));
        assert!(toggle(&mut s.workspace, id).is_err());
    }
    #[test]
    fn sparse_keyboard_navigation_and_dpi_hit_testing_use_cells() {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_UP};
        let mut s = fixture(); let id = PanelId::new(1);
        let keys: Vec<_> = ordered_desktop_items(&s.workspace, id).iter().map(|i| i.identity().persistent_key()).collect();
        place(&mut s.workspace, id, &keys[..1], GridPosition::new(1, 5));
        let mut model = create_model(&s, id).unwrap();
        let first = model.items.iter().position(|i| i.identity.persistent_key() == keys[1]).unwrap();
        let last = model.items.iter().position(|i| i.identity.persistent_key() == keys[0]).unwrap();
        let grid = model.grid(420.0, 180.0);
        model.selected = Some(first);
        assert_eq!(model.next_selection(VK_DOWN, grid), Some(last));
        model.selected = Some(last);
        assert_eq!(model.next_selection(VK_UP, grid), Some(first));
        model.scroll = 4;
        let (x, y) = model.cell(grid, last);
        for dpi in [1.0, 1.25, 1.5, 2.0] {
            assert_eq!(model.hit(grid, x + 8.0, y + 8.0, dpi), Some(last));
        }
        let min = model.fixed_width();
        assert_eq!(model.cell(model.grid(800.0, 180.0), last), (x, y));
        assert!(min >= layout::PADDING * 2.0 + grid.cell_width * 3.0);
    }

    #[test]
    fn mode_and_positions_are_saved_together_and_removed_with_panel() {
        let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
        let id = PanelId::new(1);
        handle(&state, id, Event::ToggleFixedGrid).unwrap();
        let mut s = state.borrow_mut();
        assert!(s.store.load_workspace().unwrap().panel(id).unwrap().fixed_grid());
        remove_panel(&mut s.workspace, id);
        let workspace = s.workspace.clone();
        s.store.save_workspace(&workspace).unwrap();
        assert!(s.store.preference("panel_fixed_grid:1").unwrap().is_none());
    }

    #[test]
    fn transfers_leave_fixed_source_holes_and_do_not_shift_destination_icons() {
        let mut s = fixture(); let source = PanelId::new(1); let target = PanelId::new(2);
        toggle(&mut s.workspace, target).unwrap();
        let keys: Vec<_> = ordered_desktop_items(&s.workspace, source).iter().map(|i| i.identity().persistent_key()).collect();
        let untouched = cell(&s.workspace, &keys[2]);
        transfer_many(&mut s, source, &[0], target, 5).unwrap();
        assert_eq!(cell(&s.workspace, &keys[2]), untouched);
        let destination = cell(&s.workspace, &keys[0]);
        transfer_many(&mut s, source, &[0], target, 0).unwrap();
        assert_eq!(cell(&s.workspace, &keys[0]), destination);
        assert_eq!(cell(&s.workspace, &keys[2]), untouched);
        let restored = s.store.load_workspace().unwrap();
        for key in &keys { assert_eq!(cell(&restored, key), cell(&s.workspace, key)); }
        toggle(&mut s.workspace, target).unwrap();
        transfer_many(&mut s, source, &[0], target, 0).unwrap();
        assert!(items_for(&s, source).is_empty());
        assert_eq!(items_for(&s, target).len(), 3);
    }

}
