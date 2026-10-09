//! CLI dispatcher. Requests use a dedicated message, not the maintenance wake.
mod plans;
mod settings;
mod geometry;
mod content_layout;
mod tab_plan;
mod folders;
mod transient;
mod font_catalog;
mod startup;
use super::*;
use luciddesk_api::{Request, Response};
use serde_json::json;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
pub(super) fn enabled(store: &WorkspaceStore) -> bool {
    // Preferences are cached in memory; requests do not read configuration files.
    store.preference("cli_enabled").is_ok_and(|value| value.as_deref() != Some("false"))
}

const READY: u32 = WM_APP + 0x4c3;
struct Pending {
    request: Request,
    reply: mpsc::SyncSender<Response>,
    expires: Instant,
}
struct Snapshot {
    instance: String,
    item_ids: HashMap<String, String>,
    next_item: u64,
    seen: Option<(Workspace, u64, String, String)>,
    version: u64,
    plans: plans::Plans,
    fonts: font_catalog::Catalog,
    startup: startup::Control,
}
impl Snapshot {
    fn new() -> Self {
        Self {
            instance: luciddesk_api::request_id(),
            item_ids: HashMap::new(),
            next_item: 0,
            seen: None,
            version: 0,
            plans: plans::Plans::default(),
            fonts: font_catalog::Catalog::default(),
            startup: startup::Control::default(),
        }
    }
    fn respond(&mut self, state: &mut PaneApp, request: &Request) -> Response {
        let fail = |code, message| Response::failure(&request.request_id, code, message);
        if !enabled(&state.store) {
            return fail("ACCESS_DENIED", "CLI control is disabled. Enable it in Settings > General > Agent & CLI.");
        }
        if request.protocol_version != luciddesk_api::VERSION {
            return fail("PROTOCOL_MISMATCH", "supported protocol version is 1");
        }
        if let Err(error) = request.validate() {
            return fail("INVALID_REQUEST", error);
        }
        let directory = state.runtime.as_ref().and_then(|r| r.path.parent());
        if let Some(expected) = &request.data_dir {
            let actual = directory.and_then(|p| std::fs::canonicalize(p).ok());
            let expected = std::fs::canonicalize(expected).ok();
            if actual.is_none() || expected.is_none() || actual != expected {
                return fail(
                    "DATA_DIR_MISMATCH",
                    "connected GUI uses a different data directory",
                );
            }
        }
        self.plans.refresh_desktop(state);
        if let Some((id,data)) = self.startup.poll() {self.plans.system_result(&id,data);}
        let monitors = luciddesk_window::enumerate_monitors();
        let observed = (
            state.workspace.clone(),
            state.store.change_count(),
            format!("{monitors:?}"),
            transient::fingerprint(state),
        );
        if self.seen.as_ref() != Some(&observed) {
            self.version += 1;
            self.seen = Some(observed);
        }
        let version = self.version.to_string();
        let base = luciddesk_api::Context {
            instance_id: self.instance.clone(),
            state_version: version.clone(),
            inventory_version: version.clone(),
            topology_token: version,
        };
        let context = json!(base);
        if matches!(
            request.command.as_str(),
            "plan.preview" | "plan.apply" | "request.get"
        ) {
            return self.plans.handle(state, &base, &self.item_ids, request, &monitors);
        }
        let data = match request.command.as_str() {
            "startup.get" => self.startup.query(),
            "font.list" => self.fonts.query(),
            "monitor.list" => geometry::monitors(&monitors),
            "search.get" => {
                let id=PanelId::new(request.id.as_ref().unwrap().parse().unwrap());
                match transient::search_snapshot(state,id,true) {Ok(value)=>value,Err(error)=>return Response::failure(&request.request_id,"CAPABILITY_UNAVAILABLE",error)}
            }

            "folder.get" => {
                let id=PanelId::new(request.id.as_ref().unwrap().parse().unwrap());
                match folders::query(state,id) {Ok(value)=>value,Err(error)=>return Response::failure(&request.request_id,"NOT_FOUND",error)}
            }

            "settings.get" => {
                let values = match state.store.settings() {
                    Ok(values) => values,
                    Err(error) => return Response::failure(&request.request_id, "CAPABILITY_UNAVAILABLE", error.to_string()),
                };
                let values = settings::values(values);
                let workspace_values = match settings::workspace_values(&state.store) {
                    Ok(values) => values,
                    Err(error) => return Response::failure(&request.request_id, "CAPABILITY_UNAVAILABLE", error),
                };
                json!({"scope":"application_config","values":values,"workspace_values":workspace_values,"runtime":settings::runtime(state)})
            }
            "status" => {
                json!({"application_version":env!("CARGO_PKG_VERSION"), "data_dir":directory, "desktop_connected":state.session.as_ref().is_some_and(hybrid::is_alive), "desktop_sync_status":hybrid::membership_status(state), "read_only":false})
            }
            "capabilities" => {
                json!({"commands":luciddesk_api::COMMANDS,"protocol_version":1,"max_frame_bytes":luciddesk_api::MAX_FRAME,"writes":true,"plans":true,"concurrency_tokens":true,"plan_operations":luciddesk_api::OPERATIONS,"pane_geometry":true,"content_layout":{"kinds":["desktop","folder"],"arrange_anchor":"top_right","gap_px":snap::GAP_PX,"icon_columns_min":1,"icon_columns_max":64},"item_ids":"opaque-instance-scoped","schema_version":1})
            }
            command => {
                let pane_values: Vec<_> = state.workspace.panels().iter().map(|panel| {
                    let effective = state.views.iter().find(|v| v.id == panel.id()).map(|v| v.model.borrow().collapsed)
                        .or_else(|| state.workspace.tab_group(panel.id()).and_then(|g| state.views.iter().find(|v| v.id == g.active)).map(|v| v.model.borrow().collapsed));
                    json!({"id":panel.id().get().to_string(),"title":panel.title(),
                        "kind":if panel.is_search(){"search"} else if panel.folder().is_some(){"folder"} else {"desktop"},
                        "content_layout":content_layout::live_query(state,panel.id()),"geometry":geometry::query(state,panel,&monitors),"window_bounds_px":geometry::window_bounds(state,panel.id()),"folder_path":panel.folder(),"locked":panel.locked(),"auto_hide":panel.auto_hide(),
                        "manual_collapsed":panel.collapsed(),"effective_collapsed":effective,
                        "list_view":panel.list_view(),"fixed_icon_positions":panel.fixed_grid(),"align_icons_to_grid":!panel.free_layout(),"always_on_top":panel.always_on_top()})
                }).collect();
                if command == "pane.get" {
                    match pane_values
                        .into_iter()
                        .find(|p| Some(p["id"].as_str().unwrap()) == request.id.as_deref())
                    {
                        Some(pane) => pane,
                        None => return fail("NOT_FOUND", "panel does not exist"),
                    }
                } else if command == "pane.list" {
                    json!({"panes":pane_values})
                } else {
                    if let Some(id) = &request.pane {
                        if !state
                            .workspace
                            .panels()
                            .iter()
                            .any(|p| p.id().get().to_string() == *id)
                        {
                            return fail("NOT_FOUND", "panel does not exist");
                        }
                    }
                    let mut live = std::collections::HashSet::new();
                    let mut items = Vec::new();
                    for item in state.workspace.desktop_items() {
                        let key = item.identity().persistent_key();
                        live.insert(key.clone());
                        let id = self.item_ids.entry(key).or_insert_with(|| {
                            self.next_item += 1;
                            format!("{}-item-{}", self.instance, self.next_item)
                        });
                        let (pane, placement) = match item.placement() {
                            luciddesk_core::DesktopPlacement::Pane { pane_id, position } => (
                                Some(pane_id.get().to_string()),
                                json!({"kind":"pane","pane_id":pane_id.get().to_string(),"column":position.column,"row":position.row,"position_dip":item.pane_position().map(|p|json!({"x":p.x,"y":p.y}))}),
                            ),
                            luciddesk_core::DesktopPlacement::FreeDesktop { .. } => {
                                (None, json!({"kind":"desktop"}))
                            }
                        };
                        if request.unassigned && pane.is_some()
                            || request.pane.is_some() && request.pane != pane
                        {
                            continue;
                        }
                        items.push(json!({"id":id,"display_name":item.display_name(),"path":item.identity().file_system_path(),"placement":placement}));
                    }
                    self.item_ids.retain(|key, _| live.contains(key));
                    if command == "item.list" {
                        json!({"items":items,"inventory_source":"current_app_snapshot"})
                    } else {
                        let tabs:Vec<_> = state.workspace.tab_groups().iter().map(|g| json!({"active":g.active.get().to_string(),"members":g.members.iter().map(|id| id.get().to_string()).collect::<Vec<_>>()})).collect();
                        json!({"panes":pane_values,"items":items,"tabs":tabs,"inventory_source":"current_app_snapshot"})
                    }
                }
            }
        };
        Response::success(&request.request_id, context, data)
    }
}

