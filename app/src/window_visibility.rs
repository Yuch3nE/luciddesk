//! Keep windows hidden until their native frame and content are ready.
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Finish creating a non-text canvas built with temporary WS_EX_NOACTIVATE.
/// The builder shows windows before returning, so input services must not attach
/// until we can remove the unused IME context. Native text editors keep theirs.
pub unsafe fn prepare_canvas(hwnd: windows_sys::Win32::Foundation::HWND) {
    unsafe {
        windows_sys::Win32::UI::Input::Ime::ImmAssociateContextEx(hwnd, std::ptr::null_mut(), 0);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE,
            GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & !(WS_EX_NOACTIVATE as isize));
    }
}

// windows-window::create unconditionally calls ShowWindow before returning.
// Handle this in the window procedure, before Windows commits visibility.
// Background notification windows pass false throughout their lifetime.
pub unsafe fn defer_show(message: u32, lparam: isize, prepared: bool) -> bool {
    if message != WM_WINDOWPOSCHANGING || prepared {
        return false;
    }
    unsafe {
        let position = &mut *(lparam as *mut WINDOWPOS);
        position.flags = (position.flags & !SWP_SHOWWINDOW) | SWP_NOACTIVATE;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_gate_preserves_placement_and_allows_prepared_show() {
        let mut position = WINDOWPOS {
            x: 120,
            y: 80,
            flags: SWP_SHOWWINDOW | SWP_FRAMECHANGED | SWP_NOZORDER,
            ..Default::default()
        };
        let pointer = &raw mut position as isize;
        assert!(!unsafe { defer_show(WM_NULL, 0, false) });
        assert!(!unsafe { defer_show(WM_WINDOWPOSCHANGING, pointer, true) });
        assert_ne!(position.flags & SWP_SHOWWINDOW, 0);
        assert!(unsafe { defer_show(WM_WINDOWPOSCHANGING, pointer, false) });
        assert_eq!(position.flags, SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOACTIVATE);
        assert_eq!((position.x, position.y), (120, 80));
    }

    #[test]
    fn builder_auto_show_never_makes_background_window_visible() {
        let attempted = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed = attempted.clone();
        let window = windows_window::Window::new("LucidDesk visibility test")
            .size(1, 1)
            .style(WS_POPUP)
            .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)
            .on_message(move |_, message, _, lp| {
                if unsafe { defer_show(message, lp, false) } {
                    observed.set(true);
                    return Some(0);
                }
                // Do not leave WM_QUIT in this test thread's queue.
                (message == WM_DESTROY).then_some(0)
            })
            .create()
            .unwrap();
        assert!(attempted.get());
        assert_eq!(unsafe { IsWindowVisible(window.hwnd().cast()) }, 0);
    }
}
