//! Session-only desktop presentation. Never change persisted panel geometry.
use super::{PaneApp, Rc, RefCell, WorkspaceStore};
use std::cell::Cell;
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        GetPropW, IsIconic, IsWindowVisible, RemovePropW, SW_HIDE, SW_SHOWMINNOACTIVE,
        SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetPropW, ShowWindow, WINDOWPOS,
        WM_WINDOWPOSCHANGING,
    },
};

pub(super) const HIDDEN_WINDOW: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.DesktopHidden.v1");
pub(super) const PREFERENCE: &str = "desktop_double_click_hide";
thread_local! {
    static HIDDEN: Cell<bool> = const { Cell::new(false) };
    static REQUESTED: Cell<bool> = const { Cell::new(false) };
    static CONFIGURED: Cell<Option<(usize, bool)>> = const { Cell::new(None) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
    static RESTORE_PENDING: Cell<bool> = const { Cell::new(false) };
    static RETRY: Cell<Option<Instant>> = const { Cell::new(None) };
}
pub(super) fn hidden() -> bool {
    HIDDEN.with(Cell::get)
}
pub(super) fn restoring(hwnd: HWND) -> bool {
    !hidden() && unsafe { !GetPropW(hwnd, HIDDEN_WINDOW).is_null() }
}
pub(super) fn connection_changed() {
    CONFIGURED.with(|configured| configured.set(None));
}
pub(super) fn pending() -> bool {
    REQUESTED.with(Cell::get) || RESTORE_PENDING.with(Cell::get)
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

pub(super) fn request(state: &Rc<RefCell<PaneApp>>) {
    REQUESTED.with(|requested| requested.set(!requested.get()));
    state.borrow().wake.notify();
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
                    ShowWindow(hwnd, SW_HIDE);
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
                ShowWindow(
                    hwnd,
                    if previous == 2 {
                        SW_SHOWMINNOACTIVE
                    } else {
                        SW_SHOWNOACTIVATE
                    },
                );
                // Keep the marker through WM_WINDOWPOSCHANGING so the pane
                // restores its existing surface without replaying its entrance.
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
    REQUESTED.with(|pending| pending.set(false));
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
    if !enabled || !REQUESTED.with(|pending| pending.replace(false)) {
        return Ok(());
    }
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
    fn automatic_restore_preserves_new_click_but_explicit_restore_cancels_it() {
        let state = Rc::new(RefCell::new(super::super::tests::test_state()));
        RESTORE_PENDING.with(|value| value.set(true));
        request(&state);
        restore_pending(&state).unwrap();
        assert!(REQUESTED.with(Cell::get), "retry must retain the new click");
        assert!(!RESTORE_PENDING.with(Cell::get));
        restore(&state).unwrap();
        assert!(!REQUESTED.with(Cell::get), "explicit show/disable cancels queued toggles");
    }
    #[test]
    fn restore_preserves_initial_hidden_panels_and_gates_new_windows() {
        let make = || {
            windows_window::Window::new("Desktop visibility fixture")
                .size(80, 60)
                .style(WS_POPUP)
                .on_message(|hwnd, msg, _, lp| {
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