pub(super) fn start(state: &Rc<RefCell<PaneApp>>) -> Result<windows_window::Window, String> {
    start_at(
        state,
        &luciddesk_api::transport::endpoint().map_err(|e| e.to_string())?,
    )
}
fn start_at(
    state: &Rc<RefCell<PaneApp>>,
    pipe_name: &str,
) -> Result<windows_window::Window, String> {
    let (send, receive) = mpsc::sync_channel::<Pending>(16);
    let endpoint = Arc::new(Mutex::new(0isize));
    let stopped = Arc::new(AtomicBool::new(false));
    let peer = endpoint.clone();
    let stopping = stopped.clone();
    let server = luciddesk_api::transport::Server::start_at(pipe_name, move |request| {
        let id = request.request_id.clone();
        let (reply, result) = mpsc::sync_channel(1);
        let expires = Instant::now() + Duration::from_secs(5);
        if send
            .try_send(Pending {
                request,
                reply,
                expires,
            })
            .is_err()
        {
            return Response::failure(&id, "BUSY", "control queue is full");
        }
        {
            let hwnd = peer.lock().unwrap();
            if *hwnd == 0 {
                return Response::failure(&id, "BUSY", "control window is not ready");
            }
            unsafe {
                PostMessageW(*hwnd as _, READY, 0, 0);
            }
        }
        while Instant::now() < expires && !stopping.load(Ordering::Acquire) {
            match result.recv_timeout(Duration::from_millis(50)) {
                Ok(response) => return response,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => (),
            }
        }
        Response::failure(
            &id,
            "TIMEOUT",
            "UI did not answer within the server deadline",
        )
    })
    .map_err(|e| e.to_string())?;
    let mut server = Some(server);
    let owner = endpoint.clone();
    let weak = Rc::downgrade(state);
    let mut snapshot = Snapshot::new();
    let window = windows_window::Window::new("LucidDesk Control")
        .size(1, 1)
        .style(WS_POPUP)
        .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)
        .on_message(move |raw, msg, _, lp| {
            if unsafe { crate::window_visibility::defer_show(msg, lp, false) } {
                return Some(0);
            }
            if msg == WM_DESTROY {
                *owner.lock().unwrap() = 0;
                stopped.store(true, Ordering::Release);
                server.take();
                return Some(0);
            }
            if msg != READY && msg != WM_TIMER {
                return None;
            }
            unsafe {
                KillTimer(raw.cast(), 1);
            }
            if let Some(state) = weak.upgrade() {
                if state.try_borrow_mut().is_ok() {
                    while let Ok(pending) = receive.try_recv() {
                        if Instant::now() < pending.expires {
                            let previous = state.borrow().store.change_count();
                            let before = (pending.request.command == "plan.apply")
                                .then(|| state.borrow().workspace.clone());
                            let mut response =
                                snapshot.respond(&mut state.borrow_mut(), &pending.request);
                            if pending.request.command=="plan.apply" {
                                if let Some(action)=snapshot.plans.take_runtime_action(&response) {
                                    if let luciddesk_api::Operation::StartupSet {enabled,expected_status} = action {
                                        let data=snapshot.startup.apply(&response.request_id,enabled,&expected_status);
                                        response=snapshot.plans.system_result(&response.request_id,data).unwrap();
                                    } else {
                                    let prior=transient::fingerprint(&state.borrow());
                                    let result=transient::execute(&state,action);
                                    response.data.as_mut().unwrap()["changed"]=json!(prior!=transient::fingerprint(&state.borrow()));
                                    match result {
                                        Ok(value)=>{
                                            response.data.as_mut().unwrap()["runtime_result"]=value;
                                            snapshot.plans.presentation_result(&mut response,Ok(()));
                                        }
                                        Err(error)=>snapshot.plans.presentation_result(&mut response,Err(error)),
                                    }
                                    }
                                }
                            }
                            if state.borrow().store.change_count() != previous
                                || response.data.as_ref().is_some_and(|data| data["scope"] == "settings" && data["presentation_status"] == "pending")
                            {
                                if let Some(before) = &before {
                                    if response.data.as_ref().is_some_and(|data| data["scope"] == "settings") {
                                        let result = settings::present(&state);
                                        if let Err(error) = &result {
                                            luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "cli.settings", error);
                                        }
                                        snapshot.plans.presentation_result(&mut response, result);
                                    } else {
                                        let view_result = present(&state, before);
                                        let geometry_result = geometry::present(&state, before);
                                        let result = view_result.and(geometry_result);
                                        if let Err(error) = &result {
                                            luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "cli.presentation", error);
                                        }
                                        snapshot.plans.presentation_result(&mut response, result);
                                    }
                                }
                            }
                            let _ = pending.reply.send(response);
                        }
                    }
                } else {
                    unsafe {
                        SetTimer(raw.cast(), 1, 25, None);
                    }
                }
            }
            Some(0)
        })
        .create()
        .map_err(|e| e.to_string())?;
    *endpoint.lock().unwrap() = window.hwnd() as isize;
    Ok(window)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_cli_rejects_all_requests_without_reading_or_mutating_workspace() {
        let mut state = super::super::tests::test_state();
        let mut snapshot = Snapshot::new();
        assert!(enabled(&state.store));
        state.store.save_preference("cli_enabled", "false").unwrap();
        let changes = state.store.change_count();
        for command in luciddesk_api::COMMANDS {
            let request: Request = serde_json::from_value(json!({
                "protocol_version":1, "request_id":"disabled", "command":command
            })).unwrap();
            let response = snapshot.respond(&mut state, &request);
            assert_eq!(response.error.unwrap().code, "ACCESS_DENIED", "{command}");
        }
        assert_eq!(state.store.change_count(), changes);
        assert!(snapshot.seen.is_none());
        assert_eq!(snapshot.version, 0);
        state.store.save_preference("cli_enabled", "true").unwrap();
        let request: Request = serde_json::from_value(json!({
            "protocol_version":1, "request_id":"enabled", "command":"status"
        })).unwrap();
        assert!(snapshot.respond(&mut state, &request).ok);
    }

    #[test]
    fn queries_preserve_database_and_config_and_reject_invalid_requests() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspace.db");
        let mut state = super::super::tests::test_state();
        state.store = WorkspaceStore::open(&path).unwrap();
        state.store.save_workspace(&state.workspace).unwrap();
        state.runtime = Some(runtime::State::new(path.clone()));
        let db = std::fs::read(&path).unwrap();
        let config = std::fs::read(dir.path().join("config.toml")).unwrap();
        let changes = state.store.change_count();
        let mut snapshot = Snapshot::new();
        let mut request = Request {
            protocol_version: 1,
            request_id: "test".into(),
            command: "status".into(),
            id: None,
            pane: None,
            unassigned: false,
            data_dir: None,
            plan: None,
            token: None,
        };
        for command in luciddesk_api::COMMANDS
            .iter()
            .filter(|c| !matches!(**c, "plan.preview" | "plan.apply" | "request.get" | "folder.get" | "search.get"))
        {
            request.command = (*command).into();
            request.id = (*command == "pane.get").then(|| "1".into());
            assert!(snapshot.respond(&mut state, &request).ok, "{command}");
        }
        request.command = "settings.get".into();
        request.id = None;
        let settings = snapshot.respond(&mut state, &request).data.unwrap();
        assert_eq!(settings["scope"], "application_config");
        assert_eq!(settings["values"]["diagnostics.level"], "error");
        assert!(settings["values"]["search.enabled"].is_boolean());
        assert!(settings["values"]["panel_defaults.grid_scale"].is_number());
        request.command = "status".into();
        request.protocol_version = 2;
        assert_eq!(snapshot.respond(&mut state, &request).exit_code(), 10);
        request.protocol_version = 1;
        request.data_dir = Some(dir.path().join("missing").to_string_lossy().into_owned());
        assert_eq!(snapshot.respond(&mut state, &request).exit_code(), 5);
        assert_eq!(state.store.change_count(), changes);
        assert_eq!(std::fs::read(path).unwrap(), db);
        assert_eq!(
            std::fs::read(dir.path().join("config.toml")).unwrap(),
            config
        );
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    #[test]
    fn committed_tab_selection_reuses_window_and_detach_restores_cached_content() {
        let _sta=crate::pane::test_support::apartment();
        let app=super::super::tests::test_state();
        let state=Rc::new(RefCell::new(app));
        create_view(&state,PanelId::new(1)).unwrap();
        create_view(&state,PanelId::new(2)).unwrap();
        let hwnd=state.borrow().views.iter().find(|v|v.id==PanelId::new(1)).unwrap().window.hwnd();
        let before=state.borrow().workspace.clone();
        tab_plan::apply(&mut state.borrow_mut().workspace,&luciddesk_api::Operation::TabMerge{pane_id:"2".into(),into_pane_id:"1".into()}).unwrap();
        present(&state,&before).unwrap();
        assert_eq!(state.borrow().views.len(),1);
        state.borrow().views[0].model.borrow_mut().collapsed=true;
        let before=state.borrow().workspace.clone();
        tab_plan::apply(&mut state.borrow_mut().workspace,&luciddesk_api::Operation::TabSelect{pane_id:"2".into()}).unwrap();
        let count=state.borrow().store.change_count();
        present(&state,&before).unwrap();
        assert_eq!(state.borrow().store.change_count(),count);
        assert_eq!(state.borrow().views[0].window.hwnd(),hwnd);
        assert_eq!(state.borrow().views[0].id,PanelId::new(2));
        assert!(state.borrow().views[0].model.borrow().collapsed);
        let before=state.borrow().workspace.clone();
        tab_plan::apply(&mut state.borrow_mut().workspace,&luciddesk_api::Operation::TabDetach{pane_id:"1".into()}).unwrap();
        present(&state,&before).unwrap();
        assert_eq!(state.borrow().views.len(),2);
        assert!(state.borrow().views.iter().all(|v|v.model.borrow().tabs.is_empty()));
        for view in &state.borrow().views {window::prepare_close(view.window.hwnd().cast());}
    }

    #[test]
    fn presentation_removes_windows_without_resetting_transient_collapse() {
        let _sta=crate::pane::test_support::apartment();
        let mut app=super::super::tests::test_state();
        app.workspace.panel_mut(PanelId::new(1)).unwrap().set_auto_hide(true);
        let state=Rc::new(RefCell::new(app));create_view(&state,PanelId::new(1)).unwrap();
        state.borrow().views[0].model.borrow_mut().collapsed=true;
        let before=state.borrow().workspace.clone();
        state.borrow_mut().workspace.panel_mut(PanelId::new(1)).unwrap().set_title("renamed");
        present(&state,&before).unwrap();
        assert!(state.borrow().views.iter().find(|v|v.id==PanelId::new(1)).unwrap().model.borrow().collapsed);
        let before=state.borrow().workspace.clone();
        let hwnd=state.borrow().views.iter().find(|v|v.id==PanelId::new(1)).unwrap().window.hwnd();
        state.borrow_mut().workspace.remove_panel(PanelId::new(1));
        present(&state,&before).unwrap();
        assert!(state.borrow().views.iter().all(|v|v.id!=PanelId::new(1)));
        assert_eq!(unsafe{IsWindow(hwnd.cast())},0);
    }

    #[test]
    fn pipe_query_runs_on_ui_thread_without_persistence() {
        let _sta = crate::pane::test_support::apartment();
        let mut app = super::super::tests::test_state();
        app.store.save_workspace(&app.workspace).unwrap();
        let before = app.store.change_count();
        let state = Rc::new(RefCell::new(app));
        let name = format!(
            "{}-ui-{}",
            luciddesk_api::transport::endpoint().unwrap(),
            luciddesk_api::request_id()
        );
        let control = start_at(&state, &name).unwrap();
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let request = Request {
                protocol_version: 1,
                request_id: "ui-integration".into(),
                command: "workspace.get".into(),
                id: None,
                pane: None,
                unassigned: false,
                data_dir: None,
                plan: None,
                token: None,
            };
            tx.send(luciddesk_api::transport::call_at(
                &name,
                &request,
                Duration::from_secs(3),
            ))
            .unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(4);
        let response = loop {
            if let Ok(response) = rx.try_recv() {
                break response.unwrap();
            }
            assert!(Instant::now() < deadline, "CLI response timed out");
            unsafe {
                let mut msg = std::mem::zeroed();
                while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        assert!(response.ok);
        assert_eq!(
            response.data.unwrap()["panes"].as_array().unwrap().len(),
            state.borrow().workspace.panels().len()
        );
        assert_eq!(state.borrow().store.change_count(), before);
        worker.join().unwrap();
        drop(control);
    }
}

/// Presentation follows durable state. Failures never turn a committed plan into a retryable write.
fn present(state: &Rc<RefCell<PaneApp>>, before: &Workspace) -> Result<(), String> {
    let mut errors = Vec::new();
    let switches: Vec<_> = {
        let s=state.borrow();
        s.workspace.tab_groups().iter().filter_map(|g| {
            if s.views.iter().any(|v|v.id==g.active) {return None;}
            s.views.iter().find(|v|g.members.contains(&v.id)).map(|v|(v.id,g.active))
        }).collect()
    };
    for (from,to) in switches {
        if let Err(error)=tabs::present_selection(state,from,to) {
            errors.push(error);
        }
    }

    // Destroy outside the RefCell borrow: native teardown may pump callbacks.
    let removed = {
        let mut s = state.borrow_mut();
        let mut removed = Vec::new();
        let mut at = 0;
        while at < s.views.len() {
            let id = s.views[at].id;
            if s.workspace.panel(id).is_none() || !s.workspace.tab_visible(id) {
                let view = s.views.remove(at);
                let hwnd = view.window.hwnd().cast();
                window::prepare_close(hwnd);
                hybrid::unregister_drop(&mut s, hwnd);
                if s.workspace.panel(id).is_some() {
                    s.tab_models.insert(id, view.model.borrow().clone());
                    if let Some(source)=s.folders.get(&id) {source.set_active(false);}
                } else {
                    s.tab_models.remove(&id);
                    s.folders.remove(&id);
                }
                removed.push(view);
            } else {
                at += 1;
            }
        }
        let live: std::collections::HashSet<_> = s.workspace.panels().iter().map(Panel::id).collect();
        s.tab_models.retain(|id,_| live.contains(id));
        s.folders.retain(|id,_| live.contains(id));
        removed
    };
    drop(removed);
    let missing: Vec<_> = {
        let s = state.borrow();
        s.workspace
            .panels()
            .iter()
            .filter(|p| {
                !p.is_search()
                    && s.workspace.tab_visible(p.id())
                    && !s.views.iter().any(|v| v.id == p.id())
            })
            .map(Panel::id)
            .collect()
    };
    for id in missing {
        if let Err(error) = create_view(state, id) {
            errors.push(error);
        } else {
            tabs::restore_cached_model(&mut state.borrow_mut(),id);
        }
    }
    let mut s = state.borrow_mut();
    let folder_ids: Vec<_> = s.workspace.panels().iter().filter(|p|p.folder().is_some()).map(Panel::id).collect();
    for id in folder_ids {
        if let Err(error)=folder::apply_saved_preferences(&mut s,id) {errors.push(error);}
    }
    for view in &s.views {
        if let Some(panel) = s.workspace.panel(view.id) {
            {
                let mut model = view.model.borrow_mut();
                model.title = panel.title().to_owned();
                model.locked = panel.locked();
                model.auto_hide = panel.auto_hide();
            }
            if before.panel(view.id).is_none_or(|old| {
                old.collapsed() != panel.collapsed() || old.auto_hide() != panel.auto_hide()
            }) {
                events::show_collapsed(&s, view.id, panel.collapsed());
            }
            if before
                .panel(view.id)
                .is_none_or(|old| old.always_on_top() != panel.always_on_top())
            {
                window::set_layer(view.window.hwnd().cast(), panel.always_on_top());
            }
            unsafe {
                windows_sys::Win32::Graphics::Gdi::InvalidateRect(
                    view.window.hwnd().cast(),
                    std::ptr::null(),
                    0,
                );
            }
        }
    }
    refresh_views(&mut s);
    if let Err(error) = hybrid::sync(&mut s) {
        errors.push(error);
    }
    s.wake.notify();
    if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
}
