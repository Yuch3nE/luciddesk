//! Shared settings controls. Appearance is independent of the action a page binds.
use super::{Action, Rect, Scene};

thread_local! { static OPEN_DROPDOWN: std::cell::Cell<Option<Rect>> = const { std::cell::Cell::new(None) }; }
pub(super) fn dropdown_is_open(bounds: Rect) -> bool {
    // Identity of the same immutable scene rectangle, rather than a distance test.
    let key = |r: Rect| {
        [
            r.left.to_bits(),
            r.top.to_bits(),
            r.right.to_bits(),
            r.bottom.to_bits(),
        ]
    };
    OPEN_DROPDOWN.with(|open| open.get().is_some_and(|r| key(r) == key(bounds)))
}
struct OpenDropdown(windows_sys::Win32::Foundation::HWND);
impl OpenDropdown {
    fn new(hwnd: windows_sys::Win32::Foundation::HWND, bounds: Rect) -> Self {
        OPEN_DROPDOWN.with(|open| open.set(Some(bounds)));
        unsafe {
            windows_sys::Win32::Graphics::Gdi::InvalidateRect(hwnd, std::ptr::null(), 0);
        }
        Self(hwnd)
    }
}
impl Drop for OpenDropdown {
    fn drop(&mut self) {
        OPEN_DROPDOWN.with(|open| open.set(None));
        unsafe {
            windows_sys::Win32::Graphics::Gdi::InvalidateRect(self.0, std::ptr::null(), 0);
        }
    }
}

fn screen_anchor(
    hwnd: windows_sys::Win32::Foundation::HWND,
    bounds: Rect,
    scale: f32,
) -> windows_sys::Win32::Foundation::RECT {
    let mut point = windows_sys::Win32::Foundation::POINT {
        x: (bounds.left * scale).round() as i32,
        y: (bounds.top * scale).round() as i32,
    };
    unsafe {
        windows_sys::Win32::Graphics::Gdi::ClientToScreen(hwnd, &raw mut point);
    }
    windows_sys::Win32::Foundation::RECT {
        left: point.x,
        top: point.y,
        right: point.x + ((bounds.right - bounds.left) * scale).round() as i32,
        bottom: point.y + ((bounds.bottom - bounds.top) * scale).round() as i32,
    }
}

pub(super) struct Style;
impl Style {
    pub const ICON: f32 = 12.0;
    pub const NAV_ICON: f32 = 16.0;
    pub const NAV_TEXT: f32 = 14.0;
    pub const NAV_HEADING: f32 = 11.0;
    pub const NAV_HEIGHT: f32 = 40.0;
    pub const NAV_ICON_SLOT: f32 = 20.0;
    pub const ICON_SLOT: f32 = 16.0;
    pub const ICON_GAP: f32 = 8.0;
    pub const NAV_TEXT_INSET: f32 = 48.0;
    pub const ROW_INSET: f32 = 16.0;
    pub const ROW_HEIGHT: f32 = 38.0;
    pub const CONTROL_HEIGHT: f32 = super::super::control_style::HEIGHT;
    pub const TEXT_SIZE: f32 = super::super::control_style::TEXT;
    pub const RADIUS: f32 = super::super::control_style::RADIUS;
    pub const SLIDER_INSET: f32 = 8.0;
}

