mod install;
mod integration;
pub mod self_update;
use anyhow::{Context, Result};
pub use integration::notify;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};
use toolboxcraft_core::{
    catalog::App,
    engine::Platform,
    model::{Installation, Os, Target},
    store::Store,
};

#[derive(Clone)]
pub struct Native {
    pub store: Store,
    pub target: Target,
}
impl Native {
    pub fn new(store: Store) -> Result<Self> {
        Ok(Self {
            store,
            target: Target::current()?,
        })
    }
    pub fn apps_root(&self) -> Result<PathBuf> {
        if self.target.os == Os::Mac {
            Ok(directories::BaseDirs::new()
                .context("Home directory unavailable")?
                .home_dir()
                .join("Applications/ToolboxCraft"))
        } else {
            Ok(self.store.root.join("apps"))
        }
    }
    pub fn app_path(&self, app: App) -> Result<PathBuf> {
        Ok(self.apps_root()?.join(if self.target.os == Os::Mac {
            format!("{}.app", app.name)
        } else {
            app.id.to_string()
        }))
    }
    pub fn executable(&self, app: App, path: &Path) -> Result<PathBuf> {
        match self.target.os {
            Os::Mac => Ok(path.to_path_buf()),
            Os::Linux => Ok(if path.is_file() {
                path.to_path_buf()
            } else {
                path.join(format!("{}.AppImage", app.id))
            }),
            Os::Windows => find_executable(path, &format!("{}.exe", app.id))
                .context("The installed package contains no application executable"),
        }
    }
}
pub fn find_executable(root: &Path, name: &str) -> Option<PathBuf> {
    fn walk(root: &Path, name: &str, depth: u8) -> Option<PathBuf> {
        if depth > 6 {
            return None;
        };
        for entry in std::fs::read_dir(root).ok()?.flatten() {
            let kind = entry.file_type().ok()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(name)
            {
                return Some(entry.path());
            }
            if kind.is_dir()
                && let Some(path) = walk(&entry.path(), name, depth + 1)
            {
                return Some(path);
            }
        }
        None
    }
    walk(root, name, 0)
}
impl Platform for Native {
    fn recover(&self) -> Result<BTreeMap<String, Installation>> {
        self.recover_installs()
    }
    fn detect(&self, app: App) -> Option<Installation> {
        integration::detect(self, app)
    }
    fn running(&self, installation: &Installation) -> bool {
        running_path(&installation.path)
    }
    fn install(&self, app: App, version: &str, package: &Path) -> Result<Installation> {
        self.install_package(app, version, package)
    }
    fn remove(&self, app: App, installation: &Installation) -> Result<()> {
        anyhow::ensure!(
            installation.managed && installation.path == self.app_path(app)?,
            "Refusing to remove an unmanaged path"
        );
        anyhow::ensure!(
            !running_path(&installation.path),
            "Close the application before uninstalling"
        );
        integration::remove_shortcut(self, app)?;
        remove_path(&installation.path)?;
        let receipt = self
            .store
            .root
            .join("receipts")
            .join(format!("{}.json", app.id));
        if receipt.exists() {
            std::fs::remove_file(receipt)?;
        }
        Ok(())
    }
    fn launch(&self, app: App, installation: &Installation) -> Result<()> {
        let path = self.executable(app, &installation.path)?;
        #[cfg(target_os = "macos")]
        {
            Command::new("/usr/bin/open").arg(&path).spawn()?;
        }
        #[cfg(not(target_os = "macos"))]
        {
            let mut command = Command::new(path);
            if self.target.os == Os::Linux {
                for key in [
                    "LD_LIBRARY_PATH",
                    "LD_PRELOAD",
                    "APPIMAGE",
                    "APPDIR",
                    "ARGV0",
                    "OWD",
                ] {
                    command.env_remove(key);
                }
                if !Path::new("/dev/fuse").exists() {
                    command.env("APPIMAGE_EXTRACT_AND_RUN", "1");
                }
            }
            command.spawn()?;
        }
        Ok(())
    }
    fn launch_at_login(&self, enabled: bool) -> Result<()> {
        integration::launch_at_login(self, enabled)
    }
    fn notify(&self, title: &str, message: &str) {
        integration::notify(title, message);
    }
    fn stage_self_update(&self, package: &Path) -> Result<()> {
        self_update::stage(self, package)
    }
}
pub fn running_path(path: &Path) -> bool {
    let Ok(root) = path.canonicalize() else {
        return false;
    };
    let system = sysinfo::System::new_all();
    system.processes().values().any(|p| {
        p.exe()
            .and_then(|e| e.canonicalize().ok())
            .is_some_and(|e| e == root || (root.is_dir() && e.starts_with(&root)))
            || appimage_environment_matches(&root, p.environ())
    })
}
fn appimage_environment_matches(root: &Path, environment: &[std::ffi::OsString]) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        environment
            .iter()
            .filter_map(|entry| entry.as_os_str().as_bytes().strip_prefix(b"APPIMAGE="))
            .filter_map(|path| {
                Path::new(std::ffi::OsStr::from_bytes(path))
                    .canonicalize()
                    .ok()
            })
            .any(|image| image == root || (root.is_dir() && image.starts_with(root)))
    }
    #[cfg(not(unix))]
    {
        let _ = (root, environment);
        false
    }
}
#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    #[test]
    fn appimage_processes_match_the_original_image_even_when_executables_are_mounted_elsewhere() {
        let temp = tempfile::tempdir().unwrap();
        let image = temp.path().join("photocraft.AppImage");
        std::fs::write(&image, b"image").unwrap();
        let environment = vec![std::ffi::OsString::from(format!(
            "APPIMAGE={}",
            image.display()
        ))];
        assert!(appimage_environment_matches(
            &temp.path().canonicalize().unwrap(),
            &environment
        ));
        assert!(appimage_environment_matches(
            &image.canonicalize().unwrap(),
            &environment
        ));
        assert!(!appimage_environment_matches(
            Path::new("/some-other-app"),
            &environment
        ));
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn discovers_externally_started_appimage_processes() {
        let temp = tempfile::tempdir().unwrap();
        let image = temp.path().join("photocraft.AppImage");
        std::fs::write(&image, b"image").unwrap();
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("5")
            .env("APPIMAGE", &image)
            .spawn()
            .unwrap();
        let running = running_path(temp.path());
        let _ = child.kill();
        let _ = child.wait();
        assert!(running);
    }
}
pub fn remove_path(path: &Path) -> Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
