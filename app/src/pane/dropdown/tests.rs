use super::*;
use windows_sys::Win32::{
    Graphics::Gdi::ClientToScreen,
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

fn rows(count: usize) -> Vec<Entry> {
    (0..count)
        .map(|i| super::super::menu::entry(i as i32 + 1, format!("Device {i:03}"), "", ""))
        .collect()
}

#[test]
fn popup_matches_anchor_width_flips_up_and_stays_inside_negative_monitor_bounds() {
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let work = RECT {
            left: -1920,
            top: -900,
            right: 0,
            bottom: 180,
        };
        let anchor = RECT {
            left: -300,
            top: 120,
            right: -20,
            bottom: 160,
        };
        let (r, visible) = placement(anchor, work, scale, 232.0, 40);
        assert!(r.top < anchor.top && r.bottom < anchor.top);
        assert!(
            r.left >= work.left
                && r.right <= work.right
                && r.top >= work.top
                && r.bottom <= work.bottom
        );
        assert!(r.right - r.left >= anchor.right - anchor.left);
        assert_eq!(visible, 8);
    }
    let (r, visible) = placement(
        RECT {
            left: 0,
            top: 40,
            right: 232,
            bottom: 72,
        },
        RECT {
            left: 0,
            top: 0,
            right: 180,
            bottom: 120,
        },
        1.0,
        480.0,
        100,
    );
    assert!(r.left >= 0 && r.right <= 180 && r.top >= 0 && r.bottom <= 120);
    assert!(visible < 8);
}

#[test]
fn current_selection_is_visible_keyboard_skips_disabled_and_wheel_never_commits() {
    let mut rows = rows(100);
    rows[98].enabled = false;
    let mut model = Model::new(&rows, Some(95), 8);
    assert_eq!(model.focus, Some(95));
    assert!(model.range().contains(&95));
    model.step(1);
    assert_eq!(model.focus, Some(96));
    model.edge(true);
    assert_eq!(model.focus, Some(99));
    model.step(-1);
    assert_eq!(model.focus, Some(97));
    model.step(-7);
    assert!(model.range().contains(&model.focus.unwrap()));
    model.wheel(120 * 100);
    assert_eq!(model.first, 0);
    model.wheel(-120 * 100);
    assert_eq!(model.first, 92);
    assert_eq!(model.current, Some(95));
    assert!(model.hit(20.0, 3.0, 232.0).is_none());
    assert!(model.hit(230.0, 20.0, 232.0).is_none());
    assert_eq!(model.hit(20.0, 5.0, 232.0), Some(92));
    let (top, _) = model.scrollbar(264.0).unwrap();
    model.drag_scrollbar(top + 4.0, 4.0, 264.0);
    assert_eq!(model.first, 92, "grabbing the thumb must not jump");
    model.drag_scrollbar(-100.0, 4.0, 264.0);
    assert_eq!(model.first, 0);
    model.drag_scrollbar(1000.0, 4.0, 264.0);
    assert_eq!(model.first, 92);
    assert_eq!(model.current, Some(95));
}

#[test]
fn type_ahead_matches_prefix_cycles_and_expires_without_changing_value() {
    let rows = ["AMD", "NVIDIA", "Network", "Intel", "日本語"]
        .into_iter()
        .enumerate()
        .map(|(i, label)| super::super::menu::entry(i as i32 + 1, label, "", ""))
        .collect::<Vec<_>>();
    let mut model = Model::new(&rows, Some(0), 2);
    let now = Instant::now();
    model.type_char('n', now);
    assert_eq!(model.focus, Some(1));
    model.type_char('n', now);
    assert_eq!(model.focus, Some(2));
    model.type_char('e', now);
    assert_eq!(model.focus, Some(2));
    model.type_char('i', now + Duration::from_secs(2));
    assert_eq!(model.focus, Some(3));
    model.type_char('日', now + Duration::from_secs(4));
    assert_eq!(model.focus, Some(4));
    assert!(model.range().contains(&4));
    assert_eq!(model.current, Some(0));
    let mut empty = Model::new(&[], None, 8);
    empty.type_char('n', now);
    empty.step(1);
    empty.edge(true);
    assert!(empty.focus.is_none());
}

