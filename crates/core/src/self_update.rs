use crate::{
    model::{Target, newer},
    releases::Releases,
};
use anyhow::{Context, Result, ensure};
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePackage {
    pub file: String,
    pub sha256: String,
    pub size: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    pub packages: BTreeMap<String, UpdatePackage>,
}
#[derive(Debug, Clone)]
pub struct SelfUpdate {
    pub manifest: UpdateManifest,
    pub repo: String,
    pub package: UpdatePackage,
}
pub fn configuration() -> Option<(&'static str, &'static str)> {
    match (
        option_env!("TOOLBOXCRAFT_RELEASE_REPO"),
        option_env!("TOOLBOXCRAFT_UPDATE_PUBLIC_KEY"),
    ) {
        (Some(repo), Some(key)) if !repo.is_empty() && !key.is_empty() => Some((repo, key)),
        _ => None,
    }
}
pub fn target_key(target: Target) -> &'static str {
    use crate::model::{Arch, Os};
    match (target.os, target.arch) {
        (Os::Mac, _) => "macos-universal",
        (Os::Windows, Arch::X64) => "windows-x64",
        (Os::Windows, Arch::Arm64) => "windows-arm64",
        (Os::Linux, Arch::X64) => "linux-x86_64",
        (Os::Linux, Arch::Arm64) => "linux-aarch64",
    }
}
pub fn verify_manifest(bytes: &[u8], signature: &[u8], key: &str) -> Result<UpdateManifest> {
    let key: [u8; 32] = hex::decode(key)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Update key must be 32 bytes in hexadecimal"))?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature.strip_suffix(b"\n").unwrap_or(signature))?;
    VerifyingKey::from_bytes(&key)?
        .verify_strict(bytes, &Signature::from_slice(&signature)?)
        .context("ToolboxCraft release signature is invalid")?;
    let manifest: UpdateManifest = serde_json::from_slice(bytes)?;
    semver::Version::parse(&manifest.version)?;
    Ok(manifest)
}
pub async fn check(client: &Releases, target: Target) -> Result<Option<SelfUpdate>> {
    let Some((repo, key)) = configuration() else {
        return Ok(None);
    };
    let parts: Vec<_> = repo.split('/').collect();
    ensure!(
        parts.len() == 2
            && parts.iter().all(|part| !part.is_empty()
                && *part != "."
                && *part != ".."
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))),
        "Invalid ToolboxCraft release repository"
    );
    let base = format!("https://github.com/{repo}/releases/latest/download");
    let bytes = client.small_file(&format!("{base}/update.json")).await?;
    let signature = client
        .small_file(&format!("{base}/update.json.sig"))
        .await?;
    let manifest = verify_manifest(&bytes, &signature, key)?;
    if !newer(&manifest.version, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let package = manifest
        .packages
        .get(target_key(target))
        .context("ToolboxCraft update has no package for your computer")?
        .clone();
    ensure!(
        !package.file.contains(['/', '\\'])
            && package.file.starts_with("toolboxcraft-")
            && package.file.ends_with(".zip"),
        "Invalid self-update package name"
    );
    Ok(Some(SelfUpdate {
        manifest,
        repo: repo.into(),
        package,
    }))
}
pub async fn download(
    client: &Releases,
    update: &SelfUpdate,
    path: &Path,
    cancel: &CancellationToken,
) -> Result<()> {
    let hash: [u8; 32] = hex::decode(&update.package.sha256)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid update checksum"))?;
    let url = format!(
        "https://github.com/{}/releases/download/v{}/{}",
        update.repo, update.manifest.version, update.package.file
    );
    client
        .download_url(&url, update.package.size, path, hash, cancel, |_, _| {})
        .await
}
#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    #[test]
    fn verifies_bytes_before_parsing_and_rejects_tampering() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let bytes = br#"{"version":"1.0.0","packages":{}}"#;
        let signature =
            base64::engine::general_purpose::STANDARD.encode(key.sign(bytes).to_bytes());
        let public = hex::encode(key.verifying_key().as_bytes());
        assert!(verify_manifest(bytes, signature.as_bytes(), &public).is_ok());
        assert!(verify_manifest(b"tampered", signature.as_bytes(), &public).is_err());
    }
}
