# Auto-Unlock Feature Manual Testing Guide

This document provides comprehensive manual testing procedures for the auto-unlock safety feature implemented in HandsOff.

> **Model:** auto-unlock is an **exponential backoff schedule**, not a single timeout
> (per `specs/deep-design-review-v2-2026-09.md` §2). The first window opens at the
> **base interval** of **awake time** after the locked stretch begins; the interval
> **doubles** each fired window, capped at 86400 s (24 h). **Only a successful
> passphrase unlock resets the schedule** — auto-lock re-engagements and fired
> windows do NOT reset it. Unlock is **silent** (no notification, V10).

## Prerequisites

- HandsOff application built and ready to run
- macOS system with accessibility permissions granted
- Terminal access to run the application with environment variables
- Passphrase already configured (`cargo run -- --setup` — the app refuses to run without a config)

## Test Environment Setup

Before testing, ensure you can safely test the application:
1. Save all your work in other applications
2. Have a backup way to access your system (SSH, VNC, etc.) in case of issues
3. Keep the terminal window visible to see log output
4. Test in a non-critical environment first

---

## Unit Tests (Automated)

Before manual testing, verify all unit tests pass:

```bash
# Run all unit tests (inline #[cfg(test)] modules)
cargo test

# Run only auto-unlock related tests
cargo test auto_unlock

# Run tests with output
cargo test -- --nocapture
```

**Expected Results:**
- All tests pass (auto-unlock coverage lives in `src/app_state.rs` and `src/config.rs` test modules: interval math, reset rules, window consumption, env-var parsing, resolution precedence)
- No panics or errors

---

## Manual Test Scenarios

**Timing note for all tests:** the auto-unlock thread checks every 10 s
(`AUTO_UNLOCK_CHECK_INTERVAL_SECS`), so a window fires within 0–10 s after it opens.
All intervals count **awake time** (`Instant` pauses during sleep).

### Test 1: Default Behavior (Enabled, 60-Minute Base)

**Objective:** Verify that auto-unlock is enabled by default with the 60-minute base interval when no env var is set.

**Steps:**
1. Start the application without setting `HANDS_OFF_AUTO_UNLOCK`:
   ```bash
   cargo run
   ```
2. Check the log output for auto-unlock messages
3. Lock the device using the hotkey (Ctrl+Cmd+Shift+L)
4. Verify the device remains locked (do NOT wait 60 minutes)

**Expected Results:**
- Log shows: `Auto-unlock backoff enabled: first window at 3600s, doubling up to 86400s` (INFO)
- Log shows: `Auto-unlock backoff monitoring thread started` (INFO)
- Device remains locked (the first window would open after 60 min of awake time — impractical to wait for in this test; use Test 2 to observe an actual firing)

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 2: First Window at the 60-Second Base (Minimum)

**Objective:** Verify that the first window opens after the base interval of awake time.

**Steps:**
1. Start with the minimum base interval:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked
   ```
2. Verify the log shows: `Auto-unlock backoff enabled: first window at 60s, doubling up to 86400s`
3. Verify the log shows: `Auto-unlock backoff monitoring thread started`
4. Verify input is blocked immediately (started locked)
5. Wait ~60–70 seconds without touching input
6. Observe logs and input state

**Expected Results:**
- After ~60 s of awake time (fires within 0–10 s of the window opening due to the 10 s poll):
  - Log shows: `Auto-unlock window opened - releasing input (unauthenticated)` (WARN)
  - Log shows: `AUTO-UNLOCK WINDOW FIRED after Ns awake-time` (WARN, N ≈ 60–70)
  - Log shows: `Auto-unlock backoff advanced: next window opens after 120s` (INFO)
  - Log shows: `Input unlocked due to auto-unlock window` (INFO)
  - **No notification appears** (silent unlock, V10)
  - Menu bar icon changes to unlocked state
  - Keyboard and mouse input work normally

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 3: Successful Passphrase Unlock Resets the Schedule

**Objective:** Verify that only a successful passphrase unlock resets the backoff counter to the base interval.

**Steps:**
1. Start with the minimum base interval:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked
   ```
2. Wait 30 seconds (locked)
3. Unlock using the passphrase
4. Lock again immediately
5. Wait 30 seconds
6. Verify still locked
7. Wait another 30+ seconds → window opens

**Expected Results:**
- First stretch: still locked after 30 s (below base)
- Passphrase unlock works and resets the schedule
- Second stretch: window opens after a full 60 s of awake time from the new lock — NOT sooner because of the earlier wait
- Log after re-lock (DEBUG): `Locked stretch started; auto-unlock window 0 opens after 60s`

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 4: Backoff Doubling Across Auto-Lock Re-Engagements

**Objective:** Verify the linchpin rule (§2.3): re-locks do NOT reset the counter; only passphrase unlocks (or an explicit tray Reset) do.

