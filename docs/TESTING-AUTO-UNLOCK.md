# Auto-Unlock Feature Manual Testing Guide

This document provides comprehensive manual testing procedures for the auto-unlock safety feature implemented in HandsOff.

## Prerequisites

- HandsOff application built and ready to run
- macOS system with accessibility permissions granted
- Terminal access to run the application with environment variables
- Passphrase already configured (or ready to set one)

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
# Run all unit tests
cargo test

# Run only auto-unlock related tests
cargo test auto_unlock

# Run tests with output
cargo test -- --nocapture
```

**Expected Results:**
- All 13+ tests should pass
- Tests complete in ~3 seconds
- No panics or errors

---

## Manual Test Scenarios

### Test 1: Feature Disabled by Default

**Objective:** Verify that auto-unlock does not trigger when the environment variable is not set.

**Steps:**
1. Start the application without setting `HANDS_OFF_AUTO_UNLOCK`:
   ```bash
   cargo run
   ```
2. Check the log output for auto-unlock messages
3. Lock the device using the hotkey (Ctrl+Cmd+Shift+L)
4. Wait 5 minutes
5. Verify the device remains locked

**Expected Results:**
- Log shows: "HANDS_OFF_AUTO_UNLOCK not set. Auto-unlock disabled." (at DEBUG level)
- No auto-unlock thread is started
- Device remains locked indefinitely until manually unlocked
- No auto-unlock notifications appear

**Pass/Fail:** ☐ Pass  ☐ Fail

---

### Test 2: Valid Timeout Configuration (30 seconds)

**Objective:** Verify that auto-unlock triggers correctly with a valid 30-second timeout.

**Steps:**
1. Start the application with a 30-second timeout:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=30 cargo run
   ```
2. Verify the log shows: "Auto-unlock safety feature enabled: 30 seconds"
3. Verify the log shows: "Auto-unlock monitoring thread started"
4. Lock the device using the hotkey (Ctrl+Cmd+Shift+L)
5. Try to type or move mouse (should be blocked)
6. Wait 30 seconds
7. Observe the notification and check logs
8. Verify input is unblocked

**Expected Results:**
- Log shows feature enabled with 30 seconds
- Input is blocked immediately after lock
- After ~30 seconds:
  - Log shows: "Auto-unlock timeout expired - disabling input interception"
  - Log shows: "AUTO-UNLOCK TRIGGERED after 30 seconds"
  - Notification appears: "HandsOff Auto-Unlock Activated"
  - Notification plays sound
  - Menu bar icon changes to unlocked state
  - Keyboard and mouse input work normally

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

### Test 3: Manual Unlock Before Timeout

**Objective:** Verify that manual unlock prevents auto-unlock and resets the timer.

**Steps:**
1. Start with 60-second timeout:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=60 cargo run
   ```
2. Lock the device
3. Wait 20 seconds
4. Manually unlock using the passphrase
5. Wait another 60 seconds
6. Verify no auto-unlock occurs

**Expected Results:**
- Device locks successfully
- After 20 seconds, device is still locked
- Manual unlock works correctly
- After total of 80+ seconds, no auto-unlock notification appears
- Log does not show "AUTO-UNLOCK TRIGGERED"
- Device remains unlocked until manually locked again

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

### Test 4: Multiple Lock/Unlock Cycles

**Objective:** Verify that the auto-unlock timer resets properly across multiple lock cycles.

**Steps:**
1. Start with 45-second timeout:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=45 cargo run
   ```
2. **First cycle:**
   - Lock the device
   - Wait 20 seconds
   - Manually unlock
3. **Second cycle:**
   - Lock the device immediately
   - Wait 20 seconds
   - Manually unlock
4. **Third cycle:**
   - Lock the device
   - Wait full 45+ seconds
   - Observe auto-unlock

**Expected Results:**
- First cycle: No auto-unlock after 20 seconds, manual unlock works
- Second cycle: Timer starts fresh, no auto-unlock after 20 seconds
- Third cycle: Auto-unlock triggers after 45 seconds
- Each lock cycle has independent timing
- Log shows "Lock engaged" for each cycle

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

### Test 5: Minimum Timeout Value (60 seconds)

**Objective:** Verify the minimum accepted timeout value works correctly.

