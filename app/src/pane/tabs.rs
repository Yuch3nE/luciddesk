//! One native window per tab group; content IDs and worker channels never change.
use super::*;
use luciddesk_core::PaneTabs;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// A mixed group remains available when Explorer reconnects.
pub(super) fn independent(workspace: &Workspace, id: PanelId) -> bool {
    workspace
        .panel(id)
        .is_some_and(|p| p.folder().is_some() || p.is_search())
        || workspace.tab_group(id).is_some_and(|group| {
            group
                .members
                .iter()
                .any(|id| workspace.panel(*id).is_some_and(|p| p.folder().is_some()))
        })
}

pub(super) fn decorate(workspace: &Workspace, id: PanelId, model: &mut GroupModel) {
    model.active_tab = id;
    model.fixed_grid = workspace.panel(id).is_some_and(Panel::fixed_grid);
    let free = workspace.panel(id).is_some_and(Panel::free_layout);
    if model.free_layout != free { model.scroll = 0; }
    model.free_layout = free;
    let tabs = workspace
        .tab_group(id)
        .map(|group| {
            group
                .members
                .iter()
                .filter_map(|id| workspace.panel(*id).map(|p| (*id, p.title().to_owned())))
                .collect()
        })
        .unwrap_or_default();
    model.tabs = tabs;
}

pub(super) fn select(
    state: &Rc<RefCell<PaneApp>>,
    from: PanelId,
    to: PanelId,
) -> Result<(), String> {
    select_impl(state, from, to, true)
}

/// Reconcile a committed CLI selection without saving the same change again.
pub(super) fn present_selection(state: &Rc<RefCell<PaneApp>>, from: PanelId, to: PanelId) -> Result<(), String> {
    select_impl(state, from, to, false)
}
fn select_impl(state: &Rc<RefCell<PaneApp>>, from: PanelId, to: PanelId, persist: bool) -> Result<(), String> {
    if from == to {
        return Ok(());
    }
    let (hwnd, previous) = {
        let s = state.borrow();
        let group = s.workspace.tab_group(from).ok_or(crate::i18n::text("ui-tab-group-closed"))?;
        if !group.members.contains(&to) {
            return Err(crate::i18n::text("ui-target-tab-does-not-belong-to-this-panel").into());
        }
        let view = s.views.iter().find(|v| v.id == from).ok_or(crate::i18n::text("ui-panel-closed"))?;
        (view.window.hwnd().cast(), s.workspace.clone())
    };
    rename::cancel(hwnd);
    {
        let mut s = state.borrow_mut();
        let mut next = match s.tab_models.get(&to) {
            Some(model) => model.clone(),
            None => create_model(&s, to)?,
        };
        next.folder_columns = folder::saved_columns(&s.store, to)?;
        next.folder_visible_columns = folder::visible_columns(&s.store, to)?;
        folder::ensure(&mut s, to)?;
        if let Some(source) = s.folders.get(&to) {
            source.set_active(false);
        }
        if persist {
            s.workspace.sync_tab_windows();
            let mut groups = s.workspace.tab_groups().to_vec();
            groups
                .iter_mut()
                .find(|g| g.members.contains(&from))
                .unwrap()
                .active = to;
            s.workspace
                .set_tab_groups(groups)
                .map_err(|e| e.to_string())?;
            if let Err(error) = s.store.save_active_tab(from, to) {
                s.workspace = previous;
                return Err(error.to_string());
            }
        }
        s.tab_models.remove(&to);
        let panel = s.workspace.panel(to).unwrap();
        next.title = panel.title().to_owned();
        next.options = s.workspace.pane_options();
        next.theme = panel.theme();
        next.dark = theme::is_dark(panel.theme());
        next.backdrop = panel.backdrop();
        // A tab switch reuses the same window and its transient hover state.
        let live = s.views.iter().find(|v| v.id == from).unwrap().model.borrow();
        next.collapsed = live.collapsed;
        next.reveal = live.reveal;
        drop(live);
        next.locked = panel.locked();
        next.auto_hide = panel.auto_hide();
        next.list_view = panel.list_view();
        next.merge_preview.clear();
        next.merge_occluded = false;
        next.hovered_item = None;
        next.hovered_button = None;
        next.hovered_tab = None;
        next.pressed_button = None;
        next.renaming = None;
        next.scrollbar = Default::default();
        decorate(&s.workspace, to, &mut next);
        hybrid::unregister_drop(&mut s, hwnd);
        let view = s.views.iter_mut().find(|v| v.id == from).unwrap();
        next.focused = view.model.borrow().focused;
        next.native_material = view.model.borrow().native_material;
        let old = std::mem::replace(&mut *view.model.borrow_mut(), next);
        view.id = to;
        view.target.set(to);
        s.tab_models.insert(from, old);
        if let Some(source) = s.folders.get(&from) {
            source.set_active(false);
        }
        if let Some(source) = s.folders.get(&to) {
            source.set_active(true);
        }
        if let Some(runtime) = &mut s.runtime {
            if let Some(position) = runtime.layouts.positions.get(&from).copied() {
                runtime.layouts.positions.insert(to, position);
            }
        }
        refresh_changed_views(&mut s, true);
        s.wake.notify();
    }
    let register = state.borrow().session.is_some()
        || state
            .borrow()
            .workspace
            .panel(to)
            .unwrap()
            .folder()
            .is_some();
    if register {
        hybrid::register_drop(state, to)?;
    }
    unsafe {
        PostMessageW(hwnd, window::TAB_CHANGED, 0, 0);
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
    Ok(())
}

pub(super) fn add(
    state: &Rc<RefCell<PaneApp>>,
    id: PanelId,
    folder: Option<std::path::PathBuf>,
) -> Result<(), String> {
    let next = {
        let mut s = state.borrow_mut();
        let source = s.workspace.panel(id).ok_or(crate::i18n::text("ui-panel-closed"))?.clone();
        if !source.supports_tabs() || folder.is_some() {
            return Err(crate::i18n::text("ui-only-group-panels-support-tabs").into());
        }
        if source.locked() {
            return Err(crate::i18n::text("ui-unlock-the-panel-first").into());
        }
        let next = PanelId::new(
            s.workspace
                .panels()
                .iter()
                .map(|p| p.id().get())
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(crate::i18n::text("ui-tab-limit-exceeded"))?,
        );
        let title = folder
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| crate::i18n::text("ui-new-group").into());
        let mut panel = Panel::new(next, title, source.rect());
        panel.set_folder(folder);
        if panel.folder().is_some() {
            folder::Defaults::load(&s.store)?.apply(&s.store, &mut panel)?;
        }
        let previous = s.workspace.clone();
        s.workspace.add_panel(panel).map_err(|e| e.to_string())?;
        let mut groups = s.workspace.tab_groups().to_vec();
        if let Some(group) = groups.iter_mut().find(|g| g.members.contains(&id)) {
            group.members.push(next);
        } else {
            groups.push(PaneTabs {
                members: vec![id, next],
                active: id,
            });
        }
        s.workspace
            .set_tab_groups(groups)
            .map_err(|e| e.to_string())?;
        if let Err(error) = save(&mut s) {
            s.workspace = previous;
            return Err(error);
        }
        refresh_views(&mut s);
        next
    };
    select(state, id, next)
}

