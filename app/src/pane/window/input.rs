//! Pointer state, hit testing, grid geometry and drag previews.
use super::*;

pub(super) fn sync_pointer(hwnd: HWND, model: &RefCell<GroupModel>) {
    let mut p = POINT::default();
    let over = unsafe { GetCursorPos(&raw mut p) != 0 && WindowFromPoint(p) == hwnd };
    if over {
        unsafe {
            windows_sys::Win32::Graphics::Gdi::ScreenToClient(hwnd, &raw mut p);
        }
        let m = model.borrow();
        let in_client = pane_hit(client(hwnd), p, scale(hwnd), &m) == HTCLIENT;
        drop(m);
        if in_client {
            update_pointer(hwnd, model, Some(p));
            track_client_leave(hwnd);
            return;
        }
    }
    update_pointer(hwnd, model, None);
}

pub(super) fn track_client_leave(hwnd: HWND) {
    unsafe {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
        let mut track = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };
        TrackMouseEvent(&raw mut track);
    }
}

pub(super) fn update_pointer(hwnd: HWND, model: &RefCell<GroupModel>, pointer: Option<POINT>) {
    let mut m = model.borrow_mut();
    let (button, item, hovered, tab) = if let Some(p) = pointer {
        let viewport = Viewport::read(hwnd);
        let (x, y) = viewport.point(p);
        let grid = viewport.grid(&m);
        let hovered = crate::pane::scrollbar::Bar::for_grid(&m, grid, viewport.width, viewport.height)
            .is_some_and(|bar| bar.contains(x, y));
        (
            m.header_button(viewport.width, x, y),
            if hovered { None } else { m.hit(grid, x, y, viewport.scale) },
            hovered,
            crate::pane::tabs::hit(&m, viewport.width, x, y),
        )
    } else {
        (None, None, false, None)
    };
    if (
        m.hovered_button,
        m.hovered_item,
        m.scrollbar.hovered,
        m.hovered_tab,
    ) != (button, item, hovered, tab)
    {
        m.hovered_tab = tab;
        m.hovered_button = button;
        m.hovered_item = item;
        m.scrollbar.hovered = hovered;
        drop(m);
        invalidate(hwnd);
    }
}

pub(super) fn tab_drag_threshold(origin: POINT, current: POINT, dpi: u32) -> bool {
    use windows_sys::Win32::UI::HiDpi::GetSystemMetricsForDpi;
    let dx = unsafe { GetSystemMetricsForDpi(SM_CXDRAG, dpi) }.max(1) as u32;
    let dy = unsafe { GetSystemMetricsForDpi(SM_CYDRAG, dpi) }.max(1) as u32;
    origin.x.abs_diff(current.x) >= dx || origin.y.abs_diff(current.y) >= dy
}

pub(super) fn pane_hit(r: RECT, p: POINT, scale: f32, model: &GroupModel) -> u32 {
    let viewport = Viewport::from_client(r, scale);
    let (x, y) = viewport.point(p);
    let (width, height) = (viewport.width, viewport.height);
    if crate::pane::tabs::hit(
        model,
        width,
        x,
        y,
    )
    .is_some()
    {
        return HTCLIENT;
    }
    if crate::pane::scrollbar::Bar::for_model(
        model,
        width,
        height,
    )
    .is_some_and(|bar| bar.contains(x, y))
    {
        return HTCLIENT;
    }
    if model
        .header_button(
            width,
            x,
            y,
        )
        .is_some()
    {
        HTCLIENT
    } else {
        let hit = frame_hit(r, p, scale, model.collapsed, model.locked);
        if hit == HTCLIENT && model.tabs.len() > 1 && !model.locked && (y) < HEADER
        {
            HTCAPTION
        } else {
            hit
        }
    }
}

