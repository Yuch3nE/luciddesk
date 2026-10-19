//! Plugin-owned configuration. The host supplies a directory, never a database.
use crate::Preferences;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, io::Write, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Window {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub collapsed: bool,
    pub locked: bool,
    pub always_on_top: bool,
    pub auto_hide: bool,
}
impl Default for Window {
    fn default() -> Self {
        Self {
            x: 40.0,
            y: 40.0,
            width: 360.0,
            height: 600.0,
            collapsed: false,
            locked: false,
            always_on_top: false,
            auto_hide: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Config {
    pub options: Preferences,
    pub window: Window,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            options: Preferences {
                enabled: false,
                ..Default::default()
            },
            window: Window::default(),
        }
    }
}
impl Config {
    fn validate(&self) -> Result<(), String> {
        if ![1, 2, 5].contains(&self.options.interval) {
            return Err("Invalid sampling interval".into());
        }
        if self.options.metrics.iter().enumerate().any(|(i, metric)| self.options.metrics[..i].contains(metric)) {
            return Err("Duplicate monitor metric".into());
        }
        let w = &self.window;
        if ![w.x, w.y, w.width, w.height]
            .into_iter()
            .all(f32::is_finite)
            || w.width <= 0.0
            || w.height <= 0.0
        {
            return Err("Invalid monitor window bounds".into());
        }
        Ok(())
    }
}
#[derive(Default)]
struct State {
    path: Option<PathBuf>,
    config: Config,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
pub(crate) fn open(directory: PathBuf) -> Result<(), String> {
    let path = directory.join("config.toml");
    let config = match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str::<Config>(&text).map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(e) => return Err(e.to_string()),
    };
    config.validate()?;
    STATE.with(|state| {
        *state.borrow_mut() = State {
            path: Some(path),
            config,
        }
    });
    Ok(())
}
pub(crate) fn get() -> Config {
    STATE.with(|s| s.borrow().config.clone())
}
fn save(config: Config) -> Result<(), String> {
    config.validate()?;
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.config == config {
            return Ok(());
        }
        if let Some(path) = &state.path {
            let directory = path.parent().unwrap();
            std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
            let text = toml::to_string_pretty(&config).map_err(|e| e.to_string())?;
            let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
            file.write_all(text.as_bytes())
                .and_then(|_| file.as_file().sync_all())
                .map_err(|e| e.to_string())?;
            file.persist(path).map_err(|e| e.to_string())?;
        }
        state.config = config;
        Ok(())
    })
}
pub(crate) fn options(options: Preferences) -> Result<(), String> {
    save(Config { options, ..get() })
}
pub(crate) fn window(window: Window) -> Result<(), String> {
    save(Config { window, ..get() })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_roundtrips_without_creating_a_database_and_rejects_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        open(dir.path().into()).unwrap();
        assert!(!get().options.enabled);
        assert!(!dir.path().join("config.toml").exists());
        let options = Preferences {
            enabled: true,
            interval: 5,
            ..Default::default()
        };
        super::options(options.clone()).unwrap();
        window(Window {
            x: -500.0,
            always_on_top: true,
            ..Default::default()
        })
        .unwrap();
        let bytes = std::fs::read(dir.path().join("config.toml")).unwrap();
        super::options(options.clone()).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("config.toml")).unwrap(),
            bytes
        );
        open(dir.path().into()).unwrap();
        assert_eq!(get().options, options);
        assert_eq!(get().window.x, -500.0);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        std::fs::write(dir.path().join("config.toml"), "broken = [").unwrap();
        assert!(open(dir.path().into()).is_err());
        assert_eq!(get().options, options);
    }
    #[test]
    fn failed_save_preserves_cached_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("blocked");
        open(blocked.clone()).unwrap();
        std::fs::write(&blocked, "file").unwrap();
        assert!(options(Preferences::default()).is_err());
        assert!(!get().options.enabled);
    }
}
