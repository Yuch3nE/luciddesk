//! Settings window construction and native message dispatch.
use super::*;

pub(in crate::pane) fn show(state: &Rc<RefCell<PaneApp>>, id: PanelId) -> Result<(), String> {
    // Native activation synchronously dispatches messages to other panes. Do not
    // hold a shared-state borrow while showing or activating this window.
    let existing = state.borrow().settings.as_ref().map(|window| window.hwnd());
    if let Some(hwnd) = existing {
        unsafe {
            SendMessageW(hwnd.cast(), SELECT_PANEL, id.get() as usize, 0);
            ShowWindow(hwnd.cast(), SW_RESTORE);
            SetForegroundWindow(hwnd.cast());
        }
        return Ok(());
    }
    let weak = Rc::downgrade(state);
    let (mut panels, mut appearance) = {
        let state = state.borrow();
        (
            state.workspace.panels().to_vec(),
            state
                .workspace
                .appearance()
                .or_else(|| {
                    state
                        .workspace
                        .panels()
                        .first()
                        .map(|p| (p.theme(), p.backdrop()))
                })
                .unwrap_or((PanelTheme::System, Backdrop::Mica)),
        )
    };
    let mut options = state.borrow().workspace.pane_options();
    let mut folder_defaults = folder::Defaults::load(&state.borrow().store)?;
    let mut layout_defaults = layout_defaults::Mode::load(&state.borrow().store)?;
    let mut folder_entry_mode = folder::EntryMode::load(&state.borrow().store)?;
    let (mut show_panels_enabled, mut show_panels_shortcut, mut chosen_language) = {
        let owner = state.borrow();
        (show_hotkey::enabled(&owner.store), show_hotkey::settings(&owner.store),
            owner.store.preference("language").ok().flatten().unwrap_or_else(|| "system".into()))
    };
    let mut font_choices = Vec::<String>::new();
    let mut all_fonts = Vec::<String>::new();
    let mut font_load: Option<fonts::CandidateLoad> = None;
    let mut fonts_loaded = false;
    let mut font_load_failed = false;
    let mut font_editor = font_search::Editor::default();
    let mut painter_family = fonts::family();
    let mut painter_language = crate::i18n::language();
    let mut painter = Painter::new().map_err(|e| e.to_string())?;
    let mut surface: Option<composition::Surface> = None;
    let mut paint_recovery = composition::PaintRecovery::default();
    let mut reveal: Option<PendingReveal> = None;
    // Navigation belongs to this window, not the saved workspace or connection state.
    let mut page = 0;
    let mut recording_peek = false;
    let mut recording_search = false;
    let mut recording_show_panels = false;
    let mut search_visible = false;
    let mut selected = id;
    let mut hover = None;
    let mut focus = None;
    let mut keyboard_focus = false;
    let mut pressed = None;
    let mut pending_styles = PendingStyles::default();
    let mut style_input: Option<(bool, String)> = None;
    let mut cached_scene = None;
    let mut scene_key = None;
    let mut scroll_offset = 0.0f32;
    let mut scroll_drag = None;
    let mut scroll_page = page;
    let mut desktop_status = String::new();
    let mut diagnostics_copied = false;
    let mut skill_prompt_copied = false;
    let mut cli_enabled = control::enabled(&state.borrow().store);
    let mut updates = crate::updates::Controller::default();
    let mut startup = crate::startup::Controller::default();
    let mut backup_view = recovery::View::default();
    let mut backup_policy = recovery::Policy::default();
    let mut backup_offset = 0usize;
    let mut toggle_timer_running = false;
    let mut toggle_motion = std::collections::HashMap::<usize, ToggleMotion>::new();
    let prepared = Rc::new(std::cell::Cell::new(false));
    let show_prepared = Rc::clone(&prepared);
    let window = windows_window::Window::new(crate::i18n::text("ui-luciddesk-settings"))
        .size(900, DEFAULT_HEIGHT)
        .style(WS_OVERLAPPEDWINDOW)
        .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP)
        .on_message(move |raw, msg, wp, lp| {
            let hwnd = raw.cast();
            if unsafe { defer_show(msg, lp, show_prepared.get()) } {
                return Some(0);
            }
            if msg == WM_NCCALCSIZE || msg == WM_NCPAINT {
                return Some(0);
            }
            if msg == WM_NCACTIVATE {
                return Some(1);
            }
            let Some(state) = weak.upgrade() else {
                return Some(0);
            };
            if msg == WM_TIMER && wp == UPDATE_TIMER {
                updates.poll();
                if !updates.busy() { unsafe { KillTimer(hwnd, UPDATE_TIMER); } }
                scene_key = None;
                unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                return Some(0);
            }
            if msg == WM_TIMER && wp == STARTUP_TIMER {
                if let Some(error) = startup.poll() { window::error(&error); }
                if !startup.busy() { unsafe { KillTimer(hwnd, STARTUP_TIMER); } }
                scene_key = None;
                unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                return Some(0);
            }
            if msg == WM_TIMER && wp == FONT_LOAD_TIMER {
                if let Some(load) = &font_load {
                    match load.poll() {
                        Err(std::sync::mpsc::TryRecvError::Empty) => return Some(0),
                        result => {
                            font_load_failed = result.is_err();
                            all_fonts = result.unwrap_or_default();
                            let mut query = vec![0u16; if font_editor.hwnd.is_null() { 1 } else { unsafe { GetWindowTextLengthW(font_editor.hwnd) }.max(0) as usize + 1 }];
                            let len = if font_editor.hwnd.is_null() { 0 } else { unsafe { GetWindowTextW(font_editor.hwnd, query.as_mut_ptr(), query.len() as i32) }.max(0) as usize };
                            font_choices = filter_fonts(&all_fonts, &String::from_utf16_lossy(&query[..len]));
                            fonts_loaded = true; font_load = None; scene_key = None;
                        }
                    }
                }
                unsafe { KillTimer(hwnd, FONT_LOAD_TIMER); InvalidateRect(hwnd, std::ptr::null(), 0); }
                return Some(0);
            }
            if page == 11 && !fonts_loaded && font_load.is_none() && msg == WM_PAINT {
                match fonts::CandidateLoad::start() {
                    Ok(load) => {
                        if unsafe { SetTimer(hwnd, FONT_LOAD_TIMER, 100, None) } != 0 { font_load = Some(load); }
                        else { fonts_loaded = true; font_load_failed = true; }
                    }
                    Err(_) => { fonts_loaded = true; font_load_failed = true; }
                }
                scene_key = None;
            }
            if msg == WM_FONTCHANGE {
                font_load = None; fonts_loaded = false; font_load_failed = false;
                all_fonts.clear();
                let query = if font_editor.hwnd.is_null() { String::new() } else {
                    let mut text = vec![0u16; unsafe { GetWindowTextLengthW(font_editor.hwnd) }.max(0) as usize + 1];
                    let len = unsafe { GetWindowTextW(font_editor.hwnd, text.as_mut_ptr(), text.len() as i32) };
                    String::from_utf16_lossy(&text[..len.max(0) as usize])
                };
                font_choices = filter_fonts(&all_fonts, &query);
                scroll_offset = 0.0; focus = None; scene_key = None;
                unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                return Some(0);
            }
            if msg == crate::i18n::CHANGED {
                scene_key = None;
                font_load = None; fonts_loaded = false; font_load_failed = false;
                all_fonts.clear(); font_choices.clear();
                // Keep the query and page position. The new language's background
                // scan reapplies the current query when its results arrive.
                unsafe { KillTimer(hwnd, FONT_LOAD_TIMER); }
                focus = None;
                hover = None;
                pressed = None;
                scroll_drag = None;
                toggle_motion.clear();
                unsafe {
                    SetWindowTextW(hwnd, crate::i18n::wide("ui-luciddesk-settings"));
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                return Some(0);
            }
            if msg == WM_COMMAND && !font_editor.hwnd.is_null() && lp == font_editor.hwnd as isize
                && (wp >> 16) as u32 == EN_CHANGE {
                let mut text = vec![0u16; unsafe { GetWindowTextLengthW(font_editor.hwnd) }.max(0) as usize + 1];
                let len = unsafe { GetWindowTextW(font_editor.hwnd, text.as_mut_ptr(), text.len() as i32) };
                let query = String::from_utf16_lossy(&text[..len.max(0) as usize]);
                font_choices = filter_fonts(&all_fonts, &query);
                 scroll_offset = 0.0; scene_key = None;
                unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                return Some(0);
            }
            if let Some((percentage, text)) = &mut style_input {
                if msg == WM_CHAR {
                    if wp == 8 { text.pop(); }
                    else if let Some(c) = char::from_u32(wp as u32) {
                        if (c.is_ascii_hexdigit() || c == '#' || c == '%') && text.len() < 8 { text.push(c); }
                    }
                    scene_key = None;
                    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                    return Some(0);
                }
                if msg == WM_KEYDOWN {
                    if wp == VK_ESCAPE as usize { style_input = None; }
                    else if wp == VK_RETURN as usize {
                        if let Some(value) = edited_solid(appearance.1, *percentage, text) {
                            if let Err(error) = handle(&state, selected, Event::Material(value)) { window::error(&error); }
                            style_input = None;
                        } else { window::error(crate::i18n::text("ui-enter-a-six-digit-hex-color-or-a-percentage-from-0-to-100")); }
                    } else { return Some(0); }
                    scene_key = None;
                    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                    return Some(0);
                }
                if msg == WM_LBUTTONDOWN { style_input = None; scene_key = None; }
            }
            if msg == PREPARE_REVEAL {
                let ready = surface.as_ref().and_then(|s| s.commit_ready().ok())
                    .unwrap_or_else(|| Box::new(|| true));
                let mut animations = 1i32;
                unsafe {
                    SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut animations).cast(), 0);
                }
                reveal = Some(PendingReveal {
                    started: std::time::Instant::now(), ready, fade: animations != 0,
                });
                unsafe {
                    if SetTimer(hwnd, REVEAL_TIMER, USER_TIMER_MINIMUM, None) == 0 {
                        reveal = None;
                        let _ = cloak(hwnd, false);
                        SetForegroundWindow(hwnd);
                    }
                }
                return Some(0);
            }
            if msg == WM_TIMER && wp == REVEAL_TIMER {
                if let Some(pending) = &mut reveal {
                    let timed_out = pending.started.elapsed().as_secs() >= 1;
                    if !(pending.ready)() && !timed_out { return Some(0); }
                    if pending.fade && !timed_out {
                        pending.fade = false;
                        // Commit the initial animation frame while still cloaked,
                        // so uncloaking cannot briefly expose full-opacity content.
                        if let Some(surface) = &surface
                            && let Ok(ready) = surface.fade_in().and_then(|()| surface.commit_ready())
                        {
                            pending.ready = ready;
                            return Some(0);
                        }
                    }
                    reveal = None;
                    unsafe {
                        KillTimer(hwnd, REVEAL_TIMER);
                        let _ = windows::Win32::Graphics::Dwm::DwmFlush();
                        let _ = cloak(hwnd, false);
                        SetForegroundWindow(hwnd);
                    }
                }
                return Some(0);
            }
            if msg == SELECT_PANEL {
                if wp != 0 {
                    selected = PanelId::new(wp as u64);
                }
                unsafe {
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                return Some(0);
            }
            if msg == WM_CLOSE {
                updates = crate::updates::Controller::default();
                if !lifecycle::close(&state, hwnd, &mut pending_styles) {
                    unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0); }
                }
                return Some(0);
            }
            if msg == WM_DESTROY {
                return Some(0);
            }
            let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
            if msg == WM_NCHITTEST {
                let mut point = windows_sys::Win32::Foundation::POINT {
                    x: (lp as u16 as i16).into(),
                    y: ((lp >> 16) as u16 as i16).into(),
                };
                let mut bounds = RECT::default();
                unsafe {
                    ScreenToClient(hwnd, &raw mut point);
                    GetClientRect(hwnd, &raw mut bounds);
                }
                return Some(frame_hit(
                    point.x as f32 / scale,
                    point.y as f32 / scale,
                    bounds.right as f32 / scale,
                    bounds.bottom as f32 / scale,
                    unsafe { IsZoomed(hwnd) } != 0,
                ) as isize);
            }
            if msg == WM_GETMINMAXINFO {
                unsafe {
                    let info = &mut *(lp as *mut MINMAXINFO);
                    info.ptMinTrackSize.x = (800.0 * scale) as i32;
                    info.ptMinTrackSize.y = (MIN_HEIGHT * scale) as i32;
                    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                    let mut metrics = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    if GetMonitorInfoW(monitor, &raw mut metrics) != 0 {
                        info.ptMinTrackSize.x = info
                            .ptMinTrackSize
                            .x
                            .min(metrics.rcWork.right - metrics.rcWork.left);
                        info.ptMinTrackSize.y = info
                            .ptMinTrackSize
                            .y
                            .min(metrics.rcWork.bottom - metrics.rcWork.top);
                        info.ptMaxPosition.x = metrics.rcWork.left - metrics.rcMonitor.left;
                        info.ptMaxPosition.y = metrics.rcWork.top - metrics.rcMonitor.top;
                        info.ptMaxSize.x = metrics.rcWork.right - metrics.rcWork.left;
                        info.ptMaxSize.y = metrics.rcWork.bottom - metrics.rcWork.top;
                    }
                }
                return Some(0);
            }
            if matches!(msg, WM_SETTINGCHANGE | WM_THEMECHANGED | WM_POWERBROADCAST) {
                unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
            }
            if msg == WM_DPICHANGED {
                let _ = crate::app_icon::apply(hwnd);
                unsafe {
                    let r = &*(lp as *const RECT);
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                return Some(0);
            }
            if msg == WM_SETCURSOR && (lp as u16) as u32 == HTCLIENT && page == 11 {
                if let Some(scene) = cached_scene.as_ref() {
                    let mut point = windows_sys::Win32::Foundation::POINT::default();
                    unsafe { GetCursorPos(&raw mut point); ScreenToClient(hwnd, &raw mut point); }
                    let x = point.x as f32 / scale;
                    let y = point.y as f32 / scale;
                    if font_search_hit(scene, x, y) {
                        let clear = scene.controls.iter().any(|c| matches!(c.action, Action::FontSearch)
                            && c.selected && x >= c.bounds.right - 40.0);
                        unsafe { SetCursor(LoadCursorW(std::ptr::null_mut(), if clear { IDC_ARROW } else { IDC_IBEAM })); }
                        return Some(1);
                    }
                }
            }
            // Hook synchronization can repaint/activate this window while the
            // workspace is mutably borrowed. Render the last complete snapshot
            // during that reentry; unrelated native messages need no snapshot.
            if !matches!(
                msg,
                WM_PAINT
                    | WM_ERASEBKGND
                    | WM_SIZE
                    | WM_ACTIVATE
                    | WM_WINDOWPOSCHANGED
                    | WM_SHOWWINDOW
                    | WM_MOUSEWHEEL
                    | WM_MOUSEMOVE
                    | WM_MOUSELEAVE
                    | WM_NCMOUSEMOVE
                    | WM_LBUTTONDOWN
                    | WM_LBUTTONUP
                    | WM_CAPTURECHANGED
                    | WM_CANCELMODE
                    | WM_TIMER
                    | WM_KEYDOWN
                    | WM_SYSKEYDOWN
            ) {
                return None;
            }
            let mut snapshot_changed = false;
            if painter_family != fonts::family() || painter_language != crate::i18n::language() {
                if let Ok(fresh) = Painter::new() {
                    painter = fresh;
                    painter_family = fonts::family();
                    painter_language = crate::i18n::language();
                    snapshot_changed = true;
                }
            }
            let available = if let Ok(state) = state.try_borrow() {
                if panels != state.workspace.panels() {
                    panels = state.workspace.panels().to_vec();
                    snapshot_changed = true;
                }
                let fresh_cli = control::enabled(&state.store);
                snapshot_changed |= cli_enabled != fresh_cli;
                cli_enabled = fresh_cli;
                search_visible = state.views.iter().any(|v| state.workspace.panel(v.id).is_some_and(Panel::is_search));
                desktop_status = runtime::status(&state);
                let fresh = recovery::view(&state);
                let policy = recovery::Policy::load(&state.store);
                snapshot_changed |= fresh != backup_view || policy != backup_policy;
                backup_view = fresh; backup_policy = policy;
                if backup_offset >= backup_view.records.len() {backup_offset = 0;}
                let defaults=folder::Defaults::load(&state.store).unwrap_or_default();
                snapshot_changed |= defaults != folder_defaults;
                folder_defaults=defaults;
                let defaults = layout_defaults::Mode::load(&state.store).unwrap_or_default();
                snapshot_changed |= defaults != layout_defaults;
                layout_defaults = defaults;
                let mode = folder::EntryMode::load(&state.store).unwrap_or_default();
                snapshot_changed |= mode != folder_entry_mode;
                folder_entry_mode = mode;
                show_panels_enabled = show_hotkey::enabled(&state.store);
                show_panels_shortcut = show_hotkey::settings(&state.store);
                chosen_language = state.store.preference("language").ok().flatten().unwrap_or_else(|| "system".into());
                options = state.workspace.pane_options();
                appearance = state
                    .workspace
                    .appearance()
                    .or_else(|| panels.first().map(|p| (p.theme(), p.backdrop())))
                    .unwrap_or((PanelTheme::System, Backdrop::Mica));
                true
            } else {
                false
            };
            let at = panels.iter().position(|p| p.id() == selected).unwrap_or(0);
            let panel = panels.get(at);
            if let Some(p) = panel {
                selected = p.id();
            }
            let dark = theme::is_dark(appearance.0);
            let mut bounds = RECT::default();
            unsafe {
                GetClientRect(hwnd, &raw mut bounds);
            }
            let (w, h) = (bounds.right as f32 / scale, bounds.bottom as f32 / scale);
            let key = (
                w.to_bits(),
                h.to_bits(),
                page,
                selected,
                appearance,
                options,
                peek::settings(),
                everything_settings::settings(),
                (recording_peek, recording_search, search_hotkey::settings(), search_hotkey::status(),
                    recording_show_panels, show_panels_enabled,
                    show_panels_shortcut, show_hotkey::status()),
                unsafe { IsZoomed(hwnd) } != 0,
                search_visible,
                (desktop_status.clone(), header_divider::enabled(), compact_menu::enabled(), updates.status(), startup.status(), startup.busy()),
            );
            if scroll_page != page { scroll_offset = 0.0; scroll_page = page; }
            let scene_changed = snapshot_changed || scene_key.as_ref() != Some(&key);
            if scene_changed {
                let mut body = scene(
                        w,
                        h - TITLE_HEIGHT,
                        page,
                        search_visible,
                        appearance,
                        options,
                    );
                if page == 12 {
                    layout::language(&mut body, w, &chosen_language);
                }
                if page == 13 { layout::general(&mut body, w, startup.status(), startup.busy(), cli_enabled, skill_prompt_copied); }
                if page == 1 {
                    layout::desktop_defaults(&mut body, w, layout_defaults);
                    layout::show_panels_shortcut(&mut body, w, show_panels_enabled, show_panels_shortcut);
                }
                if page == 11 { layout::fonts_status(&mut body, w, &font_choices, if !fonts_loaded { Some("font-loading") } else if font_load_failed { Some("font-load-failed") } else { None }); }
                if page == 8 { layout::folder_defaults(&mut body, w, folder_defaults, folder_entry_mode); }
                if matches!(page,6|9|10) {
                    if page==9 {layout::backup_history(&mut body,w,&backup_view,backup_offset);} else {layout::backup_page(&mut body,w,&backup_view,backup_policy,page==10);}
                }
                if page == 5 {
                    layout::about_updates(&mut body, w, &desktop_status, diagnostics_copied, &updates);
                }
                let mut full = with_titlebar(body, w, key.9);
                full.scroll_to(w, h, &mut scroll_offset);
                cached_scene = Some(full);
                scene_key = Some(key);
            }
            if recording_peek || recording_search || recording_show_panels {
                for control in &mut cached_scene.as_mut().unwrap().controls {
                    if (recording_peek && matches!(control.action, Action::PeekShortcut)) || (recording_search && matches!(control.action, Action::SearchShortcut)) || (recording_show_panels && matches!(control.action, Action::ShowPanelsShortcut)) { control.label = crate::i18n::text("ui-press-a-shortcut").into(); }
                }
            }
            if let Some((percentage, text)) = &style_input {
                for control in &mut cached_scene.as_mut().unwrap().controls {
                    if matches!(control.action, Action::StyleInput(p) if p == *percentage) { control.label = format!("{text}|"); }
                }
            }
            for control in &mut cached_scene.as_mut().unwrap().controls {
                if matches!(control.action, Action::FontSearch) {
                    let empty = font_editor.hwnd.is_null() || unsafe { GetWindowTextLengthW(font_editor.hwnd) } == 0;
                    let focused = !font_editor.hwnd.is_null() && unsafe { GetFocus() } == font_editor.hwnd;
                    control.selected = !empty;
                    control.label = if empty && !focused { crate::i18n::text("font-search").into() } else { String::new() };
                }
            }
            let scene = cached_scene.as_ref().unwrap();
            if !font_editor.sync(hwnd, page, scene, scale, dark) { return Some(0); }

            let interaction_before = (hover, focus, keyboard_focus, pressed);
            let mut activate = None;
            let mut radius_change = None;
            let mut grid_change = None;
            let mut opacity_change = None;
            let mut strength_change = None;
            let mut channel_change = None;
            match msg {
                WM_MOUSEWHEEL if scene.viewport.is_some() => {
                    let mut pointer = windows_sys::Win32::Foundation::POINT { x: lp as u16 as i16 as i32, y: (lp >> 16) as u16 as i16 as i32 };
                    unsafe { ScreenToClient(hwnd, &raw mut pointer); }
                    if pointer.x as f32 / scale < Tokens::content_x() { return Some(0); }
                    let delta = ((wp >> 16) as u16 as i16) as f32 / 120.0;
                    scroll_offset = (scroll_offset - delta * 64.0).clamp(0.0, scene.scroll_max);
                    scene_key = None; hover = None; pressed = None;
                    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                    return Some(0);
                }
                WM_PAINT => {
                    let now = std::time::Instant::now();
                    let mut enabled = 1i32;
                    unsafe {
                        SystemParametersInfoW(
                            SPI_GETCLIENTAREAANIMATION,
                            0,
                            (&raw mut enabled).cast(),
                            0,
                        );
                    }
                    let mut positions = std::collections::HashMap::new();
                    let mut animating = false;
                    for (i, control) in scene.controls.iter().enumerate().filter(|(_, c)| c.is_toggle())
                    {
                        let to = if control.selected { 1.0 } else { 0.0 };
                        let motion = toggle_motion.entry(i).or_insert_with(|| ToggleMotion::settled(to, now));
                        let value = motion.retarget(to, now, enabled != 0);
                        animating |= (value - to).abs() > 0.0001;
                        positions.insert(i, value);
                    }
                    unsafe {
                        if animating != toggle_timer_running {
                            toggle_timer_running = animating && SetTimer(hwnd, TOGGLE_TIMER, USER_TIMER_MINIMUM, None) != 0;
                            if !toggle_timer_running { KillTimer(hwnd, TOGGLE_TIMER); }
                        }
                    }
                    unsafe {
                        let mut ps = PAINTSTRUCT::default();
                        BeginPaint(hwnd, &raw mut ps);
                        EndPaint(hwnd, &raw const ps);
                    }
                    if bounds.right > 0 && bounds.bottom > 0 {
                        paint_recovery.paint(hwnd, &mut surface, "pane.settings", |surface| -> windows::core::Result<bool> {
                            if surface.is_none() {
                                *surface = Some(composition::Surface::new_settings(
                                    windows::Win32::Foundation::HWND(hwnd),
                                )?);
                                composition::Surface::disable_window_shadow(
                                    windows::Win32::Foundation::HWND(hwnd),
                                )?;
                            }
                            let surface = surface.as_mut().unwrap();
                            surface.theme(windows::Win32::Foundation::HWND(hwnd), dark);
                            surface.material(windows::Win32::Foundation::HWND(hwnd), settings_backdrop(appearance.1, dark));
                            let Some(target) =
                                surface.try_begin_frame(bounds.right as u32, bounds.bottom as u32)? else {
                                    return Ok(false);
                                };
                            painter.paint(
                                &target,
                                &scene,
                                w,
                                h,
                                scale,
                                dark,
                                surface.native,
                                hover,
                                if keyboard_focus { focus } else { None },
                                &positions,
                            )?;
                            surface.end_frame()?;
                            Ok(true)
                        });
                    }
                }
                WM_TIMER if wp == TOGGLE_TIMER => {}
                WM_ERASEBKGND => return Some(1),
                WM_SIZE => {
                    hover = None;
                }
                WM_ACTIVATE => {
                    scene_key = None;
                    if wp & 0xffff != WA_INACTIVE as usize && page == 13 {
                        if let Err(error) = start_login_operation(&mut startup, hwnd, None) { window::error(&error); }
                    }
                    if wp & 0xffff == WA_INACTIVE as usize {
                        hover = None;
                        pressed = None;
                        keyboard_focus = false;
                    }
                }
                WM_MOUSELEAVE | WM_NCMOUSEMOVE => {
                    hover = None;
                }
                WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP => {
                    if msg == WM_MOUSEMOVE {
                        unsafe {
                            let mut tracking = TRACKMOUSEEVENT {
                                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                                dwFlags: TME_LEAVE,
                                hwndTrack: hwnd,
                                dwHoverTime: 0,
                            };
                            TrackMouseEvent(&raw mut tracking);
                        }
                    }
                    let x = (lp as u16 as i16) as f32 / scale;
                    let y = ((lp >> 16) as u16 as i16) as f32 / scale;
                    if msg == WM_LBUTTONDOWN && !font_editor.hwnd.is_null() && font_search_hit(scene, x, y) {
                        if scene.controls.iter().any(|c| matches!(c.action, Action::FontSearch) && c.selected && x >= c.bounds.right - 40.0) {
                            unsafe { SetWindowTextW(font_editor.hwnd, windows_sys::w!("")); SetFocus(font_editor.hwnd); }
                            font_choices = all_fonts.clone(); scroll_offset = 0.0; scene_key = None;
                            unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                            return Some(0);
                        }
                        // Color-keyed EDIT pixels hit the owner; forward the click so
                        // the native editor can position the caret and capture dragging.
                        focus = scene.controls.iter().position(|c| matches!(c.action, Action::FontSearch));
                        let mut point = windows_sys::Win32::Foundation::POINT {
                            x: lp as u16 as i16 as i32, y: (lp >> 16) as u16 as i16 as i32,
                        };
                        unsafe {
                            MapWindowPoints(hwnd, font_editor.hwnd, &raw mut point, 1);
                            SetFocus(font_editor.hwnd);
                            SendMessageW(font_editor.hwnd, WM_LBUTTONDOWN, wp,
                                ((point.x as u16 as u32) | ((point.y as u16 as u32) << 16)) as isize);
                        }
                        return Some(0);
                    }
                    if let Some(thumb) = scene.scroll_thumb() {
                        let viewport = scene.viewport.unwrap();
                        if msg == WM_LBUTTONDOWN && x >= viewport.right - 16.0 && y >= viewport.top && y <= viewport.bottom {
                            scroll_drag = Some(if y >= thumb.top && y <= thumb.bottom { y - thumb.top } else { (thumb.bottom - thumb.top) / 2.0 });
                            unsafe { SetCapture(hwnd); }
                        }
                        if let Some(grab) = scroll_drag {
                            let travel = viewport.bottom - viewport.top - 16.0 - (thumb.bottom - thumb.top);
                            scroll_offset = ((y - grab - viewport.top - 8.0) / travel.max(1.0) * scene.scroll_max).clamp(0.0, scene.scroll_max);
                            if msg == WM_LBUTTONUP { scroll_drag = None; unsafe { ReleaseCapture(); } }
                            scene_key = None; hover = None; pressed = None;
                            unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                            return Some(0);
                        }
                    }
                    let hit = scene
                        .controls
                        .iter()
                        .position(|c| c.enabled && scene.accepts_pointer(c, x, y));
                    hover = hit;
                    if msg == WM_LBUTTONDOWN {
                        pressed = hit;
                        focus = hit;
                        keyboard_focus = false;
                        unsafe {
                            SetFocus(hwnd);
                            SetCapture(hwnd);
                        }
                    }
                    if let Some(control) = pressed.and_then(|i| scene.controls.get(i)) {
                        if let Action::Channel(channel, _) = control.action {
                            let fraction = controls::slider_fraction(control.bounds, x);
                            channel_change = Some((channel, (fraction * 255.0).round() as u8));
                        }
                        if matches!(control.action, Action::Strength(_)) {
                            strength_change = Some((controls::slider_fraction(control.bounds, x) * 100.0).round() as u8);
                        }
                        if matches!(control.action, Action::Opacity(_)) {
                            opacity_change = Some((controls::slider_fraction(control.bounds, x) * 100.0).round() as u8);
                        }
                        if let Action::GridSize(_) = control.action {
                            let fraction = controls::slider_fraction(control.bounds, x);
                            grid_change = Some(grid_slider_value(fraction));
                        }
                        if matches!(control.action, Action::Radius(_)) {
                            radius_change = Some(radius_from_pointer(control.bounds, x));
                        }
                    }
                    if msg == WM_LBUTTONUP {
                        if pressed.take() == hit {
                            activate = hit;
                        }
                        unsafe {
                            ReleaseCapture();
                        }
                    }
                }
                WM_CAPTURECHANGED | WM_CANCELMODE => {
                    scroll_drag = None;
                    pressed = None;
                }
                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    if recording_search || recording_show_panels {
                        if lp & (1 << 30) != 0 { return Some(0); }
                        let key = wp as u16;
                        if matches!(key, VK_CONTROL | VK_SHIFT | VK_MENU | VK_LWIN | VK_RWIN) { return Some(0); }
                        if key != VK_ESCAPE {
                            let value = search_hotkey::Shortcut { key, modifiers: peek::modifier_bits(&keyboard::Modifiers::current()) };
                            let result = if recording_show_panels { show_hotkey::save(&state.borrow().store, value) } else { search_hotkey::save(&state.borrow().store, value) };
                            if let Err(error) = result { window::error(&error); return Some(0); }
                        }
                        recording_search = false;
                        recording_show_panels = false;
                        unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                        return Some(0);
                    }
                    if recording_peek {
                        if lp & (1 << 30) != 0 { return Some(0); }
                        let key = wp as u16;
                        if matches!(key, VK_CONTROL | VK_SHIFT | VK_MENU | VK_LWIN | VK_RWIN) { return Some(0); }
                        if key != VK_ESCAPE {
                            let bits = peek::modifier_bits(&keyboard::Modifiers::current());
                            if !peek::valid_shortcut(key, bits) {
                                window::error(crate::i18n::text("ui-shortcut-conflicts-or-is-unsupported-use-letters-digits-function-key"));
                                return Some(0);
                            }
                            let mut value = peek::settings(); value.key = key; value.modifiers = bits;
                            let saved = peek::save(&state.borrow().store, value);
                            if let Err(error) = saved { window::error(&error); }
                        }
                        recording_peek = false;
                        unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                        return Some(0);
                    }
                    if msg == WM_SYSKEYDOWN { return None; }
                    if wp == VK_ESCAPE as usize {
                        if page == 7 {
                            page = 0; scene_key = None; focus = None;
                            unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                            return Some(0);
                        }
                        unsafe {
                            PostMessageW(hwnd, WM_CLOSE, 0, 0);
                        }
                        return Some(0);
                    }
                    if let Some(control) = focus.and_then(|i| scene.controls.get(i)) {
                        if let Action::Channel(channel, value) = control.action {
                            channel_change = match wp as u16 {
                                VK_LEFT => Some(value.saturating_sub(1)), VK_RIGHT => Some(value.saturating_add(1)),
                                VK_HOME => Some(0), VK_END => Some(255), _ => None,
                            }.map(|value| (channel, value));
                        }
                        if let Action::Strength(value) = control.action {
                            strength_change = match wp as u16 {
                                VK_LEFT => Some(value.saturating_sub(1)), VK_RIGHT => Some((value + 1).min(100)),
                                VK_HOME => Some(0), VK_END => Some(100), _ => None,
                            };
                        }
                        if let Action::Opacity(value) = control.action {
                            opacity_change = match wp as u16 {
                                VK_LEFT => Some(value.saturating_sub(1)), VK_RIGHT => Some((value + 1).min(100)),
                                VK_HOME => Some(0), VK_END => Some(100), _ => None,
                            };
                        }
                        if let Action::GridSize(value) = control.action {
                            let range = grid_range();
                            grid_change = match wp as u16 {
                                VK_LEFT => Some((value - 1.0).max(range.0)),
                                VK_RIGHT => Some((value + 1.0).min(range.1)),
                                VK_HOME => Some(range.0), VK_END => Some(range.1), _ => None,
                            };
                        }
                        if let Action::Radius(value) = control.action {
                            radius_change = match wp as u16 {
                                VK_LEFT => Some((value - 0.1).max(0.0)),
                                VK_RIGHT => Some((value + 0.1).min(24.0)),
                                VK_HOME => Some(0.0),
                                VK_END => Some(24.0),
                                _ => None,
                            };
                        }
                    }
                    if scene.viewport.is_some() && (wp == VK_NEXT as usize || wp == VK_PRIOR as usize) {
                        let step = scene.viewport.unwrap().bottom - scene.viewport.unwrap().top - 16.0;
                        scroll_offset = (scroll_offset + if wp == VK_NEXT as usize { step } else { -step }).clamp(0.0, scene.scroll_max);
                        scene_key = None;
                        unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                    } else if wp == VK_TAB as usize || wp == VK_DOWN as usize || wp == VK_UP as usize {
                        keyboard_focus = true;
                        let n = scene.controls.len();
                        let backwards = wp == VK_UP as usize
                            || (wp == VK_TAB as usize
                                && unsafe { GetKeyState(VK_SHIFT as i32) } < 0);
                        focus = Some(match focus {
                            Some(i) if backwards => (i + n - 1) % n,
                            Some(i) => (i + 1) % n,
                            None => {
                                if backwards {
                                    n - 1
                                } else {
                                    0
                                }
                            }
                        });
                        for _ in 0..n {
                            if scene.controls[focus.unwrap()].enabled { break; }
                            let i = focus.unwrap();
                            focus = Some(if backwards { (i + n - 1) % n } else { (i + 1) % n });
                        }
                        if let (Some(index), Some(viewport)) = (focus, scene.viewport) {
                            let c = &scene.controls[index];
                            if !matches!(c.kind, ControlKind::Caption) && c.bounds.left >= Tokens::content_x() && (!scene.fixed_list() || Scene::list_row(c)) {
                                let delta = if c.bounds.top < viewport.top + 8.0 { c.bounds.top - viewport.top - 8.0 }
                                    else if c.bounds.bottom > viewport.bottom - 8.0 { c.bounds.bottom - viewport.bottom + 8.0 } else { 0.0 };
                                if delta != 0.0 { scroll_offset = (scroll_offset + delta).clamp(0.0, scene.scroll_max); scene_key = None; }
                            }
                        }
                    } else if wp == VK_SPACE as usize || wp == VK_RETURN as usize {
                        activate = focus;
                    }
                }
                _ => return None,
            }
            // Keyboard steps persist immediately; pointer gestures share one
            // preview/commit path regardless of the material property edited.
            let mut apply_material = |backdrop| {
                if msg == WM_KEYDOWN {
                    if let Err(error) = handle(&state, selected, Event::Material(backdrop)) { window::error(&error); }
                } else {
                    pending_styles.material.get_or_insert(appearance.1);
                    events::preview_material(&mut state.borrow_mut(), backdrop);
                }
            };
            if let Some((channel, value)) = channel_change.filter(|_| available) {
                if let Backdrop::Solid { color, opacity } = appearance.1 {
                    let backdrop = Backdrop::Solid { color: color_channel(color, channel, value), opacity };
                    apply_material(backdrop);
                }
            }
            if let Some(value) = strength_change.filter(|_| available) {
                let backdrop = appearance.1.with_strength(value);
                apply_material(backdrop);
            }
            if let Some(value) = opacity_change.filter(|_| available) {
                if let Backdrop::Solid { color, .. } = appearance.1 {
                    let backdrop = Backdrop::Solid { color, opacity: f32::from(value) / 100.0 };
                    apply_material(backdrop);
                }
            }
            if available && matches!(msg, WM_LBUTTONUP | WM_CAPTURECHANGED | WM_CANCELMODE) {
                if let Some(original) = pending_styles.material.take() {
                    let saved = events::commit_material(&mut state.borrow_mut(), original);
                    if let Err(error) = saved { window::error(&error); }
                }
            }
            if let Some(value) = grid_change.filter(|_| available) {
                if msg == WM_KEYDOWN {
                    if let Err(error) = handle(&state, selected, Event::SetIconGrid(value)) { window::error(&error); }
                } else {
                    let mut owner = state.borrow_mut();
                    let options = owner.workspace.pane_options();
                    pending_styles.grid.get_or_insert(options.grid_scale);
                    events::preview_grid(&mut owner, value);
                }
            }
            if available && matches!(msg, WM_LBUTTONUP | WM_CAPTURECHANGED | WM_CANCELMODE) {
                if let Some(original) = pending_styles.grid.take() {
                    let saved = events::commit_grid(&mut state.borrow_mut(), original);
                    if let Err(error) = saved { window::error(&error); }
                }
            }
            if let Some(radius) = radius_change.filter(|_| available) {
                if msg == WM_KEYDOWN {
                    if let Err(error) = handle(&state, selected, Event::SetCornerRadius(radius)) {
                        window::error(&error);
                    }
                } else {
                    let mut owner = state.borrow_mut();
                    pending_styles.radius.get_or_insert(owner.workspace.pane_options().corner_radius);
                    events::preview_radius(&mut owner, radius);
                }
            }
            if available && matches!(msg, WM_LBUTTONUP | WM_CAPTURECHANGED | WM_CANCELMODE) {
                if let Some(original) = pending_styles.radius.take() {
                    let saved = events::commit_radius(&mut state.borrow_mut(), original);
                    if let Err(error) = saved {
                        window::error(&error);
                    }
                }
            }
            if let Some(c) = activate
                .filter(|_| available)
                .and_then(|i| scene.controls.get(i))
                .filter(|c| c.enabled)
            {
                if actions::execute(actions::Context {
                    state: &state, hwnd, selected, appearance, dark, scale, backup_view: &backup_view, desktop_status: &desktop_status, font_search: font_editor.hwnd, all_fonts: &all_fonts,
                    page: &mut page,
                    focus: &mut focus,
                    hover: &mut hover,
                    backup_offset: &mut backup_offset,
                    scroll_offset: &mut scroll_offset,
                    folder_entry_mode: &mut folder_entry_mode,
                    folder_defaults: &mut folder_defaults,
                    layout_defaults: &mut layout_defaults,
                    style_input: &mut style_input,
                    startup: &mut startup,
                    diagnostics_copied: &mut diagnostics_copied,
                    skill_prompt_copied: &mut skill_prompt_copied,
                    updates: &mut updates,
                    recording_show_panels: &mut recording_show_panels,
                    recording_search: &mut recording_search,
                    recording_peek: &mut recording_peek,
                    painter: &mut painter,
                    font_choices: &mut font_choices,
                    fonts_loaded: &mut fonts_loaded,
                    font_load_failed: &mut font_load_failed,
                    toggle_motion: &mut toggle_motion,
                }, c) { scene_key = None; }
            }
            if msg != WM_PAINT
                && (scene_changed
                    || interaction_before != (hover, focus, keyboard_focus, pressed)
                    || activate.is_some()
                    || grid_change.is_some()
                    || radius_change.is_some_and(|radius| radius != options.corner_radius)
                    || matches!(msg, WM_SIZE | WM_ACTIVATE | WM_TIMER))
            {
                unsafe {
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            }
            Some(0)
        })
        .create()
        .map_err(|e| e.to_string())?;
    crate::app_icon::apply(window.hwnd().cast())?;
    unsafe {
        if windows_sys::Win32::UI::Shell::SetWindowSubclass(
            window.hwnd().cast(),
            Some(frame_proc),
            0x4c505346,
            0,
        ) == 0
        {
            return Err(crate::i18n::text("ui-could-not-initialize-settings-window-frame").into());
        }
        let hwnd = window.hwnd().cast();
        let dpi = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let mut monitor = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(
            MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
            &raw mut monitor,
        ) != 0
        {
            let work = monitor.rcWork;
            let width = ((900.0 * dpi).round() as i32).min(work.right - work.left);
            let height = ((DEFAULT_HEIGHT as f32 * dpi).round() as i32).min(work.bottom - work.top);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                work.left + (work.right - work.left - width) / 2,
                work.top + (work.bottom - work.top - height) / 2,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
        // Prepare material and content at the final size before exposing the HWND.
        SendMessageW(hwnd, WM_PAINT, 0, 0);
        // Cloaking keeps the visible HWND in DWM composition without exposing
        // a partial frame. A hidden HWND cannot prepare host backdrop sampling.
        if cloak(hwnd, true).is_ok() {
            prepared.set(true);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            PostMessageW(hwnd, PREPARE_REVEAL, 0, 0);
        } else {
            prepared.set(true);
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
    }
    state.borrow_mut().settings = Some(window);
    Ok(())
}
