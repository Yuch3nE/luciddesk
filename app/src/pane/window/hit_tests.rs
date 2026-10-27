use super::*;

/// Keep a foreign UI thread responsive to native window-position messages.
struct OtherThreadPane {
    hwnd: HWND,
    stop: std::sync::mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl OtherThreadPane {
    fn new() -> Self {
        let (ready, window) = std::sync::mpsc::channel();
        let (stop, stopped) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let pane = Window::new("Foreign UI thread pane")
                .size(100, 100)
                .style(WS_POPUP)
                .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)
                .create()
                .unwrap();
            let hwnd = pane.hwnd().cast();
            unsafe {
                assert_ne!(SetPropW(hwnd, DESKTOP_LAYER, 1usize as _), 0);
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
            ready.send(hwnd as usize).unwrap();
            while stopped.recv_timeout(std::time::Duration::from_millis(5))
                == Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            {
                unsafe {
                    let mut message = MSG::default();
                    while PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0
                    {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
            }
        });
        Self {
            hwnd: window.recv().unwrap() as HWND,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for OtherThreadPane {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        let _ = self.thread.take().unwrap().join();
    }
}

#[test]
fn foreign_ui_thread_panes_are_not_peers_and_remain_application_boundaries() {
    let foreign = OtherThreadPane::new();
    let make = || {
        let pane = Window::new("Local UI thread pane")
            .size(100, 100)
            .style(WS_POPUP)
            .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)
            .create()
            .unwrap();
        unsafe {
            assert_ne!(
                windows_sys::Win32::UI::Shell::SetWindowSubclass(
                    pane.hwnd().cast(),
                    Some(borderless_proc),
                    1,
                    0,
                ),
                0
            );
            ShowWindow(pane.hwnd().cast(), SW_SHOWNOACTIVATE);
        }
        set_layer(pane.hwnd().cast(), false);
        pane
    };
    let first = make();
    let hwnd = first.hwnd().cast();
    assert!(
        desktop_insert_after(hwnd).is_none(),
        "a foreign marker is not a local peer"
    );
    unsafe {
        let mut activation = WINDOWPOS {
            hwnd,
            hwndInsertAfter: HWND_TOP,
            flags: SWP_NOMOVE | SWP_NOSIZE,
            ..Default::default()
        };
        SendMessageW(
            hwnd,
            WM_WINDOWPOSCHANGING,
            0,
            (&raw mut activation) as isize,
        );
        assert_ne!(activation.flags & SWP_NOZORDER, 0);
    }
    let second = make();
    raise_once(second.hwnd().cast());
    unsafe {
        assert_ne!(
            SetWindowPos(
                foreign.hwnd,
                second.hwnd().cast(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER
            ),
            0
        );
        // Establish a deterministic adjacent band even when desktop apps are open.
        RemovePropW(hwnd, DESKTOP_LAYER);
        assert_ne!(
            SetWindowPos(
                hwnd,
                foreign.hwnd,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER
            ),
            0
        );
        SetPropW(hwnd, DESKTOP_LAYER, 1usize as _);
        assert_eq!(GetWindow(hwnd, GW_HWNDPREV), foreign.hwnd);
        assert_eq!(desktop_insert_after(hwnd), Some(foreign.hwnd));
        raise_among_peers(hwnd);
        assert_eq!(GetWindow(hwnd, GW_HWNDPREV), foreign.hwnd);
    }
}

#[test]
fn desktop_panes_raise_among_peers_without_covering_apps() {
    unsafe extern "system" fn count_positions(
        hwnd: HWND,
        message: u32,
        wparam: usize,
        lparam: isize,
        _: usize,
        data: usize,
    ) -> isize {
        unsafe {
            if message == WM_WINDOWPOSCHANGING {
                let count = &*(data as *const std::cell::Cell<u32>);
                count.set(count.get() + 1);
            }
            windows_sys::Win32::UI::Shell::DefSubclassProc(hwnd, message, wparam, lparam)
        }
    }
    unsafe {
        let make = || {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                windows_sys::w!("STATIC"),
                std::ptr::null(),
                WS_POPUP | WS_VISIBLE,
                20,
                20,
                240,
                120,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        let first = make();
        let second = make();
        let app = make();
        let owner = make();
        ShowWindow(owner, SW_HIDE);
        for hwnd in [first, second] {
            assert!(!hwnd.is_null());
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner as isize);
            assert_ne!(
                windows_sys::Win32::UI::Shell::SetWindowSubclass(hwnd, Some(borderless_proc), 1, 0),
                0
            );
        }
        let above = |a, b| {
            let mut current = GetWindow(b, GW_HWNDPREV);
            while !current.is_null() {
                if current == a {
                    return true;
                }
                current = GetWindow(current, GW_HWNDPREV);
            }
            false
        };
        set_layer(first, false);
        SetWindowPos(
            app,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
        set_layer(second, false);
        assert!(above(second, first), "new pane must be above its peers");
        assert!(
            above(app, second),
            "desktop panes must remain below applications"
        );
        SetWindowPos(
            first,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
        assert!(above(first, second), "activation must raise the old pane");
        assert!(above(app, first));
        for _ in 0..3 {
            let mut menu_owner = first;
            let mut popups = Vec::new();
            for _ in 0..2 {
                let popup = make();
                ShowWindow(popup, SW_HIDE);
                SetWindowLongPtrW(popup, GWLP_HWNDPARENT, menu_owner as isize);
                SetWindowPos(
                    popup,
                    HWND_TOPMOST,
                    20,
                    20,
                    100,
                    100,
                    SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                );
                SetWindowPos(
                    popup,
                    HWND_TOPMOST,
                    20,
                    20,
                    100,
                    100,
                    SWP_SHOWWINDOW | SWP_NOOWNERZORDER,
                );
                SetForegroundWindow(popup);
                assert!(
                    above(first, second),
                    "opening an owned menu must preserve pane order"
                );
                assert!(
                    above(app, first),
                    "opening a menu must preserve the application band"
                );
                assert!(above(popup, first));
                assert_eq!(GetWindowLongW(first, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0);
                menu_owner = popup;
                popups.push(popup);
            }
            for popup in popups.into_iter().rev() {
                DestroyWindow(popup);
                assert!(
                    above(first, second),
                    "closing an owned menu must preserve pane order"
                );
                assert!(
                    above(app, first),
                    "closing a menu must preserve the application band"
                );
            }
        }
        let positions = std::cell::Cell::new(0u32);
        assert_ne!(
            windows_sys::Win32::UI::Shell::SetWindowSubclass(
                first,
                Some(count_positions),
                2,
                (&raw const positions) as usize,
            ),
            0
        );
        for interaction in [WM_LBUTTONDOWN, WM_NCLBUTTONDOWN, WM_ENTERSIZEMOVE] {
            // Simulate a peer covering an already-active pane: there is no
            // WM_MOUSEACTIVATE before the next click or native move loop.
            SetWindowPos(
                second,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
            assert!(above(second, first));
            SendMessageW(first, interaction, 0, 0);
            assert!(
                above(first, second),
                "interaction {interaction:#x} must raise the pane"
            );
            assert!(
                above(app, first),
                "interaction must preserve the application band"
            );
            positions.set(0);
            SendMessageW(first, interaction, 0, 0);
            assert_eq!(
                positions.get(),
                0,
                "repeated interaction must not reset z-order"
            );
        }
        // A revealed peer above an app must not pull a lower pane across it.
        super::super::quick_reveal::show(second);
        SetWindowPos(app, second, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
        assert!(above(second, app) && above(app, first));
        for interaction in [WM_MOUSEACTIVATE, WM_LBUTTONDOWN, WM_NCLBUTTONDOWN, WM_ENTERSIZEMOVE] {
            SendMessageW(first, interaction, 0, 0);
            assert!(above(app, first), "interaction {interaction:#x} must not cross an intervening app");
        }
        SetWindowPos(first, HWND_TOP, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER);
        assert!(above(app, first), "activation must respect the same application boundary");
        super::super::quick_reveal::show(first);
        assert!(above(first, app), "explicit reveal must still lift a pane above apps");
        assert_eq!(GetWindowLongW(first, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0);
        SetWindowPos(app, HWND_TOP, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
        assert!(above(app, first), "an ordinary app must still cover a revealed pane");
        // With no visible peer the pane still cannot go below its owner.
        // Repeated HWND_BOTTOM requests must not be mistaken for useful raises.
        ShowWindow(second, SW_HIDE);
        set_layer(first, false);
        for interaction in [
            WM_MOUSEACTIVATE,
            WM_LBUTTONDOWN,
            WM_NCLBUTTONDOWN,
            WM_ENTERSIZEMOVE,
        ] {
            positions.set(0);
            SendMessageW(first, interaction, 0, 0);
            assert_eq!(
                positions.get(),
                0,
                "single owned pane must not reorder on {interaction:#x}"
            );
        }
        let mut activation = WINDOWPOS {
            hwnd: first,
            hwndInsertAfter: HWND_TOP,
            flags: SWP_NOMOVE | SWP_NOSIZE,
            ..Default::default()
        };
        SendMessageW(
            first,
            WM_WINDOWPOSCHANGING,
            0,
            (&raw mut activation) as isize,
        );
        assert_ne!(
            activation.flags & SWP_NOZORDER,
            0,
            "activation of a single pane must preserve its position, not move to HWND_BOTTOM"
        );
        windows_sys::Win32::UI::Shell::RemoveWindowSubclass(first, Some(count_positions), 2);
        set_layer(first, true);
        assert_ne!(GetWindowLongW(first, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0);
        set_layer(first, false);
        assert_eq!(GetWindowLongW(first, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0);
        assert!(above(app, first));
        for hwnd in [first, second, app, owner] {
            DestroyWindow(hwnd);
        }
    }
}

#[test]
fn borderless_activation_keeps_default_state_without_frame_painting() {
    let activations = Rc::new(RefCell::new(Vec::new()));
    let received = Rc::clone(&activations);
    let window = Window::new("Borderless activation")
        .size(240, 160)
        .style(WS_POPUP | WS_THICKFRAME)
        .on_message(move |_, message, wparam, lparam| {
            if message == WM_NCACTIVATE {
                received.borrow_mut().push((wparam, lparam));
            }
            None
        })
        .create()
        .unwrap();
    let hwnd = window.hwnd().cast();
    unsafe {
        assert_ne!(
            windows_sys::Win32::UI::Shell::SetWindowSubclass(hwnd, Some(borderless_proc), 1, 0),
            0
        );
        activations.borrow_mut().clear();
        SendMessageW(hwnd, WM_NCACTIVATE, 1, 0);
        assert_ne!(SendMessageW(hwnd, WM_NCACTIVATE, 0, 0), 0);
        assert_eq!(*activations.borrow(), [(1, -1), (0, -1)]);
        assert_eq!(SendMessageW(hwnd, WM_NCPAINT, 1, 0), 0);
        assert_eq!(SendMessageW(hwnd, WM_ERASEBKGND, 0, 0), 1);
    }
}

#[test]
fn preview_clip_preserves_window_capture_and_restores_region() {
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetCapture;
    unsafe {
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            windows_sys::w!("STATIC"),
            std::ptr::null(),
            WS_POPUP | WS_VISIBLE,
            20,
            20,
            240,
            120,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        assert!(!hwnd.is_null());
        SetCapture(hwnd);
        let mut clipped = shape::WindowShape::default();
        clip_drag(hwnd, true, &mut clipped);
        assert!(clipped.hidden);
        assert_ne!(IsWindowVisible(hwnd), 0);
        assert_eq!(GetCapture(), hwnd);
        let region = CreateRectRgn(0, 0, 0, 0);
        assert_eq!(GetWindowRgn(hwnd, region), NULLREGION);
        clip_drag(hwnd, false, &mut clipped);
        assert!(!clipped.hidden);
        assert_eq!(GetWindowRgn(hwnd, region), 0);
        assert_eq!(GetCapture(), hwnd);
        DeleteObject(region);
        ReleaseCapture();
        DestroyWindow(hwnd);
    }
}

#[test]
fn tab_click_jitter_does_not_start_a_pane_drag() {
    for dpi in [96, 144, 192] {
        let origin = POINT { x: 120, y: 18 };
        assert!(!tab_drag_threshold(origin, origin, dpi));
        assert!(!tab_drag_threshold(origin, POINT { x: 121, y: 19 }, dpi));
        assert!(tab_drag_threshold(origin, POINT { x: 160, y: 18 }, dpi));
        assert!(tab_drag_threshold(origin, POINT { x: 120, y: -30 }, dpi));
    }
}

#[test]
fn locked_pane_blocks_moving_but_preserves_resize_hit_targets() {
    for scale in [1.0, 1.25, 1.5, 2.0] {
        for collapsed in [false, true] {
            let height = if collapsed { HEADER } else { 300.0 };
            let r = RECT {
                left: 0,
                top: 0,
                right: (240.0 * scale) as i32,
                bottom: (height * scale) as i32,
            };
            for x in [1.0, 100.0, 216.0, 239.0] {
                for y in [1.0, 19.0, height - 1.0] {
                    let p = POINT {
                        x: (x * scale) as i32,
                        y: (y * scale) as i32,
                    };
                    let unlocked = frame_hit(r, p, scale, collapsed, false);
                    assert_eq!(
                        frame_hit(r, p, scale, collapsed, true),
                        if unlocked == HTCAPTION {
                            HTCLIENT
                        } else {
                            unlocked
                        }
                    );
                }
            }
            assert_eq!(
                frame_hit(
                    r,
                    POINT {
                        x: (100.0 * scale) as i32,
                        y: (19.0 * scale) as i32
                    },
                    scale,
                    collapsed,
                    false
                ),
                HTCAPTION
            );
        }
    }
}

#[test]
fn collapsed_header_has_buttons_and_dragging_but_no_vertical_resize() {
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let r = RECT {
            left: 0,
            top: 0,
            right: (240.0 * scale) as i32,
            bottom: (HEADER * scale) as i32,
        };
        let at = |x: f32, y: f32, collapsed| {
            frame_hit(
                r,
                POINT {
                    x: (x * scale) as i32,
                    y: (y * scale) as i32,
                },
                scale,
                collapsed,
                false,
            )
        };
        for y in [1.0, 19.0, 37.0] {
            assert_eq!(at(100.0, y, true), HTCAPTION);
            assert_eq!(at(216.0, y, true), HTCLIENT);
            assert_eq!(at(184.0, y, true), HTCLIENT);
            assert_eq!(at(1.0, y, true), HTLEFT);
            assert_eq!(at(239.0, y, true), HTRIGHT);
        }
        assert_eq!(at(100.0, 1.0, false), HTTOP);
        assert_eq!(at(100.0, 37.0, false), HTBOTTOM);
        assert_eq!(at(1.0, 1.0, false), HTTOPLEFT);
        assert_eq!(at(239.0, 37.0, false), HTBOTTOMRIGHT);
    }
}
