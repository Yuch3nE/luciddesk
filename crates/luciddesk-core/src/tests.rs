use super::{
    DesktopItem, DesktopPlacement, GridPosition, Panel, PanelId, RectDip, ShellIdentity, Workspace,
    WorkspaceError, PaneTabs,
};
use std::path::PathBuf;

fn panel(id: u64) -> Panel {
    Panel::new(PanelId::new(id), format!("Panel {id}"), RectDip::default())
}

#[test]
fn panel_defaults_do_not_override_measured_window_bounds() {
    let raw = RectDip {
        x: -10.0,
        y: 20.0,
        width: 1.0,
        height: 2.0,
    };
    let created = Panel::new(PanelId::new(1), "Small", raw);
    let mut updated = panel(2);
    updated.set_rect(raw);
    assert_eq!(updated.rect(), raw);
    let measured = RectDip::from_bounds(-10.0, 20.0, 112.0, 135.0);
    updated.set_rect(measured);
    assert_eq!(updated.rect(), measured);
    assert_eq!(created.rect().width, RectDip::MIN_WIDTH);
    assert_eq!(created.rect().height, RectDip::MIN_HEIGHT);
    assert_eq!(created.rect(), RectDip::new(-10.0, 20.0, 1.0, 2.0));
}

#[test]
fn switching_content_sources_preserves_independent_preferences() {
    let mut panel = panel(1);
    let path = PathBuf::from(r"C:\folder");
    panel.set_folder(Some(path.clone()));
    panel.set_search(false);
    assert_eq!(panel.folder(), Some(path.as_path()));
    panel.set_list_view(false);
    panel.set_locked(true);
    panel.set_collapsed(true);
    panel.set_auto_hide(true);
    panel.set_search(true);
    assert!(panel.is_search());
    assert!(panel.folder().is_none());
    assert!(!panel.collapsed());
    assert!(!panel.auto_hide());
    assert!(panel.locked());
    assert!(!panel.list_view());
    panel.set_folder(None);
    assert!(panel.is_search());
    panel.set_search(false);
    assert!(!panel.is_search());
    assert!(panel.folder().is_none());
    panel.set_search(true);
    panel.set_folder(Some(path.clone()));
    assert!(!panel.is_search());
    assert_eq!(panel.folder(), Some(path.as_path()));
    panel.set_folder(None);
    assert!(!panel.is_search());
    assert!(panel.folder().is_none());
}

#[test]
fn workspace_rejects_duplicate_ids() {
    let result = Workspace::from_panels(vec![panel(7), panel(7)]);
    assert_eq!(result, Err(WorkspaceError::DuplicatePanel(PanelId::new(7))));
}

#[test]
fn desktop_reconciliation_preserves_membership_without_moving_files() {
    let identity = ShellIdentity::FileSystem {
        path: PathBuf::from(r"C:\Users\Test\Desktop\Editor.lnk"),
        volume_id: None,
        file_id: None,
    };
    let mut existing = DesktopItem::new(identity.clone(), "Old Editor Name");
    existing.set_placement(DesktopPlacement::Pane {
        pane_id: PanelId::new(7),
        position: GridPosition::new(2, 3),
    });
    let mut workspace = Workspace::new();
    workspace.reconcile_desktop_items([existing]);

    workspace.reconcile_desktop_items([
        DesktopItem::new(identity.clone(), "Editor"),
        DesktopItem::new(
            ShellIdentity::Namespace {
                parsing_name: "::{645FF040-5081-101B-9F08-00AA002F954E}".into(),
            },
            "Recycle Bin",
        ),
    ]);

    let editor = workspace.desktop_item(&identity).unwrap();
    assert_eq!(editor.display_name(), "Editor");
    assert_eq!(
        editor.placement(),
        &DesktopPlacement::Pane {
            pane_id: PanelId::new(7),
            position: GridPosition::new(2, 3),
        }
    );
    assert_eq!(
        identity.file_system_path(),
        Some(PathBuf::from(r"C:\Users\Test\Desktop\Editor.lnk").as_path())
    );
    assert_eq!(workspace.desktop_items().len(), 2);
}

#[test]
fn stable_file_identity_preserves_placement_across_a_rename() {
    let before = ShellIdentity::FileSystem {
        path: PathBuf::from(r"C:\Users\Test\Desktop\Before.txt"),
        volume_id: Some(11),
        file_id: Some(22),
    };
    let after = ShellIdentity::FileSystem {
        path: PathBuf::from(r"C:\Users\Test\Desktop\After.txt"),
        volume_id: Some(11),
        file_id: Some(22),
    };
    let placement = DesktopPlacement::Pane {
        pane_id: PanelId::new(3),
        position: GridPosition::new(1, 4),
    };
    let mut existing = DesktopItem::new(before, "Before");
    existing.set_placement(placement.clone());
    let mut workspace = Workspace::new();
    workspace.reconcile_desktop_items([existing]);

    workspace.reconcile_desktop_items([DesktopItem::new(after.clone(), "After")]);

    let renamed = workspace.desktop_item(&after).unwrap();
    assert_eq!(renamed.display_name(), "After");
    assert_eq!(renamed.placement(), &placement);
    assert_eq!(renamed.identity(), &after);
}

#[test]
fn tab_groups_validate_members_and_preserve_content_on_activation_and_removal() {
    let mut workspace = Workspace::new();
    for id in 1..=3 { workspace.add_panel(Panel::new(PanelId::new(id), format!("Tab {id}"), RectDip::default())).unwrap(); }
    let group = PaneTabs { members: vec![PanelId::new(1), PanelId::new(2)], active: PanelId::new(2) };
    workspace.set_tab_groups(vec![group.clone()]).unwrap();
    assert!(!workspace.tab_visible(PanelId::new(1)));
    assert!(workspace.tab_visible(PanelId::new(2)));
    let previous = workspace.clone();
    assert!(workspace.set_tab_groups(vec![group.clone(), group]).is_err());
    assert_eq!(workspace, previous);
    workspace.remove_panel(PanelId::new(2));
    assert!(workspace.tab_groups().is_empty());
    assert!(workspace.tab_visible(PanelId::new(1)));
    assert!(workspace.panel(PanelId::new(1)).is_some());
}

#[test]
fn folder_panels_cannot_join_tab_groups() {
    let mut workspace = Workspace::new();
    for id in 1..=2 {
        let mut panel = Panel::new(PanelId::new(id), "Panel", RectDip::default());
        if id == 2 { panel.set_folder(Some("C:/folder".into())); }
        workspace.add_panel(panel).unwrap();
    }
    let before = workspace.clone();
    assert!(workspace.set_tab_groups(vec![PaneTabs {
        members: vec![PanelId::new(1), PanelId::new(2)], active: PanelId::new(1),
    }]).is_err());
    assert_eq!(workspace, before);
}
