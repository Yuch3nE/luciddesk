//! Typed workspace preferences. Conversion is pure; saving is one metadata transaction.
use super::*;
use SettingValue::{Boolean, Number, Text};
const FLAGS: [(&str, &str); 3] = [
    ("interface.title_emoji_color", "pane_title_emoji_color"),
    ("interface.compact_menu", "pane_compact_menu"),
    ("interface.header_divider", "pane_header_divider"),
];
pub(super) fn contains(key: &str) -> bool {
    FLAGS.iter().any(|(name, _)| *name == key)
        || matches!(
            key,
            "font.family"
                | "folder_defaults.list_view"
                | "folder_defaults.show_modified"
                | "folder_defaults.show_type"
                | "folder_defaults.show_size"
                | "folder_defaults.entry_mode"
                | "backup.enabled"
                | "backup.interval_minutes"
                | "backup.keep"
        )
}
pub(super) fn read(store: &WorkspaceStore) -> Result<BTreeMap<String, SettingValue>, String> {
    let mut values = BTreeMap::new();
    for (name, key) in FLAGS {
        values.insert(
            name.into(),
            Boolean(store.preference(key).map_err(|e| e.to_string())?.as_deref() != Some("false")),
        );
    }
    values.insert(
        "font.family".into(),
        Text(
            store
                .preference("ui_font_family")
                .map_err(|e| e.to_string())?
                .unwrap_or_default(),
        ),
    );
    let defaults = folder::Defaults::load(store)?;
    values.insert("folder_defaults.list_view".into(), Boolean(defaults.list));
    for (name, bit) in [("modified", 2), ("type", 4), ("size", 8)] {
        values.insert(
            format!("folder_defaults.show_{name}"),
            Boolean(defaults.columns & bit != 0),
        );
    }
    values.insert(
        "folder_defaults.entry_mode".into(),
        Text(
            if folder::EntryMode::load(store)? == folder::EntryMode::Explorer {
                "explorer"
            } else {
                "inline"
            }
            .into(),
        ),
    );
    let policy = recovery::Policy::load(store)?;
    values.insert("backup.enabled".into(), Boolean(policy.enabled));
    values.insert(
        "backup.interval_minutes".into(),
        Number(policy.minutes as f64),
    );
    values.insert("backup.keep".into(), Number(policy.keep as f64));
    Ok(values)
}
pub(super) fn patch(
    before: &BTreeMap<String, SettingValue>,
    updates: &BTreeMap<String, SettingValue>,
) -> Result<BTreeMap<String, SettingValue>, String> {
    let mut after = before.clone();
    for (key, value) in updates {
        let old = before.get(key).ok_or_else(|| {
            format!("unknown workspace setting or mixed persistence domains: {key}")
        })?;
        if std::mem::discriminant(old) != std::mem::discriminant(value) {
            return Err(format!("invalid setting type: {key}"));
        }
        let valid = match (key.as_str(), value) {
            ("font.family", Text(name)) => {
                name.is_empty() || name == crate::i18n::default_font() || fonts::available(name)
            }
            ("folder_defaults.entry_mode", Text(mode)) => {
                matches!(mode.as_str(), "inline" | "explorer")
            }
            ("backup.interval_minutes", Number(n)) => [5.0, 15.0, 30.0, 60.0].contains(n),
            ("backup.keep", Number(n)) => [10.0, 20.0, 50.0].contains(n),
            (_, Boolean(_)) => true,
            _ => false,
        };
        if !valid {
            return Err(format!("invalid setting value: {key}"));
        }
        after.insert(key.clone(), value.clone());
    }
    Ok(after)
}
fn encode(values: &BTreeMap<String, SettingValue>) -> BTreeMap<String, String> {
    let flag = |key: &str| values[key] == Boolean(true);
    let text = |key: &str| match &values[key] {
        Text(v) => v.clone(),
        _ => unreachable!(),
    };
    let number = |key: &str| match values[key] {
        Number(v) => v as u64,
        _ => unreachable!(),
    };
    let mut result = BTreeMap::new();
    for (name, key) in FLAGS {
        result.insert(key.into(), flag(name).to_string());
    }
    result.insert("ui_font_family".into(), text("font.family"));
    result.insert(
        "folder_entry_mode".into(),
        text("folder_defaults.entry_mode"),
    );
    let columns = 1
        | (u8::from(flag("folder_defaults.show_modified")) << 1)
        | (u8::from(flag("folder_defaults.show_type")) << 2)
        | (u8::from(flag("folder_defaults.show_size")) << 3);
    result.insert(
        "folder_panel_defaults".into(),
        format!(
            "{},{}",
            if flag("folder_defaults.list_view") {
                "list"
            } else {
                "icons"
            },
            columns
        ),
    );
    result.insert(
        "backup_policy".into(),
        format!(
            "{},{},{}",
            u8::from(flag("backup.enabled")),
            number("backup.interval_minutes"),
            number("backup.keep")
        ),
    );
    result
}
pub(super) fn encode_changes(
    before: &BTreeMap<String, SettingValue>,
    after: &BTreeMap<String, SettingValue>,
) -> Vec<(String, String)> {
    let old = encode(before);
    encode(after)
        .into_iter()
        .filter(|(key, value)| old.get(key) != Some(value))
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_settings_are_typed_atomic_and_noop_aware() {
        let mut store = WorkspaceStore::open_in_memory().unwrap();
        let notifications = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = notifications.clone();
        store.set_change_callback(move || {
            observed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        let before = read(&store).unwrap();
        let count = store.change_count();
        let updates = BTreeMap::from([
            ("interface.header_divider".into(), Boolean(false)),
            ("folder_defaults.show_type".into(), Boolean(false)),
            ("folder_defaults.entry_mode".into(), Text("explorer".into())),
            ("backup.interval_minutes".into(), Number(30.0)),
        ]);
        let (_, after) = super::super::preview(&store, &updates).unwrap();
        assert_eq!(read(&store).unwrap(), before);
        assert_eq!(store.change_count(), count);
        super::super::save(&store, &updates).unwrap();
        assert_eq!(read(&store).unwrap(), after);
        assert_eq!(store.change_count(), count + 4);
        super::super::save(&store, &updates).unwrap();
        assert_eq!(store.change_count(), count + 4);
        for (key, value) in [
            ("backup.keep", Number(11.0)),
            ("backup.enabled", Text("true".into())),
            ("language", Text("en-US".into())),
            ("folder_defaults.entry_mode", Text("other".into())),
        ] {
            let mut invalid = updates.clone();
            invalid.insert(key.into(), value);
            assert!(super::super::save(&store, &invalid).is_err());
            assert_eq!(read(&store).unwrap(), after);
            assert_eq!(store.change_count(), count + 4);
        }
        assert_eq!(notifications.load(std::sync::atomic::Ordering::Relaxed), 1);
        let defaults = folder::Defaults::load(&store).unwrap();
        assert_eq!(defaults.columns, 11);
        assert_eq!(recovery::Policy::load(&store).unwrap().minutes, 30);
    }
}
