//! Workspace database lifecycle, loading, and transactional saves.
mod codec;
mod config;
pub use config::SettingValue;
#[cfg(test)]
mod config_tests;
mod desktop_items;
mod folder_view;
pub use folder_view::FolderPreferences;
mod geometry;
mod monitor_layout;
mod recovery;
mod schema;
mod tabs;

use crate::StoreError;
use luciddesk_core::{Backdrop, Panel, PanelId, RectDip, Workspace};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::path::{Path, PathBuf};

use codec::{
    decode_backdrop, encode_backdrop, encode_panel_text, equivalent_backdrop, workspace_preferences,
};
use desktop_items::{insert_desktop_items, load_desktop_items};

/// Stores workspace state and, for file-backed stores, application preferences.
pub struct WorkspaceStore {
    connection: Connection,
    config: Option<std::cell::RefCell<config::ConfigFile>>,
    on_change: Option<Box<dyn Fn() + Send>>,
    committed_changes: std::cell::Cell<u64>,
}

impl std::fmt::Debug for WorkspaceStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkspaceStore")
            .field("has_config", &self.config.is_some())
            .finish_non_exhaustive()
    }
}

impl WorkspaceStore {
    /// Installs a notification after successful writes. The callback should only
    /// enqueue work, never access this store or synchronously reenter its owner.
    pub fn set_change_callback(&mut self, callback: impl Fn() + Send + 'static) {
        self.on_change = Some(Box::new(callback));
    }

    fn notify_change(&self, previous: u64) {
        let changes = self.raw_change_count().saturating_sub(previous);
        if changes != 0 {
            self.committed_changes.set(self.committed_changes.get() + changes);
            self.emit_change();
        }
    }

    fn emit_change(&self) {
        if let Some(callback) = &self.on_change {
            callback();
        }
    }

    /// Reads an optional application preference.
    /// # Errors
    /// Returns an error if the database query fails.
    pub fn preference(&self, key: &str) -> Result<Option<String>, StoreError> {
        if config::KEYS.contains(&key)
            && let Some(config) = &self.config
        {
            return Ok(config.borrow().values.get(key).cloned());
        }
        if let Some(id) = key.strip_prefix("panel_folder_sort:") {
            return Ok(self.connection.query_row("SELECT sort_column||':'||sort_direction FROM panel_folder_settings WHERE panel_id=?1",[id],|r|r.get(0)).optional()?);
        }
        if let Some(value) = folder_view::read(&self.connection, key)? {
            return Ok(Some(value));
        }
        Ok(self
            .connection
            .query_row("SELECT value FROM metadata WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// Saves an application preference atomically.
    /// # Errors
    /// Returns an error if the database update fails.
    pub fn save_preference(&self, key: &str, value: &str) -> Result<(), StoreError> {
        let previous = self.raw_change_count();
        if config::KEYS.contains(&key)
            && let Some(config) = &self.config
        {
            config.borrow_mut().save(&[(key, value.to_owned())])?;
            self.notify_change(previous);
            return Ok(());
        }
        if let Some(id) = key.strip_prefix("panel_folder_sort:") {
            let (column, direction) = schema::parse_sort(value)?;
            self.connection.execute("UPDATE panel_folder_settings SET sort_column=?2,sort_direction=?3 WHERE panel_id=?1 AND (sort_column IS NOT ?2 OR sort_direction IS NOT ?3)",params![id,column,direction])?;
            self.notify_change(previous);
            return Ok(());
        }
        if folder_view::key(key).is_some() {
            let tx = self.connection.unchecked_transaction()?;
            if folder_view::write(&tx, key, value)? {
                tx.execute("DELETE FROM metadata WHERE key=?1",[key])?;
                tx.commit()?;
                self.notify_change(previous);
                return Ok(());
            }
        }
        self.connection.execute(
            "INSERT INTO metadata(key,value) VALUES (?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value
             WHERE metadata.value != excluded.value",
            params![key, value],
        )?;
        self.notify_change(previous);
        Ok(())
    }

    /// Saves database-only preferences in one transaction and sends one notification.
    /// # Errors
    /// Rejects TOML and structured folder-setting keys; rolls back the entire batch on error.
    pub fn save_metadata_preferences(&self, updates: &[(&str, &str)]) -> Result<(), StoreError> {
        if updates.iter().any(|(key, _)| config::KEYS.contains(key) || key.starts_with("panel_folder_sort:") || folder_view::key(key).is_some()) {
            return Err(StoreError::InvalidData("batch requires metadata preferences".into()));
        }
        let previous = self.raw_change_count();
        let transaction = self.connection.unchecked_transaction()?;
        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO metadata(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value WHERE value IS NOT excluded.value",
            )?;
            for (key, value) in updates {
                statement.execute(params![key, value])?;
            }
        }
        transaction.commit()?;
        self.notify_change(previous);
        Ok(())
    }

    /// Opens or creates a `LucidDesk` workspace database.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened or initialized.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let connection = Connection::open(path)?;
        let mut store = Self::from_connection(connection)?;
        store.attach_config(&path.with_file_name("config.toml"))?;
        Ok(store)
    }

    #[cfg(test)]
    fn open_database(path: &Path) -> Result<Self, StoreError> {
        Self::from_connection(Connection::open(path)?)
    }

    /// Creates an in-memory workspace database for tests and temporary sessions.
    ///
    /// # Errors
    ///
    /// Returns an error when the schema cannot be initialized.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let connection = Connection::open_in_memory()?;
        Self::from_connection(connection)
    }

