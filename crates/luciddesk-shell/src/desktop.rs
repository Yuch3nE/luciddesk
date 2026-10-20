//! Read-only discovery of Explorer's desktop icon window and Shell view.
use crate::error::com_context;
use std::ptr;
use windows::{
    Win32::{
        System::{Com::IServiceProvider, Variant::VARIANT},
        UI::Shell::{
            CSIDL_DESKTOP, IShellBrowser, IShellView, IShellWindows, SID_STopLevelBrowser,
            SWC_DESKTOP, SWFO_NEEDDISPATCH,
        },
    },
    core::{Interface, Result},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM},
    UI::{
        Shell::{SHELLSTATEA, SHGetSetSettings, SSF_HIDEICONS},
        WindowsAndMessaging::{EnumWindows, FindWindowExW, GetShellWindow, IsWindowVisible},
    },
};

const HIDE_DESKTOP_ICONS_BIT: i32 = 1 << 12;

/// Resolve the current desktop view on the caller's COM apartment. The caller
/// owns the ShellWindows connection, including any reconnect/cache policy.
pub(crate) fn shell_view(shell: &IShellWindows) -> Result<IShellView> {
    unsafe {
        let mut desktop_hwnd = 0;
        let dispatch = shell
            .FindWindowSW(
                &VARIANT::from(CSIDL_DESKTOP.cast_signed()),
                &VARIANT::default(),
                SWC_DESKTOP,
                &raw mut desktop_hwnd,
                SWFO_NEEDDISPATCH,
            )
            .map_err(|e| com_context(e, "IShellWindows::FindWindowSW"))?;
        let provider: IServiceProvider = dispatch
            .cast()
            .map_err(|e| com_context(e, "QueryInterface(IServiceProvider)"))?;
        let browser: IShellBrowser = provider
            .QueryService(&SID_STopLevelBrowser)
            .map_err(|e| com_context(e, "IServiceProvider::QueryService(IShellBrowser)"))?;
        browser
            .QueryActiveShellView()
            .map_err(|e| com_context(e, "IShellBrowser::QueryActiveShellView"))
    }
}

/// Returns whether Explorer's native desktop icons are currently hidden.
#[must_use]
pub fn desktop_icons_hidden() -> bool {
    let mut state = SHELLSTATEA::default();
    unsafe {
        SHGetSetSettings(&raw mut state, SSF_HIDEICONS, 0);
    }
    let shell_state_hidden = state._bitfield1 & HIDE_DESKTOP_ICONS_BIT != 0;
    let view_hidden = desktop_list_view().is_some_and(|view| unsafe { IsWindowVisible(view) == 0 });
    shell_state_hidden || view_hidden
}

fn desktop_list_view() -> Option<HWND> {
    unsafe extern "system" fn enumerate_window(window: HWND, state: LPARAM) -> i32 {
        let result = unsafe { &mut *(state as *mut HWND) };
        if let Some(view) = unsafe { list_view_under(window) } {
            *result = view;
            0
        } else {
            1
        }
    }

    let shell = unsafe { GetShellWindow() };
    if !shell.is_null()
        && let Some(view) = unsafe { list_view_under(shell) }
    {
        return Some(view);
    }
    let mut result: HWND = ptr::null_mut();
    unsafe {
        EnumWindows(Some(enumerate_window), (&raw mut result) as LPARAM);
    }
    (!result.is_null()).then_some(result)
}

unsafe fn list_view_under(window: HWND) -> Option<HWND> {
    let definition = unsafe {
        FindWindowExW(
            window,
            ptr::null_mut(),
            windows_sys::w!("SHELLDLL_DefView"),
            ptr::null(),
        )
    };
    if definition.is_null() {
        return None;
    }
    let named = unsafe {
        FindWindowExW(
            definition,
            ptr::null_mut(),
            windows_sys::w!("SysListView32"),
            windows_sys::w!("FolderView"),
        )
    };
    if !named.is_null() {
        return Some(named);
    }
    let unnamed = unsafe {
        FindWindowExW(
            definition,
            ptr::null_mut(),
            windows_sys::w!("SysListView32"),
            ptr::null(),
        )
    };
    (!unnamed.is_null()).then_some(unnamed)
}
