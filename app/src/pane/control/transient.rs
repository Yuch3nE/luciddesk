//! Deferred single-operation plans. Startup OS writes are dispatched separately.
use super::*;
use luciddesk_api::Operation;
pub(super) fn is_runtime(op: &Operation) -> bool {
    matches!(
        op,
        Operation::StartupSet { .. } | Operation::FolderNavigate { .. }
            | Operation::FolderRefresh { .. }
            | Operation::FolderBack { .. }
            | Operation::FolderHome { .. }
            | Operation::SearchQuery { .. }
            | Operation::SearchRefresh { .. }
            | Operation::SearchMore { .. }
    )
}
fn id(raw: &str) -> Result<PanelId, String> {
    raw.parse::<u64>()
        .ok()
        .filter(|id| *id > 0 && *id <= i64::MAX as u64)
        .map(PanelId::new)
        .ok_or_else(|| "invalid panel ID".into())
}
pub(super) fn prepare(workspace: &Workspace, op: &Operation) -> Result<Operation, String> {
    if let Operation::StartupSet {expected_status,..} = op {
        let status = crate::startup::Status::from_code(expected_status).ok_or("invalid expected startup status")?;
        if !status.editable() {return Err("startup status does not permit changes".into());}
        return Ok(op.clone());
    }
    let (raw, folder) = match op {
        Operation::FolderNavigate { pane_id, .. }
        | Operation::FolderRefresh { pane_id }
        | Operation::FolderBack { pane_id }
        | Operation::FolderHome { pane_id } => (pane_id, true),
        Operation::SearchQuery { pane_id, .. }
        | Operation::SearchRefresh { pane_id }
        | Operation::SearchMore { pane_id } => (pane_id, false),
        _ => return Err("not a runtime operation".into()),
    };
    let panel = workspace.panel(id(raw)?).ok_or("panel does not exist")?;
    if (folder && panel.folder().is_none()) || (!folder && !panel.is_search()) {
        return Err("wrong panel kind for runtime operation".into());
    }
    let mut op = op.clone();
    match &mut op {
        Operation::FolderNavigate { path, .. } => {
            *path = super::folders::path(path)?.to_string_lossy().into_owned()
        }
        Operation::SearchQuery { query, .. } => {
            if query.contains('\0') || query.encode_utf16().count() > 32767 {
                return Err("search query exceeds 32767 UTF-16 units or contains NUL".into());
            }
        }
        _ => {}
    }
    Ok(op)
}
pub(super) fn search_snapshot(
    s: &PaneApp,
    id: PanelId,
    entries: bool,
) -> Result<serde_json::Value, String> {
    if !s.workspace.panel(id).is_some_and(Panel::is_search) {
        return Err("search panel does not exist".into());
    }
    let view = s
        .views
        .iter()
        .find(|v| v.id == id)
        .ok_or("search panel is not open; enable search in settings")?;
    search::control::snapshot(view.window.hwnd().cast(), entries)
}
pub(super) fn fingerprint(s: &PaneApp) -> String {
    let mut folders: Vec<_> = s
        .folders
        .iter()
        .map(|(id, source)| (id.get(), source.navigation_context(), source.items.len(), source.loading, &source.status))
        .collect();
    folders.sort();
    let searches: Vec<_> = s
        .workspace
        .panels()
        .iter()
        .filter(|p| p.is_search())
        .map(|p| (p.id().get(), search_snapshot(s, p.id(), false).ok()))
        .collect();
    format!("{folders:?}:{searches:?}")
}
pub(super) fn execute(
    state: &Rc<RefCell<PaneApp>>,
    op: Operation,
) -> Result<serde_json::Value, String> {
    match op {
        Operation::FolderNavigate { pane_id, path } => {
            let id = id(&pane_id)?;
            {
                let mut s = state.borrow_mut();
                let source = s.folders.get(&id).ok_or("folder panel is not open")?;
                if source.path != Path::new(&path) {
                    folder::navigate(&mut s, id, Some(path.into()))?;
                }
            }
            super::folders::query(&state.borrow(), id)
        }
        Operation::FolderRefresh { pane_id } => {
            let id=id(&pane_id)?;
            let mut s=state.borrow_mut();
            let source=s.folders.get_mut(&id).ok_or("folder panel is not open")?;
            source.loading=true;
            source.status=None;
            source.refresh();
            super::folders::query(&s,id)
        }
        Operation::FolderBack { pane_id } => {
            let id = id(&pane_id)?;
            let mut s = state.borrow_mut();
            if !s.folders.contains_key(&id) {
                return Err("folder panel is not open".into());
            }
            folder::navigate(&mut s, id, None)?;
            super::folders::query(&s, id)
        }
        Operation::FolderHome { pane_id } => {
            let id = id(&pane_id)?;
            let mut s = state.borrow_mut();
            if !s.folders.contains_key(&id) {
                return Err("folder panel is not open".into());
            }
            folder::home(&mut s, id)?;
            super::folders::query(&s, id)
        }
        other => {
            let (pane, action) = match other {
                Operation::SearchQuery { pane_id, query } => {
                    (pane_id, search::control::Action::Query(query))
                }
                Operation::SearchRefresh { pane_id } => (pane_id, search::control::Action::Refresh),
                Operation::SearchMore { pane_id } => (pane_id, search::control::Action::More),
                _ => return Err("unknown runtime operation".into()),
            };
            let id = id(&pane)?;
            let hwnd = {
                let s = state.borrow();
                s.views
                    .iter()
                    .find(|v| v.id == id)
                    .ok_or("search panel is not open")?
                    .window
                    .hwnd()
                    .cast()
            };
            search::control::execute(hwnd, action)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_and_search_are_transient_and_search_generation_is_observable() {
        let _sta = crate::pane::test_support::apartment();
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let mut app = crate::pane::tests::test_state();
        app.workspace
            .panel_mut(PanelId::new(1))
            .unwrap()
            .set_folder(Some(root.path().into()));
        app.workspace
            .panel_mut(PanelId::new(2))
            .unwrap()
            .set_search(true);
        app.store.save_workspace(&app.workspace).unwrap();
        let state = Rc::new(RefCell::new(app));
        create_view(&state, PanelId::new(1)).unwrap();
        create_view(&state, PanelId::new(2)).unwrap();
        let count = state.borrow().store.change_count();
        let before = fingerprint(&state.borrow());
        execute(
            &state,
            Operation::FolderNavigate {
                pane_id: "1".into(),
                path: child.to_string_lossy().into_owned(),
            },
        )
        .unwrap();
        assert_ne!(fingerprint(&state.borrow()), before);
        assert_eq!(
            state
                .borrow()
                .workspace
                .panel(PanelId::new(1))
                .unwrap()
                .folder(),
            Some(root.path())
        );
        execute(
            &state,
            Operation::FolderBack {
                pane_id: "1".into(),
            },
        )
        .unwrap();
        assert_eq!(state.borrow().folders[&PanelId::new(1)].path, root.path());
        let result = execute(
            &state,
            Operation::SearchQuery {
                pane_id: "2".into(),
                query: "CLI transient test".into(),
            },
        )
        .unwrap();
        assert_eq!(result["query"], "CLI transient test");
        assert_eq!(result["busy"], true);
        let generation = result["generation"].clone();
        let same = execute(
            &state,
            Operation::SearchQuery {
                pane_id: "2".into(),
                query: "CLI transient test".into(),
            },
        )
        .unwrap();
        assert_eq!(same["generation"], generation);
        let clear = execute(
            &state,
            Operation::SearchQuery {
                pane_id: "2".into(),
                query: String::new(),
            },
        )
        .unwrap();
        assert_eq!(clear["busy"], false);
        assert_eq!(clear["total"], 0);
        assert_eq!(state.borrow().store.change_count(), count);
        for view in &state.borrow().views {
            window::prepare_close(view.window.hwnd().cast());
        }
    }
}
