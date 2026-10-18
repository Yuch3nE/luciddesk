#[test]
fn bundled_sqlite_omits_unused_extensions_but_keeps_core_safety() {
    let db = rusqlite::Connection::open_in_memory().unwrap();
    for (option, expected) in [
        ("ENABLE_FTS3", false),
        ("ENABLE_FTS5", false),
        ("ENABLE_RTREE", false),
        ("ENABLE_DBSTAT_VTAB", false),
        ("OMIT_LOAD_EXTENSION", true),
        ("ENABLE_API_ARMOR", true),
        ("THREADSAFE=1", true),
        ("DEFAULT_FOREIGN_KEYS", true),
    ] {
        let enabled: bool = db
            .query_row("SELECT sqlite_compileoption_used(?1)", [option], |row| row.get(0))
            .unwrap();
        assert_eq!(enabled, expected, "unexpected SQLite build option: {option}");
    }
}

#[test]
fn unchanged_preferences_do_not_count_as_database_changes() {
    let store = WorkspaceStore::open_in_memory().unwrap();
    store.save_preference("test", "first").unwrap();
    let changes = store.change_count();
    store.save_preference("test", "first").unwrap();
    assert_eq!(store.change_count(), changes);
    store.save_preference("test", "second").unwrap();
    assert_eq!(store.change_count(), changes + 1);
    assert_eq!(store.preference("test").unwrap().as_deref(), Some("second"));
    let options = Workspace::new().pane_options();
    store.save_pane_options(options).unwrap();
    let changes = store.change_count();
    store.save_pane_options(options).unwrap();
    assert_eq!(store.change_count(), changes);
}

#[test]
fn material_strength_round_trips_and_remembers_each_material() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let mut workspace = Workspace::new();
    workspace
        .add_panel(Panel::new(
            PanelId::new(1),
            "Material",
            RectDip::new(0.0, 0.0, 300.0, 200.0),
        ))
        .unwrap();
    for base in [Backdrop::Acrylic, Backdrop::Mica] {
        for strength in [0, 33, 50, 100] {
            let value = base.with_strength(strength);
            workspace.set_appearance(luciddesk_core::PanelTheme::Dark, value);
            store.save_workspace(&workspace).unwrap();
            let restored = store.load_workspace().unwrap();
            assert_eq!(restored.appearance(), workspace.appearance());
            assert_eq!(restored.panels()[0].backdrop(), value);
        }
    }
    for base in [Backdrop::Acrylic, Backdrop::Mica] {
        assert_eq!(
            store
                .preference(base.strength_key().unwrap())
                .unwrap()
                .as_deref(),
            Some("100")
        );
    }
    workspace.set_appearance(
        luciddesk_core::PanelTheme::Dark,
        Backdrop::Tuned {
            material: luciddesk_core::TunableMaterial::Mica,
            strength: 200,
        },
    );
    assert!(store.save_workspace(&workspace).is_err());
    assert_eq!(
        store.load_workspace().unwrap().appearance().unwrap().1,
        Backdrop::Mica.with_strength(100)
    );
    assert!(super::decode_backdrop("mica_alt_tuned", Some(0.5), None).is_err());
    assert_eq!(Backdrop::MicaAlt.strength(), None);
    assert_eq!(Backdrop::MicaAlt.strength_key(), None);
    assert!(super::decode_backdrop("mica_tuned", Some(f32::NAN), None).is_err());
}

#[test]
fn solid_style_round_trips_and_survives_switching_material() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let mut workspace = Workspace::new();
    workspace
        .add_panel(Panel::new(
            PanelId::new(1),
            "Solid",
            RectDip::new(0.0, 0.0, 300.0, 200.0),
        ))
        .unwrap();
    for opacity in [0.0, 0.5, 1.0] {
        let solid = Backdrop::Solid {
            color: 0x1234ab,
            opacity,
        };
        workspace.set_appearance(luciddesk_core::PanelTheme::Dark, solid);
        store.save_workspace(&workspace).unwrap();
        let restored = store.load_workspace().unwrap();
        assert_eq!(restored.appearance(), workspace.appearance());
        assert_eq!(restored.panels()[0].backdrop(), solid);
    }
    workspace.set_appearance(luciddesk_core::PanelTheme::Dark, Backdrop::Mica);
    store.save_workspace(&workspace).unwrap();
    assert_eq!(
        store.preference("solid_style").unwrap().as_deref(),
        Some("1193131|1")
    );
    for opacity in [f32::NAN, -0.1, 1.1] {
        workspace.set_appearance(
            luciddesk_core::PanelTheme::Dark,
            Backdrop::Solid {
                color: 0x1234ab,
                opacity,
            },
        );
        assert!(store.save_workspace(&workspace).is_err());
        assert_eq!(
            store.load_workspace().unwrap().appearance().unwrap().1,
            Backdrop::Mica
        );
    }
}