#[derive(Clone, Copy)]
pub(super) struct Slider {
    pub value: f32,
    pub max: f32,
    pub centered: bool,
    pub channel: Option<u8>,
}
impl Slider {
    pub fn linear(value: f32, max: f32) -> Self {
        Self {
            value,
            max,
            centered: false,
            channel: None,
        }
    }
    pub fn centered(value: f32, max: f32) -> Self {
        Self {
            centered: true,
            ..Self::linear(value, max)
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum ControlKind {
    Button,
    BackButton,
    Row,
    ForwardRow,
    FooterLink,
    Navigation,
    Caption,
    Toggle,
    Checkbox,
    Slider(Slider),
    Combo,
}
impl ControlKind {
    pub fn is_row(self) -> bool {
        matches!(self, Self::Row | Self::ForwardRow)
    }
    pub fn is_back(self) -> bool {
        matches!(self, Self::BackButton)
    }
}

#[derive(Clone)]
pub(super) struct Control {
    pub bounds: Rect,
    pub label: String,
    pub action: Action,
    pub selected: bool,
    pub kind: ControlKind,
    pub enabled: bool,
}
impl Control {
    pub fn is_toggle(&self) -> bool {
        matches!(self.kind, ControlKind::Toggle)
    }

    pub fn same_toggle(&self, other: &Self) -> bool {
        if !self.is_toggle() || !other.is_toggle()
            || (self.bounds.left, self.bounds.top, self.bounds.right, self.bounds.bottom)
                != (other.bounds.left, other.bounds.top, other.bounds.right, other.bounds.bottom)
        { return false; }
        match (&self.action, &other.action) {
            (Action::Change(a), Action::Change(b)) => std::mem::discriminant(a) == std::mem::discriminant(b),
            (Action::PluginEnabled(a, _), Action::PluginEnabled(b, _)) => a == b,
            (Action::BackupPolicy(a), Action::BackupPolicy(b)) => a == b,
            (a, b) => std::mem::discriminant(a) == std::mem::discriminant(b),
        }
    }
}

impl Scene {
    pub fn text(&mut self, r: Rect, text: impl Into<String>, size: usize) {
        self.text.push((r, text.into(), size));
    }
    pub fn control(
        &mut self,
        kind: ControlKind,
        bounds: Rect,
        label: &str,
        action: Action,
        selected: bool,
    ) {
        self.controls.push(Control {
            bounds,
            label: label.into(),
            action,
            selected,
            kind,
            enabled: true,
        });
    }
    pub fn button(&mut self, bounds: Rect, label: &str, action: Action, selected: bool) {
        self.control(ControlKind::Button, bounds, label, action, selected);
    }
    pub fn toggle(&mut self, bounds: Rect, action: Action, selected: bool, enabled: bool) {
        self.controls.push(Control {
            bounds,
            label: String::new(),
            action,
            selected,
            enabled,
            kind: ControlKind::Toggle,
        });
    }
    /// The full labeled row is interactive, not just the check mark.
    pub fn checkbox(&mut self, bounds: Rect, label: &str, action: Action, checked: bool, enabled: bool) {
        self.controls.push(Control {
            bounds, label: label.into(), action, selected: checked,
            enabled, kind: ControlKind::Checkbox,
        });
    }
    pub fn slider(&mut self, bounds: Rect, value: Slider, action: Action) {
        self.control(ControlKind::Slider(value), bounds, "", action, false);
    }
    pub fn row(&mut self, x: f32, y: f32, width: f32, label: &str, action: Action) {
        self.control(
            ControlKind::Row,
            Rect::from_xywh(x, y, width, Style::ROW_HEIGHT),
            label,
            action,
            false,
        );
    }
    pub fn forward_row(&mut self, x: f32, y: f32, width: f32, label: &str, action: Action) {
        self.control(
            ControlKind::ForwardRow,
            Rect::from_xywh(x, y, width, Style::ROW_HEIGHT),
            label,
            action,
            false,
        );
    }
}

pub(super) fn slider_fraction(bounds: Rect, x: f32) -> f32 {
    ((x - bounds.left - Style::SLIDER_INSET)
        / (bounds.right - bounds.left - 2.0 * Style::SLIDER_INSET).max(1.0))
    .clamp(0.0, 1.0)
}

pub(super) fn centered_icon_left(bounds: Rect, label_width: f32) -> f32 {
    let width = Style::ICON_SLOT + Style::ICON_GAP + label_width;
    ((bounds.left + bounds.right - width) / 2.0).max(bounds.left + Style::ICON_GAP)
}

/// Uses the shared popup for all value selectors; dismissal never becomes a value.
pub(super) fn choose(
    hwnd: windows_sys::Win32::Foundation::HWND,
    bounds: Rect,
    appearance: (luciddesk_core::PanelTheme, luciddesk_core::Backdrop),
    options: &[(u64, &'static str)],
    selected: u64,
) -> Option<u64> {
    let rows = options
        .iter()
        .enumerate()
        .map(|(index, (value, label))| {
            super::super::menu::entry(
                index as i32 + 1,
                *label,
                if *value == selected { "\u{e73e}" } else { "" },
                "",
            )
        })
        .collect();
    let scale =
        unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
    let anchor = screen_anchor(hwnd, bounds, scale);
    let _open = OpenDropdown::new(hwnd, bounds);
    let current = options.iter().position(|(value, _)| *value == selected);
    let command = super::super::dropdown::show(hwnd, anchor, appearance, rows, current)
        .and_then(|index| i32::try_from(index + 1).ok())
        .unwrap_or(0);
    choice_value(options, command)
}

fn choice_value(options: &[(u64, &'static str)], command: i32) -> Option<u64> {
    usize::try_from(command.checked_sub(1)?)
        .ok()
        .and_then(|index| options.get(index))
        .map(|(value, _)| *value)
}

/// Open after the settings callback returns: the popup runs its own message loop.
pub(super) fn open_dropdown(
    hwnd: windows_sys::Win32::Foundation::HWND,
    appearance: (luciddesk_core::PanelTheme, luciddesk_core::Backdrop),
    scale: f32,
    control: &Control,
    sender: std::sync::mpsc::Sender<Control>,
) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow, PostMessageW};
    let mut control = control.clone();
    let Action::Dropdown(options) = &control.action else {
        return;
    };
    let options = std::sync::Arc::clone(options);
    let anchor = screen_anchor(hwnd, control.bounds, scale);
    super::window::defer_action(move || {
        if unsafe { IsWindow(hwnd) } == 0 {
            return;
        }
        let rows = options
            .iter()
            .enumerate()
            .map(|(index, (label, _, selected))| {
                super::super::menu::entry(
                    index as i32 + 1,
                    label.clone(),
                    if *selected { "\u{e73e}" } else { "" },
                    "",
                )
            })
            .collect();
        let _open = OpenDropdown::new(hwnd, control.bounds);
        let current = options.iter().position(|(_, _, selected)| *selected);
        let command = super::super::dropdown::show(hwnd, anchor, appearance, rows, current)
            .and_then(|index| i32::try_from(index + 1).ok())
            .unwrap_or(0);
        if let Some(action) = dropdown_action(&options, command) {
            control.action = action;
            if sender.send(control).is_ok() {
                unsafe {
                    PostMessageW(hwnd, super::SELECT_SETTING, 0, 0);
                }
            }
        }
    });
}

fn dropdown_action(options: &[(String, Action, bool)], command: i32) -> Option<Action> {
    usize::try_from(command.checked_sub(1)?)
        .ok()
        .and_then(|index| options.get(index))
        .filter(|(_, _, selected)| !selected)
        .map(|(_, action, _)| action.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropdown_dismissal_and_unknown_commands_do_not_select() {
        let options = [(5, "five"), (15, "fifteen")];
        assert_eq!(choice_value(&options, 0), None);
        assert_eq!(choice_value(&options, -1), None);
        assert_eq!(choice_value(&options, 3), None);
        assert_eq!(choice_value(&options, 2), Some(15));
    }

    #[test]
    fn dropdown_only_dispatches_a_changed_valid_option() {
        let options = vec![
            ("Current".into(), Action::Page(1), true),
            ("Next".into(), Action::Page(2), false),
        ];
        for command in [i32::MIN, -1, 0, 1, 3, i32::MAX] {
            assert!(dropdown_action(&options, command).is_none());
        }
        assert!(matches!(
            dropdown_action(&options, 2),
            Some(Action::Page(2))
        ));
        assert!(dropdown_action(&[], 1).is_none());
    }

    #[test]
    fn cloning_dropdown_controls_shares_options_and_plugin_payloads() {
        let action = Action::Dropdown(vec![("Device".into(), Action::Page(1), true)].into());
        let cloned = action.clone();
        let (Action::Dropdown(first), Action::Dropdown(second)) = (&action, &cloned) else {
            unreachable!()
        };
        assert!(std::sync::Arc::ptr_eq(first, second));
    }

    #[test]
    fn icon_text_group_centers_different_label_widths_and_clamps_overflow() {
        let bounds = Rect::from_xywh(10.0, 0.0, 180.0, 32.0);
        for label_width in [28.0, 84.0, 112.0] {
            let left = centered_icon_left(bounds, label_width);
            let right = left + Style::ICON_SLOT + Style::ICON_GAP + label_width;
            assert!(((left + right) / 2.0 - 100.0).abs() < 0.01);
        }
        assert_eq!(centered_icon_left(bounds, 400.0), 18.0);
        assert_eq!(slider_fraction(bounds, -100.0), 0.0);
        assert_eq!(slider_fraction(bounds, 100.0), 0.5);
        assert_eq!(slider_fraction(bounds, 400.0), 1.0);
    }
}
