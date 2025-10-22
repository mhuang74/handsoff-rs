# Development Notes

## Implementation Overview

This project implements a macOS input locking application using Rust, following the specifications in `specs/handsoff-design.md`.

## Key Components

### 1. Event Tap (CGEventTap)

The core functionality uses macOS's `CGEventTap` API to intercept and block input events at the system level.

**Location**: `src/input_blocking/event_tap.rs`

Key implementation details:
- Uses raw FFI bindings to CoreGraphics framework
- Creates an event tap at the session level to intercept all input events
- Event types monitored: KeyDown, KeyUp, MouseMoved, LeftMouseDown, LeftMouseUp, RightMouseDown, RightMouseUp, ScrollWheel
- Callback function processes events and returns `null` to block or the event to pass through

### 2. Passphrase Authentication

**Location**: `src/auth/mod.rs`, `src/utils/mod.rs`

- Uses SHA-256 hashing (via `ring` crate) for secure passphrase storage
- Keycode-to-character mapping for US keyboard layout
- Input buffer with 5-second timeout for accidental input reset

### 3. Keychain Integration

**Location**: `src/auth/keychain.rs`

- Stores passphrase hash securely in macOS Keychain
- Uses service name: `com.handsoff.inputlock`
- Also stores hotkey configurations and auto-lock timeout

### 4. Global Hotkeys

**Location**: `src/input_blocking/hotkeys.rs`

Implemented hotkeys:
- Lock: `Ctrl+Cmd+Shift+L`
- Talk: `Ctrl+Cmd+Shift+T`
- Touch ID: `Ctrl+Cmd+Shift+U` (handled in event tap)

### 5. Menu Bar Interface

**Location**: `src/ui/menubar.rs`

- Uses Cocoa's NSStatusBar for menu bar integration
- Shows 🔓 when unlocked, 🔒 when locked
- Menu items disabled when locked for security

### 6. Background Threads

**Location**: `src/main.rs`

Three background threads:
1. Buffer reset thread: Checks every second if buffer should be cleared (5s timeout)
2. Auto-lock thread: Checks every 10 seconds if auto-lock should engage (3min timeout)
3. Hotkey listener thread: Monitors for global hotkey events

## Technical Challenges & Solutions

### Challenge 1: CGEventTap API Access

**Problem**: The `core-graphics` crate doesn't expose all necessary CGEventTap functions.

**Solution**: Implemented raw FFI bindings using `extern "C"` blocks to access CoreGraphics framework directly.

### Challenge 2: CGEventType Comparison

**Problem**: `CGEventType` doesn't implement `PartialEq` trait.

**Solution**: Cast to `u32` for comparisons: `(event_type as u32) == (CGEventType::KeyDown as u32)`

### Challenge 3: Event Tap Callback Type Safety

**Problem**: Event tap callback needs to pass state between Rust and C.

**Solution**:
- Box the state and convert to raw pointer: `Box::into_raw(Box::new(state))`
- Reconstruct in callback without taking ownership: `&*(user_info as *const Arc<AppState>)`

### Challenge 4: Touch ID Integration

**Problem**: No simple Rust bindings for LocalAuthentication framework.

**Solution**: Used `osascript` to trigger system authentication dialogs. For production use, would implement proper FFI bindings to LocalAuthentication.

### Challenge 5: Keycode to Character Mapping

**Problem**: Need to convert macOS keycodes to characters for passphrase input.

**Solution**: Implemented comprehensive mapping based on HIToolbox/Events.h for US keyboard layout with shift modifier support.

## Build Notes

### Dependencies

- `cocoa`: Cocoa (AppKit) bindings for menu bar and UI
- `core-graphics`: CoreGraphics for event handling
- `foreign-types`: FFI type conversions
- `security-framework`: macOS security APIs
- `keyring`: Simplified keychain access
- `ring`: Cryptographic hashing
- `global-hotkey`: Global hotkey registration
- `parking_lot`: Fast mutex implementation

### Compatibility

- Targets macOS 10.11+ (El Capitan)
- Works on both Intel (`x86_64-apple-darwin`) and Apple Silicon (`aarch64-apple-darwin`)
- Uses edition 2021 for modern Rust features

## Testing Approach

1. **Accessibility Permissions**: Check at startup, show dialog if not granted
2. **Event Blocking**: Verify all input types are blocked when locked
3. **Passphrase**: Test correct unlock, gibberish + reset, special characters
4. **Auto-lock**: Verify timeout works with various activity patterns
5. **Hotkeys**: Test all three hotkeys in locked and unlocked states

## Future Enhancements

1. **Menu Bar Actions**: Wire up menu items to actually enable/disable lock
2. **Talk Hotkey**: Implement proper spacebar passthrough
3. **Settings UI**: Create proper settings window for configuration
4. **Notification Improvements**: Add full-screen overlay for unlock confirmation
5. **Hotkey Customization**: Allow users to configure hotkeys via UI
6. **Selective Blocking**: Option to block only keyboard or only mouse
7. **Hotkey Passthrough**: Allow specific app hotkeys (e.g., Zoom mute) to work

## Known Limitations

1. Touch ID implementation uses osascript instead of direct LocalAuthentication framework
2. Menu bar actions need proper delegate implementation
3. Talk hotkey framework exists but needs spacebar event injection
4. No visual lock indicator beyond menu bar icon
5. Settings are stored in keychain but no UI to modify them

## Code Quality

- Modular architecture with clear separation of concerns
- Comprehensive error handling using `anyhow`
- Thread-safe state management using `Arc<Mutex<>>`
- Logging with `log` and `env_logger`
- Follows Rust best practices for safety and ownership

## Performance Considerations

- Event tap callback is performance-critical (runs on every input event)
- Minimal allocations in hot path
- Background threads use reasonable polling intervals
- State mutations are fast (simple flag checks and string operations)
