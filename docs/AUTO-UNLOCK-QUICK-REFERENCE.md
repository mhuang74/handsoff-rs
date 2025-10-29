# Auto-Unlock Quick Reference Guide

**Quick lookup guide for the HandsOff auto-unlock safety feature**

---

## TL;DR

```bash
# Enable with 30-second timeout (testing)
HANDS_OFF_AUTO_UNLOCK=30 cargo run

# Enable with 5-minute timeout (development)
HANDS_OFF_AUTO_UNLOCK=300 ./handsoff

# Disabled (default)
./handsoff
```

---

## Configuration

| Setting | Value | Notes |
|---------|-------|-------|
| **Environment Variable** | `HANDS_OFF_AUTO_UNLOCK` | Must be set before app starts |
| **Minimum Timeout** | 60 seconds | Below this = disabled with warning |
| **Maximum Timeout** | 900 seconds (15 minutes) | Above this = disabled with warning |
| **Default** | Disabled (unset or 0) | Feature is opt-in |
| **Recommended for Testing** | 30-60 seconds | Quick iteration |
| **Recommended for Development** | 300-600 seconds (5-10 min) | Safety net |
| **Production** | DO NOT USE | Security risk |

---

## Behavior

| Action | Result |
|--------|--------|
| **Lock device** | Timer starts counting |
| **Timeout expires** | Auto-unlock triggers |
| **Manual unlock before timeout** | Timer resets, no auto-unlock |
| **Lock again** | Timer starts fresh |
| **Invalid config value** | Feature disabled with warning |

---

## Log Messages

### Startup (INFO)
```
Auto-unlock safety feature enabled: 30 seconds
Auto-unlock monitoring thread started
```

### Lock/Unlock (DEBUG)
```
Lock engaged at Instant { ... }
Lock disengaged
```

### Auto-Unlock Triggered (WARN)
```
Auto-unlock timeout expired - disabling input interception
AUTO-UNLOCK TRIGGERED after 30 seconds
```

### Notification (INFO)
```
Auto-unlock notification delivered
```

### Invalid Config (WARN)
```
Invalid auto-unlock timeout: 5 (must be 60-900 or 0). Feature disabled.
Failed to parse HANDS_OFF_AUTO_UNLOCK: invalid digit found in string. Feature disabled.
```

---

## Common Commands

```bash
# Check if env var is set
echo $HANDS_OFF_AUTO_UNLOCK

# Run with logging
RUST_LOG=info HANDS_OFF_AUTO_UNLOCK=30 cargo run

# Run with debug logging
RUST_LOG=debug HANDS_OFF_AUTO_UNLOCK=30 cargo run

# Test valid values
HANDS_OFF_AUTO_UNLOCK=10 cargo run    # Minimum
HANDS_OFF_AUTO_UNLOCK=3600 cargo run  # Maximum

# Test invalid values (should warn and disable)
HANDS_OFF_AUTO_UNLOCK=5 cargo run     # Too low
HANDS_OFF_AUTO_UNLOCK=5000 cargo run  # Too high
HANDS_OFF_AUTO_UNLOCK=abc cargo run   # Invalid

# Explicitly disable
HANDS_OFF_AUTO_UNLOCK=0 cargo run

# Run unit tests
cargo test auto_unlock
cargo test -- --nocapture  # With output
```

---

## Notification

When auto-unlock triggers, you'll see:

**Title:** HandsOff Auto-Unlock Activated

**Message:** Input interception disabled by safety timeout. You can use your computer normally.

**Sound:** System default notification sound

---

## Timing

| Configured Timeout | Actual Unlock Time |
|-------------------|-------------------|
| 30 seconds | 30-40 seconds |
| 60 seconds | 60-70 seconds |
| 300 seconds | 300-310 seconds |

**Why?** The monitoring thread sleeps 10 seconds between checks for efficiency.

---

## File Locations

### Implementation
- `src/app_state.rs` - State management (lines 28-31, 60-73, 129-169)
- `src/main.rs` - Parsing and thread (lines 22-48, 188-211)
- `src/ui/notifications.rs` - Notification (lines 74-111)

