# Controller overlay

Settings → Controller overlay opens a separate transparent, frameless window showing the selected built-in or imported controller with real engine input. There is no background, backlight, telemetry panel or trigger percentage badge. The main controller view retains its original glow, stick movement and click feedback.

- Drag with the left mouse button to move the window.
- Drag any edge/corner to resize; the artwork retains its proportions.
- Always on top keeps it above other ordinary windows.
- Lock overlay enables mouse passthrough; unlock in Settings to move/resize again.
- Size and position are saved. Reset restores the initial size and moves it near the main window.
- Right-click the unlocked overlay to close it, or disable it in Settings.
- The deferred viewport repaints independently at up to 60 fps, reading current physical input on every frame, including when the main window is unfocused/minimized.

For Discord, share the screen containing both the content and overlay. Capturing only a game window may exclude an independent overlay window. The app does not inject graphics into another application's rendering. Capture behavior of transparent windows depends on the capture software.

Implementation: `src/overlay.rs`, persisted `model::OverlaySettings`, settings page in `ui.rs`, and `pad::paint_overlay`. Existing settings deserialize with the overlay disabled. No new dependencies.

## Input and settings placement

Built-in overlays use an alpha-derived mouse hit test. Empty background areas and the gap between the grips pass mouse clicks through, while the actual controller and thin resize strips remain interactive. The hit mask is cached and refreshed for model, size or DPI changes. Imported skins retain their own rectangular window boundary. The mouse hook also exempts clicks targeting any Padmimic window before that window receives focus, so a configured mouse binding cannot prevent activation of the app.

Settings now animate their contents on a fixed-size native surface rather than resizing the GPU surface on every animation frame. A short reveal/slide and fade work in both directions; reduced motion still follows the existing Animations preference.

Appearance → Settings open → Smart positioning across monitors is enabled by default, including for existing settings files. It checks coverage by the union of active monitor work areas, accounting for gaps, negative coordinates, unequal heights and taskbars. The panel prefers right, fades across to the left only when the full panel will no longer fit, and returns right when room is available. Disabling smart positioning restricts placement to the main window's current monitor. No side having room falls back to settings inside the app.
