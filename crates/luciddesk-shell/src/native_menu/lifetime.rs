//! Diagnostic `WinEvent` observer, restricted to the owning Explorer process.
//! This only records window show/hide and menu events; it does not send input.

// Diagnostics must never panic across a Windows callback when output is unavailable.
macro_rules! trace {
    ($($arg:tt)*) => {
        luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Trace, "shell.menu", $($arg)*);
    };
}
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_OBJECT_SHOW, EVENT_SYSTEM_MENUPOPUPSTART,
    GUITHREADINFO, GUI_INMENUMODE, GUI_POPUPMENUMODE, GetClassNameW, GetGUIThreadInfo, GetPropW,
    GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, OBJID_MENU, OBJID_WINDOW,
};
thread_local! { static POPUPS: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) }; }
thread_local! { static FIRST_VISIBLE: Cell<Option<Instant>> = const { Cell::new(None) }; }
thread_local! { static CLASSIC_SEEN: Cell<bool> = const { Cell::new(false) }; }

const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

type Hook = *mut c_void;
type Callback = unsafe extern "system" fn(Hook, u32, HWND, i32, i32, u32, u32);
#[link(name = "user32")]
unsafe extern "system" {
    fn SetWinEventHook(
        first: u32,
        last: u32,
        module: *mut c_void,
        callback: Option<Callback>,
        process: u32,
        thread: u32,
        flags: u32,
    ) -> Hook;
    fn UnhookWinEvent(hook: Hook) -> i32;
}

#[derive(Default)]
struct CloseBarrier {
    seen: bool,
    quiet_since: Option<Instant>,
}

impl CloseBarrier {
    fn closed(&mut self, now: Instant, observed: bool, visible: bool, menu_active: bool) -> bool {
        self.seen |= observed || visible;
        if visible || menu_active {
            self.quiet_since = None;
            return false;
        }
        self.seen
            && now.duration_since(*self.quiet_since.get_or_insert(now)) >= Duration::from_millis(200)
    }
}

pub struct Observer {
    hooks: Vec<Hook>,
    process: u32,
    thread: u32,
}
impl Observer {
    pub fn new(process: u32, thread: u32) -> windows::core::Result<Self> {
        POPUPS.with(|popups| popups.borrow_mut().clear());
        FIRST_VISIBLE.set(None);
        CLASSIC_SEEN.set(false);
        trace!("menu_observer_process={process}, thread={thread}");
        let mut result = Self {
            hooks: Vec::new(),
            process,
            thread,
        };
        for (first, last) in [(4, 7), (0x8000, 0x8003)] {
            let hook = unsafe {
                SetWinEventHook(
                    first,
                    last,
                    std::ptr::null_mut(),
                    Some(on_event),
                    process,
                    thread,
                    2,
                )
            };
            if hook.is_null() {
                return Err(windows::core::Error::from_thread());
            }
            result.hooks.push(hook);
        }
        Ok(result)
    }

    #[cfg(feature = "desktop-menu-diagnostics")]
    pub fn first_visible(&self) -> Option<Instant> {
        FIRST_VISIBLE.get()
    }

