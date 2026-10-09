//! Execute settings controls; local UI changes stay separate from app borrows.
use super::*;

pub(super) struct Context<'a> {
    pub state: &'a Rc<RefCell<PaneApp>>,
    pub hwnd: windows_sys::Win32::Foundation::HWND,
    pub selected: PanelId,
    pub appearance: (PanelTheme, Backdrop),
    pub dark: bool,
    pub scale: f32,
    pub backup_view: &'a recovery::View,
    pub desktop_status: &'a str,
    pub font_search: windows_sys::Win32::Foundation::HWND,
    pub all_fonts: &'a [String],

    pub page: &'a mut usize,
    pub focus: &'a mut Option<usize>,
    pub hover: &'a mut Option<usize>,
    pub backup_offset: &'a mut usize,
    pub scroll_offset: &'a mut f32,
    pub folder_entry_mode: &'a mut folder::EntryMode,
    pub folder_defaults: &'a mut folder::Defaults,
    pub layout_defaults: &'a mut layout_defaults::Mode,
    pub style_input: &'a mut Option<(bool, String)>,
    pub startup: &'a mut crate::startup::Controller,
    pub diagnostics_copied: &'a mut bool,
    pub skill_prompt_copied: &'a mut bool,
    pub updates: &'a mut crate::updates::Controller,
    pub recording_show_panels: &'a mut bool,
    pub recording_search: &'a mut bool,
    pub recording_peek: &'a mut bool,
    pub painter: &'a mut Painter,
    pub font_choices: &'a mut Vec<String>,
    pub fonts_loaded: &'a mut bool,
    pub font_load_failed: &'a mut bool,
    pub toggle_motion: &'a mut std::collections::HashMap<usize, ToggleMotion>,
}

