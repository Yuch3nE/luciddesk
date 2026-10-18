//! Diagnostic-only DLL. No address patches or private function hooks.
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, IServiceProvider};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::*;
use windows::core::Interface;
use windows_sys::Win32::UI::WindowsAndMessaging::*;
#[path = "desktop_filter_transaction.rs"]
mod transaction;
mod isolated_menu_probe;
#[path = "../shell_pane_probe.rs"]
mod visible_shell_probe;

static BUSY: AtomicBool = AtomicBool::new(false);
const LOG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/desktop-filter-inprocess.log"
);

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DesktopFilterProbeHook(code: i32, wp: usize, lp: isize) -> isize {
    if code >= 0 && wp == PM_REMOVE as usize && lp != 0 {
        let message = unsafe { &*(lp as *const MSG) };
        if message.message
            == unsafe { RegisterWindowMessageW(windows_sys::w!("LucidDesk.DesktopFilterProbe.v1")) }
            && message.wParam == 0x4c504650
            && !BUSY.swap(true, Ordering::SeqCst)
        {
            if (3..=5).contains(&message.lParam) || (7..=8).contains(&message.lParam) {
                unsafe {
                    windows_sys::Win32::UI::Shell::SetWindowSubclass(message.hwnd, Some(menu_work), 0x4c504d57, 0);
                    PostMessageW(message.hwnd, RegisterWindowMessageW(windows_sys::w!("LucidDesk.MenuProbe.Work.v1")), 0, message.lParam);
                }
                return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wp, lp) };
            }
            let result = std::panic::catch_unwind(|| inspect(message.lParam));
            let mut report = match result {
                Ok(Ok(report)) => report,
                Ok(Err(error)) => format!("error={error:?}\n"),
                Err(_) => "panic caught\n".into(),
            };
            report.push_str("\nDONE\n");
            let _ = std::fs::write(LOG, report);
            BUSY.store(false, Ordering::SeqCst);
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wp, lp) }
}

unsafe extern "system" fn menu_work(hwnd: windows_sys::Win32::Foundation::HWND, msg: u32, wp: usize, lp: isize, _: usize, _: usize) -> isize {
    unsafe {
        if msg == RegisterWindowMessageW(windows_sys::w!("LucidDesk.MenuProbe.Work.v1")) {
            windows_sys::Win32::UI::Shell::RemoveWindowSubclass(hwnd, Some(menu_work), 0x4c504d57);
            let result = std::panic::catch_unwind(|| inspect(lp));
            let mut report = match result {
                Ok(Ok(report)) => report,
                Ok(Err(error)) => format!("error={error:?}\n"),
                Err(_) => "panic caught\n".into(),
            };
            report.push_str("\nDONE\n");
            let _ = std::fs::write(LOG, report);
            BUSY.store(false, Ordering::SeqCst);
            return 0;
        }
        windows_sys::Win32::UI::Shell::DefSubclassProc(hwnd,msg,wp,lp)
    }
}

fn inspect(command: isize) -> windows::core::Result<String> {
    unsafe {
        if (3..=8).contains(&command) {
            // Native Shell/XAML can release service callbacks after this probe's
            // stack has unwound. Keep their vtables mapped for Explorer's lifetime.
            let mut module = std::ptr::null_mut();
            if windows_sys::Win32::System::LibraryLoader::GetModuleHandleExW(
                windows_sys::Win32::System::LibraryLoader::GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | windows_sys::Win32::System::LibraryLoader::GET_MODULE_HANDLE_EX_FLAG_PIN,
                (DesktopFilterProbeHook as *const ()).cast(), &raw mut module,
            ) == 0 { return Err(windows::core::Error::from_thread()); }
        }
        if command == 8 {
            visible_shell_probe::start_on_explorer_loop().map_err(|error| windows::core::Error::new(
                windows::Win32::Foundation::E_FAIL, error.to_string()))?;
            return Ok("visible_shell_original_loop_started=true".into());
        }
        if command == 7 {
            // Controlled comparison: same host and presenter on Explorer's
            // existing desktop STA. The nested pump keeps messages flowing.
            std::fs::write(LOG,"visible_shell_desktop_sta_started=true\nDONE\n").ok();
            visible_shell_probe::run_in_explorer().map_err(|error| windows::core::Error::new(
                windows::Win32::Foundation::E_FAIL,error.to_string()))?;
            return Ok("visible_shell_desktop_sta_closed=true".into());
        }
        if command == 6 {
            // Keep the experiment separate from the real desktop STA and state.
            std::thread::Builder::new().name("LucidDesk visible Shell probe".into()).spawn(|| {
                let result = visible_shell_probe::run_in_explorer();
                if let Err(error) = result {
                    let _ = std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/visible-shell-probe-error.log"),error.to_string());
                }
            }).map_err(|error| windows::core::Error::new(windows::Win32::Foundation::E_FAIL,error.to_string()))?;
            return Ok("visible_shell_worker_started=true".into());
        }
        let mut log = String::new();
        let shell: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
        let mut desktop_hwnd = 0;
        let dispatch = shell.FindWindowSW(
            &VARIANT::from(CSIDL_DESKTOP.cast_signed()),
            &VARIANT::default(),
            SWC_DESKTOP,
            &raw mut desktop_hwnd,
            SWFO_NEEDDISPATCH,
        )?;
        let provider: IServiceProvider = dispatch.cast()?;
        let browser: IShellBrowser = provider.QueryService(&SID_STopLevelBrowser)?;
        let view = browser.QueryActiveShellView()?;
        let folder: IFolderView2 = view.cast()?;
        let _ = writeln!(
            log,
            "inprocess_pid={} thread={}",
            windows_sys::Win32::System::Threading::GetCurrentProcessId(),
            windows_sys::Win32::System::Threading::GetCurrentThreadId()
        );
        let _ = writeln!(log, "shell_count={}", folder.ItemCount(SVGIO_ALLVIEW)?);
        let _ = writeln!(
            log,
            "desktop_folder_IResultsFolder={:?}",
            folder.GetFolder::<IResultsFolder>().map(|_| ())
        );
        let _ = writeln!(
            log,
            "legacy_count={:?}",
            view.cast::<IShellFolderView>().and_then(|legacy| legacy.GetObjectCount())
        );
        let _ = writeln!(
            log,
            "view_IShellFolderView={:?}",
            view.cast::<IShellFolderView>().map(|_| ())
        );
        let _ = writeln!(
            log,
            "view_IFolderFilterSite={:?}",
            view.cast::<IFolderFilterSite>().map(|_| ())
        );
        let _ = writeln!(
            log,
            "browser_IFolderFilterSite={:?}",
            browser.cast::<IFolderFilterSite>().map(|_| ())
        );
        if command == 1 || command == 2 {
            if let Err(error) = transaction::run(&view, &folder, command == 2, &mut log) {
                let _ = writeln!(log, "transaction_error={error:?}");
            }
        }
        if (3..=5).contains(&command) {
            if let Err(error) = isolated_menu_probe::run(&folder, command, &mut log) {
                let _ = writeln!(log, "isolated_menu_error={error:?}");
            }
        }
        Ok(log)
    }
}
