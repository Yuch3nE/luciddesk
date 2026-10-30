//! Coalesced worker notifications. The mutex also fences HWND destruction.
use std::sync::{Arc, Mutex};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

pub(super) const READY: u32 = WM_APP + 0x4c2;

/// windows-window detaches its callback during dispatch. COM may pump a READY
/// message in that interval; replay it only after the outer callback is restored.
pub(super) fn protect_dispatch(hwnd: isize) -> Result<(), String> {
    use std::{cell::Cell, rc::Rc};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::WM_NCDESTROY;

    #[derive(Default)]
    struct Dispatch {
        active: Cell<bool>,
        deferred: Cell<bool>,
        destroyed: Cell<bool>,
    }
    unsafe extern "system" fn dispatch(
        hwnd: windows_sys::Win32::Foundation::HWND, msg: u32, wp: usize, lp: isize,
        id: usize, data: usize,
    ) -> isize {
        // A nested WM_NCDESTROY can release the subclass's ownership while the
        // outer call is still running. Keep a reference for each invocation.
        let pointer = data as *const Dispatch;
        let state = unsafe { Rc::increment_strong_count(pointer); Rc::from_raw(pointer) };
        if msg == WM_NCDESTROY {
            state.destroyed.set(true);
            unsafe { RemoveWindowSubclass(hwnd, Some(dispatch), id); drop(Rc::from_raw(pointer)); }
        }
        if state.active.get() {
            if msg == READY {
                state.deferred.set(true);
                return 0;
            }
            return unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
        }
        state.active.set(true);
        let result = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
        state.active.set(false);
        if state.deferred.replace(false) && !state.destroyed.get() {
            // Repost directly: Wake still marks the swallowed message queued.
            unsafe { PostMessageW(hwnd, READY, 0, 0); }
        }
        result
    }
    let pointer = Rc::into_raw(Rc::new(Dispatch::default()));
    if unsafe { SetWindowSubclass(hwnd as _, Some(dispatch), READY as usize, pointer as usize) } == 0 {
        unsafe { drop(Rc::from_raw(pointer)); }
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

#[derive(Clone, Default)]
pub(super) struct Wake(Arc<Mutex<(isize, bool, bool)>>);

pub(super) struct NotifyOnDrop(Wake);
impl Drop for NotifyOnDrop {
    fn drop(&mut self) { self.0.notify(); }
}

impl Wake {
    pub fn on_drop(&self) -> NotifyOnDrop { NotifyOnDrop(self.clone()) }
    pub fn bind(&self, hwnd: isize) {
        *self.0.lock().unwrap() = (hwnd, false, false);
        self.notify();
    }

    pub fn unbind(&self) {
        *self.0.lock().unwrap() = (0, false, false);
    }

    pub fn received(&self) -> bool {
        let mut state = self.0.lock().unwrap();
        state.1 = false;
        std::mem::take(&mut state.2)
    }

    fn layout_changed(&self) {
        self.0.lock().unwrap().2 = true;
        self.notify();
    }

    /// Observe lifecycle and per-window DPI without borrowing PaneApp during
    /// synchronous destruction or layout callbacks.
    pub fn watch_window(&self, hwnd: isize) -> Result<(), String> {
        use windows_sys::Win32::UI::Shell::SetWindowSubclass;
        let data = Box::into_raw(Box::new(self.clone()));
        if unsafe { SetWindowSubclass(hwnd as _, Some(window_event), READY as usize, data as usize) } == 0 {
            unsafe { drop(Box::from_raw(data)); }
            return Err(crate::i18n::text("ui-could-not-register-panel-notifications").into());
        }
        Ok(())
    }

    /// Called only on the UI thread, outside any PaneApp borrow.
    pub fn refresh_hotkeys(&self) -> bool {
        let hwnd = self.0.lock().unwrap().0;
        hwnd != 0 && unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd as _, super::runtime::REFRESH_HOTKEYS, 0, 0)
        } == 1
    }

    pub fn notify(&self) {
        let mut state = self.0.lock().unwrap();
        if state.0 != 0 && !state.1 {
            state.1 = unsafe { PostMessageW(state.0 as _, READY, 0, 0) } != 0;
        }
    }
}

