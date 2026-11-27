//! Canonical Shell verbs preserve Explorer clipboard formats and file dialogs.
use luciddesk_core::ShellIdentity;
use std::{cell::RefCell, rc::Rc};
use windows::{
    Win32::{
        Foundation::{HWND, POINT, S_OK},
        System::{Com::IDataObject, Ole::OleFlushClipboard},
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{HSTRING, Interface, PCSTR, PCWSTR, PSTR, Result},
};

thread_local! {
    static FILE_CLIPBOARD: RefCell<Option<IDataObject>> = const { RefCell::new(None) };
}

fn is_current_file_clipboard(data: &IDataObject) -> bool {
    // S_FALSE also succeeds as an HRESULT, but means ownership has changed.
    let code = unsafe { windows_sys::Win32::System::Ole::OleIsCurrentClipboard(data.as_raw()) };
    code == S_OK.0
}

/// Persists the current file clipboard, if owned by this STA, before OLE teardown.
pub fn flush_file_clipboard() {
    let data = FILE_CLIPBOARD.take();
    if let Some(data) = data
        && is_current_file_clipboard(&data)
    {
        let _ = unsafe { OleFlushClipboard() };
    }
}

// Only an explicit Windows cancellation is benign. Do not hide generic E_ABORT,
// permission failures or an extension's unknown error as user cancellation.
pub(crate) fn command_completed(result: Result<()>) -> Result<bool> {
    let win32_cancelled =
        windows::core::HRESULT::from_win32(windows::Win32::Foundation::ERROR_CANCELLED.0);
    match result {
        Ok(()) => Ok(true),
        Err(error) if error.code() == COPYENGINE_E_USER_CANCELLED || error.code() == win32_cancelled => {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

// PerformOperations may fail or be cancelled after recycling part of a batch.
// Reconcile the namespace at completion, independently of panel/icon visibility,
// and preserve the original operation result if this best-effort update fails.
fn recycling_completed(result: Result<()>, refresh: impl FnOnce() -> Result<()>) -> Result<bool> {
    if let Err(error) = refresh() {
        luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Debug,
            "shell.file-operation", &format!("Recycle namespace reconciliation failed: {error}"));
    }
    command_completed(result)
}

fn refresh_recycle_namespace() -> Result<()> {
    unsafe {
        let item: IShellItem = SHGetKnownFolderItem(&FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, None)?;
        let view: IShellView = item.BindToHandler(None, &BHID_SFViewObject)?;
        view.Refresh()
    }
}

fn file_command_completed(command: FileCommand, result: Result<()>) -> Result<bool> {
    if command == FileCommand::Delete {
        recycling_completed(result, refresh_recycle_namespace)
    } else {
        command_completed(result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileCommand {
    Copy,
    Cut,
    Paste,
    Delete,
}

impl FileCommand {
    fn verb(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Cut => "cut",
            Self::Paste => "paste",
            Self::Delete => "delete",
        }
    }
}

/// Copies filesystem items using Windows conflict and progress dialogs.
/// # Errors
/// Returns a Shell error if sources, destination or copy operation fail.
pub fn copy_to_folder(
    owner: HWND,
    selected: &[ShellIdentity],
    destination: &std::path::Path,
) -> Result<()> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    if selected.is_empty() {
        return Ok(());
    }
    unsafe {
        let target: IShellItem =
            SHCreateItemFromParsingName(&HSTRING::from(destination.as_os_str()), None)?;
        let operation: IFileOperation =
            CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
        operation.SetOwnerWindow(owner)?;
        operation.SetOperationFlags(FOF_ALLOWUNDO)?;
        operation.CopyItems(&shell_items(selected)?, &target)?;
        command_completed(operation.PerformOperations())?;
    }
    Ok(())
}

/// Pastes into this exact folder; missing folders never fall back to Desktop.
/// # Errors
/// Returns errors opening the destination or invoking the clipboard command.
pub fn paste_into_folder(owner: HWND, destination: &std::path::Path) -> Result<bool> {
    unsafe {
        let item: IShellItem =
            SHCreateItemFromParsingName(&HSTRING::from(destination.as_os_str()), None)?;
        paste_into_item(owner, &item)
    }
}

// Background menus can omit Paste from GetCommandString; invoke its canonical verb.
fn paste_into_item(owner: HWND, item: &IShellItem) -> Result<bool> {
    unsafe {
        let folder: IShellFolder = item.BindToHandler(None, &BHID_SFObject)?;
        let context: IContextMenu = folder.CreateViewObject(owner)?;
        let menu = Menu(CreatePopupMenu()?);
        context
            .QueryContextMenu(menu.0, 0, 1, 0x7fff, CMF_NORMAL)
            .ok()?;
        let info = CMINVOKECOMMANDINFO {
            cbSize: size_of::<CMINVOKECOMMANDINFO>() as u32,
            hwnd: owner,
            lpVerb: PCSTR(b"paste\0".as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        command_completed(context.InvokeCommand(&info))
    }
}

/// The Shell data object and drag loop for an outgoing file drop.
pub struct FileDrag(windows::Win32::System::Com::IDataObject);

impl FileDrag {
    /// Creates a native Shell data object from the selected items.
    /// # Errors
    /// Returns an error if the Shell cannot bind the selection as a data object.
    pub fn new(selected: &[ShellIdentity]) -> Result<Self> {
        unsafe { shell_items(selected)?.BindToHandler(None, &BHID_DataObject).map(Self) }
    }

    #[must_use]
    pub fn data(&self) -> &windows::Win32::System::Com::IDataObject {
        &self.0
    }

    /// Runs the native drag loop with the Shell's default drop source.
    /// # Errors
    /// Returns an error if the Shell cannot run the drag.
    pub fn run(&self, owner: HWND, image: Option<&crate::FileDragImage>) -> Result<()> {
        use windows::Win32::System::Ole::*;
        // A preview failure must not prevent the underlying file operation.
        let _image_helper = image.and_then(|image| image.initialize(self.data()).ok());
        unsafe {
            SHDoDragDrop(
                Some(owner),
                self.data(),
                None,
                DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK,
            )?;
        }
        Ok(())
    }
}

/// Starts a native file drag; Explorer determines destination copy/move semantics.
/// # Errors
/// Returns errors creating the Shell data object or starting the drag loop.
pub fn drag_file_items(
    owner: HWND,
    selected: &[ShellIdentity],
    image: Option<&crate::FileDragImage>,
) -> Result<()> {
    if selected.is_empty() {
        return Ok(());
    }
    FileDrag::new(selected)?.run(owner, image)
}

struct MenuMessageState {
    context: IContextMenu,
}

pub(super) struct MenuMessages {
    state: Rc<MenuMessageState>,
    owner: HWND,
}

impl MenuMessages {
    const SUBCLASS_ID: usize = 0x4c50464d;

    pub(super) fn install(owner: HWND, context: &IContextMenu) -> Result<Self> {
        let state = Rc::new(MenuMessageState {
            context: context.clone(),
        });
        // The subclass owns a strong reference until it is detached.
        let registered = Rc::into_raw(Rc::clone(&state));
        if unsafe {
            windows_sys::Win32::UI::Shell::SetWindowSubclass(
                owner.0,
                Some(menu_messages),
                Self::SUBCLASS_ID,
                registered as usize,
            )
        } == 0
        {
            let error = windows::core::Error::from_thread();
            unsafe { drop(Rc::from_raw(registered)) };
            return Err(error);
        }
        Ok(Self { state, owner })
    }
}

impl Drop for MenuMessages {
    fn drop(&mut self) {
        unsafe {
            use windows_sys::Win32::UI::Shell::GetWindowSubclass;
            let registered = Rc::as_ptr(&self.state);
            let mut data = 0;
            if GetWindowSubclass(
                self.owner.0,
                Some(menu_messages),
                Self::SUBCLASS_ID,
                &raw mut data,
            ) != 0 && data == registered as usize
            {
                detach_menu_messages(self.owner.0, data as *const MenuMessageState);
            }
        }
    }
}

unsafe fn detach_menu_messages(
    hwnd: windows_sys::Win32::Foundation::HWND,
    registered: *const MenuMessageState,
) {
    unsafe {
        if windows_sys::Win32::UI::Shell::RemoveWindowSubclass(
            hwnd,
            Some(menu_messages),
            MenuMessages::SUBCLASS_ID,
        ) != 0
        {
            drop(Rc::from_raw(registered));
        }
    }
}

unsafe extern "system" fn menu_messages(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    _id: usize,
    data: usize,
) -> isize {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::core::Interface;
    unsafe {
        let registered = data as *const MenuMessageState;
        Rc::increment_strong_count(registered);
        let state = Rc::from_raw(registered);
        if msg == WM_NCDESTROY {
            detach_menu_messages(hwnd, registered);
        }
        let owner_draw = matches!(msg, WM_DRAWITEM | WM_MEASUREITEM);
        if matches!(
            msg,
            WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR
        ) && (!owner_draw || (wp == 0 && lp != 0))
        {
            if let Ok(context) = state.context.cast::<IContextMenu3>() {
                let mut result = windows::Win32::Foundation::LRESULT::default();
                if context
                    .HandleMenuMsg2(msg, WPARAM(wp), LPARAM(lp), Some(&raw mut result))
                    .is_ok()
                {
                    return if owner_draw { 1 } else { result.0 };
                }
            } else if msg != WM_MENUCHAR
                && let Ok(context) = state.context.cast::<IContextMenu2>()
                && context.HandleMenuMsg(msg, WPARAM(wp), LPARAM(lp)).is_ok()
            {
                return isize::from(owner_draw);
            }
        }
        windows_sys::Win32::UI::Shell::DefSubclassProc(hwnd, msg, wp, lp)
    }
}

/// Shows a filesystem context menu; true requests inline rename.
/// # Errors
/// Returns errors creating the Shell menu or invoking its selected command.
pub fn show_file_items_menu(
    owner: HWND,
    selected: &[ShellIdentity],
    point: POINT,
    open_location_label: &str,
) -> Result<bool> {
    if selected.is_empty() {
        return Ok(false);
    }
    let _active = crate::native_menu::ActiveMenu::acquire()?;
    let theme = luciddesk_menu::apply_scoped_theme(owner.0);
    unsafe {
        let context: IContextMenu = shell_items(selected)?.BindToHandler(None, &BHID_SFUIObject)?;
        let menu = Menu(CreatePopupMenu()?);
        context
            .QueryContextMenu(menu.0, 0, 1, 0x7fff, CMF_NORMAL | CMF_CANRENAME)
            .ok()?;
        add_location_command(menu.0, selected, open_location_label)?;
        let messages = MenuMessages::install(owner, &context)?;
        let frame = luciddesk_menu::MenuFrame::install(owner.0, windows_sys::Win32::Foundation::POINT { x: point.x, y: point.y });
        let chosen = TrackPopupMenuEx(
            menu.0,
            (TPM_RETURNCMD | TPM_RIGHTBUTTON).0,
            point.x,
            point.y,
            owner,
            None,
        )
        .0;
        let offset = chosen.saturating_sub(1).max(0) as usize;
        let command = menu_invocation(owner, offset, Some(point));
        drop(frame);
        drop(messages);
        drop(theme);
        if chosen == 0 {
            return Ok(false);
        }
        if chosen as usize == OPEN_LOCATION {
            open_location(&selected[0])?;
            return Ok(false);
        }
        let verb = canonical_verb(&context, offset);
        if selected.len() == 1
            && verb.as_deref().is_some_and(|verb| verb.eq_ignore_ascii_case("rename"))
        {
            return Ok(true);
        }
        let result = context.InvokeCommand((&raw const command).cast());
        if verb.as_deref().is_some_and(|verb| verb.eq_ignore_ascii_case("delete")) {
            file_command_completed(FileCommand::Delete, result)?;
        } else {
            command_completed(result)?;
        }
        Ok(false)
    }
}

pub(super) fn menu_invocation(
    owner: HWND,
    offset: usize,
    point: Option<POINT>,
) -> CMINVOKECOMMANDINFOEX {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};
    let mut flags = SEE_MASK_UNICODE;
    if point.is_some() {
        flags |= CMIC_MASK_PTINVOKE;
        if unsafe { GetKeyState(i32::from(VK_SHIFT)) } < 0 {
            flags |= CMIC_MASK_SHIFT_DOWN;
        }
        if unsafe { GetKeyState(i32::from(VK_CONTROL)) } < 0 {
            flags |= CMIC_MASK_CONTROL_DOWN;
        }
    }
    CMINVOKECOMMANDINFOEX {
        cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
        fMask: flags,
        hwnd: owner,
        lpVerb: PCSTR(offset as *const u8),
        lpVerbW: PCWSTR(offset as *const u16),
        nShow: SW_SHOWNORMAL.0,
        ptInvoke: point.unwrap_or_default(),
        ..Default::default()
    }
}

fn canonical_verb(context: &IContextMenu, offset: usize) -> Option<String> {
    let mut wide = [0u16; 256];
    if unsafe {
        context.GetCommandString(
            offset, GCS_VERBW, None, PSTR(wide.as_mut_ptr().cast()), wide.len() as u32,
        )
    }.is_ok()
        && let Some(end) = wide.iter().position(|&unit| unit == 0).filter(|&end| end != 0)
        && let Ok(verb) = String::from_utf16(&wide[..end])
    {
        return Some(verb);
    }

    let mut ansi = [0u8; 256];
    unsafe {
        context.GetCommandString(offset, GCS_VERBA, None, PSTR(ansi.as_mut_ptr()), ansi.len() as u32)
    }.ok()?;
    let end = ansi.iter().position(|&byte| byte == 0).filter(|&end| end != 0)?;
    // The canonical verbs we recognize contain only ASCII.
    std::str::from_utf8(&ansi[..end]).ok().map(str::to_owned)
}

// Outside the range assigned to Shell extensions by QueryContextMenu.
const OPEN_LOCATION: usize = 0x8000;

fn add_location_command(menu: HMENU, selected: &[ShellIdentity], label: &str) -> Result<()> {
    if selected.len() == 1
        && selected[0].file_system_path().and_then(std::path::Path::parent).is_some()
    {
        unsafe {
            let label = HSTRING::from(label);
            InsertMenuW(menu, 1, MF_BYPOSITION | MF_STRING, OPEN_LOCATION, &label)?;
            InsertMenuW(menu, 2, MF_BYPOSITION | MF_SEPARATOR, 0, None)?;
        }
    }
    Ok(())
}

fn find_menu_command(
    menu: HMENU,
    last_id: u32,
    matches: &mut impl FnMut(usize) -> bool,
) -> Result<Option<(usize, bool)>> {
    let count = unsafe { GetMenuItemCount(Some(menu)) };
    if count < 0 {
        return Err(windows::core::Error::from_thread());
    }
    for position in 0..count as u32 {
        let mut item = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE | MIIM_ID | MIIM_STATE | MIIM_SUBMENU,
            ..Default::default()
        };
        unsafe { GetMenuItemInfoW(menu, position, true, &raw mut item) }?;
        if item.fType & MFT_SEPARATOR != MENU_ITEM_TYPE(0) {
            continue;
        }
        let enabled = item.fState & (MFS_DISABLED | MFS_GRAYED) == MENU_ITEM_STATE(0);
        if !item.hSubMenu.is_invalid() {
            if let Some((offset, child_enabled)) = find_menu_command(item.hSubMenu, last_id, matches)? {
                return Ok(Some((offset, enabled && child_enabled)));
            }
        } else if (1..=last_id).contains(&item.wID) {
            let offset = item.wID as usize - 1;
            if matches(offset) {
                return Ok(Some((offset, enabled)));
            }
        }
    }
    Ok(None)
}

fn open_location(identity: &ShellIdentity) -> Result<()> {
    // Passing the item's full PIDL with no child array opens its parent and
    // selects the item, including a folder or shortcut itself (not its target).
    unsafe {
        let item = shell_item(identity)?;
        let pidl = SHGetIDListFromObject(&item)?;
        let result = SHOpenFolderAndSelectItems(pidl, None, 0);
        windows::Win32::System::Com::CoTaskMemFree(Some(pidl.cast()));
        result
    }
}

pub(super) struct Menu(pub(super) HMENU);
impl Drop for Menu {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.0);
        }
    }
}

fn shell_item(identity: &ShellIdentity) -> Result<IShellItem> {
    let name = HSTRING::from(identity.activation_name());
    unsafe { SHCreateItemFromParsingName(&name, None) }
}

fn shell_items(selected: &[ShellIdentity]) -> Result<IShellItemArray> {
    struct Pidl(*mut Common::ITEMIDLIST);
    impl Drop for Pidl {
        fn drop(&mut self) {
            unsafe {
                windows::Win32::System::Com::CoTaskMemFree(Some(self.0.cast()));
            }
        }
    }
    unsafe {
        let pidls: Vec<_> = selected
            .iter()
            .map(|identity| {
                let item = shell_item(identity)?;
                SHGetIDListFromObject(&item).map(Pidl)
            })
            .collect::<Result<_>>()?;
        let pointers: Vec<_> = pidls.iter().map(|pidl| pidl.0.cast_const()).collect();
        SHCreateShellItemArrayFromIDLists(&pointers)
    }
}

fn invoke_batch(owner: HWND, selected: &[ShellIdentity], command: FileCommand) -> Result<bool> {
    use windows::Win32::System::{
        Com::{
            CLSCTX_INPROC_SERVER, CoCreateInstance, DVASPECT_CONTENT, FORMATETC,
            STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
        },
        DataExchange::RegisterClipboardFormatW,
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        Ole::{
            DROPEFFECT_COPY, DROPEFFECT_MOVE, OleSetClipboard, ReleaseStgMedium,
        },
    };
    unsafe {
        let items = shell_items(selected)?;
        if command == FileCommand::Delete {
            let operation: IFileOperation =
                CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
            operation.SetOwnerWindow(owner)?;
            operation
                .SetOperationFlags(FOF_ALLOWUNDO | FOF_WANTNUKEWARNING | FOFX_RECYCLEONDELETE)?;
            operation.DeleteItems(&items)?;
            if !file_command_completed(command, operation.PerformOperations())? { return Ok(false); }
            return Ok(!operation.GetAnyOperationsAborted()?.as_bool());
        }
        let data: IDataObject = items.BindToHandler(None, &BHID_DataObject)?;
        let format_id = RegisterClipboardFormatW(CFSTR_PREFERREDDROPEFFECT);
        if format_id == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let memory = GlobalAlloc(GMEM_MOVEABLE, size_of::<u32>())?;
        let mut medium = STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: memory },
            ..Default::default()
        };
        let pointer = GlobalLock(memory).cast::<u32>();
        if pointer.is_null() {
            let error = windows::core::Error::from_thread();
            ReleaseStgMedium(&mut medium);
            return Err(error);
        }
        pointer.write(if command == FileCommand::Cut {
            DROPEFFECT_MOVE.0
        } else {
            DROPEFFECT_COPY.0
        });
        let _ = GlobalUnlock(memory);
        let format = FORMATETC {
            cfFormat: format_id as u16,
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
            ..Default::default()
        };
        if let Err(error) = data.SetData(&format, &medium, true) {
            ReleaseStgMedium(&mut medium);
            return Err(error);
        }
        OleSetClipboard(&data)?;
        // A nested command may have replaced the clipboard while OLE pumped messages.
        if is_current_file_clipboard(&data) {
            let previous = FILE_CLIPBOARD.replace(Some(data));
            drop(previous);
        }
        Ok(true)
    }
}

