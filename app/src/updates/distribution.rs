use std::path::PathBuf;
use std::{fs, io, os::windows::fs::MetadataExt};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
pub(crate) fn inno_scope(directory: &std::path::Path) -> Option<&'static str> {
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
