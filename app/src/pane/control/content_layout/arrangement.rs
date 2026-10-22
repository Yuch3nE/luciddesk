//! Arrange panel columns within a monitor work area.
use super::*;

pub(super) fn expand(
    context: &LayoutContext<'_>,
    monitor_id: &str,
    columns: &[Vec<String>],
    icon_columns: &u32,
) -> Result<Vec<Operation>, String> {
    let LayoutContext {
        workspace: w,
        positions,
        monitors,
        folders,
    } = *context;
    let mut result = Vec::new();
    if columns.is_empty()
        || columns.len() > 16
        || columns.iter().any(Vec::is_empty)
        || columns.iter().map(Vec::len).sum::<usize>() > 256
    {
        return Err("expected 1..16 nonempty panel columns, at most 256 panels".into());
    }
    let m = monitors
        .iter()
        .find(|m| m.id.as_str() == monitor_id)
        .ok_or("unknown monitor ID")?;
    let scale = m.dpi as f32 / 96.0;
    let mut selected = std::collections::HashSet::new();
    let mut sizes = Vec::new();
    for column in columns {
        let mut cells = Vec::new();
        for raw in column {
            let id = parse(raw)?;
            let (width, height) = pixel_size(w, id, *icon_columns, scale, folders, None)?;
            for member in members(w, id) {
                if !selected.insert(member) {
                    return Err("duplicate panel or shared tab window in arrangement".into());
                }
            }
            cells.push((id, width, height));
        }
        sizes.push(cells);
    }
    let gap = snap::gap_px() as f32;
    let mut right = m.work_area.width as f32 - gap;
    let mut rectangles = Vec::new();
    // Input columns are left-to-right, panes within each are top-to-bottom.
    for column in sizes.iter().rev() {
        let width = column.iter().map(|(_, w, _)| *w).fold(0.0, f32::max);
        let x = right - width;
        let mut y = gap;
        if x < gap {
            return Err("arrangement exceeds work area width; use fewer panel columns".into());
        }
        for &(id, width, height) in column {
            if y + height > m.work_area.height as f32 - gap {
                return Err(
                    "arrangement exceeds work area height; use more panel columns or icon columns"
                        .into(),
                );
            }
            result.push(wire(id, m, x, y, width, height));
            rectangles.push(RectDip {
                x: m.work_area.x as f32 + x,
                y: m.work_area.y as f32 + y,
                width,
                height,
            });
            y += height + gap;
        }
        right = x - gap;
    }
    for p in w.panels().iter().filter(|p| !selected.contains(&p.id())) {
        let r = current(w, p.id(), positions, monitors)?;
        if rectangles.iter().any(|placed| overlaps(*placed, r)) {
            return Err(format!(
                "arrangement overlaps unselected panel {}; include or move it first",
                p.id().get()
            ));
        }
    }
    Ok(result)
}
