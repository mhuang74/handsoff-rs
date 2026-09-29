# Auto-Unlock Quick Reference Guide

**Quick lookup guide for the HandsOff auto-unlock safety feature**

---

## TL;DR

```bash
# Shortest useful base interval (testing) — first window after 60 s awake-time
HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked

# 5-minute base interval (development)
HANDS_OFF_AUTO_UNLOCK=300 ./handsoff

# Explicitly disabled
HANDS_OFF_AUTO_UNLOCK=0 ./handsoff

# Unset (default): enabled with base interval 3600 s (60 min) from the config schema
./handsoff
```

---

## Configuration

Auto-unlock is an **exponential backoff schedule** (per `specs/deep-design-review-v2-2026-09.md` §2), configured via the `HANDS_OFF_AUTO_UNLOCK` env var, `config.toml` (`auto_unlock_mode` + `auto_unlock_base_interval`), or defaults:

| Setting | Value | Notes |
|---------|-------|-------|
| **Environment Variable** | `HANDS_OFF_AUTO_UNLOCK` | Base interval override, in seconds |
| **Minimum Base Interval** | 60 seconds | Below this = warning, env var ignored |
| **Maximum Base Interval** | 86400 seconds (24 h) | Also the interval ceiling |
| **Disabled** | `0` | Explicitly disables the failsafe |
| **Default** | Enabled, base 3600 s (60 min) | Enabled by default (V2), not opt-in |
| **Recommended for Testing** | 60 seconds | Minimum; quick iteration |
| **Recommended for Development** | 300–3600 seconds | Safety net |

In `config.toml`, auto-unlock is stored as a **mode** (`auto_unlock_mode: "backoff"` or `"disabled"`) plus a persisted base interval (`auto_unlock_base_interval`), not a scalar timeout (§2.7).

---

## Behavior

| Action | Result |
|--------|--------|
| **Lock device (new locked stretch)** | Schedule anchored at the base interval |
| **Base interval of awake-time elapses** | Window opens: input released silently, counter advances (next window doubles) |
| **Passphrase unlock** | Schedule resets to the base interval (the ONLY reset besides tray Reset) |
| **Auto-lock re-engagement** | Does NOT reset the schedule (§2.3 linchpin) |
| **Window fires** | Does NOT reset the schedule; next window doubles |
| **Invalid config value** | Env var ignored with warning; config file / default applies (never silently disabled) |
| **System sleep** | Schedule counts awake-time only (`Instant` pauses, §2.4) |

---

## Log Messages

### Startup (INFO)
```
Auto-unlock backoff enabled: first window at 3600s, doubling up to 86400s
Auto-unlock backoff monitoring thread started
```

### Locked Stretch (DEBUG)
```
Locked stretch started; auto-unlock window 0 opens after 3600s
```

### Window Fired (WARN + INFO)
```
Auto-unlock window opened - releasing input (unauthenticated)
AUTO-UNLOCK WINDOW FIRED after Ns awake-time
Auto-unlock backoff advanced: next window opens after 7200s
Input unlocked due to auto-unlock window
```

### Explicit Disable (INFO)
```
Auto-unlock disabled via HANDS_OFF_AUTO_UNLOCK=0
```

### Invalid Config (WARN)
```
Invalid auto-unlock base interval: 5 (must be 60-86400 or 0). Ignoring environment variable.
Failed to parse HANDS_OFF_AUTO_UNLOCK: invalid digit found in string. Ignoring environment variable.
```

---

## Common Commands

```bash
# Check if env var is set
echo $HANDS_OFF_AUTO_UNLOCK

# Run with logging
RUST_LOG=info HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked

# Run with debug logging
RUST_LOG=debug HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked

# Test boundary values
HANDS_OFF_AUTO_UNLOCK=60 cargo run      # Minimum base interval
HANDS_OFF_AUTO_UNLOCK=86400 cargo run   # Maximum

# Test invalid values (should warn and fall back to config/default)
HANDS_OFF_AUTO_UNLOCK=30 cargo run      # Below minimum
HANDS_OFF_AUTO_UNLOCK=90000 cargo run   # Above maximum
HANDS_OFF_AUTO_UNLOCK=abc cargo run     # Unparseable

# Explicitly disable
HANDS_OFF_AUTO_UNLOCK=0 cargo run

# Run unit tests
cargo test auto_unlock
cargo test -- --nocapture  # With output
```

---

## Notification

**None.** Auto-unlock is **silent by design** (V10): no notification is posted when a window opens. Check the menu bar icon and logs instead.

---

## Timing

Windows open at **cumulative awake times**: window 0 opens `base` after lock; window N opens `interval(N)` after window N−1 **opened** — independent of when auto-lock re-engages between windows (a re-lock never moves the schedule). The monitoring thread polls every 10 s, so a window fires within 0–10 s after it opens.

The table lists the **gaps between consecutive windows**, not elapsed times. With base 3600 s the cumulative open times are t = 60 min, 180 min (3 h), 420 min (7 h), 900 min (15 h)…

| Base Interval | Gap to Window 1 | Gap to Window 2 | Gap to Window 3 | Gap to Window 4+ |
|---------------|----------|----------|----------|-----------|
| 60 s (min) | 60 s | 120 s | 240 s | doubles… |
| 3600 s (default) | 60 min | 2 h | 4 h | doubles… |
| 86400 s (max) | 24 h | 24 h (at ceiling) | 24 h | 24 h |

