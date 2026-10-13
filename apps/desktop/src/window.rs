use crate::tray::TrayAction;
use eframe::egui;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// A second Windows launch should activate the existing instance, not silently exit.
#[cfg(target_os = "windows")]
pub fn activate_existing() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let title: Vec<u16> = "ToolboxCraft\0".encode_utf16().collect();
    // FindWindow is confined to the interactive desktop. Only window visibility
    // and focus change; no application commands or user data are sent.
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd.is_null() || IsWindow(hwnd) == 0 {
            return false;
        }
        ShowWindow(
            hwnd,
            if IsIconic(hwnd) != 0 {
                SW_RESTORE
            } else {
                SW_SHOW
            },
        );
        SetForegroundWindow(hwnd);
        true
    }
}
#[cfg(not(target_os = "windows"))]
pub fn activate_existing() -> bool {
    false
}

/// Window activation must also work when the hidden GUI is not receiving redraws.
#[derive(Clone)]
pub struct WindowControl {
    context: egui::Context,
    quit_requested: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    hwnd: Option<usize>,
}
impl WindowControl {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        #[cfg(target_os = "windows")]
        let hwnd = {
            use raw_window_handle::HasWindowHandle;
            cc.window_handle()
                .ok()
                .and_then(|handle| match handle.as_raw() {
                    raw_window_handle::RawWindowHandle::Win32(handle) => {
                        Some(handle.hwnd.get() as usize)
                    }
                    _ => None,
                })
        };
        Self {
            context: cc.egui_ctx.clone(),
            quit_requested: Arc::new(AtomicBool::new(false)),
            #[cfg(target_os = "windows")]
            hwnd,
        }
    }
    pub fn signal(&self, actions: &std::sync::mpsc::Sender<TrayAction>, action: TrayAction) {
        // Queue before waking so the first restored frame can handle Quit immediately.
        if actions.send(action).is_err() {
            return;
        }
        if matches!(action, TrayAction::Quit) {
            // UI shutdown must not wait for tray refresh or the engine's state lock.
            self.quit_requested.store(true, Ordering::Relaxed);
            tracing::info!("Quit requested from tray");
        }
        #[cfg(target_os = "windows")]
        if let Some(hwnd) = self.hwnd {
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            let hwnd = hwnd as windows_sys::Win32::Foundation::HWND;
            // Tray callbacks run on the window's native event-loop thread. The handle
            // belongs to eframe, and IsWindow protects teardown; we never destroy it here.
            unsafe {
                if IsWindow(hwnd) != 0 {
                    match action {
                        TrayAction::Open => {
                            ShowWindow(
                                hwnd,
                                if IsIconic(hwnd) != 0 {
                                    SW_RESTORE
                                } else {
                                    SW_SHOW
                                },
                            );
                            SetForegroundWindow(hwnd);
                        }
                        TrayAction::Quit => {
                            if IsIconic(hwnd) != 0 {
                                ShowWindow(hwnd, SW_RESTORE);
                            } else if IsWindowVisible(hwnd) == 0 {
                                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                            }
                        }
                    }
                }
            }
        }
        // Keep eframe's visibility commands in sync with the native restore. A
        // pending hide must not prevent the subsequent close frame from running.
        self.context
            .send_viewport_cmd(egui::ViewportCommand::Visible(true));
        self.context
            .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        if matches!(action, TrayAction::Open) {
            self.context.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        self.context.request_repaint();
    }
    pub fn quit_requested(&self) -> bool {
        self.quit_requested.load(Ordering::Relaxed)
    }
    pub fn wake(&self) {
        self.context.request_repaint();
    }
    /// The updater completes on a worker thread, including while hidden to tray.
    pub fn request_restart(&self) {
        self.quit_requested.store(true, Ordering::Relaxed);
        #[cfg(target_os = "windows")]
        if let Some(hwnd) = self.hwnd {
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            let hwnd = hwnd as windows_sys::Win32::Foundation::HWND;
            unsafe {
                if IsWindow(hwnd) != 0 {
                    // Unlike ShowWindow, this does not block the worker on the UI thread.
                    ShowWindowAsync(hwnd, SW_SHOWNOACTIVATE);
                }
            }
        }
        self.context
            .send_viewport_cmd(egui::ViewportCommand::Visible(true));
        self.context
            .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        self.wake();
    }
    pub fn hide(&self) {
        self.context
            .send_viewport_cmd(egui::ViewportCommand::Visible(false));
        #[cfg(target_os = "windows")]
        if let Some(hwnd) = self.hwnd {
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            let hwnd = hwnd as windows_sys::Win32::Foundation::HWND;
            // A second launch can show the HWND while winit still caches it as
            // hidden. In that state Visible(false) alone is a no-op in winit.
            unsafe {
                if IsWindow(hwnd) != 0 {
                    ShowWindow(hwnd, SW_HIDE);
                }
            }
        }
        tracing::info!("Window hidden to tray");
    }
    #[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
    pub fn for_test(context: egui::Context) -> Self {
        Self {
            context,
            quit_requested: Arc::new(AtomicBool::new(false)),
            #[cfg(target_os = "windows")]
            hwnd: None,
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    #[test]
    fn tray_activation_restores_a_hidden_or_minimized_native_window_without_a_gui_frame() {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let title: Vec<u16> = "ToolboxCraft\0".encode_utf16().collect();
        unsafe {
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                200,
                200,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!hwnd.is_null());
            struct OwnedWindow(windows_sys::Win32::Foundation::HWND);
            impl Drop for OwnedWindow {
                fn drop(&mut self) {
                    unsafe {
                        DestroyWindow(self.0);
                    }
                }
            }
            let _window = OwnedWindow(hwnd);
            let control = WindowControl {
                context: egui::Context::default(),
                quit_requested: Arc::new(AtomicBool::new(false)),
                hwnd: Some(hwnd as usize),
            };
            let (actions, events) = std::sync::mpsc::channel();
            assert_eq!(IsWindowVisible(hwnd), 0);
            assert!(activate_existing());
            assert_ne!(IsWindowVisible(hwnd), 0);
            ShowWindow(hwnd, SW_HIDE);
            control.signal(&actions, TrayAction::Open);
            assert_ne!(IsWindowVisible(hwnd), 0);
            assert!(matches!(events.try_recv().unwrap(), TrayAction::Open));
            ShowWindow(hwnd, SW_MINIMIZE);
            assert_ne!(IsIconic(hwnd), 0);
            control.signal(&actions, TrayAction::Open);
            assert_eq!(IsIconic(hwnd), 0);
            assert_ne!(IsWindowVisible(hwnd), 0);
            ShowWindow(hwnd, SW_HIDE);
            control.signal(&actions, TrayAction::Quit);
            assert_ne!(IsWindowVisible(hwnd), 0);
            assert!(control.quit_requested());
            assert!(matches!(events.try_recv().unwrap(), TrayAction::Open));
            assert!(matches!(events.try_recv().unwrap(), TrayAction::Quit));
            // Quit needs a drawable window for both minimized and hidden/minimized
            // states, without depending on an egui frame to restore it first.
            for hide in [false, true] {
                ShowWindow(hwnd, SW_MINIMIZE);
                if hide {
                    ShowWindow(hwnd, SW_HIDE);
                }
                assert_ne!(IsIconic(hwnd), 0);
                control.signal(&actions, TrayAction::Quit);
                assert_eq!(IsIconic(hwnd), 0);
                assert_ne!(IsWindowVisible(hwnd), 0);
                assert!(matches!(events.try_recv().unwrap(), TrayAction::Quit));
            }
            drop(_window);

            // Reproduce the second-launch visibility mismatch with a real winit
            // window: native ShowWindow does not update winit's visibility cache.
            use raw_window_handle::HasWindowHandle;
            use winit::platform::windows::EventLoopBuilderExtWindows;
            let event_loop = winit::event_loop::EventLoop::builder()
                .with_any_thread(true)
                .build()
                .unwrap();
            #[allow(deprecated)] // Test synchronous visibility without running a GUI loop.
            let window = event_loop
                .create_window(
                    winit::window::Window::default_attributes().with_title("ToolboxCraft"),
                )
                .unwrap();
            window.set_visible(false);
            assert_eq!(window.is_visible(), Some(false));
            assert!(activate_existing());
            assert_eq!(window.is_visible(), Some(true));
            window.set_visible(false);
            assert_eq!(
                window.is_visible(),
                Some(true),
                "expected the stale-cache reproduction"
            );
            let raw_window_handle::RawWindowHandle::Win32(handle) =
                window.window_handle().unwrap().as_raw()
            else {
                panic!("Expected a Win32 window");
            };
            let control = WindowControl {
                context: egui::Context::default(),
                quit_requested: Arc::new(AtomicBool::new(false)),
                hwnd: Some(handle.hwnd.get() as usize),
            };
            control.hide();
            assert_eq!(window.is_visible(), Some(false));
            assert!(!control.quit_requested());
            assert!(activate_existing());
            assert_eq!(window.is_visible(), Some(true));
            control.hide();
            assert_eq!(window.is_visible(), Some(false));
        }
    }
}
