//! Opt-in measurement of filtering on Explorer's STA.
//! Enable the Perf.Enabled window property only for a bounded measurement, then
//! clear the counters afterwards. No item identities or user data are recorded.
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

const OBJECT_COUNT: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.ObjectCount.v1");
const FOLDER_COUNT: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.FolderCount.v1");
pub(super) fn counts(hwnd: HWND, objects: u32, items: i32) {
    // Offset by one so a valid zero count is distinguishable from no sample.
    unsafe {
        SetPropW(hwnd, OBJECT_COUNT, (u64::from(objects) + 1) as usize as _);
        SetPropW(hwnd, FOLDER_COUNT, (i64::from(items) + 1) as usize as _);
    }
}
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
    unsafe { for name in [STAGE, CODE, RECOVERY, RESTORE_CODE, RESTORE_STAGE, REFRESH_CODE, OBJECT_COUNT, FOLDER_COUNT] { RemovePropW(hwnd, name); } }
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
        let object_count = (GetPropW(hwnd, OBJECT_COUNT) as usize).checked_sub(1);
        let folder_count = (GetPropW(hwnd, FOLDER_COUNT) as usize).checked_sub(1);
        format!("object_count={object_count:?}, folder_count={folder_count:?}, stage={}({id}), HRESULT=0x{:08x}, build={build}, explorer_pid={pid}, view=0x{:x}, recovery={status}, restore_stage={}, restore=0x{:08x}, refresh=0x{:08x}",
            stage_name(id), GetPropW(hwnd, CODE) as usize as u32, hwnd as usize,
            stage_name(GetPropW(hwnd, RESTORE_STAGE) as usize),
            GetPropW(hwnd, RESTORE_CODE) as usize as u32, GetPropW(hwnd, REFRESH_CODE) as usize as u32)
    }
}

const ENABLED: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.Filter.Perf.Enabled");

// Inclusive timings: phases can nest, so their totals must not be added together.
#[derive(Clone, Copy)]
#[repr(usize)]
pub(super) enum Metric { Background, Paint, Request, Restore, Snapshot, Names, Validate, Present, Folder, Read, Add, Remove, Position }
impl Metric {
    fn properties(self) -> [windows_sys::core::PCWSTR; 3] {

        match self {
            Self::Background => [windows_sys::w!("LucidDesk.Filter.Perf.Count"), windows_sys::w!("LucidDesk.Filter.Perf.Micros"), windows_sys::w!("LucidDesk.Filter.Perf.MaxMicros")],
            Self::Paint => [windows_sys::w!("LucidDesk.Filter.Perf.Paint.Count"), windows_sys::w!("LucidDesk.Filter.Perf.Paint.Micros"), windows_sys::w!("LucidDesk.Filter.Perf.Paint.MaxMicros")],
            Self::Request => [windows_sys::w!("LucidDesk.Filter.Perf.Request.Count"), windows_sys::w!("LucidDesk.Filter.Perf.Request.Micros"), windows_sys::w!("LucidDesk.Filter.Perf.Request.MaxMicros")],
            Self::Restore => [windows_sys::w!("LucidDesk.Filter.Perf.Restore.Count"), windows_sys::w!("LucidDesk.Filter.Perf.Restore.Micros"), windows_sys::w!("LucidDesk.Filter.Perf.Restore.MaxMicros")],
            Self::Snapshot => [windows_sys::w!("LucidDesk.Filter.Perf.Snapshot.Count"), windows_sys::w!("LucidDesk.Filter.Perf.Snapshot.Micros"), windows_sys::w!("LucidDesk.Filter.Perf.Snapshot.MaxMicros")],
            Self::Names => [windows_sys::w!("LucidDesk.Filter.Perf.Names.Count"), windows_sys::w!("LucidDesk.Filter.Perf.Names.Micros"), windows_sys::w!("LucidDesk.Filter.Perf.Names.MaxMicros")],
            Self::Validate => [windows_sys::w!("LucidDesk.Filter.Perf.Validate.Count"), windows_sys::w!("LucidDesk.Filter.Perf.Validate.Micros"), windows_sys::w!("LucidDesk.Filter.Perf.Validate.MaxMicros")],
            Self::Folder => [windows_sys::w!("LucidDesk.Filter.Perf.Folder.Count"), windows_sys::w!("LucidDesk.Filter.Perf.Folder.Micros"), windows_sys::w!("LucidDesk.Filter.Perf.Folder.MaxMicros")],
            Self::Read => [windows_sys::w!("LucidDesk.Filter.Perf.Read.Count"), windows_sys::w!("LucidDesk.Filter.Perf.Read.Micros"), windows_sys::w!("LucidDesk.Filter.Perf.Read.MaxMicros")],
            Self::Add => [windows_sys::w!("LucidDesk.Filter.Perf.Add.Count"), windows_sys::w!("LucidDesk.Filter.Perf.Add.Micros"), windows_sys::w!("LucidDesk.Filter.Perf.Add.MaxMicros")],
            Self::Remove => [windows_sys::w!("LucidDesk.Filter.Perf.Remove.Count"), windows_sys::w!("LucidDesk.Filter.Perf.Remove.Micros"), windows_sys::w!("LucidDesk.Filter.Perf.Remove.MaxMicros")],
            Self::Position => [windows_sys::w!("LucidDesk.Filter.Perf.Position.Count"), windows_sys::w!("LucidDesk.Filter.Perf.Position.Micros"), windows_sys::w!("LucidDesk.Filter.Perf.Position.MaxMicros")],
            Self::Present => [windows_sys::w!("LucidDesk.Filter.Perf.Present.Count"), windows_sys::w!("LucidDesk.Filter.Perf.Present.Micros"), windows_sys::w!("LucidDesk.Filter.Perf.Present.MaxMicros")],
        }
    }
}
const METRICS: [Metric; 13] = [Metric::Background, Metric::Paint, Metric::Request, Metric::Restore,
    Metric::Snapshot, Metric::Names, Metric::Validate, Metric::Present, Metric::Folder, Metric::Read, Metric::Add, Metric::Remove, Metric::Position];