**Steps:**
1. Start with minimum 60-second timeout:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=60 cargo run
   ```
2. Verify log shows: "Auto-unlock safety feature enabled: 60 seconds"
3. Lock the device
4. Wait 60+ seconds
5. Verify auto-unlock triggers

**Expected Results:**
- Application starts successfully
- Feature is enabled with 60 seconds
- Auto-unlock triggers after 60 seconds
- Notification appears

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

### Test 6: Maximum Timeout Value (900 seconds / 15 minutes)

**Objective:** Verify the maximum accepted timeout value is accepted (not practical to wait).

**Steps:**
1. Start with maximum timeout:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=900 cargo run
   ```
2. Check the log output

**Expected Results:**
- Log shows: "Auto-unlock safety feature enabled: 900 seconds"
- Auto-unlock thread starts
- Application runs normally
- (No need to wait 15 minutes for this test)

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

### Test 7: Invalid Value - Below Minimum

**Objective:** Verify that values below 10 seconds are rejected.

**Steps:**
1. Start with value below minimum:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=5 cargo run
   ```
2. Check log output
3. Lock the device
4. Wait 2 minutes
5. Verify device remains locked

**Expected Results:**
- Log shows: "Invalid auto-unlock timeout: 5 (must be 10-3600 or 0). Feature disabled."
- No auto-unlock thread is started
- Device remains locked indefinitely
- No auto-unlock occurs

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

### Test 8: Invalid Value - Above Maximum

**Objective:** Verify that values above 3600 seconds are rejected.

**Steps:**
1. Start with value above maximum:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=5000 cargo run
   ```
2. Check log output
3. Verify feature is disabled

**Expected Results:**
- Log shows: "Invalid auto-unlock timeout: 5000 (must be 10-3600 or 0). Feature disabled."
- Feature is disabled
- No auto-unlock thread starts

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

### Test 9: Invalid Value - Non-Numeric

**Objective:** Verify that non-numeric values are rejected gracefully.

**Steps:**
1. Test with non-numeric value:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=invalid cargo run
   ```
2. Check log output

**Expected Results:**
- Log shows: "Failed to parse HANDS_OFF_AUTO_UNLOCK: ... Feature disabled."
- Application starts normally
- Feature is disabled

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

### Test 10: Explicit Disable with Zero

**Objective:** Verify that setting the value to 0 explicitly disables the feature.

**Steps:**
1. Start with zero value:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=0 cargo run
   ```
2. Check log output

**Expected Results:**
- Log shows: "Auto-unlock disabled (value: 0)"
- Feature is disabled
- No auto-unlock thread starts

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

### Test 11: Passphrase Buffer Cleared on Auto-Unlock

**Objective:** Verify that partial passphrase input is cleared when auto-unlock triggers.

**Steps:**
1. Start with 20-second timeout:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=20 cargo run
   ```
2. Lock the device
3. Type partial passphrase (e.g., first 3 characters)
4. Stop typing
5. Wait for auto-unlock to trigger
6. Lock again
7. Type the same partial passphrase
8. Verify it doesn't unlock (buffer was cleared)

**Expected Results:**
- Partial passphrase is entered
- Auto-unlock triggers after 20 seconds
- Log shows buffer was cleared
- Re-locking and typing partial passphrase doesn't unlock
- Must type full passphrase to unlock manually

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

### Test 12: Notification System

**Objective:** Verify the auto-unlock notification is displayed correctly.

**Steps:**
1. Start with 15-second timeout:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=15 cargo run
   ```
2. Lock the device
3. Wait for auto-unlock
4. Observe the notification

**Expected Results:**
- Notification appears in notification center
- Title: "HandsOff Auto-Unlock Activated"
- Message: "Input interception disabled by safety timeout. You can use your computer normally."
- Notification sound plays
- Notification remains in notification center until dismissed
- Log shows: "Auto-unlock notification delivered"

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

### Test 13: Menu Bar Icon Update

**Objective:** Verify the menu bar icon updates when auto-unlock triggers.

