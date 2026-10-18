use super::*;
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::{ProcessStatus::*, Threading::*},
};

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
            GetProcessMemoryInfo(
                process,
                (&raw mut memory).cast(),
                memory.cb
            ),
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
    // Warm ordinary host rendering and plugin settings before the first monitor ever opens.
    create_view(&state, PanelId::new(1)).unwrap();
    crate::plugins::request(
        super::super::PLUGIN_ID,
        serde_json::json!({"op":"settings","panels":[]}),
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
    handle(&state, PanelId::new(0), Event::NewMonitor).unwrap();
    let id = state
        .borrow()
        .workspace
        .panels()
        .iter()
        .find(|p| p.is_monitor())
        .unwrap()
        .id();
    for cycle in 1..=cycles {
        if cycle != 1 {
            handle(&state, id, Event::MonitorOptions(Preferences::default())).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        while state.borrow().monitors.processes[&id].hwnd == 0 {
            assert!(Instant::now() < deadline, "Child startup timed out");
            pump(&state, 20);
        }
        let (pid, hwnd) = {
            let s = state.borrow();
            let p = &s.monitors.processes[&id];
            (p.child.id(), p.hwnd)
        };
        let child = unsafe {
            OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        assert!(!child.is_null());
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
                (state.borrow().workspace.panel(id).unwrap().rect().width - 320.0).abs() < 1.0,
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
        while state.borrow().monitors.processes.contains_key(&id) {
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
        assert!(
            !Preferences::load(&state.borrow().store, id)
                .unwrap()
                .enabled
        );
        pump(&state, 300);
        measure(cycle, "closed", host);
    }
    // Host shutdown (dropping its pipe) must also exit a still-visible child normally.
    handle(&state, id, Event::MonitorOptions(Preferences::default())).unwrap();
    pump(&state, 1500);
    let pid = state.borrow().monitors.processes[&id].child.id();
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
        let process = s.monitors.processes.get_mut(&id).unwrap();
        writeln!(process.input.as_mut().unwrap(), "invalid configuration").unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while state.borrow().monitors.processes.contains_key(&id) {
        assert!(Instant::now() < deadline);
        pump(&state, 20);
    }
    assert!(state.borrow().workspace.panel(id).unwrap().is_monitor());
    assert!(
        !Preferences::load(&state.borrow().store, id)
            .unwrap()
            .enabled
    );
    // Re-enabling after the failure starts a fresh, usable process.
    handle(&state, id, Event::MonitorOptions(Preferences::default())).unwrap();
    pump(&state, 1500);
    assert_ne!(state.borrow().monitors.processes[&id].hwnd, 0);
    state.borrow_mut().monitors = Manager::default();
}
