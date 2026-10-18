//! Shell namespace enumeration, display names, and stable file identity.
use crate::{ShellError, ShellIdentity};
use std::os::windows::{ffi::OsStringExt, fs::OpenOptionsExt, io::AsRawHandle};
use std::{
    ffi::c_void,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    ptr,
    time::SystemTime,
};
use windows::Win32::{
    Foundation::HWND as WindowsHwnd,
    System::{
        Com::CoTaskMemFree as CoTaskMemFreeCom,
        SystemServices::{
            SFGAO_CANDELETE, SFGAO_CANRENAME, SFGAO_FILESYSTEM, SFGAO_FOLDER, SFGAO_HIDDEN,
            SFGAO_LINK,
        },
    },
    UI::Shell::{
        IShellFolder, IShellItem, SHCONTF_FOLDERS, SHCONTF_NONFOLDERS, SHCreateItemWithParent,
        SHGetDesktopFolder, SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY,
    },
};
use windows_sys::Win32::{
    Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FileIdInfo, GetFileInformationByHandleEx,
    },
    System::Com::CoTaskMemFree,
    UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ShellAttributes {
    pub folder: bool,
    pub file_system: bool,
    pub link: bool,
    pub hidden: bool,
    pub can_rename: bool,
    pub can_delete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellEntry {
    pub identity: ShellIdentity,
    pub display_name: String,
    pub attributes: ShellAttributes,
    pub modified: Option<SystemTime>,
    /// File length from the directory snapshot; no recursive folder sizing.
    pub size: Option<u64>,
}

/// Resolves LocalAppData without assuming its physical location.
///
/// # Errors
///
/// Returns an error when Windows cannot resolve the requested folder.
pub fn local_app_data_path() -> Result<PathBuf, ShellError> {
    let mut raw_path = ptr::null_mut();
    let result = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_LocalAppData,
            0,
            ptr::null_mut(),
            &raw mut raw_path,
        )
    };
    if result < 0 {
        return Err(ShellError::Windows(result));
    }

    let path = unsafe {
        let mut length = 0;
        while *raw_path.add(length) != 0 {
            length += 1;
        }
        let value = std::ffi::OsString::from_wide(std::slice::from_raw_parts(raw_path, length));
        CoTaskMemFree(raw_path.cast::<c_void>());
        PathBuf::from(value)
    };
    Ok(path)
}

/// Enumerates the Windows Desktop Shell Namespace, including virtual Shell items.
///
/// Unlike reading the user and public desktop directories separately, this follows the
/// Shell's unified Desktop Folder and can return items such as Recycle Bin or This PC.
/// The calling thread must own a [`crate::ShellApartment`].
///
/// # Errors
///
/// Returns a COM or Shell error when the Desktop Folder cannot be enumerated.
pub fn enumerate_desktop_namespace(owner: isize) -> Result<Vec<ShellEntry>, ShellError> {
    let desktop = unsafe { SHGetDesktopFolder() }?;
    enumerate_shell_folder(&desktop, owner, false)
}

/// Reads the source independently of the filtered Explorer view, including hidden
/// items. Callers must restrict this to their existing managed membership.
/// # Errors
/// Any unresolved item aborts the capture so it cannot look like a deletion.
pub fn enumerate_desktop_source() -> Result<Vec<ShellEntry>, ShellError> {
    let desktop = unsafe { SHGetDesktopFolder() }?;
    enumerate_shell_folder(&desktop, 0, true)
}

/// Lightweight source revision, independent of view filtering. No per-item file
/// identity or icon loading is performed.
/// # Errors
/// Fails if source enumeration cannot be completed.
pub fn desktop_source_revision() -> Result<Vec<Vec<u8>>, ShellError> {
    unsafe {
        let desktop = SHGetDesktopFolder()?;
        let mut enumerator = None;
        desktop
            .EnumObjects(
                WindowsHwnd::default(),
                (SHCONTF_FOLDERS.0
                    | SHCONTF_NONFOLDERS.0
                    | windows::Win32::UI::Shell::SHCONTF_INCLUDEHIDDEN.0
                    | windows::Win32::UI::Shell::SHCONTF_INCLUDESUPERHIDDEN.0)
                    as u32,
                &raw mut enumerator,
            )
            .ok()?;
        let mut ids = Vec::new();
        if let Some(enumerator) = enumerator {
            loop {
                let mut child = [ptr::null_mut()];
                let mut fetched = 0;
                enumerator.Next(&mut child, Some(&raw mut fetched)).ok()?;
                if fetched == 0 {
                    break;
                }
                let child = Pidl::new(child[0]);
                let length = windows::Win32::UI::Shell::ILGetSize(Some(child.as_ptr())) as usize;
                ids.push(std::slice::from_raw_parts(child.as_ptr().cast(), length).to_vec());
            }
        }
        ids.sort();
        Ok(ids)
    }
}

