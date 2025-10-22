# Input Lock macOS App Specification (Updated)

## 1. Overview
The Input Lock app is a macOS menu bar application designed to prevent accidental or unsolicited input from keyboard, trackpad, and mouse devices during scenarios such as video conferencing with a child present, software demos, or leaving the laptop unattended. The app blocks these inputs while keeping the screen visible and allowing other system functions (e.g., microphone, camera) to operate normally. Unlocking is achieved by entering a secret passphrase on the keyboard or using Touch ID via a specific key combination, with a 5-second input buffer reset to handle accidental input. The app supports enabling the lock via a hotkey, auto-locks after 1 minute of inactivity, and is compatible with macOS versions from the last 10 years (10.11 El Capitan and later) on both Intel and Apple Silicon (M1 and later) Macs.

### 1.1 Purpose
- Prevent disruptions during video conferences (e.g., Zoom, Google Meet) by blocking keyboard, trackpad, and mouse inputs.
- Ensure microphone and camera functionality remain unaffected for seamless video calls.
- Provide secure unlocking via a keyboard-entered passphrase or Touch ID, with a 5-second input buffer reset to handle accidental input (e.g., from a child).
- Allow enabling the lock via a customizable hotkey (e.g., `Ctrl+Cmd+Shift+L`).
- Auto-lock after several minutes of no input activity (configurable, defaults to 3 min)
- Maintain a lightweight, unobtrusive presence via a menu bar interface for enabling the lock and setting the passphrase.
- Ensure compatibility with macOS 10.11 (El Capitan, 2015) and later, supporting both Intel and Apple Silicon architectures.

### 1.2 Use Cases
- **Video Conferencing**: Allow users to participate in video calls with a toddler in their lap without risk of accidental inputs disrupting the call.
- **Software Demos**: Enable safe demonstration of software on a laptop without fear of audience interference via input devices.
- **Unattended Laptop**: Allow users to step away from their laptop while keeping the screen visible, preventing unauthorized or accidental inputs (e.g., by a child).

## 2. Requirements

### 2.1 Functional Requirements
- **Input Blocking**:
  - Block all keyboard inputs (`KeyDown`, `KeyUp`) except for passphrase detection and specific key combinations (e.g., for enabling lock or triggering Touch ID).
  - Block all mouse and trackpad inputs (`MouseMoved`, `LeftMouseDown`, `LeftMouseUp`, `RightMouseDown`, `RightMouseUp`, `ScrollWheel`).
  - Ensure the mouse cursor remains static and clicks are not registered.
  - Allow microphone and camera to function normally for video conferencing apps.
- **Hotkey to Enable Lock**:
  - Support a customizable hotkey (default: `Ctrl+Cmd+Shift+L`) to enable the lock instantly.
  - Allow users to configure the hotkey via a settings UI.
- **Hotkey to Talk**:
  - Support a customizable hotkey (default: `Ctrl+Cmd+Shift+T`) to passthrough Spacebar keypress, which allows the speaker to unmute and talk in most video conf software.
  - Allow users to configure the passthrough keypress via a settings UI (defaults to Spacebar)
- **Auto-Lock**:
  - Automatically enable the lock after 3 minute (180 seconds) of no input activity (keyboard, mouse, or trackpad).
  - Reset the auto-lock timer on any input event.
  - Allow users to configure the auto-lock timeout via a settings UI.
- **Authentication**:
  - Support unlocking via a user-defined passphrase entered on the keyboard.
  - Clear the input buffer after 5 seconds of no key presses to reset accidental or gibberish input (e.g., from a child).
  - Support unlocking via Touch ID, triggered by a specific key combination (e.g., `Ctrl+Cmd+Shift+U`).
  - Fallback to passphrase entry if Touch ID is unavailable or fails.
- **User Interface**:
  - Provide a menu bar icon showing lock status (e.g., 🔓 for unlocked, 🔒 for locked).
  - Offer menu options for “Enable Lock,” “Disable Lock,” “Set Passphrase,” “Set Hotkey,” and “Quit” (accessible only when unlocked).
  - Display a window for setting the passphrase and hotkey configuration.
  - Use macOS’s native Touch ID prompt for fingerprint authentication.
