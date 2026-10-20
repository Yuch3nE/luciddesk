//! Owned Shell operation snapshots and deferred activation.
use super::*;

pub(in crate::pane) fn activate_with(
    state: &Rc<RefCell<PaneApp>>,
    id: PanelId,
    index: usize,
    open: impl FnOnce(isize, &ShellIdentity) -> Result<(), String> + 'static,
) -> Result<(), String> {
    let target = {
        let s = state.borrow();
        s.views.iter().find(|view| view.id == id).and_then(|view| {
            view.model
                .borrow()
                .items
                .get(index)
                .map(|item| (view.window.hwnd() as isize, item.identity.clone()))
        })
    };
    if let Some((owner, identity)) = target {
        // Shell execution can pump messages for every pane. Run after both
        // the app borrows and the current window/event callbacks have returned.
        if !window::post_action(owner as _, move || {
            if unsafe { IsWindow(owner as _) } == 0 {
                return;
            }
            if let Err(error) = open(owner, &identity) {
                window::error_for(owner as _, &crate::i18n::format("error-open-item", &[
                    ("path", identity.activation_name().to_string_lossy().into_owned()),
                    ("error", error),
                ]));
            }
        }) {
            return Err(crate::i18n::text("ui-could-not-schedule-opening-the-item").into());
        }
    }
    Ok(())
}

pub(in crate::pane) fn activate_item_with(
    state: &Rc<RefCell<PaneApp>>,
    id: PanelId,
    index: usize,
    open: impl FnOnce(isize, &ShellIdentity) -> Result<(), String> + 'static,
) -> Result<(), String> {
    let target = folder::entry_mode::navigation_target(&state.borrow(), id, index)?;
    if let Some(path) = target {
        folder::navigate(&mut state.borrow_mut(), id, Some(path))
    } else {
        activate_with(state, id, index, open)
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn route(
    state: &Rc<RefCell<PaneApp>>,
    id: PanelId,
    event: Event,
) -> Result<bool, String> {
    if let Event::FileDrag(image) = event {
        let target = {
            let s = state.borrow();
            s.views.iter().find(|v| v.id == id).map(|v| {
                (
                    v.window.hwnd() as isize,
                    v.model.borrow().selected_identities(),
                )
            })
        };
        if let Some((owner, items)) = target {
            window::post_action(owner as _, move || {
                if let Err(error) = luciddesk_shell::drag_file_items(
                    windows::Win32::Foundation::HWND(owner as _),
                    &items,
                    image.as_ref(),
                ) {
                    window::error_for(owner as _, &crate::i18n::format("ui-file-operation-failed", &[("error", error.to_string())]));
                }
            });
        }
        return Ok(false);
    }
    if matches!(event, Event::NewFolder | Event::ChangeFolder) {
        folder::request_picker(state, id, matches!(event, Event::ChangeFolder))?;
        return Ok(false);
    }
    if let Event::SetFolder(path) = &event {
        let mut s = state.borrow_mut();
        let old = s.workspace.clone();
        let panel = s
            .workspace
            .panel_mut(id)
            .ok_or(crate::i18n::text("ui-panel-closed"))?;
        if panel.folder().is_none() {
            return Err(crate::i18n::text("ui-this-is-not-a-folder-panel").into());
        }
        panel.set_folder(Some(path.clone()));
        let title = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        panel.set_title(title.clone());
        if let Err(error) = save(&mut s) {
            s.workspace = old;
            return Err(error);
        }
        folder::ensure(&mut s, id)?;
        folder::home(&mut s, id)?;
        if let Some(view) = s.views.iter().find(|v| v.id == id) {
            let mut model = view.model.borrow_mut();
            model.folder = Some(path.clone());
            model.title = title;
            unsafe {
                InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
            }
            model.clear_selection();
            model.scroll = 0;
        }
        refresh_views(&mut s);
        return Ok(false);
    }
    if matches!(event, Event::OpenFolder) {
        let target = {
            let s = state.borrow();
            s.folders.get(&id).map(|source| &source.path).map(|path| {
                (
                    s.views
                        .iter()
                        .find(|v| v.id == id)
                        .map_or(0, |v| v.window.hwnd() as isize),
                    folder::identity(path.to_path_buf()),
                )
            })
        };
        if let Some((owner, identity)) = target {
            window::defer_action(move || {
                if let Err(error) = open_shell_identity(owner, &identity) {
                    window::error_for(owner as _, &crate::i18n::format("error-open-item", &[
                        ("path", identity.activation_name().to_string_lossy().into_owned()),
                        ("error", error.to_string()),
                    ]));
                }
            });
        }
        return Ok(false);
    }
    if let Event::Peek = event {
        let index = {
            let s = state.borrow();
            s.views.iter().find(|view| view.id == id).and_then(|view| {
                let model = view.model.borrow();
                if model.collapsed {
                    return None;
                }
                model
                    .selected
                    .filter(|index| model.selection.contains(index))
                    .or_else(|| model.selection.iter().next().copied())
            })
        };
        if let Some(index) = index {
            let weak = Rc::downgrade(state);
            activate_with(state, id, index, move |owner, identity| {
                let Some(state) = weak.upgrade() else {
                    return Ok(());
                };
                if peek::settings().provider == peek::Provider::QuickLook
                    || state
                        .borrow()
                        .workspace
                        .panel(id)
                        .is_some_and(|p| p.folder().is_some())
                {
                    return peek::open_path(identity);
                }
                if !hybrid::pause_for_preview(&state.borrow(), true)? {
                    return Ok(());
                }
                let result = peek::open(owner, identity);
                let restored = hybrid::pause_for_preview(&state.borrow(), false).map(|_| ());
                result.and(restored)
            })?;
        }
        return Ok(false);
    }
    if let Event::FileCommand(command) = event {
        let target = {
            let s = state.borrow();
            s.views.iter().find(|view| view.id == id).map(|view| {
                let model = view.model.borrow();
                let destination = (command == luciddesk_shell::FileCommand::Paste)
                    .then(|| s.folders.get(&id).map(|source| source.path.clone()))
                    .flatten();
                (view.window.hwnd(), model.selected_identities(), destination)
            })
        };
        if let Some((owner, identity, destination)) = target {
            if !window::post_action(owner.cast(), move || {
                if unsafe { IsWindow(owner.cast()) } == 0 {
                    return;
                }
                let result = if let Some(path) = destination {
                    luciddesk_shell::paste_into_folder(
                        windows::Win32::Foundation::HWND(owner.cast()),
                        &path,
                    )
                } else {
                    luciddesk_shell::invoke_file_commands(
                        windows::Win32::Foundation::HWND(owner.cast()),
                        &identity,
                        command,
                    )
                };
                if let Err(error) = result {
                    window::error_for(owner.cast(), &crate::i18n::format(
                        "ui-file-operation-failed",
                        &[("error", format!("{}", error))],
                    ));
                }
            }) {
                return Err(crate::i18n::text("ui-could-not-schedule-file-operation").into());
            }
        }
        return Ok(false);
    }
    if let Event::ActivateSelection = event {
        let indices: Vec<_> = {
            let s = state.borrow();
            s.views
                .iter()
                .find(|view| view.id == id)
                .map(|view| view.model.borrow().selection.iter().copied().collect())
                .unwrap_or_default()
        };
        if indices.len() == 1 {
            return handle(state, id, Event::Activate(indices[0]));
        }
        for index in indices {
            activate_with(state, id, index, |owner, identity| {
                open_shell_identity(owner, identity).map_err(|error| error.to_string())
            })?;
        }
        return Ok(false);
    }
    unreachable!("only Shell commands are routed here")
}