pub(super) fn choose_folder(_state: &Rc<RefCell<PaneApp>>, _id: PanelId) -> Result<(), String> {
    Err(crate::i18n::text("ui-folder-panels-do-not-support-tabs-create-a-separate-panel").into())
}

pub(super) fn close(state: &Rc<RefCell<PaneApp>>, id: PanelId) -> Result<(), String> {
    let group = state.borrow().workspace.tab_group(id).cloned();
    let Some(group) = group else {
        handle(state, id, Event::ClosePane)?;
        return Ok(());
    };
    if state
        .borrow()
        .workspace
        .panel(id)
        .is_some_and(Panel::locked)
    {
        return Err(crate::i18n::text("ui-unlock-the-panel-first").into());
    }
    if group.active == id {
        let at = group
            .members
            .iter()
            .position(|member| *member == id)
            .unwrap();
        let next = if at + 1 < group.members.len() {
            group.members[at + 1]
        } else {
            group.members[at - 1]
        };
        select(state, id, next)?;
    }
    let mut s = state.borrow_mut();
    let previous = s.workspace.clone();
    remove_panel(&mut s.workspace, id);
    if let Err(error) = save(&mut s) {
        s.workspace = previous;
        let _ = hybrid::sync(&mut s);
        return Err(error);
    }
    s.folders.remove(&id);
    s.tab_models.remove(&id);
    refresh_changed_views(&mut s, true);
    Ok(())
}

pub(super) fn reorder(
    state: &Rc<RefCell<PaneApp>>,
    id: PanelId,
    moved: PanelId,
    before: PanelId,
) -> Result<(), String> {
    let mut s = state.borrow_mut();
    if s.workspace.panel(id).is_some_and(Panel::locked) {
        return Err(crate::i18n::text("ui-unlock-the-panel-first").into());
    }
    let previous = s.workspace.clone();
    let mut groups = s.workspace.tab_groups().to_vec();
    let group = groups
        .iter_mut()
        .find(|g| g.members.contains(&id))
        .ok_or(crate::i18n::text("ui-tab-group-closed"))?;
    let from = group
        .members
        .iter()
        .position(|member| *member == moved)
        .ok_or(crate::i18n::text("ui-tab-closed"))?;
    let to = group
        .members
        .iter()
        .position(|member| *member == before)
        .ok_or(crate::i18n::text("ui-tab-closed"))?;
    group.members.remove(from);
    group.members.insert(to, moved);
    s.workspace
        .set_tab_groups(groups)
        .map_err(|e| e.to_string())?;
    if let Err(error) = save(&mut s) {
        s.workspace = previous;
        return Err(error);
    }
    refresh_changed_views(&mut s, true);
    Ok(())
}

