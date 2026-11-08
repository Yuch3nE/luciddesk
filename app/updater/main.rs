#![windows_subsystem = "windows"]

#[path = "../src/updates/distribution.rs"]
mod distribution;
#[path = "../src/updates/http.rs"]
mod http;
mod package;
mod platform;
mod release;

use semver::Version;
use std::{env, fs, path::Path, process::Command};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDOK, MB_ICONERROR, MB_ICONINFORMATION, MB_OKCANCEL,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

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
    let build = package::build(root)?;
    let portable = root.join("portable").is_file();
    if build.version != current
        || build.architecture != "windows-x64"
        || build.portable != portable
        || root.join("msix").exists()
    {
        return Err("Unsupported distribution or updater version".into());
    }
    let scope = if root.join("installed").exists() {
        Some(
            distribution::inno_scope(root)
                .ok_or("Use the original installer to update this installation")?,
        )
    } else {
        None
    };
    let kind = if scope.is_some() {
        "installed"
    } else if portable {
        "portable"
    } else {
        "green"
    };
    if scope.is_none() {
        for item in fs::read_dir(root)? {
            let item = item?;
            if item
                .file_name()
                .to_string_lossy()
                .starts_with(".luciddesk-update-")
                && item.path().join("backup").exists()
            {
                return Err(format!(
                    "Recover the interrupted update in {} before retrying",
                    item.path().display()
                )
                .into());
            }
        }
    }
    let temp = env::temp_dir();
    let parent = if scope.is_some() {
        temp.as_path()
    } else {
        root
    };
    distribution::no_links(parent)?;
    let work = tempfile::Builder::new()
        .prefix(".luciddesk-update-")
        .tempdir_in(parent)?;
    let stage = work.path().join("new");
    let archive = work.path().join(if scope.is_some() {
        "setup.exe"
    } else {
        "update.zip"
    });
    let mut verified = release::fetch(version, kind, &archive)?;
    let mut files = Vec::new();
    if scope.is_none() {
        files = package::extract(&mut verified, &stage, version, portable)?;
        package::check_targets(root, &files, &build)?;
    }
    if platform::message(&prompt.to_string_lossy(), MB_OKCANCEL | MB_ICONINFORMATION) != IDOK {
        return Ok(());
    }
    let gate = platform::gate(r"Local\LucidDesk.UpdateApplying")?;
    original.close()?;
    let mut retain = false;
    let result = match scope {
        Some(scope) => platform::setup(&archive, root, scope),
        None => apply_zip(root, work.path(), &files, &mut retain),
    };
    drop(verified);
    drop(gate);
    if retain {
        let _recovery = work.keep();
    } else {
        Command::new(&original.exe).spawn()?;
    }
    result
}

fn apply_zip(root: &Path, work: &Path, files: &[String], retain: &mut bool) -> Result<()> {
    let _instance = platform::gate(r"Local\LucidDesk.DesktopSession")?;
    let probe = Command::new(work.join("new/luciddesk.exe"))
        .arg("--check-desktop-component")
        .status()?;
    if !probe.success() {
        return Err("Explorer has not released the desktop component".into());
    }
    package::replace(root, work, files, retain)
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
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
        package::check_targets(&root, &files, &package::build(&root).unwrap()).unwrap();
        let mut retain = false;
        apply_zip(&root, work.path(), &files, &mut retain).unwrap();
        assert!(!retain);
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
