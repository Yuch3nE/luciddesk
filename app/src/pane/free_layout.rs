//! Free icon positions are DIPs relative to the pane content origin.
use super::*;
use luciddesk_core::{DesktopItem, PointDip};

pub(super) fn grid(w: &Workspace) -> layout::Grid {
    layout::desktop_grid(
        0.0,
        0.0,
        layout::DESKTOP_ICON_SIZE,
        w.pane_options().grid_scale,
    )
}
pub(super) fn point(item: &DesktopItem, g: layout::Grid) -> PointDip {
    item.pane_position()
        .unwrap_or_else(|| match item.placement() {
            DesktopPlacement::Pane { position, .. } => PointDip::new(
                position.column as f32 * g.cell_width,
                position.row as f32 * g.cell_height,
            ),
            _ => PointDip::default(),
        })
}
pub(super) fn width(w: &Workspace, id: PanelId) -> f32 {
    let g=grid(w);
    w.desktop_items().iter().filter(|i|matches!(i.placement(),DesktopPlacement::Pane { pane_id,.. } if *pane_id==id))
        .map(|i|point(i,g).x+g.cell_width).fold(0.0,f32::max)
}
pub(super) fn extent(w: &Workspace, id: PanelId) -> (f32, f32) {
    let g = grid(w);
    w.desktop_items().iter().filter(|i| matches!(i.placement(),DesktopPlacement::Pane { pane_id, .. } if *pane_id==id))
        .fold((0.0_f32, 0.0_f32), |(right, bottom), item| {
            let p = point(item, g);
            (
                right.max(p.x + g.cell_width),
                bottom.max(p.y + layout::icon_row_height(g, [item.display_name()].into_iter())),
            )
        })
}
pub(super) fn toggle(w: &mut Workspace, id: PanelId) -> Result<(), String> {
    let panel = w.panel(id).ok_or("panel does not exist")?;
    if !panel.fixed_grid() || panel.locked() {
        return Err("disable auto arrange on an unlocked desktop panel first".into());
    }
    let enable = !panel.free_layout();
    let g = grid(w);
    let positions: Vec<_> = ordered_desktop_items(w, id)
        .iter()
        .map(|i| (i.identity().clone(), point(i, g)))
        .collect();
    w.panel_mut(id).unwrap().set_free_layout(enable);
    if enable {
        for (identity, point) in positions {
            w.desktop_item_mut(&identity)
                .unwrap()
                .set_pane_position(Some(point));
        }
    } else {
        let columns = fixed_grid::columns(w, id);
        let mut used = std::collections::HashSet::new();
        for (identity, p) in positions {
            let mut slot = (p.y / g.cell_height).round() as usize * columns
                + ((p.x / g.cell_width).round() as usize).min(columns - 1);
            while !used.insert(slot) {
                slot += 1;
            }
            w.desktop_item_mut(&identity)
                .unwrap()
                .set_placement(DesktopPlacement::Pane {
                    pane_id: id,
                    position: fixed_grid::position(slot, columns),
                });
        }
    }
    Ok(())
}