/// Split one content ID out without changing its desktop membership or folder source.
pub(super) fn detach(state: &Rc<RefCell<PaneApp>>, id: PanelId) -> Result<(), String> {
    let group = {
        let s = state.borrow();
        if s.workspace.panel(id).is_none_or(Panel::locked) {
            return Err(crate::i18n::text("ui-unlock-the-panel-first").into());
        }
        s.workspace.tab_group(id).cloned()
    };
    let Some(group) = group else {
        return Ok(());
    };
    if group.active == id {
        let next = *group.members.iter().find(|member| **member != id).unwrap();
        select(state, id, next)?;
    }
    let previous = state.borrow().workspace.clone();
    {
        let mut s = state.borrow_mut();
        s.workspace.sync_tab_windows();
        let groups = s
            .workspace
            .tab_groups()
            .iter()
            .cloned()
            .filter_map(|mut g| {
                g.members.retain(|member| *member != id);
                (g.members.len() > 1).then_some(g)
            })
            .collect();
        s.workspace
            .set_tab_groups(groups)
            .map_err(|e| e.to_string())?;
        let panel = s.workspace.panel_mut(id).unwrap();
        let mut rect = panel.rect();
        rect.x += 32.0;
        rect.y += 32.0;
        panel.set_rect(rect);
    }
    let previous_position = state
        .borrow_mut()
        .runtime
        .as_mut()
        .and_then(|runtime| runtime.layouts.positions.remove(&id));
    let result = create_view(state, id).and_then(|()| {
        let mut s = state.borrow_mut();
        let workspace = s.workspace.clone();
        s.store
            .save_workspace(&workspace)
            .map_err(|e| e.to_string())
    });
    if let Err(error) = result {
        let view = {
            let mut s = state.borrow_mut();
            s.workspace = previous;
            if let (Some(runtime), Some(position)) = (&mut s.runtime, previous_position) {
                runtime.layouts.positions.insert(id, position);
            }
            if let Some(source) = s.folders.get(&id) {
                source.set_active(false);
            }
            take_view(&mut s, id)
        };
        drop(view);
        return Err(error);
    }
    let mut s = state.borrow_mut();
    restore_cached_model(&mut s, id);
    if let Some(source) = s.folders.get(&id) {
        source.set_active(true);
    }
    refresh_changed_views(&mut s, true);
    s.wake.notify();
    Ok(())
}

pub(super) fn restore_cached_model(s: &mut PaneApp, id: PanelId) {
    if let Some(mut cached) = s.tab_models.remove(&id) {
        let view = s.views.iter().find(|v| v.id == id).unwrap();
        let fresh = view.model.borrow();
        cached.title = fresh.title.clone();
        if (cached.free_layout && !cached.is_list()) != (fresh.free_layout && !fresh.is_list()) { cached.scroll = 0; }
        cached.list_view = fresh.list_view;
        cached.fixed_grid = fresh.fixed_grid;
        cached.free_layout = fresh.free_layout;
        cached.theme = fresh.theme;
        cached.dark = fresh.dark;
        cached.backdrop = fresh.backdrop;
        cached.collapsed = fresh.collapsed;
        cached.locked = fresh.locked;
        cached.auto_hide = fresh.auto_hide;
        cached.reveal = fresh.reveal;
        cached.options = fresh.options;
        cached.native_material = fresh.native_material;
        drop(fresh);
        cached.focused = view.model.borrow().focused;
        cached.merge_preview.clear();
        cached.merge_occluded = false;
        cached.renaming = None;
        cached.hovered_item = None;
        cached.hovered_button = None;
        cached.hovered_tab = None;
        cached.pressed_button = None;
        *view.model.borrow_mut() = cached;
    }
    if let Some(view) = s.views.iter().find(|v|v.id==id) { decorate(&s.workspace,id,&mut view.model.borrow_mut()); }
}

fn take_view(s: &mut PaneApp, id: PanelId) -> Option<View> {
    let at = s.views.iter().position(|v| v.id == id)?;
    let view = s.views.remove(at);
    let hwnd = view.window.hwnd().cast();
    window::prepare_close(hwnd);
    hybrid::unregister_drop(s, hwnd);
    Some(view)
}