#[test]
fn cached_layouts_render_visible_rows_and_selection_at_multiple_scales() {
    let _sta = crate::pane::test_support::apartment();
    let device = windows_canvas::GpuDevice::new_warp().unwrap();
    let mut entries = rows(80);
    entries[40].label =
        "NVIDIA GeForce RTX 4080 SUPER — very long device description for overflow validation"
            .into();
    for dark in [false, true] {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let mut popup = Popup::new(
                &entries,
                Options {
                    anchor: RECT {
                        left: 50,
                        top: 50,
                        right: 282,
                        bottom: 82,
                    },
                    current: Some(40),
                },
                RECT {
                    left: 0,
                    top: 0,
                    right: 1920,
                    bottom: 1080,
                },
                scale,
            )
            .unwrap();
            assert!(popup.model.range().contains(&40));
            assert!(popup.painter.truncated(40));
            let r = popup.bounds;
            let bitmap = crate::pane::canvas::Offscreen::new(
                &device,
                (r.right - r.left) as u32,
                (r.bottom - r.top) as u32,
            )
            .unwrap();
            let mut highlights = vec![0.0; entries.len()];
            highlights[41] = 1.0;
            popup
                .paint(&bitmap.target, scale, &highlights, false, dark)
                .unwrap();
            let first = bitmap.pixels().unwrap();
            // Idle redraws reuse the exact text layouts and palette.
            popup
                .paint(&bitmap.target, scale, &highlights, false, dark)
                .unwrap();
            assert_eq!(first, bitmap.pixels().unwrap());
            if std::env::var_os("LUCIDDESK_TEST_EXPORT_SNAPSHOTS").is_some() && scale == 1.0 {
                let width = (r.right - r.left) as u32;
                let height = (r.bottom - r.top) as u32;
                let mut bmp = vec![0_u8; 54];
                bmp[..2].copy_from_slice(b"BM");
                bmp[2..6].copy_from_slice(&(54 + first.len() as u32).to_le_bytes());
                bmp[10..14].copy_from_slice(&54_u32.to_le_bytes());
                bmp[14..18].copy_from_slice(&40_u32.to_le_bytes());
                bmp[18..22].copy_from_slice(&width.to_le_bytes());
                bmp[22..26].copy_from_slice(&(-(height as i32)).to_le_bytes());
                bmp[26..28].copy_from_slice(&1_u16.to_le_bytes());
                bmp[28..30].copy_from_slice(&32_u16.to_le_bytes());
                bmp.extend_from_slice(&first);
                std::fs::write(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../target")
                        .join(if dark {
                            "dropdown-dark.bmp"
                        } else {
                            "dropdown-light.bmp"
                        }),
                    bmp,
                )
                .unwrap();
            }
        }
    }
}