/// Executes one Shell command for the entire selection, preserving a single
/// clipboard data object and the Shell's batch confirmation/conflict handling.
/// Paste uses a directory only when exactly one directory is selected, otherwise Desktop.
/// Call on the OLE STA without app/model borrows; Shell can pump window messages.
/// # Errors
/// Returns errors from Shell target resolution or command execution.
pub fn invoke_file_commands(
    owner: HWND,
    selected: &[ShellIdentity],
    command: FileCommand,
) -> Result<bool> {
    unsafe {
        if command == FileCommand::Paste {
            let directory = selected.first().filter(|identity| {
                selected.len() == 1
                    && matches!(identity, ShellIdentity::FileSystem { path, .. } if path.is_dir())
            });
            let item = if let Some(identity) = directory {
                shell_item(identity)?
            } else {
                SHGetKnownFolderItem::<IShellItem>(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None)?
            };
            return paste_into_item(owner, &item);
        }
        if selected.len() > 1 {
            return invoke_batch(owner, selected, command);
        }
        let Some(identity) = selected.first() else {
            return Ok(false);
        };
        let context: IContextMenu = shell_item(identity)?.BindToHandler(None, &BHID_SFUIObject)?;
        let menu = Menu(CreatePopupMenu()?);
        let result = context.QueryContextMenu(menu.0, 0, 1, 0x7fff, CMF_NORMAL);
        result.ok()?;
        let Some((offset, true)) = find_menu_command(menu.0, result.0 as u32 & 0xffff, &mut |offset| {
            canonical_verb(&context, offset)
                .is_some_and(|verb| verb.eq_ignore_ascii_case(command.verb()))
        })? else {
            return Ok(false);
        };
        // Keyboard/application commands retain their ordinary Shell behavior.
        let info = menu_invocation(owner, offset, None);
        file_command_completed(command, context.InvokeCommand((&raw const info).cast()))
    }
}