#[test]
fn search_panes_round_trip_and_do_not_retain_folder_sources() {
    use luciddesk_core::{Panel, PanelId, RectDip, Workspace};
    let mut store = super::WorkspaceStore::open_in_memory().unwrap();
    let mut panel = Panel::new(
        PanelId::new(7),
        "Everything",
        RectDip::new(120.0, 140.0, 860.0, 520.0),
    );
    panel.set_folder(Some(std::path::PathBuf::from(r"C:\folder")));
    panel.set_search(true);
    assert!(panel.folder().is_none());
    let mut workspace = Workspace::from_panels(vec![panel]).unwrap();
    store.save_workspace(&workspace).unwrap();
    let loaded = store.load_workspace().unwrap();
    assert_eq!(loaded.panels(), workspace.panels());
    assert!(loaded.desktop_items().is_empty());
    workspace
        .panel_mut(PanelId::new(7))
        .unwrap()
        .set_folder(Some(std::path::PathBuf::from(r"C:\folder")));
    assert!(!workspace.panel(PanelId::new(7)).unwrap().is_search());
    store.save_workspace(&workspace).unwrap();
    assert!(store.preference("panel_search:7").unwrap().is_none());
    workspace
        .panel_mut(PanelId::new(7))
        .unwrap()
        .set_search(true);
    store.save_workspace(&workspace).unwrap();
    workspace.remove_panel(PanelId::new(7));
    store.save_workspace(&workspace).unwrap();
    assert!(store.preference("panel_search:7").unwrap().is_none());
}
#[test]
fn folder_sources_survive_reopen_and_are_removed_with_the_panel() {
    let mut store = super::WorkspaceStore::open_in_memory().unwrap();
    let mut folder = luciddesk_core::Panel::new(
        luciddesk_core::PanelId::new(2),
        "映射",
        luciddesk_core::RectDip::default(),
    );
    let path = std::path::PathBuf::from(r"C:\资料\尚未挂载");
    folder.set_folder(Some(path.clone()));
    let mut workspace = luciddesk_core::Workspace::from_panels(vec![
        luciddesk_core::Panel::new(
            luciddesk_core::PanelId::new(1),
            "桌面",
            luciddesk_core::RectDip::default(),
        ),
        folder,
    ])
    .unwrap();
    store.save_preference("peek", "keep").unwrap();
    store.save_workspace(&workspace).unwrap();
    let loaded = store.load_workspace().unwrap();
    assert_eq!(
        loaded
            .panel(luciddesk_core::PanelId::new(2))
            .unwrap()
            .folder(),
        Some(path.as_path())
    );
    assert!(
        loaded
            .panel(luciddesk_core::PanelId::new(1))
            .unwrap()
            .folder()
            .is_none()
    );
    assert!(loaded.desktop_items().is_empty());
    assert!(
        loaded
            .panel(luciddesk_core::PanelId::new(2))
            .unwrap()
            .list_view()
    );
    workspace
        .panel_mut(luciddesk_core::PanelId::new(2))
        .unwrap()
        .set_list_view(false);
    store.save_workspace(&workspace).unwrap();
    assert!(
        !store
            .load_workspace()
            .unwrap()
            .panel(luciddesk_core::PanelId::new(2))
            .unwrap()
            .list_view()
    );
    workspace.remove_panel(luciddesk_core::PanelId::new(2));
    store.save_workspace(&workspace).unwrap();
    assert!(store.preference("panel_folder:2").unwrap().is_none());
    assert!(store.preference("panel_folder_view:2").unwrap().is_none());
    assert_eq!(store.preference("peek").unwrap().as_deref(), Some("keep"));
}
use super::WorkspaceStore;
#[test]
fn desktop_list_preference_is_per_panel_and_removed_with_panel() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let mut workspace = luciddesk_core::Workspace::default();
    let id = luciddesk_core::PanelId::new(42);
    let mut panel = luciddesk_core::Panel::new(id, "List", luciddesk_core::RectDip::new(0.0, 0.0, 400.0, 300.0));
    assert!(!panel.list_view());
    panel.set_list_view(true);
    workspace.add_panel(panel).unwrap();
    store.save_workspace(&workspace).unwrap();
    assert!(store.load_workspace().unwrap().panel(id).unwrap().list_view());
    workspace.remove_panel(id);
    store.save_workspace(&workspace).unwrap();
    assert!(store.preference("panel_desktop_list:42").unwrap().is_none());
}
use luciddesk_core::{
    Backdrop, DesktopItem, DesktopPlacement, GridPosition, Panel, PanelId, RectDip, ShellIdentity,
    Workspace,
};
use rusqlite::Connection;