---

## File Locations

### Implementation
- `src/config.rs` — `AutoUnlockConfig` enum (`Disabled` / `Backoff { base_interval_secs }`), env-var parsing (`parse_auto_unlock_config`), resolution precedence (`resolve_auto_unlock`)
- `src/app_state.rs` — `AutoUnlockState` (`base_interval_secs`, `stretch_start`, `window_index`), `set_auto_unlock_config`, `should_auto_unlock`, `trigger_auto_unlock`, `complete_passphrase_unlock`
- `src/lib.rs` — auto-unlock monitoring thread (10 s poll) and startup log line
- `src/constants.rs` — `AUTO_UNLOCK_BASE_SECONDS`, `AUTO_UNLOCK_CEILING_SECONDS`, `AUTO_UNLOCK_CHECK_INTERVAL_SECS`

### Tests
- `src/app_state.rs` — interval math, reset rules, window consumption, disable-clears-schedule
- `src/config.rs` — env-var parsing, resolution precedence

### Documentation
- `README.md` — user documentation
- `DEVELOPER.md` — "Auto-Unlock Safety Feature" section
- `specs/auto-unlock-safety-feature.md` — detailed specification
- `docs/TESTING-AUTO-UNLOCK.md` — manual testing guide
- `docs/AUTO-UNLOCK-QUICK-REFERENCE.md` — this file

---

## Troubleshooting

| Problem | Solution |
|---------|----------|
| **Feature disabled unexpectedly** | Check env var (`echo $HANDS_OFF_AUTO_UNLOCK`) and config `auto_unlock_mode` |
| **No log messages** | Run with logging: `RUST_LOG=info ./handsoff` |
| **Auto-unlock not triggering** | Verify device is locked (menu bar icon shows 🔒); check awake-time vs wall-clock |
| **No notification on unlock** | Expected — auto-unlock is silent (V10) |
| **Timer seems wrong** | Expected — fires within 10 s of the window opening; awake-time pauses during sleep |

---

## Security Notes

✅ **Reasonable:**
- As a lockout failsafe during development
- On your own device, with the default 60-min base
- Knowing the doubling schedule limits repeated unauthenticated unlocks

❌ **Understand the risk:**
- An attacker who knows the feature exists could wait for a window
- A window stays open while input keeps arriving (auto-lock's idle window) — an at-keyboard masher can hold a window open within the first base-interval stretch (§2.2 accepted consequence)
- Not suitable for public/shared computers

---

## Code Snippets

### Configure auto-unlock (runtime)
```rust
use handsoff::config::AutoUnlockConfig;

// Enabled with a base interval
state.set_auto_unlock_config(AutoUnlockConfig::Backoff {
    base_interval_secs: std::num::NonZeroU64::new(60).unwrap(),
});

// Disabled
state.set_auto_unlock_config(AutoUnlockConfig::Disabled);
```

### Resolve from env + config (precedence: env > config > default)
```rust
let config = handsoff::config::resolve_auto_unlock(
    Some(cfg.auto_unlock_backoff_enabled()),
    cfg.auto_unlock_base_interval,
);
```

### Window fire (monitoring thread)
```rust
if state.should_auto_unlock() {
    state.trigger_auto_unlock(); // consumes the window: counter advances, no schedule reset
}
```

---

## Test Scenarios

### Quick Smoke Test
```bash
# 1. Start locked with the minimum base interval
HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked

# 2. Wait ~60-70 s of awake time (10 s poll)
# 3. Verify NO notification, menu bar icon flips to unlocked, input works
# 4. Check logs for the WARN window-fired lines
```

### Full Test Suite
```bash
# Run automated tests
cargo test

# Run manual tests
# See docs/TESTING-AUTO-UNLOCK.md for 17 test scenarios
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
        <string>3600</string>  <!-- 60-minute base interval -->
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

**Q: Why isn't the timing exactly as configured?**
A: The monitoring thread sleeps 10 seconds between checks. A window fires within 0–10 seconds after it opens.

**Q: Can I change the base interval while the app is running?**
A: No, you must restart the app with the new environment variable value.

**Q: Does auto-unlock work if my Mac goes to sleep?**
A: The schedule counts awake-time only — sleep pauses it. A locked-then-slept machine unlocks later than wall-clock predicts (§2.4).

**Q: What happens if I lock/unlock/lock quickly?**
A: A passphrase unlock resets the schedule to the base interval. Auto-lock re-engagements do NOT reset it — the counter is keyed to the whole locked stretch, so repeated windows keep doubling (§2.3).

**Q: Is this secure?**
A: It's a **safety feature**, not a security feature — it deliberately trades some security for availability (spec §5).

**Q: Can I disable it?**
A: Yes: `HANDS_OFF_AUTO_UNLOCK=0` or `auto_unlock_mode = "disabled"` in config.toml, then restart the app.

---

## Related Documentation

- **Full Specification:** `specs/auto-unlock-safety-feature.md`
- **Design Review:** `specs/deep-design-review-v2-2026-09.md` (§2 — backoff model)
- **User Guide:** `README.md` (Auto-Unlock Safety Feature section)
- **Manual Testing:** `docs/TESTING-AUTO-UNLOCK.md`
- **Developer Guide:** `DEVELOPER.md` ("Auto-Unlock Safety Feature")

---

**Last Updated:** 2026-10
**Version:** 2.0 (backoff model)
**Status:** Matches current implementation
