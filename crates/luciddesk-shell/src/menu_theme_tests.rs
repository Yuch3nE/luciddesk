//! Native menu lifecycle checks; never select or invoke a file command.
use windows_sys::Win32::{
    Foundation::HWND,
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
    UI::WindowsAndMessaging::*,
};

unsafe extern "system" fn cancel(hwnd: HWND, _: u32, timer: usize, _: u32) {
    unsafe {
        let mut visible = false;
        EnumThreadWindows(windows_sys::Win32::System::Threading::GetCurrentThreadId(),
            Some(find_popup), (&raw mut visible) as isize);
        if visible {
            KillTimer(hwnd, timer);
            EndMenu();
        }
    }
}

unsafe extern "system" fn find_popup(hwnd: HWND, data: isize) -> i32 {
    unsafe {
        if GetClassLongPtrW(hwnd, GCW_ATOM) == 32768 && IsWindowVisible(hwnd) != 0 {
            *(data as *mut bool) = true;
        }
    }
    1
}

#[test]
#[ignore = "opens and cancels native menus and changes test-process theme; run alone on an interactive desktop"]
fn native_menus_restore_process_theme_after_cancel_and_failure() {
    let _sta = crate::ShellApartment::initialize_sta().unwrap();
    // Also gates the version-sensitive ordinal on supported Windows builds.
    let Some(restore) = luciddesk_menu::apply_scoped_theme(std::ptr::null_mut()) else { return };
    unsafe {
        let module = GetModuleHandleW(windows_sys::w!("uxtheme.dll"));
        let preferred: unsafe extern "system" fn(i32) -> i32 =
            std::mem::transmute(GetProcAddress(module, 135usize as *const u8).unwrap());
        let window = windows_window::Window::new("LucidDesk native menu theme regression")
            .size(480, 320).style(WS_OVERLAPPEDWINDOW)
            .on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0))
            .create().unwrap();
        let hwnd = window.hwnd().cast();
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        let owner = windows::Win32::Foundation::HWND(hwnd);
        let point = windows::Win32::Foundation::POINT { x: 250, y: 250 };
        let root = tempfile_root();
        std::fs::create_dir(&root).unwrap();
        let path = root.join("fixture.txt");
        std::fs::write(&path, b"owned native menu fixture").unwrap();
        let identity = |path| crate::ShellIdentity::FileSystem { path, volume_id: None, file_id: None };
        for prior in [2, 3] {
            preferred(prior);
            assert_ne!(SetTimer(hwnd, 0x4c545443, 500, Some(cancel)), 0);
            let result = crate::show_file_items_menu(owner, &[identity(path.clone())], point);
            KillTimer(hwnd, 0x4c545443);
            assert!(!result.unwrap(), "cancelling must not request rename");
            assert_eq!(preferred(prior), prior, "file popup must restore process preference");

            assert_ne!(SetTimer(hwnd, 0x4c545443, 500, Some(cancel)), 0);
            let result = crate::show_folder_menu(owner, &root, point);
            KillTimer(hwnd, 0x4c545443);
            assert!(matches!(result.unwrap(), crate::FolderMenuResult::Cancelled));
            assert_eq!(preferred(prior), prior, "folder popup must restore process preference");

            assert!(crate::show_file_items_menu(owner, &[identity(root.join("missing.txt"))], point).is_err());
            assert_eq!(preferred(prior), prior, "failed file menu creation must restore process preference");
            assert!(crate::show_folder_menu(owner, &root.join("missing"), point).is_err());
            assert_eq!(preferred(prior), prior, "failed folder menu creation must preserve process preference");
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    drop(restore);
}

fn tempfile_root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("luciddesk-native-menu-theme-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))
}
