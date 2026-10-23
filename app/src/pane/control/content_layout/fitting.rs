//! Fit one panel to its content within its monitor.
use super::*;

pub(super) fn expand(
    context: &LayoutContext<'_>,
    op: &Operation,
    pane_id: &str,
) -> Result<Vec<Operation>, String> {
    let LayoutContext {
        workspace: w,
        positions,
        monitors,
        folders,
    } = *context;
    let id = parse(pane_id)?;
    let p = w.panel(id).ok_or("panel does not exist")?;
    let (columns, max_rows) = match op {
        Operation::Fit { icon_columns, .. } => (*icon_columns, None),
        Operation::FolderFit {
            icon_columns,
            max_rows,
            ..
        } => {
            if p.folder().is_none() {
                return Err("folder.fit requires a folder panel".into());
            }
            if p.list_view() && icon_columns.is_some() {
                return Err(
                    "icon_columns is only valid in icon view; omit it for list view".into(),
                );
            }
            (
                icon_columns.unwrap_or_else(||
                    ((p.rect().width - layout::PADDING * 2.0) / metrics(w, id).cell_width)
                        .floor()
                        .clamp(1.0, 64.0) as u32,
                ),
                *max_rows,
            )
        }
        _ => unreachable!(),
    };
    let old = current(w, id, positions, monitors)?;
    let m = geometry::nearest_monitor(old, monitors)
        .ok_or("no monitor available")?;
    let (width, height) = pixel_size(w, id, columns, m.dpi as f32 / 96.0, folders, max_rows)?;
    if width > m.work_area.width as f32 || height > m.work_area.height as f32 {
        return Err(
            "content does not fit the monitor; choose more icon columns or split the panel".into(),
        );
    }
    let x = (old.x - m.work_area.x as f32).clamp(0.0, m.work_area.width as f32 - width);
    let y = (old.y - m.work_area.y as f32).clamp(0.0, m.work_area.height as f32 - height);
    Ok(vec![wire(id, m, x, y, width, height)])
}
