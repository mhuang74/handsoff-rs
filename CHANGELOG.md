# Changelog

## [0.7.0] - 2026-09-30

## 📦 Uncategorized

- docs: add deep design review notes (2026-09)
   - PR: #21
- feat: keycode-sequence passphrases + exponential-backoff auto-unlock (v0.7.0) + 4 rounds review remediation
   - PR: #23



## [Unreleased]

- feat: keycode-sequence passphrases (`keycode-v1`) — setup captures physical key-codes via a temporary event tap (interactive only); hash stored in config; legacy encrypted configs force re-setup (V1, V6)
- feat: auto-unlock exponential backoff — enabled by default, base 60 min, doubling capped at 24 h; only a successful passphrase unlock resets the schedule; awake-time semantics (V2, V7–V9)
- feat: first-run setup enforcement in tray — default `qwet` passphrase auto-creation removed, tooltip hint removed (S-3, L-5)
- fix: passphrase buffer contents no longer written to logs; length only (S-1)
- feat: Reset force-unlocks via state, no plaintext stored (S-2)
- feat: silent unlock — no notification when input is restored (V10)
- feat: tooltip shows auto-unlock countdown only when < 5 min away (V11)
- refactor: single-guard keystroke handler; removed `crypto.rs` (AES-256-GCM) and its dependencies
- perf: steady-state CPU reduction — `MouseMoved` removed from the event tap (idle time now read via `CGEventSourceSecondsSinceLastEventType`, `min`-combined with the tap clock for decision and countdown display); tray tooltip rebuilt on state change or every 15 s instead of every 500 ms tick
- change: auto-lock default 120 s → 180 s

## [0.6.10] - 2026-09-27

## 📦 Uncategorized

- CI: x86_64 macOS release builds, release job restructure, AI code-review updates
   - PR: #20



## [0.6.10] - 2026-09-27

## 📦 Uncategorized

- CI: x86_64 macOS release builds, release job restructure, AI code-review updates
   - PR: #20



## [0.6.9] - 2026-03-28

## 📦 Uncategorized

- fix: prevent tray app showing locked state when input is not blocked
   - PR: #19



## [0.6.8] - 2026-03-09

## 📦 Uncategorized

- fix: eliminate desktop stutter from event tap timeout
   - PR: #16

## [0.6.7] - 2026-03-03

## 📦 Uncategorized

- investigate: add sleep/wake stutter telemetry and fix zombie Mach port accumulation
   - PR: #13

## [0.6.6] - 2026-02-27

## 📦 Uncategorized

- feat: reduce passphrase retry delay and add Escape key
   - PR: #12

## [0.6.5] - 2026-02-20

## 📦 Uncategorized

- fix: release CGEventTapRef to prevent desktop stuttering
   - PR: #11

## [0.6.4] - 2025-11-15

- Set default passphrase to `quit`, so user can skip setup and immediately try out HandsOff

## [0.6.3] - 2025-11-15

## 📦 Uncategorized

- Fix Talk hotkey compatibility with Google Meet/Zoom
   - PR: #10

## [0.6.2] - 2025-11-14

## 📦 Uncategorized

- Feature: support configurable hotkeys
   - PR: #9

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

> Note: features listed below reflect the 0.1.0 architecture (Keychain storage, Touch ID, 3-minute auto-lock). Storage moved to config.toml with keycode-v1 passphrase hashes and auto-lock default 180 s in later releases; Touch ID was removed.

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
