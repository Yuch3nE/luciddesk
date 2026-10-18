//! Native monitor content, hosted with its sampling worker in a dedicated process.
pub(super) mod process;
mod drawing;
mod sampling;
#[cfg(test)]
mod tests;
use super::{Event, GroupModel};
pub(super) use drawing::Drawing;
#[cfg(test)]
use luciddesk_core::PanelId;
pub(super) use sampling::Client;
pub(super) const PLUGIN_ID: &str = "system-monitor";
pub(super) fn available() -> bool {
    crate::plugins::supports(PLUGIN_ID, "system-monitor.v1")
}
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, rc::Rc};
use windows_sys::Win32::Foundation::{HWND, POINT};

pub(super) const TIMER: usize = 71;

/// Layout changes run after the menu callback releases application/model borrows.
pub(super) fn resize_layout(hwnd: HWND, compact: bool) -> Option<luciddesk_core::RectDip> {
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        },
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                GetWindowRect, IsWindow, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
            },
        },
    };
    unsafe {
        if IsWindow(hwnd) == 0 {
            return None;
        }
        let mut current = RECT::default();
        let mut monitor = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetWindowRect(hwnd, &raw mut current) == 0
            || GetMonitorInfoW(
                MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                &raw mut monitor,
            ) == 0
        {
            return None;
        }
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let (width, height) = if compact {
            (320.0, 280.0)
        } else {
            (360.0, 600.0)
        };
        let work = monitor.rcWork;
        let width = ((width * scale).round() as i32)
            .min(work.right - work.left)
            .max(1);
        let height = ((height * scale).round() as i32)
            .min(work.bottom - work.top)
            .max(1);
        let x = current.left.clamp(work.left, work.right - width);
        let y = current.top.clamp(work.top, work.bottom - height);
        if SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        ) == 0
        {
            return None;
        }
        Some(luciddesk_core::RectDip::from_bounds(
            x as f32 / scale,
            y as f32 / scale,
            width as f32 / scale,
            height as f32 / scale,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Metric {
    Cpu,
    Memory,
    Gpu,
    Network,
    Disk,
}
impl Metric {
    #[cfg(test)]
    pub(super) fn label(self) -> &'static str {
        crate::i18n::text(match self {
            Self::Cpu => "monitor-cpu",
            Self::Memory => "monitor-memory",
            Self::Gpu => "monitor-gpu",
            Self::Network => "monitor-network",
            Self::Disk => "monitor-disk",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Preferences {
    pub enabled: bool,
    pub metrics: Vec<Metric>,
    pub compact: bool,
    pub paused: bool,
    pub interval: u64,
    pub network: Option<String>,
    pub disk: Option<String>,
    pub gpu: Option<String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            enabled: true,
            metrics: vec![
                Metric::Cpu,
                Metric::Memory,
                Metric::Gpu,
                Metric::Network,
                Metric::Disk,
            ],
            compact: false,
            paused: false,
            interval: 1,
            network: None,
            disk: None,
            gpu: None,
        }
    }
}
impl Preferences {
    pub(super) fn load() -> Result<Self, String> {
        let config = crate::plugins::request(PLUGIN_ID, serde_json::json!({"op":"config_get"}))?;
        serde_json::from_value(config["options"].clone()).map_err(|e| e.to_string())
    }
    pub(super) fn save(&self) -> Result<(), String> {
        crate::plugins::request(PLUGIN_ID, serde_json::json!({"op":"save_options", "options":self})).map(|_| ())
    }
    pub(super) fn minimum(&self, collapsed: bool) -> (f32, f32) {
        (
            300.0,
            if collapsed {
                super::layout::HEADER
            } else if self.compact {
                240.0
            } else {
                400.0
            },
        )
    }
}

pub(super) fn show_menu(
    hwnd: HWND,
    anchor: POINT,
    anchored: bool,
    model: &Rc<RefCell<GroupModel>>,
    mut event: impl FnMut(Event),
) {
    use super::menu::{entry, show_entries};
    let (theme, backdrop, locked, collapsed, hide) = {
        let m = model.borrow();
        (m.theme, m.backdrop, m.locked, m.collapsed, m.auto_hide)
    };
    let check = |yes| if yes { "\u{e73e}" } else { "" };
    let rows = vec![
        entry(18, crate::i18n::text("monitor-settings"), "", ""),
        entry(0, "", "", ""),
        entry(
            48,
            crate::i18n::text(if collapsed {
                "ui-expand-panel"
            } else {
                "ui-collapse-panel"
            }),
            "",
            "",
        ),
        entry(7, crate::i18n::text("ui-auto-collapse"), check(hide), ""),
        entry(10, crate::i18n::text("ui-lock-panel"), check(locked), ""),
        entry(
            12,
            crate::i18n::text("ui-always-on-top"),
            check(super::quick_reveal::permanent_topmost(hwnd)),
            "",
        ),
    ];
    match show_entries(hwnd, anchor, anchored, theme, backdrop, rows) {
        18 => event(Event::Settings),
        48 => event(Event::Collapse),
        7 => event(Event::ToggleAutoHide),
        10 => event(Event::ToggleLocked),
        12 => event(Event::ToggleTopmost),
        _ => {}
    }
}

pub(super) fn devices(metric: Metric) -> Vec<(String, String)> {
    let index = match metric {
        Metric::Network => 0,
        Metric::Disk => 1,
        Metric::Gpu => 2,
        _ => return Vec::new(),
    };
    crate::plugins::request(PLUGIN_ID, serde_json::json!({"op":"devices"}))
        .ok()
        .and_then(|v| serde_json::from_value(v[index].clone()).ok())
        .unwrap_or_default()
}
