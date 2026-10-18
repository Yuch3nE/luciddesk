use std::collections::{BTreeMap, BTreeSet};
use super::diagnostics::Stage;
use windows::{
    Win32::{
        Foundation::POINT,
        System::Com::CoTaskMemFree,
        UI::Shell::{Common::ITEMIDLIST, *},
    },
    core::Result,
};

#[derive(Debug)]
pub struct ApplyError {
    pub error: windows::core::Error,
    pub before_write: bool,
    pub stage: Stage,
}
impl ApplyError {
    fn at(error: windows::core::Error, stage: Stage) -> Self {
        Self { error, before_write: false, stage }
    }
    pub fn code(&self) -> windows::core::HRESULT {
        self.error.code()
    }
}
impl From<windows::core::Error> for ApplyError {
    fn from(error: windows::core::Error) -> Self {
        Self {
            error,
            before_write: false,
            stage: Stage::Membership,
        }
    }
}

// The same transaction runs against COM in production and a fault-injecting
// view in tests. No test-only branch changes the recovery decisions.
trait View {
    fn snapshot(&mut self) -> Result<Vec<Item>>;
    fn exists(&mut self, name: &str) -> Result<bool>;
    fn add(&mut self, row: &Item) -> Result<()>;
    fn remove(&mut self, row: &Item) -> Result<()>;
    fn position(&mut self, rows: &[(*const ITEMIDLIST, POINT)]) -> Result<()>;
}
struct ShellView<'a> { folder: &'a IFolderView2, objects: &'a IShellFolderView }
impl View for ShellView<'_> {
    fn snapshot(&mut self) -> Result<Vec<Item>> { snapshot(self.folder) }
    fn exists(&mut self, name: &str) -> Result<bool> { exists(name) }
    fn add(&mut self, row: &Item) -> Result<()> { unsafe { self.objects.AddObject(row.pidl).map(|_| ()) } }
    fn remove(&mut self, row: &Item) -> Result<()> { unsafe { self.objects.RemoveObject(Some(row.pidl)).map(|_| ()) } }
    fn position(&mut self, rows: &[(*const ITEMIDLIST, POINT)]) -> Result<()> {
        let ids: Vec<_> = rows.iter().map(|(id, _)| *id).collect();
        let points: Vec<_> = rows.iter().map(|(_, point)| *point).collect();
        unsafe { self.folder.SelectAndPositionItems(ids.len() as u32, ids.as_ptr(), Some(points.as_ptr()), SVSI_POSITIONITEM.0 as u32) }
    }
}
impl From<windows::core::HRESULT> for ApplyError {
    fn from(error: windows::core::HRESULT) -> Self {
        windows::core::Error::from_hresult(error).into()
    }
}

pub struct Item {
    pub pidl: *mut ITEMIDLIST,
    pub name: String,
    pub position: POINT,
}
impl Drop for Item {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFree(Some(self.pidl.cast()));
        }
    }
}
pub fn key(name: &str) -> String {
    name.to_lowercase()
}

#[derive(PartialEq, Eq)]
struct RowRevision {
    id: Vec<u8>,
    position: (i32, i32),
}

fn changed_snapshot() -> windows::core::Error {
    windows::core::Error::new(
        windows::core::HRESULT::from_win32(windows::Win32::Foundation::ERROR_RETRY.0),
        "桌面过滤清单在读取期间变化，暂不写入",
    )
}

fn row_revision(row: &Item) -> Result<RowRevision> {
    if row.pidl.is_null() {
        return Err(changed_snapshot());
    }
    let bytes = unsafe {
        let size = ILGetSize(Some(row.pidl)) as usize;
        if size <= 2 {
            return Err(changed_snapshot());
        }
        std::slice::from_raw_parts(row.pidl.cast::<u8>(), size).to_vec()
    };
    Ok(RowRevision {
        id: bytes,
        position: (row.position.x, row.position.y),
    })
}

fn validate_snapshot(before: &[RowRevision], after: &[RowRevision]) -> Result<()> {
    let ids: BTreeSet<_> = before.iter().map(|row| &row.id).collect();
    if before != after || ids.len() != before.len() || before.iter().any(|row| row.id.is_empty()) {
        return Err(changed_snapshot());
    }
    Ok(())
}

