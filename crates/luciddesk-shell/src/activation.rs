//! Shell activation and decoding of incoming drag identities.
use crate::{
    ShellError, ShellIdentity,
    namespace::{shell_identity_from_names, shell_item_name, shell_item_path},
};
use std::{borrow::Cow, ffi::OsStr, os::windows::ffi::OsStrExt, path::Path, ptr};
use windows_sys::Win32::{
    Foundation::{ERROR_CANCELLED, ERROR_NO_ASSOCIATION, GetLastError, HWND},
    UI::{
        Shell::{
            OAIF_EXEC, OPENASINFO, SEE_MASK_FLAG_NO_UI, SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW,
            SHOpenWithDialog, ShellExecuteExW,
        },
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
};

/// Decode filesystem and virtual desktop items from an OLE drag without moving files.
/// # Errors
/// Rejects non-Shell data or an oversized drag.
pub fn drag_shell_identities(
    data: &windows::Win32::System::Com::IDataObject,
) -> windows::core::Result<Vec<ShellIdentity>> {
    use windows::Win32::UI::Shell::{
        IShellItemArray, SHCreateShellItemArrayFromDataObject, SIGDN_DESKTOPABSOLUTEPARSING,
    };
    unsafe {
        let items: IShellItemArray = SHCreateShellItemArrayFromDataObject(data)?;
        let count = items.GetCount()?;
        if count > 512 {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
        let mut identities = Vec::new();
        for index in 0..count {
            let shell_item = items.GetItemAt(index)?;
            // DragEnter runs during the cross-process preview handoff. Only
            // resolve names here: opening files for IDs and reading metadata
            // delays that handoff. The target matches these names against its
            // existing inventory and keeps the inventory's stable identities.
            let to_com_error = |e: ShellError| {
                windows::core::Error::new(windows::Win32::Foundation::E_FAIL, e.to_string())
            };
            let parsing_name =
                shell_item_name(&shell_item, SIGDN_DESKTOPABSOLUTEPARSING).map_err(to_com_error)?;
            let identity =
                shell_identity_from_names(Ok(parsing_name), || shell_item_path(&shell_item))
                    .map_err(to_com_error)?;
            identities.push(identity);
        }
        Ok(identities)
    }
}

/// Opens a filesystem or virtual Shell identity, offering Open With for unassociated files.
///
/// # Errors
///
/// Returns an error when Shell execution or the Open With dialog fails.
pub fn open_shell_identity(owner: isize, identity: &ShellIdentity) -> Result<(), ShellError> {
    let path = identity
        .file_system_path()
        .map(|path| {
            if path.is_absolute() {
                Ok(Cow::Borrowed(path))
            } else {
                std::path::absolute(path).map(Cow::Owned)
            }
        })
        .transpose()?;
    let target = wide_null(
        path.as_deref()
            .map_or_else(|| identity.activation_name(), Path::as_os_str),
    );
    let directory = path
        .as_deref()
        .and_then(Path::parent)
        .map(|parent| wide_null(parent.as_os_str()));
    let mut mask = SEE_MASK_FLAG_NO_UI;
    if matches!(identity, ShellIdentity::Namespace { .. }) {
        // A fully qualified parsing name can invoke the namespace handler directly.
        mask |= SEE_MASK_INVOKEIDLIST;
    }
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: mask,
        hwnd: owner as HWND,
        lpFile: target.as_ptr(),
        lpDirectory: directory.as_ref().map_or(ptr::null(), |path| path.as_ptr()),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&raw mut info) } == 0 {
        let error = unsafe { GetLastError() };
        return finish_failed_open(error, path.as_deref(), || {
            let dialog = OPENASINFO {
                pcszFile: target.as_ptr(),
                pcszClass: ptr::null(),
                oaifInFlags: OAIF_EXEC,
            };
            windows::core::HRESULT(unsafe { SHOpenWithDialog(owner as HWND, &raw const dialog) }).ok()
        });
    }
    Ok(())
}

fn finish_failed_open(
    error: u32,
    path: Option<&Path>,
    open_with: impl FnOnce() -> windows::core::Result<()>,
) -> Result<(), ShellError> {
    if error == ERROR_CANCELLED {
        return Ok(());
    }
    if error != ERROR_NO_ASSOCIATION || !path.is_some_and(Path::is_file) {
        return Err(ShellError::Execute(error as isize));
    }
    match open_with() {
        Err(error) if error.code() == windows::core::HRESULT::from_win32(ERROR_CANCELLED) => Ok(()),
        result => result.map_err(Into::into),
    }
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_with_only_handles_unassociated_files_and_cancel_is_benign() {
        let file = std::env::current_exe().unwrap();
        let cancelled = windows::core::HRESULT::from_win32(ERROR_CANCELLED);
        assert!(finish_failed_open(ERROR_NO_ASSOCIATION, Some(&file), || Ok(())).is_ok());
        assert!(
            finish_failed_open(ERROR_NO_ASSOCIATION, Some(&file), || Err(cancelled.into())).is_ok()
        );
        assert!(finish_failed_open(ERROR_CANCELLED, Some(&file), || unreachable!()).is_ok());
        for path in [file.parent(), None] {
            assert!(matches!(
                finish_failed_open(ERROR_NO_ASSOCIATION, path, || unreachable!()),
                Err(ShellError::Execute(code)) if code == ERROR_NO_ASSOCIATION as isize
            ));
        }
        let access_denied = windows::Win32::Foundation::E_ACCESSDENIED;
        assert!(matches!(
            finish_failed_open(ERROR_NO_ASSOCIATION, Some(&file), || Err(access_denied.into())),
            Err(ShellError::Com(error)) if error.code() == access_denied
        ));
    }

    #[test]
    fn opening_missing_file_returns_the_win32_error() {
        let _sta = crate::ShellApartment::initialize_sta().unwrap();
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let identity = ShellIdentity::FileSystem {
            path: std::env::temp_dir().join(format!("luciddesk-missing-{suffix}.exe")),
            volume_id: None,
            file_id: None,
        };
        assert!(matches!(
            open_shell_identity(0, &identity),
            Err(ShellError::Execute(code))
                if code == windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND as isize
        ));
    }
}
