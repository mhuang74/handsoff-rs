# Changelog

## [0.1.0] - 2025-10-22

### Initial Release

#### Features
- Complete input blocking via CGEventTap for keyboard, mouse, and trackpad
- Passphrase-based authentication with SHA-256 hashing
- Touch ID support (macOS 10.12.2+)
- Global hotkeys:
  - Lock: `Ctrl+Cmd+Shift+L`
  - Talk: `Ctrl+Cmd+Shift+T`
  - Touch ID: `Ctrl+Cmd+Shift+U`
- Auto-lock after 3 minutes of inactivity
- 5-second input buffer reset for accidental input
- macOS Keychain integration for secure storage
- Menu bar interface with lock status indicator
- System notifications for lock/unlock events

#### Technical Details
- Built with Rust 2021 edition
- Uses CFMachPortCreateRunLoopSource for event tap run loop integration
- Compatible with macOS 10.11+ (El Capitan and later)
- Supports both Intel (x86_64) and Apple Silicon (arm64) architectures

#### Known Limitations
- Touch ID uses osascript for authentication (future: direct LocalAuthentication framework)
- Menu bar menu items don't yet trigger lock/unlock actions
- Talk hotkey framework exists but doesn't implement spacebar passthrough yet
- No visual lock indicator beyond menu bar icon
- Settings UI for customization not yet implemented

### Fixed
- Linker error with CGEventTapCreateRunLoopSource by using CFMachPortCreateRunLoopSource instead
