//! Relative placement against an existing panel window.
use super::*;

pub(super) fn expand(
    context: &LayoutContext<'_>,
    pane_id: &str,
    target_pane_id: &str,
    side: &SnapSide,
    align: &SnapAlign,
    icon_columns: &Option<u32>,
) -> Result<Vec<Operation>, String> {
    let LayoutContext {
        workspace: w,
        positions,
        monitors,
        folders,
    } = *context;
    let mut result = Vec::new();
    let id = parse(pane_id)?;
    let target = parse(target_pane_id)?;
    let moving = members(w, id);
    let anchors = members(w, target);
    if moving.iter().any(|id| anchors.contains(id)) {
        return Err("cannot snap a panel to its own shared window".into());
    }
    for member in moving.iter().chain(anchors.iter()) {
        let p = w.panel(*member).ok_or("panel does not exist")?;
        if p.is_search() {
            return Err("relative snapping requires fixed-height desktop or folder panels; use geometry for search".into());
        }
        if p.collapsed() {
            return Err("expand both panels before relative snapping".into());
        }
    }
    let anchor = current(w, target, positions, monitors)?;
    let m = geometry::nearest_monitor(anchor, monitors)
        .ok_or("target monitor unavailable")?;
    let scale = m.dpi as f32 / 96.0;
    let (width, height) = if let Some(columns) = icon_columns {
        pixel_size(w, id, *columns, scale, folders, None)?
    } else {
        let r = current(w, id, positions, monitors)?;
        let source_scale = geometry::nearest_monitor(r, monitors)
            .ok_or("source monitor unavailable")?.dpi as f32 / 96.0;
        ((r.width / source_scale * scale).round(), (r.height / source_scale * scale).round())
    };
    let horizontal = snap::AxisTargets::new(anchor.x.round() as i32,
        (anchor.x + anchor.width).round() as i32, width as i32, snap::gap_px());
    let vertical = snap::AxisTargets::new(anchor.y.round() as i32,
        (anchor.y + anchor.height).round() as i32, height as i32, snap::gap_px());
    let aligned = |targets: &snap::AxisTargets| match align {
        SnapAlign::Start => targets.start,
        SnapAlign::Center => targets.center,
        SnapAlign::End => targets.end,
    };
    let (x, y) = match side {
        SnapSide::Left => (horizontal.before, aligned(&vertical)),
        SnapSide::Right => (horizontal.after, aligned(&vertical)),
        SnapSide::Top => (aligned(&horizontal), vertical.before),
        SnapSide::Bottom => (aligned(&horizontal), vertical.after),
    };
    let (x, y) = (x as f32, y as f32);
    let placed = RectDip {
        x,
        y,
        width,
        height,
    };
    for panel in w
        .panels()
        .iter()
        .filter(|p| !moving.contains(&p.id()) && !anchors.contains(&p.id()))
    {
        if overlaps(placed, current(w, panel.id(), positions, monitors)?) {
            return Err(format!(
                "snap would overlap panel {}; move it first",
                panel.id().get()
            ));
        }
    }
    let proposed = wire(
        id,
        m,
        x - m.work_area.x as f32,
        y - m.work_area.y as f32,
        width,
        height,
    );
    if let Operation::Geometry {
        x,
        y,
        width,
        height,
        ..
    } = &proposed
    {
        geometry::convert(
            m,
            RectDip {
                x: *x,
                y: *y,
                width: *width,
                height: *height,
            },
        )?;
    }
    result.push(proposed);
    Ok(result)
}