    fn from_connection(connection: Connection) -> Result<Self, StoreError> {
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        initialize_schema(&connection)?;
        Ok(Self {
            connection,
            config: None,
            on_change: None,
            committed_changes: std::cell::Cell::new(0),
        })
    }

    /// Loads the complete workspace.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid persisted data or a database failure.
    pub fn load_workspace(&self) -> Result<Workspace, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT id, title, x, y, width, height, \
             collapsed, locked, backdrop_kind, opacity, color FROM panels ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| {
            let raw_id: i64 = row.get(0)?;
            let backdrop_kind: String = row.get(8)?;
            let opacity: Option<f32> = row.get(9)?;
            Ok(PersistedPanel {
                id: raw_id,
                title: row.get(1)?,
                rect: RectDip::from_bounds(row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?),
                collapsed: row.get(6)?,
                locked: row.get(7)?,
                backdrop_kind,
                opacity,
                color: row.get(10)?,
            })
        })?;

        let mut panels = Vec::new();
        for row in rows {
            panels.push(row?.into_panel()?);
        }
        drop(statement);
        for panel in &mut panels {
            let (theme, top, hide, kind): (String, bool, bool, String) =
                self.connection.query_row(
                    "SELECT theme,always_on_top,auto_hide,kind FROM panels WHERE id=?1",
                    [panel.id().get()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?;
            panel.set_theme(match theme.as_str() {
                "light" => luciddesk_core::PanelTheme::Light,
                "dark" => luciddesk_core::PanelTheme::Dark,
                "system" => luciddesk_core::PanelTheme::System,
                _ => return Err(StoreError::InvalidData("invalid panel theme".into())),
            });
            panel.set_always_on_top(top);
            panel.set_auto_hide(hide);
            panel.set_search(kind == "search");
            if kind == "desktop" {
                panel.set_fixed_grid(self.preference(&format!("panel_fixed_grid:{}", panel.id().get()))?.as_deref() == Some("1"));
                panel.set_free_layout(self.preference(&format!("panel_free_layout:{}", panel.id().get()))?.as_deref() == Some("1"));
                panel.set_list_view(self.preference(&format!("panel_desktop_list:{}", panel.id().get()))?.as_deref() == Some("1"));
            }
            if kind == "folder" {
                let (path, view): (String, String) = self.connection.query_row(
                    "SELECT path,view FROM panel_folder_settings WHERE panel_id=?1",
                    [panel.id().get()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                panel.set_folder(Some(PathBuf::from(path)));
                panel.set_list_view(view == "list");
            }
        }
        let mut workspace = Workspace::from_panels(panels)
            .map_err(|error| StoreError::InvalidData(error.to_string()))?;
        workspace.reconcile_desktop_items(load_desktop_items(&self.connection)?);
        let appearance = self.preference("appearance")?;
        if let Some(value) = appearance {
            let parts: Vec<_> = value.split('|').collect();
            if parts.len() != 3 && parts.len() != 4 {
                return Err(StoreError::InvalidData("invalid appearance".into()));
            }
            let theme = match parts[0] {
                "light" => luciddesk_core::PanelTheme::Light,
                "dark" => luciddesk_core::PanelTheme::Dark,
                _ => luciddesk_core::PanelTheme::System,
            };
            let opacity = parts[2]
                .parse()
                .map_err(|_| StoreError::InvalidData("invalid opacity".into()))?;
            let color = parts
                .get(3)
                .filter(|v| !v.is_empty())
                .map(|v| v.parse::<u32>())
                .transpose()
                .map_err(|_| StoreError::InvalidData("invalid color".into()))?;
            let backdrop = decode_backdrop(parts[1], Some(opacity), color)?;
            workspace.set_appearance_defaults(theme, backdrop);
            let inherited: Vec<(u64, bool, bool)> = self
                .connection
                .prepare("SELECT id,inherit_theme,inherit_backdrop FROM panels")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<_, _>>()?;
            for (id, inherit_theme, inherit_backdrop) in inherited {
                if let Some(panel) = workspace.panel_mut(PanelId::new(id)) {
                    if inherit_theme {
                        panel.set_theme(theme);
                    }
                    if inherit_backdrop {
                        panel.set_backdrop(backdrop);
                    }
                }
            }
        }
        let options = self.preference("pane_options")?;
        if let Some(value) = options {
            let parts = value.split('|').collect::<Vec<_>>();
            let grid_scale = if parts.len() == 6 {
                let value: f32 = parts[5].parse().map_err(|_| StoreError::InvalidData("invalid grid scale".into()))?;
                let (min, max) = luciddesk_core::PaneOptions::GRID_SCALE_RANGE;
                if !(min..=max).contains(&value) { return Err(StoreError::InvalidData("invalid grid scale".into())); }
                value
            } else { 100.0 };
            let base = if parts.len() == 6 { &parts[..5] } else { parts.as_slice() };
            let [radius, border, snap, text @ ..] = base else {
                return Err(StoreError::InvalidData("invalid pane options".into()));
            };
            let invalid = || StoreError::InvalidData("invalid pane options".into());
            let corner_radius = match *radius {
                "true" => 7.0,
                "false" => 0.0,
                value => value.parse::<f32>().map_err(|_| invalid())?,
            };
            if !corner_radius.is_finite()
                || !(0.0..=luciddesk_core::PaneOptions::MAX_CORNER_RADIUS).contains(&corner_radius)
            {
                return Err(invalid());
            }
            workspace.set_pane_options(luciddesk_core::PaneOptions {
                corner_radius,
                grid_scale,
                border: border.parse().map_err(|_| invalid())?,
                snap: snap.parse().map_err(|_| invalid())?,
                text: match text {
                    [] | ["auto"] | ["auto", _] => luciddesk_core::PanelText::Auto,
                    ["light"] | ["light", _] => luciddesk_core::PanelText::Light,
                    ["dark"] | ["dark", _] => luciddesk_core::PanelText::Dark,
                    _ => return Err(invalid()),
                },
                text_protection: match text {
                    [] | [_] => false,
                    [_, enabled] => enabled.parse().map_err(|_| invalid())?,
                    _ => return Err(invalid()),
                },
            });
        }
        tabs::load(&self.connection, &mut workspace)?;
        Ok(workspace)
    }

    /// Updates only global pane options.
    ///
    /// # Errors
    /// Returns an error if the metadata update fails.
    pub fn save_pane_options(&self, options: luciddesk_core::PaneOptions) -> Result<(), StoreError> {
        self.save_preference(
            "pane_options",
            &format!(
                "{}|{}|{}|{}|{}|{}",
                options.corner_radius,
                options.border,
                options.snap,
                encode_panel_text(options.text),
                options.text_protection, options.grid_scale
            ),
        )
    }

    /// Replaces the persisted workspace in one transaction.
    ///
    /// # Errors
    /// Returns an error when serialization or commit fails.
    pub fn save_workspace(&mut self, workspace: &Workspace) -> Result<(), StoreError> {
        self.save_workspace_with_layout(workspace, None)
    }

    /// Saves workspace rows and optional display geometry in one SQLite transaction.
    /// # Errors
    /// Returns validation or database errors without committing partial layout changes.
    pub fn save_workspace_with_layout(
        &mut self,
        workspace: &Workspace,
        layout: Option<(&str, &[(PanelId, RectDip)])>,
    ) -> Result<(), StoreError> {
        self.save_workspace_with_folder_preferences(workspace, layout, &[])
    }

    /// Commits workspace, display layout and folder preferences in one transaction.
    /// # Errors
    /// Invalid folder preferences or database errors roll back the whole operation.
    pub fn save_workspace_with_folder_preferences(
        &mut self,
        workspace: &Workspace,
        layout: Option<(&str, &[(PanelId, RectDip)])>,
        folders: &[(PanelId, FolderPreferences)],
    ) -> Result<(), StoreError> {
        let previous = self.raw_change_count();
        let transaction = self.connection.transaction()?;
        tabs::save(&transaction, workspace)?;
        if self.config.is_none() {
            if workspace.appearance().is_none() {
                transaction.execute("DELETE FROM metadata WHERE key = 'appearance'", [])?;
            }
            for (key, value) in workspace_preferences(workspace)? {
                transaction.prepare_cached("INSERT INTO metadata(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value WHERE metadata.value IS NOT excluded.value")?.execute(params![key,value])?;
            }
        }
        let live_items: std::collections::HashSet<_> = workspace
            .desktop_items()
            .iter()
            .map(|i| i.identity().persistent_key())
            .collect();
        let old_items: Vec<String> = transaction
            .prepare("SELECT identity_key FROM desktop_items")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for key in old_items {
            if !live_items.contains(&key) {
                transaction.prepare_cached("DELETE FROM desktop_items WHERE identity_key=?1")?.execute( [key])?;
            }
        }

        for panel in workspace.panels() {
            let key = format!("panel_desktop_list:{}", panel.id().get());
            if panel.folder().is_none() && !panel.is_search() && panel.list_view() {
                transaction.prepare_cached("INSERT INTO metadata(key,value) VALUES (?1,'1') ON CONFLICT(key) DO UPDATE SET value='1' WHERE value != '1'")?.execute( [&key])?;
            } else {
                transaction.prepare_cached("DELETE FROM metadata WHERE key=?1")?.execute( [&key])?;
            }
            let key = format!("panel_fixed_grid:{}", panel.id().get());
            if panel.fixed_grid() {
                transaction.prepare_cached("INSERT INTO metadata(key,value) VALUES (?1,'1') ON CONFLICT(key) DO UPDATE SET value='1' WHERE value != '1'")?.execute([&key])?;
            } else {
                transaction.prepare_cached("DELETE FROM metadata WHERE key=?1")?.execute([&key])?;
            }
            let key = format!("panel_free_layout:{}", panel.id().get());
            if panel.free_layout() {
                transaction.prepare_cached("INSERT INTO metadata(key,value) VALUES (?1,'1') ON CONFLICT(key) DO UPDATE SET value='1' WHERE value != '1'")?.execute([&key])?;
            } else {
                transaction.prepare_cached("DELETE FROM metadata WHERE key=?1")?.execute([&key])?;
            }
            insert_panel(&transaction, panel, workspace.appearance())?;
        }
        folder_view::absorb(&transaction)?;
        for (id, preferences) in folders { folder_view::save_preferences(&transaction, *id, preferences)?; }
        insert_desktop_items(&transaction, workspace.desktop_items())?;
        let live_panels: std::collections::HashSet<_> =
            workspace.panels().iter().map(|p| p.id().get()).collect();
        let old_panels: Vec<u64> = transaction
            .prepare("SELECT id FROM panels")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for id in old_panels {
            if !live_panels.contains(&id) {
                transaction.prepare_cached("DELETE FROM panels WHERE id=?1")?.execute( [id])?;
                transaction.prepare_cached(
                    "DELETE FROM metadata WHERE key IN (?1,?2,?3,?4,?5)",
                )?.execute(params![
                    format!("panel_fixed_grid:{id}"),
                    format!("panel_free_layout:{id}"),
                    format!("panel_desktop_list:{id}"),
                    format!("panel_folder_columns:{id}"),
                    format!("panel_folder_visible_columns:{id}"),
                ])?;
            }
        }
        if let Some((topology, entries)) = layout {
            monitor_layout::update(&transaction, topology, entries)?;
        }
        let previous_config = self.config.as_ref().map(|c| c.borrow().source.clone());
        if let Some(config) = &self.config {
            config
                .borrow_mut()
                .save(&workspace_preferences(workspace)?)?;
        }
        if let Err(error) = transaction.commit() {
            if let (Some(config), Some(source)) = (&self.config, previous_config) {
                let mut config = config.borrow_mut();
                if config.source != source {
                    config::atomic_write(&config.path, &source)?;
                    *config = config::ConfigFile::parse(config.path.clone(), source)?;
                }
            }
            return Err(error.into());
        }
        self.notify_change(previous);
        Ok(())
    }
}

fn initialize_schema(connection: &Connection) -> Result<(), StoreError> {
    let has_tables: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%')",
        [], |row| row.get(0),
    )?;
    if has_tables {
        return schema::validate_and_upgrade(connection);
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch(schema::SCHEMA)?;
    transaction.execute_batch(folder_view::SCHEMA)?;
    transaction.commit()?;
    Ok(())
}

fn insert_panel(
    transaction: &Transaction<'_>,
    panel: &Panel,
    defaults: Option<(luciddesk_core::PanelTheme, Backdrop)>,
) -> Result<(), StoreError> {
    let id = i64::try_from(panel.id().get())
        .map_err(|_| StoreError::InvalidData("panel id exceeds SQLite range".into()))?;
    let (backdrop_kind, opacity, color) = encode_backdrop(panel.backdrop());
    decode_backdrop(backdrop_kind, opacity, color)?;
    let rect = panel.rect();
    transaction.prepare_cached(
        "INSERT INTO panels(
             id, title, x, y, width, height,
             collapsed, locked, backdrop_kind, opacity, color, theme, always_on_top, auto_hide, kind, inherit_theme, inherit_backdrop
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17) ON CONFLICT(id) DO UPDATE SET title=excluded.title,x=excluded.x,y=excluded.y,width=excluded.width,height=excluded.height,collapsed=excluded.collapsed,locked=excluded.locked,backdrop_kind=excluded.backdrop_kind,opacity=excluded.opacity,color=excluded.color,theme=excluded.theme,always_on_top=excluded.always_on_top,auto_hide=excluded.auto_hide,kind=excluded.kind,inherit_theme=excluded.inherit_theme,inherit_backdrop=excluded.inherit_backdrop WHERE (title,x,y,width,height,collapsed,locked,backdrop_kind,opacity,color,theme,always_on_top,auto_hide,kind,inherit_theme,inherit_backdrop) IS NOT (excluded.title,excluded.x,excluded.y,excluded.width,excluded.height,excluded.collapsed,excluded.locked,excluded.backdrop_kind,excluded.opacity,excluded.color,excluded.theme,excluded.always_on_top,excluded.auto_hide,excluded.kind,excluded.inherit_theme,excluded.inherit_backdrop)",
    )?.execute(
        params![
            id,
            panel.title(),
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            panel.collapsed(),
            panel.locked(),
            backdrop_kind,
            opacity,
            color,
            match panel.theme() { luciddesk_core::PanelTheme::System=>"system",luciddesk_core::PanelTheme::Light=>"light",luciddesk_core::PanelTheme::Dark=>"dark" },
            panel.always_on_top(),panel.auto_hide(),if panel.is_search(){"search"}else if panel.folder().is_some(){"folder"}else{"desktop"},
            defaults.is_some_and(|(theme,_)|panel.theme()==theme),
            defaults.is_some_and(|(_,backdrop)| equivalent_backdrop(panel.backdrop(),backdrop)),
        ],
    )?;
    if let Some(path) = panel.folder() {
        transaction.prepare_cached("INSERT INTO panel_folder_settings(panel_id,path,view) VALUES (?1,?2,?3) ON CONFLICT(panel_id) DO UPDATE SET path=excluded.path,view=excluded.view WHERE path IS NOT excluded.path OR view IS NOT excluded.view")?.execute(params![id,path.to_string_lossy(),if panel.list_view(){"list"}else{"icons"}])?;
    } else {
        transaction.prepare_cached("DELETE FROM panel_folder_settings WHERE panel_id=?1")?.execute( [id])?;
    }
    Ok(())
}

struct PersistedPanel {
    id: i64,
    title: String,
    rect: RectDip,
    collapsed: bool,
    locked: bool,
    backdrop_kind: String,
    opacity: Option<f32>,
    color: Option<u32>,
}

impl PersistedPanel {
    fn into_panel(self) -> Result<Panel, StoreError> {
        if ![self.rect.x, self.rect.y, self.rect.width, self.rect.height]
            .into_iter()
            .all(f32::is_finite)
            || self.rect.width <= 0.0
            || self.rect.height <= 0.0
        {
            return Err(StoreError::InvalidData("invalid panel geometry".into()));
        }
        let id = u64::try_from(self.id)
            .map_err(|_| StoreError::InvalidData("panel id is negative".into()))?;
        let backdrop = decode_backdrop(&self.backdrop_kind, self.opacity, self.color)?;

        let mut panel = Panel::new(PanelId::new(id), self.title, self.rect);
        panel.set_rect(self.rect);
        panel.set_collapsed(self.collapsed);
        panel.set_locked(self.locked);
        panel.set_backdrop(backdrop);
        Ok(panel)
    }
}

#[cfg(test)]
mod tests;