pub fn snapshot(folder: &IFolderView2) -> Result<Vec<Item>> {
    unsafe {
        let parent: IShellFolder = folder.GetFolder()?;
        let mut result = Vec::new();
        for index in 0..folder.ItemCount(SVGIO_ALLVIEW)? {
            let mut row = Item {
                pidl: folder.Item(index)?,
                name: String::new(),
                position: POINT::default(),
            };
            if row.pidl.is_null() {
                return Err(changed_snapshot());
            }
            row.position = folder.GetItemPosition(row.pidl)?;
            let item: IShellItem = SHCreateItemWithParent(None, &parent, row.pidl)?;
            let raw = item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING)?;
            let name = raw.to_string();
            CoTaskMemFree(Some(raw.0.cast()));
            row.name = key(&name?);
            result.push(row);
        }
        // Names may invoke Shell extensions and reenter Explorer. Verify the
        // ordered PIDLs and positions again before using rows as a write plan
        // or restoration baseline. These reads never freeze or mutate the view.
        let before = result
            .iter()
            .map(row_revision)
            .collect::<Result<Vec<_>>>()?;
        let names: BTreeSet<_> = result.iter().map(|row| &row.name).collect();
        if names.len() != result.len() {
            return Err(changed_snapshot());
        }
        let count = folder.ItemCount(SVGIO_ALLVIEW)?;
        if usize::try_from(count).ok() != Some(result.len()) {
            return Err(changed_snapshot());
        }
        let mut after = Vec::with_capacity(result.len());
        for index in 0..count {
            let mut row = Item {
                pidl: folder.Item(index)?,
                name: String::new(),
                position: POINT::default(),
            };
            if row.pidl.is_null() {
                return Err(changed_snapshot());
            }
            row.position = folder.GetItemPosition(row.pidl)?;
            after.push(row_revision(&row)?);
        }
        if folder.ItemCount(SVGIO_ALLVIEW)? != count {
            return Err(changed_snapshot());
        }
        validate_snapshot(&before, &after)?;
        Ok(result)
    }
}

