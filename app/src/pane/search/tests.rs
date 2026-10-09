#[test]
#[ignore = "Activates real windows; run alone in an interactive desktop session"]
fn editor_click_raises_search_among_panes_but_hotkey_stays_on_desktop() {
    use super::*;
    let _sta = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(super::super::tests::test_state()));
    super::super::handle(&state, luciddesk_core::PanelId::new(0), Event::EnableSearch).unwrap();
    let hwnd = state.borrow().views[0].window.hwnd().cast();
    let make = || windows_window::Window::new("Search layer regression")
        .style(WS_POPUP).ex_style(WS_EX_TOOLWINDOW).size(100, 100)
        .on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0))
        .create().unwrap();
    let app = make();
    let peer = make();
    let app_hwnd = app.hwnd().cast();
    let peer_hwnd = peer.hwnd().cast();
    unsafe {
        let above = |a, b| {
            let mut current = GetWindow(b, GW_HWNDPREV);
            while !current.is_null() {
                if current == a { return true; }
                current = GetWindow(current, GW_HWNDPREV);
            }
            false
        };
        ShowWindow(app_hwnd, SW_SHOWNOACTIVATE);
        SetWindowPos(app_hwnd, HWND_TOP, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
        super::hotkey::activate(&state);
        assert!(above(app_hwnd, hwnd), "global hotkey must retain the desktop layer");
        ShowWindow(peer_hwnd, SW_SHOWNOACTIVATE);
        SetWindowSubclass(peer_hwnd, Some(super::super::window::borderless_proc), 1, 0);
        super::super::window::set_layer(peer_hwnd, false);
        SetFocus(edit(hwnd));
        for message in [WM_MOUSEACTIVATE, WM_LBUTTONDOWN] {
            SetWindowPos(peer_hwnd, HWND_TOP, 0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
            assert!(above(peer_hwnd, hwnd));
            SendMessageW(edit(hwnd), message, 0, 0);
            SendMessageW(edit(hwnd), WM_LBUTTONUP, 0, 0);
            assert!(above(hwnd, peer_hwnd), "editor interaction must raise its pane");
            assert!(above(app_hwnd, edit(hwnd)), "editor must remain below applications");
            assert!(above(edit(hwnd), hwnd), "editor must remain above its own pane");
            assert_eq!(GetFocus(), edit(hwnd));
            assert_eq!(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0);
        }
        ReleaseCapture();
        super::super::window::set_layer(hwnd, true);
        assert_ne!(GetWindowLongW(edit(hwnd), GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0,
            "editor must follow a permanently topmost pane");
        assert!(above(edit(hwnd), hwnd));
        super::super::window::set_layer(hwnd, false);
        assert_eq!(GetWindowLongW(edit(hwnd), GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0,
            "editor must leave the topmost band with its pane");
        assert!(above(app_hwnd, edit(hwnd)));
    }
}

#[test]
fn compact_search_edges_resize_and_icon_drags_at_each_scale() {
    use super::*;
    for scale in [1.0, 1.5, 2.0] {
        let bounds = RECT {
            right: (360.0 * scale) as i32,
            bottom: (TOP * scale) as i32,
            ..Default::default()
        };
        for (x, y, expected) in [
            (2.0, 20.0, HTLEFT),
            (358.0, 20.0, HTRIGHT),
            (24.0, 20.0, HTCAPTION),
            (100.0, 20.0, HTCLIENT),
        ] {
            let point = POINT {
                x: (x * scale) as i32,
                y: (y * scale) as i32,
            };
            assert_eq!(search_frame_hit(bounds, point, scale, false), expected);
        }
        assert_eq!(
            search_frame_hit(
                bounds,
                POINT {
                    x: (24.0 * scale) as i32,
                    y: (20.0 * scale) as i32
                },
                scale,
                true
            ),
            HTCLIENT
        );
    }
}

use super::*;

#[test]
fn pending_queries_keep_results_visible_but_not_actionable() {
    let mut state = populated_search();
    state.select(2, false, false);
    let old_generation = state.generation;
    state.change("new query".into());
    assert_eq!(state.entries.len(), 30);
    assert!(state.selected().is_empty());
    assert_eq!(state.row_at(TOP + ROW_INSET + 5.0), None);
    assert_eq!(state.footer(), "正在搜索…");
    assert!(!state.accept(
        old_generation,
        Ok(Page {
            total: 0,
            offset: 0,
            entries: vec![]
        })
    ));
    state.accept(
        state.generation,
        Ok(Page {
            total: 1,
            offset: 0,
            entries: vec![Entry {
                path: r"C:\new.txt".into(),
                folder: false,
            }],
        }),
    );
    assert_eq!(state.row_at(TOP + ROW_INSET + 5.0), Some(0));
    assert!(state.selection.is_empty());
    assert_eq!(state.footer(), "1 个结果");
    assert_eq!(state.row_at(TOP + ROW_INSET - 1.0), None);
    assert_eq!(
        state.row_at(TOP + ROW_INSET + ROW),
        None,
        "footer is not a result row"
    );
    state.change("bad query".into());
    state.accept(state.generation, Err("未连接 Everything".into()));
    assert!(state.entries.is_empty());
    assert!(state.failed);
    assert!(state.selected().is_empty());
}

#[test]
fn refresh_restores_selection_by_path_after_reordering() {
    let mut state = populated_search();
    state.select(20, false, false);
    let selected = state.entries[20].path.clone();
    state.change("x".into());
    state.accept(
        state.generation,
        Ok(Page {
            total: 2,
            offset: 0,
            entries: vec![
                Entry {
                    path: selected.clone(),
                    folder: false,
                },
                Entry {
                    path: r"C:\other".into(),
                    folder: true,
                },
            ],
        }),
    );
    assert_eq!(state.selection, BTreeSet::from([0]));
    assert_eq!(state.focused, Some(0));
    assert_eq!(state.scroll, 0);
    assert_eq!(
        state.selected()[0].file_system_path(),
        Some(selected.as_path())
    );
}

#[test]
fn results_use_available_space_without_moving_the_input() {
    for scale in [1.0, 1.5, 2.0] {
        let mut state = populated_search();
        let work = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let current = RECT {
            left: 100,
            top: 650,
            right: 580,
            bottom: 650 + (TOP * scale) as i32,
        };
        let fitted = fit_search(work, current, scale, &mut state);
        assert_eq!(fitted.top, current.top);
        assert_eq!(fitted.left, current.left);
        assert!(fitted.bottom <= work.bottom);
        assert!(state.visible_rows < VISIBLE);
    }
}

#[test]
fn full_path_tooltip_updates_and_releases_native_window() {
    let window = windows_window::Window::new("Search tooltip fixture")
        .style(WS_POPUP)
        .size(400, 200)
        .on_message(|_, message, _, _| if message == WM_DESTROY { Some(0) } else { None })
        .create()
        .unwrap();
    let owner = window.hwnd().cast();
    let mut tip = tooltip::Tooltip::new(owner).unwrap();
    tip.show_for_row(owner, r"C:\项目\完整文件名.txt", TOP + ROW_INSET);
    tip.hide();
    tip.show_for_row(owner, r"D:\another.txt", TOP + ROW_INSET + ROW);
    drop(tip);
    // A fresh tooltip can be registered again on the same owner.
    assert!(tooltip::Tooltip::new(owner).is_some());
}

#[test]
fn clear_button_keeps_fixed_input_and_ime_escape_is_not_intercepted() {
    let _apartment = crate::pane::test_support::apartment();
    let app = Rc::new(RefCell::new(super::super::tests::test_state()));
    super::super::handle(&app, luciddesk_core::PanelId::new(0), Event::EnableSearch).unwrap();
    let hwnd = app.borrow().views[0].window.hwnd().cast();
    unsafe {
        SetWindowTextW(edit(hwnd), wide("query").as_ptr());
        SendMessageW(hwnd, INPUT, 0, 0);
        let mut bounds = RECT::default();
        GetClientRect(hwnd, &raw mut bounds);
        assert!(bounds.bottom as f32 / scale(hwnd) > TOP);
        let x = bounds.right - (28.0 * scale(hwnd)) as i32;
        let y = (28.0 * scale(hwnd)) as i32;
        SendMessageW(
            hwnd,
            WM_LBUTTONDOWN,
            0,
            (x as u16 as usize | ((y as u16 as usize) << 16)) as isize,
        );
        SendMessageW(hwnd, INPUT, 0, 0);
        assert_eq!(text(edit(hwnd)), "");
        assert_eq!(GetFocus(), edit(hwnd));
        GetClientRect(hwnd, &raw mut bounds);
        assert_eq!(bounds.bottom, (TOP * scale(hwnd)).round() as i32);
        assert_ne!(IsWindowVisible(hwnd), 0);
        SetWindowTextW(edit(hwnd), wide("正在输入").as_ptr());
        SendMessageW(edit(hwnd), WM_IME_STARTCOMPOSITION, 0, 0);
        SendMessageW(edit(hwnd), WM_KEYDOWN, VK_ESCAPE as usize, 0);
        assert_eq!(text(edit(hwnd)), "正在输入");
        SendMessageW(edit(hwnd), WM_IME_ENDCOMPOSITION, 0, 0);
        SendMessageW(edit(hwnd), WM_KEYDOWN, VK_ESCAPE as usize, 0);
        assert_eq!(text(edit(hwnd)), "");
    }
}

fn populated_search() -> Search {
    let mut state = Search::new();
    state.change("x".into());
    state.entries = (0..30)
        .map(|i| Entry {
            path: format!("C:\\{i}").into(),
            folder: false,
        })
        .collect();
    state.replacing = false;
    state.busy = false;
    state.total = state.entries.len() as u32;
    state
}
#[test]
fn viewport_fits_screen_edges_and_reduces_rows_at_high_dpi() {
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let mut state = populated_search();
        let work = RECT {
            left: -1280,
            top: -100,
            right: 0,
            bottom: 300,
        };
        let current = RECT {
            left: -300,
            top: 250,
            right: 200,
            bottom: 306,
        };
        state.select(29, false, false);
        let fitted = fit_search(work, current, scale, &mut state);
        assert!(fitted.left >= work.left && fitted.right <= work.right);
        assert!(fitted.top >= work.top && fitted.bottom <= work.bottom);
        assert!(state.visible_rows < VISIBLE);
        assert!(state.scroll <= 29 && state.scroll + state.visible_rows > 29);
        assert!(
            (fitted.bottom - fitted.top) as f32 >= (TOP + ROW * state.visible_rows as f32) * scale
        );
        state.change(String::new());
        let compact = fit_search(work, fitted, scale, &mut state);
        assert_eq!(compact.bottom - compact.top, (TOP * scale).round() as i32);
    }
}
#[test]
fn keyboard_range_extension_keeps_disjoint_selection_and_ctrl_only_focus() {
    let mut state = populated_search();
    state.select(0, false, false);
    state.select(10, true, false);
    state.move_focus(12, true, true);
    assert_eq!(state.selection, BTreeSet::from([0, 10, 11, 12]));
    state.move_focus(15, true, false);
    assert_eq!(state.focused, Some(15));
    assert_eq!(state.selection, BTreeSet::from([0, 10, 11, 12]));
    state.move_focus(16, false, true);
    assert_eq!(state.selection, BTreeSet::from([15, 16]));
}
#[test]
fn input_colors_survive_owner_callback_reentry() {
    let _apartment = crate::pane::test_support::apartment();
    const REENTER: u32 = WM_APP + 121;
    let owner = windows_window::Window::new("Search color regression")
        .on_message(|raw, msg, wp, _| {
            if msg == REENTER {
                let hwnd = raw.cast();
                return Some(unsafe {
                    SendMessageW(hwnd, WM_CTLCOLOREDIT, wp, edit(hwnd) as isize)
                });
            }
            None
        })
        .create()
        .unwrap();
    let hwnd = owner.hwnd().cast();
    let mut editor = Editor::new(hwnd).unwrap();
    for dark in [true, false] {
        editor.line_height = 1;
        editor.appearance(hwnd, dark);
        unsafe {
            let mut bounds = RECT::default();
            GetWindowRect(editor.hwnd, &raw mut bounds);
            MapWindowPoints(std::ptr::null_mut(), hwnd, (&raw mut bounds).cast(), 2);
            assert_eq!(bounds.bottom - bounds.top, editor.line_height);
            assert!((bounds.top + bounds.bottom - (TOP * scale(hwnd)).round() as i32).abs() <= 1);
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            assert!(!dc.is_null());
            SetBkColor(dc, 0xffffff);
            let brush = SendMessageW(hwnd, REENTER, dc as usize, 0);
            assert_ne!(brush, 0);
            assert_eq!(GetBkColor(dc), if dark { 0x202020 } else { 0xf5f5f5 });
            assert_eq!(GetDCBrushColor(dc), GetBkColor(dc));
            DeleteDC(dc);
        }
    }
}
#[test]
#[ignore = "requires Everything and an interactive desktop"]
fn live_compact_query_and_clear() {
    let _apartment = crate::pane::test_support::apartment();
    let app = Rc::new(RefCell::new(super::super::tests::test_state()));
    app.borrow_mut().workspace.set_appearance(
        luciddesk_core::PanelTheme::Dark,
        luciddesk_core::Backdrop::Translucent { opacity: 1.0 },
    );
    super::super::handle(&app, luciddesk_core::PanelId::new(0), Event::EnableSearch).unwrap();
    let hwnd = app.borrow().views[0].window.hwnd().cast();
    unsafe {
        let mut key = 0;
        let mut alpha = 0;
        let mut flags = 0;
        assert_ne!(
            GetLayeredWindowAttributes(edit(hwnd), &raw mut key, &raw mut alpha, &raw mut flags),
            0
        );
        assert_eq!(key, 0x202020);
        assert_eq!(flags, LWA_COLORKEY);
        SetFocus(hwnd);
        let x = (80.0 * scale(hwnd)) as isize;
        let y = (28.0 * scale(hwnd)) as isize;
        SendMessageW(hwnd, WM_LBUTTONDOWN, 1, x | (y << 16));
        assert_eq!(GetFocus(), edit(hwnd));
        assert_eq!(GetCapture(), edit(hwnd));
        SendMessageW(edit(hwnd), WM_LBUTTONUP, 0, 0);
    }
    // Widths outside the old 320..640 range survive resize and persistence.
    for width in [280.0, 720.0] {
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                (width * scale(hwnd)) as i32,
                (TOP * scale(hwnd)) as i32,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            SendMessageW(hwnd, WM_EXITSIZEMOVE, 0, 0);
        }
        let saved = app.borrow().store.load_workspace().unwrap();
        let panel = saved.panels().iter().find(|p| p.is_search()).unwrap();
        assert!((panel.rect().width - width).abs() < 1.0);
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let mut monitor = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        assert_ne!(
            GetMonitorInfoW(
                MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                &raw mut monitor
            ),
            0
        );
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            monitor.rcWork.left + 40,
            monitor.rcWork.bottom - (TOP * scale(hwnd)) as i32,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    let query = wide(&format!("path:\"{}\" Cargo.toml", root.display()));
    unsafe {
        SetWindowTextW(edit(hwnd), query.as_ptr());
    }
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut bounds = RECT::default();
    loop {
        windows_window::pump();
        unsafe {
            GetWindowRect(hwnd, &raw mut bounds);
        }
        if (bounds.bottom - bounds.top) as f32 / scale(hwnd) > TOP + 64.0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "search did not expand to results"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    unsafe {
        assert!(bounds.top >= monitor.rcWork.top && bounds.bottom <= monitor.rcWork.bottom);
        let mut editor_bounds = RECT::default();
        GetWindowRect(edit(hwnd), &raw mut editor_bounds);
        assert!(
            (editor_bounds.top + editor_bounds.bottom
                - 2 * bounds.top
                - (TOP * scale(hwnd)).round() as i32)
                .abs()
                <= 1
        );
        SetWindowTextW(edit(hwnd), windows_sys::w!(""));
    }
    windows_window::pump();
    unsafe {
        GetWindowRect(hwnd, &raw mut bounds);
    }
    assert_eq!(bounds.bottom - bounds.top, (TOP * scale(hwnd)) as i32);
}

#[test]
fn compact_render_keeps_rows_and_border_inside_the_pane() {
    let _apartment = crate::pane::test_support::apartment();
    let app = Rc::new(RefCell::new(super::super::tests::test_state()));
    app.borrow_mut().workspace.set_appearance(
        luciddesk_core::PanelTheme::Dark,
        luciddesk_core::Backdrop::Translucent { opacity: 1.0 },
    );
    super::super::handle(&app, luciddesk_core::PanelId::new(0), Event::EnableSearch).unwrap();
    let mut model = app.borrow().views[0].model.borrow().clone();
    let fixture = windows_window::Window::new("Search render fixture")
        .style(WS_POPUP)
        .size(480, 180)
        .on_message(|_, message, _, _| if message == WM_DESTROY { Some(0) } else { None })
        .create()
        .unwrap();
    let hwnd = fixture.hwnd().cast();

    model.dark = true;
    model.backdrop = luciddesk_core::Backdrop::Translucent { opacity: 1.0 };
    let mut state = Search::new();
    state.change("LucidDesk".into());
    state.entries = vec![
        Entry {
            path: r"E:\Project\LucidDesk\README.md".into(),
            folder: false,
        },
        Entry {
            path: r"E:\Project\LucidDesk\docs".into(),
            folder: true,
        },
    ];
    state.replacing = false;
    state.busy = false;
    state.total = state.entries.len() as u32;
    state.select(0, false, false);
    let mut editor = Editor::new(hwnd).unwrap();
    editor.appearance(hwnd, true);
    unsafe {
        SetWindowTextW(editor.hwnd, wide("LucidDesk").as_ptr());
    }
    resize(hwnd, &mut state);
    let mut drawing = Drawing::new(hwnd).unwrap();
    // Present rotates this flip-model swap chain. Capture after rasterization,
    // before submitting it; buffer zero after Present belongs to the next frame.
    assert!(drawing.draw_frame(hwnd, &model, &state).unwrap());
    let pixels = drawing.surface.readback().unwrap();
    let mut r = RECT::default();
    unsafe {
        GetClientRect(hwnd, &raw mut r);
    }
    assert_eq!(pixels.len(), (r.right * r.bottom * 4) as usize);
    assert!(
        pixels
            .chunks_exact(4)
            .any(|p| p[0] > 180 && p[1] > 180 && p[2] > 180 && p[3] > 200)
    );
    // Both result names must appear in their own rows, not merely in a status
    // label or the input's separately drawn native EDIT window.
    let dpi = scale(hwnd);
    for row in 0..2 {
        let top = ((TOP + ROW_INSET + row as f32 * ROW + 2.0) * dpi).floor() as usize;
        let bottom = ((TOP + ROW_INSET + row as f32 * ROW + 24.0) * dpi).ceil() as usize;
        let left = (46.0 * dpi).floor() as usize;
        let right = (r.right as f32 - 18.0 * dpi).floor() as usize;
        assert!((top..bottom).any(|y| (left..right).any(|x| {
            let p = &pixels[(y * r.right as usize + x) * 4..][..4];
            p[0] > 180 && p[1] > 180 && p[2] > 180 && p[3] > 200
        })), "missing text in result row {row}");
    }
    if let Some(dir) = std::env::var_os("LUCIDDESK_RENDER_OUTPUT") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("search.bgra"), pixels).unwrap();
        std::fs::write(
            dir.join("search-size.txt"),
            format!("{} {}", r.right, r.bottom),
        )
        .unwrap();
    }
    // Native EDIT draws separately from the composition surface; validate its
    // contents directly, and export the pane layer for visual review.
    for dark in [false, true] {
        model.dark = dark;
        editor.appearance(hwnd, dark);
        for width in [280.0, 480.0] {
            for scenario in ["results", "empty", "error"] {
                state.change("项目".into());
                unsafe {
                    SetWindowTextW(editor.hwnd, wide("项目").as_ptr());
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        0,
                        0,
                        (width * scale(hwnd)) as i32,
                        (TOP * scale(hwnd)) as i32,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                assert_eq!(text(editor.hwnd), "项目");
                let result = match scenario {
                    "results" => Ok(Page { total: 482, offset: 0, entries: vec![
                        Entry { path: r"E:\工作\客户交付\2026\九月\项目计划与验收资料\项目计划与交付说明最终修订版.docx".into(), folder: false },
                        Entry { path: r"D:\资料\项目计划参考资料".into(), folder: true },
                    ] }),
                    "empty" => Ok(Page { total: 0, offset: 0, entries: vec![] }),
                    _ => Err("未连接 Everything，请启动 Everything 并启用 IPC，然后刷新。".into()),
                };
                state.accept(state.generation, result);
                if scenario == "results" {
                    state.select(0, false, false);
                    state.hovered = Some(1);
                }
                resize(hwnd, &mut state);
                assert!(drawing.draw_frame(hwnd, &model, &state).unwrap());
                let pixels = drawing.surface.readback().unwrap();
                unsafe {
                    GetClientRect(hwnd, &raw mut r);
                }
                assert_eq!(pixels.len(), (r.right * r.bottom * 4) as usize);
                if let Some(dir) = std::env::var_os("LUCIDDESK_RENDER_OUTPUT") {
                    let mut bmp = vec![0u8; 54];
                    bmp[..2].copy_from_slice(b"BM");
                    bmp[2..6].copy_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
                    bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
                    bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
                    bmp[18..22].copy_from_slice(&r.right.to_le_bytes());
                    bmp[22..26].copy_from_slice(&(-r.bottom).to_le_bytes());
                    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
                    bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
                    bmp.extend(pixels);
                    let path = std::path::PathBuf::from(dir).join(format!(
                        "search-{scenario}-{width}-{}.bmp",
                        if dark { "dark" } else { "light" }
                    ));
                    std::fs::write(path, bmp).unwrap();
                }
            }
        }
    }
}
#[test]
fn empty_query_collapses_and_rejects_pending_results() {
    let mut state = Search::new();
    assert_eq!(state.height(), TOP);
    assert!(state.due.is_none());
    state.change("example".into());
    let old = state.generation;
    assert!(state.height() > TOP);
    state.change(" ".into());
    assert_eq!(state.height(), TOP);
    assert!(state.due.is_none());
    assert!(!state.accept(
        old,
        Ok(Page {
            offset: 0,
            total: 1,
            entries: vec![Entry {
                path: "C:\\x".into(),
                folder: false,
            }]
        })
    ));
    assert!(state.entries.is_empty());
}
#[test]
fn results_expand_to_eight_rows_and_selection_scrolls() {
    let mut state = Search::new();
    state.change("x".into());
    let entries = (0..20)
        .map(|i| Entry {
            path: format!("C:\\{i}").into(),
            folder: false,
        })
        .collect();
    assert!(state.accept(
        state.generation,
        Ok(Page {
            offset: 0,
            total: 20,
            entries
        })
    ));
    assert_eq!(
        state.height(),
        TOP + ROW_INSET + ROW * VISIBLE as f32 + FOOTER
    );
    state.select(0, false, false);
    state.select(10, false, true);
    assert_eq!(state.selection.len(), 11);
    assert_eq!(state.scroll, 3);
}

#[test]
fn clearing_search_releases_large_result_buffer_and_rejects_stale_pages() {
    let mut state = populated_search();
    state.entries.reserve(100_000);
    let previous = state.generation;
    assert!(state.entries.capacity() >= 100_000);
    state.change(String::new());
    assert_eq!(state.entries.capacity(), 0);
    assert!(!state.accept(
        previous,
        Ok(Page {
            total: 1,
            offset: 0,
            entries: vec![Entry {
                path: r"C:\stale".into(),
                folder: false
            }],
        })
    ));
    assert!(state.entries.is_empty());
}

#[test]
fn refresh_restores_later_page_selection_and_anchor_atomically() {
    let mut state = populated_search();
    let (sender, requests) = mpsc::channel();
    state.sender = sender;
    state.entries = (0..400).map(|i| Entry { path: format!("C:/result-{i}").into(), folder: false }).collect();
    state.total = 800;
    state.select(250, false, false);
    state.select(260, false, true);
    let old_scroll = state.scroll;
    let original = state.entries.clone();
    state.change("x".into());
    state.due = None;
    state.accept(state.generation, Ok(Page { total: 800, offset: 0, entries: original[..200].to_vec() }));
    assert!(state.replacing && state.busy);
    assert_eq!(state.entries.len(), 400);
    assert_eq!(state.scroll, old_scroll);
    assert!(state.selected().is_empty());
    assert_eq!(requests.try_recv().unwrap().offset, 200);
    state.accept(state.generation, Ok(Page { total: 800, offset: 200, entries: original[200..].to_vec() }));
    assert!(!state.replacing && !state.busy);
    assert_eq!(state.selection, (250..=260).collect());
    assert_eq!(state.focused, Some(260));
    assert_eq!(state.anchor, Some(250));
    assert_eq!(state.scroll, old_scroll);
    assert!(requests.try_recv().is_err(), "refresh must not enumerate unseen pages");
}

#[test]
fn refresh_stops_on_shrinking_results_and_discards_cancelled_pages() {
    let mut state = populated_search();
    let (sender, requests) = mpsc::channel();
    state.sender = sender;
    state.select(20, false, false);
    state.change("x".into());
    state.accept(state.generation, Ok(Page { total: 1, offset: 0, entries: vec![state.entries[0].clone()] }));
    assert!(!state.replacing);
    assert!(state.selection.is_empty());
    assert!(requests.try_recv().is_err());
    state.refresh_entries = state.entries.clone();
    let stale = state.generation;
    state.change("different".into());
    assert!(state.refresh_entries.is_empty());
    assert_eq!(state.refresh_limit, 0);
    assert!(!state.accept(stale, Ok(Page { total: 0, offset: 200, entries: vec![] })));
}

#[test]
fn double_click_requires_same_file_within_interval() {
    let path = std::path::PathBuf::from("C:/first.txt");
    let recent = (path.clone(), Instant::now());
    let interval = Duration::from_millis(500);
    assert!(same_click_target(Some(&recent), &path, interval));
    assert!(!same_click_target(Some(&recent), std::path::Path::new("C:/replacement.txt"), interval));
    assert!(!same_click_target(None, &path, interval));
    let expired = (path.clone(), Instant::now() - Duration::from_secs(1));
    assert!(!same_click_target(Some(&expired), &path, interval));
}