/// Enumerates every direct filesystem child, including hidden and system entries.
/// # Errors
/// Returns an I/O error if the folder cannot be opened or enumerated.
pub fn enumerate_folder(path: &Path) -> Result<Vec<ShellEntry>, ShellError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{Foundation::{INVALID_HANDLE_VALUE, ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_FILES, GetLastError}, Storage::FileSystem::*};
    // Find data contains type/hidden attributes and timestamps in the same read.
    // No per-child metadata fallback can turn an unknown directory into a file.
    let pattern: Vec<_> = path.join("*").as_os_str().encode_wide().chain(Some(0)).collect();
    let mut data = WIN32_FIND_DATAW::default();
    let handle = unsafe { FindFirstFileW(pattern.as_ptr(), &raw mut data) };
    if handle == INVALID_HANDLE_VALUE {
        let code = unsafe { GetLastError() };
        if code == ERROR_FILE_NOT_FOUND && path.is_dir() { return Ok(Vec::new()); }
        return Err(std::io::Error::from_raw_os_error(code as i32).into());
    }
    struct Search(windows_sys::Win32::Foundation::HANDLE);
    impl Drop for Search { fn drop(&mut self) { unsafe { FindClose(self.0); } } }
    let _search = Search(handle);
    let mut items = Vec::new();
    loop {
        let end = data.cFileName.iter().position(|c| *c == 0).unwrap_or(data.cFileName.len());
        let name = std::ffi::OsString::from_wide(&data.cFileName[..end]);
        if name != "." && name != ".." {
            let child = path.join(&name);
            let folder = data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
            let ticks = (u64::from(data.ftLastWriteTime.dwHighDateTime) << 32) | u64::from(data.ftLastWriteTime.dwLowDateTime);
            let epoch_ticks = 116_444_736_000_000_000u64;
            let duration = std::time::Duration::from_nanos(ticks.abs_diff(epoch_ticks) % 10_000_000 * 100)
                + std::time::Duration::from_secs(ticks.abs_diff(epoch_ticks) / 10_000_000);
            let modified = if ticks >= epoch_ticks { std::time::UNIX_EPOCH.checked_add(duration) } else { std::time::UNIX_EPOCH.checked_sub(duration) };
            items.push(ShellEntry {
                display_name: name.to_string_lossy().into_owned(),
                attributes: ShellAttributes {
                    folder, hidden: data.dwFileAttributes & FILE_ATTRIBUTE_HIDDEN != 0,
                    file_system: true, link: child.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("lnk")),
                    can_rename: true, can_delete: true,
                },
                size: (!folder).then_some((u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow)),
                modified,
                identity: ShellIdentity::FileSystem { path: child, volume_id: None, file_id: None },
            });
        }
        if unsafe { FindNextFileW(handle, &raw mut data) } == 0 {
            let code = unsafe { GetLastError() };
            if code != ERROR_NO_MORE_FILES { return Err(std::io::Error::from_raw_os_error(code as i32).into()); }
            break;
        }
    }
    Ok(items)
}

fn enumerate_shell_folder(
    desktop: &IShellFolder,
    owner: isize,
    complete_source: bool,
) -> Result<Vec<ShellEntry>, ShellError> {
    let mut enumerator = None;
    let flags = (SHCONTF_FOLDERS.0
        | SHCONTF_NONFOLDERS.0
        | if complete_source {
            windows::Win32::UI::Shell::SHCONTF_INCLUDEHIDDEN.0
                | windows::Win32::UI::Shell::SHCONTF_INCLUDESUPERHIDDEN.0
        } else {
            0
        }) as u32;
    let result = unsafe {
        desktop.EnumObjects(
            WindowsHwnd(owner as *mut c_void),
            flags,
            &raw mut enumerator,
        )
    };
    result.ok()?;
    let Some(enumerator) = enumerator else {
        return Ok(Vec::new());
    };

    let mut items = Vec::new();
    loop {
        let mut child = [ptr::null_mut()];
        let mut fetched = 0_u32;
        let result = unsafe { enumerator.Next(&mut child, Some(&raw mut fetched)) };
        if fetched == 0 {
            if result.is_err() {
                return Err(windows::core::Error::from(result).into());
            }
            break;
        }
        let child = Pidl::new(child[0]);
        let resolved = (|| -> Result<ShellEntry, ShellError> {
            let shell_item: IShellItem =
                unsafe { SHCreateItemWithParent(None, desktop, child.as_ptr()) }?;
            let mut item = shell_entry(&shell_item)?;
            let parsing = shell_item_name(&shell_item, SIGDN_DESKTOPABSOLUTEPARSING)?;
            if parsing.starts_with("::{") {
                item.identity = ShellIdentity::Namespace {
                    parsing_name: parsing,
                };
            }
            Ok(item)
        })();
        match resolved {
            Ok(item) => items.push(item),
            Err(error) if complete_source => return Err(error),
            Err(_) => {}
        }
    }

    items.sort_by_cached_key(|item| {
        (
            !item.attributes.folder,
            item.display_name.to_lowercase(),
            item.display_name.clone(),
        )
    });
    Ok(items)
}

