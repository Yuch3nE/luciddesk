//! Explicit ordinary pane sorting with asynchronous file metadata reads.
use super::*;

pub(super) fn apply(workspace: &mut Workspace, id: PanelId, descending: bool) -> Result<bool, String> {
    apply_order(workspace, id, descending, None)
}

pub(super) fn apply_order(workspace: &mut Workspace, id: PanelId, descending: bool, ranks: Option<&HashMap<String, usize>>) -> Result<bool, String> {
    let panel = workspace.panel(id).ok_or("panel does not exist")?;
    if !panel.supports_tabs() { return Err("only desktop panels support name sorting".into()); }
    if panel.locked() { return Err("panel is locked".into()); }

    struct Entry {
        index: usize,
        name: Vec<u16>,
        key: String,
        position: (u32, u32),
    }
    let mut entries: Vec<_> = workspace.desktop_items().iter().enumerate()
        .filter_map(|(index, item)| match item.placement() {
            DesktopPlacement::Pane { pane_id, position } if *pane_id == id => Some(Entry {
                index,
                name: item.display_name().encode_utf16().chain(Some(0)).collect(),
                key: item.identity().persistent_key(),
                position: (position.row, position.column),
            }),
            _ => None,
        }).collect();
    entries.sort_unstable_by(|a, b| {
        if let Some(ranks) = ranks { return ranks[&a.key].cmp(&ranks[&b.key]); }
        let cmp = unsafe { windows_sys::Win32::UI::Shell::StrCmpLogicalW(a.name.as_ptr(), b.name.as_ptr()) }.cmp(&0)
            .then_with(|| a.name.cmp(&b.name)).then_with(|| a.key.cmp(&b.key));
        if descending { cmp.reverse() } else { cmp }
    });
    // The desired order is unchanged exactly when it is also ordered by the
    // existing grid positions (identity breaks ties). Preserve sparse grids on no-op.
    let fixed = panel.fixed_grid();
    let columns = fixed_grid::visible_columns(workspace, id);
    let free = workspace.panel(id).is_some_and(Panel::free_layout);
    let metrics = free_layout::grid(workspace);
    if (!fixed || entries.iter().enumerate().all(|(at, e)| {
        let p = fixed_grid::position(at, columns);
        e.position == (p.row, p.column) && (!free || workspace.desktop_items()[e.index].pane_position() == Some(luciddesk_core::PointDip::new(p.column as f32 * metrics.cell_width, p.row as f32 * metrics.cell_height)))
    })) && entries.windows(2).all(|pair| {
        (pair[0].position, &pair[0].key) <= (pair[1].position, &pair[1].key)
    }) {
        return Ok(false);
    }
    // The item slice has not changed; use its indices instead of hashing every
    // desktop identity again and scanning unrelated panes to apply the result.
    let items = workspace.desktop_items_mut();
    for (at, entry) in entries.into_iter().enumerate() {
        fixed_grid::set_position(&mut items[entry.index], id, if fixed { fixed_grid::position(at, columns) } else { GridPosition::new(at as u32, 0) }, free, metrics);
    }
    Ok(true)
}

/// Snapshots avoid repeating metadata work and database writes on idle wakes.
#[derive(Default)]
pub(super) struct State {
    pub orders: Rc<RefCell<HashMap<PanelId, (u8, bool)>>>,
    pending: HashMap<PanelId, Pending>,
    observed: HashMap<PanelId, Arc<HashMap<ShellIdentity, luciddesk_core::DesktopItem>>>,
    checked_revision: Option<u64>,
}
struct Pending {
    before: Arc<HashMap<ShellIdentity, luciddesk_core::DesktopItem>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    panel: Panel,
    receiver: mpsc::Receiver<Vec<Item>>,
    order: (u8, bool),
}

impl Drop for Pending {
    fn drop(&mut self) { self.cancelled.store(true, std::sync::atomic::Ordering::Relaxed); }
}

pub(super) fn next_order(previous: Option<(u8, bool)>, column: u8) -> (u8, bool) {
    (column, match previous {
        Some((old, descending)) if old == column => !descending,
        _ => column == 2,
    })
}

