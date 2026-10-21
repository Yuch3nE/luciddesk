//! Session-only desktop presentation. Never change persisted panel geometry.
use super::{PaneApp, Rc, RefCell, WorkspaceStore};
use std::cell::Cell;
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::HWND,
    System::SystemInformation::GetTickCount,
    UI::WindowsAndMessaging::{
        GetPropW, IsIconic, IsWindowVisible, RemovePropW, SW_HIDE, SW_SHOWMINNOACTIVE,
        SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetPropW, ShowWindow, SendMessageW, WINDOWPOS,
        WM_WINDOWPOSCHANGING,
    },
};

pub(super) const HIDDEN_WINDOW: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.DesktopHidden.v1");
pub(super) const PREFERENCE: &str = "desktop_double_click_hide";
// This is a direct gesture, not a command to replay after a busy desktop recovers.
const REQUEST_LIFETIME_MS: u32 = 1_000;
thread_local! {
    static HIDDEN: Cell<bool> = const { Cell::new(false) };
    static REQUESTED: Cell<Option<u32>> = const { Cell::new(None) };
    static CONFIGURED: Cell<Option<(usize, bool)>> = const { Cell::new(None) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
    static RESTORE_PENDING: Cell<bool> = const { Cell::new(false) };
    static RETRY: Cell<Option<Instant>> = const { Cell::new(None) };
}
pub(super) fn hidden() -> bool {
    HIDDEN.with(Cell::get)
}
pub(super) fn connection_changed() {
    cancel_request("connection changed");
    CONFIGURED.with(|configured| configured.set(None));
}
pub(super) fn pending() -> bool {
    REQUESTED.with(Cell::get).is_some() || RESTORE_PENDING.with(Cell::get)
}
pub(super) fn next_work() -> Option<Duration> {
    pending().then(|| {
        RETRY
            .with(Cell::get)
            .map_or(Duration::from_millis(25), |due| {
                due.saturating_duration_since(Instant::now())
                    .max(Duration::from_millis(25))
            })
    })
}
pub(super) fn enabled(store: &WorkspaceStore) -> bool {
    store.preference(PREFERENCE).ok().flatten().as_deref() == Some("1")
}

fn windows(state: &Rc<RefCell<PaneApp>>) -> Vec<HWND> {
    let s = state.borrow();
    s.views
        .iter()
        .map(|v| v.window.hwnd().cast())
        .collect()
}

pub(super) fn request(time: u32) {
    request_at(time, unsafe { GetTickCount() });
}

fn request_at(time: u32, now: u32) {
    expire_request(now);
    let delay = now.wrapping_sub(time);
    if delay > REQUEST_LIFETIME_MS {
        luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Info,
            "desktop.visibility", &format!("discard double-click: expired on receipt; delay_ms={delay}"));
        return;
    }
    let queued = REQUESTED.with(|pending| {
        let next = pending.get().is_none().then_some(time);
        pending.set(next);
        next.is_some()
    });
    luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Info,
        "desktop.visibility", &format!("double-click received; delay_ms={delay}; queued={queued}"));
}

fn cancel_request(reason: &str) {
    if REQUESTED.with(|pending| pending.take()).is_some() {
        luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Info,
            "desktop.visibility", &format!("discard double-click: {reason}"));
    }
}

fn expire_request(now: u32) {
    if let Some(time) = REQUESTED.with(Cell::get) {
        let delay = now.wrapping_sub(time);
        if delay > REQUEST_LIFETIME_MS {
            REQUESTED.with(|pending| pending.set(None));
            luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Info,
                "desktop.visibility", &format!("discard double-click: expired while pending; delay_ms={delay}"));
        }
    }
}

/// Gate every pane's native show path, including newly created/rebuilt windows.
pub(super) unsafe fn gate(hwnd: HWND, msg: u32, lp: isize) {
    if msg == WM_WINDOWPOSCHANGING && hidden() {
        unsafe {
            let position = &mut *(lp as *mut WINDOWPOS);
            if position.flags & SWP_SHOWWINDOW != 0 {
                if GetPropW(hwnd, HIDDEN_WINDOW).is_null()
                    && SetPropW(hwnd, HIDDEN_WINDOW, 1usize as _) == 0
                {
                    return;
                }
                position.flags = (position.flags & !SWP_SHOWWINDOW) | SWP_NOACTIVATE;
            }
        }
    }
}

