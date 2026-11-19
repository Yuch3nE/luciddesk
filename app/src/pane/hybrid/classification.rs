//! Desktop classification over accepted inventories and explicit manual previews.
use super::*;
use luciddesk_core::{Classification, ClassificationItemType, CompiledClassificationRule};
use luciddesk_storage::SettingValue;
use std::collections::HashSet;
use std::time::SystemTime;

pub(in crate::pane) const KEY: &str = "desktop.classification";
const NAME_SETTLE: Duration = Duration::from_secs(1);
const TMP_SETTLE: Duration = Duration::from_secs(60);

pub(super) fn load(store: &WorkspaceStore) -> Result<(String, Classification), String> {
    let mut settings = store.settings().map_err(|error| error.to_string())?;
    match settings.remove(KEY) {
        Some(SettingValue::Text(raw)) => {
            let config = serde_json::from_str(&raw).map_err(|error| error.to_string())?;
            Ok((raw, config))
        }
        _ => Ok((String::new(), Classification::default())),
    }
}

struct PendingName {
    name: std::ffi::OsString,
    label: String,
    stable_at: Instant,
    evaluated: bool,
}

#[derive(Default)]
pub(super) struct Automatic {
    enabled: bool,
    // None means the next accepted inventory establishes a baseline.
    seen: Option<HashSet<String>>,
    deadline: Option<Instant>,
    pending: HashMap<String, PendingName>,
    renaming: bool,
}

impl Automatic {
    pub(super) fn set_enabled(&mut self, enabled: bool) -> bool {
        if self.enabled == enabled {
            return false;
        }
        self.enabled = enabled;
        self.seen = None;
        self.pending.clear();
        self.renaming = false;
        self.deadline = None;
        true
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub(super) fn take_due(&mut self, now: Instant) -> bool {
        self.deadline.take_if(|deadline| *deadline <= now).is_some()
    }

    fn remember(&mut self, identity: &ShellIdentity) {
        if let Some(seen) = &mut self.seen {
            let key = identity.persistent_key();
            self.pending.remove(&key);
            seen.insert(key);
        }
    }

    pub(super) fn record_released(&mut self, before: &Workspace, after: &Workspace) {
        if self.seen.is_none() {
            return;
        }
        let grouped: HashSet<_> = before.desktop_items().iter()
            .filter(|item| !unassigned(item))
            .map(|item| item.identity().persistent_key())
            .collect();
        for item in after.desktop_items().iter().filter(|item| unassigned(item)) {
            if grouped.contains(&item.identity().persistent_key()) {
                self.remember(item.identity());
            }
        }
    }

    fn candidates<'a>(
        &mut self,
        items: &'a [DesktopItem],
        times: &InventoryMetadata<'_>,
        now: Instant,
        renaming: bool,
    ) -> Vec<&'a DesktopItem> {
        self.deadline = None;
        if !self.enabled {
            return Vec::new();
        }
        let Some(seen) = &mut self.seen else {
            self.seen = Some(
                items
                    .iter()
                    .map(|item| item.identity().persistent_key())
                    .collect(),
            );
            return Vec::new();
        };
        let finished_rename = std::mem::replace(&mut self.renaming, renaming) && !renaming;
        let mut next_seen = HashSet::with_capacity(items.len());
        let mut present = HashSet::with_capacity(items.len());
        let mut candidates = Vec::new();
        for item in items {
            let key = item.identity().persistent_key();
            present.insert(key.clone());
            if seen.contains(&key) || !unassigned(item) {
                self.pending.remove(&key);
                next_seen.insert(key);
                continue;
            }
            let name = item.identity().activation_name().to_owned();
            let pending = self
                .pending
                .entry(key.clone())
                .or_insert_with(|| PendingName {
                    name: name.clone(),
                    label: item.display_name().into(),
                    stable_at: now + NAME_SETTLE,
                    evaluated: false,
                });
            let name_changed = pending.name != name || pending.label != item.display_name();
            if name_changed { pending.evaluated = false; }
            if name_changed || renaming || finished_rename {
                pending.name = name;
                pending.label = item.display_name().into();
                pending.stable_at = now + NAME_SETTLE;
            }
            if pending.evaluated { continue; }
            let name_wait = pending.stable_at.saturating_duration_since(now);
            let wait = if renaming {
                Some(NAME_SETTLE)
            } else {
                times.ready_in(item).map(|wait| wait.max(name_wait))
            };
            match wait {
                Some(Duration::ZERO) => {
                    pending.evaluated = true;
                    candidates.push(item);
                }
                Some(wait) => {
                    if let Some(due) = now.checked_add(wait) {
                        self.deadline = Some(self.deadline.map_or(due, |old| old.min(due)));
                    }
                }
                None => {}
            }
            // A new partial file stays eligible for a later inventory. Its final
            // name can arrive with the same file ID when the download renames it.
        }
        self.pending.retain(|key, _| present.contains(key));
        *seen = next_seen;
        candidates
    }
}

