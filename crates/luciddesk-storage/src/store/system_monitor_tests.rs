use super::*;
#[test]
fn plugin_window_cannot_be_persisted_as_a_workspace_panel() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let workspace = Workspace::from_panels(vec![Panel::new(PanelId::new(1), "Group", RectDip::default())]).unwrap();
    store.save_workspace(&workspace).unwrap();
    let before = store.load_workspace().unwrap();
    let changes = store.change_count();
    let mut invalid = workspace;
    invalid.panel_mut(PanelId::new(1)).unwrap().set_monitor(true);
    assert!(matches!(store.save_workspace(&invalid), Err(StoreError::InvalidData(_))));
    assert_eq!(store.load_workspace().unwrap(), before);
    assert_eq!(store.change_count(), changes);
    let tables: u64 = store.connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name='panel_monitor_settings'", [], |r| r.get(0)).unwrap();
    assert_eq!(tables, 0);
}
