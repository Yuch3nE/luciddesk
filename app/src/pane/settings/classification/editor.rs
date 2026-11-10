//! Native text inputs hosted by the settings window, positioned by its scene.
use super::*;
use windows_sys::Win32::UI::{
    Controls::EM_SETCUEBANNER,
    Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
};

const INPUT_ID: i32 = 0x4c60;
const INPUT_SUBCLASS: usize = 0x4c60;
const DARK: isize = 1;
const COMPOSING: isize = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::pane::settings) enum Field {
    Extensions,
    Keyword,
}

impl Field {
    const ALL: [Self; 2] = [Self::Extensions, Self::Keyword];

    pub(super) fn id(self) -> i32 {
        INPUT_ID + self as i32
    }

    fn label_key(self) -> &'static str {
        match self {
            Self::Extensions => "classification-extensions",
            Self::Keyword => "classification-keyword",
        }
    }

    fn from_id(id: i32) -> Option<Self> {
        usize::try_from(id - INPUT_ID)
            .ok()
            .and_then(|index| Self::ALL.get(index))
            .copied()
    }
}

pub(super) struct Draft {
    pub index: Option<usize>,
    pub original: String,
    pub regex: bool,
    pub rule: ClassificationRule,
    pub inputs: Inputs,
}

impl Draft {
    pub fn edited_rule(&self) -> ClassificationRule {
        let extensions = self.inputs.text(Field::Extensions);
        let keyword = self.inputs.text(Field::Keyword);
        let trimmed = keyword.trim();
        let (name_contains, name_regex) = if self.regex {
            (None, (!trimmed.is_empty()).then_some(keyword))
        } else {
            ((!trimmed.is_empty()).then(|| trimmed.to_owned()), None)
        };
        ClassificationRule {
            extensions: extensions
                .split([',', ';', '，', '；', ' '])
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect(),
            name_contains,
            name_regex,
            ..self.rule.clone()
        }
    }
}

pub(super) struct Inputs {
    handles: [HWND; 2],
    positions: [Option<[i32; 4]>; 2],
    clips: [Option<[i32; 4]>; 2],
    requested_focus: Option<Field>,
    font: Option<font_search::SearchFont>,
    font_style: (String, i32),
    language: &'static str,
}

impl Inputs {
    pub fn new(owner: HWND, rule: &ClassificationRule) -> Result<Self, String> {
        let mut inputs = Self {
            handles: [std::ptr::null_mut(); 2],
            positions: [None; 2],
            clips: [None; 2],
            requested_focus: None,
            font: None,
            font_style: (String::new(), 0),
            language: "",
        };
        for (field, value) in Field::ALL.into_iter().zip([
            rule.extensions.join(", "),
            rule.name_regex
                .as_ref()
                .or(rule.name_contains.as_ref())
                .cloned()
                .unwrap_or_default(),
        ]) {
            let value = wide(&value);
            unsafe {
                let hwnd = CreateWindowExW(
                    WS_EX_LAYERED,
                    windows_sys::w!("EDIT"),
                    value.as_ptr(),
                    WS_CHILD | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                    0,
                    0,
                    1,
                    1,
                    owner,
                    field.id() as _,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                );
                if hwnd.is_null() {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                inputs.handles[field as usize] = hwnd;
                // The settings parent has no GDI redirection bitmap. Give each
                // native child its own composited surface so its text remains
                // visible above the DirectComposition backdrop.
                if SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA) == 0 {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                if SetWindowSubclass(hwnd, Some(input_proc), INPUT_SUBCLASS, 0) == 0 {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                SendMessageW(
                    hwnd,
                    WM_SETFONT,
                    GetStockObject(DEFAULT_GUI_FONT) as usize,
                    0,
                );
            }
        }
        Ok(inputs)
    }

    pub fn field_for(&self, hwnd: HWND) -> Option<Field> {
        Field::ALL
            .into_iter()
            .find(|field| self.handles[*field as usize] == hwnd)
    }

    fn text(&self, field: Field) -> String {
        let hwnd = self.handles[field as usize];
        let mut text = vec![0; unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize + 1];
        let len = unsafe { GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32) };
        String::from_utf16_lossy(&text[..len.max(0) as usize])
    }

    pub fn focus(&mut self, field: Field) {
        self.requested_focus = Some(field);
    }

    pub fn hide(&mut self) {
        self.requested_focus = None;
        for (index, hwnd) in self.handles.iter().enumerate() {
            unsafe { ShowWindow(*hwnd, SW_HIDE); }
            self.positions[index] = None;
        }
    }

    pub fn sync(&mut self, scene: &Scene, scale: f32, dark: bool) {
        let language = crate::i18n::language();
        if self.language != language {
            for field in Field::ALL {
                unsafe {
                    SendMessageW(
                        self.handles[field as usize],
                        EM_SETCUEBANNER,
                        0,
                        crate::i18n::wide(field.label_key()) as isize,
                    );
                }
            }
            self.language = language;
        }
        let style = (fonts::family(), (Style::TEXT_SIZE * scale).round() as i32);
        if self.font_style != style {
            let family = wide(&style.0);
            unsafe {
                let font = CreateFontW(
                    -style.1,
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
                    family.as_ptr(),
                );
                if !font.is_null() {
                    for hwnd in self.handles {
                        SendMessageW(hwnd, WM_SETFONT, font as usize, 1);
                    }
                    self.font = Some(font_search::SearchFont(font));
                    self.font_style = style;
                }
            }
        }
        for field in Field::ALL {
            let index = field as usize;
            let hwnd = self.handles[index];
            let control = scene.controls.iter().find(|control| {
                matches!(control.action, Action::Classification(Command::Input(value)) if value == field)
            });
            unsafe {
                let Some(bounds) = control.map(|control| control.bounds).filter(|bounds| {
                    scene
                        .viewport
                        .is_none_or(|v| bounds.bottom > v.top && bounds.top < v.bottom)
                }) else {
                    if self.positions[index].take().is_some() {
                        ShowWindow(hwnd, SW_HIDE);
                    }
                    continue;
                };
                let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
                let styled = (state & !DARK) | isize::from(dark);
                if state != styled {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, styled);
                    InvalidateRect(hwnd, std::ptr::null(), 1);
                }
                let padding = Tokens::GAP;
                let line_height = (Style::TEXT_SIZE * 1.4 * scale).ceil() as i32;
                let position = [
                    ((bounds.left + padding) * scale).round() as i32,
                    ((bounds.top + bounds.bottom) * scale / 2.0).round() as i32 - line_height / 2,
                    ((bounds.right - bounds.left - 2.0 * padding) * scale)
                        .round()
                        .max(1.0) as i32,
                    line_height,
                ];
                let clip = input_clip(position, scene.viewport, scale);
                if clip[0] >= clip[2] || clip[1] >= clip[3] {
                    ShowWindow(hwnd, SW_HIDE);
                    self.positions[index] = None;
                    continue;
                }
                if self.clips[index] != Some(clip) {
                    let region = CreateRectRgn(clip[0], clip[1], clip[2], clip[3]);
                    if !region.is_null() {
                        if SetWindowRgn(hwnd, region, 1) == 0 {
                            DeleteObject(region);
                        } else {
                            self.clips[index] = Some(clip);
                        }
                    }
                }
                if self.positions[index] != Some(position) {
                    let [x, y, width, height] = position;
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        x,
                        y,
                        width,
                        height,
                        SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                    self.positions[index] = Some(position);
                }
                if self.requested_focus == Some(field) {
                    SetFocus(hwnd);
                    self.requested_focus = None;
                }
            }
        }
    }
}

