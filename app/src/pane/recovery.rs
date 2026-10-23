//! Configuration backup history and serialized background operations.
use super::*;
use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant, SystemTime},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Policy {
    pub enabled: bool,
    pub minutes: u64,
    pub keep: usize,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: true,
            minutes: 5,
            keep: 10,
        }
    }
}
impl Policy {
    pub fn load(store: &WorkspaceStore) -> Result<Self, String> {
        Self::decode(store.preference("backup_policy").map_err(|e| e.to_string()))
    }
    fn decode(raw: Result<Option<String>, String>) -> Result<Self, String> {
        let raw = raw?;
        let Some(raw) = raw else { return Ok(Self::default()); };
        let parts: Vec<_> = raw.split(',').collect();
        if parts.len() != 3 || !matches!(parts[0], "0" | "1") {
            return Err("Invalid backup policy".into());
        }
        let minutes = parts[1].parse().ok().filter(|v| [5, 15, 30, 60].contains(v));
        let keep = parts[2].parse().ok().filter(|v| [10, 20, 50].contains(v));
        match (minutes, keep) {
            (Some(minutes), Some(keep)) => Ok(Self { enabled: parts[0] == "1", minutes, keep }),
            _ => Err("Invalid backup policy".into()),
        }
    }
    pub fn save(self, store: &WorkspaceStore) -> Result<(), String> {
        store
            .save_preference(
                "backup_policy",
                &format!("{},{},{}", u8::from(self.enabled), self.minutes, self.keep),
            )
            .map_err(|e| e.to_string())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Record {
    pub path: PathBuf,
    pub date: String,
    pub kind: &'static str,
    pub bytes: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct View {
    pub records: Arc<[Record]>,
    pub status: String,
    pub busy: bool,
    pub undo: Option<PathBuf>,
}
struct Completed {
    result: Result<Outcome, String>,
    records: Arc<[Record]>,
}
struct RestoreInput {
    original: PathBuf,
    temporary: tempfile::TempDir,
    count: usize,
    version: String,
    missing: Vec<String>,
}
impl RestoreInput {
    fn path(&self) -> PathBuf {
        self.temporary.path().join("restore.db")
    }
}
enum Outcome {
    Saved(String),
    BackupSaved(String, u64),
    Unchanged(u64),
    Inspect(RestoreInput),
    ReadyRestore(RestoreInput, PathBuf, u64),
}
pub(super) struct Manager {
    pub view: View,
    receiver: Option<mpsc::Receiver<Completed>>,
    observed: u64,
    checked: Option<u64>,
    changed: Instant,
    attempted: Instant,
    initialized: bool,
}
impl Default for Manager {
    fn default() -> Self {
        Self {
            view: View::default(),
            receiver: None,
            observed: 0,
            checked: None,
            changed: Instant::now(),
            attempted: Instant::now() - Duration::from_secs(3600),
            initialized: false,
        }
    }
}
pub(super) fn directory(s: &PaneApp) -> Result<PathBuf, String> {
    let runtime = s.runtime
        .as_ref()
        .ok_or(crate::i18n::text("backup-runtime-state-missing"))?;
    runtime.path.parent()
        .map(|p| p.join("backups"))
        .ok_or_else(|| crate::i18n::text("configuration-directory-path-unavailable").into())
}
fn name(label: &str) -> String {
    let mut time = windows_sys::Win32::Foundation::SYSTEMTIME::default();
    unsafe {
        windows_sys::Win32::System::SystemInformation::GetLocalTime(&raw mut time);
    }
    format!(
        "{label}-{:04}{:02}{:02}-{:02}{:02}{:02}-{:03}.db",
        time.wYear,
        time.wMonth,
        time.wDay,
        time.wHour,
        time.wMinute,
        time.wSecond,
        time.wMilliseconds
    )
}
fn records(directory: &Path) -> Vec<Record> {
    let mut result: Vec<_> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "db") {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = if name.starts_with("auto-") {
                crate::i18n::text("ui-automatic")
            } else if name.starts_with("before-restore-") {
                crate::i18n::text("ui-before-restore")
            } else {
                crate::i18n::text("ui-manual")
            };
            Some((
                metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                Record {
                    path,
                    date: folder::modified_text(metadata.modified().ok()),
                    kind,
                    bytes: metadata.len(),

                },
            ))
        })
        .collect();
    result.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.path.cmp(&a.1.path)));
    result.into_iter().map(|(_, r)| r).collect()
}
fn cleanup(directory: &Path, prefix: &str, keep: usize) -> Option<String> {
    let mut failed = 0;
    for record in records(directory)
        .into_iter()
        .filter(|r| r.path.file_name().is_some_and(|name| name.to_string_lossy().starts_with(prefix)))
        .skip(keep)
    {
        if std::fs::remove_file(record.path).is_err() {
            failed += 1;
        }
    }
    (failed > 0).then(|| crate::i18n::format("ui-old-backups-not-removed", &[("failed", format!("{}", failed))]))
}
fn save_snapshot(
    store: &WorkspaceStore,
    directory: &Path,
    label: &str,
    keep: usize,
) -> Result<(PathBuf, String), String> {
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let mut path = directory.join(name(label));
    // Never overwrite another snapshot, even when the clock moves backwards.
    if path.exists() {
        path = directory.join(format!(
            "{label}-{}.db",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
    }
    store
        .save_preference("backup_manifest_version", env!("CARGO_PKG_VERSION"))
        .map_err(|e| e.to_string())?;
    store.export_backup(&path).map_err(|e| e.to_string())?;
    let warning = match label {
        "auto" => cleanup(directory, "auto-", keep),
        "before-restore" => cleanup(directory, "before-restore-", 5),
        _ => None,
    }
    .unwrap_or_default();
    Ok((path, warning))
}
fn begin(
    state: &Rc<RefCell<PaneApp>>,
    label: &str,
    work: impl FnOnce(PathBuf) -> Result<Outcome, String> + Send + 'static,
) -> Result<(), String> {
    let mut s = state.borrow_mut();
    let directory = directory(&s)?;
    let wake = s.wake.clone();
    let manager = &mut s.runtime.as_mut().ok_or(crate::i18n::text("backup-runtime-state-missing"))?.backup;
    if manager.view.busy {
        return Err(crate::i18n::text("ui-a-backup-task-is-running-please-wait").into());
    }
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("configuration-backup".into())
        .spawn(move || {
            let result = work(directory.clone());
            let _ = tx.send(Completed {
                result,
                records: records(&directory).into(),
            });
            wake.notify();
        })
        .map_err(|e| e.to_string())?;
    manager.receiver = Some(rx);
    manager.view.busy = true;
    manager.view.status = label.into();
    if let Some(settings) = &s.settings {
        unsafe {
            windows_sys::Win32::Graphics::Gdi::InvalidateRect(
                settings.hwnd().cast(),
                std::ptr::null(),
                0,
            );
        }
    }
    Ok(())
}
pub(super) fn view(state: &PaneApp) -> View {
    state
        .runtime
        .as_ref()
        .map(|r| r.backup.view.clone())
        .unwrap_or_default()
}
pub(super) fn maintain(state: &Rc<RefCell<PaneApp>>) {
    let completed = {
        let mut s = state.borrow_mut();
        let Some(runtime) = s.runtime.as_mut() else {
            return;
        };
        runtime
            .backup
            .receiver
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(v) => Some(v),
                Err(mpsc::TryRecvError::Disconnected) => Some(Completed {
                    result: Err(crate::i18n::text("ui-backup-task-ended-unexpectedly-try-again").into()),
                    records: runtime.backup.view.records.clone(),
                }),
                _ => None,
            })
    };
    if let Some(completed) = completed {
        {
            let mut s = state.borrow_mut();
            let m = &mut s.runtime.as_mut().unwrap().backup;
            m.receiver = None;
            m.view.busy = false;
            m.view.records = completed.records;
            if m.view
                .undo
                .as_ref()
                .is_some_and(|path| !m.view.records.iter().any(|r| &r.path == path))
            {
                m.view.undo = None;
            }
        }
        match completed.result {
            Ok(Outcome::Saved(message)) => set_status(state, &message),
            Ok(Outcome::BackupSaved(message, revision)) => {
                state.borrow_mut().runtime.as_mut().unwrap().backup.checked = Some(revision);
                set_status(state, &message);
            }
            Ok(Outcome::Unchanged(revision)) => {
                state.borrow_mut().runtime.as_mut().unwrap().backup.checked = Some(revision);
                set_status(state, crate::i18n::text("ui-configuration-unchanged-no-new-backup-needed"));
            }
            Ok(Outcome::Inspect(input)) => {
                state
                    .borrow_mut()
                    .runtime
                    .as_mut()
                    .unwrap()
                    .backup
                    .view
                    .busy = true;
                let weak = Rc::downgrade(state);
                window::defer_action(move || {
                    if let Some(state) = weak.upgrade() {
                        confirm_restore(&state, input);
                    }
                });
            }
            Ok(Outcome::ReadyRestore(input, rollback, revision)) => {
                state
                    .borrow_mut()
                    .runtime
                    .as_mut()
                    .unwrap()
                    .backup
                    .view
                    .busy = true;
                let weak = Rc::downgrade(state);
                window::defer_action(move || {
                    if let Some(state) = weak.upgrade() {
                        apply_restore(&state, input, rollback, revision);
                    }
                });
            }
            Err(error) => set_status(state, &crate::i18n::format("backup-task-failed", &[("error", format!("{}", error))])),
        }
        return;
    }
    let due = {
        let mut s = state.borrow_mut();
        let policy = match Policy::load(&s.store) {
            Ok(policy) => policy,
            Err(error) => {
                drop(s);
                set_status(state, &format!("Backup policy unavailable: {error}"));
                return;
            }
        };
        let changes = s.store.change_count();
        let Some(r) = s.runtime.as_mut() else {
            return;
        };
        let m = &mut r.backup;
        if changes != m.observed {
            m.observed = changes;
            m.changed = Instant::now();
        }
        if m.view.busy {
            false
        } else if !m.initialized {
            m.initialized = true;
            drop(s);
            let _ = begin(state, crate::i18n::text("ui-loading-backup-history"), |directory| {
                Ok(Outcome::Saved(records(&directory).first().map_or_else(
                    || crate::i18n::text("ui-no-backup-history").into(),
                    |r| crate::i18n::format("ui-last-backup", &[("arg0", format!("{}", r.date))]),
                )))
            });
            return;
        } else {
            policy.enabled
                && m.checked != Some(changes)
                && m.changed.elapsed() >= Duration::from_secs(10)
                && m.attempted.elapsed() >= Duration::from_secs(policy.minutes * 60)
        }
    };
    if due {
        if let Some(r) = state.borrow_mut().runtime.as_mut() {
            r.backup.attempted = Instant::now();
        }
        if let Err(error) = create(state, true) {
            set_status(state, &crate::i18n::format("ui-automatic-backup-failed", &[("error", format!("{}", error))]));
        }
    }
}
pub(super) fn deadline(s: &PaneApp) -> Option<Instant> {
    let manager = &s.runtime.as_ref()?.backup;
    if manager.view.busy {
        return None; // Completion is delivered by Wake.
    }
    let policy = Policy::load(&s.store).ok()?;
    if !manager.initialized {
        return Some(Instant::now());
    }
    (policy.enabled && manager.checked != Some(s.store.change_count())).then(|| {
        (manager.changed + Duration::from_secs(10))
            .max(manager.attempted + Duration::from_secs(policy.minutes * 60))
    })
}
fn set_status(state: &Rc<RefCell<PaneApp>>, message: &str) {
    let mut state = state.borrow_mut();
    if let Some(r) = state.runtime.as_mut() {
        r.backup.view.status = message.into();
    }
    if let Some(settings) = &state.settings {
        unsafe {
            windows_sys::Win32::Graphics::Gdi::InvalidateRect(
                settings.hwnd().cast(),
                std::ptr::null(),
                0,
            );
        }
    }
}
pub(super) fn create(state: &Rc<RefCell<PaneApp>>, automatic: bool) -> Result<(), String> {
    if view(&state.borrow()).busy {
        return Err(crate::i18n::text("ui-a-backup-task-is-running-please-wait").into());
    }
    let (snapshot, policy, revision) = {
        let s = state.borrow();
        (
            s.store.backup_snapshot().map_err(|e| e.to_string())?,
            Policy::load(&s.store)?,
            s.store.change_count(),
        )
    };
    begin(state, crate::i18n::text("ui-creating-backup"), move |directory| {
        if automatic {
            if let Some(previous) = records(&directory).first() {
                if WorkspaceStore::backup_file_content(&previous.path)
                    .ok()
                    .as_ref()
                    == Some(&snapshot.backup_content().map_err(|e| e.to_string())?)
                {
                    return Ok(Outcome::Unchanged(revision));
                }
            }
        }
        let (_, warning) = save_snapshot(
            &snapshot,
            &directory,
            if automatic { "auto" } else { "manual" },
            policy.keep,
        )?;
        Ok(Outcome::BackupSaved(crate::i18n::format("ui-backup-completed", &[("arg0", format!("{}", if automatic { crate::i18n::text("ui-automatic") } else { crate::i18n::text("ui-manual") })), ("warning", format!("{}", warning))]), revision))
    })
}
fn choose(owner: isize, export: bool) -> Result<Option<PathBuf>, String> {
    use windows::Win32::{System::Com::*, UI::Shell::*};
    unsafe {
        let dialog: IFileDialog = if export {
            let d: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| e.to_string())?;
            windows::core::Interface::cast(&d).map_err(|e| e.to_string())?
        } else {
            let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| e.to_string())?;
            windows::core::Interface::cast(&d).map_err(|e| e.to_string())?
        };
        dialog
            .SetOptions(
                FOS_FORCEFILESYSTEM
                    | FOS_PATHMUSTEXIST
                    | FOS_NOCHANGEDIR
                    | if export {
                        FOS_OVERWRITEPROMPT
                    } else {
                        FOS_FILEMUSTEXIST
                    },
            )
            .map_err(|e| e.to_string())?;
        dialog
            .SetTitle(if export {
                windows::core::PCWSTR(crate::i18n::wide("ui-export-luciddesk-configuration"))
            } else {
                windows::core::PCWSTR(crate::i18n::wide("ui-restore-luciddesk-configuration"))
            })
            .map_err(|e| e.to_string())?;
        dialog
            .SetFileTypes(&[Common::COMDLG_FILTERSPEC {
                pszName: windows::core::PCWSTR(crate::i18n::wide("ui-luciddesk-configuration-db")),
                pszSpec: windows::core::w!("*.db"),
            }])
            .map_err(|e| e.to_string())?;
        dialog
            .SetDefaultExtension(windows::core::w!("db"))
            .map_err(|e| e.to_string())?;
        if export {
            dialog
                .SetFileName(&windows::core::HSTRING::from(name("LucidDesk")))
                .map_err(|e| e.to_string())?;
        }
        if let Err(e) = dialog.Show(Some(windows::Win32::Foundation::HWND(owner as _))) {
            if e.code().0 as u32 == 0x800704c7 {
                return Ok(None);
            }
            return Err(e.to_string());
        }
        let item = dialog.GetResult().map_err(|e| e.to_string())?;
        let raw = item
            .GetDisplayName(SIGDN_FILESYSPATH)
            .map_err(|e| e.to_string())?;
        use std::os::windows::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_wide(raw.as_wide()));
        CoTaskMemFree(Some(raw.0.cast()));
        Ok(Some(path))
    }
}

