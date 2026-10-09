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
    if wparam == 0 {
        return Vec::new();
    }
    let pair = wparam as *const *const Common::ITEMIDLIST;
    if event & SHCNE_UPDATEIMAGE.0 != 0 {
        let extra = unsafe { *pair.add(1) };
        let index = if !extra.is_null() {
            unsafe { SHHandleUpdateImage(extra) }
        } else {
            // Legacy DWORD notifications are encoded as SHChangeDWORDAsIDList.
            let first = unsafe { *pair };
            if !first.is_null() && unsafe { ILGetSize(Some(first)) } >= 12 {
                unsafe { std::ptr::read_unaligned(first.cast::<u8>().add(2).cast::<i32>()) }
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
    if event & item_events == 0 {
        return Vec::new();
    }
    let count = if event & (SHCNE_RENAMEITEM.0 | SHCNE_RENAMEFOLDER.0) != 0 {
        2
    } else {
        1
    };
    (0..count)
        .filter_map(|index| unsafe {
            let pidl = *pair.add(index);
            if pidl.is_null() {
                return None;
            }
            let size = ILGetSize(Some(pidl)) as usize;
            Some(Change::Item(
                std::slice::from_raw_parts(pidl.cast::<u8>(), size).to_vec(),
            ))
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
                    for kind in [SIGDN_FILESYSPATH, SIGDN_DESKTOPABSOLUTEPARSING] {
                        if let Ok(name) = SHGetNameFromIDList(pidl, kind) {
                            if let Ok(value) = name.to_string() {
                                names.insert(normalize(&value));
                            }
                            CoTaskMemFree(Some(name.0.cast()));
                        }
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
                        return false;
                    };
                    let Ok(pidl) = SHGetIDListFromObject(&item) else {
                        return false;
                    };
                    let ok = SHGetFileInfoW(
                        PCWSTR(pidl.cast()),
                        windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
                        Some(&raw mut info),
                        std::mem::size_of::<SHFILEINFOW>() as u32,
                        SHGFI_PIDL | SHGFI_SYSICONINDEX,
                    );
                    CoTaskMemFree(Some(pidl.cast()));
                    ok != 0 && images.contains(&info.iIcon)
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            dwItem1: 27,
            dwItem2: 0,
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
