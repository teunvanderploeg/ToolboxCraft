use crate::model::{Operation, State};
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Store {
    pub root: PathBuf,
}
impl Store {
    pub fn standard() -> Result<Self> {
        let dirs = directories::ProjectDirs::from("dev", "ToolboxCraft", "ToolboxCraft")
            .context("Cannot locate your application data directory")?;
        Self::new(dirs.data_local_dir())
    }
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        Ok(Self {
            root: root.as_ref().to_path_buf(),
        })
    }
    pub fn lock(&self) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join("instance.lock"))?;
        file.try_lock_exclusive()
            .context("ToolboxCraft is already running. Open it from the tray or menu bar.")?;
        Ok(file)
    }
    pub fn load(&self) -> Result<State> {
        let path = self.root.join("state.json");
        if !path.exists() {
            return Ok(State::default());
        }
        let state: State = serde_json::from_slice(&fs::read(path)?)
            .context("Cannot read state.json; restore it from a backup before restarting")?;
        ensure!(
            state.schema == 1,
            "This saved state requires a newer ToolboxCraft version"
        );
        Ok(state)
    }
    pub fn save(&self, state: &State) -> Result<()> {
        atomic_write(
            &self.root.join("state.json"),
            &serde_json::to_vec_pretty(state)?,
        )
    }
    pub fn recover(&self, state: &mut State) -> Result<()> {
        for op in state.operations.values_mut() {
            if op.active() {
                *op = Operation::Failed(
                    "Interrupted. Your installed app was preserved. Retry the operation.".into(),
                );
            }
        }
        let downloads = self.root.join("downloads");
        if downloads.is_dir() {
            for entry in fs::read_dir(downloads)? {
                let entry = entry?;
                if entry.file_type()?.is_file()
                    && entry.path().extension().is_some_and(|s| s == "package")
                {
                    fs::remove_file(entry.path())?;
                }
            }
        }
        self.save(state)
    }
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = path.with_extension("json.tmp");
    let mut file = File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    #[cfg(windows)]
    {
        // MoveFileExW provides replacement without a remove/write gap.
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn MoveFileExW(a: *const u16, b: *const u16, flags: u32) -> i32;
        }
        let a: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 0x1 | 0x8) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(not(windows))]
    fs::rename(temp, path)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_settings_enable_launcher_updates_without_changing_app_preferences() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::new(d.path()).unwrap();
        let mut state = State::default();
        state.settings.auto_update = false;
        state.settings.disabled_apps.push("photocraft".into());
        let mut old = serde_json::to_value(&state).unwrap();
        old["settings"]
            .as_object_mut()
            .unwrap()
            .remove("auto_update_toolboxcraft");
        fs::write(
            store.root.join("state.json"),
            serde_json::to_vec(&old).unwrap(),
        )
        .unwrap();
        let mut migrated = store.load().unwrap();
        assert!(migrated.settings.auto_update_toolboxcraft);
        assert!(!migrated.settings.auto_update);
        assert_eq!(migrated.settings.disabled_apps, ["photocraft"]);
        migrated.settings.auto_update_toolboxcraft = false;
        store.save(&migrated).unwrap();
        assert!(!store.load().unwrap().settings.auto_update_toolboxcraft);
    }
    #[test]
    fn interruption_preserves_installs_and_releases_lock() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::new(d.path()).unwrap();
        let lock = store.lock().unwrap();
        assert!(store.lock().is_err());
        drop(lock);
        assert!(store.lock().is_ok());
        let mut state = State::default();
        state
            .operations
            .insert("photocraft".into(), Operation::Installing);
        store.save(&state).unwrap();
        let mut recovered = store.load().unwrap();
        store.recover(&mut recovered).unwrap();
        assert!(matches!(
            recovered.operations["photocraft"],
            Operation::Failed(_)
        ));
        assert_eq!(store.load().unwrap().schema, 1);
    }
}
