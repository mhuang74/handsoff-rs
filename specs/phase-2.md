# HandsOff Phase 2 Implementation Plan

## Executive Summary

This document outlines the implementation plan for missing features in the HandsOff Input Lock application. Based on a review of the current codebase against the design specification (`specs/handsoff-design.md`), most core features have been implemented successfully. Phase 2 focuses on completing the remaining functionality, improving user experience, and enhancing configurability.

## Current Implementation Status

### ✅ Completed Features

1. **Core Input Blocking**
   - ✅ CGEventTap-based input blocking for keyboard, mouse, and trackpad
   - ✅ Passphrase detection with SHA-256 hashing
   - ✅ 5-second input buffer reset for accidental input handling
   - ✅ Touch ID authentication via Ctrl+Cmd+Shift+U
   - ✅ Keychain integration for secure storage
   - ✅ Backspace support in passphrase entry

2. **Hotkey Support**
   - ✅ Lock hotkey (Ctrl+Cmd+Shift+L) to enable lock
   - ✅ Talk hotkey (Ctrl+Cmd+Shift+T) with spacebar passthrough
   - ✅ Global hotkey registration using `global-hotkey` crate

3. **Auto-Lock Functionality**
   - ✅ Auto-lock after configurable timeout (default: 3 minutes)
   - ✅ Input activity tracking to reset timer
   - ✅ Keychain storage for auto-lock timeout preference

4. **Menu Bar Interface**
   - ✅ Status bar icon with lock/unlock states (🔒/🔓)
   - ✅ Basic menu structure (Enable Lock, Disable Lock, Set Passphrase, Settings, Quit)
   - ✅ Menu bar icon updates from any thread

5. **User Feedback**
   - ✅ Lock/unlock notifications via NSUserNotification
   - ✅ Permissions dialog for Accessibility access

6. **Architecture**
   - ✅ Thread-safe AppState with parking_lot Mutex
   - ✅ Modular code structure (auth, input_blocking, ui, utils)
   - ✅ Background threads for buffer reset, auto-lock, and hotkey listening
   - ✅ Comprehensive logging with env_logger

### ❌ Missing Features (Phase 2)

## Phase 2 Tasks

### 1. Functional Menu Bar Actions

**Status**: Menu items exist but actions are not wired up

**Current State**:
- Menu items are created in `src/ui/menubar.rs` with selectors like `enableLock:`, `disableLock:`, `setPassphrase:`, `showSettings:`, and `terminate:`
- No Objective-C delegate class exists to handle these actions
- Clicking menu items currently does nothing

**Implementation**:

#### 1.1 Create NSObject Delegate Class
**File**: `src/ui/menubar.rs`

**Tasks**:
- Use `objc::declare` to create a custom NSObject subclass (e.g., `MenuBarDelegate`)
- Add instance variable to store Arc<AppState> pointer
- Implement action methods:
  - `enableLock:` - Set state.is_locked to true, update icon, show notification
  - `disableLock:` - Prompt for passphrase, then unlock if valid
  - `setPassphrase:` - Show dialog, hash and store new passphrase
  - `showSettings:` - Display settings window (see section 2)
  - `terminate:` - Clean up and quit application
- Set the delegate as the target for each menu item

**Example Pattern**:
```rust
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};

// Declare MenuBarDelegate class
let superclass = class!(NSObject);
let mut decl = ClassDecl::new("MenuBarDelegate", superclass).unwrap();

// Add state storage
decl.add_ivar::<usize>("state_ptr");

// Add action methods
extern "C" fn enable_lock(this: &Object, _cmd: Sel, _sender: id) {
    // Get state from ivar and enable lock
}

unsafe {
    decl.add_method(
        sel!(enableLock:),
        enable_lock as extern "C" fn(&Object, Sel, id)
    );
}

let delegate_class = decl.register();
```

**Acceptance Criteria**:
- Clicking "Enable Lock" locks input and updates icon
- Clicking "Disable Lock" prompts for passphrase and unlocks on success
- Clicking "Set Passphrase" opens dialog and stores new passphrase
- Clicking "Settings" opens settings window
- Clicking "Quit" exits the application cleanly

