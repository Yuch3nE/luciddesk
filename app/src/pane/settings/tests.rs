use super::*;

#[test]
fn solid_defaults_follow_theme_until_a_color_is_saved() {
    let dir = tempfile::tempdir().unwrap();
    let store = luciddesk_storage::WorkspaceStore::open(&dir.path().join("workspace.db")).unwrap();
    for (dark, color) in [(true, 0x202020), (false, 0xf3f3f3)] {
        assert_eq!(solid_style(&store, dark), Backdrop::Solid { color, opacity: 0.85 });
    }
    store.save_preference("solid_style", "1193046|0.5").unwrap();
    for dark in [false, true] {
        assert_eq!(solid_style(&store, dark), Backdrop::Solid { color: 0x123456, opacity: 0.5 });
    }
}

#[test]
fn changing_font_releases_app_before_editor_destruction() {
    let _sta = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(super::super::tests::test_state()));
    create_view(&state, PanelId::new(1)).unwrap();
    let mut cancelled = 0;
    actions::refresh_font_views(&state, |hwnd| {
        assert!(!hwnd.is_null());
        assert!(state.try_borrow_mut().is_ok(), "editor destruction must allow app reentry");
        cancelled += 1;
    });
    assert_eq!(cancelled, 1);
}

#[test]
fn closing_settings_retries_without_consuming_pending_styles() {
    let state = Rc::new(RefCell::new(super::super::tests::test_state()));
    let mut pending = PendingStyles { grid: Some(100.0), ..Default::default() };
    let owner = state.borrow_mut();
    assert!(!lifecycle::close(&state, std::ptr::null_mut(), &mut pending));
    assert_eq!(pending.grid, Some(100.0));
    drop(owner);
}