### Tests
- `src/app_state.rs` - 9 unit tests (lines 178-361)
- `src/main.rs` - 4 unit tests (lines 213-305)

### Documentation
- `README.md` - User documentation
- `specs/auto-unlock-safety-feature.md` - Detailed specification
- `TESTING-AUTO-UNLOCK.md` - Manual testing guide
- `docs/AUTO-UNLOCK-QUICK-REFERENCE.md` - This file

---

## Troubleshooting

| Problem | Solution |
|---------|----------|
| **Feature not enabling** | Check env var: `echo $HANDS_OFF_AUTO_UNLOCK` |
| **No log messages** | Run with logging: `RUST_LOG=info ./handsoff` |
| **Auto-unlock not triggering** | Verify device is locked (menu bar icon shows 🔒) |
| **Notification not showing** | Check System Settings > Notifications > HandsOff |
| **Timer seems wrong** | Normal - triggers within 10s of timeout |

---

## Security Checklist

✅ **Safe to use:**
- During development
- For personal testing
- On your own device
- With timeouts ≥ 5 minutes

❌ **DO NOT use:**
- In production environments
- On shared/public computers
- With timeouts < 60 seconds (for real use)
- When security is critical

---

## Code Snippets

### Check if auto-unlock is configured
```rust
let state = AppState::new();
state.set_auto_unlock_timeout(Some(30));

if state.should_auto_unlock() {
    // Timeout has expired
}
```

### Parse environment variable
```rust
let timeout = parse_auto_unlock_timeout();
match timeout {
    Some(seconds) => info!("Enabled: {} seconds", seconds),
    None => info!("Disabled"),
}
```

### Trigger auto-unlock manually
```rust
state.trigger_auto_unlock();
```

---

## Test Scenarios

### Quick Smoke Test
```bash
# 1. Enable with 15-second timeout
HANDS_OFF_AUTO_UNLOCK=15 cargo run

# 2. Lock device (Ctrl+Cmd+Shift+L)
# 3. Wait 15-25 seconds
# 4. Verify notification appears
# 5. Verify input is unlocked
```

### Full Test Suite
```bash
# Run automated tests
cargo test

# Run manual tests
# See TESTING-AUTO-UNLOCK.md for 17 test scenarios
```

---

## Launch Agent Configuration

To make the configuration permanent:

```xml
<!-- ~/Library/LaunchAgents/com.handsoff.plist -->
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.handsoff</string>

    <key>ProgramArguments</key>
    <array>
        <string>/Applications/HandsOff.app/Contents/MacOS/handsoff</string>
    </array>

    <key>EnvironmentVariables</key>
    <dict>
        <key>HANDS_OFF_AUTO_UNLOCK</key>
        <string>300</string>  <!-- 5 minutes -->
    </dict>

    <key>RunAtLoad</key>
    <true/>

    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
```

Load with:
```bash
launchctl load ~/Library/LaunchAgents/com.handsoff.plist
```

---

## FAQ

**Q: Why isn't the timeout exactly as configured?**
A: The monitoring thread sleeps 10 seconds between checks. Auto-unlock will trigger within 0-10 seconds after the configured timeout.

**Q: Can I change the timeout while the app is running?**
A: No, you must restart the app with the new environment variable value.

**Q: Does auto-unlock work if my Mac goes to sleep?**
A: Yes, the timer continues counting based on elapsed time, not CPU time.

**Q: What happens if I lock/unlock/lock quickly?**
A: Each lock starts the timer fresh. The timeout only applies to the current lock session.

**Q: Is this secure?**
A: It's a **safety feature**, not a security feature. Use it for development/testing only.

**Q: Can I disable it after it's been configured?**
A: Yes, unset the environment variable or set it to 0, then restart the app.

---

## Related Documentation

- **Full Specification:** `specs/auto-unlock-safety-feature.md`
- **User Guide:** `README.md` (Auto-Unlock Safety Feature section)
- **Manual Testing:** `TESTING-AUTO-UNLOCK.md`
- **Code Documentation:** See inline comments in source files

---

**Last Updated:** October 28, 2025
**Version:** 1.0
**Status:** Production Ready
