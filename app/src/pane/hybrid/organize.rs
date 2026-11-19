//! Manual organize previews and the most recent committed batch.
use super::*;
use classification::{InventoryMetadata, Move};
use std::collections::{BTreeMap, HashSet};
use std::rc::Weak;
use std::time::SystemTime;

pub(in crate::pane) use classification::{PreviewEntry, SkipReason};

pub(in crate::pane) struct OrganizePreview {
    session: Weak<FilterSession>,
    rules: String,
    pub(in crate::pane) entries: Vec<PreviewEntry>,
    pub(in crate::pane) changes: Option<Vec<PreviewChange>>,
}

pub(in crate::pane) enum PreviewChange {
    Added(PreviewEntry),
    Removed(PreviewEntry),
    Changed {
        before: PreviewEntry,
        after: PreviewEntry,
    },
}

pub(in crate::pane) enum OrganizeOutcome {
    Applied(Vec<Move>),
    Reconfirm(OrganizePreview),
    Undone { restored: usize, skipped: usize },
}

pub(super) enum OrganizeOperation {
    Apply(OrganizePreview),
    Undo,
}

pub(super) struct OrganizeRequest {
    pub(super) operation: OrganizeOperation,
    pub(super) result: mpsc::Sender<Result<OrganizeOutcome, String>>,
}

impl OrganizePreview {
    #[cfg(test)]
    pub(in crate::pane) fn fixture(
        entries: Vec<PreviewEntry>,
        changes: Option<Vec<PreviewChange>>,
    ) -> Self {
        Self {
            session: Weak::new(),
            rules: String::new(),
            entries,
            changes,
        }
    }

    pub(in crate::pane) fn move_count(&self) -> usize {
        self.moves().count()
    }

    pub(in crate::pane) fn moves(&self) -> impl Iterator<Item = &Move> {
        self.entries.iter().filter_map(|entry| match entry {
            PreviewEntry::Move(entry) => Some(entry),
            PreviewEntry::Skip { .. } => None,
        })
    }

    fn differs_from(&self, previous: &Self) -> bool {
        !self.session.ptr_eq(&previous.session)
            || self.rules != previous.rules
            || self.entries != previous.entries
    }

    fn update_from(&mut self, previous: Self) {
        let mut old: BTreeMap<_, _> = previous
            .entries
            .into_iter()
            .map(|entry| (entry.identity().persistent_key(), entry))
            .collect();
        let mut changes = Vec::new();
        for after in &self.entries {
            match old.remove(&after.identity().persistent_key()) {
                Some(before) if before != *after => changes.push(PreviewChange::Changed {
                    before,
                    after: after.clone(),
                }),
                None => changes.push(PreviewChange::Added(after.clone())),
                Some(_) => {}
            }
        }
        changes.extend(old.into_values().map(PreviewChange::Removed));
        self.changes = Some(changes);
    }
}

pub(in crate::pane) fn preview_organize(s: &PaneApp) -> Result<OrganizePreview, String> {
    let session = s
        .session
        .as_ref()
        .filter(|session| is_alive(session) && !session.menu_active.get())
        .ok_or_else(|| crate::i18n::text("classification-desktop-unavailable"))?;
    let (raw, config) = classification::load(&s.store)?;
    let rules = config.compile().map_err(|error| error.to_string())?;
    let metadata = InventoryMetadata::new(&session.snapshot.items, SystemTime::now());
    Ok(OrganizePreview {
        session: Rc::downgrade(&session.hook),
        rules: raw,
        entries: classification::preview(&s.workspace, &rules, &metadata),
        changes: None,
    })
}

pub(in crate::pane) fn apply_organize(
    s: &mut PaneApp,
    preview: OrganizePreview,
) -> Result<mpsc::Receiver<Result<OrganizeOutcome, String>>, String> {
    enqueue(s, OrganizeOperation::Apply(preview))
}

pub(in crate::pane) fn undo_organize(
    s: &mut PaneApp,
) -> Result<mpsc::Receiver<Result<OrganizeOutcome, String>>, String> {
    enqueue(s, OrganizeOperation::Undo)
}

fn enqueue(
    s: &mut PaneApp,
    operation: OrganizeOperation,
) -> Result<mpsc::Receiver<Result<OrganizeOutcome, String>>, String> {
    let session = s
        .session
        .as_mut()
        .filter(|session| is_alive(session) && !session.menu_active.get())
        .ok_or_else(|| crate::i18n::text("classification-desktop-unavailable"))?;
    if session.organize.is_some() {
        return Err(crate::i18n::text("ui-desktop-groups-are-syncing-try-again-shortly").into());
    }
    let (result, receiver) = mpsc::channel();
    session.organize = Some(OrganizeRequest { operation, result });
    // Discard captures started before the user confirmed the operation.
    session.audit.invalidate_snapshot();
    s.wake.notify();
    Ok(receiver)
}

