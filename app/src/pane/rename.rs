//! Native inline EDIT with its own redirected surface above the composition pane.
#![allow(
    clippy::wildcard_imports,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
use super::GroupModel;
use luciddesk_core::ShellIdentity;
use std::{cell::RefCell, rc::Rc};
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::*,
    UI::{
        Controls::*,
        HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi},
        Input::KeyboardAndMouse::{SetFocus, VK_ESCAPE, VK_RETURN},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
const PROPERTY: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.RenameEdit");
const SUBCLASS: usize = 0x4c50_524e;
const FINISH: u32 = WM_APP + 71;

struct Name {
    text: String,
    suffix: String,
    selection: usize,
}
impl Name {
    fn new(identity: &ShellIdentity, label: &str) -> Self {
        let mut text = label.to_string();
        let mut suffix = String::new();
        let mut selection = text.encode_utf16().count();
        if let ShellIdentity::FileSystem { path, .. } = identity {
            if let Some(filename) = path.file_name() {
                text = filename.to_string_lossy().into_owned();
                if !path.is_dir()
                    && let Some(dot) = text.rfind('.').filter(|&i| i > 0)
                {
                    if text[..dot] == *label {
                        suffix = text[dot..].to_string();
                        text.truncate(dot);
                    }
                    selection = text[..dot.min(text.len())].encode_utf16().count();
                } else {
                    selection = text.encode_utf16().count();
                }
            }
        }
        Self {
            text,
            suffix,
            selection,
        }
    }
    fn committed(&self, value: &str) -> String {
        format!("{value}{}", self.suffix)
    }
}
struct Editor {
    owner: HWND,
    identity: ShellIdentity,
    model: Rc<RefCell<GroupModel>>,
    name: Name,
    font: HFONT,
    composing: bool,
    finishing: bool,
    title_target: Option<luciddesk_core::PanelId>,
    title_commit: Option<Box<dyn Fn(String) -> Result<(), String>>>,
    item_commit: Option<Rc<dyn Fn(&ShellIdentity, &str) -> Result<bool, String>>>,
    background: HBRUSH,
}
pub(super) fn show_title(
    owner: HWND,
    model: Rc<RefCell<GroupModel>>,
    commit: Box<dyn Fn(String) -> Result<(), String>>,
) -> Result<(), String> {
    show_tab_title(owner, model, None, commit)
}
pub(super) fn show_tab_title(
    owner: HWND, model: Rc<RefCell<GroupModel>>, target: Option<luciddesk_core::PanelId>,
    commit: Box<dyn Fn(String) -> Result<(), String>>,
) -> Result<(), String> {
    let title = {
        let m = model.borrow();
        target.and_then(|id| m.tabs.iter().find(|(tab, _)| *tab == id).map(|(_, title)| title.clone()))
            .unwrap_or_else(|| m.title.clone())
    };
    show_editor(owner, &ShellIdentity::Namespace { parsing_name: String::new() }, &title, model, Some(commit), None, target)
}
pub(super) fn active(owner: HWND) -> bool {
    unsafe { !GetPropW(owner, PROPERTY).is_null() }
}

pub(super) fn cancel(owner: HWND) {
    let edit = unsafe { GetPropW(owner, PROPERTY) };
    if !edit.is_null() { unsafe { DestroyWindow(edit); } }
}

