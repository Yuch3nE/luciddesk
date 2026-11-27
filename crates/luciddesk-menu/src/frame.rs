//! Outer popup padding through the native window frame, not synthetic menu items.
use std::{
    cell::{Cell, RefCell},
    ptr::null_mut,
    rc::Rc,
};
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
    System::Threading::GetCurrentThreadId,
    UI::{
        HiDpi::GetDpiForWindow,
        Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

const SUBCLASS: usize = 0x4c504d46;
thread_local! { static ACTIVE: Cell<*const FrameState> = const { Cell::new(std::ptr::null()) }; }

struct Popup {
    hwnd: HWND,
    work: RECT,
    padding: Cell<i32>,
    adjusted_height: Cell<i32>,
}

struct FrameState {
    padding: i32,
    work: RECT,
    // Keep subclass data stable for the lifetime of the native popup loop.
    popup: RefCell<Option<Rc<Popup>>>,
}

pub struct MenuFrame {
    hook: HHOOK,
    state: *const FrameState,
}

impl MenuFrame {
    pub fn install(owner: HWND, point: POINT) -> Option<Self> {
        if ACTIVE.get().is_null() {
            let mut monitor = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            unsafe {
                if GetMonitorInfoW(
                    MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST),
                    &raw mut monitor,
                ) == 0
                {
                    return None;
                }
            }
            let state = Rc::new(FrameState {
                work: monitor.rcWork,
                padding: unsafe { ((3 * GetDpiForWindow(owner).max(96) + 48) / 96) as i32 },
                popup: RefCell::new(None),
            });
            let hook = unsafe {
                SetWindowsHookExW(WH_CBT, Some(created), null_mut(), GetCurrentThreadId())
            };
            if !hook.is_null() {
                let state = Rc::into_raw(state);
                ACTIVE.set(state);
                return Some(Self { hook, state });
            }
        }
        None
    }
}

unsafe extern "system" fn created(code: i32, wp: usize, lp: isize) -> isize {
    unsafe {
        let pointer = ACTIVE.get();
        if code == HCBT_CREATEWND as i32 && !pointer.is_null() {
            let hwnd = wp as HWND;
            // The system popup class atom, scoped to this thread and popup loop.
            if GetClassLongPtrW(hwnd, GCW_ATOM) == 32768 {
                Rc::increment_strong_count(pointer);
                let state = Rc::from_raw(pointer);
                // Only the root menu needs this adjustment. Shell extensions
                // retain control over any submenus they create or reposition.
                if state.popup.borrow().is_some() {
                    return CallNextHookEx(null_mut(), code, wp, lp);
                }
                let popup = Rc::new(Popup {
                    hwnd,
                    work: state.work,
                    padding: Cell::new(state.padding),
                    adjusted_height: Cell::new(0),
                });
                // Keep a separate reference owned by the subclass registration.
                let registered = Rc::into_raw(Rc::clone(&popup));
                *state.popup.borrow_mut() = Some(popup);
                if SetWindowSubclass(hwnd, Some(frame), SUBCLASS, registered as usize) == 0 {
                    drop(Rc::from_raw(registered));
                    state.popup.borrow_mut().take();
                }
            }
        }
        CallNextHookEx(null_mut(), code, wp, lp)
    }
}

unsafe extern "system" fn frame(
    hwnd: HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    _: usize,
    data: usize,
) -> isize {
    unsafe {
        let registered = data as *const Popup;
        Rc::increment_strong_count(registered);
        let popup = Rc::from_raw(registered);
        if msg == WM_NCDESTROY {
            detach_popup(hwnd, registered);
        }
        if msg == WM_WINDOWPOSCHANGING && lp != 0 {
            let pos = &mut *(lp as *mut WINDOWPOS);
            let pad = popup.padding.get();
            if pos.flags & SWP_NOSIZE == 0 && pos.cy > 0 && pos.cy != popup.adjusted_height.get() {
                let work = popup.work;
                let height = pos.cy.saturating_add(pad * 2);
                // Full-height/scrolling menus keep the system frame. Do not
                // shrink their client area or hide their scrolling arrows.
                if height <= work.bottom - work.top {
                    pos.cy = height;
                    if pos.flags & SWP_NOMOVE == 0 {
                        pos.y = pos.y.clamp(work.top, work.bottom - height);
                    }
                } else {
                    popup.padding.set(0);
                }
                popup.adjusted_height.set(pos.cy);
            }
            // User32 sizes the popup first, then positions it in a separate
            // SWP_NOSIZE call. Clamp that move using the enlarged window height.
            if pos.flags & SWP_NOMOVE == 0 && popup.adjusted_height.get() > 0 {
                let work = popup.work;
                let height = popup.adjusted_height.get();
                if height <= work.bottom - work.top {
                    pos.y = pos.y.clamp(work.top, work.bottom - height);
                }
            }
        }
        let result = DefSubclassProc(hwnd, msg, wp, lp);
        if msg == WM_NCCALCSIZE && lp != 0 {
            // RECT is also the first field of NCCALCSIZE_PARAMS (wParam != 0).
            let rect = &mut *(lp as *mut RECT);
            let pad = popup.padding.get();
            if rect.bottom - rect.top > pad * 2 {
                rect.top += pad;
                rect.bottom -= pad;
            }
        }
        result
    }
}

unsafe fn detach_popup(hwnd: HWND, registered: *const Popup) {
    unsafe {
        if RemoveWindowSubclass(hwnd, Some(frame), SUBCLASS) != 0 {
            drop(Rc::from_raw(registered));
        }
    }
}

impl Drop for MenuFrame {
    fn drop(&mut self) {
        unsafe {
            ACTIVE.set(std::ptr::null());
            UnhookWindowsHookEx(self.hook);
            let state = Rc::from_raw(self.state);
            let popup = state.popup.borrow_mut().take();
            if let Some(popup) = popup {
                let registered = Rc::as_ptr(&popup);
                let mut data = 0;
                if GetWindowSubclass(popup.hwnd, Some(frame), SUBCLASS, &raw mut data) != 0
                    && data == registered as usize
                {
                    detach_popup(popup.hwnd, data as *const Popup);
                }
            }
        }
    }
}