---

#### 1.2 Dynamic Menu Item Enablement
**File**: `src/ui/menubar.rs`

**Tasks**:
- Implement NSMenuDelegate to update menu items before display
- Disable "Set Passphrase", "Settings", and "Quit" when locked
- Enable "Disable Lock" only when locked
- Enable "Enable Lock" only when unlocked

**Implementation**:
- Add `menuNeedsUpdate:` delegate method
- Call `update_menu_items()` method (already exists but unused)

**Acceptance Criteria**:
- Menu items are properly enabled/disabled based on lock state
- Locked state prevents access to sensitive operations

---

### 2. Settings UI with Configuration Options

**Status**: Settings dialog shows static text, no configuration possible

**Current State**:
- `show_settings_dialog()` in `src/ui/dialogs.rs` displays hardcoded settings
- No ability to customize hotkeys, timeout, or passthrough key
- Auto-lock timeout stored in keychain but not user-configurable

**Implementation**:

#### 2.1 Settings Window Structure
**File**: `src/ui/settings.rs` (new file)

**Tasks**:
- Create NSWindow-based settings panel using Cocoa
- Use NSTextField for timeout input
- Use custom hotkey capture fields for lock/talk hotkeys
- Use NSPopUpButton for passthrough key selection (Space, Enter, etc.)
- Add "Save" and "Cancel" buttons

**Layout**:
```
+------------------------------------------+
| Settings                                 |
|------------------------------------------|
| Auto-lock timeout: [___] seconds         |
|                                          |
| Lock Hotkey:  [Ctrl+Cmd+Shift+L] Capture|
| Talk Hotkey:  [Ctrl+Cmd+Shift+T] Capture|
|                                          |
| Talk passthrough key: [Spacebar ▾]      |
|                                          |
|                    [Cancel]  [Save]      |
+------------------------------------------+
```

**Acceptance Criteria**:
- Settings window opens as modal sheet or separate window
- All fields display current values on open
- Changes are validated before saving

---

#### 2.2 Hotkey Capture Functionality
**File**: `src/ui/settings.rs`

**Tasks**:
- Implement hotkey capture text field that listens for key combinations
- Detect modifiers (Ctrl, Cmd, Shift, Option) + key press
- Display captured hotkey in human-readable format
- Validate hotkey is not conflicting with system shortcuts
- Store hotkey configuration in keychain using existing functions

**Implementation Details**:
- Create NSTextField with custom event handler
- Override `keyDown:` to capture key events
- Convert CGKeyCode + modifiers to serializable format (JSON or string)
- Use `store_lock_hotkey()` and `store_talk_hotkey()` from `auth/keychain.rs`

**Acceptance Criteria**:
- User can click "Capture" button and press key combination
- Hotkey is displayed clearly (e.g., "⌃⌘⇧L")
- Saved hotkeys persist across app restarts

---

#### 2.3 Dynamic Hotkey Registration
**File**: `src/input_blocking/hotkeys.rs`

**Tasks**:
- Modify `HotkeyManager` to support custom hotkey registration
- Load hotkeys from keychain at startup (main.rs)
- Unregister default hotkeys if custom ones are configured
- Add methods: `register_custom_lock_hotkey(modifiers, keycode)` and `register_custom_talk_hotkey(modifiers, keycode)`

**Implementation**:
```rust
pub fn register_custom_lock_hotkey(&mut self, modifiers: Modifiers, code: Code) -> Result<()> {
    // Unregister existing if present
    if let Some(hotkey) = self.lock_hotkey.take() {
        self.manager.unregister(hotkey)?;
    }

    let hotkey = HotKey::new(Some(modifiers), code);
    self.manager.register(hotkey)?;
    self.lock_hotkey = Some(hotkey);
    Ok(())
}
```

**Acceptance Criteria**:
- Custom hotkeys work immediately after saving settings
- Hotkeys persist across app restarts
- Invalid hotkeys show error messages

