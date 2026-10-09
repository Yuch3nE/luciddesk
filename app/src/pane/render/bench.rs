//! Interactive, repeatable measurements; no timing thresholds in CI.
use super::Renderer;
use crate::pane::{composition::Surface, menu::Entry};
use std::time::Instant;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

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
            id,
            label: "文件夹面板与菜单",
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
        );
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
