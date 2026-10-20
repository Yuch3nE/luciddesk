//! Native dialogs share formatting and suppress only duplicates currently open.
use std::{cell::RefCell, collections::HashSet};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MessageKind { Error, Warning, Information }

type Key = (isize, String, String, MessageKind);
thread_local! {
    static ACTIVE: RefCell<HashSet<Key>> = RefCell::new(HashSet::new());
}
struct Active(Key);
impl Active {
    fn enter(key: Key) -> Option<Self> {
        ACTIVE.with(|active| active.borrow_mut().insert(key.clone())).then(|| Self(key))
    }
}
impl Drop for Active {
    fn drop(&mut self) { ACTIVE.with(|active| { active.borrow_mut().remove(&self.0); }); }
}

fn normalized(text: &str) -> String {
    // An embedded NUL must not hide the remainder of an error or file path.
    text.trim().replace('\0', "␀").replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n")
}

/// Show immediately. The caller must release application borrows before calling.
/// Invalid/foreign owners are ignored; a valid owner remains disabled until dismissal.
/// Duplicate suppression lasts only while that exact dialog is open, never across retries.
pub fn show_message(owner: isize, title: &str, message: &str, kind: MessageKind) {
    let owner = valid_owner(owner);
    let title = normalized(title);
    let message = normalized(message);
    let Some(_active) = Active::enter((owner, title.clone(), message.clone(), kind)) else { return; };
    let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let icon = match kind {
        MessageKind::Error => MB_ICONERROR,
        MessageKind::Warning => MB_ICONWARNING,
        MessageKind::Information => MB_ICONINFORMATION,
    };
    unsafe { MessageBoxW(owner as _, message.as_ptr(), title.as_ptr(), MB_OK | icon); }
}

fn valid_owner(owner: isize) -> isize {
    let mut process = 0;
    if owner != 0 && unsafe { GetWindowThreadProcessId(owner as _, &raw mut process) } != 0
        && process == std::process::id()
    { owner } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formatting_preserves_details_paths_and_text_after_nul() {
        assert_eq!(normalized("  A\r\nB\nC\rD\0E  "), "A\r\nB\r\nC\r\nD␀E");
        assert_eq!(normalized("C:\\work\\a.txt (0x80070005)"), "C:\\work\\a.txt (0x80070005)");
    }
    #[test]
    fn identical_nested_dialogs_are_suppressed_but_new_errors_and_retries_are_not() {
        let key = (0, "title".into(), "error".into(), MessageKind::Error);
        let first = Active::enter(key.clone()).unwrap();
        assert!(Active::enter(key.clone()).is_none());
        assert!(Active::enter(key.clone()).is_none(), "rejecting a duplicate must not release the original dialog's guard");
        assert!(Active::enter((0, "title".into(), "different".into(), MessageKind::Error)).is_some());
        drop(first);
        assert!(Active::enter(key).is_some());
        assert_eq!(valid_owner(-1), 0);
    }
}