pub(super) fn commit(
    s: &mut PaneApp,
    operation: OrganizeOperation,
) -> Result<OrganizeOutcome, String> {
    let outcome = match operation {
        OrganizeOperation::Apply(preview) => {
            let current = preview_organize(s)?;
            save_organize(
                &mut s.store,
                &mut s.workspace,
                &mut s.last_organize,
                preview,
                current,
            )?
        }
        OrganizeOperation::Undo => {
            let before = s.workspace.clone();
            let outcome = save_undo(&mut s.store, &mut s.workspace, &mut s.last_organize)?;
            if outcome.changed() {
                record_released_items(s, &before);
            }
            outcome
        }
    };
    if outcome.changed() {
        let session = s.session.as_mut().unwrap();
        session.audit.invalidate_snapshot();
        session.dirty.set(true);
        queue_pane_icons(s, true);
        refresh_views(s);
        s.wake.notify();
    }
    Ok(outcome)
}

impl OrganizeOutcome {
    pub(super) fn changed(&self) -> bool {
        match self {
            Self::Applied(moves) => !moves.is_empty(),
            Self::Undone { restored, .. } => *restored > 0,
            Self::Reconfirm(_) => false,
        }
    }
}

fn save_organize(
    store: &mut WorkspaceStore,
    workspace: &mut Workspace,
    last_organize: &mut Option<OrganizeUndo>,
    preview: OrganizePreview,
    mut current: OrganizePreview,
) -> Result<OrganizeOutcome, String> {
    if current.differs_from(&preview) {
        current.update_from(preview);
        return Ok(OrganizeOutcome::Reconfirm(current));
    }
    let mut moves: Vec<_> = current.entries.into_iter().filter_map(|entry| match entry {
        PreviewEntry::Move(entry) => Some(entry),
        PreviewEntry::Skip { .. } => None,
    }).collect();
    if !moves.is_empty() {
        let mut next = workspace.clone();
        classification::apply_moves(&mut next, &moves);
        store
            .save_workspace(&next)
            .map_err(|error| error.to_string())?;
        let undo = OrganizeUndo::new(workspace, &next);
        moves.retain(|entry| undo.entries.get(&entry.identity.persistent_key()).is_some_and(|saved| {
            matches!(saved.grouped, DesktopPlacement::Pane { pane_id, .. } if pane_id == entry.target)
        }));
        *last_organize = Some(undo);
        *workspace = next;
    }
    Ok(OrganizeOutcome::Applied(moves))
}

pub(in crate::pane) struct OrganizeUndo {
    count: usize,
    entries: HashMap<String, UndoEntry>,
}

struct UndoEntry {
    previous: DesktopPlacement,
    grouped: DesktopPlacement,
    pane_position: Option<luciddesk_core::PointDip>,
}

impl UndoEntry {
    fn matches(&self, item: &DesktopItem) -> bool {
        item.placement() == &self.grouped && item.pane_position() == self.pane_position
    }
}

impl OrganizeUndo {
    fn new(before: &Workspace, after: &Workspace) -> Self {
        // Applying a batch changes placements in place, preserving inventory order.
        let entries: HashMap<_, _> = before
            .desktop_items()
            .iter()
            .zip(after.desktop_items())
            .filter(|(before, after)| {
                matches!(before.placement(), DesktopPlacement::FreeDesktop { .. })
                    && matches!(after.placement(), DesktopPlacement::Pane { .. })
            })
            .map(|(before, after)| {
                (
                    after.identity().persistent_key(),
                    UndoEntry {
                        previous: before.placement().clone(),
                        grouped: after.placement().clone(),
                        pane_position: after.pane_position(),
                    },
                )
            })
            .collect();
        Self {
            count: entries.len(),
            entries,
        }
    }

    pub(in crate::pane) fn retain_unchanged(&mut self, workspace: &Workspace) {
        if self.entries.is_empty() {
            return;
        }
        // A later move or removal invalidates the entry even if the same cell is reused.
        let unchanged: HashSet<_> = workspace
            .desktop_items()
            .iter()
            .filter_map(|item| {
                let key = item.identity().persistent_key();
                self.entries
                    .get(&key)
                    .filter(|entry| entry.matches(item))
                    .map(|_| key)
            })
            .collect();
        self.entries.retain(|key, _| unchanged.contains(key));
    }
}