pub(crate) fn shell_entry(item: &IShellItem) -> Result<ShellEntry, ShellError> {
    let display_name = shell_item_name(item, SIGDN_NORMALDISPLAY)?;
    let identity = shell_item_name(item, SIGDN_FILESYSPATH)
        .ok()
        .filter(|value| !value.is_empty())
        .map_or_else(
            || {
                shell_item_name(item, SIGDN_DESKTOPABSOLUTEPARSING)
                    .map(|parsing_name| ShellIdentity::Namespace { parsing_name })
            },
            |path| {
                let path = PathBuf::from(path);
                let (volume_id, file_id) = stable_file_identity(&path)
                    .map_or((None, None), |(volume_id, file_id)| {
                        (Some(volume_id), Some(file_id))
                    });
                Ok(ShellIdentity::FileSystem {
                    path,
                    volume_id,
                    file_id,
                })
            },
        )?;
    let attribute_mask = SFGAO_FOLDER
        | SFGAO_FILESYSTEM
        | SFGAO_LINK
        | SFGAO_HIDDEN
        | SFGAO_CANRENAME
        | SFGAO_CANDELETE;
    let raw_attributes =
        unsafe { item.GetAttributes(attribute_mask) }.map_or(0, |attributes| attributes.0);
    let metadata = identity
        .file_system_path()
        .and_then(|path| fs::metadata(path).ok());
    let path_is_link = identity.file_system_path().is_some_and(|path| {
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
    });
    let attributes = ShellAttributes {
        folder: raw_attributes & SFGAO_FOLDER.0 != 0
            || metadata.as_ref().is_some_and(fs::Metadata::is_dir),
        file_system: raw_attributes & SFGAO_FILESYSTEM.0 != 0
            || identity.file_system_path().is_some(),
        link: raw_attributes & SFGAO_LINK.0 != 0 || path_is_link,
        hidden: raw_attributes & SFGAO_HIDDEN.0 != 0,
        can_rename: raw_attributes & SFGAO_CANRENAME.0 != 0,
        can_delete: raw_attributes & SFGAO_CANDELETE.0 != 0,
    };
    let size = metadata
        .as_ref()
        .filter(|value| value.is_file())
        .map(fs::Metadata::len);
    let modified = metadata.and_then(|metadata| metadata.modified().ok());
    Ok(ShellEntry {
        identity,
        display_name,
        attributes,
        modified,
        size,
    })
}

pub(crate) fn stable_file_identity(path: &Path) -> Option<(u64, u128)> {
    let file = OpenOptions::new()
        .access_mode(0)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    let mut info = FILE_ID_INFO::default();
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&raw mut info).cast(),
            u32::try_from(size_of::<FILE_ID_INFO>()).ok()?,
        )
    };
    (succeeded != 0).then(|| {
        (
            info.VolumeSerialNumber,
            u128::from_le_bytes(info.FileId.Identifier),
        )
    })
}

pub(crate) fn shell_item_name(
    item: &IShellItem,
    kind: windows::Win32::UI::Shell::SIGDN,
) -> Result<String, ShellError> {
    let raw = unsafe { item.GetDisplayName(kind) }?;
    let value = unsafe { raw.to_string() };
    unsafe {
        CoTaskMemFreeCom(Some(raw.0.cast()));
    }
    value.map_err(Into::into)
}

pub(crate) struct Pidl(*mut windows::Win32::UI::Shell::Common::ITEMIDLIST);

impl Pidl {
    pub(crate) fn new(value: *mut windows::Win32::UI::Shell::Common::ITEMIDLIST) -> Self {
        Self(value)
    }

    pub(crate) const fn as_ptr(&self) -> *const windows::Win32::UI::Shell::Common::ITEMIDLIST {
        self.0
    }
}

impl Drop for Pidl {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFreeCom(Some(self.0.cast()));
        }
    }
}
