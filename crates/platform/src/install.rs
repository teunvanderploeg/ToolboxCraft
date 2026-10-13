use crate::{Native, integration, remove_path, running_path};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use toolboxcraft_core::{
    catalog::{App, CATALOG},
    model::{Installation, Os},
    store::atomic_write,
};

impl Native {
    fn receipt_path(&self, app: App) -> PathBuf {
        self.store
            .root
            .join("receipts")
            .join(format!("{}.json", app.id))
    }
    fn journal_path(&self, app: App) -> PathBuf {
        self.store
            .root
            .join("transactions")
            .join(format!("{}.json", app.id))
    }
    fn staging(&self, app: App) -> Result<PathBuf> {
        Ok(self.apps_root()?.join(format!(".{}.stage", app.id)))
    }
    fn backup(&self, app: App) -> Result<PathBuf> {
        Ok(self.apps_root()?.join(format!(".{}.backup", app.id)))
    }
    fn receipt(&self, app: App, version: &str) -> Result<Installation> {
        let installation = Installation {
            version: version.into(),
            path: self.app_path(app)?,
            managed: true,
        };
        fs::create_dir_all(self.store.root.join("receipts"))?;
        atomic_write(
            &self.receipt_path(app),
            &serde_json::to_vec_pretty(&installation)?,
        )?;
        Ok(installation)
    }
    pub(crate) fn recover_installs(
        &self,
    ) -> Result<std::collections::BTreeMap<String, Installation>> {
        let mut installations = std::collections::BTreeMap::new();
        for app in CATALOG {
            let target = self.app_path(app)?;
            let stage = self.staging(app)?;
            let backup = self.backup(app)?;
            let journal = self.journal_path(app);
            if journal.exists() {
                if backup.exists() && !target.exists() {
                    fs::rename(&backup, &target)?;
                } else if target.exists()
                    && !stage.exists()
                    && (backup.exists() || !self.receipt_path(app).exists())
                {
                    let record: serde_json::Value = serde_json::from_slice(&fs::read(&journal)?)?;
                    self.receipt(
                        app,
                        record["version"]
                            .as_str()
                            .context("Invalid installation recovery record")?,
                    )?;
                }
                remove_path(&backup)?;
                remove_path(&stage)?;
                fs::remove_file(journal)?;
            } else {
                remove_path(&stage)?;
            }
            let receipt = self.receipt_path(app);
            if receipt.exists() {
                let i: Installation = serde_json::from_slice(&fs::read(receipt)?)?;
                ensure!(
                    i.managed && i.path == target,
                    "Invalid managed installation receipt for {}",
                    app.name
                );
                if i.path.exists() {
                    installations.insert(app.id.into(), i);
                }
            }
        }
        Ok(installations)
    }
    pub(crate) fn install_package(
        &self,
        app: App,
        version: &str,
        package: &Path,
    ) -> Result<Installation> {
        semver_check(version)?;
        fs::create_dir_all(self.apps_root()?)?;
        fs::create_dir_all(self.store.root.join("transactions"))?;
        let target = self.app_path(app)?;
        let stage = self.staging(app)?;
        let backup = self.backup(app)?;
        ensure!(!running_path(&target), "Close {} before updating", app.name);
        if target.exists() {
            ensure!(
                self.receipt_path(app).exists(),
                "An unmanaged app already exists at {}. Move it before installing.",
                target.display()
            );
        }
        ensure!(
            !backup.exists(),
            "An earlier installation needs recovery. Restart ToolboxCraft."
        );
        remove_path(&stage)?;
        let prepared = match self.target.os {
            Os::Mac => prepare_mac(app, version, package, &stage),
            Os::Windows => {
                fs::create_dir_all(&stage)?;
                extract_zip(package, &stage).and_then(|()| self.executable(app, &stage).map(|_| ()))
            }
            Os::Linux => {
                fs::create_dir_all(&stage)?;
                let file = stage.join(format!("{}.AppImage", app.id));
                fs::copy(package, &file)?;
                let mut magic = [0; 4];
                fs::File::open(&file)?.read_exact(&mut magic)?;
                ensure!(
                    &magic == b"\x7fELF",
                    "The downloaded AppImage is not an ELF executable"
                );
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(file, fs::Permissions::from_mode(0o755))?;
                }
                Ok(())
            }
        };
        if let Err(err) = prepared {
            remove_path(&stage)?;
            return Err(err);
        }
        // Stage is complete before touching a working installation. The journal is deliberately outside signed bundles.
        atomic_write(
            &self.journal_path(app),
            &serde_json::to_vec(&json!({"version": version}))?,
        )?;
        ensure!(
            !running_path(&target),
            "{} opened during installation. Close it and retry.",
            app.name
        );
        if target.exists() {
            fs::rename(&target, &backup)?;
        }
        if let Err(err) = fs::rename(&stage, &target) {
            if backup.exists() {
                fs::rename(&backup, &target)?;
            }
            return Err(err.into());
        }
        let installed =
            match integration::shortcut(self, app).and_then(|()| self.receipt(app, version)) {
                Ok(i) => i,
                Err(err) => {
                    remove_path(&target)?;
                    if backup.exists() {
                        fs::rename(&backup, &target)?;
                    }
                    fs::remove_file(self.journal_path(app))?;
                    return Err(err);
                }
            };
        remove_path(&backup)?;
        fs::remove_file(self.journal_path(app))?;
        Ok(installed)
    }
}
fn semver_check(version: &str) -> Result<()> {
    ensure!(
        !version.is_empty()
            && version
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ".+-".contains(c)),
        "Invalid version"
    );
    Ok(())
}
pub fn extract_zip(package: &Path, stage: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(fs::File::open(package)?)?;
    ensure!(archive.len() <= 100_000, "Archive contains too many files");
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name();
        ensure!(
            !name.contains(['\\', ':'])
                && !name.starts_with('/')
                && !name.split('/').any(|p| p == ".."),
            "Unsafe archive path"
        );
        let path = entry
            .enclosed_name()
            .context("Archive tries to write outside its installation folder")?;
        ensure!(
            entry.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
            "Archive symlinks are not allowed"
        );
        total = total
            .checked_add(entry.size())
            .context("Archive is too large")?;
        ensure!(total <= 16 * 1024 * 1024 * 1024, "Archive is too large");
        let output = stage.join(&path);
        if entry.is_dir() {
            fs::create_dir_all(output)?;
        } else {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)?;
            std::io::copy(&mut entry, &mut file)?;
            file.sync_all()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = entry.unix_mode() {
                    fs::set_permissions(output, fs::Permissions::from_mode(mode & 0o777))?;
                }
            }
        }
    }
    Ok(())
}
#[cfg(target_os = "macos")]
fn prepare_mac(app: App, version: &str, package: &Path, stage: &Path) -> Result<()> {
    use std::process::Command;
    let mount = stage.with_extension("mount");
    remove_path(&mount)?;
    fs::create_dir_all(&mount)?;
    let output = Command::new("/usr/bin/hdiutil")
        .args([
            "attach",
            "-readonly",
            "-nobrowse",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(&mount)
        .arg(package)
        .output()?;
    ensure!(
        output.status.success(),
        "Cannot mount disk image: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    struct Mounted(PathBuf);
    impl Drop for Mounted {
        fn drop(&mut self) {
            let _ = Command::new("/usr/bin/hdiutil")
                .arg("detach")
                .arg(&self.0)
                .output();
            let _ = fs::remove_dir(&self.0);
        }
    }
    let _mount = Mounted(mount.clone());
    let source = mount.join(format!("{}.app", app.name));
    ensure!(source.exists(), "Disk image contains no {}.app", app.name);
    // Finder adds resource metadata on mounted images. Copy only bundle files before
    // strict verification; code signatures remain intact and quarantine is reapplied below.
    let output = Command::new("/usr/bin/ditto")
        .args(["--norsrc", "--noextattr", "--noacl"])
        .arg(&source)
        .arg(stage)
        .output()?;
    ensure!(
        output.status.success(),
        "Cannot copy application: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    verify_mac_signature(stage)?;
    let info = stage.join("Contents/Info.plist");
    let plist_value = |key: &str| -> Result<String> {
        let o = Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", &format!("Print :{key}")])
            .arg(&info)
            .output()?;
        ensure!(o.status.success(), "App bundle metadata is missing {key}");
        Ok(String::from_utf8(o.stdout)?.trim().to_string())
    };
    ensure!(
        plist_value("CFBundleIdentifier")? == app.bundle_id(),
        "Unexpected application bundle identity"
    );
    ensure!(
        plist_value("CFBundleShortVersionString")? == version.trim_start_matches('v'),
        "App bundle does not match the downloaded release version"
    );
    let quarantine = format!("0081;{:x};ToolboxCraft;", toolboxcraft_core::model::now());
    let status = Command::new("/usr/bin/xattr")
        .args(["-w", "com.apple.quarantine", &quarantine])
        .arg(stage)
        .status()?;
    ensure!(status.success(), "Cannot apply macOS download quarantine");
    Ok(())
}
#[cfg(target_os = "macos")]
fn verify_mac_signature(bundle: &Path) -> Result<()> {
    // '=' tells codesign this is an inline requirement, rather than a file name.
    let requirement = "=anchor apple generic and certificate leaf[subject.OU] = \"DJ6XS33FX8\"";
    let output = std::process::Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict", "-R", requirement])
        .arg(bundle)
        .output()?;
    ensure!(
        output.status.success(),
        "Artcraft publisher signature verification failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn prepare_mac(_: App, _: &str, _: &Path, _: &Path) -> Result<()> {
    anyhow::bail!("macOS installation requires macOS")
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use toolboxcraft_core::{
        model::{Arch, Target},
        store::Store,
    };
    fn archive(path: &Path, name: &str) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"data").unwrap();
        zip.finish().unwrap();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn publisher_check_rejects_unsigned_and_adhoc_bundles() {
        let d = tempfile::tempdir().unwrap();
        let bundle = d.path().join("Test.app");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::copy("/usr/bin/true", bundle.join("Contents/MacOS/Test")).unwrap();
        fs::write(bundle.join("Contents/Info.plist"), br#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>dev.toolboxcraft.signature-test</string><key>CFBundleExecutable</key><string>Test</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>"#).unwrap();
        let initial = verify_mac_signature(&bundle).unwrap_err().to_string();
        assert!(
            !initial.contains("invalid requirement specification"),
            "{initial}"
        );
        assert!(
            std::process::Command::new("/usr/bin/codesign")
                .args(["--force", "--sign", "-"])
                .arg(&bundle)
                .output()
                .unwrap()
                .status
                .success()
        );
        let err = verify_mac_signature(&bundle).unwrap_err().to_string();
        assert!(
            err.contains("failed to satisfy specified code requirement"),
            "{err}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires an official Artcraft DMG via TOOLBOXCRAFT_TEST_DMG and its version via TOOLBOXCRAFT_TEST_VERSION"]
    fn official_wordcraft_dmg_stages_with_valid_signature_and_quarantine() {
        let package = PathBuf::from(std::env::var("TOOLBOXCRAFT_TEST_DMG").unwrap());
        let version = std::env::var("TOOLBOXCRAFT_TEST_VERSION").unwrap();
        let d = tempfile::tempdir().unwrap();
        let stage = d.path().join("WordCraft.app");
        let app = toolboxcraft_core::catalog::by_id("wordcraft").unwrap();
        prepare_mac(app, &version, &package, &stage).unwrap();
        verify_mac_signature(&stage).unwrap();
        let quarantine = std::process::Command::new("/usr/bin/xattr")
            .args(["-p", "com.apple.quarantine"])
            .arg(&stage)
            .output()
            .unwrap();
        assert!(quarantine.status.success());
        assert!(String::from_utf8_lossy(&quarantine.stdout).contains("ToolboxCraft"));
    }

    #[test]
    fn managed_install_update_and_uninstall_preserve_documents() {
        use toolboxcraft_core::engine::Platform;
        let d = tempfile::tempdir().unwrap();
        let native = Native {
            store: Store::new(d.path()).unwrap(),
            target: Target {
                os: Os::Linux,
                arch: Arch::X64,
            },
        };
        let app = CATALOG[0];
        let package = d.path().join("download.AppImage");
        let document = d.path().join("user-document.psd");
        fs::write(&document, b"document").unwrap();
        fs::write(&package, b"\x7fELFfirst version").unwrap();
        let installed = native.install_package(app, "v1.0.0", &package).unwrap();
        assert_eq!(installed.version, "v1.0.0");
        assert!(installed.path.join("photocraft.AppImage").is_file());
        fs::write(&package, b"invalid image").unwrap();
        assert!(native.install_package(app, "v2.0.0", &package).is_err());
        assert_eq!(
            fs::read(installed.path.join("photocraft.AppImage")).unwrap(),
            b"\x7fELFfirst version"
        );
        fs::write(&package, b"\x7fELFsecond version").unwrap();
        let updated = native.install_package(app, "v2.0.0", &package).unwrap();
        assert_eq!(native.recover_installs().unwrap()[app.id].version, "v2.0.0");
        native.remove(app, &updated).unwrap();
        assert!(!updated.path.exists());
        assert_eq!(fs::read(document).unwrap(), b"document");
    }
    #[test]
    fn refuses_to_overwrite_an_unmanaged_installation() {
        let d = tempfile::tempdir().unwrap();
        let native = Native {
            store: Store::new(d.path()).unwrap(),
            target: Target {
                os: Os::Linux,
                arch: Arch::X64,
            },
        };
        let app = CATALOG[0];
        fs::create_dir_all(native.app_path(app).unwrap()).unwrap();
        let package = d.path().join("download");
        fs::write(&package, b"\x7fELF").unwrap();
        assert!(
            native
                .install_package(app, "1.0.0", &package)
                .unwrap_err()
                .to_string()
                .contains("unmanaged")
        );
    }
    #[test]
    fn extraction_rejects_traversal_and_windows_paths() {
        let d = tempfile::tempdir().unwrap();
        for name in ["../escape", "/escape", "C:/escape", "dir\\escape"] {
            let zip = d.path().join("test.zip");
            archive(&zip, name);
            assert!(extract_zip(&zip, &d.path().join("out")).is_err());
        }
        assert!(!d.path().join("escape").exists());
    }
    #[test]
    fn recovery_rolls_back_missing_target_and_commits_activated_target() {
        let d = tempfile::tempdir().unwrap();
        let native = Native {
            store: Store::new(d.path()).unwrap(),
            target: Target {
                os: Os::Linux,
                arch: Arch::X64,
            },
        };
        let app = CATALOG[0];
        fs::create_dir_all(native.backup(app).unwrap()).unwrap();
        fs::create_dir_all(native.store.root.join("transactions")).unwrap();
        native.receipt(app, "1.0.0").unwrap();
        atomic_write(&native.journal_path(app), br#"{"version":"2.0.0"}"#).unwrap();
        let recovered = native.recover_installs().unwrap();
        assert_eq!(recovered[app.id].version, "1.0.0");
        assert!(native.app_path(app).unwrap().exists());
        fs::create_dir_all(native.backup(app).unwrap()).unwrap();
        atomic_write(&native.journal_path(app), br#"{"version":"2.0.0"}"#).unwrap();
        let recovered = native.recover_installs().unwrap();
        assert_eq!(recovered[app.id].version, "2.0.0");
        assert!(!native.backup(app).unwrap().exists());
    }
}
