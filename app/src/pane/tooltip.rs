//! Shared native hover text; callers supply a client rectangle in physical pixels.
use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    UI::{
        Controls::{
            TOOLTIPS_CLASSW, TTF_SUBCLASS, TTM_ACTIVATE, TTM_ADDTOOLW, TTM_NEWTOOLRECTW, TTM_POP,
            TTM_SETMAXTIPWIDTH, TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW,
        },
        HiDpi::GetDpiForWindow,
        WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, SendMessageW, WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_POPUP,
        },
    },
};

pub(super) struct Tooltip {
    hwnd: HWND,
    tool: TTTOOLINFOW,
    text: Vec<u16>,
}
impl Tooltip {
    pub(super) fn new(owner: HWND) -> Option<Self> {
        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                TOOLTIPS_CLASSW,
                std::ptr::null(),
                WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
                0,
                0,
                0,
                0,
                owner,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return None;
            }
            let mut text = vec![0u16];
            let mut tool = TTTOOLINFOW {
                cbSize: size_of::<TTTOOLINFOW>() as u32,
                uFlags: TTF_SUBCLASS,
                hwnd: owner,
                uId: 1,
                lpszText: text.as_mut_ptr(),
                ..Default::default()
            };
            SendMessageW(hwnd, TTM_ADDTOOLW, 0, (&raw mut tool) as isize);
            SendMessageW(
                hwnd,
                TTM_SETMAXTIPWIDTH,
                0,
                (560.0 * GetDpiForWindow(owner).max(96) as f32 / 96.0) as isize,
            );
            Some(Self { hwnd, tool, text })
        }
    }
    pub(super) fn hide(&mut self) {
        unsafe {
            SendMessageW(self.hwnd, TTM_POP, 0, 0);
            SendMessageW(self.hwnd, TTM_ACTIVATE, 0, 0);
        }
    }
    pub(super) fn show(&mut self, text: &str, rect: RECT) {
        self.hide();
        self.text.clear();
        self.text.extend(text.encode_utf16().chain(Some(0)));
        self.tool.lpszText = self.text.as_mut_ptr();
        self.tool.rect = rect;
        unsafe {
            SendMessageW(
                self.hwnd,
                TTM_NEWTOOLRECTW,
                0,
                (&raw mut self.tool) as isize,
            );
            SendMessageW(
                self.hwnd,
                TTM_UPDATETIPTEXTW,
                0,
                (&raw mut self.tool) as isize,
            );
            SendMessageW(self.hwnd, TTM_ACTIVATE, 1, 0);
        }
    }
}
impl Drop for Tooltip {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.hwnd);
        }
    }
}
