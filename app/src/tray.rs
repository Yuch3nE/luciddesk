//! Notification-area entry, owned by the same UI thread as the panes.
use crate::app_icon::Icon;
use std::{ptr::null_mut, rc::Rc};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{Shell::*, WindowsAndMessaging::*},
};
use windows_window::Window;

const CALLBACK: u32 = WM_APP + 80;
const DISPATCH: u32 = WM_APP + 81;
const ICON_ID: u32 = 1;
// Shell headers define this macro as NIN_SELECT | NINF_KEY.
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;

#[derive(Clone, Copy)]
pub enum Action {
    Settings,
    Search,
    Refresh,
    OpenConfig,
    Show,
    New,
    NewFolder,
    Exit,
}

pub struct Tray {
    window: Window,
    _icon: Rc<Icon>,
}
impl Tray {
    pub fn new(
        mut appearance: impl FnMut() -> (luciddesk_core::PanelTheme, luciddesk_core::Backdrop, bool)
        + 'static,
        mut action: impl FnMut(Action) + 'static,
    ) -> Result<Self, String> {
        let icon = Rc::new(make_icon()?);
        let callback_icon = Rc::clone(&icon);
        let recreated = unsafe { RegisterWindowMessageW(windows_sys::w!("TaskbarCreated")) };
        if recreated == 0 {
            return Err(crate::i18n::text("ui-could-not-register-tray-recovery-message").into());
        }
        let mut pending = None;
        let window = Window::new("LucidDesk Tray")
            .size(1, 1)
            .style(WS_POPUP)
            .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)
            .on_message(move |raw, message, wp, lp| {
                if unsafe { crate::window_visibility::defer_show(message, lp, false) } {
                    return Some(0);
                }
                let hwnd = raw.cast();
                if message == WM_CLOSE {
                    action(Action::Exit);
                    return Some(0);
                }
                if message == crate::i18n::CHANGED {
                    unsafe {
                        Shell_NotifyIconW(NIM_MODIFY, &data(hwnd, callback_icon.0));
                    }
                    return Some(0);
                }
                if message == recreated {
                    // Explorer discards all notification icons when rebuilding
                    // its taskbar. This re-adds ours without restarting Explorer.
                    let _ = add(hwnd, callback_icon.0);
                    return Some(0);
                }
                if message == DISPATCH {
                    if let Some((event, anchor)) = pending.take() {
                        match event {
                            NIN_SELECT | NIN_KEYSELECT => action(Action::Show),
                            WM_CONTEXTMENU => {
                                // Read fresh settings after the synchronous Shell
                                // callback has returned, before pumping menu messages.
                                let (theme, backdrop, search_enabled) = appearance();
                                if let Some(command) =
                                    menu(hwnd, anchor, theme, backdrop, search_enabled)
                                {
                                    action(command);
                                }
                            }
                            _ => {}
                        }
                    }
                    return Some(0);
                }
                if message != CALLBACK {
                    return None;
                }
                if ((lp as u32) >> 16) != ICON_ID {
                    return Some(0);
                }
                let event = lp as u32 & 0xffff;
                // Explorer can send callbacks during FilterSession's synchronous
                // IPC, while PaneApp is borrowed. Never run actions or pump a
                // modal menu on that stack. Coalesce callbacks until dispatch.
                if matches!(event, NIN_SELECT | NIN_KEYSELECT | WM_CONTEXTMENU)
                    && pending.is_none()
                    && unsafe { PostMessageW(hwnd, DISPATCH, 0, 0) } != 0
                {
                    pending = Some((event, wp));
                }
                Some(0)
            })
            .create()
            .map_err(|e| e.to_string())?;
        add(window.hwnd().cast(), icon.0)?;
        Ok(Self {
            window,
            _icon: icon,
        })
    }
}
impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &data(self.window.hwnd().cast(), self._icon.0));
        }
    }
}

fn data(hwnd: HWND, icon: HICON) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ICON_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: CALLBACK,
        hIcon: icon,
        ..Default::default()
    };
    for (slot, ch) in data
        .szTip
        .iter_mut()
        .zip(crate::i18n::text("ui-luciddesk-desktop-organization").encode_utf16())
    {
        *slot = ch;
    }
    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    data
}
fn add(hwnd: HWND, icon: HICON) -> Result<(), String> {
    let data = data(hwnd, icon);
    unsafe {
        if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
            return Err(crate::i18n::text("ui-could-not-add-luciddesk-tray-icon").into());
        }
        if Shell_NotifyIconW(NIM_SETVERSION, &data) == 0 {
            Shell_NotifyIconW(NIM_DELETE, &data);
            return Err(crate::i18n::text("ui-could-not-initialize-tray-interaction").into());
        }
    }
    Ok(())
}

