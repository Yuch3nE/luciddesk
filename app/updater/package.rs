use crate::{
    ApplyFailure, ApplyResult, Result,
    distribution::{UPDATE_BACKUP, no_links},
};
use serde::Deserialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::Path,
};
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, GENERIC_WRITE},
    Storage::FileSystem::{
        DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_RENAME_INFO,
        FILE_SHARE_READ, FileRenameInfo, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        SetFileInformationByHandle,
    },
};

#[derive(Deserialize, PartialEq, Eq)]
pub struct Build {
    pub version: String,
    pub architecture: String,
    pub portable: bool,
    pub files: Vec<OwnedFile>,
}
#[derive(Deserialize, PartialEq, Eq)]
pub struct OwnedFile {
    pub file: String,
}
pub fn build(root: &Path) -> Result<Build> {
    let bytes = fs::read(root.join("build.json"))?;
    parse_build(&bytes)
}

fn parse_build(bytes: &[u8]) -> Result<Build> {
    Ok(serde_json::from_slice(bytes.trim_prefix(&[0xef, 0xbb, 0xbf]))?)
}

pub struct StagedFile {
    pub relative: String,
    handle: File,
}

fn protected_file(path: &Path, create: bool) -> io::Result<File> {
    let file = OpenOptions::new()
        .write(true)
        .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .create_new(create)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other(format!(
            "Invalid update file: {}",
            path.display()
        )));
    }
    Ok(file)
}

fn named_file(name: &str, suffix: &str) -> bool {
    name.strip_suffix(suffix).is_some_and(|stem| {
        !stem.is_empty()
            && stem
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
            && !matches!(
                stem.to_ascii_uppercase().as_str(),
                "AUX" | "CON" | "PRN" | "NUL"
            )
            && !(stem.len() == 4
                && ["COM", "LPT"]
                    .iter()
                    .any(|s| stem[..3].eq_ignore_ascii_case(s))
                && stem.as_bytes()[3].is_ascii_digit())
    })
}
fn allowed(name: &str) -> bool {
    matches!(
        name,
        "luciddesk.exe"
            | "updater.exe"
            | "luciddesk-cli.exe"
            | "luciddesk_explorer.dll"
            | "build.json"
            | "portable"
            | "LICENSE"
            | "protocol.schema.json"
            | "Refresh-App-Icon.ps1"
            | "README.md"
            | "cli.md"
            | "usage.md"
            | "usage.en.md"
            | "brand.md"
            | "installer.md"
            | "portable.md"
            | "CHANGELOG.md"
            | "CHANGELOG.en.md"
            | "PRIVACY.md"
            | "PRIVACY.en.md"
            | "skills/luciddesk-control/SKILL.md"
    ) || name
        .strip_prefix("plugins/")
        .is_some_and(|s| named_file(s, ".dll"))
        || name
            .strip_prefix("skills/luciddesk-control/references/")
            .is_some_and(|s| named_file(s, ".md"))
}

pub fn extract(
    file: &mut File,
    stage: &Path,
    version: &str,
    portable: bool,
) -> Result<Vec<StagedFile>> {
    let mut zip = zip::ZipArchive::new(file)?;
    if zip.len() > 4096 {
        return Err("Oversized ZIP directory".into());
    }
    let mut files = Vec::new();
    let mut remaining = 1024u64 * 1024 * 1024;
    let mut prefix = None;
    let mut staged_build = None;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let name = entry.name()?.replace('\\', "/");
        if name.ends_with('/') {
            continue;
        }
        let (folder, rel) = name.split_once('/').ok_or("Invalid ZIP layout")?;
        if !folder.starts_with("LucidDesk-")
            || prefix.get_or_insert_with(|| folder.to_owned()) != folder
            || !allowed(rel)
            || entry.is_symlink()
            || entry.size() > remaining
        {
            return Err(format!("Invalid ZIP entry: {name}").into());
        }
        let path = stage.join(rel);
        no_links(&path)?;
        fs::create_dir_all(path.parent().unwrap())?;
        let mut out = protected_file(&path, true)?;
        let size = entry.size();
        if io::copy(&mut entry.take(size + 1), &mut out)? != size {
            return Err("Incorrect ZIP entry size".into());
        }
        out.sync_all()?;
        if rel == "build.json" {
            if size > 2 * 1024 * 1024 {
                return Err("Oversized build manifest".into());
            }
            let mut bytes = Vec::new();
            out.rewind()?;
            out.read_to_end(&mut bytes)?;
            staged_build = Some(parse_build(&bytes)?);
        }
        remaining -= size;
        files.push(StagedFile {
            relative: rel.to_owned(),
            handle: out,
        });
    }
    for required in [
        "luciddesk.exe",
        "luciddesk-cli.exe",
        "updater.exe",
        "luciddesk_explorer.dll",
    ] {
        if !files.iter().any(|file| file.relative == required) {
            return Err(format!("Missing {required}").into());
        }
    }
    let b = staged_build.ok_or("Missing build.json")?;
    if b.version != version
        || b.architecture != "windows-x64"
        || b.portable != portable
        || files.iter().any(|file| file.relative == "portable") != portable
    {
        return Err("ZIP does not match this installation".into());
    }
    Ok(files)
}

