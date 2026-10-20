use super::*;
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::{ProcessStatus::*, Threading::*},
};

#[test]
fn visibility_only_update_does_not_apply_settings_or_start_a_monitor() {
    let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
    let id = PanelId::new(999);
    let mut panel = Panel::new(id, "Monitor", RectDip::from_bounds(0.0, 0.0, 100.0, 100.0));
    panel.set_monitor(true);
    state.borrow_mut().workspace.add_panel(panel).unwrap();
    for (index, hidden) in [true, false, true, false].into_iter().enumerate() {
        // Emulate a local move/resize after the last parent configuration.
        let rect = RectDip::from_bounds(50.0 + index as f32, 60.0, 120.0, 130.0);
        state.borrow_mut().workspace.panel_mut(id).unwrap().set_rect(rect);
        let mut wire = Vec::new();
        write_json(&mut wire, &Update::DesktopVisibility(hidden)).unwrap();
        let update = serde_json::from_slice(&wire).unwrap();
        apply_update(&state, update).unwrap();
        assert_eq!(super::super::super::desktop_toggle::hidden(), hidden);
        assert_eq!(state.borrow().workspace.panel(id).unwrap().rect(), rect,
            "visibility must preserve live geometry independently of parent config");
        assert!(state.borrow().views.is_empty(), "visibility must not create UI");
        assert!(state.borrow().store.preference("language").unwrap().is_none(),
            "visibility must not rewrite settings");
    }
    desktop_visibility(&state, true).unwrap();
    assert!(state.borrow().monitors.process.is_none(), "visibility must not spawn a process");
}

#[test]
fn monitor_controller_stays_hidden_without_activation_or_a_taskbar_window() {
    let _sta = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
    let (_sender, commands) = mpsc::channel();
    let controller = create_controller(&state, commands).unwrap();
    let hwnd = controller.hwnd().cast();
    let style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
    assert_ne!(style & WS_EX_TOOLWINDOW, 0);
    assert_ne!(style & WS_EX_NOACTIVATE, 0);
    assert_eq!(style & WS_EX_APPWINDOW, 0);
    assert_eq!(unsafe { IsWindowVisible(hwnd) }, 0);
    let foreground = unsafe { GetForegroundWindow() };
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        SetWindowPos(hwnd, HWND_TOP, 0, 0, 1, 1, SWP_SHOWWINDOW);
    }
    assert_eq!(unsafe { IsWindowVisible(hwnd) }, 0);
    assert_eq!(unsafe { GetForegroundWindow() }, foreground);
    // Coalesced IPC wake-ups still reach an invisible controller.
    let wake = state.borrow().wake.clone();
    wake.bind(hwnd as isize);
    let mut message = MSG::default();
    assert_ne!(
        unsafe { PeekMessageW(&raw mut message, hwnd, wake::READY, wake::READY, PM_REMOVE) },
        0
    );
    unsafe {
        DispatchMessageW(&message);
    }
    wake.notify();
    assert_ne!(
        unsafe { PeekMessageW(&raw mut message, hwnd, wake::READY, wake::READY, PM_REMOVE) },
        0
    );
    wake.unbind();
}

fn assert_child_windows_stay_off_taskbar(pid: u32, panel: windows_sys::Win32::Foundation::HWND) {
    unsafe extern "system" fn visit(
        hwnd: windows_sys::Win32::Foundation::HWND,
        data: isize,
    ) -> i32 {
        unsafe {
            let (pid, windows) = &mut *(data as *mut (u32, Vec<(String, bool, u32)>));
            let mut owner_pid = 0;
            GetWindowThreadProcessId(hwnd, &raw mut owner_pid);
            if owner_pid == *pid {
                let mut title = [0u16; 128];
                let len =
                    GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32).max(0) as usize;
                windows.push((
                    String::from_utf16_lossy(&title[..len]),
                    IsWindowVisible(hwnd) != 0,
                    GetWindowLongW(hwnd, GWL_EXSTYLE) as u32,
                ));
            }
        }
        1
    }
    let mut inventory = (pid, Vec::<(String, bool, u32)>::new());
    unsafe {
        EnumWindows(Some(visit), (&raw mut inventory) as isize);
    }
    let (_, visible, style) = inventory
        .1
        .iter()
        .find(|(title, _, _)| title == "LucidDesk monitor controller")
        .expect("monitor IPC controller missing");
    assert!(!visible, "background controller became visible");
    assert_ne!(style & WS_EX_TOOLWINDOW, 0);
    assert_eq!(style & WS_EX_APPWINDOW, 0);
    let panel_style = unsafe { GetWindowLongW(panel, GWL_EXSTYLE) } as u32;
    assert_ne!(panel_style & WS_EX_TOOLWINDOW, 0);
    assert_eq!(panel_style & WS_EX_APPWINDOW, 0);
}

