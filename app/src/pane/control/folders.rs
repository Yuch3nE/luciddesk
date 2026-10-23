use super::*;
use luciddesk_api::FolderColumn;
use luciddesk_storage::FolderPreferences;

pub(super) fn path(raw: &str) -> Result<std::path::PathBuf, String> {
    let path = Path::new(raw);
    if raw.contains(['\0', '\n', '\r']) || !path.is_absolute() {
        return Err("folder path must be absolute".into());
    }
    let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    if !path.is_dir() {
        return Err("folder path is not a directory".into());
    }
    Ok(path)
}
pub(super) fn sort(column: FolderColumn) -> u8 {
    match column {
        FolderColumn::Name => 0,
        FolderColumn::Type => 1,
        FolderColumn::Modified => 2,
        FolderColumn::Size => 3,
    }
}
pub(super) fn mask(columns: &[FolderColumn]) -> Result<u8, String> {
    let mut bits = 0;
    for column in columns {
        let bit = 1
            << match column {
                FolderColumn::Name => 0,
                FolderColumn::Modified => 1,
                FolderColumn::Type => 2,
                FolderColumn::Size => 3,
            };
        if bits & bit != 0 {
            return Err("duplicate visible column".into());
        }
        bits |= bit;
    }
    if bits & 1 == 0 {
        return Err("name column must remain visible".into());
    }
    Ok(bits)
}
pub(super) fn preferences(p: &FolderPreferences) -> serde_json::Value {
    json!({"sort_column":match p.sort_column{0=>"name",1=>"type",2=>"modified",_=>"size"},"descending":p.descending,
        "column_widths":p.column_widths,"column_order":["name","modified","type","size"],
        "visible_columns":(["name","modified","type","size"].into_iter().enumerate().filter(|(at,_)|p.visible_columns&(1<<at)!=0).map(|(_,name)|name).collect::<Vec<_>>())})
}
pub(super) fn query(s: &PaneApp, id: PanelId) -> Result<serde_json::Value, String> {
    let panel = s
        .workspace
        .panel(id)
        .filter(|p| p.folder().is_some())
        .ok_or("folder panel does not exist")?;
    let prefs = s.store.folder_preferences(id).map_err(|e| e.to_string())?;
    let source = s.folders.get(&id);
    Ok(
        json!({"content_layout":super::content_layout::live_query(s,id),"pane_id":id.get().to_string(),"root_path":panel.folder(),"list_view":panel.list_view(),"preferences":preferences(&prefs),
        "runtime":s.views.iter().find(|v|v.id==id).map(|v|{let m=v.model.borrow();json!({"list_view":m.list_view,"sort_column_index":m.folder_sort.0,"descending":m.folder_sort.1,"column_widths":m.folder_columns,"visible_columns_mask":m.folder_visible_columns})}),"content_revision":source.map(|s|s.content_revision.to_string()),"current_path":source.map(|s|&s.path),"loading":source.is_some_and(|s|s.loading),"available":source.is_some(),"error":source.and_then(|s|s.status.as_deref()),
        "can_navigate":source.map(|s|s.navigation()),"inventory_source":"current_app_snapshot",
        "items":source.map(|s|s.items.iter().map(|item|json!({"display_name":item.label,"path":item.identity.file_system_path(),"is_folder":item.details.folder,"kind":item.details.kind,"size":item.details.size})).collect::<Vec<_>>()).unwrap_or_default()}),
    )
}