**Steps:**
1. Start with 20-second timeout:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=20 cargo run
   ```
2. Observe menu bar icon (should show unlocked)
3. Lock the device (icon should show locked)
4. Wait for auto-unlock
5. Observe menu bar icon updates to unlocked

**Expected Results:**
- Icon shows unlocked state initially
- Icon shows locked state when locked
- Icon automatically updates to unlocked state when auto-unlock triggers
- Icon state matches actual lock state

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

### Test 14: Logging Coverage

**Objective:** Verify all expected log messages appear.

**Steps:**
1. Start with 25-second timeout:
   ```bash
   RUST_LOG=debug HANDS_OFF_AUTO_UNLOCK=25 cargo run
   ```
2. Lock the device
3. Wait for auto-unlock
4. Review all log output

**Expected Log Messages:**
- ✓ "Auto-unlock safety feature enabled: 25 seconds" (INFO)
- ✓ "Auto-unlock monitoring thread started" (INFO)
- ✓ "Lock engaged at ..." (DEBUG)
- ✓ "Auto-unlock timeout expired - disabling input interception" (WARN)
- ✓ "AUTO-UNLOCK TRIGGERED after XX seconds" (WARN)
- ✓ "Lock disengaged" (DEBUG)
- ✓ "Auto-unlock notification delivered" (INFO)

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

### Test 15: Stress Test - Rapid Lock/Unlock Cycles

**Objective:** Verify the feature handles rapid lock/unlock cycles without crashes.

**Steps:**
1. Start with 30-second timeout:
   ```bash
   HANDS_OFF_AUTO_UNLOCK=30 cargo run
   ```
2. Perform 10 rapid lock/unlock cycles:
   - Lock (Ctrl+Cmd+Shift+L)
   - Immediately unlock (type passphrase)
   - Repeat 10 times
3. Lock once more and let auto-unlock trigger

**Expected Results:**
- No crashes or panics during rapid cycling
- Lock state tracked correctly each time
- Final auto-unlock works correctly
- No memory leaks (check Activity Monitor)
- Log shows clean lock/unlock transitions

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

## Edge Cases and Error Conditions

### Test 16: System Sleep During Lock

**Objective:** Verify behavior when system sleeps while locked.

**Steps:**
1. Start with 120-second timeout
2. Lock the device
3. Put system to sleep manually
4. Wake system before timeout expires
5. Observe behavior

**Expected Results:**
- Timer continues counting after wake
- Auto-unlock should trigger based on elapsed time
- No crashes or unexpected behavior

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

---

### Test 17: High CPU Load

**Objective:** Verify auto-unlock thread continues working under high system load.

**Steps:**
1. Start with 30-second timeout
2. Lock the device
3. Start a CPU-intensive task (e.g., compile a large project)
4. Wait for auto-unlock

**Expected Results:**
- Auto-unlock triggers despite high CPU usage
- Timing may be slightly delayed but within acceptable range
- No crashes or thread starvation

**Pass/Fail:** ☐ Pass  ☐ Fail

**Notes:**
_________________________________

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
# Normal startup (feature disabled)
cargo run

# Enable with 30 seconds
HANDS_OFF_AUTO_UNLOCK=30 cargo run

# Enable with debug logging
RUST_LOG=debug HANDS_OFF_AUTO_UNLOCK=30 cargo run

# Run tests
cargo test auto_unlock

# Build for testing
cargo build
```

### Hotkeys
- **Lock:** Ctrl+Cmd+Shift+L
- **Talk Mode (spacebar passthrough):** Ctrl+Cmd+Shift+T
- **Touch ID:** Ctrl+Cmd+Shift+U

### Log Levels
- **DEBUG:** Lock state changes, timer details
- **INFO:** Feature status, notifications
- **WARN:** Auto-unlock triggers, invalid configs
- **ERROR:** System failures

---

## Troubleshooting

### Auto-unlock not triggering
- Check env var is set: `echo $HANDS_OFF_AUTO_UNLOCK`
- Verify value is in valid range (10-3600)
- Check logs for "Auto-unlock monitoring thread started"
- Ensure device was actually locked
- Check system time hasn't changed

### Notification not appearing
- Check Notification Center settings
- Ensure app has notification permissions
- Look for error in logs: "Failed to get notification center"
- Try manually opening Notification Center

### Timer seems inaccurate
- Note: Thread sleeps 10 seconds between checks
- Auto-unlock will trigger within 10 seconds of timeout
- This is expected behavior (not a bug)

---

**End of Manual Testing Guide**
