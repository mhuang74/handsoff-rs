# HandsOff: CLI and Tray App Design Specification

**Version:** 1.0
**Date:** 2025-10-29
**Status:** Design Phase

---

## 1. Overview

### 1.1 Objective

Convert the `handsoff-rs` repository to produce **two separate binaries** from a single codebase:

1. **`handsoff` (CLI)**: Command-line tool with existing functionality
2. **`handsoff-tray` (Tray App)**: macOS menu bar application with minimal UI

Both binaries will share the same core input-blocking logic but differ in their user interface and interaction model.

### 1.2 Design Goals

- **Code Reuse**: Maximum sharing of core logic between CLI and Tray App
- **Minimal Binary Size**: Target < 5MB for both binaries
- **Native macOS Integration**: Use system-native menu bar and notifications
- **Backward Compatibility**: CLI maintains all existing features and behavior
- **Simple Tray UI**: Lock, Quit, Version, Help menu items only
- **Cross-Architecture**: Support Intel and Apple Silicon (universal binary)
- **macOS Version Support**: 10.11+ (El Capitan and newer)

---

## 2. Architecture Overview

### 2.1 Three-Tier Structure

```
┌─────────────────────────────────────────────────────────┐
│                   User Interface Layer                   │
├──────────────────────────┬──────────────────────────────┤
│   handsoff (CLI)         │   handsoff-tray (Tray App)   │
│   - clap args parsing    │   - tray-icon menu bar       │
│   - Terminal output      │   - notify-rust alerts       │
│   - CFRunLoop only       │   - tao event loop           │
└──────────────────────────┴──────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│              Core Library (libhandsoff)                  │
├─────────────────────────────────────────────────────────┤
│  • Input Blocking (CGEventTap, event handlers)          │
│  • State Management (AppState, Arc<Mutex>)              │
│  • Authentication (passphrase hashing/verification)     │
│  • Hotkeys (global hotkey registration)                 │
│  • Background Threads (auto-lock, buffer reset, etc.)   │
│  • Utilities (keycode mapping, hashing)                 │
└─────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                  macOS System APIs                       │
├─────────────────────────────────────────────────────────┤
│  • CoreGraphics (event tap)                             │
│  • CoreFoundation (run loop)                            │
│  • AppKit (menu bar, notifications)                     │
│  • ApplicationServices (accessibility)                  │
└─────────────────────────────────────────────────────────┘
```

### 2.2 Component Responsibilities

| Component | Responsibility |
|-----------|----------------|
| **Core Library** | Input blocking, state management, authentication, hotkeys, background threads |
| **CLI Binary** | Argument parsing, terminal I/O, simple CFRunLoop |
| **Tray App Binary** | Menu bar UI, notifications, tao event loop integration |

---

## 3. Technology Stack (Recommended)

### 3.1 Stack Selection Rationale

| Technology | Purpose | Why Chosen |
|------------|---------|------------|
| **tray-icon** (v0.17+) | Menu bar icon and menu | Native NSStatusItem, lightweight, event loop compatible |
| **core-graphics** (v0.23+) | Global event interception | CGEventTap for keyboard/mouse blocking, dynamic enable/disable |
| **core-foundation** (v0.9+) | Event loop integration | CFRunLoop integration for event tap |
| **notify-rust** (v4+) | Native notifications | macOS UserNotification for lock/unlock alerts |
| **tao** (v0.28+) | Event loop (Tray App) | Cross-platform AppKit/NSRunLoop wrapper, tray-icon dependency |
| **clap** (v4.5+) | CLI argument parsing | Existing, mature, derive macros |
| **global-hotkey** (v0.6+) | Global hotkeys | Existing, cross-platform |
| **parking_lot** (v0.12+) | Thread-safe locks | Faster than std::sync::Mutex |
| **ring** (v0.17+) | SHA-256 hashing | Existing, cryptographic security |

### 3.2 Why NOT Use These Alternatives

