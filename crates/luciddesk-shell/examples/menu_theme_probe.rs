//! Manual native Shell menu visual check. Right-click, inspect, then cancel.
use windows_sys::Win32::UI::WindowsAndMessaging::*;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _sta = luciddesk_shell::ShellApartment::initialize_sta()?;
    let path = std::env::args_os().nth(1).expect("absolute test file path");
    let identity = luciddesk_shell::ShellIdentity::FileSystem {
        path: path.into(),
        volume_id: None,
        file_id: None,
    };
    let window = windows_window::Window::new("LucidDesk native menu theme check - right click")
        .size(480, 320)
        .style(WS_OVERLAPPEDWINDOW)
        .on_message(move |raw, message, _, lparam| {
            let hwnd = raw.cast();
            match message {
                WM_CONTEXTMENU => {
                    let result = luciddesk_shell::show_file_items_menu(
                        windows::Win32::Foundation::HWND(hwnd),
                        std::slice::from_ref(&identity),
                        windows::Win32::Foundation::POINT {
                            x: i32::from(lparam as u16 as i16),
                            y: i32::from((lparam >> 16) as u16 as i16),
                        },
                        "Open file location",
                    );
                    println!("popup_result={result:?}");
                    Some(0)
                }
                WM_CLOSE => {
                    unsafe {
                        DestroyWindow(hwnd);
                    }
                    Some(0)
                }
                WM_DESTROY => {
                    unsafe {
                        PostQuitMessage(0);
                    }
                    Some(0)
                }
                _ => None,
            }
        })
        .create()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    unsafe {
        ShowWindow(window.hwnd().cast(), SW_SHOW);
        let mut message = MSG::default();
        while GetMessageW(&raw mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}
