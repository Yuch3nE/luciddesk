//! Temporary icon visibility and blank-area gestures on Explorer's existing STA.
use windows::core::Result;
use windows_sys::Win32::{
    Foundation::{HWND, POINT},
    Graphics::Gdi::{ClientToScreen, ScreenToClient, RedrawWindow, RDW_INVALIDATE, RDW_UPDATENOW, RDW_ALLCHILDREN},
    System::SystemServices::{MK_CONTROL, MK_SHIFT},
    UI::{
        Controls::{LVHITTESTINFO, LVM_GETEDITCONTROL, LVM_HITTEST},
        Input::KeyboardAndMouse::{GetDoubleClickTime, GetKeyState, VK_MENU},
        WindowsAndMessaging::{
            GA_ROOT, GetAncestor, GetParent, GetSystemMetrics, IsWindow, IsWindowVisible, MSG,
            ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE,
            GetCursor, GetCursorPos, LoadCursorW, SetCursor, WindowFromPoint,
            IDC_ARROW, IDC_WAIT, IDC_APPSTARTING,
            GetWindowLongW, GWL_STYLE, WS_VISIBLE,
            SM_CXDOUBLECLK, SM_CXDRAG, SM_CYDOUBLECLK, SM_CYDRAG, SendMessageW, WM_LBUTTONDBLCLK,
            WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_RBUTTONDOWN,
            WM_KEYDOWN, WM_SYSKEYDOWN, WM_MOUSEWHEEL, WM_MOUSEHWHEEL, WM_XBUTTONDOWN, WM_SETREDRAW,
        },
    },
};

#[derive(Default)]
pub(super) struct DesktopVisibility {
    enabled: bool,
    original: Option<bool>,
    clicks: Clicks,
}

impl DesktopVisibility {
    pub fn cancel_gesture(&mut self) {
        self.clicks = Clicks::default();
    }
    pub fn enable(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.cancel_gesture();
    }

    /// A recognized blank-area pair belongs to us. Letting Shell dispatch its
    /// DBLCLK as well can run its default activation path before our second UP.
    pub fn consumes_double_click(&self, message: u32) -> bool {
        message == WM_LBUTTONDBLCLK && self.clicks.down.is_some_and(|(_, _, second)| second)
    }

    pub fn hide(&mut self, view: HWND) -> Result<()> {
        unsafe {
            if IsWindow(view) == 0 {
                return Err(windows::Win32::Foundation::E_HANDLE.into());
            }
            self.original.get_or_insert(!own_visible(view));
            // This runs on Explorer's own UI thread. Hide only the ListView;
            // changing FWF_NOICONS makes Shell rebuild/refresh desktop content.
            ShowWindow(view, SW_HIDE);
            if own_visible(view) {
                return Err(windows::Win32::Foundation::E_FAIL.into());
            }
        }
        self.cancel_gesture();
        unsafe { restore_pointer(view); }
        Ok(())
    }

    pub fn restore(&mut self, view: HWND, icons_enabled: bool) -> Result<()> {
        if self.original == Some(false) && icons_enabled {
            unsafe {
                // A destroyed Explorer view needs no restoration. Never touch
                // a replacement desktop window owned by a new connection.
                if IsWindow(view) != 0 {
                    // Batch the ListView reveal so its old backing surface does not
                    // remain visible while panels are already fading in.
                    SendMessageW(view, WM_SETREDRAW, 0, 0);
                    ShowWindow(view, SW_SHOWNOACTIVATE);
                    SendMessageW(view, WM_SETREDRAW, 1, 0);
                    // Finish the icon view paint before acknowledging restoration.
                    RedrawWindow(view, std::ptr::null(), std::ptr::null_mut(),
                        RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN);
                    if !own_visible(view) {
                        return Err(windows::Win32::Foundation::E_FAIL.into());
                    }
                }
            }
        }
        self.original = None;
        self.cancel_gesture();
        unsafe { restore_pointer(view); }
        Ok(())
    }

    pub fn restore_from_shell(&mut self, view: HWND, folder: &windows::Win32::UI::Shell::IFolderView2) -> Result<()> {
        // Read the current Shell policy without changing it. The user may have
        // disabled desktop icons while our temporary window hide was active.
        let enabled = self.original != Some(false) || unsafe {
            IsWindow(view) == 0
                || folder.GetCurrentFolderFlags()? & windows::Win32::UI::Shell::FWF_NOICONS.0 as u32 == 0
        };
        self.restore(view, enabled)
    }

