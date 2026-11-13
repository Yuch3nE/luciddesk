//! Tab operations mutate only the candidate workspace.
use super::*;
use luciddesk_api::Operation;
use luciddesk_core::PaneTabs;
fn panel_id(raw: &str) -> Result<PanelId, String> {
    raw.parse::<u64>()
        .ok()
        .filter(|v| *v > 0 && *v <= i64::MAX as u64)
        .map(PanelId::new)
        .ok_or_else(|| "invalid panel ID".into())
}
fn editable(workspace: &Workspace, id: PanelId, locked: bool) -> Result<(), String> {
    let p = workspace.panel(id).ok_or("panel does not exist")?;
    if !p.supports_tabs() {
        return Err("only ordinary desktop panels support tabs".into());
    }
    if locked && p.locked() {
        return Err("panel is locked; explicitly unlock it first".into());
    }
    Ok(())
}
/// Returns the destination window's active tab when a merge changes membership.
pub(super) fn apply(next: &mut Workspace, op: &Operation) -> Result<Option<PanelId>, String> {
    let mut groups = next.tab_groups().to_vec();
    let mut destination_active = None;
    match op {
        Operation::TabMerge {
            pane_id,
            into_pane_id,
        } => {
            let source = panel_id(pane_id)?;
            let target = panel_id(into_pane_id)?;
            editable(next, source, true)?;
            editable(next, target, false)?;
            if source == target
                || next
                    .tab_group(source)
                    .is_some_and(|g| g.members.contains(&target))
            {
                return Ok(None);
            }
            let source_members = next
                .tab_group(source)
                .map_or_else(|| vec![source], |g| g.members.clone());
            let mut destination = next.tab_group(target).cloned().unwrap_or(PaneTabs {
                members: vec![target],
                active: target,
            });
            for id in source_members.iter().chain(destination.members.iter()) {
                editable(next, *id, source_members.contains(id))?;
            }
            destination_active = Some(destination.active);
            destination.members.extend(source_members);
            groups.retain(|g| !g.members.contains(&source) && !g.members.contains(&target));
            groups.push(destination);
        }
        Operation::TabSelect { pane_id } => {
            let id = panel_id(pane_id)?;
            editable(next, id, false)?;
            groups
                .iter_mut()
                .find(|g| g.members.contains(&id))
                .ok_or("panel has no tab group")?
                .active = id;
        }
        Operation::TabReorder { pane_id, pane_ids } => {
            let id = panel_id(pane_id)?;
            editable(next, id, false)?;
            let group = groups
                .iter_mut()
                .find(|g| g.members.contains(&id))
                .ok_or("panel has no tab group")?;
            let members = pane_ids
                .iter()
                .map(|id| panel_id(id))
                .collect::<Result<Vec<_>, _>>()?;
            let unique: std::collections::HashSet<_> = members.iter().copied().collect();
            if members.len() != group.members.len()
                || unique.len() != members.len()
                || group.members.iter().any(|id| !unique.contains(id))
            {
                return Err("reorder requires every member exactly once".into());
            }
            for id in &members {
                editable(next, *id, false)?;
            }
            group.members = members;
        }
        Operation::TabDetach { pane_id } => {
            let id = panel_id(pane_id)?;
            editable(next, id, true)?;
            let Some(group) = groups.iter_mut().find(|g| g.members.contains(&id)) else {
                return Ok(None);
            };
            group.members.retain(|member| *member != id);
            if group.active == id {
                group.active = group.members[0];
            }
            groups.retain(|g| g.members.len() > 1);
            // Keep the expanded bounds; a following pane.geometry can position the new window.
        }
        _ => return Err("not a tab operation".into()),
    }
    next.set_tab_groups(groups).map_err(|e| e.to_string())?;
    Ok(destination_active)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merge_reorder_select_and_detach_preserve_content_and_validate_membership() {
        let mut next = crate::pane::tests::test_state().workspace;
        let original = next.desktop_items().to_vec();
        apply(
            &mut next,
            &Operation::TabMerge {
                pane_id: "2".into(),
                into_pane_id: "1".into(),
            },
        )
        .unwrap();
        assert_eq!(
            next.tab_group(PanelId::new(1)).unwrap().members,
            vec![PanelId::new(1), PanelId::new(2)]
        );
        let before = next.clone();
        assert!(
            apply(
                &mut next,
                &Operation::TabReorder {
                    pane_id: "1".into(),
                    pane_ids: vec!["1".into(), "1".into()]
                }
            )
            .is_err()
        );
        assert_eq!(next, before);
        apply(
            &mut next,
            &Operation::TabReorder {
                pane_id: "1".into(),
                pane_ids: vec!["2".into(), "1".into()],
            },
        )
        .unwrap();
        apply(
            &mut next,
            &Operation::TabSelect {
                pane_id: "2".into(),
            },
        )
        .unwrap();
        assert!(next.tab_visible(PanelId::new(2)));
        assert!(!next.tab_visible(PanelId::new(1)));
        apply(
            &mut next,
            &Operation::TabDetach {
                pane_id: "2".into(),
            },
        )
        .unwrap();
        assert!(next.tab_groups().is_empty());
        assert_eq!(next.desktop_items(), original);
        next.panel_mut(PanelId::new(1)).unwrap().set_locked(true);
        assert!(
            apply(
                &mut next,
                &Operation::TabMerge {
                    pane_id: "2".into(),
                    into_pane_id: "1".into()
                }
            )
            .is_ok()
        );
    }
}