---

#### 2.4 Configurable Talk Passthrough Key
**File**: `src/app_state.rs`, `src/input_blocking/mod.rs`

**Tasks**:
- Add `talk_passthrough_keycode` field to AppStateInner (default: 49 for spacebar)
- Modify `handle_keyboard_event()` to use configurable keycode
- Add keychain storage functions in `auth/keychain.rs`
- Add dropdown in settings UI to select passthrough key

**Supported Keys**:
- Spacebar (49)
- Return/Enter (36)
- Tab (48)

**Acceptance Criteria**:
- User can select passthrough key from dropdown
- Selected key is correctly passed through when Talk hotkey is held
- Configuration persists across restarts

---

#### 2.5 Configurable Auto-Lock Timeout
**File**: `src/ui/settings.rs`, `src/main.rs`

**Tasks**:
- Add NSTextField for timeout input in seconds
- Validate input (minimum 30 seconds, maximum 3600 seconds)
- Save using existing `store_auto_lock_timeout()` function
- Reload timeout from keychain after saving
- Update AppState.auto_lock_timeout dynamically

**Acceptance Criteria**:
- User can set custom auto-lock timeout
- Invalid values (non-numeric, too small/large) show error
- New timeout takes effect immediately without restart

---

### 3. Improved Touch ID Implementation

**Status**: Basic Touch ID works but uses insecure workaround

**Current State**:
- `src/auth/touchid.rs` uses `osascript` with admin privileges
- Not true Touch ID - prompts for admin password
- Comment notes this should use LocalAuthentication framework

**Implementation**:

#### 3.1 LocalAuthentication Framework FFI
**File**: `src/auth/touchid.rs`

**Tasks**:
- Create Objective-C FFI bindings for LocalAuthentication framework
- Implement LAContext class and methods:
  - `canEvaluatePolicy:error:` - Check if Touch ID is available
  - `evaluatePolicy:localizedReason:reply:` - Trigger Touch ID prompt
- Use `LAPolicyDeviceOwnerAuthenticationWithBiometrics` policy

**Example FFI Structure**:
```rust
#[link(name = "LocalAuthentication", kind = "framework")]
extern "C" {
    fn LAContext_class() -> *const Class;
}

// Create LAContext instance
let context: id = msg_send![class!(LAContext), alloc];
let context: id = msg_send![context, init];

// Check availability
let policy = 2; // LAPolicyDeviceOwnerAuthenticationWithBiometrics
let mut error: id = nil;
let can_evaluate: bool = msg_send![context, canEvaluatePolicy:policy error:&mut error];

// Evaluate with callback
let reason = NSString::alloc(nil).init_str("Unlock HandsOff input");
msg_send![context, evaluatePolicy:policy
                  localizedReason:reason
                  reply:^(BOOL success, NSError *error) {
                      // Handle result
                  }];
```

**Acceptance Criteria**:
- Touch ID prompt shows native biometric dialog
- Does not prompt for admin password
- Falls back gracefully on non-Touch ID devices
- Works on macOS 10.12.2+ with Touch ID hardware

---

#### 3.2 Block-Based Reply Handler
**File**: `src/auth/touchid.rs`

**Tasks**:
- Implement Objective-C block callback for async Touch ID result
- Use `block` crate or manual block implementation
- Send result back to Rust via channel or callback

**Implementation**:
```rust
use std::sync::mpsc::channel;

let (tx, rx) = channel();

// Create block that captures sender
let block = ConcreteBlock::new(move |success: bool, error: id| {
    tx.send(success).ok();
});

let reply_block = &*block as *const _ as id;
msg_send![context, evaluatePolicy:policy localizedReason:reason reply:reply_block];

// Wait for result
let success = rx.recv_timeout(Duration::from_secs(30)).unwrap_or(false);
```

**Acceptance Criteria**:
- Touch ID authentication completes asynchronously
- Result is properly communicated back to event tap thread
- Timeout handling prevents hanging

---

### 4. Enhanced Unlock Notification

**Status**: Uses deprecated NSUserNotification, not very visible

