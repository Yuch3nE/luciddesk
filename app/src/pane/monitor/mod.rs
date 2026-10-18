//! Native monitor content. All windows share one demand-driven sampling worker.
mod drawing;
mod gpu;
mod sampling;
#[cfg(test)]
mod tests;
use super::{Event, GroupModel};
pub(super) use drawing::Drawing;
use luciddesk_core::PanelId;
use luciddesk_storage::WorkspaceStore;
pub(super) use sampling::Client;
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
    fn label(self) -> &'static str {
        crate::i18n::text(match self {
            Self::Cpu => "monitor-cpu",
            Self::Memory => "monitor-memory",
            Self::Gpu => "monitor-gpu",
            Self::Network => "monitor-network",
            Self::Disk => "monitor-disk",
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Preferences {
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
    pub(super) fn load(store: &WorkspaceStore, id: PanelId) -> Result<Self, String> {
        let raw = store
            .preference(&format!("monitor:{}", id.get()))
            .map_err(|e| e.to_string())?;
        let mut value = raw
            .map(|s| serde_json::from_str::<Self>(&s))
            .transpose()
            .map_err(|e| format!("Invalid monitor preferences: {e}"))?
            .unwrap_or_default();
        let mut unique = Vec::new();
        value.metrics.retain(|m| {
            if unique.contains(m) {
                false
            } else {
                unique.push(*m);
                true
            }
        });
        if value.metrics.is_empty() {
            value.metrics = Self::default().metrics;
        }
        if ![1, 2, 5].contains(&value.interval) {
            value.interval = 1;
        }
        Ok(value)
    }
    pub(super) fn save(&self, store: &WorkspaceStore, id: PanelId) -> Result<(), String> {
        store
            .save_preference(
                &format!("monitor:{}", id.get()),
                &serde_json::to_string(self).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())
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
    let (mut options, theme, backdrop, locked, collapsed, hide) = {
        let m = model.borrow();
        (
            m.monitor.clone().unwrap(),
            m.theme,
            m.backdrop,
            m.locked,
            m.collapsed,
            m.auto_hide,
        )
    };
    let check = |yes| if yes { "\u{e73e}" } else { "" };
    let mut rows = vec![
        entry(
            100,
            crate::i18n::text("monitor-compact"),
            check(options.compact),
            "",
        ),
        entry(
            101,
            crate::i18n::text(if options.paused {
                "monitor-resume"
            } else {
                "monitor-pause"
            }),
            "",
            "",
        ),
    ];
    let all = [
        Metric::Cpu,
        Metric::Memory,
        Metric::Gpu,
        Metric::Network,
        Metric::Disk,
    ];
    let mut metrics = entry(102, crate::i18n::text("monitor-metrics"), "", "");
    metrics.children = all
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let mut row = entry(
                110 + i as i32,
                m.label(),
                check(options.metrics.contains(m)),
                "",
            );
            row.enabled = !options.metrics.contains(m) || options.metrics.len() > 1;
            row
        })
        .collect();
    rows.push(metrics);
    let mut order = entry(103, crate::i18n::text("monitor-first"), "", "");
    order.children = options
        .metrics
        .iter()
        .map(|m| {
            entry(
                120 + all.iter().position(|x| x == m).unwrap() as i32,
                m.label(),
                "",
                "",
            )
        })
        .collect();
    rows.push(order);
    let mut interval = entry(104, crate::i18n::text("monitor-interval"), "", "");
    interval.children = [
        (1, "monitor-one-second"),
        (2, "monitor-two-seconds"),
        (5, "monitor-five-seconds"),
    ]
    .into_iter()
    .map(|(n, key)| {
        entry(
            130 + n as i32,
            crate::i18n::text(key),
            check(options.interval == n),
            "",
        )
    })
    .collect();
    rows.push(interval);
    let snapshot = sampling::snapshot();
    // Entry labels borrow the persistent i18n catalog. Device names are dynamic;
    // the native popup below supports them without leaking strings into that catalog.
    rows.extend([
        entry(140, crate::i18n::text("monitor-select-network"), "", ""),
        entry(141, crate::i18n::text("monitor-select-disk"), "", ""),
        entry(142, crate::i18n::text("monitor-select-gpu"), "", ""),
        entry(143, crate::i18n::text("monitor-task-manager"), "", ""),
        entry(0, "", "", ""),
        entry(43, crate::i18n::text("ui-rename-panel"), "", ""),
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
        entry(0, "", "", ""),
        entry(18, crate::i18n::text("ui-settings"), "", ""),
        entry(11, crate::i18n::text("ui-close-panel"), "", ""),
    ]);
    let command = show_entries(hwnd, anchor, anchored, theme, backdrop, rows);
    match command {
        100 => options.compact = !options.compact,
        101 => options.paused = !options.paused,
        110..=114 => {
            let metric = all[(command - 110) as usize];
            if options.metrics.contains(&metric) {
                if options.metrics.len() > 1 {
                    options.metrics.retain(|m| *m != metric);
                }
            } else {
                options.metrics.push(metric);
            }
        }
        120..=124 => {
            let metric = all[(command - 120) as usize];
            options.metrics.retain(|m| *m != metric);
            options.metrics.insert(0, metric);
        }
        131 | 132 | 135 => options.interval = (command - 130) as u64,
        140..=142 => {
            let names: Vec<_> = match command {
                140 => snapshot
                    .networks
                    .iter()
                    .map(|d| (d.id.clone(), d.name.clone()))
                    .collect(),
                141 => snapshot
                    .disks
                    .iter()
                    .map(|d| (d.id.clone(), d.name.clone()))
                    .collect(),
                _ => snapshot
                    .gpus
                    .iter()
                    .map(|d| (d.id.clone(), d.name.clone()))
                    .collect(),
            };
            let selected = match command {
                140 => &options.network,
                141 => &options.disk,
                _ => &options.gpu,
            };
            let Some(selected) = device_menu(hwnd, anchor, &names, selected.as_deref()) else {
                return;
            };
            match command {
                140 => options.network = selected,
                141 => options.disk = selected,
                _ => options.gpu = selected,
            }
        }
        143 => {
            let task_manager = std::path::PathBuf::from(
                std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
            )
            .join("System32/taskmgr.exe");
            if let Err(e) = std::process::Command::new(task_manager).spawn() {
                super::window::error(&e.to_string());
            }
            return;
        }
        43 if !locked => {
            event(Event::RenameTitle);
            return;
        }
        48 => {
            event(Event::Collapse);
            return;
        }
        7 => {
            event(Event::ToggleAutoHide);
            return;
        }
        10 => {
            event(Event::ToggleLocked);
            return;
        }
        12 => {
            event(Event::ToggleTopmost);
            return;
        }
        18 => {
            event(Event::Settings);
            return;
        }
        11 => {
            event(Event::ClosePane);
            return;
        }
        _ => return,
    }
    event(Event::MonitorOptions(options));
}

fn device_menu(
    hwnd: HWND,
    point: POINT,
    devices: &[(String, String)],
    selected: Option<&str>,
) -> Option<Option<String>> {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return None;
        }
        let mut names = vec![crate::i18n::text("monitor-auto").to_string()];
        names.extend(devices.iter().map(|(_, name)| name.clone()));
        for (i, name) in names.iter().enumerate() {
            let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            let checked = if i == 0 {
                selected.is_none()
            } else {
                selected == Some(devices[i - 1].0.as_str())
            };
            AppendMenuW(
                menu,
                MF_STRING | if checked { MF_CHECKED } else { 0 },
                i + 1,
                wide.as_ptr(),
            );
        }
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        DestroyMenu(menu);
        match command {
            0 => None,
            1 => Some(None),
            n => devices.get(n as usize - 2).map(|d| Some(d.0.clone())),
        }
    }
}

