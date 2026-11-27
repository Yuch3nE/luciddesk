//! Desktop view discovery and conflicting extension detection.
#![allow(clippy::cast_possible_truncation)]
use std::{
    mem::size_of,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        EnumWindows, FindWindowExW, GetClassNameW, GetShellWindow, GetWindowThreadProcessId,
    },
};

const DESKTOP_VIEW_UNAVAILABLE: &str = "Explorer desktop icon view is unavailable";

/// Finds the real desktop `ListView`; never creates or hides a replacement icon layer.
/// # Errors
/// Returns an error when the current Shell desktop cannot be found.
pub fn desktop_view() -> Result<isize, String> {
    let shell = unsafe { GetShellWindow() };
    let process = window_process(shell).ok_or(DESKTOP_VIEW_UNAVAILABLE)?;
    if let Some(view) = list_view_under(shell, process) {
        return Ok(view as isize);
    }
    let mut search = DesktopSearch { process, view: None };
    unsafe {
        EnumWindows(Some(find_view), (&raw mut search) as isize);
    }
    search.view.map(|view| view as isize).ok_or_else(|| DESKTOP_VIEW_UNAVAILABLE.into())
}

struct DesktopSearch {
    process: u32,
    view: Option<HWND>,
}

fn window_process(window: HWND) -> Option<u32> {
    let mut process = 0;
    let thread = unsafe { GetWindowThreadProcessId(window, &raw mut process) };
    (thread != 0 && process != 0).then_some(process)
}

unsafe extern "system" fn find_view(hwnd: HWND, lp: isize) -> i32 {
    let search = unsafe { &mut *(lp as *mut DesktopSearch) };
    if window_process(hwnd) == Some(search.process) {
        let mut class = [0u16; 16];
        let length = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
        if length > 0 && ["Progman", "WorkerW"].iter().any(|name| {
            class[..length as usize].iter().copied().eq(name.encode_utf16())
        }) {
            search.view = list_view_under(hwnd, search.process);
        }
    }
    i32::from(search.view.is_none())
}

fn list_view_under(hwnd: HWND, process: u32) -> Option<HWND> {
    let defview = unsafe {
        FindWindowExW(
            hwnd,
            null_mut(),
            windows_sys::w!("SHELLDLL_DefView"),
            null(),
        )
    };
    if window_process(defview) != Some(process) {
        return None;
    }
    let view = unsafe {
        FindWindowExW(defview, null_mut(), windows_sys::w!("SysListView32"), null())
    };
    (window_process(view) == Some(process)).then_some(view)
}

/// Detect another active desktop organizer before allowing work-area changes.
#[must_use]
pub fn conflicting_desktop_extension() -> bool {
    // Detection is intentionally limited to known loaded Explorer modules, not window titles.
    // The app may also use this to display a concrete conflict instead of fighting its layout.
    let Ok(view) = desktop_view() else {
        return false;
    };
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(view as HWND, &raw mut pid);
    }
    loaded_fences(pid) && fences_running()
}

fn fences_running() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return true;
        }
        let mut item = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..std::mem::zeroed()
        };
        let mut current = Process32FirstW(snapshot, &raw mut item);
        let mut found = false;
        while current != 0 {
            let end = item
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(item.szExeFile.len());
            if String::from_utf16_lossy(&item.szExeFile[..end]).eq_ignore_ascii_case("Fences.exe") {
                found = true;
                break;
            }
            current = Process32NextW(snapshot, &raw mut item);
        }
        CloseHandle(snapshot);
        found
    }
}

fn loaded_fences(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW, TH32CS_SNAPMODULE,
        TH32CS_SNAPMODULE32,
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
        if snapshot == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry = MODULEENTRY32W {
            dwSize: size_of::<MODULEENTRY32W>() as u32,
            ..std::mem::zeroed()
        };
        let mut found = false;
        let mut available = Module32FirstW(snapshot, &raw mut entry);
        while available != 0 {
            let end = entry
                .szModule
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szModule.len());
            let name = String::from_utf16_lossy(&entry.szModule[..end]).to_lowercase();
            if name == "desktopdock64.dll" {
                found = true;
                break;
            }
            available = Module32NextW(snapshot, &raw mut entry);
        }
        CloseHandle(snapshot);
        found
    }
}