#[test]
fn close_errors_are_reported_after_borrows_and_callback_return() {
    let state = Rc::new(RefCell::new(super::super::tests::test_state()));
    let reported = Rc::new(std::cell::Cell::new(false));
    let observed = reported.clone();
    let callback_state = state.clone();
    let owner = state.borrow_mut();
    report_close_errors(vec!["material failed".into(), "grid failed".into()], move |message| {
        assert!(callback_state.try_borrow_mut().is_ok());
        assert_eq!(message, "material failed\ngrid failed");
        observed.set(true);
    });
    assert!(!reported.get());
    drop(owner);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !reported.get() && std::time::Instant::now() < deadline {
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(reported.get());
    report_close_errors(Vec::new(), |_| panic!("no errors must not show a dialog"));
}

#[test]
fn backup_in_progress_preserves_policy_controls_and_prevents_duplicate_jobs() {
    for enabled in [false, true] {
        let mut body = scene(
            800.0,
            MIN_HEIGHT - TITLE_HEIGHT,
            6,
            true,
            (PanelTheme::System, Backdrop::Mica),
            luciddesk_core::PaneOptions::default(),
        );

        let view = recovery::View {
            busy: true,
            ..Default::default()
        };
        layout::backup_page(
            &mut body,
            800.0,
            &view,
            recovery::Policy {
                enabled,
                ..Default::default()
            },
            false,
        );
        let toggle = body
            .controls
            .iter()
            .find(|c| matches!(c.action, Action::BackupPolicy(0)))
            .unwrap();
        assert!(toggle.enabled);
        assert_eq!(toggle.selected, enabled);
        assert!(
            body.controls
                .iter()
                .filter(|c| matches!(c.action, Action::BackupPolicy(_)))
                .all(|c| c.enabled)
        );
        assert!(
            !body
                .controls
                .iter()
                .find(|c| matches!(c.action, Action::Change(Event::CreateBackup)))
                .unwrap()
                .enabled
        );
    }
}

#[test]
fn folder_modes_fit_minimum_window_and_show_current_choice() {
    for mode in [folder::EntryMode::Inline, folder::EntryMode::Explorer] {
        let mut body = scene(
            800.0,
            MIN_HEIGHT - TITLE_HEIGHT,
            8,
            false,
            (PanelTheme::System, Backdrop::Mica),
            luciddesk_core::PaneOptions::default(),
        );
        layout::folder_defaults(&mut body, 800.0, folder::Defaults::default(), mode);
        let s = with_titlebar(body, 800.0, false);
        for bounds in s
            .text
            .iter()
            .map(|(r, _, _)| r)
            .chain(s.controls.iter().map(|c| &c.bounds))
        {
            assert!(bounds.right <= 800.0);
        }
        let choices: Vec<_> = s
            .controls
            .iter()
            .filter(|c| matches!(c.action, Action::FolderEntryMode(_)))
            .collect();
        assert_eq!(choices.len(), 2);
        assert_eq!(choices.iter().filter(|c| c.selected).count(), 1);
        assert!(
            choices.iter().any(|c| c.selected
                && matches!(c.action, Action::FolderEntryMode(value) if value == mode))
        );
    }
}

#[test]
fn folder_defaults_are_saved_and_only_copied_into_new_panels() {
    let store = WorkspaceStore::open_in_memory().unwrap();
    assert_eq!(
        folder::Defaults::load(&store).unwrap(),
        folder::Defaults::default()
    );
    let mut first = Panel::new(
        PanelId::new(10),
        "first",
        luciddesk_core::RectDip::new(0.0, 0.0, 480.0, 360.0),
    );
    first.set_folder(Some(std::path::PathBuf::from(r"C:\first")));
    folder::Defaults::load(&store)
        .unwrap()
        .apply(&store, &mut first)
        .unwrap();
    folder::Defaults {
        list: false,
        columns: 8,
    }
    .save(&store)
    .unwrap();
    let saved = folder::Defaults::load(&store).unwrap();
    assert_eq!(saved.columns, 9);
    let mut second = Panel::new(PanelId::new(11), "second", first.rect());
    second.set_folder(Some(std::path::PathBuf::from(r"C:\second")));
    saved.apply(&store, &mut second).unwrap();
    assert!(!second.list_view());
    assert_eq!(folder::visible_columns(&store, second.id()).unwrap(), 9);
    folder::Defaults::default().save(&store).unwrap();
    assert!(first.list_view());
    assert_eq!(folder::visible_columns(&store, first.id()).unwrap(), 15);
    assert!(!second.list_view());
    assert_eq!(folder::visible_columns(&store, second.id()).unwrap(), 9);
}

#[test]
fn grid_slider_centers_default_and_scales_in_both_directions() {
    assert_eq!(grid_slider_position(100.0), 0.5);
    assert_eq!(grid_slider_value(0.5), 100.0);
    assert_eq!(grid_slider_value(0.0), grid_range().0);
    assert_eq!(grid_slider_value(1.0), grid_range().1);
    for value in 50..=200 {
        assert_eq!(
            grid_slider_value(grid_slider_position(value as f32)),
            value as f32
        );
    }
    assert!(grid_slider_value(0.25) < 100.0);
    assert!(grid_slider_value(0.75) > 100.0);
}

#[test]
fn radius_drag_preserves_fractional_values() {
    let bounds = Rect::from_xywh(0.0, 0.0, 180.0, 34.0);
    let first = radius_from_pointer(bounds, 80.0);
    let next = radius_from_pointer(bounds, 81.0);
    assert!(first.fract() != 0.0);
    assert!(next > first && next - first < 1.0);
    assert_eq!(radius_from_pointer(bounds, -10.0), 0.0);
    assert_eq!(radius_from_pointer(bounds, 190.0), 24.0);
}

#[test]
fn initial_library_show_remains_hidden_until_prepared() {
    let prepared = Rc::new(std::cell::Cell::new(false));
    let callback_prepared = Rc::clone(&prepared);
    let window = windows_window::Window::new("LucidDesk initial visibility test")
        .style(WS_OVERLAPPEDWINDOW)
        .on_message(move |_, msg, _, lp| {
            if unsafe { defer_show(msg, lp, callback_prepared.get()) } || msg == WM_DESTROY {
                Some(0)
            } else {
                None
            }
        })
        .create()
        .unwrap();
    unsafe {
        assert_eq!(IsWindowVisible(window.hwnd().cast()), 0);
        prepared.set(true);
        ShowWindow(window.hwnd().cast(), SW_SHOWNOACTIVATE);
        assert_ne!(IsWindowVisible(window.hwnd().cast()), 0);
    }
}

#[test]
fn settings_opacity_is_independent_and_rgb_preserves_other_channels() {
    for dark in [false, true] {
        for color in [0x123456, 0x7d4441, 0xffffff] {
            for opacity in [0.0, 0.5, 1.0] {
                assert_eq!(
                    settings_backdrop(Backdrop::Solid { color, opacity }, dark),
                    Backdrop::Solid {
                        color: if dark { 0x202020 } else { 0xf3f3f3 },
                        opacity: 1.0
                    }
                );
            }
        }
    }
    assert_eq!(
        settings_backdrop(Backdrop::Acrylic, true),
        Backdrop::Acrylic
    );
    for base in [Backdrop::Acrylic, Backdrop::Mica, Backdrop::MicaAlt] {
        for strength in [0, 50, 100] {
            assert_eq!(settings_backdrop(base.with_strength(strength), true), base);
        }
    }
    assert_eq!(color_channel(0x123456, 0, 255), 0xff3456);
    assert_eq!(color_channel(0x123456, 1, 0), 0x120056);
    assert_eq!(color_channel(0x123456, 2, 255), 0x1234ff);
    let picker = with_titlebar(
        scene(
            800.0,
            480.0 - TITLE_HEIGHT,
            7,
            false,
            (
                PanelTheme::Dark,
                Backdrop::Solid {
                    color: 0x123456,
                    opacity: 0.5,
                },
            ),
            Default::default(),
        ),
        800.0,
        false,
    );
    assert!(picker.controls.iter().all(|c| c.bounds.right <= 800.0));
    assert_eq!(
        picker
            .controls
            .iter()
            .filter(|c| matches!(c.action, Action::Channel(_, _)))
            .count(),
        3
    );
    assert_eq!(picker.previews.len(), 1);
}

#[test]
fn panel_options_text_fits_default_and_minimum_window() {
    for (width, height) in [(800.0, MIN_HEIGHT), (900.0, DEFAULT_HEIGHT as f32)] {
        let s = with_titlebar(
            scene(
                width,
                height - TITLE_HEIGHT,
                1,
                false,
                (PanelTheme::Dark, Backdrop::Mica),
                Default::default(),
            ),
            width,
            false,
        );
        for (bounds, text, _) in &s.text {
            assert!(bounds.right <= width, "Clipped text: {text}");
        }
        let last = s
            .controls
            .iter()
            .position(|c| matches!(c.action, Action::Change(Event::ResetPaneOptions)))
            .unwrap();
        let mut scrolled = s;
        let mut offset = f32::MAX;
        scrolled.scroll_to(width, height, &mut offset);
        assert!(offset > 0.0);
        let r = scrolled.controls[last].bounds;
        assert!(r.bottom <= height - 16.0);
        assert!(scrolled.accepts_pointer(&scrolled.controls[last], r.left + 1.0, r.top + 1.0));
    }
}

#[test]
fn solid_inputs_validate_color_and_opacity_without_changing_other_channels() {
    let solid = Backdrop::Solid {
        color: 0x123456,
        opacity: 0.85,
    };
    assert_eq!(
        edited_solid(solid, false, "#A1b2C3"),
        Some(Backdrop::Solid {
            color: 0xa1b2c3,
            opacity: 0.85
        })
    );
    for value in ["0", "50%", "100"] {
        assert!(edited_solid(solid, true, value).is_some());
    }
    for value in ["101", "-1", "NaN", ""] {
        assert!(edited_solid(solid, true, value).is_none());
    }
    for value in ["123", "GG0000", "1234567"] {
        assert!(edited_solid(solid, false, value).is_none());
    }
}

#[test]
fn switch_thumb_stays_centered_with_equal_end_insets() {
    for (width, height) in [(42.0, 22.0), (48.0, 24.0)] {
        let bounds = Rect::from_xywh(100.0, 50.0, width, height);
        for position in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let thumb = toggle_thumb(bounds, position);
            assert_eq!(thumb.center.y, 50.0 + height / 2.0);
            assert!(thumb.center.x - thumb.radius_x >= bounds.left + 4.0);
            assert!(thumb.center.x + thumb.radius_x <= bounds.right - 4.0);
        }
        assert_eq!(
            toggle_thumb(bounds, 0.0).center.x - toggle_thumb(bounds, 0.0).radius_x,
            bounds.left + 4.0
        );
        assert_eq!(
            toggle_thumb(bounds, 1.0).center.x + toggle_thumb(bounds, 1.0).radius_x,
            bounds.right - 4.0
        );
    }
}

#[test]
fn switch_motion_reverses_continuously_and_respects_disabled_animation() {
    let now = std::time::Instant::now();
    let _sta = crate::pane::test_support::apartment();
    let mut motion = ToggleMotion::settled(0.0, now);
    assert_eq!(motion.retarget(1.0, now, true), 0.0);
    let halfway = now + std::time::Duration::from_millis(80);
    let value = motion.sample(halfway).unwrap();
    assert!(value > 0.0 && value < 1.0);
    assert_eq!(motion.retarget(0.0, halfway, true), value);
    assert_eq!(
        motion
            .sample(halfway + std::time::Duration::from_millis(160))
            .unwrap(),
        0.0
    );
    assert_eq!(motion.retarget(1.0, halfway, false), 1.0);
}

