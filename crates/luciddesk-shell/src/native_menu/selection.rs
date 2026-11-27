//! Stable Shell identities for the research menu target and selection restoration.

// Diagnostics must never panic across a Windows callback when output is unavailable.
macro_rules! trace {
    ($($arg:tt)*) => {
        luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Trace, "shell.menu", $($arg)*);
    };
}
use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    sync::{Mutex, OnceLock},
};
use windows::Win32::{Foundation::{E_FAIL, E_POINTER}, System::Com::CoTaskMemFree};
use windows::Win32::UI::Shell::{
    IFolderView2, IShellFolder, IShellItem, SHCreateItemFromParsingName, SHCreateItemWithParent,
    SICHINT_CANONICAL, SICHINT_TEST_FILESYSPATH_IF_NOT_EQUAL, SVGIO_ALLVIEW, SVSI_DESELECTOTHERS,
    SVSI_FOCUSED, SVSI_SELECT,
};
use windows::core::{HSTRING, Result};

static INDEX_HINTS: OnceLock<Mutex<HashMap<OsString, i32>>> = OnceLock::new();

/// Refresh from an already captured desktop inventory, without extra Shell calls.
pub(crate) fn update_hints(items: impl Iterator<Item = (OsString, i32)>) {
    let next = items.collect();
    if let Ok(mut cache) = INDEX_HINTS.get_or_init(Mutex::default).lock() {
        *cache = next;
    }
}

pub(super) fn matches(item: &IShellItem, target: &IShellItem) -> Result<bool> {
    unsafe {
        Ok(item.Compare(
            target,
            (SICHINT_CANONICAL.0 | SICHINT_TEST_FILESYSPATH_IF_NOT_EQUAL.0).cast_unsigned(),
        )? == 0)
    }
}

pub fn item_at(folder: &IFolderView2, index: i32) -> Result<IShellItem> {
    let parent: IShellFolder = unsafe { folder.GetFolder()? };
    item_with_parent(folder, &parent, index)
}

fn item_with_parent(
    folder: &IFolderView2,
    parent: &IShellFolder,
    index: i32,
) -> Result<IShellItem> {
    unsafe {
        let pidl = folder.Item(index)?;
        if pidl.is_null() {
            return Err(E_POINTER.into());
        }
        let item = SHCreateItemWithParent(None, parent, pidl);
        CoTaskMemFree(Some(pidl.cast()));
        item
    }
}

pub fn find(folder: &IFolderView2, target: &IShellItem) -> Result<Option<i32>> {
    let parent: IShellFolder = unsafe { folder.GetFolder()? };
    let count = unsafe { folder.ItemCount(SVGIO_ALLVIEW)? };
    find_match(count, |index| {
        // The merged desktop namespace and a filesystem path can have different PIDLs.
        matches(&item_with_parent(folder, &parent, index)?, target)
    })
}

