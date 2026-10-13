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
    let repo = option_env!("TOOLBOXCRAFT_RELEASE_REPO").unwrap_or("teunvanderploeg/ToolboxCraft");
    let key = option_env!("TOOLBOXCRAFT_UPDATE_PUBLIC_KEY")
        .unwrap_or(include_str!("../../../packaging/update-public-key.hex"))
        .trim();
    (!repo.is_empty() && !key.is_empty()).then_some((repo, key))
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
    check_at(
        client,
        target,
        repo,
        key,
        &format!("{}/{repo}/releases/latest/download", client.github_origin),
    )
    .await
}
pub(crate) async fn check_at(
    client: &Releases,
    target: Target,
    repo: &str,
    key: &str,
    base: &str,
) -> Result<Option<SelfUpdate>> {
    let bytes = client.small_file(&format!("{base}/update.json")).await?;
    let signature = client
        .small_file(&format!("{base}/update.json.sig"))
        .await?;
    let manifest = verify_manifest(&bytes, &signature, key)?;
    ensure!(
        semver::Version::parse(&manifest.version)?.pre.is_empty(),
        "ToolboxCraft update must be a stable release"
    );
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
        "{}/{}/releases/download/v{}/{}",
        client.github_origin, update.repo, update.manifest.version, update.package.file
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
        let other = hex::encode(SigningKey::from_bytes(&[8; 32]).verifying_key().as_bytes());
        assert!(verify_manifest(bytes, signature.as_bytes(), &other).is_err());
    }
    #[test]
    fn default_build_has_a_valid_pinned_publisher_key() {
        let (repo, key) = configuration().unwrap();
        assert_eq!(repo, "teunvanderploeg/ToolboxCraft");
        let bytes: [u8; 32] = hex::decode(key).unwrap().try_into().unwrap();
        assert!(VerifyingKey::from_bytes(&bytes).is_ok());
    }
}