pub(super) fn request(state: &mut PaneApp, id: PanelId, column: u8) -> Result<(), String> {
    if column > 3 { return Err("invalid sort column".into()); }
    let panel = state.workspace.panel(id).ok_or("panel does not exist")?;
    if !panel.supports_tabs() || panel.locked() { return Err("only unlocked desktop panels can be sorted".into()); }
    let previous = state.sorting.pending.get(&id).map(|p| p.order)
        .or_else(|| state.sorting.orders.borrow().get(&id).copied());
    let order = next_order(previous, column);
    // Reuse a metadata request for repeated clicks; stale inventory is still
    // rejected by poll before any result is applied.
    if let Some(pending) = state.sorting.pending.get_mut(&id) {
        pending.order = order;
        pending.panel = state.workspace.panel(id).unwrap().clone();
        return Ok(());
    }
    start(state, id, order)
}

fn observe(state: &mut PaneApp, id: PanelId) -> Arc<HashMap<ShellIdentity, luciddesk_core::DesktopItem>> {
    let items = state.workspace.desktop_items().iter()
        .filter(|i| matches!(i.placement(), DesktopPlacement::Pane { pane_id, .. } if *pane_id == id))
        .map(|i| (i.identity().clone(), i.clone())).collect();
    let items = Arc::new(items);
    state.sorting.observed.insert(id, Arc::clone(&items));
    items
}

fn start(state: &mut PaneApp, id: PanelId, order: (u8, bool)) -> Result<(), String> {
    if order.0 == 0 {
        let before = state.workspace.clone();
        if apply(&mut state.workspace, id, order.1)? {
            if let Err(error) = save(state) { state.workspace = before; return Err(error); }
            refresh_views(state);
        }
        state.sorting.orders.borrow_mut().insert(id, order);
        observe(state, id);
        return Ok(());
    }
    let panel = state.workspace.panel(id).ok_or("panel does not exist")?.clone();
    let before = observe(state, id);
    let inventory = Arc::clone(&before);
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancel = Arc::clone(&cancelled);
    let wake = state.wake.clone();
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new().name("pane-sort".into()).spawn(move || {
        let _sta = luciddesk_shell::ShellApartment::initialize_sta().ok();
        let Some(items) = read_items(&inventory, &cancel, file_details) else { return; };
        let _ = sender.send(items);
        wake.notify();
    }).map_err(|e| e.to_string())?;
    state.sorting.pending.insert(id, Pending { before, panel, receiver, order, cancelled });
    Ok(())
}

fn read_items(inventory: &HashMap<ShellIdentity, luciddesk_core::DesktopItem>, cancel: &std::sync::atomic::AtomicBool,
    mut details: impl FnMut(&ShellIdentity) -> ItemDetails) -> Option<Vec<Item>> {
    let mut items = Vec::with_capacity(inventory.len());
    for item in inventory.values() {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) { return None; }
        let detail = details(item.identity());
        if cancel.load(std::sync::atomic::Ordering::Relaxed) { return None; }
        items.push(Item { identity: item.identity().clone(), label: item.display_name().to_owned(), image: None, details: detail });
    }
    Some(items)
}

/// Called by refresh after a committed change, not on every runtime wake.
pub(super) fn maintain(state: &mut PaneApp) {
    let revision = state.store.change_count();
    if state.sorting.checked_revision == Some(revision) { return; }
    state.sorting.checked_revision = Some(revision);
    let expired: Vec<_> = state.sorting.pending.iter().filter_map(|(&id, pending)|
        (state.workspace.panel(id) != Some(&pending.panel) || !snapshot_matches(&state.workspace, id, &pending.before)).then_some(id)).collect();
    for id in expired { invalidate(&mut state.sorting, id); }
    state.sorting.observed.retain(|id, items| {
        if state.sorting.pending.contains_key(id) && state.workspace.panel(*id).is_some() { return true; }
        let unchanged = state.workspace.panel(*id).is_some() && snapshot_matches(&state.workspace, *id, items);
        if !unchanged { state.sorting.orders.borrow_mut().remove(id); }
        unchanged
    });
}

fn snapshot_matches(workspace: &Workspace, id: PanelId, snapshot: &HashMap<ShellIdentity, luciddesk_core::DesktopItem>) -> bool {
    let mut count = 0;
    workspace.desktop_items().iter().filter(|item| matches!(item.placement(), DesktopPlacement::Pane { pane_id, .. } if *pane_id == id))
        .all(|item| { count += 1; snapshot.get(item.identity()) == Some(item) }) && count == snapshot.len()
}

fn invalidate(state: &mut State, id: PanelId) {
    state.pending.remove(&id);
    state.orders.borrow_mut().remove(&id);
    state.observed.remove(&id);
}

