use std::path::PathBuf;
use std::process::{Child, Command};

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

pub(super) fn launch(version: &str) -> Result<Child, String> {
    stage_and_launch(version).map_err(|e| e.to_string())
}

fn stage_and_launch(version: &str) -> std::io::Result<Child> {
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
        .arg(serde_json::to_string(&(
            crate::i18n::text("dialog-confirm"), crate::i18n::text("dialog-cancel"),
            crate::i18n::text("ui-copy"), luciddesk_window::message_is_dark(),
        ))?)
        .spawn()?;
    let _ = work.keep();
    Ok(child)
}