#[derive(Clone, Copy, Default)]
struct Timing { count: usize, micros: usize, max: usize }
thread_local! {
    static TIMINGS: std::cell::Cell<[Timing; 13]> = const { std::cell::Cell::new([Timing { count: 0, micros: 0, max: 0 }; 13]) };
    static SAMPLE_WINDOW: std::cell::Cell<HWND> = const { std::cell::Cell::new(std::ptr::null_mut()) };
}
pub(super) fn clear(hwnd: HWND) {
    unsafe {
        RemovePropW(hwnd, ENABLED);
        for metric in METRICS {
            for property in metric.properties() { RemovePropW(hwnd, property); }
        }
    }
}

pub(super) struct Sample {
    hwnd: HWND,
    previous: HWND,
    metric: Metric,
    started: Instant,
}
impl Sample {
    pub fn start(hwnd: HWND, metric: Metric) -> Option<Self> {
        if unsafe { GetPropW(hwnd, ENABLED) }.is_null() { return None; }
        Some(Self::enabled(hwnd, metric))
    }
    // Nested phases use the active scope rather than querying Explorer per item.
    pub fn phase(metric: Metric) -> Option<Self> {
        let hwnd = SAMPLE_WINDOW.get();
        (!hwnd.is_null()).then(|| Self::enabled(hwnd, metric))
    }
    fn enabled(hwnd: HWND, metric: Metric) -> Self {
        let previous = SAMPLE_WINDOW.replace(hwnd);
        if previous.is_null() { TIMINGS.set([Timing::default(); 13]); }
        Self { hwnd, previous, metric, started: Instant::now() }
    }
}
impl Drop for Sample {
    fn drop(&mut self) {
        let elapsed = self.started.elapsed().as_micros().min(usize::MAX as u128) as usize;
        let mut timings = TIMINGS.get();
        let timing = &mut timings[self.metric as usize];
        timing.count = timing.count.saturating_add(1);
        timing.micros = timing.micros.saturating_add(elapsed);
        timing.max = timing.max.max(elapsed);
        TIMINGS.set(timings);
        SAMPLE_WINDOW.set(self.previous);
        if !self.previous.is_null() { return; }
        // Publish once per outer operation, outside its measured interval. Never
        // make window-property calls per icon; instrumentation would dominate.
        unsafe {
            if GetPropW(self.hwnd, ENABLED).is_null() { return; }
            for metric in METRICS {
                let timing = timings[metric as usize];
                if timing.count == 0 { continue; }
                let [count, micros, max] = metric.properties();
                SetPropW(self.hwnd, count, (GetPropW(self.hwnd, count) as usize).saturating_add(timing.count) as _);
                SetPropW(self.hwnd, micros, (GetPropW(self.hwnd, micros) as usize).saturating_add(timing.micros) as _);
                SetPropW(self.hwnd, max, (GetPropW(self.hwnd, max) as usize).max(timing.max) as _);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn performance_scopes_are_opt_in_nested_and_clearable() {
        unsafe {
            let hwnd = CreateWindowExW(0, windows_sys::w!("STATIC"), windows_sys::w!("perf test"), WS_POPUP,
                0, 0, 1, 1, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
            assert!(!hwnd.is_null());
            assert!(Sample::start(hwnd, Metric::Paint).is_none());
            assert!(Sample::phase(Metric::Snapshot).is_none());
            SetPropW(hwnd, ENABLED, 1usize as _);
            {
                let _root = Sample::start(hwnd, Metric::Paint).unwrap();
                { let _phase = Sample::phase(Metric::Snapshot).unwrap(); }
                assert_eq!(SAMPLE_WINDOW.get(), hwnd);
            }
            assert!(Sample::phase(Metric::Snapshot).is_none());
            for metric in [Metric::Paint, Metric::Snapshot] {
                let [count, total, max] = metric.properties();
                assert_eq!(GetPropW(hwnd, count) as usize, 1);
                assert_eq!(GetPropW(hwnd, total), GetPropW(hwnd, max));
            }
            let interrupted = Sample::start(hwnd, Metric::Request).unwrap();
            clear(hwnd);
            drop(interrupted);
            for metric in [Metric::Paint, Metric::Snapshot, Metric::Request] {
                for name in metric.properties() { assert!(GetPropW(hwnd, name).is_null()); }
            }
            assert!(SAMPLE_WINDOW.get().is_null());
            DestroyWindow(hwnd);
        }
    }
    #[test]
    fn original_and_recovery_errors_survive_detach_until_next_attach() {
        unsafe {
            let hwnd = CreateWindowExW(0, windows_sys::w!("STATIC"), windows_sys::w!("filter diagnostic test"), WS_POPUP,
                0, 0, 1, 1, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
            assert!(!hwnd.is_null());
            counts(hwnd, 0, 2);
            assert!(summary(hwnd).contains("object_count=Some(0), folder_count=Some(2)"));
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
            assert!(summary(hwnd).contains("object_count=None, folder_count=None"));
            assert!(summary(hwnd).contains("HRESULT=0x00000000"));
            assert!(summary(hwnd).contains("recovery=none"));
            DestroyWindow(hwnd);
        }
    }
}