#[test]
fn missing_shortcuts_are_not_persisted_or_restored_after_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("workspace.db");
    let mut store = WorkspaceStore::open(&path).unwrap();
    let mut workspace = Workspace::from_panels(vec![Panel::new(
        PanelId::new(42), "Music", RectDip::default(),
    )]).unwrap();
    let identity = ShellIdentity::FileSystem {
        path: "Music.lnk".into(), volume_id: None, file_id: None,
    };
    let mut item = DesktopItem::new(identity.clone(), "Music");
    item.set_placement(DesktopPlacement::Pane { pane_id: PanelId::new(42), position: GridPosition::new(2, 3) });
    workspace.reconcile_desktop_items([item]);
    store.save_workspace(&workspace).unwrap();
    workspace.reconcile_desktop_items([]);
    store.save_workspace(&workspace).unwrap();
    let backup = directory.path().join("backup.db");
    store.export_backup(&backup).unwrap();
    let restored = WorkspaceStore::read_backup(&backup).unwrap().load_workspace().unwrap();
    assert_eq!(restored.panels(), workspace.panels());
    assert!(restored.desktop_items().is_empty());
    assert_eq!(store.connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name='shortcut_history'", [], |r| r.get::<_, i64>(0),
    ).unwrap(), 0);
    drop(store);
    let store = WorkspaceStore::open(&path).unwrap();
    let mut reopened = store.load_workspace().unwrap();
    assert_eq!(reopened.panels(), workspace.panels());
    assert!(reopened.desktop_items().is_empty());
    reopened.reconcile_desktop_items([DesktopItem::new(identity, "Music")]);
    assert!(matches!(reopened.desktop_items()[0].placement(), DesktopPlacement::FreeDesktop { .. }));
}

#[test]
fn workspace_round_trips() {
    let mut panel = Panel::new(
        PanelId::new(42),
        "Downloads",
        RectDip::new(-300.0, 75.0, 540.0, 480.0),
    );
    panel.set_backdrop(Backdrop::Translucent { opacity: 0.72 });
    panel.set_collapsed(true);
    panel.set_auto_hide(true);
    panel.set_always_on_top(true);
    panel.set_theme(luciddesk_core::PanelTheme::Light);

    let mut workspace = Workspace::from_panels(vec![panel]).unwrap();
    let mut desktop_item = DesktopItem::new(
        ShellIdentity::Namespace {
            parsing_name: "::{645FF040-5081-101B-9F08-00AA002F954E}".into(),
        },
        "Recycle Bin",
    );
    desktop_item.set_placement(DesktopPlacement::Pane {
        pane_id: PanelId::new(42),
        position: GridPosition::new(1, 2),
    });
    workspace.reconcile_desktop_items([desktop_item]);
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    for (theme, backdrop) in [
        (luciddesk_core::PanelTheme::System, Backdrop::MicaAlt),
        (luciddesk_core::PanelTheme::Light, Backdrop::Translucent { opacity: 0.72 }),
        (luciddesk_core::PanelTheme::Dark, Backdrop::Mica),
    ] {
        let panel = workspace.panel_mut(PanelId::new(42)).unwrap();
        panel.set_theme(theme);
        panel.set_backdrop(backdrop);
        store.save_workspace(&workspace).unwrap();
        assert_eq!(store.load_workspace().unwrap(), workspace);
    }
}

#[test]
fn global_appearance_survives_empty_workspace_and_new_panels() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let mut workspace = Workspace::new();
    workspace.set_appearance(luciddesk_core::PanelTheme::Dark, Backdrop::MicaAlt);
    store.save_workspace(&workspace).unwrap();
    let mut restored = store.load_workspace().unwrap();
    assert_eq!(restored.appearance(), workspace.appearance());
    restored
        .add_panel(Panel::new(PanelId::new(1), "New", RectDip::default()))
        .unwrap();
    assert_eq!(restored.panels()[0].theme(), luciddesk_core::PanelTheme::Dark);
    assert_eq!(restored.panels()[0].backdrop(), Backdrop::MicaAlt);
    store.save_workspace(&restored).unwrap();
    assert_eq!(store.load_workspace().unwrap(), restored);
}

#[test]
fn save_replaces_previous_snapshot() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let first =
        Workspace::from_panels(vec![Panel::new(PanelId::new(1), "One", RectDip::default())])
            .unwrap();
    store.save_workspace(&first).unwrap();
    store.save_workspace(&Workspace::new()).unwrap();
    assert!(store.load_workspace().unwrap().panels().is_empty());
}

