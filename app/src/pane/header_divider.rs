//! Global header separator preference, included in workspace metadata backups.
use std::sync::atomic::{AtomicBool, Ordering};
static ENABLED: AtomicBool = AtomicBool::new(true);
const KEY: &str = "pane_header_divider";
pub(super) fn enabled() -> bool { ENABLED.load(Ordering::Relaxed) }
pub(super) fn load(store: &luciddesk_storage::WorkspaceStore) -> Result<(), String> {
    let value = store.preference(KEY).map_err(|e| e.to_string())?;
    ENABLED.store(value.as_deref() != Some("false"), Ordering::Relaxed);
    Ok(())
}
pub(super) fn save(store: &luciddesk_storage::WorkspaceStore, value: bool) -> Result<(), String> {
    store.save_preference(KEY, if value { "true" } else { "false" }).map_err(|e| e.to_string())?;
    ENABLED.store(value, Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn divider_setting_round_trips_and_changes_rendering_without_layout_changes() {
        let _sta = crate::pane::test_support::apartment();
        struct Restore(bool);
        impl Drop for Restore { fn drop(&mut self) { ENABLED.store(self.0, Ordering::Relaxed); } }
        let _restore = Restore(enabled());
        let store = luciddesk_storage::WorkspaceStore::open_in_memory().unwrap();
        load(&store).unwrap(); assert!(enabled());
        let mut state = super::super::tests::test_state();
        state.workspace.set_appearance(luciddesk_core::PanelTheme::Dark, luciddesk_core::Backdrop::Mica);
        let model = super::super::create_model(&state, luciddesk_core::PanelId::new(1)).unwrap();
        let mut renderer = super::super::render::Renderer::new().unwrap();
        let shown = renderer.pixels(420, 300, 1.0, &model).unwrap();
        save(&store, false).unwrap();
        assert_ne!(shown, renderer.pixels(420, 300, 1.0, &model).unwrap());
        ENABLED.store(true, Ordering::Relaxed);
        load(&store).unwrap(); assert!(!enabled());
        assert_eq!(store.backup_snapshot().unwrap().preference(KEY).unwrap().as_deref(), Some("false"));
    }
}