unsafe extern "system" fn window_event(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    id: usize,
    data: usize,
) -> isize {
    use windows_sys::Win32::UI::{Shell::*, WindowsAndMessaging::*};
    let wake = unsafe { &*(data as *const Wake) };
    if msg == WM_DPICHANGED {
        wake.layout_changed();
    } else if msg == WM_NCDESTROY {
        wake.notify();
        unsafe {
            RemoveWindowSubclass(hwnd, Some(window_event), id);
            drop(Box::from_raw(data as *mut Wake));
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    #[test]
    fn nested_dispatch_preserves_worker_wake_and_future_notifications() {
        let wake = Wake::default();
        let handler_wake = wake.clone();
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let observed = count.clone();
        let window = windows_window::Window::new("Nested wake regression")
            .style(WS_POPUP)
            .on_message(move |hwnd, msg, _, _| {
                if msg == WM_DESTROY { return Some(0); }
                if msg != READY { return None; }
                handler_wake.received();
                observed.set(observed.get() + 1);
                if observed.get() == 1 {
                    handler_wake.notify();
                    // Model a COM call pumping the next worker completion while
                    // windows-window has temporarily detached this callback.
                    unsafe {
                        let mut message = MSG::default();
                        assert_ne!(PeekMessageW(&raw mut message, hwnd, READY, READY, PM_REMOVE), 0);
                        DispatchMessageW(&message);
                    }
                }
                Some(0)
            }).create().unwrap();
        let hwnd = window.hwnd().cast();
        protect_dispatch(hwnd as isize).unwrap();
        wake.bind(hwnd as isize);
        let pump = || unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&raw mut message, hwnd, READY, READY, PM_REMOVE) != 0 {
                DispatchMessageW(&message);
            }
        };
        pump();
        assert_eq!(count.get(), 2, "nested worker completion must be delivered after the callback returns");
        wake.notify();
        pump();
        assert_eq!(count.get(), 3, "coalescing must not remain stuck after nested dispatch");
        wake.unbind();
    }

    #[test]
    fn worker_exit_wakes_after_disconnect_even_during_unwinding() {
        let wake = Wake::default();
        let window = windows_window::Window::new("Worker exit test").style(WS_POPUP).create().unwrap();
        let hwnd = window.hwnd().cast();
        let take = || unsafe {
            let mut message = MSG::default();
            PeekMessageW(&raw mut message, hwnd, READY, READY, PM_REMOVE) != 0
        };
        wake.bind(window.hwnd() as isize);
        assert!(take());
        wake.received();
        for panic in [false, true] {
            let ready = wake.clone();
            let (sender, receiver) = std::sync::mpsc::channel::<()>();
            let result = std::thread::spawn(move || {
                let _exit = ready.on_drop();
                let _sender = sender;
                assert!(!panic, "injected worker panic");
            }).join();
            assert_eq!(result.is_err(), panic);
            assert!(take());
            assert_eq!(receiver.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected));
            wake.received();
        }
        wake.unbind();
    }

    #[test]
    fn pane_dpi_and_destruction_notify_without_borrowing_application_state() {
        let wake = Wake::default();
        let supervisor = windows_window::Window::new("Lifecycle supervisor")
            .style(WS_POPUP)
            .on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0))
            .create().unwrap();
        let pane = windows_window::Window::new("Lifecycle pane")
            .style(WS_POPUP)
            .on_message(|_, msg, _, _| matches!(msg, WM_DESTROY | WM_DPICHANGED).then_some(0))
            .create().unwrap();
        wake.bind(supervisor.hwnd() as isize);
        wake.watch_window(pane.hwnd() as isize).unwrap();
        let take = || unsafe {
            let mut message = MSG::default();
            PeekMessageW(&raw mut message, supervisor.hwnd().cast(), READY, READY, PM_REMOVE) != 0
        };
        assert!(take());
        assert!(!wake.received());
        unsafe { SendMessageW(pane.hwnd().cast(), WM_DPICHANGED, 0, 0); }
        assert!(take());
        assert!(wake.received());
        drop(pane);
        assert!(take());
        assert!(!wake.received());
        wake.unbind();
    }

    #[test]
    fn worker_bursts_coalesce_and_notifications_stop_after_unbind() {
        let wake = Wake::default();
        let window = windows_window::Window::new("Wake test")
            .size(1, 1)
            .style(WS_POPUP)
            .create()
            .unwrap();
        let hwnd = window.hwnd().cast();
        unsafe {
            ShowWindow(hwnd, SW_HIDE);
        }
        let take = || unsafe {
            let mut msg = MSG::default();
            PeekMessageW(&raw mut msg, hwnd, READY, READY, PM_REMOVE) != 0
        };
        wake.bind(window.hwnd() as isize);
        let worker = wake.clone();
        std::thread::spawn(move || {
            for _ in 0..1000 {
                worker.notify();
            }
        })
        .join()
        .unwrap();
        assert!(take());
        assert!(!take());
        wake.received();
        wake.notify();
        assert!(take());
        wake.unbind();
        wake.notify();
        assert!(!take());
    }
}
