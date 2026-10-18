use super::*;

#[test]
fn plugin_content_draws_through_host_context_and_preserves_the_header() {
    let _sta = crate::pane::test_support::apartment();
    if let Ok(expected) = std::env::var("LUCIDDESK_TEST_MONITOR_PLUGIN") {
        assert_eq!(available(), expected == "1");
    }
    if !available() {
        return;
    }
    let mut client = Client::new().unwrap();
    // Populate real history so the full layout exercises chart geometry, not only placeholders.
    for _ in 0..10 {
        client.tick(true, &Preferences::default());
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let device = windows_canvas::GpuDevice::new_warp().unwrap();
    let mut drawing = Drawing::new().unwrap();
    for compact in [false, true] {
        for dark in [false, true] {
            for scale in [1.0, 1.5, 2.0] {
                let options = Preferences {
                    compact,
                    ..Default::default()
                };
                client.tick(true, &options);
                let bitmap = crate::pane::canvas::Offscreen::new(
                    &device,
                    (360.0 * scale) as u32,
                    (600.0 * scale) as u32,
                )
                .unwrap();
                crate::pane::canvas::draw(&bitmap.target, scale, |target| {
                    target.clear(windows_canvas::ColorF::new(1.0, 1.0, 1.0, 1.0));
                    drawing.paint(
                        &target,
                        &options,
                        windows_canvas::Rect::from_xywh(0.0, 50.0, 360.0, 550.0),
                        dark,
                        if dark { 1.0 } else { 0.0 },
                    )?;
                    target.finish()
                })
                .unwrap();
                let pixels = bitmap.pixels().unwrap();
                let header = (360.0 * scale) as usize * (50.0 * scale) as usize * 4;
                assert!(
                    pixels[..header]
                        .chunks_exact(4)
                        .all(|p| p == [255, 255, 255, 255])
                );
                assert!(
                    pixels[header..]
                        .chunks_exact(4)
                        .any(|p| p != [255, 255, 255, 255])
                );
            }
        }
    }
    drop(drawing);
    drop(client);
    crate::plugins::shutdown();
}

#[test]
fn monitor_window_reuses_native_panel_lifecycle_without_registering_shell_drop() {
    let _sta = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
    if !available() {
        assert!(super::super::handle(&state, PanelId::new(0), Event::NewMonitor).is_err());
        let id = state.borrow().workspace.panels()[0].id();
        state
            .borrow_mut()
            .workspace
            .panel_mut(id)
            .unwrap()
            .set_monitor(true);
        let saved = state.borrow().workspace.clone();
        state.borrow_mut().store.save_workspace(&saved).unwrap();
        super::super::create_view(&state, id).unwrap();
        assert!(state.borrow().views.is_empty());
        assert!(state.borrow().workspace.panel(id).unwrap().is_monitor());
        assert_eq!(state.borrow().store.load_workspace().unwrap(), saved);
        return;
    }
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

#[test]
#[ignore = "Interactive native windows; requires monitor DLL built with test-diagnostics"]
fn single_monitor_releases_resources_after_repeated_close() {
    use std::time::{Duration, Instant};
    use windows_sys::Win32::{
        System::{ProcessStatus::*, Threading::*},
        UI::WindowsAndMessaging::*,
    };
    let _sta = crate::pane::test_support::apartment();
    assert!(available());
    let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
    let diagnostics =
        || crate::plugins::request(PLUGIN_ID, serde_json::json!({"op":"diagnostics"})).unwrap();
    let pump = |duration| {
        let end = Instant::now() + Duration::from_millis(duration);
        while Instant::now() < end {
            let mut message = MSG::default();
            while unsafe { PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) }
                != 0
            {
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    let measure = |cycle, phase| unsafe {
        let process = GetCurrentProcess();
        let mut memory = PROCESS_MEMORY_COUNTERS_EX::default();
        memory.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        assert_ne!(
            GetProcessMemoryInfo(process, (&raw mut memory).cast(), memory.cb),
            0
        );
        if std::env::var_os("LUCIDDESK_MEMORY_HEAPS").is_some() {
            use windows_sys::Win32::System::Memory::*;
            let mut heaps =
                vec![std::ptr::null_mut(); GetProcessHeaps(0, std::ptr::null_mut()) as usize];
            let count = GetProcessHeaps(heaps.len() as u32, heaps.as_mut_ptr());
            heaps.truncate(count as usize);
            let mut allocated = 0;
            let mut committed = 0;
            for heap in heaps {
                let mut summary = HEAP_SUMMARY {
                    cb: size_of::<HEAP_SUMMARY>() as u32,
                    ..Default::default()
                };
                assert_ne!(HeapSummary(heap, 0, &raw mut summary), 0);
                allocated += summary.cbAllocated;
                committed += summary.cbCommitted;
            }
            println!(
                "heaps cycle={cycle} phase={phase} allocated_kib={} committed_kib={}",
                allocated / 1024,
                committed / 1024
            );
            let mut modules = vec![std::ptr::null_mut(); 1024];
            let mut needed = 0;
            if EnumProcessModulesEx(
                process,
                modules.as_mut_ptr(),
                (modules.len() * size_of::<usize>()) as u32,
                &raw mut needed,
                LIST_MODULES_ALL,
            ) != 0
            {
                modules.truncate(needed as usize / size_of::<usize>());
                let names: Vec<_> = modules
                    .into_iter()
                    .map(|module| {
                        let mut name = [0u16; 512];
                        let len = GetModuleBaseNameW(
                            process,
                            module,
                            name.as_mut_ptr(),
                            name.len() as u32,
                        );
                        String::from_utf16_lossy(&name[..len as usize])
                    })
                    .collect();
                println!("modules cycle={cycle} phase={phase} {names:?}");
            }
        }
        let mut handles = 0;
        assert_ne!(GetProcessHandleCount(process, &raw mut handles), 0);
        println!(
            "cycle={cycle} phase={phase} private_kib={} working_kib={} handles={} gdi={} user={} plugin={}",
            memory.PrivateUsage / 1024,
            memory.WorkingSetSize / 1024,
            handles,
            GetGuiResources(process, GR_GDIOBJECTS),
            GetGuiResources(process, GR_USEROBJECTS),
            diagnostics()
        );
    };
    // Warm the same host window/graphics path without starting monitoring.
    super::super::handle(&state, PanelId::new(0), Event::New).unwrap();
    let ordinary = state.borrow().views[0].id;
    pump(500);
    super::super::handle(&state, ordinary, Event::ClosePane).unwrap();
    pump(300);
    measure(0, "host-warm");
    let trace_wait = std::env::var("LUCIDDESK_MEMORY_TRACE_WAIT_MS")
        .map(|value| value.parse::<u64>().unwrap()).unwrap_or(0);
    pump(trace_wait);
    let metrics: Vec<Metric> = std::env::var("LUCIDDESK_MEMORY_METRICS")
        .map(|s| serde_json::from_str(&s).unwrap())
        .unwrap_or_else(|_| Preferences::default().metrics);
    let cycles: u32 = std::env::var("LUCIDDESK_MEMORY_CYCLES")
        .map(|s| s.parse().unwrap())
        .unwrap_or(10);
    let first_ms: u64 = std::env::var("LUCIDDESK_MEMORY_FIRST_MS")
        .map(|s| s.parse().unwrap())
        .unwrap_or(63000);
    for cycle in 1..=cycles {
        super::super::handle(&state, PanelId::new(0), Event::NewMonitor).unwrap();
        let (id, hwnd, model) = {
            let s = state.borrow();
            assert_eq!(s.views.len(), 1);
            let view = &s.views[0];
            (
                view.id,
                view.window.hwnd().cast(),
                Rc::downgrade(&view.model),
            )
        };
        {
            let s = state.borrow();
            s.views[0]
                .model
                .borrow_mut()
                .monitor
                .as_mut()
                .unwrap()
                .metrics = metrics.clone();
        }
        pump(if cycle == 1 { first_ms } else { 2200 });
        let active = diagnostics();
        assert_eq!(active["clients"], 1);
        assert_eq!(active["drawings"], 1);
        assert_eq!(active["service"], true);
        assert!(metrics.is_empty() || active["frames"].as_u64().unwrap() > 0);
        measure(cycle, "open");
        if cycle % 2 == 0 {
            super::super::handle(
                &state,
                id,
                Event::MonitorOptions(Preferences {
                    enabled: false,
                    ..Default::default()
                }),
            )
            .unwrap();
        } else {
            assert_ne!(unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) }, 0);
        }
        pump(400);
        measure(cycle, "closed");
        pump(trace_wait);
        assert_eq!(unsafe { IsWindow(hwnd) }, 0);
        assert!(model.upgrade().is_none(), "closed model is still retained");
        assert_eq!(
            diagnostics(),
            serde_json::json!({"clients":0,"drawings":0,"service":false,"frames":0})
        );
        // Disabling preserves configuration; remove it only after checking teardown.
        super::super::handle(&state, id, Event::ClosePane).unwrap();
    }
}