/// Confirm that Explorer no longer has any desktop component mapped, including
/// legacy pinned images loaded from a different installation or a renamed file.
/// # Errors
/// Returns an error if the desktop process or its module list cannot be inspected.
pub fn desktop_component_released() -> Result<bool, String> {
    let view = desktop_view()?;
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(view as HWND, &raw mut pid);
    }
    component_released_in(pid)
}

pub(crate) fn component_released_in(pid: u32) -> Result<bool, String> {
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_BAD_LENGTH, ERROR_NO_MORE_FILES, GetLastError, INVALID_HANDLE_VALUE,
        },
        System::{Diagnostics::ToolHelp::*, Threading::Sleep},
    };
    if pid == 0 {
        return Err("无法确定 Explorer 进程".into());
    }
    unsafe {
        let mut snapshot = INVALID_HANDLE_VALUE;
        for _ in 0..10 {
            snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
            if snapshot != INVALID_HANDLE_VALUE {
                break;
            }
            if GetLastError() != ERROR_BAD_LENGTH {
                break;
            }
            Sleep(10);
        }
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut entry = MODULEENTRY32W {
            dwSize: size_of::<MODULEENTRY32W>() as u32,
            ..std::mem::zeroed()
        };
        let mut available = Module32FirstW(snapshot, &raw mut entry);
        let mut released = true;
        while available != 0 {
            let end = entry
                .szModule
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szModule.len());
            if is_component_module(&String::from_utf16_lossy(&entry.szModule[..end]))
            {
                released = false;
                break;
            }
            available = Module32NextW(snapshot, &raw mut entry);
        }
        let error = GetLastError();
        CloseHandle(snapshot);
        if released && error != ERROR_NO_MORE_FILES {
            return Err(format!("无法确认 Explorer 桌面组件已释放 ({error})"));
        }
        Ok(released)
    }
}

// Older installations may still have the previous DLL loaded in Explorer.
fn is_component_module(name: &str) -> bool {
    name.eq_ignore_ascii_case("luciddesk_explorer.dll")
        || name.eq_ignore_ascii_case("luciddesk_desktop.dll")
}

#[cfg(test)]
mod component_tests {
    use super::*;
    use windows_sys::Win32::{
        Foundation::FreeLibrary,
        System::{LibraryLoader::LoadLibraryExW, Threading::GetCurrentProcessId},
    };

    #[test]
    fn mapped_component_blocks_attach_before_any_hook_is_installed() {
        let dll = std::env::current_exe()
            .unwrap()
            .with_file_name("luciddesk_explorer.dll");
        let legacy_dir = std::env::temp_dir().join(format!("luciddesk-component-{}", std::process::id()));
        std::fs::create_dir_all(&legacy_dir).unwrap();
        let legacy_dll = legacy_dir.join("luciddesk_desktop.dll");
        std::fs::copy(&dll, &legacy_dll).unwrap();
        // Exercise real module enumeration for both current and pre-rename installations.
        for dll in [dll, legacy_dll.clone()] {
            let path: Vec<u16> = dll
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect();
            unsafe {
                let module = LoadLibraryExW(path.as_ptr(), null_mut(), 0);
                assert!(!module.is_null(), "{}", std::io::Error::last_os_error());
                let window = windows_sys::Win32::UI::WindowsAndMessaging::CreateWindowExW(
                    0,
                    windows_sys::w!("STATIC"),
                    windows_sys::w!("component guard test"),
                    windows_sys::Win32::UI::WindowsAndMessaging::WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                );
                assert!(!window.is_null());
                assert!(!component_released_in(GetCurrentProcessId()).unwrap());
                let result =
                    crate::filter::FilterSession::connect(window as isize, window as isize, &dll);
                assert!(result.err().unwrap().contains("旧桌面组件尚未释放"));
                windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(window);
                assert_ne!(FreeLibrary(module), 0);
                assert!(component_released_in(GetCurrentProcessId()).unwrap());
            }
        }
        std::fs::remove_file(legacy_dll).unwrap();
        std::fs::remove_dir(legacy_dir).unwrap();
    }

    #[test]
    fn invalid_process_is_an_error_not_a_safe_to_install_result() {
        assert!(component_released_in(0).is_err());
        assert!(component_released_in(u32::MAX).is_err());
    }
}
