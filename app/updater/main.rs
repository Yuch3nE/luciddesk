#![windows_subsystem = "windows"]

#[path = "../src/updates/distribution.rs"]
mod distribution;
#[path = "../src/updates/handoff.rs"]
mod handoff;
#[path = "../src/updates/http.rs"]
mod http;
mod package;
mod platform;
#[path = "../src/updates/probe.rs"]
mod probe;
mod release;
#[path = "../src/updates/signed.rs"]
mod signed;

use semver::Version;
use handoff::{Appearance, Stage};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};
use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDOK, MB_ICONERROR, MB_ICONINFORMATION, MB_OKCANCEL,
};
type UpdateError = Box<dyn std::error::Error>;
type Result<T> = std::result::Result<T, UpdateError>;

#[derive(Debug)]
enum ApplyFailure {
    Restartable(UpdateError),
    RecoveryRequired(UpdateError),
}

type ApplyResult = std::result::Result<(), ApplyFailure>;

fn main() {
    let mut failed = "The update failed.".to_owned();
    if let Err(error) = run(&mut failed) {
        platform::message(&format!("{failed}\n\n{error}"), MB_ICONERROR);
        std::process::exit(1);
    }
}

fn run(failed: &mut String) -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        println!("updater {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let [pid, version, prompt, appearance @ ..] = args.as_slice() else {
        return Err("Start the updater from Settings → About in LucidDesk.".into());
    };
    let mut progress = Progress(None);
    if let [appearance] = appearance {
        let json = appearance.to_str().ok_or("Invalid dialog settings")?;
        let appearance = match serde_json::from_str::<Appearance>(json) {
            Ok(appearance) => {
                progress = Progress::staged();
                appearance
            }
            Err(_) => {
                let (confirm, cancel, copy, dark) = serde_json::from_str(json)?;
                Appearance { confirm, cancel, copy, dark, failed: failed.clone() }
            }
        };
        *failed = appearance.failed;
        luciddesk_window::set_message_labels(&appearance.confirm, &appearance.cancel, &appearance.copy);
        luciddesk_window::set_message_theme(Some(appearance.dark));
    } else if !appearance.is_empty() {
        return Err("Unexpected updater arguments".into());
    }
    let original = platform::Original::open(pid.to_str().ok_or("Invalid PID")?.parse()?)?;
    let version = version.to_str().ok_or("Invalid version")?;
    let incoming = Version::parse(version)?;
    let current = env!("CARGO_PKG_VERSION");
    if !incoming.pre.is_empty()
        || !incoming.build.is_empty()
        || incoming <= Version::parse(current)?
    {
        return Err("Invalid upgrade version".into());
    }
    let root = original
        .exe
        .parent()
        .ok_or("Missing installation directory")?;
    let update = PreparedUpdate::prepare(root, version, current, &progress);
    if progress.canceled() { return Ok(()); }
    let update = update?;
    if !progress.confirm(|| {
        platform::message(&prompt.to_string_lossy(), MB_OKCANCEL | MB_ICONINFORMATION) == IDOK
    })? {
        return Ok(());
    }
    progress.show(Stage::Applying);
    update.install(root, &original)
}

struct Progress(Option<PathBuf>);

impl Progress {
    fn staged() -> Self {
        let directory = env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
        let cache = env::temp_dir().join("LucidDesk-Updaters").canonicalize().ok();
        Self(directory.filter(|directory| {
            cache.is_some()
                && directory.parent().and_then(|p| p.canonicalize().ok()) == cache
                && directory.file_name().is_some_and(|name| name.to_string_lossy().starts_with("run-"))
                && distribution::no_links(directory).is_ok()
        }))
    }

    fn show(&self, stage: Stage) {
        let Some(directory) = &self.0 else { return; };
        if let Err(error) = publish_stage(directory, stage) && !matches!(stage, Stage::Downloading(_)) {
            let warning = format!("Could not publish {stage:?}: {error}");
            let recorded = OpenOptions::new().create(true).append(true)
                .open(directory.join(handoff::STATUS_WARNING))
                .and_then(|mut file| writeln!(file, "{warning}"));
            if recorded.is_err() {
                luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Warn, "updates.progress", &warning);
            }
        }
    }

    fn canceled(&self) -> bool {
        self.0.as_ref().is_some_and(|directory| directory.join(handoff::DECISION).exists())
    }

    fn confirm(&self, prompt: impl FnOnce() -> bool) -> std::io::Result<bool> {
        if let Some(directory) = &self.0 && !handoff::claim_decision(directory)? {
            return Ok(false);
        }
        self.show(Stage::Confirming);
        Ok(prompt())
    }
}

