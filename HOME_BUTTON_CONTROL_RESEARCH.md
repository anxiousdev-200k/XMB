# Windows Controller Home/Guide Button Research

**Status:** Best-effort Home/Guide and Select+Start controls are implemented; controller-specific validation remains necessary.
**Research date:** 2026-10-31
**Scope:** Whether MaXMB can take over a controller's Xbox/Home/Guide button from Windows and provide a console-like return-to-XMB flow.

## Executive conclusion

MaXMB can plausibly use the controller's Home/Guide button on supported controllers, but a normal desktop application has no documented Windows API that registers exclusive ownership of that system button. Reading the button and suppressing Windows' Game Bar action are separate problems.

The practical first attempt is:

1. Have the user disable the Windows setting that opens Xbox Game Bar from the controller's Xbox button.
2. Prototype reading the button per controller model/transport, starting with XInput's undocumented `XInputGetStateEx` and checking raw HID where needed.
3. If this succeeds, use the button to show an XMB quick menu or return focus to XMB while leaving the game process running.

This may provide a seamless experience on a tested PC/controller configuration, but cannot currently be promised across all controllers, drivers, games, Windows versions, or exclusive-fullscreen modes. True system-wide suppression likely needs controller remapping software or a privileged HID filter/virtual-controller driver, which is a substantial, optional dependency and should not be the default MaXMB design.

## What the public Windows APIs establish

### XInput can read controllers, but its documented guide-button contract is absent

Microsoft documents `XInputGetState` as a way to retrieve a controller state. Its documented `XINPUT_GAMEPAD.wButtons` bits include the ordinary face, shoulder, D-pad, start/back, and stick buttons; remaining bits are reserved/undefined. There is no documented Guide/Home bit in this public contract.

References:

