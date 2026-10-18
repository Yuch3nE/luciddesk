//! Opt-in measurement of background membership reconciliation on Explorer's STA.
//! Enable the Perf.Enabled window property only for a bounded measurement, then
//! remove all three properties. No item identities or user data are recorded.
use std::time::Instant;
use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(super) enum Stage {
    Attach = 1, ShellWindows, DesktopDispatch, ServiceProvider, Browser, ActiveView,
    ViewWindow, ViewIdentity, FolderView, ObjectView, ObjectCount, FolderCount,
    CountMatch, OwnerProcess, Library, OwnerWatch, ListSubclass, ParentSubclass, Timer,
    Snapshot, Existence, Add, Remove, Position, VerifyAdd, VerifyRemove, Membership,
    Ready, LoadLibrary, Hook, PostAttach, Export,
}
const STAGE: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.Stage.v1");
const CODE: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.StageError.v1");
const RECOVERY: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.Recovery.v1");
const RESTORE_CODE: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.RestoreError.v1");
const RESTORE_STAGE: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.RestoreStage.v1");
const REFRESH_CODE: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.RefreshError.v1");

pub(super) fn stage(hwnd: HWND, value: Stage) {
    unsafe { SetPropW(hwnd, STAGE, value as usize as _); }
}
pub(super) fn failure(hwnd: HWND, value: Stage, code: windows::core::HRESULT) {
    stage(hwnd, value);
    attach_failure(hwnd, code);
}
pub(super) fn attach_failure(hwnd: HWND, code: windows::core::HRESULT) {
    unsafe { SetPropW(hwnd, CODE, code.0 as u32 as usize as _); }
}
pub(super) fn recovery(hwnd: HWND, status: usize, restore: Option<windows::core::HRESULT>, refresh: Option<windows::core::HRESULT>) {
    unsafe {
        SetPropW(hwnd, RECOVERY, status as _);
        if let Some(code) = restore { SetPropW(hwnd, RESTORE_CODE, code.0 as u32 as usize as _); }
        if let Some(code) = refresh { SetPropW(hwnd, REFRESH_CODE, code.0 as u32 as usize as _); }
    }
}
pub(super) fn restore_failure(hwnd: HWND, value: Stage, code: windows::core::HRESULT) {
    unsafe {
        SetPropW(hwnd, RESTORE_STAGE, value as usize as _);
        SetPropW(hwnd, RESTORE_CODE, code.0 as u32 as usize as _);
    }
}
pub(super) fn reset(hwnd: HWND) {
    unsafe { for name in [STAGE, CODE, RECOVERY, RESTORE_CODE, RESTORE_STAGE, REFRESH_CODE] { RemovePropW(hwnd, name); } }
}
fn stage_name(id: usize) -> &'static str {
    // Unknown/new DLL stage IDs remain visible as numbers in summary().
    match id {
        1 => "Attach", 2 => "CoCreateInstance(ShellWindows)", 3 => "FindWindowSW",
        4 => "QueryInterface(IServiceProvider)", 5 => "QueryService(IShellBrowser)",
        6 => "QueryActiveShellView", 7 => "GetWindow", 8 => "ViewIdentity",
        9 => "QueryInterface(IFolderView2)", 10 => "QueryInterface(IShellFolderView)",
        11 => "GetObjectCount", 12 => "ItemCount", 13 => "CountMatch",
        14 => "OpenProcess", 15 => "LibraryLease", 16 => "OwnerWatch",
        17 => "SetWindowSubclass(List)", 18 => "SetWindowSubclass(Parent)", 19 => "SetTimer",
        20 => "Snapshot", 21 => "GetFileAttributesW", 22 => "AddObject", 23 => "RemoveObject",
        24 => "SelectAndPositionItems", 25 => "VerifyAdd", 26 => "VerifyRemove", 27 => "Membership",
        28 => "Ready", 29 => "LoadLibraryExW", 30 => "SetWindowsHookExW", 31 => "PostMessageW(Attach)", 32 => "GetProcAddress",
        _ => "Unknown",
    }
}
pub(super) fn summary(hwnd: HWND) -> String {
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
    #[link(name = "ntdll")]
    unsafe extern "system" { fn RtlGetVersion(version: *mut OSVERSIONINFOW) -> i32; }
    unsafe {
        let mut version: OSVERSIONINFOW = std::mem::zeroed();
        version.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
        let build = if RtlGetVersion(&raw mut version) >= 0 { version.dwBuildNumber } else { 0 };
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &raw mut pid);
        let id = GetPropW(hwnd, STAGE) as usize;
        let status = match GetPropW(hwnd, RECOVERY) as usize { 1 => "pending", 2 => "verified", _ => "none" };
        format!("stage={}({id}), HRESULT=0x{:08x}, build={build}, explorer_pid={pid}, view=0x{:x}, recovery={status}, restore_stage={}, restore=0x{:08x}, refresh=0x{:08x}",
            stage_name(id), GetPropW(hwnd, CODE) as usize as u32, hwnd as usize,
            stage_name(GetPropW(hwnd, RESTORE_STAGE) as usize),
            GetPropW(hwnd, RESTORE_CODE) as usize as u32, GetPropW(hwnd, REFRESH_CODE) as usize as u32)
    }
}

const ENABLED: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.Perf.Enabled");
const COUNT: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.Perf.Count");
const MICROS: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.Perf.Micros");

pub(super) fn clear(hwnd: HWND) {
    unsafe {
        for property in [ENABLED, COUNT, MICROS] {
            RemovePropW(hwnd, property);
        }
    }
}

pub(super) struct Sample(HWND, Instant);
impl Sample {
    pub fn start(hwnd: HWND) -> Option<Self> {
        (!unsafe { GetPropW(hwnd, ENABLED) }.is_null()).then(|| Self(hwnd, Instant::now()))
    }
}
impl Drop for Sample {
    fn drop(&mut self) {
        let elapsed = self.1.elapsed().as_micros().min(usize::MAX as u128) as usize;
        unsafe {
            let count = (GetPropW(self.0, COUNT) as usize).saturating_add(1);
            let micros = (GetPropW(self.0, MICROS) as usize).saturating_add(elapsed);
            SetPropW(self.0, COUNT, count as _);
            SetPropW(self.0, MICROS, micros as _);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_and_recovery_errors_survive_detach_until_next_attach() {
        unsafe {
            let hwnd = CreateWindowExW(0, windows_sys::w!("STATIC"), windows_sys::w!("filter diagnostic test"), WS_POPUP,
                0, 0, 1, 1, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
            assert!(!hwnd.is_null());
            failure(hwnd, Stage::ObjectView, windows::Win32::Foundation::E_NOINTERFACE);
            restore_failure(hwnd, Stage::Add, windows::Win32::Foundation::E_ACCESSDENIED);
            recovery(hwnd, 1, None, Some(windows::Win32::Foundation::E_FAIL));
            clear(hwnd); // Perf cleanup must not erase the failure report.
            let report = summary(hwnd);
            assert!(report.contains("QueryInterface(IShellFolderView)"));
            assert!(report.contains("HRESULT=0x80004002"));
            assert!(report.contains("recovery=pending"));
            assert!(report.contains("restore_stage=AddObject"));
            assert!(report.contains("restore=0x80070005"));
            assert!(report.contains("refresh=0x80004005"));
            recovery(hwnd, 2, None, None);
            assert!(summary(hwnd).contains("recovery=verified"));
            reset(hwnd);
            assert!(summary(hwnd).contains("HRESULT=0x00000000"));
            assert!(summary(hwnd).contains("recovery=none"));
            DestroyWindow(hwnd);
        }
    }
}
