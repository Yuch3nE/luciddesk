//! Reveal panes once without changing the saved topmost preference.
use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};

pub(super) fn permanent_topmost(hwnd: HWND) -> bool {
    unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0 }
}

pub(super) fn show(hwnd: HWND) {
    super::window::raise_once(hwnd);
}

/// Shared by the tray and the optional global shortcut. Release the app borrow
/// before changing window order, because native calls can dispatch messages.
pub(super) fn show_all(state: &std::rc::Rc<std::cell::RefCell<super::PaneApp>>) {
    luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Info, "pane.desktop_state",
        "show_all requested; app_desktop_hidden={}", super::desktop_toggle::hidden());
    match super::desktop_toggle::restore(state) {
        Err(error) => luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Error, "desktop.visibility", "Reveal restore: {error}"),
        Ok(_) => {}
    }
    let windows: Vec<_> = state.borrow().views.iter().map(|v| v.window.hwnd()).collect();
    let monitors = state.borrow().monitors.windows();
    // Explicit reveal changes stacking, not the active window. In particular,
    // do not activate a Shell-owned pane before the user's next Show Desktop.
    let foreground = unsafe { GetForegroundWindow() };
    for hwnd in monitors { show(hwnd); }
    for &hwnd in &windows { show(hwnd.cast()); }
    luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Info, "pane.desktop_state",
        "show_all no-activate; foreground_before={foreground:?} foreground_after={:?}",
        unsafe { GetForegroundWindow() });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reveal_preserves_topmost_and_allows_other_windows_to_cover_it() {
        let make = || windows_window::Window::new("LucidDesk reveal test")
            .size(100, 100).style(WS_POPUP).ex_style(WS_EX_TOOLWINDOW)
            .create().unwrap();
        let pane = make();
        let other = make();
        let hwnd = pane.hwnd().cast();
        let other_hwnd = other.hwnd().cast();
        super::super::window::set_layer(hwnd, false);
        show(hwnd);
        assert!(!permanent_topmost(hwnd));
        unsafe {
            ShowWindow(other_hwnd, SW_SHOWNOACTIVATE);
            SetWindowPos(other_hwnd, HWND_TOP, 0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            let mut next = GetWindow(other_hwnd, GW_HWNDNEXT);
            while !next.is_null() && next != hwnd { next = GetWindow(next, GW_HWNDNEXT); }
            assert_eq!(next, hwnd, "another ordinary window can cover the revealed pane");
        }
        show(hwnd);
        assert!(!permanent_topmost(hwnd));
        unsafe {
            let mut next = GetWindow(hwnd, GW_HWNDNEXT);
            while !next.is_null() && next != other_hwnd { next = GetWindow(next, GW_HWNDNEXT); }
            assert_eq!(next, other_hwnd, "explicit reveal must actually rise above an ordinary window");
        }
        super::super::window::set_layer(hwnd, true);
        show(hwnd);
        assert!(permanent_topmost(hwnd));
        super::super::window::set_layer(hwnd, false);
    }
}