#[cfg(test)]
mod preference_tests {
    use super::*;
    #[test]
    fn monitor_preferences_roundtrip_devices_order_layout_and_sampling() {
        let store = WorkspaceStore::open_in_memory().unwrap();
        let id = PanelId::new(1);
        let preferences = Preferences {
            metrics: vec![Metric::Network, Metric::Gpu],
            compact: true,
            paused: true,
            interval: 5,
            network: Some("Ethernet".into()),
            disk: Some("C:\\".into()),
            gpu: Some("adapter-luid".into()),
        };
        preferences.save(&store, id).unwrap();
        let loaded = Preferences::load(&store, id).unwrap();
        assert_eq!(
            serde_json::to_value(loaded).unwrap(),
            serde_json::to_value(preferences).unwrap()
        );
    }
    #[test]
    fn persisted_duplicate_metrics_and_invalid_intervals_are_normalized() {
        let store = WorkspaceStore::open_in_memory().unwrap();
        store
            .save_preference(
                "monitor:1",
                r#"{"metrics":["Cpu","Memory","Cpu"],"interval":0}"#,
            )
            .unwrap();
        let preferences = Preferences::load(&store, PanelId::new(1)).unwrap();
        assert_eq!(preferences.metrics, [Metric::Cpu, Metric::Memory]);
        assert_eq!(preferences.interval, 1);
    }
}
