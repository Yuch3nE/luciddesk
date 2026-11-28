//! Physical window bounds are remembered independently for each display topology.
use super::*;
use luciddesk_window::MonitorDescriptor;
use std::time::{Duration, Instant};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Shared initial bounds for startup and user-created panels, in DIP.
pub(super) fn new_pane(workspace: &Workspace, search: bool) -> RectDip {
    let (width, height) = if search { (360.0, 200.0) } else { (480.0, 360.0) };
    let mut rect = RectDip::new(240.0, 240.0, width, height);
    // Compare origins rather than intersections: a small cascade intentionally overlaps.
    while workspace.panels().iter().any(|panel| {
        let existing = panel.rect();
        (existing.x - rect.x).abs() < 1.0 && (existing.y - rect.y).abs() < 1.0
    }) {
        rect.x -= 24.0;
        rect.y -= 24.0;
    }
    rect
}

pub(super) struct Layouts {
    monitors: Vec<MonitorDescriptor>,
    pending: Option<(Vec<MonitorDescriptor>, Instant)>,
    next_check: Option<Instant>,
    pub positions: HashMap<PanelId, RectDip>,
}
impl Default for Layouts {
    fn default() -> Self {
        Self {
            monitors: Vec::new(),
            pending: None,
            next_check: Some(Instant::now()),
            positions: HashMap::new(),
        }
    }
}
impl Layouts {
    pub(super) fn max_scale(&self) -> f32 {
        self.monitors.iter().map(|monitor| monitor.dpi).max().unwrap_or(96) as f32 / 96.0
    }

    pub(super) fn invalidate(&mut self) {
        self.next_check = Some(Instant::now());
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.next_check
    }
}
pub(super) fn key(monitors: &[MonitorDescriptor]) -> String {
    let mut parts: Vec<_> = monitors
        .iter()
        .map(|m| format!("{}:{:?}:{}", m.id.as_str(), m.work_area, m.dpi))
        .collect();
    parts.sort();
    parts.join("|")
}

fn fit(mut r: RectDip, monitors: &[MonitorDescriptor]) -> RectDip {
    let distance = |m: &&MonitorDescriptor| {
        let w = m.work_area;
        let x = r.x + r.width / 2.0;
        let y = r.y + r.height / 2.0;
        let dx = x - x.clamp(w.x as f32, (w.x + w.width) as f32);
        let dy = y - y.clamp(w.y as f32, (w.y + w.height) as f32);
        (dx * dx + dy * dy) as u64
    };
    if let Some(m) = monitors.iter().min_by_key(distance) {
        let w = m.work_area;
        r.width = r.width.min(w.width as f32).max(1.0);
        r.height = r.height.min(w.height as f32).max(1.0);
        r.x = r.x.clamp(w.x as f32, (w.x + w.width) as f32 - r.width);
        r.y = r.y.clamp(w.y as f32, (w.y + w.height) as f32 - r.height);
    }
    r
}

fn monitor_scale(r: RectDip, monitors: &[MonitorDescriptor]) -> f32 {
    monitors
        .iter()
        .find(|m| {
            let b = m.bounds;
            r.x >= b.x as f32
                && r.x < (b.x + b.width) as f32
                && r.y >= b.y as f32
                && r.y < (b.y + b.height) as f32
        })
        .or_else(|| monitors.first())
        .map_or(1.0, |m| m.dpi as f32 / 96.0)
}

pub(super) fn initialize(s: &mut PaneApp, monitors: Vec<MonitorDescriptor>) -> Result<(), String> {
    if monitors.is_empty() {
        return Ok(());
    }
    let saved: HashMap<_, _> = s
        .store
        .monitor_layout(&key(&monitors))
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    let Some(runtime) = &mut s.runtime else {
        return Ok(());
    };
    let primary_scale = monitors.first().map_or(1.0, |m| m.dpi as f32 / 96.0);
    let mut positions = HashMap::new();
    let ids: Vec<_> = s.workspace.panels().iter().map(Panel::id).collect();
    for id in ids {
        let panel = s.workspace.panel_mut(id).unwrap();
        let old = panel.rect();
        let physical = saved
            .get(&id)
            .or_else(|| runtime.layouts.positions.get(&id))
            .copied()
            .unwrap_or(RectDip::from_bounds(
                old.x * primary_scale,
                old.y * primary_scale,
                old.width * primary_scale,
                old.height * primary_scale,
            ));
        let physical = fit(physical, &monitors);
        let scale = monitor_scale(physical, &monitors);
        panel.set_rect(RectDip::from_bounds(
            physical.x / scale,
            physical.y / scale,
            physical.width / scale,
            physical.height / scale,
        ));
        positions.insert(id, physical);
    }
    for group in s.workspace.tab_groups() {
        if let Some(position) = positions.get(&group.active).copied() {
            for id in &group.members { positions.insert(*id, position); }
        }
    }
    s.workspace.sync_tab_windows();
    runtime.layouts = Layouts {
        monitors,
        pending: None,
        next_check: None,
        positions,
    };
    Ok(())
}

