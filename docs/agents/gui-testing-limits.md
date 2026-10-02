# GUI testing limits (macOS)

This dev machine's TCC grants do NOT allow agent processes to drive or capture the GUI. Verified 2026-10-03:

- **Window screenshots blocked**: `screencapture` runs but without Screen Recording permission it captures only the desktop/wallpaper. `screencapture -l<windowid>` fails outright (`could not create image from window`). Region grabs (`-R x,y,w,h`) over an app window also return wallpaper. Desktop-only captures work.
- **No UI automation**: osascript/System Events fails with assistive-access errors (-1719, -25211); synthetic CGEvent clicks need the Accessibility grant that the app under test itself is fixing.

## What works instead

1. **Logs**: launch from a terminal with `RUST_LOG=debug target/debug/handsoff-tray`; env_logger writes to stderr. (Single-instance flock guard: kill any running instance first, or the duplicate exits.)
2. **Numeric verification**: temporary debug logging of frames (`frame()`, `fittingSize`, row counts) inside the flow under test, or standalone AppKit probes in `/tmp` replicating the layout, then compare numbers.
3. **User screenshots**: for final visual confirmation, ask the user to screenshot (Cmd+Shift+4, space, click the window) and paste the image. The user pastes images into the session; do not ask them to save to a path you can read unless they offer.
4. **Throwaway env hooks**: a temporary env-var gate in `main` (e.g. open a specific window immediately, exit on close) reaches a specific window without menu automation. Remove it after verification.
