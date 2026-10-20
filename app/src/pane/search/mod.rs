//! Compact search pane using the same composition backdrop as icon panes.
#![allow(clippy::wildcard_imports, clippy::too_many_lines)]
mod drawing;
pub(super) mod control;
mod tooltip;
use drawing::Drawing;
mod everything;
pub(super) mod everything_settings;
pub(super) mod hotkey;

use super::{Event, GroupModel};
use luciddesk_core::{RectDip, ShellIdentity};
use everything::{Entry, Page};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    UI::{
        Controls::*,
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::*,
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
const OPEN: usize = 110;
const LOCATION: usize = 111;
const COPY: usize = 112;
const CUT: usize = 113;
const DELETE: usize = 114;
const PEEK: usize = 115;
const REFRESH: usize = 107;

pub(super) fn refresh(hwnd: HWND) {
    unsafe { PostMessageW(hwnd, WM_COMMAND, REFRESH, 0); }
}

pub(super) const INPUT: u32 = WM_APP + 119;
const NAVIGATE: u32 = WM_APP + 120;
pub(super) const FOCUS_INPUT: u32 = WM_APP + 130;
pub(super) const RESTORE_LAYOUT: u32 = WM_APP + 129;
pub(super) const CLEAR_SELECTION: u32 = WM_APP + 118;
const POLL: usize = 38;
const TOP: f32 = 56.0;
const ROW: f32 = 48.0;
const FOOTER: f32 = 32.0;
const ROW_INSET: f32 = 4.0;
const VISIBLE: usize = 8;
const EDIT_PROPERTY: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.SearchInput");
#[cfg(test)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn edit(hwnd: HWND) -> HWND {
    unsafe { GetPropW(hwnd, EDIT_PROPERTY) }
}
fn text(hwnd: HWND) -> String {
    let mut buf = vec![0; unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize + 1];
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}
fn invalidate(hwnd: HWND) {
    unsafe {
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}
fn scale(hwnd: HWND) -> f32 {
    unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0
}
fn identity(path: std::path::PathBuf) -> ShellIdentity {
    ShellIdentity::FileSystem {
        path,
        volume_id: None,
        file_id: None,
    }
}

fn same_click_target(last: Option<&(std::path::PathBuf, Instant)>, path: &std::path::Path, interval: Duration) -> bool {
    last.is_some_and(|(previous, time)| previous == path && time.elapsed() < interval)
}

struct Request {
    generation: u64,
    query: String,
    offset: u32,
}
struct Search {
    wake: super::wake::Wake,
    sender: mpsc::Sender<Request>,
    receiver: mpsc::Receiver<(u64, Result<Page, String>)>,
    generation: u64,
    query: String,
    due: Option<Instant>,
    busy: bool,
    total: u32,
    entries: Vec<Entry>,
    selection: BTreeSet<usize>,
    focused: Option<usize>,
    anchor: Option<usize>,
    scroll: usize,
    visible_rows: usize,
    status: Option<String>,
    replacing: bool,
    preserve_selection: bool,
    refresh_limit: usize,
    refresh_entries: Vec<Entry>,
    failed: bool,
    hovered: Option<usize>,
}
impl Search {
    fn new() -> Self {
        let wake = super::wake::Wake::default();
        let ready = wake.clone();
        let (sender, requests) = mpsc::channel::<Request>();
        let (responses, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(mut request) = requests.recv() {
                for next in requests.try_iter() {
                    request = next;
                }
                let result = everything::query(&request.query, request.offset);
                if responses.send((request.generation, result)).is_err() {
                    break;
                }
                ready.notify();
            }
        });
        Self {
            wake,
            sender,
            receiver,
            generation: 0,
            query: String::new(),
            due: None,
            busy: false,
            total: 0,
            entries: Vec::new(),
            selection: BTreeSet::new(),
            focused: None,
            anchor: None,
            scroll: 0,
            visible_rows: VISIBLE,
            status: None,
            replacing: false,
            preserve_selection: false,
            refresh_limit: 0,
            refresh_entries: Vec::new(),
            failed: false,
            hovered: None,
        }
    }
    fn change(&mut self, value: String) {
        self.wake.notify();
        self.generation += 1;
        self.preserve_selection = value.trim() == self.query;
        self.refresh_limit = if self.preserve_selection { self.entries.len() } else { 0 };
        self.refresh_entries = Vec::new();
        self.query = value.trim().into();
        if !self.preserve_selection || self.query.is_empty() {
            self.selection.clear();
            self.focused = None;
            self.anchor = None;
            self.scroll = 0;
        }
        if self.query.is_empty() {
            self.entries = Vec::new();
            self.total = 0;
        }
        self.hovered = None;
        self.failed = false;
        self.replacing = !self.query.is_empty();
        self.busy = !self.query.is_empty();
        self.status = self.busy.then(|| crate::i18n::text("ui-searching").into());
        self.due = self
            .busy
            .then(|| Instant::now() + Duration::from_millis(250));
    }
    fn request(&mut self, offset: u32) {
        if self
            .sender
            .send(Request {
                generation: self.generation,
                query: self.query.clone(),
                offset,
            })
            .is_err()
        {
            self.accept(
                self.generation,
                Err(crate::i18n::text("ui-search-thread-stopped-reopen-the-panel").into()),
            );
        } else {
            self.busy = true;
        }
    }
    fn accept(&mut self, generation: u64, result: Result<Page, String>) -> bool {
        if generation != self.generation || self.query.is_empty() {
            return false;
        }
        self.busy = false;
        match result {
            Ok(mut page) => {
                // Refresh the already loaded range before replacing the visible snapshot.
                // Bound this work by the previous result count, never the entire index.
                if self.replacing && self.refresh_limit > 0 {
                    let progress = !page.entries.is_empty();
                    self.refresh_entries.append(&mut page.entries);
                    if progress && self.refresh_entries.len() < self.refresh_limit.min(page.total as usize) {
                        self.request(self.refresh_entries.len() as u32);
                        return true;
                    }
                    page.offset = 0;
                    page.entries = std::mem::take(&mut self.refresh_entries);
                    self.refresh_limit = 0;
                }
                self.total = page.total;
                if page.offset == 0 {
                    let selected: BTreeSet<_> = if self.preserve_selection {
                        self.selection
                            .iter()
                            .filter_map(|i| self.entries.get(*i))
                            .map(|e| e.path.clone())
                            .collect()
                    } else {
                        BTreeSet::new()
                    };
                    let focused = self
                        .focused
                        .and_then(|i| self.entries.get(i))
                        .map(|e| e.path.clone());
                    let anchor = self.anchor.and_then(|i| self.entries.get(i)).map(|e| e.path.clone());
                    self.entries = page.entries;
                    self.selection = self
                        .entries
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| selected.contains(&e.path))
                        .map(|(i, _)| i)
                        .collect();
                    self.focused =
                        focused.and_then(|path| self.entries.iter().position(|e| e.path == path));
                    self.anchor = anchor.and_then(|path| self.entries.iter().position(|e| e.path == path));
                } else {
                    self.entries.extend(page.entries);
                }
                self.replacing = false;
                self.failed = false;
                self.hovered = None;
                self.scroll = self
                    .scroll
                    .min(self.entries.len().saturating_sub(self.visible_rows));
                if let Some(index) = self.focused {
                    if index < self.scroll {
                        self.scroll = index;
                    } else if index >= self.scroll + self.visible_rows {
                        self.scroll = index + 1 - self.visible_rows;
                    }
                }
                self.status = if self.entries.is_empty() {
                    Some(crate::i18n::text("ui-no-matching-files-found").into())
                } else {
                    None
                };
            }
            Err(error) => {
                self.refresh_entries = Vec::new();
                self.refresh_limit = 0;
                self.failed = true;
                if self.replacing {
                    self.entries = Vec::new();
                    self.selection.clear();
                    self.focused = None;
                    self.anchor = None;
                    self.total = 0;
                }
                self.replacing = false;
                self.status = Some(error);
            }
        }
        true
    }
    fn tick(&mut self) -> bool {
        let mut changed = false;
        if self.due.is_some_and(|t| Instant::now() >= t) {
            self.due = None;
            self.request(0);
            changed = true;
        }
        while let Ok((generation, result)) = self.receiver.try_recv() {
            changed |= self.accept(generation, result);
        }
        changed
    }
    fn more(&mut self) {
        if !self.busy
            && self.status.is_none()
            && self.entries.len() < self.total as usize
            && self.scroll + self.visible_rows >= self.entries.len()
        {
            self.request(self.entries.len() as u32);
        }
    }
    fn height(&self) -> f32 {
        if self.query.is_empty() {
            TOP
        } else if self.entries.is_empty() {
            TOP + 96.0
        } else {
            TOP + ROW_INSET + ROW * self.entries.len().min(self.visible_rows) as f32 + FOOTER
        }
    }
    fn select(&mut self, index: usize, ctrl: bool, shift: bool) {
        if index >= self.entries.len() {
            return;
        }
        if shift {
            let anchor = self.anchor.unwrap_or(index);
            if !ctrl {
                self.selection.clear();
            }
            self.selection.extend(anchor.min(index)..=anchor.max(index));
        } else {
            if ctrl {
                if !self.selection.remove(&index) {
                    self.selection.insert(index);
                }
            } else {
                self.selection.clear();
                self.selection.insert(index);
            }
            self.anchor = Some(index);
        }
        self.focused = Some(index);
        if index < self.scroll {
            self.scroll = index;
        } else if index >= self.scroll + self.visible_rows {
            self.scroll = index + 1 - self.visible_rows;
        }
    }
    fn selected(&self) -> Vec<ShellIdentity> {
        if self.replacing {
            return Vec::new();
        }
        self.selection
            .iter()
            .filter_map(|i| self.entries.get(*i))
            .map(|e| identity(e.path.clone()))
            .collect()
    }
    fn move_focus(&mut self, index: usize, ctrl: bool, shift: bool) {
        let selected = self.selection.clone();
        self.select(index, ctrl && shift, shift);
        if ctrl && !shift {
            self.selection = selected;
        }
    }

    fn row_at(&self, y: f32) -> Option<usize> {
        let y = y - TOP - ROW_INSET;
        if self.replacing || y < 0.0 || y >= ROW * self.entries.len().min(self.visible_rows) as f32
        {
            return None;
        }
        Some(self.scroll + (y / ROW) as usize).filter(|i| *i < self.entries.len())
    }

    fn footer(&self) -> String {
        if self.busy {
            return if self.replacing {
                crate::i18n::text("ui-searching")
            } else {
                crate::i18n::text("ui-loading-more")
            }
            .into();
        }
        if self.failed {
            return crate::i18n::text("ui-loading-failed-click-to-retry").into();
        }
        if self.selection.is_empty() {
            crate::i18n::format("ui-results", &[("arg0", format!("{}", self.total))])
        } else {
            crate::i18n::format("ui-results-selected", &[("arg0", format!("{}", self.total)), ("arg1", format!("{}", self.selection.len()))])
        }
    }
}
fn client_width(hwnd: HWND) -> f32 {
    let mut bounds = RECT::default();
    unsafe {
        GetClientRect(hwnd, &raw mut bounds);
    }
    bounds.right as f32 / scale(hwnd)
}