- [XInputGetState](https://learn.microsoft.com/en-us/windows/win32/api/xinput/nf-xinput-xinputgetstate)
- [XINPUT_GAMEPAD](https://learn.microsoft.com/en-us/windows/win32/api/xinput/ns-xinput-xinput_gamepad)

Several widely used non-Microsoft headers and libraries define a `0x0400` Guide bit and obtain extended state through an export commonly called `XInputGetStateEx`. This is evidence that some Windows/controller configurations expose the button through an implementation detail; it is **not** a supported public contract. It can vary or break, and it does not by itself stop Game Bar from responding.

### GameInput does not make exclusive Guide ownership evident

Microsoft describes GameInput as a broad, modern input API that supersedes the legacy input families. The public `GameInputGamepadButtons` enumeration in the Windows SDK includes normal gamepad controls but has no named Guide-button value in the inspected public header. This means adopting GameInput is not, on its own, a substantiated fix for Guide-button interception.

- [XInput and Game Controller APIs](https://learn.microsoft.com/en-us/windows/win32/xinput/xinput-game-controller-apis-portal)
- [Windows SDK GameInput header](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/RecompiledIdlHeaders/um/GameInput.h)

### Raw Input/HID is not an exclusive system hotkey mechanism

`RegisterRawInputDevices` registers a process window to receive raw device messages. Microsoft notes that only one window per device class is the receiving window within a process, and warns libraries not to register because they can interfere with other raw-input code. This API is not documented as a way to claim a system button or cancel Windows shell actions.

HID can be useful for inspecting a physical controller's report, but which report exposes Guide depends on controller, connection mode, and driver. Opening a HID path to read data should not be equated with suppressing the Windows/Game Bar action.

- [RegisterRawInputDevices](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerrawinputdevices)
- [RAWINPUTDEVICE](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawinputdevice)

### Keyboard global-hotkey APIs do not cover controller buttons

`RegisterHotKey` defines keyboard hotkeys and delivers `WM_HOTKEY`; it does not register gamepad controls. A global keyboard chord is a reasonable fallback, not an Xbox-button equivalent.

- [RegisterHotKey](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey)

## Feasibility by goal

| Goal | Assessment | Notes |
|---|---|---|
| Detect Home/Guide on one known Xbox controller setup | **Worth prototyping** | Probe `XInputGetStateEx` and/or the physical HID report. Treat the undocumented XInput route as experimental. |
| Prevent Game Bar from opening when Guide is pressed | **Likely achievable by user configuration** | Have the user disable the Xbox-button/Game-Bar association in Windows Settings. Verify on the exact Windows build; this is configuration, not MaXMB taking exclusive ownership. |
| Make MaXMB the only app that reacts to Guide without user configuration | **Not established through supported desktop APIs** | No documented app-level exclusive Guide registration/suppression API was found. |
| Suppress the physical controller from Windows and provide a virtual controller to games | **Technically possible in principle; high cost/risk** | Requires a filter/hide and virtual-device path, commonly involving a kernel driver, admin installation, signing/compatibility work, recovery/uninstall UX, and testing against games/anti-cheat. Do not silently install or require this for MaXMB. |
| Show an overlay over any game | **Not guaranteed** | A topmost borderless window is most plausible over borderless-windowed games. Exclusive fullscreen, elevated games, anti-cheat, secure desktop, and display mode can prevent or complicate overlays. |
| Return to XMB while leaving a game alive | **Feasible in MaXMB's current process model** | The current watchdog tracks the game process and intentionally reclaims focus. Add an explicit “game still running / XMB in foreground” state so the watcher continues detecting exit but does not force the game back to foreground while the quick menu/XMB is active. |

## Current MaXMB integration points

- [`src-tauri/src/focus.rs`](./src-tauri/src/focus.rs): `watch_game` tracks launched processes and reclaims foreground if XMB takes focus; `start_pad_watch` intentionally ignores controller input while `in_game()`.
- [`src-tauri/src/hidpad.rs`](./src-tauri/src/hidpad.rs): native HID reader supports Sony DualShock/DualSense devices and already emits controller state. It is not currently an Xbox Guide listener.
- [`src-tauri/src/gamepad.rs`](./src-tauri/src/gamepad.rs): native gamepad reader used by the focus watchdog; inspect its current platform/backend before extending it.
- [`src-tauri/src/lib.rs`](./src-tauri/src/lib.rs): `launch_game` starts `focus::watch_game`; `game_mode_end` stops game mode.
- [`src/js/main.js`](./src/js/main.js): `dispatch()` rejects all input during `body.in-game`; gamepad polling also idles during game mode.
- [`src/js/xmb.js`](./src/js/xmb.js): `_launch()` enters game mode and restores XMB when focus returns or the game-exited event fires.

Avoid changing `in_game` to mean “ignore all game tracking.” The process watcher must continue detecting game exit even while the user is in XMB. Separate these concepts, for example:

- `game_running`: a tracked game process is alive.
- `game_foreground_policy`: whether the watcher should reclaim focus.
- `quick_menu_open` / `xmb_returned`: controller/menu interaction while the game remains alive.

## Recommended implementation plan

### Phase 0 — collect target configuration

Before implementation/testing, identify:

- Exact controller model and firmware.
- USB, Bluetooth, Xbox Wireless Adapter, or third-party receiver.
- Whether Steam Input, reWASD, DS4Windows, or other remappers are active.
- Windows version/build and whether Xbox Game Bar's controller shortcut is enabled.
- Whether target games are borderless-windowed or exclusive fullscreen; whether any are elevated or use anti-cheat.

Do not assume results from a DualSense/Sony HID report apply to an Xbox controller.

### Phase 1 — read-only capability probe

Build a small, opt-in diagnostic or temporary debug path that reports:

1. Controller enumeration and backend (XInput/HID/other).
2. Button transitions from documented XInput state.
3. Button transitions from `XInputGetStateEx` if the export exists, loaded dynamically and treated as optional.
4. HID report bytes for the known physical device, if accessible, without changing device visibility or installing drivers.
5. Whether Guide still opens Game Bar after the user disables the relevant Windows setting.

Do not log raw controller reports continuously in production. Do not treat undocumented API absence as an app failure. Record test results per model/transport.

### Phase 2 — user-configured Guide shortcut

If Phase 1 can reliably detect Guide after Game Bar's shortcut is disabled:

- Add a Settings item describing the Windows prerequisite and a diagnostic/test action.
- While no game is running, let Guide open an XMB quick menu.
- While a game is running, keep the process watcher alive but suspend its foreground-reclaim behavior; show the same XMB menu or return to XMB without terminating the game.
- Add explicit **Resume Game**, **Quit Game**, and **Quit to Desktop** actions, with confirmation before terminating.
- Track the process tree and launcher/game handoff; avoid killing only a launcher PID or unrelated process.
- Make a keyboard shortcut configurable as a fallback and document that it does not capture Guide.

### Phase 3 — optional exclusive device-driver path (only if demanded)

Evaluate an established, maintained controller-hiding/virtualization stack as an optional add-on, not new custom kernel code. Verify:

- The physical Guide press is available to MaXMB while hidden from Game Bar/other apps.
- Other controller buttons are re-exposed to games through a virtual controller.
- Multiple controllers and reconnects work.
- Driver signing, Windows updates, anti-cheat compatibility, elevation, uninstall and recovery are acceptable.
- Users can recover input if MaXMB crashes.

If these requirements cannot be met cleanly, stop at the Windows-setting + read-only detection approach.

## Explicit non-goals / cautions

- Do not inject into games, hook game processes, or attempt to bypass anti-cheat.
- Do not patch Windows shell/Game Bar binaries or modify protected system state.
- Do not install a kernel filter or virtual-device driver without explicit opt-in, clear provenance, administrative consent, and a tested uninstall/recovery path.
- Do not promise that the Xbox button will always open MaXMB; Windows and vendor software can reserve or consume it first.
- Do not promise overlays over exclusive-fullscreen or protected content.
- Do not terminate the tracked process tree on a single input without confirmation and process identity validation.

## Current implementation and handoff

MaXMB now probes XInput's optional ordinal-100 `XInputGetStateEx` export for the Guide bit and reads the Sony PS-button bit from its native HID path. While a game is tracked, the native watcher recognizes Guide and Select+Start, returns MaXMB to the foreground without stopping the game watcher, and suppresses automatic focus reclamation while MaXMB is intentionally displayed. The Game column then places **Quit Game** first and focuses it. Selecting another game attempts to close the tracked game before launching the new one.

The centered, opaque Quick Menu contains **Resume Game**, **System Volume**, **Output Device**, **Rest Mode**, and **Turn Off System**. Volume uses Windows' master-volume keys; Output Device opens Windows Sound settings rather than embedding a device picker. Rest Mode requests Windows sleep. Quit Game and system shutdown have confirmation steps.

Known constraints remain:

- The XInput Guide path is undocumented and depends on the controller/runtime. Sony HID and Xbox controller handling must be tested separately.
- Disable the Xbox Game Bar controller shortcut in Windows; MaXMB cannot claim exclusive Guide ownership, and Windows may still react first.
- Select+Start is observed, not consumed, so the game may also see it.
- The menu is a foreground XMB screen, not an overlay guaranteed to work over exclusive fullscreen. Press Guide again to return; the game process continues to be watched.
- `taskkill` is used only for tracked PIDs whose current executable path still matches the recorded process identity. Windows permissions or protected games may still prevent termination.

Test on the intended Windows build with the Xbox Bluetooth controller and DS4Windows separately, with Game Bar's controller shortcut disabled. Confirm Guide detection, Select+Start return, resume, game exit, game switching, volume, Sound Settings, sleep, and shutdown. Do not describe this as exclusive system-level Home-button takeover.