#[test]
fn rejects_incompatible_structure_without_modifying_data() {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL); INSERT INTO metadata VALUES ('sentinel','keep');").unwrap();
    assert!(super::initialize_schema(&connection).is_err());
    let value: String = connection
        .query_row("SELECT value FROM metadata WHERE key='sentinel'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(value, "keep");
    let count: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn size_sort_upgrades_existing_databases_and_survives_reopen() {
    for legacy in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("workspace.db");
        let connection = Connection::open(&path).unwrap();
        let schema = if legacy {
            super::schema::SCHEMA.replace("sort_column BETWEEN 0 AND 3", "sort_column BETWEEN 0 AND 2")
        } else {
            super::schema::SCHEMA.to_owned()
        };
        connection.execute_batch(&schema).unwrap();
        // Seed through the old store directly so the upgrade is exercised with
        // real mappings, view options, and pre-existing sort values.
        let mut old = WorkspaceStore { connection, config: None, on_change: None, committed_changes: std::cell::Cell::new(0) };
        let mut panel = Panel::new(PanelId::new(2), "Folder", RectDip::default());
        panel.set_folder(Some(std::path::PathBuf::from(r"C:\Downloads")));
        panel.set_list_view(false);
        let workspace = Workspace::from_panels(vec![panel]).unwrap();
        old.save_workspace(&workspace).unwrap();
        old.save_preference("panel_folder_sort:2", "2:desc").unwrap();
        old.connection.execute("INSERT INTO metadata VALUES (?1,?2)", ["panel_folder_columns:2", "0.4,0.2,0.3,0.1"]).unwrap();
        drop(old);
        let mut store = WorkspaceStore::open_database(&path).unwrap();
        assert_eq!(store.load_workspace().unwrap(), workspace);
        assert_eq!(store.preference("panel_folder_sort:2").unwrap().as_deref(), Some("2:desc"));
        for value in ["3:asc", "3:desc", "0:asc", "3:desc"] {
            store.save_preference("panel_folder_sort:2", value).unwrap();
            store.save_workspace(&workspace).unwrap();
            drop(store);
            store = WorkspaceStore::open_database(&path).unwrap();
            assert_eq!(store.preference("panel_folder_sort:2").unwrap().as_deref(), Some(value));
            assert_eq!(store.load_workspace().unwrap(), workspace);
        }
        for value in ["4:asc", "-1:asc", "3:invalid"] {
            assert!(store.save_preference("panel_folder_sort:2", value).is_err());
        }
        assert_eq!(store.preference("panel_folder_sort:2").unwrap().as_deref(), Some("3:desc"));
        assert_eq!(store.preference("panel_folder_columns:2").unwrap().as_deref(), Some("0.400000,0.200000,0.300000,0.100000"));
        store.save_workspace(&Workspace::new()).unwrap();
        assert_eq!(store.preference("panel_folder_sort:2").unwrap(), None);
    }
}

#[test]
fn folder_sort_upgrade_does_not_modify_an_incompatible_database() {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(&super::schema::SCHEMA.replace(
        "sort_column BETWEEN 0 AND 3", "sort_column BETWEEN 0 AND 2"
    )).unwrap();
    connection.execute_batch("ALTER TABLE monitor_layouts ADD COLUMN unexpected TEXT;").unwrap();
    let snapshot = || connection.prepare("SELECT sql FROM sqlite_schema ORDER BY name").unwrap()
        .query_map([], |row| row.get::<_, Option<String>>(0)).unwrap()
        .collect::<Result<Vec<_>, _>>().unwrap();
    let before = snapshot();
    assert!(super::initialize_schema(&connection).is_err());
    assert_eq!(snapshot(), before);
}

#[test]
fn option_update_does_not_rewrite_workspace_rows() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let workspace = Workspace::from_panels(vec![Panel::new(
        PanelId::new(1),
        "Keep",
        RectDip::default(),
    )])
    .unwrap();
    store.save_workspace(&workspace).unwrap();
    store.connection.execute_batch("CREATE TRIGGER forbid_panel_delete BEFORE DELETE ON panels BEGIN SELECT RAISE(ABORT, 'unexpected workspace rewrite'); END;").unwrap();
    let options = luciddesk_core::PaneOptions {
        corner_radius: 24.0,
        ..luciddesk_core::PaneOptions::DEFAULT
    };
    store.save_pane_options(options).unwrap();
    let restored = store.load_workspace().unwrap();
    assert_eq!(restored.panels(), workspace.panels());
    assert_eq!(restored.pane_options(), options);
}

#[test]
fn pane_options_round_trip_without_panels() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    assert_eq!(
        store.load_workspace().unwrap().pane_options(),
        luciddesk_core::PaneOptions::DEFAULT
    );
    for bits in 0..8 {
        let options = luciddesk_core::PaneOptions {
            corner_radius: if bits & 1 != 0 { 24.0 } else { 0.0 },
            border: bits & 2 != 0,
            snap: bits & 4 != 0,
            text: luciddesk_core::PanelText::Auto,
            text_protection: true,
            ..luciddesk_core::PaneOptions::DEFAULT
        };
        let mut workspace = Workspace::new();
        workspace.set_pane_options(options);
        store.save_workspace(&workspace).unwrap();
        let reopened = WorkspaceStore::from_connection(store.connection).unwrap();
        assert_eq!(reopened.load_workspace().unwrap().pane_options(), options);
        store = reopened;
    }
}

#[test]
fn panel_text_modes_round_trip_and_legacy_options_default_to_auto() {
    use luciddesk_core::PanelText;
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    store
        .save_preference("pane_options", "6|true|false")
        .unwrap();
    assert_eq!(
        store.load_workspace().unwrap().pane_options().text,
        PanelText::Auto
    );
    for value in ["6|true|false", "6|true|false|dark"] {
        store.save_preference("pane_options", value).unwrap();
        assert!(
            !store
                .load_workspace()
                .unwrap()
                .pane_options()
                .text_protection
        );
    }
    for (text, text_protection) in [PanelText::Auto, PanelText::Light, PanelText::Dark]
        .into_iter()
        .flat_map(|text| [true, false].map(|enabled| (text, enabled)))
    {
        let options = luciddesk_core::PaneOptions {
            text,
            text_protection,
            ..Default::default()
        };
        store.save_pane_options(options).unwrap();
        let workspace = store.load_workspace().unwrap();
        assert_eq!(workspace.pane_options(), options);
        store.save_workspace(&workspace).unwrap();
        assert_eq!(store.load_workspace().unwrap().pane_options(), options);
    }
}

