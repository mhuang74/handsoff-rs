# HandsOff - macOS Input Lock

A macOS menu bar application that prevents accidental or unsolicited input from keyboard, trackpad, and mouse devices during video conferencing, presentations, or when leaving your laptop unattended.

## Features

- **Complete Input Blocking**: Blocks all keyboard, trackpad, and mouse inputs while keeping the screen visible
- **Secure Unlocking**: Unlock via passphrase
- **Auto-Lock**: Automatically locks after 3 minutes of inactivity (configurable)
- **Auto-Unlock Safety Feature**: Configurable timeout that automatically unlocks after a set period to prevent permanent lockouts (optional, for development/testing)
- **Smart Buffer Reset**: 5-second input buffer reset to handle accidental input
- **Hotkeys**:
  - `Ctrl+Cmd+Shift+L`: Enable lock
  - `Ctrl+Cmd+Shift+T`: Talk hotkey (spacebar passthrough for unmuting)
- **Microphone & Camera**: Video conferencing apps continue to work normally
- **Menu Bar Interface**: Unobtrusive menu bar icon showing lock status (🔓/🔒)

## Requirements

- macOS 10.11 (El Capitan) or later
- Rust toolchain (for building from source)
- Accessibility permissions (granted on first run)

## Building

```bash
# Clone the repository
cd handsoff-rs

# Build the project
cargo build --release

# The binary will be at: target/release/handsoff
```

For Apple Silicon Macs:
```bash
cargo build --release --target aarch64-apple-darwin
```

For Intel Macs:
```bash
cargo build --release --target x86_64-apple-darwin
```

## Installation

1. Build the project using the commands above
2. Copy the binary to your Applications folder or preferred location
3. Run the app - you'll be prompted to grant Accessibility permissions
4. Go to System Settings > Privacy & Security > Accessibility
5. Add HandsOff to the list of allowed apps
6. Restart HandsOff

## Usage

### First Run

On first launch, you'll be prompted to set a passphrase. This passphrase will be used to unlock the input when locked.

### Locking Input

You can lock input in two ways:
1. Click the menu bar icon (🔓) and select "Enable Lock"
2. Press `Ctrl+Cmd+Shift+L` (default hotkey)

When locked, the menu bar icon changes to 🔒 and all keyboard/mouse/trackpad input is blocked.

### Unlocking Input

Two ways to unlock:
1. **Passphrase**: Type your passphrase on the keyboard (even though you can't see the input)
2. **Wait**: If you accidentally type gibberish, wait 5 seconds for the buffer to reset, then try again

### Auto-Lock

The app automatically locks after 3 minutes of no input activity. You can configure this timeout in the keychain settings.

### Talk Hotkey

When locked, press `Ctrl+Cmd+Shift+T` to temporarily pass through a spacebar keypress, allowing you to unmute in video conferencing apps like Zoom or Google Meet.

### Auto-Unlock Safety Feature

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

## Project Structure

```
src/
├── main.rs                 # Application entry point
├── app_state.rs           # Shared application state
├── auth/                  # Authentication modules
│   ├── mod.rs
│   └── keychain.rs        # Keychain storage
├── input_blocking/        # Input blocking modules
│   ├── mod.rs
│   ├── event_tap.rs       # CGEventTap implementation
│   └── hotkeys.rs         # Global hotkey handling
├── ui/                    # User interface modules
│   ├── mod.rs
│   ├── menubar.rs         # Menu bar interface
│   ├── notifications.rs   # System notifications
│   └── dialogs.rs         # Alert dialogs
└── utils/                 # Utility modules
    ├── mod.rs
    └── keycode.rs         # Keycode to character mapping
```

## Security

- Passphrases are stored as SHA-256 hashes in macOS Keychain
- No network connections or telemetry
- All data stays on your device

## Compatibility

- Tested on macOS 10.11 (El Capitan) through macOS 14 (Sonoma)
- Works on both Intel and Apple Silicon Macs

## Troubleshooting

### App doesn't block input
- Ensure Accessibility permissions are granted in System Settings > Privacy & Security > Accessibility
- Restart the app after granting permissions

### Forgot passphrase
- Quit the app (when unlocked)
- Remove the keychain entry: `security delete-generic-password -s com.handsoff.inputlock -a passphrase_hash`
- Restart the app and set a new passphrase

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
   pkill -f handsoff
   ```

2. **Force quit** (if you can still access menu bar):
   - Press `Cmd+Option+Esc`
   - Select HandsOff and click Force Quit

3. **Hard reboot** (last resort):
   - Hold power button until Mac shuts down
   - This is why auto-unlock exists - to prevent this scenario

**Prevention:**
- Always test auto-unlock works before relying on it
- Start with short timeout (30s) for testing
- Increase to longer timeout (5-10 minutes) for actual use
- Keep terminal window visible to see logs during testing

## License

See LICENSE file for details.

## Acknowledgments

Built with:
- `cocoa-rs`: Rust bindings for Cocoa (AppKit)
- `core-graphics-rs`: CoreGraphics event handling
- `keyring-rs`: Keychain integration
- `global-hotkey`: Global hotkey registration
- `ring`: Cryptographic hashing
