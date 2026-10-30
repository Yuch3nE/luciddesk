//! Native desktop layout access. Explorer retains all rendering and input ownership.
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::UI::Shell::{IFolderView2, IShellWindows, SVGIO_ALLVIEW, ShellWindows};
use windows::core::Interface;

use crate::error::com_context;
use crate::namespace::Pidl;

const SNAPSHOT_CHANGED: windows::core::HRESULT =
    windows::core::HRESULT::from_win32(windows::Win32::Foundation::ERROR_RETRY.0);

// Retry only races, never permission/connection failures. Each attempt starts
// from a fresh view and discards all data from the unsuccessful attempt.
fn retry_snapshot<T>(
    mut capture: impl FnMut() -> windows::core::Result<T>,
) -> windows::core::Result<T> {
    for attempt in 0..3 {
        match capture() {
            Err(error)
                if attempt < 2
                    && (error.code() == windows::Win32::Foundation::E_BOUNDS
                        || error.code() == SNAPSHOT_CHANGED) => {}
            result => return result,
        }
    }
    unreachable!()
}

#[track_caller]
fn read_error(
    error: windows::core::Error,
    operation: &str,
    index: i32,
    count: i32,
) -> windows::core::Error {
    com_context(error, &format!("{operation} index={index}, count={count}"))
}

fn validate_revision(
    before: &NativeDesktopRevision,
    after: &NativeDesktopRevision,
) -> windows::core::Result<()> {
    let unique: std::collections::BTreeSet<_> = before.item_ids.iter().collect();
    if before != after
        || unique.len() != before.item_ids.len()
        || before.item_ids.iter().any(Vec::is_empty)
    {
        return Err(windows::core::Error::new(
            SNAPSHOT_CHANGED,
            "桌面列表在读取期间变化，丢弃本次快照",
        ));
    }
    Ok(())
}

#[track_caller]
fn entry_error(
    error: crate::ShellError,
    index: i32,
    count: i32,
    parsing_name: Option<&str>,
) -> windows::core::Error {
    // Preserve COM retry semantics and the original diagnostic. Non-COM
    // decoding failures are not evidence that Explorer changed its contents.
    let code = match &error {
        crate::ShellError::Com(error) => error.code(),
        _ => windows::Win32::Foundation::E_FAIL,
    };
    let error = windows::core::Error::new(
        code,
        format!(
            "Shell item metadata failed: index={index}, count={count}, parsing_name={:?}: {error}",
            parsing_name.unwrap_or("<unavailable>")
        ),
    );
    com_context(error, "Shell item metadata")
}

/// Explorer-owned item IDs and view metrics, without opening desktop files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeDesktopRevision {
    view: isize,
    icon_size: i32,
    spacing: (i32, i32),
    dpi: u32,
    item_ids: Vec<Vec<u8>>,
}

impl NativeDesktopRevision {
    /// Shell view window used for this revision (the parent of the desktop ListView).
    #[must_use]
    pub fn view_window(&self) -> isize {
        self.view
    }
}

/// Read-only capture of Explorer's visible items and icon metrics. Never changes folder flags.
#[derive(Clone, Debug)]
pub struct NativeDesktopSnapshot {
    pub revision: NativeDesktopRevision,
    pub icon_size: i32,
    pub spacing: (i32, i32),
    pub dpi: u32,
    pub items: Vec<(super::ShellEntry, i32, i32)>,
    /// Original view indices, aligned with items even if an individual Shell item was skipped.
    pub view_indices: Vec<i32>,
}

impl NativeDesktopSnapshot {
    /// Reject partial metadata captures before reconciling persisted membership.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.items.len() == self.revision.item_ids.len()
            && self.items.len() == self.view_indices.len()
    }
}

/// Captures the actual desktop view, including its visibility preferences and display names.
/// # Errors
/// Returns an error when Explorer's desktop COM view cannot be reached.
pub fn native_desktop_snapshot() -> Result<NativeDesktopSnapshot, String> {
    capture_desktop_snapshot()
}

/// Read only Explorer's in-memory item IDs on a worker STA. Metadata is resolved
/// by the full snapshot only after a revision change has been detected.
#[derive(Default)]
pub struct NativeDesktopReader {
    shell: Option<IShellWindows>,
}