#[cfg(test)]
mod tests {
    #[windows::core::implement(IContextMenu)]
    struct VerbProvider(bool);

    impl IContextMenu_Impl for VerbProvider_Impl {
        fn QueryContextMenu(&self, _: HMENU, _: u32, _: u32, _: u32, _: u32) -> windows::core::HRESULT {
            S_OK
        }

        fn InvokeCommand(&self, _: *const CMINVOKECOMMANDINFO) -> Result<()> {
            Ok(())
        }

        fn GetCommandString(&self, _: usize, kind: u32, _: *const u32, text: PSTR, max: u32) -> Result<()> {
            if kind == GCS_VERBW && self.0 {
                let verb: Vec<u16> = "ReNaMe\0".encode_utf16().collect();
                assert!(max as usize >= verb.len());
                unsafe { std::ptr::copy_nonoverlapping(verb.as_ptr(), text.as_ptr().cast(), verb.len()) };
            } else if kind == GCS_VERBA {
                let verb = b"DeLeTe\0";
                assert!(max as usize >= verb.len());
                unsafe { std::ptr::copy_nonoverlapping(verb.as_ptr(), text.as_ptr(), verb.len()) };
            } else {
                return Err(windows::Win32::Foundation::E_NOTIMPL.into());
            }
            Ok(())
        }
    }

