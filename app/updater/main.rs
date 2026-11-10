#![windows_subsystem = "windows"]

#[path = "../src/updates/distribution.rs"]
mod distribution;
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
use std::{
    env,
    fs::File,
    path::{Path, PathBuf},
    process::Command,
};
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
    if let Err(error) = run() {
        platform::message(&error.to_string(), MB_ICONERROR);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        println!("updater {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let [pid, version, prompt, appearance @ ..] = args.as_slice() else {
        return Err("Start the updater from Settings → About in LucidDesk.".into());
    };
    if let [appearance] = appearance {
        let (ok, cancel, copy, dark): (String, String, String, bool) =
            serde_json::from_str(appearance.to_str().ok_or("Invalid dialog settings")?)?;
        luciddesk_window::set_message_labels(&ok, &cancel, &copy);
        luciddesk_window::set_message_theme(Some(dark));
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
    let update = PreparedUpdate::prepare(root, version, current)?;
    if platform::message(&prompt.to_string_lossy(), MB_OKCANCEL | MB_ICONINFORMATION) != IDOK {
        return Ok(());
    }
    update.install(root, &original)
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
    fn prepare(root: &Path, version: &str, current: &str) -> Result<Self> {
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
        let mut verified_archive = release::fetch(version, distribution.kind(), &archive)?;
        let payload = match distribution {
            distribution::Distribution::Inno(scope) => PreparedPayload::Inno {
                setup: archive,
                scope,
            },
            _ => {
                let files = package::extract(
                    &mut verified_archive,
                    &work.path().join("new"),
                    version,
                    portable,
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
        package::extract(&mut archive, &root, version, portable).unwrap();
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
            package::extract(&mut archive, &work.path().join("new"), version, portable).unwrap();
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
