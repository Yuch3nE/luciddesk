use crate::{
    Result, http,
    signed::{self, Asset},
};
use ring::digest;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek},
    os::windows::fs::OpenOptionsExt,
    path::Path,
    sync::atomic::AtomicBool,
};

pub fn fetch(version: &str, kind: &str, destination: &Path) -> Result<File> {
    let cancel = AtomicBool::new(false);
    let (base, asset) = signed::find_asset(version, kind, &cancel)?
        .ok_or("Missing signed update feed. Use the release page to update manually.")?;
    {
        let mut file = File::create_new(destination)?;
        if !http::download(
            "github.com",
            &format!("{base}/{}", asset.name),
            &cancel,
            asset.size,
            &mut file,
        )? {
            return Err("Missing release asset".into());
        }
    }
    verified_file(destination, &asset)
}

fn verified_file(destination: &Path, asset: &Asset) -> Result<File> {
    // Hold this read-only share through extraction or installation, including the confirmation dialog.
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(destination)?;
    let mut hash = digest::Context::new(&digest::SHA256);
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let actual: String = hash
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if file.metadata()?.len() != asset.size || !actual.eq_ignore_ascii_case(&asset.sha256) {
        return Err("Release asset checksum mismatch".into());
    }
    file.rewind()?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use signed::{asset, find_asset_with};
    #[test]
    #[ignore = "Downloads and verifies production release assets; requires a configured public key and published signed release"]
    fn live_release_packages_verify() {
        let version = std::env::var("LUCIDDESK_TEST_RELEASE")
            .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_owned());
        let dir = tempfile::tempdir().unwrap();
        for kind in ["green", "portable", "installed"] {
            let destination = dir.path().join(format!("{kind}.download"));
            let mut file = fetch(&version, kind, &destination)
                .unwrap_or_else(|error| panic!("Production release {version}, {kind}: {error}"));
            if kind == "installed" {
                let mut magic = [0; 2];
                file.read_exact(&mut magic).unwrap();
                assert_eq!(&magic, b"MZ");
            } else {
                crate::package::extract(
                    &mut file,
                    &dir.path().join(kind),
                    &version,
                    kind == "portable",
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn reads_release_signer_output_and_rejects_tampering() {
        let dir = tempfile::tempdir().unwrap();
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/validation/test-self-update.ps1");
        let status = std::process::Command::new("pwsh")
            .args(["-NoProfile", "-File"])
            .arg(script)
            .arg(dir.path())
            .status()
            .unwrap();
        assert!(status.success());
        let feed = std::fs::read(dir.path().join("update.json")).unwrap();
        let key = std::fs::read_to_string(dir.path().join("public.hex")).unwrap();
        assert!(
            find_asset_with("99.0.1", "green", &key, |_| Ok(None))
                .unwrap()
                .is_none()
        );
        let (base, _) = find_asset_with("99.0.1", "green", &key, |path| {
            Ok((!path.contains("/v99.0.1/")).then(|| feed.clone()))
        })
        .unwrap()
        .unwrap();
        assert!(base.ends_with("/99.0.1"));
        assert!(find_asset_with("99.0.1", "msix", &key, |_| Ok(Some(feed.clone()))).is_err());
        for (kind, suffix) in [
            ("installed", "setup.exe"),
            ("green", "120000.zip"),
            ("portable", "portable.zip"),
        ] {
            let package = asset(&feed, &key, "99.0.1", kind).unwrap();
            assert!(package.name.ends_with(suffix));
            assert_eq!(package.size, 3);
        }
        let green = asset(&feed, &key, "99.0.1", "green").unwrap();
        let archive = dir.path().join(&green.name);
        let verified = verified_file(&archive, &green).unwrap();
        assert!(std::fs::write(&archive, [4, 5, 6]).is_err());
        drop(verified);
        std::fs::write(&archive, [4, 5, 6]).unwrap();
        assert!(verified_file(&archive, &green).is_err());
        assert!(asset(&feed, &key, "99.0.2", "green").is_err());
        let mut changed: serde_json::Value = serde_json::from_slice(&feed).unwrap();
        changed["payload"] = STANDARD.encode(b"{}").into();
        let changed = serde_json::to_vec(&changed).unwrap();
        assert!(find_asset_with("99.0.1", "green", &key, |_| Ok(Some(changed.clone()))).is_err());
    }
}
