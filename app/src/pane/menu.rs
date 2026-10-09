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
    pub label: &'static str,
    pub icon: &'static str,
    pub trailing: &'static str,
    pub children: Vec<Entry>,
}
pub const ROW_HEIGHT: f32 = 30.0;
pub(super) const CORNER_RADIUS: f32 = 8.0;
pub fn row_top(rows: &[Entry], index: usize) -> f32 {
    4.0 + rows[..index]
        .iter()
        .map(|row| if row.id == 0 { 7.0 } else { ROW_HEIGHT })
        .sum::<f32>()
}
pub(crate) fn entry(
    id: i32,
    label: &'static str,
    icon: &'static str,
    trailing: &'static str,
) -> Entry {
    Entry {
        id,
        label,
        icon,
        trailing,
        children: Vec::new(),
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
) -> i32 {
    let topmost = super::quick_reveal::permanent_topmost(owner);
    show_entries(owner, anchor, anchored, theme, backdrop,
        pane_entries_fixed(folder, visible_columns, auto_hide, locked, topmost, collapsed, fixed_grid, free_layout))
}

fn pane_entries(folder: (bool, bool), visible_columns: u8,
    auto_hide: bool, locked: bool, topmost: bool, collapsed: bool) -> Vec<Entry> {
    pane_entries_fixed(folder, visible_columns, auto_hide, locked, topmost, collapsed, false, false)
}
#[allow(clippy::too_many_arguments)]
fn pane_entries_fixed(folder: (bool, bool), visible_columns: u8,
    auto_hide: bool, locked: bool, topmost: bool, collapsed: bool, fixed_grid: bool, free_layout: bool) -> Vec<Entry> {
    let mut rows = Vec::new();
    if folder.0 {
        rows.push(entry(20, crate::i18n::text("ui-open-in-file-explorer"), "", ""));
    }
    rows.push(entry(9, crate::i18n::text("ui-refresh"), "", "F5"));
    if !locked {
        rows.push(entry(43, crate::i18n::text("ui-rename-panel"), "", ""));
    }
    if folder.0 {
        rows.push(entry(21, crate::i18n::text("ui-change-folder"), "", ""));
    }
    rows.push(entry(0, "", "", ""));
    let mut view = entry(22, crate::i18n::text("ui-view"), "", "");
    view.children = vec![
        entry(25, crate::i18n::text("ui-icons"), if folder.1 { "" } else { "✓" }, ""),
        entry(26, crate::i18n::text("ui-list"), if folder.1 { "✓" } else { "" }, ""),
    ];
    if folder.0 && folder.1 {
        view.children.push(entry(0, "", "", ""));
        let mut columns = entry(24, crate::i18n::text("ui-columns"), "", "");
        columns.children = column_entries(visible_columns);
        view.children.push(columns);
    }
    rows.push(view);
    if !folder.0 && !locked {
        rows.push(entry(54, crate::i18n::text("ui-auto-arrange-icons"), if fixed_grid { "" } else { "✓" }, ""));
        if fixed_grid { rows.push(entry(55, crate::i18n::text("ui-align-icons-to-grid"), if free_layout { "" } else { "✓" }, "")); }
        let mut sort = entry(51, crate::i18n::text("ui-sort-by-name"), "", "");
        sort.children = vec![
            entry(52, crate::i18n::text("ui-sort-ascending"), "", ""),
            entry(53, crate::i18n::text("ui-sort-descending"), "", ""),
        ];
        rows.push(sort);
    }
    rows.extend([
        entry(48, crate::i18n::text(if collapsed { "ui-expand-panel" } else { "ui-collapse-panel" }), "", ""),
        entry(7, crate::i18n::text("ui-auto-collapse"), if auto_hide { "✓" } else { "" }, ""),
        entry(10, crate::i18n::text("ui-lock-panel"), if locked { "✓" } else { "" }, ""),
        entry(12, crate::i18n::text("ui-always-on-top"), if topmost { "✓" } else { "" }, ""),
        entry(0, "", "", ""),
    ]);
    let mut create = entry(50, crate::i18n::text("ui-new-group"), "", "");
    create.children = vec![
        entry(1, crate::i18n::text("ui-new-group-panel"), "", ""),
        entry(19, crate::i18n::text("ui-new-folder-panel"), "", ""),
    ];
    rows.push(create);
    if !folder.0 {
        rows.push(Entry { children: tab_entries(), ..entry(39, crate::i18n::text("ui-tabs"), "", "") });
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
fn ordinary_sort_menu_is_available_only_when_unlocked() {
    let rows = pane_entries((false,false),15,false,false,false,false);
    let sort = rows.iter().find(|e|e.id==51).unwrap();
    assert_eq!(sort.children.iter().map(|e|e.id).collect::<Vec<_>>(), vec![52,53]);
    for (folder,locked) in [(true,false),(false,true)] {
        assert!(!pane_entries((folder,false),15,false,locked,false,false).iter().any(|e|e.id==51));
    }
}

pub(super) fn tab_entries() -> Vec<Entry> {
    vec![
        entry(40, crate::i18n::text("ui-new-group-tab"), "", ""),
        entry(0, "", "", ""),
        entry(46, crate::i18n::text("ui-previous-tab"), "", "Ctrl+Shift+Tab"),
        entry(47, crate::i18n::text("ui-next-tab"), "", "Ctrl+Tab"),
        entry(44, crate::i18n::text("ui-move-left"), "", ""),
        entry(45, crate::i18n::text("ui-move-right"), "", ""),
        entry(49, crate::i18n::text("ui-detach-as-panel"), "", ""),
        entry(0, "", "", ""),
        entry(42, crate::i18n::text("ui-close-tab"), "", "Ctrl+W"),
    ]
}

pub(super) fn tab_context_entries(model: &super::GroupModel, topmost: bool) -> Vec<Entry> {
    // Keep every ordinary pane command directly accessible from a tab.
    let mut entries = pane_entries((model.folder.is_some(), model.is_list()),
        model.folder_visible_columns, model.auto_hide, model.locked, topmost, model.collapsed);
    entries.retain(|row| row.id != 43 && row.id != 54 && row.id != 55);
    let mut tab_actions = vec![entry(49, crate::i18n::text("ui-detach-as-panel"), "", "")];
    if !model.locked {
        tab_actions.push(entry(43, crate::i18n::text("ui-rename-tab"), "", ""));
    }
    tab_actions.extend([entry(42, crate::i18n::text("ui-close-tab"), "", "Ctrl+W"), entry(0, "", "", "")]);
    entries.splice(0..0, tab_actions);
    entries
}

pub(super) fn column_entries(visible: u8) -> Vec<Entry> {
    [(1, crate::i18n::text("ui-type")), (2, crate::i18n::text("ui-modified")), (3, crate::i18n::text("ui-size"))].into_iter()
        .map(|(column, label)| entry(30 + column, label, if visible & (1 << column) != 0 { "✓" } else { "" }, ""))
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
    show_level(owner, anchor, anchored, theme, backdrop, rows, None, Rc::new(Cell::new(false)))
}

fn show_level(
    owner: HWND, anchor: POINT, anchored: bool,
    theme: luciddesk_core::PanelTheme, backdrop: Backdrop, rows: Vec<Entry>,
    parent_row: Option<RECT>,
    resume_parent: Rc<Cell<bool>>,
) -> i32 {
    let dark = super::theme::is_dark(theme);
    let backdrop = default_material(backdrop, dark);
    let scale = unsafe { GetDpiForWindow(owner) }.max(96) as f32 / 96.0;
    let mut animate = 1i32;
    unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut animate).cast(), 0);
    }
    let Ok(renderer) = Renderer::new() else { return 0; };
    let menu_width = renderer.menu_width(&rows);
    let width = (menu_width * scale).round() as i32;
    let height = ((row_top(&rows, rows.len()) + 4.0) * scale).round() as i32;
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
    let left = (if anchored { anchor.x - width } else { anchor.x })
        .clamp(work.left, (work.right - width).max(work.left));
    let top = anchor
        .y
        .clamp(work.top, (work.bottom - height).max(work.top));
    let done = Rc::new(Cell::new(false));
    let command = Rc::new(Cell::new(0));
    let done_handler = Rc::clone(&done);
    let command_handler = Rc::clone(&command);
    let resume_handler = Rc::clone(&resume_parent);
    let child_active = Rc::new(Cell::new(false));
    let child_active_handler = Rc::clone(&child_active);
    let pending = Rc::new(Cell::new(None));
    let pending_handler = Rc::clone(&pending);
    let child_rows = rows.clone();
    let mut surface: Option<Surface> = None;
    let mut selected: Option<usize> = None;
    let mut down: Option<usize> = None;
    let mut fade_started: Option<Instant> = None;
    let mut fade: Option<super::animation::Fade> = None;
    let mut fade_finished = animate == 0;
    let mut hover_motion: Vec<_> = rows
        .iter()
        .map(|_| super::animation::Motion::settled(0.0, Instant::now()))
        .collect();
    let mut hover_timer_running = false;
    let window = windows_window::Window::new(crate::i18n::text("ui-group-menu"))
        .style(WS_POPUP)
        .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST)
        .size(width, height)
        .on_message(move |raw, message, wparam, lparam| {
            let hwnd = raw.cast();
            let mut activate = None;
            match message {
                WM_DESTROY | WM_ERASEBKGND | WM_NCCALCSIZE => return Some(0),
                WM_CLOSE => {
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
                        let highlights: Vec<_> = hover_motion
                            .iter_mut()
                            .enumerate()
                            .map(|(i, motion)| {
                                let to = if selected == Some(i) { 1.0 } else { 0.0 };
                                let value = motion.retarget_with_duration(
                                    to,
                                    now,
                                    animate != 0,
                                    super::animation::HOVER_DURATION,
                                );
                                hovering |= (value - to).abs() > 0.0001;
                                value
                            })
                            .collect();
                        if hovering != hover_timer_running {
                            unsafe {
                                hover_timer_running =
                                    hovering && SetTimer(hwnd, 2, USER_TIMER_MINIMUM, None) != 0;
                                if !hover_timer_running {
                                    KillTimer(hwnd, 2);
                                }
                            }
                        }
                        renderer.paint_flyout(
                            &target,
                            width as u32,
                            height as u32,
                            scale,
                            &rows,
                            &highlights,
                            value.native,
                            dark,
                        )?;
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
                    let hit = rows.iter().enumerate().find_map(|(index, row)| {
                        let top = row_top(&rows, index);
                        (row.id != 0
                            && x >= 5.0
                            && x < width as f32 / scale - 5.0
                            && y >= top
                            && y < top + ROW_HEIGHT)
                            .then_some(index)
                    });
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
                WM_KEYDOWN => match wparam as u16 {
                    VK_ESCAPE | VK_LEFT => {
                        resume_handler.set(parent_row.is_some());
                        done_handler.set(true);
                    }
                    VK_RETURN | VK_SPACE | VK_RIGHT => {
                        activate = selected;
                    }
                    VK_UP | VK_DOWN => {
                        let indices: Vec<_> = rows
                            .iter()
                            .enumerate()
                            .filter_map(|(i, row)| (row.id != 0).then_some(i))
                            .collect();
                        let current =
                            selected.and_then(|index| indices.iter().position(|&i| i == index));
                        let next = if wparam as u16 == VK_DOWN {
                            current.map_or(0, |i| (i + 1) % indices.len())
                        } else {
                            current.map_or(indices.len() - 1, |i| {
                                (i + indices.len() - 1) % indices.len()
                            })
                        };
                        selected = Some(indices[next]);
                        unsafe {
                            InvalidateRect(hwnd, std::ptr::null(), 0);
                        }
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
            if let Some(index) = activate {
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
                    theme, backdrop, child_rows[index].children.clone(), Some(row), Rc::clone(&resume));
                child_active.set(false);
                if result != 0 { command.set(result); done.set(true); }
                else if resume.get() || GetForegroundWindow() == hwnd { SetFocus(hwnd); }
                else { done.set(true); }
            }
        }
        KillTimer(hwnd, 1);
        KillTimer(hwnd, 2);
        KillTimer(hwnd, 3);
        if parent_row.is_some() && (resume_parent.get() || GetForegroundWindow() == hwnd) {
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
        for rows in [&open, &locked] {
            assert!(!rows.iter().any(|row| row.id == 4));
            assert_eq!(rows.last().unwrap().id, 11);
            assert!(!rows.windows(2).any(|pair| pair[0].id == 0 && pair[1].id == 0));
        }
    }

    #[test]
    fn folder_menus_exclude_tabs_and_ordinary_tabs_exclude_folders() {
        assert!(!pane_entries((true, false), 7, false, false, false, false).iter().any(|r| r.id == 39));
        assert!(pane_entries((false, false), 7, false, false, false, false).iter().any(|r| r.id == 39));
        assert!(!tab_entries().iter().any(|r| r.id == 41));
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
                (false, false), 15, false, false, false
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
                if String::from_utf16_lossy(&title[..len.max(0) as usize]) == "面板菜单" {
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
        for batch in 0..6 {
            let started = Instant::now();
            for _ in 0..20 {
                assert_eq!(
                    show(
                        owner,
                        POINT { x: 40, y: 40 },
                        false,
                        false,
                        false,
                        luciddesk_core::PanelTheme::Dark,
                        Backdrop::Mica,
                        (false, false), 15, false, false, false
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
                    "menus={} private_kib={} handles={} gui={gui:?} batch_ms={}",
                    (batch + 1) * 20,
                    memory.PrivateUsage / 1024,
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