fn input_clip([x, y, width, height]: [i32; 4], viewport: Option<Rect>, scale: f32) -> [i32; 4] {
    viewport.map_or([0, 0, width, height], |v| [
        ((v.left * scale).ceil() as i32 - x).clamp(0, width),
        ((v.top * scale).ceil() as i32 - y).clamp(0, height),
        ((v.right * scale).floor() as i32 - x).clamp(0, width),
        ((v.bottom * scale).floor() as i32 - y).clamp(0, height),
    ])
}

#[test]
fn input_clipping_preserves_visible_text_at_both_edges() {
    let viewport = Some(Rect::from_xywh(0.0, 20.0, 200.0, 80.0));
    assert_eq!(input_clip([10, 15, 100, 24], viewport, 1.0), [0, 5, 100, 24]);
    assert_eq!(input_clip([10, 90, 100, 24], viewport, 1.0), [0, 0, 100, 10]);
    assert_eq!(input_clip([10, 30, 100, 24], viewport, 1.0), [0, 0, 100, 24]);
    assert_eq!(input_clip([10, 110, 100, 24], viewport, 1.0), [0, 0, 100, 0]);
}

impl Drop for Inputs {
    fn drop(&mut self) {
        for hwnd in self.handles {
            if !hwnd.is_null() {
                unsafe {
                    DestroyWindow(hwnd);
                }
            }
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

pub(in crate::pane::settings) fn input_colors(dc: HDC, hwnd: HWND) -> Option<isize> {
    unsafe {
        Field::from_id(GetDlgCtrlID(hwnd))?;
        let dark = GetWindowLongPtrW(hwnd, GWLP_USERDATA) & DARK != 0;
        let palette = components::Palette::for_theme(dark);
        let colorref = |rgb: u32| ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff);
        SetTextColor(dc, colorref(palette.ink));
        SetBkColor(dc, colorref(palette.background));
        SetDCBrushColor(dc, colorref(palette.background));
        Some(GetStockObject(DC_BRUSH) as isize)
    }
}

unsafe extern "system" fn input_proc(
    hwnd: HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    id: usize,
    _: usize,
) -> isize {
    unsafe {
        let owner = GetParent(hwnd);
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        match msg {
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, Some(input_proc), id);
            }
            WM_IME_STARTCOMPOSITION => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, state | COMPOSING);
            }
            WM_IME_ENDCOMPOSITION => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, state & !COMPOSING);
            }
            WM_SETFOCUS | WM_KILLFOCUS => {
                InvalidateRect(owner, std::ptr::null(), 0);
            }
            WM_MOUSEWHEEL => {
                PostMessageW(owner, msg, wp, lp);
                return 0;
            }
            WM_KEYDOWN
                if state & COMPOSING == 0
                    && matches!(wp as u16, VK_TAB | VK_RETURN | VK_ESCAPE) =>
            {
                let shift_tab = wp == VK_TAB as usize && GetKeyState(VK_SHIFT as i32) < 0;
                let key = if shift_tab { VK_UP as usize } else { wp };
                PostMessageW(owner, CLASSIFICATION_KEY, key, hwnd as isize);
                return 0;
            }
            WM_CHAR if matches!(wp as u16, VK_TAB | VK_RETURN | VK_ESCAPE) => return 0,
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}