impl NativeDesktopReader {
    /// Must be used and dropped within the caller's initialized COM apartment.
    /// Failed queries discard the cached connection so the next check reconnects.
    ///
    /// # Errors
    /// Returns an error when Explorer's desktop COM view cannot be reached.
    pub fn revision(&mut self) -> Result<NativeDesktopRevision, String> {
        let result = retry_snapshot(|| self.capture_revision());
        if result.is_err() {
            self.shell = None;
        }
        result.map_err(|error| format!("读取桌面项目标识失败：{error}"))
    }

    fn capture_revision(&mut self) -> windows::core::Result<NativeDesktopRevision> {
        if self.shell.is_none() {
            self.shell = Some(unsafe {
                CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)
                    .map_err(|e| com_context(e, "CoCreateInstance(ShellWindows)"))?
            });
        }
        let shell = self.shell.as_ref().unwrap();
        let before = capture_revision(shell)?;
        let after = capture_revision(shell)?;
        validate_revision(&before, &after)?;
        Ok(before)
    }
}

fn capture_revision(shell: &IShellWindows) -> windows::core::Result<NativeDesktopRevision> {
    let (folder, hwnd) = desktop_folder(shell)?;
    let mut revision = revision_header(&folder, hwnd)?;
    let count = unsafe {
        folder
            .ItemCount(SVGIO_ALLVIEW)
            .map_err(|e| com_context(e, "IFolderView2::ItemCount"))?
    };
    for index in 0..count {
        if index > 0 && index % 8 == 0 {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let pidl = Pidl::new(
            unsafe { folder.Item(index) }
                .map_err(|error| read_error(error, "IFolderView2::Item", index, count))?,
        );
        revision.item_ids.push(item_id(&pidl));
    }
    if unsafe {
        folder
            .ItemCount(SVGIO_ALLVIEW)
            .map_err(|e| com_context(e, "IFolderView2::ItemCount"))?
    } != count
    {
        return Err(windows::core::Error::new(
            SNAPSHOT_CHANGED,
            "桌面项目数量在读取期间变化",
        ));
    }
    Ok(revision)
}

fn revision_header(
    folder: &IFolderView2,
    hwnd: windows::Win32::Foundation::HWND,
) -> windows::core::Result<NativeDesktopRevision> {
    unsafe {
        let mut icon_size = 48;
        let mut mode = windows::Win32::UI::Shell::FOLDERVIEWMODE::default();
        folder
            .GetViewModeAndIconSize(&raw mut mode, &raw mut icon_size)
            .map_err(|e| com_context(e, "IFolderView2::GetViewModeAndIconSize"))?;
        let mut spacing = POINT::default();
        folder
            .GetSpacing(&raw mut spacing)
            .map_err(|e| com_context(e, "IFolderView2::GetSpacing"))?;
        Ok(NativeDesktopRevision {
            view: hwnd.0 as isize,
            icon_size,
            spacing: (spacing.x, spacing.y),
            dpi: windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd.0).max(96),
            item_ids: Vec::new(),
        })
    }
}

fn item_id(pidl: &Pidl) -> Vec<u8> {
    if pidl.as_ptr().is_null() {
        return Vec::new();
    }
    unsafe {
        let size = windows::Win32::UI::Shell::ILGetSize(Some(pidl.as_ptr())) as usize;
        std::slice::from_raw_parts(pidl.as_ptr().cast::<u8>(), size).to_vec()
    }
}

fn desktop_folder(
    shell: &IShellWindows,
) -> windows::core::Result<(IFolderView2, windows::Win32::Foundation::HWND)> {
    unsafe {
        let view = crate::desktop::shell_view(shell)?;
        let folder: IFolderView2 = view
            .cast()
            .map_err(|e| com_context(e, "QueryInterface(IFolderView2)"))?;
        let hwnd = view
            .GetWindow()
            .map_err(|e| com_context(e, "IShellView::GetWindow"))?;
        Ok((folder, hwnd))
    }
}

