//! The actual directory's Explorer classic background menu.
use super::file_command::{Menu, MenuMessages, menu_messages};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};
use windows::{
    Win32::{
        Foundation::{E_NOINTERFACE, E_POINTER, HWND, POINT},
        System::{
            Com::{CoTaskMemFree, IServiceProvider, IServiceProvider_Impl},
            Ole::IObjectWithSite,
        },
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{GUID, HSTRING, Interface, PCSTR, PCWSTR, Result, implement},
};
use windows_core::IUnknownImpl;

pub enum FolderMenuResult {
    Cancelled,
    Invoked { created: Option<PathBuf> },
}

#[implement(INewMenuClient, IServiceProvider)]
struct NewClient(Rc<RefCell<Option<PathBuf>>>);
impl INewMenuClient_Impl for NewClient_Impl {
    fn IncludeItems(&self) -> Result<i32> {
        Ok(NMCII_ITEMS.0 | NMCII_FOLDERS.0)
    }
    fn SelectAndEditItem(&self, pidl: *const Common::ITEMIDLIST, _: i32) -> Result<()> {
        unsafe {
            let item: IShellItem = SHCreateItemFromIDList(pidl)?;
            let name = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            use std::os::windows::ffi::OsStringExt;
            *self.0.borrow_mut() =
                Some(PathBuf::from(std::ffi::OsString::from_wide(name.as_wide())));
            CoTaskMemFree(Some(name.0.cast()));
        }
        Ok(())
    }
}
impl IServiceProvider_Impl for NewClient_Impl {
    fn QueryService(
        &self,
        service: *const GUID,
        iid: *const GUID,
        out: *mut *mut std::ffi::c_void,
    ) -> Result<()> {
        unsafe {
            if service.is_null() || iid.is_null() || out.is_null() {
                return Err(E_POINTER.into());
            }
            *out = std::ptr::null_mut();
            if *service != INewMenuClient::IID {
                return Err(E_NOINTERFACE.into());
            }
            let client = self.to_interface::<INewMenuClient>();
            (client.vtable().base__.QueryInterface)(client.as_raw(), iid, out).ok()
        }
    }
}

fn background_context(owner: HWND, path: &Path) -> Result<IContextMenu> {
    if !path.is_dir() {
        return Err(windows::Win32::Foundation::E_INVALIDARG.into());
    }
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None)?;
        let folder: IShellFolder = item.BindToHandler(None, &BHID_SFObject)?;
        folder.CreateViewObject(owner)
    }
}

/// Shows the real directory background menu, including its registered extensions.
/// Missing directories fail instead of falling back to the desktop or mapped root.
pub fn show_folder_menu(owner: HWND, path: &Path, point: POINT) -> Result<FolderMenuResult> {
    let context = background_context(owner, path)?;
    let created = Rc::new(RefCell::new(None));
    let site: INewMenuClient = NewClient(Rc::clone(&created)).into();
    if let Ok(with_site) = context.cast::<IObjectWithSite>() {
        unsafe {
            let _ = with_site.SetSite(&site);
        }
    }
    let theme = luciddesk_menu::apply_scoped_theme(owner.0);
    unsafe {
        let menu = Menu(CreatePopupMenu()?);
        context
            .QueryContextMenu(menu.0, 0, 1, 0x7fff, CMF_NORMAL | CMF_SYNCCASCADEMENU)
            .ok()?;
        let messages = Box::new(MenuMessages {
            context: context.clone(),
            owner,
        });
        if windows_sys::Win32::UI::Shell::SetWindowSubclass(
            owner.0,
            Some(menu_messages),
            0x4c50464d,
            (&raw const *messages) as usize,
        ) == 0
        {
            return Err(windows::core::Error::from_thread());
        }
        let frame = luciddesk_menu::MenuFrame::install(
            owner.0,
            windows_sys::Win32::Foundation::POINT {
                x: point.x,
                y: point.y,
            },
        );
        let chosen = TrackPopupMenuEx(
            menu.0,
            (TPM_RETURNCMD | TPM_RIGHTBUTTON).0,
            point.x,
            point.y,
            owner,
            None,
        )
        .0;
        drop(frame);
        drop(messages);
        drop(theme);
        if chosen == 0 {
            return Ok(FolderMenuResult::Cancelled);
        }
        let directory = HSTRING::from(path.as_os_str());
        let command = CMINVOKECOMMANDINFOEX {
            cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
            // CMIC_MASK_UNICODE is the Shell header alias for SEE_MASK_UNICODE.
            fMask: SEE_MASK_UNICODE | CMIC_MASK_PTINVOKE,
            hwnd: owner,
            lpVerb: PCSTR((chosen as usize - 1) as *const u8),
            lpVerbW: PCWSTR((chosen as usize - 1) as *const u16),
            lpDirectoryW: PCWSTR(directory.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ptInvoke: point,
            ..Default::default()
        };
        context.InvokeCommand((&raw const command).cast())?;
        Ok(FolderMenuResult::Invoked {
            created: created.borrow_mut().take(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn context_targets_current_directory_and_rejects_missing_destinations() {
        let _sta = crate::ShellApartment::initialize_sta().unwrap();
        let root = std::env::temp_dir();
        let context = background_context(HWND::default(), &root).unwrap();
        let menu = Menu(unsafe { CreatePopupMenu().unwrap() });
        unsafe {
            context
                .QueryContextMenu(menu.0, 0, 1, 0x7fff, CMF_NORMAL)
                .ok()
                .unwrap();
            assert!(GetMenuItemCount(Some(menu.0)) > 0);
        }
        assert!(
            background_context(
                HWND::default(),
                &root.join(format!("luciddesk-missing-menu-{}", std::process::id()))
            )
            .is_err()
        );
    }
}