fn hide_windows(windows: &[HWND]) {
    for &hwnd in windows {
        unsafe {
            if IsWindowVisible(hwnd) != 0 {
                // Preserve minimization as well as panels that were already hidden.
                let state = if IsIconic(hwnd) != 0 { 2usize } else { 1 };
                if SetPropW(hwnd, HIDDEN_WINDOW, state as _) != 0 {
                    super::rename::cancel(hwnd);
                    if state == 2 { ShowWindow(hwnd, SW_HIDE); }
                    else { SendMessageW(hwnd, super::visibility::DESKTOP_HIDE, 0, 0); }
                }
            }
        }
    }
}
fn restore_windows(windows: &[HWND]) {
    for &hwnd in windows {
        unsafe {
            let previous = GetPropW(hwnd, HIDDEN_WINDOW) as usize;
            if previous != 0 {
                if previous == 1 && IsWindowVisible(hwnd) != 0 {
                    // Reverse an unfinished fade-out without hiding the HWND.
                    SendMessageW(hwnd, super::visibility::DESKTOP_SHOW, 0, 0);
                }
                ShowWindow(
                    hwnd,
                    if previous == 2 {
                        SW_SHOWMINNOACTIVE
                    } else {
                        SW_SHOWNOACTIVATE
                    },
                );
                // The marker preserves which windows belonged to this toggle.
                RemovePropW(hwnd, HIDDEN_WINDOW);
            }
        }
    }
}

/// Monitor children receive the desired presentation state before creating UI.
pub(super) fn child_visibility(hidden: bool, windows: &[HWND]) {
    HIDDEN.with(|value| value.set(hidden));
    if hidden {
        hide_windows(windows);
    } else {
        restore_windows(windows);
    }
}

/// Restore even if the desktop connection failed. Native session cleanup retains
/// responsibility for restoring icons when its last Shell request cannot finish.
pub(super) fn restore(state: &Rc<RefCell<PaneApp>>) -> Result<bool, String> {
    cancel_request("explicit restore or feature unavailable");
    restore_pending(state)
}

// Automatic retries must not discard clicks received since the last attempt.
fn restore_pending(state: &Rc<RefCell<PaneApp>>) -> Result<bool, String> {
    let was_hidden = HIDDEN.with(|hidden| hidden.replace(false));
    if !was_hidden && !RESTORE_PENDING.with(Cell::get) {
        return Ok(false);
    }
    let hook = super::hybrid::visibility_hook(&state.borrow());
    let result = match hook {
        Some((hook, true)) => {
            let result = hook.desktop_hidden(false);
            RESTORE_PENDING.with(|pending| pending.set(result.is_err()));
            RETRY.with(|retry| {
                retry.set(
                    result
                        .is_err()
                        .then(|| Instant::now() + Duration::from_secs(1)),
                );
            });
            result
        }
        Some((_, false)) => {
            RESTORE_PENDING.with(|pending| pending.set(true));
            Ok(())
        }
        None => {
            RESTORE_PENDING.with(|pending| pending.set(false));
            RETRY.with(|retry| retry.set(None));
            Ok(())
        }
    };
    restore_windows(&windows(state));
    if was_hidden {
        super::monitor::process::desktop_visibility(state, false)?;
    }
    if was_hidden {
        luciddesk_diagnostics::emit!(
            luciddesk_diagnostics::Level::Info,
            "desktop.visibility",
            "restore panels; icons_result={result:?}"
        );
    }
    result.map(|()| was_hidden)
}

struct Busy;
impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.with(|busy| busy.set(false));
    }
}

