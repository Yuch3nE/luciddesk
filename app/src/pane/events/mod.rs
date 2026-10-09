//! Group commands and their persisted state transitions.
use super::search::everything_settings;
use super::*;
mod appearance;
mod panel;
mod lifecycle;
mod shell;
mod tab_events;
pub(super) use appearance::{preview_grid, preview_radius, preview_material};
pub(super) use appearance::{commit_grid, commit_radius, commit_material};
pub(super) use shell::activate_item_with;

pub(super) fn handle(
    state: &Rc<RefCell<PaneApp>>,
    id: PanelId,
    event: Event,
) -> Result<bool, String> {
    if matches!(event, Event::ResetPaneOptions) {
        let s = state.borrow();
        s.store.save_metadata_preferences(&[
            ("pane_title_emoji_color", "true"),
            ("pane_compact_menu", "true"),
            ("pane_header_divider", "true"),
            ("desktop_panel_layout", "compact"),
        ]).map_err(|e| e.to_string())?;
        title_emoji::load(&s.store)?;
        compact_menu::load(&s.store)?;
        header_divider::load(&s.store)?;
        for view in &s.views { unsafe { InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0); } }
    }
    if matches!(event, Event::SetTitleEmojiColor(_)) {
        let color = if let Event::SetTitleEmojiColor(color) = event { color } else { true };
        let s = state.borrow();
        title_emoji::save(&s.store, color)?;
        for view in &s.views { unsafe { InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0); } }
        if matches!(event, Event::SetTitleEmojiColor(_)) { return Ok(false); }
    }
    if matches!(event, Event::ToggleCompactMenu) {
        compact_menu::save(&state.borrow().store, !compact_menu::enabled())?;
        if matches!(event, Event::ToggleCompactMenu) { return Ok(false); }
    }
    if matches!(event, Event::ToggleHeaderDivider) {
        let s = state.borrow();
        header_divider::save(&s.store, !header_divider::enabled())?;
        for view in &s.views { unsafe { InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0); } }
        if matches!(event, Event::ToggleHeaderDivider) { return Ok(false); }
    }
    if let Some(result) = tab_events::route(state, id, &event)? { return Ok(result); }
    if let Event::SortPaneColumn(target, column) = event {
        sorting::request(&mut state.borrow_mut(), target, column)?;
        return Ok(false);
    }
    if let Event::SortFolderMenu(column) = event {
        folder::sort(&mut state.borrow_mut(), id, column)?;
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
        let panel = s.workspace.panel_mut(id).ok_or(crate::i18n::text("ui-panel-closed"))?;
        if panel.is_search() {
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
    if matches!(event, Event::FileDrag(_) | Event::NewFolder | Event::ChangeFolder | Event::SetFolder(_) | Event::OpenFolder | Event::Peek | Event::FileCommand(_) | Event::ActivateSelection) {
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
            rename::show_managed(owner, &identity, &label, model, Rc::new(move |identity, name| {
                let state = weak.upgrade().ok_or(crate::i18n::text("ui-group-closed"))?;
                hybrid::rename_item(&state, owner, identity, name)
            }))?;
        }
        return Ok(false);
    }
    if matches!(event, Event::ToggleSearch) {
        return lifecycle::toggle_search(state, id);
    }
    if matches!(
        event,
        Event::New | Event::EnableSearch | Event::MapFolder(_)
    ) {
        return lifecycle::create(state, event);
    }
    panel::apply(state, id, event)
}

/// Animate effective visibility without changing saved window preferences.
pub(super) fn show_collapsed(state: &PaneApp, id: PanelId, collapsed: bool) {
    let Some(panel) = state.workspace.panel(id) else { return; };
    let Some(view) = state.views.iter().find(|v| v.id == id) else { return; };
    if view.model.borrow().collapsed == collapsed { return; }
    view.model.borrow_mut().collapsed = collapsed;
    let height = if collapsed { layout::HEADER } else { panel.rect().height };
    unsafe {
        PostMessageW(view.window.hwnd().cast(), window::ANIMATE_FOLD, 0, height.round() as isize);
    }
}