    pub unsafe fn message(&mut self, view: HWND, message: &MSG, blocked: bool) -> bool {
        if !self.enabled || blocked || interrupts_gesture(message.message) {
            self.cancel_gesture();
            return false;
        }
        if !observes(message.message) {
            return false;
        }
        if message.message == WM_MOUSEMOVE && self.clicks.down.is_none() {
            return false;
        }
        unsafe {
            // Hiding FolderView transfers mouse input to DefView or
            // its desktop host. Observe all three on the same existing hook.
            let desktop = message.hwnd == view
                || message.hwnd == GetParent(view)
                || message.hwnd == GetAncestor(view, GA_ROOT);
            if !desktop {
                self.cancel_gesture();
                return false;
            }
            if message.wParam & (MK_CONTROL | MK_SHIFT) as usize != 0
                || GetKeyState(i32::from(VK_MENU)) < 0
            {
                self.cancel_gesture();
                return false;
            }
            let packed = message.lParam.to_le_bytes();
            let mut screen = POINT {
                x: i32::from(i16::from_le_bytes([packed[0], packed[1]])),
                y: i32::from(i16::from_le_bytes([packed[2], packed[3]])),
            };
            if ClientToScreen(message.hwnd, &raw mut screen) == 0 {
                self.cancel_gesture();
                return false;
            }
            let mut pt = screen;
            if ScreenToClient(view, &raw mut pt) == 0 {
                self.cancel_gesture();
                return false;
            }
            // A visibility lease describes our restoration responsibility, not
            // the current UI: Explorer's menu may have shown icons meanwhile.
            let hidden = IsWindowVisible(view) == 0;
            let blank = hidden || {
                let mut hit = LVHITTESTINFO {
                    pt,
                    ..Default::default()
                };
                SendMessageW(view, LVM_HITTEST, 0, (&raw mut hit) as isize) == -1
                    && SendMessageW(view, LVM_GETEDITCONTROL, 0, 0) == 0
            };
            let limits = Limits {
                time: GetDoubleClickTime(),
                click_x: GetSystemMetrics(SM_CXDOUBLECLK).max(2) / 2,
                click_y: GetSystemMetrics(SM_CYDOUBLECLK).max(2) / 2,
                drag_x: GetSystemMetrics(SM_CXDRAG).max(2) / 2,
                drag_y: GetSystemMetrics(SM_CYDRAG).max(2) / 2,
            };
            self.clicks
                .message(message.message, message.time, screen, blank, limits)
        }
    }
}

// IsWindowVisible also tests ancestors. The lease changes only this HWND's
// WS_VISIBLE bit, so a temporarily hidden desktop host must not alter its snapshot.
unsafe fn own_visible(view: HWND) -> bool {
    unsafe { GetWindowLongW(view, GWL_STYLE) as u32 & WS_VISIBLE != 0 }
}

/// Visibility changes can leave Shell's previous busy cursor behind when the
/// window under the stationary pointer changes. Only finish our own desktop
/// interaction; never replace system cursors or another window's cursor.
unsafe fn restore_pointer(view: HWND) {
    unsafe {
        let mut point = POINT::default();
        if GetCursorPos(&raw mut point) == 0 { return; }
        let under = WindowFromPoint(point);
        let desktop = !under.is_null()
            && (under == view || under == GetParent(view) || under == GetAncestor(view, GA_ROOT));
        let current = GetCursor();
        let busy = current == LoadCursorW(std::ptr::null_mut(), IDC_WAIT)
            || current == LoadCursorW(std::ptr::null_mut(), IDC_APPSTARTING);
        if desktop && busy {
            SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_ARROW));
        }
    }
}

fn interrupts_gesture(message: u32) -> bool {
    matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN | WM_MOUSEWHEEL | WM_MOUSEHWHEEL | WM_XBUTTONDOWN)
}

pub(super) fn observes(message: u32) -> bool {
    interrupts_gesture(message)
        || matches!(message, WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_LBUTTONUP
            | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_MOUSEMOVE)
}

