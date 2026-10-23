//! Group commands and their persisted state transitions.
use super::search::everything_settings;
use super::*;
mod appearance;
mod lifecycle;
mod panel;
mod shell;
mod tab_events;
pub(super) use appearance::{commit_grid, commit_material, commit_radius};
pub(super) use appearance::{preview_grid, preview_material, preview_radius};
pub(super) use shell::activate_item_with;

pub(super) fn handle(
    state: &Rc<RefCell<PaneApp>>,
    id: PanelId,
    event: Event,
) -> Result<bool, String> {
    if !state.borrow().monitor_child {
        let options = match &event {
            Event::EnableMonitor => Some(monitor::Preferences { enabled: true, ..monitor::Preferences::load()? }),
            Event::MonitorOptions(options) => Some(options.clone()),
            _ => None,
        };
        if let Some(options) = options {
            options.save()?;
            monitor::process::configure(state)?;
            return Ok(false);
        }
    }
    if let Event::MonitorOptions(options) = &event {
        let s = state.borrow();
        if !s.workspace.panel(id).is_some_and(Panel::is_monitor) {
            return Ok(false);
        }
        options.save()?;
        let has_view = s.views.iter().any(|view| view.id == id);
        if !options.enabled || !has_view {
            drop(s);
            if options.enabled {
                create_view(state, id)?;
            } else {
                let removed = {
                    let mut s = state.borrow_mut();
                    s.views
                        .iter()
                        .position(|v| v.id == id)
                        .map(|at| s.views.remove(at))
                };
                if let Some(view) = &removed {
                    window::prepare_close(view.window.hwnd().cast());
                }
                drop(removed);
            }
            return Ok(false);
        }
        let mut resize = None;
        if let Some(view) = s.views.iter().find(|v| v.id == id) {
            if !view.model.borrow().collapsed
                && view
                    .model
                    .borrow()
                    .monitor
                    .as_ref()
                    .is_some_and(|old| old.compact != options.compact)
            {
                resize = Some(view.window.hwnd() as isize);
            }
            view.model.borrow_mut().monitor = Some(options.clone());
            unsafe {
                InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
            }
        }
        drop(s);
        if let Some(hwnd) = resize {
            let weak = Rc::downgrade(state);
            let compact = options.compact;
            window::post_action(hwnd as _, move || {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                if let Some(rect) = monitor::resize_layout(hwnd as _, compact) {
                    if let Err(error) = handle(&state, id, Event::Geometry(rect)) {
                        window::error_for(hwnd as _, &error);
                    }
                }
            });
        }
        return Ok(false);
    }
    if matches!(event, Event::ResetPaneOptions) {
        let mut s = state.borrow_mut();
        s.store
            .save_metadata_preferences(&[
                ("pane_title_emoji_color", "true"),
                ("pane_compact_menu", "true"),
                ("pane_header_divider", "true"),
                ("pane_icon_only", "false"),
                ("pane_icon_size", "large"),
                ("pane_system_corners", "round"),
                ("pane_snap_gap", "5"),
                ("desktop_panel_layout", "compact"),
            ])
            .map_err(|e| e.to_string())?;
        interface_flags::TITLE_EMOJI.load(&s.store)?;
        interface_flags::COMPACT_MENU.load(&s.store)?;
        interface_flags::HEADER_DIVIDER.load(&s.store)?;
        interface_flags::ICON_ONLY.load(&s.store)?;
        interface_flags::load_icon_size(&s.store)?;
        interface_flags::PANEL_SHADOW.load(&s.store)?;
        interface_flags::load_system_corners(&s.store)?;
        snap::load_gap(&s.store)?;
        s.workspace.set_icon_defaults(luciddesk_core::IconDisplay::DEFAULT);
        for view in &s.views {
            unsafe {
                InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
            }
        }
    }
    if let Event::SetTitleEmojiColor(color) = event {
        let s = state.borrow();
        interface_flags::TITLE_EMOJI.save(&s.store, color)?;
        for view in &s.views {
            unsafe {
                InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
            }
        }
        return Ok(false);
    }
    if matches!(event, Event::ToggleCompactMenu) {
        interface_flags::COMPACT_MENU
            .save(&state.borrow().store, !interface_flags::COMPACT_MENU.get())?;
        return Ok(false);
    }
    if let Event::SetPanelIconDisplay(target, display) = event {
        let mut s = state.borrow_mut();
        let panel = s.workspace.panel_mut(target).ok_or("panel does not exist")?;
        let previous = panel.icon_display();
        panel.set_icon_display(display);
        // Changing artwork does not change Explorer membership or item placement.
        if let Err(error) = s.store.save_preference(
            &format!("panel_icon_display:{}", target.get()),
            &format!("{}|{}", display.icon_only, display.size.name()),
        ) {
            s.workspace.panel_mut(target).unwrap().set_icon_display(previous);
            return Err(error.to_string());
        }
        for view in &s.views {
            let mut model = view.model.borrow_mut();
            if model.active_tab != target { continue; }
            model.icon_display = display;
            model.scroll = 0;
            model.scroll_x = 0.0;
            model.hovered_item = None;
            unsafe {
                PostMessageW(view.window.hwnd().cast(), window::SYNC_POINTER, 0, 0);
                InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
            }
        }
        return Ok(false);
    }
    if matches!(event, Event::ToggleIconOnly) {
        let mut s = state.borrow_mut();
        interface_flags::ICON_ONLY.save(&s.store, !interface_flags::ICON_ONLY.get())?;
        let mut defaults = s.workspace.icon_defaults();
        defaults.icon_only = interface_flags::ICON_ONLY.get();
        s.workspace.set_icon_defaults(defaults);
        return Ok(false);
    }
    if matches!(event, Event::ToggleHeaderDivider) {
        let s = state.borrow();
        interface_flags::HEADER_DIVIDER.save(&s.store, !interface_flags::HEADER_DIVIDER.get())?;
        for view in &s.views {
            unsafe {
                InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
            }
        }
        return Ok(false);
    }
    if let Event::SetSystemRendering(enabled) = event {
        let s = state.borrow();
        interface_flags::PANEL_SHADOW.save(&s.store, enabled)?;
        for view in &s.views {
            unsafe {
                InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
            }
        }
        return Ok(false);
    }
    if let Event::SetIconSize(size) = event {
        let mut s = state.borrow_mut();
        interface_flags::save_icon_size(&s.store, size)?;
        let mut defaults = s.workspace.icon_defaults();
        defaults.size = size;
        s.workspace.set_icon_defaults(defaults);
        return Ok(false);
    }
    if let Event::SetSystemCorners(corners) = event {
        let s = state.borrow();
        interface_flags::save_system_corners(&s.store, corners)?;
        for view in &s.views {
            unsafe {
                InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
            }
        }
        return Ok(false);
    }
    if let Some(result) = tab_events::route(state, id, &event)? {
        return Ok(result);
    }
    if let Event::SortPaneColumn(target, column) = event {
        sorting::request(&mut state.borrow_mut(), target, column)?;
        return Ok(false);
    }
    if let Event::SortFolder(column) = event {
        folder::sort(&mut state.borrow_mut(), id, column)?;
        return Ok(false);
    }
    if let Event::FolderItemCreated(path) = event {
        folder::item_created(&mut state.borrow_mut(), id, path);
        return Ok(false);
    }
    if let Event::SetFolderColumns(widths) = event {
        folder::save_columns(&state.borrow(), id, widths)?;
        return Ok(false);
    }
    if let Event::ToggleFolderColumn(column) = event {
        folder::toggle_column(&state.borrow(), id, column)?;
        return Ok(false);
    }
    if matches!(event, Event::FolderBack) {
        folder::navigate(&mut state.borrow_mut(), id, None)?;
        return Ok(false);
    }
    if matches!(event, Event::FolderHome) {
        folder::home(&mut state.borrow_mut(), id)?;
        return Ok(false);
    }
    if let Event::Activate(index) = event {
        activate_item_with(state, id, index, |owner, identity| {
            open_shell_identity(owner, identity).map_err(|error| error.to_string())
        })?;
        return Ok(false);
    }
    if matches!(
        event,
        Event::ExportBackup
            | Event::CreateBackup
            | Event::RestoreBackupPath(_)
            | Event::ExportBackupPath(_)
            | Event::DeleteBackup(_)
            | Event::RestoreBackup
            | Event::OpenBackups
            | Event::OpenConfigDirectory
            | Event::ReloadConfig
    ) {
        recovery::request(state, &event);
        return Ok(false);
    }
    if matches!(event, Event::RetryDesktop) {
        runtime::maintain(state, true)?;
        if state.borrow().session.is_none() {
            return Err(runtime::status(&state.borrow()));
        }
        return Ok(false);
    }
    if matches!(event, Event::New)
        && state.borrow().runtime.is_some()
        && state.borrow().session.is_none()
    {
        return Err(runtime::status(&state.borrow()));
    }
    if matches!(event, Event::ToggleListView) {
        let mut s = state.borrow_mut();
        let old = s.workspace.clone();
        let panel = s
            .workspace
            .panel_mut(id)
            .ok_or(crate::i18n::text("ui-panel-closed"))?;
        if panel.is_search() || panel.is_monitor() {
            return Ok(false);
        }
        let enabled = !panel.list_view();
        panel.set_list_view(enabled);
        if let Err(error) = save(&mut s) {
            s.workspace = old;
            return Err(error);
        }
        if let Some(view) = s.views.iter().find(|v| v.id == id) {
            let mut model = view.model.borrow_mut();
            model.list_view = enabled;
            model.scroll = 0;
            model.hovered_item = None;
        }
        refresh_changed_views(&mut s, true);
        return Ok(false);
    }
    if matches!(
        event,
        Event::FileDrag(_)
            | Event::NewFolder
            | Event::ChangeFolder
            | Event::SetFolder(_)
            | Event::OpenFolder
            | Event::Peek
            | Event::FileCommand(_)
            | Event::ActivateSelection
    ) {
        return shell::route(state, id, event);
    }
    if matches!(event, Event::RenameTitle | Event::SetTitle(_))
        && state
            .borrow()
            .workspace
            .panel(id)
            .is_some_and(|panel| panel.locked())
    {
        return Ok(false);
    }
    if matches!(
        event,
        Event::SetCornerRadius(_)
            | Event::SetIconGrid(_)
            | Event::SetPanelText(_)
            | Event::ToggleTextProtection
            | Event::ToggleBorder
            | Event::ToggleSnap
            | Event::ResetPaneOptions
    ) {
        return appearance::options(state, event);
    }
    if matches!(event, Event::Settings) {
        if state.borrow().workspace.panel(id).is_some_and(Panel::is_monitor) {
            settings::show_monitor(state)?;
            return Ok(false);
        }
        settings::show(state, id)?;
        return Ok(false);
    }
    if matches!(event, Event::Theme(_) | Event::Material(_)) {
        return appearance::material(state, event);
    }
    if matches!(event, Event::ClosePane) {
        return lifecycle::close(state, id);
    }
    if matches!(event, Event::RenameTitle) {
        let target = state
            .borrow()
            .views
            .iter()
            .find(|v| v.id == id)
            .map(|v| (v.window.hwnd().cast(), v.model.clone()));
        if let Some((owner, model)) = target {
            let state = Rc::clone(state);
            rename::show_title(
                owner,
                model,
                Box::new(move |title| handle(&state, id, Event::SetTitle(title)).map(|_| ())),
            )?;
        }
        return Ok(false);
    }
    if let Event::PaneItemFocus = event {
        let s = state.borrow();
        for view in s.views.iter().filter(|view| view.id != id) {
            if s.workspace.panel(view.id).is_some_and(Panel::is_search) {
                unsafe {
                    PostMessageW(view.window.hwnd().cast(), search::CLEAR_SELECTION, 0, 0);
                }
            }
            let mut model = view.model.borrow_mut();
            if model.selected.is_some() || !model.selection.is_empty() || model.focused {
                model.clear_selection();
                model.focused = false;
                drop(model);
                unsafe {
                    windows_sys::Win32::Graphics::Gdi::InvalidateRect(
                        view.window.hwnd().cast(),
                        std::ptr::null(),
                        0,
                    );
                }
            }
        }
        hybrid::clear_desktop_selection(&s)?;
        return Ok(false);
    }
    if let Event::BeginItemMenu(reply) = event {
        *reply.borrow_mut() = Some(hybrid::begin_item_menu(&state.borrow()));
        return Ok(false);
    }
    if let Event::EndItemMenu = event {
        hybrid::end_item_menu(&state.borrow());
        return Ok(false);
    }
    if let Event::RenameItem(identity) = event {
        hybrid::clear_desktop_selection(&state.borrow())?;
        let target = {
            let s = state.borrow();
            s.views.iter().find(|v| v.id == id).and_then(|v| {
                let model = v.model.borrow();
                model
                    .items
                    .iter()
                    .find(|i| i.identity == identity)
                    .map(|i| (v.window.hwnd().cast(), i.label.clone(), v.model.clone()))
            })
        };
        if let Some((owner, label, model)) = target {
            let weak = Rc::downgrade(state);
            rename::show_managed(
                owner,
                &identity,
                &label,
                model,
                Rc::new(move |identity, name| {
                    let state = weak.upgrade().ok_or(crate::i18n::text("rename-context-unavailable"))?;
                    hybrid::rename_item(&state, owner, identity, name)
                }),
            )?;
        }
        return Ok(false);
    }
    if matches!(event, Event::ToggleSearch) {
        return lifecycle::toggle_search(state, id);
    }
    if matches!(
        event,
        Event::New | Event::EnableMonitor | Event::EnableSearch | Event::MapFolder(_)
    ) {
        return lifecycle::create(state, event);
    }
    panel::apply(state, id, event)
}

/// Animate effective visibility without changing saved window preferences.
pub(super) fn show_collapsed(state: &PaneApp, id: PanelId, collapsed: bool) {
    let Some(panel) = state.workspace.panel(id) else {
        return;
    };
    let Some(view) = state.views.iter().find(|v| v.id == id) else {
        return;
    };
    if view.model.borrow().collapsed == collapsed {
        return;
    }
    view.model.borrow_mut().collapsed = collapsed;
    let height = if collapsed {
        layout::HEADER
    } else {
        panel.rect().height
    };
    unsafe {
        PostMessageW(
            view.window.hwnd().cast(),
            window::ANIMATE_FOLD,
            0,
            height.round() as isize,
        );
    }
}