- **egui / iced / cacao**: Too heavy (>10MB), unnecessary for simple menu bar UI
- **Tauri**: Requires web stack (WebView), overkill for menu-only UI
- **rdev with unstable_grab**: Cannot dynamically toggle event blocking (risk of lockout)

---

## 4. Binary Structure

### 4.1 Directory Layout

```
handsoff-rs/
├── src/
│   ├── lib.rs                    # Core library (public API)
│   ├── app_state.rs              # Shared state
│   ├── auth/                     # Authentication module
│   ├── input_blocking/           # Event tap, hotkeys
│   ├── utils/                    # Utilities
│   ├── bin/
│   │   ├── handsoff.rs           # CLI binary (main for CLI)
│   │   └── handsoff-tray.rs      # Tray App binary (main for Tray)
├── Cargo.toml                    # Define [[bin]] targets
├── build.rs                      # Framework linking
└── specs/                        # This document
```

### 4.2 Cargo.toml Binary Configuration

```toml
[[bin]]
name = "handsoff"
path = "src/bin/handsoff.rs"

[[bin]]
name = "handsoff-tray"
path = "src/bin/handsoff-tray.rs"
```

---

## 5. Core Library Design

### 5.1 Public API (`lib.rs`)

The core library exposes a high-level API for both binaries:

```rust
// lib.rs
pub mod app_state;
pub mod auth;
pub mod input_blocking;
pub mod utils;

// Re-exports for convenience
pub use app_state::AppState;
pub use auth::Auth;
pub use input_blocking::{EventTap, HotkeyManager};

// Core functionality
pub struct HandsOffCore {
    pub state: Arc<Mutex<AppState>>,
    pub auth: Auth,
    pub event_tap: EventTap,
    pub hotkey_manager: HotkeyManager,
}

impl HandsOffCore {
    pub fn new(secret_hash: String) -> Result<Self, Error>;
    pub fn start_event_tap(&mut self) -> Result<(), Error>;
    pub fn start_hotkeys(&mut self) -> Result<(), Error>;
    pub fn start_background_threads(&self) -> Result<(), Error>;
    pub fn lock(&self) -> Result<(), Error>;
    pub fn unlock(&self, passphrase: &str) -> Result<bool, Error>;
    pub fn is_locked(&self) -> bool;
}
```

### 5.2 Core Library Modules (Unchanged)

These modules remain **unchanged** from the current implementation:

- **`app_state.rs`**: Thread-safe shared state (`Arc<Mutex<AppState>>`)
- **`auth/mod.rs`**: Passphrase hashing and verification (SHA-256)
- **`input_blocking/event_tap.rs`**: CGEventTap implementation
- **`input_blocking/hotkeys.rs`**: Global hotkey registration
- **`utils/keycode.rs`**: macOS keycode-to-character mapping
- **`utils/mod.rs`**: SHA-256 hashing utilities

### 5.3 Background Threads (Moved to Core Library)

Currently scattered in `main.rs`, these will be encapsulated in the core library:

1. **Buffer Reset Thread**: Clears passphrase input buffer after 5s inactivity
2. **Auto-Lock Thread**: Checks every 5s if inactivity timeout elapsed
3. **Hotkey Listener Thread**: Listens for global hotkey events
4. **Auto-Unlock Thread**: Optional, checks every 10s if auto-unlock timeout reached

All threads will be spawned by `HandsOffCore::start_background_threads()`.

---

## 6. CLI Binary Implementation

### 6.1 File: `src/bin/handsoff.rs`

**Responsibilities:**
- Parse command-line arguments (clap)
- Read environment variables (`HANDS_OFF_SECRET_PHRASE`, `HANDS_OFF_AUTO_LOCK`, etc.)
- Initialize `HandsOffCore` with configuration
- Start event tap, hotkeys, and background threads
- Run CFRunLoop (blocks until Ctrl+C)
- Handle terminal output (log messages, status)

### 6.2 CLI Arguments (Unchanged)

