#![windows_subsystem = "windows"]

mod plugins;

mod error_report;
mod app_icon;
mod clipboard;
mod desktop_component;
mod i18n;
mod pane;
mod startup;
mod system_info;
mod tray;
mod updates;
mod window_visibility;

use luciddesk_shell::{ShellApartment, local_app_data_path};
use std::{ffi::OsString, fs, path::PathBuf};

fn main() -> Result<(), String> {
    luciddesk_diagnostics::install_panic_hook();
    let monitor_child = std::env::args_os().any(|arg| arg == "--monitor-child");
    run().inspect_err(|error| {
        // Monitor children report through IPC; never open an invisible child dialog.
        if !monitor_child {
            luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "app.startup", error);
            error_report::show(0, i18n::text("error-startup-title"), error, luciddesk_window::MessageKind::Error);
        }
    })
}

fn run() -> Result<(), String> {
    // Installer preflight only: no COM, application windows, data access or hooks.
    if std::env::args_os()
        .skip(1)
        .eq([OsString::from("--check-desktop-component")])
    {
        std::process::exit(match luciddesk_explorer::desktop_component_released() {
            Ok(true) => 0,
            Ok(false) => 1,
            Err(_) => 2,
        });
    }
    unsafe {
        windows_sys::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let _apartment = ShellApartment::initialize_sta().map_err(|e| e.to_string())?;
    // Match the installer shortcuts; keep independent of version and install path.
    // Set before creating any UI, including when launched directly from the EXE.
    unsafe {
        windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(windows::core::w!(
            "Yuchen95.LucidDesk"
        ))
    }
    .map_err(|error| format!("failed to set application identity: {error}"))?;
    if arguments == [OsString::from("--monitor-child")] {
        return pane::run_monitor_child();
    }
    let startup = arguments.iter().any(|argument| argument == "--startup");
    let title = parse_options(arguments)?;
    let Some(_instance) = Instance::acquire(!startup)? else {
        return Ok(());
    };
    let _ = system_info::system();
    pane::render_debug::render_trace(format_args!("startup"));
    let path = database_path()?;
    init_logging(&path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    pane::run(&path, title)
}

fn init_logging(database: &std::path::Path) {
    luciddesk_diagnostics::initialize(
        database,
        env!("CARGO_PKG_VERSION"),
        env!("LUCIDDESK_BUILD_REVISION"),
        system_info::report,
    );
}

struct Instance(windows_sys::Win32::Foundation::HANDLE);
impl Instance {
    fn acquire(show_existing: bool) -> Result<Option<Self>, String> {
        use windows_sys::Win32::{Foundation::*, System::Threading::*, UI::WindowsAndMessaging::*};
        unsafe {
            let handle = CreateMutexW(
                std::ptr::null(),
                0,
                windows_sys::w!("Local\\LucidDesk.DesktopSession"),
            );
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(handle);
                if show_existing {
                    PostMessageW(
                        HWND_BROADCAST,
                        RegisterWindowMessageW(windows_sys::w!("LucidDesk.ShowExisting")),
                        0,
                        0,
                    );
                }
                return Ok(None);
            }
            Ok(Some(Self(handle)))
        }
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

fn parse_options(arguments: impl IntoIterator<Item = OsString>) -> Result<Option<String>, String> {
    let mut arguments = arguments.into_iter();
    let mut title = None;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--startup") => {}
            Some("--title") => {
                title = Some(
                    arguments
                        .next()
                        .ok_or("--title requires a value")?
                        .into_string()
                        .map_err(|_| "title must be valid Unicode")?,
                );
            }
            _ => {
                return Err(format!(
                    "Unsupported argument {}. Supported options: --title <name>, --startup.",
                    argument.to_string_lossy()
                ));
            }
        }
    }
    Ok(title)
}

fn database_path() -> Result<PathBuf, String> {
    if let Some(root) = std::env::var_os("LUCIDDESK_DATA_DIR") {
        return Ok(PathBuf::from(root).join("workspace.db"));
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    if let Some(directory) = executable.parent().and_then(portable_data_directory) {
        return Ok(directory.join("workspace.db"));
    }
    let root = local_app_data_path()
        .map_err(|error| format!("failed to resolve LocalAppData: {error}"))?;
    Ok(default_data_directory(&root).join("workspace.db"))
}

fn default_data_directory(root: &std::path::Path) -> PathBuf {
    root.join("LucidDesk")
}

fn portable_data_directory(executable_directory: &std::path::Path) -> Option<PathBuf> {
    executable_directory
        .join("portable")
        .is_file()
        .then(|| executable_directory.join("data"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_data_stays_beside_executable_only_when_enabled() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(portable_data_directory(root.path()), None);
        std::fs::write(root.path().join("portable"), "").unwrap();
        assert_eq!(
            portable_data_directory(root.path()),
            Some(root.path().join("data"))
        );
        assert!(!root.path().join("data").exists());
    }
    #[test]
    fn diagnostics_do_not_change_data_directory() {
        let root = tempfile::tempdir().unwrap();
        let current = root.path().join("LucidDesk");
        assert_eq!(default_data_directory(root.path()), current);
        std::fs::create_dir(root.path().join("LucidDesk-Diagnostics")).unwrap();
        assert_eq!(default_data_directory(root.path()), current);
        std::fs::create_dir(&current).unwrap();
        assert_eq!(default_data_directory(root.path()), current);
    }
    #[test]
    fn accepts_optional_title_and_rejects_invalid_arguments() {
        assert_eq!(parse_options([]).unwrap(), None);
        assert_eq!(parse_options([OsString::from("--startup")]).unwrap(), None);

        assert_eq!(
            parse_options([OsString::from("--title"), OsString::from("Work")])
                .unwrap()
                .as_deref(),
            Some("Work")
        );
        for value in ["--unknown", "C:\\folder", "--title"] {
            assert!(parse_options([OsString::from(value)]).is_err(), "{value}");
        }
    }
}