fn capture_desktop_snapshot() -> Result<NativeDesktopSnapshot, String> {
    unsafe {
        let capture = || -> windows::core::Result<NativeDesktopSnapshot> {
            let shell: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)
                .map_err(|e| com_context(e, "CoCreateInstance(ShellWindows)"))?;
            let (folder, hwnd) = desktop_folder(&shell)?;
            let parent: windows::Win32::UI::Shell::IShellFolder = folder
                .GetFolder()
                .map_err(|e| com_context(e, "IFolderView2::GetFolder(IShellFolder)"))?;
            let mut origin = windows_sys::Win32::Foundation::POINT::default();
            windows_sys::Win32::Graphics::Gdi::ClientToScreen(hwnd.0, &raw mut origin);
            let mut revision = revision_header(&folder, hwnd)?;
            let mut items = Vec::new();
            let mut view_indices = Vec::new();
            let count = folder
                .ItemCount(SVGIO_ALLVIEW)
                .map_err(|e| com_context(e, "IFolderView2::ItemCount"))?;
            for index in 0..count {
                let pidl = Pidl::new(
                    folder
                        .Item(index)
                        .map_err(|error| read_error(error, "IFolderView2::Item", index, count))?,
                );
                revision.item_ids.push(item_id(&pidl));
                let position = folder.GetItemPosition(pidl.as_ptr()).map_err(|error| {
                    read_error(error, "IFolderView2::GetItemPosition", index, count)
                })?;
                let item: windows::Win32::UI::Shell::IShellItem =
                    windows::Win32::UI::Shell::SHCreateItemWithParent(None, &parent, pidl.as_ptr())
                        .map_err(|error| {
                            read_error(error, "SHCreateItemWithParent", index, count)
                        })?;
                let mut entry = crate::namespace::shell_entry(&item).map_err(|error| {
                    let name = crate::namespace::shell_item_name(
                        &item,
                        windows::Win32::UI::Shell::SIGDN_DESKTOPABSOLUTEPARSING,
                    )
                    .ok();
                    entry_error(error, index, count, name.as_deref())
                })?;
                // Known-folder desktop objects can expose a filesystem path but have a
                // different icon and verbs from the underlying directory (e.g. User Files).
                if let Ok(parsing_name) = crate::namespace::shell_item_name(
                    &item,
                    windows::Win32::UI::Shell::SIGDN_DESKTOPABSOLUTEPARSING,
                ) && parsing_name.starts_with("::{")
                {
                    entry.identity = luciddesk_core::ShellIdentity::Namespace { parsing_name };
                }
                items.push((entry, position.x + origin.x, position.y + origin.y));
                view_indices.push(index);
            }
            validate_revision(&revision, &capture_revision(&shell)?)?;
            Ok(NativeDesktopSnapshot {
                icon_size: revision.icon_size,
                spacing: revision.spacing,
                dpi: revision.dpi,
                revision,
                items,
                view_indices,
            })
        };
        let snapshot = retry_snapshot(capture)
            .map_err(|error| format!("读取原生桌面布局失败（最多 3 次尝试）：{error}"))?;
        super::native_menu::update_hints(snapshot.items.iter().zip(&snapshot.view_indices).map(
            |((item, _, _), &index)| {
                let name = match &item.identity {
                    luciddesk_core::ShellIdentity::FileSystem { path, .. } => {
                        path.to_string_lossy().into_owned()
                    }
                    luciddesk_core::ShellIdentity::Namespace { parsing_name } => {
                        parsing_name.clone()
                    }
                };
                (name, index)
            },
        ));
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_failure_preserves_cause_and_rejects_partial_capture() {
        let mut attempts = 0;
        let result = retry_snapshot(|| {
            attempts += 1;
            let mut captured = Vec::new();
            for index in 0..3 {
                if index == 1 {
                    return Err(entry_error(
                        crate::ShellError::Com(windows::core::Error::from_hresult(
                            windows::Win32::Foundation::E_ACCESSDENIED,
                        )),
                        index,
                        3,
                        Some("::{test-item}"),
                    ));
                }
                captured.push(index);
            }
            Ok(captured)
        });
        let error = result.unwrap_err();
        assert_eq!(attempts, 1);
        assert_eq!(error.code(), windows::Win32::Foundation::E_ACCESSDENIED);
        let message = error.to_string();
        assert!(message.contains("index=1, count=3"));
        assert!(message.contains("::{test-item}"));
        assert!(message.contains("Windows Shell COM error"));
    }

    #[test]
    fn undecodable_metadata_is_not_reported_as_a_snapshot_race() {
        let error = entry_error(
            crate::ShellError::Utf16(String::from_utf16(&[0xD800]).unwrap_err()),
            0,
            1,
            None,
        );
        assert_eq!(error.code(), windows::Win32::Foundation::E_FAIL);
        assert!(error.to_string().contains("invalid UTF-16"));
        assert!(error.to_string().contains("<unavailable>"));
    }

    #[test]
    fn retries_bounds_and_changed_snapshots_without_returning_partial_data() {
        let mut calls = 0;
        let result = retry_snapshot(|| {
            calls += 1;
            match calls {
                1 => Err(windows::core::Error::from_hresult(
                    windows::Win32::Foundation::E_BOUNDS,
                )),
                2 => Err(windows::core::Error::from_hresult(SNAPSHOT_CHANGED)),
                _ => Ok(vec![3, 4]),
            }
        })
        .unwrap();
        assert_eq!(calls, 3);
        assert_eq!(result, vec![3, 4]);
    }

    #[test]
    fn persistent_races_are_bounded_and_other_failures_are_not_retried() {
        for (code, attempts) in [
            (SNAPSHOT_CHANGED, 3),
            (windows::Win32::Foundation::E_ACCESSDENIED, 1),
        ] {
            let mut calls = 0;
            let result: windows::core::Result<()> = retry_snapshot(|| {
                calls += 1;
                Err(read_error(
                    windows::core::Error::from_hresult(code),
                    "IFolderView2::Item",
                    19,
                    20,
                ))
            });
            let error = result.unwrap_err();
            assert_eq!(calls, attempts);
            assert_eq!(error.code(), code);
            assert!(error.to_string().contains("index=19, count=20"));
        }
    }

    #[test]
    fn detects_shrink_growth_same_count_replacement_reorder_and_duplicate_ids() {
        let revision = |ids: &[u8]| NativeDesktopRevision {
            item_ids: ids.iter().map(|id| vec![*id]).collect(),
            ..Default::default()
        };
        let before = revision(&[1, 2]);
        assert!(validate_revision(&before, &before).is_ok());
        for ids in [&[1][..], &[1, 2, 3], &[1, 3], &[2, 1]] {
            assert_eq!(
                validate_revision(&before, &revision(ids))
                    .unwrap_err()
                    .code(),
                SNAPSHOT_CHANGED
            );
        }
        let duplicate = revision(&[1, 1]);
        assert!(validate_revision(&duplicate, &duplicate).is_err());
        let mut after = before.clone();
        after.view = 1;
        assert!(validate_revision(&before, &after).is_err());
        let invalid = NativeDesktopRevision {
            item_ids: vec![Vec::new()],
            ..Default::default()
        };
        assert!(validate_revision(&invalid, &invalid).is_err());
    }

    #[test]
    fn snapshot_race_matrix_never_publishes_the_failed_attempt() {
        let stable = NativeDesktopRevision {
            view: 42,
            icon_size: 48,
            spacing: (80, 80),
            dpi: 96,
            item_ids: vec![vec![1], vec![2]],
        };
        for case in 0..8 {
            let mut changed = stable.clone();
            match case {
                0 => {
                    changed.item_ids.pop();
                }
                1 => changed.item_ids.push(vec![3]),
                2 => changed.item_ids.reverse(),
                3 => changed.item_ids[1] = vec![3],
                4 => changed.view += 1,
                5 => changed.icon_size += 1,
                6 => changed.spacing.0 += 1,
                _ => changed.dpi = 144,
            }
            let mut calls = 0;
            let result = retry_snapshot(|| {
                calls += 1;
                let after = if calls == 1 { &changed } else { &stable };
                validate_revision(&stable, after)?;
                Ok((calls, after.clone()))
            })
            .unwrap();
            assert_eq!(result, (2, stable.clone()), "case {case}");
        }
        let empty = NativeDesktopRevision::default();
        assert!(validate_revision(&empty, &empty).is_ok());
    }

    #[test]
    #[ignore = "Reads live Explorer; run in an interactive session with the desktop unchanged"]
    fn background_revision_matches_full_snapshot() {
        let _sta = crate::ShellApartment::initialize_sta().unwrap();
        let snapshot = native_desktop_snapshot().unwrap();
        let revision = std::thread::spawn(|| {
            let _sta = crate::ShellApartment::initialize_sta().unwrap();
            let mut reader = NativeDesktopReader::default();
            let first = reader.revision().unwrap();
            assert_eq!(reader.revision().unwrap(), first);
            // Simulate invalidating a stale connection without restarting Explorer.
            reader.shell = None;
            assert_eq!(reader.revision().unwrap(), first);
            first
        })
        .join()
        .unwrap();
        assert_eq!(revision, snapshot.revision);
    }
}
