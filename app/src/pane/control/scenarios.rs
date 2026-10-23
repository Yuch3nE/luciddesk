//! Skill workflows over a private pipe and temporary workspace, never the live desktop.
use super::*;
use serde_json::Value;

struct Client(String);
impl Client {
    fn send(&self, command: &str, fields: Value) -> Response {
        let mut value = json!({"protocol_version":1,"request_id":luciddesk_api::request_id(),"command":command});
        value
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        let request: Request = serde_json::from_value(value).unwrap();
        luciddesk_api::transport::call_at(&self.0, &request, Duration::from_secs(5)).unwrap()
    }
    fn query(&self, command: &str, fields: Value) -> Response {
        let result = self.send(command, fields);
        assert!(result.ok, "{result:?}");
        result
    }
    fn preview(&self, operations: Value) -> Response {
        let context = self.query("workspace.get", json!({})).context.unwrap();
        self.query(
            "plan.preview",
            json!({"plan":{"protocol_version":1,"base":context,"operations":operations}}),
        )
    }
    fn try_apply(&self, preview: &Response) -> Response {
        let args = json!({"token":preview.data.as_ref().unwrap()["plan_token"],"request_id":luciddesk_api::request_id()});
        let result = self.send("plan.apply", args.clone());
        if !result.ok {
            return result;
        }
        assert_eq!(
            result.data.as_ref().unwrap()["presentation_status"],
            "applied",
            "{result:?}"
        );
        let replay = self.query("plan.apply", args);
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            serde_json::to_value(replay).unwrap()
        );
        let receipt = self.query("request.get", json!({"id":result.request_id}));
        assert_eq!(
            receipt.data.unwrap()["result"],
            serde_json::to_value(&result).unwrap()
        );
        result
    }
    fn apply(&self, preview: &Response) -> Response {
        let result = self.try_apply(preview);
        assert!(result.ok, "{result:?}");
        result
    }
    fn change(&self, operations: Value) -> Response {
        // A completed folder scan can invalidate a reviewed snapshot. Only an
        // explicit pre-commit conflict permits a fresh preview; never retry an
        // uncertain submission or a failed presentation as a new operation.
        for _ in 0..5 {
            let result = self.try_apply(&self.preview(operations.clone()));
            if result
                .error
                .as_ref()
                .is_some_and(|error| error.code == "CONFLICT")
            {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            assert!(result.ok, "{result:?}");
            return result;
        }
        panic!("workspace did not settle after five fresh previews");
    }
}
fn scenario(run: impl FnOnce(Client, std::path::PathBuf) + Send + 'static) {
    scenario_with_search(run, false);
}
fn scenario_with_search(
    run: impl FnOnce(Client, std::path::PathBuf) + Send + 'static,
    search: bool,
) {
    scenario_with_options(run, search, false);
}
fn scenario_with_options(
    run: impl FnOnce(Client, std::path::PathBuf) + Send + 'static,
    search: bool,
    backup: bool,
) {
    let _sta = crate::pane::test_support::apartment();
    let root = tempfile::tempdir().unwrap();
    let fixture = root.path().join("files");
    std::fs::create_dir_all(fixture.join("child")).unwrap();
    std::fs::write(fixture.join("keep.txt"), "untouched").unwrap();
    let mut app = crate::pane::tests::test_state();
    app.store = WorkspaceStore::open(&root.path().join("workspace.db")).unwrap();
    if search {
        app.workspace
            .panel_mut(PanelId::new(2))
            .unwrap()
            .set_search(true);
    }
    app.store.save_workspace(&app.workspace).unwrap();
    if backup {
        app.runtime = Some(runtime::State::new(root.path().join("workspace.db")));
    }
    let state = Rc::new(RefCell::new(app));
    // No runtime connection state: the supervisor cannot attach to Explorer.
    let _runtime = (!search && !backup).then(|| runtime::supervisor(&state).unwrap());
    let endpoint = format!(
        "{}-skill-{}",
        luciddesk_api::transport::endpoint().unwrap(),
        luciddesk_api::request_id()
    );
    if search {
        create_view(&state, PanelId::new(2)).unwrap();
    }
    let control = start_at(&state, &endpoint).unwrap();
    let worker = std::thread::spawn(move || run(Client(endpoint), fixture));
    let deadline = Instant::now() + Duration::from_secs(45);
    while !worker.is_finished() {
        assert!(Instant::now() < deadline, "scenario timed out");
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        folder::poll(&mut state.borrow_mut());
        std::thread::sleep(Duration::from_millis(1));
    }
    worker.join().unwrap();
    drop(control);
}

