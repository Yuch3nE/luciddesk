//! Apply rules through the same placement and persistence path as manual grouping.
use super::*;

pub(super) fn apply(s: &mut PaneApp) -> Result<(), String> {
    apply_mode(s, false).map(|_| ())
}

pub(super) fn apply_mode(s: &mut PaneApp, manual: bool) -> Result<usize, String> {
    let settings = s.store.settings().map_err(|e| e.to_string())?;
    let Some(luciddesk_storage::SettingValue::Text(raw)) = settings.get("desktop.classification") else { return Ok(0); };
    let config: luciddesk_core::Classification = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    config.validate()?;
    let session = s.session.as_mut().unwrap();
    let live: std::collections::HashSet<_> = s.workspace.desktop_items().iter().map(|i| i.identity().persistent_key()).collect();
    session.classification_seen.retain(|key| live.contains(key));
    if !config.enabled && !manual { return Ok(0); }
    let mut moves = Vec::new();
    for item in s.workspace.desktop_items() {
        let key = item.identity().persistent_key();
        let seen = !session.classification_seen.insert(key.clone());
        if (seen && !manual) || !matches!(item.placement(), DesktopPlacement::FreeDesktop { .. }) { continue; }
        if let Some(rule) = config.rules.iter().find(|rule| rule.matches(item)
            && s.workspace.panel(PanelId::new(rule.target_panel)).is_some_and(|p| p.supports_tabs() && !p.locked())) {
            moves.push((key, PanelId::new(rule.target_panel)));
        }
    }
    let count = moves.len();
    for (key, target) in moves {
        if s.workspace.panel(target).is_some_and(Panel::fixed_grid) {
            fixed_grid::place(&mut s.workspace, target, &[key], GridPosition::new(0, 0));
        } else {
            let next = s.workspace.desktop_items().iter().filter_map(|i| match i.placement() {
                DesktopPlacement::Pane { pane_id, position } if *pane_id == target => Some(position.column.saturating_add(1)),
                _ => None,
            }).max().unwrap_or(0);
            let item = s.workspace.desktop_items_mut().iter_mut().find(|i| i.identity().persistent_key() == key).unwrap();
            item.set_placement(DesktopPlacement::Pane { pane_id: target, position: GridPosition::new(next, 0) });
        }
    }
    Ok(count)
}
