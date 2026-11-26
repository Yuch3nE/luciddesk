//! Shared, themed dialog used by the desktop client and the standalone updater.
//! Native controls retain keyboard navigation, selection and accessibility names.
use super::MessageKind;
use crate::control_style::{self as style, Palette, colorref};
use crate::controls::{
    ButtonKind, State,
    gdi::{self, GdiCanvas},
    motion::PointerMotion,
};
use std::{
    cell::{Cell, RefCell},
    mem::size_of,
    ptr::{null, null_mut},
    sync::atomic::{AtomicI8, Ordering},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    System::{DataExchange::*, LibraryLoader::GetModuleHandleW, Memory::*, Registry::*},
    UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

const COPY: i32 = 100;
const HEADING: i32 = 101;
const BODY: i32 = 102;
const BODY_SCROLLBAR: i32 = 103;
const APP_ICON_RESOURCE: u16 = 1;
const PADDING: i32 = 24;
const HEADING_ICON_SIZE: i32 = 20;
const HEADING_ROW_HEIGHT: i32 = 28;
static THEME: AtomicI8 = AtomicI8::new(-1);
thread_local! {
    static LABELS: RefCell<[String; 3]> = RefCell::new(["OK".into(), "Cancel".into(), "Copy".into()]);
    static FONT_FAMILY: RefCell<Vec<u16>> = RefCell::new(wide("Segoe UI"));
}
pub fn set_message_labels(ok: &str, cancel: &str, copy: &str) {
    LABELS.with(|labels| *labels.borrow_mut() = [ok.into(), cancel.into(), copy.into()]);
}
pub fn set_message_theme(dark: Option<bool>) {
    THEME.store(dark.map_or(-1, i8::from), Ordering::Relaxed);
}
pub fn set_message_font(family: &str) {
    FONT_FAMILY.with(|font| *font.borrow_mut() = wide(family));
}
pub fn message_is_dark() -> bool {
    match THEME.load(Ordering::Relaxed) {
        0 => false,
        1 => true,
        _ => {
            let mut light = 1u32;
            let mut size = size_of::<u32>() as u32;
            unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    windows_sys::w!(
                        "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"
                    ),
                    windows_sys::w!("AppsUseLightTheme"),
                    RRF_RT_REG_DWORD,
                    null_mut(),
                    (&raw mut light).cast(),
                    &raw mut size,
                );
            }
            light == 0
        }
    }
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
pub(super) enum Buttons<'a> {
    Dismiss,
    Confirm,
    Action(&'a str),
}

struct Dialog {
    title: Vec<u16>,
    body: Vec<u16>,
    copy_text: Vec<u16>,
    labels: [String; 3],
    family: Vec<u16>,
    confirmation: bool,
    compact: bool,
    dark: bool,
    kind: MessageKind,
    background: HBRUSH,
    font: Cell<HFONT>,
    heading_font: Cell<HFONT>,
    caption_icon: Cell<HICON>,
    heading_row_height: Cell<i32>,
    scale: Cell<f32>,
    button_motion: RefCell<[PointerMotion; 3]>,
}
impl Dialog {
    fn background_color(&self) -> u32 {
        colorref(Palette::for_theme(self.dark).page_background)
    }
    fn heading_icon(&self) -> (&'static str, u32) {
        match self.kind {
            MessageKind::Error => (
                style::ERROR_BADGE,
                if self.dark { 0xff99a4 } else { 0xaa2632 },
            ),
            MessageKind::Warning => (style::WARNING, if self.dark { 0xf4c468 } else { 0x8d5c00 }),
            MessageKind::Information => (style::INFO, Palette::for_theme(self.dark).accent),
        }
    }
    fn horizontal_padding(&self) -> i32 {
        self.px(if self.compact { 20 } else { PADDING })
    }
    fn vertical_padding(&self) -> i32 {
        self.px(if self.compact { 16 } else { PADDING })
    }

    fn update_caption_icon(&self, hwnd: HWND, dpi: u32) {
        unsafe {
            // LucidDesk and the updater both embed the app icon at resource ID 1.
            let icon = LoadImageW(
                GetModuleHandleW(null()),
                usize::from(APP_ICON_RESOURCE) as _,
                IMAGE_ICON,
                GetSystemMetricsForDpi(SM_CXSMICON, dpi),
                GetSystemMetricsForDpi(SM_CYSMICON, dpi),
                0,
            );
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, icon as isize);
                let previous = self.caption_icon.replace(icon);
                if !previous.is_null() {
                    DestroyIcon(previous);
                }
            }
        }
    }

    fn px(&self, value: i32) -> i32 {
        self.dip(value as f32)
    }
    fn dip(&self, value: f32) -> i32 {
        (value * self.scale.get()).round() as i32
    }
}
impl Drop for Dialog {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.background);
            DeleteObject(self.font.get());
            DeleteObject(self.heading_font.get());
            if !self.caption_icon.get().is_null() {
                DestroyIcon(self.caption_icon.get());
            }
        }
    }
}

