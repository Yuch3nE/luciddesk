//! Explorer view membership filtering; no private function addresses or detours.
mod client;
mod diagnostics;
mod engine;
mod desktop_visibility;
mod explorer;
mod items;
mod library;
mod menu;
mod owner;
mod retry;
mod recovery;
mod schedule;
mod wire;
pub use client::FilterSession;

/// Read the current or most recent native view failure, including pending recovery.
/// Diagnostics survive a successful detach until the next attachment.
#[must_use]
pub fn desktop_filter_diagnostics() -> Option<String> {
    crate::desktop_view().ok().map(|view| diagnostics::summary(view as _))
}

use windows_sys::Win32::UI::WindowsAndMessaging::*;
pub(crate) const OWNER: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.Owner.v1");
pub(crate) const ACK: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.Ack.v1");
pub(crate) const ERROR: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.Error.v1");
pub(crate) const RENAME: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.Rename.v1");
pub(crate) const MENU_HOST: windows_sys::core::PCWSTR =
    windows_sys::w!("LucidDesk.Filter.MenuHost.v1");
pub(crate) const MENU_ERROR: windows_sys::core::PCWSTR =
    windows_sys::w!("LucidDesk.Filter.MenuError.v1");
const UPDATE_RELEASE: windows_sys::core::PCWSTR =
    windows_sys::w!("LucidDesk.Filter.UpdateRelease.v1");
const UPDATE_RELEASED: windows_sys::core::PCWSTR =
    windows_sys::w!("LucidDesk.Filter.UpdateReleased.v1");
const REQUEST_ERROR: windows_sys::core::PCWSTR =
    windows_sys::w!("LucidDesk.Filter.RequestError.v1");
const MAGIC: usize = 0x4c504631;
fn message() -> u32 {
    static ID: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *ID.get_or_init(|| unsafe {
        RegisterWindowMessageW(windows_sys::w!("LucidDesk.Filter.Attach.v1"))
    })
}
fn work_message() -> u32 {
    static ID: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *ID.get_or_init(|| unsafe {
        RegisterWindowMessageW(windows_sys::w!("LucidDesk.Filter.Work.v1"))
    })
}

/// Windows invokes this only for messages removed from the target desktop queue.
/// # Safety
/// The callback payload must be a valid Windows MSG supplied by WH_GETMESSAGE.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn LucidDeskFilterHook(code: i32, wp: usize, lp: isize) -> isize {
    let _callback = library::Callback::enter();
    if code >= 0 && wp == PM_REMOVE as usize && lp != 0 {
        let _ = std::panic::catch_unwind(|| {
            let msg = unsafe { &mut *(lp as *mut MSG) };
            if engine::desktop_gesture(msg) {
                msg.message = WM_NULL;
                msg.wParam = 0;
                msg.lParam = 0;
            }
            if msg.message == message() && msg.lParam == MAGIC as isize {
                engine::attach(msg.hwnd, msg.wParam as _);
            }
        });
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wp, lp) }
}