```
handsoff [OPTIONS]

Options:
  -a, --auto-lock <SECONDS>   Auto-lock after inactivity (20-600s, default: 120)
  -h, --help                  Print help
  -V, --version               Print version
```

Environment variables:
- `HANDS_OFF_SECRET_PHRASE`: SHA-256 hash of unlock passphrase (required)
- `HANDS_OFF_AUTO_LOCK`: Auto-lock timeout in seconds (optional)
- `HANDS_OFF_AUTO_UNLOCK`: Auto-unlock timeout in seconds (optional, 0=disabled)

### 6.3 CLI Main Flow

```rust
// src/bin/handsoff.rs
use handsoff::{HandsOffCore, auth::Auth};
use clap::Parser;
use core_foundation::runloop::CFRunLoop;

#[derive(Parser)]
#[command(name = "handsoff", version, about)]
struct Cli {
    #[arg(short, long, value_name = "SECONDS")]
    auto_lock: Option<u64>,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Cli::parse();

    // 1. Load configuration from env vars
    let secret_hash = std::env::var("HANDS_OFF_SECRET_PHRASE")?;
    let auto_lock = args.auto_lock
        .or_else(|| std::env::var("HANDS_OFF_AUTO_LOCK").ok()?.parse().ok());

    // 2. Initialize core
    let mut core = HandsOffCore::new(secret_hash)?;
    core.set_auto_lock(auto_lock);

    // 3. Start components
    core.start_event_tap()?;
    core.start_hotkeys()?;
    core.start_background_threads()?;

    log::info!("HandsOff CLI started. Press Ctrl+C to quit.");

    // 4. Run CFRunLoop (blocks indefinitely)
    CFRunLoop::run_current();

    Ok(())
}
```

### 6.4 CLI Output

- Uses `env_logger` for structured logging
- Log levels: `info!()`, `warn!()`, `error!()`
- Example messages:
  - `"Input locked. Enter passphrase to unlock."`
  - `"Input unlocked."`
  - `"Auto-lock will activate in 30 seconds of inactivity."`

---

## 7. Tray App Implementation

### 7.1 File: `src/bin/handsoff-tray.rs`

**Responsibilities:**
- Create menu bar icon (NSStatusItem via tray-icon)
- Build menu with: Lock, Quit, Version, Help
- Initialize `HandsOffCore` with hardcoded/default configuration
- Integrate CGEventTap with tao event loop
- Send notifications on lock/unlock events
- Handle menu item clicks

### 7.2 Menu Bar Icon

**Icons:**
- **Unlocked**: 🔓 or custom icon (monochrome template image for macOS)
- **Locked**: 🔒 or custom icon

**Implementation:**
```rust
use tray_icon::{TrayIcon, TrayIconBuilder, menu::MenuBuilder};

let icon = include_bytes!("../../assets/unlocked.png");
let tray = TrayIconBuilder::new()
    .with_icon(icon.to_vec())
    .with_tooltip("HandsOff")
    .build()?;
```

### 7.3 Menu Items

**Important:** When input is locked, the event tap blocks ALL mouse clicks, including clicks on the tray menu. Therefore, the menu is inaccessible when locked, and unlock must be performed by typing the passphrase on the keyboard (same as CLI).

| Menu Item | Action | Details |
|-----------|--------|---------|
| **Lock Input** | Lock input immediately | Calls `core.lock()`, changes icon to 🔒. Only functional when unlocked. |
| **---** | Separator | Visual separator |
| **Version** | Show version info | Display alert with version number |
| **Help** | Show help | Display alert with usage instructions |
| **Quit** | Exit app | Gracefully shutdown and exit |

**Note:** There is no "Unlock" menu item because when locked, mouse clicks are blocked and the menu cannot be accessed. Users must type their passphrase to unlock (identical to CLI behavior).

