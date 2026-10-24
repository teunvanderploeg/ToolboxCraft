#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod markdown;
mod tray;
mod ui;
mod window;
use std::sync::Arc;
use toolboxcraft_core::{
    engine::{Command, Engine},
    store::Store,
};
fn main() -> anyhow::Result<()> {
    let store = Store::standard()?;
    let preview = std::env::args().any(|a| a == "--preview");
    let _lock = if preview {
        None
    } else {
        match store.lock() {
            Ok(lock) => Some(lock),
            Err(_) if window::activate_existing() => return Ok(()),
            Err(err) => return Err(err),
        }
    };
    let log_dir = store.root.join("logs");
    std::fs::create_dir_all(&log_dir)?;
    let writer = tracing_appender::rolling::daily(log_dir, "toolboxcraft.log");
    let (writer, _log_guard) = tracing_appender::non_blocking(writer);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(writer)
        .with_ansi(false)
        .init();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let native = toolboxcraft_platform::Native::new(store.clone())?;
    std::fs::create_dir_all(store.root.join("icons"))?;
    for app in toolboxcraft_core::catalog::CATALOG {
        std::fs::write(
            store.root.join("icons").join(format!("{}.png", app.id)),
            ui::icon_bytes(app.id),
        )?;
    }
    let args: Vec<String> = std::env::args().collect();
    let preview = args.iter().any(|a| a == "--preview");
    let screenshot = args
        .windows(2)
        .find(|a| a[0] == "--screenshot")
        .map(|a| std::path::PathBuf::from(&a[1]));
    let scale = args
        .windows(2)
        .find(|a| a[0] == "--scale")
        .and_then(|a| a[1].parse::<f32>().ok());
    let (handle, events) = if preview {
        let (commands, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let (_sender, events) = std::sync::mpsc::channel();
        (
            toolboxcraft_core::engine::EngineHandle {
                commands,
                state: Arc::new(std::sync::Mutex::new(
                    toolboxcraft_core::model::State::default(),
                )),
            },
            events,
        )
    } else {
        Engine::start(store, Arc::new(native), &runtime)?
    };
    if preview {
        ui::preview_state(&handle);
        if args
            .windows(2)
            .any(|a| a[0] == "--theme" && a[1] == "light")
        {
            handle.state.lock().unwrap().settings.theme = toolboxcraft_core::model::Theme::Light;
        }
    }
    let background = args.iter().any(|a| a == "--background");
    let close_handle = handle.clone();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("ToolboxCraft")
            .with_inner_size([460.0, 720.0])
            .with_min_inner_size([400.0, 480.0])
            .with_icon(Arc::new(tray::window_icon())),
        ..Default::default()
    };
    eframe::run_native(
        "ToolboxCraft",
        options,
        Box::new(move |cc| {
            Ok(Box::new(ui::Toolbox::new(
                cc, handle, events, background, preview, screenshot, scale,
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    tracing::info!("Window event loop stopped; shutting down background workers");
    let _ = close_handle.commands.send(Command::Shutdown);
    runtime.shutdown_timeout(std::time::Duration::from_secs(3));
    tracing::info!("ToolboxCraft shutdown complete");
    Ok(())
}
