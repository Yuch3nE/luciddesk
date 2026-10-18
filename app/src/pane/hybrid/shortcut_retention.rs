//! Briefly preserve placement while an updater replaces a managed shortcut.
use super::*;
const TTL: Duration = Duration::from_secs(300);

#[derive(Default)]
pub(super) struct ShortcutRetention {
    missing: Vec<(DesktopItem, Instant)>,
}

impl ShortcutRetention {
    fn eligible(workspace: &Workspace, item: &DesktopItem) -> bool {
        item.identity().file_system_path().is_some_and(|path|
            path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("lnk")))
            && matches!(item.placement(), DesktopPlacement::Pane { pane_id, .. }
                if workspace.panel(*pane_id).is_some_and(Panel::supports_tabs))
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.missing.iter().map(|(_, expires)| *expires).min()
    }

    pub(super) fn expire(&mut self, workspace: &Workspace, now: Instant) {
        self.missing.retain(|(item, expires)| *expires > now && Self::eligible(workspace, item));
        if self.missing.is_empty() { self.missing = Vec::new(); }
    }

    pub(super) fn reconcile(
        &mut self, workspace: &mut Workspace,
        inventory: impl IntoIterator<Item = DesktopItem>, now: Instant,
    ) {
        self.expire(workspace, now);
        let mut incoming: Vec<_> = inventory.into_iter().collect();
        for item in workspace.desktop_items() {
            if Self::eligible(workspace, item)
                && !incoming.iter().any(|next| item.identity().equivalent_to(next.identity()))
            {
                // A recreated file has a new file ID. Remember only its path.
                let path = item.identity().file_system_path().unwrap().to_owned();
                let identity = ShellIdentity::FileSystem { path, volume_id: None, file_id: None };
                self.missing.retain(|(old, _)| !old.identity().equivalent_to(&identity));
                let mut remembered = DesktopItem::new(identity, item.display_name());
                remembered.set_placement(item.placement().clone());
                remembered.set_pane_position(item.pane_position());
                self.missing.push((remembered, now + TTL));
            }
        }
        for item in &mut incoming {
            if let Some(index) = self.missing.iter().position(|(old, _)|
                old.identity().equivalent_to(item.identity()))
            {
                let (old, _) = self.missing.remove(index);
                item.set_placement(old.placement().clone());
                item.set_pane_position(old.pane_position());
            }
        }
        workspace.reconcile_desktop_items(incoming);
        if self.missing.is_empty() { self.missing = Vec::new(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(path: &str, file_id: u128) -> DesktopItem {
        DesktopItem::new(ShellIdentity::FileSystem {
            path: path.into(), volume_id: Some(1), file_id: Some(file_id),
        }, "Shortcut")
    }
    fn workspace() -> Workspace {
        let mut workspace = Workspace::from_panels(vec![
            Panel::new(PanelId::new(1), "Group", RectDip::default()),
        ]).unwrap();
        let mut shortcut = item("Music.lnk", 1);
        shortcut.set_placement(DesktopPlacement::Pane {
            pane_id: PanelId::new(1), position: GridPosition::new(2, 3),
        });
        shortcut.set_pane_position(Some(luciddesk_core::PointDip::new(12.0, 24.0)));
        workspace.reconcile_desktop_items([shortcut]);
        workspace
    }
    #[test]
    fn updater_replacement_recovers_position_without_persisting_history() {
        let mut workspace = workspace();
        let original = workspace.desktop_items()[0].clone();
        let mut cache = ShortcutRetention::default();
        let now = Instant::now();
        let mut store = WorkspaceStore::open_in_memory().unwrap();
        store.save_workspace(&workspace).unwrap();
        cache.reconcile(&mut workspace, [], now);
        store.save_workspace(&workspace).unwrap();
        assert!(store.load_workspace().unwrap().desktop_items().is_empty());
        assert_eq!(store.load_workspace().unwrap(), workspace);
        // Repeated empty snapshots must not extend the deadline.
        cache.reconcile(&mut workspace, [], now + Duration::from_secs(60));
        assert_eq!(cache.deadline(), Some(now + TTL));
        cache.reconcile(&mut workspace, [item("Other.lnk", 1)], now + Duration::from_secs(90));
        assert!(matches!(workspace.desktop_items()[0].placement(), DesktopPlacement::FreeDesktop { .. }));
        cache.reconcile(&mut workspace, [item("Music.lnk", 2)], now + TTL - Duration::from_secs(1));
        assert_eq!(workspace.desktop_items()[0].placement(), original.placement());
        assert_eq!(workspace.desktop_items()[0].pane_position(), original.pane_position());
        assert_eq!(cache.deadline(), None);
        store.save_workspace(&workspace).unwrap();
        assert_eq!(store.load_workspace().unwrap(), workspace);
        // An explicitly released shortcut must not regain membership.
        workspace.desktop_items_mut()[0].set_placement(DesktopPlacement::default());
        cache.reconcile(&mut workspace, [], now);
        assert_eq!(cache.deadline(), None);
    }
    #[test]
    fn expiry_session_restart_and_panel_removal_forget_missing_shortcuts() {
        for scenario in 0..4 {
            let mut workspace = workspace();
            let mut cache = ShortcutRetention::default();
            let now = Instant::now();
            cache.reconcile(&mut workspace, [], now);
            match scenario {
                0 => cache.expire(&workspace, now + TTL),
                1 => cache = ShortcutRetention::default(),
                2 => { workspace.remove_panel(PanelId::new(1)); cache.expire(&workspace, now); }
                _ => {}
            }
            if scenario != 3 { assert_eq!(cache.deadline(), None); }
            cache.reconcile(&mut workspace, [item("Music.lnk", 2)], now + TTL);
            assert!(matches!(workspace.desktop_items()[0].placement(), DesktopPlacement::FreeDesktop { .. }));
            assert_eq!(cache.missing.capacity(), 0);
        }
    }
    #[test]
    fn ordinary_files_are_not_retained() {
        let mut workspace = workspace();
        let mut file = item("Notes.txt", 3);
        file.set_placement(workspace.desktop_items()[0].placement().clone());
        workspace.reconcile_desktop_items([file]);
        let mut cache = ShortcutRetention::default();
        cache.reconcile(&mut workspace, [], Instant::now());
        assert_eq!(cache.deadline(), None);
    }
}