fn merge(state: &Rc<RefCell<PaneApp>>, from: PanelId, to: PanelId) -> Result<(), String> {
    let view = {
        let mut s = state.borrow_mut();
        for id in [from, to] {
            let panel = s.workspace.panel(id).ok_or(crate::i18n::text("ui-panel-closed"))?;
            if panel.locked() || !panel.supports_tabs() {
                return Err(crate::i18n::text("ui-select-an-unlocked-group-panel").into());
            }
            if !s.views.iter().any(|v| v.id == id) {
                return Err(crate::i18n::text("ui-panel-closed-or-active-tab-changed").into());
            }
        }
        if from == to {
            return Ok(());
        }
        let previous = s.workspace.clone();
        let source = s
            .workspace
            .tab_group(from)
            .map_or_else(|| vec![from], |g| g.members.clone());
        let mut members = s
            .workspace
            .tab_group(to)
            .map_or_else(|| vec![to], |g| g.members.clone());
        if members.contains(&from) {
            return Ok(());
        }
        members.extend(source.iter().copied());
        let mut groups = s.workspace.tab_groups().to_vec();
        groups.retain(|g| !g.members.contains(&from) && !g.members.contains(&to));
        groups.push(PaneTabs {
            members,
            active: to,
        });
        s.workspace
            .set_tab_groups(groups)
            .map_err(|e| e.to_string())?;
        let workspace = s.workspace.clone();
        if let Err(error) = s.store.save_workspace(&workspace) {
            s.workspace = previous;
            return Err(error.to_string());
        }
        let view = take_view(&mut s, from).unwrap();
        s.tab_models.insert(from, view.model.borrow().clone());
        for id in source {
            if let Some(source) = s.folders.get(&id) {
                source.set_active(false);
            }
        }
        refresh_changed_views(&mut s, true);
        s.wake.notify();
        view
    };
    drop(view);
    Ok(())
}

fn header_contains(bounds: RECT, dpi: u32, point: windows_sys::Win32::Foundation::POINT) -> bool {
    let height = (layout::HEADER * dpi.max(96) as f32 / 96.0).round() as i32;
    point.x >= bounds.left
        && point.x < bounds.right
        && point.y >= bounds.top
        && point.y < (bounds.top + height).min(bounds.bottom)
}

fn merge_target(s: &PaneApp, from: PanelId) -> Option<PanelId> {
    if s.workspace
        .panel(from)
        .is_none_or(|p| p.locked() || !p.supports_tabs())
    {
        return None;
    }
    let mut pointer = windows_sys::Win32::Foundation::POINT::default();
    if unsafe { GetCursorPos(&raw mut pointer) } == 0 {
        return None;
    }
    let retained = MERGE_HOVER.with(|hover| {
        let hover = hover.borrow();
        hover
            .candidate
            .filter(|(source, _, _)| *source == from && hover.ready)
            .map(|(_, to, _)| to)
    });
    if let Some(view) = s.views.iter().find(|v| Some(v.id) == retained) {
        let hwnd = view.window.hwnd().cast();
        let mut bounds = RECT::default();
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        let margin = (6.0 * dpi as f32 / 96.0).round() as i32;
        if s.workspace
            .panel(view.id)
            .is_some_and(|p| !p.locked() && p.supports_tabs())
            && unsafe { IsWindowVisible(hwnd) != 0 && GetWindowRect(hwnd, &raw mut bounds) != 0 }
        {
            let bottom = bounds.top + (layout::HEADER * dpi as f32 / 96.0).round() as i32;
            if pointer.x >= bounds.left - margin
                && pointer.x < bounds.right + margin
                && pointer.y >= bounds.top - margin
                && pointer.y < bottom + margin
            {
                return Some(view.id);
            }
        }
    }
    // Follow native stacking order, ignoring only the pane being dragged.
    let mut hwnd = unsafe { GetTopWindow(std::ptr::null_mut()) };
    while !hwnd.is_null() {
        if let Some(view) = s
            .views
            .iter()
            .find(|v| v.id != from && v.window.hwnd().cast() == hwnd)
        {
            let mut bounds = RECT::default();
            if unsafe {
                IsWindowVisible(hwnd) != 0
                    && IsIconic(hwnd) == 0
                    && GetWindowRect(hwnd, &raw mut bounds) != 0
            } && pointer.x >= bounds.left
                && pointer.x < bounds.right
                && pointer.y >= bounds.top
                && pointer.y < bounds.bottom
            {
                return (s
                    .workspace
                    .panel(view.id)
                    .is_some_and(|p| !p.locked() && p.supports_tabs())
                    && header_contains(bounds, unsafe { GetDpiForWindow(hwnd) }, pointer))
                .then_some(view.id);
            }
        }
        hwnd = unsafe { GetWindow(hwnd, GW_HWNDNEXT) };
    }
    None
}

#[derive(Default)]
struct MergeHover {
    candidate: Option<(PanelId, PanelId, std::time::Instant)>,
    ready: bool,
}
impl MergeHover {
    fn update(
        &mut self,
        from: PanelId,
        target: Option<PanelId>,
        now: std::time::Instant,
    ) -> Option<PanelId> {
        if self.candidate.map(|(source, to, _)| (source, to)) != target.map(|to| (from, to)) {
            self.candidate = target.map(|to| (from, to, now));
            self.ready = false;
        }
        if let Some((_, to, since)) = self.candidate {
            self.ready |= now.duration_since(since) >= std::time::Duration::from_millis(150);
            return self.ready.then_some(to);
        }
        None
    }
}
thread_local! { static MERGE_HOVER: RefCell<MergeHover> = RefCell::new(MergeHover::default()); }

