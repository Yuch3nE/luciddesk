//! Shared executable picker for preview and search applications.
use windows_sys::Win32::UI::Controls::Dialogs::*;

pub(super) fn browse(owner: isize, label: &str, executable: &str,
    failure: &'static str, invalid: &str) -> Result<Option<String>, String> {
    let filter: Vec<u16> = format!("{label}\0{executable}\0\0").encode_utf16().collect();
    let mut file = [0u16; 32768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner as _, lpstrFilter: filter.as_ptr(),
        lpstrFile: file.as_mut_ptr(), nMaxFile: file.len() as u32,
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    let selected = unsafe { GetOpenFileNameW(&raw mut dialog) } != 0;
    let error = if selected { 0 } else { unsafe { CommDlgExtendedError() } };
    selection(&file, selected, error, executable, failure, invalid)
}

fn selection(file: &[u16], selected: bool, error: u32, executable: &str,
    failure: &'static str, invalid: &str) -> Result<Option<String>, String> {
    if !selected {
        return if error == 0 { Ok(None) }
        else { Err(crate::i18n::format(failure, &[("error", error.to_string())])) };
    }
    let end = file.iter().position(|c| *c == 0).unwrap_or(file.len());
    let path = String::from_utf16_lossy(&file[..end]);
    if !std::path::Path::new(&path).file_name().is_some_and(|n| n.eq_ignore_ascii_case(executable)) {
        return Err(invalid.into());
    }
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picker_distinguishes_cancel_error_and_validated_selection() {
        for executable in ["Everything.exe", "QuickLook.exe", "PowerToys.Peek.UI.exe"] {
            let path = format!(r"C:\工具\{}", executable.to_uppercase());
            let buffer: Vec<_> = path.encode_utf16().chain([0, 65]).collect();
            let finish = |selected, error, expected| selection(&buffer, selected, error,
                expected, "ui-could-not-select-preview-app", "wrong executable");
            assert_eq!(finish(false, 0, executable).unwrap(), None);
            assert!(finish(false, 0x3003, executable).is_err());
            assert_eq!(finish(true, 0, executable).unwrap(), Some(path));
            assert_eq!(finish(true, 0, "other.exe").unwrap_err(), "wrong executable");
        }
    }
}
