# Changelog

## [0.6.3] - 2025-11-15

## 📦 Uncategorized

- Fix Talk hotkey compatibility with Google Meet/Zoom
   - PR: #10



## [0.6.1] - 2025-11-13

### Fixed
- Fix auto-unlock timeout=0 causing immediate unlock instead of disabling
- Fix typo

### Changed
- Disable Auto-Unlock by Default for Release Builds

## [0.6.0] - 2025-11-12

### Fixed
- Fix critical permission loss bug that could cause system lockout (#7)

## [0.5.1] - 2025-11-06

### Fixed
- Fix PKG installer postinstall script by including LaunchAgent plist template in app bundle

## [0.5.0] - 2025-11-06

### Added
- Add encrypted passphrase storage with AES-256-GCM (#6)
- Add CLI binary releases and update documentation

### Changed
- Separate developer documentation from end-user README
- Update install help text
- Remove deprecated menu items

## [0.4.0] - 2025-11-05

### Added
- Add GitHub Actions workflow for automated macOS releases
- Add Disable feature for minimal CPU usage
- Add dark mode support to installer HTML
- Convert project to produce both CLI and Tray App (#2)

### Changed
- Change installer to user-level installation (no root required)

### Fixed
- Fix WindowServer stability issues in Disabled mode
- Fix reset after disable (#4)
- Fix GitHub Actions workflow syntax errors

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