#[derive(Default)]
pub struct Membership {
    pub desired: BTreeSet<String>,
    hidden: BTreeMap<String, Item>,
    baseline: BTreeMap<String, POINT>,
    baseline_order: Vec<String>,
    user_layout_changed: bool,
}
impl Membership {
    fn needs_position_restore(&self, current: &[Item], added: bool) -> bool {
        // New rows may need their original auto-arrange order even when their
        // current coordinates happen to match. Otherwise leave equal positions alone.
        added
            || current.iter().any(|row| {
                self.baseline
                    .get(&row.name)
                    .is_some_and(|point| point.x != row.position.x || point.y != row.position.y)
            })
    }
    pub fn replace_identity(&mut self, old: &str, new: &str) -> Result<()> {
        let old = key(old);
        let new = key(new);
        if !self.desired.contains(&old) {
            return Ok(());
        }
        let mut pidl = std::ptr::null_mut();
        let wide: Vec<u16> = new.encode_utf16().chain(Some(0)).collect();
        unsafe {
            SHParseDisplayName(
                windows::core::PCWSTR(wide.as_ptr()),
                None,
                &raw mut pidl,
                0,
                None,
            )?;
        }
        let position = self
            .baseline
            .get(&old)
            .copied()
            .or_else(|| self.hidden.get(&old).map(|row| row.position))
            .unwrap_or_default();
        self.hidden.remove(&old);
        self.hidden.insert(
            new.clone(),
            Item {
                pidl,
                name: new.clone(),
                position,
            },
        );
        self.desired.remove(&old);
        self.desired.insert(new.clone());
        if let Some(position) = self.baseline.remove(&old) {
            self.baseline.insert(new.clone(), position);
        }
        for name in &mut self.baseline_order {
            if *name == old {
                *name = new.clone();
            }
        }
        Ok(())
    }
    pub fn apply(
        &mut self,
        folder: &IFolderView2,
        object_view: &IShellFolderView,
        paused: bool,
    ) -> std::result::Result<(), ApplyError> {
        self.apply_before_write(folder, object_view, paused, || {})
    }
    pub fn apply_before_write(
        &mut self,
        folder: &IFolderView2,
        object_view: &IShellFolderView,
        paused: bool,
        before_write: impl FnMut(),
    ) -> std::result::Result<(), ApplyError> {
        self.apply_view(&mut ShellView { folder, objects: object_view }, paused, before_write)
    }
    fn apply_view(
        &mut self,
        view: &mut impl View,
        paused: bool,
        mut before_write: impl FnMut(),
    ) -> std::result::Result<(), ApplyError> {
        if self.desired.is_empty() && self.hidden.is_empty() && self.baseline.is_empty() {
            return Ok(());
        }
        let mut current = view.snapshot().map_err(|error| ApplyError {
            error,
            before_write: true,
            stage: Stage::Snapshot,
        })?;

        let layout: BTreeMap<_, _> = current
            .iter()
            .map(|row| (row.name.clone(), row.position))
            .collect();
        if self.baseline.is_empty() && !self.desired.is_empty() && !paused {
            self.baseline_order = current.iter().map(|row| row.name.clone()).collect();
            self.baseline = layout;
            self.user_layout_changed = false;
        }
        let release: Vec<_> = self
            .hidden
            .keys()
            .filter(|name| paused || !self.desired.contains(*name))
            .cloned()
            .collect();
        let mut added = false;
        for name in &release {
            let row = &self.hidden[name];
            if !current.iter().any(|item| &item.name == name)
                && view.exists(&row.name).map_err(|e| ApplyError::at(e, Stage::Existence))? {
                before_write();
                view.add(row).map_err(|e| ApplyError::at(e, Stage::Add))?;
                added = true;
            }
        }
        if added {
            current = view.snapshot().map_err(|e| ApplyError::at(e, Stage::VerifyAdd))?;
        }
        // Do not discard recovery PIDLs merely because AddObject returned S_OK.
        // Keep the entire release set until all surviving targets are observed.
        for name in &release {
            if !current.iter().any(|row| &row.name == name)
                && view.exists(name).map_err(|e| ApplyError::at(e, Stage::Existence))? {
                return Err(ApplyError::at(changed_snapshot(), Stage::VerifyAdd));
            }
        }
        for name in release {
            self.hidden.remove(&name);
        }
        // Restore the initial layout only while the user has not moved/sorted it.
        // Native changes during the session take precedence over our old baseline.
        if self.hidden.is_empty()
            && (paused || self.desired.is_empty())
            && !self.baseline.is_empty()
            && !self.user_layout_changed
            && self.needs_position_restore(&current, added)
        {
            // With auto-arrange enabled, the batch order influences insertion.
            // Reapply in the original view order, not the order AddObject produced.
            let positioned: Vec<_> = self
                .baseline_order
                .iter()
                .filter_map(|name| {
                    let row = current.iter().find(|row| &row.name == name)?;
                    self.baseline
                        .get(name)
                        .map(|position| (row.pidl.cast_const(), *position))
                })
                .collect();
            if !positioned.is_empty() {
                before_write();
                view.position(&positioned).map_err(|e| ApplyError::at(e, Stage::Position))?;
            }
        }
        if !paused {
            let mut removed = BTreeSet::new();
            // Remove by PIDL, never by the indices invalidated by earlier removals.
            for row in current
                .into_iter()
                .rev()
                .filter(|row| self.desired.contains(&row.name))
            {
                before_write();
                // Record first: even a failing COM call may have changed the view.
                let name = row.name.clone();
                let mut row = row;
                if let Some(previous) = self.hidden.get(&name) { row.position = previous.position; }
                self.hidden.insert(name.clone(), row);
                view.remove(&self.hidden[&name]).map_err(|e| ApplyError::at(e, Stage::Remove))?;
                removed.insert(name);
            }
            if !removed.is_empty() {
                let observed = view.snapshot().map_err(|e| ApplyError::at(e, Stage::VerifyRemove))?;
                if observed.iter().any(|row| removed.contains(&row.name)) {
                    return Err(ApplyError::at(changed_snapshot(), Stage::VerifyRemove));
                }
            }
        }
        if !paused && self.desired.is_empty() {
            self.baseline.clear();
            self.baseline_order.clear();
            self.user_layout_changed = false;
        }
        Ok(())
    }
    pub fn restore(&mut self, folder: &IFolderView2, object_view: &IShellFolderView) -> std::result::Result<(), ApplyError> {
        self.desired.clear();
        self.apply(folder, object_view, false)
    }
    pub fn user_changed_layout(&mut self) {
        self.user_layout_changed = true;
    }
}