fn confirm_restore(state: &Rc<RefCell<PaneApp>>, input: RestoreInput) {
    let owner = state
        .borrow()
        .settings
        .as_ref()
        .map_or(0, |w| w.hwnd() as isize);
    let missing = if input.missing.is_empty() {
        String::new()
    } else {
        crate::i18n::format("ui-nthese-folders-are-currently-unavailable-their-mappings-will-be-kept", &[("arg0", format!("{}", input.missing.join("\n")))])
    };
    let text = crate::i18n::format("ui-integrity-and-compatibility-checks-passed-npanels-app-version-n-nres", &[("arg0", format!("{}", input.count)), ("arg1", format!("{}", input.version)), ("arg2", format!("{}", input.original.display())), ("missing", format!("{}", missing))]);
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    if unsafe {
        MessageBoxW(
            owner as _,
            wide.as_ptr(),
            crate::i18n::wide("ui-restore-backup"),
            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
        )
    } != IDYES
    {
        state
            .borrow_mut()
            .runtime
            .as_mut()
            .unwrap()
            .backup
            .view
            .busy = false;
        set_status(state, crate::i18n::text("ui-restore-canceled"));
        return;
    }
    state
        .borrow_mut()
        .runtime
        .as_mut()
        .unwrap()
        .backup
        .view
        .busy = false;
    let result = (|| {
        let (snapshot, revision) = {
            let s = state.borrow();
            (s.store.backup_snapshot().map_err(|e| e.to_string())?, s.store.change_count())
        };
        begin(state, crate::i18n::text("ui-backing-up-current-state"), move |directory| {
            let (rollback, _) = save_snapshot(&snapshot, &directory, "before-restore", 5)?;
            Ok(Outcome::ReadyRestore(input, rollback, revision))
        })
    })();
    if let Err(e) = result {
        set_status(state, &crate::i18n::format("ui-restore-not-performed", &[("e", format!("{}", e))]));
    }
}
fn apply_restore(state: &Rc<RefCell<PaneApp>>, input: RestoreInput, rollback: PathBuf, revision: u64) {
    if state.borrow().store.change_count() != revision || hybrid::workspace_save_pending(&state.borrow()) {
        state.borrow_mut().runtime.as_mut().unwrap().backup.view.busy = false;
        set_status(state, crate::i18n::text("backup-restore-state-changed"));
        return;
    }
    apply_restore_with(state, input, rollback, runtime::reload);
}
fn apply_restore_with(
    state: &Rc<RefCell<PaneApp>>,
    input: RestoreInput,
    rollback: PathBuf,
    mut reload: impl FnMut(&Rc<RefCell<PaneApp>>) -> Result<(), String>,
) {
    let result = state
        .borrow_mut()
        .store
        .restore_backup(&input.path())
        .map_err(|e| e.to_string());
    if let Err(error) = result {
        // Storage validates before replacement and rolls back failed writes.
        // Do not overwrite newer live data with an older rollback snapshot.
        state.borrow_mut().runtime.as_mut().unwrap().backup.view.busy = false;
        set_status(state, &crate::i18n::format("ui-restore-not-performed", &[("e", error)]));
        return;
    }
    let result = reload(state);
    state
        .borrow_mut()
        .runtime
        .as_mut()
        .unwrap()
        .backup
        .view
        .busy = false;
    match result {
        Ok(()) => {
            state
                .borrow_mut()
                .runtime
                .as_mut()
                .unwrap()
                .backup
                .view
                .undo = Some(rollback);
            set_status(state, crate::i18n::text("ui-restored-you-can-undo-this-restore"));
        }
        Err(error) => {
            let rollback_result = state
                .borrow_mut()
                .store
                .restore_backup(&rollback)
                .map_err(|e| e.to_string());
            let rollback_result = rollback_result.and_then(|_| reload(state));
            set_status(
                state,
                &match rollback_result {
                    Ok(()) => crate::i18n::format("ui-restore-failed-rolled-back", &[("error", format!("{}", error))]),
                    Err(e) => crate::i18n::format("ui-restore-failed-rollback-failed-previous-backup", &[("error", format!("{}", error)), ("e", format!("{}", e)), ("arg0", format!("{}", rollback.display()))]),
                },
            );
        }
    }
}
pub(super) fn request(state: &Rc<RefCell<PaneApp>>, event: &Event) {
    let event = event.clone();
    let weak = Rc::downgrade(state);
    window::defer_action(move || {
        if let Some(state) = weak.upgrade() {
            if let Err(e) = execute(&state, &event) {
                set_status(&state, &crate::i18n::format("configuration-operation-failed", &[("e", format!("{}", e))]));
                window::error(&e);
            }
        }
    });
}
fn execute(state: &Rc<RefCell<PaneApp>>, event: &Event) -> Result<(), String> {
    if view(&state.borrow()).busy {
        return Err(crate::i18n::text("ui-a-backup-task-is-running-please-wait").into());
    }
    let owner = state
        .borrow()
        .settings
        .as_ref()
        .map_or(0, |w| w.hwnd() as isize);
    if matches!(event, Event::ReloadConfig) {
        state
            .borrow()
            .store
            .reload_config()
            .map_err(|e| e.to_string())?;
        return runtime::reload(state);
    }
    if matches!(event, Event::OpenConfigDirectory | Event::OpenBackups) {
        let path = if matches!(event, Event::OpenBackups) {
            directory(&state.borrow())?
        } else {
            state
                .borrow()
                .store
                .config_path()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .ok_or(crate::i18n::text("configuration-directory-path-unavailable"))?
        };
        std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        return open_shell_identity(owner, &folder::identity(path)).map_err(|e| e.to_string());
    }
    match event {
        Event::CreateBackup => create(state, false),
        Event::RestoreBackup | Event::RestoreBackupPath(_) => {
            let path = if let Event::RestoreBackupPath(path) = event {
                path.clone()
            } else {
                let Some(path) = choose(owner, false)? else {
                    return Ok(());
                };
                path
            };
            begin(state, crate::i18n::text("ui-validating-backup"), move |_| {
                let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
                let staged = temporary.path().join("restore.db");
                std::fs::copy(&path, &staged).map_err(|e| e.to_string())?;
                let count = WorkspaceStore::inspect_backup(&staged).map_err(|e| e.to_string())?;
                let version = WorkspaceStore::read_backup(&staged)
                    .map_err(|e| e.to_string())?
                    .preference("backup_manifest_version")
                    .map_err(|e| e.to_string())?
                    .unwrap_or_else(|| crate::i18n::text("ui-legacy-backup").into());
                let workspace = WorkspaceStore::read_backup(&staged)
                    .map_err(|e| e.to_string())?
                    .load_workspace()
                    .map_err(|e| e.to_string())?;
                let missing = workspace
                    .panels()
                    .iter()
                    .filter_map(|p| p.folder())
                    .filter(|p| !p.is_dir())
                    .take(5)
                    .map(|p| p.display().to_string())
                    .collect();
                Ok(Outcome::Inspect(RestoreInput {
                    original: path,
                    temporary,
                    count,
                    version,
                    missing,
                }))
            })
        }
        Event::DeleteBackup(path) => {
            let root = directory(&state.borrow())?
                .canonicalize()
                .map_err(|e| e.to_string())?;
            let path = path.canonicalize().map_err(|e| e.to_string())?;
            if path.parent() != Some(root.as_path()) || path.extension().is_none_or(|e| e != "db") {
                return Err(crate::i18n::text("ui-only-records-in-the-backup-folder-can-be-deleted").into());
            }
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            if unsafe {
                MessageBoxW(
                    owner as _,
                    crate::i18n::wide("ui-delete-this-backup-your-current-configuration-will-not-change"),
                    crate::i18n::wide("ui-delete-backup"),
                    MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
                )
            } != IDYES
            {
                return Ok(());
            }
            begin(state, crate::i18n::text("ui-deleting"), move |_| {
                std::fs::remove_file(path).map_err(|e| e.to_string())?;
                Ok(Outcome::Saved(crate::i18n::text("ui-backup-deleted").into()))
            })
        }
        Event::ExportBackup | Event::ExportBackupPath(_) => {
            let Some(path) = choose(owner, true)? else {
                return Ok(());
            };
            let s = state.borrow();
            if reserved_export_path(&s, &path) {
                return Err(crate::i18n::text("backup-export-path-reserved").into());
            }
            let snapshot = if let Event::ExportBackupPath(source) = event {
                WorkspaceStore::read_backup(source).map_err(|e| e.to_string())?
            } else {
                s.store.backup_snapshot().map_err(|e| e.to_string())?
            };
            drop(s);
            begin(state, crate::i18n::text("ui-exporting"), move |_| {
                snapshot
                    .export_backup_replace(&path)
                    .map_err(|e| e.to_string())?;
                Ok(Outcome::Saved(crate::i18n::format("ui-exported", &[("arg0", format!("{}", path.display()))])))
            })
        }
        _ => Ok(()),
    }
}

