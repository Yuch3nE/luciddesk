//! Content counts, grid metrics and folder readiness snapshots.
use super::*;

#[derive(Clone, Debug)]
pub(in crate::pane::control) struct FolderSnapshot {
    pub(super) root: std::path::PathBuf,
    pub(super) labels: Vec<String>,
    pub(super) ready: bool,
}
pub(in crate::pane::control) type FolderSnapshots = HashMap<PanelId, FolderSnapshot>;
pub(in crate::pane::control) fn snapshots(state: &PaneApp) -> FolderSnapshots {
    state
        .folders
        .iter()
        .filter_map(|(id, source)| {
            let root = state.workspace.panel(*id)?.folder()?.to_path_buf();
            Some((
                *id,
                FolderSnapshot {
                    root,
                    labels: source.items.iter().map(|item| item.label.clone()).collect(),
                    ready: !source.loading && source.status.is_none(),
                },
            ))
        })
        .collect()
}
fn folder_count(w: &Workspace, id: PanelId, folders: &FolderSnapshots) -> Result<usize, String> {
    let source = folders
        .get(&id)
        .ok_or("folder is not loaded; query folder get and wait before fitting")?;
    if !source.ready {
        return Err("folder snapshot is loading or failed; query folder get before fitting".into());
    }
    if w.panel(id).and_then(Panel::folder) != Some(source.root.as_path()) {
        return Err(
            "folder mapping changed; apply it and wait for the new snapshot before fitting".into(),
        );
    }
    Ok(source.labels.len())
}
pub(in crate::pane::control) fn live_query(state: &PaneApp, id: PanelId) -> serde_json::Value {
    let panel = state.workspace.panel(id).unwrap();
    if panel.folder().is_none() {
        return query(&state.workspace, id);
    }
    let source = state.folders.get(&id);
    let ready = source.is_some_and(|s| !s.loading && s.status.is_none());
    let n = source.map_or(0, |s| s.items.len());
    let columns = if panel.list_view() {
        1
    } else {
        ((panel.rect().width - layout::PADDING * 2.0) / metrics(&state.workspace).cell_width)
            .floor()
            .max(1.0) as usize
    };
    let rows = n.div_ceil(columns);
    let height = content_height(&state.workspace, id, columns, &snapshots(state), None).ok();
    json!({"supported":true,"ready":ready,"view":if panel.list_view(){"list"}else{"icons"},"item_count":n,"icon_columns":if panel.list_view(){serde_json::Value::Null}else{json!(columns)},"content_rows":rows,"current_path":source.map(|s|&s.path),"required_height_dip":if ready{json!(height)}else{serde_json::Value::Null},"gap_px":snap::GAP_PX})
}

pub(super) fn members(w: &Workspace, id: PanelId) -> Vec<PanelId> {
    w.tab_group(id)
        .map_or_else(|| vec![id], |g| g.members.clone())
}
pub(super) fn count(w: &Workspace, id: PanelId) -> usize {
    w.desktop_items()
        .iter()
        .filter(
            |i| matches!(i.placement(), DesktopPlacement::Pane { pane_id, .. } if *pane_id == id),
        )
        .count()
}
pub(super) fn metrics(w: &Workspace) -> layout::Grid {
    layout::desktop_grid(
        0.0,
        0.0,
        layout::DESKTOP_ICON_SIZE,
        w.pane_options().grid_scale,
    )
}
#[cfg(test)]
pub(super) fn size(w: &Workspace, id: PanelId, columns: u32) -> Result<(f32, f32), String> {
    size_with_folders(w, id, columns, &HashMap::new(), None)
}
pub(super) fn size_with_folders(
    w: &Workspace,
    id: PanelId,
    columns: u32,
    folders: &FolderSnapshots,
    max_rows: Option<u32>,
) -> Result<(f32, f32), String> {
    if !(1..=64).contains(&columns) {
        return Err("icon_columns must be 1..64".into());
    }
    for member in members(w, id) {
        let p = w.panel(member).ok_or("panel does not exist")?;
        if p.is_search() {
            return Err("content fitting does not support dynamic search panels".into());
        }
        if p.locked() {
            return Err("explicitly unlock the panel before fitting or arranging".into());
        }
        if p.collapsed() {
            return Err("expand the panel before fitting or arranging".into());
        }
    }
    if max_rows.is_some_and(|n| n == 0 || n > 10000) {
        return Err("max_rows must be 1..10000".into());
    }
    let panel = w.panel(id).unwrap();
    let g = metrics(w);
    let group = members(w, id);
    let list_minimum = if group.iter().any(|member| w.panel(*member).unwrap().list_view()) {
        layout::pane_minimum((layout::LIST_CELL_WIDTH, layout::LIST_ROW), false, false, None).0
    } else { 0.0 };
    let fixed_columns = group.iter().filter(|member| w.panel(**member).is_some_and(|p| p.fixed_grid() && !p.free_layout() && !p.list_view()))
        .map(|member| fixed_grid::extent(w, *member).0).max().unwrap_or(0);
    let width = if panel.list_view() {
        panel.rect().width.max(list_minimum)
    } else {
        let columns = (columns as usize).max(fixed_columns).max(((list_minimum - layout::PADDING * 2.0) / g.cell_width).ceil().max(0.0) as usize);
        layout::icon_width(columns, g, group.len() > 1).max(fixed_grid::minimum_width(w,id))
    };
    let actual = ((width - layout::PADDING * 2.0) / g.cell_width).floor().max(1.0) as usize;
    let height = members(w, id).into_iter().try_fold(0.0_f32, |height, member| {
        content_height(w, member, actual, folders, max_rows).map(|value| height.max(value))
    })?;
    Ok((width, height))
}

