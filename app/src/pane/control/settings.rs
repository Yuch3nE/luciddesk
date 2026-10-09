//! Apply loaded settings without rebuilding ordinary/folder windows or resetting interactions.
mod metadata;
use super::*;
use crate::pane::search::{everything_settings, hotkey as search_hotkey};
use luciddesk_storage::SettingValue;
use std::collections::BTreeMap;

pub(super) fn values(
    settings: BTreeMap<String, SettingValue>,
) -> serde_json::Map<String, serde_json::Value> {
    settings
        .into_iter()
        .map(|(key, value)| {
            (
                key,
                match value {
                    SettingValue::Boolean(v) => json!(v),
                    SettingValue::Number(v) => json!(v),
                    SettingValue::Text(v) => json!(v),
                },
            )
        })
        .collect()
}

// A plan uses one persistence domain so its commit remains atomic.
pub(super) fn preview(store: &WorkspaceStore, updates: &BTreeMap<String, SettingValue>) -> Result<(BTreeMap<String, SettingValue>, BTreeMap<String, SettingValue>), String> {
    if updates.keys().any(|key| metadata::contains(key)) {
        let before = metadata::read(store)?;
        let after = metadata::patch(&before, updates)?;
        Ok((before, after))
    } else {
        Ok((store.settings().map_err(|e| e.to_string())?, store.preview_settings(updates).map_err(|e| e.to_string())?))
    }
}
pub(super) fn save(store: &WorkspaceStore, updates: &BTreeMap<String, SettingValue>) -> Result<(), String> {
    if updates.keys().any(|key| metadata::contains(key)) {
        let before = metadata::read(store)?;
        let after = metadata::patch(&before, updates)?;
        let entries = metadata::encode_changes(&before, &after);
        store.save_metadata_preferences(&entries.iter().map(|(k,v)| (k.as_str(),v.as_str())).collect::<Vec<_>>()).map_err(|e|e.to_string())
    } else { store.save_settings(updates).map_err(|e|e.to_string()) }
}
pub(super) fn workspace_values(store: &WorkspaceStore) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    metadata::read(store).map(values)
}

pub(super) fn runtime(state: &PaneApp) -> serde_json::Value {
    json!({
        "font_family":fonts::family(),
        "title_emoji_color":title_emoji::color(),
        "compact_menu":compact_menu::enabled(),
        "header_divider":header_divider::enabled(),
        "diagnostics_level":format!("{:?}", luciddesk_diagnostics::level()).to_ascii_lowercase(),
        "search_visible":state.views.iter().any(|v| state.workspace.panel(v.id).is_some_and(Panel::is_search)),
        "preview_enabled":peek::settings().enabled,
        "search_shortcut_status":search_hotkey::status(),
        "show_panels_shortcut_status":show_hotkey::status(),
        "grid_scale":state.workspace.pane_options().grid_scale,
        "corner_radius":state.workspace.pane_options().corner_radius,
    })
}

