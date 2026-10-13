use crate::window::WindowControl;
use eframe::egui;
use toolboxcraft_core::engine::{Command, EngineHandle};
#[derive(Clone, Copy)]
pub enum TrayAction {
    Open,
    Quit,
}
pub trait TrayEvents {
    fn poll(&mut self) -> Vec<TrayAction>;
}
pub fn window_icon() -> egui::IconData {
    let image = image::load_from_memory(include_bytes!("../../../assets/icons/toolboxcraft.png"))
        .unwrap()
        .into_rgba8();
    egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    }
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct Tray {
    icon: tray_icon::TrayIcon,
    handle: EngineHandle,
    fingerprint: Vec<String>,
    actions: std::sync::mpsc::Receiver<TrayAction>,
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Tray {
    pub fn new(handle: EngineHandle, window: WindowControl) -> anyhow::Result<Self> {
        let pixels = window_icon();
        let image = tray_icon::Icon::from_rgba(pixels.rgba, pixels.width, pixels.height)?;
        let builder = tray_icon::TrayIconBuilder::new()
            .with_tooltip("ToolboxCraft")
            .with_icon(image)
            .with_menu(Box::new(tray_icon::menu::Menu::new()));
        // Windows left-click activates the app; right-click opens the menu.
        // Keep the standard menu-bar behavior on macOS.
        #[cfg(target_os = "windows")]
        let builder = builder.with_menu_on_left_click(false);
        let icon = builder.build()?;
        let (tx, actions) = std::sync::mpsc::channel();
        let window2 = window.clone();
        let tx2 = tx.clone();
        let engine = handle.clone();
        tray_icon::menu::MenuEvent::set_event_handler(Some(
            move |event: tray_icon::menu::MenuEvent| {
                route_menu(&engine, &tx2, &window2, event.id.as_ref());
            },
        ));
        tray_icon::TrayIconEvent::set_event_handler(Some(move |event| {
            if activates_window(&event) {
                window.signal(&tx, TrayAction::Open);
            }
        }));
        let mut result = Self {
            icon,
            handle,
            fingerprint: vec![],
            actions,
        };
        let ids = result
            .handle
            .state
            .try_lock()
            .map(|state| state.installations.keys().cloned().collect())
            .unwrap_or_default();
        result.refresh(ids)?;
        Ok(result)
    }
    fn refresh(&mut self, ids: Vec<String>) -> anyhow::Result<()> {
        use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
        let menu = Menu::new();
        menu.append(&MenuItem::with_id("open", "Open ToolboxCraft", true, None))?;
        menu.append(&PredefinedMenuItem::separator())?;
        for id in &ids {
            if let Some(app) = toolboxcraft_core::catalog::by_id(id) {
                menu.append(&MenuItem::with_id(
                    format!("launch:{id}"),
                    app.name,
                    true,
                    None,
                ))?;
            }
        }
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&MenuItem::with_id("check", "Check for updates", true, None))?;
        menu.append(&MenuItem::with_id("quit", "Quit", true, None))?;
        self.icon.set_menu(Some(Box::new(menu)));
        self.fingerprint = ids;
        Ok(())
    }
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
impl TrayEvents for Tray {
    fn poll(&mut self) -> Vec<TrayAction> {
        let actions: Vec<_> = self.actions.try_iter().collect();
        let ids = self
            .handle
            .state
            .try_lock()
            .ok()
            .map(|state| state.installations.keys().cloned().collect::<Vec<_>>());
        if let Some(ids) = ids
            && ids != self.fingerprint
        {
            let _ = self.refresh(ids);
        }
        actions
    }
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn route_menu(
    handle: &EngineHandle,
    actions: &std::sync::mpsc::Sender<TrayAction>,
    window: &WindowControl,
    id: &str,
) {
    match id {
        "open" => {
            window.signal(actions, TrayAction::Open);
        }
        "quit" => {
            window.signal(actions, TrayAction::Quit);
        }
        "check" => {
            let _ = handle.commands.send(Command::Check);
        }
        id => {
            if let Some(id) = id.strip_prefix("launch:")
                && toolboxcraft_core::catalog::by_id(id).is_some()
            {
                let _ = handle.commands.send(Command::Launch(id.into()));
            }
        }
    }
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn activates_window(event: &tray_icon::TrayIconEvent) -> bool {
    matches!(
        event,
        tray_icon::TrayIconEvent::Click {
            button: tray_icon::MouseButton::Left,
            button_state: tray_icon::MouseButtonState::Up,
            ..
        } | tray_icon::TrayIconEvent::DoubleClick {
            button: tray_icon::MouseButton::Left,
            ..
        }
    )
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Drop for Tray {
    fn drop(&mut self) {
        tray_icon::menu::MenuEvent::set_event_handler::<fn(tray_icon::menu::MenuEvent)>(None);
        tray_icon::TrayIconEvent::set_event_handler::<fn(tray_icon::TrayIconEvent)>(None);
    }
}
#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod tests {
    use super::*;
    #[test]
    fn only_left_release_and_left_double_click_activate_the_window() {
        use tray_icon::{MouseButton, MouseButtonState, Rect, TrayIconEvent};
        let event = |button, button_state| TrayIconEvent::Click {
            id: "test".into(),
            position: Default::default(),
            rect: Rect::default(),
            button,
            button_state,
        };
        assert!(activates_window(&event(
            MouseButton::Left,
            MouseButtonState::Up
        )));
        assert!(!activates_window(&event(
            MouseButton::Left,
            MouseButtonState::Down
        )));
        assert!(!activates_window(&event(
            MouseButton::Right,
            MouseButtonState::Up
        )));
        assert!(!activates_window(&event(
            MouseButton::Middle,
            MouseButtonState::Up
        )));
        assert!(activates_window(&TrayIconEvent::DoubleClick {
            id: "test".into(),
            position: Default::default(),
            rect: Rect::default(),
            button: MouseButton::Left,
        }));
        assert!(!activates_window(&TrayIconEvent::DoubleClick {
            id: "test".into(),
            position: Default::default(),
            rect: Rect::default(),
            button: MouseButton::Right,
        }));
    }
    #[test]
    fn tray_callbacks_route_menu_commands_and_window_actions() {
        let (commands, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = EngineHandle {
            commands,
            state: std::sync::Arc::new(std::sync::Mutex::new(
                toolboxcraft_core::model::State::default(),
            )),
        };
        let window = WindowControl::for_test(egui::Context::default());
        let (actions, window_events) = std::sync::mpsc::channel();
        route_menu(&handle, &actions, &window, "check");
        assert!(matches!(receiver.try_recv().unwrap(), Command::Check));
        route_menu(&handle, &actions, &window, "launch:photocraft");
        assert!(matches!(receiver.try_recv().unwrap(),Command::Launch(id) if id=="photocraft"));
        route_menu(&handle, &actions, &window, "open");
        assert!(matches!(
            window_events.try_recv().unwrap(),
            TrayAction::Open
        ));
        route_menu(&handle, &actions, &window, "quit");
        assert!(matches!(
            window_events.try_recv().unwrap(),
            TrayAction::Quit
        ));
    }
}
#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::sync::mpsc;
    struct Service {
        engine: EngineHandle,
        actions: mpsc::Sender<TrayAction>,
        window: WindowControl,
    }
    impl Service {
        fn signal(&self, action: TrayAction) {
            self.window.signal(&self.actions, action);
        }
    }
    impl ksni::Tray for Service {
        fn id(&self) -> String {
            "toolboxcraft".into()
        }
        fn title(&self) -> String {
            "ToolboxCraft".into()
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            let p = window_icon();
            let mut bytes = p.rgba;
            for pixel in bytes.as_chunks_mut::<4>().0 {
                pixel.rotate_right(1);
            }
            vec![ksni::Icon {
                width: p.width as i32,
                height: p.height as i32,
                data: bytes,
            }]
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            self.signal(TrayAction::Open);
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::StandardItem;
            let mut menu = vec![
                StandardItem {
                    label: "Open ToolboxCraft".into(),
                    activate: Box::new(|s: &mut Self| s.signal(TrayAction::Open)),
                    ..Default::default()
                }
                .into(),
                ksni::MenuItem::Separator,
            ];
            for id in self.engine.state.lock().unwrap().installations.keys() {
                if let Some(app) = toolboxcraft_core::catalog::by_id(id) {
                    let id = id.clone();
                    menu.push(
                        StandardItem {
                            label: app.name.into(),
                            activate: Box::new(move |s: &mut Self| {
                                let _ = s.engine.commands.send(Command::Launch(id.clone()));
                            }),
                            ..Default::default()
                        }
                        .into(),
                    );
                }
            }
            menu.push(ksni::MenuItem::Separator);
            menu.push(
                StandardItem {
                    label: "Check for updates".into(),
                    activate: Box::new(|s: &mut Self| {
                        let _ = s.engine.commands.send(Command::Check);
                    }),
                    ..Default::default()
                }
                .into(),
            );
            menu.push(
                StandardItem {
                    label: "Quit".into(),
                    activate: Box::new(|s: &mut Self| s.signal(TrayAction::Quit)),
                    ..Default::default()
                }
                .into(),
            );
            menu
        }
    }
    pub struct LinuxTray {
        handle: ksni::blocking::Handle<Service>,
        actions: mpsc::Receiver<TrayAction>,
    }
    impl LinuxTray {
        pub fn new(engine: EngineHandle, window: WindowControl) -> anyhow::Result<Self> {
            use ksni::blocking::TrayMethods;
            let (tx, actions) = mpsc::channel();
            let handle = Service {
                engine,
                actions: tx,
                window,
            }
            .spawn()?;
            Ok(Self { handle, actions })
        }
    }
    impl TrayEvents for LinuxTray {
        fn poll(&mut self) -> Vec<TrayAction> {
            self.handle.update(|_| {});
            self.actions.try_iter().collect()
        }
    }
}
#[cfg(target_os = "linux")]
pub use linux::LinuxTray as Tray;
