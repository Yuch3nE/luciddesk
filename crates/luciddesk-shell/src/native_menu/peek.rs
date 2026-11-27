//! Use Peek's resident Shell entry point, preserving display names and virtual items.
use super::*;
use luciddesk_core::ShellIdentity;
use std::time::{Duration, Instant};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::UI::Shell::{
    IFolderView2, IShellWindows, SVGIO_SELECTION, SVSI_DESELECTOTHERS, SVSI_FOCUSED, SVSI_SELECT,
    SVUIA_ACTIVATE_FOCUS, ShellWindows,
};
use windows::Win32::{Foundation::E_FAIL, UI::Shell::SIGDN_NORMALDISPLAY};
use windows::core::Error;
use windows::core::Interface;
use windows_sys::Win32::{
    Foundation::{CloseHandle as CloseRawHandle, HANDLE},
    System::Threading::{
        EVENT_MODIFY_STATE, OpenEventW, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW, SetEvent,
    },
    UI::WindowsAndMessaging::*,
};

struct EventHandle(HANDLE);
impl Drop for EventHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseRawHandle(self.0) };
    }
}

#[derive(Debug)]
pub enum PeekError {
    Unavailable(Error),
    SelectionMismatch,
    ForegroundUnavailable,
    Cancelled,
    TimedOut,
    Shell(Error),
}

impl std::fmt::Display for PeekError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(error) => write!(formatter, "Open the PowerToys Peek signal: {error}"),
            Self::SelectionMismatch => formatter.write_str("The desktop selection does not match the Peek target"),
            Self::ForegroundUnavailable => formatter.write_str("The desktop could not take foreground focus for Peek"),
            Self::Cancelled => formatter.write_str("The preview was cancelled"),
            Self::TimedOut => formatter.write_str("Peek did not acknowledge the requested desktop item"),
            Self::Shell(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PeekError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable(error) | Self::Shell(error) => Some(error),
            _ => None,
        }
    }
}

impl From<Error> for PeekError {
    fn from(error: Error) -> Self {
        Self::Shell(error)
    }
}

fn peek_has_item(name: &str, file_name: Option<&str>, path: &mut [u16]) -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &raw mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut length = path.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &raw mut length);
        CloseRawHandle(process);
        if ok == 0
            || !String::from_utf16_lossy(&path[..length as usize])
                .to_ascii_lowercase()
                .ends_with("\\powertoys.peek.ui.exe")
        {
            return false;
        }
        let mut title = [0u16; 1024];
        let length = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
        let title = String::from_utf16_lossy(&title[..length.max(0) as usize]);
        // Peek sets its title after the synchronous Shell-item query. Activation
        // alone happens before that query on first launch and is not sufficient.
        matches_peek_title(&title, name, file_name)
    }
}

fn matches_peek_title(title: &str, name: &str, file_name: Option<&str>) -> bool {
    let matches_name = |candidate: &str| !candidate.is_empty()
        && (candidate == name || file_name == Some(candidate));
    matches_name(title)
        || title.rsplit_once(" - ").is_some_and(|(item, suffix)| {
            !suffix.is_empty() && matches_name(item)
        })
}

/// Preview a desktop namespace member through PowerToys' running Peek service.
/// Caller must permit temporary hidden desktop selection and release model borrows.
/// # Errors
/// Fails if Peek is unavailable, the item cannot be resolved, or handoff times out.
pub fn peek_desktop_item(owner: HWND, identity: &ShellIdentity) -> std::result::Result<(), PeekError> {
    let _active = ActiveMenu::acquire()?;
    unsafe {
        let signal = EventHandle(OpenEventW(
            EVENT_MODIFY_STATE,
            0,
            windows_sys::w!("Local\\ShowPeekEvent"),
        ));
        if signal.0.is_null() {
            return Err(PeekError::Unavailable(Error::from_thread()));
        }
        let shell: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
        let view = crate::desktop::shell_view(&shell)?;
        let folder: IFolderView2 = view.cast()?;
        let index = selection::resolve(&folder, identity.activation_name())?;
        let item = selection::item_at(&folder, index)?;
        let name = crate::namespace::shell_item_name(&item, SIGDN_NORMALDISPLAY)
            .map_err(|error| match error {
                crate::ShellError::Com(error) => crate::error::com_context(error, "Read Peek target display name"),
                other => Error::new(E_FAIL, other.to_string()),
            })?;
        let name = name.to_string_lossy();
        let path = crate::namespace::shell_item_path(&item);
        let file_name = path.as_deref()
            .and_then(std::path::Path::file_name)
            .map(std::ffi::OsStr::to_string_lossy);
        let hwnd = view.GetWindow()?.0;
        let restore = selection::RestoreSelection::deselect_on_close(&folder);
        let _focus = ReturnFocus {
            owner,
            desktop: HWND(GetAncestor(hwnd, GA_ROOT)),
        };
        folder.SelectItem(
            index,
            (SVSI_SELECT.0 | SVSI_FOCUSED.0 | SVSI_DESELECTOTHERS.0) as u32,
        )?;
        // Refuse to wake Peek with a stale/different desktop selection.
        let selected: windows::Win32::UI::Shell::IShellItemArray = folder.Items(SVGIO_SELECTION)?;
        if selected.GetCount()? != 1 || !selection::matches(&selected.GetItemAt(0)?, &item)? {
            return Err(PeekError::SelectionMismatch);
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &raw mut pid);
        if AllowSetForegroundWindow(pid) == 0
            || SetForegroundWindow(GetAncestor(hwnd, GA_ROOT)) == 0
        {
            return Err(PeekError::ForegroundUnavailable);
        }
        view.UIActivate(SVUIA_ACTIVATE_FOCUS.0 as u32)?;
        if SetEvent(signal.0) == 0 {
            return Err(crate::error::com_context(Error::from_thread(), "Signal PowerToys Peek").into());
        }
        let mut process_path = vec![0u16; 32768];
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if peek_has_item(&name, file_name.as_deref(), &mut process_path) {
                return Ok(restore.finish()?);
            }
            if !lifetime::pump_messages() {
                return Err(PeekError::Cancelled);
            }
            MsgWaitForMultipleObjectsEx(0, std::ptr::null(), 25, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
        Err(PeekError::TimedOut)
    }
}

#[cfg(test)]
mod tests {
    use super::matches_peek_title;

    #[test]
    fn peek_matches_the_display_name_or_full_filename() {
        let name = "report - draft";
        let file_name = Some("report - draft.txt");
        assert!(matches_peek_title(name, name, file_name));
        assert!(matches_peek_title("report - draft - Peek", name, file_name));
        assert!(matches_peek_title("report - draft.txt - Peek", name, file_name));
        assert!(!matches_peek_title("report - draft - other.txt - Peek", name, file_name));
        assert!(!matches_peek_title("Peek", "", None));
    }
}