fn unassigned(item: &DesktopItem) -> bool {
    matches!(item.placement(), DesktopPlacement::FreeDesktop { .. })
}

fn download_or_lock(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with("~$"))
        || path.extension().is_some_and(|extension| {
            [
                "crdownload",
                "part",
                "partial",
                "download",
                "!ut",
                "opdownload",
            ]
            .iter()
            .any(|suffix| extension.eq_ignore_ascii_case(suffix))
        })
}

fn is_tmp(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("tmp"))
}

pub(super) struct InventoryMetadata<'a> {
    entries: &'a [luciddesk_shell::ShellEntry],
    latest: HashMap<&'a Path, SystemTime>,
    wall_now: SystemTime,
}

impl<'a> InventoryMetadata<'a> {
    pub(super) fn new(entries: &'a [luciddesk_shell::ShellEntry], wall_now: SystemTime) -> Self {
        let latest = entries
            .iter()
            .filter_map(|entry| {
                let path = entry
                    .identity
                    .file_system_path()
                    .filter(|path| is_tmp(path))?;
                let timestamp = entry.created.into_iter().chain(entry.modified).max()?;
                Some((path, timestamp))
            })
            .collect();
        Self { entries, latest, wall_now }
    }

    fn item_types(
        &self,
        rules: &[CompiledClassificationRule<'_>],
    ) -> HashMap<&'a Path, ClassificationItemType> {
        if !rules.iter().any(CompiledClassificationRule::requires_item_type) {
            return HashMap::new();
        }
        self.entries
            .iter()
            .filter_map(|entry| {
                let path = entry.identity.file_system_path()?;
                let directory = entry.attributes.filesystem_directory?;
                let item_type = if directory {
                    ClassificationItemType::Folder
                } else if entry.attributes.link
                    || path.extension().is_some_and(|extension| {
                        ["lnk", "url", "website"]
                            .iter()
                            .any(|suffix| extension.eq_ignore_ascii_case(suffix))
                    })
                {
                    ClassificationItemType::Shortcut
                } else {
                    ClassificationItemType::File
                };
                Some((path, item_type))
            })
            .collect()
    }

    /// Zero means ready. Unknown timestamps and unfinished names wait for a later inventory.
    fn ready_in(&self, item: &DesktopItem) -> Option<Duration> {
        self.readiness(item).ok()
    }

