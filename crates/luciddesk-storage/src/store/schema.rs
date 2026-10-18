use super::StoreError;
use rusqlite::{Connection, OptionalExtension};

pub(super) fn validate_and_upgrade(connection: &Connection) -> Result<(), StoreError> {
    let transaction = connection.unchecked_transaction()?;
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(SCHEMA)?;
    reference.execute_batch(super::folder_view::SCHEMA)?;
    let definitions: Vec<(String, String)> = reference
        .prepare("SELECT name,sql FROM sqlite_schema WHERE sql IS NOT NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let normalize = |sql: &str| {
        sql.split_whitespace()
            .collect::<String>()
            .to_ascii_lowercase()
    };
    let mut desktop_upgrade = None;
    let mut folder_upgrade = None;
    let mut layout_index = None;
    let mut folder_view = None;
    for (name, expected) in definitions {
        let actual: Option<String> = connection
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name=?1",
                [&name],
                |r| r.get(0),
            )
            .optional()?;
        let actual = actual.as_deref().map(normalize);
        let normalized_expected = normalize(&expected);
        if actual.as_deref() == Some(normalized_expected.as_str()) {
            continue;
        }
        if name == "desktop_items" && actual.as_deref() == Some(normalized_expected.replace(
            "((xisnullandyisnull)or(xisnotnullandyisnotnullandx>=0andy>=0))", "xisnullandyisnull").as_str()) {
            desktop_upgrade = Some(expected);
            continue;
        }
        if name == "panel_folder_view" && actual.is_none() {
            folder_view = Some(expected);
            continue;
        }
        if name == "monitor_layouts_panel" && actual.is_none() {
            layout_index = Some(expected);
            continue;
        }
        if name == "panel_folder_settings"
            && actual.as_deref() == Some(normalized_expected
                .replace("sort_columnbetween0and3", "sort_columnbetween0and2").as_str())
        {
            folder_upgrade = Some(expected);
        } else {
            return Err(StoreError::InvalidData(format!(
                "数据库结构不兼容（{name}），请使用新的开发数据目录。"
            )));
        }
    }
    // Apply known upgrades only after validating every table and index.
    // Keep existing mappings and sort settings in the same transaction.
    if let Some(create) = desktop_upgrade {
        transaction.execute_batch("ALTER TABLE desktop_items RENAME TO desktop_items_before_free_layout;")?;
        transaction.execute_batch(&create)?;
        transaction.execute_batch("INSERT INTO desktop_items SELECT * FROM desktop_items_before_free_layout;
            DROP TABLE desktop_items_before_free_layout;
            CREATE INDEX desktop_items_pane ON desktop_items(pane_id);")?;
    }
    if let Some(create) = folder_upgrade {
        transaction.execute_batch(
            "ALTER TABLE panel_folder_settings RENAME TO panel_folder_settings_before_size;",
        )?;
        transaction.execute_batch(&create)?;
        transaction.execute_batch(
            "INSERT INTO panel_folder_settings (panel_id,path,view,sort_column,sort_direction)
             SELECT panel_id,path,view,sort_column,sort_direction FROM panel_folder_settings_before_size;
             DROP TABLE panel_folder_settings_before_size;"
        )?;
    }
    if let Some(create) = layout_index {
        transaction.execute_batch(&create)?;
    }
    if let Some(create) = folder_view {
        transaction.execute_batch(&create)?;
        super::folder_view::absorb(&transaction)?;
    }
    transaction.commit()?;
    Ok(())
}