// A color-keyed EDIT exposes the pane backdrop while retaining native IME,
// caret and selection. The owner forwards clicks through transparent pixels.
struct Editor {
    hwnd: HWND,
    font: HFONT,
    font_family: String,
    dark: bool,
    dpi: u32,
    line_height: i32,
    alpha: u8,
}
impl Editor {
    fn new(owner: HWND) -> Result<Self, String> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                windows_sys::w!("EDIT"),
                std::ptr::null(),
                WS_POPUP | ES_AUTOHSCROLL as u32,
                0,
                0,
                1,
                1,
                owner,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err(crate::i18n::text("ui-could-not-create-search-box").into());
        }
        unsafe {
            SetPropW(owner, EDIT_PROPERTY, hwnd);
            SetWindowSubclass(owner, Some(input_color_proc), 1, 0);
            SetWindowSubclass(hwnd, Some(input_proc), 1, 0);
            SendMessageW(hwnd, EM_SETLIMITTEXT, 16384, 0);
        }
        Ok(Self {
            hwnd,
            font: std::ptr::null_mut(),
            font_family: String::new(),
            dark: false,
            dpi: 0,
            line_height: 24,
            alpha: 255,
        })
    }
    fn appearance(&mut self, owner: HWND, dark: bool) {
        let dpi = unsafe { GetDpiForWindow(owner) }.max(96);
        let family = crate::pane::fonts::family();
        if self.dpi == dpi && self.dark == dark && self.font_family == family {
            return;
        }
        let face: Vec<u16> = family.encode_utf16().chain([0]).collect();
        let font = unsafe {
            CreateFontW(
                -((14 * dpi / 96) as i32),
                0,
                0,
                0,
                FW_NORMAL as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                ANTIALIASED_QUALITY as u32,
                0,
                face.as_ptr(),
            )
        };
        if !font.is_null() {
            unsafe {
                SendMessageW(self.hwnd, WM_SETFONT, font as usize, 0);
                DeleteObject(self.font);
            }
            self.font = font;
            self.font_family = family;
        }
        unsafe {
            let background = if dark { 0x202020 } else { 0xf5f5f5 };
            SetLayeredWindowAttributes(self.hwnd, background, self.alpha, LWA_COLORKEY | LWA_ALPHA);
        }
        self.dark = dark;
        self.dpi = dpi;
        unsafe {
            let dc = GetDC(self.hwnd);
            let old = SelectObject(dc, self.font);
            let mut metrics = TEXTMETRICW::default();
            if GetTextMetricsW(dc, &raw mut metrics) != 0 {
                self.line_height = metrics.tmHeight.max(1);
            }
            SelectObject(dc, old);
            ReleaseDC(self.hwnd, dc);
        }
        self.position(owner);
        invalidate(self.hwnd);
    }
    fn opacity(&mut self, opacity: f32) {
        self.alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        unsafe {
            SetLayeredWindowAttributes(self.hwnd, if self.dark { 0x202020 } else { 0xf5f5f5 },
                self.alpha, LWA_COLORKEY | LWA_ALPHA);
        }
    }
    fn position(&self, owner: HWND) {
        position_editor(owner, self.hwnd, self.line_height);
    }
}
fn position_editor(owner: HWND, editor: HWND, line_height: i32) {
    let s = scale(owner);
    let mut r = RECT::default();
    let mut p = POINT {
        x: (44.0 * s) as i32,
        y: (((TOP * s).round() as i32 - line_height) / 2).max(0),
    };
    unsafe {
        GetClientRect(owner, &raw mut r);
        ClientToScreen(owner, &raw mut p);
        let mut before = RECT::default();
        GetWindowRect(editor, &raw mut before);
        let width = (r.right - (90.0 * s) as i32).max(1);
        let height = line_height;
        if before.left != p.x
            || before.top != p.y
            || before.right - before.left != width
            || before.bottom - before.top != height
        {
            SetWindowPos(
                editor,
                std::ptr::null_mut(),
                p.x,
                p.y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
    }
}
impl Drop for Editor {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.hwnd);
            DeleteObject(self.font);
        }
    }
}
// Color requests can reenter the owner while its application callback is busy
// forwarding focus/mouse messages. Handle them before that callback, without
// borrowing its state, so EDIT never falls back to the system white brush.
unsafe extern "system" fn input_color_proc(
    owner: HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    _: usize,
    _: usize,
) -> LRESULT {
    let editor = edit(owner);
    if msg == WM_WINDOWPOSCHANGED && !editor.is_null() {
        unsafe {
            let topmost = GetWindowLongW(owner, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0;
            let editor_topmost = GetWindowLongW(editor, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0;
            let mut previous = GetWindow(owner, GW_HWNDPREV);
            if topmost != editor_topmost || previous != editor {
                if previous == editor { previous = GetWindow(editor, GW_HWNDPREV); }
                let after = if topmost {
                    if previous.is_null() { HWND_TOPMOST } else { previous }
                } else if previous.is_null()
                    || GetWindowLongW(previous, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0
                { HWND_NOTOPMOST } else { previous };
                SetWindowPos(editor, after,
                    0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
            }
        }
    }
    if msg == WM_MOUSEACTIVATE && !editor.is_null() && unsafe { GetFocus() } == editor {
        return MA_NOACTIVATE as isize;
    }
    if matches!(msg, WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC)
        && !editor.is_null()
        && lp == editor as isize
    {
        return unsafe { input_background(editor, wp as HDC) } as isize;
    }
    unsafe { DefSubclassProc(owner, msg, wp, lp) }
}

unsafe fn input_background(hwnd: HWND, dc: HDC) -> HBRUSH {
    let mut key = 0x202020;
    unsafe {
        GetLayeredWindowAttributes(
            hwnd,
            &raw mut key,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        SetTextColor(dc, if key == 0x202020 { 0xf0f0f0 } else { 0x202020 });
        SetBkColor(dc, key);
        SetDCBrushColor(dc, key);
        GetStockObject(DC_BRUSH) as HBRUSH
    }
}

unsafe extern "system" fn input_proc(
    hwnd: HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    _: usize,
    _: usize,
) -> LRESULT {
    let owner = unsafe { GetParent(hwnd) };
    if msg == WM_WINDOWPOSCHANGING {
        let position = unsafe { &mut *(lp as *mut WINDOWPOS) };
        if position.flags & SWP_NOZORDER == 0
            && super::window::is_desktop_layer(owner)
        {
            // Focusing the popup EDIT must not lift it out of the pane's band.
            // Keep it immediately above the owner instead of above applications.
            unsafe {
                let mut previous = GetWindow(owner, GW_HWNDPREV);
                if previous == hwnd { previous = GetWindow(hwnd, GW_HWNDPREV); }
                position.hwndInsertAfter = if previous.is_null()
                    || GetWindowLongW(previous, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0
                { HWND_TOP } else { previous };
                position.flags |= SWP_NOOWNERZORDER;
            }
        }
    }
    // The native EDIT is an owned popup, so its clicks do not reach the pane's
    // borderless subclass. Apply the same desktop-band ordering before focus
    // handling, including clicks while the editor already holds focus.
    if matches!(msg, WM_MOUSEACTIVATE | WM_LBUTTONDOWN) {
        super::window::raise_among_peers(owner);
    }
    if matches!(msg, WM_SETFOCUS | WM_KILLFOCUS) {
        invalidate(owner);
    }
    if msg == WM_MOUSEACTIVATE && unsafe { GetFocus() } == hwnd {
        return MA_NOACTIVATE as isize;
    }
    if msg == WM_SETCURSOR {
        unsafe {
            SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_IBEAM));
        }
        return 1;
    }
    if msg == WM_ERASEBKGND {
        unsafe {
            let mut rect = RECT::default();
            GetClientRect(hwnd, &raw mut rect);
            let dc = wp as HDC;
            FillRect(dc, &rect, input_background(hwnd, dc));
        }
        return 1;
    }
    if msg == WM_MOUSEWHEEL {
        unsafe {
            PostMessageW(owner, msg, wp, lp);
        }
        return 0;
    }
    const COMPOSING: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.SearchComposing");
    if msg == WM_IME_STARTCOMPOSITION {
        unsafe {
            SetPropW(hwnd, COMPOSING, 1usize as _);
        }
    }
    if msg == WM_IME_ENDCOMPOSITION {
        unsafe {
            RemovePropW(hwnd, COMPOSING);
        }
    }
    if msg == WM_KEYDOWN && unsafe { GetPropW(hwnd, COMPOSING) }.is_null() {
        let ctrl = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
        match wp as u16 {
            VK_ESCAPE => {
                unsafe {
                    SetWindowTextW(hwnd, windows_sys::w!(""));
                }
                return 0;
            }
            VK_DOWN => {
                unsafe {
                    SetFocus(owner);
                    PostMessageW(owner, NAVIGATE, VK_DOWN as usize, 0);
                }
                return 0;
            }
            VK_RETURN | VK_F5 => {
                unsafe {
                    PostMessageW(owner, WM_COMMAND, REFRESH, 0);
                }
                return 0;
            }
            0x41 | 0x4c if ctrl => {
                unsafe {
                    SendMessageW(hwnd, EM_SETSEL, 0, -1);
                }
                return 0;
            }
            _ => {}
        }
    }
    let result = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
    if matches!(
        msg,
        WM_CHAR | WM_PASTE | WM_CUT | WM_CLEAR | WM_UNDO | WM_SETTEXT | WM_IME_ENDCOMPOSITION
    ) {
        unsafe {
            PostMessageW(owner, INPUT, 0, 0);
        }
    }
    result
}

fn fit_search(work: RECT, current: RECT, scale: f32, state: &mut Search) -> RECT {
    let previous_rows = state.visible_rows;
    // Keep the input anchored while results expand. Move only when even one
    // result (or the empty/error state) cannot fit below the input.
    let minimum = if state.query.is_empty() {
        TOP
    } else if state.entries.is_empty() {
        TOP + 96.0
    } else {
        TOP + ROW_INSET + ROW + FOOTER
    };
    let anchor = current.top.clamp(
        work.top,
        (work.bottom - (minimum * scale).ceil() as i32).max(work.top),
    );
    state.visible_rows = (((work.bottom - anchor) as f32 / scale - TOP - ROW_INSET - FOOTER) / ROW)
        .floor()
        .clamp(1.0, VISIBLE as f32) as usize;
    state.scroll = state
        .scroll
        .min(state.entries.len().saturating_sub(state.visible_rows));
    if let Some(focused) = state
        .focused
        .filter(|_| previous_rows != state.visible_rows)
    {
        if focused < state.scroll {
            state.scroll = focused;
        }
        if focused >= state.scroll + state.visible_rows {
            state.scroll = focused + 1 - state.visible_rows;
        }
    }
    let height = ((state.height() * scale).round() as i32)
        .min(work.bottom - work.top)
        .max(1);
    let width = (current.right - current.left)
        .min(work.right - work.left)
        .max(1);
    let left = current
        .left
        .clamp(work.left, (work.right - width).max(work.left));
    let top = anchor.clamp(work.top, (work.bottom - height).max(work.top));
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

fn resize(hwnd: HWND, state: &mut Search) {
    unsafe {
        let mut current = RECT::default();
        GetWindowRect(hwnd, &raw mut current);
        let mut monitor = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if GetMonitorInfoW(
            MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
            &raw mut monitor,
        ) != 0
        {
            monitor.rcWork
        } else {
            current
        };
        let bounds = fit_search(work, current, scale(hwnd), state);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            bounds.left,
            bounds.top,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    // SetWindowPos can reenter a busy owner callback, so do not depend on
    // WM_WINDOWPOSCHANGED to move the owned native editor with the pane.
    let editor = edit(hwnd);
    if !editor.is_null() {
        let mut bounds = RECT::default();
        unsafe {
            GetWindowRect(editor, &raw mut bounds);
        }
        position_editor(hwnd, editor, bounds.bottom - bounds.top);
    }
    invalidate(hwnd);
}

fn search_frame_hit(bounds: RECT, point: POINT, scale: f32, locked: bool) -> u32 {
    let x = point.x as f32 / scale;
    let y = point.y as f32 / scale;
    let width = bounds.right as f32 / scale;
    let edge = 5.0_f32.min(width / 2.0);
    if x < edge {
        HTLEFT
    } else if x >= width - edge {
        HTRIGHT
    } else if !locked && y < TOP && (x < 40.0 || x > width - 12.0 || y < 5.0 || y >= TOP - 5.0) {
        HTCAPTION
    } else {
        HTCLIENT
    }
}

pub(super) fn create(
    rect: RectDip,
    model: Rc<RefCell<GroupModel>>,
    event: impl FnMut(Event) -> bool + 'static,
) -> Result<windows_window::Window, String> {
    let initial_dark = {
        let m = model.borrow();
        super::theme::panel_contrast(m.backdrop, m.dark, m.options.text, true).light_text
    };
    let event = Rc::new(RefCell::new(event));
    let callback = Rc::clone(&event);
    let editor: Rc<RefCell<Option<Editor>>> = Rc::new(RefCell::new(None));
    let input = Rc::clone(&editor);
    let mut drawing: Option<Drawing> = None;
    let mut paint_recovery = super::composition::PaintRecovery::default();
    let mut visibility = super::visibility::Transition::default();
    let mut state = Search::new();
    let mailbox = Rc::new(RefCell::new(control::Mailbox::default()));
    let control_mailbox = Rc::clone(&mailbox);
    let wake = state.wake.clone();
    let mut last_click: Option<(std::path::PathBuf, Instant)> = None;
    let mut move_origin: Option<super::snap::DragOrigin> = None;
    let mut tooltip: Option<tooltip::Tooltip> = None;
    let mut error_tip = false;
    let prepared = Rc::new(std::cell::Cell::new(false));
    let show_prepared = Rc::clone(&prepared);
    let window = windows_window::Window::new(crate::i18n::text("ui-everything-search"))
        .style(WS_POPUP | WS_THICKFRAME)
        .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP)
        .size(rect.width.max(1.0) as i32, TOP as i32)
        .on_message(move |raw, msg, wp, lp| {
            if unsafe { crate::window_visibility::defer_show(msg, lp, show_prepared.get()) } {
                return Some(0);
            }
            let hwnd = raw.cast();
            if msg == crate::i18n::CHANGED {
                unsafe { SetWindowTextW(hwnd, crate::i18n::wide("ui-everything-search")); }
                if state.busy { state.status = Some(crate::i18n::text("ui-searching").into()); }
                invalidate(hwnd);
                return Some(0);
            }
            #[cfg(test)]
            if msg == WM_APP + 199 {
                return Some(drawing.as_ref().map_or(-1, |d| (d.surface.current_opacity() * 1000.0) as isize));
            }
            if visibility.message(hwnd, msg, wp, lp, drawing.as_ref().map(|d| &d.surface), |opacity| {
                if let Some(editor) = input.borrow_mut().as_mut() { editor.opacity(opacity); }
            }) { return Some(0); }
            if msg == control::MESSAGE {control::receive(hwnd,&mut state,&control_mailbox);return Some(0);}
            match msg {
                WM_NCCALCSIZE | WM_ERASEBKGND => return Some(0),
                WM_DESTROY => {
                    control::remove(hwnd);
                    tooltip = None;
                    state.wake.unbind();
                    unsafe {
                        KillTimer(hwnd, POLL);
                    }
                    return Some(0);
                }
                WM_CLOSE | super::visibility::CLOSED => {
                    let event = Rc::clone(&callback);
                    super::window::defer_action(move || {
                        (event.borrow_mut())(Event::ClosePane);
                    });
                    return Some(0);
                }
                WM_NCHITTEST => {
                    let mut p = POINT {
                        x: lp as u16 as i16 as i32,
                        y: (lp >> 16) as u16 as i16 as i32,
                    };
                    let mut r = RECT::default();
                    unsafe {
                        ScreenToClient(hwnd, &raw mut p);
                        GetClientRect(hwnd, &raw mut r);
                    }
                    return Some(
                        search_frame_hit(r, p, scale(hwnd), model.borrow().locked) as isize
                    );
                }
                WM_SETCURSOR if lp as u16 as u32 == HTCLIENT => {
                    let mut point = POINT::default();
                    unsafe {
                        GetCursorPos(&raw mut point);
                        ScreenToClient(hwnd, &raw mut point);
                        SetCursor(LoadCursorW(
                            std::ptr::null_mut(),
                            if point.y as f32 / scale(hwnd) < TOP
                                && point.x as f32 / scale(hwnd) < client_width(hwnd) - 44.0
                            {
                                IDC_IBEAM
                            } else {
                                IDC_ARROW
                            },
                        ));
                    }
                    return Some(1);
                }
                WM_GETMINMAXINFO if lp != 0 => {
                    let info = unsafe { &mut *(lp as *mut MINMAXINFO) };
                    info.ptMinTrackSize.x = 1;
                    info.ptMinTrackSize.y = (TOP * scale(hwnd)).round() as i32;
                    return Some(0);
                }
                WM_ENTERSIZEMOVE => {
                    let mut bounds = RECT::default();
                    let mut pointer = POINT::default();
                    move_origin = if unsafe { GetWindowRect(hwnd, &raw mut bounds) } != 0
                        && unsafe { GetCursorPos(&raw mut pointer) } != 0
                    {
                        Some(super::snap::DragOrigin::new(bounds, pointer))
                    } else {
                        None
                    };
                    return Some(0);
                }
                WM_MOVING if lp != 0 => {
                    if let Some(origin) = &move_origin {
                        let mut pointer = POINT::default();
                        if unsafe { GetCursorPos(&raw mut pointer) } != 0 {
                            unsafe {
                                *(lp as *mut RECT) = origin.proposal(pointer);
                            }
                        }
                    }
                    (callback.borrow_mut())(Event::Moving(lp as *mut RECT));
                    return Some(1);
                }
                WM_SIZING if lp != 0 => {
                    let bounds = unsafe { &mut *(lp as *mut RECT) };
                    let proposal = *bounds;
                    (callback.borrow_mut())(Event::Sizing(bounds, proposal, wp as u32));
                    return Some(1);
                }
                WM_WINDOWPOSCHANGED | WM_SIZE => {
                    if let Some(input) = input.borrow().as_ref() {
                        input.position(hwnd);
                    }
                    if msg == WM_SIZE {
                        invalidate(hwnd);
                    }
                }
                WM_SHOWWINDOW => {
                    if !edit(hwnd).is_null() {
                        unsafe {
                            ShowWindow(
                                edit(hwnd),
                                if wp == 0 { SW_HIDE } else { SW_SHOWNOACTIVATE },
                            );
                        }
                    }
                }
                FOCUS_INPUT => {
                    let editor = edit(hwnd);
                    if !editor.is_null() {
                        unsafe {
                            SetFocus(editor);
                            SendMessageW(editor, EM_SETSEL, 0, -1);
                        }
                    }
                    return Some(0);
                }
                RESTORE_LAYOUT => {
                    resize(hwnd, &mut state);
                    invalidate(hwnd);
                    return Some(0);
                }
                WM_EXITSIZEMOVE => {
                    move_origin = None;
                    resize(hwnd, &mut state);
                    let mut r = RECT::default();
                    unsafe {
                        GetWindowRect(hwnd, &raw mut r);
                    }
                    let s = scale(hwnd);
                    (callback.borrow_mut())(Event::Geometry(RectDip::new(
                        r.left as f32 / s,
                        r.top as f32 / s,
                        (r.right - r.left) as f32 / s,
                        rect.height,
                    )));
                }
                WM_DPICHANGED => {
                    if let Some(input) = input.borrow_mut().as_mut() {
                        let m = model.borrow();
                        input.appearance(
                            hwnd,
                            super::theme::panel_contrast(m.backdrop, m.dark, m.options.text, true)
                                .light_text,
                        );
                    }
                    if lp != 0 {
                        let r = unsafe { &*(lp as *const RECT) };
                        unsafe {
                            SetWindowPos(
                                hwnd,
                                std::ptr::null_mut(),
                                r.left,
                                r.top,
                                r.right - r.left,
                                (state.height() * scale(hwnd)) as i32,
                                SWP_NOZORDER | SWP_NOACTIVATE,
                            );
                        }
                    }
                    resize(hwnd, &mut state);
                }
                INPUT => {
                    let value = text(edit(hwnd));
                    invalidate(hwnd);
                    if value.trim() != state.query {
                        if let Some(tip) = &mut tooltip {
                            tip.hide();
                        }
                        last_click = None;
                        state.change(value);
                        resize(hwnd, &mut state);
                    }
                    return Some(0);
                }
                WM_COMMAND if (wp >> 16) as u32 == EN_CHANGE => {
                    unsafe {
                        PostMessageW(hwnd, INPUT, 0, 0);
                    }
                    return Some(0);
                }
                message if message == super::wake::READY || (message == WM_TIMER && wp == POLL) => {
                    state.wake.received();
                    unsafe {
                        KillTimer(hwnd, POLL);
                    }
                    if state.tick() {
                        last_click = None;
                        if let Some(tip) = &mut tooltip {
                            tip.hide();
                        }
                        resize(hwnd, &mut state);
                    }
                    if let Some(due) = state.due {
                        let delay = due
                            .saturating_duration_since(Instant::now())
                            .as_millis()
                            .clamp(1, 1000) as u32;
                        unsafe {
                            SetTimer(hwnd, POLL, delay, None);
                        }
                    }
                    if let Some(input) = input.borrow_mut().as_mut() {
                        let m = model.borrow();
                        input.appearance(
                            hwnd,
                            super::theme::panel_contrast(m.backdrop, m.dark, m.options.text, true)
                                .light_text,
                        );
                    }
                    return Some(0);
                }
                CLEAR_SELECTION => {
                    state.selection.clear();
                    state.focused = None;
                    state.anchor = None;
                    last_click = None;
                    invalidate(hwnd);
                    return Some(0);
                }
                WM_MOUSEWHEEL => {
                    state.hovered = None;
                    if let Some(tip) = &mut tooltip {
                        tip.hide();
                    }
                    let delta = (wp >> 16) as u16 as i16;
                    if delta > 0 {
                        state.scroll = state.scroll.saturating_sub(3);
                    } else {
                        state.scroll = (state.scroll + 3)
                            .min(state.entries.len().saturating_sub(state.visible_rows));
                        state.more();
                    }
                    invalidate(hwnd);
                    return Some(0);
                }
                WM_RBUTTONDOWN => {
                    let y = (lp >> 16) as u16 as i16 as f32 / scale(hwnd);
                    if let Some(row) = state.row_at(y) {
                        unsafe {
                            SetFocus(hwnd);
                        }
                        if !state.selection.contains(&row) {
                            state.select(row, false, false);
                        }
                        (callback.borrow_mut())(Event::PaneItemFocus);
                        invalidate(hwnd);
                    }
                    return Some(0);
                }
                WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                    let y = (lp >> 16) as u16 as i16 as f32 / scale(hwnd);
                    let x = lp as u16 as i16 as f32 / scale(hwnd);
                    let width = client_width(hwnd);
                    if y < TOP
                        && x >= width - 44.0
                        && x < width - 8.0
                        && unsafe { GetWindowTextLengthW(edit(hwnd)) } > 0
                    {
                        unsafe {
                            SetWindowTextW(edit(hwnd), windows_sys::w!(""));
                            SetFocus(edit(hwnd));
                        }
                        return Some(0);
                    }
                    if state.failed
                        && ((!state.entries.is_empty() && y >= state.height() - FOOTER)
                            || (state.entries.is_empty()
                                && (TOP + 60.0..TOP + 88.0).contains(&y)
                                && (18.0..102.0).contains(&x)))
                    {
                        unsafe {
                            PostMessageW(hwnd, WM_COMMAND, REFRESH, 0);
                        }
                        return Some(0);
                    }
                    if state.failed
                        && state.entries.is_empty()
                        && (TOP + 60.0..TOP + 88.0).contains(&y)
                        && x >= (width - 146.0).max(112.0)
                        && x < width - 18.0
                    {
                        let owner = hwnd as isize;
                        super::window::defer_action(move || {
                            if unsafe { IsWindow(owner as _) } == 0 {
                                return;
                            }
                            if let Err(error) = everything_settings::launch() {
                                super::window::error_for(owner as _, &error);
                            } else {
                                unsafe {
                                    PostMessageW(owner as _, WM_COMMAND, REFRESH, 0);
                                }
                            }
                        });
                        return Some(0);
                    }
                    if y < TOP {
                        let editor = edit(hwnd);
                        let mut point = POINT {
                            x: lp as u16 as i16 as i32,
                            y: (lp >> 16) as u16 as i16 as i32,
                        };
                        unsafe {
                            MapWindowPoints(hwnd, editor, &raw mut point, 1);
                            if GetFocus() != editor {
                                SetFocus(editor);
                            }
                            let position =
                                (point.x as u16 as usize) | ((point.y as u16 as usize) << 16);
                            SendMessageW(editor, msg, wp, position as isize);
                        }
                    } else if let Some(row) = state.row_at(y) {
                        unsafe {
                            SetFocus(hwnd);
                        }
                        let ctrl = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
                        let shift = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
                        state.select(row, ctrl, shift);
                        (callback.borrow_mut())(Event::PaneItemFocus);
                        if !ctrl
                            && !shift
                            && same_click_target(last_click.as_ref(), &state.entries[row].path,
                                Duration::from_millis(u64::from(unsafe { GetDoubleClickTime() })))
                        {
                            action(hwnd, OPEN, state.selected());
                            last_click = None;
                        } else {
                            last_click = Some((state.entries[row].path.clone(), Instant::now()));
                        }
                    }
                    invalidate(hwnd);
                    return Some(0);
                }
                NAVIGATE | WM_KEYDOWN => {
                    let mods = super::keyboard::Modifiers::current();
                    let key = wp as u16;
                    if mods.ctrl && key == 0x4c {
                        unsafe {
                            SetFocus(edit(hwnd));
                            SendMessageW(edit(hwnd), EM_SETSEL, 0, -1);
                        }
                        return Some(0);
                    }
                    if key == VK_ESCAPE {
                        unsafe {
                            SetWindowTextW(edit(hwnd), windows_sys::w!(""));
                            SetFocus(edit(hwnd));
                        }
                        return Some(0);
                    }
                    if mods.ctrl && key == 0x41 {
                        if !state.replacing {
                            state.selection.extend(0..state.entries.len());
                        }
                        (callback.borrow_mut())(Event::PaneItemFocus);
                        invalidate(hwnd);
                        return Some(0);
                    }
                    if matches!(key, VK_UP | VK_DOWN | VK_HOME | VK_END | VK_PRIOR | VK_NEXT) {
                        if key == VK_UP && state.focused == Some(0) && !mods.ctrl && !mods.shift {
                            unsafe {
                                SetFocus(edit(hwnd));
                            }
                            invalidate(hwnd);
                            return Some(0);
                        }
                        if !state.entries.is_empty() && !state.replacing {
                            let current = state.focused.unwrap_or(0);
                            let index = match key {
                                VK_UP => current.saturating_sub(1),
                                VK_DOWN => {
                                    if state.focused.is_none() {
                                        0
                                    } else {
                                        current + 1
                                    }
                                }
                                VK_HOME => 0,
                                VK_END => state.entries.len() - 1,
                                VK_PRIOR => current.saturating_sub(state.visible_rows),
                                _ => current + state.visible_rows,
                            }
                            .min(state.entries.len() - 1);
                            state.move_focus(index, mods.ctrl, mods.shift);
                            state.more();
                            (callback.borrow_mut())(Event::PaneItemFocus);
                            invalidate(hwnd);
                        }
                        return Some(0);
                    }
                    let command = if super::peek::matches(key, &mods, lp & (1 << 30) != 0) {
                        Some(PEEK)
                    } else {
                        match key {
                            VK_RETURN => Some(if mods.ctrl { LOCATION } else { OPEN }),
                            VK_F5 => Some(REFRESH),
                            VK_DELETE if !mods.shift && !mods.ctrl && !mods.alt => Some(DELETE),
                            0x43 if mods.ctrl => Some(COPY),
                            0x58 if mods.ctrl => Some(CUT),
                            _ => None,
                        }
                    };
                    if let Some(command) = command {
                        if lp & (1 << 30) == 0 {
                            unsafe {
                                PostMessageW(hwnd, WM_COMMAND, command, 0);
                            }
                        }
                        return Some(0);
                    }
                }
                WM_COMMAND => {
                    if wp == REFRESH {
                        last_click = None;
                        state.change(text(edit(hwnd)));
                        resize(hwnd, &mut state);
                    } else {
                        action(hwnd, wp, state.selected());
                    }
                    return Some(0);
                }
                WM_SETFOCUS | WM_KILLFOCUS => {
                    invalidate(hwnd);
                }
                WM_MOUSELEAVE => {
                    state.hovered = None;
                    error_tip = false;
                    if let Some(tip) = &mut tooltip {
                        tip.hide();
                    }
                    invalidate(hwnd);
                }
                WM_MOUSEMOVE => {
                    let y = (lp >> 16) as u16 as i16 as f32 / scale(hwnd);
                    let hovered = state.row_at(y);
                    let show_error = state.failed
                        && state.entries.is_empty()
                        && (TOP + 10.0..TOP + 56.0).contains(&y);
                    if hovered != state.hovered || show_error != error_tip {
                        state.hovered = hovered;
                        error_tip = show_error;
                        if tooltip.is_none() {
                            tooltip = tooltip::Tooltip::new(hwnd);
                        }
                        if let Some(tip) = &mut tooltip {
                            if let Some(row) = hovered {
                                tip.show_for_row(
                                    hwnd,
                                    &state.entries[row].path.to_string_lossy(),
                                    TOP + ROW_INSET + (row - state.scroll) as f32 * ROW,
                                );
                            } else if show_error {
                                tip.show_for_row(
                                    hwnd,
                                    state.status.as_deref().unwrap_or(crate::i18n::text("ui-please-try-again")),
                                    TOP + 10.0,
                                );
                            } else {
                                tip.hide();
                            }
                        }
                        invalidate(hwnd);
                    }
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
                WM_CONTEXTMENU => {
                    let items = state.selected();
                    let (theme, backdrop) = {
                        let m = model.borrow();
                        (m.theme, m.backdrop)
                    };
                    let event = Rc::clone(&callback);
                    let owner = hwnd as isize;
                    super::window::defer_action(move || {
                        let hwnd = owner as HWND;
                        if unsafe { IsWindow(hwnd) } == 0 {
                            return;
                        }
                        let mut point = POINT::default();
                        unsafe {
                            GetCursorPos(&raw mut point);
                        }
                        let mut rows = Vec::new();
                        if !items.is_empty() {
                            for (id, label, shortcut) in [
                                (OPEN, crate::i18n::text("ui-open"), "Enter"),
                                (LOCATION, crate::i18n::text("ui-open-file-location"), ""),
                                (COPY, crate::i18n::text("ui-copy"), "Ctrl+C"),
                                (CUT, crate::i18n::text("ui-cut"), "Ctrl+X"),
                                (DELETE, crate::i18n::text("ui-delete-8907"), "Del"),
                                (PEEK, crate::i18n::text("ui-preview"), ""),
                            ] {
                                rows.push(super::menu::entry(id as i32, label, "", shortcut));
                            }
                            rows.push(super::menu::entry(0, "", "", ""));
                        }
                        rows.push(super::menu::entry(201, crate::i18n::text("ui-settings"), "", ""));
                        rows.push(super::menu::entry(202, crate::i18n::text("ui-close-search-panel"), "", ""));
                        let command =
                            super::menu::show_entries(hwnd, point, false, theme, backdrop, rows);
                        match command {
                            201 => {
                                (event.borrow_mut())(Event::Settings);
                            }
                            202 => unsafe {
                                PostMessageW(hwnd, WM_CLOSE, 0, 0);
                            },
                            _ => action(hwnd, command as usize, items),
                        }
                    });
                    return Some(0);
                }
                WM_PAINT => {
                    if let Some(input) = input.borrow_mut().as_mut() {
                        let m = model.borrow();
                        input.appearance(
                            hwnd,
                            super::theme::panel_contrast(m.backdrop, m.dark, m.options.text, true)
                                .light_text,
                        );
                    }
                    let mut ps = PAINTSTRUCT::default();
                    unsafe {
                        BeginPaint(hwnd, &raw mut ps);
                        EndPaint(hwnd, &ps);
                    }
                    paint_recovery.paint(hwnd, &mut drawing, "pane.search", |drawing| {
                        if drawing.is_none() {
                            *drawing = Some(Drawing::new(hwnd)?);
                        }
                        drawing.as_mut().unwrap().paint(hwnd, &model.borrow(), &state)
                    });
                    return Some(0);
                }
                _ => {}
            }
            None
        })
        .create()
        .map_err(|e| e.to_string())?;
    let hwnd = window.hwnd().cast();
    unsafe {
        SetWindowSubclass(hwnd, Some(super::window::borderless_proc), 1, 0);
    }
    let mut native = Editor::new(hwnd)?;
    native.appearance(hwnd, initial_dark);
    *editor.borrow_mut() = Some(native);
    let s = scale(hwnd);
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            (rect.x * s) as i32,
            (rect.y * s) as i32,
            (rect.width.max(1.0) * s) as i32,
            (TOP * s) as i32,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    if let Some(input) = editor.borrow().as_ref() {
        input.position(hwnd);
    }
    invalidate(hwnd);
    unsafe {
        SendMessageW(hwnd, WM_PAINT, 0, 0);
        prepared.set(true);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        ShowWindow(edit(hwnd), SW_SHOWNOACTIVATE);
    }
    control::register(window.hwnd().cast(),&mailbox);
    wake.bind(hwnd as isize);
    Ok(window)
}

#[cfg(test)]
mod tests;
fn action(hwnd: HWND, command: usize, items: Vec<ShellIdentity>) {
    if items.is_empty() {
        return;
    }
    let owner = hwnd as isize;
    super::window::defer_action(move || {
        if unsafe { IsWindow(owner as HWND) } == 0 {
            return;
        }
        let result = match command {
            OPEN => luciddesk_shell::open_shell_identity(owner, &items[0]).map_err(|e| e.to_string()),
            LOCATION => show_location(&items[0]),
            PEEK => super::peek::open_path(&items[0]),
            COPY | CUT | DELETE => luciddesk_shell::invoke_file_commands(
                windows::Win32::Foundation::HWND(owner as _),
                &items,
                match command {
                    COPY => luciddesk_shell::FileCommand::Copy,
                    CUT => luciddesk_shell::FileCommand::Cut,
                    _ => luciddesk_shell::FileCommand::Delete,
                },
            )
            .map(|_| ())
            .map_err(|e| e.to_string()),
            _ => Ok(()),
        };
        if let Err(error) = result {
            super::window::error_for(owner as _, &error);
        }
        if command == DELETE {
            unsafe {
                PostMessageW(owner as HWND, WM_COMMAND, REFRESH, 0);
            }
        }
    });
}
fn show_location(item: &ShellIdentity) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{SHOpenFolderAndSelectItems, SHParseDisplayName},
    };
    let path = item.file_system_path().ok_or(crate::i18n::text("ui-search-result-has-no-file-path"))?;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut pidl = std::ptr::null_mut();
        SHParseDisplayName(
            windows_core::PCWSTR(path.as_ptr()),
            None,
            &raw mut pidl,
            0,
            None,
        )
        .map_err(|e| e.to_string())?;
        let result = SHOpenFolderAndSelectItems(pidl, None, 0).map_err(|e| e.to_string());
        CoTaskMemFree(Some(pidl.cast()));
        result
    }
}