    fn readiness(&self, item: &DesktopItem) -> Result<Duration, SkipReason> {
        let Some(path) = item.identity().file_system_path() else {
            return Ok(Duration::ZERO);
        };
        if download_or_lock(path) {
            return Err(SkipReason::DownloadOrLock);
        }
        if !is_tmp(path) {
            return Ok(Duration::ZERO);
        }
        let ready_at = self.latest.get(path).and_then(|time| time.checked_add(TMP_SETTLE))
            .ok_or(SkipReason::UnknownTime)?;
        Ok(ready_at.duration_since(self.wall_now).unwrap_or_default())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::pane) struct Move {
    pub(in crate::pane) identity: ShellIdentity,
    pub(in crate::pane) item_name: String,
    pub(in crate::pane) target: PanelId,
    pub(in crate::pane) target_name: String,
    pub(in crate::pane) rule: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::pane) enum SkipReason {
    DownloadOrLock,
    TemporaryFile,
    UnknownTime,
    VirtualIcon,
    MissingTarget,
    NoMatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::pane) enum PreviewEntry {
    Move(Move),
    Skip { identity: ShellIdentity, item_name: String, reason: SkipReason },
}

impl PreviewEntry {
    pub(super) fn identity(&self) -> &ShellIdentity {
        match self {
            Self::Move(entry) => &entry.identity,
            Self::Skip { identity, .. } => identity,
        }
    }
}

fn targets<'a>(workspace: &'a Workspace, rules: &[CompiledClassificationRule<'_>]) -> Vec<Option<&'a Panel>> {
    rules.iter().map(|rule| {
        workspace.panel(PanelId::new(rule.target_panel())).filter(|panel| panel.supports_tabs())
    }).collect()
}

fn matching_move(
    item: &DesktopItem,
    item_type: Option<ClassificationItemType>,
    rules: &[CompiledClassificationRule<'_>],
    targets: &[Option<&Panel>],
) -> Option<Move> {
    let (index, target) = rules.iter().zip(targets).enumerate().find_map(|(index, (rule, target))| {
        target.filter(|_| rule.matches_with_type(item, item_type)).map(|panel| (index, panel))
    })?;
    Some(Move {
        identity: item.identity().clone(),
        item_name: item.display_name().into(),
        target: target.id(),
        target_name: target.title().into(),
        rule: index + 1,
    })
}

pub(super) fn preview(
    workspace: &Workspace,
    rules: &[CompiledClassificationRule<'_>],
    metadata: &InventoryMetadata<'_>,
) -> Vec<PreviewEntry> {
    let targets = targets(workspace, rules);
    let item_types = metadata.item_types(rules);
    workspace.desktop_items().iter().filter(|item| unassigned(item)).map(|item| {
        let reason = match metadata.readiness(item) {
            Err(reason) => reason,
            Ok(wait) if !wait.is_zero() => SkipReason::TemporaryFile,
            Ok(_) if item.identity().file_system_path().is_none() => SkipReason::VirtualIcon,
            Ok(_) => {
                let item_type = item.identity().file_system_path()
                    .and_then(|path| item_types.get(path).copied());
                if let Some(entry) = matching_move(item, item_type, rules, &targets) {
                    return PreviewEntry::Move(entry);
                }
                if rules.iter().zip(&targets).any(|(rule, target)| {
                    target.is_none() && rule.matches_with_type(item, item_type)
                }) {
                    SkipReason::MissingTarget
                } else {
                    SkipReason::NoMatch
                }
            }
        };
        PreviewEntry::Skip { identity: item.identity().clone(), item_name: item.display_name().into(), reason }
    }).collect()
}

pub(super) fn moves<'a>(
    workspace: &Workspace,
    items: impl IntoIterator<Item = &'a DesktopItem>,
    rules: &[CompiledClassificationRule<'_>],
    metadata: &InventoryMetadata<'_>,
) -> Vec<Move> {
    let targets = targets(workspace, rules);
    let item_types = metadata.item_types(rules);
    items
        .into_iter()
        .filter(|item| unassigned(item) && metadata.ready_in(item) == Some(Duration::ZERO))
        .filter_map(|item| {
            let item_type = item.identity().file_system_path()
                .and_then(|path| item_types.get(path).copied());
            matching_move(item, item_type, rules, &targets)
        })
        .collect()
}

pub(super) fn apply_moves<'a>(workspace: &mut Workspace, moves: impl IntoIterator<Item = &'a Move>) {
    for entry in moves {
        if workspace.panel(entry.target).is_some_and(Panel::fixed_grid) {
            fixed_grid::place(
                workspace,
                entry.target,
                &[entry.identity.persistent_key()],
                GridPosition::new(0, 0),
            );
        } else {
            let next = workspace
                .desktop_items()
                .iter()
                .filter_map(|item| match item.placement() {
                    DesktopPlacement::Pane { pane_id, position } if *pane_id == entry.target => {
                        Some(position.column.saturating_add(1))
                    }
                    _ => None,
                })
                .max()
                .unwrap_or(0);
            if let Some(item) = workspace.desktop_item_mut(&entry.identity) {
                item.set_placement(DesktopPlacement::Pane {
                    pane_id: entry.target,
                    position: GridPosition::new(next, 0),
                });
            }
        }
    }
}

// Query only the desktop ListView; unrelated Explorer windows must not hold up
// desktop classification. A busy/disconnected view is conservatively deferred.
fn desktop_renaming(view: isize) -> bool {
    use windows_sys::Win32::UI::Controls::LVM_GETEDITCONTROL;
    let mut edit = 0;
    unsafe {
        SendMessageTimeoutW(
            view as _,
            LVM_GETEDITCONTROL,
            0,
            0,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            50,
            &raw mut edit,
        ) == 0
            || edit != 0
    }
}

pub(super) fn apply(s: &mut PaneApp) -> Result<bool, String> {
    let (_, config) = load(&s.store)?;
    let session = s.session.as_mut().unwrap();
    let metadata = InventoryMetadata::new(&session.snapshot.items, SystemTime::now());
    let automatic = &mut session.classification;
    automatic.set_enabled(config.enabled);
    let candidates = automatic.candidates(
        s.workspace.desktop_items(),
        &metadata,
        Instant::now(),
        desktop_renaming(session.view),
    );
    if candidates.is_empty() {
        return Ok(false);
    }
    let rules = config.compile().map_err(|error| error.to_string())?;
    let planned = moves(&s.workspace, candidates, &rules, &metadata);
    // Preserve deliberate releases even if they happen before the next inventory.
    for entry in &planned {
        automatic.remember(&entry.identity);
    }
    apply_moves(&mut s.workspace, &planned);
    Ok(!planned.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(file: &str, id: u128) -> DesktopItem {
        DesktopItem::new(
            ShellIdentity::FileSystem {
                path: file.into(),
                volume_id: Some(1),
                file_id: Some(id),
            },
            file,
        )
    }

    fn settled_candidates<'a>(
        automatic: &mut Automatic,
        items: &'a [DesktopItem],
    ) -> Vec<&'a DesktopItem> {
        let now = Instant::now();
        let times = InventoryMetadata::new(&[], SystemTime::now());
        let mut result = automatic.candidates(items, &times, now, false);
        result.extend(automatic.candidates(items, &times, now + NAME_SETTLE, false));
        result
    }

    #[test]
    fn enabling_and_reconnecting_baseline_existing_icons() {
        let mut automatic = Automatic::default();
        let existing = item("existing.pdf", 1);
        assert!(settled_candidates(&mut automatic, std::slice::from_ref(&existing)).is_empty());

        assert!(automatic.set_enabled(true));
        assert!(settled_candidates(&mut automatic, std::slice::from_ref(&existing)).is_empty());
        let items = [existing, item("new.pdf", 2)];
        assert_eq!(settled_candidates(&mut automatic, &items), [&items[1]]);
        assert!(settled_candidates(&mut automatic, &items).is_empty());

        automatic.set_enabled(false);
        automatic.set_enabled(true);
        assert!(settled_candidates(&mut automatic, &items).is_empty());
        let mut reconnected = Automatic::default();
        reconnected.set_enabled(true);
        assert!(settled_candidates(&mut reconnected, &items).is_empty());
    }

    #[test]
    fn a_download_can_complete_under_the_same_identity() {
        let mut automatic = Automatic::default();
        automatic.set_enabled(true);
        let already_downloading = item("old.pdf.crdownload", 1);
        settled_candidates(&mut automatic, &[already_downloading]);

        let pending = [item("old.pdf", 1), item("new.pdf.CRDOWNLOAD", 2)];
        assert!(settled_candidates(&mut automatic, &pending).is_empty());
        assert!(settled_candidates(&mut automatic, &pending).is_empty());

        let finished = [item("old.pdf", 1), item("new.pdf", 2)];
        assert_eq!(
            settled_candidates(&mut automatic, &finished),
            [&finished[1]]
        );
        assert!(settled_candidates(&mut automatic, &finished).is_empty());
    }

    #[test]
    fn new_icons_wait_for_the_final_stable_name() {
        let mut automatic = Automatic::default();
        automatic.set_enabled(true);
        let start = Instant::now();
        let times = InventoryMetadata::new(&[], SystemTime::now());
        let old = [item("existing.pdf", 1)];
        assert!(automatic.candidates(&old, &times, start, false).is_empty());
        let first = [item("existing.pdf", 1), item("draft.txt", 2)];
        assert!(
            automatic
                .candidates(&first, &times, start, false)
                .is_empty()
        );
        let renamed = [item("existing-renamed.pdf", 1), item("final.pdf", 2)];
        let changed_at = start + Duration::from_millis(800);
        assert!(
            automatic
                .candidates(&renamed, &times, changed_at, false)
                .is_empty()
        );
        assert!(
            automatic
                .candidates(&renamed, &times, start + NAME_SETTLE, false)
                .is_empty()
        );
        assert_eq!(
            automatic.candidates(&renamed, &times, changed_at + NAME_SETTLE, false),
            [&renamed[1]]
        );
        assert!(
            automatic
                .candidates(&renamed, &times, changed_at + NAME_SETTLE * 2, false)
                .is_empty()
        );
    }

    #[test]
    fn unmatched_new_icons_retry_on_rename_without_revisiting_baseline() {
        let mut automatic = Automatic::default();
        automatic.set_enabled(true);
        let now = Instant::now();
        let times = InventoryMetadata::new(&[], SystemTime::now());
        automatic.candidates(&[item("old.txt", 1)], &times, now, false);
        let items = [item("old.txt", 1), item("draft.unknown", 2)];
        automatic.candidates(&items, &times, now, false);
        assert_eq!(automatic.candidates(&items, &times, now + NAME_SETTLE, false), [&items[1]]);
        assert!(automatic.candidates(&items, &times, now + NAME_SETTLE * 2, false).is_empty());
        assert_eq!(automatic.deadline(), None);
        let renamed = [item("old.pdf", 1), item("final.pdf", 2)];
        assert!(automatic.candidates(&renamed, &times, now + NAME_SETTLE * 3, false).is_empty());
        assert_eq!(automatic.candidates(&renamed, &times, now + NAME_SETTLE * 4, false), [&renamed[1]]);
    }

    #[test]
    fn native_rename_edit_waits_until_confirm_or_cancel_then_settles() {
        for committed in [false, true] {
            let mut automatic = Automatic::default();
            automatic.set_enabled(true);
            let start = Instant::now();
            let times = InventoryMetadata::new(&[], SystemTime::now());
            automatic.candidates(&[], &times, start, false);
            let draft = [item("new.txt", 2)];
            assert!(automatic.candidates(&draft, &times, start, true).is_empty());
            assert!(
                automatic
                    .candidates(&draft, &times, start + Duration::from_secs(20), true)
                    .is_empty()
            );
            let final_items = [item(if committed { "final.pdf" } else { "new.txt" }, 2)];
            let finished = start + Duration::from_secs(21);
            assert!(
                automatic
                    .candidates(&final_items, &times, finished, false)
                    .is_empty()
            );
            assert_eq!(automatic.deadline(), Some(finished + NAME_SETTLE));
            assert_eq!(
                automatic.candidates(&final_items, &times, finished + NAME_SETTLE, false),
                [&final_items[0]]
            );
        }
    }

    #[test]
    fn disappearing_and_assigned_candidates_do_not_leave_a_retry() {
        let mut automatic = Automatic::default();
        automatic.set_enabled(true);
        let start = Instant::now();
        let times = InventoryMetadata::new(&[], SystemTime::now());
        automatic.candidates(&[], &times, start, false);
        let mut items = [item("new.txt", 2)];
        automatic.candidates(&items, &times, start, false);
        items[0].set_placement(DesktopPlacement::Pane {
            pane_id: PanelId::new(1),
            position: GridPosition::new(0, 0),
        });
        assert!(
            automatic
                .candidates(&items, &times, start + NAME_SETTLE, false)
                .is_empty()
        );
        assert!(automatic.pending.is_empty());
        assert_eq!(automatic.deadline(), None);
        items[0].set_placement(DesktopPlacement::default());
        assert!(
            automatic
                .candidates(&items, &times, start + NAME_SETTLE * 2, false)
                .is_empty()
        );
        automatic.candidates(&[], &times, start + NAME_SETTLE * 3, false);
        automatic.candidates(&items, &times, start + NAME_SETTLE * 4, false);
        automatic.candidates(&[], &times, start + NAME_SETTLE * 5, false);
        assert!(automatic.pending.is_empty());
        assert_eq!(automatic.deadline(), None);
    }

    #[test]
    fn a_manual_release_between_inventories_cancels_pending_classification() {
        let mut automatic = Automatic::default();
        automatic.set_enabled(true);
        let now = Instant::now();
        let times = InventoryMetadata::new(&[], SystemTime::now());
        automatic.candidates(&[], &times, now, false);
        let id = PanelId::new(1);
        let mut workspace = Workspace::from_panels(vec![Panel::new(id, "Target", RectDip::default())]).unwrap();
        workspace.reconcile_desktop_items([item("new.pdf", 1), item("other.pdf", 2)]);
        automatic.candidates(workspace.desktop_items(), &times, now, false);

        workspace.desktop_items_mut()[0].set_placement(DesktopPlacement::Pane {
            pane_id: id, position: GridPosition::new(0, 0),
        });
        let grouped = workspace.clone();
        workspace.desktop_items_mut()[0].set_placement(DesktopPlacement::default());
        automatic.record_released(&grouped, &workspace);
        let later = now + NAME_SETTLE;
        assert_eq!(automatic.candidates(workspace.desktop_items(), &times, later, false), [&workspace.desktop_items()[1]]);

        automatic.candidates(&[], &times, later, false);
        let new = [item("new.pdf", 1)];
        automatic.candidates(&new, &times, later, false);
        assert_eq!(automatic.candidates(&new, &times, later + NAME_SETTLE, false), [&new[0]]);
    }

    #[test]
    fn tmp_deadline_uses_the_latest_timestamp_and_rechecks_writes() {
        let start = Instant::now();
        let wall = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let mut automatic = Automatic::default();
        automatic.set_enabled(true);
        settled_candidates(&mut automatic, &[]);
        let items = [
            item("stable.TMP", 1),
            item("file.pdf.opdownload", 2),
            item("~$file.docx", 3),
        ];
        let mut entry = luciddesk_shell::ShellEntry {
            identity: items[0].identity().clone(),
            display_name: "stable.TMP".into(),
            attributes: Default::default(),
            created: Some(wall),
            modified: Some(wall - Duration::from_secs(5)),
            size: None,
        };
        let scan = |automatic: &mut Automatic, entry: &luciddesk_shell::ShellEntry, seconds| {
            let offset = Duration::from_secs(seconds);
            let times = InventoryMetadata::new(std::slice::from_ref(entry), wall + offset);
            automatic.candidates(&items, &times, start + offset, false)
        };
        assert!(scan(&mut automatic, &entry, 0).is_empty());
        assert_eq!(automatic.deadline(), Some(start + TMP_SETTLE));
        assert!(automatic.take_due(start + TMP_SETTLE));
        entry.modified = Some(wall + Duration::from_secs(30));
        assert!(scan(&mut automatic, &entry, 60).is_empty());
        assert_eq!(automatic.deadline(), Some(start + Duration::from_secs(90)));
        assert_eq!(scan(&mut automatic, &entry, 90), [&items[0]]);
        assert_eq!(automatic.deadline(), None);
    }

    #[test]
    fn manual_preview_excludes_grouped_and_partial_icons_but_accepts_a_locked_target() {
        let source = PanelId::new(1);
        let target = PanelId::new(2);
        let mut workspace = Workspace::from_panels(vec![
            Panel::new(source, "Existing", RectDip::default()),
            Panel::new(target, "Documents", RectDip::default()),
        ])
        .unwrap();
        let mut grouped = item("grouped.pdf", 1);
        grouped.set_placement(DesktopPlacement::Pane {
            pane_id: source,
            position: GridPosition::new(0, 0),
        });
        workspace.reconcile_desktop_items([
            grouped.clone(),
            item("report.pdf", 2),
            item("report2.pdf.part", 3),
        ]);
        let config: Classification = serde_json::from_str(
            r#"{"enabled":false,"rules":[{"target_panel":2,"name_contains":"report"},{"target_panel":2,"extensions":["pdf"]}]}"#,
        )
        .unwrap();
        let rules = config.compile().unwrap();
        let times = InventoryMetadata::new(&[], SystemTime::now());
        let preview = moves(&workspace, workspace.desktop_items(), &rules, &times);
        assert_eq!(preview.len(), 1);
        assert_eq!(preview[0].item_name, "report.pdf");
        assert_eq!(preview[0].rule, 1);
        assert!(unassigned(&workspace.desktop_items()[1]));

        workspace.panel_mut(target).unwrap().set_locked(true);
        assert_eq!(moves(&workspace, workspace.desktop_items(), &rules, &times), preview);
        apply_moves(&mut workspace, &preview);
        assert_eq!(workspace.desktop_item(grouped.identity()), Some(&grouped));
        assert!(matches!(workspace.desktop_items()[1].placement(),
            DesktopPlacement::Pane { pane_id, .. } if *pane_id == target));
        assert!(unassigned(&workspace.desktop_items()[2]));
    }

    #[test]
    fn preview_explains_skips_and_keeps_the_first_available_matching_rule() {
        let now = SystemTime::now();
        let mut workspace = Workspace::from_panels(vec![Panel::new(
            PanelId::new(1), "Available", RectDip::default(),
        )]).unwrap();
        let pending = item("pending.tmp", 4);
        let pending_entry = luciddesk_shell::ShellEntry {
            identity: pending.identity().clone(),
            display_name: pending.display_name().into(),
            attributes: Default::default(),
            created: None,
            modified: Some(now),
            size: None,
        };
        workspace.reconcile_desktop_items([
            item("report.pdf", 1),
            item("missing.png", 2),
            item("download.pdf.part", 3),
            pending,
            item("unknown.tmp", 5),
            item("unmatched.docx", 6),
            DesktopItem::new(ShellIdentity::Namespace { parsing_name: "test:virtual".into() }, "Virtual"),
        ]);
        let config: Classification = serde_json::from_str(
            r#"{"rules":[{"target_panel":2,"extensions":["pdf","png"]},{"target_panel":1,"extensions":["pdf"]},{"target_panel":1,"extensions":["docx"],"enabled":false}]}"#,
        ).unwrap();
        let rules = config.compile().unwrap();
        let entries = [pending_entry];
        let times = InventoryMetadata::new(&entries, now);
        let decisions = preview(&workspace, &rules, &times);
        assert!(matches!(decisions.as_slice(), [
            PreviewEntry::Move(entry),
            PreviewEntry::Skip { reason: SkipReason::MissingTarget, .. },
            PreviewEntry::Skip { reason: SkipReason::DownloadOrLock, .. },
            PreviewEntry::Skip { reason: SkipReason::TemporaryFile, .. },
            PreviewEntry::Skip { reason: SkipReason::UnknownTime, .. },
            PreviewEntry::Skip { reason: SkipReason::NoMatch, .. },
            PreviewEntry::Skip { reason: SkipReason::VirtualIcon, .. },
        ] if entry.rule == 2 && entry.target == PanelId::new(1)));
        assert!(matches!(moves(&workspace, workspace.desktop_items(), &rules, &times).as_slice(),
            [entry] if entry.rule == 2 && entry.target == PanelId::new(1)));
    }

    #[test]
    fn preview_and_automatic_share_types_and_keep_untyped_rule_matching() {
        let mut workspace = Workspace::from_panels(
            (1..=4).map(|id| Panel::new(PanelId::new(id), format!("Group {id}"), RectDip::default())).collect(),
        ).unwrap();
        let specifications = [
            ("Report.PDF", Some(false), false, false),
            ("Archive.pdf", Some(true), true, false),
            ("Folder shortcut.lnk", Some(false), true, true),
            ("Web.URL", Some(false), false, false),
            ("Site.website", Some(false), false, false),
            ("Real directory.lnk", Some(true), true, true),
            ("Browsable.pdf", Some(false), true, false),
            ("Unknown type.pdf", None, true, false),
        ];
        let entries: Vec<_> = specifications.into_iter().enumerate().map(
            |(index, (name, directory, folder, link))| luciddesk_shell::ShellEntry {
                identity: item(name, index as u128 + 1).identity().clone(),
                display_name: name.into(),
                attributes: luciddesk_shell::ShellAttributes {
                    filesystem_directory: directory,
                    folder,
                    link,
                    ..Default::default()
                },
                created: None,
                modified: None,
                size: None,
            },
        ).collect();
        workspace.reconcile_desktop_items(entries.iter().map(|entry| {
            DesktopItem::new(entry.identity.clone(), &entry.display_name)
        }).chain([item("Without metadata.pdf", 9)]));
        let config: Classification = serde_json::from_str(
            r#"{"rules":[
                {"target_panel":1,"item_type":"file","extensions":["pdf"]},
                {"target_panel":2,"item_type":"folder"},
                {"target_panel":3,"item_type":"shortcut"},
                {"target_panel":4,"extensions":["pdf"]}
            ]}"#,
        ).unwrap();
        let rules = config.compile().unwrap();
        let metadata = InventoryMetadata::new(&entries, SystemTime::now());
        let decisions = preview(&workspace, &rules, &metadata);
        let preview_moves: Vec<_> = decisions.iter().filter_map(|decision| match decision {
            PreviewEntry::Move(entry) => Some(entry.clone()),
            PreviewEntry::Skip { .. } => None,
        }).collect();
        assert_eq!(preview_moves.iter().map(|entry| entry.target.get()).collect::<Vec<_>>(),
            [1, 2, 3, 3, 3, 2, 1, 4, 4]);

        let mut automatic = Automatic::default();
        automatic.set_enabled(true);
        let start = Instant::now();
        assert!(automatic.candidates(&[], &metadata, start, false).is_empty());
        assert!(automatic.candidates(workspace.desktop_items(), &metadata, start, false).is_empty());
        let candidates = automatic.candidates(workspace.desktop_items(), &metadata, start + NAME_SETTLE, false);
        assert_eq!(moves(&workspace, candidates, &rules, &metadata), preview_moves);

        let legacy = moves(&workspace, workspace.desktop_items(), &rules[3..], &metadata);
        assert!(legacy.iter().any(|entry| entry.item_name == "Archive.pdf"));
    }