fn highlight_merge(s: &PaneApp, source: PanelId, target: Option<PanelId>) {
    let incoming: Vec<_> = s
        .workspace
        .tab_group(source)
        .map_or_else(|| vec![source], |g| g.members.clone())
        .into_iter()
        .filter_map(|id| s.workspace.panel(id).map(|p| (id, p.title().to_owned())))
        .collect();
    for view in &s.views {
        let preview = if target == Some(view.id) {
            incoming.clone()
        } else {
            Vec::new()
        };
        let mut model = view.model.borrow_mut();
        model.merge_occluded = target.is_some() && view.id == source;
        if model.merge_preview != preview {
            model.merge_preview = preview;
            unsafe {
                InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
            }
        }
    }
}

pub(super) fn preview_merge(s: &PaneApp, from: PanelId) -> bool {
    // Mouse-up is committed by WM_EXITSIZEMOVE; a timer between the two must not erase the preview.
    if unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x01) } >= 0 {
        return MERGE_HOVER.with(|hover| hover.borrow().ready);
    }
    let candidate = merge_target(s, from);
    let target = MERGE_HOVER.with(|hover| {
        hover
            .borrow_mut()
            .update(from, candidate, std::time::Instant::now())
    });
    highlight_merge(s, from, target);
    candidate.is_some()
}

pub(super) fn finish_move(
    state: &Rc<RefCell<PaneApp>>,
    from: PanelId,
    commit: bool,
) -> Result<(), String> {
    let (owner, target) = {
        let s = state.borrow();
        let hit = merge_target(&s, from);
        let target = MERGE_HOVER.with(|hover| {
            let mut hover = hover.borrow_mut();
            let target = hover
                .candidate
                .filter(|(source, _, _)| *source == from && hover.ready)
                .map(|(_, target, _)| target)
                .filter(|target| commit && hit == Some(*target));
            *hover = MergeHover::default();
            target
        });
        highlight_merge(&s, from, None);
        if target.is_some() {
            if let Some(view) = s.views.iter().find(|v| v.id == from) {
                view.model.borrow_mut().merge_occluded = true;
            }
        }
        let owner = s
            .views
            .iter()
            .find(|v| v.id == from)
            .ok_or(crate::i18n::text("ui-panel-closed"))?
            .window
            .hwnd() as isize;
        (owner, target)
    };
    let Some(to) = target else {
        return Ok(());
    };
    let weak = Rc::downgrade(state);
    // Close the source only after the native move callback and geometry save return.
    if !window::post_action(owner as _, move || {
        if let Some(state) = weak.upgrade() {
            if let Err(error) = merge(&state, from, to) {
                unsafe {
                    PostMessageW(owner as _, window::RESTORE_MERGE, 0, 0);
                }
                window::error(&error);
            }
        }
    }) {
        unsafe {
            PostMessageW(owner as _, window::RESTORE_MERGE, 0, 0);
        }
        return Err(crate::i18n::text("ui-could-not-merge-panels").into());
    }
    Ok(())
}

/// Drawing-only projection: never alters the real tabs, hit targets or active content.
pub(super) fn merge_strip(model: &GroupModel, width: f32) -> Vec<(String, bool, bool, RectDip)> {
    if model.merge_preview.is_empty() {
        return Vec::new();
    }
    let existing = if model.tabs.is_empty() {
        vec![(model.active_tab, model.title.clone())]
    } else {
        model.tabs.clone()
    };
    let inset = layout::HEADER_INSET;
    let available = (width - 2.0 * inset).max(1.0);
    let capacity = ((available / 72.0).floor() as usize).max(2);
    let incoming_count = model.merge_preview.len().min(capacity - existing.len().min((capacity / 2).max(1)));
    let existing_count = existing.len().min(capacity - incoming_count);
    let active = existing
        .iter()
        .position(|(id, _)| *id == model.active_tab)
        .unwrap_or(0);
    let start = active
        .saturating_sub(existing_count / 2)
        .min(existing.len() - existing_count);
    let mut labels: Vec<_> = existing
        .iter()
        .skip(start)
        .take(existing_count)
        .map(|(id, title)| (title.clone(), false, *id == model.active_tab))
        .collect();
    for (index, (_, title)) in model.merge_preview.iter().take(incoming_count).enumerate() {
        let remaining = model.merge_preview.len() - index;
        let text = if index + 1 == incoming_count && remaining > 1 {
            crate::i18n::format("ui-tabs-a81a", &[("remaining", format!("{}", remaining))])
        } else {
            format!("＋ {title}")
        };
        labels.push((text, true, false));
    }
    let gap = 6.0;
    let size = ((available - gap * (labels.len() - 1) as f32) / labels.len() as f32).max(1.0);
    labels
        .into_iter()
        .enumerate()
        .map(|(index, (title, incoming, active))| {
            (
                title,
                incoming,
                active,
                RectDip {
                    x: inset + index as f32 * (size + gap),
                    y: inset,
                    width: size,
                    height: layout::HEADER - 2.0 * inset,
                },
            )
        })
        .collect()
}