**Steps:**
1. Start with the minimum base interval and the shortest auto-lock:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=60 HANDS_OFF_AUTO_LOCK=20 cargo run -- --locked
   ```
2. When the first window opens (~60 s), do NOT unlock — leave the machine idle
3. Wait for auto-lock to re-engage (after 20 s of idle in the open window)
4. Wait for the next window
5. Observe the interval

**Expected Results:**
- After the first window fires and auto-lock re-engages, the next window opens after **120 s** of awake time in the (continuous) locked stretch — doubled, not reset
- Each subsequent window doubles again (240 s, 480 s, …)
- Log shows: `Auto-unlock backoff advanced: next window opens after 120s` after each fire

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 5: Minimum Value (60) Accepted

**Objective:** Verify the minimum accepted base interval works (covered in Test 2; repeat standalone).

**Steps:**
1. `HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked`
2. Check the startup log

**Expected Results:**
- Log shows: `Auto-unlock backoff enabled: first window at 60s, doubling up to 86400s`
- Application starts and locks normally

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 6: Maximum Value (86400) Accepted

**Objective:** Verify the maximum base interval value is accepted (24 h — the window ceiling; not practical to wait).

**Steps:**
1. `HANDS_OFF_AUTO_UNLOCK=86400 cargo run`
2. Check the log output

**Expected Results:**
- Log shows: `Auto-unlock backoff enabled: first window at 86400s, doubling up to 86400s`
- Auto-unlock monitoring thread starts
- Application runs normally

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 7: Invalid Value — Below Minimum

**Objective:** Verify that values below 60 seconds are rejected with a warning and fall back to the config file value (NOT silently disabled).

**Steps:**
1. Start with a value below the minimum:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=5 cargo run
   ```
2. Check log output
3. Verify the effective base interval from the log line

**Expected Results:**
- Log shows: `Invalid auto-unlock base interval: 5 (must be 60-86400 or 0). Ignoring environment variable.` (WARN)
- The env var is ignored; the config file value applies (default: enabled, base 3600 s) — the log line shows the actual base in effect
- Auto-unlock remains ENABLED at the fallback base — an invalid value never disables the failsafe

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 8: Invalid Value — Above Maximum

**Objective:** Verify that values above 86400 seconds are rejected.

**Steps:**
1. `HANDS_OFF_AUTO_UNLOCK=90000 cargo run`
2. Check log output

**Expected Results:**
- Log shows: `Invalid auto-unlock base interval: 90000 (must be 60-86400 or 0). Ignoring environment variable.` (WARN)
- Config file / default value applies (see Test 7)

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 9: Invalid Value — Non-Numeric

**Objective:** Verify that non-numeric values are rejected gracefully.

**Steps:**
1. `HANDS_OFF_AUTO_UNLOCK=invalid cargo run`
2. Check log output

**Expected Results:**
- Log shows: `Failed to parse HANDS_OFF_AUTO_UNLOCK: ... Ignoring environment variable.` (WARN)
- Application starts normally with the config file / default value

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 10: Explicit Disable with Zero

**Objective:** Verify that setting the value to 0 explicitly disables the feature.

**Steps:**
1. `HANDS_OFF_AUTO_UNLOCK=0 cargo run`
2. Check log output
3. Lock the device and wait past any plausible interval

**Expected Results:**
- Log shows: `Auto-unlock disabled via HANDS_OFF_AUTO_UNLOCK=0` (INFO)
- No auto-unlock monitoring activity; device stays locked until passphrase unlock
- (Recovery: SSH kill or reboot — see `DEVELOPER.md` troubleshooting)

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 11: Passphrase Buffer Cleared on Window Fire

**Objective:** Verify that partial passphrase input is cleared when a window fires.

**Steps:**
1. `HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked`
2. Lock is active from start; type a partial passphrase (a few keys)
3. Stop typing and wait for the window to fire
4. When auto-lock re-engages, type the same partial sequence again
5. Verify it does not unlock

**Expected Results:**
- Window fires; buffer is cleared with the unlock
- Re-locked partial input does not unlock — a fresh full sequence is required

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 12: Silent Unlock (No Notification)

**Objective:** Verify that the window firing produces NO notification (V10).

**Steps:**
1. `HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked`
2. Wait for the window to fire
3. Observe Notification Center and the menu bar

**Expected Results:**
- **No notification appears** — unlock is silent by design (V10)
- Menu bar icon updates to unlocked state
- Input works immediately

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 13: Menu Bar Icon Update

**Objective:** Verify the menu bar icon tracks the actual lock state.

**Steps:**
1. `HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked`
2. Observe the menu bar icon (locked state at start)
3. Wait for the window to fire
4. Observe the icon

**Expected Results:**
- Icon shows locked state initially
- Icon automatically updates to unlocked state when the window fires
- Icon state matches actual lock state

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 14: Logging Coverage

**Objective:** Verify all expected log messages appear.