    #[test]
    fn automatic_organize_groups_new_files_from_a_windows_inventory_and_persists_membership() {
        let fixture = tempfile::tempdir().unwrap();
        let desktop = fixture.path().join("desktop");
        std::fs::create_dir(&desktop).unwrap();
        std::fs::write(desktop.join("Existing.pdf"), b"existing").unwrap();
        let database = fixture.path().join("workspace.db");
        let mut store = WorkspaceStore::open(&database).unwrap();
        let rules_json = r#"{"enabled":true,"rules":[
            {"target_panel":99,"item_type":"file","extensions":["pdf"]},
            {"target_panel":1,"item_type":"file","extensions":["pdf"]},
            {"target_panel":2,"item_type":"folder"},
            {"target_panel":3,"item_type":"shortcut"},
            {"target_panel":2,"extensions":["pdf"]}
        ]}"#;
        store.save_settings(&[(KEY.into(), SettingValue::Text(rules_json.into()))].into())
            .unwrap();
        let (_, config) = load(&store).unwrap();
        let rules = config.compile().unwrap();
        let mut workspace = Workspace::from_panels(vec![
            Panel::new(PanelId::new(1), "Documents", RectDip::default()),
            Panel::new(PanelId::new(2), "Folders", RectDip::default()),
            Panel::new(PanelId::new(3), "Shortcuts", RectDip::default()),
        ]).unwrap();
        let scan = |workspace: &mut Workspace| {
            let entries = luciddesk_shell::enumerate_folder(&desktop).unwrap();
            workspace.reconcile_desktop_items(entries.iter().map(|entry| {
                DesktopItem::new(entry.identity.clone(), &entry.display_name)
            }));
            entries
        };
        let group_of = |workspace: &Workspace, name| {
            match workspace.desktop_items().iter().find(|item| item.display_name() == name).unwrap().placement() {
                DesktopPlacement::Pane { pane_id, .. } => Some(pane_id.get()),
                _ => None,
            }
        };
        let mut automatic = Automatic::default();
        automatic.set_enabled(config.enabled);
        let start = Instant::now();
        let baseline = scan(&mut workspace);
        let metadata = InventoryMetadata::new(&baseline, SystemTime::now());
        assert!(automatic.candidates(workspace.desktop_items(), &metadata, start, false).is_empty());

        std::fs::write(desktop.join("Report.PDF"), b"report").unwrap();
        std::fs::create_dir(desktop.join("Project.pdf")).unwrap();
        std::fs::write(desktop.join("Quick.url"), b"[InternetShortcut]\r\nURL=file:///C:/\r\n").unwrap();
        std::fs::write(desktop.join("Download.pdf.crdownload"), b"download").unwrap();
        let next = scan(&mut workspace);
        let metadata = InventoryMetadata::new(&next, SystemTime::now());
        assert!(automatic.candidates(workspace.desktop_items(), &metadata, start, false).is_empty());
        let ready = automatic.candidates(workspace.desktop_items(), &metadata, start + NAME_SETTLE, false);
        let planned = moves(&workspace, ready, &rules, &metadata);
        assert_eq!(planned.len(), 3);
        assert!(planned.iter().any(|entry| entry.item_name == "Report.PDF" && entry.rule == 2));
        apply_moves(&mut workspace, &planned);
        assert_eq!(group_of(&workspace, "Report.PDF"), Some(1));
        assert_eq!(group_of(&workspace, "Project.pdf"), Some(2));
        assert_eq!(group_of(&workspace, "Quick.url"), Some(3));
        assert_eq!(group_of(&workspace, "Existing.pdf"), None);
        assert_eq!(group_of(&workspace, "Download.pdf.crdownload"), None);

        std::fs::rename(desktop.join("Download.pdf.crdownload"), desktop.join("Download.pdf")).unwrap();
        let finished = scan(&mut workspace);
        let metadata = InventoryMetadata::new(&finished, SystemTime::now());
        let renamed_at = start + 2 * NAME_SETTLE;
        assert!(automatic.candidates(workspace.desktop_items(), &metadata, renamed_at, false).is_empty());
        let ready = automatic.candidates(workspace.desktop_items(), &metadata, renamed_at + NAME_SETTLE, false);
        let planned = moves(&workspace, ready, &rules, &metadata);
        assert!(matches!(planned.as_slice(), [entry] if entry.item_name == "Download.pdf"));
        apply_moves(&mut workspace, &planned);
        assert_eq!(group_of(&workspace, "Download.pdf"), Some(1));
        assert!(matches!(preview(&workspace, &rules, &metadata).as_slice(),
            [PreviewEntry::Move(entry)] if entry.item_name == "Existing.pdf"));

        store.save_workspace(&workspace).unwrap();
        drop(store);
        let reopened = WorkspaceStore::open(&database).unwrap();
        let saved_workspace = reopened.load_workspace().unwrap();
        assert_eq!(saved_workspace.desktop_items().len(), workspace.desktop_items().len());
        for item in workspace.desktop_items() {
            assert_eq!(saved_workspace.desktop_item(item.identity()), Some(item));
        }
        assert_eq!(std::fs::read(desktop.join("Report.PDF")).unwrap(), b"report");
        assert_eq!(std::fs::read(desktop.join("Download.pdf")).unwrap(), b"download");
        assert!(desktop.join("Project.pdf").is_dir());
    }
}
