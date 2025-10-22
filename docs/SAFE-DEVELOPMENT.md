# Safe Development and Testing Guide for HandsOff

## ⚠️ Critical Safety Considerations

HandsOff blocks ALL keyboard and mouse input when locked. **If you get locked out, you cannot use your computer until it's unlocked.** This guide provides strategies to develop and test safely.

---

## Safety Strategies

### Strategy 1: Emergency Unlock Mechanisms (RECOMMENDED)

#### 1.1 Development Mode with Timeout Auto-Unlock
**Add a development flag that auto-unlocks after a short period**

```rust
// In src/app_state.rs
pub struct AppStateInner {
    // ... existing fields
    pub dev_mode: bool,
    pub dev_unlock_timeout: u64, // seconds
}

impl AppState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(AppStateInner {
                // ... existing fields
                dev_mode: std::env::var("HANDSOFF_DEV_MODE").is_ok(),
                dev_unlock_timeout: 10, // Auto-unlock after 10 seconds in dev mode
            })),
        }
    }
}

// In main.rs auto-lock thread
fn start_dev_unlock_thread(state: Arc<AppState>) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(1));

        if state.lock().dev_mode && state.is_locked() {
            let locked_duration = state.lock().lock_start_time
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);

            if locked_duration >= state.lock().dev_unlock_timeout {
                info!("DEV MODE: Auto-unlocking after {} seconds", locked_duration);
                state.set_locked(false);
                ui::menubar::update_menu_bar_icon(false);
            }
        }
    });
}
```

**Usage**:
```bash
# Run in development mode with 10-second auto-unlock
HANDSOFF_DEV_MODE=1 cargo run

# Or set a custom timeout
HANDSOFF_DEV_UNLOCK_SECS=5 cargo run
```

**Pros**:
- Simple to implement
- Guaranteed escape mechanism
- No external dependencies

**Cons**:
- Can't test long lock scenarios
- Must remember to enable flag

---

#### 1.2 SSH Backdoor (HIGHLY RECOMMENDED)

**Enable SSH and keep a terminal open from another machine**

```bash
# On your Mac, enable SSH
sudo systemsetup -setremotelogin on

# From another computer (or phone with SSH client)
ssh you@your-mac.local
pkill handsoff  # Kill the app if locked out
```

**Pros**:
- Works even when fully locked out
- No code changes needed
- Can troubleshoot any issue

**Cons**:
- Requires second device
- Requires SSH setup

---

#### 1.3 Screen Sharing to Second Mac/VM

**Use another Mac or virtual machine to control your dev machine**

```bash
# Enable Screen Sharing on your Mac
sudo launchctl load -w /System/Library/LaunchDaemons/com.apple.screensharing.plist

# Connect from another Mac via Screen Sharing app
# You can then kill the process or unlock
```

**Pros**:
- Full visual control
- Can see what's happening

**Cons**:
- Requires second Mac or VM
- Slower than SSH

---

#### 1.4 Hardware Emergency Key Combination

**Add a secret emergency unlock key combo that always works**

```rust
// In src/input_blocking/mod.rs
const EMERGENCY_UNLOCK_KEYCODE: i64 = 53; // Escape key

fn handle_keyboard_event(
    event: &CGEvent,
    event_type: CGEventType,
    state: &AppState,
) -> bool {
    let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
    let flags = event.get_flags();

    // EMERGENCY UNLOCK: Ctrl+Cmd+Opt+Shift+Esc
    if keycode == EMERGENCY_UNLOCK_KEYCODE &&
        flags.contains(CGEventFlags::CGEventFlagControl) &&
        flags.contains(CGEventFlags::CGEventFlagCommand) &&
        flags.contains(CGEventFlags::CGEventFlagAlternate) &&
        flags.contains(CGEventFlags::CGEventFlagShift)
    {
        warn!("EMERGENCY UNLOCK TRIGGERED");
        state.set_locked(false);
        state.clear_buffer();
        crate::ui::menubar::update_menu_bar_icon(false);
        return false; // Allow the event through to show it worked
    }

    // ... rest of function
}
```

**Pros**:
- Always available
- No external dependencies
- Fast unlock

**Cons**:
- Defeats the purpose of the lock
- Must remember complex combo
- Should be disabled in release builds

---

### Strategy 2: Test in a Sandboxed Environment

#### 2.1 Virtual Machine Testing