**Menu Structure:**
```
┌─────────────────┐
│  🔓 HandsOff    │  (Tray Icon when unlocked)
├─────────────────┤
│  Lock Input     │
│  ──────────     │
│  Version        │
│  Help           │
│  Quit           │
└─────────────────┘

When locked:
┌─────────────────┐
│  🔒 HandsOff    │  (Icon changes, but menu not clickable)
├─────────────────┤
│  [Menu inaccessible - mouse clicks blocked]
│  Type passphrase to unlock
└─────────────────┘
```

### 7.4 Tray App Main Flow

```rust
// src/bin/handsoff-tray.rs
use handsoff::HandsOffCore;
use tray_icon::{TrayIconBuilder, menu::{MenuBuilder, MenuItem}};
use tao::event_loop::{EventLoop, ControlFlow};
use notify_rust::Notification;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    // 1. Load configuration (default or from file)
    let secret_hash = std::env::var("HANDS_OFF_SECRET_PHRASE")?;

    // 2. Initialize core
    let mut core = HandsOffCore::new(secret_hash)?;
    core.start_event_tap()?;
    core.start_hotkeys()?;
    core.start_background_threads()?;

    // 3. Create event loop (tao)
    let event_loop = EventLoop::new();

    // 4. Build menu
    let lock_item = MenuItem::new("Lock", true, None);
    let version_item = MenuItem::new("Version", true, None);
    let help_item = MenuItem::new("Help", true, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let menu = MenuBuilder::new()
        .add_item(&lock_item)
        .add_separator()
        .add_item(&version_item)
        .add_item(&help_item)
        .add_item(&quit_item)
        .build()?;

    // 5. Create tray icon
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("HandsOff")
        .with_icon(load_unlocked_icon())
        .build()?;

    // 6. Run event loop
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::MenuEvent { id } => {
                if id == lock_item.id() {
                    handle_lock(&core, &tray);
                } else if id == quit_item.id() {
                    *control_flow = ControlFlow::Exit;
                } else if id == version_item.id() {
                    show_version();
                } else if id == help_item.id() {
                    show_help();
                }
            }
            Event::TrayEvent { event, .. } => {
                // Optional: handle tray icon click
            }
            _ => {}
        }
    });

    Ok(())
}

fn handle_lock(core: &HandsOffCore, tray: &TrayIcon) {
    // Note: This function only handles locking, not unlocking.
    // When locked, mouse clicks are blocked, so this menu item is inaccessible.
    // Unlock is done by typing the passphrase (same as CLI).

    if core.is_locked() {
        // This should not be reachable (menu inaccessible when locked)
        // But handle gracefully in case of race condition
        eprintln!("Lock menu clicked while already locked (mouse should be blocked)");
    } else {
        core.lock().ok();
        tray.set_icon(load_locked_icon());
        Notification::new()
            .summary("HandsOff")
            .body("Input locked - Type passphrase to unlock")
            .show()
            .ok();
    }
}
```

### 7.5 Notifications

Use `notify-rust` for native macOS UserNotification alerts:

```rust
use notify_rust::Notification;

Notification::new()
    .summary("HandsOff")
    .body("Input locked")
    .timeout(notify_rust::Timeout::Milliseconds(3000))
    .show()?;
```

**Notification Events:**
- Lock activated: "Input locked"
- Lock deactivated: "Input unlocked"
- Auto-lock triggered: "Auto-lock activated after inactivity"

### 7.6 Unlock Behavior

**Important:** The Tray App does NOT use a passphrase dialog for unlocking. When input is locked, ALL mouse clicks are blocked by the event tap, including clicks on the tray menu. Therefore, users must unlock the same way as the CLI: by typing their passphrase on the keyboard.

**Unlock Process:**
1. When locked, the tray icon changes to 🔒 and shows notification "Input locked - Type passphrase to unlock"
2. User types passphrase on keyboard (input buffer, invisible)
3. On successful match, input unlocks and icon changes to 🔓
4. If passphrase is wrong, buffer clears after 5 seconds and user can retry

This is identical to the CLI behavior and ensures consistency across both binaries.

