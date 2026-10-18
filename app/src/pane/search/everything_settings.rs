use luciddesk_storage::WorkspaceStore;
use std::{
    path::PathBuf,
    sync::{LazyLock, RwLock},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::pane) struct Settings {
    pub path: String,
}
static SETTINGS: LazyLock<RwLock<Settings>> = LazyLock::new(|| RwLock::new(Settings::default()));
pub(in crate::pane) fn settings() -> Settings {
    SETTINGS.read().unwrap().clone()
}
// Missing preferences leave search disabled until the user enables it.
pub(in crate::pane) fn enabled(store: &WorkspaceStore) -> Result<bool, String> {
    Ok(store
        .preference("search_enabled")
        .map_err(|e| e.to_string())?
        .as_deref()
        == Some("1")
        && resolved(&settings()).is_some())
}
pub(in crate::pane) fn set_enabled(store: &WorkspaceStore, enabled: bool) -> Result<(), String> {
    store
        .save_preference("search_enabled", if enabled { "1" } else { "0" })
        .map_err(|e| e.to_string())
}
pub(in crate::pane) fn load(store: &WorkspaceStore) -> Result<(), String> {
    *SETTINGS.write().unwrap() = store
        .preference("everything")
        .map_err(|e| e.to_string())?
        .map(|path| Settings { path })
        .unwrap_or_default();
    Ok(())
}
pub(in crate::pane) fn save(store: &WorkspaceStore, value: Settings) -> Result<(), String> {
    store
        .save_preference("everything", &value.path)
        .map_err(|e| e.to_string())?;
    *SETTINGS.write().unwrap() = value;
    Ok(())
}
pub(in crate::pane) fn detect() -> Option<PathBuf> {
    for (variable, base) in [
        ("ProgramFiles", "Everything"),
        ("ProgramFiles(x86)", "Everything"),
        ("LOCALAPPDATA", "Programs\\Everything"),
    ] {
        if let Some(root) = std::env::var_os(variable) {
            let path = PathBuf::from(root).join(base).join("Everything.exe");
            if path.is_file() {
                return Some(path);
            }
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("Everything.exe")))
        .filter(|p| p.is_file())
}
pub(in crate::pane) fn resolved(value: &Settings) -> Option<PathBuf> {
    if value.path.is_empty() {
        detect()
    } else {
        Some(PathBuf::from(&value.path)).filter(|p| {
            p.is_file()
                && p.file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("Everything.exe"))
        })
    }
}
pub(in crate::pane) fn launch() -> Result<(), String> {
    let path = resolved(&settings())
        .filter(|p| p.is_file())
        .ok_or(crate::i18n::text("ui-everything-not-found-select-everything-exe-in-settings"))?;
    std::process::Command::new(path)
        .arg("-startup")
        .spawn()
        .map_err(|e| crate::i18n::format("ui-could-not-start-everything", &[("e", format!("{}", e))]))?;
    Ok(())
}
pub(in crate::pane) fn browse(owner: isize) -> Result<Option<String>, String> {
    super::super::executable_picker::browse(
        owner, "Everything", "Everything.exe", "ui-could-not-select-everything",
        crate::i18n::text("ui-select-everything-exe"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_availability_tracks_configured_path() {
        let dir = std::env::temp_dir().join(format!(
            "luciddesk-availability-Everything.exe-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Everything.exe");
        let value = Settings {
            path: path.to_string_lossy().into_owned(),
        };
        assert!(resolved(&value).is_none());
        std::fs::write(&path, b"availability fixture").unwrap();
        assert_eq!(resolved(&value), Some(path.clone()));
        std::fs::remove_file(&path).unwrap();
        assert!(resolved(&value).is_none());
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn settings_preserve_unicode_path_without_unused_flags() {
        let store = WorkspaceStore::open_in_memory().unwrap();
        let value = Settings { path: "D:/应用/Everything.exe".into() };
        save(&store, value.clone()).unwrap();
        assert_eq!(store.preference("everything").unwrap().as_deref(), Some(value.path.as_str()));
        load(&store).unwrap();
        assert_eq!(settings(), value);
    }
}
