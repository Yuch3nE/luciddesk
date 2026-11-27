//! The actual directory's Explorer classic background menu.
use super::file_command::{
    LAST_SHELL_MENU_COMMAND_ID, Menu, MenuMessages, menu_invocation, shell_menu_command_offset,
};
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
    core::{GUID, HSTRING, IUnknown, Interface, PCWSTR, Result, implement},
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

struct MenuSite(IObjectWithSite);

impl Drop for MenuSite {
    fn drop(&mut self) {
        let _ = unsafe { self.0.SetSite(None::<&IUnknown>) };
    }
}

/// Shows the real directory background menu, including its registered extensions.
/// Missing directories fail instead of falling back to the desktop or mapped root.
pub fn show_folder_menu(owner: HWND, path: &Path, point: POINT) -> Result<FolderMenuResult> {
    let _active = crate::native_menu::ActiveMenu::acquire()?;
    let context = background_context(owner, path)?;
    let created = Rc::new(RefCell::new(None));
    let site: INewMenuClient = NewClient(Rc::clone(&created)).into();
    let site_binding = context.cast::<IObjectWithSite>().ok().and_then(|with_site| {
        unsafe { with_site.SetSite(&site) }.ok()?;
        Some(MenuSite(with_site))
    });
    let theme = luciddesk_menu::apply_scoped_theme(owner.0);
    unsafe {
        let menu = Menu(CreatePopupMenu()?);
        context
            .QueryContextMenu(
                menu.0,
                0,
                1,
                LAST_SHELL_MENU_COMMAND_ID,
                CMF_NORMAL | CMF_SYNCCASCADEMENU,
            )
            .ok()?;
        let messages = MenuMessages::install(owner, &context)?;
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
        let command = shell_menu_command_offset(chosen)
            .map(|offset| menu_invocation(owner, offset, Some(point)));
        drop(frame);
        drop(messages);
        drop(theme);
        let Some(mut command) = command else {
            return Ok(FolderMenuResult::Cancelled);
        };
        let directory = HSTRING::from(path.as_os_str());
        command.lpDirectoryW = PCWSTR(directory.as_ptr());
        let invoked = context.InvokeCommand((&raw const command).cast());
        if !crate::file_command::command_completed(invoked)? {
            return Ok(FolderMenuResult::Cancelled);
        }
        drop(site_binding);
        Ok(FolderMenuResult::Invoked {
            created: created.borrow_mut().take(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{Win32::System::Ole::IObjectWithSite_Impl, core::Ref};

    #[implement(IObjectWithSite)]
    struct SiteRecorder(Rc<RefCell<Vec<bool>>>);

    impl IObjectWithSite_Impl for SiteRecorder_Impl {
        fn SetSite(&self, site: Ref<IUnknown>) -> Result<()> {
            self.0.borrow_mut().push(site.as_ref().is_some());
            Ok(())
        }

        fn GetSite(&self, _: *const GUID, _: *mut *mut std::ffi::c_void) -> Result<()> {
            Err(E_NOINTERFACE.into())
        }
    }

    #[test]
    fn menu_site_is_detached_when_the_guard_exits() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let context: IObjectWithSite = SiteRecorder(Rc::clone(&calls)).into();
        let client: INewMenuClient = NewClient(Rc::new(RefCell::new(None))).into();
        unsafe { context.SetSite(&client) }.unwrap();
        let site = MenuSite(context);
        assert_eq!(*calls.borrow(), [true]);
        drop(site);
        assert_eq!(*calls.borrow(), [true, false]);
    }
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