**Current State**:
- `show_unlock_notification()` in `src/ui/notifications.rs` uses NSUserNotification
- NSUserNotification is deprecated in macOS 11+
- Notification may not be visible during active video calls
- Spec requires "quick screen overlay message" for clarity

**Implementation**:

#### 4.1 Full-Screen Overlay Window
**File**: `src/ui/overlay.rs` (new file)

**Tasks**:
- Create NSWindow with fullscreen overlay properties
- Set window level to float above all windows (NSStatusWindowLevel or NSPopUpMenuWindowLevel)
- Make window transparent with background color (alpha 0.7)
- Display large, centered text: "🔓 INPUT UNLOCKED"
- Auto-dismiss after 1.5 seconds
- Ensure window doesn't capture focus or input

**Window Configuration**:
```rust
let window_style_mask = NSWindowStyleMask::NSBorderlessWindowMask;
let window: id = NSWindow::alloc(nil).initWithContentRect_styleMask_backing_defer_(
    screen_rect,
    window_style_mask,
    NSBackingStoreBuffered,
    NO
);

// Configure window
let _: () = msg_send![window, setLevel: NSStatusWindowLevel];
let _: () = msg_send![window, setBackgroundColor: semi_transparent_green];
let _: () = msg_send![window, setOpaque: NO];
let _: () = msg_send![window, setIgnoresMouseEvents: YES];
let _: () = msg_send![window, setCollectionBehavior: NSWindowCollectionBehaviorCanJoinAllSpaces];
```

**Acceptance Criteria**:
- Overlay is visible above all applications, including fullscreen video calls
- Message is large and clear
- Overlay dismisses automatically after 1.5 seconds
- Does not interfere with video conferencing UI

---

#### 4.2 Fallback to UNUserNotification
**File**: `src/ui/notifications.rs`

**Tasks**:
- Add runtime macOS version detection
- For macOS 10.14+, migrate to UNUserNotificationCenter
- Keep NSUserNotification for macOS 10.11-10.13
- Request notification permissions at startup if needed

**Implementation**:
```rust
#[link(name = "UserNotifications", kind = "framework")]
extern "C" {
    // UNUserNotificationCenter bindings
}

// Check macOS version
let version = get_macos_version();
if version >= (10, 14, 0) {
    use_un_user_notification();
} else {
    use_ns_user_notification();
}
```

**Acceptance Criteria**:
- Notifications work on all macOS versions 10.11+
- No deprecation warnings on modern macOS
- User is prompted for notification permissions if needed

---

### 5. Code Quality Improvements

#### 5.1 Error Handling in Event Tap
**File**: `src/input_blocking/event_tap.rs`, `src/input_blocking/mod.rs`

**Tasks**:
- Add error handling for Touch ID spawn thread failures
- Log errors when passphrase verification fails
- Handle edge cases (null events, invalid keycodes)
- Add panic handler to prevent event tap crashes

**Acceptance Criteria**:
- No panics in event tap callback
- All errors are logged for debugging
- App remains stable under error conditions

---

#### 5.2 Universal Binary Build Support
**File**: `Cargo.toml`, `.cargo/config.toml` (new)

**Tasks**:
- Add build script or configuration for universal binary
- Document build process for both architectures
- Test on Intel and Apple Silicon Macs

**Implementation**:
```toml
# .cargo/config.toml
[target.aarch64-apple-darwin]
rustflags = ["-C", "link-arg=-mmacosx-version-min=10.11"]

[target.x86_64-apple-darwin]
rustflags = ["-C", "link-arg=-mmacosx-version-min=10.11"]
```

**Build Commands**:
```bash
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
lipo -create -output handsoff target/x86_64-apple-darwin/release/handsoff target/aarch64-apple-darwin/release/handsoff
```

**Acceptance Criteria**:
- Universal binary runs on both Intel and Apple Silicon
- Minimum deployment target is macOS 10.11
- Build process is documented in DEVELOPMENT.md

---

#### 5.3 Comprehensive Testing
**File**: `tests/` directory (new)

