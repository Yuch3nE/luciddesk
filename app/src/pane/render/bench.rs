//! Interactive, repeatable measurements; no timing thresholds in CI.
use super::Renderer;
use crate::pane::{composition::Surface, menu::Entry};
use std::time::{Duration, Instant};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[test]
#[ignore = "Manual hot-path benchmark; run alone on an interactive desktop"]
fn paint_hot_path_latency() {
    let _sta = crate::pane::test_support::apartment();
    let _graphics = crate::pane::native_graphics::GraphicsLifetime;
    let device = crate::pane::native_graphics::gpu_device().unwrap();
    let offscreen = crate::pane::canvas::Offscreen::new(&device, 1000, 680).unwrap();
    let mut renderer = Renderer::new().unwrap();
    let mut model = crate::pane::tests::test_model("Paint benchmark");
    model.folder = Some(std::path::PathBuf::from(r"C:\Benchmark"));
    model.items = (0..1200).map(|i| crate::pane::Item {
        identity: luciddesk_core::ShellIdentity::Namespace { parsing_name: format!("hot:{i}") },
        label: format!("Document {i} · 项目资料"),
        details: crate::pane::ItemDetails { size: Some(131072 + i * 1048576), ..Default::default() },
        image: Some(std::sync::Arc::new(crate::pane::assets::Pixels {
            width: 128, height: 128, data: [60, 100, i as u8, 255].repeat(128 * 128),
        })),
    }).collect();
    for (scenario, count) in [("cold_scroll", 24), ("grid_hover", 240), ("details_hover", 240)] {
        model.list_view = scenario == "details_hover";
        model.scroll = 0;
        renderer.release_device_resources();
        if scenario != "cold_scroll" {
            for _ in 0..32 {
                renderer.paint_animated(&offscreen.target, 1000, 680, 1.25, &model, false).unwrap();
            }
        }
        let mut times = Vec::with_capacity(count);
        let mut uploads = 0;
        for frame in 0..count {
            if scenario == "cold_scroll" { model.scroll = 6 * frame; }
            model.hovered_item = Some(frame % 12);
            let before = renderer.image_uploads;
            let start = Instant::now();
            renderer.paint_animated(&offscreen.target, 1000, 680, 1.25, &model, false).unwrap();
            times.push(start.elapsed());
            uploads = uploads.max(renderer.image_uploads - before);
        }
        println!("{scenario} max_uploads_per_frame={uploads}");
        report(scenario, &mut times);
    }
}

fn report(name: &str, samples: &mut [Duration]) {
    samples.sort_unstable();
    println!("{name} n={} p50_us={} p95_us={}", samples.len(),
        samples[samples.len() / 2].as_micros(), samples[samples.len() * 95 / 100].as_micros());
}