/// Translate the selection as a group. Overlap is intentional in free mode.
pub(super) fn move_items(
    w: &mut Workspace,
    source: PanelId,
    target: PanelId,
    keys: &[String],
    anchor: &str,
    destination: PointDip,
) -> Result<(), String> {
    let g = grid(w);
    let columns = fixed_grid::columns(w, source);
    let positions: Vec<_> = ordered_desktop_items(w, source)
        .iter()
        .enumerate()
        .filter(|(_, i)| keys.contains(&i.identity().persistent_key()))
        .map(|(index, i)| {
            let p = if w.panel(source).is_some_and(Panel::fixed_grid) {
                point(i, g)
            } else {
                PointDip::new(
                    (index % columns) as f32 * g.cell_width,
                    (index / columns) as f32 * g.cell_height,
                )
            };
            (i.identity().clone(), p)
        })
        .collect();
    let Some((_, origin)) = positions.iter().find(|(i, _)| i.persistent_key() == anchor) else {
        return Err("dragged item is no longer available".into());
    };
    let min_x = positions
        .iter()
        .map(|(_, p)| p.x)
        .fold(f32::INFINITY, f32::min);
    let min_y = positions
        .iter()
        .map(|(_, p)| p.y)
        .fold(f32::INFINITY, f32::min);
    let right = positions
        .iter()
        .map(|(_, p)| p.x + g.cell_width)
        .fold(0.0, f32::max);
    let available = w
        .panel(target)
        .ok_or("target panel is unavailable")?
        .rect()
        .width
        - layout::PADDING * 2.0;
    if right - min_x > available {
        return Err("Selection is wider than the target panel. Widen the panel before moving these icons together.".into());
    }
    let dx = (destination.x - origin.x).clamp(-min_x, available - right);
    let dy = (destination.y - origin.y).max(-min_y);
    for (identity, p) in positions {
        let p = PointDip::new(p.x + dx, p.y + dy);
        let item = w.desktop_item_mut(&identity).unwrap();
        item.set_placement(DesktopPlacement::Pane {
            pane_id: target,
            position: GridPosition::new((p.x / g.cell_width) as u32, (p.y / g.cell_height) as u32),
        });
        item.set_pane_position(Some(p));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PaneApp {
        let mut s = crate::pane::tests::test_state();
        let id = PanelId::new(1);
        let identities: Vec<_> = s
            .workspace
            .desktop_items()
            .iter()
            .map(|i| i.identity().clone())
            .collect();
        for (at, identity) in identities.iter().enumerate() {
            s.workspace
                .desktop_item_mut(identity)
                .unwrap()
                .set_placement(DesktopPlacement::Pane {
                    pane_id: id,
                    position: GridPosition::new(at as u32, 0),
                });
        }
        fixed_grid::toggle(&mut s.workspace, id).unwrap();
        toggle(&mut s.workspace, id).unwrap();
        s
    }
    #[test]
    fn coordinates_survive_refresh_reload_and_unchanged_save() {
        let mut s = fixture();
        let id = PanelId::new(1);
        let key = s.workspace.desktop_items()[0].identity().persistent_key();
        move_items(
            &mut s.workspace,
            id,
            id,
            &[key.clone()],
            &key,
            PointDip::new(31.25, 253.5),
        )
        .unwrap();
        let inventory: Vec<_> = s
            .workspace
            .desktop_items()
            .iter()
            .map(|i| DesktopItem::new(i.identity().clone(), i.display_name()))
            .collect();
        s.workspace.reconcile_desktop_items(inventory);
        normalize_pane_orders(&mut s);
        s.store.save_workspace(&s.workspace).unwrap();
        let restored = s.store.load_workspace().unwrap();
        assert!(restored.panel(id).unwrap().free_layout());
        let item = restored
            .desktop_items()
            .iter()
            .find(|i| i.identity().persistent_key() == key)
            .unwrap();
        assert_eq!(item.pane_position(), Some(PointDip::new(31.25, 253.5)));
        let count = s.store.change_count();
        s.store.save_workspace(&restored).unwrap();
        assert_eq!(s.store.change_count(), count);
    }
    #[test]
    fn group_translation_preserves_offsets_and_clamps_as_a_group() {
        let mut s = fixture();
        let id = PanelId::new(1);
        let g = grid(&s.workspace);
        let items = ordered_desktop_items(&s.workspace, id);
        let keys: Vec<_> = items
            .iter()
            .map(|i| i.identity().persistent_key())
            .collect();
        let original: Vec<_> = items.iter().map(|i| point(i, g)).collect();
        move_items(
            &mut s.workspace,
            id,
            id,
            &keys[..2],
            &keys[1],
            PointDip::new(-30.0, -10.0),
        )
        .unwrap();
        let find = |key: &str| {
            point(
                s.workspace
                    .desktop_items()
                    .iter()
                    .find(|i| i.identity().persistent_key() == key)
                    .unwrap(),
                g,
            )
        };
        let a = find(&keys[0]);
        let b = find(&keys[1]);
        assert_eq!(b.x - a.x, original[1].x - original[0].x);
        assert_eq!(b.y - a.y, original[1].y - original[0].y);
        assert!(a.x >= 0.0 && a.y >= 0.0 && b.x >= 0.0 && b.y >= 0.0);
        assert_eq!(find(&keys[2]), original[2]);
    }
    #[test]
    fn overlap_hits_topmost_and_scroll_and_dpi_use_free_coordinates() {
        let mut s = fixture();
        let id = PanelId::new(1);
        for item in s.workspace.desktop_items_mut() {
            item.set_pane_position(Some(PointDip::new(27.5, 237.25)));
        }
        let mut m = create_model(&s, id).unwrap();
        let g = m.grid(600.0, 180.0);
        assert!(g.scroll_limit.unwrap() > 0);
        m.scroll = g.cell_height as usize;
        let (x, y) = m.cell(g, 0);
        assert_eq!(x, layout::PADDING + 27.5);
        assert_eq!(y, g.content_top + 237.25 - g.cell_height);
        for scale in [1.0, 1.25, 1.5, 2.0] {
            assert_eq!(m.hit(g, x + 10.0, y + 10.0, scale), Some(m.items.len() - 1));
        }
        assert_eq!(
            m.hit(g, layout::PADDING + 1.0, g.content_top + 1.0, 1.0),
            None
        );
        assert!(m.visible_indices(g, y, y + 50.0).next().is_some());
    }
    #[test]
    fn alignment_resolves_overlap_and_auto_arrange_clears_coordinates() {
        let mut s = fixture();
        let id = PanelId::new(1);
        for item in s.workspace.desktop_items_mut() {
            item.set_pane_position(Some(PointDip::new(27.5, 37.25)));
        }
        toggle(&mut s.workspace, id).unwrap();
        assert!(!s.workspace.panel(id).unwrap().free_layout());
        let positions: std::collections::HashSet<_> = s
            .workspace
            .desktop_items()
            .iter()
            .map(|i| match i.placement() {
                DesktopPlacement::Pane { position, .. } => (position.column, position.row),
                _ => panic!(),
            })
            .collect();
        assert_eq!(positions.len(), s.workspace.desktop_items().len());
        assert!(
            s.workspace
                .desktop_items()
                .iter()
                .all(|i| i.pane_position().is_none())
        );
        toggle(&mut s.workspace, id).unwrap();
        fixed_grid::toggle(&mut s.workspace, id).unwrap();
        assert!(!s.workspace.panel(id).unwrap().free_layout());
        assert!(
            s.workspace
                .desktop_items()
                .iter()
                .all(|i| i.pane_position().is_none())
        );
    }
    #[test]
    fn incoming_icons_avoid_free_rectangles_and_sort_is_explicit() {
        let mut s = fixture();
        let id = PanelId::new(1);
        let keys: Vec<_> = ordered_desktop_items(&s.workspace, id)
            .iter()
            .map(|i| i.identity().persistent_key())
            .collect();
        move_items(
            &mut s.workspace,
            id,
            id,
            &keys[..1],
            &keys[0],
            PointDip::new(20.0, 20.0),
        )
        .unwrap();
        fixed_grid::place(&mut s.workspace, id, &keys[1..2], GridPosition::new(0, 0));
        let items = ordered_desktop_items(&s.workspace, id);
        let a = items
            .iter()
            .find(|i| i.identity().persistent_key() == keys[0])
            .unwrap();
        let b = items
            .iter()
            .find(|i| i.identity().persistent_key() == keys[1])
            .unwrap();
        let g = grid(&s.workspace);
        let p = point(a, g);
        let q = point(b, g);
        assert!(
            q.x + g.cell_width <= p.x
                || q.x >= p.x + g.cell_width
                || q.y >= p.y + layout::icon_row_height(g, [a.display_name()].into_iter())
        );
        sorting::apply(&mut s.workspace, id, false).unwrap();
        assert!(s.workspace.panel(id).unwrap().free_layout());
        assert!(
            s.workspace
                .desktop_items()
                .iter()
                .all(|i| i.pane_position().is_some())
        );
        let m = create_model(&s, id).unwrap();
        assert!(m.items.iter().all(|i| i.details.free_position.is_some()));
    }
    #[test]
    fn drop_coordinates_reverse_cell_transform_with_scroll_and_grab_offset() {
        let mut s = fixture();
        let id = PanelId::new(1);
        let position = PointDip::new(31.25, 650.5);
        for item in s.workspace.desktop_items_mut() { item.set_pane_position(Some(position)); }
        let mut model = create_model(&s, id).unwrap();
        let offset = PointDip::new(17.5, 12.25);
        for scroll in [0, 125, 500] {
            model.scroll = scroll;
            let grid = model.grid(600.0, 400.0);
            let (x, y) = model.cell(grid, 0);
            let (_, destination) = model.drop_destination(grid, x + offset.x, y + offset.y, offset);
            assert_eq!(destination, Some(position));
            let (_, shifted) = model.drop_destination(grid, x + offset.x - 4.5, y + offset.y + 7.25, offset);
            assert_eq!(shifted, Some(PointDip::new(position.x - 4.5, position.y + 7.25)));
        }
        model.free_layout = false;
        model.scroll = 3;
        let grid = model.grid(600.0, 400.0);
        let (at, free) = model.drop_destination(grid, layout::PADDING + grid.cell_width * 1.5,
            grid.content_top + grid.cell_height * 0.5, offset);
        assert_eq!(at, 3 * grid.columns + 1);
        assert_eq!(free, None);
    }

    #[test]
    fn keyboard_reveals_partial_icon_without_row_jump() {
        let mut s=fixture();let id=PanelId::new(1);
        for item in s.workspace.desktop_items_mut() { item.set_pane_position(Some(PointDip::new(10.0,170.0))); }
        let mut m=create_model(&s,id).unwrap();
        let g=m.grid(600.0,layout::HEADER+layout::PADDING*2.0+192.0);
        let before=m.selection_bounds(g,0,1.0);
        let expected=(before.y+before.height-g.content_top-192.0).ceil().max(0.0) as usize;
        assert!(expected>0);
        m.ensure_visible(g,0,1.0);
        assert_eq!(m.scroll,expected);
        let after=m.selection_bounds(g,0,1.0);
        assert!(after.y>=g.content_top && after.y+after.height<=g.content_top+192.0);
        let bar=scrollbar::Bar::for_model(&m,600.0,layout::HEADER+layout::PADDING*2.0+192.0).unwrap();
        assert_eq!(bar.page,192);
    }
    #[test]
    fn sparse_layout_minimum_queries_do_not_allocate_virtual_rows() {
        let mut s=fixture();let id=PanelId::new(1);
        s.workspace.desktop_items_mut()[0].set_pane_position(Some(PointDip::new(10.0,100000.0)));
        let m=create_model(&s,id).unwrap();let g=m.grid(600.0,300.0);
        assert!(m.content_rows(g)>100);
        assert_eq!(m.row_contents(g).len(),1);
    }
    #[test]
    fn free_visibility_excludes_offscreen_items_between_visible_indices() {
        let mut s = fixture();
        let id = PanelId::new(1);
        for (index, item) in s.workspace.desktop_items_mut().iter_mut().enumerate() {
            item.set_pane_position(Some(PointDip::new(10.0, if index == 1 { 10000.0 } else { 20.0 })));
        }
        let model = create_model(&s, id).unwrap();
        let grid = model.grid(600.0, 300.0);
        let visible: Vec<_> = model.visible_indices(grid, grid.content_top, 300.0).collect();
        assert_eq!(visible, vec![0, 2]);
        assert_eq!(model.minimum_row_content(grid), model.row_contents(grid).first().copied());
    }

    #[test]
    fn fixed_minimum_queries_only_requested_row_even_for_sparse_layout() {
        let mut s = fixture();
        let id = PanelId::new(1);
        s.workspace.panel_mut(id).unwrap().set_free_layout(false);
        for item in s.workspace.desktop_items_mut() {
            item.set_placement(DesktopPlacement::Pane { pane_id: id, position: GridPosition::new(0, 1000000) });
        }
        let mut model = create_model(&s, id).unwrap();
        model.scroll = 1000000;
        let grid = model.grid(600.0, 300.0);
        assert_eq!(model.minimum_row_content(grid), Some(layout::icon_row_height(grid, model.items.iter().map(|i| i.label.as_str()))));
        model.collapsed = true;
        assert_eq!(model.minimum_row_content(grid), None);
    }

    #[test]
    fn reordered_free_coordinates_stay_fixed_when_grid_scale_changes() {
        let mut s=fixture();let id=PanelId::new(1);
        sorting::apply(&mut s.workspace,id,false).unwrap();
        assert!(!sorting::apply(&mut s.workspace,id,false).unwrap());
        let points:Vec<_>=s.workspace.desktop_items().iter().map(|i|i.pane_position().unwrap()).collect();
        let mut g=grid(&s.workspace);g.cell_width*=1.5;g.cell_height*=1.5;
        for (i,p) in s.workspace.desktop_items().iter().zip(points) { assert_eq!(point(i,g),p); }
        let items=items_for(&s,id);set_order(&mut s.workspace,id,&items);
        assert!(s.workspace.desktop_items().iter().all(|i|i.pane_position().is_some()));
    }

}
