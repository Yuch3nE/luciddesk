use base64::{Engine, engine::general_purpose::STANDARD};
use ring::signature;
use serde::Deserialize;
use std::{collections::HashMap, sync::atomic::AtomicBool};

pub(super) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(super) const MODULUS: &str = match option_env!("UPDATE_RSA_MODULUS") {
    Some(key) => key,
    None => "",
};

#[derive(Deserialize)]
struct Envelope {
    payload: String,
    signature: String,
}
#[derive(Deserialize)]
struct Manifest {
    version: String,
    packages: HashMap<String, Asset>,
}
#[derive(Deserialize)]
pub(super) struct Asset {
    pub(super) name: String,
    pub(super) size: u64,
    pub(super) sha256: String,
}

pub(super) fn asset(bytes: &[u8], modulus: &str, version: &str, kind: &str) -> Result<Asset> {
    let envelope: Envelope = serde_json::from_slice(bytes)?;
    let payload = STANDARD.decode(envelope.payload)?;
    let signature = STANDARD.decode(envelope.signature)?;
    if !modulus.is_ascii() || !modulus.len().is_multiple_of(2) {
        return Err("Invalid update key".into());
    }
    let n = (0..modulus.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&modulus[i..i + 2], 16))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    signature::RsaPublicKeyComponents {
        n: n.as_slice(),
        e: &[1, 0, 1],
    }
    .verify(&signature::RSA_PKCS1_2048_8192_SHA256, &payload, &signature)
    .map_err(|_| "Invalid release signature")?;
    let mut manifest: Manifest = serde_json::from_slice(&payload)?;
    let asset = manifest
        .packages
        .remove(kind)
        .ok_or("Missing update package")?;
    if manifest.version != version
        || !(1..=512 * 1024 * 1024).contains(&asset.size)
        || !asset.name.starts_with("LucidDesk-")
        || !asset
            .name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
        || asset.sha256.len() != 64
        || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Invalid signed package metadata".into());
    }
    Ok(asset)
}

pub(super) fn find_asset(
    version: &str,
    kind: &str,
    cancel: &AtomicBool,
) -> Result<Option<(String, Asset)>> {
    find_asset_with(version, kind, MODULUS, |path| {
        super::http::get("github.com", path, cancel)
    })
}

pub(super) fn find_asset_with(
    version: &str,
    kind: &str,
    modulus: &str,
    mut get: impl FnMut(&str) -> std::result::Result<Option<Vec<u8>>, String>,
) -> Result<Option<(String, Asset)>> {
    for tag in [format!("v{version}"), version.to_owned()] {
        let base = format!("/Yuch3nE/luciddesk/releases/download/{tag}");
        if let Some(feed) = get(&format!("{base}/update.json"))? {
            return Ok(Some((base, asset(&feed, modulus, version, kind)?)));
        }
    }
    Ok(None)
}