pub(super) fn place(s: &PaneApp, id: PanelId) {
    let Some(runtime) = &s.runtime else {
        return;
    };
    let Some(r) = runtime.layouts.positions.get(&id) else {
        return;
    };
    let Some(v) = s.views.iter().find(|v| v.id == id) else {
        return;
    };
    let scale = monitor_scale(*r, &runtime.layouts.monitors);
    let panel = s.workspace.panel(id).unwrap();
    let height = if panel.is_search() {
        56.0 * scale
    } else if v.model.borrow().collapsed {
        layout::HEADER * scale
    } else {
        r.height
    };
    unsafe {
        SetWindowPos(
            v.window.hwnd().cast(),
            std::ptr::null_mut(),
            r.x as i32,
            r.y as i32,
            r.width as i32,
            height as i32,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
        if panel.is_search() {
            PostMessageW(v.window.hwnd().cast(), search::RESTORE_LAYOUT, 0, 0);
        }
    }
}

pub(super) fn capture(s: &mut PaneApp) -> Option<(String, Vec<(PanelId, RectDip)>)> {
    let Some(runtime) = &mut s.runtime else {
        return None;
    };
    let current = luciddesk_window::enumerate_monitors();
    if runtime.layouts.monitors != current || runtime.layouts.pending.is_some() {
        return None;
    }
    for view in &s.views {
        let mut r = RECT::default();
        if unsafe { GetWindowRect(view.window.hwnd().cast(), &raw mut r) } == 0 {
            continue;
        }
        let scale = unsafe { GetDpiForWindow(view.window.hwnd().cast()) }.max(96) as f32 / 96.0;
        let panel = s.workspace.panel(view.id).unwrap();
        runtime.layouts.positions.insert(
            view.id,
            RectDip::from_bounds(
                r.left as f32,
                r.top as f32,
                (r.right - r.left) as f32,
                if view.model.borrow().collapsed || panel.is_search() {
                    panel.rect().height * scale
                } else {
                    (r.bottom - r.top) as f32
                },
            ),
        );
    }
    for group in s.workspace.tab_groups() {
        if let Some(position) = runtime.layouts.positions.get(&group.active).copied() {
            for id in &group.members { runtime.layouts.positions.insert(*id, position); }
        }
    }
    runtime
        .layouts
        .positions
        .retain(|id, _| s.workspace.panel(*id).is_some());
    let layout: Vec<_> = runtime
        .layouts
        .positions
        .iter()
        .map(|(id, r)| (*id, *r))
        .collect();
    Some((key(&current), layout))
}

pub(super) fn record(s: &mut PaneApp) -> Result<(), String> {
    if let Some((topology, layout)) = capture(s) {
        s.store.save_monitor_layout(&topology, &layout).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(super) fn tick(state: &Rc<RefCell<PaneApp>>) -> Result<(), String> {
    {
        let mut s = state.borrow_mut();
        let Some(runtime) = &mut s.runtime else { return Ok(()); };
        if runtime.layouts.next_check.is_none_or(|due| due > Instant::now()) {
            return Ok(());
        }
        // Retry transient empty topology and wait for a stable display arrangement.
        runtime.layouts.next_check = Some(Instant::now() + Duration::from_secs(2));
    }
    let current = luciddesk_window::enumerate_monitors();
    if current.is_empty() {
        return Ok(());
    }
    {
        let mut s = state.borrow_mut();
        let Some(runtime) = &mut s.runtime else {
            return Ok(());
        };
        if runtime.layouts.monitors == current {
            runtime.layouts.pending = None;
            runtime.layouts.next_check = None;
            return Ok(());
        }
        match &runtime.layouts.pending {
            Some((pending, since))
                if *pending == current && since.elapsed() >= Duration::from_secs(2) => {}
            Some((pending, _)) if *pending == current => return Ok(()),
            _ => {
                runtime.layouts.pending = Some((current, Instant::now()));
                return Ok(());
            }
        }
        initialize(&mut s, current)?;
    }
    let ids: Vec<_> = state.borrow().views.iter().map(|v| v.id).collect();
    for id in ids {
        place(&state.borrow(), id);
    }
    record(&mut state.borrow_mut())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_panels_share_default_bounds_and_cascade_only_at_occupied_origins() {
        let mut workspace = Workspace::new();
        let first = new_pane(&workspace, false);
        assert_eq!(first, RectDip::new(240.0, 240.0, 480.0, 360.0));
        workspace.add_panel(Panel::new(PanelId::new(1), "First", first)).unwrap();
        let second = new_pane(&workspace, false);
        assert_eq!(second, RectDip::new(216.0, 216.0, 480.0, 360.0));
        workspace.add_panel(Panel::new(PanelId::new(2), "Second", second)).unwrap();
        assert_eq!(new_pane(&workspace, false), RectDip::new(192.0, 192.0, 480.0, 360.0));
        assert_eq!(new_pane(&workspace, true), RectDip::new(192.0, 192.0, 360.0, 200.0));
        workspace.panel_mut(PanelId::new(1)).unwrap().set_rect(RectDip::new(300.0, 300.0, 480.0, 360.0));
        assert_eq!(new_pane(&workspace, false), first);
    }

    #[test]
    fn settled_layout_is_not_enumerated_until_invalidated() {
        let mut app = super::super::tests::test_state();
        app.runtime = Some(runtime::State::new(std::path::PathBuf::from("unused.json")));
        app.runtime.as_mut().unwrap().layouts.next_check = None;
        let state = Rc::new(RefCell::new(app));
        tick(&state).unwrap();
        assert!(state.borrow().runtime.as_ref().unwrap().layouts.pending.is_none());
        state.borrow_mut().runtime.as_mut().unwrap().layouts.invalidate();
        tick(&state).unwrap();
        let s = state.borrow();
        assert!(s.runtime.as_ref().unwrap().layouts.deadline().is_some());
    }
    #[test]
    fn removed_monitor_moves_panes_into_work_area_without_minimum_width() {
        let monitor = MonitorDescriptor {
            id: luciddesk_core::MonitorId::new("primary"),
            bounds: luciddesk_window::PixelRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            work_area: luciddesk_window::PixelRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1040,
            },
            dpi: 96,
            primary: true,
        };
        let r = fit(RectDip::new(-1200.0, -900.0, 80.0, 300.0), &[monitor]);
        assert_eq!(r, RectDip::new(0.0, 0.0, 80.0, 300.0));
    }
}
