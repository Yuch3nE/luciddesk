//! Single-panel state changes, geometry and item transfers.
use super::*;

pub(super) fn apply(
    state: &Rc<RefCell<PaneApp>>,
    id: PanelId,
    event: Event,
) -> Result<bool, String> {
    let mut s = state.borrow_mut();
    match event {
        Event::DetachTab(_)
        | Event::PreviewPaneMove
        | Event::FinishPaneMove(_)
        | Event::RenameTab(_)
        | Event::MoveTabId(..)
        | Event::ToggleHeaderDivider
        | Event::ToggleCompactMenu
        | Event::SetTitleEmojiColor(_)
        | Event::NewTab(_)
        | Event::SelectTab(_)
        | Event::CloseTab
        | Event::CloseTabId(_)
        | Event::MoveTab(_) => unreachable!("Tabs handled before borrowing PaneApp"),
        Event::SortFolder(_)
        | Event::FolderItemCreated(_)
        | Event::SetFolderColumns(_)
        | Event::ToggleFolderColumn(_)
        | Event::FolderBack
        | Event::FolderHome
        | Event::ExportBackup
        | Event::CreateBackup
        | Event::RestoreBackupPath(_)
        | Event::ExportBackupPath(_)
        | Event::DeleteBackup(_)
        | Event::RestoreBackup
        | Event::OpenBackups
        | Event::OpenConfigDirectory
        | Event::ReloadConfig
        | Event::RetryDesktop
        | Event::ToggleListView
        | Event::ToggleSearch
        | Event::EnableSearch
        | Event::FileDrag(_)
        | Event::NewFolder
        | Event::MapFolder(_)
        | Event::ChangeFolder
        | Event::SetFolder(_)
        | Event::OpenFolder => unreachable!("Handled before borrowing PaneApp"),
        Event::RenameTitle | Event::ClosePane | Event::Settings => {
            unreachable!("Handled before borrowing PaneApp")
        }
        Event::SetTitle(title) => {
            let old = s.workspace.clone();
            s.workspace
                .panel_mut(id)
                .ok_or(crate::i18n::text("ui-group-not-found"))?
                .set_title(title.clone());
            if let Err(error) = save(&mut s) {
                s.workspace = old;
                return Err(error);
            }
            if let Some(view) = s.views.iter().find(|view| view.id == id) {
                view.model.borrow_mut().title = title;
                unsafe {
                    InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
                }
            }
            refresh_changed_views(&mut s, true);
        }
        Event::PaneItemFocus
        | Event::BeginItemMenu(_)
        | Event::EndItemMenu
        | Event::RenameItem(_) => unreachable!("Handled before borrowing PaneApp"),
        Event::Theme(_)
        | Event::Material(_)
        | Event::SetCornerRadius(_)
        | Event::SetIconGrid(_)
        | Event::SetPanelText(_)
        | Event::ToggleTextProtection
        | Event::ToggleBorder
        | Event::ToggleSnap
        | Event::ResetPaneOptions => {
            unreachable!("Handled before borrowing PaneApp")
        }
        Event::ToggleLocked => {
            let panel = s
                .workspace
                .panel_mut(id)
                .ok_or(crate::i18n::text("ui-group-not-found"))?;
            let enabled = !panel.locked();
            panel.set_locked(enabled);
            if let Err(error) = save(&mut s) {
                s.workspace.panel_mut(id).unwrap().set_locked(!enabled);
                return Err(error);
            }
            if let Some(view) = s.views.iter().find(|v| v.id == id) {
                view.model.borrow_mut().locked = enabled;
                unsafe {
                    InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
                }
            }
        }
        Event::ToggleTopmost => {
            let panel = s
                .workspace
                .panel_mut(id)
                .ok_or(crate::i18n::text("ui-group-not-found"))?;
            let enabled = !panel.always_on_top();
            panel.set_always_on_top(enabled);
            if let Err(error) = save(&mut s) {
                s.workspace
                    .panel_mut(id)
                    .unwrap()
                    .set_always_on_top(!enabled);
                return Err(error);
            }
            if let Some(view) = s.views.iter().find(|v| v.id == id) {
                window::set_layer(view.window.hwnd().cast(), enabled);
            }
        }
        Event::Refresh => {
            if let Some(source) = s.folders.get(&id) {
                source.refresh();
            } else {
                hybrid::refresh_icons(&mut s);
            }
        }
        Event::Moving(rect) | Event::Sizing(rect, _, _) => {
            if matches!(event, Event::Moving(_)) && tabs::preview_merge(&s, id) {
                return Ok(false);
            }
            if !s.workspace.pane_options().snap {
                return Ok(false);
            }
            let peers: Vec<_> = s
                .views
                .iter()
                .filter(|view| view.id != id)
                .filter(|view| unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(
                        view.window.hwnd().cast(),
                    ) != 0
                })
                .filter_map(|view| {
                    let mut bounds = RECT::default();
                    (unsafe { GetWindowRect(view.window.hwnd().cast(), &raw mut bounds) } != 0)
                        .then_some(bounds)
                })
                .collect();
            if let Some(view) = s.views.iter().find(|view| view.id == id) {
                let scale =
                    unsafe { GetDpiForWindow(view.window.hwnd().cast()) }.max(96) as f32 / 96.0;
                if let Event::Sizing(_, proposal, edge) = event {
                    unsafe {
                        snap::resize(
                            &mut *rect,
                            &proposal,
                            edge,
                            &peers,
                            snap::GAP_PX,
                            (14.0 * scale).round() as i32,
                        );
                    }
                    return Ok(false);
                }
                unsafe {
                    use windows_sys::Win32::Graphics::Gdi::{
                        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
                    };
                    let monitor = MonitorFromRect(rect, MONITOR_DEFAULTTONEAREST);
                    let mut info = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    let work =
                        (GetMonitorInfoW(monitor, &raw mut info) != 0).then_some(info.rcWork);
                    snap::snap(
                        &mut *rect,
                        &peers,
                        work.as_ref(),
                        snap::GAP_PX, // Physical pixels at every DPI.
                        (14.0 * scale).round() as i32,
                    );
                }
            }
        }
        Event::ToggleGridAlignment => {
            let previous = s.workspace.clone();
            free_layout::toggle(&mut s.workspace, id)?;
            if let Err(error) = save(&mut s) { s.workspace = previous; return Err(error); }
            refresh_views(&mut s);
        }
        Event::ToggleFixedGrid => {
            let previous = s.workspace.clone();
            fixed_grid::toggle(&mut s.workspace, id)?;
            if let Err(error) = save(&mut s) { s.workspace = previous; return Err(error); }
            refresh_views(&mut s);
        }
        Event::SortPane(target, descending) => {
            let previous = s.workspace.clone();
            if sorting::apply(&mut s.workspace, target, descending)? {
                if let Err(error) = save(&mut s) {
                    s.workspace = previous;
                    return Err(error);
                }
                refresh_views(&mut s);
            }
        }
        Event::ToggleAutoHide => {
            if s.workspace.panel(id).is_none() {
                return Ok(false);
            }
            let old = s.workspace.clone();
            let panel = s.workspace.panel_mut(id).unwrap();
            let enabled = !panel.auto_hide();
            panel.set_auto_hide(enabled);
            if let Err(error) = save(&mut s) {
                s.workspace = old;
                return Err(error);
            }
            if let Some(view) = s.views.iter().find(|view| view.id == id) {
                view.model.borrow_mut().auto_hide = enabled;
                window::update_auto_hide(view.window.hwnd().cast(), enabled);
            }
            if !enabled {
                show_collapsed(&s, id, s.workspace.panel(id).unwrap().collapsed());
            }
        }
        Event::Activate(_) | Event::ActivateSelection | Event::Peek | Event::FileCommand(_) => {
            unreachable!("Handled before borrowing PaneApp")
        }
        Event::Geometry(rect) => {
            if s.workspace.panel(id).is_none() {
                return Ok(false);
            }
            let previous = s.workspace.clone();
            let collapsed = s.views.iter().find(|v| v.id == id).map_or_else(
                || s.workspace.panel(id).unwrap().collapsed(),
                |v| v.model.borrow().collapsed,
            );
            if let Some(panel) = s.workspace.panel_mut(id) {
                panel.set_rect(if collapsed {
                    RectDip {
                        height: panel.rect().height,
                        ..rect
                    }
                } else {
                    rect
                });
            }
            s.workspace.sync_tab_windows();
            let rectangles: Vec<_> = s
                .workspace
                .panels()
                .iter()
                .map(|p| (p.id(), p.rect()))
                .collect();
            let layout = display_layout::capture(&mut s);
            if let Err(error) = s.store.save_panel_geometry(
                &rectangles,
                layout
                    .as_ref()
                    .map(|(topology, entries)| (topology.as_str(), entries.as_slice())),
            ) {
                s.workspace = previous;
                return Err(error.to_string());
            }
        }
        Event::AutoHideCollapsed(collapsed) => {
            // Hover state belongs to the live window, never to the persisted panel.
            if s.workspace.panel(id).is_some_and(Panel::auto_hide) {
                show_collapsed(&s, id, collapsed);
            }
        }
        Event::Collapse => {
            let Some(panel) = s.workspace.panel(id) else {
                return Ok(false);
            };
            let collapsed = !s
                .views
                .iter()
                .find(|v| v.id == id)
                .map_or(panel.collapsed(), |v| v.model.borrow().collapsed);
            let previous = s.workspace.clone();
            s.workspace.panel_mut(id).unwrap().set_collapsed(collapsed);
            if let Err(error) = save(&mut s) {
                s.workspace = previous;
                return Err(error);
            }
            show_collapsed(&s, id, collapsed);
        }
        Event::Drop { index, point, offset } => {
            let source = items_for(&s, id);
            let Some(_) = source.get(index) else {
                return Ok(false);
            };
            let indices: Vec<usize> = s
                .views
                .iter()
                .find(|v| v.id == id)
                .map(|v| {
                    let m = v.model.borrow();
                    if m.selection.contains(&index) {
                        m.selection.iter().copied().collect()
                    } else {
                        vec![index]
                    }
                })
                .unwrap_or_else(|| vec![index]);
            let target_window = unsafe { WindowFromPoint(point) };
            let target = s.views.iter().rev().find_map(|view| {
                if s.workspace.panel(view.id).is_some_and(Panel::is_search) {
                    return None;
                }
                let hwnd = view.window.hwnd().cast();
                if target_window != hwnd {
                    return None;
                }
                if unsafe { IsWindow(hwnd) } == 0 {
                    return None;
                }
                let model = view.model.borrow();
                if model.collapsed { return None; }
                let mut local = point;
                let mut bounds = RECT::default();
                unsafe {
                    if ScreenToClient(hwnd, &raw mut local) == 0
                        || windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &raw mut bounds) == 0 { return None; }
                }
                if local.x < bounds.left || local.x >= bounds.right
                    || local.y < bounds.top || local.y >= bounds.bottom { return None; }
                let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
                let (x, y) = (local.x as f32 / scale, local.y as f32 / scale);
                let grid = model.grid(
                    (bounds.right - bounds.left) as f32 / scale,
                    (bounds.bottom - bounds.top) as f32 / scale,
                );
                let (at, free) = model.drop_destination(grid, x, y, offset);
                Some((view.id, at, free))
            });
            if let Some((target, at, free)) = target {
                if let Some(path) = s.folders.get(&target).map(|source| source.path.clone()) {
                    let identities: Vec<_> = indices
                        .iter()
                        .filter_map(|i| source.get(*i))
                        .map(|i| i.identity.clone())
                        .collect();
                    if !folder::accepts_copy(&identities, &path) {
                        return Ok(false);
                    }
                    let owner = s
                        .views
                        .iter()
                        .find(|v| v.id == target)
                        .unwrap()
                        .window
                        .hwnd() as isize;
                    window::post_action(owner as _, move || {
                        if let Err(error) = luciddesk_shell::copy_to_folder(
                            windows::Win32::Foundation::HWND(owner as _),
                            &identities,
                            &path,
                        ) {
                            window::error(&error.to_string());
                        }
                    });
                    return Ok(false);
                }
                if s.workspace.panel(id).is_some_and(|p| p.folder().is_some()) {
                    return Ok(false);
                }
                if let Some(destination) = free {
                    let previous = s.workspace.clone();
                    let keys: Vec<_> = indices.iter().filter_map(|i| source.get(*i)).map(|i| i.identity.persistent_key()).collect();
                    free_layout::move_items(&mut s.workspace, id, target, &keys, &source[index].identity.persistent_key(), destination)?;
                    if target != id && !s.workspace.panel(id).is_some_and(Panel::fixed_grid) {
                        let remaining = items_for(&s, id); set_order(&mut s.workspace, id, &remaining);
                    }
                    if let Err(error) = save(&mut s) { s.workspace = previous; return Err(error); }
                } else { transfer_many(&mut s, id, &indices, target, at)?; }
                for view in &s.views {
                    view.model.borrow_mut().clear_selection();
                }
                refresh_views(&mut s);
            } else if hybrid::release(&mut s, id, &indices, point)? {
                refresh_views(&mut s);
            }
        }
        Event::Exit => windows_window::quit(),
        Event::New => unreachable!(),
    }
    Ok(false)
}