    pub fn wait_for_close(&self, host: Option<HWND>) -> windows::core::Result<()> {
        debug_assert_eq!(
            self.hooks.len(),
            2,
            "The observer must stay registered while waiting"
        );
        let start = Instant::now();
        let mut close = CloseBarrier::default();
        let mut awaiting_classic_since = None;
        loop {
            if !pump_messages() {
                return Err(windows::core::Error::from_hresult(
                    windows::Win32::Foundation::E_ABORT,
                ));
            }
            let visible = POPUPS.with(|popups| {
                let popups = popups.borrow();
                popups.iter().any(|&hwnd| self.is_live_popup(hwnd))
            });
            let observed = FIRST_VISIBLE.get().is_some() || host.is_some_and(|hwnd| unsafe {
                !GetPropW(hwnd, windows_sys::w!("LucidDesk.Menu.Closed.v1")).is_null()
            });
            if (visible || observed) && !close.seen {
                trace!("native_popup_visible_after_ms={}", start.elapsed().as_millis());
            }
            let now = Instant::now();
            let expansion = host.map_or(0, |hwnd| unsafe {
                GetPropW(hwnd, windows_sys::w!("LucidDesk.Menu.Expanding.v1")) as usize
            });
            let awaiting_classic = expansion == 1
                || (expansion == 2 && !CLASSIC_SEEN.get()
                    && now.duration_since(*awaiting_classic_since.get_or_insert(now)) < OPEN_TIMEOUT);
            let menu_active = awaiting_classic || (!visible && self.in_native_menu_loop());
            if close.closed(now, observed, visible, menu_active) {
                trace!("native_popup_closed_after_ms={}", start.elapsed().as_millis());
                return Ok(());
            }
            if !close.seen && start.elapsed() > OPEN_TIMEOUT {
                if menu_active {
                    trace!("native_popup_still_opening=true");
                }
                trace!("native_popup_not_observed=true");
                return Err(windows::core::Error::from_hresult(
                    windows::Win32::Foundation::E_FAIL,
                ));
            }
            // Shell/WinUI can synchronously query the Pane during creation.
            // Wake for those calls immediately instead of delaying every
            // cross-thread round trip behind a fixed polling sleep.
            unsafe {
                use windows_sys::Win32::UI::WindowsAndMessaging::*;
                MsgWaitForMultipleObjectsEx(0, std::ptr::null(), 20, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            }
        }
    }

    fn in_native_menu_loop(&self) -> bool {
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        (unsafe { GetGUIThreadInfo(self.thread, &raw mut info) != 0 })
            && info.flags & (GUI_INMENUMODE | GUI_POPUPMENUMODE) != 0
    }

    fn is_live_popup(&self, hwnd: HWND) -> bool {
        unsafe {
            let mut process = 0;
            let thread = GetWindowThreadProcessId(hwnd, &raw mut process);
            if process != self.process || thread != self.thread || !has_visible_bounds(hwnd) {
                return false;
            }
            let mut name = [0u16; 256];
            let count = GetClassNameW(hwnd, name.as_mut_ptr(), 256);
            usize::try_from(count).is_ok_and(|count| {
                let name = String::from_utf16_lossy(&name[..count]);
                if name == "#32768" {
                    CLASSIC_SEEN.set(true);
                }
                is_menu_class(&name)
            })
        }
    }
}

fn has_visible_bounds(hwnd: HWND) -> bool {
    let mut rect = RECT::default();
    unsafe {
        IsWindowVisible(hwnd) != 0
            && GetWindowRect(hwnd, &raw mut rect) != 0
            && rect.right > rect.left
            && rect.bottom > rect.top
    }
}

fn is_menu_class(name: &str) -> bool {
    matches!(
        name,
        "Microsoft.UI.Content.PopupWindowSiteBridge" | "#32768"
    )
}

pub(super) fn pump_messages() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, PostQuitMessage, TranslateMessage, WM_QUIT,
    };
    unsafe {
        let mut message = MSG::default();
        // Paint/animation producers may keep the queue nonempty. Yield to popup
        // lifetime checks even under continuous input or render activity.
        for _ in 0..32 {
            if PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) == 0 {
                break;
            }
            if message.message == WM_QUIT {
                PostQuitMessage(message.wParam as i32);
                return false;
            }
            TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    true
}
impl Drop for Observer {
    fn drop(&mut self) {
        for &hook in &self.hooks {
            unsafe {
                UnhookWinEvent(hook);
            }
        }
    }
}
unsafe extern "system" fn on_event(
    _hook: Hook,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    thread: u32,
    time: u32,
) {
    if hwnd.is_null() || (event >= 0x8000 && (object != OBJID_WINDOW || child != 0)) {
        return;
    }
    if matches!(event, EVENT_OBJECT_HIDE | EVENT_OBJECT_DESTROY) {
        POPUPS.with(|popups| popups.borrow_mut().retain(|popup| *popup != hwnd));
    }
    let mut name = [0u16; 256];
    let count = unsafe { GetClassNameW(hwnd, name.as_mut_ptr(), 256) };
    if let Ok(count) = usize::try_from(count) {
        let name = String::from_utf16_lossy(&name[..count]);
        // The system popup-start event remains evidence after a fast dismissal.
        // An object-show event for a zero-sized XAML bridge does not.
        let popup_start = event == EVENT_SYSTEM_MENUPOPUPSTART
            && child == 0 && matches!(object, OBJID_WINDOW | OBJID_MENU);
        let menu_window = is_menu_class(&name);
        if popup_start || (event == EVENT_OBJECT_SHOW && menu_window && has_visible_bounds(hwnd)) {
            if FIRST_VISIBLE.get().is_none() {
                FIRST_VISIBLE.set(Some(Instant::now()));
            }
            // System popup-start can arrive after a classic popup was destroyed.
            if name == "#32768" || (popup_start && name.is_empty()) {
                CLASSIC_SEEN.set(true);
            }
        }
        if (popup_start || event == EVENT_OBJECT_SHOW) && menu_window {
            POPUPS.with(|popups| {
                let mut popups = popups.borrow_mut();
                if !popups.contains(&hwnd) {
                    popups.push(hwnd);
                }
            });
        }
        trace!(
            "menu_window_event=0x{event:x}, hwnd={hwnd:?}, class={name}, thread={thread}, time={time}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_barrier_observes_fast_dismissal_and_waits_through_expansion() {
        let start = Instant::now();
        let mut close = CloseBarrier::default();
        assert!(!close.closed(start, false, false, false));
        assert!(!close.closed(start, true, false, false));
        let expansion = start + Duration::from_millis(150);
        assert!(!close.closed(expansion, false, false, true));
        let rebuilt = expansion + Duration::from_secs(1);
        assert!(!close.closed(rebuilt, false, false, true));
        assert!(!close.closed(rebuilt, false, true, false));
        assert!(!close.closed(rebuilt, false, false, false));
        assert!(close.closed(rebuilt + Duration::from_millis(200), false, false, false));
    }
}
