//! One non-modal window for rules and the current organize operation.
use super::*;
use crate::pane::tooltip::Tooltip;
use std::cell::Cell;
use std::sync::mpsc::{self, Sender};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;

pub(super) const SELECT_TAB: u32 = WM_APP + 100;
const RULE_DELETE_RESULT: u32 = WM_APP + 103;
const RULE_DRAG_TIMER: usize = 0x4c4f44;
const ORGANIZER_MOTION_TIMER: usize = 0x4c4f4d;
#[cfg(test)]
pub(super) const INSPECT_TAB: u32 = WM_APP + 101;
#[cfg(test)]
pub(super) const HAS_CONTROL_FOCUS: u32 = WM_APP + 102;

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
                RULE_DELETE_RESULT => {
                    organizer.finish_delete(&state, hwnd, wp != 0);
                    return Some(0);
                }
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
                #[cfg(test)]
                HAS_CONTROL_FOCUS => return Some(isize::from(organizer.focus.is_some())),
                crate::i18n::CHANGED => {
                    unsafe {
                        SetWindowTextW(hwnd, crate::i18n::wide("organizer-window-title"));
                    }
                    redraw(hwnd);
                }
                WM_CLOSE => {
                    let _ = organizer.stop_rule_drag(hwnd);
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
                        KillTimer(hwnd, ORGANIZER_MOTION_TIMER);
                    }
                    drop(closing);
                }
                WM_TIMER if wp == ORGANIZE_TIMER => {
                    if organizer.editor.poll_organize() {
                        organizer.preview_fit_pending = organizer.editor.previewing();
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
                WM_TIMER if wp == RULE_DRAG_TIMER => organizer.scroll_rule_drag(hwnd),
                WM_TIMER if wp == ORGANIZER_MOTION_TIMER => {
                    let now = Instant::now();
                    organizer.switch_motion.retain(|_, motion| {
                        motion.sample(now).is_ok_and(|value| value != motion.to)
                    });
                    let row_transition = organizer.editor.update_rule_presence(now);
                    if !organizer.editor.tick_disclosure_animations(now)
                        && !row_transition
                        && !organizer.rule_motion.playing(now) && organizer.switch_motion.is_empty()
                        && !organizer.interface_playing
                    {
                        unsafe { KillTimer(hwnd, ORGANIZER_MOTION_TIMER); }
                    }
                    redraw(hwnd);
                }
                WM_PAINT => organizer.paint(&state, hwnd, scale),
                WM_SETTINGCHANGE => {
                    if !scrollbar::animations_enabled() {
                        organizer.finish_animations(hwnd);
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
                    let _ = organizer.stop_rule_drag(hwnd);
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
                    let point = client_point(lp);
                    if organizer.rule_drag_pointer(&state, hwnd, msg, point, scale) {
                        return Some(0);
                    }
                    if let Some(control) = organizer.pointer(hwnd, msg, point, scale) {
                        organizer.invoke(&state, hwnd, &control, scale, &dropdown_sender);
                    }
                }
                WM_SETCURSOR if lp as u16 == HTCLIENT as u16 => {
                    let mut pointer = POINT::default();
                    unsafe {
                        if organizer.rule_drag.is_some()
                            || (GetCursorPos(&raw mut pointer) != 0
                                && ScreenToClient(hwnd, &raw mut pointer) != 0
                                && organizer.rule_grip_at(pointer.x as f32 / scale, pointer.y as f32 / scale).is_some())
                        {
                            SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_SIZEALL));
                            return Some(1);
                        }
                    }
                    return None;
                }
                WM_MOUSELEAVE => {
                    organizer.clear_hover();
                    redraw(hwnd);
                }
                WM_CAPTURECHANGED | WM_CANCELMODE => {
                    let _ = organizer.stop_rule_drag(hwnd);
                    organizer.pressed = None;
                    organizer.scroll_grab = None;
                }
                WM_KEYDOWN | WM_SYSKEYDOWN | CLASSIFICATION_KEY => {
                    if organizer.rule_drag.is_some() {
                        if wp as u16 == VK_ESCAPE {
                            let _ = organizer.stop_rule_drag(hwnd);
                        }
                        return Some(0);
                    }
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

struct RuleDrag {
    from: usize,
    start_y: f32,
    grab_offset: f32,
    pointer: (f32, f32),
    active: bool,
}

impl RuleDrag {
    fn drop_target(&self, scene: &Scene) -> Option<usize> {
        let (x, y) = self.pointer;
        scene.viewport.filter(|viewport| contains(viewport, x, y))?;
        let first = scene.rule_rows.first()?.bounds;
        let last = scene.rule_rows.last()?.bounds;
        if !self.active || x < first.left || x >= first.right || y < first.top || y > last.bottom {
            return None;
        }
        // Use the layout positions so the animated rows do not move their drop thresholds.
        let slot = scene.rule_rows.partition_point(|row| y >= (row.bounds.top + row.bounds.bottom) * 0.5);
        let to = slot - usize::from(slot > self.from);
        (to != self.from).then_some(to)
    }
}

#[derive(Default)]
struct RuleReorderMotion {
    offsets: HashMap<usize, Motion>,
    // The visible row tops in the new order before the layout catches up.
    release: Option<Vec<f32>>,
    floating: Option<usize>,
}

impl RuleReorderMotion {
    fn playing(&self, now: Instant) -> bool {
        self.offsets.values().any(|motion| motion.sample(now).is_ok_and(|value| value != motion.to))
    }

    fn place(&mut self, scene: &mut Scene, drag: Option<&RuleDrag>, now: Instant, animate: bool) {
        if !animate {
            *self = Self::default();
        }
        if let Some(tops) = self.release.take() {
            for (index, (row, top)) in scene.rule_rows.iter().zip(tops).enumerate() {
                let mut offset = Motion::settled(top - (row.bounds.top + scene.scroll_offset), now);
                offset.retarget_with_duration(0.0, now, animate, SETTINGS_DURATION);
                self.offsets.insert(index, offset);
            }
        }
        let displacement = drag.map(|drag| {
            let to = drag.drop_target(scene).unwrap_or(drag.from);
            let source = scene.rule_rows[drag.from].bounds;
            let height = source.bottom - source.top;
            let y = scene.viewport.map_or(drag.pointer.1, |viewport| drag.pointer.1.clamp(viewport.top, viewport.bottom));
            (drag.from, to, height, y - drag.grab_offset - source.top)
        });
        for index in 0..scene.rule_rows.len() {
            let mut goal = 0.0;
            if let Some((from, to, height, dragged)) = displacement {
                if index == from {
                    scene.set_rule_row_offset(index, dragged);
                    continue;
                }
                if from < index && index <= to {
                    goal = -height;
                } else if to <= index && index < from {
                    goal = height;
                }
            }
            let offset = if animate && (goal != 0.0 || self.offsets.contains_key(&index)) {
                self.offsets.entry(index).or_insert_with(|| Motion::settled(0.0, now))
                    .retarget_with_duration(goal, now, true, SETTINGS_DURATION)
            } else {
                goal
            };
            scene.set_rule_row_offset(index, offset);
        }
        self.offsets.retain(|_, motion| motion.to != 0.0 || motion.sample(now).is_ok_and(|value| value != 0.0));
        self.floating = self.floating.filter(|index| self.offsets.contains_key(index));
        scene.floating_rule = drag.map(|drag| drag.from).or(self.floating);
    }
}

impl Scene {
    fn floating_rule_at(&self, x: f32, y: f32) -> Option<usize> {
        self.floating_rule.filter(|&index| {
            self.viewport.is_some_and(|viewport| contains(&viewport, x, y))
                && contains(&self.rule_rows[index].visual_bounds(), x, y)
        })
    }

    fn set_rule_row_offset(&mut self, index: usize, offset: f32) {
        if offset == 0.0 { return; }
        let row = &mut self.rule_rows[index];
        row.offset = offset;
        if let Some(transition) = self.rule_transitions.iter_mut().find(|transition| transition.row.controls == row.controls) {
            transition.row.offset = offset;
        }
        for bounds in self.controls[row.controls.clone()].iter_mut().map(|control| &mut control.bounds)
            .chain(self.text[row.text.clone()].iter_mut().map(|(bounds, _, _)| bounds))
            .chain(&mut self.separators[row.separators.clone()])
            .chain(self.disclosure_clips[row.detail_clips.clone()].iter_mut().map(|clip| &mut clip.bounds))
        {
            bounds.top += offset;
            bounds.bottom += offset;
        }
    }
}

struct RuleDeleteRequest {
    index: usize,
    expected: SettingValue,
}

struct Organizer {
    tab: Tab,
    editor: Editor,
    scene: Option<Scene>,
    rules: Option<SettingValue>,
    pending_delete: Option<RuleDeleteRequest>,
    appearance: (PanelTheme, Backdrop),
    painter: Painter,
    typography: (String, &'static str),
    surface: Option<composition::Surface>,
    paint_recovery: composition::PaintRecovery,
    scroll: f32,
    tab_scroll: [f32; 2],
    opened_preview: bool,
    auto_fit_preview: bool,
    preview_fit_pending: bool,
    scroll_grab: Option<f32>,
    scroll_hover: Option<usize>,
    interface_motion: motions::InterfaceMotion,
    interface_playing: bool,
    rule_drag: Option<RuleDrag>,
    rule_motion: RuleReorderMotion,
    pending_switch: Option<Command>,
    switch_motion: HashMap<RuleSwitch, Motion>,
    grip_hover: Option<usize>,
    hover: Option<usize>,
    hovered_destination: Option<(usize, usize)>,
    pressed: Option<usize>,
    focus: Option<usize>,
    keyboard_focus: bool,
    tip: Option<Tooltip>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum RuleSwitch {
    Automatic,
    Rule(usize),
}

impl RuleSwitch {
    fn for_command(command: Command) -> Option<Self> {
        match command {
            Command::Enable(_) => Some(Self::Automatic),
            Command::Toggle(index) => Some(Self::Rule(index)),
            _ => None,
        }
    }

    fn saved_change(
        command: Command,
        before: Option<&SettingValue>,
        after: Option<&SettingValue>,
    ) -> Option<(Self, f32, f32)> {
        let mut expected = decode(before).ok()?;
        let (id, enabled, to) = match command {
            Command::Enable(to) => (Self::Automatic, &mut expected.enabled, to),
            Command::Toggle(index) => {
                let enabled = &mut expected.rules.get_mut(index)?.enabled;
                let to = !*enabled;
                (Self::Rule(index), enabled, to)
            }
            _ => return None,
        };
        let from = std::mem::replace(enabled, to);
        // A reorder, edit or external change can reuse a row index. Animate only the saved toggle.
        if from != to && after == Some(&SettingValue::Text(encode(&expected).ok()?)) {
            Some((id, f32::from(u8::from(from)), f32::from(u8::from(to))))
        } else {
            None
        }
    }
}

impl Organizer {
    fn new() -> Result<Self, String> {
        Ok(Self {
            tab: Tab::Rules,
            editor: Editor::default(),
            scene: None,
            rules: None,
            pending_delete: None,
            appearance: (PanelTheme::System, Backdrop::Mica),
            painter: Painter::new().map_err(|error| error.to_string())?,
            typography: (fonts::family(), crate::i18n::language()),
            surface: None,
            paint_recovery: Default::default(),
            scroll: 0.0,
            tab_scroll: [0.0; 2],
            opened_preview: false,
            auto_fit_preview: true,
            preview_fit_pending: true,
            scroll_grab: None,
            scroll_hover: None,
            interface_motion: motions::InterfaceMotion::default(),
            interface_playing: false,
            rule_drag: None,
            rule_motion: RuleReorderMotion::default(),
            pending_switch: None,
            switch_motion: HashMap::new(),
            grip_hover: None,
            hover: None,
            hovered_destination: None,
            pressed: None,
            focus: None,
            keyboard_focus: false,
            tip: None,
        })
    }

    fn select(&mut self, state: &Rc<RefCell<PaneApp>>, hwnd: HWND, tab: Tab, entry: bool) {
        if state.try_borrow_mut().is_err() {
            return;
        }
        if self.tab != tab {
            let _ = self.stop_rule_drag(hwnd);
            self.finish_animations(hwnd);
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
            let pending_switch = self.pending_switch.take();
            if self.rules != rules {
                let _ = self.stop_rule_drag(hwnd);
                if self.rule_motion.release.is_none() {
                    self.rule_motion = RuleReorderMotion::default();
                }
                if self.update_switch_motion(pending_switch, rules.as_ref(), Instant::now(), scrollbar::animations_enabled()) {
                    self.schedule_animations(hwnd);
                }
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
            if self.auto_fit_preview && self.preview_fit_pending && self.tab == Tab::Organize && self.editor.previewing() {
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
            if !scene.rule_rows.is_empty() {
                let drag = self.rule_drag.as_ref().filter(|drag| drag.active);
                if drag.is_some() || self.rule_motion.release.is_some() || !self.rule_motion.offsets.is_empty() {
                    let now = Instant::now();
                    self.rule_motion.place(&mut scene, drag, now, scrollbar::animations_enabled());
                    if self.rule_motion.playing(now) {
                        self.schedule_animations(hwnd);
                    }
                }
                scene.rule_grip_pressed = self.rule_drag.as_ref().map(|drag| drag.from);
                scene.rule_grip_hover = self.grip_hover;
            }
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
        if self.tab == Tab::Organize && self.editor.previewing() {
            self.preview_fit_pending = false;
        }
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
        let mut switch_positions = HashMap::new();
        if !self.switch_motion.is_empty() {
            let now = Instant::now();
            for (index, control) in scene.controls.iter().enumerate() {
                if let Action::Classification(command) = control.action
                    && let Some(id) = RuleSwitch::for_command(command)
                    && let Some(motion) = self.switch_motion.get(&id)
                    && let Ok(position) = motion.sample(now)
                {
                    switch_positions.insert(index, position);
                }
            }
        }
        let focus = self.visible_focus(scene);
        let (motion, playing) = self.interface_motion.frame(scene, motions::MotionInput {
            hover: self.hover,
            pressed: self.pressed,
            scrollbar: self.scroll_grab.map(|_| 0).or(self.scroll_hover),
            now: Instant::now(),
            enabled: scrollbar::animations_enabled(),
        });
        if playing && !self.interface_playing {
            unsafe { SetTimer(hwnd, ORGANIZER_MOTION_TIMER, USER_TIMER_MINIMUM, None); }
        }
        self.interface_playing = playing;
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
                self.painter.paint_animated(
                    &target,
                    scene,
                    width,
                    height,
                    scale,
                    dark,
                    surface.native,
                    self.hover,
                    focus,
                    &switch_positions,
                    &motion,
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
            Action::Classification(Command::Delete(index)) => self.request_delete(state, hwnd, index),
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

    fn request_delete(&mut self, state: &Rc<RefCell<PaneApp>>, hwnd: HWND, index: usize) {
        if self.tab != Tab::Rules || self.editor.editing() || self.pending_delete.is_some() {
            return;
        }
        let Some(expected) = self.rules.clone() else { return; };
        let Ok(config) = decode(Some(&expected)) else { return; };
        let Some(rule) = config.rules.get(index) else { return; };
        let group = targets(state.borrow().workspace.panels())
            .find(|panel| panel.id().get() == rule.target_panel)
            .map_or(t("classification-missing"), Panel::title)
            .to_owned();
        let body = crate::i18n::format(
            "classification-delete-confirm-body",
            &[("position", (index + 1).to_string()), ("group", group)],
        );
        let owner = Rc::downgrade(state);
        let theme = self.appearance.0;
        // The modal dialog pumps the owner's messages after its current callback returns.
        if window::defer_action(move || {
            let still_open = || owner.upgrade().is_some_and(|state| {
                state.try_borrow().is_ok_and(|state| {
                    state.organizer.as_ref().is_some_and(|window| window.hwnd().cast() == hwnd)
                })
            });
            if !still_open() {
                return;
            }
            luciddesk_window::set_message_theme(match theme {
                PanelTheme::System => None,
                PanelTheme::Dark => Some(true),
                PanelTheme::Light => Some(false),
            });
            let confirmed = crate::error_report::confirm(
                hwnd as isize,
                t("classification-delete-confirm-title"),
                &body,
            );
            if still_open() {
                unsafe { SendMessageW(hwnd, RULE_DELETE_RESULT, usize::from(confirmed), 0); }
            }
        }) {
            self.pending_delete = Some(RuleDeleteRequest { index, expected });
        }
    }

    fn finish_delete(&mut self, state: &Rc<RefCell<PaneApp>>, hwnd: HWND, confirmed: bool) {
        let Some(request) = self.pending_delete.take() else { return; };
        if !confirmed || self.tab != Tab::Rules || self.editor.editing() {
            return;
        }
        let current = state.borrow().store.settings().map(|mut values| values.remove(KEY));
        if current.is_ok_and(|value| value.as_ref() == Some(&request.expected)) {
            self.run_command(state, hwnd, Command::Delete(request.index));
        } else {
            self.editor.error = Some(t("classification-conflict").into());
            redraw(hwnd);
        }
    }

    fn run_command(&mut self, state: &Rc<RefCell<PaneApp>>, hwnd: HWND, command: Command) {
        if self.tab != command.tab() || state.try_borrow_mut().is_err() {
            return;
        }
        let previous = self.editor.expanded;
        let previous_skipped = if command == Command::ToggleSkipped {
            self.editor.preview.as_ref().map(|preview| self.editor.skipped_expanded(preview))
        } else {
            None
        };
        self.editor.execute(state, hwnd, command);
        if matches!(command, Command::Delete(_) | Command::UndoDelete) && self.editor.error.is_none() {
            // The rebuilt controls must not inherit focus from a vanished button.
            self.focus = None;
            self.clear_hover();
            if self.editor.rule_presence.is_some() {
                self.schedule_animations(hwnd);
            }
        }
        if self.editor.error.is_none() && RuleSwitch::for_command(command).is_some() {
            self.pending_switch = Some(command);
        }
        if matches!(command, Command::ToggleDetails(_)) && previous != self.editor.expanded
            && scrollbar::animations_enabled()
        {
            self.editor.animate_details(previous, Instant::now());
            self.schedule_animations(hwnd);
        }
        if let Some(previous) = previous_skipped {
            self.preview_fit_pending = false;
            if self.editor.animate_skipped(previous, Instant::now(), scrollbar::animations_enabled()) {
                self.schedule_animations(hwnd);
            }
        }
        if self.tab == Tab::Organize {
            if command == Command::Run && self.editor.organize_error.is_none() && self.editor.previewing() {
                self.preview_fit_pending = true;
            }
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
        self.grip_hover = None;
        self.scroll_hover = None;
        self.hovered_destination = None;
        if let Some(tip) = &mut self.tip {
            tip.hide();
        }
    }

    fn visible_focus(&self, scene: &Scene) -> Option<usize> {
        self.focus.filter(|&index| {
            scene.controls.get(index).is_some_and(|control| {
                self.keyboard_focus || !matches!(control.kind, ControlKind::SectionDisclosure { .. })
            })
        })
    }

    fn scroll_by(&mut self, hwnd: HWND, delta: f32) {
        if let Some(scene) = &self.scene {
            self.scroll = (self.scroll + delta).clamp(0.0, scene.scroll_max);
            self.clear_hover();
            redraw(hwnd);
        }
    }

    fn rule_grip_at(&self, x: f32, y: f32) -> Option<(usize, Rect)> {
        let scene = self.scene.as_ref()?;
        if scene.rule_rows.len() < 2 || !scene.viewport.is_some_and(|viewport| contains(&viewport, x, y)) {
            return None;
        }
        let floating = scene.floating_rule_at(x, y);
        scene.rule_rows.iter().enumerate().find_map(|(index, row)| {
            let grip = row.grip();
            (floating.is_none_or(|raised| raised == index) && contains(&grip, x, y)
                && scene.rule_transition(row.controls.start, |row| &row.controls).is_none())
                .then_some((index, grip))
        })
    }

    fn rule_drag_pointer(
        &mut self, state: &Rc<RefCell<PaneApp>>, hwnd: HWND, msg: u32, point: POINT, scale: f32,
    ) -> bool {
        let pointer = (point.x as f32 / scale, point.y as f32 / scale);
        if let Some(drag) = &mut self.rule_drag {
            drag.pointer = pointer;
            if !drag.active && (drag.start_y - pointer.1).abs() >= 4.0 {
                drag.active = true;
                unsafe { SetTimer(hwnd, RULE_DRAG_TIMER, 50, None); }
            }
            if msg == WM_LBUTTONUP {
                self.layout(state, hwnd, client_bounds(hwnd), scale);
                if let Some(drag) = self.stop_rule_drag(hwnd)
                    && let (Some(scene), Some(expected)) = (&self.scene, &self.rules)
                    && drag.active
                {
                    let saved = drag.drop_target(scene).filter(|&to| {
                        state.try_borrow().is_ok_and(|owner| self.editor.drop_rule(&owner, drag.from, to, expected))
                    });
                    if scrollbar::animations_enabled() {
                        let mut tops: Vec<_> = scene.rule_rows.iter()
                            .map(|row| row.visual_bounds().top + scene.scroll_offset).collect();
                        let mut floating = drag.from;
                        if let Some(to) = saved {
                            let top = tops.remove(drag.from);
                            tops.insert(to, top);
                            floating = to;
                        }
                        self.rule_motion.release = Some(tops);
                        self.rule_motion.floating = Some(floating);
                        if saved.is_some() {
                            self.editor.collapse_details();
                        }
                    }
                }
            }
            redraw(hwnd);
            return true;
        }
        if msg != WM_LBUTTONDOWN {
            return false;
        }
        let Some((from, grip)) = self.rule_grip_at(pointer.0, pointer.1) else { return false; };
        let grab_offset = pointer.1 - grip.top + Tokens::GAP;
        self.clear_hover();
        self.pressed = None;
        self.focus = None;
        unsafe {
            SetFocus(hwnd);
            SetCapture(hwnd);
        }
        if unsafe { GetCapture() } == hwnd {
            self.rule_motion = RuleReorderMotion::default();
            self.rule_drag = Some(RuleDrag { from, start_y: pointer.1, grab_offset, pointer, active: false });
        }
        redraw(hwnd);
        true
    }

    fn stop_rule_drag(&mut self, hwnd: HWND) -> Option<RuleDrag> {
        let drag = self.rule_drag.take()?;
        self.rule_motion = RuleReorderMotion::default();
        unsafe {
            KillTimer(hwnd, RULE_DRAG_TIMER);
            if GetCapture() == hwnd {
                ReleaseCapture();
            }
        }
        redraw(hwnd);
        Some(drag)
    }

    fn schedule_animations(&mut self, hwnd: HWND) {
        if unsafe { SetTimer(hwnd, ORGANIZER_MOTION_TIMER, USER_TIMER_MINIMUM, None) } == 0 {
            self.finish_animations(hwnd);
        }
    }

    fn update_switch_motion(
        &mut self,
        command: Option<Command>,
        rules: Option<&SettingValue>,
        now: Instant,
        animate: bool,
    ) -> bool {
        let change = command.filter(|_| animate)
            .and_then(|command| RuleSwitch::saved_change(command, self.rules.as_ref(), rules));
        let Some((id, from, to)) = change else {
            self.switch_motion.clear();
            return false;
        };
        let motion = self.switch_motion.entry(id).or_insert_with(|| Motion::settled(from, now));
        motion.retarget(to, now, true) != to
    }

    fn finish_animations(&mut self, hwnd: HWND) {
        self.editor.finish_disclosure_animations();
        self.editor.rule_presence = None;
        self.rule_motion = RuleReorderMotion::default();
        self.pending_switch = None;
        self.switch_motion.clear();
        self.interface_playing = false;
        unsafe { KillTimer(hwnd, ORGANIZER_MOTION_TIMER); }
    }

    fn scroll_rule_drag(&mut self, hwnd: HWND) {
        let Some(drag) = self.rule_drag.as_ref().filter(|drag| drag.active) else { return; };
        let Some(scene) = &self.scene else { return; };
        let (Some(viewport), Some(row)) = (scene.viewport, scene.rule_rows.first()) else { return; };
        let (x, y) = drag.pointer;
        if !contains(&viewport, x, y) || x < row.bounds.left || x >= row.bounds.right {
            return;
        }
        if y - viewport.top < Tokens::MARGIN && self.scroll > 0.0 {
            self.scroll_by(hwnd, -Tokens::INSET);
        } else if viewport.bottom - y < Tokens::MARGIN && self.scroll < scene.scroll_max {
            self.scroll_by(hwnd, Tokens::INSET);
        }
    }

    fn pointer(&mut self, hwnd: HWND, msg: u32, point: POINT, scale: f32) -> Option<Control> {
        let (x, y) = (point.x as f32 / scale, point.y as f32 / scale);
        let scene = self.scene.as_ref()?;
        let scroll_hover = motions::scrollbar_at(scene, x, y);
        if scroll_hover != self.scroll_hover {
            self.scroll_hover = scroll_hover;
            redraw(hwnd);
        }
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
        let floating = scene.floating_rule_at(x, y).map(|index| &scene.rule_rows[index]);
        let hit = scene
            .controls
            .iter()
            .enumerate()
            .find(|(index, control)| {
                floating.is_none_or(|row| row.controls.contains(index))
                    && control.enabled && scene.accepts_pointer(control, x, y)
            })
            .map(|(index, _)| index);
        let destination = if msg == WM_MOUSEMOVE && hit.is_none() {
            destination_tooltip(scene, x, y)
        } else {
            None
        };
        let destination_hit = destination.map(|(hit, _, _)| hit);
        let grip = (msg == WM_MOUSEMOVE).then(|| self.rule_grip_at(x, y)).flatten();
        let grip_hover = grip.map(|(index, _)| index);
        match msg {
            WM_LBUTTONDOWN => {
                self.pressed = hit;
                self.focus = hit;
                self.keyboard_focus = false;
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
            WM_MOUSEMOVE if hit != self.hover || destination_hit != self.hovered_destination || grip_hover != self.grip_hover => {
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
                            components::text_width_with_weight(&control.label, SectionHeader::TITLE_SIZE, SectionHeader::TITLE_WEIGHT)
                                > control.bounds.right - control.bounds.left - inset
                        }
                        _ => false,
                    })
                    .map(|icon| (icon.bounds, icon.label.as_str()))
                    .or_else(|| destination.map(|(_, bounds, name)| (bounds, name)))
                    .or_else(|| grip.map(|(_, bounds)| (bounds, t("classification-reorder"))));
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
                if self.hover != hit || self.grip_hover != grip_hover {
                self.hover = hit;
                self.grip_hover = grip_hover;
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
                self.keyboard_focus = true;
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
                    self.keyboard_focus = true;
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
fn rule_grip_hover_updates_when_entering_and_leaving_empty_space() {
    let _sta = crate::pane::test_support::apartment();
    let window = windows_window::Window::new("Grip hover test")
        .on_message(|_, message, _, _| (message == WM_DESTROY).then_some(0))
        .create().unwrap();
    let hwnd = window.hwnd().cast();
    let mut organizer = Organizer::new().unwrap();
    let mut scene = Scene::empty(Backdrop::Mica, 16.0, 100.0);
    scene.viewport = Some(Rect::from_xywh(16.0, 100.0, 480.0, 200.0));
    scene.rule_rows = [100.0, 164.0].into_iter().map(|top| RuleRow {
        bounds: Rect::from_xywh(32.0, top, 440.0, 64.0),
        ..RuleRow::default()
    }).collect();
    let grip = scene.rule_rows[0].grip();
    organizer.scene = Some(scene);

    organizer.pointer(hwnd, WM_MOUSEMOVE, POINT { x: grip.left as i32 + 2, y: grip.top as i32 + 2 }, 1.0);
    assert_eq!(organizer.grip_hover, Some(0));
    assert_eq!(organizer.hover, None);
    organizer.pointer(hwnd, WM_MOUSEMOVE, POINT { x: grip.right as i32 + 2, y: grip.top as i32 + 2 }, 1.0);
    assert_eq!(organizer.grip_hover, None);
}

#[test]
fn rule_drag_advances_insertion_when_scrolled_at_the_viewport_edge() {
    let _sta = crate::pane::test_support::apartment();
    let window = windows_window::Window::new("Rule drag test")
        .on_message(|_, message, _, _| (message == WM_DESTROY).then_some(0))
        .create().unwrap();
    let hwnd = window.hwnd().cast();
    let mut organizer = Organizer::new().unwrap();
    let mut scene = Scene::empty(Backdrop::Mica, Tokens::MARGIN, 96.0);
    scene.rule_rows = (0..16)
        .map(|index| RuleRow {
            bounds: Rect::from_xywh(40.0, 240.0 + index as f32 * 70.0, 400.0, 70.0),
            ..RuleRow::default()
        })
        .collect();
    scene.cards.push(Rect::from_xywh(24.0, 240.0, 432.0, 16.0 * 70.0));
    scene.scroll_to(480.0, 380.0, &mut 0.0);
    let drag = RuleDrag { from: 0, start_y: 260.0, grab_offset: 20.0, pointer: (52.0, 374.0), active: true };
    assert_eq!(drag.drop_target(&scene), Some(1));
    organizer.rule_drag = Some(drag);
    organizer.scene = Some(scene);
    for _ in 0..10 {
        organizer.scroll_rule_drag(hwnd);
    }
    assert_eq!(organizer.scroll, 10.0 * Tokens::INSET);
    let scene = organizer.scene.as_mut().unwrap();
    scene.scroll_to(480.0, 380.0, &mut organizer.scroll);
    let drag = organizer.rule_drag.as_ref().unwrap();
    assert_eq!(drag.drop_target(scene), Some(3));
}

#[test]
fn rule_switch_motion_reverses_and_only_follows_the_saved_switch() {
    let _sta = crate::pane::test_support::apartment();
    let mut organizer = Organizer::new().unwrap();
    let snapshot = |config: &Classification| SettingValue::Text(encode(config).unwrap());
    let mut config = Classification {
        enabled: false,
        rules: vec![ClassificationRule {
            target_panel: 1,
            enabled: true,
            item_type: None,
            extensions: vec!["pdf".into()],
            name_contains: None,
            name_regex: None,
        }],
    };
    let start = Instant::now();
    organizer.rules = Some(snapshot(&config));
    config.enabled = true;
    let enabled = snapshot(&config);
    assert!(organizer.update_switch_motion(Some(Command::Enable(true)), Some(&enabled), start, true));
    organizer.rules = Some(enabled);

    let halfway = start + crate::pane::animation::TOGGLE_DURATION / 2;
    let automatic = RuleSwitch::Automatic;
    let fraction = organizer.switch_motion[&automatic].sample(halfway).unwrap();
    assert!(fraction > 0.0 && fraction < 1.0);
    config.rules[0].enabled = false;
    let disabled_rule = snapshot(&config);
    assert!(organizer.update_switch_motion(Some(Command::Toggle(0)), Some(&disabled_rule), halfway, true));
    assert_eq!(organizer.switch_motion[&RuleSwitch::Rule(0)].sample(halfway).unwrap(), 1.0);
    organizer.rules = Some(disabled_rule);

    config.enabled = false;
    let disabled = snapshot(&config);
    assert!(organizer.update_switch_motion(Some(Command::Enable(false)), Some(&disabled), halfway, true));
    assert!((organizer.switch_motion[&automatic].sample(halfway).unwrap() - fraction).abs() < 0.001);

    organizer.rules = Some(disabled);
    config.rules[0].enabled = true;
    let enabled_rule = snapshot(&config);
    assert!(!organizer.update_switch_motion(Some(Command::Toggle(0)), Some(&enabled_rule), halfway, false));
    assert!(organizer.switch_motion.is_empty());
    config.rules[0].target_panel = 2;
    let unrelated_edit = snapshot(&config);
    assert!(!organizer.update_switch_motion(Some(Command::Toggle(0)), Some(&unrelated_edit), halfway, true));
    assert!(organizer.switch_motion.is_empty());
}

#[test]
fn rule_drag_opens_the_gap_and_settles_from_the_last_visible_positions() {
    let _sta = crate::pane::test_support::apartment();
    let make_scene = || {
        let mut scene = Scene::empty(Backdrop::Mica, 16.0, 100.0);
        scene.viewport = Some(Rect::from_xywh(16.0, 100.0, 480.0, 200.0));
        scene.rule_rows = (0..3).map(|index| RuleRow {
            bounds: Rect::from_xywh(32.0, 100.0 + index as f32 * 48.0, 440.0, 48.0),
            ..RuleRow::default()
        }).collect();
        scene
    };
    let mut motion = RuleReorderMotion::default();
    let drag = RuleDrag { from: 0, start_y: 124.0, grab_offset: 24.0, pointer: (48.0, 226.0), active: true };
    let now = Instant::now();
    let mut scene = make_scene();
    motion.place(&mut scene, Some(&drag), now, true);
    assert_eq!(scene.floating_rule, Some(0));
    assert_eq!(scene.rule_rows[0].visual_bounds().top, 202.0);
    assert_eq!(scene.rule_rows[1].offset, 0.0);

    let halfway = now + SETTINGS_DURATION / 2;
    let mut scene = make_scene();
    motion.place(&mut scene, Some(&drag), halfway, true);
    let first_shift = scene.rule_rows[1].offset;
    assert!(first_shift < 0.0 && first_shift > -48.0);
    assert_eq!(scene.rule_rows[2].offset, first_shift);
    let old_tops: Vec<_> = [1, 2, 0].into_iter().map(|index| scene.rule_rows[index].visual_bounds().top).collect();

    let mut scene = make_scene();
    let mut motion = RuleReorderMotion { release: Some(old_tops.clone()), floating: Some(2), ..Default::default() };
    motion.place(&mut scene, None, halfway, true);
    assert_eq!(scene.floating_rule, Some(2));
    for (row, old) in scene.rule_rows.iter().zip(old_tops) {
        assert!((row.visual_bounds().top - old).abs() < 0.01);
    }
    let mut scene = make_scene();
    motion.place(&mut scene, None, halfway + SETTINGS_DURATION, true);
    assert!(scene.floating_rule.is_none());
    assert!(scene.rule_rows.iter().all(|row| row.offset == 0.0));

    motion.place(&mut scene, Some(&drag), halfway + SETTINGS_DURATION, false);
    assert_eq!(scene.floating_rule, Some(0));
    assert_eq!(scene.rule_rows[1].offset, -48.0);
    assert_eq!(scene.rule_rows[2].offset, -48.0);
    assert!(motion.offsets.is_empty());
}

#[test]
fn skipped_disclosure_only_shows_focus_after_keyboard_interaction() {
    let _sta = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
    let window = windows_window::Window::new("Skipped disclosure test").create().unwrap();
    let hwnd = window.hwnd().cast();
    let bounds = RECT { right: 800, bottom: 560, ..Default::default() };
    let mut organizer = Organizer::new().unwrap();
    organizer.tab = Tab::Organize;
    organizer.editor.show_skipped = Some(false);
    organizer.editor.preview = Some(hybrid::OrganizePreview::fixture(vec![hybrid::PreviewEntry::Skip {
        identity: ShellIdentity::FileSystem { path: "Draft.txt".into(), volume_id: None, file_id: None },
        item_name: "Draft.txt".into(),
        reason: hybrid::SkipReason::NoMatch,
    }], None));
    let disclosure = |organizer: &Organizer| {
        let scene = organizer.scene.as_ref().unwrap();
        let (index, control) = scene.controls.iter().enumerate()
            .find(|(_, control)| Command::ToggleSkipped.is_control(&control.action)).unwrap();
        (index, control.bounds, control.selected, organizer.visible_focus(scene))
    };
    assert_ne!(organizer.layout(&state, hwnd, bounds, 1.0), bounds.bottom as f32);
    let (index, heading, expanded, _) = disclosure(&organizer);
    assert!(!expanded);
    let point = POINT {
        x: (heading.right - Tokens::INSET) as i32,
        y: ((heading.top + heading.bottom) / 2.0) as i32,
    };
    let (sender, _receiver) = mpsc::channel();
    organizer.pointer(hwnd, WM_LBUTTONDOWN, point, 1.0);
    let clicked = organizer.pointer(hwnd, WM_LBUTTONUP, point, 1.0).unwrap();
    organizer.invoke(&state, hwnd, &clicked, 1.0, &sender);
    assert_eq!(organizer.layout(&state, hwnd, bounds, 1.0), bounds.bottom as f32);
    assert_eq!(organizer.focus, Some(index));
    assert_eq!(disclosure(&organizer), (index, heading, true, None));

    assert!(organizer.key(&state, hwnd, (WM_KEYDOWN, VK_SPACE, 0), 1.0, &sender));
    assert_eq!(organizer.layout(&state, hwnd, bounds, 1.0), bounds.bottom as f32);
    assert_eq!(disclosure(&organizer), (index, heading, false, Some(index)));
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
    organizer.editor.finish_disclosure_animations();
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