#[test]
#[ignore = "Shows a GPU benchmark window briefly; run alone on an interactive desktop"]
fn animation_timer_cadence() {
    use std::{cell::RefCell, rc::Rc};
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, EndPaint, GetDC, GetDeviceCaps, InvalidateRect, PAINTSTRUCT, ReleaseDC, VREFRESH,
    };

    #[derive(Default)]
    struct Samples {
        ticks: usize,
        paint: usize,
        accepted_at: Vec<Instant>,
        busy: usize,
        deferred: usize,
        draw: Duration,
    }

    let _sta = crate::pane::test_support::apartment();
    let _graphics = crate::pane::native_graphics::GraphicsLifetime;
    let width = 640;
    let height = 420;
    let mut renderer = Renderer::new().unwrap();
    let mut model = crate::pane::tests::test_model("Animation timer benchmark");
    let image = std::sync::Arc::new(crate::pane::assets::Pixels {
        width: 48, height: 48, data: [60, 100, 180, 255].repeat(48 * 48),
    });
    model.items = (0..48).map(|i| crate::pane::Item {
        details: Default::default(),
        identity: luciddesk_core::ShellIdentity::Namespace { parsing_name: format!("motion:{i}") },
        label: format!("Document {i}"),
        image: Some(image.clone()),
    }).collect();
    let stats = Rc::new(RefCell::new(Samples::default()));
    let recording = Rc::clone(&stats);
    let motion_clock = Instant::now();
    let mut surface = None;
    let window = windows_window::Window::new("LucidDesk animation benchmark")
        .size(width as i32, height as i32)
        .style(WS_POPUP)
        .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP)
        .on_message(move |raw, message, timer, _| {
            let hwnd = raw.cast();
            match message {
                WM_ERASEBKGND => Some(1),
                WM_TIMER if timer == 1 => {
                    let mut stats = recording.borrow_mut();
                    stats.ticks += 1;
                    model.hovered_item = Some((motion_clock.elapsed().as_millis() / 75 % 24) as usize);
                    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                    Some(0)
                }
                WM_PAINT => {
                    unsafe {
                        let mut paint = PAINTSTRUCT::default();
                        BeginPaint(hwnd, &raw mut paint);
                        EndPaint(hwnd, &raw const paint);
                    }
                    let start = Instant::now();
                    let surface = surface.get_or_insert_with(|| Surface::new(
                        windows::Win32::Foundation::HWND(hwnd),
                    ).unwrap());
                    let mut stats = recording.borrow_mut();
                    stats.paint += 1;
                    if let Some(target) = surface.try_begin_frame(width, height).unwrap() {
                        renderer.paint_animated(&target, width, height, 1.0, &model, true).unwrap();
                        if surface.end_frame().unwrap() {
                            stats.accepted_at.push(Instant::now());
                        } else {
                            stats.busy += 1;
                        }
                    } else {
                        stats.deferred += 1;
                    }
                    stats.draw += start.elapsed();
                    Some(0)
                }
                _ => None,
            }
        }).create().unwrap();
    let hwnd = window.hwnd().cast();
    let pump = |time| {
        let until = Instant::now() + time;
        while Instant::now() < until {
            unsafe {
                let mut message = MSG::default();
                while PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&raw const message);
                    DispatchMessageW(&raw const message);
                }
                let wait = until.saturating_duration_since(Instant::now()).as_millis() as u32;
                MsgWaitForMultipleObjectsEx(0, std::ptr::null(), wait, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            }
        }
    };
    unsafe {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let dc = GetDC(hwnd);
        println!("display_hz={}", GetDeviceCaps(dc, VREFRESH as i32));
        ReleaseDC(hwnd, dc);
        assert_ne!(IsWindowVisible(hwnd), 0);
    }
    pump(Duration::from_millis(280));
    for interval in [10, 16, 16, 10, 10, 16] {
        *stats.borrow_mut() = Samples::default();
        assert_ne!(unsafe { SetTimer(hwnd, 1, interval, None) }, 0);
        let start = Instant::now();
        pump(Duration::from_millis(1000));
        unsafe { KillTimer(hwnd, 1); }
        let samples = stats.borrow();
        let mut gaps: Vec<_> = samples.accepted_at.windows(2).map(|p| p[1] - p[0]).collect();
        assert!(!gaps.is_empty(), "no frames reached DXGI at {interval} ms");
        println!("timer_ms={interval} wall_ms={} ticks={} paints={} accepted={} busy={} deferred={} draw_us={}",
            start.elapsed().as_millis(), samples.ticks, samples.paint, samples.accepted_at.len(),
            samples.busy, samples.deferred, samples.draw.as_micros());
        report("present_gap", &mut gaps);
        drop(samples);
        pump(Duration::from_millis(60));
    }
}

#[test]
#[ignore = "Manual offscreen benchmark; timings are not CI assertions"]
fn rendering_hot_paths_latency() {
    let _sta = crate::pane::test_support::apartment();
    let device = windows_canvas::GpuDevice::new().unwrap();
    let surface = crate::pane::canvas::Offscreen::new(&device, 1200, 600).unwrap();
    let mut renderer = Renderer::new().unwrap();
    let mut model = crate::pane::tests::test_model("📁 Projects ✨ LucidDesk");
    model.items = (0..32).map(|i| crate::pane::Item {
        details: Default::default(),
        identity: luciddesk_core::ShellIdentity::Namespace { parsing_name: format!("bench:{i}") },
        label: format!("Project document {i} · 项目资料"), image: None,
    }).collect();
    model.list_view = true;
    model.selection.insert(1);
    for scenario in ["selection_resize", "emoji_list", "emoji_tabs_list"] {
        if scenario == "emoji_tabs_list" {
            model.tabs = ["📁 Projects", "✨ Documents", "Archive"].into_iter().enumerate()
                .map(|(i, title)| (luciddesk_core::PanelId::new(i as u64 + 1), title.into())).collect();
            model.active_tab = luciddesk_core::PanelId::new(1);
        }
        let mut times = Vec::with_capacity(360);
        for frame in 0..384 {
            let width = if scenario == "selection_resize" { 900 + frame % 221 } else { 1000 };
            let start = Instant::now();
            renderer.paint(&surface.target, width, 600, 1.25, &model).unwrap();
            if frame >= 24 { times.push(start.elapsed().as_micros()); }
        }
        times.sort_unstable();
        println!("{scenario} p50_us={} p95_us={}", times[180], times[342]);
    }
}