fn exists(name: &str) -> Result<bool> {
    if name.starts_with("::{") {
        return Ok(true);
    }
    // No file content is opened. A deleted/renamed file must never be resurrected
    // as a stale Shell item when its membership is released.
    let wide: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let attributes = windows_sys::Win32::Storage::FileSystem::GetFileAttributesW(wide.as_ptr());
        if attributes != windows_sys::Win32::Storage::FileSystem::INVALID_FILE_ATTRIBUTES {
            Ok(true)
        } else {
            missing_or_error(windows_sys::Win32::Foundation::GetLastError())
        }
    }
}

fn missing_or_error(code: u32) -> Result<bool> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
    match code {
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        _ => Err(windows::core::Error::from_hresult(if code == 0 {
            windows::Win32::Foundation::E_FAIL
        } else { windows::core::HRESULT::from_win32(code) })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FaultView {
        visible: BTreeSet<String>,
        missing: BTreeSet<String>,
        unknown: bool,
        add_error: bool,
        add_noop: bool,
        remove_error_after_write: bool,
        remove_noop: bool,
        reads: usize,
        fail_read: Option<usize>,
    }
    fn row(name: &str) -> Item {
        Item { name: name.into(), pidl: std::ptr::null_mut(), position: POINT::default() }
    }
    impl View for FaultView {
        fn snapshot(&mut self) -> Result<Vec<Item>> {
            self.reads += 1;
            if self.fail_read == Some(self.reads) { return Err(windows::Win32::Foundation::E_FAIL.into()); }
            Ok(self.visible.iter().map(|name| row(name)).collect())
        }
        fn exists(&mut self, name: &str) -> Result<bool> {
            if self.unknown { return Err(windows::Win32::Foundation::E_ACCESSDENIED.into()); }
            Ok(!self.missing.contains(name))
        }
        fn add(&mut self, row: &Item) -> Result<()> {
            if self.add_error { return Err(windows::Win32::Foundation::E_FAIL.into()); }
            if !self.add_noop { self.visible.insert(row.name.clone()); }
            Ok(())
        }
        fn remove(&mut self, row: &Item) -> Result<()> {
            if !self.remove_noop { self.visible.remove(&row.name); }
            if self.remove_error_after_write { return Err(windows::Win32::Foundation::E_FAIL.into()); }
            Ok(())
        }
        fn position(&mut self, _: &[(*const ITEMIDLIST, POINT)]) -> Result<()> { Ok(()) }
    }
    fn hidden() -> Membership {
        let mut membership = Membership::default();
        membership.hidden.insert("a".into(), row("a"));
        membership
    }
    #[test]
    fn existence_only_discards_confirmed_missing_files() {
        for code in [2, 3] { assert!(!missing_or_error(code).unwrap()); }
        for code in [5, 32, 53, 64, 121, 0] { assert!(missing_or_error(code).is_err()); }
        let mut membership = hidden();
        let mut view = FaultView { unknown: true, ..Default::default() };
        let error = membership.apply_view(&mut view, false, || {}).unwrap_err();
        assert_eq!(error.stage, Stage::Existence);
        assert!(membership.hidden.contains_key("a"));
        view.unknown = false;
        view.missing.insert("a".into());
        membership.apply_view(&mut view, false, || {}).unwrap();
        assert!(membership.hidden.is_empty());
        assert!(view.visible.is_empty());
    }
    #[test]
    fn failed_or_noop_add_retains_pidl_until_presence_is_verified() {
        for noop in [false, true] {
            let mut membership = hidden();
            let mut view = FaultView { add_error: !noop, add_noop: noop, ..Default::default() };
            let error = membership.apply_view(&mut view, false, || {}).unwrap_err();
            assert_eq!(error.stage, if noop { Stage::VerifyAdd } else { Stage::Add });
            assert!(membership.hidden.contains_key("a"));
            // Delayed Shell enumeration or Refresh eventually exposes the item.
            view.visible.insert("a".into());
            membership.apply_view(&mut view, false, || {}).unwrap();
            assert!(membership.hidden.is_empty());
        }
    }
    #[test]
    fn failed_remove_after_mutation_remains_recoverable() {
        let mut membership = Membership::default();
        membership.desired.insert("a".into());
        let mut view = FaultView { visible: ["a".into(), "unrelated".into()].into(), remove_error_after_write: true, ..Default::default() };
        let error = membership.apply_view(&mut view, false, || {}).unwrap_err();
        assert_eq!(error.stage, Stage::Remove);
        assert!(!error.before_write);
        assert!(!view.visible.contains("a"));
        assert!(membership.hidden.contains_key("a"));
        membership.desired.clear();
        membership.apply_view(&mut view, false, || {}).unwrap();
        assert!(view.visible.contains("a"));
        assert!(view.visible.contains("unrelated"));
        assert!(membership.hidden.is_empty());
    }
    #[test]
    fn successful_noop_remove_is_not_acknowledged_as_filtered() {
        let mut membership = Membership::default();
        membership.desired.insert("a".into());
        let mut view = FaultView { visible: ["a".into()].into(), remove_noop: true, ..Default::default() };
        let error = membership.apply_view(&mut view, false, || {}).unwrap_err();
        assert_eq!(error.stage, Stage::VerifyRemove);
        assert!(membership.hidden.contains_key("a"));
        membership.desired.clear();
        membership.apply_view(&mut view, false, || {}).unwrap();
        assert!(membership.hidden.is_empty());
        assert!(view.visible.contains("a"));
    }
    #[test]
    fn read_failure_after_add_retains_release_records() {
        let mut membership = hidden();
        let mut view = FaultView { fail_read: Some(2), ..Default::default() };
        let error = membership.apply_view(&mut view, false, || {}).unwrap_err();
        assert_eq!(error.stage, Stage::VerifyAdd);
        assert!(!error.before_write);
        assert!(membership.hidden.contains_key("a"));
        assert!(view.visible.contains("a"));
        membership.apply_view(&mut view, false, || {}).unwrap();
        assert!(membership.hidden.is_empty());
    }
    #[test]
    fn batch_validation_allows_unrelated_items_and_preserves_other_members() {
        let mut membership = hidden();
        membership.hidden.insert("b".into(), row("b"));
        membership.desired.insert("b".into());
        let mut view = FaultView { visible: ["unrelated".into()].into(), ..Default::default() };
        membership.apply_view(&mut view, false, || {}).unwrap();
        assert!(!membership.hidden.contains_key("a"));
        assert!(membership.hidden.contains_key("b"));
        assert_eq!(view.visible, ["a".into(), "unrelated".into()].into());
    }
    #[test]
    fn unchanged_layout_does_not_reposition_but_insertions_keep_original_order() {
        let mut membership = Membership::default();
        membership
            .baseline
            .insert("one".into(), POINT { x: 10, y: 20 });
        let mut rows = vec![Item {
            pidl: std::ptr::null_mut(),
            name: "one".into(),
            position: POINT { x: 10, y: 20 },
        }];
        assert!(!membership.needs_position_restore(&rows, false));
        assert!(membership.needs_position_restore(&rows, true));
        rows[0].position.x += 1;
        assert!(membership.needs_position_restore(&rows, false));
        rows[0].name = "unrelated".into();
        assert!(!membership.needs_position_restore(&rows, false));
    }
    #[test]
    #[ignore = "Read-only live Explorer snapshot; requires an interactive desktop"]
    fn live_filter_snapshot_reads_without_mutating_desktop() {
        use windows::{
            Win32::System::{
                Com::{
                    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                    CoUninitialize, IServiceProvider,
                },
                Variant::VARIANT,
            },
            core::Interface,
        };
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap();
        }
        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        let _apartment = Apartment;
        unsafe {
            let shell: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).unwrap();
            let mut raw = 0;
            let dispatch = shell
                .FindWindowSW(
                    &VARIANT::from(CSIDL_DESKTOP.cast_signed()),
                    &VARIANT::default(),
                    SWC_DESKTOP,
                    &raw mut raw,
                    SWFO_NEEDDISPATCH,
                )
                .unwrap();
            let provider: IServiceProvider = dispatch.cast().unwrap();
            let browser: IShellBrowser = provider.QueryService(&SID_STopLevelBrowser).unwrap();
            let folder: IFolderView2 = browser.QueryActiveShellView().unwrap().cast().unwrap();
            let rows = snapshot(&folder).unwrap();
            assert!(
                rows.iter()
                    .all(|row| !row.pidl.is_null() && !row.name.is_empty())
            );
        }
    }
    #[test]
    fn filter_snapshot_rejects_partial_reordered_replaced_and_moved_rows() {
        let rows = |ids: &[u8]| {
            ids.iter()
                .map(|id| RowRevision {
                    id: vec![*id],
                    position: (10, 20),
                })
                .collect::<Vec<_>>()
        };
        let before = rows(&[1, 2]);
        assert!(validate_snapshot(&before, &rows(&[1, 2])).is_ok());
        for ids in [&[1][..], &[1, 2, 3], &[2, 1], &[1, 3]] {
            assert!(validate_snapshot(&before, &rows(ids)).is_err());
        }
        let mut moved = rows(&[1, 2]);
        moved[1].position.0 += 1;
        assert!(validate_snapshot(&before, &moved).is_err());
        assert!(validate_snapshot(&rows(&[1, 1]), &rows(&[1, 1])).is_err());
        assert!(validate_snapshot(&[], &[]).is_ok());
        let invalid = vec![RowRevision {
            id: Vec::new(),
            position: (0, 0),
        }];
        assert!(validate_snapshot(&invalid, &invalid).is_err());
    }
    #[test]
    fn renamed_membership_uses_new_pidl_and_keeps_original_restore_position() {
        use windows::Win32::System::Com::{
            COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize,
        };
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap();
        }
        let root = std::env::temp_dir().join(format!(
            "luciddesk-membership-rename-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let before = root.join("before.txt");
        let after = root.join("after.txt");
        std::fs::write(&before, b"owned rename regression").unwrap();
        {
            let old = key(&before.to_string_lossy());
            let new = key(&after.to_string_lossy());
            let mut membership = Membership::default();
            membership.desired.insert(old.clone());
            membership
                .baseline
                .insert(old.clone(), POINT { x: 120, y: 240 });
            membership.baseline_order.push(old.clone());
            std::fs::rename(&before, &after).unwrap();
            membership.replace_identity(&old, &new).unwrap();
            assert!(!membership.desired.contains(&old));
            assert!(membership.desired.contains(&new));
            assert_eq!(membership.baseline_order, vec![new.clone()]);
            assert_eq!(membership.baseline[&new].x, 120);
            assert_eq!(membership.hidden[&new].position.y, 240);
            assert!(!membership.hidden[&new].pidl.is_null());
            let item: IShellItem =
                unsafe { SHCreateItemFromIDList(membership.hidden[&new].pidl).unwrap() };
            let raw = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH).unwrap() };
            let actual = unsafe { raw.to_string().unwrap() };
            unsafe {
                CoTaskMemFree(Some(raw.0.cast()));
            }
            assert_eq!(key(&actual), new);
            assert!(
                membership
                    .replace_identity(&new, &root.join("missing.txt").to_string_lossy())
                    .is_err()
            );
            assert!(membership.desired.contains(&new));
        }
        std::fs::remove_file(after).unwrap();
        std::fs::remove_dir(root).unwrap();
        unsafe {
            CoUninitialize();
        }
    }
}
