//! Optional global reveal shortcut, independent of Everything search.
use super::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
pub(super) const ID: i32 = 0x4c51;
thread_local! { static STATUS: RefCell<String> = const { RefCell::new(String::new()) }; }

pub(super) fn default_shortcut() -> Shortcut { Shortcut { key: 0x44, modifiers: 3 } }
pub(super) fn enabled(store: &WorkspaceStore) -> bool {
    store.preference("show_panels_enabled").ok().flatten().as_deref() == Some("1")
}
pub(super) fn settings(store: &WorkspaceStore) -> Shortcut {
    store.preference("show_panels_hotkey").ok().flatten()
        .and_then(|raw| decode(&raw)).unwrap_or_else(default_shortcut)
}
pub(super) fn save(store: &WorkspaceStore, value: Shortcut) -> Result<(), String> {
    if !valid(value) {
        return Err(crate::i18n::text("ui-use-ctrl-or-alt-with-a-letter-digit-space-or-function-key-avoid-exis").into());
    }
    store.save_preference("show_panels_hotkey", &format!("{}:{}", value.key, value.modifiers))
        .map_err(|e| e.to_string())
}
pub(super) fn status() -> String { STATUS.with(|s| s.borrow().clone()) }
pub(super) fn update_status(value: String) { STATUS.with(|s| *s.borrow_mut() = value); }
pub(super) fn activate(state: &Rc<RefCell<PaneApp>>) {
    if enabled(&state.borrow().store) { quick_reveal::show_all(state); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    #[test]
    fn disabled_does_nothing_and_enabled_reveals_every_panel_type() {
        let mut app = super::super::tests::test_state();
        for n in 10..13 {
            let id = PanelId::new(n);
            let mut panel = Panel::new(id, format!("Panel {n}"), RectDip::default());
            if n == 11 { panel.set_folder(Some(std::env::temp_dir())); }
            if n == 12 { panel.set_search(true); }
            app.workspace.add_panel(panel).unwrap();
            let model = Rc::new(RefCell::new(create_model(&app, id).unwrap()));
            let window = windows_window::Window::new("Reveal shortcut test")
                .style(WS_POPUP).size(100, 100).create().unwrap();
            super::super::window::set_layer(window.hwnd().cast(), n == 11);
            unsafe { ShowWindow(window.hwnd().cast(), SW_HIDE); }
            app.views.push(View { id, target: Rc::new(std::cell::Cell::new(id)), window, model });
        }
        let state = Rc::new(RefCell::new(app));
        activate(&state);
        for view in &state.borrow().views { assert_eq!(unsafe { IsWindowVisible(view.window.hwnd().cast()) }, 0); }
        state.borrow().store.save_preference("show_panels_enabled", "1").unwrap();
        activate(&state);
        for (i, view) in state.borrow().views.iter().enumerate() {
            assert_ne!(unsafe { IsWindowVisible(view.window.hwnd().cast()) }, 0);
            assert_eq!(quick_reveal::permanent_topmost(view.window.hwnd().cast()), i == 1);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::pane) struct Shortcut {
    pub key: u16,
    pub modifiers: u8,
}
pub(in crate::pane) fn valid(value: Shortcut) -> bool {
    // Require Ctrl or Alt; reserve system/menu combinations and the debugger's F12.
    value.modifiers <= 7
        && value.modifiers & 5 != 0
        && value.key != VK_F12
        && peek::valid_shortcut(value.key, value.modifiers)
}
pub(in crate::pane) fn decode(raw: &str) -> Option<Shortcut> {
    let (key, modifiers) = raw.split_once(':')?;
    let value = Shortcut {
        key: key.parse().ok()?,
        modifiers: modifiers.parse().ok()?,
    };
    valid(value).then_some(value)
}
pub(in crate::pane) fn label(value: Shortcut) -> String {
    peek::shortcut_label(&peek::Settings {
        key: value.key,
        modifiers: value.modifiers,
        ..Default::default()
    })
}
fn flags(value: Shortcut) -> u32 {
    MOD_NOREPEAT
        | if value.modifiers & 1 != 0 {
            MOD_CONTROL
        } else {
            0
        }
        | if value.modifiers & 2 != 0 {
            MOD_SHIFT
        } else {
            0
        }
        | if value.modifiers & 4 != 0 { MOD_ALT } else { 0 }
}
pub(in crate::pane) struct Registration {
    id: i32,
    hwnd: isize,
    desired: Option<Shortcut>,
    registered: bool,
    attempted: Option<std::time::Instant>,
}
impl Default for Registration {
    fn default() -> Self { Self::with_id(ID) }
}
impl Registration {
    pub fn with_id(id: i32) -> Self {
        Self { id, hwnd: 0, desired: None, registered: false, attempted: None }
    }
    pub fn ready(&self) -> bool { self.desired.is_none() || self.registered }
    pub fn message(&self) -> String {
        if self.desired.is_none() {
            crate::i18n::text("show-panels-disabled").into()
        } else if self.registered {
            crate::i18n::text("ui-works-globally-esc-cancels-recording").into()
        } else {
            crate::i18n::format("ui-is-unavailable-choose-another-shortcut", &[("arg0", label(self.desired.unwrap()))])
        }
    }

    pub fn retry_deadline(&self) -> Option<std::time::Instant> {
        self.attempted.filter(|_| self.desired.is_some() && !self.registered)
            .map(|time| time + std::time::Duration::from_secs(10))
    }
    pub fn update(&mut self, hwnd: isize, desired: Option<Shortcut>) {
        if self.desired == desired
            && (self.registered
                || desired.is_none()
                || self.attempted.is_some_and(|t| t.elapsed().as_secs() < 10))
        {
            return;
        }
        self.clear();
        self.hwnd = hwnd;
        self.desired = desired;
        self.attempted = Some(std::time::Instant::now());
        self.registered = desired.is_some_and(|value| unsafe { RegisterHotKey(hwnd as _, self.id, flags(value), u32::from(value.key)) } != 0);
    }
    fn clear(&mut self) {
        if self.registered {
            unsafe {
                UnregisterHotKey(self.hwnd as _, self.id);
            }
        }
        self.registered = false;
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod registration_tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{WS_POPUP, WM_DESTROY};
    #[test]
    fn native_registration_reports_conflicts_and_releases_disabled_binding() {
        let window = windows_window::Window::new("Hotkey test")
            .style(WS_POPUP)
            .on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0))
            .create()
            .unwrap();
        let hwnd = window.hwnd() as isize;
        let shortcut = (VK_F13..=VK_F24).filter(|key| *key != VK_F12).map(|key| Shortcut { key, modifiers: 7 }).find(|value| unsafe { RegisterHotKey(hwnd as _, ID + 1, flags(*value), u32::from(value.key)) } != 0).expect("available test binding");
        let mut registration = Registration::default();
        registration.update(hwnd, Some(shortcut));
        assert!(
            !registration.registered,
            "must not steal an existing binding"
        );
        unsafe {
            UnregisterHotKey(hwnd as _, ID + 1);
        }
        registration.update(hwnd, None);
        registration.update(hwnd, Some(shortcut));
        assert!(registration.registered);
        registration.update(hwnd, None);
        assert_ne!(
            unsafe { RegisterHotKey(hwnd as _, ID + 1, flags(shortcut), u32::from(shortcut.key)) },
            0
        );
        unsafe {
            UnregisterHotKey(hwnd as _, ID + 1);
        }
    }
    #[test]
    fn independent_registrations_release_only_their_own_binding() {
        let window = windows_window::Window::new("Independent shortcuts")
            .style(WS_POPUP).create().unwrap();
        let hwnd = window.hwnd() as isize;
        let mut first = Registration::default();
        let mut second = Registration::with_id(ID + 2);
        let available: Vec<_> = (VK_F13..=VK_F24).map(|key| Shortcut { key, modifiers: 7 })
            .filter(|value| unsafe {
                if RegisterHotKey(hwnd as _, ID + 8, flags(*value), u32::from(value.key)) == 0 { return false; }
                UnregisterHotKey(hwnd as _, ID + 8); true
            }).take(2).collect();
        assert_eq!(available.len(), 2);
        first.update(hwnd, Some(available[0]));
        second.update(hwnd, Some(available[1]));
        assert!(first.registered && second.registered);
        second.update(hwnd, None);
        assert!(first.registered);
        assert_eq!(unsafe { RegisterHotKey(hwnd as _, ID + 8, flags(available[0]), u32::from(available[0].key)) }, 0);
        assert_ne!(unsafe { RegisterHotKey(hwnd as _, ID + 8, flags(available[1]), u32::from(available[1].key)) }, 0);
        unsafe { UnregisterHotKey(hwnd as _, ID + 8); }
    }

    #[test]
    fn shortcut_persists_and_reserves_system_and_file_commands() {
        let store = WorkspaceStore::open_in_memory().unwrap();
        save(&store, default_shortcut()).unwrap();
        assert_eq!(settings(&store), default_shortcut());
        assert_eq!(label(settings(&store)), "Ctrl + Shift + D");
        for value in [
            Shortcut {
                key: VK_F12,
                modifiers: 3,
            },
            Shortcut {
                key: VK_SPACE,
                modifiers: 4,
            },
            Shortcut {
                key: 0x43,
                modifiers: 1,
            },
            Shortcut {
                key: 0x41,
                modifiers: 0,
            },
        ] {
            assert!(save(&store, value).is_err());
        }
        assert_eq!(decode("broken"), None);
        assert_eq!(
            flags(default_shortcut()),
            MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT
        );
    }
}
