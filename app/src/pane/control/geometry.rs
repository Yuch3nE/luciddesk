//! Monitor-relative DIP at the boundary; physical pixels for native placement.
use super::*;
use luciddesk_window::MonitorDescriptor;

pub(super) fn monitors(monitors: &[MonitorDescriptor]) -> serde_json::Value {
    json!({"coordinate_space":"monitor_work_area_dip","monitors":monitors.iter().map(|m| {
        let scale = m.dpi as f32 / 96.0;
        json!({"id":m.id.as_str(),"primary":m.primary,"dpi":m.dpi,
            "work_area_px":{"x":m.work_area.x,"y":m.work_area.y,"width":m.work_area.width,"height":m.work_area.height},
            "work_area_dip":{"width":m.work_area.width as f32/scale,"height":m.work_area.height as f32/scale}})
    }).collect::<Vec<_>>()})
}
pub(super) fn convert(m: &MonitorDescriptor, r: RectDip) -> Result<(RectDip, RectDip), String> {
    let scale = m.dpi as f32 / 96.0;
    if ![r.x, r.y, r.width, r.height, scale]
        .into_iter()
        .all(f32::is_finite)
        || scale <= 0.0
        || r.x < 0.0
        || r.y < 0.0
        || r.width <= 0.0
        || r.height <= 0.0
        || (r.x + r.width) * scale > m.work_area.width as f32 + 0.01
        || (r.y + r.height) * scale > m.work_area.height as f32 + 0.01
    {
        return Err("geometry must have positive dimensions and fit the monitor work area".into());
    }
    let px = RectDip {
        x: m.work_area.x as f32 + (r.x * scale).round(),
        y: m.work_area.y as f32 + (r.y * scale).round(),
        width: (r.width * scale).round(),
        height: (r.height * scale).round(),
    };
    if px.x + px.width > (m.work_area.x + m.work_area.width) as f32
        || px.y + px.height > (m.work_area.y + m.work_area.height) as f32
    {
        return Err("rounded geometry exceeds the monitor work area".into());
    }
    let stored = RectDip {
        x: px.x / scale,
        y: px.y / scale,
        width: px.width / scale,
        height: px.height / scale,
    };
    Ok((stored, px))
}
/// Read-only native snapshot. Tab members share the same visible window bounds.
#[derive(Clone, Default, PartialEq)]
pub(super) struct LayoutObservation {
    pub positions: HashMap<PanelId, RectDip>,
    pub collapsed: std::collections::HashSet<PanelId>,
}
impl LayoutObservation {
    pub(super) fn expanded_bounds(&self, panel: &Panel) -> Option<RectDip> {
        let mut bounds = *self.positions.get(&panel.id())?;
        if self.collapsed.contains(&panel.id()) {
            let rect = panel.rect();
            bounds.height = (rect.height * (bounds.width / rect.width)).round();
        }
        Some(bounds)
    }
}
pub(super) fn observe_layout(state: &PaneApp) -> LayoutObservation {
    let mut observed = LayoutObservation::default();
    for view in &state.views {
        let hwnd = view.window.hwnd().cast();
        let mut r = RECT::default();
        if unsafe { IsWindowVisible(hwnd) == 0 || GetWindowRect(hwnd, &raw mut r) == 0 } { continue; }
        let bounds = RectDip::from_bounds(r.left as f32, r.top as f32,
            (r.right - r.left) as f32, (r.bottom - r.top) as f32);
        let members = state.workspace.tab_group(view.id).map_or_else(|| vec![view.id], |g| g.members.clone());
        for id in members {
            observed.positions.insert(id, bounds);
            if view.model.borrow().collapsed { observed.collapsed.insert(id); }
        }
    }
    observed
}

pub(super) fn window_bounds(s: &PaneApp, id: PanelId) -> serde_json::Value {
    let active = s.workspace.tab_group(id).map_or(id, |g| g.active);
    let Some(view) = s.views.iter().find(|v| v.id == active) else {
        return serde_json::Value::Null;
    };
    let mut r = RECT::default();
    if unsafe { GetWindowRect(view.window.hwnd().cast(), &raw mut r) } == 0 {
        return serde_json::Value::Null;
    }
    json!({"x":r.left,"y":r.top,"width":r.right-r.left,"height":r.bottom-r.top})
}

pub(super) fn query(
    s: &PaneApp,
    panel: &Panel,
    monitors: &[MonitorDescriptor],
) -> serde_json::Value {
    let px = s
        .runtime
        .as_ref()
        .and_then(|r| r.layouts.positions.get(&panel.id()))
        .copied();
    let px = px.unwrap_or_else(|| scale_bounds(panel.rect(), monitors));
    describe(px, monitors)
}

