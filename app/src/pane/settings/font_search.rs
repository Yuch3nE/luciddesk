//! Native font search editor lifetime and font filtering.
use super::*;

pub(super) struct SearchFont(pub(super) HFONT);
impl Drop for SearchFont {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.0);
        }
    }
}
pub(super) type SearchFontOwner = Rc<RefCell<Option<SearchFont>>>;

pub(super) fn attach_search_lifetime(
    editor: windows_sys::Win32::Foundation::HWND,
    font: &SearchFontOwner,
) -> bool {
    let reference = Rc::into_raw(Rc::clone(font));
    if unsafe {
        windows_sys::Win32::UI::Shell::SetWindowSubclass(
            editor,
            Some(font_search_focus),
            0x4c46,
            reference as usize,
        )
    } == 0
    {
        unsafe {
            drop(Rc::from_raw(reference));
        }
        return false;
    }
    true
}

pub(super) fn create_font_search(
    owner: windows_sys::Win32::Foundation::HWND,
) -> windows_sys::Win32::Foundation::HWND {
    // The settings backdrop is a composition surface. An owned popup keeps
    // the native EDIT above that surface, as with the search panel's editor.
    unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            windows_sys::w!("EDIT"),
            windows_sys::w!(""),
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
    }
}

pub(super) unsafe extern "system" fn font_search_colors(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    _: usize,
    _: usize,
) -> isize {
    use windows_sys::Win32::UI::Shell::DefSubclassProc;
    unsafe {
        let editor = GetPropW(hwnd, windows_sys::w!("LucidDesk.FontSearch"));
        if matches!(msg, WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC)
            && !editor.is_null()
            && lp == editor as isize
        {
            let mut key = 0;
            GetLayeredWindowAttributes(
                editor,
                &raw mut key,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            let dc = wp as HDC;
            SetTextColor(dc, if key == 0x202020 { 0xf0f0f0 } else { 0x202020 });
            SetBkColor(dc, key);
            SetDCBrushColor(dc, key);
            return GetStockObject(DC_BRUSH) as isize;
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

pub(super) unsafe extern "system" fn font_search_focus(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    id: usize,
    reference: usize,
) -> isize {
    unsafe {
        if msg == WM_NCDESTROY {
            // The editor owns a reference even if its owner's Rust callback was
            // detached before destruction. Release only after native teardown.
            let font = Rc::from_raw(reference as *const RefCell<Option<SearchFont>>);
            let owner = GetWindow(hwnd, GW_OWNER);
            if GetPropW(owner, windows_sys::w!("LucidDesk.FontSearch")) == hwnd {
                RemovePropW(owner, windows_sys::w!("LucidDesk.FontSearch"));
            }
            windows_sys::Win32::UI::Shell::RemoveWindowSubclass(hwnd, Some(font_search_focus), id);
            let result = windows_sys::Win32::UI::Shell::DefSubclassProc(hwnd, msg, wp, lp);
            font.borrow_mut().take();
            return result;
        }
        if msg == WM_SETCURSOR {
            SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_IBEAM));
            return 1;
        }
        let owner = GetWindow(hwnd, GW_OWNER);
        if msg == WM_MOUSEWHEEL {
            PostMessageW(owner, msg, wp, lp);
            return 0;
        }
        if msg == WM_KEYDOWN && matches!(wp as u16, VK_TAB | VK_DOWN) {
            SetFocus(owner);
            PostMessageW(owner, WM_KEYDOWN, wp, lp);
            return 0;
        }
        if msg == WM_KEYDOWN && wp == VK_ESCAPE as usize {
            SetWindowTextW(hwnd, windows_sys::w!(""));
            return 0;
        }
        if msg == WM_CHAR && matches!(wp, 9 | 27) {
            return 0;
        }
        if msg == WM_SETFOCUS {
            // Merely opening the font page must not activate an input method.
            // Restore the default context only when the user edits the query.
            windows_sys::Win32::UI::Input::Ime::ImmAssociateContextEx(
                hwnd, std::ptr::null_mut(), windows_sys::Win32::UI::Input::Ime::IACE_DEFAULT);
        }
        if matches!(msg, WM_SETFOCUS | WM_KILLFOCUS) {
            InvalidateRect(GetWindow(hwnd, GW_OWNER), std::ptr::null(), 0);
        }
        windows_sys::Win32::UI::Shell::DefSubclassProc(hwnd, msg, wp, lp)
    }
}

pub(super) fn font_search_hit(scene: &Scene, x: f32, y: f32) -> bool {
    (scene.fixed_list()
        || scene
            .viewport
            .as_ref()
            .is_none_or(|viewport| contains(viewport, x, y)))
        && scene.controls.iter().any(|control| {
            matches!(control.action, Action::FontSearch) && contains(&control.bounds, x, y)
        })
}

pub(super) fn normalized_font_name(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    for c in value.chars() {
        let c = if ('\u{ff01}'..='\u{ff5e}').contains(&c) {
            char::from_u32(c as u32 - 0xfee0).unwrap()
        } else {
            c
        };
        if c.is_alphanumeric() {
            normalized.extend(c.to_lowercase());
        } else {
            normalized.push(' ');
        }
    }
    normalized
}
pub(super) fn filter_fonts(names: &[String], query: &str) -> Vec<String> {
    let query = normalized_font_name(query);
    let tokens: Vec<_> = query.split_whitespace().collect();
    if tokens.is_empty() {
        return names.to_vec();
    }
    names
        .iter()
        .filter(|name| {
            let name = normalized_font_name(name);
            let compact: String = name.chars().filter(|c| !c.is_whitespace()).collect();
            tokens
                .iter()
                .all(|token| name.contains(token) || compact.contains(token))
        })
        .cloned()
        .collect()
}

/// Own the editor's Rust state; the native owner controls its window lifetime.
pub(super) struct Editor {
    pub hwnd: windows_sys::Win32::Foundation::HWND,
    font: SearchFontOwner,
    style: (String, u32),
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            hwnd: std::ptr::null_mut(),
            font: Rc::new(RefCell::new(None)),
            style: (String::new(), 0),
        }
    }
}
impl Editor {
    pub fn sync(
        &mut self,
        hwnd: windows_sys::Win32::Foundation::HWND,
        page: usize,
        scene: &Scene,
        scale: f32,
        dark: bool,
    ) -> bool {
        if page == 11 {
            if let Some(control) = scene
                .controls
                .iter()
                .find(|c| matches!(c.action, Action::FontSearch))
            {
                let r = control.bounds;
                unsafe {
                    if self.hwnd.is_null() {
                        self.hwnd = create_font_search(hwnd);
                        if self.hwnd.is_null() {
                            return false;
                        }
                        windows_sys::Win32::UI::Input::Ime::ImmAssociateContextEx(
                            self.hwnd, std::ptr::null_mut(), 0);
                        SetPropW(hwnd, windows_sys::w!("LucidDesk.FontSearch"), self.hwnd);
                        windows_sys::Win32::UI::Shell::SetWindowSubclass(
                            hwnd,
                            Some(font_search_colors),
                            0x4c46,
                            0,
                        );
                        if !attach_search_lifetime(self.hwnd, &self.font) {
                            RemovePropW(hwnd, windows_sys::w!("LucidDesk.FontSearch"));
                            DestroyWindow(self.hwnd);
                            self.hwnd = std::ptr::null_mut();
                            return false;
                        }
                        SendMessageW(
                            self.hwnd,
                            windows_sys::Win32::UI::Controls::EM_SETLIMITTEXT,
                            128,
                            0,
                        );
                    }
                    let font_style = (fonts::family(), (scale * 96.0).round() as u32);
                    if self.style != font_style {
                        let face: Vec<u16> = font_style.0.encode_utf16().chain(Some(0)).collect();
                        let font = CreateFontW(
                            -(14.0 * scale).round() as i32,
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
                        );
                        if !font.is_null() {
                            SendMessageW(self.hwnd, WM_SETFONT, font as usize, 1);
                            *self.font.borrow_mut() = Some(SearchFont(font));
                            self.style = font_style;
                        }
                    }
                    SetLayeredWindowAttributes(
                        self.hwnd,
                        if dark { 0x202020 } else { 0xf5f5f5 },
                        255,
                        LWA_COLORKEY,
                    );
                    SendMessageW(
                        self.hwnd,
                        windows_sys::Win32::UI::Controls::EM_SETCUEBANNER,
                        0,
                        windows_sys::w!("") as isize,
                    );
                    let mut origin = windows_sys::Win32::Foundation::POINT {
                        x: ((r.left + 40.0) * scale) as i32,
                        y: ((r.top + 10.0) * scale) as i32,
                    };
                    ClientToScreen(hwnd, &raw mut origin);
                    SetWindowPos(
                        self.hwnd,
                        std::ptr::null_mut(),
                        origin.x,
                        origin.y,
                        ((r.right - r.left - 80.0) * scale) as i32,
                        ((r.bottom - r.top - 20.0) * scale) as i32,
                        SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER,
                    );
                    let visible = IsWindowVisible(hwnd) != 0
                        && IsIconic(hwnd) == 0
                        && (scene.fixed_list()
                            || scene
                                .viewport
                                .as_ref()
                                .is_none_or(|v| r.top >= v.top && r.bottom <= v.bottom));
                    ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
                }
            }
        } else if !self.hwnd.is_null() {
            unsafe {
                ShowWindow(self.hwnd, SW_HIDE);
            }
        }
        true
    }
}
