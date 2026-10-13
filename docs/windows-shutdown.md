# Windows window-close and shutdown regression checks

The window's X button and Alt+F4 hide ToolboxCraft when its tray is available.
To exit the process, select **Quit** from the tray menu.

## Automated checks

Run these on Windows:

```powershell
cargo test -p toolboxcraft --locked quit_
cargo test -p toolboxcraft --locked x_button
cargo test -p toolboxcraft --locked queued_tray_open
cargo test -p toolboxcraft --locked tray_activation_restores_a_hidden_or_minimized_native_window_without_a_gui_frame
```

The UI tests hold the engine state lock while requesting shutdown, and combine
Quit with pending hide commands and minimized/close input over successive frames.
The state-lock test fails against the previous UI shutdown code. The native test
checks hidden, minimized, and hidden/minimized Win32 windows without a GUI redraw.
It also reproduces the visibility-cache mismatch with a real winit window: after
the second-launch path shows a hidden HWND, winit's `set_visible(false)` can leave
it visible. The explicit native hide must hide it successfully, including after
another second-launch activation.
The X-button UI tests check hiding while the engine state lock is busy, rendering
from the last snapshot while it stays busy, delayed tray Open actions, and normal
exit when no tray is available.
These checks cover shutdown decisions and restoration; they do not exercise the
complete tray menu and graphics event loop together.

## Full application check

Build with `cargo build -p toolboxcraft --locked`, then run
`target\debug\toolboxcraft.exe`.

First check the reported X-button behavior:

1. Click X. The window should disappear and the tray icon should remain.
2. Launch `toolboxcraft.exe` again. The existing window should become visible;
   Task Manager should still show one launcher process.
3. Click X again. It should disappear. Repeat the launch/X sequence several times.
4. Repeat using the tray icon to restore the window instead of launching again.
5. Repeat while checking for updates or downloading an application update.

The second-launch sequence can show the native window without updating the UI
framework's cached visibility. X now explicitly hides the native window and runs
before tray refresh or shared-state access. Normal Windows UI frames and tray
refreshes use nonblocking state reads so background disk writes cannot hold up
the native message loop.

Then check full shutdown, using a fresh process for each case:

1. Leave the window visible, then right-click the tray icon and select **Quit**.
2. Click X to hide the window, then select **Quit** from the tray.
3. Minimize the window, then select **Quit** from the tray.
4. Launch with `--background`, then select **Quit** without opening the window.
5. Repeat the hidden and minimized cases while checking for updates or downloading
   an application update.

After Quit, the tray icon should disappear and `toolboxcraft.exe` should leave
Task Manager. Allow roughly five seconds for the window event loop to stop and
the existing three-second background-worker shutdown timeout. Also check that X
still hides to the tray and that clicking the tray icon restores the window.

## If shutdown still stalls

Start from PowerShell with `$env:RUST_LOG = 'toolboxcraft=debug,eframe=debug'`.
Logs are under `%LOCALAPPDATA%\ToolboxCraft\ToolboxCraft\data\logs`.
The following messages identify how far shutdown got:

- `Window hidden to tray`: the X-button handler ran and requested hiding.
- `Quit requested from tray`: the native tray callback received the command.
- `Closing ToolboxCraft`: the UI received the persistent Quit signal.
- `Window event loop stopped; shutting down background workers`: native window
  teardown finished.
- `ToolboxCraft shutdown complete`: the worker shutdown timeout returned.

Record the last message, whether the window was hidden or minimized, and whether
an install/update was active. This distinguishes an unresponsive tray callback,
a stalled UI/graphics loop, and background-worker teardown.