pub(super) fn scale_bounds(r: RectDip, monitors: &[MonitorDescriptor]) -> RectDip {
    let scale = monitors.first().map_or(1.0, |m| m.dpi as f32 / 96.0);
    RectDip {
        x: r.x * scale,
        y: r.y * scale,
        width: r.width * scale,
        height: r.height * scale,
    }
}
pub(super) fn nearest_monitor(px: RectDip, monitors: &[MonitorDescriptor]) -> Option<&MonitorDescriptor> {
    monitors.iter().min_by_key(|m| {
        let w = m.work_area;
        let x = px.x + px.width / 2.0;
        let y = px.y + px.height / 2.0;
        let dx = x - x.clamp(w.x as f32, (w.x + w.width) as f32);
        let dy = y - y.clamp(w.y as f32, (w.y + w.height) as f32);
        (dx * dx + dy * dy) as u64
    })
}
pub(super) fn describe(px: RectDip, monitors: &[MonitorDescriptor]) -> serde_json::Value {
    let monitor = nearest_monitor(px, monitors);
    let Some(m) = monitor else {
        return serde_json::Value::Null;
    };
    let scale = m.dpi as f32 / 96.0;
    json!({"monitor_id":m.id.as_str(),"x":(px.x-m.work_area.x as f32)/scale,"y":(px.y-m.work_area.y as f32)/scale,"width":px.width/scale,"height":px.height/scale})
}

pub(super) fn present(state: &Rc<RefCell<PaneApp>>, before: &Workspace) -> Result<(), String> {
    let positions: Vec<_> = {
        let s = state.borrow();
        let Some(runtime) = s.runtime.as_ref() else {
            return Ok(());
        };
        s.views
            .iter()
            .filter_map(|v| {
                let p = s.workspace.panel(v.id)?;
                // The surviving view may still be at its old window's physical position,
                // even if the source and destination had equal stored DIP bounds.
                let previous_group = before.tab_group(v.id);
                let merged = s.workspace.tab_group(v.id).is_some_and(|group| {
                    group.members.iter().any(|id| {
                        *id != v.id && previous_group.is_none_or(|old| !old.members.contains(id))
                    })
                });
                if !merged && before.panel(v.id).is_some_and(|old| old.rect() == p.rect()) {
                    return None;
                }
                let r = *runtime.layouts.positions.get(&v.id)?;
                let scale = r.width / p.rect().width;
                let height = if p.is_search() {
                    56.0 * scale
                } else if v.model.borrow().collapsed {
                    layout::HEADER * scale
                } else {
                    r.height
                };
                Some((v.window.hwnd() as isize, r, height, p.is_search()))
            })
            .collect()
    };
    for (hwnd, r, height, search) in positions {
        unsafe {
            if SetWindowPos(
                hwnd as _,
                std::ptr::null_mut(),
                r.x as i32,
                r.y as i32,
                r.width as i32,
                height.round() as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let mut actual = RECT::default();
            if GetWindowRect(hwnd as _, &raw mut actual) == 0
                || actual.left != r.x as i32
                || actual.top != r.y as i32
                || actual.right - actual.left != r.width as i32
                || (!search && actual.bottom - actual.top != height.round() as i32)
            {
                return Err("native window geometry differs from committed layout".into());
            }
            if search {
                PostMessageW(hwnd as _, search::RESTORE_LAYOUT, 0, 0);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_dpi_negative_origin_roundtrips_and_rejects_offscreen() {
        let m = MonitorDescriptor {
            id: luciddesk_core::MonitorId::new("left"),
            dpi: 144,
            primary: false,
            bounds: luciddesk_window::PixelRect {
                x: -1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
            work_area: luciddesk_window::PixelRect {
                x: -1920,
                y: 30,
                width: 1920,
                height: 1020,
            },
        };
        let (stored, px) = convert(&m, RectDip::new(100.0, 80.0, 480.0, 360.0)).unwrap();
        assert_eq!(px.x, -1770.0);
        assert_eq!(px.y, 150.0);
        assert_eq!(px.width, 720.0);
        assert_eq!(stored.x * 1.5, px.x);
        for r in [
            RectDip {
                x: -1.0,
                y: 0.0,
                width: 480.0,
                height: 360.0,
            },
            RectDip {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 360.0,
            },
            RectDip::new(1200.0, 0.0, 480.0, 360.0),
            RectDip {
                x: f32::NAN,
                y: 0.0,
                width: 480.0,
                height: 360.0,
            },
        ] {
            assert!(convert(&m, r).is_err());
        }
    }
}