#[repr(C, align(4))]
struct Template {
    header: DLGTEMPLATE,
    menu: u16,
    class: u16,
    title: u16,
}

pub(super) fn show(
    owner: isize,
    title: &str,
    body: &str,
    kind: MessageKind,
    buttons: Buttons<'_>,
) -> bool {
    let _antialiasing = gdi::Antialiasing::start();
    let confirmation = !matches!(buttons, Buttons::Dismiss);
    let mut labels = LABELS.with(|labels| labels.borrow().clone());
    let compact = if let Buttons::Action(action) = buttons {
        labels[0] = action.to_owned();
        true
    } else {
        false
    };
    let mut dialog = Dialog {
        title: wide(title),
        body: wide(body),
        copy_text: wide(&format!("{title}\r\n\r\n{body}")),
        labels,
        family: FONT_FAMILY.with(|family| family.borrow().clone()),
        confirmation,
        compact,
        dark: message_is_dark(),
        kind,
        background: null_mut(),
        font: Cell::new(null_mut()),
        heading_font: Cell::new(null_mut()),
        caption_icon: Cell::new(null_mut()),
        heading_row_height: Cell::new(0),
        scale: Cell::new(1.0),
        button_motion: RefCell::default(),
    };
    dialog.background = unsafe { CreateSolidBrush(dialog.background_color()) };
    let template = Template {
        header: DLGTEMPLATE {
            style: WS_POPUP | WS_CAPTION | WS_SYSMENU,
            dwExtendedStyle: 0,
            cdit: 0,
            x: 0,
            y: 0,
            cx: 320,
            cy: 170,
        },
        menu: 0,
        class: 0,
        title: 0,
    };
    let result = unsafe {
        DialogBoxIndirectParamW(
            null_mut(),
            &raw const template.header,
            owner as HWND,
            Some(procedure),
            (&raw mut dialog) as isize,
        )
    };
    // Emergency fallback only: never silently discard an error if window creation fails.
    if result == -1 {
        return unsafe {
            MessageBoxW(
                owner as HWND,
                dialog.body.as_ptr(),
                dialog.title.as_ptr(),
                if confirmation {
                    MB_OKCANCEL | MB_DEFBUTTON2 | MB_ICONWARNING
                } else {
                    MB_OK | MB_ICONERROR
                },
            )
        } == IDOK;
    }
    result == IDOK as isize
}

unsafe fn create(hwnd: HWND, id: i32, class: *const u16, text: *const u16, style: u32) -> bool {
    let child = unsafe {
        CreateWindowExW(
            0,
            class,
            text,
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            1,
            1,
            hwnd,
            id as usize as _,
            null_mut(),
            null(),
        )
    };
    if child.is_null() {
        return false;
    }
    unsafe {
        SetWindowSubclass(child, Some(control_proc), 1, hwnd as usize);
    }
    true
}

unsafe extern "system" fn control_proc(
    hwnd: HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    _: usize,
    parent: usize,
) -> isize {
    // Dialog keyboard navigation changes pushbutton styles; retain our owner drawing.
    if msg == BM_SETSTYLE {
        return 0;
    }
    if msg == WM_TIMER && wp == gdi::POINTER_TIMER {
        unsafe { InvalidateRect(hwnd, null(), 0); }
        return 0;
    }
    if msg == WM_NCMOUSEMOVE && unsafe { GetDlgCtrlID(hwnd) } == BODY {
        unsafe {
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE | TME_NONCLIENT,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&raw mut tracking);
        }
    }
    if msg == WM_MOUSEMOVE || msg == WM_MOUSELEAVE {
        unsafe {
            if msg == WM_MOUSEMOVE {
                let mut tracking = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&raw mut tracking);
            }
            InvalidateRect(hwnd, null(), 0);
        }
    }
    if msg == WM_KEYDOWN && wp == b'C' as usize && unsafe { GetKeyState(VK_CONTROL as i32) } < 0 {
        // Preserve native selection copying inside the message body.
        let mut start = 0u32;
        let mut end = 0u32;
        if unsafe { GetDlgCtrlID(hwnd) } == BODY {
            unsafe {
                SendMessageW(
                    hwnd,
                    EM_GETSEL,
                    (&raw mut start) as usize,
                    (&raw mut end) as isize,
                );
            }
        }
        if start == end {
            unsafe {
                SendMessageW(parent as HWND, WM_COMMAND, COPY as usize, 0);
            }
            return 0;
        }
    }
    let result = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
    unsafe {
        if GetDlgCtrlID(hwnd) == BODY
            && matches!(
                msg,
                WM_NCPAINT | WM_VSCROLL | WM_NCLBUTTONDOWN | WM_NCMOUSEMOVE | WM_NCMOUSELEAVE
                    | WM_CAPTURECHANGED | WM_MOUSEWHEEL | WM_KEYDOWN
            )
        {
            invalidate_scrollbar(parent as HWND);
        }
    }
    result
}

