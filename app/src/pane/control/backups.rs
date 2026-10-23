//! CLI backup jobs share the GUI busy gate and storage's validated restore.
use super::*;
use luciddesk_api::Operation;
use std::path::PathBuf;

const MAX_BACKUP_BYTES: u64 = 64 * 1024 * 1024;
pub(super) struct Prepared {
    pub summary: serde_json::Value,
    destination: Option<PathBuf>,
    source: Option<tempfile::TempDir>,
    restore: bool,
}
pub(super) fn supported(op: &Operation) -> bool {
    matches!(
        op,
        Operation::BackupCreate {}
            | Operation::BackupExport { .. }
            | Operation::BackupRestore { .. }
    )
}
fn absolute(raw: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw);
    if !path.is_absolute() || raw.contains(['\0', '\r', '\n']) {
        return Err("backup path must be absolute and contain no control characters".into());
    }
    if path.components().any(|part| matches!(part, std::path::Component::Normal(name) if name.to_string_lossy().contains(':'))) {
        return Err("alternate data stream paths are not supported".into());
    }
    Ok(path)
}
fn reserved(state: &PaneApp, path: &Path) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "workspace.db" | "workspace.db-wal" | "workspace.db-shm" | "config.toml"
    ) {
        return true;
    }
    let resolved = path.canonicalize().ok();
    [
        state.runtime.as_ref().map(|r| r.path.as_path()),
        state.store.config_path().as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|live| {
        live == path
            || resolved
                .as_ref()
                .is_some_and(|p| live.canonicalize().ok().as_ref() == Some(p))
    })
}
fn stage(state: &PaneApp, raw: &str) -> Result<(tempfile::TempDir, usize), String> {
    use std::io::{Read, Write};
    let path = absolute(raw)?.canonicalize().map_err(|e| e.to_string())?;
    if reserved(state, &path) {
        return Err("select a standalone backup, not live application data".into());
    }
    if !path.is_file()
        || path
            .with_file_name(format!(
                "{}-wal",
                path.file_name().unwrap().to_string_lossy()
            ))
            .exists()
    {
        return Err("source must be a standalone backup without a WAL file".into());
    }
    let input = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    if input.metadata().map_err(|e| e.to_string())?.len() > MAX_BACKUP_BYTES {
        return Err("backup exceeds 64 MiB preview limit".into());
    }
    let mut bytes = Vec::new();
    input
        .take(MAX_BACKUP_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BACKUP_BYTES {
        return Err("backup exceeds 64 MiB preview limit".into());
    }
    let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
    let staged = temporary.path().join("restore.db");
    std::fs::File::create(&staged)
        .and_then(|mut f| f.write_all(&bytes))
        .map_err(|e| e.to_string())?;
    let count = WorkspaceStore::inspect_backup(&staged).map_err(|e| e.to_string())?;
    Ok((temporary, count))
}
pub(super) fn prepare(state: &PaneApp, op: &Operation) -> Result<Prepared, String> {
    if recovery::view(state).busy {
        return Err("a backup task is running".into());
    }
    recovery::directory(state)?;
    if hybrid::workspace_save_pending(state) {
        return Err("wait for pending workspace saves before backing up or restoring".into());
    }
    let mut prepared = Prepared {
        summary: json!({"operation":op,"includes":["settings","panel_layout"],"includes_user_files":false}),
        destination: None,
        source: None,
        restore: false,
    };
    match op {
        Operation::BackupCreate {} => {}
        Operation::BackupExport { path, source } => {
            let path = absolute(path)?;
            let parent = path
                .parent()
                .ok_or("invalid destination")?
                .canonicalize()
                .map_err(|e| e.to_string())?;
            let path = parent.join(path.file_name().ok_or("invalid destination filename")?);
            if reserved(state, &path) || path.try_exists().map_err(|e| e.to_string())? {
                return Err(
                    "destination is reserved or already exists; choose a new filename".into(),
                );
            }
            prepared.destination = Some(path);
            if let Some(source) = source {
                prepared.source = Some(stage(state, source)?.0);
            }
        }
        Operation::BackupRestore { path } => {
            let (source, count) = stage(state, path)?;
            prepared.summary["panel_count"] = json!(count);
            prepared.summary["replaces_current_configuration"] = json!(true);
            prepared.summary["source_is_frozen"] = json!(true);
            let staged = WorkspaceStore::read_backup(&source.path().join("restore.db"))
                .map_err(|e| e.to_string())?;
            prepared.summary["restored_cli_enabled"] = json!(enabled(&staged));
            prepared.source = Some(source);
            prepared.restore = true;
        }
        _ => return Err("not a backup operation".into()),
    }
    Ok(prepared)
}
pub(super) fn query(state: &PaneApp) -> Result<serde_json::Value, String> {
    let directory = recovery::directory(state)?;
    match std::fs::read_dir(&directory) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let records = recovery::records(&directory);
    Ok(
        json!({"directory":directory,"busy":recovery::view(state).busy,"backups":records.iter().map(|r|json!({"path":r.path,"date":r.date,"kind":r.kind,"bytes":r.bytes})).collect::<Vec<_>>()}),
    )
}
struct PendingJob {
    id: String,
    revision: u64,
    workspace: Workspace,
    receiver: mpsc::Receiver<Result<CompletedJob, String>>,
}
struct CompletedJob {
    path: PathBuf,
    source: Option<tempfile::TempDir>,
}
#[derive(Default)]
pub(super) struct Control {
    pending: Option<PendingJob>,
}
fn result(id: &str, status: &str, commit: &str, error: Option<String>) -> serde_json::Value {
    json!({"scope":"backup","operation_id":id,"operation_status":status,"commit_status":commit,
        "presentation_status":if status=="pending"{"pending"}else if error.is_some(){"failed"}else{"applied"},"error":error})
}
impl Control {
    pub(super) fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub(super) fn apply(
        &mut self,
        state: &Rc<RefCell<PaneApp>>,
        id: &str,
        prepared: Prepared,
    ) -> serde_json::Value {
        let launched = (|| -> Result<(), String> {
            let mut s = state.borrow_mut();
            if self.busy() || recovery::view(&s).busy {
                return Err("a backup task is running".into());
            }
            if hybrid::workspace_save_pending(&s) {
                return Err("workspace save is pending".into());
            }
            let directory = recovery::directory(&s)?;
            let revision = s.store.change_count();
            let workspace = s.workspace.clone();
            let snapshot = s.store.backup_snapshot().map_err(|e| e.to_string())?;
            let (tx, receiver) = mpsc::channel();
            std::thread::Builder::new()
                .name("cli-backup".into())
                .spawn(move || {
                    let run = || -> Result<CompletedJob, String> {
                        if prepared.restore {
                            std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
                            // Do not prune rollback snapshots as part of restoring.
                            let path = directory
                                .join(format!("before-restore-{}.db", luciddesk_api::request_id()));
                            snapshot.export_backup(&path).map_err(|e| e.to_string())?;
                            return Ok(CompletedJob {
                                path,
                                source: prepared.source,
                            });
                        }
                        let source = if let Some(source) = &prepared.source {
                            WorkspaceStore::read_backup(&source.path().join("restore.db"))
                                .map_err(|e| e.to_string())?
                        } else {
                            snapshot
                        };
                        let path = if let Some(path) = prepared.destination {
                            path
                        } else {
                            std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
                            directory.join(format!("manual-{}.db", luciddesk_api::request_id()))
                        };
                        source.export_backup(&path).map_err(|e| e.to_string())?;
                        Ok(CompletedJob { path, source: None })
                    };
                    let _ = tx.send(run());
                })
                .map_err(|e| e.to_string())?;
            s.runtime.as_mut().unwrap().backup.view.busy = true;
            self.pending = Some(PendingJob {
                id: id.into(),
                revision,
                workspace,
                receiver,
            });
            Ok(())
        })();
        match launched {
            Ok(()) => result(id, "pending", "pending", None),
            Err(error) => result(id, "failed", "not_committed", Some(error)),
        }
    }
    pub(super) fn poll(
        &mut self,
        state: &Rc<RefCell<PaneApp>>,
    ) -> Option<(String, serde_json::Value)> {
        self.poll_with(state, runtime::reload)
    }
    fn poll_with(
        &mut self,
        state: &Rc<RefCell<PaneApp>>,
        mut reload: impl FnMut(&Rc<RefCell<PaneApp>>) -> Result<(), String>,
    ) -> Option<(String, serde_json::Value)> {
        let job = self.pending.as_ref()?;
        let completed = match job.receiver.try_recv() {
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("backup worker disconnected; inspect destination before retrying".into())
            }
            Ok(value) => value,
        };
        let job = self.pending.take().unwrap();
        let mut data = result(&job.id, "completed", "committed", None);
        match completed {
            Err(error) => data = result(&job.id, "failed", "unknown", Some(error)),
            Ok(done) => {
                data["path"] = json!(done.path);
                if let Some(source) = done.source {
                    data["rollback_path"] = json!(done.path);
                    let unchanged = {
                        let s = state.borrow();
                        s.store.change_count() == job.revision
                            && s.workspace == job.workspace
                            && !hybrid::workspace_save_pending(&s)
                    };
                    let restored = if unchanged {
                        state
                            .borrow_mut()
                            .store
                            .restore_backup(&source.path().join("restore.db"))
                            .map_err(|e| e.to_string())
                    } else {
                        Err("configuration changed while preparing restore; preview again".into())
                    };
                    if let Err(error) = restored {
                        data = result(&job.id, "failed", "not_committed", Some(error));
                        data["rollback_path"] = json!(done.path);
                    } else if let Err(error) = reload(state) {
                        let rollback = state
                            .borrow_mut()
                            .store
                            .restore_backup(&done.path)
                            .map_err(|e| e.to_string());
                        let rollback = rollback.and_then(|_| reload(state));
                        data = result(
                            &job.id,
                            "failed",
                            if rollback.is_ok() {
                                "rolled_back"
                            } else {
                                "unknown"
                            },
                            Some(error),
                        );
                        data["rollback_error"] = json!(rollback.err());
                        data["rollback_path"] = json!(done.path);
                    } else {
                        state
                            .borrow_mut()
                            .runtime
                            .as_mut()
                            .unwrap()
                            .backup
                            .view
                            .undo = Some(done.path);
                    }
                }
            }
        }
        let mut s = state.borrow_mut();
        let directory = recovery::directory(&s).ok();
        let view = &mut s.runtime.as_mut().unwrap().backup.view;
        view.busy = false;
        view.status = data["error"]
            .as_str()
            .unwrap_or("CLI backup operation completed")
            .into();
        if let Some(directory) = directory {
            view.records = recovery::records(&directory).into();
        }
        Some((job.id, data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(root: &Path) -> Rc<RefCell<PaneApp>> {
        let mut app = crate::pane::tests::test_state();
        let path = root.join("workspace.db");
        app.store = WorkspaceStore::open(&path).unwrap();
        app.store.save_workspace(&app.workspace).unwrap();
        app.runtime = Some(runtime::State::new(path));
        Rc::new(RefCell::new(app))
    }
    fn reload(state: &Rc<RefCell<PaneApp>>) -> Result<(), String> {
        let workspace = state
            .borrow()
            .store
            .load_workspace()
            .map_err(|e| e.to_string())?;
        state.borrow_mut().workspace = workspace;
        Ok(())
    }
    fn finish(
        control: &mut Control,
        state: &Rc<RefCell<PaneApp>>,
        mut load: impl FnMut(&Rc<RefCell<PaneApp>>) -> Result<(), String>,
    ) -> serde_json::Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some((_, data)) = control.poll_with(state, &mut load) {
                return data;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn backup(state: &Rc<RefCell<PaneApp>>, path: &Path) {
        state.borrow().store.export_backup(path).unwrap();
    }
    #[test]
    fn preview_rejects_corrupt_live_and_existing_files_and_freezes_source() {
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        let path = root.path().join("saved.db");
        backup(&s, &path);
        let count = s.borrow().store.change_count();
        let prepared = prepare(
            &s.borrow(),
            &Operation::BackupRestore {
                path: path.display().to_string(),
            },
        )
        .unwrap();
        assert_eq!(s.borrow().store.change_count(), count);
        std::fs::write(&path, b"corrupt").unwrap();
        assert_eq!(
            WorkspaceStore::inspect_backup(&prepared.source.unwrap().path().join("restore.db"))
                .unwrap(),
            2
        );
        assert!(
            prepare(
                &s.borrow(),
                &Operation::BackupRestore {
                    path: path.display().to_string()
                }
            )
            .is_err()
        );
        assert!(
            prepare(
                &s.borrow(),
                &Operation::BackupRestore {
                    path: root.path().join("workspace.db").display().to_string()
                }
            )
            .is_err()
        );
        for path in [
            path,
            root.path().join("WORKSPACE.DB-WAL"),
            root.path().join("config.toml"),
        ] {
            assert!(
                prepare(
                    &s.borrow(),
                    &Operation::BackupExport {
                        path: path.display().to_string(),
                        source: None
                    }
                )
                .is_err()
            );
        }
    }
    #[test]
    fn create_export_and_destination_race_never_overwrite() {
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        let mut c = Control::default();
        let prepared = prepare(&s.borrow(), &Operation::BackupCreate {}).unwrap();
        assert_eq!(
            c.apply(&s, "create", prepared)["operation_status"],
            "pending"
        );
        assert!(recovery::view(&s.borrow()).busy);
        assert!(prepare(&s.borrow(), &Operation::BackupCreate {}).is_err());
        let done = finish(&mut c, &s, reload);
        assert_eq!(done["operation_status"], "completed");
        let source = done["path"].as_str().unwrap();
        let dest = root.path().join("export.db");
        let op = Operation::BackupExport {
            path: dest.display().to_string(),
            source: Some(source.into()),
        };
        let prepared = prepare(&s.borrow(), &op).unwrap();
        c.apply(&s, "export", prepared);
        assert_eq!(finish(&mut c, &s, reload)["operation_status"], "completed");
        assert_eq!(
            WorkspaceStore::backup_file_content(Path::new(source)).unwrap(),
            WorkspaceStore::backup_file_content(&dest).unwrap()
        );
        let dest = root.path().join("race.db");
        let prepared = prepare(
            &s.borrow(),
            &Operation::BackupExport {
                path: dest.display().to_string(),
                source: None,
            },
        )
        .unwrap();
        std::fs::write(&dest, b"do not replace").unwrap();
        c.apply(&s, "race", prepared);
        assert_eq!(finish(&mut c, &s, reload)["operation_status"], "failed");
        assert_eq!(std::fs::read(dest).unwrap(), b"do not replace");
    }
    #[test]
    fn restore_rechecks_concurrent_changes_and_preserves_rollback() {
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        let mut c = Control::default();
        let path = root.path().join("saved.db");
        backup(&s, &path);
        let op = Operation::BackupRestore {
            path: path.display().to_string(),
        };
        let prepared = prepare(&s.borrow(), &op).unwrap();
        c.apply(&s, "restore", prepared);
        s.borrow()
            .store
            .save_preference("sentinel", "newer")
            .unwrap();
        let done = finish(&mut c, &s, |_| panic!("must not reload after conflict"));
        assert_eq!(done["commit_status"], "not_committed");
        assert!(Path::new(done["rollback_path"].as_str().unwrap()).exists());
        assert_eq!(
            s.borrow().store.preference("sentinel").unwrap().as_deref(),
            Some("newer")
        );
        let prepared = prepare(&s.borrow(), &op).unwrap();
        c.apply(&s, "restore2", prepared);
        let done = finish(&mut c, &s, reload);
        assert_eq!(done["operation_status"], "completed");
        assert_eq!(s.borrow().store.preference("sentinel").unwrap(), None);
        assert_eq!(
            WorkspaceStore::read_backup(Path::new(done["rollback_path"].as_str().unwrap()))
                .unwrap()
                .preference("sentinel")
                .unwrap()
                .as_deref(),
            Some("newer")
        );
    }
    #[test]
    fn reload_failure_rolls_back_and_reports_double_failure() {
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        let mut c = Control::default();
        let path = root.path().join("saved.db");
        backup(&s, &path);
        s.borrow()
            .store
            .save_preference("sentinel", "current")
            .unwrap();
        for fail_twice in [false, true] {
            let prepared = prepare(
                &s.borrow(),
                &Operation::BackupRestore {
                    path: path.display().to_string(),
                },
            )
            .unwrap();
            c.apply(&s, "restore", prepared);
            let mut calls = 0;
            let done = finish(&mut c, &s, |state| {
                calls += 1;
                if calls == 1 || fail_twice {
                    Err("injected reload failure".into())
                } else {
                    reload(state)
                }
            });
            assert_eq!(calls, 2);
            assert_eq!(
                done["commit_status"],
                if fail_twice { "unknown" } else { "rolled_back" }
            );
            assert_eq!(
                s.borrow().store.preference("sentinel").unwrap().as_deref(),
                Some("current")
            );
            assert!(Path::new(done["rollback_path"].as_str().unwrap()).exists());
        }
    }
    #[test]
    fn backup_plan_retains_one_job_and_replays_final_receipt() {
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        let mut snapshot = Snapshot::new();
        let request = |command: &str, id: &str, fields: serde_json::Value| {
            let mut v = json!({"protocol_version":1,"request_id":id,"command":command});
            v.as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            serde_json::from_value::<Request>(v).unwrap()
        };
        let context = snapshot
            .respond(
                &mut state.borrow_mut(),
                &request("workspace.get", "context", json!({})),
            )
            .context
            .unwrap();
        let preview = snapshot.respond(&mut state.borrow_mut(),&request("plan.preview","preview",json!({"plan":{"protocol_version":1,"base":context,"operations":[{"op":"backup.create"}]}})));
        assert!(preview.ok, "{preview:?}");
        let apply = request(
            "plan.apply",
            "apply",
            json!({"token":preview.data.unwrap()["plan_token"]}),
        );
        let initial = snapshot.respond(&mut state.borrow_mut(), &apply);
        assert!(initial.ok);
        assert!(snapshot.plans.take_runtime_action(&initial).is_some());
        let prepared = snapshot.plans.take_backup(&initial).unwrap();
        assert!(snapshot.plans.take_backup(&initial).is_none());
        let pending = snapshot.backups.apply(&state, "apply", prepared);
        snapshot.plans.system_result("apply", pending);
        let repeated = snapshot.respond(&mut state.borrow_mut(), &apply);
        assert!(snapshot.plans.take_runtime_action(&repeated).is_none());
        let done = finish(&mut snapshot.backups, &state, reload);
        snapshot.plans.system_result("apply", done.clone());
        assert_eq!(
            snapshot
                .respond(&mut state.borrow_mut(), &apply)
                .data
                .unwrap(),
            done
        );
        assert_eq!(
            query(&state.borrow()).unwrap()["backups"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