#[test]
fn fractional_corner_radius_round_trips_and_rejects_non_finite_values() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let options = luciddesk_core::PaneOptions {
        corner_radius: 6.375,
        ..Default::default()
    };
    store.save_pane_options(options).unwrap();
    let workspace = store.load_workspace().unwrap();
    assert_eq!(workspace.pane_options().corner_radius, 6.375);
    store.save_workspace(&workspace).unwrap();
    assert_eq!(
        store.load_workspace().unwrap().pane_options().corner_radius,
        6.375
    );
    for radius in ["NaN", "inf", "-0.5", "24.1"] {
        store
            .save_preference("pane_options", &format!("{radius}|true|true"))
            .unwrap();
        assert!(store.load_workspace().is_err());
    }
}

#[test]
fn current_database_reopens_without_reinitializing_it() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let workspace = Workspace::from_panels(vec![Panel::new(
        PanelId::new(1),
        "Current",
        RectDip::default(),
    )])
    .unwrap();
    store.save_workspace(&workspace).unwrap();
    let reopened = WorkspaceStore::from_connection(store.connection).unwrap();
    assert_eq!(reopened.load_workspace().unwrap(), workspace);
}
#[test]
fn successful_changes_notify_without_notifying_for_noops_or_failed_writes() {
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let notified = count.clone();
    store.set_change_callback(move || { notified.fetch_add(1, Ordering::Relaxed); });
    store.save_preference("notification-test", "one").unwrap();
    assert_eq!(count.load(Ordering::Relaxed), 1);
    store.save_preference("notification-test", "one").unwrap();
    assert_eq!(count.load(Ordering::Relaxed), 1);
    store.connection.execute_batch("PRAGMA query_only=ON").unwrap();
    assert!(store.save_preference("notification-test", "two").is_err());
    assert_eq!(count.load(Ordering::Relaxed), 1);
    store.connection.execute_batch("PRAGMA query_only=OFF").unwrap();
    let snapshot = store.backup_snapshot().unwrap();
    snapshot.save_preference("notification-test", "snapshot").unwrap();
    assert_eq!(count.load(Ordering::Relaxed), 1, "background copies must not notify the live store");
}

#[test]
fn combined_geometry_save_is_one_commit_and_unchanged_save_is_read_only() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("workspace.db");
    let mut store = WorkspaceStore::open(&path).unwrap();
    let id = PanelId::new(1);
    let mut workspace = Workspace::from_panels(vec![Panel::new(id,"Test",RectDip::default())]).unwrap();
    store.save_workspace(&workspace).unwrap();
    workspace = store.load_workspace().unwrap();
    let observer = rusqlite::Connection::open(&path).unwrap();
    let version = || observer.query_row("PRAGMA data_version", [], |row| row.get::<_,u64>(0)).unwrap();
    let before = version();
    let rect = RectDip::new(100.0,200.0,480.0,360.0);
    workspace.panel_mut(id).unwrap().set_rect(rect);
    store.save_workspace_with_layout(&workspace,Some(("single",&[(id,rect)]))).unwrap();
    assert_eq!(version(),before+1);
    let changed = store.change_count();
    let bytes = std::fs::read(&path).unwrap();
    let timestamp = std::fs::metadata(&path).unwrap().modified().unwrap();
    store.save_workspace_with_layout(&workspace,Some(("single",&[(id,rect)]))).unwrap();
    assert_eq!(version(),before+1);
    assert_eq!(store.change_count(),changed);
    assert_eq!(std::fs::read(&path).unwrap(),bytes);
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(),timestamp);
    let original = store.load_workspace().unwrap();
    workspace.panel_mut(id).unwrap().set_title("Should roll back");
    assert!(store.save_workspace_with_layout(&workspace,Some(("single",&[(PanelId::new(999),rect)]))).is_err());
    assert_eq!(store.load_workspace().unwrap(),original);
    assert_eq!(store.monitor_layout("single").unwrap(),vec![(id,rect)]);
    assert_eq!(version(),before+1);
}

#[test]
fn sqlite_creation_leaves_existing_json_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let json = directory.path().join("workspace.json");
    std::fs::write(&json,b"existing JSON is preserved").unwrap();
    let path = directory.path().join("workspace.db");
    let store = WorkspaceStore::open(&path).unwrap();
    assert!(store.load_workspace().unwrap().panels().is_empty());
    assert!(std::fs::read(path).unwrap().starts_with(b"SQLite format 3\0"));
    assert_eq!(std::fs::read(json).unwrap(),b"existing JSON is preserved");
}


