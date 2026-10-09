//! One-shot ordinary pane ordering shared by menus and the control API.
use super::*;

pub(super) fn apply(workspace: &mut Workspace, id: PanelId, descending: bool) -> Result<bool, String> {
    let panel = workspace.panel(id).ok_or("panel does not exist")?;
    if !panel.supports_tabs() { return Err("only desktop panels support name sorting".into()); }
    if panel.locked() { return Err("panel is locked".into()); }

    struct Entry {
        index: usize,
        name: Vec<u16>,
        key: String,
        position: (u32, u32),
    }
    let mut entries: Vec<_> = workspace.desktop_items().iter().enumerate()
        .filter_map(|(index, item)| match item.placement() {
            DesktopPlacement::Pane { pane_id, position } if *pane_id == id => Some(Entry {
                index,
                name: item.display_name().encode_utf16().chain(Some(0)).collect(),
                key: item.identity().persistent_key(),
                position: (position.row, position.column),
            }),
            _ => None,
        }).collect();
    entries.sort_unstable_by(|a, b| {
        let cmp = unsafe { windows_sys::Win32::UI::Shell::StrCmpLogicalW(a.name.as_ptr(), b.name.as_ptr()) }.cmp(&0)
            .then_with(|| a.name.cmp(&b.name)).then_with(|| a.key.cmp(&b.key));
        if descending { cmp.reverse() } else { cmp }
    });
    // The desired order is unchanged exactly when it is also ordered by the
    // existing grid positions (identity breaks ties). Preserve sparse grids on no-op.
    let fixed = panel.fixed_grid();
    let columns = fixed_grid::columns(workspace, id);
    let free = workspace.panel(id).is_some_and(Panel::free_layout);
    let metrics = free_layout::grid(workspace);
    if (!fixed || entries.iter().enumerate().all(|(at, e)| {
        let p = fixed_grid::position(at, columns);
        e.position == (p.row, p.column) && (!free || workspace.desktop_items()[e.index].pane_position() == Some(luciddesk_core::PointDip::new(p.column as f32 * metrics.cell_width, p.row as f32 * metrics.cell_height)))
    })) && entries.windows(2).all(|pair| {
        (pair[0].position, &pair[0].key) <= (pair[1].position, &pair[1].key)
    }) {
        return Ok(false);
    }
    // The item slice has not changed; use its indices instead of hashing every
    // desktop identity again and scanning unrelated panes to apply the result.
    let items = workspace.desktop_items_mut();
    for (at, entry) in entries.into_iter().enumerate() {
        fixed_grid::set_position(&mut items[entry.index], id, if fixed { fixed_grid::position(at, columns) } else { GridPosition::new(at as u32, 0) }, free, metrics);
    }
    Ok(true)
}