fn reserved_export_path(state: &PaneApp, path: &Path) -> bool {
    let name = path.file_name().map(|name| name.to_string_lossy().to_ascii_lowercase());
    if name.as_deref().is_some_and(|name| ["workspace.db", "workspace.db-wal", "workspace.db-shm", "config.toml"].contains(&name)) {
        return true;
    }
    let destination = path.canonicalize().ok();
    state.runtime.as_ref().is_some_and(|runtime| runtime.path == path
        || destination.as_ref().is_some_and(|target| runtime.path.canonicalize().ok().as_ref() == Some(target)))
        || state.store.config_path().is_some_and(|config| config == path
            || destination.as_ref().is_some_and(|target| config.canonicalize().ok().as_ref() == Some(target)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn backup_task_reports_missing_runtime_before_resolving_directory() {
        let app = Rc::new(RefCell::new(super::super::tests::test_state()));
        assert!(app.borrow().runtime.is_none());
        let missing = crate::i18n::text("backup-runtime-state-missing");
        assert_eq!(directory(&app.borrow()).unwrap_err(), missing);
        assert_eq!(begin(&app, "unused", |_| panic!("must not start a worker without runtime state")).unwrap_err(), missing);

        // A missing parent is a path derivation error, not a permission error.
        app.borrow_mut().runtime = Some(runtime::State::new(PathBuf::from(r"C:\")));
        let missing_path = crate::i18n::text("configuration-directory-path-unavailable");
        assert_eq!(begin(&app, "unused", |_| panic!("must not start a worker without a directory path")).unwrap_err(), missing_path);
        let app = app.borrow();
        let manager = &app.runtime.as_ref().unwrap().backup;
        assert!(!manager.view.busy);
        assert!(manager.receiver.is_none());
    }

    #[test]
    fn backup_directory_resolution_does_not_claim_to_check_access() {
        let root = tempfile::tempdir().unwrap();
        let absent = root.path().join("not-created");
        let mut app = super::super::tests::test_state();
        app.runtime = Some(runtime::State::new(absent.join("workspace.db")));
        assert_eq!(directory(&app).unwrap(), absent.join("backups"));
        assert!(!absent.exists());
    }

    #[test]
    fn backup_defaults_only_apply_to_missing_preferences() {
        assert_eq!(super::Policy::decode(Ok(None)).unwrap(), super::Policy::default());
        assert!(super::Policy::decode(Err("database read failed".into())).is_err());
        for raw in ["", "bad", "1,5,1", "x,5,10"] {
            assert!(super::Policy::decode(Ok(Some(raw.into()))).is_err());
        }
        let policy = super::Policy::decode(Ok(Some("0,30,50".into()))).unwrap();
        assert!(!policy.enabled);
        assert_eq!(policy.keep, 50);
    }

    #[test]
    fn completion_reaches_runtime_without_dispatching_timers() {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        state.borrow_mut().workspace = Workspace::default();
        Policy { enabled: false, ..Default::default() }.save(&state.borrow().store).unwrap();
        state.borrow_mut().runtime.as_mut().unwrap().backup.initialized = true;
        let supervisor = runtime::supervisor(&state).unwrap();
        begin(&state, "waiting", |_| Ok(Outcome::Saved("event delivered".into()))).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while view(&state.borrow()).busy && Instant::now() < deadline {
            unsafe {
                let mut message = MSG::default();
                while PeekMessageW(&raw mut message, supervisor.hwnd().cast(), wake::READY, wake::READY, PM_REMOVE) != 0 {
                    DispatchMessageW(&message);
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!view(&state.borrow()).busy);
        assert_eq!(view(&state.borrow()).status, "event delivered");
    }

    #[test]
    fn backup_views_share_history_and_keep_previous_snapshot_stable() {
        let mut current = super::View {
            records: (0..1000).map(|index| super::Record {
                path: std::path::PathBuf::from(format!("backup-{index}.db")),
                date: "2026-09-14".into(), kind: "manual", bytes: 1024,
            }).collect(), ..Default::default()
        };
        let previous = current.clone();
        assert!(std::sync::Arc::ptr_eq(&current.records, &previous.records));
        current.records = Default::default();
        assert_eq!(previous.records.len(), 1000);
    }

    use super::*;
    fn staged_input(bytes: &[u8]) -> RestoreInput {
        let temporary = tempfile::tempdir().unwrap();
        let original = temporary.path().join("restore.db");
        std::fs::write(&original, bytes).unwrap();
        RestoreInput { original, temporary, count: 0, version: "test".into(), missing: vec![] }
    }

    #[test]
    fn invalid_restore_never_replaces_newer_configuration_with_rollback() {
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        state.borrow().store.save_preference("sentinel", "old").unwrap();
        let rollback = root.path().join("rollback.db");
        state.borrow().store.export_backup(&rollback).unwrap();
        state.borrow().store.save_preference("sentinel", "new").unwrap();
        let before = state.borrow().store.backup_snapshot().unwrap().backup_content().unwrap();
        apply_restore_with(&state, staged_input(b"corrupt"), rollback, |_| panic!("invalid backup must not reload or roll back"));
        assert_eq!(state.borrow().store.backup_snapshot().unwrap().backup_content().unwrap(), before);
        assert!(!view(&state.borrow()).busy);
    }

    #[test]
    fn changes_after_rollback_snapshot_cancel_restore_without_touching_live_data() {
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        let rollback = root.path().join("rollback.db");
        state.borrow().store.export_backup(&rollback).unwrap();
        let revision = state.borrow().store.change_count();
        let input = staged_input(&std::fs::read(&rollback).unwrap());
        state.borrow().store.save_preference("sentinel", "new").unwrap();
        state.borrow_mut().runtime.as_mut().unwrap().backup.view.busy = true;
        apply_restore(&state, input, rollback, revision);
        assert_eq!(state.borrow().store.preference("sentinel").unwrap().as_deref(), Some("new"));
        assert_eq!(view(&state.borrow()).status, crate::i18n::text("backup-restore-state-changed"));
        assert!(!view(&state.borrow()).busy);
    }

    #[test]
    fn export_rejects_case_variants_and_live_database_sidecars() {
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        for name in ["WORKSPACE.DB", "Config.Toml", "workspace.db-wal", "WORKSPACE.DB-SHM"] {
            assert!(reserved_export_path(&state.borrow(), &root.path().join(name)));
        }
        assert!(!reserved_export_path(&state.borrow(), &root.path().join("export.db")));
    }

    fn state(root: &Path) -> Rc<RefCell<PaneApp>> {
        let mut state = super::super::tests::test_state();
        let path = root.join("workspace.db");
        state.store = WorkspaceStore::open(&path).unwrap();
        state.store.save_workspace(&state.workspace).unwrap();
        state.runtime = Some(runtime::State::new(path));
        Rc::new(RefCell::new(state))
    }
    fn finish(state: &Rc<RefCell<PaneApp>>) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while view(&state.borrow()).busy {
            assert!(Instant::now() < deadline, "backup worker timed out");
            maintain(state);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn unchanged_scheduler_sleeps_and_changes_during_backup_are_not_lost() {
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        state.borrow_mut().runtime.as_mut().unwrap().backup.initialized = true;
        create(&state, true).unwrap();
        // A later change must not be marked as included in the running snapshot.
        state.borrow().store.save_preference("during_backup", "new").unwrap();
        finish(&state);
        assert!(deadline(&state.borrow()).is_some());
        create(&state, true).unwrap();
        finish(&state);
        assert!(deadline(&state.borrow()).is_none());
        let count = view(&state.borrow()).records.len();
        maintain(&state);
        assert!(!view(&state.borrow()).busy);
        assert_eq!(view(&state.borrow()).records.len(), count);
        state.borrow().store.save_preference("after_backup", "new").unwrap();
        assert!(deadline(&state.borrow()).is_some());
    }

    #[test]
    fn worker_serializes_jobs_deduplicates_content_and_rejects_corrupt_restore() {
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        create(&state, true).unwrap();
        assert!(create(&state, false).is_err());
        finish(&state);
        assert_eq!(view(&state.borrow()).records.len(), 1);
        create(&state, true).unwrap();
        finish(&state);
        assert_eq!(view(&state.borrow()).records.len(), 1);
        assert_eq!(view(&state.borrow()).status, crate::i18n::text("ui-configuration-unchanged-no-new-backup-needed"));
        state
            .borrow()
            .store
            .save_preference("test_changed", "yes")
            .unwrap();
        create(&state, true).unwrap();
        finish(&state);
        assert_eq!(view(&state.borrow()).records.len(), 2);
        let corrupt = root.path().join("corrupt.db");
        std::fs::write(&corrupt, b"not sqlite").unwrap();
        execute(&state, &Event::RestoreBackupPath(corrupt)).unwrap();
        finish(&state);
        assert!(view(&state.borrow()).status.contains("失败"));
        assert_eq!(
            state
                .borrow()
                .store
                .preference("test_changed")
                .unwrap()
                .as_deref(),
            Some("yes")
        );
        let policy = Policy { enabled: false, minutes: 30, keep: 20 };
        policy.save(&state.borrow().store).unwrap();
        assert_eq!(Policy::load(&state.borrow().store).unwrap(), policy);
    }
    #[test]
    fn retention_preserves_manual_snapshots_and_reports_cleanup_failure_separately() {
        let root = tempfile::tempdir().unwrap();
        let store = WorkspaceStore::open_in_memory().unwrap();
        for _ in 0..12 {
            save_snapshot(&store, root.path(), "auto", 10).unwrap();
        }
        for _ in 0..7 {
            save_snapshot(&store, root.path(), "before-restore", 5).unwrap();
        }
        for _ in 0..3 {
            save_snapshot(&store, root.path(), "manual", 10).unwrap();
        }
        let rows = records(root.path());
        assert_eq!(rows.iter().filter(|r| r.kind == "自动").count(), 10);
        assert_eq!(rows.iter().filter(|r| r.kind == "恢复前").count(), 5);
        assert_eq!(rows.iter().filter(|r| r.kind == "手动").count(), 3);
        let path = rows.iter().find(|r| r.kind == "自动").unwrap().path.clone();
        use std::os::windows::fs::OpenOptionsExt;
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        assert!(
            cleanup(root.path(), "auto-", 0)
                .unwrap()
                .contains("清理失败")
        );
        drop(held);
    }
    #[test]
    fn failed_reload_restores_previous_database_and_configuration() {
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        state
            .borrow()
            .store
            .save_preference("search_enabled", "0")
            .unwrap();
        let rollback = root.path().join("rollback.db");
        state.borrow().store.export_backup(&rollback).unwrap();
        state
            .borrow()
            .store
            .save_preference("search_enabled", "1")
            .unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let staged = temporary.path().join("restore.db");
        state.borrow().store.export_backup(&staged).unwrap();
        state.borrow_mut().store.restore_backup(&rollback).unwrap();
        let input = RestoreInput {
            original: staged,
            temporary,
            count: 3,
            version: "test".into(),
            missing: vec![],
        };
        let mut attempts = 0;
        apply_restore_with(&state, input, rollback, |state| {
            assert!(state.try_borrow_mut().is_ok(), "reload must not retain a store borrow");
            attempts += 1;
            if attempts == 1 {
                Err("injected reload failure".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(attempts, 2);
        assert_eq!(
            state
                .borrow()
                .store
                .preference("search_enabled")
                .unwrap()
                .as_deref(),
            Some("0")
        );
        assert!(view(&state.borrow()).status.contains("已还原为恢复前的配置"));
    }
}