#[derive(Clone, Copy)]
struct Limits {
    time: u32,
    click_x: i32,
    click_y: i32,
    drag_x: i32,
    drag_y: i32,
}
#[derive(Default)]
struct Clicks {
    first: Option<(u32, POINT)>,
    down: Option<(u32, POINT, bool)>,
}
fn near(a: POINT, b: POINT, x: i32, y: i32) -> bool {
    a.x.abs_diff(b.x) <= x.unsigned_abs() && a.y.abs_diff(b.y) <= y.unsigned_abs()
}
impl Clicks {
    fn message(&mut self, msg: u32, time: u32, point: POINT, blank: bool, limits: Limits) -> bool {
        match msg {
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK if blank => {
                let second = self.first.is_some_and(|(at, first)| {
                    time.wrapping_sub(at) <= limits.time
                        && near(first, point, limits.click_x, limits.click_y)
                });
                self.down = Some((time, point, second));
            }
            WM_LBUTTONUP => {
                if let Some((at, down, second)) = self.down.take()
                    && blank
                    && time.wrapping_sub(at) <= limits.time
                    && near(down, point, limits.drag_x, limits.drag_y)
                {
                    self.first = if second { None } else { Some((at, down)) };
                    return second;
                }
                self.first = None;
            }
            WM_MOUSEMOVE => {
                if self
                    .down
                    .is_some_and(|(_, down, _)| !near(down, point, limits.drag_x, limits.drag_y))
                {
                    *self = Self::default();
                }
            }
            _ => *self = Self::default(),
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_blank_hit_testing_and_hidden_parent_routes_work_without_global_mouse_hooks() {
        use windows_sys::Win32::UI::{Controls::*, WindowsAndMessaging::*};
        unsafe {
            let common = INITCOMMONCONTROLSEX {
                dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_LISTVIEW_CLASSES,
            };
            assert_ne!(InitCommonControlsEx(&raw const common), 0);
            let root = CreateWindowExW(
                0,
                windows_sys::w!("STATIC"),
                windows_sys::w!("Desktop gesture fixture"),
                WS_POPUP,
                0,
                0,
                320,
                220,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            let parent = CreateWindowExW(
                0,
                windows_sys::w!("STATIC"),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                320,
                220,
                root,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            let view = CreateWindowExW(
                0,
                windows_sys::w!("SysListView32"),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE | LVS_ICON,
                0,
                0,
                320,
                220,
                parent,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!view.is_null());
            ShowWindow(root, SW_SHOWNOACTIVATE);
            let mut label: Vec<_> = "Fixture\0".encode_utf16().collect();
            let item = LVITEMW {
                mask: LVIF_TEXT,
                pszText: label.as_mut_ptr(),
                ..Default::default()
            };
            assert_eq!(
                SendMessageW(view, LVM_INSERTITEMW, 0, (&raw const item) as isize),
                0
            );
            let mut bounds = windows_sys::Win32::Foundation::RECT::default();
            assert_ne!(
                SendMessageW(view, LVM_GETITEMRECT, 0, (&raw mut bounds) as isize),
                0
            );
            let mut lease = DesktopVisibility::default();
            lease.hide(view).unwrap();
            lease.hide(view).unwrap();
            assert_eq!(IsWindowVisible(view), 0);
            lease.restore(view, true).unwrap();
            assert_ne!(IsWindowVisible(view), 0);
            ShowWindow(view, SW_HIDE);
            lease.hide(view).unwrap();
            lease.restore(view, true).unwrap();
            assert_eq!(IsWindowVisible(view), 0, "originally hidden icons stay hidden");
            lease.hide(view).unwrap();
            ShowWindow(view, SW_SHOWNOACTIVATE);
            lease.restore(view, true).unwrap();
            assert_ne!(IsWindowVisible(view), 0, "preserve external show when originally hidden");
            lease.hide(view).unwrap();
            lease.restore(view, false).unwrap();
            assert_eq!(IsWindowVisible(view), 0, "respect the user's current hide-icons policy");
            assert!(lease.original.is_none(), "release our lease without changing Shell policy");
            lease.restore(view, true).unwrap();
            assert_eq!(IsWindowVisible(view), 0, "later cleanup must not undo that decision");
            ShowWindow(view, SW_SHOWNOACTIVATE);
            for hide_parent_first in [true, false] {
                if hide_parent_first { ShowWindow(root, SW_HIDE); }
                assert!(own_visible(view));
                lease.hide(view).unwrap();
                if !hide_parent_first { ShowWindow(root, SW_HIDE); }
                lease.restore(view, true).unwrap();
                assert!(own_visible(view), "restore the child style even while its parent is hidden");
                assert_eq!(IsWindowVisible(view), 0);
                ShowWindow(root, SW_SHOWNOACTIVATE);
                assert_ne!(IsWindowVisible(view), 0, "icons must reappear with their parent");
            }
            let mut gesture = DesktopVisibility::default();
            gesture.enable(true);
            // Simulate icons shown manually while our hide lease is active.
            gesture.original = Some(false);
            let mut pair = |target, x: i32, y: i32| {
                let mut toggles = 0;
                let mut consumed = 0;
                for (msg, time) in [
                    (WM_LBUTTONDOWN, 10),
                    (WM_LBUTTONUP, 20),
                    (WM_LBUTTONDBLCLK, 100),
                    (WM_LBUTTONUP, 110),
                ] {
                    toggles += usize::from(gesture.message(
                        view,
                        &MSG {
                            hwnd: target,
                            message: msg,
                            lParam: ((x & 65535) | ((y & 65535) << 16)) as isize,
                            time,
                            ..Default::default()
                        },
                        false,
                    ));
                    consumed += usize::from(gesture.consumes_double_click(msg));
                }
                assert_eq!(consumed, toggles, "only recognized blank pairs suppress Shell's double-click");
                toggles
            };
            assert_eq!(pair(view, 290, 190), 1, "blank desktop must toggle");
            assert_eq!(
                pair(
                    view,
                    (bounds.left + bounds.right) / 2,
                    (bounds.top + bounds.bottom) / 2
                ),
                0,
                "desktop items must preserve their double click"
            );
            ShowWindow(view, SW_HIDE);
            assert_eq!(
                pair(parent, 290, 190),
                1,
                "hidden ListView must leave a parent restore route"
            );
            assert_eq!(pair(root, 290, 190), 1, "desktop root must also restore");
            DestroyWindow(root);
        }
    }
    const LIMITS: Limits = Limits {
        time: 500,
        click_x: 3,
        click_y: 3,
        drag_x: 3,
        drag_y: 3,
    };
    #[test]
    fn pairs_require_two_blank_clicks_and_do_not_repeat_on_triple_click() {
        let p = POINT { x: -1500, y: 200 };
        for second in [WM_LBUTTONDOWN, WM_LBUTTONDBLCLK] {
            let mut clicks = Clicks::default();
            for (msg, at, expected) in [
                (WM_LBUTTONDOWN, 10, false),
                (WM_LBUTTONUP, 20, false),
                (second, 100, false),
                (WM_LBUTTONUP, 110, true),
                (WM_LBUTTONDOWN, 150, false),
                (WM_LBUTTONUP, 160, false),
            ] {
                assert_eq!(clicks.message(msg, at, p, true, LIMITS), expected);
            }
        }
    }
    #[test]
    fn icons_dragging_intervening_input_and_slow_clicks_do_not_toggle() {
        let p = POINT { x: 10, y: 20 };
        for scenario in 0..4 {
            let mut clicks = Clicks::default();
            clicks.message(WM_LBUTTONDOWN, 10, p, scenario != 0, LIMITS);
            if scenario == 1 {
                clicks.message(WM_MOUSEMOVE, 12, POINT { x: 30, y: 20 }, true, LIMITS);
            }
            clicks.message(WM_LBUTTONUP, 20, p, true, LIMITS);
            if scenario == 2 {
                clicks.message(WM_RBUTTONDOWN, 30, p, true, LIMITS);
            }
            clicks.message(
                WM_LBUTTONDOWN,
                if scenario == 3 { 800 } else { 100 },
                p,
                true,
                LIMITS,
            );
            let released = if scenario == 3 { 810 } else { 110 };
            assert!(!clicks.message(WM_LBUTTONUP, released, p, true, LIMITS));
        }
    }

    #[test]
    fn idle_and_unpaired_releases_do_not_toggle() {
        let point = POINT { x: 10, y: 20 };
        let mut clicks = Clicks::default();
        for (message, time) in [
            (WM_LBUTTONUP, 1),
            (WM_LBUTTONDOWN, 10),
            (WM_LBUTTONUP, 20),
            (WM_MOUSEMOVE, 60_000),
            (WM_LBUTTONUP, 60_010),
            (WM_LBUTTONDBLCLK, 60_020),
            (WM_LBUTTONUP, 60_030),
            (WM_LBUTTONDOWN, 120_000),
            (WM_LBUTTONUP, 120_010),
        ] {
            assert!(!clicks.message(message, time, point, true, LIMITS));
        }
    }

    #[test]
    fn held_clicks_are_not_double_clicks_and_timestamp_wrap_is_supported() {
        let point = POINT { x: 10, y: 20 };
        let mut clicks = Clicks::default();
        clicks.message(WM_LBUTTONDOWN, 10, point, true, LIMITS);
        clicks.message(WM_LBUTTONUP, 20, point, true, LIMITS);
        clicks.message(WM_LBUTTONDOWN, 100, point, true, LIMITS);
        assert!(!clicks.message(WM_LBUTTONUP, 601, point, true, LIMITS));
        assert!(clicks.first.is_none());
        for (message, time, expected) in [
            (WM_LBUTTONDOWN, u32::MAX - 100, false),
            (WM_LBUTTONUP, u32::MAX - 90, false),
            (WM_LBUTTONDOWN, 10, false),
            (WM_LBUTTONUP, 20, true),
        ] {
            assert_eq!(clicks.message(message, time, point, true, LIMITS), expected);
        }
    }

    #[test]
    fn keyboard_and_wheel_interrupt_a_pending_pair() {
        for message in [WM_KEYDOWN, WM_SYSKEYDOWN, WM_MOUSEWHEEL, WM_MOUSEHWHEEL, WM_XBUTTONDOWN] {
            assert!(observes(message), "the Explorer hook must forward this input");
            let mut gesture = DesktopVisibility::default();
            gesture.enable(true);
            gesture.clicks.first = Some((10, POINT { x: 0, y: 0 }));
            // Interrupts are processed before accessing any native window.
            assert!(!unsafe {
                gesture.message(std::ptr::null_mut(), &MSG { message, ..Default::default() }, false)
            });
            assert!(gesture.clicks.first.is_none());
        }
    }

    #[test]
    #[ignore = "Places a temporary cursor fixture on the interactive desktop; run serially with desktop-test authorization"]
    fn busy_pointer_is_restored_only_over_our_desktop_target() {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        use windows_sys::Win32::UI::HiDpi::*;
        unsafe {
            struct Dpi(DPI_AWARENESS_CONTEXT);
            impl Drop for Dpi {
                fn drop(&mut self) { unsafe { SetThreadDpiAwarenessContext(self.0); } }
            }
            let _dpi = Dpi(SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2));
            struct Fixture(HWND, HCURSOR);
            impl Drop for Fixture {
                fn drop(&mut self) { unsafe {
                    DestroyWindow(self.0);
                    UnregisterClassW(windows_sys::w!("LucidDesk.Cursor.Test"), std::ptr::null_mut());
                    SetCursor(self.1);
                } }
            }
            let mut point = POINT::default();
            assert_ne!(GetCursorPos(&raw mut point), 0);
            let previous = GetCursor();
            let class = WNDCLASSW {
                lpfnWndProc: Some(DefWindowProcW),
                lpszClassName: windows_sys::w!("LucidDesk.Cursor.Test"),
                ..Default::default()
            };
            assert_ne!(RegisterClassW(&raw const class), 0);
            let hwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                class.lpszClassName, windows_sys::w!("Cursor regression fixture"),
                WS_POPUP, point.x - 10, point.y - 10, 40, 40,
                std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
            assert!(!hwnd.is_null());
            let _fixture = Fixture(hwnd, previous);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            assert_eq!(WindowFromPoint(point), hwnd);
            let arrow = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
            for kind in [IDC_WAIT, IDC_APPSTARTING] {
                let busy = LoadCursorW(std::ptr::null_mut(), kind);
                SetCursor(busy);
                restore_pointer(std::ptr::null_mut());
                assert_eq!(GetCursor(), busy, "unrelated window must keep its cursor");
                restore_pointer(hwnd);
                assert_eq!(GetCursor(), arrow);
            }
            let text = LoadCursorW(std::ptr::null_mut(), IDC_IBEAM);
            SetCursor(text);
            restore_pointer(hwnd);
            assert_eq!(GetCursor(), text, "non-busy cursors must be preserved");
        }
    }
}
