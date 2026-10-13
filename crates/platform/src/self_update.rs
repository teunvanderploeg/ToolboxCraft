use crate::{Native, install::extract_zip, remove_path};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
use toolboxcraft_core::model::Os;

pub fn stage(native: &Native, package: &Path) -> Result<()> {
    let stage = native.store.root.join("self-update-stage");
    remove_path(&stage)?;
    fs::create_dir_all(&stage)?;
    extract_zip(package, &stage)?;
    let executable = std::env::current_exe()?;
    let (target, replacement, launcher) = if native.target.os == Os::Mac {
        let bundle = executable
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .context("Run ToolboxCraft from its installed .app bundle to update it")?;
        ensure!(
            bundle.extension().is_some_and(|e| e == "app"),
            "Self-update requires an installed .app bundle"
        );
        let replacement = stage.join("ToolboxCraft.app");
        let output = Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&replacement)
            .output()?;
        ensure!(
            output.status.success(),
            "Updated ToolboxCraft bundle has an invalid signature"
        );
        // Require the installed app's publisher, independently of the manifest signature.
        let identity = |path: &Path| -> Result<String> {
            let o = Command::new("/usr/bin/codesign")
                .args(["-dv", "--verbose=4"])
                .arg(path)
                .output()?;
            let text = String::from_utf8_lossy(&o.stderr);
            text.lines()
                .find_map(|l| l.strip_prefix("TeamIdentifier="))
                .filter(|s| *s != "not set")
                .map(str::to_owned)
                .context("Self-update requires a publisher-signed macOS app")
        };
        ensure!(
            identity(bundle)? == identity(&replacement)?,
            "Updated ToolboxCraft has a different publisher"
        );
        (
            bundle.to_path_buf(),
            replacement,
            "Contents/MacOS/toolboxcraft",
        )
    } else if native.target.os == Os::Linux && std::env::var_os("APPIMAGE").is_some() {
        let target = std::path::PathBuf::from(std::env::var_os("APPIMAGE").unwrap());
        ensure!(
            target.is_absolute() && target.is_file(),
            "AppImage installation path is unavailable"
        );
        (target, stage.join("ToolboxCraft.AppImage"), "--file")
    } else {
        // Update archives contain a directory, preserving the helper and other runtime files together.
        let target = executable
            .parent()
            .context("Cannot locate ToolboxCraft installation")?
            .to_path_buf();
        ensure!(
            target
                .file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("ToolboxCraft")),
            "Move the portable ToolboxCraft folder to a directory named ToolboxCraft before self-updating"
        );
        let name = if native.target.os == Os::Windows {
            "toolboxcraft.exe"
        } else {
            "toolboxcraft"
        };
        (target, stage.join("ToolboxCraft"), name)
    };
    ensure!(
        (launcher == "--file" && replacement.is_file()) || replacement.join(launcher).is_file(),
        "Self-update package has no launcher executable"
    );
    if native.target.os == Os::Windows {
        let uninstaller = target.join("Uninstall.exe");
        if uninstaller.is_file() {
            fs::copy(uninstaller, replacement.join("Uninstall.exe"))?;
        }
    }
    let helper_name = if native.target.os == Os::Windows {
        "toolboxcraft-updater.exe"
    } else {
        "toolboxcraft-updater"
    };
    let helper_source = executable.with_file_name(helper_name);
    ensure!(
        helper_source.is_file(),
        "The updater helper is missing. Install a packaged ToolboxCraft release."
    );
    let helper = native.store.root.join(helper_name);
    fs::copy(helper_source, &helper)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o755))?;
    }
    Command::new(helper)
        .arg(std::process::id().to_string())
        .arg(target)
        .arg(replacement)
        .arg(launcher)
        .spawn()?;
    Ok(())
}
pub fn apply(args: &[String]) -> Result<()> {
    ensure!(
        args.len() == 4,
        "Usage: toolboxcraft-updater PID TARGET REPLACEMENT LAUNCHER"
    );
    let pid: u32 = args[0].parse()?;
    let target = Path::new(&args[1]);
    let replacement = Path::new(&args[2]);
    let launcher = Path::new(&args[3]);
    ensure!(
        target.is_absolute()
            && replacement.is_absolute()
            && launcher.is_relative()
            && !launcher
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
        "Invalid self-update paths"
    );
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let system = sysinfo::System::new_all();
        if system.process(sysinfo::Pid::from_u32(pid)).is_none() {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "ToolboxCraft did not exit; no files were replaced"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
    let backup = target.with_extension("toolboxcraft-backup");
    ensure!(!backup.exists(), "Previous self-update backup still exists");
    let adjacent = target.with_extension("toolboxcraft-update");
    ensure!(
        !adjacent.exists(),
        "Previous self-update staging folder still exists"
    );
    if replacement.is_file() {
        fs::copy(replacement, &adjacent)?;
    } else {
        copy_tree(replacement, &adjacent)?;
    }
    fs::rename(target, &backup)?;
    if let Err(err) = fs::rename(&adjacent, target) {
        fs::rename(&backup, target)?;
        return Err(err.into());
    }
    let launch_path = if launcher == Path::new("--file") {
        target.to_path_buf()
    } else {
        target.join(launcher)
    };
    if let Err(err) = Command::new(&launch_path).spawn() {
        remove_path(target)?;
        fs::rename(&backup, target)?;
        let _ = Command::new(&launch_path).spawn();
        return Err(err.into());
    }
    // Keep the last working copy for recovery from a startup failure.
    // The replacement may be an AppImage launched by a mounted old runtime.
    remove_path(replacement)?;
    Ok(())
}
fn copy_tree(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let dest = target.join(entry.file_name());
        ensure!(
            !entry.file_type()?.is_symlink(),
            "Update package contains a symlink"
        );
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

pub fn confirm_startup() -> Result<()> {
    let executable = std::env::current_exe()?;
    let target = if cfg!(target_os = "macos") {
        executable
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .filter(|p| p.extension().is_some_and(|e| e == "app"))
            .map(Path::to_path_buf)
    } else if let Some(path) = std::env::var_os("APPIMAGE") {
        Some(path.into())
    } else {
        executable
            .parent()
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("ToolboxCraft"))
            })
            .map(Path::to_path_buf)
    };
    if let Some(target) = target {
        remove_path(&target.with_extension("toolboxcraft-backup"))?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn helper_restores_working_folder_when_replacement_cannot_launch() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("ToolboxCraft");
        let replacement = d.path().join("replacement");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&replacement).unwrap();
        fs::copy("/usr/bin/true", target.join("toolboxcraft")).unwrap();
        fs::set_permissions(
            target.join("toolboxcraft"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        fs::write(target.join("working"), b"old files").unwrap();
        fs::write(replacement.join("toolboxcraft"), b"not an executable").unwrap();
        let result = apply(&[
            u32::MAX.to_string(),
            target.display().to_string(),
            replacement.display().to_string(),
            "toolboxcraft".into(),
        ]);
        assert!(result.is_err());
        assert_eq!(fs::read(target.join("working")).unwrap(), b"old files");
        assert!(!target.with_extension("toolboxcraft-backup").exists());
    }
}