fn find_match(count: i32, mut compare: impl FnMut(i32) -> Result<bool>) -> Result<Option<i32>> {
    let mut first_error = None;
    for index in 0..count {
        match compare(index) {
            Ok(true) => return Ok(Some(index)),
            Ok(false) => {}
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    first_error.map_or(Ok(None), Err)
}

pub fn resolve(folder: &IFolderView2, name: impl AsRef<OsStr>) -> Result<i32> {
    let name = name.as_ref();
    let target: IShellItem = unsafe { SHCreateItemFromParsingName(&HSTRING::from(name), None)? };
    // Never trust cached indices after sorting, deletion or an Explorer restart.
    // Release the lock before every COM call (COM can re-enter the message loop).
    let hint = INDEX_HINTS
        .get()
        .and_then(|cache| cache.lock().ok()?.get(name).copied());
    if let Some(index) = hint
        && item_at(folder, index)
            .and_then(|item| matches(&item, &target))
            .unwrap_or(false)
    {
        return Ok(index);
    }
    let index = find(folder, &target)?.ok_or_else(|| {
        windows::core::Error::new(
            windows::Win32::Foundation::E_INVALIDARG,
            "Requested Shell item is not in the desktop view; refusing a different target",
        )
    })?;
    if let Ok(mut cache) = INDEX_HINTS.get_or_init(Mutex::default).lock() {
        // Bound fallback-only use when no inventory publisher is present.
        if cache.len() >= 1024 {
            cache.clear();
        }
        cache.insert(name.to_os_string(), index);
    }
    Ok(index)
}

pub struct RestoreSelection {
    folder: IFolderView2,
    selected: Vec<(IShellItem, u32)>,
    armed: bool,
    restore_previous: bool,
}

fn focused_index(folder: &IFolderView2) -> Result<Option<i32>> {
    match unsafe { folder.GetFocusedItem() } {
        Ok(index) => Ok(Some(index)),
        // When the view cannot supply an index, check focus on each item below.
        Err(error) if error.code() == E_FAIL => Ok(None),
        Err(error) => Err(error),
    }
}

fn selection_flags(folder: &IFolderView2, index: i32, focused: Option<i32>) -> Result<u32> {
    let actual = unsafe {
        let pidl = folder.Item(index)?;
        if pidl.is_null() {
            return Err(E_POINTER.into());
        }
        let flags = folder.GetSelectionState(pidl);
        CoTaskMemFree(Some(pidl.cast()));
        flags?
    };
    let selected = actual & SVSI_SELECT.0.cast_unsigned();
    let focused = focused.map_or(actual & SVSI_FOCUSED.0.cast_unsigned() != 0, |at| at == index);
    Ok(selected | if focused { SVSI_FOCUSED.0.cast_unsigned() } else { 0 })
}

impl RestoreSelection {
    #[cfg(feature = "desktop-menu-diagnostics")]
    pub fn capture(folder: &IFolderView2) -> Result<Self> {
        let mut selected = Vec::new();
        unsafe {
            let focused = focused_index(folder)?;
            for index in 0..folder.ItemCount(SVGIO_ALLVIEW)? {
                let flags = selection_flags(folder, index, focused)?;
                if flags != 0 {
                    selected.push((item_at(folder, index)?, flags));
                }
            }
        }
        Ok(Self {
            folder: folder.clone(),
            selected,
            armed: true,
            restore_previous: true,
        })
    }

    /// Pane input owns selection now. Never reselect/refocus an old desktop item.
    pub fn deselect_on_close(folder: &IFolderView2) -> Self {
        Self {
            folder: folder.clone(),
            selected: Vec::new(),
            armed: true,
            restore_previous: false,
        }
    }

    pub fn finish(mut self) -> Result<()> {
        self.armed = false;
        self.restore()
    }

    fn restore(&self) -> Result<()> {
        // Resolve again before changing selection: deletion/reordering invalidates indices.
        let mut resolved = Vec::new();
        for (item, flags) in &self.selected {
            if let Some(index) = find(&self.folder, item)? {
                resolved.push((index, *flags));
            }
        }
        unsafe {
            self.folder
                .SelectItem(-1, SVSI_DESELECTOTHERS.0.cast_unsigned())?;
            if !self.restore_previous {
                // The hook clears the temporary hidden target's focus on menu end.
                // Calling SelectItem with SVSI_FOCUSED here would activate Explorer.
                return Ok(());
            }
            for &(index, flags) in &resolved {
                self.folder.SelectItem(index, flags)?;
            }
            let focused = focused_index(&self.folder)?;
            for index in 0..self.folder.ItemCount(SVGIO_ALLVIEW)? {
                let actual = selection_flags(&self.folder, index, focused)?;
                let expected = resolved
                    .iter()
                    .find(|(i, _)| *i == index)
                    .map_or(0, |(_, f)| *f);
                if actual != expected {
                    return Err(windows::core::Error::new(
                        windows::Win32::Foundation::E_FAIL,
                        format!(
                            "Restored selection differs at index {index}: expected {expected}, actual {actual}"
                        ),
                    ));
                }
            }
        }
        trace!("desktop_selection_and_focus_verified=true");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_lookup_continues_past_failed_rows_but_keeps_an_inconclusive_error() {
        let compare = |index: i32| match index {
            0 => Err(E_POINTER.into()),
            1 => Ok(false),
            _ => Ok(true),
        };
        assert_eq!(find_match(3, compare).unwrap(), Some(2));
        assert_eq!(find_match(2, compare).unwrap_err().code(), E_POINTER);
        assert_eq!(find_match(2, |_| Ok(false)).unwrap(), None);
    }
}

impl Drop for RestoreSelection {
    fn drop(&mut self) {
        if self.armed {
            // Restoration is behavior, not a formatting argument: always execute it.
            let result = self.restore();
            if let Err(error) = &result {
                luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Warn, "shell.selection", "restore failed: {error}");
            }
            trace!("desktop_selection_restored={result:?}");
        }
    }
}