fn save_undo(
    store: &mut WorkspaceStore,
    workspace: &mut Workspace,
    last_organize: &mut Option<OrganizeUndo>,
) -> Result<OrganizeOutcome, String> {
    let mut restored = 0;
    let Some(undo) = last_organize.as_ref() else {
        return Ok(OrganizeOutcome::Undone {
            restored,
            skipped: 0,
        });
    };
    let mut next = workspace.clone();
    for item in next.desktop_items_mut() {
        if let Some(entry) = undo
            .entries
            .get(&item.identity().persistent_key())
            .filter(|entry| entry.matches(item))
        {
            item.set_placement(entry.previous.clone());
            restored += 1;
        }
    }
    if restored > 0 {
        store
            .save_workspace(&next)
            .map_err(|error| error.to_string())?;
        *workspace = next;
    }
    let skipped = undo.count - restored;
    *last_organize = None;
    Ok(OrganizeOutcome::Undone { restored, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = r#"{"rules":[{"target_panel":1,"extensions":["pdf","tmp"]}]}"#;

    fn entry(name: &str, id: u128) -> luciddesk_shell::ShellEntry {
        luciddesk_shell::ShellEntry {
            identity: ShellIdentity::FileSystem {
                path: name.into(),
                volume_id: Some(1),
                file_id: Some(id),
            },
            display_name: name.into(),
            attributes: Default::default(),
            created: None,
            modified: None,
            size: None,
        }
    }

    fn workspace() -> Workspace {
        Workspace::from_panels(vec![
            Panel::new(PanelId::new(1), "First", RectDip::default()),
            Panel::new(PanelId::new(2), "Second", RectDip::default()),
        ])
        .unwrap()
    }

    fn reconcile(workspace: &mut Workspace, entries: &[luciddesk_shell::ShellEntry]) {
        workspace.reconcile_desktop_items(
            entries
                .iter()
                .map(|entry| DesktopItem::new(entry.identity.clone(), &entry.display_name)),
        );
    }

    fn plan(
        workspace: &Workspace,
        entries: &[luciddesk_shell::ShellEntry],
        now: SystemTime,
    ) -> OrganizePreview {
        let config: luciddesk_core::Classification = serde_json::from_str(RULES).unwrap();
        OrganizePreview {
            session: Weak::new(),
            rules: RULES.into(),
            entries: classification::preview(
                workspace,
                &config.compile().unwrap(),
                &InventoryMetadata::new(entries, now),
            ),
            changes: None,
        }
    }

    #[test]
    fn changed_preview_reports_added_removed_and_changed_entries_before_any_save() {
        let now = SystemTime::now();
        let mut writing = entry("writing.tmp", 2);
        writing.modified = Some(now - Duration::from_secs(90));
        let original = [
            entry("report.pdf", 1),
            writing.clone(),
            entry("removed.pdf", 3),
        ];
        let mut workspace = workspace();
        reconcile(&mut workspace, &original);
        let preview = plan(&workspace, &original, now);
        assert_eq!(preview.move_count(), 3);

        writing.modified = Some(now);
        let fresh = [entry("renamed.pdf", 1), writing, entry("added.pdf", 4)];
        reconcile(&mut workspace, &fresh);
        let before = workspace.clone();
        let mut store = WorkspaceStore::open_in_memory().unwrap();
        store.save_workspace(&workspace).unwrap();
        let writes = store.change_count();
        let mut undo = None;
        let current = plan(&workspace, &fresh, now);
        let OrganizeOutcome::Reconfirm(updated) =
            save_organize(&mut store, &mut workspace, &mut undo, preview, current).unwrap()
        else {
            panic!("the changed batch requires confirmation");
        };
        let changes = updated.changes.as_deref().unwrap();
        assert!(matches!(changes, [
            PreviewChange::Changed { before: PreviewEntry::Move(old), after: PreviewEntry::Move(new) },
            PreviewChange::Changed { before: PreviewEntry::Move(_), after: PreviewEntry::Skip { reason: SkipReason::TemporaryFile, .. } },
            PreviewChange::Added(PreviewEntry::Move(added)),
            PreviewChange::Removed(PreviewEntry::Move(removed)),
        ] if old.item_name == "report.pdf" && new.item_name == "renamed.pdf"
            && added.item_name == "added.pdf" && removed.item_name == "removed.pdf"));
        assert_eq!(workspace, before);
        assert_eq!(store.change_count(), writes);
        assert!(undo.is_none());

        let confirmed = plan(&workspace, &fresh, now);
        let OrganizeOutcome::Applied(moves) =
            save_organize(&mut store, &mut workspace, &mut undo, updated, confirmed).unwrap()
        else {
            panic!("the confirmed batch should be applied");
        };
        let names: Vec<_> = moves.iter().map(|entry| entry.item_name.as_str()).collect();
        assert_eq!(names, ["renamed.pdf", "added.pdf"]);
        for entry in moves {
            assert_eq!(entry.target_name, "First");
            assert!(matches!(workspace.desktop_item(&entry.identity).unwrap().placement(),
                DesktopPlacement::Pane { pane_id, .. } if *pane_id == entry.target));
        }
        assert_eq!(store.load_workspace().unwrap(), workspace);
        assert_eq!(undo.unwrap().count, 2);
        assert!(matches!(
            workspace.desktop_items()[1].placement(),
            DesktopPlacement::FreeDesktop { .. }
        ));
    }

    #[test]
    fn rules_can_require_confirmation_even_with_no_visible_row_changes() {
        let now = SystemTime::now();
        let entries = [entry("report.pdf", 1)];
        let mut workspace = workspace();
        reconcile(&mut workspace, &entries);
        let old = plan(&workspace, &entries, now);
        let mut current = plan(&workspace, &entries, now);
        current.rules.push(' ');
        let mut store = WorkspaceStore::open_in_memory().unwrap();
        let mut undo = None;

        assert!(matches!(
            save_organize(&mut store, &mut workspace, &mut undo, old, current).unwrap(),
            OrganizeOutcome::Reconfirm(preview) if preview.changes.as_ref().is_some_and(Vec::is_empty)
        ));
        assert!(undo.is_none());
    }

    #[test]
    fn undo_uses_only_latest_batch_and_skips_moved_deleted_or_replaced_items() {
        let now = SystemTime::now();
        let mut entries = vec![entry("first-batch.pdf", 1)];
        let mut workspace = workspace();
        let mut store = WorkspaceStore::open_in_memory().unwrap();
        let mut undo = None;
        let mut organize = |workspace: &mut Workspace, entries: &[luciddesk_shell::ShellEntry]| {
            reconcile(workspace, entries);
            let preview = plan(workspace, entries, now);
            let confirmed = plan(workspace, entries, now);
            save_organize(&mut store, workspace, &mut undo, preview, confirmed).unwrap();
        };
        organize(&mut workspace, &entries);
        entries.extend([
            entry("restore.pdf", 2),
            entry("moved.pdf", 3),
            entry("deleted.pdf", 4),
            entry("replaced.pdf", 5),
        ]);
        reconcile(&mut workspace, &entries);
        let original_position = DesktopPlacement::FreeDesktop {
            monitor: Default::default(),
            position: luciddesk_core::PointDip::new(24.0, 36.0),
        };
        workspace
            .desktop_item_mut(&entries[1].identity)
            .unwrap()
            .set_placement(original_position.clone());
        organize(&mut workspace, &entries);

        let organized_cell = workspace
            .desktop_item(&entries[2].identity)
            .unwrap()
            .placement()
            .clone();
        workspace
            .desktop_item_mut(&entries[2].identity)
            .unwrap()
            .set_placement(DesktopPlacement::Pane {
                pane_id: PanelId::new(2),
                position: GridPosition::default(),
            });
        undo.as_mut().unwrap().retain_unchanged(&workspace);
        workspace
            .desktop_item_mut(&entries[2].identity)
            .unwrap()
            .set_placement(organized_cell.clone());
        entries.remove(3);
        entries[3] = entry("replaced.pdf", 6);
        reconcile(&mut workspace, &entries);
        assert!(matches!(
            save_undo(&mut store, &mut workspace, &mut undo).unwrap(),
            OrganizeOutcome::Undone {
                restored: 1,
                skipped: 3
            }
        ));
        assert_eq!(
            workspace
                .desktop_item(&entries[1].identity)
                .unwrap()
                .placement(),
            &original_position
        );
        assert_eq!(
            workspace
                .desktop_item(&entries[2].identity)
                .unwrap()
                .placement(),
            &organized_cell
        );
        for at in [0, 3] {
            assert!(
                matches!(workspace.desktop_item(&entries[at].identity).unwrap().placement(),
                DesktopPlacement::Pane { pane_id, .. } if *pane_id == PanelId::new(1))
            );
        }
        assert!(undo.is_none());
        assert_eq!(store.load_workspace().unwrap(), workspace);
    }
}
