//! Desktop classification over accepted inventories and explicit manual previews.
use super::*;
use luciddesk_core::{Classification, CompiledClassificationRule};
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

    fn candidates<'a>(
        &mut self,
        items: &'a [DesktopItem],
        times: &TemporaryTimes<'_>,
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

pub(super) struct TemporaryTimes<'a> {
    latest: HashMap<&'a Path, SystemTime>,
    wall_now: SystemTime,
}

impl<'a> TemporaryTimes<'a> {
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
        Self { latest, wall_now }
    }

    /// Zero means ready. Unknown timestamps and unfinished names wait for a later inventory.
    fn ready_in(&self, item: &DesktopItem) -> Option<Duration> {
        let Some(path) = item.identity().file_system_path() else {
            return Some(Duration::ZERO);
        };
        if download_or_lock(path) {
            return None;
        }
        if !is_tmp(path) {
            return Some(Duration::ZERO);
        }
        let ready_at = self.latest.get(path)?.checked_add(TMP_SETTLE)?;
        Some(ready_at.duration_since(self.wall_now).unwrap_or_default())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Move {
    pub(super) identity: ShellIdentity,
    pub(super) item_name: String,
    pub(super) target: PanelId,
    pub(super) target_name: String,
    pub(super) rule: usize,
}

pub(super) fn moves<'a>(
    workspace: &Workspace,
    items: impl IntoIterator<Item = &'a DesktopItem>,
    rules: &[CompiledClassificationRule<'_>],
    times: &TemporaryTimes<'_>,
) -> Vec<Move> {
    let targets: Vec<_> = rules
        .iter()
        .map(|rule| {
            workspace
                .panel(PanelId::new(rule.target_panel()))
                .filter(|panel| panel.supports_tabs())
        })
        .collect();
    items
        .into_iter()
        .filter(|item| unassigned(item) && times.ready_in(item) == Some(Duration::ZERO))
        .filter_map(|item| {
            let (index, target) =
                rules
                    .iter()
                    .zip(&targets)
                    .enumerate()
                    .find_map(|(index, (rule, target))| {
                        target
                            .filter(|_| rule.matches(item))
                            .map(|panel| (index, panel))
                    })?;
            Some(Move {
                identity: item.identity().clone(),
                item_name: item.display_name().into(),
                target: target.id(),
                target_name: target.title().into(),
                rule: index + 1,
            })
        })
        .collect()
}

pub(super) fn apply_moves(workspace: &mut Workspace, moves: &[Move]) {
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
    let times = TemporaryTimes::new(&session.snapshot.items, SystemTime::now());
    let automatic = &mut session.classification;
    automatic.set_enabled(config.enabled);
    let candidates = automatic.candidates(
        s.workspace.desktop_items(),
        &times,
        Instant::now(),
        desktop_renaming(session.view),
    );
    if candidates.is_empty() {
        return Ok(false);
    }
    let rules = config.compile().map_err(|error| error.to_string())?;
    let planned = moves(&s.workspace, candidates, &rules, &times);
    // Preserve deliberate releases even if they happen before the next inventory.
    for entry in &planned {
        let key = entry.identity.persistent_key();
        automatic.pending.remove(&key);
        if let Some(seen) = &mut automatic.seen { seen.insert(key); }
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
        let times = TemporaryTimes::new(&[], SystemTime::now());
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
        let times = TemporaryTimes::new(&[], SystemTime::now());
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
        let times = TemporaryTimes::new(&[], SystemTime::now());
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
            let times = TemporaryTimes::new(&[], SystemTime::now());
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
        let times = TemporaryTimes::new(&[], SystemTime::now());
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
            let times = TemporaryTimes::new(std::slice::from_ref(entry), wall + offset);
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
        let times = TemporaryTimes::new(&[], SystemTime::now());
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
}