fn publish_stage(directory: &Path, stage: Stage) -> io::Result<()> {
    let bytes = serde_json::to_vec(&stage)?;
    let next = directory.join(handoff::NEXT_STATUS);
    let target = directory.join(handoff::STATUS);
    let publish = || fs::write(&next, &bytes).and_then(|()| fs::rename(&next, &target));
    let mut result = publish();
    if !matches!(stage, Stage::Downloading(_)) {
        for _ in 0..3 {
            if !result.as_ref().is_err_and(|error| {
                error.raw_os_error().is_some_and(|code| {
                    matches!(code as u32, ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)
                })
            }) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            result = publish();
        }
    }
    result
}

enum PreparedPayload {
    Zip(Vec<package::StagedFile>),
    Inno { setup: PathBuf, scope: &'static str },
}

struct PreparedUpdate {
    // Drop the staged file and archive handles before removing the working directory.
    payload: PreparedPayload,
    verified_archive: File,
    work: tempfile::TempDir,
    distribution: distribution::Distribution,
    build: package::Build,
}

impl PreparedUpdate {
    fn prepare(root: &Path, version: &str, current: &str, progress: &Progress) -> Result<Self> {
        check_pending_recovery(root)?;
        let build = package::build(root)?;
        let distribution = distribution::Distribution::detect(root)
            .ok_or("Use the original installer to update this installation")?;
        let portable = distribution == distribution::Distribution::Portable;
        if build.version != current
            || build.architecture != "windows-x64"
            || build.portable != portable
        {
            return Err("Unsupported distribution or updater version".into());
        }

        let (parent, archive_name) = match distribution {
            distribution::Distribution::Inno(_) => (env::temp_dir(), "setup.exe"),
            _ => (root.to_path_buf(), "update.zip"),
        };
        distribution::no_links(&parent)?;
        let work = tempfile::Builder::new()
            .prefix(distribution::UPDATE_WORK_PREFIX)
            .tempdir_in(parent)?;
        let archive = work.path().join(archive_name);
        let canceled = || progress.canceled();
        let mut verified_archive = release::fetch(version, distribution.kind(), &archive, &canceled,
            |stage| progress.show(stage))?;
        let payload = match distribution {
            distribution::Distribution::Inno(scope) => PreparedPayload::Inno {
                setup: archive,
                scope,
            },
            _ => {
                progress.show(Stage::Extracting);
                let files = package::extract(
                    &mut verified_archive,
                    &work.path().join("new"),
                    version,
                    portable,
                    &canceled,
                )?;
                package::check_targets(
                    root,
                    files.iter().map(|file| file.relative.as_str()),
                    &build,
                )?;
                PreparedPayload::Zip(files)
            }
        };

        Ok(Self {
            payload,
            verified_archive,
            work,
            distribution,
            build,
        })
    }