fn labels<'a>(w: &'a Workspace, id: PanelId, folders: &'a FolderSnapshots) -> Result<Vec<&'a str>, String> {
    if w.panel(id).unwrap().folder().is_some() {
        folder_count(w, id, folders)?;
        Ok(folders[&id].labels.iter().map(String::as_str).collect())
    } else {
        Ok(super::super::super::ordered_desktop_items(w, id).into_iter().map(|item| item.display_name()).collect())
    }
}
pub(super) fn content_height(w: &Workspace, id: PanelId, columns: usize, folders: &FolderSnapshots, max_rows: Option<u32>) -> Result<f32, String> {
    let panel = w.panel(id).unwrap();
    let g = metrics(w);
    if panel.free_layout() && !panel.list_view() {
        let bottom = free_layout::extent(w, id).1.max(g.icon_size + layout::LABEL_OFFSET + 1.0);
        let bottom = max_rows.map_or(bottom, |rows| bottom.min(rows as f32 * g.cell_height));
        return Ok(layout::HEADER + layout::PADDING * 2.0 + bottom);
    }
    let names = labels(w, id, folders)?;
    if panel.list_view() {
        let rows = names.len().min(max_rows.map_or(usize::MAX, |n| n as usize)).max(1);
        return Ok(layout::HEADER + if panel.folder().is_some() { layout::LIST_HEADER + layout::PADDING } else { layout::PADDING * 2.0 } + rows as f32 * layout::LIST_ROW);
    }
    if panel.fixed_grid() {
        let items = super::super::super::ordered_desktop_items(w, id);
        let rows = fixed_grid::extent(w, id).1.min(max_rows.map_or(usize::MAX, |n| n as usize));
        let heights: Vec<_> = (0..rows).map(|row| layout::icon_row_height(g,
            items.iter().filter(|item| matches!(item.placement(), DesktopPlacement::Pane { position, .. } if position.row as usize == row)).map(|item| item.display_name()))).collect();
        return Ok(layout::pane_content_height(rows.max(1), g.cell_height, &heights));
    }
    let rows: Vec<_> = names.chunks(columns).take(max_rows.map_or(usize::MAX, |n| n as usize))
        .map(|row| layout::icon_row_height(g, row.iter().copied())).collect();
    Ok(layout::pane_content_height(rows.len().max(1), g.cell_height, &rows))
}

pub(in crate::pane::control) fn minimum(w: &Workspace, id: PanelId, width: f32, folders: &FolderSnapshots) -> (f32, f32) {
    let panel = w.panel(id).unwrap();
    if panel.is_search() { return (RectDip::MIN_WIDTH, RectDip::MIN_HEIGHT); }
    let g = metrics(w);
    let columns = ((width - layout::PADDING * 2.0) / g.cell_width).floor().max(1.0) as usize;
    let cell = if panel.list_view() { (layout::LIST_CELL_WIDTH, layout::LIST_ROW) } else { (g.cell_width, g.cell_height) };
    let first = if panel.list_view() { Some(layout::LIST_ROW) } else {
        labels(w, id, folders).ok().filter(|names| !names.is_empty())
            .map(|names| layout::icon_row_height(g, names.iter().take(columns).copied()))
    };
    let mut minimum = layout::pane_minimum(cell, false, members(w, id).len() > 1, first);
    minimum.0 = minimum.0.max(fixed_grid::minimum_width(w,id));
    minimum
}

pub(in crate::pane::control) fn query(w: &Workspace, id: PanelId) -> serde_json::Value {
    let p = w.panel(id).unwrap();
    if p.is_search() || p.folder().is_some() {
        return json!({"supported":false,"reason":"desktop panels only"});
    }
    let g = metrics(w);
    let columns = ((p.rect().width - layout::PADDING * 2.0) / g.cell_width)
        .floor()
        .max(1.0) as usize;
    let n = count(w, id);
    let window_count = members(w, id)
        .into_iter()
        .map(|id| count(w, id))
        .max()
        .unwrap_or(0);
    let minimum = minimum(w, id, p.rect().width, &HashMap::new());
    json!({"supported":true,"item_count":n,"window_item_count":window_count,"icon_columns":columns,"fixed_icon_positions":p.fixed_grid(),"align_icons_to_grid":!p.free_layout(),"content_rows":if p.fixed_grid() && !p.free_layout() && !p.list_view(){fixed_grid::extent(w,id).1}else{n.div_ceil(columns)},
        "cell_dip":{"width":g.cell_width,"height":g.cell_height},"icon_size_dip":g.icon_size,
        "minimum_size_dip":{"width":minimum.0,"height":minimum.1},"gap_px":snap::GAP_PX,
        "required_height_dip":members(w,id).into_iter().filter_map(|member|content_height(w,member,columns,&HashMap::new(),None).ok()).fold(0.0,f32::max)})
}
pub(super) fn pixel_size(
    w: &Workspace,
    id: PanelId,
    columns: u32,
    scale: f32,
    folders: &FolderSnapshots,
    max_rows: Option<u32>,
) -> Result<(f32, f32), String> {
    let (width, height) = size_with_folders(w, id, columns, folders, max_rows)?;
    // Ceil avoids losing the last cell to fractional-DPI rounding.
    Ok(((width * scale).ceil(), (height * scale).ceil()))
}