/// Shared geometry for painting and pointer input; keep the active tab visible.
pub(super) fn strip(model: &GroupModel, width: f32) -> Vec<(PanelId, RectDip)> {
    if model.tabs.len() < 2 {
        return Vec::new();
    }
    let right_buttons = if model.folder.is_some() { 70.0 } else { 0.0 };
    // Use one inset for the top, left and right edges.
    let inset = layout::HEADER_INSET;
    let available = (width - right_buttons - inset * 2.0).max(72.0);
    let count = ((available / 72.0).floor() as usize)
        .max(1)
        .min(model.tabs.len());
    let active = model
        .tabs
        .iter()
        .position(|(id, _)| *id == model.active_tab)
        .unwrap_or(0);
    let start = active
        .saturating_sub(count / 2)
        .min(model.tabs.len() - count);
    let gap = 6.0;
    let size = (available - gap * (count - 1) as f32) / count as f32;
    model
        .tabs
        .iter()
        .skip(start)
        .take(count)
        .enumerate()
        .map(|(at, (id, _))| {
            (
                *id,
                RectDip {
                    x: inset + at as f32 * (size + gap),
                    y: inset,
                    width: size,
                    height: layout::HEADER - inset * 2.0,
                },
            )
        })
        .collect()
}

pub(super) fn hit(model: &GroupModel, width: f32, x: f32, y: f32) -> Option<PanelId> {
    strip(model, width)
        .into_iter()
        .find(|(_, r)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
        .map(|(hit, _)| hit)
}

pub(super) fn adjacent(model: &GroupModel, step: i8) -> Option<PanelId> {
    let at = model
        .tabs
        .iter()
        .position(|(id, _)| *id == model.active_tab)?;
    let next = (at as isize + step as isize).rem_euclid(model.tabs.len() as isize) as usize;
    Some(model.tabs[next].0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_share_one_window_keep_selection_and_release_only_closed_membership() {
        let _apartment = crate::pane::test_support::apartment();
        let state = Rc::new(RefCell::new(super::super::tests::test_state()));
        state
            .borrow_mut()
            .workspace
            .set_appearance(luciddesk_core::PanelTheme::Dark, luciddesk_core::Backdrop::Mica);
        let first = PanelId::new(1);
        create_view(&state, first).unwrap();
        let hwnd = state.borrow().views[0].window.hwnd();
        state.borrow().views[0]
            .model
            .borrow_mut()
            .select_item(1, false, false);
        let original = state.borrow().workspace.desktop_items().to_vec();
        add(&state, first, None).unwrap();
        let second = state.borrow().views[0].id;
        assert_ne!(first, second);
        assert_eq!(state.borrow().views.len(), 1);
        assert_eq!(state.borrow().views[0].window.hwnd(), hwnd);
        assert_eq!(state.borrow().workspace.desktop_items(), original);
        assert!(state.borrow().views[0].model.borrow().items.is_empty());
        handle(&state, second, Event::SetTitle("资料".into())).unwrap();
        select(&state, second, first).unwrap();
        assert_eq!(state.borrow().views[0].model.borrow().selected, Some(1));
        assert_eq!(state.borrow().views[0].model.borrow().tabs[1].1, "资料");
        add(&state, first, None).unwrap();
        let third = state.borrow().views[0].id;
        select(&state, third, first).unwrap();
        handle(&state, first, Event::CloseTabId(third)).unwrap();
        assert_eq!(state.borrow().views[0].id, first);
        assert!(state.borrow().workspace.panel(third).is_none());
        handle(&state, first, Event::MoveTabId(second, -1)).unwrap();
        assert_eq!(state.borrow().views[0].id, first);
        assert_eq!(
            state.borrow().workspace.tab_group(first).unwrap().members[0],
            second
        );
        detach(&state, second).unwrap();
        assert_eq!(state.borrow().views.len(), 2);
        let detached = state.borrow().views.iter().find(|v| v.id == second).unwrap().window.hwnd().cast();
        let above = |a, b| unsafe {
            let mut current = GetWindow(b, GW_HWNDPREV);
            while !current.is_null() {
                if current == a { return true; }
                current = GetWindow(current, GW_HWNDPREV);
            }
            false
        };
        assert!(above(detached, hwnd.cast()));
        unsafe { SendMessageW(hwnd.cast(), WM_MOUSEACTIVATE, 0, HTCLIENT as isize); }
        assert!(above(hwnd.cast(), detached));
        unsafe { SendMessageW(detached, WM_MOUSEACTIVATE, 0, HTCLIENT as isize); }
        assert!(above(detached, hwnd.cast()));
        assert!(state.borrow().workspace.tab_groups().is_empty());
        assert_eq!(state.borrow().workspace.desktop_items(), original);
        merge(&state, second, first).unwrap();
        assert_eq!(state.borrow().views.len(), 1);
        assert_eq!(state.borrow().views[0].window.hwnd(), hwnd);
        assert_eq!(state.borrow().views[0].model.borrow().selected, Some(1));
        detach(&state, first).unwrap();
        assert_eq!(state.borrow().views.len(), 2);
        assert_eq!(
            state
                .borrow()
                .views
                .iter()
                .find(|v| v.id == first)
                .unwrap()
                .model
                .borrow()
                .selected,
            Some(1)
        );
        merge(&state, first, second).unwrap();
        select(&state, second, first).unwrap();
        assert_eq!(
            state.borrow().store.load_workspace().unwrap(),
            state.borrow().workspace
        );
        close(&state, first).unwrap();
        assert_eq!(state.borrow().views.len(), 1);
        assert_eq!(state.borrow().views[0].id, second);
        assert!(state.borrow().workspace.tab_groups().is_empty());
        assert!(state.borrow().workspace.desktop_items().iter().all(|item| !matches!(item.placement(), DesktopPlacement::Pane { pane_id, .. } if *pane_id == first)));
        assert_eq!(
            state.borrow().store.load_workspace().unwrap(),
            state.borrow().workspace
        );
        window::prepare_close(state.borrow().views[0].window.hwnd().cast());
        drop(state);
        // Exercise both sources in one UI apartment, as the application does.
        folder_panels_reject_tab_creation_and_merging();
        merge_whole_groups_and_reject_locked_panes();
    }

    fn merge_whole_groups_and_reject_locked_panes() {
        let state = Rc::new(RefCell::new(super::super::tests::test_state()));
        state
            .borrow_mut()
            .workspace
            .set_appearance(luciddesk_core::PanelTheme::Dark, luciddesk_core::Backdrop::Mica);
        let first = PanelId::new(1);
        create_view(&state, first).unwrap();
        add(&state, first, None).unwrap();
        let second = state.borrow().views[0].id;
        detach(&state, second).unwrap();
        add(&state, first, None).unwrap();
        let left = state.borrow().workspace.tab_group(first).unwrap().clone();
        add(&state, second, None).unwrap();
        let right = state.borrow().workspace.tab_group(second).unwrap().clone();
        state
            .borrow_mut()
            .workspace
            .panel_mut(right.active)
            .unwrap()
            .set_locked(true);
        let before = state.borrow().workspace.clone();
        highlight_merge(&state.borrow(), left.active, Some(right.active));
        assert!(
            state
                .borrow()
                .views
                .iter()
                .find(|v| v.id == right.active)
                .unwrap()
                .model
                .borrow()
                .merge_preview
                .is_empty()
                == false
        );
        finish_move(&state, left.active, false).unwrap();
        assert!(
            state
                .borrow()
                .views
                .iter()
                .all(|v| v.model.borrow().merge_preview.is_empty())
        );
        assert!(merge(&state, left.active, right.active).is_err());
        assert!(detach(&state, right.active).is_err());
        assert_eq!(state.borrow().workspace, before);
        assert_eq!(state.borrow().views.len(), 2);
        state
            .borrow_mut()
            .workspace
            .panel_mut(right.active)
            .unwrap()
            .set_locked(false);
        merge(&state, left.active, right.active).unwrap();
        let mut expected = right.members;
        expected.extend(left.members);
        assert_eq!(
            state.borrow().workspace.tab_group(first).unwrap().members,
            expected
        );
        assert_eq!(state.borrow().views.len(), 1);
        assert_eq!(state.borrow().views[0].id, right.active);
        detach(&state, first).unwrap();
        assert_eq!(
            state
                .borrow()
                .workspace
                .tab_group(second)
                .unwrap()
                .members
                .len(),
            3
        );
        assert_eq!(state.borrow().views.len(), 2);
        assert_eq!(
            state.borrow().store.load_workspace().unwrap(),
            state.borrow().workspace
        );
        handle(&state, first, Event::ClosePane).unwrap();
        handle(&state, right.active, Event::ClosePane).unwrap();
    }

    fn folder_panels_reject_tab_creation_and_merging() {
        let root = tempfile::tempdir().unwrap();
        let state = Rc::new(RefCell::new(super::super::tests::test_state()));
        let first = PanelId::new(1);
        let folder = PanelId::new(90);
        let mut panel = Panel::new(folder, "Folder", RectDip::default());
        panel.set_folder(Some(root.path().to_path_buf()));
        state.borrow_mut().workspace.add_panel(panel).unwrap();
        state.borrow_mut().workspace.set_appearance(luciddesk_core::PanelTheme::Dark, luciddesk_core::Backdrop::Mica);
        create_view(&state, first).unwrap();
        create_view(&state, folder).unwrap();
        let before = state.borrow().workspace.clone();
        assert!(add(&state, first, Some(root.path().to_path_buf())).is_err());
        assert!(add(&state, folder, None).is_err());
        assert!(merge(&state, folder, first).is_err());
        assert!(merge(&state, first, folder).is_err());
        assert_eq!(merge_target(&state.borrow(), folder), None);
        assert_eq!(state.borrow().workspace, before);
        handle(&state, folder, Event::ClosePane).unwrap();
        handle(&state, first, Event::ClosePane).unwrap();
    }

    #[test]
    fn merge_preview_waits_and_resets_without_mutating_real_tabs() {
        let now = std::time::Instant::now();
        let from = PanelId::new(1);
        let to = PanelId::new(2);
        let mut hover = MergeHover::default();
        assert_eq!(hover.update(from, Some(to), now), None);
        assert_eq!(
            hover.update(from, Some(to), now + std::time::Duration::from_millis(149)),
            None
        );
        assert_eq!(
            hover.update(from, Some(to), now + std::time::Duration::from_millis(150)),
            Some(to)
        );
        assert_eq!(
            hover.update(from, None, now + std::time::Duration::from_millis(200)),
            None
        );
        assert!(!hover.ready);
        assert_eq!(
            hover.update(from, Some(to), now + std::time::Duration::from_millis(201)),
            None
        );
        let state = super::super::tests::test_state();
        let mut model = create_model(&state, from).unwrap();
        model.tabs = vec![(from, "当前".into()), (to, "另一个".into())];
        model.active_tab = to;
        model.merge_preview = (3..=12)
            .map(|id| (PanelId::new(id), format!("加入 {id}")))
            .collect();
        let original = model.tabs.clone();
        for width in [260.0, 420.0, 1200.0] {
            let strip = merge_strip(&model, width);
            assert!(
                strip
                    .iter()
                    .any(|(_, incoming, active, _)| !incoming && *active)
            );
            assert!(
                strip
                    .iter()
                    .any(|(text, incoming, _, _)| *incoming && text.starts_with('＋'))
            );
            let mut previous_right = 0.0;
            for (_, _, _, rect) in strip {
                assert!(rect.x >= previous_right && rect.x + rect.width <= width);
                previous_right = rect.x + rect.width;
            }
        }
        assert_eq!(model.tabs, original);
        assert_eq!(model.active_tab, to);
        model.merge_preview.clear();
        assert!(merge_strip(&model, 420.0).is_empty());
    }

    #[test]
    fn merge_header_hit_respects_dpi_and_excludes_content() {
        use windows_sys::Win32::Foundation::POINT;
        for dpi in [96, 144, 192] {
            let bounds = RECT {
                left: -500,
                top: 100,
                right: -100,
                bottom: 600,
            };
            let bottom = 100 + (layout::HEADER * dpi as f32 / 96.0).round() as i32;
            assert!(header_contains(bounds, dpi, POINT { x: -500, y: 100 }));
            assert!(header_contains(
                bounds,
                dpi,
                POINT {
                    x: -101,
                    y: bottom - 1
                }
            ));
            for point in [
                POINT { x: -501, y: 110 },
                POINT { x: -100, y: 110 },
                POINT { x: -300, y: 99 },
                POINT { x: -300, y: bottom },
                POINT { x: -300, y: 500 },
            ] {
                assert!(!header_contains(bounds, dpi, point));
            }
        }
    }

    #[test]
    fn tab_hit_geometry_never_overlaps_content_and_active_tab_survives_overflow() {
        let mut state = super::super::tests::test_state();
        state
            .workspace
            .set_appearance(luciddesk_core::PanelTheme::Dark, luciddesk_core::Backdrop::Mica);
        let mut model = create_model(&state, PanelId::new(1)).unwrap();
        model.tabs = (1..=12)
            .map(|id| (PanelId::new(id), format!("标签 {id}")))
            .collect();
        model.active_tab = PanelId::new(10);
        for folder in [None, Some(std::path::PathBuf::from("C:/"))] {
            model.folder = folder;
            for collapsed in [false, true] {
                model.collapsed = collapsed;
                for width in [260.0, 420.0, 800.0] {
                    let strip = strip(&model, width);
                    assert!(strip.iter().any(|(hit, _)| *hit == model.active_tab));
                    for (expected, rect) in strip {
                        let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
                        assert_eq!(hit(&model, width, x, y), Some(expected));
                        assert!(rect.y >= 0.0 && rect.y + rect.height <= layout::HEADER);
                        assert!(model.header_button(width, x, y).is_none());
                        assert!(model.grid(width, 360.0).hit(x, y, 0, 3).is_none());
                        assert!(rect.x >= 0.0 && rect.x + rect.width <= width);
                    }
                }
            }
        }
    }
}