**Run HandsOff in a VM so you can reset if locked out**

```bash
# Using UTM, Parallels, or VMware Fusion
# 1. Create macOS VM
# 2. Install Rust and dependencies
# 3. Test HandsOff inside VM
# 4. If locked out, force restart VM from host
```

**Pros**:
- Completely safe
- Can test production builds
- Can snapshot and restore

**Cons**:
- Slower development cycle
- Requires VM setup
- May need macOS license

---

#### 2.2 Secondary User Account

**Create a test user account on your Mac**

```bash
# Create test user via System Settings > Users & Groups
# Or via command line:
sudo dscl . -create /Users/handsofftest
sudo dscl . -create /Users/handsofftest UserShell /bin/bash
sudo dscl . -create /Users/handsofftest RealName "HandsOff Test"
sudo dscl . -create /Users/handsofftest UniqueID 503
sudo dscl . -create /Users/handsofftest PrimaryGroupID 20
sudo dscl . -create /Users/handsofftest NFSHomeDirectory /Users/handsofftest
sudo dscl . -passwd /Users/handsofftest testpassword

# Fast user switching: Enable in System Settings
# Lock desktop and switch users if needed
```

**Pros**:
- No VM overhead
- Easy to switch between accounts
- Can test fresh environment

**Cons**:
- Still need recovery method
- Can't help if you're locked in that account

---

### Strategy 3: Incremental Testing

#### 3.1 Test with Event Logging Only (No Blocking)

**Disable actual blocking, just log what would be blocked**

```rust
// Add to AppStateInner
pub struct AppStateInner {
    // ... existing fields
    pub dry_run: bool, // Log only, don't actually block
}

// In event_tap_callback
unsafe extern "C" fn event_tap_callback(
    _proxy: CGEventTapRef,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef {
    let state = &*(user_info as *const Arc<AppState>);

    if !state.is_locked() {
        state.update_input_time();
        return event;
    }

    // ... existing logic to determine should_block

    if should_block {
        if state.lock().dry_run {
            info!("DRY RUN: Would block event type {}", event_type);
            return event; // Allow through in dry run mode
        }
        std::ptr::null_mut() // Actually block in production
    } else {
        event
    }
}
```

**Usage**:
```bash
HANDSOFF_DRY_RUN=1 cargo run
```

**Pros**:
- Zero risk of lockout
- Can test all logic paths
- Good for development

**Cons**:
- Doesn't test actual blocking
- Different code path than production

---

#### 3.2 Block Only Mouse, Not Keyboard

**Test blocking incrementally**

```rust
// Environment variable to control what gets blocked
let block_keyboard = std::env::var("BLOCK_KEYBOARD").is_ok();
let block_mouse = std::env::var("BLOCK_MOUSE").is_ok();

// In event_tap_callback
match event_type {
    t if t == CGEventType::KeyDown as u32 || t == CGEventType::KeyUp as u32 => {
        if !block_keyboard {
            info!("Keyboard blocking disabled, passing through");
            return event;
        }
        handle_keyboard_event(&cg_event, event_type_enum, state)
    }
    t if t == CGEventType::MouseMoved as u32 || ... => {
        if !block_mouse {
            info!("Mouse blocking disabled, passing through");
            return event;
        }
        handle_mouse_event(event_type_enum, state)
    }
}
```

**Usage**:
```bash
# Test mouse blocking only (keyboard still works!)
BLOCK_MOUSE=1 cargo run

# Test both (full production mode)
BLOCK_KEYBOARD=1 BLOCK_MOUSE=1 cargo run
```

**Pros**:
- Can test mouse blocking safely
- Gradual risk increase
- Keyboard always available for unlock

**Cons**:
- Still need recovery for full testing

---

### Strategy 4: Watchdog Timer

#### 4.1 External Watchdog Process

**Create a separate process that kills HandsOff if it doesn't receive heartbeat**

```rust
// watchdog.rs (separate binary)
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};

fn main() {
    let last_heartbeat = Arc::new(Mutex::new(Instant::now()));

    // Start heartbeat listener (TCP/Unix socket)
    let heartbeat_clone = last_heartbeat.clone();
    thread::spawn(move || {
        // Listen for heartbeats from HandsOff
        // Update last_heartbeat when received
    });

    // Watchdog loop
    loop {
        thread::sleep(Duration::from_secs(5));

        let elapsed = last_heartbeat.lock().unwrap().elapsed();
        if elapsed > Duration::from_secs(30) {
            eprintln!("HandsOff not responding, killing process");
            Command::new("pkill").arg("handsoff").output().ok();
            break;
        }
    }
}
```

