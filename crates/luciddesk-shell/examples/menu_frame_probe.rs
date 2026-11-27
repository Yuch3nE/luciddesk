//! Isolated native-frame test. No file verbs run unless explicitly selected.
use windows_sys::Win32::{Foundation::*, System::LibraryLoader::*, UI::{HiDpi::*, WindowsAndMessaging::*}};
use std::ptr::null_mut;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let module = LoadLibraryExW(windows_sys::w!("uxtheme.dll"), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        let enable: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(GetProcAddress(module, 135usize as _).unwrap());
        enable(if std::env::args().any(|arg| arg == "--light") { 3 } else { 1 });
    }
    let _sta = luciddesk_shell::ShellApartment::initialize_sta()?;
    let enabled = std::env::args().any(|arg| arg == "--frame");
    let verify = std::env::args().any(|arg| arg == "--verify");
    let path = std::env::args_os().skip(1).find(|arg| arg != "--frame" && arg != "--verify" && arg != "--light");
    let window = windows_window::Window::new("LucidDesk frame padding experiment - right click")
        .size(640, 400).style(WS_OVERLAPPEDWINDOW)
        .on_message(move |raw, msg, _, lp| unsafe {
            let hwnd = raw.cast();
            match msg {
                WM_ENTERIDLE if lp != 0 => {
                    let popup = lp as HWND;
                    let mut info = MENUBARINFO { cbSize: size_of::<MENUBARINFO>() as u32, ..Default::default() };
                    let mut rect = RECT::default();
                    if GetMenuBarInfo(popup, -4, 0, &raw mut info) != 0 {
                        let mut first = RECT::default(); let mut last = RECT::default();
                        GetWindowRect(popup, &raw mut rect);
                        let count = GetMenuItemCount(info.hMenu);
                        GetMenuItemRect(null_mut(), info.hMenu, 0, &raw mut first);
                        GetMenuItemRect(null_mut(), info.hMenu, (count-1) as u32, &raw mut last);
                        eprintln!("geometry count={count} width={} height={} top={} bottom={} first_height={} last_height={}", rect.right-rect.left,rect.bottom-rect.top,first.top-rect.top,rect.bottom-last.bottom,first.bottom-first.top,last.bottom-last.top);
                    }
                    None
                }
                WM_CONTEXTMENU => {
                    eprintln!("dark={:?} frame={enabled}", luciddesk_menu::apply_theme(hwnd));
                    let point = windows::Win32::Foundation::POINT { x: lp as u16 as i16 as i32, y: (lp >> 16) as u16 as i16 as i32 };
                    let _frame = (enabled && path.is_none()).then(|| luciddesk_menu::MenuFrame::install(hwnd, POINT { x: point.x, y: point.y })).flatten();
                    if let Some(path) = &path {
                        let item = luciddesk_shell::ShellIdentity::FileSystem { path: path.into(), volume_id: None, file_id: None };
                        eprintln!("result={:?}", luciddesk_shell::show_file_items_menu(windows::Win32::Foundation::HWND(hwnd), &[item], point, "Open file location"));
                    } else {
                        let menu = CreatePopupMenu();
                        AppendMenuW(menu, MF_STRING, 1, windows_sys::w!("Open (&O)"));
                        SetMenuDefaultItem(menu, 1, 0);
                        AppendMenuW(menu, MF_STRING, 2, windows_sys::w!("Open file location"));
                        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                        AppendMenuW(menu, MF_STRING, 3, windows_sys::w!("Properties (&R)"));
                        let chosen = TrackPopupMenuEx(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, point.x, point.y, hwnd, std::ptr::null());
                        eprintln!("chosen={chosen}");
                        DestroyMenu(menu);
                    }
                    Some(0)
                }
                WM_CLOSE => { DestroyWindow(hwnd); Some(0) }
                WM_DESTROY => { PostQuitMessage(0); Some(0) }
                _ => None,
            }
        }).create().map_err(|e| std::io::Error::other(e.to_string()))?;
    unsafe {
        ShowWindow(window.hwnd().cast(), SW_SHOW);
        if verify {
            verify_frame(window.hwnd().cast());
            DestroyWindow(window.hwnd().cast());
            return Ok(());
        }
        let mut message = MSG::default();
        while GetMessageW(&raw mut message, null_mut(), 0, 0) > 0 { TranslateMessage(&message); DispatchMessageW(&message); }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct Dimensions { width: i32, height: i32, top: i32, bottom: i32, row_height: i32, count: i32, screen_bottom: i32 }
thread_local! {
    static TEST_MENU: std::cell::Cell<HMENU> = const { std::cell::Cell::new(std::ptr::null_mut()) };
    static DIMENSIONS: std::cell::Cell<Option<Dimensions>> = const { std::cell::Cell::new(None) };
}
unsafe extern "system" fn measure_popup(hwnd: HWND, _: u32, timer: usize, _: u32) {
    unsafe {
        KillTimer(hwnd, timer);
        EnumThreadWindows(windows_sys::Win32::System::Threading::GetCurrentThreadId(), Some(measure_window), 0);
        EndMenu();
    }
}
unsafe extern "system" fn measure_window(hwnd: HWND, _: isize) -> i32 {
    unsafe {
        if GetClassLongPtrW(hwnd, GCW_ATOM) != 32768 || IsWindowVisible(hwnd) == 0 { return 1; }
        let menu = TEST_MENU.get();
        let mut outer = RECT::default(); let mut first = RECT::default(); let mut last = RECT::default();
        let count = GetMenuItemCount(menu);
        if GetWindowRect(hwnd, &raw mut outer) != 0
            && GetMenuItemRect(null_mut(), menu, 0, &raw mut first) != 0
            && GetMenuItemRect(null_mut(), menu, (count - 1) as u32, &raw mut last) != 0 {
            DIMENSIONS.set(Some(Dimensions { width: outer.right - outer.left, height: outer.bottom - outer.top,
                top: first.top - outer.top, bottom: outer.bottom - last.bottom, row_height: first.bottom - first.top, count, screen_bottom: outer.bottom }));
        }
        1
    }
}
unsafe fn sample_frame(hwnd: HWND, enabled: bool, x: i32, y: i32, extra_rows: u32) -> Dimensions {
    unsafe {
        luciddesk_menu::apply_theme(hwnd);
        let _frame = enabled.then(|| luciddesk_menu::MenuFrame::install(hwnd, POINT { x, y })).flatten();
        let menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING, 1, windows_sys::w!("Open (&O)"));
        SetMenuDefaultItem(menu, 1, 0);
        AppendMenuW(menu, MF_STRING, 2, windows_sys::w!("Open file location"));
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, 3, windows_sys::w!("Properties (&R)"));
        for row in 0..extra_rows { AppendMenuW(menu, MF_STRING, 4 + row as usize, windows_sys::w!("Additional native menu item")); }
        TEST_MENU.set(menu); DIMENSIONS.set(None);
        SetTimer(hwnd, 71, 250, Some(measure_popup));
        TrackPopupMenuEx(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, hwnd, std::ptr::null());
        KillTimer(hwnd, 71); DestroyMenu(menu);
        DIMENSIONS.take().expect("native menu geometry was captured")
    }
}
unsafe fn verify_frame(hwnd: HWND) {
    unsafe {
        let baseline = sample_frame(hwnd, false, 400, 400, 0);
        let padded = sample_frame(hwnd, true, 400, 400, 0);
        eprintln!("baseline={baseline:?}\npadded={padded:?}");
        let padding = ((3 * GetDpiForWindow(hwnd).max(96) + 48) / 96) as i32;
        assert_eq!(baseline.count, padded.count);
        assert_eq!(baseline.width, padded.width);
        assert_eq!(baseline.row_height, padded.row_height);
        assert_eq!(padded.top - baseline.top, padding);
        assert_eq!(padded.bottom - baseline.bottom, padding);
        assert_eq!(padded.height - baseline.height, 2 * padding);
        use windows_sys::Win32::Graphics::Gdi::*;
        let monitor = MonitorFromPoint(POINT { x: 400, y: 400 }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        assert_ne!(GetMonitorInfoW(monitor, &raw mut info), 0);
        let edge = sample_frame(hwnd, true, info.rcWork.right - 1, info.rcWork.bottom - 1, 0);
        eprintln!("edge={edge:?} work_bottom={}", info.rcWork.bottom);
        assert!(edge.screen_bottom <= info.rcWork.bottom);
        assert_eq!(edge.row_height, baseline.row_height);
        let full_baseline = sample_frame(hwnd, false, 400, 400, 150);
        let full_padded = sample_frame(hwnd, true, 400, 400, 150);
        assert_eq!(full_baseline.height, full_padded.height);
        assert_eq!(full_baseline.width, full_padded.width);
        assert_eq!(full_baseline.row_height, full_padded.row_height);
        eprintln!("PASS: native item count, width and row height unchanged; only outer padding increased");
        eprintln!("PASS: bottom edge stays on-screen; scrolling menus retain native geometry");
    }
}
