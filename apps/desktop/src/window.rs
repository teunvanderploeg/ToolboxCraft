use crate::tray::TrayAction;
use eframe::egui;

/// Window activation must also work when the hidden GUI is not receiving redraws.
#[derive(Clone)]
pub struct WindowControl {
    context: egui::Context,
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
            #[cfg(target_os = "windows")]
            hwnd,
        }
    }
    pub fn signal(&self, actions: &std::sync::mpsc::Sender<TrayAction>, action: TrayAction) {
        // Queue before waking so the first restored frame can handle Quit immediately.
        if actions.send(action).is_err() {
            return;
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
                            if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
                                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                            }
                        }
                    }
                }
            }
        }
        if matches!(action, TrayAction::Open) {
            self.context
                .send_viewport_cmd(egui::ViewportCommand::Visible(true));
            self.context
                .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            self.context.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        self.context.request_repaint();
    }
    #[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
    pub fn for_test(context: egui::Context) -> Self {
        Self {
            context,
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
        unsafe {
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
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
                hwnd: Some(hwnd as usize),
            };
            let (actions, events) = std::sync::mpsc::channel();
            assert_eq!(IsWindowVisible(hwnd), 0);
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
            assert!(matches!(events.try_recv().unwrap(), TrayAction::Open));
            assert!(matches!(events.try_recv().unwrap(), TrayAction::Quit));
        }
    }
}
