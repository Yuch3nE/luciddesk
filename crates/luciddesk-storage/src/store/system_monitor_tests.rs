use super::*;

#[test]
fn monitor_source_roundtrips_geometry_and_never_loads_desktop_layout_preferences() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let id = PanelId::new(8);
    let mut panel = Panel::new(id, "Monitor", RectDip::new(10.0, 20.0, 360.0, 460.0));
    panel.set_monitor(true);
    panel.set_auto_hide(true);
    panel.set_always_on_top(true);
    let workspace = Workspace::from_panels(vec![panel]).unwrap();
    store.save_workspace(&workspace).unwrap();
    store.save_preference("panel_desktop_list:8", "1").unwrap();
    store.save_preference("panel_fixed_grid:8", "1").unwrap();
    let loaded = store.load_workspace().unwrap();
    assert_eq!(loaded, workspace);
    let panel = loaded.panel(id).unwrap();
    assert!(panel.is_monitor());
    assert!(!panel.supports_tabs());
    assert!(!panel.list_view());
    assert!(!panel.fixed_grid());
}

#[test]
fn existing_database_gains_monitor_marker_without_changing_panels_or_metadata() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let id = PanelId::new(42);
    let workspace =
        Workspace::from_panels(vec![Panel::new(id, "Keep", RectDip::default())]).unwrap();
    store.save_workspace(&workspace).unwrap();
    store
        .connection
        .execute_batch(
            "DROP TABLE panel_monitor_settings; INSERT INTO metadata VALUES ('keep','value');",
        )
        .unwrap();
    let mut store = WorkspaceStore::from_connection(store.connection).unwrap();
    assert_eq!(store.load_workspace().unwrap(), workspace);
    assert_eq!(store.preference("keep").unwrap().as_deref(), Some("value"));
    let mut changed = workspace;
    changed.panel_mut(id).unwrap().set_monitor(true);
    store.save_workspace(&changed).unwrap();
    assert!(
        store
            .load_workspace()
            .unwrap()
            .panel(id)
            .unwrap()
            .is_monitor()
    );
    changed.remove_panel(id);
    store.save_workspace(&changed).unwrap();
    assert_eq!(
        store
            .connection
            .query_row("SELECT count(*) FROM panel_monitor_settings", [], |r| r
                .get::<_, u64>(0))
            .unwrap(),
        0
    );
}