---

## 8. Migration Plan

### 8.1 Code Changes Summary

| File | Change Type | Description |
|------|-------------|-------------|
| `src/lib.rs` | **Modify** | Export `HandsOffCore` struct with high-level API |
| `src/main.rs` | **Move** | Move to `src/bin/handsoff.rs`, simplify to CLI-only |
| `src/bin/handsoff-tray.rs` | **Create** | New Tray App binary |
| `Cargo.toml` | **Modify** | Add `[[bin]]` targets, new dependencies |
| `build.rs` | **Unchanged** | Keep existing framework linking |
| `src/app_state.rs` | **Unchanged** | Already library code |
| `src/auth/` | **Unchanged** | Already library code |
| `src/input_blocking/` | **Unchanged** | Already library code |
| `src/utils/` | **Unchanged** | Already library code |

### 8.2 Migration Steps

1. **Create `src/bin/` directory**
2. **Move `src/main.rs` → `src/bin/handsoff.rs`**
   - Simplify: remove library code, focus on CLI UX
3. **Create `src/bin/handsoff-tray.rs`**
   - Implement tray menu, notifications, event loop integration
4. **Refactor `src/lib.rs`**
   - Create `HandsOffCore` struct
   - Expose public API for both binaries
   - Move background thread spawning to library
5. **Update `Cargo.toml`**
   - Add `tray-icon`, `tao`, `notify-rust` dependencies
   - Define `[[bin]]` targets
6. **Test both binaries**
   - CLI: Verify all existing functionality works
   - Tray App: Verify menu, lock/unlock, notifications

---

## 9. Dependencies

### 9.1 New Dependencies (Tray App Only)

Add to `Cargo.toml`:

```toml
[dependencies]
# Existing dependencies...
core-graphics = "0.25"
core-foundation = "0.10"
global-hotkey = "0.6"
parking_lot = "0.12"
ring = "0.17"
hex = "0.4"
clap = { version = "4.5", features = ["derive"] }
log = "0.4"
env_logger = "0.11"
anyhow = "1.0"

# New dependencies for Tray App
tray-icon = "0.17"
tao = "0.28"
notify-rust = "4"
objc = "0.2"  # For native dialogs (optional)
```

### 9.2 Feature Flags (Optional Future Enhancement)

To reduce CLI binary size, could use feature flags:

```toml
[features]
default = []
tray = ["tray-icon", "tao", "notify-rust"]

[dependencies]
tray-icon = { version = "0.17", optional = true }
tao = { version = "0.28", optional = true }
notify-rust = { version = "4", optional = true }
```

Then build:
```bash
cargo build --bin handsoff              # CLI only (no tray deps)
cargo build --bin handsoff-tray --features tray
```

---

## 10. Build Configuration

### 10.1 Universal Binary (Intel + Apple Silicon)

Build both architectures:

```bash
# Install targets
rustup target add x86_64-apple-darwin
rustup target add aarch64-apple-darwin

# Build universal binary
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin

# Combine with lipo
lipo -create \
  target/x86_64-apple-darwin/release/handsoff \
  target/aarch64-apple-darwin/release/handsoff \
  -output target/release/handsoff-universal
```

Or use `cargo-universal`:
```bash
cargo install cargo-universal
cargo universal build --release
```

### 10.2 Code Signing (Required for Distribution)

```bash
# Sign the binaries
codesign --force --deep --sign "Developer ID Application: Your Name" \
  target/release/handsoff

codesign --force --deep --sign "Developer ID Application: Your Name" \
  target/release/handsoff-tray

# Verify
codesign --verify --verbose target/release/handsoff
```

### 10.3 App Bundle (Tray App Only)

For Tray App, create `.app` bundle:

```
HandsOff.app/
├── Contents/
│   ├── Info.plist
│   ├── MacOS/
│   │   └── handsoff-tray
│   └── Resources/
│       ├── unlocked.png
│       └── locked.png
```

