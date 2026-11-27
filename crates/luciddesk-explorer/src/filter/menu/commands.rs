//! Keep rename in the Pane editor while forwarding Shell commands unchanged.
use windows::{
    Win32::{
        Foundation::{E_POINTER, HWND, LPARAM, LRESULT, WPARAM},
        System::Ole::{IObjectWithSite, IObjectWithSite_Impl},
        UI::{Shell::*, WindowsAndMessaging::HMENU},
    },
    core::{GUID, HRESULT, IUnknown, Interface, PSTR, Ref, Result, implement},
};
#[implement(IContextMenu3, IObjectWithSite)]
struct Menu {
    inner: IContextMenu,
    desktop: HWND,
    first: std::rc::Rc<std::cell::Cell<Option<u32>>>,
    cancelled: std::rc::Rc<dyn Fn() -> bool>,
}
// CDefView sites the menu before the presenter enumerates Explorer commands.
// Losing this interface leaves Open With's command without its host; shell32
// dereferences that null site in COpenWithExplorerCommand::EnumSubCommands.
impl IObjectWithSite_Impl for Menu_Impl {
    fn SetSite(&self, site: Ref<IUnknown>) -> Result<()> {
        unsafe { self.inner.cast::<IObjectWithSite>()?.SetSite(site.as_ref()) }
    }
    fn GetSite(&self, iid: *const GUID, out: *mut *mut std::ffi::c_void) -> Result<()> {
        unsafe {
            if out.is_null() {
                return Err(E_POINTER.into());
            }
            *out = std::ptr::null_mut();
            if iid.is_null() {
                return Err(E_POINTER.into());
            }
            let inner = self.inner.cast::<IObjectWithSite>()?;
            (inner.vtable().GetSite)(inner.as_raw(), iid, out).ok()
        }
    }
}
#[cfg(test)]
pub fn wrap(
    inner: IContextMenu,
    desktop: HWND,
    first: std::rc::Rc<std::cell::Cell<Option<u32>>>,
) -> IContextMenu {
    wrap_cancellable(inner, desktop, first, std::rc::Rc::new(|| false))
}
pub fn wrap_cancellable(
    inner: IContextMenu,
    desktop: HWND,
    first: std::rc::Rc<std::cell::Cell<Option<u32>>>,
    cancelled: std::rc::Rc<dyn Fn() -> bool>,
) -> IContextMenu {
    let menu: IContextMenu3 = Menu {
        inner,
        desktop,
        first,
        cancelled,
    }
    .into();
    menu.into()
}
pub(super) fn is_rename(menu: &IContextMenu, offset: usize) -> bool {
    let mut verb = [0u16; 128];
    unsafe {
        menu.GetCommandString(
            offset,
            GCS_VERBW,
            None,
            PSTR(verb.as_mut_ptr().cast()),
            verb.len() as u32,
        )
        .is_ok()
            && String::from_utf16_lossy(
                &verb[..verb.iter().position(|c| *c == 0).unwrap_or(verb.len())],
            )
            .eq_ignore_ascii_case("rename")
    }
}
pub(super) fn is_rename_command(menu: &IContextMenu, first: Option<u32>, command: u32) -> bool {
    first
        .and_then(|first| command.checked_sub(first))
        .is_some_and(|offset| is_rename(menu, offset as usize))
}
impl IContextMenu_Impl for Menu_Impl {
    fn QueryContextMenu(
        &self,
        menu: HMENU,
        index: u32,
        first: u32,
        last: u32,
        flags: u32,
    ) -> HRESULT {
        self.first.set(None);
        if (self.cancelled)() {
            return windows::Win32::Foundation::E_ABORT;
        }
        super::cursor::normal_pointer();
        let result = unsafe { self.inner.QueryContextMenu(menu, index, first, last, flags) };
        if (self.cancelled)() {
            return windows::Win32::Foundation::E_ABORT;
        }
        super::cursor::normal_pointer();
        if result.is_ok() {
            self.first.set(Some(first));
        }
        result
    }
    fn InvokeCommand(&self, info: *const CMINVOKECOMMANDINFO) -> Result<()> {
        if (self.cancelled)() {
            return Err(windows::Win32::Foundation::E_ABORT.into());
        }
        unsafe {
            if info.is_null() {
                return Err(E_POINTER.into());
            }
            let value = (*info).lpVerb.as_ptr() as usize;
            let rename = if value <= 0xffff {
                is_rename(&self.inner, value)
            } else {
                (*info).lpVerb.as_bytes().eq_ignore_ascii_case(b"rename")
            };
            if rename {
                windows_sys::Win32::UI::WindowsAndMessaging::SetPropW(
                    self.desktop.0,
                    super::super::RENAME,
                    1usize as _,
                );
                return Ok(());
            }
            self.inner.InvokeCommand(info)
        }
    }
    fn GetCommandString(
        &self,
        id: usize,
        kind: u32,
        reserved: *const u32,
        text: PSTR,
        max: u32,
    ) -> Result<()> {
        unsafe {
            self.inner
                .GetCommandString(id, kind, Some(reserved), text, max)
        }
    }
}
impl IContextMenu2_Impl for Menu_Impl {
    fn HandleMenuMsg(&self, msg: u32, wp: WPARAM, lp: LPARAM) -> Result<()> {
        unsafe {
            self.inner
                .cast::<IContextMenu2>()?
                .HandleMenuMsg(msg, wp, lp)
        }
    }
}
impl IContextMenu3_Impl for Menu_Impl {
    fn HandleMenuMsg2(&self, msg: u32, wp: WPARAM, lp: LPARAM, result: *mut LRESULT) -> Result<()> {
        unsafe {
            if let Ok(menu) = self.inner.cast::<IContextMenu3>() {
                menu.HandleMenuMsg2(msg, wp, lp, Some(result))
            } else if msg == windows::Win32::UI::WindowsAndMessaging::WM_MENUCHAR {
                Err(windows::Win32::Foundation::E_NOTIMPL.into())
            } else {
                if let Some(result) = result.as_mut() {
                    *result = LRESULT(0);
                }
                self.inner
                    .cast::<IContextMenu2>()?
                    .HandleMenuMsg(msg, wp, lp)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};
    use windows::core::PCSTR;

    #[implement(IContextMenu, IObjectWithSite)]
    struct Fixture {
        site: std::cell::RefCell<Option<IUnknown>>,
        invoked: Rc<Cell<usize>>,
        fail: Rc<Cell<bool>>,
    }
    impl IObjectWithSite_Impl for Fixture_Impl {
        fn SetSite(&self, site: Ref<IUnknown>) -> Result<()> {
            *self.site.borrow_mut() = site.as_ref().cloned();
            Ok(())
        }
        fn GetSite(&self, iid: *const GUID, out: *mut *mut std::ffi::c_void) -> Result<()> {
            let site = self.site.borrow();
            let site = site.as_ref().ok_or(windows::Win32::Foundation::E_FAIL)?;
            unsafe { (site.vtable().QueryInterface)(site.as_raw(), iid, out).ok() }
        }
    }
    #[test]
    fn wrapper_preserves_site_identity_replacement_and_release() {
        let inner: IContextMenu = Fixture {
            site: Default::default(),
            invoked: Rc::new(Cell::new(0)),
            fail: Rc::new(Cell::new(false)),
        }.into();
        let native_site: IObjectWithSite = inner.cast().unwrap();
        let menu = wrap(inner, HWND::default(), Rc::new(Cell::new(None)));
        let forwarded: IObjectWithSite = menu.cast().unwrap();
        unsafe {
            assert!(forwarded.GetSite::<IUnknown>().is_err());
            for _ in 0..2 {
                let host: IContextMenu = Fixture {
                    site: Default::default(),
                    invoked: Rc::new(Cell::new(0)),
                    fail: Rc::new(Cell::new(false)),
                }.into();
                forwarded.SetSite(&host).unwrap();
                assert_eq!(native_site.GetSite::<IUnknown>().unwrap(), host.cast::<IUnknown>().unwrap());
                assert_eq!(forwarded.GetSite::<IContextMenu>().unwrap(), host);
                assert_eq!(forwarded.cast::<IUnknown>().unwrap(), menu.cast::<IUnknown>().unwrap());
            }
            forwarded.SetSite(None::<&IUnknown>).unwrap();
            assert!(native_site.GetSite::<IUnknown>().is_err());
            assert!(forwarded.GetSite::<IUnknown>().is_err());
        }
    }
    impl IContextMenu_Impl for Fixture_Impl {
        fn QueryContextMenu(&self, _: HMENU, _: u32, _: u32, _: u32, _: u32) -> HRESULT {
            if self.fail.get() {
                windows::Win32::Foundation::E_FAIL
            } else {
                HRESULT(4)
            }
        }
        fn InvokeCommand(&self, _: *const CMINVOKECOMMANDINFO) -> Result<()> {
            self.invoked.set(self.invoked.get() + 1);
            Ok(())
        }
        fn GetCommandString(
            &self,
            id: usize,
            kind: u32,
            _: *const u32,
            text: PSTR,
            max: u32,
        ) -> Result<()> {
            if kind != GCS_VERBW || id > 3 {
                return Err(windows::Win32::Foundation::E_INVALIDARG.into());
            }
            let verb = if id == 2 { "rename" } else { "properties" };
            let value: Vec<u16> = verb.encode_utf16().chain(Some(0)).collect();
            assert!(max as usize >= value.len());
            unsafe {
                std::ptr::copy_nonoverlapping(value.as_ptr(), text.0.cast::<u16>(), value.len());
            }
            Ok(())
        }
    }
    #[test]
    fn rename_tracks_each_menu_id_range_and_rejects_missing_or_failed_ranges() {
        let fail = Rc::new(Cell::new(false));
        let inner: IContextMenu = Fixture {
            site: Default::default(),
            invoked: Rc::new(Cell::new(0)),
            fail: fail.clone(),
        }
        .into();
        let first = Rc::new(Cell::new(None));
        let menu = wrap(inner, HWND::default(), first.clone());
        for base in [1, 350, 0x7901] {
            unsafe {
                menu.QueryContextMenu(HMENU::default(), 0, base, base + 10, 0)
                    .ok()
                    .unwrap();
            }
            assert!(is_rename_command(&menu, first.get(), base + 2));
            assert!(!is_rename_command(&menu, first.get(), base + 3));
            assert!(!is_rename_command(&menu, first.get(), base - 1));
            assert!(!is_rename_command(&menu, None, base + 2));
        }
        fail.set(true);
        unsafe {
            assert!(
                menu.QueryContextMenu(HMENU::default(), 0, 500, 510, 0)
                    .is_err()
            );
        }
        assert_eq!(first.get(), None);
    }
    #[test]
    fn wrapper_forwards_other_commands_and_intercepts_canonical_rename() {
        let invoked = Rc::new(Cell::new(0));
        let inner: IContextMenu = Fixture {
            site: Default::default(),
            invoked: invoked.clone(),
            fail: Rc::new(Cell::new(false)),
        }
        .into();
        let menu = wrap(inner, HWND::default(), Rc::new(Cell::new(None)));
        unsafe {
            let mut info = CMINVOKECOMMANDINFO {
                cbSize: size_of::<CMINVOKECOMMANDINFO>() as u32,
                lpVerb: PCSTR(3usize as _),
                ..Default::default()
            };
            menu.InvokeCommand(&info).unwrap();
            assert_eq!(invoked.get(), 1);
            info.lpVerb = PCSTR(2usize as _);
            menu.InvokeCommand(&info).unwrap();
            assert_eq!(invoked.get(), 1);
            info.lpVerb = windows::core::s!("properties");
            menu.InvokeCommand(&info).unwrap();
            assert_eq!(invoked.get(), 2);
            assert!(menu.InvokeCommand(std::ptr::null()).is_err());
        }
    }

    #[test]
    fn cancellation_rejects_late_commands_and_new_menu_population() {
        let invoked = Rc::new(Cell::new(0));
        let cancelled = Rc::new(Cell::new(false));
        let token = cancelled.clone();
        let first = Rc::new(Cell::new(None));
        let menu = wrap_cancellable(
            Fixture {
                site: Default::default(),
                invoked: invoked.clone(),
                fail: Rc::new(Cell::new(false)),
            }
            .into(),
            HWND::default(),
            first.clone(),
            Rc::new(move || token.get()),
        );
        unsafe {
            menu.QueryContextMenu(HMENU::default(), 0, 1, 10, 0)
                .ok()
                .unwrap();
            cancelled.set(true);
            for verb in [
                PCSTR(2usize as _),
                PCSTR(3usize as _),
                windows::core::s!("properties"),
            ] {
                let info = CMINVOKECOMMANDINFO {
                    cbSize: size_of::<CMINVOKECOMMANDINFO>() as u32,
                    lpVerb: verb,
                    ..Default::default()
                };
                assert_eq!(
                    menu.InvokeCommand(&info).unwrap_err().code(),
                    windows::Win32::Foundation::E_ABORT
                );
            }
            assert_eq!(invoked.get(), 0);
            assert_eq!(
                menu.QueryContextMenu(HMENU::default(), 0, 1, 10, 0),
                windows::Win32::Foundation::E_ABORT
            );
            assert_eq!(first.get(), None);
        }
    }
}