pub(super) fn present(state: &Rc<RefCell<PaneApp>>) -> Result<(), String> {
    let (language_changed, search_enabled, preview_enabled) = {
        let mut s = state.borrow_mut();
        // Read persisted inheritance flags; copy only settings, never replace live inventory/layout.
        let loaded = s.store.load_workspace().map_err(|e| e.to_string())?;
        let old_options = s.workspace.pane_options();
        let options = loaded.pane_options();
        s.workspace.set_pane_options(options);
        if let Some((theme, backdrop)) = loaded.appearance() {
            s.workspace.set_appearance_defaults(theme, backdrop);
        }
        let ids: Vec<_> = s.workspace.panels().iter().map(Panel::id).collect();
        for id in ids {
            if let Some(saved) = loaded.panel(id) {
                let panel = s.workspace.panel_mut(id).unwrap();
                panel.set_theme(saved.theme());
                panel.set_backdrop(saved.backdrop());
            }
        }
        load_log_level(&s.store)?;
        peek::load(&s.store)?;
        everything_settings::load(&s.store)?;
        search_hotkey::load(&s.store)?;
        let language_changed = crate::i18n::initialize(&s.store)?;
        fonts::load(&s.store)?;
        title_emoji::load(&s.store)?;
        compact_menu::load(&s.store)?;
        header_divider::load(&s.store)?;
        for view in &s.views {
            if let Some(panel) = s.workspace.panel(view.id) {
                let mut model = view.model.borrow_mut();
                model.options = options;
                model.theme = panel.theme();
                model.dark = theme::is_dark(panel.theme());
                model.backdrop = panel.backdrop();
                if !model.is_list() && options.grid_scale != old_options.grid_scale {
                    model.scroll = 0;
                    model.hovered_item = None;
                }
            }
            unsafe {
                windows_sys::Win32::Graphics::Gdi::InvalidateRect(
                    view.window.hwnd().cast(),
                    std::ptr::null(),
                    0,
                );
            }
        }
        if let Some(settings) = &s.settings {
            unsafe {
                windows_sys::Win32::Graphics::Gdi::InvalidateRect(
                    settings.hwnd().cast(),
                    std::ptr::null(),
                    0,
                );
            }
        }
        let configured = s.store.settings().map_err(|e| e.to_string())?;
        (
            language_changed,
            configured["search.enabled"] == SettingValue::Boolean(true),
            configured["preview.enabled"] == SettingValue::Boolean(true),
        )
    };
    if language_changed {
        unsafe extern "system" fn notify(
            hwnd: windows_sys::Win32::Foundation::HWND,
            _: isize,
        ) -> i32 {
            unsafe {
                PostMessageW(hwnd, crate::i18n::CHANGED, 0, 0);
                windows_sys::Win32::Graphics::Gdi::InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            1
        }
        unsafe {
            EnumThreadWindows(
                windows_sys::Win32::System::Threading::GetCurrentThreadId(),
                Some(notify),
                0,
            );
        }
    }
    let search_views: Vec<_> = {
        let s = state.borrow();
        s.views
            .iter()
            .filter(|v| s.workspace.panel(v.id).is_some_and(Panel::is_search))
            .map(|v| v.id)
            .collect()
    };
    let mut errors = Vec::new();
    if search_enabled {
        if everything_settings::resolved(&everything_settings::settings()).is_none() {
            errors.push("Search is enabled but Everything.exe is unavailable".to_owned());
        } else if search_views.is_empty() {
            if let Err(error) = events::handle(state, PanelId::new(0), Event::EnableSearch) {
                errors.push(error);
            }
        }
    } else {
        for id in search_views {
            if let Err(error) = events::handle(state, id, Event::ClosePane) {
                errors.push(error);
            }
        }
    }
    if preview_enabled && !peek::settings().enabled {
        errors.push("Preview is enabled but its provider is unavailable".to_owned());
    }
    let wake = state.borrow().wake.clone();
    if !wake.refresh_hotkeys() {
        errors.push(
            "Global shortcut registration is unavailable; inspect settings runtime status"
                .to_owned(),
        );
    }
    wake.notify();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_refresh_keeps_windows_and_transient_collapse_without_extra_writes() {
        let _sta = crate::pane::test_support::apartment();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspace.db");
        let mut app = super::super::super::tests::test_state();
        app.store = WorkspaceStore::open(&path).unwrap();
        app.workspace
            .panel_mut(PanelId::new(1))
            .unwrap()
            .set_auto_hide(true);
        app.store.save_workspace(&app.workspace).unwrap();
        app.runtime = Some(crate::pane::runtime::State::new(path));
        let state = Rc::new(RefCell::new(app));
        let _runtime = crate::pane::runtime::supervisor(&state).unwrap();
        create_view(&state, PanelId::new(1)).unwrap();
        let hwnd = state.borrow().views[0].window.hwnd();
        state.borrow().views[0].model.borrow_mut().collapsed = true;
        state
            .borrow()
            .store
            .save_settings(&BTreeMap::from([
                (
                    "panel_defaults.grid_scale".into(),
                    SettingValue::Number(125.0),
                ),
                (
                    "panel_defaults.corner_radius".into(),
                    SettingValue::Number(10.0),
                ),
            ]))
            .unwrap();
        let revision = state.borrow().store.change_count();
        present(&state).unwrap();
        {
            let s = state.borrow();
            assert_eq!(s.store.change_count(), revision);
            assert_eq!(s.views[0].window.hwnd(), hwnd);
            let model = s.views[0].model.borrow();
            assert!(model.collapsed);
            assert_eq!(model.options.grid_scale, 125.0);
            assert_eq!(s.workspace.pane_options().corner_radius, 10.0);
        }
        present(&state).unwrap();
        assert_eq!(state.borrow().store.change_count(), revision);
        for view in &state.borrow().views {
            window::prepare_close(view.window.hwnd().cast());
        }
    }
}
