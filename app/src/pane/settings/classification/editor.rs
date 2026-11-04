use super::*;
use windows_sys::Win32::Foundation::HWND;

fn wide(value: &str) -> Vec<u16> { value.encode_utf16().chain(Some(0)).collect() }
fn text(hwnd: HWND) -> String {
    let mut value = vec![0; unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize + 1];
    let n = unsafe { GetWindowTextW(hwnd, value.as_mut_ptr(), value.len() as i32) };
    String::from_utf16_lossy(&value[..n.max(0) as usize])
}

pub(super) fn show(owner: HWND, panels: &[Panel], initial: ClassificationRule) -> Result<Option<ClassificationRule>, String> {
    let done = Rc::new(std::cell::Cell::new(false));
    let result = Rc::new(RefCell::new(None));
    let finished = done.clone();
    let output = result.clone();
    let targets: Vec<_> = panels.iter().map(Panel::id).collect();
    let enabled = initial.enabled;
    let window = windows_window::Window::new(t("classification-edit"))
        .size(540, 350).style(WS_CAPTION | WS_SYSMENU).ex_style(WS_EX_DLGMODALFRAME)
        .on_message(move |raw, msg, wp, _| {
            let hwnd: HWND = raw.cast();
            if msg == WM_CLOSE { finished.set(true); return Some(0); }
            if msg == WM_COMMAND {
                match wp & 0xffff {
                    2 => { finished.set(true); return Some(0); }
                    1 => {
                        let target = unsafe { SendMessageW(GetDlgItem(hwnd, 101), CB_GETCURSEL, 0, 0) };
                        let Some(target) = usize::try_from(target).ok().and_then(|i| targets.get(i)) else { window::error_for(hwnd, t("classification-no-target")); return Some(0); };
                        let extensions = text(unsafe { GetDlgItem(hwnd, 102) }).split([',', ';', '，', '；', ' ']).filter(|s| !s.trim().is_empty()).map(|s| s.trim().to_owned()).collect();
                        let name = text(unsafe { GetDlgItem(hwnd, 103) }).trim().to_owned();
                        let rule = ClassificationRule { target_panel: target.get(), enabled, extensions, name_contains: (!name.is_empty()).then_some(name) };
                        if let Err(error) = (Classification { enabled: true, rules: vec![rule.clone()] }).validate() { window::error_for(hwnd, &error); return Some(0); }
                        *output.borrow_mut() = Some(rule); finished.set(true); return Some(0);
                    }
                    _ => {}
                }
            }
            None
        }).create().map_err(|e| e.to_string())?;
    let hwnd: HWND = window.hwnd().cast();
    let scale = unsafe { GetDpiForWindow(owner) }.max(96) as f32 / 96.0;
    let child = |class: &str, label: &str, style: u32, id: usize, y: i32, height: i32| -> Result<HWND, String> {
        let handle = unsafe { CreateWindowExW(0, wide(class).as_ptr(), wide(label).as_ptr(), WS_CHILD | WS_VISIBLE | style,
            (20.0 * scale) as i32, (y as f32 * scale) as i32, (480.0 * scale) as i32, (height as f32 * scale) as i32,
            hwnd, id as _, std::ptr::null_mut(), std::ptr::null()) };
        if handle.is_null() { return Err(std::io::Error::last_os_error().to_string()); }
        unsafe { SendMessageW(handle, WM_SETFONT, GetStockObject(DEFAULT_GUI_FONT) as usize, 1); }
        Ok(handle)
    };
    unsafe { SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner as isize); }
    child("STATIC", t("classification-target"), 0, 0, 12, 22)?;
    let target = child("COMBOBOX", "", WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL, 101, 36, 180)?;
    for panel in panels { unsafe { SendMessageW(target, CB_ADDSTRING, 0, wide(panel.title()).as_ptr() as isize); } }
    let selection = panels.iter().position(|p| p.id().get() == initial.target_panel);
    unsafe { SendMessageW(target, CB_SETCURSEL, selection.unwrap_or(usize::MAX), 0); }
    child("STATIC", t("classification-extensions-hint"), 0, 0, 76, 22)?;
    child("EDIT", &initial.extensions.join(", "), WS_TABSTOP | WS_BORDER | ES_AUTOHSCROLL as u32, 102, 100, 26)?;
    child("STATIC", t("classification-name"), 0, 0, 142, 22)?;
    child("EDIT", initial.name_contains.as_deref().unwrap_or(""), WS_TABSTOP | WS_BORDER | ES_AUTOHSCROLL as u32, 103, 166, 26)?;
    let save = child("BUTTON", t("classification-save"), WS_TABSTOP | BS_DEFPUSHBUTTON as u32, 1, 216, 30)?;
    let cancel = child("BUTTON", t("classification-cancel"), WS_TABSTOP, 2, 258, 30)?;
    let mut rect = RECT::default();
    unsafe {
        GetWindowRect(owner, &raw mut rect);
        SetWindowPos(hwnd, std::ptr::null_mut(), rect.left + 40, rect.top + 40, (540.0 * scale) as i32, (350.0 * scale) as i32, SWP_NOZORDER);
        EnableWindow(owner, 0); ShowWindow(hwnd, SW_SHOW); SetFocus(target);
    }
    let mut message = MSG::default();
    while !done.get() {
        let status = unsafe { GetMessageW(&raw mut message, std::ptr::null_mut(), 0, 0) };
        if status <= 0 { if status == 0 { unsafe { PostQuitMessage(message.wParam as i32); } } break; }
        if message.message == WM_KEYDOWN && message.wParam == VK_ESCAPE as usize { done.set(true); continue; }
        if message.message == WM_KEYDOWN && message.wParam == VK_RETURN as usize {
            let button = if unsafe { GetFocus() } == cancel { 2 } else { 1 };
            unsafe { SendMessageW(hwnd, WM_COMMAND, button, save as isize); } continue;
        }
        if unsafe { IsDialogMessageW(hwnd, &raw mut message) } == 0 { unsafe { TranslateMessage(&message); DispatchMessageW(&message); } }
    }
    unsafe { EnableWindow(owner, 1); SetForegroundWindow(owner); }
    Ok(result.borrow_mut().take())
}
