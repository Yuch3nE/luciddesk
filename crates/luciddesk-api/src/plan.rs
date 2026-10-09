//! Typed plans reject unknown operations and fields before domain execution.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub instance_id: String,
    pub state_version: String,
    pub inventory_version: String,
    pub topology_token: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub protocol_version: u32,
    pub base: Context,
    pub operations: Vec<Operation>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapSide { Left, Right, Top, Bottom }
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapAlign { #[default] Start, Center, End }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum Operation {
    #[serde(rename = "pane.sort")]
    Sort {
        pane_id: String,
        #[serde(default)]
        sort_column: FolderColumn,
        #[serde(default)]
        descending: bool,
    },
    #[serde(rename = "startup.set")]
    StartupSet { enabled: bool, expected_status: String },
    #[serde(rename = "folder.fit")]
    FolderFit {
        pane_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "optional")]
        icon_columns: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "optional")]
        max_rows: Option<u32>,
    },
    #[serde(rename = "folder.refresh")]
    FolderRefresh { pane_id: String },
    #[serde(rename = "folder.navigate")]
    FolderNavigate { pane_id: String, path: String },
    #[serde(rename = "folder.back")]
    FolderBack { pane_id: String },
    #[serde(rename = "folder.home")]
    FolderHome { pane_id: String },
    #[serde(rename = "search.query")]
    SearchQuery { pane_id: String, query: String },
    #[serde(rename = "search.refresh")]
    SearchRefresh { pane_id: String },
    #[serde(rename = "search.more")]
    SearchMore { pane_id: String },
    #[serde(rename = "folder.create")]
    FolderCreate {
        #[serde(rename = "ref")]
        reference: String,
        title: String,
        path: String,
    },
    #[serde(rename = "folder.update")]
    FolderUpdate {
        pane_id: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional"
        )]
        path: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional"
        )]
        list_view: Option<bool>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional"
        )]
        sort_column: Option<FolderColumn>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional"
        )]
        descending: Option<bool>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional"
        )]
        column_widths: Option<[f32; 4]>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional"
        )]
        visible_columns: Option<Vec<FolderColumn>>,
    },
    #[serde(rename = "tab.merge")]
    TabMerge {
        pane_id: String,
        into_pane_id: String,
    },
    #[serde(rename = "tab.select")]
    TabSelect { pane_id: String },
    #[serde(rename = "tab.reorder")]
    TabReorder {
        pane_id: String,
        pane_ids: Vec<String>,
    },
    #[serde(rename = "tab.detach")]
    TabDetach { pane_id: String },
    #[serde(rename = "pane.snap")]
    Snap {
        pane_id: String,
        target_pane_id: String,
        side: SnapSide,
        #[serde(default)]
        align: SnapAlign,
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "optional")]
        icon_columns: Option<u32>,
    },
    #[serde(rename = "pane.fit")]
    Fit { pane_id: String, icon_columns: u32 },
    #[serde(rename = "pane.arrange")]
    Arrange { monitor_id: String, columns: Vec<Vec<String>>, icon_columns: u32 },
    #[serde(rename = "pane.geometry")]
    Geometry {
        pane_id: String,
        monitor_id: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    #[serde(rename = "settings.update")]
    SettingsUpdate {
        values: std::collections::BTreeMap<String, SettingValue>,
    },
    #[serde(rename = "pane.create")]
    Create {
        #[serde(rename = "ref")]
        reference: String,
        title: String,
    },
    #[serde(rename = "pane.update")]
    Update {
        pane_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "optional")]
        auto_compact: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "optional")]
        align_icons_to_grid: Option<bool>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional"
        )]
        title: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional"
        )]
        locked: Option<bool>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional"
        )]
        auto_hide: Option<bool>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional"
        )]
        collapsed: Option<bool>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional"
        )]
        always_on_top: Option<bool>,
    },
    #[serde(rename = "pane.remove")]
    Remove {
        pane_id: String,
        #[serde(default)]
        release_items: bool,
    },
    #[serde(rename = "item.release")]
    Release { item_ids: Vec<String> },
    #[serde(rename = "item.assign")]
    Assign {
        item_ids: Vec<String>,
        #[serde(default)]
        pane_id: Option<String>,
        #[serde(default)]
        pane_ref: Option<String>,
    },
    #[serde(rename = "item.reorder")]
    Reorder {
        pane_id: String,
        item_ids: Vec<String>,
    },
    #[serde(rename = "item.position")]
    Position {
        pane_id: String,
        item_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "optional")]
        column: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "optional")]
        row: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "optional")]
        x: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "optional")]
        y: Option<f32>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderColumn {
    Name,
    Modified,
    Type,
    Size,
}
impl Default for FolderColumn { fn default() -> Self { Self::Name } }

/// JSON scalar settings preserve types and reject null, arrays and objects.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SettingValue {
    Boolean(bool),
    Number(f64),
    Text(String),
}

// Omission means unchanged; explicit null is invalid, never an implicit reset.
fn optional<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn setting_values_reject_null_and_structures() {
        for raw in ["null", "[]", "{}"] {
            let input =
                format!(r#"{{"op":"settings.update","values":{{"search.enabled":{raw}}}}}"#);
            assert!(serde_json::from_str::<Operation>(&input).is_err());
        }
        let input = r#"{"op":"settings.update","values":{"search.enabled":true,"panel_defaults.grid_scale":125,"language":"system"}}"#;
        assert!(serde_json::from_str::<Operation>(input).is_ok());
    }

    #[test]
    fn operations_reject_unknown_fields_and_preserve_string_ids() {
        let raw = r#"{"op":"pane.update","pane_id":"9007199254740993","title":"整理"}"#;
        let op: Operation = serde_json::from_str(raw).unwrap();
        assert_eq!(
            serde_json::to_value(op).unwrap()["pane_id"],
            "9007199254740993"
        );
        assert!(serde_json::from_str::<Operation>(&raw.replace("title", "unknown")).is_err());
        assert!(
            serde_json::from_str::<Operation>(r#"{"op":"item.remove","item_ids":[]}"#).is_err()
        );
        assert!(
            serde_json::from_str::<Operation>(
                r#"{"op":"pane.update","pane_id":"1","title":"a","title":"b"}"#
            )
            .is_err()
        );
    }
}
