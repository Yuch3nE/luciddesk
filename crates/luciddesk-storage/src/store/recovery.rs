use super::*;

fn copy_database(source: &Connection, target: &mut Connection) -> Result<(), StoreError> {
    let backup = rusqlite::backup::Backup::new(source, target)?;
    if backup.step(-1)? != rusqlite::backup::StepResult::Done {
        return Err(StoreError::InvalidData("database is busy".into()));
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn unique_backup_path(path: &Path, label: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    path.with_extension(format!("{label}-{stamp}.db"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_backup_content_is_order_independent_typed_and_binary_safe() {
        let store = WorkspaceStore::open_in_memory().unwrap();
        store.connection.execute_batch("CREATE TABLE memory_payload (value); INSERT INTO memory_payload VALUES (1), ('1'), (NULL), (1.5);").unwrap();
        let first = store.backup_content().unwrap();
        store.connection.execute_batch("DELETE FROM memory_payload; INSERT INTO memory_payload VALUES (1.5), (NULL), ('1'), (1);").unwrap();
        assert_eq!(first, store.backup_content().unwrap());
        store.connection.execute_batch("DELETE FROM memory_payload WHERE typeof(value)='text'; INSERT INTO memory_payload VALUES (x'31');").unwrap();
        assert_ne!(first, store.backup_content().unwrap());
        store.connection.execute_batch("DELETE FROM memory_payload;").unwrap();
        let baseline = store.backup_content().unwrap().len();
        let blob = vec![255u8; 1024 * 1024];
        store.connection.execute("INSERT INTO memory_payload VALUES (?1)", [&blob]).unwrap();
        let bytes = store.backup_content().unwrap();
        assert!(bytes.len() - baseline < blob.len() + 64, "binary content must not expand into debug strings");
        store.save_preference("backup_manifest_version", "test").unwrap();
        assert_eq!(bytes, store.backup_content().unwrap());
    }

    fn temp(label: &str) -> PathBuf {
        unique_backup_path(&std::env::temp_dir().join("luciddesk-recovery"), label)
    }
    #[test]
    fn backup_restores_layout_preferences_and_monitor_profiles() {
        let path = temp("roundtrip");
        let mut store = WorkspaceStore::open_in_memory().unwrap();
        let mut workspace = Workspace::new();
        let id = PanelId::new(42);
        let rect = RectDip::new(-800.0, 50.0, 480.0, 320.0);
        workspace.add_panel(Panel::new(id, "项目", rect)).unwrap();
        store.save_workspace(&workspace).unwrap();
        store.save_preference("search_enabled", "0").unwrap();
        store
            .save_monitor_layout("two displays", &[(id, rect)])
            .unwrap();
        store.export_backup(&path).unwrap();
        assert!(store.export_backup(&path).is_err());
        store.save_workspace(&Workspace::new()).unwrap();
        store.save_preference("search_enabled", "1").unwrap();
        store.restore_backup(&path).unwrap();
        assert_eq!(store.load_workspace().unwrap(), workspace);
        assert_eq!(
            store.preference("search_enabled").unwrap().as_deref(),
            Some("0")
        );
        assert_eq!(
            store.monitor_layout("two displays").unwrap(),
            vec![(id, rect)]
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn invalid_restore_does_not_change_live_configuration_or_source() {
        let path = temp("invalid");
        std::fs::write(&path, b"not a SQLite database").unwrap();
        let mut store = WorkspaceStore::open_in_memory().unwrap();
        store.save_preference("sentinel", "keep").unwrap();
        assert!(store.restore_backup(&path).is_err());
        assert_eq!(
            store.preference("sentinel").unwrap().as_deref(),
            Some("keep")
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"not a SQLite database");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn invalid_display_profile_is_rejected_before_restore() {
        let path = temp("bad-profile");
        let mut live = WorkspaceStore::open_in_memory().unwrap();
        live.save_preference("sentinel", "keep").unwrap();
        {
            let source = WorkspaceStore::open_database(&path).unwrap();
            source
                .connection
                .execute_batch("PRAGMA foreign_keys=OFF; PRAGMA ignore_check_constraints=ON;")
                .unwrap();
            source
                .connection
                .execute(
                    "INSERT INTO monitor_layouts VALUES ('bad',1,0,0,-100,100)",
                    [],
                )
                .unwrap();
        }
        assert!(live.restore_backup(&path).is_err());
        assert_eq!(
            live.preference("sentinel").unwrap().as_deref(),
            Some("keep")
        );
        std::fs::remove_file(path).unwrap();
    }

}

impl WorkspaceStore {
    pub fn read_backup(path: &Path) -> Result<Self, StoreError> {
        let source = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut connection = Connection::open_in_memory()?;
        copy_database(&source, &mut connection)?;
        Self::from_connection(connection)
    }
    pub fn backup_file_content(path: &Path) -> Result<Vec<u8>, StoreError> {
        Self::read_backup(path)?.backup_content()
    }
    /// Captures a consistent, independent snapshot for background file operations.
    pub fn backup_snapshot(&self) -> Result<Self, StoreError> {
        let mut connection = Connection::open_in_memory()?;
        copy_database(&self.connection, &mut connection)?;
        if let Some(config) = &self.config {
            let config = config.borrow();
            config.check_disk()?;
            connection.execute(
                "INSERT OR REPLACE INTO metadata VALUES ('backup_config',?1)",
                [&config.source],
            )?;
        }
        Self::from_connection(connection)
    }

    /// Compares logical rows rather than database pages or write counters.
    pub fn backup_content(&self) -> Result<Vec<u8>, StoreError> {
        use rusqlite::types::ValueRef;

        fn field(output: &mut Vec<u8>, value: &[u8]) {
            output.extend_from_slice(&(value.len() as u64).to_le_bytes());
            output.extend_from_slice(value);
        }

        let tables: Vec<String> = self.connection.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?
            .query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        let mut result = Vec::new();
        for table in tables {
            let quoted = table.replace('"', "\"\"");
            let mut statement = self.connection.prepare(&format!("SELECT * FROM \"{quoted}\""))?;
            let columns = statement.column_count();
            let mut records = Vec::new();
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                if table == "metadata" && row.get::<_, String>(0)?.starts_with("backup_manifest_") {
                    continue;
                }
                let mut record = Vec::new();
                for column in 0..columns {
                    match row.get_ref(column)? {
                        ValueRef::Null => record.push(0),
                        ValueRef::Integer(value) => {
                            record.push(1);
                            record.extend_from_slice(&value.to_le_bytes());
                        }
                        ValueRef::Real(value) => {
                            record.push(2);
                            record.extend_from_slice(&value.to_bits().to_le_bytes());
                        }
                        ValueRef::Text(value) => { record.push(3); field(&mut record, value); }
                        ValueRef::Blob(value) => { record.push(4); field(&mut record, value); }
                    }
                }
                records.push(record);
            }
            records.sort_unstable();
            field(&mut result, table.as_bytes());
            result.extend_from_slice(&(columns as u64).to_le_bytes());
            result.extend_from_slice(&(records.len() as u64).to_le_bytes());
            for record in records { field(&mut result, &record); }
        }
        Ok(result)
    }

    /// Validates a portable backup without modifying the live store.
    pub fn inspect_backup(path: &Path) -> Result<usize, StoreError> {
        let source = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let config: String = source.query_row(
            "SELECT value FROM metadata WHERE key='backup_config'",
            [],
            |r| r.get(0),
        )?;
        config::ConfigFile::parse(path.with_extension("toml"), config)?;
        let mut scratch = Self::open_in_memory()?;
        scratch.restore_backup(path)?;
        Ok(scratch.load_workspace()?.panels().len())
    }

    /// Atomically replaces an explicitly approved export destination.
    pub fn export_backup_replace(&self, path: &Path) -> Result<(), StoreError> {
        let directory = tempfile::tempdir_in(
            path.parent()
                .ok_or_else(|| StoreError::InvalidData("invalid destination".into()))?,
        )
        .map_err(|e| StoreError::InvalidData(e.to_string()))?;
        let staged = directory.path().join("snapshot.db");
        self.export_backup(&staged)?;
        let mut output = tempfile::NamedTempFile::new_in(path.parent().unwrap())
            .map_err(|e| StoreError::InvalidData(e.to_string()))?;
        let mut input =
            std::fs::File::open(staged).map_err(|e| StoreError::InvalidData(e.to_string()))?;
        std::io::copy(&mut input, &mut output)
            .map_err(|e| StoreError::InvalidData(e.to_string()))?;
        output
            .as_file()
            .sync_all()
            .map_err(|e| StoreError::InvalidData(e.to_string()))?;
        output
            .persist(path)
            .map_err(|e| StoreError::InvalidData(e.to_string()))?;
        Ok(())
    }
    /// Successful changes through the store API, excluding rolled-back writes.
    #[must_use]
    pub fn change_count(&self) -> u64 {
        self.committed_changes.get()
    }
    pub(super) fn raw_change_count(&self) -> u64 {
        self.connection.total_changes() + self.config.as_ref().map_or(0, |c| c.borrow().changes)
    }
    /// Creates a consistent standalone database snapshot, including preferences.
    /// # Errors
    /// Rejects existing destinations and reports SQLite or path errors.
    pub fn export_backup(&self, path: &Path) -> Result<(), StoreError> {
        if path.exists() {
            return Err(StoreError::InvalidData(
                "backup destination already exists".into(),
            ));
        }
        let staging = tempfile::NamedTempFile::new_in(path.parent().unwrap())
            .map_err(|e| StoreError::InvalidData(e.to_string()))?;
        self.connection
            .backup(rusqlite::MAIN_DB, staging.path(), None)?;
        if let Some(config) = &self.config {
            let config = config.borrow();
            config.check_disk()?;
            let snapshot = Connection::open(staging.path())?;
            snapshot.execute(
                "INSERT OR REPLACE INTO metadata VALUES ('backup_config',?1)",
                [&config.source],
            )?;
        }
        staging.as_file().sync_all().map_err(|e| StoreError::InvalidData(e.to_string()))?;
        staging
            .persist_noclobber(path)
            .map_err(|e| StoreError::InvalidData(e.to_string()))?;
        Ok(())
    }

    /// Validates a backup in isolation before replacing the live database atomically.
    /// # Errors
    /// Invalid databases, unsupported schemas and failed restores leave the live data intact.
    pub fn restore_backup(&mut self, path: &Path) -> Result<(), StoreError> {
        let source = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let integrity: String = source.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if integrity != "ok" {
            return Err(StoreError::InvalidData(integrity));
        }
        let mut copy = Connection::open_in_memory()?;
        {
            let backup = rusqlite::backup::Backup::new(&source, &mut copy)?;
            if backup.step(-1)? != rusqlite::backup::StepResult::Done {
                return Err(StoreError::InvalidData("backup is busy".into()));
            }
        }
        let validated = Self::from_connection(copy)?;
        let restored_config = if let Some(config) = &self.config {
            let config = config.borrow();
            config.check_disk()?;
            let source = validated.preference("backup_config")?.ok_or_else(|| {
                StoreError::InvalidData("备份缺少 config.toml，不能恢复全局设置。".into())
            })?;
            Some(config::ConfigFile::parse(config.path.clone(), source)?)
        } else {
            None
        };
        validated.load_workspace()?;
        for table in ["panel_folder_settings", "monitor_layouts"] {
            let cascades: i64 = validated.connection.query_row(
                "SELECT count(*) FROM pragma_foreign_key_list(?1) WHERE \"table\"='panels' AND on_delete='CASCADE'",
                [table], |r| r.get(0))?;
            if cascades != 1 {
                return Err(StoreError::InvalidData(
                    "backup has an incompatible table schema".into(),
                ));
            }
        }
        let topologies: Vec<String> = validated
            .connection
            .prepare("SELECT DISTINCT topology FROM monitor_layouts")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for topology in topologies {
            validated.monitor_layout(&topology)?;
        }
        let violations: i64 = validated.connection.query_row(
            "SELECT count(*) FROM pragma_foreign_key_check",
            [],
            |r| r.get(0),
        )?;
        if violations != 0 {
            return Err(StoreError::InvalidData("invalid backup references".into()));
        }
        let mut rollback = Connection::open_in_memory()?;
        copy_database(&self.connection, &mut rollback)?;
        // The pending document is committed with the DB and replayed after an interrupted restore.
        if let Some(config) = &restored_config {
            validated.connection.execute(
                "INSERT OR REPLACE INTO metadata VALUES ('pending_config',?1)",
                [&config.source],
            )?;
        }
        validated
            .connection
            .execute("DELETE FROM metadata WHERE key='backup_config'", [])?;
        copy_database(&validated.connection, &mut self.connection)?;
        if let Some(mut config) = restored_config {
            if let Err(error) = config::atomic_write(&config.path, &config.source) {
                copy_database(&rollback, &mut self.connection)?;
                return Err(error);
            }
            config.changes = self.config.as_ref().unwrap().borrow().changes + 1;
            *self.config.as_ref().unwrap().borrow_mut() = config;
            self.connection
                .execute("DELETE FROM metadata WHERE key='pending_config'", [])?;
        }
        self.committed_changes.set(self.committed_changes.get() + 1);
        self.emit_change();
        Ok(())
    }

}
