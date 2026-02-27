# HandsOff - macOS Input Lock

[![build rust](https://github.com/mhuang74/handsoff-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/mhuang74/handsoff-rs/actions/workflows/rust.yml)
[![Latest Release](https://img.shields.io/github/v/release/mhuang74/handsoff-rs)](https://github.com/mhuang74/handsoff-rs/releases)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-macOS-lightgrey.svg)](https://github.com/mhuang74/handsoff-rs)

A macOS utility that prevents accidental or unsolicited input from keyboard, trackpad, and mouse devices during video conferencing, presentations, or when leaving your laptop unattended.

**Available in two modes:**
- **CLI**: Command-line interface with terminal output
- **Tray App**: Native macOS menu bar application with notifications

## Features

- **Complete Input Blocking**: Blocks all keyboard, trackpad, and mouse inputs while keeping the screen visible
- **Secure Unlocking**: Unlock via passphrase
- **Auto-Lock**: Automatically locks after 120 seconds of inactivity (configurable)
- **Smart Buffer Reset**: 3-second input buffer reset to handle accidental input (or press Escape to clear immediately)
- **Configurable Hotkeys**: Customize the last key while keeping `Cmd+Ctrl+Shift` modifiers
  - `Ctrl+Cmd+Shift+L` (default): Enable lock
  - `Ctrl+Cmd+Shift+T` (default): Talk hotkey (spacebar passthrough for unmuting)
- **Microphone & Camera**: Video conferencing apps continue to work normally
- **Menu Bar Interface**: Unobtrusive menu bar icon showing lock status (locked: red)
- **Auto-Unlock Safety Feature**: Configurable timeout that automatically unlocks after a set period to prevent permanent lockouts (disabled by default)


## Requirements

- macOS 10.11 (El Capitan) or later
- Accessibility permissions (granted on first run)

## Installation

HandsOff is available in two forms: **Tray App** (recommended for most users) and **CLI** (for advanced users).

### Option 1: Tray App (Recommended)

**Download the PKG installer from [GitHub Releases](https://github.com/mhuang74/handsoff-rs/releases):**

1. Download `HandsOff-v{VERSION}-arm64.pkg` from the latest release
2. Run the installer (installs to `~/Applications/HandsOff.app` and configures launch agent automatically)
3. Grant Accessibility permissions:
   - Go to System Settings > Privacy & Security > Accessibility
   - Add HandsOff to the list of allowed apps
4. Configure your passphrase:
   ```bash
   ~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup
   ```
   This will prompt you for:
   - Secret passphrase (typing hidden for security)
   - Auto-lock timeout (default: 120 seconds)
   - Auto-unlock timeout (default: 0 seconds/disabled)
5. Start the app:
   ```bash
   launchctl start com.handsoff.inputlock
   ```
6. The app will start automatically at login

**Key advantages:**
- ✅ Native menu bar interface with notifications
- ✅ Automatic startup at login
- ✅ Passphrase stored encrypted (AES-256-GCM)
- ✅ Visual lock status indicator (locked: red)
- ✅ One-time setup

### Option 2: CLI (Advanced Users)

**Download the CLI tarball from [GitHub Releases](https://github.com/mhuang74/handsoff-rs/releases):**

1. Download `handsoff-cli-v{VERSION}-arm64.tar.gz` from the latest release
2. Extract and install:
   ```bash
   tar -xzf handsoff-cli-v{VERSION}-arm64.tar.gz
   sudo mv handsoff-cli/handsoff /usr/local/bin/
   ```
3. Grant Accessibility permissions:
   - Go to System Settings > Privacy & Security > Accessibility
   - Add the `handsoff` binary to the list of allowed apps
4. Run the setup command to configure your passphrase:
   ```bash
   handsoff --setup
   ```
   This will prompt you for:
   - Secret passphrase (typing hidden for security)
   - Auto-lock timeout (default: 120 seconds)
   - Auto-unlock timeout (default: 0 seconds in Release builds, 60 seconds in Debug/Dev builds; can be overridden via config or HANDS_OFF_AUTO_UNLOCK)
5. Run the CLI:
   ```bash
   handsoff
   ```

**Key advantages:**
- ✅ Terminal-based interface with log output
- ✅ More control over configuration via flags
- ✅ Suitable for remote/headless usage (via SSH)
- ✅ Lightweight (no GUI dependencies)

**Building from Source:**

For developers who want to build from source, see [DEVELOPER.md](DEVELOPER.md).

---

## Usage

### Configuration

**Configuration depends on which version you're using:**

#### Shared Configuration (Both CLI and Tray App)

Both CLI and Tray App use the same encrypted configuration file:

**Configuration file location:** `~/Library/Application Support/handsoff/config.toml`

**Initial setup:**
- **Tray App**: `~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup`
- **CLI**: `handsoff --setup`

The setup wizard will prompt you for:
- Secret passphrase (stored encrypted using AES-256-GCM)
- Auto-lock timeout (default: 120 seconds)
- Auto-unlock timeout (default: 0 seconds in Release builds, 60 seconds in Debug/Dev builds; can be overridden via config or HANDS_OFF_AUTO_UNLOCK)

**Changing configuration:**
Run the setup command again to reconfigure.

#### Optional Environment Variable Overrides

You can optionally use environment variables to override config file settings:

```bash
# Optional: Override auto-lock timeout (20-600 seconds)
export HANDS_OFF_AUTO_LOCK=60

# Optional: Override auto-unlock timeout (60-900 seconds, 0=disabled)
export HANDS_OFF_AUTO_UNLOCK=300

# Optional: Override lock hotkey last key (A-Z)
export HANDS_OFF_LOCK_HOTKEY=L

# Optional: Override talk hotkey last key (A-Z)
export HANDS_OFF_TALK_HOTKEY=T
```

For permanent overrides, add these to your `~/.zshrc` or `~/.bash_profile`.

### Using the Tray App

If you installed via PKG installer, the app will start automatically at login.

**Tray App Features:**
- Menu bar icon color showing lock status (locked: red, unlocked/disabled: white)
- Desktop notifications for lock/unlock events
- Menu items: Lock Input, Disable, Reset

**Menu Items:**
- **Lock Input**: Lock immediately (only functional when unlocked)
- **Disable**: Temporarily disable HandsOff (stops event tap and hotkeys for minimal CPU usage)
- **Reset**: Resets to Unlocked and restart everything

**Important:** When locked, ALL mouse clicks are blocked (including clicks on the tray menu). The menu becomes inaccessible and you must type your passphrase to unlock.

### Using the CLI

After setting environment variables (see Configuration section above):

```bash
# Start the CLI
handsoff

# With options
handsoff --locked        # Start locked
handsoff --auto-lock 60  # Auto-lock after 60s

# View help
handsoff --help
```

**CLI Output:**
```
INFO  Starting HandsOff Input Lock
INFO  Configuration loaded from: /Users/username/Library/Application Support/handsoff/config.toml
INFO  HandsOff is running - press Ctrl+C to quit
INFO  STATUS: INPUT IS UNLOCKED
INFO  - Press Ctrl+Cmd+Shift+L to lock input
```

### Locking Input

**Tray App:**
1. Click the menu bar icon and select "Lock Input"
2. Press `Ctrl+Cmd+Shift+L` (global hotkey)

**CLI:**
1. Press `Ctrl+Cmd+Shift+L` (global hotkey)

When locked, all keyboard/mouse/trackpad input is blocked (except for Talk/Unmute hotkey and passphrase entry).

### Unlocking Input

**Both CLI and Tray App use the same unlock method:**

1. Type your passphrase on the keyboard (even though you can't see the input)
2. If you mistype, press **Escape** to clear the buffer immediately, or wait 3 seconds for it to reset automatically

**Important for Tray App users:** You CANNOT unlock via the menu! When locked, mouse clicks are blocked by the event tap, making the tray menu inaccessible. You must type your passphrase just like CLI users.

**Note:** The input buffer clears automatically after 3 seconds of inactivity to prevent multiple failed attempts from interfering with each other. You can also press **Escape** at any time to clear the buffer instantly and retry.

### Auto-Lock

The app automatically locks after 30 seconds of no input activity. You can configure this timeout. See [Configuration](#configuration).

### Talk Hotkey

When locked, press `Ctrl+Cmd+Shift+T` to temporarily pass through a spacebar keypress, allowing you to unmute in video conferencing apps like Zoom or Google Meet.


## Security

- **Encrypted Storage**: Passphrases are stored encrypted using AES-256-GCM in `~/Library/Application Support/handsoff/config.toml`
- **Protection Level**: Provides obfuscation against casual file inspection. Note that the encryption key is embedded in the binary and could be extracted through reverse engineering
- **File Permissions**: Config file has 600 permissions (readable only by your user account)
- **No Network**: No network connections or telemetry
- **Local Only**: All data stays on your device

**For maximum security:**
- Use a strong, unique passphrase
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
- **Both CLI and Tray App**: Run the setup command again to reconfigure:
  - Tray App: `~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup`
  - CLI: `handsoff --setup`
- If locked and can't unlock: Restart in Safe Mode to avoid launching HandsOff, then run setup again
- If remote access is enabled: ssh into host and `killall handsoff-tray` or `killall handsoff`

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
- `ring`: Cryptographic hashing (SHA-256)
- `clap`: Command-line argument parsing (CLI)
- `parking_lot`: Fast mutex implementation


## License

See LICENSE file for details.