#[test]
#[ignore = "Manual GPU benchmark; timings are not CI assertions"]
fn warm_pane_draw_latency() {
    let _sta = crate::pane::test_support::apartment();
    let _graphics = crate::pane::native_graphics::GraphicsLifetime;
    let device = crate::pane::native_graphics::gpu_device().unwrap();
    let surface = crate::pane::canvas::Offscreen::new(&device, 640, 480).unwrap();
    let mut renderer = Renderer::new().unwrap();
    let mut model = crate::pane::tests::test_model("Desktop resources");
    let image = std::sync::Arc::new(crate::pane::assets::Pixels {
        width: 32, height: 32, data: [60, 100, 180, 255].repeat(32 * 32),
    });
    model.items = (0..48).map(|i| crate::pane::Item {
        details: Default::default(),
        identity: luciddesk_core::ShellIdentity::Namespace { parsing_name: format!("test:{i}") },
        label: format!("Document {i}"), image: Some(image.clone()),
    }).collect();
    for list in [false, true] {
        model.list_view = list;
        for _ in 0..20 {
            renderer.paint(&surface.target, 640, 480, 1.0, &model).unwrap();
        }
        let mut times = Vec::new();
        for frame in 0..300 {
            model.hovered_item = Some(frame % 12);
            let start = Instant::now();
            renderer.paint(&surface.target, 640, 480, 1.0, &model).unwrap();
            times.push(start.elapsed().as_micros());
        }
        times.sort_unstable();
        println!("list={list} draw_p50_us={} draw_p95_us={}", times[150], times[285]);
    }
}

#[test]
#[ignore = "Shows four GPU windows; run alone on an interactive desktop"]
fn multi_window_render_latency() {
    let _sta = crate::pane::test_support::apartment();
    let start = Instant::now();
    let mut windows = Vec::new();
    let mut surfaces = Vec::new();
    let renderer = Renderer::new().unwrap();
    let rows: Vec<_> = (1..=10)
        .map(|id| Entry {
            enabled: true,
            id,
            label: "文件夹面板与菜单".into(),
            icon: "",
            trailing: "Ctrl+L",
            children: Vec::new(),
        })
        .collect();
    for i in 0..4 {
        let window = windows_window::Window::new("LucidDesk GPU benchmark")
            .size(320, 340)
            .style(WS_POPUP)
            .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP)
            .on_message(|_, msg, _, _| matches!(msg, WM_DESTROY | WM_ERASEBKGND).then_some(0))
            .create()
            .unwrap();
        unsafe {
            SetWindowPos(
                window.hwnd().cast(),
                std::ptr::null_mut(),
                40 + i * 330,
                80,
                320,
                340,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
        let mut surface =
            Surface::new(windows::Win32::Foundation::HWND(window.hwnd().cast())).unwrap();
        surface.material(
            windows::Win32::Foundation::HWND(window.hwnd().cast()),
            luciddesk_core::Backdrop::Acrylic,
        ).unwrap();
        surfaces.push(surface);
        windows.push(window);
    }
    let cold = start.elapsed();
    let mut draw = Vec::new();
    let mut present = Vec::new();
    for round in 0..32 {
        for surface in &mut surfaces {
            let start = Instant::now();
            let target = surface.begin_frame(320, 340).unwrap();
            renderer
                .paint_flyout(
                    &target,
                    320,
                    340,
                    1.0,
                    &rows,
                    &(0..rows.len())
                        .map(|i| if i == round % 10 { 1.0 } else { 0.0 })
                        .collect::<Vec<_>>(),
                    surface.native,
                    true,
                )
                .unwrap();
            draw.push(start.elapsed().as_micros());
            let start = Instant::now();
            surface.end_frame().unwrap();
            present.push(start.elapsed().as_micros());
        }
    }
    draw.sort_unstable();
    present.sort_unstable();
    println!(
        "cold_four_surfaces_ms={} draw_p50_us={} draw_p95_us={} present_p50_us={} present_p95_us={} present_total_us={}",
        cold.as_millis(),
        draw[64],
        draw[121],
        present[64],
        present[121],
        present.iter().sum::<u128>()
    );
    drop(surfaces);
    drop(windows);
}