pub(super) fn maintain(state: &Rc<RefCell<PaneApp>>) -> Result<(), String> {
    if BUSY.with(|busy| busy.replace(true)) {
        return Ok(());
    }
    let _busy = Busy;
    expire_request(unsafe { GetTickCount() });
    if RETRY
        .with(Cell::get)
        .is_some_and(|due| due > Instant::now())
    {
        return Ok(());
    }
    if RESTORE_PENDING.with(Cell::get) {
        restore_pending(state)?;
        if RESTORE_PENDING.with(Cell::get) {
            return Ok(());
        }
    }
    let enabled = enabled(&state.borrow().store);
    let hook = super::hybrid::visibility_hook(&state.borrow());
    if !enabled || hook.is_none() {
        restore(state)?;
    }
    let Some((hook, ready)) = hook else {
        CONFIGURED.with(|configured| configured.set(None));
        return Ok(());
    };
    if !ready {
        return Ok(());
    }
    let signature = (Rc::as_ptr(&hook) as usize, enabled);
    if CONFIGURED.with(Cell::get) != Some(signature) {
        hook.desktop_gesture(enabled)?;
        CONFIGURED.with(|configured| configured.set(Some(signature)));
    }
    if !enabled {
        return Ok(());
    }
    let now = unsafe { GetTickCount() };
    expire_request(now);
    let Some(time) = REQUESTED.with(|pending| pending.take()) else {
        return Ok(());
    };
    luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Info,
        "desktop.visibility", &format!("execute double-click; action={}; delay_ms={}",
        if hidden() { "restore" } else { "hide" }, now.wrapping_sub(time)));
    if hidden() {
        restore(state)?;
    } else {
        if let Err(error) = hook.desktop_hidden(true) {
            let rollback = hook.desktop_hidden(false);
            if rollback.is_err() {
                RESTORE_PENDING.with(|pending| pending.set(true));
                RETRY.with(|retry| retry.set(Some(Instant::now() + Duration::from_secs(1))));
            }
            return Err(format!("{error}; restore={rollback:?}"));
        }
        HIDDEN.with(|hidden| hidden.set(true));
        hide_windows(&windows(state));
        super::monitor::process::desktop_visibility(state, true)?;
        luciddesk_diagnostics::emit!(
            luciddesk_diagnostics::Level::Info,
            "desktop.visibility",
            "hide icons and panels"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_DESTROY, WS_POPUP};
    #[test]
    fn stale_receipt_does_not_cancel_a_fresh_request_and_fresh_pairs_cancel() {
        REQUESTED.with(|pending| pending.set(None));
        request_at(100, 110);
        request_at(100u32.wrapping_sub(REQUEST_LIFETIME_MS + 1), 110);
        assert_eq!(REQUESTED.with(Cell::get), Some(100));
        request_at(120, 130);
        assert_eq!(REQUESTED.with(Cell::get), None);
        request_at(140, 150);
        connection_changed();
        assert!(!pending());
        let report = luciddesk_diagnostics::recent_report();
        assert!(report.contains("double-click received; delay_ms=10"));
        assert!(report.contains("expired on receipt"));
        assert!(report.contains("connection changed"));
    }

    #[test]
    fn expired_request_cannot_cancel_a_new_click_and_tick_wrap_is_supported() {
        REQUESTED.with(|pending| pending.set(None));
        request_at(100, 100);
        request_at(1_101, 1_101);
        assert_eq!(REQUESTED.with(Cell::get), Some(1_101));
        expire_request(2_102);
        assert!(!pending());
        request_at(u32::MAX - 100, 20);
        assert_eq!(REQUESTED.with(Cell::get), Some(u32::MAX - 100));
        expire_request(899);
        assert!(pending(), "the one-second boundary remains valid");
        expire_request(900);
        assert!(!pending());
    }

    #[test]
    fn restore_retry_discards_expired_toggle_before_waiting() {
        let state = Rc::new(RefCell::new(super::super::tests::test_state()));
        let now = unsafe { GetTickCount() };
        request_at(now.wrapping_sub(REQUEST_LIFETIME_MS + 1), now.wrapping_sub(1));
        RESTORE_PENDING.with(|pending| pending.set(true));
        RETRY.with(|retry| retry.set(Some(Instant::now() + Duration::from_secs(60))));
        maintain(&state).unwrap();
        assert!(REQUESTED.with(Cell::get).is_none());
        assert!(!hidden());
        assert!(RESTORE_PENDING.with(Cell::get), "restoration itself must still retry");
        restore_pending(&state).unwrap();
        assert!(!pending());
    }

    #[test]
    fn automatic_restore_preserves_new_click_but_explicit_restore_cancels_it() {
        let state = Rc::new(RefCell::new(super::super::tests::test_state()));
        RESTORE_PENDING.with(|value| value.set(true));
        request(unsafe { GetTickCount() });
        restore_pending(&state).unwrap();
        assert!(REQUESTED.with(Cell::get).is_some(), "retry must retain the new click");
        assert!(!RESTORE_PENDING.with(Cell::get));
        restore(&state).unwrap();
        assert!(REQUESTED.with(Cell::get).is_none(), "explicit show/disable cancels queued toggles");
    }
    #[test]
    fn restore_preserves_initial_hidden_panels_and_gates_new_windows() {
        let make = || {
            let mut transition = super::super::visibility::Transition::default();
            windows_window::Window::new("Desktop visibility fixture")
                .size(80, 60)
                .style(WS_POPUP)
                .on_message(move |hwnd, msg, wp, lp| {
                    if transition.message(hwnd.cast(), msg, wp, lp, None, |_| {}) { return Some(0); }
                    unsafe {
                        gate(hwnd.cast(), msg, lp);
                    }
                    (msg == WM_DESTROY).then_some(0)
                })
                .create()
                .unwrap()
        };
        let shown = make();
        let already_hidden = make();
        let hwnd = shown.hwnd().cast();
        let other = already_hidden.hwnd().cast();
        unsafe {
            ShowWindow(other, SW_HIDE);
        }
        child_visibility(true, &[hwnd, other]);
        let created = make();
        for window in [hwnd, other, created.hwnd().cast()] {
            assert_eq!(unsafe { IsWindowVisible(window) }, 0);
        }
        child_visibility(false, &[hwnd, other, created.hwnd().cast()]);
        assert_ne!(unsafe { IsWindowVisible(hwnd) }, 0);
        assert_eq!(unsafe { IsWindowVisible(other) }, 0);
        assert_ne!(unsafe { IsWindowVisible(created.hwnd().cast()) }, 0);
    }
    #[test]
    fn disabling_feature_restores_hidden_windows_without_a_desktop_connection() {
        let state = Rc::new(RefCell::new(super::super::tests::test_state()));
        assert!(!enabled(&state.borrow().store));
        child_visibility(true, &[]);
        assert!(hidden());
        maintain(&state).unwrap();
        assert!(!hidden());
        assert!(!pending());
    }
}