    fn install(self, root: &Path, original: &platform::Original) -> Result<()> {
        let install_gate = platform::installation_gate(root)?;
        let session_gate = platform::gate(r"Local\LucidDesk.UpdateApplying")?;
        check_pending_recovery(root)?;
        // Another session may have completed an update while we waited for confirmation.
        if package::build(root)? != self.build
            || distribution::Distribution::detect(root) != Some(self.distribution)
        {
            return Err(
                "The installation changed while preparing the update. Check for updates again."
                    .into(),
            );
        }
        original.close()?;

        let Self {
            payload,
            verified_archive,
            work,
            build,
            ..
        } = self;
        let outcome = match payload {
            PreparedPayload::Inno { setup, scope } => {
                platform::setup(&setup, root, scope).map_err(ApplyFailure::Restartable)
            }
            PreparedPayload::Zip(files) => apply_zip(root, work.path(), &files, &build),
        };
        drop(verified_archive);

        let result = match outcome {
            Ok(()) => Ok(()),
            Err(ApplyFailure::Restartable(error)) => Err(error),
            Err(ApplyFailure::RecoveryRequired(error)) => {
                let _recovery = work.keep();
                return Err(error);
            }
        };
        drop(work);
        drop(session_gate);
        drop(install_gate);
        let restart = Command::new(&original.exe).spawn();
        result?;
        restart?;
        Ok(())
    }
}

fn check_pending_recovery(root: &Path) -> Result<()> {
    if let Some(work) = distribution::interrupted_update(root)? {
        return Err(format!(
            "Recover the interrupted update in {} before retrying",
            work.display()
        )
        .into());
    }
    Ok(())
}

fn apply_zip(
    root: &Path,
    work: &Path,
    files: &[package::StagedFile],
    build: &package::Build,
) -> ApplyResult {
    let _instance =
        platform::gate(r"Local\LucidDesk.DesktopSession").map_err(ApplyFailure::Restartable)?;
    match probe::wait() {
        Ok(true) => package::replace(root, work, files, build),
        Ok(false) => Err(ApplyFailure::Restartable(
            "Explorer has not released the desktop component".into(),
        )),
        Err(error) => Err(ApplyFailure::Restartable(
            format!("Could not inspect the Explorer desktop component: {error}").into(),
        )),
    }
}


#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::fs;

    #[test]
    fn cancellation_and_confirmation_are_arbitrated_before_showing_the_dialog() {
        let work = tempfile::tempdir().unwrap();
        let progress = Progress(Some(work.path().to_path_buf()));
        progress.show(Stage::Downloading(10));
        progress.show(Stage::Verifying);
        let status = fs::read(work.path().join(handoff::STATUS)).unwrap();
        assert_eq!(serde_json::from_slice::<Stage>(&status).unwrap(), Stage::Verifying);
        assert!(!progress.canceled());
        assert!(progress.confirm(|| {
            let status = fs::read(work.path().join(handoff::STATUS)).unwrap();
            assert_eq!(serde_json::from_slice::<Stage>(&status).unwrap(), Stage::Confirming);
            assert!(!handoff::claim_decision(work.path()).unwrap(), "a late Settings click lost the claim");
            true
        }).unwrap());

        let work = tempfile::tempdir().unwrap();
        let progress = Progress(Some(work.path().to_path_buf()));
        assert!(handoff::claim_decision(work.path()).unwrap());
        assert!(progress.canceled());
        assert!(!progress.confirm(|| panic!("a cancel that won must prevent the dialog")).unwrap());
    }

    #[test]
    fn a_blocked_milestone_rename_leaves_the_old_status_and_records_a_warning() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

