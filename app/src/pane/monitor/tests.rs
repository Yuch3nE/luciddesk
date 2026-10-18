use super::sampling::{Device, Frame, Snapshot};
use super::*;
use std::time::{Duration, Instant};

#[test]
fn native_monitor_layouts_render_in_both_themes_at_multiple_dpi() {
    let _sta = crate::pane::test_support::apartment();
    let now = Instant::now();
    let history = (0..60)
        .map(|i| Frame {
            at: now - Duration::from_secs(59 - i),
            cpu: Some(24.0 + (i as f64 / 4.0).sin() * 15.0),
            memory: Some((17_824_000_000, 34_359_738_368)),
            networks: vec![Device {
                id: "Wi-Fi".into(),
                name: "Wi-Fi".into(),
                rate: Some((12_400_000.0, 820_000.0)),
                total: 0,
                used: None,
                usage: None,
            }],
            disks: vec![Device {
                id: "C:\\".into(),
                name: "C:\\".into(),
                rate: Some((
                    28_000_000.0 + (i as f64 / 4.0).sin() * 12_000_000.0,
                    8_000_000.0 + (i as f64 / 6.0).sin() * 4_000_000.0,
                )),
                total: 1_000_000_000_000,
                used: Some(400_000_000_000),
                usage: None,
            }],
            gpus: vec![Device {
                id: "gpu-0".into(),
                name: "GPU 0".into(),
                rate: None,
                total: 8_589_934_592,
                used: Some(2_254_857_830),
                usage: Some(18.0 + (i as f64 / 6.0).sin() * 10.0),
            }],
        })
        .collect::<std::collections::VecDeque<_>>();
    let _fixture = sampling::fixture(Snapshot {
        sequence: 1,
        unavailable: false,
        frame: history.back().unwrap().clone(),
        history,
    });
    let mut model = crate::pane::tests::test_model(crate::i18n::text("monitor-title"));
    model.native_material = false;
    let mut renderer = crate::pane::render::Renderer::new().unwrap();
    for compact in [false, true] {
        model.monitor = Some(Preferences {
            compact,
            ..Default::default()
        });
        for dark in [false, true] {
            model.dark = dark;
            model.backdrop = luciddesk_core::Backdrop::Translucent { opacity: 1.0 };
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let (w, h) = if compact {
                    (320.0, 280.0)
                } else {
                    (360.0, 600.0)
                };
                let (width, height) = ((w * scale) as u32, (h * scale) as u32);
                let pixels = renderer.pixels(width, height, scale, &model).unwrap();
                let repeated = renderer.pixels(width, height, scale, &model).unwrap();
                assert_eq!(
                    pixels, repeated,
                    "Repeated frames must not mutate shared text alignment"
                );
                assert_eq!(pixels.len(), (width * height * 4) as usize);
                // Accent marks/graphs must survive both light and dark contrast paths.
                assert!(
                    pixels
                        .chunks_exact(4)
                        .filter(|p| p[0].abs_diff(p[2]) > 50 && p[3] > 200)
                        .count()
                        > 50
                );
                if scale == 1.0 && std::env::var_os("LUCIDDESK_MONITOR_PREVIEWS").is_some() {
                    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../target/monitor-previews");
                    std::fs::create_dir_all(&directory).unwrap();
                    let name = format!(
                        "{}-{}-{width}x{height}.bgra",
                        if compact { "compact" } else { "standard" },
                        if dark { "dark" } else { "light" }
                    );
                    std::fs::write(directory.join(name), pixels).unwrap();
                }
            }
        }
    }
}

#[test]
fn folding_monitor_content_does_not_paint_over_the_header() {
    let _sta = crate::pane::test_support::apartment();
    let device = crate::pane::native_graphics::gpu_device().unwrap();
    let bitmap = crate::pane::canvas::Offscreen::new(&device, 360, 80).unwrap();
    let mut drawing = Drawing::new().unwrap();
    crate::pane::canvas::draw(&bitmap.target, 1.0, |target| {
        target.clear(windows_canvas::ColorF::new(1.0, 1.0, 1.0, 1.0));
        let text = crate::pane::native_graphics::canvas_result(
            target.create_solid_brush(windows_canvas::ColorF::new(0.0, 0.0, 0.0, 1.0)),
        )?;
        drawing.paint(
            &target,
            &Preferences::default(),
            windows_canvas::Rect::from_xywh(0.0, 50.0, 360.0, 2.0),
            &text,
            &text,
            false,
            0.0,
        )?;
        target.finish()
    })
    .unwrap();
    let pixels = bitmap.pixels().unwrap();
    assert!(
        pixels[..360 * 50 * 4]
            .chunks_exact(4)
            .all(|p| p == [255, 255, 255, 255])
    );
}

#[test]
fn monitor_window_reuses_native_panel_lifecycle_without_registering_shell_drop() {
    let _sta = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
    super::super::handle(&state, PanelId::new(0), Event::NewMonitor).unwrap();
    let (id, hwnd) = {
        let s = state.borrow();
        let view = s.views.last().unwrap();
        assert!(s.workspace.panel(view.id).unwrap().is_monitor());
        assert!(crate::pane::tabs::independent(&s.workspace, view.id));
        assert!(view.model.borrow().monitor.is_some());
        assert!(s.drops.is_empty());
        (view.id, view.window.hwnd().cast())
    };
    let prefs = Preferences {
        compact: true,
        paused: true,
        ..Default::default()
    };
    super::super::handle(&state, id, Event::MonitorOptions(prefs)).unwrap();
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let mut posted = MSG::default();
    while unsafe {
        PeekMessageW(
            &raw mut posted,
            hwnd,
            crate::pane::window::RUN_POSTED_ACTION,
            crate::pane::window::RUN_POSTED_ACTION,
            PM_REMOVE,
        )
    } != 0
    {
        unsafe {
            DispatchMessageW(&posted);
        }
    }
    let compact_bounds = state.borrow().workspace.panel(id).unwrap().rect();
    assert!((compact_bounds.width - 320.0).abs() < 1.0);
    assert!((compact_bounds.height - 280.0).abs() < 1.0);
    assert!(
        Preferences::load(&state.borrow().store, id)
            .unwrap()
            .compact
    );
    let mut minimum = MINMAXINFO::default();
    unsafe {
        SendMessageW(hwnd, WM_GETMINMAXINFO, 0, (&raw mut minimum) as isize);
    }
    assert!(minimum.ptMinTrackSize.x >= 300);
    assert!(minimum.ptMinTrackSize.y >= 240);
    super::super::handle(&state, id, Event::ClosePane).unwrap();
    assert!(state.borrow().workspace.panel(id).is_none());
    assert!(state.borrow().views.is_empty());
    assert!(
        state
            .borrow()
            .store
            .preference(&format!("monitor:{}", id.get()))
            .unwrap()
            .is_none()
    );
}