#[test]
fn custom_frame_keeps_caption_buttons_and_resize_edges_separate() {
    assert_eq!(frame_hit(80.0, 16.0, 1040.0, 760.0, false), HTCAPTION);
    assert_eq!(frame_hit(1020.0, 16.0, 1040.0, 760.0, false), HTCLIENT);
    assert_eq!(frame_hit(2.0, 2.0, 1040.0, 760.0, false), HTTOPLEFT);
    assert_eq!(
        frame_hit(1038.0, 758.0, 1040.0, 760.0, false),
        HTBOTTOMRIGHT
    );
    assert_eq!(frame_hit(80.0, 2.0, 1040.0, 760.0, true), HTCAPTION);
    let s = with_titlebar(
        scene(
            1040.0,
            728.0,
            0,
            false,
            (PanelTheme::Dark, Backdrop::Mica),
            luciddesk_core::PaneOptions::default(),
        ),
        1040.0,
        false,
    );
    assert_eq!(
        s.controls
            .iter()
            .filter(|c| matches!(c.action, Action::Window(_)))
            .count(),
        3
    );
}
#[test]
fn settings_layout_and_rendering_at_multiple_scales() {
    let _apartment = crate::pane::test_support::apartment();
    let painter = Painter::new().unwrap();
    let export_snapshots = std::env::var_os("LUCIDDESK_TEST_EXPORT_SNAPSHOTS").is_some();
    {
        let device = windows_canvas::GpuDevice::new_warp().unwrap();
        for (viewport_width, viewport_height) in [(800.0, 560.0), (940.0, 620.0)] {
            for at_bottom in [false, true] {
                for scale in [1.0, 1.5, 2.0] {
                    for page in [0, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11] {
                        for dark in [false, true] {
                            let mut body = scene(
                                viewport_width,
                                viewport_height - TITLE_HEIGHT,
                                page,
                                true,
                                (
                                    PanelTheme::System,
                                    if page == 0 {
                                        Backdrop::Acrylic.with_strength(65)
                                    } else if page == 7 {
                                        Backdrop::Solid {
                                            color: 0x24364b,
                                            opacity: 0.85,
                                        }
                                    } else {
                                        Backdrop::Mica
                                    },
                                ),
                                luciddesk_core::PaneOptions::default(),
                            );
                            if matches!(page, 6 | 9 | 10) {
                                let view = recovery::View {
                                    status: "手动备份成功 · 上次备份：今天 14:32".into(),
                                    records: (0..5)
                                        .map(|i| recovery::Record {
                                            path: std::path::PathBuf::from(format!(
                                                "backup-{i}.db"
                                            )),
                                            date: "2026/09/14 14:32".into(),
                                            kind: if i == 0 { "手动" } else { "自动" },
                                            bytes: 131072,
                                        })
                                        .collect(),
                                    ..Default::default()
                                };
                                if page == 9 {
                                    layout::backup_history(&mut body, viewport_width, &view, 0);
                                } else {
                                    layout::backup_page(
                                        &mut body,
                                        viewport_width,
                                        &view,
                                        recovery::Policy::default(),
                                        page == 10,
                                    );
                                }
                            }
                            if page == 11 {
                                layout::fonts(&mut body, viewport_width, &fonts::installed(), 0);
                            }
                            if page == 8 {
                                layout::folder_defaults(
                                    &mut body,
                                    viewport_width,
                                    folder::Defaults::default(),
                                    if dark {
                                        folder::EntryMode::Explorer
                                    } else {
                                        folder::EntryMode::Inline
                                    },
                                );
                            }
                            if page == 5 {
                                layout::about_status(
                                    &mut body,
                                    viewport_width,
                                    "桌面面板已连接",
                                    false,
                                );
                            }
                            let mut s = with_titlebar(body, viewport_width, false);
                            s.scroll_to(
                                viewport_width,
                                viewport_height,
                                &mut if at_bottom { f32::MAX } else { 0.0 },
                            );
                            for c in &s.controls {
                                assert!(
                                    c.bounds.left >= 0.0
                                        && c.bounds.top >= -s.scroll_max
                                        && c.bounds.right <= viewport_width
                                        && c.bounds.bottom <= viewport_height + s.scroll_max
                                );
                                assert!(contains(
                                    &c.bounds,
                                    (c.bounds.left + c.bounds.right) / 2.0,
                                    (c.bounds.top + c.bounds.bottom) / 2.0
                                ));
                            }
                            let width = (viewport_width * scale) as u32;
                            let height = (viewport_height * scale) as u32;
                            let bitmap =
                                super::super::canvas::Offscreen::new(&device, width, height)
                                    .unwrap();
                            let target = bitmap.target.clone();
                            painter
                                .paint(
                                    &target,
                                    &s,
                                    viewport_width,
                                    viewport_height,
                                    scale,
                                    dark,
                                    false,
                                    None,
                                    None,
                                    &std::collections::HashMap::new(),
                                )
                                .unwrap();
                            let pixels = bitmap.pixels().unwrap();
                            assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
                            assert_eq!(pixels[0] < 128, dark);
                            if export_snapshots && scale == 1.0 {
                                // Standalone raster for visual review, independent of the live desktop.
                                let mut bmp = vec![0u8; 54];
                                bmp[0..2].copy_from_slice(b"BM");
                                bmp[2..6]
                                    .copy_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
                                bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
                                bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
                                bmp[18..22].copy_from_slice(&(width as i32).to_le_bytes());
                                bmp[22..26].copy_from_slice(&(-(height as i32)).to_le_bytes());
                                bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
                                bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
                                bmp.extend(pixels);
                                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                                    .join("../target")
                                    .join(match (page, dark) {
                                        (11, true) => "settings-fonts-dark.bmp",
                                        (11, false) => "settings-fonts-light.bmp",
                                        (8, true) => "settings-folder-dark.bmp",
                                        (8, false) => "settings-folder-light.bmp",
                                        (3, true) => "settings-peek-dark.bmp",
                                        (3, false) => "settings-peek-light.bmp",
                                        (4, true) => "settings-search-dark.bmp",
                                        (4, false) => "settings-search-light.bmp",
                                        (9, true) => "settings-backup-history-dark.bmp",
                                        (9, false) => "settings-backup-history-light.bmp",
                                        (10, true) => "settings-backup-advanced-dark.bmp",
                                        (10, false) => "settings-backup-advanced-light.bmp",
                                        (6, true) => "settings-backup-dark.bmp",
                                        (6, false) => "settings-backup-light.bmp",
                                        (7, true) => "settings-colors-dark.bmp",
                                        (7, false) => "settings-colors-light.bmp",
                                        (5, true) => "settings-about-dark.bmp",
                                        (5, false) => "settings-about-light.bmp",
                                        (1, true) => "settings-pane-dark.bmp",
                                        (1, false) => "settings-pane-light.bmp",
                                        (_, true) => "settings-dark.bmp",
                                        (_, false) => "settings-light.bmp",
                                    });
                                let path = if viewport_width == 800.0 || at_bottom {
                                    path.with_file_name(format!(
                                        "{}-{}{}.bmp",
                                        path.file_stem().unwrap().to_string_lossy(),
                                        viewport_width as u32,
                                        if at_bottom { "-bottom" } else { "" }
                                    ))
                                } else {
                                    path
                                };
                                std::fs::write(path, bmp).unwrap();
                            }
                            if page == 0 && !at_bottom {
                                painter
                                    .paint(
                                        &target,
                                        &s,
                                        viewport_width,
                                        viewport_height,
                                        scale,
                                        dark,
                                        true,
                                        None,
                                        None,
                                        &std::collections::HashMap::new(),
                                    )
                                    .unwrap();
                                let overlay = bitmap.pixels().unwrap();
                                assert_eq!(
                                    overlay[3], 0,
                                    "the native material must remain visible beneath the sidebar"
                                );
                                let card_at = (((190.0 * scale) as u32 * width
                                    + (270.0 * scale) as u32)
                                    * 4) as usize;
                                assert!(
                                    overlay[card_at + 3] > 0 && overlay[card_at + 3] < 255,
                                    "settings content must retain material transparency"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn font_picker_fits_minimum_window_and_exposes_list_and_reset() {
    let names: Vec<_> = (0..19).map(|i| format!("Font {i}")).collect();
    for offset in [0, 7, 14] {
        let mut body = scene(
            800.0,
            MIN_HEIGHT - TITLE_HEIGHT,
            11,
            false,
            (PanelTheme::Dark, Backdrop::Mica),
            luciddesk_core::PaneOptions::default(),
        );
        layout::fonts(&mut body, 800.0, &names, offset);
        let s = with_titlebar(body, 800.0, false);
        for r in s
            .text
            .iter()
            .map(|(r, _, _)| r)
            .chain(s.controls.iter().map(|c| &c.bounds))
        {
            assert!(r.right <= 800.0);
        }
        assert_eq!(
            s.controls
                .iter()
                .filter(|c| matches!(&c.action, Action::Font(name) if name.starts_with("Font ")))
                .count(),
            names.len()
        );
        assert!(
            s.controls
                .iter()
                .any(|c| matches!(&c.action, Action::Font(name) if name == assets::UI_FONT))
        );
    }
}

#[test]
fn setting_cards_scroll_without_moving_navigation_or_hitting_caption() {
    for page in [0, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11] {
        for width in [800.0, 940.0, 1440.0] {
            let build = || {
                let mut body = scene(
                    width,
                    520.0,
                    page,
                    false,
                    (
                        PanelTheme::Dark,
                        Backdrop::Solid {
                            color: 0x24364b,
                            opacity: 0.85,
                        },
                    ),
                    Default::default(),
                );
                let view = recovery::View {
                    records: (0..8)
                        .map(|i| recovery::Record {
                            path: std::path::PathBuf::from(format!("backup-{i}.db")),
                            date: "2026/09/27 14:32".into(),
                            kind: "手动",
                            bytes: 131072,
                        })
                        .collect(),
                    undo: Some("undo.db".into()),
                    ..Default::default()
                };
                match page {
                    5 => layout::about_status(&mut body, width, "桌面面板已连接", false),
                    6 | 10 => layout::backup_page(
                        &mut body,
                        width,
                        &view,
                        recovery::Policy::default(),
                        page == 10,
                    ),
                    8 => layout::folder_defaults(
                        &mut body,
                        width,
                        folder::Defaults::default(),
                        folder::EntryMode::Inline,
                    ),
                    9 => layout::backup_history(&mut body, width, &view, 0),
                    11 => layout::fonts(
                        &mut body,
                        width,
                        &(0..19).map(|i| format!("Font {i}")).collect::<Vec<_>>(),
                        0,
                    ),
                    _ => {}
                }
                with_titlebar(body, width, false)
            };
            let original = build();
            for pair in original.cards.windows(2) {
                assert!(pair[0].bottom <= pair[1].top);
            }
            for index in 0..original.controls.len() {
                let control = &original.controls[index];
                if control.bounds.left < Tokens::content_x()
                    || matches!(control.kind, ControlKind::Caption)
                {
                    continue;
                }
                let mut body = build();
                let origin = if body.fixed_list() && Scene::list_row(control) {
                    body.controls.iter().filter(|c| Scene::list_row(c)).map(|c| c.bounds.top).fold(f32::MAX, f32::min)
                } else { TITLE_HEIGHT + 16.0 };
                let mut offset = (control.bounds.top - origin).max(0.0);
                body.scroll_to(width, 560.0, &mut offset);
                assert_eq!(body.controls[0].bounds.top, original.controls[0].bounds.top);
                let c = &body.controls[index];
                assert!(body.accepts_pointer(c, c.bounds.left + 1.0, c.bounds.top + 1.0));
                assert!(!body.accepts_pointer(c, c.bounds.left + 1.0, TITLE_HEIGHT - 1.0));
            }
        }
    }
}

#[test]
fn settings_cards_align_and_long_paths_do_not_overlap_actions() {
    for width in [800.0, 940.0, 1440.0] {
        let mut s = scene(width, 520.0, usize::MAX, false,
            (PanelTheme::Dark, Backdrop::Mica), Default::default());
        let path = format!(
            r"C:\Users\测试用户\{}\Everything.exe",
            "很长的文件夹名称".repeat(20)
        );
        let mut form = SettingsForm::new(&mut s, width, "布局测试");
        form.toggle("开关", "说明", true, Action::PeekEnable);
        form.slider(
            "滑块",
            "说明",
            Slider::linear(50.0, 100.0),
            "50%",
            Action::Opacity(50),
        );
        form.button("按钮", "说明", "浏览", Action::PeekBrowse);
        form.combo("下拉", "说明", "5 分钟", Action::BackupPolicy(1));
        form.path(
            "程序路径",
            &path,
            vec![
                ("浏览", Action::PeekBrowse),
                ("自动检测", Action::PeekDetect),
            ],
        );
        form.shortcut(
            "全局快捷键",
            "说明",
            "Ctrl + Shift + Space",
            Action::SearchShortcut,
            Action::SearchReset,
        );
        let right =
            Tokens::content_x() + (width - Tokens::content_x() - Tokens::MARGIN).min(Tokens::MAX_WIDTH);
        for card in &s.cards {
            assert_eq!(card.left, Tokens::content_x());
            assert_eq!(card.right, right);
            for control in s.controls.iter().filter(|c| {
                c.bounds.top >= card.top
                    && c.bounds.bottom <= card.bottom
                    && c.bounds.left >= card.left
            }) {
                assert!(control.bounds.left >= card.left + Tokens::INSET);
                assert!(control.bounds.right <= card.right - Tokens::INSET);
                for (text, _, _) in s
                    .text
                    .iter()
                    .filter(|(r, _, _)| r.top >= card.top && r.bottom <= card.bottom)
                {
                    assert!(
                        text.right <= control.bounds.left
                            || text.left >= control.bounds.right
                            || text.bottom <= control.bounds.top
                            || text.top >= control.bounds.bottom,
                        "text and control overlap"
                    );
                }
            }
        }
        let path_bounds = s.text.iter().find(|(_, text, _)| text == &path).unwrap().0;
        let path_card = s
            .cards
            .iter()
            .find(|r| path_bounds.top >= r.top && path_bounds.bottom <= r.bottom)
            .unwrap();
        assert!(
            s.controls
                .iter()
                .filter(|c| c.bounds.left >= path_card.left && c.bounds.right <= path_card.right
                    && c.bounds.top >= path_card.top && c.bounds.bottom <= path_card.bottom)
                .all(|c| c.bounds.top >= path_bounds.bottom + 12.0)
        );
        let shortcut = s
            .controls
            .iter()
            .find(|c| matches!(c.action, Action::SearchShortcut))
            .unwrap();
        assert_eq!(shortcut.bounds.right - shortcut.bounds.left, 232.0);
    }
}

#[test]
fn pagination_disables_unavailable_directions_and_short_pages_do_not_scroll() {
    let mut s = with_titlebar(
        scene(
            940.0,
            968.0,
            8,
            false,
            (PanelTheme::Dark, Backdrop::Mica),
            Default::default(),
        ),
        940.0,
        false,
    );
    let mut form = SettingsForm::new(&mut s, 940.0, "短页面");
    form.info("状态", "无需滚动");
    s.scroll_to(940.0, 1000.0, &mut 100.0);
    assert_eq!(s.scroll_max, 0.0);
    assert_eq!(s.scroll_offset, 0.0);
    assert!(s.scroll_thumb().is_none());
}

#[test]
fn color_channel_titles_align_with_slider_centers() {
    for width in [800.0, 940.0, 1440.0] {
        let s = scene(
            width,
            520.0,
            7,
            false,
            (
                PanelTheme::Dark,
                Backdrop::Solid {
                    color: 0x24364b,
                    opacity: 0.85,
                },
            ),
            Default::default(),
        );
        for (i, title) in ["红 R", "绿 G", "蓝 B"].iter().enumerate() {
            let label = s.text.iter().find(|(_, text, _)| text == title).unwrap().0;
            let control = s
                .controls
                .iter()
                .find(|c| matches!(c.action, Action::Channel(channel, _) if channel as usize == i))
                .unwrap()
                .bounds;
            assert_eq!(
                (label.top + label.bottom) / 2.0,
                (control.top + control.bottom) / 2.0
            );
        }
    }
}

#[test]
fn material_choices_reach_the_preview_with_their_strength() {
    let _apartment = crate::pane::test_support::apartment();
    let painter = Painter::new().unwrap();
    let device = windows_canvas::GpuDevice::new_warp().unwrap();
    for dark in [false, true] {
        for (name, material) in [
            ("mica", Backdrop::Mica),
            ("mica-alt", Backdrop::MicaAlt),
            ("acrylic", Backdrop::Acrylic),
        ] {
            for strength in [0, 50, 100] {
                let material = material.with_strength(strength);
                let mut s = with_titlebar(
                    scene(
                        940.0,
                        588.0,
                        0,
                        false,
                        (
                            if dark {
                                PanelTheme::Dark
                            } else {
                                PanelTheme::Light
                            },
                            material,
                        ),
                        Default::default(),
                    ),
                    940.0,
                    false,
                );
                assert_eq!(s.previews[0].1, material);
                s.scroll_to(940.0, 620.0, &mut 0.0);
                if strength != 50 {
                    continue;
                }
                let bitmap = super::super::canvas::Offscreen::new(&device, 940, 620).unwrap();
                painter
                    .paint(
                        &bitmap.target,
                        &s,
                        940.0,
                        620.0,
                        1.0,
                        dark,
                        false,
                        None,
                        None,
                        &Default::default(),
                    )
                    .unwrap();
                if std::env::var_os("LUCIDDESK_TEST_EXPORT_SNAPSHOTS").is_some() {
                    let pixels = bitmap.pixels().unwrap();
                    let mut bmp = vec![0u8; 54];
                    bmp[0..2].copy_from_slice(b"BM");
                    bmp[2..6].copy_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
                    bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
                    bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
                    bmp[18..22].copy_from_slice(&940i32.to_le_bytes());
                    bmp[22..26].copy_from_slice(&(-620i32).to_le_bytes());
                    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
                    bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
                    bmp.extend(pixels);
                    std::fs::write(
                        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../target")
                            .join(format!(
                                "settings-{name}-{}.bmp",
                                if dark { "dark" } else { "light" }
                            )),
                        bmp,
                    )
                    .unwrap();
                }
            }
        }
    }
}


#[test]
fn corner_slider_drags_to_both_limits_and_saves() {
    let _sta = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(super::super::tests::test_state()));
    state.borrow_mut().workspace.set_appearance(PanelTheme::Dark, Backdrop::Mica);
    show(&state, PanelId::new(1)).unwrap();
    let hwnd = state.borrow().settings.as_ref().unwrap().hwnd().cast();
    let nav = control_position(&state, hwnd, 0, |a| matches!(a, Action::Page(1)), 0.5);
    unsafe {
        SendMessageW(hwnd, WM_LBUTTONDOWN, 0, nav);
        SendMessageW(hwnd, WM_LBUTTONUP, 0, nav);
    }
    let radius = |fraction| control_position(&state, hwnd, 1, |a| matches!(a, Action::Radius(_)), fraction);
    let (middle, left, right) = (radius(0.5), radius(0.0), radius(1.0));
    let saved = || state.borrow().store.load_workspace().unwrap().pane_options().corner_radius;
    unsafe {
        SendMessageW(hwnd, WM_LBUTTONDOWN, 0, middle);
        assert_eq!(state.borrow().workspace.pane_options().corner_radius, 12.0);
        SendMessageW(hwnd, WM_MOUSEMOVE, 1, left);
        assert_eq!(state.borrow().workspace.pane_options().corner_radius, 0.0);
        assert_eq!(saved(), luciddesk_core::PaneOptions::DEFAULT.corner_radius, "preview must not write storage");
        SendMessageW(hwnd, WM_MOUSEMOVE, 1, right);
        SendMessageW(hwnd, WM_LBUTTONUP, 0, right);
        assert_eq!(saved(), 24.0);
        SendMessageW(hwnd, WM_LBUTTONDOWN, 0, middle);
        SendMessageW(hwnd, WM_CAPTURECHANGED, 0, 0);
        assert_eq!(saved(), 12.0, "losing capture must finish the preview");
        SendMessageW(hwnd, WM_CLOSE, 0, 0);
    }
}

fn control_position(
    state: &Rc<RefCell<PaneApp>>, hwnd: windows_sys::Win32::Foundation::HWND, page: usize,
    predicate: fn(&Action) -> bool, fraction: f32,
) -> isize {
    unsafe {
        // Reset actual scrolling, then locate the semantic control in the current layout.
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let mut pointer = windows_sys::Win32::Foundation::POINT {
            x: ((Tokens::content_x() + 32.0) * scale) as i32,
            y: (200.0 * scale) as i32,
        };
        ClientToScreen(hwnd, &raw mut pointer);
        let screen_point = ((pointer.y as isize) << 16) | (pointer.x as isize & 0xffff);
        SendMessageW(hwnd, WM_MOUSEWHEEL, (32760usize) << 16, screen_point);
        let mut rect = RECT::default();
        GetClientRect(hwnd, &raw mut rect);
        let w = rect.right as f32 / scale;
        let h = rect.bottom as f32 / scale;
        let s = state.borrow();
        let appearance = s.workspace.appearance().unwrap_or((PanelTheme::System, Backdrop::Mica));
        let mut body = with_titlebar(scene(w, h - TITLE_HEIGHT, page, false, appearance, Default::default()), w, false);
        let mut scroll = 0.0;
        body.scroll_to(w, h, &mut scroll);
        let bounds = body.controls.iter().find(|c| predicate(&c.action)).expect("settings control missing").bounds;
        drop(s);
        let viewport = body.viewport.unwrap();
        while bounds.bottom - scroll > viewport.bottom {
            SendMessageW(hwnd, WM_MOUSEWHEEL, ((-120i16) as u16 as usize) << 16, screen_point);
            let next = (scroll + 64.0).min(body.scroll_max);
            assert!(next > scroll, "control cannot be scrolled into view");
            scroll = next;
        }
        SendMessageW(hwnd, WM_PAINT, 0, 0);
        let x = (bounds.left + (bounds.right - bounds.left) * fraction) * scale;
        let y = ((bounds.top + bounds.bottom) * 0.5 - scroll) * scale;
        ((y as isize) << 16) | (x as isize & 0xffff)
    }
}

// Called by the isolated native-window scenario so composition stays on one STA.
pub(in crate::pane) fn preference_pages_survive_reentry(
    state: &Rc<RefCell<PaneApp>>, hwnd: windows_sys::Win32::Foundation::HWND,
) {
    for choose in [
        (|a: &Action| matches!(a, Action::Page(1))) as fn(&Action) -> bool,
        |a| matches!(a, Action::Page(12)),
        |a| matches!(a, Action::Page(0)),
    ] {
        let point = control_position(state, hwnd, 0, choose, 0.5);
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, 1, point);
            SendMessageW(hwnd, WM_LBUTTONUP, 0, point);
            let _updating = state.borrow_mut();
            // Force a layout rebuild as well as repaint while configuration is unavailable.
            SendMessageW(hwnd, crate::i18n::CHANGED, 0, 0);
            SendMessageW(hwnd, WM_SIZE, 0, 0);
            SendMessageW(hwnd, WM_PAINT, 0, 0);
        }
    }
}

pub(in crate::pane) fn solid_settings_edit_preview_save_and_remember_style() {
    let state = Rc::new(RefCell::new(super::super::tests::test_state()));
    show(&state, PanelId::new(1)).unwrap();
    let hwnd = state.borrow().settings.as_ref().unwrap().hwnd().cast();
    let point = |page, predicate, fraction| control_position(&state, hwnd, page, predicate, fraction);
    let click = |p| unsafe {
        SendMessageW(hwnd, WM_LBUTTONDOWN, 1, p);
        SendMessageW(hwnd, WM_LBUTTONUP, 0, p);
        SendMessageW(hwnd, WM_PAINT, 0, 0);
    };
    click(point(0, |a| matches!(a, Action::Change(Event::Material(Backdrop::Acrylic))), 0.5));
    let before = state.borrow().store.change_count();
    let slider = point(0, |a| matches!(a, Action::Strength(_)), 0.75);
    unsafe { SendMessageW(hwnd, WM_LBUTTONDOWN, 1, slider); }
    let adjusted = state.borrow().workspace.appearance().unwrap().1;
    assert_ne!(adjusted.strength(), Some(50));
    assert_eq!(state.borrow().store.change_count(), before, "drag must defer persistence");
    unsafe { SendMessageW(hwnd, WM_LBUTTONUP, 0, slider); }
    assert_eq!(state.borrow().store.load_workspace().unwrap().appearance().unwrap().1, adjusted);
    click(point(0, |a| matches!(a, Action::Change(Event::Material(Backdrop::Mica))), 0.5));
    assert_eq!(state.borrow().workspace.appearance().unwrap().1.strength(), Some(50));
    click(point(0, |a| matches!(a, Action::Change(Event::Material(Backdrop::Acrylic))), 0.5));
    assert_eq!(state.borrow().workspace.appearance().unwrap().1, adjusted);
    click(point(0, |a| matches!(a, Action::StrengthReset), 0.5));
    assert_eq!(state.borrow().workspace.appearance().unwrap().1, Backdrop::Acrylic);
    click(point(0, |a| matches!(a, Action::Strength(_)), 0.5));
    for (key, value) in [(VK_HOME, 0), (VK_END, 100)] {
        unsafe { SendMessageW(hwnd, WM_KEYDOWN, key as usize, 0); }
        assert_eq!(state.borrow().workspace.appearance().unwrap().1.strength(), Some(value));
    }
    click(point(0, |a| matches!(a, Action::Change(Event::Material(Backdrop::Solid { .. }))), 0.5));
    click(point(0, |a| matches!(a, Action::SolidColor), 0.5));
    click(point(7, |a| matches!(a, Action::StyleInput(false)), 0.5));
    unsafe {
        for c in "#1234AB".chars() { SendMessageW(hwnd, WM_CHAR, c as usize, 0); }
        SendMessageW(hwnd, WM_KEYDOWN, VK_RETURN as usize, 0);
    }
    assert!(matches!(state.borrow().store.load_workspace().unwrap().appearance().unwrap().1,
        Backdrop::Solid { color: 0x1234ab, .. }));
    unsafe {
        SendMessageW(hwnd, WM_KEYDOWN, VK_ESCAPE as usize, 0);
        SendMessageW(hwnd, WM_PAINT, 0, 0);
    }
    assert!(state.borrow().settings.is_some(), "Escape should leave the picker");
    let before = state.borrow().store.change_count();
    let opacity = point(0, |a| matches!(a, Action::Opacity(_)), 0.5);
    unsafe { SendMessageW(hwnd, WM_LBUTTONDOWN, 1, opacity); }
    assert_eq!(state.borrow().store.change_count(), before);
    unsafe { SendMessageW(hwnd, WM_LBUTTONUP, 0, opacity); }
    let solid = state.borrow().workspace.appearance().unwrap().1;
    assert!(matches!(solid, Backdrop::Solid { color: 0x1234ab, opacity } if (opacity - 0.5).abs() < 0.02));
    assert_eq!(state.borrow().store.load_workspace().unwrap().appearance().unwrap().1, solid);
    click(point(0, |a| matches!(a, Action::Change(Event::Material(Backdrop::Mica))), 0.5));
    click(point(0, |a| matches!(a, Action::Change(Event::Material(Backdrop::Solid { .. }))), 0.5));
    assert_eq!(state.borrow().workspace.appearance().unwrap().1, solid);
    unsafe { SendMessageW(hwnd, WM_CLOSE, 0, 0); }
}


#[test]
fn all_languages_layout_and_render_without_control_overflow() {
    let _apartment = crate::pane::test_support::apartment();
    let device = windows_canvas::GpuDevice::new_warp().unwrap();
    for locale in 0..7 {
        crate::i18n::with_locale(locale, || {
            let painter = Painter::new().unwrap();
            for page in [0, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13] {
                let width = 800.0;
                let height = MIN_HEIGHT;
                let material = if page == 7 { Backdrop::Solid { color: 0xf3f3f3, opacity: 1.0 } } else { Backdrop::Acrylic };
                let mut body = scene(width, height - TITLE_HEIGHT, page, true,
                    (PanelTheme::Light, material), luciddesk_core::PaneOptions::default());
                match page {
                    1 => layout::show_panels_shortcut(&mut body, width, false, show_hotkey::default_shortcut()),
                    5 => layout::about_status(&mut body, width, "桌面面板已连接", true),
                    8 => layout::folder_defaults(&mut body, width, folder::Defaults::default(), folder::EntryMode::Inline),
                    9 => layout::backup_history(&mut body, width, &recovery::View::default(), 0),
                    6 | 10 => layout::backup_page(&mut body, width, &recovery::View::default(), recovery::Policy::default(), page == 10),
                    11 => layout::fonts(&mut body, width, &fonts::installed(), 0),
                    12 => layout::language(&mut body, width, "system"),
                    13 => layout::general(&mut body, width, crate::startup::Status::DisabledByWindows, false, true, false),
                    _ => {},
                }
                let mut scene = with_titlebar(body, width, false);
                scene.scroll_to(width, height, &mut 0.0);
                for bounds in scene.text.iter().map(|(bounds, _, _)| bounds)
                    .chain(scene.app_icon.iter())
                    .chain(scene.cards.iter())
                    .chain(scene.controls.iter().map(|control| &control.bounds))
                {
                    assert!(bounds.left >= 0.0 && bounds.right <= width, "locale={locale}, page={page}");
                }
                let navigation: Vec<_> = scene.controls.iter().filter(|c| matches!(c.kind, ControlKind::Navigation)).collect();
                for pair in navigation.windows(2) { assert!(pair[0].bounds.bottom <= pair[1].bounds.top); }
                for c in &scene.controls {
                    if matches!(c.kind, ControlKind::Navigation) {
                        assert!(painter.label_width(&c.label).unwrap() <= c.bounds.right - c.bounds.left - controls::Style::NAV_TEXT_INSET - 8.0, "Navigation truncated: {}", c.label);
                    }
                }
                let scale = 1.0;
                let w = (width * scale) as u32;
                let h = (height * scale) as u32;
                let bitmap = super::super::canvas::Offscreen::new(&device, w, h).unwrap();
                painter.paint(&bitmap.target, &scene, width, height, scale, false, false, None, None, &Default::default()).unwrap();
                if std::env::var_os("LUCIDDESK_TEST_EXPORT_SNAPSHOTS").is_some() {
                    let pixels = bitmap.pixels().unwrap();
                    let mut bmp = vec![0u8; 54];
                    bmp[..2].copy_from_slice(b"BM");
                    bmp[2..6].copy_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
                    bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
                    bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
                    bmp[18..22].copy_from_slice(&(w as i32).to_le_bytes());
                    bmp[22..26].copy_from_slice(&(-(h as i32)).to_le_bytes());
                    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
                    bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
                    bmp.extend(pixels);
                    std::fs::write(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../target/i18n-{locale}-{page}.bmp")), bmp).unwrap();
                }
            }
        });
    }
}

#[test]
fn show_panels_shortcut_is_off_and_accessible_in_every_language() {
    for locale in 0..7 {
        crate::i18n::with_locale(locale, || {
            let mut body = scene(800.0, 560.0 - TITLE_HEIGHT, 1, false,
                (PanelTheme::Light, Backdrop::Acrylic), Default::default());
            layout::show_panels_shortcut(&mut body, 800.0, false, show_hotkey::default_shortcut());
            let mut s = with_titlebar(body, 800.0, false);
            let toggle = s.controls.iter().find(|c| matches!(c.action, Action::ShowPanelsEnable)).unwrap();
            assert!(!toggle.selected);
            let mut offset = toggle.bounds.top - TITLE_HEIGHT - 16.0;
            s.scroll_to(800.0, 560.0, &mut offset);
            for c in s.controls.iter().filter(|c| matches!(c.action, Action::ShowPanelsEnable | Action::ShowPanelsShortcut | Action::ShowPanelsReset)) {
                assert!(c.bounds.right <= 800.0 && c.bounds.left >= Tokens::content_x());
                assert!(s.accepts_pointer(c, c.bounds.left + 1.0, c.bounds.top + 1.0));
            }
        });
    }
}

#[test]
fn font_search_uses_owned_surface_and_accepts_unicode_text() {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let owner = windows_window::Window::new("Font search test").style(WS_POPUP)
        .on_message(|_, message, _, _| (message == WM_DESTROY).then_some(0)).create().unwrap();
    let hwnd = super::create_font_search(owner.hwnd().cast());
    assert!(!hwnd.is_null());
    unsafe {
        assert_eq!(GetWindow(hwnd, GW_OWNER), owner.hwnd().cast());
        assert_eq!(GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CHILD, 0);
        assert_ne!(GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_POPUP, 0);
        SetWindowTextW(hwnd, windows_sys::w!("微软雅黑 UI"));
        let mut text = [0u16; 64];
        let length = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
        assert_eq!(String::from_utf16_lossy(&text[..length as usize]), "微软雅黑 UI");
        DestroyWindow(hwnd);
    }
}

#[test]
fn font_list_scroll_keeps_search_and_current_font_fixed() {
    let names: Vec<_> = (0..200).map(|i| format!("Font {i}")).collect();
    let make = || {
        let mut body = scene(940.0, 588.0, 11, false, (PanelTheme::Dark, Backdrop::Mica), Default::default());
        layout::fonts(&mut body, 940.0, &names, 0);
        with_titlebar(body, 940.0, false)
    };
    let mut first = make(); first.scroll_to(940.0, 620.0, &mut 0.0);
    let mut scrolled = make(); scrolled.scroll_to(940.0, 620.0, &mut 480.0);
    let field = |s: &Scene| s.controls.iter().find(|c| matches!(c.action, Action::FontSearch)).unwrap().bounds;
    assert_eq!(field(&first), field(&scrolled));
    assert_eq!(first.cards, scrolled.cards);
    assert!(scrolled.scroll_max > 0.0);
    let r = field(&scrolled);
    assert!(font_search_hit(&scrolled, r.left + 10.0, r.top + 10.0));
    let row = scrolled.controls.iter().find(|c| Scene::list_row(c)).unwrap();
    assert!(!scrolled.accepts_pointer(row, row.bounds.left + 1.0, row.bounds.top + 1.0));
    let visible = scrolled.controls.iter().filter(|c| Scene::list_row(c)
        && scrolled.viewport.is_some_and(|v| c.bounds.bottom > v.top && c.bounds.top < v.bottom)).count();
    assert!(visible > 0 && visible < 10);
}

#[test]
#[ignore = "Native composition window; run alone to isolate STA graphics lifetime"]
fn native_font_search_tracks_window_and_handles_clear_and_page_leave() {
    let _sta = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(super::super::tests::test_state()));
    show(&state, PanelId::new(1)).unwrap();
    let hwnd = state.borrow().settings.as_ref().unwrap().hwnd().cast();
    unsafe {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let mut bounds = RECT::default(); GetClientRect(hwnd, &raw mut bounds);
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let width = bounds.right as f32 / scale;
        let nav = with_titlebar(scene(width, bounds.bottom as f32 / scale - TITLE_HEIGHT, 0,
            false, (PanelTheme::Dark, Backdrop::Mica), Default::default()), width, false);
        let click = |rect: Rect| {
            let x = ((rect.left + rect.right) * 0.5 * scale) as u16;
            let y = ((rect.top + rect.bottom) * 0.5 * scale) as u16;
            let lp = (u32::from(x) | (u32::from(y) << 16)) as isize;
            SendMessageW(hwnd, WM_LBUTTONDOWN, 1, lp); SendMessageW(hwnd, WM_LBUTTONUP, 0, lp);
            SendMessageW(hwnd, WM_PAINT, 0, 0);
        };
        click(nav.controls.iter().find(|c| matches!(c.action, Action::Page(11))).unwrap().bounds);
        let editor = GetPropW(hwnd, windows_sys::w!("LucidDesk.FontSearch"));
        assert!(!editor.is_null());
        assert_ne!(IsWindowVisible(editor), 0);
        SetWindowTextW(editor, windows_sys::w!("微软雅黑"));
        assert_eq!(GetWindowTextLengthW(editor), 4);
        crate::i18n::with_locale(2, || {
            SendMessageW(hwnd, crate::i18n::CHANGED, 0, 0);
            SendMessageW(hwnd, WM_PAINT, 0, 0);
            assert_eq!(GetPropW(hwnd, windows_sys::w!("LucidDesk.FontSearch")), editor);
            let mut query = [0u16; 32];
            let count = GetWindowTextW(editor, query.as_mut_ptr(), query.len() as i32);
            assert_eq!(String::from_utf16_lossy(&query[..count as usize]), "微软雅黑");
        });

        SendMessageW(editor, WM_KEYDOWN, VK_ESCAPE as usize, 0);
        assert_eq!(GetWindowTextLengthW(editor), 0);
        SetWindowTextW(editor, windows_sys::w!("missing-font"));
        SendMessageW(hwnd, WM_PAINT, 0, 0);
        let mut field = RECT::default(); GetWindowRect(editor, &raw mut field);
        let mut point = windows_sys::Win32::Foundation::POINT { x: field.right + (16.0 * scale) as i32, y: (field.top + field.bottom) / 2 };
        ScreenToClient(hwnd, &raw mut point);
        let lp = (point.x as u16 as u32 | ((point.y as u16 as u32) << 16)) as isize;
        SendMessageW(hwnd, WM_LBUTTONDOWN, 1, lp); SendMessageW(hwnd, WM_LBUTTONUP, 0, lp);
        assert_eq!(GetWindowTextLengthW(editor), 0, "clear button must clear the native editor");
        ShowWindow(hwnd, SW_MINIMIZE);
        assert_eq!(IsWindowVisible(editor), 0);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE); SendMessageW(hwnd, WM_PAINT, 0, 0);
        assert_ne!(IsWindowVisible(editor), 0);
        let mut before = RECT::default(); GetWindowRect(editor, &raw mut before);
        let mut owner = RECT::default(); GetWindowRect(hwnd, &raw mut owner);
        SetWindowPos(hwnd, std::ptr::null_mut(), owner.left + 20, owner.top + 20, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        let mut after = RECT::default(); GetWindowRect(editor, &raw mut after);
        assert_eq!((after.left - before.left, after.top - before.top), (20, 20));
        click(nav.controls.iter().find(|c| matches!(c.action, Action::Page(0))).unwrap().bounds);
        assert_eq!(IsWindowVisible(editor), 0);
        SendMessageW(hwnd, WM_CLOSE, 0, 0);
    }
}

#[test]
fn font_search_matches_words_fullwidth_and_separators() {
    let names: Vec<String> = ["Microsoft YaHei UI", "Maple Mono NF-CN", "Noto Sans SC", "微软雅黑"].into_iter().map(String::from).collect();
    for query in ["yahei microsoft", "ＭＩＣＲＯＳＯＦＴ　ＵＩ", "microsoftyahei"] {
        assert_eq!(filter_fonts(&names, query), names[..1]);
    }
    for query in ["CN maple", "nf_cn", "MapleMono"] {
        assert_eq!(filter_fonts(&names, query), names[1..2]);
    }
    assert_eq!(filter_fonts(&names, "软 雅"), names[3..]);
    assert_eq!(filter_fonts(&names, "  UI  "), names[..1]);
    assert_eq!(filter_fonts(&names, "雅黑"), names[3..]);
    assert_eq!(filter_fonts(&names, ""), names);
    assert!(filter_fonts(&names, "missing-font").is_empty());
    assert_eq!(filter_fonts(&names, "  "), names);
    assert!(filter_fonts(&names, "maple yahei").is_empty());
}

#[test]
#[ignore = "Process-wide GDI counts; run alone to exclude other rendering tests"]
fn search_font_lifetime_survives_detached_callback_and_releases_on_close() {
    use windows_sys::Win32::Graphics::Gdi::*;
    unsafe {
        for detached_first in [false, true] {
            for _ in 0..32 {
                use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetGuiResources};
                let process = GetCurrentProcess();
                let before = windows_sys::Win32::System::Threading::GetGuiResources(process, 0);
                let owner = CreateWindowExW(0, windows_sys::w!("STATIC"), windows_sys::w!("Font lifetime"),
                    WS_POPUP, 0, 0, 1, 1, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
                assert!(!owner.is_null());
                windows_sys::Win32::UI::Shell::SetWindowSubclass(owner, Some(frame_proc), 1, 0);
                let editor = create_font_search(owner);
                assert!(!editor.is_null());
                let font = CreateFontW(-14, 0, 0, 0, FW_NORMAL as i32, 0, 0, 0,
                    DEFAULT_CHARSET as u32, 0, 0, ANTIALIASED_QUALITY as u32, 0, windows_sys::w!("Segoe UI"));
                assert!(!font.is_null());
                let resources: SearchFontOwner = Rc::new(RefCell::new(Some(SearchFont(font))));
                let weak = Rc::downgrade(&resources);
                assert!(attach_search_lifetime(editor, &resources));
                SetPropW(owner, windows_sys::w!("LucidDesk.FontSearch"), editor);
                SendMessageW(editor, WM_SETFONT, font as usize, 0);
                assert!(GetGuiResources(process, 0) > before, "probe must allocate GDI resources");
                assert_eq!(Rc::strong_count(&resources), 2);
                if detached_first {
                    drop(resources);
                    assert!(weak.upgrade().is_some(), "editor must keep its selected font alive");
                    DestroyWindow(owner);
                } else {
                    DestroyWindow(owner);
                    assert!(resources.borrow().is_none(), "native destruction releases the font even with a live callback");
                    drop(resources);
                }
                assert_eq!(IsWindow(editor), 0);
                assert!(weak.upgrade().is_none());
                GdiFlush();
                let after = windows_sys::Win32::System::Threading::GetGuiResources(process, 0);
                // GDI can retain a deleted font in its internal cache, so a stale
                // handle's GetObjectType is not a measure of process ownership.
                assert_eq!(after, before, "closing the editor must restore the GDI resource count");
            }
        }
    }
}
