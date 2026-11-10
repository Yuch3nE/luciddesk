use std::{
    fs, io,
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Path, PathBuf},
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_READ_ATTRIBUTES, GetFileInformationByHandle,
};

pub(crate) const UPDATE_WORK_PREFIX: &str = ".luciddesk-update-";
pub(crate) const UPDATE_BACKUP: &str = "backup";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Distribution {
    Green,
    Portable,
    Inno(&'static str),
}

impl Distribution {
    pub(crate) fn detect(directory: &Path) -> Option<Self> {
        if directory.join("msix").exists() {
            return None;
        }
        if directory.join("installed").exists() {
            if directory.join("portable").is_file() {
                return None;
            }
            return inno_scope(directory).map(Self::Inno);
        }
        if directory.join("portable").is_file() {
            Some(Self::Portable)
        } else {
            Some(Self::Green)
        }
    }

    pub(crate) fn kind(self) -> &'static str {
        match self {
            Self::Green => "green",
            Self::Portable => "portable",
            Self::Inno(_) => "installed",
        }
    }
}

pub(crate) fn update_gate_name(directory: &Path) -> io::Result<Vec<u16>> {
    let handle = fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(directory)?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(handle.as_raw_handle(), &raw mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(format!(
        r"Global\LucidDesk.UpdateApplying.{:08x}.{:08x}{:08x}",
        info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
    )
    .encode_utf16()
    .chain(Some(0))
    .collect())
}


pub(crate) fn interrupted_update(directory: &std::path::Path) -> io::Result<Option<PathBuf>> {
    if directory.join("installed").exists() || directory.join("msix").exists() {
        return Ok(None);
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(UPDATE_WORK_PREFIX)
            || !entry.file_type()?.is_dir()
        {
            continue;
        }
        let work = entry.path();
        if work.join(UPDATE_BACKUP).try_exists()? {
            return Ok(Some(work));
        }
    }
    Ok(None)
}

fn inno_scope(directory: &Path) -> Option<&'static str> {
    let Ok(directory) = directory.canonicalize() else {
        return None;
    };
    let uninstall = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{A059751B-F1E3-4C4A-AB35-A03FB70C3CF4}_is1";
    [
        windows_registry::CURRENT_USER,
        windows_registry::LOCAL_MACHINE,
    ]
    .iter()
    .zip(["/CURRENTUSER", "/ALLUSERS"])
    .find_map(|(root, scope)| {
        root.open(uninstall)
            .and_then(|key| key.get_string("InstallLocation"))
            .ok()
            .and_then(|path| PathBuf::from(path).canonicalize().ok())
            .is_some_and(|path| path == directory)
            .then_some(scope)
    })
}

pub(crate) fn no_links(path: &std::path::Path) -> std::io::Result<()> {
    for parent in path.ancestors() {
        match fs::symlink_metadata(parent) {
            Ok(meta) if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
                return Err(io::Error::other(format!(
                    "Linked update path: {}",
                    parent.display()
                )));
            }
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}