pub(super) fn execute(context: Context<'_>, c: &Control) -> bool {
    let state = context.state;
    let hwnd = context.hwnd;
    let selected = context.selected;
    let appearance = context.appearance;
    let dark = context.dark;
    let scale = context.scale;
    let backup_view = context.backup_view;
    let desktop_status = context.desktop_status;
    let font_search = context.font_search;
    let all_fonts = context.all_fonts;
    let mut invalidate = false;
    match &c.action {
        Action::BackupStatus => {
            let message = backup_view.status.clone();
            window::defer_action(move || {
                window::error(&message);
            });
        }
        Action::BackupAdvanced => {
            *context.page = 10;
            invalidate = true;
            *context.focus = None;
        }
        Action::BackupPage(direction) => {
            let count = 6;
            *context.backup_offset = if *direction < 0 {
                context.backup_offset.saturating_sub(count)
            } else {
                *context.backup_offset + count
            };
            invalidate = true;
            *context.scroll_offset = 0.0;
            *context.focus = None;
        }
        Action::BackupPolicy(kind) => {
            let state = Rc::clone(&state);
            let kind = *kind;
            let mut point = windows_sys::Win32::Foundation::POINT {
                x: (c.bounds.left * scale).round() as i32,
                y: ((c.bounds.bottom + 4.0) * scale).round() as i32,
            };
            unsafe {
                ClientToScreen(hwnd, &raw mut point);
            }
            window::defer_action(move || {
                let mut policy = recovery::Policy::load(&state.borrow().store);
                if kind == 0 {
                    policy.enabled = !policy.enabled;
                } else {
                    let options: &[(u64, &'static str)] = if kind == 1 {
                        &[
                            (5, crate::i18n::text("ui-5-minutes")),
                            (15, crate::i18n::text("ui-15-minutes")),
                            (30, crate::i18n::text("ui-30-minutes")),
                            (60, crate::i18n::text("ui-60-minutes")),
                        ]
                    } else {
                        &[
                            (10, crate::i18n::text("ui-last-10")),
                            (20, crate::i18n::text("ui-last-20")),
                            (50, crate::i18n::text("ui-last-50")),
                        ]
                    };
                    let selected = if kind == 1 {
                        policy.minutes
                    } else {
                        policy.keep as u64
                    };
                    if let Some(value) =
                        controls::choose(hwnd, point, appearance, options, selected)
                    {
                        if kind == 1 {
                            policy.minutes = value;
                        } else {
                            policy.keep = value as usize;
                        }
                    }
                }
                let saved = policy.save(&state.borrow().store);
                if let Err(e) = saved {
                    window::error(&e);
                }
                unsafe {
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            });
        }
        Action::BackupRecord(path) => {
            let state = Rc::clone(&state);
            let path = path.clone();
            let mut point = windows_sys::Win32::Foundation::POINT::default();
            unsafe {
                GetCursorPos(&raw mut point);
            }
            window::defer_action(move || {
                let rows = vec![
                    crate::pane::menu::entry(1, crate::i18n::text("ui-restore-06f9"), "", ""),
                    crate::pane::menu::entry(2, crate::i18n::text("ui-export"), "", ""),
                    crate::pane::menu::entry(3, crate::i18n::text("ui-delete"), "", ""),
                ];
                let result = crate::pane::menu::show_entries(
                    hwnd,
                    point,
                    false,
                    appearance.0,
                    appearance.1,
                    rows,
                );
                let event = match result {
                    1 => Some(Event::RestoreBackupPath(path)),
                    2 => Some(Event::ExportBackupPath(path)),
                    3 => Some(Event::DeleteBackup(path)),
                    _ => None,
                };
                if let Some(event) = event {
                    recovery::request(&state, &event);
                }
            });
        }
        Action::FolderEntryMode(value) => {
            let saved = value.save(&state.borrow().store);
            match saved {
                Ok(()) => {
                    *context.folder_entry_mode = *value;
                    invalidate = true;
                }
                Err(error) => window::error(&error),
            }
        }
        Action::LayoutDefaults(value) => {
            let saved = value.save(&state.borrow().store);
            match saved {
                Ok(()) => {
                    *context.layout_defaults = *value;
                    invalidate = true;
                }
                Err(error) => window::error(&error),
            }
        }
        Action::FolderDefaults(value) => {
            let saved = value.save(&state.borrow().store);
            match saved {
                Ok(()) => {
                    *context.folder_defaults = *value;
                    invalidate = true;
                }
                Err(error) => window::error(&error),
            }
        }
        Action::StyleInput(percentage) => {
            *context.style_input = Some((*percentage, String::new()));
            invalidate = true;
        }
        Action::SolidColor => {
            *context.page = 7;
            *context.focus = None;
            *context.hover = None;
            invalidate = true;
        }
        Action::ColorPreset(color) => {
            if let Backdrop::Solid { opacity, .. } = appearance.1 {
                if let Err(error) = handle(
                    &state,
                    selected,
                    Event::Material(Backdrop::Solid {
                        color: *color,
                        opacity,
                    }),
                ) {
                    window::error(&error);
                }
            }
        }
        Action::StrengthReset => {
            if let Err(error) = handle(&state, selected, Event::Material(appearance.1.base())) {
                window::error(&error);
            }
        }
        Action::SolidReset => {
            let value = Backdrop::solid_default(dark);
            if let Err(error) = handle(&state, selected, Event::Material(value)) {
                window::error(&error);
            }
        }
        Action::Change(Event::Material(Backdrop::Solid { .. })) => {
            let value = solid_style(&state.borrow().store, dark);
            if let Err(error) = handle(&state, selected, Event::Material(value)) {
                window::error(&error);
            }
        }
        Action::CliEnabled(enabled) => {
            let result = state.borrow().store.save_preference("cli_enabled", &enabled.to_string());
            if let Err(error) = result { window::error(&error.to_string()); }
            invalidate = true;
        }
        Action::CopySkillPrompt => {
            let result = agent::installation_prompt().and_then(|prompt|
                crate::clipboard::copy(hwnd as isize, &prompt).map_err(|e| e.to_string()));
            match result {
                Ok(()) => *context.skill_prompt_copied = true,
                Err(error) => window::error(&error),
            }
            invalidate = true;
        }
        Action::LogLevel(level) => {
            let result = state
                .borrow()
                .store
                .save_preference("log_level", &level.label().to_ascii_lowercase());
            match result {
                Ok(()) => luciddesk_diagnostics::set_level(*level),
                Err(error) => window::error(&error.to_string()),
            }
            invalidate = true;
        }
        Action::Language(code) => {
            let result = state.borrow().store.save_preference("language", code);
            if let Err(error) = result {
                window::error(&error.to_string());
            }
            invalidate = true;
        }
        Action::Startup(enabled) => {
            if let Err(error) = start_login_operation(&mut *context.startup, hwnd, Some(*enabled)) {
                window::error(&error);
            }
            invalidate = true;
        }
        Action::CopyDiagnostics => {
            let report = format!(
                "{}Desktop: {}\r\n",
                crate::system_info::report(),
                desktop_status
            );
            match crate::clipboard::copy(hwnd as isize, &report) {
                Ok(()) => {
                    *context.diagnostics_copied = true;
                    invalidate = true;
                }
                Err(error) => window::error(&error.to_string()),
            }
        }
        Action::Update => {
            if !context.updates.busy() {
                let result = context.updates.check();
                if let Err(error) = result {
                    window::error(&error);
                }
                if context.updates.busy() && unsafe { SetTimer(hwnd, UPDATE_TIMER, 200, None) } == 0
                {
                    *context.updates = crate::updates::Controller::default();
                    window::error(crate::i18n::text("update-failed"));
                }
                invalidate = true;
            }
        }
        Action::ProjectLink(url) => {
            if let Err(error) = luciddesk_shell::open_shell_identity(
                hwnd as isize,
                &luciddesk_core::ShellIdentity::Namespace {
                    parsing_name: (*url).into(),
                },
            ) {
                window::error(&error.to_string());
            }
        }
        Action::EverythingBrowse | Action::EverythingDetect | Action::EverythingLaunch => {
            let mut value = everything_settings::settings();
            let result = (|| -> Result<(), String> {
                match c.action {
                    Action::EverythingBrowse => {
                        let Some(path) = everything_settings::browse(hwnd as isize)? else {
                            return Ok(());
                        };
                        value.path = path;
                    }
                    Action::EverythingDetect => value.path.clear(),
                    Action::EverythingLaunch => return everything_settings::launch(),
                    _ => unreachable!(),
                }
                everything_settings::save(&state.borrow().store, value)
            })();
            if let Err(error) = result {
                window::error(&error);
            }
            invalidate = true;
        }
        Action::ShowPanelsEnable => {
            let saved = {
                let s = state.borrow();
                let enabled = !show_hotkey::enabled(&s.store);
                s.store
                    .save_preference("show_panels_enabled", if enabled { "1" } else { "0" })
            };
            if let Err(error) = saved {
                window::error(&error.to_string());
            }
            *context.recording_show_panels = false;
        }
        Action::ShowPanelsShortcut => {
            *context.recording_show_panels = true;
            *context.recording_search = false;
            *context.recording_peek = false;
        }
        Action::ShowPanelsReset => {
            let saved = show_hotkey::save(&state.borrow().store, show_hotkey::default_shortcut());
            if let Err(error) = saved {
                window::error(&error);
            }
            *context.recording_show_panels = false;
        }
        Action::SearchShortcut => {
            *context.recording_search = true;
            *context.recording_peek = false;
            *context.recording_show_panels = false;
        }
        Action::SearchReset => {
            let saved = search_hotkey::save(&state.borrow().store, Default::default());
            if let Err(error) = saved {
                window::error(&error);
            }
            *context.recording_search = false;
        }
        Action::PeekShortcut => {
            *context.recording_peek = true;
            *context.recording_search = false;
            *context.recording_show_panels = false;
        }
        Action::PreviewProvider(_)
        | Action::PeekEnable
        | Action::PeekBrowse
        | Action::PeekDetect
        | Action::PeekReset => {
            let mut value = peek::settings();
            let result = (|| -> Result<(), String> {
                match c.action {
                    Action::PreviewProvider(provider) => value.provider = provider,
                    Action::PeekEnable => value.enabled = !value.enabled,
                    Action::PeekBrowse => {
                        let Some(path) = peek::browse(hwnd as isize, value.provider)? else {
                            return Ok(());
                        };
                        value.set_path(path);
                    }
                    Action::PeekDetect => {
                        value.set_path(String::new());
                    }
                    Action::PeekReset => {
                        value.key = VK_SPACE;
                        value.modifiers = 0;
                    }
                    _ => unreachable!(),
                }
                peek::save(&state.borrow().store, value)
            })();
            if let Err(error) = result {
                window::error(&error);
            }
            invalidate = true;
        }
        Action::Window(command) => unsafe {
            let command = if *command == SC_MAXIMIZE && IsZoomed(hwnd) != 0 {
                SC_RESTORE
            } else {
                *command
            };
            PostMessageW(hwnd, WM_SYSCOMMAND, command as usize, 0);
        },
        Action::FontSearch => unsafe {
            SetFocus(font_search);
        },
        Action::Font(name) => {
            let result = (|| -> Result<(), String> {
                fonts::save(&state.borrow().store, name)?;
                *context.painter = Painter::new().map_err(|e| e.to_string())?;
                refresh_font_views(state, rename::cancel);
                Ok(())
            })();
            if let Err(error) = result {
                window::error(&error);
            }
            invalidate = true;
        }
        Action::Page(value) => {
            if *value == 13 {
                if let Err(error) = start_login_operation(&mut *context.startup, hwnd, None) {
                    window::error(&error);
                }
            }
            if *value == 11 {
                *context.font_choices = all_fonts.to_vec();
                if *context.font_load_failed {
                    *context.fonts_loaded = false;
                    *context.font_load_failed = false;
                }
                if !font_search.is_null() {
                    unsafe {
                        SetWindowTextW(font_search, windows_sys::w!(""));
                    }
                }
            }
            *context.recording_peek = false;
            *context.recording_search = false;
            *context.recording_show_panels = false;
            *context.diagnostics_copied = false;
            *context.page = *value;
            context.toggle_motion.clear();
            *context.focus = None;
        }
        Action::GridSize(_)
        | Action::Radius(_)
        | Action::Opacity(_)
        | Action::Strength(_)
        | Action::Channel(_, _) => {}
        Action::Change(Event::Material(value)) if value.strength().is_some() => {
            let backdrop = material_style(&state.borrow().store, *value);
            if let Err(error) = handle(&state, selected, Event::Material(backdrop)) {
                window::error(&error);
            }
        }
        Action::Change(event) => {
            if let Err(e) = handle(&state, selected, event.clone()) {
                window::error(&e);
            }
        }
    }
    invalidate
}

/// Destroying a focused editor can synchronously reenter the owning pane.
pub(super) fn refresh_font_views(
    state: &Rc<RefCell<PaneApp>>,
    mut cancel: impl FnMut(windows_sys::Win32::Foundation::HWND),
) {
    let windows: Vec<_> = state
        .borrow()
        .views
        .iter()
        .map(|view| view.window.hwnd().cast())
        .collect();
    for hwnd in windows {
        cancel(hwnd);
    }
    let mut owner = state.borrow_mut();
    refresh_changed_views(&mut owner, true);
    for view in &owner.views {
        unsafe {
            InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
        }
    }
    owner.wake.notify();
}
