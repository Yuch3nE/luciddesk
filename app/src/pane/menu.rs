//! WinUI-inspired composition flyout. Uses the owning pane material with default properties.
#![allow(
    clippy::wildcard_imports,
    clippy::fn_params_excessive_bools,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
use super::{composition::Surface, render::Renderer};
use luciddesk_core::Backdrop;
use std::{cell::Cell, rc::Rc, time::Instant};
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::*,
    UI::{HiDpi::GetDpiForWindow, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

#[derive(Clone)]
pub struct Entry {
    pub id: i32,
    pub enabled: bool,
    pub label: std::borrow::Cow<'static, str>,
    pub icon: &'static str,
    pub trailing: &'static str,
    pub children: Vec<Entry>,
}
impl Entry {
    pub fn trailing_is_icon(&self) -> bool {
        self.trailing.chars().next().is_some_and(|ch| ('\u{e000}'..='\u{f8ff}').contains(&ch))
    }

    pub fn trailing_width(&self) -> f32 {
        if !self.children.is_empty() || self.trailing_is_icon() { 24.0 }
        else if self.trailing.is_empty() { 0.0 }
        else { 72.0 }
    }
}

pub const ROW_HEIGHT: f32 = super::control_style::HEIGHT;
pub(super) const CORNER_RADIUS: f32 = 8.0;
pub fn row_top(rows: &[Entry], index: usize) -> f32 {
    4.0 + rows[..index]
        .iter()
        .map(|row| if row.id == 0 { 7.0 } else { ROW_HEIGHT })
        .sum::<f32>()
}
pub(crate) fn entry(
    id: i32,
    label: impl Into<std::borrow::Cow<'static, str>>,
    icon: &'static str,
    trailing: &'static str,
) -> Entry {
    Entry {
        id,
        enabled: true,
        label: label.into(),
        icon,
        trailing,
        children: Vec::new(),
    }
}

// Skip disabled commands and separators without allocating on every key.
fn keyboard_selection(rows: &[Entry], selected: Option<usize>, key: u16) -> Option<usize> {
    let enabled = |i: usize| rows[i].enabled && rows[i].id != 0;
    match key {
        VK_HOME => (0..rows.len()).find(|&i| enabled(i)),
        VK_END => (0..rows.len()).rev().find(|&i| enabled(i)),
        VK_DOWN => {
            let start = selected.filter(|&i| i < rows.len()).map_or(0, |i| i + 1);
            (start..rows.len()).chain(0..start).find(|&i| enabled(i))
        }
        VK_UP => {
            let end = selected.filter(|&i| i < rows.len()).unwrap_or(rows.len());
            (0..end).rev().chain((end..rows.len()).rev()).find(|&i| enabled(i))
        }
        _ => selected,
    }
}

fn default_material(backdrop: Backdrop, dark: bool) -> Backdrop {
    match backdrop {
        Backdrop::Solid { .. } => Backdrop::Solid {
            color: if dark { 0x181b20 } else { 0xf5f6f8 },
            opacity: 0.85,
        },
        Backdrop::Translucent { .. } => Backdrop::Translucent { opacity: 1.0 },
        other => other.base(),
    }
}

pub fn show(
    owner: HWND,
    anchor: POINT,
    anchored: bool,
    auto_hide: bool,
    locked: bool,
    theme: luciddesk_core::PanelTheme,
    backdrop: Backdrop,
    folder: (bool, bool),
    visible_columns: u8,
    collapsed: bool,
    fixed_grid: bool,
    free_layout: bool,
    sort_order: Option<(u8, bool)>,
    display: luciddesk_core::IconDisplay,
    tab_position: Option<(usize, usize)>,
) -> i32 {
    let topmost = super::quick_reveal::permanent_topmost(owner);
    let mut rows = pane_entries_fixed(folder, visible_columns, auto_hide, locked, topmost, collapsed, fixed_grid, free_layout, tab_position);
    set_sort_entries(&mut rows, sort_order);
    set_icon_entries(&mut rows, folder.1, display);
    show_entries(owner, anchor, anchored, theme, backdrop, rows)
}

fn set_icon_entries(rows: &mut Vec<Entry>, list_view: bool, display: luciddesk_core::IconDisplay) {
    if let Some(view) = rows.iter_mut().find(|row| row.id == 22) {
        let arrangement: Vec<_> = view.children.drain(..).filter(|row| matches!(row.id, 54 | 55)).collect();
        view.children = [
            (25, "menu-icons-with-names", !list_view && !display.icon_only),
            (70, "menu-icons-only", !list_view && display.icon_only),
            (26, "ui-list", list_view),
        ].into_iter().map(|(id, key, checked)| entry(id, crate::i18n::text(key), if checked { "\u{e73e}" } else { "" }, "")).collect();
        if !arrangement.is_empty() {
            view.children.push(entry(0, "", "", ""));
            view.children.extend(arrangement);
        }
    }
    if !list_view && display.icon_only {
        let mut sizes = entry(74, crate::i18n::text("icon-size-title"), "", "");
        sizes.children = [
            (71, "icon-size-small", luciddesk_core::IconSize::Small),
            (72, "icon-size-medium", luciddesk_core::IconSize::Medium),
            (73, "icon-size-large", luciddesk_core::IconSize::Large),
        ].into_iter().map(|(id, key, size)| entry(id, crate::i18n::text(key), if display.size == size { "\u{e73e}" } else { "" }, "")).collect();
        let at = rows.iter().position(|r| r.id == 22).map_or(0, |i| i + 1);
        rows.insert(at, sizes);
    }
}

#[test]
fn icon_menu_matches_active_panel_and_preserves_list_mode() {
    let mut model = super::tests::test_model("Icons");
    model.icon_display = luciddesk_core::IconDisplay { icon_only: true, size: luciddesk_core::IconSize::Medium };
    for list in [false, true] {
        model.list_view = list;
        let rows = tab_context_entries(&model, false, model.active_tab);
        let view = rows.iter().find(|r| r.id == 22).unwrap();
        let only = view.children.iter().find(|r| r.id == 70).unwrap();
        assert!(only.enabled);
        assert_eq!(!only.icon.is_empty(), !list);
        assert_eq!(view.children.iter().filter(|r| matches!(r.id, 25 | 26 | 70) && !r.icon.is_empty()).count(), 1);
        let sizes = rows.iter().find(|r| r.id == 74);
        if list { assert!(sizes.is_none()); continue; }
        let sizes = sizes.unwrap();
        assert_eq!(sizes.children.iter().filter(|r| !r.icon.is_empty()).map(|r| r.id).collect::<Vec<_>>(), vec![72]);
    }
}

#[cfg(test)]
fn pane_entries(folder: (bool, bool), visible_columns: u8,
    auto_hide: bool, locked: bool, topmost: bool, collapsed: bool) -> Vec<Entry> {
    pane_entries_fixed(folder, visible_columns, auto_hide, locked, topmost, collapsed, false, false, None)
}
#[allow(clippy::too_many_arguments)]
fn pane_entries_fixed(folder: (bool, bool), visible_columns: u8,
    auto_hide: bool, locked: bool, topmost: bool, collapsed: bool, fixed_grid: bool, free_layout: bool,
    tab_position: Option<(usize, usize)>) -> Vec<Entry> {
    let mut view = entry(22, crate::i18n::text("ui-view"), "", "");
    view.children = vec![
        entry(25, crate::i18n::text("menu-icons-with-names"), if folder.1 { "" } else { "\u{e73e}" }, ""),
        entry(26, crate::i18n::text("ui-list"), if folder.1 { "\u{e73e}" } else { "" }, ""),
    ];
    if !folder.0 && !folder.1 && !locked {
        view.children.push(entry(0, "", "", ""));
        view.children.push(entry(55, crate::i18n::text("ui-align-icons-to-grid"), if free_layout { "" } else { "\u{e73e}" }, ""));
        view.children.push(entry(54, crate::i18n::text("ui-auto-arrange-icons"), if fixed_grid { "" } else { "\u{e73e}" }, ""));
    }
    let mut rows = vec![view];
    if !locked {
        rows.push(Entry { children: sort_entries(None), ..entry(51, crate::i18n::text("ui-sort-by"), "", "") });
    }
    if folder.0 && folder.1 {
        rows.push(Entry { children: column_entries(visible_columns), ..entry(24, crate::i18n::text("ui-columns"), "", "") });
    }
    rows.push(entry(0, "", "", ""));
    if folder.0 {
        rows.push(entry(20, crate::i18n::text("ui-open-in-file-explorer"), "", ""));
        rows.push(entry(21, crate::i18n::text("ui-change-folder"), "", ""));
    }
    rows.push(entry(9, crate::i18n::text("ui-refresh"), "", "F5"));
    rows.push(entry(0, "", "", ""));
    if !locked {
        rows.push(entry(43, crate::i18n::text("ui-rename-panel"), "", ""));
    }
    rows.extend([
        entry(48, crate::i18n::text(if collapsed { "ui-expand-panel" } else { "ui-collapse-panel" }), "", ""),
        entry(0, "", "", ""),
        entry(7, crate::i18n::text("ui-auto-collapse"), if auto_hide { "\u{e73e}" } else { "" }, ""),
        entry(10, crate::i18n::text("ui-lock-panel"), if locked { "\u{e73e}" } else { "" }, ""),
        entry(12, crate::i18n::text("ui-always-on-top"), if topmost { "\u{e73e}" } else { "" }, ""),
        entry(0, "", "", ""),
    ]);
    let mut create = entry(50, crate::i18n::text("ui-new-group"), "", "");
    create.children = vec![
        entry(1, crate::i18n::text("ui-new-group-panel"), "", ""),
        entry(19, crate::i18n::text("ui-new-folder-panel"), "", ""),
    ];
    rows.push(create);
    if !folder.0 {
        let mut children = tab_entries(tab_position, locked);
        if children.len() == 1 {
            rows.push(children.remove(0));
        } else {
            rows.push(Entry { children, ..entry(39, crate::i18n::text("ui-tabs"), "", "") });
        }
    }
    rows.extend([
        entry(0, "", "", ""),
        entry(18, crate::i18n::text("ui-settings-9497"), "", ""),
        entry(0, "", "", ""),
        entry(11, crate::i18n::text("ui-close-panel"), "", ""),
    ]);
    rows
}

#[test]
fn sort_menu_is_available_only_when_unlocked() {
    let rows = pane_entries((false,false),15,false,false,false,false);
    let sort = rows.iter().find(|e|e.id==51).unwrap();
    assert_eq!(sort.children.iter().map(|e|e.id).collect::<Vec<_>>(), vec![60,62,61,63]);
    for (folder,locked) in [(true,true),(false,true)] {
        assert!(!pane_entries((folder,false),15,false,locked,false,false).iter().any(|e|e.id==51));
    }
}

pub(super) fn tab_position(model: &super::GroupModel, target: luciddesk_core::PanelId) -> Option<(usize, usize)> {
    model.tabs.iter().position(|(id, _)| *id == target).map(|index| (index, model.tabs.len()))
}

fn tab_entries(position: Option<(usize, usize)>, locked: bool) -> Vec<Entry> {
    let mut create = entry(40, crate::i18n::text("ui-new-group-tab"), "", "");
    create.enabled = !locked;
    let Some((index, count)) = position.filter(|(index, count)| *count > 1 && index < count) else {
        return vec![create];
    };
    let mut rows = vec![
        create,
        entry(0, "", "", ""),
        entry(46, crate::i18n::text("ui-previous-tab"), "", "Ctrl+Shift+Tab"),
        entry(47, crate::i18n::text("ui-next-tab"), "", "Ctrl+Tab"),
        entry(44, crate::i18n::text("ui-move-left"), "", ""),
        entry(45, crate::i18n::text("ui-move-right"), "", ""),
        entry(49, crate::i18n::text("ui-detach-as-panel"), "", ""),
        entry(0, "", "", ""),
        entry(42, crate::i18n::text("ui-close-tab"), "", "Ctrl+W"),
    ];
    for row in &mut rows {
        row.enabled = match row.id {
            44 => !locked && index > 0,
            45 => !locked && index + 1 < count,
            42 | 49 => !locked,
            _ => row.enabled,
        };
    }
    rows
}

pub(super) fn tab_context_entries(model: &super::GroupModel, topmost: bool, target: luciddesk_core::PanelId) -> Vec<Entry> {
    // Keep every ordinary pane command directly accessible from a tab.
    let position = tab_position(model, target);
    let mut entries = pane_entries_fixed((model.folder.is_some(), model.is_list()),
        model.folder_visible_columns, model.auto_hide, model.locked, topmost, model.collapsed,
        model.fixed_grid, model.free_layout, position);
    let order = if model.folder.is_some() && target == model.active_tab { Some(model.folder_sort) }
        else { model.sort_orders.borrow().get(&target).copied() };
    set_sort_entries(&mut entries, order);
    if target == model.active_tab { set_icon_entries(&mut entries, model.is_list(), model.icon_display); }
    if position.is_none_or(|(_, count)| count <= 1) {
        return entries;
    }
    entries.retain(|row| row.id != 43);
    if target != model.active_tab { entries.retain(|row| !matches!(row.id, 22 | 24)); }
    for row in &mut entries {
        if row.id == 22 {
            // Arrangement commands target the active model, not the clicked tab.
            row.children.retain(|child| !matches!(child.id, 54 | 55));
        } else if row.id == 39 {
            // These commands already appear in the clicked tab's action group.
            row.children.retain(|child| !matches!(child.id, 49 | 42));
        }
        while row.children.last().is_some_and(|child| child.id == 0) {
            row.children.pop();
        }
    }
    entries.dedup_by(|a, b| a.id == 0 && b.id == 0);
    if entries.first().is_some_and(|row| row.id == 0) { entries.remove(0); }
    let actions = tab_entries(position, model.locked);
    let mut tab_actions = vec![actions.iter().find(|row| row.id == 49).unwrap().clone()];
    if !model.locked {
        tab_actions.push(entry(43, crate::i18n::text("ui-rename-tab"), "", ""));
    }
    tab_actions.extend([actions.iter().find(|row| row.id == 42).unwrap().clone(), entry(0, "", "", "")]);
    entries.splice(0..0, tab_actions);
    entries
}

fn sort_entries(order: Option<(u8, bool)>) -> Vec<Entry> {
    [(0, "ui-name"), (2, "ui-modified"), (1, "ui-type"), (3, "ui-size")].into_iter()
        .map(|(column, key)| entry(60 + i32::from(column), crate::i18n::text(key),
            if order.is_some_and(|(selected, _)| selected == column) { "\u{e73e}" } else { "" },
            match order { Some((selected, descending)) if selected == column => if descending { "\u{e70d}" } else { "\u{e70e}" }, _ => "" }))
        .collect()
}
fn set_sort_entries(rows: &mut [Entry], order: Option<(u8, bool)>) {
    if let Some(sort) = rows.iter_mut().find(|row| row.id == 51) { sort.children = sort_entries(order); }
}

pub(super) fn column_entries(visible: u8) -> Vec<Entry> {
    [(1, crate::i18n::text("ui-type")), (2, crate::i18n::text("ui-modified")), (3, crate::i18n::text("ui-size"))].into_iter()
        .map(|(column, label)| entry(30 + column, label, if visible & (1 << column) != 0 { "\u{e73e}" } else { "" }, ""))
        .collect()
}

pub(crate) fn show_entries(
    owner: HWND,
    anchor: POINT,
    anchored: bool,
    theme: luciddesk_core::PanelTheme,
    backdrop: Backdrop,
    rows: Vec<Entry>,
) -> i32 {
    show_level(owner, anchor, anchored, theme, backdrop, rows, None, Rc::new(Cell::new(false)), None)
}

pub(super) fn show_picker(
    owner: HWND,
    appearance: (luciddesk_core::PanelTheme, Backdrop),
    rows: Vec<Entry>,
    options: super::dropdown::Options,
) -> i32 {
    let anchor = POINT { x: options.anchor.left, y: options.anchor.bottom };
    show_level(owner, anchor, false, appearance.0, appearance.1, rows, None, Rc::new(Cell::new(false)), Some(options))
}

fn show_level(
    owner: HWND, anchor: POINT, anchored: bool,
    theme: luciddesk_core::PanelTheme, backdrop: Backdrop, rows: Vec<Entry>,
    parent_row: Option<RECT>,
    resume_parent: Rc<Cell<bool>>,
    picker_options: Option<super::dropdown::Options>,
) -> i32 {
    let accepts_text = picker_options.is_some();
    let dark = super::theme::is_dark(theme);
    let backdrop = default_material(backdrop, dark);
    let scale = unsafe { GetDpiForWindow(owner) }.max(96) as f32 / 96.0;
    let mut animate = 1i32;
    unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut animate).cast(), 0);
    }
    let renderer = if picker_options.is_none() {
        let Ok(renderer) = Renderer::new() else { return 0; };
        Some(renderer)
    } else { None };
    let mut monitor = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        GetMonitorInfoW(
            MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST),
            &raw mut monitor,
        );
    }
    let work = monitor.rcWork;
    let picker_anchor = picker_options.as_ref().map(|options| options.anchor);
    let mut picker = if let Some(options) = picker_options {
        let Ok(popup) = super::dropdown::Popup::new(&rows, options, work, scale) else { return 0; };
        Some(popup)
    } else { None };
    let (left, top, width, height) = if let Some(picker) = &picker {
        let r = picker.bounds;
        (r.left, r.top, r.right - r.left, r.bottom - r.top)
    } else {
        let width = (renderer.as_ref().unwrap().menu_width(&rows) * scale).round() as i32;
        let height = ((row_top(&rows, rows.len()) + 4.0) * scale).round() as i32;
        let left = (if anchored { anchor.x - width } else { anchor.x }).clamp(work.left, (work.right - width).max(work.left));
        let top = anchor.y.clamp(work.top, (work.bottom - height).max(work.top));
        (left, top, width, height)
    };
    let done = Rc::new(Cell::new(false));
    let command = Rc::new(Cell::new(0));
    let done_handler = Rc::clone(&done);
    let command_handler = Rc::clone(&command);
    let resume_handler = Rc::clone(&resume_parent);
    let child_active = Rc::new(Cell::new(false));
    let child_active_handler = Rc::clone(&child_active);
    let pending = Rc::new(Cell::new(None));
    let pending_handler = Rc::clone(&pending);
    let child_rows = if picker.is_none() { rows.clone() } else { Vec::new() };
    let mut surface: Option<Surface> = None;
    let mut selected = picker.as_ref().and_then(|popup| popup.model.focus);
    let mut down: Option<usize> = None;
    let mut picker_drag = None;
    let mut pointer = POINT::default();
    unsafe { GetCursorPos(&raw mut pointer); }
    let mut fade_started: Option<Instant> = None;
    let mut fade: Option<super::animation::Fade> = None;
    let mut fade_finished = animate == 0;
    let mut hover_motion: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(i, _)| super::animation::Motion::settled(if selected == Some(i) { 1.0 } else { 0.0 }, Instant::now()))
        .collect();
    let mut highlights = vec![0.0; rows.len()];
    let mut hover_timer_running = false;
    let window = windows_window::Window::new(crate::i18n::text("ui-group-menu"))
        .style(WS_POPUP)
        .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_NOACTIVATE)
        .size(width, height)
        .on_message(move |raw, message, wparam, lparam| {
            let hwnd = raw.cast();
            let mut activate = None;
            match message {
                WM_ERASEBKGND | WM_NCCALCSIZE => return Some(0),
                WM_DESTROY | WM_CLOSE => {
                    done_handler.set(true);
                    return Some(0);
                }
                WM_ACTIVATE if wparam & 0xffff == WA_INACTIVE as usize => {
                    if !child_active_handler.get() { done_handler.set(true); }
                    return Some(0);
                }
                WM_PAINT => {
                    unsafe {
                        let mut paint = PAINTSTRUCT::default();
                        BeginPaint(hwnd, &raw mut paint);
                        EndPaint(hwnd, &raw const paint);
                    }
                    let result = (|| -> windows::core::Result<()> {
                        if surface.is_none() {
                            let mut value = Surface::new_flyout(
                                windows::Win32::Foundation::HWND(hwnd),
                                if fade_finished { 1.0 } else { 0.0 },
                            )?;
                            value.theme(windows::Win32::Foundation::HWND(hwnd), dark);
                            value.material(windows::Win32::Foundation::HWND(hwnd), backdrop);
                            surface = Some(value);
                        }
                        let value = surface.as_mut().unwrap();
                        let Some(target) = value.try_begin_frame(width as u32, height as u32)?
                        else {
                            return Ok(());
                        };
                        let now = Instant::now();
                        let mut hovering = false;
                        let visible = picker.as_ref().map_or(0..rows.len(), |popup| popup.model.range());
                        for i in visible {
                                let motion = &mut hover_motion[i];
                                let to = if selected == Some(i) { 1.0 } else { 0.0 };
                                let value = motion.retarget_with_duration(
                                    to,
                                    now,
                                    animate != 0,
                                    super::animation::HOVER_DURATION,
                                );
                                hovering |= (value - to).abs() > 0.0001;
                                highlights[i] = value;
                        }
                        if hovering != hover_timer_running {
                            unsafe {
                                hover_timer_running =
                                    hovering && SetTimer(hwnd, 2, USER_TIMER_MINIMUM, None) != 0;
                                if !hover_timer_running {
                                    KillTimer(hwnd, 2);
                                }
                            }
                        }
                        if let Some(picker) = &mut picker {
                            picker.paint(&target, scale, &highlights, value.native, dark)?;
                        } else { renderer.as_ref().unwrap().paint_flyout(
                            &target,
                            width as u32,
                            height as u32,
                            scale,
                            &rows,
                            &highlights,
                            value.native,
                            dark,
                        )?; }
                        value.end_frame()?;
                        // Start after the first frame is ready: device creation and
                        // rasterization must not consume the animation's time budget.
                        if !fade_finished && fade.is_none() {
                            match super::animation::Fade::new(super::animation::MENU_DURATION) {
                                Ok(animation) => {
                                    fade = Some(animation);
                                }
                                Err(error) => {
                                    luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Warn, "pane.menu", &format!("Menu animation unavailable: {error}"));
                                    fade_finished = true;
                                }
                            }
                        }
                        let started = *fade_started.get_or_insert_with(Instant::now);
                        let opacity = if fade_finished {
                            1.0
                        } else {
                            match fade.as_ref().unwrap().sample(started.elapsed()) {
                                Ok(opacity) => opacity,
                                Err(error) => {
                                    luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "pane.menu", &format!("Menu animation failed: {error}"));
                                    fade_finished = true;
                                    1.0
                                }
                            }
                        };
                        value.opacity(opacity)
                    })();
                    if result.is_err() {
                        done_handler.set(true);
                    }
                    return Some(0);
                }
                WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP => {
                    let x = f32::from((lparam as u16).cast_signed()) / scale;
                    let y = f32::from(((lparam >> 16) as u16).cast_signed()) / scale;
                    if picker.is_some() && message == WM_MOUSEMOVE {
                        let current = POINT { x: left + (x * scale).round() as i32, y: top + (y * scale).round() as i32 };
                        if current.x == pointer.x && current.y == pointer.y { return Some(0); }
                        pointer = current;
                    }
                    if let Some(popup) = &mut picker {
                        if message == WM_MOUSEMOVE {
                            let mut tracking = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                            unsafe { TrackMouseEvent(&raw mut tracking); }
                        }
                        if picker_drag.is_some() || (message == WM_LBUTTONDOWN && x >= width as f32 / scale - 10.0 && rows.len() > popup.model.visible) {
                            if message == WM_LBUTTONDOWN {
                                let (top, thumb) = popup.model.scrollbar(height as f32 / scale).unwrap();
                                picker_drag = Some(if y >= top && y <= top + thumb { y - top } else { thumb / 2.0 });
                                unsafe { SetCapture(hwnd); }
                            }
                            popup.model.drag_scrollbar(y, picker_drag.unwrap(), height as f32 / scale);
                            popup.hide_tip();
                            down = None; selected = None; popup.model.focus = None;
                            if message == WM_LBUTTONUP { picker_drag = None; unsafe { ReleaseCapture(); } }
                            unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                            return Some(0);
                        }
                    }
                    let hit = if let Some(popup) = &picker {
                        popup.model.hit(x, y, width as f32 / scale)
                    } else { rows.iter().enumerate().find_map(|(index, row)| {
                        let top = row_top(&rows, index);
                        (row.enabled && row.id != 0
                            && x >= 5.0
                            && x < width as f32 / scale - 5.0
                            && y >= top
                            && y < top + ROW_HEIGHT)
                            .then_some(index)
                    }) };
                    if let Some(popup) = &mut picker {
                        popup.model.focus = hit;
                        if message == WM_MOUSEMOVE { popup.hover_tip(hwnd, &rows, hit, scale); }
                    }
                    if selected != hit {
                        selected = hit;
                        unsafe {
                            KillTimer(hwnd, 3);
                            if hit.is_some_and(|i| !rows[i].children.is_empty()) {
                                SetTimer(hwnd, 3, 200, None);
                            }
                            InvalidateRect(hwnd, std::ptr::null(), 0);
                        }
                    }
                    if message == WM_LBUTTONDOWN {
                        down = hit;
                    }
                    if message == WM_LBUTTONUP && down.take() == hit {
                        activate = hit;
                    }
                }
                WM_MOUSEWHEEL if picker.is_some() => {
                    let popup = picker.as_mut().unwrap();
                    if popup.model.wheel(i32::from(((wparam >> 16) as u16).cast_signed())) {
                        popup.hide_tip(); down = None; selected = None; popup.model.focus = None;
                        unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                    }
                }
                WM_CAPTURECHANGED if picker.is_some() => { picker_drag = None; down = None; }
                windows_sys::Win32::UI::Controls::WM_MOUSELEAVE if picker.is_some() => {
                    picker.as_mut().unwrap().hide_tip();
                    selected = None;
                    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                }
                WM_CHAR if picker.is_some() => {
                    let popup = picker.as_mut().unwrap();
                    if let Some(ch) = char::from_u32(wparam as u32) {
                        popup.model.type_char(ch, Instant::now());
                        selected = popup.model.focus;
                        popup.hide_tip();
                        unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                    }
                }
                WM_SYSKEYDOWN if picker.is_some() && matches!(wparam as u16, VK_UP | VK_DOWN) => { done_handler.set(true); }
                WM_KEYDOWN if picker.is_some() => {
                    let popup = picker.as_mut().unwrap();
                    popup.hide_tip();
                    let page = i32::try_from(popup.model.visible.saturating_sub(1).max(1)).unwrap_or(1);
                    match wparam as u16 {
                        VK_ESCAPE | VK_F4 => done_handler.set(true),
                        VK_TAB => {
                            done_handler.set(true);
                            unsafe { PostMessageW(owner, WM_KEYDOWN, wparam, lparam); }
                        }
                        VK_RETURN | VK_SPACE => activate = selected,
                        VK_UP => popup.model.step(-1),
                        VK_DOWN => popup.model.step(1),
                        VK_HOME => popup.model.edge(false),
                        VK_END => popup.model.edge(true),
                        VK_PRIOR => popup.model.step(-page),
                        VK_NEXT => popup.model.step(page),
                        _ => {}
                    }
                    selected = popup.model.focus;
                    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                }
                WM_KEYDOWN => match wparam as u16 {
                    VK_ESCAPE | VK_LEFT => {
                        resume_handler.set(parent_row.is_some());
                        done_handler.set(true);
                    }
                    VK_RETURN | VK_SPACE | VK_RIGHT => {
                        activate = selected;
                    }
                    VK_UP | VK_DOWN | VK_HOME | VK_END => {
                        selected = keyboard_selection(&rows, selected, wparam as u16);
                        unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
                    }
                    _ => {}
                },
                WM_TIMER if wparam == 2 => unsafe {
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                },
                WM_TIMER if wparam == 3 => {
                    unsafe { KillTimer(hwnd, 3); }
                    activate = selected;
                }
                WM_TIMER if wparam == 1 => {
                    if fade_finished {
                        unsafe {
                            KillTimer(hwnd, 1);
                        }
                    } else {
                        if let (Some(started), Some(value)) = (fade_started, surface.as_mut()) {
                            let opacity = match fade.as_ref().unwrap().sample(started.elapsed()) {
                                Ok(opacity) => opacity,
                                Err(error) => {
                                    luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "pane.menu", &format!("Menu animation failed: {error}"));
                                    1.0
                                }
                            };
                            // Commit the final opacity even if a busy UI thread skips
                            // every tick in the fade interval. No pixel readback/redraw.
                            if value.opacity(opacity).is_err() {
                                done_handler.set(true);
                            }
                            if opacity >= 1.0 {
                                fade_finished = true;
                                fade = None;
                                unsafe {
                                    KillTimer(hwnd, 1);
                                }
                            }
                        }
                    }
                }
                _ => return None,
            }
            if let Some(index) = activate.filter(|&index| rows[index].enabled) {
                if rows[index].children.is_empty() {
                    if message != WM_KEYDOWN || wparam as u16 != VK_RIGHT {
                        command_handler.set(rows[index].id);
                        done_handler.set(true);
                    }
                } else if !child_active_handler.get() {
                    pending_handler.set(Some(index));
                }
            }

            Some(0)
        })
        .create();
    let Ok(window) = window else {
        return 0;
    };
    let hwnd = window.hwnd().cast();
    unsafe {
        crate::window_visibility::prepare_canvas(hwnd);
        // Pickers support type-ahead, including composed characters. Ordinary
        // command menus have no text input and do not need an IME context.
        if accepts_text {
            windows_sys::Win32::UI::Input::Ime::ImmAssociateContextEx(hwnd, std::ptr::null_mut(),
                windows_sys::Win32::UI::Input::Ime::IACE_DEFAULT);
        }
        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner as isize);
        // Prepare the complete first frame while the popup is still hidden.
        // Neither the content target nor the material starts at full opacity.
        SetWindowPos(hwnd, HWND_TOPMOST, left, top, width, height, SWP_NOACTIVATE | SWP_NOOWNERZORDER);
        super::window::round_flyout(hwnd, width, height, CORNER_RADIUS, scale);
        SendMessageW(hwnd, WM_PAINT, 0, 0);
        if done.get() {
            return 0;
        }
        SetWindowPos(hwnd, HWND_TOPMOST, left, top, width, height, SWP_SHOWWINDOW | SWP_NOOWNERZORDER);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        if animate != 0 {
            SetTimer(hwnd, 1, 16, None);
        }
        let mut message = MSG::default();
        while !done.get() {
            let status = GetMessageW(&raw mut message, std::ptr::null_mut(), 0, 0);
            if status <= 0 {
                if status == 0 {
                    PostQuitMessage(i32::try_from(message.wParam).unwrap_or_default());
                }
                break;
            }
            if let Some(row) = parent_row {
                if message.hwnd == owner && message.message == WM_MOUSEMOVE {
                    let mut point = POINT {
                        x: i32::from((message.lParam as u16).cast_signed()),
                        y: i32::from(((message.lParam >> 16) as u16).cast_signed()),
                    };
                    ClientToScreen(owner, &raw mut point);
                    if point.x < row.left || point.x >= row.right || point.y < row.top || point.y >= row.bottom {
                        resume_parent.set(true);
                        DispatchMessageW(&raw const message);
                        break;
                    }
                }
            }
            if let Some(anchor) = picker_anchor {
                if message.hwnd == owner && message.message == WM_MOUSEWHEEL {
                    PostMessageW(owner, message.message, message.wParam, message.lParam);
                    break;
                }
                if matches!(message.message, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN) && message.hwnd != hwnd {
                    let mut point = POINT { x: i32::from((message.lParam as u16).cast_signed()), y: i32::from(((message.lParam >> 16) as u16).cast_signed()) };
                    ClientToScreen(message.hwnd, &raw mut point);
                    if point.x >= anchor.left && point.x < anchor.right && point.y >= anchor.top && point.y < anchor.bottom { break; }
                }
            }
            if anchored && message.hwnd == owner && message.message == WM_LBUTTONDOWN {
                let mut bounds = RECT::default();
                GetClientRect(owner, &raw mut bounds);
                let owner_scale = GetDpiForWindow(owner).max(96) as f32 / 96.0;
                let x = f32::from((message.lParam as u16).cast_signed()) / owner_scale;
                let y = f32::from(((message.lParam >> 16) as u16).cast_signed()) / owner_scale;
                if super::layout::header_button(bounds.right as f32 / owner_scale, x, y) == Some(1)
                {
                    // Consume the toggle click before the owner can arm another
                    // menu open on mouse-up. Other outside clicks still dispatch.
                    break;
                }
            }
            TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
            if let Some(index) = pending.take() {
                child_active.set(true);
                let row = RECT { left, right: left + width,
                    top: top + (row_top(&child_rows, index) * scale).round() as i32,
                    bottom: top + ((row_top(&child_rows, index) + ROW_HEIGHT) * scale).round() as i32 };
                let child_width = (216.0 * scale).round() as i32;
                let child_left = if row.right + child_width <= work.right { row.right } else { row.left - child_width };
                let resume = Rc::new(Cell::new(false));
                let result = show_level(hwnd, POINT { x: child_left, y: row.top - (4.0 * scale) as i32 }, false,
                    theme, backdrop, child_rows[index].children.clone(), Some(row), Rc::clone(&resume), None);
                child_active.set(false);
                if result != 0 { command.set(result); done.set(true); }
                else if resume.get() || GetForegroundWindow() == hwnd { SetFocus(hwnd); }
                else { done.set(true); }
            }
        }
        KillTimer(hwnd, 1);
        KillTimer(hwnd, 2);
        KillTimer(hwnd, 3);
        if (parent_row.is_some() && resume_parent.get())
            || (GetForegroundWindow() == hwnd && (parent_row.is_some() || command.get() != 0))
        {
            // A selected root command belongs to the owner. Destroying the
            // active flyout first can activate an unrelated application.
            SetForegroundWindow(owner);
            SetFocus(owner);
        }
    }
    drop(window);
    debug_assert_eq!(Rc::strong_count(&done), 1, "menu callback was not released");
    command.get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::{ProcessStatus::*, Threading::*};

    fn tab_commands(rows: &[Entry]) -> Vec<(i32, bool)> {
        let mut commands = Vec::new();
        for row in rows {
            if matches!(row.id, 40 | 42 | 44..=47 | 49) {
                commands.push((row.id, row.enabled));
            }
            commands.extend(tab_commands(&row.children));
        }
        commands.sort();
        commands
    }

    fn assert_clean_separators(rows: &[Entry]) {
        assert!(rows.first().is_none_or(|row| row.id != 0));
        assert!(rows.last().is_none_or(|row| row.id != 0));
        assert!(!rows.windows(2).any(|pair| pair[0].id == 0 && pair[1].id == 0));
        for row in rows { assert_clean_separators(&row.children); }
    }

    #[test]
    fn tab_menus_follow_count_target_position_and_lock_state() {
        use luciddesk_core::PanelId;
        for count in [0, 1, 2, 3] {
            for index in 0..count.max(1) {
                for locked in [false, true] {
                    let mut model = super::super::tests::test_model("Tabs");
                    model.tabs = (0..count).map(|i| (PanelId::new(i + 1), format!("Tab {i}"))).collect();
                    model.active_tab = PanelId::new(1);
                    model.locked = locked;
                    let target = PanelId::new(index + 1);
                    let position = tab_position(&model, target);
                    let pane = pane_entries_fixed((false, false), 15, false, locked, false, false, false, false, position);
                    let context = tab_context_entries(&model, false, target);
                    assert_eq!(tab_commands(&pane), tab_commands(&context));
                    for rows in [&pane, &context] {
                        assert_clean_separators(rows);
                        let commands = tab_commands(rows);
                        if count <= 1 {
                            assert_eq!(commands, vec![(40, !locked)]);
                            assert!(!rows.iter().any(|row| row.id == 39));
                            assert!(rows.iter().any(|row| row.id == 40));
                        } else {
                            assert_eq!(commands, vec![
                                (40, !locked), (42, !locked),
                                (44, !locked && index > 0),
                                (45, !locked && index + 1 < count),
                                (46, true), (47, true), (49, !locked),
                            ]);
                            assert!(rows.iter().any(|row| row.id == 39));
                        }
                    }
                    if count > 1 {
                        assert_eq!(super::super::tabs::adjacent_from(&model, target, 1),
                            Some(PanelId::new((index + 1) % count + 1)));
                        assert_eq!(super::super::tabs::adjacent_from(&model, target, -1),
                            Some(PanelId::new((index + count - 1) % count + 1)));
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "Requires an operator to select commands in real menus"]
    fn tab_menus_interactive_validation() {
        let _sta = crate::pane::test_support::apartment();
        let ready = Rc::new(Cell::new(false));
        let ready_handler = ready.clone();
        let owner = windows_window::Window::new("Tab menu validation - press Enter")
            .style(WS_OVERLAPPEDWINDOW)
            .size(320, 240)
            .on_message(move |_, msg, key, _| {
                if msg == WM_KEYDOWN && key == VK_RETURN as usize {
                    ready_handler.set(true);
                    return Some(0);
                }
                (msg == WM_DESTROY).then_some(0)
            })
            .create().unwrap();
        unsafe { ShowWindow(owner.hwnd().cast(), SW_SHOWNOACTIVATE); }
        let started = Instant::now();
        while !ready.get() && started.elapsed().as_secs() < 180 {
            unsafe {
                let mut message = MSG::default();
                while PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(ready.get(), "operator must activate the fixture and press Enter");
        assert_eq!(unsafe { GetForegroundWindow() }, owner.hwnd().cast());
        // Select New tab; Move right; Move left; Next tab, in that order.
        // The final locked menu must keep structural commands disabled.
        for (count, index, locked, context, expected) in [
            (1, 0, false, false, 40),
            (3, 0, false, false, 45),
            (3, 2, false, true, 44),
            (3, 1, true, true, 47),
        ] {
            let mut model = super::super::tests::test_model("Tabs");
            model.tabs = (0..count).map(|i| (luciddesk_core::PanelId::new(i + 1), format!("Tab {i}"))).collect();
            model.active_tab = luciddesk_core::PanelId::new(1);
            model.locked = locked;
            let target = luciddesk_core::PanelId::new(index + 1);
            let rows = if context {
                tab_context_entries(&model, false, target)
            } else {
                pane_entries_fixed((false, false), 15, false, locked, false, false,
                    false, false, tab_position(&model, target))
            };
            println!("count={count} target={index} locked={locked} context={context} expected={expected}");
            let actual = show_entries(owner.hwnd().cast(), POINT { x: 100, y: 100 }, false,
                luciddesk_core::PanelTheme::Dark, Backdrop::Acrylic, rows);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn pane_actions_follow_state_and_keep_exit_in_tray() {
        let open = pane_entries((false, false), 7, false, false, false, false);
        let locked = pane_entries((false, false), 7, true, true, true, true);
        assert!(open.iter().any(|row| row.id == 9));
        assert!(open.iter().any(|row| row.id == 43));
        assert!(!locked.iter().any(|row| row.id == 43));
        assert_eq!(open.iter().find(|row| row.id == 48).unwrap().label,
            crate::i18n::text("ui-collapse-panel"));
        assert_eq!(locked.iter().find(|row| row.id == 48).unwrap().label,
            crate::i18n::text("ui-expand-panel"));
        for (rows, enabled) in [(&open, false), (&locked, true)] {
            assert!(!rows.iter().any(|row| row.id == 4));
            assert_eq!(rows.last().unwrap().id, 11);
            let collapse = rows.iter().position(|row| row.id == 48).unwrap();
            assert_eq!(rows[collapse + 1].id, 0);
            assert_eq!(rows[collapse + 2..collapse + 5].iter().map(|row| row.id).collect::<Vec<_>>(), [7, 10, 12]);
            for id in [7, 10, 12] {
                assert_eq!(!rows.iter().find(|row| row.id == id).unwrap().icon.is_empty(), enabled);
            }
            assert!(!rows.windows(2).any(|pair| pair[0].id == 0 && pair[1].id == 0));
        }
    }

    #[test]
    fn folder_menus_exclude_tabs_and_ordinary_tabs_exclude_folders() {
        assert!(!pane_entries((true, false), 7, false, false, false, false).iter().any(|r| r.id == 39));
        assert!(pane_entries((false, false), 7, false, false, false, false).iter().any(|r| r.id == 40));
        assert!(!tab_entries(Some((0, 2)), false).iter().any(|r| r.id == 41));
    }

    #[test]
    #[ignore = "Opens nested menus on the interactive desktop"]
    fn columns_submenu_opens_on_hover_and_keyboard() {
        unsafe fn owned_popup(owner: HWND) -> HWND {
            unsafe extern "system" fn visit(hwnd: HWND, data: isize) -> i32 {
                unsafe {
                    let pair = &mut *(data as *mut (HWND, HWND));
                    let mut title = [0u16; 64];
                    let length = GetWindowTextW(hwnd, title.as_mut_ptr(), 64);
                    if GetWindow(hwnd, GW_OWNER) == pair.0
                        && String::from_utf16_lossy(&title[..length as usize]) == "面板菜单" {
                        pair.1 = hwnd;
                    }
                }
                1
            }
            let mut pair = (owner, std::ptr::null_mut());
            unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(visit), (&raw mut pair) as isize); }
            pair.1
        }
        let _sta = crate::pane::test_support::apartment();
        for keyboard in [false, true] {
            let stage = Rc::new(Cell::new(0));
            let observed = Rc::clone(&stage);
            let root_window: Cell<HWND> = Cell::new(std::ptr::null_mut());
            let mut ticks = 0;
            let owner = windows_window::Window::new("Submenu fixture")
                .style(WS_POPUP).size(100, 100)
                .on_message(move |raw, msg, _, _| {
                    if msg == WM_DESTROY { return Some(0); }
                    if msg != WM_TIMER { return None; }
                    unsafe {
                        ticks += 1;
                        if root_window.get().is_null() {
                            root_window.set(owned_popup(raw.cast()));
                        }
                        let root = root_window.get();
                        let child = owned_popup(root);
                        let popup = if child.is_null() { root } else { child };
                        let step = observed.get();
                        if ticks > 20 {
                            PostMessageW(popup, WM_CLOSE, 0, 0);
                            PostMessageW(root, WM_CLOSE, 0, 0);
                            return Some(0);
                        }
                        if step == 0 && root != raw.cast() {
                            if keyboard {
                                PostMessageW(root, WM_KEYDOWN, VK_DOWN as usize, 0);
                                PostMessageW(root, WM_KEYDOWN, VK_RIGHT as usize, 0);
                            } else {
                                let scale = GetDpiForWindow(root).max(96) as f32 / 96.0;
                                let p = (16.0 * scale) as isize;
                                PostMessageW(root, WM_MOUSEMOVE, 0, (p << 16) | p);
                            }
                            observed.set(1);
                        } else if step == 1 && popup != root {
                            assert!(IsWindowVisible(root) != 0);
                            PostMessageW(popup, WM_KEYDOWN, VK_LEFT as usize, 0);
                            observed.set(2);
                        } else if step == 2 && popup == root {
                            PostMessageW(root, WM_KEYDOWN, VK_RIGHT as usize, 0);
                            observed.set(3);
                        } else if step == 3 && popup != root {
                            PostMessageW(popup, WM_KEYDOWN, VK_DOWN as usize, 0);
                            PostMessageW(popup, WM_KEYDOWN, VK_RETURN as usize, 0);
                            observed.set(4);
                        }
                    }
                    Some(0)
                }).create().unwrap();
            let hwnd = owner.hwnd().cast();
            let mut parent = entry(24, "显示列", "", "");
            parent.children = column_entries(15);
            unsafe { SetTimer(hwnd, 99, 250, None); }
            let result = show_entries(hwnd, POINT { x: 100, y: 100 }, false,
                luciddesk_core::PanelTheme::Dark, Backdrop::Acrylic, vec![parent]);
            unsafe { KillTimer(hwnd, 99); }
            assert_eq!(stage.get(), 4);
            assert_eq!(result, 31);
        }
    }

    #[test]
    #[ignore = "Opens a real menu; run in an interactive desktop session"]
    fn owner_menu_button_dismisses_without_rearming() {
        let _sta = crate::pane::test_support::apartment();
        let clicks = Rc::new(Cell::new(0));
        let pulses = Rc::new(Cell::new(0));
        let topmost = Rc::new(Cell::new(false));
        let observed_topmost = Rc::clone(&topmost);
        let observed_clicks = Rc::clone(&clicks);
        let observed_pulses = Rc::clone(&pulses);
        let owner = windows_window::Window::new("Menu toggle fixture")
            .style(WS_POPUP)
            .size(320, 240)
            .on_message(move |raw, msg, _, _| {
                let hwnd = raw.cast();
                match msg {
                    WM_DESTROY => Some(0),
                    WM_LBUTTONDOWN => {
                        observed_clicks.set(observed_clicks.get() + 1);
                        Some(0)
                    }
                    WM_TIMER => {
                        observed_pulses.set(observed_pulses.get() + 1);
                        unsafe {
                            if observed_pulses.get() == 1 {
                                let popup =
                                    FindWindowW(std::ptr::null(), windows_sys::w!("面板菜单"));
                                observed_topmost.set(
                                    !popup.is_null()
                                        && GetWindow(popup, GW_OWNER) == hwnd
                                        && GetWindowLongW(popup, GWL_EXSTYLE) as u32
                                            & WS_EX_TOPMOST
                                            != 0,
                                );
                                let mut bounds = RECT::default();
                                GetClientRect(hwnd, &raw mut bounds);
                                let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
                                let x = ((super::super::layout::header_button_x(
                                    bounds.right as f32 / scale,
                                    1,
                                ) + 14.0)
                                    * scale) as isize;
                                let point = (((19.0 * scale) as isize) << 16) | x;
                                PostMessageW(hwnd, WM_LBUTTONDOWN, 0, point);
                            } else {
                                let popup = GetLastActivePopup(hwnd);
                                if popup != hwnd {
                                    PostMessageW(popup, WM_CLOSE, 0, 0);
                                }
                            }
                        }
                        Some(0)
                    }
                    _ => None,
                }
            })
            .create()
            .unwrap();
        let hwnd = owner.hwnd().cast();
        unsafe {
            SetTimer(hwnd, 99, 160, None);
        }
        assert_eq!(
            show(
                hwnd,
                POINT { x: 40, y: 40 },
                true,
                false,
                false,
                luciddesk_core::PanelTheme::Dark,
                Backdrop::Acrylic,
                (false, false), 15, false, false, false, None, Default::default(), None
            ),
            0
        );
        unsafe {
            KillTimer(hwnd, 99);
        }
        assert_eq!(
            pulses.get(),
            1,
            "toggle click must close without the watchdog"
        );
        assert_eq!(clicks.get(), 0, "owner must not arm another menu open");
        assert!(topmost.get(), "menu must be above topmost panes");
    }

    #[test]
    #[ignore = "Opens real menus repeatedly; run alone in an interactive desktop session"]
    fn repeated_open_close_releases_resources() {
        unsafe extern "system" fn close(hwnd: HWND, _: isize) -> i32 {
            let mut title = [0u16; 32];
            unsafe {
                let len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
                if String::from_utf16_lossy(&title[..len.max(0) as usize]) == crate::i18n::text("ui-group-menu") {
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                }
            }
            1
        }
        unsafe extern "system" fn tick(_: HWND, _: u32, _: usize, _: u32) {
            unsafe {
                EnumThreadWindows(GetCurrentThreadId(), Some(close), 0);
            }
        }
        let _sta = crate::pane::test_support::apartment();
        let owner = windows_window::Window::new("Menu lifecycle fixture")
            .style(WS_POPUP)
            .size(320, 240)
            .on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0))
            .create()
            .unwrap();
        let owner = owner.hwnd().cast();
        unsafe {
            assert_ne!(SetTimer(owner, 99, 160, Some(tick)), 0);
        }
        let mut samples = Vec::new();
        let picker = std::env::var("LUCIDDESK_MENU_PROBE").is_ok_and(|value| value == "picker");
        for batch in 0..6 {
            let started = Instant::now();
            for _ in 0..20 {
                if picker {
                    assert_eq!(show_picker(owner, (luciddesk_core::PanelTheme::Dark, Backdrop::Mica),
                        (1..=40).map(|id| entry(id, format!("Item {id}"), "", "")).collect(),
                        super::super::dropdown::Options {
                            anchor: RECT { left: 40, top: 40, right: 300, bottom: 80 }, current: Some(20),
                        }), 0);
                    continue;
                }
                assert_eq!(
                    show(
                        owner,
                        POINT { x: 40, y: 40 },
                        false,
                        false,
                        false,
                        luciddesk_core::PanelTheme::Dark,
                        Backdrop::Mica,
                        (false, false), 15, false, false, false, None, Default::default(), None
                    ),
                    0
                );
            }
            unsafe {
                let process = GetCurrentProcess();
                let mut memory = PROCESS_MEMORY_COUNTERS_EX::default();
                memory.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
                assert_ne!(
                    GetProcessMemoryInfo(process, (&raw mut memory).cast(), memory.cb),
                    0
                );
                let mut handles = 0;
                assert_ne!(GetProcessHandleCount(process, &raw mut handles), 0);
                let gui = (
                    GetGuiResources(process, GR_GDIOBJECTS),
                    GetGuiResources(process, GR_USEROBJECTS),
                );
                println!(
                    "menus={} private_kib={} working_kib={} handles={} gui={gui:?} batch_ms={}",
                    (batch + 1) * 20,
                    memory.PrivateUsage / 1024,
                    memory.WorkingSetSize / 1024,
                    handles,
                    started.elapsed().as_millis()
                );
                samples.push((memory.PrivateUsage, handles, gui));
            }
        }
        unsafe {
            KillTimer(owner, 99);
        }
        let warm = samples[1];
        let last = samples[5];
        assert!(
            last.0 <= warm.0 + 16 * 1024 * 1024,
            "private bytes keep growing: {samples:?}"
        );
        assert!(
            last.1 <= warm.1 + 8,
            "process handles keep growing: {samples:?}"
        );
        // DWM/driver lazy initialization may add a small number of helper windows.
        assert!(
            last.2.0 <= warm.2.0 + 2 && last.2.1 <= warm.2.1 + 2,
            "GUI resources keep growing: {samples:?}"
        );
    }
}

#[test]
fn sort_fields_show_only_the_current_direction() {
    for column in 0..4 {
        for descending in [false, true] {
            let rows = sort_entries(Some((column, descending)));
            assert_eq!(rows.iter().filter(|e| e.icon == "\u{e73e}").count(), 1);
            let selected = rows.iter().find(|e| e.id == 60 + i32::from(column)).unwrap();
            assert_eq!(selected.trailing, if descending { "\u{e70d}" } else { "\u{e70e}" });
        }
    }
    assert!(sort_entries(None).iter().all(|e| e.icon.is_empty() && e.trailing.is_empty()));
}

#[test]
fn keyboard_navigation_skips_disabled_rows_and_wraps() {
    let mut disabled = entry(2, "disabled", "", "");
    disabled.enabled = false;
    let rows = [entry(0, "", "", ""), entry(1, "first", "", ""), disabled,
        entry(3, "last", "", ""), entry(0, "", "", "")];
    for selected in [None, Some(0), Some(2), Some(usize::MAX)] {
        assert_eq!(keyboard_selection(&rows, selected, VK_HOME), Some(1));
        assert_eq!(keyboard_selection(&rows, selected, VK_END), Some(3));
    }
    assert_eq!(keyboard_selection(&rows, Some(1), VK_DOWN), Some(3));
    assert_eq!(keyboard_selection(&rows, Some(3), VK_DOWN), Some(1));
    assert_eq!(keyboard_selection(&rows, Some(1), VK_UP), Some(3));
    assert_eq!(keyboard_selection(&rows, Some(3), VK_UP), Some(1));
    for key in [VK_UP, VK_DOWN, VK_HOME, VK_END] {
        assert_eq!(keyboard_selection(&[], Some(0), key), None);
        assert_eq!(keyboard_selection(&rows[2..3], None, key), None);
    }
}

#[test]
fn view_contains_grid_alignment_and_auto_arrangement() {
    for (fixed, free) in [(false, false), (true, false), (true, true)] {
        let rows = pane_entries_fixed((false, false), 7, false, false, false, false, fixed, free, None);
        for (id, checked) in [(54, !fixed), (55, !free)] {
            let entry = rows.iter().find(|r| r.id == 22).unwrap().children.iter().find(|r| r.id == id).unwrap();
            assert!(entry.enabled);
            assert_eq!(!entry.icon.is_empty(), checked);
        }
        let sort = rows.iter().find(|r| r.id == 51).unwrap();
        assert_eq!(sort.children.iter().map(|r| r.id).collect::<Vec<_>>(), [60, 62, 61, 63]);
    }
}