/// Commit a successful manual move and its sorting transition atomically.
pub(super) fn finish_move(state: &mut PaneApp, before: Workspace, source: PanelId, target: PanelId) -> Result<(), String> {
    if state.workspace.desktop_items() == before.desktop_items() { return Ok(()); }
    if let Err(error) = save(state) { state.workspace = before; return Err(error); }
    for id in [source, target] {
        invalidate(&mut state.sorting, id);
    }
    state.wake.notify();
    Ok(())
}

pub(super) fn file_details(identity: &ShellIdentity) -> ItemDetails {
    let ShellIdentity::FileSystem { path, .. } = identity else { return ItemDetails::default(); };
    let Ok(metadata) = std::fs::symlink_metadata(path) else { return ItemDetails::default(); };
    let mut details = ItemDetails {
        folder: metadata.is_dir(), modified_time: metadata.modified().ok(),
        size: (!metadata.is_dir()).then_some(metadata.len()), ..Default::default()
    };
    // Ask for the file's own registered type; never resolve shortcut targets.
    use windows_sys::Win32::{Storage::FileSystem::*, UI::Shell::*};
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut info = SHFILEINFOW::default();
    let attributes = if details.folder { FILE_ATTRIBUTE_DIRECTORY } else { FILE_ATTRIBUTE_NORMAL };
    if unsafe { SHGetFileInfoW(wide.as_ptr(), attributes, &raw mut info, size_of::<SHFILEINFOW>() as u32,
        SHGFI_TYPENAME | SHGFI_USEFILEATTRIBUTES) } != 0 {
        let end = info.szTypeName.iter().position(|c| *c == 0).unwrap_or(info.szTypeName.len());
        details.kind = String::from_utf16_lossy(&info.szTypeName[..end]);
    }
    details
}