**Tasks**:
- Add unit tests for passphrase hashing and verification
- Add integration tests for keychain operations
- Add tests for keycode conversion
- Document manual testing procedures

**Test Coverage**:
- `auth::hash_passphrase()` and `auth::verify_passphrase()`
- `keychain::store_*()` and `keychain::retrieve_*()` functions
- `keycode_to_char()` with various modifiers
- AppState thread safety

**Acceptance Criteria**:
- `cargo test` passes all tests
- Manual testing checklist completed (see section 6)

---

### 6. Documentation Updates

#### 6.1 User Documentation
**File**: `README.md`

**Tasks**:
- Add screenshots of menu bar and settings UI
- Document all hotkeys and their defaults
- Provide troubleshooting section
- Add FAQ for common issues

**Acceptance Criteria**:
- README is complete and user-friendly
- New users can install and use the app without confusion

---

#### 6.2 Developer Documentation
**File**: `DEVELOPMENT.md`

**Tasks**:
- Document architecture and module structure
- Explain event tap implementation details
- Provide debugging tips for Accessibility permissions
- Document build and release process

**Acceptance Criteria**:
- New developers can understand the codebase
- Build instructions are complete and tested

---

## Implementation Priority

### High Priority (Must Have for v1.0)
1. Functional Menu Bar Actions (1.1, 1.2) - Core UX blocker
2. Enhanced Unlock Notification (4.1) - Spec requirement
3. Improved Touch ID Implementation (3.1, 3.2) - Security and UX

### Medium Priority (Should Have for v1.0)
4. Settings UI with Auto-Lock Timeout (2.1, 2.5) - User configurability
5. Dynamic Hotkey Registration (2.3) - Flexibility
6. Universal Binary Support (5.2) - Compatibility

### Low Priority (Nice to Have for v1.1)
7. Custom Hotkey Capture (2.2) - Advanced feature
8. Configurable Talk Passthrough Key (2.4) - Edge case
9. Fallback Notifications (4.2) - Modernization
10. Comprehensive Testing (5.3) - Quality assurance
11. Documentation Updates (6.1, 6.2) - Ongoing

---

## Testing Checklist

For each completed feature, perform the following tests:

### Menu Bar Actions
- [ ] Enable Lock from menu while unlocked
- [ ] Disable Lock from menu with correct passphrase
- [ ] Disable Lock from menu with incorrect passphrase (should fail)
- [ ] Set Passphrase from menu and verify it works
- [ ] Open Settings from menu when unlocked
- [ ] Verify Settings menu item is disabled when locked
- [ ] Quit from menu

### Settings UI
- [ ] Open settings and verify current values are displayed
- [ ] Change auto-lock timeout and verify it persists
- [ ] Change lock hotkey and verify it works immediately
- [ ] Change talk hotkey and verify spacebar passthrough works
- [ ] Cancel settings changes and verify values unchanged
- [ ] Save settings with invalid values and verify error messages

### Touch ID
- [ ] Trigger Touch ID with Ctrl+Cmd+Shift+U while locked
- [ ] Authenticate successfully with fingerprint
- [ ] Cancel Touch ID prompt and verify still locked
- [ ] Test on device without Touch ID (should fail gracefully)

### Unlock Notification
- [ ] Unlock via passphrase and verify overlay is visible
- [ ] Unlock via Touch ID and verify overlay is visible
- [ ] Verify overlay dismisses after ~1.5 seconds
- [ ] Test during fullscreen video call to ensure visibility

### Hotkeys
- [ ] Lock via Ctrl+Cmd+Shift+L hotkey
- [ ] Hold Ctrl+Cmd+Shift+T and press spacebar to test passthrough
- [ ] Release talk hotkey and verify spacebar is blocked again
- [ ] Test custom hotkeys after configuration

### Auto-Lock
- [ ] Verify auto-lock engages after configured timeout
- [ ] Verify mouse movement resets auto-lock timer
- [ ] Verify keyboard input resets auto-lock timer
- [ ] Change timeout in settings and verify new timeout works

