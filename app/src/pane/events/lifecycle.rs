//! Panel creation, search activation and window teardown.
use super::*;

pub(super) fn enable_search_view(
    state: &Rc<RefCell<PaneApp>>,
    id: PanelId,
    create: impl FnOnce(&Rc<RefCell<PaneApp>>, PanelId) -> Result<(), String>,
) -> Result<bool, String> {
    enable_search_view_with(state, id, create, |store| {
        everything_settings::set_enabled(store, true)
    })
}

fn enable_search_view_with(
    state: &Rc<RefCell<PaneApp>>,
    id: PanelId,
    create: impl FnOnce(&Rc<RefCell<PaneApp>>, PanelId) -> Result<(), String>,
    persist: impl FnOnce(&WorkspaceStore) -> Result<(), String>,
) -> Result<bool, String> {
    if !state.borrow().views.iter().any(|v| v.id == id) {
        if let Err(error) = create(state, id) {
            everything_settings::set_enabled(&state.borrow().store, false)?;
            return Err(error);
        }
    }
    // Release PaneApp before the failure path removes and destroys the view.
    let saved = persist(&state.borrow().store);
    if let Err(error) = saved {
        let view = {
            let mut s = state.borrow_mut();
            s.views
                .iter()
                .position(|v| v.id == id)
                .map(|at| s.views.remove(at))
        };
        drop(view);
        return Err(error);
    }
    Ok(false)
}

#[cfg(test)]
mod search_lifecycle_tests {
    use super::*;
    #[test]
    fn failed_enable_save_releases_state_before_removing_window() {
        let _apartment = crate::pane::test_support::apartment();
        let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
        let id = PanelId::new(1);
        let result = enable_search_view_with(&state, id, create_view, |_| {
            Err("injected persistence failure".into())
        });
        assert_eq!(result.unwrap_err(), "injected persistence failure");
        assert!(state.borrow().views.is_empty());
        assert!(state.try_borrow_mut().is_ok());
        assert!(!everything_settings::enabled(&state.borrow().store).unwrap());
    }
    #[test]
    fn failed_search_window_can_be_enabled_again() {
        let _apartment = crate::pane::test_support::apartment();
        let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
        state.borrow_mut().workspace.set_appearance(
            luciddesk_core::PanelTheme::Dark,
            luciddesk_core::Backdrop::Translucent { opacity: 1.0 },
        );
        handle(&state, PanelId::new(0), Event::EnableSearch).unwrap();
        let id = state.borrow().views[0].id;
        handle(&state, id, Event::ClosePane).unwrap();
        let failed = enable_search_view(&state, id, |_, _| {
            Err("injected window creation failure".into())
        });
        assert!(failed.is_err());
        assert!(state.borrow().views.is_empty());
        assert!(!everything_settings::enabled(&state.borrow().store).unwrap());
        handle(&state, id, Event::EnableSearch).unwrap();
        assert_eq!(state.borrow().views[0].id, id);
        assert!(everything_settings::enabled(&state.borrow().store).unwrap());
    }
}

pub(super) fn close(state: &Rc<RefCell<PaneApp>>, id: PanelId) -> Result<bool, String> {
    let mut s = state.borrow_mut();
    if s.workspace.panel(id).is_none() {
        return Ok(false);
    }
    let old = s.workspace.clone();
    let search = s.workspace.panel(id).is_some_and(Panel::is_search);
    let members = s
        .workspace
        .tab_group(id)
        .map_or_else(|| vec![id], |g| g.members.clone());
    let result = if search {
        everything_settings::set_enabled(&s.store, false)
    } else {
        for member in &members {
            remove_panel(&mut s.workspace, *member);
        }
        save(&mut s)
    };
    if let Err(error) = result {
        s.workspace = old;
        hybrid::sync(&mut s)?;
        return Err(error);
    }
    for member in members {
        s.folders.remove(&member);
        s.tab_models.remove(&member);
    }
    let view = s
        .views
        .iter()
        .position(|view| view.id == id)
        .map(|at| s.views.remove(at));
    if let Some(view) = &view {
        let hwnd = view.window.hwnd().cast();
        window::prepare_close(hwnd);
        hybrid::unregister_drop(&mut s, hwnd);
    }
    refresh_views(&mut s);
    drop(s);
    drop(view);
    return Ok(false);
}
pub(super) fn toggle_search(state: &Rc<RefCell<PaneApp>>, id: PanelId) -> Result<bool, String> {
    let ids: Vec<_> = state
        .borrow()
        .workspace
        .panels()
        .iter()
        .filter(|p| p.is_search())
        .map(Panel::id)
        .collect();
    let visible = state.borrow().views.iter().any(|v| ids.contains(&v.id));
    if !visible {
        return handle(state, id, Event::EnableSearch);
    }
    for id in ids {
        handle(state, id, Event::ClosePane)?;
    }
    return Ok(false);
}
pub(super) fn create(state: &Rc<RefCell<PaneApp>>, event: Event) -> Result<bool, String> {
    let search = matches!(event, Event::EnableSearch);
    if search && everything_settings::resolved(&everything_settings::settings()).is_none() {
        return Ok(false);
    }
    if search {
        let existing = state
            .borrow()
            .workspace
            .panels()
            .iter()
            .find(|p| p.is_search())
            .map(Panel::id);
        if let Some(existing) = existing {
            return enable_search_view(state, existing, create_view);
        }
    }
    let path = if let Event::MapFolder(path) = &event {
        Some(path.clone())
    } else {
        None
    };
    let next = {
        let mut s = state.borrow_mut();
        let old = s.workspace.clone();
        let next = PanelId::new(
            s.workspace
                .panels()
                .iter()
                .map(|p| p.id().get())
                .max()
                .unwrap_or(0)
                + 1,
        );
        let mut panel = Panel::new(
            next,
            path.as_ref()
                .map(|p| {
                    p.file_name()
                        .unwrap_or(p.as_os_str())
                        .to_string_lossy()
                        .into_owned()
                })
                .unwrap_or_else(|| crate::i18n::text("ui-new-group").into()),
            display_layout::new_pane(&s.workspace, search),
        );
        panel.set_folder(path.clone());
        if path.is_some() {
            folder::Defaults::load(&s.store)?.apply(&s.store, &mut panel)?;
        }
        if search {
            panel.set_search(true);
            panel.set_title(crate::i18n::text("ui-everything-search").to_string());
        }
        if panel.supports_tabs() {
            layout_defaults::Mode::load(&s.store)?.apply(&mut panel);
        }
        s.workspace.add_panel(panel).map_err(|e| e.to_string())?;
        if s.workspace.appearance().is_none() {
            s.workspace
                .panel_mut(next)
                .unwrap()
                .set_backdrop(luciddesk_core::Backdrop::Acrylic);
        }
        if let Err(error) = save(&mut s) {
            s.workspace = old;
            return Err(error);
        }
        next
    };
    if search {
        // Keep the saved configuration on failure so enabling can retry.
        everything_settings::set_enabled(&state.borrow().store, false)?;
        enable_search_view(state, next, create_view)?;
    } else {
        create_view(state, next)?;
    }
    return Ok(false);
}