unsafe fn invalidate_scrollbar(dialog: HWND) {
    unsafe {
        let bar = GetDlgItem(dialog, BODY_SCROLLBAR);
        if !bar.is_null() {
            InvalidateRect(bar, null(), 0);
        }
    }
}

unsafe fn clipboard(hwnd: HWND, value: &[u16]) {
    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return;
        }
        let memory = GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of_val(value));
        if !memory.is_null() {
            let address = GlobalLock(memory);
            if !address.is_null() {
                std::ptr::copy_nonoverlapping(value.as_ptr(), address.cast(), value.len());
                GlobalUnlock(memory);
                if EmptyClipboard() == 0 || SetClipboardData(13, memory).is_null() {
                    GlobalFree(memory);
                }
            } else {
                GlobalFree(memory);
            }
        }
        CloseClipboard();
    }
}

unsafe fn layout(hwnd: HWND, dialog: &Dialog, dpi: u32) {
    unsafe {
        let dpi = dpi.max(96);
        dialog.scale.set(dpi as f32 / 96.0);
        dialog.update_caption_icon(hwnd, dpi);
        let font = |height, weight| {
            CreateFontW(
                -dialog.dip(height),
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                CLEARTYPE_QUALITY as u32,
                0,
                dialog.family.as_ptr(),
            )
        };
        let old_font = dialog.font.replace(font(style::TEXT, FW_NORMAL as i32));
        let old_heading = dialog.heading_font.replace(font(20.0, FW_SEMIBOLD as i32));
        for id in [IDOK, IDCANCEL, COPY, BODY, HEADING] {
            let child = GetDlgItem(hwnd, id);
            if child.is_null() {
                continue;
            }
            SendMessageW(
                child,
                WM_SETFONT,
                if id == HEADING {
                    dialog.heading_font.get()
                } else {
                    dialog.font.get()
                } as usize,
                1,
            );
        }
        DeleteObject(old_font);
        DeleteObject(old_heading);
        let mut monitor = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..std::mem::zeroed()
        };
        let owner = GetWindow(hwnd, GW_OWNER);
        GetMonitorInfoW(
            MonitorFromWindow(
                if owner.is_null() { hwnd } else { owner },
                MONITOR_DEFAULTTONEAREST,
            ),
            &raw mut monitor,
        );
        let work = monitor.rcWork;
        let width = dialog
            .px(if dialog.compact { 400 } else { 520 })
            .min((work.right - work.left - dialog.px(32)).max(dialog.px(240)));
        let padding = dialog.horizontal_padding();
        let vertical_padding = dialog.vertical_padding();
        let heading_inset = dialog.dip(HEADING_ICON_SIZE as f32 + style::ICON_GAP);
        let heading_left = padding + heading_inset;
        let body_left = if dialog.compact { heading_left } else { padding };
        let body_width = width - body_left - padding;
        let dc = GetDC(hwnd);
        let old = SelectObject(dc, dialog.heading_font.get());
        let mut metrics = TEXTMETRICW::default();
        GetTextMetricsW(dc, &raw mut metrics);
        // Keep the glyph centered on the first heading row in either dialog layout.
        dialog.heading_row_height.set(metrics.tmHeight.max(dialog.px(HEADING_ROW_HEIGHT)));
        SelectObject(dc, dialog.font.get());
        GetTextMetricsW(dc, &raw mut metrics);
        // Labels may be considerably wider in translated locales.
        let gap = dialog.dip(style::BUTTON_GAP);
        let button_count = if dialog.confirmation && !dialog.compact { 3 } else { 2 };
        let button_width = dialog
            .labels
            .iter()
            .enumerate()
            .filter(|(index, _)| (*index != 1 || dialog.confirmation) && (*index != 2 || !dialog.compact))
            .map(|(_, label)| {
                let mut rect = RECT::default();
                DrawTextW(
                    dc,
                    wide(label).as_ptr(),
                    -1,
                    &raw mut rect,
                    DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
                );
                (rect.right + dialog.dip(style::BUTTON_PADDING * 2.0)).max(dialog.px(96))
            })
            .max()
            .unwrap_or(dialog.px(96))
            .min((width - padding * 2 - gap * (button_count - 1)) / button_count);
        SelectObject(dc, old);
        ReleaseDC(hwnd, dc);
        let heading_height = dialog.heading_row_height.get()
            .max(dialog.px(if dialog.compact { HEADING_ROW_HEIGHT } else { 54 }));
        let body_top = vertical_padding + heading_height + dialog.px(if dialog.compact { 4 } else { 0 });
        let body_limit = ((work.bottom - work.top) * 2 / 3 - dialog.px(180)).max(dialog.px(52));
        #[cfg(test)]
        let body_limit = tests::PREVIEW_BODY_LIMIT.with(|limit| {
            limit
                .get()
                .map_or(body_limit, |dip| body_limit.min(dialog.px(dip)))
        });
        // Measure the edit's own wrapped lines with the full text width before reserving a bar.
        let edit = GetDlgItem(hwnd, BODY);
        ShowScrollBar(edit, SB_VERT, 0);
        MoveWindow(edit, body_left, body_top, body_width, body_limit, 0);
        let line_height = metrics.tmHeight.max(dialog.dip(style::TEXT));
        let lines = SendMessageW(edit, EM_GETLINECOUNT, 0, 0) as i32;
        let text_height = lines.saturating_mul(line_height).saturating_add(dialog.px(4));
        let body_height = text_height.max(dialog.px(if dialog.compact { 20 } else { 52 })).min(body_limit);
        let scrolls = text_height > body_limit;
        let control_height = dialog.dip(style::HEIGHT);
        let button_gap = dialog.px(if dialog.compact { 16 } else { 28 });
        let bottom = body_top + body_height + button_gap;
        let height = bottom + control_height + vertical_padding;
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        AdjustWindowRectExForDpi(
            &raw mut frame,
            GetWindowLongW(hwnd, GWL_STYLE) as u32,
            0,
            GetWindowLongW(hwnd, GWL_EXSTYLE) as u32,
            dpi,
        );
        let outer_width = frame.right - frame.left;
        let outer_height = frame.bottom - frame.top;
        let mut anchor = work;
        if !owner.is_null() && IsIconic(owner) == 0 {
            GetWindowRect(owner, &raw mut anchor);
        }
        let x = ((anchor.left + anchor.right - outer_width) / 2)
            .clamp(work.left, (work.right - outer_width).max(work.left));
        let y = ((anchor.top + anchor.bottom - outer_height) / 2)
            .clamp(work.top, (work.bottom - outer_height).max(work.top));
        SetWindowPos(
            hwnd,
            null_mut(),
            x,
            y,
            outer_width,
            outer_height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        MoveWindow(
            GetDlgItem(hwnd, HEADING),
            heading_left,
            vertical_padding,
            width - heading_left - padding,
            heading_height,
            1,
        );
        MoveWindow(edit, body_left, body_top, body_width, body_height, 1);
        let bar = GetDlgItem(hwnd, BODY_SCROLLBAR);
        if scrolls {
            ShowScrollBar(edit, SB_VERT, 1);
            gdi::position_edit_scrollbar(hwnd, edit, bar);
        } else {
            ShowWindow(bar, SW_HIDE);
        }
        if !dialog.compact {
            MoveWindow(GetDlgItem(hwnd, COPY), padding, bottom, button_width, control_height, 1);
        }
        MoveWindow(
            GetDlgItem(hwnd, IDOK),
            width - padding - button_width,
            bottom,
            button_width,
            control_height,
            1,
        );
        if dialog.confirmation {
            MoveWindow(
                GetDlgItem(hwnd, IDCANCEL),
                width - padding - 2 * button_width - gap,
                bottom,
                button_width,
                control_height,
                1,
            );
        }
        InvalidateRect(hwnd, null(), 1);
    }
}

unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    unsafe {
        if msg == WM_INITDIALOG {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, lp);
        }
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Dialog;
        if pointer.is_null() {
            return 0;
        }
        let dialog = &*pointer;
        match msg {
            WM_INITDIALOG => {
                SetWindowTextW(hwnd, windows_sys::w!("LucidDesk"));
                let dark = i32::from(dialog.dark);
                let corner = 2i32;
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                    (&raw const dark).cast(),
                    4,
                );
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                    (&raw const corner).cast(),
                    4,
                );
                let color = dialog.background_color();
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_CAPTION_COLOR as u32,
                    (&raw const color).cast(),
                    4,
                );
                let ok = create(
                    hwnd,
                    HEADING,
                    windows_sys::w!("STATIC"),
                    dialog.title.as_ptr(),
                    0,
                ) && create(
                    hwnd,
                    BODY,
                    windows_sys::w!("EDIT"),
                    dialog.body.as_ptr(),
                    WS_TABSTOP
                        | WS_VSCROLL
                        | WS_CLIPSIBLINGS
                        | ES_MULTILINE as u32
                        | ES_READONLY as u32
                        | ES_AUTOVSCROLL as u32,
                ) && (dialog.compact || create(
                    hwnd,
                    COPY,
                    windows_sys::w!("BUTTON"),
                    wide(&dialog.labels[2]).as_ptr(),
                    WS_TABSTOP | BS_OWNERDRAW as u32,
                )) && (!dialog.confirmation
                    || create(
                        hwnd,
                        IDCANCEL,
                        windows_sys::w!("BUTTON"),
                        wide(&dialog.labels[1]).as_ptr(),
                        WS_TABSTOP | BS_OWNERDRAW as u32,
                    ))
                    && create(
                        hwnd,
                        IDOK,
                        windows_sys::w!("BUTTON"),
                        wide(&dialog.labels[0]).as_ptr(),
                        WS_TABSTOP | BS_OWNERDRAW as u32,
                    );
                if !ok {
                    EndDialog(hwnd, -1);
                    return 1;
                }
                gdi::add_edit_scrollbar(hwnd, GetDlgItem(hwnd, BODY), BODY_SCROLLBAR, dialog.dark);
                let owner = GetWindow(hwnd, GW_OWNER);
                layout(
                    hwnd,
                    dialog,
                    GetDpiForWindow(if owner.is_null() { hwnd } else { owner }),
                );
                SendMessageW(
                    hwnd,
                    DM_SETDEFID,
                    if dialog.confirmation { IDCANCEL } else { IDOK } as usize,
                    0,
                );
                SetFocus(GetDlgItem(
                    hwnd,
                    if dialog.confirmation { IDCANCEL } else { IDOK },
                ));
                0
            }
            WM_COMMAND => {
                match (wp & 0xffff) as i32 {
                    BODY if matches!((wp >> 16) as u32, EN_VSCROLL | EN_UPDATE) => {
                        invalidate_scrollbar(hwnd);
                    }
                    IDOK => {
                        EndDialog(hwnd, IDOK as isize);
                    }
                    IDCANCEL => {
                        EndDialog(hwnd, IDCANCEL as isize);
                    }
                    COPY => clipboard(hwnd, &dialog.copy_text),
                    _ => return 0,
                }
                1
            }
            WM_CLOSE => {
                EndDialog(hwnd, IDCANCEL as isize);
                1
            }
            WM_DPICHANGED => {
                layout(hwnd, dialog, (wp & 0xffff) as u32);
                1
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT => {
                SetTextColor(wp as HDC, colorref(Palette::for_theme(dialog.dark).ink));
                SetBkColor(wp as HDC, dialog.background_color());
                dialog.background as isize
            }
            WM_ERASEBKGND => {
                let mut rect = RECT::default();
                GetClientRect(hwnd, &raw mut rect);
                FillRect(wp as HDC, &rect, dialog.background);
                1
            }
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &raw mut paint);
                let palette = Palette::for_theme(dialog.dark);
                let canvas = GdiCanvas {
                    dc,
                    scale: dialog.scale.get(),
                    palette: &palette,
                };
                let (glyph, rgb) = dialog.heading_icon();
                let left = dialog.horizontal_padding();
                let top = dialog.vertical_padding();
                let bounds = RECT {
                    left,
                    top,
                    right: left + dialog.px(HEADING_ICON_SIZE),
                    bottom: top + dialog.heading_row_height.get(),
                };
                canvas.icons(HEADING_ICON_SIZE as f32, [(bounds, glyph, rgb)]);
                EndPaint(hwnd, &paint);
                1
            }
            WM_DRAWITEM => {
                let item = &*(lp as *const DRAWITEMSTRUCT);
                let (kind, index) = match item.CtlID as i32 {
                    IDOK => (ButtonKind::Primary, 0),
                    IDCANCEL => (ButtonKind::Secondary, 1),
                    COPY => (ButtonKind::Secondary, 2),
                    _ => return 0,
                };
                let mut point = POINT::default();
                GetCursorPos(&raw mut point);
                ScreenToClient(item.hwndItem, &raw mut point);
                let enabled = item.itemState & ODS_DISABLED == 0;
                let hovered = enabled && PtInRect(&item.rcItem, point) != 0;
                let pressed = enabled && item.itemState & ODS_SELECTED != 0;
                let hover = gdi::pointer_feedback(item.hwndItem, &mut dialog.button_motion.borrow_mut()[index], hovered, pressed);
                let state = State {
                    enabled,
                    active: hovered || pressed || hover > 0.0,
                    focused: item.itemState & ODS_FOCUS != 0,
                };
                FillRect(item.hDC, &item.rcItem, dialog.background);
                let font = SelectObject(item.hDC, dialog.font.get());
                let palette = Palette::for_theme(dialog.dark);
                let mut canvas = GdiCanvas {
                    dc: item.hDC,
                    scale: dialog.scale.get(),
                    palette: &palette,
                };
                let bounds = canvas.bounds(item.rcItem);
                kind.paint_with_hover(&mut canvas, bounds, &dialog.labels[index], state, hover);
                SelectObject(item.hDC, font);
                1
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                0
            }
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    thread_local! {
        static OBSERVED: Cell<bool> = const { Cell::new(false) };
        static ACTION: Cell<u32> = const { Cell::new(0) };
        static DEFAULT_CANCEL: Cell<bool> = const { Cell::new(false) };
        static BODY_PRESENT: Cell<bool> = const { Cell::new(false) };
        static BUTTON_LAYOUT: Cell<bool> = const { Cell::new(false) };
        static SCROLLBAR_WORKS: Cell<bool> = const { Cell::new(false) };
        static ARROWS_MATCH_SCROLL_POSITION: Cell<bool> = const { Cell::new(false) };
        static FIT: Cell<Option<(bool, bool, bool, i32, i32)>> = const { Cell::new(None) };
        static COMPACT_ALIGNMENT: Cell<bool> = const { Cell::new(false) };
        pub(super) static PREVIEW_BODY_LIMIT: Cell<Option<i32>> = const { Cell::new(None) };
    }
    unsafe fn arrow_contrast(scrollbar: HWND, arrow_height: i32, background: u32) -> [u32; 2] {
        unsafe {
            let mut bounds = RECT::default();
            GetClientRect(scrollbar, &raw mut bounds);
            let window_dc = GetDC(scrollbar);
            let dc = CreateCompatibleDC(window_dc);
            let bitmap = CreateCompatibleBitmap(window_dc, bounds.right, bounds.bottom);
            ReleaseDC(scrollbar, window_dc);
            let old = SelectObject(dc, bitmap);
            SendMessageW(scrollbar, WM_PRINTCLIENT, dc as usize, PRF_CLIENT as isize);
            let background = colorref(background);
            // Font rasterization blends arrow edges with the background.
            let result = [0, bounds.bottom - arrow_height].map(|top| {
                let mut contrast = 0_u32;
                for y in top..top + arrow_height {
                    for x in 0..bounds.right {
                        let pixel = GetPixel(dc, x, y);
                        let difference = [0, 8, 16]
                            .into_iter()
                            .map(|shift| {
                                ((pixel >> shift) & 255).abs_diff((background >> shift) & 255)
                            })
                            .sum();
                        contrast = contrast.max(difference);
                    }
                }
                contrast
            });
            SelectObject(dc, old);
            DeleteObject(bitmap);
            DeleteDC(dc);
            result
        }
    }
    unsafe extern "system" fn inspect(_: HWND, _: u32, timer: usize, _: u32) {
        unsafe {
            let hwnd = GetActiveWindow();
            if GetDlgItem(hwnd, BODY).is_null() {
                return;
            }
            KillTimer(null_mut(), timer);
            DEFAULT_CANCEL.with(|value| value.set(GetFocus() == GetDlgItem(hwnd, IDCANCEL)));
            let bounds = |id| {
                let mut rect = RECT::default();
                GetWindowRect(GetDlgItem(hwnd, id), &raw mut rect);
                rect
            };
            let (copy, cancel, confirm) = (bounds(COPY), bounds(IDCANCEL), bounds(IDOK));
            let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
            BUTTON_LAYOUT.with(|value| {
                value.set(
                    copy.right <= cancel.left
                        && cancel.right <= confirm.left
                        && GetNextDlgTabItem(hwnd, GetDlgItem(hwnd, IDCANCEL), 0)
                            == GetDlgItem(hwnd, IDOK)
                        && [copy, cancel, confirm].iter().all(|rect| {
                            rect.top == copy.top
                                && rect.bottom - rect.top == (style::HEIGHT * scale).round() as i32
                        }),
                )
            });
            BODY_PRESENT
                .with(|value| value.set(GetWindowTextLengthW(GetDlgItem(hwnd, BODY)) > 1000));
            let edit = GetDlgItem(hwnd, BODY);
            let scrollbar = GetDlgItem(hwnd, BODY_SCROLLBAR);
            let bar = bounds(BODY_SCROLLBAR);
            let mut info = SCROLLBARINFO {
                cbSize: size_of::<SCROLLBARINFO>() as u32,
                ..Default::default()
            };
            let native_bar = GetScrollBarInfo(edit, OBJID_VSCROLL, &raw mut info) != 0;
            let arrow_height = info.dxyLineButton;
            let hit = |target, y| {
                let packed =
                    ((y as u16 as usize) << 16) | ((bar.left + bar.right) / 2) as u16 as usize;
                SendMessageW(target, WM_NCHITTEST, 0, packed as isize)
            };
            let arrows_hit = [bar.top + arrow_height / 2, bar.bottom - arrow_height / 2]
                .into_iter()
                .all(|y| {
                    hit(scrollbar, y) == HTTRANSPARENT as isize
                        && hit(edit, y) == HTVSCROLL as isize
                });
            let first_line = || SendMessageW(edit, EM_GETFIRSTVISIBLELINE, 0, 0);
            let palette = Palette::for_theme(message_is_dark());
            let contrast = || arrow_contrast(scrollbar, arrow_height, palette.page_background);
            let [up_at_top, down_at_top] = contrast();
            SendMessageW(edit, WM_VSCROLL, SB_LINEDOWN as usize, 0);
            let down_one = first_line() == 1;
            let [up_in_middle, down_in_middle] = contrast();
            SendMessageW(edit, WM_VSCROLL, SB_LINEUP as usize, 0);
            let up_one = first_line() == 0;
            SendMessageW(edit, WM_VSCROLL, SB_BOTTOM as usize, 0);
            let [up_at_bottom, down_at_bottom] = contrast();
            SendMessageW(edit, WM_VSCROLL, SB_TOP as usize, 0);
            ARROWS_MATCH_SCROLL_POSITION.with(|value| {
                value.set(
                    up_at_top > 0
                        && up_at_top < up_in_middle.min(up_at_bottom)
                        && down_at_bottom > 0
                        && down_at_bottom < down_in_middle.min(down_at_top),
                )
            });
            let before = first_line();
            SendMessageW(edit, WM_VSCROLL, SB_PAGEDOWN as usize, 0);
            let moved = first_line() > before;
            SendMessageW(edit, WM_VSCROLL, SB_TOP as usize, 0);
            SCROLLBAR_WORKS.with(|value| {
                value.set(
                    native_bar
                        && bar.right > bar.left
                        && IsWindowVisible(scrollbar) != 0
                        && arrows_hit
                        && down_one
                        && up_one
                        && moved
                        && first_line() == 0,
                )
            });
            OBSERVED.with(|value| value.set(true));
            match ACTION.with(Cell::get) {
                1 => {
                    PostMessageW(hwnd, WM_COMMAND, IDOK as usize, 0);
                }
                2 => {
                    PostMessageW(GetFocus(), WM_KEYDOWN, VK_RETURN as usize, 0);
                }
                3 => {
                    SetFocus(GetDlgItem(hwnd, BODY));
                    PostMessageW(GetFocus(), WM_KEYDOWN, VK_RETURN as usize, 0);
                }
                _ => {
                    PostMessageW(hwnd, WM_KEYDOWN, VK_ESCAPE as usize, 0);
                }
            }
        }
    }
    #[test]
    #[ignore = "Interactive modal keyboard test; run alone on a Windows desktop"]
    fn confirmation_defaults_to_cancel_and_preserves_long_text() {
        let prior_theme = THEME.load(Ordering::Relaxed);
        for action in 0..4 {
            set_message_theme(Some(action % 2 != 0));
            ACTION.with(|value| value.set(action));
            OBSERVED.with(|value| value.set(false));
            unsafe {
                assert_ne!(SetTimer(null_mut(), 0, 100, Some(inspect)), 0);
            }
            let text = "Long message C:\\文件\\workspace.db\r\n".repeat(100);
            assert_eq!(
                super::super::confirm_message(0, "Dialog regression", &text, MessageKind::Warning),
                action == 1
            );
            assert!(OBSERVED.with(Cell::get));
            assert!(DEFAULT_CANCEL.with(Cell::get));
            assert!(BODY_PRESENT.with(Cell::get));
            assert!(BUTTON_LAYOUT.with(Cell::get));
            assert!(SCROLLBAR_WORKS.with(Cell::get));
            assert!(ARROWS_MATCH_SCROLL_POSITION.with(Cell::get));
        }
        THEME.store(prior_theme, Ordering::Relaxed);
    }

    unsafe extern "system" fn inspect_fit(_: HWND, _: u32, timer: usize, _: u32) {
        unsafe {
            let hwnd = GetActiveWindow();
            let edit = GetDlgItem(hwnd, BODY);
            if edit.is_null() {
                return;
            }
            KillTimer(null_mut(), timer);
            let mut bounds = RECT::default();
            GetClientRect(hwnd, &raw mut bounds);
            let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
            let copy = !GetDlgItem(hwnd, COPY).is_null();
            let native_bar = GetWindowLongW(edit, GWL_STYLE) as u32 & WS_VSCROLL != 0;
            let themed_bar = IsWindowVisible(GetDlgItem(hwnd, BODY_SCROLLBAR)) != 0;
            let height = (bounds.bottom as f32 / scale).round() as i32;
            let lines = SendMessageW(edit, EM_GETLINECOUNT, 0, 0) as i32;
            FIT.with(|value| value.set(Some((copy, native_bar, themed_bar, height, lines))));
            let screen_bounds = |child| {
                let mut rect = RECT::default();
                GetWindowRect(child, &raw mut rect);
                rect
            };
            let heading = screen_bounds(GetDlgItem(hwnd, HEADING));
            let body = screen_bounds(edit);
            let confirm = screen_bounds(GetDlgItem(hwnd, IDOK));
            COMPACT_ALIGNMENT.with(|value| value.set(
                heading.left == body.left && heading.right == body.right
                    && body.right == confirm.right
                    && body.top - heading.bottom == (4.0 * scale).round() as i32
                    && confirm.top - body.bottom == (16.0 * scale).round() as i32,
            ));
            PostMessageW(hwnd, WM_COMMAND, IDCANCEL as usize, 0);
        }
    }

    #[test]
    #[ignore = "Interactive native layout test; run alone on a Windows desktop"]
    fn actions_fit_short_and_wrapped_text_and_scroll_only_on_overflow() {
        let prior_theme = THEME.load(Ordering::Relaxed);
        let short = "删除第 1 条规则（目标分组「开发」）？";
        let wrapped = format!("Delete the rule for “{}”?", "Desktop workspace ".repeat(8).trim());
        let long = "Long message C:\\文件\\workspace.db\r\n".repeat(100);
        let inspect = |body: &str, compact| {
            FIT.with(|value| value.set(None));
            unsafe { assert_ne!(SetTimer(null_mut(), 0, 100, Some(inspect_fit)), 0); }
            let result = if compact {
                super::super::confirm_action(0, "删除规则", body, MessageKind::Warning, "删除")
            } else {
                super::super::confirm_message(0, "删除规则", body, MessageKind::Warning)
            };
            assert!(!result);
            if compact {
                assert!(COMPACT_ALIGNMENT.with(Cell::get));
            }
            FIT.with(Cell::get).expect("dialog must open")
        };
        for dark in [false, true] {
            set_message_theme(Some(dark));
            let (copy, native, themed, standard_height, _) = inspect(short, false);
            assert!(copy && !native && !themed);
            let (copy, native, themed, compact_height, lines) = inspect(short, true);
            assert!(!copy && !native && !themed);
            assert_eq!(lines, 1);
            assert!(compact_height + 40 < standard_height);
            let (copy, native, themed, wrapped_height, lines) = inspect(&wrapped, true);
            assert!(!copy && !native && !themed);
            assert!(lines > 1 && wrapped_height > compact_height);
            let (copy, native, themed, height, _) = inspect(&long, true);
            assert!(!copy && native && themed && height > wrapped_height);
        }
        THEME.store(prior_theme, Ordering::Relaxed);
    }

    #[test]
    #[ignore = "Manual visual preview; dismiss the dialog to finish"]
    fn visual_preview() {
        set_message_labels("安装更新", "取消", "复制详情");
        set_message_theme(Some(
            std::env::var("LUCIDDESK_DIALOG_THEME").as_deref() != Ok("light"),
        ));
        let body = "已验证新版本。现在退出 LucidDesk、安装更新并重新打开？\n\n你的配置、面板布局和个人文件会保留。";
        let long_sample = std::env::var_os("LUCIDDESK_DIALOG_LONG_SAMPLE").is_some();
        PREVIEW_BODY_LIMIT.with(|limit| limit.set(long_sample.then_some(280)));
        let details = if long_sample {
            format!(
                "{body}\n\n安装检查详情\n{}",
                (1..=30)
                    .map(|index| format!("已验证第 {index} 项更新文件\n"))
                    .collect::<String>()
            )
        } else {
            body.to_owned()
        };
        super::super::confirm_message(0, "准备安装更新", &details, MessageKind::Information);
        PREVIEW_BODY_LIMIT.with(|limit| limit.set(None));
    }
}
