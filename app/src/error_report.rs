//! Consistent presentation; operation-specific causes remain intact.
use luciddesk_window::MessageKind;

pub(crate) fn show(owner: isize, title: &str, message: &str, kind: MessageKind) {
    let message = if message.trim().is_empty() { crate::i18n::text("error-unknown") } else { message };
    if kind != MessageKind::Information {
        let level = if kind == MessageKind::Warning { luciddesk_diagnostics::Level::Warn } else { luciddesk_diagnostics::Level::Error };
        luciddesk_diagnostics::log(level, "ui.error", message);
    }
    let body = format!("{}\n\n{}", message.trim(), crate::i18n::text("error-copy-hint"));
    luciddesk_window::show_message(owner, &format!("LucidDesk — {title}"), &body, kind);
}

pub(crate) fn error(owner: isize, message: &str) {
    show(owner, crate::i18n::text("error-operation-title"), message, MessageKind::Error);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::{Cell, RefCell}, rc::Rc};
    use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;

    struct Probe {
        owner: HWND,
        disabled: Cell<bool>,
        duplicate_returned: Cell<bool>,
        observed: Cell<bool>,
    }
    thread_local! { static PROBE: RefCell<Option<Rc<Probe>>> = const { RefCell::new(None) }; }

    unsafe extern "system" fn inspect(_: HWND, _: u32, timer: usize, _: u32) {
        let Some(probe) = PROBE.with(|slot| slot.borrow().clone()) else { return; };
        let dialog = unsafe { GetLastActivePopup(probe.owner) };
        let mut class = [0u16; 32];
        let length = unsafe { GetClassNameW(dialog, class.as_mut_ptr(), class.len() as i32) };
        if String::from_utf16_lossy(&class[..length as usize]) != "#32770" { return; }
        unsafe { KillTimer(std::ptr::null_mut(), timer); }
        probe.disabled.set(unsafe { IsWindowEnabled(probe.owner) } == 0);
        // A real nested Windows timer callback attempts to show the same error.
        // A second modal dialog would hang here instead of returning.
        for _ in 0..3 {
            error(probe.owner as isize, "Native error regression (0x80070005)");
        }
        probe.duplicate_returned.set(true);
        probe.observed.set(true);
        unsafe { PostMessageW(dialog, WM_CLOSE, 0, 0); }
    }

    #[test]
    #[ignore = "Requires interactive native dialogs; run alone"]
    fn native_owned_errors_suppress_nested_duplicates_and_allow_later_retry() {
        let owner = windows_window::Window::new("LucidDesk error test")
            .size(320, 160).on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0))
            .create().unwrap();
        for _ in 0..2 {
            let probe = Rc::new(Probe { owner: owner.hwnd().cast(), disabled: Cell::new(false), duplicate_returned: Cell::new(false), observed: Cell::new(false) });
            PROBE.with(|slot| *slot.borrow_mut() = Some(probe.clone()));
            assert_ne!(unsafe { SetTimer(std::ptr::null_mut(), 0, 100, Some(inspect)) }, 0);
            error(owner.hwnd() as isize, "Native error regression (0x80070005)");
            assert!(probe.observed.get());
            assert!(probe.disabled.get(), "the initiating window must be disabled while its dialog is open");
            assert!(probe.duplicate_returned.get());
            assert_ne!(unsafe { IsWindowEnabled(owner.hwnd().cast()) }, 0);
            PROBE.with(|slot| slot.borrow_mut().take());
        }
    }
}
