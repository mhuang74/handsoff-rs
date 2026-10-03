# Safe Development and Testing Guide for HandsOff

## ⚠️ Critical Safety Considerations

HandsOff blocks ALL keyboard and mouse input when locked. **If you get locked out, you cannot use your computer until it's unlocked.** This guide provides strategies to develop and test safely.

---

## Safety Strategies

### Strategy 1: Emergency Unlock Mechanisms (RECOMMENDED)

#### 1.1 Short Auto-Unlock Window (RECOMMENDED FIRST LINE OF DEFENSE)
**Run with a 60-second auto-unlock base interval — no code changes needed**

The auto-unlock backoff schedule is enabled by default. Setting `HANDS_OFF_AUTO_UNLOCK=60` opens the first window after 60 s of awake time (60 is the minimum; `0` disables, values outside 60–86400 are rejected with a warning):

```bash
# First window opens after 60 s of awake time; input is released silently
HANDS_OFF_AUTO_UNLOCK=60 cargo run --bin handsoff-tray
```

Note the backoff rule: after a window fires, the next window doubles (120 s, 240 s, …). Re-locking does NOT reset the schedule — only a successful passphrase unlock does. For repeated quick testing, kill and restart the process between runs, or rely on SSH (1.2).

**Pros**:
- No code changes, works in any build
- Guaranteed escape mechanism

**Cons**:
- Backoff doubles after each fired window (restart the process for repeated tests)
- Must remember to set the env var

---

#### 1.2 SSH Backdoor (HIGHLY RECOMMENDED)

**Enable SSH and keep a terminal open from another machine**

```bash
# On your Mac, enable SSH
sudo systemsetup -setremotelogin on

# From another computer (or phone with SSH client)
ssh you@your-mac.local
pkill handsoff-tray  # Kill the app if locked out
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
// Hypothetical sketch (NOT implemented — there is no emergency unlock combo
// in the current code); shown to convey the idea only.
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
        // (hypothetical — illustrative only; src/ui/menubar.rs no longer exists;
        // the tray app updates its own icon in src/bin/handsoff-tray.rs)
        // crate::ui::menubar::update_menu_bar_icon(false);
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

#### 3.1 Lock, Verify Blocking, Unlock (Full Production Path)

There is no dry-run or partial-blocking mode in the code — blocking is all-or-nothing once locked. The safe incremental path is timing, not code:

```bash
# Shortest auto-unlock base interval (60 s awake-time)
HANDS_OFF_AUTO_UNLOCK=60 cargo run --bin handsoff-tray
```

1. Lock via hotkey (Ctrl+Cmd+Shift+L)
2. Verify keyboard AND mouse are blocked (they block together)
3. Type your passphrase to unlock
4. Repeat as needed

**Pros**:
- Exercises the exact production code path
- Zero code changes

**Cons**:
- Real blocking on the first run — have SSH ready (1.2)

---

#### 3.2 Bound the Blast Radius with Short Auto-Lock

If a window is accidentally left unlocked, auto-lock re-engages after idle time. Use the 20 s minimum instead of the 180 s default:

```bash
HANDS_OFF_AUTO_LOCK=20 cargo run --bin handsoff-tray
```

**Pros**:
- Unattended machine re-locks quickly
- Composable with the auto-unlock override

**Cons**:
- Frequent re-locking can interrupt testing

---

### Strategy 4: Watchdog Timer

#### 4.1 External Watchdog Process

**Create a separate process that kills HandsOff if it doesn't receive heartbeat**

```rust
// Hypothetical sketch (NOT implemented — no watchdog binary exists); shown
// to convey the idea only.
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
            Command::new("pkill").arg("handsoff-tray").output().ok();
            break;
        }
    }
}
```

```rust
// Illustrative sketch (no src/main.rs anymore — runtime lives in
// src/lib.rs + src/bin/*.rs); shown to convey the watchdog heartbeat idea.
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
cargo run --bin handsoff-tray
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
1. **Set up SSH** from another device (phone, laptop) — your escape route
2. **Run with a short auto-unlock window** (`HANDS_OFF_AUTO_UNLOCK=60`)
3. **Run `cargo test`** (inline `#[cfg(test)]` modules; no input blocked)

```bash
# Always run with a safety window during development
HANDS_OFF_AUTO_UNLOCK=60 cargo run --bin handsoff-tray
```

### Phase 2: Incremental Risk (Week 3)
1. **Test the lock → block → unlock cycle** with SSH ready
2. **Bound the blast radius** with the shortest auto-lock (`HANDS_OFF_AUTO_LOCK=20`)
3. **Test auto-unlock window firing** (wait out the 60 s awake-time)

```bash
HANDS_OFF_AUTO_UNLOCK=60 HANDS_OFF_AUTO_LOCK=20 cargo run --bin handsoff-tray
```

