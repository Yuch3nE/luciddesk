//! Initial icon layout for new ordinary panes; existing panes keep their layout.
use luciddesk_core::Panel;
use luciddesk_storage::WorkspaceStore;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    Compact,
    Grid,
    Free,
}

impl Mode {
    pub fn load(store: &WorkspaceStore) -> Result<Self, String> {
        let value = store.preference("desktop_panel_layout").map_err(|e| e.to_string())?;
        match value.as_deref() {
            None | Some("compact") => Ok(Self::Compact),
            Some("grid") => Ok(Self::Grid),
            Some("free") => Ok(Self::Free),
            _ => Err("invalid desktop panel layout default".into()),
        }
    }

    pub fn save(self, store: &WorkspaceStore) -> Result<(), String> {
        let value = match self {
            Self::Compact => "compact",
            Self::Grid => "grid",
            Self::Free => "free",
        };
        store.save_preference("desktop_panel_layout", value).map_err(|e| e.to_string())
    }

    pub fn toggle_auto_arrange(self) -> Self {
        if self == Self::Compact { Self::Grid } else { Self::Compact }
    }

    pub fn toggle_align(self) -> Self {
        if self == Self::Free { Self::Grid } else { Self::Free }
    }

    pub fn apply(self, panel: &mut Panel) {
        if panel.supports_tabs() {
            panel.set_fixed_grid(self != Self::Compact);
            panel.set_free_layout(self == Self::Free);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use luciddesk_core::{PanelId, RectDip};

    #[test]
    fn toggles_keep_auto_arrangement_aligned_and_allow_free_positions() {
        for mode in [Mode::Compact, Mode::Grid, Mode::Free] {
            assert_eq!(mode.toggle_auto_arrange(), if mode == Mode::Compact { Mode::Grid } else { Mode::Compact });
            assert_eq!(mode.toggle_align(), if mode == Mode::Free { Mode::Grid } else { Mode::Free });
        }
    }

    #[test]
    fn defaults_persist_without_rewriting_existing_panels_or_unchanged_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspace.db");
        let store = WorkspaceStore::open(&path).unwrap();
        assert_eq!(Mode::load(&store).unwrap(), Mode::Compact);
        let existing = Panel::new(PanelId::new(1), "Existing", RectDip::new(0.0, 0.0, 300.0, 200.0));
        for mode in [Mode::Free, Mode::Grid, Mode::Compact] {
            mode.save(&store).unwrap();
            let changes = store.change_count();
            mode.save(&store).unwrap();
            assert_eq!(store.change_count(), changes);
            let reopened = WorkspaceStore::open(&path).unwrap();
            let loaded = Mode::load(&reopened).unwrap();
            assert_eq!(loaded, mode);
            let mut new = existing.clone();
            loaded.apply(&mut new);
            assert_eq!(new.fixed_grid(), mode != Mode::Compact);
            assert_eq!(new.free_layout(), mode == Mode::Free);
            assert!(!existing.fixed_grid());
            assert!(!existing.free_layout());
            new.set_folder(Some(dir.path().to_owned()));
            loaded.apply(&mut new);
            assert!(!new.fixed_grid());
            assert!(!new.free_layout());
        }
    }
}
