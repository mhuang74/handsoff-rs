# HandsOff - macOS Input Lock

A macOS utility that prevents accidental or unsolicited input from keyboard, trackpad, and mouse devices during video conferencing, presentations, or when leaving your laptop unattended.

**Available in two modes:**
- **CLI**: Command-line interface with terminal output
- **Tray App**: Native macOS menu bar application with notifications

## Features

- **Complete Input Blocking**: Blocks all keyboard, trackpad, and mouse inputs while keeping the screen visible
- **Secure Unlocking**: Unlock via passphrase
- **Auto-Lock**: Automatically locks after 30 seconds of inactivity (configurable)
- **Smart Buffer Reset**: 5-second input buffer reset to handle accidental input
- **Hotkeys**:
  - `Ctrl+Cmd+Shift+L`: Enable lock
  - `Ctrl+Cmd+Shift+T`: Talk hotkey (spacebar passthrough for unmuting)
- **Microphone & Camera**: Video conferencing apps continue to work normally
- **Menu Bar Interface**: Unobtrusive menu bar icon showing lock status (locked: red)
- **Auto-Unlock Safety Feature [CLI Only]**: Configurable timeout that automatically unlocks after a set period to prevent permanent lockouts (disabled by default)


## Requirements

- macOS 10.11 (El Capitan) or later
- Accessibility permissions (granted on first run)

## Installation

HandsOff is available in two forms: **Tray App** (recommended for most users) and **CLI** (for advanced users).

### Option 1: Tray App (Recommended)

**Download the PKG installer from [GitHub Releases](https://github.com/your-repo/handsoff-rs/releases):**

1. Download `HandsOff-v{VERSION}-arm64.pkg` from the latest release
2. Run the installer (installs to `~/Applications/HandsOff.app`)
3. Grant Accessibility permissions:
   - Go to System Settings > Privacy & Security > Accessibility
   - Add HandsOff to the list of allowed apps
4. Run the setup script to configure your passphrase and launch agent:
   ```bash
   ~/Applications/HandsOff.app/Contents/MacOS/setup-launch-agent.sh
   ```
5. The app will start automatically at login

**Key advantages:**
- ✅ Native menu bar interface with notifications
- ✅ Automatic startup at login
- ✅ Passphrase embedded in launch agent (no environment variables needed)
- ✅ Visual lock status indicator (locked: red)
- ✅ One-time setup

### Option 2: CLI (Advanced Users)

**Download the CLI tarball from [GitHub Releases](https://github.com/your-repo/handsoff-rs/releases):**

1. Download `handsoff-cli-v{VERSION}-arm64.tar.gz` from the latest release
2. Extract and install:
   ```bash
   tar -xzf handsoff-cli-v{VERSION}-arm64.tar.gz
   sudo mv handsoff-cli/handsoff /usr/local/bin/
   ```
3. Grant Accessibility permissions:
   - Go to System Settings > Privacy & Security > Accessibility
   - Add the `handsoff` binary to the list of allowed apps
4. Configure environment variables (required for CLI):
   ```bash
   export HANDS_OFF_SECRET_PHRASE='your-secret-passphrase'
   ```
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

#### Tray App Configuration

**No environment variables needed!** The Tray App uses the launch agent for configuration.

- **Secret Passphrase**: Set once during initial setup via `setup-launch-agent.sh` (embedded in launch agent plist)
- **Auto-lock**: Defaults to 30 seconds, but may be customized.

The setup script handles the Secret Passphrase configuration automatically. Auto-lock defaults to 30 seconds. Both may be changed by editing the EnvironmentVariables section of plist file.

Example Env Var Section in plist file
```xml
<!-- ~/Library/LaunchAgents/com.handsoff.inputlock.plist -->
    <key>EnvironmentVariables</key>
    <dict>
        <key>HANDS_OFF_SECRET_PHRASE</key>
        <string>knockknock</string>
         <key>HANDS_OFF_AUTO_LOCK</key>
         <string>60</string>  <!-- 60 seconds -->
    </dict>
</dict>
```

#### CLI Configuration

**Environment variables required** before running the CLI:

```bash
# Required: Your secret passphrase
export HANDS_OFF_SECRET_PHRASE='your-secret-passphrase'

# Optional: Auto-lock after inactivity (20-600 seconds, default: 30)
export HANDS_OFF_AUTO_LOCK=60

# Optional: Auto-unlock safety timeout (60-900 seconds, 0=disabled)
export HANDS_OFF_AUTO_UNLOCK=300
```

For permanent CLI configuration, add these to your `~/.zshrc` or `~/.bash_profile`.

### Using the Tray App

If you installed via PKG installer, the app will start automatically at login.

**Tray App Features:**
- Menu bar icon color showing lock status (locked: red, unlocked/disabled: white)
- Desktop notifications for lock/unlock events
- Menu items: Lock Input, Disable/Enable, Reset, Version, Help, Quit

**Menu Items:**
- **Lock Input**: Lock immediately (only functional when unlocked)
- **Disable**: Temporarily disable HandsOff (stops event tap and hotkeys for minimal CPU usage)
- **Enable**: Re-enable HandsOff after disabling
- **Reset**: Restart event tap (useful if permissions were restored)
- **Version**: Show app version
- **Help**: Show usage instructions
- **Quit**: Exit the application

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
INFO  Using passphrase from HANDS_OFF_SECRET_PHRASE environment variable
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
2. If you mistype, wait 5 seconds for the buffer to reset, then try again

**Important for Tray App users:** You CANNOT unlock via the menu! When locked, mouse clicks are blocked by the event tap, making the tray menu inaccessible. You must type your passphrase just like CLI users.

**Note:** The input buffer clears automatically after 5 seconds of inactivity to prevent multiple failed attempts from interfering with each other.

### Auto-Lock

The app automatically locks after 30 seconds of no input activity. You can configure this timeout. See [Configuration](#configuration).

### Talk Hotkey

When locked, press `Ctrl+Cmd+Shift+T` to temporarily pass through a spacebar keypress, allowing you to unmute in video conferencing apps like Zoom or Google Meet.


## Security

- Passphrases are currently stored as plain text [Caveat Emptor]
- No network connections or telemetry
- All data stays on your device

## Compatibility

- Tested on MBA M2 with macOS 15.7 (Sequoia)
- Should work on older macOS due to minimal dependencies
- Should work on both Intel and Apple Silicon Macs (Rust cross-platform)

## Troubleshooting

### App doesn't block input
- Ensure Accessibility permissions are granted in System Settings > Privacy & Security > Accessibility
- Restart the app after granting permissions

### Forgot passphrase
- **Tray App**: If unlocked, run `setup-launch-agent.sh` to set a new passphrase
- **CLI**: Update your `HANDS_OFF_SECRET_PHRASE` environment variable
- If locked and can't unlock: Restart in Safe Mode to avoid launching HandsOff, then reconfigure
- If remote access is enabled, ssh into host and `killall HandsOff`

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