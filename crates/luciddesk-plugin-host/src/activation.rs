//! Host-owned activation policy in the user data directory's plugins/config.toml.
use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
};
use toml_edit::{DocumentMut, Item, Table, value};

pub struct ActivationConfig {
    path: PathBuf,
    document: DocumentMut,
}

impl ActivationConfig {
    /// Read activation policy without creating a file. Missing entries default to enabled.
    /// # Errors
    /// Returns read, TOML syntax or activation field type errors without changing the file.
    pub fn open(directory: &Path) -> Result<Self, String> {
        let path = directory.join("config.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        let document = text
            .parse::<DocumentMut>()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let config = Self { path, document };
        config.disabled()?;
        Ok(config)
    }

    /// Get disabled IDs, including records for plugins which are not installed.
    /// # Errors
    /// Returns an error if `plugins` entries are not tables with boolean `enabled` fields.
    pub fn disabled(&self) -> Result<BTreeSet<String>, String> {
        let mut disabled = BTreeSet::new();
        if let Some(plugins) = self.document.get("plugins") {
            let plugins = plugins
                .as_table_like()
                .ok_or("plugins must be a TOML table")?;
            for (id, entry) in plugins.iter() {
                let entry = entry
                    .as_table_like()
                    .ok_or_else(|| format!("plugins.{id} must be a table"))?;
                if let Some(enabled) = entry.get("enabled") {
                    let enabled = enabled
                        .as_bool()
                        .ok_or_else(|| format!("plugins.{id}.enabled must be a boolean"))?;
                    if !enabled {
                        disabled.insert(id.to_owned());
                    }
                }
            }
        }
        Ok(disabled)
    }

    /// Atomically persist a single preference, preserving comments and other plugin records.
    /// # Errors
    /// Returns filesystem errors; the in-memory policy is updated only after a successful save.
    pub fn save_enabled(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        if self.disabled()?.contains(id) != enabled {
            return Ok(());
        }
        let mut document = self.document.clone();
        if document.get("plugins").is_none() {
            document["plugins"] = Item::Table(Table::new());
        }
        if document["plugins"].get(id).is_none() {
            document["plugins"][id] = Item::Table(Table::new());
        }
        document["plugins"][id]["enabled"] = value(enabled);
        let directory = self
            .path
            .parent()
            .ok_or("Missing plugin config directory")?;
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let mut file =
            tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
        file.write_all(document.to_string().as_bytes())
            .and_then(|()| file.as_file().sync_all())
            .map_err(|error| error.to_string())?;
        file.persist(&self.path)
            .map_err(|error| format!("{}: {error}", self.path.display()))?;
        self.document = document;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_roundtrips_under_plugins_and_preserves_other_records() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("plugins");
        let mut config = ActivationConfig::open(&root).unwrap();
        assert!(config.disabled().unwrap().is_empty());
        assert!(!root.join("config.toml").exists());
        config.save_enabled("example.echo", false).unwrap();
        config.save_enabled("system-monitor", false).unwrap();
        let path = root.join("config.toml");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("# keep this comment\n{text}\n[plugins.uninstalled]\nenabled = false\nextra = 'preserved'\n")).unwrap();
        let mut config = ActivationConfig::open(&root).unwrap();
        config.save_enabled("example.echo", true).unwrap();
        let disabled = ActivationConfig::open(&root).unwrap().disabled().unwrap();
        assert_eq!(
            disabled,
            BTreeSet::from(["system-monitor".into(), "uninstalled".into()])
        );
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# keep this comment"));
        assert!(saved.contains("extra = 'preserved'"));
        assert!(!directory.path().join("config.toml").exists());
    }

    #[test]
    fn malformed_policy_is_not_overwritten_or_treated_as_enabled() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        for invalid in [
            "[plugins",
            "plugins = false",
            "[plugins]\necho = false",
            "[plugins.echo]\nenabled = 'false'",
        ] {
            std::fs::write(&path, invalid).unwrap();
            assert!(ActivationConfig::open(directory.path()).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
        }
    }

    #[test]
    fn failed_save_keeps_previous_policy_and_unrelated_file() {
        let directory = tempfile::tempdir().unwrap();
        let mut config = ActivationConfig::open(directory.path()).unwrap();
        std::fs::create_dir(directory.path().join("config.toml")).unwrap();
        assert!(config.save_enabled("example.echo", false).is_err());
        assert!(config.disabled().unwrap().is_empty());
        assert!(directory.path().join("config.toml").is_dir());
    }
}