pub(super) fn poll(state: &mut PaneApp) {
    let ready: Vec<_> = state.sorting.pending.iter().filter_map(|(&id, pending)| {
        match pending.receiver.try_recv() {
            Ok(items) => Some((id, Some(items))),
            Err(mpsc::TryRecvError::Disconnected) => Some((id, None)),
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }).collect();
    for (id, items) in ready {
        let pending = state.sorting.pending.remove(&id).unwrap();
        let Some(mut items) = items else { invalidate(&mut state.sorting, id); continue; };
        if state.workspace.panel(id) != Some(&pending.panel) { invalidate(&mut state.sorting, id); continue; }
        if !snapshot_matches(&state.workspace, id, &pending.before) { invalidate(&mut state.sorting, id); continue; }
        folder::sort_items(&mut items, pending.order);
        let ranks = items.iter().enumerate().map(|(i, item)| (item.identity.persistent_key(), i)).collect();
        let previous = state.workspace.clone();
        let applied = if pending.order.0 == 0 { apply(&mut state.workspace, id, pending.order.1) }
            else { apply_order(&mut state.workspace, id, false, Some(&ranks)) };
        let result = applied.and_then(|changed| {
            if changed { save(state)?; refresh_views(state); }
            Ok(())
        });
        if let Err(error) = result {
            state.workspace = previous;
            luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "pane.sort", &error);
            window::defer_action(move || window::error(&error));
        } else { state.sorting.orders.borrow_mut().insert(id, pending.order); observe(state, id); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn finish(state: &mut PaneApp) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !state.sorting.pending.is_empty() {
            poll(state);
            assert!(std::time::Instant::now() < deadline, "sort worker timed out");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    fn names(state: &PaneApp) -> Vec<String> {
        ordered_desktop_items(&state.workspace, PanelId::new(1)).iter().map(|i| i.display_name().to_owned()).collect()
    }
    #[test]
    fn cancellation_stops_before_reading_another_file() {
        let mut state = super::super::tests::test_state();
        let id = PanelId::new(1);
        let snapshot = observe(&mut state, id);
        assert!(Arc::ptr_eq(&snapshot, &state.sorting.observed[&id]));
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut reads = 0;
        let result = read_items(&snapshot, &cancel, |_| {
            reads += 1;
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            ItemDetails::default()
        });
        assert!(result.is_none());
        assert_eq!(reads, 1);
    }

    #[test]
    fn metadata_reader_handles_precancel_completion_and_empty_inventory() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let mut state = super::super::tests::test_state();
        let snapshot = observe(&mut state, PanelId::new(1));
        let cancel = AtomicBool::new(true);
        assert!(read_items(&snapshot, &cancel, |_| panic!("cancelled work must not read metadata")).is_none());
        cancel.store(false, Ordering::Relaxed);
        let mut reads = 0;
        let result = read_items(&snapshot, &cancel, |_| {
            reads += 1;
            ItemDetails { size: Some(123), ..Default::default() }
        }).unwrap();
        assert_eq!(reads, snapshot.len());
        assert_eq!(result.len(), snapshot.len());
        for item in result {
            assert_eq!(item.label, snapshot[&item.identity].display_name());
            assert_eq!(item.details.size, Some(123));
            assert!(item.image.is_none());
        }
        assert!(read_items(&HashMap::new(), &cancel, |_| panic!("empty inventory")).unwrap().is_empty());
    }

    #[test]
    fn committed_change_cancels_pending_work_without_waiting_for_a_result() {
        for change_panel in [false, true] {
            let mut state = super::super::tests::test_state();
            let id = PanelId::new(1);
            let before = observe(&mut state, id);
            let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (sender, receiver) = mpsc::channel();
            state.sorting.orders.borrow_mut().insert(id, (3, false));
            state.sorting.pending.insert(id, Pending { before, panel: state.workspace.panel(id).unwrap().clone(),
                receiver, order: (3, false), cancelled: Arc::clone(&cancel) });
            maintain(&mut state);
            assert!(!cancel.load(std::sync::atomic::Ordering::Relaxed));
            if change_panel { state.workspace.panel_mut(id).unwrap().set_locked(true); }
            else { state.workspace.desktop_items_mut()[0].set_display_name("Changed"); }
            save(&mut state).unwrap();
            let writes = state.store.change_count();
            let items = state.workspace.desktop_items().to_vec();
            maintain(&mut state);
            assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));
            assert!(sender.send(Vec::new()).is_err(), "stale result channel must be released");
            assert!(state.sorting.pending.is_empty());
            assert!(state.sorting.observed.is_empty());
            assert!(state.sorting.orders.borrow().is_empty());
            poll(&mut state);
            assert_eq!(state.store.change_count(), writes);
            assert_eq!(state.workspace.desktop_items(), items);
        }
    }

    #[test]
    fn snapshot_comparison_ignores_inventory_order_but_detects_removal() {
        let mut state = super::super::tests::test_state();
        let id = PanelId::new(1);
        let snapshot = observe(&mut state, id);
        state.workspace.desktop_items_mut().reverse();
        assert!(snapshot_matches(&state.workspace, id, &snapshot));
        let identity = state.workspace.desktop_items()[0].identity().clone();
        state.workspace.desktop_item_mut(&identity).unwrap().set_placement(DesktopPlacement::Pane {
            pane_id: PanelId::new(999), position: GridPosition::new(0, 0),
        });
        assert!(!snapshot_matches(&state.workspace, id, &snapshot));
    }

    #[test]
    fn stale_worker_clears_old_rule_and_later_committed_changes_invalidate_new_rule() {
        let mut state = super::super::tests::test_state();
        let id = PanelId::new(1);
        request(&mut state, id, 0).unwrap();
        let (sender, receiver) = mpsc::channel();
        let before = observe(&mut state, id);
        state.sorting.pending.insert(id, Pending { before, panel: state.workspace.panel(id).unwrap().clone(), receiver, order: (3, false), cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)) });
        state.workspace.desktop_items_mut()[0].set_display_name("Renamed during sorting");
        sender.send(Vec::new()).unwrap();
        poll(&mut state);
        assert!(!state.sorting.orders.borrow().contains_key(&id));
        assert!(!state.sorting.observed.contains_key(&id));
        request(&mut state, id, 0).unwrap();
        let writes = state.store.change_count();
        for _ in 0..20 { super::super::refresh_views(&mut state); }
        assert_eq!(state.store.change_count(), writes);
        assert_eq!(state.sorting.orders.borrow().get(&id), Some(&(0, false)));
        let identity = ordered_desktop_items(&state.workspace, id)[0].identity().clone();
        state.workspace.desktop_item_mut(&identity).unwrap().set_display_name("Changed after sorting");
        state.store.save_workspace(&state.workspace).unwrap();
        super::super::refresh_views(&mut state);
        assert!(!state.sorting.orders.borrow().contains_key(&id));
    }

    #[test]
    fn explicit_sort_uses_visible_columns_and_preserves_manual_mode() {
        for free in [false, true] {
            let mut state = super::super::tests::test_state();
            let id = PanelId::new(1);
            let g = free_layout::grid(&state.workspace);
            let mut rect = state.workspace.panel(id).unwrap().rect();
            rect.width = layout::PADDING * 2.0 + g.cell_width * 2.0;
            state.workspace.panel_mut(id).unwrap().set_rect(rect);
            state.workspace.panel_mut(id).unwrap().set_fixed_grid(true);
            state.workspace.panel_mut(id).unwrap().set_free_layout(free);
            for (index, item) in state.workspace.desktop_items_mut().iter_mut().enumerate() {
                fixed_grid::set_position(item, id, GridPosition::new((index * 4) as u32, 0), free, g);
            }
            assert!(fixed_grid::columns(&state.workspace, id) > 2);
            assert_eq!(fixed_grid::visible_columns(&state.workspace, id), 2);
            assert!(apply(&mut state.workspace, id, false).unwrap());
            for (index, item) in ordered_desktop_items(&state.workspace, id).iter().enumerate() {
                assert_eq!(item.placement(), &DesktopPlacement::Pane { pane_id: id, position: fixed_grid::position(index, 2) });
            }
            assert_eq!(state.workspace.panel(id).unwrap().free_layout(), free);
            assert!(state.workspace.panel(id).unwrap().fixed_grid());
            assert!(!apply(&mut state.workspace, id, false).unwrap());
        }
    }

    #[test]
    fn clicks_toggle_only_the_same_field_and_remain_one_shot() {
        for column in 0..4 {
            assert_eq!(next_order(None, column), (column, column == 2));
            assert_eq!(next_order(Some((column, false)), column), (column, true));
            assert_eq!(next_order(Some((column, true)), column), (column, false));
            assert_eq!(next_order(Some(((column + 1) % 4, true)), column), (column, column == 2));
        }
        let mut state = super::super::tests::test_state();
        let id = PanelId::new(1);
        request(&mut state, id, 0).unwrap();
        let ascending = names(&state);
        request(&mut state, id, 0).unwrap();
        assert_eq!(names(&state), ascending.into_iter().rev().collect::<Vec<_>>());
        assert!(!state.workspace.panel(id).unwrap().fixed_grid());
    }
    #[test]
    fn async_size_sort_uses_shortcut_file_and_discards_changed_snapshot() {
        let _sta = crate::pane::test_support::apartment();
        let root = tempfile::tempdir().unwrap();
        let mut state = super::super::tests::test_state();
        let id = PanelId::new(1);
        let mut items = Vec::new();
        for (index, (name, size)) in [("Large.lnk", 100), ("Small.txt", 2), ("Medium.txt", 10)].into_iter().enumerate() {
            let path = root.path().join(name);
            std::fs::write(&path, vec![0; size]).unwrap();
            let identity = ShellIdentity::FileSystem { path, volume_id: None, file_id: None };
            assert_eq!(file_details(&identity).size, Some(size as u64));
            let mut item = luciddesk_core::DesktopItem::new(identity, name);
            item.set_placement(DesktopPlacement::Pane { pane_id: id, position: GridPosition::new(index as u32, 0) });
            items.push(item);
        }
        state.workspace.reconcile_desktop_items(items);
        request(&mut state, id, 3).unwrap();
        finish(&mut state);
        assert_eq!(names(&state), ["Small.txt", "Medium.txt", "Large.lnk"]);
        request(&mut state, id, 3).unwrap();
        finish(&mut state);
        assert_eq!(names(&state), ["Large.lnk", "Medium.txt", "Small.txt"]);
        std::fs::write(root.path().join("Small.txt"), vec![0; 200]).unwrap();
        let writes = state.store.change_count();
        for _ in 0..20 { maintain(&mut state); poll(&mut state); }
        assert_eq!(state.store.change_count(), writes);
        assert_eq!(names(&state), ["Large.lnk", "Medium.txt", "Small.txt"]);
        let before = state.workspace.clone();
        request(&mut state, id, 2).unwrap();
        state.workspace.panel_mut(id).unwrap().set_locked(true);
        finish(&mut state);
        assert_eq!(state.workspace.desktop_items(), before.desktop_items());
        assert_eq!(state.sorting.orders.borrow().get(&id), None);
    }
}
