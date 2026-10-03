# HandsOff - macOS Input Lock

[![build rust](https://github.com/mhuang74/handsoff-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/mhuang74/handsoff-rs/actions/workflows/rust.yml)
[![Latest Release](https://img.shields.io/github/v/release/mhuang74/handsoff-rs)](https://github.com/mhuang74/handsoff-rs/releases)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-macOS-lightgrey.svg)](https://github.com/mhuang74/handsoff-rs)

A macOS utility that prevents accidental or unsolicited input from keyboard, trackpad, and mouse devices during video conferencing, presentations, or when leaving your laptop unattended.

**Available as a native macOS menu bar application (Tray App).**

## Features

- **Complete Input Blocking**: Blocks all keyboard, trackpad, and mouse inputs while keeping the screen visible
- **Secure Unlocking**: Unlock via a physical-key passphrase (works on any keyboard layout)
- **Auto-Lock**: Automatically locks after 180 seconds of inactivity (configurable)
- **Smart Buffer Reset**: 3-second input buffer reset to handle accidental input (or press Escape to clear immediately)
- **Configurable Hotkeys**: Customize the last key while keeping `Cmd+Ctrl+Shift` modifiers
  - `Ctrl+Cmd+Shift+L` (default): Enable lock
  - `Ctrl+Cmd+Shift+T` (default): Talk hotkey (spacebar passthrough for unmuting)
- **Microphone & Camera**: Video conferencing apps continue to work normally
- **Menu Bar Interface**: Unobtrusive menu bar icon showing lock status (locked: red)
- **Auto-Unlock Safety Feature**: Automatically unlocks after an exponentially increasing wait (60 min → 2 h → 4 h …, capped at 24 h) to prevent permanent lockouts (enabled by default)


## Requirements

- macOS 13 (Ventura) or later
- Accessibility permissions (granted on first run)

## Installation

HandsOff is available as a **Tray App** (macOS menu bar application).

### Tray App

**Download the DMG from [GitHub Releases](https://github.com/mhuang74/handsoff-rs/releases):**

1. Download the DMG for your Mac's architecture from the latest release:
   - Apple Silicon: `HandsOff-v{VERSION}-arm64.dmg`
   - Intel: `HandsOff-v{VERSION}-x86_64.dmg`
2. Mount the DMG and drag HandsOff.app to `/Applications`
3. First launch (unsigned app): right-click (or Control-click) HandsOff.app
   and choose **Open**, then confirm **Open** in the Gatekeeper dialog.
   You only need to do this once.
4. Grant Accessibility permissions:
   - Go to System Settings > Privacy & Security > Accessibility
   - Add HandsOff to the list of allowed apps
5. Configure your passphrase with the built-in Setup Wizard, which opens
   automatically on first run (when no valid config exists). It prompts for:
   - Secret passphrase (captured as physical key presses — layout-independent)
   - Auto-lock timeout (default: 180 seconds)
   - Auto-unlock (default: enabled — 60-minute base, doubling up to 24 h)
   - Hotkeys and a "Launch at login" checkbox
6. Optionally enable the login item (wizard checkbox) so the app starts
   automatically at login

**Key advantages:**
- ✅ Native menu bar interface with notifications
- ✅ Optional automatic startup at login (login item)
- ✅ Passphrase stored as a SHA-256 hash (no plaintext on disk)
- ✅ Visual lock status indicator (locked: red)
- ✅ One-time setup via the built-in Setup Wizard

> **Updating to a newer release?** Each build has a different signature, so
> the old Accessibility permission must be **deleted and re-added** — see
> "Updating / Reinstalling" in
> [docs/DMG-GUIDE.md](docs/DMG-GUIDE.md). In short:
> `tccutil reset Accessibility handsoff-tray.handsoff`, then replace the
> app and grant the permission again (or use the in-app
> **Fix Accessibility Permission…** menu item).

**Building from Source:**

For developers who want to build from source, see [DEVELOPER.md](DEVELOPER.md).

---

## Usage

### Configuration

HandsOff uses a single encrypted configuration file:

**Configuration file location:** `~/Library/Application Support/handsoff/config.toml`

**Initial setup:**
- **Tray App**: launch HandsOff.app — the Setup Wizard opens automatically
  when no valid config exists (reconfigure later via tray menu → Preferences)

The setup wizard will prompt you for:
- Secret passphrase (stored as a SHA-256 hash of your physical key sequence)
- Auto-lock timeout (default: 180 seconds)
- Auto-unlock (default: enabled — 60-minute base, doubling up to 24 h)

**Changing configuration:**
Use the tray menu (Preferences…, Change Passphrase…, Reset…) to reconfigure.

#### Optional Environment Variable Overrides

You can optionally use environment variables to override config file settings:

```bash
# Optional: Override auto-lock timeout (20-600 seconds)
export HANDS_OFF_AUTO_LOCK=60

# Optional: Override auto-unlock base interval (seconds, 0=disabled)
export HANDS_OFF_AUTO_UNLOCK=3600
```

For permanent overrides, add these to your `~/.zshrc` or `~/.bash_profile`.

### Using the Tray App

If you enabled the login item in the Setup Wizard (or System Settings →
General → Login Items), the app starts automatically at login.

**Tray App Features:**
- Menu bar icon color showing lock status (locked: red, unlocked/disabled: white)
- Desktop notifications for lock/unlock events
- Menu items: Lock Input, Disable, Reenable, Preferences…, Change Passphrase…, Reset…, Fix Accessibility Permission…, Check for Updates…, Help

**Menu Items:**
- **Lock Input**: Lock immediately (only functional when unlocked)
- **Disable**: Temporarily disable HandsOff (stops event tap and hotkeys for minimal CPU usage). Use Reenable to resume.
- **Reenable**: End a stuck Lock and restart input blocking; the unguarded escape hatch. Does NOT change your configuration.
- **Preferences…**: Edit hotkeys, auto-lock timeout, and auto-unlock backoff without re-entering your passphrase.
- **Change Passphrase…**: Capture a new passphrase via the same silent double-entry flow as setup.
- **Reset…**: Wipe the configuration (double-confirmed) and restart the Setup Wizard. Recovery path for a forgotten passphrase — your current passphrase stops working.
- **Fix Accessibility Permission…**: Clear a stale Accessibility grant and relaunch to re-request the permission (for when an app update invalidated the old grant).
- **Check for Updates…**: Open the latest GitHub release page in your browser.
- **Help**: Open an in-app window with status, menu guide, and troubleshooting.

**Important:** When locked, ALL mouse clicks are blocked (including clicks on the tray menu). The menu becomes inaccessible and you must type your passphrase to unlock.

### Locking Input

**Tray App:**
1. Click the menu bar icon and select "Lock Input"
2. Press `Ctrl+Cmd+Shift+L` (global hotkey)

When locked, all keyboard/mouse/trackpad input is blocked (except for Talk/Unmute hotkey and passphrase entry).

### Unlocking Input

**Unlock method:**

1. Type your passphrase on the keyboard (even though you can't see the input)
2. If you mistype, press **Escape** to clear the buffer immediately, or wait 3 seconds for it to reset automatically

**Important:** You CANNOT unlock via the menu! When locked, mouse clicks are blocked by the event tap, making the tray menu inaccessible. You must type your passphrase to unlock.

**Note:** The input buffer clears automatically after 3 seconds of inactivity to prevent multiple failed attempts from interfering with each other. You can also press **Escape** at any time to clear the buffer instantly and retry.

### Auto-Lock

The app automatically locks after 180 seconds of no input activity. You can configure this timeout. See [Configuration](#configuration).

### Talk Hotkey

When locked, press `Ctrl+Cmd+Shift+T` to temporarily pass through a spacebar keypress, allowing you to unmute in video conferencing apps like Zoom or Google Meet.

## Security

- **Hashed Storage**: Passphrases are stored as a SHA-256 hash of your physical key sequence in `~/Library/Application Support/handsoff/config.toml`
- **Layout-Independent**: The passphrase is matched as a sequence of physical keycodes, so it works on any keyboard layout
- **File Permissions**: Config file has 600 permissions (readable only by your user account)
- **No Network**: No network connections or telemetry
- **Local Only**: All data stays on your device

**Threat model (V5):** HandsOff guards against casual interference — a child, colleague, or screenshare audience. It is not a barrier against a determined local actor, who can reboot the machine (the app relaunches unlocked), kill the app, or simply carry the machine away.

**For maximum security:**
- Use at least 4 physical keys for your passphrase (it is matched as a key sequence, independent of keyboard layout)
- Enable FileVault disk encryption on macOS
- Keep your system and user account secure

## Compatibility

- Tested on MBA M2 with macOS 15.7 (Sequoia)
- Should work on older macOS due to minimal dependencies
- Should work on both Intel and Apple Silicon Macs (Rust cross-platform)

## Troubleshooting

### App doesn't block input
- Ensure Accessibility permissions are granted in System Settings > Privacy & Security > Accessibility
- Restart the app after granting permissions

### Forgot passphrase
- Reconfigure via the Setup Wizard: delete the config file, relaunch the app —
  the Setup Wizard opens automatically (or use tray menu → Change Passphrase)
- If locked and can't unlock: Restart in Safe Mode to avoid launching HandsOff, then run setup again

---

## For Developers

For information on:
- Building from source
- Tech stack and libraries used
- Auto-unlock safety feature (for development/testing)
- Project structure and architecture

See **[DEVELOPER.md](DEVELOPER.md)**

---

## Acknowledgments

Built with:
- `core-graphics`: CoreGraphics event handling (CGEventTap)
- `core-foundation`: CFRunLoop integration
- `tray-icon`: Native macOS menu bar icon (Tray App)
- `tao`: Cross-platform event loop (Tray App)
- `notify-rust`: Native macOS notifications (Tray App)
- `global-hotkey`: Global hotkey registration
- `ring`: Cryptographic hashing (SHA-256 over keycode sequences)
- `parking_lot`: Fast mutex implementation


## License

See LICENSE file for details.