    #[test]
    fn canonical_verbs_prefer_unicode_and_fall_back_to_ansi() {
        let unicode: IContextMenu = VerbProvider(true).into();
        let ansi: IContextMenu = VerbProvider(false).into();
        assert!(canonical_verb(&unicode, 0).unwrap().eq_ignore_ascii_case("rename"));
        assert!(canonical_verb(&ansi, 0).unwrap().eq_ignore_ascii_case("delete"));
    }

    #[test]
    fn shortcut_lookup_only_queries_real_menu_commands() {
        use super::*;

        unsafe {
            let menu = Menu(CreatePopupMenu().unwrap());
            let label = windows::core::w!("Item");
            AppendMenuW(menu.0, MF_STRING, 2, label).unwrap();
            AppendMenuW(menu.0, MF_SEPARATOR, 0, None).unwrap();
            AppendMenuW(menu.0, MF_STRING | MF_GRAYED, 9, label).unwrap();
            AppendMenuW(menu.0, MF_STRING, 14, label).unwrap();

            let mut queried = Vec::new();
            let command = find_menu_command(menu.0, 9, &mut |offset| {
                queried.push(offset);
                offset == 8
            }).unwrap();
            assert_eq!(command, Some((8, false)));
            assert_eq!(queried, [1, 8]);
        }
    }