- **Security**:
  - Store the passphrase as a SHA-256 hash in macOS Keychain.
  - Store the hotkey configuration in a secure, local storage (e.g., Keychain or `UserDefaults` equivalent in Rust).
  - Use macOS’s secure enclave for Touch ID authentication.
- **Permissions**:
  - Request Accessibility permissions for input event blocking and hotkey registration.
  - Request Touch ID permissions for fingerprint authentication.

### 2.2 Non-Functional Requirements
- **Performance**: Minimal CPU and memory usage to avoid impacting video conferencing performance.
- **Compatibility**:
  - Support macOS versions from 10.11 (El Capitan, 2015) to 14.x (Sonoma, 2023) and later.
  - Ensure compatibility with both Intel and Apple Silicon (M1 and later) architectures.
- **Security**: Ensure secure storage of sensitive data (passphrase hash, hotkey configuration) and robust authentication.
- **Reliability**: Handle edge cases (e.g., external input devices, failed Touch ID attempts, accidental input) without crashing.
- **Usability**: Allow users to wait 5 seconds after accidental input and then enter the correct passphrase to unlock.

## 3. Tech Stack
- **Language**: Rust
  - Chosen for system-level control and safety guarantees, with support for cross-architecture compilation (Intel and Apple Silicon).
- **Frameworks and Libraries**:
  - `cocoa-rs` (v0.25): Rust bindings for Cocoa (AppKit) to create the menu bar app and native UI, compatible with macOS 10.11+.
  - `core-graphics-rs` (v0.23): For intercepting and blocking input events via `CGEventTap`, supported on macOS 10.11+.
  - `security-framework-rs` (v2.7): For Keychain access to store passphrase hash and hotkey configuration.
  - `keyring-rs` (v2.0): For simplified Keychain integration.
  - `ring` (v0.17): For SHA-256 hashing of the passphrase.
  - `local_auth` (v0.1 or latest): For Touch ID authentication via macOS LocalAuthentication framework (available since macOS 10.12.2).
  - `hex` (v0.4): For encoding hashed passphrase as a string.
  - `global_hotkey` (v0.3 or latest): For registering and handling global hotkeys to enable the lock.
- **Optional UI Framework**: `tauri` (v1.x)
  - Alternative for a lightweight, web-based UI (HTML/CSS/JS) for passphrase and hotkey setting, compatible with macOS 10.11+.
- **Build Tool**: Cargo
  - Rust’s package manager for dependency management and building, with cross-compilation support for Intel and Apple Silicon.

### 3.1 Dependencies
Add to `Cargo.toml`:
```toml
[dependencies]
cocoa = "0.25"
core-graphics = "0.23"
security-framework = "2.7"
keyring = "2.0"
ring = "0.17"
local_auth = "0.1" # Verify latest version
hex = "0.4"
global_hotkey = "0.3" # Verify latest version
```

### 3.2 Compatibility Notes
- **macOS 10.11+**: Use `cocoa-rs` and `core-graphics-rs` APIs compatible with macOS 10.11 (El Capitan). Avoid newer APIs (e.g., introduced in macOS 11 Big Sur) unless guarded with version checks.
- **Intel and Apple Silicon**: Ensure Rust targets `x86_64-apple-darwin` (Intel) and `aarch64-apple-darwin` (Apple Silicon). Use Cargo’s cross-compilation support (`--target`) to build universal binaries.
- **Touch ID**: Available on macOS 10.12.2+ with compatible hardware (e.g., MacBook Pro 2016+). Fall back to passphrase on older systems or non-Touch ID devices.

## 4. Design Details

### 4.1 Architecture
- **App Type**: Menu bar application running in the macOS status bar.
- **Components**:
  - **Menu Bar Interface**: A status bar item with a menu for enabling/disabling the lock, setting the passphrase, configuring the hotkey, and quitting (accessible only when unlocked).
  - **Input Blocking**: A `CGEventTap` to intercept and block input events, with logic to detect passphrase entry, Touch ID trigger, and hotkey for enabling lock.
  - **Auto-Lock**: A background thread to monitor input activity and enable the lock after configured minutes of inactivity.
  - **Authentication Module**: Handles passphrase verification (via keyboard input) and Touch ID authentication (via key combination).
  - **UI Module**: Displays a window for setting the passphrase and hotkey, and triggers the native Touch ID prompt.
