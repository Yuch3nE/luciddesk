//! Shared, themed dialog used by the desktop client and the standalone updater.
//! Native controls retain keyboard navigation, selection and accessibility names.
use super::MessageKind;
use std::{
    cell::{Cell, RefCell},
    mem::size_of,
    ptr::{null, null_mut},
    sync::atomic::{AtomicI8, Ordering},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    System::{DataExchange::*, Memory::*, Registry::*},
    UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

const COPY: i32 = 100;
const HEADING: i32 = 101;
const BODY: i32 = 102;
static THEME: AtomicI8 = AtomicI8::new(-1);
thread_local! {
    static LABELS: RefCell<[String; 3]> = RefCell::new(["OK".into(), "Cancel".into(), "Copy".into()]);
}
pub fn set_message_labels(ok: &str, cancel: &str, copy: &str) {
    LABELS.with(|labels| *labels.borrow_mut() = [ok.into(), cancel.into(), copy.into()]);
}
pub fn set_message_theme(dark: Option<bool>) {
    THEME.store(dark.map_or(-1, i8::from), Ordering::Relaxed);
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
fn rgb(r: u32, g: u32, b: u32) -> u32 {
    r | g << 8 | b << 16
}

struct Dialog {
    title: Vec<u16>,
    body: Vec<u16>,
    copy_text: Vec<u16>,
    labels: [String; 3],
    confirmation: bool,
    dark: bool,
    kind: MessageKind,
    background: HBRUSH,
    font: Cell<HFONT>,
    heading_font: Cell<HFONT>,
    scale: Cell<f32>,
}
impl Dialog {
    fn background_color(&self) -> u32 {
        if self.dark {
            rgb(32, 32, 32)
        } else {
            rgb(249, 249, 249)
        }
    }
    fn text_color(&self) -> u32 {
        if self.dark {
            rgb(245, 245, 245)
        } else {
            rgb(27, 27, 27)
        }
    }
    fn accent(&self) -> u32 {
        match self.kind {
            MessageKind::Error => {
                if self.dark {
                    rgb(255, 153, 164)
                } else {
                    rgb(170, 38, 50)
                }
            }
            MessageKind::Warning => {
                if self.dark {
                    rgb(244, 196, 104)
                } else {
                    rgb(141, 92, 0)
                }
            }
            MessageKind::Information => {
                if self.dark {
                    rgb(118, 185, 224)
                } else {
                    rgb(0, 104, 153)
                }
            }
        }
    }
    fn px(&self, value: i32) -> i32 {
        (value as f32 * self.scale.get()).round() as i32
    }
}
impl Drop for Dialog {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.background);
            DeleteObject(self.font.get());
            DeleteObject(self.heading_font.get());
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
    confirmation: bool,
) -> bool {
    let mut dialog = Dialog {
        title: wide(title),
        body: wide(body),
        copy_text: wide(&format!("{title}\r\n\r\n{body}")),
        labels: LABELS.with(|labels| labels.borrow().clone()),
        confirmation,
        dark: message_is_dark(),
        kind,
        background: null_mut(),
        font: Cell::new(null_mut()),
        heading_font: Cell::new(null_mut()),
        scale: Cell::new(1.0),
    };
    dialog.background = unsafe { CreateSolidBrush(dialog.background_color()) };
    let template = Template {
        header: DLGTEMPLATE {
            style: WS_POPUP | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME as u32,
            dwExtendedStyle: WS_EX_DLGMODALFRAME,
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
    unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
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
        dialog.scale.set(dpi.max(96) as f32 / 96.0);
        let font = |height, weight| {
            CreateFontW(
                -dialog.px(height),
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
                windows_sys::w!("Segoe UI"),
            )
        };
        let old_font = dialog.font.replace(font(14, FW_NORMAL as i32));
        let old_heading = dialog.heading_font.replace(font(20, FW_SEMIBOLD as i32));
        for id in [IDOK, IDCANCEL, COPY, BODY, HEADING] {
            SendMessageW(
                GetDlgItem(hwnd, id),
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
            .px(520)
            .min((work.right - work.left - dialog.px(32)).max(dialog.px(240)));
        let padding = dialog.px(24);
        let dc = GetDC(hwnd);
        let old = SelectObject(dc, dialog.font.get());
        let mut bounds = RECT {
            left: 0,
            top: 0,
            right: width - padding * 2,
            bottom: 0,
        };
        DrawTextW(
            dc,
            dialog.body.as_ptr(),
            -1,
            &raw mut bounds,
            DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
        );
        // Labels may be considerably wider in translated locales.
        let button_width = dialog
            .labels
            .iter()
            .map(|label| {
                let mut rect = RECT::default();
                DrawTextW(
                    dc,
                    wide(label).as_ptr(),
                    -1,
                    &raw mut rect,
                    DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
                );
                (rect.right + dialog.px(32)).max(dialog.px(92))
            })
            .max()
            .unwrap_or(dialog.px(92))
            .min((width - padding * 2 - dialog.px(24)) / 3);
        SelectObject(dc, old);
        ReleaseDC(hwnd, dc);
        let body_height = bounds
            .bottom
            .max(dialog.px(52))
            .min(((work.bottom - work.top) * 2 / 3 - dialog.px(180)).max(dialog.px(52)));
        let heading_height = dialog.px(54);
        let height = padding + heading_height + body_height + dialog.px(84);
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
            padding + dialog.px(12),
            padding,
            width - padding * 2 - dialog.px(12),
            heading_height,
            1,
        );
        MoveWindow(
            GetDlgItem(hwnd, BODY),
            padding,
            padding + heading_height,
            width - padding * 2,
            body_height,
            1,
        );
        let bottom = height - dialog.px(56);
        let gap = dialog.px(12);
        MoveWindow(
            GetDlgItem(hwnd, COPY),
            padding,
            bottom,
            button_width,
            dialog.px(34),
            1,
        );
        MoveWindow(
            GetDlgItem(hwnd, IDOK),
            width
                - padding
                - button_width * if dialog.confirmation { 2 } else { 1 }
                - if dialog.confirmation { gap } else { 0 },
            bottom,
            button_width,
            dialog.px(34),
            1,
        );
        if dialog.confirmation {
            MoveWindow(
                GetDlgItem(hwnd, IDCANCEL),
                width - padding - button_width,
                bottom,
                button_width,
                dialog.px(34),
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
                        | ES_MULTILINE as u32
                        | ES_READONLY as u32
                        | ES_AUTOVSCROLL as u32,
                ) && create(
                    hwnd,
                    COPY,
                    windows_sys::w!("BUTTON"),
                    wide(&dialog.labels[2]).as_ptr(),
                    WS_TABSTOP | BS_OWNERDRAW as u32,
                ) && create(
                    hwnd,
                    IDOK,
                    windows_sys::w!("BUTTON"),
                    wide(&dialog.labels[0]).as_ptr(),
                    WS_TABSTOP | BS_OWNERDRAW as u32,
                ) && (!dialog.confirmation
                    || create(
                        hwnd,
                        IDCANCEL,
                        windows_sys::w!("BUTTON"),
                        wide(&dialog.labels[1]).as_ptr(),
                        WS_TABSTOP | BS_OWNERDRAW as u32,
                    ));
                if !ok {
                    EndDialog(hwnd, -1);
                    return 1;
                }
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
                SetTextColor(wp as HDC, dialog.text_color());
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
                let brush = CreateSolidBrush(dialog.accent());
                let rect = RECT {
                    left: dialog.px(24),
                    top: dialog.px(26),
                    right: dialog.px(28),
                    bottom: dialog.px(48),
                };
                FillRect(dc, &rect, brush);
                DeleteObject(brush);
                EndPaint(hwnd, &paint);
                1
            }
            WM_DRAWITEM => {
                let item = &*(lp as *const DRAWITEMSTRUCT);
                let primary = item.CtlID == IDOK as u32;
                let pressed = item.itemState & ODS_SELECTED != 0;
                let mut point = POINT::default();
                GetCursorPos(&raw mut point);
                ScreenToClient(item.hwndItem, &raw mut point);
                let hover = PtInRect(&item.rcItem, point) != 0;
                let color = if primary {
                    dialog.accent()
                } else if dialog.dark {
                    if hover || pressed {
                        rgb(60, 60, 60)
                    } else {
                        rgb(48, 48, 48)
                    }
                } else if hover || pressed {
                    rgb(240, 240, 240)
                } else {
                    rgb(255, 255, 255)
                };
                let brush = CreateSolidBrush(color);
                let pen = CreatePen(
                    PS_SOLID,
                    1,
                    if dialog.dark {
                        rgb(80, 80, 80)
                    } else {
                        rgb(205, 205, 205)
                    },
                );
                let old_brush = SelectObject(item.hDC, brush);
                let old_pen = SelectObject(item.hDC, pen);
                let rect = item.rcItem;
                RoundRect(
                    item.hDC,
                    rect.left,
                    rect.top,
                    rect.right,
                    rect.bottom,
                    dialog.px(8),
                    dialog.px(8),
                );
                SelectObject(item.hDC, old_pen);
                SelectObject(item.hDC, old_brush);
                DeleteObject(pen);
                DeleteObject(brush);
                SetBkMode(item.hDC, TRANSPARENT as i32);
                SetTextColor(
                    item.hDC,
                    if primary {
                        if dialog.dark {
                            rgb(12, 22, 28)
                        } else {
                            rgb(255, 255, 255)
                        }
                    } else {
                        dialog.text_color()
                    },
                );
                let font = SelectObject(item.hDC, dialog.font.get());
                let label = match item.CtlID as i32 {
                    IDOK => &dialog.labels[0],
                    IDCANCEL => &dialog.labels[1],
                    _ => &dialog.labels[2],
                };
                let mut text_rect = rect;
                if pressed {
                    OffsetRect(&raw mut text_rect, 1, 1);
                }
                DrawTextW(
                    item.hDC,
                    wide(label).as_ptr(),
                    -1,
                    &raw mut text_rect,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
                SelectObject(item.hDC, font);
                if item.itemState & ODS_FOCUS != 0 {
                    let mut focus = rect;
                    InflateRect(&raw mut focus, -dialog.px(4), -dialog.px(4));
                    DrawFocusRect(item.hDC, &focus);
                }
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
    }
    unsafe extern "system" fn inspect(_: HWND, _: u32, timer: usize, _: u32) {
        unsafe {
            let hwnd = GetActiveWindow();
            if GetDlgItem(hwnd, BODY).is_null() {
                return;
            }
            KillTimer(null_mut(), timer);
            DEFAULT_CANCEL.with(|value| value.set(GetFocus() == GetDlgItem(hwnd, IDCANCEL)));
            BODY_PRESENT
                .with(|value| value.set(GetWindowTextLengthW(GetDlgItem(hwnd, BODY)) > 1000));
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
        for action in 0..4 {
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
        }
    }
    #[test]
    #[ignore = "Manual visual preview; dismiss the dialog to finish"]
    fn visual_preview() {
        set_message_labels("安装更新", "取消", "复制详情");
        set_message_theme(Some(
            std::env::var("LUCIDDESK_DIALOG_THEME").as_deref() != Ok("light"),
        ));
        super::super::confirm_message(
            0,
            "准备安装更新",
            "已验证新版本。现在退出 LucidDesk、安装更新并重新打开？\n\n你的配置、面板布局和个人文件会保留。",
            MessageKind::Information,
        );
    }
}
