//! Modeless settings, drawn with the same native composition pipeline as panes.
#![allow(
    clippy::wildcard_imports,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use windows_canvas::{ColorF, Ellipse, Rect, RoundedRect, Vector2};

use super::native_graphics::canvas_result;
use super::search::{everything_settings, hotkey as search_hotkey};
use super::*;
use luciddesk_core::{Backdrop, PanelTheme};
use windows_canvas::ID2D1DeviceContext;
use windows_sys::Win32::{
    Graphics::Gdi::*,
    UI::{HiDpi::GetDpiForWindow, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
const FONT_LOAD_TIMER: usize = 0x4c5046;
const UPDATE_TIMER: usize = 0x4c5056;
const STARTUP_TIMER: usize = 0x4c5354;
const SELECT_PANEL: u32 = WM_APP + 95;
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
    Startup(bool),
    CliEnabled(bool),
    CopySkillPrompt,
    LogLevel(luciddesk_diagnostics::Level),
    Font(String),
    FontSearch,
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
    Window(u32),
    Page(usize),
    Language(&'static str),
    Change(Event),
    Radius(f32),
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
    SearchShortcut,
    ShowPanelsEnable,
    ShowPanelsShortcut,
    ShowPanelsReset,
    SearchReset,
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

mod controls;
mod components;
mod preview;
use components::{Tokens, SettingsForm, ContentClip};
use controls::{Control, ControlKind, Slider, Style};
struct Scene {
    material: Backdrop,
    viewport: Option<Rect>,
    scroll_max: f32,
    scroll_offset: f32,
    text: Vec<(Rect, String, usize)>,
    cards: Vec<Rect>,
    separators: Vec<Rect>,
    controls: Vec<Control>,
    previews: Vec<(Rect, Backdrop)>,
    app_icon: Option<Rect>,
}

fn contains(r: &Rect, x: f32, y: f32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}
mod layout;
use layout::scene;

const TOGGLE_TIMER: usize = 0x4c5054;
use super::animation::Motion as ToggleMotion;
mod painter;
use painter::Painter;

mod agent;
mod actions;
mod host;
pub(super) use host::show;

#[cfg(test)]
pub(super) mod tests;