### Phase 3: Production Testing (Week 4)
1. **Test in VM** or secondary account
2. **Test with watchdog** process
3. **Test with defaults** (180 s auto-lock, 60 min auto-unlock base) but with SSH ready

### Phase 4: Release
1. **Test with default timing** (no env overrides) before tagging a release
2. **Keep the auto-unlock failsafe enabled** in release builds (it is by default)
3. **Add warning in README** about lockout risks

---

## Unit Testing Strategy

### What CAN Be Unit Tested

#### 1. Passphrase Hashing and Verification ✅
```rust
// Inline #[cfg(test)] module in src/utils/mod.rs (actual tests exist there)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_keycodes_deterministic() {
        let keycodes: Vec<u32> = vec![0, 1, 2, 3];
        let h1 = hash_keycodes(&keycodes);
        let h2 = hash_keycodes(&keycodes);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex is 64 chars
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_hash_keycodes_order_sensitive() {
        // Keycode sequences are ordered: [0,1] and [1,0] hash differently
        assert_ne!(hash_keycodes(&[0, 1]), hash_keycodes(&[1, 0]));
    }
}
```

(See `src/utils/mod.rs` for the real test list — hashing is over the keycode sequence, `keycode-v1` format, not over plaintext characters.)

#### 2. Keycode to Character Conversion
**Removed** — the character-decode map (`keycode_to_char`) was deleted when setup switched to double-capture confirm (passphrases are never displayed). Passphrase membership is keycode-set based (`is_rejected_keycode` for capture, `is_unlock_blocked_keycode` for unlock); no char decoding exists anywhere.

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

#### 4. Config File Persistence ✅
```rust
// Inline #[cfg(test)] module in src/config_file.rs (actual tests exist there)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_load_roundtrip() {
        let config = Config::new(&[0, 1, 2, 3], 120, true, 3600, None, None).unwrap();
        let path = std::env::temp_dir().join("handsoff-test-config.toml");
        config.save_to_path(&path).unwrap();

        let loaded = Config::load_from_path(&path).unwrap();
        assert_eq!(loaded.passphrase_hash, config.passphrase_hash);
        assert_eq!(loaded.auto_lock_timeout, 120);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_legacy_config_rejected() {
        // A config with a non-keycode-v1 passphrase_format must fail to load
        // and force re-setup (never silently trust an unverifiable passphrase)
    }
}
```

(The real module covers round-trip, permission bits, legacy-format rejection, and base-interval range checks — see `src/config_file.rs`.)

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
// Integration tests that spawn the real app must run on macOS with
// Accessibility permissions granted, and should always give the process an
// auto-unlock escape route:
#[test]
#[ignore] // Run manually with: cargo test -- --ignored
fn integration_test_with_safety() {
    // Shortest auto-unlock base interval as the escape route
    std::env::set_var("HANDS_OFF_AUTO_UNLOCK", "60");

    // Start app in background thread (starts locked when launched in locked
    // mode — for the tray app, trigger Lock via hotkey or menu)
    let handle = std::thread::spawn(|| {
        // handsoff runtime start
    });

    // Wait for startup, exercise lock/unlock...

    // Cleanup
    // (auto-unlock window opens after 60 s awake-time as the failsafe)
}
```

---

## Emergency Recovery Procedures

### If You Get Locked Out

#### Option 1: Wait for Auto-Unlock (if enabled)
- Run with `HANDS_OFF_AUTO_UNLOCK=60`: input is released after 60 s of awake time (silently, no notification)

#### Option 2: SSH Kill
```bash
# From another computer
ssh you@your-mac.local
pkill handsoff-tray
```

#### Option 3: Force Restart
- Hold power button for 10 seconds
- Mac will force restart
- Last resort only (the app relaunches unlocked — accepted bypass, §2.5)

---

## Checklist Before Each Development Session

- [ ] SSH enabled and tested from another device
- [ ] Auto-unlock window set short (`HANDS_OFF_AUTO_UNLOCK=60`)
- [ ] Know the passphrase (write it down!)
- [ ] Another terminal/computer ready to kill process
- [ ] Changes committed to git (in case of force restart)
- [ ] Testing plan written down (know what to test)
- [ ] Time-limited session (stop before getting tired)

---

## Production Safety Features

For release builds, include these safety features:

1. **First-run tutorial** explaining lockout risks
2. **Confirm passphrase dialog** (setup already requires typing the sequence twice)
3. **Auto-unlock backoff schedule** (enabled by default: first window at the base interval, doubling up to 24 h; only a successful passphrase unlock resets it)
4. **Warning before enabling** (checkbox: "I understand the risks")

---

*Remember: An ounce of prevention is worth a pound of force-restarting your Mac!*