**Info.plist:**
```xml
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>handsoff-tray</string>
    <key>CFBundleIdentifier</key>
    <string>com.handsoff.tray</string>
    <key>CFBundleName</key>
    <string>HandsOff</string>
    <key>CFBundleVersion</key>
    <string>1.0.0</string>
    <key>LSUIElement</key>
    <true/>  <!-- Hide from Dock -->
</dict>
</plist>
```

---

## 11. Testing Strategy

### 11.1 Unit Tests (Existing)

Keep all existing unit tests in `tests/`:
- `app_state_tests.rs`
- `auth_tests.rs`
- `keycode_tests.rs`

### 11.2 Integration Tests

**CLI Binary:**
1. Launch `handsoff` with `--auto-lock 20`
2. Verify event tap starts
3. Trigger lock hotkey (Ctrl+Cmd+Shift+L)
4. Verify input blocked
5. Send unlock passphrase
6. Verify input unblocked

**Tray App Binary:**
1. Launch `handsoff-tray`
2. Verify menu bar icon appears
3. Click "Lock" menu item
4. Verify icon changes to 🔒
5. Verify notification appears
6. Click "Unlock", enter passphrase
7. Verify icon changes to 🔓

### 11.3 Manual Testing Checklist

- [ ] CLI: Builds successfully
- [ ] CLI: Starts without errors
- [ ] CLI: Lock hotkey works
- [ ] CLI: Auto-lock works
- [ ] CLI: Passphrase unlock works
- [ ] Tray App: Builds successfully
- [ ] Tray App: Menu bar icon appears
- [ ] Tray App: Lock menu item works
- [ ] Tray App: Unlock with passphrase works
- [ ] Tray App: Version shows correct info
- [ ] Tray App: Help shows instructions
- [ ] Tray App: Quit exits cleanly
- [ ] Tray App: Notifications appear
- [ ] Universal binary: Runs on Intel Mac
- [ ] Universal binary: Runs on Apple Silicon Mac

---

## 12. Documentation Updates

### 12.1 README.md Updates

Add sections:
- **Two Modes**: CLI and Tray App
- **Building Both Binaries**: `cargo build --bin handsoff` vs `--bin handsoff-tray`
- **Tray App Usage**: How to launch, menu items, notifications
- **Icon Assets**: Where to place custom icons

### 12.2 Help Text

**CLI (`handsoff --help`):**
```
HandsOff CLI - Block keyboard and mouse input on macOS

Usage: handsoff [OPTIONS]

Options:
  -a, --auto-lock <SECONDS>  Auto-lock after inactivity (20-600s)
  -h, --help                 Print help
  -V, --version              Print version

Environment:
  HANDS_OFF_SECRET_PHRASE    SHA-256 hash of unlock passphrase (required)
  HANDS_OFF_AUTO_LOCK        Auto-lock timeout in seconds (optional)
  HANDS_OFF_AUTO_UNLOCK      Auto-unlock timeout (dev only, 0=disabled)

Hotkeys:
  Ctrl+Cmd+Shift+L           Lock/unlock input
  Ctrl+Cmd+Shift+T (hold)    Talk mode (allow spacebar while held)
```

**Tray App (Help menu item):**
```
HandsOff Tray App

Menu Items:
• Lock: Lock input immediately (or unlock if already locked)
• Version: Show version information
• Help: Show this help text
• Quit: Exit the application

Hotkeys:
• Ctrl+Cmd+Shift+L: Lock/unlock input
• Ctrl+Cmd+Shift+T (hold): Talk mode (allow spacebar)

Configuration:
Set HANDS_OFF_SECRET_PHRASE environment variable before launching.

Permissions:
Requires Accessibility permission in System Settings.
```

---

## 13. Future Enhancements (Out of Scope)

These are **not** part of the initial implementation but could be added later:

1. **Tray App Settings UI**: Popup window for configuring auto-lock, passphrase, hotkeys
2. **Launch at Login**: Add to Login Items automatically
3. **Multiple Lock Profiles**: Different passphrase + settings per profile
4. **Touchbar Support**: Show lock status on MacBook Pro Touch Bar
5. **iCloud Sync**: Sync settings across Macs via iCloud
6. **Whitelist Apps**: Allow specific apps to receive input when locked
7. **Scheduled Lock**: Lock at specific times (e.g., 5pm daily)
8. **Bluetooth Device Lock**: Auto-lock when Bluetooth device disconnects

---

## 14. Open Questions

1. **Icon Assets**: Should we include default PNG icons or use Unicode emoji (🔓/🔒)?
   - **Decision**: Start with Unicode, allow custom icons via `assets/` directory

2. **Passphrase Storage**: Should Tray App store passphrase in macOS Keychain?
   - **Decision**: No, require environment variable for security (same as CLI)

3. **Auto-Lock in Tray App**: Should it be configurable via menu?
   - **Decision**: No, keep menu minimal. Use environment variable like CLI.

4. **Tray App Multi-Instance**: Should we prevent multiple Tray App instances?
   - **Decision**: Yes, use file lock (`.handsoff-tray.lock`) to prevent duplicates.

---

## 15. Success Criteria

The implementation is considered **complete** when:

- ✅ Both `handsoff` and `handsoff-tray` binaries build successfully
- ✅ CLI maintains all existing functionality (no regressions)
- ✅ Tray App shows menu bar icon with Lock, Quit, Version, Help
- ✅ Tray App can lock/unlock input via menu
- ✅ Tray App shows notifications on lock/unlock
- ✅ Both binaries share >90% of core logic (DRY principle)
- ✅ Binary sizes are <5MB each (release build)
- ✅ Universal binary works on Intel and Apple Silicon
- ✅ All existing unit tests pass
- ✅ Documentation updated (README, help text)

---

## 16. Timeline Estimate

| Phase | Duration | Tasks |
|-------|----------|-------|
| **Phase 1: Core Library Refactor** | 2-3 hours | Extract `HandsOffCore`, move threads to lib |
| **Phase 2: CLI Binary** | 1 hour | Move to `src/bin/handsoff.rs`, test |
| **Phase 3: Tray App Binary** | 3-4 hours | Build menu, notifications, event loop |
| **Phase 4: Testing** | 2 hours | Manual testing both binaries |
| **Phase 5: Documentation** | 1 hour | Update README, help text |
| **Total** | **9-11 hours** | |

---

## Appendix A: Dependency Licenses

All dependencies are MIT or Apache-2.0 licensed (permissive):

| Crate | License | Notes |
|-------|---------|-------|
| tray-icon | MIT/Apache-2.0 | Safe |
| tao | Apache-2.0 | Safe |
| notify-rust | MIT/Apache-2.0 | Safe |
| core-graphics | MIT/Apache-2.0 | Safe |
| core-foundation | MIT/Apache-2.0 | Safe |
| clap | MIT/Apache-2.0 | Safe |
| ring | ISC (permissive) | Safe |

---

## Appendix B: Event Loop Architecture

**Why Two Event Loops?**

- **CLI**: Uses `CFRunLoop::run_current()` (CoreFoundation) for minimal overhead
  - Only needs event tap to work, no UI events
  - Blocks until Ctrl+C (SIGINT)

- **Tray App**: Uses `tao::event_loop::EventLoop` (AppKit/NSRunLoop wrapper)
  - Required by `tray-icon` for menu bar integration
  - Handles both UI events (menu clicks) and event tap
  - Can integrate CGEventTap by adding to current run loop:

```rust
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};

// In tray app: add event tap source to current run loop
let run_loop = CFRunLoop::get_current();
run_loop.add_source(&event_tap_source, kCFRunLoopCommonModes);

// Then run tao event loop (which wraps the same NSRunLoop)
event_loop.run(...);
```

This allows a single run loop to handle both CGEventTap and tray menu events.

---

**End of Design Specification**
