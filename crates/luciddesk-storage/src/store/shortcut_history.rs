//! Persist absent shortcuts without exposing them as live desktop items.
use crate::StoreError;
use luciddesk_core::{
    DesktopItem, DesktopPlacement, GridPosition, PanelId, PointDip, ShellIdentity,
};
use rusqlite::{Connection, Transaction, params};

pub(super) fn load(connection: &Connection) -> Result<Vec<DesktopItem>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT path,display_name,pane_id,grid_column,grid_row,x,y FROM shortcut_history ORDER BY path",
    )?;
    let rows = statement.query_map([], |row| {
        let mut item = DesktopItem::new(
            ShellIdentity::FileSystem {
                path: std::path::PathBuf::from(row.get::<_, String>(0)?),
                volume_id: None,
                file_id: None,
            },
            row.get::<_, String>(1)?,
        );
        item.set_placement(DesktopPlacement::Pane {
            pane_id: PanelId::new(row.get(2)?),
            position: GridPosition::new(row.get(3)?, row.get(4)?),
        });
        let x: Option<f32> = row.get(5)?;
        let y: Option<f32> = row.get(6)?;
        item.set_pane_position(x.zip(y).map(|(x, y)| PointDip::new(x, y)));
        Ok(item)
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub(super) fn save(transaction: &Transaction<'_>, items: &[DesktopItem]) -> Result<(), StoreError> {
    let old: Vec<String> = transaction
        .prepare("SELECT path FROM shortcut_history")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    let paths: std::collections::HashSet<_> = items
        .iter()
        .filter_map(|item| {
            item.identity()
                .file_system_path()
                .map(|path| path.to_string_lossy().into_owned())
        })
        .collect();
    for path in old {
        if !paths.contains(&path) {
            transaction
                .prepare_cached("DELETE FROM shortcut_history WHERE path=?1")?
                .execute([path])?;
        }
    }
    let mut statement = transaction.prepare_cached(
        "INSERT INTO shortcut_history(path,display_name,pane_id,grid_column,grid_row,x,y)
         VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(path) DO UPDATE SET
         display_name=excluded.display_name,pane_id=excluded.pane_id,grid_column=excluded.grid_column,
         grid_row=excluded.grid_row,x=excluded.x,y=excluded.y
         WHERE (display_name,pane_id,grid_column,grid_row,x,y) IS NOT
         (excluded.display_name,excluded.pane_id,excluded.grid_column,excluded.grid_row,excluded.x,excluded.y)",
    )?;
    for item in items {
        let DesktopPlacement::Pane { pane_id, position } = item.placement() else {
            continue;
        };
        let Some(path) = item.identity().file_system_path() else {
            continue;
        };
        statement.execute(params![
            path.to_string_lossy(),
            item.display_name(),
            pane_id.get(),
            position.column,
            position.row,
            item.pane_position().map(|p| p.x),
            item.pane_position().map(|p| p.y)
        ])?;
    }
    Ok(())
}