#[test]
fn legacy_layout_index_is_added_once_without_changing_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("workspace.db");
    let mut store = WorkspaceStore::open_database(&path).unwrap();
    let id = PanelId::new(1);
    let rect = RectDip::default();
    let workspace = Workspace::from_panels(vec![Panel::new(id, "Keep", rect)]).unwrap();
    store.save_workspace_with_layout(&workspace, Some(("dual", &[(id, rect)]))).unwrap();
    store.connection.execute_batch("DROP INDEX monitor_layouts_panel").unwrap();
    drop(store);
    let store = WorkspaceStore::open_database(&path).unwrap();
    assert_eq!(store.load_workspace().unwrap(), workspace);
    assert_eq!(store.monitor_layout("dual").unwrap(), vec![(id, rect)]);
    let columns: Vec<String> = store.connection.prepare("PRAGMA index_info(monitor_layouts_panel)").unwrap()
        .query_map([], |r| r.get(2)).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(columns, vec!["panel_id"]);
    drop(store);
    let bytes = std::fs::read(&path).unwrap();
    let store = WorkspaceStore::open_database(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(store.change_count(), 0);
}

#[test]
fn missing_layout_index_does_not_partially_upgrade_incompatible_schema() {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection.execute_batch(super::schema::SCHEMA).unwrap();
    connection.execute_batch("DROP INDEX monitor_layouts_panel; DROP INDEX desktop_items_pane;").unwrap();
    assert!(super::schema::validate_and_upgrade(&connection).is_err());
    let count: i64 = connection.query_row("SELECT count(*) FROM sqlite_schema WHERE name='monitor_layouts_panel'", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn unchanged_appearance_and_layout_do_not_write_or_notify() {
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let id = PanelId::new(1);
    let rect = RectDip::default();
    let mut workspace = Workspace::from_panels(vec![Panel::new(id, "Test", rect)]).unwrap();
    workspace.set_appearance_defaults(luciddesk_core::PanelTheme::Dark, Backdrop::Mica);
    store.save_workspace_with_layout(&workspace, Some(("single", &[(id, rect)]))).unwrap();
    let changes = store.change_count();
    let count = Arc::new(AtomicUsize::new(0));
    let notified = count.clone();
    store.set_change_callback(move || { notified.fetch_add(1, Ordering::Relaxed); });
    for _ in 0..10 {
        store.save_workspace_with_layout(&workspace, Some(("single", &[(id, rect)]))).unwrap();
        store.save_monitor_layout("single", &[(id, rect)]).unwrap();
    }
    assert_eq!(store.change_count(), changes);
    assert_eq!(count.load(Ordering::Relaxed), 0);
    workspace.panel_mut(id).unwrap().set_title("Changed");
    store.save_workspace(&workspace).unwrap();
    assert_eq!(store.change_count(), changes + 1, "only the renamed panel should change");
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[test]
fn deleting_panel_cascades_layouts_and_cleans_only_its_preferences() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let id = PanelId::new(1);
    let rect = RectDip::default();
    let mut panel = Panel::new(id, "Folder", rect);
    panel.set_folder(Some(std::path::PathBuf::from("C:/Example")));
    store.save_workspace(&Workspace::from_panels(vec![panel]).unwrap()).unwrap();
    for topology in ["single", "dual"] {
        store.save_monitor_layout(topology, &[(id, rect)]).unwrap();
    }
    for key in ["panel_desktop_list:1", "panel_folder_columns:1", "panel_folder_visible_columns:1", "panel_folder_columns:11"] {
        store.save_preference(key, match key { "panel_folder_columns:1"=>"0.4,0.2,0.3,0.1", "panel_folder_visible_columns:1"=>"15", _=>"keep until removed" }).unwrap();
    }
    store.save_workspace(&Workspace::new()).unwrap();
    for topology in ["single", "dual"] {
        assert!(store.monitor_layout(topology).unwrap().is_empty());
    }
    for key in ["panel_desktop_list:1", "panel_folder_columns:1", "panel_folder_visible_columns:1", "panel_folder_sort:1"] {
        assert_eq!(store.preference(key).unwrap(), None);
    }
    assert_eq!(store.preference("panel_folder_columns:11").unwrap().as_deref(), Some("keep until removed"));
    assert_eq!(store.connection.query_row("SELECT count(*) FROM panel_folder_settings", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}


#[test]
fn failed_commit_does_not_advance_backup_revision() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let id = PanelId::new(1);
    let rect = RectDip::default();
    let mut workspace = Workspace::from_panels(vec![Panel::new(id, "Original", rect)]).unwrap();
    store.save_workspace(&workspace).unwrap();
    let before = store.change_count();
    workspace.panel_mut(id).unwrap().set_title("Rolled back");
    assert!(store.save_workspace_with_layout(&workspace, Some(("test", &[(PanelId::new(999), rect)]))).is_err());
    assert_eq!(store.change_count(), before);
    workspace.panel_mut(id).unwrap().set_title("Original");
    store.save_workspace(&workspace).unwrap();
    assert_eq!(store.change_count(), before, "a later no-op must not count rolled-back changes");
    workspace.panel_mut(id).unwrap().set_title("Committed");
    store.save_workspace(&workspace).unwrap();
    assert_eq!(store.change_count(), before + 1);
}

#[test]
fn metadata_batch_commits_once_and_rolls_back_as_a_unit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workspace.db");
    let store = WorkspaceStore::open(&path).unwrap();
    let observer = rusqlite::Connection::open(&path).unwrap();
    let version = || observer.query_row("PRAGMA data_version", [], |r| r.get::<_, u64>(0)).unwrap();
    let before = version();
    store.save_metadata_preferences(&[("a", "one"), ("b", "two"), ("c", "three")]).unwrap();
    assert_eq!(version(), before + 1);
    let changes = store.change_count();
    store.save_metadata_preferences(&[("a", "one"), ("b", "two")]).unwrap();
    assert_eq!(version(), before + 1);
    assert_eq!(store.change_count(), changes);
    store.connection.execute_batch("CREATE TEMP TRIGGER reject_b BEFORE UPDATE ON metadata WHEN NEW.key='b' BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(store.save_metadata_preferences(&[("a", "changed"), ("b", "changed")]).is_err());
    assert_eq!(store.preference("a").unwrap().as_deref(), Some("one"));
    assert_eq!(store.change_count(), changes);
    assert_eq!(version(), before + 1);
    assert!(store.save_metadata_preferences(&[("a", "changed"), ("language", "en-US")]).is_err());
    assert_eq!(store.preference("a").unwrap().as_deref(), Some("one"));
}

#[test]
fn tab_selection_changes_only_tab_metadata_and_rejects_stale_selection() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let one = PanelId::new(1);
    let two = PanelId::new(2);
    let mut workspace = Workspace::from_panels(vec![Panel::new(one, "One", RectDip::default()), Panel::new(two, "Two", RectDip::default())]).unwrap();
    workspace.set_tab_groups(vec![luciddesk_core::PaneTabs { members: vec![one, two], active: one }]).unwrap();
    store.save_workspace(&workspace).unwrap();
    store.connection.execute_batch("CREATE TEMP TRIGGER no_panel_update BEFORE UPDATE ON panels BEGIN SELECT RAISE(ABORT,'unexpected panel update'); END;").unwrap();
    let before = store.change_count();
    store.save_active_tab(one, two).unwrap();
    assert_eq!(store.change_count(), before + 1);
    assert_eq!(store.load_workspace().unwrap().tab_groups()[0].active, two);
    store.save_active_tab(two, two).unwrap();
    assert_eq!(store.change_count(), before + 1);
    assert!(store.save_active_tab(one, two).is_err());
    assert!(store.save_active_tab(two, PanelId::new(999)).is_err());
    assert_eq!(store.change_count(), before + 1);
    store.save_active_tab(two, one).unwrap();
    assert_eq!(store.load_workspace().unwrap(), workspace);
}

#[test]
fn folder_view_is_structured_noop_safe_and_cascades() {
    let mut store=WorkspaceStore::open_in_memory().unwrap();
    let id=PanelId::new(1);let mut panel=Panel::new(id,"folder",RectDip::default());panel.set_folder(Some("C:/test".into()));
    store.save_workspace(&Workspace::from_panels(vec![panel]).unwrap()).unwrap();
    store.save_preference("panel_folder_columns:1","0.4,0.2,0.3,0.1").unwrap();
    store.save_preference("panel_folder_visible_columns:1","7").unwrap();
    let count=store.change_count();
    store.save_preference("panel_folder_columns:1","0.400000,0.200000,0.300000,0.100000").unwrap();
    store.save_preference("panel_folder_visible_columns:1","7").unwrap();
    assert_eq!(store.change_count(),count);
    assert!(store.save_preference("panel_folder_columns:1","NaN,0.2,0.3,0.1").is_err());
    assert!(store.save_preference("panel_folder_visible_columns:1","2").is_err());
    assert_eq!(store.connection.query_row("SELECT count(*) FROM metadata WHERE key LIKE 'panel_folder_%'",[],|r|r.get::<_,u64>(0)).unwrap(),0);
    store.save_workspace(&Workspace::new()).unwrap();
    assert_eq!(store.connection.query_row("SELECT count(*) FROM panel_folder_view",[],|r|r.get::<_,u64>(0)).unwrap(),0);
}
#[test]
fn geometry_only_save_preserves_other_state_rolls_back_and_skips_disk_writes() {
    let dir=tempfile::tempdir().unwrap();let path=dir.path().join("workspace.db");let mut store=WorkspaceStore::open(&path).unwrap();
    let id=PanelId::new(1);let mut panel=Panel::new(id,"Search",RectDip::default());panel.set_search(true);
    store.save_workspace(&Workspace::from_panels(vec![panel]).unwrap()).unwrap();
    let rect=RectDip::new(10.0,20.0,400.0,200.0);let entries=[(id,rect)];
    store.save_panel_geometry(&entries,Some(("single",&entries))).unwrap();
    let count=store.change_count();let bytes=std::fs::read(&path).unwrap();let config=std::fs::read(dir.path().join("config.toml")).unwrap();
    for _ in 0..20 {store.save_panel_geometry(&entries,Some(("single",&entries))).unwrap();}
    assert_eq!(store.change_count(),count);assert_eq!(std::fs::read(&path).unwrap(),bytes);assert_eq!(std::fs::read(dir.path().join("config.toml")).unwrap(),config);
    let bad=[(id,RectDip::new(99.0,99.0,500.0,300.0)),(PanelId::new(999),rect)];
    assert!(store.save_panel_geometry(&bad,None).is_err());assert_eq!(store.change_count(),count);
    let loaded=store.load_workspace().unwrap();assert_eq!(loaded.panel(id).unwrap().rect(),rect);assert!(loaded.panel(id).unwrap().is_search());assert_eq!(loaded.panel(id).unwrap().title(),"Search");
}
#[test]
fn folder_view_survives_backup_restore() {
    let dir=tempfile::tempdir().unwrap();let backup=dir.path().join("backup.db");let mut store=WorkspaceStore::open_in_memory().unwrap();
    let mut panel=Panel::new(PanelId::new(1),"folder",RectDip::default());panel.set_folder(Some("C:/test".into()));
    store.save_workspace(&Workspace::from_panels(vec![panel]).unwrap()).unwrap();
    store.save_preference("panel_folder_visible_columns:1","3").unwrap();store.export_backup(&backup).unwrap();
    store.save_preference("panel_folder_visible_columns:1","15").unwrap();store.restore_backup(&backup).unwrap();
    assert_eq!(store.preference("panel_folder_visible_columns:1").unwrap().as_deref(),Some("3"));
}

#[test]
fn workspace_and_folder_preferences_commit_together_and_rollback_invalid_batches() {
    let mut store=WorkspaceStore::open_in_memory().unwrap();
    let mut workspace=Workspace::new();
    let mut panel=Panel::new(PanelId::new(1),"mapped",RectDip::default());
    panel.set_folder(Some(std::env::temp_dir()));
    workspace.add_panel(panel).unwrap();
    let valid=crate::FolderPreferences{sort_column:2,descending:true,column_widths:Some([0.4,0.2,0.2,0.2]),visible_columns:3};
    let invalid=crate::FolderPreferences{visible_columns:2,..valid.clone()};
    assert!(store.save_workspace_with_folder_preferences(&workspace,None,&[(PanelId::new(1),invalid)]).is_err());
    assert!(store.load_workspace().unwrap().panels().is_empty());
    store.save_workspace_with_folder_preferences(&workspace,None,&[(PanelId::new(1),valid.clone())]).unwrap();
    assert_eq!(store.folder_preferences(PanelId::new(1)).unwrap(),valid);
    let count=store.change_count();
    store.save_workspace_with_folder_preferences(&workspace,None,&[(PanelId::new(1),valid)]).unwrap();
    assert_eq!(store.change_count(),count);
}

#[test]
fn measured_small_panel_bounds_survive_database_roundtrip() {
    let mut store = WorkspaceStore::open_in_memory().unwrap();
    let mut workspace = Workspace::new();
    let id = PanelId::new(1);
    let mut panel = Panel::new(id, "Single icon", RectDip::default());
    let measured = RectDip::from_bounds(-100.0, 40.0, 112.0, 135.0);
    panel.set_rect(measured);
    workspace.add_panel(panel).unwrap();
    store.save_workspace(&workspace).unwrap();
    assert_eq!(store.load_workspace().unwrap().panel(id).unwrap().rect(), measured);
    let count = store.change_count();
    store.save_workspace(&store.load_workspace().unwrap()).unwrap();
    assert_eq!(store.change_count(), count);
}

#[test]
fn released_schemas_preserve_workspace_and_upgrade_once() {
    for schema in [include_str!("fixtures/v0.8.1.sql"), include_str!("fixtures/v0.19.1.sql"), include_str!("fixtures/v0.20.4.sql")] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("workspace.db");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(schema).unwrap();
        db.execute_batch("INSERT INTO panels(id,title,x,y,width,height,collapsed,locked,backdrop_kind,kind)
            VALUES(1,'Files',10,20,400,300,0,0,'mica','folder');
            INSERT INTO panel_folder_settings(panel_id,path,sort_column) VALUES(1,'C:/Files',2);
            INSERT INTO metadata VALUES('keep','original');").unwrap();
        drop(db);
        let store = WorkspaceStore::open_database(&path).unwrap();
        let workspace = store.load_workspace().unwrap();
        assert_eq!(workspace.panels().len(), 1);
        assert_eq!(workspace.panels()[0].title(), "Files");
        assert_eq!(workspace.panels()[0].folder(), Some(std::path::Path::new("C:/Files")));
        assert_eq!(store.preference("keep").unwrap().as_deref(), Some("original"));
        assert_eq!(store.preference("panel_folder_sort:1").unwrap().as_deref(), Some("2:asc"));
        drop(store);
        let bytes = std::fs::read(&path).unwrap();
        let reopened = WorkspaceStore::open_database(&path).unwrap();
        assert_eq!(reopened.load_workspace().unwrap(), workspace);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