### Edge Cases
- [ ] Test with external keyboard, mouse, and trackpad
- [ ] Test passphrase entry with gibberish, wait 5 seconds, then enter correct passphrase
- [ ] Test backspace in passphrase entry
- [ ] Test app behavior when Accessibility permissions revoked
- [ ] Test app startup with no passphrase set

---

## Estimated Effort

| Task | Complexity | Estimated Time |
|------|-----------|----------------|
| 1.1 Menu Bar Delegate | Medium | 4 hours |
| 1.2 Dynamic Menu Items | Low | 1 hour |
| 2.1 Settings Window | High | 8 hours |
| 2.2 Hotkey Capture | Medium | 4 hours |
| 2.3 Dynamic Hotkey Registration | Low | 2 hours |
| 2.4 Configurable Passthrough Key | Low | 2 hours |
| 2.5 Configurable Timeout | Low | 1 hour |
| 3.1 LocalAuthentication FFI | High | 6 hours |
| 3.2 Block Reply Handler | Medium | 3 hours |
| 4.1 Overlay Window | Medium | 4 hours |
| 4.2 UNUserNotification Fallback | Low | 2 hours |
| 5.1 Error Handling | Low | 2 hours |
| 5.2 Universal Binary | Low | 2 hours |
| 5.3 Testing | Medium | 6 hours |
| 6.1 User Documentation | Low | 2 hours |
| 6.2 Developer Documentation | Low | 2 hours |
| **Total** | | **~51 hours** |

**High Priority Subset**: ~17 hours
**Medium Priority Subset**: ~15 hours

---

## Dependencies and Risks

### Dependencies
- `block` crate or manual Objective-C block implementation for Touch ID callbacks
- macOS LocalAuthentication framework (available 10.12.2+, will need fallback for 10.11)
- NSWindow and NSView APIs for overlay (available 10.11+)

### Risks
1. **LocalAuthentication Complexity**: Objective-C blocks in Rust are non-trivial
   - *Mitigation*: Use `block` crate or fall back to current implementation initially

2. **Hotkey Capture Conflicts**: System hotkeys may interfere
   - *Mitigation*: Validate against known system shortcuts, allow user override

3. **Overlay Window Visibility**: May not work in all fullscreen modes
   - *Mitigation*: Test with multiple video conferencing apps, document limitations

4. **Universal Binary Testing**: Requires access to both Intel and Apple Silicon Macs
   - *Mitigation*: Use GitHub Actions or test on loaner devices

---

## Success Criteria

Phase 2 is complete when:
1. ✅ All menu bar actions are functional
2. ✅ Settings UI allows configuration of timeout and hotkeys
3. ✅ Touch ID uses LocalAuthentication framework
4. ✅ Unlock overlay is clearly visible during video calls
5. ✅ All high-priority tests pass
6. ✅ Universal binary builds successfully
7. ✅ Documentation is complete and accurate

---

## Next Steps

1. **Review and Approve**: Review this plan with stakeholders
2. **Prioritize**: Confirm implementation priority based on timeline
3. **Start High Priority**: Begin with task 1.1 (Menu Bar Delegate)
4. **Iterative Testing**: Test each feature as it's completed
5. **Release Planning**: Plan v1.0 release after high-priority tasks complete

---

## Appendix: Additional Nice-to-Have Features (Post-v1.0)

These features were mentioned in the spec but are lower priority:

1. **Selective Input Blocking**: Block only keyboard or only mouse/trackpad
2. **Multiple Passphrases**: Support profiles with different passphrases
3. **Scheduled Auto-Lock**: Auto-lock at specific times of day
4. **Activity Logging**: Log lock/unlock events for audit purposes
5. **Quick Lock Mode**: Temporary lock without changing settings
6. **Custom Lock Screen**: Branded overlay instead of system notification
7. **Integration with Calendar**: Auto-lock during scheduled meetings

---

*Document Version: 1.0*
*Last Updated: 2025-10-22*
*Author: Implementation Review based on specs/handsoff-design.md*
