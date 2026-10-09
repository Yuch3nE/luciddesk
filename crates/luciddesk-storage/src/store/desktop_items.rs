//! Persistence of desktop identities and placement.
use crate::StoreError;
use luciddesk_core::{
    DesktopItem, DesktopPlacement, GridPosition, MonitorId, PanelId, PointDip, ShellIdentity,
};
use rusqlite::{Connection, Transaction, params};
use std::path::PathBuf;

pub(super) fn load_desktop_items(connection: &Connection) -> Result<Vec<DesktopItem>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT identity_kind, identity_value, volume_id, file_id, display_name,
                placement_kind, monitor_id, x, y, pane_id, grid_column, grid_row
         FROM desktop_items ORDER BY identity_key",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(PersistedDesktopItem {
            identity_kind: row.get(0)?,
            identity_value: row.get(1)?,
            volume_id: row.get(2)?,
            file_id: row.get(3)?,
            display_name: row.get(4)?,
            placement_kind: row.get(5)?,
            monitor_id: row.get(6)?,
            x: row.get(7)?,
            y: row.get(8)?,
            pane_id: row.get(9)?,
            grid_column: row.get(10)?,
            grid_row: row.get(11)?,
        })
    })?;
    rows.map(|row| {
        row.map_err(StoreError::from)
            .and_then(PersistedDesktopItem::into_item)
    })
    .collect()
}

pub(super) fn insert_desktop_items(
    transaction: &Transaction<'_>,
    items: &[DesktopItem],
) -> Result<(), StoreError> {
    let mut statement = transaction.prepare_cached("INSERT INTO desktop_items(
                 identity_key, identity_kind, identity_value, volume_id, file_id, display_name,
                 placement_kind, monitor_id, x, y, pane_id, grid_column, grid_row
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13) ON CONFLICT(identity_key) DO UPDATE SET identity_kind=excluded.identity_kind,identity_value=excluded.identity_value,volume_id=excluded.volume_id,file_id=excluded.file_id,display_name=excluded.display_name,placement_kind=excluded.placement_kind,monitor_id=excluded.monitor_id,x=excluded.x,y=excluded.y,pane_id=excluded.pane_id,grid_column=excluded.grid_column,grid_row=excluded.grid_row WHERE (identity_kind,identity_value,volume_id,file_id,display_name,placement_kind,monitor_id,x,y,pane_id,grid_column,grid_row) IS NOT (excluded.identity_kind,excluded.identity_value,excluded.volume_id,excluded.file_id,excluded.display_name,excluded.placement_kind,excluded.monitor_id,excluded.x,excluded.y,excluded.pane_id,excluded.grid_column,excluded.grid_row)")?;
    for item in items {
        let (identity_kind, identity_value, volume_id, file_id) = match item.identity() {
            ShellIdentity::FileSystem {
                path,
                volume_id,
                file_id,
            } => (
                "filesystem",
                path.as_os_str().to_string_lossy(),
                volume_id.map(|value| value.to_string()),
                file_id.map(|value| value.to_string()),
            ),
            ShellIdentity::Namespace { parsing_name } => {
                ("namespace", std::borrow::Cow::Borrowed(parsing_name.as_str()), None, None)
            }
        };
        let (placement_kind, monitor_id, x, y, pane_id, grid_column, grid_row) =
            match item.placement() {
                DesktopPlacement::FreeDesktop { monitor, position } => (
                    "free",
                    Some(monitor.as_str()),
                    Some(f64::from(position.x)),
                    Some(f64::from(position.y)),
                    None,
                    None,
                    None,
                ),
                DesktopPlacement::Pane { pane_id, position } => (
                    "pane",
                    None,
                    item.pane_position().map(|p| f64::from(p.x)),
                    item.pane_position().map(|p| f64::from(p.y)),
                    Some(i64::try_from(pane_id.get()).map_err(|_| {
                        StoreError::InvalidData("pane id exceeds SQLite range".into())
                    })?),
                    Some(i64::from(position.column)),
                    Some(i64::from(position.row)),
                ),
            };
        statement.execute(
            params![
                item.identity().persistent_key(),
                identity_kind,
                identity_value,
                volume_id,
                file_id,
                item.display_name(),
                placement_kind,
                monitor_id,
                x,
                y,
                pane_id,
                grid_column,
                grid_row,
            ],
        )?;
    }
    Ok(())
}

struct PersistedDesktopItem {
    identity_kind: String,
    identity_value: String,
    volume_id: Option<String>,
    file_id: Option<String>,
    display_name: String,
    placement_kind: String,
    monitor_id: Option<String>,
    x: Option<f32>,
    y: Option<f32>,
    pane_id: Option<i64>,
    grid_column: Option<i64>,
    grid_row: Option<i64>,
}

impl PersistedDesktopItem {
    fn into_item(self) -> Result<DesktopItem, StoreError> {
        let identity = match self.identity_kind.as_str() {
            "filesystem" => ShellIdentity::FileSystem {
                path: PathBuf::from(self.identity_value),
                volume_id: parse_optional(self.volume_id.as_deref(), "volume id")?,
                file_id: parse_optional(self.file_id.as_deref(), "file id")?,
            },
            "namespace" => ShellIdentity::Namespace {
                parsing_name: self.identity_value,
            },
            kind => {
                return Err(StoreError::InvalidData(format!(
                    "unknown desktop identity {kind}"
                )));
            }
        };
        let placement = match self.placement_kind.as_str() {
            "free" => DesktopPlacement::FreeDesktop {
                monitor: MonitorId::new(required(self.monitor_id, "monitor id")?),
                position: PointDip::new(
                    required(self.x, "desktop x")?,
                    required(self.y, "desktop y")?,
                ),
            },
            "pane" => DesktopPlacement::Pane {
                pane_id: PanelId::new(
                    u64::try_from(required(self.pane_id, "pane id")?)
                        .map_err(|_| StoreError::InvalidData("pane id is negative".into()))?,
                ),
                position: GridPosition::new(
                    u32::try_from(required(self.grid_column, "grid column")?).map_err(|_| {
                        StoreError::InvalidData("grid column is outside u32 range".into())
                    })?,
                    u32::try_from(required(self.grid_row, "grid row")?).map_err(|_| {
                        StoreError::InvalidData("grid row is outside u32 range".into())
                    })?,
                ),
            },
            kind => {
                return Err(StoreError::InvalidData(format!(
                    "unknown desktop placement {kind}"
                )));
            }
        };
        let mut item = DesktopItem::new(identity, self.display_name);
        item.set_placement(placement);
        if self.placement_kind == "pane" {
            match (self.x, self.y) {
                (Some(x), Some(y)) if x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0 => item.set_pane_position(Some(PointDip::new(x, y))),
                (None, None) => {},
                _ => return Err(StoreError::InvalidData("invalid pane coordinates".into())),
            }
        }
        Ok(item)
    }
}

fn parse_optional<T: std::str::FromStr>(
    value: Option<&str>,
    label: &str,
) -> Result<Option<T>, StoreError> {
    value
        .map(|value| {
            value
                .parse()
                .map_err(|_| StoreError::InvalidData(format!("invalid {label}")))
        })
        .transpose()
}

fn required<T>(value: Option<T>, label: &str) -> Result<T, StoreError> {
    value.ok_or_else(|| StoreError::InvalidData(format!("missing {label}")))
}