fn owns_target(rel: &str, build: &Build) -> bool {
    !rel.contains('/')
        || build
            .files
            .iter()
            .any(|file| file.file.eq_ignore_ascii_case(rel))
}

pub fn check_targets<'a>(
    root: &Path,
    files: impl IntoIterator<Item = &'a str>,
    build: &Build,
) -> Result<()> {
    for rel in files {
        let target = root.join(rel);
        no_links(&target)?;
        if !owns_target(rel, build) && target.try_exists()? {
            return Err(format!("Existing user file: {}", target.display()).into());
        }
    }
    Ok(())
}

fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    // Canonicalize the parent for Win32 long path support, even if the target is absent.
    let parent = path.parent().unwrap().canonicalize()?;
    Ok(parent
        .join(path.file_name().unwrap())
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect())
}

fn move_marker(from: &Path, to: &Path) -> io::Result<()> {
    let from = wide_path(from)?;
    let to = wide_path(to)?;
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn move_protected(file: &File, target: &Path) -> io::Result<()> {
    no_links(target)?;
    let name = wide_path(target)?;
    let name_offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let size = (name_offset + name.len() * size_of::<u16>()).max(size_of::<FILE_RENAME_INFO>());
    let mut buffer = vec![0usize; size.div_ceil(size_of::<usize>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // The zeroed flags forbid replacement. Win32 takes the byte count excluding the NUL.
    unsafe {
        (*info).FileNameLength = ((name.len() - 1) * size_of::<u16>()) as u32;
        let destination = buffer
            .as_mut_ptr()
            .cast::<u8>()
            .add(name_offset)
            .cast::<u16>();
        std::ptr::copy_nonoverlapping(name.as_ptr(), destination, name.len());
        if SetFileInformationByHandle(
            file.as_raw_handle(),
            FileRenameInfo,
            info.cast(),
            size as u32,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

struct Applied<'a> {
    file: &'a StagedFile,
    previous: Option<File>,
    installed: bool,
}

impl Applied<'_> {
    fn restore(&self, root: &Path, stage: &Path) -> io::Result<()> {
        if self.installed {
            move_protected(&self.file.handle, &stage.join(&self.file.relative))?;
            self.file.handle.sync_all()?;
        }
        if let Some(previous) = &self.previous {
            move_protected(previous, &root.join(&self.file.relative))?;
            previous.sync_all()?;
        }
        Ok(())
    }
}

pub fn replace(root: &Path, work: &Path, files: &[StagedFile], build: &Build) -> ApplyResult {
    let backup = work.join(UPDATE_BACKUP);
    let stage = work.join("new");
    let pending = work.join("backup-preparing");
    // Publish the recovery directory before touching any installed file.
    fs::create_dir(&pending)
        .and_then(|()| move_marker(&pending, &backup))
        .map_err(|error| ApplyFailure::Restartable(error.into()))?;
    let mut applied = Vec::new();
    let mut install = || -> Result<()> {
        for file in files {
            let rel = &file.relative;
            let target = root.join(rel);
            no_links(&target)?;
            fs::create_dir_all(target.parent().unwrap())?;
            let previous = if owns_target(rel, build) {
                match protected_file(&target, false) {
                    Ok(previous) => {
                        let saved = backup.join(rel);
                        fs::create_dir_all(saved.parent().unwrap())?;
                        move_protected(&previous, &saved)?;
                        Some(previous)
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                    Err(error) => {
                        return Err(format!("Could not open {}: {error}", target.display()).into());
                    }
                }
            } else {
                None
            };
            applied.push(Applied {
                file,
                previous,
                installed: false,
            });
            let entry = applied.last_mut().unwrap();
            if let Some(previous) = &entry.previous {
                previous.sync_all()?;
            }
            move_protected(&file.handle, &target)
                .map_err(|error| format!("Could not update {}: {error}", target.display()))?;
            entry.installed = true;
            file.handle.sync_all()?;
        }
        Ok(())
    };
    let result = install();
    if let Err(error) = &result {
        let mut recovery_error = None;
        for entry in applied.iter().rev() {
            if let Err(recovery) = entry.restore(root, &stage) {
                recovery_error.get_or_insert(recovery);
            }
        }
        if let Some(recovery) = recovery_error {
            return Err(ApplyFailure::RecoveryRequired(
                format!(
                    "{error}. Rollback failed: {recovery}. Recovery folder: {}",
                    work.display()
                )
                .into(),
            ));
        }
    }
    drop(applied);
    if let Err(error) = move_marker(&backup, &work.join("backup-completed")) {
        return Err(ApplyFailure::RecoveryRequired(
            format!(
                "Could not finalize the update: {error}. Recovery folder: {}",
                work.display()
            )
            .into(),
        ));
    }
    result.map_err(ApplyFailure::Restartable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, Write};
    #[test]
    fn zip_upgrades_protect_the_staged_payload_and_preserve_data() {
        for portable in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let work = tempfile::Builder::new()
                .prefix(crate::distribution::UPDATE_WORK_PREFIX)
                .tempdir_in(root.path())
                .unwrap();
            let stage = work.path().join("new");
            let mut file = tempfile::tempfile().unwrap();
            let mut zip = zip::ZipWriter::new(&mut file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for name in [
                "luciddesk.exe",
                "updater.exe",
                "luciddesk_explorer.dll",
                "luciddesk-cli.exe",
                "build.json",
                "portable",
            ] {
                if name == "portable" && !portable {
                    continue;
                }
                zip.start_file(format!("LucidDesk-99.0.1/{name}"), options)
                    .unwrap();
                if name == "build.json" {
                    serde_json::to_writer(&mut zip,&serde_json::json!({"version":"99.0.1","architecture":"windows-x64","portable":portable,"files":[]})).unwrap();
                } else {
                    zip.write_all(b"new").unwrap();
                }
            }
            zip.finish().unwrap();
            file.rewind().unwrap();
            let paths = extract(&mut file, &stage, "99.0.1", portable).unwrap();
            fs::create_dir(root.path().join("data")).unwrap();
            fs::write(root.path().join("data/workspace.db"), b"layout").unwrap();
            let updater = root.path().join("updater.exe");
            fs::write(&updater, b"old").unwrap();
            let build = build(&stage).unwrap();
            assert!(fs::write(stage.join("updater.exe"), b"changed").is_err());
            assert!(fs::rename(stage.join("updater.exe"), stage.join("renamed.exe")).is_err());
            replace(root.path(), work.path(), &paths, &build).unwrap();
            drop(paths);
            assert!(
                crate::distribution::interrupted_update(root.path())
                    .unwrap()
                    .is_none()
            );
            assert_eq!(fs::read(&updater).unwrap(), b"new");
            assert_eq!(root.path().join("portable").exists(), portable);
            assert_eq!(
                fs::read(root.path().join("data/workspace.db")).unwrap(),
                b"layout"
            );
        }
    }
    #[test]
    fn refuses_non_distribution_paths() {
        for name in [
            "../luciddesk.exe",
            "data/workspace.db",
            "plugins/../../outside.dll",
            "plugins/con.dll",
            "plugins/COM1.dll",
            "plugins/foo.dll:stream",
        ] {
            assert!(!allowed(name));
        }
    }
}

#[cfg(test)]
mod failure_tests {
    use super::*;
    use std::io::{Seek, Write};
    use std::os::windows::fs::OpenOptionsExt;

    fn old_build() -> Build {
        Build {
            version: "1.0.0".into(),
            architecture: "windows-x64".into(),
            portable: false,
            files: vec![],
        }
    }

    fn staged(work: &Path, paths: &[&str]) -> Vec<StagedFile> {
        paths
            .iter()
            .map(|relative| StagedFile {
                relative: (*relative).to_owned(),
                handle: protected_file(&work.join("new").join(relative), false).unwrap(),
            })
            .collect()
    }

    #[test]
    fn locked_file_restores_prior_replacements_and_removes_new_files() {
        let root = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir_in(root.path()).unwrap();
        fs::create_dir(work.path().join("new")).unwrap();
        for name in ["updater.exe", "luciddesk.exe", "luciddesk_explorer.dll"] {
            fs::write(root.path().join(name), b"old").unwrap();
            fs::write(work.path().join("new").join(name), b"new").unwrap();
        }
        fs::write(work.path().join("new/README.md"), b"new").unwrap();
        let _locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(root.path().join("luciddesk_explorer.dll"))
            .unwrap();
        let paths = [
            "updater.exe",
            "README.md",
            "luciddesk.exe",
            "luciddesk_explorer.dll",
        ];
        let files = staged(work.path(), &paths);
        assert!(matches!(
            replace(root.path(), work.path(), &files, &old_build()),
            Err(ApplyFailure::Restartable(_))
        ));
        drop(files);
        for name in ["updater.exe", "luciddesk.exe", "luciddesk_explorer.dll"] {
            assert_eq!(fs::read(root.path().join(name)).unwrap(), b"old");
        }
        assert!(!root.path().join("README.md").exists());
    }

    #[test]
    fn custom_plugins_are_protected_and_owned_plugins_are_updatable() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("plugins")).unwrap();
        fs::write(root.path().join("plugins/custom.dll"), b"user").unwrap();
        let mut build = old_build();
        let paths = ["plugins/custom.dll"];
        assert!(check_targets(root.path(), paths, &build).is_err());
        build.files.push(OwnedFile {
            file: paths[0].into(),
        });
        assert!(check_targets(root.path(), paths, &build).is_ok());
    }

    #[test]
    fn file_created_after_preflight_is_never_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir_in(root.path()).unwrap();
        let stage = work.path().join("new");
        fs::create_dir_all(stage.join("plugins")).unwrap();
        fs::create_dir(root.path().join("plugins")).unwrap();
        let paths = ["updater.exe", "plugins/new.dll"];
        for rel in paths {
            fs::write(stage.join(rel), b"release").unwrap();
        }
        let updater = root.path().join(paths[0]);
        fs::write(&updater, b"old updater").unwrap();
        let build = old_build();
        check_targets(root.path(), paths, &build).unwrap();
        let user_file = root.path().join(paths[1]);
        fs::write(&user_file, b"user").unwrap();

        let files = staged(work.path(), &paths);
        assert!(matches!(
            replace(root.path(), work.path(), &files, &build),
            Err(ApplyFailure::Restartable(_))
        ));
        assert_eq!(fs::read(&user_file).unwrap(), b"user");
        assert_eq!(fs::read(&updater).unwrap(), b"old updater");
    }

    #[test]
    fn failed_finalization_requires_recovery_and_keeps_the_backup() {
        let root = tempfile::tempdir().unwrap();
        let work = tempfile::Builder::new()
            .prefix(crate::distribution::UPDATE_WORK_PREFIX)
            .tempdir_in(root.path())
            .unwrap();
        fs::create_dir(work.path().join("new")).unwrap();
        fs::create_dir(work.path().join("backup-completed")).unwrap();
        fs::write(root.path().join("updater.exe"), b"old").unwrap();
        fs::write(work.path().join("new/updater.exe"), b"new").unwrap();
        let files = staged(work.path(), &["updater.exe"]);

        assert!(matches!(
            replace(root.path(), work.path(), &files, &old_build()),
            Err(ApplyFailure::RecoveryRequired(_))
        ));
        assert_eq!(
            crate::distribution::interrupted_update(root.path())
                .unwrap()
                .as_deref(),
            Some(work.path())
        );
        assert_eq!(
            fs::read(work.path().join(UPDATE_BACKUP).join("updater.exe")).unwrap(),
            b"old"
        );
    }

    #[test]
    fn rollback_moves_the_open_file_and_keeps_an_external_replacement() {
        let root = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir_in(root.path()).unwrap();
        let stage = work.path().join("new");
        fs::create_dir(&stage).unwrap();
        let target = root.path().join("updater.exe");
        let backup = work.path().join("backup.exe");
        fs::write(&target, b"old").unwrap();
        fs::write(stage.join("updater.exe"), b"update").unwrap();
        let files = staged(work.path(), &["updater.exe"]);
        let previous = protected_file(&target, false).unwrap();
        move_protected(&previous, &backup).unwrap();
        move_protected(&files[0].handle, &target).unwrap();

        let external = root.path().join("external.exe");
        fs::write(&external, b"user").unwrap();
        assert!(fs::rename(&external, &target).is_err());
        let mut entry = Applied {
            file: &files[0],
            previous: Some(previous),
            installed: true,
        };
        move_protected(&files[0].handle, &stage.join("updater.exe")).unwrap();
        entry.installed = false;
        fs::rename(&external, &target).unwrap();
        assert!(entry.restore(root.path(), &stage).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"user");
        assert_eq!(fs::read(&backup).unwrap(), b"old");
    }

    #[test]
    fn extraction_rejects_traversal_duplicate_targets_and_missing_binaries() {
        for entries in [
            vec!["LucidDesk-test/../outside"],
            vec!["LucidDesk-test/data/workspace.db"],
            vec!["LucidDesk-test/updater.exe", "LucidDesk-test/UPDATER.exe"],
            vec![
                "LucidDesk-test/updater.exe",
                "LucidDesk-other/luciddesk.exe",
            ],
            vec!["LucidDesk-test/updater.exe"],
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut file = tempfile::tempfile().unwrap();
            let mut zip = zip::ZipWriter::new(&mut file);
            for name in entries {
                zip.start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(b"fake").unwrap();
            }
            zip.finish().unwrap();
            file.rewind().unwrap();
            assert!(extract(&mut file, &root.path().join("new"), "99.0.1", false).is_err());
            assert!(!root.path().join("outside").exists());
        }
    }
}
