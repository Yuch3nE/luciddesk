//! One non-modal window for rules and the current organize operation.
use super::*;
use crate::pane::tooltip::Tooltip;
use std::cell::Cell;
use std::sync::mpsc::{self, Sender};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;

pub(super) const SELECT_TAB: u32 = WM_APP + 100;
const RULE_MOTION_TIMER: usize = 0x4c4f4d;
#[cfg(test)]
pub(super) const INSPECT_TAB: u32 = WM_APP + 101;

pub(super) fn show(state: &Rc<RefCell<PaneApp>>, tab: Tab) -> Result<(), String> {
    let existing = state
        .borrow()
        .organizer
        .as_ref()
        .map(|window| window.hwnd());
    if let Some(hwnd) = existing {
        unsafe {
            activate(hwnd.cast(), tab);
        }
        return Ok(());
    }
    let weak = Rc::downgrade(state);
    let prepared = Rc::new(Cell::new(false));
    let show_prepared = Rc::clone(&prepared);
    let (dropdown_sender, dropdown_receiver) = mpsc::channel();
    let mut organizer = Organizer::new()?;
    let window = windows_window::Window::new(t("organizer-window-title"))
        .size(780, 620)
        .style(WS_OVERLAPPEDWINDOW)
        .ex_style(WS_EX_APPWINDOW | WS_EX_NOREDIRECTIONBITMAP | WS_EX_NOACTIVATE)
        .on_message(move |raw, msg, wp, lp| {
            let hwnd = raw.cast();
            if unsafe { defer_show(msg, lp, show_prepared.get()) } {
                return Some(0);
            }
            let state = weak.upgrade()?;
            let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
            match msg {
                WM_NCCALCSIZE | WM_NCPAINT | WM_DESTROY => {}
                WM_NCACTIVATE | WM_ERASEBKGND => return Some(1),
                WM_NCHITTEST => {
                    let mut point = client_point(lp);
                    let bounds = client_bounds(hwnd);
                    unsafe {
                        ScreenToClient(hwnd, &raw mut point);
                    }
                    return Some(frame_hit(
                        point.x as f32 / scale,
                        point.y as f32 / scale,
                        bounds.right as f32 / scale,
                        bounds.bottom as f32 / scale,
                        unsafe { IsZoomed(hwnd) } != 0,
                    ) as isize);
                }
                WM_GETMINMAXINFO => {
                    let info = unsafe { &mut *(lp as *mut MINMAXINFO) };
                    let monitor = monitor_info(hwnd);
                    let work = monitor.rcWork;
                    info.ptMinTrackSize.x = ((480.0 * scale) as i32).min(work.right - work.left);
                    info.ptMinTrackSize.y = ((MIN_WINDOW_HEIGHT * scale) as i32).min(work.bottom - work.top);
                    info.ptMaxPosition.x = work.left - monitor.rcMonitor.left;
                    info.ptMaxPosition.y = work.top - monitor.rcMonitor.top;
                    info.ptMaxSize.x = work.right - work.left;
                    info.ptMaxSize.y = work.bottom - work.top;
                }
                WM_DPICHANGED => {
                    position(hwnd, unsafe { &*(lp as *const RECT) });
                    let _ = crate::app_icon::apply(hwnd);
                    redraw(hwnd);
                }
                SELECT_TAB => organizer.select(
                    &state,
                    hwnd,
                    if wp == Tab::Organize as usize {
                        Tab::Organize
                    } else {
                        Tab::Rules
                    },
                    lp != 0,
                ),
                #[cfg(test)]
                INSPECT_TAB => {
                    return Some(if wp == 0 {
                        organizer.tab as isize + 1
                    } else {
                        isize::from(
                            organizer.editor.previewing()
                                || organizer.editor.organize_error.is_some(),
                        )
                    });
                }
                crate::i18n::CHANGED => {
                    unsafe {
                        SetWindowTextW(hwnd, crate::i18n::wide("organizer-window-title"));
                    }
                    redraw(hwnd);
                }
                WM_CLOSE => {
                    let Ok(mut owner) = state.try_borrow_mut() else {
                        unsafe {
                            PostMessageW(hwnd, WM_CLOSE, 0, 0);
                        }
                        return Some(0);
                    };
                    let closing = owner.organizer.take();
                    drop(owner);
                    organizer.editor.discard();
                    organizer.tip = None;
                    unsafe {
                        KillTimer(hwnd, ORGANIZE_TIMER);
                        KillTimer(hwnd, RULE_MOTION_TIMER);
                    }
                    drop(closing);
                }
                WM_TIMER if wp == ORGANIZE_TIMER => {
                    if organizer.editor.poll_organize() {
                        organizer.tab_scroll[Tab::Organize as usize] = 0.0;
                        if organizer.tab == Tab::Organize {
                            organizer.scroll = 0.0;
                        }
                        unsafe {
                            KillTimer(hwnd, ORGANIZE_TIMER);
                        }
                        redraw(hwnd);
                    }
                }
                WM_TIMER if wp == RULE_MOTION_TIMER => {
                    let now = Instant::now();
                    if !organizer.editor.tick_rule_animations(now) {
                        unsafe { KillTimer(hwnd, RULE_MOTION_TIMER); }
                    }
                    redraw(hwnd);
                }
                WM_PAINT => organizer.paint(&state, hwnd, scale),
                WM_SETTINGCHANGE => {
                    if !scrollbar::animations_enabled() {
                        organizer.finish_rule_animations(hwnd);
                    }
                    redraw(hwnd);
                }
                WM_THEMECHANGED => redraw(hwnd),
                WM_SIZING => {
                    organizer.auto_fit_preview = false;
                    return None;
                }
                WM_SIZE => {
                    if wp == SIZE_MAXIMIZED as usize {
                        organizer.auto_fit_preview = false;
                    }
                    organizer.clear_hover();
                    redraw(hwnd);
                }
                WM_ACTIVATE if wp & 0xffff == WA_INACTIVE as usize => {
                    organizer.clear_hover();
                    organizer.pressed = None;
                    redraw(hwnd);
                }
                SELECT_SETTING => {
                    if let Ok(control) = dropdown_receiver.try_recv() {
                        organizer.invoke(&state, hwnd, &control, scale, &dropdown_sender);
                    }
                }
                WM_MOUSEWHEEL => {
                    let delta = (wp >> 16) as u16 as i16;
                    organizer.scroll_by(hwnd, -f32::from(delta) / 120.0 * 64.0);
                }
                WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP => {
                    if msg == WM_LBUTTONDOWN {
                        organizer.layout(&state, hwnd, client_bounds(hwnd), scale);
                    }
                    if let Some(control) = organizer.pointer(hwnd, msg, client_point(lp), scale) {
                        organizer.invoke(&state, hwnd, &control, scale, &dropdown_sender);
                    }
                }
                WM_MOUSELEAVE => {
                    organizer.clear_hover();
                    redraw(hwnd);
                }
                WM_CAPTURECHANGED | WM_CANCELMODE => {
                    organizer.pressed = None;
                    organizer.scroll_grab = None;
                }
                WM_KEYDOWN | WM_SYSKEYDOWN | CLASSIFICATION_KEY => {
                    organizer.layout(&state, hwnd, client_bounds(hwnd), scale);
                    if !organizer.key(&state, hwnd, (msg, wp as u16, lp), scale, &dropdown_sender) {
                        return None;
                    }
                }
                _ => return None,
            }
            Some(0)
        })
        .create()
        .map_err(|error| error.to_string())?;

    let hwnd = window.hwnd().cast();
    crate::app_icon::apply(hwnd)?;
    unsafe {
        if windows_sys::Win32::UI::Shell::SetWindowSubclass(hwnd, Some(frame_proc), 0x4c4f5247, 0)
            == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        crate::window_visibility::prepare_canvas(hwnd);
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let work = monitor_info(hwnd).rcWork;
        let width = ((780.0 * scale).round() as i32).min(work.right - work.left);
        let height = ((620.0 * scale).round() as i32).min(work.bottom - work.top);
        let left = work.left + (work.right - work.left - width) / 2;
        let top = work.top + (work.bottom - work.top - height) / 2;
        position(
            hwnd,
            &RECT {
                left,
                top,
                right: left + width,
                bottom: top + height,
            },
        );
    }
    state.borrow_mut().organizer = Some(window);
    unsafe {
        SendMessageW(hwnd, SELECT_TAB, tab as usize, 1);
        SendMessageW(hwnd, WM_PAINT, 0, 0);
        prepared.set(true);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
    Ok(())
}

unsafe fn activate(hwnd: HWND, tab: Tab) {
    unsafe {
        SendMessageW(hwnd, SELECT_TAB, tab as usize, 1);
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
}

struct Organizer {
    tab: Tab,
    editor: Editor,
    scene: Option<Scene>,
    rules: Option<SettingValue>,
    appearance: (PanelTheme, Backdrop),
    painter: Painter,
    typography: (String, &'static str),
    surface: Option<composition::Surface>,
    paint_recovery: composition::PaintRecovery,
    scroll: f32,
    tab_scroll: [f32; 2],
    opened_preview: bool,
    auto_fit_preview: bool,
    scroll_grab: Option<f32>,
    hover: Option<usize>,
    hovered_destination: Option<(usize, usize)>,
    pressed: Option<usize>,
    focus: Option<usize>,
    tip: Option<Tooltip>,
}

impl Organizer {
    fn new() -> Result<Self, String> {
        Ok(Self {
            tab: Tab::Rules,
            editor: Editor::default(),
            scene: None,
            rules: None,
            appearance: (PanelTheme::System, Backdrop::Mica),
            painter: Painter::new().map_err(|error| error.to_string())?,
            typography: (fonts::family(), crate::i18n::language()),
            surface: None,
            paint_recovery: Default::default(),
            scroll: 0.0,
            tab_scroll: [0.0; 2],
            opened_preview: false,
            auto_fit_preview: true,
            scroll_grab: None,
            hover: None,
            hovered_destination: None,
            pressed: None,
            focus: None,
            tip: None,
        })
    }

    fn select(&mut self, state: &Rc<RefCell<PaneApp>>, hwnd: HWND, tab: Tab, entry: bool) {
        if state.try_borrow_mut().is_err() {
            return;
        }
        if self.tab != tab {
            self.finish_rule_animations(hwnd);
            self.tab_scroll[self.tab as usize] = self.scroll;
            self.editor.hide_inputs();
            self.tab = tab;
            self.scroll = self.tab_scroll[tab as usize];
            self.clear_hover();
            self.focus = Some(tab as usize);
            unsafe {
                SetFocus(hwnd);
            }
        }
        if tab == Tab::Organize && self.editor.applying.is_none() && (entry || !self.opened_preview)
        {
            self.opened_preview = true;
            self.run_command(state, hwnd, Command::Run);
        }
        redraw(hwnd);
    }

    fn layout(&mut self, state: &Rc<RefCell<PaneApp>>, hwnd: HWND, bounds: RECT, scale: f32) -> f32 {
        let (width, height) = (bounds.right as f32 / scale, bounds.bottom as f32 / scale);
        let mut preferred_height = height;
        if let Ok(owner) = state.try_borrow() {
            let appearance = owner.workspace.appearance().or_else(|| {
                owner
                    .workspace
                    .panels()
                    .first()
                    .map(|panel| (panel.theme(), panel.backdrop()))
            });
            self.appearance = appearance.unwrap_or((PanelTheme::System, Backdrop::Mica));
            let rules = owner
                .store
                .settings()
                .ok()
                .and_then(|mut values| values.remove(KEY));
            if self.rules != rules {
                self.rules = rules;
                self.editor.collapse_details();
                self.pressed = None;
            }
            let mut scene = organizer_titlebar(
                window_scene(
                    (width, height),
                    visible_material(self.appearance.1, supports_mica()),
                    self.tab,
                    owner.workspace.panels(),
                    self.rules.as_ref(),
                    &self.editor,
                    owner.session.is_some() && owner.last_organize.is_some(),
                ),
                width,
                unsafe { IsZoomed(hwnd) } != 0,
            );
            if self.auto_fit_preview && self.tab == Tab::Organize && self.editor.previewing() {
                preferred_height = preferred_preview_height(&scene);
            }
            if let Some(command) = self.editor.take_reveal(self.tab)
                && let Some(index) = scene
                    .controls
                    .iter()
                    .position(|control| command.is_control(&control.action))
            {
                self.focus = Some(index);
                let bounds = scene.controls[index].bounds;
                if !Scene::fixed_control(&scene.controls[index]) {
                    let bottom = height - scene.footer_height;
                    if bounds.top - self.scroll < scene.content_top + Tokens::GAP {
                        self.scroll = bounds.top - scene.content_top - Tokens::GAP;
                    } else if bounds.bottom - self.scroll > bottom - Tokens::GAP {
                        self.scroll = bounds.bottom - bottom + Tokens::GAP;
                    }
                }
            }
            scene.scroll_to(width, height, &mut self.scroll);
            self.scene = Some(scene);
        }
        preferred_height
    }

    fn paint(&mut self, state: &Rc<RefCell<PaneApp>>, hwnd: HWND, scale: f32) {
        let mut bounds = client_bounds(hwnd);
        unsafe {
            let mut paint = PAINTSTRUCT::default();
            BeginPaint(hwnd, &raw mut paint);
            EndPaint(hwnd, &raw const paint);
        }
        if bounds.right <= 0 || bounds.bottom <= 0 {
            return;
        }
        let preferred_height = self.layout(state, hwnd, bounds, scale);
        if preferred_height != bounds.bottom as f32 / scale && resize_preview(hwnd, preferred_height, scale) {
            bounds = client_bounds(hwnd);
            self.clear_hover();
            self.pressed = None;
            self.layout(state, hwnd, bounds, scale);
        }
        let (width, height) = (bounds.right as f32 / scale, bounds.bottom as f32 / scale);
        let Some(scene) = &self.scene else {
            return;
        };
        let typography = (fonts::family(), crate::i18n::language());
        if self.typography != typography
            && let Ok(painter) = Painter::new()
        {
            self.painter = painter;
            self.typography = typography;
        }
        let dark = theme::is_dark(self.appearance.0);
        if self.tab == Tab::Rules {
            self.editor.sync(scene, scale, dark);
        }
        self.paint_recovery.paint(
            hwnd,
            &mut self.surface,
            "pane.organizer",
            |surface| -> windows::core::Result<bool> {
                let raw = windows::Win32::Foundation::HWND(hwnd);
                if surface.is_none() {
                    *surface = Some(composition::Surface::new_settings(raw)?);
                }
                let surface = surface.as_mut().unwrap();
                surface.theme(raw, dark);
                surface.material(raw, settings_backdrop(self.appearance.1, dark))?;
                let Some(target) =
                    surface.try_begin_frame(bounds.right as u32, bounds.bottom as u32)?
                else {
                    return Ok(false);
                };
                self.painter.paint(
                    &target,
                    scene,
                    width,
                    height,
                    scale,
                    dark,
                    surface.native,
                    self.hover,
                    self.focus,
                    &HashMap::new(),
                )?;
                surface.end_frame()?;
                Ok(true)
            },
        );
    }

    fn invoke(
        &mut self,
        state: &Rc<RefCell<PaneApp>>,
        hwnd: HWND,
        control: &Control,
        scale: f32,
        sender: &Sender<Control>,
    ) {
        match control.action {
            Action::Classification(command) => self.run_command(state, hwnd, command),
            Action::OrganizeTab(tab) => self.select(state, hwnd, tab, false),
            Action::Dropdown(_) => {
                controls::open_dropdown(hwnd, self.appearance, scale, control, sender.clone())
            }
            Action::Window(command) => unsafe {
                PostMessageW(hwnd, WM_SYSCOMMAND, command as usize, 0);
            },
            _ => {}
        }
    }

    fn run_command(&mut self, state: &Rc<RefCell<PaneApp>>, hwnd: HWND, command: Command) {
        if self.tab != command.tab() || state.try_borrow_mut().is_err() {
            return;
        }
        let previous = self.editor.expanded;
        self.editor.execute(state, hwnd, command);
        if matches!(command, Command::ToggleDetails(_)) && previous != self.editor.expanded
            && scrollbar::animations_enabled()
        {
            self.editor.animate_details(previous, Instant::now());
            self.schedule_rule_animations(hwnd);
        }
        if self.tab == Tab::Organize {
            if matches!(command, Command::Run | Command::CancelPreview)
                || self.editor.applying.is_some()
            {
                self.scroll = 0.0;
                self.clear_hover();
            }
            if self.editor.applying.is_some() {
                self.focus = Some(Tab::Organize as usize);
            }
        }
        if matches!(command, Command::Save | Command::Cancel) && !self.editor.editing() {
            unsafe {
                SetFocus(hwnd);
            }
        }
        redraw(hwnd);
    }

    fn clear_hover(&mut self) {
        self.hover = None;
        self.hovered_destination = None;
        if let Some(tip) = &mut self.tip {
            tip.hide();
        }
    }

    fn scroll_by(&mut self, hwnd: HWND, delta: f32) {
        if let Some(scene) = &self.scene {
            self.scroll = (self.scroll + delta).clamp(0.0, scene.scroll_max);
            self.clear_hover();
            redraw(hwnd);
        }
    }

    fn schedule_rule_animations(&mut self, hwnd: HWND) {
        if unsafe { SetTimer(hwnd, RULE_MOTION_TIMER, USER_TIMER_MINIMUM, None) } == 0 {
            self.editor.finish_rule_animations();
        }
    }

    fn finish_rule_animations(&mut self, hwnd: HWND) {
        self.editor.finish_rule_animations();
        unsafe { KillTimer(hwnd, RULE_MOTION_TIMER); }
    }

    fn pointer(&mut self, hwnd: HWND, msg: u32, point: POINT, scale: f32) -> Option<Control> {
        let (x, y) = (point.x as f32 / scale, point.y as f32 / scale);
        let scene = self.scene.as_ref()?;
        if let (Some(scrollbar), Some(viewport)) = (scene.scrollbar(), scene.viewport) {
            if msg == WM_LBUTTONDOWN
                && x >= viewport.right - luciddesk_window::controls::Scrollbar::GUTTER
                && contains(&viewport, x, y)
            {
                self.scroll_grab = Some(scrollbar.grab(y));
                unsafe {
                    SetCapture(hwnd);
                }
            }
            if let Some(grab) = self.scroll_grab {
                let offset = scrollbar.drag_to(y, grab);
                if msg == WM_LBUTTONUP {
                    self.scroll_grab = None;
                    unsafe {
                        ReleaseCapture();
                    }
                }
                self.scroll_by(hwnd, offset - self.scroll);
                return None;
            }
        }
        let hit = scene
            .controls
            .iter()
            .position(|control| control.enabled && scene.accepts_pointer(control, x, y));
        let destination = if msg == WM_MOUSEMOVE && hit.is_none() {
            destination_tooltip(scene, x, y)
        } else {
            None
        };
        let destination_hit = destination.map(|(hit, _, _)| hit);
        match msg {
            WM_LBUTTONDOWN => {
                self.pressed = hit;
                self.focus = hit;
                if hit.is_some() {
                    unsafe {
                        SetCapture(hwnd);
                    }
                }
                redraw(hwnd);
            }
            WM_LBUTTONUP => {
                let picked = self
                    .pressed
                    .take()
                    .filter(|index| Some(*index) == hit)
                    .and_then(|index| scene.controls.get(index))
                    .cloned();
                unsafe {
                    ReleaseCapture();
                }
                return picked;
            }
            WM_MOUSEMOVE if hit != self.hover || destination_hit != self.hovered_destination => {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                unsafe {
                    TrackMouseEvent(&raw mut track);
                }
                let target = hit
                    .and_then(|index| scene.controls.get(index))
                    .filter(|control| match control.kind {
                        ControlKind::IconButton(_) => true,
                        ControlKind::RuleDisclosure { .. } => {
                            let inset = luciddesk_window::controls::InlineControl::DISCLOSURE_LABEL_INSET + Style::ICON_GAP;
                            components::text_width_with_weight(&control.label, Tokens::SECTION_TITLE_SIZE, windows_canvas::FontWeight(600))
                                > control.bounds.right - control.bounds.left - inset
                        }
                        _ => false,
                    })
                    .map(|icon| (icon.bounds, icon.label.as_str()))
                    .or_else(|| destination.map(|(_, bounds, name)| (bounds, name)));
                if let Some((bounds, label)) = target {
                    if self.tip.is_none() {
                        self.tip = Tooltip::new(hwnd);
                    }
                    if let Some(tip) = &mut self.tip {
                        tip.show(label, RECT {
                            left: (bounds.left * scale).round() as i32,
                            top: (bounds.top * scale).round() as i32,
                            right: (bounds.right * scale).round() as i32,
                            bottom: (bounds.bottom * scale).round() as i32,
                        });
                    }
                } else if let Some(tip) = &mut self.tip {
                    tip.hide();
                }
                self.hovered_destination = destination_hit;
                if self.hover != hit {
                    self.hover = hit;
                    redraw(hwnd);
                }
            }
            _ => {}
        }
        None
    }

    fn key(
        &mut self,
        state: &Rc<RefCell<PaneApp>>,
        hwnd: HWND,
        message: (u32, u16, isize),
        scale: f32,
        sender: &Sender<Control>,
    ) -> bool {
        let (msg, key, lp) = message;
        let input = if msg == CLASSIFICATION_KEY {
            let Some(field) = self.editor.field_for(lp as _) else {
                return true;
            };
            Some(field)
        } else {
            None
        };
        let Some(scene) = &self.scene else {
            return false;
        };
        let focused_tab = self
            .focus
            .and_then(|index| scene.controls.get(index))
            .is_some_and(|control| matches!(control.action, Action::OrganizeTab(_)));
        if (key == VK_TAB && unsafe { GetKeyState(VK_CONTROL as i32) } < 0)
            || (focused_tab && matches!(key, VK_LEFT | VK_RIGHT))
        {
            let tab = if self.tab == Tab::Rules {
                Tab::Organize
            } else {
                Tab::Rules
            };
            self.select(state, hwnd, tab, false);
            return true;
        }
        if key == VK_ESCAPE || (key == VK_RETURN && input.is_some()) {
            let command = match self.tab {
                Tab::Rules if self.editor.editing() => Some(if key == VK_RETURN {
                    Command::Save
                } else {
                    Command::Cancel
                }),
                Tab::Organize if self.editor.previewing() => Some(Command::CancelPreview),
                _ => None,
            };
            if let Some(command) = command {
                self.run_command(state, hwnd, command);
            } else {
                unsafe {
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                }
            }
            return true;
        }
        if let Some(field) = input {
            self.focus = scene
                .controls
                .iter()
                .position(|control| Command::Input(field).is_control(&control.action));
            unsafe {
                SetFocus(hwnd);
            }
        }
        let current = self.focus.and_then(|index| scene.controls.get(index));
        let combo = current.is_some_and(|control| matches!(control.kind, ControlKind::Combo))
            && (key == VK_F4 || (msg == WM_SYSKEYDOWN && key == VK_DOWN));
        if msg == WM_SYSKEYDOWN && !combo {
            return false;
        }
        match key {
            VK_TAB | VK_UP | VK_DOWN if !combo => {
                let n = scene.controls.len();
                let backwards =
                    key == VK_UP || (key == VK_TAB && unsafe { GetKeyState(VK_SHIFT as i32) } < 0);
                let at = self.focus.unwrap_or(if backwards { 0 } else { n - 1 });
                self.focus = (1..=n)
                    .map(|step| {
                        if backwards {
                            (at + n - step) % n
                        } else {
                            (at + step) % n
                        }
                    })
                    .find(|index| scene.controls[*index].enabled);
                if let Some(control) = self.focus.and_then(|index| scene.controls.get(index)) {
                    if let Action::Classification(Command::Input(field)) = control.action {
                        self.editor.focus(field);
                    }
                    if let Some(viewport) = scene.viewport
                        && !Scene::fixed_control(control)
                    {
                        let bounds = control.bounds;
                        let delta = if bounds.top < viewport.top + Tokens::GAP {
                            bounds.top - viewport.top - Tokens::GAP
                        } else {
                            (bounds.bottom - viewport.bottom + Tokens::GAP).max(0.0)
                        };
                        self.scroll_by(hwnd, delta);
                    }
                }
                redraw(hwnd);
            }
            VK_PRIOR | VK_NEXT => {
                let step = scene
                    .viewport
                    .map_or(0.0, |area| area.bottom - area.top - Tokens::ROW_HEIGHT);
                self.scroll_by(hwnd, if key == VK_PRIOR { -step } else { step });
            }
            VK_HOME | VK_END => self.scroll_by(
                hwnd,
                if key == VK_HOME {
                    -self.scroll
                } else {
                    scene.scroll_max - self.scroll
                },
            ),
            VK_SPACE | VK_RETURN | VK_F4 | VK_DOWN => {
                if let Some(control) = current
                    .filter(|control| {
                        control.enabled && (combo || matches!(key, VK_SPACE | VK_RETURN))
                    })
                    .cloned()
                {
                    self.invoke(state, hwnd, &control, scale, sender);
                }
            }
            _ => return false,
        }
        true
    }
}

fn destination_tooltip(scene: &Scene, x: f32, y: f32) -> Option<((usize, usize), Rect, &str)> {
    let viewport = scene.viewport.filter(|viewport| contains(viewport, x, y))?;
    let (row, change) = scene.destination_changes.iter().enumerate()
        .find(|(_, change)| contains(&change.bounds, x, y))?;
    change.destinations().into_iter().enumerate().find_map(|(side, (bounds, destination))| {
        let [mut group, _] = destination.cells(bounds);
        if !destination.truncated || !contains(&group, x, y) {
            return None;
        }
        group.top = group.top.max(viewport.top);
        group.bottom = group.bottom.min(viewport.bottom);
        Some(((row, side), group, destination.group.as_str()))
    })
}

fn redraw(hwnd: HWND) {
    unsafe {
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

fn client_bounds(hwnd: HWND) -> RECT {
    let mut bounds = RECT::default();
    unsafe {
        GetClientRect(hwnd, &raw mut bounds);
    }
    bounds
}

fn client_point(lp: isize) -> POINT {
    POINT {
        x: (lp as u16 as i16).into(),
        y: ((lp >> 16) as u16 as i16).into(),
    }
}

fn monitor_info(hwnd: HWND) -> MONITORINFO {
    let mut monitor = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        if GetMonitorInfoW(
            MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
            &raw mut monitor,
        ) != 0
        {
            return monitor;
        }
    }
    let bounds = RECT {
        right: 780,
        bottom: 620,
        ..Default::default()
    };
    monitor.rcMonitor = bounds;
    monitor.rcWork = bounds;
    monitor
}

fn position(hwnd: HWND, bounds: &RECT) {
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            bounds.left,
            bounds.top,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

fn resize_preview(hwnd: HWND, preferred_height: f32, scale: f32) -> bool {
    let mut window = RECT::default();
    unsafe {
        if IsIconic(hwnd) != 0 || IsZoomed(hwnd) != 0 || GetWindowRect(hwnd, &raw mut window) == 0 {
            return false;
        }
    }
    let work = monitor_info(hwnd).rcWork;
    let height = ((preferred_height * scale).ceil() as i32).min(work.bottom - work.top);
    if window.bottom - window.top == height {
        return false;
    }
    let top = window.top.clamp(work.top, work.bottom - height);
    unsafe {
        SetWindowPos(
            hwnd, std::ptr::null_mut(), window.left, top,
            window.right - window.left, height, SWP_NOZORDER | SWP_NOACTIVATE,
        ) != 0
    }
}

#[test]
fn clipped_destination_names_can_be_read_without_hiding_rule_numbers() {
    let _sta = crate::pane::test_support::apartment();
    let old = "Previously selected destination group with a long descriptive title";
    let new = "Newly selected destination group with a different long descriptive title";
    let mut scene = Scene::empty(Backdrop::Mica, 24.0, 50.0);
    scene.viewport = Some(Rect::from_xywh(24.0, 54.0, 432.0, 40.0));
    scene.destination_changes.push(DestinationChange::new(
        Rect::from_xywh(40.0, 50.0, 400.0, 0.0),
        (old, "Rule 127".into()),
        (new, "Rule 256".into()),
    ));
    let change = &scene.destination_changes[0];
    assert!(change.bounds.right <= 440.0);
    for (index, (bounds, destination)) in change.destinations().into_iter().enumerate() {
        let [group, rule] = destination.cells(bounds);
        assert!(destination.truncated && group.right < rule.left);
        assert!(rule.right <= bounds.right);
        assert!(rule.right - rule.left >= components::text_width(&destination.rule, 12.0));
        let (hit, tooltip, name) = destination_tooltip(&scene, group.left + 1.0, 60.0).unwrap();
        assert_eq!(hit, (0, index));
        assert_eq!(name, [old, new][index]);
        assert_eq!(tooltip.top, 54.0);
        assert!(destination_tooltip(&scene, rule.left + 1.0, 60.0).is_none());
        assert!(destination_tooltip(&scene, group.left + 1.0, 52.0).is_none());
    }
}

#[test]
fn preview_resize_keeps_window_width_and_fits_the_monitor() {
    let window = windows_window::Window::new("Organizer resize test")
        .size(480, 620)
        .create()
        .unwrap();
    let hwnd = window.hwnd().cast();
    let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
    let work = monitor_info(hwnd).rcWork;
    let mut before = RECT::default();
    assert_ne!(unsafe { GetWindowRect(hwnd, &raw mut before) }, 0);
    let preferred = 420.0;
    let expected = ((preferred * scale).ceil() as i32).min(work.bottom - work.top);
    assert_eq!(resize_preview(hwnd, preferred, scale), before.bottom - before.top != expected);
    let mut after = RECT::default();
    assert_ne!(unsafe { GetWindowRect(hwnd, &raw mut after) }, 0);
    assert_eq!(after.bottom - after.top, expected);
    assert_eq!(after.right - after.left, before.right - before.left);
    if before.bottom - before.top != expected {
        assert!(after.top >= work.top && after.bottom <= work.bottom);
    }
    assert!(!resize_preview(hwnd, preferred, scale));
}

#[test]
fn long_preview_keeps_its_review_state_and_footer_when_switching_tabs() {
    let _sta = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
    let window = windows_window::Window::new("Organizer test")
        .create()
        .unwrap();
    let hwnd = window.hwnd().cast();
    let bounds = RECT {
        right: 480,
        bottom: 380,
        ..Default::default()
    };
    let identity = |path: String| ShellIdentity::FileSystem {
        path: path.into(),
        volume_id: None,
        file_id: None,
    };
    let moves: Vec<_> = (0..24)
        .map(|index| {
            let item_name = format!("Report {index}.pdf");
            hybrid::PreviewEntry::Move(hybrid::OrganizeMove {
                identity: identity(item_name.clone()),
                item_name,
                target: PanelId::new(1),
                target_name: "Reports".into(),
                rule: 1,
            })
        })
        .collect();
    let changes = vec![hybrid::PreviewChange::Added(moves[0].clone())];
    let skipped = hybrid::PreviewEntry::Skip {
        identity: identity("Draft.txt".into()),
        item_name: "Draft.txt".into(),
        reason: hybrid::SkipReason::NoMatch,
    };
    let mut organizer = Organizer::new().unwrap();
    organizer.tab = Tab::Organize;
    organizer.opened_preview = true;
    organizer.editor.preview = Some(hybrid::OrganizePreview::fixture(
        moves.into_iter().chain([skipped]).collect(),
        Some(changes),
    ));
    let apply_button_top = |organizer: &Organizer| {
        let scene = organizer.scene.as_ref().unwrap();
        let apply = scene
            .controls
            .iter()
            .find(|control| Command::ApplyPreview.is_control(&control.action))
            .unwrap();
        assert_eq!(apply.label, t("classification-preview-confirm"));
        assert!(scene.accepts_pointer(apply, apply.bounds.left + 2.0, apply.bounds.top + 2.0));
        apply.bounds.top
    };
    let fitted = organizer.layout(&state, hwnd, bounds, 1.0);
    assert_eq!(fitted, MAX_PREVIEW_HEIGHT);
    organizer.auto_fit_preview = false;
    assert_eq!(organizer.layout(&state, hwnd, bounds, 1.0), bounds.bottom as f32);
    let button_top = apply_button_top(&organizer);
    organizer.run_command(&state, hwnd, Command::ToggleSkipped);
    organizer.layout(&state, hwnd, bounds, 1.0);
    let scroll = organizer.scroll;
    assert!(scroll > 0.0);
    let scene = organizer.scene.as_ref().unwrap();
    assert!(
        scene
            .text
            .iter()
            .any(|(_, text, _)| text == t("classification-skip-no-match"))
    );
    assert_eq!(apply_button_top(&organizer), button_top);

    organizer.select(&state, hwnd, Tab::Rules, false);
    organizer.layout(&state, hwnd, bounds, 1.0);
    organizer.select(&state, hwnd, Tab::Organize, false);
    organizer.layout(&state, hwnd, bounds, 1.0);
    assert_eq!(organizer.scroll, scroll);
    assert!(organizer.editor.show_skipped.unwrap());
    assert_eq!(apply_button_top(&organizer), button_top);

    organizer.run_command(&state, hwnd, Command::Run);
    assert!(organizer.editor.organize_error.is_some());
    assert!(organizer.editor.preview.unwrap().changes.is_some());
}