- **State Management**:
  - Track lock state (`is_locked: bool`).
  - Store the event tap handle for enabling/disabling input blocking.
  - Maintain an input buffer (`String`) for passphrase detection, cleared after 5 seconds of inactivity.
  - Track the last input time (`Instant`) for auto-lock (1 minute) and buffer reset (5 seconds).
  - Store the passphrase hash and hotkey configuration in Keychain.

### 4.2 User Flow
1. **Launch**: App starts and displays a menu bar icon (🔓 indicating unlocked).
2. **Set Passphrase and Hotkey**:
   - User selects “Set Passphrase” or “Set Hotkey” from the menu (when unlocked).
   - A window prompts for passphrase input (hashed and stored in Keychain) or hotkey configuration (stored in Keychain or local storage).
3. **Enable Lock**:
   - User selects “Enable Lock” from the menu or presses the hotkey (default: `Ctrl+Cmd+Shift+L`).
   - App creates a `CGEventTap` to block all mouse, trackpad, and keyboard events (except for passphrase detection and Touch ID trigger).
   - Menu bar icon changes to 🔒.
4. **Auto-Lock**:
   - App monitors input events (keyboard, mouse, trackpad). If no activity occurs for configured minutes, the lock is enabled automatically.
   - Any input event resets the auto-lock timer.
5. **Video Conferencing**:
   - Microphone and camera continue to function for Zoom, Google Meet, etc.
   - Screen remains visible; all inputs are blocked except for passphrase, Touch ID trigger, or the Talk hotkey.
   - A customizable hotkey (default: `Ctrl+Cmd+Shift+T`) to passthrough Spacebar keypress, which allows the speaker to unmute and talk in most video conf software.
6. **Unlock via Passphrase**:
   - User types the secret passphrase on the keyboard.
   - App captures key presses, builds an input buffer, and checks against the stored passphrase hash.
   - If a child types gibberish, the buffer clears after 5 seconds of no key presses, allowing the user to enter the correct passphrase.
   - On successful match, inputs are unlocked, and the icon changes to 🔓. There should be a quick screen overlay message to make it very clear that input is now unlocked.
7. **Unlock via Touch ID**:
   - User presses a specific key combination (e.g., `Ctrl+Cmd+Shift+U`).
   - App triggers a Touch ID prompt; on success, inputs are unlocked.
   - If Touch ID fails or is unavailable (e.g., on macOS 10.11 or non-Touch ID devices), user can enter the passphrase.

### 4.3 Input Blocking
- Use `CGEventTap` at `CGEventTapLocation::Session` to block:
  - All mouse and trackpad events: `MouseMoved`, `LeftMouseDown`, `LeftMouseUp`, `RightMouseDown`, `RightMouseUp`, `ScrollWheel`.
  - All keyboard events (`KeyDown`, `KeyUp`) except for passphrase detection, Touch ID trigger, and hotkey to enable lock.
- In the `CGEventTap` callback:
  - For `KeyDown` events:
    - Convert keycodes to characters using a keycode-to-character mapping (based on macOS HIToolbox/Events.h, compatible with 10.11+).
    - Append characters to an input buffer (`String`).
    - Update a `last_input_time` timestamp (`Instant`) for buffer reset and auto-lock.
    - Hash the buffer (SHA-256) and compare with the stored hash in Keychain.
    - Return `None` to block the event from reaching other applications.
  - For hotkey to enable lock (e.g., `Ctrl+Cmd+Shift+L`):
    - Check keycode and modifiers (e.g., `CGEventFlags::CGEventFlagControl | CGEventFlagCommand | CGEventFlagShift`).
    - Trigger lock enablement if unlocked; return `Some(())` to allow the event.
  - For Touch ID trigger (e.g., `Ctrl+Cmd+Shift+U`):
    - Check keycode and modifiers.
    - Trigger the Touch ID prompt using `local_auth`.
    - Return `Some(())` to allow the event.
  - For all other events: Return `None` to block.