fn pump(state: &Rc<RefCell<PaneApp>>, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        poll(state).unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn measure(cycle: usize, phase: &str, process: windows_sys::Win32::Foundation::HANDLE) {
    unsafe {
        let mut memory = PROCESS_MEMORY_COUNTERS_EX::default();
        memory.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        assert_ne!(
            GetProcessMemoryInfo(process, (&raw mut memory).cast(), memory.cb),
            0
        );
        let mut handles = 0;
        assert_ne!(GetProcessHandleCount(process, &mut handles), 0);
        println!(
            "cycle={cycle} phase={phase} private_kib={} working_kib={} handles={} gdi={} user={}",
            memory.PrivateUsage / 1024,
            memory.WorkingSetSize / 1024,
            handles,
            GetGuiResources(process, GR_GDIOBJECTS),
            GetGuiResources(process, GR_USEROBJECTS)
        );
    }
}
#[test]
#[ignore = "Real monitor child, interactive desktop, built luciddesk.exe and diagnostic plugin required"]
fn isolated_monitor_closes_process_and_keeps_host_sampling_empty() {
    let _sta = crate::pane::test_support::apartment();
    assert!(super::super::available());
    let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
    state.borrow_mut().monitor_child = false;
    let directory = tempfile::tempdir().unwrap();
    {
        let mut s = state.borrow_mut();
        s.store = WorkspaceStore::open(&directory.path().join("workspace.db")).unwrap();
        let workspace = s.workspace.clone();
        s.store.save_workspace(&workspace).unwrap();
        crate::plugins::initialize(&s.store).unwrap();
    }
    let database_before = std::fs::read(directory.path().join("workspace.db")).unwrap();
    let workspace_before = state.borrow().workspace.clone();
    let stored_before = state.borrow().store.load_workspace().unwrap();
    // Warm ordinary host rendering and plugin settings before the first monitor ever opens.
    create_view(&state, PanelId::new(1)).unwrap();
    crate::plugins::request(
        super::super::PLUGIN_ID,
        serde_json::json!({"op":"settings"}),
    )
    .unwrap();
    pump(&state, 500);
    let host = unsafe { GetCurrentProcess() };
    measure(0, "before", host);
    let cycles: usize = std::env::var("LUCIDDESK_MEMORY_CYCLES")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(10);
    let first: u64 = std::env::var("LUCIDDESK_MEMORY_FIRST_MS")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(63000);
    handle(&state, PanelId::new(0), Event::EnableMonitor).unwrap();
    let id = PanelId::new(0);
    for cycle in 1..=cycles {
        if cycle != 1 {
            handle(&state, id, Event::MonitorOptions(Preferences::default())).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        while state.borrow().monitors.process.as_ref().unwrap().hwnd == 0 {
            assert!(Instant::now() < deadline, "Child startup timed out");
            pump(&state, 20);
        }
        let (pid, hwnd) = {
            let s = state.borrow();
            let p = &s.monitors.process.as_ref().unwrap();
            (p.child.as_ref().unwrap().id(), p.hwnd)
        };
        let child = unsafe {
            OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        assert!(!child.is_null());
        assert_child_windows_stay_off_taskbar(pid, hwnd as _);
        pump(&state, if cycle == 1 { first } else { 2200 });
        assert!(state.borrow().views.iter().all(|v| v.id != id));
        let diagnostics = crate::plugins::request(
            super::super::PLUGIN_ID,
            serde_json::json!({"op":"diagnostics"}),
        )
        .unwrap();
        assert_eq!(diagnostics["clients"], 0);
        assert_eq!(diagnostics["drawings"], 0);
        assert_eq!(diagnostics["service"], false);
        measure(cycle, "open-host", host);
        measure(cycle, "open-child", child);
        if cycle == 1 {
            handle(
                &state,
                id,
                Event::MonitorOptions(Preferences {
                    compact: true,
                    ..Default::default()
                }),
            )
            .unwrap();
            pump(&state, 500);
            assert!(
                (state
                    .borrow()
                    .monitors
                    .process
                    .as_ref()
                    .unwrap()
                    .config
                    .panel
                    .rect()
                    .width
                    - 320.0)
                    .abs()
                    < 1.0,
                "Compact layout was not returned to host"
            );
        }
        if cycle % 2 == 1 {
            unsafe {
                PostMessageW(hwnd as _, WM_CLOSE, 0, 0);
            }
        } else {
            handle(
                &state,
                id,
                Event::MonitorOptions(Preferences {
                    enabled: false,
                    ..Default::default()
                }),
            )
            .unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        while state.borrow().monitors.process.is_some() {
            assert!(Instant::now() < deadline, "Child close timed out");
            pump(&state, 20);
        }
        assert_eq!(unsafe { WaitForSingleObject(child, 5000) }, 0);
        let mut code = 0;
        unsafe {
            GetExitCodeProcess(child, &mut code);
            CloseHandle(child);
        }
        assert_eq!(code, 0, "Child must exit normally");
        assert_eq!(unsafe { IsWindow(hwnd as _) }, 0);
        assert!(!Preferences::load().unwrap().enabled);
        pump(&state, 300);
        measure(cycle, "closed", host);
    }
    // The settings plugin switch stops the real child without changing panel preferences.
    handle(&state, id, Event::MonitorOptions(Preferences::default())).unwrap();
    pump(&state, 1500);
    let pid = state.borrow().monitors.process.as_ref().unwrap().child.as_ref().unwrap().id();
    let child = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid) };

    crate::pane::settings::plugin_page::set_enabled(&state, super::super::PLUGIN_ID, false)
        .unwrap();
    assert_eq!(unsafe { WaitForSingleObject(child, 5000) }, 0);
    let mut code = 1;
    unsafe {
        GetExitCodeProcess(child, &mut code);
        CloseHandle(child);
    }
    assert_eq!(code, 0);
    assert!(!super::super::available());
    assert!(state.borrow().monitors.process.is_none());
    assert_eq!(state.borrow().workspace, workspace_before);
    let saved_config =
        std::fs::read(directory.path().join("plugins/system-monitor/config.toml")).unwrap();
    configure(&state).unwrap();
    assert!(state.borrow().monitors.process.is_none());
    crate::pane::settings::plugin_page::set_enabled(&state, super::super::PLUGIN_ID, true).unwrap();
    pump(&state, 1500);
    assert_ne!(state.borrow().monitors.process.as_ref().unwrap().hwnd, 0);
    assert!(Preferences::load().unwrap().enabled);
    assert_eq!(
        std::fs::read(directory.path().join("plugins/system-monitor/config.toml")).unwrap(),
        saved_config
    );
    assert_ne!(
        state.borrow().monitors.process.as_ref().unwrap().child.as_ref().unwrap().id(),
        pid
    );
    state.borrow_mut().monitors = Manager::default();
    // Host shutdown (dropping its pipe) must also exit a still-visible child normally.
    handle(&state, id, Event::MonitorOptions(Preferences::default())).unwrap();
    pump(&state, 1500);
    let pid = state.borrow().monitors.process.as_ref().unwrap().child.as_ref().unwrap().id();
    let child = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid) };
    state.borrow_mut().monitors = Manager::default();
    assert_eq!(unsafe { WaitForSingleObject(child, 5000) }, 0);
    let mut code = 0;
    unsafe {
        GetExitCodeProcess(child, &mut code);
        CloseHandle(child);
    }
    assert_eq!(code, 0);
    // A broken child protocol disables only this panel, retaining the saved configuration.
    handle(&state, id, Event::MonitorOptions(Preferences::default())).unwrap();
    pump(&state, 1500);
    {
        let mut s = state.borrow_mut();
        let process = s.monitors.process.as_mut().unwrap();
        writeln!(process.input.as_mut().unwrap(), "invalid configuration").unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while state.borrow().monitors.process.is_some() {
        assert!(Instant::now() < deadline);
        pump(&state, 20);
    }
    assert_eq!(state.borrow().workspace, workspace_before);
    assert!(!Preferences::load().unwrap().enabled);
    // Re-enabling after the failure starts a fresh, usable process.
    handle(&state, id, Event::MonitorOptions(Preferences::default())).unwrap();
    pump(&state, 1500);
    assert_ne!(state.borrow().monitors.process.as_ref().unwrap().hwnd, 0);
    state.borrow_mut().monitors = Manager::default();
    assert_eq!(state.borrow().workspace, workspace_before);
    assert_eq!(
        state.borrow().store.load_workspace().unwrap(),
        stored_before
    );
    assert_eq!(
        std::fs::read(directory.path().join("workspace.db")).unwrap(),
        database_before
    );
    assert!(
        directory
            .path()
            .join("plugins/system-monitor/config.toml")
            .exists()
    );
}

#[test]
#[ignore = "Owned subprocess entry point for shutdown regression"]
fn shutdown_fixture_child() {
    match std::env::var("LUCIDDESK_SHUTDOWN_FIXTURE").as_deref() {
        Ok("hung") => std::thread::sleep(Duration::from_secs(60)),
        Ok("eof") => { let mut s = String::new(); let _ = std::io::stdin().read_line(&mut s); }
        _ => {}
    }
}

#[test]
fn renderer_cleanup_is_async_and_bounds_hung_children() {
    for mode in ["eof", "hung"] {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["pane::monitor::process::tests::shutdown_fixture_child", "--exact", "--ignored"])
            .env("LUCIDDESK_SHUTDOWN_FIXTURE", mode)
            .creation_flags(CREATE_NO_WINDOW).stdin(Stdio::piped())
            .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let job = own_renderer(&child).unwrap();
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, child.id()) };
        assert!(!handle.is_null());
        child.stdin.take();
        let started = Instant::now();
        let worker = reap(child, Some(job), None, Duration::from_millis(500)).unwrap();
        assert!(started.elapsed() < Duration::from_millis(250), "UI thread must not wait for child exit");
        assert_eq!(unsafe { WaitForSingleObject(handle, 3000) }, 0, "owned renderer must not survive timeout");
        let mut code = 0;
        unsafe { assert_ne!(GetExitCodeProcess(handle, &raw mut code), 0); CloseHandle(handle); }
        if mode == "eof" { assert_eq!(code, 0, "normal EOF must remain graceful"); }
        else { assert_ne!(code, 0, "hung renderer must be stopped"); }
        worker.join().unwrap();
    }
}
