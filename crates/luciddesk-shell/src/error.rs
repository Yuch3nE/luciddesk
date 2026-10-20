//! Errors reported by Windows Shell operations.
use std::{fmt, io};

/// Preserve HRESULT identity (including retryable codes) while identifying the failed API.
#[track_caller]
pub(crate) fn com_context(error: windows::core::Error, operation: &str) -> windows::core::Error {
    windows::core::Error::new(
        error.code(),
        format!(
            "{operation} location={}: {error}",
            std::panic::Location::caller()
        ),
    )
}

#[derive(Debug)]
pub enum ShellError {
    Io(io::Error),
    Com(windows::core::Error),
    Utf16(std::string::FromUtf16Error),
    Windows(i32),
    Execute(isize),
    ChangeNotification,
}

impl fmt::Display for ShellError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "folder I/O error: {error}"),
            Self::Com(error) => write!(formatter, "Windows Shell COM error: {error}"),
            Self::Utf16(error) => write!(formatter, "invalid UTF-16 from Windows Shell: {error}"),
            Self::Windows(code) => write!(formatter, "Windows Shell error 0x{code:08x}"),
            Self::Execute(code) => write!(formatter, "ShellExecuteW failed with code {code}"),
            Self::ChangeNotification => {
                write!(
                    formatter,
                    "failed to register Desktop Shell change notifications"
                )
            }
        }
    }
}

impl std::error::Error for ShellError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Com(error) => Some(error),
            Self::Utf16(error) => Some(error),
            Self::Windows(_) | Self::Execute(_) | Self::ChangeNotification => None,
        }
    }
}

impl From<io::Error> for ShellError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<windows::core::Error> for ShellError {
    fn from(value: windows::core::Error) -> Self {
        Self::Com(value)
    }
}

impl From<std::string::FromUtf16Error> for ShellError {
    fn from(value: std::string::FromUtf16Error) -> Self {
        Self::Utf16(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn api_context_preserves_original_hresult_cause_and_call_site() {
        for code in [
            windows::Win32::Foundation::E_ACCESSDENIED,
            windows::Win32::Foundation::E_BOUNDS,
            windows::core::HRESULT::from_win32(windows::Win32::Foundation::ERROR_RETRY.0),
        ] {
            let original = windows::core::Error::new(code, "原始错误原因：desktop COM failure");
            let line = line!() + 1;
            let detailed = com_context(original, "IFolderView2::ItemCount");
            assert_eq!(detailed.code(), code);
            let text = detailed.to_string();
            assert!(text.contains("IFolderView2::ItemCount"));
            assert!(text.contains("原始错误原因：desktop COM failure"));
            assert!(text.contains(&format!("{}:{line}:", file!())));
        }
    }
}