- Run a background thread to:
  - Clear the input buffer after 5 seconds of no key presses (for passphrase detection).
  - Enable the lock after 1 minute of no input activity (keyboard, mouse, or trackpad).
- Request Accessibility permissions to enable the event tap and hotkey registration.

### 4.4 Authentication
- **Passphrase**:
  - Capture `KeyDown` events in the `CGEventTap` to build an input buffer.
  - Hash the buffer using `ring` (SHA-256) and compare with the stored hash in Keychain (`keyring-rs`).
  - Clear the buffer after successful unlock or 5 seconds of inactivity.
  - For setting the passphrase, display a UI window (via `cocoa-rs` or `tauri`) to accept user input, hash it, and store it in Keychain.
- **Touch ID**:
  - Detect a specific key combination (e.g., `Ctrl+Cmd+Shift+U`) in the `CGEventTap`.
  - Use `local_auth` to check if Touch ID is available (`can_evaluate_policy`, supported on macOS 10.12.2+).
  - Prompt for fingerprint authentication with `evaluate_policy` and reason “Unlock input devices.”
  - Fallback to passphrase entry if Touch ID fails, is unavailable, or on macOS 10.11.
- **Hotkey**:
  - Register a global hotkey (e.g., `Ctrl+Cmd+Shift+L`) using `global_hotkey` to enable the lock.
  - Store the hotkey configuration in Keychain or local storage.
  - Allow users to customize the hotkey via a settings UI.

### 4.5 User Interface
- **Menu Bar**:
  - Icon: Use system symbols (🔓 for unlocked, 🔒 for locked).
  - Menu items: “Enable Lock,” “Disable Lock,” “Set Passphrase,” “Set Hotkey,” “Quit” (accessible only when unlocked).
- **Passphrase and Hotkey Setting Window**:
  - Native Option: Use `cocoa-rs` to create an `NSWindow` with an `NSTextField` (secure) for passphrase input, an input for hotkey configuration, and a “Save” button.
  - Tauri Option: Use a web-based form (HTML/CSS/JS) with a secure text input for passphrase, a hotkey capture field, and a submit button, communicating with Rust via Tauri’s API.
- **Touch ID Prompt**: Triggered via `local_auth`, displaying macOS’s native Touch ID dialog when the key combination is detected.
- **Optional**: Add a subtle screen overlay or blinking menu bar icon to indicate locked state during video calls.

## 5. Security Considerations
- Store the passphrase as a SHA-256 hash in Keychain using `keyring-rs`, not plain text.
- Store the hotkey configuration securely (e.g., Keychain or encrypted local storage).
- Use macOS’s secure enclave for Touch ID authentication via `local_auth` (macOS 10.12.2+).
- Prompt for Accessibility permissions at launch if not granted.
- Handle failed authentication attempts gracefully (e.g., retry Touch ID or continue passphrase entry).
- Ensure the input buffer is cleared after 5 seconds of inactivity to prevent accidental passphrase matches.

## 6. Edge Cases
- **External Devices**: Ensure `CGEventTap` blocks inputs from external keyboards, mice, and trackpads.
- **Video Conferencing**: Verify microphone and camera functionality in Zoom and Google Meet with the lock enabled.
- **Accidental Input**: Clear the input buffer after 5 seconds of no key presses to reset gibberish input from a child.
- **Failed Authentication**: Fallback to passphrase if Touch ID fails, is unavailable, or on macOS 10.11.
- **Permissions**: Prompt user to grant Accessibility permissions if missing, with clear instructions.
- **Hotkey Conflicts**: Ensure the hotkey (e.g., `Ctrl+Cmd+Shift+L`) does not conflict with common video conferencing hotkeys; allow customization.
- **Auto-Lock**: Reset the 1-minute timer on any input event to prevent locking during active use.