    #[test]
    fn recycling_completion_reconciles_partial_failures_without_masking_results() {
        use std::cell::Cell;
        use windows::{core::{Error, HRESULT}, Win32::Foundation::{E_ACCESSDENIED, E_FAIL, ERROR_CANCELLED}};
        let calls = Cell::new(0);
        let refresh = || {
            calls.set(calls.get() + 1);
            Err(Error::from_hresult(E_FAIL))
        };
        assert!(super::recycling_completed(Ok(()), refresh).unwrap());
        assert!(!super::recycling_completed(Err(Error::from_hresult(HRESULT::from_win32(ERROR_CANCELLED.0))), refresh).unwrap());
        assert_eq!(super::recycling_completed(Err(Error::from_hresult(E_ACCESSDENIED)), refresh).unwrap_err().code(), E_ACCESSDENIED);
        assert_eq!(calls.get(), 3, "a partial/cancelled batch may already have recycled items");
    }

    #[test]
    #[ignore = "Reconciles the live Recycle Bin namespace without changing its contents"]
    fn live_recycling_completion_without_a_panel() {
        let _sta = crate::ShellApartment::initialize_sta().unwrap();
        assert!(super::recycling_completed(Ok(()), super::refresh_recycle_namespace).unwrap());
    }

    #[test]
    fn explicit_cancellation_is_not_an_error_but_failures_keep_their_hresult() {
        use windows::Win32::Foundation::{ERROR_CANCELLED, E_ACCESSDENIED, E_ABORT};
        use windows::core::{Error, HRESULT};
        assert!(super::command_completed(Ok(())).unwrap());
        for code in [
            HRESULT::from_win32(ERROR_CANCELLED.0),
            super::COPYENGINE_E_USER_CANCELLED,
        ] {
            assert!(!super::command_completed(Err(Error::from_hresult(code))).unwrap());
        }
        for code in [E_ACCESSDENIED, E_ABORT, super::COPYENGINE_E_CANCELLED] {
            assert_eq!(super::command_completed(Err(Error::from_hresult(code))).unwrap_err().code(), code);
        }
    }
    #[test]
    fn drag_image_preserves_native_file_data() {
        use super::*;
        use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL};
        let _apartment = crate::ShellApartment::initialize_sta().unwrap();
        let selected = [ShellIdentity::FileSystem {
            path: std::env::current_exe().unwrap(), volume_id: None, file_id: None,
        }];
        unsafe {
            let data: IDataObject = shell_items(&selected).unwrap().BindToHandler(None, &BHID_DataObject).unwrap();
            let file_format = FORMATETC {
                cfFormat: 15, // CF_HDROP
                dwAspect: DVASPECT_CONTENT.0, lindex: -1,
                tymed: TYMED_HGLOBAL.0 as u32, ..Default::default()
            };
            data.QueryGetData(&file_format).ok().unwrap();
            let image = crate::FileDragImage {
                width: 2, height: 2, pixels: [0, 0, 255, 255].repeat(4),
                hotspot: windows::Win32::Foundation::POINT { x: 1, y: 1 },
            };
            let _helper = image.initialize(&data).unwrap();
            data.QueryGetData(&file_format).ok().expect("Custom image must preserve file-drop formats");
            assert_eq!(crate::drag_shell_identities(&data).unwrap(), selected);
        }
    }

    #[test]
    fn location_command_preserves_shell_ids_and_requires_a_single_filesystem_item() {
        use super::*;
        let item = ShellIdentity::FileSystem {
            path: r"C:\Folder\file.txt".into(), volume_id: None, file_id: None,
        };
        unsafe {
            let menu = Menu(CreatePopupMenu().unwrap());
            AppendMenuW(menu.0, MF_STRING, 1, windows::core::w!("Shell command")).unwrap();
            add_location_command(menu.0, std::slice::from_ref(&item), "Open file location").unwrap();
            assert_eq!(GetMenuItemCount(Some(menu.0)), 3);
            assert_eq!(GetMenuItemID(menu.0, 0), 1);
            assert_eq!(GetMenuItemID(menu.0, 1), OPEN_LOCATION as u32);
            for items in [vec![], vec![item.clone(), item], vec![ShellIdentity::Namespace { parsing_name: "virtual".into() }]] {
                let menu = Menu(CreatePopupMenu().unwrap());
                add_location_command(menu.0, &items, "Open file location").unwrap();
                assert_eq!(GetMenuItemCount(Some(menu.0)), 0);
            }
        }
    }

    use super::*;

    #[test]
    fn folder_item_menu_copy_and_missing_paste_destination() {
        let _apartment = crate::ShellApartment::initialize_sta().unwrap();
        let root = std::env::temp_dir().join(format!(
            "luciddesk-folder-shell-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let destination = root.join("destination");
        std::fs::create_dir(&destination).unwrap();
        let paths = [root.join("one.txt"), root.join("two.txt")];
        for path in &paths {
            std::fs::write(path, b"folder copy").unwrap();
        }
        let items: Vec<_> = paths
            .iter()
            .map(|path| ShellIdentity::FileSystem {
                path: path.clone(),
                volume_id: None,
                file_id: None,
            })
            .collect();
        unsafe {
            let context: IContextMenu = shell_items(&items)
                .unwrap()
                .BindToHandler(None, &BHID_SFUIObject)
                .unwrap();
            let menu = Menu(CreatePopupMenu().unwrap());
            context
                .QueryContextMenu(menu.0, 0, 1, 0x7fff, CMF_NORMAL)
                .ok()
                .unwrap();
        }
        copy_to_folder(HWND::default(), &items, &destination).unwrap();
        for path in &paths {
            assert_eq!(std::fs::read(path).unwrap(), b"folder copy");
            let copied = destination.join(path.file_name().unwrap());
            assert_eq!(std::fs::read(&copied).unwrap(), b"folder copy");
            std::fs::remove_file(copied).unwrap();
            std::fs::remove_file(path).unwrap();
        }
        assert!(paste_into_folder(HWND::default(), &root.join("missing")).is_err());
        std::fs::remove_dir(destination).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    #[ignore = "Uses the interactive Shell clipboard; restores its previous contents"]
    fn shell_copy_and_cut_paste_preserve_file_contents() {
        use windows::Win32::System::{
            Com::IDataObject,
            Ole::{OleFlushClipboard, OleGetClipboard, OleSetClipboard},
        };
        let _apartment = crate::ShellApartment::initialize_sta().unwrap();
        struct Clipboard(Option<IDataObject>);
        impl Drop for Clipboard {
            fn drop(&mut self) {
                unsafe {
                    if OleSetClipboard(self.0.as_ref()).is_ok() && self.0.is_some() {
                        let _ = OleFlushClipboard();
                    }
                }
            }
        }
        let _clipboard = Clipboard(unsafe { OleGetClipboard().ok() });
        let root = std::env::temp_dir().join(format!(
            "luciddesk-clipboard-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let copied = root.join("copied");
        let moved = root.join("moved");
        std::fs::create_dir_all(&copied).unwrap();
        std::fs::create_dir_all(&moved).unwrap();
        let source = root.join("fixture.txt");
        std::fs::write(&source, b"clipboard regression").unwrap();
        let identity = |path| ShellIdentity::FileSystem {
            path,
            volume_id: None,
            file_id: None,
        };
        for (command, from, to) in [
            (FileCommand::Copy, source.clone(), copied.clone()),
            (FileCommand::Cut, copied.join("fixture.txt"), moved.clone()),
        ] {
            assert!(
                invoke_file_commands(HWND::default(), &[identity(from.clone())], command)
                    .unwrap()
            );
            assert!(
                invoke_file_commands(
                    HWND::default(),
                    &[identity(to.clone())],
                    FileCommand::Paste
                )
                .unwrap()
            );
            let destination = to.join("fixture.txt");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !destination.exists() && std::time::Instant::now() < deadline {
                unsafe {
                    let mut message = MSG::default();
                    while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                        DispatchMessageW(&message);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(std::fs::read(destination).unwrap(), b"clipboard regression");
            assert_eq!(from.exists(), command == FileCommand::Copy);
        }
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        let sources = [root.join("batch-a.txt"), nested.join("batch-b.txt")];
        for path in &sources {
            std::fs::write(path, b"batch clipboard").unwrap();
        }
        for (command, paths, target) in [
            (FileCommand::Copy, sources.to_vec(), copied.clone()),
            (
                FileCommand::Cut,
                vec![copied.join("batch-a.txt"), copied.join("batch-b.txt")],
                moved.clone(),
            ),
        ] {
            let selected: Vec<_> = paths.iter().cloned().map(&identity).collect();
            assert!(invoke_file_commands(HWND::default(), &selected, command).unwrap());
            assert!(
                invoke_file_commands(
                    HWND::default(),
                    &[identity(target.clone())],
                    FileCommand::Paste
                )
                .unwrap()
            );
            for path in &paths {
                let destination = target.join(path.file_name().unwrap());
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while !destination.exists() && std::time::Instant::now() < deadline {
                    unsafe {
                        let mut message = MSG::default();
                        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                            DispatchMessageW(&message);
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                assert_eq!(std::fs::read(destination).unwrap(), b"batch clipboard");
                assert_eq!(path.exists(), command == FileCommand::Copy);
            }
        }
        drop(_clipboard);
        std::fs::remove_dir_all(root).unwrap();
    }
}
