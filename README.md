# HandsOff - macOS Input Lock

A macOS menu bar application that prevents accidental or unsolicited input from keyboard, trackpad, and mouse devices during video conferencing, presentations, or when leaving your laptop unattended.

## Features

- **Complete Input Blocking**: Blocks all keyboard, trackpad, and mouse inputs while keeping the screen visible
- **Secure Unlocking**: Unlock via passphrase or Touch ID
- **Auto-Lock**: Automatically locks after 3 minutes of inactivity (configurable)
- **Smart Buffer Reset**: 5-second input buffer reset to handle accidental input
- **Hotkeys**:
  - `Ctrl+Cmd+Shift+L`: Enable lock
  - `Ctrl+Cmd+Shift+T`: Talk hotkey (spacebar passthrough for unmuting)
  - `Ctrl+Cmd+Shift+U`: Trigger Touch ID unlock
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

Three ways to unlock:
1. **Passphrase**: Type your passphrase on the keyboard (even though you can't see the input)
2. **Touch ID**: Press `Ctrl+Cmd+Shift+U` to trigger Touch ID authentication
3. **Wait**: If you accidentally type gibberish, wait 5 seconds for the buffer to reset, then try again

### Auto-Lock

The app automatically locks after 3 minutes of no input activity. You can configure this timeout in the keychain settings.

### Talk Hotkey

When locked, press `Ctrl+Cmd+Shift+T` to temporarily pass through a spacebar keypress, allowing you to unmute in video conferencing apps like Zoom or Google Meet.

## Project Structure

```
src/
├── main.rs                 # Application entry point
├── app_state.rs           # Shared application state
├── auth/                  # Authentication modules
│   ├── mod.rs
│   ├── keychain.rs        # Keychain storage
│   └── touchid.rs         # Touch ID authentication
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
- Touch ID uses macOS's secure enclave
- No network connections or telemetry
- All data stays on your device

## Compatibility

- Tested on macOS 10.11 (El Capitan) through macOS 14 (Sonoma)
- Works on both Intel and Apple Silicon Macs
- Touch ID requires macOS 10.12.2+ and compatible hardware

## Troubleshooting

### App doesn't block input
- Ensure Accessibility permissions are granted in System Settings > Privacy & Security > Accessibility
- Restart the app after granting permissions

### Touch ID doesn't work
- Touch ID requires macOS 10.12.2+ and compatible hardware (MacBook Pro 2016+)
- Fall back to passphrase entry if Touch ID is unavailable

### Forgot passphrase
- Quit the app (when unlocked)
- Remove the keychain entry: `security delete-generic-password -s com.handsoff.inputlock -a passphrase_hash`
- Restart the app and set a new passphrase

## License

See LICENSE file for details.

## Acknowledgments

Built with:
- `cocoa-rs`: Rust bindings for Cocoa (AppKit)
- `core-graphics-rs`: CoreGraphics event handling
- `keyring-rs`: Keychain integration
- `global-hotkey`: Global hotkey registration
- `ring`: Cryptographic hashing