## 7. Testing Plan
- **Functional Tests**:
  - Verify input blocking (keyboard, mouse, trackpad) during a Zoom/Google Meet call.
  - Confirm microphone and camera functionality are unaffected.
  - Test passphrase entry: Enter correct passphrase to unlock; simulate gibberish input, wait 5 seconds, and enter correct passphrase.
  - Test Touch ID: Trigger via key combination (e.g., `Ctrl+Cmd+Shift+U`), verify unlock, and test passphrase fallback.
  - Test Lock hotkey: Enable lock via default hotkey (`Ctrl+Cmd+Shift+L`) and custom hotkey.
  - Test Talk hotkey: Enable conference talk via default hotkey (`Ctrl+Cmd+Shift+T`) and custom hotkey.
  - Test auto-lock: Confirm lock engages after 1 minute of no input; verify timer reset on input.
  - Validate menu bar icon updates (🔓/🔒).
- **Compatibility Tests**:
  - Test on macOS 10.11 (El Capitan), 10.12 (Sierra), 10.15 (Catalina), 13.x (Ventura), and 14.x (Sonoma).
  - Test on Intel and Apple Silicon (M1/M2) Macs using universal binaries.
  - Verify Touch ID functionality on macOS 10.12.2+ with compatible hardware; test passphrase fallback on 10.11.
- **Edge Case Tests**:
  - Test with no Touch ID enrolled or on non-Touch ID devices (passphrase fallback).
  - Test with Accessibility permissions disabled.
  - Test buffer reset with rapid, random key presses followed by a 5-second pause.
  - Test auto-lock with various input patterns (e.g., sporadic mouse movement).
  - Test with multiple input devices connected (built-in and external).

## 8. Future Enhancements
- Allow selective input blocking (e.g., keyboard only).
- Implement specific hotkey passthrough (e.g., Zoom’s mute hotkey) if requested.
- Enhance UI with a visual lock indicator (e.g., screen overlay).
- Add option to disable auto-lock in settings.

## 9. Implementation Notes
- **Permissions**: Prompt for Accessibility permissions at launch using a native dialog. Provide instructions to enable in System Settings > Privacy & Security > Accessibility.
- **Error Handling**: Log errors (e.g., failed event tap creation, authentication errors, hotkey registration failures) to a file or console for debugging.
- **Keycode Mapping**: Implement a complete keycode-to-character mapping based on macOS HIToolbox/Events.h (compatible with 10.11+) for accurate passphrase detection.
- **Thread Safety**: Use `Arc<Mutex<>>` for the input buffer, last input time, and auto-lock timer to ensure thread-safe access.
- **Compatibility**:
  - Use conditional compilation or runtime version checks (e.g., `std::env::var("MAC_OS_X_VERSION_MAX_ALLOWED")`) to avoid newer APIs on macOS 10.11.
  - Build universal binaries with `cargo build --target x86_64-apple-darwin` and `aarch64-apple-darwin`.
- **Hotkey Registration**: Use `global_hotkey` to register the hotkey globally, ensuring it works even when the app is not in focus.
- **Dependencies**: Verify crate versions in `Cargo.toml` match the latest stable releases compatible with macOS 10.11.
- **Code Structure**:
  - Modularize input blocking, authentication, auto-lock, hotkey handling, and UI into separate modules.
  - Use Rust’s ownership model to manage `AppState` safely.
- **Sample Code**: Use the provided `main.rs` (artifact ID: c06a34c8-6b25-4624-85fc-6d4fff9a447d) as a starting point for the menu bar, input blocking, and passphrase detection. Extend with Touch ID authentication (artifact ID: 341a9c32-60b5-47ff-a276-b87ad734334b), hotkey registration, and auto-lock logic.

## 10. Deliverables
- Rust source code for the menu bar app, input blocking, passphrase detection, Touch ID authentication, hotkey registration, and auto-lock.
- `Cargo.toml` with all required dependencies.
- Optional Tauri-based UI for passphrase and hotkey setting if chosen over native Cocoa.
- Complete keycode-to-character mapping for passphrase input.
- Universal binary supporting Intel and Apple Silicon.
- Documentation for building and running the app on macOS 10.11+.
- Instructions for granting Accessibility and Touch ID permissions.