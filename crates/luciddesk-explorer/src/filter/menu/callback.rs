//! Adapter for the presenter's unpublished callback interface. All ABI calls
//! are gated by QueryInterface; command IDs come from QueryContextMenu at runtime.
use std::{
    cell::Cell,
    ffi::c_void,
    rc::Rc,
    sync::atomic::{AtomicU32, Ordering},
};
use windows::{
    Win32::{
        Foundation::{E_POINTER, HWND, POINT},
        UI::{Shell::IContextMenu, WindowsAndMessaging::HMENU},
    },
    core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface, Result},
};
const IID: GUID = GUID::from_u128(0x9a19ddcf_9ed4_4a18_89de_c3b4dd1d7ae3);
#[repr(C)]
struct Vtable {
    base: IUnknown_Vtbl,
    invoke: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, HMENU, u32, POINT),
    dismiss: unsafe extern "system" fn(*mut c_void),
    expanded: unsafe extern "system" fn(*mut c_void, POINT, i32, *mut GUID, *mut POINT, i32),
    focus: unsafe extern "system" fn(*mut c_void),
}
#[repr(C)]
struct Callback {
    vtable: *const Vtable,
    refs: AtomicU32,
    inner: IUnknown,
    host: HWND,
    desktop: HWND,
    first: Rc<Cell<Option<u32>>>,
    cancelled: Rc<dyn Fn() -> bool>,
}
impl Callback {
    unsafe fn inner_vtable(&self) -> &Vtable {
        unsafe { &**(self.inner.as_raw() as *const *const Vtable) }
    }
}
static VTABLE: Vtable = Vtable {
    base: IUnknown_Vtbl {
        QueryInterface: query,
        AddRef: add_ref,
        Release: release,
    },
    invoke,
    dismiss,
    expanded,
    focus,
};
pub fn create(
    view: &windows::Win32::UI::Shell::IShellView,
    host: HWND,
    desktop: HWND,
    first: Rc<Cell<Option<u32>>>,
    cancelled: Rc<dyn Fn() -> bool>,
) -> Result<IUnknown> {
    unsafe {
        let unknown: IUnknown = view.cast()?;
        let mut raw = std::ptr::null_mut();
        (unknown.vtable().QueryInterface)(unknown.as_raw(), &IID, &raw mut raw).ok()?;
        let value = Box::new(Callback {
            vtable: &VTABLE,
            refs: AtomicU32::new(1),
            inner: IUnknown::from_raw(raw),
            host,
            desktop,
            first,
            cancelled,
        });
        Ok(IUnknown::from_raw(Box::into_raw(value).cast()))
    }
}
unsafe extern "system" fn query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    unsafe {
        if out.is_null() {
            return E_POINTER;
        }
        *out = std::ptr::null_mut();
        if iid.is_null() {
            return E_POINTER;
        }
        if *iid == IID || *iid == IUnknown::IID {
            *out = this;
            add_ref(this);
            return HRESULT(0);
        }
        let value = &*(this as *const Callback);
        (value.inner.vtable().QueryInterface)(value.inner.as_raw(), iid, out)
    }
}
unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
    unsafe {
        (*(this as *const Callback))
            .refs
            .fetch_add(1, Ordering::Relaxed)
            + 1
    }
}
unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    unsafe {
        let remaining = (*(this as *const Callback))
            .refs
            .fetch_sub(1, Ordering::AcqRel)
            - 1;
        if remaining == 0 {
            drop(Box::from_raw(this as *mut Callback));
        }
        remaining
    }
}
unsafe extern "system" fn invoke(
    this: *mut c_void,
    menu: *mut c_void,
    items: *mut c_void,
    hmenu: HMENU,
    command: u32,
    point: POINT,
) {
    unsafe {
        let value = &*(this as *const Callback);
        if (value.cancelled)() {
            return;
        }
        let rename = IContextMenu::from_raw_borrowed(&menu).is_some_and(|menu| {
            super::commands::is_rename_command(menu, value.first.get(), command)
        });
        if rename {
            // Complete the native view's menu session without starting an editor
            // inside the clipped Shell view. Pane owns the visible rename UI.
            (value.inner_vtable().dismiss)(value.inner.as_raw());
            windows_sys::Win32::UI::WindowsAndMessaging::SetPropW(
                value.desktop.0,
                super::super::RENAME,
                1usize as _,
            );
        } else {
            (value.inner_vtable().invoke)(value.inner.as_raw(), menu, items, hmenu, command, point);
        }
    }
}
unsafe extern "system" fn dismiss(this: *mut c_void) {
    unsafe {
        let value = &*(this as *const Callback);
        (value.inner_vtable().dismiss)(value.inner.as_raw());
        if !(value.cancelled)() {
            super::mark_popup_closed(value.host.0);
        }
    }
}

struct Expansion(HWND);
impl Drop for Expansion {
    fn drop(&mut self) {
        unsafe {
            // Explorer can post the follow-up popup after this call returns.
            windows_sys::Win32::UI::WindowsAndMessaging::SetPropW(
                self.0.0,
                super::MENU_EXPANDING,
                2usize as _,
            );
        }
    }
}

unsafe extern "system" fn expanded(
    this: *mut c_void,
    point: POINT,
    focus: i32,
    correlation: *mut GUID,
    position: *mut POINT,
    flags: i32,
) {
    unsafe {
        let value = &*(this as *const Callback);
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetPropW, SetPropW};
        // Building the classic menu can outlast the gap between its popups.
        let _expansion = (GetPropW(value.host.0, super::MENU_EXPANDING).is_null()
            && SetPropW(value.host.0, super::MENU_EXPANDING, 1usize as _) != 0)
            .then(|| Expansion(value.host));
        (value.inner_vtable().expanded)(
            value.inner.as_raw(),
            point,
            focus,
            correlation,
            position,
            flags,
        );
    }
}
unsafe extern "system" fn focus(this: *mut c_void) {
    unsafe {
        let value = &*(this as *const Callback);
        (value.inner_vtable().focus)(value.inner.as_raw());
    }
}
