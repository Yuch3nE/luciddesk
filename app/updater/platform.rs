use crate::Result;
use std::{
    ffi::{OsStr, OsString},
    io,
    mem::size_of,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr::null,
};
use windows_sys::Win32::{
    Foundation::*,
    System::Threading::*,
    UI::{Shell::*, WindowsAndMessaging::*},
};

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}
fn handle(h: HANDLE) -> io::Result<OwnedHandle> {
    if h.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(h) })
    }
}
pub fn message(text: &str, flags: u32) -> i32 {
    use luciddesk_window::{MessageKind, confirm_message, show_message};
    let kind = if flags & MB_ICONERROR != 0 { MessageKind::Error } else { MessageKind::Information };
    if flags & MB_OKCANCEL != 0 {
        if confirm_message(0, "LucidDesk", text, kind) { IDOK } else { IDCANCEL }
    } else {
        show_message(0, "LucidDesk", text, kind);
        IDOK
    }
}

pub struct Original {
    process: OwnedHandle,
    pid: u32,
    pub exe: PathBuf,
}
impl Original {
    pub fn open(pid: u32) -> Result<Self> {
        let process = handle(unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZATION_SYNCHRONIZE,
                0,
                pid,
            )
        })?;
        let mut path = vec![0; 32768];
        let mut len = path.len() as u32;
        if unsafe {
            QueryFullProcessImageNameW(process.as_raw_handle(), 0, path.as_mut_ptr(), &mut len)
        } == 0
        {
            return Err(io::Error::last_os_error().into());
        }
        let exe = PathBuf::from(OsString::from_wide(&path[..len as usize]));
        if !exe
            .file_name()
            .is_some_and(|s| s.eq_ignore_ascii_case("luciddesk.exe"))
        {
            return Err("Unexpected application process".into());
        }
        Ok(Self { process, pid, exe })
    }
    pub fn close(&self) -> Result<()> {
        unsafe {
            let hwnd = FindWindowW(
                windows_sys::w!("windows-window.Window"),
                windows_sys::w!("LucidDesk Tray"),
            );
            let mut owner = 0;
            GetWindowThreadProcessId(hwnd, &mut owner);
            if owner != self.pid
                || WaitForSingleObject(self.process.as_raw_handle(), 0) != WAIT_TIMEOUT
                || PostMessageW(hwnd, WM_CLOSE, 0, 0) == 0
                || WaitForSingleObject(self.process.as_raw_handle(), 20000) != WAIT_OBJECT_0
            {
                return Err("LucidDesk could not be closed safely".into());
            }
        }
        Ok(())
    }
}
pub fn gate(name: &str) -> Result<OwnedHandle> {
    unsafe {
        let raw = CreateMutexW(null(), 0, wide(name).as_ptr());
        let existing = GetLastError() == ERROR_ALREADY_EXISTS;
        let mutex = handle(raw)?;
        if existing {
            return Err("Another LucidDesk instance or update is active".into());
        }
        Ok(mutex)
    }
}
pub fn setup(path: &Path, root: &Path, scope: &str) -> Result<()> {
    let mut args = OsString::from(format!("{scope} /SILENT /NORESTART /SP- /DIR=\""));
    args.push(root);
    args.push("\"");
    let args = wide(args);
    let exe = wide(path);
    // ShellExecute honors the installer's elevation requirements for all-users installs.
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpFile: exe.as_ptr(),
        lpParameters: args.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe {
        if ShellExecuteExW(&mut info) == 0 {
            return Err(io::Error::last_os_error().into());
        }
        let process = handle(info.hProcess)?;
        let mut code = 0;
        if WaitForSingleObject(process.as_raw_handle(), INFINITE) != WAIT_OBJECT_0
            || GetExitCodeProcess(process.as_raw_handle(), &mut code) == 0
        {
            return Err(io::Error::last_os_error().into());
        }
        if code != 0 {
            return Err(format!("Installer exited with code {code}").into());
        }
    }
    Ok(())
}
