//! Validate on a workspace copy, then commit once. No window or Shell calls here.
use super::*;
use luciddesk_api::{Context, Operation, Plan};
use std::collections::{HashSet, VecDeque};
const TTL: Duration = Duration::from_secs(300);
pub(super) struct Prepared {
    token: String,
    base: Context,
    expires: Instant,
    next: Workspace,
    refs: HashMap<String, String>,
    diff: serde_json::Value,
    membership: bool,
    applied: bool,
    settings: Option<std::collections::BTreeMap<String, luciddesk_storage::SettingValue>>,
    settings_changed: bool,
    layout: Option<(String, Vec<(PanelId, RectDip)>)>,
    layout_observation: Option<super::geometry::LayoutObservation>,
    folders: Vec<(PanelId, luciddesk_storage::FolderPreferences)>,
    folders_changed: bool,
    runtime_action: Option<Operation>,
}
struct Receipt {
    desktop_membership: Option<Vec<String>>,
    runtime_action: Option<Operation>,
    id: String,
    signature: String,
    response: Response,
    expires: Instant,
}
#[derive(Default)]
pub(super) struct Plans {
    metadata: super::sort_metadata::Cache,
    pending: VecDeque<Prepared>,
    receipts: VecDeque<Receipt>,
}
fn invalid(message: impl Into<String>) -> (String, String) {
    ("INVALID_REQUEST".into(), message.into())
}
fn id(raw: &str) -> Result<PanelId, (String, String)> {
    raw.parse::<u64>()
        .ok()
        .filter(|n| *n > 0 && *n <= i64::MAX as u64)
        .map(PanelId::new)
        .ok_or_else(|| invalid("invalid panel ID"))
}
fn editable(workspace: &Workspace, id: PanelId) -> Result<(), (String, String)> {
    let panel = workspace
        .panel(id)
        .ok_or_else(|| ("NOT_FOUND".into(), "panel does not exist".into()))?;
    if !panel.supports_tabs() {
        return Err(invalid("only desktop panels are supported"));
    }
    if panel.locked() {
        return Err(("PANE_LOCKED".into(), "panel is locked".into()));
    }
    Ok(())
}
fn ordered(workspace: &Workspace, pane: PanelId) -> Vec<String> {
    let mut items: Vec<_> = workspace
        .desktop_items()
        .iter()
        .filter_map(|item| match item.placement() {
            DesktopPlacement::Pane { pane_id, position } if *pane_id == pane => Some((
                (position.row, position.column),
                item.identity().persistent_key(),
            )),
            _ => None,
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    items.into_iter().map(|(_, key)| key).collect()
}
fn order(workspace: &mut Workspace, pane: PanelId, keys: &[String]) {
    let fixed = workspace.panel(pane).is_some_and(Panel::fixed_grid);
    let columns = fixed_grid::visible_columns(workspace, pane);
    let free = workspace.panel(pane).is_some_and(Panel::free_layout);
    let metrics = free_layout::grid(workspace);
    for (at, key) in keys.iter().enumerate() {
        if let Some(item) = workspace
            .desktop_items_mut()
            .iter_mut()
            .find(|i| i.identity().persistent_key() == *key)
        {
            fixed_grid::set_position(item, pane, if fixed { fixed_grid::position(at, columns) } else { GridPosition::new(at as u32, 0) }, free, metrics);
        }
    }
}
fn keys(
    workspace: &Workspace,
    tokens: &[String],
    ids: &HashMap<String, String>,
) -> Result<Vec<String>, (String, String)> {
    if tokens.is_empty() || tokens.len() > 10000 {
        return Err(invalid("expected 1..10000 items"));
    }
    let mut seen = HashSet::new();
    tokens
        .iter()
        .map(|token| {
            if !seen.insert(token) {
                return Err(invalid("duplicate item ID"));
            }
            ids.iter()
                .find(|(_, v)| *v == token)
                .map(|(key, _)| key.clone())
                .filter(|key| {
                    workspace
                        .desktop_items()
                        .iter()
                        .any(|i| i.identity().persistent_key() == *key)
                })
                .ok_or_else(|| {
                    (
                        "NOT_FOUND".into(),
                        "item ID expired or does not exist; query workspace again".into(),
                    )
                })
        })
        .collect()
}
fn release(workspace: &mut Workspace, moving: &[String]) -> Result<(), (String, String)> {
    let moving: HashSet<_> = moving.iter().collect();
    let mut sources = HashSet::new();
    for item in workspace.desktop_items() {
        if moving.contains(&item.identity().persistent_key()) {
            if let DesktopPlacement::Pane { pane_id, .. } = item.placement() {
                editable(workspace, *pane_id)?;
                sources.insert(*pane_id);
            }
        }
    }
    for item in workspace.desktop_items_mut() {
        if moving.contains(&item.identity().persistent_key())
            && matches!(item.placement(), DesktopPlacement::Pane { .. })
        {
            item.set_placement(DesktopPlacement::default());
        }
    }
    for pane in sources {
        if workspace.panel(pane).is_some_and(Panel::fixed_grid) { continue; }
        let items = ordered(workspace, pane);
        order(workspace, pane, &items);
    }
    Ok(())
}
fn summary(workspace: &Workspace) -> serde_json::Value {
    json!({"tabs":workspace.tab_groups().iter().map(|g|json!({"active":g.active.get().to_string(),"members":g.members.iter().map(|id|id.get().to_string()).collect::<Vec<_>>()})).collect::<Vec<_>>(),"panes":workspace.panels().iter().map(|p|json!({"id":p.id().get().to_string(),"title":p.title(),"locked":p.locked(),"auto_hide":p.auto_hide(),"manual_collapsed":p.collapsed(),"always_on_top":p.always_on_top(),"folder_path":p.folder(),"list_view":p.list_view(),"auto_compact":!p.fixed_grid(),"align_icons_to_grid":!p.free_layout()})).collect::<Vec<_>>(),
        "placements":workspace.desktop_items().iter().map(|item|{
            let placement=match item.placement(){DesktopPlacement::Pane{pane_id,position}=>json!({"pane_id":pane_id.get().to_string(),"column":position.column,"row":position.row,"position_dip":item.pane_position().map(|p|json!({"x":p.x,"y":p.y}))}),_=>serde_json::Value::Null};
            (item.identity().persistent_key(),placement)
        }).collect::<std::collections::BTreeMap<_,_>>()})
}
fn prepare(
    workspace: &Workspace,
    store: &WorkspaceStore,
    plan: &Plan,
    ids: &HashMap<String, String>,
    monitors: &[luciddesk_window::MonitorDescriptor],
    folder_content: &super::content_layout::FolderSnapshots,
    observed: &super::geometry::LayoutObservation,
    metadata: Option<&HashMap<String, ItemDetails>>,
) -> Result<Prepared, (String, String)> {
    if plan.protocol_version != 1 {
        return Err(("PROTOCOL_MISMATCH".into(), "plan protocol must be 1".into()));
    }
    if plan.operations.is_empty() || plan.operations.len() > 256 {
        return Err(invalid("expected 1..256 operations"));
    }
    if plan.operations.iter().any(super::transient::is_runtime) {
        if plan.operations.len()!=1 {return Err(invalid("runtime operations must be submitted in separate plans"));}
        let action=super::transient::prepare(workspace,&plan.operations[0]).map_err(invalid)?;
        return Ok(Prepared {
            token:luciddesk_api::request_id(),base:plan.base.clone(),expires:Instant::now()+TTL,
            next:workspace.clone(),refs:HashMap::new(),diff:json!({"runtime_action":action,"persistence":if matches!(action,Operation::StartupSet{..}){"operating_system"}else{"none"}}),membership:false,applied:false,
            settings:None,settings_changed:false,layout:None,layout_observation:None,folders:Vec::new(),folders_changed:false,runtime_action:Some(action),
        });
    }
    if plan.operations.iter().any(|op| matches!(op, Operation::SettingsUpdate { .. })) {
        let [Operation::SettingsUpdate { values }] = plan.operations.as_slice() else {
            return Err(invalid("settings.update must be the only operation; batch fields in values"));
        };
        if values.is_empty() { return Err(invalid("settings update requires at least one field")); }
        let updates = values.iter().map(|(key, value)| {
            use luciddesk_api::SettingValue as Wire;
            use luciddesk_storage::SettingValue as Stored;
            (key.clone(), match value {
                Wire::Boolean(v) => Stored::Boolean(*v),
                Wire::Number(v) => Stored::Number(*v),
                Wire::Text(v) => Stored::Text(v.clone()),
            })
        }).collect();
        let (before, after) = super::settings::preview(store, &updates).map_err(invalid)?;
        return Ok(Prepared {
            token: luciddesk_api::request_id(), base: plan.base.clone(), expires: Instant::now() + TTL,
            next: workspace.clone(), refs: HashMap::new(), membership: false, applied: false,
            settings_changed: before != after,
            diff: json!({"before":super::settings::values(before),"after":super::settings::values(after)}),
            settings: Some(updates),
            layout: None,
            layout_observation: None,
            folders: Vec::new(),
            folders_changed: false,
            runtime_action: None,
        });
    }
    let mut next = workspace.clone();
    let mut refs = HashMap::new();
    let mut membership = false;
    let mut geometry = HashMap::new();
    let mut folders: HashMap<PanelId,luciddesk_storage::FolderPreferences> = HashMap::new();
    let mut folders_before = serde_json::Map::new();
    let mut next_id = workspace
        .panels()
        .iter()
        .map(|p| p.id().get())
        .max()
        .unwrap_or(0);
    let mut operations: VecDeque<_> = plan.operations.iter().cloned().collect();
    while let Some(operation) = operations.pop_front() {
        let operation = &operation;
        match operation {
            Operation::FolderFit { .. } | Operation::Snap { .. } | Operation::Fit { .. } | Operation::Arrange { .. } => {
                let targets: Vec<&str> = match operation {
                    Operation::FolderFit { pane_id, .. } | Operation::Snap { pane_id, .. } | Operation::Fit { pane_id, .. } => vec![pane_id.as_str()],
                    Operation::Arrange { columns, .. } => columns.iter().flatten().map(String::as_str).collect(),
                    _ => unreachable!(),
                };
                for raw in targets {
                    let panel = next.panel(id(raw)?).ok_or_else(|| ("NOT_FOUND".into(), "panel does not exist".into()))?;
                    if panel.locked() { return Err(("PANE_LOCKED".into(), "explicitly unlock the panel first".into())); }
                }
                if let Operation::Snap { pane_id, target_pane_id, .. } = operation {
                    if [id(pane_id)?, id(target_pane_id)?].iter().any(|id| observed.collapsed.contains(id)) {
                        return Err(invalid("reveal both panels before snapping; an auto-hidden panel is currently collapsed"));
                    }
                }
                let mut positions = observed.positions.clone();
                positions.extend(geometry.iter().map(|(id, bounds)| (*id, *bounds)));
                let expanded = super::content_layout::expand_with_folders(&next, store, operation, monitors, &positions, folder_content).map_err(invalid)?;
                for op in expanded.into_iter().rev() { operations.push_front(op); }
            }
            Operation::FolderRefresh{..}|Operation::FolderNavigate{..}|Operation::FolderBack{..}|Operation::FolderHome{..}|Operation::SearchQuery{..}|Operation::SearchRefresh{..}|Operation::SearchMore{..} => unreachable!("handled above"),
            Operation::StartupSet { .. } | Operation::SettingsUpdate { .. } => unreachable!("handled above"),
            Operation::FolderUpdate { pane_id, path, list_view, sort_column, descending, column_widths, visible_columns } => {
                let target=id(pane_id)?;
                let panel=next.panel(target).filter(|p|p.folder().is_some()).ok_or_else(||("NOT_FOUND".into(),"folder panel does not exist".into()))?;
                if panel.locked(){return Err(("PANE_LOCKED".into(),"explicitly unlock the panel first".into()));}
                if path.is_none() && list_view.is_none() && sort_column.is_none() && descending.is_none() && column_widths.is_none() && visible_columns.is_none(){return Err(invalid("folder update requires at least one field"));}
                if !folders.contains_key(&target) {
                    let original=store.folder_preferences(target).map_err(|e|invalid(e.to_string()))?;
                    folders_before.insert(pane_id.clone(),super::folders::preferences(&original));
                    folders.insert(target,original);
                }
                let settings=folders.get_mut(&target).unwrap();
                if let Some(column)=sort_column {settings.sort_column=super::folders::sort(*column);}
                if let Some(value)=descending {settings.descending=*value;}
                if let Some(widths)=column_widths {settings.column_widths=Some(*widths);}
                if let Some(columns)=visible_columns {settings.visible_columns=super::folders::mask(columns).map_err(invalid)?;}
                settings.validate().map_err(|e|invalid(e.to_string()))?;
                let panel=next.panel_mut(target).unwrap();
                if let Some(path)=path {panel.set_folder(Some(super::folders::path(path).map_err(invalid)?));}
                if let Some(value)=list_view {panel.set_list_view(*value);}
            }

            Operation::TabMerge { .. } | Operation::TabSelect { .. } | Operation::TabReorder { .. } | Operation::TabDetach { .. } => {
                super::tab_plan::apply(&mut next, operation).map_err(invalid)?;
            }

            Operation::Geometry { pane_id, monitor_id, x, y, width, height } => {
                let target = id(pane_id)?;
                let panel = next.panel(target).ok_or_else(|| ("NOT_FOUND".into(), "panel does not exist".into()))?;
                if panel.locked() { return Err(("PANE_LOCKED".into(), "explicitly unlock the panel first".into())); }
                let monitor = monitors.iter().find(|m| m.id.as_str() == monitor_id)
                    .ok_or_else(|| invalid("unknown monitor ID; query monitor list again"))?;
                let minimum = super::content_layout::minimum(&next, target, *width, folder_content);
                if *width + 0.01 < minimum.0 || *height + 0.01 < minimum.1 {
                    return Err(invalid(format!("geometry must be at least {} x {} DIP for this panel", minimum.0, minimum.1)));
                }
                let (stored, physical) = super::geometry::convert(monitor, RectDip { x:*x,y:*y,width:*width,height:*height }).map_err(invalid)?;
                let members = next.tab_group(target).map_or_else(||vec![target],|g|g.members.clone());
                for member in members {
                    next.panel_mut(member).unwrap().set_rect(stored);
                    geometry.insert(member, physical);
                }
            }

            Operation::Create { reference, title } | Operation::FolderCreate { reference, title, .. } => {
                if reference.is_empty() || reference.len() > 128 || refs.contains_key(reference) {
                    return Err(invalid("invalid or duplicate panel ref"));
                }
                if title.trim().is_empty() || title.chars().count() > 256 {
                    return Err(invalid("title must contain 1..256 characters"));
                }
                next_id = next_id
                    .checked_add(1)
                    .filter(|n| *n <= i64::MAX as u64)
                    .ok_or_else(|| invalid("panel ID exhausted"))?;
                let number = next_id;
                let mut panel = Panel::new(
                    PanelId::new(number),
                    title,
                    display_layout::new_pane(&next, false),
                );
                if let Operation::FolderCreate { path, .. } = operation {
                    panel.set_folder(Some(super::folders::path(path).map_err(invalid)?));
                    let defaults=folder::Defaults::load(store).map_err(invalid)?;
                    panel.set_list_view(defaults.list);
                    folders.insert(panel.id(), luciddesk_storage::FolderPreferences { visible_columns:defaults.columns,..Default::default() });
                } else {
                    layout_defaults::Mode::load(store).map_err(invalid)?.apply(&mut panel);
                }
                next.add_panel(panel).map_err(|e| invalid(e.to_string()))?;
                refs.insert(reference.clone(), number.to_string());
            }
            Operation::Update {
                pane_id,
                auto_compact,
                align_icons_to_grid,
                title,
                locked,
                auto_hide,
                collapsed,
                always_on_top,
            } => {
                let target = id(pane_id)?;
                let panel = next
                    .panel(target)
                    .ok_or_else(|| ("NOT_FOUND".into(), "panel does not exist".into()))?;
                if panel.is_search() {
                    return Err(invalid("search panel window options are controlled separately"));
                }
                if title.is_none()
                    && auto_compact.is_none() && align_icons_to_grid.is_none()
                    && locked.is_none()
                    && auto_hide.is_none()
                    && collapsed.is_none()
                    && always_on_top.is_none()
                {
                    return Err(invalid("update requires at least one field"));
                }
                if panel.locked() && *locked != Some(false) {
                    return Err((
                        "PANE_LOCKED".into(),
                        "explicitly unlock the panel first".into(),
                    ));
                }
                if auto_compact.is_some() || align_icons_to_grid.is_some() {
                    if !panel.supports_tabs() { return Err(invalid("arrangement options require a desktop pane")); }
                    if *auto_compact == Some(true) && *align_icons_to_grid == Some(false) {
                        return Err(invalid("auto_compact=true requires grid alignment"));
                    }
                    if *locked == Some(false) { next.panel_mut(target).unwrap().set_locked(false); }
                    if let Some(compact) = auto_compact {
                        if *compact == next.panel(target).unwrap().fixed_grid() { fixed_grid::toggle(&mut next, target).map_err(invalid)?; }
                    }
                    if let Some(aligned) = align_icons_to_grid {
                        if *aligned == next.panel(target).unwrap().free_layout() { free_layout::toggle(&mut next, target).map_err(invalid)?; }
                    }
                    membership = true;
                }
                if let Some(title) = title {
                    if title.trim().is_empty() || title.chars().count() > 256 {
                        return Err(invalid("title must contain 1..256 characters"));
                    }
                    next.panel_mut(target).unwrap().set_title(title);
                }
                // Window options belong to every member of a tab group, including inactive tabs.
                let members = next
                    .tab_group(target)
                    .map_or_else(|| vec![target], |g| g.members.clone());
                for member in members {
                    let panel = next.panel_mut(member).unwrap();
                    if let Some(value) = locked {
                        panel.set_locked(*value);
                    }
                    if let Some(value) = auto_hide {
                        panel.set_auto_hide(*value);
                    }
                    if let Some(value) = collapsed {
                        panel.set_collapsed(*value);
                    }
                    if let Some(value) = always_on_top {
                        panel.set_always_on_top(*value);
                    }
                }
            }
            Operation::Release { item_ids } => {
                let moving = keys(&next, item_ids, ids)?;
                release(&mut next, &moving)?;
                membership = true;
            }
            Operation::Remove {
                pane_id,
                release_items,
            } => {
                let target = id(pane_id)?;
                let panel=next.panel(target).ok_or_else(||("NOT_FOUND".into(),"panel does not exist".into()))?;
                if panel.is_search(){return Err(invalid("disable search through settings.update"));}
                if panel.locked(){return Err(("PANE_LOCKED".into(),"panel is locked".into()));}
                let members = ordered(&next, target);
                if !members.is_empty() && !release_items {
                    return Err(invalid("panel is not empty; set release_items explicitly"));
                }
                if !members.is_empty() {
                    release(&mut next, &members)?;
                    membership = true;
                }
                next.remove_panel(target);
            }
            Operation::Assign {
                item_ids,
                pane_id,
                pane_ref,
            } => {
                membership = true;
                let target = match (pane_id, pane_ref) {
                    (Some(raw), None) => id(raw)?,
                    (None, Some(reference)) => id(refs
                        .get(reference)
                        .ok_or_else(|| invalid("unknown panel ref"))?)?,
                    _ => return Err(invalid("provide exactly one of pane_id/pane_ref")),
                };
                editable(&next, target)?;
                let moving = keys(&next, item_ids, ids)?;
                let mut sources = HashSet::new();
                for item in next.desktop_items() {
                    if moving.contains(&item.identity().persistent_key()) {
                        if let DesktopPlacement::Pane { pane_id, .. } = item.placement() {
                            editable(&next, *pane_id)?;
                            sources.insert(*pane_id);
                        }
                    }
                }
                if next.panel(target).is_some_and(Panel::fixed_grid) {
                    let existing = ordered(&next, target);
                    let keys: Vec<_> = moving.iter().filter(|key| !existing.contains(key)).cloned().collect();
                    fixed_grid::place(&mut next, target, &keys, GridPosition::new(0, 0));
                } else {
                    let mut destination = ordered(&next, target);
                    for key in moving {
                        if !destination.contains(&key) {
                            destination.push(key);
                        }
                    }
                    order(&mut next, target, &destination);
                }
                for pane in sources {
                    if next.panel(pane).is_some_and(Panel::fixed_grid) { continue; }
                    let items = ordered(&next, pane);
                    order(&mut next, pane, &items);
                }
            }
            Operation::Sort { pane_id, descending, sort_column } => {
                let target = id(pane_id)?;
                editable(&next, target)?;
                if *sort_column == luciddesk_api::FolderColumn::Name {
                    membership |= super::super::sorting::apply(&mut next, target, *descending).map_err(invalid)?;
                } else {
                    let metadata = metadata.ok_or_else(|| invalid("sort metadata is not ready"))?;
                    let mut items: Vec<_> = ordered_desktop_items(&next, target).iter().map(|item| {
                        let details = metadata.get(&item.identity().persistent_key()).cloned().ok_or_else(|| invalid("sort metadata missing for an item"))?;
                        Ok(Item { identity: item.identity().clone(), label: item.display_name().to_owned(), image: None, details })
                    }).collect::<Result<_, (String, String)>>()?;
                    let column = match sort_column { luciddesk_api::FolderColumn::Type => 1, luciddesk_api::FolderColumn::Modified => 2, _ => 3 };
                    folder::sort_items(&mut items, (column, *descending));
                    let ranks = items.iter().enumerate().map(|(i, item)| (item.identity.persistent_key(), i)).collect();
                    membership |= super::super::sorting::apply_order(&mut next, target, false, Some(&ranks)).map_err(invalid)?;
                }
            }
            Operation::Position { pane_id, item_id, column, row, x, y } => {
                let target = id(pane_id)?;
                editable(&next, target)?;
                let panel = next.panel(target).unwrap();
                if !panel.fixed_grid() { return Err(invalid("disable auto_compact before positioning items")); }
                let key = keys(&next, std::slice::from_ref(item_id), ids)?.remove(0);
                let item = next.desktop_items().iter().find(|i| i.identity().persistent_key() == key).unwrap();
                if !matches!(item.placement(), DesktopPlacement::Pane { pane_id, .. } if *pane_id == target) { return Err(invalid("item must already belong to the target pane")); }
                let identity = item.identity().clone();
                if panel.free_layout() {
                    let (Some(x), Some(y), None, None) = (x, y, column, row) else { return Err(invalid("free layout requires x/y only, in content-relative DIP")); };
                    if !x.is_finite() || !y.is_finite() || !(0.0..=1_000_000.0).contains(x) || !(0.0..=1_000_000.0).contains(y) { return Err(invalid("coordinates must be finite and within 0..1000000 DIP")); }
                    next.desktop_item_mut(&identity).unwrap().set_pane_position(Some(luciddesk_core::PointDip::new(*x, *y)));
                } else {
                    let (Some(column), Some(row), None, None) = (column, row, x, y) else { return Err(invalid("grid layout requires column/row only")); };
                    if *column > 10000 || *row > 10000 { return Err(invalid("grid coordinates must be within 0..10000")); }
                    let position = GridPosition::new(*column, *row);
                    if next.desktop_items().iter().any(|i| i.identity() != &identity && i.placement() == &(DesktopPlacement::Pane { pane_id: target, position })) { return Err(invalid("target cell is occupied; other icons will not be moved")); }
                    next.desktop_item_mut(&identity).unwrap().set_placement(DesktopPlacement::Pane { pane_id: target, position });
                }
                membership = true;
            }
            Operation::Reorder { pane_id, item_ids } => {
                membership = true;
                let target = id(pane_id)?;
                editable(&next, target)?;
                let requested = if item_ids.is_empty() {
                    Vec::new()
                } else {
                    keys(&next, item_ids, ids)?
                };
                let existing = ordered(&next, target);
                if requested.iter().collect::<HashSet<_>>()
                    != existing.iter().collect::<HashSet<_>>()
                {
                    return Err(invalid(
                        "reorder requires every current member exactly once",
                    ));
                }
                order(&mut next, target, &requested);
            }
        }
    }
    refs.retain(|_, raw| id(raw).is_ok_and(|id| next.panel(id).is_some()));
    let mut before = summary(workspace);
    let mut after = summary(&next);
    // Never expose internal Shell identity encoding in the wire diff.
    for value in [&mut before, &mut after] {
        let map = value["placements"].as_object_mut().unwrap();
        let old = std::mem::take(map);
        for (key, value) in old {
            if let Some(token) = ids.get(&key) {
                map.insert(token.clone(), value);
            }
        }
    }
    if next.tab_groups() != workspace.tab_groups() {
        let saved: HashMap<_,_> = store.monitor_layout(&display_layout::key(monitors)).map_err(|e|invalid(e.to_string()))?.into_iter().collect();
        for group in next.tab_groups() {
            let physical = geometry.get(&group.active).or_else(||saved.get(&group.active)).copied().unwrap_or_else(|| {
                let r=next.panel(group.active).unwrap().rect();
                let scale=monitors.first().map_or(1.0,|m|m.dpi as f32/96.0);
                RectDip{x:r.x*scale,y:r.y*scale,width:r.width*scale,height:r.height*scale}
            });
            for id in &group.members {geometry.insert(*id,physical);}
        }
    }
    let mut diff = json!({"before":before,"after":after});
    folders.retain(|id,_| next.panel(*id).is_some());
    let folders_after: serde_json::Map<_,_> = folders.iter().map(|(id,p)|(id.get().to_string(),super::folders::preferences(p))).collect();
    let folders_changed = folders_before != folders_after;
    if !folders.is_empty() {diff["folder_preferences"]=json!({"before":folders_before,"after":folders_after});}

    let layout = if geometry.is_empty() { None } else {
        geometry.retain(|id,_| next.panel(*id).is_some());
        diff["geometry"] = json!(geometry.iter().map(|(id,r)|(id.get().to_string(),super::geometry::describe(*r,monitors))).collect::<std::collections::BTreeMap<_,_>>());
        let topology = display_layout::key(monitors);
        let mut positions: HashMap<_,_> = store.monitor_layout(&topology).map_err(|e|invalid(e.to_string()))?.into_iter().collect();
        positions.retain(|id,_|next.panel(*id).is_some());
        positions.extend(geometry);
        Some((topology,positions.into_iter().collect()))
    };
    Ok(Prepared {
        layout_observation: layout.as_ref().map(|_| observed.clone()),
        token: luciddesk_api::request_id(),
        base: plan.base.clone(),
        expires: Instant::now() + TTL,
        next,
        refs,
        diff,
        membership,
        applied: false,
        settings: None,
        settings_changed: false,
        layout,
        folders: folders.into_iter().collect(),
        folders_changed,
        runtime_action: None,
    })
}
fn receipt_pending(receipt:&Receipt)->bool {receipt.response.data.as_ref().is_some_and(|data|data["presentation_status"]=="pending")}
fn result_is_failed(response:&Response)->bool {response.data.as_ref().is_some_and(|data|data["presentation_status"]=="failed")}
impl Plans {
    pub(super) fn system_result(&mut self, id: &str, data: serde_json::Value) -> Option<Response> {
        let receipt = self.receipts.iter_mut().find(|r|r.id==id)?;
        let was_pending=receipt_pending(receipt);
        receipt.response.data = Some(data);
        if was_pending && !receipt_pending(receipt) {receipt.expires=Instant::now()+Duration::from_secs(600);}
        Some(receipt.response.clone())
    }

    pub(super) fn take_runtime_action(&mut self,response:&Response)->Option<Operation> {
        self.receipts.iter_mut().find(|r|r.id==response.request_id)?.runtime_action.take()
    }
    pub(super) fn refresh_desktop(&mut self, state: &PaneApp) {
        if !self.receipts.iter().any(|receipt|receipt.desktop_membership.is_some()) {return;}
        let desired=hybrid::desired_membership(state);
        let status=hybrid::membership_status(state);
        for receipt in &mut self.receipts {
            let Some(expected)=&receipt.desktop_membership else {continue;};
            let (result,error)=if *expected!=desired {
                ("superseded",Some("Desktop membership changed after this commit; inspect the current workspace"))
            } else if status=="disconnected" {
                ("failed",Some("Desktop component disconnected after commit; inspect state after reconnection"))
            } else if status=="applied" {("applied",None)} else {continue;};
            if let Some(data)=receipt.response.data.as_mut() {
                data["presentation_status"]=json!(result);
                if let Some(error)=error {data["presentation_error"]=json!(error);}
            }
            receipt.desktop_membership=None;
            receipt.expires=Instant::now()+Duration::from_secs(600);
        }
    }
    pub(super) fn presentation_result(&mut self, response: &mut Response, result: Result<(), String>) {
        let Some(data) = response.data.as_mut() else { return; };
        match result {
            Ok(()) => data["presentation_status"] = json!(if self.receipts.iter().any(|r|r.id==response.request_id && r.desktop_membership.is_some()) {"pending"} else {"applied"}),
            Err(error) => {
                data["presentation_status"] = json!("failed");
                data["presentation_error"] = json!(error);
            }
        }
        if let Some(receipt) = self.receipts.iter_mut().find(|r| r.id == response.request_id) {
            if result_is_failed(response) {receipt.desktop_membership=None;}
            receipt.response = response.clone();
        }
    }
    pub(super) fn handle(
        &mut self,
        state: &mut PaneApp,
        context: &Context,
        ids: &HashMap<String, String>,
        request: &Request,
        monitors: &[luciddesk_window::MonitorDescriptor],
    ) -> Response {
        let fail = |code: &str, msg: &str| Response::failure(&request.request_id, code, msg);
        let now = Instant::now();
        self.pending.retain(|p| p.expires > now);
        self.receipts.retain(|r| r.expires > now || receipt_pending(r));
        if request.command == "request.get" {
            return self.receipts.iter().find(|r|Some(&r.id)==request.id.as_ref()).map(|r|{
                Response::success(&request.request_id,json!(context),json!({"result":r.response}))
            }).unwrap_or_else(||fail("RESULT_UNKNOWN","request was not retained in this instance; inspect workspace before retrying"));
        }
        if request.command == "plan.preview" {
            let plan = request.plan.as_ref().unwrap();
            if plan.base != *context {
                self.metadata.remove(&request.request_id);
                return fail("CONFLICT", "workspace changed; query and preview again");
            }
            let metadata = match self.metadata.prepare(&state.workspace, plan, &request.request_id, ids) {
                Ok(metadata) => metadata,
                Err((code, message)) => {
                    let mut response = fail(code, &message);
                    if code == "SORT_METADATA_PENDING" {
                        response.data = Some(json!({"pending_preview":{"request_id":request.request_id,"plan":plan}}));
                    }
                    return response;
                }
            };
            let prepared = prepare(&state.workspace, &state.store, plan, ids, monitors, &super::content_layout::snapshots(state), &super::geometry::observe_layout(state), metadata);
            self.metadata.remove(&request.request_id);
            match prepared {
                Err((code, message)) => fail(&code, &message),
                Ok(plan) => {
                    let response = Response::success(
                        &request.request_id,
                        json!(context),
                        json!({"plan_token":plan.token,"expires_in_seconds":300,"changed":plan.runtime_action.is_some() || plan.settings_changed || plan.folders_changed || plan.next!=state.workspace,"diff":plan.diff,"provisional_refs":plan.refs}),
                    );
                    if serde_json::to_vec(&response)
                        .map_or(true, |bytes| bytes.len() > luciddesk_api::MAX_FRAME)
                    {
                        return fail("RESULT_TOO_LARGE", "preview exceeds response limit");
                    }
                    if self.pending.len() == 64 {
                        self.pending.pop_front();
                    }
                    self.pending.push_back(plan);
                    response
                }
            }
        } else {
            let signature = serde_json::to_string(request).unwrap();
            if let Some(receipt) = self.receipts.iter().find(|r| r.id == request.request_id) {
                return if receipt.signature == signature {
                    receipt.response.clone()
                } else {
                    fail("REQUEST_ID_REUSED", "request ID has different content")
                };
            }
            let Some(plan) = self
                .pending
                .iter_mut()
                .find(|p| Some(&p.token) == request.token.as_ref())
            else {
                return fail(
                    "PLAN_EXPIRED",
                    "plan expired or belongs to another instance",
                );
            };
            if plan.applied {
                return fail("PLAN_ALREADY_APPLIED", "plan has already been committed");
            }
            if plan.base != *context {
                return fail("CONFLICT", "workspace changed; preview again");
            }
            if plan.layout_observation.as_ref().is_some_and(|before| *before != super::geometry::observe_layout(state)) {
                return fail("CONFLICT", "visible panel bounds changed; preview again");
            }
            let changed = plan.runtime_action.is_some() || plan.settings_changed || plan.folders_changed || plan.next != state.workspace;
            if changed && plan.membership && !state.session.as_ref().is_some_and(hybrid::is_alive) {
                return fail(
                    "CAPABILITY_UNAVAILABLE",
                    "desktop integration must be connected for item operations",
                );
            }
            if self.receipts.len() >= 1024 && self.receipts.iter().all(receipt_pending) {
                return fail("BUSY", "all retained requests are still awaiting completion");
            }
            if changed && plan.runtime_action.is_none() {
                if let Some(updates) = &plan.settings {
                    if let Err(error) = super::settings::save(&state.store, updates) {
                        return fail("PERSISTENCE_ERROR", &error.to_string());
                    }
                } else {
                    if let Err(error) = state.store.save_workspace_with_folder_preferences(&plan.next, plan.layout.as_ref().map(|(key,entries)|(key.as_str(),entries.as_slice())), &plan.folders) {
                        return fail("PERSISTENCE_ERROR", &error.to_string());
                    }
                    state.workspace = plan.next.clone();
                    if let Some(runtime) = &mut state.runtime {
                        runtime.layouts.positions.retain(|id,_|state.workspace.panel(*id).is_some());
                    }
                    if let (Some(runtime), Some((_,entries))) = (&mut state.runtime, &plan.layout) {
                        runtime.layouts.positions.extend(entries.iter().copied());
                    }
                }
            }
            plan.applied = true;
            let response = Response::success(
                &request.request_id,
                json!(context),
                json!({"changed":changed,"commit_status":if plan.runtime_action.is_some(){"not_persisted"}else if changed{"committed"}else{"unchanged"},"presentation_status":if changed || plan.settings.is_some(){"pending"}else{"applied"},"refs":plan.refs,"scope":if plan.runtime_action.is_some(){"runtime"}else if plan.settings.is_some(){"settings"}else{"workspace"}}),
            );
            if self.receipts.len() >= 1024 {
                if let Some(index)=self.receipts.iter().position(|receipt|!receipt_pending(receipt)) {self.receipts.remove(index);}
            }
            self.receipts.push_back(Receipt {
                desktop_membership: (plan.membership && changed).then(||hybrid::desired_membership(state)),
                runtime_action:plan.runtime_action.clone(),
                id: request.request_id.clone(),
                signature,
                response: response.clone(),
                expires: now + Duration::from_secs(600),
            });
            response
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn create_preview_copies_layout_defaults_and_explicit_updates_override_them() {
        let mut state = super::super::super::tests::test_state();
        let mut snapshot = Snapshot::new();
        layout_defaults::Mode::Free.save(&state.store).unwrap();
        snapshot.respond(&mut state, &request("workspace.get"));
        let original = state.workspace.clone();
        let writes = state.store.change_count();
        let response = preview(&mut snapshot, &mut state, json!([
            {"op":"pane.create","ref":"new","title":"Free"}
        ]));
        assert!(response.ok, "{response:?}");
        assert!(snapshot.plans.pending.back().unwrap().next.panels().last().unwrap().free_layout());
        assert_eq!(state.workspace, original);
        assert_eq!(state.store.change_count(), writes);
        let next_id = (state.workspace.panels().iter().map(|p| p.id().get()).max().unwrap() + 1).to_string();
        let response = preview(&mut snapshot, &mut state, json!([
            {"op":"pane.create","ref":"new","title":"Compact"},
            {"op":"pane.update","pane_id":next_id,"auto_compact":true}
        ]));
        assert!(response.ok, "{response:?}");
        let panel = snapshot.plans.pending.back().unwrap().next.panels().last().unwrap();
        assert!(!panel.fixed_grid());
        assert!(!panel.free_layout());
    }
    #[test]
    fn arrangement_and_position_preview_preserve_others_and_reject_ambiguous_input() {
        let mut state = super::super::super::tests::test_state();
        let mut snapshot = Snapshot::new();
        snapshot.respond(&mut state, &request("workspace.get"));
        let item = state.workspace.desktop_items()[0].identity().clone();
        let token = snapshot.item_ids[&item.persistent_key()].clone();
        let original = state.workspace.clone();
        let writes = state.store.change_count();
        let p = preview(&mut snapshot, &mut state, json!([
            {"op":"pane.update","pane_id":"1","auto_compact":false},
            {"op":"item.position","pane_id":"1","item_id":token,"column":8,"row":7}
        ]));
        assert!(p.ok, "{p:?}");
        assert_eq!(state.workspace, original);
        assert_eq!(state.store.change_count(), writes);
        let next = snapshot.plans.pending.back().unwrap().next.clone();
        assert_eq!(next.desktop_item(&item).unwrap().placement(), &DesktopPlacement::Pane { pane_id: PanelId::new(1), position: GridPosition::new(8, 7) });
        state.workspace = next;
        state.store.save_workspace(&state.workspace).unwrap();
        let same = preview(&mut snapshot, &mut state, json!([{"op":"item.position","pane_id":"1","item_id":token,"column":8,"row":7}]));
        assert_eq!(same.data.as_ref().unwrap()["changed"], false);
        let writes = state.store.change_count();
        let commit = apply(&same);
        assert!(snapshot.respond(&mut state, &commit).ok);
        assert!(snapshot.respond(&mut state, &commit).ok);
        assert_eq!(state.store.change_count(), writes);
        let free = preview(&mut snapshot, &mut state, json!([
            {"op":"pane.update","pane_id":"1","align_icons_to_grid":false},
            {"op":"item.position","pane_id":"1","item_id":token,"x":350.5,"y":90.25}
        ]));
        assert!(free.ok, "{free:?}");
        let next = &snapshot.plans.pending.back().unwrap().next;
        assert_eq!(next.desktop_item(&item).unwrap().pane_position(), Some(luciddesk_core::PointDip::new(350.5,90.25)));
        for invalid in [
            json!({"op":"pane.update","pane_id":"1","auto_compact":true,"align_icons_to_grid":false}),
            json!({"op":"item.position","pane_id":"1","item_id":token,"x":1,"y":2}),
            json!({"op":"item.position","pane_id":"1","item_id":token,"column":1,"row":0}),
            json!({"op":"item.position","pane_id":"2","item_id":token,"column":1,"row":0}),
            json!({"op":"item.position","pane_id":"1","item_id":token,"column":10001,"row":0}),
        ] { assert!(!preview(&mut snapshot, &mut state, json!([invalid])).ok); }
    }

    #[test]
    fn metadata_sort_preview_uses_file_properties_without_writing() {
        let _sta = crate::pane::test_support::apartment();
        let root = tempfile::tempdir().unwrap();
        let mut state = super::super::super::tests::test_state();
        let mut items = Vec::new();
        for (index, (name, size)) in [("Large.lnk",100), ("Small.txt",2), ("Medium.txt",10)].into_iter().enumerate() {
            let path = root.path().join(name);
            std::fs::write(&path, vec![0; size]).unwrap();
            let mut item = luciddesk_core::DesktopItem::new(ShellIdentity::FileSystem {path, volume_id:None, file_id:None}, name);
            item.set_placement(DesktopPlacement::Pane { pane_id:PanelId::new(1), position:GridPosition::new(index as u32,0) });
            items.push(item);
        }
        state.workspace.reconcile_desktop_items(items);
        let mut snapshot = Snapshot::new();
        let base = snapshot.respond(&mut state, &request("workspace.get")).context.unwrap();
        let mut req = request("plan.preview");
        req.plan = Some(serde_json::from_value(json!({"protocol_version":1,"base":base,"operations":[{"op":"pane.sort","pane_id":"1","sort_column":"size"}]})).unwrap());
        let writes = state.store.change_count();
        let original = state.workspace.clone();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let response = snapshot.respond(&mut state, &req);
            if response.ok { break; }
            assert_eq!(response.error.unwrap().code, "SORT_METADATA_PENDING");
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let next = &snapshot.plans.pending.back().unwrap().next;
        assert_eq!(ordered_desktop_items(next, PanelId::new(1)).iter().map(|i|i.display_name()).collect::<Vec<_>>(), ["Small.txt","Medium.txt","Large.lnk"]);
        assert_eq!(state.workspace, original);
        assert_eq!(state.store.change_count(), writes);
    }

    #[test]
    fn all_metadata_columns_have_explicit_stable_directions() {
        let mut state = super::super::super::tests::test_state();
        let mut snapshot = Snapshot::new();
        let base = snapshot.respond(&mut state, &request("workspace.get")).context.unwrap();
        let metadata: HashMap<_, _> = state.workspace.desktop_items().iter().map(|item| {
            let rank = match item.display_name() { "A" => 3, "B" => 1, _ => 2 };
            (item.identity().persistent_key(), ItemDetails { kind: rank.to_string(), size: Some(rank),
                modified_time: Some(std::time::UNIX_EPOCH + Duration::from_secs(rank)), ..Default::default() })
        }).collect();
        for column in ["type", "size", "modified"] {
            for descending in [false, true] {
                let plan = serde_json::from_value(json!({"protocol_version":1,"base":base,
                    "operations":[{"op":"pane.sort","pane_id":"1","sort_column":column,"descending":descending}]})).unwrap();
                let prepared = prepare(&state.workspace, &state.store, &plan, &snapshot.item_ids,
                    &luciddesk_window::enumerate_monitors(), &super::content_layout::snapshots(&state),
                    &super::geometry::observe_layout(&state), Some(&metadata)).unwrap();
                let actual: Vec<_> = ordered_desktop_items(&prepared.next, PanelId::new(1)).iter().map(|i| i.display_name().to_owned()).collect();
                assert_eq!(actual, if descending { ["A","C","B"] } else { ["B","C","A"] }, "{column}, descending={descending}");
            }
        }
    }
    use super::*;
    fn request(command: &str) -> Request {
        serde_json::from_value(
            json!({"protocol_version":1,"request_id":luciddesk_api::request_id(),"command":command}),
        )
        .unwrap()
    }
    fn preview(
        snapshot: &mut Snapshot,
        state: &mut PaneApp,
        operations: serde_json::Value,
    ) -> Response {
        let context = snapshot
            .respond(state, &request("workspace.get"))
            .context
            .unwrap();
        let mut req = request("plan.preview");
        req.plan = Some(
            serde_json::from_value(
                json!({"protocol_version":1,"base":context,"operations":operations}),
            )
            .unwrap(),
        );
        snapshot.respond(state, &req)
    }
    fn apply(response: &Response) -> Request {
        assert!(response.ok, "{response:?}");
        let mut req = request("plan.apply");
        req.token = Some(
            response.data.as_ref().unwrap()["plan_token"]
                .as_str()
                .unwrap()
                .into(),
        );
        req
    }
    #[test]
    fn folder_refresh_is_transient_and_invalidates_prepared_fit() {
        let _sta=crate::pane::test_support::apartment();
        let root=tempfile::tempdir().unwrap();std::fs::write(root.path().join("one.txt"),"fixture").unwrap();
        let mut app=super::super::super::tests::test_state();
        app.workspace.panel_mut(PanelId::new(1)).unwrap().set_folder(Some(root.path().into()));
        app.store.save_workspace(&app.workspace).unwrap();
        let state=Rc::new(RefCell::new(app));create_view(&state,PanelId::new(1)).unwrap();
        let deadline=Instant::now()+Duration::from_secs(5);
        while state.borrow().folders[&PanelId::new(1)].loading {
            assert!(Instant::now()<deadline);
            std::thread::sleep(Duration::from_millis(10));folder::poll(&mut state.borrow_mut());
        }
        let mut snapshot=Snapshot::new();
        let p=preview(&mut snapshot,&mut state.borrow_mut(),json!([{"op":"folder.fit","pane_id":"1","max_rows":4}]));
        assert!(p.ok,"{p:?}");
        let count=state.borrow().store.change_count();
        let refreshed=super::super::transient::execute(&state,Operation::FolderRefresh{pane_id:"1".into()}).unwrap();
        assert_eq!(refreshed["loading"],true);
        assert_eq!(snapshot.respond(&mut state.borrow_mut(),&apply(&p)).error.unwrap().code,"CONFLICT");
        assert_eq!(state.borrow().store.change_count(),count);
    }

    #[test]
    fn content_layout_preview_commits_once_and_repeated_fit_is_noop() {
        let mut state=super::super::super::tests::test_state();
        state.store.save_workspace(&state.workspace).unwrap();
        let mut snapshot=Snapshot::new();
        let count=state.store.change_count();
        let op=json!({"op":"pane.fit","pane_id":"1","icon_columns":1});
        let p=preview(&mut snapshot,&mut state,json!([op.clone()]));
        assert!(p.ok,"{p:?}");
        assert_eq!(state.store.change_count(),count);
        assert!(p.data.as_ref().unwrap()["diff"]["geometry"]["1"].is_object());
        assert!(snapshot.respond(&mut state,&apply(&p)).ok);
        assert_eq!(state.workspace.panel(PanelId::new(1)).unwrap().rect().width, 112.0);
        assert_eq!(state.store.load_workspace().unwrap().panel(PanelId::new(1)).unwrap().rect(), state.workspace.panel(PanelId::new(1)).unwrap().rect());
        let committed=state.store.change_count();assert!(committed>count);
        let repeated=preview(&mut snapshot,&mut state,json!([op.clone()]));
        assert_eq!(repeated.data.as_ref().unwrap()["changed"],false);
        assert!(snapshot.respond(&mut state,&apply(&repeated)).ok);
        assert_eq!(state.store.change_count(),committed);
        state.workspace.panel_mut(PanelId::new(1)).unwrap().set_locked(true);
        assert_eq!(preview(&mut snapshot,&mut state,json!([op])).error.unwrap().code,"PANE_LOCKED");
    }

    #[test]
    fn snap_rejects_auto_hidden_windows_and_stale_native_preview() {
        let _sta = crate::pane::test_support::apartment();
        let state = Rc::new(RefCell::new(super::super::super::tests::test_state()));
        create_view(&state, PanelId::new(1)).unwrap();
        let model = state.borrow().views[0].model.clone();
        let mut snapshot = Snapshot::new();
        let geometry = json!({"op":"pane.fit","pane_id":"1","icon_columns":4});
        let p = preview(&mut snapshot, &mut state.borrow_mut(), json!([geometry]));
        assert!(p.ok, "{p:?}");
        let before = state.borrow().store.change_count();
        model.borrow_mut().collapsed = true;
        let response = snapshot.respond(&mut state.borrow_mut(), &apply(&p));
        assert_eq!(response.error.unwrap().code, "CONFLICT");
        assert_eq!(state.borrow().store.change_count(), before);
        let p = preview(&mut snapshot, &mut state.borrow_mut(), json!([{"op":"pane.snap","pane_id":"2","target_pane_id":"1","side":"right","align":"start"}]));
        assert!(!p.ok);
        assert!(p.error.unwrap().message.contains("reveal both panels"));
    }

    #[test]
    fn auto_hide_interactions_preserve_preview_context_and_do_not_write() {
        let _sta = crate::pane::test_support::apartment();
        let state = Rc::new(RefCell::new(super::super::super::tests::test_state()));
        let id = PanelId::new(1);
        create_view(&state, id).unwrap();
        handle(&state, id, Event::ToggleAutoHide).unwrap();
        let mut snapshot = Snapshot::new();
        let p = preview(&mut snapshot, &mut state.borrow_mut(), json!([
            {"op":"pane.update","pane_id":"1","title":"after hovering"}
        ]));
        let count = state.borrow().store.change_count();
        let context = snapshot.respond(&mut state.borrow_mut(), &request("status")).context;
        for collapsed in [true, false, true, false] {
            handle(&state, id, Event::AutoHideCollapsed(collapsed)).unwrap();
            assert_eq!(state.borrow().views[0].model.borrow().collapsed, collapsed);
            assert_eq!(snapshot.respond(&mut state.borrow_mut(), &request("status")).context, context);
            assert_eq!(state.borrow().store.change_count(), count);
        }
        assert!(snapshot.respond(&mut state.borrow_mut(), &apply(&p)).ok);
        assert_eq!(state.borrow().workspace.panel(id).unwrap().title(), "after hovering");
    }

    #[test]
    fn failed_storage_transaction_preserves_live_state_and_allows_exact_retry() {
        let mut state = super::super::super::tests::test_state();
        state.store.save_workspace(&state.workspace).unwrap();
        let original = state.workspace.clone();
        let count = state.store.change_count();
        let mut snapshot = Snapshot::new();
        let p = preview(&mut snapshot, &mut state, json!([
            {"op":"pane.update","pane_id":"1","title":"committed only after recovery"}
        ]));
        let req = apply(&p);
        // Exercise the transaction coordinator with a fixed observed context;
        // topology changes are covered separately, not part of this fault case.
        let base = snapshot.plans.pending.back().unwrap().base.clone();
        let monitors = luciddesk_window::enumerate_monitors();
        // Inject a foreign-key failure after workspace rows have been updated inside
        // the real SQLite transaction. No production fault-injection API is needed.
        snapshot.plans.pending.back_mut().unwrap().layout = Some((
            "fault-test".into(), vec![(PanelId::new(999), RectDip::default())],
        ));
        let failed = snapshot.plans.handle(&mut state, &base, &snapshot.item_ids, &req, &monitors);
        assert_eq!(failed.error.unwrap().code, "PERSISTENCE_ERROR");
        assert_eq!(state.workspace, original);
        assert_eq!(state.store.load_workspace().unwrap(), original);
        assert_eq!(state.store.change_count(), count);
        assert!(snapshot.plans.receipts.is_empty());
        assert!(!snapshot.plans.pending.back().unwrap().applied);
        snapshot.plans.pending.back_mut().unwrap().layout = None;
        let recovered = snapshot.plans.handle(&mut state, &base, &snapshot.item_ids, &req, &monitors);
        assert!(recovered.ok, "{recovered:?}");
        assert_eq!(state.store.load_workspace().unwrap(), state.workspace);
        let committed = state.store.change_count();
        assert!(committed > count);
        assert!(snapshot.plans.handle(&mut state, &base, &snapshot.item_ids, &req, &monitors).ok);
        assert_eq!(state.store.change_count(), committed);
    }

    #[test]
    fn restored_database_and_changed_inventory_invalidate_old_plans() {
        let dir = tempfile::tempdir().unwrap();
        let backup = dir.path().join("restore-test.backup");
        let mut state = super::super::super::tests::test_state();
        state.store.save_workspace(&state.workspace).unwrap();
        state.store.export_backup(&backup).unwrap();
        let mut snapshot = Snapshot::new();
        let p = preview(&mut snapshot, &mut state, json!([
            {"op":"pane.update","pane_id":"1","title":"stale before restore"}
        ]));
        state.store.restore_backup(&backup).unwrap();
        state.workspace = state.store.load_workspace().unwrap();
        let restored = state.store.change_count();
        assert_eq!(snapshot.respond(&mut state, &apply(&p)).error.unwrap().code, "CONFLICT");
        assert_eq!(state.store.change_count(), restored);
        let p = preview(&mut snapshot, &mut state, json!([
            {"op":"pane.update","pane_id":"1","title":"stale before inventory change"}
        ]));
        let remaining = state.workspace.desktop_items()[1..].to_vec();
        state.workspace.reconcile_desktop_items(remaining);
        assert_eq!(snapshot.respond(&mut state, &apply(&p)).error.unwrap().code, "CONFLICT");
        assert_eq!(state.store.change_count(), restored);
    }

    #[test]
    fn pending_receipts_survive_expiry_and_saturation_prevents_writes() {
        let mut state=super::super::super::tests::test_state();
        let mut snapshot=Snapshot::new();
        for index in 0..1024 {
            let id=format!("pending-{index}");
            snapshot.plans.receipts.push_back(Receipt {
                desktop_membership:None,runtime_action:None,signature:String::new(),
                response:Response::success(&id,json!({}),json!({"presentation_status":"pending"})),
                id,expires:Instant::now()-Duration::from_secs(1),
            });
        }
        let mut lookup=request("request.get");lookup.id=Some("pending-0".into());
        assert!(snapshot.respond(&mut state,&lookup).ok);
        assert_eq!(snapshot.plans.receipts.len(),1024);
        let plan=preview(&mut snapshot,&mut state,json!([{"op":"pane.update","pane_id":"1","title":"blocked at capacity"}]));
        let revision=state.store.change_count();
        let blocked=snapshot.respond(&mut state,&apply(&plan));
        assert_eq!(blocked.error.unwrap().code,"BUSY");
        assert_eq!(state.store.change_count(),revision);
        assert_ne!(state.workspace.panel(PanelId::new(1)).unwrap().title(),"blocked at capacity");
        snapshot.plans.receipts[0].response.data.as_mut().unwrap()["presentation_status"]=json!("applied");
        assert!(snapshot.respond(&mut state,&apply(&plan)).ok);
        assert_eq!(snapshot.plans.receipts.len(),1024);
        assert!(!snapshot.plans.receipts.iter().any(|r|r.id=="pending-0"));
    }
    #[test]
    fn desktop_confirmation_reports_disconnect_or_superseded_without_undoing_commit() {
        let state=super::super::super::tests::test_state();
        let expected=hybrid::desired_membership(&state);
        for (membership,outcome) in [(expected,"failed"),(vec!["different desired membership".into()],"superseded")] {
            let mut plans=Plans::default();
            plans.receipts.push_back(Receipt {
                desktop_membership:Some(membership),runtime_action:None,id:"receipt".into(),signature:String::new(),
                response:Response::success("receipt",json!({}),json!({"commit_status":"committed","presentation_status":"pending"})),
                expires:Instant::now()+TTL,
            });
            plans.refresh_desktop(&state);
            let receipt=&plans.receipts[0];
            assert_eq!(receipt.response.data.as_ref().unwrap()["commit_status"],"committed");
            assert_eq!(receipt.response.data.as_ref().unwrap()["presentation_status"],outcome);
            assert!(receipt.desktop_membership.is_none());
        }
    }
    #[test]
    fn startup_plans_retain_async_receipts_without_reexecuting() {
        let mut state=super::super::super::tests::test_state();
        let mut snapshot=Snapshot::new();
        let before=state.store.change_count();
        let invalid=preview(&mut snapshot,&mut state,json!([{"op":"startup.set","enabled":true,"expected_status":"other_location"}]));
        assert!(!invalid.ok);
        let mixed=preview(&mut snapshot,&mut state,json!([{"op":"startup.set","enabled":true,"expected_status":"off"},{"op":"pane.create","ref":"x","title":"no"}]));
        assert!(!mixed.ok);
        let plan=preview(&mut snapshot,&mut state,json!([{"op":"startup.set","enabled":true,"expected_status":"off"}]));
        let request=apply(&plan);
        let response=snapshot.respond(&mut state,&request);
        assert!(matches!(snapshot.plans.take_runtime_action(&response),Some(Operation::StartupSet{enabled:true,..})));
        assert!(snapshot.plans.take_runtime_action(&response).is_none());
        let completed=json!({"scope":"system","operation_status":"completed","commit_status":"committed"});
        snapshot.plans.system_result(&request.request_id,completed.clone()).unwrap();
        let repeated=snapshot.respond(&mut state,&request);
        assert_eq!(repeated.data,Some(completed));
        assert!(snapshot.plans.take_runtime_action(&repeated).is_none());
        assert_eq!(state.store.change_count(),before);
    }
    #[test]
    fn folder_plan_preview_is_pure_updates_are_atomic_and_removal_keeps_files() {
        let root=tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("keep.txt"),"keep").unwrap();
        let mut state=super::super::super::tests::test_state();
        state.store.save_workspace(&state.workspace).unwrap();
        let mut snapshot=Snapshot::new();
        let count=state.store.change_count();
        let plan=preview(&mut snapshot,&mut state,json!([{"op":"folder.create","ref":"mapped","title":"Files","path":root.path()}]));
        assert!(plan.ok,"{plan:?}");assert_eq!(state.store.change_count(),count);
        let applied=snapshot.respond(&mut state,&apply(&plan));assert!(applied.ok,"{applied:?}");
        let pane=applied.data.unwrap()["refs"]["mapped"].as_str().unwrap().to_owned();
        let op=json!({"op":"folder.update","pane_id":pane,"sort_column":"modified","descending":true,"list_view":false,"visible_columns":["name","modified"],"column_widths":[0.4,0.2,0.2,0.2]});
        let plan=preview(&mut snapshot,&mut state,json!([op.clone()]));
        assert!(snapshot.respond(&mut state,&apply(&plan)).ok);
        let count=state.store.change_count();
        let noop=preview(&mut snapshot,&mut state,json!([op]));
        assert!(!noop.data.as_ref().unwrap()["changed"].as_bool().unwrap());
        assert!(snapshot.respond(&mut state,&apply(&noop)).ok);assert_eq!(state.store.change_count(),count);
        let bad=preview(&mut snapshot,&mut state,json!([{"op":"pane.update","pane_id":pane,"title":"must rollback"},{"op":"folder.update","pane_id":pane,"visible_columns":["size"]}]));
        assert!(!bad.ok);assert_eq!(state.workspace.panel(id(&pane).unwrap()).unwrap().title(),"Files");
        let mut query=request("folder.get");query.id=Some(pane.clone());
        let result=snapshot.respond(&mut state,&query);assert!(result.ok);
        assert_eq!(result.data.unwrap()["preferences"]["sort_column"],"modified");
        let removed=preview(&mut snapshot,&mut state,json!([{"op":"pane.remove","pane_id":pane}]));
        assert!(snapshot.respond(&mut state,&apply(&removed)).ok);
        assert_eq!(std::fs::read_to_string(root.path().join("keep.txt")).unwrap(),"keep");
    }

    #[test]
    fn geometry_plan_persists_layout_atomically_and_rejects_locked_or_unknown_monitors() {
        let mut state = super::super::super::tests::test_state();
        state.store.save_workspace(&state.workspace).unwrap();
        let mut snapshot = Snapshot::new();
        let monitors = luciddesk_window::enumerate_monitors();
        let monitor = monitors.first().expect("test desktop monitor");
        let op = json!({"op":"pane.geometry","pane_id":"1","monitor_id":monitor.id.as_str(),"x":30,"y":40,"width":400,"height":240});
        let count = state.store.change_count();
        let plan = preview(&mut snapshot,&mut state,json!([op.clone()]));
        assert!(plan.ok, "{plan:?}");
        assert_eq!(state.store.change_count(),count);
        let response=snapshot.respond(&mut state,&apply(&plan));
        assert!(response.ok, "{response:?}");
        let layout=state.store.monitor_layout(&display_layout::key(&monitors)).unwrap();
        let px=layout.iter().find(|(id,_)|*id==PanelId::new(1)).unwrap().1;
        let scale=monitor.dpi as f32/96.0;
        assert_eq!(px.x,monitor.work_area.x as f32+(30.0*scale).round());
        assert_eq!(state.store.load_workspace().unwrap().panel(PanelId::new(1)).unwrap().rect(),state.workspace.panel(PanelId::new(1)).unwrap().rect());
        let count=state.store.change_count();
        let noop=preview(&mut snapshot,&mut state,json!([op.clone()]));
        assert!(snapshot.respond(&mut state,&apply(&noop)).ok);
        assert_eq!(state.store.change_count(),count);
        let mut invalid=op.clone();invalid["monitor_id"]=json!("absent");
        assert!(!preview(&mut snapshot,&mut state,json!([invalid])).ok);
        state.workspace.panel_mut(PanelId::new(1)).unwrap().set_locked(true);
        let blocked=preview(&mut snapshot,&mut state,json!([op]));
        assert_eq!(blocked.error.unwrap().code,"PANE_LOCKED");
    }

    #[test]
    fn settings_plan_validates_before_writing_and_retains_presentation_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = super::super::super::tests::test_state();
        state.store = WorkspaceStore::open(&dir.path().join("workspace.db")).unwrap();
        let mut snapshot = Snapshot::new();
        let count = state.store.change_count();
        let invalid = preview(&mut snapshot, &mut state, json!([
            {"op":"settings.update","values":{"diagnostics.level":"trace","panel_defaults.grid_scale":0}}
        ]));
        assert!(!invalid.ok);
        let mixed = preview(&mut snapshot, &mut state, json!([
            {"op":"settings.update","values":{"diagnostics.level":"debug"}},
            {"op":"pane.create","ref":"x","title":"x"}
        ]));
        assert!(!mixed.ok);
        let plan = preview(&mut snapshot, &mut state, json!([
            {"op":"settings.update","values":{"diagnostics.level":"debug","panel_defaults.grid_scale":125}}
        ]));
        assert!(plan.ok);
        assert_eq!(state.store.change_count(), count);
        let req = apply(&plan);
        let mut response = snapshot.respond(&mut state, &req);
        assert!(response.ok);
        assert_eq!(state.store.change_count(), count + 1);
        assert_eq!(response.data.as_ref().unwrap()["presentation_status"], "pending");
        snapshot.plans.presentation_result(&mut response, Err("injected unavailable shortcut".into()));
        let retry = snapshot.respond(&mut state, &req);
        assert_eq!(serde_json::to_value(retry).unwrap(), serde_json::to_value(response).unwrap());
        assert_eq!(state.store.change_count(), count + 1);
        let noop = preview(&mut snapshot, &mut state, json!([
            {"op":"settings.update","values":{"diagnostics.level":"debug"}}
        ]));
        assert_eq!(noop.data.as_ref().unwrap()["changed"], false);
        let response = snapshot.respond(&mut state, &apply(&noop));
        assert!(response.ok);
        assert_eq!(response.data.unwrap()["commit_status"], "unchanged");
        assert_eq!(state.store.change_count(), count + 1);
    }


    #[test]
    fn cli_reorder_uses_viewport_columns_for_scrolled_manual_grid() {
        let mut state = super::super::super::tests::test_state();
        let pane = PanelId::new(1);
        let g = free_layout::grid(&state.workspace);
        let mut rect = state.workspace.panel(pane).unwrap().rect();
        rect.width = layout::PADDING * 2.0 + 2.0 * g.cell_width;
        state.workspace.panel_mut(pane).unwrap().set_rect(rect);
        state.workspace.panel_mut(pane).unwrap().set_fixed_grid(true);
        for (index, item) in state.workspace.desktop_items_mut().iter_mut().enumerate() {
            item.set_placement(DesktopPlacement::Pane { pane_id: pane, position: GridPosition::new(index as u32 * 4, 0) });
        }
        let keys = ordered(&state.workspace, pane);
        order(&mut state.workspace, pane, &keys);
        for (index, item) in ordered_desktop_items(&state.workspace, pane).iter().enumerate() {
            assert_eq!(item.placement(), &DesktopPlacement::Pane { pane_id: pane, position: fixed_grid::position(index, 2) });
        }
    }

    #[test]
    fn ordinary_panel_sort_is_natural_scoped_and_noop_aware() {
        let mut state = super::super::super::tests::test_state();
        for (index, item) in state.workspace.desktop_items_mut().iter_mut().enumerate() {
            item.set_display_name(["文件10", "文件2", "Other"][index]);
            item.set_placement(DesktopPlacement::Pane { pane_id: PanelId::new(if index == 2 { 2 } else { 1 }), position: GridPosition::new(index as u32, 0) });
        }
        let original = state.workspace.clone();
        let mut snapshot = Snapshot::new();
        let p = preview(&mut snapshot, &mut state, json!([{"op":"pane.sort","pane_id":"1"}]));
        assert!(p.ok, "{p:?}");
        assert_eq!(state.workspace, original);
        let sorted = snapshot.plans.pending.back().unwrap().next.clone();
        assert_eq!(ordered(&sorted, PanelId::new(1)), ordered(&original, PanelId::new(1)).into_iter().rev().collect::<Vec<_>>());
        assert_eq!(ordered(&sorted, PanelId::new(2)), ordered(&original, PanelId::new(2)));
        state.workspace = sorted;
        state.store.save_workspace(&state.workspace).unwrap();
        let count = state.store.change_count();
        let same = preview(&mut snapshot, &mut state, json!([{"op":"pane.sort","pane_id":"1"}]));
        assert_eq!(same.data.as_ref().unwrap()["changed"], false);
        assert!(snapshot.respond(&mut state, &apply(&same)).ok);
        assert_eq!(state.store.change_count(), count);
        let reversed = preview(&mut snapshot, &mut state, json!([{"op":"pane.sort","pane_id":"1","descending":true}]));
        assert!(reversed.ok);
        assert_eq!(ordered(&snapshot.plans.pending.back().unwrap().next, PanelId::new(1)), ordered(&original, PanelId::new(1)));
        state.workspace.panel_mut(PanelId::new(1)).unwrap().set_locked(true);
        assert_eq!(preview(&mut snapshot, &mut state, json!([{"op":"pane.sort","pane_id":"1"}])).error.unwrap().code, "PANE_LOCKED");
        state.workspace.panel_mut(PanelId::new(1)).unwrap().set_locked(false);
        state.workspace.panel_mut(PanelId::new(1)).unwrap().set_folder(Some(std::env::temp_dir()));
        assert!(!preview(&mut snapshot, &mut state, json!([{"op":"pane.sort","pane_id":"1"}])).ok);
    }
    #[test]
    fn preview_is_pure_commit_is_idempotent_and_receipt_is_queryable() {
        let mut state = super::super::super::tests::test_state();
        state.store.save_workspace(&state.workspace).unwrap();
        let before = state.workspace.clone();
        let count = state.store.change_count();
        let mut snapshot = Snapshot::new();
        let p = preview(
            &mut snapshot,
            &mut state,
            json!([{"op":"pane.create","ref":"new","title":"整理"},{"op":"pane.update","pane_id":"1","title":"文档"}]),
        );
        assert_eq!(state.workspace, before);
        assert_eq!(state.store.change_count(), count);
        let mut req = apply(&p);
        let response = snapshot.respond(&mut state, &req);
        assert!(response.ok, "{response:?}");
        assert_eq!(state.workspace.panels().len(), 3);
        assert_eq!(
            state.workspace.panel(PanelId::new(1)).unwrap().title(),
            "文档"
        );
        let committed = state.store.change_count();
        assert!(committed > count);
        let retry = snapshot.respond(&mut state, &req);
        assert_eq!(
            serde_json::to_value(&retry).unwrap(),
            serde_json::to_value(&response).unwrap()
        );
        assert_eq!(state.store.change_count(), committed);
        let mut query = request("request.get");
        query.id = Some(req.request_id.clone());
        assert!(snapshot.respond(&mut state, &query).ok);
        req.token = Some("different".into());
        assert_eq!(
            snapshot.respond(&mut state, &req).error.unwrap().code,
            "REQUEST_ID_REUSED"
        );
        assert_eq!(state.store.change_count(), committed);
    }
    #[test]
    fn expired_plan_and_evicted_receipt_are_reported_without_writes() {
        let mut state = super::super::super::tests::test_state();
        let mut snapshot = Snapshot::new();
        let p = preview(
            &mut snapshot,
            &mut state,
            json!([{"op":"pane.create","ref":"new","title":"new"}]),
        );
        let req = apply(&p);
        let count = state.store.change_count();
        snapshot.plans.pending[0].expires = Instant::now() - Duration::from_secs(1);
        assert_eq!(
            snapshot.respond(&mut state, &req).error.unwrap().code,
            "PLAN_EXPIRED"
        );
        let mut query = request("request.get");
        query.id = Some(req.request_id);
        assert_eq!(
            snapshot.respond(&mut state, &query).error.unwrap().code,
            "RESULT_UNKNOWN"
        );
        assert_eq!(state.store.change_count(), count);
    }
    #[test]
    fn remove_requires_explicit_release_and_normalizes_only_affected_members() {
        let mut state = super::super::super::tests::test_state();
        let mut snapshot = Snapshot::new();
        for (index, item) in state.workspace.desktop_items_mut().iter_mut().enumerate() {
            item.set_placement(DesktopPlacement::Pane {
                pane_id: PanelId::new(1),
                position: GridPosition::new(index as u32, 0),
            });
        }
        let before = state.workspace.clone();
        let count = state.store.change_count();
        let denied = preview(
            &mut snapshot,
            &mut state,
            json!([{"op":"pane.remove","pane_id":"1"}]),
        );
        assert!(!denied.ok);
        let prepared = preview(
            &mut snapshot,
            &mut state,
            json!([{"op":"pane.remove","pane_id":"1","release_items":true}]),
        );
        assert!(prepared.ok);
        let next = &snapshot.plans.pending.back().unwrap().next;
        assert!(next.panel(PanelId::new(1)).is_none());
        assert!(
            next.desktop_items()
                .iter()
                .all(|i| matches!(i.placement(), DesktopPlacement::FreeDesktop { .. }))
        );
        assert_eq!(state.workspace, before);
        assert_eq!(state.store.change_count(), count);
        assert_eq!(
            snapshot
                .respond(&mut state, &apply(&prepared))
                .error
                .unwrap()
                .code,
            "CAPABILITY_UNAVAILABLE"
        );
    }
    #[test]
    fn options_require_explicit_unlock_and_preview_lists_shared_changes() {
        let mut state = super::super::super::tests::test_state();
        let mut snapshot = Snapshot::new();
        state
            .workspace
            .set_tab_groups(vec![luciddesk_core::PaneTabs {
                members: vec![PanelId::new(1), PanelId::new(2)],
                active: PanelId::new(1),
            }])
            .unwrap();
        let p = preview(
            &mut snapshot,
            &mut state,
            json!([{"op":"pane.update","pane_id":"2","locked":true,"auto_hide":true,"collapsed":true,"always_on_top":true}]),
        );
        assert!(snapshot.respond(&mut state, &apply(&p)).ok);
        for panel in state.workspace.panels() {
            assert!(
                panel.locked() && panel.auto_hide() && panel.collapsed() && panel.always_on_top()
            );
        }
        let denied = preview(
            &mut snapshot,
            &mut state,
            json!([{"op":"pane.update","pane_id":"1","title":"blocked"}]),
        );
        assert_eq!(denied.error.unwrap().code, "PANE_LOCKED");
        let p = preview(
            &mut snapshot,
            &mut state,
            json!([{"op":"pane.update","pane_id":"2","locked":false,"title":"unlocked"}]),
        );
        assert!(snapshot.respond(&mut state, &apply(&p)).ok);
        assert!(state.workspace.panels().iter().all(|p| !p.locked()));
    }
    #[test]
    fn provisional_ids_are_not_reused_within_a_plan() {
        let mut state = super::super::super::tests::test_state();
        let mut snapshot = Snapshot::new();
        let p = preview(
            &mut snapshot,
            &mut state,
            json!([{"op":"pane.create","ref":"discarded","title":"discarded"},{"op":"pane.remove","pane_id":"3"},{"op":"pane.create","ref":"kept","title":"kept"}]),
        );
        assert!(p.ok);
        let refs = &p.data.unwrap()["provisional_refs"];
        assert!(refs.get("discarded").is_none());
        assert_eq!(refs["kept"], "4");
    }
    #[test]
    fn stale_plan_and_invalid_batch_never_write() {
        let mut state = super::super::super::tests::test_state();
        let mut snapshot = Snapshot::new();
        let count = state.store.change_count();
        let original = state.workspace.clone();
        let invalid = preview(
            &mut snapshot,
            &mut state,
            json!([{"op":"pane.create","ref":"new","title":"test"},{"op":"pane.update","pane_id":"999","title":"missing"}]),
        );
        assert!(!invalid.ok);
        assert_eq!(state.workspace, original);
        let p = preview(
            &mut snapshot,
            &mut state,
            json!([{"op":"pane.update","pane_id":"1","title":"CLI"}]),
        );
        state
            .workspace
            .panel_mut(PanelId::new(1))
            .unwrap()
            .set_title("GUI");
        assert_eq!(
            snapshot.respond(&mut state, &apply(&p)).error.unwrap().code,
            "CONFLICT"
        );
        assert_eq!(state.store.change_count(), count);
    }
    #[test]
    fn no_op_and_disconnected_membership_do_not_write() {
        let mut state = super::super::super::tests::test_state();
        let mut snapshot = Snapshot::new();
        let count = state.store.change_count();
        let p = preview(
            &mut snapshot,
            &mut state,
            json!([{"op":"pane.update","pane_id":"1","title":"Group 1"}]),
        );
        assert_eq!(
            snapshot.respond(&mut state, &apply(&p)).data.unwrap()["changed"],
            false
        );
        let items = snapshot
            .respond(&mut state, &request("workspace.get"))
            .data
            .unwrap()["items"]
            .clone();
        let p = preview(
            &mut snapshot,
            &mut state,
            json!([{"op":"item.assign","pane_id":"2","item_ids":[items[0]["id"]]}]),
        );
        assert_eq!(
            snapshot.respond(&mut state, &apply(&p)).error.unwrap().code,
            "CAPABILITY_UNAVAILABLE"
        );
        assert_eq!(state.store.change_count(), count);
    }
    #[test]
    fn assignment_preserves_unrelated_locked_pane_and_reorder_checks_membership() {
        let mut state = super::super::super::tests::test_state();
        let mut snapshot = Snapshot::new();
        let pane = PanelId::new(2);
        state.workspace.desktop_items_mut()[2].set_placement(DesktopPlacement::Pane {
            pane_id: pane,
            position: GridPosition::new(9, 3),
        });
        state.workspace.panel_mut(pane).unwrap().set_locked(true);
        let original = state.workspace.clone();
        let data = snapshot.respond(&mut state, &request("workspace.get"));
        let items = &data.data.as_ref().unwrap()["items"];
        let plan:Plan=serde_json::from_value(json!({"protocol_version":1,"base":data.context,"operations":[{"op":"pane.create","ref":"new","title":"new"},{"op":"item.assign","pane_ref":"new","item_ids":[items[0]["id"]]}]})).unwrap();
        let prepared = prepare(&state.workspace, &state.store, &plan, &snapshot.item_ids, &luciddesk_window::enumerate_monitors(), &super::content_layout::snapshots(&state), &super::geometry::observe_layout(&state), None).unwrap();
        assert_eq!(
            prepared.next.desktop_items()[2],
            original.desktop_items()[2]
        );
        assert_eq!(state.workspace, original);
        let p = preview(
            &mut snapshot,
            &mut state,
            json!([{"op":"item.reorder","pane_id":"1","item_ids":[items[0]["id"],items[0]["id"]]}]),
        );
        assert!(!p.ok);
        let p = preview(
            &mut snapshot,
            &mut state,
            json!([{"op":"pane.update","pane_id":"2","title":"locked"}]),
        );
        assert_eq!(p.error.unwrap().code, "PANE_LOCKED");
    }
    #[test]
    fn cli_release_and_assignment_preserve_fixed_grid_holes() {
        let mut state = super::super::super::tests::test_state();
        let id = PanelId::new(1);
        fixed_grid::toggle(&mut state.workspace, id).unwrap();
        let entries: Vec<_> = super::super::super::ordered_desktop_items(&state.workspace,id).iter().map(|i| i.identity().persistent_key()).collect();
        fixed_grid::place(&mut state.workspace,id,&entries[..1],GridPosition::new(1,4));
        let mut snapshot = Snapshot::new();
        snapshot.respond(&mut state,&request("workspace.get"));
        let released = snapshot.item_ids[&entries[1]].clone();
        let p=preview(&mut snapshot,&mut state,json!([{"op":"item.release","item_ids":[released]}]));
        assert!(p.ok,"{p:?}");
        let next=snapshot.plans.pending.back().unwrap().next.clone();
        assert!(matches!(next.desktop_items().iter().find(|i|i.identity().persistent_key()==entries[0]).unwrap().placement(),DesktopPlacement::Pane{position,..} if *position==GridPosition::new(1,4)));
        state.workspace=next;
        let p=preview(&mut snapshot,&mut state,json!([{"op":"item.assign","pane_id":"1","item_ids":[released]}]));
        assert!(p.ok,"{p:?}");
        let next=&snapshot.plans.pending.back().unwrap().next;
        assert!(matches!(next.desktop_items().iter().find(|i|i.identity().persistent_key()==entries[0]).unwrap().placement(),DesktopPlacement::Pane{position,..} if *position==GridPosition::new(1,4)));
    }

    #[test]
    fn explicit_free_reorder_materializes_coordinates() {
        let mut state=crate::pane::tests::test_state();let id=PanelId::new(1);
        fixed_grid::toggle(&mut state.workspace,id).unwrap();
        free_layout::toggle(&mut state.workspace,id).unwrap();
        let mut keys=ordered(&state.workspace,id);keys.reverse();
        order(&mut state.workspace,id,&keys);
        let positions:Vec<_>=state.workspace.desktop_items().iter().map(|i|i.pane_position().unwrap()).collect();
        let mut g=free_layout::grid(&state.workspace);g.cell_width*=1.5;g.cell_height*=1.5;
        for (i,p) in state.workspace.desktop_items().iter().zip(positions) { assert_eq!(free_layout::point(i,g),p); }
        state.store.save_workspace(&state.workspace).unwrap();
        assert_eq!(state.store.load_workspace().unwrap().desktop_items()[0].pane_position(),state.workspace.desktop_items()[0].pane_position());
    }

}
