//! Canonical Shell verbs preserve Explorer clipboard formats and file dialogs.
use luciddesk_core::ShellIdentity;
use windows::{
    Win32::{
        Foundation::HWND,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{HSTRING, PCSTR, PSTR, Result},
};

// Only an explicit Windows cancellation is benign. Do not hide generic E_ABORT,
// permission failures or an extension's unknown error as user cancellation.
pub(crate) fn command_completed(result: Result<()>) -> Result<bool> {
    match result {
        Ok(()) => Ok(true),
        Err(error) if error.code() == windows::core::HRESULT::from_win32(
            windows::Win32::Foundation::ERROR_CANCELLED.0,
        ) => Ok(false),
        Err(error) => Err(error),
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
    fn verb(self) -> &'static [u8] {
        match self {
            Self::Copy => b"copy\0",
            Self::Cut => b"cut\0",
            Self::Paste => b"paste\0",
            Self::Delete => b"delete\0",
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

/// Starts a native file drag; Explorer determines destination copy/move semantics.
/// # Errors
/// Returns errors creating the Shell data object or starting the drag loop.
pub fn drag_file_items(
    owner: HWND,
    selected: &[ShellIdentity],
    image: Option<&crate::FileDragImage>,
) -> Result<()> {
    use windows::Win32::System::{Com::IDataObject, Ole::*};
    if selected.is_empty() {
        return Ok(());
    }
    unsafe {
        let data: IDataObject = shell_items(selected)?.BindToHandler(None, &BHID_DataObject)?;
        // A preview failure must not prevent the underlying file operation.
        let _image_helper = image.and_then(|image| image.initialize(&data).ok());
        SHDoDragDrop(
            Some(owner),
            &data,
            None,
            DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK,
        )?;
    }
    Ok(())
}

pub(super) struct MenuMessages {
    pub(super) context: IContextMenu,
    pub(super) owner: HWND,
}
impl Drop for MenuMessages {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::UI::Shell::RemoveWindowSubclass(
                self.owner.0,
                Some(menu_messages),
                0x4c50464d,
            );
        }
    }
}
pub(super) unsafe extern "system" fn menu_messages(
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
        let state = &*(data as *const MenuMessages);
        if matches!(
            msg,
            WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR
        ) {
            if let Ok(context) = state.context.cast::<IContextMenu3>() {
                let mut result = windows::Win32::Foundation::LRESULT::default();
                if context
                    .HandleMenuMsg2(msg, WPARAM(wp), LPARAM(lp), Some(&raw mut result))
                    .is_ok()
                {
                    return result.0;
                }
            } else if let Ok(context) = state.context.cast::<IContextMenu2>() {
                if context.HandleMenuMsg(msg, WPARAM(wp), LPARAM(lp)).is_ok() {
                    return 0;
                }
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
    point: windows::Win32::Foundation::POINT,
) -> Result<bool> {
    if selected.is_empty() {
        return Ok(false);
    }
    luciddesk_menu::apply_theme(owner.0);
    unsafe {
        let context: IContextMenu = shell_items(selected)?.BindToHandler(None, &BHID_SFUIObject)?;
        let menu = Menu(CreatePopupMenu()?);
        context
            .QueryContextMenu(menu.0, 0, 1, 0x7fff, CMF_NORMAL | CMF_CANRENAME)
            .ok()?;
        add_location_command(menu.0, selected)?;
        let messages = Box::new(MenuMessages {
            context: context.clone(),
            owner,
        });
        if windows_sys::Win32::UI::Shell::SetWindowSubclass(
            owner.0,
            Some(menu_messages),
            0x4c50464d,
            (&raw const *messages) as usize,
        ) == 0
        {
            return Err(windows::core::Error::from_thread());
        }
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
        drop(frame);
        drop(messages);
        if chosen == 0 {
            return Ok(false);
        }
        if chosen as usize == OPEN_LOCATION {
            open_location(&selected[0])?;
            return Ok(false);
        }
        let offset = chosen as usize - 1;
        let mut verb = [0u8; 256];
        let _ = context.GetCommandString(
            offset,
            GCS_VERBA,
            None,
            PSTR(verb.as_mut_ptr()),
            verb.len() as u32,
        );
        if selected.len() == 1 && verb.starts_with(b"rename\0") {
            return Ok(true);
        }
        let command = CMINVOKECOMMANDINFO {
            cbSize: size_of::<CMINVOKECOMMANDINFO>() as u32,
            hwnd: owner,
            lpVerb: PCSTR(offset as *const u8),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        command_completed(context.InvokeCommand(&command))?;
        Ok(false)
    }
}

// Outside the range assigned to Shell extensions by QueryContextMenu.
const OPEN_LOCATION: usize = 0x8000;

fn add_location_command(menu: HMENU, selected: &[ShellIdentity]) -> Result<()> {
    if selected.len() == 1
        && selected[0].file_system_path().and_then(std::path::Path::parent).is_some()
    {
        unsafe {
            InsertMenuW(menu, 1, MF_BYPOSITION | MF_STRING, OPEN_LOCATION,
                windows::core::w!("打开所在文件夹"))?;
            InsertMenuW(menu, 2, MF_BYPOSITION | MF_SEPARATOR, 0, None)?;
        }
    }
    Ok(())
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
    let name = HSTRING::from(identity.activation_name().to_string_lossy().as_ref());
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
            CLSCTX_INPROC_SERVER, CoCreateInstance, DVASPECT_CONTENT, FORMATETC, IDataObject,
            STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
        },
        DataExchange::RegisterClipboardFormatW,
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        Ole::{
            DROPEFFECT_COPY, DROPEFFECT_MOVE, OleFlushClipboard, OleSetClipboard, ReleaseStgMedium,
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
            if !command_completed(operation.PerformOperations())? { return Ok(false); }
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
        OleFlushClipboard()?;
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
        for offset in 0..(result.0 as u32 & 0xffff) {
            let mut verb = [0u8; 256];
            if context
                .GetCommandString(
                    offset as usize,
                    GCS_VERBA,
                    None,
                    PSTR(verb.as_mut_ptr()),
                    verb.len() as u32,
                )
                .is_err()
                || !verb.starts_with(command.verb())
            {
                continue;
            }
            let flags = GetMenuState(menu.0, offset + 1, MF_BYCOMMAND);
            if flags == u32::MAX || flags & (MF_DISABLED.0 | MF_GRAYED.0) != 0 {
                return Ok(false);
            }
            // No shift/permanent-delete or no-confirmation flags are supplied.
            let info = CMINVOKECOMMANDINFO {
                cbSize: size_of::<CMINVOKECOMMANDINFO>() as u32,
                hwnd: owner,
                lpVerb: PCSTR(offset as usize as *const u8),
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };
            return command_completed(context.InvokeCommand(&info));
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn explicit_cancellation_is_not_an_error_but_failures_keep_their_hresult() {
        use windows::Win32::Foundation::{ERROR_CANCELLED, E_ACCESSDENIED, E_ABORT};
        use windows::core::{Error, HRESULT};
        assert!(super::command_completed(Ok(())).unwrap());
        assert!(!super::command_completed(Err(Error::from_hresult(HRESULT::from_win32(ERROR_CANCELLED.0)))).unwrap());
        for code in [E_ACCESSDENIED, E_ABORT] {
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
            add_location_command(menu.0, std::slice::from_ref(&item)).unwrap();
            assert_eq!(GetMenuItemCount(Some(menu.0)), 3);
            assert_eq!(GetMenuItemID(menu.0, 0), 1);
            assert_eq!(GetMenuItemID(menu.0, 1), OPEN_LOCATION as u32);
            for items in [vec![], vec![item.clone(), item], vec![ShellIdentity::Namespace { parsing_name: "virtual".into() }]] {
                let menu = Menu(CreatePopupMenu().unwrap());
                add_location_command(menu.0, &items).unwrap();
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
