//! Native HMENU theme opt-in. Unsupported builds keep Windows' default behavior.
//! uxtheme ordinals are undocumented; 135 changed ABI before Windows 10 1903.
use std::sync::OnceLock;
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HWND},
    System::{
        LibraryLoader::{
            GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
        },
        SystemInformation::OSVERSIONINFOW,
    },
    UI::{
        Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
        WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW},
    },
};

type PreferredMode = unsafe extern "system" fn(i32) -> i32;
type AllowWindow = unsafe extern "system" fn(HWND, bool) -> bool;
type ShouldUseDark = unsafe extern "system" fn() -> bool;
type Refresh = unsafe extern "system" fn();

struct ThemeApi {
    // Keep the DLL loaded for the lifetime of the process and its function pointers.
    _module: usize,
    preferred: PreferredMode,
    allow_window: AllowWindow,
    should_use_dark: ShouldUseDark,
    refresh: Refresh,
    flush: Refresh,
}

fn api() -> Option<&'static ThemeApi> {
    static API: OnceLock<Option<ThemeApi>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let ntdll = GetModuleHandleW(windows_sys::w!("ntdll.dll"));
        let version_proc = GetProcAddress(ntdll, c"RtlGetVersion".as_ptr().cast())?;
        let version_proc: unsafe extern "system" fn(*mut OSVERSIONINFOW) -> i32 =
            std::mem::transmute(version_proc);
        let mut version = OSVERSIONINFOW {
            dwOSVersionInfoSize: u32::try_from(size_of::<OSVERSIONINFOW>()).ok()?,
            ..Default::default()
        };
        if version_proc(&raw mut version) < 0
            || version.dwMajorVersion < 10
            || version.dwBuildNumber < 18362
        {
            return None;
        }
        let module = LoadLibraryExW(
            windows_sys::w!("uxtheme.dll"),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        );
        if module.is_null() {
            return None;
        }
        let functions = (|| {
            Some(ThemeApi {
                _module: module as usize,
                preferred: std::mem::transmute::<unsafe extern "system" fn() -> isize, PreferredMode>(GetProcAddress(module, 135usize as *const u8)?),
                allow_window: std::mem::transmute::<unsafe extern "system" fn() -> isize, AllowWindow>(GetProcAddress(module, 133usize as *const u8)?),
                should_use_dark: std::mem::transmute::<unsafe extern "system" fn() -> isize, ShouldUseDark>(GetProcAddress(
                    module,
                    132usize as *const u8,
                )?),
                refresh: std::mem::transmute::<unsafe extern "system" fn() -> isize, Refresh>(GetProcAddress(module, 104usize as *const u8)?),
                flush: std::mem::transmute::<unsafe extern "system" fn() -> isize, Refresh>(GetProcAddress(module, 136usize as *const u8)?),
            })
        })();
        if functions.is_none() {
            FreeLibrary(module);
        }
        functions
    })
    .as_ref()
}

// Refresh on opening, so changing Windows' default app mode takes effect on the
// next popup without restarting or polling. This changes only this process.
pub fn apply(owner: HWND) -> Option<bool> {
    configure(owner).map(|(dark, _)| dark)
}

// Explorer hosts the desktop fallback menu. Restore process-wide opt-in after
// its popup loop rather than leaving our preference in the host process.
pub struct ScopedTheme {
    preferred: i32,
}

pub fn apply_scoped(owner: HWND) -> Option<ScopedTheme> {
    let (_, preferred) = configure(owner)?;
    Some(ScopedTheme { preferred })
}

impl Drop for ScopedTheme {
    fn drop(&mut self) {
        if let Some(api) = api() {
            unsafe {
                (api.preferred)(self.preferred);
                (api.flush)();
            }
        }
    }
}

fn configure(owner: HWND) -> Option<(bool, i32)> {
    let api = api()?;
    unsafe {
        let mut contrast = HIGHCONTRASTW {
            cbSize: u32::try_from(size_of::<HIGHCONTRASTW>()).ok()?,
            ..Default::default()
        };
        if SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            (&raw mut contrast).cast(),
            0,
        ) == 0
        {
            return None;
        }
        let high_contrast = contrast.dwFlags & HCF_HIGHCONTRASTON != 0;
        // ShouldAppsUseDarkMode also observes the process preference. Clear a
        // previous ForceLight/ForceDark before querying the user's current mode,
        // and retain the original preference for the Explorer-scoped caller.
        let preferred = (api.preferred)(if high_contrast { 0 } else { 1 });
        (api.refresh)();
        let dark = !high_contrast && (api.should_use_dark)();
        // Default in high contrast lets the accessibility theme draw the menu.
        (api.preferred)(if high_contrast {
            0
        } else if dark {
            2
        } else {
            3
        });
        (api.allow_window)(owner, dark);
        (api.flush)();
        Some((dark, preferred))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "changes only the test process theme; run in a standalone native test process"]
    fn scoped_theme_reads_system_preference_and_restores_previous_process_mode() {
        let Some(api) = api() else { return };
        unsafe {
            let _restore = ScopedTheme { preferred: (api.preferred)(1) };
            let mut contrast = HIGHCONTRASTW {
                cbSize: size_of::<HIGHCONTRASTW>() as u32,
                ..Default::default()
            };
            assert_ne!(SystemParametersInfoW(SPI_GETHIGHCONTRAST, contrast.cbSize,
                (&raw mut contrast).cast(), 0), 0);
            (api.refresh)();
            let expected = contrast.dwFlags & HCF_HIGHCONTRASTON == 0 && (api.should_use_dark)();
            for prior in [2, 3] {
                (api.preferred)(prior);
                let (dark, previous) = configure(std::ptr::null_mut()).unwrap();
                assert_eq!(dark, expected, "process mode {prior} must not override Windows preference");
                assert_eq!(previous, prior);
                drop(ScopedTheme { preferred: previous });
                assert_eq!((api.preferred)(1), prior, "popup cleanup must restore the original process mode");
            }
        }
    }
}
