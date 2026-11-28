use super::handoff::{self, Appearance, Stage};
use std::{fs, io, os::windows::fs::OpenOptionsExt, path::{Path, PathBuf}, process::{Child, Command, ExitStatus}};

const LEASE: &str = "launch.lock";

impl Stage {
    fn allows_cancel(self) -> bool {
        !matches!(self, Self::Confirming | Self::Applying)
    }
}

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
            cleanup_run(&self.directory);
            return Ok(Some(status));
        }
        self.refresh_stage();
        Ok(None)
    }

    pub fn cancel(&mut self) -> io::Result<()> {
        if handoff::claim_decision(&self.directory)? {
            self.canceled = true;
        } else {
            self.refresh_stage();
        }
        Ok(())
    }

    pub(super) fn can_cancel(&self) -> bool {
        !self.canceled && self.stage.allows_cancel()
    }

    fn refresh_stage(&mut self) {
        if let Ok(bytes) = fs::read(self.directory.join(handoff::STATUS))
            && let Ok(stage) = serde_json::from_slice::<Stage>(&bytes)
            && (self.stage.allows_cancel() || !stage.allows_cancel())
        {
            self.stage = stage;
        }
        if self.can_cancel() && self.directory.join(handoff::DECISION).exists() {
            // The updater claimed confirmation before publishing its next stage.
            self.stage = Stage::Confirming;
        }
    }
}

fn cache() -> PathBuf {
    std::env::temp_dir().join("LucidDesk-Updaters")
}

fn cleanup_run(directory: &Path) {
    if super::distribution::no_links(directory).is_err() { return; }
    let lease_path = directory.join(LEASE);
    let lease = match fs::OpenOptions::new().read(true).write(true).share_mode(0).open(&lease_path) {
        Ok(handle) => Some(handle),
        Err(error) if error.kind() == io::ErrorKind::NotFound
            && fs::metadata(directory).and_then(|meta| meta.modified()).ok()
                .and_then(|time| time.elapsed().ok()).is_some_and(|age| age.as_secs() >= 60) => None,
        Err(_) => return,
    };
    match fs::remove_file(directory.join("updater.exe")) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return,
    }
    if let Ok(warning) = fs::read_to_string(directory.join(handoff::STATUS_WARNING)) {
        luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Warn, "updates.progress", warning.trim_end());
    }
    for name in [handoff::STATUS, handoff::NEXT_STATUS, handoff::DECISION, handoff::STATUS_WARNING] {
        let _ = fs::remove_file(directory.join(name));
    }
    drop(lease);
    let _ = fs::remove_file(lease_path);
    let _ = fs::remove_dir(directory);
}

fn cleanup(cache: &Path) -> bool {
    if super::distribution::no_links(cache).is_err() { return false; }
    let Ok(entries) = fs::read_dir(cache) else { return false; };
    let mut pending = false;
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with("run-")
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            cleanup_run(&entry.path());
            pending |= entry.path().exists();
        }
    }
    pending
}

pub(crate) fn cleanup_after_start() {
    let cache = cache();
    let _ = std::thread::Builder::new().name("updater-cleanup".into()).spawn(move || {
        for delay in [0, 2, 8, 50] {
            std::thread::sleep(std::time::Duration::from_secs(delay));
            if !cleanup(&cache) { break; }
        }
    });
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
    let cache = cache();
    super::distribution::no_links(&cache)?;
    std::fs::create_dir_all(&cache)?;
    cleanup(&cache);
    let work = tempfile::Builder::new().prefix("run-").tempdir_in(&cache)?;
    let lease = fs::OpenOptions::new().write(true).create_new(true).share_mode(0)
        .open(work.path().join(LEASE))?;
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
    drop(lease);
    Ok(Running::new(child, work.keep()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_cleanup_respects_the_lease_and_leaves_unrelated_files() {
        let cache = tempfile::tempdir().unwrap();
        let run = cache.path().join("run-fixture");
        fs::create_dir(&run).unwrap();
        let lease = fs::OpenOptions::new().write(true).create_new(true).share_mode(0)
            .open(run.join(LEASE)).unwrap();
        fs::write(run.join("updater.exe"), b"fixture").unwrap();
        fs::write(run.join(handoff::STATUS), b"fixture").unwrap();
        fs::write(run.join("note.txt"), b"user data").unwrap();
        assert!(cleanup(cache.path()));
        assert!(run.join("updater.exe").is_file());

        drop(lease);
        cleanup(cache.path());
        assert!(!run.join("updater.exe").exists());
        assert!(!run.join(handoff::STATUS).exists());
        assert_eq!(fs::read(run.join("note.txt")).unwrap(), b"user data");
    }
}
