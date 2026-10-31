//! Copy notification payloads immediately; resolve names and icon indices on a worker STA.
use luciddesk_core::ShellIdentity;
use std::collections::BTreeSet;
use windows::{
    Win32::{System::Com::CoTaskMemFree, UI::Shell::*},
    core::PCWSTR,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Change {
    Item(Vec<u8>),
    Image(i32),
    Name(String),
    All,
}

// DesktopChangeSubscription uses legacy delivery: wParam points to two PIDLs
// owned by Shell for the duration of this callback. Never retain their pointers.
pub(super) unsafe fn capture(wparam: usize, event: u32) -> Vec<Change> {
    if event & SHCNE_ASSOCCHANGED.0 != 0 {
        return vec![Change::All];
    }
    let item_events = SHCNE_UPDATEITEM.0
        | SHCNE_UPDATEDIR.0
        | SHCNE_ATTRIBUTES.0
        | SHCNE_RENAMEITEM.0
        | SHCNE_RENAMEFOLDER.0
        | SHCNE_CREATE.0
        | SHCNE_DELETE.0
        | SHCNE_MKDIR.0
        | SHCNE_RMDIR.0
        | SHCNE_DRIVEADD.0
        | SHCNE_DRIVEREMOVED.0
        | SHCNE_MEDIAINSERTED.0
        | SHCNE_MEDIAREMOVED.0;
    if wparam == 0 {
        // Missing invalidation payload is uncertainty, not proof that no icon changed.
        return if event & (item_events | SHCNE_UPDATEIMAGE.0) != 0 {
            vec![Change::All]
        } else {
            Vec::new()
        };
    }
    let pair = wparam as *const *const Common::ITEMIDLIST;
    if event & SHCNE_UPDATEIMAGE.0 != 0 {
        let extra = unsafe { *pair.add(1) };
        let index = if !extra.is_null() {
            unsafe { SHHandleUpdateImage(extra) }
        } else {
            // SHCNE_UPDATEIMAGE puts the system-image index in dwItem2, not
            // dwItem1 (normally zero). Shell packs both into this legacy PIDL.
            let first = unsafe { *pair };
            if !first.is_null() && unsafe { ILGetSize(Some(first)) } >= 12 {
                unsafe { std::ptr::read_unaligned(first.cast::<u8>().add(6).cast::<i32>()) }
            } else {
                -1
            }
        };
        return vec![if index >= 0 {
            Change::Image(index)
        } else {
            Change::All
        }];
    }
    if event & item_events == 0 {
        return Vec::new();
    }
    let count = if event & (SHCNE_RENAMEITEM.0 | SHCNE_RENAMEFOLDER.0) != 0 {
        2
    } else {
        1
    };
    (0..count)
        .map(|index| unsafe {
            let pidl = *pair.add(index);
            if pidl.is_null() {
                return Change::All;
            }
            let size = ILGetSize(Some(pidl)) as usize;
            if size <= 2 {
                return Change::All;
            }
            Change::Item(std::slice::from_raw_parts(pidl.cast::<u8>(), size).to_vec())
        })
        .collect()
}

#[derive(Clone, Default)]
pub(super) struct Pending {
    changes: BTreeSet<Change>,
}
impl Pending {
    pub(super) fn merge(&mut self, other: Self) {
        self.add(other.changes);
    }
    pub(super) fn add(&mut self, changes: impl IntoIterator<Item = Change>) {
        for change in changes {
            if self.changes.contains(&Change::All) {
                break;
            }
            if change == Change::All || self.changes.len() >= 256 {
                self.changes.clear();
                self.changes.insert(Change::All);
                break;
            }
            self.changes.insert(change);
        }
    }
    pub(super) fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

fn normalize(name: &str) -> String {
    name.trim_end_matches(['\\', '/'])
        .replace('/', "\\")
        .to_lowercase()
}

impl Pending {
    pub(super) fn affected(&self, identities: Vec<ShellIdentity>) -> Vec<ShellIdentity> {
        if self.changes.contains(&Change::All) {
            return identities;
        }
        let mut names = BTreeSet::new();
        let mut images = BTreeSet::new();
        for change in &self.changes {
            match change {
                Change::Item(bytes) => unsafe {
                    let pidl = bytes.as_ptr().cast();
                    let mut resolved = false;
                    for kind in [SIGDN_FILESYSPATH, SIGDN_DESKTOPABSOLUTEPARSING] {
                        if let Ok(name) = SHGetNameFromIDList(pidl, kind) {
                            if let Ok(value) = name.to_string() {
                                if !value.is_empty() {
                                    names.insert(normalize(&value));
                                    resolved = true;
                                }
                            }
                            CoTaskMemFree(Some(name.0.cast()));
                        }
                    }
                    if !resolved {
                        return identities;
                    }
                },
                Change::Image(index) => {
                    images.insert(*index);
                }
                Change::Name(name) => {
                    names.insert(normalize(name));
                }
                Change::All => {}
            }
        }
        identities
            .into_iter()
            .filter(|identity| {
                let name = match identity {
                    ShellIdentity::FileSystem { path, .. } => path.to_string_lossy().into_owned(),
                    ShellIdentity::Namespace { parsing_name } => parsing_name.clone(),
                };
                if names.contains(&normalize(&name)) {
                    return true;
                }
                if images.is_empty() {
                    return false;
                }
                let name: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
                let mut info = SHFILEINFOW::default();
                unsafe {
                    // Namespace parsing names need PIDL lookup; no bitmap is loaded here.
                    let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(
                        PCWSTR(name.as_ptr()),
                        None,
                    ) else {
                        return true;
                    };
                    let Ok(pidl) = SHGetIDListFromObject(&item) else {
                        return true;
                    };
                    let ok = SHGetFileInfoW(
                        PCWSTR(pidl.cast()),
                        windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
                        Some(&raw mut info),
                        std::mem::size_of::<SHFILEINFOW>() as u32,
                        SHGFI_PIDL | SHGFI_SYSICONINDEX,
                    );
                    CoTaskMemFree(Some(pidl.cast()));
                    ok == 0 || images.contains(&info.iIcon)
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Delivers a real Shell image-change notification; run alone on an interactive desktop"]
    fn native_image_notification_preserves_system_index() {
        use std::{cell::RefCell, rc::Rc, time::{Duration, Instant}};
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let _sta = crate::pane::test_support::apartment();
        let received = Rc::new(RefCell::new(Vec::new()));
        let output = received.clone();
        const MESSAGE: u32 = WM_APP + 921;
        let window = windows_window::Window::new("LucidDesk icon notification test")
            .size(1, 1).style(WS_POPUP).ex_style(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)
            .on_message(move |_, message, wp, lp| {
                if message == MESSAGE && lp as u32 & SHCNE_UPDATEIMAGE.0 != 0 {
                    output.borrow_mut().extend(unsafe { capture(wp, lp as u32) });
                    Some(0)
                } else { None }
            }).create().unwrap();
        let _subscription = luciddesk_shell::DesktopChangeSubscription::register(window.hwnd() as isize, MESSAGE).unwrap();
        unsafe { SHChangeNotify(SHCNE_UPDATEIMAGE, SHCNF_DWORD | SHCNF_FLUSHNOWAIT, None, Some(27usize as *const _)); }
        let deadline = Instant::now() + Duration::from_secs(3);
        while received.borrow().is_empty() && Instant::now() < deadline {
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(received.borrow().contains(&Change::Image(27)), "actual payload: {:?}", received.borrow());
    }

    #[test]
    fn missing_item_payload_invalidates_instead_of_dropping_changes() {
        for event in [SHCNE_UPDATEIMAGE, SHCNE_UPDATEITEM, SHCNE_RENAMEITEM, SHCNE_UPDATEDIR] {
            assert_eq!(unsafe { capture(0, event.0) }, [Change::All]);
        }
        let pair = [std::ptr::null::<Common::ITEMIDLIST>(); 2];
        assert_eq!(unsafe { capture(pair.as_ptr() as usize, SHCNE_UPDATEITEM.0) }, [Change::All]);
        assert!(unsafe { capture(0, SHCNE_FREESPACE.0) }.is_empty());
    }

    #[test]
    fn failed_icon_index_lookup_remains_a_refresh_candidate() {
        let _sta = crate::pane::test_support::apartment();
        let unavailable = ShellIdentity::Namespace {
            parsing_name: "::{C678BBA5-CEB1-4F2A-8D5F-3524B8C09372}".into(),
        };
        let computer = ShellIdentity::Namespace {
            parsing_name: "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}".into(),
        };
        let mut pending = Pending::default();
        pending.add([Change::Image(i32::MAX)]);
        assert_eq!(pending.affected(vec![computer, unavailable.clone()]), [unavailable]);
    }
    #[test]
    fn duplicate_notifications_coalesce_and_global_changes_supersede_items() {
        let mut pending = Pending::default();
        pending.add([Change::Image(7), Change::Image(7), Change::Image(9)]);
        assert_eq!(pending.changes.len(), 2);
        pending.add([Change::All, Change::Image(10)]);
        assert_eq!(pending.changes, BTreeSet::from([Change::All]));
    }
    #[test]
    fn copied_shell_notification_refreshes_only_its_namespace_item() {
        let _sta = crate::pane::test_support::apartment();
        let recycle = "::{645FF040-5081-101B-9F08-00AA002F954E}";
        let computer = "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}";
        let wide: Vec<_> = recycle.encode_utf16().chain(Some(0)).collect();
        let changes = unsafe {
            let item: IShellItem =
                SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).unwrap();
            let pidl = SHGetIDListFromObject(&item).unwrap();
            let pair = [pidl as *const Common::ITEMIDLIST, std::ptr::null()];
            let changes = capture(pair.as_ptr() as usize, SHCNE_UPDATEITEM.0);
            CoTaskMemFree(Some(pidl.cast()));
            changes
        };
        let mut pending = Pending::default();
        pending.add(changes);
        let candidates = vec![
            ShellIdentity::Namespace {
                parsing_name: recycle.into(),
            },
            ShellIdentity::Namespace {
                parsing_name: computer.into(),
            },
        ];
        let selected = pending.affected(candidates);
        assert_eq!(selected.len(), 1);
        assert_eq!(
            selected[0],
            ShellIdentity::Namespace {
                parsing_name: recycle.into()
            }
        );
    }

    #[test]
    fn image_payload_is_decoded_as_index_not_a_shell_item() {
        let payload = SHChangeDWORDAsIDList {
            cb: 10,
            dwItem1: 0,
            dwItem2: 27,
            cbZero: 0,
        };
        let pair = [
            (&raw const payload).cast::<Common::ITEMIDLIST>(),
            std::ptr::null(),
        ];
        let changes = unsafe { capture(pair.as_ptr() as usize, SHCNE_UPDATEIMAGE.0) };
        assert_eq!(changes, vec![Change::Image(27)]);
        assert!(unsafe { capture(0, SHCNE_FREESPACE.0) }.is_empty());
    }

    #[test]
    fn recycle_scope_refreshes_recycle_only_without_item_payload() {
        let recycle = "::{645FF040-5081-101B-9F08-00AA002F954E}";
        let mut pending = Pending::default();
        pending.add([Change::Name(recycle.into())]);
        let affected = pending.affected(vec![
            ShellIdentity::Namespace {
                parsing_name: recycle.into(),
            },
            ShellIdentity::Namespace {
                parsing_name: "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}".into(),
            },
        ]);
        assert_eq!(affected.len(), 1);
        assert_eq!(
            affected[0],
            ShellIdentity::Namespace {
                parsing_name: recycle.into()
            }
        );
    }

    #[test]
    fn names_include_full_location_not_display_label() {
        assert_ne!(
            normalize("C:\\Users\\Public\\Desktop\\LocalSend.lnk"),
            normalize("C:\\Users\\Yuchen\\Desktop\\LocalSend.lnk")
        );
        assert_eq!(
            normalize("C:/Desktop/Icon.lnk"),
            normalize("c:\\desktop\\icon.lnk")
        );
    }
}