**Steps:**
1. `RUST_LOG=debug HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked`
2. Wait for the window to fire
3. Review all log output

**Expected Log Messages:**
- ✓ `Auto-unlock backoff enabled: first window at 60s, doubling up to 86400s` (INFO)
- ✓ `Auto-unlock backoff monitoring thread started` (INFO)
- ✓ `Locked stretch started; auto-unlock window 0 opens after 60s` (DEBUG)
- ✓ `Auto-unlock window opened - releasing input (unauthenticated)` (WARN)
- ✓ `AUTO-UNLOCK WINDOW FIRED after Ns awake-time` (WARN)
- ✓ `Auto-unlock backoff advanced: next window opens after 120s` (INFO)
- ✓ `Input unlocked due to auto-unlock window` (INFO)

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 15: Stress Test — Rapid Lock/Unlock Cycles

**Objective:** Verify the feature handles rapid lock/unlock cycles without crashes.

**Steps:**
1. `HANDS_OFF_AUTO_UNLOCK=60 HANDS_OFF_AUTO_LOCK=20 cargo run -- --locked`
2. Perform 10 rapid lock/unlock cycles:
   - Unlock with the passphrase
   - Wait for auto-lock (20 s idle) to re-lock
   - Repeat 10 times
3. Then wait out a full window

**Expected Results:**
- No crashes or panics during rapid cycling
- Lock state tracked correctly each time
- Because every unlock in the cycle is a *passphrase* unlock, each resets the schedule — the final window opens at the base interval, not doubled
- No memory leaks (check Activity Monitor)
- Log shows clean lock/unlock transitions

**Pass/Fail:** ☐ Pass  ☐ Fail

---

## Edge Cases and Error Conditions

### Test 16: System Sleep During Lock

**Objective:** Verify awake-time semantics (§2.4).

**Steps:**
1. `HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked`
2. Lock the device
3. Put the system to sleep for several hours
4. Wake the system and observe

**Expected Results:**
- The schedule counts **awake time only**: after wake, the window opens 60 s of *awake* time after the stretch began — NOT immediately because wall-clock time passed during sleep
- No crashes or unexpected immediate unlock

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 17: High CPU Load

**Objective:** Verify the auto-unlock thread continues working under high system load.

**Steps:**
1. `HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked`
2. Lock the device
3. Start a CPU-intensive task (e.g., compile a large project)
4. Wait for the window

**Expected Results:**
- Window fires despite high CPU usage
- Timing may be slightly delayed but within acceptable range (10 s poll)
- No crashes or thread starvation

**Pass/Fail:** ☐ Pass  ☐ Fail

---

## Test Summary

**Date:** _______________
**Tester:** _______________
**Build Version:** _______________

**Total Tests:** 17
**Passed:** _____
**Failed:** _____
**Blocked:** _____

### Critical Issues Found:
_________________________________
_________________________________
_________________________________

### Notes:
_________________________________
_________________________________
_________________________________

### Sign-off:
- [ ] All critical tests passed
- [ ] All documentation is accurate
- [ ] Feature is ready for production use

**Signature:** _______________ **Date:** _______________

---

## Quick Reference

### Common Commands

```bash
# Normal startup (auto-unlock enabled by default, base 3600 s)
cargo run

# Shortest useful base interval for testing (60 s)
HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked

# With debug logging
RUST_LOG=debug HANDS_OFF_AUTO_UNLOCK=60 cargo run -- --locked

# Explicitly disabled
HANDS_OFF_AUTO_UNLOCK=0 cargo run

# Run tests
cargo test auto_unlock

# Build for testing
cargo build
```

### Hotkeys
- **Lock:** Ctrl+Cmd+Shift+L
- **Talk Mode (spacebar passthrough):** Ctrl+Cmd+Shift+T

### Log Levels
- **DEBUG:** Lock-state transitions, window anchors
- **INFO:** Feature status, backoff advances
- **WARN:** Window fired, invalid configs
- **ERROR:** System failures

---

## Troubleshooting

### Auto-unlock not triggering
- Check the startup log for `Auto-unlock backoff enabled: first window at Ns...`
  - If absent: the feature is disabled (`HANDS_OFF_AUTO_UNLOCK=0` or `auto_unlock_mode = "disabled"` in config)
- Verify the device was actually locked (check menu bar icon)
- Remember the base interval counts **awake time** — a machine that slept most of the interval will not unlock on wall-clock schedule
- The thread polls every 10 s; the window fires within 0–10 s after it opens

### No notification on unlock
- **Expected behavior** — auto-unlock is silent by design (V10). Check the menu bar icon and logs instead.

### Timer seems inaccurate
- Two expected effects:
  - The thread sleeps 10 s between checks → fires 0–10 s late
  - Sleep/wake pauses the clock (`Instant` awake-time semantics)

---

**End of Manual Testing Guide**