pub(super) fn frame_hit(r: RECT, p: POINT, scale: f32, collapsed: bool, locked: bool) -> u32 {
    let border = (5.0 * scale) as i32;
    match (
        p.x < border,
        p.x >= r.right - border,
        !collapsed && p.y < border,
        !collapsed && p.y >= r.bottom - border,
    ) {
        (true, _, true, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, true, _, true) => HTBOTTOMRIGHT,
        (true, _, _, _) => HTLEFT,
        (_, true, _, _) => HTRIGHT,
        (_, _, true, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        _ if !locked
            && p.y as f32 / scale < HEADER
            && p.x as f32 / scale
                < r.right as f32 / scale - crate::pane::layout::HEADER_BUTTONS_WIDTH =>
        {
            HTCAPTION
        }
        _ => HTCLIENT,
    }
}

pub(super) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
pub(super) fn point(value: isize) -> POINT {
    POINT {
        x: (value as u16).cast_signed().into(),
        y: ((value >> 16) as u16).cast_signed().into(),
    }
}
pub(super) fn scale(hwnd: HWND) -> f32 {
    unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0
}
pub(super) fn client(hwnd: HWND) -> RECT {
    let mut r = RECT::default();
    unsafe {
        GetClientRect(hwnd, &raw mut r);
    }
    r
}

pub(super) fn drag_preview(
    hwnd: HWND,
    m: &GroupModel,
    index: usize,
) -> Option<(crate::pane::assets::Pixels, POINT)> {
    let viewport = Viewport::read(hwnd);
    let s = viewport.scale;
    let grid = viewport.grid(m);
    let selected = if m.selection.contains(&index) {
        m.selection.iter().copied().collect::<Vec<_>>()
    } else {
        vec![index]
    };
    let placeholder = crate::pane::assets::Pixels {
        width: 1,
        height: 1,
        data: vec![0; 4],
    };
    let cells = selected
        .into_iter()
        .filter_map(|index| {
            let item = m.items.get(index)?;
            let render = if m.is_list() {
                crate::pane::drag_drop::image::list_item_pixels
            } else {
                crate::pane::drag_drop::image::item_pixels
            };
            let pixels = render(
                item.image.as_deref().unwrap_or(&placeholder),
                &item.label,
                grid,
                s,
            )?;
            let (x, y) = m.cell(grid, index);
            Some((
                pixels,
                POINT {
                    x: (x * s).round() as i32,
                    y: (y * s).round() as i32,
                },
            ))
        })
        .collect::<Vec<_>>();
    crate::pane::drag_drop::image::selection_pixels(&cells)
}

pub(super) fn update_marquee(
    hwnd: HWND,
    model: &mut GroupModel,
    marquee: &mut crate::pane::marquee::Marquee,
    point: POINT,
) {
    let client = Viewport::read(hwnd);
    let viewport = RectDip {
        x: 0.0,
        y: model.content_header(),
        width: client.width,
        height: (client.height - model.content_header()).max(0.0),
    };
    marquee.update(model, client.grid(model), client.scale, point, viewport);
}

/// One client-coordinate snapshot per operation; never cached across DPI changes.
#[derive(Clone, Copy)]
pub(super) struct Viewport {
    pub scale: f32,
    pub width: f32,
    pub height: f32,
}

impl Viewport {
    pub fn read(hwnd: HWND) -> Self {
        Self::from_client(client(hwnd), scale(hwnd))
    }

    fn from_client(bounds: RECT, scale: f32) -> Self {
        Self {
            scale,
            width: (bounds.right - bounds.left) as f32 / scale,
            height: (bounds.bottom - bounds.top) as f32 / scale,
        }
    }

    pub fn point(self, point: POINT) -> (f32, f32) {
        (point.x as f32 / self.scale, point.y as f32 / self.scale)
    }

    pub fn grid(self, model: &GroupModel) -> Grid {
        model.grid(self.width, self.height)
    }
}

pub(super) fn grid(hwnd: HWND, model: &GroupModel) -> Grid {
    Viewport::read(hwnd).grid(model)
}

pub(super) struct ColumnDrag {
    pub(super) divider: usize,
    pub(super) bounds: [f32; 5],
    pub(super) original: Option<[f32; 4]>,
    pub(super) proportions: [f32; 4],
    pub(super) visible: u8,
}

pub(super) fn scrollbar(hwnd: HWND, model: &GroupModel) -> Option<crate::pane::scrollbar::Bar> {
    let r = client(hwnd);
    let s = scale(hwnd);
    crate::pane::scrollbar::Bar::for_model(model, r.right as f32 / s, r.bottom as f32 / s)
}

pub(super) fn update_scrollbar_animation(
    hwnd: HWND,
    model: &RefCell<GroupModel>,
    motion: &mut crate::pane::animation::Motion,
    enabled: bool,
    timer_running: &mut bool,
) {
    let mut m = model.borrow_mut();
    let visible = scrollbar(hwnd, &m).is_some();
    let animating = m
        .scrollbar
        .animate(motion, std::time::Instant::now(), enabled, visible);
    if animating != *timer_running {
        unsafe {
            *timer_running = animating && SetTimer(hwnd, 4, USER_TIMER_MINIMUM, None) != 0;
            if !*timer_running {
                KillTimer(hwnd, 4);
            }
        }
    }
}

pub(super) fn column_divider(hwnd: HWND, model: &GroupModel, point: POINT) -> Option<usize> {
    if !model.is_list() || model.folder.is_none() || model.collapsed {
        return None;
    }
    let viewport = Viewport::read(hwnd);
    let scale = viewport.scale;
    let grid = viewport.grid(model);
    let top = grid.content_top - crate::pane::layout::LIST_HEADER;
    let y = point.y as f32 / scale;
    if !(top..grid.content_top).contains(&y) {
        return None;
    }
    let x = point.x as f32 / scale - crate::pane::layout::PADDING;
    let columns = model.list_columns(grid.cell_width);
    (1..4).find(|&divider| {
        model.folder_visible_columns & (1 << divider) != 0 && (x - columns[divider]).abs() <= 4.0
    })
}
pub(super) fn invalidate(hwnd: HWND) {
    unsafe {
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

#[cfg(test)]
mod dpi_tests {
    use super::*;

    #[test]
    fn fractional_dpi_preserves_free_icon_hit_and_scrollbar_geometry() {
        let _sta = luciddesk_shell::ShellApartment::initialize_sta().unwrap();
        let state = crate::pane::tests::test_state();
        let mut model = crate::pane::create_model(&state, luciddesk_core::PanelId::new(1)).unwrap();
        model.fixed_grid = true;
        model.free_layout = true;
        model.items[0].details.free_position = Some(luciddesk_core::PointDip::new(31.25, 650.5));
        model.scroll = 500;
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0] {
            let viewport = Viewport::from_client(RECT {
                right: (600.0 * scale) as i32,
                bottom: (400.0 * scale) as i32,
                ..Default::default()
            }, scale);
            let grid = viewport.grid(&model);
            let bounds = model.selection_bounds(grid, 0, scale);
            let px = POINT {
                x: ((bounds.x + bounds.width / 2.0) * scale).round() as i32,
                y: ((bounds.y + 10.0) * scale).round() as i32,
            };
            let (x, y) = viewport.point(px);
            assert_eq!(model.hit(grid, x, y, scale), Some(0));
            assert_eq!(viewport.width, 600.0);
            assert_eq!(viewport.height, 400.0);
            let bar = crate::pane::scrollbar::Bar::for_grid(&model, grid, viewport.width, viewport.height).unwrap();
            let reference = crate::pane::scrollbar::Bar::for_model(&model, 600.0, 400.0).unwrap();
            assert_eq!((bar.max, bar.page, bar.thumb_top), (reference.max, reference.page, reference.thumb_top));
            let (x, y) = viewport.point(POINT { x: -25, y: -50 });
            assert!(x < 0.0 && y < 0.0);
        }
    }
}