#[cfg(test)]
pub(super) fn show(
    owner: HWND,
    identity: &ShellIdentity,
    label: &str,
    model: Rc<RefCell<GroupModel>>,
) -> Result<(), String> {
    show_editor(owner, identity, label, model, None, None, None)
}
pub(super) fn show_managed(
    owner: HWND, identity: &ShellIdentity, label: &str, model: Rc<RefCell<GroupModel>>,
    commit: Rc<dyn Fn(&ShellIdentity, &str) -> Result<bool, String>>,
) -> Result<(), String> {
    show_editor(owner, identity, label, model, None, Some(commit), None)
}
fn show_editor(
    owner: HWND,
    identity: &ShellIdentity,
    label: &str,
    model: Rc<RefCell<GroupModel>>,
    title_commit: Option<Box<dyn Fn(String) -> Result<(), String>>>,
    item_commit: Option<Rc<dyn Fn(&ShellIdentity, &str) -> Result<bool, String>>>,
    title_target: Option<luciddesk_core::PanelId>,
) -> Result<(), String> {
    unsafe {
        if active(owner) {
            SetFocus(GetPropW(owner, PROPERTY));
            return Ok(());
        }
        let dpi = GetDpiForWindow(owner).max(96);
        let mut logical_font = LOGFONTW::default();
        if SystemParametersInfoForDpi(
            SPI_GETICONTITLELOGFONT,
            size_of::<LOGFONTW>() as u32,
            (&raw mut logical_font).cast(),
            0,
            dpi,
        ) == 0
        {
            return Err(crate::i18n::text("ui-could-not-read-desktop-font").into());
        }
        super::assets::use_ui_font(&mut logical_font);
        let title = title_commit.is_some();
        let styled = title || model.borrow().is_list();
        if title && model.borrow().tabs.len() > 1 {
            // Match the 12 DIP tab label instead of the larger desktop icon font.
            logical_font.lfHeight = -(12.0 * dpi as f32 / 96.0).round() as i32;
            logical_font.lfWeight = 400;
        } else if title {
            logical_font.lfHeight -= (dpi as f32 / 96.0).round() as i32;
        } else if !title && !model.borrow().is_list() {
            logical_font.lfHeight = (logical_font.lfHeight as f32
                * model.borrow().options.grid_scale / 100.0).round() as i32;
        }
        let font = CreateFontIndirectW(&raw const logical_font);
        if font.is_null() {
            return Err(crate::i18n::text("ui-could-not-create-rename-font").into());
        }
        let name = Name::new(identity, label);
        let text = wide(&name.text);
        let edit = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            windows_sys::w!("EDIT"),
            text.as_ptr(),
            WS_POPUP
                | if styled { 0 } else { WS_BORDER }
                | WS_TABSTOP
                | if title {
                    ES_CENTER as u32 | ES_AUTOHSCROLL as u32
                } else if model.borrow().is_list() {
                    ES_LEFT as u32 | ES_AUTOHSCROLL as u32
                } else {
                    ES_CENTER as u32
                }
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32,
            0,
            0,
            1,
            1,
            owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        if edit.is_null() {
            DeleteObject(font);
            return Err(crate::i18n::text("ui-could-not-create-icon-name-editor").into());
        }
        let editor = Box::new(Editor {
            owner,
            identity: identity.clone(),
            model: model.clone(),
            name,
            font,
            composing: false,
            finishing: false,
            title_commit,
            title_target,
            item_commit,
            background: if styled {
                CreateSolidBrush(if model.borrow().dark {
                    0x002b2b2b
                } else {
                    0x00ffffff
                })
            } else {
                std::ptr::null_mut()
            },
        });
        let selection = editor.name.selection;
        let pointer = Box::into_raw(editor);
        if SetWindowSubclass(edit, Some(edit_proc), SUBCLASS, pointer as usize) == 0 {
            DeleteObject((*pointer).background);
            drop(Box::from_raw(pointer));
            DestroyWindow(edit);
            DeleteObject(font);
            return Err(crate::i18n::text("ui-could-not-attach-name-editor").into());
        }
        if SetPropW(owner, PROPERTY, edit) == 0
            || SetWindowSubclass(owner, Some(owner_proc), SUBCLASS, pointer as usize) == 0
        {
            DestroyWindow(edit);
            return Err(crate::i18n::text("ui-could-not-attach-group-editing-state").into());
        }
        if !title {
            model.borrow_mut().renaming = Some(identity.clone());
        }
        SendMessageW(edit, WM_SETFONT, font as usize, 0);
        SendMessageW(edit, EM_LIMITTEXT, 255, 0);
        resize(edit, pointer);
        ShowWindow(edit, SW_SHOW);
        InvalidateRect(owner, std::ptr::null(), 0);
        SetForegroundWindow(edit);
        SetFocus(edit);
        SendMessageW(
            edit,
            EM_SETSEL,
            0,
            isize::try_from(selection).unwrap_or(isize::MAX),
        );
        Ok(())
    }
}
fn text(edit: HWND) -> String {
    let mut value = [0u16; 256];
    let len = unsafe { GetWindowTextW(edit, value.as_mut_ptr(), 256) };
    String::from_utf16_lossy(&value[..len.max(0) as usize])
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

unsafe fn resize(edit: HWND, pointer: *mut Editor) {
    let editor = unsafe { &*pointer };
    let Ok(model) = editor.model.try_borrow() else {
        return;
    };
    if editor.title_commit.is_some() {
        unsafe {
            let scale = GetDpiForWindow(editor.owner).max(96) as f32 / 96.0;
            let mut client = RECT::default();
            GetClientRect(editor.owner, &raw mut client);
            let width = client.right as f32 / scale;
            let bounds = editor.title_target.and_then(|target| super::tabs::strip(&model, width).into_iter()
                .find(|(id, _)| *id == target).map(|(_, rect)| rect)).unwrap_or_else(|| title_bounds(&model, width));
            let mut origin = POINT {
                x: (bounds.x * scale).round() as i32,
                y: (bounds.y * scale).round() as i32,
            };
            ClientToScreen(editor.owner, &raw mut origin);
            let width = (bounds.width * scale).round().max(1.0) as i32;
            let height = (bounds.height * scale).round() as i32;
            let mut before = RECT::default();
            GetWindowRect(edit, &raw mut before);
            let size_changed =
                before.right - before.left != width || before.bottom - before.top != height;
            if size_changed {
                let region = title_region(width, height, scale, model.options.corner_radius);
                if !region.is_null() && SetWindowRgn(edit, region, 0) == 0 {
                    DeleteObject(region);
                }
            }
            if size_changed || before.left != origin.x || before.top != origin.y {
                SetWindowPos(
                    edit,
                    HWND_TOP,
                    origin.x,
                    origin.y,
                    width,
                    height,
                    SWP_NOACTIVATE,
                );
            }
            let dc = GetDC(edit);
            let old = SelectObject(dc, editor.font);
            let mut metrics = TEXTMETRICW::default();
            GetTextMetricsW(dc, &raw mut metrics);
            SelectObject(dc, old);
            ReleaseDC(edit, dc);
            let padding = (if model.tabs.len() > 1 { 10.0 } else { 6.0 } * scale).round() as i32;
            let bounds = RECT {
                left: padding,
                top: ((height - metrics.tmHeight) / 2).max(1),
                right: (width - padding).max(padding + 1),
                bottom: height - 1,
            };
            SendMessageW(edit, EM_SETRECT, 0, (&raw const bounds) as isize);
        }
        return;
    }
    let Some(index) = model
        .items
        .iter()
        .position(|item| item.identity == editor.identity)
    else {
        unsafe {
            PostMessageW(edit, FINISH, 0, 0);
        }
        return;
    };
    let mut client = RECT::default();
    unsafe {
        GetClientRect(editor.owner, &raw mut client);
    }
    let scale = unsafe { GetDpiForWindow(editor.owner) }.max(96) as f32 / 96.0;
    let grid = model.grid(client.right as f32 / scale, client.bottom as f32 / scale);
    let (x, y) = model.cell(grid, index);
    let list = model.is_list();
    let columns = model.list_columns(grid.cell_width);
    let (left, available, top) = if list {
        (x + columns[0], columns[1] - columns[0] - 8.0, y + 2.0)
    } else {
        (
            x,
            grid.cell_width,
            y + grid.icon_size + super::layout::LABEL_OFFSET - 2.0 / scale,
        )
    };
    let center = (left + available / 2.0) * scale;
    let top = (top * scale).round() as i32;
    let maximum = (available * scale).round() as i32;
    drop(model);
    unsafe {
        let dc = GetDC(edit);
        let old = SelectObject(dc, editor.font);
        let mut metrics = TEXTMETRICW::default();
        GetTextMetricsW(dc, &raw mut metrics);
        let mut value = wide(&text(edit));
        let mut bounds = RECT {
            right: (maximum - 8).max(12),
            bottom: 4096,
            ..Default::default()
        };
        DrawTextW(
            dc,
            value.as_mut_ptr(),
            -1,
            &raw mut bounds,
            DT_CALCRECT
                | (if list { DT_SINGLELINE } else { DT_WORDBREAK })
                | DT_EDITCONTROL
                | DT_NOPREFIX,
        );
        SelectObject(dc, old);
        ReleaseDC(edit, dc);
        let width = if list {
            maximum.max(24)
        } else {
            (bounds.right - bounds.left + 8).clamp(24, maximum.max(24))
        };
        let height = (bounds.bottom - bounds.top)
            .max(metrics.tmHeight)
            .min(metrics.tmHeight.max(1) * 6)
            + 4;
        let height = if list {
            ((super::layout::LIST_ROW - 4.0) * scale).round() as i32
        } else {
            height
        };
        let left =
            ((center - width as f32 / 2.0).round() as i32).clamp(0, (client.right - width).max(0));
        let mut before = RECT::default();
        GetWindowRect(edit, &raw mut before);
        if list {
            if before.right - before.left != width || before.bottom - before.top != height {
                let region = title_region(width, height, scale, 4.0);
                if !region.is_null() && SetWindowRgn(edit, region, 0) == 0 { DeleteObject(region); }
            }
        }
        // A child EDIT shares the owner's missing GDI redirection bitmap and
        // disappears underneath DirectComposition. An owned popup has its own
        // surface; its position must therefore be expressed in screen pixels.
        let mut origin = POINT { x: left, y: top };
        ClientToScreen(editor.owner, &raw mut origin);
        if (
            before.left,
            before.top,
            before.right - before.left,
            before.bottom - before.top,
        ) != (origin.x, origin.y, width, height)
        {
            SetWindowPos(
                edit,
                HWND_TOP,
                origin.x,
                origin.y,
                width,
                height,
                SWP_NOACTIVATE,
            );
        }
        if list {
            let padding = (6.0 * scale).round() as i32;
            let format = RECT { left: padding, top: ((height - metrics.tmHeight) / 2).max(1),
                right: (width - padding).max(padding + 1), bottom: height - 1 };
            SendMessageW(edit, EM_SETRECTNP, 0, (&raw const format) as isize);
        }
    }
}
fn title_bounds(model: &GroupModel, width: f32) -> luciddesk_core::RectDip {
    if let Some((_, rect)) = super::tabs::strip(model, width).into_iter().find(|(id, _)| *id == model.active_tab) {
        return rect;
    }
    let (left, width) = super::layout::title_area(width);
    let inset = super::layout::HEADER_INSET;
    luciddesk_core::RectDip {
        x: left - 6.0,
        y: inset,
        width: width + 12.0,
        height: super::layout::HEADER - inset * 2.0,
    }
}

fn title_corner_diameter(scale: f32, radius: f32) -> i32 {
    (radius * 2.0 * scale).round().max(0.0) as i32
}

unsafe fn title_region(width: i32, height: i32, scale: f32, radius: f32) -> HRGN {
    let diameter = title_corner_diameter(scale, radius);
    // The region rasterizer excludes the last right/bottom pixel that RoundRect
    // paints. Include that pixel so the focus border survives on all four sides.
    unsafe { CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter) }
}