#[test]
fn skill_defaults_tabs_and_exact_receipt_workflow() {
    scenario(|client, _| {
        let original = client.query("pane.get", json!({"id":"1"})).data.unwrap();
        client.change(json!([{"op":"settings.update","values":{"panel_defaults.icon_only":true,"panel_defaults.icon_size":"small"}}]));
        let existing = client.query("pane.get", json!({"id":"1"})).data.unwrap();
        assert_eq!(existing["icon_only"], original["icon_only"]);
        assert_eq!(existing["icon_size"], original["icon_size"]);
        let preview =
            client.preview(json!([{"op":"pane.create","ref":"new","title":"Skill fixture"}]));
        assert_eq!(
            client.query("pane.list", json!({})).data.unwrap()["panes"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let added = client.apply(&preview).data.unwrap()["refs"]["new"].clone();
        let pane = client.query("pane.get", json!({"id":added})).data.unwrap();
        assert_eq!(pane["icon_only"], true);
        assert_eq!(pane["icon_size"], "small");
        client.change(json!([
            {"op":"tab.merge","pane_id":added,"into_pane_id":"1"},
            {"op":"tab.select","pane_id":added},
            {"op":"pane.update","pane_id":added,"icon_size":"large"}
        ]));
        assert_eq!(
            client.query("pane.get", json!({"id":"1"})).data.unwrap()["icon_size"],
            original["icon_size"]
        );
        let stale = client.preview(json!([{"op":"pane.update","pane_id":added,"title":"stale"}]));
        client.change(json!([{"op":"pane.update","pane_id":added,"title":"newer"}]));
        let conflict = client.send(
            "plan.apply",
            json!({"token":stale.data.unwrap()["plan_token"]}),
        );
        assert_eq!(conflict.error.unwrap().code, "CONFLICT");
        client.change(
            json!([{"op":"tab.detach","pane_id":added},{"op":"pane.remove","pane_id":added}]),
        );
        assert_eq!(
            client.query("workspace.get", json!({})).data.unwrap()["tabs"],
            json!([])
        );
    });
}

#[test]
fn skill_folder_defaults_navigation_and_removal_workflow() {
    scenario(|client, fixture| {
        client.change(json!([{"op":"settings.update","values":{"folder_defaults.list_view":false,"folder_defaults.show_type":false}}]));
        let created = client
            .change(json!([{"op":"folder.create","ref":"folder","title":"Files","path":fixture}]));
        let pane = created.data.unwrap()["refs"]["folder"].clone();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let folder = client.query("folder.get", json!({"id":pane})).data.unwrap();
            if folder["content_layout"]["ready"] == true {
                break;
            }
            assert!(Instant::now() < deadline, "folder did not load");
            std::thread::sleep(Duration::from_millis(20));
        }
        let folder = client.query("folder.get", json!({"id":pane})).data.unwrap();
        assert_eq!(folder["list_view"], false);
        assert!(
            !folder["preferences"]["visible_columns"]
                .as_array()
                .unwrap()
                .contains(&json!("type"))
        );
        client
            .change(json!([{"op":"folder.navigate","pane_id":pane,"path":fixture.join("child")} ]));
        let child = client.query("folder.get", json!({"id":pane})).data.unwrap();
        assert!(child["current_path"].as_str().unwrap().ends_with("child"));
        assert_eq!(child["root_path"], folder["root_path"]);
        client.change(json!([{"op":"folder.home","pane_id":pane}]));
        client.change(json!([{"op":"pane.remove","pane_id":pane}]));
        assert_eq!(
            std::fs::read_to_string(fixture.join("keep.txt")).unwrap(),
            "untouched"
        );
    });
}

#[test]
#[ignore = "Requires the real Everything IPC service; run explicitly on a configured Windows host"]
fn skill_search_real_backend_query_page_and_clear() {
    scenario_with_search(
        |client, _| {
            // Read-only query restricted to this repository, not the user's whole index.
            let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap();
            let query = format!("path:\"{}\"", directory.display());
            let applied = client.change(json!([{"op":"search.query","pane_id":"2","query":query}]));
            let generation = applied.data.unwrap()["runtime_result"]["generation"].clone();
            let wait = |generation: &Value| {
                let deadline = Instant::now() + Duration::from_secs(15);
                loop {
                    let data = client.query("search.get", json!({"id":"2"})).data.unwrap();
                    assert_eq!(&data["generation"], generation);
                    assert_ne!(data["failed"], true, "{data}");
                    if data["busy"] == false && data["replacing"] == false {
                        return data;
                    }
                    assert!(Instant::now() < deadline, "search did not complete");
                    std::thread::sleep(Duration::from_millis(20));
                }
            };
            let first = wait(&generation);
            assert!(
                first["loaded_count"].as_u64().unwrap() > 0,
                "repository is not indexed"
            );
            assert_eq!(
                first["has_more"], true,
                "fixture needs more than one result page"
            );
            let page = client.change(json!([{"op":"search.more","pane_id":"2"}]));
            let next = wait(&page.data.unwrap()["runtime_result"]["generation"]);
            assert!(
                next["loaded_count"].as_u64().unwrap() > first["loaded_count"].as_u64().unwrap()
            );
            let cleared = client.change(json!([{"op":"search.query","pane_id":"2","query":""}]));
            let empty = wait(&cleared.data.unwrap()["runtime_result"]["generation"]);
            assert_eq!(empty["loaded_count"], 0);
            assert_eq!(empty["has_more"], false);
        },
        true,
    );
}

#[test]
fn skill_backup_private_pipe_exports_and_replays_without_duplicate_files() {
    scenario_with_options(
        |client, fixture| {
            let complete = |operations: Value| {
                let preview = client.preview(operations);
                let args = json!({"token":preview.data.unwrap()["plan_token"],"request_id":luciddesk_api::request_id()});
                let applied = client.query("plan.apply", args.clone());
                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    let receipt = client
                        .query("request.get", json!({"id":applied.request_id}))
                        .data
                        .unwrap()["result"]
                        .clone();
                    if receipt["data"]["operation_status"] != "pending" {
                        assert_eq!(
                            receipt["data"]["operation_status"], "completed",
                            "{receipt}"
                        );
                        assert_eq!(
                            serde_json::to_value(client.query("plan.apply", args)).unwrap(),
                            receipt
                        );
                        break receipt["data"].clone();
                    }
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(10));
                }
            };
            let created = complete(json!([{"op":"backup.create"}]));
            let destination = fixture.join("export.db");
            complete(json!([{"op":"backup.export","source":created["path"],"path":destination}]));
            assert_eq!(WorkspaceStore::inspect_backup(&destination).unwrap(), 2);
            assert_eq!(
                client.query("backup.list", json!({})).data.unwrap()["backups"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
        },
        false,
        true,
    );
}