fn menu(
    hwnd: HWND,
    anchor: usize,
    theme: luciddesk_core::PanelTheme,
    backdrop: luciddesk_core::Backdrop,
    search_enabled: bool,
) -> Option<Action> {
    use crate::pane::menu::show_entries;
    use windows_sys::Win32::Foundation::POINT;

    // Version 4 packs signed screen coordinates in wParam.
    let mut position = POINT {
        x: i32::from(anchor as u16 as i16),
        y: i32::from((anchor >> 16) as u16 as i16),
    };
    unsafe {
        if position.x == -1 && position.y == -1 {
            let id = NOTIFYICONIDENTIFIER {
                cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
                hWnd: hwnd,
                uID: ICON_ID,
                ..Default::default()
            };
            let mut rect = windows_sys::Win32::Foundation::RECT::default();
            if Shell_NotifyIconGetRect(&id, &raw mut rect) >= 0 {
                position.x = rect.left + (rect.right - rect.left) / 2;
                position.y = rect.top;
            } else {
                GetCursorPos(&raw mut position);
            }
        }
        // The hidden owner must use the taskbar monitor's DPI for the flyout.
        SetWindowPos(
            hwnd,
            null_mut(),
            position.x,
            position.y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        SetForegroundWindow(hwnd);
    }
    let command = show_entries(
        hwnd,
        position,
        false,
        theme,
        backdrop,
        menu_entries(search_enabled),
    );
    unsafe {
        PostMessageW(hwnd, WM_NULL, 0, 0);
        Shell_NotifyIconW(NIM_SETFOCUS, &data(hwnd, null_mut()));
    }
    match command {
        1 => Some(Action::Show),
        2 => Some(Action::New),
        3 => Some(Action::Exit),
        4 => Some(Action::Settings),
        5 => Some(Action::NewFolder),
        6 => Some(Action::Search),
        7 => Some(Action::Refresh),
        8 => Some(Action::OpenConfig),
        _ => None,
    }
}
fn menu_entries(search_enabled: bool) -> Vec<crate::pane::menu::Entry> {
    use crate::pane::menu::entry;
    let mut rows = vec![entry(
        1,
        crate::i18n::text("ui-show-panels"),
        "\u{e737}",
        "",
    )];
    if search_enabled {
        rows.push(entry(6, crate::i18n::text("tray-search"), "\u{e721}", ""));
    }
    rows.extend([
        entry(7, crate::i18n::text("tray-refresh-all"), "\u{e72c}", ""),
        entry(0, "", "", ""),
        entry(2, crate::i18n::text("ui-new-group"), "\u{e710}", ""),
        entry(5, crate::i18n::text("ui-new-folder-panel"), "\u{e8b7}", ""),
        entry(0, "", "", ""),
        entry(4, crate::i18n::text("ui-settings"), "\u{e713}", ""),
        entry(8, crate::i18n::text("tray-open-config"), "\u{e8b7}", ""),
        entry(0, "", "", ""),
        entry(3, crate::i18n::text("ui-exit-luciddesk"), "\u{e7e8}", ""),
    ]);
    rows
}

fn make_icon() -> Result<Icon, String> {
    crate::app_icon::load(unsafe { GetSystemMetrics(SM_CXSMICON) }, unsafe {
        GetSystemMetrics(SM_CYSMICON)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    #[ignore = "Requires the interactive Windows notification area; run alone with --ignored"]
    fn native_close_requests_normal_exit_without_destroying_tray_early() {
        let exits = Rc::new(Cell::new(0));
        let observed = Rc::clone(&exits);
        let tray = Tray::new(
            || {
                (
                    luciddesk_core::PanelTheme::System,
                    luciddesk_core::Backdrop::Mica,
                    false,
                )
            },
            move |action| {
                if matches!(action, Action::Exit) {
                    observed.set(observed.get() + 1);
                }
            },
        )
        .unwrap();
        let hwnd = tray.window.hwnd().cast();
        unsafe {
            SendMessageW(hwnd, WM_CLOSE, 0, 0);
        }
        assert_eq!(exits.get(), 1);
        assert_ne!(
            unsafe { IsWindow(hwnd) },
            0,
            "Tray stays alive until normal application teardown"
        );
    }

    #[test]
    fn common_actions_follow_search_setting_and_exit_is_separate() {
        for enabled in [false, true] {
            let rows = menu_entries(enabled);
            assert_eq!(
                rows.iter().filter(|r| r.id == 6).count(),
                usize::from(enabled)
            );
            for id in [1, 2, 3, 4, 5, 7, 8] {
                assert_eq!(rows.iter().filter(|r| r.id == id).count(), 1);
            }
            assert_eq!(rows.last().unwrap().id, 3);
            assert_eq!(rows[rows.len() - 2].id, 0);
        }
    }

    #[test]
    #[ignore = "Requires the interactive Windows notification area; run with --ignored"]
    fn tray_registers_handles_keyboard_selection_readds_and_removes() {
        let calls = Rc::new(Cell::new(0));
        let observed = Rc::clone(&calls);
        let tray = Tray::new(
            || {
                (
                    luciddesk_core::PanelTheme::System,
                    luciddesk_core::Backdrop::Mica,
                    false,
                )
            },
            move |action| {
                if matches!(action, Action::Show) {
                    observed.set(observed.get() + 1);
                }
            },
        )
        .unwrap();
        let hwnd = tray.window.hwnd().cast();
        let id = NOTIFYICONIDENTIFIER {
            cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: hwnd,
            uID: ICON_ID,
            ..Default::default()
        };
        let mut rect = windows_sys::Win32::Foundation::RECT::default();
        unsafe {
            assert!(Shell_NotifyIconGetRect(&id, &raw mut rect) >= 0);
            SendMessageW(
                hwnd,
                CALLBACK,
                0,
                ((ICON_ID << 16) | NIN_KEYSELECT) as isize,
            );
            assert_eq!(
                calls.get(),
                0,
                "Shell callbacks must defer application work"
            );
            dispatch_pending(hwnd);
            assert_eq!(calls.get(), 1);
            SendMessageW(hwnd, CALLBACK, 0, ((2 << 16) | NIN_SELECT) as isize);
            assert_eq!(calls.get(), 1, "Another icon's callback must be ignored");
            Shell_NotifyIconW(NIM_DELETE, &data(hwnd, tray._icon.0));
            let recreated = RegisterWindowMessageW(windows_sys::w!("TaskbarCreated"));
            SendMessageW(hwnd, recreated, 0, 0);
            assert!(Shell_NotifyIconGetRect(&id, &raw mut rect) >= 0);
        }
        drop(tray);
        assert!(unsafe { Shell_NotifyIconGetRect(&id, &raw mut rect) } < 0);
    }

    fn dispatch_pending(hwnd: HWND) {
        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(&raw mut message, hwnd, DISPATCH, DISPATCH, PM_REMOVE) != 0 {
                DispatchMessageW(&raw const message);
            }
        }
    }

    #[test]
    #[ignore = "Requires the interactive Windows notification area; run with --ignored"]
    fn sent_clicks_defer_until_state_is_released_and_coalesce() {
        let state = Rc::new(std::cell::RefCell::new(0));
        let observed = Rc::clone(&state);
        let tray = Tray::new(
            || {
                (
                    luciddesk_core::PanelTheme::System,
                    luciddesk_core::Backdrop::Mica,
                    false,
                )
            },
            move |_| *observed.borrow_mut() += 1,
        )
        .unwrap();
        let hwnd = tray.window.hwnd().cast();
        for _ in 0..10 {
            // Model a Shell callback delivered while synchronous Hook IPC
            // has the application's state borrowed on the same UI thread.
            let borrowed = state.borrow_mut();
            for event in [NIN_SELECT, NIN_KEYSELECT, NIN_SELECT, NIN_KEYSELECT] {
                unsafe {
                    SendMessageW(hwnd, CALLBACK, 0, ((ICON_ID << 16) | event) as isize);
                }
            }
            drop(borrowed);
            dispatch_pending(hwnd);
        }
        assert_eq!(*state.borrow(), 10);
    }
}
