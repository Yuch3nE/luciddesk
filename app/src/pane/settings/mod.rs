//! Modeless settings, drawn with the same native composition pipeline as panes.
#![allow(
    clippy::wildcard_imports,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use windows_canvas::{ColorF, Ellipse, Rect, RoundedRect, Vector2};

use super::native_graphics::canvas_result;
use super::search::everything_settings;
use super::*;
use luciddesk_core::{Backdrop, PanelTheme};
use luciddesk_window::controls::NavigationItem;
use windows_canvas::ID2D1DeviceContext;
use windows_sys::Win32::{
    Graphics::Gdi::*,
    UI::{ Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
const FONT_LOAD_TIMER: usize = 0x4c5046;
const UPDATE_TIMER: usize = 0x4c5056;
const STARTUP_TIMER: usize = 0x4c5354;
const ORGANIZE_TIMER: usize = 0x4c4f52;
const SELECT_PANEL: u32 = WM_APP + 95;
const SELECT_PLUGIN: u32 = WM_APP + 97;
const SELECT_SETTING: u32 = WM_APP + 98;
const CLASSIFICATION_KEY: u32 = WM_APP + 99;
#[cfg(test)]
const INSPECT_STARTUP: u32 = WM_APP + 198;
#[cfg(test)]
const SELECT_TEST_PAGE: u32 = WM_APP + 199;
thread_local! { static PLUGIN_SELECTION: RefCell<Option<String>> = const { RefCell::new(None) }; }
const PREPARE_REVEAL: u32 = WM_APP + 96;
const REVEAL_TIMER: usize = 0x4c5055;
pub(super) const DEFAULT_HEIGHT: i32 = 600;
const MIN_HEIGHT: f32 = 560.0;

struct PendingReveal {
    started: std::time::Instant,
    ready: Box<dyn Fn() -> bool>,
    fade: bool,
}

use crate::window_visibility::defer_show;

unsafe fn cloak(
    hwnd: windows_sys::Win32::Foundation::HWND,
    hidden: bool,
) -> windows::core::Result<()> {
    unsafe {
        super::native_graphics::set_attribute(
            windows::Win32::Foundation::HWND(hwnd),
            windows::Win32::Graphics::Dwm::DWMWA_CLOAK.0,
            &i32::from(hidden),
        )
    }
}
use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;

#[derive(Clone)]
enum Action {
    Classification(classification::Command),
    OrganizeTab(classification::Tab),
    OpenOrganizer,
    Dropdown(std::sync::Arc<[(String, Action, bool)]>),
    PluginEnabled(String, bool),
    PluginPage(String),
    PluginAction(String, serde_json::Value),
    Startup(bool),
    CliEnabled(bool),
    CopySkillPrompt,
    LogLevel(luciddesk_diagnostics::Level),
    Font(String),
    FontSearch,
    ClearFontSearch,
    FolderDefaults(folder::Defaults),
    LayoutDefaults(layout_defaults::Mode),
    FolderEntryMode(folder::EntryMode),
    BackupPolicy(u8),
    BackupRecord(std::path::PathBuf),
    BackupPage(isize),
    BackupAdvanced,
    BackupStatus,
    ProjectLink(&'static str),
    CopyDiagnostics,
    Update,
    InstallUpdate,
    AutoCheckUpdates(bool),
    Window(u32),
    Page(usize),
    Language(&'static str),
    Change(Event),
    Radius(f32),
    SnapGap(u32),
    GridSize(f32),
    Opacity(u8),
    Strength(u8),
    StrengthReset,
    Channel(u8, u8),
    ColorPreset(u32),
    SolidColor,
    SolidReset,
    StyleInput(bool),
    PeekEnable,
    PreviewProvider(peek::Provider),
    PeekBrowse,
    PeekDetect,
    PeekShortcut,
    ShowPanelsEnable,
    DesktopDoubleClick(bool),
    ShowPanelsShortcut,
    ShowPanelsReset,
    PeekReset,
    EverythingBrowse,
    EverythingDetect,
    EverythingLaunch,
}
mod lifecycle;
use lifecycle::*;
mod font_search;
use font_search::*;
mod appearance;
use appearance::*;
mod frame;
use frame::*;

mod components;
mod controls;
mod motions;
mod preview;
use components::{ContentClip, SettingsForm, Tokens};
use controls::{Control, ControlKind, DestinationChange, SectionHeader, Slider, Style};
struct Scene {
    content_left: f32,
    content_top: f32,
    navigation_viewport: Option<Rect>,
    navigation_text: std::ops::Range<usize>,
    navigation_max: f32,
    navigation_offset: f32,
    material: Backdrop,
    viewport: Option<Rect>,
    footer_height: f32,
    footer_status: Option<(Rect, String)>,
    scroll_max: f32,
    scroll_offset: f32,
    text: Vec<(Rect, String, usize)>,
    cards: Vec<Rect>,
    segment_tracks: Vec<SegmentTrack>,
    status_pills: Vec<(Rect, bool)>,
    specimen_boxes: Vec<Rect>,
    section_headers: Vec<SectionHeader>,
    destination_changes: Vec<DestinationChange>,
    rule_rows: Vec<RuleRow>,
    rule_transitions: Vec<RuleRowTransition>,
    disclosure_clips: Vec<DisclosureClip>,
    activity: Option<Rect>,
    rule_grip_hover: Option<usize>,
    rule_grip_pressed: Option<usize>,
    floating_rule: Option<usize>,
    separators: Vec<Rect>,
    controls: Vec<Control>,
    previews: Vec<(Rect, Backdrop)>,
    app_icon: Option<Rect>,
    caption_icon: Option<Rect>,
}

struct SegmentTrack {
    bounds: Rect,
    members: std::ops::Range<usize>,
}

impl Scene {
    fn empty(material: Backdrop, content_left: f32, content_top: f32) -> Self {
        Self {
            content_left,
            content_top,
            navigation_viewport: None,
            navigation_text: 0..0,
            navigation_max: 0.0,
            navigation_offset: 0.0,
            material,
            viewport: None,
            footer_height: 0.0,
            footer_status: None,
            scroll_max: 0.0,
            scroll_offset: 0.0,
            text: Vec::new(),
            cards: Vec::new(),
            segment_tracks: Vec::new(),
            status_pills: Vec::new(),
            specimen_boxes: Vec::new(),
            section_headers: Vec::new(),
            destination_changes: Vec::new(),
            rule_rows: Vec::new(),
            rule_transitions: Vec::new(),
            disclosure_clips: Vec::new(),
            activity: None,
            rule_grip_hover: None,
            rule_grip_pressed: None,
            floating_rule: None,
            separators: Vec::new(),
            controls: Vec::new(),
            previews: Vec::new(),
            app_icon: None,
            caption_icon: None,
        }
    }

    fn rule_transition(
        &self, index: usize, range: fn(&RuleRow) -> &std::ops::Range<usize>,
    ) -> Option<&RuleRowTransition> {
        let next = self.rule_transitions.partition_point(|row| range(&row.row).end <= index);
        self.rule_transitions.get(next).filter(|row| range(&row.row).contains(&index))
    }
}

struct DisclosureClip {
    text: std::ops::Range<usize>,
    separators: std::ops::Range<usize>,
    bounds: Rect,
}

struct RuleRowTransition {
    row: RuleRow,
    opacity: f32,
    leaving: bool,
    grip: bool,
}

#[derive(Clone, Default)]
struct RuleRow {
    bounds: Rect,
    offset: f32,
    controls: std::ops::Range<usize>,
    text: std::ops::Range<usize>,
    separators: std::ops::Range<usize>,
    detail_clips: std::ops::Range<usize>,
}

impl RuleRow {
    const GRIP_WIDTH: f32 = Tokens::CONTROL_HEIGHT;

    fn visual_bounds(&self) -> Rect {
        Rect { top: self.bounds.top + self.offset, bottom: self.bounds.bottom + self.offset, ..self.bounds }
    }

    fn grip(&self) -> Rect {
        Rect::from_xywh(self.bounds.left, self.bounds.top + self.offset + Tokens::GAP,
            Self::GRIP_WIDTH, Tokens::CONTROL_HEIGHT)
    }
}

fn contains(r: &Rect, x: f32, y: f32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}
mod layout;
mod plugin_content;
pub(in crate::pane) mod plugin_page;
use layout::scene;

const CONTROL_MOTION_TIMER: usize = 0x4c5054;
use super::animation::Motion as ToggleMotion;
mod painter;
use painter::Painter;

mod actions;
mod classification;
mod agent;
mod host;
pub(super) use host::{show, show_monitor};
pub(super) use classification::show_organize_preview;

#[cfg(test)]
pub(super) mod tests;
