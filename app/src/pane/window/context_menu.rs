//! Deferred panel, tab and Shell context menus. Never retain a model borrow across dispatch.
use super::*;

pub(super) fn show<F: FnMut(Event) -> bool>(
    hwnd: HWND,
    lparam: isize,
    wparam: usize,
    model: &Rc<RefCell<GroupModel>>,
    events: &Rc<RefCell<F>>,
) {
    let event = |value| (events.borrow_mut())(value);
    let mut anchor = point(lparam);
    let viewport = input::Viewport::read(hwnd);
    let mut tab_context = None;
    if lparam != -1 {
        let mut p = anchor;
        unsafe {
            windows_sys::Win32::Graphics::Gdi::ScreenToClient(hwnd, &raw mut p);
        }
        let tab = crate::pane::tabs::hit(
            &model.borrow(),
            viewport.width,
            p.x as f32 / viewport.scale,
            p.y as f32 / viewport.scale,
        );
        if let Some(tab) = tab {
            tab_context = Some(tab);
        }
        let column_menu = {
            let m = model.borrow();
            (m.folder.is_some()
                && m.is_list()
                && !m.collapsed
                && (m.content_header()..m.content_header() + crate::pane::layout::LIST_HEADER)
                    .contains(&(p.y as f32 / viewport.scale)))
            .then_some((m.theme, m.backdrop, m.folder_visible_columns))
        };
        if let Some((theme, backdrop, visible)) = column_menu {
            let command = crate::pane::menu::show_entries(
                hwnd,
                anchor,
                false,
                theme,
                backdrop,
                crate::pane::menu::column_entries(visible),
            );
            if (31..=33).contains(&command) {
                event(Event::ToggleFolderColumn((command - 30) as u8));
            }
            return;
        }
    }
    let index = if lparam == -1 {
        if wparam != 0 {
            let m = model.borrow();
            m.selected
                .filter(|i| m.selection.contains(i))
                .or_else(|| m.selection.first().copied())
        } else {
            None
        }
    } else {
        let mut p = anchor;
        unsafe {
            windows_sys::Win32::Graphics::Gdi::ScreenToClient(hwnd, &raw mut p);
        }
        let m = model.borrow();
        m.hit(
            viewport.grid(&m),
            p.x as f32 / viewport.scale,
            p.y as f32 / viewport.scale,
            viewport.scale,
        )
    };
    if let Some(index) = index {
        let (identity, identities) = {
            let mut m = model.borrow_mut();
            if !m.selection.contains(&index) {
                m.select_item(index, false, false);
            }
            m.selected = Some(index);
            (m.items[index].identity.clone(), m.selected_identities())
        };
        if lparam == -1 {
            let m = model.borrow();
            let g = viewport.grid(&m);
            let (x, y) = m.cell(g, index);
            let s = viewport.scale;
            let mut rect = RECT::default();
            unsafe {
                GetClientRect(hwnd, &raw mut rect);
                anchor = POINT {
                    x: ((x + g.cell_width * 0.5) * s)
                        .round()
                        .clamp(0.0, rect.right.max(0) as f32) as i32,
                    y: ((y + g.icon_size) * s)
                        .round()
                        .clamp(0.0, rect.bottom.max(0) as f32) as i32,
                };
                windows_sys::Win32::Graphics::Gdi::ClientToScreen(hwnd, &raw mut anchor);
            }
        }
        invalidate(hwnd);
        if model.borrow().folder.is_some() || !crate::pane::compact_menu::enabled() {
            let result = luciddesk_shell::show_file_items_menu(
                windows::Win32::Foundation::HWND(hwnd),
                &identities,
                windows::Win32::Foundation::POINT {
                    x: anchor.x,
                    y: anchor.y,
                },
            );
            match result {
                Ok(true) => {
                    event(Event::RenameItem(identity));
                }
                Ok(false) => {}
                Err(message) => error(&message.to_string()),
            }
            return;
        }
        let reply = Rc::new(RefCell::new(None));
        event(Event::BeginItemMenu(Rc::clone(&reply)));
        let hook = reply
            .borrow_mut()
            .take()
            .unwrap_or_else(|| Err(crate::i18n::text("ui-menu-is-not-ready").into()));
        let hook = match hook {
            Ok(hook) => hook,
            Err(message) => {
                error(&message);
                return;
            }
        };
        let result =
            crate::pane::shell_menu::show_many(hwnd, &hook, &identities, anchor, lparam == -1);
        event(Event::EndItemMenu);
        match result {
            Ok(true) => {
                event(Event::RenameItem(identity));
            }
            Ok(false) => {}
            Err(message) => error(&message),
        }
        // Inventory polling detects rename/delete; dismissing a menu must not
        // discard every image and trigger a visible reload.
        return;
    }
    let background = {
        let m = model.borrow();
        let y = if lparam == -1 {
            None
        } else {
            let mut p = anchor;
            unsafe {
                windows_sys::Win32::Graphics::Gdi::ScreenToClient(hwnd, &raw mut p);
            }
            Some(p.y as f32 / viewport.scale)
        };
        crate::pane::folder_context::is_content_background(&m, y, wparam != 0)
    };
    if background && tab_context.is_none() {
        model.borrow_mut().clear_selection();
        update_pointer(hwnd, &model, None);
        invalidate(hwnd);
        if lparam == -1 {
            anchor = POINT {
                x: (16.0 * viewport.scale) as i32,
                y: ((model.borrow().content_header() + 48.0) * viewport.scale) as i32,
            };
            unsafe {
                ClientToScreen(hwnd, &raw mut anchor);
            }
        }
        if let Err(message) = crate::pane::folder_context::show(hwnd, anchor, &model, |value| {
            event(value);
        }) {
            error(&message);
        }
        update_pointer(hwnd, &model, None);
        invalidate(hwnd);
        return;
    }
    let (auto_hide, locked, theme, backdrop, collapsed) = {
        let m = model.borrow();
        (m.auto_hide, m.locked, m.theme, m.backdrop, m.collapsed)
    };
    update_pointer(hwnd, &model, None);
    invalidate(hwnd);
    let is_folder = {
        let model = model.borrow();
        (model.folder.is_some(), model.is_list())
    };
    let visible_columns = model.borrow().folder_visible_columns;
    let fixed_grid = model.borrow().fixed_grid;
    let free_layout = model.borrow().free_layout;
    let command = if tab_context.is_some() {
        let entries = crate::pane::menu::tab_context_entries(
            &model.borrow(),
            crate::pane::quick_reveal::permanent_topmost(hwnd),
        );
        crate::pane::menu::show_entries(hwnd, anchor, false, theme, backdrop, entries)
    } else {
        menu(
            hwnd,
            lparam,
            auto_hide,
            locked,
            theme,
            backdrop,
            is_folder,
            visible_columns,
            collapsed,
            fixed_grid,
            free_layout,
        )
    };
    update_pointer(hwnd, &model, None);
    invalidate(hwnd);
    match command {
        54 => { event(Event::ToggleFixedGrid); },
        55 => { event(Event::ToggleGridAlignment); },
        52 | 53 => dispatch_sort_menu(&model, tab_context, command == 53, |action| {
            event(action);
        }),
        49 => {
            let id = tab_context.unwrap_or(model.borrow().active_tab);
            event(Event::DetachTab(id));
        }
        48 => {
            event(Event::Collapse);
        }
        40 => {
            event(Event::NewTab(false));
        }
        41 => {
            event(Event::NewTab(true));
        }
        42 => {
            let id = tab_context.unwrap_or(model.borrow().active_tab);
            event(Event::CloseTabId(id));
        }
        43 => {
            if let Some(id) = tab_context {
                event(Event::RenameTab(id));
            } else {
                event(Event::RenameTitle);
            }
        }
        44 | 45 => {
            let step = if command == 44 { -1 } else { 1 };
            if let Some(id) = tab_context {
                event(Event::MoveTabId(id, step));
            } else {
                event(Event::MoveTab(step));
            }
        }
        46 | 47 => {
            let next =
                crate::pane::tabs::adjacent(&model.borrow(), if command == 46 { -1 } else { 1 });
            if let Some(next) = next {
                event(Event::SelectTab(next));
            }
        }
        31..=33 => {
            event(Event::ToggleFolderColumn((command - 30) as u8));
        }
        10 => {
            event(Event::ToggleLocked);
        }
        25 | 26 => {
            if model.borrow().is_list() != (command == 26) {
                event(Event::ToggleListView);
            }
        }
        19 => {
            event(Event::NewFolder);
        }
        20 => {
            event(Event::OpenFolder);
        }
        21 => {
            event(Event::ChangeFolder);
        }
        1 => {
            event(Event::New);
        }
        4 => {
            event(Event::Exit);
        }
        7 => {
            event(Event::ToggleAutoHide);
        }
        12 => {
            event(Event::ToggleTopmost);
        }
        9 => {
            event(Event::Refresh);
        }
        11 => {
            event(Event::ClosePane);
        }
        18 => {
            event(Event::Settings);
        }
        _ => {}
    }
}

fn menu(
    hwnd: HWND,
    lparam: isize,
    auto_hide: bool,
    locked: bool,
    theme: luciddesk_core::PanelTheme,
    backdrop: luciddesk_core::Backdrop,
    folder: (bool, bool),
    visible_columns: u8,
    collapsed: bool,
    fixed_grid: bool,
    free_layout: bool,
) -> i32 {
    let anchored = lparam == -1;
    let mut anchor = point(lparam);
    if anchored {
        let dpi = scale(hwnd);
        anchor = POINT {
            x: client(hwnd).right - (10.0 * dpi) as i32,
            y: ((HEADER + 4.0) * dpi) as i32,
        };
        unsafe {
            ClientToScreen(hwnd, &raw mut anchor);
        }
    }
    crate::pane::menu::show(
        hwnd,
        anchor,
        anchored,
        auto_hide,
        locked,
        theme,
        backdrop,
        folder,
        visible_columns,
        collapsed,
        fixed_grid,
        free_layout,
    )
}
