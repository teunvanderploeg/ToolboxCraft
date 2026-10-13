use crate::{Native, find_executable};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use toolboxcraft_core::{
    catalog::App,
    model::{Installation, Os},
    store::atomic_write,
};

pub fn detect(native: &Native, app: App) -> Option<Installation> {
    let base = directories::BaseDirs::new()?;
    let mut paths = match native.target.os {
        Os::Mac => vec![
            base.home_dir()
                .join(format!("Applications/{}.app", app.name)),
            PathBuf::from(format!("/Applications/{}.app", app.name)),
        ],
        Os::Windows => {
            let mut paths = Vec::new();
            for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
                if let Some(root) = std::env::var_os(variable) {
                    let root = PathBuf::from(root);
                    paths.push(root.join(app.name));
                    paths.push(root.join("Programs").join(app.name));
                }
            }
            paths
        }
        Os::Linux => vec![
            PathBuf::from(format!("/usr/bin/{}", app.id)),
            PathBuf::from(format!("/usr/local/bin/{}", app.id)),
            base.home_dir()
                .join(format!("Applications/{}.AppImage", app.name)),
            base.home_dir()
                .join(format!("Applications/{}.AppImage", app.id)),
        ],
    };
    if native.target.os == Os::Windows {
        paths.retain(|p| find_executable(p, &format!("{}.exe", app.id)).is_some());
    }
    paths.into_iter().find(|p| p.exists()).map(|path| {
        let version = if native.target.os == Os::Mac {
            mac_version(&path).unwrap_or_else(|| "Unknown".into())
        } else {
            "Unknown".into()
        };
        Installation {
            version,
            path,
            managed: false,
        }
    })
}
fn mac_version(path: &Path) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let o = Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", "Print :CFBundleShortVersionString"])
            .arg(path.join("Contents/Info.plist"))
            .output()
            .ok()?;
        if o.status.success() {
            return Some(String::from_utf8_lossy(&o.stdout).trim().into());
        }
    }
    let _ = path;
    None
}
fn shortcut_path(native: &Native, app: App) -> Result<PathBuf> {
    if cfg!(test) {
        return Ok(native
            .store
            .root
            .join("test-shortcuts")
            .join(format!("{}.desktop", app.id)));
    }
    let base = directories::BaseDirs::new().context("Cannot locate home directory")?;
    if native.target.os == Os::Windows {
        let root = std::env::var_os("APPDATA").context("APPDATA is not set")?;
        Ok(PathBuf::from(root)
            .join("Microsoft/Windows/Start Menu/Programs/ToolboxCraft")
            .join(format!("{}.lnk", app.name)))
    } else {
        Ok(base
            .data_dir()
            .join("applications")
            .join(format!("toolboxcraft-{}.desktop", app.id)))
    }
}
pub(crate) fn shortcut(native: &Native, app: App) -> Result<()> {
    if native.target.os == Os::Mac {
        return Ok(());
    }
    let target = native.executable(app, &native.app_path(app)?)?;
    let shortcut = shortcut_path(native, app)?;
    fs::create_dir_all(shortcut.parent().unwrap())?;
    if native.target.os == Os::Windows {
        let script = "$s=(New-Object -ComObject WScript.Shell).CreateShortcut($env:TC_SHORTCUT);$s.TargetPath=$env:TC_TARGET;$s.WorkingDirectory=[IO.Path]::GetDirectoryName($env:TC_TARGET);$s.Save()";
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("TC_SHORTCUT", shortcut)
            .env("TC_TARGET", target)
            .output()?;
        ensure!(
            output.status.success(),
            "Cannot create Start menu shortcut: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        let text = format!(
            "[Desktop Entry]\nType=Application\nName={}\nExec={}\nIcon={}\nTerminal=false\nCategories=Graphics;\n",
            app.name,
            desktop_quote(&target)?,
            native
                .store
                .root
                .join("icons")
                .join(format!("{}.png", app.id))
                .display()
        );
        atomic_write(&shortcut, text.as_bytes())?;
    }
    Ok(())
}
pub(crate) fn remove_shortcut(native: &Native, app: App) -> Result<()> {
    if native.target.os != Os::Mac {
        let path = shortcut_path(native, app)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}
fn desktop_quote(path: &Path) -> Result<String> {
    let text = path
        .to_str()
        .context("Application path is not valid UTF-8")?;
    ensure!(!text.contains(['\n', '\r']), "Invalid application path");
    Ok(format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('`', "\\`")
            .replace('$', "\\$")
            .replace('%', "%%")
    ))
}
pub fn launch_at_login(native: &Native, enabled: bool) -> Result<()> {
    let executable = std::env::current_exe()?;
    let base = directories::BaseDirs::new().context("Cannot locate home directory")?;
    match native.target.os {
        Os::Mac => {
            let path = base
                .home_dir()
                .join("Library/LaunchAgents/dev.toolboxcraft.launcher.plist");
            if !enabled {
                if path.exists() {
                    fs::remove_file(path)?;
                }
                return Ok(());
            }
            fs::create_dir_all(path.parent().unwrap())?;
            let xml_path = executable
                .to_string_lossy()
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;");
            atomic_write(&path, format!("<?xml version=\"1.0\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>Label</key><string>dev.toolboxcraft.launcher</string><key>ProgramArguments</key><array><string>{xml_path}</string><string>--background</string></array><key>RunAtLoad</key><true/></dict></plist>").as_bytes())?;
        }
        Os::Windows => {
            let mut command = Command::new("reg.exe");
            if enabled {
                command
                    .args([
                        "add",
                        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                        "/v",
                        "ToolboxCraft",
                        "/t",
                        "REG_SZ",
                        "/d",
                    ])
                    .arg(format!("\"{}\" --background", executable.display()))
                    .arg("/f");
            } else {
                command.args([
                    "delete",
                    "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                    "/v",
                    "ToolboxCraft",
                    "/f",
                ]);
            }
            let output = command.output()?;
            if enabled {
                ensure!(output.status.success(), "Could not enable launch at login");
            }
        }
        Os::Linux => {
            let path = base.config_dir().join("autostart/toolboxcraft.desktop");
            if !enabled {
                if path.exists() {
                    fs::remove_file(path)?;
                }
                return Ok(());
            }
            fs::create_dir_all(path.parent().unwrap())?;
            atomic_write(&path, format!("[Desktop Entry]\nType=Application\nName=ToolboxCraft\nExec={} --background\nTerminal=false\n", desktop_quote(&executable)?).as_bytes())?;
        }
    }
    Ok(())
}
pub fn notify(title: &str, message: &str) {
    let title = title.to_string();
    let message = message.to_string();
    std::thread::spawn(move || {
        if let Err(err) = notify_rust::Notification::new()
            .summary(&title)
            .body(&message)
            .appname("ToolboxCraft")
            .show()
        {
            tracing::debug!("Desktop notification unavailable: {err}");
        }
    });
}
