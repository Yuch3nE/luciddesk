use crate::{Result, distribution::no_links};
use serde::Deserialize;
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
pub struct Build {
    pub version: String,
    pub architecture: String,
    pub portable: bool,
    pub files: Vec<OwnedFile>,
}
#[derive(Deserialize)]
pub struct OwnedFile {
    pub file: String,
}
pub fn build(root: &Path) -> Result<Build> {
    let bytes = fs::read(root.join("build.json"))?;
    Ok(serde_json::from_slice(
        bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes),
    )?)
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
) -> Result<Vec<String>> {
    let mut zip = zip::ZipArchive::new(file)?;
    if zip.len() > 4096 {
        return Err("Oversized ZIP directory".into());
    }
    let mut files = Vec::new();
    let mut remaining = 1024u64 * 1024 * 1024;
    let mut prefix = None;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let name = entry.name().replace('\\', "/");
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
        fs::create_dir_all(path.parent().unwrap())?;
        let mut out = File::create_new(path)?;
        let size = entry.size();
        if io::copy(&mut entry.take(size + 1), &mut out)? != size {
            return Err("Incorrect ZIP entry size".into());
        }
        remaining -= size;
        files.push(rel.to_owned());
    }
    for required in [
        "luciddesk.exe",
        "luciddesk-cli.exe",
        "updater.exe",
        "luciddesk_explorer.dll",
        "build.json",
    ] {
        if !files.iter().any(|s| s == required) {
            return Err(format!("Missing {required}").into());
        }
    }
    let b = build(stage)?;
    if b.version != version
        || b.architecture != "windows-x64"
        || b.portable != portable
        || stage.join("portable").exists() != portable
    {
        return Err("ZIP does not match this installation".into());
    }
    Ok(files)
}

pub fn check_targets(root: &Path, files: &[String], build: &Build) -> Result<()> {
    for rel in files {
        let target = root.join(rel);
        no_links(&target)?;
        if rel.contains('/')
            && target.exists()
            && !build.files.iter().any(|f| f.file.eq_ignore_ascii_case(rel))
        {
            return Err(format!("Existing user file: {}", target.display()).into());
        }
    }
    Ok(())
}

pub fn replace(root: &Path, work: &Path, files: &[String], retain: &mut bool) -> Result<()> {
    let backup = work.join("backup");
    let mut applied: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let mut install = || -> Result<()> {
        for rel in files {
            let target = root.join(rel);
            no_links(&target)?;
            fs::create_dir_all(target.parent().unwrap())?;
            let previous = if target.try_exists()? {
                let saved = backup.join(rel);
                fs::create_dir_all(saved.parent().unwrap())?;
                fs::copy(&target, &saved)?;
                Some(saved)
            } else {
                None
            };
            fs::rename(work.join("new").join(rel), &target)?;
            applied.push((target, previous));
        }
        Ok(())
    };
    if let Err(error) = install() {
        for (target, previous) in applied.into_iter().rev() {
            let restored = match previous {
                Some(saved) => fs::rename(saved, target),
                None => fs::remove_file(target),
            };
            if let Err(recovery) = restored {
                *retain = true;
                return Err(format!(
                    "{error}. Rollback failed: {recovery}. Recovery folder: {}",
                    backup.display()
                )
                .into());
            }
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, Write};
    #[test]
    fn zip_upgrades_replace_updater_preserve_data_and_roll_back() {
        for portable in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let work = tempfile::tempdir_in(root.path()).unwrap();
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
            let mut retain = false;
            let bad = ["updater.exe".to_owned(), "missing".to_owned()];
            assert!(replace(root.path(), work.path(), &bad, &mut retain).is_err());
            assert!(!retain);
            assert_eq!(fs::read(&updater).unwrap(), b"old");
            fs::write(stage.join("updater.exe"), b"new").unwrap();
            replace(root.path(), work.path(), &paths, &mut retain).unwrap();
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
        ]
        .map(str::to_owned);
        let mut retain = false;
        assert!(replace(root.path(), work.path(), &paths, &mut retain).is_err());
        assert!(!retain);
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
        let mut build = Build {
            version: "1.0.0".into(),
            architecture: "windows-x64".into(),
            portable: false,
            files: vec![],
        };
        let paths = vec!["plugins/custom.dll".to_owned()];
        assert!(check_targets(root.path(), &paths, &build).is_err());
        build.files.push(OwnedFile {
            file: paths[0].clone(),
        });
        assert!(check_targets(root.path(), &paths, &build).is_ok());
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
