//! Tab grouping is workspace metadata; existing panel/content IDs stay intact.
use super::StoreError;
use luciddesk_core::{PaneTabs, PanelId, Workspace};
use rusqlite::{Connection, OptionalExtension};

pub(super) fn load(connection: &Connection, workspace: &mut Workspace) -> Result<(), StoreError> {
    let raw: Option<String> = connection
        .query_row(
            "SELECT value FROM metadata WHERE key='pane_tabs_v1'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw.filter(|raw| !raw.is_empty()) else {
        return Ok(());
    };
    let invalid = || StoreError::InvalidData("invalid pane tabs".into());
    let id = |raw: &str| {
        raw.parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .map(PanelId::new)
            .ok_or_else(invalid)
    };
    let groups = raw
        .split(';')
        .map(|group| {
            let (active, members) = group.split_once(':').ok_or_else(invalid)?;
            Ok(PaneTabs {
                active: id(active)?,
                members: members.split(',').map(id).collect::<Result<_, _>>()?,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    workspace
        .set_tab_groups(groups)
        .map_err(|error| StoreError::InvalidData(error.to_string()))
}

pub(super) fn save(connection: &Connection, workspace: &Workspace) -> Result<(), StoreError> {
    let value = workspace
        .tab_groups()
        .iter()
        .map(|group| {
            format!(
                "{}:{}",
                group.active.get(),
                group
                    .members
                    .iter()
                    .map(|id| id.get().to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(";");
    if value.is_empty() {
        connection.execute("DELETE FROM metadata WHERE key='pane_tabs_v1'", [])?;
    } else {
        connection.execute("INSERT INTO metadata(key,value) VALUES ('pane_tabs_v1',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value WHERE value != excluded.value", [&value])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorkspaceStore;
    use luciddesk_core::{Panel, RectDip};

    #[test]
    fn tabs_round_trip_without_rewriting_unchanged_groups_and_reject_bad_references() {
        let mut store = WorkspaceStore::open_in_memory().unwrap();
        let mut workspace = Workspace::new();
        for id in 1..=3 {
            workspace
                .add_panel(Panel::new(
                    PanelId::new(id),
                    format!("Tab {id}"),
                    RectDip::default(),
                ))
                .unwrap();
        }
        workspace
            .set_tab_groups(vec![PaneTabs {
                members: vec![PanelId::new(3), PanelId::new(1), PanelId::new(2)],
                active: PanelId::new(1),
            }])
            .unwrap();
        store.save_workspace(&workspace).unwrap();
        assert_eq!(store.load_workspace().unwrap(), workspace);
        let before = store.change_count();
        store.save_workspace(&workspace).unwrap();
        assert_eq!(store.change_count(), before);
        assert_eq!(
            store.backup_snapshot().unwrap().load_workspace().unwrap(),
            workspace
        );
        workspace.panel_mut(PanelId::new(3)).unwrap().set_folder(Some("C:/Files".into()));
        store.save_workspace(&workspace).unwrap();
        store.save_preference("pane_tabs_v1", "1:1,3").unwrap();
        assert!(store.load_workspace().is_err());
        assert_eq!(store.preference("pane_tabs_v1").unwrap().as_deref(), Some("1:1,3"));
        store.save_preference("pane_tabs_v1", "1:1,99").unwrap();
        assert!(store.load_workspace().is_err());
    }
}

impl super::WorkspaceStore {
    /// Persists only the active member of an existing tab group.
    /// # Errors
    /// Rejects stale selection or a target outside the saved group.
    pub fn save_active_tab(&self, from: PanelId, to: PanelId) -> Result<(), StoreError> {
        let previous = self.raw_change_count();
        let tx = self.connection.unchecked_transaction()?;
        let raw: String = tx.query_row("SELECT value FROM metadata WHERE key='pane_tabs_v1'", [], |r| r.get(0))?;
        let invalid = || StoreError::InvalidData("saved tab group changed; reload before selecting".into());
        let mut found = false;
        let mut groups = Vec::new();
        for group in raw.split(';') {
            let (active, members) = group.split_once(':').ok_or_else(invalid)?;
            let active = active.parse::<u64>().map_err(|_| invalid())?;
            let ids = members.split(',').map(str::parse::<u64>).collect::<Result<Vec<_>, _>>().map_err(|_| invalid())?;
            if ids.contains(&from.get()) {
                if found || active != from.get() || !ids.contains(&to.get()) {
                    return Err(invalid());
                }
                found = true;
                groups.push(format!("{}:{members}", to.get()));
            } else {
                groups.push(group.to_owned());
            }
        }
        if !found { return Err(invalid()); }
        let value = groups.join(";");
        tx.execute("UPDATE metadata SET value=?1 WHERE key='pane_tabs_v1' AND value IS NOT ?1", [&value])?;
        tx.commit()?;
        self.notify_change(previous);
        Ok(())
    }
}