#[test]
#[ignore = "Opens real dropdowns and drives native keyboard/mouse input; run on the interactive desktop"]
fn native_popup_selects_current_navigates_scrolls_searches_and_cancels() {
    use std::{cell::Cell, rc::Rc};
    let _sta = crate::pane::test_support::apartment();
    let cases = vec![
        (
            vec![
                (WM_KEYDOWN, VK_DOWN as usize, 0),
                (WM_KEYDOWN, VK_RETURN as usize, 0),
            ],
            Some(42),
        ),
        (
            vec![
                (WM_KEYDOWN, VK_END as usize, 0),
                (WM_KEYDOWN, VK_RETURN as usize, 0),
            ],
            Some(99),
        ),
        (
            vec![
                (WM_KEYDOWN, VK_HOME as usize, 0),
                (WM_KEYDOWN, VK_NEXT as usize, 0),
                (WM_KEYDOWN, VK_RETURN as usize, 0),
            ],
            Some(7),
        ),
        (
            "device 090"
                .chars()
                .map(|ch| (WM_CHAR, ch as usize, 0))
                .chain([(WM_KEYDOWN, VK_RETURN as usize, 0)])
                .collect(),
            Some(90),
        ),
        (
            vec![
                (WM_MOUSEWHEEL, ((-120i16 as u16) as usize) << 16, 0),
                (WM_KEYDOWN, VK_DOWN as usize, 0),
                (WM_KEYDOWN, VK_RETURN as usize, 0),
            ],
            Some(39),
        ),
        (vec![(WM_KEYDOWN, VK_ESCAPE as usize, 0)], None),
        (vec![(WM_KEYDOWN, VK_RETURN as usize, 0)], Some(40)),
        (vec![(WM_LBUTTONDOWN, usize::MAX, 16 | (16 << 16))], None),
        (vec![(WM_CLOSE, usize::MAX, 0)], None),
        (
            vec![
                (WM_LBUTTONDOWN, 0, 228 | (180 << 16)),
                (WM_MOUSEMOVE, 0, 228 | (250 << 16)),
                (WM_LBUTTONUP, 0, 228 | (250 << 16)),
                (WM_KEYDOWN, VK_DOWN as usize, 0),
                (WM_KEYDOWN, VK_RETURN as usize, 0),
            ],
            Some(92),
        ),
    ];
    for round in 0..3 {
        for (commands, expected) in cases.clone() {
            let sent = Rc::new(Cell::new(false));
            let sent_handler = sent.clone();
            let started = Instant::now();
            let owner = windows_window::Window::new("Dropdown input fixture")
                .style(WS_POPUP)
                .ex_style(WS_EX_TOOLWINDOW)
                .size(400, 200)
                .on_message(move |raw, message, _, _| {
                    if message == WM_DESTROY {
                        return Some(0);
                    }
                    if message == WM_TIMER {
                        let hwnd: HWND = raw.cast();
                        unsafe extern "system" fn find_popup(window: HWND, data: isize) -> i32 {
                            unsafe {
                                let pair = &mut *(data as *mut (HWND, HWND));
                                if GetWindow(window, GW_OWNER) == pair.0
                                    && IsWindowVisible(window) != 0
                                {
                                    pair.1 = window;
                                }
                            }
                            1
                        }
                        let mut pair = (hwnd, std::ptr::null_mut::<std::ffi::c_void>());
                        unsafe {
                            EnumThreadWindows(
                                windows_sys::Win32::System::Threading::GetCurrentThreadId(),
                                Some(find_popup),
                                (&raw mut pair) as isize,
                            );
                        }
                        let popup = pair.1;
                        if !popup.is_null() {
                            if !sent_handler.replace(true) {
                                for &(message, wp, lp) in &commands {
                                    unsafe {
                                        let lp = if wp != usize::MAX
                                            && matches!(
                                                message,
                                                WM_LBUTTONDOWN | WM_MOUSEMOVE | WM_LBUTTONUP
                                            ) {
                                            let mut client = RECT::default();
                                            GetClientRect(popup, &raw mut client);
                                            let scale =
                                                windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd)
                                                    .max(96)
                                                    as f32
                                                    / 96.0;
                                            let x = client.right - (4.0 * scale).round() as i32;
                                            let y = (f32::from(((lp >> 16) as u16).cast_signed())
                                                * scale)
                                                .round()
                                                as i32;
                                            ((y as isize) << 16) | x as isize
                                        } else {
                                            lp
                                        };
                                        PostMessageW(
                                            if wp == usize::MAX { hwnd } else { popup },
                                            message,
                                            if wp == usize::MAX { 0 } else { wp },
                                            lp,
                                        );
                                    }
                                }
                            } else if started.elapsed() > Duration::from_secs(5) {
                                unsafe {
                                    PostMessageW(popup, WM_CLOSE, 0, 0);
                                }
                            }
                        }
                        return Some(0);
                    }
                    None
                })
                .create()
                .unwrap();
            let hwnd = owner.hwnd().cast();
            let mut point = windows_sys::Win32::Foundation::POINT { x: 16, y: 16 };
            unsafe {
                ClientToScreen(hwnd, &raw mut point);
                SetTimer(hwnd, 99, 25, None);
            }
            let mut entries = rows(100);
            entries[41].enabled = false;
            entries[40].id = 700;
            entries[42].id = 900;
            let result = show(
                hwnd,
                RECT {
                    left: point.x,
                    top: point.y,
                    right: point.x + 232,
                    bottom: point.y + 32,
                },
                (
                    luciddesk_core::PanelTheme::Dark,
                    luciddesk_core::Backdrop::Mica,
                ),
                entries,
                Some(40),
            );
            unsafe {
                KillTimer(hwnd, 99);
            }
            assert!(sent.get(), "dropdown did not become visible");
            assert_eq!(result, expected);
        }
        let process = unsafe { windows_sys::Win32::System::Threading::GetCurrentProcess() };
        let mut handles = 0;
        unsafe {
            windows_sys::Win32::System::Threading::GetProcessHandleCount(process, &raw mut handles);
        }
        println!("round={round} handles={handles}");
    }
}
