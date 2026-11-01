//! Real Explorer fallback rendering check. Close LucidDesk before running.
//! Arguments: absolute diagnostic DLL path, dark|light. Own temporary file only.
use luciddesk_explorer::filter::FilterSession;
use std::{sync::{Arc, Mutex}, time::Duration};
use windows_sys::Win32::{Foundation::HWND, Graphics::Gdi::*, UI::WindowsAndMessaging::*};

unsafe extern "system" fn inspect(hwnd: HWND, data: isize) -> i32 {
    unsafe {
        if GetClassLongPtrW(hwnd, GCW_ATOM) == 32768 && IsWindowVisible(hwnd) != 0 {
            let mut rect = Default::default();
            GetClientRect(hwnd, &raw mut rect);
            let dc = GetDC(hwnd);
            if !dc.is_null() {
                let color = GetPixel(dc, (rect.right - 20).max(1), (rect.bottom / 2).max(1));
                ReleaseDC(hwnd, dc);
                if color != u32::MAX { *(data as *mut Option<u32>) = Some(color); }
            }
        }
    }
    1
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dll = std::env::args_os().nth(1).ok_or("expected diagnostic DLL path")?;
    let expected = match std::env::args().nth(2).as_deref() {
        Some("dark") => true, Some("light") => false, _ => return Err("expected dark|light".into()),
    };
    let _sta = luciddesk_shell::ShellApartment::initialize_sta()?;
    let owner = windows_window::Window::new("LucidDesk Explorer fallback theme probe")
        .size(480, 320).style(WS_OVERLAPPEDWINDOW)
        .on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0))
        .create().map_err(|e| e.to_string())?;
    let hwnd = owner.hwnd().cast();
    unsafe {
        SetPropW(hwnd, windows_sys::w!("LucidDesk.Test.ForceClassic"), 1usize as _);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
    // Background-launched processes may not take foreground permission. Wait for
    // an actual activation rather than misreporting a theme failure or bypassing
    // Windows' foreground lock.
    println!("Activate the probe window to begin; owner={:x}", hwnd as usize);
    let activation = std::time::Instant::now();
    unsafe {
        while GetForegroundWindow() != hwnd {
            if activation.elapsed() > Duration::from_secs(60) { return Err("probe window was not activated".into()); }
            let mut message = MSG::default();
            while PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            MsgWaitForMultipleObjectsEx(0, std::ptr::null(), 50, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
    let desktop = luciddesk_explorer::desktop_view()?;
    let hook = FilterSession::connect(desktop, hwnd as isize, std::path::Path::new(&dll))?;
    let path = std::env::temp_dir().join(format!("luciddesk-explorer-theme-{}.txt", std::process::id()));
    std::fs::write(&path, b"owned fallback menu test fixture")?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        for iteration in 0..2 {
            let host = hook.prepare_menu(hwnd as isize, &[path.to_string_lossy().into_owned()], 300, 300)?;
            let (pid, thread) = unsafe {
                let mut pid = 0;
                let thread = GetWindowThreadProcessId(host as _, &raw mut pid);
                let mut desktop_pid = 0;
                GetWindowThreadProcessId(desktop as _, &raw mut desktop_pid);
                assert_eq!(pid, desktop_pid, "menu must be owned by real Explorer");
                assert_ne!(pid, std::process::id());
                assert_eq!(GetPropW(host as _, windows_sys::w!("LucidDesk.Menu.Presenter")) as usize, 2,
                    "requires a menu-diagnostics DLL with the fallback injection enabled");
                (pid, thread)
            };
            let observed = Arc::new(Mutex::new(None::<u32>));
            let output = observed.clone();
            let sampler = std::thread::spawn(move || {
                let mut color = None::<u32>;
                for _ in 0..45 {
                    std::thread::sleep(Duration::from_millis(100));
                    unsafe { EnumThreadWindows(thread, Some(inspect), (&raw mut color) as isize); }
                    if color.is_some() {
                        // Allow the observer to see the shown menu and painting to settle.
                        std::thread::sleep(Duration::from_millis(350));
                        unsafe { EnumThreadWindows(thread, Some(inspect), (&raw mut color) as isize); }
                        break;
                    }
                }
                unsafe { PostMessageW(host as _, WM_CANCELMODE, 0, 0); }
                *output.lock().unwrap() = color;
            });
            let shown = luciddesk_shell::show_isolated_item_menu(
                windows::Win32::Foundation::HWND(hwnd), windows::Win32::Foundation::HWND(host as _),
                luciddesk_shell::MenuInvocation::Mouse);
            sampler.join().unwrap();
            if let Err(error) = shown {
                eprintln!("popup failed: iteration={iteration} explorer_pid={pid} sample={:?}", observed.lock().unwrap());
                hook.cancel_menu()?;
                return Err(error.into());
            }
            hook.finish_menu()?;
            let color = observed.lock().unwrap().ok_or("no visible Explorer popup sampled")?;
            let r = color & 255;
            let g = (color >> 8) & 255;
            let b = (color >> 16) & 255;
            println!("iteration={iteration} explorer_pid={pid} menu_thread={thread} host={host:x} presenter=classic background=#{r:02X}{g:02X}{b:02X}");
            if (r + g + b < 384) != expected { return Err("popup color does not match expected Windows theme".into()); }
        }
        // Removing the injection must restore private Presenter creation without restart.
        unsafe { RemovePropW(hwnd, windows_sys::w!("LucidDesk.Test.ForceClassic")); }
        let host = hook.prepare_menu(hwnd as isize, &[path.to_string_lossy().into_owned()], 300, 300)?;
        let presenter = unsafe { GetPropW(host as _, windows_sys::w!("LucidDesk.Menu.Presenter")) as usize };
        hook.finish_menu()?;
        println!("after_injection_removed_presenter={presenter}");
        if presenter != 1 { return Err("Win11 Presenter did not recover after forced fallback".into()); }
        Ok(())
    })();
    let _ = hook.cancel_menu();
    drop(hook);
    std::fs::remove_file(path)?;
    result
}
