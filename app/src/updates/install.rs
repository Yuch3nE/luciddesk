use super::handoff::{self, Appearance, Stage};
use std::{fs, io, path::PathBuf, process::{Child, Command, ExitStatus}};

pub(super) struct Running {
    child: Child,
    directory: PathBuf,
    pub stage: Stage,
    pub canceled: bool,
}

impl Running {
    pub(super) fn new(child: Child, directory: PathBuf) -> Self {
        Self { child, directory, stage: Stage::Preparing, canceled: false }
    }

    pub fn poll(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = self.child.try_wait()? {
            return Ok(Some(status));
        }
        if let Ok(bytes) = fs::read(self.directory.join(handoff::STATUS))
            && let Ok(stage) = serde_json::from_slice(&bytes)
        {
            self.stage = stage;
        }
        Ok(None)
    }

    pub fn cancel(&mut self) -> io::Result<()> {
        match fs::OpenOptions::new().write(true).create_new(true).open(self.directory.join(handoff::DECISION)) {
            Ok(_) => self.canceled = true,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        Ok(())
    }
}

pub(super) fn kind() -> Option<&'static str> {
    let key = super::signed::MODULUS;
    if key.len() < 512
        || !key.len().is_multiple_of(2)
        || !key.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    let dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))?;
    if !dir.join("build.json").is_file() || !dir.join("updater.exe").is_file() {
        return None;
    }
    super::distribution::Distribution::detect(&dir).map(|distribution| distribution.kind())
}

pub(super) fn launch(version: &str) -> Result<Running, String> {
    stage_and_launch(version).map_err(|e| e.to_string())
}

fn stage_and_launch(version: &str) -> io::Result<Running> {
    let exe = std::env::current_exe()?;
    let updater = exe.with_file_name("updater.exe");
    let cache = std::env::temp_dir().join("LucidDesk-Updaters");
    super::distribution::no_links(&cache)?;
    std::fs::create_dir_all(&cache)?;
    // A running image cannot be deleted on Windows. Remove only our previous
    // image and its empty directory; concurrent or unrelated files stay intact.
    for entry in std::fs::read_dir(&cache)?.flatten() {
        let dir = entry.path();
        if entry.file_name().to_string_lossy().starts_with("run-")
            && entry.file_type()?.is_dir()
            && super::distribution::no_links(&dir).is_ok()
        {
            let _ = std::fs::remove_file(dir.join("updater.exe"));
            let _ = std::fs::remove_dir(dir);
        }
    }
    let work = tempfile::Builder::new().prefix("run-").tempdir_in(&cache)?;
    let image = work.path().join("updater.exe");
    std::fs::copy(updater, &image)?;
    let child = Command::new(&image)
        .arg(std::process::id().to_string())
        .arg(version)
        .arg(crate::i18n::format(
            "update-restart",
            &[("version", version.into())],
        ))
        .arg(serde_json::to_string(&Appearance {
            confirm: crate::i18n::text("dialog-confirm").into(),
            cancel: crate::i18n::text("dialog-cancel").into(),
            copy: crate::i18n::text("ui-copy").into(),
            dark: luciddesk_window::message_is_dark(),
            failed: crate::i18n::text("update-install-failed").into(),
        })?)
        .spawn()?;
    Ok(Running::new(child, work.keep()))
}
