//! Explorer-hosted desktop item menus. Call on the UI STA in response to user input.
mod input;
mod lifetime;
pub use input::MenuInvocation;
#[cfg(feature = "desktop-menu-diagnostics")]
mod legacy;
#[cfg(feature = "desktop-menu-diagnostics")]
mod legacy_input;
mod peek;
#[cfg(feature = "desktop-menu-diagnostics")]
mod performance;
mod selection;
#[cfg(feature = "desktop-menu-diagnostics")]
pub use legacy::{show_desktop_item_menu, show_desktop_items_menu};
pub use peek::peek_desktop_item;
pub(crate) use selection::update_hints;

use std::cell::Cell;
use windows::Win32::Foundation::{ERROR_BUSY, HWND};
use windows::core::Result;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, GA_ROOT, GetAncestor, GetForegroundWindow, GetWindowThreadProcessId,
    IsWindowVisible, SetForegroundWindow,
};

thread_local! { static ACTIVE: Cell<bool> = const { Cell::new(false) }; }
pub(crate) struct ActiveMenu;
impl ActiveMenu {
    pub(crate) fn acquire() -> Result<Self> {
        if ACTIVE.with(|active| active.replace(true)) {
            return Err(windows::core::Error::from_hresult(
                windows::core::HRESULT::from_win32(ERROR_BUSY.0),
            ));
        }
        Ok(Self)
    }
}
impl Drop for ActiveMenu {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(false));
    }
}

#[cfg(test)]
#[test]
fn menu_guard_rejects_reentry_until_dropped() {
    let active = ActiveMenu::acquire().unwrap();
    assert!(ActiveMenu::acquire().is_err());
    drop(active);
    assert!(ActiveMenu::acquire().is_ok());
}

struct ReturnFocus {
    owner: HWND,
    desktop: HWND,
}
impl Drop for ReturnFocus {
    fn drop(&mut self) {
        unsafe {
            focus_trace("return-before", self.owner, self.desktop, None);
            // A command may have opened Properties or another app, or the user
            // may have switched away. Never steal focus from that destination.
            if !self.owner.0.is_null()
                && IsWindowVisible(self.owner.0) != 0
                && GetForegroundWindow() == self.desktop.0
            {
                let accepted = SetForegroundWindow(self.owner.0);
                focus_trace("return-attempt", self.owner, self.desktop, Some(accepted));
            }
            focus_trace("return-after", self.owner, self.desktop, None);
        }
    }
}

// Opt-in diagnostics contain window classes/handles only, never window titles
// or filenames. Keep filesystem work out of the Explorer hook and paint paths.
fn focus_trace(stage: &str, owner: HWND, desktop: HWND, accepted: Option<i32>) {
    if !luciddesk_diagnostics::enabled(luciddesk_diagnostics::Level::Trace) {
        return;
    }
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GUITHREADINFO, GetClassNameW, GetGUIThreadInfo,
    };
    unsafe {
        let foreground = GetForegroundWindow();
        let mut name = [0u16; 128];
        let len = GetClassNameW(foreground, name.as_mut_ptr(), 128).max(0) as usize;
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let got = GetGUIThreadInfo(0, &raw mut info);
        luciddesk_diagnostics::emit!(
            luciddesk_diagnostics::Level::Trace,
            "shell.menu.focus",
            "{:?} {stage} owner={:?} desktop={:?} foreground={foreground:?} class={} accepted={accepted:?} gui={got} active={:?} focus={:?}",
            std::time::SystemTime::now(),
            owner.0,
            desktop.0,
            String::from_utf16_lossy(&name[..len]),
            info.hwndActive,
            info.hwndFocus
        );
    }
}

/// Shows a menu in an independently prepared Explorer Shell view. The caller
/// retains that view until this function returns, then finishes the menu session.
/// The backend may keep the idle host for reuse or for outstanding command dialogs.
/// The prepared host already owns the validated physical popup anchor.
/// # Errors
/// Fails if the host is invalid, focus cannot be handed off, or no popup appears.
pub fn show_isolated_item_menu(owner: HWND, host: HWND, invocation: MenuInvocation) -> Result<()> {
    let _active = ActiveMenu::acquire()?;
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, RegisterWindowMessageW};
        let open_message = RegisterWindowMessageW(windows_sys::w!("LucidDesk.IsolatedMenu.Open.v1"));
        if open_message == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let mut pid = 0;
        let thread = GetWindowThreadProcessId(host.0, &raw mut pid);
        if pid == 0 || thread == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let observer = lifetime::Observer::new(pid, thread)?;
        let _focus = ReturnFocus {
            owner,
            desktop: HWND(GetAncestor(host.0, GA_ROOT)),
        };
        // Preparation may already have handed foreground permission to Explorer.
        // A second grant can fail after that handoff; the host checks activation.
        AllowSetForegroundWindow(pid);
        if PostMessageW(
            host.0,
            open_message,
            usize::from(invocation == MenuInvocation::Keyboard),
            0,
        ) == 0
        {
            return Err(windows::core::Error::from_thread());
        }
        let result = observer.wait_for_close(Some(host.0));
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::GetPropW;
            luciddesk_diagnostics::emit!(
                luciddesk_diagnostics::Level::Debug,
                "shell.menu",
                "menu_shell_get_item_us={} menu_shell_build_us={} busy_cursor_cleared={}",
                (GetPropW(host.0, windows_sys::w!("LucidDesk.Menu.GetItemUs")) as usize)
                    .saturating_sub(1),
                (GetPropW(host.0, windows_sys::w!("LucidDesk.Menu.BuildUs")) as usize)
                    .saturating_sub(1),
                GetPropW(
                    GetAncestor(host.0, GA_ROOT),
                    windows_sys::w!("LucidDesk.Menu.BusyCursorCleared")
                ) as usize
            );
        }
        result
    }
}