        let work = tempfile::tempdir().unwrap();
        let progress = Progress(Some(work.path().to_path_buf()));
        let status = work.path().join(handoff::STATUS);
        let read_stage = || serde_json::from_slice::<Stage>(&fs::read(&status).unwrap()).unwrap();
        progress.show(Stage::Extracting);
        let blocker = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&status).unwrap();
        progress.show(Stage::Confirming);
        assert_eq!(read_stage(), Stage::Extracting);
        let warning = fs::read_to_string(work.path().join(handoff::STATUS_WARNING)).unwrap();
        assert!(warning.contains("Confirming"));
        drop(blocker);

        progress.show(Stage::Applying);
        assert_eq!(read_stage(), Stage::Applying);
    }

    #[test]
    #[ignore = "Installs two isolated Inno fixtures through the native updater launcher"]
    fn real_installer_upgrade_preserves_files() {
        let root = std::path::PathBuf::from(env::var_os("LUCIDDESK_TEST_INSTALL_DIR").unwrap());
        for (index, name) in ["LUCIDDESK_TEST_SETUP_OLD", "LUCIDDESK_TEST_SETUP_NEW"]
            .iter()
            .enumerate()
        {
            let setup = std::path::PathBuf::from(env::var_os(name).unwrap());
            platform::setup(&setup, &root, "/CURRENTUSER").unwrap();
            assert!(root.join("installed").is_file());
            assert!(root.join("updater.exe").is_file());
            if index == 0 {
                fs::create_dir_all(root.join("data")).unwrap();
                fs::write(root.join("data/user-note.txt"), b"preserve").unwrap();
                fs::write(root.join("updater.exe"), b"old updater fixture").unwrap();
            } else {
                assert_eq!(
                    fs::read(root.join("data/user-note.txt")).unwrap(),
                    b"preserve"
                );
                assert!(fs::metadata(root.join("updater.exe")).unwrap().len() > 10000);
            }
        }
    }
    struct Running(std::process::Child);
    impl Drop for Running {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                if let Ok(original) = platform::Original::open(self.0.id()) {
                    let _ = original.close();
                }
                if self.0.try_wait().ok().flatten().is_none() {
                    let _ = self.0.kill();
                }
                let _ = self.0.wait();
            }
        }
    }
    #[test]
    #[ignore = "Launches an isolated GUI from LUCIDDESK_TEST_ZIP to exercise native process handoff"]
    fn real_zip_upgrade_preserves_workspace() {
        let available = platform::gate(r"Local\LucidDesk.DesktopSession")
            .expect("Exit the existing LucidDesk before running this isolated lifecycle test");
        drop(available);
        let zip = env::var_os("LUCIDDESK_TEST_ZIP").expect("Provide a newly built ZIP");
        let sandbox = tempfile::Builder::new()
            .prefix("native-lifecycle-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../target"))
            .unwrap();
        let root = sandbox.path().join("app");
        let version = env!("CARGO_PKG_VERSION");
        let portable = env::var("LUCIDDESK_TEST_PORTABLE").unwrap() == "true";
        let mut archive = fs::File::open(&zip).unwrap();
        package::extract(&mut archive, &root, version, portable, &|| false).unwrap();
        let data = if portable {
            root.join("data")
        } else {
            sandbox.path().join("userdata")
        };
        let mut old = Running(
            Command::new(root.join("luciddesk.exe"))
                .arg("--title")
                .arg("Updater lifecycle fixture")
                .env("LUCIDDESK_DATA_DIR", &data)
                .spawn()
                .unwrap(),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !data.join("workspace.db").exists() {
            let status = old.0.try_wait().unwrap();
            assert!(
                status.is_none(),
                "Fixture exited during startup: {status:?}"
            );
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        // Startup may create the database before the tray window is ready.
        std::thread::sleep(std::time::Duration::from_secs(2));
        let original = platform::Original::open(old.0.id()).unwrap();
        let gate = platform::gate(r"Local\LucidDesk.UpdateApplying").unwrap();
        original.close().unwrap();
        old.0.wait().unwrap();
        let before_config = fs::read(data.join("config.toml")).unwrap();
        let before_database = fs::read(data.join("workspace.db")).unwrap();
        fs::write(data.join("user-note.txt"), b"preserve").unwrap();
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(root.join("plugins/user-notes.txt"), b"custom").unwrap();
        let work = tempfile::Builder::new()
            .prefix(".luciddesk-update-")
            .tempdir_in(&root)
            .unwrap();
        let mut archive = fs::File::open(zip).unwrap();
        let files =
            package::extract(&mut archive, &work.path().join("new"), version, portable, &|| false).unwrap();
        let build = package::build(&root).unwrap();
        package::check_targets(
            &root,
            files.iter().map(|file| file.relative.as_str()),
            &build,
        )
        .unwrap();
        apply_zip(&root, work.path(), &files, &build).unwrap();
        drop(files);
        assert_eq!(fs::read(data.join("config.toml")).unwrap(), before_config);
        assert_eq!(
            fs::read(data.join("workspace.db")).unwrap(),
            before_database
        );
        assert_eq!(fs::read(data.join("user-note.txt")).unwrap(), b"preserve");
        assert_eq!(
            fs::read(root.join("plugins/user-notes.txt")).unwrap(),
            b"custom"
        );
        drop(gate);
        let mut new = Running(
            Command::new(root.join("luciddesk.exe"))
                .env("LUCIDDESK_DATA_DIR", &data)
                .spawn()
                .unwrap(),
        );
        std::thread::sleep(std::time::Duration::from_secs(3));
        assert!(new.0.try_wait().unwrap().is_none());
        platform::Original::open(new.0.id())
            .unwrap()
            .close()
            .unwrap();
        assert!(new.0.wait().unwrap().success());
    }
}