```rust
// In main.rs - send heartbeats
fn start_watchdog_heartbeat() {
    thread::spawn(|| loop {
        thread::sleep(Duration::from_secs(5));
        // Send heartbeat to watchdog process
        // (TCP connection, Unix socket, or shared file)
    });
}
```

**Usage**:
```bash
# Terminal 1: Start watchdog
cargo run --bin watchdog

# Terminal 2: Start HandsOff
cargo run
```

**Pros**:
- Automatic recovery
- External to app (can't be blocked)
- Customizable timeout

**Cons**:
- More complex setup
- Need to manage two processes

---

## Recommended Development Workflow

### Phase 1: Safe Development (Week 1-2)
1. **Enable Development Mode** with 10-second auto-unlock
2. **Set up SSH** from another device (phone, laptop)
3. **Test in dry-run mode** first
4. **Add emergency unlock combo** (Ctrl+Cmd+Opt+Shift+Esc)

```bash
# Always run with safety flags during development
HANDSOFF_DEV_MODE=1 HANDSOFF_DRY_RUN=1 cargo run
```

### Phase 2: Incremental Risk (Week 3)
1. **Test mouse blocking only** (keyboard passthrough)
2. **Test keyboard blocking only** (mouse passthrough)
3. **Test full blocking** with SSH session ready

```bash
# Test mouse first
HANDSOFF_DEV_MODE=1 BLOCK_MOUSE=1 cargo run

# Then keyboard
HANDSOFF_DEV_MODE=1 BLOCK_KEYBOARD=1 cargo run

# Then both with 10s auto-unlock
HANDSOFF_DEV_MODE=1 BLOCK_KEYBOARD=1 BLOCK_MOUSE=1 cargo run
```

### Phase 3: Production Testing (Week 4)
1. **Test in VM** or secondary account
2. **Test with watchdog** process
3. **Test without dev mode** but with SSH ready

### Phase 4: Release
1. **Disable all development flags** in release builds
2. **Keep emergency unlock** as documented feature
3. **Add warning in README** about lockout risks

---

## Unit Testing Strategy

### What CAN Be Unit Tested

#### 1. Passphrase Hashing and Verification ✅
```rust
// tests/auth_tests.rs
#[cfg(test)]
mod tests {
    use handsoff::auth;

    #[test]
    fn test_hash_passphrase() {
        let passphrase = "test123";
        let hash = auth::hash_passphrase(passphrase);
        assert_eq!(hash.len(), 64); // SHA-256 hex is 64 chars
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_verify_passphrase_correct() {
        let passphrase = "correct_password";
        let hash = auth::hash_passphrase(passphrase);
        assert!(auth::verify_passphrase(passphrase, &hash));
    }

    #[test]
    fn test_verify_passphrase_incorrect() {
        let passphrase = "correct_password";
        let hash = auth::hash_passphrase(passphrase);
        assert!(!auth::verify_passphrase("wrong_password", &hash));
    }

    #[test]
    fn test_hash_deterministic() {
        let passphrase = "same_input";
        let hash1 = auth::hash_passphrase(passphrase);
        let hash2 = auth::hash_passphrase(passphrase);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_different_inputs() {
        let hash1 = auth::hash_passphrase("input1");
        let hash2 = auth::hash_passphrase("input2");
        assert_ne!(hash1, hash2);
    }
}
```

#### 2. Keycode to Character Conversion ✅
```rust
// tests/keycode_tests.rs
#[cfg(test)]
mod tests {
    use handsoff::utils::keycode::keycode_to_char;

    #[test]
    fn test_letter_keys_no_shift() {
        assert_eq!(keycode_to_char(0, false), Some('a'));
        assert_eq!(keycode_to_char(1, false), Some('s'));
        assert_eq!(keycode_to_char(2, false), Some('d'));
    }

    #[test]
    fn test_letter_keys_with_shift() {
        assert_eq!(keycode_to_char(0, true), Some('A'));
        assert_eq!(keycode_to_char(1, true), Some('S'));
        assert_eq!(keycode_to_char(2, true), Some('D'));
    }

    #[test]
    fn test_number_keys_no_shift() {
        assert_eq!(keycode_to_char(18, false), Some('1'));
        assert_eq!(keycode_to_char(19, false), Some('2'));
        assert_eq!(keycode_to_char(20, false), Some('3'));
    }

    #[test]
    fn test_number_keys_with_shift() {
        assert_eq!(keycode_to_char(18, true), Some('!'));
        assert_eq!(keycode_to_char(19, true), Some('@'));
        assert_eq!(keycode_to_char(20, true), Some('#'));
    }

    #[test]
    fn test_special_keys() {
        assert_eq!(keycode_to_char(49, false), Some(' ')); // Space
        assert_eq!(keycode_to_char(51, false), None); // Delete (no char)
    }

    #[test]
    fn test_invalid_keycode() {
        assert_eq!(keycode_to_char(9999, false), None);
    }
}
```

#### 3. AppState Logic ✅
```rust
// tests/app_state_tests.rs
#[cfg(test)]
mod tests {
    use handsoff::app_state::AppState;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_initial_state() {
        let state = AppState::new();
        assert!(!state.is_locked());
        assert_eq!(state.get_buffer(), "");
        assert!(state.get_passphrase_hash().is_none());
    }

    #[test]
    fn test_lock_unlock() {
        let state = AppState::new();
        state.set_locked(true);
        assert!(state.is_locked());
        state.set_locked(false);
        assert!(!state.is_locked());
    }

    #[test]
    fn test_buffer_operations() {
        let state = AppState::new();
        state.append_to_buffer('a');
        state.append_to_buffer('b');
        state.append_to_buffer('c');
        assert_eq!(state.get_buffer(), "abc");
        state.clear_buffer();
        assert_eq!(state.get_buffer(), "");
    }

    #[test]
    fn test_passphrase_hash() {
        let state = AppState::new();
        let hash = "abc123def456".to_string();
        state.set_passphrase_hash(hash.clone());
        assert_eq!(state.get_passphrase_hash(), Some(hash));
    }

    #[test]
    fn test_buffer_reset_timing() {
        let state = AppState::new();
        state.lock().buffer_reset_timeout = 1; // 1 second for testing

        state.append_to_buffer('x');
        state.update_key_time();

        assert!(!state.should_reset_buffer());

        thread::sleep(Duration::from_secs(2));
        assert!(state.should_reset_buffer());
    }

    #[test]
    fn test_auto_lock_timing() {
        let state = AppState::new();
        state.lock().auto_lock_timeout = 1; // 1 second for testing

        assert!(!state.should_auto_lock()); // Starts unlocked

        thread::sleep(Duration::from_secs(2));
        assert!(state.should_auto_lock());

        state.update_input_time();
        assert!(!state.should_auto_lock()); // Reset
    }

    #[test]
    fn test_talk_key_state() {
        let state = AppState::new();
        assert!(!state.is_talk_key_pressed());

        state.set_talk_key_pressed(true);
        assert!(state.is_talk_key_pressed());

        state.set_talk_key_pressed(false);
        assert!(!state.is_talk_key_pressed());
    }

    #[test]
    fn test_thread_safety() {
        let state = AppState::new();
        let state_clone = state.clone();

        let handle = thread::spawn(move || {
            for _ in 0..100 {
                state_clone.append_to_buffer('a');
            }
        });

        for _ in 0..100 {
            state.append_to_buffer('b');
        }

        handle.join().unwrap();
        assert_eq!(state.get_buffer().len(), 200);
    }
}
```

#### 4. Keychain Integration ✅
```rust
// tests/keychain_tests.rs
#[cfg(test)]
mod tests {
    use handsoff::auth::keychain;

    #[test]
    fn test_store_and_retrieve_passphrase() {
        let hash = "test_hash_12345";
        keychain::store_passphrase_hash(hash).unwrap();
        let retrieved = keychain::retrieve_passphrase_hash().unwrap();
        assert_eq!(retrieved, Some(hash.to_string()));
    }

    #[test]
    fn test_retrieve_nonexistent() {
        // Delete any existing entry first
        let _ = keychain::store_passphrase_hash("");
        // Implementation should handle missing entry gracefully
    }

    #[test]
    fn test_update_passphrase() {
        let hash1 = "first_hash";
        let hash2 = "second_hash";

        keychain::store_passphrase_hash(hash1).unwrap();
        let retrieved1 = keychain::retrieve_passphrase_hash().unwrap();
        assert_eq!(retrieved1, Some(hash1.to_string()));

        keychain::store_passphrase_hash(hash2).unwrap();
        let retrieved2 = keychain::retrieve_passphrase_hash().unwrap();
        assert_eq!(retrieved2, Some(hash2.to_string()));
    }

    #[test]
    fn test_auto_lock_timeout_storage() {
        keychain::store_auto_lock_timeout(300).unwrap();
        let retrieved = keychain::retrieve_auto_lock_timeout().unwrap();
        assert_eq!(retrieved, Some(300));
    }
}
```

#### 5. Hotkey Configuration Parsing ✅
```rust
// tests/hotkey_tests.rs
#[cfg(test)]
mod tests {
    use handsoff::input_blocking::hotkeys::HotkeyManager;
    use global_hotkey::hotkey::{Code, Modifiers};

    #[test]
    fn test_hotkey_manager_creation() {
        let manager = HotkeyManager::new();
        assert!(manager.is_ok());
    }

    #[test]
    fn test_default_hotkeys() {
        let mut manager = HotkeyManager::new().unwrap();
        assert!(manager.register_lock_hotkey().is_ok());
        assert!(manager.register_talk_hotkey().is_ok());
    }

    // Note: Can't test actual hotkey triggering in unit tests
    // (requires system-level input simulation)
}
```

---

### What CANNOT Be Unit Tested (Requires Integration/Manual Testing)

#### ❌ Event Tap Blocking
- Requires actual system events
- Needs Accessibility permissions
- Can only test manually or in integration environment

#### ❌ Menu Bar Interaction
- Requires NSApp run loop
- Needs actual UI rendering
- Must test manually

#### ❌ Touch ID Authentication
- Requires hardware (Touch ID sensor)
- Needs user fingerprint enrollment
- System framework with side effects

#### ❌ Notification Display
- Requires notification center
- Visual verification needed
- System-level UI

#### ❌ Auto-Lock Behavior
- Requires real-time system events
- Long timeouts hard to test
- Best tested manually with short timeouts

---

## Integration Testing Approach

```rust
// tests/integration_test.rs
#[test]
#[ignore] // Run manually with: cargo test -- --ignored
fn integration_test_with_safety() {
    // Set up safe test environment
    std::env::set_var("HANDSOFF_DEV_MODE", "1");
    std::env::set_var("HANDSOFF_DRY_RUN", "1");

    // Start app in background thread
    let handle = std::thread::spawn(|| {
        handsoff::main().unwrap();
    });

    // Wait for startup
    std::thread::sleep(Duration::from_secs(2));

    // Run tests...

    // Cleanup
    // (watchdog will kill after timeout)
}
```

---

## Emergency Recovery Procedures

### If You Get Locked Out

#### Option 1: Wait for Auto-Unlock (Dev Mode)
- Dev mode auto-unlocks after 10 seconds
- Just wait it out

#### Option 2: SSH Kill
```bash
# From another computer
ssh you@your-mac.local
pkill handsoff
```

#### Option 3: Force Restart
- Hold power button for 10 seconds
- Mac will force restart
- Last resort only

#### Option 4: Emergency Unlock Combo
- Press Ctrl+Cmd+Opt+Shift+Esc simultaneously
- Only works if implemented

---

## Checklist Before Each Development Session

- [ ] SSH enabled and tested from another device
- [ ] Dev mode enabled (`HANDSOFF_DEV_MODE=1`)
- [ ] Know the passphrase (write it down!)
- [ ] Emergency unlock combo memorized
- [ ] Another terminal/computer ready to kill process
- [ ] Changes committed to git (in case of force restart)
- [ ] Testing plan written down (know what to test)
- [ ] Time-limited session (stop before getting tired)

---

## Production Safety Features

For release builds, include these safety features:

1. **First-run tutorial** explaining lockout risks
2. **Confirm passphrase dialog** (type twice)
3. **Passphrase hint storage** (optional, in keychain)
4. **Emergency unlock option** (documented, disabled by default)
5. **Auto-unlock after 24 hours** (configurable, disabled by default)
6. **Warning before enabling** (checkbox: "I understand the risks")

---

*Remember: An ounce of prevention is worth a pound of force-restarting your Mac!*
