use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Arch {
    X64,
    Arm64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub os: Os,
    pub arch: Arch,
}
impl Target {
    pub fn current() -> anyhow::Result<Self> {
        let os = match std::env::consts::OS {
            "macos" => Os::Mac,
            "windows" => Os::Windows,
            "linux" => Os::Linux,
            _ => anyhow::bail!("This operating system is not supported"),
        };
        let arch = match std::env::consts::ARCH {
            "x86_64" => Arch::X64,
            "aarch64" => Arch::Arm64,
            _ => anyhow::bail!("This processor architecture is not supported"),
        };
        Ok(Self { os, arch })
    }
    pub fn asset_suffix(self) -> &'static str {
        match (self.os, self.arch) {
            (Os::Mac, _) => "macos-universal.dmg",
            (Os::Windows, Arch::X64) => "windows-x64-portable.zip",
            (Os::Windows, Arch::Arm64) => "windows-arm64-portable.zip",
            (Os::Linux, Arch::X64) => "linux-x86_64.AppImage",
            (Os::Linux, Arch::Arm64) => "linux-aarch64.AppImage",
        }
    }
    pub fn label(self) -> &'static str {
        match (self.os, self.arch) {
            (Os::Mac, Arch::Arm64) => "macOS · Apple silicon",
            (Os::Mac, _) => "macOS · Intel",
            (Os::Windows, Arch::Arm64) => "Windows · ARM64",
            (Os::Windows, _) => "Windows · x64",
            (Os::Linux, Arch::Arm64) => "Linux · ARM64",
            (Os::Linux, _) => "Linux · x86_64",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub body: Option<String>,
    pub html_url: String,
    pub published_at: Option<String>,
    pub prerelease: bool,
    pub draft: bool,
    pub assets: Vec<Asset>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedRelease {
    pub release: Release,
    pub etag: Option<String>,
    pub checked_at: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Installation {
    pub version: String,
    pub path: PathBuf,
    pub managed: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub auto_update: bool,
    pub auto_update_toolboxcraft: bool,
    pub disabled_apps: Vec<String>,
    pub launch_at_login: bool,
    pub theme: Theme,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_update: true,
            auto_update_toolboxcraft: true,
            disabled_apps: vec![],
            launch_at_login: false,
            theme: Theme::System,
        }
    }
}
impl Settings {
    pub fn auto_update_app(&self, id: &str) -> bool {
        self.auto_update && !self.disabled_apps.iter().any(|v| v == id)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Operation {
    Queued,
    Downloading { received: u64, total: u64 },
    Verifying,
    WaitingForExit,
    Installing,
    Complete,
    Cancelled,
    Failed(String),
}
impl Operation {
    pub fn active(&self) -> bool {
        matches!(
            self,
            Self::Queued
                | Self::Downloading { .. }
                | Self::Verifying
                | Self::WaitingForExit
                | Self::Installing
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub app: String,
    pub version: String,
    pub timestamp: u64,
    pub action: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub schema: u32,
    pub settings: Settings,
    pub releases: BTreeMap<String, CachedRelease>,
    pub installations: BTreeMap<String, Installation>,
    pub operations: BTreeMap<String, Operation>,
    pub history: Vec<HistoryEntry>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            schema: 1,
            settings: Settings::default(),
            releases: BTreeMap::new(),
            installations: BTreeMap::new(),
            operations: BTreeMap::new(),
            history: vec![],
        }
    }
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn newer(latest: &str, installed: &str) -> bool {
    match (
        semver::Version::parse(latest.trim_start_matches('v')),
        semver::Version::parse(installed.trim_start_matches('v')),
    ) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}