unsafe fn finish(edit: HWND, pointer: *mut Editor, commit: bool) {
    if unsafe { (*pointer).finishing } {
        return;
    }
    unsafe {
        (*pointer).finishing = true;
    }
    let owner = unsafe { (*pointer).owner };
    if !commit {
        unsafe {
            DestroyWindow(edit);
        }
        return;
    }
    let value = text(edit);
    if value.trim().is_empty() {
        unsafe {
            (*pointer).finishing = false;
            SetFocus(edit);
        }
        return;
    }
    if unsafe { (*pointer).title_commit.is_some() } {
        let title = value.replace(['\r', '\n'], " ").trim().to_string();
        let result = unsafe { (*pointer).title_commit.as_ref().unwrap()(title) };
        unsafe {
            if let Err(error) = result {
                (*pointer).finishing = false;
                MessageBoxW(
                    edit,
                    wide(&error).as_ptr(),
                    crate::i18n::wide("ui-rename-failed"),
                    MB_OK | MB_ICONERROR,
                );
                SetFocus(edit);
            } else {
                DestroyWindow(edit);
            }
        }
        return;
    }
    let (identity, name, unchanged) = unsafe {
        (
            (*pointer).identity.clone(),
            (*pointer).name.committed(&value),
            value == (*pointer).name.text,
        )
    };
    // Retain the callback independently: Shell may close the editor while pumping.
    let item_commit = unsafe { (*pointer).item_commit.clone() };
    let result = if unchanged {
        Ok(true)
    } else if let Some(commit) = item_commit {
        commit(&identity, &name)
    } else {
        luciddesk_shell::rename_shell_identity(
            windows::Win32::Foundation::HWND(owner),
            &identity,
            &name,
        ).map_err(|error| error.to_string())
    };
    // Shell can pump messages, including closing the owning pane.
    if unsafe { GetPropW(owner, PROPERTY) } != edit {
        return;
    }
    match result {
        Ok(true) => unsafe {
            DestroyWindow(edit);
        },
        outcome => unsafe {
            if let Err(error) = outcome {
                MessageBoxW(
                    owner,
                    wide(&crate::i18n::format("ui-rename-failed-9e1f", &[("error", format!("{}", error))])).as_ptr(),
                    windows_sys::w!("LucidDesk"),
                    MB_OK | MB_ICONERROR,
                );
            }
            if GetPropW(owner, PROPERTY) == edit {
                (*pointer).finishing = false;
                SetFocus(edit);
            }
        },
    }
}
unsafe extern "system" fn edit_proc(
    edit: HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    _: usize,
    data: usize,
) -> isize {
    let pointer = data as *mut Editor;
    match msg {
        WM_PAINT if unsafe { !(*pointer).background.is_null() } => unsafe {
            let result = DefSubclassProc(edit, msg, wp, lp);
            let dc = GetDC(edit);
            let mut rect = RECT::default();
            GetClientRect(edit, &raw mut rect);
            let dark = (*pointer).model.borrow().dark;
            // Outline the actual clipped region. RoundRect and window regions
            // rasterize corner pixels differently, leaving dark corner seams.
            let region = CreateRectRgn(0, 0, 0, 0);
            let client = CreateRectRgn(0, 0, rect.right, rect.bottom);
            if GetWindowRgn(edit, region) != 0 {
                CombineRgn(region, region, client, RGN_AND);
                let border = CreateSolidBrush(if dark { 0x00787878 } else { 0x00909090 });
                FrameRgn(dc, region, border, 1, 1);
                DeleteObject(border);
            }
            DeleteObject(client);
            DeleteObject(region);
            ReleaseDC(edit, dc);
            return result;
        },
        WM_ERASEBKGND if unsafe { !(*pointer).background.is_null() } => unsafe {
            let mut rect = RECT::default();
            GetClientRect(edit, &raw mut rect);
            FillRect(wp as HDC, &rect, (*pointer).background);
            return 1;
        },
        WM_IME_STARTCOMPOSITION => unsafe {
            (*pointer).composing = true;
        },
        WM_IME_ENDCOMPOSITION => unsafe {
            (*pointer).composing = false;
        },
        WM_KEYDOWN if [usize::from(VK_RETURN), usize::from(VK_ESCAPE)].contains(&wp) => {
            if !unsafe { (*pointer).composing } {
                unsafe {
                    PostMessageW(edit, FINISH, usize::from(wp == usize::from(VK_RETURN)), 0);
                }
                return 0;
            }
        }
        WM_CHAR if wp == 13 || wp == 27 => return 0,
        WM_GETDLGCODE => return (DLGC_WANTALLKEYS | DLGC_WANTCHARS) as isize,
        WM_KILLFOCUS => {
            if !unsafe { (*pointer).finishing } {
                unsafe {
                    PostMessageW(edit, FINISH, 1, 0);
                }
            }
        }
        FINISH => {
            unsafe {
                finish(edit, pointer, wp != 0);
            }
            return 0;
        }
        WM_NCDESTROY => {
            let editor = unsafe { Box::from_raw(pointer) };
            unsafe {
                RemoveWindowSubclass(edit, Some(edit_proc), SUBCLASS);
                RemoveWindowSubclass(editor.owner, Some(owner_proc), SUBCLASS);
                RemovePropW(editor.owner, PROPERTY);
                if let Ok(mut model) = editor.model.try_borrow_mut() {
                    model.renaming = None;
                }
                let result = DefSubclassProc(edit, msg, wp, lp);
                DeleteObject(editor.font);
                DeleteObject(editor.background);
                InvalidateRect(editor.owner, std::ptr::null(), 0);
                PostMessageW(editor.owner, super::window::SYNC_POINTER, 0, 0);
                return result;
            }
        }
        _ => {}
    }
    unsafe { DefSubclassProc(edit, msg, wp, lp) }
}
unsafe extern "system" fn owner_proc(
    owner: HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    _: usize,
    data: usize,
) -> isize {
    let edit = unsafe { GetPropW(owner, PROPERTY) };
    if msg == WM_CTLCOLOREDIT && lp == edit as isize {
        let editor = unsafe { &*(data as *mut Editor) };
        if !editor.background.is_null() {
            let dark = editor.model.borrow().dark;
            unsafe {
                SetTextColor(wp as HDC, if dark { 0x00f4f4f4 } else { 0x00202020 });
                SetBkColor(wp as HDC, if dark { 0x002b2b2b } else { 0x00ffffff });
            }
            return editor.background as isize;
        }
    }
    let result = unsafe { DefSubclassProc(owner, msg, wp, lp) };
    if edit.is_null() || unsafe { GetPropW(owner, PROPERTY) } != edit {
        return result;
    }
    if [WM_PAINT, WM_SIZE, WM_MOVE, WM_DPICHANGED, WM_MOUSEWHEEL].contains(&msg)
        || (msg == WM_COMMAND && lp == edit as isize && (wp >> 16) == EN_CHANGE as usize)
    {
        unsafe {
            resize(edit, data as *mut Editor);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn title_clip_removes_all_four_background_corners_at_each_dpi() {
        unsafe {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let width = (200.0 * scale) as i32;
                let height = (28.0 * scale) as i32;
                let region = title_region(width, height, scale, 6.0);
                assert!(!region.is_null());
                for (x, y) in [
                    (0, 0),
                    (width - 1, 0),
                    (0, height - 1),
                    (width - 1, height - 1),
                ] {
                    assert_eq!(PtInRegion(region, x, y), 0);
                }
                assert_ne!(PtInRegion(region, width / 2, height / 2), 0);
                assert_ne!(PtInRegion(region, width / 2, 0), 0);
                assert_ne!(PtInRegion(region, width / 2, height - 1), 0);
                assert_ne!(PtInRegion(region, 0, height / 2), 0);
                assert_ne!(PtInRegion(region, width - 1, height / 2), 0);
                DeleteObject(region);
            }
        }
    }

    fn identity(name: &str) -> ShellIdentity {
        ShellIdentity::FileSystem {
            path: std::env::temp_dir().join(name),
            volume_id: None,
            file_id: None,
        }
    }
    #[test]
    fn hidden_extensions_are_preserved_and_visible_extensions_are_not_preselected() {
        let name = Name::new(&identity("网易云音乐.lnk"), "网易云音乐");
        assert_eq!(name.text, "网易云音乐");
        assert_eq!(name.selection, 5);
        assert_eq!(name.committed("音乐"), "音乐.lnk");
        let name = Name::new(&identity("报告.txt"), "报告.txt");
        assert_eq!(name.text, "报告.txt");
        assert_eq!(name.selection, 2);
        assert_eq!(name.committed("新报告.txt"), "新报告.txt");
    }
    #[test]
    fn inline_editor_tracks_label_and_escape_cleans_up_without_a_dialog() {
        let identity = identity("网易云音乐.lnk");
        let model = Rc::new(RefCell::new(GroupModel {
            merge_preview: Vec::new(),
        merge_occluded: false,
            tabs: Vec::new(),
            active_tab: luciddesk_core::PanelId::new(0),
            folder_sort: (0, false),
            folder_columns: None,
            folder_visible_columns: 15,
        folder_navigation: [false; 2],
            list_view: false,
            fixed_grid: false,
            free_layout: false,
            minimum_icon_width: 0.0,
            folder: None,
            folder_status: None,
            options: luciddesk_core::PaneOptions::default(),
            theme: luciddesk_core::PanelTheme::System,
            dark: true,

            hovered_item: None,
            scrollbar: Default::default(),
            focused: true,
            auto_hide: false,
            locked: false,
            reveal: 1.0,
            hovered_tab: None,
            hovered_button: None,
            pressed_button: None,
            backdrop: luciddesk_core::Backdrop::Acrylic,
            native_material: false,
            title: "测试".into(),
            items: vec![super::super::Item {
                details: Default::default(),
                identity: identity.clone(),
                label: "网易云音乐".into(),
                image: None,
            }],
            icon_size: 48.0,
            selected: Some(0),
            selection: [0].into_iter().collect(),
            selection_anchor: Some(0),
            renaming: None,
            scroll: 0,
            collapsed: false,
            loading: false,
        }));
        unsafe {
            let owner = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP,
                windows_sys::w!("STATIC"),
                windows_sys::w!("Inline rename fixture"),
                WS_POPUP,
                0,
                0,
                400,
                300,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!owner.is_null());
            show(owner, &identity, "网易云音乐", model.clone()).unwrap();
            let edit = GetPropW(owner, PROPERTY);
            assert_eq!(GetParent(edit), owner);
            assert_eq!(GetWindowLongW(edit, GWL_STYLE) as u32 & WS_CHILD, 0);
            assert_ne!(GetWindowLongW(edit, GWL_STYLE) as u32 & WS_POPUP, 0);
            assert_eq!(
                GetWindowLongW(edit, GWL_EXSTYLE) as u32 & WS_EX_NOREDIRECTIONBITMAP,
                0
            );
            assert_eq!(text(edit), "网易云音乐");
            assert_eq!(model.borrow().renaming.as_ref(), Some(&identity));
            let mut bounds = RECT::default();
            GetWindowRect(edit, &raw mut bounds);
            MapWindowPoints(std::ptr::null_mut(), owner, (&raw mut bounds).cast(), 2);
            let dpi = GetDpiForWindow(owner).max(96) as f32 / 96.0;
            assert_eq!(
                bounds.top,
                ((super::super::layout::HEADER
                    + super::super::layout::PADDING
                    + 48.0
                    + super::super::layout::LABEL_OFFSET)
                    * dpi)
                    .round() as i32
                    - 2
            );
            let mut start = 99u32;
            let mut end = 99u32;
            SendMessageW(
                edit,
                EM_GETSEL,
                (&raw mut start) as usize,
                (&raw mut end) as isize,
            );
            assert_eq!((start, end), (0, 5));
            let mut original = RECT::default();
            GetWindowRect(edit, &raw mut original);
            SetWindowPos(
                owner,
                std::ptr::null_mut(),
                137,
                81,
                400,
                300,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let mut moved = RECT::default();
            GetWindowRect(edit, &raw mut moved);
            assert_eq!(
                (moved.left - original.left, moved.top - original.top),
                (137, 81)
            );
            SetWindowTextW(edit, windows_sys::w!("取消后不能提交"));
            SendMessageW(edit, WM_KEYDOWN, VK_ESCAPE as usize, 0);
            let mut message = MSG::default();
            while PeekMessageW(&raw mut message, edit, FINISH, FINISH, PM_REMOVE) != 0 {
                DispatchMessageW(&raw const message);
            }
            assert!(!active(owner));
            assert!(model.borrow().renaming.is_none());
            assert_eq!(IsWindow(edit), 0);
            show(owner, &identity, "网易云音乐", model.clone()).unwrap();
            let edit = GetPropW(owner, PROPERTY);
            SendMessageW(edit, WM_IME_STARTCOMPOSITION, 0, 0);
            SendMessageW(edit, WM_KEYDOWN, VK_RETURN as usize, 0);
            assert_eq!(
                PeekMessageW(&raw mut message, edit, FINISH, FINISH, PM_REMOVE),
                0,
                "IME confirmation must not finish rename"
            );
            SendMessageW(edit, WM_IME_ENDCOMPOSITION, 0, 0);
            SendMessageW(edit, WM_KEYDOWN, VK_RETURN as usize, 0);
            while PeekMessageW(&raw mut message, edit, FINISH, FINISH, PM_REMOVE) != 0 {
                DispatchMessageW(&raw const message);
            }
            assert!(
                !active(owner),
                "Enter must finish unchanged names without a Shell operation"
            );
            let committed = Rc::new(RefCell::new(String::new()));
            let saved = committed.clone();
            show_title(
                owner,
                model.clone(),
                Box::new(move |title| {
                    *saved.borrow_mut() = title;
                    Ok(())
                }),
            )
            .unwrap();
            let edit = GetPropW(owner, PROPERTY);
            let mut bounds = RECT::default();
            GetWindowRect(edit, &raw mut bounds);
            MapWindowPoints(std::ptr::null_mut(), owner, (&raw mut bounds).cast(), 2);
            let mut owner_bounds = RECT::default();
            GetClientRect(owner, &raw mut owner_bounds);
            assert!((bounds.left + bounds.right - owner_bounds.right).abs() <= 1);
            assert_eq!(bounds.top, (super::super::layout::HEADER_INSET * dpi).round() as i32);
            let mut format = RECT::default();
            SendMessageW(edit, EM_GETRECT, 0, (&raw mut format) as isize);
            assert_eq!(format.left, (6.0 * dpi).round() as i32);
            assert!(format.top > 0, "title text has vertical padding");
            UpdateWindow(edit);
            let border_colors = || {
                let mut client = RECT::default();
                GetClientRect(edit, &raw mut client);
                let dc = GetDC(edit);
                let colors = [
                    GetPixel(dc, client.right / 2, 0),
                    GetPixel(dc, client.right / 2, client.bottom - 1),
                    GetPixel(dc, 0, client.bottom / 2),
                    GetPixel(dc, client.right - 1, client.bottom / 2),
                ];
                ReleaseDC(edit, dc);
                colors
            };
            let before = border_colors();
            assert_eq!(
                before, [0x00787878; 4],
                "all four focus borders are visible"
            );
            SendMessageW(owner, WM_PAINT, 0, 0);
            assert_eq!(
                border_colors(),
                before,
                "pane repaint must preserve the edit border"
            );
            SetWindowTextW(edit, windows_sys::w!("  工作分组  "));
            SendMessageW(edit, WM_IME_STARTCOMPOSITION, 0, 0);
            SendMessageW(edit, WM_KEYDOWN, VK_RETURN as usize, 0);
            assert_eq!(
                PeekMessageW(&raw mut message, edit, FINISH, FINISH, PM_REMOVE),
                0
            );
            SendMessageW(edit, WM_IME_ENDCOMPOSITION, 0, 0);
            SendMessageW(edit, FINISH, 1, 0);
            assert_eq!(&*committed.borrow(), "工作分组");
            assert!(!active(owner));
            {
                let mut m = model.borrow_mut();
                m.tabs = vec![(luciddesk_core::PanelId::new(1), "工作".into()), (luciddesk_core::PanelId::new(2), "资料".into())];
                m.active_tab = luciddesk_core::PanelId::new(2);
            }
            show_tab_title(
                owner,
                model.clone(),
                Some(luciddesk_core::PanelId::new(1)),
                Box::new(|_| panic!("cancel must not commit")),
            )
            .unwrap();
            let edit = GetPropW(owner, PROPERTY);
            assert_eq!(model.borrow().active_tab, luciddesk_core::PanelId::new(2));
            let expected = super::super::tabs::strip(&model.borrow(), owner_bounds.right as f32 / dpi).into_iter()
                .find(|(id, _)| *id == luciddesk_core::PanelId::new(1)).unwrap().1;
            GetWindowRect(edit, &raw mut bounds);
            MapWindowPoints(std::ptr::null_mut(), owner, (&raw mut bounds).cast(), 2);
            assert_eq!(bounds.left, (expected.x * dpi).round() as i32);
            assert_eq!(bounds.right - bounds.left, (expected.width * dpi).round() as i32);
            SendMessageW(edit, EM_GETRECT, 0, (&raw mut format) as isize);
            assert_eq!(format.left, (10.0 * dpi).round() as i32);
            assert_ne!(GetWindowLongW(edit, GWL_STYLE) as u32 & ES_CENTER as u32, 0);
            SendMessageW(GetPropW(owner, PROPERTY), FINISH, 0, 0);
            assert!(!active(owner));
            show(owner, &identity, "网易云音乐", model.clone()).unwrap();
            DestroyWindow(owner);
            assert!(
                model.borrow().renaming.is_none(),
                "Destroying the pane must release the editor"
            );
        }
    }
}
