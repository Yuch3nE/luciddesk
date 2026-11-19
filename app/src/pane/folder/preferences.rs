//! Folder view defaults and per-pane column preferences.
use crate::pane::{PaneApp, columns};
use luciddesk_core::{Panel, PanelId};
use luciddesk_storage::WorkspaceStore;

const ALL_COLUMNS: u8 = 15;

fn normalized_columns(value: u8) -> u8 {
    // The name column is required; discard unknown column bits from storage.
    (value & ALL_COLUMNS) | 1
}

fn save_visible_columns(store: &WorkspaceStore, id: PanelId, visible: u8) -> Result<(), String> {
    store
        .save_preference(
            &format!("panel_folder_visible_columns:{}", id.get()),
            &visible.to_string(),
        )
        .map_err(|error| error.to_string())
}

pub(in crate::pane) fn saved_columns(
    store: &WorkspaceStore,
    id: PanelId,
) -> Result<Option<[f32; 4]>, String> {
    Ok(store
        .preference(&format!("panel_folder_columns:{}", id.get()))
        .map_err(|error| error.to_string())?
        .and_then(|value| columns::decode(&value)))
}

pub(in crate::pane) fn visible_columns(store: &WorkspaceStore, id: PanelId) -> Result<u8, String> {
    Ok(store
        .preference(&format!("panel_folder_visible_columns:{}", id.get()))
        .map_err(|error| error.to_string())?
        .and_then(|value| value.parse::<u8>().ok())
        .map_or(ALL_COLUMNS, normalized_columns))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::pane) struct Defaults {
    pub list: bool,
    pub columns: u8,
}
impl Default for Defaults {
    fn default() -> Self {
        Self {
            list: true,
            columns: ALL_COLUMNS,
        }
    }
}
impl Defaults {
    pub const KEY: &'static str = "folder_panel_defaults";

    pub fn load(store: &WorkspaceStore) -> Result<Self, String> {
        let saved = store.preference(Self::KEY).map_err(|e| e.to_string())?;
        Ok(saved
            .and_then(|value| {
                let (view, columns) = value.split_once(',')?;
                let list = match view {
                    "list" => true,
                    "icons" => false,
                    _ => return None,
                };
                Some(Self {
                    list,
                    columns: normalized_columns(columns.parse::<u8>().ok()?),
                })
            })
            .unwrap_or_default())
    }
    pub fn encode(self) -> String {
        let view = if self.list { "list" } else { "icons" };
        format!("{view},{}", normalized_columns(self.columns))
    }
    pub fn save(self, store: &WorkspaceStore) -> Result<(), String> {
        store
            .save_preference(Self::KEY, &self.encode())
            .map_err(|e| e.to_string())
    }
    pub fn apply(self, store: &WorkspaceStore, panel: &mut Panel) -> Result<(), String> {
        panel.set_list_view(self.list);
        save_visible_columns(store, panel.id(), normalized_columns(self.columns))
    }
}

pub(in crate::pane) fn toggle_column(
    state: &PaneApp,
    id: PanelId,
    column: u8,
) -> Result<(), String> {
    if !(1..=3).contains(&column) {
        return Ok(());
    }
    let Some(view) = state.views.iter().find(|view| view.id == id) else {
        return Ok(());
    };
    let visible = (view.model.borrow().folder_visible_columns ^ (1 << column)) | 1;
    save_visible_columns(&state.store, id, visible)?;
    view.model.borrow_mut().folder_visible_columns = visible;
    unsafe {
        windows_sys::Win32::Graphics::Gdi::InvalidateRect(
            view.window.hwnd().cast(),
            std::ptr::null(),
            0,
        );
    }
    Ok(())
}

pub(in crate::pane) fn save_columns(
    state: &PaneApp,
    id: PanelId,
    widths: [f32; 4],
) -> Result<(), String> {
    if !columns::valid(widths) {
        return Ok(());
    }
    state
        .store
        .save_preference(
            &format!("panel_folder_columns:{}", id.get()),
            &widths.map(|value| format!("{value:.6}")).join(","),
        )
        .map_err(|error| error.to_string())
}
