//! Short, event-driven opacity transitions shared by every pane type.
use super::{animation::{Fade, PANE_SHOW_DURATION}, composition::Surface};
use std::time::Instant;
use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};

const CLOSE: u32 = WM_APP + 198;
pub(super) const RESTORE: u32 = WM_APP + 196;
pub(super) const CLOSED: u32 = WM_APP + 197;
const TIMER: usize = 0x4c5056;
const CLOSE_STATE: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.CloseAnimation");

pub(super) fn request_close(hwnd: HWND) -> bool {
    let phase = unsafe { GetPropW(hwnd, CLOSE_STATE) } as usize;
    if phase == 1 { return true; }
    if phase == 2 || !super::scrollbar::animations_enabled() || unsafe { IsWindowVisible(hwnd) } == 0 {
        return false;
    }
    unsafe {
        if SetPropW(hwnd, CLOSE_STATE, 1usize as _) == 0 { return false; }
        if PostMessageW(hwnd, CLOSE, 0, 0) == 0 {
            RemovePropW(hwnd, CLOSE_STATE);
            return false;
        }
        windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(hwnd, 0);
    }
    true
}

fn finish_close(hwnd: HWND) {
    unsafe {
        SetPropW(hwnd, CLOSE_STATE, 2usize as _);
        windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(hwnd, 1);
        PostMessageW(hwnd, CLOSED, 0, 0);
    }
}

#[derive(Default)]
pub(super) struct Transition {
    pending: bool,
    closing: bool,
    from: f32,
    opacity: Option<f32>,
    track: Option<(Instant, Fade)>,
}

impl Transition {
    pub fn opacity(&self) -> f32 { self.opacity.unwrap_or(1.0) }

    pub fn message(&mut self, hwnd: HWND, msg: u32, wp: usize, lp: isize,
        surface: Option<&Surface>, mut companion: impl FnMut(f32)) -> bool {
        let flags = if msg == WM_WINDOWPOSCHANGING {
            unsafe { (*(lp as *const WINDOWPOS)).flags }
        } else { 0 };
        if msg == RESTORE {
            unsafe { RemovePropW(hwnd, CLOSE_STATE); }
            self.closing = false;
            self.complete(hwnd, surface, &mut companion);
            return true;
        }
        if msg == WM_DESTROY || flags & SWP_HIDEWINDOW != 0 {
            self.pending = false;
            self.track = None;
            unsafe { KillTimer(hwnd, TIMER); }
        }
        // Activation/z-order changes may repeat SHOWWINDOW for an already
        // visible HWND. Only a real hidden-to-visible transition starts a fade.
        let showing = flags & SWP_SHOWWINDOW != 0 && unsafe { IsWindowVisible(hwnd) } == 0;
        // Only request_close may initiate a close. Ignore unsolicited or stale
        // notifications rather than turning a broadcast into a persisted delete.
        if msg == CLOSE && (unsafe { GetPropW(hwnd, CLOSE_STATE) } as usize != 1 || self.closing) {
            return true;
        }
        if msg == CLOSE || (showing && !self.closing) {
            self.closing = msg == CLOSE;
            self.from = if self.closing { self.opacity() } else { 0.0 };
            self.track = None;
            self.pending = super::scrollbar::animations_enabled() && surface.is_some();
            if self.pending {
                self.opacity = Some(self.from);
                companion(self.from);
                self.pending = surface.unwrap().opacity(self.from).is_ok()
                    && unsafe { SetTimer(hwnd, TIMER, USER_TIMER_MINIMUM, None) } != 0;
            }
            if !self.pending { self.complete(hwnd, surface, &mut companion); }
            return msg == CLOSE;
        }
        if msg != WM_TIMER || wp != TIMER { return false; }
        if self.pending {
            self.pending = false;
            self.track = Fade::new(PANE_SHOW_DURATION).ok().map(|fade| (Instant::now(), fade));
        }
        let progress = self.track.as_ref().map_or(1.0, |(start, fade)|
            fade.sample(start.elapsed()).unwrap_or(1.0));
        let opacity = if self.closing { self.from * (1.0 - progress) } else { progress };
        self.opacity = Some(opacity);
        companion(opacity);
        let applied = surface.is_some_and(|surface| surface.opacity(opacity).is_ok());
        if progress >= 1.0 || !applied { self.complete(hwnd, surface, &mut companion); }
        true
    }

    fn complete(&mut self, hwnd: HWND, surface: Option<&Surface>, companion: &mut impl FnMut(f32)) {
        self.pending = false;
        self.track = None;
        unsafe { KillTimer(hwnd, TIMER); }
        let opacity = if self.closing { 0.0 } else { 1.0 };
        self.opacity = Some(opacity);
        companion(opacity);
        if let Some(surface) = surface { let _ = surface.opacity(opacity); }
        if self.closing {
            // Keep the window recoverable if persistence rejects the close.
            self.closing = false;
            finish_close(hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_notification_and_unsolicited_close_preserve_visibility() {
        assert!(![CLOSE, CLOSED, RESTORE].contains(&crate::i18n::CHANGED));
        let window = windows_window::Window::new("Language visibility regression")
            .size(1, 1).style(WS_POPUP)
            .on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0))
            .create().unwrap();
        let hwnd = window.hwnd().cast();
        let mut transition = Transition::default();
        assert!(!transition.message(hwnd, crate::i18n::CHANGED, 0, 0, None,
            |_| panic!("language notification changed opacity")));
        assert!(transition.message(hwnd, CLOSE, 0, 0, None,
            |_| panic!("unsolicited close changed opacity")));
        assert_eq!(transition.opacity(), 1.0);
        assert!(!transition.closing);
        assert!(unsafe { GetPropW(hwnd, CLOSE_STATE) }.is_null());
    }

    #[test]
    fn repeated_show_and_move_preserve_visible_surface_opacity() {
        let _sta = crate::pane::test_support::apartment();
        let window = windows_window::Window::new("Visibility regression")
            .size(96, 64).style(WS_POPUP)
            .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP)
            .create().unwrap();
        let hwnd = window.hwnd().cast();
        let surface = Surface::new_with_opacity(windows::Win32::Foundation::HWND(hwnd), 0.65).unwrap();
        let mut transition = Transition { opacity: Some(0.65), ..Default::default() };
        unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE); }
        for flags in [SWP_SHOWWINDOW, SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE, SWP_NOACTIVATE] {
            let mut position = WINDOWPOS { hwnd, flags, ..Default::default() };
            transition.message(hwnd, WM_WINDOWPOSCHANGING, 0,
                (&raw mut position) as isize, Some(&surface), |_| panic!("unexpected opacity change"));
            assert_eq!(transition.opacity(), 0.65);
            assert_eq!(surface.current_opacity(), 0.65);
            assert!(!transition.pending);
        }
        unsafe { ShowWindow(hwnd, SW_HIDE); }
        let mut position = WINDOWPOS { hwnd, flags: SWP_SHOWWINDOW, ..Default::default() };
        transition.message(hwnd, WM_WINDOWPOSCHANGING, 0,
            (&raw mut position) as isize, Some(&surface), |_| {});
        assert_eq!(transition.opacity(), if super::super::scrollbar::animations_enabled() { 0.0 } else { 1.0 });
    }
}
