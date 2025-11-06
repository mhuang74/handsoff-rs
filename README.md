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
- If unlocked, run `setup-launch-agent.sh` to set a new passphrase
- If locked, restart in Safe Mode to avoid launching HandsOff, then run `setup-launch-agent.sh` to set a new passphrase
- If remote access is enabled, ssh into host and `killall HandsOff`

## DEVELOPER

Advanced instructions for developers to build and test HandsOff with Auto-unlock feature.

### Build Both Binaries

```bash
# Clone the repository
cd handsoff-rs

# Build both CLI and Tray App
cargo build --release

# The binaries will be at:
# - target/release/handsoff (CLI)
# - target/release/handsoff-tray (Tray App)
```

### Build Individual Binaries

```bash
# CLI only
cargo build --release --bin handsoff

# Tray App only
cargo build --release --bin handsoff-tray
```

### Build for Specific Architecture

For Apple Silicon Macs:
```bash
cargo build --release --target aarch64-apple-darwin
```

For Intel Macs:
```bash
cargo build --release --target x86_64-apple-darwin
```

### Universal Binary (Both Architectures)

```bash
# Install targets
rustup target add x86_64-apple-darwin
rustup target add aarch64-apple-darwin

# Build for both architectures
cargo build --release --target x86_64-apple-darwin --bin handsoff
cargo build --release --target aarch64-apple-darwin --bin handsoff

# Combine with lipo
lipo -create \
  target/x86_64-apple-darwin/release/handsoff \
  target/aarch64-apple-darwin/release/handsoff \
  -output target/release/handsoff-universal
```

## Auto-Unlock Safety Feature

The auto-unlock feature provides a fail-safe mechanism that automatically disables input interception after a configurable timeout. This prevents permanent lockouts due to bugs, forgotten passphrases during development, or other unexpected issues.

**⚠️ Important:** This feature is designed for **development, testing, and personal emergency use only**. It should NOT be enabled in production environments where security is critical.

#### Enabling Auto-Unlock

Set the `HANDS_OFF_AUTO_UNLOCK` environment variable to the desired timeout in seconds:

```bash
# Enable with 30-second timeout (for quick testing)
HANDS_OFF_AUTO_UNLOCK=30 cargo run

# Enable with 5-minute timeout (for development)
HANDS_OFF_AUTO_UNLOCK=300 ./handsoff

# Enable with 10-minute timeout (more conservative)
HANDS_OFF_AUTO_UNLOCK=600 ./handsoff

# Disabled (default behavior - no auto-unlock)
./handsoff
```

#### Valid Configuration Values

- **Minimum:** 60 seconds
- **Maximum:** 900 seconds (15 minutes)
- **Disabled:** 0 or unset (default)
- **Invalid values** (below 60 or above 900) will disable the feature with a warning

#### How It Works

1. When you lock the device, a timer starts counting
2. Every 10 seconds, the app checks if the timeout has been exceeded
3. If the timeout expires while locked:
   - Input interception is automatically disabled
   - A prominent notification appears: "HandsOff Auto-Unlock Activated"
   - The menu bar icon updates to unlocked state
   - The event is logged at WARNING level for audit purposes
4. If you manually unlock before the timeout, the timer resets

#### Use Cases

**Development/Testing:**
```bash
# Quick testing during development
HANDS_OFF_AUTO_UNLOCK=30 cargo run
```

**Personal Use (Emergency Failsafe):**
```bash
# Set a 10-minute failsafe in case you forget your passphrase
HANDS_OFF_AUTO_UNLOCK=600 ./handsoff
```

**Launch Agent (Permanent Configuration):**
```xml
<!-- ~/Library/LaunchAgents/com.handsoff.plist -->
<key>EnvironmentVariables</key>
<dict>
    <key>HANDS_OFF_AUTO_UNLOCK</key>
    <string>300</string>  <!-- 5 minutes -->
</dict>
```

#### Security Implications

**Benefits:**
- Prevents denial-of-service if bugs occur
- Provides emergency access during development
- Logged for audit purposes

**Risks:**
- Reduces security if timeout is too short
- An attacker who knows the feature exists could wait for auto-unlock
- Not suitable for public/shared computers

**Recommendations:**
- ✅ Use for development and testing
- ✅ Use with longer timeouts (5-10 minutes) for personal devices
- ❌ Do NOT use in production/public environments
- ❌ Do NOT set timeouts shorter than 60 seconds for actual use
- ❌ Do NOT enable on shared computers

#### Verification

When auto-unlock is enabled, check the logs at startup:

```bash
# You should see this in the logs
INFO  Auto-unlock safety feature enabled: 30 seconds
INFO  Auto-unlock monitoring thread started
```

When auto-unlock triggers:

```bash
WARN  Auto-unlock timeout expired - disabling input interception
WARN  AUTO-UNLOCK TRIGGERED after 30 seconds
INFO  Auto-unlock notification delivered
```
### Auto-unlock not triggering

**Check if feature is enabled:**
```bash
# Verify environment variable is set
echo $HANDS_OFF_AUTO_UNLOCK

# Run with logging to see status
RUST_LOG=info HANDS_OFF_AUTO_UNLOCK=30 ./handsoff
```

**Common issues:**
- Environment variable not set or set to invalid value
- Value is outside valid range (60-900 seconds)
- Device was not actually locked (check menu bar icon)
- Manual unlock occurred before timeout expired

**Expected behavior:**
- Feature logs "Auto-unlock safety feature enabled: X seconds" at startup
- Auto-unlock thread logs "Auto-unlock monitoring thread started"
- Triggers within 10 seconds of configured timeout (thread sleeps 10s between checks)

### Auto-unlock notification not appearing

**Check notification permissions:**
```bash
# Ensure app can send notifications
# System Settings > Notifications > HandsOff
```

**Check logs for errors:**
```bash
RUST_LOG=debug ./handsoff
# Look for: "Failed to get notification center" or similar errors
```

**Try manually opening Notification Center** while the app is running to ensure notifications are enabled

### Auto-unlock timer seems inaccurate

This is **expected behavior**, not a bug:
- The monitoring thread sleeps for 10 seconds between checks
- Auto-unlock will trigger within 0-10 seconds after the configured timeout
- Example: With `HANDS_OFF_AUTO_UNLOCK=30`, unlock will occur between 30-40 seconds after lock
- This design balances accuracy with CPU efficiency

### Locked out despite auto-unlock being enabled

**Emergency recovery options:**

1. **SSH from another device** (if SSH is enabled):
   ```bash
   ssh user@your-mac
   pkill -f HandsOff
   ```

2. **Hard Reboot** (last resort):
   - Hold power button until Mac shuts down
   - If HandsOff locks right away after login, then try botting into Safe Mode

**Prevention:**
- Always test auto-unlock works before relying on it
- Start with short timeout (30s) for testing
- Increase to longer timeout (5-10 minutes) for actual use
- Keep terminal window visible to see logs during testing


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