// Development schema: unrelated older structures remain unsupported. The known
// folder size-sort constraint and layout index are upgraded without resetting data.
pub(super) const SCHEMA: &str = "
CREATE TABLE metadata (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
CREATE TABLE panels (
    id INTEGER PRIMARY KEY CHECK(id>0), title TEXT NOT NULL,
    x REAL NOT NULL, y REAL NOT NULL, width REAL NOT NULL CHECK(width>0), height REAL NOT NULL CHECK(height>0),
    collapsed INTEGER NOT NULL CHECK(collapsed IN (0,1)), locked INTEGER NOT NULL CHECK(locked IN (0,1)),
    backdrop_kind TEXT NOT NULL CHECK(backdrop_kind IN ('mica','mica_alt','acrylic','mica_tuned','acrylic_tuned','translucent','solid')),
    opacity REAL CHECK(opacity BETWEEN 0 AND 1), color INTEGER CHECK(color BETWEEN 0 AND 16777215),
    theme TEXT NOT NULL DEFAULT 'system' CHECK(theme IN ('system','light','dark')),
    always_on_top INTEGER NOT NULL DEFAULT 0 CHECK(always_on_top IN (0,1)),
    auto_hide INTEGER NOT NULL DEFAULT 0 CHECK(auto_hide IN (0,1)),
    inherit_theme INTEGER NOT NULL DEFAULT 0 CHECK(inherit_theme IN (0,1)),
    inherit_backdrop INTEGER NOT NULL DEFAULT 0 CHECK(inherit_backdrop IN (0,1)),
    kind TEXT NOT NULL DEFAULT 'desktop' CHECK(kind IN ('desktop','folder','search'))
);
CREATE TABLE panel_folder_settings (
    panel_id INTEGER PRIMARY KEY REFERENCES panels(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    view TEXT NOT NULL DEFAULT 'list' CHECK(view IN ('list','icons')),
    sort_column INTEGER NOT NULL DEFAULT 0 CHECK(sort_column BETWEEN 0 AND 3),
    sort_direction TEXT NOT NULL DEFAULT 'asc' CHECK(sort_direction IN ('asc','desc'))
);
CREATE TABLE desktop_items (
    identity_key TEXT PRIMARY KEY NOT NULL,
    identity_kind TEXT NOT NULL CHECK(identity_kind IN ('filesystem','namespace')),
    identity_value TEXT NOT NULL, volume_id TEXT, file_id TEXT, display_name TEXT NOT NULL,
    placement_kind TEXT NOT NULL CHECK(placement_kind IN ('free','pane')),
    monitor_id TEXT, x REAL, y REAL,
    pane_id INTEGER REFERENCES panels(id) DEFERRABLE INITIALLY DEFERRED,
    grid_column INTEGER, grid_row INTEGER,
    CHECK((placement_kind='free' AND monitor_id IS NOT NULL AND x IS NOT NULL AND y IS NOT NULL AND pane_id IS NULL AND grid_column IS NULL AND grid_row IS NULL)
       OR (placement_kind='pane' AND pane_id IS NOT NULL AND grid_column>=0 AND grid_column IS NOT NULL AND grid_row>=0 AND grid_row IS NOT NULL AND monitor_id IS NULL AND ((x IS NULL AND y IS NULL) OR (x IS NOT NULL AND y IS NOT NULL AND x>=0 AND y>=0))))
);
CREATE INDEX desktop_items_pane ON desktop_items(pane_id);
CREATE TABLE monitor_layouts (
    topology TEXT NOT NULL, panel_id INTEGER NOT NULL REFERENCES panels(id) ON DELETE CASCADE,
    x REAL NOT NULL, y REAL NOT NULL, width REAL NOT NULL CHECK(width>0), height REAL NOT NULL CHECK(height>0),
    PRIMARY KEY(topology,panel_id)
);
CREATE INDEX monitor_layouts_panel ON monitor_layouts(panel_id);
";

pub(super) fn parse_sort(raw: &str) -> Result<(i64, &str), StoreError> {
    let (column, direction) = raw
        .split_once(':')
        .ok_or_else(|| StoreError::InvalidData("invalid folder sort".into()))?;
    let column = column
        .parse::<i64>()
        .ok()
        .filter(|c| (0..=3).contains(c))
        .ok_or_else(|| StoreError::InvalidData("invalid folder sort column".into()))?;
    if !["asc", "desc"].contains(&direction) {
        return Err(StoreError::InvalidData(
            "invalid folder sort direction".into(),
        ));
    }
    Ok((column, direction))
}

#[cfg(test)]
mod free_layout_tests {
    use super::*;
    #[test]
    fn upgrades_old_item_constraint_without_losing_positions_or_index() {
        let connection=Connection::open_in_memory().unwrap();
        let old=SCHEMA.replace("((x IS NULL AND y IS NULL) OR (x IS NOT NULL AND y IS NOT NULL AND x>=0 AND y>=0))", "x IS NULL AND y IS NULL");
        assert_ne!(old,SCHEMA);
        connection.execute_batch(&old).unwrap();
        connection.execute_batch("INSERT INTO desktop_items(identity_key,identity_kind,identity_value,display_name,placement_kind,monitor_id,x,y) VALUES ('sample','namespace','sample','sample','free','primary',10,20);").unwrap();
        validate_and_upgrade(&connection).unwrap();
        let position:(f32,f32)=connection.query_row("SELECT x,y FROM desktop_items WHERE identity_key='sample'",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(position,(10.0,20.0));
        assert_eq!(connection.query_row("SELECT count(*) FROM sqlite_schema WHERE name='desktop_items_pane'",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        let before=connection.total_changes();validate_and_upgrade(&connection).unwrap();assert_eq!(connection.total_changes(),before);
    }
}
